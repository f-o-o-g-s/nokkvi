//! PlaybackController — audio engine and transport controls
//!
//! Owns the audio engine and queue navigator. Handles play/pause/stop/seek,
//! volume, mode toggles (random/repeat/consume), and gapless playback preparation.

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::{
    Mutex,
    mpsc::{self, UnboundedReceiver},
};
use tracing::debug;

use crate::{
    audio::engine::CustomAudioEngine,
    backend::{queue::QueueService, settings::SettingsService},
    services::{
        playback::{QueueNavigator, RemovalAftermath},
        queue::PreviousOutcome,
        task_manager::TaskManager,
    },
    utils::url_redaction::redact_subsonic_url,
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
    /// M7/M10 skip supersession counter: stamped (under the engine+navigator
    /// locks) at the START of every manual next/previous, by every click
    /// that plans a fade ([`plan_click_play`], M10 — one shared counter so
    /// skips and clicks supersede each other), and re-checked
    /// before a [`SkipFadePlan`](crate::services::playback::SkipFadePlan)
    /// fires — the plan's decoder builds with NO locks held, so a newer skip
    /// can land mid-build and must win (only the LATEST skip may drive the
    /// engine). Competing NON-skip actions are covered separately by the
    /// source-generation snapshot inside `crossfade_to_next`.
    skip_fade_seq: Arc<std::sync::atomic::AtomicU64>,
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
            // Downgrade to Weak to avoid a strong-Arc cycle: the closure is stored
            // inside the engine itself via `set_completion_callback`, so capturing a
            // strong Arc here would make the engine's refcount never reach zero.
            // Mirror the `engine_weak` pattern used above for `set_engine_reference`.
            let engine_weak = Arc::downgrade(&audio_engine);
            let queue_vm = queue_service.clone();
            let task_manager_for_callback = task_manager.clone();
            // Move the sender directly into the closure — it lives as long as the
            // completion callback is set on the engine, which the struct owns.
            let loop_tx_cb = loop_tx;
            let queue_changed_tx_cb = queue_changed_tx;

            let mut engine = audio_engine.lock().await;
            engine.set_completion_callback(move |is_loop| {
                let nav = navigator_arc.clone();
                let ew = engine_weak.clone();
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
                    let Some(ea) = ew.upgrade() else {
                        // Engine has already been dropped — nothing to advance.
                        return Ok(());
                    };
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
                skip_fade_seq: Arc::new(std::sync::atomic::AtomicU64::new(0)),
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
                    let stream_url = crate::utils::artwork_url::build_stream_url(
                        &song.id,
                        &server_url,
                        &subsonic_credential,
                    );

                    // Load and play the track
                    let rg = song.replay_gain.clone();
                    let expected_ms = song.expected_duration_ms();
                    drop(queue_manager);
                    audio.load_track_with_rg(&stream_url, rg, expected_ms).await;
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
            let current_index = queue_manager.current_index().or_else(|| {
                if queue_manager.is_queue_empty() {
                    None
                } else {
                    Some(0)
                }
            });
            let song = current_index
                .and_then(|idx| queue_manager.song_id_at(idx))
                .and_then(|id| queue_manager.get_song(id))
                .cloned();

            // Persist the resolved current_index so the queue navigator and UI
            // stay in sync (mirrors what play_song_from_queue does). Engine is
            // already locked above, so the next-track-reset effect is
            // discharged in-line against the held lock.
            if let Some(idx) = current_index
                && queue_manager.current_index().is_none()
            {
                let effect = queue_manager.reposition_to_index(Some(idx));
                let _ = queue_manager.save_order();
                effect.apply_locked(&mut audio).await;
            }
            drop(queue_manager);

            if let Some(song) = song {
                let (server_url, subsonic_credential) =
                    self.queue_service.get_server_config().await;
                if server_url.is_empty() {
                    return Ok(());
                }

                // Construct streaming URL
                let stream_url = crate::utils::artwork_url::build_stream_url(
                    &song.id,
                    &server_url,
                    &subsonic_credential,
                );

                // Sync reactive current_index for UI highlighting
                self.queue_service.refresh_from_queue().await?;

                // Load and play the track
                audio
                    .load_track_with_rg(
                        &stream_url,
                        song.replay_gain.clone(),
                        song.expected_duration_ms(),
                    )
                    .await;
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
        use crate::services::playback::NextOutcome;

        let (server_url, subsonic_credential) = self.queue_service.get_server_config().await;
        if server_url.is_empty() {
            return Ok(false);
        }

        let mut engine = self.audio_engine.lock().await;
        let queue_navigator = self.queue_navigator.lock().await;

        // Stamp this skip as the latest (any in-flight skip-fade build is
        // superseded), and resolve the effective mode under the locks.
        let seq = self
            .skip_fade_seq
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        let skip_fade = engine.skip_fade_mode();

        match queue_navigator
            .play_next(&mut engine, &server_url, &subsonic_credential, skip_fade)
            .await
        {
            Ok(NextOutcome::NoNext) => {
                drop(queue_navigator);
                drop(engine);
                // Sync reactive current_index for UI highlighting
                self.queue_service.refresh_from_queue().await?;
                Ok(false)
            }
            Ok(NextOutcome::Played(..)) => {
                drop(queue_navigator);
                drop(engine);
                self.queue_service.refresh_from_queue().await?;
                Ok(true)
            }
            Ok(NextOutcome::FadePlanned(plan)) => {
                // Snapshot for the fire-time staleness check while the lock
                // is still held — this reads the POST-plan value
                // (`plan_skip_fade` bumped it inside `play_next`), so every
                // completion dispatch from before the plan is already stale.
                let generation = engine.source_generation();
                drop(queue_navigator);
                drop(engine);
                // Refresh FIRST — the queue already advanced (cursor, history,
                // consume), so the UI reflects the skip immediately while the
                // incoming decoder builds.
                self.queue_service.refresh_from_queue().await?;
                self.complete_skip_fade(plan, generation, seq).await?;
                Ok(true)
            }
            Err(e) => {
                drop(queue_navigator);
                drop(engine);
                Err(e)
            }
        }
    }

    /// Play previous track
    pub async fn previous(&self) -> Result<PreviousOutcome> {
        let (server_url, subsonic_credential) = self.queue_service.get_server_config().await;
        if server_url.is_empty() {
            return Ok(PreviousOutcome::Stepped);
        }

        let mut engine = self.audio_engine.lock().await;
        let queue_navigator = self.queue_navigator.lock().await;

        let seq = self
            .skip_fade_seq
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        let skip_fade = engine.skip_fade_mode();

        match queue_navigator
            .play_previous(&mut engine, &server_url, &subsonic_credential, skip_fade)
            .await
        {
            Ok((outcome, plan)) => {
                let generation = engine.source_generation();
                drop(queue_navigator);
                drop(engine);
                // A blocked step-back changed nothing, so skip the reactive
                // refresh; otherwise sync current_index for UI highlighting.
                if outcome == PreviousOutcome::Stepped {
                    self.queue_service.refresh_from_queue().await?;
                }
                if let Some(plan) = plan {
                    self.complete_skip_fade(plan, generation, seq).await?;
                }
                Ok(outcome)
            }
            Err(e) => {
                drop(queue_navigator);
                drop(engine);
                Err(e)
            }
        }
    }

    /// Complete a queue-layer [`SkipFadePlan`](crate::services::playback::SkipFadePlan):
    /// build the incoming decoder with NO locks held (invariants 13 + 14 —
    /// through the shared registry via `AudioDecoder::init`, never under the
    /// engine lock), then briefly lock the engine to fire the blend. When
    /// the blend is refused (`Blocked`) or the build failed, fall back to
    /// the boundary out-fade (self-refusing) + a plain hard load so the skip
    /// still lands; when superseded (`Stale` / a newer skip stamped the
    /// sequence), do nothing — the competing action owns the engine.
    ///
    /// Body lives in the module-level [`complete_skip_fade`] so the fallback
    /// branches are testable without constructing a full controller.
    async fn complete_skip_fade(
        &self,
        plan: crate::services::playback::SkipFadePlan,
        generation: u64,
        seq: u64,
    ) -> Result<()> {
        complete_skip_fade(
            &self.audio_engine,
            &self.skip_fade_seq,
            plan,
            generation,
            seq,
        )
        .await
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
        let effect = queue_manager.toggle_shuffle()?;
        let is_random = queue_manager.get_queue().shuffle;
        drop(queue_manager);

        // Invalidate engine-level gapless prep (stale after order change)
        effect.apply_to(&self.audio_engine).await;

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

        // `set_repeat` commits via `QueueWriteGuard::commit_save_order`, which
        // already calls `clear_queued()` + `save_order()` under the guard.
        // Calling them again here would be redundant work.
        let effect = queue_manager.set_repeat(next_repeat)?;
        drop(queue_manager);

        // Invalidate engine-level gapless prep (stale after mode change)
        effect.apply_to(&self.audio_engine).await;

        let repeat = next_repeat == RepeatMode::Track;
        let repeat_queue = next_repeat == RepeatMode::Playlist;
        Ok((repeat, repeat_queue))
    }

    /// Set repeat mode to a specific value (idempotent; used by MPRIS LoopStatus).
    ///
    /// Mirrors `cycle_repeat` but accepts an explicit target instead of
    /// advancing through the cycle. MPRIS clients (`playerctl`, KDE Plasma
    /// media controls, GNOME Shell extensions) emit
    /// `org.mpris.MediaPlayer2.Player.LoopStatus = "Track" | "Playlist" | "None"`
    /// as a *direct* request — routing those through `cycle_repeat` would
    /// land on the wrong mode whenever the current state isn't `None`.
    ///
    /// A no-op set (current == target) still applies the engine effect, which
    /// is harmless: gapless prep is invalidated and the next `peek_next_song`
    /// re-resolves under the same mode.
    pub async fn set_repeat_mode(
        &self,
        mode: crate::types::queue::RepeatMode,
    ) -> Result<(bool, bool)> {
        use crate::types::queue::RepeatMode;

        let queue_manager_arc = self.queue_service.queue_manager();
        let mut queue_manager = queue_manager_arc.lock().await;

        // `set_repeat` commits via `QueueWriteGuard::commit_save_order`, which
        // already calls `clear_queued()` + `save_order()` under the guard.
        let effect = queue_manager.set_repeat(mode)?;
        drop(queue_manager);

        // Invalidate engine-level gapless prep (stale after mode change)
        effect.apply_to(&self.audio_engine).await;

        let repeat = mode == RepeatMode::Track;
        let repeat_queue = mode == RepeatMode::Playlist;
        Ok((repeat, repeat_queue))
    }

    /// Toggle consume mode
    ///
    /// Clears the engine's prepared next-track decoder because consume mode
    /// affects post-transition queue state (the finished song may be removed).
    pub async fn toggle_consume(&self) -> Result<bool> {
        let queue_manager_arc = self.queue_service.queue_manager();
        let mut queue_manager = queue_manager_arc.lock().await;
        let effect = queue_manager.toggle_consume()?;
        let consume = queue_manager.get_queue().consume;
        drop(queue_manager);

        // Invalidate engine-level gapless prep (stale after mode change)
        effect.apply_to(&self.audio_engine).await;

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
        // Quick check if already prepared (minimal lock time); also snapshot
        // the crossfade-policy inputs (min-track floor + album-continuity
        // gate) for the M4 decision below, plus the M8 transition-shaping
        // knobs (bar-snap, gap offset, leading-trim verdict).
        let prep_cfg = {
            let engine = self.audio_engine.lock().await;
            if engine.is_next_track_prepared().await {
                return false; // Already prepared
            }
            engine.transition_prep_cfg()
        };

        let (server_url, subsonic_credential) = self.queue_service.get_server_config().await;
        if server_url.is_empty() {
            return false;
        }

        // Get the next track URL from queue manager WITHOUT holding the engine
        // lock. Also resolve the CURRENT song + the transition reason — the
        // engine boundary carries no Song metadata, so the crossfade-vs-gapless
        // policy decision (M4) is computed here, controller-side.
        struct PeekedPrep {
            url: String,
            replay_gain: Option<crate::types::song::ReplayGain>,
            expected_duration_ms: Option<u64>,
            directives: crate::audio::engine::PreparedTransitionDirectives,
        }
        let (prep, is_repeat_track): (Option<PeekedPrep>, bool) = {
            let queue_manager_arc = self.queue_service.queue_manager();
            let mut queue_manager = queue_manager_arc.lock().await;
            let repeat_track =
                queue_manager.get_queue().repeat == crate::types::queue::RepeatMode::Track;
            let current_song = queue_manager.get_current_song();

            if let Some(peeked) = queue_manager.peek_next_song() {
                let next_song = peeked.song().clone();
                let reason = peeked.reason();
                drop(peeked); // explicit: clears queued; gapless prep proceeds with the captured data
                let url = crate::utils::artwork_url::build_stream_url(
                    &next_song.id,
                    &server_url,
                    &subsonic_credential,
                );
                // The per-transition verdicts ride down to
                // `store_prepared_decoder` (the engine boundary carries no
                // Song metadata). An unresolvable current song can't prove a
                // continuation and carries no BPM — default to not
                // suppressing / no snap (crossfade proceeds exactly as
                // before M4/M8); the gap offset still applies (it is a
                // spacing preference, not a metadata verdict).
                let (decision, outgoing_bpm) = match &current_song {
                    Some(current) => (
                        Some(crate::audio::crossfade_policy::crossfade_decision(
                            current,
                            &next_song,
                            reason,
                            &prep_cfg.policy,
                        )),
                        current.bpm,
                    ),
                    None => (None, None),
                };
                let suppress_crossfade = decision.is_some_and(|d| d.suppresses_crossfade());
                // Bar-snap only shapes a transition that will actually blend.
                let duration_override_ms = if prep_cfg.bar_snap && !suppress_crossfade {
                    crate::audio::crossfade_policy::bar_snapped_crossfade_ms(
                        prep_cfg.crossfade_duration_ms,
                        outgoing_bpm,
                    )
                } else {
                    None
                };
                // "Keep Gapless Albums Seamless" means no gap either —
                // an authored segue stays tight; every other gapless join
                // honors the user's spacing.
                let gap_offset_ms = if decision
                    == Some(
                        crate::audio::crossfade_policy::CrossfadeDecision::GaplessAlbumContinuation,
                    ) {
                    0
                } else {
                    prep_cfg.gap_offset_ms
                };
                (
                    Some(PeekedPrep {
                        url,
                        replay_gain: next_song.replay_gain.clone(),
                        expected_duration_ms: next_song.expected_duration_ms(),
                        directives: crate::audio::engine::PreparedTransitionDirectives {
                            suppress_crossfade,
                            duration_override_ms,
                            gap_offset_ms,
                        },
                    }),
                    repeat_track,
                )
            } else {
                (None, repeat_track)
            }
        };

        let Some(PeekedPrep {
            url,
            replay_gain,
            expected_duration_ms,
            directives,
        }) = prep
        else {
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

        let trim_leading_silence = prep_cfg.trim_leading_silence;
        self.task_manager
            .spawn_result("gapless_prep", move || async move {
                // Create and initialize decoder OUTSIDE the engine lock
                // This is the slow part - downloads ~20MB of audio
                let mut decoder = crate::audio::AudioDecoder::default();
                decoder.set_expected_duration_ms(expected_duration_ms);
                // M8: transition decoders opt into the leading-silence trim
                // (user-initiated loads stay honest; bit-perfect gated off
                // engine-side in `transition_prep_cfg`).
                decoder.set_trim_leading_silence(trim_leading_silence);
                decoder.init(&url_for_task).await?;

                // BRIEF lock to store the already-downloaded decoder
                let mut engine = audio_engine.lock().await;
                engine
                    .store_prepared_decoder(decoder, url_for_task.clone(), rg_for_task, directives)
                    .await;
                drop(engine);
                debug!(
                    " [GAPLESS] Prepared next track: {}",
                    redact_subsonic_url(&url_for_task)
                );

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

        // 1. Set queue with songs, starting at play_index. The returned
        //    `NextTrackResetEffect` is dispatched against the engine
        //    further down where the lock is held.
        let effect = self
            .queue_service
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

        // 3-4. Load, play (or blend, per "Fade on Skip"), discharge the
        //      set_queue reset, and update the navigator so consume mode
        //      knows what's playing.
        self.load_play_and_set_current(&stream_url, Some(song.clone()), effect, song.id.clone())
            .await?;

        Ok(())
    }

    /// Shared engine-load epilogue for the 3 same-shape play primitives:
    /// acquire the engine lock, route the click through "Fade on Skip"
    /// ([`plan_click_play`], M10), then — on the hard route — load the track,
    /// play, discharge the queue mutation's `NextTrackResetEffect` while the
    /// lock is held, drop the lock, and update the navigator's
    /// `current_song_id` (used by consume mode). On the crossfade route the
    /// engine keeps playing the outgoing untouched and the skip-fade plan is
    /// completed via [`Self::complete_skip_fade`] (lock-free decoder build,
    /// then the direct-Active fire).
    ///
    /// `song` carries the clicked song's full metadata for the fade plan +
    /// ReplayGain; `None` (a defensive pool-miss) degrades to the hard route
    /// with no ReplayGain, exactly as before M10.
    ///
    /// EXCLUDES `play_song_direct` (method on `QueueNavigator`, caller-held
    /// `&mut engine`, set-before-load, M7's skip-fade domain),
    /// `apply_removal_aftermath` (conditional resume, no effect), and the
    /// cold-start branch (engine lock already held) — none of those is a
    /// click that starts a different track while one is audibly playing.
    async fn load_play_and_set_current(
        &self,
        stream_url: &str,
        song: Option<crate::types::song::Song>,
        effect: crate::types::next_track_reset::NextTrackResetEffect,
        song_id: String,
    ) -> Result<()> {
        let mut engine = self.audio_engine.lock().await;
        let effect = match plan_click_play(
            &mut engine,
            &self.skip_fade_seq,
            effect,
            song.as_ref(),
            stream_url,
        )
        .await
        {
            ClickPlayRoute::FadePlanned {
                plan,
                generation,
                seq,
            } => {
                drop(engine);
                // Mirror `skip_to_song`: the queue layer names the clicked
                // song BEFORE the unlocked decoder build, so completions
                // deferred by the pending window and UI reads agree with
                // the already-repositioned queue.
                self.queue_navigator
                    .lock()
                    .await
                    .set_current_song_id(Some(song_id))
                    .await;
                return self.complete_skip_fade(*plan, generation, seq).await;
            }
            ClickPlayRoute::BoundaryFadeThenHard(effect) => {
                // M7's ease-out (self-refusing: paused, bit-perfect
                // stream, live blend, drained ring all degrade to the
                // honest instant cut), then the normal hard load below —
                // M2's onset ramp covers the incoming edge.
                engine.run_skip_out_fade().await;
                effect
            }
            ClickPlayRoute::Hard(effect) => effect,
        };
        let (rg, expected_duration_ms) = song.as_ref().map_or((None, None), |s| {
            (s.replay_gain.clone(), s.expected_duration_ms())
        });
        engine
            .load_track_with_rg(stream_url, rg, expected_duration_ms)
            .await;
        engine.play().await?;
        effect.apply_locked(&mut engine).await;
        drop(engine);
        self.queue_navigator
            .lock()
            .await
            .set_current_song_id(Some(song_id))
            .await;
        Ok(())
    }

    /// Whether a play-from-here click should re-anchor the shuffle order
    /// (start a fresh shuffle headed by the clicked track) instead of
    /// repositioning into the existing order — true when the engine is not
    /// actively producing audio (`Stopped` or `Paused`).
    ///
    /// A click in either state begins playback fresh: `load_play_and_set_current`
    /// always calls `engine.play()`, and a `Paused` engine tears down and
    /// restarts on a new source exactly like `Stopped`, so repositioning into a
    /// spent order there would reproduce the tail dead-end. Only an actively
    /// `Playing` engine is a true mid-session listen whose upcoming order must
    /// be preserved. Trade-off: clicking the row of the already-paused track
    /// also reshuffles next-up (otherwise a no-op resume), which is the intended
    /// "fresh start" semantics; resuming via the transport play button does not
    /// route through here and is unaffected.
    fn should_reanchor_for_play(state: crate::audio::engine::PlaybackState) -> bool {
        !matches!(state, crate::audio::engine::PlaybackState::Playing)
    }

    /// Play an already-queued song addressed by its per-row `entry_id`.
    ///
    /// Drift-immune sibling of [`Self::play_song_from_queue`]: the
    /// `entry_id` → (queue_index, song_id) resolution happens under the
    /// queue lock, so a stale UI snapshot cannot send a wrong raw index.
    /// Duplicate-aware — two queue rows that share a `song_id` carry
    /// distinct `entry_id`s, so the user gets the exact instance they
    /// clicked.
    pub async fn play_entry_from_queue(&self, entry_id: u64) -> Result<()> {
        let queue_manager = self.queue_service.queue_manager();

        // Under shuffle, a click that STARTS a new session re-anchors the play
        // order so the chosen track heads a fresh shuffle — otherwise
        // repositioning into a spent order can strand the user at its tail
        // (clicking the last shuffle slot would play once and stop). Scope
        // lives in `should_reanchor_for_play`. Snapshot on the
        // engine's own lock, dropped before the queue lock — the engine lock is
        // never nested under the qm lock.
        let starting_fresh = Self::should_reanchor_for_play(self.audio_engine.lock().await.state());

        // 0+1. Record history, resolve entry_id, reposition — all under
        //      one qm lock so the resolution is atomic with the reposition.
        let (song_id, reposition_effect) = {
            let current_id = self
                .queue_navigator
                .lock()
                .await
                .get_current_song_id()
                .await;

            let mut qm = queue_manager.lock().await;
            if let Some(ref cid) = current_id {
                // Key history by the leaving row's stable entry_id, resolved
                // from the recorded song's OWN row so (song, entry_id) agree.
                qm.add_to_history_by_song_id(cid);
            }

            let queue_index = qm.index_of_entry(entry_id).ok_or_else(|| {
                anyhow::anyhow!("play_entry_from_queue: entry_id {entry_id} not in queue")
            })?;
            let song_id = qm
                .song_id_at(queue_index)
                .map(str::to_owned)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "play_entry_from_queue: song_id missing at queue position {queue_index}"
                    )
                })?;
            let effect = if starting_fresh {
                // Re-anchor under shuffle (no-op reshuffle when shuffle is off,
                // so this stays a plain reposition for sequential playback).
                qm.reanchor_shuffle_to_index(queue_index)
            } else {
                qm.reposition_to_index(Some(queue_index))
            };
            qm.save_order()?;
            (song_id, effect)
        };

        // 2. Sync the reactive current_index property with queue state
        self.queue_service.refresh_from_queue().await?;

        // 3. Build stream URL and play (mirrors play_song_from_queue)
        let (server_url, subsonic_credential) = self.queue_service.get_server_config().await;
        let stream_url = crate::utils::artwork_url::build_stream_url(
            &song_id,
            &server_url,
            &subsonic_credential,
        );

        if stream_url.is_empty() {
            return Err(anyhow::anyhow!("Failed to build stream URL"));
        }

        let song = {
            let qm = queue_manager.lock().await;
            qm.get_song(&song_id).cloned()
        };

        self.load_play_and_set_current(&stream_url, song, reposition_effect, song_id)
            .await?;

        Ok(())
    }

    /// Play a song that's already in the queue by its ID and queue index.
    ///
    /// **Internal/orchestrator use only.** UI handlers must use
    /// [`Self::play_entry_from_queue`] — `queue_index` is drift-prone
    /// across the optimistic-mutation window. The orchestrator
    /// (`queue_orchestrator::enqueue_and_play`) uses this method legitimately:
    /// it appends songs and immediately plays the first new row at the
    /// known just-appended index, so no other mutation has had a chance
    /// to shift positions.
    pub async fn play_song_from_queue(&self, song_id: &str, queue_index: usize) -> Result<()> {
        // 0. Record current song in history before jumping
        let queue_manager = self.queue_service.queue_manager();
        {
            let queue_navigator = self.queue_navigator.lock().await;
            let current_id = queue_navigator.get_current_song_id().await;
            if let Some(ref cid) = current_id {
                let mut qm = queue_manager.lock().await;
                // Key history by the leaving row's stable entry_id,
                // resolved from the recorded song's OWN row.
                qm.add_to_history_by_song_id(cid);
            }
        }

        // 1. Set queue current index directly (no index_of scan needed).
        //    The reposition produces a `NextTrackResetEffect` that is
        //    discharged below where the engine lock is held.
        let mut qm = queue_manager.lock().await;
        let reposition_effect = qm.reposition_to_index(Some(queue_index));
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

        let song = {
            let qm = queue_manager.lock().await;
            qm.get_song(song_id).cloned()
        };

        self.load_play_and_set_current(&stream_url, song, reposition_effect, song_id.to_string())
            .await?;

        Ok(())
    }

    /// Snapshot the navigator's current song ID without holding any other lock.
    ///
    /// Callers use this to capture "what was playing" before a queue mutation
    /// so [`crate::services::playback::decide_removal_aftermath`] can later
    /// detect whether the removal unhooked the engine.
    pub async fn current_song_id(&self) -> Option<String> {
        self.queue_navigator
            .lock()
            .await
            .get_current_song_id()
            .await
    }

    /// Snapshot whether the engine is *genuinely producing audio* right now
    /// (`PlaybackState::Playing`) — as opposed to merely having a navigator
    /// `current_song_id`, which is populated from the persisted queue at
    /// startup even when nothing has ever played.
    ///
    /// Callers snapshot this before a queue mutation so
    /// [`crate::services::playback::decide_removal_aftermath`] can tell a real
    /// "remove the playing track" from "remove the persisted current row of a
    /// stopped/paused app". Reads one `Copy` field under a single,
    /// independently-acquired engine lock that is dropped immediately — never
    /// nested under the navigator or queue lock.
    pub async fn engine_is_playing(&self) -> bool {
        matches!(
            self.audio_engine.lock().await.state(),
            crate::audio::engine::PlaybackState::Playing
        )
    }

    /// Apply a [`RemovalAftermath`] plan to the engine + navigator.
    ///
    /// Called from [`super::app_service::AppService::remove_queue_entries`]
    /// after the queue has already been mutated and the plan has been decided.
    /// Mirrors the engine-load body of [`Self::play_song_from_queue`] but skips
    /// the play-history append (the previous song is being deleted, not skipped).
    ///
    /// Lock discipline: each lock is taken and dropped independently. We never
    /// hold the engine and navigator locks simultaneously, and `qm` is only
    /// held briefly to read the replay-gain.
    pub async fn apply_removal_aftermath(&self, plan: RemovalAftermath) -> Result<()> {
        match plan {
            RemovalAftermath::NoCurrentChange => Ok(()),
            RemovalAftermath::StopEmpty => {
                {
                    let mut engine = self.audio_engine.lock().await;
                    engine.stop().await;
                }
                self.queue_navigator
                    .lock()
                    .await
                    .set_current_song_id(None)
                    .await;
                debug!("⏹️ Queue emptied by removal — engine stopped");
                Ok(())
            }
            RemovalAftermath::LoadNewCurrent {
                new_song_id,
                new_index: _,
                resume,
            } => {
                let (replay_gain, expected_ms) = {
                    let qm_arc = self.queue_service.queue_manager();
                    let qm = qm_arc.lock().await;
                    qm.get_song(&new_song_id).map_or((None, None), |s| {
                        (s.replay_gain.clone(), s.expected_duration_ms())
                    })
                };

                let (server_url, subsonic_credential) =
                    self.queue_service.get_server_config().await;
                let stream_url = crate::utils::artwork_url::build_stream_url(
                    &new_song_id,
                    &server_url,
                    &subsonic_credential,
                );
                if stream_url.is_empty() {
                    return Err(anyhow::anyhow!(
                        "Failed to build stream URL for removal-aftermath transition"
                    ));
                }

                {
                    // Always swap the engine source to the new current so the
                    // engine never keeps streaming (or stays cued on) the
                    // deleted track. `load_track_with_rg` on a stopped/paused
                    // engine does no network I/O and starts no renderer — the
                    // decoder only initialises inside `play()`. Resume playback
                    // ONLY when the engine was genuinely playing: a stopped or
                    // paused app must not start playing just because its
                    // current row was removed.
                    let mut engine = self.audio_engine.lock().await;
                    engine
                        .load_track_with_rg(&stream_url, replay_gain, expected_ms)
                        .await;
                    if resume {
                        engine.play().await?;
                    }
                }

                self.queue_navigator
                    .lock()
                    .await
                    .set_current_song_id(Some(new_song_id.clone()))
                    .await;
                if resume {
                    debug!("▶️ Removal advanced engine to new current: {}", new_song_id);
                } else {
                    debug!(
                        "⏸️ Removal re-cued stopped/paused engine to new current (no autoplay): {}",
                        new_song_id
                    );
                }
                Ok(())
            }
        }
    }

    /// Cue a freshly-pulled server queue onto the engine ("cue, don't play").
    ///
    /// Mirrors [`Self::apply_removal_aftermath`]'s `LoadNewCurrent` arm — the
    /// shipped "current row changed under a live engine" primitive — because
    /// `set_queue` mutates only the queue model + reactives: the engine still
    /// holds the pre-pull song and `play()` would resume it (its non-empty
    /// source early return), and any bare `seek()` would seek the OLD song.
    /// Deliberately NOT routed through `play_song_from_queue` /
    /// `load_play_and_set_current`: their `plan_click_play` fade routes would
    /// blend into the pulled song at position 0 and the follow-up seek would
    /// cancel the live blend, restoring the outgoing — the pre-pull song.
    ///
    /// - `was_playing == true`: hard-load + `play()` — the fresh-start branch
    ///   consumes the pending offset so the decoder sits at `position_ms`
    ///   BEFORE the renderer starts (no position-0 audio).
    /// - `was_playing == false`: stage without playing (no network I/O, no
    ///   decoder until `play()`); the armed offset makes the user's next Play
    ///   resume mid-song at the server-saved position.
    ///
    /// Every pull takes this ONE uniform reload+arm path — including a pull
    /// of the song this engine is already playing (pull-back of this
    /// device's own push): stream URLs carry a per-call cache-buster, so
    /// `set_source` always treats the load as a fresh source. That also
    /// makes repeated pulls idempotent (each restages and re-arms).
    ///
    /// The caller (`AppService::pull_queue`) has already anchored the queue
    /// cursor via `set_queue` and discharged the `NextTrackResetEffect`; this
    /// method finishes the job by syncing the remaining two sources of truth
    /// (engine source, navigator `current_song_id`).
    pub async fn cue_pulled_queue(
        &self,
        song_id: &str,
        position_ms: i64,
        was_playing: bool,
    ) -> Result<()> {
        let (replay_gain, expected_ms) = {
            let qm_arc = self.queue_service.queue_manager();
            let qm = qm_arc.lock().await;
            qm.get_song(song_id).map_or((None, None), |s| {
                (s.replay_gain.clone(), s.expected_duration_ms())
            })
        };

        let (server_url, subsonic_credential) = self.queue_service.get_server_config().await;
        let stream_url =
            crate::utils::artwork_url::build_stream_url(song_id, &server_url, &subsonic_credential);
        if stream_url.is_empty() {
            return Err(anyhow::anyhow!(
                "Failed to build stream URL for pulled queue"
            ));
        }

        {
            let mut engine = self.audio_engine.lock().await;
            engine
                .load_track_with_rg(&stream_url, replay_gain, expected_ms)
                .await;
            if position_ms > 0 {
                // Consumed inside play()'s fresh-start branch — on the
                // playing branch below immediately, on the paused/stopped
                // branch at the user's next Play. set_source (inside the
                // load) cleared any stale offset first, so the arm is
                // always scoped to exactly this staged track.
                engine.set_pending_start_ms(position_ms as u64);
            }
            if was_playing {
                engine.play().await?;
            }
        }

        self.queue_navigator
            .lock()
            .await
            .set_current_song_id(Some(song_id.to_string()))
            .await;
        debug!(
            "🌊 Pulled queue cued: song={song_id} position={position_ms}ms was_playing={was_playing}"
        );
        Ok(())
    }
}

/// How a user click that starts a track should reach the engine (M10 —
/// clicks honor "Fade on Skip"). Decided by [`plan_click_play`] under the
/// engine lock; the two hard variants carry the still-undischarged
/// [`NextTrackResetEffect`](crate::types::next_track_reset::NextTrackResetEffect)
/// so the hard path keeps today's exact apply-after-play ordering.
enum ClickPlayRoute {
    /// Hard load + play — the historical path, byte-identical.
    Hard(crate::types::next_track_reset::NextTrackResetEffect),
    /// Run M7's `run_skip_out_fade` ease-out first (self-refusing), then the
    /// hard load + play ("Fade on Skip" = Boundary Fade).
    BoundaryFadeThenHard(crate::types::next_track_reset::NextTrackResetEffect),
    /// `plan_skip_fade` ran under the engine lock ("Fade on Skip" =
    /// Crossfade, viable): the caller must complete the blend via
    /// [`complete_skip_fade`] — build the incoming decoder with NO locks
    /// held, then fire `crossfade_to_next`. The effect was already
    /// discharged at plan time (its `reset_next_track` must precede the
    /// fire, never follow it — it would cancel the blend it belongs to).
    FadePlanned {
        /// Boxed: `SkipFadePlan` carries a full `Song`, which would bloat
        /// the two lean hard variants (clippy `large_enum_variant`).
        plan: Box<crate::services::playback::SkipFadePlan>,
        generation: u64,
        seq: u64,
    },
}

/// M10 phase 1 for the click paths (queue click / play-from-here /
/// browse-view play): decide, UNDER the engine lock, how a user click that
/// starts a track honors "Fade on Skip" — the same setting M7 wired into
/// manual Next/Previous.
///
/// Crossfade mode with the engine audibly playing a finite stream (and not
/// bit-perfect Strict — [`CustomAudioEngine::click_skip_crossfade_viable`])
/// mirrors `QueueNavigator::skip_to_song`'s plan arm: stamp this click as
/// the latest manual skip (`skip_fade_seq` — a competing skip or click
/// during the unlocked build must win), discharge the click's queue-mutation
/// `NextTrackResetEffect` (cancelling any live blend so nothing can finalize
/// against the just-repositioned/replaced queue during the build, while the
/// outgoing's decode loop keeps producing), then `plan_skip_fade` (cancel +
/// generation bump + pending-window latch). Boundary Fade mode routes
/// through the M7 ease-out before the hard load. Everything else — mode Off,
/// paused/stopped, an infinite (radio) outgoing (M6's switch-fade domain),
/// bit-perfect Strict, or a metadata-less click — takes today's hard path
/// unchanged.
///
/// Module-level (over `&mut CustomAudioEngine` + the supersession counter)
/// so the routing matrix is unit testable without a full controller.
async fn plan_click_play(
    engine: &mut CustomAudioEngine,
    skip_fade_seq: &std::sync::atomic::AtomicU64,
    effect: crate::types::next_track_reset::NextTrackResetEffect,
    song: Option<&crate::types::song::Song>,
    stream_url: &str,
) -> ClickPlayRoute {
    use crate::types::player_settings::FadeOnSkip;

    match engine.skip_fade_mode() {
        FadeOnSkip::Crossfade if engine.click_skip_crossfade_viable() => {
            let Some(song) = song else {
                return ClickPlayRoute::Hard(effect);
            };
            // Stamp this click as the latest manual skip while the lock is
            // held (mirrors `PlaybackController::next`) — a competing skip
            // or click during the unlocked decoder build must win.
            let seq = skip_fade_seq.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            // Discharge the click's queue-mutation reset BEFORE planning:
            // its `reset_next_track` cancels any live blend / prepared
            // transition from the pre-click queue, so nothing can finalize
            // against the just-repositioned (or replaced) queue during the
            // build. The outgoing's PRIMARY decode loop keeps producing —
            // its liveness rides `decode_loop`, not this reset.
            effect.apply_locked(engine).await;
            // Plan-time invalidation (M7 phase 1): cancel again
            // (idempotent), bump the source generation, latch the
            // generation-keyed pending window so completions dispatched
            // during the build are deferred instead of double-advancing the
            // already-repositioned queue.
            engine.plan_skip_fade().await;
            let generation = engine.source_generation();
            debug!(
                "🔀 Click fade planned: {} - {} (id: {})",
                song.title, song.artist, song.id
            );
            ClickPlayRoute::FadePlanned {
                plan: Box::new(crate::services::playback::SkipFadePlan {
                    song: song.clone(),
                    reason: crate::services::queue::TransitionReason::Click,
                    stream_url: stream_url.to_string(),
                }),
                generation,
                seq,
            }
        }
        FadeOnSkip::BoundaryFade if engine.skip_crossfade_viable() => {
            ClickPlayRoute::BoundaryFadeThenHard(effect)
        }
        FadeOnSkip::Off | FadeOnSkip::BoundaryFade | FadeOnSkip::Crossfade => {
            ClickPlayRoute::Hard(effect)
        }
    }
}

/// Body of [`PlaybackController::complete_skip_fade`] — see its doc. A
/// module-level function over the shared engine mutex + supersession counter
/// (rather than `&self`) so the phase-3 fire/fallback branches are unit
/// testable without a full controller.
async fn complete_skip_fade(
    audio_engine: &Mutex<CustomAudioEngine>,
    skip_fade_seq: &std::sync::atomic::AtomicU64,
    plan: crate::services::playback::SkipFadePlan,
    generation: u64,
    seq: u64,
) -> Result<()> {
    // Phase 2 — the slow network part, no locks held.
    let mut decoder = crate::audio::AudioDecoder::default();
    decoder.set_expected_duration_ms(plan.song.expected_duration_ms());
    let built = decoder.init(&plan.stream_url).await;

    // Phase 3 — brief engine lock to fire (or fall back).
    let mut engine = audio_engine.lock().await;
    if skip_fade_seq.load(std::sync::atomic::Ordering::SeqCst) != seq {
        debug!("🔀 [SKIP FADE] Superseded by a newer skip — abandoning build");
        // The superseding action normally owns the engine and closes the
        // window with its own generation bump — but a NO-OP skip (Next at
        // the end of the queue, Previous with nothing to step back to)
        // stamps the sequence without ever touching the engine, and THIS
        // plan's latch would stay matched to the live generation forever,
        // deferring every end-of-track completion at `on_renderer_finished`
        // (silence, no advance, UI stuck playing). Close the plan's own
        // window; the generation gate inside leaves a newer plan's re-latch
        // (always at a strictly greater generation) untouched.
        engine.close_skip_fade_window(generation);
        return Ok(());
    }

    match built {
        Ok(()) => {
            use crate::audio::engine::SkipFadeOutcome;
            match engine
                .crossfade_to_next(
                    decoder,
                    plan.stream_url.clone(),
                    plan.song.replay_gain.clone(),
                    generation,
                )
                .await
            {
                SkipFadeOutcome::Fired => {
                    debug!(
                        "▶️ Now Playing: {} - {} ({}, skip fade)",
                        plan.song.title, plan.song.artist, plan.reason
                    );
                    return Ok(());
                }
                SkipFadeOutcome::Stale => return Ok(()),
                SkipFadeOutcome::Blocked => {
                    debug!("🔀 [SKIP FADE] Blend blocked — boundary fallback");
                }
            }
        }
        Err(e) => {
            debug!("🔀 [SKIP FADE] Incoming decoder build failed ({e}) — hard fallback");
        }
    }

    // Fallback: the queue already advanced, so the skip MUST still land.
    // Guard against a competing action that took the engine while the build
    // ran (its load owns the state).
    if engine.source_generation() != generation {
        return Ok(());
    }
    // A Stop or Pause pressed during the unlocked build window flips
    // transport state WITHOUT bumping the source generation (only source
    // CHANGES bump), so the guard above cannot see it — and it must WIN:
    // hard-loading + `play()` here would audibly override the user's
    // action. Stage the skip target as the engine source WITHOUT playing
    // (`load_track_with_rg` on a silent engine does no network I/O): the
    // queue already names the target, and the engine's stale source would
    // otherwise make a later Play resume the OUTGOING against the advanced
    // queue. The `set_source` bump also closes the pending window.
    if !engine.immediate_playing() {
        engine
            .load_track_with_rg(
                &plan.stream_url,
                plan.song.replay_gain.clone(),
                plan.song.expected_duration_ms(),
            )
            .await;
        debug!(
            "⏹️ [SKIP FADE] Engine stopped/paused during build — staged {} - {} without playing",
            plan.song.title, plan.song.artist
        );
        return Ok(());
    }
    engine.run_skip_out_fade().await;
    engine
        .load_track_with_rg(
            &plan.stream_url,
            plan.song.replay_gain.clone(),
            plan.song.expected_duration_ms(),
        )
        .await;
    engine.play().await?;
    debug!(
        "▶️ Now Playing: {} - {} ({}, skip-fade fallback)",
        plan.song.title, plan.song.artist, plan.reason
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::engine::{CustomAudioEngine, PlaybackState};

    /// Locks the play-from-here re-anchor gate contract across every engine
    /// state: a stopped OR paused engine starts a fresh shuffle; only an
    /// actively playing listen keeps the plain reposition (so its upcoming
    /// order isn't re-randomized). Guards against a refactor silently flipping
    /// the `matches!` or swapping in the (differently-scoped) `engine_is_playing()`.
    #[test]
    fn reanchor_gate_reanchors_unless_playing() {
        assert!(
            PlaybackController::should_reanchor_for_play(PlaybackState::Stopped),
            "Stopped must re-anchor (fresh shuffle)"
        );
        assert!(
            PlaybackController::should_reanchor_for_play(PlaybackState::Paused),
            "Paused must re-anchor — a click there restarts playback fresh"
        );
        assert!(
            !PlaybackController::should_reanchor_for_play(PlaybackState::Playing),
            "Playing must keep plain reposition (mid-session listen)"
        );
    }

    /// A `SkipFadePlan` aimed at a port that never listens (connection
    /// refused — fast + offline-safe), so the phase-2 decoder build fails
    /// deterministically and `complete_skip_fade` runs its fallback branch.
    fn unreachable_plan() -> crate::services::playback::SkipFadePlan {
        crate::services::playback::SkipFadePlan {
            song: crate::types::song::Song::test_default("b", "Song b"),
            reason: crate::services::queue::TransitionReason::Next,
            stream_url: "http://127.0.0.1:9/rest/stream?id=b".to_string(),
        }
    }

    /// M7 review cycle 1 — a Stop pressed during the unlocked decoder-build
    /// window must WIN: stop flips transport state without bumping the
    /// source generation, so the fallback's generation guard cannot see it.
    /// The fallback must not hard-load + `play()` (audible override of the
    /// user's stop); it stages the skip target as the engine source WITHOUT
    /// playing, so a later Play starts the target the queue already names —
    /// not the stale outgoing.
    ///
    /// (On the unreachable test URL a `play()` attempt would surface as an
    /// `Err` from decoder init — `is_ok()` therefore pins "no play attempt".)
    #[tokio::test(flavor = "multi_thread")]
    async fn skip_fade_fallback_respects_stop_during_build() {
        let engine = Arc::new(Mutex::new(CustomAudioEngine::new()));
        let seq = std::sync::atomic::AtomicU64::new(1);
        let generation;
        {
            let mut e = engine.lock().await;
            e.force_playing_for_test();
            e.plan_skip_fade().await;
            generation = e.source_generation();
            // The user presses Stop while the incoming decoder builds
            // (locks released).
            e.stop().await;
            assert!(!e.playing(), "precondition: the user stop landed");
        }

        let result = complete_skip_fade(&engine, &seq, unreachable_plan(), generation, 1).await;

        let e = engine.lock().await;
        assert!(
            result.is_ok(),
            "the fallback must not attempt play() after a user stop: {result:?}"
        );
        assert!(
            !e.playing(),
            "the user's Stop must win over the skip fallback"
        );
        assert!(
            e.is_playing_source("http://127.0.0.1:9/rest/stream?id=b"),
            "the skip target must be staged as the engine source (queue is \
             already on it; a later Play then starts IT, not the outgoing)"
        );
    }

    /// M7 review cycle 1 — the Pause variant of the same window: pre-M7,
    /// Next-then-Pause ended silent on the new track; the fallback must not
    /// end PLAYING it. Like the stop case, the target is staged un-played.
    #[tokio::test(flavor = "multi_thread")]
    async fn skip_fade_fallback_respects_pause_during_build() {
        let engine = Arc::new(Mutex::new(CustomAudioEngine::new()));
        let seq = std::sync::atomic::AtomicU64::new(7);
        let generation;
        {
            let mut e = engine.lock().await;
            e.force_playing_for_test();
            e.plan_skip_fade().await;
            generation = e.source_generation();
            e.pause();
            assert!(e.immediate_paused(), "precondition: the user pause landed");
        }

        let result = complete_skip_fade(&engine, &seq, unreachable_plan(), generation, 7).await;

        let e = engine.lock().await;
        assert!(
            result.is_ok(),
            "the fallback must not attempt play() after a user pause: {result:?}"
        );
        assert!(
            !e.playing(),
            "the user's Pause must win — the skip fallback must not resume"
        );
        assert!(
            e.is_playing_source("http://127.0.0.1:9/rest/stream?id=b"),
            "the skip target must be staged so a later Play starts it"
        );
    }

    /// M10 review cycle 1 — a competing NO-OP skip must not strand the
    /// pending window. Next at the end of the queue (or Previous with
    /// nothing to step back to) stamps `skip_fade_seq` WITHOUT touching the
    /// engine, so no generation bump ever un-matches an in-flight plan's
    /// latch. When that plan's build is then abandoned at the seq gate, the
    /// abandon must close its own window — otherwise every end-of-track
    /// completion defers forever at `on_renderer_finished` (silence, no
    /// advance, UI stuck playing) until the next source-changing action.
    #[tokio::test(flavor = "multi_thread")]
    async fn skip_fade_abandon_after_noop_skip_closes_pending_window() {
        let engine = Arc::new(Mutex::new(CustomAudioEngine::new()));
        let seq = std::sync::atomic::AtomicU64::new(1);
        let generation;
        {
            let mut e = engine.lock().await;
            e.force_playing_for_test();
            // The click's plan (seq 1): window latched at generation G.
            e.plan_skip_fade().await;
            generation = e.source_generation();
        }
        // A competing no-op skip stamps the sequence counter — exactly what
        // `next()`'s NoNext arm does engine-wise: no plan, no load, no bump.
        seq.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        let result = complete_skip_fade(&engine, &seq, unreachable_plan(), generation, 1).await;

        let e = engine.lock().await;
        assert!(result.is_ok(), "abandon must be clean: {result:?}");
        assert_eq!(
            e.source_generation(),
            generation,
            "precondition: the no-op skip left the generation unmoved, so \
             only the abandon branch itself can ever close the window"
        );
        assert!(
            !e.skip_fade_window_pending(),
            "the seq-abandon must close the plan's own pending window — a \
             stranded latch defers every end-of-track completion forever"
        );
    }

    /// The abandon-side close must touch ONLY the abandoned plan's window: a
    /// newer plan re-latches at a strictly greater generation (its
    /// `plan_skip_fade` bump), and the stale build's abandon must leave that
    /// newer window open for the newer completion to consume.
    #[tokio::test(flavor = "multi_thread")]
    async fn skip_fade_abandon_leaves_newer_plan_window_latched() {
        let engine = Arc::new(Mutex::new(CustomAudioEngine::new()));
        let seq = std::sync::atomic::AtomicU64::new(1);
        let stale_generation;
        {
            let mut e = engine.lock().await;
            e.force_playing_for_test();
            e.plan_skip_fade().await; // plan A (seq 1)
            stale_generation = e.source_generation();
            // Plan B supersedes while A's build runs: new seq + new latch.
            seq.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            e.plan_skip_fade().await;
        }

        let result =
            complete_skip_fade(&engine, &seq, unreachable_plan(), stale_generation, 1).await;

        let e = engine.lock().await;
        assert!(result.is_ok(), "abandon must be clean: {result:?}");
        assert!(
            e.skip_fade_window_pending(),
            "plan B's window (latched at a newer generation) must survive \
             plan A's abandon"
        );
    }

    // ── M10 — clicks honor "Fade on Skip": plan_click_play routing matrix ──
    //
    // The routing is decided UNDER the engine lock by the module-level
    // `plan_click_play` (mirroring `complete_skip_fade`'s testable shape).
    // These tests drive it with a bare engine — no audio device, no network.

    fn click_song(id: &str) -> crate::types::song::Song {
        crate::types::song::Song::test_default(id, &format!("Song {id}"))
    }

    fn effect() -> crate::types::next_track_reset::NextTrackResetEffect {
        crate::types::next_track_reset::NextTrackResetEffect::new()
    }

    /// Crossfade mode + audibly playing a finite stream: the click must take
    /// the M7 plan path — `plan_skip_fade` runs under the lock (source
    /// generation bumped, pending window latched), the seq counter stamps
    /// this click as the latest skip, the plan carries the clicked song, and
    /// the engine's outgoing source is left untouched for the blend.
    #[tokio::test(flavor = "current_thread")]
    async fn click_crossfade_mode_playing_plans_fade() {
        let mut engine = CustomAudioEngine::new();
        engine.force_playing_for_test();
        engine.set_skip_fade(crate::types::player_settings::FadeOnSkip::Crossfade, 2);
        let seq_counter = std::sync::atomic::AtomicU64::new(0);
        let gen_before = engine.source_generation();
        let song = click_song("b");

        let route = plan_click_play(
            &mut engine,
            &seq_counter,
            effect(),
            Some(&song),
            "http://server/rest/stream?id=b",
        )
        .await;

        let ClickPlayRoute::FadePlanned {
            plan,
            generation,
            seq,
        } = route
        else {
            panic!("expected FadePlanned for a crossfade-mode click while playing");
        };
        assert_eq!(plan.song.id, "b");
        assert_eq!(
            plan.reason,
            crate::services::queue::TransitionReason::Click,
            "the click plan carries the click reason for its log line"
        );
        assert_eq!(plan.stream_url, "http://server/rest/stream?id=b");
        assert_eq!(seq, 1, "the click stamps itself as the latest skip");
        assert_eq!(
            generation,
            gen_before + 1,
            "plan_skip_fade must invalidate in-flight completions AT PLAN TIME"
        );
        assert_eq!(engine.source_generation(), generation);
        assert!(
            engine.source().is_empty(),
            "the engine must keep the outgoing untouched (no load happened)"
        );
        assert!(
            engine.playing(),
            "the outgoing keeps playing until the fire"
        );
    }

    /// Mode Off is the historical path, byte-identical: hard route, no seq
    /// stamp, no generation bump, no window latch.
    #[tokio::test(flavor = "current_thread")]
    async fn click_mode_off_stays_hard() {
        let mut engine = CustomAudioEngine::new();
        engine.force_playing_for_test();
        let seq_counter = std::sync::atomic::AtomicU64::new(0);
        let gen_before = engine.source_generation();
        let song = click_song("b");

        let route = plan_click_play(
            &mut engine,
            &seq_counter,
            effect(),
            Some(&song),
            "http://server/rest/stream?id=b",
        )
        .await;

        assert!(matches!(route, ClickPlayRoute::Hard(_)));
        assert_eq!(engine.source_generation(), gen_before);
        assert_eq!(seq_counter.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    /// A stopped app must not fade — clicks there stay instant (hard route).
    #[tokio::test(flavor = "current_thread")]
    async fn click_crossfade_mode_stopped_stays_hard() {
        let mut engine = CustomAudioEngine::new();
        engine.set_skip_fade(crate::types::player_settings::FadeOnSkip::Crossfade, 2);
        let seq_counter = std::sync::atomic::AtomicU64::new(0);
        let gen_before = engine.source_generation();
        let song = click_song("b");

        let route = plan_click_play(
            &mut engine,
            &seq_counter,
            effect(),
            Some(&song),
            "http://server/rest/stream?id=b",
        )
        .await;

        assert!(matches!(route, ClickPlayRoute::Hard(_)));
        assert_eq!(engine.source_generation(), gen_before);
    }

    /// A paused app must not fade either — there is nothing audible to blend.
    #[tokio::test(flavor = "current_thread")]
    async fn click_crossfade_mode_paused_stays_hard() {
        let mut engine = CustomAudioEngine::new();
        engine.force_playing_for_test();
        engine.pause();
        assert!(engine.immediate_paused(), "precondition");
        engine.set_skip_fade(crate::types::player_settings::FadeOnSkip::Crossfade, 2);
        let seq_counter = std::sync::atomic::AtomicU64::new(0);
        let song = click_song("b");

        let route = plan_click_play(
            &mut engine,
            &seq_counter,
            effect(),
            Some(&song),
            "http://server/rest/stream?id=b",
        )
        .await;

        assert!(matches!(route, ClickPlayRoute::Hard(_)));
    }

    /// Clicking out of a playing radio stream is M6's switch-fade domain —
    /// the click path must not double-fade (hard route; `set_source`'s
    /// radio→queue edge handles the fade when enabled).
    #[tokio::test(flavor = "current_thread")]
    async fn click_crossfade_mode_radio_stays_hard() {
        let mut engine = CustomAudioEngine::new();
        engine.force_playing_for_test();
        engine.force_infinite_for_test();
        engine.set_skip_fade(crate::types::player_settings::FadeOnSkip::Crossfade, 2);
        let seq_counter = std::sync::atomic::AtomicU64::new(0);
        let gen_before = engine.source_generation();
        let song = click_song("b");

        let route = plan_click_play(
            &mut engine,
            &seq_counter,
            effect(),
            Some(&song),
            "http://server/rest/stream?id=b",
        )
        .await;

        assert!(matches!(route, ClickPlayRoute::Hard(_)));
        assert_eq!(engine.source_generation(), gen_before);
    }

    /// Bit-perfect Strict refuses EVERY blend at the fire's format gate, so
    /// a click must not even plan (a plan would buy a wasted network decoder
    /// build before the same hard cut) — today's path, byte-identical.
    #[tokio::test(flavor = "current_thread")]
    async fn click_crossfade_mode_bit_perfect_strict_stays_hard() {
        let mut engine = CustomAudioEngine::new();
        engine.force_playing_for_test();
        engine
            .set_bit_perfect(crate::types::player_settings::BitPerfectMode::Strict)
            .await;
        engine.set_skip_fade(crate::types::player_settings::FadeOnSkip::Crossfade, 2);
        let seq_counter = std::sync::atomic::AtomicU64::new(0);
        let gen_before = engine.source_generation();
        let song = click_song("b");

        let route = plan_click_play(
            &mut engine,
            &seq_counter,
            effect(),
            Some(&song),
            "http://server/rest/stream?id=b",
        )
        .await;

        assert!(matches!(route, ClickPlayRoute::Hard(_)));
        assert_eq!(engine.source_generation(), gen_before);
        assert_eq!(seq_counter.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    /// Relaxed is NOT pre-gated: its blend verdict needs the incoming
    /// format, which only exists after the build — the click must plan and
    /// let the fire's `crossfade_blocked` decide.
    #[tokio::test(flavor = "current_thread")]
    async fn click_crossfade_mode_bit_perfect_relaxed_still_plans() {
        let mut engine = CustomAudioEngine::new();
        engine.force_playing_for_test();
        engine
            .set_bit_perfect(crate::types::player_settings::BitPerfectMode::Relaxed)
            .await;
        engine.set_skip_fade(crate::types::player_settings::FadeOnSkip::Crossfade, 2);
        let seq_counter = std::sync::atomic::AtomicU64::new(0);
        let song = click_song("b");

        let route = plan_click_play(
            &mut engine,
            &seq_counter,
            effect(),
            Some(&song),
            "http://server/rest/stream?id=b",
        )
        .await;

        assert!(matches!(route, ClickPlayRoute::FadePlanned { .. }));
    }

    /// Boundary Fade mode: the click routes through the M7 ease-out before
    /// the hard load (the caller runs `run_skip_out_fade`, which self-refuses
    /// when there is nothing audible to fade).
    #[tokio::test(flavor = "current_thread")]
    async fn click_boundary_mode_playing_routes_through_ease_out() {
        let mut engine = CustomAudioEngine::new();
        engine.force_playing_for_test();
        engine.set_skip_fade(crate::types::player_settings::FadeOnSkip::BoundaryFade, 2);
        let seq_counter = std::sync::atomic::AtomicU64::new(0);
        let song = click_song("b");

        let route = plan_click_play(
            &mut engine,
            &seq_counter,
            effect(),
            Some(&song),
            "http://server/rest/stream?id=b",
        )
        .await;

        assert!(matches!(route, ClickPlayRoute::BoundaryFadeThenHard(_)));
    }

    /// Boundary Fade from a stopped/paused app (or over radio) is a plain
    /// hard click — instant, exactly as today.
    #[tokio::test(flavor = "current_thread")]
    async fn click_boundary_mode_stopped_stays_hard() {
        let mut engine = CustomAudioEngine::new();
        engine.set_skip_fade(crate::types::player_settings::FadeOnSkip::BoundaryFade, 2);
        let seq_counter = std::sync::atomic::AtomicU64::new(0);
        let song = click_song("b");

        let route = plan_click_play(
            &mut engine,
            &seq_counter,
            effect(),
            Some(&song),
            "http://server/rest/stream?id=b",
        )
        .await;

        assert!(matches!(route, ClickPlayRoute::Hard(_)));
    }

    /// A defensive pool-miss (no Song metadata to build the plan from)
    /// degrades to the hard route even in Crossfade mode.
    #[tokio::test(flavor = "current_thread")]
    async fn click_crossfade_mode_without_song_metadata_stays_hard() {
        let mut engine = CustomAudioEngine::new();
        engine.force_playing_for_test();
        engine.set_skip_fade(crate::types::player_settings::FadeOnSkip::Crossfade, 2);
        let seq_counter = std::sync::atomic::AtomicU64::new(0);

        let route = plan_click_play(
            &mut engine,
            &seq_counter,
            effect(),
            None,
            "http://server/rest/stream?id=b",
        )
        .await;

        assert!(matches!(route, ClickPlayRoute::Hard(_)));
    }

    /// Supersession: a SECOND click stamped during the first click's
    /// unlocked decoder build must win — the first build's completion is
    /// abandoned at the seq gate without touching the engine.
    #[tokio::test(flavor = "multi_thread")]
    async fn second_click_supersedes_first_click_build() {
        let engine = Arc::new(Mutex::new(CustomAudioEngine::new()));
        let seq_counter = std::sync::atomic::AtomicU64::new(0);
        let (first_plan, first_generation, first_seq);
        {
            let mut e = engine.lock().await;
            e.force_playing_for_test();
            e.set_skip_fade(crate::types::player_settings::FadeOnSkip::Crossfade, 2);
            let song_b = click_song("b");
            let ClickPlayRoute::FadePlanned {
                plan,
                generation,
                seq,
            } = plan_click_play(
                &mut e,
                &seq_counter,
                effect(),
                Some(&song_b),
                "http://127.0.0.1:9/rest/stream?id=b",
            )
            .await
            else {
                panic!("first click must plan");
            };
            (first_plan, first_generation, first_seq) = (plan, generation, seq);

            // The second click lands while the first build runs (locks
            // released in production; here we just stamp its plan).
            let song_c = click_song("c");
            let second = plan_click_play(
                &mut e,
                &seq_counter,
                effect(),
                Some(&song_c),
                "http://127.0.0.1:9/rest/stream?id=c",
            )
            .await;
            assert!(matches!(second, ClickPlayRoute::FadePlanned { seq: 2, .. }));
        }

        // The FIRST click's build now completes — it must be abandoned at
        // the seq gate (the second click owns the engine), staging nothing.
        let result = complete_skip_fade(
            &engine,
            &seq_counter,
            *first_plan,
            first_generation,
            first_seq,
        )
        .await;

        assert!(result.is_ok());
        let e = engine.lock().await;
        assert!(
            e.source().is_empty(),
            "the superseded click must not stage or load anything"
        );
        assert!(e.playing(), "the outgoing keeps playing for the winner");
    }

    // ── M10 controller-level integration (fixture over unreachable server) ──
    //
    // Mirrors the queue_orchestrator fixture: real QueueService + real
    // PlaybackController over tempfile-backed storage, with a session resumed
    // against 127.0.0.1:9 (connection refused instantly) so stream URLs
    // build but every network decoder init fails deterministically. The
    // discriminating observable between the plan path and the hard path is
    // the navigator's `current_song_id`: the plan path names the clicked
    // song BEFORE the unlocked build, while the hard path only names it
    // after a successful `play()` — which the unreachable URL denies.

    struct ClickFixture {
        _temp: tempfile::TempDir,
        queue: QueueService,
        playback: PlaybackController,
    }

    async fn click_fixture() -> Result<ClickFixture> {
        let temp = tempfile::tempdir()?;
        let storage_q =
            crate::services::state_storage::StateStorage::new(temp.path().join("queue.redb"))?;
        let storage_s =
            crate::services::state_storage::StateStorage::new(temp.path().join("settings.redb"))?;
        let auth = crate::backend::auth::AuthGateway::new()?;
        auth.resume_session(
            "http://127.0.0.1:9".to_string(),
            "alice".to_string(),
            "jwt".to_string(),
            "u=alice&s=salt&t=token".to_string(),
        )
        .await?;
        let queue = QueueService::new(auth, storage_q)?;
        let settings = SettingsService::new(storage_s)?;
        let tm = Arc::new(TaskManager::new());
        let (playback, _loop_rx, _qc_rx) =
            PlaybackController::new(queue.clone(), settings, tm).await?;
        Ok(ClickFixture {
            _temp: temp,
            queue,
            playback,
        })
    }

    /// Queue click (play-from-here) in Crossfade mode while playing: the
    /// controller must route through the skip-fade plan — queue repositioned,
    /// the clicked song named at plan time, and (after the unreachable build
    /// fails) the fallback stages the click target as the engine source.
    #[tokio::test(flavor = "multi_thread")]
    async fn play_song_from_queue_crossfade_mode_takes_plan_path() -> Result<()> {
        let fx = click_fixture().await?;
        let songs = vec![click_song("a"), click_song("b")];
        let _ = fx.queue.set_queue(songs, Some(0)).await?;
        {
            let engine_arc = fx.playback.audio_engine();
            let mut e = engine_arc.lock().await;
            e.force_playing_for_test();
            e.set_skip_fade(crate::types::player_settings::FadeOnSkip::Crossfade, 2);
        }

        // The unreachable server makes the fallback's play() fail — the
        // routing itself is what's under test, so tolerate the Err.
        let _ = fx.playback.play_song_from_queue("b", 1).await;

        assert_eq!(
            fx.playback.current_song_id().await.as_deref(),
            Some("b"),
            "the plan path names the clicked song BEFORE the decoder build"
        );
        let engine_arc = fx.playback.audio_engine();
        let e = engine_arc.lock().await;
        assert!(
            e.source().contains("id=b"),
            "the blocked-blend fallback must stage the click target, got {:?}",
            e.source()
        );
        Ok(())
    }

    /// Browse-view play (album/playlist/songs) in Crossfade mode while
    /// playing: `play_songs_from_index` REPLACES the queue, and the fade
    /// plan must target the replaced queue's clicked row.
    #[tokio::test(flavor = "multi_thread")]
    async fn play_songs_from_index_crossfade_mode_plans_against_replaced_queue() -> Result<()> {
        let fx = click_fixture().await?;
        // An old queue is loaded and audibly playing…
        let _ = fx
            .queue
            .set_queue(vec![click_song("x"), click_song("y")], Some(0))
            .await?;
        {
            let engine_arc = fx.playback.audio_engine();
            let mut e = engine_arc.lock().await;
            e.force_playing_for_test();
            e.set_skip_fade(crate::types::player_settings::FadeOnSkip::Crossfade, 2);
        }

        // …when a browse click replaces it and starts at index 1 ("b").
        let _ = fx
            .playback
            .play_songs_from_index(vec![click_song("a"), click_song("b"), click_song("c")], 1)
            .await;

        let ids: Vec<String> = fx.queue.get_songs().iter().map(|s| s.id.clone()).collect();
        assert_eq!(ids, vec!["a", "b", "c"], "the queue was replaced");
        assert_eq!(
            fx.playback.current_song_id().await.as_deref(),
            Some("b"),
            "the fade plan must name the REPLACED queue's clicked row"
        );
        let engine_arc = fx.playback.audio_engine();
        let e = engine_arc.lock().await;
        assert!(
            e.source().contains("id=b"),
            "the fallback must stage the replaced queue's target, got {:?}",
            e.source()
        );
        Ok(())
    }

    /// Mode Off keeps the click byte-identical to today: the hard path never
    /// names the song at plan time (it names it only after a successful
    /// play(), which the unreachable server denies here).
    #[tokio::test(flavor = "multi_thread")]
    async fn play_song_from_queue_off_mode_keeps_hard_path() -> Result<()> {
        let fx = click_fixture().await?;
        let _ = fx
            .queue
            .set_queue(vec![click_song("a"), click_song("b")], Some(0))
            .await?;
        {
            let engine_arc = fx.playback.audio_engine();
            let mut e = engine_arc.lock().await;
            e.force_playing_for_test();
        }

        let result = fx.playback.play_song_from_queue("b", 1).await;

        assert!(result.is_err(), "hard play against 127.0.0.1:9 must fail");
        assert_eq!(
            fx.playback.current_song_id().await,
            None,
            "the hard path must not run any plan-time naming"
        );
        Ok(())
    }

    /// Regression test for the strong-Arc cycle introduced by `set_completion_callback`.
    ///
    /// Before the fix: `set_completion_callback` captured a strong
    /// `Arc<Mutex<CustomAudioEngine>>` in the closure it stored on the engine.
    /// That meant the engine's Arc refcount could never drop to zero — the
    /// engine held a strong reference to itself via the callback.
    ///
    /// After the fix: the closure captures only a `Weak`. Once the last external
    /// `Arc` is dropped, the strong count reaches zero and the engine can be freed.
    ///
    /// Requires `#[tokio::test]` because `CustomAudioEngine::new()` grabs
    /// `tokio::runtime::Handle::current()` inside `AudioRenderer::new()`.
    #[tokio::test]
    async fn completion_callback_does_not_create_strong_arc_cycle() {
        let engine_arc: Arc<Mutex<CustomAudioEngine>> =
            Arc::new(Mutex::new(CustomAudioEngine::new()));

        // Downgrade — this is the fix pattern.  Keep a second Weak for the
        // post-drop assertion; the first one moves into the closure below.
        let engine_weak_for_cb = Arc::downgrade(&engine_arc);
        let engine_weak_probe = Arc::downgrade(&engine_arc);

        // Simulate what the fixed set_completion_callback does: capture only Weak.
        {
            let mut engine = engine_arc.lock().await;
            engine.set_completion_callback(move |_is_loop| {
                // Upgrade inside the closure — this is the runtime path.
                if let Some(_ea) = engine_weak_for_cb.upgrade() {
                    // Would do engine work here in production.
                }
            });
        }

        // `engine_arc` is the sole strong holder. Dropping it must reduce the
        // strong count to zero; if the callback still held a strong clone the
        // count would remain at 1 (inside the `Arc<dyn Fn>` on the engine).
        drop(engine_arc);

        // After dropping the only external Arc the Weak must be dangling — i.e.
        // strong count is 0, so upgrade() returns None.
        assert!(
            engine_weak_probe.upgrade().is_none(),
            "engine Arc strong count did not reach zero: completion_callback \
             still holds a strong reference (cycle not broken)"
        );
    }
}
