//! TOML-serializable settings for the `[settings]` section of config.toml.
//!
//! Contains only user-facing preferences. High-frequency values (volume),
//! runtime state (queue, active playlist), and sensitive data (credentials)
//! remain in redb.

use serde::{Deserialize, Serialize, Serializer};

use crate::{
    audio::eq::CustomEqPreset,
    types::player_settings::{
        EnterBehavior, NavDisplayMode, NavLayout, NormalizationLevel, SlotRowHeight,
        StripClickAction, TrackInfoDisplay, VisualizationMode,
    },
};

/// Settings section in config.toml — user-facing preferences only.
///
/// All enum fields use their existing serde `rename_all` attributes, producing
/// clean snake_case/lowercase TOML values (e.g. `visualization_mode = "bars"`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TomlSettings {
    // -- Application --
    pub start_view: String,
    pub enter_behavior: EnterBehavior,
    pub local_music_path: String,
    /// When true, all settings (including defaults) are written to config.toml
    pub verbose_config: bool,

    // -- Behavior --
    pub stable_viewport: bool,
    pub auto_follow_playing: bool,

    // -- Interface --
    pub light_mode: bool,
    pub rounded_mode: bool,
    pub nav_layout: NavLayout,
    pub nav_display_mode: NavDisplayMode,
    pub track_info_display: TrackInfoDisplay,
    pub slot_row_height: SlotRowHeight,
    pub opacity_gradient: bool,
    pub horizontal_volume: bool,

    // -- Metadata Strip --
    pub strip_show_title: bool,
    pub strip_show_artist: bool,
    pub strip_show_album: bool,
    pub strip_show_format_info: bool,
    pub strip_click_action: StripClickAction,

    // -- Playback --
    pub crossfade_enabled: bool,
    pub crossfade_duration_secs: u32,
    pub volume_normalization: bool,
    pub normalization_level: NormalizationLevel,
    pub visualization_mode: VisualizationMode,
    pub sound_effects_enabled: bool,
    #[serde(serialize_with = "round_f32")]
    pub sfx_volume: f32,

    // -- Scrobbling --
    pub scrobbling_enabled: bool,
    #[serde(serialize_with = "round_f32")]
    pub scrobble_threshold: f32,

    // -- Playlists --
    pub quick_add_to_playlist: bool,

    // -- Equalizer --
    pub eq_enabled: bool,
    #[serde(serialize_with = "round_f32_array")]
    pub eq_gains: [f32; 10],
    pub custom_eq_presets: Vec<CustomEqPreset>,
}

/// Serialize an f32 rounded to 4 decimal places to avoid f32→f64 representation noise
/// (e.g. 0.8999999761581421 → 0.9).
fn round_f32<S: Serializer>(val: &f32, s: S) -> Result<S::Ok, S::Error> {
    let rounded = (f64::from(*val) * 10_000.0).round() / 10_000.0;
    s.serialize_f64(rounded)
}

/// Serialize an f32 array with each element rounded to 4 decimal places.
fn round_f32_array<S: Serializer, const N: usize>(arr: &[f32; N], s: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeSeq;
    let mut seq = s.serialize_seq(Some(N))?;
    for val in arr {
        let rounded = (f64::from(*val) * 10_000.0).round() / 10_000.0;
        seq.serialize_element(&rounded)?;
    }
    seq.end()
}

impl Default for TomlSettings {
    fn default() -> Self {
        Self {
            start_view: "Queue".to_string(),
            enter_behavior: EnterBehavior::default(),
            local_music_path: String::new(),
            verbose_config: false,
            stable_viewport: true,
            auto_follow_playing: true,
            light_mode: false,
            rounded_mode: false,
            nav_layout: NavLayout::default(),
            nav_display_mode: NavDisplayMode::default(),
            track_info_display: TrackInfoDisplay::default(),
            slot_row_height: SlotRowHeight::default(),
            opacity_gradient: true,
            horizontal_volume: false,
            strip_show_title: true,
            strip_show_artist: true,
            strip_show_album: true,
            strip_show_format_info: true,
            strip_click_action: StripClickAction::default(),
            crossfade_enabled: false,
            crossfade_duration_secs: 5,
            volume_normalization: false,
            normalization_level: NormalizationLevel::default(),
            visualization_mode: VisualizationMode::default(),
            sound_effects_enabled: true,
            sfx_volume: 0.68,
            scrobbling_enabled: true,
            scrobble_threshold: 0.50,
            quick_add_to_playlist: false,
            eq_enabled: false,
            eq_gains: [0.0; 10],
            custom_eq_presets: Vec::new(),
        }
    }
}

impl TomlSettings {
    /// Build a `TomlSettings` from a `PlayerSettings` (for migration from redb).
    pub fn from_player_settings(ps: &crate::types::player_settings::PlayerSettings) -> Self {
        Self {
            start_view: ps.start_view.clone(),
            enter_behavior: ps.enter_behavior,
            local_music_path: ps.local_music_path.clone(),
            verbose_config: ps.verbose_config,
            stable_viewport: ps.stable_viewport,
            auto_follow_playing: ps.auto_follow_playing,
            light_mode: false, // Will be read from theme.light_mode or fresh default
            rounded_mode: ps.rounded_mode,
            nav_layout: ps.nav_layout,
            nav_display_mode: ps.nav_display_mode,
            track_info_display: ps.track_info_display,
            slot_row_height: ps.slot_row_height,
            opacity_gradient: ps.opacity_gradient,
            horizontal_volume: ps.horizontal_volume,
            strip_show_title: ps.strip_show_title,
            strip_show_artist: ps.strip_show_artist,
            strip_show_album: ps.strip_show_album,
            strip_show_format_info: ps.strip_show_format_info,
            strip_click_action: ps.strip_click_action,
            crossfade_enabled: ps.crossfade_enabled,
            crossfade_duration_secs: ps.crossfade_duration_secs,
            volume_normalization: ps.volume_normalization,
            normalization_level: ps.normalization_level,
            visualization_mode: ps.visualization_mode,
            sound_effects_enabled: ps.sound_effects_enabled,
            sfx_volume: ps.sfx_volume,
            scrobbling_enabled: ps.scrobbling_enabled,
            scrobble_threshold: ps.scrobble_threshold,
            quick_add_to_playlist: ps.quick_add_to_playlist,
            eq_enabled: ps.eq_enabled,
            eq_gains: ps.eq_gains,
            custom_eq_presets: ps.custom_eq_presets.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_roundtrip() {
        let settings = TomlSettings::default();
        let toml_str = toml::to_string_pretty(&settings).expect("serialize");
        let parsed: TomlSettings = toml::from_str(&toml_str).expect("deserialize");
        // Spot-check key fields
        assert_eq!(parsed.start_view, "Queue");
        assert_eq!(parsed.crossfade_duration_secs, 5);
        assert!(parsed.scrobbling_enabled);
        assert_eq!(parsed.eq_gains, [0.0; 10]);
    }

    #[test]
    fn toml_enum_serialization_format() {
        let settings = TomlSettings::default();
        let toml_str = toml::to_string_pretty(&settings).expect("serialize");
        // Verify enums serialize to readable TOML values
        assert!(
            toml_str.contains("visualization_mode = \"bars\""),
            "Expected bars, got:\n{toml_str}"
        );
        assert!(
            toml_str.contains("enter_behavior = \"play_all\""),
            "Expected play_all, got:\n{toml_str}"
        );
        assert!(
            toml_str.contains("nav_layout = \"top\""),
            "Expected top, got:\n{toml_str}"
        );
        assert!(
            toml_str.contains("strip_click_action = \"go_to_queue\""),
            "Expected go_to_queue, got:\n{toml_str}"
        );
    }

    #[test]
    fn toml_deserializes_with_missing_fields() {
        // Minimal TOML — all other fields should use defaults
        let minimal = r#"
            start_view = "Albums"
        "#;
        let parsed: TomlSettings = toml::from_str(minimal).expect("deserialize");
        assert_eq!(parsed.start_view, "Albums");
        assert!(parsed.stable_viewport); // default
        assert_eq!(parsed.crossfade_duration_secs, 5); // default
    }
}
