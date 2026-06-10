//! TOML-serializable settings for the `[settings]` section of config.toml.
//!
//! Contains only user-facing preferences. High-frequency values (volume),
//! runtime state (queue, active playlist), and sensitive data (credentials)
//! remain in redb.

use serde::{Deserialize, Serialize, Serializer};

use crate::{
    audio::eq::{CustomEqPreset, EQ_BAND_COUNT},
    types::{
        player_settings::{
            ArtworkColumnMode, ArtworkResolution, ArtworkStretchFit, CollapsedAppearance,
            EnterBehavior, LibraryPageSize, NavDisplayMode, NavLayout, NormalizationLevel,
            RatingReminderTrigger, RoundedMode, SlotRowHeight, StripClickAction, TrackInfoDisplay,
            VisualizationMode, VolumeNormalizationMode, deserialize_rounded_mode_with_bool_compat,
        },
        view_columns::ViewColumns,
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
    /// Per-view column-visibility toggles — flattened so every
    /// `<view>_show_<col>` key stays a TOP-LEVEL `[settings]` key (pinned by
    /// `toml_column_keys_stay_flat_on_the_toml_wire`). Missing keys fill from
    /// `ViewColumns::default()` — the single source of truth for the shipped
    /// column defaults.
    #[serde(flatten)]
    pub view_columns: ViewColumns,

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
    #[serde(deserialize_with = "deserialize_rounded_mode_with_bool_compat")]
    pub rounded_mode: RoundedMode,
    pub nav_layout: NavLayout,
    pub nav_display_mode: NavDisplayMode,
    pub track_info_display: TrackInfoDisplay,
    pub slot_row_height: SlotRowHeight,
    pub opacity_gradient: bool,
    pub slot_text_links: bool,
    pub horizontal_volume: bool,
    /// Whether the view-header toolbar auto-hides to a thin line until hovered
    /// or a sort/search shortcut is used (default false).
    #[serde(default)]
    pub autohide_toolbar: bool,
    /// Collapsed auto-hide toolbar height in px (default 6).
    pub autohide_toolbar_height: u32,
    /// Whether the collapsed auto-hide toolbar shows a centered accent grip bar (default true).
    pub autohide_toolbar_grip: bool,
    /// What the collapsed auto-hide toolbar shows (default Hairline).
    pub autohide_collapsed_appearance: CollapsedAppearance,
    pub mini_player_show_volume: bool,
    pub mini_player_show_modes: bool,
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
    /// Whether the Previous button restarts the current track (instead of
    /// stepping back) once it has played past the threshold. Default false.
    pub rewind_on_previous: bool,
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
    pub eq_gains: [f32; EQ_BAND_COUNT],
    pub custom_eq_presets: Vec<CustomEqPreset>,

    // -- System tray --
    #[serde(default)]
    pub show_tray_icon: bool,
    #[serde(default)]
    pub close_to_tray: bool,

    // -- Rating reminder --
    #[serde(default)]
    pub rating_reminder_enabled: bool,
    #[serde(default)]
    pub rating_reminder_trigger: RatingReminderTrigger,
    #[serde(default = "default_rating_reminder_percent")]
    pub rating_reminder_percent: u32,
}

fn default_replay_gain_prevent_clipping() -> bool {
    true
}

fn default_rating_reminder_percent() -> u32 {
    75
}

fn default_true() -> bool {
    true
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
            stable_viewport: true,
            auto_follow_playing: true,
            light_mode: false,
            rounded_mode: RoundedMode::On,
            nav_layout: NavLayout::default(),
            nav_display_mode: NavDisplayMode::default(),
            track_info_display: TrackInfoDisplay::PlayerBar,
            slot_row_height: SlotRowHeight::Compact,
            opacity_gradient: false,
            slot_text_links: true,
            horizontal_volume: false,
            autohide_toolbar: false,
            autohide_toolbar_height: 6,
            autohide_toolbar_grip: true,
            autohide_collapsed_appearance: CollapsedAppearance::default(),
            mini_player_show_volume: true,
            mini_player_show_modes: true,
            font_family: String::new(),
            strip_show_title: true,
            strip_show_artist: true,
            strip_show_album: true,
            strip_show_format_info: true,
            strip_merged_mode: true,
            strip_click_action: StripClickAction::default(),
            strip_show_labels: true,
            strip_separator: crate::types::player_settings::StripSeparator::Slash,
            crossfade_enabled: true,
            crossfade_duration_secs: 7,
            rewind_on_previous: false,
            volume_normalization: VolumeNormalizationMode::default(),
            normalization_level: NormalizationLevel::default(),
            replay_gain_preamp_db: 0.0,
            replay_gain_fallback_db: 0.0,
            replay_gain_fallback_to_agc: false,
            replay_gain_prevent_clipping: true,
            visualization_mode: VisualizationMode::default(),
            sound_effects_enabled: false,
            sfx_volume: 0.68,
            scrobbling_enabled: true,
            scrobble_threshold: 0.50,
            quick_add_to_playlist: false,
            queue_show_default_playlist: false,
            eq_enabled: false,
            eq_gains: [0.0; EQ_BAND_COUNT],
            custom_eq_presets: Vec::new(),
            show_tray_icon: false,
            close_to_tray: false,
            rating_reminder_enabled: false,
            rating_reminder_trigger: RatingReminderTrigger::default(),
            rating_reminder_percent: 75,
        }
    }
}

impl TomlSettings {
    /// Build a `TomlSettings` from a `LivePlayerSettings` (for migration from redb).
    ///
    /// Composition: start from default TOML, apply each tab's macro-emitted
    /// `write_<tab>_toml` (covers ~53 migrated setting keys), apply each
    /// view's macro-emitted `write_<view>_columns_to_toml` (covers ~54
    /// column-visibility booleans, including `queue_show_genre` and
    /// `songs_show_genre` that the legacy hand-written body silently
    /// omitted), then hand-write the residual scalars that aren't (yet)
    /// owned by any per-tab or per-view-column macro invocation.
    ///
    /// `light_mode` has no writer that sources from `ps` (the value lives
    /// on a UI atomic + `config.toml`, not on `LivePlayerSettings`). To
    /// prevent the whole-section replace in `write_section` from stomping
    /// the targeted writer's prior value, this entry point reads the
    /// current on-disk value and threads it through. Tests use
    /// [`from_player_settings_with_existing`] to pin the merge in isolation.
    pub fn from_player_settings(ps: &crate::types::player_settings::LivePlayerSettings) -> Self {
        let existing_light_mode = crate::services::toml_settings_io::read_toml_settings()
            .ok()
            .flatten()
            .map(|s| s.light_mode);
        Self::from_player_settings_with_existing(ps, existing_light_mode)
    }

    /// Same as [`from_player_settings`], but accepts an `existing_light_mode`
    /// override that the production caller is expected to source from the
    /// on-disk `[settings]` table. Tests pass a known value directly so they
    /// can assert the merge behavior without needing a writable `config.toml`
    /// path (which `get_config_path` does not expose for tests).
    pub fn from_player_settings_with_existing(
        ps: &crate::types::player_settings::LivePlayerSettings,
        existing_light_mode: Option<bool>,
    ) -> Self {
        let mut ts = Self::default();

        // Per-tab macro-emitted writers (define_settings! `write:` closures).
        crate::services::settings_tables::write_general_tab_toml(ps, &mut ts);
        crate::services::settings_tables::write_interface_tab_toml(ps, &mut ts);
        crate::services::settings_tables::write_playback_tab_toml(ps, &mut ts);

        // Per-view-column macro-emitted writers (define_view_column_toml_helpers!).
        crate::types::view_column_toml::write_albums_columns_to_toml(ps, &mut ts);
        crate::types::view_column_toml::write_artists_columns_to_toml(ps, &mut ts);
        crate::types::view_column_toml::write_genres_columns_to_toml(ps, &mut ts);
        crate::types::view_column_toml::write_playlists_columns_to_toml(ps, &mut ts);
        crate::types::view_column_toml::write_similar_columns_to_toml(ps, &mut ts);
        crate::types::view_column_toml::write_songs_columns_to_toml(ps, &mut ts);
        crate::types::view_column_toml::write_queue_columns_to_toml(ps, &mut ts);

        // Hand-written residuals — fields not (yet) owned by any per-tab or
        // per-view-column macro invocation:
        //
        // - `artwork_column_width_pct` is the pixel-drag-driven slider that
        //   the Artwork Column section intentionally leaves off the items
        //   dispatcher (see `interface.rs` — "absent: it has a setter but no
        //   UI dispatch arm").
        // - `font_family` routes through `Message::ApplyFont`, not a tab
        //   dispatcher, so no `define_settings!` entry owns it.
        // - The 3 audio/visualizer scalars (`visualization_mode`,
        //   `sound_effects_enabled`, `sfx_volume`) and 3 EQ fields
        //   (`eq_enabled`, `eq_gains`, `custom_eq_presets`) live on
        //   different code paths and aren't claimed by any tab today.
        // - `light_mode` is owned by the `SetLightModeAtomic` side-effect
        //   handler in the UI crate (targeted `update_config_value` write).
        //   The value is threaded in via `existing_light_mode` below so the
        //   whole-section replace doesn't reset it to `false`.
        ts.artwork_column_width_pct = ps.artwork_column_width_pct;
        ts.font_family = ps.font_family.clone();
        ts.visualization_mode = ps.visualization_mode;
        ts.sound_effects_enabled = ps.sound_effects_enabled;
        ts.sfx_volume = ps.sfx_volume;
        ts.eq_enabled = ps.eq_enabled;
        ts.eq_gains = ps.eq_gains;
        ts.custom_eq_presets = ps.custom_eq_presets.clone();

        if let Some(v) = existing_light_mode {
            ts.light_mode = v;
        }

        ts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sparse-config round-trip safety: a `[settings]` table with every
    /// default-valued key stripped out must read back as exactly
    /// `TomlSettings::default()`. Equivalent to: an empty table deserializes to
    /// the struct default. If any field's serde fill default (a field-level
    /// `#[serde(default)]` or the container default) diverges from the struct
    /// `Default` impl, this fails — that field would silently drift when the
    /// sparse writer omits it. (This guard caught `strip_separator` flipping
    /// Slash → Dot.)
    #[test]
    fn empty_table_deserializes_to_struct_default() {
        let empty: TomlSettings = toml::from_str("").expect("deserialize empty [settings]");
        // TomlSettings has no PartialEq (f32 + Vec<CustomEqPreset>), so compare
        // the serialized forms — identical bytes ⇔ identical field values.
        let from_empty = toml::to_string_pretty(&empty).expect("serialize empty-derived");
        let from_default =
            toml::to_string_pretty(&TomlSettings::default()).expect("serialize default");
        assert_eq!(
            from_empty, from_default,
            "an absent [settings] key must fill from the struct default; a mismatch \
             means that field silently drifts when sparse-stripped"
        );
    }

    #[test]
    fn toml_roundtrip() {
        let settings = TomlSettings::default();
        let toml_str = toml::to_string_pretty(&settings).expect("serialize");
        let parsed: TomlSettings = toml::from_str(&toml_str).expect("deserialize");
        // Spot-check key fields
        assert_eq!(parsed.start_view, "Queue");
        assert_eq!(parsed.crossfade_duration_secs, 7);
        assert!(parsed.scrobbling_enabled);
        assert_eq!(parsed.eq_gains, [0.0; EQ_BAND_COUNT]);
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
            view_columns: ViewColumns {
                queue_show_stars: false,
                queue_show_album: true,
                queue_show_duration: false,
                queue_show_love: false,
                queue_show_plays: true,
                ..ViewColumns::default()
            },
            ..TomlSettings::default()
        };

        let toml_str = toml::to_string_pretty(&settings).expect("serialize");
        let parsed: TomlSettings = toml::from_str(&toml_str).expect("deserialize");
        assert!(!parsed.view_columns.queue_show_stars);
        assert!(parsed.view_columns.queue_show_album);
        assert!(!parsed.view_columns.queue_show_duration);
        assert!(!parsed.view_columns.queue_show_love);
        assert!(parsed.view_columns.queue_show_plays);
    }

    #[test]
    fn toml_queue_show_plays_default_is_off() {
        let settings = TomlSettings::default();
        assert!(!settings.view_columns.queue_show_plays);
    }

    #[test]
    fn toml_show_genre_defaults_are_off() {
        let s = TomlSettings::default();
        assert!(!s.view_columns.queue_show_genre);
        assert!(!s.view_columns.songs_show_genre);
    }

    #[test]
    fn toml_show_genre_roundtrips() {
        let s = TomlSettings {
            view_columns: ViewColumns {
                queue_show_genre: true,
                songs_show_genre: true,
                ..ViewColumns::default()
            },
            ..TomlSettings::default()
        };
        let toml_str = toml::to_string_pretty(&s).expect("serialize");
        let parsed: TomlSettings = toml::from_str(&toml_str).expect("deserialize");
        assert!(parsed.view_columns.queue_show_genre);
        assert!(parsed.view_columns.songs_show_genre);
    }

    #[test]
    fn toml_view_column_defaults_preserve_today_behavior() {
        let s = TomlSettings::default();
        // Albums: stars + plays opt-in (today only show on their sort modes).
        assert!(!s.view_columns.albums_show_stars);
        assert!(s.view_columns.albums_show_songcount);
        assert!(!s.view_columns.albums_show_plays);
        assert!(s.view_columns.albums_show_love);
        // Songs: same opt-in pattern.
        assert!(!s.view_columns.songs_show_stars);
        assert!(s.view_columns.songs_show_album);
        assert!(s.view_columns.songs_show_duration);
        assert!(!s.view_columns.songs_show_plays);
        assert!(s.view_columns.songs_show_love);
        // Artists: everything on (today's permanent layout).
        assert!(s.view_columns.artists_show_stars);
        assert!(s.view_columns.artists_show_albumcount);
        assert!(s.view_columns.artists_show_songcount);
        assert!(s.view_columns.artists_show_plays);
        assert!(s.view_columns.artists_show_love);
    }

    #[test]
    fn toml_roundtrip_view_column_visibility() {
        let s = TomlSettings {
            view_columns: ViewColumns {
                albums_show_stars: true,
                albums_show_plays: true,
                songs_show_stars: true,
                songs_show_album: false,
                artists_show_plays: false,
                artists_show_love: false,
                ..ViewColumns::default()
            },
            ..TomlSettings::default()
        };

        let toml_str = toml::to_string_pretty(&s).expect("serialize");
        let parsed: TomlSettings = toml::from_str(&toml_str).expect("deserialize");
        assert!(parsed.view_columns.albums_show_stars);
        assert!(parsed.view_columns.albums_show_plays);
        assert!(parsed.view_columns.songs_show_stars);
        assert!(!parsed.view_columns.songs_show_album);
        assert!(!parsed.view_columns.artists_show_plays);
        assert!(!parsed.view_columns.artists_show_love);
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
    /// TOP-LEVEL `<view>_show_<col>` key inside `[settings]` — never as a
    /// nested `view_columns` table. Existing config.toml files store the
    /// flat shape, and the sparse writer (`toml_settings_io::settings_value`)
    /// prunes against this exact key set. The ViewColumns composition must
    /// preserve it via `#[serde(flatten)]` — this test is intentionally
    /// written without any struct-field access so it survives that refactor
    /// byte-for-byte.
    #[test]
    fn toml_column_keys_stay_flat_on_the_toml_wire() {
        let toml_str = toml::to_string_pretty(&TomlSettings::default()).expect("serialize");
        assert!(
            toml_str.contains("albums_show_stars = "),
            "albums_show_stars must serialize as a flat key, got:\n{toml_str}"
        );
        assert!(
            !toml_str.contains("[view_columns]") && !toml_str.contains("view_columns."),
            "column toggles must not nest under a view_columns table, got:\n{toml_str}"
        );

        // The sparse writer prunes the toml::Value table form — the 50
        // column keys must stay top-level there too, or pruning would stop
        // seeing them and verbose/sparse round-trips would drift.
        let v = toml::Value::try_from(TomlSettings::default()).expect("to toml::Value");
        let table = v.as_table().expect("table");
        assert!(
            !table.contains_key("view_columns"),
            "no nested view_columns key in the toml::Value form"
        );
        let column_keys: Vec<&str> = table
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

    /// Wire-format pin (read direction): a flat top-level column key in an
    /// existing config.toml must land on the matching toggle. Asserted via
    /// re-serialization (no struct-field access) so the test survives the
    /// ViewColumns composition unchanged.
    #[test]
    fn toml_column_keys_deserialize_from_flat_keys() {
        // albums_show_stars defaults to false — the flat key must flip it.
        let parsed: TomlSettings = toml::from_str("albums_show_stars = true").expect("deserialize");
        let v = toml::Value::try_from(parsed).expect("to toml::Value");
        assert_eq!(
            v.get("albums_show_stars"),
            Some(&toml::Value::Boolean(true)),
            "flat albums_show_stars key must land on the toggle"
        );
        // A neighboring default-true column stays at its default.
        assert_eq!(
            v.get("albums_show_love"),
            Some(&toml::Value::Boolean(true)),
            "unrelated columns keep their serde-fill defaults"
        );
    }

    #[test]
    fn toml_strip_merged_mode_default_is_on() {
        let settings = TomlSettings::default();
        assert!(settings.strip_merged_mode);
    }

    #[test]
    fn toml_strip_merged_mode_roundtrip() {
        let settings = TomlSettings {
            strip_merged_mode: false,
            ..TomlSettings::default()
        };
        let toml_str = toml::to_string_pretty(&settings).expect("serialize");
        let parsed: TomlSettings = toml::from_str(&toml_str).expect("deserialize");
        assert!(!parsed.strip_merged_mode);
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
        assert_eq!(parsed.crossfade_duration_secs, 7); // default
    }

    // ══════════════════════════════════════════════════════════════════
    //  from_player_settings_with_existing — light_mode merge
    // ══════════════════════════════════════════════════════════════════
    //
    // Regression guard: toggling light mode then changing any other
    // general-tab setting used to revert the theme. Root cause was
    // `from_player_settings` starting from `Self::default()` (light_mode
    // = false) and the per-tab writers not touching the field, so the
    // resulting `TomlSettings` always serialized `light_mode = false` and
    // the whole-section `[settings]` replace in `write_section` stomped
    // the targeted writer's prior `light_mode = true`. The fix routes
    // through a seedable helper so the production caller can pass the
    // on-disk value and tests can pin merge behavior in isolation.

    #[test]
    fn from_player_settings_with_existing_preserves_some_true() {
        let ps = crate::types::player_settings::LivePlayerSettings::default();
        let ts = TomlSettings::from_player_settings_with_existing(&ps, Some(true));
        assert!(
            ts.light_mode,
            "Some(true) must override the default — whole-section serialize would otherwise stomp on-disk truth",
        );
    }

    #[test]
    fn from_player_settings_with_existing_preserves_some_false() {
        let ps = crate::types::player_settings::LivePlayerSettings::default();
        let ts = TomlSettings::from_player_settings_with_existing(&ps, Some(false));
        assert!(!ts.light_mode, "Some(false) must be honored");
    }

    #[test]
    fn from_player_settings_with_existing_none_keeps_default() {
        let ps = crate::types::player_settings::LivePlayerSettings::default();
        let ts = TomlSettings::from_player_settings_with_existing(&ps, None);
        assert!(
            !ts.light_mode,
            "None must preserve the struct default — no on-disk source means no override",
        );
    }
}
