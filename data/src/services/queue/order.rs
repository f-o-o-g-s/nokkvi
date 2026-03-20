//! Order array management for `QueueManager`
//!
//! Contains all order array manipulation methods: rebuild, shuffle,
//! unshuffle, extend, remove, insert, and sync helpers.

use rand::seq::SliceRandom;
use tracing::debug;

use super::QueueManager;
use crate::types::queue::RepeatMode;

impl QueueManager {
    // ══════════════════════════════════════════════════════════════════════
    //  Order Array Management
    // ══════════════════════════════════════════════════════════════════════

    /// Rebuild the order array as identity `[0, 1, 2, …, len-1]`.
    /// Called after `set_queue`, `add_songs`, sort, and on migration.
    pub(crate) fn rebuild_order(&mut self) {
        self.queue.order = (0..self.queue.song_ids.len()).collect();
    }

    /// Rebuild the order array and sync `current_order` with `current_index`.
    pub(crate) fn rebuild_order_and_sync(&mut self) {
        self.rebuild_order();
        self.sync_current_order_to_index();
    }

    /// Sync `current_order` to match `current_index` in the order array.
    /// Used when the order array is identity (shuffle off) or after rebuild.
    pub(crate) fn sync_current_order_to_index(&mut self) {
        self.queue.current_order = self
            .queue
            .current_index
            .and_then(|idx| self.queue.order.iter().position(|&o| o == idx));
    }

    /// Fisher-Yates shuffle the order array, keeping the currently-playing
    /// song at its current order position.
    pub(crate) fn shuffle_order(&mut self) {
        if self.queue.order.len() <= 1 {
            return;
        }

        let mut rng = rand::rng();

        // If we have a current order position, anchor it
        if let Some(cur_order) = self.queue.current_order {
            // Move current to front, shuffle the rest, then swap back
            let cur_song_idx = self.queue.order[cur_order];
            self.queue.order.swap(0, cur_order);
            self.queue.order[1..].shuffle(&mut rng);
            // Put current back at position 0 so it's "already played"
            // and next song is order[1]
            self.queue.order[0] = cur_song_idx;
            self.queue.current_order = Some(0);
        } else {
            self.queue.order.shuffle(&mut rng);
        }

        debug!(
            " [SHUFFLE] Order array shuffled ({} entries)",
            self.queue.order.len()
        );
    }

    /// Restore order array to identity `[0, 1, 2, …]` and sync current_order.
    pub(crate) fn unshuffle_order(&mut self) {
        self.rebuild_order();
        self.sync_current_order_to_index();
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
        } else if self.queue.repeat == RepeatMode::Playlist {
            Some(0)
        } else {
            None
        }
    }

    /// Insert new song_ids indices into the order array at the end.
    /// Used when songs are appended to the queue.
    pub(crate) fn extend_order(&mut self, new_indices: std::ops::Range<usize>) {
        if self.queue.shuffle {
            // In shuffle mode, insert new entries at random positions
            // AFTER current_order so they're in the "upcoming" portion
            let insert_after = self.queue.current_order.map_or(0, |c| c + 1);
            for idx in new_indices {
                let insert_at = if insert_after < self.queue.order.len() {
                    rand::random_range(insert_after..=self.queue.order.len())
                } else {
                    self.queue.order.len()
                };
                self.queue.order.insert(insert_at, idx);
            }
        } else {
            self.queue.order.extend(new_indices);
        }
    }

    /// Remove a song_ids index from the order array and adjust all
    /// indices that are > removed to account for the shift.
    pub(crate) fn remove_from_order(&mut self, removed_song_idx: usize) {
        // Find and remove the entry pointing to removed_song_idx
        if let Some(order_pos) = self.queue.order.iter().position(|&o| o == removed_song_idx) {
            self.queue.order.remove(order_pos);

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

        // Adjust all order entries: indices > removed_song_idx shift down by 1
        for entry in &mut self.queue.order {
            if *entry > removed_song_idx {
                *entry -= 1;
            }
        }
    }

    /// Insert entries for new song_ids indices at a specific position.
    /// Adjusts existing order entries that are >= insert_pos upward.
    pub(crate) fn insert_into_order(&mut self, insert_pos: usize, count: usize) {
        // Bump existing entries that reference indices >= insert_pos
        for entry in &mut self.queue.order {
            if *entry >= insert_pos {
                *entry += count;
            }
        }

        // Also adjust current_order if songs were inserted before it in order
        // (current_order references into the order array, not song_ids, so
        // we only need to adjust song_ids references above)

        // Add new entries. In shuffle mode, place them randomly after current.
        // In sequential mode, place them at the corresponding order position.
        if self.queue.shuffle {
            let insert_after = self.queue.current_order.map_or(0, |c| c + 1);
            for i in 0..count {
                let new_song_idx = insert_pos + i;
                let insert_at = if insert_after < self.queue.order.len() {
                    rand::random_range(insert_after..=self.queue.order.len())
                } else {
                    self.queue.order.len()
                };
                self.queue.order.insert(insert_at, new_song_idx);

                // Adjust current_order and queued since we inserted into the order array
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
        } else {
            // Sequential: insert at the matching order position
            // Find where in the order array the insert_pos-th song_ids entry would go
            let order_insert = self
                .queue
                .order
                .iter()
                .position(|&o| o >= insert_pos)
                .unwrap_or(self.queue.order.len());
            for i in 0..count {
                self.queue.order.insert(order_insert + i, insert_pos + i);
                // Adjust current_order and queued
                if let Some(cur) = self.queue.current_order
                    && order_insert + i <= cur
                {
                    self.queue.current_order = Some(cur + 1);
                }
                if let Some(q) = self.queue.queued
                    && order_insert + i <= q
                {
                    self.queue.queued = Some(q + 1);
                }
            }
        }
    }
}
