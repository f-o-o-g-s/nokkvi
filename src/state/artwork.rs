//! Artwork caches (mini, large, dominant colors, collages) and loading state.

use std::{
    collections::{HashMap, HashSet},
    num::NonZeroUsize,
};

use iced::widget::image;
use lru::LruCache;

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

/// Per-target collage artwork cache (genre or playlist)
#[derive(Debug, Clone)]
pub struct CollageArtworkCache {
    /// Mini artwork LRU cache (item_id -> Handle, first album's cover)
    pub mini: LruCache<String, image::Handle>,
    /// Read-only snapshot of `mini` for view() borrowing (refreshed after LRU mutations).
    pub mini_snapshot: HashMap<String, image::Handle>,
    /// Collage artwork LRU cache (item_id -> Vec<Handle> for 3x3 collage, up to 9)
    pub collage: LruCache<String, Vec<image::Handle>>,
    /// Read-only snapshot of `collage` for view() borrowing (refreshed after LRU mutations).
    pub collage_snapshot: HashMap<String, Vec<image::Handle>>,
    /// IDs with pending artwork loads (prevents duplicate in-flight requests)
    pub pending: HashSet<String>,
}

impl CollageArtworkCache {
    pub fn new() -> Self {
        Self {
            mini: LruCache::new(
                NonZeroUsize::new(COLLAGE_MINI_CACHE_CAPACITY).expect("capacity must be > 0"),
            ),
            mini_snapshot: HashMap::new(),
            collage: LruCache::new(
                NonZeroUsize::new(COLLAGE_ARTWORK_CACHE_CAPACITY).expect("capacity must be > 0"),
            ),
            collage_snapshot: HashMap::new(),
            pending: HashSet::new(),
        }
    }

    /// Refresh both read-only snapshots from the LRU caches.
    /// Call after any mutation to `mini` or `collage` (put/get).
    pub fn refresh_snapshot(&mut self) {
        self.mini_snapshot = self
            .mini
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        self.collage_snapshot = self
            .collage
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
    }
}

/// Artwork caches and loading state
#[derive(Clone)]
pub struct ArtworkState {
    /// Mini artwork cache (album_id -> Handle), bounded LRU.
    /// Without a persistent disk cache, this is the only thing keeping recently-
    /// rendered thumbnails warm. Capacity must stay above the typical viewport
    /// + scrollback or slot lists thrash.
    pub album_art: LruCache<String, image::Handle>,
    /// Read-only snapshot of `album_art` for view() borrowing (refreshed after LRU mutations).
    pub album_art_snapshot: HashMap<String, image::Handle>,
    /// Large artwork cache for detail views (LRU-bounded)
    pub large_artwork: LruCache<String, image::Handle>,
    /// Read-only snapshot of large_artwork for view() borrowing (refreshed after LRU mutations)
    pub large_artwork_snapshot: HashMap<String, image::Handle>,
    /// Cache for album dominant colors (extracted from large artwork bytes)
    pub album_dominant_colors: LruCache<String, iced::Color>,
    /// Read-only snapshot of dominant colors for view()
    pub album_dominant_colors_snapshot: HashMap<String, iced::Color>,
    /// Genre artwork cache
    pub genre: CollageArtworkCache,
    /// Playlist artwork cache
    pub playlist: CollageArtworkCache,
    /// Currently loading large artwork album ID
    pub loading_large_artwork: Option<String>,
}

impl ArtworkState {
    /// Refresh the read-only snapshot from the LRU cache.
    /// Call after any mutation to `large_artwork` (put/get).
    pub fn refresh_large_artwork_snapshot(&mut self) {
        self.large_artwork_snapshot = self
            .large_artwork
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
    }

    /// Refresh the read-only snapshot of mini album art from the LRU cache.
    /// Call after any mutation to `album_art` (put/get/pop).
    pub fn refresh_album_art_snapshot(&mut self) {
        self.album_art_snapshot = self
            .album_art
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
    }

    /// Refresh the read-only snapshot of dominant colors from the LRU cache.
    pub fn refresh_dominant_colors_snapshot(&mut self) {
        self.album_dominant_colors_snapshot = self
            .album_dominant_colors
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect();
    }
}

impl Default for ArtworkState {
    fn default() -> Self {
        Self {
            album_art: LruCache::new(
                NonZeroUsize::new(MINI_ARTWORK_CACHE_CAPACITY).expect("capacity must be > 0"),
            ),
            album_art_snapshot: HashMap::new(),
            large_artwork: LruCache::new(
                NonZeroUsize::new(LARGE_ARTWORK_CACHE_CAPACITY).expect("capacity must be > 0"),
            ),
            large_artwork_snapshot: HashMap::new(),
            album_dominant_colors: LruCache::new(
                NonZeroUsize::new(LARGE_ARTWORK_CACHE_CAPACITY).expect("capacity must be > 0"),
            ),
            album_dominant_colors_snapshot: HashMap::new(),
            genre: CollageArtworkCache::new(),
            playlist: CollageArtworkCache::new(),
            loading_large_artwork: None,
        }
    }
}

impl std::fmt::Debug for ArtworkState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArtworkState")
            .field("album_art", &self.album_art.len())
            .field("album_art_snapshot", &self.album_art_snapshot.len())
            .field("large_artwork", &self.large_artwork.len())
            .field("large_artwork_snapshot", &self.large_artwork_snapshot.len())
            .field("album_dominant_colors", &self.album_dominant_colors.len())
            .field(
                "album_dominant_colors_snapshot",
                &self.album_dominant_colors_snapshot.len(),
            )
            .field("genre", &self.genre)
            .field("playlist", &self.playlist)
            .field("loading_large_artwork", &self.loading_large_artwork)
            .finish()
    }
}
