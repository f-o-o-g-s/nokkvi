//! Library page-size setting (controls how many items are fetched per API request).

use serde::{Deserialize, Serialize};

use crate::define_labeled_enum;

define_labeled_enum! {
    /// Library page size — controls how many items are fetched per API request.
    ///
    /// Affects Songs, Albums, Artists views and progressive queue batching.
    /// Serializes to lowercase strings for redb storage.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum LibraryPageSize {
        /// Small pages for constrained environments (100 items)
        Small { label: "Small (100)", wire: "small" },
        /// Balanced fetch size (500 items)
        #[default]
        Default { label: "Default (500)", wire: "default" },
        /// Large pages for fast connections (1,000 items)
        Large { label: "Large (1,000)", wire: "large" },
        /// Very large pages (5,000 items) — may use significant memory
        Massive { label: "Massive (5,000)", wire: "massive" },
    }
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
}
