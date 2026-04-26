//! Albums Page Component
//!
//! Self-contained albums view with slot list navigation, search, and filtering.
//! Uses message bubbling pattern to communicate global actions to root.
//! Supports inline track expansion (Shift+Enter) using flattened SlotListEntry list.

use std::collections::HashMap;

use iced::{
    Alignment, Element, Length, Task,
    widget::{button, container, image, row, text},
};
use nokkvi_data::{
    backend::{albums::AlbumUIViewData, songs::SongUIViewData},
    utils::formatters,
};

use super::expansion::{ExpansionState, SlotListEntry};
use crate::{
    app_message::Message,
    widgets::{self, SlotListPageState, view_header::SortMode},
};

/// Albums page local state
#[derive(Debug)]
pub struct AlbumsPage {
    pub common: SlotListPageState,
    /// Inline expansion state (album → tracks)
    pub expansion: ExpansionState<SongUIViewData>,
}

/// View data passed from root (read-only, borrows from app state to avoid allocations)
pub struct AlbumsViewData<'a> {
    pub albums: &'a [AlbumUIViewData],
    pub album_art: &'a HashMap<String, image::Handle>,
    pub large_artwork: &'a HashMap<String, image::Handle>,
    pub dominant_colors: &'a HashMap<String, iced::Color>,
    pub window_width: f32,
    pub window_height: f32,
    pub scale_factor: f32,
    pub modifiers: iced::keyboard::Modifiers,
    pub total_album_count: usize,
    pub loading: bool,
    pub stable_viewport: bool,
}

/// Messages for local album page interactions
#[derive(Debug, Clone)]
pub enum AlbumsMessage {
    // Slot list navigation
    SlotListNavigateUp,
    SlotListNavigateDown,
    SlotListSetOffset(usize, iced::keyboard::Modifiers),
    SlotListScrollSeek(usize),
    SlotListActivateCenter,
    SlotListClickPlay(usize), // Click non-center to play directly (skip focus)
    AddCenterToQueue,         // Add centered album to queue (Shift+Q)

    // Mouse click on star/heart (item_index, value)
    ClickSetRating(usize, usize), // (item_index, rating 1-5)
    ClickToggleStar(usize),       // item_index

    // Context menu
    ContextMenuAction(usize, crate::widgets::context_menu::LibraryContextEntry),

    CenterOnPlaying,

    // Inline expansion (Shift+Enter)
    ExpandCenter,
    FocusAndExpand(usize), // Clicked 'X songs' — focus that row and expand it
    CollapseExpansion,
    /// Tracks loaded for expanded album (album_id, tracks)
    TracksLoaded(String, Vec<SongUIViewData>),

    // View header
    SortModeSelected(widgets::view_header::SortMode),
    ToggleSortOrder,
    SearchQueryChanged(String),
    SearchFocused(bool),
    RefreshViewData,

    // Data loading (moved from root Message enum)
    AlbumsLoaded {
        result: Result<Vec<AlbumUIViewData>, String>,
        total_count: usize,
        background: bool,
        anchor_id: Option<String>,
    },
    AlbumsPageLoaded(Result<Vec<AlbumUIViewData>, String>, usize), // result, total_count (subsequent page)

    // Artwork loading (moved from root Message enum)
    /// Album artwork loaded (album_id, handle)
    ArtworkLoaded(String, Option<image::Handle>),
    /// Large album artwork loaded (album_id, handle)
    LargeArtworkLoaded(String, Option<image::Handle>),
    /// Refresh artwork for a specific album (album_id)
    RefreshArtwork(String),

    /// Navigate to a view and apply an ID filter
    NavigateAndFilter(crate::View, nokkvi_data::types::filter::LibraryFilter),
}

/// Actions that bubble up to root for global state mutation
#[derive(Debug, Clone)]
pub enum AlbumsAction {
    PlayAlbum(String), // album_id - clear queue and play
    PlayBatch(nokkvi_data::types::batch::BatchPayload),
    AddBatchToQueue(nokkvi_data::types::batch::BatchPayload),
    LoadLargeArtwork(String), // center_idx as string
    CenterOnPlaying,
    /// Expand album inline — root should load tracks (album_id)
    ExpandAlbum(String),
    /// Play batch starting from a specific track (album_id, track_index)
    PlayAlbumFromTrack(String, usize),
    /// Set rating on item (item_id, item_type "album"|"song", rating)
    SetRating(String, &'static str, usize),
    /// Star/unstar item (item_id, item_type, new_starred)
    ToggleStar(String, &'static str, bool),

    LoadPage(usize),       // offset - trigger fetch of next page
    SearchChanged(String), // trigger reload
    SortModeChanged(widgets::view_header::SortMode), // trigger reload
    SortOrderChanged(bool), // trigger reload
    RefreshViewData,       // trigger reload
    PlayNext(String),      // album_id - insert after currently playing
    AddBatchToPlaylist(nokkvi_data::types::batch::BatchPayload),
    ShowInfo(Box<nokkvi_data::types::info_modal::InfoModalItem>), // Open info modal
    ShowInFolder(String), // album_id - fetch a song path and open containing folder
    ShowSongInFolder(String), // song path - open containing folder directly (expansion child)
    RefreshArtwork(String), // album_id - refresh artwork from server
    FindSimilar(String, String), // (entity_id, label) - open similar tab
    NavigateAndFilter(crate::View, nokkvi_data::types::filter::LibraryFilter), // Navigate to target view and filter
    None,
}

impl super::HasCommonAction for AlbumsAction {
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

impl Default for AlbumsPage {
    fn default() -> Self {
        Self {
            common: SlotListPageState::new(
                widgets::view_header::SortMode::RecentlyAdded,
                false, // sort_ascending
            ),
            expansion: ExpansionState::default(),
        }
    }
}

impl AlbumsPage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Convert sort mode to API string for server requests
    pub fn sort_mode_to_api_string(
        sort_mode: crate::widgets::view_header::SortMode,
    ) -> &'static str {
        use crate::widgets::view_header::SortMode;
        match sort_mode {
            SortMode::RecentlyAdded => "recentlyAdded",
            SortMode::RecentlyPlayed => "recentlyPlayed",
            SortMode::MostPlayed => "mostPlayed",
            SortMode::Favorited => "favorited",
            SortMode::Random => "random",
            SortMode::Name => "name",
            SortMode::AlbumArtist => "albumArtist",
            SortMode::Artist => "artist",
            SortMode::ReleaseYear => "year",
            SortMode::SongCount => "songCount",
            SortMode::Duration => "duration",
            SortMode::Rating => "rating",
            SortMode::Genre => "genre",
            SortMode::AlbumCount => "albumCount",
            _ => "recentlyAdded", // Fallback for song-specific sort modes
        }
    }

    /// Update internal state and return actions for root
    pub fn update(
        &mut self,
        message: AlbumsMessage,
        total_items: usize,
        albums: &[AlbumUIViewData],
    ) -> (Task<AlbumsMessage>, AlbumsAction) {
        match super::impl_expansion_update!(
            self, message, albums, total_items,
            id_fn: |a| &a.id,
            expand_center: AlbumsMessage::ExpandCenter => AlbumsAction::ExpandAlbum,
            collapse: AlbumsMessage::CollapseExpansion,
            children_loaded: AlbumsMessage::TracksLoaded,
            sort_selected: AlbumsMessage::SortModeSelected => AlbumsAction::SortModeChanged,
            toggle_sort: AlbumsMessage::ToggleSortOrder => AlbumsAction::SortOrderChanged,
            search_changed: AlbumsMessage::SearchQueryChanged => AlbumsAction::SearchChanged,
            search_focused: AlbumsMessage::SearchFocused,
            action_none: AlbumsAction::None,
        ) {
            Ok(result) => result,
            Err(msg) => match msg {
                AlbumsMessage::SlotListNavigateUp => {
                    let center = self.expansion.handle_navigate_up(albums, &mut self.common);
                    match center {
                        Some(idx) => (
                            Task::none(),
                            AlbumsAction::LoadLargeArtwork(idx.to_string()),
                        ),
                        None => (Task::none(), AlbumsAction::None),
                    }
                }
                AlbumsMessage::SlotListNavigateDown => {
                    let center = self
                        .expansion
                        .handle_navigate_down(albums, &mut self.common);
                    match center {
                        Some(idx) => (
                            Task::none(),
                            AlbumsAction::LoadLargeArtwork(idx.to_string()),
                        ),
                        None => (Task::none(), AlbumsAction::None),
                    }
                }
                AlbumsMessage::SlotListSetOffset(offset, modifiers) => {
                    let center = self.expansion.handle_select_offset(
                        offset,
                        modifiers,
                        albums,
                        &mut self.common,
                    );
                    match center {
                        Some(idx) => (
                            Task::none(),
                            AlbumsAction::LoadLargeArtwork(idx.to_string()),
                        ),
                        None => (Task::none(), AlbumsAction::None),
                    }
                }
                AlbumsMessage::FocusAndExpand(offset) => {
                    let center = self.expansion.handle_select_offset(
                        offset,
                        Default::default(),
                        albums,
                        &mut self.common,
                    );
                    if let Some(idx) = center {
                        // Now expand it
                        if let Some(parent_id) =
                            self.expansion
                                .handle_expand_center(albums, |a| &a.id, &mut self.common)
                        {
                            (Task::none(), AlbumsAction::ExpandAlbum(parent_id))
                        } else {
                            (
                                Task::none(),
                                AlbumsAction::LoadLargeArtwork(idx.to_string()),
                            )
                        }
                    } else {
                        (Task::none(), AlbumsAction::None)
                    }
                }
                AlbumsMessage::SlotListScrollSeek(offset) => {
                    self.expansion
                        .handle_set_offset(offset, albums, &mut self.common);
                    (Task::none(), AlbumsAction::None)
                }
                AlbumsMessage::SlotListClickPlay(offset) => {
                    // Set offset then activate (play without focusing)
                    self.expansion
                        .handle_set_offset(offset, albums, &mut self.common);
                    self.update(AlbumsMessage::SlotListActivateCenter, total_items, albums)
                }
                AlbumsMessage::SlotListActivateCenter => {
                    let total = self.expansion.flattened_len(albums);
                    let center_idx = self.common.get_center_item_index(total);
                    let target_indices = self
                        .common
                        .slot_list
                        .selected_indices
                        .iter()
                        .copied()
                        .collect::<Vec<_>>();

                    if !target_indices.is_empty() {
                        use nokkvi_data::types::batch::{BatchItem, BatchPayload};
                        let payload = target_indices
                            .into_iter()
                            .filter_map(|i| {
                                match self.expansion.get_entry_at(i, albums, |a| &a.id) {
                                    Some(SlotListEntry::Parent(album)) => {
                                        Some(BatchItem::Album(album.id.clone()))
                                    }
                                    Some(SlotListEntry::Child(song, _)) => {
                                        let item: nokkvi_data::types::song::Song =
                                            song.clone().into();
                                        Some(BatchItem::Song(Box::new(item)))
                                    }
                                    None => None,
                                }
                            })
                            .fold(BatchPayload::new(), |p, item| p.with_item(item));
                        return (Task::none(), AlbumsAction::PlayBatch(payload));
                    }

                    if let Some(center_idx) = center_idx {
                        self.common.slot_list.flash_center();
                        match self.expansion.get_entry_at(center_idx, albums, |a| &a.id) {
                            Some(SlotListEntry::Child(_song, parent_album_id)) => {
                                let track_index =
                                    self.expansion
                                        .count_children_before(center_idx, albums, |a| &a.id);
                                (
                                    Task::none(),
                                    AlbumsAction::PlayAlbumFromTrack(parent_album_id, track_index),
                                )
                            }
                            Some(SlotListEntry::Parent(_)) => (
                                Task::none(),
                                AlbumsAction::PlayAlbum(center_idx.to_string()),
                            ),
                            None => (Task::none(), AlbumsAction::None),
                        }
                    } else {
                        (Task::none(), AlbumsAction::None)
                    }
                }
                AlbumsMessage::AddCenterToQueue => {
                    use nokkvi_data::types::batch::BatchItem;

                    let total = self.expansion.flattened_len(albums);
                    let target_indices = self.common.get_queue_target_indices(total);

                    if target_indices.is_empty() {
                        return (Task::none(), AlbumsAction::None);
                    }

                    let payload = super::expansion::build_batch_payload(target_indices, |i| {
                        match self.expansion.get_entry_at(i, albums, |a| &a.id) {
                            Some(SlotListEntry::Parent(album)) => {
                                Some(BatchItem::Album(album.id.clone()))
                            }
                            Some(SlotListEntry::Child(song, _)) => {
                                let item: nokkvi_data::types::song::Song = song.clone().into();
                                Some(BatchItem::Song(Box::new(item)))
                            }
                            None => None,
                        }
                    });

                    (Task::none(), AlbumsAction::AddBatchToQueue(payload))
                }
                // Data loading messages (handled at root level, no action needed here)
                AlbumsMessage::AlbumsLoaded { .. } => (Task::none(), AlbumsAction::None),
                AlbumsMessage::AlbumsPageLoaded(_, _) => (Task::none(), AlbumsAction::None),
                AlbumsMessage::ArtworkLoaded(_, _) => (Task::none(), AlbumsAction::None),
                AlbumsMessage::LargeArtworkLoaded(_, _) => (Task::none(), AlbumsAction::None),
                AlbumsMessage::RefreshViewData => (Task::none(), AlbumsAction::RefreshViewData),
                AlbumsMessage::RefreshArtwork(album_id) => {
                    (Task::none(), AlbumsAction::RefreshArtwork(album_id))
                }
                AlbumsMessage::ClickSetRating(item_index, rating) => {
                    if let Some(entry) = self.expansion.get_entry_at(item_index, albums, |a| &a.id)
                    {
                        use nokkvi_data::utils::formatters::compute_rating_toggle;
                        match entry {
                            SlotListEntry::Child(song, _) => {
                                let current = song.rating.unwrap_or(0) as usize;
                                let new_rating = compute_rating_toggle(current, rating);
                                (
                                    Task::none(),
                                    AlbumsAction::SetRating(song.id.clone(), "song", new_rating),
                                )
                            }
                            SlotListEntry::Parent(album) => {
                                let current = album.rating.unwrap_or(0) as usize;
                                let new_rating = compute_rating_toggle(current, rating);
                                (
                                    Task::none(),
                                    AlbumsAction::SetRating(album.id.clone(), "album", new_rating),
                                )
                            }
                        }
                    } else {
                        (Task::none(), AlbumsAction::None)
                    }
                }
                AlbumsMessage::ClickToggleStar(item_index) => {
                    if let Some(entry) = self.expansion.get_entry_at(item_index, albums, |a| &a.id)
                    {
                        match entry {
                            SlotListEntry::Child(song, _) => (
                                Task::none(),
                                AlbumsAction::ToggleStar(song.id.clone(), "song", !song.is_starred),
                            ),
                            SlotListEntry::Parent(album) => (
                                Task::none(),
                                AlbumsAction::ToggleStar(
                                    album.id.clone(),
                                    "album",
                                    !album.is_starred,
                                ),
                            ),
                        }
                    } else {
                        (Task::none(), AlbumsAction::None)
                    }
                }
                AlbumsMessage::ContextMenuAction(clicked_idx, entry) => {
                    use nokkvi_data::types::batch::BatchItem;

                    use crate::widgets::context_menu::LibraryContextEntry;

                    match entry {
                        LibraryContextEntry::AddToQueue | LibraryContextEntry::AddToPlaylist => {
                            let target_indices = self.common.get_batch_target_indices(clicked_idx);
                            let payload = super::expansion::build_batch_payload(
                                target_indices,
                                |i| match self.expansion.get_entry_at(i, albums, |a| &a.id) {
                                    Some(SlotListEntry::Parent(album)) => {
                                        Some(BatchItem::Album(album.id.clone()))
                                    }
                                    Some(SlotListEntry::Child(song, _)) => {
                                        let item: nokkvi_data::types::song::Song =
                                            song.clone().into();
                                        Some(BatchItem::Song(Box::new(item)))
                                    }
                                    None => None,
                                },
                            );

                            match entry {
                                LibraryContextEntry::AddToQueue => {
                                    (Task::none(), AlbumsAction::AddBatchToQueue(payload))
                                }
                                LibraryContextEntry::AddToPlaylist => {
                                    (Task::none(), AlbumsAction::AddBatchToPlaylist(payload))
                                }
                                _ => unreachable!(),
                            }
                        }
                        // Non-batched actions (apply only to the clicked item)
                        _ => match self.expansion.get_entry_at(clicked_idx, albums, |a| &a.id) {
                            Some(SlotListEntry::Parent(album)) => match entry {
                                LibraryContextEntry::GetInfo => {
                                    use nokkvi_data::types::info_modal::InfoModalItem;
                                    let item = InfoModalItem::Album {
                                        name: album.name.clone(),
                                        album_artist: Some(album.artist.clone()),
                                        release_type: album.release_type.clone(),
                                        genre: album.genre.clone(),
                                        genres: album.genres.clone(),
                                        duration: album.duration,
                                        year: album.year,
                                        song_count: Some(album.song_count),
                                        compilation: album.compilation,
                                        size: album.size,
                                        is_starred: album.is_starred,
                                        rating: album.rating,
                                        play_count: album.play_count,
                                        play_date: album.play_date.clone(),
                                        updated_at: album.updated_at.clone(),
                                        created_at: album.created_at.clone(),
                                        mbz_album_id: album.mbz_album_id.clone(),
                                        comment: album.comment.clone(),
                                        id: album.id.clone(),
                                        tags: album.tags.clone(),
                                        participants: album.participants.clone(),
                                        representative_path: self
                                            .expansion
                                            .children
                                            .first()
                                            .map(|s| s.path.clone()),
                                    };
                                    (Task::none(), AlbumsAction::ShowInfo(Box::new(item)))
                                }
                                LibraryContextEntry::ShowInFolder => {
                                    (Task::none(), AlbumsAction::ShowInFolder(album.id.clone()))
                                }
                                LibraryContextEntry::FindSimilar => (
                                    Task::none(),
                                    AlbumsAction::FindSimilar(
                                        album.artist.clone(),
                                        format!("Similar to: {}", album.name),
                                    ),
                                ),
                                LibraryContextEntry::Separator => {
                                    (Task::none(), AlbumsAction::None)
                                }
                                _ => (Task::none(), AlbumsAction::None),
                            },
                            Some(SlotListEntry::Child(song, _)) => match entry {
                                LibraryContextEntry::GetInfo => {
                                    use nokkvi_data::types::info_modal::InfoModalItem;
                                    let item = InfoModalItem::from_song_view_data(song);
                                    (Task::none(), AlbumsAction::ShowInfo(Box::new(item)))
                                }
                                LibraryContextEntry::ShowInFolder => (
                                    Task::none(),
                                    AlbumsAction::ShowSongInFolder(song.path.clone()),
                                ),
                                LibraryContextEntry::FindSimilar => (
                                    Task::none(),
                                    AlbumsAction::FindSimilar(
                                        song.id.clone(),
                                        format!("Similar to: {}", song.title),
                                    ),
                                ),
                                LibraryContextEntry::Separator => {
                                    (Task::none(), AlbumsAction::None)
                                }
                                _ => (Task::none(), AlbumsAction::None),
                            },
                            None => (Task::none(), AlbumsAction::None),
                        },
                    }
                }
                // Common arms already handled by macro above
                AlbumsMessage::CenterOnPlaying => (Task::none(), AlbumsAction::CenterOnPlaying),
                AlbumsMessage::NavigateAndFilter(view, filter) => {
                    (Task::none(), AlbumsAction::NavigateAndFilter(view, filter))
                }
                _ => (Task::none(), AlbumsAction::None),
            },
        }
    }

    // NOTE: build_flattened_list, collapse, clear are now on self.expansion (ExpansionState)

    /// Build the view
    pub fn view<'a>(&'a self, data: AlbumsViewData<'a>) -> Element<'a, AlbumsMessage> {
        use crate::widgets::view_header::SortMode;

        let header = widgets::view_header::view_header(
            self.common.current_sort_mode,
            SortMode::ALBUM_OPTIONS,
            self.common.sort_ascending,
            &self.common.search_query,
            data.albums.len(),
            data.total_album_count,
            "albums",
            crate::views::ALBUMS_SEARCH_ID,
            AlbumsMessage::SortModeSelected,
            Some(AlbumsMessage::ToggleSortOrder),
            None, // No shuffle button for albums
            Some(AlbumsMessage::RefreshViewData),
            Some(AlbumsMessage::CenterOnPlaying),
            None, // on_add
            None, // trailing_button
            true, // show_search
            AlbumsMessage::SearchQueryChanged,
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

        // If no albums match search, show message but keep the header
        if data.albums.is_empty() {
            return widgets::base_slot_list_empty_state(
                header,
                "No albums match your search.",
                &layout_config,
            );
        }

        // Configure slot list with albums-specific chrome height (has view header)
        use crate::widgets::slot_list::{
            SlotListConfig, chrome_height_with_header, slot_list_view_with_scroll,
        };

        let config =
            SlotListConfig::with_dynamic_slots(data.window_height, chrome_height_with_header())
                .with_modifiers(data.modifiers);

        // Capture values needed in closure
        let _scale_factor = data.scale_factor;
        let albums = data.albums; // Borrow slice to extend lifetime
        let album_art = data.album_art;
        let current_sort_mode = self.common.current_sort_mode;

        // Build flattened list (albums + injected tracks when expanded)
        let flattened = self.expansion.build_flattened_list(albums, |a| &a.id);
        let center_index = self.common.get_center_item_index(flattened.len());

        // Render slot list using generic component with item renderer closure
        let slot_list_content = slot_list_view_with_scroll(
            &self.common.slot_list,
            &flattened,
            &config,
            AlbumsMessage::SlotListNavigateUp,
            AlbumsMessage::SlotListNavigateDown,
            {
                let total = flattened.len();
                move |f| AlbumsMessage::SlotListScrollSeek((f * total as f32) as usize)
            },
            |entry, ctx| match entry {
                SlotListEntry::Parent(album) => self.render_album_row(
                    album,
                    &ctx,
                    album_art,
                    current_sort_mode,
                    data.stable_viewport,
                ),
                SlotListEntry::Child(song, _parent_album_id) => {
                    self.render_track_row(song, &ctx, data.stable_viewport)
                }
            },
        );

        // Wrap slot list content with standard background (prevents color bleed-through)
        use crate::widgets::slot_list::slot_list_background_container;
        let slot_list_content = slot_list_background_container(slot_list_content);

        // Use base slot list layout with artwork column
        use crate::widgets::base_slot_list_layout::{
            base_slot_list_layout, single_artwork_panel_with_pill,
        };

        // Build artwork column component — show parent album art even when on a child track
        let centered_album = center_index.and_then(|idx| match flattened.get(idx) {
            Some(SlotListEntry::Parent(album)) => Some(album),
            Some(SlotListEntry::Child(_, parent_id)) => albums.iter().find(|a| &a.id == parent_id),
            None => None,
        });

        let artwork_handle = centered_album.and_then(|album| data.large_artwork.get(&album.id));
        let active_dominant_color =
            centered_album.and_then(|album| data.dominant_colors.get(&album.id).copied());

        let on_refresh =
            centered_album.map(|album| AlbumsMessage::RefreshArtwork(album.id.clone()));

        // Overlay building
        let overlay_content = centered_album.map(|album| {
            use iced::widget::{column, text};

            use crate::theme;

            let mut col = column![
                text(album.name.clone())
                    .size(24)
                    .font(iced::Font {
                        weight: iced::font::Weight::Bold,
                        ..theme::ui_font()
                    })
                    .color(theme::fg0()),
                text(album.artist.clone())
                    .size(16)
                    .color(theme::fg1())
                    .font(theme::ui_font()),
            ]
            .spacing(4)
            .align_x(iced::Alignment::Center);

            // Date Resolution (Feishin logic cascade)
            let date_text = if let Some(orig_date) = &album.original_date {
                nokkvi_data::utils::formatters::format_release_date(orig_date)
            } else if let Some(rel_date) = &album.release_date {
                nokkvi_data::utils::formatters::format_release_date(rel_date)
            } else if let Some(year) = album.original_year.or(album.year) {
                year.to_string()
            } else {
                String::new()
            };

            let mut info_stats = Vec::new();
            if !date_text.is_empty() {
                let mut full_date = date_text;
                if let (Some(orig_yr), Some(yr)) = (album.original_year, album.year)
                    && orig_yr != yr
                {
                    full_date = format!("{full_date} ({yr})");
                }
                info_stats.push(full_date);
            }

            let count = album.song_count;
            if count > 0 {
                info_stats.push(format!("{count} tracks"));
            }

            if let Some(secs) = album.duration {
                info_stats.push(nokkvi_data::utils::formatters::format_duration_short(secs));
            }

            use crate::widgets::metadata_pill::{auth_status_row, dot_row, play_stats_row};

            if let Some(row) = dot_row::<AlbumsMessage>(info_stats, 14.0, theme::fg2()) {
                col = col.push(row);
            }

            // Genre row
            if let Some(genres_display) = &album.genres {
                col = col.push(
                    text(genres_display.clone())
                        .size(13)
                        .color(theme::fg3())
                        .font(theme::ui_font()),
                );
            }

            if let Some(row) =
                play_stats_row::<AlbumsMessage>(album.play_count, album.play_date.as_deref())
            {
                col = col.push(row);
            }

            if let Some(row) = auth_status_row::<AlbumsMessage>(album.is_starred, album.rating) {
                col = col.push(row);
            }

            col.into()
        });

        let artwork_content = Some(single_artwork_panel_with_pill(
            artwork_handle,
            overlay_content,
            active_dominant_color,
            on_refresh,
        ));

        base_slot_list_layout(&layout_config, header, slot_list_content, artwork_content)
    }

    /// Render an album row in the slot list (existing album layout)
    fn render_album_row<'a>(
        &self,
        album: &AlbumUIViewData,
        ctx: &crate::widgets::slot_list::SlotListRowContext,
        album_art: &'a HashMap<String, image::Handle>,
        current_sort_mode: SortMode,
        stable_viewport: bool,
    ) -> Element<'a, AlbumsMessage> {
        use crate::widgets::slot_list::{
            SLOT_LIST_SLOT_PADDING, SlotListSlotStyle, slot_list_index_column,
        };

        let album_id = album.id.clone();
        let album_name = album.name.clone();
        let album_artist = album.artist.clone();
        let song_count = album.song_count;
        let is_starred = album.is_starred;
        let rating = album.rating.unwrap_or(0).min(5) as usize;
        let extra_value = get_extra_column_value(album, current_sort_mode);

        // Check if this album is the expanded one
        let is_expanded = self.expansion.is_expanded_parent(&album.id);
        let style = SlotListSlotStyle::for_slot(
            ctx.is_center,
            is_expanded,
            ctx.is_selected,
            ctx.has_multi_selection,
            ctx.opacity,
            0,
        );

        let m = ctx.metrics;
        let artwork_size = m.artwork_size;
        let title_size = m.title_size_lg;
        let subtitle_size = m.subtitle_size;
        let song_count_size = m.metadata_size;
        let star_size = m.star_size;
        let index_size = m.metadata_size;

        let content = row![
            slot_list_index_column(ctx.item_index, index_size, style, ctx.opacity),
            {
                use crate::widgets::slot_list::slot_list_artwork_column;
                slot_list_artwork_column(
                    album_art.get(&album_id),
                    artwork_size,
                    ctx.is_center,
                    false,
                    ctx.opacity,
                )
            },
            {
                use crate::widgets::slot_list::slot_list_text_column;
                let artist_click = Some(AlbumsMessage::NavigateAndFilter(
                    crate::View::Artists,
                    nokkvi_data::types::filter::LibraryFilter::ArtistId {
                        id: album.artist_id.clone(),
                        name: album_artist.clone(),
                    },
                ));
                let title_click = Some(AlbumsMessage::ContextMenuAction(
                    ctx.item_index,
                    crate::widgets::context_menu::LibraryContextEntry::GetInfo,
                ));
                slot_list_text_column(
                    album_name,
                    title_click,
                    album_artist,
                    artist_click,
                    title_size,
                    subtitle_size,
                    style,
                    ctx.is_center,
                    50,
                )
            },
            {
                let idx = ctx.item_index;
                use crate::widgets::slot_list::slot_list_metadata_column;
                slot_list_metadata_column(
                    format!("{song_count} songs"),
                    Some(AlbumsMessage::FocusAndExpand(idx)),
                    song_count_size,
                    style,
                    22,
                )
            },
            {
                if current_sort_mode == SortMode::Rating {
                    let star_icon_size = m.title_size;
                    let idx = ctx.item_index;
                    use crate::widgets::slot_list::slot_list_star_rating;
                    slot_list_star_rating(
                        rating,
                        star_icon_size,
                        ctx.is_center,
                        ctx.opacity,
                        Some(21),
                        Some(move |star: usize| AlbumsMessage::ClickSetRating(idx, star)),
                    )
                } else if !extra_value.is_empty() {
                    let mut click_msg = None;
                    if current_sort_mode == SortMode::Genre {
                        click_msg = Some(AlbumsMessage::NavigateAndFilter(
                            crate::View::Genres,
                            nokkvi_data::types::filter::LibraryFilter::GenreId {
                                id: extra_value.clone(),
                                name: extra_value.clone(),
                            },
                        ));
                    }
                    use crate::widgets::slot_list::slot_list_metadata_column;
                    slot_list_metadata_column(extra_value, click_msg, m.title_size, style, 21)
                } else {
                    container(text("")).width(Length::FillPortion(21)).into()
                }
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
                    Some(AlbumsMessage::ClickToggleStar(ctx.item_index)),
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
            .on_press(if ctx.modifiers.control() || ctx.modifiers.shift() {
                AlbumsMessage::SlotListSetOffset(ctx.item_index, ctx.modifiers)
            } else if ctx.is_center {
                AlbumsMessage::SlotListActivateCenter
            } else if stable_viewport {
                AlbumsMessage::SlotListSetOffset(ctx.item_index, ctx.modifiers)
            } else {
                AlbumsMessage::SlotListClickPlay(ctx.item_index)
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
                    AlbumsMessage::ContextMenuAction(item_idx, e)
                })
            },
        )
        .into()
    }

    /// Render a child track row in the slot list (indented, simpler layout)
    fn render_track_row<'a>(
        &self,
        song: &SongUIViewData,
        ctx: &crate::widgets::slot_list::SlotListRowContext,
        stable_viewport: bool,
    ) -> Element<'a, AlbumsMessage> {
        let track_el = super::expansion::render_child_track_row(
            song,
            ctx,
            AlbumsMessage::SlotListActivateCenter,
            if stable_viewport {
                AlbumsMessage::SlotListSetOffset(ctx.item_index, ctx.modifiers)
            } else {
                AlbumsMessage::SlotListClickPlay(ctx.item_index)
            },
            Some(AlbumsMessage::ClickToggleStar(ctx.item_index)),
            song.artist_id.as_ref().map(|id| {
                AlbumsMessage::NavigateAndFilter(
                    crate::View::Artists,
                    nokkvi_data::types::filter::LibraryFilter::ArtistId {
                        id: id.clone(),
                        name: song.artist.clone(),
                    },
                )
            }),
            1, // depth 1: child tracks under album
        );

        use crate::widgets::context_menu::{
            context_menu, library_entry_view, song_entries_with_folder,
        };
        let item_idx = ctx.item_index;
        context_menu(
            track_el,
            song_entries_with_folder(),
            move |entry, length| {
                library_entry_view(entry, length, |e| {
                    AlbumsMessage::ContextMenuAction(item_idx, e)
                })
            },
        )
        .into()
    }
}

/// Get extra column value based on current sort mode (matches QML getExtraColumnData)
fn get_extra_column_value(album: &AlbumUIViewData, sort_mode: SortMode) -> String {
    match sort_mode {
        SortMode::RecentlyAdded => {
            album.created_at.as_ref()
                .and_then(|d| formatters::format_date(d).ok())
                .unwrap_or_default()
        }
        SortMode::RecentlyPlayed => {
            album.play_date.as_ref().map_or_else(|| "never".to_string(), |d| d.split('T').next().unwrap_or(d).to_string())
        }
        SortMode::MostPlayed => {
            let count = album.play_count.unwrap_or(0);
            format!("{count} plays")
        }
        SortMode::ReleaseYear => {
            album.year
                .map(|y| y.to_string())
                .unwrap_or_default()
        }
        SortMode::Duration => {
            album.duration
                .map(|d| formatters::format_time(d as u32))
                .unwrap_or_default()
        }
        SortMode::Genre => {
            album.genre
                .clone()
                .unwrap_or_default()
        }
        // No extra column for these views (they sort by name/artist already visible)
        SortMode::Favorited | SortMode::Random | SortMode::Name |
        SortMode::AlbumArtist | SortMode::Artist | SortMode::SongCount |
        SortMode::Rating | SortMode::AlbumCount |
        // Song-specific sort modes (not applicable to albums)
        SortMode::Title | SortMode::Album | SortMode::Bpm |
        SortMode::Channels | SortMode::Comment | SortMode::UpdatedAt => String::new(),
    }
}

// ============================================================================
// ViewPage trait implementation
// ============================================================================

impl super::ViewPage for AlbumsPage {
    fn common(&self) -> &SlotListPageState {
        &self.common
    }
    fn common_mut(&mut self) -> &mut SlotListPageState {
        &mut self.common
    }

    fn is_expanded(&self) -> bool {
        self.expansion.is_expanded()
    }
    fn collapse_expansion_message(&self) -> Option<Message> {
        Some(Message::Albums(AlbumsMessage::CollapseExpansion))
    }

    fn search_input_id(&self) -> &'static str {
        super::ALBUMS_SEARCH_ID
    }

    fn sort_mode_options(&self) -> Option<&'static [SortMode]> {
        Some(SortMode::ALBUM_OPTIONS)
    }
    fn sort_mode_selected_message(&self, mode: SortMode) -> Option<Message> {
        Some(Message::Albums(AlbumsMessage::SortModeSelected(mode)))
    }
    fn toggle_sort_order_message(&self) -> Message {
        Message::Albums(AlbumsMessage::ToggleSortOrder)
    }

    fn add_to_queue_message(&self) -> Option<Message> {
        Some(Message::Albums(AlbumsMessage::AddCenterToQueue))
    }
    fn expand_center_message(&self) -> Option<Message> {
        Some(Message::Albums(AlbumsMessage::ExpandCenter))
    }
    fn reload_message(&self) -> Option<Message> {
        Some(Message::LoadAlbums)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_on_playing_translates_to_action() {
        let mut page = AlbumsPage::new();
        let empty_albums: Vec<AlbumUIViewData> = vec![];
        let (_, action) = page.update(AlbumsMessage::CenterOnPlaying, 0, &empty_albums);

        assert!(matches!(action, AlbumsAction::CenterOnPlaying));
    }
}
