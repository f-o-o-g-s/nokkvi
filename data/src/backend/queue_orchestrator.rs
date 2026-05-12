//! Five queue verbs that consume `Vec<Song>` and dispatch to existing
//! `QueueService` / `PlaybackController` primitives.
//!
//! Borrowed from `AppService` via `app.queue_orchestrator()` — like
//! `LibraryOrchestrator`, holds only references.

use anyhow::Result;
use tracing::debug;

use crate::{
    backend::{playback_controller::PlaybackController, queue::QueueService},
    types::song::Song,
};

pub struct QueueOrchestrator<'a> {
    queue: &'a QueueService,
    playback: &'a PlaybackController,
}

impl<'a> QueueOrchestrator<'a> {
    pub(crate) fn new(queue: &'a QueueService, playback: &'a PlaybackController) -> Self {
        Self { queue, playback }
    }

    /// Replace queue with `songs`, set current to `start_index`, start playback.
    /// Mirrors today's `play_album` etc. — the universal "play this entity now" verb.
    pub async fn play(&self, songs: Vec<Song>, start_index: usize) -> Result<()> {
        self.playback
            .play_songs_from_index(songs, start_index)
            .await
    }

    /// Append to queue without changing playback state.
    /// Mirrors today's `add_*_to_queue` family.
    pub async fn enqueue(&self, songs: Vec<Song>) -> Result<()> {
        self.queue.add_songs(songs).await
    }

    /// Append, then jump-play the first newly-appended song.
    /// Mirrors today's `add_*_and_play` family. Records the pre-append
    /// queue length to know which index the new songs land at.
    pub async fn enqueue_and_play(&self, songs: Vec<Song>) -> Result<()> {
        if songs.is_empty() {
            return Ok(());
        }
        let first_id = songs[0].id.clone();
        let queue_index = self.queue.get_songs().len();
        self.queue.add_songs(songs).await?;
        self.playback
            .play_song_from_queue(&first_id, queue_index)
            .await
    }

    /// Insert at an explicit position.
    /// Mirrors today's `insert_*_at_position` family.
    pub async fn insert_at(&self, songs: Vec<Song>, position: usize) -> Result<()> {
        self.queue.insert_songs_at(position, songs).await
    }

    /// Insert immediately after the current song (single splice).
    /// Mirrors today's `play_next_*` family + the private
    /// `AppService::play_next_songs` helper at app_service.rs:759-777.
    /// Preserves that helper's empty-input rejection and debug log verbatim.
    pub async fn play_next(&self, songs: Vec<Song>) -> Result<()> {
        if songs.is_empty() {
            return Err(anyhow::anyhow!("No songs to add"));
        }
        let count = songs.len();

        let current_idx = self.queue.current_index().await;
        let target = current_idx.map_or(0, |i| i + 1);

        // `insert_songs_at` now refreshes all three reactives atomically under
        // the same queue lock — no separate `refresh_from_queue` needed.
        self.queue.insert_songs_at(target, songs).await?;
        debug!(
            "⏭ Inserted {} songs as play-next at position {}",
            count, target
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use anyhow::Result;
    use tempfile::TempDir;

    use super::*;
    use crate::{
        backend::{auth::AuthGateway, settings::SettingsService},
        services::{state_storage::StateStorage, task_manager::TaskManager},
    };

    /// Test fixture — real `QueueService` + real `PlaybackController` over
    /// tempfile-backed `StateStorage`. The audio engine is constructed but
    /// never asked to play, so PipeWire is not touched (mirrors the safe
    /// pattern at `services/playback.rs:602`).
    struct Fixture {
        _temp: TempDir,
        queue: QueueService,
        playback: PlaybackController,
    }

    async fn fixture() -> Result<Fixture> {
        let temp = tempfile::tempdir()?;
        let storage_q = StateStorage::new(temp.path().join("queue.redb"))?;
        let storage_s = StateStorage::new(temp.path().join("settings.redb"))?;
        let auth = AuthGateway::new()?;
        let queue = QueueService::new(auth, storage_q)?;
        let settings = SettingsService::new(storage_s)?;
        let tm = Arc::new(TaskManager::new());
        let (playback, _loop_rx, _qc_rx) =
            PlaybackController::new(queue.clone(), settings, tm).await?;
        Ok(Fixture {
            _temp: temp,
            queue,
            playback,
        })
    }

    fn make_songs(ids: &[&str]) -> Vec<Song> {
        ids.iter()
            .map(|id| Song::test_default(id, &format!("Song {id}")))
            .collect()
    }

    fn queue_ids(fx: &Fixture) -> Vec<String> {
        fx.queue.get_songs().iter().map(|s| s.id.clone()).collect()
    }

    #[tokio::test(flavor = "current_thread")]
    async fn enqueue_appends_without_changing_current() -> Result<()> {
        let fx = fixture().await?;
        let orch = QueueOrchestrator::new(&fx.queue, &fx.playback);

        fx.queue.set_queue(make_songs(&["a", "b"]), Some(0)).await?;
        let before_current = fx.queue.current_index().await;

        orch.enqueue(make_songs(&["c", "d"])).await?;

        assert_eq!(queue_ids(&fx), vec!["a", "b", "c", "d"]);
        assert_eq!(fx.queue.current_index().await, before_current);
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn insert_at_passes_position_through() -> Result<()> {
        let fx = fixture().await?;
        let orch = QueueOrchestrator::new(&fx.queue, &fx.playback);

        fx.queue
            .set_queue(make_songs(&["a", "b", "c"]), Some(0))
            .await?;
        orch.insert_at(make_songs(&["x", "y"]), 1).await?;

        assert_eq!(queue_ids(&fx), vec!["a", "x", "y", "b", "c"]);
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn play_next_inserts_at_current_plus_one() -> Result<()> {
        let fx = fixture().await?;
        let orch = QueueOrchestrator::new(&fx.queue, &fx.playback);

        // Current index 1 ("b" is playing) — play_next inserts at 2.
        fx.queue
            .set_queue(make_songs(&["a", "b", "c"]), Some(1))
            .await?;
        orch.play_next(make_songs(&["x"])).await?;

        assert_eq!(queue_ids(&fx), vec!["a", "b", "x", "c"]);
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn play_next_when_no_current_inserts_at_zero() -> Result<()> {
        let fx = fixture().await?;
        let orch = QueueOrchestrator::new(&fx.queue, &fx.playback);

        // Empty queue → current_index = None → target = 0.
        fx.queue.set_queue(vec![], None).await?;
        orch.play_next(make_songs(&["x", "y"])).await?;

        assert_eq!(queue_ids(&fx), vec!["x", "y"]);
        Ok(())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn enqueue_and_play_noop_on_empty_input() -> Result<()> {
        let fx = fixture().await?;
        let orch = QueueOrchestrator::new(&fx.queue, &fx.playback);

        fx.queue.set_queue(make_songs(&["a", "b"]), Some(0)).await?;
        let before = queue_ids(&fx);
        orch.enqueue_and_play(vec![]).await?;
        assert_eq!(queue_ids(&fx), before);
        Ok(())
    }

    /// Compile-only smoke for the playback-touching verbs (`play`,
    /// `enqueue_and_play` with non-empty input). Both call into
    /// `PlaybackController::play_*` which opens PipeWire output —
    /// behavioral coverage lives downstream once Lane C wires the
    /// real callers and integration tests target a live audio stack.
    #[allow(dead_code)]
    async fn _compiles_playback_verbs(orch: &QueueOrchestrator<'_>) -> Result<()> {
        orch.play(vec![], 0).await?;
        orch.enqueue_and_play(vec![Song::test_default("z", "Song z")])
            .await?;
        Ok(())
    }
}
