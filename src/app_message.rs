//! Message enum for the application
//!
//! All messages that flow through the Iced update loop are defined here.
//! Messages are organized by domain:
//! - Navigation & Login
//! - Data Loading
//! - Playback Control
//! - ViewHeader (search/sort/filter)
//! - Slot List Navigation
//! - Window Events
//! - Component Bubbling

use iced::widget::image;
use nokkvi_data::backend::app_service::AppService;

use crate::{View, services, views, widgets};

/// Kind of playlist mutation — drives toast messages and reload.
#[derive(Debug, Clone)]
pub enum PlaylistMutation {
    Deleted(String),
    Renamed(String),
    /// (name, playlist_id) — playlist_id used to set queue header when created from queue
    Created(String, Option<String>),
    /// (name, playlist_id) — playlist_id used to set queue header when overwritten from queue
    Overwritten(String, Option<String>),
    /// Songs appended to a playlist. `name` drives the toast; `id` lets the
    /// handler re-resolve the open editor's buffer when the append targeted the
    /// playlist currently being edited (so the new tracks appear).
    Appended {
        name: String,
        id: String,
    },
}

impl std::fmt::Display for PlaylistMutation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Deleted(name) => write!(f, "Deleted '{name}'"),
            Self::Renamed(name) => write!(f, "Renamed to '{name}'"),
            Self::Created(name, _) => write!(f, "Created playlist '{name}'"),
            Self::Overwritten(name, _) => write!(f, "Overwritten playlist '{name}'"),
            Self::Appended { name, .. } => write!(f, "Added songs to '{name}'"),
        }
    }
}

/// Named struct for player settings — canonical definition in data crate
pub(crate) use nokkvi_data::types::player_settings::LivePlayerSettings;
/// Named struct for all view sort preferences — canonical definition in data crate
pub(crate) use nokkvi_data::types::view_preferences::AllViewPreferences;

/// Grouped playback state for cleaner message passing (R1 refactoring)
#[derive(Debug, Clone)]
pub struct PlaybackStateUpdate {
    pub position: u32,
    pub duration: u32,
    pub playing: bool,
    pub paused: bool,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art_url: Option<String>,
    pub random: bool,
    pub repeat: bool,
    pub repeat_queue: bool,
    pub consume: bool,
    pub current_index: Option<usize>,
    /// Snapshot of the playing row's per-row `entry_id`, taken under the
    /// same queue lock as `current_index`. Drift-immune handle that
    /// survives intervening optimistic UI mutations — see
    /// `QueueAction::FocusOnSong`.
    pub current_entry_id: Option<u64>,
    pub song_id: Option<String>,
    pub format_suffix: String,
    pub sample_rate: u32,
    /// Whether the CURRENTLY-PLAYING stream was actually built bit-perfect
    /// (build-time fact from the renderer, not the live setting). Drives the
    /// honest now-playing badge so a mid-track toggle can't claim BIT-PERFECT
    /// for a stream still on the DSP path.
    pub current_stream_bit_perfect: bool,
    /// Bitrate in kbps (e.g., 320 for MP3, 1411 for CD-quality FLAC)
    pub bitrate: u32,
    /// Live ICY-metadata parsed by IcyMetadataReader
    pub live_icy_metadata: Option<String>,
    /// Tagged BPM of the current song, if the file/server reports one.
    /// Optional because not all music has BPM metadata; the boat
    /// physics falls back to the spectral-flux envelope when absent.
    pub bpm: Option<u32>,
}

/// Playback-related messages, namespaced under `Message::Playback(..)`
#[derive(Debug, Clone)]
pub enum PlaybackMessage {
    Tick,
    /// Grouped playback state update (replaces 16 positional params)
    PlaybackStateUpdated(Box<PlaybackStateUpdate>),
    TogglePlay,
    Play,
    Pause,
    Stop,
    NextTrack,
    PrevTrack,
    ToggleRandom,
    RandomToggled(bool),
    ToggleRepeat,
    /// Set repeat mode to a specific value — used by MPRIS `LoopStatus`
    /// (`playerctl`, KDE Plasma media controls, GNOME Shell extensions),
    /// which emits the target mode directly rather than a cycle request.
    /// The on-screen repeat button still uses `ToggleRepeat`.
    SetRepeatMode(nokkvi_data::types::queue::RepeatMode),
    RepeatToggled(bool, bool),
    ToggleConsume,
    ConsumeToggled(bool),
    ToggleSoundEffects,
    SfxVolumeChanged(f32),
    CycleVisualization,
    ToggleCrossfade,
    ToggleBitPerfect,
    /// Result of the off-thread `/proc/asound` device-rate probe for the honest
    /// bit-perfect badge. `generation` is the dispatch id (only the latest is
    /// applied, so an out-of-order stale probe can't clobber a fresher result);
    /// `track_rate` is the rate at dispatch time (dropped if the track changed
    /// under it); `device_rate` is the real ALSA clock, or `None` when unknown.
    BitPerfectDeviceRateProbed {
        generation: u64,
        track_rate: u32,
        device_rate: Option<u32>,
        /// The app holding the sink at a different rate (when resampled + the
        /// PipeWire graph could be read) — the "who" of the blocker diagnostic.
        holder: Option<String>,
    },
    Seek(f32),
    VolumeChanged(f32),
    /// Discrete user-committed volume value — always persists to disk
    /// regardless of the in-flight 500ms `VolumeChanged` throttle. Covers
    /// slider drag-release (the final cursor position once the user lets
    /// go) and individual wheel notches (each notch is an atomic gesture
    /// with no "still scrolling" state). Without bypassing the throttle,
    /// commits that fit inside the 500ms window silently drop on next
    /// launch.
    VolumeCommitted(f32),
    /// Trigger gapless preparation when track is ~80% complete
    PrepareNextForGapless,
    /// Persisted player settings loaded from redb
    PlayerSettingsLoaded(Box<LivePlayerSettings>),
    /// Initialize scrobble state with current song ID from persisted queue
    InitializeScrobbleState(Option<String>),
    /// Live ICY-metadata parsed from internet radio stream (Artist, Title)
    RadioMetadataUpdate(Option<String>, Option<String>),
}

/// Scrobbling-related messages, namespaced under `Message::Scrobble(..)`
#[derive(Debug, Clone)]
pub enum ScrobbleMessage {
    /// timer_id, song_id - debounced "now playing" notification
    NowPlaying(u64, String),
    /// song_id - submit scrobble
    Submit(String),
    /// Submission result — `Ok(song_id)` carries the scrobbled song so the UI
    /// can optimistically bump its local play count to mirror Navidrome.
    SubmissionResult(Result<String, String>),
    /// Now-playing heartbeat result — does not affect server play counts.
    NowPlayingResult(Result<(), String>),
    /// song_id — the same track looped in repeat-one mode.
    /// Triggers scrobble submission for the completed loop and resets state.
    TrackLooped(String),
    /// timer_id, song_id — periodic now-playing heartbeat. Fired ~30s after a
    /// successful now-playing send so the server's ephemeral now-playing entry
    /// does not age out during a long single track. Re-emits a `NowPlaying`
    /// only when still live (matching timer_id, playing, not paused, queue
    /// playback); otherwise it is a no-op.
    NowPlayingRefresh(u64, String),
}

/// Hotkey action messages, namespaced under `Message::Hotkey(..)`
#[derive(Debug, Clone)]
pub enum HotkeyMessage {
    ClearSearch,
    CycleSortMode(bool),
    CenterOnPlaying,
    ToggleStar,
    /// Update starred status locally (song_id, new_starred_status)
    SongStarredStatusUpdated(String, bool),
    /// Update starred status locally (album_id, new_starred_status)
    AlbumStarredStatusUpdated(String, bool),
    /// Update starred status locally for artist (artist_id, new_starred_status)
    ArtistStarredStatusUpdated(String, bool),

    /// Add centered album/song to queue (Shift+A)
    AddToQueue,
    SaveQueueAsPlaylist,
    /// Remove centered item from queue (Ctrl+D) - Queue view only
    RemoveFromQueue,
    /// Clear entire queue (Shift+D) - Queue view only
    ClearQueue,
    FocusSearch,
    /// Increase rating of centered item by 1 (max 5)
    IncreaseRating,
    /// Decrease rating of centered item by 1 (min 0)
    DecreaseRating,
    /// Update song rating locally (song_id, new_rating)
    SongRatingUpdated(String, u32),
    /// Increment a song's local play count by 1 after a successful scrobble.
    SongPlayCountIncremented(String),
    /// Update album rating locally (album_id, new_rating)
    AlbumRatingUpdated(String, u32),
    ArtistRatingUpdated(String, u32),
    /// Expand/collapse center item inline (Shift+Enter)
    ExpandCenter,
    /// Move centered queue track up (Shift+↑)
    MoveTrackUp,
    /// Move centered queue track down (Shift+↓)
    MoveTrackDown,
    /// Open Get Info modal for centered item (Shift+I)
    GetInfo,
    /// Find Similar songs for the currently playing track (Shift+S)
    FindSimilar,
    /// Show Top Songs for the currently playing track's artist (Shift+T)
    FindTopSongs,
    /// Settings edit value up (enable) or down (disable)
    EditValue(bool),
    /// Settings sidebar category motion: true = next, false = previous
    SettingsCategoryMotion(bool),
    /// Refresh data for the current active view (except Queue/Settings)
    RefreshView,
    /// Start a Roulette spin on the current pane's view (Ctrl+R).
    /// Resolves the target view at dispatch time, then forwards to
    /// `RouletteMessage::Start`. Settings is a natural no-op (zero items).
    StartRoulette,
}

/// Discriminant for genre vs playlist collage artwork pipelines
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollageTarget {
    Genre,
    Playlist,
}

/// Outcome of a mini (80px) cover fetch, so the loaded-handlers can tell a
/// DETERMINISTIC "no art for this id" (safe to negative-cache) from a TRANSIENT
/// failure (HTTP 429 throttle / timeout / empty body — re-attempt on the next
/// prefetch revisit, never cached). Distinguishing them is what keeps a single
/// throttle drop from permanently blanking a thumbnail that actually has art.
#[derive(Debug, Clone)]
pub enum MiniArt {
    /// Decoded cover handle.
    Loaded(image::Handle),
    /// Navidrome has no artwork for this id (code-70 / non-image 200 body, per
    /// `nokkvi_data::backend::albums::is_missing_artwork`). Negative-cacheable.
    Missing,
    /// Transient failure — record nothing so the next revisit re-attempts.
    Transient,
}

impl MiniArt {
    /// Classify a raw 80px artwork fetch result: `Ok` → `Loaded`; a deterministic
    /// `NonImageResponse` (code-70) → `Missing`; any other error → `Transient`.
    pub fn from_fetch(result: anyhow::Result<Vec<u8>>) -> Self {
        match result {
            Ok(bytes) => MiniArt::Loaded(image::Handle::from_bytes(bytes)),
            Err(e) if nokkvi_data::backend::albums::is_missing_artwork(&e) => MiniArt::Missing,
            Err(_) => MiniArt::Transient,
        }
    }
}

/// Artwork pipeline messages, namespaced under `Message::Artwork(..)`
///
/// Covers shared album artwork, collage artwork (genre/playlist), and song artwork.
#[derive(Debug, Clone)]
pub enum ArtworkMessage {
    // --- Shared Album Artwork ---
    /// `(album_id, updated_at, MiniArt)`. `updated_at` is the cache-buster the
    /// fetch URL carried; on `MiniArt::Loaded` it is recorded into
    /// `album_art_versions` in lockstep with the handle so a later server cover
    /// change is a version-aware prefetch miss (N17). `MiniArt::Missing` is
    /// negatively cached; `MiniArt::Transient` records nothing.
    Loaded(String, Option<String>, MiniArt),
    LargeLoaded(String, Option<image::Handle>),
    LargeArtistLoaded(String, Option<image::Handle>),
    LoadLarge(String),
    /// Force-refresh a specific album's artwork (evict all cached sizes, re-fetch).
    /// User-initiated: shows "Refreshing artwork…" / "Artwork refreshed" toasts.
    RefreshAlbumArtwork(String),
    /// Same as `RefreshAlbumArtwork` but suppresses progress/success toasts.
    /// Dispatched by SSE-driven invalidation so background updates are quiet.
    RefreshAlbumArtworkSilent(String),
    /// Result of a refresh: (album_id, thumb_handle, large_handle, silent).
    /// `silent = true` suppresses the success toast in the completion handler.
    RefreshComplete(String, Option<image::Handle>, Option<image::Handle>, bool),

    // --- Collage Artwork (Genre / Playlist) ---
    LoadCollage(CollageTarget, String, String, String, Vec<String>),
    /// Mini-only variant for non-centered viewport items: skips the up-to-9
    /// collage-tile fetch since only the centered slot displays a 3×3 panel.
    /// On a typical settle this drops total request volume from ~250 to ~25.
    LoadCollageMini(CollageTarget, String, String, String, Vec<String>),
    StartCollagePrefetch(CollageTarget),
    CollageAlbumIdsLoaded(CollageTarget, Vec<(String, Vec<String>)>),
    CollageMiniLoaded(CollageTarget, String, Option<image::Handle>),
    CollageLoaded(
        CollageTarget,
        String,
        Option<image::Handle>,
        Vec<image::Handle>,
        Vec<String>,
    ),
    CollageBatchReady(CollageTarget, Vec<String>, String, String),

    // --- Song Artwork ---
    /// `(album_id, updated_at, MiniArt)`. See [`ArtworkMessage::Loaded`] — the
    /// `updated_at` is recorded into `album_art_versions` on `MiniArt::Loaded`.
    SongMiniLoaded(String, Option<String>, MiniArt),

    // --- Artwork Pane Drag ---
    /// Resize the artwork column via the split handle. `Change` is per-frame
    /// drag preview; `Commit` fires once on release and persists to TOML.
    /// Dispatched centrally so every view's drag handle routes through one
    /// arm instead of per-view `*Message::ArtworkColumnDrag` variants.
    ColumnDrag(crate::widgets::artwork_split_handle::DragEvent),
    /// Always-Vertical artwork drag — same semantics as `ColumnDrag` but for
    /// the vertical-height split (artwork stacked above the slot list).
    VerticalDrag(crate::widgets::artwork_split_handle::DragEvent),
}

/// All application messages
/// Slot list navigation + view-header messages, routed to current view
#[derive(Debug, Clone)]
pub enum SlotListMessage {
    NavigateUp,
    NavigateDown,
    SetOffset(usize),
    ActivateCenter,
    ToggleSortOrder,
    /// Timer-triggered: scrollbar fade animation complete (view, generation_id guard)
    ScrollbarFadeComplete(View, u64),
    /// Timer-triggered: scrollbar seek settled — load artwork for current viewport.
    /// Fires ~150ms after the last seek event (view, generation_id guard).
    SeekSettled(View, u64),
}

/// Roulette (slot-machine random pick) messages, namespaced under
/// `Message::Roulette(..)`. The animation lives entirely on the UI side —
/// no shell calls — and dispatches a normal play action on settle.
#[derive(Debug, Clone)]
pub enum RouletteMessage {
    /// User selected the "Roulette" entry from a view's sort dropdown.
    /// Snapshots item count and starts the indefinite cruise; target and
    /// decel keyframes are rolled later when `Stop` fires.
    Start(View),
    /// User pressed Enter while the wheel was cruising. Rolls the landing
    /// target, builds the decel walk anchored at the keypress instant, and
    /// transitions the spin into its decel phase. No-op if `decel` is
    /// already armed (during the decel walk itself) — the spin is
    /// committed once Stop has fired.
    Stop,
    /// Animation tick from the per-frame subscription.
    Tick(std::time::Instant),
    /// Escape / view change / explicit cancel — restore the original
    /// viewport offset, clear state, no auto-play.
    Cancel,
}

/// Cross-cutting navigation messages — view switches, navigate-and-filter,
/// and navigate-and-expand chains (both top-pane and browsing-pane variants).
/// Handled by `update/navigation.rs`.
///
/// The `Expand` and `ExpandTimeout` variants reuse `PendingExpand` as the
/// carrier — it already encodes the entity kind (Album / Artist / Genre /
/// Song) plus the `for_browsing_pane: bool` discriminator that picks between
/// the top-pane and browsing-pane variants of each chain. Genre intentionally
/// lacks a counterpart in `ItemKind` (see `data/src/types/item_kind.rs`),
/// which is why `PendingExpand` is the right shared carrier here.
#[derive(Debug, Clone)]
pub enum NavigationMessage {
    /// Switch the top pane to a new view.
    SwitchView(crate::View),
    /// Navigate to a view and populate its active filter. `for_browsing_pane`
    /// routes the change into the browsing panel's internal tab instead of
    /// the top pane (Queue stays put in split-view).
    NavigateAndFilter {
        view: crate::View,
        filter: nokkvi_data::types::filter::LibraryFilter,
        for_browsing_pane: bool,
    },
    /// Navigate to the entity's host view, clear any active search/filter, and
    /// page through the unfiltered list until the target id appears, then
    /// auto-expand it inline. `for_browsing_pane` on the inner `PendingExpand`
    /// chooses between the top-pane and browsing-pane variant of the chain.
    Expand(crate::state::PendingExpand),
    /// Internal: 2s after a find-and-expand chain starts, this checks whether
    /// the carried target is still pending and shows a "Finding {entity}…"
    /// toast if so. No-op when the find resolved within the threshold.
    /// Songs only enter the chain via the CenterOnPlaying (Shift+C) fallback.
    ExpandTimeout(crate::state::PendingExpand),
}

/// Cross-cutting Find/Similar lookup messages — triggered from any view to
/// query similar/top songs.
///
/// Distinct from `Message::Similar(SimilarMessage)`, which is the Similar
/// VIEW's per-view message. `FindMessage` covers the cross-view triggers and
/// the API response that populates the Similar tab.
#[derive(Debug, Clone)]
pub enum FindMessage {
    /// Trigger "Find Similar" from any view — opens browsing panel, fires getSimilarSongs2.
    Similar { id: String, label: String },
    /// Trigger "Top Songs" from artists view — opens browsing panel, fires getTopSongs.
    TopSongs { artist_name: String, label: String },
    /// API response for similar/top songs (generation counter, result, label).
    Loaded(
        u64,
        Result<Vec<nokkvi_data::types::song::Song>, String>,
        String,
    ),
}

/// Cross-cutting split-view shell control messages — playlist edit mode,
/// browsing-panel toggle, pane focus, and save flow. Handled by
/// `update/browsing_panel.rs`.
#[derive(Debug, Clone)]
pub enum SplitViewMessage {
    /// Enter split-view playlist editing mode.
    EnterEditMode {
        playlist_id: String,
        playlist_name: String,
        playlist_comment: String,
        playlist_public: bool,
    },
    /// Exit split-view playlist editing mode.
    ExitEditMode,
    /// Toggle browsing panel alongside queue (Ctrl+E).
    ToggleBrowsingPanel,
    /// Toggle keyboard focus between queue and browser panes.
    SwitchPaneFocus,
    /// Save current queue as the edited playlist's tracks.
    SavePlaylistEdits,
    /// Playlist edits saved successfully. Carries the server's new `updatedAt`
    /// token (empty when unavailable) so the editor's optimistic-concurrency
    /// guard can advance for a subsequent save in the same still-mounted session.
    PlaylistEditsSaved(String),
}

/// Playlist-editor messages, namespaced under `Message::Editor(..)`.
///
/// The editor operates on its OWN in-memory track buffer
/// (`Nokkvi.playlist_editor`), decoupled from the live play queue. These
/// mirror the *edit-mode* subset of [`views::QueueMessage`] (reorder, remove,
/// metadata edits, save) and deliberately omit the playback subset — the
/// editor has no "now playing" concept.
///
/// Phase 1 only defines the enum + a no-op routing stub. Real handling lands
/// in Phase 3+, when the split-view edit variants currently living on
/// `QueueMessage` / `SplitViewMessage` migrate here.
#[derive(Debug, Clone)]
pub enum EditorMessage {
    /// Async resolve result — the playlist's tracks, ready to fill the editor
    /// buffer. The TESTABLE entry point (dispatch with a fabricated payload).
    SongsLoaded(Vec<nokkvi_data::backend::queue::QueueSongUIViewData>),
    /// Async resolve FAILED — marks the editor session `Failed` so save and
    /// track mutations are gated off (the empty buffer is not the real
    /// playlist). The editor stays mounted; the user can reload or discard.
    SongsLoadFailed,
    /// Async result of a cross-pane drag drop into the editor: the resolved
    /// rows for the dragged browser item(s), to splice into the buffer at the
    /// `at` slot index (relative to the editor's current — possibly filtered —
    /// view). Fresh sequential `entry_id`s are assigned on insert so they
    /// never collide with existing buffer rows.
    SongsInserted {
        rows: Vec<nokkvi_data::backend::queue::QueueSongUIViewData>,
        at: usize,
    },
    /// Shared slot-list navigation/activation/selection/search carrier.
    SlotList(widgets::SlotListPageMessage),
    /// Drag-and-drop reorder within the editor buffer.
    DragReorder(widgets::drag_column::DragEvent),
    /// Remove a single row at the given buffer index.
    RemoveAt(usize),
    /// Context-menu action against the row at the given index.
    ContextMenuAction(usize, views::queue::QueueContextEntry),
    /// Edit-bar: playlist name changed (per keystroke).
    NameChanged(String),
    /// Edit-bar: playlist comment changed (per keystroke).
    CommentChanged(String),
    /// Edit-bar: public/private toggled.
    PublicToggled(bool),
    /// Persist the editor buffer back to the playlist.
    Save,
    /// Edit-bar discard / exit control — forwards to the shared
    /// `SplitViewMessage::ExitEditMode` handler.
    ExitEditMode,
    /// Open/close a per-row context menu (editor rows live in the split-view
    /// left pane). Forwards to the root `SetOpenMenu` so the single overlay
    /// stack is preserved.
    SetOpenMenu(Option<OpenMenu>),
}

/// Cross-pane drag state machine messages — drag from browsing panel into
/// queue. Handled by `update/cross_pane_drag.rs`.
#[derive(Debug, Clone)]
pub enum CrossPaneDragMessage {
    /// Mouse pressed in browser pane — record origin for threshold detection.
    Pressed,
    /// Cursor moved while mouse button held — check for drag threshold / update position.
    Moved(iced::Point),
    /// Mouse released — finalize drop or cancel.
    Released,
    /// Cancel active drag (e.g. pressing Escape).
    Cancel,
}

/// Toast notification messages, namespaced under `Message::Toast(..)`
#[derive(Debug, Clone)]
pub enum ToastMessage {
    /// Push a new toast notification
    Push(nokkvi_data::types::toast::Toast),
    /// Push a toast and then dispatch a follow-up message
    PushThen(nokkvi_data::types::toast::Toast, Box<Message>),
    /// Dismiss the current (most recent) toast
    Dismiss,
    /// Dismiss a keyed/sticky toast by key
    DismissKey(String),
}

/// Library filter messages, namespaced under `Message::Library(..)`.
///
/// Drives the nav-bar library selector popover and the active-library
/// filter state on `AppService`. Handler bodies live in
/// [`crate::update::library_filter`].
#[derive(Debug, Clone)]
pub enum LibraryMessage {
    /// Open or close the library selector popover. `trigger_bounds` is
    /// captured at click time so the overlay anchors below the trigger.
    OpenChange {
        open: bool,
        trigger_bounds: Option<iced::Rectangle>,
    },
    /// Toggle a single library in the active set. Empty set = "all libraries".
    Toggle(i32),
    /// Library list fetched from server (Subsonic `getMusicFolders`).
    Loaded(Vec<nokkvi_data::types::library::Library>),
    /// Library list fetch failed; surface as a toast.
    LoadFailed(String),
}

// ============================================================================
// Loader Result Messages — backend data-loading results, namespaced per domain.
// ============================================================================
//
// Each per-view `*Message` enum (in `views/*/mod.rs`) carries view-interaction
// variants only; backend data-load results live in their own `*LoaderMessage`
// so the dispatcher in `update/mod.rs` can route them with a single arm and
// the per-view `update()` doesn't need to carry no-op exhaustiveness arms.
//
// Variant shape per the plan §2.4:
//   - Paged domains: `Loaded { result, total_count, background, anchor_id }`
//                  + `PageLoaded(result, total_count)`
//   - Single-shot domains: `Loaded(result, total_count)` (or `Loaded(result)`
//                          for Queue, which has no separate `total_count`).
//
// Each enum is in its own block with a comment header so Phase 2 implementers
// (one per domain) can edit disjoint regions without conflict.

// --- Albums Loader ---------------------------------------------------------

/// Albums loader results — paged backend responses for the Albums view.
///
/// `Loaded` is the first-page response (or background reload via SSE);
/// `background` and `anchor_id` drive the SSE-refresh re-anchor path.
/// `PageLoaded` is a subsequent paged-scroll response and is appended to the
/// existing `PagedBuffer<AlbumUIViewData>`.
#[derive(Debug, Clone)]
pub enum AlbumsLoaderMessage {
    Loaded {
        result: Result<Vec<nokkvi_data::backend::albums::AlbumUIViewData>, String>,
        total_count: usize,
        background: bool,
        anchor_id: Option<String>,
    },
    PageLoaded(
        Result<Vec<nokkvi_data::backend::albums::AlbumUIViewData>, String>,
        usize,
    ),
}

// --- Artists Loader --------------------------------------------------------

/// Artists loader results — paged backend responses for the Artists view.
/// Mirrors `AlbumsLoaderMessage`; same `background` / `anchor_id` semantics.
#[derive(Debug, Clone)]
pub enum ArtistsLoaderMessage {
    Loaded {
        result: Result<Vec<nokkvi_data::backend::artists::ArtistUIViewData>, String>,
        total_count: usize,
        background: bool,
        anchor_id: Option<String>,
    },
    PageLoaded(
        Result<Vec<nokkvi_data::backend::artists::ArtistUIViewData>, String>,
        usize,
    ),
}

// --- Songs Loader ----------------------------------------------------------

/// Songs loader results — paged backend responses for the Songs view.
/// Mirrors `AlbumsLoaderMessage`; same `background` / `anchor_id` semantics.
#[derive(Debug, Clone)]
pub enum SongsLoaderMessage {
    Loaded {
        result: Result<Vec<nokkvi_data::backend::songs::SongUIViewData>, String>,
        total_count: usize,
        background: bool,
        anchor_id: Option<String>,
    },
    PageLoaded(
        Result<Vec<nokkvi_data::backend::songs::SongUIViewData>, String>,
        usize,
    ),
}

// --- Genres Loader ---------------------------------------------------------

/// Genres loader results — single-shot full-list response (genres are not
/// paged). Tuple shape `(result, total_count)` matches the existing fire site.
#[derive(Debug, Clone)]
pub enum GenresLoaderMessage {
    Loaded(
        Result<Vec<nokkvi_data::backend::genres::GenreUIViewData>, String>,
        usize,
    ),
}

// --- Playlists Loader ------------------------------------------------------

/// Playlists loader results — single-shot full-list response (playlists are
/// not paged). Tuple shape `(result, total_count)` matches the existing
/// fire site.
#[derive(Debug, Clone)]
pub enum PlaylistsLoaderMessage {
    Loaded(
        Result<Vec<nokkvi_data::backend::playlists::PlaylistUIViewData>, String>,
        usize,
    ),
}

// --- Queue Loader ----------------------------------------------------------

/// Queue loader results — single-shot full-queue response. No `total_count`
/// because the queue *is* the entire dataset (not paged); the caller consumes
/// `Vec::len()` directly.
#[derive(Debug, Clone)]
pub enum QueueLoaderMessage {
    Loaded(Result<Vec<nokkvi_data::backend::queue::QueueSongUIViewData>, String>),
}

/// Identifies the single overlay menu that may be open across the app.
///
/// Only one of these can be active at a time; opening any new menu replaces the
/// previous value via `Message::SetOpenMenu`. This is what enforces mutual
/// exclusion between the hamburger menu, the player-bar kebab, view-header
/// checkbox dropdowns, and right-click context menus.
#[derive(Debug, Clone, PartialEq)]
pub enum OpenMenu {
    /// Application hamburger menu. Only one is rendered per layout (top nav vs
    /// player bar), so no disambiguator is needed.
    Hamburger,
    /// Player-bar kebab "modes" menu.
    PlayerModes,
    /// View-header checkbox dropdown (column visibility toggles). The
    /// `trigger_bounds` are captured at click time so the overlay can anchor
    /// below the trigger without re-reading layout each frame.
    CheckboxDropdown {
        view: View,
        trigger_bounds: iced::Rectangle,
    },
    /// Similar's columns dropdown lives in the browsing panel only and lacks a
    /// matching `View` variant, so it gets its own discriminator instead of
    /// shoehorning a synthetic `View::Similar` through the rest of the app.
    CheckboxDropdownSimilar { trigger_bounds: iced::Rectangle },
    /// Right-click context menu, anchored to the screen-space cursor position
    /// captured at click time. `id` disambiguates between widget instances
    /// (slot rows, browsing-panel rows, the now-playing strip), which matters
    /// when split-view shows two slot lists at once.
    Context {
        id: ContextMenuId,
        position: iced::Point,
    },
    /// Library filter popover anchored below the nav-bar trigger. The bounds
    /// are captured at click time so the overlay positioning matches the
    /// other dropdown overlays (column dropdown, similar columns).
    LibrarySelector { trigger_bounds: iced::Rectangle },
}

/// Identifies a specific `context_menu` widget instance for `OpenMenu::Context`.
#[derive(Debug, Clone, PartialEq)]
pub enum ContextMenuId {
    /// Now-playing track info strip (player bar / top bar — only one visible
    /// at a time, so no further disambiguation required).
    Strip,
    /// A row in the main library slot list.
    LibraryRow { view: View, item_index: usize },
    /// A row in the queue view.
    QueueRow(usize),
    /// A row in the playlist-editor view (split-view left pane while editing).
    EditorRow(usize),
    /// A row in the radios view.
    RadioRow(usize),
    /// A row in the Similar/Top Songs results (Similar lives only in the
    /// browsing panel, so a single discriminator is enough).
    SimilarRow(usize),
    /// The single "Refresh Artwork" right-click menu on a view's main artwork
    /// panel (one per view at most).
    ArtworkPanel(View),
}

///
/// Messages are organized by domain:
/// - Navigation & Login
/// - Data Loading
/// - Playback Control
/// - Slot List Navigation (namespaced)
/// - Window Events
/// - Component Bubbling
#[derive(Debug, Clone)]
pub enum Message {
    // --- Navigation (namespaced) ---
    Navigation(NavigationMessage),
    /// Track info strip was clicked — dispatch depends on strip_click_action setting
    StripClicked,
    /// Track info strip right-click context menu action
    StripContextAction(crate::widgets::context_menu::StripContextEntry),
    /// Toggle settings view: open if not in settings, return to Queue if already there
    ToggleSettings,
    /// Set the currently open overlay menu (or `None` to close any open menu).
    /// Sole entry point for menu state changes — guarantees mutual exclusion
    /// between the hamburger menu, player-bar kebab, checkbox dropdowns, and
    /// context menus.
    SetOpenMenu(Option<OpenMenu>),

    // --- Login Result (handled at app level since it transitions screens) ---
    LoginResult(Result<AppService, String>),
    /// Resume session from stored JWT (no password needed)
    ResumeSession,
    /// Response from pinging the server to fetch its native application version
    ServerVersionFetched(Option<String>),
    /// Session was terminated (e.g. 401 Unauthorized) — logout and notify
    SessionExpired,

    // --- Data Loading ---
    /// SSE: Navidrome library scan completed with changes.
    ///
    /// Structured payload enumerating each resource kind the server reports
    /// as changed (album / artist / song / playlist / genre + wildcard flag).
    /// `handle_library_changed` branches on `affects_*` so only the caches
    /// that received SSE notifications reload. Wildcard (full-scan) payloads
    /// reload every kind but still skip per-album artwork eviction to avoid
    /// mass re-downloads.
    LibraryChanged(crate::services::navidrome_sse::LibraryChange),
    LoadAlbums,
    LoadQueue,
    LoadArtists,
    LoadGenres,
    LoadPlaylists,
    /// Load internet radio stations from Subsonic API
    LoadRadioStations,
    /// Playlist was mutated (created/deleted/renamed/overwritten/appended) — toast and reload
    PlaylistMutated(PlaylistMutation),
    /// Playlists fetched on-demand for Save Queue as Playlist dialog
    PlaylistsFetchedForDialog(Vec<(String, String)>), // (id, name) pairs
    /// Playlists fetched for Add to Playlist dialog (playlists, pre-resolved song_ids)
    PlaylistsFetchedForAddToPlaylist(Vec<(String, String)>, Vec<String>),
    LoadSongs,
    /// Fetch one page of songs and append to queue, then chain next page if needed.
    /// Enables per-page UI refresh during progressive queue building.
    ProgressiveQueueAppendPage {
        sort_mode: String,
        sort_order: String,
        search_query: Option<String>,
        offset: usize,
        total_count: usize,
        generation: u64,
    },
    /// All pages of a progressive queue chain have been loaded.
    /// Clears `queue_loading_target` so the header shows the actual count.
    ProgressiveQueueDone,

    // --- Loader Results (per-domain *LoaderMessage) ---
    // Backend data-load responses are routed via `dispatch_<domain>_loader`
    // helpers in `update/<domain>.rs`. The corresponding *trigger* messages
    // (`LoadAlbums`, `LoadGenres`, …) live in the Data Loading block above.
    AlbumsLoader(AlbumsLoaderMessage),
    ArtistsLoader(ArtistsLoaderMessage),
    SongsLoader(SongsLoaderMessage),
    GenresLoader(GenresLoaderMessage),
    PlaylistsLoader(PlaylistsLoaderMessage),
    QueueLoader(QueueLoaderMessage),

    // --- Artwork Pipeline (namespaced) ---
    Artwork(ArtworkMessage),

    // --- Playback Control (namespaced) ---
    Playback(PlaybackMessage),
    // --- Scrobbling (namespaced) ---
    Scrobble(ScrobbleMessage),
    // --- Hotkey Actions (namespaced) ---
    Hotkey(HotkeyMessage),
    /// Hotkey config updated after async persistence (hot-reload)
    HotkeyConfigUpdated(nokkvi_data::types::hotkey_config::HotkeyConfig),
    // --- Library Filter (namespaced) ---
    Library(LibraryMessage),
    NoOp,
    /// Quit application (from hamburger menu or tray)
    QuitApp,
    /// Async shutdown sequence completed — proceed to iced::exit().
    ///
    /// Dispatched by the `WindowCloseRequested` handler after the bounded
    /// `request_shutdown` future resolves (or times out). The message exists
    /// so the shutdown async work can be wrapped in `Task::perform` and have
    /// a typed completion callback, keeping `iced::exit()` in the normal
    /// message-dispatch path.
    ShutdownComplete,
    /// Toggle light/dark mode (from hamburger menu)
    ToggleLightMode,
    /// View sort preferences loaded from app.redb
    ViewPreferencesLoaded(AllViewPreferences),

    // --- Slot List Navigation (namespaced, routed to current page) ---
    SlotList(SlotListMessage),

    // --- Window Events ---
    WindowResized(f32, f32),
    ScaleFactorChanged(f32),

    /// Play sound effect
    PlaySfx(nokkvi_data::audio::SfxType),

    // --- Component Message Bubbling ---
    Login(views::LoginMessage),
    PlayerBar(widgets::PlayerBarMessage),
    NavBar(widgets::NavBarMessage),
    Albums(views::AlbumsMessage),
    Artists(views::ArtistsMessage),
    Queue(views::QueueMessage),
    Songs(views::SongsMessage),
    Genres(views::GenresMessage),
    Playlists(views::PlaylistsMessage),
    Settings(views::SettingsMessage),
    /// Similar Songs page messages
    Similar(views::SimilarMessage),
    /// Internet Radio page messages
    Radios(views::RadiosMessage),

    // --- MPRIS D-Bus Integration ---
    Mpris(services::mpris::MprisEvent),

    // --- Rating-reminder desktop notifications ---
    Notification(services::notifications::NotificationEvent),

    // --- System Tray (StatusNotifierItem) ---
    Tray(services::tray::TrayEvent),
    /// First (or only) window opened — capture its id so the tray handler can
    /// later issue `window::set_mode` against it.
    WindowOpened(iced::window::Id),
    /// Window close button (X) was pressed. Branches on close_to_tray + tray
    /// availability to either hide the window or quit the app.
    WindowCloseRequested(iced::window::Id),

    // --- Visualizer Hot-Reload ---
    /// Config file changed, apply new visualizer settings
    VisualizerConfigChanged(crate::visualizer_config::VisualizerConfig),

    // --- Surfing-Boat Overlay (lines mode) ---
    /// Per-frame tick from `iced::window::frames()` driving the boat overlay's
    /// eased horizontal motion + waveform-height sampling. Cheap when not in
    /// lines mode — handler bails after the visibility check.
    BoatTick(std::time::Instant),

    // --- Settings Hot-Reload ---
    SettingsConfigReloaded,
    SettingsReloadDataLoaded(
        nokkvi_data::types::view_preferences::AllViewPreferences,
        nokkvi_data::types::hotkey_config::HotkeyConfig,
        Box<nokkvi_data::types::player_settings::LivePlayerSettings>,
    ),

    // --- Theme Hot-Reload ---
    /// Config file changed, reload theme colors
    ThemeConfigReloaded,

    // --- Raw Keyboard Event ---
    // --- Raw Keyboard Event ---
    /// Raw key press forwarded from subscription; dispatched via HotkeyConfig in update()
    /// The `Status` field indicates whether a widget (e.g. text_input) has already
    /// captured this event. Used to suppress hotkeys while typing in search fields.
    RawKeyEvent(
        iced::keyboard::Key,
        iced::keyboard::Modifiers,
        iced::event::Status,
    ),
    /// Tracks global keyboard modifiers (shift/ctrl/alt/logo) for mouse interaction (e.g. shift+click)
    ModifiersChanged(iced::keyboard::Modifiers),

    // --- Toast Notifications ---
    Toast(ToastMessage),

    // --- Roulette (slot-machine random pick) ---
    Roulette(RouletteMessage),

    // --- Task Manager Notifications ---
    TaskStatusChanged(
        nokkvi_data::services::task_manager::TaskHandle,
        nokkvi_data::services::task_manager::TaskStatus,
    ),

    // --- Text Input Dialog ---
    TextInputDialog(crate::widgets::text_input_dialog::TextInputDialogMessage),

    // -------------------------------------------------------------------------
    // Modals
    // -------------------------------------------------------------------------
    InfoModal(crate::widgets::info_modal::InfoModalMessage),
    AboutModal(crate::widgets::about_modal::AboutModalMessage),
    EqModal(crate::widgets::EqModalMessage),
    /// Default-playlist picker (modal overlay opened from the header chip)
    DefaultPlaylistPicker(crate::widgets::default_playlist_picker::DefaultPlaylistPickerMessage),

    // --- Playlist Edit Mode (split-view) ---
    BrowsingPanel(views::BrowsingPanelMessage),
    SplitView(SplitViewMessage),
    /// Playlist editor (owns its own buffer; decoupled from the live queue).
    /// Phase 1 routes to a no-op stub; real handling lands in Phase 3+.
    Editor(EditorMessage),

    // --- Cross-Pane Drag (browsing panel → queue) ---
    CrossPaneDrag(CrossPaneDragMessage),

    /// Open a song's containing folder in the file manager (relative path from Navidrome)
    ShowInFolder(String),

    // --- Similar Songs (cross-cutting find/load — distinct from Message::Similar per-view) ---
    Find(FindMessage),

    // --- IPC (nokkvi-ipc workspace crate; see services::ipc + update::ipc) ---
    /// A request arrived over the Unix-socket IPC channel. The wrapper
    /// carries the parsed `IpcRequest` plus a cloneable
    /// [`services::ipc::IpcResponder`] handle; the dispatcher builds an
    /// `IpcResponse` and calls `incoming.responder.send(resp)`.
    Ipc(Box<services::ipc::IpcIncoming>),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `FindMessage` sub-enum routing — the three flat root variants
    /// (`FindSimilar` / `FindTopSongs` / `SimilarSongsLoaded`) collapsed
    /// onto `Message::Find(FindMessage)`. This pins the carrier shape and
    /// the variant payload for each of the three.
    #[test]
    fn find_message_sub_enum_routing() {
        // Similar trigger
        let msg = Message::Find(FindMessage::Similar {
            id: "song-42".into(),
            label: "Similar to: Lorem".into(),
        });
        match msg {
            Message::Find(FindMessage::Similar { id, label }) => {
                assert_eq!(id, "song-42");
                assert_eq!(label, "Similar to: Lorem");
            }
            _ => panic!("expected Message::Find(FindMessage::Similar)"),
        }

        // TopSongs trigger
        let msg = Message::Find(FindMessage::TopSongs {
            artist_name: "Artist X".into(),
            label: "Top Songs: Artist X".into(),
        });
        match msg {
            Message::Find(FindMessage::TopSongs { artist_name, label }) => {
                assert_eq!(artist_name, "Artist X");
                assert_eq!(label, "Top Songs: Artist X");
            }
            _ => panic!("expected Message::Find(FindMessage::TopSongs)"),
        }

        // Loaded API response (success path)
        let msg = Message::Find(FindMessage::Loaded(7, Ok(Vec::new()), "label".into()));
        match msg {
            Message::Find(FindMessage::Loaded(generation, result, label)) => {
                assert_eq!(generation, 7);
                assert!(result.is_ok());
                assert_eq!(label, "label");
            }
            _ => panic!("expected Message::Find(FindMessage::Loaded)"),
        }
    }

    /// `FindMessage::Loaded` carries the error string through the sub-enum
    /// boundary unmodified — the renamed shape preserves the previous
    /// `(generation, Result<_, String>, label)` payload of the old flat
    /// `Message::SimilarSongsLoaded` variant.
    #[test]
    fn find_message_loaded_carries_error_string() {
        let msg = Message::Find(FindMessage::Loaded(
            42,
            Err("boom".into()),
            "test-label".into(),
        ));
        match msg {
            Message::Find(FindMessage::Loaded(generation, Err(err), label)) => {
                assert_eq!(generation, 42);
                assert_eq!(err, "boom");
                assert_eq!(label, "test-label");
            }
            _ => panic!("expected Message::Find(FindMessage::Loaded(_, Err(_), _))"),
        }
    }

    /// `SplitViewMessage` sub-enum routing — the six flat root variants
    /// (`EnterPlaylistEditMode` / `ExitPlaylistEditMode` / `ToggleBrowsingPanel`
    /// / `SwitchPaneFocus` / `SavePlaylistEdits` / `PlaylistEditsSaved`)
    /// collapsed onto `Message::SplitView(SplitViewMessage)`. This pins the
    /// carrier shape and the variant payload for each of the six.
    #[test]
    fn split_view_message_sub_enum_routing() {
        // EnterEditMode carries all four struct fields through the boundary.
        let msg = Message::SplitView(SplitViewMessage::EnterEditMode {
            playlist_id: "p1".into(),
            playlist_name: "Mix".into(),
            playlist_comment: "Notes".into(),
            playlist_public: true,
        });
        match msg {
            Message::SplitView(SplitViewMessage::EnterEditMode {
                playlist_id,
                playlist_name,
                playlist_comment,
                playlist_public,
            }) => {
                assert_eq!(playlist_id, "p1");
                assert_eq!(playlist_name, "Mix");
                assert_eq!(playlist_comment, "Notes");
                assert!(playlist_public);
            }
            _ => panic!("expected Message::SplitView(SplitViewMessage::EnterEditMode)"),
        }

        // ExitEditMode
        let msg = Message::SplitView(SplitViewMessage::ExitEditMode);
        assert!(matches!(
            msg,
            Message::SplitView(SplitViewMessage::ExitEditMode)
        ));

        // ToggleBrowsingPanel
        let msg = Message::SplitView(SplitViewMessage::ToggleBrowsingPanel);
        assert!(matches!(
            msg,
            Message::SplitView(SplitViewMessage::ToggleBrowsingPanel)
        ));

        // SwitchPaneFocus
        let msg = Message::SplitView(SplitViewMessage::SwitchPaneFocus);
        assert!(matches!(
            msg,
            Message::SplitView(SplitViewMessage::SwitchPaneFocus)
        ));

        // SavePlaylistEdits
        let msg = Message::SplitView(SplitViewMessage::SavePlaylistEdits);
        assert!(matches!(
            msg,
            Message::SplitView(SplitViewMessage::SavePlaylistEdits)
        ));

        // PlaylistEditsSaved carries the new server updatedAt token.
        let msg = Message::SplitView(SplitViewMessage::PlaylistEditsSaved("T1".into()));
        assert!(matches!(
            msg,
            Message::SplitView(SplitViewMessage::PlaylistEditsSaved(_))
        ));
    }

    /// `CrossPaneDragMessage` sub-enum routing — the four flat root variants
    /// (`CrossPaneDragPressed` / `Moved` / `Released` / `Cancel`) collapsed
    /// onto `Message::CrossPaneDrag(CrossPaneDragMessage)`. The `Moved`
    /// variant uses a non-default `Point` so the test pins that both
    /// coordinates survive the boundary.
    #[test]
    fn cross_pane_drag_message_sub_enum_routing() {
        // Pressed
        let msg = Message::CrossPaneDrag(CrossPaneDragMessage::Pressed);
        assert!(matches!(
            msg,
            Message::CrossPaneDrag(CrossPaneDragMessage::Pressed)
        ));

        // Moved carries the iced::Point payload.
        let msg =
            Message::CrossPaneDrag(CrossPaneDragMessage::Moved(iced::Point::new(123.0, 456.0)));
        match msg {
            Message::CrossPaneDrag(CrossPaneDragMessage::Moved(point)) => {
                assert!((point.x - 123.0).abs() < f32::EPSILON);
                assert!((point.y - 456.0).abs() < f32::EPSILON);
            }
            _ => panic!("expected Message::CrossPaneDrag(CrossPaneDragMessage::Moved)"),
        }

        // Released
        let msg = Message::CrossPaneDrag(CrossPaneDragMessage::Released);
        assert!(matches!(
            msg,
            Message::CrossPaneDrag(CrossPaneDragMessage::Released)
        ));

        // Cancel
        let msg = Message::CrossPaneDrag(CrossPaneDragMessage::Cancel);
        assert!(matches!(
            msg,
            Message::CrossPaneDrag(CrossPaneDragMessage::Cancel)
        ));
    }

    /// `NavigationMessage` sub-enum routing — the ten flat root variants
    /// (`SwitchView` / `NavigateAndFilter` / `BrowserPaneNavigateAndFilter` /
    /// `NavigateAndExpand{Album,Artist,Genre}` /
    /// `BrowserPaneNavigateAndExpand{Album,Artist,Genre}` /
    /// `PendingExpandTimeout`) collapsed onto
    /// `Message::Navigation(NavigationMessage)`. The `Expand` and
    /// `ExpandTimeout` variants reuse `PendingExpand` as the carrier —
    /// `for_browsing_pane: bool` on the inner enum discriminates the
    /// top-pane vs browsing-pane variant.
    #[test]
    fn navigation_message_sub_enum_routing() {
        use nokkvi_data::types::filter::LibraryFilter;

        use crate::state::PendingExpand;

        // SwitchView carries the View payload.
        let msg = Message::Navigation(NavigationMessage::SwitchView(View::Albums));
        assert!(matches!(
            msg,
            Message::Navigation(NavigationMessage::SwitchView(View::Albums))
        ));

        // NavigateAndFilter (top pane).
        let msg = Message::Navigation(NavigationMessage::NavigateAndFilter {
            view: View::Albums,
            filter: LibraryFilter::GenreId {
                id: "Rock".into(),
                name: "Rock".into(),
            },
            for_browsing_pane: false,
        });
        match msg {
            Message::Navigation(NavigationMessage::NavigateAndFilter {
                view,
                filter,
                for_browsing_pane,
            }) => {
                assert_eq!(view, View::Albums);
                assert!(
                    matches!(filter, LibraryFilter::GenreId { ref name, .. } if name == "Rock")
                );
                assert!(!for_browsing_pane);
            }
            _ => panic!("expected Message::Navigation(NavigationMessage::NavigateAndFilter)"),
        }

        // NavigateAndFilter (browsing pane) pins the `for_browsing_pane: true`
        // discriminator that consolidates the old
        // `BrowserPaneNavigateAndFilter` variant.
        let msg = Message::Navigation(NavigationMessage::NavigateAndFilter {
            view: View::Songs,
            filter: LibraryFilter::ArtistId {
                id: "ar-1".into(),
                name: "Artist".into(),
            },
            for_browsing_pane: true,
        });
        match msg {
            Message::Navigation(NavigationMessage::NavigateAndFilter {
                for_browsing_pane: true,
                ..
            }) => {}
            _ => panic!(
                "expected Message::Navigation(NavigationMessage::NavigateAndFilter \
                 with for_browsing_pane: true)"
            ),
        }

        // Expand(Album, browsing-pane) — the browsing-pane discriminator now
        // rides on `PendingExpand::Album.for_browsing_pane` rather than a
        // separate root `BrowserPaneNavigateAndExpandAlbum` variant.
        let msg = Message::Navigation(NavigationMessage::Expand(PendingExpand::Album {
            album_id: "alb-9".into(),
            for_browsing_pane: true,
        }));
        match msg {
            Message::Navigation(NavigationMessage::Expand(PendingExpand::Album {
                album_id,
                for_browsing_pane,
            })) => {
                assert_eq!(album_id, "alb-9");
                assert!(for_browsing_pane);
            }
            _ => panic!(
                "expected Message::Navigation(NavigationMessage::Expand(PendingExpand::Album))"
            ),
        }

        // Expand(Artist, top-pane).
        let msg = Message::Navigation(NavigationMessage::Expand(PendingExpand::Artist {
            artist_id: "ar-1".into(),
            for_browsing_pane: false,
        }));
        assert!(matches!(
            msg,
            Message::Navigation(NavigationMessage::Expand(PendingExpand::Artist {
                for_browsing_pane: false,
                ..
            }))
        ));

        // Expand(Genre, browsing-pane) — the carrier doubles for the genre
        // path even though `ItemKind` (in `data/src/types/item_kind.rs`)
        // intentionally lacks a Genre variant. Reusing `PendingExpand` is
        // why the namespace doesn't need a parallel `ItemKind`-shaped enum.
        let msg = Message::Navigation(NavigationMessage::Expand(PendingExpand::Genre {
            genre_id: "Rock".into(),
            for_browsing_pane: true,
        }));
        match msg {
            Message::Navigation(NavigationMessage::Expand(PendingExpand::Genre {
                genre_id,
                for_browsing_pane,
            })) => {
                assert_eq!(genre_id, "Rock");
                assert!(for_browsing_pane);
            }
            _ => panic!(
                "expected Message::Navigation(NavigationMessage::Expand(PendingExpand::Genre))"
            ),
        }
    }

    /// `NavigationMessage::ExpandTimeout` is the renamed
    /// `Message::PendingExpandTimeout(PendingExpand)` and routes through the
    /// new namespaced carrier. This pins that all four `PendingExpand`
    /// variants (Album / Artist / Genre / Song) survive the boundary —
    /// Song lives here because `handle_pending_expand_timeout` already
    /// encounters it via the CenterOnPlaying (Shift+C) fallback chain.
    #[test]
    fn navigation_message_expand_timeout_carries_pending_expand() {
        use crate::state::PendingExpand;

        // Album variant.
        let msg = Message::Navigation(NavigationMessage::ExpandTimeout(PendingExpand::Album {
            album_id: "alb-7".into(),
            for_browsing_pane: false,
        }));
        match msg {
            Message::Navigation(NavigationMessage::ExpandTimeout(PendingExpand::Album {
                album_id,
                for_browsing_pane,
            })) => {
                assert_eq!(album_id, "alb-7");
                assert!(!for_browsing_pane);
            }
            _ => panic!(
                "expected Message::Navigation(NavigationMessage::ExpandTimeout(\
                 PendingExpand::Album))"
            ),
        }

        // Song variant — only enters the chain via Shift+C, but the
        // sub-enum carries it through the boundary unchanged.
        let msg = Message::Navigation(NavigationMessage::ExpandTimeout(PendingExpand::Song {
            song_id: "s-3".into(),
            for_browsing_pane: true,
        }));
        match msg {
            Message::Navigation(NavigationMessage::ExpandTimeout(PendingExpand::Song {
                song_id,
                for_browsing_pane,
            })) => {
                assert_eq!(song_id, "s-3");
                assert!(for_browsing_pane);
            }
            _ => panic!(
                "expected Message::Navigation(NavigationMessage::ExpandTimeout(\
                 PendingExpand::Song))"
            ),
        }
    }
}
