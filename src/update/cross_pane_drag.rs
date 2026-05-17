//! Cross-pane drag-and-drop: browsing panel → queue
//!
//! Handles the application-level drag state machine:
//! 1. Press in browser pane → record origin
//! 2. Move past threshold (5px) → activate drag with browsing panel's centered item
//! 3. Release over queue pane → add to queue (single or batch)
//! 4. Release elsewhere / Escape → cancel
//!
//! Multi-selection aware: if the pressed item is within an active multi-selection,
//! the entire selection set is dragged. Otherwise, the selection is cleared and
//! only the pressed item is dragged (matching context menu semantics).
//!
//! Slot resolution is structural: every slot rendered by
//! `build_slot_list_slots` is wrapped in a `mouse_area` whose `on_enter`
//! publishes a `HoverEnterSlot { slot_index, item_index }` for the slot's
//! row, stored on `SlotListView::hovered_slot`. These handlers read that
//! state directly — no chrome reconstruction, no `cursor_y → slot` math,
//! no stored-vs-inline `slot_count` divergence.

use iced::Task;
use tracing::debug;

use crate::{Nokkvi, app_message::Message, state::CrossPaneDragState, views, widgets::HoveredSlot};

/// Minimum pixel distance before a press becomes a drag
const DRAG_THRESHOLD: f32 = 5.0;

impl Nokkvi {
    /// Look up the currently-hovered slot for the browsing panel's active
    /// view. Returns `None` when no browsing panel is open or the cursor
    /// is not over any slot in the active view (chrome, gaps, queue pane).
    fn browsing_pane_hovered_slot(&self) -> Option<HoveredSlot> {
        let panel = self.browsing_panel.as_ref()?;
        match panel.active_view {
            views::BrowsingView::Albums => self.albums_page.common.slot_list.hovered_slot,
            views::BrowsingView::Songs => self.songs_page.common.slot_list.hovered_slot,
            views::BrowsingView::Artists => self.artists_page.common.slot_list.hovered_slot,
            views::BrowsingView::Genres => self.genres_page.common.slot_list.hovered_slot,
            views::BrowsingView::Similar => self.similar_page.common.slot_list.hovered_slot,
        }
    }

    /// Mouse pressed — record origin for threshold detection.
    /// Only arms the drag state machine when the cursor is over a populated
    /// slot in the browsing panel's active view; chrome, queue pane, and
    /// empty-trailing-slot presses are no-ops.
    pub(crate) fn handle_cross_pane_drag_pressed(&mut self) -> Task<Message> {
        // When Ctrl or Shift is held the user is multi-selecting, not starting
        // a drag.  Bail out to avoid clearing the selection state — the button's
        // on_press (which fires on mouse-up) will handle the click via
        // handle_slot_click with the correct modifiers.
        let mods = self.window.keyboard_modifiers;
        if mods.control() || mods.shift() {
            return Task::none();
        }

        // Press only counts as drag-start when the cursor is over a real
        // browser-pane slot. The per-slot `mouse_area::on_enter` already
        // wrote that into the active view's `hovered_slot`, so:
        //   - cursor over chrome / queue pane / different view → None,
        //     no drag armed.
        //   - cursor over a trailing empty browser slot → no item to drag,
        //     no drag armed.
        //   - cursor over a populated slot → arm with that item index.
        let pressed_index = match self.browsing_pane_hovered_slot() {
            Some(HoveredSlot::Item { item_index, .. }) => item_index,
            Some(HoveredSlot::Empty { .. }) | None => return Task::none(),
        };

        self.cross_pane_drag_press_origin = Some(self.last_cursor_position);
        self.cross_pane_drag_pressed_item = Some(pressed_index);

        // Selection mutation is deferred to `handle_cross_pane_drag_moved`
        // once the drag threshold is exceeded. Mutating on press would
        // race with the per-row checkbox toggle (which also fires on a
        // left-click in the browser pane) — the toggle removes the index,
        // this handler re-inserts it, and the user sees the just-clicked
        // checkbox stay checked while every other selected one disappears.
        //
        // Plain clicks now leave selection untouched here; the row's
        // button (`SlotListSetOffset` on release) and the checkbox's
        // `SlotListSelectionToggle` retain full ownership of the
        // click-to-select behaviour.

        debug!(" [DRAG] Press on browser pane: item_index={pressed_index}");

        Task::none()
    }

    /// Cursor moved while tracking — check threshold and update drag position.
    pub(crate) fn handle_cross_pane_drag_moved(&mut self, position: iced::Point) -> Task<Message> {
        self.last_cursor_position = position;

        // If we have a press origin but no active drag, check threshold
        if let Some(origin) = self.cross_pane_drag_press_origin {
            if self.cross_pane_drag.is_none() {
                let dx = position.x - origin.x;
                let dy = position.y - origin.y;
                let distance = (dx * dx + dy * dy).sqrt();

                if distance >= DRAG_THRESHOLD {
                    // Threshold exceeded — activate drag
                    debug!(
                        " [DRAG] Cross-pane drag started from ({:.0}, {:.0})",
                        origin.x, origin.y
                    );

                    // Determine the drag's selection batch *now* — only
                    // when the gesture is firmly a drag, not on every
                    // press. Press-time mutation conflicted with the
                    // checkbox toggle.
                    //
                    // If the pressed item is already in the multi-
                    // selection, the whole batch is dragged. Otherwise,
                    // clear and select only the pressed item (matching
                    // evaluate_context_menu semantics).
                    let selection_count = if let (Some(panel), Some(pressed_index)) = (
                        self.browsing_panel.as_ref().map(|p| p.active_view),
                        self.cross_pane_drag_pressed_item,
                    ) {
                        let common: &mut crate::widgets::SlotListPageState = match panel {
                            views::BrowsingView::Albums => &mut self.albums_page.common,
                            views::BrowsingView::Songs => &mut self.songs_page.common,
                            views::BrowsingView::Artists => &mut self.artists_page.common,
                            views::BrowsingView::Genres => &mut self.genres_page.common,
                            views::BrowsingView::Similar => &mut self.similar_page.common,
                        };
                        if common.slot_list.selected_indices.contains(&pressed_index) {
                            common.slot_list.selected_indices.len()
                        } else {
                            common.clear_multi_selection();
                            common.slot_list.selected_indices.insert(pressed_index);
                            common.slot_list.anchor_index = Some(pressed_index);
                            1
                        }
                    } else {
                        1
                    };
                    self.cross_pane_drag_selection_count = selection_count;

                    self.cross_pane_drag = Some(CrossPaneDragState {
                        origin,
                        cursor: position,
                        center_index: self.cross_pane_drag_pressed_item,
                        selection_count,
                    });
                }
            } else if let Some(drag) = self.cross_pane_drag.as_mut() {
                // Active drag — drop target is read structurally from the
                // queue's hover state at render / release time, so the only
                // mutation here is to track the cursor for the floating
                // preview overlay.
                drag.cursor = position;
            }
        }

        Task::none()
    }

    /// Mouse released — check if drop is over queue pane, trigger add-to-queue at drop position.
    pub(crate) fn handle_cross_pane_drag_released(&mut self) -> Task<Message> {
        // Clear press state regardless
        self.cross_pane_drag_press_origin = None;
        self.cross_pane_drag_pressed_item = None;
        self.cross_pane_drag_selection_count = 1;

        let drag = match self.cross_pane_drag.take() {
            Some(d) => d,
            None => return Task::none(), // No active drag
        };

        // Drop is over the queue pane iff a queue slot is currently hovered.
        // No cursor-X check needed — the per-slot `mouse_area` only fires
        // `on_enter` when the cursor is inside a slot's rendered bounds.
        let queue_insert_index = match self.compute_queue_drop_slot() {
            Some(idx) => idx,
            None => {
                debug!(" [DRAG] Cross-pane drag cancelled: released outside queue slots");
                return Task::none();
            }
        };

        // Store the target position for the update handler to consume
        self.pending_queue_insert_position = Some(queue_insert_index);

        debug!(" [DRAG] Drop target queue index: {queue_insert_index}");

        // For single-item drags: set selected_offset so AddCenterToQueue picks up
        // the correct item. This is necessary because the button's SlotListSetOffset
        // never fired (Iced buttons fire on release, not press).
        //
        // For multi-selection drags (selection_count > 1): skip this — the
        // selected_indices are already populated from handle_cross_pane_drag_pressed
        // and AddCenterToQueue will resolve the batch from them.
        if drag.selection_count <= 1
            && let (Some(center_idx), Some(panel)) =
                (drag.center_index, self.browsing_panel.as_ref())
        {
            match panel.active_view {
                views::BrowsingView::Albums => {
                    let total = self.library.albums.len();
                    self.albums_page
                        .common
                        .slot_list
                        .set_selected(center_idx, total);
                }
                views::BrowsingView::Songs => {
                    let total = self.library.songs.len();
                    self.songs_page
                        .common
                        .slot_list
                        .set_selected(center_idx, total);
                }
                views::BrowsingView::Artists => {
                    let total = self.library.artists.len();
                    self.artists_page
                        .common
                        .slot_list
                        .set_selected(center_idx, total);
                }
                views::BrowsingView::Genres => {
                    let total = self.library.genres.len();
                    self.genres_page
                        .common
                        .slot_list
                        .set_selected(center_idx, total);
                }
                views::BrowsingView::Similar => {
                    let total = self.similar_songs.as_ref().map_or(0, |s| s.songs.len());
                    self.similar_page
                        .common
                        .slot_list
                        .set_selected(center_idx, total);
                }
            }
        }

        // Dispatch the existing AddCenterToQueue message for the browsing panel's active view.
        // This reuses all existing add-to-queue logic (song resolution, backend calls, etc.)
        // The update handler will check `pending_queue_insert_position` to decide whether
        // to insert at position or append.
        self.dispatch_browser_add_to_queue()
    }

    /// Cancel active drag (Escape key, etc.)
    pub(crate) fn handle_cross_pane_drag_cancel(&mut self) -> Task<Message> {
        if self.cross_pane_drag.is_some() {
            debug!(" [DRAG] Cross-pane drag cancelled by user");
        }
        self.cross_pane_drag = None;
        self.cross_pane_drag_press_origin = None;
        self.cross_pane_drag_pressed_item = None;
        self.cross_pane_drag_selection_count = 1;
        Task::none()
    }

    /// Dispatch the browsing panel's active view's AddCenterToQueue message.
    /// This reuses the exact same code path as the Shift+A hotkey.
    fn dispatch_browser_add_to_queue(&mut self) -> Task<Message> {
        let panel = match self.browsing_panel.as_ref() {
            Some(p) => p,
            None => return Task::none(),
        };

        let msg = match panel.active_view {
            views::BrowsingView::Albums => Message::Albums(views::AlbumsMessage::SlotList(
                crate::widgets::SlotListPageMessage::AddCenterToQueue,
            )),
            views::BrowsingView::Songs => Message::Songs(views::SongsMessage::SlotList(
                crate::widgets::SlotListPageMessage::AddCenterToQueue,
            )),
            views::BrowsingView::Artists => Message::Artists(views::ArtistsMessage::SlotList(
                crate::widgets::SlotListPageMessage::AddCenterToQueue,
            )),
            views::BrowsingView::Genres => Message::Genres(views::GenresMessage::SlotList(
                crate::widgets::SlotListPageMessage::AddCenterToQueue,
            )),
            views::BrowsingView::Similar => Message::Similar(views::SimilarMessage::SlotList(
                crate::widgets::SlotListPageMessage::AddCenterToQueue,
            )),
        };

        Task::done(msg)
    }

    /// Render a full slot list slot replica for the centered browsing panel item.
    /// Uses the same shared helpers (`slot_list_artwork_column`, `slot_list_text_column`,
    /// `slot_list_metadata_column`) as the actual views for pixel-perfect fidelity.
    pub(crate) fn render_drag_slot(&self) -> iced::Element<'_, Message> {
        use iced::{Element, Length};

        let panel = match self.browsing_panel.as_ref() {
            Some(p) => p,
            None => {
                return iced::widget::text("Drag to queue")
                    .color(crate::theme::fg0())
                    .into();
            }
        };

        let Some(drag) = self.cross_pane_drag.as_ref() else {
            return iced::widget::text("Drag to queue")
                .color(crate::theme::fg0())
                .into();
        };
        let selection_count = drag.selection_count;

        let center_idx = match drag.center_index {
            Some(idx) => idx,
            None => {
                return iced::widget::text("Drag to queue")
                    .color(crate::theme::fg0())
                    .into();
            }
        };

        // Extract (artwork, title, subtitle, meta) per view type
        let slot_data: Option<(Option<&iced::widget::image::Handle>, String, String, String)> =
            match panel.active_view {
                views::BrowsingView::Albums => self.library.albums.get(center_idx).map(|a| {
                    (
                        self.artwork.album_art.peek(&a.id),
                        a.name.clone(),
                        a.artist.clone(),
                        format!("{} songs", a.song_count),
                    )
                }),
                views::BrowsingView::Songs => self.library.songs.get(center_idx).map(|s| {
                    (
                        s.album_id
                            .as_ref()
                            .and_then(|aid| self.artwork.album_art.peek(aid)),
                        s.title.clone(),
                        s.artist.clone(),
                        s.album.clone(),
                    )
                }),
                views::BrowsingView::Artists => self.library.artists.get(center_idx).map(|a| {
                    (
                        self.artwork.album_art.peek(&a.id),
                        a.name.clone(),
                        format!("{} albums", a.album_count),
                        format!("{} songs", a.song_count),
                    )
                }),
                views::BrowsingView::Genres => self.library.genres.get(center_idx).map(|g| {
                    (
                        self.artwork.genre.mini_snapshot.get(&g.id),
                        g.name.clone(),
                        format!("{} albums", g.album_count),
                        format!("{} songs", g.song_count),
                    )
                }),
                views::BrowsingView::Similar => self.similar_songs.as_ref().and_then(|state| {
                    state.songs.get(center_idx).map(|s| {
                        (
                            s.album_id
                                .as_ref()
                                .and_then(|aid| self.artwork.album_art.peek(aid)),
                            s.title.clone(),
                            s.artist.clone(),
                            s.album.clone(),
                        )
                    })
                }),
            };

        let slot_content: Element<'_, Message> = match slot_data {
            Some((art, title, subtitle, meta)) => {
                Self::build_drag_preview_row(art, title, subtitle, meta)
            }
            None => iced::widget::text("Drag to queue")
                .color(crate::theme::fg0())
                .into(),
        };

        // Wrap in center-slot styled container
        use crate::widgets::slot_list::SlotListSlotStyle;
        let style = SlotListSlotStyle::for_slot(true, false, false, false, 1.0, 0);

        // For multi-selection drags, overlay a count badge
        if selection_count > 1 {
            let badge = iced::widget::container(
                iced::widget::text(format!("×{selection_count}"))
                    .size(13)
                    .font(crate::theme::ui_font())
                    .color(crate::theme::fg0()),
            )
            .padding(iced::Padding {
                left: 6.0,
                right: 6.0,
                top: 2.0,
                bottom: 2.0,
            })
            .style(|_theme: &iced::Theme| iced::widget::container::Style {
                background: Some(crate::theme::accent().into()),
                border: iced::Border {
                    radius: crate::theme::ui_border_radius(),
                    ..Default::default()
                },
                ..Default::default()
            });

            let stack = iced::widget::Stack::new()
                .push(
                    iced::widget::container(slot_content)
                        .style(move |_theme| style.to_container_style())
                        .width(Length::Fill)
                        .height(Length::Fill),
                )
                .push(
                    iced::widget::container(badge)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .align_x(iced::alignment::Horizontal::Right)
                        .align_y(iced::alignment::Vertical::Top)
                        .padding(iced::Padding {
                            top: 4.0,
                            right: 8.0,
                            ..Default::default()
                        }),
                );

            stack.into()
        } else {
            iced::widget::container(slot_content)
                .style(move |_theme| style.to_container_style())
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        }
    }

    /// Build a drag preview row with artwork, text columns, and metadata.
    /// Shared by all view types in `render_drag_slot`.
    fn build_drag_preview_row<'a>(
        artwork: Option<&'a iced::widget::image::Handle>,
        title: String,
        subtitle: String,
        meta: String,
    ) -> iced::Element<'a, Message> {
        use iced::{Alignment, Length, widget::row};
        use nokkvi_data::utils::scale::calculate_font_size;

        use crate::widgets::slot_list::{
            SlotListSlotStyle, slot_list_artwork_column, slot_list_metadata_column,
            slot_list_text_column,
        };

        let style = SlotListSlotStyle::for_slot(true, false, false, false, 1.0, 0);
        let row_height = 64.0_f32;
        let artwork_size = (row_height - 16.0).max(32.0);
        let title_size = calculate_font_size(16.0, row_height, 1.0);
        let subtitle_size = calculate_font_size(13.0, row_height, 1.0);
        let meta_size = calculate_font_size(12.0, row_height, 1.0);

        row![
            slot_list_artwork_column(artwork, artwork_size, true, false, 1.0),
            slot_list_text_column(
                title,
                None,
                subtitle,
                None,
                title_size,
                subtitle_size,
                style,
                true,
                50
            ),
            slot_list_metadata_column(meta, None, meta_size, style, 22),
        ]
        .spacing(6.0)
        .padding(iced::Padding {
            left: 8.0,
            right: 4.0,
            top: 4.0,
            bottom: 4.0,
        })
        .align_y(Alignment::Center)
        .height(Length::Fill)
        .into()
    }

    /// Resolve the queue insertion index for the current cross-pane drag.
    ///
    /// Reads `queue_page.common.slot_list.hovered_slot`, which the per-slot
    /// `mouse_area::on_enter` / `on_move` writes whenever the cursor is
    /// over a queue slot's rendered bounds. `HoveredSlot::Item { item_index }`
    /// maps to insert-before-that-item; `HoveredSlot::Empty` (cursor on a
    /// trailing empty slot — top-packing tail or queue end) maps to
    /// insert-at-end. `None` means the cursor is not over any queue slot
    /// at all — either it is on chrome, a different pane, or completely
    /// outside the slot list.
    ///
    /// Staleness gate: if `queue_songs.len()` has changed since the hover
    /// was baked (consume auto-advance, SSE library refresh, optimistic
    /// reorder), reject the payload and return `None`. The released drag
    /// then cancels rather than mis-dropping at a position that no longer
    /// matches what the user sees. The iced `mouse_area` diff condition
    /// (`reference-iced/widget/src/mouse_area.rs:320`) only re-fires
    /// `on_enter` on cursor or bounds change, so a queue mutation beneath
    /// a stationary cursor never refreshes the payload organically.
    pub(crate) fn compute_queue_drop_slot(&self) -> Option<usize> {
        let hovered = self.queue_page.common.slot_list.hovered_slot?;
        let current_len = self.library.queue_songs.len();
        let baked_len = match hovered {
            HoveredSlot::Item { items_len, .. } | HoveredSlot::Empty { items_len, .. } => items_len,
        };
        if baked_len != current_len {
            return None;
        }
        match hovered {
            HoveredSlot::Item { item_index, .. } => Some(item_index),
            HoveredSlot::Empty { .. } => Some(current_len),
        }
    }
}
