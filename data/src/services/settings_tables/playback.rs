//! Playback-tab settings table.
//!
//! Owns the `general.crossfade_*`, `general.volume_normalization`,
//! `general.normalization_level`, `general.replay_gain_*`,
//! `general.scrobbl*`, the playlist scalars, the queue column-visibility
//! booleans, and the `general.opacity_gradient` / `general.rounded_mode`
//! Theme-tab top scalars. The macro emits dispatch + apply in lockstep;
//! audio-engine pushes still happen via `PlayerSettingsLoaded` after the
//! refreshed `LivePlayerSettings` round-trips back to the UI.
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
        player_settings::{
            BitPerfectMode, CROSSFADE_DURATION_MAX_SECS, CROSSFADE_DURATION_MIN_SECS,
            NormalizationLevel, RatingReminderTrigger, RoundedMode, VolumeNormalizationMode,
        },
        settings_data::PlaybackSettingsData,
    },
};

define_settings! {
    tab: crate::types::setting_def::Tab::Playback,
    data_type: PlaybackSettingsData,
    mgr_type: crate::services::settings::SettingsManager,
    items_fn: build_playback_tab_settings_items,
    settings_const: TAB_PLAYBACK_SETTINGS,
    contains_fn: tab_playback_contains,
    dispatch_fn: dispatch_playback_tab_setting,
    apply_fn: apply_toml_playback_tab,
    dump_fn: dump_playback_tab_player_settings,
    write_fn: write_playback_tab_toml,
    settings: [
        // -- Playback ---------------------------------------------------------
        CrossfadeEnabled {
            key: "general.crossfade_enabled",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_crossfade_enabled(v),
            toml_apply: |ts, p| p.crossfade_enabled = ts.crossfade_enabled,
            read: |src, out| out.crossfade_enabled = src.crossfade_enabled,
            write: |ps, ts| ts.crossfade_enabled = ps.crossfade_enabled,
            ui_meta: {
                label: "Crossfade",
                category: "Playback",
                subtitle: Some(
                    "Overlap and blend the end of each track into the next. Off plays tracks \
                     gapless with no overlap. Tracks under 10 seconds always play gapless. \
                     Mutually exclusive with Bit-Perfect — turning Crossfade on switches \
                     Bit-Perfect off.",
                ),
                default: true,
                read_field: |d| d.crossfade_enabled,
            },
        },
        BitPerfect {
            key: "general.bit_perfect",
            value_type: Enum,
            setter: |mgr, v: String| mgr.set_bit_perfect(BitPerfectMode::from_label(&v)),
            toml_apply: |ts, p| p.bit_perfect = ts.bit_perfect,
            read: |src, out| out.bit_perfect = src.bit_perfect,
            write: |ps, ts| ts.bit_perfect = ps.bit_perfect,
            ui_meta: {
                label: "Bit-Perfect Output",
                category: "Playback",
                subtitle: Some(
                    "Off keeps the standard 48kHz DSP path (EQ, software volume, crossfade). \
                     Strict and Relaxed bypass EQ, software volume, and the limiter and feed the \
                     DAC each track at its native rate (volume moves to the PipeWire node) — Strict \
                     hard-cuts between every track, Relaxed crossfades tracks that share a sample \
                     rate (only that few-second blend isn't bit-perfect) and hard-cuts the rest. \
                     Mutually exclusive with Crossfade — choosing Strict or Relaxed switches \
                     Crossfade off (Relaxed runs its own same-rate crossfade). A mid-session step \
                     down (e.g. 96k to 44.1k) is high-quality resampled, because the DAC can't \
                     re-clock live without a gap. Needs PipeWire rate-switching (allowed-rates) \
                     configured.",
                ),
                default: "Off",
                options: &["Off", "Strict", "Relaxed"],
                read_field: |d| d.bit_perfect.as_ref(),
            },
        },
        CrossfadeDuration {
            key: "general.crossfade_duration",
            value_type: Int,
            setter: |mgr, v: i64| mgr.set_crossfade_duration(v as u32),
            toml_apply: |ts, p| p.crossfade_duration_secs = ts.crossfade_duration_secs,
            read: |src, out| out.crossfade_duration_secs = src.crossfade_duration_secs,
            write: |ps, ts| ts.crossfade_duration_secs = ps.crossfade_duration_secs,
            ui_meta: {
                label: "Crossfade Duration",
                category: "Playback",
                subtitle: Some("1s = quick blend, 12s = long overlap. Applies as tracks change, not when you skip."),
                default: 7_i64,
                min: i64::from(CROSSFADE_DURATION_MIN_SECS),
                max: i64::from(CROSSFADE_DURATION_MAX_SECS),
                step: 1_i64,
                unit: "s",
                read_field: |d| d.crossfade_duration_secs,
            },
        },
        RewindOnPrevious {
            key: "general.rewind_on_previous",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_rewind_on_previous(v),
            toml_apply: |ts, p| p.rewind_on_previous = ts.rewind_on_previous,
            read: |src, out| out.rewind_on_previous = src.rewind_on_previous,
            write: |ps, ts| ts.rewind_on_previous = ps.rewind_on_previous,
            ui_meta: {
                label: "Rewind on Previous",
                category: "Playback",
                subtitle: Some(
                    "Previous restarts the current track if it's played past 5s, instead of skipping back",
                ),
                default: false,
                read_field: |d| d.rewind_on_previous,
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
            write: |ps, ts| ts.volume_normalization = ps.volume_normalization,
            ui_meta: {
                label: "Volume Normalization",
                category: "Playback",
                subtitle: Some("Off · ReplayGain (track or album) · AGC (real-time)"),
                default: "Off",
                options: &["Off", "ReplayGain (Track)", "ReplayGain (Album)", "AGC"],
                read_field: |d| d.volume_normalization.as_ref(),
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
            write: |ps, ts| ts.normalization_level = ps.normalization_level,
        },
        ReplayGainPreampDb {
            key: "general.replay_gain_preamp_db",
            value_type: Int,
            setter: |mgr, v: i64| mgr.set_replay_gain_preamp_db(v as f32),
            toml_apply: |ts, p| p.replay_gain_preamp_db = ts.replay_gain_preamp_db,
            read: |src, out| out.replay_gain_preamp_db = src.replay_gain_preamp_db,
            write: |ps, ts| ts.replay_gain_preamp_db = ps.replay_gain_preamp_db,
        },
        ReplayGainFallbackDb {
            key: "general.replay_gain_fallback_db",
            value_type: Int,
            setter: |mgr, v: i64| mgr.set_replay_gain_fallback_db(v as f32),
            toml_apply: |ts, p| p.replay_gain_fallback_db = ts.replay_gain_fallback_db,
            read: |src, out| out.replay_gain_fallback_db = src.replay_gain_fallback_db,
            write: |ps, ts| ts.replay_gain_fallback_db = ps.replay_gain_fallback_db,
        },
        ReplayGainFallbackToAgc {
            key: "general.replay_gain_fallback_to_agc",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_replay_gain_fallback_to_agc(v),
            toml_apply: |ts, p| p.replay_gain_fallback_to_agc = ts.replay_gain_fallback_to_agc,
            read: |src, out| out.replay_gain_fallback_to_agc = src.replay_gain_fallback_to_agc,
            write: |ps, ts| ts.replay_gain_fallback_to_agc = ps.replay_gain_fallback_to_agc,
        },
        ReplayGainPreventClipping {
            key: "general.replay_gain_prevent_clipping",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_replay_gain_prevent_clipping(v),
            toml_apply: |ts, p| p.replay_gain_prevent_clipping = ts.replay_gain_prevent_clipping,
            read: |src, out| out.replay_gain_prevent_clipping = src.replay_gain_prevent_clipping,
            write: |ps, ts| ts.replay_gain_prevent_clipping = ps.replay_gain_prevent_clipping,
        },

        // -- Scrobbling -------------------------------------------------------
        ScrobblingEnabled {
            key: "general.scrobbling_enabled",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_scrobbling_enabled(v),
            toml_apply: |ts, p| p.scrobbling_enabled = ts.scrobbling_enabled,
            read: |src, out| out.scrobbling_enabled = src.scrobbling_enabled,
            write: |ps, ts| ts.scrobbling_enabled = ps.scrobbling_enabled,
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
            // UI-PS holds f32, TomlSettings holds f32 — direct copy.
            write: |ps, ts| ts.scrobble_threshold = ps.scrobble_threshold,
            ui_meta: {
                label: "Scrobble Threshold",
                category: "Scrobbling",
                subtitle: Some("% of track, or 4 min, to scrobble"),
                default: 50_i64,
                min: 25_i64,
                max: 90_i64,
                step: 5_i64,
                unit: "%",
                read_field: |d| (d.scrobble_threshold * 100.0).round() as i64,
            },
        },

        // -- Radio scrobbling (direct to ListenBrainz) ------------------------
        RadioScrobblingEnabled {
            key: "general.radio_scrobbling_enabled",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_radio_scrobbling_enabled(v),
            toml_apply: |ts, p| p.radio_scrobbling_enabled = ts.radio_scrobbling_enabled,
            read: |src, out| out.radio_scrobbling_enabled = src.radio_scrobbling_enabled,
            write: |ps, ts| ts.radio_scrobbling_enabled = ps.radio_scrobbling_enabled,
            ui_meta: {
                label: "Scrobble Radio",
                category: "Radio Scrobbling",
                subtitle: Some("Submit internet-radio tracks (from the stream's ICY title) to ListenBrainz. Needs a token below."),
                default: false,
                read_field: |d| d.radio_scrobbling_enabled,
            },
        },
        // Radio streams report no duration, so this is an ABSOLUTE seconds gate
        // (not a percentage). Stored as u32 internally; the UI works in i64.
        RadioScrobbleThresholdSecs {
            key: "general.radio_scrobble_threshold_secs",
            value_type: Int,
            setter: |mgr, v: i64| mgr.set_radio_scrobble_threshold_secs(v),
            toml_apply: |ts, p| {
                p.radio_scrobble_threshold_secs = ts.radio_scrobble_threshold_secs.clamp(
                    crate::types::settings::RADIO_SCROBBLE_THRESHOLD_MIN,
                    crate::types::settings::RADIO_SCROBBLE_THRESHOLD_MAX,
                );
            },
            read: |src, out| out.radio_scrobble_threshold_secs = src.radio_scrobble_threshold_secs,
            write: |ps, ts| ts.radio_scrobble_threshold_secs = ps.radio_scrobble_threshold_secs,
            ui_meta: {
                label: "Radio Listen Threshold",
                category: "Radio Scrobbling",
                subtitle: Some("Seconds a radio track must play before it scrobbles. Lower catches short songs; higher skips station IDs and ads."),
                default: 60_i64,
                min: 20_i64,
                max: 240_i64,
                step: 10_i64,
                unit: "s",
                read_field: |d| d.radio_scrobble_threshold_secs,
            },
        },
        RadioNowPlayingEnabled {
            key: "general.radio_now_playing_enabled",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_radio_now_playing_enabled(v),
            toml_apply: |ts, p| p.radio_now_playing_enabled = ts.radio_now_playing_enabled,
            read: |src, out| out.radio_now_playing_enabled = src.radio_now_playing_enabled,
            write: |ps, ts| ts.radio_now_playing_enabled = ps.radio_now_playing_enabled,
            ui_meta: {
                label: "Send Radio Now-Playing",
                category: "Radio Scrobbling",
                subtitle: Some("Update your now-playing each time the radio track changes."),
                default: true,
                read_field: |d| d.radio_now_playing_enabled,
            },
        },

        // -- Rating reminder --------------------------------------------------
        RatingReminderEnabled {
            key: "general.rating_reminder_enabled",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_rating_reminder_enabled(v),
            toml_apply: |ts, p| p.rating_reminder_enabled = ts.rating_reminder_enabled,
            read: |src, out| out.rating_reminder_enabled = src.rating_reminder_enabled,
            write: |ps, ts| ts.rating_reminder_enabled = ps.rating_reminder_enabled,
            ui_meta: {
                label: "Rating Reminder",
                category: "Rating Reminder",
                subtitle: Some(
                    "Desktop notification reminding you to rate the current track",
                ),
                default: false,
                read_field: |d| d.rating_reminder_enabled,
            },
        },
        RatingChangeNotificationEnabled {
            key: "general.rating_change_notification_enabled",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_rating_change_notification_enabled(v),
            toml_apply: |ts, p| {
                p.rating_change_notification_enabled = ts.rating_change_notification_enabled;
            },
            read: |src, out| {
                out.rating_change_notification_enabled = src.rating_change_notification_enabled;
            },
            write: |ps, ts| {
                ts.rating_change_notification_enabled = ps.rating_change_notification_enabled;
            },
            ui_meta: {
                label: "Rating Change Notification",
                category: "Rating Reminder",
                subtitle: Some(
                    "Desktop notification with the new star rating when you rate by hotkey or CLI · silent when you click stars in-window",
                ),
                default: false,
                read_field: |d| d.rating_change_notification_enabled,
            },
        },
        RatingReminderTriggerKey {
            key: "general.rating_reminder_trigger",
            value_type: Enum,
            setter: |mgr, v: String| {
                mgr.set_rating_reminder_trigger(RatingReminderTrigger::from_label(&v))
            },
            toml_apply: |ts, p| p.rating_reminder_trigger = ts.rating_reminder_trigger,
            read: |src, out| out.rating_reminder_trigger = src.rating_reminder_trigger,
            write: |ps, ts| ts.rating_reminder_trigger = ps.rating_reminder_trigger,
            ui_meta: {
                label: "Reminder Timing",
                category: "Rating Reminder",
                subtitle: Some(
                    "Fire when the track scrobbles, or once a set percentage has played",
                ),
                default: "On Scrobble",
                options: &["On Scrobble", "Percentage Played"],
                read_field: |d| d.rating_reminder_trigger.as_ref(),
            },
        },
        RatingReminderPercent {
            key: "general.rating_reminder_percent",
            value_type: Int,
            setter: |mgr, v: i64| mgr.set_rating_reminder_percent(v as u32),
            toml_apply: |ts, p| p.rating_reminder_percent = ts.rating_reminder_percent,
            read: |src, out| out.rating_reminder_percent = src.rating_reminder_percent,
            write: |ps, ts| ts.rating_reminder_percent = ps.rating_reminder_percent,
            ui_meta: {
                label: "Reminder Percentage",
                category: "Rating Reminder",
                subtitle: Some(
                    "Percent of the track that must play before the reminder fires",
                ),
                default: 75_i64,
                min: 60_i64,
                max: 90_i64,
                step: 5_i64,
                unit: "%",
                read_field: |d| d.rating_reminder_percent,
            },
        },

        // -- Playlists --------------------------------------------------------
        QuickAddToPlaylist {
            key: "general.quick_add_to_playlist",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_quick_add_to_playlist(v),
            toml_apply: |ts, p| p.quick_add_to_playlist = ts.quick_add_to_playlist,
            read: |src, out| out.quick_add_to_playlist = src.quick_add_to_playlist,
            write: |ps, ts| ts.quick_add_to_playlist = ps.quick_add_to_playlist,
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
            write: |ps, ts| ts.queue_show_default_playlist = ps.queue_show_default_playlist,
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
            key: "general.view_columns.queue_show_stars",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_queue_show_stars(v),
            toml_apply: |ts, p| p.view_columns.queue_show_stars = ts.view_columns.queue_show_stars,
            read: |src, out| out.view_columns.queue_show_stars = src.view_columns.queue_show_stars,
            write: |ps, ts| ts.view_columns.queue_show_stars = ps.view_columns.queue_show_stars,
        },
        QueueShowAlbum {
            key: "general.view_columns.queue_show_album",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_queue_show_album(v),
            toml_apply: |ts, p| p.view_columns.queue_show_album = ts.view_columns.queue_show_album,
            read: |src, out| out.view_columns.queue_show_album = src.view_columns.queue_show_album,
            write: |ps, ts| ts.view_columns.queue_show_album = ps.view_columns.queue_show_album,
        },
        QueueShowDuration {
            key: "general.view_columns.queue_show_duration",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_queue_show_duration(v),
            toml_apply: |ts, p| p.view_columns.queue_show_duration = ts.view_columns.queue_show_duration,
            read: |src, out| out.view_columns.queue_show_duration = src.view_columns.queue_show_duration,
            write: |ps, ts| ts.view_columns.queue_show_duration = ps.view_columns.queue_show_duration,
        },
        QueueShowLove {
            key: "general.view_columns.queue_show_love",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_queue_show_love(v),
            toml_apply: |ts, p| p.view_columns.queue_show_love = ts.view_columns.queue_show_love,
            read: |src, out| out.view_columns.queue_show_love = src.view_columns.queue_show_love,
            write: |ps, ts| ts.view_columns.queue_show_love = ps.view_columns.queue_show_love,
        },

        // -- Theme tab top scalars (Bool) ------------------------------------
        OpacityGradient {
            key: "general.opacity_gradient",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_opacity_gradient(v),
            toml_apply: |ts, p| p.opacity_gradient = ts.opacity_gradient,
            read: |src, out| out.opacity_gradient = src.opacity_gradient,
            write: |ps, ts| ts.opacity_gradient = ps.opacity_gradient,
        },
        RoundedModeSetting {
            key: "general.rounded_mode",
            value_type: Enum,
            setter: |mgr, v: String| mgr.set_rounded_mode(RoundedMode::from_label(&v)),
            toml_apply: |ts, p| p.rounded_mode = ts.rounded_mode,
            read: |src, out| out.rounded_mode = src.rounded_mode,
            write: |ps, ts| ts.rounded_mode = ps.rounded_mode,
        },
    ],
    copy_only_const: TAB_PLAYBACK_COPY_ONLY_KEYS,
    copy_only: [
        // Copy-only residuals: apply/dump/write copy-steps ONLY — no setter,
        // no dispatch arm, no ui_meta row. Their write paths live elsewhere
        // (see each entry).
        //
        // `sfx_volume` carries the f64 (Persisted) <-> f32 (Live/Toml) split:
        // the casts below are load-bearing; TOML emit quantizes via the
        // `round_f32` serializer. Written from the SFX volume slider path.
        SfxVolume {
            key: "playback.sfx_volume",
            toml_apply: |ts, p| p.sfx_volume = f64::from(ts.sfx_volume),
            read: |src, out| out.sfx_volume = src.sfx_volume as f32,
            write: |ps, ts| ts.sfx_volume = ps.sfx_volume,
        },
        // The audio-visualizer on/off/mode selector, cycled by the
        // player-bar toggle (NOT the [visualizer] config universe). Copy
        // enum, plain assignment.
        VisualizationModeResidual {
            key: "playback.visualization_mode",
            toml_apply: |ts, p| p.visualization_mode = ts.visualization_mode,
            read: |src, out| out.visualization_mode = src.visualization_mode,
            write: |ps, ts| ts.visualization_mode = ps.visualization_mode,
        },
        // Written from the SFX toggle's own path (plain bool on all three).
        SoundEffectsEnabled {
            key: "playback.sound_effects_enabled",
            toml_apply: |ts, p| p.sound_effects_enabled = ts.sound_effects_enabled,
            read: |src, out| out.sound_effects_enabled = src.sound_effects_enabled,
            write: |ps, ts| ts.sound_effects_enabled = ps.sound_effects_enabled,
        },
        // EQ fields persist from the EQ modal's own path (eq_modal.rs),
        // synced to the engine's EqState — never via the tab dispatcher.
        EqEnabled {
            key: "playback.eq_enabled",
            toml_apply: |ts, p| p.eq_enabled = ts.eq_enabled,
            read: |src, out| out.eq_enabled = src.eq_enabled,
            write: |ps, ts| ts.eq_enabled = ps.eq_enabled,
        },
        // [f32; EQ_BAND_COUNT] is Copy — plain assignment; TOML emit
        // quantizes via round_f32_array.
        EqGains {
            key: "playback.eq_gains",
            toml_apply: |ts, p| p.eq_gains = ts.eq_gains,
            read: |src, out| out.eq_gains = src.eq_gains,
            write: |ps, ts| ts.eq_gains = ps.eq_gains,
        },
        // Vec<CustomEqPreset> — .clone() in all three directions.
        CustomEqPresets {
            key: "playback.custom_eq_presets",
            toml_apply: |ts, p| p.custom_eq_presets = ts.custom_eq_presets.clone(),
            read: |src, out| out.custom_eq_presets = src.custom_eq_presets.clone(),
            write: |ps, ts| ts.custom_eq_presets = ps.custom_eq_presets.clone(),
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
            settings::PersistedPlayerSettings,
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

    fn default_playback_data() -> PlaybackSettingsData {
        PlaybackSettingsData {
            crossfade_enabled: false,
            bit_perfect: "Off".into(),
            crossfade_duration_secs: 5,
            rewind_on_previous: false,
            volume_normalization: "Off".into(),
            normalization_level: "Normal".into(),
            replay_gain_preamp_db: 0,
            replay_gain_fallback_db: 0,
            replay_gain_fallback_to_agc: false,
            replay_gain_prevent_clipping: true,
            scrobbling_enabled: true,
            scrobble_threshold: 0.50,
            radio_scrobbling_enabled: false,
            radio_scrobble_threshold_secs: 60,
            radio_now_playing_enabled: true,
            listenbrainz_source: crate::services::radio_scrobble::source::CredSource::Unset,
            lastfm_credentials_source: crate::services::radio_scrobble::source::CredSource::Unset,
            lastfm_username: "".into(),
            quick_add_to_playlist: false,
            default_playlist_name: "".into(),
            queue_show_default_playlist: false,
            rating_reminder_enabled: false,
            rating_change_notification_enabled: false,
            rating_reminder_trigger: "On Scrobble".into(),
            rating_reminder_percent: 75,
        }
    }

    /// 12 entries get ui_meta: 4 unconditional Playback rows (crossfade enable,
    /// crossfade duration, rewind-on-previous, volume normalization), 2
    /// Scrobbling, 4 Rating Reminder (enable, change-notification, trigger,
    /// percentage), and 2 Playlists. The 5 conditional AGC/RG knobs and the
    /// `default_playlist_name` dialog row stay hand-written; the 6
    /// lifecycle-only entries (queue column visibility, opacity_gradient,
    /// rounded_mode) emit nothing here. The Rating Reminder trigger/percentage
    /// rows are emitted here unconditionally but the UI builder
    /// (`items_playback.rs`) only splices them in when the feature is enabled.
    #[test]
    fn build_playback_tab_settings_items_emits_sixteen_rows() {
        let data = default_playback_data();
        let entries = build_playback_tab_settings_items(&data);
        assert_eq!(entries.len(), 16);
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

    /// Single-source-of-truth interlock: the crossfade-duration slider's
    /// declared `max` MUST equal what `set_crossfade_duration` actually persists
    /// for that value. The slider once offered 15s while the setter clamped to
    /// 12s, so dragging to 13-15 silently truncated to 12 and snapped back on
    /// reload. Both sides now derive from `CROSSFADE_DURATION_MAX_SECS`.
    #[test]
    fn crossfade_duration_slider_max_matches_persisted_clamp() {
        let entries = build_playback_tab_settings_items(&default_playback_data());
        let slider_max = entries
            .iter()
            .find_map(|e| match e {
                SettingsEntry::Item(item) if item.key.as_ref() == "general.crossfade_duration" => {
                    match &item.value {
                        SettingValue::Int { max, .. } => Some(*max),
                        _ => None,
                    }
                }
                _ => None,
            })
            .expect("crossfade_duration row with an Int value");

        let (mut mgr, _tmp) = make_test_manager();

        // The top of the slider must round-trip through the setter unchanged.
        mgr.set_crossfade_duration(slider_max as u32)
            .expect("set to slider max");
        assert_eq!(
            i64::from(mgr.get_player_settings().crossfade_duration_secs),
            slider_max,
            "slider max {slider_max}s is silently truncated by the persistence clamp"
        );

        // One step past the top clamps down to exactly the slider max.
        mgr.set_crossfade_duration(slider_max as u32 + 1)
            .expect("set above slider max");
        assert_eq!(
            i64::from(mgr.get_player_settings().crossfade_duration_secs),
            slider_max,
            "values above the slider max must clamp to the slider max"
        );
    }

    #[test]
    fn tab_playback_contains_recognizes_declared_keys() {
        assert!(tab_playback_contains("general.crossfade_enabled"));
        assert!(tab_playback_contains("general.volume_normalization"));
        assert!(tab_playback_contains("general.replay_gain_preamp_db"));
        assert!(tab_playback_contains("general.scrobble_threshold"));
        assert!(tab_playback_contains("general.opacity_gradient"));
        assert!(tab_playback_contains("general.rounded_mode"));
        assert!(tab_playback_contains(
            "general.view_columns.queue_show_stars"
        ));
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
    /// example. Default is `true`; flip via the dispatcher and confirm
    /// `get_player_settings()` reports the new value.
    #[test]
    fn dispatch_playback_bool_round_trip_crossfade_enabled() {
        let (mut mgr, _tmp) = make_test_manager();
        assert!(mgr.get_player_settings().crossfade_enabled);

        let result = dispatch_playback_tab_setting(
            "general.crossfade_enabled",
            SettingValue::Bool(false),
            &mut mgr,
        );

        assert!(matches!(
            result,
            Some(Ok(
                crate::types::settings_side_effect::SettingsSideEffect::None
            ))
        ));
        assert!(!mgr.get_player_settings().crossfade_enabled);
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
    /// from `TomlSettings` onto the redb-backed `PersistedPlayerSettings`. Spot-check
    /// a Bool, a Number, and an Enum to prove the closures all wired up.
    #[test]
    fn apply_toml_playback_copies_declared_fields() {
        let mut ts = TomlSettings::default();
        ts.crossfade_enabled = true;
        ts.crossfade_duration_secs = 9;
        ts.rewind_on_previous = true;
        ts.replay_gain_preamp_db = 4.0;
        ts.volume_normalization = VolumeNormalizationMode::ReplayGainAlbum;
        ts.normalization_level = NormalizationLevel::Loud;
        ts.opacity_gradient = false;
        ts.rounded_mode = RoundedMode::On;
        ts.view_columns.queue_show_stars = false;

        let mut p = PersistedPlayerSettings::default();
        apply_toml_playback_tab(&ts, &mut p);

        assert!(p.crossfade_enabled);
        assert_eq!(p.crossfade_duration_secs, 9);
        assert!(p.rewind_on_previous);
        assert_eq!(p.replay_gain_preamp_db, 4.0);
        assert_eq!(
            p.volume_normalization,
            VolumeNormalizationMode::ReplayGainAlbum
        );
        assert_eq!(p.normalization_level, NormalizationLevel::Loud);
        assert!(!p.opacity_gradient);
        assert_eq!(p.rounded_mode, RoundedMode::On);
        assert!(!p.view_columns.queue_show_stars);
    }

    /// Read-side: `dump_playback_tab_player_settings` mirrors migrated fields
    /// onto the UI-facing struct. Includes the f64→f32 narrowing on
    /// `scrobble_threshold` — the only non-trivial cast on this tab.
    #[test]
    fn dump_playback_round_trip_copies_migrated_fields() {
        let (mgr, _tmp) = make_test_manager();
        let mut ui = mgr.get_player_settings();

        let mut src = PersistedPlayerSettings::default();
        src.crossfade_enabled = true;
        src.crossfade_duration_secs = 9;
        src.rewind_on_previous = true;
        src.replay_gain_preamp_db = 4.0;
        src.volume_normalization = VolumeNormalizationMode::ReplayGainAlbum;
        src.normalization_level = NormalizationLevel::Loud;
        src.opacity_gradient = false;
        src.rounded_mode = RoundedMode::On;
        src.view_columns.queue_show_stars = false;
        src.scrobble_threshold = 0.75;

        dump_playback_tab_player_settings(&src, &mut ui);

        assert!(ui.crossfade_enabled);
        assert_eq!(ui.crossfade_duration_secs, 9);
        assert!(ui.rewind_on_previous);
        assert_eq!(ui.replay_gain_preamp_db, 4.0);
        assert_eq!(
            ui.volume_normalization,
            VolumeNormalizationMode::ReplayGainAlbum
        );
        assert_eq!(ui.normalization_level, NormalizationLevel::Loud);
        assert!(!ui.opacity_gradient);
        assert_eq!(ui.rounded_mode, RoundedMode::On);
        assert!(!ui.view_columns.queue_show_stars);
        assert!((ui.scrobble_threshold - 0.75_f32).abs() < f32::EPSILON);
    }

    /// Write-side: `write_playback_tab_toml` copies the migrated fields
    /// from the UI-facing struct onto `TomlSettings` for config.toml
    /// serialization. The `scrobble_threshold` field is f32 on both
    /// `LivePlayerSettings` and `TomlSettings` (the f64 only exists on the
    /// redb-backed `PersistedPlayerSettings`) — the write closure is
    /// therefore a plain copy without cast.
    #[test]
    fn write_playback_round_trip_copies_migrated_fields_to_toml() {
        let mut ps = crate::types::player_settings::LivePlayerSettings::default();
        ps.crossfade_enabled = true;
        ps.crossfade_duration_secs = 9;
        ps.rewind_on_previous = true;
        ps.replay_gain_preamp_db = 4.0;
        ps.replay_gain_fallback_db = 1.5;
        ps.replay_gain_fallback_to_agc = true;
        ps.replay_gain_prevent_clipping = false;
        ps.volume_normalization = VolumeNormalizationMode::ReplayGainAlbum;
        ps.normalization_level = NormalizationLevel::Loud;
        ps.opacity_gradient = false;
        ps.rounded_mode = RoundedMode::On;
        ps.view_columns.queue_show_stars = false;
        ps.view_columns.queue_show_album = false;
        ps.view_columns.queue_show_duration = false;
        ps.view_columns.queue_show_love = false;
        ps.scrobble_threshold = 0.75;
        ps.quick_add_to_playlist = true;
        ps.queue_show_default_playlist = true;
        ps.scrobbling_enabled = false;

        let mut ts = TomlSettings::default();
        write_playback_tab_toml(&ps, &mut ts);

        assert!(ts.crossfade_enabled);
        assert_eq!(ts.crossfade_duration_secs, 9);
        assert!(ts.rewind_on_previous);
        assert!((ts.replay_gain_preamp_db - 4.0).abs() < f32::EPSILON);
        assert!((ts.replay_gain_fallback_db - 1.5).abs() < f32::EPSILON);
        assert!(ts.replay_gain_fallback_to_agc);
        assert!(!ts.replay_gain_prevent_clipping);
        assert_eq!(
            ts.volume_normalization,
            VolumeNormalizationMode::ReplayGainAlbum
        );
        assert_eq!(ts.normalization_level, NormalizationLevel::Loud);
        assert!(!ts.opacity_gradient);
        assert_eq!(ts.rounded_mode, RoundedMode::On);
        assert!(!ts.view_columns.queue_show_stars);
        assert!(!ts.view_columns.queue_show_album);
        assert!(!ts.view_columns.queue_show_duration);
        assert!(!ts.view_columns.queue_show_love);
        assert!((ts.scrobble_threshold - 0.75).abs() < f32::EPSILON);
        assert!(ts.quick_add_to_playlist);
        assert!(ts.queue_show_default_playlist);
        assert!(!ts.scrobbling_enabled);
    }

    /// Parity guard: every Playback-tab row's `ui_meta.default` must agree with
    /// the canonical `PersistedPlayerSettings::default()` projected through the
    /// production `dump_playback_tab_player_settings` -> `PlaybackSettingsData`
    /// path (the same projection the live UI builds in `update/settings.rs`).
    ///
    /// This mirrors the live `ResetToDefault` guard (`views/settings/mod.rs`),
    /// which compares `value.display() != default.display()`: on a fresh-default
    /// build the two must be equal so that "restore default" is a correct no-op.
    /// When the macro `ui_meta.default` literals drift away from the struct
    /// `Default` impl (as crossfade did in 7e8dc60), this trips — catching the
    /// whole ui_meta-vs-canonical-default drift class for Playback rows.
    #[test]
    fn ui_meta_defaults_match_persisted_player_settings_defaults() {
        let p = PersistedPlayerSettings::default();
        let mut live = crate::types::player_settings::LivePlayerSettings::default();
        dump_playback_tab_player_settings(&p, &mut live);

        let data = PlaybackSettingsData {
            crossfade_enabled: live.crossfade_enabled,
            bit_perfect: live.bit_perfect.as_label().into(),
            crossfade_duration_secs: i64::from(live.crossfade_duration_secs),
            rewind_on_previous: live.rewind_on_previous,
            volume_normalization: live.volume_normalization.as_label().into(),
            normalization_level: live.normalization_level.as_label().into(),
            replay_gain_preamp_db: live.replay_gain_preamp_db.round() as i64,
            replay_gain_fallback_db: live.replay_gain_fallback_db.round() as i64,
            replay_gain_fallback_to_agc: live.replay_gain_fallback_to_agc,
            replay_gain_prevent_clipping: live.replay_gain_prevent_clipping,
            scrobbling_enabled: live.scrobbling_enabled,
            scrobble_threshold: f64::from(live.scrobble_threshold),
            radio_scrobbling_enabled: live.radio_scrobbling_enabled,
            radio_scrobble_threshold_secs: i64::from(live.radio_scrobble_threshold_secs),
            radio_now_playing_enabled: live.radio_now_playing_enabled,
            // Connection status is sourced from redb in build_settings_view_data,
            // not from LivePlayerSettings — not part of this macro round-trip.
            listenbrainz_source: crate::services::radio_scrobble::source::CredSource::Unset,
            lastfm_credentials_source: crate::services::radio_scrobble::source::CredSource::Unset,
            lastfm_username: String::new().into(),
            quick_add_to_playlist: live.quick_add_to_playlist,
            default_playlist_name: live.default_playlist_name.clone().into(),
            queue_show_default_playlist: live.queue_show_default_playlist,
            rating_reminder_enabled: live.rating_reminder_enabled,
            rating_change_notification_enabled: live.rating_change_notification_enabled,
            rating_reminder_trigger: live.rating_reminder_trigger.as_label().into(),
            rating_reminder_percent: i64::from(live.rating_reminder_percent),
        };

        for e in build_playback_tab_settings_items(&data) {
            if let SettingsEntry::Item(item) = e {
                assert_eq!(
                    item.value.display(),
                    item.default.display(),
                    "ui_meta default for {} disagrees with PersistedPlayerSettings::default()",
                    item.key
                );
            }
        }
    }
}
