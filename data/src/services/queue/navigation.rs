//! Song navigation for `QueueManager`
//!
//! Peek/transition/next/previous song logic with order array,
//! repeat modes, and playback history.

use tracing::{debug, trace};

use super::{HistoryEntry, QueueManager};
use crate::types::{queue::RepeatMode, song::Song};

#[derive(Debug, Clone)]
pub struct NextSongResult {
    pub song: Song,
    pub index: usize,
    pub reason: String, // "repeat", "shuffle", "next", "repeatQueue"
}

/// Result of a transition to the queued song.
#[derive(Debug, Clone)]
pub struct TransitionResult {
    pub song: Song,
    /// The song_ids index we transitioned FROM (for consume/history).
    pub old_index: Option<usize>,
    /// The song_ids index we transitioned TO.
    pub new_index: usize,
}

/// Result of looking up the previous song from playback history
#[derive(Debug, Clone)]
pub enum PreviousSongResult {
    /// Song found in current queue at this index
    InQueue(Song, usize),
    /// Song from history but no longer in queue (consumed/removed) — caller should re-insert
    Removed(Song),
    /// No previous song available
    None,
}

/// Borrow guard returned by [`QueueManager::peek_next_song`].
///
/// Owns the only public path to commit the peek to a transition.
/// Dropping without calling [`Self::transition`] runs `clear_queued()`
/// — the "abandon peek" path is a clean reset rather than silent
/// stale-queued state.
pub struct PeekedQueue<'a> {
    mgr: Option<&'a mut QueueManager>,
    info: NextSongResult,
}

impl<'a> PeekedQueue<'a> {
    pub fn song(&self) -> &Song {
        &self.info.song
    }
    pub fn index(&self) -> usize {
        self.info.index
    }
    pub fn reason(&self) -> &str {
        &self.info.reason
    }
    pub fn info(&self) -> &NextSongResult {
        &self.info
    }

    /// Consume the peek and advance current_index/current_order.
    pub fn transition(mut self) -> TransitionResult {
        let mgr = self.mgr.take().expect("guard already consumed");
        mgr.transition_to_queued_internal()
            .expect("PeekedQueue invariant: queued is set when guard exists")
    }
}

impl Drop for PeekedQueue<'_> {
    fn drop(&mut self) {
        if let Some(mgr) = self.mgr.take() {
            mgr.clear_queued();
        }
    }
}

impl QueueManager {
    // ══════════════════════════════════════════════════════════════════════
    //  Song Navigation (Order Array Based)
    // ══════════════════════════════════════════════════════════════════════

    /// Peek at the next song WITHOUT updating current_index/current_order.
    /// Sets `queued` to the next order position if not already set, and returns
    /// a [`PeekedQueue`] guard that owns the only public path to `transition()`.
    /// Dropping the guard without calling `transition()` clears `queued`.
    /// Used for gapless/crossfade preparation.
    pub fn peek_next_song(&mut self) -> Option<PeekedQueue<'_>> {
        let info = self.compute_peek_next()?;
        Some(PeekedQueue {
            mgr: Some(self),
            info,
        })
    }

    fn compute_peek_next(&mut self) -> Option<NextSongResult> {
        if self.queue.song_ids.is_empty() || self.queue.order.is_empty() {
            return None;
        }

        // Mode Priority 1: Repeat Track. mpd consume-wins — a consuming queue
        // suppresses the replay so the gapless-prepared next track is the real
        // next song, letting the queue drain.
        if self.queue.repeat == RepeatMode::Track && !self.queue.consume {
            if let Some(idx) = self.queue.current_index
                && let Some(id) = self.queue.song_ids.get(idx)
                && let Some(song) = self.pool.get(id)
            {
                return Some(NextSongResult {
                    song: song.clone(),
                    index: idx,
                    reason: "repeat".to_string(),
                });
            }
            return None;
        }

        // If already queued, return that song
        if let Some(queued_order) = self.queue.queued {
            if queued_order < self.queue.order.len() {
                let song_idx = self.queue.order[queued_order];
                if let Some(id) = self.queue.song_ids.get(song_idx)
                    && let Some(song) = self.pool.get(id)
                {
                    return Some(NextSongResult {
                        song: song.clone(),
                        index: song_idx,
                        reason: if self.queue.shuffle {
                            "shuffle"
                        } else {
                            "next"
                        }
                        .to_string(),
                    });
                }
            }
            // Stale queued entry — clear and fall through
            self.queue.queued = None;
        }

        // Compute next from order array
        let next_order = match self.next_order_index() {
            Some(0) if self.queue.shuffle && !self.queue.consume => {
                // Shuffle + repeat-playlist wrap: reshuffle so each cycle is a fresh order.
                debug!(
                    " [QUEUE] Shuffle+repeat: reshuffling {} songs at playlist wrap",
                    self.queue.order.len()
                );
                self.shuffle_order();
                // After reshuffle, current is at position 0; next is position 1.
                // For single-song queues, shuffle_order is a no-op so clamp to 0.
                let next = self.queue.current_order.map_or(0, |c| c + 1);
                next.min(self.queue.order.len().saturating_sub(1))
            }
            Some(idx) => idx,
            None if self.queue.shuffle && self.queue.consume && self.queue.order.len() > 1 => {
                // Shuffle+consume: at end of shuffled order with unplayed songs remaining.
                // Reshuffle and continue from position 1 (current song stays at position 0).
                debug!(
                    " [QUEUE] Shuffle+consume: reshuffling {} remaining songs at end of order",
                    self.queue.order.len()
                );
                self.shuffle_order();
                self.queue.current_order.map_or(0, |c| c + 1)
            }
            None => return None,
        };
        let song_idx = self.queue.order[next_order];
        let id = self.queue.song_ids.get(song_idx)?;
        let song = self.pool.get(id)?;

        self.queue.queued = Some(next_order);
        debug!(
            " [QUEUE] Queued next: order[{}] → song_ids[{}] = {} (id: {})",
            next_order, song_idx, song.title, id
        );

        Some(NextSongResult {
            song: song.clone(),
            index: song_idx,
            reason: if self.queue.shuffle {
                "shuffle"
            } else if next_order == 0 {
                "repeatQueue"
            } else {
                "next"
            }
            .to_string(),
        })
    }

    /// Transition to the queued next song. Updates current_index and current_order.
    /// This is the SINGLE transition path — all automatic and manual transitions
    /// converge here (gapless, crossfade, normal end-of-track, manual skip).
    ///
    /// Crate-internal: the only public commit path is [`PeekedQueue::transition`].
    /// `get_next_song` reaches in directly to skip the guard ceremony for the
    /// peek-then-transition convenience.
    ///
    /// Returns `None` if no song is queued.
    pub(crate) fn transition_to_queued_internal(&mut self) -> Option<TransitionResult> {
        let queued_order = self.queue.queued.take()?;
        if queued_order >= self.queue.order.len() {
            return None;
        }

        let old_index = self.queue.current_index;
        let song_idx = self.queue.order[queued_order];
        let id = self.queue.song_ids.get(song_idx)?;
        let song = self.pool.get(id)?.clone();

        self.queue.current_order = Some(queued_order);
        self.queue.current_index = Some(song_idx);
        self.save_order().ok();

        debug!(
            " [QUEUE] Transitioned: order {} → song_ids[{}] = {} (old_index: {:?})",
            queued_order, song_idx, song.title, old_index
        );

        Some(TransitionResult {
            song,
            old_index,
            new_index: song_idx,
        })
    }

    /// Get next song: peek + transition in one call.
    /// Used by manual skip (play_next) and non-gapless auto-advance.
    /// Mode Priority (checked in order):
    /// 1. Repeat Mode: Replays current track (takes precedence)
    /// 2. Order Array: Next entry in play order (shuffled or sequential)
    /// 3. Repeat Playlist: Wraps to beginning
    ///
    /// Note: Consume mode is handled separately after playback starts
    pub fn get_next_song(&mut self) -> Option<NextSongResult> {
        debug!(
            " [QUEUE] get_next_song called, current_index: {:?}, current_order: {:?}, consume: {}, queue_length: {}",
            self.queue.current_index,
            self.queue.current_order,
            self.queue.consume,
            self.queue.song_ids.len()
        );

        // Trace: Log current queue state (very verbose with large queues)
        trace!(" [QUEUE] Current queue songs:");
        for (i, id) in self.queue.song_ids.iter().enumerate() {
            let marker = if Some(i) == self.queue.current_index {
                "▶️"
            } else {
                "  "
            };
            if let Some(song) = self.pool.get(id) {
                trace!(
                    " [QUEUE] {} [{}] {} - {} (id: {})",
                    marker, i, song.title, song.artist, song.id
                );
            }
        }

        if self.queue.song_ids.is_empty() {
            debug!(" [QUEUE] Queue is empty, returning None");
            return None;
        }

        // Bypass RepeatTrack for manual skip
        let was_repeat_track = self.queue.repeat == RepeatMode::Track;
        if was_repeat_track {
            self.queue.repeat = RepeatMode::None;
        }

        // Peek and (if Some) transition in one expression so the guard's
        // borrow on `self` ends before we touch `self.queue.repeat` again.
        let transition = self.peek_next_song().map(|p| p.transition());

        if was_repeat_track {
            self.queue.repeat = RepeatMode::Track;
            // The transition's save_order ran while repeat was the transient
            // None, so redb now holds repeat=None. Re-persist the restored
            // value so a manual skip can't silently drop the repeat-one
            // preference across a relaunch. Best-effort + idempotent.
            let _ = self.save_order();
        }

        let transition = transition?;

        Some(NextSongResult {
            song: transition.song,
            index: transition.new_index,
            reason: if self.queue.shuffle {
                "shuffle"
            } else {
                "next"
            }
            .to_string(),
        })
    }

    /// Get previous song using playback history or previous index
    pub fn get_previous_song(&mut self, _current_index: Option<usize>) -> PreviousSongResult {
        if self.queue.song_ids.is_empty() && self.playback_history.is_empty() {
            return PreviousSongResult::None;
        }

        // Try to use playback history first
        if let Some(prev) = self.playback_history.last() {
            let prev_id = prev.song.id.clone();
            let prev_eid = prev.entry_id;

            // Resolve the exact played row by entry_id first (lands on the row
            // that actually played, even with adjacent duplicate-id rows),
            // falling back to first-match-by-id when there is no row context or
            // the row was consumed/removed (entry_id no longer present).
            let found_idx = prev_eid
                .and_then(|eid| self.index_of_entry(eid))
                .or_else(|| self.queue.song_ids.iter().position(|id| *id == prev_id));

            // Pop after lookup (avoids double-borrow)
            let popped = self.playback_history.pop().expect("checked Some above");

            if let Some(idx) = found_idx {
                // Found in queue — navigate to it
                self.queue.current_index = Some(idx);
                self.sync_current_order_to_index();
                self.clear_queued();
                self.save_order().ok();
                let song = self.pool.get(&prev_id).cloned().unwrap_or(popped.song);
                return PreviousSongResult::InQueue(song, idx);
            }

            // Song not in queue (consumed/removed) — return for re-insertion
            return PreviousSongResult::Removed(popped.song);
        }

        // Order-aware backward step (no history available). Mirrors the
        // forward `next_order_index` walk: compute the previous ORDER
        // position, map through `order[prev]` to the physical index, and set
        // BOTH `current_index` and `current_order`. Reduces to physical
        // idx-1 / wrap-to-last when shuffle is off (identity order). Setting
        // both fields directly (rather than via `sync_current_order_to_index`)
        // keeps the invariant `order[current_order] == current_index` exact
        // even with duplicate physical ids, where a first-match sync could
        // re-derive the wrong order slot.
        if let Some(prev_order) = self.prev_order_index()
            && let Some(&phys) = self.queue.order.get(prev_order)
            && let Some(song) = self
                .queue
                .song_ids
                .get(phys)
                .and_then(|id| self.pool.get(id))
                .cloned()
        {
            self.queue.current_index = Some(phys);
            self.queue.current_order = Some(prev_order);
            self.clear_queued();
            self.save_order().ok();
            return PreviousSongResult::InQueue(song, phys);
        }

        PreviousSongResult::None
    }

    /// Add a song to playback history, keyed by the per-row `entry_id` when
    /// available.
    ///
    /// Repeat-track guard: skips when this push would duplicate the last entry.
    /// With a known `entry_id`, "duplicate" means the SAME physical row replayed
    /// (same `entry_id`), so two distinct rows of the same song id are both
    /// recorded. Without row context (`entry_id == None`), it falls back to the
    /// legacy `Song.id` comparison so the repeat-track behavior is preserved.
    pub fn add_to_history(&mut self, song: Song, entry_id: Option<u64>) {
        let is_duplicate = match entry_id {
            Some(eid) => self.playback_history.last().and_then(|h| h.entry_id) == Some(eid),
            None => self.playback_history.last().map(|h| &h.song.id) == Some(&song.id),
        };
        if is_duplicate {
            return;
        }
        self.playback_history.push(HistoryEntry { entry_id, song });
        if self.playback_history.len() > self.max_history_size {
            self.playback_history.remove(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        services::queue::tests::{make_test_manager, make_test_song},
        types::queue::RepeatMode,
    };

    // ── peek_next_song tests ──

    #[test]
    fn peek_does_not_advance_current_index() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

        let peeked = qm.peek_next_song().unwrap();
        assert_eq!(peeked.index(), 1);
        assert_eq!(peeked.song().id, "b");
        drop(peeked);
        // current_index must NOT have changed
        assert_eq!(qm.queue.current_index, Some(0));
        assert_eq!(qm.queue.current_order, Some(0));
    }

    #[test]
    fn peek_repeat_track_returns_current() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(1));
        let _ = qm.set_repeat(RepeatMode::Track).unwrap();

        let peeked = qm.peek_next_song().unwrap();
        assert_eq!(peeked.index(), 1);
        assert_eq!(peeked.song().id, "b");
        assert_eq!(peeked.reason(), "repeat");
    }

    #[test]
    fn peek_at_end_no_repeat_returns_none() {
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let (mut qm, _temp) = make_test_manager(songs, Some(1)); // at last song

        let peeked = qm.peek_next_song();
        assert!(peeked.is_none());
    }

    #[test]
    fn peek_at_end_repeat_playlist_wraps() {
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let (mut qm, _temp) = make_test_manager(songs, Some(1)); // at last song
        let _ = qm.set_repeat(RepeatMode::Playlist).unwrap();

        let peeked = qm.peek_next_song().unwrap();
        assert_eq!(peeked.index(), 0);
        assert_eq!(peeked.song().id, "a");
        assert_eq!(peeked.reason(), "repeatQueue");
    }

    #[test]
    fn peek_then_transition_matches_get_next() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

        // Peek first
        let peeked = qm.peek_next_song().unwrap();
        assert_eq!(peeked.song().id, "b");

        // Then transition (consumes the guard)
        let result = peeked.transition();
        assert_eq!(result.new_index, 1);
        assert_eq!(result.old_index, Some(0));
        assert_eq!(qm.queue.current_index, Some(1));
    }

    // ── get_next_song tests ──

    #[test]
    fn get_next_sequential_advance() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
            make_test_song("d"),
            make_test_song("e"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

        // Advance through all songs sequentially
        for expected_idx in 1..5 {
            let next = qm.get_next_song().unwrap();
            assert_eq!(
                next.index, expected_idx,
                "Expected index {expected_idx}, got {}",
                next.index
            );
        }
        assert_eq!(qm.queue.current_index, Some(4));
    }

    #[test]
    fn get_next_at_end_no_repeat_returns_none() {
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let (mut qm, _temp) = make_test_manager(songs, Some(1));

        let next = qm.get_next_song();
        assert!(next.is_none());
        // current_index unchanged
        assert_eq!(qm.queue.current_index, Some(1));
    }

    #[test]
    fn get_next_at_end_repeat_playlist_wraps() {
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let (mut qm, _temp) = make_test_manager(songs, Some(1));
        let _ = qm.set_repeat(RepeatMode::Playlist).unwrap();

        let next = qm.get_next_song().unwrap();
        assert_eq!(next.index, 0);
        assert_eq!(next.song.id, "a");
        assert_eq!(qm.queue.current_index, Some(0));
    }

    #[test]
    fn get_next_repeat_track_bypasses_repeat() {
        // Manual skip (get_next_song) should bypass Repeat Track and advance to the next song.
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(1));
        let _ = qm.set_repeat(RepeatMode::Track).unwrap();

        let next = qm.get_next_song().unwrap();
        assert_eq!(next.index, 2);
        assert_eq!(next.song.id, "c");
        assert_eq!(next.reason, "next");

        // Ensure repeat mode remains active
        assert_eq!(qm.queue.repeat, RepeatMode::Track);
        // current_index should be advanced
        assert_eq!(qm.queue.current_index, Some(2));
    }

    #[test]
    fn get_next_repeat_track_persists_repeat_across_reload() {
        use crate::services::{queue::QueueManager, state_storage::StateStorage};

        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));
        // The cloned StateStorage shares the underlying redb so a fresh
        // QueueManager::new reads back exactly what this manager persisted.
        let storage: StateStorage = qm.storage.clone();

        // Persist repeat=Track via the write guard.
        let _ = qm.set_repeat(RepeatMode::Track).unwrap();

        // Manual skip once — the bypass transiently sets repeat=None and
        // save_order fires while it is None.
        let _ = qm.get_next_song();

        // Reconstruct on the SAME storage: the reloaded repeat must be Track.
        let qm2 = QueueManager::new(storage).expect("reload");
        assert_eq!(
            qm2.get_queue().repeat,
            RepeatMode::Track,
            "repeat-one preference must survive a manual skip + relaunch",
        );
    }

    #[test]
    fn get_next_empty_queue_returns_none() {
        let (mut qm, _temp) = make_test_manager(vec![], None);
        assert!(qm.get_next_song().is_none());
    }

    // ── get_previous_song tests ──

    #[test]
    fn previous_from_history() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(2));

        // Add "a" to history (simulating having played it)
        let e0 = qm.entry_id_at(0);
        qm.add_to_history(make_test_song("a"), e0);

        let result = qm.get_previous_song(Some(2));
        match result {
            PreviousSongResult::InQueue(song, idx) => {
                assert_eq!(song.id, "a");
                assert_eq!(idx, 0);
            }
            other => panic!("Expected InQueue, got {other:?}"),
        }
    }

    #[test]
    fn previous_fallback_to_index() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(2));
        // No history — should fall back to index-1

        let result = qm.get_previous_song(Some(2));
        match result {
            PreviousSongResult::InQueue(song, idx) => {
                assert_eq!(song.id, "b");
                assert_eq!(idx, 1);
            }
            other => panic!("Expected InQueue, got {other:?}"),
        }
    }

    #[test]
    fn previous_removed_song_from_history() {
        let songs = vec![make_test_song("b"), make_test_song("c")];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

        // Add "x" (not in queue) to history — no row context.
        qm.add_to_history(make_test_song("x"), None);

        let result = qm.get_previous_song(Some(0));
        match result {
            PreviousSongResult::Removed(song) => {
                assert_eq!(song.id, "x");
            }
            other => panic!("Expected Removed, got {other:?}"),
        }
    }

    #[test]
    fn previous_no_history_at_start_returns_none() {
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));
        // No history, at index 0 — nowhere to go

        let result = qm.get_previous_song(Some(0));
        assert!(matches!(result, PreviousSongResult::None));
    }

    #[test]
    fn previous_at_head_repeat_playlist_wraps_to_last() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));
        let _ = qm.set_repeat(RepeatMode::Playlist).unwrap();
        // No shuffle / consume / history.

        let result = qm.get_previous_song(Some(0));
        match result {
            PreviousSongResult::InQueue(song, idx) => {
                assert_eq!(song.id, "c");
                assert_eq!(idx, 2);
            }
            other => panic!("Expected InQueue wrap to last, got {other:?}"),
        }
        assert_eq!(qm.queue.current_index, Some(2));
    }

    #[test]
    fn previous_at_head_repeat_playlist_consume_returns_none() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));
        let _ = qm.set_repeat(RepeatMode::Playlist).unwrap();
        qm.queue.consume = true;

        let result = qm.get_previous_song(Some(0));
        assert!(matches!(result, PreviousSongResult::None));
        assert_eq!(qm.queue.current_index, Some(0));
    }

    #[test]
    fn previous_under_shuffle_empty_history_walks_order_mid() {
        // Restored shuffled queue with EMPTY history (history is never
        // persisted). order=[2,0,3,1] is a permutation; playing d at
        // physical index 3 = order position 2. The true play-order
        // predecessor is order[1]=0=physical a, NOT the physical neighbor c.
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
            make_test_song("d"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(3));
        qm.queue.shuffle = true;
        qm.queue.order = vec![2, 0, 3, 1];
        qm.queue.current_index = Some(3);
        qm.queue.current_order = Some(2);
        qm.queue.repeat = RepeatMode::None;

        let result = qm.get_previous_song(Some(3));
        match result {
            PreviousSongResult::InQueue(song, idx) => {
                assert_eq!(song.id, "a");
                assert_eq!(idx, 0);
            }
            other => panic!("Expected InQueue(a, 0), got {other:?}"),
        }
        // Invariant order[current_order] == current_index holds: order[1]==0.
        assert_eq!(qm.queue.current_index, Some(0));
        assert_eq!(qm.queue.current_order, Some(1));
    }

    #[test]
    fn previous_under_shuffle_empty_history_wraps_order_head() {
        // Restored shuffled queue, empty history, current at PLAY-ORDER head
        // with repeat Playlist. order=[3,0,2,1] so the physical-neighbor
        // answer (idx-1) and the order-wrap answer DIFFER: playing physical
        // index 3 = order[0]; the physical neighbor is song_ids[2]=c, but the
        // order-wrap predecessor is order[len-1]=order[3]=1=physical b.
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
            make_test_song("d"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(3));
        qm.queue.shuffle = true;
        qm.queue.order = vec![3, 0, 2, 1];
        qm.queue.current_index = Some(3);
        qm.queue.current_order = Some(0);
        qm.queue.repeat = RepeatMode::Playlist;
        qm.queue.consume = false;

        let result = qm.get_previous_song(Some(3));
        match result {
            PreviousSongResult::InQueue(song, idx) => {
                assert_eq!(song.id, "b");
                assert_eq!(idx, 1);
            }
            other => panic!("Expected InQueue(b, 1), got {other:?}"),
        }
        assert_eq!(qm.queue.current_index, Some(1));
        assert_eq!(qm.queue.current_order, Some(3));
    }

    #[test]
    fn previous_under_shuffle_empty_history_head_no_repeat_returns_none() {
        // Restored shuffled queue at play-order head, repeat None, empty
        // history. order=[2,0,3,1]; current physical = order[0]=2 (playing c).
        // With no order predecessor and no repeat-wrap, Previous must return
        // None and leave current_index untouched — NOT surface the physical
        // neighbor b.
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
            make_test_song("d"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(2));
        qm.queue.shuffle = true;
        qm.queue.order = vec![2, 0, 3, 1];
        qm.queue.current_index = Some(2);
        qm.queue.current_order = Some(0);
        qm.queue.repeat = RepeatMode::None;
        qm.queue.consume = false;

        let result = qm.get_previous_song(Some(2));
        assert!(matches!(result, PreviousSongResult::None));
        assert_eq!(qm.queue.current_index, Some(2));
    }

    // ── History tests ──

    #[test]
    fn history_repeat_guard_skips_duplicates() {
        let songs = vec![make_test_song("a")];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

        // Same physical row replayed three times (same entry_id) — the
        // repeat-track guard collapses them to a single history entry.
        qm.add_to_history(make_test_song("x"), Some(7));
        qm.add_to_history(make_test_song("x"), Some(7)); // duplicate — skipped
        qm.add_to_history(make_test_song("x"), Some(7)); // duplicate — skipped

        // Only one entry should exist
        assert_eq!(qm.playback_history.len(), 1);
    }

    #[test]
    fn history_max_size_cap() {
        let songs = vec![make_test_song("a")];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));

        // Add more than max_history_size entries (distinct entry_ids).
        for i in 0..150 {
            qm.add_to_history(make_test_song(&format!("h{i}")), Some(i as u64));
        }

        assert!(qm.playback_history.len() <= qm.max_history_size);
        // Most recent should be the last one added
        assert_eq!(qm.playback_history.last().unwrap().song.id, "h149");
    }

    #[test]
    fn previous_adjacent_duplicate_rows_lands_on_played_row() {
        // Two adjacent physical rows of the same id `a`, then `b`.
        // song_ids=[a,a,b], entry_ids=[e0,e1,e2]. The history must key on the
        // per-row entry_id so Previous from `b` lands on the SECOND `a` (row 1,
        // the row that actually played before b), not the first physical `a`.
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));
        qm.replace_song_ids_for_test(vec!["a".into(), "a".into(), "b".into()], Some(0));

        let e0 = qm.entry_id_at(0);
        let e1 = qm.entry_id_at(1);
        // Auto-advance simulation: record each LEFT row with its real entry_id.
        qm.add_to_history(make_test_song("a"), e0); // first a, row 0
        qm.add_to_history(make_test_song("a"), e1); // second a, row 1
        qm.queue.current_index = Some(2); // now playing b
        qm.sync_current_order_to_index();

        let result = qm.get_previous_song(Some(2));
        match result {
            PreviousSongResult::InQueue(song, idx) => {
                assert_eq!(song.id, "a");
                assert_eq!(idx, 1, "Previous must land on the SECOND a's row");
            }
            other => panic!("Expected InQueue(a, 1), got {other:?}"),
        }
        assert_eq!(qm.queue.current_index, Some(1));
    }

    #[test]
    fn history_dedup_distinguishes_distinct_rows_same_id() {
        // song_ids=[a,a,b], entry_ids=[e0,e1,e2]. Two distinct rows of id `a`
        // are recorded as two history entries; a same-row replay (same
        // entry_id) is still suppressed by the entry_id repeat-track guard.
        let songs = vec![make_test_song("a"), make_test_song("b")];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));
        qm.replace_song_ids_for_test(vec!["a".into(), "a".into(), "b".into()], Some(0));

        let e0 = qm.entry_id_at(0);
        let e1 = qm.entry_id_at(1);
        qm.add_to_history(make_test_song("a"), e0); // row 0
        qm.add_to_history(make_test_song("a"), e1); // row 1 (distinct entry_id)
        assert_eq!(
            qm.playback_history.len(),
            2,
            "two distinct rows of the same id must both be recorded"
        );

        // Same physical row replayed (same entry_id) → suppressed.
        qm.add_to_history(make_test_song("a"), e1);
        assert_eq!(
            qm.playback_history.len(),
            2,
            "same-row replay must be suppressed by the entry_id guard"
        );
    }

    // ── Shuffle + Consume tests ──

    #[test]
    fn next_shuffle_consume_reshuffles_at_end() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));
        qm.queue.shuffle = true;
        qm.queue.consume = true;
        // Force current_order to end of the shuffled order
        qm.queue.current_order = Some(qm.queue.order.len() - 1);

        // Should reshuffle and find a next song (not return None)
        let next = qm.get_next_song();
        assert!(next.is_some(), "Expected Some (reshuffle), got None");

        // Verify we transitioned to a valid song
        let result = next.unwrap();
        assert!(
            ["a", "b", "c"].contains(&result.song.id.as_str()),
            "Got unexpected song id: {}",
            result.song.id
        );
    }

    #[test]
    fn next_shuffle_consume_single_song_returns_none() {
        let songs = vec![make_test_song("a")];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));
        qm.queue.shuffle = true;
        qm.queue.consume = true;

        // Only 1 song — no reshuffle possible, should return None
        let next = qm.get_next_song();
        assert!(next.is_none());
    }

    #[test]
    fn next_shuffle_no_consume_at_end_returns_none() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));
        qm.queue.shuffle = true;
        qm.queue.consume = false;
        qm.queue.current_order = Some(qm.queue.order.len() - 1);

        // Shuffle ON, no repeat — should return None at end
        let next = qm.get_next_song();
        assert!(next.is_none());
    }

    // ── Shuffle + Repeat Playlist tests ──

    #[test]
    fn next_shuffle_repeat_playlist_reshuffles_on_wrap() {
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
            make_test_song("d"),
            make_test_song("e"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));
        qm.queue.shuffle = true;
        let _ = qm.set_repeat(RepeatMode::Playlist).unwrap();
        qm.shuffle_order();

        // Record the initial shuffle order
        let initial_order = qm.queue.order.clone();

        // Advance to the last song in the order
        qm.queue.current_order = Some(qm.queue.order.len() - 1);
        qm.queue.current_index = Some(qm.queue.order[qm.queue.order.len() - 1]);

        // Next should succeed (repeat-playlist wrap) and reshuffle
        let next = qm.get_next_song();
        assert!(next.is_some(), "Expected Some (reshuffle wrap), got None");

        // The order should have been reshuffled (with 5 songs, extremely unlikely to be identical)
        let new_order = qm.queue.order.clone();
        // Both orders should contain the same elements (just reordered)
        let mut sorted_initial = initial_order.clone();
        let mut sorted_new = new_order.clone();
        sorted_initial.sort();
        sorted_new.sort();
        assert_eq!(
            sorted_initial, sorted_new,
            "Order arrays should contain the same indices"
        );
    }

    #[test]
    fn next_shuffle_repeat_playlist_single_song() {
        let songs = vec![make_test_song("a")];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));
        qm.queue.shuffle = true;
        let _ = qm.set_repeat(RepeatMode::Playlist).unwrap();

        // Single song + shuffle + repeat-playlist — should wrap and return the same song
        let next = qm.get_next_song();
        assert!(
            next.is_some(),
            "Single-song repeat-playlist should still work"
        );
        assert_eq!(next.unwrap().song.id, "a");
    }

    // ══════════════════════════════════════════════════════════════════════
    //  Mode Combination Matrix (Hand-Written Exhaustive)
    // ══════════════════════════════════════════════════════════════════════

    /// Helper: configure a QueueManager with the given mode flags
    fn with_modes(shuffle: bool, consume: bool, repeat: RepeatMode) -> QueueManager {
        let songs: Vec<_> = (0..10).map(|i| make_test_song(&i.to_string())).collect();
        let (mut qm, _temp) = make_test_manager(songs, Some(0));
        if shuffle {
            let _ = qm.toggle_shuffle().unwrap();
        }
        qm.queue.consume = consume;
        let _ = qm.set_repeat(repeat).unwrap();
        qm
    }

    /// Assert invariants that must hold after ANY navigation operation:
    /// - order array indices are within bounds of song_ids
    /// - current_order points to a valid order entry
    /// - current_index matches order[current_order]
    fn assert_navigation_invariants(qm: &QueueManager, label: &str) {
        let len = qm.queue.song_ids.len();
        // Order array entries must be in [0, len)
        for (pos, &idx) in qm.queue.order.iter().enumerate() {
            assert!(
                idx < len,
                "{label}: order[{pos}]={idx} >= song_ids.len()={len}"
            );
        }
        // Order must be a permutation (no duplicates)
        {
            let mut sorted = qm.queue.order.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(
                sorted.len(),
                qm.queue.order.len(),
                "{label}: order has duplicates"
            );
        }
        // current_order must be valid
        if let Some(co) = qm.queue.current_order {
            assert!(
                co < qm.queue.order.len(),
                "{label}: current_order={co} >= order.len()={}",
                qm.queue.order.len()
            );
            // current_index must match order[current_order]
            if let Some(ci) = qm.queue.current_index {
                assert_eq!(
                    ci, qm.queue.order[co],
                    "{label}: current_index {ci} != order[{co}]={}",
                    qm.queue.order[co]
                );
            }
        }
    }

    #[test]
    fn matrix_sequential_no_consume_no_repeat() {
        let mut qm = with_modes(false, false, RepeatMode::None);
        // Advance through all 10 songs
        for _ in 0..9 {
            let next = qm.get_next_song();
            assert!(next.is_some());
            assert_navigation_invariants(&qm, "seq/nc/nr");
        }
        // 11th call should return None
        assert!(qm.get_next_song().is_none());
    }

    #[test]
    fn matrix_sequential_no_consume_repeat_playlist() {
        let mut qm = with_modes(false, false, RepeatMode::Playlist);
        // Advance 20 times — should loop twice
        for i in 0..20 {
            let next = qm.get_next_song();
            assert!(next.is_some(), "expected song at step {i}");
            assert_navigation_invariants(&qm, "seq/nc/rp");
        }
    }

    #[test]
    fn matrix_sequential_no_consume_repeat_track() {
        let mut qm = with_modes(false, false, RepeatMode::Track);
        // peek_next_song with repeat=Track always returns current
        let peeked = qm.peek_next_song().unwrap();
        assert_eq!(peeked.index(), 0);
        assert_eq!(peeked.reason(), "repeat");
        drop(peeked);

        // get_next_song bypasses repeat-track and advances
        let next = qm.get_next_song().unwrap();
        assert_eq!(next.index, 1);
        assert_navigation_invariants(&qm, "seq/nc/rt");
    }

    #[test]
    fn matrix_sequential_consume_no_repeat() {
        // Consume mode is handled externally (by the playback controller),
        // so get_next_song doesn't itself remove songs. However, the order
        // array must still be valid after advancing.
        let mut qm = with_modes(false, true, RepeatMode::None);
        for _ in 0..9 {
            let next = qm.get_next_song();
            assert!(next.is_some());
            assert_navigation_invariants(&qm, "seq/c/nr");
        }
        assert!(qm.get_next_song().is_none());
    }

    #[test]
    fn matrix_shuffle_no_consume_no_repeat() {
        let mut qm = with_modes(true, false, RepeatMode::None);
        let mut visited = std::collections::HashSet::new();
        for _ in 0..9 {
            let next = qm.get_next_song();
            assert!(next.is_some());
            visited.insert(next.unwrap().song.id.clone());
            assert_navigation_invariants(&qm, "shuf/nc/nr");
        }
        // Should visit all 9 remaining songs (started at 0, advanced 9 times)
        assert_eq!(visited.len(), 9);
        assert!(qm.get_next_song().is_none());
    }

    #[test]
    fn matrix_shuffle_no_consume_repeat_playlist() {
        let mut qm = with_modes(true, false, RepeatMode::Playlist);
        // Should loop forever — advance 25 times
        for i in 0..25 {
            let next = qm.get_next_song();
            assert!(next.is_some(), "expected song at step {i}");
            assert_navigation_invariants(&qm, "shuf/nc/rp");
        }
    }

    #[test]
    fn matrix_shuffle_consume_no_repeat() {
        // Consume mode removes songs EXTERNALLY (via playback controller
        // calling consume_current after playback starts). get_next_song
        // itself doesn't remove songs. With shuffle+consume+no-repeat,
        // the reshuffle fallback fires at end-of-order since songs still
        // exist — this is correct behavior (consume removes them later).
        //
        // Test that the core invariants hold throughout.
        let mut qm = with_modes(true, true, RepeatMode::None);
        for _ in 0..15 {
            if qm.get_next_song().is_none() {
                break;
            }
            assert_navigation_invariants(&qm, "shuf/c/nr");
        }
    }

    #[test]
    fn matrix_shuffle_consume_repeat_playlist() {
        let mut qm = with_modes(true, true, RepeatMode::Playlist);
        for i in 0..20 {
            let next = qm.get_next_song();
            assert!(next.is_some(), "expected song at step {i}");
            assert_navigation_invariants(&qm, "shuf/c/rp");
        }
    }

    // ── Previous song with different modes ──

    #[test]
    fn previous_with_shuffle_uses_history() {
        let mut qm = with_modes(true, false, RepeatMode::None);
        // Start playing, advance a few times
        let e0 = qm.entry_id_at(0);
        qm.add_to_history(make_test_song("0"), e0);
        let next = qm.get_next_song().unwrap();
        let next_eid = qm.entry_id_at(next.index);
        qm.add_to_history(next.song, next_eid);

        // Previous should go back to history
        let prev = qm.get_previous_song(qm.queue.current_index);
        match prev {
            PreviousSongResult::InQueue(_, _) => {}
            other => {
                panic!("Expected InQueue, got {other:?}")
            }
        }
        assert_navigation_invariants(&qm, "prev/shuf");
    }

    #[test]
    fn previous_with_consume_returns_removed() {
        // Simulate consume mode: playing "b", "a" was consumed (removed)
        let songs = vec![make_test_song("b"), make_test_song("c")];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));
        qm.queue.consume = true;

        // Add consumed song "a" to history — no longer in queue, no row context.
        qm.add_to_history(make_test_song("a"), None);

        let prev = qm.get_previous_song(Some(0));
        match prev {
            PreviousSongResult::Removed(song) => {
                assert_eq!(song.id, "a");
            }
            other => panic!("Expected Removed, got {other:?}"),
        }
    }

    // ── Consume + Repeat-Playlist interaction tests ──
    // Bug: crossfade + consume + repeat-queue wraps to position 0 even though
    // consume will empty the queue, causing a ghost track to play from an empty queue.

    #[test]
    fn consume_repeat_playlist_last_song_returns_none() {
        // The exact bug scenario: 1 song left, consume + repeat-playlist.
        // peek_next_song must return None — there's nothing to wrap to.
        let songs = vec![make_test_song("a")];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));
        qm.queue.consume = true;
        let _ = qm.set_repeat(RepeatMode::Playlist).unwrap();

        let peeked = qm.peek_next_song();
        assert!(
            peeked.is_none(),
            "consume + repeat-playlist with 1 song should NOT wrap (got {:?})",
            peeked.map(|r| r.song().id.clone())
        );
    }

    #[test]
    fn consume_repeat_playlist_sequential_no_wrap() {
        // Sequential consume + repeat-playlist: drain all songs, should stop.
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));
        qm.queue.consume = true;
        let _ = qm.set_repeat(RepeatMode::Playlist).unwrap();

        // Advance through all songs
        let mut count = 0;
        while qm.get_next_song().is_some() {
            count += 1;
            if count > 5 {
                panic!("consume + repeat-playlist should terminate, not loop");
            }
        }
        // Should have advanced exactly 2 times (b, c) then stopped
        assert_eq!(count, 2, "expected 2 advances (b, c) then None");
    }

    #[test]
    fn consume_shuffle_repeat_playlist_last_song_returns_none() {
        // Shuffle + consume + repeat-playlist with 1 song: must return None.
        let songs = vec![make_test_song("a")];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));
        qm.queue.shuffle = true;
        qm.queue.consume = true;
        let _ = qm.set_repeat(RepeatMode::Playlist).unwrap();

        let peeked = qm.peek_next_song();
        assert!(
            peeked.is_none(),
            "shuffle + consume + repeat-playlist with 1 song should NOT wrap (got {:?})",
            peeked.map(|r| r.song().id.clone())
        );
    }

    #[test]
    fn consume_repeat_track_peek_advances_not_replays() {
        // mpd consume-wins: consume + repeat-Track must drain the queue
        // (advance), not replay the current song forever.
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));
        qm.queue.consume = true;
        let _ = qm.set_repeat(RepeatMode::Track).unwrap();

        let peeked = qm.peek_next_song().unwrap();
        assert_eq!(peeked.index(), 1, "should advance to next song, not replay");
        assert_ne!(
            peeked.reason(),
            "repeat",
            "consume must suppress the repeat-track replay",
        );
    }

    #[test]
    fn consume_repeat_playlist_with_remaining_advances() {
        // Consume + repeat-playlist with songs remaining DOES advance.
        // The fix must not break normal mid-queue consume behavior.
        let songs = vec![
            make_test_song("a"),
            make_test_song("b"),
            make_test_song("c"),
        ];
        let (mut qm, _temp) = make_test_manager(songs, Some(0));
        qm.queue.consume = true;
        let _ = qm.set_repeat(RepeatMode::Playlist).unwrap();

        let next = qm.peek_next_song().unwrap();
        assert_eq!(next.song().id, "b", "should advance to next song mid-queue");
    }
}

// ══════════════════════════════════════════════════════════════════════
//  Property-Based Tests (proptest)
// ══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod proptest_navigation {
    use proptest::prelude::*;

    use crate::{
        services::queue::tests::{make_test_manager, make_test_song},
        types::queue::RepeatMode,
    };

    fn repeat_mode_strategy() -> impl Strategy<Value = RepeatMode> {
        prop_oneof![
            Just(RepeatMode::None),
            Just(RepeatMode::Track),
            Just(RepeatMode::Playlist),
        ]
    }

    /// Generate a random queue of N songs with random mode flags
    fn queue_setup(
        n: usize,
        shuffle: bool,
        consume: bool,
        repeat: RepeatMode,
        start_idx: usize,
    ) -> super::QueueManager {
        let songs: Vec<_> = (0..n).map(|i| make_test_song(&i.to_string())).collect();
        let start = if n > 0 { start_idx % n } else { 0 };
        let (mut qm, _temp) = make_test_manager(songs, if n > 0 { Some(start) } else { None });
        if shuffle {
            let _ = qm.toggle_shuffle().unwrap();
        }
        qm.queue.consume = consume;
        let _ = qm.set_repeat(repeat).unwrap();
        qm
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(500))]

        /// Invariant: after any number of get_next_song calls, the order
        /// array must contain valid indices into song_ids.
        #[test]
        fn order_array_always_valid(
            n in 1usize..30,
            shuffle in any::<bool>(),
            consume in any::<bool>(),
            repeat in repeat_mode_strategy(),
            start in 0usize..30,
            advances in 1usize..50,
        ) {
            let mut qm = queue_setup(n, shuffle, consume, repeat, start);
            let max_advances = advances.min(100); // cap for sanity

            for _ in 0..max_advances {
                if qm.get_next_song().is_none() {
                    break;
                }
                // Core invariant: order indices are in bounds
                for (pos, &idx) in qm.queue.order.iter().enumerate() {
                    prop_assert!(
                        idx < qm.queue.song_ids.len(),
                        "order[{}]={} >= len={} (n={}, shuf={}, consume={}, repeat={:?})",
                        pos, idx, qm.queue.song_ids.len(), n, shuffle, consume, repeat
                    );
                }

                // Order must be a permutation
                let mut sorted = qm.queue.order.clone();
                sorted.sort();
                sorted.dedup();
                prop_assert_eq!(
                    sorted.len(),
                    qm.queue.order.len(),
                    "order has duplicates after advance"
                );
            }
        }

        /// Invariant: current_index always points to a valid song.
        #[test]
        fn current_index_always_valid(
            n in 1usize..20,
            shuffle in any::<bool>(),
            consume in any::<bool>(),
            repeat in repeat_mode_strategy(),
            start in 0usize..20,
            advances in 1usize..30,
        ) {
            let mut qm = queue_setup(n, shuffle, consume, repeat, start);

            for _ in 0..advances {
                if qm.get_next_song().is_none() {
                    break;
                }
                if let Some(ci) = qm.queue.current_index {
                    prop_assert!(
                        ci < qm.queue.song_ids.len(),
                        "current_index={} >= len={} (n={}, shuf={}, consume={}, repeat={:?})",
                        ci, qm.queue.song_ids.len(), n, shuffle, consume, repeat
                    );
                }
            }
        }

        /// Invariant: sequential (non-shuffle) traversal without repeat
        /// always terminates after exactly n-1 advances.
        #[test]
        fn sequential_no_repeat_terminates(
            n in 2usize..30,
            start in 0usize..30,
        ) {
            let songs: Vec<_> = (0..n).map(|i| make_test_song(&i.to_string())).collect();
            let s = start % n;
            let (mut qm, _temp) = make_test_manager(songs, Some(s));
            // Sequential, no consume, no repeat
            qm.queue.consume = false;
            let _ = qm.set_repeat(RepeatMode::None).unwrap();

            let remaining = n - s - 1; // songs after start
            let mut count = 0;
            while qm.get_next_song().is_some() {
                count += 1;
                if count > n + 5 {
                    prop_assert!(false, "did not terminate: n={}, start={}", n, s);
                }
            }
            prop_assert_eq!(
                count, remaining,
                "expected {} advances from start={} in queue of {}", remaining, s, n
            );
        }

        /// Invariant: peek_next_song is idempotent — calling it twice
        /// returns the same result without side effects.
        #[test]
        fn peek_is_idempotent(
            n in 1usize..20,
            shuffle in any::<bool>(),
            repeat in repeat_mode_strategy(),
            start in 0usize..20,
        ) {
            let mut qm = queue_setup(n, shuffle, false, repeat, start);

            let peek1 = qm.peek_next_song().map(|r| (r.index(), r.song().id.clone()));
            let peek2 = qm.peek_next_song().map(|r| (r.index(), r.song().id.clone()));

            prop_assert_eq!(
                peek1, peek2,
                "peek_next_song not idempotent (n={}, shuffle={}, repeat={:?})", n, shuffle, repeat
            );
        }

        /// Invariant: shuffle traversal visits all songs exactly once
        /// (no repeat, no consume).
        #[test]
        fn shuffle_visits_all_once(n in 2usize..20) {
            let songs: Vec<_> = (0..n).map(|i| make_test_song(&i.to_string())).collect();
            let (mut qm, _temp) = make_test_manager(songs, Some(0));
            let _ = qm.toggle_shuffle().unwrap();
            qm.queue.consume = false;
            let _ = qm.set_repeat(RepeatMode::None).unwrap();

            let mut visited = std::collections::HashSet::new();
            visited.insert("0".to_string()); // starting song
            let mut count = 0;
            while let Some(next) = qm.get_next_song() {
                visited.insert(next.song.id.clone());
                count += 1;
                if count > n + 5 {
                    prop_assert!(false, "shuffle did not terminate: n={}", n);
                }
            }
            prop_assert_eq!(
                visited.len(), n,
                "shuffle didn't visit all songs: {:?}", visited
            );
        }

        /// QUEUE-1 regression: removing the currently-playing row under a
        /// shuffled order must reach every still-upcoming survivor exactly
        /// once before stopping, with no replay. The upcoming survivors are
        /// the song ids at order[current_order+1..] (captured before removal);
        /// after removing the current row the derive fix makes the next
        /// upcoming survivor the new current, so draining via get_next_song
        /// must reproduce exactly that upcoming set, in order, with no repeats.
        #[test]
        fn remove_current_under_shuffle_reaches_all_upcoming(
            n in 2usize..8,
            co_pick in 0usize..8,
        ) {
            let songs: Vec<_> = (0..n).map(|i| make_test_song(&i.to_string())).collect();
            let (mut qm, _temp) = make_test_manager(songs, Some(0));
            let _ = qm.toggle_shuffle().unwrap();
            qm.queue.consume = false;
            let _ = qm.set_repeat(RepeatMode::None).unwrap();

            // Pick a random valid current_order and re-anchor the invariant
            // (toggle_shuffle keeps song 0 anchored at current_order=0; choose
            // a fresh position and sync current_index to order[co]).
            let co = co_pick % n;
            qm.queue.current_order = Some(co);
            qm.queue.current_index = Some(qm.queue.order[co]);
            qm.queue.queued = None;

            // Capture the play-order tail strictly after current_order BEFORE
            // removal: these genuinely-upcoming survivors must all be reached.
            let upcoming_before: Vec<String> = qm.queue.order[co + 1..]
                .iter()
                .map(|&phys| qm.queue.song_ids[phys].clone())
                .collect();

            // Remove the currently-playing physical row.
            let cur_phys = qm.queue.current_index.unwrap();
            let _ = qm.remove_song(cur_phys).unwrap();

            // Invariant restored after removal.
            let (post_co, post_ci) = (qm.queue.current_order, qm.queue.current_index);
            if let (Some(c_ord), Some(c_idx)) = (post_co, post_ci) {
                prop_assert_eq!(
                    qm.queue.order[c_ord], c_idx,
                    "invariant order[co]==ci broken after remove (n={}, co={})", n, co
                );
            }

            // The true reachable set is the post-removal play-order tail from
            // the new current_order: order[post_co..] mapped to song ids. The
            // derive fix makes the next-upcoming survivor (or, in the last-slot
            // case, the survivor that slid into the slot) the new current, so
            // draining current + get_next_song* must reproduce exactly this.
            let expected: Vec<String> = match post_co {
                Some(c) => qm.queue.order[c..]
                    .iter()
                    .map(|&phys| qm.queue.song_ids[phys].clone())
                    .collect(),
                None => Vec::new(),
            };

            // Drain via get_next_song collecting the play sequence.
            let mut seq: Vec<String> = Vec::new();
            if let Some(ci) = post_ci {
                seq.push(qm.queue.song_ids[ci].clone());
            }
            let cap = n + 5;
            let mut count = 0;
            while let Some(next) = qm.get_next_song() {
                seq.push(next.song.id.clone());
                count += 1;
                prop_assert!(count <= cap, "drain did not terminate (n={}, co={})", n, co);
            }

            // No id repeated (no replay / over-play).
            let mut dedup = seq.clone();
            dedup.sort();
            dedup.dedup();
            prop_assert_eq!(
                dedup.len(), seq.len(),
                "drain replayed/over-played a song: {:?} (n={}, co={})", seq, n, co
            );

            // Exact reachability: drain equals the post-removal play-order tail.
            prop_assert_eq!(
                seq.clone(), expected,
                "drain != post-removal play-order tail: {:?} (n={}, co={})", seq, n, co
            );

            // And every genuinely-upcoming survivor (captured before removal)
            // is reached — guards against silently dropping the unplayed tail.
            for up in &upcoming_before {
                prop_assert!(
                    seq.contains(up),
                    "upcoming survivor {} stranded (n={}, co={}, seq={:?})", up, n, co, seq
                );
            }
        }
    }
}
