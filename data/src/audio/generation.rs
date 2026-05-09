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

    /// User-driven source change (manual skip, seek, set_source).
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
