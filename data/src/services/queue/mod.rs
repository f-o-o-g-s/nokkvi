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
mod write_guard;

use anyhow::Result;
pub use navigation::{NextSongResult, PeekedQueue, PreviousSongResult, TransitionResult};
use rand::seq::SliceRandom;
use tracing::{debug, warn};

use crate::{
    services::state_storage::StateStorage,
    types::{
        NextTrackResetEffect,
        queue::{MoveBatchTarget, Queue, RepeatMode},
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
    /// Per-row unique identifiers, parallel to `queue.song_ids`. Two queue
    /// entries that share a `song_id` (duplicate adds, "Play Next" of an
    /// already-queued song) still get distinct `entry_id`s, so right-click
    /// "Remove from queue" can target a single row.
    ///
    /// Runtime-only — rebuilt from scratch on every `QueueManager::new()`,
    /// so a persisted queue snapshot loads fine on an older client and the
    /// IDs start fresh on relaunch.
    pub(crate) entry_ids: Vec<u64>,
    /// Monotonic counter that hands out the next `entry_id`. Never reused
    /// within a process lifetime.
    pub(crate) next_entry_id: u64,
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
// Re-exported from `services::storage_keys`; see that module for the
// on-disk-compat invariant.
const KEY_QUEUE_ORDER: &str = crate::services::storage_keys::QUEUE_ORDER;
const KEY_QUEUE_SONGS: &str = crate::services::storage_keys::QUEUE_SONGS;

/// Cross-validate a freshly-loaded [`Queue`] against its [`SongPool`] and
/// repair any inconsistency in place. Returns `true` (`dirty`) when the
/// queue was changed, so the caller can persist the cleaned state.
///
/// Steps:
/// 1. Prune `song_ids` whose id is absent from `pool`, building an
///    old→new index remap.
/// 2. Rewrite `order` through the remap, dropping missing entries. If the
///    result is not a valid permutation of `0..new_len`, fall back to the
///    canonical identity order (matches `rebuild_order`) and drop
///    `current_order`.
/// 3. Remap `current_index` through the old→new map; if its row was pruned,
///    clamp to the last valid index (or `None` when the queue is now empty).
/// 4. Re-derive `current_order` from `current_index`'s position in `order`.
/// 5. ALWAYS clear `queued` — gapless-prep transient, never valid across a
///    relaunch.
///
/// Pure and non-panicking; unit-testable without redb.
fn reconcile_loaded_queue(queue: &mut Queue, pool: &SongPool) -> bool {
    let old_len = queue.song_ids.len();

    // (1) Prune missing ids, building an old→new index remap.
    let mut remap: Vec<Option<usize>> = Vec::with_capacity(old_len);
    let mut pruned_ids: Vec<String> = Vec::with_capacity(old_len);
    for id in &queue.song_ids {
        if pool.get(id).is_some() {
            remap.push(Some(pruned_ids.len()));
            pruned_ids.push(id.clone());
        } else {
            remap.push(None);
        }
    }
    let new_len = pruned_ids.len();
    let pruned_any = new_len != old_len;

    // Always clear the transient gapless-prep field on restore.
    let had_queued = queue.queued.is_some();
    queue.queued = None;

    if !pruned_any {
        // No rows dropped: validate/repair the order array, clamp an
        // out-of-range playhead, and re-derive current_order so a stale
        // persisted order can't desync the playhead.
        let order_was_valid = order_is_identity_permutation(&queue.order, old_len);
        if !order_was_valid {
            queue.order = (0..old_len).collect();
        }
        let index_was_in_range = clamp_current_index(queue, old_len);
        sync_current_order(queue);
        return had_queued || !order_was_valid || !index_was_in_range;
    }

    queue.song_ids = pruned_ids;

    // (2) Rewrite order through the remap, dropping pruned entries.
    let remapped_order: Vec<usize> = queue
        .order
        .iter()
        .filter_map(|&old_idx| remap.get(old_idx).copied().flatten())
        .collect();
    queue.order = if order_is_identity_permutation(&remapped_order, new_len) {
        remapped_order
    } else {
        // Canonical identity fallback (deterministic, matches rebuild_order).
        queue.current_order = None;
        (0..new_len).collect()
    };

    // (3) Remap or clamp current_index.
    queue.current_index = match queue.current_index {
        Some(old) => match remap.get(old).copied().flatten() {
            Some(new) => Some(new),
            None if new_len == 0 => None,
            // Its row was pruned — clamp to the last valid index.
            None => Some(new_len - 1),
        },
        None => None,
    };

    // (4) Re-derive current_order from current_index's position in order.
    sync_current_order(queue);

    true
}

/// `true` when `order` is exactly a permutation of `0..len` (no out-of-range
/// entry, no duplicate, correct length).
fn order_is_identity_permutation(order: &[usize], len: usize) -> bool {
    if order.len() != len {
        return false;
    }
    let mut seen = vec![false; len];
    for &idx in order {
        match seen.get_mut(idx) {
            Some(slot) if !*slot => *slot = true,
            _ => return false,
        }
    }
    true
}

/// Clamp `current_index` into `[0, len)`. Returns `false` when the index was
/// out of range and had to be corrected (clamped to `len-1`, or `None` when
/// the queue is empty), so the caller can mark the queue dirty.
fn clamp_current_index(queue: &mut Queue, len: usize) -> bool {
    match queue.current_index {
        Some(idx) if idx < len => true,
        Some(_) if len == 0 => {
            queue.current_index = None;
            false
        }
        Some(_) => {
            queue.current_index = Some(len - 1);
            false
        }
        None => true,
    }
}

/// Set `current_order` to the position of `current_index` within `order`
/// (or `None` when there is no current index / no match).
fn sync_current_order(queue: &mut Queue) {
    queue.current_order = queue
        .current_index
        .and_then(|idx| queue.order.iter().position(|&o| o == idx));
}

impl QueueManager {
    pub fn new(storage: StateStorage) -> Result<Self> {
        // ORDER load is best-effort: a physically-corrupt or
        // schema-incompatible Queue blob must never abort AppService
        // construction (which would bounce the user to the login screen).
        // Degrade to an empty queue on decode error, mirroring the pool's
        // existing degradation below. The bad blob self-heals on the next
        // save_order once a track plays.
        let loaded_order: Option<Queue> = match storage.load_binary::<Queue>(KEY_QUEUE_ORDER) {
            Ok(opt) => opt,
            Err(e) => {
                warn!(" [QUEUE] Failed to load persisted queue order, starting empty: {e}");
                None
            }
        };

        let (mut queue, pool) = if let Some(queue) = loaded_order {
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

        // Reconcile the independently-loaded ORDER and POOL: prune queue rows
        // whose id is absent from the pool, remap the order array + playhead,
        // and always clear the transient `queued` field. A torn write (pre-fix
        // I28), server-side id churn, or an empty-pool fallback can otherwise
        // leave current_index/order pointing at ids the pool no longer holds.
        let dirty = reconcile_loaded_queue(&mut queue, &pool);

        // Seed runtime entry_ids parallel to the (now pruned) song_ids. The
        // counter starts past the seeded range so subsequent inserts cannot
        // collide. MUST run AFTER reconcile so entry_ids stay strictly
        // parallel to the pruned song_ids.
        let initial_len = queue.song_ids.len();
        let entry_ids: Vec<u64> = (0..initial_len as u64).collect();
        let next_entry_id = initial_len as u64;

        let mgr = Self {
            queue,
            pool,
            storage,
            playback_history: Vec::new(),
            max_history_size: 100,
            entry_ids,
            next_entry_id,
        };

        // Persist the cleaned state so the repair is durable (best-effort: a
        // failed save must not abort login — the in-memory queue is already
        // consistent).
        if dirty && let Err(e) = mgr.save_all() {
            warn!(" [QUEUE] Failed to persist reconciled queue on load: {e}");
        }

        Ok(mgr)
    }

    // ── Song Pool Accessors ──

    /// Look up a song by ID from the pool (O(1)).
    pub fn get_song(&self, id: &str) -> Option<&Song> {
        self.pool.get(id)
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
    fn index_of(&self, song_id: &str) -> Option<usize> {
        self.queue.song_ids.iter().position(|id| id == song_id)
    }

    /// Read-only access to the per-row `entry_id` array (parallel to
    /// `queue.song_ids`). Used by `transform_songs_from_pool` so each
    /// `QueueSongUIViewData` carries the row identifier the view layer
    /// echoes back on right-click removal.
    pub fn entry_ids(&self) -> &[u64] {
        &self.entry_ids
    }

    /// Look up the `entry_id` for a queue position. `None` if `index` is
    /// out of bounds.
    pub fn entry_id_at(&self, index: usize) -> Option<u64> {
        self.entry_ids.get(index).copied()
    }

    /// O(n) scan to find the queue position holding a given `entry_id`.
    pub fn index_of_entry(&self, entry_id: u64) -> Option<usize> {
        self.entry_ids.iter().position(|&id| id == entry_id)
    }

    /// Hand out `count` fresh, never-reused `entry_id`s.
    fn allocate_entry_ids(&mut self, count: usize) -> Vec<u64> {
        let start = self.next_entry_id;
        self.next_entry_id = self
            .next_entry_id
            .checked_add(count as u64)
            .expect("queue entry_id counter overflow");
        (start..start + count as u64).collect()
    }

    /// Test-only fast-path that replaces `song_ids` and reseeds the parallel
    /// `entry_ids` in lockstep. Pool insertion is the caller's responsibility.
    ///
    /// Production code MUST NOT touch `queue.song_ids` directly — every
    /// mutator pairs the song_ids change with the matching entry_ids work.
    /// This helper exists so test fixtures don't have to copy-paste the
    /// invariant-restoration ritual (and so a future contributor reading
    /// the test code can't mistake the field-bypass idiom for production
    /// usage).
    #[cfg(test)]
    pub(crate) fn replace_song_ids_for_test(
        &mut self,
        song_ids: Vec<String>,
        current_index: Option<usize>,
    ) {
        let count = song_ids.len();
        self.queue.song_ids = song_ids;
        self.entry_ids = self.allocate_entry_ids(count);
        self.queue.current_index = current_index;
        self.rebuild_order_and_sync();
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

    pub fn add_songs(&mut self, mut songs: Vec<Song>) -> Result<NextTrackResetEffect> {
        self.assign_original_positions(&mut songs);
        let count = songs.len();
        let fresh_entry_ids = self.allocate_entry_ids(count);
        let mut tx = self.write();
        let start_idx = tx.queue.song_ids.len();

        // Add IDs to ordering, songs to pool
        for song in &songs {
            tx.queue.song_ids.push(song.id.clone());
        }
        tx.entry_ids.extend(fresh_entry_ids);
        tx.pool.insert_many(songs);

        // Extend order array with new indices
        tx.extend_order(start_idx..start_idx + count);
        tx.commit_save_all()
    }

    pub fn set_queue(
        &mut self,
        mut songs: Vec<Song>,
        current_index: Option<usize>,
    ) -> Result<NextTrackResetEffect> {
        // Assign original_position to capture insertion order
        for (i, song) in songs.iter_mut().enumerate() {
            song.original_position = Some(i as u32);
        }
        let fresh_entry_ids = self.allocate_entry_ids(songs.len());
        let mut tx = self.write();
        tx.queue.song_ids = songs.iter().map(|s| s.id.clone()).collect();
        tx.entry_ids = fresh_entry_ids;
        tx.queue.current_index = current_index;
        // Clear and rebuild pool
        tx.pool.clear();
        tx.pool.insert_many(songs);
        // Clear history on context switch (new album/playlist) — Spotify behavior
        tx.playback_history.clear();
        // Rebuild order array and sync
        tx.rebuild_order_and_sync();
        // If shuffle is on, shuffle the new order
        if tx.queue.shuffle {
            tx.shuffle_order();
        }
        tx.commit_save_all()
    }

    pub fn remove_song(&mut self, index: usize) -> Result<NextTrackResetEffect> {
        if index >= self.queue.song_ids.len() {
            return Ok(NextTrackResetEffect::new());
        }
        let mut tx = self.write();
        let removed_id = tx.queue.song_ids.remove(index);
        if index < tx.entry_ids.len() {
            tx.entry_ids.remove(index);
        }
        // Only drop the pool entry when no other queue row still references
        // this song_id — a duplicate add keeps the pool alive for survivors.
        if !tx.queue.song_ids.iter().any(|id| id == &removed_id) {
            tx.pool.remove(&removed_id);
        }

        // Remove from order array and adjust indices
        tx.remove_from_order(index);

        // Adjust current_index to keep tracking the same playing song
        if let Some(cur) = tx.queue.current_index {
            if tx.queue.song_ids.is_empty() {
                // Queue is now empty
                tx.queue.current_index = None;
            } else if index < cur {
                // Removed before current — shift back
                tx.queue.current_index = Some(cur - 1);
            } else if index == cur {
                // Removed the current song. remove_from_order has already moved
                // current_order onto the play-order slot the next surviving
                // upcoming song slid into, so derive current_index from
                // order[current_order] to restore the invariant
                // order[current_order] == current_index. Under a shuffled
                // (non-identity) order a bare physical clamp (cur.min(len-1))
                // diverges from the order clamp, desyncing the two and causing
                // the next peek/transition to replay or strand survivors. The
                // or_else physical clamp is a defensive fallback for the
                // (unreachable while non-empty) current_order == None case.
                tx.queue.current_index = tx
                    .queue
                    .current_order
                    .and_then(|co| tx.queue.order.get(co).copied())
                    .or_else(|| {
                        let new_len = tx.queue.song_ids.len();
                        Some(cur.min(new_len - 1))
                    });
            }
            // index > cur: no adjustment needed
        }

        tx.commit_save_all()
    }

    /// Remove every queue row matching a song_id.
    ///
    /// Useful for "drop this song everywhere it appears" semantics. For
    /// per-row removal (right-click on a single duplicate) use
    /// [`Self::remove_entry_by_id`] instead — that path is duplicate-aware.
    pub fn remove_song_by_id(&mut self, id: &str) -> Result<NextTrackResetEffect> {
        while let Some(idx) = self.index_of(id) {
            let _ = self.remove_song(idx)?;
        }
        Ok(NextTrackResetEffect::new())
    }

    /// Remove every queue row matching any of the given song_ids.
    ///
    /// Each ID is resolved freshly between removals so cascading shifts can't
    /// desync the targets. Unknown IDs are skipped silently. As with
    /// [`Self::remove_song_by_id`], duplicate rows of a song all disappear —
    /// callers that need single-row removal should use
    /// [`Self::remove_entries_by_ids`].
    pub fn remove_songs_by_ids(&mut self, ids: &[String]) -> Result<NextTrackResetEffect> {
        for id in ids {
            while let Some(idx) = self.index_of(id) {
                let _ = self.remove_song(idx)?;
            }
        }
        Ok(NextTrackResetEffect::new())
    }

    /// Remove a single queue row by its per-row `entry_id`.
    ///
    /// Drift-immune *and* duplicate-aware: two queue rows that share a
    /// `song_id` get distinct `entry_id`s, so right-click "Remove from
    /// queue" can target one row without taking the other with it.
    /// No-op if `entry_id` doesn't match any current row.
    pub fn remove_entry_by_id(&mut self, entry_id: u64) -> Result<NextTrackResetEffect> {
        if let Some(idx) = self.index_of_entry(entry_id) {
            let _ = self.remove_song(idx)?;
        }
        Ok(NextTrackResetEffect::new())
    }

    /// Remove a batch of queue rows by their `entry_id`s.
    ///
    /// Each ID is resolved freshly between removals — order of `entry_ids`
    /// is irrelevant. Unknown IDs are skipped silently.
    pub fn remove_entries_by_ids(&mut self, entry_ids: &[u64]) -> Result<NextTrackResetEffect> {
        for &eid in entry_ids {
            if let Some(idx) = self.index_of_entry(eid) {
                let _ = self.remove_song(idx)?;
            }
        }
        Ok(NextTrackResetEffect::new())
    }

    pub fn toggle_shuffle(&mut self) -> Result<NextTrackResetEffect> {
        let mut tx = self.write();
        tx.queue.shuffle = !tx.queue.shuffle;
        debug!(
            " [SHUFFLE] Shuffle mode: {}",
            if tx.queue.shuffle { "ON" } else { "OFF" }
        );
        if tx.queue.shuffle {
            tx.shuffle_order();
        } else {
            tx.unshuffle_order();
        }
        tx.commit_save_order()
    }

    /// Shuffle the queue order randomly.
    /// Preserves the currently playing song at its current index.
    pub fn shuffle_queue(&mut self) -> Result<NextTrackResetEffect> {
        if self.queue.song_ids.is_empty() {
            return Ok(NextTrackResetEffect::new());
        }

        let mut tx = self.write();
        let current_song_id = tx
            .queue
            .current_index
            .and_then(|idx| tx.queue.song_ids.get(idx))
            .cloned();

        // Shuffle song_ids together with entry_ids so per-row identity
        // follows the row through the shuffle. (Field-disjoint borrows
        // don't see through the guard's Deref/DerefMut, so reborrow once
        // and operate via `qm`.)
        let qm: &mut QueueManager = &mut tx;
        let mut rng = rand::rng();
        let song_ids = std::mem::take(&mut qm.queue.song_ids);
        let entry_ids = std::mem::take(&mut qm.entry_ids);
        let mut pairs: Vec<(String, u64)> = song_ids.into_iter().zip(entry_ids).collect();
        pairs.shuffle(&mut rng);
        for (sid, eid) in pairs {
            qm.queue.song_ids.push(sid);
            qm.entry_ids.push(eid);
        }

        // Update current_index to point to the same song after shuffle
        if let Some(song_id) = current_song_id {
            tx.queue.current_index = tx.index_of(&song_id);
        }

        // Rebuild order after physical reorder
        tx.rebuild_order_and_sync();
        if tx.queue.shuffle {
            tx.shuffle_order();
        }
        debug!(" [QUEUE] Queue shuffled, new order preserved");
        tx.commit_save_order()
    }

    /// Sort the queue by the given sort mode and direction.
    /// Physically reorders `queue.song_ids` so next/previous follows sorted order.
    /// Preserves the currently-playing song's position via `current_index` update.
    /// `Random` delegates to `shuffle_queue` and ignores `ascending`.
    pub fn sort_queue(
        &mut self,
        mode: QueueSortMode,
        ascending: bool,
    ) -> Result<NextTrackResetEffect> {
        if self.queue.song_ids.is_empty() {
            return Ok(NextTrackResetEffect::new());
        }

        if matches!(mode, QueueSortMode::Random) {
            return self.shuffle_queue();
        }

        let mut tx = self.write();
        // The sort_by closure needs a disjoint borrow of `pool` and
        // `queue.song_ids`. Field-disjoint borrows work through a real
        // `&mut QueueManager`, but not through the guard's Deref/DerefMut
        // (which hide field structure). Reborrow once and operate via `qm`.
        let qm: &mut QueueManager = &mut tx;
        let current_song_id = qm
            .queue
            .current_index
            .and_then(|idx| qm.queue.song_ids.get(idx))
            .cloned();

        // Sort song_ids + entry_ids as a pair so per-row identity follows
        // the row through the sort. (`sort_by` on song_ids alone would
        // leave the parallel entry_ids in stale positions.) Take the
        // backing storage out via `mem::take` so the closure can borrow
        // `pool` immutably without overlapping these mutable accesses.
        let song_ids_buf = std::mem::take(&mut qm.queue.song_ids);
        let entry_ids_buf = std::mem::take(&mut qm.entry_ids);
        let pool = &qm.pool;
        let mut pairs: Vec<(String, u64)> = song_ids_buf.into_iter().zip(entry_ids_buf).collect();
        pairs.sort_by(|(a_id, _), (b_id, _)| {
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
        for (sid, eid) in pairs {
            qm.queue.song_ids.push(sid);
            qm.entry_ids.push(eid);
        }

        // Update current_index to point to the same song after sort
        if let Some(song_id) = current_song_id {
            qm.queue.current_index = qm.index_of(&song_id);
        }

        // Rebuild order after physical reorder
        qm.rebuild_order_and_sync();
        if qm.queue.shuffle {
            qm.shuffle_order();
        }
        debug!(
            " [QUEUE] Queue sorted by {:?} ({})",
            mode,
            if ascending { "ASC" } else { "DESC" }
        );
        tx.commit_save_order()
    }

    pub fn set_repeat(&mut self, mode: RepeatMode) -> Result<NextTrackResetEffect> {
        let mut tx = self.write();
        tx.queue.repeat = mode;
        tx.commit_save_order()
    }

    pub fn toggle_consume(&mut self) -> Result<NextTrackResetEffect> {
        let mut tx = self.write();
        tx.queue.consume = !tx.queue.consume;
        tx.commit_save_order()
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

    /// Save both queue ordering and song pool in ONE atomic redb
    /// transaction. Used for mutations that change both (add, remove,
    /// set_queue, reorder + remove).
    ///
    /// Persisting both blobs in a single `save_binary_batch` commit closes
    /// the torn-write window the prior two-transaction version exposed: a
    /// crash/kill between the two commits could otherwise leave a new ORDER
    /// blob paired with a stale (or missing) SONG-POOL, silently dropping
    /// queue rows on the next load.
    pub fn save_all(&self) -> Result<()> {
        // Hold the encoded buffers in locals so the &[u8] slices passed to
        // `save_binary_batch` outlive the call. Mirror `save_binary`'s
        // bincode config + error mapping for on-disk compatibility.
        let order_bytes =
            bincode_next::encode_to_vec(&self.queue, bincode_next::config::standard())
                .map_err(|e| anyhow::anyhow!("bincode encode (queue order): {e}"))?;
        let pool_bytes = bincode_next::encode_to_vec(&self.pool, bincode_next::config::standard())
            .map_err(|e| anyhow::anyhow!("bincode encode (song pool): {e}"))?;
        self.storage.save_binary_batch(&[
            (KEY_QUEUE_ORDER, order_bytes.as_slice()),
            (KEY_QUEUE_SONGS, pool_bytes.as_slice()),
        ])?;
        Ok(())
    }

    // ── Queue Accessors ──

    pub fn get_queue(&self) -> &Queue {
        &self.queue
    }

    /// Directly reposition the playhead to `index` without triggering a
    /// gapless transition. Use for play-from-here, stop, and shuffle resets.
    ///
    /// For gapless transitions, use `peek_next_song()` →
    /// `transition_to_queued_internal()` instead.
    pub fn reposition_to_index(&mut self, index: Option<usize>) -> NextTrackResetEffect {
        let mut tx = self.write();
        tx.queue.current_index = index;
        tx.sync_current_order_to_index();
        tx.commit_no_save()
    }

    // ══════════════════════════════════════════════════════════════════════
    //  Queue Item Operations
    // ══════════════════════════════════════════════════════════════════════

    /// Move a song from one position to another in the queue.
    /// Used for drag-and-drop reordering.
    /// Updates `current_index` so the currently-playing song isn't lost.
    pub fn move_item(&mut self, from: usize, to: usize) -> Result<NextTrackResetEffect> {
        let len = self.queue.song_ids.len();
        if from >= len || to > len || from == to {
            return Ok(NextTrackResetEffect::new());
        }

        let mut tx = self.write();
        // Under shuffle, snapshot the play-order as stable entry_ids BEFORE
        // the physical move so the upcoming random order can be reproduced
        // afterward — a move must not re-randomize next-up.
        let play_order_eids = if tx.queue.shuffle {
            Some(tx.capture_play_order_entry_ids())
        } else {
            None
        };

        let item = tx.queue.song_ids.remove(from);
        let insert_at = if from < to { to - 1 } else { to };
        tx.queue.song_ids.insert(insert_at, item);
        // Keep entry_ids parallel with song_ids so per-row identity follows
        // the row through reorders.
        let entry = tx.entry_ids.remove(from);
        tx.entry_ids.insert(insert_at, entry);

        // Adjust current_index to keep tracking the same song
        if let Some(cur) = tx.queue.current_index {
            tx.queue.current_index = Some(if cur == from {
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

        // Rebuild order after move (indices changed). Under shuffle, splice
        // the moved row inside the existing order instead of reshuffling the
        // whole tail so the user's manual move sticks and next-up stays
        // deterministic.
        match play_order_eids {
            Some(eids) => tx.rebuild_order_from_play_sequence(&eids),
            None => tx.rebuild_order_and_sync(),
        }
        debug!(
            "📦 [QUEUE] Moved item from {} to {} (inserted at {})",
            from, to, insert_at
        );
        tx.commit_save_order()
    }

    /// Multi-row reorder addressed by per-row `entry_id`s. Drift-immune
    /// across the UI's optimistic-mutation window: the `entry_id` → current
    /// queue position resolution happens under the write guard, not at the
    /// dispatch site, so a stale UI snapshot cannot send a wrong raw index.
    ///
    /// The moved rows keep their `entry_id`s (mirroring [`Self::move_item`]'s
    /// single-row preservation), so a follow-up action that addresses any
    /// of them by `entry_id` still resolves correctly before the projection
    /// catches up.
    ///
    /// Unknown `entry_id`s are silently skipped. Duplicate `entry_id`s in
    /// the input slice de-duplicate to a single move. If the target is itself
    /// in the move set, the moved block lands where that target sat
    /// pre-removal.
    pub fn move_batch_by_entry_ids(
        &mut self,
        entry_ids: &[u64],
        target: MoveBatchTarget,
    ) -> Result<NextTrackResetEffect> {
        // Resolve entry_ids → (index, song_id, entry_id) triples,
        // dropping unknown ids silently. Sort + dedup by index so a
        // single entry_id passed twice still moves one row.
        let mut to_move: Vec<(usize, String, u64)> = entry_ids
            .iter()
            .filter_map(|&eid| {
                let idx = self.index_of_entry(eid)?;
                let song_id = self.queue.song_ids.get(idx)?.clone();
                Some((idx, song_id, eid))
            })
            .collect();
        if to_move.is_empty() {
            return Ok(NextTrackResetEffect::new());
        }
        to_move.sort_unstable_by_key(|&(i, _, _)| i);
        to_move.dedup_by_key(|&mut (i, _, _)| i);

        // Resolve target → raw position BEFORE any removal. `End` and an
        // unknown `AboveEntry` both fall through to "append".
        let target_idx = match target {
            MoveBatchTarget::AboveEntry(eid) => self
                .index_of_entry(eid)
                .unwrap_or(self.queue.song_ids.len()),
            MoveBatchTarget::End => self.queue.song_ids.len(),
        };

        // Capture the playing row's `entry_id` so current_index can be
        // restored by identity (not by position arithmetic) after the
        // reorder. Handles the duplicate-row case `move_item`'s
        // position-arithmetic cannot.
        let current_entry_id = self
            .queue
            .current_index
            .and_then(|i| self.entry_ids.get(i).copied());

        let mut tx = self.write();

        // Under shuffle, snapshot the play-order as stable entry_ids BEFORE
        // the physical reorder so the upcoming random order can be reproduced
        // afterward (mirrors `move_item`).
        let play_order_eids = if tx.queue.shuffle {
            Some(tx.capture_play_order_entry_ids())
        } else {
            None
        };

        // Remove rows in descending order so surviving indices stay valid.
        let mut descending: Vec<usize> = to_move.iter().map(|&(i, _, _)| i).collect();
        descending.sort_unstable_by(|a, b| b.cmp(a));
        for &i in &descending {
            tx.queue.song_ids.remove(i);
            tx.entry_ids.remove(i);
        }

        // Post-removal insert position: shift the original target back by
        // the count of removed rows that sat before it, then clamp.
        let removed_before_target = descending.iter().filter(|&&i| i < target_idx).count();
        let insert_at = target_idx
            .saturating_sub(removed_before_target)
            .min(tx.queue.song_ids.len());

        // Insert in original ascending order so the moved block preserves
        // the user's selection ordering.
        for (offset, (_, song_id, entry_id)) in to_move.iter().enumerate() {
            let pos = insert_at + offset;
            tx.queue.song_ids.insert(pos, song_id.clone());
            tx.entry_ids.insert(pos, *entry_id);
        }

        // Restore current_index by entry_id identity (duplicate-aware).
        tx.queue.current_index =
            current_entry_id.and_then(|eid| tx.entry_ids.iter().position(|&id| id == eid));

        // Order array depends on the physical positions. Under shuffle,
        // splice the moved rows inside the existing order (preserving the
        // random tail) instead of reshuffling; otherwise rebuild identity.
        match play_order_eids {
            Some(eids) => tx.rebuild_order_from_play_sequence(&eids),
            None => tx.rebuild_order_and_sync(),
        }

        debug!(
            "📦 [QUEUE] Moved batch of {} rows to position {} (target {:?})",
            to_move.len(),
            insert_at,
            target,
        );
        tx.commit_save_order()
    }

    /// Insert songs right after the currently playing position ("Play Next").
    /// If nothing is playing, appends to the end.
    /// Does NOT change `current_index` — the currently playing song stays the same.
    pub fn insert_after_current(&mut self, mut songs: Vec<Song>) -> Result<NextTrackResetEffect> {
        self.assign_original_positions(&mut songs);
        let count = songs.len();
        let fresh_entry_ids = self.allocate_entry_ids(count);

        let mut tx = self.write();
        let insert_pos = tx
            .queue
            .current_index
            .map_or(tx.queue.song_ids.len(), |idx| idx + 1);

        let clamped = insert_pos.min(tx.queue.song_ids.len());

        // Insert IDs + entry_ids in reverse so they end up in original
        // forward order at `clamped`.
        for (song, eid) in songs.into_iter().zip(fresh_entry_ids).rev() {
            tx.queue.song_ids.insert(clamped, song.id.clone());
            tx.entry_ids.insert(clamped, eid);
            tx.pool.insert(song);
        }

        // Update order array for the insertion
        tx.insert_into_order(clamped, count);

        // Adjust current_index for songs inserted before it
        if let Some(cur) = tx.queue.current_index
            && clamped <= cur
        {
            tx.queue.current_index = Some(cur + count);
        }

        debug!("📦 [QUEUE] Inserted songs after current (pos {})", clamped);
        tx.commit_save_all()
    }

    /// Insert a song at `index` and set it as the currently-playing song.
    /// Used to re-insert songs from history (consume mode).
    pub fn insert_song_and_make_current(
        &mut self,
        index: usize,
        song: Song,
    ) -> Result<NextTrackResetEffect> {
        let clamped = index.min(self.queue.song_ids.len());
        let _ = self.insert_songs_at(clamped, vec![song])?;
        let _ = self.reposition_to_index(Some(clamped));
        self.save_order()?;
        Ok(NextTrackResetEffect::new())
    }

    /// Insert multiple songs at a specific index in the queue.
    /// Used for cross-pane drag-and-drop (browsing panel → queue at drop position).
    /// Does NOT change `current_index` to point at the inserted songs, but adjusts
    /// it forward if the insertion point is before the currently-playing song.
    /// See `insert_song_and_make_current` for the singular variant that sets the playhead.
    pub fn insert_songs_at(
        &mut self,
        index: usize,
        mut songs: Vec<Song>,
    ) -> Result<NextTrackResetEffect> {
        if songs.is_empty() {
            return Ok(NextTrackResetEffect::new());
        }
        self.assign_original_positions(&mut songs);
        let count = songs.len();
        let fresh_entry_ids = self.allocate_entry_ids(count);

        let mut tx = self.write();
        let clamped = index.min(tx.queue.song_ids.len());

        // Insert in reverse so they end up in order at `clamped`. entry_ids
        // ride along to keep the parallel arrays aligned.
        for (song, eid) in songs.into_iter().zip(fresh_entry_ids).rev() {
            tx.queue.song_ids.insert(clamped, song.id.clone());
            tx.entry_ids.insert(clamped, eid);
            tx.pool.insert(song);
        }

        // Update order array for the insertion
        tx.insert_into_order(clamped, count);

        // Adjust current_index: if inserting before the playing song, shift it forward
        if let Some(cur) = tx.queue.current_index
            && clamped <= cur
        {
            tx.queue.current_index = Some(cur + count);
        }

        debug!(
            "📦 [QUEUE] Inserted {} songs at position {}",
            count, clamped
        );
        tx.commit_save_all()
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
    ) -> (QueueManager, tempfile::TempDir) {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let db_path = temp.path().join("queue.redb");
        let storage = StateStorage::new(db_path).expect("temp storage");
        let mut qm = QueueManager::new(storage).expect("queue manager");
        let ids: Vec<String> = songs.iter().map(|s| s.id.clone()).collect();
        qm.pool.insert_many(songs);
        qm.replace_song_ids_for_test(ids, current_index);
        (qm, temp)
    }

    #[test]
    fn move_item_forward() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, None);

        let _ = qm.move_item(0, 2).unwrap();
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
        let (mut qm, _temp) = make_test_manager(songs, None);

        let _ = qm.move_item(2, 0).unwrap();
        let ids: Vec<&str> = qm.queue.song_ids.iter().map(|s| s.as_str()).collect();
        assert_eq!(ids, vec!["c", "a", "b"]);
    }

    #[test]
    fn move_item_same_position_is_noop() {
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let (mut qm, _temp) = make_test_manager(songs, None);

        let _ = qm.move_item(1, 1).unwrap();
        let ids: Vec<&str> = qm.queue.song_ids.iter().map(|s| s.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"]);
    }

    #[test]
    fn move_item_out_of_bounds_is_noop() {
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let (mut qm, _temp) = make_test_manager(songs, None);

        let _ = qm.move_item(5, 0).unwrap();
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
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

        let _ = qm.move_item(0, 2).unwrap();
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
        let (mut qm, _temp) = make_test_manager(songs, Some(2));

        let _ = qm.move_item(2, 0).unwrap();
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
        let (mut qm, _temp) = make_test_manager(songs, Some(1));

        let _ = qm.move_item(0, 2).unwrap();
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
        let (mut qm, _temp) = make_test_manager(songs, Some(1));

        let _ = qm.move_item(2, 0).unwrap();
        assert_eq!(qm.queue.current_index, Some(2));
        assert_eq!(qm.queue.song_ids[2], "b");
    }

    #[test]
    fn move_item_to_end_of_two_item_queue() {
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let (mut qm, _temp) = make_test_manager(songs, None);

        // from=0, to=2 (== len) means "place after the last item"
        let _ = qm.move_item(0, 2).unwrap();
        let ids: Vec<&str> = qm.queue.song_ids.iter().map(|s| s.as_str()).collect();
        assert_eq!(ids, vec!["b", "a"]);
    }

    // ── move_batch_by_entry_ids tests ──

    fn songs_n(n: usize) -> Vec<Song> {
        (0..n).map(|i| make_test_song(&format!("s{i}"))).collect()
    }

    #[test]
    fn move_batch_by_entry_ids_above_target_collects_block() {
        let (mut qm, _t) = make_test_manager(songs_n(5), None);
        let eids = qm.entry_ids().to_vec();

        // Move s0, s2, s4 to above s1 → block lands at position 0
        // (s1's index 1 minus 1 row removed before it = 0).
        let _: NextTrackResetEffect = qm
            .move_batch_by_entry_ids(
                &[eids[0], eids[2], eids[4]],
                MoveBatchTarget::AboveEntry(eids[1]),
            )
            .unwrap();

        let ids: Vec<&str> = qm.queue.song_ids.iter().map(|s| s.as_str()).collect();
        assert_eq!(ids, vec!["s0", "s2", "s4", "s1", "s3"]);
        assert_eq!(
            qm.entry_ids(),
            &[eids[0], eids[2], eids[4], eids[1], eids[3]],
            "entry_ids must ride with their songs through a batch move",
        );
    }

    #[test]
    fn move_batch_by_entry_ids_to_end_appends_block() {
        let (mut qm, _t) = make_test_manager(songs_n(4), None);
        let eids = qm.entry_ids().to_vec();

        let _ = qm
            .move_batch_by_entry_ids(&[eids[0], eids[2]], MoveBatchTarget::End)
            .unwrap();

        let ids: Vec<&str> = qm.queue.song_ids.iter().map(|s| s.as_str()).collect();
        assert_eq!(ids, vec!["s1", "s3", "s0", "s2"]);
        assert_eq!(qm.entry_ids(), &[eids[1], eids[3], eids[0], eids[2]]);
    }

    #[test]
    fn move_batch_by_entry_ids_unknown_ids_silently_skipped() {
        let (mut qm, _t) = make_test_manager(songs_n(3), None);
        let eids = qm.entry_ids().to_vec();

        // 9999 is a fresh u64 that hasn't been handed out.
        let _ = qm
            .move_batch_by_entry_ids(&[eids[0], 9999, eids[2]], MoveBatchTarget::End)
            .unwrap();

        let ids: Vec<&str> = qm.queue.song_ids.iter().map(|s| s.as_str()).collect();
        assert_eq!(ids, vec!["s1", "s0", "s2"]);
    }

    #[test]
    fn move_batch_by_entry_ids_empty_is_noop() {
        let (mut qm, _t) = make_test_manager(songs_n(3), None);
        let before_ids = qm.queue.song_ids.clone();
        let before_eids = qm.entry_ids().to_vec();

        let _ = qm
            .move_batch_by_entry_ids(&[], MoveBatchTarget::End)
            .unwrap();

        assert_eq!(qm.queue.song_ids, before_ids);
        assert_eq!(qm.entry_ids(), before_eids.as_slice());
    }

    #[test]
    fn move_batch_by_entry_ids_dedups_repeated_input() {
        let (mut qm, _t) = make_test_manager(songs_n(3), None);
        let eids = qm.entry_ids().to_vec();

        // Same entry_id passed twice → resolves to one move.
        let _ = qm
            .move_batch_by_entry_ids(&[eids[0], eids[0]], MoveBatchTarget::End)
            .unwrap();

        let ids: Vec<&str> = qm.queue.song_ids.iter().map(|s| s.as_str()).collect();
        assert_eq!(ids, vec!["s1", "s2", "s0"]);
    }

    #[test]
    fn move_batch_by_entry_ids_preserves_current_song_through_shift() {
        // s1 playing; move s0 to end → s1 shifts to index 0 but stays current.
        let (mut qm, _t) = make_test_manager(songs_n(4), Some(1));
        let eids = qm.entry_ids().to_vec();

        let _ = qm
            .move_batch_by_entry_ids(&[eids[0]], MoveBatchTarget::End)
            .unwrap();

        assert_eq!(qm.queue.song_ids, vec!["s1", "s2", "s3", "s0"]);
        assert_eq!(qm.queue.current_index, Some(0));
        assert_eq!(qm.entry_id_at(0), Some(eids[1]));
    }

    #[test]
    fn move_batch_by_entry_ids_preserves_current_when_current_is_moved() {
        // s2 playing; move s1, s2 to end → s2 still current at new position.
        let (mut qm, _t) = make_test_manager(songs_n(4), Some(2));
        let eids = qm.entry_ids().to_vec();

        let _ = qm
            .move_batch_by_entry_ids(&[eids[1], eids[2]], MoveBatchTarget::End)
            .unwrap();

        assert_eq!(qm.queue.song_ids, vec!["s0", "s3", "s1", "s2"]);
        assert_eq!(qm.queue.current_index, Some(3));
        assert_eq!(qm.entry_id_at(3), Some(eids[2]));
    }

    #[test]
    fn move_batch_by_entry_ids_disambiguates_duplicates() {
        // Two rows share song_id "a" but have distinct entry_ids.
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("a"),
        ];
        let (mut qm, _t) = make_test_manager(songs, None);
        let eids = qm.entry_ids().to_vec();

        // Move only the FIRST "a" to end; the second "a" stays put.
        let _ = qm
            .move_batch_by_entry_ids(&[eids[0]], MoveBatchTarget::End)
            .unwrap();

        assert_eq!(qm.queue.song_ids, vec!["b", "a", "a"]);
        assert_eq!(
            qm.entry_ids(),
            &[eids[1], eids[2], eids[0]],
            "the SPECIFIC duplicate moved, identified by entry_id",
        );
    }

    #[test]
    fn move_batch_by_entry_ids_drift_immune_against_external_insert() {
        // Models the drift window: UI captured eids before some other
        // mutation shifted positions. The batch move resolves entry_ids
        // freshly under its own lock and lands rows correctly.
        let (mut qm, _t) = make_test_manager(songs_n(5), None);
        let eids = qm.entry_ids().to_vec();
        let target_eid = eids[2];

        // External insert shifts s2 from index 2 to index 3.
        let _ = qm.insert_songs_at(0, vec![make_test_song("X")]).unwrap();
        assert_eq!(qm.queue.song_ids[3], "s2");

        // The pre-shift entry_id still resolves to s2.
        let _ = qm
            .move_batch_by_entry_ids(&[target_eid], MoveBatchTarget::End)
            .unwrap();

        assert_eq!(qm.queue.song_ids.last(), Some(&"s2".to_string()));
    }

    #[test]
    fn move_batch_by_entry_ids_target_in_move_set_lands_contiguous() {
        // Move s1, s2, s3 above s2. s2 is in the move set; the block
        // lands where s2 originally sat (index 2), minus the removed
        // count before it (1 → s1), so insert_at = 1. Result is the
        // original order (effectively a no-op for a contiguous run).
        let (mut qm, _t) = make_test_manager(songs_n(5), None);
        let eids = qm.entry_ids().to_vec();

        let _ = qm
            .move_batch_by_entry_ids(
                &[eids[1], eids[2], eids[3]],
                MoveBatchTarget::AboveEntry(eids[2]),
            )
            .unwrap();

        let ids: Vec<&str> = qm.queue.song_ids.iter().map(|s| s.as_str()).collect();
        assert_eq!(ids, vec!["s0", "s1", "s2", "s3", "s4"]);
    }

    // ── N7: shuffle-aware move preserves upcoming play order ──

    #[test]
    fn move_item_under_shuffle_preserves_upcoming_order() {
        let songs: Vec<Song> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|s| make_test_song(s))
            .collect();
        let (mut qm, _t) = make_test_manager(songs, Some(0));
        let _ = qm.toggle_shuffle().unwrap();

        // The play-order sequence of stable row identities before the move.
        let before_play_eids = qm.capture_play_order_entry_ids();
        let before_order = qm.queue.order.clone();

        // Move a non-current physical row.
        let _ = qm.move_item(3, 1).unwrap();

        // The play-order is the SAME multiset of row indices (a permutation).
        let mut sorted_before = before_order.clone();
        let mut sorted_after = qm.queue.order.clone();
        sorted_before.sort();
        sorted_after.sort();
        assert_eq!(sorted_before, sorted_after, "order must stay a permutation");

        // The entire play-order sequence of rows is preserved (tail NOT
        // re-randomized) — each row, moved or not, keeps its play position.
        let after_play_eids = qm.capture_play_order_entry_ids();
        assert_eq!(
            after_play_eids, before_play_eids,
            "a move under shuffle must not re-randomize the upcoming order",
        );

        // next-up is deterministic across a repeated identical move sequence.
        let next1 = qm
            .queue
            .order
            .get(qm.queue.current_order.unwrap() + 1)
            .copied();
        let songs2: Vec<Song> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|s| make_test_song(s))
            .collect();
        let (mut qm2, _t2) = make_test_manager(songs2, Some(0));
        qm2.queue.shuffle = true;
        // Force qm2 to the same initial shuffled order as qm BEFORE its move,
        // then apply the identical move.
        qm2.queue.order = before_order.clone();
        qm2.sync_current_order_to_index();
        let _ = qm2.move_item(3, 1).unwrap();
        let next2 = qm2
            .queue
            .order
            .get(qm2.queue.current_order.unwrap() + 1)
            .copied();
        assert_eq!(
            next1, next2,
            "identical move on identical order is deterministic"
        );
    }

    #[test]
    fn move_batch_under_shuffle_preserves_upcoming_order() {
        let songs: Vec<Song> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|s| make_test_song(s))
            .collect();
        let (mut qm, _t) = make_test_manager(songs, Some(0));
        let _ = qm.toggle_shuffle().unwrap();
        let eids = qm.entry_ids().to_vec();

        let before_play_eids = qm.capture_play_order_entry_ids();
        let before_order = qm.queue.order.clone();

        // Move two non-current rows to the end.
        let _ = qm
            .move_batch_by_entry_ids(&[eids[3], eids[4]], MoveBatchTarget::End)
            .unwrap();

        // Still a valid permutation.
        let mut sorted_before = before_order.clone();
        let mut sorted_after = qm.queue.order.clone();
        sorted_before.sort();
        sorted_after.sort();
        assert_eq!(sorted_before, sorted_after);

        // Play-order of rows fully preserved (no tail re-randomization).
        assert_eq!(qm.capture_play_order_entry_ids(), before_play_eids);
    }

    /// The current song must stay anchored at play-order position 0 through a
    /// shuffle-aware move (shuffle invariant: current at order[0]).
    #[test]
    fn move_item_under_shuffle_keeps_current_anchored() {
        let songs: Vec<Song> = (0..6).map(|i| make_test_song(&i.to_string())).collect();
        let (mut qm, _t) = make_test_manager(songs, Some(2));
        let _ = qm.toggle_shuffle().unwrap();
        // After toggle_shuffle, current is anchored at order[0].
        assert_eq!(qm.queue.current_order, Some(0));

        let _ = qm.move_item(4, 1).unwrap();

        assert_eq!(
            qm.queue.current_order,
            Some(0),
            "current must remain anchored at the head of the play order",
        );
        // current_index still resolves the same song.
        let ci = qm.queue.current_index.unwrap();
        assert_eq!(qm.queue.order[qm.queue.current_order.unwrap()], ci);
    }

    // remove_song current_index tracking tests

    #[test]
    fn remove_song_before_current_decrements_index() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(2)); // playing "c"

        let _ = qm.remove_song(0).unwrap(); // remove "a"
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
        let (mut qm, _temp) = make_test_manager(songs, Some(0)); // playing "a"

        let _ = qm.remove_song(2).unwrap(); // remove "c"
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
        let (mut qm, _temp) = make_test_manager(songs, Some(2)); // playing "c" (last)

        let _ = qm.remove_song(2).unwrap(); // remove "c"
        assert_eq!(qm.queue.current_index, Some(1)); // clamped to last valid
    }

    #[test]
    fn remove_song_until_empty_clears_index() {
        let songs = vec![make_test_song("a")];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

        let _ = qm.remove_song(0).unwrap();
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
        let (mut qm, _temp) = make_test_manager(songs, Some(4)); // playing "e" at index 4

        let _ = qm.remove_song(0).unwrap(); // remove "a" → current becomes 3
        let _ = qm.remove_song(0).unwrap(); // remove "b" → current becomes 2
        let _ = qm.remove_song(0).unwrap(); // remove "c" → current becomes 1

        assert_eq!(qm.queue.current_index, Some(1));
        assert_eq!(qm.queue.song_ids[1], "e");
    }

    // SongPool integration tests

    #[test]
    fn pool_get_returns_song_data() {
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let (qm, _temp) = make_test_manager(songs, None);

        assert_eq!(qm.get_song("a").unwrap().title, "Song a");
        assert_eq!(qm.get_song("b").unwrap().title, "Song b");
        assert!(qm.get_song("nonexistent").is_none());
    }

    // ── reconcile_loaded_queue (I30) ──

    fn pool_with(ids: &[&str]) -> SongPool {
        let mut pool = SongPool::default();
        pool.insert_many(ids.iter().map(|id| make_test_song(id)).collect());
        pool
    }

    fn queue_with(song_ids: &[&str], current_index: Option<usize>) -> Queue {
        let mut q = Queue::default();
        q.song_ids = song_ids.iter().map(|s| s.to_string()).collect();
        q.order = (0..song_ids.len()).collect();
        q.current_index = current_index;
        q.current_order = current_index;
        q
    }

    #[test]
    fn reconcile_prunes_missing_id_and_remaps_index() {
        // song_ids [A,B,C,D], current=Some(2) (=C), pool drops B.
        let mut q = queue_with(&["A", "B", "C", "D"], Some(2));
        let pool = pool_with(&["A", "C", "D"]);

        let dirty = reconcile_loaded_queue(&mut q, &pool);
        assert!(dirty);
        assert_eq!(q.song_ids, vec!["A", "C", "D"]);
        // C followed by remap: was index 2, now index 1 (NOT clamped/None).
        assert_eq!(q.current_index, Some(1));
        // order is a valid permutation of 0..3 with no entry >= 3.
        assert!(order_is_identity_permutation(&q.order, 3));
        assert!(q.order.iter().all(|&i| i < 3));
    }

    #[test]
    fn reconcile_clamps_out_of_range_index() {
        let mut q = queue_with(&["A", "B"], Some(7));
        let pool = pool_with(&["A", "B"]);

        let dirty = reconcile_loaded_queue(&mut q, &pool);
        // Index 7 is out of range for the rebuilt order; current_index must
        // land within [0, 2).
        assert!(dirty);
        if let Some(i) = q.current_index {
            assert!(i < 2, "current_index {i} not clamped");
        }
    }

    #[test]
    fn reconcile_empty_pool_normalizes() {
        let mut q = queue_with(&["A", "B", "C"], Some(1));
        let pool = SongPool::default();

        let dirty = reconcile_loaded_queue(&mut q, &pool);
        assert!(dirty);
        assert!(q.song_ids.is_empty());
        assert_eq!(q.current_index, None);
        assert!(q.order.is_empty());
        assert_eq!(q.current_order, None);
    }

    #[test]
    fn reconcile_clears_transient_queued() {
        let mut q = queue_with(&["A", "B", "C"], Some(0));
        q.queued = Some(1);
        let pool = pool_with(&["A", "B", "C"]);

        let dirty = reconcile_loaded_queue(&mut q, &pool);
        assert_eq!(q.queued, None);
        assert!(dirty, "clearing a set queued marks the queue dirty");
    }

    #[test]
    fn reconcile_clean_queue_is_not_dirty() {
        // Consistent order + pool with nothing to clear → no spurious save.
        let mut q = queue_with(&["A", "B", "C"], Some(1));
        let pool = pool_with(&["A", "B", "C"]);

        let dirty = reconcile_loaded_queue(&mut q, &pool);
        assert!(!dirty);
        assert_eq!(q.song_ids, vec!["A", "B", "C"]);
        assert_eq!(q.current_index, Some(1));
    }

    #[test]
    fn new_recovers_from_corrupt_order_blob() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let db_path = temp.path().join("queue.redb");
        let storage = StateStorage::new(db_path).expect("temp storage");

        // Write a payload under the ORDER key whose bincode layout cannot
        // decode as a Queue (a tuple of unrelated shape).
        storage
            .save_binary(KEY_QUEUE_ORDER, &("garbage", 123u64, vec![1u8, 2, 3]))
            .expect("write garbage");

        // new() must recover (Ok) rather than propagate Err.
        let qm = QueueManager::new(storage).expect("new must degrade to empty, not abort");
        assert!(qm.get_queue().song_ids.is_empty());
        assert_eq!(qm.get_queue().current_index, None);
    }

    #[test]
    fn new_restores_valid_order_blob() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let db_path = temp.path().join("queue.redb");
        let storage = StateStorage::new(db_path).expect("temp storage");

        // Round-trip a real, populated queue.
        {
            let mut qm = QueueManager::new(storage.clone()).expect("queue manager");
            qm.pool
                .insert_many(vec![make_test_song("a"), make_test_song("b")]);
            qm.replace_song_ids_for_test(vec!["a".to_string(), "b".to_string()], Some(0));
            qm.save_all().unwrap();
        }

        let qm2 = QueueManager::new(storage).expect("reload");
        assert_eq!(qm2.get_queue().song_ids, vec!["a", "b"]);
        assert_eq!(qm2.get_queue().current_index, Some(0));
    }

    #[test]
    fn save_all_persists_order_and_pool_atomically() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let (qm, _temp) = make_test_manager(songs, Some(0));

        // Persist order + pool through the atomic batch path.
        qm.save_all().unwrap();

        // Reconstruct a fresh QueueManager on the SAME StateStorage (clone
        // shares the underlying redb Arc) and confirm a consistent snapshot.
        let storage = qm.storage.clone();
        let qm2 = QueueManager::new(storage).unwrap();
        assert!(
            qm2.get_current_song().is_some(),
            "current song must resolve from the atomically-persisted pool",
        );
        assert_eq!(qm2.songs_in_order().len(), 3);
    }

    #[test]
    fn save_order_does_not_include_song_data() {
        let songs = vec![make_test_song("x"), make_test_song("y")];
        let (qm, _temp) = make_test_manager(songs, Some(0));

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
        let (qm, _temp) = make_test_manager(songs, Some(0));

        assert_eq!(qm.queue.order, vec![0, 1, 2]);
        assert_eq!(qm.queue.current_order, Some(0));
    }

    #[test]
    fn order_array_shuffled_preserves_current() {
        let songs: Vec<Song> = (0..10).map(|i| make_test_song(&i.to_string())).collect();
        let (mut qm, _temp) = make_test_manager(songs, Some(3));

        // Toggle shuffle on
        let _ = qm.toggle_shuffle().unwrap();

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
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

        // Peek should return song at index 1 (next in order)
        let peeked = qm.peek_next_song().unwrap();
        assert_eq!(peeked.index(), 1);
        assert_eq!(peeked.song().id, "b");
    }

    #[test]
    fn peek_next_shuffle_returns_order_array_entry() {
        let songs: Vec<Song> = (0..10).map(|i| make_test_song(&i.to_string())).collect();
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

        let _ = qm.toggle_shuffle().unwrap();

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
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

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
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

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
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

        // Set queued directly (the guard's drop semantics would otherwise
        // clear it before we can observe the mutation's effect).
        qm.queue.queued = Some(1);
        assert!(qm.queue.queued.is_some());

        // Add a song — should clear queued
        let _ = qm.add_songs(vec![make_test_song("d")]).unwrap();
        assert!(qm.queue.queued.is_none());
    }

    #[test]
    fn set_repeat_clears_queued() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

        // Set queued directly (the guard's drop semantics would otherwise
        // clear it before we can observe the mutation's effect).
        qm.queue.queued = Some(1);
        assert!(qm.queue.queued.is_some());

        let _ = qm.set_repeat(RepeatMode::Track).unwrap();
        assert!(
            qm.queue.queued.is_none(),
            "set_repeat must clear queued (IG-5)"
        );
    }

    #[test]
    fn toggle_consume_clears_queued() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

        // Set queued directly (the guard's drop semantics would otherwise
        // clear it before we can observe the mutation's effect).
        qm.queue.queued = Some(1);
        assert!(qm.queue.queued.is_some());

        let _ = qm.toggle_consume().unwrap();
        assert!(
            qm.queue.queued.is_none(),
            "toggle_consume must clear queued (IG-5)"
        );
    }

    #[test]
    fn remove_song_adjusts_order_indices() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
            make_test_song("d"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

        // Order is [0, 1, 2, 3]. Remove song at index 1 ("b")
        let _ = qm.remove_song(1).unwrap();

        // Order should now be [0, 1, 2] (indices adjusted)
        assert_eq!(qm.queue.order, vec![0, 1, 2]);
        assert_eq!(qm.queue.song_ids, vec!["a", "c", "d"]);
    }

    // ── QUEUE-1: removing the playing row under shuffle must keep the
    //    invariant order[current_order] == current_index AND reach every
    //    still-upcoming survivor exactly once before stopping (no replay,
    //    no strand). These tests drain via get_next_song() and assert the
    //    EXACT play sequence so the bare sync_current_order_to_index()
    //    one-liner (which strands / over-plays) cannot pass.

    /// Drain the queue from the current song: push the current song id, then
    /// repeatedly call get_next_song() collecting each id until None (capped to
    /// avoid spinning under repeat modes). Returns the full play sequence.
    fn drain_play_sequence(qm: &mut QueueManager, cap: usize) -> Vec<String> {
        let mut seq = Vec::new();
        if let Some(idx) = qm.queue.current_index
            && let Some(id) = qm.queue.song_ids.get(idx)
        {
            seq.push(id.clone());
        }
        for _ in 0..cap {
            match qm.get_next_song() {
                Some(next) => seq.push(next.song.id.clone()),
                None => break,
            }
        }
        seq
    }

    fn assert_no_repeats(seq: &[String]) {
        let mut sorted = seq.to_vec();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            seq.len(),
            "play sequence replays/over-plays a song: {seq:?}"
        );
    }

    /// STRANDING DISCRIMINATOR. order=[0,2,1], playing s0 (current_order=0).
    /// Upcoming survivors after removing s0 are s2 (order pos 1) then s1
    /// (order pos 2). The correct derive fix drains to exactly [s2, s1]. The
    /// rejected bare sync_current_order_to_index() one-liner STRANDS s2 and
    /// drains to only [s1]. This is the test that enforces the spec's
    /// acceptance bar "do NOT ship the bare one-liner alone".
    #[test]
    fn remove_current_under_shuffle_strands_with_bare_oneliner() {
        let songs = vec![
            make_test_song("s0"),
            make_test_song("s1"),
            make_test_song("s2"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));
        qm.queue.shuffle = true;
        qm.queue.consume = false;
        qm.queue.repeat = RepeatMode::None;
        qm.queue.order = vec![0, 2, 1];
        qm.queue.current_index = Some(0); // order[0] == 0 == s0, invariant holds
        qm.queue.current_order = Some(0);

        let _ = qm.remove_song(0).unwrap(); // remove playing s0
        // song_ids after removal: [s1, s2]
        assert_eq!(qm.queue.song_ids, vec!["s1", "s2"]);

        // (a) invariant restored
        let co = qm.queue.current_order.expect("current_order set");
        let ci = qm.queue.current_index.expect("current_index set");
        assert_eq!(qm.queue.order[co], ci, "invariant order[co]==ci broken");

        // (b) no immediate replay of the removed/current song
        let cur_id = qm.queue.song_ids[ci].clone();
        if let Some(peek) = qm.peek_next_song() {
            assert_ne!(peek.song().id, cur_id, "peek replays the current song");
        }

        // (c) exact reachability: both upcoming survivors reached, in order,
        //     with no repeats. Bare one-liner strands s2 → drains to [s1].
        let seq = drain_play_sequence(&mut qm, 8);
        assert_no_repeats(&seq);
        assert_eq!(
            seq,
            vec!["s2".to_string(), "s1".to_string()],
            "drain must reach both upcoming survivors s2 then s1"
        );
    }

    /// MID-ORDER placement. order=[2,1,0,3], playing s1 (current_order=1).
    /// Upcoming survivors are order[2]=s0 then order[3]=s3; s2 sits at the
    /// already-played order pos 0 and must NOT be revisited. Correct fix
    /// drains to exactly [s0, s3]. Bare one-liner yields [s2, s0, s3]
    /// (over-plays already-played s2) which the exact-sequence assert rejects.
    #[test]
    fn remove_current_under_shuffle_mid_order_reaches_all_survivors() {
        let songs = vec![
            make_test_song("s0"),
            make_test_song("s1"),
            make_test_song("s2"),
            make_test_song("s3"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(1));
        qm.queue.shuffle = true;
        qm.queue.consume = false;
        qm.queue.repeat = RepeatMode::None;
        qm.queue.order = vec![2, 1, 0, 3];
        qm.queue.current_index = Some(1); // order[1] == 1 == s1, invariant holds
        qm.queue.current_order = Some(1);

        let _ = qm.remove_song(1).unwrap(); // remove playing s1
        // song_ids after removal: [s0, s2, s3]
        assert_eq!(qm.queue.song_ids, vec!["s0", "s2", "s3"]);

        // (a) invariant restored
        let co = qm.queue.current_order.expect("current_order set");
        let ci = qm.queue.current_index.expect("current_index set");
        assert_eq!(qm.queue.order[co], ci, "invariant order[co]==ci broken");

        // (b) no immediate replay
        let cur_id = qm.queue.song_ids[ci].clone();
        if let Some(peek) = qm.peek_next_song() {
            assert_ne!(peek.song().id, cur_id, "peek replays the current song");
        }

        // (c) exact play sequence [s0, s3], no repeats, s2 not over-played.
        let seq = drain_play_sequence(&mut qm, 8);
        assert_no_repeats(&seq);
        assert_eq!(
            seq,
            vec!["s0".to_string(), "s3".to_string()],
            "drain must be exactly [s0, s3]"
        );
    }

    /// LAST-ORDER-SLOT placement. order=[2,0,1], playing s1 (current_order=2,
    /// the last order slot). Nothing sits after the last order position, so the
    /// upcoming set is empty. After removal the play-order survivor that slid
    /// into the slot (s0) is the new current and a clean None stop is correct.
    /// Correct fix drains to exactly [s0] with no replay; s2 (already-played at
    /// order pos 0) is correctly not revisited.
    #[test]
    fn remove_current_under_shuffle_last_order_slot_no_replay_no_strand() {
        let songs = vec![
            make_test_song("s0"),
            make_test_song("s1"),
            make_test_song("s2"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(1));
        qm.queue.shuffle = true;
        qm.queue.consume = false;
        qm.queue.repeat = RepeatMode::None;
        qm.queue.order = vec![2, 0, 1];
        qm.queue.current_index = Some(1); // order[2] == 1 == s1, invariant holds
        qm.queue.current_order = Some(2);

        let _ = qm.remove_song(1).unwrap(); // remove playing s1
        // song_ids after removal: [s0, s2]
        assert_eq!(qm.queue.song_ids, vec!["s0", "s2"]);

        // (a) invariant restored
        let co = qm.queue.current_order.expect("current_order set");
        let ci = qm.queue.current_index.expect("current_index set");
        assert_eq!(qm.queue.order[co], ci, "invariant order[co]==ci broken");

        // (b) no replay of the current song
        let cur_id = qm.queue.song_ids[ci].clone();
        if let Some(peek) = qm.peek_next_song() {
            assert_ne!(peek.song().id, cur_id, "peek replays the current song");
        }

        // (c) exact play sequence [s0], no repeats, clean stop, no strand.
        let seq = drain_play_sequence(&mut qm, 8);
        assert_no_repeats(&seq);
        assert_eq!(seq, vec!["s0".to_string()], "drain must be exactly [s0]");
    }

    #[test]
    fn add_songs_extends_order() {
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

        assert_eq!(qm.queue.order, vec![0, 1]);

        let _ = qm
            .add_songs(vec![make_test_song("c"), make_test_song("d")])
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
        let (mut qm, _temp) = make_test_manager(songs, Some(3));

        // Shuffle
        let _ = qm.toggle_shuffle().unwrap();
        assert!(qm.queue.shuffle);

        // Unshuffle
        let _ = qm.toggle_shuffle().unwrap();
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

        let (mut qm, _temp) = make_test_manager(songs, Some(0)); // playing "c" = "Charlie"
        let _ = qm.sort_queue(QueueSortMode::Title, true).unwrap();

        // After title sort ascending: Alpha, Bravo, Charlie
        // "c" (Charlie) should now be at index 2
        assert_eq!(qm.queue.current_index, Some(2));
        assert_eq!(qm.queue.song_ids[2], "c");
    }

    #[test]
    fn sort_queue_empty_is_noop() {
        use crate::types::queue_sort_mode::QueueSortMode;

        let (mut qm, _temp) = make_test_manager(vec![], None);
        let _ = qm.sort_queue(QueueSortMode::Title, true).unwrap();
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
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

        // ascending=true mirrors Rating's pre-flip convention: highest first.
        let _ = qm.sort_queue(QueueSortMode::MostPlayed, true).unwrap();
        assert_eq!(qm.queue.song_ids, vec!["b", "c", "a"]);
    }

    #[test]
    fn sort_queue_by_most_played_treats_none_as_zero() {
        use crate::types::queue_sort_mode::QueueSortMode;

        let mut songs = vec![make_test_song("a"), make_test_song("b")];
        songs[0].play_count = None;
        songs[1].play_count = Some(3);
        let (mut qm, _temp) = make_test_manager(songs, None);

        let _ = qm.sort_queue(QueueSortMode::MostPlayed, true).unwrap();
        assert_eq!(qm.queue.song_ids, vec!["b", "a"]);
    }

    #[test]
    fn shuffle_queue_preserves_current_song_identity() {
        let songs: Vec<Song> = (0..20).map(|i| make_test_song(&i.to_string())).collect();
        let (mut qm, _temp) = make_test_manager(songs, Some(7)); // playing "7"

        let _ = qm.shuffle_queue().unwrap();

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
        let (mut qm, _temp) = make_test_manager(songs, Some(1)); // playing "b" at 1

        let new_songs = vec![make_test_song("x"), make_test_song("y")];
        let _ = qm.insert_after_current(new_songs).unwrap();

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
        let (mut qm, _temp) = make_test_manager(songs, None);

        let new_songs = vec![make_test_song("x")];
        let _ = qm.insert_after_current(new_songs).unwrap();

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
        let (mut qm, _temp) = make_test_manager(songs, Some(3)); // playing "d" at 3

        let new_songs = vec![make_test_song("x"), make_test_song("y")];
        let _ = qm.insert_songs_at(1, new_songs).unwrap();

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
        let (mut qm, _temp) = make_test_manager(songs, Some(1)); // playing "b" at 1

        let new_songs = vec![make_test_song("x")];
        let _ = qm.insert_songs_at(3, new_songs).unwrap(); // insert after end

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
        let (mut qm, _temp) = make_test_manager(songs, Some(2)); // playing "c" at 2

        let new_songs = vec![
            make_test_song("x"),
            make_test_song("y"),
            make_test_song("z"),
        ];
        let _ = qm.add_songs(new_songs).unwrap();

        assert_eq!(qm.queue.current_index, Some(2)); // unchanged
        assert_eq!(qm.queue.song_ids[2], "c");
        assert_eq!(qm.queue.song_ids.len(), 6);
    }

    #[test]
    fn increment_song_play_count_bumps_existing_value() {
        let mut song = make_test_song("a");
        song.play_count = Some(3);
        let (mut qm, _temp) = make_test_manager(vec![song], Some(0));

        qm.increment_song_play_count("a").unwrap();
        assert_eq!(qm.pool.get("a").unwrap().play_count, Some(4));
    }

    #[test]
    fn increment_song_play_count_starts_from_none() {
        let mut song = make_test_song("a");
        song.play_count = None;
        let (mut qm, _temp) = make_test_manager(vec![song], Some(0));

        qm.increment_song_play_count("a").unwrap();
        assert_eq!(qm.pool.get("a").unwrap().play_count, Some(1));
    }

    #[test]
    fn increment_song_play_count_unknown_id_is_noop() {
        let mut song = make_test_song("a");
        song.play_count = Some(2);
        let (mut qm, _temp) = make_test_manager(vec![song], Some(0));

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
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

        let _ = qm.remove_song_by_id("c").unwrap();

        assert_eq!(qm.queue.song_ids, vec!["a", "b", "d"]);
        assert!(qm.pool.get("c").is_none());
    }

    #[test]
    fn remove_song_by_id_unknown_id_is_noop() {
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

        let _ = qm.remove_song_by_id("nonexistent").unwrap();

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
        let (mut qm, _temp) = make_test_manager(songs, Some(2)); // playing "c"

        // Remove "a" (before current) — current should shift back
        let _ = qm.remove_song_by_id("a").unwrap();
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
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

        let _ = qm
            .remove_songs_by_ids(&["b".to_string(), "d".to_string()])
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
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

        let _ = qm
            .remove_songs_by_ids(&["b".to_string(), "nonexistent".to_string(), "c".to_string()])
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
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

        // IDs deliberately given in ascending-index order — the buggy version
        // (snapshot indices, remove ascending) would mistakenly remove "b" and "d".
        let _ = qm
            .remove_songs_by_ids(&["a".to_string(), "c".to_string()])
            .unwrap();

        assert_eq!(qm.queue.song_ids, vec!["b", "d"]);
    }

    #[test]
    fn remove_songs_by_ids_empty_is_noop() {
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

        let _ = qm.remove_songs_by_ids(&[]).unwrap();

        assert_eq!(qm.queue.song_ids, vec!["a", "b"]);
        assert_eq!(qm.queue.current_index, Some(0));
    }

    // ══════════════════════════════════════════════════════════════════════
    //  Per-Row entry_id Removal (duplicate-aware)
    // ══════════════════════════════════════════════════════════════════════

    /// Regression: two queue rows of the same song_id must each be removable
    /// without taking the other with them. The legacy `remove_songs_by_ids`
    /// path tore both rows out because the queue identifier was the
    /// `song_id`, which collided across duplicate adds.
    #[test]
    fn remove_entry_by_id_removes_only_targeted_duplicate() {
        let song = make_test_song("dup");
        let (mut qm, _temp) = make_test_manager(vec![song.clone(), song.clone()], Some(0));

        assert_eq!(qm.queue.song_ids, vec!["dup", "dup"]);
        let entry_ids = qm.entry_ids().to_vec();
        assert_eq!(entry_ids.len(), 2, "two rows should have two entry_ids");
        assert_ne!(
            entry_ids[0], entry_ids[1],
            "duplicate rows must get distinct entry_ids",
        );

        let target = entry_ids[1];
        let _ = qm.remove_entry_by_id(target).unwrap();

        assert_eq!(qm.queue.song_ids, vec!["dup"], "second row should remain");
        assert_eq!(qm.entry_ids(), &[entry_ids[0]]);
        // The pool entry survives because another row still references it.
        assert!(
            qm.get_song("dup").is_some(),
            "pool entry must survive while at least one duplicate row remains",
        );
    }

    #[test]
    fn remove_entry_by_id_unknown_is_noop() {
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

        let _ = qm.remove_entry_by_id(99_999).unwrap();

        assert_eq!(qm.queue.song_ids, vec!["a", "b"]);
        assert_eq!(qm.entry_ids().len(), 2);
    }

    #[test]
    fn remove_entries_by_ids_removes_each_targeted_row() {
        let song = make_test_song("dup");
        let unique = make_test_song("uniq");
        let (mut qm, _temp) = make_test_manager(vec![song.clone(), unique, song.clone()], Some(0));
        let entry_ids = qm.entry_ids().to_vec();

        // Remove the two duplicate rows, leave the unique row.
        let _ = qm
            .remove_entries_by_ids(&[entry_ids[0], entry_ids[2]])
            .unwrap();

        assert_eq!(qm.queue.song_ids, vec!["uniq"]);
        // Pool drops "dup" only because no row references it anymore.
        assert!(qm.get_song("dup").is_none());
        assert!(qm.get_song("uniq").is_some());
    }

    /// `remove_song_by_id` on a duplicate must clear *every* row of that
    /// song_id (the "drop everywhere" semantics that batch flows still want).
    /// Distinct from the per-row `remove_entry_by_id` path above.
    #[test]
    fn remove_song_by_id_drops_all_duplicates() {
        let song = make_test_song("dup");
        let unique = make_test_song("uniq");
        let (mut qm, _temp) = make_test_manager(vec![song.clone(), unique, song.clone()], Some(0));

        let _ = qm.remove_song_by_id("dup").unwrap();

        assert_eq!(qm.queue.song_ids, vec!["uniq"]);
        assert!(qm.get_song("dup").is_none());
    }

    /// End-to-end mimic of `QueueNavigator::record_and_consume` on a
    /// duplicate-row queue: peek → transition (advances current_index) →
    /// remove the just-finished row (consume). The surviving duplicate must
    /// stay in the queue AND in the pool, with its `entry_id` preserved.
    ///
    /// This is the path the user reports as buggy ("seeking through one
    /// duplicate drops both"). If this test passes, the QueueManager layer
    /// is innocent and the bug lives in the UI projection.
    #[test]
    fn consume_on_duplicate_keeps_survivor() {
        let song = make_test_song("A");
        let other = make_test_song("B");
        let (mut qm, _temp) = make_test_manager(vec![song.clone(), song.clone(), other], Some(0));
        qm.queue.consume = true;
        let original_entry_ids = qm.entry_ids().to_vec();
        assert_eq!(original_entry_ids.len(), 3);

        // Mimic `on_track_finished`'s decide_transition: peek + transition
        // bumps current_index from 0 → 1.
        let peeked = qm.peek_next_song().expect("peek next song");
        let transition = peeked.transition();
        assert_eq!(transition.old_index, Some(0));
        assert_eq!(transition.new_index, 1);
        assert_eq!(qm.queue.current_index, Some(1));

        // Then `record_and_consume` runs `remove_song(prev_index)` where
        // prev_index is the captured `transition.old_index`.
        let _ = qm.remove_song(0).expect("consume previous index");

        assert_eq!(
            qm.queue.song_ids,
            vec!["A", "B"],
            "first duplicate consumed; the survivor and B remain",
        );
        assert_eq!(
            qm.entry_ids(),
            &[original_entry_ids[1], original_entry_ids[2]],
            "entry_ids ride with their rows through consume",
        );
        assert!(
            qm.get_song("A").is_some(),
            "pool must keep A while the duplicate survives — losing it here would drop both rows from the UI projection",
        );
        assert!(qm.get_song("B").is_some());
        assert_eq!(
            qm.queue.current_index,
            Some(0),
            "current_index shifts back from 1 → 0 after the index-0 removal",
        );
    }

    /// Two consecutive consume cycles on adjacent duplicates: simulates the
    /// user playing through both copies of "A" with consume on. Each cycle
    /// must independently consume one row. The pool entry for "A" sticks
    /// around until the *second* cycle drops the last copy.
    #[test]
    fn consume_through_both_duplicates_drops_pool_only_on_last() {
        let song = make_test_song("A");
        let other = make_test_song("B");
        let (mut qm, _temp) = make_test_manager(vec![song.clone(), song.clone(), other], Some(0));
        qm.queue.consume = true;

        // ── First cycle: A1 finishes, consume removes idx 0 ──
        let peeked = qm.peek_next_song().expect("peek 1");
        let transition = peeked.transition();
        assert_eq!(transition.old_index, Some(0));
        assert_eq!(transition.new_index, 1);
        let _ = qm.remove_song(0).expect("consume cycle 1");
        assert_eq!(qm.queue.song_ids, vec!["A", "B"]);
        assert_eq!(qm.queue.current_index, Some(0));
        assert!(
            qm.get_song("A").is_some(),
            "pool keeps A after first cycle — survivor row still references it",
        );

        // ── Second cycle: A2 (the survivor) finishes, consume removes idx 0 ──
        let peeked = qm.peek_next_song().expect("peek 2");
        let transition = peeked.transition();
        assert_eq!(transition.old_index, Some(0));
        assert_eq!(transition.new_index, 1);
        let _ = qm.remove_song(0).expect("consume cycle 2");
        assert_eq!(qm.queue.song_ids, vec!["B"]);
        assert_eq!(qm.queue.current_index, Some(0));
        assert!(
            qm.get_song("A").is_none(),
            "pool finally drops A — no row references it anymore",
        );
        assert!(qm.get_song("B").is_some());
    }

    #[test]
    fn entry_ids_survive_move_and_sort() {
        use crate::types::queue_sort_mode::QueueSortMode;

        let mut songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        songs[0].title = "Charlie".into();
        songs[1].title = "Alpha".into();
        songs[2].title = "Bravo".into();
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

        let original = qm.entry_ids().to_vec();
        let _ = qm.sort_queue(QueueSortMode::Title, true).unwrap();

        // After ascending title sort: Alpha (b), Bravo (c), Charlie (a).
        assert_eq!(qm.queue.song_ids, vec!["b", "c", "a"]);
        // entry_ids ride with their songs through the sort.
        assert_eq!(
            qm.entry_ids(),
            &[original[1], original[2], original[0]],
            "entry_ids must follow their song through sort",
        );
    }

    /// Sibling of [`entry_ids_survive_move_and_sort`] specifically for
    /// `move_item` — the original test only covered `sort_queue`, leaving
    /// the single-row reorder's parallelism implicit on the
    /// `entry_ids.remove`/`entry_ids.insert` pair in
    /// [`QueueManager::move_item`].
    /// Pins the drift-immunity contract that play_next/play_previous's
    /// consume removal must honor: anchoring by entry_id survives a
    /// concurrent shift, while a raw index captured pre-shift removes the
    /// wrong row.
    #[test]
    fn consume_by_entry_id_is_drift_immune() {
        // Manager 1 — NEW path: capture B's entry_id (current at idx 1),
        // then a concurrent shift removes an earlier row, then remove by id.
        let songs = vec![
            make_test_song("A"),
            make_test_song("B"),
            make_test_song("C"),
            make_test_song("D"),
        ];
        let (mut qm, _t) = make_test_manager(songs, Some(1)); // current = B
        let b_eid = qm.entry_id_at(1).expect("entry_id for B");

        // Concurrent shift: an earlier row is removed → [B, C, D].
        let _ = qm.remove_song(0).unwrap();
        assert_eq!(qm.queue.song_ids, vec!["B", "C", "D"]);

        // NEW path removes B (correct) regardless of the index drift.
        let _ = qm.remove_entry_by_id(b_eid).unwrap();
        assert_eq!(qm.queue.song_ids, vec!["C", "D"], "B removed by identity");

        // Manager 2 — OLD raw-index path demonstrates the bug: the stale
        // index 1 now removes C, not B.
        let songs = vec![
            make_test_song("A"),
            make_test_song("B"),
            make_test_song("C"),
            make_test_song("D"),
        ];
        let (mut qm2, _t2) = make_test_manager(songs, Some(1));
        let _ = qm2.remove_song(0).unwrap(); // [B, C, D]
        let _ = qm2.remove_song(1).unwrap(); // raw stale index → removes C
        assert_eq!(
            qm2.queue.song_ids,
            vec!["B", "D"],
            "raw index removed the WRONG row (C) — the bug the fix avoids",
        );
    }

    #[test]
    fn entry_ids_survive_move_item() {
        let (mut qm, _temp) = make_test_manager(songs_n(3), None);
        let eids = qm.entry_ids().to_vec();

        // Move s0 to position 3 (end). entry_ids must travel with the row.
        let _ = qm.move_item(0, 3).unwrap();

        assert_eq!(qm.queue.song_ids, vec!["s1", "s2", "s0"]);
        assert_eq!(
            qm.entry_ids(),
            &[eids[1], eids[2], eids[0]],
            "move_item must keep entry_ids parallel to song_ids through the reorder",
        );
    }

    // ══════════════════════════════════════════════════════════════════════
    //  `NextTrackResetEffect` contract
    // ══════════════════════════════════════════════════════════════════════

    /// Regression net for the queue/engine desync bug: when the queue
    /// is reordered while shuffle + crossfade are active, the audio
    /// engine's pre-buffered next-track decoder is stale. The fix is
    /// to make every queue mutation return a
    /// [`NextTrackResetEffect`] obligation. Each binding below is a
    /// compile-time pin — a future contributor who regresses one of
    /// these methods to `Result<()>` (or `()`) breaks this test
    /// instead of silently re-introducing the desync.
    #[test]
    fn reorder_mutators_return_next_track_reset_effect() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

        let _: NextTrackResetEffect = qm.move_item(0, 2).unwrap();
        let _: NextTrackResetEffect = qm.add_songs(vec![make_test_song("d")]).unwrap();
        let _: NextTrackResetEffect = qm.insert_songs_at(0, vec![make_test_song("e")]).unwrap();
        let _: NextTrackResetEffect = qm.insert_after_current(vec![make_test_song("f")]).unwrap();
        let _: NextTrackResetEffect = qm.remove_song(0).unwrap();
        let _: NextTrackResetEffect = qm
            .insert_song_and_make_current(0, make_test_song("g"))
            .unwrap();
        let _: NextTrackResetEffect = qm.remove_song_by_id("a").unwrap();
        let _: NextTrackResetEffect = qm.remove_songs_by_ids(&["b".into()]).unwrap();
        let _: NextTrackResetEffect = qm.remove_entry_by_id(0).unwrap();
        let _: NextTrackResetEffect = qm.remove_entries_by_ids(&[]).unwrap();
        let _: NextTrackResetEffect = qm.sort_queue(QueueSortMode::Title, true).unwrap();
        let _: NextTrackResetEffect = qm.shuffle_queue().unwrap();
        let _: NextTrackResetEffect = qm.toggle_shuffle().unwrap();
        let _: NextTrackResetEffect = qm.toggle_consume().unwrap();
        let _: NextTrackResetEffect = qm.set_repeat(RepeatMode::Track).unwrap();
        let _: NextTrackResetEffect = qm.reposition_to_index(Some(0));
        let _: NextTrackResetEffect = qm.set_queue(vec![make_test_song("h")], Some(0)).unwrap();
        let _: NextTrackResetEffect = qm
            .move_batch_by_entry_ids(&[], MoveBatchTarget::End)
            .unwrap();
    }
}
