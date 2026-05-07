//! Library page-size setting (controls how many items are fetched per API request).

use serde::{Deserialize, Serialize};

/// Library page size — controls how many items are fetched per API request.
///
/// Affects Songs, Albums, Artists views and progressive queue batching.
/// Serializes to lowercase strings for redb storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LibraryPageSize {
    /// Small pages for constrained environments (100 items)
    Small,
    /// Balanced fetch size (500 items)
    #[default]
    Default,
    /// Large pages for fast connections (1,000 items)
    Large,
    /// Very large pages (5,000 items) — may use significant memory
    Massive,
}

impl LibraryPageSize {
    /// Convert to record count
    pub fn to_usize(self) -> usize {
        match self {
            Self::Small => 100,
            Self::Default => 500,
            Self::Large => 1000,
            Self::Massive => 5000,
        }
    }

    /// Calculate dynamic fetch threshold based on page size
    pub fn fetch_threshold(self) -> usize {
        (self.to_usize() / 5).clamp(20, 500)
    }

    /// Convert from settings GUI label to enum variant
    pub fn from_label(label: &str) -> Self {
        match label {
            "Small (100)" => Self::Small,
            "Large (1,000)" => Self::Large,
            "Massive (5,000)" => Self::Massive,
            _ => Self::Default,
        }
    }

    /// Convert to settings GUI label
    pub fn as_label(self) -> &'static str {
        match self {
            Self::Small => "Small (100)",
            Self::Default => "Default (500)",
            Self::Large => "Large (1,000)",
            Self::Massive => "Massive (5,000)",
        }
    }
}

impl std::fmt::Display for LibraryPageSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Small => write!(f, "small"),
            Self::Default => write!(f, "default"),
            Self::Large => write!(f, "large"),
            Self::Massive => write!(f, "massive"),
        }
    }
}
