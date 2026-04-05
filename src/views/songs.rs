//! Songs Page Component
//!
//! Self-contained songs view with slot list navigation, search, and filtering.
//! Uses message bubbling pattern to communicate global actions to root.

use std::collections::HashMap;

use iced::{
    Alignment, Element, Length, Task,
    widget::{button, container, image, row, text},
};
use nokkvi_data::{
    backend::songs::SongUIViewData,
    utils::{formatters, scale::calculate_font_size},
};

use crate::widgets::{self, SlotListPageState, view_header::SortMode};

/// Songs page local state
#[derive(Debug)]
pub struct SongsPage {
    pub common: SlotListPageState,
}

/// View data passed from root (read-only, borrows from app state to avoid allocations)
pub struct SongsViewData<'a> {
    pub songs: &'a [SongUIViewData],
    pub album_art: &'a HashMap<String, image::Handle>, // album_id -> artwork
    pub large_artwork: &'a HashMap<String, image::Handle>,
    pub window_width: f32,
    pub window_height: f32,
    pub scale_factor: f32,
    pub modifiers: iced::keyboard::Modifiers,
    pub total_song_count: usize,
    pub loading: bool,
    pub stable_viewport: bool,
}

/// Messages for local song page interactions
#[derive(Debug, Clone)]
pub enum SongsMessage {
    // Slot list navigation
    SlotListNavigateUp,
    SlotListNavigateDown,
    SlotListSetOffset(usize, iced::keyboard::Modifiers),
    SlotListScrollSeek(usize),
    SlotListActivateCenter,
    SlotListClickPlay(usize), // Click non-center to play directly (skip focus)
    AddCenterToQueue,         // Add centered song to queue (Shift+Q)
    ToggleCenterStar,         // Toggle star on centered song (L key)

    // Mouse click on star/heart (item_index, value)
    ClickSetRating(usize, usize), // (item_index, rating 1-5)
    ClickToggleStar(usize),       // item_index

    // Context menu
    ContextMenuAction(usize, crate::widgets::context_menu::LibraryContextEntry),

    // View header
    SortModeSelected(widgets::view_header::SortMode),
    ToggleSortOrder,
    SearchQueryChanged(String),
    SearchFocused(bool),
    RefreshViewData,
    CenterOnPlaying,

    // Data loading (moved from root Message enum)
    SongsLoaded(Result<Vec<SongUIViewData>, String>, usize), // result, total_count
    SongsPageLoaded(Result<Vec<SongUIViewData>, String>, usize), // result, total_count (subsequent page)
    /// Refresh artwork for a specific album (album_id)
    RefreshArtwork(String),
}

/// Actions that bubble up to root for global state mutation
#[derive(Debug, Clone)]
pub enum SongsAction {
    PlaySongFromIndex(usize), // Play songs starting from index
    AddBatchToQueue(nokkvi_data::types::batch::BatchPayload),
    AddBatchToPlaylist(nokkvi_data::types::batch::BatchPayload),
    ToggleStar(String, bool), // (song_id, star)
    SetRating(String, usize), // (song_id, rating) - set absolute rating
    LoadLargeArtwork(String), // album_id for artwork
    LoadPage(usize),          // offset - trigger fetch of next page

    SearchChanged(String),                                  // trigger reload
    SortModeChanged(widgets::view_header::SortMode),        // trigger reload
    SortOrderChanged(bool),                                 // trigger reload
    RefreshViewData,                                        // trigger reload
    PlayNextBatch(nokkvi_data::types::batch::BatchPayload), // Batch payload
    PlayBatch(nokkvi_data::types::batch::BatchPayload),     // Play immediately
    AddToPlaylist(String),                                  // song_id - add to playlist dialog
    ShowInfo(Box<nokkvi_data::types::info_modal::InfoModalItem>), // Open info modal
    ShowInFolder(String),   // relative path - open containing folder
    RefreshArtwork(String), // album_id - refresh artwork from server
    CenterOnPlaying,
    None,
}

impl super::HasCommonAction for SongsAction {
    fn as_common(&self) -> super::CommonViewAction {
        match self {
            Self::SearchChanged(_) => super::CommonViewAction::SearchChanged,
            Self::SortModeChanged(m) => super::CommonViewAction::SortModeChanged(*m),
            Self::SortOrderChanged(a) => super::CommonViewAction::SortOrderChanged(*a),
            Self::RefreshViewData => super::CommonViewAction::RefreshViewData,
            Self::CenterOnPlaying => super::CommonViewAction::CenterOnPlaying,
            Self::None => super::CommonViewAction::None,
            _ => super::CommonViewAction::ViewSpecific,
        }
    }
}

impl Default for SongsPage {
    fn default() -> Self {
        Self {
            common: SlotListPageState::new(
                widgets::view_header::SortMode::RecentlyAdded,
                false, // sort_ascending
            ),
        }
    }
}

impl SongsPage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Update internal state and return actions for root
    pub fn update(
        &mut self,
        message: SongsMessage,
        songs: &[SongUIViewData],
    ) -> (Task<SongsMessage>, SongsAction) {
        let total_items = songs.len();

        match message {
            SongsMessage::SlotListNavigateUp => {
                self.common.handle_navigate_up(total_items);
                if let Some(center_idx) = self.common.get_center_item_index(total_items)
                    && let Some(song) = songs.get(center_idx)
                    && let Some(album_id) = &song.album_id
                {
                    return (
                        Task::none(),
                        SongsAction::LoadLargeArtwork(album_id.clone()),
                    );
                }
                (Task::none(), SongsAction::None)
            }
            SongsMessage::SlotListNavigateDown => {
                self.common.handle_navigate_down(total_items);
                if let Some(center_idx) = self.common.get_center_item_index(total_items)
                    && let Some(song) = songs.get(center_idx)
                    && let Some(album_id) = &song.album_id
                {
                    return (
                        Task::none(),
                        SongsAction::LoadLargeArtwork(album_id.clone()),
                    );
                }
                (Task::none(), SongsAction::None)
            }
            SongsMessage::SlotListSetOffset(offset, modifiers) => {
                self.common
                    .handle_slot_click(offset, total_items, modifiers);
                if let Some(center_idx) = self.common.get_center_item_index(total_items)
                    && let Some(song) = songs.get(center_idx)
                    && let Some(album_id) = &song.album_id
                {
                    return (
                        Task::none(),
                        SongsAction::LoadLargeArtwork(album_id.clone()),
                    );
                }
                (Task::none(), SongsAction::None)
            }
            SongsMessage::SlotListScrollSeek(offset) => {
                self.common.handle_set_offset(offset, total_items);
                (Task::none(), SongsAction::None)
            }
            SongsMessage::SlotListActivateCenter => {
                if !self.common.slot_list.selected_indices.is_empty() {
                    use nokkvi_data::types::batch::{BatchItem, BatchPayload};
                    let payload = self
                        .common
                        .slot_list
                        .selected_indices
                        .iter()
                        .filter_map(|&index| {
                            songs.get(index).map(|s| {
                                let item: nokkvi_data::types::song::Song = s.clone().into();
                                BatchItem::Song(Box::new(item))
                            })
                        })
                        .fold(BatchPayload::new(), |p, i| p.with_item(i));
                    (Task::none(), SongsAction::PlayBatch(payload))
                } else if let Some(center_idx) = self.common.get_center_item_index(total_items) {
                    self.common.slot_list.flash_center();
                    (Task::none(), SongsAction::PlaySongFromIndex(center_idx))
                } else {
                    (Task::none(), SongsAction::None)
                }
            }
            SongsMessage::SlotListClickPlay(offset) => {
                self.common.handle_set_offset(offset, total_items);
                self.update(SongsMessage::SlotListActivateCenter, songs)
            }
            SongsMessage::AddCenterToQueue => {
                use nokkvi_data::types::batch::{BatchItem, BatchPayload};

                let target_indices: Vec<usize> =
                    if !self.common.slot_list.selected_indices.is_empty() {
                        self.common
                            .slot_list
                            .selected_indices
                            .iter()
                            .copied()
                            .collect()
                    } else if let Some(center_idx) = self.common.get_center_item_index(total_items)
                    {
                        vec![center_idx]
                    } else {
                        vec![]
                    };

                if target_indices.is_empty() {
                    return (Task::none(), SongsAction::None);
                }

                let payload = target_indices
                    .into_iter()
                    .filter_map(|i| {
                        songs.get(i).map(|s| {
                            let item: nokkvi_data::types::song::Song = s.clone().into();
                            BatchItem::Song(Box::new(item))
                        })
                    })
                    .fold(BatchPayload::new(), |p: BatchPayload, item| {
                        p.with_item(item)
                    });

                (Task::none(), SongsAction::AddBatchToQueue(payload))
            }
            SongsMessage::ToggleCenterStar => {
                if let Some(center_idx) = self.common.get_center_item_index(total_items)
                    && let Some(song) = songs.get(center_idx)
                {
                    return (
                        Task::none(),
                        SongsAction::ToggleStar(song.id.clone(), !song.is_starred),
                    );
                }
                (Task::none(), SongsAction::None)
            }
            SongsMessage::ClickSetRating(item_index, rating) => {
                if let Some(song) = songs.get(item_index) {
                    let current = song.rating.unwrap_or(0) as usize;
                    // Click same star = decrease by 1 (toggle), else set to clicked value
                    let new_rating = if rating == current {
                        rating.saturating_sub(1)
                    } else {
                        rating
                    };
                    (
                        Task::none(),
                        SongsAction::SetRating(song.id.clone(), new_rating),
                    )
                } else {
                    (Task::none(), SongsAction::None)
                }
            }
            SongsMessage::ClickToggleStar(item_index) => {
                if let Some(song) = songs.get(item_index) {
                    return (
                        Task::none(),
                        SongsAction::ToggleStar(song.id.clone(), !song.is_starred),
                    );
                }
                (Task::none(), SongsAction::None)
            }
            SongsMessage::SortModeSelected(sort_mode) => {
                use crate::widgets::SlotListPageAction;
                match self.common.handle_sort_mode_selected(sort_mode) {
                    SlotListPageAction::SortModeChanged(vt) => {
                        (Task::none(), SongsAction::SortModeChanged(vt))
                    }
                    _ => (Task::none(), SongsAction::None),
                }
            }
            SongsMessage::ToggleSortOrder => {
                use crate::widgets::SlotListPageAction;
                match self.common.handle_toggle_sort_order() {
                    SlotListPageAction::SortOrderChanged(ascending) => {
                        (Task::none(), SongsAction::SortOrderChanged(ascending))
                    }
                    _ => (Task::none(), SongsAction::None),
                }
            }
            SongsMessage::SearchQueryChanged(query) => {
                use crate::widgets::SlotListPageAction;
                match self.common.handle_search_query_changed(query, total_items) {
                    SlotListPageAction::SearchChanged(q) => {
                        (Task::none(), SongsAction::SearchChanged(q))
                    }
                    _ => (Task::none(), SongsAction::None),
                }
            }
            SongsMessage::SearchFocused(focused) => {
                self.common.handle_search_focused(focused);
                (Task::none(), SongsAction::None)
            }

            SongsMessage::ContextMenuAction(clicked_idx, entry) => {
                use nokkvi_data::types::batch::{BatchItem, BatchPayload};

                use crate::widgets::context_menu::LibraryContextEntry;

                let target_indices = self.common.evaluate_context_menu(clicked_idx);
                self.common.clear_multi_selection();

                let payload = target_indices
                    .into_iter()
                    .filter_map(|i| {
                        songs.get(i).map(|s| {
                            let item: nokkvi_data::types::song::Song = s.clone().into();
                            BatchItem::Song(Box::new(item))
                        })
                    })
                    .fold(BatchPayload::new(), |p: BatchPayload, item| {
                        p.with_item(item)
                    });

                if let Some(song) = songs.get(clicked_idx) {
                    match entry {
                        LibraryContextEntry::AddToQueue => {
                            (Task::none(), SongsAction::AddBatchToQueue(payload))
                        }
                        LibraryContextEntry::AddToPlaylist => {
                            // AddToPlaylist backend takes a Vec<String> of song IDs, or a batch?
                            // We will emit AddBatchToPlaylist but for now, if Batch doesn't fit AddToPlaylist perfectly,
                            // we can map payload -> IDs. Let's just pass payload.
                            (Task::none(), SongsAction::AddBatchToPlaylist(payload))
                        }
                        LibraryContextEntry::GetInfo => {
                            use nokkvi_data::types::info_modal::InfoModalItem;
                            let item = InfoModalItem::from_song_view_data(song);
                            (Task::none(), SongsAction::ShowInfo(Box::new(item)))
                        }
                        LibraryContextEntry::ShowInFolder => {
                            (Task::none(), SongsAction::ShowInFolder(song.path.clone()))
                        }
                        LibraryContextEntry::Separator => (Task::none(), SongsAction::None),
                    }
                } else {
                    (Task::none(), SongsAction::None)
                }
            }

            // Data loading messages (handled at root level, no action needed here)
            SongsMessage::SongsLoaded(_, _) | SongsMessage::SongsPageLoaded(_, _) => {
                (Task::none(), SongsAction::None)
            }
            SongsMessage::RefreshViewData => (Task::none(), SongsAction::RefreshViewData),
            SongsMessage::RefreshArtwork(album_id) => {
                (Task::none(), SongsAction::RefreshArtwork(album_id))
            }
            SongsMessage::CenterOnPlaying => (Task::none(), SongsAction::CenterOnPlaying),
        }
    }

    /// Convert SortMode to API string for ViewModel
    pub fn sort_mode_to_api_string(sort_mode: SortMode) -> &'static str {
        match sort_mode {
            SortMode::RecentlyAdded => "recentlyAdded",
            SortMode::RecentlyPlayed => "recentlyPlayed",
            SortMode::MostPlayed => "mostPlayed",
            SortMode::Favorited => "favorited",
            SortMode::Random => "random",
            SortMode::Title | SortMode::Name => "title",
            SortMode::Album => "album",
            SortMode::Artist => "artist",
            SortMode::AlbumArtist => "albumArtist",
            SortMode::ReleaseYear => "year",
            SortMode::Duration => "duration",
            SortMode::Bpm => "bpm",
            SortMode::Channels => "channels",
            SortMode::Genre => "genre",
            SortMode::Rating => "rating",
            SortMode::Comment => "comment",
            SortMode::SongCount | SortMode::AlbumCount | SortMode::UpdatedAt => "recentlyAdded", // Fallback
        }
    }

    /// Build the view
    pub fn view<'a>(&'a self, data: SongsViewData<'a>) -> Element<'a, SongsMessage> {
        use crate::widgets::view_header::SortMode;

        let header = widgets::view_header::view_header(
            self.common.current_sort_mode,
            SortMode::SONG_OPTIONS,
            self.common.sort_ascending,
            &self.common.search_query,
            data.songs.len(),
            data.total_song_count,
            "songs",
            crate::views::SONGS_SEARCH_ID,
            SongsMessage::SortModeSelected,
            SongsMessage::ToggleSortOrder,
            None, // No shuffle button for songs
            Some(SongsMessage::RefreshViewData),
            Some(SongsMessage::CenterOnPlaying),
            SongsMessage::SearchQueryChanged,
        );

        // Create layout config BEFORE empty checks to route empty states through
        // base_slot_list_layout, preserving the widget tree structure and search focus
        use crate::widgets::base_slot_list_layout::BaseSlotListLayoutConfig;
        let layout_config = BaseSlotListLayoutConfig {
            window_width: data.window_width,
            window_height: data.window_height,
            show_artwork_column: true,
        };

        // If loading, show header with loading message
        if data.loading {
            return widgets::base_slot_list_empty_state(header, "Loading...", &layout_config);
        }

        // If no songs match search, show message but keep the header
        if data.songs.is_empty() {
            return widgets::base_slot_list_empty_state(
                header,
                "No songs match your search.",
                &layout_config,
            );
        }

        // Configure slot list with songs-specific chrome height (has view header)
        use crate::widgets::slot_list::{
            SlotListConfig, chrome_height_with_header, slot_list_view_with_scroll,
        };

        let config =
            SlotListConfig::with_dynamic_slots(data.window_height, chrome_height_with_header())
                .with_modifiers(data.modifiers);

        // Capture values needed in closure
        let songs = data.songs;
        let song_artwork = data.album_art;
        let current_sort_mode = self.common.current_sort_mode;
        let center_index = self.common.slot_list.get_center_item_index(songs.len());

        // Render slot list using generic component with item renderer closure
        let slot_list_content = slot_list_view_with_scroll(
            &self.common.slot_list,
            songs,
            &config,
            SongsMessage::SlotListNavigateUp,
            SongsMessage::SlotListNavigateDown,
            {
                let total = songs.len();
                move |f| SongsMessage::SlotListScrollSeek((f * total as f32) as usize)
            },
            |song, ctx| {
                // Clone all data from song at the start to avoid lifetime issues
                let song_title = song.title.clone();
                let song_artist = song.artist.clone();
                let song_album = song.album.clone();
                let album_id = song.album_id.clone();
                let duration = song.duration;
                let is_starred = song.is_starred;
                let rating = song.rating.unwrap_or(0).min(5) as usize;

                // Get extra column value based on current sort mode
                let extra_value = nokkvi_data::backend::songs::SongsService::get_extra_column_data(
                    song,
                    Self::sort_mode_to_api_string(current_sort_mode),
                );

                // Get centralized slot list slot styling
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

                // Dynamic scaling based on row height AND scale factor
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

                // Layout: [Index] [Art] [Title/Artist (40%)] [Album (25%)] [Extra (18%)] [Duration (10%)] [Star (5%)]
                let content = row![
                    // 0. Index number (fixed width)
                    slot_list_index_column(ctx.item_index, index_size, style, ctx.opacity),
                    // 1. Album Art (fixed width)
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
                    // 2. Title + Artist (40%)
                    {
                        use crate::widgets::slot_list::slot_list_text_column;
                        slot_list_text_column(
                            song_title,
                            song_artist,
                            title_size,
                            subtitle_size,
                            style,
                            ctx.is_center,
                            40,
                        )
                    },
                    // 3. Album (25%)
                    {
                        use crate::widgets::slot_list::slot_list_metadata_column;
                        slot_list_metadata_column(song_album, metadata_size, style, 25)
                    },
                    // 4. Extra Column (18%) - Dynamic based on current sort mode
                    {
                        if current_sort_mode == SortMode::Rating {
                            use crate::widgets::slot_list::slot_list_star_rating;
                            let star_icon_size =
                                calculate_font_size(14.0, ctx.row_height, ctx.scale_factor)
                                    * ctx.scale_factor;
                            let idx = ctx.item_index;
                            slot_list_star_rating(
                                rating,
                                star_icon_size,
                                ctx.is_center,
                                ctx.opacity,
                                Some(18),
                                Some(move |star: usize| SongsMessage::ClickSetRating(idx, star)),
                            )
                        } else if !extra_value.is_empty() {
                            container(slot_list_text(
                                extra_value,
                                calculate_font_size(14.0, ctx.row_height, ctx.scale_factor)
                                    * ctx.scale_factor,
                                style.subtext_color,
                            ))
                            .width(Length::FillPortion(18))
                            .height(Length::Fill)
                            .clip(true)
                            .align_y(Alignment::Center)
                            .into()
                        } else {
                            container(text("")).width(Length::FillPortion(18)).into()
                        }
                    },
                    // 5. Duration (10%)
                    {
                        let duration_str = formatters::format_time(duration);
                        container(slot_list_text(
                            duration_str,
                            metadata_size,
                            style.subtext_color,
                        ))
                        .width(Length::FillPortion(10))
                        .height(Length::Fill)
                        .align_x(Alignment::End)
                        .align_y(Alignment::Center)
                    },
                    // 6. Star/Heart Icon (5%)
                    container({
                        use crate::widgets::slot_list::slot_list_favorite_icon;
                        slot_list_favorite_icon(
                            is_starred,
                            ctx.is_center,
                            false,
                            ctx.opacity,
                            star_size,
                            "heart",
                            Some(SongsMessage::ClickToggleStar(ctx.item_index)),
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

                // Wrap in clickable container
                let clickable = container(content)
                    .style(move |_theme| style.to_container_style())
                    .width(Length::Fill);

                let slot_button = button(clickable)
                    .on_press(if ctx.modifiers.control() || ctx.modifiers.shift() {
                        SongsMessage::SlotListSetOffset(ctx.item_index, ctx.modifiers)
                    } else if ctx.is_center {
                        SongsMessage::SlotListActivateCenter
                    } else if data.stable_viewport {
                        SongsMessage::SlotListSetOffset(ctx.item_index, ctx.modifiers)
                    } else {
                        SongsMessage::SlotListClickPlay(ctx.item_index)
                    })
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
                            SongsMessage::ContextMenuAction(item_idx, e)
                        })
                    },
                )
                .into()
            },
        );

        // Wrap slot list content with standard background
        use crate::widgets::slot_list::slot_list_background_container;
        let slot_list_content = slot_list_background_container(slot_list_content);

        use crate::widgets::base_slot_list_layout::{
            base_slot_list_layout, single_artwork_panel_with_menu,
        };

        // Build artwork column component - use album artwork of centered song
        let centered_song = center_index.and_then(|idx| songs.get(idx));
        let artwork_handle = centered_song
            .and_then(|song| song.album_id.as_ref())
            .and_then(|album_id| data.large_artwork.get(album_id));
        let on_refresh = centered_song
            .and_then(|song| song.album_id.clone())
            .map(SongsMessage::RefreshArtwork);

        let artwork_content = Some(single_artwork_panel_with_menu(artwork_handle, on_refresh));

        base_slot_list_layout(&layout_config, header, slot_list_content, artwork_content)
    }
}

// ============================================================================
// ViewPage trait implementation
// ============================================================================

use crate::app_message::Message;

impl super::ViewPage for SongsPage {
    fn common(&self) -> &SlotListPageState {
        &self.common
    }
    fn common_mut(&mut self) -> &mut SlotListPageState {
        &mut self.common
    }

    fn search_input_id(&self) -> &'static str {
        super::SONGS_SEARCH_ID
    }

    fn sort_mode_options(&self) -> Option<&'static [SortMode]> {
        Some(SortMode::SONG_OPTIONS)
    }
    fn sort_mode_selected_message(&self, mode: SortMode) -> Option<Message> {
        Some(Message::Songs(SongsMessage::SortModeSelected(mode)))
    }
    fn toggle_sort_order_message(&self) -> Message {
        Message::Songs(SongsMessage::ToggleSortOrder)
    }

    fn add_to_queue_message(&self) -> Option<Message> {
        Some(Message::Songs(SongsMessage::AddCenterToQueue))
    }
    fn reload_message(&self) -> Option<Message> {
        Some(Message::LoadSongs)
    }
}
