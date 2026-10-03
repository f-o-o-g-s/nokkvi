//! Loaded library data buffers + per-view counts.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// Stale-drop generation for the Songs view's progressive queue load. Clones
/// share one counter: the chain's page task holds a clone, so a page fetched
/// before a newer queue replacement is dropped before it is appended, not
/// only the pages after it.
#[derive(Debug, Clone, Default)]
pub struct ProgressiveQueueGeneration(Arc<AtomicU64>);

impl ProgressiveQueueGeneration {
    pub fn current(&self) -> u64 {
        self.0.load(Ordering::Acquire)
    }

    /// Invalidate every running chain; returns the new generation.
    pub fn bump(&self) -> u64 {
        self.0.fetch_add(1, Ordering::AcqRel).wrapping_add(1)
    }

    pub fn is_current(&self, generation: u64) -> bool {
        self.current() == generation
    }
}

/// All loaded library data vectors + counts
///
/// Groups the 6 data vectors and their associated counts that were
/// previously individual fields on Nokkvi.
///
/// Albums, artists, songs, genres, and playlists use `PagedBuffer<T>` for
/// server-side pagination. Queue stays as `Vec<T>` since it's managed
/// locally by the queue service, not paginated from the API.
#[derive(Debug, Clone, Default)]
pub struct LibraryData {
    pub albums: nokkvi_data::types::paged_buffer::PagedBuffer<
        nokkvi_data::backend::albums::AlbumUIViewData,
    >,
    pub artists: nokkvi_data::types::paged_buffer::PagedBuffer<
        nokkvi_data::backend::artists::ArtistUIViewData,
    >,
    pub songs:
        nokkvi_data::types::paged_buffer::PagedBuffer<nokkvi_data::backend::songs::SongUIViewData>,
    pub genres: nokkvi_data::types::paged_buffer::PagedBuffer<
        nokkvi_data::backend::genres::GenreUIViewData,
    >,
    pub playlists: nokkvi_data::types::paged_buffer::PagedBuffer<
        nokkvi_data::backend::playlists::PlaylistUIViewData,
    >,
    pub queue_songs: Vec<nokkvi_data::backend::queue::QueueSongUIViewData>,
    pub radio_stations: Vec<nokkvi_data::types::radio_station::RadioStation>,
    /// Target count during progressive queue loading (e.g., 12036 while loading).
    /// When `Some`, the queue header shows "X of Y songs" as pages are appended.
    pub queue_loading_target: Option<usize>,
    /// Generation counter for progressive queue loading. Bumped each time
    /// play-from-songs starts a new chain; stale chains self-cancel by comparing
    /// their generation against this value.
    pub progressive_queue_generation: ProgressiveQueueGeneration,
    pub counts: LibraryCounts,
}

/// Total counts for library items (used in headers)
#[derive(Debug, Clone, Default)]
pub struct LibraryCounts {
    pub albums: usize,
    pub artists: usize,
    pub genres: usize,
    pub playlists: usize,
    pub songs: usize,
}
