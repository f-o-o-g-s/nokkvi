//! Albums Page Component
//!
//! Self-contained albums view with slot list navigation, search, and filtering.
//! Uses message bubbling pattern to communicate global actions to root.
//! Supports inline track expansion (Shift+Enter) using flattened SlotListEntry list.
//!
//! Module layout:
//! - `mod.rs`: types (Page, Message, Action, ViewData), trait impls, tests
//!   for column declarations
//! - `update.rs`: `impl AlbumsPage { fn update }` + the
//!   center-on-playing/toggle-column tests
//! - `view.rs`: `impl AlbumsPage { fn view, fn render_album_row,
//!   fn render_track_row }`, the per-mode column helpers
//!   (`albums_stars_visible`, `albums_plays_visible`), the dynamic-slot
//!   value resolver (`get_extra_column_value`), and the helper-tests

use std::collections::HashMap;

use iced::widget::image;
use nokkvi_data::{
    backend::{albums::AlbumUIViewData, songs::SongUIViewData},
    types::ItemKind,
};

use super::expansion::ExpansionState;
use crate::{
    app_message::Message,
    widgets::{SlotListPageState, view_header::SortMode},
};

mod update;
mod view;

/// Albums page local state
#[derive(Debug)]
pub struct AlbumsPage {
    pub common: SlotListPageState,
    /// Inline expansion state (album → tracks)
    pub expansion: ExpansionState<SongUIViewData>,
    /// Per-column visibility toggles surfaced via the columns-cog dropdown.
    pub column_visibility: AlbumsColumnVisibility,
}

// Toggleable albums columns. Index/Art/Title+Artist are always shown.
// The dynamic 21% slot still auto-renders Date/Year/Duration/Genre when
// sorted by those modes — Stars and Plays are now dedicated columns.
//
// Stars and Plays default off — today they only appear when their sort
// mode is active (see view::albums_stars_visible / albums_plays_visible).
// SongCount and Love default on (always-shown today). Index/Thumbnail
// default on to match historical always-on rendering of those leading
// columns. Select defaults off — opt-in discovery affordance for
// multi-selection.
super::define_view_columns! {
    AlbumsColumn => AlbumsColumnVisibility {
        Select: select = false => set_albums_show_select,
        Index: index = true => set_albums_show_index,
        Thumbnail: thumbnail = true => set_albums_show_thumbnail,
        Stars: stars = false => set_albums_show_stars,
        SongCount: songcount = true => set_albums_show_songcount,
        Plays: plays = false => set_albums_show_plays,
        Love: love = true => set_albums_show_love,
    }
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
    /// True when this view is rendered inside the library browsing panel
    /// (split-view, right pane). Used to suppress chrome that doesn't fit
    /// the narrower pane — e.g. the "Center on Playing" header button.
    pub in_browsing_panel: bool,
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
    // Slot list navigation / activation / selection / queue (wrapped carrier)
    SlotList(crate::widgets::SlotListPageMessage),

    // Mouse click on star/heart (item_index, value)
    ClickSetRating(usize, usize), // (item_index, rating 1-5)
    ClickToggleStar(usize),       // item_index

    // Context menu
    ContextMenuAction(usize, crate::widgets::context_menu::LibraryContextEntry),

    // Inline expansion (Shift+Enter)
    ExpandCenter,
    FocusAndExpand(usize), // Clicked 'X songs' — focus that row and expand it
    CollapseExpansion,
    /// Tracks loaded for expanded album (album_id, tracks)
    TracksLoaded(String, Vec<SongUIViewData>),

    // View header
    SortModeSelected(crate::widgets::view_header::SortMode),
    ToggleSortOrder,
    SearchQueryChanged(String),
    SearchFocused(bool),
    /// Sort dropdown's "Roulette" entry was selected — intercepted at the
    /// root handler before the page's `update` runs, so the page never
    /// sees this and no per-view state changes here.
    Roulette,

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
    /// Always-Vertical artwork drag handle event — intercepted at root.
    ArtworkColumnVerticalDrag(crate::widgets::artwork_split_handle::DragEvent),
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
    /// Set rating on item (item_id, kind, rating)
    SetRating(String, ItemKind, usize),
    /// Star/unstar item (item_id, kind, new_starred)
    ToggleStar(String, ItemKind, bool),

    LoadPage(usize),       // offset - trigger fetch of next page
    SearchChanged(String), // trigger reload
    SortModeChanged(crate::widgets::view_header::SortMode), // trigger reload
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

super::impl_has_common_action!(AlbumsAction {
    NavigateAndExpandArtist,
    NavigateAndExpandGenre
});

impl Default for AlbumsPage {
    fn default() -> Self {
        Self {
            common: SlotListPageState::new(
                crate::widgets::view_header::SortMode::RecentlyAdded,
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
        Some(super::sort_api::sort_modes_for_view(crate::View::Albums))
    }
    fn sort_mode_selected_message(&self, mode: SortMode) -> Option<Message> {
        Some(Message::Albums(AlbumsMessage::SortModeSelected(mode)))
    }
    fn toggle_sort_order_message(&self) -> Message {
        Message::Albums(AlbumsMessage::ToggleSortOrder)
    }

    fn add_to_queue_message(&self) -> Option<Message> {
        Some(Message::Albums(AlbumsMessage::SlotList(
            crate::widgets::SlotListPageMessage::AddCenterToQueue,
        )))
    }
    fn expand_center_message(&self) -> Option<Message> {
        Some(Message::Albums(AlbumsMessage::ExpandCenter))
    }
    fn reload_message(&self) -> Option<Message> {
        Some(Message::LoadAlbums)
    }

    fn synth_set_offset_message(&self, offset: usize) -> Option<Message> {
        Some(Message::Albums(AlbumsMessage::SlotList(
            crate::widgets::SlotListPageMessage::SetOffset(
                offset,
                iced::keyboard::Modifiers::default(),
            ),
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn albums_column_visibility_default_preserves_today_behavior() {
        let v = AlbumsColumnVisibility::default();
        // Stars/Plays opt-in (today only show on their sort modes).
        assert!(!v.stars);
        assert!(v.songcount);
        assert!(!v.plays);
        assert!(v.love);
    }
}
