//! Consolidated state structs for Nokkvi
//!
//! These structs group related fields to reduce field sprawl in the main app struct.
//! Designed to scale for future views (Genres, Playlists).

use std::{
    collections::{HashMap, HashSet},
    num::NonZeroUsize,
};

use iced::widget::image;
use lru::LruCache;

/// Maximum entries in the large artwork LRU cache.
/// Each 500px image handle is ~80KB, so 200 entries ≈ 16MB cap.
const LARGE_ARTWORK_CACHE_CAPACITY: usize = 200;
/// Capacity for the mini-artwork (`album_art`) LRU. Sized roughly 6× a typical
/// 80px slot list viewport so recently-visited slot regions stay warm but
/// memory stays bounded as the user scrolls a large library.
const MINI_ARTWORK_CACHE_CAPACITY: usize = 512;

// ============================================================================
// Pane Focus (split-view playlist editing)
// ============================================================================

/// Which pane has keyboard focus during playlist edit mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaneFocus {
    #[default]
    Queue,
    Browser,
}

// ============================================================================
// Cross-Pane Drag State (browsing panel → queue)
// ============================================================================

/// Active cross-pane drag state (tracked at app level since DragColumn
/// can't span across separate widget trees).
///
/// No payload is needed — on drop we dispatch the active browsing view's
/// `AddCenterToQueue` message, which resolves the item internally.
#[derive(Debug, Clone)]
pub struct CrossPaneDragState {
    /// Where the drag started — used to draw offset from origin.
    pub origin: iced::Point,
    /// Current cursor position — updated via mouse event subscription.
    pub cursor: iced::Point,
    /// Snapshotted center item index at drag activation time.
    /// This is read from the browsing view's effective center when the drag
    /// threshold is exceeded, so the preview is decoupled from subsequent
    /// state changes (e.g., `selected_offset` being cleared by scrolling).
    pub center_index: Option<usize>,
    /// Queue slot the cursor is currently hovering over (for drop indicator).
    /// Updated on every cursor move; `None` when outside the queue pane.
    pub drop_target_slot: Option<usize>,
    /// Number of items in this drag. 1 = single item, >1 = batch from multi-selection.
    /// When >1, `handle_cross_pane_drag_released` skips `set_selected()` and lets
    /// `AddCenterToQueue` read the existing `selected_indices` on the slot list.
    pub selection_count: usize,
}

// ============================================================================
// Session & Playlist Context
// ============================================================================

/// Stored session for JWT-based auto-login.
///
/// Replaces the anonymous `Option<(String, String, String, String)>` tuple
/// that made field order ambiguous at every destructure site.
#[derive(Debug, Clone)]
pub struct StoredSession {
    pub server_url: String,
    pub username: String,
    pub jwt_token: String,
    pub subsonic_credential: String,
}

/// Identity of the playlist currently loaded in the queue.
///
/// Replaces the anonymous `Option<(String, String, String)>` tuple.
/// Set on PlayPlaylist, cleared on non-playlist play actions.
#[derive(Debug, Clone)]
pub struct ActivePlaylistContext {
    pub id: String,
    pub name: String,
    pub comment: String,
}

// ============================================================================
// Playback State
// ============================================================================

/// What is currently driving audio output.
/// `Queue` = normal library playback. `Radio` = direct internet radio stream.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ActivePlayback {
    #[default]
    Queue,
    Radio(RadioPlaybackState),
}

/// Transient state for an active radio stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RadioPlaybackState {
    pub station: nokkvi_data::types::radio_station::RadioStation,
    pub icy_artist: Option<String>,
    pub icy_title: Option<String>,
    pub icy_url: Option<String>,
}

impl ActivePlayback {
    pub fn is_radio(&self) -> bool {
        matches!(self, Self::Radio(_))
    }

    pub fn is_queue(&self) -> bool {
        matches!(self, Self::Queue)
    }

    pub fn radio_station(&self) -> Option<&nokkvi_data::types::radio_station::RadioStation> {
        match self {
            Self::Radio(state) => Some(&state.station),
            _ => None,
        }
    }

    /// Extract standard metadata for the Top Nav bar, overriding with
    /// radio metadata if a radio stream is active.
    pub fn nav_metadata(&self, fallback: &PlaybackState) -> (String, String, String) {
        match self {
            Self::Radio(state) => (
                state.station.name.clone(),
                "Radio".to_string(),
                state.station.stream_url.clone(),
            ),
            Self::Queue => (
                fallback.title.clone(),
                fallback.artist.clone(),
                fallback.album.clone(),
            ),
        }
    }
}

/// Playback-related state for the player bar
#[derive(Debug, Clone)]
pub struct PlaybackState {
    pub position: u32,
    pub duration: u32,
    pub playing: bool,
    pub paused: bool,
    pub title: String,
    pub artist: String,
    /// Album name of the currently playing track
    pub album: String,
    pub volume: f32,
    pub show_volume_percentage: bool,
    pub volume_change_id: u64,
    /// Audio format suffix (e.g., "flac", "mp3", "opus")
    pub format_suffix: String,
    /// Sample rate in Hz (e.g., 44100, 48000, 96000)
    pub sample_rate: u32,
    /// Bitrate in kbps (e.g., 320, 1411)
    pub bitrate: u32,
    /// Throttle timestamp for volume persistence to storage
    pub volume_persist_throttle: Option<std::time::Instant>,
    /// The last title successfully sent to PipeWire via IPC, to prevent redundant cross-thread FFI calls
    pub pw_last_title: Option<String>,
    /// Shared EQ state — gains and enabled flag. Read by audio thread, written by UI.
    pub eq_state: nokkvi_data::audio::EqState,
}

impl PlaybackState {
    /// Whether a track is actively loaded (playing or paused).
    pub fn has_track(&self) -> bool {
        self.playing || self.paused
    }
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            position: 0,
            duration: 0,
            playing: false,
            paused: false,
            title: "Not Playing".to_string(),
            artist: String::new(),
            album: String::new(),
            volume: 1.0,
            show_volume_percentage: false,
            volume_change_id: 0,
            format_suffix: String::new(),
            sample_rate: 0,
            bitrate: 0,
            volume_persist_throttle: None,
            pw_last_title: None,
            eq_state: nokkvi_data::audio::EqState::default(),
        }
    }
}

// ============================================================================
// Scrobble State
// ============================================================================

/// Scrobbling state with anti-seek-fraud protection
#[derive(Debug, Clone, Default)]
pub struct ScrobbleState {
    /// Actual seconds listened (not playback position) - prevents seek-fraud
    pub listening_time: f32,
    /// Last known position for calculating listening time deltas
    pub last_position: f32,
    /// Whether current song has been submitted (prevents double-scrobble)
    pub submitted: bool,
    /// Timer ID for debounced "now playing" notification
    pub now_playing_timer_id: u64,
    /// Current song ID for scrobble tracking
    pub current_song_id: Option<String>,
}

impl ScrobbleState {
    /// Reset for a new song
    pub fn reset_for_new_song(&mut self, song_id: Option<String>, position: f32) {
        self.current_song_id = song_id;
        self.listening_time = 0.0;
        self.last_position = position;
        self.submitted = false;
    }

    /// Check if scrobble conditions are met for the given track duration.
    /// Returns true if accumulated listening time meets the configured percentage
    /// of track duration and the song hasn't been submitted yet.
    pub fn should_scrobble(&self, track_duration: u32, threshold_percent: f32) -> bool {
        if self.submitted || track_duration == 0 {
            return false;
        }
        self.listening_time >= (track_duration as f32 * threshold_percent)
    }
}

// ============================================================================
// Playback Modes
// ============================================================================

/// Playback modes (random, repeat, consume) — persisted via AppService
#[derive(Debug, Clone, Default)]
pub struct PlaybackModes {
    pub random: bool,
    pub repeat: bool,
    pub repeat_queue: bool,
    pub consume: bool,
}

// ============================================================================
// Sound Effects State
// ============================================================================

/// Sound effects engine state
#[derive(Debug, Clone)]
pub struct SfxState {
    pub enabled: bool,
    pub volume: f32,
    pub show_percentage: bool,
    pub volume_change_id: u64,
}

impl Default for SfxState {
    fn default() -> Self {
        Self {
            enabled: true,
            volume: 0.68,
            show_percentage: false,
            volume_change_id: 0,
        }
    }
}

// ============================================================================
// Audio Engine State
// ============================================================================

/// Audio engine transient state (visualization, gapless, crossfade)
#[derive(Debug, Clone, Default)]
pub struct EngineState {
    pub visualization_mode: nokkvi_data::types::player_settings::VisualizationMode,
    pub gapless_preparing: bool,
    /// Whether crossfade between tracks is enabled
    pub crossfade_enabled: bool,
    /// Crossfade duration in seconds (1–12)
    pub crossfade_duration_secs: u32,
    /// Whether volume normalization (AGC) is enabled
    pub volume_normalization: bool,
    /// Volume normalization target level
    pub normalization_level: nokkvi_data::types::player_settings::NormalizationLevel,
}

/// Per-target collage artwork cache (genre or playlist)
#[derive(Debug, Clone, Default)]
pub struct CollageArtworkCache {
    /// Mini artwork cache (item_id -> Handle, first album's cover)
    pub mini: HashMap<String, image::Handle>,
    /// Collage artwork cache (item_id -> Vec<Handle> for 3x3 collage, up to 9)
    pub collage: HashMap<String, Vec<image::Handle>>,
    /// IDs with pending artwork loads (prevents duplicate in-flight requests)
    pub pending: HashSet<String>,
}

/// Artwork caches and loading state
#[derive(Clone)]
pub struct ArtworkState {
    /// Mini artwork cache (album_id -> Handle), bounded LRU.
    /// Without a persistent disk cache, this is the only thing keeping recently-
    /// rendered thumbnails warm. Capacity must stay above the typical viewport
    /// + scrollback or slot lists thrash.
    pub album_art: LruCache<String, image::Handle>,
    /// Read-only snapshot of `album_art` for view() borrowing (refreshed after LRU mutations).
    pub album_art_snapshot: HashMap<String, image::Handle>,
    /// Large artwork cache for detail views (LRU-bounded)
    pub large_artwork: LruCache<String, image::Handle>,
    /// Read-only snapshot of large_artwork for view() borrowing (refreshed after LRU mutations)
    pub large_artwork_snapshot: HashMap<String, image::Handle>,
    /// Cache for album dominant colors (extracted from large artwork bytes)
    pub album_dominant_colors: LruCache<String, iced::Color>,
    /// Read-only snapshot of dominant colors for view()
    pub album_dominant_colors_snapshot: HashMap<String, iced::Color>,
    /// Genre artwork cache
    pub genre: CollageArtworkCache,
    /// Playlist artwork cache
    pub playlist: CollageArtworkCache,
    /// Currently loading large artwork album ID
    pub loading_large_artwork: Option<String>,
}

impl ArtworkState {
    /// Refresh the read-only snapshot from the LRU cache.
    /// Call after any mutation to `large_artwork` (put/get).
    pub fn refresh_large_artwork_snapshot(&mut self) {
        self.large_artwork_snapshot = self
            .large_artwork
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
    }

    /// Refresh the read-only snapshot of mini album art from the LRU cache.
    /// Call after any mutation to `album_art` (put/get/pop).
    pub fn refresh_album_art_snapshot(&mut self) {
        self.album_art_snapshot = self
            .album_art
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
    }

    /// Refresh the read-only snapshot of dominant colors from the LRU cache.
    pub fn refresh_dominant_colors_snapshot(&mut self) {
        self.album_dominant_colors_snapshot = self
            .album_dominant_colors
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect();
    }
}

impl Default for ArtworkState {
    fn default() -> Self {
        Self {
            album_art: LruCache::new(
                NonZeroUsize::new(MINI_ARTWORK_CACHE_CAPACITY).expect("capacity must be > 0"),
            ),
            album_art_snapshot: HashMap::new(),
            large_artwork: LruCache::new(
                NonZeroUsize::new(LARGE_ARTWORK_CACHE_CAPACITY).expect("capacity must be > 0"),
            ),
            large_artwork_snapshot: HashMap::new(),
            album_dominant_colors: LruCache::new(
                NonZeroUsize::new(LARGE_ARTWORK_CACHE_CAPACITY).expect("capacity must be > 0"),
            ),
            album_dominant_colors_snapshot: HashMap::new(),
            genre: CollageArtworkCache::default(),
            playlist: CollageArtworkCache::default(),
            loading_large_artwork: None,
        }
    }
}

impl std::fmt::Debug for ArtworkState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArtworkState")
            .field("album_art", &self.album_art.len())
            .field("album_art_snapshot", &self.album_art_snapshot.len())
            .field("large_artwork", &self.large_artwork.len())
            .field("large_artwork_snapshot", &self.large_artwork_snapshot.len())
            .field("album_dominant_colors", &self.album_dominant_colors.len())
            .field(
                "album_dominant_colors_snapshot",
                &self.album_dominant_colors_snapshot.len(),
            )
            .field("genre", &self.genre)
            .field("playlist", &self.playlist)
            .field("loading_large_artwork", &self.loading_large_artwork)
            .finish()
    }
}

// ============================================================================
// Window State
// ============================================================================

/// Window dimensions and scale factor
#[derive(Debug, Clone)]
pub struct WindowState {
    pub width: f32,
    pub height: f32,
    pub scale_factor: f32,
    /// Whether the EQ modal overlay is currently visible.
    pub eq_modal_open: bool,
    /// Whether the EQ modal is in "save preset" mode (showing name input).
    pub eq_save_mode: bool,
    /// Text input content for the preset name being saved.
    pub eq_save_name: String,
    /// Cached custom EQ presets (loaded from redb, kept in sync on save/delete).
    pub custom_eq_presets: Vec<nokkvi_data::audio::eq::CustomEqPreset>,
    /// Global keyboard modifiers tracked for mouse interaction (e.g. shift-clicking)
    pub keyboard_modifiers: iced::keyboard::Modifiers,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            width: 1200.0,
            height: 800.0,
            scale_factor: 1.0,
            eq_modal_open: false,
            eq_save_mode: false,
            eq_save_name: String::new(),
            custom_eq_presets: Vec::new(),
            keyboard_modifiers: iced::keyboard::Modifiers::default(),
        }
    }
}

// ============================================================================
// Library Data
// ============================================================================

/// All loaded library data vectors + counts
///
/// Groups the 6 data vectors and their associated counts that were
/// previously individual fields on Nokkvi.
///
/// Albums, artists, songs, genres, and playlists use `PagedBuffer<T>` for
/// server-side pagination. Queue stays as `Vec<T>` since it's managed
/// locally by the queue service, not paginated from the API.
#[derive(Debug, Clone, Default)]
pub struct LibraryData {
    pub albums: nokkvi_data::types::paged_buffer::PagedBuffer<
        nokkvi_data::backend::albums::AlbumUIViewData,
    >,
    pub artists: nokkvi_data::types::paged_buffer::PagedBuffer<
        nokkvi_data::backend::artists::ArtistUIViewData,
    >,
    pub songs:
        nokkvi_data::types::paged_buffer::PagedBuffer<nokkvi_data::backend::songs::SongUIViewData>,
    pub genres: nokkvi_data::types::paged_buffer::PagedBuffer<
        nokkvi_data::backend::genres::GenreUIViewData,
    >,
    pub playlists: nokkvi_data::types::paged_buffer::PagedBuffer<
        nokkvi_data::backend::playlists::PlaylistUIViewData,
    >,
    pub queue_songs: Vec<nokkvi_data::backend::queue::QueueSongUIViewData>,
    pub radio_stations: Vec<nokkvi_data::types::radio_station::RadioStation>,
    /// Target count during progressive queue loading (e.g., 12036 while loading).
    /// When `Some`, the queue header shows "X of Y songs" as pages are appended.
    pub queue_loading_target: Option<usize>,
    /// Generation counter for progressive queue loading. Incremented each time
    /// play-from-songs starts a new chain; stale chains self-cancel by comparing
    /// their generation against this value.
    pub progressive_queue_generation: u64,
    pub counts: LibraryCounts,
}

// ============================================================================
// Library Counts
// ============================================================================

/// Total counts for library items (used in headers)
#[derive(Debug, Clone, Default)]
pub struct LibraryCounts {
    pub albums: usize,
    pub artists: usize,
    pub genres: usize,
    pub playlists: usize,
    pub songs: usize,
}

// ============================================================================
// Toast State
// ============================================================================

/// In-app notification state (bounded ring buffer, render-time expiry)
///
/// Follows rmpc's `StatusMessage` pattern: no GC pass needed — `current()`
/// checks expiry at render time.
#[derive(Debug, Clone, Default)]
pub struct ToastState {
    pub toasts: std::collections::VecDeque<nokkvi_data::types::toast::Toast>,
}

impl ToastState {
    /// Maximum active toasts before oldest is evicted
    const MAX_TOASTS: usize = 10;

    /// Push a new toast. If the toast has a `key`, remove any existing toast
    /// with the same key and re-insert at the back (most-recent position).
    pub fn push(&mut self, toast: nokkvi_data::types::toast::Toast) {
        if let Some(ref key) = toast.key {
            // Remove existing keyed toast so the updated one lands at the back
            self.toasts.retain(|t| t.key.as_deref() != Some(key));
        }
        if self.toasts.len() >= Self::MAX_TOASTS {
            self.toasts.pop_front();
        }
        self.toasts.push_back(toast);
    }

    /// Remove a keyed toast by its key.
    pub fn dismiss_key(&mut self, key: &str) {
        self.toasts.retain(|t| t.key.as_deref() != Some(key));
    }

    /// Most recent non-expired toast (scans from back to find the first visible one)
    pub fn current(&self) -> Option<&nokkvi_data::types::toast::Toast> {
        self.toasts.iter().rev().find(|t| !t.is_expired())
    }
}

// ============================================================================
// Similar Songs State
// ============================================================================

/// Ephemeral state for Similar Songs / Top Songs API results.
///
/// Populated by `getSimilarSongs2` or `getTopSongs` API calls triggered from
/// context menus. Not persisted — re-triggered via right-click → Find Similar.
#[derive(Debug, Clone)]
pub struct SimilarSongsState {
    /// API result songs (one-shot, not PagedBuffer)
    pub songs: Vec<nokkvi_data::types::song::Song>,
    /// Provenance label: "Similar to: Paranoid Android" / "Top Songs: Radiohead"
    pub label: String,
    /// Whether an API call is currently in flight
    pub loading: bool,
}
