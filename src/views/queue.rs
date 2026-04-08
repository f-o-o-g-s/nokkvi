//! Queue Page Component
//!
//! Self-contained queue view with slot list navigation showing currently playing queue.
//! Uses message bubbling pattern to communicate global actions to root.

use iced::{
    Alignment, Element, Length, Task,
    widget::{button, column, container, mouse_area, row},
};
// Re-export QueueSortMode from data crate (canonical definition)
pub(crate) use nokkvi_data::types::queue_sort_mode::QueueSortMode;
use nokkvi_data::{backend::queue::QueueSongUIViewData, utils::scale::calculate_font_size};
use tracing::{debug, trace};

use crate::{
    app_message::Message,
    widgets::{self, SlotListPageState, drag_column::DragEvent, hover_overlay::HoverOverlay},
};

/// Queue page local state
#[derive(Debug)]
pub struct QueuePage {
    pub common: SlotListPageState,
    /// Queue uses its own sort mode enum (QueueSortMode), separate from
    /// the library views' SortMode.
    pub queue_sort_mode: QueueSortMode,
}

/// View data passed from root (read-only)
pub struct QueueViewData<'a> {
    pub queue_songs: std::borrow::Cow<'a, [QueueSongUIViewData]>,
    pub album_art: &'a std::collections::HashMap<String, iced::widget::image::Handle>,
    pub large_artwork: &'a std::collections::HashMap<String, iced::widget::image::Handle>,
    pub window_width: f32,
    pub window_height: f32,
    pub scale_factor: f32,
    pub modifiers: iced::keyboard::Modifiers,
    pub current_playing_song_id: Option<String>,
    pub current_playing_queue_index: Option<usize>,
    pub is_playing: bool, // True if playback is active (not stopped/paused)
    pub total_queue_count: usize, // Total count before filtering (for empty state detection)
    pub stable_viewport: bool,
    /// When in edit mode: (playlist_name, is_dirty)
    pub edit_mode_info: Option<(String, bool)>,
    /// Playlist comment when in edit mode
    pub edit_mode_comment: Option<String>,
    /// When a playlist is loaded for playback (not editing): (playlist_id, playlist_name, comment)
    pub playlist_context_info: Option<(String, String, String)>,
}

/// Context menu entries for queue items
#[derive(Debug, Clone, Copy)]
pub enum QueueContextEntry {
    Play,
    PlayNext,
    Separator,
    RemoveFromQueue,
    AddToPlaylist,
    SaveAsPlaylist,
    OpenBrowsingPanel,
    GetInfo,
    ShowInFolder,
    FindSimilar,
    TopSongs,
}

/// Messages for local queue page interactions
#[derive(Debug, Clone)]
pub enum QueueMessage {
    // Slot list navigation
    SlotListNavigateUp,
    SlotListNavigateDown,
    SlotListSetOffset(usize, iced::keyboard::Modifiers),
    SlotListScrollSeek(usize),
    SlotListActivateCenter,
    SlotListClickPlay(usize), // Click non-center to play directly (skip focus)
    FocusCurrentPlaying(usize, bool), // Auto-scroll slot list to center currently playing track (by queue index, flash)

    // Mouse click on star/heart (item_index, value)
    ClickSetRating(usize, usize), // (item_index, rating 1-5)
    ClickToggleStar(usize),       // item_index

    // Context menu
    ContextMenuAction(usize, QueueContextEntry), // (item_index, entry)

    // Drag-and-drop reordering
    DragReorder(DragEvent),

    // View header interactions
    SortModeSelected(QueueSortMode),
    ToggleSortOrder,
    ShuffleQueue,
    SearchQueryChanged(String),

    // Playlist edit mode
    SavePlaylist,
    DiscardEdits,
    PlaylistNameChanged(String),
    PlaylistCommentChanged(String),
    EditPlaylist,      // Enter edit mode for the currently-playing playlist
    QuickSavePlaylist, // Save current queue back to the active playlist without entering edit mode

    // Data loading (moved from root Message enum)
    QueueLoaded(Result<Vec<QueueSongUIViewData>, String>), // queue_songs
    /// Refresh artwork for a specific album (album_id)
    RefreshArtwork(String),
    /// Navigate to a view and set its search query
    NavigateAndSearch(crate::View, String),
}

/// Actions that bubble up to root for global state mutation
#[derive(Debug, Clone)]
pub enum QueueAction {
    PlaySong(usize),                                  // song index in queue
    FocusOnSong(usize, bool), // queue index to scroll to (bubbles up to handler), flash
    SortModeChanged(QueueSortMode), // trigger reload/resort
    SortOrderChanged(bool),   // trigger resort
    ShuffleQueue,             // shuffle queue order
    SearchChanged(String),    // trigger filter
    SetRating(String, usize), // (song_id, rating) - set absolute rating
    ToggleStar(String, bool), // (song_id, new_starred) - toggle starred state
    MoveItem { from: usize, to: usize }, // drag-and-drop reorder (absolute item indices)
    MoveBatch { indices: Vec<usize>, target: usize }, // multi-selection drag reorder
    RemoveFromQueue(Vec<usize>), // remove songs at indices
    PlayNext(Vec<usize>),     // insert songs after currently playing
    ShowToast(String),        // informational toast (e.g. drag disabled reason)
    SaveAsPlaylist,           // open dialog to save queue as new playlist
    OpenBrowsingPanel,        // toggle the library browser panel
    AddToPlaylist(Vec<String>), // song_ids - add to playlist dialog
    SavePlaylist,             // save playlist edits (edit mode)
    DiscardEdits,             // discard edits and exit edit mode
    PlaylistNameChanged(String), // playlist name edited inline
    PlaylistCommentChanged(String), // playlist comment edited inline
    EditPlaylist,             // enter edit mode from playlist context bar
    ShowInfo(usize),          // Open info modal (queue index for full Song lookup)
    ShowInFolder(usize),      // Open containing folder (queue index, path fetched via API)
    RefreshArtwork(String),   // album_id - refresh artwork from server
    FindSimilar(usize),       // Open Find Similar panel for queue index
    TopSongs(usize),          // Open Top Songs panel for queue index
    NavigateAndSearch(crate::View, String), // Navigate to target view and search
    None,
}

impl Default for QueuePage {
    fn default() -> Self {
        Self {
            common: SlotListPageState::new_without_sort_mode(),
            queue_sort_mode: QueueSortMode::Album,
        }
    }
}

impl QueuePage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Update internal state and return actions for root
    pub fn update(
        &mut self,
        message: QueueMessage,
        queue_songs: &[QueueSongUIViewData],
    ) -> (Task<QueueMessage>, QueueAction) {
        let total_items = queue_songs.len();
        match message {
            QueueMessage::SlotListNavigateUp => {
                self.common.handle_navigate_up(total_items);
                (Task::none(), QueueAction::None)
            }
            QueueMessage::SlotListNavigateDown => {
                self.common.handle_navigate_down(total_items);
                (Task::none(), QueueAction::None)
            }
            QueueMessage::SlotListSetOffset(offset, modifiers) => {
                self.common
                    .handle_slot_click(offset, total_items, modifiers);
                (Task::none(), QueueAction::None)
            }
            QueueMessage::SlotListScrollSeek(offset) => {
                self.common.handle_set_offset(offset, total_items);
                (Task::none(), QueueAction::None)
            }
            QueueMessage::SlotListActivateCenter => {
                // Play the centered song
                if let Some(center_idx) = self.common.get_center_item_index(total_items) {
                    self.common.slot_list.flash_center();
                    (Task::none(), QueueAction::PlaySong(center_idx))
                } else {
                    (Task::none(), QueueAction::None)
                }
            }
            QueueMessage::SlotListClickPlay(offset) => {
                self.common.handle_set_offset(offset, total_items);
                self.update(QueueMessage::SlotListActivateCenter, queue_songs)
            }
            QueueMessage::FocusCurrentPlaying(queue_index, flash) => {
                // Auto-scroll slot list to center the currently playing track by queue index
                // Bubble up to handler which has access to queue_songs to find the slot
                trace!(
                    " [QUEUE PAGE] FocusCurrentPlaying({}) called, current_offset={}",
                    queue_index, self.common.slot_list.viewport_offset
                );
                (Task::none(), QueueAction::FocusOnSong(queue_index, flash))
            }
            QueueMessage::NavigateAndSearch(view, query) => {
                (Task::none(), QueueAction::NavigateAndSearch(view, query))
            }
            QueueMessage::SortModeSelected(sort_mode) => {
                self.queue_sort_mode = sort_mode;
                (Task::none(), QueueAction::SortModeChanged(sort_mode))
            }
            QueueMessage::ToggleSortOrder => {
                self.common.sort_ascending = !self.common.sort_ascending;
                (
                    Task::none(),
                    QueueAction::SortOrderChanged(self.common.sort_ascending),
                )
            }
            QueueMessage::ShuffleQueue => {
                // Bubble up to app layer to shuffle the queue
                (Task::none(), QueueAction::ShuffleQueue)
            }
            QueueMessage::SearchQueryChanged(query) => {
                self.common.search_query = query.clone();
                self.common.slot_list.set_offset(0, total_items); // Reset to top on search
                (Task::none(), QueueAction::SearchChanged(query))
            }

            // Data loading messages (handled at root level, no action needed here)
            QueueMessage::QueueLoaded(_) => (Task::none(), QueueAction::None),
            QueueMessage::ClickSetRating(item_index, rating) => {
                if let Some(song) = queue_songs.get(item_index) {
                    let current = song.rating.unwrap_or(0) as usize;
                    let new_rating = if rating == current {
                        rating.saturating_sub(1)
                    } else {
                        rating
                    };
                    (
                        Task::none(),
                        QueueAction::SetRating(song.id.clone(), new_rating),
                    )
                } else {
                    (Task::none(), QueueAction::None)
                }
            }
            QueueMessage::ClickToggleStar(item_index) => {
                if let Some(song) = queue_songs.get(item_index) {
                    (
                        Task::none(),
                        QueueAction::ToggleStar(song.id.clone(), !song.starred),
                    )
                } else {
                    (Task::none(), QueueAction::None)
                }
            }
            QueueMessage::DragReorder(drag_event) => {
                // Drag is allowed in any sort mode, but blocked during active search
                let drag_allowed = self.common.search_query.is_empty();

                match drag_event {
                    DragEvent::Picked { .. } if !drag_allowed => (
                        Task::none(),
                        QueueAction::ShowToast("Clear search to reorder queue".to_string()),
                    ),
                    DragEvent::Dropped {
                        index,
                        target_index,
                    } if drag_allowed => {
                        // Translate slot indices to absolute item indices using the
                        // same effective_center logic that build_slot_list_slots uses for
                        // rendering. Simple `viewport_offset + slot` is wrong because
                        // it doesn't account for the center_slot offset.
                        let from = self.common.slot_list.slot_to_item_index(index, total_items);
                        let to = self
                            .common
                            .slot_list
                            .slot_to_item_index_for_drop(target_index, total_items);
                        debug!(
                            "\u{1f4e6} [QUEUE] Drag reorder: slot {}\u{2192}{} \u{2192} item {:?}\u{2192}{:?} \\
                             (viewport_offset={}, slot_count={}, total={})",
                            index,
                            target_index,
                            from,
                            to,
                            self.common.slot_list.viewport_offset,
                            self.common.slot_list.slot_count,
                            total_items,
                        );

                        // Multi-selection batch drag: if selected_indices has multiple
                        // items and the dragged item is one of them, move the whole batch.
                        let selected = &self.common.slot_list.selected_indices;
                        if selected.len() > 1
                            && from.is_some_and(|f| selected.contains(&f))
                            && let Some(t) = to
                        {
                            let indices: Vec<usize> = selected.iter().copied().collect();
                            self.common.clear_multi_selection();
                            (Task::none(), QueueAction::MoveBatch { indices, target: t })
                        } else {
                            match (from, to) {
                                (Some(f), Some(t)) => {
                                    // Keep highlight on the moved item at its new position
                                    let insert_at = if f < t { t - 1 } else { t };
                                    self.common.slot_list.set_selected(insert_at, total_items);
                                    (Task::none(), QueueAction::MoveItem { from: f, to: t })
                                }
                                _ => {
                                    debug!(
                                        "\u{1f4e6} [QUEUE] Drag dropped on empty slot, ignoring"
                                    );
                                    (Task::none(), QueueAction::None)
                                }
                            }
                        }
                    }
                    DragEvent::Picked { index } if drag_allowed => {
                        // Check if the picked item is part of an active multi-selection.
                        // If yes, preserve the selection (batch drag). If not, highlight
                        // only the dragged item (single drag).
                        if let Some(item_index) =
                            self.common.slot_list.slot_to_item_index(index, total_items)
                            && !self.common.slot_list.selected_indices.contains(&item_index)
                        {
                            self.common.slot_list.set_selected(item_index, total_items);
                        }
                        (Task::none(), QueueAction::None)
                    }
                    _ => (Task::none(), QueueAction::None),
                }
            }
            QueueMessage::ContextMenuAction(clicked_idx, entry) => match entry {
                QueueContextEntry::Play => {
                    self.common.handle_set_offset(clicked_idx, total_items);
                    (Task::none(), QueueAction::PlaySong(clicked_idx))
                }
                QueueContextEntry::RemoveFromQueue | QueueContextEntry::PlayNext => {
                    let target_indices = self.common.evaluate_context_menu(clicked_idx);
                    self.common.clear_multi_selection();

                    match entry {
                        QueueContextEntry::RemoveFromQueue => {
                            (Task::none(), QueueAction::RemoveFromQueue(target_indices))
                        }
                        QueueContextEntry::PlayNext => {
                            (Task::none(), QueueAction::PlayNext(target_indices))
                        }
                        _ => unreachable!(),
                    }
                }
                QueueContextEntry::AddToPlaylist => {
                    let target_indices = self.common.evaluate_context_menu(clicked_idx);
                    self.common.clear_multi_selection();
                    let target_songs: Vec<String> = target_indices
                        .iter()
                        .filter_map(|&idx| queue_songs.get(idx).map(|s| s.id.clone()))
                        .collect();
                    if target_songs.is_empty() {
                        (Task::none(), QueueAction::None)
                    } else {
                        (Task::none(), QueueAction::AddToPlaylist(target_songs))
                    }
                }
                QueueContextEntry::Separator => (Task::none(), QueueAction::None),
                QueueContextEntry::SaveAsPlaylist => (Task::none(), QueueAction::SaveAsPlaylist),
                QueueContextEntry::OpenBrowsingPanel => {
                    (Task::none(), QueueAction::OpenBrowsingPanel)
                }
                QueueContextEntry::GetInfo => (Task::none(), QueueAction::ShowInfo(clicked_idx)),
                QueueContextEntry::ShowInFolder => {
                    (Task::none(), QueueAction::ShowInFolder(clicked_idx))
                }
                QueueContextEntry::FindSimilar => {
                    (Task::none(), QueueAction::FindSimilar(clicked_idx))
                }
                QueueContextEntry::TopSongs => (Task::none(), QueueAction::TopSongs(clicked_idx)),
            },
            QueueMessage::SavePlaylist => (Task::none(), QueueAction::SavePlaylist),
            QueueMessage::DiscardEdits => (Task::none(), QueueAction::DiscardEdits),
            QueueMessage::PlaylistNameChanged(name) => {
                (Task::none(), QueueAction::PlaylistNameChanged(name))
            }
            QueueMessage::PlaylistCommentChanged(comment) => {
                (Task::none(), QueueAction::PlaylistCommentChanged(comment))
            }
            QueueMessage::EditPlaylist => (Task::none(), QueueAction::EditPlaylist),
            QueueMessage::QuickSavePlaylist => (Task::none(), QueueAction::SaveAsPlaylist),
            QueueMessage::RefreshArtwork(album_id) => {
                (Task::none(), QueueAction::RefreshArtwork(album_id))
            }
        }
    }

    /// Build the view
    pub fn view<'a>(&'a self, data: QueueViewData<'a>) -> Element<'a, QueueMessage> {
        use crate::widgets::slot_list::{SlotListConfig, SlotListRowContext};

        // Build ViewHeader using generic component
        const QUEUE_VIEW_OPTIONS: &[QueueSortMode] = &[
            QueueSortMode::Album,
            QueueSortMode::Artist,
            QueueSortMode::Title,
            QueueSortMode::Duration,
            QueueSortMode::Genre,
            QueueSortMode::Rating,
        ];

        let header = widgets::view_header::view_header(
            self.queue_sort_mode,
            QUEUE_VIEW_OPTIONS,
            self.common.sort_ascending,
            &self.common.search_query,
            data.queue_songs.len(),
            data.total_queue_count, // Use total count for header display
            "songs",
            crate::views::QUEUE_SEARCH_ID,
            QueueMessage::SortModeSelected,
            Some(QueueMessage::ToggleSortOrder),
            Some(QueueMessage::ShuffleQueue), // Shuffle button for queue
            None,                             // No refresh button for queue
            data.current_playing_queue_index
                .map(|idx| QueueMessage::FocusCurrentPlaying(idx, true)),
            true, // show_search
            QueueMessage::SearchQueryChanged,
        );

        // Build final header: regular header + optional edit mode bar
        let header: Element<'a, QueueMessage> = if let Some((ref name, _)) = data.edit_mode_info {
            use iced::widget::svg;

            // Pencil-line icon to indicate editing
            let edit_icon = crate::embedded_svg::svg_widget("assets/icons/pencil-line.svg")
                .width(Length::Fixed(14.0))
                .height(Length::Fixed(14.0))
                .style(|_theme, _status| svg::Style {
                    color: Some(crate::theme::accent()),
                });

            let name_input = iced::widget::text_input("Playlist name", name)
                .on_input(QueueMessage::PlaylistNameChanged)
                .font(iced::font::Font {
                    weight: iced::font::Weight::Medium,
                    ..crate::theme::ui_font()
                })
                .size(12)
                .width(Length::FillPortion(3))
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

            // Comment text input — lighter, smaller, visually secondary
            let comment_value = data.edit_mode_comment.as_deref().unwrap_or_default();
            let comment_input = iced::widget::text_input("Comment", comment_value)
                .on_input(QueueMessage::PlaylistCommentChanged)
                .font(crate::theme::ui_font())
                .size(11)
                .width(Length::FillPortion(2))
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

            // Icon-only action button — mouse_area + HoverOverlay(container) so the
            // press scale effect fires (native button captures ButtonPressed first).
            let icon_btn =
                |icon_path: &'static str, msg: QueueMessage| -> Element<'a, QueueMessage> {
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

            let save_btn = icon_btn("assets/icons/save.svg", QueueMessage::SavePlaylist);
            let discard_btn = icon_btn("assets/icons/x.svg", QueueMessage::DiscardEdits);

            let name_comment_col: Element<'a, QueueMessage> =
                iced::widget::column![name_input, comment_input]
                    .spacing(1)
                    .width(Length::Fill)
                    .into();

            let edit_bar = container(
                row![edit_icon, name_comment_col, save_btn, discard_btn,]
                    .spacing(6)
                    .align_y(Alignment::Center)
                    .padding([0, 8])
                    .width(Length::Fill),
            )
            .height(Length::Fixed(44.0))
            .style(|_theme| container::Style {
                background: Some(crate::theme::bg0_soft().into()),
                ..Default::default()
            })
            .width(Length::Fill);

            let sep_bottom: Element<'a, QueueMessage> = crate::theme::horizontal_separator(1.0);
            column![edit_bar, sep_bottom, header].into()
        } else if let Some((ref _playlist_id, ref playlist_name, ref comment)) =
            data.playlist_context_info
        {
            // Read-only playlist context bar (playing a playlist, not editing)
            use iced::widget::svg;

            let playlist_icon = crate::embedded_svg::svg_widget("assets/icons/list-music.svg")
                .width(Length::Fixed(14.0))
                .height(Length::Fixed(14.0))
                .style(|_theme, _status| svg::Style {
                    color: Some(crate::theme::accent()),
                });

            let name_label = iced::widget::text(playlist_name.clone())
                .font(iced::font::Font {
                    weight: iced::font::Weight::Medium,
                    ..crate::theme::ui_font()
                })
                .size(12)
                .color(crate::theme::fg0());

            // Build name + optional comment as a column, constrained to prevent overflow.
            // Without a width constraint, long comments expand to intrinsic text width
            // and push save/edit icons off-screen, cascading layout breakage.
            let name_area: Element<'a, QueueMessage> = if comment.is_empty() {
                container(name_label).width(Length::Fill).clip(true).into()
            } else {
                let comment_label = iced::widget::text(comment.clone())
                    .font(crate::theme::ui_font())
                    .size(10)
                    .color(crate::theme::fg2())
                    .wrapping(iced::widget::text::Wrapping::None);
                container(column![name_label, comment_label].spacing(1))
                    .width(Length::Fill)
                    .clip(true)
                    .into()
            };

            // Save button — quick-saves the current queue back to this playlist
            let save_icon = crate::embedded_svg::svg_widget("assets/icons/save.svg")
                .width(Length::Fixed(14.0))
                .height(Length::Fixed(14.0))
                .style(|_theme, _status| svg::Style {
                    color: Some(crate::theme::fg2()),
                });

            let save_btn: Element<'a, QueueMessage> = mouse_area(
                HoverOverlay::new(
                    container(save_icon)
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
            .on_press(QueueMessage::QuickSavePlaylist)
            .interaction(iced::mouse::Interaction::Pointer)
            .into();

            // Edit button — enters split-view playlist edit mode
            let edit_icon = crate::embedded_svg::svg_widget("assets/icons/pencil-line.svg")
                .width(Length::Fixed(14.0))
                .height(Length::Fixed(14.0))
                .style(|_theme, _status| svg::Style {
                    color: Some(crate::theme::fg2()),
                });

            let edit_btn: Element<'a, QueueMessage> = mouse_area(
                HoverOverlay::new(
                    container(edit_icon)
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
            .on_press(QueueMessage::EditPlaylist)
            .interaction(iced::mouse::Interaction::Pointer)
            .into();

            let playlist_bar = container(
                row![playlist_icon, name_area, save_btn, edit_btn]
                    .spacing(6)
                    .align_y(Alignment::Center)
                    .padding([0, 8])
                    .width(Length::Fill),
            )
            .height(Length::Fixed(32.0))
            .style(|_theme| container::Style {
                background: Some(crate::theme::bg0_soft().into()),
                ..Default::default()
            })
            .width(Length::Fill);

            let sep_bottom: Element<'a, QueueMessage> = crate::theme::horizontal_separator(1.0);
            column![playlist_bar, sep_bottom, header].into()
        } else {
            header
        };

        let header: Element<'a, QueueMessage> = header;

        // Create layout config BEFORE empty checks to route empty states through
        // base_slot_list_layout, preserving the widget tree structure and search focus
        use crate::widgets::base_slot_list_layout::BaseSlotListLayoutConfig;
        let layout_config = BaseSlotListLayoutConfig {
            window_width: data.window_width,
            window_height: data.window_height,
            show_artwork_column: true,
        };

        // If no songs in filtered results, show appropriate message (like albums view)
        if data.queue_songs.is_empty() {
            let message = if data.total_queue_count == 0 {
                "Queue is empty."
            } else {
                "No songs match your search."
            };
            return widgets::base_slot_list_empty_state(header, message, &layout_config);
        }

        // Configure slot list with queue-specific chrome height (with view header now)
        // Edit mode adds a 44px bar + context bar adds 32px bar; account for the tallest so
        // the last slot isn't shorter than the rest.
        use crate::widgets::slot_list::chrome_height_with_header;
        let chrome_height = if data.edit_mode_info.is_some() {
            chrome_height_with_header() + 45.0 // 44px edit bar + 1px separator
        } else if data.playlist_context_info.is_some() {
            chrome_height_with_header() + 33.0 // 32px context bar + 1px separator
        } else {
            chrome_height_with_header()
        };
        let config = SlotListConfig::with_dynamic_slots(data.window_height, chrome_height)
            .with_modifiers(data.modifiers);

        // Capture values needed in closure
        let _scale_factor = data.scale_factor;
        let current_playing_song_id = data.current_playing_song_id;
        let current_playing_queue_index = data.current_playing_queue_index;
        let current_sort_mode = self.queue_sort_mode; // For conditional column/genre display
        let album_art = data.album_art; // Move artwork maps
        let large_artwork = data.large_artwork;
        let queue_songs = data.queue_songs; // Move ownership to extend lifetime
        // Always show the album column to provide context across all sort modes
        let show_album_column = true;

        // Build the render_item closure (shared between drag and non-drag paths)
        let render_item = |song: &QueueSongUIViewData,
                           ctx: SlotListRowContext|
         -> Element<'a, QueueMessage> {
            // Clone all data from song at the start to avoid lifetime issues
            let title = song.title.clone();
            let artist = song.artist.clone();
            let album = song.album.clone();
            let album_id = song.album_id.clone();
            let duration = song.duration.clone();
            let genre = song.genre.clone();
            let starred = song.starred;
            let rating = song.rating.unwrap_or(0).min(5) as usize;

            // Match on both song ID AND queue position (track_number) to
            // disambiguate duplicate tracks sharing the same song ID.
            // Suppressed while ctrl/shift is held (active multi-selection) so
            // users can clearly see which items are selected.
            let is_current = !(ctx.modifiers.shift() || ctx.modifiers.control())
                && current_playing_queue_index
                    .is_some_and(|idx| idx == song.track_number as usize - 1)
                && current_playing_song_id.as_ref() == Some(&song.id);

            // Get centralized slot list slot styling
            use crate::widgets::slot_list::{
                SLOT_LIST_SLOT_PADDING, SlotListSlotStyle, slot_list_index_column, slot_list_text,
            };
            let style = SlotListSlotStyle::for_slot(
                ctx.is_center,
                is_current,
                ctx.is_selected,
                ctx.has_multi_selection,
                ctx.opacity,
                0,
            );

            // Dynamic scaling - match albums view font sizes, apply scale_factor
            // Calculate artwork directly from row_height and apply slot scale_factor
            let base_artwork_size = (ctx.row_height - 16.0).max(32.0);
            let artwork_size = base_artwork_size * ctx.scale_factor;
            let title_size =
                calculate_font_size(16.0, ctx.row_height, ctx.scale_factor) * ctx.scale_factor;
            let subtitle_size =
                calculate_font_size(13.0, ctx.row_height, ctx.scale_factor) * ctx.scale_factor;
            let index_size =
                calculate_font_size(12.0, ctx.row_height, ctx.scale_factor) * ctx.scale_factor;
            let duration_size =
                calculate_font_size(12.0, ctx.row_height, ctx.scale_factor) * ctx.scale_factor;
            let icon_size = (ctx.row_height * 0.3 * ctx.scale_factor).clamp(16.0, 24.0); // Match albums star_size

            // Dynamic column proportions: title gets more space when album/rating columns are hidden
            let show_rating_column = current_sort_mode == QueueSortMode::Rating;
            let title_portion: u16 = if show_rating_column { 35 } else { 40 };

            // Layout: [Index] [Art] [Title/Artist] [Album?] [Rating?] [Duration] [Heart]
            let mut content_row = row![
                // 0. Index number (fixed width)
                slot_list_index_column(ctx.item_index, index_size, style, ctx.opacity),
                // 1. Album Art (fixed width) - match albums pattern
                {
                    use crate::widgets::slot_list::slot_list_artwork_column;
                    slot_list_artwork_column(
                        album_art.get(&album_id),
                        artwork_size,
                        ctx.is_center,
                        is_current,
                        ctx.opacity,
                    )
                },
                // 2. Title + Artist (always simple text column)
                {
                    use crate::widgets::slot_list::slot_list_text_column;
                    let title_click = Some(QueueMessage::ContextMenuAction(
                        ctx.item_index,
                        QueueContextEntry::GetInfo,
                    ));
                    slot_list_text_column(
                        title,
                        title_click,
                        artist.clone(),
                        Some(QueueMessage::NavigateAndSearch(
                            crate::View::Artists,
                            artist,
                        )),
                        title_size,
                        subtitle_size,
                        style,
                        ctx.is_center || is_current,
                        title_portion,
                    )
                },
            ]
            .spacing(6.0)
            .align_y(Alignment::Center);

            // 3. Album column — always shown
            if show_album_column {
                content_row = content_row.push(
                    container({
                        let click_album =
                            QueueMessage::NavigateAndSearch(crate::View::Albums, album.clone());
                        let album_widget: Element<'_, QueueMessage> =
                            crate::widgets::link_text::LinkText::new(album)
                                .size(subtitle_size)
                                .color(style.subtext_color)
                                .hover_color(style.hover_text_color)
                                .font(crate::theme::ui_font())
                                .on_press(Some(click_album))
                                .into();

                        let content: Element<'_, QueueMessage> = if current_sort_mode
                            == QueueSortMode::Genre
                        {
                            let click_genre =
                                QueueMessage::NavigateAndSearch(crate::View::Genres, genre.clone());
                            let genre_font_size =
                                calculate_font_size(10.0, ctx.row_height, ctx.scale_factor)
                                    * ctx.scale_factor;
                            let genre_widget: Element<'_, QueueMessage> =
                                crate::widgets::link_text::LinkText::new(if genre.is_empty() {
                                    "Unknown".to_string()
                                } else {
                                    genre
                                })
                                .size(genre_font_size)
                                .color(style.subtext_color)
                                .hover_color(style.hover_text_color)
                                .font(crate::theme::ui_font())
                                .on_press(Some(click_genre))
                                .into();

                            column![album_widget, genre_widget].spacing(2.0).into()
                        } else {
                            album_widget
                        };
                        content
                    })
                    .width(Length::FillPortion(30))
                    .height(Length::Fill)
                    .clip(true)
                    .align_y(Alignment::Center),
                );
            }

            // 4. Rating column — only shown for Rating sort mode (dedicated column, not inline with title)
            if show_rating_column {
                let star_icon_size =
                    calculate_font_size(14.0, ctx.row_height, ctx.scale_factor) * ctx.scale_factor;
                let idx = ctx.item_index;
                use crate::widgets::slot_list::slot_list_star_rating;
                content_row = content_row.push(slot_list_star_rating(
                    rating,
                    star_icon_size,
                    ctx.is_center,
                    ctx.opacity,
                    Some(15),
                    Some(move |star: usize| QueueMessage::ClickSetRating(idx, star)),
                ));
            }

            // 5. Duration - right aligned
            content_row = content_row.push(
                container(slot_list_text(duration, duration_size, style.subtext_color))
                    .width(Length::FillPortion(10))
                    .align_x(Alignment::End)
                    .align_y(Alignment::Center),
            );

            // 5. Heart Icon - use reusable component, with symmetric padding for centering
            content_row = content_row.push(
                container({
                    use crate::widgets::slot_list::slot_list_favorite_icon;
                    slot_list_favorite_icon(
                        starred,
                        ctx.is_center,
                        is_current,
                        ctx.opacity,
                        icon_size,
                        "heart",
                        Some(QueueMessage::ClickToggleStar(ctx.item_index)),
                    )
                })
                .width(Length::FillPortion(5))
                .padding(iced::Padding {
                    left: 4.0,
                    right: 4.0,
                    ..Default::default()
                })
                .align_x(Alignment::Center)
                .align_y(Alignment::Center),
            );

            let content = content_row
                .padding(iced::Padding {
                    left: SLOT_LIST_SLOT_PADDING,
                    right: 4.0,
                    top: 4.0,
                    bottom: 4.0,
                })
                .height(Length::Fill);

            // Wrap in clickable container
            let clickable = container(content)
                .style(move |_theme| style.to_container_style())
                .width(Length::Fill);

            // Make it interactive
            let slot_button = button(clickable)
                .on_press(if ctx.modifiers.control() || ctx.modifiers.shift() {
                    QueueMessage::SlotListSetOffset(ctx.item_index, ctx.modifiers)
                } else if ctx.is_center {
                    QueueMessage::SlotListActivateCenter
                } else if data.stable_viewport {
                    QueueMessage::SlotListSetOffset(ctx.item_index, ctx.modifiers)
                } else {
                    QueueMessage::SlotListClickPlay(ctx.item_index)
                })
                .style(|_theme, _status| button::Style {
                    background: None,
                    border: iced::Border::default(),
                    ..Default::default()
                })
                .padding(0)
                .width(Length::Fill);

            // Wrap in context menu
            use crate::widgets::context_menu::{context_menu, menu_button, menu_separator};
            let item_idx = ctx.item_index;
            let entries = vec![
                QueueContextEntry::Play,
                QueueContextEntry::PlayNext,
                QueueContextEntry::Separator,
                QueueContextEntry::RemoveFromQueue,
                QueueContextEntry::Separator,
                QueueContextEntry::AddToPlaylist,
                QueueContextEntry::SaveAsPlaylist,
                QueueContextEntry::Separator,
                QueueContextEntry::OpenBrowsingPanel,
                QueueContextEntry::Separator,
                QueueContextEntry::GetInfo,
                QueueContextEntry::ShowInFolder,
                QueueContextEntry::FindSimilar,
                QueueContextEntry::TopSongs,
            ];

            context_menu(slot_button, entries, move |entry, _length| match entry {
                QueueContextEntry::Play => menu_button(
                    Some("assets/icons/circle-play.svg"),
                    "Play",
                    QueueMessage::ContextMenuAction(item_idx, QueueContextEntry::Play),
                ),
                QueueContextEntry::PlayNext => menu_button(
                    Some("assets/icons/list-end.svg"),
                    "Play Next",
                    QueueMessage::ContextMenuAction(item_idx, QueueContextEntry::PlayNext),
                ),
                QueueContextEntry::RemoveFromQueue => menu_button(
                    Some("assets/icons/trash-2.svg"),
                    "Remove from Queue",
                    QueueMessage::ContextMenuAction(item_idx, QueueContextEntry::RemoveFromQueue),
                ),
                QueueContextEntry::Separator => menu_separator(),
                QueueContextEntry::AddToPlaylist => menu_button(
                    Some("assets/icons/list-music.svg"),
                    "Add to Playlist",
                    QueueMessage::ContextMenuAction(item_idx, QueueContextEntry::AddToPlaylist),
                ),
                QueueContextEntry::SaveAsPlaylist => menu_button(
                    Some("assets/icons/list-music.svg"),
                    "Save Queue as Playlist",
                    QueueMessage::ContextMenuAction(item_idx, QueueContextEntry::SaveAsPlaylist),
                ),
                QueueContextEntry::OpenBrowsingPanel => menu_button(
                    Some("assets/icons/panel-right-open.svg"),
                    "Library Browser",
                    QueueMessage::ContextMenuAction(item_idx, QueueContextEntry::OpenBrowsingPanel),
                ),
                QueueContextEntry::GetInfo => menu_button(
                    Some("assets/icons/info.svg"),
                    "Get Info",
                    QueueMessage::ContextMenuAction(item_idx, QueueContextEntry::GetInfo),
                ),
                QueueContextEntry::ShowInFolder => menu_button(
                    Some("assets/icons/folder-open.svg"),
                    "Show in File Manager",
                    QueueMessage::ContextMenuAction(item_idx, QueueContextEntry::ShowInFolder),
                ),
                QueueContextEntry::FindSimilar => menu_button(
                    Some("assets/icons/radar.svg"),
                    "Find Similar",
                    QueueMessage::ContextMenuAction(item_idx, QueueContextEntry::FindSimilar),
                ),
                QueueContextEntry::TopSongs => menu_button(
                    Some("assets/icons/star.svg"),
                    "Top Songs",
                    QueueMessage::ContextMenuAction(item_idx, QueueContextEntry::TopSongs),
                ),
            })
            .into()
        };

        // Build slot list content: always use DragColumn so we detect drag attempts
        // (toast shown if drag is disabled for current sort/search state)
        let slot_list_content = {
            use crate::widgets::slot_list::slot_list_view_with_drag;
            slot_list_view_with_drag(
                &self.common.slot_list,
                &queue_songs,
                &config,
                QueueMessage::SlotListNavigateUp,
                QueueMessage::SlotListNavigateDown,
                {
                    let total = queue_songs.len();
                    move |f| QueueMessage::SlotListScrollSeek((f * total as f32) as usize)
                },
                QueueMessage::DragReorder,
                render_item,
            )
        };

        // Wrap slot list content with standard background (prevents color bleed-through)
        use crate::widgets::slot_list::slot_list_background_container;
        let slot_list_content = slot_list_background_container(slot_list_content);

        let slot_list_content: Element<'a, QueueMessage> = slot_list_content;

        // Get large artwork: prioritize currently playing song, fallback to centered song
        let center_artwork_handle: Option<&iced::widget::image::Handle> = if data.is_playing {
            current_playing_song_id
                .as_ref()
                .and_then(|song_id| queue_songs.iter().find(|s| &s.id == song_id))
                .and_then(|song| large_artwork.get(&song.album_id))
        } else {
            None
        }
        .or_else(|| {
            self.common
                .slot_list
                .get_center_item_index(queue_songs.len())
                .and_then(|center_idx| queue_songs.get(center_idx))
                .and_then(|song| large_artwork.get(&song.album_id))
        });

        use crate::widgets::base_slot_list_layout::{
            base_slot_list_layout, single_artwork_panel_with_menu,
        };

        // Build artwork column component — determine album_id for refresh action
        let center_album_id: Option<String> = if data.is_playing {
            current_playing_song_id
                .as_ref()
                .and_then(|song_id| queue_songs.iter().find(|s| &s.id == song_id))
                .map(|song| song.album_id.clone())
        } else {
            None
        }
        .or_else(|| {
            self.common
                .slot_list
                .get_center_item_index(queue_songs.len())
                .and_then(|center_idx| queue_songs.get(center_idx))
                .map(|song| song.album_id.clone())
        });
        let on_refresh = center_album_id.map(QueueMessage::RefreshArtwork);
        let artwork_content = Some(single_artwork_panel_with_menu(
            center_artwork_handle,
            on_refresh,
        ));

        base_slot_list_layout(&layout_config, header, slot_list_content, artwork_content)
    }
}

// ============================================================================
// ViewPage trait implementation
// ============================================================================

impl super::ViewPage for QueuePage {
    fn common(&self) -> &SlotListPageState {
        &self.common
    }
    fn common_mut(&mut self) -> &mut SlotListPageState {
        &mut self.common
    }

    fn search_input_id(&self) -> &'static str {
        super::QUEUE_SEARCH_ID
    }

    // Queue uses QueueSortMode, not SortMode — sort_mode_selected_message returns None (default).
    fn toggle_sort_order_message(&self) -> Message {
        Message::Queue(QueueMessage::ToggleSortOrder)
    }

    // Queue items are already in the queue, so add_to_queue_message returns None (default).
    // Queue has no reload_message (client-side filtering, no server fetch needed on Escape).
}
