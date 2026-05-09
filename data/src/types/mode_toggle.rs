//! `ModeToggleEffect`: typed `#[must_use]` token that compels callers
//! of `QueueManager::toggle_shuffle` / `set_repeat` / `toggle_consume`
//! to dispatch `engine.reset_next_track()` after the queue mutation.
//!
//! Today the `PlaybackController` calls `reset_next_track()` after each
//! mode-toggle queue mutation by hand. The token makes that pairing a
//! compile-time obligation: a caller who drops the effect without
//! consuming it produces a `must_use` warning, which the workspace's
//! `-D warnings` clippy gate escalates to an error.

use tokio::sync::Mutex;

use crate::audio::engine::CustomAudioEngine;

#[must_use = "ModeToggleEffect must be applied via `effect.apply_to(&engine).await` to reset gapless prep — forgetting silently corrupts next-track state"]
pub struct ModeToggleEffect {
    _seal: (),
}

impl ModeToggleEffect {
    pub(crate) fn new() -> Self {
        Self { _seal: () }
    }

    /// Consume the effect by resetting the engine's prepared next-track
    /// state. The only path to actually perform the reset.
    pub async fn apply_to(self, engine: &Mutex<CustomAudioEngine>) {
        engine.lock().await.reset_next_track().await;
    }
}
