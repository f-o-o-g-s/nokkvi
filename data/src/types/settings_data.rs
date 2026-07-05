//! Per-tab settings data passed to the macro-emitted items builders.
//!
//! `define_settings!` emits one `build_<tab>_tab_settings_items(data: &<TabData>)`
//! helper per tab; the helper reads each entry's `ui_meta.read_field` closure
//! against the data struct here. UI-crate hand-written builders (in
//! `src/views/settings/items_*.rs`) construct one of these from live config
//! state and pass it to the helper.
//!
//! These types live in the data crate so `define_settings!` (also in the data
//! crate) can reference them at expansion time. They are iced-free —
//! `Cow<'static, str>` string slots, primitives, and `f64` only. The UI crate
//! re-exports each via `pub(crate) use nokkvi_data::types::settings_data::...;`
//! so existing import paths in the items modules keep resolving.
//!
//! String fields use `Cow<'static, str>` so the same struct accepts either a
//! `&'static str` literal (zero-cost `Cow::Borrowed` in test fixtures) or an
//! owned `String` (live config snapshot in production). `Default` is
//! implemented with a recognizable `"test-default"` sentinel so any path that
//! accidentally reads it in production is obvious.

use std::borrow::Cow;

/// Data needed by the General tab builder.
#[derive(Debug, Clone)]
pub struct GeneralSettingsData {
    pub server_url: Cow<'static, str>,
    pub username: Cow<'static, str>,
    pub start_view: Cow<'static, str>,
    pub stable_viewport: bool,
    pub auto_follow_playing: bool,
    pub enter_behavior: Cow<'static, str>,
    pub enter_shuffle: bool,
    pub local_music_path: Cow<'static, str>,
    /// Verbose-config mode label ("On" / "Off" / "Clean").
    pub verbose_config: Cow<'static, str>,
    pub library_page_size: Cow<'static, str>,
    pub artwork_resolution: Cow<'static, str>,
    pub show_album_artists_only: bool,
    pub suppress_library_refresh_toasts: bool,
    pub show_tray_icon: bool,
    pub close_to_tray: bool,
}

impl Default for GeneralSettingsData {
    fn default() -> Self {
        Self {
            server_url: Cow::Borrowed("test-default"),
            username: Cow::Borrowed("test-default"),
            start_view: Cow::Borrowed("test-default"),
            stable_viewport: false,
            auto_follow_playing: false,
            enter_behavior: Cow::Borrowed("test-default"),
            enter_shuffle: false,
            local_music_path: Cow::Borrowed("test-default"),
            verbose_config: Cow::Borrowed("test-default"),
            library_page_size: Cow::Borrowed("test-default"),
            artwork_resolution: Cow::Borrowed("test-default"),
            show_album_artists_only: false,
            suppress_library_refresh_toasts: false,
            show_tray_icon: false,
            close_to_tray: false,
        }
    }
}

/// Data needed by the Interface tab builder.
#[derive(Debug, Clone)]
pub struct InterfaceSettingsData {
    pub nav_layout: Cow<'static, str>,
    pub nav_display_mode: Cow<'static, str>,
    pub track_info_display: Cow<'static, str>,
    pub slot_row_height: Cow<'static, str>,
    pub horizontal_volume: bool,
    pub autohide_toolbar: bool,
    pub autohide_toolbar_height: i64,
    pub autohide_toolbar_grip: bool,
    pub autohide_collapsed_appearance: Cow<'static, str>,
    pub mini_player_show_volume: bool,
    pub mini_player_show_modes: bool,
    pub slot_text_links: bool,
    /// Scrollbar visibility label (On hover / Always / Hidden).
    pub scrollbar_visibility: Cow<'static, str>,
    /// Icon set label (Lucide / Phosphor).
    pub icon_set: Cow<'static, str>,
    pub font_family: Cow<'static, str>,
    pub strip_show_title: bool,
    pub strip_show_artist: bool,
    pub strip_show_album: bool,
    pub strip_show_format_info: bool,
    pub strip_merged_mode: bool,
    pub strip_show_labels: bool,
    pub strip_separator: Cow<'static, str>,
    pub strip_click_action: Cow<'static, str>,
    pub albums_artwork_overlay: bool,
    pub artists_artwork_overlay: bool,
    pub songs_artwork_overlay: bool,
    pub playlists_artwork_overlay: bool,
    /// Artwork column display mode label (Auto / Always (Native) / Always (Stretched) / Never)
    pub artwork_column_mode: Cow<'static, str>,
    /// Artwork column stretch fit label (Cover / Fill) — only consumed when mode is stretched.
    pub artwork_column_stretch_fit: Cow<'static, str>,
    /// Auto-mode max artwork fraction of the window's short axis (0.30..=0.70).
    pub artwork_auto_max_pct: f64,
    /// Always-Vertical artwork height as a fraction of window height (0.10..=0.80).
    pub artwork_vertical_height_pct: f64,
}

impl Default for InterfaceSettingsData {
    fn default() -> Self {
        Self {
            nav_layout: Cow::Borrowed("test-default"),
            nav_display_mode: Cow::Borrowed("test-default"),
            track_info_display: Cow::Borrowed("test-default"),
            slot_row_height: Cow::Borrowed("test-default"),
            horizontal_volume: false,
            autohide_toolbar: false,
            autohide_toolbar_height: 6,
            autohide_toolbar_grip: true,
            autohide_collapsed_appearance: Cow::Borrowed("test-default"),
            mini_player_show_volume: true,
            mini_player_show_modes: true,
            slot_text_links: false,
            scrollbar_visibility: Cow::Borrowed("test-default"),
            icon_set: Cow::Borrowed("test-default"),
            font_family: Cow::Borrowed("test-default"),
            strip_show_title: false,
            strip_show_artist: false,
            strip_show_album: false,
            strip_show_format_info: false,
            strip_merged_mode: false,
            strip_show_labels: false,
            strip_separator: Cow::Borrowed("test-default"),
            strip_click_action: Cow::Borrowed("test-default"),
            albums_artwork_overlay: false,
            artists_artwork_overlay: false,
            songs_artwork_overlay: false,
            playlists_artwork_overlay: false,
            artwork_column_mode: Cow::Borrowed("test-default"),
            artwork_column_stretch_fit: Cow::Borrowed("test-default"),
            artwork_auto_max_pct: 0.0,
            artwork_vertical_height_pct: 0.0,
        }
    }
}

/// Data needed by the Playback tab builder.
#[derive(Debug, Clone)]
pub struct PlaybackSettingsData {
    pub crossfade_enabled: bool,
    /// Bit-perfect mode label ("Off" / "Strict" / "Relaxed").
    pub bit_perfect: Cow<'static, str>,
    pub crossfade_duration_secs: i64,
    /// Crossfade curve label ("Equal Power" / "Constant Gain" / "Linear").
    pub crossfade_curve: Cow<'static, str>,
    /// Minimum track length (seconds) below which transitions play gapless.
    pub crossfade_min_track_secs: i64,
    /// Whether sequential same-album tracks skip the blend (album-continuity gate).
    pub crossfade_album_gapless: bool,
    /// Whether new non-bit-perfect streams get the ~20 ms de-click onset ramp.
    pub smooth_track_starts: bool,
    /// Whether pause/resume ramp the volume instead of cutting (opt-in).
    pub fade_on_pause: bool,
    /// Pause/resume ramp length in milliseconds (20–500).
    pub fade_pause_ms: i64,
    /// Whether stopping playback ramps the volume down instead of cutting (opt-in).
    pub fade_on_stop: bool,
    /// Stop ramp length in milliseconds (20–500).
    pub fade_stop_ms: i64,
    /// Whether radio↔queue switches fade out and back in instead of hard-cutting (opt-in).
    pub fade_radio_transitions: bool,
    /// "Fade on Skip" mode label ("Off" / "Boundary Fade" / "Crossfade").
    pub fade_on_skip: Cow<'static, str>,
    /// "Fade on Skip" length in seconds (1–4).
    pub fade_skip_secs: i64,
    /// Whether silent tails/lead-ins are skipped at track transitions (M8, opt-in).
    pub skip_silence: bool,
    /// Gap / overlap trim in seconds (−2..+2; negative = overlap, positive = gap).
    pub crossfade_offset_secs: i64,
    /// Whether the crossfade length snaps to whole bars of the outgoing BPM (M8, opt-in).
    pub crossfade_bar_snap: bool,
    /// Whether Previous restarts the current track past the threshold (default false).
    pub rewind_on_previous: bool,
    /// Volume-normalization mode label ("Off" / "AGC" / "ReplayGain (Track)" / "ReplayGain (Album)")
    pub volume_normalization: Cow<'static, str>,
    pub normalization_level: Cow<'static, str>,
    /// Pre-amp dB applied on top of resolved ReplayGain (rounded to int for UI).
    pub replay_gain_preamp_db: i64,
    /// Fallback dB for tracks with no ReplayGain tags.
    pub replay_gain_fallback_db: i64,
    /// Whether untagged tracks fall through to AGC.
    pub replay_gain_fallback_to_agc: bool,
    /// Whether the resolver clamps gain so peak·gain ≤ 1.0.
    pub replay_gain_prevent_clipping: bool,
    pub scrobbling_enabled: bool,
    /// Scrobble threshold as a fraction (0.25–0.90).
    pub scrobble_threshold: f64,
    /// Whether internet-radio scrobbling (direct to ListenBrainz) is enabled.
    pub radio_scrobbling_enabled: bool,
    /// Absolute seconds a radio track must play before it scrobbles.
    pub radio_scrobble_threshold_secs: i64,
    /// Whether radio now-playing updates are sent.
    pub radio_now_playing_enabled: bool,
    /// Which layer supplies each radio-scrobble credential (env / config.toml /
    /// redb / unset), so the settings rows show "Saved" vs "Set in config.toml"
    /// vs "Set via env var" — and a GUI clear can warn when a higher layer still
    /// shadows it.
    pub listenbrainz_source: crate::services::radio_scrobble::source::CredSource,
    pub lastfm_credentials_source: crate::services::radio_scrobble::source::CredSource,
    /// Linked Last.fm username, empty when not connected.
    pub lastfm_username: Cow<'static, str>,
    pub quick_add_to_playlist: bool,
    pub default_playlist_name: Cow<'static, str>,
    pub queue_show_default_playlist: bool,
    /// Whether the rate-this-track desktop reminder is enabled.
    pub rating_reminder_enabled: bool,
    /// Whether a desktop notification fires when a rating changes via a hotkey
    /// or the `nokkvi rate` IPC verb.
    pub rating_change_notification_enabled: bool,
    /// Whether a desktop notification fires when a track is loved/unloved via
    /// the love hotkey or the `nokkvi love` IPC verb.
    pub love_change_notification_enabled: bool,
    /// Reminder trigger label ("On Scrobble" / "Percentage Played").
    pub rating_reminder_trigger: Cow<'static, str>,
    /// Percent of the track played that fires the reminder (percentage mode).
    pub rating_reminder_percent: i64,
}

impl Default for PlaybackSettingsData {
    fn default() -> Self {
        Self {
            crossfade_enabled: false,
            bit_perfect: Cow::Borrowed("test-default"),
            crossfade_duration_secs: 0,
            crossfade_curve: Cow::Borrowed("test-default"),
            crossfade_min_track_secs: 0,
            crossfade_album_gapless: false,
            smooth_track_starts: false,
            fade_on_pause: false,
            fade_pause_ms: 0,
            fade_on_stop: false,
            fade_stop_ms: 0,
            fade_radio_transitions: false,
            fade_on_skip: Cow::Borrowed("test-default"),
            fade_skip_secs: 0,
            skip_silence: false,
            crossfade_offset_secs: 0,
            crossfade_bar_snap: false,
            rewind_on_previous: false,
            volume_normalization: Cow::Borrowed("test-default"),
            normalization_level: Cow::Borrowed("test-default"),
            replay_gain_preamp_db: 0,
            replay_gain_fallback_db: 0,
            replay_gain_fallback_to_agc: false,
            replay_gain_prevent_clipping: false,
            scrobbling_enabled: false,
            scrobble_threshold: 0.0,
            radio_scrobbling_enabled: false,
            radio_scrobble_threshold_secs: 0,
            radio_now_playing_enabled: false,
            listenbrainz_source: crate::services::radio_scrobble::source::CredSource::Unset,
            lastfm_credentials_source: crate::services::radio_scrobble::source::CredSource::Unset,
            lastfm_username: Cow::Borrowed(""),
            quick_add_to_playlist: false,
            default_playlist_name: Cow::Borrowed("test-default"),
            queue_show_default_playlist: false,
            rating_reminder_enabled: false,
            rating_change_notification_enabled: false,
            love_change_notification_enabled: false,
            rating_reminder_trigger: Cow::Borrowed("test-default"),
            rating_reminder_percent: 0,
        }
    }
}

/// Data needed by the Visualizer tab builder — one slot per macro-emitted
/// row (the `ui_meta.read_field` closures in
/// `settings_tables/visualizer.rs` read these). Populated from the live
/// [`VisualizerConfig`][crate::types::visualizer_config::VisualizerConfig]
/// via the `From<&VisualizerConfig>` impl; enum slots carry WIRE strings
/// (`as_wire_str`), matching the wire-keyed visualizer dropdowns.
#[derive(Debug, Clone)]
pub struct VisualizerSettingsData {
    pub height_percent: f64,
    pub opacity: f64,
    pub bloom: bool,
    pub bloom_intensity: f64,
    pub beat_reactivity: f64,
    pub crt: f64,
    pub noise_reduction: f64,
    pub lower_cutoff_freq: i64,
    pub higher_cutoff_freq: i64,
    pub auto_sensitivity: bool,
    pub bars_placement: Cow<'static, str>,
    pub waves: bool,
    pub waves_smoothing: i64,
    pub monstercat: f64,
    pub bars_max_bars: i64,
    pub bars_bar_width_min: i64,
    pub bars_bar_width_max: i64,
    pub bars_bar_spacing: i64,
    pub bars_border_width: i64,
    pub bars_led_bars: bool,
    pub bars_led_segment_height: i64,
    pub bars_gradient_mode: Cow<'static, str>,
    pub bars_gradient_orientation: Cow<'static, str>,
    pub bars_peak_gradient_mode: Cow<'static, str>,
    pub bars_peak_mode: Cow<'static, str>,
    pub bars_peak_hold_time: i64,
    pub bars_peak_fade_time: i64,
    pub bars_peak_fall_speed: i64,
    pub bars_peak_height_ratio: i64,
    pub bars_bar_depth_3d: i64,
    pub bars_flash_intensity: f64,
    pub bars_trails: f64,
    pub bars_echo: f64,
    pub lines_placement: Cow<'static, str>,
    pub lines_point_count: i64,
    pub lines_line_thickness: f64,
    pub lines_outline_thickness: f64,
    pub lines_outline_opacity: f64,
    pub lines_animation_speed: f64,
    pub lines_gradient_mode: Cow<'static, str>,
    pub lines_fill_opacity: f64,
    pub lines_glow_intensity: f64,
    pub lines_mirror: bool,
    pub lines_style: Cow<'static, str>,
    pub lines_boat: bool,
    pub lines_trails: f64,
    pub lines_echo: f64,
    pub scope_radius: f64,
    pub scope_sensitivity: f64,
    pub scope_point_count: i64,
    pub scope_line_thickness: f64,
    pub scope_fill_opacity: f64,
    pub scope_glow_intensity: f64,
    pub scope_outline_thickness: f64,
    pub scope_outline_opacity: f64,
    pub scope_gradient_mode: Cow<'static, str>,
    pub scope_animation_speed: f64,
    pub scope_style: Cow<'static, str>,
    pub scope_particles: bool,
    pub scope_particle_count: i64,
    pub scope_particle_speed: f64,
    pub scope_beam: bool,
    pub scope_trails: f64,
    pub scope_echo: f64,
}

impl Default for VisualizerSettingsData {
    fn default() -> Self {
        Self {
            height_percent: 0.0,
            opacity: 0.0,
            bloom: false,
            bloom_intensity: 0.0,
            beat_reactivity: 0.0,
            crt: 0.0,
            noise_reduction: 0.0,
            lower_cutoff_freq: 0,
            higher_cutoff_freq: 0,
            auto_sensitivity: false,
            bars_placement: Cow::Borrowed("test-default"),
            waves: false,
            waves_smoothing: 0,
            monstercat: 0.0,
            bars_max_bars: 0,
            bars_bar_width_min: 0,
            bars_bar_width_max: 0,
            bars_bar_spacing: 0,
            bars_border_width: 0,
            bars_led_bars: false,
            bars_led_segment_height: 0,
            bars_gradient_mode: Cow::Borrowed("test-default"),
            bars_gradient_orientation: Cow::Borrowed("test-default"),
            bars_peak_gradient_mode: Cow::Borrowed("test-default"),
            bars_peak_mode: Cow::Borrowed("test-default"),
            bars_peak_hold_time: 0,
            bars_peak_fade_time: 0,
            bars_peak_fall_speed: 0,
            bars_peak_height_ratio: 0,
            bars_bar_depth_3d: 0,
            bars_flash_intensity: 0.0,
            bars_trails: 0.0,
            bars_echo: 0.0,
            lines_placement: Cow::Borrowed("test-default"),
            lines_point_count: 0,
            lines_line_thickness: 0.0,
            lines_outline_thickness: 0.0,
            lines_outline_opacity: 0.0,
            lines_animation_speed: 0.0,
            lines_gradient_mode: Cow::Borrowed("test-default"),
            lines_fill_opacity: 0.0,
            lines_glow_intensity: 0.0,
            lines_mirror: false,
            lines_style: Cow::Borrowed("test-default"),
            lines_boat: false,
            lines_trails: 0.0,
            lines_echo: 0.0,
            scope_radius: 0.0,
            scope_sensitivity: 0.0,
            scope_point_count: 0,
            scope_line_thickness: 0.0,
            scope_fill_opacity: 0.0,
            scope_glow_intensity: 0.0,
            scope_outline_thickness: 0.0,
            scope_outline_opacity: 0.0,
            scope_gradient_mode: Cow::Borrowed("test-default"),
            scope_animation_speed: 0.0,
            scope_style: Cow::Borrowed("test-default"),
            scope_particles: false,
            scope_particle_count: 0,
            scope_particle_speed: 0.0,
            scope_beam: false,
            scope_trails: 0.0,
            scope_echo: 0.0,
        }
    }
}

impl From<&crate::types::visualizer_config::VisualizerConfig> for VisualizerSettingsData {
    fn from(c: &crate::types::visualizer_config::VisualizerConfig) -> Self {
        Self {
            height_percent: f64::from(c.height_percent),
            opacity: f64::from(c.opacity),
            bloom: c.bloom,
            bloom_intensity: f64::from(c.bloom_intensity),
            beat_reactivity: f64::from(c.beat_reactivity),
            crt: f64::from(c.crt),
            noise_reduction: c.noise_reduction,
            lower_cutoff_freq: c.lower_cutoff_freq as i64,
            higher_cutoff_freq: c.higher_cutoff_freq as i64,
            auto_sensitivity: c.auto_sensitivity,
            bars_placement: Cow::Borrowed(c.bars.placement.as_wire_str()),
            waves: c.waves,
            waves_smoothing: c.waves_smoothing as i64,
            monstercat: c.monstercat,
            bars_max_bars: c.bars.max_bars as i64,
            bars_bar_width_min: c.bars.bar_width_min as i64,
            bars_bar_width_max: c.bars.bar_width_max as i64,
            bars_bar_spacing: c.bars.bar_spacing as i64,
            bars_border_width: c.bars.border_width as i64,
            bars_led_bars: c.bars.led_bars,
            bars_led_segment_height: c.bars.led_segment_height as i64,
            bars_gradient_mode: Cow::Borrowed(c.bars.gradient_mode.as_wire_str()),
            bars_gradient_orientation: Cow::Borrowed(c.bars.gradient_orientation.as_wire_str()),
            bars_peak_gradient_mode: Cow::Borrowed(c.bars.peak_gradient_mode.as_wire_str()),
            bars_peak_mode: Cow::Borrowed(c.bars.peak_mode.as_wire_str()),
            bars_peak_hold_time: c.bars.peak_hold_time as i64,
            bars_peak_fade_time: c.bars.peak_fade_time as i64,
            bars_peak_fall_speed: c.bars.peak_fall_speed as i64,
            bars_peak_height_ratio: c.bars.peak_height_ratio as i64,
            bars_bar_depth_3d: c.bars.bar_depth_3d as i64,
            bars_flash_intensity: f64::from(c.bars.flash_intensity),
            bars_trails: f64::from(c.bars.trails),
            bars_echo: f64::from(c.bars.echo),
            lines_placement: Cow::Borrowed(c.lines.placement.as_wire_str()),
            lines_point_count: c.lines.point_count as i64,
            lines_line_thickness: f64::from(c.lines.line_thickness),
            lines_outline_thickness: f64::from(c.lines.outline_thickness),
            lines_outline_opacity: f64::from(c.lines.outline_opacity),
            lines_animation_speed: f64::from(c.lines.animation_speed),
            lines_gradient_mode: Cow::Borrowed(c.lines.gradient_mode.as_wire_str()),
            lines_fill_opacity: f64::from(c.lines.fill_opacity),
            lines_glow_intensity: f64::from(c.lines.glow_intensity),
            lines_mirror: c.lines.mirror,
            lines_style: Cow::Borrowed(c.lines.style.as_wire_str()),
            lines_boat: c.lines.boat,
            lines_trails: f64::from(c.lines.trails),
            lines_echo: f64::from(c.lines.echo),
            scope_radius: f64::from(c.scope.radius),
            scope_sensitivity: f64::from(c.scope.sensitivity),
            scope_point_count: c.scope.point_count as i64,
            scope_line_thickness: f64::from(c.scope.line_thickness),
            scope_fill_opacity: f64::from(c.scope.fill_opacity),
            scope_glow_intensity: f64::from(c.scope.glow_intensity),
            scope_outline_thickness: f64::from(c.scope.outline_thickness),
            scope_outline_opacity: f64::from(c.scope.outline_opacity),
            scope_gradient_mode: Cow::Borrowed(c.scope.gradient_mode.as_wire_str()),
            scope_animation_speed: f64::from(c.scope.animation_speed),
            scope_style: Cow::Borrowed(c.scope.style.as_wire_str()),
            scope_particles: c.scope.particles,
            scope_particle_count: c.scope.particle_count as i64,
            scope_particle_speed: f64::from(c.scope.particle_speed),
            scope_beam: c.scope.beam,
            scope_trails: f64::from(c.scope.trails),
            scope_echo: f64::from(c.scope.echo),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn general_settings_data_default_uses_test_sentinel() {
        let data = GeneralSettingsData::default();
        assert_eq!(data.server_url.as_ref(), "test-default");
        assert_eq!(data.username.as_ref(), "test-default");
        assert_eq!(data.start_view.as_ref(), "test-default");
        assert_eq!(data.enter_behavior.as_ref(), "test-default");
        assert_eq!(data.local_music_path.as_ref(), "test-default");
        assert_eq!(data.library_page_size.as_ref(), "test-default");
        assert_eq!(data.artwork_resolution.as_ref(), "test-default");
        assert_eq!(data.verbose_config.as_ref(), "test-default");
        assert!(!data.stable_viewport);
        assert!(!data.auto_follow_playing);
        assert!(!data.show_album_artists_only);
        assert!(!data.suppress_library_refresh_toasts);
        assert!(!data.show_tray_icon);
        assert!(!data.close_to_tray);
    }

    #[test]
    fn interface_settings_data_default_uses_test_sentinel() {
        let data = InterfaceSettingsData::default();
        assert_eq!(data.nav_layout.as_ref(), "test-default");
        assert_eq!(data.nav_display_mode.as_ref(), "test-default");
        assert_eq!(data.track_info_display.as_ref(), "test-default");
        assert_eq!(data.slot_row_height.as_ref(), "test-default");
        assert_eq!(data.font_family.as_ref(), "test-default");
        assert_eq!(data.strip_separator.as_ref(), "test-default");
        assert_eq!(data.strip_click_action.as_ref(), "test-default");
        assert_eq!(data.artwork_column_mode.as_ref(), "test-default");
        assert_eq!(data.artwork_column_stretch_fit.as_ref(), "test-default");
        assert_eq!(data.artwork_auto_max_pct, 0.0);
        assert_eq!(data.artwork_vertical_height_pct, 0.0);
        assert!(!data.horizontal_volume);
        assert!(!data.slot_text_links);
    }

    #[test]
    fn visualizer_settings_data_default_uses_sentinel() {
        let data = VisualizerSettingsData::default();
        assert_eq!(data.bars_gradient_mode.as_ref(), "test-default");
        assert_eq!(data.scope_style.as_ref(), "test-default");
        assert!(!data.auto_sensitivity);
        assert_eq!(data.noise_reduction, 0.0);
        assert_eq!(data.bars_max_bars, 0);
    }

    #[test]
    fn visualizer_settings_data_from_config_carries_wire_strings_and_casts() {
        let mut cfg = crate::types::visualizer_config::VisualizerConfig::default();
        cfg.bars.gradient_mode = crate::types::visualizer_config::BarsGradientMode::Static;
        cfg.bars.bar_width_min = 7.0; // f32 config field surfaced as an Int row
        cfg.noise_reduction = 0.42;
        let data = VisualizerSettingsData::from(&cfg);
        assert_eq!(data.bars_gradient_mode.as_ref(), "static");
        assert_eq!(data.bars_bar_width_min, 7);
        assert_eq!(data.noise_reduction, 0.42);
        assert!(data.bloom);
    }

    #[test]
    fn playback_settings_data_default_uses_test_sentinel() {
        let data = PlaybackSettingsData::default();
        assert_eq!(data.volume_normalization.as_ref(), "test-default");
        assert_eq!(data.normalization_level.as_ref(), "test-default");
        assert_eq!(data.default_playlist_name.as_ref(), "test-default");
        assert_eq!(data.crossfade_duration_secs, 0);
        assert_eq!(data.scrobble_threshold, 0.0);
        assert_eq!(data.replay_gain_preamp_db, 0);
        assert_eq!(data.replay_gain_fallback_db, 0);
        assert!(!data.crossfade_enabled);
        assert!(!data.rewind_on_previous);
        assert!(!data.replay_gain_fallback_to_agc);
        assert!(!data.replay_gain_prevent_clipping);
        assert!(!data.scrobbling_enabled);
        assert!(!data.quick_add_to_playlist);
        assert!(!data.queue_show_default_playlist);
    }
}
