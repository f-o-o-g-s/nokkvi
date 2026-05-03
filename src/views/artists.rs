//! Artists Page Component
//!
//! Self-contained artists view with slot list navigation, search, and filtering.
//! Uses message bubbling pattern to communicate global actions to root.
//! Supports inline album expansion (Shift+Enter) using flattened SlotListEntry list.

use std::collections::HashMap;

use iced::{
    Alignment, Element, Length, Task,
    widget::{Row, button, container, image},
};
use nokkvi_data::backend::{albums::AlbumUIViewData, artists::ArtistUIViewData};

use super::expansion::{ExpansionState, ThreeTierEntry};
use crate::{
    app_message::Message,
    widgets::{self, SlotListPageState, view_header::SortMode},
};

/// Artists page local state
#[derive(Debug)]
pub struct ArtistsPage {
    pub common: SlotListPageState,
    /// Inline expansion state (artist → albums)
    pub expansion: ExpansionState<AlbumUIViewData>,
    /// Sub-expansion state (album → tracks)
    pub sub_expansion: ExpansionState<nokkvi_data::backend::songs::SongUIViewData>,
    /// Per-column visibility toggles surfaced via the columns-cog dropdown.
    pub column_visibility: ArtistsColumnVisibility,
}

/// Toggleable artists columns. The artist name is always shown; everything
/// else is user-toggleable through the columns-cog dropdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtistsColumn {
    Index,
    Thumbnail,
    Stars,
    AlbumCount,
    SongCount,
    Plays,
    Love,
}

/// User-toggle state for each toggleable artists column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtistsColumnVisibility {
    pub index: bool,
    pub thumbnail: bool,
    pub stars: bool,
    pub albumcount: bool,
    pub songcount: bool,
    pub plays: bool,
    pub love: bool,
}

impl Default for ArtistsColumnVisibility {
    fn default() -> Self {
        // All-on matches today's permanent layout (after the Plays-column
        // commit) — no surprise visual change on first launch.
        Self {
            index: true,
            thumbnail: true,
            stars: true,
            albumcount: true,
            songcount: true,
            plays: true,
            love: true,
        }
    }
}

impl ArtistsColumnVisibility {
    pub fn get(&self, col: ArtistsColumn) -> bool {
        match col {
            ArtistsColumn::Index => self.index,
            ArtistsColumn::Thumbnail => self.thumbnail,
            ArtistsColumn::Stars => self.stars,
            ArtistsColumn::AlbumCount => self.albumcount,
            ArtistsColumn::SongCount => self.songcount,
            ArtistsColumn::Plays => self.plays,
            ArtistsColumn::Love => self.love,
        }
    }

    pub fn set(&mut self, col: ArtistsColumn, value: bool) {
        match col {
            ArtistsColumn::Index => self.index = value,
            ArtistsColumn::Thumbnail => self.thumbnail = value,
            ArtistsColumn::Stars => self.stars = value,
            ArtistsColumn::AlbumCount => self.albumcount = value,
            ArtistsColumn::SongCount => self.songcount = value,
            ArtistsColumn::Plays => self.plays = value,
            ArtistsColumn::Love => self.love = value,
        }
    }
}

/// Stars auto-show when sort = Rating regardless of toggle.
pub(crate) fn artists_stars_visible(sort: SortMode, user_visible: bool) -> bool {
    user_visible || matches!(sort, SortMode::Rating)
}

/// Plays auto-show when sort = MostPlayed regardless of toggle.
pub(crate) fn artists_plays_visible(sort: SortMode, user_visible: bool) -> bool {
    user_visible || matches!(sort, SortMode::MostPlayed)
}

/// View data passed from root (read-only, borrows from app state to avoid allocations)
pub struct ArtistsViewData<'a> {
    pub artists: &'a [ArtistUIViewData],
    pub artist_art: &'a HashMap<String, image::Handle>,
    /// Album artwork cache, keyed by album_id. Used by nested child album
    /// rows in the artist→album expansion when `column_visibility.thumbnail`
    /// is enabled.
    pub album_art: &'a HashMap<String, image::Handle>,
    pub large_artwork: &'a HashMap<String, image::Handle>,
    pub dominant_colors: &'a HashMap<String, iced::Color>,
    pub window_width: f32,
    pub window_height: f32,
    pub scale_factor: f32,
    pub modifiers: iced::keyboard::Modifiers,
    pub total_artist_count: usize,
    pub loading: bool,
    pub stable_viewport: bool,
    /// Whether the column-visibility checkbox dropdown is open (controlled
    /// by `Nokkvi.open_menu`).
    pub column_dropdown_open: bool,
    /// Trigger bounds captured when the dropdown was opened.
    pub column_dropdown_trigger_bounds: Option<iced::Rectangle>,
    /// Borrowed reference to the root open-menu state, so per-row context
    /// menus and the artwork-panel context menu can resolve their own
    /// open/closed status.
    pub open_menu: Option<&'a crate::app_message::OpenMenu>,
}

/// Messages for local artist page interactions
#[derive(Debug, Clone)]
pub enum ArtistsMessage {
    // Slot list navigation
    SlotListNavigateUp,
    SlotListNavigateDown,
    SlotListSetOffset(usize, iced::keyboard::Modifiers),
    SlotListScrollSeek(usize),
    SlotListActivateCenter,
    SlotListClickPlay(usize), // Click non-center to play directly (skip focus)
    AddCenterToQueue,         // Add all songs from centered artist to queue (Shift+Q)

    // Mouse click on star/heart (item_index, value)
    ClickSetRating(usize, usize), // (item_index, rating 1-5)
    ClickToggleStar(usize),       // item_index

    // Context menu
    ContextMenuAction(usize, crate::widgets::context_menu::LibraryContextEntry),

    // Inline expansion — first level (Shift+Enter on artist)
    ExpandCenter,
    FocusAndExpand(usize), // Clicked 'X albums' — focus that row and expand it
    CollapseExpansion,
    /// Albums loaded for expanded artist (artist_id, albums)
    AlbumsLoaded(String, Vec<AlbumUIViewData>),

    // Inline expansion — second level (Shift+Enter on child album)
    ExpandAlbum,
    FocusAndExpandAlbum(usize), // Clicked 'X songs' on child album — focus and expand tracks
    CollapseAlbumExpansion,
    /// Tracks loaded for expanded album (album_id, songs)
    TracksLoaded(String, Vec<nokkvi_data::backend::songs::SongUIViewData>),

    // View header
    SortModeSelected(widgets::view_header::SortMode),
    ToggleSortOrder,
    SearchQueryChanged(String),
    SearchFocused(bool),
    RefreshViewData,
    CenterOnPlaying,
    ToggleColumnVisible(ArtistsColumn),

    // Data loading (moved from root Message enum)
    ArtistsLoaded {
        result: Result<Vec<ArtistUIViewData>, String>,
        total_count: usize,
        background: bool,
        anchor_id: Option<String>,
    },
    ArtistsPageLoaded(Result<Vec<ArtistUIViewData>, String>, usize), // result, total_count (subsequent page)

    NavigateAndFilter(crate::View, nokkvi_data::types::filter::LibraryFilter), // Navigate to target view and filter

    // Open external URL
    OpenExternalUrl(String),

    /// Column-dropdown open/close request — bubbled to root
    /// `Message::SetOpenMenu`. Intercepted in `handle_artists` before the
    /// page's `update` runs.
    SetOpenMenu(Option<crate::app_message::OpenMenu>),
    /// Artwork column drag handle event — intercepted at root, page never sees it.
    ArtworkColumnDrag(crate::widgets::artwork_split_handle::DragEvent),
}

/// Actions that bubble up to root for global state mutation
#[derive(Debug, Clone)]
pub enum ArtistsAction {
    PlayArtist(String), // artist_id - clear queue and play all songs
    AddBatchToQueue(nokkvi_data::types::batch::BatchPayload),
    PlayAlbum(String),    // album_id - play child album
    PlayTrack(String),    // song_id - play single expanded track
    StarArtist(String),   // artist_id - star the artist
    UnstarArtist(String), // artist_id - unstar the artist
    /// Set absolute rating on item (item_id, item_type, rating)
    SetRating(String, &'static str, usize),
    /// Star/unstar item by click (item_id, item_type, new_starred)
    ToggleStar(String, &'static str, bool),
    /// Expand artist inline — root should load albums (artist_id)
    ExpandArtist(String),
    /// Expand album inline — root should load tracks (album_id)
    ExpandAlbum(String),
    LoadPage(usize),       // offset - trigger fetch of next page
    SearchChanged(String), // trigger reload
    SortModeChanged(widgets::view_header::SortMode), // trigger reload
    SortOrderChanged(bool), // trigger reload
    RefreshViewData,       // trigger reload
    PlayNextBatch(nokkvi_data::types::batch::BatchPayload), // artist_id or album_id - insert after currently playing
    AddBatchToPlaylist(nokkvi_data::types::batch::BatchPayload),
    ShowInfo(Box<nokkvi_data::types::info_modal::InfoModalItem>), // Open info modal
    ShowAlbumInFolder(String), // album_id - fetch a song path and open containing folder
    ShowSongInFolder(String),  // song path - open containing folder directly
    FindSimilar(String, String), // (entity_id, label) - open similar tab
    TopSongs(String, String),  // (artist_name, label) - open similar tab for top songs
    CenterOnPlaying,
    NavigateAndFilter(crate::View, nokkvi_data::types::filter::LibraryFilter),
    ColumnVisibilityChanged(ArtistsColumn, bool),
    /// Refresh the artists viewport: prefetch mini artwork, fetch the 500px
    /// artwork + dominant color for the new center artist, and chain a
    /// page-fetch if the viewport is near the loaded edge.
    ///
    /// Emitted from settled-scroll and hotkey navigation paths only.
    /// `SlotListScrollSeek` (mid-drag) deliberately does NOT emit this —
    /// rapid scrollbar drag previously hung the app by spawning hundreds
    /// of in-flight 500px fetches + dominant-color blocking tasks per drag.
    LoadLargeArtwork,
    None,
}

impl super::HasCommonAction for ArtistsAction {
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

impl Default for ArtistsPage {
    fn default() -> Self {
        Self {
            common: SlotListPageState::new(
                widgets::view_header::SortMode::Name,
                true, // sort_ascending
            ),
            expansion: ExpansionState::default(),
            sub_expansion: ExpansionState::default(),
            column_visibility: ArtistsColumnVisibility::default(),
        }
    }
}

impl ArtistsPage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Convert sort mode to API string for server requests.
    /// Thin shim — the unified mapping lives in `views/sort_api.rs`.
    pub fn sort_mode_to_api_string(
        sort_mode: crate::widgets::view_header::SortMode,
    ) -> &'static str {
        super::sort_api::sort_mode_to_api_string(crate::View::Artists, sort_mode)
    }

    /// Update internal state and return actions for root
    pub fn update(
        &mut self,
        message: ArtistsMessage,
        total_items: usize,
        artists: &[ArtistUIViewData],
    ) -> (Task<ArtistsMessage>, ArtistsAction) {
        // Shift+Enter routing for the 3-tier list. The macro's ExpandCenter
        // handler operates on the 2-tier (parent + first-level children) view,
        // which is wrong once a sub-expansion has injected grandchildren below
        // a child album. Inspect the centered entry first and re-route:
        //   * Child / Grandchild → ExpandAlbum (toggles the inner expansion)
        //   * Parent (outer already expanded) → collapse outer + clear sub
        //   * otherwise (no expansion) → fall through to macro to open it.
        if matches!(message, ArtistsMessage::ExpandCenter) && self.expansion.is_expanded() {
            let total = super::expansion::three_tier_flattened_len(
                artists,
                &self.expansion,
                self.sub_expansion.children.len(),
            );
            let entry = self.common.get_center_item_index(total).and_then(|idx| {
                super::expansion::three_tier_get_entry_at(
                    idx,
                    artists,
                    &self.expansion,
                    &self.sub_expansion,
                    |a| &a.id,
                    |a| &a.id,
                )
            });
            match entry {
                Some(ThreeTierEntry::Child(_, _) | ThreeTierEntry::Grandchild(_, _)) => {
                    return self.update(ArtistsMessage::ExpandAlbum, total_items, artists);
                }
                Some(ThreeTierEntry::Parent(_)) => {
                    self.sub_expansion.clear();
                    self.expansion
                        .collapse(artists, |a| &a.id, &mut self.common);
                    return (Task::none(), ArtistsAction::None);
                }
                None => {}
            }
        }

        match super::impl_expansion_update!(
            self, message, artists, total_items,
            id_fn: |a| &a.id,
            expand_center: ArtistsMessage::ExpandCenter => ArtistsAction::ExpandArtist,
            collapse: ArtistsMessage::CollapseExpansion,
            children_loaded: ArtistsMessage::AlbumsLoaded,
            sort_selected: ArtistsMessage::SortModeSelected => ArtistsAction::SortModeChanged,
            toggle_sort: ArtistsMessage::ToggleSortOrder => ArtistsAction::SortOrderChanged,
            search_changed: ArtistsMessage::SearchQueryChanged => ArtistsAction::SearchChanged,
            search_focused: ArtistsMessage::SearchFocused,
            action_none: ArtistsAction::None,
        ) {
            Ok((task, action)) => {
                // Clear sub_expansion when the outer expansion is collapsed or reloaded
                if matches!(
                    action,
                    ArtistsAction::SortModeChanged(_)
                        | ArtistsAction::SortOrderChanged(_)
                        | ArtistsAction::SearchChanged(_)
                ) {
                    self.sub_expansion.clear();
                }
                (task, action)
            }
            Err(msg) => match msg {
                ArtistsMessage::FocusAndExpand(offset) => {
                    let len = super::expansion::three_tier_flattened_len(
                        artists,
                        &self.expansion,
                        self.sub_expansion.children.len(),
                    );
                    self.common
                        .handle_slot_click(offset, len, Default::default());
                    // Now expand the centered artist
                    if let Some(parent_id) =
                        self.expansion
                            .handle_expand_center(artists, |a| &a.id, &mut self.common)
                    {
                        self.sub_expansion.clear();
                        (Task::none(), ArtistsAction::ExpandArtist(parent_id))
                    } else {
                        (Task::none(), ArtistsAction::None)
                    }
                }
                // CollapseExpansion handled by macro — clear sub_expansion too
                ArtistsMessage::CollapseAlbumExpansion => {
                    // Restore position to where user was when album was expanded
                    let saved = self.sub_expansion.parent_offset;
                    self.sub_expansion.clear();
                    let total =
                        super::expansion::three_tier_flattened_len(artists, &self.expansion, 0);
                    self.common.handle_set_offset(saved, total);
                    (Task::none(), ArtistsAction::None)
                }
                ArtistsMessage::FocusAndExpandAlbum(offset) => {
                    // Clicked 'X songs' link on a child album — focus that row, then expand
                    let len = super::expansion::three_tier_flattened_len(
                        artists,
                        &self.expansion,
                        self.sub_expansion.children.len(),
                    );
                    self.common
                        .handle_slot_click(offset, len, Default::default());
                    // Delegate to the existing ExpandAlbum logic
                    self.update(ArtistsMessage::ExpandAlbum, total_items, artists)
                }
                ArtistsMessage::ExpandAlbum => {
                    // Shift+Enter on a child album row — expand its tracks
                    let total = super::expansion::three_tier_flattened_len(
                        artists,
                        &self.expansion,
                        self.sub_expansion.children.len(),
                    );
                    let center_idx = self.common.get_center_item_index(total);
                    let action = center_idx.and_then(|idx| {
                        super::expansion::three_tier_get_entry_at(
                            idx,
                            artists,
                            &self.expansion,
                            &self.sub_expansion,
                            |a| &a.id,
                            |a| &a.id,
                        )
                    });
                    match action {
                        Some(ThreeTierEntry::Child(album, _)) => {
                            // Toggle: if already expanded, collapse
                            let aid = album.id.clone();
                            if self.sub_expansion.is_expanded_parent(&aid) {
                                let saved = self.sub_expansion.parent_offset;
                                self.sub_expansion.clear();
                                let total = super::expansion::three_tier_flattened_len(
                                    artists,
                                    &self.expansion,
                                    0,
                                );
                                self.common.handle_set_offset(saved, total);
                                (Task::none(), ArtistsAction::None)
                            } else {
                                // Collapse any existing sub-expansion, start new one
                                self.sub_expansion.clear();
                                self.sub_expansion.parent_offset =
                                    self.common.slot_list.viewport_offset;
                                (Task::none(), ArtistsAction::ExpandAlbum(aid))
                            }
                        }
                        Some(ThreeTierEntry::Grandchild(_, _)) => {
                            // On a grandchild — collapse the album sub-expansion
                            let saved = self.sub_expansion.parent_offset;
                            self.sub_expansion.clear();
                            let total = super::expansion::three_tier_flattened_len(
                                artists,
                                &self.expansion,
                                0,
                            );
                            self.common.handle_set_offset(saved, total);
                            (Task::none(), ArtistsAction::None)
                        }
                        _ => (Task::none(), ArtistsAction::None), // On parent or nothing
                    }
                }
                ArtistsMessage::TracksLoaded(album_id, songs) => {
                    self.sub_expansion.set_children(
                        album_id,
                        songs,
                        &self.expansion.children,
                        &mut self.common,
                    );
                    (Task::none(), ArtistsAction::None)
                }
                ArtistsMessage::SlotListNavigateUp => {
                    let len = super::expansion::three_tier_flattened_len(
                        artists,
                        &self.expansion,
                        self.sub_expansion.children.len(),
                    );
                    self.common.handle_navigate_up(len);
                    (Task::none(), ArtistsAction::LoadLargeArtwork)
                }
                ArtistsMessage::SlotListNavigateDown => {
                    let len = super::expansion::three_tier_flattened_len(
                        artists,
                        &self.expansion,
                        self.sub_expansion.children.len(),
                    );
                    self.common.handle_navigate_down(len);
                    (Task::none(), ArtistsAction::LoadLargeArtwork)
                }
                ArtistsMessage::SlotListSetOffset(offset, modifiers) => {
                    let len = super::expansion::three_tier_flattened_len(
                        artists,
                        &self.expansion,
                        self.sub_expansion.children.len(),
                    );
                    self.common.handle_slot_click(offset, len, modifiers);
                    (Task::none(), ArtistsAction::LoadLargeArtwork)
                }
                ArtistsMessage::SlotListScrollSeek(offset) => {
                    let len = super::expansion::three_tier_flattened_len(
                        artists,
                        &self.expansion,
                        self.sub_expansion.children.len(),
                    );
                    self.common.handle_set_offset(offset, len);
                    // Mid-drag: update viewport offset only. Artwork +
                    // page-fetch deferred to the SeekSettled debounce, which
                    // synthesises a SetOffset message that emits LoadLargeArtwork.
                    (Task::none(), ArtistsAction::None)
                }
                ArtistsMessage::SlotListClickPlay(offset) => {
                    let len = super::expansion::three_tier_flattened_len(
                        artists,
                        &self.expansion,
                        self.sub_expansion.children.len(),
                    );
                    self.common.handle_set_offset(offset, len);
                    self.update(ArtistsMessage::SlotListActivateCenter, total_items, artists)
                }
                ArtistsMessage::SlotListActivateCenter => {
                    let total = super::expansion::three_tier_flattened_len(
                        artists,
                        &self.expansion,
                        self.sub_expansion.children.len(),
                    );
                    if let Some(center_idx) = self.common.get_center_item_index(total) {
                        self.common.slot_list.flash_center();
                        match super::expansion::three_tier_get_entry_at(
                            center_idx,
                            artists,
                            &self.expansion,
                            &self.sub_expansion,
                            |a| &a.id,
                            |a| &a.id,
                        ) {
                            Some(ThreeTierEntry::Grandchild(song, _)) => {
                                (Task::none(), ArtistsAction::PlayTrack(song.id.clone()))
                            }
                            Some(ThreeTierEntry::Child(album, _)) => {
                                (Task::none(), ArtistsAction::PlayAlbum(album.id.clone()))
                            }
                            Some(ThreeTierEntry::Parent(_)) => (
                                Task::none(),
                                ArtistsAction::PlayArtist(center_idx.to_string()),
                            ),
                            None => (Task::none(), ArtistsAction::None),
                        }
                    } else {
                        (Task::none(), ArtistsAction::None)
                    }
                }
                ArtistsMessage::AddCenterToQueue => {
                    use nokkvi_data::types::batch::BatchItem;
                    let total = super::expansion::three_tier_flattened_len(
                        artists,
                        &self.expansion,
                        self.sub_expansion.children.len(),
                    );

                    let target_indices = self.common.get_queue_target_indices(total);

                    if target_indices.is_empty() {
                        return (Task::none(), ArtistsAction::None);
                    }

                    let payload = super::expansion::build_batch_payload(target_indices, |i| {
                        match super::expansion::three_tier_get_entry_at(
                            i,
                            artists,
                            &self.expansion,
                            &self.sub_expansion,
                            |a| &a.id,
                            |a| &a.id,
                        ) {
                            Some(ThreeTierEntry::Parent(artist)) => {
                                Some(BatchItem::Artist(artist.id.clone()))
                            }
                            Some(ThreeTierEntry::Child(album, _)) => {
                                Some(BatchItem::Album(album.id.clone()))
                            }
                            Some(ThreeTierEntry::Grandchild(song, _)) => {
                                let item: nokkvi_data::types::song::Song = song.clone().into();
                                Some(BatchItem::Song(Box::new(item)))
                            }
                            None => None,
                        }
                    });

                    (Task::none(), ArtistsAction::AddBatchToQueue(payload))
                }

                // Data loading messages (handled at root level, no action needed here)
                ArtistsMessage::ArtistsLoaded { .. } => (Task::none(), ArtistsAction::None),
                ArtistsMessage::ArtistsPageLoaded(_, _) => (Task::none(), ArtistsAction::None),
                // Routed up to root in `handle_artists` before this match runs;
                // arm exists only for exhaustiveness.
                ArtistsMessage::SetOpenMenu(_) => (Task::none(), ArtistsAction::None),
                ArtistsMessage::RefreshViewData => (Task::none(), ArtistsAction::RefreshViewData),
                ArtistsMessage::CenterOnPlaying => (Task::none(), ArtistsAction::CenterOnPlaying),
                ArtistsMessage::NavigateAndFilter(view, filter) => {
                    (Task::none(), ArtistsAction::NavigateAndFilter(view, filter))
                }
                ArtistsMessage::ToggleColumnVisible(col) => {
                    let new_value = !self.column_visibility.get(col);
                    self.column_visibility.set(col, new_value);
                    (
                        Task::none(),
                        ArtistsAction::ColumnVisibilityChanged(col, new_value),
                    )
                }
                ArtistsMessage::ClickSetRating(item_index, rating) => {
                    use nokkvi_data::utils::formatters::compute_rating_toggle;
                    match super::expansion::three_tier_get_entry_at(
                        item_index,
                        artists,
                        &self.expansion,
                        &self.sub_expansion,
                        |a| &a.id,
                        |a| &a.id,
                    ) {
                        Some(ThreeTierEntry::Grandchild(song, _)) => {
                            let current = song.rating.unwrap_or(0) as usize;
                            let new_rating = compute_rating_toggle(current, rating);
                            (
                                Task::none(),
                                ArtistsAction::SetRating(song.id.clone(), "song", new_rating),
                            )
                        }
                        Some(ThreeTierEntry::Child(album, _)) => {
                            let current = album.rating.unwrap_or(0) as usize;
                            let new_rating = compute_rating_toggle(current, rating);
                            (
                                Task::none(),
                                ArtistsAction::SetRating(album.id.clone(), "album", new_rating),
                            )
                        }
                        Some(ThreeTierEntry::Parent(artist)) => {
                            let current = artist.rating.unwrap_or(0) as usize;
                            let new_rating = compute_rating_toggle(current, rating);
                            (
                                Task::none(),
                                ArtistsAction::SetRating(artist.id.clone(), "artist", new_rating),
                            )
                        }
                        None => (Task::none(), ArtistsAction::None),
                    }
                }
                ArtistsMessage::ClickToggleStar(item_index) => {
                    match super::expansion::three_tier_get_entry_at(
                        item_index,
                        artists,
                        &self.expansion,
                        &self.sub_expansion,
                        |a| &a.id,
                        |a| &a.id,
                    ) {
                        Some(ThreeTierEntry::Grandchild(song, _)) => (
                            Task::none(),
                            ArtistsAction::ToggleStar(song.id.clone(), "song", !song.is_starred),
                        ),
                        Some(ThreeTierEntry::Child(album, _)) => (
                            Task::none(),
                            ArtistsAction::ToggleStar(album.id.clone(), "album", !album.is_starred),
                        ),
                        Some(ThreeTierEntry::Parent(artist)) => (
                            Task::none(),
                            ArtistsAction::ToggleStar(
                                artist.id.clone(),
                                "artist",
                                !artist.is_starred,
                            ),
                        ),
                        None => (Task::none(), ArtistsAction::None),
                    }
                }
                ArtistsMessage::ContextMenuAction(clicked_idx, entry) => {
                    use nokkvi_data::types::batch::BatchItem;

                    use crate::widgets::context_menu::LibraryContextEntry;

                    match entry {
                        LibraryContextEntry::AddToQueue | LibraryContextEntry::AddToPlaylist => {
                            let target_indices = self.common.get_batch_target_indices(clicked_idx);
                            let payload =
                                super::expansion::build_batch_payload(target_indices, |i| {
                                    match super::expansion::three_tier_get_entry_at(
                                        i,
                                        artists,
                                        &self.expansion,
                                        &self.sub_expansion,
                                        |a| &a.id,
                                        |a| &a.id,
                                    ) {
                                        Some(ThreeTierEntry::Parent(artist)) => {
                                            Some(BatchItem::Artist(artist.id.clone()))
                                        }
                                        Some(ThreeTierEntry::Child(album, _)) => {
                                            Some(BatchItem::Album(album.id.clone()))
                                        }
                                        Some(ThreeTierEntry::Grandchild(song, _)) => {
                                            let item: nokkvi_data::types::song::Song =
                                                song.clone().into();
                                            Some(BatchItem::Song(Box::new(item)))
                                        }
                                        None => None,
                                    }
                                });

                            match entry {
                                LibraryContextEntry::AddToQueue => {
                                    (Task::none(), ArtistsAction::AddBatchToQueue(payload))
                                }
                                LibraryContextEntry::AddToPlaylist => {
                                    (Task::none(), ArtistsAction::AddBatchToPlaylist(payload))
                                }
                                _ => unreachable!(),
                            }
                        }
                        // Non-batched actions (apply only to the clicked item)
                        _ => {
                            match super::expansion::three_tier_get_entry_at(
                                clicked_idx,
                                artists,
                                &self.expansion,
                                &self.sub_expansion,
                                |a| &a.id,
                                |a| &a.id,
                            ) {
                                Some(ThreeTierEntry::Parent(artist)) => match entry {
                                    LibraryContextEntry::GetInfo => {
                                        use nokkvi_data::types::info_modal::InfoModalItem;
                                        let item = InfoModalItem::Artist {
                                            name: artist.name.clone(),
                                            song_count: Some(artist.song_count),
                                            album_count: Some(artist.album_count),
                                            is_starred: artist.is_starred,
                                            rating: artist.rating,
                                            play_count: artist.play_count,
                                            play_date: artist.play_date.clone(),
                                            size: artist.size,
                                            mbz_artist_id: artist.mbz_artist_id.clone(),
                                            biography: artist.biography.clone(),
                                            external_url: artist.external_url.clone(),
                                            id: artist.id.clone(),
                                        };
                                        (Task::none(), ArtistsAction::ShowInfo(Box::new(item)))
                                    }
                                    LibraryContextEntry::ShowInFolder
                                    | LibraryContextEntry::Separator => {
                                        (Task::none(), ArtistsAction::None)
                                    }
                                    LibraryContextEntry::FindSimilar => (
                                        Task::none(),
                                        ArtistsAction::FindSimilar(
                                            artist.id.clone(),
                                            format!("Similar to: {}", artist.name),
                                        ),
                                    ),
                                    LibraryContextEntry::TopSongs => (
                                        Task::none(),
                                        ArtistsAction::TopSongs(
                                            artist.name.clone(),
                                            format!("Top Songs: {}", artist.name),
                                        ),
                                    ),
                                    _ => (Task::none(), ArtistsAction::None),
                                },
                                Some(ThreeTierEntry::Child(album, _)) => match entry {
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
                                                .sub_expansion
                                                .children
                                                .first()
                                                .map(|s| s.path.clone()),
                                        };
                                        (Task::none(), ArtistsAction::ShowInfo(Box::new(item)))
                                    }
                                    LibraryContextEntry::ShowInFolder => (
                                        Task::none(),
                                        ArtistsAction::ShowAlbumInFolder(album.id.clone()),
                                    ),
                                    LibraryContextEntry::Separator => {
                                        (Task::none(), ArtistsAction::None)
                                    }
                                    LibraryContextEntry::FindSimilar => {
                                        let aid = album.artist.clone();
                                        (
                                            Task::none(),
                                            ArtistsAction::FindSimilar(
                                                aid,
                                                format!("Similar to: {}", album.name),
                                            ),
                                        )
                                    }
                                    _ => (Task::none(), ArtistsAction::None),
                                },
                                Some(ThreeTierEntry::Grandchild(song, _)) => match entry {
                                    LibraryContextEntry::GetInfo => {
                                        use nokkvi_data::types::info_modal::InfoModalItem;
                                        let item = InfoModalItem::from_song_view_data(song);
                                        (Task::none(), ArtistsAction::ShowInfo(Box::new(item)))
                                    }
                                    LibraryContextEntry::ShowInFolder => (
                                        Task::none(),
                                        ArtistsAction::ShowSongInFolder(song.path.clone()),
                                    ),
                                    LibraryContextEntry::Separator => {
                                        (Task::none(), ArtistsAction::None)
                                    }
                                    LibraryContextEntry::FindSimilar => (
                                        Task::none(),
                                        ArtistsAction::FindSimilar(
                                            song.id.clone(),
                                            format!("Similar to: {}", song.title),
                                        ),
                                    ),
                                    _ => (Task::none(), ArtistsAction::None),
                                },
                                None => (Task::none(), ArtistsAction::None),
                            }
                        }
                    }
                }
                // Common arms already handled by macro above
                _ => (Task::none(), ArtistsAction::None),
            },
        }
    }

    // NOTE: build_flattened_list, collapse, clear are now on self.expansion (ExpansionState)

    /// Build the view
    pub fn view<'a>(&'a self, data: ArtistsViewData<'a>) -> Element<'a, ArtistsMessage> {
        use crate::widgets::view_header::SortMode;

        // Build the columns-visibility dropdown for the artists view header.
        let column_dropdown: Element<'a, ArtistsMessage> = {
            use crate::widgets::checkbox_dropdown::checkbox_dropdown;
            let items: Vec<(ArtistsColumn, &'static str, bool)> = vec![
                (ArtistsColumn::Index, "Index", self.column_visibility.index),
                (
                    ArtistsColumn::Thumbnail,
                    "Thumbnail",
                    self.column_visibility.thumbnail,
                ),
                (ArtistsColumn::Stars, "Stars", self.column_visibility.stars),
                (
                    ArtistsColumn::AlbumCount,
                    "Album Count",
                    self.column_visibility.albumcount,
                ),
                (
                    ArtistsColumn::SongCount,
                    "Song Count",
                    self.column_visibility.songcount,
                ),
                (ArtistsColumn::Plays, "Plays", self.column_visibility.plays),
                (ArtistsColumn::Love, "Love", self.column_visibility.love),
            ];
            checkbox_dropdown(
                "assets/icons/columns-3-cog.svg",
                "Show/hide columns",
                items,
                ArtistsMessage::ToggleColumnVisible,
                |trigger_bounds| match trigger_bounds {
                    Some(b) => ArtistsMessage::SetOpenMenu(Some(
                        crate::app_message::OpenMenu::CheckboxDropdown {
                            view: crate::View::Artists,
                            trigger_bounds: b,
                        },
                    )),
                    None => ArtistsMessage::SetOpenMenu(None),
                },
                data.column_dropdown_open,
                data.column_dropdown_trigger_bounds,
            )
            .into()
        };

        let header = widgets::view_header::view_header(
            self.common.current_sort_mode,
            SortMode::ARTIST_OPTIONS,
            self.common.sort_ascending,
            &self.common.search_query,
            data.artists.len(),
            data.total_artist_count,
            "artists",
            crate::views::ARTISTS_SEARCH_ID,
            ArtistsMessage::SortModeSelected,
            Some(ArtistsMessage::ToggleSortOrder),
            None, // No shuffle button for artists
            Some(ArtistsMessage::RefreshViewData),
            Some(ArtistsMessage::CenterOnPlaying),
            None,                  // on_add
            Some(column_dropdown), // trailing_button
            true,                  // show_search
            ArtistsMessage::SearchQueryChanged,
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

        // If no artists match search, show message but keep the header
        if data.artists.is_empty() {
            return widgets::base_slot_list_empty_state(
                header,
                "No artists match your search.",
                &layout_config,
            );
        }

        // Configure slot list with artists-specific chrome height (has view header)
        use crate::widgets::slot_list::{
            SlotListConfig, chrome_height_with_header, slot_list_view_with_scroll,
        };

        let config =
            SlotListConfig::with_dynamic_slots(data.window_height, chrome_height_with_header())
                .with_modifiers(data.modifiers);
        let artists = data.artists; // Borrow slice to extend lifetime
        let artist_art = data.artist_art;
        let open_menu_for_rows = data.open_menu;

        // Build flattened list (artists + injected albums + injected tracks when expanded)
        let flattened = super::expansion::build_three_tier_list(
            artists,
            &self.expansion,
            &self.sub_expansion,
            |a| &a.id,
            |a| &a.id,
        );
        let center_index = self.common.get_center_item_index(flattened.len());

        // Render slot list using generic component with item renderer closure
        let slot_list_content = slot_list_view_with_scroll(
            &self.common.slot_list,
            &flattened,
            &config,
            ArtistsMessage::SlotListNavigateUp,
            ArtistsMessage::SlotListNavigateDown,
            {
                let total = flattened.len();
                move |f| ArtistsMessage::SlotListScrollSeek((f * total as f32) as usize)
            },
            |entry, ctx| match entry {
                ThreeTierEntry::Parent(artist) => self.render_artist_row(
                    artist,
                    &ctx,
                    artist_art,
                    data.stable_viewport,
                    open_menu_for_rows,
                ),
                ThreeTierEntry::Child(album, _parent_artist_id) => self.render_album_child_row(
                    album,
                    &ctx,
                    data.album_art,
                    data.stable_viewport,
                    open_menu_for_rows,
                ),
                ThreeTierEntry::Grandchild(song, _album_id) => {
                    let track_el = super::expansion::render_child_track_row(
                        song,
                        &ctx,
                        ArtistsMessage::SlotListActivateCenter,
                        if data.stable_viewport {
                            ArtistsMessage::SlotListSetOffset(ctx.item_index, ctx.modifiers)
                        } else {
                            ArtistsMessage::SlotListClickPlay(ctx.item_index)
                        },
                        Some(ArtistsMessage::ClickToggleStar(ctx.item_index)),
                        None, // click artist - artist is already the parent anyway, so maybe None? Or wait, can click it to search artist in artists view? No, we are already in artists view.
                        2,    // depth 2: grandchild tracks (artist → album → track)
                    );
                    use crate::widgets::context_menu::{
                        context_menu, library_entry_view, open_state_for, song_entries_with_folder,
                    };
                    let item_idx = ctx.item_index;
                    let cm_id = crate::app_message::ContextMenuId::LibraryRow {
                        view: crate::View::Artists,
                        item_index: item_idx,
                    };
                    let (cm_open, cm_position) = open_state_for(open_menu_for_rows, &cm_id);
                    context_menu(
                        track_el,
                        song_entries_with_folder(),
                        move |entry, length| {
                            library_entry_view(entry, length, |e| {
                                ArtistsMessage::ContextMenuAction(item_idx, e)
                            })
                        },
                        cm_open,
                        cm_position,
                        move |position| match position {
                            Some(p) => ArtistsMessage::SetOpenMenu(Some(
                                crate::app_message::OpenMenu::Context {
                                    id: cm_id.clone(),
                                    position: p,
                                },
                            )),
                            None => ArtistsMessage::SetOpenMenu(None),
                        },
                    )
                    .into()
                }
            },
        );

        // Wrap slot list content with standard background (prevents color bleed-through)
        use crate::widgets::slot_list::slot_list_background_container;
        let slot_list_content = slot_list_background_container(slot_list_content);

        use crate::widgets::base_slot_list_layout::single_artwork_panel_with_pill;

        // Build artwork column — show parent artist art even when on a child or grandchild
        let centered_artist = center_index.and_then(|idx| match flattened.get(idx) {
            Some(ThreeTierEntry::Parent(artist)) => {
                Some(artists.iter().find(|a| a.id == artist.id)?)
            }
            Some(ThreeTierEntry::Child(_, parent_id)) => {
                artists.iter().find(|a| &a.id == parent_id)
            }
            Some(ThreeTierEntry::Grandchild(_, _)) => {
                // grandchild: look up via sub_expansion parent (album) → outer expansion parent (artist)
                self.expansion
                    .expanded_id
                    .as_ref()
                    .and_then(|aid| artists.iter().find(|a| &a.id == aid))
            }
            None => None,
        });

        let artwork_handle = centered_artist.and_then(|artist| data.large_artwork.get(&artist.id));
        let active_dominant_color =
            centered_artist.and_then(|artist| data.dominant_colors.get(&artist.id).copied());

        let pill_content = centered_artist
            .filter(|_| crate::theme::artists_artwork_overlay())
            .map(|artist| {
                use iced::widget::{button, column, text};

                use crate::theme;

                let mut col = column![
                    text(artist.name.clone())
                        .size(24)
                        .font(iced::Font {
                            weight: iced::font::Weight::Bold,
                            ..theme::ui_font()
                        })
                        .color(theme::fg0()),
                ]
                .spacing(4)
                .align_x(iced::Alignment::Center);

                use crate::widgets::metadata_pill::{auth_status_row, dot_row, play_stats_row};

                let mut lib_stats = vec![
                    format!("{} albums", artist.album_count),
                    format!("{} songs", artist.song_count),
                ];
                if let Some(plays) = artist.play_count {
                    lib_stats.push(format!("{plays} plays"));
                }

                if let Some(row) = dot_row::<ArtistsMessage>(lib_stats, 14.0, theme::fg2()) {
                    col = col.push(
                        iced::widget::container(row)
                            .width(iced::Length::Shrink)
                            .center_x(iced::Length::Fill),
                    );
                }

                if let Some(row) =
                    play_stats_row::<ArtistsMessage>(None, artist.play_date.as_deref())
                {
                    col = col.push(
                        iced::widget::container(row)
                            .width(iced::Length::Shrink)
                            .center_x(iced::Length::Fill),
                    );
                }

                if let Some(row) =
                    auth_status_row::<ArtistsMessage>(artist.is_starred, artist.rating)
                {
                    col = col.push(row);
                }

                // Biography section (artists-specific)
                if let Some(bio) = &artist.biography
                    && !bio.is_empty()
                {
                    let bio_preview: String = bio.chars().take(350).collect();
                    let bio_preview = if bio.chars().count() > 350 {
                        format!("{}...", bio_preview.trim_end())
                    } else {
                        bio_preview
                    };

                    let mut bio_col = column![
                        text(bio_preview)
                            .size(13)
                            .color(theme::fg1())
                            .font(theme::ui_font())
                            .center()
                    ]
                    .spacing(4)
                    .align_x(iced::Alignment::Center);

                    if let Some(url) = &artist.external_url {
                        let read_more_btn = button(
                            text("Read more on Last.fm")
                                .size(11)
                                .color(theme::accent_bright())
                                .font(iced::Font {
                                    weight: iced::font::Weight::Bold,
                                    ..theme::ui_font()
                                }),
                        )
                        .on_press(ArtistsMessage::OpenExternalUrl(url.clone()))
                        .padding(iced::Padding {
                            top: 2.0,
                            bottom: 2.0,
                            left: 6.0,
                            right: 6.0,
                        })
                        .style(|_theme, status| {
                            let opacity = match status {
                                iced::widget::button::Status::Hovered
                                | iced::widget::button::Status::Pressed => 0.75,
                                _ => 1.0,
                            };
                            let mut color = theme::accent_bright();
                            color.a = opacity;
                            iced::widget::button::Style {
                                background: None,
                                border: iced::Border {
                                    width: 0.0,
                                    ..Default::default()
                                },
                                text_color: color,
                                ..Default::default()
                            }
                        });
                        bio_col = bio_col.push(read_more_btn);
                    }

                    col = col.push(bio_col);
                }

                col.into()
            });

        // Artists artwork panel has no refresh action wired up, but still
        // needs the controlled-component plumbing arguments. They're inert
        // when `on_refresh` is None — the helper short-circuits.
        let artwork_content = Some(single_artwork_panel_with_pill::<ArtistsMessage>(
            artwork_handle,
            pill_content,
            active_dominant_color,
            None,
            false,
            None,
            |_| ArtistsMessage::SetOpenMenu(None),
        ));

        crate::widgets::base_slot_list_layout::base_slot_list_layout_with_handle(
            &layout_config,
            header,
            slot_list_content,
            artwork_content,
            Some(ArtistsMessage::ArtworkColumnDrag),
        )
    }

    /// Render an artist row in the slot list (standard layout)
    fn render_artist_row<'a>(
        &self,
        artist: &ArtistUIViewData,
        ctx: &crate::widgets::slot_list::SlotListRowContext,
        artist_art: &'a HashMap<String, image::Handle>,
        stable_viewport: bool,
        open_menu: Option<&'a crate::app_message::OpenMenu>,
    ) -> Element<'a, ArtistsMessage> {
        use crate::widgets::slot_list::{
            SLOT_LIST_SLOT_PADDING, SlotListSlotStyle, slot_list_index_column,
        };

        let artist_id = artist.id.clone();
        let artist_name = artist.name.clone();
        let album_count = artist.album_count;
        let song_count = artist.song_count;
        let is_starred = artist.is_starred;
        let rating = artist.rating.unwrap_or(0).min(5) as usize;
        let play_count = artist.play_count.unwrap_or(0);

        // Check if this artist is the expanded one (gives it the group highlight)
        let is_expanded = self.expansion.is_expanded_parent(&artist.id);
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
        let title_size = m.title_size;
        let metadata_size = m.metadata_size;
        let star_size = m.star_size;
        let index_size = m.metadata_size;

        // Per-column visibility (sort overrides Stars/Plays toggles).
        let sort = self.common.current_sort_mode;
        let vis = self.column_visibility;
        let show_stars = artists_stars_visible(sort, vis.stars);
        let show_albumcount = vis.albumcount;
        let show_songcount = vis.songcount;
        let show_plays = artists_plays_visible(sort, vis.plays);
        let show_love = vis.love;

        // Fixed portions for each toggleable column. Name column expands to
        // fill whatever's left.
        const STARS_PORTION: u16 = 12;
        const ALBUMCOUNT_PORTION: u16 = 16;
        const SONGCOUNT_PORTION: u16 = 16;
        const PLAYS_PORTION: u16 = 16;
        const LOVE_PORTION: u16 = 5;
        let mut consumed: u16 = 0;
        if show_stars {
            consumed += STARS_PORTION;
        }
        if show_albumcount {
            consumed += ALBUMCOUNT_PORTION;
        }
        if show_songcount {
            consumed += SONGCOUNT_PORTION;
        }
        if show_plays {
            consumed += PLAYS_PORTION;
        }
        if show_love {
            consumed += LOVE_PORTION;
        }
        let name_portion = 100u16.saturating_sub(consumed).max(20);

        // Leading columns (Index, Thumbnail) are now user-toggleable; Name is always-on.
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
                artist_art.get(&artist_id),
                artwork_size,
                ctx.is_center,
                false,
                ctx.opacity,
            ));
        }
        content_row = content_row.push({
            let title_click = Some(ArtistsMessage::ContextMenuAction(
                ctx.item_index,
                crate::widgets::context_menu::LibraryContextEntry::GetInfo,
            ));
            let link_color = if ctx.is_center {
                style.text_color
            } else {
                crate::theme::accent_bright()
            };
            container(
                crate::widgets::link_text::LinkText::new(artist_name)
                    .size(title_size)
                    .color(style.text_color)
                    .hover_color(link_color)
                    .on_press(title_click),
            )
            .width(Length::FillPortion(name_portion))
            .height(Length::Fill)
            .clip(true)
            .align_y(Alignment::Center)
        });

        // Stars column (auto-show on sort=Rating). Replaces the old inline
        // star widget under the artist name.
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
                Some(move |star: usize| ArtistsMessage::ClickSetRating(idx, star)),
            ));
        }

        // Album Count column.
        if show_albumcount {
            use crate::widgets::slot_list::slot_list_metadata_column;
            let album_text = if album_count == 1 {
                "1 album".to_string()
            } else {
                format!("{album_count} albums")
            };
            let idx = ctx.item_index;
            content_row = content_row.push(slot_list_metadata_column(
                album_text,
                Some(ArtistsMessage::FocusAndExpand(idx)),
                metadata_size,
                style,
                ALBUMCOUNT_PORTION,
            ));
        }

        // Song Count column.
        if show_songcount {
            use crate::widgets::slot_list::slot_list_metadata_column;
            content_row = content_row.push(slot_list_metadata_column(
                format!("{song_count} songs"),
                None,
                metadata_size,
                style,
                SONGCOUNT_PORTION,
            ));
        }

        // Plays column (auto-show on sort=MostPlayed).
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

        // Heart (Love) column.
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
                    Some(ArtistsMessage::ClickToggleStar(ctx.item_index)),
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
            .height(Length::Fill);

        // Wrap in clickable container
        let clickable = container(content)
            .style(move |_theme| style.to_container_style())
            .width(Length::Fill);

        let slot_button = button(clickable)
            .on_press(if ctx.modifiers.control() || ctx.modifiers.shift() {
                ArtistsMessage::SlotListSetOffset(ctx.item_index, ctx.modifiers)
            } else if ctx.is_center {
                ArtistsMessage::SlotListActivateCenter
            } else if stable_viewport {
                ArtistsMessage::SlotListSetOffset(ctx.item_index, ctx.modifiers)
            } else {
                ArtistsMessage::SlotListClickPlay(ctx.item_index)
            })
            .style(|_theme, _status| button::Style {
                background: None,
                border: iced::Border::default(),
                ..Default::default()
            })
            .padding(0)
            .width(Length::Fill);

        use crate::widgets::context_menu::{
            artist_entries_with_folder, context_menu, library_entry_view, open_state_for,
        };
        let item_idx = ctx.item_index;
        let cm_id = crate::app_message::ContextMenuId::LibraryRow {
            view: crate::View::Artists,
            item_index: item_idx,
        };
        let (cm_open, cm_position) = open_state_for(open_menu, &cm_id);
        context_menu(
            slot_button,
            artist_entries_with_folder(),
            move |entry, length| {
                library_entry_view(entry, length, |e| {
                    ArtistsMessage::ContextMenuAction(item_idx, e)
                })
            },
            cm_open,
            cm_position,
            move |position| match position {
                Some(p) => {
                    ArtistsMessage::SetOpenMenu(Some(crate::app_message::OpenMenu::Context {
                        id: cm_id.clone(),
                        position: p,
                    }))
                }
                None => ArtistsMessage::SetOpenMenu(None),
            },
        )
        .into()
    }

    /// Render a child album row in the slot list (indented, simpler layout)
    fn render_album_child_row<'a>(
        &self,
        album: &AlbumUIViewData,
        ctx: &crate::widgets::slot_list::SlotListRowContext,
        album_art: &'a HashMap<String, image::Handle>,
        stable_viewport: bool,
        open_menu: Option<&'a crate::app_message::OpenMenu>,
    ) -> Element<'a, ArtistsMessage> {
        let album_el = super::expansion::render_child_album_row(
            album,
            ctx,
            album_art.get(&album.id),
            self.column_visibility.thumbnail,
            ArtistsMessage::SlotListActivateCenter,
            if stable_viewport {
                ArtistsMessage::SlotListSetOffset(ctx.item_index, ctx.modifiers)
            } else {
                ArtistsMessage::SlotListClickPlay(ctx.item_index)
            },
            false, // artist is already the parent row
            Some(ArtistsMessage::ClickToggleStar(ctx.item_index)),
            Some(ArtistsMessage::FocusAndExpandAlbum(ctx.item_index)),
            Some(ArtistsMessage::FocusAndExpandAlbum(ctx.item_index)),
            None, // artist click - artist is already the parent
            1,    // depth 1: child albums under artist
        );

        use crate::widgets::context_menu::{
            context_menu, library_entries_with_folder, library_entry_view, open_state_for,
        };
        let item_idx = ctx.item_index;
        let cm_id = crate::app_message::ContextMenuId::LibraryRow {
            view: crate::View::Artists,
            item_index: item_idx,
        };
        let (cm_open, cm_position) = open_state_for(open_menu, &cm_id);
        context_menu(
            album_el,
            library_entries_with_folder(),
            move |entry, length| {
                library_entry_view(entry, length, |e| {
                    ArtistsMessage::ContextMenuAction(item_idx, e)
                })
            },
            cm_open,
            cm_position,
            move |position| match position {
                Some(p) => {
                    ArtistsMessage::SetOpenMenu(Some(crate::app_message::OpenMenu::Context {
                        id: cm_id.clone(),
                        position: p,
                    }))
                }
                None => ArtistsMessage::SetOpenMenu(None),
            },
        )
        .into()
    }
}

// ============================================================================
// ViewPage trait implementation
// ============================================================================

impl super::ViewPage for ArtistsPage {
    fn common(&self) -> &SlotListPageState {
        &self.common
    }
    fn common_mut(&mut self) -> &mut SlotListPageState {
        &mut self.common
    }

    fn is_expanded(&self) -> bool {
        self.expansion.is_expanded() || self.sub_expansion.is_expanded()
    }
    fn collapse_expansion_message(&self) -> Option<Message> {
        if self.sub_expansion.is_expanded() {
            // Inner collapse first
            Some(Message::Artists(ArtistsMessage::CollapseAlbumExpansion))
        } else {
            Some(Message::Artists(ArtistsMessage::CollapseExpansion))
        }
    }

    fn search_input_id(&self) -> &'static str {
        super::ARTISTS_SEARCH_ID
    }

    fn sort_mode_options(&self) -> Option<&'static [SortMode]> {
        Some(SortMode::ARTIST_OPTIONS)
    }
    fn sort_mode_selected_message(&self, mode: SortMode) -> Option<Message> {
        Some(Message::Artists(ArtistsMessage::SortModeSelected(mode)))
    }
    fn toggle_sort_order_message(&self) -> Message {
        Message::Artists(ArtistsMessage::ToggleSortOrder)
    }

    fn add_to_queue_message(&self) -> Option<Message> {
        Some(Message::Artists(ArtistsMessage::AddCenterToQueue))
    }
    fn expand_center_message(&self) -> Option<Message> {
        // Always dispatch ExpandCenter; update() inspects the centered 3-tier
        // entry and routes parent rows to outer-collapse and child/grandchild
        // rows to the album sub-expansion handler. Mirrors Albums/Playlists
        // toggle-on-self semantics.
        Some(Message::Artists(ArtistsMessage::ExpandCenter))
    }
    fn reload_message(&self) -> Option<Message> {
        Some(Message::LoadArtists)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artists_column_visibility_default_all_on() {
        let v = ArtistsColumnVisibility::default();
        assert!(v.stars);
        assert!(v.albumcount);
        assert!(v.songcount);
        assert!(v.plays);
        assert!(v.love);
    }

    #[test]
    fn artists_column_visibility_get_set_round_trip() {
        let mut v = ArtistsColumnVisibility::default();
        v.set(ArtistsColumn::Stars, false);
        v.set(ArtistsColumn::Plays, false);
        assert!(!v.get(ArtistsColumn::Stars));
        assert!(v.get(ArtistsColumn::AlbumCount));
        assert!(v.get(ArtistsColumn::SongCount));
        assert!(!v.get(ArtistsColumn::Plays));
        assert!(v.get(ArtistsColumn::Love));
    }

    #[test]
    fn artists_stars_visible_auto_shows_on_rating_sort() {
        assert!(artists_stars_visible(SortMode::Rating, false));
        assert!(artists_stars_visible(SortMode::Rating, true));
    }

    #[test]
    fn artists_stars_visible_follows_toggle_for_other_sorts() {
        assert!(!artists_stars_visible(SortMode::Name, false));
        assert!(artists_stars_visible(SortMode::Name, true));
    }

    #[test]
    fn artists_plays_visible_auto_shows_on_most_played() {
        assert!(artists_plays_visible(SortMode::MostPlayed, false));
        assert!(artists_plays_visible(SortMode::MostPlayed, true));
    }

    #[test]
    fn artists_plays_visible_follows_toggle_for_other_sorts() {
        assert!(!artists_plays_visible(SortMode::Name, false));
        assert!(artists_plays_visible(SortMode::Name, true));
    }

    #[test]
    fn toggle_column_visible_flips_state_and_emits_action() {
        let mut page = ArtistsPage::default();
        let artists: Vec<ArtistUIViewData> = Vec::new();

        let (_t, action) = page.update(
            ArtistsMessage::ToggleColumnVisible(ArtistsColumn::Plays),
            0,
            &artists,
        );
        assert!(!page.column_visibility.plays);
        assert!(matches!(
            action,
            ArtistsAction::ColumnVisibilityChanged(ArtistsColumn::Plays, false)
        ));
    }
}
