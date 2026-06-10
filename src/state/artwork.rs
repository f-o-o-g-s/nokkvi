//! Artwork caches (mini, large, collages) and loading state.
//!
//! Pre-redesign also held dominant-color extractions for the artwork-elevated
//! header — the redesign moved every active-state visual onto `accent_bright()`
//! fills, so the dominant-color path was removed (along with `color-thief`).

use std::{
    collections::{HashMap, HashSet},
    num::NonZeroUsize,
};

use iced::widget::image;

use super::snapshotted_lru::SnapshottedLru;

/// Maximum entries in the large artwork LRU cache.
/// Each 500px image handle is ~80KB, so 200 entries ≈ 16MB cap.
const LARGE_ARTWORK_CACHE_CAPACITY: NonZeroUsize =
    NonZeroUsize::new(200).expect("capacity must be > 0");
/// Capacity for the mini-artwork (`album_art`) LRU. Sized roughly 6× a typical
/// 80px slot list viewport so recently-visited slot regions stay warm but
/// memory stays bounded as the user scrolls a large library.
const MINI_ARTWORK_CACHE_CAPACITY: NonZeroUsize =
    NonZeroUsize::new(512).expect("capacity must be > 0");
/// Capacity for the per-target collage mini LRU (genre or playlist).
const COLLAGE_MINI_CACHE_CAPACITY: NonZeroUsize =
    NonZeroUsize::new(100).expect("capacity must be > 0");
/// Capacity for the per-target collage tile LRU (genre or playlist).
const COLLAGE_ARTWORK_CACHE_CAPACITY: NonZeroUsize =
    NonZeroUsize::new(100).expect("capacity must be > 0");

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
            mini: SnapshottedLru::new(COLLAGE_MINI_CACHE_CAPACITY),
            collage: SnapshottedLru::new(COLLAGE_ARTWORK_CACHE_CAPACITY),
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
    /// Sibling map recording the `updated_at` cache-buster that warmed each
    /// `album_art` slot. Kept in lockstep with `album_art` on every put: when
    /// the server-side cover changes, the album's `updated_at` changes, and a
    /// later prefetch tick sees `album_art_versions[id] != new_updated_at` and
    /// treats the slot as a genuine miss — re-fetching the changed cover on the
    /// album-coherent surfaces (Albums view, Artists/Genres expansion) that pass
    /// `album.updated_at`, without re-introducing SSE auto-refresh or threading a
    /// full `(album_id, updated_at)` key through the ~15 view read sites (N17).
    /// The passive surfaces (queue, song-mini, similar, playlist editor) carry
    /// only a per-song `updated_at`, which would oscillate this album_id-keyed
    /// map, so they feed a constant `None` (id-only dedup,
    /// `update::components::passive_artwork_version`) and re-warm the current
    /// cover on the next cold load.
    ///
    /// Reset to empty by `Default`, so logout (`ArtworkState::default()`) drops
    /// it for free. `album_art` evicts silently at capacity, so a version entry
    /// can outlive its handle — `should_refetch` guards against that by checking
    /// `album_art` membership, not just this map.
    pub album_art_versions: HashMap<String, Option<String>>,
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
            album_art: SnapshottedLru::new(MINI_ARTWORK_CACHE_CAPACITY),
            album_art_versions: HashMap::new(),
            large_artwork: SnapshottedLru::new(LARGE_ARTWORK_CACHE_CAPACITY),
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
            .field("album_art_versions", &self.album_art_versions)
            .field("large_artwork", &self.large_artwork)
            .field("genre", &self.genre)
            .field("playlist", &self.playlist)
            .field("loading_large_artwork", &self.loading_large_artwork)
            .finish()
    }
}
