//! Artists Page Component
//!
//! Self-contained artists view with slot list navigation, search, and filtering.
//! Uses message bubbling pattern to communicate global actions to root.
//! Supports inline album expansion (Shift+Enter) using flattened SlotListEntry list.
//!
//! Module layout:
//! - `mod.rs`: types (Page, Message, Action, ViewData), trait impls, tests
//!   for column declarations
//! - `update.rs`: `impl ArtistsPage { fn update }` + the toggle-column test
//! - `view.rs`: `impl ArtistsPage { fn view, fn render_artist_row,
//!   fn render_album_child_row }`, the per-mode column helpers
//!   (`artists_stars_visible`, `artists_plays_visible`), and helper tests

use std::collections::HashMap;

use iced::widget::image;
use nokkvi_data::backend::{albums::AlbumUIViewData, artists::ArtistUIViewData};

use super::expansion::ExpansionState;
use crate::{
    app_message::Message,
    widgets::{SlotListPageState, view_header::SortMode},
};

mod update;
mod view;

/// Artists page local state
#[derive(Debug)]
pub struct ArtistsPage {
    pub common: SlotListPageState,
    /// Inline expansion state (artist → albums)
    pub expansion: ExpansionState<AlbumUIViewData>,
    /// Per-column visibility toggles surfaced via the columns-cog dropdown.
    pub column_visibility: ArtistsColumnVisibility,
}

// Toggleable artists columns. The artist name is always shown; everything
// else is user-toggleable through the columns-cog dropdown.
//
// All-on matches today's permanent layout (after the Plays-column commit) —
// no surprise visual change on first launch. Select defaults off — opt-in
// discovery affordance for multi-selection.
super::define_view_columns! {
    ArtistsColumn => ArtistsColumnVisibility {
        Select: select = false,
        Index: index = true,
        Thumbnail: thumbnail = true,
        Stars: stars = true,
        AlbumCount: albumcount = true,
        SongCount: songcount = true,
        Plays: plays = true,
        Love: love = true,
    }
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
    /// True when this view is rendered inside the library browsing panel
    /// (split-view, right pane). Used to suppress chrome that doesn't fit
    /// the narrower pane — e.g. the "Center on Playing" header button.
    pub in_browsing_panel: bool,
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
    /// Click on a row's leading select checkbox — toggles `item_index` in
    /// `selected_indices`. No play/highlight side effects.
    SlotListSelectionToggle(usize),
    /// Click on the tri-state "select all" header — fills selection with
    /// every visible row, or clears if every visible row is already selected.
    SlotListSelectAllToggle,
    AddCenterToQueue, // Add all songs from centered artist to queue (Shift+Q)

    // Mouse click on star/heart (item_index, value)
    ClickSetRating(usize, usize), // (item_index, rating 1-5)
    ClickToggleStar(usize),       // item_index

    // Context menu
    ContextMenuAction(usize, crate::widgets::context_menu::LibraryContextEntry),

    // Inline expansion (Shift+Enter on artist)
    ExpandCenter,
    FocusAndExpand(usize), // Clicked 'X albums' — focus that row and expand it
    CollapseExpansion,
    /// Albums loaded for expanded artist (artist_id, albums)
    AlbumsLoaded(String, Vec<AlbumUIViewData>),

    /// Click on a child album row's "X songs" / album-name link, or
    /// Shift+Enter on a centered child album row. Bubbles up as
    /// `ArtistsAction::NavigateAndExpandAlbum`, which the root translates
    /// into a top- or browsing-pane Albums-view switch with the album
    /// already primed for expansion.
    NavigateAndExpandAlbum(String),

    // View header
    SortModeSelected(crate::widgets::view_header::SortMode),
    ToggleSortOrder,
    SearchQueryChanged(String),
    SearchFocused(bool),
    RefreshViewData,
    CenterOnPlaying,
    ToggleColumnVisible(ArtistsColumn),
    /// Sort dropdown's "Roulette" entry was selected — intercepted at the
    /// root handler before the page's `update` runs.
    Roulette,

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
    StarArtist(String),   // artist_id - star the artist
    UnstarArtist(String), // artist_id - unstar the artist
    /// Set absolute rating on item (item_id, item_type, rating)
    SetRating(String, &'static str, usize),
    /// Star/unstar item by click (item_id, item_type, new_starred)
    ToggleStar(String, &'static str, bool),
    /// Expand artist inline — root should load albums (artist_id)
    ExpandArtist(String),
    /// Switch to Albums view and prime the named album for inline expansion.
    /// Carries the album id so the root can route to top vs browsing pane.
    NavigateAndExpandAlbum(String),
    LoadPage(usize),       // offset - trigger fetch of next page
    SearchChanged(String), // trigger reload
    SortModeChanged(crate::widgets::view_header::SortMode), // trigger reload
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

super::impl_has_common_action!(ArtistsAction {
    NavigateAndExpandAlbum
});

impl Default for ArtistsPage {
    fn default() -> Self {
        Self {
            common: SlotListPageState::new(
                crate::widgets::view_header::SortMode::Name,
                true, // sort_ascending
            ),
            expansion: ExpansionState::default(),
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
        self.expansion.is_expanded()
    }
    fn collapse_expansion_message(&self) -> Option<Message> {
        Some(Message::Artists(ArtistsMessage::CollapseExpansion))
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
        // ExpandCenter on a 2nd-tier album row routes through `update()`'s
        // pre-check to NavigateAndExpandAlbum (cross-view drill-down);
        // parent rows toggle inline expansion via the macro.
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
}
