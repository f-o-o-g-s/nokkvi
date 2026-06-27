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
/// memory stays bounded as the user scrolls a large library. Doubled when the
/// playlist 2×2 quads landed: a playlists viewport claims up to 4 ids per row
/// instead of 1, and the quads share this cache with every other 80px surface
/// — at ~3-6KB per 80px handle the cap stays a few MB.
const MINI_ARTWORK_CACHE_CAPACITY: NonZeroUsize =
    NonZeroUsize::new(1024).expect("capacity must be > 0");
/// Capacity for the per-target collage mini LRU (genre or playlist).
const COLLAGE_MINI_CACHE_CAPACITY: NonZeroUsize =
    NonZeroUsize::new(100).expect("capacity must be > 0");
/// Capacity for the per-target collage tile LRU (genre or playlist).
const COLLAGE_ARTWORK_CACHE_CAPACITY: NonZeroUsize =
    NonZeroUsize::new(100).expect("capacity must be > 0");
/// Capacity for the mini radio-station artwork LRU (`station_id -> Handle`).
/// Radio station lists are small (a few dozen), so a modest cap holds every
/// station's logo / remembered stream-art thumbnail with room to spare.
const RADIO_ART_CACHE_CAPACITY: NonZeroUsize =
    NonZeroUsize::new(256).expect("capacity must be > 0");
/// Capacity for the large radio-station artwork LRU (`station_id -> Handle`).
/// The panel shows one station's large art at a time, but the cache accumulates
/// every logo station ever centered (each scroll/nav step dispatches a
/// `LoadRadioLarge`), and the playing station's entry is `put` once and never
/// recency-bumped (the view reads the snapshot without touching the LRU). Sized
/// to comfortably exceed any realistic station list so scrolling can't evict the
/// now-playing station's large art out from under the locked panel / MiniPlayer.
const RADIO_LARGE_ART_CACHE_CAPACITY: NonZeroUsize =
    NonZeroUsize::new(512).expect("capacity must be > 0");

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
    /// Album ids with an in-flight 80px quad-tile fetch. The quad prefetch is
    /// re-dispatched from every scroll step and collage event; without an
    /// in-flight gate a cold viewport would duplicate every still-loading
    /// request on each step. Inserted when a quad fetch task is built, removed
    /// by `handle_artwork_loaded` on success AND failure (the `Loaded` message
    /// always arrives). The single-id prefetch surfaces keep their existing
    /// gate-free behavior — only the ×4 quad paths consult this set.
    pub album_art_pending: HashSet<String>,
    /// Negative cache: album/artist ids whose 80px cover fetch returned NO image
    /// — a stale/deleted id that resolves to Navidrome's code-70 "Artwork not
    /// found". An album that merely lacks a cover gets a placeholder *image* and
    /// caches normally via `album_art`, so it never lands here; only genuinely
    /// unresolvable ids do. Maps `id -> the updated_at that failed`, mirroring
    /// `album_art_versions`: the membership-based prefetch gates (`should_refetch`,
    /// the artist gate, the quad gate) consult it to stop re-queuing a known-dead
    /// id on every scroll/resize/view-switch. A CHANGED `updated_at` (server cover
    /// added) bypasses the entry and re-attempts; it is cleared on any later
    /// success in the loaded-handlers and on a user "Refresh Artwork", and dropped
    /// wholesale by `ArtworkState::default()` on logout/session reset (so server-A
    /// failures never suppress server-B art). Unbounded like `album_art_versions`,
    /// but populated only by the narrow stale-id case and reset on logout.
    pub failed_art: HashMap<String, Option<String>>,
    /// Large artwork cache for detail views (LRU-bounded).
    pub large_artwork: SnapshottedLru<String, image::Handle>,
    /// Mini radio-station artwork (`station_id -> Handle`) for the Radios slot
    /// list. Holds either an admin-uploaded station logo (fetched via
    /// `getCoverArt?id=ra-…` when [`RadioStation::logo_cover_art`] is present)
    /// or, for logo-less stations, the remembered last-played stream (ICY) art
    /// hydrated from the on-disk [`RadioArtStore`]. Reset on logout via
    /// `Default`; the disk store re-hydrates it next launch.
    ///
    /// [`RadioStation::logo_cover_art`]: nokkvi_data::types::radio_station::RadioStation::logo_cover_art
    /// [`RadioArtStore`]: nokkvi_data::services::radio_art_store::RadioArtStore
    pub radio_art: SnapshottedLru<String, image::Handle>,
    /// Large radio-station artwork (`station_id -> Handle`) for the artwork
    /// panel: a resolution-sized station logo (logo stations) or the
    /// remembered/now-playing stream (ICY) image (logo-less stations). A logo
    /// station's live track art is NOT stored here — its logo is its stable
    /// in-app identity. Falls back to [`Self::radio_art`] then the radio-tower
    /// glyph at the view layer.
    pub radio_large_art: SnapshottedLru<String, image::Handle>,
    /// Per-station dedup of the last ICY `StreamUrl` we fetched now-playing art
    /// from (`station_id -> url`). The playback tick re-parses ICY metadata
    /// every ~100ms, so this gates the external fetch to fire only when the
    /// URL actually changes (a new track). Seeded from the on-disk
    /// `RadioArtStore` on launch so a restart doesn't re-fetch unchanged art.
    pub radio_icy_captured: HashMap<String, String>,
    /// Whether remembered radio art has been hydrated from disk this session.
    /// A one-shot gate — probing `radio_icy_captured` emptiness conflates
    /// capture-state with hydration-state and re-reads disk on an empty store.
    /// Reset with the rest of `ArtworkState` on logout.
    pub radio_art_hydrated: bool,
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
            album_art_pending: HashSet::new(),
            failed_art: HashMap::new(),
            large_artwork: SnapshottedLru::new(LARGE_ARTWORK_CACHE_CAPACITY),
            radio_art: SnapshottedLru::new(RADIO_ART_CACHE_CAPACITY),
            radio_large_art: SnapshottedLru::new(RADIO_LARGE_ART_CACHE_CAPACITY),
            radio_icy_captured: HashMap::new(),
            radio_art_hydrated: false,
            genre: CollageArtworkCache::new(),
            playlist: CollageArtworkCache::new(),
            loading_large_artwork: None,
        }
    }
}

impl ArtworkState {
    /// True when `id`'s cover fetch already failed at exactly `version`, so the
    /// membership-based prefetch gates should skip re-queuing it. A different
    /// `version` (server cover changed) is deliberately NOT suppressed. Used by
    /// the artist gate (which has no version) with `version == &None`.
    pub fn art_failed_at(&self, id: &str, version: &Option<String>) -> bool {
        self.failed_art.get(id) == Some(version)
    }
}

impl std::fmt::Debug for ArtworkState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArtworkState")
            .field("album_art", &self.album_art)
            .field("album_art_versions", &self.album_art_versions)
            .field("large_artwork", &self.large_artwork)
            .field("radio_art", &self.radio_art)
            .field("radio_large_art", &self.radio_large_art)
            .field("genre", &self.genre)
            .field("playlist", &self.playlist)
            .field("loading_large_artwork", &self.loading_large_artwork)
            .finish()
    }
}
