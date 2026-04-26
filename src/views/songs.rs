//! Songs Page Component
//!
//! Self-contained songs view with slot list navigation, search, and filtering.
//! Uses message bubbling pattern to communicate global actions to root.

use std::collections::HashMap;

use iced::{
    Alignment, Element, Length, Task,
    widget::{button, container, image, row},
};
use nokkvi_data::{backend::songs::SongUIViewData, utils::formatters};

use crate::widgets::{self, SlotListPageState, view_header::SortMode};

/// Songs page local state
#[derive(Debug)]
pub struct SongsPage {
    pub common: SlotListPageState,
    /// Per-column visibility toggles surfaced via the columns-cog dropdown.
    pub column_visibility: SongsColumnVisibility,
}

/// Toggleable songs columns. Index/Art/Title+Artist are always shown; the
/// dynamic 18% slot still auto-renders Date/Year/Genre when sorted by those
/// modes. Stars and Plays are now dedicated columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SongsColumn {
    Stars,
    Album,
    Duration,
    Plays,
    Love,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SongsColumnVisibility {
    pub stars: bool,
    pub album: bool,
    pub duration: bool,
    pub plays: bool,
    pub love: bool,
}

impl Default for SongsColumnVisibility {
    fn default() -> Self {
        // Stars and Plays opt-in; Album/Duration/Love always-on today.
        Self {
            stars: false,
            album: true,
            duration: true,
            plays: false,
            love: true,
        }
    }
}

impl SongsColumnVisibility {
    pub fn get(&self, col: SongsColumn) -> bool {
        match col {
            SongsColumn::Stars => self.stars,
            SongsColumn::Album => self.album,
            SongsColumn::Duration => self.duration,
            SongsColumn::Plays => self.plays,
            SongsColumn::Love => self.love,
        }
    }

    pub fn set(&mut self, col: SongsColumn, value: bool) {
        match col {
            SongsColumn::Stars => self.stars = value,
            SongsColumn::Album => self.album = value,
            SongsColumn::Duration => self.duration = value,
            SongsColumn::Plays => self.plays = value,
            SongsColumn::Love => self.love = value,
        }
    }
}

pub(crate) fn songs_stars_visible(sort: SortMode, user_visible: bool) -> bool {
    user_visible || matches!(sort, SortMode::Rating)
}

pub(crate) fn songs_plays_visible(sort: SortMode, user_visible: bool) -> bool {
    user_visible || matches!(sort, SortMode::MostPlayed)
}

/// View data passed from root (read-only, borrows from app state to avoid allocations)
pub struct SongsViewData<'a> {
    pub songs: &'a [SongUIViewData],
    pub album_art: &'a HashMap<String, image::Handle>, // album_id -> artwork
    pub large_artwork: &'a HashMap<String, image::Handle>,
    pub dominant_colors: &'a HashMap<String, iced::Color>,
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

    // Mouse click on star/heart (item_index, value)
    ClickSetRating(usize, usize), // (item_index, rating 1-5)
    ClickToggleStar(usize),       // item_index

    // Context menu
    ContextMenuAction(usize, crate::widgets::context_menu::LibraryContextEntry),

    // View header
    SortModeSelected(widgets::view_header::SortMode),
    ToggleSortOrder,
    SearchQueryChanged(String),

    RefreshViewData,
    CenterOnPlaying,

    // Data loading (moved from root Message enum)
    SongsLoaded {
        result: Result<Vec<SongUIViewData>, String>,
        total_count: usize,
        background: bool,
        anchor_id: Option<String>,
    },
    SongsPageLoaded(Result<Vec<SongUIViewData>, String>, usize), // result, total_count (subsequent page)
    /// Refresh artwork for a specific album (album_id)
    RefreshArtwork(String),
    /// Navigate to a view and apply an ID filter
    NavigateAndFilter(crate::View, nokkvi_data::types::filter::LibraryFilter),
    ToggleColumnVisible(SongsColumn),
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
    ShowInFolder(String),        // relative path - open containing folder
    RefreshArtwork(String),      // album_id - refresh artwork from server
    FindSimilar(String, String), // (id, label) - Find similar to this song
    TopSongs(String, String),    // (artist, label) - Find top songs by artist
    CenterOnPlaying,
    NavigateAndFilter(crate::View, nokkvi_data::types::filter::LibraryFilter), // Navigate to target view and filter
    ColumnVisibilityChanged(SongsColumn, bool),
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
            Self::NavigateAndFilter(v, f) => {
                super::CommonViewAction::NavigateAndFilter(*v, f.clone())
            }
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
            column_visibility: SongsColumnVisibility::default(),
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
                use nokkvi_data::types::batch::BatchItem;

                let target_indices = self.common.get_queue_target_indices(total_items);

                if target_indices.is_empty() {
                    return (Task::none(), SongsAction::None);
                }

                let payload = super::expansion::build_batch_payload(target_indices, |i| {
                    songs.get(i).map(|s| {
                        let item: nokkvi_data::types::song::Song = s.clone().into();
                        BatchItem::Song(Box::new(item))
                    })
                });

                (Task::none(), SongsAction::AddBatchToQueue(payload))
            }

            SongsMessage::ClickSetRating(item_index, rating) => {
                if let Some(song) = songs.get(item_index) {
                    use nokkvi_data::utils::formatters::compute_rating_toggle;
                    let current = song.rating.unwrap_or(0) as usize;
                    let new_rating = compute_rating_toggle(current, rating);
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

            SongsMessage::ContextMenuAction(clicked_idx, entry) => {
                use nokkvi_data::types::batch::BatchItem;

                use crate::widgets::context_menu::LibraryContextEntry;

                let target_indices = self.common.get_batch_target_indices(clicked_idx);

                let payload = super::expansion::build_batch_payload(target_indices, |i| {
                    songs.get(i).map(|s| {
                        let item: nokkvi_data::types::song::Song = s.clone().into();
                        BatchItem::Song(Box::new(item))
                    })
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
                        LibraryContextEntry::FindSimilar => (
                            Task::none(),
                            SongsAction::FindSimilar(song.id.clone(), song.title.clone()),
                        ),
                        LibraryContextEntry::TopSongs => {
                            let artist = &song.artist;
                            if !artist.is_empty() {
                                (
                                    Task::none(),
                                    SongsAction::TopSongs(
                                        artist.clone(),
                                        format!("Top Songs: {artist}"),
                                    ),
                                )
                            } else {
                                (Task::none(), SongsAction::None)
                            }
                        }
                        LibraryContextEntry::Separator
                        | LibraryContextEntry::ReplaceQueueWithAllFound
                        | LibraryContextEntry::AddAllFoundToQueue
                        | LibraryContextEntry::AddAllFoundToPlaylist => {
                            (Task::none(), SongsAction::None)
                        }
                    }
                } else {
                    (Task::none(), SongsAction::None)
                }
            }

            // Data loading messages (handled at root level, no action needed here)
            SongsMessage::SongsLoaded { .. } | SongsMessage::SongsPageLoaded(_, _) => {
                (Task::none(), SongsAction::None)
            }
            SongsMessage::RefreshViewData => (Task::none(), SongsAction::RefreshViewData),
            SongsMessage::RefreshArtwork(album_id) => {
                (Task::none(), SongsAction::RefreshArtwork(album_id))
            }
            SongsMessage::CenterOnPlaying => (Task::none(), SongsAction::CenterOnPlaying),
            SongsMessage::NavigateAndFilter(view, filter) => {
                (Task::none(), SongsAction::NavigateAndFilter(view, filter))
            }
            SongsMessage::ToggleColumnVisible(col) => {
                let new_value = !self.column_visibility.get(col);
                self.column_visibility.set(col, new_value);
                (
                    Task::none(),
                    SongsAction::ColumnVisibilityChanged(col, new_value),
                )
            }
        }
    }

    /// Convert SortMode to API string for ViewModel.
    /// Thin shim — the unified mapping lives in `views/sort_api.rs`.
    pub fn sort_mode_to_api_string(sort_mode: SortMode) -> &'static str {
        super::sort_api::sort_mode_to_api_string(crate::View::Songs, sort_mode)
    }

    /// Build the view
    pub fn view<'a>(&'a self, data: SongsViewData<'a>) -> Element<'a, SongsMessage> {
        use crate::widgets::view_header::SortMode;

        let column_dropdown: Element<'a, SongsMessage> = {
            use crate::widgets::checkbox_dropdown::checkbox_dropdown;
            let items = vec![
                ("Stars".to_string(), self.column_visibility.stars),
                ("Album".to_string(), self.column_visibility.album),
                ("Duration".to_string(), self.column_visibility.duration),
                ("Plays".to_string(), self.column_visibility.plays),
                ("Love".to_string(), self.column_visibility.love),
            ];
            checkbox_dropdown(
                "assets/icons/columns-3-cog.svg",
                "Show/hide columns",
                items,
                |idx| match idx {
                    0 => SongsMessage::ToggleColumnVisible(SongsColumn::Stars),
                    1 => SongsMessage::ToggleColumnVisible(SongsColumn::Album),
                    2 => SongsMessage::ToggleColumnVisible(SongsColumn::Duration),
                    3 => SongsMessage::ToggleColumnVisible(SongsColumn::Plays),
                    _ => SongsMessage::ToggleColumnVisible(SongsColumn::Love),
                },
            )
            .into()
        };

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
            Some(SongsMessage::ToggleSortOrder),
            None, // No shuffle button for songs
            Some(SongsMessage::RefreshViewData),
            Some(SongsMessage::CenterOnPlaying),
            None,                  // on_add
            Some(column_dropdown), // trailing_button
            true,                  // show_search
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
                    0,
                );

                let m = ctx.metrics;
                let artwork_size = m.artwork_size;
                let title_size = m.title_size_lg;
                let subtitle_size = m.subtitle_size;
                let metadata_size = m.metadata_size;
                let star_size = m.star_size;
                let index_size = m.metadata_size;
                let play_count = song.play_count.unwrap_or(0);

                // Per-column visibility (Stars/Plays auto-shown by sort mode).
                let vis = self.column_visibility;
                let show_stars = songs_stars_visible(current_sort_mode, vis.stars);
                let show_album = vis.album;
                let show_plays = songs_plays_visible(current_sort_mode, vis.plays);
                let show_duration = vis.duration;
                let show_love = vis.love;
                // Dynamic slot now only carries non-Rating/MostPlayed/Duration
                // sort modes (those have dedicated columns or are redundant).
                let show_dynamic_slot = !extra_value.is_empty();

                const ALBUM_PORTION: u16 = 22;
                const STARS_PORTION: u16 = 11;
                const PLAYS_PORTION: u16 = 13;
                const DYNAMIC_PORTION: u16 = 18;
                const DURATION_PORTION: u16 = 10;
                const LOVE_PORTION: u16 = 5;
                let mut consumed: u16 = 0;
                if show_album {
                    consumed += ALBUM_PORTION;
                }
                if show_stars {
                    consumed += STARS_PORTION;
                }
                if show_plays {
                    consumed += PLAYS_PORTION;
                }
                if show_dynamic_slot {
                    consumed += DYNAMIC_PORTION;
                }
                if show_duration {
                    consumed += DURATION_PORTION;
                }
                if show_love {
                    consumed += LOVE_PORTION;
                }
                let title_portion = 100u16.saturating_sub(consumed).max(20);

                let mut content_row = row![
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
                        let artist_click = song.artist_id.as_ref().map(|id| {
                            SongsMessage::NavigateAndFilter(
                                crate::View::Artists,
                                nokkvi_data::types::filter::LibraryFilter::ArtistId {
                                    id: id.clone(),
                                    name: song_artist.clone(),
                                },
                            )
                        });
                        let title_click = Some(SongsMessage::ContextMenuAction(
                            ctx.item_index,
                            crate::widgets::context_menu::LibraryContextEntry::GetInfo,
                        ));
                        slot_list_text_column(
                            song_title,
                            title_click,
                            song_artist,
                            artist_click,
                            title_size,
                            subtitle_size,
                            style,
                            ctx.is_center,
                            title_portion,
                        )
                    },
                ]
                .spacing(6.0)
                .align_y(Alignment::Center);

                if show_album {
                    use crate::widgets::slot_list::slot_list_metadata_column;
                    let album_click = song.album_id.as_ref().map(|id| {
                        SongsMessage::NavigateAndFilter(
                            crate::View::Albums,
                            nokkvi_data::types::filter::LibraryFilter::AlbumId {
                                id: id.clone(),
                                title: song_album.clone(),
                            },
                        )
                    });
                    content_row = content_row.push(slot_list_metadata_column(
                        song_album,
                        album_click,
                        metadata_size,
                        style,
                        ALBUM_PORTION,
                    ));
                }

                if show_stars {
                    use crate::widgets::slot_list::slot_list_star_rating;
                    let star_icon_size = m.title_size;
                    let idx = ctx.item_index;
                    content_row = content_row.push(slot_list_star_rating(
                        rating,
                        star_icon_size,
                        ctx.is_center,
                        ctx.opacity,
                        Some(STARS_PORTION),
                        Some(move |star: usize| SongsMessage::ClickSetRating(idx, star)),
                    ));
                }

                if show_plays {
                    use crate::widgets::slot_list::slot_list_metadata_column;
                    content_row = content_row.push(slot_list_metadata_column(
                        format!("{play_count} plays"),
                        None,
                        metadata_size,
                        style,
                        PLAYS_PORTION,
                    ));
                }

                if show_dynamic_slot {
                    let mut click_msg = None;
                    if current_sort_mode == SortMode::Genre {
                        click_msg = Some(SongsMessage::NavigateAndFilter(
                            crate::View::Genres,
                            nokkvi_data::types::filter::LibraryFilter::GenreId {
                                id: extra_value.clone(),
                                name: extra_value.clone(),
                            },
                        ));
                    }
                    use crate::widgets::slot_list::slot_list_metadata_column;
                    content_row = content_row.push(slot_list_metadata_column(
                        extra_value,
                        click_msg,
                        m.title_size,
                        style,
                        DYNAMIC_PORTION,
                    ));
                }

                if show_duration {
                    let duration_str = formatters::format_time(duration);
                    content_row = content_row.push(
                        container(slot_list_text(
                            duration_str,
                            metadata_size,
                            style.subtext_color,
                        ))
                        .width(Length::FillPortion(DURATION_PORTION))
                        .height(Length::Fill)
                        .align_x(Alignment::End)
                        .align_y(Alignment::Center),
                    );
                }

                if show_love {
                    use crate::widgets::slot_list::slot_list_favorite_icon;
                    content_row = content_row.push(
                        container(slot_list_favorite_icon(
                            is_starred,
                            ctx.is_center,
                            false,
                            ctx.opacity,
                            star_size,
                            "heart",
                            Some(SongsMessage::ClickToggleStar(ctx.item_index)),
                        ))
                        .width(Length::FillPortion(LOVE_PORTION))
                        .padding(iced::Padding {
                            left: 4.0,
                            right: 4.0,
                            ..Default::default()
                        })
                        .align_x(Alignment::Center)
                        .align_y(Alignment::Center),
                    );
                }

                let content = content_row
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
                    context_menu, library_entry_view, song_entries_with_folder,
                };
                let item_idx = ctx.item_index;
                context_menu(
                    slot_button,
                    song_entries_with_folder(),
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
            base_slot_list_layout, single_artwork_panel_with_pill,
        };

        // Build artwork column component - use album artwork of centered song
        let centered_song = center_index.and_then(|idx| songs.get(idx));
        let artwork_handle = centered_song
            .and_then(|song| song.album_id.as_ref())
            .and_then(|album_id| data.large_artwork.get(album_id));
        let active_dominant_color = centered_song
            .and_then(|song| song.album_id.as_ref())
            .and_then(|album_id| data.dominant_colors.get(album_id).copied());

        let on_refresh = centered_song
            .and_then(|song| song.album_id.clone())
            .map(SongsMessage::RefreshArtwork);

        let pill_content = centered_song
            .filter(|_| crate::theme::songs_artwork_overlay())
            .map(|song| {
                use iced::widget::{column, text};

                use crate::theme;

                let mut col = column![
                    text(song.title.clone())
                        .size(20)
                        .font(iced::Font {
                            weight: iced::font::Weight::Bold,
                            ..theme::ui_font()
                        })
                        .color(theme::fg0()),
                    text(song.artist.clone())
                        .size(16)
                        .color(theme::fg1())
                        .font(theme::ui_font()),
                ]
                .spacing(4)
                .align_x(iced::Alignment::Center);

                use crate::widgets::metadata_pill::{
                    auth_status_row, dot_row, play_stats_row, tech_specs_row,
                };

                // Row 1: Track • Year • Genre
                let mut info_stats = Vec::new();
                if let Some(track) = song.track {
                    info_stats.push(format!("Track {track}"));
                }
                if let Some(year) = song.year {
                    info_stats.push(year.to_string());
                }
                if let Some(genre) = &song.genre {
                    info_stats.push(genre.clone());
                }
                if let Some(row) = dot_row::<SongsMessage>(info_stats, 13.0, theme::fg2()) {
                    col = col.push(row);
                }

                // Row 2: Plays • Last played
                if let Some(row) =
                    play_stats_row::<SongsMessage>(song.play_count, song.play_date.as_deref())
                {
                    col = col.push(row);
                }

                // Row 3: Favorited / Rating
                if let Some(row) = auth_status_row::<SongsMessage>(song.is_starred, song.rating) {
                    col = col.push(row);
                }

                // Row 4: Tech specs
                if let Some(row) = tech_specs_row::<SongsMessage>(
                    song.suffix.as_deref(),
                    song.bit_depth,
                    song.sample_rate,
                    song.bitrate,
                    song.bpm,
                ) {
                    col = col.push(row);
                }

                col.into()
            });

        let artwork_content = Some(single_artwork_panel_with_pill(
            artwork_handle,
            pill_content,
            active_dominant_color,
            on_refresh,
        ));

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn songs_column_visibility_default_preserves_today_behavior() {
        let v = SongsColumnVisibility::default();
        assert!(!v.stars);
        assert!(v.album);
        assert!(v.duration);
        assert!(!v.plays);
        assert!(v.love);
    }

    #[test]
    fn songs_stars_visible_auto_shows_on_rating_sort() {
        assert!(songs_stars_visible(SortMode::Rating, false));
        assert!(songs_stars_visible(SortMode::Rating, true));
        assert!(!songs_stars_visible(SortMode::Title, false));
        assert!(songs_stars_visible(SortMode::Title, true));
    }

    #[test]
    fn songs_plays_visible_auto_shows_on_most_played() {
        assert!(songs_plays_visible(SortMode::MostPlayed, false));
        assert!(songs_plays_visible(SortMode::MostPlayed, true));
        assert!(!songs_plays_visible(SortMode::Title, false));
        assert!(songs_plays_visible(SortMode::Title, true));
    }

    #[test]
    fn songs_toggle_column_visible_flips_state_and_emits_action() {
        let mut page = SongsPage::default();
        let empty: Vec<SongUIViewData> = vec![];
        let (_t, action) = page.update(
            SongsMessage::ToggleColumnVisible(SongsColumn::Plays),
            &empty,
        );
        assert!(page.column_visibility.plays);
        assert!(matches!(
            action,
            SongsAction::ColumnVisibilityChanged(SongsColumn::Plays, true)
        ));
    }
}
