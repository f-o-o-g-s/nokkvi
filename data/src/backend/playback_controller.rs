//! PlaybackController — audio engine and transport controls
//!
//! Owns the audio engine and queue navigator. Handles play/pause/stop/seek,
//! volume, mode toggles (random/repeat/consume), and gapless playback preparation.

use std::sync::Arc;

use anyhow::Result;
use chrono;
use tokio::sync::{
    Mutex,
    mpsc::{self, UnboundedReceiver},
};
use tracing::debug;

use crate::{
    audio::engine::CustomAudioEngine,
    backend::{queue::QueueService, settings::SettingsService},
    services::{playback::QueueNavigator, task_manager::TaskManager},
};

/// PlaybackController — Owns the audio engine and queue navigator.
///
/// Handles all direct playback operations: play/pause/stop/next/previous,
/// seeking, volume control, mode toggles (random/repeat/consume), and
/// gapless playback preparation.
///
/// Higher-level orchestration ("play album X", "add genre to queue") remains
/// on [`AppService`](super::app_service::AppService).
#[derive(Clone)]
pub struct PlaybackController {
    audio_engine: Arc<Mutex<CustomAudioEngine>>,
    queue_navigator: Arc<Mutex<QueueNavigator>>,
    queue_service: QueueService,
    settings_service: SettingsService,
    task_manager: Arc<TaskManager>,
}

impl std::fmt::Debug for PlaybackController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlaybackController").finish()
    }
}

impl PlaybackController {
    /// Create a new PlaybackController.
    ///
    /// Sets up the audio engine, queue navigator, completion callback for
    /// auto-advance, and engine self-reference for the renderer.
    ///
    /// Returns `(controller, loop_receiver, queue_changed_receiver)`. The caller
    /// should store both receivers and expose them via subscriptions so that
    /// repeat-one loops route to `ScrobbleMessage::TrackLooped` and queue
    /// mutations (consume mode) route to `Message::LoadQueue`.
    pub async fn new(
        queue_service: QueueService,
        settings_service: SettingsService,
        task_manager: Arc<TaskManager>,
    ) -> Result<(Self, UnboundedReceiver<String>, UnboundedReceiver<()>)> {
        let queue_manager = queue_service.queue_manager();
        let queue_navigator = Arc::new(Mutex::new(QueueNavigator::new(queue_manager).await?));
        let audio_engine = Arc::new(Mutex::new(CustomAudioEngine::new()));
        let (loop_tx, loop_rx) = mpsc::unbounded_channel::<String>();
        let (queue_changed_tx, queue_changed_rx) = mpsc::unbounded_channel::<()>();

        // Set up engine reference in renderer (required for on_renderer_finished callback)
        {
            let engine_weak = Arc::downgrade(&audio_engine);
            let mut engine = audio_engine.lock().await;
            engine.set_engine_reference(engine_weak);
        }

        // Set up completion callback to trigger auto-advance on track finish
        {
            let navigator_arc = queue_navigator.clone();
            let engine_arc = audio_engine.clone();
            let queue_vm = queue_service.clone();
            let task_manager_for_callback = task_manager.clone();
            // Move the sender directly into the closure — it lives as long as the
            // completion callback is set on the engine, which the struct owns.
            let loop_tx_cb = loop_tx;
            let queue_changed_tx_cb = queue_changed_tx;

            let mut engine = audio_engine.lock().await;
            engine.set_completion_callback(move |is_loop| {
                let nav = navigator_arc.clone();
                let ea = engine_arc.clone();
                let qvm = queue_vm.clone();
                let tm = task_manager_for_callback.clone();
                let tx = loop_tx_cb.clone();
                let queue_tx = queue_changed_tx_cb.clone();
                tm.spawn_result("track_completion", move || async move {
                    let (url, cred) = qvm.get_server_config().await;
                    if url.is_empty() {
                        debug!(" [COMPLETION] No server config, cannot auto-advance");
                        return Ok::<_, anyhow::Error>(());
                    }
                    let mut engine = ea.lock().await;
                    // Phase 1 lock-discipline: hold the outer `nav` mutex only
                    // for the queue-mutation half of the work. Drop it before
                    // running the engine ops so concurrent navigator calls
                    // (e.g. hotkey-triggered `play_next`) aren't blocked
                    // behind `engine.play()`'s network probe + prebuffer.
                    let plan = {
                        let nav_guard = nav.lock().await;
                        nav_guard
                            .decide_transition(&mut engine, &url, &cred)
                            .await
                    };
                    match QueueNavigator::execute_transition(plan, &mut engine).await {
                        Ok(Some((song, reason))) => {
                            debug!(
                                " [COMPLETION] Auto-advanced to: {} - {} ({})",
                                song.title, song.artist, reason
                            );
                            let song_id = song.id.clone();
                            drop(engine);
                            let _ = qvm.refresh_from_queue().await;
                            // Signal the UI that queue state has changed (post-consume)
                            let _ = queue_tx.send(());

                            // Notify repeat-one loop so the UI can scrobble correctly
                            if is_loop {
                                debug!(
                                    " [COMPLETION] Track looped (repeat-one), notifying scrobble layer: {}",
                                    song_id
                                );
                                let _ = tx.send(song_id);
                            }
                        }
                        Ok(None) => {
                            debug!(" [COMPLETION] No next track, playback stopped");
                            drop(engine);
                            // Refresh queue view so UI shows the consumed state
                            let _ = qvm.refresh_from_queue().await;
                            let _ = queue_tx.send(());
                        }
                        Err(e) => {
                            debug!(" [COMPLETION] Error during auto-advance: {}", e);
                        }
                    }
                    Ok(())
                });
            });
        }

        Ok((
            Self {
                audio_engine,
                queue_navigator,
                queue_service,
                settings_service,
                task_manager,
            },
            loop_rx,
            queue_changed_rx,
        ))
    }

    /// Get a clone of the audio engine Arc (for external progress polling).
    pub fn audio_engine(&self) -> Arc<Mutex<CustomAudioEngine>> {
        self.audio_engine.clone()
    }

    // =========================================================================
    // Transport Controls
    // =========================================================================

    /// Play/pause playback
    pub async fn play_pause(&self) -> Result<()> {
        let mut audio = self.audio_engine.lock().await;
        if audio.playing() {
            audio.pause();
        } else if audio.source().is_empty() {
            // Cold start: no source loaded (e.g. after app restart with persisted queue).
            // Delegate to play() which resolves the current song from the queue.
            drop(audio);
            self.play().await?;
        } else {
            // Source exists but not playing (paused state) — resume directly.
            audio.play().await?;
        }
        Ok(())
    }

    /// Start playback
    pub async fn play(&self) -> Result<()> {
        let mut audio = self.audio_engine.lock().await;

        // If we have a source set, try to play/resume
        if !audio.source().is_empty() {
            audio.play().await?;
            return Ok(());
        }

        // No source set - check if we have a current song to play
        {
            let queue_navigator = self.queue_navigator.lock().await;
            if let Some(song_id) = queue_navigator.get_current_song_id().await {
                drop(queue_navigator);

                let (server_url, subsonic_credential) =
                    self.queue_service.get_server_config().await;
                if server_url.is_empty() {
                    return Ok(());
                }

                // Find the song in the pool (O(1) lookup)
                let queue_manager_arc = self.queue_service.queue_manager();
                let queue_manager = queue_manager_arc.lock().await;
                if let Some(song) = queue_manager.get_song(&song_id) {
                    // Construct streaming URL
                    let stream_url = format!(
                        "{}/rest/stream?id={}&{}&f=json&v=1.8.0&c=nokkvi&_={}",
                        server_url,
                        song.id,
                        subsonic_credential,
                        chrono::Utc::now().timestamp_millis()
                    );

                    // Load and play the track
                    let rg = song.replay_gain.clone();
                    drop(queue_manager);
                    audio.set_pending_replay_gain(rg);
                    audio.load_track(&stream_url).await;
                    audio.play().await?;
                    return Ok(());
                }
            }
        }

        // No current song - try to play the song at the current queue index.
        // If current_index is None but songs exist (e.g. after add_songs() to an
        // empty queue), default to index 0 as a cold-start fallback so the play
        // button works identically to pressing Enter on the first queue slot.
        {
            let queue_manager_arc = self.queue_service.queue_manager();
            let mut queue_manager = queue_manager_arc.lock().await;
            let current_index = queue_manager.get_queue().current_index.or_else(|| {
                if queue_manager.get_queue().song_ids.is_empty() {
                    None
                } else {
                    Some(0)
                }
            });
            let song = current_index
                .and_then(|idx| queue_manager.get_queue().song_ids.get(idx))
                .and_then(|id| queue_manager.get_song(id))
                .cloned();

            // Persist the resolved current_index so the queue navigator and UI
            // stay in sync (mirrors what play_song_from_queue does).
            if let Some(idx) = current_index
                && queue_manager.get_queue().current_index.is_none()
            {
                queue_manager.set_current_index(Some(idx));
                let _ = queue_manager.save_order();
            }
            drop(queue_manager);

            if let Some(song) = song {
                let (server_url, subsonic_credential) =
                    self.queue_service.get_server_config().await;
                if server_url.is_empty() {
                    return Ok(());
                }

                // Construct streaming URL
                let stream_url = format!(
                    "{}/rest/stream?id={}&{}&f=json&v=1.8.0&c=nokkvi&_={}",
                    server_url,
                    song.id,
                    subsonic_credential,
                    chrono::Utc::now().timestamp_millis()
                );

                // Sync reactive current_index for UI highlighting
                self.queue_service.refresh_from_queue().await?;

                // Load and play the track
                audio.set_pending_replay_gain(song.replay_gain.clone());
                audio.load_track(&stream_url).await;
                audio.play().await?;

                // Update navigator's current_song_id so consume/gapless knows what's playing
                let queue_navigator = self.queue_navigator.lock().await;
                queue_navigator
                    .set_current_song_id(Some(song.id.clone()))
                    .await;

                return Ok(());
            }
        }

        Ok(())
    }

    /// Pause playback
    pub async fn pause(&self) -> Result<()> {
        let mut audio = self.audio_engine.lock().await;
        audio.pause();
        Ok(())
    }

    /// Stop playback (pause and reset to beginning)
    pub async fn stop(&self) -> Result<()> {
        let mut audio = self.audio_engine.lock().await;
        audio.stop().await;
        Ok(())
    }

    // =========================================================================
    // Track Navigation
    // =========================================================================

    /// Play next track. Returns `true` if a next track was played, `false` if at end of queue.
    pub async fn next(&self) -> Result<bool> {
        let (server_url, subsonic_credential) = self.queue_service.get_server_config().await;
        if server_url.is_empty() {
            return Ok(false);
        }

        let mut engine = self.audio_engine.lock().await;
        let queue_navigator = self.queue_navigator.lock().await;

        match queue_navigator
            .play_next(&mut engine, &server_url, &subsonic_credential)
            .await
        {
            Ok(result) => {
                let advanced = result.is_some();
                drop(queue_navigator);
                drop(engine);
                // Sync reactive current_index for UI highlighting
                self.queue_service.refresh_from_queue().await?;
                Ok(advanced)
            }
            Err(e) => {
                drop(queue_navigator);
                drop(engine);
                Err(e)
            }
        }
    }

    /// Play previous track
    pub async fn previous(&self) -> Result<()> {
        let (server_url, subsonic_credential) = self.queue_service.get_server_config().await;
        if server_url.is_empty() {
            return Ok(());
        }

        let mut engine = self.audio_engine.lock().await;
        let queue_navigator = self.queue_navigator.lock().await;

        match queue_navigator
            .play_previous(&mut engine, &server_url, &subsonic_credential)
            .await
        {
            Ok(_) => {
                drop(queue_navigator);
                drop(engine);
                // Sync reactive current_index for UI highlighting
                self.queue_service.refresh_from_queue().await?;
                Ok(())
            }
            Err(e) => {
                drop(queue_navigator);
                drop(engine);
                Err(e)
            }
        }
    }

    /// Seek to position
    pub async fn seek(&self, position_seconds: f64) -> Result<()> {
        let position_ms = (position_seconds * 1000.0) as u64;
        let mut audio = self.audio_engine.lock().await;
        audio.seek(position_ms).await;
        Ok(())
    }

    // =========================================================================
    // Volume Control
    // =========================================================================

    /// Set volume (0.0 to 1.0), apply to audio engine, and persist
    pub async fn set_volume(&self, volume: f32) -> Result<()> {
        let mut audio = self.audio_engine.lock().await;
        audio.set_volume(volume as f64);
        drop(audio);
        self.settings_service.set_volume(volume).await?;
        Ok(())
    }

    // =========================================================================
    // Playback Modes
    // =========================================================================

    /// Toggle random mode
    ///
    /// Clears the engine's prepared next-track decoder because toggling shuffle
    /// reshuffles the order array, invalidating any pre-buffered gapless song.
    pub async fn toggle_random(&self) -> Result<bool> {
        let queue_manager_arc = self.queue_service.queue_manager();
        let mut queue_manager = queue_manager_arc.lock().await;
        queue_manager.toggle_shuffle()?;
        let is_random = queue_manager.get_queue().shuffle;
        drop(queue_manager);

        // Invalidate engine-level gapless prep (stale after order change)
        let mut engine = self.audio_engine.lock().await;
        engine.reset_next_track().await;

        Ok(is_random)
    }

    /// Cycle repeat mode: None → Track → Playlist → None
    ///
    /// Clears queued next-song and engine prep because repeat mode affects
    /// what `peek_next_song` returns (e.g. repeat-track → same song).
    pub async fn cycle_repeat(&self) -> Result<(bool, bool)> {
        use crate::types::queue::RepeatMode;

        let queue_manager_arc = self.queue_service.queue_manager();
        let mut queue_manager = queue_manager_arc.lock().await;
        let current_repeat = queue_manager.get_queue().repeat;

        let next_repeat = match current_repeat {
            RepeatMode::None => RepeatMode::Track,
            RepeatMode::Track => RepeatMode::Playlist,
            RepeatMode::Playlist => RepeatMode::None,
        };

        queue_manager.set_repeat(next_repeat)?;
        queue_manager.clear_queued();
        queue_manager.save_order()?;
        drop(queue_manager);

        // Invalidate engine-level gapless prep (stale after mode change)
        let mut engine = self.audio_engine.lock().await;
        engine.reset_next_track().await;

        let repeat = next_repeat == RepeatMode::Track;
        let repeat_queue = next_repeat == RepeatMode::Playlist;
        Ok((repeat, repeat_queue))
    }

    /// Toggle consume mode
    ///
    /// Clears the engine's prepared next-track decoder because consume mode
    /// affects post-transition queue state (the finished song may be removed).
    pub async fn toggle_consume(&self) -> Result<bool> {
        let queue_manager_arc = self.queue_service.queue_manager();
        let mut queue_manager = queue_manager_arc.lock().await;
        queue_manager.toggle_consume()?;
        let consume = queue_manager.get_queue().consume;
        drop(queue_manager);

        // Invalidate engine-level gapless prep (stale after mode change)
        let mut engine = self.audio_engine.lock().await;
        engine.reset_next_track().await;

        Ok(consume)
    }

    /// Get current modes (random, repeat_track, repeat_queue, consume)
    pub async fn get_modes(&self) -> (bool, bool, bool, bool) {
        use crate::types::queue::RepeatMode;

        let queue_manager_arc = self.queue_service.queue_manager();
        let queue_manager = queue_manager_arc.lock().await;
        let queue = queue_manager.get_queue();
        (
            queue.shuffle,
            queue.repeat == RepeatMode::Track,
            queue.repeat == RepeatMode::Playlist,
            queue.consume,
        )
    }

    // =========================================================================
    // Gapless Playback
    // =========================================================================

    /// Prepare next track for gapless playback
    /// Should be called when current track is ~80% complete
    /// Spawns preparation in background to avoid blocking the audio engine/visualizer
    ///
    /// CRITICAL: Downloads the track OUTSIDE the engine lock to prevent visualizer stalls.
    /// The engine lock is only held briefly to store the already-downloaded decoder.
    /// Returns true if preparation was triggered, false if not needed
    pub async fn prepare_next_for_gapless(&self) -> bool {
        // Quick check if already prepared (minimal lock time)
        {
            let engine = self.audio_engine.lock().await;
            if engine.is_next_track_prepared().await {
                return false; // Already prepared
            }
        }

        let (server_url, subsonic_credential) = self.queue_service.get_server_config().await;
        if server_url.is_empty() {
            return false;
        }

        // Get the next track URL from queue manager WITHOUT holding the engine lock
        let (stream_url, replay_gain, is_repeat_track): (
            Option<String>,
            Option<crate::types::song::ReplayGain>,
            bool,
        ) = {
            let queue_manager_arc = self.queue_service.queue_manager();
            let mut queue_manager = queue_manager_arc.lock().await;
            let repeat_track =
                queue_manager.get_queue().repeat == crate::types::queue::RepeatMode::Track;

            if let Some(ref next_result) = queue_manager.peek_next_song() {
                let url = format!(
                    "{}/rest/stream?id={}&{}&f=json&v=1.8.0&c=nokkvi&_={}",
                    server_url,
                    next_result.song.id,
                    subsonic_credential,
                    chrono::Utc::now().timestamp_millis()
                );
                (
                    Some(url),
                    next_result.song.replay_gain.clone(),
                    repeat_track,
                )
            } else {
                (None, None, repeat_track)
            }
        };

        let Some(url) = stream_url else {
            return false;
        };

        // Check if the song ID matches the currently-playing source.
        // Compare extracted song IDs rather than full URLs, because each URL
        // contains a unique `_=timestamp` parameter that makes full-URL
        // comparison always fail even for the same song.
        // Skip this guard in repeat-track mode — same-song prep is intentional there.
        if !is_repeat_track {
            let engine = self.audio_engine.lock().await;
            let current_source = engine.source();
            if !current_source.is_empty() {
                let current_id = current_source
                    .find("id=")
                    .map(|i| &current_source[i + 3..])
                    .and_then(|s| s.find('&').map(|e| &s[..e]).or(Some(s)));
                let next_id = url
                    .find("id=")
                    .map(|i| &url[i + 3..])
                    .and_then(|s| s.find('&').map(|e| &s[..e]).or(Some(s)));
                if current_id.is_some() && current_id == next_id {
                    return false;
                }
            }
        }

        // Spawn the actual download/decode work in a background task
        // This download happens WITHOUT holding the engine lock!
        let audio_engine = self.audio_engine.clone();
        let url_for_task = url.clone();
        let rg_for_task = replay_gain;

        self.task_manager
            .spawn_result("gapless_prep", move || async move {
                // Create and initialize decoder OUTSIDE the engine lock
                // This is the slow part - downloads ~20MB of audio
                let mut decoder = crate::audio::AudioDecoder::default();
                decoder.init(&url_for_task).await?;

                // BRIEF lock to store the already-downloaded decoder
                let mut engine = audio_engine.lock().await;
                engine
                    .store_prepared_decoder(decoder, url_for_task.clone(), rg_for_task)
                    .await;
                drop(engine);
                debug!(" [GAPLESS] Prepared next track: {}", url_for_task);

                Ok::<_, anyhow::Error>(())
            });

        true // Preparation triggered (spawned)
    }

    // =========================================================================
    // Queue + Play (used by AppService orchestration methods)
    // =========================================================================

    /// Core helper: set queue, build stream URL, load and play from a specific index.
    ///
    /// All "play X" flows in `AppService` converge here. This is the single
    /// authoritative definition of "what it means to play a list of songs".
    pub(crate) async fn play_songs_from_index(
        &self,
        songs: Vec<crate::types::song::Song>,
        start_index: usize,
    ) -> Result<()> {
        if songs.is_empty() {
            return Err(anyhow::anyhow!("No songs to play"));
        }

        let play_index = start_index.min(songs.len() - 1);

        // 1. Set queue with songs, starting at play_index
        self.queue_service
            .set_queue(songs.clone(), Some(play_index))
            .await?;

        // 2. Build stream URL for the target song
        let song = &songs[play_index];
        let (server_url, subsonic_credential) = self.queue_service.get_server_config().await;
        let stream_url = crate::utils::artwork_url::build_stream_url(
            &song.id,
            &server_url,
            &subsonic_credential,
        );

        if stream_url.is_empty() {
            return Err(anyhow::anyhow!("Failed to build stream URL"));
        }

        // 3. Load and play
        let mut engine = self.audio_engine.lock().await;
        engine.set_pending_replay_gain(song.replay_gain.clone());
        engine.set_source(stream_url).await;
        engine.play().await?;
        drop(engine);

        // 4. Update navigator's current_song_id so consume mode knows what's playing
        let queue_navigator = self.queue_navigator.lock().await;
        queue_navigator
            .set_current_song_id(Some(song.id.clone()))
            .await;

        Ok(())
    }

    /// Play a song that's already in the queue by its ID and queue index.
    ///
    /// This sets the queue's current index directly and starts playback.
    /// Use this for "jump to song in queue" operations.
    /// The `queue_index` parameter identifies the specific queue position,
    /// avoiding the `index_of` first-match bug with duplicate tracks.
    pub async fn play_song_from_queue(&self, song_id: &str, queue_index: usize) -> Result<()> {
        // 0. Record current song in history before jumping
        let queue_manager = self.queue_service.queue_manager();
        {
            let queue_navigator = self.queue_navigator.lock().await;
            let current_id = queue_navigator.get_current_song_id().await;
            if let Some(ref cid) = current_id {
                let mut qm = queue_manager.lock().await;
                if let Some(current_song) = qm.get_song(cid).cloned() {
                    qm.add_to_history(current_song);
                }
            }
        }

        // 1. Set queue current index directly (no index_of scan needed)
        let mut qm = queue_manager.lock().await;
        qm.set_current_index(Some(queue_index));
        qm.save_order()?;
        drop(qm);

        // 2. Sync the reactive current_index property with queue state
        self.queue_service.refresh_from_queue().await?;

        // 3. Build stream URL and play
        let (server_url, subsonic_credential) = self.queue_service.get_server_config().await;
        let stream_url =
            crate::utils::artwork_url::build_stream_url(song_id, &server_url, &subsonic_credential);

        if stream_url.is_empty() {
            return Err(anyhow::anyhow!("Failed to build stream URL"));
        }

        let rg = {
            let qm = queue_manager.lock().await;
            qm.get_song(song_id).and_then(|s| s.replay_gain.clone())
        };

        let mut engine = self.audio_engine.lock().await;
        engine.set_pending_replay_gain(rg);
        engine.set_source(stream_url).await;
        engine.play().await?;
        drop(engine);

        // Update navigator's current_song_id so consume mode knows what's playing
        let queue_navigator = self.queue_navigator.lock().await;
        queue_navigator
            .set_current_song_id(Some(song_id.to_string()))
            .await;

        Ok(())
    }
}
