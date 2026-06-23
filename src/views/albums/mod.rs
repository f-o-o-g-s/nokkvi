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
//!   fn render_track_row }` and the dynamic-slot value resolver
//!   (`get_extra_column_value`). The Stars / Plays auto-show-on-sort
//!   decision is centralized in `crate::views::auto_show_on_sort`.

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
// mode is active (see view::view::auto_show_on_sort with
// [SortMode::Rating] / [SortMode::MostPlayed] respectively). SongCount
// and Love default on (always-shown today). Index/Thumbnail default on
// to match historical always-on rendering of those leading columns.
// Select defaults off — opt-in discovery affordance for multi-selection.
super::define_view_columns! {
    AlbumsColumn => AlbumsColumnVisibility {
        Select("Select"): select = false => set_albums_show_select @ albums_show_select,
        Index("Index"): index = true => set_albums_show_index @ albums_show_index,
        Thumbnail("Thumbnail"): thumbnail = true => set_albums_show_thumbnail @ albums_show_thumbnail,
        Stars("Stars"): stars = false => set_albums_show_stars @ albums_show_stars,
        SongCount("Song Count"): songcount = true => set_albums_show_songcount @ albums_show_songcount,
        Plays("Plays"): plays = false => set_albums_show_plays @ albums_show_plays,
        Love("Love"): love = true => set_albums_show_love @ albums_show_love,
    }
}

/// View data passed from root (read-only, borrows from app state to avoid allocations)
pub struct AlbumsViewData<'a> {
    pub albums: &'a [AlbumUIViewData],
    pub album_art: &'a HashMap<String, image::Handle>,
    pub large_artwork: &'a HashMap<String, image::Handle>,
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
    /// Whether artwork-elevation is in effect for this frame. Forwarded into
    /// BaseSlotListLayoutConfig.elevated. Always false in split-view /
    /// side-nav / none-nav.
    pub elevated: bool,
    /// Shared overlay-menu plumbing (column-dropdown open/bounds + borrowed
    /// `open_menu` reference). See `super::OverlayMenuViewData`.
    pub overlay: super::OverlayMenuViewData<'a>,
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
    PlayAlbum(String, bool), // (album_id, force_shuffle) - replace queue and play
    /// Replace the queue with the selection/clicked item and play. `true` shuffles
    /// the batch once (Ctrl+Enter / context-menu Shuffle Play); `false` honors the
    /// `enter_shuffle` setting.
    PlayBatch(nokkvi_data::types::batch::BatchPayload, bool),
    AddBatchToQueue(nokkvi_data::types::batch::BatchPayload),
    LoadLargeArtwork(String), // center_idx as string
    CenterOnPlaying,
    /// Expand album inline — root should load tracks (album_id)
    ExpandAlbum(String),
    /// Play album from a specific track (album_id, track_index, force_shuffle)
    PlayAlbumFromTrack(String, usize, bool),
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

    fn slot_list_message(&self, msg: crate::widgets::SlotListPageMessage) -> Message {
        Message::Albums(AlbumsMessage::SlotList(msg))
    }

    fn uses_horizontal_artwork_column(&self) -> bool {
        true
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
