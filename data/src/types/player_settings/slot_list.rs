//! Slot-list interaction settings — Enter-key behavior and row density.

use serde::{Deserialize, Serialize};

/// What happens when pressing Enter on a song in the Songs view.
///
/// Serializes to snake_case strings for redb storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnterBehavior {
    /// Replace queue with all songs in the current view, play from selected index
    #[default]
    PlayAll,
    /// Replace queue with just the selected song
    PlaySingle,
    /// Append the selected song to the existing queue and start playing it
    AppendAndPlay,
}

impl EnterBehavior {
    /// Convert from settings GUI label to enum variant
    pub fn from_label(label: &str) -> Self {
        match label {
            "Play Single" => Self::PlaySingle,
            "Append & Play" => Self::AppendAndPlay,
            _ => Self::PlayAll,
        }
    }

    /// Convert to settings GUI label
    pub fn as_label(self) -> &'static str {
        match self {
            Self::PlayAll => "Play All",
            Self::PlaySingle => "Play Single",
            Self::AppendAndPlay => "Append & Play",
        }
    }
}

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
    Compact,
    /// Balanced (70px target)
    #[default]
    Default,
    /// Fewer, taller rows (90px target)
    Comfortable,
    /// Maximum row height (110px target)
    Spacious,
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

    /// Convert from settings GUI label to enum variant.
    pub fn from_label(label: &str) -> Self {
        match label {
            "Compact" => Self::Compact,
            "Comfortable" => Self::Comfortable,
            "Spacious" => Self::Spacious,
            _ => Self::Default,
        }
    }

    /// Convert to settings GUI label.
    pub fn as_label(self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Default => "Default",
            Self::Comfortable => "Comfortable",
            Self::Spacious => "Spacious",
        }
    }
}

impl std::fmt::Display for SlotRowHeight {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Compact => write!(f, "compact"),
            Self::Default => write!(f, "default"),
            Self::Comfortable => write!(f, "comfortable"),
            Self::Spacious => write!(f, "spacious"),
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
}
