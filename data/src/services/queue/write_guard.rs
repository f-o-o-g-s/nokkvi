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
use crate::types::NextTrackResetEffect;

pub struct QueueWriteGuard<'a> {
    mgr: Option<&'a mut QueueManager>,
}

/// Debug-only invariant check, fired on every commit path. After the typed
/// Phase-2 model most of the historical checks are STRUCTURAL and gone from
/// here: `order` is a permutation by `PlayOrder` construction, and
/// `current_index` is derived from `order[current_order]`, so the I3
/// coupling cannot break. What still needs asserting is the one coupling
/// the types cannot see: `order` must track the ROW VECTOR's length, and a
/// `Some` cursor must be in range (a stale cursor after an order shrink
/// would silently derive `None`). Release builds skip the check;
/// `every_mutator_keeps_rows_order_consistent` covers release builds.
#[inline]
fn assert_order_consistent(mgr: &QueueManager) {
    let n = mgr.queue.rows.len();
    debug_assert_eq!(
        mgr.queue.order.len(),
        n,
        "order length drifted from rows: a mutator updated one without the other",
    );
    if let Some(co) = mgr.queue.current_order {
        debug_assert!(
            co < mgr.queue.order.len(),
            "current_order {co} out of range 0..{}",
            mgr.queue.order.len(),
        );
    }
}

impl QueueWriteGuard<'_> {
    /// Commit with full save (queue ordering + song pool). Returns a
    /// [`NextTrackResetEffect`] the caller must dispatch to the audio
    /// engine — every queue mutation may have invalidated the prepared
    /// next-track decoder.
    pub fn commit_save_all(mut self) -> Result<NextTrackResetEffect> {
        let mgr = self.mgr.take().expect("guard already consumed");
        assert_order_consistent(mgr);
        mgr.clear_queued();
        mgr.save_all()?;
        Ok(NextTrackResetEffect::new())
    }

    /// Commit with order-only save (song pool unchanged). Returns a
    /// [`NextTrackResetEffect`] obligation — see [`Self::commit_save_all`].
    pub fn commit_save_order(mut self) -> Result<NextTrackResetEffect> {
        let mgr = self.mgr.take().expect("guard already consumed");
        assert_order_consistent(mgr);
        mgr.clear_queued();
        mgr.save_order()?;
        Ok(NextTrackResetEffect::new())
    }

    /// Commit without persisting (in-memory mutation only). Returns a
    /// [`NextTrackResetEffect`] obligation — see [`Self::commit_save_all`].
    pub fn commit_no_save(mut self) -> NextTrackResetEffect {
        let mgr = self.mgr.take().expect("guard already consumed");
        assert_order_consistent(mgr);
        mgr.clear_queued();
        NextTrackResetEffect::new()
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
