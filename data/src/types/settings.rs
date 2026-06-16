use serde::{Deserialize, Serialize};

use crate::{
    audio::eq::EQ_BAND_COUNT,
    types::{
        hotkey_config::HotkeyConfig,
        player_settings::{
            ArtworkColumnMode, ArtworkResolution, ArtworkStretchFit, BitPerfectMode,
            CollapsedAppearance, EnterBehavior, LibraryPageSize, NavDisplayMode, NavLayout,
            NormalizationLevel, RatingReminderTrigger, RoundedMode, SlotRowHeight,
            StripClickAction, StripSeparator, TrackInfoDisplay, VisualizationMode,
            VolumeNormalizationMode, deserialize_bit_perfect_with_bool_compat,
            deserialize_rounded_mode_with_bool_compat,
        },
        queue::{QueueSortPreferences, SortPreferences},
        queue_sort_mode::QueueSortMode,
        sort_mode::SortMode,
        view_columns::ViewColumns,
    },
};

/// Player-related settings (volume, visualizer, theme, general)
///
/// Redb-shaped: persisted via `serde_json::to_vec` in
/// `services/state_storage.rs`. Renamed from `PlayerSettings` so it no longer
/// collides with the UI-facing `LivePlayerSettings` in the adjacent
/// `player_settings` module. Persistence is byte-stable across this rename:
/// serde_json keys by field name, never by struct name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedPlayerSettings {
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
    /// Rounded corners mode (default: `On`).
    ///
    /// Field-level shim accepts legacy bool values (`true` → `On`,
    /// `false` → `Off`) for configs written before the tri-state migration.
    #[serde(
        default,
        deserialize_with = "deserialize_rounded_mode_with_bool_compat"
    )]
    pub rounded_mode: RoundedMode,
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
    /// Whether the opacity gradient on non-center slots is enabled (default: false)
    #[serde(default = "default_opacity_gradient")]
    pub opacity_gradient: bool,
    /// Whether clickable text links in slot list items are enabled (default: true)
    #[serde(default = "default_true")]
    pub slot_text_links: bool,
    /// Whether crossfade between tracks is enabled (default: true)
    #[serde(default = "default_true")]
    pub crossfade_enabled: bool,
    /// Bit-perfect output mode (Off / Strict / Relaxed): Strict and Relaxed
    /// play at each track's native sample rate with the DSP chain (EQ / software
    /// volume / limiter) bypassed, letting PipeWire switch the device clock.
    /// They differ only on same-rate crossfades. Off by default (opt-in).
    /// Legacy bool records load via the compat shim (true → Strict, false → Off).
    #[serde(default, deserialize_with = "deserialize_bit_perfect_with_bool_compat")]
    pub bit_perfect: BitPerfectMode,
    /// Crossfade duration in seconds (1–12, default 7)
    #[serde(default = "default_crossfade_duration_secs")]
    pub crossfade_duration_secs: u32,
    /// Whether the Previous button restarts the current track once it has
    /// played past the threshold (default false).
    #[serde(default)]
    pub rewind_on_previous: bool,
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
    /// Whether the view-header toolbar auto-hides to a thin line until hovered
    /// or a sort/search shortcut is used (default: true)
    #[serde(default)]
    pub autohide_toolbar: bool,
    /// Collapsed auto-hide toolbar height in px (default: 4)
    #[serde(default = "default_autohide_toolbar_height")]
    pub autohide_toolbar_height: u32,
    /// Whether the collapsed auto-hide toolbar shows a centered accent grip bar (default: true)
    #[serde(default = "default_true")]
    pub autohide_toolbar_grip: bool,
    /// What the collapsed auto-hide toolbar shows (default: Count strip)
    #[serde(default)]
    pub autohide_collapsed_appearance: CollapsedAppearance,
    /// Whether the mini-player bar shows the volume slider (default: true).
    /// Only affects `TrackInfoDisplay::MiniPlayer`.
    #[serde(default = "default_true")]
    pub mini_player_show_volume: bool,
    /// Whether the mini-player bar shows the modes menu (default: true).
    /// Only affects `TrackInfoDisplay::MiniPlayer`.
    #[serde(default = "default_true")]
    pub mini_player_show_modes: bool,
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
    /// scrolling unit with one set of bookend separators (default: true).
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
    /// (default: Slash /).
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
    /// Active playlist total duration in seconds (0.0 when unknown).
    #[serde(default)]
    pub active_playlist_duration: f32,
    /// Active playlist last-updated timestamp (raw ISO-8601; empty when unknown).
    #[serde(default)]
    pub active_playlist_updated: String,
    /// Active playlist public/private visibility (drives the strip lock chip).
    #[serde(default)]
    pub active_playlist_public: bool,
    /// Active playlist song count (0 when unknown; strip falls back to queue length).
    #[serde(default)]
    pub active_playlist_song_count: u32,
    /// Whether the 10-band graphic EQ is enabled (master bypass).
    #[serde(default)]
    pub eq_enabled: bool,
    /// Per-band EQ gain values in dB (-12.0 to +12.0). Indexed by band.
    #[serde(default = "default_eq_gains")]
    pub eq_gains: [f32; EQ_BAND_COUNT],
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
    /// emits a library-refresh event (default: true = toasts suppressed).
    #[serde(default)]
    pub suppress_library_refresh_toasts: bool,
    /// Per-view column-visibility toggles — flattened so every
    /// `<view>_show_<col>` key stays a TOP-LEVEL key on the persisted JSON
    /// wire (pinned by `persisted_column_keys_stay_flat_on_the_json_wire`).
    /// Missing keys fill from `ViewColumns::default()` — the single source
    /// of truth for the shipped column defaults.
    #[serde(flatten)]
    pub view_columns: ViewColumns,

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
    /// Auto-mode max artwork size as a fraction of the window's short axis
    /// (0.30..=0.70). Default 0.40. The Auto resolver uses this for both the
    /// horizontal candidate and the portrait-fallback vertical candidate.
    #[serde(default = "default_artwork_auto_max_pct")]
    pub artwork_auto_max_pct: f32,
    /// Always-Vertical artwork height as a fraction of window height
    /// (0.10..=0.80). Default 0.40. Consulted by the AlwaysVerticalNative /
    /// AlwaysVerticalStretched resolver branches.
    #[serde(default = "default_artwork_vertical_height_pct")]
    pub artwork_vertical_height_pct: f32,

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

    // -- Rating reminder --
    /// Whether the rate-this-track desktop notification is enabled (default false).
    #[serde(default)]
    pub rating_reminder_enabled: bool,
    /// When the rating reminder fires (default: on scrobble).
    #[serde(default)]
    pub rating_reminder_trigger: RatingReminderTrigger,
    /// Percent of track played that fires the reminder in percentage mode
    /// (default 75; UI clamp 60–90).
    #[serde(default = "default_rating_reminder_percent")]
    pub rating_reminder_percent: u32,
}

fn default_artwork_column_width_pct() -> f32 {
    crate::types::player_settings::ARTWORK_COLUMN_WIDTH_PCT_DEFAULT
}

fn default_artwork_auto_max_pct() -> f32 {
    crate::types::player_settings::ARTWORK_AUTO_MAX_PCT_DEFAULT
}

fn default_artwork_vertical_height_pct() -> f32 {
    crate::types::player_settings::ARTWORK_VERTICAL_HEIGHT_PCT_DEFAULT
}

fn default_eq_gains() -> [f32; EQ_BAND_COUNT] {
    [0.0; EQ_BAND_COUNT]
}

fn default_volume() -> f64 {
    1.0
}
fn default_sfx_volume() -> f64 {
    0.68
}
fn default_sound_effects_enabled() -> bool {
    false
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
    false
}
fn default_crossfade_duration_secs() -> u32 {
    7
}
fn default_autohide_toolbar_height() -> u32 {
    4
}
fn default_true() -> bool {
    true
}
fn default_rating_reminder_percent() -> u32 {
    75
}

impl Default for PersistedPlayerSettings {
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
            rounded_mode: RoundedMode::On,
            nav_layout: NavLayout::default(),
            nav_display_mode: NavDisplayMode::default(),
            track_info_display: TrackInfoDisplay::MiniPlayer,
            slot_row_height: SlotRowHeight::Compact,
            opacity_gradient: default_opacity_gradient(),
            slot_text_links: default_true(),
            crossfade_enabled: true,
            bit_perfect: BitPerfectMode::default(),
            crossfade_duration_secs: default_crossfade_duration_secs(),
            rewind_on_previous: false,
            default_playlist_id: None,
            default_playlist_name: String::new(),
            quick_add_to_playlist: false,
            queue_show_default_playlist: false,
            horizontal_volume: false,
            autohide_toolbar: true,
            autohide_toolbar_height: default_autohide_toolbar_height(),
            autohide_toolbar_grip: true,
            autohide_collapsed_appearance: CollapsedAppearance::CountStrip,
            mini_player_show_volume: true,
            mini_player_show_modes: true,
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
            strip_merged_mode: true,
            strip_click_action: StripClickAction::default(),
            strip_show_labels: true,
            strip_separator: StripSeparator::Slash,
            active_playlist_id: None,
            active_playlist_name: String::new(),
            active_playlist_comment: String::new(),
            active_playlist_duration: 0.0,
            active_playlist_updated: String::new(),
            active_playlist_public: false,
            active_playlist_song_count: 0,
            eq_enabled: false,
            eq_gains: default_eq_gains(),
            custom_eq_presets: Vec::new(),
            verbose_config: false,
            library_page_size: LibraryPageSize::default(),
            artwork_resolution: ArtworkResolution::default(),
            show_album_artists_only: default_true(),
            suppress_library_refresh_toasts: true,
            view_columns: ViewColumns::default(),
            albums_artwork_overlay: true,
            artists_artwork_overlay: true,
            songs_artwork_overlay: true,
            playlists_artwork_overlay: true,
            artwork_column_mode: ArtworkColumnMode::default(),
            artwork_column_stretch_fit: ArtworkStretchFit::default(),
            artwork_column_width_pct: default_artwork_column_width_pct(),
            artwork_auto_max_pct: default_artwork_auto_max_pct(),
            artwork_vertical_height_pct: default_artwork_vertical_height_pct(),
            show_tray_icon: false,
            close_to_tray: false,
            rating_reminder_enabled: false,
            rating_reminder_trigger: RatingReminderTrigger::default(),
            rating_reminder_percent: default_rating_reminder_percent(),
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
    pub player: PersistedPlayerSettings,
    #[serde(default)]
    pub views: ViewPreferences,
    #[serde(default)]
    pub hotkeys: HotkeyConfig,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_merged_mode_default_is_on() {
        let p = PersistedPlayerSettings::default();
        assert!(p.strip_merged_mode);
    }

    #[test]
    fn strip_merged_mode_roundtrips_through_serde() {
        let p = PersistedPlayerSettings {
            strip_merged_mode: false,
            ..PersistedPlayerSettings::default()
        };
        let json = serde_json::to_string(&p).expect("serialize");
        let parsed: PersistedPlayerSettings = serde_json::from_str(&json).expect("deserialize");
        assert!(!parsed.strip_merged_mode);
    }

    #[test]
    fn strip_merged_mode_missing_field_defaults_to_serde_false() {
        // Pre-feature redb rows have no `strip_merged_mode` key, so serde fills
        // it via the field-level `#[serde(default)]` — which resolves to
        // `bool::default()` (false), not the struct-level Default (true).
        // This pin keeps the serde-vs-struct distinction explicit for future
        // changes to the shipped default.
        let json = r#"{}"#;
        let parsed: PersistedPlayerSettings = serde_json::from_str(json).expect("deserialize");
        assert!(!parsed.strip_merged_mode);
    }

    #[test]
    fn replay_gain_prevent_clipping_defaults_to_true_for_missing_field() {
        let json = r#"{}"#;
        let parsed: PersistedPlayerSettings = serde_json::from_str(json).expect("deserialize");
        assert!(parsed.replay_gain_prevent_clipping);
    }

    /// True for the 50 per-view column-visibility keys (`<view>_show_<col>`).
    /// `queue_show_default_playlist` (the header chip) is NOT a column; the
    /// `strip_show_*` / `mini_player_show_*` / `*_artwork_overlay` toggles
    /// don't carry a view prefix and never match.
    fn is_view_column_key(key: &str) -> bool {
        const VIEW_PREFIXES: [&str; 7] = [
            "albums_show_",
            "artists_show_",
            "genres_show_",
            "playlists_show_",
            "similar_show_",
            "songs_show_",
            "queue_show_",
        ];
        key != "queue_show_default_playlist" && VIEW_PREFIXES.iter().any(|p| key.starts_with(p))
    }

    /// Wire-format pin: every per-view column toggle serializes as a
    /// TOP-LEVEL `<view>_show_<col>` key on the persisted JSON object —
    /// never nested under a `view_columns` map. Older redb rows store the
    /// flat shape; a nested shape would silently re-default every column on
    /// load. The ViewColumns composition must preserve this via
    /// `#[serde(flatten)]` — this test is intentionally written without any
    /// struct-field access so it survives that refactor byte-for-byte.
    #[test]
    fn persisted_column_keys_stay_flat_on_the_json_wire() {
        let v = serde_json::to_value(PersistedPlayerSettings::default()).expect("serialize");
        let obj = v.as_object().expect("JSON object");

        assert!(
            !obj.contains_key("view_columns"),
            "column toggles must flatten to top-level keys, not nest under view_columns"
        );
        let column_keys: Vec<&str> = obj
            .keys()
            .map(String::as_str)
            .filter(|k| is_view_column_key(k))
            .collect();
        assert_eq!(
            column_keys.len(),
            50,
            "expected the 50 per-view column keys at the top level, got {column_keys:?}"
        );
    }

    /// Wire-format pin (read direction): a flat top-level column key in a
    /// pre-refactor redb row must land on the matching toggle. Asserted via
    /// re-serialization (no struct-field access) so the test survives the
    /// ViewColumns composition unchanged.
    #[test]
    fn persisted_column_keys_deserialize_from_flat_json() {
        // albums_show_stars defaults to false — the flat key must flip it.
        let parsed: PersistedPlayerSettings =
            serde_json::from_str(r#"{"albums_show_stars": true}"#).expect("deserialize");
        let v = serde_json::to_value(parsed).expect("serialize");
        assert_eq!(
            v.get("albums_show_stars"),
            Some(&serde_json::Value::Bool(true)),
            "flat albums_show_stars key must land on the toggle"
        );
    }

    /// Wire-format pin (defaults): the serde fill value for a MISSING column
    /// key must equal the struct `Default` for every column toggle. Sparse
    /// redb rows written before a column existed must read back as the
    /// shipped default — if the two ever diverge, an upgrade silently flips
    /// columns for existing users.
    #[test]
    fn persisted_missing_column_keys_fill_from_struct_defaults() {
        let filled: PersistedPlayerSettings = serde_json::from_str("{}").expect("deserialize");
        let filled = serde_json::to_value(filled).expect("serialize filled");
        let defaults =
            serde_json::to_value(PersistedPlayerSettings::default()).expect("serialize defaults");

        let mut checked = 0_usize;
        for (key, default_value) in defaults.as_object().expect("object") {
            if is_view_column_key(key) {
                assert_eq!(
                    filled.get(key),
                    Some(default_value),
                    "serde fill for missing `{key}` diverges from the struct default"
                );
                checked += 1;
            }
        }
        assert_eq!(checked, 50, "expected to check all 50 column keys");
    }

    #[test]
    fn live_first_launch_overrides_agree_with_persisted_defaults() {
        // `Nokkvi::default()` (src/main.rs) derives `LivePlayerSettings` (all-zero)
        // but hand-restores exactly FIVE fields to non-zero first-launch values so
        // the pre-`PlayerSettingsLoaded` window matches the persisted shape. This
        // pins those five values against `PersistedPlayerSettings::default()`: if a
        // future "retune defaults" commit changes any persisted default here without
        // updating the hand-restored block in src/main.rs, this test fails loudly.
        //
        // The data crate cannot reference the UI-crate `Nokkvi` type, so it asserts
        // the persisted-side values directly; the UI-crate companion test
        // `nokkvi_default_overrides_match_persisted_defaults` closes the loop by
        // pinning `Nokkvi::default().settings` against these same persisted defaults.
        let p = PersistedPlayerSettings::default();

        assert!(
            p.scrobbling_enabled,
            "scrobbling_enabled must default to true"
        );
        assert!(
            (p.scrobble_threshold - 0.50).abs() < f64::EPSILON,
            "scrobble_threshold must default to 0.50"
        );
        assert_eq!(
            p.start_view, "Queue",
            "start_view must default to \"Queue\""
        );
        assert!(p.stable_viewport, "stable_viewport must default to true");
        assert!(
            p.auto_follow_playing,
            "auto_follow_playing must default to true"
        );
    }
}
