//! Playback normalization settings — AGC level and mode (off / AGC / ReplayGain).

use serde::{Deserialize, Serialize};

use crate::define_labeled_enum;

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
    pub fn is_off(self) -> bool {
        matches!(self, Self::Off)
    }

    pub fn is_agc(self) -> bool {
        matches!(self, Self::Agc)
    }

    pub fn is_replay_gain(self) -> bool {
        matches!(self, Self::ReplayGainTrack | Self::ReplayGainAlbum)
    }

    pub fn prefers_album(self) -> bool {
        matches!(self, Self::ReplayGainAlbum)
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
        assert!(VolumeNormalizationMode::Off.is_off());
        assert!(VolumeNormalizationMode::Agc.is_agc());
        assert!(VolumeNormalizationMode::ReplayGainTrack.is_replay_gain());
        assert!(VolumeNormalizationMode::ReplayGainAlbum.is_replay_gain());
        assert!(VolumeNormalizationMode::ReplayGainAlbum.prefers_album());
        assert!(!VolumeNormalizationMode::ReplayGainTrack.prefers_album());
    }
}
