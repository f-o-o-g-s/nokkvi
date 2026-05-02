use serde::{Deserialize, Serialize};

use crate::types::{
    hotkey_config::HotkeyConfig,
    player_settings::{
        ArtworkColumnMode, ArtworkResolution, ArtworkStretchFit, EnterBehavior, LibraryPageSize,
        NavDisplayMode, NavLayout, NormalizationLevel, SlotRowHeight, StripClickAction,
        StripSeparator, TrackInfoDisplay, VisualizationMode, VolumeNormalizationMode,
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
    /// Whether clickable text links in slot list items are enabled (default: true)
    #[serde(default = "default_true")]
    pub slot_text_links: bool,
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
    /// Whether the queue view's header shows the default playlist chip (default: false)
    #[serde(default)]
    pub queue_show_default_playlist: bool,
    /// Whether volume sliders in the player bar are horizontal (default: false = vertical)
    #[serde(default)]
    pub horizontal_volume: bool,
    /// Font family override (default: empty = system default sans-serif)
    #[serde(default)]
    pub font_family: String,
    /// Volume normalization mode (default: Off). On-disk key is
    /// `volume_normalization_mode`.
    #[serde(default, rename = "volume_normalization_mode")]
    pub volume_normalization: VolumeNormalizationMode,
    /// AGC target level (default: Normal). Only meaningful when
    /// `volume_normalization == Agc`.
    #[serde(default)]
    pub normalization_level: NormalizationLevel,
    /// Pre-amp dB applied on top of resolved ReplayGain (default 0.0).
    #[serde(default)]
    pub replay_gain_preamp_db: f32,
    /// Fallback dB for tracks with no ReplayGain tags (default 0.0 = unity).
    #[serde(default)]
    pub replay_gain_fallback_db: f32,
    /// When true, untagged tracks fall through to AGC.
    #[serde(default)]
    pub replay_gain_fallback_to_agc: bool,
    /// When true, clamp gain so `peak * gain <= 1.0` (default true).
    #[serde(default = "default_true")]
    pub replay_gain_prevent_clipping: bool,
    /// Whether the title field is visible in the track info strip (default: true)
    #[serde(default = "default_true")]
    pub strip_show_title: bool,
    /// Whether the artist field is visible in the track info strip (default: true)
    #[serde(default = "default_true")]
    pub strip_show_artist: bool,
    /// Whether the album field is visible in the track info strip (default: true)
    #[serde(default = "default_true")]
    pub strip_show_album: bool,
    /// Whether format info (codec/kHz/kbps) is visible in the track info strip (default: true)
    #[serde(default = "default_true")]
    pub strip_show_format_info: bool,
    /// Whether the metastrip renders artist/album/title as a single shared
    /// scrolling unit with one set of bookend separators (default: false).
    #[serde(default)]
    pub strip_merged_mode: bool,
    /// What happens when clicking the track info strip (default: GoToQueue)
    #[serde(default)]
    pub strip_click_action: StripClickAction,
    /// Whether `title:` / `artist:` / `album:` labels are prepended to each
    /// field in the metadata strip (default: true).
    #[serde(default = "default_true")]
    pub strip_show_labels: bool,
    /// Visual character used to join visible fields in merged-mode rendering
    /// (default: Dot · — matches the historical hardcoded join).
    #[serde(default)]
    pub strip_separator: StripSeparator,
    /// Active playlist ID loaded in the queue (None = no playlist context)
    #[serde(default)]
    pub active_playlist_id: Option<String>,
    /// Active playlist display name
    #[serde(default)]
    pub active_playlist_name: String,
    /// Active playlist comment/description
    #[serde(default)]
    pub active_playlist_comment: String,
    /// Whether the 10-band graphic EQ is enabled (master bypass).
    #[serde(default)]
    pub eq_enabled: bool,
    /// Per-band EQ gain values in dB (-12.0 to +12.0). Indexed by band.
    #[serde(default = "default_eq_gains")]
    pub eq_gains: [f32; 10],
    /// User-created custom EQ presets.
    #[serde(default)]
    pub custom_eq_presets: Vec<crate::audio::eq::CustomEqPreset>,
    /// When true, all settings (including defaults) are written to config.toml
    #[serde(default)]
    pub verbose_config: bool,
    /// Library page size controls how many items are fetched at once.
    #[serde(default)]
    pub library_page_size: LibraryPageSize,
    /// Artwork resolution for the large panel (Default / High / Ultra / Original)
    #[serde(default)]
    pub artwork_resolution: ArtworkResolution,
    /// Whether the Artists view shows only album artists
    #[serde(default = "default_true")]
    pub show_album_artists_only: bool,
    /// Whether to suppress the toast notification shown when Navidrome
    /// emits a library-refresh event (default: false = toasts shown).
    #[serde(default)]
    pub suppress_library_refresh_toasts: bool,
    /// Whether the queue's stars rating column is visible (default: true).
    /// Subject to a separate responsive width gate — see queue.rs.
    #[serde(default = "default_true")]
    pub queue_show_stars: bool,
    /// Whether the queue's album column is visible (default: true).
    #[serde(default = "default_true")]
    pub queue_show_album: bool,
    /// Whether the queue's duration column is visible (default: true).
    #[serde(default = "default_true")]
    pub queue_show_duration: bool,
    /// Whether the queue's love (heart) column is visible (default: true).
    #[serde(default = "default_true")]
    pub queue_show_love: bool,
    /// Whether the queue's plays column is visible (default: false).
    /// When sort = MostPlayed, the column auto-shows regardless of this toggle.
    #[serde(default)]
    pub queue_show_plays: bool,
    /// Whether the queue's leading row-index column is visible (default: true).
    #[serde(default = "default_true")]
    pub queue_show_index: bool,
    /// Whether the queue's leading thumbnail column is visible (default: true).
    #[serde(default = "default_true")]
    pub queue_show_thumbnail: bool,

    // -- Albums view column toggles --
    /// Stars column. Auto-shows when sort = Rating regardless of toggle.
    #[serde(default)]
    pub albums_show_stars: bool,
    /// Song count column.
    #[serde(default = "default_true")]
    pub albums_show_songcount: bool,
    /// Plays column. Auto-shows when sort = MostPlayed regardless of toggle.
    #[serde(default)]
    pub albums_show_plays: bool,
    /// Heart (favorite) column.
    #[serde(default = "default_true")]
    pub albums_show_love: bool,
    /// Leading row-index column.
    #[serde(default = "default_true")]
    pub albums_show_index: bool,
    /// Leading thumbnail column.
    #[serde(default = "default_true")]
    pub albums_show_thumbnail: bool,

    // -- Songs view column toggles --
    /// Stars column. Auto-shows when sort = Rating regardless of toggle.
    #[serde(default)]
    pub songs_show_stars: bool,
    /// Album column.
    #[serde(default = "default_true")]
    pub songs_show_album: bool,
    /// Duration column.
    #[serde(default = "default_true")]
    pub songs_show_duration: bool,
    /// Plays column. Auto-shows when sort = MostPlayed regardless of toggle.
    #[serde(default)]
    pub songs_show_plays: bool,
    /// Heart (favorite) column.
    #[serde(default = "default_true")]
    pub songs_show_love: bool,
    /// Leading row-index column.
    #[serde(default = "default_true")]
    pub songs_show_index: bool,
    /// Leading thumbnail column.
    #[serde(default = "default_true")]
    pub songs_show_thumbnail: bool,

    // -- Artists view column toggles --
    /// Stars column. Auto-shows when sort = Rating regardless of toggle.
    #[serde(default = "default_true")]
    pub artists_show_stars: bool,
    /// Album count column.
    #[serde(default = "default_true")]
    pub artists_show_albumcount: bool,
    /// Song count column.
    #[serde(default = "default_true")]
    pub artists_show_songcount: bool,
    /// Plays column. Auto-shows when sort = MostPlayed regardless of toggle.
    #[serde(default = "default_true")]
    pub artists_show_plays: bool,
    /// Heart (favorite) column.
    #[serde(default = "default_true")]
    pub artists_show_love: bool,
    /// Leading row-index column.
    #[serde(default = "default_true")]
    pub artists_show_index: bool,
    /// Leading thumbnail column.
    #[serde(default = "default_true")]
    pub artists_show_thumbnail: bool,

    // -- Genres view column toggles --
    /// Leading row-index column.
    #[serde(default = "default_true")]
    pub genres_show_index: bool,
    /// Thumbnail column on parent genre rows; also drives whether nested
    /// child album rows in the genre→album expansion render their artwork.
    #[serde(default = "default_true")]
    pub genres_show_thumbnail: bool,
    /// Album-count column.
    #[serde(default = "default_true")]
    pub genres_show_albumcount: bool,
    /// Song-count column.
    #[serde(default = "default_true")]
    pub genres_show_songcount: bool,

    // -- Per-view artwork text overlay toggles --
    /// Whether the metadata text overlay is rendered on the large artwork in Albums view.
    #[serde(default = "default_true")]
    pub albums_artwork_overlay: bool,
    /// Whether the metadata text overlay is rendered on the large artwork in Artists view.
    #[serde(default = "default_true")]
    pub artists_artwork_overlay: bool,
    /// Whether the metadata text overlay is rendered on the large artwork in Songs view.
    #[serde(default = "default_true")]
    pub songs_artwork_overlay: bool,
    /// Whether the metadata text overlay is rendered on the large artwork in Playlists view.
    #[serde(default = "default_true")]
    pub playlists_artwork_overlay: bool,

    // -- Artwork column layout --
    /// Display mode for the large artwork column (auto / always-native / always-stretched / never).
    #[serde(default)]
    pub artwork_column_mode: ArtworkColumnMode,
    /// Fit mode used when `artwork_column_mode == AlwaysStretched`.
    #[serde(default)]
    pub artwork_column_stretch_fit: ArtworkStretchFit,
    /// Artwork column width as a fraction of window width (0.05..=0.80).
    /// Only consulted in always modes.
    #[serde(default = "default_artwork_column_width_pct")]
    pub artwork_column_width_pct: f32,

    // -- System tray --
    /// Whether to register a system tray (StatusNotifierItem) icon.
    /// Requires the compositor to host an SNI tray (e.g. waybar with the
    /// `tray` module on Hyprland; AppIndicator extension on GNOME).
    #[serde(default)]
    pub show_tray_icon: bool,
    /// When true and `show_tray_icon` is on, pressing the window's close button
    /// hides the window into the tray instead of quitting the app.
    #[serde(default)]
    pub close_to_tray: bool,
}

fn default_artwork_column_width_pct() -> f32 {
    0.40
}

fn default_eq_gains() -> [f32; 10] {
    [0.0; 10]
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
fn default_true() -> bool {
    true
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
            slot_text_links: default_true(),
            crossfade_enabled: false,
            crossfade_duration_secs: default_crossfade_duration_secs(),
            default_playlist_id: None,
            default_playlist_name: String::new(),
            quick_add_to_playlist: false,
            queue_show_default_playlist: false,
            horizontal_volume: false,
            font_family: String::new(),
            volume_normalization: VolumeNormalizationMode::default(),
            normalization_level: NormalizationLevel::default(),
            replay_gain_preamp_db: 0.0,
            replay_gain_fallback_db: 0.0,
            replay_gain_fallback_to_agc: false,
            replay_gain_prevent_clipping: true,
            strip_show_title: true,
            strip_show_artist: true,
            strip_show_album: true,
            strip_show_format_info: true,
            strip_merged_mode: false,
            strip_click_action: StripClickAction::default(),
            strip_show_labels: true,
            strip_separator: StripSeparator::default(),
            active_playlist_id: None,
            active_playlist_name: String::new(),
            active_playlist_comment: String::new(),
            eq_enabled: false,
            eq_gains: default_eq_gains(),
            custom_eq_presets: Vec::new(),
            verbose_config: false,
            library_page_size: LibraryPageSize::default(),
            artwork_resolution: ArtworkResolution::default(),
            show_album_artists_only: default_true(),
            suppress_library_refresh_toasts: false,
            queue_show_stars: true,
            queue_show_album: true,
            queue_show_duration: true,
            queue_show_love: true,
            queue_show_plays: false,
            queue_show_index: true,
            queue_show_thumbnail: true,
            albums_show_stars: false,
            albums_show_songcount: true,
            albums_show_plays: false,
            albums_show_love: true,
            albums_show_index: true,
            albums_show_thumbnail: true,
            songs_show_stars: false,
            songs_show_album: true,
            songs_show_duration: true,
            songs_show_plays: false,
            songs_show_love: true,
            songs_show_index: true,
            songs_show_thumbnail: true,
            artists_show_stars: true,
            artists_show_albumcount: true,
            artists_show_songcount: true,
            artists_show_plays: true,
            artists_show_love: true,
            artists_show_index: true,
            artists_show_thumbnail: true,
            genres_show_index: true,
            genres_show_thumbnail: true,
            genres_show_albumcount: true,
            genres_show_songcount: true,
            albums_artwork_overlay: true,
            artists_artwork_overlay: true,
            songs_artwork_overlay: true,
            playlists_artwork_overlay: true,
            artwork_column_mode: ArtworkColumnMode::default(),
            artwork_column_stretch_fit: ArtworkStretchFit::default(),
            artwork_column_width_pct: default_artwork_column_width_pct(),
            show_tray_icon: false,
            close_to_tray: false,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_merged_mode_default_is_off() {
        let p = PlayerSettings::default();
        assert!(!p.strip_merged_mode);
    }

    #[test]
    fn strip_merged_mode_roundtrips_through_serde() {
        let mut p = PlayerSettings::default();
        p.strip_merged_mode = true;
        let json = serde_json::to_string(&p).expect("serialize");
        let parsed: PlayerSettings = serde_json::from_str(&json).expect("deserialize");
        assert!(parsed.strip_merged_mode);
    }

    #[test]
    fn strip_merged_mode_missing_field_defaults_to_false() {
        // Simulate older redb-stored settings without the field.
        let json = r#"{}"#;
        let parsed: PlayerSettings = serde_json::from_str(json).expect("deserialize");
        assert!(!parsed.strip_merged_mode);
    }

    #[test]
    fn replay_gain_prevent_clipping_defaults_to_true_for_missing_field() {
        let json = r#"{}"#;
        let parsed: PlayerSettings = serde_json::from_str(json).expect("deserialize");
        assert!(parsed.replay_gain_prevent_clipping);
    }
}
