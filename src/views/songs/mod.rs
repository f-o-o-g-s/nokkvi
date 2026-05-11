//! Songs Page Component
//!
//! Self-contained songs view with slot list navigation, search, and filtering.
//! Uses message bubbling pattern to communicate global actions to root.
//!
//! Songs is the only non-expansion view in the slot-list family — there are
//! no parent/child entries, just a flat song list. The update handler does
//! not use `impl_expansion_update!`.
//!
//! Module layout:
//! - `mod.rs`: types (Page, Message, Action, ViewData), trait impls, tests
//!   for column declarations
//! - `update.rs`: `impl SongsPage { fn update, fn sort_mode_to_api_string }`
//!   + the toggle-column test
//! - `view.rs`: `impl SongsPage { fn view }`, the per-mode column helpers
//!   (`songs_stars_visible`, `songs_plays_visible`, `songs_genre_visible`),
//!   and helper tests

use std::collections::HashMap;

use iced::widget::image;
use nokkvi_data::backend::songs::SongUIViewData;

use crate::{
    app_message::Message,
    widgets::{SlotListPageState, view_header::SortMode},
};

mod update;
mod view;

/// Songs page local state
#[derive(Debug)]
pub struct SongsPage {
    pub common: SlotListPageState,
    /// Per-column visibility toggles surfaced via the columns-cog dropdown.
    pub column_visibility: SongsColumnVisibility,
}

// Toggleable songs columns. Index/Art/Title+Artist are always shown; the
// dynamic 18% slot still auto-renders Date/Year when sorted by those modes;
// Genre lives in the album column slot via the dedicated Genre toggle.
//
// Stars and Plays opt-in (today only show on their sort modes).
// Album/Duration/Love always-on today. Index/Thumbnail default on to match
// historical always-on rendering. Genre opt-in; auto-shows when sort = Genre
// regardless of toggle. Select opt-in — checkbox column for multi-selection.
super::define_view_columns! {
    SongsColumn => SongsColumnVisibility {
        Select: select = false => set_songs_show_select,
        Index: index = true => set_songs_show_index,
        Thumbnail: thumbnail = true => set_songs_show_thumbnail,
        Stars: stars = false => set_songs_show_stars,
        Album: album = true => set_songs_show_album,
        Duration: duration = true => set_songs_show_duration,
        Plays: plays = false => set_songs_show_plays,
        Love: love = true => set_songs_show_love,
        Genre: genre = false => set_songs_show_genre,
    }
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

/// Messages for local song page interactions
#[derive(Debug, Clone)]
pub enum SongsMessage {
    /// Unified slot-list navigation and header actions (Pattern B — no expansion).
    SlotList(crate::widgets::SlotListPageMessage),

    // Mouse click on star/heart (item_index, value)
    ClickSetRating(usize, usize), // (item_index, rating 1-5)
    ClickToggleStar(usize),       // item_index

    // Context menu
    ContextMenuAction(usize, crate::widgets::context_menu::LibraryContextEntry),

    /// Sort dropdown's "Roulette" entry was selected — intercepted at the
    /// root handler before the page's `update` runs.
    Roulette,

    /// Refresh artwork for a specific album (album_id)
    RefreshArtwork(String),
    /// Navigate to a view and apply an ID filter
    NavigateAndFilter(crate::View, nokkvi_data::types::filter::LibraryFilter),
    /// Navigate to Albums and auto-expand the album with this id (no filter set).
    NavigateAndExpandAlbum(String),
    /// Navigate to Artists and auto-expand the artist with this id (no filter set).
    NavigateAndExpandArtist(String),
    /// Navigate to Genres and auto-expand the genre with this id (no filter set).
    NavigateAndExpandGenre(String),
    ToggleColumnVisible(SongsColumn),
    /// Column-dropdown open/close request — bubbled to root
    /// `Message::SetOpenMenu`. Intercepted in `handle_songs` before the
    /// page's `update` runs.
    SetOpenMenu(Option<crate::app_message::OpenMenu>),
    /// Artwork column drag handle event — intercepted at root, page never sees it.
    ArtworkColumnDrag(crate::widgets::artwork_split_handle::DragEvent),
    /// Always-Vertical artwork drag handle event — intercepted at root.
    ArtworkColumnVerticalDrag(crate::widgets::artwork_split_handle::DragEvent),
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
    SortModeChanged(crate::widgets::view_header::SortMode), // trigger reload
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
    NavigateAndExpandAlbum(String), // album_id - navigate to Albums and auto-expand this album
    NavigateAndExpandArtist(String), // artist_id - navigate to Artists and auto-expand this artist
    NavigateAndExpandGenre(String), // genre_id - navigate to Genres and auto-expand this genre
    ColumnVisibilityChanged(SongsColumn, bool),
    None,
}

super::impl_has_common_action!(SongsAction {
    NavigateAndExpandAlbum,
    NavigateAndExpandArtist,
    NavigateAndExpandGenre,
});

impl Default for SongsPage {
    fn default() -> Self {
        Self {
            common: SlotListPageState::new(
                crate::widgets::view_header::SortMode::RecentlyAdded,
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
}

// ============================================================================
// ViewPage trait implementation
// ============================================================================

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
        Some(super::sort_api::sort_modes_for_view(crate::View::Songs))
    }
    fn sort_mode_selected_message(&self, mode: SortMode) -> Option<Message> {
        Some(Message::Songs(SongsMessage::SlotList(
            crate::widgets::SlotListPageMessage::SortModeSelected(mode),
        )))
    }
    fn toggle_sort_order_message(&self) -> Message {
        Message::Songs(SongsMessage::SlotList(
            crate::widgets::SlotListPageMessage::ToggleSortOrder,
        ))
    }

    fn add_to_queue_message(&self) -> Option<Message> {
        Some(Message::Songs(SongsMessage::SlotList(
            crate::widgets::SlotListPageMessage::AddCenterToQueue,
        )))
    }
    fn reload_message(&self) -> Option<Message> {
        Some(Message::LoadSongs)
    }

    fn synth_set_offset_message(&self, offset: usize) -> Option<Message> {
        Some(Message::Songs(SongsMessage::SlotList(
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
    fn songs_column_visibility_default_preserves_today_behavior() {
        let v = SongsColumnVisibility::default();
        assert!(!v.stars);
        assert!(v.album);
        assert!(v.duration);
        assert!(!v.plays);
        assert!(v.love);
    }

    #[test]
    fn songs_column_visibility_default_keeps_genre_off() {
        let v = SongsColumnVisibility::default();
        assert!(!v.genre);
    }
}
