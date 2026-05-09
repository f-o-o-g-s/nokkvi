use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;
use tracing::debug;

use crate::{audio::engine::CustomAudioEngine, services::queue::QueueManager, types::song::Song};

/// Plan describing what the audio engine still needs to do after the queue
/// has been advanced to the next track.
///
/// Returned by [`QueueNavigator::decide_transition`] and consumed by
/// [`QueueNavigator::execute_transition`]. Splitting the work this way
/// lets the completion callback drop the outer `QueueNavigator` mutex
/// before any network-bound engine ops run.
#[derive(Debug, Clone)]
pub enum TrackTransitionPlan {
    /// Queue exhausted or no transition available — stop the engine.
    Stop,
    /// Engine already has the next track ready (gapless or prepared decoder).
    /// Just ensure playback is running.
    PlayPrepared { song: Song, reason: String },
    /// Need to load a fresh stream URL (path 3).
    LoadFresh {
        stream_url: String,
        song: Song,
        reason: String,
    },
}

/// Plan describing what the audio engine must do after a queue-removal has
/// already updated `QueueManager.queue.current_index`.
///
/// Returned by [`decide_removal_aftermath`]. The decision is pure — no engine
/// I/O, no further queue mutation — so the orchestrator can drop the queue
/// lock before applying the plan via
/// [`PlaybackController::apply_removal_aftermath`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemovalAftermath {
    /// Either nothing was playing or the playing song survived the removal.
    /// The engine and the navigator stay as they are.
    NoCurrentChange,
    /// The playing song was removed and the queue still has songs. The engine
    /// must load `new_song_id` (which the queue model has already promoted to
    /// `queue.current_index = new_index`) and the navigator's `current_song_id`
    /// must follow.
    LoadNewCurrent {
        new_song_id: String,
        new_index: usize,
    },
    /// The playing song was removed and the queue is now empty. The engine
    /// must stop and the navigator's `current_song_id` must clear.
    StopEmpty,
}

/// Decide what the audio engine must do after a queue-removal mutation has
/// already updated `QueueManager.queue.current_index`.
///
/// Pure — reads inputs only, no I/O, no mutation. Runs after
/// `QueueManager::remove_songs_by_ids` so `qm.queue.current_index` already
/// names whatever now occupies the playing slot (per the clamp in
/// `QueueManager::remove_song`).
///
/// `was_playing_id` is the song the navigator named *before* the removal;
/// the caller must snapshot it because the navigator's stored
/// `current_song_id` is stale by the time this decision runs.
pub fn decide_removal_aftermath(
    qm: &QueueManager,
    was_playing_id: Option<&str>,
    removed_ids: &[String],
) -> RemovalAftermath {
    let was_playing = match was_playing_id {
        Some(id) => id,
        None => return RemovalAftermath::NoCurrentChange,
    };
    if !removed_ids.iter().any(|id| id == was_playing) {
        return RemovalAftermath::NoCurrentChange;
    }
    let queue = qm.get_queue();
    match queue
        .current_index
        .and_then(|idx| queue.song_ids.get(idx).map(|id| (id.clone(), idx)))
    {
        Some((new_id, idx)) => RemovalAftermath::LoadNewCurrent {
            new_song_id: new_id,
            new_index: idx,
        },
        None => RemovalAftermath::StopEmpty,
    }
}

/// QueueNavigator - Low-level queue navigation and track transition handling
///
/// Handles:
/// - Track-to-track transitions (gapless, consume mode, normal)
/// - Manual next/previous navigation
/// - Current song ID tracking
///
/// Mode state (shuffle, repeat, consume) is read directly from QueueManager.
/// High-level orchestration ("play album X") is handled by AppService.
pub struct QueueNavigator {
    queue_manager: Arc<Mutex<QueueManager>>,
    // Playback state
    current_song_id: Arc<Mutex<Option<String>>>,
}

impl std::fmt::Debug for QueueNavigator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueueNavigator").finish()
    }
}

impl QueueNavigator {
    pub async fn new(queue_manager: Arc<Mutex<QueueManager>>) -> Result<Self> {
        // Initialize current_song_id from persisted queue to prevent false "song change"
        // detection on startup. If the queue has a current_index, get that song's ID.
        let initial_song_id = {
            let queue = queue_manager.lock().await;
            let queue_ref = queue.get_queue();
            queue_ref
                .current_index
                .and_then(|idx| queue_ref.song_ids.get(idx))
                .cloned()
        };

        Ok(Self {
            queue_manager,
            current_song_id: Arc::new(Mutex::new(initial_song_id)),
        })
    }

    // ══════════════════════════════════════════════════════════════════════
    //  Shared helpers
    // ══════════════════════════════════════════════════════════════════════

    /// Build a stream URL for a song.
    fn build_stream_url(song_id: &str, server_url: &str, subsonic_credential: &str) -> String {
        format!(
            "{}/rest/stream?id={}&{}&f=json&v=1.8.0&c=nokkvi&_={}",
            server_url,
            song_id,
            subsonic_credential,
            chrono::Utc::now().timestamp_millis()
        )
    }

    /// Record the previous song in history, then consume it if consume mode
    /// is active.
    ///
    /// This is the single entry point for all consume-mode cleanup.
    /// Call this after transitioning to the next song.
    fn record_and_consume(
        &self,
        queue_manager: &mut QueueManager,
        prev_song_id: &str,
        prev_index: usize,
    ) {
        // Record in history
        if let Some(prev_song) = queue_manager.get_song(prev_song_id).cloned() {
            queue_manager.add_to_history(prev_song);
        }

        // Consume: remove the finished song from queue + pool
        if queue_manager.get_queue().consume {
            self.consume_song_at_index(queue_manager, prev_index);
        }
    }

    /// Remove a song from the queue by its index.
    /// Uses QueueManager.remove_song() which properly maintains the order array,
    /// adjusts current_index, and persists.
    fn consume_song_at_index(&self, queue_manager: &mut QueueManager, index: usize) {
        if index >= queue_manager.get_queue().song_ids.len() {
            return;
        }

        if let Some(id) = queue_manager.get_queue().song_ids.get(index)
            && let Some(song) = queue_manager.get_song(id)
        {
            debug!(
                " [CONSUME] Removing: {} - {} (idx: {})",
                song.title, song.artist, index
            );
        }

        queue_manager.remove_song(index).ok();

        debug!(
            " [CONSUME] Queue length now: {}",
            queue_manager.get_queue().song_ids.len()
        );
    }

    // ══════════════════════════════════════════════════════════════════════
    //  Track-finished handler (automatic transitions)
    // ══════════════════════════════════════════════════════════════════════

    /// Handle track finished - play next song.
    ///
    /// Three engine states, but ONE queue transition path:
    /// 1. Engine already playing → gapless/crossfade completed by engine
    /// 2. Prepared track available → gapless load
    /// 3. Normal → need to load a new track
    ///
    /// In ALL cases, the queue transition uses `transition_to_queued()`.
    ///
    /// This is a thin wrapper around [`decide_transition`] + [`execute_transition`].
    /// The completion callback in `playback_controller.rs` calls those two halves
    /// directly so the outer `nav` mutex is dropped before engine I/O.
    pub async fn on_track_finished(
        &self,
        engine: &mut CustomAudioEngine,
        server_url: &str,
        subsonic_credential: &str,
    ) -> Result<Option<(Song, String)>> {
        let plan = self
            .decide_transition(engine, server_url, subsonic_credential)
            .await;
        Self::execute_transition(plan, engine).await
    }

    /// Decide what should happen at the engine layer for the next track.
    ///
    /// Inspects engine state, mutates the queue + `current_song_id`, and
    /// returns a [`TrackTransitionPlan`] describing the engine ops still
    /// needed. Holds the `queue_manager` lock briefly. Does no network I/O.
    ///
    /// The two engine-state mutations called here
    /// (`consume_gapless_transition`, `load_prepared_track`) are fast
    /// metadata swaps over already-prepared decoders — they do not touch
    /// the network.
    pub async fn decide_transition(
        &self,
        engine: &mut CustomAudioEngine,
        server_url: &str,
        subsonic_credential: &str,
    ) -> TrackTransitionPlan {
        // ── Determine engine state and handle audio layer ──
        let needs_load = if engine.immediate_playing() {
            // Path 1: Engine already playing (gapless/crossfade completed by engine)
            engine.consume_gapless_transition().await;
            debug!(" [TRACK FINISHED] Engine already playing - gapless transition completed");
            false
        } else if engine.load_prepared_track().await.is_ok() {
            // Path 2: Prepared track loaded successfully
            debug!(" [TRACK FINISHED] Loaded prepared track");
            false
        } else {
            // Path 3: No prepared track available
            debug!(" [TRACK FINISHED] No prepared track, will load fresh");
            true
        };

        // ── Single queue transition path ──
        let mut queue_manager = self.queue_manager.lock().await;

        let is_repeat_track =
            queue_manager.get_queue().repeat == crate::types::queue::RepeatMode::Track;

        if is_repeat_track {
            // Clear queued just in case
            queue_manager.clear_queued();

            let idx = queue_manager.get_queue().current_index;
            let song = if let Some(idx) = idx {
                if let Some(id) = queue_manager.get_queue().song_ids.get(idx) {
                    queue_manager.get_song(id).cloned()
                } else {
                    None
                }
            } else {
                None
            };

            if let Some(song) = song {
                // Do NOT consume the track since we are repeating it
                queue_manager.add_to_history(song.clone());
                drop(queue_manager);

                debug!("▶️ Now Playing: {} - {} (repeat)", song.title, song.artist);
                let reason = "repeat".to_string();
                return if needs_load {
                    let stream_url =
                        Self::build_stream_url(&song.id, server_url, subsonic_credential);
                    TrackTransitionPlan::LoadFresh {
                        stream_url,
                        song,
                        reason,
                    }
                } else {
                    TrackTransitionPlan::PlayPrepared { song, reason }
                };
            }

            drop(queue_manager);
            return TrackTransitionPlan::Stop;
        }

        // For path 3, ensure queued is set
        if needs_load && queue_manager.peek_next_song().is_none() {
            // Consume the just-finished song before stopping
            let prev_id = self.current_song_id.lock().await.clone();
            if let Some(ref pid) = prev_id
                && let Some(idx) = queue_manager.get_queue().current_index
            {
                self.record_and_consume(&mut queue_manager, pid, idx);
            }
            *self.current_song_id.lock().await = None;
            queue_manager.set_current_index(None);
            queue_manager.save_all().ok();
            drop(queue_manager);
            debug!(" No next song available (queue empty or at end)");
            return TrackTransitionPlan::Stop;
        }

        // Transition: update current_index/current_order, consume queued.
        // If `queued` was cleared by a concurrent queue mutation (add_songs/remove_song
        // calling clear_queued()) between gapless prep and this callback, re-peek now.
        // This is critical for paths 1/2 where the engine is already playing the next
        // track — stopping it would kill a successful gapless transition.
        if queue_manager.get_queue().queued.is_none() && !needs_load {
            debug!(" [TRACK FINISHED] queued was cleared (concurrent queue mutation), re-peeking");
            queue_manager.peek_next_song();
        }
        let Some(transition) = queue_manager.transition_to_queued() else {
            drop(queue_manager);
            debug!(" No queued song to transition to");
            return TrackTransitionPlan::Stop;
        };

        let song = transition.song.clone();
        let reason = if queue_manager.get_queue().shuffle {
            "shuffle"
        } else {
            "gapless"
        }
        .to_string();

        // Record history + consume previous song (via remove_song which
        // properly maintains the order array)
        let prev_id = self.current_song_id.lock().await.clone();
        if let Some(ref pid) = prev_id
            && let Some(old_idx) = transition.old_index
        {
            self.record_and_consume(&mut queue_manager, pid, old_idx);
        }

        *self.current_song_id.lock().await = Some(song.id.clone());
        drop(queue_manager);

        debug!(
            "▶️ Now Playing: {} - {} ({})",
            song.title, song.artist, reason
        );

        if needs_load {
            let stream_url = Self::build_stream_url(&song.id, server_url, subsonic_credential);
            TrackTransitionPlan::LoadFresh {
                stream_url,
                song,
                reason,
            }
        } else {
            TrackTransitionPlan::PlayPrepared { song, reason }
        }
    }

    /// Execute the engine ops described by `plan`.
    ///
    /// Takes no `&self` — safe to call without the outer `QueueNavigator`
    /// mutex held. Concurrent `play_next` / `play_previous` calls can
    /// proceed against the navigator while the engine is busy with
    /// network-bound `play()` work.
    pub async fn execute_transition(
        plan: TrackTransitionPlan,
        engine: &mut CustomAudioEngine,
    ) -> Result<Option<(Song, String)>> {
        match plan {
            TrackTransitionPlan::Stop => {
                engine.stop().await;
                Ok(None)
            }
            TrackTransitionPlan::PlayPrepared { song, reason } => {
                if !engine.immediate_playing() {
                    engine.play().await?;
                }
                Ok(Some((song, reason)))
            }
            TrackTransitionPlan::LoadFresh {
                stream_url,
                song,
                reason,
            } => {
                engine.set_pending_replay_gain(song.replay_gain.clone());
                engine.load_track(&stream_url).await;
                engine.play().await?;
                Ok(Some((song, reason)))
            }
        }
    }

    // ══════════════════════════════════════════════════════════════════════
    //  Manual navigation (button/hotkey/MPRIS)
    // ══════════════════════════════════════════════════════════════════════

    /// Play a song directly by loading its stream URL.
    ///
    /// Does NOT update `current_index` — callers (`get_next_song`,
    /// `get_previous_song`, etc.) are responsible for setting it before
    /// calling this method. This avoids the `index_of` first-match bug
    /// with duplicate tracks.
    pub async fn play_song_direct(
        &self,
        engine: &mut CustomAudioEngine,
        song: &Song,
        server_url: &str,
        subsonic_credential: &str,
    ) -> Result<()> {
        debug!(
            " Playing: {} - {} (id: {})",
            song.title, song.artist, song.id
        );

        let stream_url = Self::build_stream_url(&song.id, server_url, subsonic_credential);

        *self.current_song_id.lock().await = Some(song.id.clone());

        engine.set_pending_replay_gain(song.replay_gain.clone());
        engine.load_track(&stream_url).await;
        engine.play().await?;

        Ok(())
    }

    /// Play next song (manual skip via button/hotkey/MPRIS).
    pub async fn play_next(
        &self,
        engine: &mut CustomAudioEngine,
        server_url: &str,
        subsonic_credential: &str,
    ) -> Result<Option<(Song, String)>> {
        let mut queue_manager = self.queue_manager.lock().await;
        let current_index = queue_manager.get_queue().current_index;
        let is_consume = queue_manager.get_queue().consume;

        // Record current song in history before advancing
        let prev_id = self.current_song_id.lock().await.clone();
        if let Some(ref pid) = prev_id
            && let Some(prev_song) = queue_manager.get_song(pid).cloned()
        {
            queue_manager.add_to_history(prev_song);
        }

        let Some(result) = queue_manager.get_next_song() else {
            drop(queue_manager);
            return Ok(None);
        };
        drop(queue_manager);

        self.play_song_direct(engine, &result.song, server_url, subsonic_credential)
            .await?;

        // Consume: remove the previously played song after starting the next.
        // Use the explicit old index (not song ID) to correctly handle duplicates.
        if is_consume && let Some(old_idx) = current_index {
            let mut qm = self.queue_manager.lock().await;
            self.consume_song_at_index(&mut qm, old_idx);
        }

        Ok(Some((result.song, result.reason)))
    }

    /// Play previous song (manual skip via button/hotkey/MPRIS).
    pub async fn play_previous(
        &self,
        engine: &mut CustomAudioEngine,
        server_url: &str,
        subsonic_credential: &str,
    ) -> Result<Option<(Song, String)>> {
        use crate::services::queue::PreviousSongResult;

        let mut queue_manager = self.queue_manager.lock().await;
        let current_index = queue_manager.get_queue().current_index;
        let is_consume = queue_manager.get_queue().consume;

        match queue_manager.get_previous_song(current_index) {
            PreviousSongResult::InQueue(song, _index) => {
                debug!("⏮️ Previous: {} - {}", song.artist, song.title);

                let old_current_index = current_index;

                *self.current_song_id.lock().await = Some(song.id.clone());
                drop(queue_manager);

                self.play_song_direct(engine, &song, server_url, subsonic_credential)
                    .await?;

                // Consume: remove the previously played song after starting prev
                // Use the explicit old index to correctly handle duplicates.
                if is_consume && let Some(old_idx) = old_current_index {
                    let mut qm = self.queue_manager.lock().await;
                    self.consume_song_at_index(&mut qm, old_idx);
                }

                debug!("▶️ Now Playing: {} - {}", song.title, song.artist);
                Ok(Some((song, "prev".to_string())))
            }
            PreviousSongResult::Removed(song) => {
                debug!(
                    "⏮️ Re-inserting consumed song: {} - {}",
                    song.artist, song.title
                );

                let insert_idx = current_index.unwrap_or(0);
                queue_manager.insert_song_and_make_current(insert_idx, song.clone())?;

                *self.current_song_id.lock().await = Some(song.id.clone());
                drop(queue_manager);

                self.play_song_direct(engine, &song, server_url, subsonic_credential)
                    .await?;

                debug!(
                    "▶️ Now Playing (re-inserted): {} - {}",
                    song.title, song.artist
                );
                Ok(Some((song, "prev".to_string())))
            }
            PreviousSongResult::None => {
                drop(queue_manager);
                debug!("⏮️ No previous song available");
                Ok(None)
            }
        }
    }

    // ══════════════════════════════════════════════════════════════════════
    //  Current song ID tracking
    // ══════════════════════════════════════════════════════════════════════

    pub async fn set_current_song_id(&self, song_id: Option<String>) {
        *self.current_song_id.lock().await = song_id;
    }

    pub async fn get_current_song_id(&self) -> Option<String> {
        self.current_song_id.lock().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };

    use tokio::sync::Mutex;

    use super::*;
    use crate::{
        audio::engine::CustomAudioEngine,
        services::{queue::QueueManager, state_storage::StateStorage},
        types::song::Song,
    };

    fn temp_storage() -> StateStorage {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "nokkvi_playback_test_{}_{}.redb",
            std::process::id(),
            id
        ));
        let _ = std::fs::remove_file(&path);
        StateStorage::new(path).expect("temp storage")
    }

    fn make_song(id: &str) -> Song {
        Song::test_default(id, &format!("Song {id}"))
    }

    fn manager_with_songs(songs: Vec<Song>, current_index: Option<usize>) -> QueueManager {
        let storage = temp_storage();
        let mut qm = QueueManager::new(storage).expect("queue manager");
        let ids: Vec<String> = songs.iter().map(|s| s.id.clone()).collect();
        qm.pool.insert_many(songs);
        qm.queue.song_ids = ids;
        qm.queue.current_index = current_index;
        qm.rebuild_order_and_sync();
        qm
    }

    /// Path 3 + empty queue: `on_track_finished` must clear `current_song_id`,
    /// reset `current_index` to None, and return `Ok(None)` without panicking.
    /// This is the regression net for Phase 1 (lock-across-await refactor) —
    /// any rewrite of `on_track_finished` must preserve this behavior.
    ///
    /// PipeWire is never touched here: `engine.stop()` early-returns when not
    /// playing, `load_prepared_track` returns Err immediately when no decoder
    /// is prepared, and the test only sets up the empty-queue path.
    #[tokio::test(flavor = "current_thread")]
    async fn playback_callback_path3_empty_queue_clears_state() {
        let song = make_song("only");
        let mut qm = manager_with_songs(vec![song], Some(0));
        // Drain peek so peek_next_song() returns None on the next call:
        // the queue has one song, current_index=0, no repeat → no next song.
        assert!(qm.peek_next_song().is_none());

        let qm = Arc::new(Mutex::new(qm));
        let nav = QueueNavigator::new(qm.clone()).await.expect("navigator");
        // Simulate that "only" is currently playing.
        nav.set_current_song_id(Some("only".to_string())).await;

        let mut engine = CustomAudioEngine::new();
        // Engine is in default Stopped state — `immediate_playing` is false,
        // `load_prepared_track` returns Err (no prepared decoder), `stop` early-returns.
        let result = nav
            .on_track_finished(&mut engine, "http://example", "u=test&p=test")
            .await
            .expect("no error from path 3 empty queue");

        assert!(result.is_none(), "no transition expected on empty queue");
        assert!(
            nav.get_current_song_id().await.is_none(),
            "current_song_id must clear when queue is exhausted"
        );
        assert!(
            qm.lock().await.get_queue().current_index.is_none(),
            "current_index must reset to None"
        );
    }

    // ── decide_transition unit tests (Phase 1 lock-discipline) ──
    //
    // These exercise the decide half in isolation so engine I/O is
    // never triggered. The plan returned describes what `execute_transition`
    // would do — assertions are pure value-equality on the plan + observable
    // queue/navigator state.

    #[tokio::test(flavor = "current_thread")]
    async fn decide_path3_empty_queue_returns_stop() {
        let song = make_song("only");
        let mut qm = manager_with_songs(vec![song], Some(0));
        assert!(qm.peek_next_song().is_none());

        let qm = Arc::new(Mutex::new(qm));
        let nav = QueueNavigator::new(qm.clone()).await.expect("navigator");
        nav.set_current_song_id(Some("only".to_string())).await;

        let mut engine = CustomAudioEngine::new();
        let plan = nav
            .decide_transition(&mut engine, "http://example", "u=test&p=test")
            .await;

        assert!(
            matches!(plan, TrackTransitionPlan::Stop),
            "expected Stop, got {plan:?}"
        );
        assert!(nav.get_current_song_id().await.is_none());
        assert!(qm.lock().await.get_queue().current_index.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn decide_path3_normal_advance_returns_load_fresh() {
        let songs = vec![make_song("a"), make_song("b")];
        let qm = manager_with_songs(songs, Some(0));
        let qm = Arc::new(Mutex::new(qm));
        let nav = QueueNavigator::new(qm.clone()).await.expect("navigator");
        nav.set_current_song_id(Some("a".to_string())).await;

        let mut engine = CustomAudioEngine::new();
        let plan = nav
            .decide_transition(&mut engine, "http://server", "u=test&p=test")
            .await;

        match plan {
            TrackTransitionPlan::LoadFresh {
                stream_url,
                song,
                reason,
            } => {
                assert_eq!(song.id, "b");
                assert_eq!(reason, "gapless");
                assert!(stream_url.starts_with("http://server/rest/stream?id=b"));
            }
            other => panic!("expected LoadFresh, got {other:?}"),
        }

        // Navigator advanced current_song_id to the new track before returning.
        assert_eq!(nav.get_current_song_id().await.as_deref(), Some("b"));
        assert_eq!(qm.lock().await.get_queue().current_index, Some(1));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn decide_path3_repeat_track_returns_load_fresh_with_repeat_reason() {
        let songs = vec![make_song("a")];
        let mut qm = manager_with_songs(songs, Some(0));
        qm.set_repeat(crate::types::queue::RepeatMode::Track)
            .expect("set repeat");

        let qm = Arc::new(Mutex::new(qm));
        let nav = QueueNavigator::new(qm.clone()).await.expect("navigator");
        nav.set_current_song_id(Some("a".to_string())).await;

        let mut engine = CustomAudioEngine::new();
        let plan = nav
            .decide_transition(&mut engine, "http://server", "u=test&p=test")
            .await;

        match plan {
            TrackTransitionPlan::LoadFresh {
                stream_url,
                song,
                reason,
            } => {
                assert_eq!(song.id, "a", "repeat-track plays the same song again");
                assert_eq!(reason, "repeat");
                assert!(stream_url.starts_with("http://server/rest/stream?id=a"));
            }
            other => panic!("expected LoadFresh, got {other:?}"),
        }

        // Repeat-track preserves current_index.
        assert_eq!(qm.lock().await.get_queue().current_index, Some(0));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn decide_path3_no_queued_returns_stop() {
        // Single song at last index, no repeat → peek_next_song() returns
        // None, decide_transition must yield Stop without erroring.
        let songs = vec![make_song("only")];
        let qm = manager_with_songs(songs, Some(0));
        let qm = Arc::new(Mutex::new(qm));
        let nav = QueueNavigator::new(qm.clone()).await.expect("navigator");

        let mut engine = CustomAudioEngine::new();
        let plan = nav
            .decide_transition(&mut engine, "http://server", "u=test&p=test")
            .await;

        assert!(
            matches!(plan, TrackTransitionPlan::Stop),
            "expected Stop, got {plan:?}"
        );
    }

    // ── decide_removal_aftermath unit tests ──
    //
    // These cover the "remove from queue" decision matrix in isolation.
    // The contract: after `QueueManager::remove_songs_by_ids` has already
    // mutated the queue, `decide_removal_aftermath` reads the post-removal
    // state plus the snapshotted `was_playing_id` and decides whether the
    // engine must keep going as-is, swap to a new source, or stop.
    //
    // Engine is never constructed here; the function under test is a pure
    // free function over `QueueManager` + `Option<&str>` + `&[String]`.

    /// Nothing was playing → removal can't have unhooked anything.
    #[test]
    fn removal_aftermath_no_playing_song_returns_no_change() {
        let qm = manager_with_songs(vec![make_song("a"), make_song("b")], None);
        let plan = decide_removal_aftermath(&qm, None, &["a".to_string()]);
        assert_eq!(plan, RemovalAftermath::NoCurrentChange);
    }

    /// Playing song was NOT among the removed → engine should be left alone.
    #[test]
    fn removal_aftermath_other_song_removed_returns_no_change() {
        // Queue: [a, b, c], current = a (idx 0). Remove b. a still plays.
        let mut qm = manager_with_songs(
            vec![make_song("a"), make_song("b"), make_song("c")],
            Some(0),
        );
        qm.remove_song_by_id("b").expect("remove b");

        let plan = decide_removal_aftermath(&qm, Some("a"), &["b".to_string()]);

        assert_eq!(
            plan,
            RemovalAftermath::NoCurrentChange,
            "removing a non-playing song must not retarget the engine",
        );
    }

    /// Playing song was removed and the queue still has songs → the engine
    /// must load the song that the queue now exposes at `current_index`.
    #[test]
    fn removal_aftermath_playing_song_removed_loads_new_current() {
        // Queue: [a, b, c], current = b (idx 1). Remove b.
        // After: [a, c], queue's `remove_song` clamp leaves current_index = 1
        // (now pointing at c). The engine must transition to c.
        let mut qm = manager_with_songs(
            vec![make_song("a"), make_song("b"), make_song("c")],
            Some(1),
        );
        qm.remove_song_by_id("b").expect("remove b");

        let plan = decide_removal_aftermath(&qm, Some("b"), &["b".to_string()]);

        assert_eq!(
            plan,
            RemovalAftermath::LoadNewCurrent {
                new_song_id: "c".to_string(),
                new_index: 1,
            },
            "engine must follow the queue's clamped current_index to the next song",
        );
    }

    /// The playing song was removed in a multi-ID batch → still loads
    /// whatever now occupies `current_index` (immune to ID order).
    #[test]
    fn removal_aftermath_playing_in_batch_loads_new_current() {
        // Queue: [a, b, c, d], current = b (idx 1). Remove [b, d].
        // After: [a, c]. current_index clamped to 1 (c). Engine → c.
        let mut qm = manager_with_songs(
            vec![
                make_song("a"),
                make_song("b"),
                make_song("c"),
                make_song("d"),
            ],
            Some(1),
        );
        qm.remove_songs_by_ids(&["b".to_string(), "d".to_string()])
            .expect("remove batch");

        let plan = decide_removal_aftermath(&qm, Some("b"), &["b".to_string(), "d".to_string()]);

        assert_eq!(
            plan,
            RemovalAftermath::LoadNewCurrent {
                new_song_id: "c".to_string(),
                new_index: 1,
            },
        );
    }

    /// Playing song was the last in queue and gets removed → queue empty,
    /// engine must stop and the navigator's current_song_id must clear.
    #[test]
    fn removal_aftermath_last_song_removed_returns_stop_empty() {
        // Queue: [only], current = 0. Remove only. After: [], current_index = None.
        let mut qm = manager_with_songs(vec![make_song("only")], Some(0));
        qm.remove_song_by_id("only").expect("remove only");

        let plan = decide_removal_aftermath(&qm, Some("only"), &["only".to_string()]);

        assert_eq!(
            plan,
            RemovalAftermath::StopEmpty,
            "empty queue after removing the playing song must stop the engine",
        );
    }

    /// Edge case: every remaining queue song is also removed in the same
    /// batch → still StopEmpty, never panics on `song_ids.get(idx)`.
    #[test]
    fn removal_aftermath_clear_queue_returns_stop_empty() {
        // Queue: [a, b], current = a (idx 0). Remove [a, b]. After: [].
        let mut qm = manager_with_songs(vec![make_song("a"), make_song("b")], Some(0));
        qm.remove_songs_by_ids(&["a".to_string(), "b".to_string()])
            .expect("remove all");

        let plan = decide_removal_aftermath(&qm, Some("a"), &["a".to_string(), "b".to_string()]);

        assert_eq!(plan, RemovalAftermath::StopEmpty);
    }
}
