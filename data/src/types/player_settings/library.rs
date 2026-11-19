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
}

/// Dynamic page-fetch trigger threshold for a paginated buffer.
///
/// `PagedBuffer<T>::needs_fetch` fires a new request when the viewport is
/// within this many items of the loaded edge. Sized as a fifth of the page
/// (so a 500-item page fetches at 100 items from the end), clamped to
/// `[20, 500]` so very small page sizes still fire reasonably early and
/// very large ones don't pre-fetch megabytes off-screen.
///
/// Free fn so `PagedBuffer<T>` (a generic data type) doesn't have to depend
/// on the `LibraryPageSize` enum: it passes the page size as a number.
pub fn pagination_fetch_threshold(page_size: usize) -> usize {
    (page_size / 5).clamp(20, 500)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins the exact `(page_size / 5).clamp(20, 500)` formula that
    /// `PagedBuffer::<T>::needs_fetch` fetches by.
    #[test]
    fn fetch_threshold_formula() {
        // Below the 20-item floor: page_size/5 = 20 → 20 (boundary)
        assert_eq!(pagination_fetch_threshold(100), 20);
        // Below the floor: page_size/5 = 10 → clamps up to 20
        assert_eq!(pagination_fetch_threshold(50), 20);
        // Mid-range: 500/5 = 100
        assert_eq!(pagination_fetch_threshold(500), 100);
        // Above the floor, below the ceiling: 1000/5 = 200
        assert_eq!(pagination_fetch_threshold(1000), 200);
        // Above the 500-item ceiling: 5000/5 = 1000 → clamps down to 500
        assert_eq!(pagination_fetch_threshold(5000), 500);

        // Each LibraryPageSize variant's page size, through to_usize().
        let threshold = |size: LibraryPageSize| pagination_fetch_threshold(size.to_usize());
        assert_eq!(threshold(LibraryPageSize::Small), 20);
        assert_eq!(threshold(LibraryPageSize::Default), 100);
        assert_eq!(threshold(LibraryPageSize::Large), 200);
        assert_eq!(threshold(LibraryPageSize::Massive), 500);
    }
}
