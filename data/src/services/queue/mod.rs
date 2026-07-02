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
pub use navigation::{
    NextSongResult, PeekedQueue, PreviousOutcome, PreviousSongResult, TransitionReason,
    TransitionResult,
};
use rand::seq::SliceRandom;
use tracing::{debug, warn};

use crate::{
    services::state_storage::StateStorage,
    types::{
        NextTrackResetEffect,
        queue::{MoveBatchTarget, PlayOrder, Queue, QueueRow, RepeatMode},
        queue_sort_mode::QueueSortMode,
        song::Song,
        song_pool::SongPool,
    },
};

/// One playback-history record. Keyed by the per-row `entry_id` (when known)
/// rather than `Song.id` so Previous lands on the exact physical row that
/// played — even when two adjacent rows share a song id. `entry_id` is `None`
/// only when the row context was unavailable at push time (consumed/removed
/// row, or a defensive missing-index path), in which case history falls back
/// to id-based dedup and first-match lookup.
///
/// Runtime-only — `playback_history` is never serialized, so this type has no
/// on-disk-compat surface.
#[derive(Debug, Clone)]
pub(crate) struct HistoryEntry {
    pub(crate) entry_id: Option<u64>,
    pub(crate) song: Song,
}

pub struct QueueManager {
    pub(crate) queue: Queue,
    pub(crate) pool: SongPool,
    pub(crate) storage: StateStorage,
    pub(crate) playback_history: Vec<HistoryEntry>,
    pub(crate) max_history_size: usize,
    /// Monotonic counter that hands out the next per-row `entry_id`
    /// (`QueueRow.entry_id`). Never reused within a process lifetime;
    /// entry ids reseed `0..len` on every `QueueManager::new()`.
    pub(crate) next_entry_id: u64,
}

impl std::fmt::Debug for QueueManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueueManager")
            .field("queue_len", &self.queue.rows.len())
            .field("pool_len", &self.pool.len())
            .field("current_index", &self.queue.current_index())
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
/// 1. Prune rows whose `song_id` is absent from `pool`, building an
///    old→new index remap.
/// 2. Rewrite `order` through the remap, dropping missing entries. If the
///    result is not a valid permutation of `0..new_len`, fall back to the
///    canonical identity order (matches `rebuild_order`) and drop
///    `current_order`.
/// 3. Remap the derived physical playhead through the old→new map; if its
///    row was pruned, clamp to the adjacent survivor (or `None` when the
///    queue is now empty).
/// 4. Re-anchor `current_order` onto the surviving physical row.
/// 5. ALWAYS clear `queued` — gapless-prep transient, never valid across a
///    relaunch.
///
/// Pure and non-panicking; unit-testable without redb.
fn reconcile_loaded_queue(queue: &mut Queue, pool: &SongPool) -> bool {
    let old_len = queue.rows.len();

    // (1) Prune rows whose id is missing, building an old→new index remap.
    let mut remap: Vec<Option<usize>> = Vec::with_capacity(old_len);
    let mut pruned_rows: Vec<QueueRow> = Vec::with_capacity(old_len);
    for row in &queue.rows {
        if pool.get(&row.song_id).is_some() {
            remap.push(Some(pruned_rows.len()));
            pruned_rows.push(row.clone());
        } else {
            remap.push(None);
        }
    }
    let new_len = pruned_rows.len();
    let pruned_any = new_len != old_len;

    // Always clear the transient gapless-prep field on restore.
    let had_queued = queue.queued.is_some();
    queue.queued = None;

    // Physical playhead reconstructed from the DECODED cursor state. For
    // any valid save this equals the legacy stored index (I3 held at save
    // time); for a corrupt order/cursor it degrades to None and the repair
    // below re-anchors deterministically.
    let old_phys = queue
        .current_order
        .and_then(|co| queue.order.get(co).copied());
    let decoded_cursor = queue.current_order;

    if !pruned_any {
        // No rows dropped: validate/repair the order array, clamp a stale
        // physical playhead, and re-anchor the cursor onto it.
        let order_was_valid = order_is_identity_permutation(&queue.order, old_len);
        if !order_was_valid {
            queue.order = PlayOrder::identity(old_len);
        }
        let clamped_phys = if old_len == 0 {
            None
        } else {
            old_phys.map(|p| p.min(old_len - 1))
        };
        queue.current_order =
            clamped_phys.and_then(|idx| queue.order.iter().position(|&o| o == idx));
        return had_queued || !order_was_valid || queue.current_order != decoded_cursor;
    }

    queue.rows = pruned_rows;

    // (2) Rewrite order through the remap, dropping pruned entries.
    let remapped_order: Vec<usize> = queue
        .order
        .iter()
        .filter_map(|&old_idx| remap.get(old_idx).copied().flatten())
        .collect();
    queue.order = if order_is_identity_permutation(&remapped_order, new_len) {
        // Just validated as a full permutation of 0..new_len.
        PlayOrder::from_raw_unvalidated(remapped_order)
    } else {
        // Canonical identity fallback (deterministic, matches rebuild_order).
        PlayOrder::identity(new_len)
    };

    // (3) Remap or clamp the physical playhead.
    let new_phys = match old_phys {
        Some(old) => match remap.get(old).copied().flatten() {
            Some(new) => Some(new),
            None if new_len == 0 => None,
            // Its row was pruned — clamp to the adjacent survivor
            // (stay-in-place), matching remove_from_order. NOTE: `old` is in
            // OLD row space, so when rows BEFORE the playhead are pruned in
            // the same reconcile this can overshoot the precise successor;
            // it is still never worse than the queue tail and never replays.
            None => Some(old.min(new_len - 1)),
        },
        None => None,
    };

    // (4) Re-anchor the cursor onto the surviving physical row; the
    // physical index is DERIVED from it everywhere else.
    queue.current_order = new_phys.and_then(|idx| queue.order.iter().position(|&o| o == idx));

    true
}

/// `true` when `order` is exactly a permutation of `0..len` (no out-of-range
/// entry, no duplicate, correct length). Delegates to the single validator
/// on `PlayOrder` so the load-repair path and the newtype cannot drift.
fn order_is_identity_permutation(order: &[usize], len: usize) -> bool {
    PlayOrder::is_full_permutation(order, len)
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

        // Reseed runtime entry_ids over the (now pruned) rows, replacing
        // the Decode placeholders. The counter starts past the seeded range
        // so subsequent inserts cannot collide. MUST run AFTER reconcile so
        // ids stay dense `0..len` over exactly the surviving rows.
        for (i, row) in queue.rows.iter_mut().enumerate() {
            row.entry_id = i as u64;
        }
        let next_entry_id = queue.rows.len() as u64;

        let mgr = Self {
            queue,
            pool,
            storage,
            playback_history: Vec::new(),
            max_history_size: 100,
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
            .rows
            .iter()
            .filter_map(|row| self.pool.get(&row.song_id))
            .collect()
    }

    /// O(n) scan to find the index of a song ID in the queue.
    /// Centralized here so all callers use the same lookup.
    fn index_of(&self, song_id: &str) -> Option<usize> {
        self.queue.rows.iter().position(|r| r.song_id == song_id)
    }

    /// Owned snapshot of every row's `entry_id`, in physical order. A test
    /// convenience since the QueueRow collapse — production projections
    /// iterate [`Self::rows`] directly.
    #[cfg(test)]
    pub(crate) fn entry_ids(&self) -> Vec<u64> {
        self.queue.rows.iter().map(|r| r.entry_id).collect()
    }

    /// Look up the `entry_id` for a queue position. `None` if `index` is
    /// out of bounds.
    pub fn entry_id_at(&self, index: usize) -> Option<u64> {
        self.queue.rows.get(index).map(|r| r.entry_id)
    }

    /// O(n) scan to find the queue position holding a given `entry_id`.
    pub fn index_of_entry(&self, entry_id: u64) -> Option<usize> {
        self.queue.rows.iter().position(|r| r.entry_id == entry_id)
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

    /// Test-only fast-path that replaces the queue rows from bare song ids,
    /// allocating fresh entry ids. Pool insertion is the caller's
    /// responsibility. Keeps the `(Vec<String>, Option<usize>)` signature so
    /// fixtures stay oblivious to the row representation.
    #[cfg(test)]
    pub(crate) fn replace_song_ids_for_test(
        &mut self,
        song_ids: Vec<String>,
        current_index: Option<usize>,
    ) {
        let count = song_ids.len();
        let fresh = self.allocate_entry_ids(count);
        self.queue.rows = song_ids
            .into_iter()
            .zip(fresh)
            .map(|(song_id, entry_id)| QueueRow { song_id, entry_id })
            .collect();
        self.rebuild_order_and_set_cursor(current_index);
    }

    /// Assign `original_position` to a batch of songs, continuing from the
    /// current maximum in the pool. Used by every "append" path so numbering
    /// is consistent regardless of insertion method.
    fn assign_original_positions(&self, songs: &mut [Song]) {
        let next_pos = self
            .queue
            .rows
            .iter()
            .filter_map(|row| self.pool.get(&row.song_id))
            .filter_map(|s| s.original_position)
            .max()
            .map_or(self.queue.rows.len() as u32, |m| m + 1);
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
        let start_idx = tx.queue.rows.len();

        // Add rows to ordering, songs to pool
        for (song, entry_id) in songs.iter().zip(fresh_entry_ids) {
            tx.queue.rows.push(QueueRow {
                song_id: song.id.clone(),
                entry_id,
            });
        }
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
        tx.queue.rows = songs
            .iter()
            .zip(fresh_entry_ids)
            .map(|(s, entry_id)| QueueRow {
                song_id: s.id.clone(),
                entry_id,
            })
            .collect();
        // Clear and rebuild pool
        tx.pool.clear();
        tx.pool.insert_many(songs);
        // Clear history on context switch (new album/playlist) — Spotify behavior
        tx.playback_history.clear();
        // Rebuild order array and anchor the cursor on the requested row
        tx.rebuild_order_and_set_cursor(current_index);
        // If shuffle is on, shuffle the new order
        if tx.queue.shuffle {
            tx.shuffle_order();
        }
        tx.commit_save_all()
    }

    pub fn remove_song(&mut self, index: usize) -> Result<NextTrackResetEffect> {
        if index >= self.queue.rows.len() {
            return Ok(NextTrackResetEffect::new());
        }
        let mut tx = self.write();
        let removed_id = tx.queue.rows.remove(index).song_id;
        // Only drop the pool entry when no other queue row still references
        // this song_id — a duplicate add keeps the pool alive for survivors.
        if !tx.queue.rows.iter().any(|r| r.song_id == removed_id) {
            tx.pool.remove(&removed_id);
        }

        // Remove from order array and adjust indices. The physical playhead
        // needs NO separate bookkeeping anymore: current_index() derives
        // from order[current_order], and remove_from_order already moved the
        // cursor onto the play-order slot the next surviving song slid into
        // (removed-current case) or kept it on the same row (shifted-index
        // cases). The old hand-derivation of current_index — and the
        // shuffle-desync bug class it guarded against — is structurally
        // gone.
        tx.remove_from_order(index);

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
        if self.queue.rows.is_empty() {
            return Ok(NextTrackResetEffect::new());
        }

        let mut tx = self.write();
        let current_song_id = tx
            .queue
            .current_index()
            .and_then(|idx| tx.queue.rows.get(idx))
            .map(|r| r.song_id.clone());

        // Shuffle whole rows — per-row identity (entry_id) follows the row
        // through the shuffle by construction.
        let mut rng = rand::rng();
        tx.queue.rows.shuffle(&mut rng);

        // Re-anchor the cursor on the playing song's new physical position
        // (first match by id, matching the historical behavior) and rebuild
        // the order for the new physical layout.
        let new_row = current_song_id.and_then(|song_id| tx.index_of(&song_id));
        tx.rebuild_order_and_set_cursor(new_row);
        if tx.queue.shuffle {
            tx.shuffle_order();
        }
        debug!(" [QUEUE] Queue shuffled, new order preserved");
        tx.commit_save_order()
    }

    /// Sort the queue by the given sort mode and direction.
    /// Physically reorders `queue.rows` so next/previous follows sorted order.
    /// Preserves the currently-playing song by re-anchoring the play cursor
    /// onto its new physical position.
    /// `Random` delegates to `shuffle_queue` and ignores `ascending`.
    pub fn sort_queue(
        &mut self,
        mode: QueueSortMode,
        ascending: bool,
    ) -> Result<NextTrackResetEffect> {
        if self.queue.rows.is_empty() {
            return Ok(NextTrackResetEffect::new());
        }

        if matches!(mode, QueueSortMode::Random) {
            return self.shuffle_queue();
        }

        let mut tx = self.write();
        // The sort_by closure needs a disjoint borrow of `pool` and
        // `queue.rows`. Field-disjoint borrows work through a real
        // `&mut QueueManager`, but not through the guard's Deref/DerefMut
        // (which hide field structure). Reborrow once and operate via `qm`.
        let qm: &mut QueueManager = &mut tx;
        let current_song_id = qm
            .queue
            .current_index()
            .and_then(|idx| qm.queue.rows.get(idx))
            .map(|r| r.song_id.clone());

        // Sort whole rows — per-row identity (entry_id) follows the row
        // through the sort by construction. Take the backing storage out
        // via `mem::take` so the closure can borrow `pool` immutably
        // without overlapping these mutable accesses.
        let mut rows_buf = std::mem::take(&mut qm.queue.rows);
        let pool = &qm.pool;
        rows_buf.sort_by(|ra, rb| {
            let a = pool.get(&ra.song_id);
            let b = pool.get(&rb.song_id);
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
        qm.queue.rows = rows_buf;

        // Re-anchor the cursor on the playing song's new physical position
        // (first match by id, matching the historical behavior) and rebuild
        // the order for the new physical layout.
        let new_row = current_song_id.and_then(|song_id| qm.index_of(&song_id));
        qm.rebuild_order_and_set_cursor(new_row);
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
            .current_index()
            .and_then(|idx| self.queue.rows.get(idx))
            .and_then(|row| self.pool.get(&row.song_id))
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
    pub(crate) fn save_songs(&self) -> Result<()> {
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

    /// The physical queue rows, in order. The sanctioned external read path
    /// for per-row (song_id, entry_id) projections — keeps callers off the
    /// raw field so the storage layout can change underneath.
    pub fn rows(&self) -> &[QueueRow] {
        &self.queue.rows
    }

    /// The physical row index of the playing song (see
    /// [`Queue::current_index`]).
    pub fn current_index(&self) -> Option<usize> {
        self.queue.current_index()
    }

    /// The song id at physical queue position `index` (`None` when out of
    /// range). External callers use this instead of reaching into the
    /// queue's row storage, so the storage layout can change underneath.
    pub fn song_id_at(&self, index: usize) -> Option<&str> {
        self.queue.rows.get(index).map(|r| r.song_id.as_str())
    }

    /// Number of rows in the queue.
    pub fn queue_len(&self) -> usize {
        self.queue.rows.len()
    }

    /// Whether the queue has no rows.
    pub fn is_queue_empty(&self) -> bool {
        self.queue.rows.is_empty()
    }

    /// Owned snapshot of every row's song id, in physical order.
    pub fn song_ids_snapshot(&self) -> Vec<String> {
        self.queue.rows.iter().map(|r| r.song_id.clone()).collect()
    }

    /// Directly reposition the playhead to `index` without triggering a
    /// gapless transition. Use for play-from-here, stop, and shuffle resets.
    ///
    /// For gapless transitions, use `peek_next_song()` →
    /// `transition_to_queued_internal()` instead.
    pub fn reposition_to_index(&mut self, index: Option<usize>) -> NextTrackResetEffect {
        let mut tx = self.write();
        tx.set_cursor_to_row(index);
        tx.commit_no_save()
    }

    /// "Play from here" that begins a NEW playback session: reposition onto
    /// `index` and, under shuffle, re-anchor the play order so the chosen
    /// track becomes the head of a fresh shuffle (every remaining track
    /// reshuffled behind it).
    ///
    /// A plain [`Self::reposition_to_index`] only moves the playhead inside
    /// the EXISTING order, so clicking a track that happens to sit at the
    /// tail of a spent shuffle plays once and stops (the dead-end the engine
    /// reports as "No next song available"). Re-anchoring guarantees a manual
    /// pick under shuffle can never strand the user at the end of the order.
    ///
    /// With shuffle OFF the order is identity and this is exactly
    /// [`Self::reposition_to_index`] — no reshuffle. Intended for the
    /// play-from-here path when the engine is stopped; mid-session jumps keep
    /// using `reposition_to_index` so the upcoming order isn't re-randomized
    /// out from under an active listen.
    pub fn reanchor_shuffle_to_index(&mut self, index: usize) -> NextTrackResetEffect {
        let mut tx = self.write();
        tx.set_cursor_to_row(Some(index));
        // A valid in-range `index` always resolves to an order slot (order[] is
        // a full permutation of 0..len), so current_order is Some here. If it
        // were None, shuffle_order would fall to its no-anchor branch and leave
        // the clicked track unanchored — re-creating the dead-end. Surface that
        // contract violation in tests rather than shipping a silent regression.
        debug_assert!(
            tx.queue.current_order.is_some() || tx.queue.rows.is_empty(),
            "reanchor_shuffle_to_index: index {index} absent from order[] — \
             current_order desynced; caller must pass an in-range row index"
        );
        if tx.queue.shuffle {
            // Re-anchor: `shuffle_order` moves `current_order` (now the
            // clicked track) to order[0] and Fisher-Yates shuffles the rest,
            // so the chosen track plays first and every other track follows in
            // a fresh random order. Identity orders (shuffle off) are left
            // untouched above — this branch is skipped.
            tx.shuffle_order();
        }
        tx.commit_no_save()
    }

    // ══════════════════════════════════════════════════════════════════════
    //  Queue Item Operations
    // ══════════════════════════════════════════════════════════════════════

    /// Move a song from one position to another in the queue.
    /// Used for drag-and-drop reordering.
    /// Re-anchors the play cursor so the currently-playing song isn't lost.
    pub fn move_item(&mut self, from: usize, to: usize) -> Result<NextTrackResetEffect> {
        let len = self.queue.rows.len();
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

        // Track the playing song's physical position through the move so
        // the cursor can re-anchor onto it after the order rebuild.
        let cur_before = tx.queue.current_index();

        let row = tx.queue.rows.remove(from);
        let insert_at = if from < to { to - 1 } else { to };
        // Per-row identity (entry_id) moves with the row by construction.
        tx.queue.rows.insert(insert_at, row);

        let cur_after = cur_before.map(|cur| {
            if cur == from {
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
            }
        });

        // Rebuild order after move (indices changed). Under shuffle, splice
        // the moved row inside the existing order instead of reshuffling the
        // whole tail so the user's manual move sticks and next-up stays
        // deterministic.
        match play_order_eids {
            Some(eids) => tx.rebuild_order_from_play_sequence(&eids, cur_after),
            None => tx.rebuild_order_and_set_cursor(cur_after),
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
        // Resolve entry_ids → (index, row) pairs, dropping unknown ids
        // silently. Sort + dedup by index so a single entry_id passed
        // twice still moves one row.
        let mut to_move: Vec<(usize, QueueRow)> = entry_ids
            .iter()
            .filter_map(|&eid| {
                let idx = self.index_of_entry(eid)?;
                let row = self.queue.rows.get(idx)?.clone();
                Some((idx, row))
            })
            .collect();
        if to_move.is_empty() {
            return Ok(NextTrackResetEffect::new());
        }
        to_move.sort_unstable_by_key(|&(i, _)| i);
        to_move.dedup_by_key(|&mut (i, _)| i);

        // Resolve target → raw position BEFORE any removal. `End` and an
        // unknown `AboveEntry` both fall through to "append".
        let target_idx = match target {
            MoveBatchTarget::AboveEntry(eid) => {
                self.index_of_entry(eid).unwrap_or(self.queue.rows.len())
            }
            MoveBatchTarget::End => self.queue.rows.len(),
        };

        // Capture the playing row's `entry_id` so current_index can be
        // restored by identity (not by position arithmetic) after the
        // reorder. Handles the duplicate-row case `move_item`'s
        // position-arithmetic cannot.
        let current_entry_id = self
            .queue
            .current_index()
            .and_then(|i| self.queue.rows.get(i).map(|r| r.entry_id));

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
        let mut descending: Vec<usize> = to_move.iter().map(|&(i, _)| i).collect();
        descending.sort_unstable_by(|a, b| b.cmp(a));
        for &i in &descending {
            tx.queue.rows.remove(i);
        }

        // Post-removal insert position: shift the original target back by
        // the count of removed rows that sat before it, then clamp.
        let removed_before_target = descending.iter().filter(|&&i| i < target_idx).count();
        let insert_at = target_idx
            .saturating_sub(removed_before_target)
            .min(tx.queue.rows.len());

        // Insert in original ascending order so the moved block preserves
        // the user's selection ordering.
        for (offset, (_, row)) in to_move.iter().enumerate() {
            tx.queue.rows.insert(insert_at + offset, row.clone());
        }

        // Locate the playing row by entry_id identity (duplicate-aware) so
        // the cursor re-anchors onto it after the order rebuild.
        let cur_after =
            current_entry_id.and_then(|eid| tx.queue.rows.iter().position(|r| r.entry_id == eid));

        // Order array depends on the physical positions. Under shuffle,
        // splice the moved rows inside the existing order (preserving the
        // random tail) instead of reshuffling; otherwise rebuild identity.
        match play_order_eids {
            Some(eids) => tx.rebuild_order_from_play_sequence(&eids, cur_after),
            None => tx.rebuild_order_and_set_cursor(cur_after),
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
            .current_index()
            .map_or(tx.queue.rows.len(), |idx| idx + 1);

        let clamped = insert_pos.min(tx.queue.rows.len());

        // Insert rows in reverse so they end up in original forward order
        // at `clamped`.
        for (song, entry_id) in songs.into_iter().zip(fresh_entry_ids).rev() {
            tx.queue.rows.insert(
                clamped,
                QueueRow {
                    song_id: song.id.clone(),
                    entry_id,
                },
            );
            tx.pool.insert(song);
        }

        // Update order array for the insertion. insert_into_order bumps
        // the row indices inside `order` and the cursor where needed; the
        // derived current_index follows automatically — the old manual
        // "+= count" bookkeeping is structurally gone.
        tx.insert_into_order(clamped, count);

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
        let clamped = index.min(self.queue.rows.len());
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
        let clamped = index.min(tx.queue.rows.len());

        // Insert rows in reverse so they end up in order at `clamped`.
        for (song, entry_id) in songs.into_iter().zip(fresh_entry_ids).rev() {
            tx.queue.rows.insert(
                clamped,
                QueueRow {
                    song_id: song.id.clone(),
                    entry_id,
                },
            );
            tx.pool.insert(song);
        }

        // Update order array for the insertion (cursor adjustments happen
        // inside; the derived current_index follows automatically).
        tx.insert_into_order(clamped, count);

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

    // ── Wire-compat + row-identity characterization (pre-QueueRow lock) ──

    /// Snapshot the per-row `(song_id, entry_id)` pairs in physical order.
    fn row_pairs(qm: &QueueManager) -> Vec<(String, u64)> {
        qm.queue
            .rows
            .iter()
            .map(|r| (r.song_id.clone(), r.entry_id))
            .collect()
    }

    /// Bare song ids of a raw `Queue` (for reconcile tests that operate on
    /// a `Queue` without a manager).
    fn song_ids_of(q: &Queue) -> Vec<&str> {
        q.rows.iter().map(|r| r.song_id.as_str()).collect()
    }

    /// Invariants that must hold after ANY queue mutation: rows and entry
    /// ids stay length-aligned, `order` is a permutation of `0..len`, and
    /// the playhead coupling `order[current_order] == current_index` holds.
    pub(crate) fn assert_queue_invariants(qm: &QueueManager, label: &str) {
        let len = qm.queue.rows.len();
        assert_eq!(qm.queue.order.len(), len, "{label}: order length != len");
        let mut seen = vec![false; len];
        for &idx in qm.queue.order.iter() {
            assert!(idx < len, "{label}: order entry {idx} out of range {len}");
            assert!(!seen[idx], "{label}: order entry {idx} duplicated");
            seen[idx] = true;
        }
        if let Some(co) = qm.queue.current_order {
            assert!(co < len, "{label}: current_order {co} out of range");
            assert_eq!(
                qm.queue.current_index(),
                Some(qm.queue.order[co]),
                "{label}: order[current_order] != current_index"
            );
        }
        if let Some(ci) = qm.queue.current_index() {
            assert!(ci < len, "{label}: current_index {ci} out of range");
        }
    }

    /// M1 lock: the external-reader accessors must stay semantically glued
    /// to the queue's row storage — same ids, same length, same emptiness —
    /// so the M2 storage flip cannot silently change accessor behavior.
    #[test]
    fn accessors_match_get_queue_song_ids() {
        let songs: Vec<Song> = ["a", "b", "c"]
            .iter()
            .map(|id| make_test_song(id))
            .collect();
        let (mut qm, _temp) = make_test_manager(songs, Some(1));

        assert_eq!(qm.queue_len(), 3);
        assert!(!qm.is_queue_empty());
        for i in 0..3 {
            assert_eq!(
                qm.song_id_at(i),
                qm.get_queue().rows.get(i).map(|r| r.song_id.as_str())
            );
        }
        assert_eq!(qm.song_id_at(3), None);
        assert_eq!(qm.song_ids_snapshot(), song_ids_of(qm.get_queue()));

        let _ = qm.remove_song(0).expect("remove");
        assert_eq!(qm.queue_len(), 2);
        assert_eq!(qm.song_id_at(0), Some("b"));

        let _ = qm.remove_song(0).expect("remove");
        let _ = qm.remove_song(0).expect("remove");
        assert!(qm.is_queue_empty());
        assert_eq!(qm.queue_len(), 0);
        assert!(qm.song_ids_snapshot().is_empty());
    }

    /// C2: a `queue_order` blob written by the PRE-REFACTOR encoder (bytes
    /// captured from main @ 03eb6ea7, derived `Encode` on the 8-field
    /// `Queue`) must keep loading through both refactor phases: rows
    /// reconstructed, runtime `entry_id`s reseeded `0..len`, playhead and
    /// flags preserved, transient `queued` cleared.
    #[test]
    fn pre_refactor_queue_blob_still_loads() {
        // song_ids ["a","b","c","d"], current_index Some(2), current_order
        // Some(0), order [2,0,3,1], queued Some(1), shuffle true,
        // repeat Playlist, consume false. NEVER regenerate.
        const LEGACY_QUEUE_ORDER_BLOB: &[u8] = &[
            4, 1, 97, 1, 98, 1, 99, 1, 100, // song_ids: ["a","b","c","d"]
            1, 2, // current_index: Some(2)
            1, 0, // current_order: Some(0)
            4, 2, 0, 3, 1, // order: [2, 0, 3, 1]
            1, 1, // queued: Some(1)
            1, // shuffle: true
            2, // repeat: Playlist
            0, // consume: false
        ];

        let temp = tempfile::TempDir::new().expect("temp dir");
        let storage = StateStorage::new(temp.path().join("queue.redb")).expect("temp storage");

        // Matching pool blob. `Song`'s encode is untouched by this refactor
        // (locked by `song_pool_bincode_golden`), so encoding the pool at
        // runtime is legacy-faithful; only the QUEUE blob layout is at risk.
        let songs: Vec<Song> = ["a", "b", "c", "d"]
            .iter()
            .map(|id| make_test_song(id))
            .collect();
        let pool_blob = bincode_next::encode_to_vec(&songs, bincode_next::config::standard())
            .expect("encode pool blob");

        storage
            .save_binary_batch(&[
                (KEY_QUEUE_ORDER, LEGACY_QUEUE_ORDER_BLOB),
                (KEY_QUEUE_SONGS, pool_blob.as_slice()),
            ])
            .expect("write legacy blobs");

        let qm = QueueManager::new(storage).expect("legacy blob must load");

        let ids = qm.song_ids_snapshot();
        assert_eq!(ids, vec!["a", "b", "c", "d"]);
        assert_eq!(qm.queue.current_index(), Some(2));
        assert_eq!(qm.queue.order, vec![2, 0, 3, 1]);
        assert_eq!(qm.queue.current_order, Some(0));
        assert!(qm.queue.shuffle);
        assert_eq!(qm.queue.repeat, RepeatMode::Playlist);
        assert!(!qm.queue.consume);
        // Gapless-prep transient — always cleared on load.
        assert_eq!(qm.queue.queued, None);
        // Runtime entry ids reseed fresh per launch, counter past them.
        assert_eq!(qm.entry_ids(), vec![0, 1, 2, 3]);
        assert_eq!(qm.next_entry_id, 4);
        assert_queue_invariants(&qm, "legacy blob load");
    }

    /// M6 contract: the physical playhead is DERIVED — after any mutation,
    /// `current_index()` equals `order[current_order]` by definition.
    /// Tautological against the derived accessor (that is the point: there
    /// is no stored field left to disagree, and a reintroduced stored
    /// writer would fail to compile); kept as the executable statement of
    /// the I3 invariant.
    #[test]
    fn current_index_is_derived_from_order_cursor() {
        let songs: Vec<Song> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|id| make_test_song(id))
            .collect();
        let (mut qm, _temp) = make_test_manager(songs, Some(1));

        let assert_derived = |qm: &QueueManager, label: &str| {
            assert_eq!(
                qm.queue.current_index(),
                qm.queue
                    .current_order
                    .and_then(|co| qm.queue.order.get(co).copied()),
                "{label}: derivation contract broken"
            );
        };

        assert_derived(&qm, "seeded");
        // Concrete-value pins (independent of the accessor body): the
        // seeded identity order puts the cursor slot == physical index.
        assert_eq!(qm.current_index(), Some(1));
        let _ = qm.toggle_shuffle().expect("shuffle");
        assert_derived(&qm, "shuffle");
        // Anchored shuffle keeps the SAME physical row playing at order[0].
        assert_eq!(qm.current_index(), Some(1));
        assert_eq!(qm.queue.current_order, Some(0));
        let _ = qm.get_next_song();
        assert_derived(&qm, "next");
        let _ = qm.move_item(0, 4).expect("move");
        assert_derived(&qm, "move");
        let _ = qm
            .remove_song(qm.current_index().expect("cur"))
            .expect("remove current");
        assert_derived(&qm, "remove current");
        let _ = qm.reposition_to_index(Some(0));
        assert_derived(&qm, "reposition");
        // Concrete pin: repositioning onto row 0 derives exactly Some(0).
        assert_eq!(qm.current_index(), Some(0));
    }

    /// I11 characterization (Phase-2 gate): `current_index` is `Some` IFF
    /// `current_order` is `Some`, across every mutator AND every navigation
    /// path (next, previous incl. order-walk, peek→transition, reanchor,
    /// reposition, drain-to-empty). This coupling is the precondition for
    /// deriving `current_index` from `order[current_order]` in M6 — if it
    /// ever splits, the derived encode of wire slot #2 would be lossy.
    /// (The commit-path sentinel asserts the same iff on every write;
    /// this test additionally covers the non-commit navigation setters.)
    #[test]
    fn playhead_coupling_holds_across_all_mutators() {
        let coupled = |qm: &QueueManager, label: &str| {
            assert_eq!(
                qm.get_queue().current_index().is_some(),
                qm.get_queue().current_order.is_some(),
                "{label}: current_index/current_order Some-ness split"
            );
            assert_queue_invariants(qm, label);
        };

        let songs: Vec<Song> = ["a", "b", "c", "d", "e", "f"]
            .iter()
            .map(|id| make_test_song(id))
            .collect();
        let (mut qm, _temp) = make_test_manager(songs, Some(0));
        coupled(&qm, "seeded");

        // Mutator script (M0-style) + navigation extensions.
        let _ = qm
            .add_songs(vec![make_test_song("g"), make_test_song("h")])
            .expect("add");
        coupled(&qm, "add_songs");

        let _ = qm.toggle_shuffle().expect("shuffle on");
        coupled(&qm, "toggle_shuffle on");

        let next = qm.get_next_song();
        assert!(next.is_some(), "next under shuffle");
        coupled(&qm, "get_next_song");

        // peek → transition (the gapless path).
        if let Some(peeked) = qm.peek_next_song() {
            let _ = peeked.transition();
        }
        coupled(&qm, "peek+transition");

        // peek → drop (abandoned prep).
        drop(qm.peek_next_song());
        coupled(&qm, "peek abandoned");

        let _ = qm.move_item(0, 3).expect("move");
        coupled(&qm, "move_item");

        let _ = qm.sort_queue(QueueSortMode::Title, true).expect("sort");
        coupled(&qm, "sort_queue");

        // Previous via history.
        qm.add_to_history_by_song_id("a");
        let _ = qm.get_previous_song(qm.get_queue().current_index());
        coupled(&qm, "previous (history)");

        // Previous via order-walk (no history left).
        qm.playback_history.clear();
        let _ = qm.reposition_to_index(Some(3));
        coupled(&qm, "reposition");
        let _ = qm.get_previous_song(Some(3));
        coupled(&qm, "previous (order-walk)");

        let _ = qm.reanchor_shuffle_to_index(2);
        coupled(&qm, "reanchor");

        let _ = qm.toggle_shuffle().expect("shuffle off");
        coupled(&qm, "toggle_shuffle off");

        let _ = qm.reposition_to_index(None);
        coupled(&qm, "reposition to None");
        let _ = qm.reposition_to_index(Some(1));
        coupled(&qm, "reposition to Some");

        // Consume-drain to empty: both halves must land on None together.
        let _ = qm.toggle_consume().expect("consume on");
        coupled(&qm, "toggle_consume");
        while !qm.is_queue_empty() {
            let cur = qm.get_queue().current_index().unwrap_or(0);
            let _ = qm.remove_song(cur).expect("drain");
            coupled(&qm, "drain remove");
        }
        assert_eq!(qm.get_queue().current_index(), None);
        assert_eq!(qm.get_queue().current_order, None);

        // Refill after empty.
        let _ = qm
            .set_queue(vec![make_test_song("z1"), make_test_song("z2")], Some(1))
            .expect("set_queue");
        coupled(&qm, "set_queue after drain");
    }

    /// B1 (release-safe): EVERY queue mutator must leave `order` a full
    /// permutation of `0..rows.len()` with the playhead coupling
    /// `order[current_order] == current_index` intact. Plain asserts, so
    /// this bites in `cargo test --release` too — the commit-path
    /// `assert_order_consistent` sentinel is debug-only.
    #[test]
    fn every_mutator_keeps_rows_order_consistent() {
        type Mutator = (&'static str, fn(&mut QueueManager));
        let mutators: &[Mutator] = &[
            ("add_songs", |qm| {
                let _ = qm
                    .add_songs(vec![make_test_song("n1"), make_test_song("n2")])
                    .expect("add_songs");
            }),
            ("set_queue", |qm| {
                let songs = vec![make_test_song("x"), make_test_song("y")];
                let _ = qm.set_queue(songs, Some(1)).expect("set_queue");
            }),
            ("remove_song", |qm| {
                let _ = qm.remove_song(2).expect("remove_song");
            }),
            ("remove_song_current", |qm| {
                let cur = qm.get_queue().current_index().expect("current");
                let _ = qm.remove_song(cur).expect("remove current");
            }),
            ("remove_song_by_id", |qm| {
                let _ = qm.remove_song_by_id("dup").expect("remove_song_by_id");
            }),
            ("remove_songs_by_ids", |qm| {
                let ids = vec!["a".to_string(), "dup".to_string()];
                let _ = qm.remove_songs_by_ids(&ids).expect("remove_songs_by_ids");
            }),
            ("remove_entry_by_id", |qm| {
                let eid = qm.entry_id_at(1).expect("entry at 1");
                let _ = qm.remove_entry_by_id(eid).expect("remove_entry_by_id");
            }),
            ("remove_entries_by_ids", |qm| {
                let eids: Vec<u64> = [0usize, 3]
                    .iter()
                    .filter_map(|&i| qm.entry_id_at(i))
                    .collect();
                let _ = qm.remove_entries_by_ids(&eids).expect("remove_entries");
            }),
            ("move_item", |qm| {
                let _ = qm.move_item(0, 4).expect("move_item");
            }),
            ("move_batch_by_entry_ids", |qm| {
                let eids: Vec<u64> = [4usize, 1]
                    .iter()
                    .filter_map(|&i| qm.entry_id_at(i))
                    .collect();
                let _ = qm
                    .move_batch_by_entry_ids(&eids, MoveBatchTarget::End)
                    .expect("move_batch");
            }),
            ("sort_queue", |qm| {
                let _ = qm
                    .sort_queue(QueueSortMode::Title, false)
                    .expect("sort_queue");
            }),
            ("sort_queue_random", |qm| {
                let _ = qm
                    .sort_queue(QueueSortMode::Random, true)
                    .expect("sort random");
            }),
            ("shuffle_queue", |qm| {
                let _ = qm.shuffle_queue().expect("shuffle_queue");
            }),
            ("toggle_shuffle", |qm| {
                let _ = qm.toggle_shuffle().expect("toggle_shuffle");
            }),
            ("toggle_consume", |qm| {
                let _ = qm.toggle_consume().expect("toggle_consume");
            }),
            ("set_repeat", |qm| {
                let _ = qm.set_repeat(RepeatMode::Playlist).expect("set_repeat");
            }),
            ("reposition_to_index", |qm| {
                let _ = qm.reposition_to_index(Some(3));
            }),
            ("reposition_to_none", |qm| {
                let _ = qm.reposition_to_index(None);
            }),
            ("reanchor_shuffle_to_index", |qm| {
                let _ = qm.reanchor_shuffle_to_index(4);
            }),
            ("insert_after_current", |qm| {
                let _ = qm
                    .insert_after_current(vec![make_test_song("i1")])
                    .expect("insert_after_current");
            }),
            ("insert_songs_at", |qm| {
                let _ = qm
                    .insert_songs_at(2, vec![make_test_song("i2"), make_test_song("i3")])
                    .expect("insert_songs_at");
            }),
            ("insert_song_and_make_current", |qm| {
                let _ = qm
                    .insert_song_and_make_current(1, make_test_song("i4"))
                    .expect("insert_song_and_make_current");
            }),
        ];

        for shuffle in [false, true] {
            for &(name, op) in mutators {
                // 6 rows incl. a duplicated id so duplicate-aware paths run.
                let mut songs: Vec<Song> = ["a", "b", "c", "d", "e"]
                    .iter()
                    .map(|id| make_test_song(id))
                    .collect();
                songs.push(make_test_song("dup"));
                songs.push(make_test_song("dup"));
                let (mut qm, _temp) = make_test_manager(songs, Some(2));
                if shuffle {
                    let _ = qm.toggle_shuffle().expect("preset shuffle");
                    assert_queue_invariants(&qm, "preset shuffle");
                }
                op(&mut qm);
                let label = format!("{name} (shuffle={shuffle})");
                assert_queue_invariants(&qm, &label);
            }
        }
    }

    /// B2: the commit-path `assert_order_consistent` sentinel is
    /// unbypassable — corrupting `order` under a live write guard must
    /// panic the commit in debug builds.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "order length drifted from rows")]
    fn commit_paths_assert_order_consistency() {
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));
        let mut tx = qm.write();
        tx.queue.order.corrupt_push_for_test(0); // deliberate corruption: length drift
        let _ = tx.commit_no_save();
    }

    /// M2: the Decode placeholders (`entry_id = position` at decode time)
    /// must be RESEEDED after `reconcile_loaded_queue` prunes rows — the
    /// surviving rows carry dense `0..new_len` ids and the allocator starts
    /// past them. Without the reseed, pruning a middle row leaves a gap
    /// (ids 0,2) and a colliding allocator.
    #[test]
    fn decode_placeholder_entry_ids_are_reseeded_after_reconcile() {
        // Queue blob lists [a, b, c] but the pool only holds [a, c] — row b
        // is pruned by reconcile on load.
        const QUEUE_BLOB_ABC: &[u8] = &[
            3, 1, 97, 1, 98, 1, 99, // song_ids: ["a","b","c"]
            1, 0, // current_index: Some(0)
            1, 0, // current_order: Some(0)
            3, 0, 1, 2, // order: [0, 1, 2]
            0, // queued: None
            0, // shuffle: false
            0, // repeat: None
            0, // consume: false
        ];

        let temp = tempfile::TempDir::new().expect("temp dir");
        let storage = StateStorage::new(temp.path().join("queue.redb")).expect("temp storage");

        let songs: Vec<Song> = ["a", "c"].iter().map(|id| make_test_song(id)).collect();
        let pool_blob = bincode_next::encode_to_vec(&songs, bincode_next::config::standard())
            .expect("encode pool blob");

        storage
            .save_binary_batch(&[
                (KEY_QUEUE_ORDER, QUEUE_BLOB_ABC),
                (KEY_QUEUE_SONGS, pool_blob.as_slice()),
            ])
            .expect("write blobs");

        let qm = QueueManager::new(storage).expect("load");

        assert_eq!(
            row_pairs(&qm),
            vec![("a".to_string(), 0), ("c".to_string(), 1)],
            "surviving rows must carry dense reseeded entry ids, not decode placeholders"
        );
        assert_eq!(
            qm.next_entry_id, 2,
            "allocator starts past the reseeded range"
        );
        assert_queue_invariants(&qm, "reseed after reconcile");
    }

    /// A1: the behavior oracle for the whole QueueRow refactor. Chains every
    /// row-reordering mutator and asserts the surviving `(song_id, entry_id)`
    /// pairs after EACH step against hand-computed expectations. Physical row
    /// identity is deterministic even with shuffle on — shuffle only permutes
    /// the `order` array, never the rows.
    #[test]
    fn row_identity_survives_mutation_pipeline() {
        let songs: Vec<Song> = ["a", "b", "c", "d", "e", "f"]
            .iter()
            .map(|id| make_test_song(id))
            .collect();
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

        let pairs_of = |ids: &[(&str, u64)]| -> Vec<(String, u64)> {
            ids.iter().map(|&(s, e)| (s.to_string(), e)).collect()
        };

        assert_eq!(
            row_pairs(&qm),
            pairs_of(&[("a", 0), ("b", 1), ("c", 2), ("d", 3), ("e", 4), ("f", 5)])
        );

        // 1. add_songs: fresh rows get fresh monotonic entry ids (6, 7).
        let _ = qm
            .add_songs(vec![make_test_song("g"), make_test_song("h")])
            .expect("add_songs");
        assert_eq!(
            row_pairs(&qm),
            pairs_of(&[
                ("a", 0),
                ("b", 1),
                ("c", 2),
                ("d", 3),
                ("e", 4),
                ("f", 5),
                ("g", 6),
                ("h", 7),
            ]),
            "after add_songs"
        );
        assert_queue_invariants(&qm, "after add_songs");

        // 2. toggle_shuffle: permutes `order` only — physical rows untouched.
        let _ = qm.toggle_shuffle().expect("toggle_shuffle");
        assert_eq!(
            row_pairs(&qm),
            pairs_of(&[
                ("a", 0),
                ("b", 1),
                ("c", 2),
                ("d", 3),
                ("e", 4),
                ("f", 5),
                ("g", 6),
                ("h", 7),
            ]),
            "after toggle_shuffle"
        );
        assert_queue_invariants(&qm, "after toggle_shuffle");

        // 3. move_item(0, 3): row a lands at index 2 (insert_at = to - 1).
        let _ = qm.move_item(0, 3).expect("move_item");
        assert_eq!(
            row_pairs(&qm),
            pairs_of(&[
                ("b", 1),
                ("c", 2),
                ("a", 0),
                ("d", 3),
                ("e", 4),
                ("f", 5),
                ("g", 6),
                ("h", 7),
            ]),
            "after move_item"
        );
        assert_queue_invariants(&qm, "after move_item");

        // 4. move_batch_by_entry_ids([5, 1] → above entry 6): rows f and b
        // move as a block, ascending original order, above row g.
        let _ = qm
            .move_batch_by_entry_ids(&[5, 1], MoveBatchTarget::AboveEntry(6))
            .expect("move_batch");
        assert_eq!(
            row_pairs(&qm),
            pairs_of(&[
                ("c", 2),
                ("a", 0),
                ("d", 3),
                ("e", 4),
                ("b", 1),
                ("f", 5),
                ("g", 6),
                ("h", 7),
            ]),
            "after move_batch_by_entry_ids"
        );
        assert_queue_invariants(&qm, "after move_batch_by_entry_ids");

        // 5. sort_queue(Title asc): titles are "Song {id}" so rows return to
        // alphabetical id order — entry ids ride along with their rows.
        let _ = qm
            .sort_queue(QueueSortMode::Title, true)
            .expect("sort_queue");
        assert_eq!(
            row_pairs(&qm),
            pairs_of(&[
                ("a", 0),
                ("b", 1),
                ("c", 2),
                ("d", 3),
                ("e", 4),
                ("f", 5),
                ("g", 6),
                ("h", 7),
            ]),
            "after sort_queue"
        );
        assert_queue_invariants(&qm, "after sort_queue");

        // 6. remove_entry_by_id(2): row c disappears; every other pair intact.
        let _ = qm.remove_entry_by_id(2).expect("remove_entry_by_id");
        assert_eq!(
            row_pairs(&qm),
            pairs_of(&[
                ("a", 0),
                ("b", 1),
                ("d", 3),
                ("e", 4),
                ("f", 5),
                ("g", 6),
                ("h", 7),
            ]),
            "after remove_entry_by_id"
        );
        assert_queue_invariants(&qm, "after remove_entry_by_id");

        // 7. toggle_consume + a consume-style removal of the head row —
        // mode flags never disturb row identity.
        let _ = qm.toggle_consume().expect("toggle_consume");
        let _ = qm.remove_entry_by_id(0).expect("consume head row");
        assert_eq!(
            row_pairs(&qm),
            pairs_of(&[("b", 1), ("d", 3), ("e", 4), ("f", 5), ("g", 6), ("h", 7),]),
            "after consume removal"
        );
        assert_queue_invariants(&qm, "after consume removal");
    }

    /// A2: `order` keys rows by PHYSICAL POSITION, never by `entry_id`
    /// value. After churn pushes entry ids past `len`, `order` must remain a
    /// permutation of `0..len` while entry ids are large.
    #[test]
    fn order_indexes_rows_by_position_not_entry_id() {
        let songs: Vec<Song> = ["a", "b", "c"]
            .iter()
            .map(|id| make_test_song(id))
            .collect();
        let (mut qm, _temp) = make_test_manager(songs, None);

        // Churn: drop the first two rows, then append three fresh ones so
        // allocated entry ids (3, 4, 5) exceed every valid row index.
        let _ = qm.remove_song(0).expect("remove a");
        let _ = qm.remove_song(0).expect("remove b");
        let _ = qm
            .add_songs(vec![
                make_test_song("d"),
                make_test_song("e"),
                make_test_song("f"),
            ])
            .expect("add fresh rows");

        assert_eq!(
            row_pairs(&qm),
            vec![
                ("c".to_string(), 2),
                ("d".to_string(), 3),
                ("e".to_string(), 4),
                ("f".to_string(), 5),
            ]
        );
        let len = qm.queue.rows.len();
        let max_entry = qm.entry_ids().into_iter().max().expect("entries");
        assert!(
            max_entry >= len as u64,
            "churn must push entry ids past the index range for this test to bite"
        );

        // `order` stays a permutation of 0..len (positions), untouched by
        // the large entry-id VALUES living on the same rows.
        assert_queue_invariants(&qm, "after churn");
        let mut sorted = qm.queue.order.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..len).collect::<Vec<_>>());

        // Position→entry and entry→position lookups agree on every row.
        for (pos, &(_, eid)) in row_pairs(&qm).iter().enumerate() {
            assert_eq!(qm.entry_id_at(pos), Some(eid));
            assert_eq!(qm.index_of_entry(eid), Some(pos));
        }
    }

    /// Firmium-trap guard (one-shot Shuffle Play): permuting the list with
    /// [`OneShotShuffle`] and handing it to `set_queue` must NEVER flip the
    /// persistent shuffle MODE flag (`queue.shuffle`) — in either preset state —
    /// and must preserve the exact song multiset. The one-shot directive operates
    /// only on a detached `Vec<Song>`; it has no access to the mode flag.
    #[test]
    fn one_shot_shuffle_into_set_queue_never_flips_mode_flag() {
        use std::collections::BTreeSet;

        use rand::{SeedableRng, rngs::StdRng};

        use crate::types::one_shot_shuffle::OneShotShuffle;

        let expected: BTreeSet<String> = ["a", "b", "c", "d", "e", "f", "g", "h"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        for preset in [false, true] {
            let mut songs: Vec<Song> = expected.iter().map(|id| make_test_song(id)).collect();
            OneShotShuffle::Full.apply_with(&mut songs, &mut StdRng::seed_from_u64(11));

            let (mut qm, _t) = make_test_manager(Vec::new(), None);
            qm.queue.shuffle = preset;
            let _ = qm.set_queue(songs, Some(0)).expect("set_queue");

            assert_eq!(
                qm.queue.shuffle, preset,
                "a one-shot shuffle must not write the persistent shuffle mode flag (preset={preset})"
            );
            let got: BTreeSet<String> = qm.song_ids_snapshot().iter().cloned().collect();
            assert_eq!(
                got, expected,
                "the shuffled queue preserves the song multiset"
            );
        }
    }

    // ── reanchor_shuffle_to_index (play-from-here under shuffle) ──

    /// Baseline / regression: a plain reposition onto the LAST slot of a
    /// shuffle order dead-ends — peek returns None. This is the exact bug the
    /// re-anchor fixes (Razrushitelniy Krug at order[15], repeat/consume off).
    #[test]
    fn reposition_onto_last_shuffle_slot_dead_ends() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
            make_test_song("d"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));
        qm.queue.shuffle = true;
        // song_ids[0] ("a") sits at the LAST order slot.
        qm.queue.order = vec![1, 2, 3, 0].into();

        let _ = qm.reposition_to_index(Some(0));
        assert_eq!(qm.queue.current_order, Some(3));
        assert!(
            qm.peek_next_song().is_none(),
            "last shuffle slot with repeat/consume off has no next track"
        );
    }

    /// Re-anchoring on play-from-here makes the clicked track the head of a
    /// fresh shuffle: it lands at order[0], current_order is 0, every track is
    /// still present, and there IS a next track (the dead-end is gone).
    #[test]
    fn reanchor_shuffle_makes_clicked_track_head_and_has_next() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
            make_test_song("d"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));
        qm.queue.shuffle = true;
        // Clicked song_ids[0] ("a") starts at the LAST order slot — the exact
        // dead-end position.
        qm.queue.order = vec![1, 2, 3, 0].into();

        let _ = qm.reanchor_shuffle_to_index(0);

        assert_eq!(qm.queue.current_index(), Some(0));
        assert_eq!(qm.queue.current_order, Some(0));
        assert_eq!(qm.queue.order[0], 0, "clicked track anchored at head");

        // Order is still a full permutation of every song index — no track
        // dropped, none duplicated.
        let mut sorted = qm.queue.order.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, vec![0, 1, 2, 3]);

        // No dead-end: a next track now exists.
        assert!(
            qm.peek_next_song().is_some(),
            "re-anchored shuffle must have a next track"
        );
    }

    /// With shuffle OFF, re-anchor is a plain reposition: the identity order
    /// is untouched and no reshuffle happens.
    #[test]
    fn reanchor_with_shuffle_off_is_plain_reposition() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
            make_test_song("d"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));
        // shuffle stays off; order is identity [0, 1, 2, 3].

        let _ = qm.reanchor_shuffle_to_index(2);

        assert_eq!(qm.queue.current_index(), Some(2));
        assert_eq!(qm.queue.current_order, Some(2));
        assert_eq!(
            qm.queue.order,
            vec![0, 1, 2, 3],
            "identity order must be untouched when shuffle is off"
        );
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
        let ids = qm.song_ids_snapshot();
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
        let ids = qm.song_ids_snapshot();
        assert_eq!(ids, vec!["c", "a", "b"]);
    }

    #[test]
    fn move_item_same_position_is_noop() {
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let (mut qm, _temp) = make_test_manager(songs, None);

        let _ = qm.move_item(1, 1).unwrap();
        let ids = qm.song_ids_snapshot();
        assert_eq!(ids, vec!["a", "b"]);
    }

    #[test]
    fn move_item_out_of_bounds_is_noop() {
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let (mut qm, _temp) = make_test_manager(songs, None);

        let _ = qm.move_item(5, 0).unwrap();
        let ids = qm.song_ids_snapshot();
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
        assert_eq!(qm.queue.current_index(), Some(1));
        assert_eq!(qm.rows()[1].song_id, "a");
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
        assert_eq!(qm.queue.current_index(), Some(0));
        assert_eq!(qm.rows()[0].song_id, "c");
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
        assert_eq!(qm.queue.current_index(), Some(0));
        assert_eq!(qm.rows()[0].song_id, "b");
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
        assert_eq!(qm.queue.current_index(), Some(2));
        assert_eq!(qm.rows()[2].song_id, "b");
    }

    #[test]
    fn move_item_to_end_of_two_item_queue() {
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let (mut qm, _temp) = make_test_manager(songs, None);

        // from=0, to=2 (== len) means "place after the last item"
        let _ = qm.move_item(0, 2).unwrap();
        let ids = qm.song_ids_snapshot();
        assert_eq!(ids, vec!["b", "a"]);
    }

    // ── move_batch_by_entry_ids tests ──

    fn songs_n(n: usize) -> Vec<Song> {
        (0..n).map(|i| make_test_song(&format!("s{i}"))).collect()
    }

    #[test]
    fn move_batch_by_entry_ids_above_target_collects_block() {
        let (mut qm, _t) = make_test_manager(songs_n(5), None);
        let eids = qm.entry_ids();

        // Move s0, s2, s4 to above s1 → block lands at position 0
        // (s1's index 1 minus 1 row removed before it = 0).
        let _: NextTrackResetEffect = qm
            .move_batch_by_entry_ids(
                &[eids[0], eids[2], eids[4]],
                MoveBatchTarget::AboveEntry(eids[1]),
            )
            .unwrap();

        let ids = qm.song_ids_snapshot();
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
        let eids = qm.entry_ids();

        let _ = qm
            .move_batch_by_entry_ids(&[eids[0], eids[2]], MoveBatchTarget::End)
            .unwrap();

        let ids = qm.song_ids_snapshot();
        assert_eq!(ids, vec!["s1", "s3", "s0", "s2"]);
        assert_eq!(qm.entry_ids(), &[eids[1], eids[3], eids[0], eids[2]]);
    }

    #[test]
    fn move_batch_by_entry_ids_unknown_ids_silently_skipped() {
        let (mut qm, _t) = make_test_manager(songs_n(3), None);
        let eids = qm.entry_ids();

        // 9999 is a fresh u64 that hasn't been handed out.
        let _ = qm
            .move_batch_by_entry_ids(&[eids[0], 9999, eids[2]], MoveBatchTarget::End)
            .unwrap();

        let ids = qm.song_ids_snapshot();
        assert_eq!(ids, vec!["s1", "s0", "s2"]);
    }

    #[test]
    fn move_batch_by_entry_ids_empty_is_noop() {
        let (mut qm, _t) = make_test_manager(songs_n(3), None);
        let before_ids = qm.song_ids_snapshot();
        let before_eids = qm.entry_ids();

        let _ = qm
            .move_batch_by_entry_ids(&[], MoveBatchTarget::End)
            .unwrap();

        assert_eq!(qm.song_ids_snapshot(), before_ids);
        assert_eq!(qm.entry_ids(), before_eids.as_slice());
    }

    #[test]
    fn move_batch_by_entry_ids_dedups_repeated_input() {
        let (mut qm, _t) = make_test_manager(songs_n(3), None);
        let eids = qm.entry_ids();

        // Same entry_id passed twice → resolves to one move.
        let _ = qm
            .move_batch_by_entry_ids(&[eids[0], eids[0]], MoveBatchTarget::End)
            .unwrap();

        let ids = qm.song_ids_snapshot();
        assert_eq!(ids, vec!["s1", "s2", "s0"]);
    }

    #[test]
    fn move_batch_by_entry_ids_preserves_current_song_through_shift() {
        // s1 playing; move s0 to end → s1 shifts to index 0 but stays current.
        let (mut qm, _t) = make_test_manager(songs_n(4), Some(1));
        let eids = qm.entry_ids();

        let _ = qm
            .move_batch_by_entry_ids(&[eids[0]], MoveBatchTarget::End)
            .unwrap();

        assert_eq!(qm.song_ids_snapshot(), vec!["s1", "s2", "s3", "s0"]);
        assert_eq!(qm.queue.current_index(), Some(0));
        assert_eq!(qm.entry_id_at(0), Some(eids[1]));
    }

    #[test]
    fn move_batch_by_entry_ids_preserves_current_when_current_is_moved() {
        // s2 playing; move s1, s2 to end → s2 still current at new position.
        let (mut qm, _t) = make_test_manager(songs_n(4), Some(2));
        let eids = qm.entry_ids();

        let _ = qm
            .move_batch_by_entry_ids(&[eids[1], eids[2]], MoveBatchTarget::End)
            .unwrap();

        assert_eq!(qm.song_ids_snapshot(), vec!["s0", "s3", "s1", "s2"]);
        assert_eq!(qm.queue.current_index(), Some(3));
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
        let eids = qm.entry_ids();

        // Move only the FIRST "a" to end; the second "a" stays put.
        let _ = qm
            .move_batch_by_entry_ids(&[eids[0]], MoveBatchTarget::End)
            .unwrap();

        assert_eq!(qm.song_ids_snapshot(), vec!["b", "a", "a"]);
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
        let eids = qm.entry_ids();
        let target_eid = eids[2];

        // External insert shifts s2 from index 2 to index 3.
        let _ = qm.insert_songs_at(0, vec![make_test_song("X")]).unwrap();
        assert_eq!(qm.rows()[3].song_id, "s2");

        // The pre-shift entry_id still resolves to s2.
        let _ = qm
            .move_batch_by_entry_ids(&[target_eid], MoveBatchTarget::End)
            .unwrap();

        assert_eq!(qm.song_ids_snapshot().last(), Some(&"s2".to_string()));
    }

    #[test]
    fn move_batch_by_entry_ids_target_in_move_set_lands_contiguous() {
        // Move s1, s2, s3 above s2. s2 is in the move set; the block
        // lands where s2 originally sat (index 2), minus the removed
        // count before it (1 → s1), so insert_at = 1. Result is the
        // original order (effectively a no-op for a contiguous run).
        let (mut qm, _t) = make_test_manager(songs_n(5), None);
        let eids = qm.entry_ids();

        let _ = qm
            .move_batch_by_entry_ids(
                &[eids[1], eids[2], eids[3]],
                MoveBatchTarget::AboveEntry(eids[2]),
            )
            .unwrap();

        let ids = qm.song_ids_snapshot();
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
        let mut sorted_before = before_order.to_vec();
        let mut sorted_after = qm.queue.order.to_vec();
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
        qm2.set_cursor_to_row(Some(0));
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
        let eids = qm.entry_ids();

        let before_play_eids = qm.capture_play_order_entry_ids();
        let before_order = qm.queue.order.clone();

        // Move two non-current rows to the end.
        let _ = qm
            .move_batch_by_entry_ids(&[eids[3], eids[4]], MoveBatchTarget::End)
            .unwrap();

        // Still a valid permutation.
        let mut sorted_before = before_order.to_vec();
        let mut sorted_after = qm.queue.order.to_vec();
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
        let ci = qm.queue.current_index().unwrap();
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
        assert_eq!(qm.queue.current_index(), Some(1)); // "c" shifted from 2→1
        assert_eq!(qm.rows()[1].song_id, "c");
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
        assert_eq!(qm.queue.current_index(), Some(0)); // unchanged
        assert_eq!(qm.rows()[0].song_id, "a");
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
        assert_eq!(qm.queue.current_index(), Some(1)); // clamped to last valid
    }

    #[test]
    fn remove_song_until_empty_clears_index() {
        let songs = vec![make_test_song("a")];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

        let _ = qm.remove_song(0).unwrap();
        assert_eq!(qm.queue.current_index(), None);
        assert!(qm.song_ids_snapshot().is_empty());
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

        assert_eq!(qm.queue.current_index(), Some(1));
        assert_eq!(qm.rows()[1].song_id, "e");
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
        q.rows = song_ids
            .iter()
            .enumerate()
            .map(|(i, s)| QueueRow {
                song_id: s.to_string(),
                entry_id: i as u64,
            })
            .collect();
        q.order = PlayOrder::identity(song_ids.len());
        // Identity order: the cursor slot equals the physical index, and
        // current_index() derives from it.
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
        assert_eq!(song_ids_of(&q), vec!["A", "C", "D"]);
        // C followed by remap: was index 2, now index 1 (NOT clamped/None).
        assert_eq!(q.current_index(), Some(1));
        // order is a valid permutation of 0..3 with no entry >= 3.
        assert!(order_is_identity_permutation(&q.order, 3));
        assert!(q.order.iter().all(|&i| i < 3));
    }

    #[test]
    fn reconcile_pruned_current_clamps_to_adjacent_survivor() {
        // song_ids [A,B,C,D,E], identity order, current=Some(2) (=C).
        // Pool drops C (the currently-playing row). The surviving playhead
        // must stay in place at the adjacent survivor (D), NOT jump to the
        // queue tail (E), so the unplayed middle isn't silently skipped.
        let mut q = queue_with(&["A", "B", "C", "D", "E"], Some(2));
        let pool = pool_with(&["A", "B", "D", "E"]);

        let dirty = reconcile_loaded_queue(&mut q, &pool);
        assert!(dirty);
        assert_eq!(song_ids_of(&q), vec!["A", "B", "D", "E"]);
        // Adjacent survivor, NOT Some(3) (the tail).
        assert_eq!(q.current_index(), Some(2));
        // Resolves to D, not E.
        assert_eq!(q.rows[q.current_index().unwrap()].song_id, "D");
        // Invariant + sync: order[current_order] == current_index.
        assert_eq!(q.current_order, Some(2));
        assert_eq!(
            q.order[q.current_order.unwrap()],
            q.current_index().unwrap()
        );
        // Forward reachability of the tail: order=[0,1,2,3] so the next
        // forward step (order[current_order+1]) maps to song_ids[3] == "E".
        assert_eq!(q.order, vec![0, 1, 2, 3]);
        assert_eq!(q.rows[q.order[q.current_order.unwrap() + 1]].song_id, "E");
    }

    #[test]
    fn reconcile_clamps_out_of_range_index() {
        let mut q = queue_with(&["A", "B"], Some(7));
        let pool = pool_with(&["A", "B"]);

        let dirty = reconcile_loaded_queue(&mut q, &pool);
        // Cursor 7 is out of range for the 2-row order: the repair drops the
        // playhead entirely (cursor-anchored model — there is no stored
        // physical index to salvage) and flags the queue dirty. A regression
        // that leaves the stale out-of-range cursor in place must fail HERE,
        // not later at a write-guard commit assert.
        assert!(dirty);
        assert_eq!(q.current_order, None, "out-of-range cursor must be dropped");
        assert_eq!(q.current_index(), None);
    }

    #[test]
    fn reconcile_empty_pool_normalizes() {
        let mut q = queue_with(&["A", "B", "C"], Some(1));
        let pool = SongPool::default();

        let dirty = reconcile_loaded_queue(&mut q, &pool);
        assert!(dirty);
        assert!(q.rows.is_empty());
        assert_eq!(q.current_index(), None);
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
        assert_eq!(song_ids_of(&q), vec!["A", "B", "C"]);
        assert_eq!(q.current_index(), Some(1));
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
        assert!(qm.is_queue_empty());
        assert_eq!(qm.get_queue().current_index(), None);
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
        assert_eq!(qm2.song_ids_snapshot(), vec!["a", "b"]);
        assert_eq!(qm2.get_queue().current_index(), Some(0));
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
        assert_eq!(song_ids_of(&queue), vec!["x", "y"]);
        assert_eq!(queue.current_index(), Some(0));
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
        assert_eq!(qm.queue.current_index(), Some(1));
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
        assert_eq!(qm.song_ids_snapshot(), vec!["a", "c", "d"]);
    }

    // ── QUEUE-1: removing the playing row under shuffle must keep the
    //    invariant order[current_order] == current_index AND reach every
    //    still-upcoming survivor exactly once before stopping (no replay,
    //    no strand). These tests drain via get_next_song() and assert the
    //    EXACT play sequence so a bare first-match row re-sync (the
    //    historically rejected fix, which strands / over-plays) cannot pass.

    /// Drain the queue from the current song: push the current song id, then
    /// repeatedly call get_next_song() collecting each id until None (capped to
    /// avoid spinning under repeat modes). Returns the full play sequence.
    fn drain_play_sequence(qm: &mut QueueManager, cap: usize) -> Vec<String> {
        let mut seq = Vec::new();
        if let Some(idx) = qm.queue.current_index()
            && let Some(id) = qm.song_id_at(idx)
        {
            seq.push(id.to_owned());
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
    /// historically rejected bare first-match row re-sync STRANDS s2 and
    /// drains to only [s1]. This is the test that enforces the spec's
    /// acceptance bar "do NOT ship the bare one-liner alone". (Post-M6 the
    /// derive IS the only possible behavior — this pins it forever.)
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
        qm.queue.order = vec![0, 2, 1].into();
        qm.queue.current_order = Some(0); // order[0] == 0 == s0 playing

        let _ = qm.remove_song(0).unwrap(); // remove playing s0
        // song_ids after removal: [s1, s2]
        assert_eq!(qm.song_ids_snapshot(), vec!["s1", "s2"]);

        // (a) invariant restored
        let co = qm.queue.current_order.expect("current_order set");
        let ci = qm.queue.current_index().expect("current_index set");
        assert_eq!(qm.queue.order[co], ci, "invariant order[co]==ci broken");

        // (b) no immediate replay of the removed/current song
        let cur_id = qm.rows()[ci].song_id.clone();
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
        qm.queue.order = vec![2, 1, 0, 3].into();
        qm.queue.current_order = Some(1); // order[1] == 1 == s1 playing

        let _ = qm.remove_song(1).unwrap(); // remove playing s1
        // song_ids after removal: [s0, s2, s3]
        assert_eq!(qm.song_ids_snapshot(), vec!["s0", "s2", "s3"]);

        // (a) invariant restored
        let co = qm.queue.current_order.expect("current_order set");
        let ci = qm.queue.current_index().expect("current_index set");
        assert_eq!(qm.queue.order[co], ci, "invariant order[co]==ci broken");

        // (b) no immediate replay
        let cur_id = qm.rows()[ci].song_id.clone();
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
        qm.queue.order = vec![2, 0, 1].into();
        qm.queue.current_order = Some(2); // order[2] == 1 == s1 playing

        let _ = qm.remove_song(1).unwrap(); // remove playing s1
        // song_ids after removal: [s0, s2]
        assert_eq!(qm.song_ids_snapshot(), vec!["s0", "s2"]);

        // (a) invariant restored
        let co = qm.queue.current_order.expect("current_order set");
        let ci = qm.queue.current_index().expect("current_index set");
        assert_eq!(qm.queue.order[co], ci, "invariant order[co]==ci broken");

        // (b) no replay of the current song
        let cur_id = qm.rows()[ci].song_id.clone();
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
        let mut sorted_order = qm.queue.order.to_vec();
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
        assert_eq!(qm.queue.current_index(), Some(2));
        assert_eq!(qm.rows()[2].song_id, "c");
    }

    #[test]
    fn sort_queue_empty_is_noop() {
        use crate::types::queue_sort_mode::QueueSortMode;

        let (mut qm, _temp) = make_test_manager(vec![], None);
        let _ = qm.sort_queue(QueueSortMode::Title, true).unwrap();
        assert!(qm.song_ids_snapshot().is_empty());
        assert_eq!(qm.queue.current_index(), None);
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
        assert_eq!(qm.song_ids_snapshot(), vec!["b", "c", "a"]);
    }

    #[test]
    fn sort_queue_by_most_played_treats_none_as_zero() {
        use crate::types::queue_sort_mode::QueueSortMode;

        let mut songs = vec![make_test_song("a"), make_test_song("b")];
        songs[0].play_count = None;
        songs[1].play_count = Some(3);
        let (mut qm, _temp) = make_test_manager(songs, None);

        let _ = qm.sort_queue(QueueSortMode::MostPlayed, true).unwrap();
        assert_eq!(qm.song_ids_snapshot(), vec!["b", "a"]);
    }

    #[test]
    fn shuffle_queue_preserves_current_song_identity() {
        let songs: Vec<Song> = (0..20).map(|i| make_test_song(&i.to_string())).collect();
        let (mut qm, _temp) = make_test_manager(songs, Some(7)); // playing "7"

        let _ = qm.shuffle_queue().unwrap();

        // current_index should point to "7" wherever it ended up
        let idx = qm.queue.current_index().unwrap();
        assert_eq!(
            qm.rows()[idx].song_id,
            "7",
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
        assert_eq!(qm.queue.current_index(), Some(1));
        assert_eq!(qm.rows()[1].song_id, "b");
        // New songs at 2,3
        assert_eq!(qm.rows()[2].song_id, "x");
        assert_eq!(qm.rows()[3].song_id, "y");
    }

    #[test]
    fn insert_after_current_when_nothing_playing() {
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let (mut qm, _temp) = make_test_manager(songs, None);

        let new_songs = vec![make_test_song("x")];
        let _ = qm.insert_after_current(new_songs).unwrap();

        // With no current_index, inserts at end
        assert_eq!(qm.queue_len(), 3);
        assert_eq!(qm.rows()[2].song_id, "x");
        assert_eq!(qm.queue.current_index(), None);
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
        assert_eq!(qm.queue.current_index(), Some(5));
        assert_eq!(qm.rows()[5].song_id, "d");
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

        assert_eq!(qm.queue.current_index(), Some(1)); // unchanged
        assert_eq!(qm.rows()[1].song_id, "b");
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

        assert_eq!(qm.queue.current_index(), Some(2)); // unchanged
        assert_eq!(qm.rows()[2].song_id, "c");
        assert_eq!(qm.queue_len(), 6);
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

        assert_eq!(qm.song_ids_snapshot(), vec!["a", "b", "d"]);
        assert!(qm.pool.get("c").is_none());
    }

    #[test]
    fn remove_song_by_id_unknown_id_is_noop() {
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

        let _ = qm.remove_song_by_id("nonexistent").unwrap();

        assert_eq!(qm.song_ids_snapshot(), vec!["a", "b"]);
        assert_eq!(qm.queue.current_index(), Some(0));
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
        assert_eq!(qm.song_ids_snapshot(), vec!["b", "c"]);
        assert_eq!(qm.queue.current_index(), Some(1)); // still points at "c"
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

        assert_eq!(qm.song_ids_snapshot(), vec!["a", "c", "e"]);
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

        assert_eq!(qm.song_ids_snapshot(), vec!["a"]);
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

        assert_eq!(qm.song_ids_snapshot(), vec!["b", "d"]);
    }

    #[test]
    fn remove_songs_by_ids_empty_is_noop() {
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

        let _ = qm.remove_songs_by_ids(&[]).unwrap();

        assert_eq!(qm.song_ids_snapshot(), vec!["a", "b"]);
        assert_eq!(qm.queue.current_index(), Some(0));
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

        assert_eq!(qm.song_ids_snapshot(), vec!["dup", "dup"]);
        let entry_ids = qm.entry_ids();
        assert_eq!(entry_ids.len(), 2, "two rows should have two entry_ids");
        assert_ne!(
            entry_ids[0], entry_ids[1],
            "duplicate rows must get distinct entry_ids",
        );

        let target = entry_ids[1];
        let _ = qm.remove_entry_by_id(target).unwrap();

        assert_eq!(
            qm.song_ids_snapshot(),
            vec!["dup"],
            "second row should remain"
        );
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

        assert_eq!(qm.song_ids_snapshot(), vec!["a", "b"]);
        assert_eq!(qm.entry_ids().len(), 2);
    }

    #[test]
    fn remove_entries_by_ids_removes_each_targeted_row() {
        let song = make_test_song("dup");
        let unique = make_test_song("uniq");
        let (mut qm, _temp) = make_test_manager(vec![song.clone(), unique, song.clone()], Some(0));
        let entry_ids = qm.entry_ids();

        // Remove the two duplicate rows, leave the unique row.
        let _ = qm
            .remove_entries_by_ids(&[entry_ids[0], entry_ids[2]])
            .unwrap();

        assert_eq!(qm.song_ids_snapshot(), vec!["uniq"]);
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

        assert_eq!(qm.song_ids_snapshot(), vec!["uniq"]);
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
        let original_entry_ids = qm.entry_ids();
        assert_eq!(original_entry_ids.len(), 3);

        // Mimic `on_track_finished`'s decide_transition: peek + transition
        // bumps current_index from 0 → 1.
        let peeked = qm.peek_next_song().expect("peek next song");
        let transition = peeked.transition();
        assert_eq!(transition.old_index, Some(0));
        assert_eq!(transition.new_index, 1);
        assert_eq!(qm.queue.current_index(), Some(1));

        // Then `record_and_consume` runs `remove_song(prev_index)` where
        // prev_index is the captured `transition.old_index`.
        let _ = qm.remove_song(0).expect("consume previous index");

        assert_eq!(
            qm.song_ids_snapshot(),
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
            qm.queue.current_index(),
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
        assert_eq!(qm.song_ids_snapshot(), vec!["A", "B"]);
        assert_eq!(qm.queue.current_index(), Some(0));
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
        assert_eq!(qm.song_ids_snapshot(), vec!["B"]);
        assert_eq!(qm.queue.current_index(), Some(0));
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

        let original = qm.entry_ids();
        let _ = qm.sort_queue(QueueSortMode::Title, true).unwrap();

        // After ascending title sort: Alpha (b), Bravo (c), Charlie (a).
        assert_eq!(qm.song_ids_snapshot(), vec!["b", "c", "a"]);
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
        assert_eq!(qm.song_ids_snapshot(), vec!["B", "C", "D"]);

        // NEW path removes B (correct) regardless of the index drift.
        let _ = qm.remove_entry_by_id(b_eid).unwrap();
        assert_eq!(
            qm.song_ids_snapshot(),
            vec!["C", "D"],
            "B removed by identity"
        );

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
            qm2.song_ids_snapshot(),
            vec!["B", "D"],
            "raw index removed the WRONG row (C) — the bug the fix avoids",
        );
    }

    #[test]
    fn entry_ids_survive_move_item() {
        let (mut qm, _temp) = make_test_manager(songs_n(3), None);
        let eids = qm.entry_ids();

        // Move s0 to position 3 (end). entry_ids must travel with the row.
        let _ = qm.move_item(0, 3).unwrap();

        assert_eq!(qm.song_ids_snapshot(), vec!["s1", "s2", "s0"]);
        assert_eq!(
            qm.entry_ids(),
            &[eids[1], eids[2], eids[0]],
            "move_item must keep each row's entry_id riding with its song through the reorder",
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
