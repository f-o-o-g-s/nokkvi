//! Playlists Page Component
//!
//! Self-contained playlists view with slot list navigation, search, and filtering.
//! Uses message bubbling pattern to communicate global actions to root.
//!
//! Module layout:
//! - `mod.rs`: types (Page, Message, Action, ViewData, PlaylistContextEntry), trait impls
//! - `update.rs`: `impl PlaylistsPage { fn update }`
//! - `view.rs`: `impl PlaylistsPage { fn view, fn render_playlist_row, fn render_track_row }`,
//!   plus the column-visibility helpers and the playlist context-menu rendering.

use std::collections::HashMap;

use iced::widget::image;
use nokkvi_data::{
    backend::{playlists::PlaylistUIViewData, songs::SongUIViewData},
    types::ItemKind,
};

use super::expansion::ExpansionState;
use crate::{
    app_message::Message,
    widgets::{SlotListPageState, view_header::SortMode},
};

mod update;
mod view;

/// Playlists page local state
#[derive(Debug)]
pub struct PlaylistsPage {
    pub common: SlotListPageState,
    pub expansion: ExpansionState<SongUIViewData>,
    /// Per-column visibility toggles surfaced via the columns-cog dropdown.
    pub column_visibility: PlaylistsColumnVisibility,
}

// Toggleable playlists columns. The playlist name (title) is always shown;
// SongCount/Duration/UpdatedAt also auto-show when their matching sort mode
// is active regardless of the user toggle (see view::playlists_*_visible).
super::define_view_columns! {
    PlaylistsColumn => PlaylistsColumnVisibility {
        Select: select = false => set_playlists_show_select,
        Index: index = true => set_playlists_show_index,
        Thumbnail: thumbnail = true => set_playlists_show_thumbnail,
        SongCount: songcount = false => set_playlists_show_songcount,
        Duration: duration = false => set_playlists_show_duration,
        UpdatedAt: updatedat = false => set_playlists_show_updatedat,
    }
}

/// View data passed from root (read-only, borrows from app state to avoid allocations)
pub struct PlaylistsViewData<'a> {
    pub playlists: &'a [PlaylistUIViewData],
    pub playlist_artwork: &'a HashMap<String, image::Handle>,
    pub playlist_collage_artwork: &'a HashMap<String, Vec<image::Handle>>,
    pub window_width: f32,
    pub window_height: f32,
    pub scale_factor: f32,
    pub modifiers: iced::keyboard::Modifiers,
    pub total_playlist_count: usize,
    pub loading: bool,
    pub stable_viewport: bool,
    /// Current default playlist's display name (empty when no default set).
    /// Surfaced in the view-header chip.
    pub default_playlist_name: &'a str,
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

/// Context menu entries for playlist parent items.
///
/// Extends the shared `LibraryContextEntry` with playlist-specific actions
/// (delete, rename). Uses a `Separator` variant for visual grouping.
#[derive(Debug, Clone, Copy)]
pub enum PlaylistContextEntry {
    /// Shared library entries (Play, AddToQueue, PlayNext)
    Library(crate::widgets::context_menu::LibraryContextEntry),
    /// Visual separator between shared and playlist-specific entries
    Separator,
    /// Delete this playlist
    Delete,
    /// Rename this playlist
    Rename,
    /// Edit playlist tracks in split-view
    EditPlaylist,
    /// Set this playlist as the default for quick-add
    SetAsDefault,
}

/// Messages for local playlists page interactions
#[derive(Debug, Clone)]
pub enum PlaylistsMessage {
    // Slot list navigation (wrapped carrier — 10 variants, no CenterOnPlaying for Playlists)
    SlotList(crate::widgets::SlotListPageMessage),

    // Mouse click on heart
    ClickToggleStar(usize), // item_index

    // Context menu (shared library entries for child tracks)
    ContextMenuAction(usize, crate::widgets::context_menu::LibraryContextEntry),
    /// Playlist-specific context menu action on a parent playlist
    PlaylistContextAction(usize, PlaylistContextEntry),

    // Expansion
    ExpandCenter,          // Toggle expand/collapse on centered playlist (Shift+Enter)
    FocusAndExpand(usize), // Clicked 'X songs' or playlist name — focus that row and expand it
    CollapseExpansion,     // Collapse current expansion (Escape when expanded)
    TracksLoaded(String, Vec<SongUIViewData>), // playlist_id, tracks

    // View header (sort/search stay per-view — handled by impl_expansion_update! macro)
    SortModeSelected(crate::widgets::view_header::SortMode),
    ToggleSortOrder,
    SearchQueryChanged(String),
    SearchFocused(bool),
    /// Sort dropdown's "Roulette" entry was selected — intercepted at the
    /// root handler before the page's `update` runs.
    Roulette,

    NavigateAndFilter(crate::View, nokkvi_data::types::filter::LibraryFilter), // Navigate to target view and filter
    /// Navigate to Artists and auto-expand the artist with this id (no filter set).
    NavigateAndExpandArtist(String),

    /// Context-menu open/close request — bubbled to root
    /// `Message::SetOpenMenu`. Intercepted in `handle_playlists` before the
    /// page's `update` runs.
    SetOpenMenu(Option<crate::app_message::OpenMenu>),
    /// Artwork column drag handle event — intercepted at root, page never sees it.
    ArtworkColumnDrag(crate::widgets::artwork_split_handle::DragEvent),
    /// Always-Vertical artwork drag handle event — intercepted at root.
    ArtworkColumnVerticalDrag(crate::widgets::artwork_split_handle::DragEvent),
    /// Header chip clicked — bubble to root, opens the default-playlist picker.
    OpenDefaultPlaylistPicker,
    /// View-header `+` button clicked — bubble to root to open the
    /// Create-New-Playlist dialog.
    OpenCreatePlaylistDialog,
    /// Toggle a playlists column's visibility.
    ToggleColumnVisible(PlaylistsColumn),
}

/// Actions that bubble up to root for global state mutation
#[derive(Debug, Clone)]
pub enum PlaylistsAction {
    PlayPlaylist(String), // playlist_id - clear queue and play all songs in playlist
    AddBatchToQueue(nokkvi_data::types::batch::BatchPayload),
    ExpandPlaylist(String), // playlist_id - load tracks for expansion
    PlayPlaylistFromTrack(String, usize), // playlist_id, track_index - play from clicked track
    LoadArtwork(String),    // playlist_id - load artwork for centered playlist on slot list scroll
    PreloadArtwork(usize),  // viewport_offset - preload artwork for visible + buffer
    SearchChanged(String),  // trigger reload
    SortModeChanged(crate::widgets::view_header::SortMode), // trigger reload
    SortOrderChanged(bool), // trigger reload
    RefreshViewData,        // trigger reload
    ToggleStar(String, ItemKind, bool), // (item_id, kind, starred)
    PlayNextBatch(nokkvi_data::types::batch::BatchPayload),
    DeletePlaylist(String),                     // playlist_id
    RenamePlaylist(String),                     // playlist_id — triggers rename flow
    EditPlaylist(String, String, String, bool), // (playlist_id, playlist_name, comment, public) — enter split-view edit mode
    ShowInfo(Box<nokkvi_data::types::info_modal::InfoModalItem>), // Open info modal
    SetAsDefaultPlaylist(String, String), // (playlist_id, playlist_name) — set as quick-add default
    NavigateAndFilter(crate::View, nokkvi_data::types::filter::LibraryFilter), // Navigate to target view and filter
    NavigateAndExpandArtist(String), // artist_id - navigate to Artists and auto-expand
    /// Bubble to root: open the default-playlist picker overlay.
    OpenDefaultPlaylistPicker,
    /// Bubble to root: open the Create-New-Playlist dialog.
    OpenCreatePlaylistDialog,
    /// Persist a column-visibility toggle change (col, new_value).
    ColumnVisibilityChanged(PlaylistsColumn, bool),

    None,
}

super::impl_has_common_action!(
    PlaylistsAction,
    no_center {
        NavigateAndExpandArtist
    }
);

impl Default for PlaylistsPage {
    fn default() -> Self {
        Self {
            common: SlotListPageState::new(
                crate::widgets::view_header::SortMode::Name,
                true, // sort_ascending
            ),
            expansion: ExpansionState::default(),
            column_visibility: PlaylistsColumnVisibility::default(),
        }
    }
}

impl PlaylistsPage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Convert sort mode to API string for server requests.
    /// Thin shim — the unified mapping lives in `views/sort_api.rs`.
    pub fn sort_mode_to_api_string(
        sort_mode: crate::widgets::view_header::SortMode,
    ) -> &'static str {
        super::sort_api::sort_mode_to_api_string(crate::View::Playlists, sort_mode)
    }
}

// ============================================================================
// ViewPage trait implementation
// ============================================================================

impl super::ViewPage for PlaylistsPage {
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
        Some(Message::Playlists(PlaylistsMessage::CollapseExpansion))
    }

    fn search_input_id(&self) -> &'static str {
        super::PLAYLISTS_SEARCH_ID
    }

    fn sort_mode_options(&self) -> Option<&'static [SortMode]> {
        Some(super::sort_api::sort_modes_for_view(crate::View::Playlists))
    }
    fn sort_mode_selected_message(&self, mode: SortMode) -> Option<Message> {
        Some(Message::Playlists(PlaylistsMessage::SortModeSelected(mode)))
    }
    fn toggle_sort_order_message(&self) -> Message {
        Message::Playlists(PlaylistsMessage::ToggleSortOrder)
    }

    fn add_to_queue_message(&self) -> Option<Message> {
        Some(Message::Playlists(PlaylistsMessage::SlotList(
            crate::widgets::SlotListPageMessage::AddCenterToQueue,
        )))
    }
    fn expand_center_message(&self) -> Option<Message> {
        Some(Message::Playlists(PlaylistsMessage::ExpandCenter))
    }
    fn reload_message(&self) -> Option<Message> {
        Some(Message::LoadPlaylists)
    }

    fn synth_set_offset_message(&self, offset: usize) -> Option<Message> {
        Some(Message::Playlists(PlaylistsMessage::SlotList(
            crate::widgets::SlotListPageMessage::SetOffset(
                offset,
                iced::keyboard::Modifiers::default(),
            ),
        )))
    }

    fn slot_list_message(&self, msg: crate::widgets::SlotListPageMessage) -> Message {
        Message::Playlists(PlaylistsMessage::SlotList(msg))
    }
}
