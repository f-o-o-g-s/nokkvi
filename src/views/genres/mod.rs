//! Genres Page Component
//!
//! Self-contained genres view with slot list navigation, search, and filtering.
//! Uses message bubbling pattern to communicate global actions to root.
//!
//! Module layout:
//! - `mod.rs`: types (Page, Message, Action, ViewData), trait impls, public re-exports
//! - `update.rs`: `impl GenresPage { fn update, fn resolve_artwork_action }`
//! - `view.rs`: `impl GenresPage { fn view, fn render_genre_row, fn render_album_row }`

use std::collections::HashMap;

use iced::widget::image;
use nokkvi_data::{
    backend::{albums::AlbumUIViewData, genres::GenreUIViewData},
    types::ItemKind,
};

use super::expansion::ExpansionState;
use crate::{
    app_message::Message,
    widgets::{self, SlotListPageState, view_header::SortMode},
};

mod update;
mod view;

/// Genres page local state
#[derive(Debug)]
pub struct GenresPage {
    pub common: SlotListPageState,
    /// Inline expansion state (genre → albums)
    pub expansion: ExpansionState<AlbumUIViewData>,
    /// Per-column visibility toggles surfaced via the columns-cog dropdown.
    pub column_visibility: GenresColumnVisibility,
}

// Toggleable genres columns. The genre name (title) is always shown; everything
// else is user-toggleable through the columns-cog dropdown. The thumbnail flag
// also drives whether nested child album rows in the genre→album expansion
// render their artwork column.
super::define_view_columns! {
    GenresColumn => GenresColumnVisibility {
        Select("Select"): select = false => set_genres_show_select @ genres_show_select,
        Index("Index"): index = true => set_genres_show_index @ genres_show_index,
        Thumbnail("Thumbnail"): thumbnail = true => set_genres_show_thumbnail @ genres_show_thumbnail,
        AlbumCount("Album count"): albumcount = true => set_genres_show_albumcount @ genres_show_albumcount,
        SongCount("Song count"): songcount = true => set_genres_show_songcount @ genres_show_songcount,
    }
}

/// View data passed from root (read-only, borrows from app state to avoid allocations)
pub struct GenresViewData<'a> {
    pub genres: &'a [GenreUIViewData],
    pub genre_artwork: &'a HashMap<String, image::Handle>,
    pub genre_collage_artwork: &'a HashMap<String, Vec<image::Handle>>,
    /// Album artwork cache, keyed by album_id. Used by nested child album
    /// rows in the genre→album expansion when `column_visibility.thumbnail`
    /// is enabled.
    pub album_art: &'a HashMap<String, image::Handle>,
    pub window_width: f32,
    pub window_height: f32,
    pub scale_factor: f32,
    pub modifiers: iced::keyboard::Modifiers,
    pub total_genre_count: usize,
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

/// Messages for local genre page interactions
#[derive(Debug, Clone)]
pub enum GenresMessage {
    // Slot list navigation (consolidated)
    SlotList(crate::widgets::SlotListPageMessage),

    // Mouse click on heart
    ClickToggleStar(usize), // item_index

    // Context menu
    ContextMenuAction(usize, crate::widgets::context_menu::LibraryContextEntry),

    // Inline expansion (Shift+Enter on genre)
    ExpandCenter,
    FocusAndExpand(usize), // Clicked 'X albums' — focus that row and expand it
    CollapseExpansion,
    /// Albums loaded for expanded genre (genre_id, albums)
    AlbumsLoaded(String, Vec<AlbumUIViewData>),

    /// Click on a child album row's "X songs" / album-name link, or
    /// Shift+Enter on a centered child album row. Bubbles up as
    /// `GenresAction::NavigateAndExpandAlbum` for cross-view drill-down.
    NavigateAndExpandAlbum(String),

    // View header
    SortModeSelected(widgets::view_header::SortMode),
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
    /// `Message::SetOpenMenu`. Intercepted in `handle_genres` before the
    /// page's `update` runs.
    SetOpenMenu(Option<crate::app_message::OpenMenu>),
    /// Artwork column drag handle event — intercepted at root, page never sees it.
    ArtworkColumnDrag(crate::widgets::artwork_split_handle::DragEvent),
    /// Always-Vertical artwork drag handle event — intercepted at root.
    ArtworkColumnVerticalDrag(crate::widgets::artwork_split_handle::DragEvent),
    /// Toggle a genres column's visibility (currently only Thumbnail).
    ToggleColumnVisible(GenresColumn),
}

/// Actions that bubble up to root for global state mutation
#[derive(Debug, Clone)]
pub enum GenresAction {
    PlayGenre(String, bool), // (genre_id, force_shuffle) - replace queue and play
    /// Replace the queue with the selection/clicked item, shuffled once, and play.
    PlayBatchShuffled(nokkvi_data::types::batch::BatchPayload),
    AddBatchToQueue(nokkvi_data::types::batch::BatchPayload),
    PlayAlbum(String, bool), // (album_id, force_shuffle) - play child album
    /// Expand genre inline — root should load albums (genre_name, genre_id)
    ExpandGenre(String, String),
    /// Switch to Albums view and prime the named album for inline expansion.
    NavigateAndExpandAlbum(String),
    LoadArtwork(String), // genre_id - load artwork for centered genre on slot list scroll
    PreloadArtwork(usize), // viewport_offset - preload artwork for visible + buffer
    SearchChanged(String), // trigger reload
    SortModeChanged(widgets::view_header::SortMode), // trigger reload
    SortOrderChanged(bool), // trigger reload
    RefreshViewData,     // trigger reload
    ToggleStar(String, ItemKind, bool), // (item_id, kind, starred)
    PlayNextBatch(nokkvi_data::types::batch::BatchPayload),
    AddBatchToPlaylist(nokkvi_data::types::batch::BatchPayload),
    FindSimilar(String, String), // (entity_id, label) - open similar tab
    ShowInfo(Box<nokkvi_data::types::info_modal::InfoModalItem>), // Open info modal
    ShowAlbumInFolder(String),   // album_id - fetch a song path and open containing folder
    ShowSongInFolder(String),    // song path - open containing folder directly
    CenterOnPlaying,
    NavigateAndFilter(crate::View, nokkvi_data::types::filter::LibraryFilter),
    NavigateAndExpandArtist(String), // artist_id - navigate to Artists and auto-expand
    /// Persist a column-visibility toggle change. Root forwards to the
    /// settings service so the new value survives across launches.
    ColumnVisibilityChanged(GenresColumn, bool),
    None,
}

super::impl_has_common_action!(GenresAction {
    NavigateAndExpandArtist,
    NavigateAndExpandAlbum
});

impl Default for GenresPage {
    fn default() -> Self {
        Self {
            common: SlotListPageState::new(
                widgets::view_header::SortMode::Name,
                true, // sort_ascending
            ),
            expansion: ExpansionState::default(),
            column_visibility: GenresColumnVisibility::default(),
        }
    }
}

impl GenresPage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Convert sort mode to API string for server requests.
    /// Thin shim — the unified mapping lives in `views/sort_api.rs`.
    pub fn sort_mode_to_api_string(
        sort_mode: crate::widgets::view_header::SortMode,
    ) -> &'static str {
        super::sort_api::sort_mode_to_api_string(crate::View::Genres, sort_mode)
    }
}

// ============================================================================
// ViewPage trait implementation
// ============================================================================

impl super::ViewPage for GenresPage {
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
        Some(Message::Genres(GenresMessage::CollapseExpansion))
    }

    fn search_input_id(&self) -> &'static str {
        super::GENRES_SEARCH_ID
    }

    fn sort_mode_options(&self) -> Option<&'static [SortMode]> {
        Some(super::sort_api::sort_modes_for_view(crate::View::Genres))
    }
    fn sort_mode_selected_message(&self, mode: SortMode) -> Option<Message> {
        Some(Message::Genres(GenresMessage::SortModeSelected(mode)))
    }
    fn toggle_sort_order_message(&self) -> Message {
        Message::Genres(GenresMessage::ToggleSortOrder)
    }

    fn add_to_queue_message(&self) -> Option<Message> {
        Some(Message::Genres(GenresMessage::SlotList(
            crate::widgets::SlotListPageMessage::AddCenterToQueue,
        )))
    }
    fn expand_center_message(&self) -> Option<Message> {
        // ExpandCenter on a 2nd-tier album row routes through `update()`'s
        // pre-check to NavigateAndExpandAlbum (cross-view drill-down);
        // parent rows toggle inline expansion via the macro.
        Some(Message::Genres(GenresMessage::ExpandCenter))
    }
    fn reload_message(&self) -> Option<Message> {
        Some(Message::LoadGenres)
    }

    fn synth_set_offset_message(&self, offset: usize) -> Option<Message> {
        Some(Message::Genres(GenresMessage::SlotList(
            crate::widgets::SlotListPageMessage::SetOffset(
                offset,
                iced::keyboard::Modifiers::default(),
            ),
        )))
    }

    fn slot_list_message(&self, msg: crate::widgets::SlotListPageMessage) -> Message {
        Message::Genres(GenresMessage::SlotList(msg))
    }

    fn uses_horizontal_artwork_column(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genres_column_visibility_default_thumbnail_on() {
        let v = GenresColumnVisibility::default();
        assert!(v.thumbnail);
    }

    #[test]
    fn genres_column_visibility_get_set_round_trip() {
        let mut v = GenresColumnVisibility::default();
        assert!(v.get(GenresColumn::Thumbnail));
        v.set(GenresColumn::Thumbnail, false);
        assert!(!v.get(GenresColumn::Thumbnail));
        v.set(GenresColumn::Thumbnail, true);
        assert!(v.get(GenresColumn::Thumbnail));
    }
}
