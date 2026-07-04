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
            CROSSFADE_MIN_TRACK_MAX_SECS, CROSSFADE_MIN_TRACK_MIN_SECS, CROSSFADE_OFFSET_MAX_SECS,
            CROSSFADE_OFFSET_MIN_SECS, CrossfadeCurve, FADE_SKIP_SECS_MAX, FADE_SKIP_SECS_MIN,
            FadeOnSkip, NormalizationLevel, RatingReminderTrigger, RoundedMode,
            TRANSPORT_FADE_MS_MAX, TRANSPORT_FADE_MS_MIN, VolumeNormalizationMode,
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
                     gapless with no overlap. Mutually exclusive with Bit-Perfect — turning \
                     Crossfade on switches Bit-Perfect off.",
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
        CrossfadeCurveKey {
            key: "general.crossfade_curve",
            value_type: Enum,
            setter: |mgr, v: String| mgr.set_crossfade_curve(CrossfadeCurve::from_label(&v)),
            toml_apply: |ts, p| p.crossfade_curve = ts.crossfade_curve,
            read: |src, out| out.crossfade_curve = src.crossfade_curve,
            write: |ps, ts| ts.crossfade_curve = ps.crossfade_curve,
            ui_meta: {
                label: "Crossfade Curve",
                category: "Playback",
                subtitle: Some(
                    "Equal Power holds the volume steady through the blend — best for different \
                     songs, no mid-fade dip. Constant Gain dips about 3 dB in the middle — \
                     smoother for same-album material. Linear is a plain straight-line fade with \
                     harder ends.",
                ),
                default: "Equal Power",
                options: &["Equal Power", "Constant Gain", "Linear"],
                read_field: |d| d.crossfade_curve.as_ref(),
            },
        },
        CrossfadeMinTrack {
            key: "general.crossfade_min_track",
            value_type: Int,
            setter: |mgr, v: i64| mgr.set_crossfade_min_track(v as u32),
            toml_apply: |ts, p| p.crossfade_min_track_secs = ts.crossfade_min_track_secs,
            read: |src, out| out.crossfade_min_track_secs = src.crossfade_min_track_secs,
            write: |ps, ts| ts.crossfade_min_track_secs = ps.crossfade_min_track_secs,
            ui_meta: {
                label: "Minimum Track Length to Crossfade",
                category: "Playback",
                subtitle: Some(
                    "Tracks shorter than this play gapless instead of crossfading. 0 blends \
                     everything including interludes; 30 keeps segues sharp and only blends \
                     full-length songs.",
                ),
                default: 10_i64,
                min: i64::from(CROSSFADE_MIN_TRACK_MIN_SECS),
                max: i64::from(CROSSFADE_MIN_TRACK_MAX_SECS),
                step: 1_i64,
                unit: "s",
                read_field: |d| d.crossfade_min_track_secs,
            },
        },
        CrossfadeAlbumGapless {
            key: "general.crossfade_album_gapless",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_crossfade_album_gapless(v),
            toml_apply: |ts, p| p.crossfade_album_gapless = ts.crossfade_album_gapless,
            read: |src, out| out.crossfade_album_gapless = src.crossfade_album_gapless,
            write: |ps, ts| ts.crossfade_album_gapless = ps.crossfade_album_gapless,
            ui_meta: {
                label: "Keep Gapless Albums Seamless",
                category: "Playback",
                subtitle: Some(
                    "Skip the blend when the next track continues the same album, so intended \
                     gapless segues stay tight. Crossfade still applies between different \
                     albums, on shuffle, and on compilations.",
                ),
                default: false,
                read_field: |d| d.crossfade_album_gapless,
            },
        },
        // -- Fading (M5) -------------------------------------------------------
        SmoothTrackStarts {
            key: "general.smooth_track_starts",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_smooth_track_starts(v),
            toml_apply: |ts, p| p.smooth_track_starts = ts.smooth_track_starts,
            read: |src, out| out.smooth_track_starts = src.smooth_track_starts,
            write: |ps, ts| ts.smooth_track_starts = ps.smooth_track_starts,
            ui_meta: {
                label: "Smooth Track Starts",
                category: "Fading",
                subtitle: Some(
                    "Ramp up the first ~20 ms of each track to remove the click when a skip or \
                     seek lands mid-waveform. Off restores an instant, honest onset. Bit-perfect \
                     streams always start instantly.",
                ),
                default: true,
                read_field: |d| d.smooth_track_starts,
            },
        },
        FadeOnPause {
            key: "general.fade_on_pause",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_fade_on_pause(v),
            toml_apply: |ts, p| p.fade_on_pause = ts.fade_on_pause,
            read: |src, out| out.fade_on_pause = src.fade_on_pause,
            write: |ps, ts| ts.fade_on_pause = ps.fade_on_pause,
            ui_meta: {
                label: "Fade on Pause / Resume",
                category: "Fading",
                subtitle: Some(
                    "Dip the volume out when pausing and swell back in on resume, instead of \
                     cutting mid-waveform. Off pauses and resumes instantly. Bit-perfect streams \
                     always cut instantly.",
                ),
                default: false,
                read_field: |d| d.fade_on_pause,
            },
        },
        FadePauseMs {
            key: "general.fade_pause_ms",
            value_type: Int,
            setter: |mgr, v: i64| mgr.set_fade_pause_ms(v as u32),
            toml_apply: |ts, p| p.fade_pause_ms = ts.fade_pause_ms,
            read: |src, out| out.fade_pause_ms = src.fade_pause_ms,
            write: |ps, ts| ts.fade_pause_ms = ps.fade_pause_ms,
            ui_meta: {
                label: "Pause Fade Duration",
                category: "Fading",
                subtitle: Some(
                    "20ms barely rounds the cut edge; 500ms is a slow dip and swell.",
                ),
                default: 100_i64,
                min: i64::from(TRANSPORT_FADE_MS_MIN),
                max: i64::from(TRANSPORT_FADE_MS_MAX),
                step: 10_i64,
                unit: "ms",
                read_field: |d| d.fade_pause_ms,
            },
        },
        FadeOnStop {
            key: "general.fade_on_stop",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_fade_on_stop(v),
            toml_apply: |ts, p| p.fade_on_stop = ts.fade_on_stop,
            read: |src, out| out.fade_on_stop = src.fade_on_stop,
            write: |ps, ts| ts.fade_on_stop = ps.fade_on_stop,
            ui_meta: {
                label: "Fade on Stop",
                category: "Fading",
                subtitle: Some(
                    "Ease the sound out when playback stops instead of a hard cut. Track \
                     changes are not affected. Off stops instantly.",
                ),
                default: false,
                read_field: |d| d.fade_on_stop,
            },
        },
        FadeStopMs {
            key: "general.fade_stop_ms",
            value_type: Int,
            setter: |mgr, v: i64| mgr.set_fade_stop_ms(v as u32),
            toml_apply: |ts, p| p.fade_stop_ms = ts.fade_stop_ms,
            read: |src, out| out.fade_stop_ms = src.fade_stop_ms,
            write: |ps, ts| ts.fade_stop_ms = ps.fade_stop_ms,
            ui_meta: {
                label: "Stop Fade Duration",
                category: "Fading",
                subtitle: Some(
                    "20ms barely rounds the cut edge; 500ms is a long ease-out — stopping \
                     waits for it to finish.",
                ),
                default: 100_i64,
                min: i64::from(TRANSPORT_FADE_MS_MIN),
                max: i64::from(TRANSPORT_FADE_MS_MAX),
                step: 10_i64,
                unit: "ms",
                read_field: |d| d.fade_stop_ms,
            },
        },
        FadeRadioTransitions {
            key: "general.fade_radio_transitions",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_fade_radio_transitions(v),
            toml_apply: |ts, p| p.fade_radio_transitions = ts.fade_radio_transitions,
            read: |src, out| out.fade_radio_transitions = src.fade_radio_transitions,
            write: |ps, ts| ts.fade_radio_transitions = ps.fade_radio_transitions,
            ui_meta: {
                label: "Fade Radio Switches",
                category: "Fading",
                subtitle: Some(
                    "Fade out and back in (about a quarter second each way) when starting a \
                     radio station or returning to the queue, instead of a hard cut. The \
                     fade-in waits for the station's first audio. Off switches instantly.",
                ),
                default: false,
                read_field: |d| d.fade_radio_transitions,
            },
        },
        FadeOnSkipKey {
            key: "general.fade_on_skip",
            value_type: Enum,
            setter: |mgr, v: String| mgr.set_fade_on_skip(FadeOnSkip::from_label(&v)),
            toml_apply: |ts, p| p.fade_on_skip = ts.fade_on_skip,
            read: |src, out| out.fade_on_skip = src.fade_on_skip,
            write: |ps, ts| ts.fade_on_skip = ps.fade_on_skip,
            ui_meta: {
                label: "Fade on Skip",
                category: "Fading",
                subtitle: Some(
                    "What Next/Previous does to the sound. Off cuts instantly. Boundary Fade \
                     eases the old track out before the new one starts fresh. Crossfade \
                     overlaps and blends into the skipped-to track, like an automatic track \
                     change — falling back to the boundary fade when a blend is blocked. \
                     Bit-perfect streams always cut instantly.",
                ),
                default: "Off",
                options: &["Off", "Boundary Fade", "Crossfade"],
                read_field: |d| d.fade_on_skip.as_ref(),
            },
        },
        FadeSkipSecs {
            key: "general.fade_skip_secs",
            value_type: Int,
            setter: |mgr, v: i64| mgr.set_fade_skip_secs(v as u32),
            toml_apply: |ts, p| p.fade_skip_secs = ts.fade_skip_secs,
            read: |src, out| out.fade_skip_secs = src.fade_skip_secs,
            write: |ps, ts| ts.fade_skip_secs = ps.fade_skip_secs,
            ui_meta: {
                label: "Skip Fade Duration",
                category: "Fading",
                subtitle: Some(
                    "1s = quick blend on every manual skip, 4s = long overlap. Also the \
                     Boundary Fade ease-out length.",
                ),
                default: 2_i64,
                min: i64::from(FADE_SKIP_SECS_MIN),
                max: i64::from(FADE_SKIP_SECS_MAX),
                step: 1_i64,
                unit: "s",
                read_field: |d| d.fade_skip_secs,
            },
        },
        SkipSilence {
            key: "general.skip_silence",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_skip_silence(v),
            toml_apply: |ts, p| p.skip_silence = ts.skip_silence,
            read: |src, out| out.skip_silence = src.skip_silence,
            write: |ps, ts| ts.skip_silence = ps.skip_silence,
            ui_meta: {
                label: "Skip Silence Between Tracks",
                category: "Fading",
                subtitle: Some(
                    "Skip near-silent endings and lead-ins at track changes: a silent tail \
                     starts the next transition early, and a silent intro is dropped when the \
                     next track was prepared in advance. Off plays every recorded second. \
                     Bit-perfect streams never trim.",
                ),
                default: false,
                read_field: |d| d.skip_silence,
            },
        },
        CrossfadeOffset {
            key: "general.crossfade_offset",
            value_type: Int,
            setter: |mgr, v: i64| mgr.set_crossfade_offset_secs(v as i32),
            toml_apply: |ts, p| p.crossfade_offset_secs = ts.crossfade_offset_secs,
            read: |src, out| out.crossfade_offset_secs = src.crossfade_offset_secs,
            write: |ps, ts| ts.crossfade_offset_secs = ps.crossfade_offset_secs,
            ui_meta: {
                label: "Gap / Overlap Trim",
                category: "Fading",
                subtitle: Some(
                    "0 leaves track changes untouched. -2 starts the crossfade two seconds \
                     early, folding the old track's tail into the blend. +2 holds two seconds \
                     of silence between tracks on gapless joins — a live crossfade overrides \
                     the gap, and seamless-album joins stay tight.",
                ),
                default: 0_i64,
                min: i64::from(CROSSFADE_OFFSET_MIN_SECS),
                max: i64::from(CROSSFADE_OFFSET_MAX_SECS),
                step: 1_i64,
                unit: "s",
                read_field: |d| d.crossfade_offset_secs,
            },
        },
        CrossfadeBarSnap {
            key: "general.crossfade_bar_snap",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_crossfade_bar_snap(v),
            toml_apply: |ts, p| p.crossfade_bar_snap = ts.crossfade_bar_snap,
            read: |src, out| out.crossfade_bar_snap = src.crossfade_bar_snap,
            write: |ps, ts| ts.crossfade_bar_snap = ps.crossfade_bar_snap,
            ui_meta: {
                label: "Snap Crossfade to Musical Bars",
                category: "Fading",
                subtitle: Some(
                    "Round the crossfade to whole bars of the track's tempo so beats line up \
                     through the blend. Needs BPM tags; ignored when a track has none. No \
                     effect between tracks at different tempos.",
                ),
                default: false,
                read_field: |d| d.crossfade_bar_snap,
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
            crossfade_curve: "Equal Power".into(),
            crossfade_min_track_secs: 10,
            crossfade_album_gapless: false,
            smooth_track_starts: true,
            fade_on_pause: false,
            fade_pause_ms: 100,
            fade_on_stop: false,
            fade_stop_ms: 100,
            fade_radio_transitions: false,
            fade_on_skip: "Off".into(),
            fade_skip_secs: 2,
            skip_silence: false,
            crossfade_offset_secs: 0,
            crossfade_bar_snap: false,
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

    /// 30 entries get ui_meta: 8 unconditional Playback rows (crossfade
    /// enable, bit-perfect, crossfade duration, crossfade curve, minimum
    /// track length, keep-gapless-albums, rewind-on-previous, volume
    /// normalization), 11 Fading rows (smooth starts, fade-on-pause + its
    /// duration, fade-on-stop + its duration, fade-on-skip + its duration —
    /// the duration rows are emitted here unconditionally but the UI builder
    /// only splices each in when its enable/mode is on — plus
    /// fade-radio-transitions and the three M8 rows: skip-silence,
    /// gap/overlap trim, bar-snap), 3 Radio Scrobbling, 2 Scrobbling,
    /// 4 Rating Reminder (enable, change-notification, trigger, percentage),
    /// and 2 Playlists.
    /// The 5 conditional AGC/RG knobs and the `default_playlist_name` dialog
    /// row stay hand-written; the lifecycle-only entries (queue column
    /// visibility, opacity_gradient, rounded_mode) emit nothing here. The
    /// Rating Reminder trigger/percentage rows are emitted here
    /// unconditionally but the UI builder (`items_playback.rs`) only splices
    /// them in when the feature is enabled.
    #[test]
    fn build_playback_tab_settings_items_emits_thirty_rows() {
        let data = default_playback_data();
        let entries = build_playback_tab_settings_items(&data);
        assert_eq!(entries.len(), 30);
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

    /// M4 Int round-trip: `general.crossfade_min_track` arrives as `Int`,
    /// the setter clamps + persists, and `get_player_settings()` reports the
    /// new floor — plus the same single-source-of-truth slider-max interlock
    /// the duration slider has (both sides derive from
    /// `CROSSFADE_MIN_TRACK_MAX_SECS`).
    #[test]
    fn dispatch_playback_int_round_trip_crossfade_min_track_with_clamp_interlock() {
        let (mut mgr, _tmp) = make_test_manager();
        assert_eq!(
            mgr.get_player_settings().crossfade_min_track_secs,
            10,
            "default floor must be the historical 10s"
        );

        let result = dispatch_playback_tab_setting(
            "general.crossfade_min_track",
            SettingValue::Int {
                val: 30,
                min: 0,
                max: 60,
                step: 1,
                unit: "s",
            },
            &mut mgr,
        );
        assert!(matches!(
            result,
            Some(Ok(
                crate::types::settings_side_effect::SettingsSideEffect::None
            ))
        ));
        assert_eq!(mgr.get_player_settings().crossfade_min_track_secs, 30);

        // Slider-max interlock: the declared max round-trips unchanged, one
        // step past it clamps down to exactly the declared max.
        let entries = build_playback_tab_settings_items(&default_playback_data());
        let slider_max = entries
            .iter()
            .find_map(|e| match e {
                SettingsEntry::Item(item) if item.key.as_ref() == "general.crossfade_min_track" => {
                    match &item.value {
                        SettingValue::Int { max, .. } => Some(*max),
                        _ => None,
                    }
                }
                _ => None,
            })
            .expect("crossfade_min_track row with an Int value");
        mgr.set_crossfade_min_track(slider_max as u32)
            .expect("set to slider max");
        assert_eq!(
            i64::from(mgr.get_player_settings().crossfade_min_track_secs),
            slider_max,
            "slider max {slider_max}s is silently truncated by the persistence clamp"
        );
        mgr.set_crossfade_min_track(slider_max as u32 + 1)
            .expect("set above slider max");
        assert_eq!(
            i64::from(mgr.get_player_settings().crossfade_min_track_secs),
            slider_max,
            "values above the slider max must clamp to the slider max"
        );
    }

    /// M4 Bool round-trip: `general.crossfade_album_gapless` defaults off
    /// (opt-in); flip via the dispatcher and confirm `get_player_settings()`
    /// reports the new value.
    #[test]
    fn dispatch_playback_bool_round_trip_crossfade_album_gapless() {
        let (mut mgr, _tmp) = make_test_manager();
        assert!(
            !mgr.get_player_settings().crossfade_album_gapless,
            "album-continuity gate must default OFF (opt-in)"
        );

        let result = dispatch_playback_tab_setting(
            "general.crossfade_album_gapless",
            SettingValue::Bool(true),
            &mut mgr,
        );

        assert!(matches!(
            result,
            Some(Ok(
                crate::types::settings_side_effect::SettingsSideEffect::None
            ))
        ));
        assert!(mgr.get_player_settings().crossfade_album_gapless);
    }

    /// M5 Bool round-trips: the three Fading enables dispatch through the
    /// table and land on `get_player_settings()`. `smooth_track_starts`
    /// defaults ON (M2's ramp is the shipped default); the two fade enables
    /// default OFF (opt-in — not among the pre-authorized audible changes).
    #[test]
    fn dispatch_playback_bool_round_trips_fading_enables() {
        let (mut mgr, _tmp) = make_test_manager();
        assert!(
            mgr.get_player_settings().smooth_track_starts,
            "smooth_track_starts must default ON"
        );
        assert!(
            !mgr.get_player_settings().fade_on_pause,
            "fade_on_pause must default OFF (opt-in)"
        );
        assert!(
            !mgr.get_player_settings().fade_on_stop,
            "fade_on_stop must default OFF (opt-in)"
        );
        assert!(
            !mgr.get_player_settings().fade_radio_transitions,
            "fade_radio_transitions must default OFF (opt-in — M6)"
        );

        type Read = fn(&crate::types::player_settings::LivePlayerSettings) -> bool;
        let cases: [(&str, Read); 4] = [
            ("general.smooth_track_starts", |ps| ps.smooth_track_starts),
            ("general.fade_on_pause", |ps| ps.fade_on_pause),
            ("general.fade_on_stop", |ps| ps.fade_on_stop),
            ("general.fade_radio_transitions", |ps| {
                ps.fade_radio_transitions
            }),
        ];
        for (key, read) in cases {
            // Flip each away from its default.
            let flipped = key != "general.smooth_track_starts";
            let result = dispatch_playback_tab_setting(key, SettingValue::Bool(flipped), &mut mgr);
            assert!(
                matches!(
                    result,
                    Some(Ok(
                        crate::types::settings_side_effect::SettingsSideEffect::None
                    ))
                ),
                "{key} must dispatch through the playback table"
            );
            assert_eq!(
                read(&mgr.get_player_settings()),
                flipped,
                "{key} must round-trip"
            );
        }
    }

    /// M5 Int round-trip with the slider-max clamp interlock (same contract
    /// as crossfade duration / min-track): both fade duration sliders'
    /// declared bounds must equal what their setters persist.
    #[test]
    fn dispatch_playback_int_round_trips_fade_durations_with_clamp_interlock() {
        let (mut mgr, _tmp) = make_test_manager();
        assert_eq!(mgr.get_player_settings().fade_pause_ms, 100);
        assert_eq!(mgr.get_player_settings().fade_stop_ms, 100);

        let entries = build_playback_tab_settings_items(&default_playback_data());
        for key in ["general.fade_pause_ms", "general.fade_stop_ms"] {
            let result = dispatch_playback_tab_setting(
                key,
                SettingValue::Int {
                    val: 250,
                    min: 20,
                    max: 500,
                    step: 10,
                    unit: "ms",
                },
                &mut mgr,
            );
            assert!(
                matches!(
                    result,
                    Some(Ok(
                        crate::types::settings_side_effect::SettingsSideEffect::None
                    ))
                ),
                "{key} must dispatch through the playback table"
            );

            let (slider_min, slider_max) = entries
                .iter()
                .find_map(|e| match e {
                    SettingsEntry::Item(item) if item.key.as_ref() == key => match &item.value {
                        SettingValue::Int { min, max, .. } => Some((*min, *max)),
                        _ => None,
                    },
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{key} row with an Int value"));
            assert_eq!(
                slider_min,
                i64::from(crate::types::player_settings::TRANSPORT_FADE_MS_MIN)
            );
            assert_eq!(
                slider_max,
                i64::from(crate::types::player_settings::TRANSPORT_FADE_MS_MAX)
            );

            // The slider max round-trips unchanged; one step past clamps down.
            let set = |mgr: &mut SettingsManager, v: u32| match key {
                "general.fade_pause_ms" => mgr.set_fade_pause_ms(v),
                _ => mgr.set_fade_stop_ms(v),
            };
            let get = |mgr: &SettingsManager| match key {
                "general.fade_pause_ms" => mgr.get_player_settings().fade_pause_ms,
                _ => mgr.get_player_settings().fade_stop_ms,
            };
            set(&mut mgr, slider_max as u32).expect("set to slider max");
            assert_eq!(i64::from(get(&mgr)), slider_max);
            set(&mut mgr, slider_max as u32 + 1).expect("set above slider max");
            assert_eq!(
                i64::from(get(&mgr)),
                slider_max,
                "{key}: values above the slider max must clamp to the slider max"
            );
        }
        assert_eq!(mgr.get_player_settings().fade_pause_ms, 500);
    }

    #[test]
    fn tab_playback_contains_recognizes_declared_keys() {
        assert!(tab_playback_contains("general.crossfade_enabled"));
        assert!(tab_playback_contains("general.crossfade_curve"));
        assert!(tab_playback_contains("general.crossfade_min_track"));
        assert!(tab_playback_contains("general.crossfade_album_gapless"));
        assert!(tab_playback_contains("general.smooth_track_starts"));
        assert!(tab_playback_contains("general.fade_on_pause"));
        assert!(tab_playback_contains("general.fade_pause_ms"));
        assert!(tab_playback_contains("general.fade_on_stop"));
        assert!(tab_playback_contains("general.fade_stop_ms"));
        assert!(tab_playback_contains("general.fade_radio_transitions"));
        assert!(tab_playback_contains("general.fade_on_skip"));
        assert!(tab_playback_contains("general.fade_skip_secs"));
        assert!(tab_playback_contains("general.skip_silence"));
        assert!(tab_playback_contains("general.crossfade_offset"));
        assert!(tab_playback_contains("general.crossfade_bar_snap"));
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

    /// M8 Bool round-trips: `general.skip_silence` and
    /// `general.crossfade_bar_snap` both default OFF (opt-in) and dispatch
    /// through the table onto `get_player_settings()`.
    #[test]
    fn dispatch_playback_bool_round_trips_m8_gates() {
        let (mut mgr, _tmp) = make_test_manager();
        assert!(
            !mgr.get_player_settings().skip_silence,
            "skip_silence must default OFF (opt-in)"
        );
        assert!(
            !mgr.get_player_settings().crossfade_bar_snap,
            "crossfade_bar_snap must default OFF (opt-in)"
        );

        for key in ["general.skip_silence", "general.crossfade_bar_snap"] {
            let result = dispatch_playback_tab_setting(key, SettingValue::Bool(true), &mut mgr);
            assert!(
                matches!(
                    result,
                    Some(Ok(
                        crate::types::settings_side_effect::SettingsSideEffect::None
                    ))
                ),
                "{key} must dispatch"
            );
        }
        assert!(mgr.get_player_settings().skip_silence);
        assert!(mgr.get_player_settings().crossfade_bar_snap);
    }

    /// M8 Int round-trip for the SIGNED `general.crossfade_offset` slider:
    /// negative values round-trip (the first negative Int row in the table),
    /// and both slider ends match the persistence clamp exactly (the M4
    /// single-source-of-truth interlock).
    #[test]
    fn dispatch_playback_int_round_trip_crossfade_offset_with_signed_clamp() {
        let (mut mgr, _tmp) = make_test_manager();
        assert_eq!(
            mgr.get_player_settings().crossfade_offset_secs,
            0,
            "offset must default to 0 (untouched transitions)"
        );

        let result = dispatch_playback_tab_setting(
            "general.crossfade_offset",
            SettingValue::Int {
                val: -2,
                min: -2,
                max: 2,
                step: 1,
                unit: "s",
            },
            &mut mgr,
        );
        assert!(matches!(
            result,
            Some(Ok(
                crate::types::settings_side_effect::SettingsSideEffect::None
            ))
        ));
        assert_eq!(mgr.get_player_settings().crossfade_offset_secs, -2);

        // Slider-bounds interlock, both signs: the declared min/max round-trip
        // unchanged, one step past either end clamps to exactly that end.
        let entries = build_playback_tab_settings_items(&default_playback_data());
        let (slider_min, slider_max) = entries
            .iter()
            .find_map(|e| match e {
                SettingsEntry::Item(item) if item.key.as_ref() == "general.crossfade_offset" => {
                    match &item.value {
                        SettingValue::Int { min, max, .. } => Some((*min, *max)),
                        _ => None,
                    }
                }
                _ => None,
            })
            .expect("crossfade_offset row with an Int value");
        mgr.set_crossfade_offset_secs(slider_min as i32)
            .expect("set to slider min");
        assert_eq!(
            i64::from(mgr.get_player_settings().crossfade_offset_secs),
            slider_min
        );
        mgr.set_crossfade_offset_secs(slider_min as i32 - 1)
            .expect("set below slider min");
        assert_eq!(
            i64::from(mgr.get_player_settings().crossfade_offset_secs),
            slider_min,
            "values below the slider min must clamp to the slider min"
        );
        mgr.set_crossfade_offset_secs(slider_max as i32 + 1)
            .expect("set above slider max");
        assert_eq!(
            i64::from(mgr.get_player_settings().crossfade_offset_secs),
            slider_max,
            "values above the slider max must clamp to the slider max"
        );
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

    /// Enum round-trip (M7): `general.fade_on_skip` arrives as `Enum` with a
    /// label, the setter parses it to `FadeOnSkip`, and
    /// `get_player_settings()` reports the matching enum variant. Default is
    /// Off (opt-in — a skip fade is an audible behavior change).
    #[test]
    fn dispatch_playback_enum_round_trip_fade_on_skip() {
        use crate::types::player_settings::FadeOnSkip;
        let (mut mgr, _tmp) = make_test_manager();
        assert_eq!(
            mgr.get_player_settings().fade_on_skip,
            FadeOnSkip::Off,
            "fade_on_skip must default Off (opt-in)"
        );

        let result = dispatch_playback_tab_setting(
            "general.fade_on_skip",
            SettingValue::Enum {
                val: "Crossfade".to_string(),
                options: vec!["Off", "Boundary Fade", "Crossfade"],
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
            mgr.get_player_settings().fade_on_skip,
            FadeOnSkip::Crossfade
        );
    }

    /// M7 Int round-trip with the slider-max clamp interlock (same contract
    /// as the pause/stop fade durations): the skip-fade slider's declared
    /// bounds must equal what `set_fade_skip_secs` persists.
    #[test]
    fn dispatch_playback_int_round_trip_fade_skip_secs_with_clamp_interlock() {
        let (mut mgr, _tmp) = make_test_manager();
        assert_eq!(
            mgr.get_player_settings().fade_skip_secs,
            crate::types::player_settings::FADE_SKIP_SECS_DEFAULT
        );

        let result = dispatch_playback_tab_setting(
            "general.fade_skip_secs",
            SettingValue::Int {
                val: 3,
                min: 1,
                max: 4,
                step: 1,
                unit: "s",
            },
            &mut mgr,
        );
        assert!(matches!(
            result,
            Some(Ok(
                crate::types::settings_side_effect::SettingsSideEffect::None
            ))
        ));
        assert_eq!(mgr.get_player_settings().fade_skip_secs, 3);

        let entries = build_playback_tab_settings_items(&default_playback_data());
        let (slider_min, slider_max) = entries
            .iter()
            .find_map(|e| match e {
                SettingsEntry::Item(item) if item.key.as_ref() == "general.fade_skip_secs" => {
                    match &item.value {
                        SettingValue::Int { min, max, .. } => Some((*min, *max)),
                        _ => None,
                    }
                }
                _ => None,
            })
            .expect("fade_skip_secs row with an Int value");
        assert_eq!(
            slider_min,
            i64::from(crate::types::player_settings::FADE_SKIP_SECS_MIN)
        );
        assert_eq!(
            slider_max,
            i64::from(crate::types::player_settings::FADE_SKIP_SECS_MAX)
        );

        // The slider max round-trips unchanged; one step past clamps down.
        mgr.set_fade_skip_secs(slider_max as u32)
            .expect("set to slider max");
        assert_eq!(i64::from(mgr.get_player_settings().fade_skip_secs), {
            slider_max
        });
        mgr.set_fade_skip_secs(slider_max as u32 + 1)
            .expect("set above slider max");
        assert_eq!(
            i64::from(mgr.get_player_settings().fade_skip_secs),
            slider_max,
            "values above the slider max must clamp to the slider max"
        );
    }

    /// Enum round-trip (M3): `general.crossfade_curve` arrives as `Enum` with
    /// a label, the setter parses it to `CrossfadeCurve`, and
    /// `get_player_settings()` reports the matching enum variant — the
    /// config-write half of the curve setting's round trip.
    #[test]
    fn dispatch_playback_enum_round_trip_crossfade_curve() {
        use crate::types::player_settings::CrossfadeCurve;
        let (mut mgr, _tmp) = make_test_manager();
        assert_eq!(
            mgr.get_player_settings().crossfade_curve,
            CrossfadeCurve::EqualPower,
            "default must be Equal Power"
        );

        let result = dispatch_playback_tab_setting(
            "general.crossfade_curve",
            SettingValue::Enum {
                val: "Constant Gain".to_string(),
                options: vec!["Equal Power", "Constant Gain", "Linear"],
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
            mgr.get_player_settings().crossfade_curve,
            CrossfadeCurve::ConstantGain
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
        ts.crossfade_curve = crate::types::player_settings::CrossfadeCurve::Linear;
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
        assert_eq!(
            p.crossfade_curve,
            crate::types::player_settings::CrossfadeCurve::Linear
        );
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
        src.crossfade_curve = crate::types::player_settings::CrossfadeCurve::ConstantGain;
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
        assert_eq!(
            ui.crossfade_curve,
            crate::types::player_settings::CrossfadeCurve::ConstantGain
        );
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
        ps.crossfade_curve = crate::types::player_settings::CrossfadeCurve::Linear;
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
        assert_eq!(
            ts.crossfade_curve,
            crate::types::player_settings::CrossfadeCurve::Linear
        );
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
            crossfade_curve: live.crossfade_curve.as_label().into(),
            crossfade_min_track_secs: i64::from(live.crossfade_min_track_secs),
            crossfade_album_gapless: live.crossfade_album_gapless,
            smooth_track_starts: live.smooth_track_starts,
            fade_on_pause: live.fade_on_pause,
            fade_pause_ms: i64::from(live.fade_pause_ms),
            fade_on_stop: live.fade_on_stop,
            fade_stop_ms: i64::from(live.fade_stop_ms),
            fade_radio_transitions: live.fade_radio_transitions,
            fade_on_skip: live.fade_on_skip.as_label().into(),
            fade_skip_secs: i64::from(live.fade_skip_secs),
            skip_silence: live.skip_silence,
            crossfade_offset_secs: i64::from(live.crossfade_offset_secs),
            crossfade_bar_snap: live.crossfade_bar_snap,
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
