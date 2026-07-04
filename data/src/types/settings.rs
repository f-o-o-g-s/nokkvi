use serde::{Deserialize, Serialize};

use crate::{
    audio::eq::EQ_BAND_COUNT,
    types::{
        hotkey_config::HotkeyConfig,
        player_settings::{
            ArtworkColumnMode, ArtworkResolution, ArtworkStretchFit, BitPerfectMode,
            CollapsedAppearance, CrossfadeCurve, EnterBehavior, FadeOnSkip, IconSet,
            LibraryPageSize, NavDisplayMode, NavLayout, NormalizationLevel, RatingReminderTrigger,
            RoundedMode, ScrollbarVisibility, SlotRowHeight, StripClickAction, StripSeparator,
            TrackInfoDisplay, VerboseConfig, VisualizationMode, VolumeNormalizationMode,
            deserialize_bit_perfect_with_bool_compat, deserialize_rounded_mode_with_bool_compat,
            deserialize_verbose_config_with_bool_compat,
        },
        queue::{QueueSortPreferences, SortPreferences},
        queue_sort_mode::QueueSortMode,
        sort_mode::SortMode,
        view_columns::ViewColumns,
    },
};

// The PersistedPlayerSettings / LivePlayerSettings twins are emitted from
// this ONE field table (M4). Row order == redb serde_json emission order —
// pinned by the golden-bytes tests; see player_settings_schema.rs for the
// full contract. `crate::types::player_settings` re-exports the Live twin.
crate::player_settings_schema! {
    #[serde(default = "default_volume")]
    split volume: f64 | f32 = default_volume(),
    #[serde(default = "default_sfx_volume")]
    split sfx_volume: f64 | f32 = default_sfx_volume(),
    #[serde(default = "default_sound_effects_enabled")]
    same sound_effects_enabled: bool = default_sound_effects_enabled(),
    #[serde(default)]
    same visualization_mode: VisualizationMode = VisualizationMode::default(),
    #[serde(default)]
    persist_only light_mode: bool = false,
    /// Whether scrobbling is enabled (default: true)
    #[serde(default = "default_scrobbling_enabled")]
    same scrobbling_enabled: bool = default_scrobbling_enabled(),
    /// Scrobble threshold as a fraction of track duration (0.25–0.90, default 0.50)
    #[serde(default = "default_scrobble_threshold")]
    split scrobble_threshold: f64 | f32 = default_scrobble_threshold(),
    /// Whether internet-radio tracks are scrobbled directly to ListenBrainz
    /// (default: false — opt-in, requires a configured token)
    #[serde(default)]
    same radio_scrobbling_enabled: bool = false,
    /// Absolute seconds a radio track must play before it scrobbles
    /// (radio has no duration; default 60)
    #[serde(default = "default_radio_scrobble_threshold_secs")]
    same radio_scrobble_threshold_secs: u32 = default_radio_scrobble_threshold_secs(),
    /// Whether to send radio now-playing updates on each ICY track change
    /// (default: true)
    #[serde(default = "default_radio_now_playing_enabled")]
    same radio_now_playing_enabled: bool = default_radio_now_playing_enabled(),
    /// Start view name ("Queue", "Albums", etc. — default "Queue")
    #[serde(default = "default_start_view")]
    same start_view: String = default_start_view(),
    /// Stable viewport mode (default: true)
    /// When enabled, clicking items highlights in-place without scrolling,
    /// and playback changes don't auto-scroll the viewport.
    #[serde(default = "default_stable_viewport")]
    same stable_viewport: bool = default_stable_viewport(),
    /// Auto-follow playing track (default: true)
    /// When enabled, the queue view auto-scrolls to the currently playing
    /// track on track changes and queue reload.
    #[serde(default = "default_auto_follow_playing")]
    same auto_follow_playing: bool = default_auto_follow_playing(),
    /// What Enter does when activating items (default: PlayAll)
    #[serde(default)]
    same enter_behavior: EnterBehavior = EnterBehavior::default(),
    /// Whether plain Enter/click layers a one-shot Shuffle Play on top of
    /// `enter_behavior` (default: false). Distinct from the persistent shuffle
    /// MODE — it never writes `queue.shuffle`.
    #[serde(default)]
    same enter_shuffle: bool = false,
    /// Local filesystem prefix for the music library (default: empty = not configured).
    /// Joined with the server-relative song path to form an absolute local path.
    /// e.g. "/music/Library" for local Navidrome, or "/mnt/nas/music" for NFS mounts.
    #[serde(default)]
    same local_music_path: String = String::new(),
    /// Rounded corners mode (default: `On`).
    ///
    /// Field-level shim accepts legacy bool values (`true` → `On`,
    /// `false` → `Off`) for configs written before the tri-state migration.
    #[serde( default, deserialize_with = "deserialize_rounded_mode_with_bool_compat" )]
    same rounded_mode: RoundedMode = RoundedMode::On,
    /// Navigation layout mode (default: Top = horizontal bar)
    #[serde(default)]
    same nav_layout: NavLayout = NavLayout::default(),
    /// Navigation display mode (default: TextOnly)
    #[serde(default)]
    same nav_display_mode: NavDisplayMode = NavDisplayMode::default(),
    /// Track info display mode (off / player bar / top bar)
    #[serde(default)]
    same track_info_display: TrackInfoDisplay = TrackInfoDisplay::MiniPlayer,
    /// Slot list row density (default: Default = 70px)
    #[serde(default)]
    same slot_row_height: SlotRowHeight = SlotRowHeight::Compact,
    /// Whether the opacity gradient on non-center slots is enabled (default: false)
    #[serde(default = "default_opacity_gradient")]
    same opacity_gradient: bool = default_opacity_gradient(),
    /// Whether clickable text links in slot list items are enabled (default: false)
    #[serde(default)]
    same slot_text_links: bool = false,
    /// How the slot-list scrollbar is shown (default `Always` — a permanent
    /// gutter track). `OnHover` is the transient fade handle; `Hidden` removes
    /// the bar entirely.
    #[serde(default)]
    same scrollbar_visibility: ScrollbarVisibility = ScrollbarVisibility::default(),
    /// Which icon family the UI renders (default `Phosphor`). Missing keys fill
    /// from `IconSet::default()` (Phosphor), so configs without the key adopt
    /// Phosphor on upgrade; pick `Lucide` to keep the original outline set.
    #[serde(default)]
    same icon_set: IconSet = IconSet::default(),
    /// Whether crossfade between tracks is enabled (default: true)
    #[serde(default = "default_true")]
    same crossfade_enabled: bool = true,
    /// Bit-perfect output mode (Off / Strict / Relaxed): Strict and Relaxed
    /// play at each track's native sample rate with the DSP chain (EQ / software
    /// volume / limiter) bypassed, letting PipeWire switch the device clock.
    /// They differ only on same-rate crossfades. Off by default (opt-in).
    /// Legacy bool records load via the compat shim (true → Strict, false → Off).
    #[serde(default, deserialize_with = "deserialize_bit_perfect_with_bool_compat")]
    same bit_perfect: BitPerfectMode = BitPerfectMode::default(),
    /// Crossfade duration in seconds (1–12, default 7)
    #[serde(default = "default_crossfade_duration_secs")]
    same crossfade_duration_secs: u32 = default_crossfade_duration_secs(),
    /// Crossfade gain curve (default Equal Power — flat loudness through the
    /// blend for uncorrelated tracks; Constant Gain is the historical
    /// cos²/sin² pair; Linear is a plain ramp)
    #[serde(default)]
    same crossfade_curve: CrossfadeCurve = CrossfadeCurve::default(),
    /// Minimum track length in seconds for crossfade eligibility (0–60,
    /// default 10 — the historical hardcoded floor). Shorter tracks play
    /// gapless; 0 blends everything with a known duration.
    #[serde(default = "default_crossfade_min_track_secs")]
    same crossfade_min_track_secs: u32 = default_crossfade_min_track_secs(),
    /// Album-continuity gate (default false — opt-in): skip the blend when
    /// the next track continues the same album sequentially, so authored
    /// gapless segues stay tight. Crossfade still applies between different
    /// albums, on shuffle, across disc boundaries, and on compilations.
    #[serde(default)]
    same crossfade_album_gapless: bool = false,
    /// Whether new non-bit-perfect streams ramp up their first ~20 ms (the
    /// M2 de-click onset ramp; default true). Off restores an instant,
    /// honest onset. Bit-perfect streams never ramp regardless.
    #[serde(default = "default_true")]
    same smooth_track_starts: bool = true,
    /// Whether pause/resume ramp the volume over `fade_pause_ms` instead of
    /// cutting mid-waveform (default false — opt-in).
    #[serde(default)]
    same fade_on_pause: bool = false,
    /// Pause/resume ramp length in milliseconds (20–500, default 100).
    #[serde(default = "default_transport_fade_ms")]
    same fade_pause_ms: u32 = default_transport_fade_ms(),
    /// Whether stopping playback ramps the volume down over `fade_stop_ms`
    /// instead of cutting (default false — opt-in). Applies to user stops,
    /// not track changes.
    #[serde(default)]
    same fade_on_stop: bool = false,
    /// Stop ramp length in milliseconds (20–500, default 100).
    #[serde(default = "default_transport_fade_ms")]
    same fade_stop_ms: u32 = default_transport_fade_ms(),
    /// Whether radio↔queue switches fade out and back in (~250 ms each way)
    /// instead of hard-cutting (default false — opt-in). The incoming fade
    /// waits for the stream's first real audio.
    #[serde(default)]
    same fade_radio_transitions: bool = false,
    /// What a manual skip — Next/Previous or clicking a track — does to the
    /// sound (default Off — the historical instant cut): Boundary Fade eases
    /// the outgoing track out before the hard load; Crossfade overlaps and
    /// blends into the picked track (M7/M10).
    #[serde(default)]
    same fade_on_skip: FadeOnSkip = FadeOnSkip::default(),
    /// "Fade on Skip" length in seconds (1–4, default 2) — the skip-crossfade
    /// overlap and the Boundary Fade ease-out share it.
    #[serde(default = "default_fade_skip_secs")]
    same fade_skip_secs: u32 = default_fade_skip_secs(),
    /// M8 "Skip Silence Between Tracks" (default false — opt-in): a silent
    /// outgoing tail triggers the next transition early, and a silent lead-in
    /// is dropped from prepared transition decoders. Off plays every recorded
    /// second; bit-perfect streams never trim.
    #[serde(default)]
    same skip_silence: bool = false,
    /// M8 "Gap / Overlap Trim" in seconds (−2..+2, default 0): negative
    /// starts the crossfade early (trims the outgoing tail into the blend);
    /// positive holds that much silence between tracks on gapless joins.
    #[serde(default)]
    same crossfade_offset_secs: i32 = 0,
    /// M8 "Snap Crossfade to Musical Bars" (default false — opt-in): round
    /// the crossfade length to whole 4/4 bars of the outgoing track's BPM tag
    /// so beats line up through the blend. Untagged tracks are unaffected.
    #[serde(default)]
    same crossfade_bar_snap: bool = false,
    /// Whether the Previous button restarts the current track once it has
    /// played past the threshold (default false).
    #[serde(default)]
    same rewind_on_previous: bool = false,
    /// Default playlist ID for quick-add (None = no default)
    #[serde(default)]
    same default_playlist_id: Option<String> = None,
    /// Default playlist display name (for settings UI)
    #[serde(default)]
    same default_playlist_name: String = String::new(),
    /// Whether to skip the Add to Playlist dialog and use the default playlist directly
    #[serde(default)]
    same quick_add_to_playlist: bool = false,
    /// Whether the queue view's header shows the default playlist chip (default: false)
    #[serde(default)]
    same queue_show_default_playlist: bool = false,
    /// Whether volume sliders in the player bar are horizontal (default: false = vertical)
    #[serde(default)]
    same horizontal_volume: bool = false,
    /// Whether the view-header toolbar auto-hides to a thin line until hovered
    /// or a sort/search shortcut is used (default: true)
    #[serde(default)]
    same autohide_toolbar: bool = true,
    /// Collapsed auto-hide toolbar height in px (default: 4)
    #[serde(default = "default_autohide_toolbar_height")]
    same autohide_toolbar_height: u32 = default_autohide_toolbar_height(),
    /// Whether the collapsed auto-hide toolbar shows a centered accent grip bar (default: true)
    #[serde(default = "default_true")]
    same autohide_toolbar_grip: bool = true,
    /// What the collapsed auto-hide toolbar shows (default: Count strip)
    #[serde(default)]
    same autohide_collapsed_appearance: CollapsedAppearance = CollapsedAppearance::CountStrip,
    /// Whether the mini-player bar shows the volume slider (default: true).
    /// Only affects `TrackInfoDisplay::MiniPlayer`.
    #[serde(default = "default_true")]
    same mini_player_show_volume: bool = true,
    /// Whether the mini-player bar shows the modes menu (default: true).
    /// Only affects `TrackInfoDisplay::MiniPlayer`.
    #[serde(default = "default_true")]
    same mini_player_show_modes: bool = true,
    /// Font family override (default: empty = system default sans-serif)
    #[serde(default)]
    same font_family: String = String::new(),
    /// Volume normalization mode (default: Off). On-disk key is
    /// `volume_normalization_mode`.
    #[serde(default, rename = "volume_normalization_mode")]
    same volume_normalization: VolumeNormalizationMode = VolumeNormalizationMode::default(),
    /// AGC target level (default: Normal). Only meaningful when
    /// `volume_normalization == Agc`.
    #[serde(default)]
    same normalization_level: NormalizationLevel = NormalizationLevel::default(),
    /// Pre-amp dB applied on top of resolved ReplayGain (default 0.0).
    #[serde(default)]
    same replay_gain_preamp_db: f32 = 0.0,
    /// Fallback dB for tracks with no ReplayGain tags (default 0.0 = unity).
    #[serde(default)]
    same replay_gain_fallback_db: f32 = 0.0,
    /// When true, untagged tracks fall through to AGC.
    #[serde(default)]
    same replay_gain_fallback_to_agc: bool = false,
    /// When true, clamp gain so `peak * gain <= 1.0` (default true).
    #[serde(default = "default_true")]
    same replay_gain_prevent_clipping: bool = true,
    /// Whether the title field is visible in the track info strip (default: true)
    #[serde(default = "default_true")]
    same strip_show_title: bool = true,
    /// Whether the artist field is visible in the track info strip (default: true)
    #[serde(default = "default_true")]
    same strip_show_artist: bool = true,
    /// Whether the album field is visible in the track info strip (default: true)
    #[serde(default = "default_true")]
    same strip_show_album: bool = true,
    /// Whether format info (codec/kHz/kbps) is visible in the track info strip (default: true)
    #[serde(default = "default_true")]
    same strip_show_format_info: bool = true,
    /// Whether the metastrip renders artist/album/title as a single shared
    /// scrolling unit with one set of bookend separators (default: true).
    #[serde(default)]
    same strip_merged_mode: bool = true,
    /// What happens when clicking the track info strip (default: GoToQueue)
    #[serde(default)]
    same strip_click_action: StripClickAction = StripClickAction::default(),
    /// Whether `title:` / `artist:` / `album:` labels are prepended to each
    /// field in the metadata strip (default: true).
    #[serde(default = "default_true")]
    same strip_show_labels: bool = true,
    /// Visual character used to join visible fields in merged-mode rendering
    /// (default: Slash /).
    #[serde(default)]
    same strip_separator: StripSeparator = StripSeparator::Slash,
    /// Active playlist ID loaded in the queue (None = no playlist context)
    #[serde(default)]
    same active_playlist_id: Option<String> = None,
    /// Active playlist display name
    #[serde(default)]
    same active_playlist_name: String = String::new(),
    /// Active playlist comment/description
    #[serde(default)]
    same active_playlist_comment: String = String::new(),
    /// Active playlist total duration in seconds (0.0 when unknown).
    #[serde(default)]
    same active_playlist_duration: f32 = 0.0,
    /// Active playlist last-updated timestamp (raw ISO-8601; empty when unknown).
    #[serde(default)]
    same active_playlist_updated: String = String::new(),
    /// Active playlist public/private visibility (drives the strip lock chip).
    #[serde(default)]
    same active_playlist_public: bool = false,
    /// Active playlist song count (0 when unknown; strip falls back to queue length).
    #[serde(default)]
    same active_playlist_song_count: u32 = 0,
    /// Whether the 10-band graphic EQ is enabled (master bypass).
    #[serde(default)]
    same eq_enabled: bool = false,
    /// Per-band EQ gain values in dB (-12.0 to +12.0). Indexed by band.
    #[serde(default = "default_eq_gains")]
    same eq_gains: [f32; EQ_BAND_COUNT] = default_eq_gains(),
    /// User-created custom EQ presets.
    #[serde(default)]
    same custom_eq_presets: Vec<crate::audio::eq::CustomEqPreset> = Vec::new(),
    /// How config.toml is written (full / sparse-with-comments / sparse-clean).
    /// Legacy bool records load via the compat shim (`true` → On, `false` → Off).
    #[serde( default, deserialize_with = "deserialize_verbose_config_with_bool_compat" )]
    same verbose_config: VerboseConfig = VerboseConfig::default(),
    /// Library page size controls how many items are fetched at once.
    #[serde(default)]
    same library_page_size: LibraryPageSize = LibraryPageSize::default(),
    /// Artwork resolution for the large panel (Default / High / Ultra / Original)
    #[serde(default)]
    same artwork_resolution: ArtworkResolution = ArtworkResolution::default(),
    /// Whether the Artists view shows only album artists
    #[serde(default = "default_true")]
    same show_album_artists_only: bool = default_true(),
    /// Whether to suppress the toast notification shown when Navidrome
    /// emits a library-refresh event (default: true = toasts suppressed).
    #[serde(default)]
    same suppress_library_refresh_toasts: bool = true,
    /// Per-view column-visibility toggles — flattened so every
    /// `<view>_show_<col>` key stays a TOP-LEVEL key on the persisted JSON
    /// wire (pinned by `persisted_column_keys_stay_flat_on_the_json_wire`).
    /// Missing keys fill from `ViewColumns::default()` — the single source
    /// of truth for the shipped column defaults.
    #[serde(flatten)]
    same view_columns: ViewColumns = ViewColumns::default(),
    /// Whether the metadata text overlay is rendered on the large artwork in Albums view.
    #[serde(default = "default_true")]
    same albums_artwork_overlay: bool = true,
    /// Whether the metadata text overlay is rendered on the large artwork in Artists view.
    #[serde(default = "default_true")]
    same artists_artwork_overlay: bool = true,
    /// Whether the metadata text overlay is rendered on the large artwork in Songs view.
    #[serde(default = "default_true")]
    same songs_artwork_overlay: bool = true,
    /// Whether the metadata text overlay is rendered on the large artwork in Playlists view.
    #[serde(default = "default_true")]
    same playlists_artwork_overlay: bool = true,
    /// Display mode for the large artwork column (auto / always-native / always-stretched / never).
    #[serde(default)]
    same artwork_column_mode: ArtworkColumnMode = ArtworkColumnMode::default(),
    /// Fit mode used when `artwork_column_mode == AlwaysStretched`.
    #[serde(default)]
    same artwork_column_stretch_fit: ArtworkStretchFit = ArtworkStretchFit::default(),
    /// Artwork column width as a fraction of window width (0.05..=0.80).
    /// Only consulted in always modes.
    #[serde(default = "default_artwork_column_width_pct")]
    same artwork_column_width_pct: f32 = default_artwork_column_width_pct(),
    /// Auto-mode max artwork size as a fraction of the window's short axis
    /// (0.30..=0.70). Default 0.40. The Auto resolver uses this for both the
    /// horizontal candidate and the portrait-fallback vertical candidate.
    #[serde(default = "default_artwork_auto_max_pct")]
    same artwork_auto_max_pct: f32 = default_artwork_auto_max_pct(),
    /// Always-Vertical artwork height as a fraction of window height
    /// (0.10..=0.80). Default 0.40. Consulted by the AlwaysVerticalNative /
    /// AlwaysVerticalStretched resolver branches.
    #[serde(default = "default_artwork_vertical_height_pct")]
    same artwork_vertical_height_pct: f32 = default_artwork_vertical_height_pct(),
    /// Whether to register a system tray (StatusNotifierItem) icon.
    /// Requires the compositor to host an SNI tray (e.g. waybar with the
    /// `tray` module on Hyprland; AppIndicator extension on GNOME).
    #[serde(default)]
    same show_tray_icon: bool = false,
    /// When true and `show_tray_icon` is on, pressing the window's close button
    /// hides the window into the tray instead of quitting the app.
    #[serde(default)]
    same close_to_tray: bool = false,
    /// Whether the rate-this-track desktop notification is enabled (default false).
    #[serde(default)]
    same rating_reminder_enabled: bool = false,
    /// Whether a desktop notification fires when a rating changes via a hotkey
    /// or the `nokkvi rate` IPC verb (default false).
    #[serde(default)]
    same rating_change_notification_enabled: bool = false,
    /// When the rating reminder fires (default: on scrobble).
    #[serde(default)]
    same rating_reminder_trigger: RatingReminderTrigger = RatingReminderTrigger::default(),
    /// Percent of track played that fires the reminder in percentage mode
    /// (default 75; UI clamp 60–90).
    #[serde(default = "default_rating_reminder_percent")]
    same rating_reminder_percent: u32 = default_rating_reminder_percent(),
    /// Visualizer behavior config — sourced from the in-memory
    /// `SettingsManager.visualizer` field (config.toml `[visualizer]`-only;
    /// NEVER redb). `PersistedPlayerSettings` deliberately has no twin field.
    live_only visualizer: crate::types::visualizer_config::VisualizerConfig,
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
fn default_radio_scrobble_threshold_secs() -> u32 {
    60
}
/// Bounds for the radio listen threshold (seconds). Enforced identically on the
/// settings setter AND the config.toml load path (so a hand-edited config can't
/// bypass them), and mirror the slider min/max in the playback settings table.
pub(crate) const RADIO_SCROBBLE_THRESHOLD_MIN: u32 = 20;
pub(crate) const RADIO_SCROBBLE_THRESHOLD_MAX: u32 = 240;
fn default_radio_now_playing_enabled() -> bool {
    true
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
fn default_crossfade_min_track_secs() -> u32 {
    crate::types::player_settings::CROSSFADE_MIN_TRACK_DEFAULT_SECS
}
fn default_transport_fade_ms() -> u32 {
    crate::types::player_settings::TRANSPORT_FADE_MS_DEFAULT
}
fn default_fade_skip_secs() -> u32 {
    crate::types::player_settings::FADE_SKIP_SECS_DEFAULT
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
