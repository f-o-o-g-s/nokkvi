//! Queue Page Component
//!
//! Self-contained queue view with slot list navigation showing currently playing queue.
//! Uses message bubbling pattern to communicate global actions to root.
//!
//! Queue is the outlier in the slot-list family: it uses its own
//! `QueueSortMode` enum (not `SortMode`), doesn't implement `HasCommonAction`,
//! and carries playlist-edit / quick-save / drag-reorder behaviors that
//! aren't shared with the library views.
//!
//! Module layout:
//! - `mod.rs`: types (Page, Message, Action, ViewData, QueueContextEntry,
//!   re-export of QueueSortMode), trait impls, tests for column declarations
//! - `update.rs`: `impl QueuePage { fn update }` + the toggle-column test
//! - `view.rs`: `impl QueuePage { fn view }`, the per-mode column-visibility
//!   helpers (`rating_column_visible` etc.), the `BREAKPOINT_HIDE_QUEUE_STARS`
//!   constant, and helper tests

// Re-export QueueSortMode from data crate (canonical definition)
use nokkvi_data::backend::queue::QueueSongUIViewData;
pub(crate) use nokkvi_data::types::queue_sort_mode::QueueSortMode;

use crate::{
    app_message::Message,
    widgets::{SlotListPageMessage, SlotListPageState, drag_column::DragEvent},
};

mod update;
mod view;

/// Queue page local state
#[derive(Debug)]
pub struct QueuePage {
    pub common: SlotListPageState,
    /// Queue uses its own sort mode enum (QueueSortMode), separate from
    /// the library views' SortMode.
    pub queue_sort_mode: QueueSortMode,
    /// Per-column visibility toggles surfaced via the columns-3-cog dropdown
    /// in the view header. Persisted to config.toml.
    pub column_visibility: QueueColumnVisibility,
    /// Cache of the last `(mode, ascending, queue_len)` that was applied. The
    /// queue sort short-circuits when this matches — covers the common
    /// "user toggles same sort mode again" case and most "queue length
    /// unchanged since last sort" cases. Same-length-different-content
    /// requires the caller to manually re-trigger or invalidate.
    pub last_sort_signature: Option<(QueueSortMode, bool, usize)>,
}

// Toggleable queue columns. `Stars`, `Album`, `Duration`, `Love`, and `Plays`
// are user-toggleable from the columns dropdown; the index/title/artwork
// columns stay always-on.
super::define_view_columns! {
    QueueColumn => QueueColumnVisibility {
        Select: select = false => set_queue_show_select,
        Index: index = true => set_queue_show_index,
        Thumbnail: thumbnail = true => set_queue_show_thumbnail,
        Stars: stars = true => set_queue_show_stars,
        Album: album = true => set_queue_show_album,
        Duration: duration = true => set_queue_show_duration,
        Love: love = true => set_queue_show_love,
        Plays: plays = false => set_queue_show_plays,
        Genre: genre = false => set_queue_show_genre,
    }
}

/// View data passed from root (read-only)
pub struct QueueViewData<'a> {
    pub queue_songs: std::borrow::Cow<'a, [QueueSongUIViewData]>,
    pub album_art: &'a std::collections::HashMap<String, iced::widget::image::Handle>,
    pub large_artwork: &'a std::collections::HashMap<String, iced::widget::image::Handle>,
    pub window_width: f32,
    pub window_height: f32,
    pub scale_factor: f32,
    pub modifiers: iced::keyboard::Modifiers,
    pub current_playing_song_id: Option<String>,
    pub current_playing_queue_index: Option<usize>,
    pub is_playing: bool, // True if playback is active (not stopped/paused)
    pub total_queue_count: usize, // Total count before filtering (for empty state detection)
    pub stable_viewport: bool,
    /// When in edit mode: (playlist_name, is_dirty)
    pub edit_mode_info: Option<(String, bool)>,
    /// Playlist comment when in edit mode
    pub edit_mode_comment: Option<String>,
    /// Playlist public flag when in edit mode (drives the lock toggle button)
    pub edit_mode_public: Option<bool>,
    /// When a playlist is loaded for playback (not editing)
    pub playlist_context_info: Option<crate::state::ActivePlaylistContext>,
    /// Whether the column-visibility checkbox dropdown is open (controlled
    /// by `Nokkvi.open_menu`).
    pub column_dropdown_open: bool,
    /// Trigger bounds captured when the dropdown was opened.
    pub column_dropdown_trigger_bounds: Option<iced::Rectangle>,
    /// Borrowed reference to the root open-menu state, so per-row context
    /// menus can resolve their own open/closed status.
    pub open_menu: Option<&'a crate::app_message::OpenMenu>,
    /// Whether the queue's view-header chip should render. Gated by the
    /// `queue_show_default_playlist` user setting.
    pub show_default_playlist_chip: bool,
    /// Current default-playlist display name (empty when no default set).
    pub default_playlist_name: &'a str,
    /// Visual slot index where the cross-pane-drag drop indicator should
    /// draw — `Some` only when a drag is active and the cursor is over a
    /// queue slot. The queue view renders a 2 px accent line at the top
    /// of this slot inside its slot-list area (no chrome math).
    pub drop_indicator_slot: Option<usize>,
}

/// Context menu entries for queue items
#[derive(Debug, Clone, Copy)]
pub enum QueueContextEntry {
    Play,
    PlayNext,
    Separator,
    RemoveFromQueue,
    AddToPlaylist,
    SaveAsPlaylist,
    OpenBrowsingPanel,
    GetInfo,
    ShowInFolder,
    FindSimilar,
    TopSongs,
}

/// Messages for local queue page interactions
#[derive(Debug, Clone)]
pub enum QueueMessage {
    // Shared slot-list navigation/activation/selection/sort/search
    SlotList(SlotListPageMessage),

    FocusCurrentPlaying(usize, bool), // Auto-scroll slot list to center currently playing track (by queue index, flash)

    // Mouse click on star/heart (item_index, value)
    ClickSetRating(usize, usize), // (item_index, rating 1-5)
    ClickToggleStar(usize),       // item_index

    // Context menu
    ContextMenuAction(usize, QueueContextEntry), // (item_index, entry)

    // Drag-and-drop reordering
    DragReorder(DragEvent),

    // View header interactions
    SortModeSelected(QueueSortMode),
    ToggleColumnVisible(QueueColumn),
    /// Sort dropdown's "Roulette" entry was selected — intercepted at the
    /// root handler before the page's `update` runs.
    Roulette,

    // Playlist edit mode
    SavePlaylist,
    DiscardEdits,
    PlaylistNameChanged(String),
    PlaylistCommentChanged(String),
    PlaylistEditPublicToggled(bool),
    EditPlaylist,      // Enter edit mode for the currently-playing playlist
    QuickSavePlaylist, // Save current queue back to the active playlist without entering edit mode

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
    /// Column-dropdown open/close request — bubbled to root
    /// `Message::SetOpenMenu`. Intercepted in `handle_queue` before the
    /// page's `update` runs.
    SetOpenMenu(Option<crate::app_message::OpenMenu>),
    /// Artwork column drag handle event — intercepted at root, page never sees it.
    ArtworkColumnDrag(crate::widgets::artwork_split_handle::DragEvent),
    /// Always-Vertical artwork drag handle event — intercepted at root.
    ArtworkColumnVerticalDrag(crate::widgets::artwork_split_handle::DragEvent),
    /// Header chip clicked — bubble to root, opens the default-playlist picker.
    OpenDefaultPlaylistPicker,
}

/// Actions that bubble up to root for global state mutation
#[derive(Debug, Clone)]
pub enum QueueAction {
    PlaySong(usize),                // song index in queue
    FocusOnSong(usize, bool),       // queue index to scroll to (bubbles up to handler), flash
    SortModeChanged(QueueSortMode), // trigger reload/resort
    SortOrderChanged(bool),         // trigger resort
    SearchChanged(String),          // trigger filter
    SetRating(String, usize),       // (song_id, rating) - set absolute rating
    ToggleStar(String, bool),       // (song_id, new_starred) - toggle starred state
    MoveItem {
        from: usize,
        to: usize,
    }, // drag-and-drop reorder (absolute item indices)
    MoveBatch {
        indices: Vec<usize>,
        target: usize,
    }, // multi-selection drag reorder
    RemoveFromQueue(Vec<String>),   // remove songs by ID (immune to index drift)
    PlayNext(Vec<String>),          // insert songs after currently playing (by ID)
    ShowToast(String),              // informational toast (e.g. drag disabled reason)
    SaveAsPlaylist,                 // open dialog to save queue as new playlist
    OpenBrowsingPanel,              // toggle the library browser panel
    AddToPlaylist(Vec<String>),     // song_ids - add to playlist dialog
    SavePlaylist,                   // save playlist edits (edit mode)
    DiscardEdits,                   // discard edits and exit edit mode
    PlaylistNameChanged(String),    // playlist name edited inline
    PlaylistCommentChanged(String), // playlist comment edited inline
    PlaylistEditPublicToggled(bool), // public/private toggled in the edit bar
    EditPlaylist,                   // enter edit mode from playlist context bar
    ShowInfo(usize),                // Open info modal (queue index for full Song lookup)
    ShowInFolder(usize),            // Open containing folder (queue index, path fetched via API)
    RefreshArtwork(String),         // album_id - refresh artwork from server
    FindSimilar(usize),             // Open Find Similar panel for queue index
    TopSongs(usize),                // Open Top Songs panel for queue index
    NavigateAndFilter(crate::View, nokkvi_data::types::filter::LibraryFilter), // Navigate to target view and filter
    NavigateAndExpandAlbum(String), // album_id - navigate to Albums and auto-expand
    NavigateAndExpandArtist(String), // artist_id - navigate to Artists and auto-expand
    NavigateAndExpandGenre(String), // genre_id - navigate to Genres and auto-expand
    /// User toggled a queue column's visibility — persist to config.toml.
    ColumnVisibilityChanged(QueueColumn, bool),
    /// Bubble to root: open the default-playlist picker overlay.
    OpenDefaultPlaylistPicker,
    None,
}

impl Default for QueuePage {
    fn default() -> Self {
        Self {
            common: SlotListPageState::new_without_sort_mode(),
            queue_sort_mode: QueueSortMode::Album,
            column_visibility: QueueColumnVisibility::default(),
            last_sort_signature: None,
        }
    }
}

impl QueuePage {
    pub fn new() -> Self {
        Self::default()
    }
}

// ============================================================================
// ViewPage trait implementation
// ============================================================================

impl super::ViewPage for QueuePage {
    fn common(&self) -> &SlotListPageState {
        &self.common
    }
    fn common_mut(&mut self) -> &mut SlotListPageState {
        &mut self.common
    }

    fn search_input_id(&self) -> &'static str {
        super::QUEUE_SEARCH_ID
    }

    // Queue uses QueueSortMode, not SortMode — sort_mode_selected_message returns None (default).
    fn toggle_sort_order_message(&self) -> Message {
        Message::Queue(QueueMessage::SlotList(SlotListPageMessage::ToggleSortOrder))
    }

    // Queue items are already in the queue, so add_to_queue_message returns None (default).
    // Queue has no reload_message (client-side filtering, no server fetch needed on Escape).

    fn slot_list_message(&self, msg: SlotListPageMessage) -> Message {
        Message::Queue(QueueMessage::SlotList(msg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_column_visibility_default_shows_legacy_columns_only() {
        let v = QueueColumnVisibility::default();
        assert!(v.stars);
        assert!(v.album);
        assert!(v.duration);
        assert!(v.love);
        // Plays is opt-in (auto-shown only when sort = MostPlayed).
        assert!(!v.plays);
    }

    #[test]
    fn queue_column_visibility_get_set_round_trip() {
        let mut v = QueueColumnVisibility::default();
        assert!(v.get(QueueColumn::Stars));
        v.set(QueueColumn::Stars, false);
        assert!(!v.get(QueueColumn::Stars));
        // Other columns unchanged.
        assert!(v.get(QueueColumn::Album));
        assert!(v.get(QueueColumn::Duration));
        assert!(v.get(QueueColumn::Love));
        assert!(!v.get(QueueColumn::Plays));

        v.set(QueueColumn::Album, false);
        v.set(QueueColumn::Duration, false);
        v.set(QueueColumn::Love, false);
        v.set(QueueColumn::Plays, true);
        assert!(!v.get(QueueColumn::Album));
        assert!(!v.get(QueueColumn::Duration));
        assert!(!v.get(QueueColumn::Love));
        assert!(v.get(QueueColumn::Plays));
    }

    #[test]
    fn queue_column_visibility_default_keeps_genre_off() {
        let v = QueueColumnVisibility::default();
        assert!(!v.genre);
    }
}
