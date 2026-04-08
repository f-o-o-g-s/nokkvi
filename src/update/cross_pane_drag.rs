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

use iced::Task;
use tracing::debug;

use crate::{Nokkvi, app_message::Message, state::CrossPaneDragState, views};

/// Minimum pixel distance before a press becomes a drag
const DRAG_THRESHOLD: f32 = 5.0;

impl Nokkvi {
    /// Mouse pressed — record origin for threshold detection.
    /// Only starts tracking if the browsing panel is open and the press
    /// is in the browser zone (right side of split view).
    pub(crate) fn handle_cross_pane_drag_pressed(&mut self) -> Task<Message> {
        // Only relevant when browsing panel is visible
        let panel = match self.browsing_panel.as_ref() {
            Some(p) => p,
            None => return Task::none(),
        };

        // When Ctrl or Shift is held the user is multi-selecting, not starting
        // a drag.  Bail out to avoid clearing the selection state — the button's
        // on_press (which fires on mouse-up) will handle the click via
        // handle_slot_click with the correct modifiers.
        let mods = self.window.keyboard_modifiers;
        if mods.control() || mods.shift() {
            return Task::none();
        }

        let position = self.last_cursor_position;

        // Check if cursor is in the browser pane (right portion of split).
        // Split is 55% queue / 45% browser, so browser starts at 55% of window width.
        let browser_start_x = self.window.width * 0.55;
        if position.x < browser_start_x {
            return Task::none();
        }

        // Store press origin — we don't activate drag yet until threshold is exceeded.
        self.cross_pane_drag_press_origin = Some(position);

        // --- Compute which item was pressed from cursor Y position ---
        //
        // We CANNOT rely on button's SlotListSetOffset being processed yet because
        // Iced buttons fire on mouse RELEASE, not mouse DOWN. In a click-and-drag
        // flow the button never fires. So we compute the slot from cursor position.
        //
        // Browser pane layout from window top:
        //   nav_bar (32px) + tab_bar (36px) + view_header (48px) = 116px
        //   then slot list slots start (with SLOT_LIST_CONTAINER_PADDING of 10px)
        use crate::widgets::slot_list::{NAV_BAR_HEIGHT, SLOT_SPACING, TAB_BAR_HEIGHT};
        const CHROME_TOP: f32 = NAV_BAR_HEIGHT + TAB_BAR_HEIGHT;

        // Use the same SlotListConfig the view uses:
        // browser_height = window_height - TAB_BAR_HEIGHT, chrome = chrome_height_with_header()
        use crate::widgets::slot_list::{SlotListConfig, chrome_height_with_header};
        let browser_height = self.window.height - TAB_BAR_HEIGHT;
        let config =
            SlotListConfig::with_dynamic_slots(browser_height, chrome_height_with_header());
        let row_height = config.row_height();

        // View header contributes to chrome: chrome_height_with_header() = nav(30)+player(56)+header(48)
        // From the view's perspective, the chrome above the slot list is header(48) only
        // (nav and player are outside the view's coordinate space).
        // But from window coordinates: chrome_above_slot_list = nav(30) + tab(36) + header(48) = 114
        // Plus half the container padding to account for top spacing.
        let slot_list_start_y: f32 = CHROME_TOP + 48.0 + 5.0; // 5.0 ≈ half of SLOT_LIST_CONTAINER_PADDING

        let slot_list_y = position.y - slot_list_start_y;
        if slot_list_y < 0.0 {
            return Task::none();
        }

        let slot_step = row_height + SLOT_SPACING;
        let clicked_slot = (slot_list_y / slot_step).floor() as usize;
        if clicked_slot >= config.slot_count {
            return Task::none();
        }

        // Get the active view's SlotListView and total items.
        // Sync slot_count so slot_to_item_index uses the same layout as rendering.
        let (slot_list, total_items) = match panel.active_view {
            views::BrowsingView::Albums => (
                &mut self.albums_page.common.slot_list,
                self.library.albums.len(),
            ),
            views::BrowsingView::Songs => (
                &mut self.songs_page.common.slot_list,
                self.library.songs.len(),
            ),
            views::BrowsingView::Artists => (
                &mut self.artists_page.common.slot_list,
                self.library.artists.len(),
            ),
            views::BrowsingView::Genres => (
                &mut self.genres_page.common.slot_list,
                self.library.genres.len(),
            ),
            views::BrowsingView::Similar => (
                &mut self.similar_page.common.slot_list,
                self.similar_songs.as_ref().map_or(0, |s| s.songs.len()),
            ),
        };
        slot_list.slot_count = config.slot_count;
        let viewport_offset = slot_list.viewport_offset;

        // Delegate to slot_to_item_index — single source of truth for the
        // effective_center calculation, matching build_slot_list_slots exactly.
        let pressed_index = match slot_list.slot_to_item_index(clicked_slot, total_items) {
            Some(idx) => idx,
            None => return Task::none(),
        };
        self.cross_pane_drag_pressed_item = Some(pressed_index);

        // Determine if the pressed item is within an active multi-selection.
        // If it is, the whole selection batch will be dragged.
        // If it is NOT, clear the selection and drag only this item
        // (same semantics as evaluate_context_menu).
        let selection_count = match panel.active_view {
            views::BrowsingView::Albums => &mut self.albums_page.common,
            views::BrowsingView::Songs => &mut self.songs_page.common,
            views::BrowsingView::Artists => &mut self.artists_page.common,
            views::BrowsingView::Genres => &mut self.genres_page.common,
            views::BrowsingView::Similar => &mut self.similar_page.common,
        };
        let count = if selection_count
            .slot_list
            .selected_indices
            .contains(&pressed_index)
        {
            // Pressed item IS in the selection — drag the whole batch
            selection_count.slot_list.selected_indices.len()
        } else {
            // Pressed item is NOT in the selection — clear and drag single
            selection_count.clear_multi_selection();
            selection_count
                .slot_list
                .selected_indices
                .insert(pressed_index);
            selection_count.slot_list.anchor_index = Some(pressed_index);
            1
        };
        self.cross_pane_drag_selection_count = count;

        debug!(
            " [DRAG] Press on browser pane: slot={}, item_index={}, selection_count={} (viewport={})",
            clicked_slot, pressed_index, count, viewport_offset
        );

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

                    // Use the center index snapshotted at press time.
                    // This is immune to any state changes since the press.
                    self.cross_pane_drag = Some(CrossPaneDragState {
                        origin,
                        cursor: position,
                        center_index: self.cross_pane_drag_pressed_item,
                        drop_target_slot: None,
                        selection_count: self.cross_pane_drag_selection_count,
                    });
                }
            } else if self.cross_pane_drag.is_some() {
                // Active drag — compute drop target slot before mutating drag state
                let queue_end_x = self.window.width * 0.55;
                let target_slot = if position.x < queue_end_x {
                    self.compute_queue_drop_slot(position.y)
                } else {
                    None
                };

                if let Some(drag) = &mut self.cross_pane_drag {
                    drag.cursor = position;
                    drag.drop_target_slot = target_slot;
                }
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

        let position = self.last_cursor_position;

        // Check if cursor is in the queue pane (left portion: x < 55% of window width)
        let queue_end_x = self.window.width * 0.55;
        if position.x >= queue_end_x {
            debug!(" [DRAG] Cross-pane drag cancelled: dropped outside queue zone");
            return Task::none();
        }

        debug!(
            " [DRAG] Cross-pane drop on queue at ({:.0}, {:.0})",
            position.x, position.y
        );

        let queue_insert_index = self.compute_queue_drop_slot(position.y);

        // Store the target position for the update handler to consume
        self.pending_queue_insert_position = queue_insert_index;

        debug!(" [DRAG] Drop target queue index: {:?}", queue_insert_index);

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
            views::BrowsingView::Albums => Message::Albums(views::AlbumsMessage::AddCenterToQueue),
            views::BrowsingView::Songs => Message::Songs(views::SongsMessage::AddCenterToQueue),
            views::BrowsingView::Artists => {
                Message::Artists(views::ArtistsMessage::AddCenterToQueue)
            }
            views::BrowsingView::Genres => Message::Genres(views::GenresMessage::AddCenterToQueue),
            views::BrowsingView::Similar => {
                Message::Similar(views::SimilarMessage::AddCenterToQueue)
            }
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

        let drag = self.cross_pane_drag.as_ref().unwrap();
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
                        self.artwork.album_art.get(&a.id),
                        a.name.clone(),
                        a.artist.clone(),
                        format!("{} songs", a.song_count),
                    )
                }),
                views::BrowsingView::Songs => self.library.songs.get(center_idx).map(|s| {
                    (
                        s.album_id
                            .as_ref()
                            .and_then(|aid| self.artwork.album_art.get(aid)),
                        s.title.clone(),
                        s.artist.clone(),
                        s.album.clone(),
                    )
                }),
                views::BrowsingView::Artists => self.library.artists.get(center_idx).map(|a| {
                    (
                        self.artwork.album_art.get(&a.id),
                        a.name.clone(),
                        format!("{} albums", a.album_count),
                        format!("{} songs", a.song_count),
                    )
                }),
                views::BrowsingView::Genres => self.library.genres.get(center_idx).map(|g| {
                    (
                        self.artwork.genre.mini.get(&g.id),
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
                                .and_then(|aid| self.artwork.album_art.get(aid)),
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
        let style = SlotListSlotStyle::for_slot(true, false, false, false, 1.0);

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
                    radius: 8.0.into(),
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

        let style = SlotListSlotStyle::for_slot(true, false, false, false, 1.0);
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

    /// Compute the queue item index for a given cursor Y coordinate.
    ///
    /// Translates window Y → slot list slot → absolute queue item index.
    /// Returns `Some(index)` when pointing at a valid slot, `None` to append.
    fn compute_queue_drop_slot(&self, cursor_y: f32) -> Option<usize> {
        use crate::widgets::slot_list::{
            EDIT_BAR_HEIGHT, SLOT_SPACING, SlotListConfig, chrome_height_with_header,
        };

        let edit_bar_height: f32 =
            if self.playlist_edit.is_some() || self.active_playlist_info.is_some() {
                EDIT_BAR_HEIGHT
            } else {
                0.0
            };

        // Match the queue view's chrome height (edit bar adds 32px)
        let chrome_height = chrome_height_with_header() + edit_bar_height;
        let config = SlotListConfig::with_dynamic_slots(self.window.height, chrome_height);
        let row_height = config.row_height();

        let slot_list_start_y = crate::widgets::slot_list::queue_slot_list_start_y(edit_bar_height);
        let slot_list_y = cursor_y - slot_list_start_y;

        if slot_list_y < 0.0 {
            return Some(0);
        }

        let slot_step = row_height + SLOT_SPACING;
        let hovered_slot = (slot_list_y / slot_step).floor() as usize;

        if hovered_slot >= config.slot_count {
            None
        } else {
            let total_queue = self.library.queue_songs.len();
            self.queue_page
                .common
                .slot_list
                .slot_to_item_index(hovered_slot, total_queue)
        }
    }
}
