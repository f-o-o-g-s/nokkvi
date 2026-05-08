//! Playback-tab settings table.
//!
//! Owns the `general.crossfade_*`, `general.volume_normalization`,
//! `general.normalization_level`, `general.replay_gain_*`,
//! `general.scrobbl*`, the playlist scalars, the queue column-visibility
//! booleans, and the `general.opacity_gradient` / `general.rounded_mode`
//! Theme-tab top scalars. The macro emits dispatch + apply in lockstep;
//! audio-engine pushes still happen via `PlayerSettingsLoaded` after the
//! refreshed `PlayerSettings` round-trips back to the UI.
//!
//! Notes on the deliberately-omitted keys:
//!
//! - `general.default_playlist_name` opens a dialog instead of taking a
//!   plain `Text` value, so it stays on the special-case path.
//! - `general.light_mode` writes `config.toml` directly *and* mutates a
//!   UI-crate atomic — its setter pattern is incompatible with the
//!   `mgr.set_X(v)` macro contract, so it stays on the legacy match arm.
//!   The audit's noted asymmetry (apply_toml writes `p.light_mode` but
//!   `get_player_settings` doesn't emit it) is left as-is.

use crate::{
    define_settings,
    types::{
        player_settings::{NormalizationLevel, VolumeNormalizationMode},
        settings_data::PlaybackSettingsData,
    },
};

define_settings! {
    tab: crate::types::setting_def::Tab::Playback,
    data_type: PlaybackSettingsData<'_>,
    items_fn: build_playback_tab_settings_items,
    settings_const: TAB_PLAYBACK_SETTINGS,
    contains_fn: tab_playback_contains,
    dispatch_fn: dispatch_playback_tab_setting,
    apply_fn: apply_toml_playback_tab,
    dump_fn: dump_playback_tab_player_settings,
    settings: [
        // -- Playback ---------------------------------------------------------
        CrossfadeEnabled {
            key: "general.crossfade_enabled",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_crossfade_enabled(v),
            toml_apply: |ts, p| p.crossfade_enabled = ts.crossfade_enabled,
            read: |src, out| out.crossfade_enabled = src.crossfade_enabled,
            ui_meta: {
                label: "Crossfade",
                category: "Playback",
                subtitle: Some("Fade between tracks instead of gapless transitions"),
                default: false,
                read_field: |d| d.crossfade_enabled,
            },
        },
        CrossfadeDuration {
            key: "general.crossfade_duration",
            value_type: Int,
            setter: |mgr, v: i64| mgr.set_crossfade_duration(v as u32),
            toml_apply: |ts, p| p.crossfade_duration_secs = ts.crossfade_duration_secs,
            read: |src, out| out.crossfade_duration_secs = src.crossfade_duration_secs,
            ui_meta: {
                label: "Crossfade Duration",
                category: "Playback",
                subtitle: Some("Duration of crossfade between tracks"),
                default: 5_i64,
                min: 1_i64,
                max: 15_i64,
                step: 1_i64,
                unit: "s",
                read_field: |d| d.crossfade_duration_secs,
            },
        },
        VolumeNormalization {
            key: "general.volume_normalization",
            value_type: Enum,
            setter: |mgr, v: String| {
                mgr.set_volume_normalization(VolumeNormalizationMode::from_label(&v))
            },
            toml_apply: |ts, p| p.volume_normalization = ts.volume_normalization,
            read: |src, out| out.volume_normalization = src.volume_normalization,
            ui_meta: {
                label: "Volume Normalization",
                category: "Playback",
                subtitle: Some("Off · ReplayGain (track or album) · AGC (real-time)"),
                default: "Off",
                options: &["Off", "ReplayGain (Track)", "ReplayGain (Album)", "AGC"],
                read_field: |d| d.volume_normalization,
            },
        },
        NormalizationLevelKey {
            key: "general.normalization_level",
            value_type: Enum,
            setter: |mgr, v: String| {
                mgr.set_normalization_level(NormalizationLevel::from_label(&v))
            },
            toml_apply: |ts, p| p.normalization_level = ts.normalization_level,
            read: |src, out| out.normalization_level = src.normalization_level,
        },
        ReplayGainPreampDb {
            key: "general.replay_gain_preamp_db",
            value_type: Int,
            setter: |mgr, v: i64| mgr.set_replay_gain_preamp_db(v as f32),
            toml_apply: |ts, p| p.replay_gain_preamp_db = ts.replay_gain_preamp_db,
            read: |src, out| out.replay_gain_preamp_db = src.replay_gain_preamp_db,
        },
        ReplayGainFallbackDb {
            key: "general.replay_gain_fallback_db",
            value_type: Int,
            setter: |mgr, v: i64| mgr.set_replay_gain_fallback_db(v as f32),
            toml_apply: |ts, p| p.replay_gain_fallback_db = ts.replay_gain_fallback_db,
            read: |src, out| out.replay_gain_fallback_db = src.replay_gain_fallback_db,
        },
        ReplayGainFallbackToAgc {
            key: "general.replay_gain_fallback_to_agc",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_replay_gain_fallback_to_agc(v),
            toml_apply: |ts, p| p.replay_gain_fallback_to_agc = ts.replay_gain_fallback_to_agc,
            read: |src, out| out.replay_gain_fallback_to_agc = src.replay_gain_fallback_to_agc,
        },
        ReplayGainPreventClipping {
            key: "general.replay_gain_prevent_clipping",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_replay_gain_prevent_clipping(v),
            toml_apply: |ts, p| p.replay_gain_prevent_clipping = ts.replay_gain_prevent_clipping,
            read: |src, out| out.replay_gain_prevent_clipping = src.replay_gain_prevent_clipping,
        },

        // -- Scrobbling -------------------------------------------------------
        ScrobblingEnabled {
            key: "general.scrobbling_enabled",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_scrobbling_enabled(v),
            toml_apply: |ts, p| p.scrobbling_enabled = ts.scrobbling_enabled,
            read: |src, out| out.scrobbling_enabled = src.scrobbling_enabled,
            ui_meta: {
                label: "Scrobbling Enabled",
                category: "Scrobbling",
                subtitle: Some("Report listening activity to server"),
                default: true,
                read_field: |d| d.scrobbling_enabled,
            },
        },
        // UI sends the threshold as a 25-90 integer percentage; the setter
        // stores it as a 0.25-0.90 fraction (and clamps). Internal storage is
        // f64; the UI-facing struct narrows back to f32. The `read_field`
        // mirrors the legacy items_playback.rs conversion (fraction → pct int).
        ScrobbleThreshold {
            key: "general.scrobble_threshold",
            value_type: Int,
            setter: |mgr, v: i64| mgr.set_scrobble_threshold(v as f64 / 100.0),
            toml_apply: |ts, p| p.scrobble_threshold = ts.scrobble_threshold as f64,
            read: |src, out| out.scrobble_threshold = src.scrobble_threshold as f32,
            ui_meta: {
                label: "Scrobble Threshold",
                category: "Scrobbling",
                subtitle: Some("% of track duration needed to scrobble"),
                default: 50_i64,
                min: 25_i64,
                max: 90_i64,
                step: 5_i64,
                unit: "%",
                read_field: |d| (d.scrobble_threshold * 100.0).round() as i64,
            },
        },

        // -- Playlists --------------------------------------------------------
        QuickAddToPlaylist {
            key: "general.quick_add_to_playlist",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_quick_add_to_playlist(v),
            toml_apply: |ts, p| p.quick_add_to_playlist = ts.quick_add_to_playlist,
            read: |src, out| out.quick_add_to_playlist = src.quick_add_to_playlist,
            ui_meta: {
                label: "Quick Add to Playlist",
                category: "Playlists",
                subtitle: Some("Skip the playlist picker dialog · uses default playlist"),
                default: false,
                read_field: |d| d.quick_add_to_playlist,
            },
        },
        QueueShowDefaultPlaylist {
            key: "general.queue_show_default_playlist",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_queue_show_default_playlist(v),
            toml_apply: |ts, p| p.queue_show_default_playlist = ts.queue_show_default_playlist,
            read: |src, out| out.queue_show_default_playlist = src.queue_show_default_playlist,
            ui_meta: {
                label: "Default Playlist Chip in Queue",
                category: "Playlists",
                subtitle: Some(
                    "Display the default playlist chip in the queue view's header",
                ),
                default: false,
                read_field: |d| d.queue_show_default_playlist,
            },
        },

        // -- Queue column visibility -----------------------------------------
        // These keys aren't dispatched via `WriteGeneralSetting` today (the
        // toggles live in the Queue header, not the settings tab), but their
        // setters and apply lines exist — declaring them here moves the
        // apply-side assignments under the macro's compile-time check.
        QueueShowStars {
            key: "general.queue_show_stars",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_queue_show_stars(v),
            toml_apply: |ts, p| p.queue_show_stars = ts.queue_show_stars,
            read: |src, out| out.queue_show_stars = src.queue_show_stars,
        },
        QueueShowAlbum {
            key: "general.queue_show_album",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_queue_show_album(v),
            toml_apply: |ts, p| p.queue_show_album = ts.queue_show_album,
            read: |src, out| out.queue_show_album = src.queue_show_album,
        },
        QueueShowDuration {
            key: "general.queue_show_duration",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_queue_show_duration(v),
            toml_apply: |ts, p| p.queue_show_duration = ts.queue_show_duration,
            read: |src, out| out.queue_show_duration = src.queue_show_duration,
        },
        QueueShowLove {
            key: "general.queue_show_love",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_queue_show_love(v),
            toml_apply: |ts, p| p.queue_show_love = ts.queue_show_love,
            read: |src, out| out.queue_show_love = src.queue_show_love,
        },

        // -- Theme tab top scalars (Bool) ------------------------------------
        OpacityGradient {
            key: "general.opacity_gradient",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_opacity_gradient(v),
            toml_apply: |ts, p| p.opacity_gradient = ts.opacity_gradient,
            read: |src, out| out.opacity_gradient = src.opacity_gradient,
        },
        RoundedMode {
            key: "general.rounded_mode",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_rounded_mode(v),
            toml_apply: |ts, p| p.rounded_mode = ts.rounded_mode,
            read: |src, out| out.rounded_mode = src.rounded_mode,
        },
    ]
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::{
        services::{settings::SettingsManager, state_storage::StateStorage},
        types::{
            player_settings::{NormalizationLevel, VolumeNormalizationMode},
            setting_item::SettingsEntry,
            setting_value::SettingValue,
            settings::PlayerSettings,
            settings_data::PlaybackSettingsData,
            toml_settings::TomlSettings,
        },
    };

    fn make_test_manager() -> (SettingsManager, TempDir) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("test_settings.redb");
        let storage = StateStorage::new(path).expect("StateStorage::new");
        (SettingsManager::for_test(storage), tmp)
    }

    fn default_playback_data() -> PlaybackSettingsData<'static> {
        PlaybackSettingsData {
            crossfade_enabled: false,
            crossfade_duration_secs: 5,
            volume_normalization: "Off",
            normalization_level: "Normal",
            replay_gain_preamp_db: 0,
            replay_gain_fallback_db: 0,
            replay_gain_fallback_to_agc: false,
            replay_gain_prevent_clipping: true,
            scrobbling_enabled: true,
            scrobble_threshold: 0.50,
            quick_add_to_playlist: false,
            default_playlist_name: "",
            queue_show_default_playlist: false,
        }
    }

    /// 7 entries get ui_meta — 3 unconditional Playback rows + 2 Scrobbling
    /// + 2 Playlists. The 5 conditional AGC/RG knobs and the
    /// `default_playlist_name` dialog row stay hand-written, plus the 6
    /// lifecycle-only entries (queue column visibility, opacity_gradient,
    /// rounded_mode) emit nothing here.
    #[test]
    fn build_playback_tab_settings_items_emits_seven_rows() {
        let data = default_playback_data();
        let entries = build_playback_tab_settings_items(&data);
        assert_eq!(entries.len(), 7);
        for e in &entries {
            assert!(matches!(e, SettingsEntry::Item(_)));
        }
    }

    /// `scrobble_threshold` reads `f64 → i64` percent in `read_field`. Verify
    /// 0.75 → 75 reaches the row's `Int { val }`.
    #[test]
    fn build_playback_scrobble_threshold_reads_fraction_as_percent() {
        let mut data = default_playback_data();
        data.scrobble_threshold = 0.75;
        let entries = build_playback_tab_settings_items(&data);
        let row = entries
            .iter()
            .find_map(|e| match e {
                SettingsEntry::Item(item) if item.key.as_ref() == "general.scrobble_threshold" => {
                    Some(item)
                }
                _ => None,
            })
            .expect("scrobble_threshold row");
        match &row.value {
            SettingValue::Int { val, .. } => assert_eq!(*val, 75),
            other => panic!("expected Int, got {other:?}"),
        }
    }

    #[test]
    fn tab_playback_contains_recognizes_declared_keys() {
        assert!(tab_playback_contains("general.crossfade_enabled"));
        assert!(tab_playback_contains("general.volume_normalization"));
        assert!(tab_playback_contains("general.replay_gain_preamp_db"));
        assert!(tab_playback_contains("general.scrobble_threshold"));
        assert!(tab_playback_contains("general.opacity_gradient"));
        assert!(tab_playback_contains("general.rounded_mode"));
        assert!(tab_playback_contains("general.queue_show_stars"));
        assert!(!tab_playback_contains("general.light_mode"));
        assert!(!tab_playback_contains("general.stable_viewport"));
    }

    #[test]
    fn dispatch_playback_returns_none_for_unknown_key() {
        let (mut mgr, _tmp) = make_test_manager();
        let result =
            dispatch_playback_tab_setting("nonexistent.key", SettingValue::Bool(true), &mut mgr);
        assert!(result.is_none());
    }

    /// Bool round-trip: `general.crossfade_enabled` is the canonical Bool
    /// example. Default is `false`; flip via the dispatcher and confirm
    /// `get_player_settings()` reports the new value.
    #[test]
    fn dispatch_playback_bool_round_trip_crossfade_enabled() {
        let (mut mgr, _tmp) = make_test_manager();
        assert!(!mgr.get_player_settings().crossfade_enabled);

        let result = dispatch_playback_tab_setting(
            "general.crossfade_enabled",
            SettingValue::Bool(true),
            &mut mgr,
        );

        assert!(matches!(
            result,
            Some(Ok(
                crate::types::settings_side_effect::SettingsSideEffect::None
            ))
        ));
        assert!(mgr.get_player_settings().crossfade_enabled);
    }

    /// Number/f32 round-trip: `general.replay_gain_preamp_db` arrives as
    /// `Int` (the UI uses an integer dB stepper), the setter casts to f32,
    /// and `get_player_settings()` exposes the f32. Verifies the cast path.
    #[test]
    fn dispatch_playback_number_round_trip_replay_gain_preamp_db() {
        let (mut mgr, _tmp) = make_test_manager();
        assert_eq!(mgr.get_player_settings().replay_gain_preamp_db, 0.0);

        let result = dispatch_playback_tab_setting(
            "general.replay_gain_preamp_db",
            SettingValue::Int {
                val: 6,
                min: -15,
                max: 15,
                step: 1,
                unit: "dB",
            },
            &mut mgr,
        );

        assert!(matches!(
            result,
            Some(Ok(
                crate::types::settings_side_effect::SettingsSideEffect::None
            ))
        ));
        assert_eq!(mgr.get_player_settings().replay_gain_preamp_db, 6.0_f32);
    }

    /// Enum round-trip: `general.volume_normalization` arrives as `Enum`
    /// with a label, the setter parses it to `VolumeNormalizationMode`,
    /// and `get_player_settings()` reports the matching enum variant.
    #[test]
    fn dispatch_playback_enum_round_trip_volume_normalization() {
        let (mut mgr, _tmp) = make_test_manager();
        assert_eq!(
            mgr.get_player_settings().volume_normalization,
            VolumeNormalizationMode::default()
        );

        let result = dispatch_playback_tab_setting(
            "general.volume_normalization",
            SettingValue::Enum {
                val: "ReplayGain (Track)".to_string(),
                options: vec!["Off", "ReplayGain (Track)", "ReplayGain (Album)", "AGC"],
            },
            &mut mgr,
        );

        assert!(matches!(
            result,
            Some(Ok(
                crate::types::settings_side_effect::SettingsSideEffect::None
            ))
        ));
        assert_eq!(
            mgr.get_player_settings().volume_normalization,
            VolumeNormalizationMode::ReplayGainTrack
        );
    }

    /// Type-mismatch path: feeding `Bool` to a key declared as `Int`
    /// should yield `Some(Err(_))` — proves the macro's per-variant
    /// dispatch arm is rejecting wrong types instead of silently coercing.
    #[test]
    fn dispatch_playback_returns_err_on_type_mismatch() {
        let (mut mgr, _tmp) = make_test_manager();
        let result = dispatch_playback_tab_setting(
            "general.crossfade_duration",
            SettingValue::Bool(true),
            &mut mgr,
        );
        assert!(matches!(result, Some(Err(_))));
    }

    /// Scrobble-threshold dispatch path divides the percentage Int by 100
    /// to land in the 0.25-0.90 fraction range expected by the setter.
    #[test]
    fn dispatch_playback_scrobble_threshold_int_to_fraction() {
        let (mut mgr, _tmp) = make_test_manager();
        let result = dispatch_playback_tab_setting(
            "general.scrobble_threshold",
            SettingValue::Int {
                val: 75,
                min: 25,
                max: 90,
                step: 5,
                unit: "%",
            },
            &mut mgr,
        );

        assert!(matches!(
            result,
            Some(Ok(
                crate::types::settings_side_effect::SettingsSideEffect::None
            ))
        ));
        let p = mgr.get_player_settings();
        assert!((p.scrobble_threshold - 0.75_f32).abs() < f32::EPSILON);
    }

    /// Apply-side: `apply_toml_playback_tab` copies every declared key
    /// from `TomlSettings` onto the redb-backed `PlayerSettings`. Spot-check
    /// a Bool, a Number, and an Enum to prove the closures all wired up.
    #[test]
    fn apply_toml_playback_copies_declared_fields() {
        let mut ts = TomlSettings::default();
        ts.crossfade_enabled = true;
        ts.crossfade_duration_secs = 9;
        ts.replay_gain_preamp_db = 4.0;
        ts.volume_normalization = VolumeNormalizationMode::ReplayGainAlbum;
        ts.normalization_level = NormalizationLevel::Loud;
        ts.opacity_gradient = false;
        ts.rounded_mode = true;
        ts.queue_show_stars = false;

        let mut p = PlayerSettings::default();
        apply_toml_playback_tab(&ts, &mut p);

        assert!(p.crossfade_enabled);
        assert_eq!(p.crossfade_duration_secs, 9);
        assert_eq!(p.replay_gain_preamp_db, 4.0);
        assert_eq!(
            p.volume_normalization,
            VolumeNormalizationMode::ReplayGainAlbum
        );
        assert_eq!(p.normalization_level, NormalizationLevel::Loud);
        assert!(!p.opacity_gradient);
        assert!(p.rounded_mode);
        assert!(!p.queue_show_stars);
    }

    /// Read-side: `dump_playback_tab_player_settings` mirrors migrated fields
    /// onto the UI-facing struct. Includes the f64→f32 narrowing on
    /// `scrobble_threshold` — the only non-trivial cast on this tab.
    #[test]
    fn dump_playback_round_trip_copies_migrated_fields() {
        let (mgr, _tmp) = make_test_manager();
        let mut ui = mgr.get_player_settings();

        let mut src = PlayerSettings::default();
        src.crossfade_enabled = true;
        src.crossfade_duration_secs = 9;
        src.replay_gain_preamp_db = 4.0;
        src.volume_normalization = VolumeNormalizationMode::ReplayGainAlbum;
        src.normalization_level = NormalizationLevel::Loud;
        src.opacity_gradient = false;
        src.rounded_mode = true;
        src.queue_show_stars = false;
        src.scrobble_threshold = 0.75;

        dump_playback_tab_player_settings(&src, &mut ui);

        assert!(ui.crossfade_enabled);
        assert_eq!(ui.crossfade_duration_secs, 9);
        assert_eq!(ui.replay_gain_preamp_db, 4.0);
        assert_eq!(
            ui.volume_normalization,
            VolumeNormalizationMode::ReplayGainAlbum
        );
        assert_eq!(ui.normalization_level, NormalizationLevel::Loud);
        assert!(!ui.opacity_gradient);
        assert!(ui.rounded_mode);
        assert!(!ui.queue_show_stars);
        assert!((ui.scrobble_threshold - 0.75_f32).abs() < f32::EPSILON);
    }
}
