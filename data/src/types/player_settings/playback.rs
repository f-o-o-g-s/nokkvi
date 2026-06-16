//! Playback normalization settings — AGC level and mode (off / AGC / ReplayGain).

use serde::{Deserialize, Serialize};

use crate::define_labeled_enum;

/// Crossfade duration bounds, in seconds. Single source of truth shared by the
/// enforcement clamp in `SettingsManager::set_crossfade_duration` and the
/// slider's declared `min`/`max` in the playback settings table, so the slider
/// can never offer a value the setter would silently truncate.
pub const CROSSFADE_DURATION_MIN_SECS: u32 = 1;
pub const CROSSFADE_DURATION_MAX_SECS: u32 = 12;

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
