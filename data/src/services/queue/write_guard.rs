//! Typestate guard for queue mutations.
//!
//! [`QueueWriteGuard`] makes the IG-5 invariant — every queue mutation
//! must clear `queued` — enforced at the type level. A mutator takes
//! `let mut tx = self.write()`, mutates through the guard, then commits
//! via one of three named methods:
//!
//! - [`commit_save_all`](QueueWriteGuard::commit_save_all) — full save
//!   (queue ordering + song pool). Use after add/remove/insert/set_queue.
//! - [`commit_save_order`](QueueWriteGuard::commit_save_order) — order-only
//!   save (pool untouched). Use after move/sort/shuffle/mode-toggle.
//! - [`commit_no_save`](QueueWriteGuard::commit_no_save) — in-memory only.
//!   Use after `reposition_to_index`.
//!
//! Drop is the safety net: on `?` propagation or panic between `write()`
//! and `commit_*`, Drop still runs `clear_queued()`, so the navigator
//! cannot transition to an entry stored against pre-mutation order.

use anyhow::Result;

use super::QueueManager;

pub struct QueueWriteGuard<'a> {
    mgr: Option<&'a mut QueueManager>,
}

/// Debug-only invariant check: `entry_ids` must stay strictly parallel to
/// `queue.song_ids`. Every mutator pairs the two; this assert lights up the
/// path in tests if a future mutator forgets one half. Release builds skip
/// the check, so production cost is zero.
#[inline]
fn assert_entry_ids_parallel(mgr: &QueueManager) {
    debug_assert_eq!(
        mgr.entry_ids.len(),
        mgr.queue.song_ids.len(),
        "entry_ids drifted from song_ids: a mutator updated one without the other",
    );
}

impl QueueWriteGuard<'_> {
    /// Commit with full save (queue ordering + song pool).
    pub fn commit_save_all(mut self) -> Result<()> {
        let mgr = self.mgr.take().expect("guard already consumed");
        assert_entry_ids_parallel(mgr);
        mgr.clear_queued();
        mgr.save_all()
    }

    /// Commit with order-only save (song pool unchanged).
    pub fn commit_save_order(mut self) -> Result<()> {
        let mgr = self.mgr.take().expect("guard already consumed");
        assert_entry_ids_parallel(mgr);
        mgr.clear_queued();
        mgr.save_order()
    }

    /// Commit without persisting (in-memory mutation only).
    pub fn commit_no_save(mut self) {
        let mgr = self.mgr.take().expect("guard already consumed");
        assert_entry_ids_parallel(mgr);
        mgr.clear_queued();
    }
}

impl Drop for QueueWriteGuard<'_> {
    fn drop(&mut self) {
        if let Some(mgr) = self.mgr.take() {
            mgr.clear_queued();
        }
    }
}

impl std::ops::Deref for QueueWriteGuard<'_> {
    type Target = QueueManager;
    fn deref(&self) -> &QueueManager {
        self.mgr.as_deref().expect("guard already consumed")
    }
}

impl std::ops::DerefMut for QueueWriteGuard<'_> {
    fn deref_mut(&mut self) -> &mut QueueManager {
        self.mgr.as_deref_mut().expect("guard already consumed")
    }
}

impl QueueManager {
    /// Begin a queue mutation. Returns a guard that auto-clears `queued`
    /// on Drop and exposes named `commit_save_*` finalizers.
    pub(super) fn write(&mut self) -> QueueWriteGuard<'_> {
        QueueWriteGuard { mgr: Some(self) }
    }
}
