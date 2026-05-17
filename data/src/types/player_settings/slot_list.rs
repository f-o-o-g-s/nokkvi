//! Slot-list interaction settings — Enter-key behavior and row density.

use serde::{Deserialize, Serialize};

use crate::define_labeled_enum;

define_labeled_enum! {
    /// What happens when pressing Enter on a song in the Songs view.
    ///
    /// Serializes to snake_case strings for redb storage.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum EnterBehavior {
        /// Replace queue with all songs in the current view, play from selected index
        #[default]
        PlayAll { label: "Play All", wire: "play_all" },
        /// Replace queue with just the selected song
        PlaySingle { label: "Play Single", wire: "play_single" },
        /// Append the selected song to the existing queue and start playing it
        AppendAndPlay { label: "Append & Play", wire: "append_and_play" },
    }
}

define_labeled_enum! {
    /// Slot list row density — controls the target row height for all slot lists.
    ///
    /// Each variant is spaced far enough apart (~20px) to guarantee a different
    /// slot count at any reasonable window height, eliminating the dead-zone
    /// problem of the old continuous slider.
    ///
    /// Serializes to lowercase strings for redb storage.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum SlotRowHeight {
        /// Maximum density — smallest comfortable rows (50px target)
        Compact { label: "Compact", wire: "compact" },
        /// Balanced (70px target)
        #[default]
        Default { label: "Default", wire: "default" },
        /// Fewer, taller rows (90px target)
        Comfortable { label: "Comfortable", wire: "comfortable" },
        /// Maximum row height (110px target)
        Spacious { label: "Spacious", wire: "spacious" },
    }
}

impl SlotRowHeight {
    /// Target pixel height for this density level.
    pub fn to_pixels(self) -> u8 {
        match self {
            Self::Compact => 50,
            Self::Default => 70,
            Self::Comfortable => 90,
            Self::Spacious => 110,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_behavior_default_is_play_all() {
        assert_eq!(EnterBehavior::default(), EnterBehavior::PlayAll);
    }

    #[test]
    fn enter_behavior_serde_roundtrip() {
        let behaviors = [
            EnterBehavior::PlayAll,
            EnterBehavior::PlaySingle,
            EnterBehavior::AppendAndPlay,
        ];
        for behavior in behaviors {
            let json = serde_json::to_string(&behavior).unwrap();
            let deserialized: EnterBehavior = serde_json::from_str(&json).unwrap();
            assert_eq!(behavior, deserialized);
        }
    }

    #[test]
    fn enter_behavior_label_roundtrip() {
        for behavior in [
            EnterBehavior::PlayAll,
            EnterBehavior::PlaySingle,
            EnterBehavior::AppendAndPlay,
        ] {
            assert_eq!(EnterBehavior::from_label(behavior.as_label()), behavior);
        }
    }

    // Pins the new Display impl that the macro added to EnterBehavior — the
    // hand-written version was previously missing this trait, leaving the
    // enum asymmetric with the other settings enums. The wire strings mirror
    // the outer `#[serde(rename_all = "snake_case")]`.
    #[test]
    fn enter_behavior_display_play_all() {
        assert_eq!(EnterBehavior::PlayAll.to_string(), "play_all");
    }

    #[test]
    fn enter_behavior_display_play_single() {
        assert_eq!(EnterBehavior::PlaySingle.to_string(), "play_single");
    }

    #[test]
    fn enter_behavior_display_append_and_play() {
        assert_eq!(EnterBehavior::AppendAndPlay.to_string(), "append_and_play");
    }
}
