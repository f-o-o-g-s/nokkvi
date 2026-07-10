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
pub(crate) mod view;

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
        Select("Select"): select = false => set_playlists_show_select @ playlists_show_select,
        Index("Index"): index = true => set_playlists_show_index @ playlists_show_index,
        Thumbnail("Thumbnail"): thumbnail = true => set_playlists_show_thumbnail @ playlists_show_thumbnail,
        SongCount("Song count"): songcount = false => set_playlists_show_songcount @ playlists_show_songcount,
        Duration("Duration"): duration = false => set_playlists_show_duration @ playlists_show_duration,
        UpdatedAt("Updated at"): updatedat = false => set_playlists_show_updatedat @ playlists_show_updatedat,
    }
}

/// View data passed from root (read-only, borrows from app state to avoid allocations)
pub struct PlaylistsViewData<'a> {
    pub playlists: &'a [PlaylistUIViewData],
    pub playlist_artwork: &'a HashMap<String, image::Handle>,
    pub playlist_collage_artwork: &'a HashMap<String, Vec<image::Handle>>,
    /// Album-id-keyed 80px thumbnail snapshot (`artwork.album_art`). Slot
    /// rows resolve their 2×2 quad tiles from it via each playlist's
    /// `artwork_album_ids`, falling back to the single `playlist_artwork`
    /// mini while tiles are still cold.
    pub album_art: &'a HashMap<String, image::Handle>,
    /// Mini CUSTOM (user-uploaded) covers snapshot
    /// (`artwork.playlist_custom_art`). A playlist with `uploaded_image` set
    /// AND a handle here renders this single image instead of the quad.
    pub playlist_custom_art: &'a HashMap<String, image::Handle>,
    /// Large CUSTOM covers snapshot (`artwork.playlist_custom_large_art`)
    /// for the artwork panel; falls back to the mini while loading.
    pub playlist_custom_large_art: &'a HashMap<String, image::Handle>,
    pub window_width: f32,
    pub window_height: f32,
    pub scale_factor: f32,
    pub modifiers: iced::keyboard::Modifiers,
    pub total_playlist_count: usize,
    pub loading: bool,
    pub stable_viewport: bool,
    /// Whether artwork-elevation is in effect for this frame. Forwarded into
    /// BaseSlotListLayoutConfig.elevated. Always false in split-view /
    /// side-nav / none-nav.
    pub elevated: bool,
    /// Current default playlist's display name (empty when no default set).
    /// Surfaced in the view-header chip.
    pub default_playlist_name: &'a str,
    /// Shared overlay-menu plumbing (column-dropdown open/bounds + borrowed
    /// `open_menu` reference). See `super::OverlayMenuViewData`.
    pub overlay: super::OverlayMenuViewData<'a>,
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
    /// Pick an image file and upload it as the playlist's custom cover.
    SetCustomArtwork,
    /// Delete the uploaded cover server-side; the collage returns. Listed
    /// only when the playlist actually has an uploaded image.
    ResetArtwork,
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
    /// Panel-menu "Set Custom Artwork…" — `(playlist_id, playlist_name)`
    /// resolved at view-build time (the centered playlist), so the handler
    /// never re-resolves. Row menus reach the same action through
    /// `PlaylistContextAction(idx, SetCustomArtwork)`.
    SetCustomArtwork(String, String),
    /// Panel-menu "Reset Artwork" — `(playlist_id, playlist_name)`.
    ResetCustomArtwork(String, String),
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
    PlayPlaylist(String, bool), // (playlist_id, force_shuffle) - replace queue and play
    /// Replace the queue with the selection/clicked item and play. `true` shuffles
    /// the batch once (context-menu Shuffle Play); `false` honors `enter_shuffle`.
    PlayBatch(nokkvi_data::types::batch::BatchPayload, bool),
    AddBatchToQueue(nokkvi_data::types::batch::BatchPayload),
    /// Add the resolved selection to the Trawl crate as labeled seeds.
    AddBatchToMix(Vec<nokkvi_data::types::trawl::TrawlSeed>),
    ExpandPlaylist(String), // playlist_id - load tracks for expansion
    PlayPlaylistFromTrack(String, usize, bool), // (playlist_id, track_index, force_shuffle)
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
    /// Root should run the pick-file → upload flow. `(playlist_id, name)`.
    SetCustomArtwork(String, String),
    /// Root should DELETE the playlist's uploaded cover. `(playlist_id, name)`.
    ResetCustomArtwork(String, String),
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

    fn uses_horizontal_artwork_column(&self) -> bool {
        true
    }
}
