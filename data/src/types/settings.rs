use serde::{Deserialize, Serialize};

use crate::types::{
    hotkey_config::HotkeyConfig,
    player_settings::{
        EnterBehavior, NavDisplayMode, NavLayout, NormalizationLevel, SlotRowHeight,
        TrackInfoDisplay, VisualizationMode,
    },
    queue::{QueueSortPreferences, SortPreferences},
    queue_sort_mode::QueueSortMode,
    sort_mode::SortMode,
};

/// Player-related settings (volume, visualizer, theme, general)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerSettings {
    #[serde(default = "default_volume")]
    pub volume: f64,
    #[serde(default = "default_sfx_volume")]
    pub sfx_volume: f64,
    #[serde(default = "default_sound_effects_enabled")]
    pub sound_effects_enabled: bool,
    #[serde(default)]
    pub visualization_mode: VisualizationMode,
    #[serde(default)]
    pub light_mode: bool,
    /// Whether scrobbling is enabled (default: true)
    #[serde(default = "default_scrobbling_enabled")]
    pub scrobbling_enabled: bool,
    /// Scrobble threshold as a fraction of track duration (0.25–0.90, default 0.50)
    #[serde(default = "default_scrobble_threshold")]
    pub scrobble_threshold: f64,
    /// Start view name ("Queue", "Albums", etc. — default "Queue")
    #[serde(default = "default_start_view")]
    pub start_view: String,
    /// Stable viewport mode (default: true)
    /// When enabled, clicking items highlights in-place without scrolling,
    /// and playback changes don't auto-scroll the viewport.
    #[serde(default = "default_stable_viewport")]
    pub stable_viewport: bool,
    /// Auto-follow playing track (default: true)
    /// When enabled, the queue view auto-scrolls to the currently playing
    /// track on track changes and queue reload.
    #[serde(default = "default_auto_follow_playing")]
    pub auto_follow_playing: bool,
    /// What Enter does when activating items (default: PlayAll)
    #[serde(default)]
    pub enter_behavior: EnterBehavior,
    /// Local filesystem prefix for the music library (default: empty = not configured).
    /// Joined with the server-relative song path to form an absolute local path.
    /// e.g. "/music/Library" for local Navidrome, or "/mnt/nas/music" for NFS mounts.
    #[serde(default)]
    pub local_music_path: String,
    /// Rounded corners mode (default: false = square)
    #[serde(default)]
    pub rounded_mode: bool,
    /// Navigation layout mode (default: Top = horizontal bar)
    #[serde(default)]
    pub nav_layout: NavLayout,
    /// Navigation display mode (default: TextOnly)
    #[serde(default)]
    pub nav_display_mode: NavDisplayMode,
    /// Track info display mode (off / player bar / top bar)
    #[serde(default)]
    pub track_info_display: TrackInfoDisplay,
    /// Slot list row density (default: Default = 70px)
    #[serde(default)]
    pub slot_row_height: SlotRowHeight,
    /// Whether the opacity gradient on non-center slots is enabled (default: true)
    #[serde(default = "default_opacity_gradient")]
    pub opacity_gradient: bool,
    /// Whether crossfade between tracks is enabled (default: false)
    #[serde(default)]
    pub crossfade_enabled: bool,
    /// Crossfade duration in seconds (1–12, default 5)
    #[serde(default = "default_crossfade_duration_secs")]
    pub crossfade_duration_secs: u32,
    /// Default playlist ID for quick-add (None = no default)
    #[serde(default)]
    pub default_playlist_id: Option<String>,
    /// Default playlist display name (for settings UI)
    #[serde(default)]
    pub default_playlist_name: String,
    /// Whether to skip the Add to Playlist dialog and use the default playlist directly
    #[serde(default)]
    pub quick_add_to_playlist: bool,
    /// Whether volume sliders in the player bar are horizontal (default: false = vertical)
    #[serde(default)]
    pub horizontal_volume: bool,
    /// Whether volume normalization (AGC) is enabled (default: false)
    #[serde(default)]
    pub volume_normalization: bool,
    /// Volume normalization target level (default: Normal)
    #[serde(default)]
    pub normalization_level: NormalizationLevel,
}

fn default_volume() -> f64 {
    1.0
}
fn default_sfx_volume() -> f64 {
    0.68
}
fn default_sound_effects_enabled() -> bool {
    true
}
fn default_scrobbling_enabled() -> bool {
    true
}
fn default_scrobble_threshold() -> f64 {
    0.50
}
fn default_start_view() -> String {
    "Queue".to_string()
}
fn default_stable_viewport() -> bool {
    true
}
fn default_auto_follow_playing() -> bool {
    true
}
fn default_opacity_gradient() -> bool {
    true
}
fn default_crossfade_duration_secs() -> u32 {
    5
}

impl Default for PlayerSettings {
    fn default() -> Self {
        Self {
            volume: default_volume(),
            sfx_volume: default_sfx_volume(),
            sound_effects_enabled: default_sound_effects_enabled(),
            visualization_mode: VisualizationMode::default(),
            light_mode: false,
            scrobbling_enabled: default_scrobbling_enabled(),
            scrobble_threshold: default_scrobble_threshold(),
            start_view: default_start_view(),
            stable_viewport: default_stable_viewport(),
            auto_follow_playing: default_auto_follow_playing(),
            enter_behavior: EnterBehavior::default(),
            local_music_path: String::new(),
            rounded_mode: false,
            nav_layout: NavLayout::default(),
            nav_display_mode: NavDisplayMode::default(),
            track_info_display: TrackInfoDisplay::default(),
            slot_row_height: SlotRowHeight::default(),
            opacity_gradient: default_opacity_gradient(),
            crossfade_enabled: false,
            crossfade_duration_secs: default_crossfade_duration_secs(),
            default_playlist_id: None,
            default_playlist_name: String::new(),
            quick_add_to_playlist: false,
            horizontal_volume: false,
            volume_normalization: false,
            normalization_level: NormalizationLevel::default(),
        }
    }
}

/// View sort preferences for all views
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewPreferences {
    #[serde(default = "default_albums_prefs")]
    pub albums: SortPreferences,
    #[serde(default = "default_artists_prefs")]
    pub artists: SortPreferences,
    #[serde(default = "default_songs_prefs")]
    pub songs: SortPreferences,
    #[serde(default = "default_genres_prefs")]
    pub genres: SortPreferences,
    #[serde(default = "default_playlists_prefs")]
    pub playlists: SortPreferences,
    #[serde(default = "default_queue_prefs")]
    pub queue: QueueSortPreferences,
}

fn default_albums_prefs() -> SortPreferences {
    SortPreferences::new(SortMode::RecentlyAdded, false)
}

fn default_artists_prefs() -> SortPreferences {
    SortPreferences::new(SortMode::Name, true)
}

fn default_songs_prefs() -> SortPreferences {
    SortPreferences::new(SortMode::RecentlyAdded, false)
}

fn default_genres_prefs() -> SortPreferences {
    SortPreferences::new(SortMode::Name, true)
}

fn default_playlists_prefs() -> SortPreferences {
    SortPreferences::new(SortMode::UpdatedAt, false)
}

fn default_queue_prefs() -> QueueSortPreferences {
    QueueSortPreferences::new(QueueSortMode::Album, true)
}

impl Default for ViewPreferences {
    fn default() -> Self {
        Self {
            albums: default_albums_prefs(),
            artists: default_artists_prefs(),
            songs: default_songs_prefs(),
            genres: default_genres_prefs(),
            playlists: default_playlists_prefs(),
            queue: default_queue_prefs(),
        }
    }
}

/// Combined user settings (player + view preferences + hotkeys)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserSettings {
    #[serde(default)]
    pub player: PlayerSettings,
    #[serde(default)]
    pub views: ViewPreferences,
    #[serde(default)]
    pub hotkeys: HotkeyConfig,
}
