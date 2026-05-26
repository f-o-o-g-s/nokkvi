//! Artwork caches (mini, large, collages) and loading state.
//!
//! Pre-redesign also held dominant-color extractions for the artwork-elevated
//! header — the redesign moved every active-state visual onto `accent_bright()`
//! fills, so the dominant-color path was removed (along with `color-thief`).

use std::{collections::HashSet, num::NonZeroUsize};

use iced::widget::image;

use super::snapshotted_lru::SnapshottedLru;

/// Maximum entries in the large artwork LRU cache.
/// Each 500px image handle is ~80KB, so 200 entries ≈ 16MB cap.
const LARGE_ARTWORK_CACHE_CAPACITY: usize = 200;
/// Capacity for the mini-artwork (`album_art`) LRU. Sized roughly 6× a typical
/// 80px slot list viewport so recently-visited slot regions stay warm but
/// memory stays bounded as the user scrolls a large library.
const MINI_ARTWORK_CACHE_CAPACITY: usize = 512;
/// Capacity for the per-target collage mini LRU (genre or playlist).
const COLLAGE_MINI_CACHE_CAPACITY: usize = 100;
/// Capacity for the per-target collage tile LRU (genre or playlist).
const COLLAGE_ARTWORK_CACHE_CAPACITY: usize = 100;

/// Per-target collage artwork cache (genre or playlist).
///
/// Each LRU is wrapped in `SnapshottedLru` so the view-layer-borrowable
/// `HashMap` snapshot stays in sync automatically — callers can't forget the
/// pairing.
#[derive(Debug, Clone)]
pub struct CollageArtworkCache {
    /// Mini artwork LRU + snapshot (item_id -> first album's cover Handle).
    pub mini: SnapshottedLru<String, image::Handle>,
    /// Collage artwork LRU + snapshot (item_id -> up-to-9 tile Handles).
    pub collage: SnapshottedLru<String, Vec<image::Handle>>,
    /// IDs with pending artwork loads (prevents duplicate in-flight requests).
    pub pending: HashSet<String>,
}

impl CollageArtworkCache {
    pub fn new() -> Self {
        Self {
            mini: SnapshottedLru::new(
                NonZeroUsize::new(COLLAGE_MINI_CACHE_CAPACITY).expect("capacity must be > 0"),
            ),
            collage: SnapshottedLru::new(
                NonZeroUsize::new(COLLAGE_ARTWORK_CACHE_CAPACITY).expect("capacity must be > 0"),
            ),
            pending: HashSet::new(),
        }
    }
}

/// Artwork caches and loading state.
///
/// All three primary caches use `SnapshottedLru` so view-layer borrowing
/// against the read-only `HashMap` mirror stays consistent with the LRU
/// after every mutation — no manual `refresh_*_snapshot()` discipline
/// required.
#[derive(Clone)]
pub struct ArtworkState {
    /// Mini artwork cache (album_id -> Handle), bounded LRU.
    /// Without a persistent disk cache, this is the only thing keeping recently-
    /// rendered thumbnails warm. Capacity must stay above the typical viewport
    /// + scrollback or slot lists thrash.
    pub album_art: SnapshottedLru<String, image::Handle>,
    /// Large artwork cache for detail views (LRU-bounded).
    pub large_artwork: SnapshottedLru<String, image::Handle>,
    /// Genre artwork cache.
    pub genre: CollageArtworkCache,
    /// Playlist artwork cache.
    pub playlist: CollageArtworkCache,
    /// Currently loading large artwork album ID.
    pub loading_large_artwork: Option<String>,
}

impl Default for ArtworkState {
    fn default() -> Self {
        Self {
            album_art: SnapshottedLru::new(
                NonZeroUsize::new(MINI_ARTWORK_CACHE_CAPACITY).expect("capacity must be > 0"),
            ),
            large_artwork: SnapshottedLru::new(
                NonZeroUsize::new(LARGE_ARTWORK_CACHE_CAPACITY).expect("capacity must be > 0"),
            ),
            genre: CollageArtworkCache::new(),
            playlist: CollageArtworkCache::new(),
            loading_large_artwork: None,
        }
    }
}

impl std::fmt::Debug for ArtworkState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArtworkState")
            .field("album_art", &self.album_art)
            .field("large_artwork", &self.large_artwork)
            .field("genre", &self.genre)
            .field("playlist", &self.playlist)
            .field("loading_large_artwork", &self.loading_large_artwork)
            .finish()
    }
}
