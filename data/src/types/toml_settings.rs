//! TOML-serializable settings for the `[settings]` section of config.toml.
//!
//! Contains only user-facing preferences. High-frequency values (volume),
//! runtime state (queue, active playlist), and sensitive data (credentials)
//! remain in redb.

use serde::{Deserialize, Serialize, Serializer};

use crate::{
    audio::eq::CustomEqPreset,
    types::player_settings::{
        ArtworkColumnMode, ArtworkResolution, ArtworkStretchFit, EnterBehavior, LibraryPageSize,
        NavDisplayMode, NavLayout, NormalizationLevel, SlotRowHeight, StripClickAction,
        TrackInfoDisplay, VisualizationMode, VolumeNormalizationMode,
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
    pub library_page_size: LibraryPageSize,
    pub artwork_resolution: ArtworkResolution,
    pub show_album_artists_only: bool,
    pub suppress_library_refresh_toasts: bool,
    pub queue_show_stars: bool,
    pub queue_show_album: bool,
    pub queue_show_duration: bool,
    pub queue_show_love: bool,
    #[serde(default)]
    pub queue_show_plays: bool,
    #[serde(default = "default_true")]
    pub queue_show_index: bool,
    #[serde(default = "default_true")]
    pub queue_show_thumbnail: bool,
    #[serde(default)]
    pub queue_show_genre: bool,
    #[serde(default)]
    pub queue_show_select: bool,

    pub albums_show_stars: bool,
    pub albums_show_songcount: bool,
    pub albums_show_plays: bool,
    pub albums_show_love: bool,
    #[serde(default = "default_true")]
    pub albums_show_index: bool,
    #[serde(default = "default_true")]
    pub albums_show_thumbnail: bool,
    #[serde(default)]
    pub albums_show_select: bool,

    pub songs_show_stars: bool,
    pub songs_show_album: bool,
    pub songs_show_duration: bool,
    pub songs_show_plays: bool,
    pub songs_show_love: bool,
    #[serde(default = "default_true")]
    pub songs_show_index: bool,
    #[serde(default = "default_true")]
    pub songs_show_thumbnail: bool,
    #[serde(default)]
    pub songs_show_genre: bool,
    #[serde(default)]
    pub songs_show_select: bool,

    pub artists_show_stars: bool,
    pub artists_show_albumcount: bool,
    pub artists_show_songcount: bool,
    pub artists_show_plays: bool,
    pub artists_show_love: bool,
    #[serde(default = "default_true")]
    pub artists_show_index: bool,
    #[serde(default = "default_true")]
    pub artists_show_thumbnail: bool,
    #[serde(default)]
    pub artists_show_select: bool,

    // -- Genres view column toggles --
    #[serde(default = "default_true")]
    pub genres_show_index: bool,
    #[serde(default = "default_true")]
    pub genres_show_thumbnail: bool,
    #[serde(default = "default_true")]
    pub genres_show_albumcount: bool,
    #[serde(default = "default_true")]
    pub genres_show_songcount: bool,
    #[serde(default)]
    pub genres_show_select: bool,

    // -- Playlists view column toggles --
    #[serde(default = "default_true")]
    pub playlists_show_index: bool,
    #[serde(default = "default_true")]
    pub playlists_show_thumbnail: bool,
    #[serde(default)]
    pub playlists_show_songcount: bool,
    #[serde(default)]
    pub playlists_show_duration: bool,
    #[serde(default)]
    pub playlists_show_updatedat: bool,
    #[serde(default)]
    pub playlists_show_select: bool,

    // -- Similar view column toggles (Find Similar / Top Songs results) --
    #[serde(default = "default_true")]
    pub similar_show_index: bool,
    #[serde(default = "default_true")]
    pub similar_show_thumbnail: bool,
    #[serde(default = "default_true")]
    pub similar_show_album: bool,
    #[serde(default = "default_true")]
    pub similar_show_duration: bool,
    #[serde(default = "default_true")]
    pub similar_show_love: bool,
    #[serde(default)]
    pub similar_show_select: bool,

    // -- Per-view artwork text overlay toggles --
    pub albums_artwork_overlay: bool,
    pub artists_artwork_overlay: bool,
    pub songs_artwork_overlay: bool,
    pub playlists_artwork_overlay: bool,

    // -- Artwork column layout --
    #[serde(default)]
    pub artwork_column_mode: ArtworkColumnMode,
    #[serde(default)]
    pub artwork_column_stretch_fit: ArtworkStretchFit,
    #[serde(
        default = "default_artwork_column_width_pct",
        serialize_with = "round_f32"
    )]
    pub artwork_column_width_pct: f32,
    /// Auto-mode max artwork size as a fraction of the window's short axis
    /// (0.30..=0.70). Default 0.40. Consulted by the Auto resolver for both
    /// the horizontal candidate and the portrait-fallback vertical candidate.
    #[serde(default = "default_artwork_auto_max_pct", serialize_with = "round_f32")]
    pub artwork_auto_max_pct: f32,
    /// Always-Vertical artwork height as a fraction of window height
    /// (0.10..=0.80). Default 0.40. Consulted by the AlwaysVerticalNative /
    /// AlwaysVerticalStretched resolver branches.
    #[serde(
        default = "default_artwork_vertical_height_pct",
        serialize_with = "round_f32"
    )]
    pub artwork_vertical_height_pct: f32,

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
    pub slot_text_links: bool,
    pub horizontal_volume: bool,
    /// Font family override. Empty = system default sans-serif.
    #[serde(default)]
    pub font_family: String,

    // -- Metadata Strip --
    pub strip_show_title: bool,
    pub strip_show_artist: bool,
    pub strip_show_album: bool,
    pub strip_show_format_info: bool,
    pub strip_merged_mode: bool,
    pub strip_click_action: StripClickAction,
    #[serde(default = "default_true")]
    pub strip_show_labels: bool,
    #[serde(default)]
    pub strip_separator: crate::types::player_settings::StripSeparator,

    // -- Playback --
    pub crossfade_enabled: bool,
    pub crossfade_duration_secs: u32,
    /// Volume normalization mode (default: Off). On-disk key is
    /// `volume_normalization_mode`.
    #[serde(rename = "volume_normalization_mode")]
    pub volume_normalization: VolumeNormalizationMode,
    pub normalization_level: NormalizationLevel,
    /// Pre-amp dB applied on top of resolved ReplayGain (default 0.0).
    #[serde(default, serialize_with = "round_f32")]
    pub replay_gain_preamp_db: f32,
    /// Fallback dB for tracks with no ReplayGain tags (default 0.0 = unity).
    #[serde(default, serialize_with = "round_f32")]
    pub replay_gain_fallback_db: f32,
    /// When true, untagged tracks fall through to AGC.
    #[serde(default)]
    pub replay_gain_fallback_to_agc: bool,
    /// When true, clamp gain so `peak * gain <= 1.0`.
    #[serde(default = "default_replay_gain_prevent_clipping")]
    pub replay_gain_prevent_clipping: bool,
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
    #[serde(default)]
    pub queue_show_default_playlist: bool,

    // -- Equalizer --
    pub eq_enabled: bool,
    #[serde(serialize_with = "round_f32_array")]
    pub eq_gains: [f32; 10],
    pub custom_eq_presets: Vec<CustomEqPreset>,

    // -- System tray --
    #[serde(default)]
    pub show_tray_icon: bool,
    #[serde(default)]
    pub close_to_tray: bool,
}

fn default_replay_gain_prevent_clipping() -> bool {
    true
}

fn default_true() -> bool {
    true
}

fn default_artwork_column_width_pct() -> f32 {
    0.40
}

fn default_artwork_auto_max_pct() -> f32 {
    0.40
}

fn default_artwork_vertical_height_pct() -> f32 {
    0.40
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
            library_page_size: LibraryPageSize::default(),
            artwork_resolution: ArtworkResolution::default(),
            show_album_artists_only: true,
            suppress_library_refresh_toasts: false,
            queue_show_stars: true,
            queue_show_album: true,
            queue_show_duration: true,
            queue_show_love: true,
            queue_show_plays: false,
            queue_show_index: true,
            queue_show_thumbnail: true,
            queue_show_genre: false,
            queue_show_select: false,
            albums_show_stars: false,
            albums_show_songcount: true,
            albums_show_plays: false,
            albums_show_love: true,
            albums_show_index: true,
            albums_show_thumbnail: true,
            albums_show_select: false,
            songs_show_stars: false,
            songs_show_album: true,
            songs_show_duration: true,
            songs_show_plays: false,
            songs_show_love: true,
            songs_show_index: true,
            songs_show_thumbnail: true,
            songs_show_genre: false,
            songs_show_select: false,
            artists_show_stars: true,
            artists_show_albumcount: true,
            artists_show_songcount: true,
            artists_show_plays: true,
            artists_show_love: true,
            artists_show_index: true,
            artists_show_thumbnail: true,
            artists_show_select: false,
            genres_show_index: true,
            genres_show_thumbnail: true,
            genres_show_albumcount: true,
            genres_show_songcount: true,
            genres_show_select: false,
            playlists_show_index: true,
            playlists_show_thumbnail: true,
            playlists_show_songcount: false,
            playlists_show_duration: false,
            playlists_show_updatedat: false,
            playlists_show_select: false,
            similar_show_index: true,
            similar_show_thumbnail: true,
            similar_show_album: true,
            similar_show_duration: true,
            similar_show_love: true,
            similar_show_select: false,
            albums_artwork_overlay: true,
            artists_artwork_overlay: true,
            songs_artwork_overlay: true,
            playlists_artwork_overlay: true,
            artwork_column_mode: ArtworkColumnMode::default(),
            artwork_column_stretch_fit: ArtworkStretchFit::default(),
            artwork_column_width_pct: default_artwork_column_width_pct(),
            artwork_auto_max_pct: default_artwork_auto_max_pct(),
            artwork_vertical_height_pct: default_artwork_vertical_height_pct(),
            stable_viewport: true,
            auto_follow_playing: true,
            light_mode: false,
            rounded_mode: false,
            nav_layout: NavLayout::default(),
            nav_display_mode: NavDisplayMode::default(),
            track_info_display: TrackInfoDisplay::default(),
            slot_row_height: SlotRowHeight::default(),
            opacity_gradient: true,
            slot_text_links: true,
            horizontal_volume: false,
            font_family: String::new(),
            strip_show_title: true,
            strip_show_artist: true,
            strip_show_album: true,
            strip_show_format_info: true,
            strip_merged_mode: false,
            strip_click_action: StripClickAction::default(),
            strip_show_labels: true,
            strip_separator: crate::types::player_settings::StripSeparator::default(),
            crossfade_enabled: false,
            crossfade_duration_secs: 5,
            volume_normalization: VolumeNormalizationMode::default(),
            normalization_level: NormalizationLevel::default(),
            replay_gain_preamp_db: 0.0,
            replay_gain_fallback_db: 0.0,
            replay_gain_fallback_to_agc: false,
            replay_gain_prevent_clipping: true,
            visualization_mode: VisualizationMode::default(),
            sound_effects_enabled: true,
            sfx_volume: 0.68,
            scrobbling_enabled: true,
            scrobble_threshold: 0.50,
            quick_add_to_playlist: false,
            queue_show_default_playlist: false,
            eq_enabled: false,
            eq_gains: [0.0; 10],
            custom_eq_presets: Vec::new(),
            show_tray_icon: false,
            close_to_tray: false,
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
            library_page_size: ps.library_page_size,
            artwork_resolution: ps.artwork_resolution,
            show_album_artists_only: ps.show_album_artists_only,
            suppress_library_refresh_toasts: ps.suppress_library_refresh_toasts,
            queue_show_stars: ps.queue_show_stars,
            queue_show_album: ps.queue_show_album,
            queue_show_duration: ps.queue_show_duration,
            queue_show_love: ps.queue_show_love,
            queue_show_plays: ps.queue_show_plays,
            queue_show_index: ps.queue_show_index,
            queue_show_thumbnail: ps.queue_show_thumbnail,
            queue_show_genre: ps.queue_show_genre,
            queue_show_select: ps.queue_show_select,
            albums_show_stars: ps.albums_show_stars,
            albums_show_songcount: ps.albums_show_songcount,
            albums_show_plays: ps.albums_show_plays,
            albums_show_love: ps.albums_show_love,
            albums_show_index: ps.albums_show_index,
            albums_show_thumbnail: ps.albums_show_thumbnail,
            albums_show_select: ps.albums_show_select,
            songs_show_stars: ps.songs_show_stars,
            songs_show_album: ps.songs_show_album,
            songs_show_duration: ps.songs_show_duration,
            songs_show_plays: ps.songs_show_plays,
            songs_show_love: ps.songs_show_love,
            songs_show_index: ps.songs_show_index,
            songs_show_thumbnail: ps.songs_show_thumbnail,
            songs_show_genre: ps.songs_show_genre,
            songs_show_select: ps.songs_show_select,
            artists_show_stars: ps.artists_show_stars,
            artists_show_albumcount: ps.artists_show_albumcount,
            artists_show_songcount: ps.artists_show_songcount,
            artists_show_plays: ps.artists_show_plays,
            artists_show_love: ps.artists_show_love,
            artists_show_index: ps.artists_show_index,
            artists_show_thumbnail: ps.artists_show_thumbnail,
            artists_show_select: ps.artists_show_select,
            genres_show_index: ps.genres_show_index,
            genres_show_thumbnail: ps.genres_show_thumbnail,
            genres_show_albumcount: ps.genres_show_albumcount,
            genres_show_songcount: ps.genres_show_songcount,
            genres_show_select: ps.genres_show_select,
            playlists_show_index: ps.playlists_show_index,
            playlists_show_thumbnail: ps.playlists_show_thumbnail,
            playlists_show_songcount: ps.playlists_show_songcount,
            playlists_show_duration: ps.playlists_show_duration,
            playlists_show_updatedat: ps.playlists_show_updatedat,
            playlists_show_select: ps.playlists_show_select,
            similar_show_index: ps.similar_show_index,
            similar_show_thumbnail: ps.similar_show_thumbnail,
            similar_show_album: ps.similar_show_album,
            similar_show_duration: ps.similar_show_duration,
            similar_show_love: ps.similar_show_love,
            similar_show_select: ps.similar_show_select,
            albums_artwork_overlay: ps.albums_artwork_overlay,
            artists_artwork_overlay: ps.artists_artwork_overlay,
            songs_artwork_overlay: ps.songs_artwork_overlay,
            playlists_artwork_overlay: ps.playlists_artwork_overlay,
            artwork_column_mode: ps.artwork_column_mode,
            artwork_column_stretch_fit: ps.artwork_column_stretch_fit,
            artwork_column_width_pct: ps.artwork_column_width_pct,
            artwork_auto_max_pct: ps.artwork_auto_max_pct,
            artwork_vertical_height_pct: ps.artwork_vertical_height_pct,
            stable_viewport: ps.stable_viewport,
            auto_follow_playing: ps.auto_follow_playing,
            light_mode: false, // Will be read from theme.light_mode or fresh default
            rounded_mode: ps.rounded_mode,
            nav_layout: ps.nav_layout,
            nav_display_mode: ps.nav_display_mode,
            track_info_display: ps.track_info_display,
            slot_row_height: ps.slot_row_height,
            opacity_gradient: ps.opacity_gradient,
            slot_text_links: ps.slot_text_links,
            horizontal_volume: ps.horizontal_volume,
            font_family: ps.font_family.clone(),
            strip_show_title: ps.strip_show_title,
            strip_show_artist: ps.strip_show_artist,
            strip_show_album: ps.strip_show_album,
            strip_show_format_info: ps.strip_show_format_info,
            strip_merged_mode: ps.strip_merged_mode,
            strip_click_action: ps.strip_click_action,
            strip_show_labels: ps.strip_show_labels,
            strip_separator: ps.strip_separator,
            crossfade_enabled: ps.crossfade_enabled,
            crossfade_duration_secs: ps.crossfade_duration_secs,
            volume_normalization: ps.volume_normalization,
            normalization_level: ps.normalization_level,
            replay_gain_preamp_db: ps.replay_gain_preamp_db,
            replay_gain_fallback_db: ps.replay_gain_fallback_db,
            replay_gain_fallback_to_agc: ps.replay_gain_fallback_to_agc,
            replay_gain_prevent_clipping: ps.replay_gain_prevent_clipping,
            visualization_mode: ps.visualization_mode,
            sound_effects_enabled: ps.sound_effects_enabled,
            sfx_volume: ps.sfx_volume,
            scrobbling_enabled: ps.scrobbling_enabled,
            scrobble_threshold: ps.scrobble_threshold,
            quick_add_to_playlist: ps.quick_add_to_playlist,
            queue_show_default_playlist: ps.queue_show_default_playlist,
            eq_enabled: ps.eq_enabled,
            eq_gains: ps.eq_gains,
            custom_eq_presets: ps.custom_eq_presets.clone(),
            show_tray_icon: ps.show_tray_icon,
            close_to_tray: ps.close_to_tray,
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
    fn toml_volume_normalization_mode_serializes_with_mode_key() {
        let settings = TomlSettings::default();
        let toml_str = toml::to_string_pretty(&settings).expect("serialize");
        assert!(
            toml_str.contains("volume_normalization_mode = \"off\""),
            "Expected mode=\"off\", got:\n{toml_str}"
        );
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
    fn toml_roundtrip_queue_column_visibility() {
        let settings = TomlSettings {
            queue_show_stars: false,
            queue_show_album: true,
            queue_show_duration: false,
            queue_show_love: false,
            queue_show_plays: true,
            ..TomlSettings::default()
        };

        let toml_str = toml::to_string_pretty(&settings).expect("serialize");
        let parsed: TomlSettings = toml::from_str(&toml_str).expect("deserialize");
        assert!(!parsed.queue_show_stars);
        assert!(parsed.queue_show_album);
        assert!(!parsed.queue_show_duration);
        assert!(!parsed.queue_show_love);
        assert!(parsed.queue_show_plays);
    }

    #[test]
    fn toml_queue_show_plays_default_is_off() {
        let settings = TomlSettings::default();
        assert!(!settings.queue_show_plays);
    }

    #[test]
    fn toml_show_genre_defaults_are_off() {
        let s = TomlSettings::default();
        assert!(!s.queue_show_genre);
        assert!(!s.songs_show_genre);
    }

    #[test]
    fn toml_show_genre_roundtrips() {
        let s = TomlSettings {
            queue_show_genre: true,
            songs_show_genre: true,
            ..TomlSettings::default()
        };
        let toml_str = toml::to_string_pretty(&s).expect("serialize");
        let parsed: TomlSettings = toml::from_str(&toml_str).expect("deserialize");
        assert!(parsed.queue_show_genre);
        assert!(parsed.songs_show_genre);
    }

    #[test]
    fn toml_view_column_defaults_preserve_today_behavior() {
        let s = TomlSettings::default();
        // Albums: stars + plays opt-in (today only show on their sort modes).
        assert!(!s.albums_show_stars);
        assert!(s.albums_show_songcount);
        assert!(!s.albums_show_plays);
        assert!(s.albums_show_love);
        // Songs: same opt-in pattern.
        assert!(!s.songs_show_stars);
        assert!(s.songs_show_album);
        assert!(s.songs_show_duration);
        assert!(!s.songs_show_plays);
        assert!(s.songs_show_love);
        // Artists: everything on (today's permanent layout).
        assert!(s.artists_show_stars);
        assert!(s.artists_show_albumcount);
        assert!(s.artists_show_songcount);
        assert!(s.artists_show_plays);
        assert!(s.artists_show_love);
    }

    #[test]
    fn toml_roundtrip_view_column_visibility() {
        let s = TomlSettings {
            albums_show_stars: true,
            albums_show_plays: true,
            songs_show_stars: true,
            songs_show_album: false,
            artists_show_plays: false,
            artists_show_love: false,
            ..TomlSettings::default()
        };

        let toml_str = toml::to_string_pretty(&s).expect("serialize");
        let parsed: TomlSettings = toml::from_str(&toml_str).expect("deserialize");
        assert!(parsed.albums_show_stars);
        assert!(parsed.albums_show_plays);
        assert!(parsed.songs_show_stars);
        assert!(!parsed.songs_show_album);
        assert!(!parsed.artists_show_plays);
        assert!(!parsed.artists_show_love);
    }

    #[test]
    fn toml_strip_merged_mode_default_is_off() {
        let settings = TomlSettings::default();
        assert!(!settings.strip_merged_mode);
    }

    #[test]
    fn toml_strip_merged_mode_roundtrip() {
        let settings = TomlSettings {
            strip_merged_mode: true,
            ..TomlSettings::default()
        };
        let toml_str = toml::to_string_pretty(&settings).expect("serialize");
        let parsed: TomlSettings = toml::from_str(&toml_str).expect("deserialize");
        assert!(parsed.strip_merged_mode);
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
