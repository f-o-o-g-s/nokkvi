//! Albums Page Component
//!
//! Self-contained albums view with slot list navigation, search, and filtering.
//! Uses message bubbling pattern to communicate global actions to root.
//! Supports inline track expansion (Shift+Enter) using flattened SlotListEntry list.

use std::collections::HashMap;

use iced::{
    Alignment, Element, Length, Task,
    widget::{Row, button, container, image},
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
    /// Per-column visibility toggles surfaced via the columns-cog dropdown.
    pub column_visibility: AlbumsColumnVisibility,
}

/// Toggleable albums columns. Index/Art/Title+Artist are always shown.
/// The dynamic 21% slot still auto-renders Date/Year/Duration/Genre when
/// sorted by those modes — Stars and Plays are now dedicated columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlbumsColumn {
    Select,
    Index,
    Thumbnail,
    Stars,
    SongCount,
    Plays,
    Love,
}

/// User-toggle state for each toggleable albums column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlbumsColumnVisibility {
    pub select: bool,
    pub index: bool,
    pub thumbnail: bool,
    pub stars: bool,
    pub songcount: bool,
    pub plays: bool,
    pub love: bool,
}

impl Default for AlbumsColumnVisibility {
    fn default() -> Self {
        // Stars and Plays default off — today they only appear when their
        // sort mode is active. SongCount and Love default on (always-shown
        // today). Index/Thumbnail default on to match historical always-on
        // rendering of those leading columns. Select defaults off — opt-in
        // discovery affordance for multi-selection.
        Self {
            select: false,
            index: true,
            thumbnail: true,
            stars: false,
            songcount: true,
            plays: false,
            love: true,
        }
    }
}

impl AlbumsColumnVisibility {
    pub fn get(&self, col: AlbumsColumn) -> bool {
        match col {
            AlbumsColumn::Select => self.select,
            AlbumsColumn::Index => self.index,
            AlbumsColumn::Thumbnail => self.thumbnail,
            AlbumsColumn::Stars => self.stars,
            AlbumsColumn::SongCount => self.songcount,
            AlbumsColumn::Plays => self.plays,
            AlbumsColumn::Love => self.love,
        }
    }

    pub fn set(&mut self, col: AlbumsColumn, value: bool) {
        match col {
            AlbumsColumn::Select => self.select = value,
            AlbumsColumn::Index => self.index = value,
            AlbumsColumn::Thumbnail => self.thumbnail = value,
            AlbumsColumn::Stars => self.stars = value,
            AlbumsColumn::SongCount => self.songcount = value,
            AlbumsColumn::Plays => self.plays = value,
            AlbumsColumn::Love => self.love = value,
        }
    }
}

/// Stars auto-show when sort = Rating regardless of toggle.
pub(crate) fn albums_stars_visible(sort: SortMode, user_visible: bool) -> bool {
    user_visible || matches!(sort, SortMode::Rating)
}

/// Plays auto-show when sort = MostPlayed regardless of toggle.
pub(crate) fn albums_plays_visible(sort: SortMode, user_visible: bool) -> bool {
    user_visible || matches!(sort, SortMode::MostPlayed)
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
    /// Whether the column-visibility checkbox dropdown is open. Driven by
    /// `Nokkvi.open_menu` so a single root-level state enforces mutual
    /// exclusion with other overlay menus.
    pub column_dropdown_open: bool,
    /// Trigger bounds captured when the dropdown was opened. The overlay
    /// anchors below this rectangle.
    pub column_dropdown_trigger_bounds: Option<iced::Rectangle>,
    /// Borrowed reference to the root open-menu state, so per-row context
    /// menus and the artwork-panel context menu can resolve their own
    /// open/closed status.
    pub open_menu: Option<&'a crate::app_message::OpenMenu>,
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
    /// Click on a row's leading select checkbox — toggles `item_index` in
    /// `selected_indices`. No play/highlight side effects.
    SlotListSelectionToggle(usize),
    /// Click on the tri-state "select all" header — fills selection with
    /// every visible row, or clears if every visible row is already selected.
    SlotListSelectAllToggle,
    AddCenterToQueue, // Add centered album to queue (Shift+Q)

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
    /// Navigate to Artists and auto-expand the artist with this id (no filter set).
    NavigateAndExpandArtist(String),
    /// Navigate to Genres and auto-expand the genre with this id (no filter set).
    NavigateAndExpandGenre(String),
    ToggleColumnVisible(AlbumsColumn),
    /// Column-dropdown open/close request — bubbled to root
    /// `Message::SetOpenMenu`. Intercepted in `handle_albums` before the
    /// page's `update` runs.
    SetOpenMenu(Option<crate::app_message::OpenMenu>),
    /// Artwork column drag handle event — intercepted at root, page never sees it.
    ArtworkColumnDrag(crate::widgets::artwork_split_handle::DragEvent),
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
    NavigateAndExpandArtist(String), // artist_id - navigate to Artists and auto-expand
    NavigateAndExpandGenre(String),  // genre_id - navigate to Genres and auto-expand
    ColumnVisibilityChanged(AlbumsColumn, bool),
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
            Self::NavigateAndExpandArtist(id) => {
                super::CommonViewAction::NavigateAndExpandArtist(id.clone())
            }
            Self::NavigateAndExpandGenre(id) => {
                super::CommonViewAction::NavigateAndExpandGenre(id.clone())
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
            column_visibility: AlbumsColumnVisibility::default(),
        }
    }
}

impl AlbumsPage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Convert sort mode to API string for server requests.
    /// Thin shim — the unified mapping lives in `views/sort_api.rs`.
    pub fn sort_mode_to_api_string(
        sort_mode: crate::widgets::view_header::SortMode,
    ) -> &'static str {
        super::sort_api::sort_mode_to_api_string(crate::View::Albums, sort_mode)
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
                AlbumsMessage::SlotListSelectionToggle(offset) => {
                    // Slot list indices are flattened (parents + expansion
                    // children); `total_items` from the dispatcher is the
                    // base buffer length. Use the flattened length so the
                    // toggle's bounds check matches what the user sees.
                    let flattened = self.expansion.flattened_len(albums);
                    self.common.handle_selection_toggle(offset, flattened);
                    (Task::none(), AlbumsAction::None)
                }
                AlbumsMessage::SlotListSelectAllToggle => {
                    let flattened = self.expansion.flattened_len(albums);
                    self.common.handle_select_all_toggle(flattened);
                    (Task::none(), AlbumsAction::None)
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
                // Routed up to root in `handle_albums` before this match runs;
                // arm exists only for exhaustiveness.
                AlbumsMessage::SetOpenMenu(_) => (Task::none(), AlbumsAction::None),
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
                AlbumsMessage::NavigateAndExpandArtist(artist_id) => (
                    Task::none(),
                    AlbumsAction::NavigateAndExpandArtist(artist_id),
                ),
                AlbumsMessage::NavigateAndExpandGenre(genre_id) => {
                    (Task::none(), AlbumsAction::NavigateAndExpandGenre(genre_id))
                }
                AlbumsMessage::ToggleColumnVisible(col) => {
                    let new_value = !self.column_visibility.get(col);
                    self.column_visibility.set(col, new_value);
                    (
                        Task::none(),
                        AlbumsAction::ColumnVisibilityChanged(col, new_value),
                    )
                }
                _ => (Task::none(), AlbumsAction::None),
            },
        }
    }

    // NOTE: build_flattened_list, collapse, clear are now on self.expansion (ExpansionState)

    /// Build the view
    pub fn view<'a>(&'a self, data: AlbumsViewData<'a>) -> Element<'a, AlbumsMessage> {
        use crate::widgets::view_header::SortMode;

        let column_dropdown: Element<'a, AlbumsMessage> = {
            use crate::widgets::checkbox_dropdown::checkbox_dropdown;
            let items: Vec<(AlbumsColumn, &'static str, bool)> = vec![
                (
                    AlbumsColumn::Select,
                    "Select",
                    self.column_visibility.select,
                ),
                (AlbumsColumn::Index, "Index", self.column_visibility.index),
                (
                    AlbumsColumn::Thumbnail,
                    "Thumbnail",
                    self.column_visibility.thumbnail,
                ),
                (AlbumsColumn::Stars, "Stars", self.column_visibility.stars),
                (
                    AlbumsColumn::SongCount,
                    "Song Count",
                    self.column_visibility.songcount,
                ),
                (AlbumsColumn::Plays, "Plays", self.column_visibility.plays),
                (AlbumsColumn::Love, "Love", self.column_visibility.love),
            ];
            checkbox_dropdown(
                "assets/icons/columns-3-cog.svg",
                "Show/hide columns",
                items,
                AlbumsMessage::ToggleColumnVisible,
                |trigger_bounds| match trigger_bounds {
                    Some(b) => AlbumsMessage::SetOpenMenu(Some(
                        crate::app_message::OpenMenu::CheckboxDropdown {
                            view: crate::View::Albums,
                            trigger_bounds: b,
                        },
                    )),
                    None => AlbumsMessage::SetOpenMenu(None),
                },
                data.column_dropdown_open,
                data.column_dropdown_trigger_bounds,
            )
            .into()
        };

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
            None,                  // on_add
            Some(column_dropdown), // trailing_button
            true,                  // show_search
            AlbumsMessage::SearchQueryChanged,
        );

        // Compose with the tri-state "select all" header bar when the
        // multi-select column is on. Tri-state derives from the current
        // selection set against the *flattened* (visible) row count.
        let header = {
            let flattened_len = self
                .expansion
                .build_flattened_list(data.albums, |a| &a.id)
                .len();
            crate::widgets::slot_list::compose_header_with_select(
                self.column_visibility.select,
                self.common.select_all_state(flattened_len),
                AlbumsMessage::SlotListSelectAllToggle,
                header,
            )
        };

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
            SlotListConfig, chrome_height_with_select_header, slot_list_view_with_scroll,
        };

        let select_header_visible = self.column_visibility.select;
        let config = SlotListConfig::with_dynamic_slots(
            data.window_height,
            chrome_height_with_select_header(select_header_visible),
        )
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
                SlotListEntry::Parent(album) => {
                    let row = self.render_album_row(
                        album,
                        &ctx,
                        album_art,
                        current_sort_mode,
                        data.stable_viewport,
                        data.open_menu,
                    );
                    crate::widgets::slot_list::wrap_with_select_column(
                        select_header_visible,
                        ctx.is_selected,
                        ctx.item_index,
                        AlbumsMessage::SlotListSelectionToggle,
                        row,
                    )
                }
                SlotListEntry::Child(song, _parent_album_id) => {
                    let sub_index_label =
                        self.expansion
                            .child_sub_index_label(ctx.item_index, albums, |a| &a.id);
                    let row = self.render_track_row(
                        song,
                        &ctx,
                        &sub_index_label,
                        data.stable_viewport,
                        data.open_menu,
                    );
                    crate::widgets::slot_list::wrap_with_select_column(
                        select_header_visible,
                        ctx.is_selected,
                        ctx.item_index,
                        AlbumsMessage::SlotListSelectionToggle,
                        row,
                    )
                }
            },
        );

        // Wrap slot list content with standard background (prevents color bleed-through)
        use crate::widgets::slot_list::slot_list_background_container;
        let slot_list_content = slot_list_background_container(slot_list_content);

        // Use base slot list layout with artwork column
        use crate::widgets::base_slot_list_layout::single_artwork_panel_with_pill;

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

        // Overlay building (gated by Settings → Interface → Views → Text Overlay On Artwork)
        let overlay_content = centered_album
            .filter(|_| crate::theme::albums_artwork_overlay())
            .map(|album| {
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

                if let Some(row) = auth_status_row::<AlbumsMessage>(album.is_starred, album.rating)
                {
                    col = col.push(row);
                }

                col.into()
            });

        let artwork_menu_id = crate::app_message::ContextMenuId::ArtworkPanel(crate::View::Albums);
        let (artwork_menu_open, artwork_menu_position) =
            crate::widgets::context_menu::open_state_for(data.open_menu, &artwork_menu_id);
        let artwork_content = Some(single_artwork_panel_with_pill(
            artwork_handle,
            overlay_content,
            active_dominant_color,
            on_refresh,
            artwork_menu_open,
            artwork_menu_position,
            move |position| match position {
                Some(p) => {
                    AlbumsMessage::SetOpenMenu(Some(crate::app_message::OpenMenu::Context {
                        id: artwork_menu_id.clone(),
                        position: p,
                    }))
                }
                None => AlbumsMessage::SetOpenMenu(None),
            },
        ));

        crate::widgets::base_slot_list_layout::base_slot_list_layout_with_handle(
            &layout_config,
            header,
            slot_list_content,
            artwork_content,
            Some(AlbumsMessage::ArtworkColumnDrag),
        )
    }

    /// Render an album row in the slot list (existing album layout)
    fn render_album_row<'a>(
        &self,
        album: &AlbumUIViewData,
        ctx: &crate::widgets::slot_list::SlotListRowContext,
        album_art: &'a HashMap<String, image::Handle>,
        current_sort_mode: SortMode,
        stable_viewport: bool,
        open_menu: Option<&'a crate::app_message::OpenMenu>,
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
        let play_count = album.play_count.unwrap_or(0);

        // Per-column visibility (Stars/Plays auto-shown by their sort modes).
        let vis = self.column_visibility;
        let show_stars = albums_stars_visible(current_sort_mode, vis.stars);
        let show_songcount = vis.songcount;
        let show_plays = albums_plays_visible(current_sort_mode, vis.plays);
        let show_love = vis.love;
        // Dynamic slot now only carries Date/Year/Duration/Genre — Rating
        // and MostPlayed have been promoted to dedicated columns.
        let show_dynamic_slot = !extra_value.is_empty();

        const SONGCOUNT_PORTION: u16 = 22;
        const STARS_PORTION: u16 = 12;
        const PLAYS_PORTION: u16 = 16;
        const DYNAMIC_PORTION: u16 = 21;
        const LOVE_PORTION: u16 = 5;
        let mut consumed: u16 = 0;
        if show_songcount {
            consumed += SONGCOUNT_PORTION;
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
        if show_love {
            consumed += LOVE_PORTION;
        }
        let title_portion = 100u16.saturating_sub(consumed).max(20);

        let mut content_row = Row::new().spacing(6.0).align_y(Alignment::Center);
        if vis.index {
            content_row = content_row.push(slot_list_index_column(
                ctx.item_index,
                index_size,
                style,
                ctx.opacity,
            ));
        }
        if vis.thumbnail {
            use crate::widgets::slot_list::slot_list_artwork_column;
            content_row = content_row.push(slot_list_artwork_column(
                album_art.get(&album_id),
                artwork_size,
                ctx.is_center,
                false,
                ctx.opacity,
            ));
        }
        content_row = content_row.push({
            use crate::widgets::slot_list::slot_list_text_column;
            let artist_click = Some(AlbumsMessage::NavigateAndExpandArtist(
                album.artist_id.clone(),
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
                title_portion,
            )
        });

        if show_songcount {
            let idx = ctx.item_index;
            use crate::widgets::slot_list::slot_list_metadata_column;
            content_row = content_row.push(slot_list_metadata_column(
                format!("{song_count} songs"),
                Some(AlbumsMessage::FocusAndExpand(idx)),
                song_count_size,
                style,
                SONGCOUNT_PORTION,
            ));
        }

        if show_stars {
            let star_icon_size = m.title_size;
            let idx = ctx.item_index;
            use crate::widgets::slot_list::slot_list_star_rating;
            content_row = content_row.push(slot_list_star_rating(
                rating,
                star_icon_size,
                ctx.is_center,
                ctx.opacity,
                Some(STARS_PORTION),
                Some(move |star: usize| AlbumsMessage::ClickSetRating(idx, star)),
            ));
        }

        if show_plays {
            use crate::widgets::slot_list::slot_list_metadata_column;
            content_row = content_row.push(slot_list_metadata_column(
                format!("{play_count} plays"),
                None,
                song_count_size,
                style,
                PLAYS_PORTION,
            ));
        }

        if show_dynamic_slot {
            let mut click_msg = None;
            if current_sort_mode == SortMode::Genre {
                click_msg = Some(AlbumsMessage::NavigateAndExpandGenre(extra_value.clone()));
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
                    Some(AlbumsMessage::ClickToggleStar(ctx.item_index)),
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
            context_menu, library_entries_with_folder, library_entry_view, open_state_for,
        };
        let item_idx = ctx.item_index;
        let cm_id = crate::app_message::ContextMenuId::LibraryRow {
            view: crate::View::Albums,
            item_index: item_idx,
        };
        let (cm_open, cm_position) = open_state_for(open_menu, &cm_id);
        context_menu(
            slot_button,
            library_entries_with_folder(),
            move |entry, length| {
                library_entry_view(entry, length, |e| {
                    AlbumsMessage::ContextMenuAction(item_idx, e)
                })
            },
            cm_open,
            cm_position,
            move |position| match position {
                Some(p) => {
                    AlbumsMessage::SetOpenMenu(Some(crate::app_message::OpenMenu::Context {
                        id: cm_id.clone(),
                        position: p,
                    }))
                }
                None => AlbumsMessage::SetOpenMenu(None),
            },
        )
        .into()
    }

    /// Render a child track row in the slot list (indented, simpler layout)
    fn render_track_row<'a>(
        &self,
        song: &SongUIViewData,
        ctx: &crate::widgets::slot_list::SlotListRowContext,
        sub_index_label: &str,
        stable_viewport: bool,
        open_menu: Option<&'a crate::app_message::OpenMenu>,
    ) -> Element<'a, AlbumsMessage> {
        let track_el = super::expansion::render_child_track_row(
            song,
            ctx,
            sub_index_label,
            AlbumsMessage::SlotListActivateCenter,
            if stable_viewport {
                AlbumsMessage::SlotListSetOffset(ctx.item_index, ctx.modifiers)
            } else {
                AlbumsMessage::SlotListClickPlay(ctx.item_index)
            },
            Some(AlbumsMessage::ClickToggleStar(ctx.item_index)),
            song.artist_id
                .as_ref()
                .map(|id| AlbumsMessage::NavigateAndExpandArtist(id.clone())),
            1, // depth 1: child tracks under album
        );

        use crate::widgets::context_menu::{
            context_menu, library_entry_view, open_state_for, song_entries_with_folder,
        };
        let item_idx = ctx.item_index;
        let cm_id = crate::app_message::ContextMenuId::LibraryRow {
            view: crate::View::Albums,
            item_index: item_idx,
        };
        let (cm_open, cm_position) = open_state_for(open_menu, &cm_id);
        context_menu(
            track_el,
            song_entries_with_folder(),
            move |entry, length| {
                library_entry_view(entry, length, |e| {
                    AlbumsMessage::ContextMenuAction(item_idx, e)
                })
            },
            cm_open,
            cm_position,
            move |position| match position {
                Some(p) => {
                    AlbumsMessage::SetOpenMenu(Some(crate::app_message::OpenMenu::Context {
                        id: cm_id.clone(),
                        position: p,
                    }))
                }
                None => AlbumsMessage::SetOpenMenu(None),
            },
        )
        .into()
    }
}

/// Dynamic-slot value based on current sort mode. Rating and MostPlayed are
/// no longer rendered here — they're dedicated, toggleable columns now.
fn get_extra_column_value(album: &AlbumUIViewData, sort_mode: SortMode) -> String {
    match sort_mode {
        SortMode::RecentlyAdded => album
            .created_at
            .as_ref()
            .and_then(|d| formatters::format_date(d).ok())
            .unwrap_or_default(),
        SortMode::RecentlyPlayed => album.play_date.as_ref().map_or_else(
            || "never".to_string(),
            |d| d.split('T').next().unwrap_or(d).to_string(),
        ),
        SortMode::ReleaseYear => album.year.map(|y| y.to_string()).unwrap_or_default(),
        SortMode::Duration => album
            .duration
            .map(|d| formatters::format_time(d as u32))
            .unwrap_or_default(),
        SortMode::Genre => album.genre.clone().unwrap_or_default(),
        // Stars and Plays are dedicated columns (auto-show on Rating /
        // MostPlayed sort respectively). All other sort modes have no
        // extra-column data.
        SortMode::Rating
        | SortMode::MostPlayed
        | SortMode::Favorited
        | SortMode::Random
        | SortMode::Name
        | SortMode::AlbumArtist
        | SortMode::Artist
        | SortMode::SongCount
        | SortMode::AlbumCount
        | SortMode::Title
        | SortMode::Album
        | SortMode::Bpm
        | SortMode::Channels
        | SortMode::Comment
        | SortMode::UpdatedAt => String::new(),
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

    #[test]
    fn albums_column_visibility_default_preserves_today_behavior() {
        let v = AlbumsColumnVisibility::default();
        // Stars/Plays opt-in (today only show on their sort modes).
        assert!(!v.stars);
        assert!(v.songcount);
        assert!(!v.plays);
        assert!(v.love);
    }

    #[test]
    fn albums_stars_visible_auto_shows_on_rating_sort() {
        assert!(albums_stars_visible(SortMode::Rating, false));
        assert!(albums_stars_visible(SortMode::Rating, true));
        assert!(!albums_stars_visible(SortMode::Name, false));
        assert!(albums_stars_visible(SortMode::Name, true));
    }

    #[test]
    fn albums_plays_visible_auto_shows_on_most_played() {
        assert!(albums_plays_visible(SortMode::MostPlayed, false));
        assert!(albums_plays_visible(SortMode::MostPlayed, true));
        assert!(!albums_plays_visible(SortMode::Name, false));
        assert!(albums_plays_visible(SortMode::Name, true));
    }

    #[test]
    fn albums_toggle_column_visible_flips_state_and_emits_action() {
        let mut page = AlbumsPage::default();
        let empty: Vec<AlbumUIViewData> = vec![];
        let (_t, action) = page.update(
            AlbumsMessage::ToggleColumnVisible(AlbumsColumn::Stars),
            0,
            &empty,
        );
        assert!(page.column_visibility.stars);
        assert!(matches!(
            action,
            AlbumsAction::ColumnVisibilityChanged(AlbumsColumn::Stars, true)
        ));
    }
}
