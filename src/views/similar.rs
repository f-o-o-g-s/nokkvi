//! Similar Songs Page Component
//!
//! Displays algorithmic recommendations from Navidrome's getSimilarSongs2 / getTopSongs endpoints.
//! Stripped-down version of SongsPage — no sort, no search, no pagination.
//! Results are ephemeral (populated via context menu, not persisted).

use std::collections::HashMap;

use iced::{
    Alignment, Element, Length, Task,
    widget::{button, container, image, row},
};
use nokkvi_data::{
    types::song::Song,
    utils::{formatters, scale::calculate_font_size},
};

use crate::widgets::{self, SlotListPageState, view_header::SortMode};

/// Similar page local state — just a slot list, no sort/search/pagination.
#[derive(Debug)]
pub struct SimilarPage {
    pub common: SlotListPageState,
}

/// View data passed from root (read-only borrows from app state).
pub struct SimilarViewData<'a> {
    pub songs: &'a [Song],
    pub album_art: &'a HashMap<String, image::Handle>,
    pub large_artwork: &'a HashMap<String, image::Handle>,
    pub window_width: f32,
    pub window_height: f32,
    pub scale_factor: f32,
    pub modifiers: iced::keyboard::Modifiers,
    /// Provenance label: "Similar to: Paranoid Android" or "Top Songs: Radiohead"
    pub label: &'a str,
    /// Whether an API call is in flight
    pub loading: bool,
}

/// Messages for local similar page interactions
#[derive(Debug, Clone)]
pub enum SimilarMessage {
    NoOp,

    // Slot list navigation
    SlotListNavigateUp,
    SlotListNavigateDown,
    SlotListSetOffset(usize, iced::keyboard::Modifiers),
    SlotListScrollSeek(usize),
    AddCenterToQueue,

    // Mouse click on star/heart (item_index, value)
    ClickSetRating(usize, usize),
    ClickToggleStar(usize),

    // Context menu
    ContextMenuAction(usize, crate::widgets::context_menu::LibraryContextEntry),
}

/// Actions that bubble up to root for global state mutation
#[derive(Debug, Clone)]
pub enum SimilarAction {
    AddBatchToQueue(nokkvi_data::types::batch::BatchPayload),
    AddBatchToPlaylist(nokkvi_data::types::batch::BatchPayload),
    ToggleStar(String, bool),
    SetRating(String, usize),
    LoadLargeArtwork(String),
    ShowInfo(Box<nokkvi_data::types::info_modal::InfoModalItem>),
    ShowInFolder(String),
    /// Recursive discovery: FindSimilar from within the similar results
    FindSimilar(String, String),
    None,
}

impl Default for SimilarPage {
    fn default() -> Self {
        Self {
            common: SlotListPageState::new(
                SortMode::Title, // unused — no sort controls
                true,
            ),
        }
    }
}

impl SimilarPage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Update internal state and return actions for root
    pub fn update(
        &mut self,
        message: SimilarMessage,
        songs: &[Song],
    ) -> (Task<SimilarMessage>, SimilarAction) {
        let total_items = songs.len();

        match message {
            SimilarMessage::NoOp => (Task::none(), SimilarAction::None),
            SimilarMessage::SlotListNavigateUp => {
                self.common.handle_navigate_up(total_items);
                if let Some(center_idx) = self.common.get_center_item_index(total_items)
                    && let Some(song) = songs.get(center_idx)
                    && let Some(album_id) = &song.album_id
                {
                    return (
                        Task::none(),
                        SimilarAction::LoadLargeArtwork(album_id.clone()),
                    );
                }
                (Task::none(), SimilarAction::None)
            }
            SimilarMessage::SlotListNavigateDown => {
                self.common.handle_navigate_down(total_items);
                if let Some(center_idx) = self.common.get_center_item_index(total_items)
                    && let Some(song) = songs.get(center_idx)
                    && let Some(album_id) = &song.album_id
                {
                    return (
                        Task::none(),
                        SimilarAction::LoadLargeArtwork(album_id.clone()),
                    );
                }
                (Task::none(), SimilarAction::None)
            }
            SimilarMessage::SlotListSetOffset(offset, modifiers) => {
                self.common
                    .handle_slot_click(offset, total_items, modifiers);
                if let Some(center_idx) = self.common.get_center_item_index(total_items)
                    && let Some(song) = songs.get(center_idx)
                    && let Some(album_id) = &song.album_id
                {
                    return (
                        Task::none(),
                        SimilarAction::LoadLargeArtwork(album_id.clone()),
                    );
                }
                (Task::none(), SimilarAction::None)
            }
            SimilarMessage::SlotListScrollSeek(offset) => {
                self.common.handle_set_offset(offset, total_items);
                (Task::none(), SimilarAction::None)
            }
            SimilarMessage::AddCenterToQueue => {
                use nokkvi_data::types::batch::BatchItem;

                let target_indices = self.common.get_queue_target_indices(total_items);

                if target_indices.is_empty() {
                    return (Task::none(), SimilarAction::None);
                }

                let payload = super::expansion::build_batch_payload(target_indices, |i| {
                    songs.get(i).map(|s| BatchItem::Song(Box::new(s.clone())))
                });

                (Task::none(), SimilarAction::AddBatchToQueue(payload))
            }
            SimilarMessage::ClickSetRating(item_index, rating) => {
                if let Some(song) = songs.get(item_index) {
                    use nokkvi_data::utils::formatters::compute_rating_toggle;
                    let current = song.rating.unwrap_or(0) as usize;
                    let new_rating = compute_rating_toggle(current, rating);
                    (
                        Task::none(),
                        SimilarAction::SetRating(song.id.clone(), new_rating),
                    )
                } else {
                    (Task::none(), SimilarAction::None)
                }
            }
            SimilarMessage::ClickToggleStar(item_index) => {
                if let Some(song) = songs.get(item_index) {
                    (
                        Task::none(),
                        SimilarAction::ToggleStar(song.id.clone(), !song.starred),
                    )
                } else {
                    (Task::none(), SimilarAction::None)
                }
            }
            SimilarMessage::ContextMenuAction(clicked_idx, entry) => {
                use nokkvi_data::types::batch::BatchItem;

                use crate::widgets::context_menu::LibraryContextEntry;

                let target_indices = self.common.get_batch_target_indices(clicked_idx);

                let payload = super::expansion::build_batch_payload(target_indices, |i| {
                    songs.get(i).map(|s| BatchItem::Song(Box::new(s.clone())))
                });

                if let Some(song) = songs.get(clicked_idx) {
                    match entry {
                        LibraryContextEntry::AddToQueue => {
                            (Task::none(), SimilarAction::AddBatchToQueue(payload))
                        }
                        LibraryContextEntry::AddToPlaylist => {
                            (Task::none(), SimilarAction::AddBatchToPlaylist(payload))
                        }
                        LibraryContextEntry::GetInfo => {
                            use nokkvi_data::types::info_modal::InfoModalItem;
                            let item = InfoModalItem::from_song(song);
                            (Task::none(), SimilarAction::ShowInfo(Box::new(item)))
                        }
                        LibraryContextEntry::ShowInFolder => {
                            (Task::none(), SimilarAction::ShowInFolder(song.path.clone()))
                        }
                        LibraryContextEntry::FindSimilar => (
                            Task::none(),
                            SimilarAction::FindSimilar(song.id.clone(), song.title.clone()),
                        ),
                        LibraryContextEntry::TopSongs | LibraryContextEntry::Separator => {
                            (Task::none(), SimilarAction::None)
                        }
                    }
                } else {
                    (Task::none(), SimilarAction::None)
                }
            }
        }
    }

    /// Build the view
    pub fn view<'a>(&'a self, data: SimilarViewData<'a>) -> Element<'a, SimilarMessage> {
        // Replace custom header with standard view_header
        let header_prefix = if data.label.is_empty() {
            "similar to selected".to_string()
        } else {
            // "Similar to: Song Name"
            data.label.to_lowercase()
        };

        // We don't have sort mode options for Similar, we can pass a dummy enum or just use Song options
        // since we are hiding the sort button anyway.
        use crate::widgets::view_header::SortMode;

        let header = widgets::view_header::view_header(
            SortMode::Name,
            SortMode::SONG_OPTIONS, // dummy
            true,                   // ascending dummy
            "",                     // no search query
            data.songs.len(),
            data.songs.len(),
            &header_prefix,
            crate::views::SIMILAR_SEARCH_ID,
            |_| SimilarMessage::NoOp,
            None,  // Hide sort button
            None,  // Hide shuffle button
            None,  // Hide refresh
            None,  // Hide center on playing
            false, // Hide search
            |_| SimilarMessage::NoOp,
        );

        // Layout config
        use crate::widgets::base_slot_list_layout::BaseSlotListLayoutConfig;
        let layout_config = BaseSlotListLayoutConfig {
            window_width: data.window_width,
            window_height: data.window_height,
            show_artwork_column: true,
        };

        // Loading state
        if data.loading && data.songs.is_empty() {
            let loading_text = if data.label.starts_with("Top Songs") {
                "Loading top songs…"
            } else {
                "Loading similar songs…"
            };

            return widgets::base_slot_list_empty_state(
                header,
                loading_text,
                &layout_config,
            );
        }

        // Empty state (no results loaded yet)
        if data.songs.is_empty() {
            return widgets::base_slot_list_empty_state(
                header,
                "Right-click a track, album, or artist\nand select 'Find Similar' to discover new music.",
                &layout_config,
            );
        }

        // Configure slot list
        use crate::widgets::slot_list::{
            SlotListConfig, chrome_height_with_header, slot_list_view_with_scroll,
        };

        let config =
            SlotListConfig::with_dynamic_slots(data.window_height, chrome_height_with_header())
                .with_modifiers(data.modifiers);

        let songs = data.songs;
        let song_artwork = data.album_art;
        let center_index = self.common.slot_list.get_center_item_index(songs.len());

        // Render slot list
        let slot_list_content = slot_list_view_with_scroll(
            &self.common.slot_list,
            songs,
            &config,
            SimilarMessage::SlotListNavigateUp,
            SimilarMessage::SlotListNavigateDown,
            {
                let total = songs.len();
                move |f| SimilarMessage::SlotListScrollSeek((f * total as f32) as usize)
            },
            |song, ctx| {
                let song_title = song.title.clone();
                let song_artist = song.artist.clone();
                let song_album = song.album.clone();
                let album_id = song.album_id.clone();
                let duration = song.duration;
                let is_starred = song.starred;
                let _rating = song.rating.unwrap_or(0).min(5) as usize;

                use crate::widgets::slot_list::{
                    SLOT_LIST_SLOT_PADDING, SlotListSlotStyle, slot_list_index_column,
                    slot_list_text,
                };
                let style = SlotListSlotStyle::for_slot(
                    ctx.is_center,
                    false,
                    ctx.is_selected,
                    ctx.has_multi_selection,
                    ctx.opacity,
                );

                let base_artwork_size = (ctx.row_height - 16.0).max(32.0);
                let artwork_size = base_artwork_size * ctx.scale_factor;
                let title_size =
                    calculate_font_size(16.0, ctx.row_height, ctx.scale_factor) * ctx.scale_factor;
                let subtitle_size =
                    calculate_font_size(13.0, ctx.row_height, ctx.scale_factor) * ctx.scale_factor;
                let metadata_size =
                    calculate_font_size(12.0, ctx.row_height, ctx.scale_factor) * ctx.scale_factor;
                let star_size = (ctx.row_height * 0.3 * ctx.scale_factor).clamp(16.0, 24.0);
                let index_size =
                    calculate_font_size(12.0, ctx.row_height, ctx.scale_factor) * ctx.scale_factor;

                // Layout: [Index] [Art] [Title/Artist (45%)] [Album (30%)] [Duration (15%)] [Star (5%)]
                let content = row![
                    slot_list_index_column(ctx.item_index, index_size, style, ctx.opacity),
                    {
                        use crate::widgets::slot_list::slot_list_artwork_column;
                        let artwork_handle = album_id.as_ref().and_then(|id| song_artwork.get(id));
                        slot_list_artwork_column(
                            artwork_handle,
                            artwork_size,
                            ctx.is_center,
                            false,
                            ctx.opacity,
                        )
                    },
                    {
                        use crate::widgets::slot_list::slot_list_text_column;
                        slot_list_text_column(
                            song_title,
                            song_artist,
                            title_size,
                            subtitle_size,
                            style,
                            ctx.is_center,
                            45,
                        )
                    },
                    {
                        use crate::widgets::slot_list::slot_list_metadata_column;
                        slot_list_metadata_column(song_album, metadata_size, style, 30)
                    },
                    {
                        let duration_str = formatters::format_time(duration);
                        container(slot_list_text(
                            duration_str,
                            metadata_size,
                            style.subtext_color,
                        ))
                        .width(Length::FillPortion(15))
                        .height(Length::Fill)
                        .align_x(Alignment::End)
                        .align_y(Alignment::Center)
                    },
                    container({
                        use crate::widgets::slot_list::slot_list_favorite_icon;
                        slot_list_favorite_icon(
                            is_starred,
                            ctx.is_center,
                            false,
                            ctx.opacity,
                            star_size,
                            "heart",
                            Some(SimilarMessage::ClickToggleStar(ctx.item_index)),
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
                ]
                .spacing(6.0)
                .padding(iced::Padding {
                    left: SLOT_LIST_SLOT_PADDING,
                    right: 4.0,
                    top: 4.0,
                    bottom: 4.0,
                })
                .align_y(Alignment::Center)
                .height(Length::Fill);

                let clickable = container(content)
                    .style(move |_theme| style.to_container_style())
                    .width(Length::Fill);

                let slot_button = button(clickable)
                    .on_press(SimilarMessage::SlotListSetOffset(
                        ctx.item_index,
                        ctx.modifiers,
                    ))
                    .style(|_theme, _status| button::Style {
                        background: None,
                        border: iced::Border::default(),
                        ..Default::default()
                    })
                    .padding(0)
                    .width(Length::Fill);

                use crate::widgets::context_menu::{
                    context_menu, library_entries_with_folder, library_entry_view,
                };
                let item_idx = ctx.item_index;
                context_menu(
                    slot_button,
                    library_entries_with_folder(),
                    move |entry, length| {
                        library_entry_view(entry, length, |e| {
                            SimilarMessage::ContextMenuAction(item_idx, e)
                        })
                    },
                )
                .into()
            },
        );

        use crate::widgets::slot_list::slot_list_background_container;
        let slot_list_content = slot_list_background_container(slot_list_content);

        use crate::widgets::base_slot_list_layout::{
            base_slot_list_layout, single_artwork_panel_with_menu,
        };

        let centered_song = center_index.and_then(|idx| songs.get(idx));
        let artwork_handle = centered_song
            .and_then(|song| song.album_id.as_ref())
            .and_then(|album_id| data.large_artwork.get(album_id));
        let artwork_content = Some(single_artwork_panel_with_menu::<SimilarMessage>(
            artwork_handle,
            None,
        ));

        base_slot_list_layout(&layout_config, header, slot_list_content, artwork_content)
    }
}

// ============================================================================
// ViewPage trait implementation
// ============================================================================

use crate::app_message::Message;

impl super::ViewPage for SimilarPage {
    fn common(&self) -> &SlotListPageState {
        &self.common
    }
    fn common_mut(&mut self) -> &mut SlotListPageState {
        &mut self.common
    }

    fn search_input_id(&self) -> &'static str {
        super::SIMILAR_SEARCH_ID
    }

    // No sort mode cycling for Similar view
    fn sort_mode_options(&self) -> Option<&'static [SortMode]> {
        None
    }
    fn toggle_sort_order_message(&self) -> Message {
        Message::NoOp
    }

    fn add_to_queue_message(&self) -> Option<Message> {
        Some(Message::Similar(SimilarMessage::AddCenterToQueue))
    }
}
