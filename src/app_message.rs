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
    Appended(String),
}

impl std::fmt::Display for PlaylistMutation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Deleted(name) => write!(f, "Deleted '{name}'"),
            Self::Renamed(name) => write!(f, "Renamed to '{name}'"),
            Self::Created(name, _) => write!(f, "Created playlist '{name}'"),
            Self::Overwritten(name, _) => write!(f, "Overwritten playlist '{name}'"),
            Self::Appended(name) => write!(f, "Added songs to '{name}'"),
        }
    }
}

/// Named struct for player settings — canonical definition in data crate
pub(crate) use nokkvi_data::types::player_settings::PlayerSettings;
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
    pub song_id: Option<String>,
    pub format_suffix: String,
    pub sample_rate: u32,
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
    RepeatToggled(bool, bool),
    ToggleConsume,
    ConsumeToggled(bool),
    ToggleSoundEffects,
    SfxVolumeChanged(f32),
    CycleVisualization,
    ToggleCrossfade,
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
    PlayerSettingsLoaded(Box<PlayerSettings>),
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

/// Artwork pipeline messages, namespaced under `Message::Artwork(..)`
///
/// Covers shared album artwork, collage artwork (genre/playlist), and song artwork.
#[derive(Debug, Clone)]
pub enum ArtworkMessage {
    // --- Shared Album Artwork ---
    Loaded(String, Option<image::Handle>),
    LargeLoaded(String, Option<image::Handle>),
    LargeArtistLoaded(String, Option<image::Handle>, Option<iced::Color>),
    LoadLarge(String),
    DominantColorCalculated(String, iced::Color),
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
    SongMiniLoaded(String, Option<image::Handle>),

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
    /// Snapshots item count, picks a target, and arms the spin subscription.
    Start(View),
    /// Animation tick from the per-frame subscription.
    Tick(std::time::Instant),
    /// Escape / view change / explicit cancel — restore the original
    /// viewport offset, clear state, no auto-play.
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
}

/// Identifies a specific `context_menu` widget instance for `OpenMenu::Context`.
#[derive(Debug, Clone, PartialEq)]
pub enum ContextMenuId {
    /// Now-playing track info strip (player bar / top bar — only one visible
    /// at a time, so no further disambiguation required).
    Strip,
    /// A row in the main library slot list.
    LibraryRow { view: View, item_index: usize },
    /// A row in the browsing-panel slot list (split-view).
    BrowsingRow { view: View, item_index: usize },
    /// A row in the queue view.
    QueueRow(usize),
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
    // --- Navigation ---
    SwitchView(View),
    /// Navigate to a view and populate its active filter
    NavigateAndFilter(View, nokkvi_data::types::filter::LibraryFilter),
    /// Navigate and filter exclusively targeting the browsing panel's internal tabs
    BrowserPaneNavigateAndFilter(View, nokkvi_data::types::filter::LibraryFilter),
    /// Navigate to the Albums view, clear any active search/filter, and page
    /// through the unfiltered list until the target id appears, then auto-
    /// expand it inline. Dispatched by album-text clicks in Songs/Queue.
    NavigateAndExpandAlbum {
        album_id: String,
    },
    /// Browsing-panel variant of `NavigateAndExpandAlbum` — switches the
    /// browsing panel's tab to Albums and runs the same find chain there,
    /// leaving the top pane (Queue) untouched in split-view.
    BrowserPaneNavigateAndExpandAlbum {
        album_id: String,
    },
    /// Internal: 2s after `NavigateAndExpandAlbum` fires, this checks whether
    /// the target is still pending and shows a "Finding album…" toast if so.
    /// No-op when the find resolved within the threshold.
    PendingExpandAlbumTimeout(String),
    /// Artist-side mirror of `NavigateAndExpandAlbum`. Routes to the Artists
    /// view, clears search/filter, pages through until the target id
    /// appears, then auto-expands so the artist's albums show inline.
    NavigateAndExpandArtist {
        artist_id: String,
    },
    /// Browsing-pane variant — switches the browsing panel's tab to Artists
    /// and runs the find chain there, leaving Queue intact in split-view.
    BrowserPaneNavigateAndExpandArtist {
        artist_id: String,
    },
    /// Internal 2s "Finding artist…" toast trigger.
    PendingExpandArtistTimeout(String),
    /// Genre-side mirror — navigate to Genres, find the genre, expand it.
    NavigateAndExpandGenre {
        genre_id: String,
    },
    /// Browsing-pane variant of `NavigateAndExpandGenre`.
    BrowserPaneNavigateAndExpandGenre {
        genre_id: String,
    },
    /// Internal 2s "Finding genre…" toast trigger.
    PendingExpandGenreTimeout(String),
    /// Internal 2s "Finding song…" toast trigger. Songs only enter the
    /// find-and-expand chain via the CenterOnPlaying (Shift+C) fallback —
    /// there is no click-driven navigate-and-expand for songs.
    PendingExpandSongTimeout(String),
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
        Box<nokkvi_data::types::player_settings::PlayerSettings>,
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
    /// Enter split-view playlist editing mode
    EnterPlaylistEditMode {
        playlist_id: String,
        playlist_name: String,
        playlist_comment: String,
        playlist_public: bool,
    },
    /// Exit split-view playlist editing mode
    ExitPlaylistEditMode,
    /// Toggle browsing panel alongside queue (Ctrl+E)
    ToggleBrowsingPanel,
    /// Toggle keyboard focus between queue and browser panes
    SwitchPaneFocus,
    /// Save current queue as the edited playlist's tracks
    SavePlaylistEdits,
    /// Playlist edits saved successfully
    PlaylistEditsSaved,

    // --- Cross-Pane Drag (browsing panel → queue) ---
    /// Mouse pressed in browser pane — record origin for threshold detection
    CrossPaneDragPressed,
    /// Cursor moved while mouse button held — check for drag threshold / update position
    CrossPaneDragMoved(iced::Point),
    /// Mouse released — finalize drop or cancel
    CrossPaneDragReleased,
    /// Cancel active drag (e.g. pressing Escape)
    CrossPaneDragCancel,

    /// Open a song's containing folder in the file manager (relative path from Navidrome)
    ShowInFolder(String),

    // --- Similar Songs ---
    /// Trigger "Find Similar" from any view — opens browsing panel, fires getSimilarSongs2
    FindSimilar {
        id: String,
        label: String,
    },
    /// Trigger "Top Songs" from artists view — opens browsing panel, fires getTopSongs
    FindTopSongs {
        artist_name: String,
        label: String,
    },
    /// API response for similar/top songs (generation counter, result)
    SimilarSongsLoaded(
        u64,
        Result<Vec<nokkvi_data::types::song::Song>, String>,
        String,
    ),
}
