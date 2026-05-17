//! `NextTrackResetEffect`: typed `#[must_use]` token that compels callers
//! of any queue mutator which may have invalidated the prepared next-track
//! decoder to dispatch `engine.reset_next_track()` after the queue mutation.
//!
//! Queue mode toggles (shuffle / repeat / consume) and queue reorders
//! (move, insert, remove, sort, shuffle_queue, set_queue, add_songs,
//! reposition_to_index) all return this token via the `QueueWriteGuard`
//! commit methods. A caller who drops the effect without consuming it
//! produces a `must_use` warning, which the workspace's `-D warnings`
//! clippy gate escalates to an error.

use tokio::sync::Mutex;

use crate::audio::engine::CustomAudioEngine;

#[must_use = "NextTrackResetEffect must be applied via `effect.apply_to(&engine).await` (or `effect.apply_locked(&mut engine).await` when the engine lock is already held) to reset gapless prep — forgetting silently corrupts next-track state"]
pub struct NextTrackResetEffect {
    _seal: (),
}

impl NextTrackResetEffect {
    pub(crate) fn new() -> Self {
        Self { _seal: () }
    }

    /// Consume the effect by resetting the engine's prepared next-track
    /// state. Locks the engine internally — use when the caller does not
    /// already hold the engine lock.
    pub async fn apply_to(self, engine: &Mutex<CustomAudioEngine>) {
        engine.lock().await.reset_next_track().await;
    }

    /// Consume the effect when the caller already holds the engine lock.
    /// Used by the completion-callback consume path, `play_next` /
    /// `play_previous`, and other paths that mutate the queue from inside
    /// an existing `&mut CustomAudioEngine` borrow.
    pub async fn apply_locked(self, engine: &mut CustomAudioEngine) {
        engine.reset_next_track().await;
    }
}
