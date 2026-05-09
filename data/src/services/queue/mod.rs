//! Queue management service
//!
//! `QueueManager` owns the play queue (song IDs + order array), a `SongPool`
//! for O(1) song lookups, and playback history for previous-song navigation.
//!
//! Split into sub-modules:
//! - `order` — Order array manipulation (rebuild, shuffle, extend, remove)
//! - `navigation` — Song navigation (peek, transition, next, previous, history)

mod navigation;
mod order;

use anyhow::Result;
pub use navigation::{NextSongResult, PeekedQueue, PreviousSongResult, TransitionResult};
use rand::seq::SliceRandom;
use tracing::{debug, warn};

use crate::{
    services::state_storage::StateStorage,
    types::{
        queue::{Queue, RepeatMode},
        queue_sort_mode::QueueSortMode,
        song::Song,
        song_pool::SongPool,
    },
};

pub struct QueueManager {
    pub(crate) queue: Queue,
    pub(crate) pool: SongPool,
    pub(crate) storage: StateStorage,
    pub(crate) playback_history: Vec<Song>,
    pub(crate) max_history_size: usize,
}

impl std::fmt::Debug for QueueManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueueManager")
            .field("queue_len", &self.queue.song_ids.len())
            .field("pool_len", &self.pool.len())
            .field("current_index", &self.queue.current_index)
            .field("current_order", &self.queue.current_order)
            .finish()
    }
}

// ── Persistence key constants ──
const KEY_QUEUE_ORDER: &str = "queue_order";
const KEY_QUEUE_SONGS: &str = "queue_songs";

impl QueueManager {
    pub fn new(storage: StateStorage) -> Result<Self> {
        let (queue, pool) = if let Some(queue) = storage.load_binary::<Queue>(KEY_QUEUE_ORDER)? {
            // Pool load is best-effort: a corrupted/incompatible pool must never
            // block login. Start with an empty pool if anything goes wrong.
            let pool: SongPool = match storage.load_binary(KEY_QUEUE_SONGS) {
                Ok(Some(p)) => p,
                Ok(None) => SongPool::default(),
                Err(e) => {
                    warn!(" [QUEUE] Failed to load song pool, starting empty: {e}");
                    SongPool::default()
                }
            };
            (queue, pool)
        } else {
            (Queue::default(), SongPool::default())
        };

        let mgr = Self {
            queue,
            pool,
            storage,
            playback_history: Vec::new(),
            max_history_size: 100,
        };

        Ok(mgr)
    }

    // ── Song Pool Accessors ──

    /// Look up a song by ID from the pool (O(1)).
    pub fn get_song(&self, id: &str) -> Option<&Song> {
        self.pool.get(id)
    }

    /// Look up a song by ID from the pool (mutable, O(1)).
    pub fn get_song_mut(&mut self, id: &str) -> Option<&mut Song> {
        self.pool.get_mut(id)
    }

    /// Reconstruct an ordered `Vec<Song>` from the current queue ordering.
    /// Used by `QueueService::refresh_from_queue()` to build UI data.
    pub fn songs_in_order(&self) -> Vec<&Song> {
        self.queue
            .song_ids
            .iter()
            .filter_map(|id| self.pool.get(id))
            .collect()
    }

    /// O(n) scan to find the index of a song ID in the queue.
    /// Centralized here so all callers use the same lookup.
    pub fn index_of(&self, song_id: &str) -> Option<usize> {
        self.queue.song_ids.iter().position(|id| id == song_id)
    }

    /// Assign `original_position` to a batch of songs, continuing from the
    /// current maximum in the pool. Used by every "append" path so numbering
    /// is consistent regardless of insertion method.
    fn assign_original_positions(&self, songs: &mut [Song]) {
        let next_pos = self
            .queue
            .song_ids
            .iter()
            .filter_map(|id| self.pool.get(id))
            .filter_map(|s| s.original_position)
            .max()
            .map_or(self.queue.song_ids.len() as u32, |m| m + 1);
        for (i, song) in songs.iter_mut().enumerate() {
            song.original_position = Some(next_pos + i as u32);
        }
    }

    // ══════════════════════════════════════════════════════════════════════
    //  Queue Mutations
    // ══════════════════════════════════════════════════════════════════════

    pub fn add_songs(&mut self, mut songs: Vec<Song>) -> Result<()> {
        self.assign_original_positions(&mut songs);
        let start_idx = self.queue.song_ids.len();
        let count = songs.len();

        // Add IDs to ordering, songs to pool
        for song in &songs {
            self.queue.song_ids.push(song.id.clone());
        }
        self.pool.insert_many(songs);

        // Extend order array with new indices
        self.extend_order(start_idx..start_idx + count);
        self.clear_queued();
        self.save_all()?;
        Ok(())
    }

    pub fn set_queue(&mut self, mut songs: Vec<Song>, current_index: Option<usize>) -> Result<()> {
        // Assign original_position to capture insertion order
        for (i, song) in songs.iter_mut().enumerate() {
            song.original_position = Some(i as u32);
        }
        self.queue.song_ids = songs.iter().map(|s| s.id.clone()).collect();
        self.queue.current_index = current_index;
        // Clear and rebuild pool
        self.pool.clear();
        self.pool.insert_many(songs);
        // Clear history on context switch (new album/playlist) — Spotify behavior
        self.playback_history.clear();
        // Rebuild order array and sync
        self.rebuild_order_and_sync();
        // If shuffle is on, shuffle the new order
        if self.queue.shuffle {
            self.shuffle_order();
        }
        self.clear_queued();
        self.save_all()?;
        Ok(())
    }

    pub fn remove_song(&mut self, index: usize) -> Result<()> {
        if index < self.queue.song_ids.len() {
            let removed_id = self.queue.song_ids.remove(index);
            self.pool.remove(&removed_id);

            // Remove from order array and adjust indices
            self.remove_from_order(index);

            // Adjust current_index to keep tracking the same playing song
            if let Some(cur) = self.queue.current_index {
                if self.queue.song_ids.is_empty() {
                    // Queue is now empty
                    self.queue.current_index = None;
                } else if index < cur {
                    // Removed before current — shift back
                    self.queue.current_index = Some(cur - 1);
                } else if index == cur {
                    // Removed the current song — clamp to valid range
                    self.queue.current_index = Some(cur.min(self.queue.song_ids.len() - 1));
                }
                // index > cur: no adjustment needed
            }

            self.clear_queued();
            self.save_all()?;
        }
        Ok(())
    }

    /// Remove a song from the pool by ID (used by consume paths that manage
    /// `song_ids` removal and `current_index` adjustment themselves).
    pub fn remove_from_pool(&mut self, id: &str) {
        self.pool.remove(id);
    }

    /// Remove a single song from the queue by its ID.
    ///
    /// Resolves the index freshly via [`Self::index_of`] so callers don't have
    /// to track positions across optimistic UI mutations, client-side sorts,
    /// or concurrent queue changes. No-op if the ID isn't present.
    pub fn remove_song_by_id(&mut self, id: &str) -> Result<()> {
        if let Some(idx) = self.index_of(id) {
            self.remove_song(idx)?;
        }
        Ok(())
    }

    /// Remove multiple songs from the queue by ID.
    ///
    /// Each ID is resolved freshly between removals so cascading shifts can't
    /// desync the targets. Unknown IDs are skipped silently. Order of `ids`
    /// is irrelevant — each lookup is against the current queue state.
    pub fn remove_songs_by_ids(&mut self, ids: &[String]) -> Result<()> {
        for id in ids {
            if let Some(idx) = self.index_of(id) {
                self.remove_song(idx)?;
            }
        }
        Ok(())
    }

    pub fn toggle_shuffle(&mut self) -> Result<()> {
        self.queue.shuffle = !self.queue.shuffle;
        debug!(
            " [SHUFFLE] Shuffle mode: {}",
            if self.queue.shuffle { "ON" } else { "OFF" }
        );
        if self.queue.shuffle {
            self.shuffle_order();
        } else {
            self.unshuffle_order();
        }
        self.clear_queued();
        self.save_order()?;
        Ok(())
    }

    /// Shuffle the queue order randomly.
    /// Preserves the currently playing song at its current index.
    pub fn shuffle_queue(&mut self) -> Result<()> {
        if self.queue.song_ids.is_empty() {
            return Ok(());
        }

        let current_song_id = self
            .queue
            .current_index
            .and_then(|idx| self.queue.song_ids.get(idx))
            .cloned();

        // Shuffle the IDs using Fisher-Yates algorithm
        let mut rng = rand::rng();
        self.queue.song_ids.shuffle(&mut rng);

        // Update current_index to point to the same song after shuffle
        if let Some(song_id) = current_song_id {
            self.queue.current_index = self.index_of(&song_id);
        }

        // Rebuild order after physical reorder
        self.rebuild_order_and_sync();
        if self.queue.shuffle {
            self.shuffle_order();
        }
        self.clear_queued();
        debug!(" [QUEUE] Queue shuffled, new order preserved");
        self.save_order()?;
        Ok(())
    }

    /// Sort the queue by the given sort mode and direction.
    /// Physically reorders `queue.song_ids` so next/previous follows sorted order.
    /// Preserves the currently-playing song's position via `current_index` update.
    /// `Random` delegates to `shuffle_queue` and ignores `ascending`.
    pub fn sort_queue(&mut self, mode: QueueSortMode, ascending: bool) -> Result<()> {
        if self.queue.song_ids.is_empty() {
            return Ok(());
        }

        if matches!(mode, QueueSortMode::Random) {
            return self.shuffle_queue();
        }

        let current_song_id = self
            .queue
            .current_index
            .and_then(|idx| self.queue.song_ids.get(idx))
            .cloned();

        // Sort IDs by looking up song data from pool
        let pool = &self.pool;
        self.queue.song_ids.sort_by(|a_id, b_id| {
            let a = pool.get(a_id);
            let b = pool.get(b_id);
            let cmp = match (a, b) {
                (Some(a), Some(b)) => match mode {
                    QueueSortMode::Title => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
                    QueueSortMode::Artist => a.artist.to_lowercase().cmp(&b.artist.to_lowercase()),
                    QueueSortMode::Album => a.album.to_lowercase().cmp(&b.album.to_lowercase()),
                    QueueSortMode::Duration => a.duration.cmp(&b.duration),
                    QueueSortMode::Genre => {
                        let a_genre = a.genre.as_deref().unwrap_or("").to_lowercase();
                        let b_genre = b.genre.as_deref().unwrap_or("").to_lowercase();
                        a_genre.cmp(&b_genre)
                    }
                    QueueSortMode::Rating => {
                        let a_rating = a.rating.unwrap_or(0);
                        let b_rating = b.rating.unwrap_or(0);
                        b_rating.cmp(&a_rating)
                    }
                    QueueSortMode::MostPlayed => {
                        let a_count = a.play_count.unwrap_or(0);
                        let b_count = b.play_count.unwrap_or(0);
                        b_count.cmp(&a_count)
                    }
                    // Handled by the early-return above; keep the arm so the
                    // exhaustiveness check passes if a future caller forgets.
                    QueueSortMode::Random => std::cmp::Ordering::Equal,
                },
                _ => std::cmp::Ordering::Equal,
            };
            if ascending { cmp } else { cmp.reverse() }
        });

        // Update current_index to point to the same song after sort
        if let Some(song_id) = current_song_id {
            self.queue.current_index = self.index_of(&song_id);
        }

        // Rebuild order after physical reorder
        self.rebuild_order_and_sync();
        if self.queue.shuffle {
            self.shuffle_order();
        }
        self.clear_queued();
        debug!(
            " [QUEUE] Queue sorted by {:?} ({})",
            mode,
            if ascending { "ASC" } else { "DESC" }
        );
        self.save_order()?;
        Ok(())
    }

    pub fn set_repeat(&mut self, mode: RepeatMode) -> Result<()> {
        self.queue.repeat = mode;
        self.save_order()?;
        Ok(())
    }

    pub fn toggle_consume(&mut self) -> Result<()> {
        self.queue.consume = !self.queue.consume;
        self.save_order()?;
        Ok(())
    }

    pub fn get_current_song(&self) -> Option<Song> {
        self.queue
            .current_index
            .and_then(|idx| self.queue.song_ids.get(idx))
            .and_then(|id| self.pool.get(id))
            .cloned()
    }

    // ── Persistence ──

    /// Save queue ordering (IDs + flags + order array). Uses bincode.
    /// Called on every index change, mode toggle, shuffle/sort.
    pub fn save_order(&self) -> Result<()> {
        self.storage.save_binary(KEY_QUEUE_ORDER, &self.queue)?;
        Ok(())
    }

    /// Save only the song pool. Slower — serializes all Song data.
    /// Called only when songs are added, removed, or modified.
    pub fn save_songs(&self) -> Result<()> {
        self.storage.save_binary(KEY_QUEUE_SONGS, &self.pool)?;
        Ok(())
    }

    /// Save both queue ordering and song pool. Used for mutations that
    /// change both (add, remove, set_queue, reorder + remove).
    pub fn save_all(&self) -> Result<()> {
        self.save_order()?;
        self.save_songs()?;
        Ok(())
    }

    // ── Queue Accessors ──

    pub fn get_queue(&self) -> &Queue {
        &self.queue
    }

    pub fn get_queue_mut(&mut self) -> &mut Queue {
        &mut self.queue
    }

    /// Set the current playback position. Always syncs `current_order` and
    /// clears `queued` so the order array stays consistent.
    /// Use this instead of setting `queue.current_index` directly.
    pub fn set_current_index(&mut self, index: Option<usize>) {
        self.queue.current_index = index;
        self.sync_current_order_to_index();
        self.clear_queued();
    }

    // ══════════════════════════════════════════════════════════════════════
    //  Queue Item Operations
    // ══════════════════════════════════════════════════════════════════════

    /// Move a song from one position to another in the queue.
    /// Used for drag-and-drop reordering.
    /// Updates `current_index` so the currently-playing song isn't lost.
    pub fn move_item(&mut self, from: usize, to: usize) -> Result<()> {
        let len = self.queue.song_ids.len();
        if from >= len || to > len || from == to {
            return Ok(());
        }

        let item = self.queue.song_ids.remove(from);
        let insert_at = if from < to { to - 1 } else { to };
        self.queue.song_ids.insert(insert_at, item);

        // Adjust current_index to keep tracking the same song
        if let Some(cur) = self.queue.current_index {
            self.queue.current_index = Some(if cur == from {
                // The playing song itself was moved
                insert_at
            } else if from < cur && cur <= insert_at {
                // Item moved forward past the playing song — playing song shifts back
                cur - 1
            } else if insert_at <= cur && cur < from {
                // Item moved backward past the playing song — playing song shifts forward
                cur + 1
            } else {
                cur
            });
        }

        // Rebuild order after move (indices changed)
        self.rebuild_order_and_sync();
        if self.queue.shuffle {
            self.shuffle_order();
        }
        self.clear_queued();
        debug!(
            "📦 [QUEUE] Moved item from {} to {} (inserted at {})",
            from, to, insert_at
        );
        self.save_order()?;
        Ok(())
    }

    /// Insert songs right after the currently playing position ("Play Next").
    /// If nothing is playing, appends to the end.
    /// Does NOT change `current_index` — the currently playing song stays the same.
    pub fn insert_after_current(&mut self, mut songs: Vec<Song>) -> Result<()> {
        let insert_pos = self
            .queue
            .current_index
            .map_or(self.queue.song_ids.len(), |idx| idx + 1);

        let clamped = insert_pos.min(self.queue.song_ids.len());
        let count = songs.len();

        self.assign_original_positions(&mut songs);

        // Insert IDs in reverse so they end up in order, and add to pool
        for song in songs.into_iter().rev() {
            self.queue.song_ids.insert(clamped, song.id.clone());
            self.pool.insert(song);
        }

        // Update order array for the insertion
        self.insert_into_order(clamped, count);

        // Adjust current_index for songs inserted before it
        if let Some(cur) = self.queue.current_index
            && clamped <= cur
        {
            self.queue.current_index = Some(cur + count);
        }

        self.clear_queued();
        debug!("📦 [QUEUE] Inserted songs after current (pos {})", clamped);
        self.save_all()?;
        Ok(())
    }

    /// Insert a song at a specific index in the queue.
    /// Used to re-insert songs from history that were removed (consume mode).
    pub fn insert_song_at(&mut self, index: usize, song: Song) -> Result<()> {
        let clamped = index.min(self.queue.song_ids.len());
        self.queue.song_ids.insert(clamped, song.id.clone());
        self.pool.insert(song);

        // Update order array for the insertion
        self.insert_into_order(clamped, 1);

        self.queue.current_index = Some(clamped);
        self.sync_current_order_to_index();
        self.clear_queued();
        self.save_all()?;
        Ok(())
    }

    /// Insert multiple songs at a specific index in the queue.
    /// Used for cross-pane drag-and-drop (browsing panel → queue at drop position).
    /// Does NOT change `current_index` to point at the inserted songs, but adjusts
    /// it forward if the insertion point is before the currently-playing song.
    pub fn insert_songs_at(&mut self, index: usize, mut songs: Vec<Song>) -> Result<()> {
        if songs.is_empty() {
            return Ok(());
        }
        let clamped = index.min(self.queue.song_ids.len());
        let count = songs.len();

        self.assign_original_positions(&mut songs);

        // Insert in reverse so they end up in order at `clamped`
        for song in songs.into_iter().rev() {
            self.queue.song_ids.insert(clamped, song.id.clone());
            self.pool.insert(song);
        }

        // Update order array for the insertion
        self.insert_into_order(clamped, count);

        // Adjust current_index: if inserting before the playing song, shift it forward
        if let Some(cur) = self.queue.current_index
            && clamped <= cur
        {
            self.queue.current_index = Some(cur + count);
        }

        self.clear_queued();
        debug!(
            "📦 [QUEUE] Inserted {} songs at position {}",
            count, clamped
        );
        self.save_all()?;
        Ok(())
    }

    /// Update the rating for a song in the persisted queue by song ID (O(1)).
    pub fn update_song_rating(&mut self, song_id: &str, rating: Option<u32>) -> Result<()> {
        if let Some(song) = self.pool.get_mut(song_id) {
            song.rating = rating;
            self.save_songs()?;
        }
        Ok(())
    }

    /// Update the starred status for a song in the persisted queue by song ID (O(1)).
    pub fn update_song_starred(&mut self, song_id: &str, starred: bool) -> Result<()> {
        if let Some(song) = self.pool.get_mut(song_id) {
            song.starred = starred;
            self.save_songs()?;
        }
        Ok(())
    }

    /// Bump the play count for a song in the persisted queue by 1 (O(1)).
    /// `None` becomes `Some(1)`.
    pub fn increment_song_play_count(&mut self, song_id: &str) -> Result<()> {
        if let Some(song) = self.pool.get_mut(song_id) {
            let next = song.play_count.unwrap_or(0).saturating_add(1);
            song.play_count = Some(next);
            self.save_songs()?;
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::types::song::Song;

    pub(crate) fn make_test_song(id: &str) -> Song {
        Song::test_default(id, &format!("Song {id}"))
    }

    pub(crate) fn make_test_manager(
        songs: Vec<Song>,
        current_index: Option<usize>,
    ) -> QueueManager {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp_dir = std::env::temp_dir();
        let db_path = temp_dir.join(format!("test_queue_{}_{}.redb", std::process::id(), id));
        let _ = std::fs::remove_file(&db_path);
        let storage = StateStorage::new(db_path).expect("temp storage");
        let mut qm = QueueManager::new(storage).expect("queue manager");
        let ids: Vec<String> = songs.iter().map(|s| s.id.clone()).collect();
        qm.pool.insert_many(songs);
        qm.queue.song_ids = ids;
        qm.queue.current_index = current_index;
        qm.rebuild_order_and_sync();
        qm
    }

    #[test]
    fn move_item_forward() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let mut qm = make_test_manager(songs, None);

        qm.move_item(0, 2).unwrap();
        let ids: Vec<&str> = qm.queue.song_ids.iter().map(|s| s.as_str()).collect();
        assert_eq!(ids, vec!["b", "a", "c"]);
    }

    #[test]
    fn move_item_backward() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let mut qm = make_test_manager(songs, None);

        qm.move_item(2, 0).unwrap();
        let ids: Vec<&str> = qm.queue.song_ids.iter().map(|s| s.as_str()).collect();
        assert_eq!(ids, vec!["c", "a", "b"]);
    }

    #[test]
    fn move_item_same_position_is_noop() {
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let mut qm = make_test_manager(songs, None);

        qm.move_item(1, 1).unwrap();
        let ids: Vec<&str> = qm.queue.song_ids.iter().map(|s| s.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"]);
    }

    #[test]
    fn move_item_out_of_bounds_is_noop() {
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let mut qm = make_test_manager(songs, None);

        qm.move_item(5, 0).unwrap();
        let ids: Vec<&str> = qm.queue.song_ids.iter().map(|s| s.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"]);
    }

    #[test]
    fn move_item_updates_current_index_when_playing_song_moved_forward() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let mut qm = make_test_manager(songs, Some(0));

        qm.move_item(0, 2).unwrap();
        assert_eq!(qm.queue.current_index, Some(1));
        assert_eq!(qm.queue.song_ids[1], "a");
    }

    #[test]
    fn move_item_updates_current_index_when_playing_song_moved_backward() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let mut qm = make_test_manager(songs, Some(2));

        qm.move_item(2, 0).unwrap();
        assert_eq!(qm.queue.current_index, Some(0));
        assert_eq!(qm.queue.song_ids[0], "c");
    }

    #[test]
    fn move_item_shifts_current_index_when_item_moved_past_playing() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let mut qm = make_test_manager(songs, Some(1));

        qm.move_item(0, 2).unwrap();
        assert_eq!(qm.queue.current_index, Some(0));
        assert_eq!(qm.queue.song_ids[0], "b");
    }

    #[test]
    fn move_item_shifts_current_index_when_item_moved_before_playing() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let mut qm = make_test_manager(songs, Some(1));

        qm.move_item(2, 0).unwrap();
        assert_eq!(qm.queue.current_index, Some(2));
        assert_eq!(qm.queue.song_ids[2], "b");
    }

    #[test]
    fn move_item_to_end_of_two_item_queue() {
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let mut qm = make_test_manager(songs, None);

        // from=0, to=2 (== len) means "place after the last item"
        qm.move_item(0, 2).unwrap();
        let ids: Vec<&str> = qm.queue.song_ids.iter().map(|s| s.as_str()).collect();
        assert_eq!(ids, vec!["b", "a"]);
    }

    // remove_song current_index tracking tests

    #[test]
    fn remove_song_before_current_decrements_index() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let mut qm = make_test_manager(songs, Some(2)); // playing "c"

        qm.remove_song(0).unwrap(); // remove "a"
        assert_eq!(qm.queue.current_index, Some(1)); // "c" shifted from 2→1
        assert_eq!(qm.queue.song_ids[1], "c");
    }

    #[test]
    fn remove_song_after_current_no_change() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let mut qm = make_test_manager(songs, Some(0)); // playing "a"

        qm.remove_song(2).unwrap(); // remove "c"
        assert_eq!(qm.queue.current_index, Some(0)); // unchanged
        assert_eq!(qm.queue.song_ids[0], "a");
    }

    #[test]
    fn remove_song_at_current_clamps_index() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let mut qm = make_test_manager(songs, Some(2)); // playing "c" (last)

        qm.remove_song(2).unwrap(); // remove "c"
        assert_eq!(qm.queue.current_index, Some(1)); // clamped to last valid
    }

    #[test]
    fn remove_song_until_empty_clears_index() {
        let songs = vec![make_test_song("a")];
        let mut qm = make_test_manager(songs, Some(0));

        qm.remove_song(0).unwrap();
        assert_eq!(qm.queue.current_index, None);
        assert!(qm.queue.song_ids.is_empty());
    }

    #[test]
    fn remove_multiple_songs_before_current_tracks_correctly() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
            make_test_song("d"),
            make_test_song("e"),
        ];
        let mut qm = make_test_manager(songs, Some(4)); // playing "e" at index 4

        qm.remove_song(0).unwrap(); // remove "a" → current becomes 3
        qm.remove_song(0).unwrap(); // remove "b" → current becomes 2
        qm.remove_song(0).unwrap(); // remove "c" → current becomes 1

        assert_eq!(qm.queue.current_index, Some(1));
        assert_eq!(qm.queue.song_ids[1], "e");
    }

    // SongPool integration tests

    #[test]
    fn pool_get_returns_song_data() {
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let qm = make_test_manager(songs, None);

        assert_eq!(qm.get_song("a").unwrap().title, "Song a");
        assert_eq!(qm.get_song("b").unwrap().title, "Song b");
        assert!(qm.get_song("nonexistent").is_none());
    }

    #[test]
    fn save_order_does_not_include_song_data() {
        let songs = vec![make_test_song("x"), make_test_song("y")];
        let qm = make_test_manager(songs, Some(0));

        // Save order only
        qm.save_order().unwrap();

        // Load via bincode
        let raw_order: Option<Queue> = qm.storage.load_binary(KEY_QUEUE_ORDER).unwrap();
        let queue = raw_order.unwrap();
        assert_eq!(queue.song_ids, vec!["x", "y"]);
        assert_eq!(queue.current_index, Some(0));
    }

    // ── Order Array Tests ──

    #[test]
    fn order_array_identity_when_shuffle_off() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let qm = make_test_manager(songs, Some(0));

        assert_eq!(qm.queue.order, vec![0, 1, 2]);
        assert_eq!(qm.queue.current_order, Some(0));
    }

    #[test]
    fn order_array_shuffled_preserves_current() {
        let songs: Vec<Song> = (0..10).map(|i| make_test_song(&i.to_string())).collect();
        let mut qm = make_test_manager(songs, Some(3));

        // Toggle shuffle on
        qm.toggle_shuffle().unwrap();

        // current_order should point to the same song
        let cur_order = qm.queue.current_order.unwrap();
        let cur_song_idx = qm.queue.order[cur_order];
        assert_eq!(cur_song_idx, 3); // still points to song at index 3
    }

    #[test]
    fn peek_next_returns_order_sequence() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
            make_test_song("d"),
            make_test_song("e"),
        ];
        let mut qm = make_test_manager(songs, Some(0));

        // Peek should return song at index 1 (next in order)
        let peeked = qm.peek_next_song().unwrap();
        assert_eq!(peeked.index(), 1);
        assert_eq!(peeked.song().id, "b");
    }

    #[test]
    fn peek_next_shuffle_returns_order_array_entry() {
        let songs: Vec<Song> = (0..10).map(|i| make_test_song(&i.to_string())).collect();
        let mut qm = make_test_manager(songs, Some(0));

        qm.toggle_shuffle().unwrap();

        // Capture expected index BEFORE peek to avoid the guard's borrow on qm.
        let expected_idx = qm.queue.order[1];
        let peeked = qm.peek_next_song().unwrap();
        // Should return order[1] (whatever that maps to after shuffle)
        assert_eq!(peeked.index(), expected_idx);
    }

    #[test]
    fn transition_to_queued_advances_current() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let mut qm = make_test_manager(songs, Some(0));

        // Peek + transition via the guard
        let peeked = qm.peek_next_song().unwrap();
        let result = peeked.transition();
        assert_eq!(result.new_index, 1);
        assert_eq!(result.old_index, Some(0));
        assert_eq!(qm.queue.current_index, Some(1));
        assert_eq!(qm.queue.current_order, Some(1));
    }

    #[test]
    fn transition_consumes_queued() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let mut qm = make_test_manager(songs, Some(0));

        let peeked = qm.peek_next_song().unwrap();
        // peeked's existence implies queued is set (guard invariant).
        peeked.transition();
        assert!(qm.queue.queued.is_none());
    }

    #[test]
    fn queue_mutation_clears_queued() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let mut qm = make_test_manager(songs, Some(0));

        // Set queued directly (the guard's drop semantics would otherwise
        // clear it before we can observe the mutation's effect).
        qm.queue.queued = Some(1);
        assert!(qm.queue.queued.is_some());

        // Add a song — should clear queued
        qm.add_songs(vec![make_test_song("d")]).unwrap();
        assert!(qm.queue.queued.is_none());
    }

    #[test]
    fn remove_song_adjusts_order_indices() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
            make_test_song("d"),
        ];
        let mut qm = make_test_manager(songs, Some(0));

        // Order is [0, 1, 2, 3]. Remove song at index 1 ("b")
        qm.remove_song(1).unwrap();

        // Order should now be [0, 1, 2] (indices adjusted)
        assert_eq!(qm.queue.order, vec![0, 1, 2]);
        assert_eq!(qm.queue.song_ids, vec!["a", "c", "d"]);
    }

    #[test]
    fn add_songs_extends_order() {
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let mut qm = make_test_manager(songs, Some(0));

        assert_eq!(qm.queue.order, vec![0, 1]);

        qm.add_songs(vec![make_test_song("c"), make_test_song("d")])
            .unwrap();

        // Order should include new entries
        assert_eq!(qm.queue.order.len(), 4);
        // All indices [0, 1, 2, 3] should be present
        let mut sorted_order = qm.queue.order.clone();
        sorted_order.sort();
        assert_eq!(sorted_order, vec![0, 1, 2, 3]);
    }

    #[test]
    fn unshuffle_restores_identity() {
        let songs: Vec<Song> = (0..10).map(|i| make_test_song(&i.to_string())).collect();
        let mut qm = make_test_manager(songs, Some(3));

        // Shuffle
        qm.toggle_shuffle().unwrap();
        assert!(qm.queue.shuffle);

        // Unshuffle
        qm.toggle_shuffle().unwrap();
        assert!(!qm.queue.shuffle);
        assert_eq!(qm.queue.order, (0..10).collect::<Vec<_>>());
    }

    // ══════════════════════════════════════════════════════════════════════
    //  Sort / Shuffle / Insert current_index Tracking
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn sort_queue_preserves_current_song_identity() {
        use crate::types::queue_sort_mode::QueueSortMode;

        let mut songs: Vec<Song> = ["c", "a", "b"]
            .iter()
            .map(|t| {
                let mut s = make_test_song(t);
                s.title = t.to_string();
                s
            })
            .collect();
        // Give them different titles so sort actually reorders
        songs[0].title = "Charlie".to_string();
        songs[1].title = "Alpha".to_string();
        songs[2].title = "Bravo".to_string();

        let mut qm = make_test_manager(songs, Some(0)); // playing "c" = "Charlie"
        qm.sort_queue(QueueSortMode::Title, true).unwrap();

        // After title sort ascending: Alpha, Bravo, Charlie
        // "c" (Charlie) should now be at index 2
        assert_eq!(qm.queue.current_index, Some(2));
        assert_eq!(qm.queue.song_ids[2], "c");
    }

    #[test]
    fn sort_queue_empty_is_noop() {
        use crate::types::queue_sort_mode::QueueSortMode;

        let mut qm = make_test_manager(vec![], None);
        qm.sort_queue(QueueSortMode::Title, true).unwrap();
        assert!(qm.queue.song_ids.is_empty());
        assert_eq!(qm.queue.current_index, None);
    }

    #[test]
    fn sort_queue_by_most_played_orders_highest_first() {
        use crate::types::queue_sort_mode::QueueSortMode;

        let mut songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        songs[0].play_count = Some(5);
        songs[1].play_count = Some(20);
        songs[2].play_count = Some(10);
        let mut qm = make_test_manager(songs, Some(0));

        // ascending=true mirrors Rating's pre-flip convention: highest first.
        qm.sort_queue(QueueSortMode::MostPlayed, true).unwrap();
        assert_eq!(qm.queue.song_ids, vec!["b", "c", "a"]);
    }

    #[test]
    fn sort_queue_by_most_played_treats_none_as_zero() {
        use crate::types::queue_sort_mode::QueueSortMode;

        let mut songs = vec![make_test_song("a"), make_test_song("b")];
        songs[0].play_count = None;
        songs[1].play_count = Some(3);
        let mut qm = make_test_manager(songs, None);

        qm.sort_queue(QueueSortMode::MostPlayed, true).unwrap();
        assert_eq!(qm.queue.song_ids, vec!["b", "a"]);
    }

    #[test]
    fn shuffle_queue_preserves_current_song_identity() {
        let songs: Vec<Song> = (0..20).map(|i| make_test_song(&i.to_string())).collect();
        let mut qm = make_test_manager(songs, Some(7)); // playing "7"

        qm.shuffle_queue().unwrap();

        // current_index should point to "7" wherever it ended up
        let idx = qm.queue.current_index.unwrap();
        assert_eq!(
            qm.queue.song_ids[idx], "7",
            "playing song identity lost after shuffle"
        );
    }

    #[test]
    fn insert_after_current_does_not_shift_playing_song() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let mut qm = make_test_manager(songs, Some(1)); // playing "b" at 1

        let new_songs = vec![make_test_song("x"), make_test_song("y")];
        qm.insert_after_current(new_songs).unwrap();

        // insert_after_current inserts at pos 2 (after current=1)
        // Since 2 > 1, current_index should NOT shift
        assert_eq!(qm.queue.current_index, Some(1));
        assert_eq!(qm.queue.song_ids[1], "b");
        // New songs at 2,3
        assert_eq!(qm.queue.song_ids[2], "x");
        assert_eq!(qm.queue.song_ids[3], "y");
    }

    #[test]
    fn insert_after_current_when_nothing_playing() {
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let mut qm = make_test_manager(songs, None);

        let new_songs = vec![make_test_song("x")];
        qm.insert_after_current(new_songs).unwrap();

        // With no current_index, inserts at end
        assert_eq!(qm.queue.song_ids.len(), 3);
        assert_eq!(qm.queue.song_ids[2], "x");
        assert_eq!(qm.queue.current_index, None);
    }

    #[test]
    fn insert_songs_at_before_current_shifts_index() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
            make_test_song("d"),
        ];
        let mut qm = make_test_manager(songs, Some(3)); // playing "d" at 3

        let new_songs = vec![make_test_song("x"), make_test_song("y")];
        qm.insert_songs_at(1, new_songs).unwrap();

        // Inserted 2 songs at index 1 (before current=3)
        // current_index should shift to 5
        assert_eq!(qm.queue.current_index, Some(5));
        assert_eq!(qm.queue.song_ids[5], "d");
    }

    #[test]
    fn insert_songs_at_after_current_no_shift() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let mut qm = make_test_manager(songs, Some(1)); // playing "b" at 1

        let new_songs = vec![make_test_song("x")];
        qm.insert_songs_at(3, new_songs).unwrap(); // insert after end

        assert_eq!(qm.queue.current_index, Some(1)); // unchanged
        assert_eq!(qm.queue.song_ids[1], "b");
    }

    #[test]
    fn add_songs_does_not_affect_current_index() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let mut qm = make_test_manager(songs, Some(2)); // playing "c" at 2

        let new_songs = vec![
            make_test_song("x"),
            make_test_song("y"),
            make_test_song("z"),
        ];
        qm.add_songs(new_songs).unwrap();

        assert_eq!(qm.queue.current_index, Some(2)); // unchanged
        assert_eq!(qm.queue.song_ids[2], "c");
        assert_eq!(qm.queue.song_ids.len(), 6);
    }

    #[test]
    fn increment_song_play_count_bumps_existing_value() {
        let mut song = make_test_song("a");
        song.play_count = Some(3);
        let mut qm = make_test_manager(vec![song], Some(0));

        qm.increment_song_play_count("a").unwrap();
        assert_eq!(qm.pool.get("a").unwrap().play_count, Some(4));
    }

    #[test]
    fn increment_song_play_count_starts_from_none() {
        let mut song = make_test_song("a");
        song.play_count = None;
        let mut qm = make_test_manager(vec![song], Some(0));

        qm.increment_song_play_count("a").unwrap();
        assert_eq!(qm.pool.get("a").unwrap().play_count, Some(1));
    }

    #[test]
    fn increment_song_play_count_unknown_id_is_noop() {
        let mut song = make_test_song("a");
        song.play_count = Some(2);
        let mut qm = make_test_manager(vec![song], Some(0));

        qm.increment_song_play_count("nonexistent").unwrap();
        assert_eq!(qm.pool.get("a").unwrap().play_count, Some(2));
    }

    // ══════════════════════════════════════════════════════════════════════
    //  ID-Based Removal (immune to index drift)
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn remove_song_by_id_removes_correct_song() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
            make_test_song("d"),
        ];
        let mut qm = make_test_manager(songs, Some(0));

        qm.remove_song_by_id("c").unwrap();

        assert_eq!(qm.queue.song_ids, vec!["a", "b", "d"]);
        assert!(qm.pool.get("c").is_none());
    }

    #[test]
    fn remove_song_by_id_unknown_id_is_noop() {
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let mut qm = make_test_manager(songs, Some(0));

        qm.remove_song_by_id("nonexistent").unwrap();

        assert_eq!(qm.queue.song_ids, vec!["a", "b"]);
        assert_eq!(qm.queue.current_index, Some(0));
    }

    #[test]
    fn remove_song_by_id_adjusts_current_index() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let mut qm = make_test_manager(songs, Some(2)); // playing "c"

        // Remove "a" (before current) — current should shift back
        qm.remove_song_by_id("a").unwrap();
        assert_eq!(qm.queue.song_ids, vec!["b", "c"]);
        assert_eq!(qm.queue.current_index, Some(1)); // still points at "c"
    }

    #[test]
    fn remove_songs_by_ids_removes_all_specified() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
            make_test_song("d"),
            make_test_song("e"),
        ];
        let mut qm = make_test_manager(songs, Some(0));

        qm.remove_songs_by_ids(&["b".to_string(), "d".to_string()])
            .unwrap();

        assert_eq!(qm.queue.song_ids, vec!["a", "c", "e"]);
        assert!(qm.pool.get("b").is_none());
        assert!(qm.pool.get("d").is_none());
        assert!(qm.pool.get("a").is_some());
    }

    #[test]
    fn remove_songs_by_ids_handles_partial_unknown() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let mut qm = make_test_manager(songs, Some(0));

        qm.remove_songs_by_ids(&["b".to_string(), "nonexistent".to_string(), "c".to_string()])
            .unwrap();

        assert_eq!(qm.queue.song_ids, vec!["a"]);
    }

    #[test]
    fn remove_songs_by_ids_resolves_indices_per_step() {
        // Regression: a naive impl that snapshots indices upfront and removes
        // ascending would shift later positions and remove the wrong songs.
        // ID-based resolution must look up each ID against the current state.
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
            make_test_song("d"),
        ];
        let mut qm = make_test_manager(songs, Some(0));

        // IDs deliberately given in ascending-index order — the buggy version
        // (snapshot indices, remove ascending) would mistakenly remove "b" and "d".
        qm.remove_songs_by_ids(&["a".to_string(), "c".to_string()])
            .unwrap();

        assert_eq!(qm.queue.song_ids, vec!["b", "d"]);
    }

    #[test]
    fn remove_songs_by_ids_empty_is_noop() {
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let mut qm = make_test_manager(songs, Some(0));

        qm.remove_songs_by_ids(&[]).unwrap();

        assert_eq!(qm.queue.song_ids, vec!["a", "b"]);
        assert_eq!(qm.queue.current_index, Some(0));
    }
}
