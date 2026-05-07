//! Visualization mode (Off / Bars / Lines).

use serde::{Deserialize, Serialize};

/// Visualization mode for the audio visualizer.
///
/// Cycles: Off → Bars → Lines → Off via `next()`.
/// Serializes to lowercase strings for redb storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VisualizationMode {
    Off,
    #[default]
    Bars,
    Lines,
}

impl VisualizationMode {
    /// Cycle to the next visualization mode.
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Bars,
            Self::Bars => Self::Lines,
            Self::Lines => Self::Off,
        }
    }
}

impl std::fmt::Display for VisualizationMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Off => write!(f, "off"),
            Self::Bars => write!(f, "bars"),
            Self::Lines => write!(f, "lines"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visualization_mode_cycles() {
        assert_eq!(VisualizationMode::Off.next(), VisualizationMode::Bars);
        assert_eq!(VisualizationMode::Bars.next(), VisualizationMode::Lines);
        assert_eq!(VisualizationMode::Lines.next(), VisualizationMode::Off);
    }

    #[test]
    fn visualization_mode_default_is_bars() {
        assert_eq!(VisualizationMode::default(), VisualizationMode::Bars);
    }

    #[test]
    fn visualization_mode_serde_roundtrip() {
        let modes = [
            VisualizationMode::Off,
            VisualizationMode::Bars,
            VisualizationMode::Lines,
        ];
        for mode in modes {
            let json = serde_json::to_string(&mode).unwrap();
            let deserialized: VisualizationMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, deserialized);
        }
    }

    #[test]
    fn visualization_mode_deserializes_from_lowercase_strings() {
        assert_eq!(
            serde_json::from_str::<VisualizationMode>("\"off\"").unwrap(),
            VisualizationMode::Off
        );
        assert_eq!(
            serde_json::from_str::<VisualizationMode>("\"bars\"").unwrap(),
            VisualizationMode::Bars
        );
        assert_eq!(
            serde_json::from_str::<VisualizationMode>("\"lines\"").unwrap(),
            VisualizationMode::Lines
        );
    }
}
