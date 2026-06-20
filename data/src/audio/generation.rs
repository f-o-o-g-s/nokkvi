//! Typed atomic-counter wrappers for the audio engine's generation handles.
//!
//! These exist to give every "stop the previous decode loop" / "invalidate
//! pending source-change callbacks" call site a typed mutator instead of a
//! raw `fetch_add`. The wrapped counter is still a single `Arc<AtomicU64>`,
//! so cloning is cheap and the spawned decode tasks can read it lock-free.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// Generation counter for the decode loop. Each spawned loop captures
/// `current()` at spawn time and exits when the value moves. `supersede()`
/// is the single mutator — every "stop the decode loop" path goes through it.
#[derive(Clone, Debug)]
pub struct DecodeLoopHandle {
    counter: Arc<AtomicU64>,
}

impl DecodeLoopHandle {
    pub fn new() -> Self {
        Self {
            counter: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Invalidate every spawned decode loop currently observing the previous
    /// generation. Returns the new generation.
    pub fn supersede(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// Lock-free generation read for the spawned decode loops.
    pub fn current(&self) -> u64 {
        self.counter.load(Ordering::Acquire)
    }
}

/// Source generation counter. Shared with the renderer so completion
/// callbacks can detect staleness without taking the engine lock.
#[derive(Clone, Debug)]
pub struct SourceGeneration {
    counter: Arc<AtomicU64>,
}

impl SourceGeneration {
    pub fn new() -> Self {
        Self {
            counter: Arc::new(AtomicU64::new(0)),
        }
    }

    /// User-driven source change (manual skip / set_source). The renderer
    /// discards in-flight completion callbacks tagged with an older generation.
    ///
    /// Seek intentionally does NOT bump — the source URL is unchanged, so
    /// `renderer.seek` recreates the primary stream under the same generation
    /// (the seek window is gated by the `seeking` AtomicBool +
    /// `decode_loop.supersede` instead). Bumping on seek would needlessly
    /// invalidate the visualizer/render staleness gating mid-seek.
    pub fn bump_for_user_action(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::Release) + 1
    }

    /// Decode-loop gapless inline-swap — source URL changed.
    pub fn bump_for_gapless(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::Release) + 1
    }

    /// Crossfade-finalize path: intentional no-op so the existing
    /// "don't increment here" comment becomes a typed call.
    pub fn accept_internal_swap(&self) {}

    pub fn current(&self) -> u64 {
        self.counter.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `bump_for_user_action` advances `current()` by exactly 1, from whatever
    /// non-default generation the counter currently holds.
    #[test]
    fn bump_for_user_action_advances_current_by_one() {
        let src_gen = SourceGeneration::new();
        // Seed a distinctive, non-default generation so the assertion is a
        // relative delta rather than a "0 -> 1" tautology.
        src_gen.bump_for_user_action();
        src_gen.bump_for_gapless();
        let before = src_gen.current();

        let returned = src_gen.bump_for_user_action();

        assert_eq!(
            src_gen.current(),
            before + 1,
            "bump_for_user_action must advance current() by exactly 1"
        );
        assert_eq!(
            returned,
            before + 1,
            "bump_for_user_action must return the new generation"
        );
    }

    /// `bump_for_gapless` advances `current()` by exactly 1 — it is the sole
    /// decode-loop bump and must move the shared counter forward.
    #[test]
    fn bump_for_gapless_advances_current_by_one() {
        let src_gen = SourceGeneration::new();
        // Seed a distinctive, non-default generation.
        src_gen.bump_for_gapless();
        src_gen.bump_for_user_action();
        src_gen.bump_for_gapless();
        let before = src_gen.current();

        let returned = src_gen.bump_for_gapless();

        assert_eq!(
            src_gen.current(),
            before + 1,
            "bump_for_gapless must advance current() by exactly 1"
        );
        assert_eq!(
            returned,
            before + 1,
            "bump_for_gapless must return the new generation"
        );
    }

    /// `accept_internal_swap` is the crossfade-finalize no-op: it must leave
    /// `current()` UNCHANGED. Bumping here would re-break the consume+shuffle
    /// replay guard, so this asserts before == after, never a bump.
    #[test]
    fn accept_internal_swap_leaves_current_unchanged() {
        let src_gen = SourceGeneration::new();
        // Seed a distinctive, non-default generation so "unchanged" is a real
        // observation, not the initial 0.
        src_gen.bump_for_user_action();
        src_gen.bump_for_gapless();
        src_gen.bump_for_user_action();
        let before = src_gen.current();

        src_gen.accept_internal_swap();

        assert_eq!(
            src_gen.current(),
            before,
            "accept_internal_swap must be a no-op and leave current() unchanged"
        );
    }

    /// The counter is a shared `Arc<AtomicU64>`: a bump through one clone is
    /// observable through another, and `accept_internal_swap` on either clone
    /// leaves both readings unchanged.
    #[test]
    fn current_is_shared_across_clones() {
        let src_gen = SourceGeneration::new();
        let clone = src_gen.clone();
        clone.bump_for_user_action();
        let before = src_gen.current();

        clone.accept_internal_swap();
        assert_eq!(
            src_gen.current(),
            before,
            "accept_internal_swap must not move the shared counter for any clone"
        );

        let returned = clone.bump_for_gapless();
        assert_eq!(
            src_gen.current(),
            before + 1,
            "a bump on one clone is visible through another"
        );
        assert_eq!(returned, before + 1);
    }
}
