//! Order array management for `QueueManager`
//!
//! Contains all order array manipulation methods: rebuild, shuffle,
//! unshuffle, extend, remove, insert, and sync helpers.

use tracing::debug;

use super::QueueManager;
use crate::types::queue::{PlayOrder, RepeatMode};

impl QueueManager {
    // ══════════════════════════════════════════════════════════════════════
    //  Order Array Management
    // ══════════════════════════════════════════════════════════════════════

    /// Rebuild the order array as identity `[0, 1, 2, …, len-1]`.
    /// Called after `set_queue`, `add_songs`, sort, and on migration.
    pub(crate) fn rebuild_order(&mut self) {
        self.queue.order = PlayOrder::identity(self.queue.rows.len());
    }

    /// Rebuild the order array as identity and point the play cursor at
    /// `row` (the physical index of the playing song).
    pub(crate) fn rebuild_order_and_set_cursor(&mut self, row: Option<usize>) {
        self.rebuild_order();
        self.set_cursor_to_row(row);
    }

    /// Point the play cursor (`current_order`) at the order slot holding
    /// physical row index `row` — the single way to move the playhead by
    /// physical position. `current_index` is DERIVED from the cursor, so
    /// there is no second field to keep in sync. `None` (or a row absent
    /// from the order) clears the cursor.
    pub(crate) fn set_cursor_to_row(&mut self, row: Option<usize>) {
        self.queue.current_order =
            row.and_then(|idx| self.queue.order.iter().position(|&o| o == idx));
    }

    /// Fisher-Yates shuffle the order array, keeping the currently-playing
    /// song at its current order position.
    pub(crate) fn shuffle_order(&mut self) {
        if self.queue.order.len() <= 1 {
            return;
        }

        let mut rng = rand::rng();
        // Anchored: current moves to order[0] ("already played"), tail is
        // Fisher-Yates shuffled — the owner-blessed honest-shuffle
        // distribution, implemented inside PlayOrder.
        let anchor = self.queue.current_order;
        self.queue.current_order = self.queue.order.shuffle_anchored(anchor, &mut rng);

        debug!(
            " [SHUFFLE] Order array shuffled ({} entries)",
            self.queue.order.len()
        );
    }

    /// Capture the current play-order as a sequence of per-row `entry_id`s.
    ///
    /// `order[i]` is a `rows` index; this maps each to the row's
    /// `entry_id` — the stable row identity — yielding the upcoming play sequence in
    /// terms that survive a physical reorder. Pair with
    /// [`Self::rebuild_order_from_play_sequence`] to relocate moved rows
    /// inside a shuffled order WITHOUT re-randomizing the tail.
    pub(crate) fn capture_play_order_entry_ids(&self) -> Vec<u64> {
        self.queue
            .order
            .iter()
            .filter_map(|&row_idx| self.queue.rows.get(row_idx).map(|r| r.entry_id))
            .collect()
    }

    /// Rebuild the `order` array so it reproduces a previously-captured
    /// play-order sequence of `entry_id`s over the (possibly reordered)
    /// physical layout. Each entry_id maps to its NEW `rows` index, so
    /// the random tail keeps its relative order and only the moved rows
    /// follow their new physical slot — a queue move under shuffle stops
    /// re-randomizing next-up.
    ///
    /// Falls back to the canonical identity order if the reconstruction
    /// can't reproduce a full permutation (e.g. a row vanished), keeping
    /// the navigation invariants intact.
    /// `cursor_row` is the physical index of the playing song AFTER the
    /// physical reorder (the caller tracked it through the move); the play
    /// cursor re-anchors onto it once the order is rebuilt.
    pub(crate) fn rebuild_order_from_play_sequence(
        &mut self,
        play_order_eids: &[u64],
        cursor_row: Option<usize>,
    ) {
        let new_order: Vec<usize> = play_order_eids
            .iter()
            .filter_map(|&eid| self.queue.rows.iter().position(|r| r.entry_id == eid))
            .collect();
        let len = self.queue.rows.len();
        if !self.queue.order.splice_from_play_sequence(new_order, len) {
            self.rebuild_order();
        }
        self.set_cursor_to_row(cursor_row);
    }

    /// Restore order array to identity `[0, 1, 2, …]`, keeping the cursor
    /// on the same physical row.
    pub(crate) fn unshuffle_order(&mut self) {
        let row = self.queue.current_index();
        self.rebuild_order_and_set_cursor(row);
        debug!(" [SHUFFLE] Order array restored to identity");
    }

    /// Clear the queued next-song. Called on any queue mutation that could
    /// invalidate the stored order index.
    pub(crate) fn clear_queued(&mut self) {
        if self.queue.queued.take().is_some() {
            debug!(" [QUEUE] Cleared queued next song (queue mutated)");
        }
    }

    /// Compute the next order index based on current position and repeat mode.
    /// Returns `None` if at end of queue with no repeat.
    pub(crate) fn next_order_index(&self) -> Option<usize> {
        if self.queue.order.is_empty() {
            return None;
        }
        let cur = self.queue.current_order?;
        let next = cur + 1;
        if next < self.queue.order.len() {
            Some(next)
        } else if self.queue.repeat == RepeatMode::Playlist && !self.queue.consume {
            Some(0)
        } else {
            None
        }
    }

    /// Compute the previous order index based on current position and repeat
    /// mode. Mirrors `next_order_index`: returns `None` at the play-order head
    /// with no repeat. Order-aware so Previous walks `order[]`, not physical
    /// row positions, under shuffle.
    pub(crate) fn prev_order_index(&self) -> Option<usize> {
        if self.queue.order.is_empty() {
            return None;
        }
        let cur = self.queue.current_order?;
        if cur > 0 {
            Some(cur - 1)
        } else if self.queue.repeat == RepeatMode::Playlist && !self.queue.consume {
            Some(self.queue.order.len().saturating_sub(1))
        } else {
            None
        }
    }

    /// Insert new row indices into the order array at the end.
    /// Used when songs are appended to the queue.
    pub(crate) fn extend_order(&mut self, new_indices: std::ops::Range<usize>) {
        // In shuffle mode, new entries land at random positions AFTER
        // current_order so they're in the "upcoming" portion.
        let shuffled_after = self
            .queue
            .shuffle
            .then(|| self.queue.current_order.map_or(0, |c| c + 1));
        self.queue
            .order
            .extend_rows(new_indices, shuffled_after, &mut rand::rng());
    }

    /// Remove a row index from the order array and adjust all
    /// indices that are > removed to account for the shift.
    pub(crate) fn remove_from_order(&mut self, removed_song_idx: usize) {
        // PlayOrder removes the entry and shifts the higher indices down;
        // the play-cursor and gapless-prep adjustments stay here.
        if let Some(order_pos) = self.queue.order.remove_row(removed_song_idx) {
            // Adjust current_order
            if let Some(cur) = self.queue.current_order {
                if self.queue.order.is_empty() {
                    self.queue.current_order = None;
                } else if order_pos < cur {
                    self.queue.current_order = Some(cur - 1);
                } else if order_pos == cur {
                    // Current was removed — clamp
                    self.queue.current_order =
                        Some(cur.min(self.queue.order.len().saturating_sub(1)));
                }
            }

            // Adjust queued
            if let Some(q) = self.queue.queued {
                if order_pos == q {
                    self.queue.queued = None;
                } else if order_pos < q {
                    self.queue.queued = Some(q - 1);
                }
            }
        }
    }

    /// Insert entries for new row indices at a specific position.
    /// Adjusts existing order entries that are >= insert_pos upward.
    pub(crate) fn insert_into_order(&mut self, insert_pos: usize, count: usize) {
        // PlayOrder shifts existing indices >= insert_pos up and inserts the
        // new entries (randomly after current under shuffle, at the matching
        // sequential position otherwise), reporting where each one landed so
        // the play-cursor and gapless-prep adjustments replay here in the
        // same application order.
        let shuffled_after = self
            .queue
            .shuffle
            .then(|| self.queue.current_order.map_or(0, |c| c + 1));
        let applied =
            self.queue
                .order
                .insert_rows(insert_pos, count, shuffled_after, &mut rand::rng());
        for insert_at in applied {
            if let Some(cur) = self.queue.current_order
                && insert_at <= cur
            {
                self.queue.current_order = Some(cur + 1);
            }
            if let Some(q) = self.queue.queued
                && insert_at <= q
            {
                self.queue.queued = Some(q + 1);
            }
        }
    }
}
