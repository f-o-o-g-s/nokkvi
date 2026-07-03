//! Playlist editor view — `impl PlaylistEditorState { fn view }`.
//!
//! Renders the playlist-being-edited's OWN track buffer in the split-view left
//! pane while editing, fully decoupled from the live play queue. The per-row
//! song-list composition (columns, drag, search) is delegated to the shared
//! [`crate::views::song_list_pane`] renderer — the same implementation the
//! queue uses — but with the now-playing highlight switched OFF (the editor has
//! no "now playing" concept) and an editor-specific 2-entry context menu
//! (Get Info / Remove from playlist) instead of the queue's 11-entry menu.
//!
//! The edit-bar header (eyebrow + name input + comment input + public toggle +
//! save/discard) mirrors the queue's read-only "Playing From" banner chrome so
//! the two surfaces look cohesive. The metadata-edit messages route to
//! [`EditorMessage`] variants; the discard control reuses the existing
//! [`SplitViewMessage::ExitEditMode`] path mapped to the root [`Message`].
//!
//! Root-widget stability: every branch returns a `Column` (via
//! `base_slot_list_*`) so the edit-bar `text_input` focus survives re-renders.

use iced::{
    Alignment, Element, Length,
    widget::{Space, column, container, mouse_area, row, svg},
};

use super::EditorViewData;
use crate::{
    app_message::EditorMessage,
    state::PlaylistEditorState,
    views::{
        queue::QueueContextEntry,
        song_list_pane::{SongListPaneParams, SongListRowEvent, song_list_pane},
    },
    widgets::{self, hover_overlay::HoverOverlay},
};

/// Height of the editor's edit-bar header (eyebrow over the name + comment
/// inputs, with the public/save/discard actions). Sized to sit comfortably
/// above the shared `song_list_pane` rows.
const EDIT_BAR_H: f32 = 60.0;

/// Total slot-list chrome for the editor view.
///
/// The editor renders its [edit bar](PlaylistEditorState::edit_bar) **in place
/// of** the usual `view_header`, so the chrome is built from
/// [`chrome_height_without_view_header`] (nav bar + player bar + any top-bar
/// strip, but *not* the view header) plus the edit bar (`EDIT_BAR_H`) and its
/// 1 px separator, plus the select-all band when the multi-select column is on.
///
/// Using `chrome_height_with_header(false)` here would count the 51 px
/// `view_header_chrome()` for a header the editor never draws, under-budgeting
/// the `Length::Fill` slot rect and leaving a blank, placeholder-less band at
/// the bottom of the list.
///
/// [`chrome_height_without_view_header`]: crate::widgets::slot_list::chrome_height_without_view_header
pub(crate) fn editor_chrome_height(select_header_visible: bool) -> f32 {
    use crate::widgets::slot_list::{SELECT_HEADER_HEIGHT, chrome_height_without_view_header};

    let mut chrome = chrome_height_without_view_header() + EDIT_BAR_H + 1.0;
    if select_header_visible {
        chrome += SELECT_HEADER_HEIGHT;
    }
    chrome
}

impl PlaylistEditorState {
    /// Build the editor view: edit-bar header + the editor buffer rendered
    /// through the shared `song_list_pane`.
    pub(crate) fn view<'a>(&'a self, data: EditorViewData<'a>) -> Element<'a, EditorMessage> {
        use crate::widgets::{
            base_slot_list_layout::{BaseSlotListLayoutConfig, single_artwork_panel_with_menu},
            slot_list::{
                SlotListConfig, compose_header_with_select, slot_list_background_container,
            },
        };

        // --- Edit-bar header (mirrors the queue edit bar's chrome) ---
        let edit_bar = self.edit_bar(&data);
        let sep = crate::theme::horizontal_separator(1.0);
        let header: Element<'a, EditorMessage> = column![edit_bar, sep].into();

        // Tri-state "select all" header bar when the multi-select column is on.
        let filtered_count = data.songs.len();
        let header = compose_header_with_select(
            self.columns.select,
            self.common.select_all_state(filtered_count),
            EditorMessage::SlotList(crate::widgets::SlotListPageMessage::SelectAllToggle),
            header,
        );

        // Chrome height for the slot-count math. The edit bar stands in for the
        // view header (the editor renders no `view_header`), so this excludes
        // `view_header_chrome()`; including it over-reserved 51 px and left a
        // blank, placeholder-less band at the bottom of the list.
        let chrome_height = editor_chrome_height(self.columns.select);

        let layout_config = BaseSlotListLayoutConfig {
            window_width: data.window_width,
            window_height: data.window_height,
            show_artwork_column: true,
            slot_list_chrome: chrome_height,
            elevated: false,
        };

        // Empty state: keep the same root widget type as the populated path
        // (CLAUDE.md gotcha — protects the edit-bar `text_input` focus).
        if data.songs.is_empty() {
            // A genuinely-empty playlist (no active search) is still a valid
            // cross-pane-drag drop target: render the empty state through the
            // hover-capable helper so the pane publishes a
            // `HoveredSlot::Empty { items_len: 0 }`. `compute_editor_drop_slot`
            // then accepts it (`0 == buffer len`) and resolves an insert-at-0
            // drop. Without this, the empty editor renders no hover-emitting
            // slot widget, so a drop silently cancels (the populated path's
            // trailing empty slots are what normally provide this target).
            //
            // The "no search matches" case (total_count > 0) keeps the plain,
            // non-droppable empty state — dropping into a filtered-empty view
            // has no unambiguous insert position.
            if data.total_count == 0 {
                let hovered = crate::widgets::HoveredSlot::Empty {
                    slot_index: 0,
                    items_len: 0,
                };
                return widgets::base_slot_list_empty_state_with_hover(
                    header,
                    "This playlist is empty.",
                    &layout_config,
                    EditorMessage::SlotList(crate::widgets::SlotListPageMessage::HoverEnterSlot(
                        hovered,
                    )),
                    EditorMessage::SlotList(crate::widgets::SlotListPageMessage::HoverExitSlot(
                        hovered,
                    )),
                );
            }
            return widgets::base_slot_list_empty_state(
                header,
                "No songs match your search.",
                &layout_config,
            );
        }

        let vertical_artwork_chrome =
            crate::widgets::base_slot_list_layout::vertical_artwork_chrome(&layout_config);
        let config = SlotListConfig::with_dynamic_slots(
            data.window_height,
            chrome_height + vertical_artwork_chrome,
        )
        .with_modifiers(data.modifiers);

        let songs = data.songs.as_ref();
        let album_art = data.album_art;
        let large_artwork = data.large_artwork;
        let open_menu = data.open_menu;
        let columns = self.columns;

        // Render the editor's rows through the shared pane. Now-playing OFF
        // (`current_playing_*` = None) — the editor never highlights a row.
        // `sort_mode` is `None` (no applied sort): the editor has no sort UI,
        // so the plays/genre auto-show gates stay inert.
        let slot_list_content = song_list_pane(
            SongListPaneParams {
                slot_list: &self.common.slot_list,
                songs,
                list_config: &config,
                drop_indicator_slot: data.drop_indicator_slot,
                columns,
                sort_mode: None,
                album_art,
                current_playing_song_id: None,
                current_playing_entry_id: None,
                stable_viewport: true,
            },
            // Map the neutral row vocabulary to editor messages. Slot routing
            // (selection/navigation/scroll/hover) is fully functional. Drag is
            // emitted (handler lands in Phase 4). Title-click and the
            // nav/rating/love affordances route to ContextMenuAction /
            // currently-no-op variants — the editor surfaces metadata edits and
            // removal, not library navigation or per-row scrobble mutations.
            |e| match e {
                SongListRowEvent::Slot(m) => EditorMessage::SlotList(m),
                SongListRowEvent::Drag(d) => EditorMessage::DragReorder(d),
                SongListRowEvent::TitleClick(i) => {
                    EditorMessage::ContextMenuAction(i, QueueContextEntry::GetInfo)
                }
                // Navigation away from the editor is not offered while editing;
                // map to a no-op metadata action so the row stays inert.
                SongListRowEvent::NavArtist(_)
                | SongListRowEvent::NavAlbum(_)
                | SongListRowEvent::NavGenre(_) => {
                    EditorMessage::ContextMenuAction(0, QueueContextEntry::GetInfo)
                }
                SongListRowEvent::SetRating(i, _) => {
                    EditorMessage::ContextMenuAction(i, QueueContextEntry::GetInfo)
                }
                SongListRowEvent::ToggleLove(i) => {
                    EditorMessage::ContextMenuAction(i, QueueContextEntry::GetInfo)
                }
            },
            // Editor-specific context menu: Get Info + Remove from playlist.
            move |slot_button, item_idx| {
                use crate::widgets::context_menu::{
                    context_menu, menu_button, menu_separator, open_state_for,
                };
                // Reuse `QueueContextEntry` variants for the two editor actions
                // (GetInfo / RemoveFromQueue) so the shared menu chrome applies;
                // the editor's handler interprets RemoveFromQueue as "remove
                // from playlist" (Phase 4).
                let entries = vec![
                    QueueContextEntry::GetInfo,
                    QueueContextEntry::Separator,
                    QueueContextEntry::RemoveFromQueue,
                ];
                let cm_id = crate::app_message::ContextMenuId::EditorRow(item_idx);
                let (cm_open, cm_position) = open_state_for(open_menu, &cm_id);
                let cm_id_for_msg = cm_id.clone();
                context_menu(
                    slot_button,
                    entries,
                    move |entry, _length| match entry {
                        QueueContextEntry::GetInfo => menu_button(
                            Some("assets/icons/info.svg"),
                            "Get Info",
                            EditorMessage::ContextMenuAction(item_idx, QueueContextEntry::GetInfo),
                        ),
                        QueueContextEntry::RemoveFromQueue => menu_button(
                            Some("assets/icons/trash-2.svg"),
                            "Remove from Playlist",
                            EditorMessage::RemoveAt(item_idx),
                        ),
                        QueueContextEntry::Separator => menu_separator(),
                        // The editor menu only uses the three entries above.
                        _ => menu_separator(),
                    },
                    cm_open,
                    cm_position,
                    move |position| match position {
                        Some(p) => EditorMessage::SetOpenMenu(Some(
                            crate::app_message::OpenMenu::Context {
                                id: cm_id_for_msg.clone(),
                                position: p,
                            },
                        )),
                        None => EditorMessage::SetOpenMenu(None),
                    },
                )
                .into()
            },
        );
        let slot_list_content = slot_list_background_container(slot_list_content);

        // Artwork panel: center on the focused/centered editor row, like the
        // queue does when not playing. No refresh / artwork context menu in the
        // editor (no open-menu wiring needed for it).
        let center_handle: Option<&iced::widget::image::Handle> = self
            .common
            .slot_list
            .get_center_item_index(songs.len())
            .and_then(|idx| songs.get(idx))
            .and_then(|song| {
                large_artwork
                    .get(&song.album_id)
                    .or_else(|| album_art.get(&song.album_id))
            });
        let artwork_content = Some(single_artwork_panel_with_menu::<EditorMessage>(
            center_handle,
            Vec::new(),
            false,
            None,
            // No artwork context menu in the editor; the open-change closure is
            // required by the helper but only ever asked to close.
            |_p: Option<iced::Point>| EditorMessage::SetOpenMenu(None),
        ));

        crate::widgets::base_slot_list_layout::base_slot_list_layout_with_handle(
            &layout_config,
            header,
            slot_list_content,
            artwork_content,
            None::<fn(crate::widgets::artwork_split_handle::DragEvent) -> EditorMessage>,
            None::<fn(crate::widgets::artwork_split_handle::DragEvent) -> EditorMessage>,
        )
    }

    /// Build the edit-bar header — reproduces the queue view's edit bar:
    /// eyebrow + name input + comment input (left), public toggle + save +
    /// discard (right), accent stripe + faint wash, over a fixed-height band.
    fn edit_bar<'a>(&'a self, data: &EditorViewData<'a>) -> Element<'a, EditorMessage> {
        let accent = crate::theme::accent();

        // Eyebrow — adopts the dirty indicator: append a bullet when dirty so
        // the unsaved state reads at a glance (queue used the same wording in
        // its window-title/dirty flag).
        let eyebrow_label = if data.dirty {
            "EDITING PLAYLIST  •  UNSAVED"
        } else {
            "EDITING PLAYLIST"
        };
        let eyebrow = iced::widget::text(eyebrow_label)
            .font(crate::theme::weighted_ui_font(iced::font::Weight::Semibold))
            .size(9.5)
            .color(accent)
            .wrapping(iced::widget::text::Wrapping::None);

        let name_input = iced::widget::text_input("Playlist name", &data.name)
            .on_input(EditorMessage::NameChanged)
            .font(crate::theme::weighted_ui_font(iced::font::Weight::Bold))
            .size(14)
            .width(Length::Fill)
            .padding([2, 4])
            .style(|_theme, _status| iced::widget::text_input::Style {
                background: iced::Background::Color(iced::Color::TRANSPARENT),
                border: iced::Border {
                    color: crate::theme::bg3(),
                    width: 0.0,
                    radius: crate::theme::ui_border_radius(),
                },
                icon: crate::theme::fg0(),
                placeholder: crate::theme::fg2(),
                value: crate::theme::fg0(),
                selection: crate::theme::selection_color(),
            });

        let comment_input = iced::widget::text_input("Comment", &data.comment)
            .on_input(EditorMessage::CommentChanged)
            .font(crate::theme::ui_font())
            .size(11)
            .width(Length::Fill)
            .padding([2, 4])
            .style(|_theme, _status| iced::widget::text_input::Style {
                background: iced::Background::Color(iced::Color::TRANSPARENT),
                border: iced::Border {
                    color: crate::theme::bg3(),
                    width: 0.0,
                    radius: crate::theme::ui_border_radius(),
                },
                icon: crate::theme::fg2(),
                placeholder: crate::theme::fg2(),
                value: crate::theme::fg2(),
                selection: crate::theme::selection_color(),
            });

        // Icon-only action button (mouse_area + HoverOverlay) — matches the
        // queue edit bar's `icon_btn` so the press scale fires.
        let icon_btn =
            |icon_path: &'static str, msg: EditorMessage| -> Element<'a, EditorMessage> {
                let icon = crate::embedded_svg::svg_widget(icon_path)
                    .width(Length::Fixed(14.0))
                    .height(Length::Fixed(14.0))
                    .style(|_theme, _status| svg::Style {
                        color: Some(crate::theme::fg2()),
                    });
                mouse_area(
                    HoverOverlay::new(
                        container(icon)
                            .padding([4, 6])
                            .style(|_theme| container::Style {
                                background: None,
                                border: iced::Border {
                                    color: iced::Color::TRANSPARENT,
                                    width: 2.0,
                                    radius: crate::theme::ui_border_radius(),
                                },
                                ..Default::default()
                            })
                            .center_y(Length::Shrink),
                    )
                    .border_radius(crate::theme::ui_border_radius()),
                )
                .on_press(msg)
                .interaction(iced::mouse::Interaction::Pointer)
                .into()
            };

        // Public/private toggle — accent when public, muted when private.
        let is_public = data.public;
        let public_toggle: Element<'a, EditorMessage> = {
            let icon_path = if is_public {
                "assets/icons/lock-open.svg"
            } else {
                "assets/icons/lock.svg"
            };
            let tint = if is_public {
                crate::theme::accent()
            } else {
                crate::theme::fg2()
            };
            let tooltip_label = if is_public {
                "Public — click to make private"
            } else {
                "Private — click to make public"
            };
            let icon = crate::embedded_svg::svg_widget(icon_path)
                .width(Length::Fixed(14.0))
                .height(Length::Fixed(14.0))
                .style(move |_theme, _status| svg::Style { color: Some(tint) });
            let trigger = mouse_area(
                HoverOverlay::new(
                    container(icon)
                        .padding([4, 6])
                        .style(|_theme| container::Style {
                            background: None,
                            border: iced::Border {
                                color: iced::Color::TRANSPARENT,
                                width: 2.0,
                                radius: crate::theme::ui_border_radius(),
                            },
                            ..Default::default()
                        })
                        .center_y(Length::Shrink),
                )
                .border_radius(crate::theme::ui_border_radius()),
            )
            .on_press(EditorMessage::PublicToggled(!is_public))
            .interaction(iced::mouse::Interaction::Pointer);
            iced::widget::tooltip(
                trigger,
                container(
                    iced::widget::text(tooltip_label)
                        .size(11.0)
                        .font(crate::theme::ui_font()),
                )
                .padding(4),
                iced::widget::tooltip::Position::Bottom,
            )
            .gap(4)
            .style(crate::theme::container_tooltip)
            .into()
        };

        let save_btn = icon_btn("assets/icons/save.svg", EditorMessage::Save);
        // Discard reuses the existing exit path via `EditorMessage::ExitEditMode`,
        // which the editor handler forwards to `SplitViewMessage::ExitEditMode`.
        let discard_btn = icon_btn("assets/icons/x.svg", EditorMessage::ExitEditMode);

        let stripe = container(Space::new())
            .width(Length::Fixed(3.0))
            .height(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(accent.into()),
                ..Default::default()
            });

        let left = column![eyebrow, name_input, comment_input]
            .spacing(2)
            .width(Length::Fill);
        let actions = row![public_toggle, save_btn, discard_btn]
            .spacing(2)
            .align_y(Alignment::Center);

        let content = container(
            row![left, actions]
                .spacing(10)
                .align_y(Alignment::Center)
                .width(Length::Fill)
                .padding(iced::Padding {
                    top: 0.0,
                    right: 13.0,
                    bottom: 0.0,
                    left: 11.0,
                }),
        )
        .center_y(Length::Fixed(EDIT_BAR_H))
        .width(Length::Fill);

        let wash = crate::theme::accent_wash(crate::theme::bg0_soft(), crate::theme::HEADER_WASH);

        container(
            row![stripe, content]
                .width(Length::Fill)
                .height(Length::Fixed(EDIT_BAR_H)),
        )
        .width(Length::Fill)
        .height(Length::Fixed(EDIT_BAR_H))
        .style(move |_theme| container::Style {
            background: Some(wash.into()),
            ..Default::default()
        })
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: the editor's edit bar renders *in place of* the view header,
    /// so its slot-list chrome must NOT include `view_header_chrome()`. When it
    /// did (via `chrome_height_with_header(false)`), the `Length::Fill` slot rect was
    /// over-reserved by 51 px and the bottom of the list rendered a blank,
    /// placeholder-less band. Pins the editor chrome to exactly
    /// `view_header_chrome()` below the old (buggy) formula.
    #[test]
    fn editor_chrome_excludes_view_header() {
        use crate::{
            theme::THEME_MODE_LOCK,
            widgets::slot_list::{
                SELECT_HEADER_HEIGHT, chrome_height_with_header, view_header_chrome,
            },
        };

        // Serialize against theme-mutating tests so both chrome reads observe
        // the same global nav/strip state (mirrors the player_bar tests).
        let _guard = THEME_MODE_LOCK.lock();

        let with_phantom_header = chrome_height_with_header(false) + EDIT_BAR_H + 1.0;
        let editor = editor_chrome_height(false);
        assert!(
            (with_phantom_header - editor - view_header_chrome()).abs() < 0.01,
            "editor chrome must exclude view_header_chrome(): with_header={with_phantom_header}, \
             editor={editor}, view_header_chrome={}",
            view_header_chrome(),
        );

        // The multi-select column adds exactly SELECT_HEADER_HEIGHT.
        let delta = editor_chrome_height(true) - editor_chrome_height(false);
        assert!(
            (delta - SELECT_HEADER_HEIGHT).abs() < 0.01,
            "multi-select column should add SELECT_HEADER_HEIGHT to editor chrome, got {delta}",
        );
    }
}
