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
    /// Transient: whether the read-only playlist context strip is expanded to
    /// reveal its detail block. Driven by hover
    /// (`PlaylistStripHoverEnter`/`Exit`); reset whenever the active playlist
    /// changes or clears so a stale expansion never carries over. Also reset on
    /// entering and exiting playlist edit mode, because that transition unmounts
    /// the banner's hover `mouse_area` and the `on_exit` collapse can never fire.
    pub playlist_strip_expanded: bool,
}

// Toggleable queue columns, in columns-dropdown order (declaration order ==
// dropdown order via `dropdown_entries`). The title/artist/artwork columns
// stay always-on.
super::define_view_columns! {
    QueueColumn => QueueColumnVisibility {
        Select("Select"): select = false => set_queue_show_select @ queue_show_select,
        Index("Index"): index = true => set_queue_show_index @ queue_show_index,
        Thumbnail("Thumbnail"): thumbnail = true => set_queue_show_thumbnail @ queue_show_thumbnail,
        Stars("Stars"): stars = true => set_queue_show_stars @ queue_show_stars,
        Album("Album"): album = true => set_queue_show_album @ queue_show_album,
        Genre("Genre"): genre = false => set_queue_show_genre @ queue_show_genre,
        Duration("Duration"): duration = true => set_queue_show_duration @ queue_show_duration,
        Love("Love"): love = true => set_queue_show_love @ queue_show_love,
        Plays("Plays"): plays = false => set_queue_show_plays @ queue_show_plays,
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
    /// Per-row `entry_id` of the playing row. Drift-immune handle used
    /// by the header's "Center on Playing" button and by the auto-
    /// follow producers (see `FocusCurrentPlaying`).
    pub current_playing_entry_id: Option<u64>,
    pub is_playing: bool, // True if playback is active (not stopped/paused)
    pub total_queue_count: usize, // Total count before filtering (for empty state detection)
    pub stable_viewport: bool,
    /// Whether artwork-elevation is in effect for this frame. Forwarded into
    /// BaseSlotListLayoutConfig.elevated. Always false in split-view /
    /// side-nav / none-nav.
    pub elevated: bool,
    /// When a playlist is loaded for playback (editing happens in the
    /// decoupled `PlaylistEditor` view, never in the queue).
    pub playlist_context_info: Option<crate::state::ActivePlaylistContext>,
    /// Whether the read-only playlist context strip should render its expanded
    /// detail block this frame (mirrors `QueuePage.playlist_strip_expanded`).
    pub playlist_strip_expanded: bool,
    /// Resolved cover handle for the active playlist's strip thumbnail (collage
    /// first tile, falling back to the mini cover). `None` when no playlist is
    /// active or its artwork isn't cached yet — the strip omits the cover.
    pub playlist_cover: Option<&'a iced::widget::image::Handle>,
    /// Shared overlay-menu plumbing (column-dropdown open/bounds + borrowed
    /// `open_menu` reference). See `super::OverlayMenuViewData`.
    pub overlay: super::OverlayMenuViewData<'a>,
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

    FocusCurrentPlaying(u64, bool), // Auto-scroll slot list to center currently playing track (by per-row entry_id, flash)

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

    // Playlist editing entry points. The editor itself is the decoupled
    // `PlaylistEditor` view; the queue only launches it / quick-saves.
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
    /// Pointer entered the read-only playlist context strip — expand its detail
    /// block (hover mode). Handled locally; no root action.
    PlaylistStripHoverEnter,
    /// Pointer left the playlist context strip — collapse the detail block.
    PlaylistStripHoverExit,
}

/// Actions that bubble up to root for global state mutation
#[derive(Debug, Clone)]
pub enum QueueAction {
    PlaySong(usize),                // song index in queue
    FocusOnSong(u64, bool),         // per-row entry_id to scroll to (bubbles up to handler), flash
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
    /// Remove one or more queue rows by their per-row `entry_id`s.
    /// Duplicate-aware: targets specific rows rather than every row that
    /// shares a song_id.
    RemoveFromQueue(Vec<u64>),
    /// Insert one or more queue rows after the currently-playing position,
    /// referenced by per-row `entry_id` so a single duplicate row can be
    /// promoted without dragging the other duplicate with it.
    PlayNext(Vec<u64>),
    ShowToast(String),          // informational toast (e.g. drag disabled reason)
    SaveAsPlaylist,             // open dialog to save queue as new playlist
    OpenBrowsingPanel,          // toggle the library browser panel
    AddToPlaylist(Vec<String>), // song_ids - add to playlist dialog
    EditPlaylist,               // enter edit mode from playlist context bar
    ShowInfo(usize),            // Open info modal (queue index for full Song lookup)
    ShowInFolder(usize),        // Open containing folder (queue index, path fetched via API)
    RefreshArtwork(String),     // album_id - refresh artwork from server
    FindSimilar(usize),         // Open Find Similar panel for queue index
    TopSongs(usize),            // Open Top Songs panel for queue index
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
            playlist_strip_expanded: false,
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

    fn uses_horizontal_artwork_column(&self) -> bool {
        true
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

    #[test]
    fn queue_column_visibility_restore_from_reads_settings() {
        use nokkvi_data::types::{player_settings::LivePlayerSettings, view_columns::ViewColumns};

        // Alternating true/false by declaration order so every ADJACENT pair of
        // fields differs. `restore_from` must map each `LivePlayerSettings` field
        // to the matching struct field; a copy-pasted `@ settings_field` token
        // (the realistic drift — duplicating a neighbor) would read a value that
        // differs from the expected one and trip the matching assert below.
        let settings = LivePlayerSettings {
            view_columns: ViewColumns {
                queue_show_select: true,
                queue_show_index: false,
                queue_show_thumbnail: true,
                queue_show_stars: false,
                queue_show_album: true,
                queue_show_duration: false,
                queue_show_love: true,
                queue_show_plays: false,
                queue_show_genre: true,
                ..ViewColumns::default()
            },
            ..Default::default()
        };

        let v = QueueColumnVisibility::restore_from(&settings);
        assert_eq!(v.select, settings.view_columns.queue_show_select);
        assert_eq!(v.index, settings.view_columns.queue_show_index);
        assert_eq!(v.thumbnail, settings.view_columns.queue_show_thumbnail);
        assert_eq!(v.stars, settings.view_columns.queue_show_stars);
        assert_eq!(v.album, settings.view_columns.queue_show_album);
        assert_eq!(v.duration, settings.view_columns.queue_show_duration);
        assert_eq!(v.love, settings.view_columns.queue_show_love);
        assert_eq!(v.plays, settings.view_columns.queue_show_plays);
        assert_eq!(v.genre, settings.view_columns.queue_show_genre);
    }
}
