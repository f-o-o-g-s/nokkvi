use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;
use tracing::debug;

use crate::{
    audio::engine::CustomAudioEngine,
    services::queue::{PreviousOutcome, QueueManager, TransitionReason},
    types::{NextTrackResetEffect, player_settings::FadeOnSkip, song::Song},
};

/// A manual skip that the queue layer has fully sequenced (cursor advanced,
/// history recorded, consume applied, `current_song_id` set) but whose
/// AUDIO transition is a skip-crossfade the caller must complete: build the
/// incoming decoder with NO locks held (invariant 14 — the navigator runs
/// under the engine lock, where a network decoder build must never happen),
/// then fire `engine.crossfade_to_next` (M7).
#[derive(Debug, Clone)]
pub struct SkipFadePlan {
    /// The skipped-to song (already the queue's current row).
    pub song: Song,
    /// Why the queue advanced — for the caller's "Now Playing" log line.
    pub reason: TransitionReason,
    /// The song's stream URL (built under the lock so the plan is
    /// self-contained).
    pub stream_url: String,
}

/// What a manual Next resolved to at the queue layer (M7).
#[derive(Debug)]
pub enum NextOutcome {
    /// End of order — nothing to advance to (under consume the queue was
    /// drained and the engine stopped, exactly as before).
    NoNext,
    /// The queue advanced and the engine was told to play (hard cut or
    /// boundary fade) — the historical behavior.
    Played(Song, TransitionReason),
    /// The queue advanced; the caller must complete the skip-crossfade
    /// (see [`SkipFadePlan`]). The engine is still playing the outgoing.
    FadePlanned(SkipFadePlan),
}

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
    PlayPrepared {
        song: Song,
        reason: TransitionReason,
    },
    /// Need to load a fresh stream URL (path 3).
    LoadFresh {
        stream_url: String,
        song: Song,
        reason: TransitionReason,
    },
}

/// Plan describing what the audio engine must do after a queue-removal has
/// already moved the queue play cursor (from which the physical index derives).
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
    /// The current row was removed and the queue still has songs. The engine
    /// must load `new_song_id` (which the queue model has already promoted to
    /// the play cursor now derives `new_index`) so its source follows the queue
    /// instead of streaming the deleted track, and the navigator's
    /// `current_song_id` must follow.
    ///
    /// `resume` carries the engine's *real* transport state at removal time:
    /// `true` only when the engine was genuinely [`PlaybackState::Playing`].
    /// When `false` (stopped or paused — including a just-reopened app whose
    /// navigator merely names a persisted `current_index`), the executor loads
    /// the new source but must NOT call `play()`, so removing the centered row
    /// of a stopped queue can't spuriously start playback.
    ///
    /// [`PlaybackState::Playing`]: crate::audio::engine::PlaybackState::Playing
    LoadNewCurrent {
        new_song_id: String,
        new_index: usize,
        resume: bool,
    },
    /// The playing song was removed and the queue is now empty. The engine
    /// must stop and the navigator's `current_song_id` must clear.
    StopEmpty,
}

/// Decide what the audio engine must do after a queue-removal mutation has
/// already moved the queue play cursor (from which the physical index derives).
///
/// Pure — reads inputs only, no I/O, no mutation. Runs after
/// `QueueManager::remove_songs_by_ids` so the derived playhead already
/// names whatever now occupies the playing slot (per the clamp in
/// `QueueManager::remove_song`).
///
/// `was_playing_id` is the song the navigator named *before* the removal;
/// the caller must snapshot it because the navigator's stored
/// `current_song_id` is stale by the time this decision runs.
///
/// `engine_playing` is the engine's *real* transport state, snapshotted by
/// the caller (`true` only when `engine.state() == PlaybackState::Playing`).
/// It is distinct from "the navigator names a current song": the navigator's
/// `current_song_id` is populated from the persisted `current_index` at
/// startup, so it is `Some` even on a freshly-reopened, never-played queue.
/// `engine_playing` flows through to [`RemovalAftermath::LoadNewCurrent::resume`]
/// so the executor swaps the engine source to the new current either way, but
/// only resumes playback when the engine was actually playing — a stopped or
/// paused app must not start playing just because its current row was removed.
pub fn decide_removal_aftermath(
    qm: &QueueManager,
    was_playing_id: Option<&str>,
    removed_ids: &[String],
    engine_playing: bool,
) -> RemovalAftermath {
    let was_playing = match was_playing_id {
        Some(id) => id,
        None => return RemovalAftermath::NoCurrentChange,
    };
    if !removed_ids.iter().any(|id| id == was_playing) {
        return RemovalAftermath::NoCurrentChange;
    }
    match qm
        .current_index()
        .and_then(|idx| qm.song_id_at(idx).map(|id| (id.to_owned(), idx)))
    {
        Some((new_id, idx)) => {
            // Duplicate-row case: the current song was removed from one row,
            // but another row with the same song_id still occupies the
            // current slot. If the engine is producing that song it is already
            // correct — leave it alone instead of reloading the same URL.
            if new_id == was_playing {
                RemovalAftermath::NoCurrentChange
            } else {
                RemovalAftermath::LoadNewCurrent {
                    new_song_id: new_id,
                    new_index: idx,
                    resume: engine_playing,
                }
            }
        }
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
            queue
                .current_index()
                .and_then(|idx| queue.song_id_at(idx))
                .map(str::to_owned)
        };

        Ok(Self {
            queue_manager,
            current_song_id: Arc::new(Mutex::new(initial_song_id)),
        })
    }

    // ══════════════════════════════════════════════════════════════════════
    //  Shared helpers
    // ══════════════════════════════════════════════════════════════════════

    /// Record the previous song in history, then consume it if consume mode
    /// is active. Returns the [`NextTrackResetEffect`] produced by the
    /// underlying `remove_song` call, if consume ran — the caller (which
    /// already holds the engine lock) must dispatch it via
    /// [`NextTrackResetEffect::apply_locked`].
    ///
    /// This is the single entry point for all consume-mode cleanup.
    /// Call this after transitioning to the next song.
    fn record_and_consume(
        &self,
        queue_manager: &mut QueueManager,
        prev_song_id: &str,
        prev_index: usize,
    ) -> Option<NextTrackResetEffect> {
        // Record in history, keyed by the finished row's stable entry_id so
        // Previous lands on the exact physical row (matters for adjacent
        // duplicate-id rows). Resolve the entry_id before the mutable borrow.
        if let Some(prev_song) = queue_manager.get_song(prev_song_id).cloned() {
            let eid = queue_manager.entry_id_at(prev_index);
            queue_manager.add_to_history(prev_song, eid);
        }

        // Consume: remove the finished song from queue + pool
        if queue_manager.get_queue().consume {
            self.consume_song_at_index(queue_manager, prev_index)
        } else {
            None
        }
    }

    /// Remove a song from the queue by its index.
    /// Uses QueueManager.remove_song() which properly maintains the order array,
    /// adjusts current_index, and persists.
    ///
    /// Returns the [`NextTrackResetEffect`] produced by the underlying
    /// removal so the caller can discharge it against the engine. `None`
    /// when the index is out of bounds.
    fn consume_song_at_index(
        &self,
        queue_manager: &mut QueueManager,
        index: usize,
    ) -> Option<NextTrackResetEffect> {
        if index >= queue_manager.queue_len() {
            return None;
        }

        if let Some(id) = queue_manager.song_id_at(index)
            && let Some(song) = queue_manager.get_song(id)
        {
            debug!(
                " [CONSUME] Removing: {} - {} (idx: {})",
                song.title, song.artist, index
            );
        }

        let effect = queue_manager.remove_song(index).ok();

        debug!(" [CONSUME] Queue length now: {}", queue_manager.queue_len());

        effect
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
    /// In ALL cases, the queue transition uses `PeekedQueue::transition()`.
    ///
    /// This is a thin wrapper around [`decide_transition`] + [`execute_transition`].
    /// The completion callback in `playback_controller.rs` calls those two halves
    /// directly so the outer `nav` mutex is dropped before engine I/O.
    pub async fn on_track_finished(
        &self,
        engine: &mut CustomAudioEngine,
        server_url: &str,
        subsonic_credential: &str,
    ) -> Result<Option<(Song, TransitionReason)>> {
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
        let mut was_crossfade = false;
        let needs_load = if engine.immediate_playing() {
            // Path 1: Engine already playing (gapless/crossfade completed by engine)
            engine.consume_gapless_transition().await;
            was_crossfade = engine.take_last_transition_was_crossfade();
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
        let is_consume = queue_manager.get_queue().consume;

        // mpd consume-wins (Queue.cxx: single is disabled while consume is on):
        // a consuming queue ignores repeat-Track and falls through to the
        // advance+consume path so the queue actually drains.
        if is_repeat_track && !is_consume {
            // Clear queued just in case
            queue_manager.clear_queued();

            let idx = queue_manager.current_index();
            let song = if let Some(idx) = idx {
                if let Some(id) = queue_manager.song_id_at(idx) {
                    queue_manager.get_song(id).cloned()
                } else {
                    None
                }
            } else {
                None
            };

            if let Some(song) = song {
                // Do NOT consume the track since we are repeating it. The
                // played row is `idx` (= current_index); key history by its
                // entry_id (resolve before the mutable borrow).
                let eid = idx.and_then(|i| queue_manager.entry_id_at(i));
                queue_manager.add_to_history(song.clone(), eid);
                drop(queue_manager);

                debug!("▶️ Now Playing: {} - {} (repeat)", song.title, song.artist);
                let reason = TransitionReason::Repeat;
                return if needs_load {
                    let stream_url = crate::utils::artwork_url::build_stream_url(
                        &song.id,
                        server_url,
                        subsonic_credential,
                    );
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
            // Consume the just-finished song before stopping. Both
            // `record_and_consume` (when consume mode actually ran) and
            // the reposition produce next-track-reset effects; the
            // engine lock is already held, so discharge in-line via
            // `apply_locked` before releasing the queue lock.
            let prev_id = self.current_song_id.lock().await.clone();
            let consume_effect = if let Some(ref pid) = prev_id
                && let Some(idx) = queue_manager.current_index()
            {
                self.record_and_consume(&mut queue_manager, pid, idx)
            } else {
                None
            };
            *self.current_song_id.lock().await = None;
            let reposition_effect = queue_manager.reposition_to_index(None);
            queue_manager.save_all().ok();
            drop(queue_manager);
            if let Some(effect) = consume_effect {
                effect.apply_locked(engine).await;
            }
            reposition_effect.apply_locked(engine).await;
            debug!(" No next song available (queue empty or at end)");
            return TrackTransitionPlan::Stop;
        }

        // Transition: peek (re-peek subsumes the previous defense against
        // concurrent queue mutations clearing `queued` between gapless prep
        // and this callback), then consume the guard via `transition()` to
        // update current_index/current_order. This is critical for paths
        // 1/2 where the engine is already playing the next track — stopping
        // it would kill a successful gapless transition.
        let Some(peeked) = queue_manager.peek_next_song() else {
            drop(queue_manager);
            debug!(" No queued song to transition to");
            return TrackTransitionPlan::Stop;
        };
        let transition = peeked.transition();

        let song = transition.song.clone();
        // A crossfade is the most specific transition fact for the log, so it
        // wins the label even under shuffle (the engine blended the two tracks).
        let reason = if was_crossfade {
            TransitionReason::Crossfade
        } else if queue_manager.get_queue().shuffle {
            TransitionReason::Shuffle
        } else {
            TransitionReason::Gapless
        };

        // Record history + consume previous song (via remove_song which
        // properly maintains the order array). The consume removal may
        // have invalidated engine gapless prep — discharge the resulting
        // effect against the engine lock we already hold.
        let prev_id = self.current_song_id.lock().await.clone();
        let consume_effect = if let Some(ref pid) = prev_id
            && let Some(old_idx) = transition.old_index
        {
            self.record_and_consume(&mut queue_manager, pid, old_idx)
        } else {
            None
        };

        *self.current_song_id.lock().await = Some(song.id.clone());
        drop(queue_manager);
        if let Some(effect) = consume_effect {
            effect.apply_locked(engine).await;
        }

        debug!(
            "▶️ Now Playing: {} - {} ({})",
            song.title, song.artist, reason
        );

        if needs_load {
            let stream_url = crate::utils::artwork_url::build_stream_url(
                &song.id,
                server_url,
                subsonic_credential,
            );
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
    ) -> Result<Option<(Song, TransitionReason)>> {
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
                engine
                    .load_track_with_rg(
                        &stream_url,
                        song.replay_gain.clone(),
                        song.expected_duration_ms(),
                    )
                    .await;
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

        let stream_url =
            crate::utils::artwork_url::build_stream_url(&song.id, server_url, subsonic_credential);

        *self.current_song_id.lock().await = Some(song.id.clone());

        engine
            .load_track_with_rg(
                &stream_url,
                song.replay_gain.clone(),
                song.expected_duration_ms(),
            )
            .await;
        engine.play().await?;

        Ok(())
    }

    /// Complete a manual skip to `song` per the "Fade on Skip" mode (M7):
    /// hard load (`Off`, the historical path), boundary ease-out then hard
    /// load (`BoundaryFade` — the ramp self-refuses when there is nothing
    /// audible to fade), or — when the engine can blend — hand back a
    /// [`SkipFadePlan`] for the caller to complete (`Crossfade`; the decoder
    /// build must run with no locks held, so it cannot happen here).
    ///
    /// The `Crossfade`-but-not-viable case (paused / stopped / radio
    /// outgoing) falls through to the hard path: there is no audible
    /// outgoing to blend, and a radio outgoing is M6's domain (its
    /// `set_source` fade handles that edge when enabled).
    async fn skip_to_song(
        &self,
        engine: &mut CustomAudioEngine,
        song: &Song,
        reason: TransitionReason,
        server_url: &str,
        subsonic_credential: &str,
        skip_fade: FadeOnSkip,
    ) -> Result<Option<SkipFadePlan>> {
        match skip_fade {
            FadeOnSkip::Crossfade if engine.skip_crossfade_viable() => {
                debug!(
                    "🔀 Skip fade planned: {} - {} (id: {})",
                    song.title, song.artist, song.id
                );
                // Plan-time invalidation, UNDER the engine lock: cancel the
                // pre-skip transition, bump the source generation, and latch
                // the pending window — a natural EOF / live-blend finalize
                // processed during the unlocked decoder build would
                // otherwise run `decide_transition` against the cursor this
                // skip is about to advance (a double advance). See
                // `CustomAudioEngine::plan_skip_fade`.
                engine.plan_skip_fade().await;
                *self.current_song_id.lock().await = Some(song.id.clone());
                let stream_url = crate::utils::artwork_url::build_stream_url(
                    &song.id,
                    server_url,
                    subsonic_credential,
                );
                Ok(Some(SkipFadePlan {
                    song: song.clone(),
                    reason,
                    stream_url,
                }))
            }
            FadeOnSkip::BoundaryFade => {
                engine.run_skip_out_fade().await;
                self.play_song_direct(engine, song, server_url, subsonic_credential)
                    .await?;
                Ok(None)
            }
            FadeOnSkip::Off | FadeOnSkip::Crossfade => {
                self.play_song_direct(engine, song, server_url, subsonic_credential)
                    .await?;
                Ok(None)
            }
        }
    }

    /// Play next song (manual skip via button/hotkey/MPRIS).
    ///
    /// `skip_fade` is the EFFECTIVE "Fade on Skip" mode for this skip — the
    /// controller resolves it from the engine mirror and threads it down so
    /// the queue layer stays settings-free.
    pub async fn play_next(
        &self,
        engine: &mut CustomAudioEngine,
        server_url: &str,
        subsonic_credential: &str,
        skip_fade: FadeOnSkip,
    ) -> Result<NextOutcome> {
        let mut queue_manager = self.queue_manager.lock().await;
        let current_index = queue_manager.current_index();
        let is_consume = queue_manager.get_queue().consume;

        // Anchor the consume target to the current row's stable entry_id
        // (NOT a raw index that a concurrent queue mutation could shift
        // during the dropped-lock network await below). Captured under the
        // lock, removed by id after re-acquiring it. Distinct rows of the
        // same song get distinct entry_ids, so duplicates stay correct.
        let consume_entry: Option<u64> = if is_consume {
            current_index.and_then(|i| queue_manager.entry_id_at(i))
        } else {
            None
        };

        // Record current song in history before advancing. The helper resolves
        // the entry_id from the recorded song's OWN first-match row so the
        // (song, entry_id) pair always agrees; Previous then lands on the exact
        // physical row even with adjacent duplicate-id rows.
        let prev_id = self.current_song_id.lock().await.clone();
        if let Some(ref pid) = prev_id {
            queue_manager.add_to_history_by_song_id(pid);
        }

        let Some(result) = queue_manager.get_next_song() else {
            // End of order with nothing to advance to. Under consume, mirror
            // the auto-advance None branch (decide_transition): consume the
            // finished song, empty the playhead, and stop the engine so a
            // Next over the final track drains the queue instead of leaving
            // the last entry behind. The queue lock has no intervening await
            // here, so the entry_id resolves against the live queue.
            if let Some(eid) = consume_entry {
                let consume_effect = queue_manager.remove_entry_by_id(eid).ok();
                let reposition_effect = queue_manager.reposition_to_index(None);
                drop(queue_manager);
                *self.current_song_id.lock().await = None;
                if let Some(effect) = consume_effect {
                    effect.apply_locked(engine).await;
                }
                reposition_effect.apply_locked(engine).await;
                engine.stop().await;
            } else {
                drop(queue_manager);
            }
            return Ok(NextOutcome::NoNext);
        };
        drop(queue_manager);

        let plan = self
            .skip_to_song(
                engine,
                &result.song,
                result.reason,
                server_url,
                subsonic_credential,
                skip_fade,
            )
            .await?;

        // Consume: remove the previously played song after starting the next.
        // Resolve by the entry_id captured before the await so a concurrent
        // queue mutation can't desync the removal target (drift-immune).
        //
        // M7 ordering note: on the FadePlanned path this runs BEFORE the fade
        // fires (the caller completes the fade after we return), so the
        // removal's `NextTrackResetEffect` — whose `reset_next_track` cancels
        // any LIVE blend — can never kill the skip fade it precedes. The row
        // is consumed at skip time, exactly as on the historical hard path.
        if let Some(eid) = consume_entry {
            let mut qm = self.queue_manager.lock().await;
            let effect = qm.remove_entry_by_id(eid).ok();
            drop(qm);
            if let Some(effect) = effect {
                effect.apply_locked(engine).await;
            }
        }

        Ok(match plan {
            Some(plan) => NextOutcome::FadePlanned(plan),
            None => NextOutcome::Played(result.song, result.reason),
        })
    }

    /// Play previous song (manual skip via button/hotkey/MPRIS).
    ///
    /// `skip_fade` mirrors [`Self::play_next`]: the effective "Fade on Skip"
    /// mode, resolved by the controller. Returns the outcome plus an
    /// optional [`SkipFadePlan`] the caller must complete (Crossfade mode
    /// with an audible finite outgoing).
    pub async fn play_previous(
        &self,
        engine: &mut CustomAudioEngine,
        server_url: &str,
        subsonic_credential: &str,
        skip_fade: FadeOnSkip,
    ) -> Result<(PreviousOutcome, Option<SkipFadePlan>)> {
        use crate::services::queue::PreviousSongResult;

        let mut queue_manager = self.queue_manager.lock().await;
        let current_index = queue_manager.current_index();
        let is_consume = queue_manager.get_queue().consume;

        match queue_manager.get_previous_song(current_index) {
            PreviousSongResult::InQueue(song, _index) => {
                debug!("⏮️ Previous: {} - {}", song.artist, song.title);

                // Anchor the consume target to the previously-current row's
                // stable entry_id before dropping the lock for the network
                // await — a raw index could shift under a concurrent queue
                // mutation. Note `get_previous_song` has already moved
                // `current_index` to the previous song, so capture from the
                // pre-call `current_index` snapshot.
                let consume_entry: Option<u64> = if is_consume {
                    current_index.and_then(|i| queue_manager.entry_id_at(i))
                } else {
                    None
                };

                *self.current_song_id.lock().await = Some(song.id.clone());
                drop(queue_manager);

                let plan = self
                    .skip_to_song(
                        engine,
                        &song,
                        TransitionReason::Previous,
                        server_url,
                        subsonic_credential,
                        skip_fade,
                    )
                    .await?;

                // Consume: remove the previously played song after starting prev.
                // Resolve by the captured entry_id (drift-immune, duplicate-safe).
                // On the FadePlanned path this runs BEFORE the fade fires (see
                // the matching note in `play_next`).
                if let Some(eid) = consume_entry {
                    let mut qm = self.queue_manager.lock().await;
                    let effect = qm.remove_entry_by_id(eid).ok();
                    drop(qm);
                    if let Some(effect) = effect {
                        effect.apply_locked(engine).await;
                    }
                }

                debug!("▶️ Now Playing: {} - {}", song.title, song.artist);
                Ok((PreviousOutcome::Stepped, plan))
            }
            PreviousSongResult::Removed(song) => {
                debug!(
                    "⏮️ Re-inserting consumed song: {} - {}",
                    song.artist, song.title
                );

                let insert_idx = current_index.unwrap_or(0);
                let insert_effect =
                    queue_manager.insert_song_and_make_current(insert_idx, song.clone())?;

                *self.current_song_id.lock().await = Some(song.id.clone());
                drop(queue_manager);
                insert_effect.apply_locked(engine).await;

                let plan = self
                    .skip_to_song(
                        engine,
                        &song,
                        TransitionReason::Previous,
                        server_url,
                        subsonic_credential,
                        skip_fade,
                    )
                    .await?;

                debug!(
                    "▶️ Now Playing (re-inserted): {} - {}",
                    song.title, song.artist
                );
                Ok((PreviousOutcome::Stepped, plan))
            }
            PreviousSongResult::BlockedConsumeShuffle => {
                // Consumed-track step-back under shuffle. History was left
                // intact by `get_previous_song`; nothing to play or mutate —
                // signal the UI to surface an explanatory toast.
                drop(queue_manager);
                debug!("⏮️ Previous blocked: consumed track under shuffle");
                Ok((PreviousOutcome::BlockedConsumeShuffle, None))
            }
            PreviousSongResult::None => {
                drop(queue_manager);
                debug!("⏮️ No previous song available");
                Ok((PreviousOutcome::Stepped, None))
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
        qm.replace_song_ids_for_test(ids, current_index);
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
            qm.lock().await.current_index().is_none(),
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
        assert!(qm.lock().await.current_index().is_none());
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
                assert_eq!(reason, TransitionReason::Gapless);
                assert!(stream_url.starts_with("http://server/rest/stream?id=b"));
            }
            other => panic!("expected LoadFresh, got {other:?}"),
        }

        // Navigator advanced current_song_id to the new track before returning.
        assert_eq!(nav.get_current_song_id().await.as_deref(), Some("b"));
        assert_eq!(qm.lock().await.current_index(), Some(1));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn decide_path3_repeat_track_returns_load_fresh_with_repeat_reason() {
        let songs = vec![make_song("a")];
        let mut qm = manager_with_songs(songs, Some(0));
        let _ = qm
            .set_repeat(crate::types::queue::RepeatMode::Track)
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
                assert_eq!(reason, TransitionReason::Repeat);
                assert!(stream_url.starts_with("http://server/rest/stream?id=a"));
            }
            other => panic!("expected LoadFresh, got {other:?}"),
        }

        // Repeat-track preserves current_index.
        assert_eq!(qm.lock().await.current_index(), Some(0));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn decide_consume_repeat_track_advances_and_consumes() {
        // mpd consume-wins: end-of-track under consume + repeat-Track must
        // advance + drain, not replay the current song.
        let songs = vec![make_song("a"), make_song("b")];
        let mut qm = manager_with_songs(songs, Some(0));
        qm.queue.consume = true;
        let _ = qm
            .set_repeat(crate::types::queue::RepeatMode::Track)
            .expect("set repeat");

        let qm = Arc::new(Mutex::new(qm));
        let nav = QueueNavigator::new(qm.clone()).await.expect("navigator");
        nav.set_current_song_id(Some("a".to_string())).await;

        let mut engine = CustomAudioEngine::new();
        let plan = nav
            .decide_transition(&mut engine, "http://server", "u=test&p=test")
            .await;

        match plan {
            TrackTransitionPlan::LoadFresh { song, reason, .. } => {
                assert_eq!(song.id, "b", "must advance, not replay 'a'");
                assert_eq!(
                    reason,
                    TransitionReason::Gapless,
                    "fell through to the advance path"
                );
            }
            other => panic!("expected LoadFresh advancing to b, got {other:?}"),
        }

        // The finished song "a" was consumed (removed) from the queue.
        let song_ids = qm.lock().await.song_ids_snapshot();
        assert_eq!(song_ids, vec!["b"], "'a' must be consumed");
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

    /// N19: manual Next over the final track under consume must consume the
    /// finished song and stop (mirroring the auto-advance None branch), not
    /// leave the last entry behind.
    #[tokio::test(flavor = "current_thread")]
    async fn play_next_last_song_consume_removes_and_stops() {
        // Single-song queue: a is current and last, consume on, no repeat.
        let mut qm = manager_with_songs(vec![make_song("a")], Some(0));
        qm.queue.consume = true;
        let qm = Arc::new(Mutex::new(qm));
        let nav = QueueNavigator::new(qm.clone()).await.expect("navigator");
        nav.set_current_song_id(Some("a".to_string())).await;

        let mut engine = CustomAudioEngine::new();
        let result = nav
            .play_next(
                &mut engine,
                "http://server",
                "u=test&p=test",
                FadeOnSkip::Off,
            )
            .await
            .expect("play_next ok");

        assert!(
            matches!(result, NextOutcome::NoNext),
            "no next track at end of queue"
        );
        let q = qm.lock().await;
        assert!(q.is_queue_empty(), "the finished song must be consumed",);
        assert_eq!(q.current_index(), None);
    }

    /// N19 multi-song variant: at the last index of a multi-song queue,
    /// consume + Next removes the last song and stops.
    #[tokio::test(flavor = "current_thread")]
    async fn play_next_at_last_index_consume_removes_and_stops() {
        let mut qm = manager_with_songs(vec![make_song("a"), make_song("b")], Some(1));
        qm.queue.consume = true;
        let qm = Arc::new(Mutex::new(qm));
        let nav = QueueNavigator::new(qm.clone()).await.expect("navigator");
        nav.set_current_song_id(Some("b".to_string())).await;

        let mut engine = CustomAudioEngine::new();
        let result = nav
            .play_next(
                &mut engine,
                "http://server",
                "u=test&p=test",
                FadeOnSkip::Off,
            )
            .await
            .expect("play_next ok");

        assert!(matches!(result, NextOutcome::NoNext));
        let q = qm.lock().await;
        assert_eq!(q.song_ids_snapshot(), vec!["a"], "b consumed");
        assert_eq!(q.current_index(), None);
    }

    /// N19 negative: at the last index WITHOUT consume, Next is a plain no-op
    /// — the song stays and current_index is unchanged.
    #[tokio::test(flavor = "current_thread")]
    async fn play_next_last_song_no_consume_is_noop() {
        let qm = manager_with_songs(vec![make_song("a")], Some(0));
        let qm = Arc::new(Mutex::new(qm));
        let nav = QueueNavigator::new(qm.clone()).await.expect("navigator");
        nav.set_current_song_id(Some("a".to_string())).await;

        let mut engine = CustomAudioEngine::new();
        let result = nav
            .play_next(
                &mut engine,
                "http://server",
                "u=test&p=test",
                FadeOnSkip::Off,
            )
            .await
            .expect("play_next ok");

        assert!(matches!(result, NextOutcome::NoNext));
        let q = qm.lock().await;
        assert_eq!(
            q.song_ids_snapshot(),
            vec!["a"],
            "song stays without consume"
        );
        assert_eq!(q.current_index(), Some(0));
    }

    // ── M7 manual-skip fade plans ──

    /// M7 skip-overlap: with "Fade on Skip: Crossfade" and the engine
    /// audibly playing a finite stream, `play_next` fully sequences the
    /// queue (cursor, history, `current_song_id`) but returns a
    /// `FadePlanned` instead of loading — the ENGINE keeps playing the
    /// outgoing untouched (its source never changes here; the caller builds
    /// the incoming decoder lock-free and fires `crossfade_to_next`).
    #[tokio::test(flavor = "current_thread")]
    async fn play_next_crossfade_mode_returns_fade_plan_and_leaves_outgoing_playing() {
        let qm = manager_with_songs(vec![make_song("a"), make_song("b")], Some(0));
        let qm = Arc::new(Mutex::new(qm));
        let nav = QueueNavigator::new(qm.clone()).await.expect("navigator");
        nav.set_current_song_id(Some("a".to_string())).await;

        let mut engine = CustomAudioEngine::new();
        engine.force_playing_for_test();
        let gen_before = engine.source_generation();

        let result = nav
            .play_next(
                &mut engine,
                "http://server",
                "u=test&p=test",
                FadeOnSkip::Crossfade,
            )
            .await
            .expect("play_next ok");

        let NextOutcome::FadePlanned(plan) = result else {
            panic!("expected FadePlanned, got {result:?}");
        };
        assert_eq!(
            engine.source_generation(),
            gen_before + 1,
            "the plan must invalidate in-flight completions AT PLAN TIME \
             (plan_skip_fade bumps under the lock) — a natural EOF processed \
             during the unlocked decoder build would otherwise advance the \
             already-advanced cursor"
        );
        assert_eq!(plan.song.id, "b");
        assert_eq!(plan.reason, TransitionReason::Next);
        assert!(
            plan.stream_url.contains("id=b"),
            "the plan must carry b's stream URL, got {}",
            plan.stream_url
        );
        assert_eq!(
            nav.get_current_song_id().await.as_deref(),
            Some("b"),
            "the queue layer must already name the skipped-to song"
        );
        assert_eq!(qm.lock().await.current_index(), Some(1));
        assert!(
            engine.source().is_empty(),
            "the engine must keep the outgoing untouched (no load happened)"
        );
    }

    /// M7 consume ordering: under consume the outgoing's row is removed at
    /// skip time — BEFORE the fade fires (the plan is completed by the
    /// caller afterwards), so the removal's `NextTrackResetEffect` can never
    /// cancel the blend it precedes.
    #[tokio::test(flavor = "current_thread")]
    async fn play_next_crossfade_mode_with_consume_removes_row_before_fade() {
        let mut qm = manager_with_songs(vec![make_song("a"), make_song("b")], Some(0));
        qm.queue.consume = true;
        let qm = Arc::new(Mutex::new(qm));
        let nav = QueueNavigator::new(qm.clone()).await.expect("navigator");
        nav.set_current_song_id(Some("a".to_string())).await;

        let mut engine = CustomAudioEngine::new();
        engine.force_playing_for_test();

        let result = nav
            .play_next(
                &mut engine,
                "http://server",
                "u=test&p=test",
                FadeOnSkip::Crossfade,
            )
            .await
            .expect("play_next ok");

        assert!(
            matches!(result, NextOutcome::FadePlanned(_)),
            "consume must not downgrade the skip fade, got {result:?}"
        );
        let q = qm.lock().await;
        assert_eq!(
            q.song_ids_snapshot(),
            vec!["b"],
            "the skipped-away row is consumed at skip time"
        );
    }

    /// M7 Previous: the step-back takes the same plan path — queue fully
    /// sequenced, engine untouched, plan carries the previous song.
    #[tokio::test(flavor = "current_thread")]
    async fn play_previous_crossfade_mode_returns_fade_plan() {
        let qm = manager_with_songs(vec![make_song("a"), make_song("b")], Some(1));
        let qm = Arc::new(Mutex::new(qm));
        let nav = QueueNavigator::new(qm.clone()).await.expect("navigator");
        nav.set_current_song_id(Some("b".to_string())).await;

        let mut engine = CustomAudioEngine::new();
        engine.force_playing_for_test();
        let gen_before = engine.source_generation();

        let (outcome, plan) = nav
            .play_previous(
                &mut engine,
                "http://server",
                "u=test&p=test",
                FadeOnSkip::Crossfade,
            )
            .await
            .expect("play_previous ok");

        assert_eq!(outcome, PreviousOutcome::Stepped);
        let plan = plan.expect("a fade plan for the step-back");
        assert_eq!(plan.song.id, "a");
        assert_eq!(plan.reason, TransitionReason::Previous);
        assert_eq!(
            engine.source_generation(),
            gen_before + 1,
            "the step-back plan must run the same plan-time invalidation"
        );
        assert!(
            engine.source().is_empty(),
            "the engine must keep the outgoing untouched (no load happened)"
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
    // free function over `QueueManager` + `Option<&str>` + `&[String]` + the
    // `engine_playing` snapshot (`bool`).

    /// Nothing was playing → removal can't have unhooked anything.
    #[test]
    fn removal_aftermath_no_playing_song_returns_no_change() {
        let qm = manager_with_songs(vec![make_song("a"), make_song("b")], None);
        let plan = decide_removal_aftermath(&qm, None, &["a".to_string()], false);
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
        let _ = qm.remove_song_by_id("b").expect("remove b");

        let plan = decide_removal_aftermath(&qm, Some("a"), &["b".to_string()], true);

        assert_eq!(
            plan,
            RemovalAftermath::NoCurrentChange,
            "removing a non-playing song must not retarget the engine",
        );
    }

    /// Playing song was removed *while genuinely playing* and the queue still
    /// has songs → the engine must load the song the queue now exposes at
    /// `current_index` and resume (`resume: true`).
    #[test]
    fn removal_aftermath_playing_song_removed_loads_new_current() {
        // Queue: [a, b, c], current = b (idx 1). Remove b.
        // After: [a, c], queue's `remove_song` clamp leaves current_index = 1
        // (now pointing at c). The engine must transition to c.
        let mut qm = manager_with_songs(
            vec![make_song("a"), make_song("b"), make_song("c")],
            Some(1),
        );
        let _ = qm.remove_song_by_id("b").expect("remove b");

        let plan = decide_removal_aftermath(&qm, Some("b"), &["b".to_string()], true);

        assert_eq!(
            plan,
            RemovalAftermath::LoadNewCurrent {
                new_song_id: "c".to_string(),
                new_index: 1,
                resume: true,
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
        let _ = qm
            .remove_songs_by_ids(&["b".to_string(), "d".to_string()])
            .expect("remove batch");

        let plan =
            decide_removal_aftermath(&qm, Some("b"), &["b".to_string(), "d".to_string()], true);

        assert_eq!(
            plan,
            RemovalAftermath::LoadNewCurrent {
                new_song_id: "c".to_string(),
                new_index: 1,
                resume: true,
            },
        );
    }

    /// Playing song was the last in queue and gets removed → queue empty,
    /// engine must stop and the navigator's current_song_id must clear.
    #[test]
    fn removal_aftermath_last_song_removed_returns_stop_empty() {
        // Queue: [only], current = 0. Remove only. After: [], current_index = None.
        let mut qm = manager_with_songs(vec![make_song("only")], Some(0));
        let _ = qm.remove_song_by_id("only").expect("remove only");

        let plan = decide_removal_aftermath(&qm, Some("only"), &["only".to_string()], true);

        assert_eq!(
            plan,
            RemovalAftermath::StopEmpty,
            "empty queue after removing the playing song must stop the engine",
        );
    }

    /// Mirrors what `AppService::remove_queue_entries` does at the queue
    /// layer: resolve entry_ids → song_ids *before* the mutation, then
    /// `remove_entries_by_ids`, then `decide_removal_aftermath` with the
    /// pre-mutation song_ids. The ordering matters — after the removal,
    /// the entry_ids are gone and the resolution would yield an empty Vec,
    /// making the aftermath plan return `NoCurrentChange` when it should
    /// return `LoadNewCurrent`.
    #[test]
    fn orchestrator_resolves_entry_ids_to_song_ids_before_mutation() {
        // Queue: [a, b, c], playing = "b". Remove the row whose entry_id
        // currently sits at index 1 ("b").
        let mut qm = manager_with_songs(
            vec![make_song("a"), make_song("b"), make_song("c")],
            Some(1),
        );
        let target_entry_id = qm.entry_id_at(1).expect("entry_id for b");

        // Resolve first — this is the bit the orchestrator must do *before*
        // mutating so it knows what song_ids were removed.
        let removed_song_ids: Vec<String> = [target_entry_id]
            .iter()
            .filter_map(|&eid| {
                qm.index_of_entry(eid)
                    .and_then(|idx| qm.song_id_at(idx).map(str::to_owned))
            })
            .collect();
        assert_eq!(
            removed_song_ids,
            vec!["b".to_string()],
            "resolution must read the pre-mutation queue",
        );

        // Now mutate.
        let _ = qm
            .remove_entry_by_id(target_entry_id)
            .expect("remove by entry_id");
        assert_eq!(qm.song_ids_snapshot(), vec!["a", "c"]);
        // After removal the entry_id is gone, so a resolution attempt would
        // return an empty Vec — proves the ordering is load-bearing.
        let post_mutation_resolved: Vec<String> = [target_entry_id]
            .iter()
            .filter_map(|&eid| {
                qm.index_of_entry(eid)
                    .and_then(|idx| qm.song_id_at(idx).map(str::to_owned))
            })
            .collect();
        assert!(
            post_mutation_resolved.is_empty(),
            "post-mutation resolution must be empty — confirms ordering matters",
        );

        // And the aftermath plan, fed the pre-mutation resolution, correctly
        // routes the engine to "c" (queue's clamp landed there).
        let plan = decide_removal_aftermath(&qm, Some("b"), &removed_song_ids, true);
        assert_eq!(
            plan,
            RemovalAftermath::LoadNewCurrent {
                new_song_id: "c".to_string(),
                new_index: 1,
                resume: true,
            },
        );
    }

    /// Duplicate row removed: queue had two rows of the same song_id, the
    /// playing row was removed by entry_id. The post-removal `current_index`
    /// lands on the surviving duplicate — same song_id as `was_playing`, so
    /// the engine must NOT reload (a no-op reload would re-buffer audio
    /// that's already playing).
    #[test]
    fn removal_aftermath_duplicate_survives_keeps_engine_running() {
        // Queue: [dup, dup, b], current = first dup (idx 0).
        // Remove the first dup row. After: [dup, b], current_index clamps
        // to 0, pointing at the surviving "dup" row.
        let mut qm = manager_with_songs(
            vec![make_song("dup"), make_song("dup"), make_song("b")],
            Some(0),
        );
        // Remove only the row at index 0 (one of the duplicates).
        let _ = qm.remove_song(0).expect("remove duplicate row");
        assert_eq!(qm.song_ids_snapshot(), vec!["dup", "b"]);
        assert_eq!(qm.current_index(), Some(0));

        let plan = decide_removal_aftermath(&qm, Some("dup"), &["dup".to_string()], true);

        assert_eq!(
            plan,
            RemovalAftermath::NoCurrentChange,
            "removing one of two duplicate rows must not retarget the engine",
        );
    }

    /// Edge case: every remaining queue song is also removed in the same
    /// batch → still StopEmpty, never panics on `song_ids.get(idx)`.
    #[test]
    fn removal_aftermath_clear_queue_returns_stop_empty() {
        // Queue: [a, b], current = a (idx 0). Remove [a, b]. After: [].
        let mut qm = manager_with_songs(vec![make_song("a"), make_song("b")], Some(0));
        let _ = qm
            .remove_songs_by_ids(&["a".to_string(), "b".to_string()])
            .expect("remove all");

        let plan =
            decide_removal_aftermath(&qm, Some("a"), &["a".to_string(), "b".to_string()], true);

        assert_eq!(plan, RemovalAftermath::StopEmpty);
    }

    // ── Regression: removal must not start playback from a non-playing app ──
    //
    // Root cause: `was_playing_id` is the navigator's `current_song_id`, which
    // `QueueNavigator::new` seeds from the persisted `current_index` at startup
    // — so it is `Some` on a freshly-reopened, never-played queue. The engine's
    // real transport state (`engine_playing`) is the discriminator: a stopped
    // or paused app re-cues its source to the new current (`LoadNewCurrent`)
    // but must NOT resume (`resume: false`).

    /// THE REPORTED BUG. Stopped engine (e.g. app reopened, or user pressed
    /// Stop), the navigator still names the persisted current row, and that row
    /// is removed. The engine source must follow the queue to the next song so
    /// a later manual Play is correct — but playback must NOT auto-start.
    #[test]
    fn removal_aftermath_stopped_engine_removed_current_recues_without_resume() {
        // Queue: [a, b, c], current = b (idx 1). Remove b. After: [a, c],
        // current_index clamps to 1 (c). engine_playing = false (Stopped).
        let mut qm = manager_with_songs(
            vec![make_song("a"), make_song("b"), make_song("c")],
            Some(1),
        );
        let _ = qm.remove_song_by_id("b").expect("remove b");

        let plan = decide_removal_aftermath(&qm, Some("b"), &["b".to_string()], false);

        assert_eq!(
            plan,
            RemovalAftermath::LoadNewCurrent {
                new_song_id: "c".to_string(),
                new_index: 1,
                resume: false,
            },
            "a stopped app must re-cue to the new current WITHOUT starting playback",
        );
    }

    /// Paused engine snapshots to `engine_playing = false` exactly like a
    /// stopped one — removing the paused current row re-cues the source but
    /// must not resume into playback.
    #[test]
    fn removal_aftermath_paused_engine_removed_current_recues_without_resume() {
        let mut qm = manager_with_songs(
            vec![make_song("a"), make_song("b"), make_song("c")],
            Some(1),
        );
        let _ = qm.remove_song_by_id("b").expect("remove b");

        let plan = decide_removal_aftermath(&qm, Some("b"), &["b".to_string()], false);

        assert_eq!(
            plan,
            RemovalAftermath::LoadNewCurrent {
                new_song_id: "c".to_string(),
                new_index: 1,
                resume: false,
            },
            "a paused app must re-cue to the new current WITHOUT resuming playback",
        );
    }

    /// The play-state gate is scoped strictly to the `LoadNewCurrent` arm:
    /// emptying the queue from a stopped app still routes to `StopEmpty`
    /// (engine.stop() + navigator clear), never a re-cue.
    #[test]
    fn removal_aftermath_stopped_engine_emptied_still_stops() {
        let mut qm = manager_with_songs(vec![make_song("only")], Some(0));
        let _ = qm.remove_song_by_id("only").expect("remove only");

        let plan = decide_removal_aftermath(&qm, Some("only"), &["only".to_string()], false);

        assert_eq!(
            plan,
            RemovalAftermath::StopEmpty,
            "emptying the queue must stop the engine regardless of play-state",
        );
    }

    /// The duplicate-row guard precedes the play-state gate: a surviving
    /// duplicate of the same song still occupies `current_index`, so the
    /// engine is left alone — `NoCurrentChange`, not a re-cue — whether or not
    /// the engine was playing.
    #[test]
    fn removal_aftermath_duplicate_survives_no_change_when_stopped() {
        let mut qm = manager_with_songs(
            vec![make_song("dup"), make_song("dup"), make_song("b")],
            Some(0),
        );
        let _ = qm.remove_song(0).expect("remove duplicate row");

        let plan = decide_removal_aftermath(&qm, Some("dup"), &["dup".to_string()], false);

        assert_eq!(
            plan,
            RemovalAftermath::NoCurrentChange,
            "a surviving duplicate keeps the engine untouched regardless of play-state",
        );
    }
}
