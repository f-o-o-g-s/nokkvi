//! Playback normalization settings — AGC level and mode (off / AGC / ReplayGain).

use serde::{Deserialize, Serialize};

use crate::define_labeled_enum;

/// Crossfade duration bounds, in seconds. Single source of truth shared by the
/// enforcement clamp in `SettingsManager::set_crossfade_duration` and the
/// slider's declared `min`/`max` in the playback settings table, so the slider
/// can never offer a value the setter would silently truncate.
pub const CROSSFADE_DURATION_MIN_SECS: u32 = 1;
pub const CROSSFADE_DURATION_MAX_SECS: u32 = 12;

/// Minimum-track-length bounds and default, in seconds — the floor below
/// which a transition plays gapless instead of crossfading. Single source of
/// truth shared by the enforcement clamp in
/// `SettingsManager::set_crossfade_min_track`, the slider's declared
/// `min`/`max` in the playback settings table, and the renderer's
/// arm-gate default. 0 = blend everything with a known duration; the
/// 10 s default preserves the historical hardcoded floor.
pub const CROSSFADE_MIN_TRACK_MIN_SECS: u32 = 0;
pub const CROSSFADE_MIN_TRACK_MAX_SECS: u32 = 60;
pub const CROSSFADE_MIN_TRACK_DEFAULT_SECS: u32 = 10;

/// Transport-fade (pause/resume/stop ramp) duration bounds and default, in
/// milliseconds. Single source of truth shared by the enforcement clamps in
/// `SettingsManager::set_fade_pause_ms` / `set_fade_stop_ms`, the sliders'
/// declared `min`/`max` in the playback settings table, the engine's
/// defensive clamp in `set_transport_fades`, and the renderer's seeded
/// default. 20 ms barely rounds the cut edge; 500 ms is a slow dip/swell.
pub const TRANSPORT_FADE_MS_MIN: u32 = 20;
pub const TRANSPORT_FADE_MS_MAX: u32 = 500;
pub const TRANSPORT_FADE_MS_DEFAULT: u32 = 100;

/// M8 "Gap / Overlap Trim" bounds, in seconds. Single source of truth shared
/// by the enforcement clamp in `SettingsManager::set_crossfade_offset_secs`,
/// the slider's declared `min`/`max` in the playback settings table, and the
/// engine's defensive clamp in `set_crossfade_offset`. Negative = the blend
/// starts early (overlap trims the outgoing tail); positive = silence held
/// between tracks; 0 = untouched transitions.
pub const CROSSFADE_OFFSET_MIN_SECS: i32 = -2;
pub const CROSSFADE_OFFSET_MAX_SECS: i32 = 2;

/// "Fade on Skip" duration bounds and default, in seconds (M7). Single
/// source of truth shared by the enforcement clamp in
/// `SettingsManager::set_fade_skip_secs`, the slider's declared `min`/`max`
/// in the playback settings table, and the engine's defensive clamp in
/// `set_skip_fade`. Used for both modes: the skip-crossfade overlap length
/// and the Boundary Fade ease-out length.
pub const FADE_SKIP_SECS_MIN: u32 = 1;
pub const FADE_SKIP_SECS_MAX: u32 = 4;
pub const FADE_SKIP_SECS_DEFAULT: u32 = 2;

define_labeled_enum! {
    /// Volume normalization level — controls the AGC target loudness.
    ///
    /// Maps to rodio's `AutomaticGainControlSettings::target_level`.
    /// Serializes to lowercase strings for redb storage.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum NormalizationLevel {
        /// Reduced loudness, maximum headroom (target_level = 0.6)
        Quiet { label: "Quiet", wire: "quiet" },
        /// Maintain original perceived level (target_level = 1.0)
        #[default]
        Normal { label: "Normal", wire: "normal" },
        /// Boost quiet tracks more aggressively (target_level = 1.4)
        Loud { label: "Loud", wire: "loud" },
    }
}

impl NormalizationLevel {
    /// AGC target level for this normalization level.
    pub fn target_level(self) -> f32 {
        match self {
            Self::Quiet => 0.6,
            Self::Normal => 1.0,
            Self::Loud => 1.4,
        }
    }
}

define_labeled_enum! {
    /// Volume normalization mode — selects between off, real-time AGC, or
    /// static ReplayGain (track or album scope).
    ///
    /// AGC is rodio's `automatic_gain_control` source; ReplayGain modes use
    /// pre-computed loudness tags (`replay_gain.track_gain` /
    /// `replay_gain.album_gain`) read from the Subsonic API.
    ///
    /// Serializes to snake_case strings for redb storage.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum VolumeNormalizationMode {
        /// No normalization. Decoded audio passes through unchanged.
        #[default]
        Off { label: "Off", wire: "off" },
        /// Real-time automatic gain control.
        Agc { label: "AGC", wire: "agc" },
        /// Static gain from per-track ReplayGain tag.
        ReplayGainTrack { label: "ReplayGain (Track)", wire: "replay_gain_track" },
        /// Static gain from per-album ReplayGain tag (preserves within-album dynamics).
        ReplayGainAlbum { label: "ReplayGain (Album)", wire: "replay_gain_album" },
    }
}

impl VolumeNormalizationMode {
    pub fn is_replay_gain(self) -> bool {
        matches!(self, Self::ReplayGainTrack | Self::ReplayGainAlbum)
    }

    pub fn prefers_album(self) -> bool {
        matches!(self, Self::ReplayGainAlbum)
    }
}

define_labeled_enum! {
    /// When the rate-this-track desktop reminder fires.
    ///
    /// `OnScrobble` fires the instant the server confirms a play (the scrobble
    /// submission lands), which means the listener genuinely heard most of the
    /// track. `PercentagePlayed` fires once a configurable fraction of the
    /// track has elapsed (position-based, see `rating_reminder_percent`).
    ///
    /// Serializes to snake_case strings for redb storage.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum RatingReminderTrigger {
        /// Fire when the track's play is confirmed (scrobbled) by the server.
        #[default]
        OnScrobble { label: "On Scrobble", wire: "on_scrobble" },
        /// Fire once a configurable percentage of the track has played.
        PercentagePlayed { label: "Percentage Played", wire: "percentage_played" },
    }
}

impl RatingReminderTrigger {
    /// Whether this trigger fires off the position-based percentage threshold
    /// (as opposed to the scrobble-confirmed edge).
    pub fn is_percentage(self) -> bool {
        matches!(self, Self::PercentagePlayed)
    }

    /// Whether this trigger fires off the scrobble-confirmed edge.
    pub fn is_scrobble(self) -> bool {
        matches!(self, Self::OnScrobble)
    }
}

define_labeled_enum! {
    /// Crossfade gain-curve shape — how the outgoing/incoming gains sweep
    /// across the blend.
    ///
    /// `EqualPower` (the default) is the true equal-power pair `cos`/`sin`
    /// (first power): the gains' POWER sum (`out² + in²`) is 1 at every
    /// point, so perceived loudness holds steady through the middle of the
    /// fade for uncorrelated material (i.e. two different songs — the normal
    /// track-boundary case). `ConstantGain` is the historical `cos²`/`sin²`
    /// pair whose AMPLITUDE sum (`out + in`) is 1 — equal-power only for
    /// correlated content, it dips ~3 dB at the midpoint on uncorrelated
    /// material (a softer center some prefer for same-album blends).
    /// `Linear` is a plain straight-line ramp with harder ends.
    ///
    /// All three curves cross at progress 0.5 (the visualizer-handoff point)
    /// and hit exactly 1.0 at their full-gain endpoints (which the
    /// bit-perfect unity snap depends on — see `fade_gains`).
    ///
    /// Serializes to snake_case strings for redb storage.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum CrossfadeCurve {
        /// True equal-power cos/sin — flat loudness for uncorrelated tracks.
        #[default]
        EqualPower { label: "Equal Power", wire: "equal_power" },
        /// cos²/sin² constant amplitude-sum — softer center, ~3 dB midpoint dip.
        ConstantGain { label: "Constant Gain", wire: "constant_gain" },
        /// Straight-line ramp — constant amplitude-sum with harder ends.
        Linear { label: "Linear", wire: "linear" },
    }
}

impl CrossfadeCurve {
    /// Cycle to the next curve: EqualPower → ConstantGain → Linear →
    /// EqualPower. Mirrors [`BitPerfectMode::next`]'s shape (the settings row
    /// cycles via option badges; this exists for a future cycle hotkey).
    pub fn next(self) -> Self {
        match self {
            Self::EqualPower => Self::ConstantGain,
            Self::ConstantGain => Self::Linear,
            Self::Linear => Self::EqualPower,
        }
    }
}

define_labeled_enum! {
    /// "Fade on Skip" mode (M7) — what a manual Next/Previous does to the
    /// sound instead of a hard cut.
    ///
    /// `Off` keeps the historical instant cut. `BoundaryFade` ramps the
    /// outgoing track to silence over `fade_skip_secs` before the hard load
    /// (single stream, no overlap — the M2 onset ramp then softens the
    /// incoming edge). `Crossfade` overlaps and blends the outgoing into the
    /// skipped-to track like an automatic track change, falling back to the
    /// boundary fade (or the plain cut) when a blend is blocked — format
    /// mismatch under bit-perfect, tracks under the minimum-length floor, or
    /// nothing audibly playing.
    ///
    /// Serializes to snake_case strings for redb storage.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum FadeOnSkip {
        /// Manual skips hard-cut instantly (the historical behavior).
        #[default]
        Off { label: "Off", wire: "off" },
        /// Ease the outgoing track out, then start the next one fresh.
        BoundaryFade { label: "Boundary Fade", wire: "boundary_fade" },
        /// Overlap and blend the outgoing track into the skipped-to track.
        Crossfade { label: "Crossfade", wire: "crossfade" },
    }
}

define_labeled_enum! {
    /// Bit-perfect output mode — three states cycled by the player-bar button
    /// (Off → Strict → Relaxed → Off).
    ///
    /// `Strict` and `Relaxed` both build bit-perfect streams (EQ + software
    /// volume + limiter bypassed, the DAC fed each track at its native rate).
    /// They differ ONLY at track transitions:
    /// - `Strict` hard-cuts every transition — a crossfade blends two streams
    ///   with a gain envelope, which can never be bit-perfect.
    /// - `Relaxed` allows a crossfade between adjacent tracks that share the
    ///   same sample rate AND channel count (the few-second blend itself is not
    ///   bit-perfect; cross-rate / cross-channel changes still hard-cut, since
    ///   the DAC can't re-clock mid-blend without resampling the incoming
    ///   track). A crossfade only fires when the user's Crossfade setting is on.
    ///
    /// Serializes to snake_case strings for redb storage. Legacy `bit_perfect`
    /// records held a bool (`true`/`false`); a custom deserializer maps
    /// `true → Strict`, `false → Off` (see `settings.rs` / `toml_settings.rs`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum BitPerfectMode {
        /// Normal DSP path: EQ, software volume, limiter, and crossfade active.
        #[default]
        Off { label: "Off", wire: "off" },
        /// Untouched samples to the DAC; every track change hard-cuts.
        Strict { label: "Strict", wire: "strict" },
        /// Untouched samples, but same-rate tracks may crossfade.
        Relaxed { label: "Relaxed", wire: "relaxed" },
    }
}

impl BitPerfectMode {
    /// Whether streams should be built bit-perfect (DSP bypass + native-rate
    /// sink). True for both `Strict` and `Relaxed`; only `Off` uses the normal
    /// DSP path.
    pub fn builds_bit_perfect(self) -> bool {
        !matches!(self, Self::Off)
    }

    /// Whether this mode permits a crossfade between tracks that share an audio
    /// format (same sample rate + channel count). Only `Relaxed`. `Strict`
    /// hard-cuts everything; `Off` defers to the normal crossfade path.
    pub fn allows_relaxed_crossfade(self) -> bool {
        matches!(self, Self::Relaxed)
    }

    /// Cycle to the next mode for the player-bar button: Off → Strict →
    /// Relaxed → Off.
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Strict,
            Self::Strict => Self::Relaxed,
            Self::Relaxed => Self::Off,
        }
    }
}

/// Field-level shim used by `#[serde(deserialize_with = ...)]` on the
/// `bit_perfect` fields of `PersistedPlayerSettings` and `TomlSettings`.
///
/// Accepts the new enum wire format (`"off"` / `"strict"` / `"relaxed"`) and
/// the legacy bool from the pre-tri-state era (`true` → `Strict`,
/// `false` → `Off`) in the same field, so upgrading does not reset users'
/// existing preference. Mirrors [`deserialize_rounded_mode_with_bool_compat`].
///
/// [`deserialize_rounded_mode_with_bool_compat`]: super::deserialize_rounded_mode_with_bool_compat
pub fn deserialize_bit_perfect_with_bool_compat<'de, D>(
    deserializer: D,
) -> Result<BitPerfectMode, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Repr {
        Bool(bool),
        Mode(BitPerfectMode),
    }
    match Repr::deserialize(deserializer)? {
        Repr::Bool(true) => Ok(BitPerfectMode::Strict),
        Repr::Bool(false) => Ok(BitPerfectMode::Off),
        Repr::Mode(mode) => Ok(mode),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_normalization_mode_default_is_off() {
        assert_eq!(
            VolumeNormalizationMode::default(),
            VolumeNormalizationMode::Off
        );
    }

    #[test]
    fn volume_normalization_mode_serde_roundtrip() {
        let modes = [
            VolumeNormalizationMode::Off,
            VolumeNormalizationMode::Agc,
            VolumeNormalizationMode::ReplayGainTrack,
            VolumeNormalizationMode::ReplayGainAlbum,
        ];
        for mode in modes {
            let json = serde_json::to_string(&mode).unwrap();
            let deserialized: VolumeNormalizationMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, deserialized);
        }
    }

    #[test]
    fn volume_normalization_mode_serializes_to_snake_case() {
        assert_eq!(
            serde_json::to_string(&VolumeNormalizationMode::Off).unwrap(),
            "\"off\""
        );
        assert_eq!(
            serde_json::to_string(&VolumeNormalizationMode::Agc).unwrap(),
            "\"agc\""
        );
        assert_eq!(
            serde_json::to_string(&VolumeNormalizationMode::ReplayGainTrack).unwrap(),
            "\"replay_gain_track\""
        );
        assert_eq!(
            serde_json::to_string(&VolumeNormalizationMode::ReplayGainAlbum).unwrap(),
            "\"replay_gain_album\""
        );
    }

    #[test]
    fn volume_normalization_mode_label_roundtrip() {
        for mode in [
            VolumeNormalizationMode::Off,
            VolumeNormalizationMode::Agc,
            VolumeNormalizationMode::ReplayGainTrack,
            VolumeNormalizationMode::ReplayGainAlbum,
        ] {
            assert_eq!(VolumeNormalizationMode::from_label(mode.as_label()), mode);
        }
    }

    #[test]
    fn volume_normalization_mode_classifiers() {
        assert!(VolumeNormalizationMode::ReplayGainTrack.is_replay_gain());
        assert!(VolumeNormalizationMode::ReplayGainAlbum.is_replay_gain());
        assert!(VolumeNormalizationMode::ReplayGainAlbum.prefers_album());
        assert!(!VolumeNormalizationMode::ReplayGainTrack.prefers_album());
    }

    #[test]
    fn crossfade_curve_default_is_equal_power() {
        assert_eq!(CrossfadeCurve::default(), CrossfadeCurve::EqualPower);
    }

    #[test]
    fn crossfade_curve_cycles_equal_power_constant_gain_linear() {
        assert_eq!(
            CrossfadeCurve::EqualPower.next(),
            CrossfadeCurve::ConstantGain
        );
        assert_eq!(CrossfadeCurve::ConstantGain.next(), CrossfadeCurve::Linear);
        assert_eq!(CrossfadeCurve::Linear.next(), CrossfadeCurve::EqualPower);
    }

    #[test]
    fn crossfade_curve_serde_roundtrip_and_snake_case() {
        for curve in [
            CrossfadeCurve::EqualPower,
            CrossfadeCurve::ConstantGain,
            CrossfadeCurve::Linear,
        ] {
            let json = serde_json::to_string(&curve).unwrap();
            let back: CrossfadeCurve = serde_json::from_str(&json).unwrap();
            assert_eq!(curve, back);
        }
        assert_eq!(
            serde_json::to_string(&CrossfadeCurve::EqualPower).unwrap(),
            "\"equal_power\""
        );
        assert_eq!(
            serde_json::to_string(&CrossfadeCurve::ConstantGain).unwrap(),
            "\"constant_gain\""
        );
    }

    #[test]
    fn crossfade_curve_label_roundtrip() {
        for curve in [
            CrossfadeCurve::EqualPower,
            CrossfadeCurve::ConstantGain,
            CrossfadeCurve::Linear,
        ] {
            assert_eq!(CrossfadeCurve::from_label(curve.as_label()), curve);
        }
    }

    #[test]
    fn fade_on_skip_default_is_off() {
        assert_eq!(FadeOnSkip::default(), FadeOnSkip::Off);
    }

    #[test]
    fn fade_on_skip_serde_roundtrip_and_snake_case() {
        for mode in [
            FadeOnSkip::Off,
            FadeOnSkip::BoundaryFade,
            FadeOnSkip::Crossfade,
        ] {
            let json = serde_json::to_string(&mode).unwrap();
            let back: FadeOnSkip = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, back);
        }
        assert_eq!(
            serde_json::to_string(&FadeOnSkip::BoundaryFade).unwrap(),
            "\"boundary_fade\""
        );
    }

    #[test]
    fn fade_on_skip_label_roundtrip() {
        for mode in [
            FadeOnSkip::Off,
            FadeOnSkip::BoundaryFade,
            FadeOnSkip::Crossfade,
        ] {
            assert_eq!(FadeOnSkip::from_label(mode.as_label()), mode);
        }
    }

    #[test]
    fn bit_perfect_mode_default_is_off() {
        assert_eq!(BitPerfectMode::default(), BitPerfectMode::Off);
    }

    #[test]
    fn bit_perfect_mode_cycles_off_strict_relaxed_off() {
        assert_eq!(BitPerfectMode::Off.next(), BitPerfectMode::Strict);
        assert_eq!(BitPerfectMode::Strict.next(), BitPerfectMode::Relaxed);
        assert_eq!(BitPerfectMode::Relaxed.next(), BitPerfectMode::Off);
    }

    #[test]
    fn bit_perfect_mode_builds_bit_perfect_for_strict_and_relaxed_only() {
        assert!(!BitPerfectMode::Off.builds_bit_perfect());
        assert!(BitPerfectMode::Strict.builds_bit_perfect());
        assert!(BitPerfectMode::Relaxed.builds_bit_perfect());
    }

    #[test]
    fn bit_perfect_mode_allows_relaxed_crossfade_for_relaxed_only() {
        assert!(!BitPerfectMode::Off.allows_relaxed_crossfade());
        assert!(!BitPerfectMode::Strict.allows_relaxed_crossfade());
        assert!(BitPerfectMode::Relaxed.allows_relaxed_crossfade());
    }

    #[test]
    fn bit_perfect_mode_serde_roundtrip_and_snake_case() {
        for mode in [
            BitPerfectMode::Off,
            BitPerfectMode::Strict,
            BitPerfectMode::Relaxed,
        ] {
            let json = serde_json::to_string(&mode).unwrap();
            let back: BitPerfectMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, back);
        }
        assert_eq!(
            serde_json::to_string(&BitPerfectMode::Relaxed).unwrap(),
            "\"relaxed\""
        );
    }

    #[test]
    fn bit_perfect_mode_label_roundtrip() {
        for mode in [
            BitPerfectMode::Off,
            BitPerfectMode::Strict,
            BitPerfectMode::Relaxed,
        ] {
            assert_eq!(BitPerfectMode::from_label(mode.as_label()), mode);
        }
    }

    #[derive(Deserialize)]
    struct BitPerfectWrapper {
        #[serde(deserialize_with = "deserialize_bit_perfect_with_bool_compat")]
        bit_perfect: BitPerfectMode,
    }

    #[test]
    fn legacy_bit_perfect_bool_true_loads_as_strict() {
        let w: BitPerfectWrapper = serde_json::from_str(r#"{"bit_perfect": true}"#).unwrap();
        assert_eq!(w.bit_perfect, BitPerfectMode::Strict);
    }

    #[test]
    fn legacy_bit_perfect_bool_false_loads_as_off() {
        let w: BitPerfectWrapper = serde_json::from_str(r#"{"bit_perfect": false}"#).unwrap();
        assert_eq!(w.bit_perfect, BitPerfectMode::Off);
    }

    #[test]
    fn new_bit_perfect_string_loads_as_mode() {
        let w: BitPerfectWrapper = serde_json::from_str(r#"{"bit_perfect": "relaxed"}"#).unwrap();
        assert_eq!(w.bit_perfect, BitPerfectMode::Relaxed);
    }

    #[test]
    fn legacy_bit_perfect_bool_compat_roundtrips_through_toml() {
        // config.toml stored a bool pre-migration; confirm the same shim works
        // under the toml deserializer, not just serde_json.
        let w: BitPerfectWrapper = toml::from_str("bit_perfect = true").unwrap();
        assert_eq!(w.bit_perfect, BitPerfectMode::Strict);
        let w: BitPerfectWrapper = toml::from_str(r#"bit_perfect = "relaxed""#).unwrap();
        assert_eq!(w.bit_perfect, BitPerfectMode::Relaxed);
    }
}
