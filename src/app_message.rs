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
    Created(String),
    Overwritten(String),
    Appended(String),
}

impl std::fmt::Display for PlaylistMutation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Deleted(name) => write!(f, "Deleted '{name}'"),
            Self::Renamed(name) => write!(f, "Renamed to '{name}'"),
            Self::Created(name) => write!(f, "Created playlist '{name}'"),
            Self::Overwritten(name) => write!(f, "Overwritten playlist '{name}'"),
            Self::Appended(name) => write!(f, "Added songs to '{name}'"),
        }
    }
}

/// Named entry for batch collage artwork results
#[derive(Debug, Clone)]
pub struct ArtworkBatchEntry {
    pub id: String,
    pub mini_artwork: Option<image::Handle>,
    pub collage_handles: Vec<image::Handle>,
    pub album_ids: Vec<String>,
}

/// Batch of collage artwork results
pub(crate) type ArtworkBatchData = Vec<ArtworkBatchEntry>;

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
    Seek(f32),
    VolumeChanged(f32),
    /// Timer-triggered message with ID to hide volume %
    HideVolumePercentage(u64),
    /// Timer-triggered message with ID to hide SFX volume %
    HideSfxVolumePercentage(u64),
    /// Trigger gapless preparation when track is ~80% complete
    PrepareNextForGapless,
    /// Persisted player settings loaded from redb
    PlayerSettingsLoaded(PlayerSettings),
    /// Initialize scrobble state with current song ID from persisted queue
    InitializeScrobbleState(Option<String>),
}

/// Scrobbling-related messages, namespaced under `Message::Scrobble(..)`
#[derive(Debug, Clone)]
pub enum ScrobbleMessage {
    /// timer_id, song_id - debounced "now playing" notification
    NowPlaying(u64, String),
    /// song_id - submit scrobble
    Submit(String),
    /// API result
    Result(Result<(), String>),
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
    /// Shuffle the queue order
    ShuffleQueue,
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
    /// Settings edit value up (enable) or down (disable)
    EditValue(bool),
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
    LoadLarge(String),
    StartPrefetch,
    StartArtistPrefetch,
    /// Force-refresh a specific album's artwork (evict all cached sizes, re-fetch)
    RefreshAlbumArtwork(String),
    /// Result of a refresh: (album_id, thumb_handle, large_handle)
    RefreshComplete(String, Option<image::Handle>, Option<image::Handle>),

    // --- Collage Artwork (Genre / Playlist) ---
    LoadCollage(CollageTarget, String, String, String, Vec<String>),
    StartCollagePrefetch(CollageTarget),
    CollageAlbumIdsLoaded(CollageTarget, Vec<(String, Vec<String>)>),
    LoadCollageFromIds(CollageTarget),
    CollageMiniLoaded(CollageTarget, String, Option<image::Handle>),
    CollageLoaded(
        CollageTarget,
        String,
        Option<image::Handle>,
        Vec<image::Handle>,
        Vec<String>,
    ),
    CollageBatchLoaded(CollageTarget, ArtworkBatchData),
    CollageBatchReady(CollageTarget, Vec<String>, String, String),

    // --- Song Artwork ---
    SongMiniLoaded(String, Option<image::Handle>),
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
    /// Track info strip was clicked — dispatch depends on strip_click_action setting
    StripClicked,
    /// Track info strip right-click context menu action
    StripContextAction(crate::widgets::context_menu::StripContextEntry),
    /// Toggle settings view: open if not in settings, return to Queue if already there
    ToggleSettings,

    // --- Login Result (handled at app level since it transitions screens) ---
    LoginResult(Result<AppService, String>),
    /// Resume session from stored JWT (no password needed)
    ResumeSession,

    // --- Data Loading ---
    LoadAlbums,
    LoadQueue,
    LoadArtists,
    LoadGenres,
    LoadPlaylists,
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
    /// Quit application (from hamburger menu)
    QuitApp,
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

    // --- MPRIS D-Bus Integration ---
    Mpris(services::mpris::MprisEvent),

    // --- Visualizer Hot-Reload ---
    /// Config file changed, apply new visualizer settings
    VisualizerConfigChanged(crate::visualizer_config::VisualizerConfig),

    // --- Theme Hot-Reload ---
    /// Config file changed, reload theme colors
    ThemeConfigReloaded,

    // --- Raw Keyboard Event ---
    /// Raw key press forwarded from subscription; dispatched via HotkeyConfig in update()
    RawKeyEvent(iced::keyboard::Key, iced::keyboard::Modifiers),

    // --- Toast Notifications ---
    Toast(ToastMessage),

    // --- Text Input Dialog ---
    TextInputDialog(crate::widgets::text_input_dialog::TextInputDialogMessage),

    // --- Info Modal ---
    InfoModal(crate::widgets::info_modal::InfoModalMessage),

    // --- About Modal ---
    AboutModal(crate::widgets::about_modal::AboutModalMessage),

    // --- Playlist Edit Mode (split-view) ---
    BrowsingPanel(views::BrowsingPanelMessage),
    /// Enter split-view playlist editing mode
    EnterPlaylistEditMode {
        playlist_id: String,
        playlist_name: String,
        playlist_comment: String,
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
}
