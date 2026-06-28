//! Scrobbling handlers

use iced::Task;
use tracing::{debug, warn};

use crate::{
    Nokkvi,
    app_message::{
        HotkeyMessage, Message, RadioSubmitOutcome, ScrobbleMessage, ScrobbleTargets, ScrobbleTrack,
    },
};

/// Debounce before the FIRST now-playing emit after a song change / loop.
const NOW_PLAYING_DEBOUNCE_SECS: u64 = 2;
/// Interval between periodic now-playing heartbeats, comfortably under
/// Navidrome's ephemeral now-playing TTL so a long single track keeps its
/// server-side entry alive.
const NOW_PLAYING_REFRESH_SECS: u64 = 30;

/// Current wall-clock as unix seconds, or `None` on a pre-epoch clock. A radio
/// scrobble is timestamped at the track start, so a bogus pre-1970 clock would
/// stamp the listen at the epoch and get it rejected — the caller skips the tick
/// instead of submitting a junk timestamp.
fn unix_now_secs() -> Option<i64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

impl Nokkvi {
    /// Arm the debounced "now playing" emit for `song_id`.
    ///
    /// Bumps `now_playing_timer_id` first so any in-flight timer from a prior
    /// context (a previous song, or a repeat-one loop) is invalidated, then
    /// returns a task that waits the debounce interval before dispatching a
    /// single `NowPlaying`. Shared by `handle_scrobble_on_song_change` and
    /// `handle_scrobble_track_looped` so the two paths never drift.
    pub(crate) fn arm_now_playing(&mut self, song_id: String) -> Task<Message> {
        self.scrobble.now_playing_timer_id += 1;
        let timer_id = self.scrobble.now_playing_timer_id;
        debug!(
            "📊 [SCROBBLE] Arming now-playing timer (id={}) for: {}",
            timer_id, song_id
        );
        Task::perform(
            async move {
                tokio::time::sleep(std::time::Duration::from_secs(NOW_PLAYING_DEBOUNCE_SECS)).await;
                (timer_id, song_id)
            },
            |(timer_id, song_id)| Message::Scrobble(ScrobbleMessage::NowPlaying(timer_id, song_id)),
        )
    }

    pub(crate) fn handle_scrobble_now_playing(
        &mut self,
        timer_id: u64,
        song_id: String,
    ) -> Task<Message> {
        // Skip if scrobbling is disabled
        if !self.settings.scrobbling_enabled {
            return Task::none();
        }
        // Only send if timer_id matches (not stale from rapid song changes)
        if timer_id != self.scrobble.now_playing_timer_id {
            return Task::none();
        }
        debug!(" [SCROBBLE] Sending now-playing for: {}", song_id);
        // Capture the song id + the live timer generation so the success path
        // can chain a periodic refresh keyed on this generation. A later song
        // change / loop bumps now_playing_timer_id, which the refresh handler
        // checks before re-emitting, so a stale heartbeat self-cancels.
        let refresh_id = song_id.clone();
        self.shell_task(
            move |shell| async move {
                let auth = shell.auth();
                let (server_url, cred) = auth.server_config().await;

                let send_result = if let Some(api_client) = auth.get_client().await {
                    let http_client = api_client.http_client();
                    let url = format!(
                        "{server_url}/rest/scrobble?id={song_id}&submission=false&{cred}&f=json&v=1.8.0&c=nokkvi"
                    );
                    match http_client.get(&url).send().await {
                        Ok(resp) => {
                            if resp.status().is_success() {
                                Ok(())
                            } else {
                                Err(format!("HTTP {}", resp.status()))
                            }
                        }
                        Err(e) => Err(e.to_string()),
                    }
                } else {
                    Err("No API client".to_string())
                };

                if send_result.is_ok() {
                    // Schedule the next heartbeat. The async sleep lives inside
                    // the same task so song_id + timer_id are captured without a
                    // round-trip through the result handler (which drops them).
                    tokio::time::sleep(std::time::Duration::from_secs(NOW_PLAYING_REFRESH_SECS))
                        .await;
                }
                send_result
            },
            move |result| match result {
                Ok(()) => {
                    Message::Scrobble(ScrobbleMessage::NowPlayingRefresh(timer_id, refresh_id))
                }
                Err(e) => Message::Scrobble(ScrobbleMessage::NowPlayingResult(Err(e))),
            },
        )
    }

    /// Periodic now-playing heartbeat. Re-arms a now-playing emit for the same
    /// song only while the heartbeat is still live: the timer generation must
    /// still match (a song change or repeat-one loop bumps it and cancels the
    /// chain), playback must be active (playing, not paused), and the source
    /// must be the queue (radio has its own metadata path). Any other state is
    /// a no-op, so a single stale chain dies on the next tick.
    ///
    /// Re-arming bumps `now_playing_timer_id`, so the just-superseded heartbeat
    /// (if any duplicate is in flight) self-cancels via the generation guard,
    /// and the fresh emit chains the next refresh on its own success.
    pub(crate) fn handle_scrobble_now_playing_refresh(
        &mut self,
        timer_id: u64,
        song_id: String,
    ) -> Task<Message> {
        if !self.settings.scrobbling_enabled {
            return Task::none();
        }
        let live = timer_id == self.scrobble.now_playing_timer_id
            && self.playback.playing
            && !self.playback.paused
            && self.active_playback.is_queue();
        if !live {
            return Task::none();
        }
        debug!(
            " [SCROBBLE] Heartbeat re-arming now-playing for: {}",
            song_id
        );
        self.arm_now_playing(song_id)
    }

    pub(crate) fn handle_scrobble_submit(&mut self, song_id: String) -> Task<Message> {
        // Skip if scrobbling is disabled
        if !self.settings.scrobbling_enabled {
            return Task::none();
        }
        debug!(" [SCROBBLE] Submitting scrobble for: {}", song_id);
        let return_id = song_id.clone();
        self.shell_task(
            move |shell| async move {
                let auth = shell.auth();
                let (server_url, cred) = auth.server_config().await;

                if let Some(api_client) = auth.get_client().await {
                    let http_client = api_client.http_client();
                    let url = format!(
                        "{server_url}/rest/scrobble?id={song_id}&submission=true&{cred}&f=json&v=1.8.0&c=nokkvi"
                    );
                    match http_client.get(&url).send().await {
                        Ok(resp) => {
                            if resp.status().is_success() {
                                Ok(return_id)
                            } else {
                                Err(format!("HTTP {}", resp.status()))
                            }
                        }
                        Err(e) => Err(e.to_string()),
                    }
                } else {
                    Err("No API client".to_string())
                }
            },
            |result| Message::Scrobble(ScrobbleMessage::SubmissionResult(result)),
        )
    }

    /// Handle the result of a now-playing heartbeat. The server doesn't count
    /// these as plays, so no UI state changes — just log.
    pub(crate) fn handle_scrobble_now_playing_result(
        &mut self,
        result: Result<(), String>,
    ) -> Task<Message> {
        match result {
            Ok(()) => debug!(" [SCROBBLE] ✅ Now-playing accepted"),
            Err(e) => debug!(" [SCROBBLE] ❌ Now-playing error: {}", e),
        }
        Task::none()
    }

    /// Handle the result of a scrobble submission. On success, dispatch a local
    /// play-count increment so the UI tracks Navidrome without needing a refetch.
    pub(crate) fn handle_scrobble_submission_result(
        &mut self,
        result: Result<String, String>,
    ) -> Task<Message> {
        match result {
            Ok(song_id) => {
                debug!(" [SCROBBLE] ✅ Submission accepted for {}", song_id);
                // Latch on CONFIRMED success only, and clear the in-flight guard.
                self.scrobble.submitted = true;
                self.scrobble.submission_in_flight = false;
                // Rating reminder (scrobble-confirmed trigger): a confirmed
                // scrobble means the server counted a real listen — the moment
                // to nudge the user to rate the track before it's gone. This one
                // handler is the fan-in for all submission paths (mid-playback,
                // song-change fallback, repeat-one), so it covers them with no
                // duplication. Suppression lives in `maybe_fire_rating_reminder`.
                if self.settings.rating_reminder_trigger.is_scrobble() {
                    self.maybe_fire_rating_reminder(&song_id);
                }
                Task::done(Message::Hotkey(HotkeyMessage::SongPlayCountIncremented(
                    song_id,
                )))
            }
            Err(e) => {
                // Leave `submitted` false so the next tick retries this song's
                // scrobble; clear the in-flight guard so the retry can fire.
                // Transient blips (offline, 5xx) thus survive within the song.
                warn!(" [SCROBBLE] Submission failed (will retry): {}", e);
                self.scrobble.submission_in_flight = false;
                Task::none()
            }
        }
    }

    /// Handle a track-looped event from the audio engine (repeat-one mode).
    ///
    /// Called when the engine fires `PlaybackController::looped_callback` after
    /// detecting that the same URL was reloaded (i.e., `is_loop = true`). This is
    /// the authoritative signal for repeat-one loops — it replaces the old
    /// position-rewind heuristic in `track_listening_time`.
    ///
    /// Submits a scrobble for the play that just completed (if threshold was met),
    /// then resets `ScrobbleState` so the next loop can accumulate fresh listening
    /// time and potentially scrobble again.
    pub(crate) fn handle_scrobble_track_looped(&mut self, song_id: String) -> Task<Message> {
        debug!(
            "📊 [SCROBBLE] TrackLooped received for song: {} (listened {:.0}s, submitted: {})",
            song_id, self.scrobble.listening_time, self.scrobble.submitted
        );

        let dur = self.playback.duration;

        // Submit if threshold was met and not already submitted
        let submit_task = if self
            .scrobble
            .should_scrobble(dur, self.settings.scrobble_threshold)
        {
            debug!(
                "📊 [SCROBBLE] Submitting on loop: {} (listened {:.0}s / {}s total)",
                song_id, self.scrobble.listening_time, dur
            );
            Task::done(Message::Scrobble(ScrobbleMessage::Submit(song_id.clone())))
        } else {
            Task::none()
        };

        // Reset for the next loop (position 0, fresh listening time,
        // submitted=false). The looped track keeps the same duration.
        self.scrobble
            .reset_for_new_song(Some(song_id.clone()), 0.0, dur);

        // Debounced "now playing" — shared arming helper bumps the timer
        // generation so any in-flight timer from a previous context is
        // invalidated, then waits the debounce interval before emitting.
        let now_playing_task = self.arm_now_playing(song_id);

        Task::batch(vec![submit_task, now_playing_task])
    }

    /// Submit a radio now-playing update directly to the scrobble service
    /// (ListenBrainz). No-ops in the backend when no token is configured.
    pub(crate) fn handle_radio_now_playing(&mut self, track: ScrobbleTrack) -> Task<Message> {
        debug!(
            " [SCROBBLE] Radio now-playing: {} - {}",
            track.artist, track.title
        );
        self.shell_task(
            move |shell| async move {
                shell
                    .radio_scrobble_now_playing(track)
                    .await
                    .map_err(|e| e.to_string())
            },
            |result| Message::Scrobble(ScrobbleMessage::RadioResult(result)),
        )
    }

    /// Submit a completed radio listen directly to the scrobble service,
    /// timestamped at the track's start. No-ops in the backend when no token is
    /// configured.
    pub(crate) fn handle_radio_submit(
        &mut self,
        track: ScrobbleTrack,
        started_at: i64,
        targets: ScrobbleTargets,
    ) -> Task<Message> {
        debug!(
            " [SCROBBLE] Radio submit: {} - {} @ {} (lb={}, lf={})",
            track.artist, track.title, started_at, targets.listenbrainz, targets.lastfm
        );
        // Carry the key back so the timer can latch each target independently.
        let artist = track.artist.clone();
        let title = track.title.clone();
        self.shell_task(
            move |shell| async move {
                shell
                    .radio_scrobble_submit(track, started_at, targets)
                    .await
            },
            move |outcome| {
                Message::Scrobble(ScrobbleMessage::RadioSubmitResult {
                    artist,
                    title,
                    outcome,
                })
            },
        )
    }

    /// Handle a radio now-playing result. Best-effort: logged only, so a
    /// transient now-playing hiccup never toast-spams during listening.
    pub(crate) fn handle_radio_result(&mut self, result: Result<(), String>) -> Task<Message> {
        // Now-playing is best-effort and fired on every track change AND the 30 s
        // keep-alive, so a persistent failure (offline / expired token) would
        // warn!-spam the log all session. Log at debug! like the queue path.
        match result {
            Ok(()) => debug!(" [SCROBBLE] ✅ Radio now-playing accepted"),
            Err(e) => debug!(" [SCROBBLE] Radio now-playing failed (best-effort): {}", e),
        }
        Task::none()
    }

    /// Handle a radio scrobble-submit result: latch each target on success or
    /// re-arm it (cooldown) on failure, per the bounded re-arm policy.
    pub(crate) fn handle_radio_submit_result(
        &mut self,
        artist: String,
        title: String,
        outcome: RadioSubmitOutcome,
    ) -> Task<Message> {
        // `Some(true)` accepted, `Some(false)` failed (will re-arm), `None` not
        // attempted (unconfigured / already done).
        let lb = outcome.listenbrainz.as_ref().map(Result::is_ok);
        let lf = outcome.lastfm.as_ref().map(Result::is_ok);
        if let Some(Err(e)) = &outcome.listenbrainz {
            debug!(" [SCROBBLE] ListenBrainz scrobble failed (will re-arm): {e}");
        }
        if let Some(Err(e)) = &outcome.lastfm {
            debug!(" [SCROBBLE] Last.fm scrobble failed (will re-arm): {e}");
        }
        debug!(" [SCROBBLE] Radio scrobble outcome {artist} - {title}: lb={lb:?} lf={lf:?}");
        let now = unix_now_secs().unwrap_or(0);
        self.radio_scrobble
            .mark_outcome(&artist, &title, now, lb, lf);
        Task::none()
    }

    /// True when a higher-precedence layer (env / config.toml) supplies the
    /// ListenBrainz token, so a redb (GUI) write is shadowed.
    fn listenbrainz_shadowed_by_higher_layer(&self) -> bool {
        self.app_service
            .as_ref()
            .is_some_and(|s| s.radio_credentials().listenbrainz_source.shadows_redb())
    }

    /// True when a higher-precedence layer supplies the Last.fm key/secret.
    fn lastfm_shadowed_by_higher_layer(&self) -> bool {
        self.app_service
            .as_ref()
            .is_some_and(|s| s.radio_credentials().lastfm_source.shadows_redb())
    }

    /// Handle a ListenBrainz token set/verify result with a user-facing toast.
    pub(crate) fn handle_radio_verify_result(
        &mut self,
        result: Result<Option<String>, String>,
    ) -> Task<Message> {
        match result {
            // Clearing only removes the redb (GUI) value. If env/config still
            // supplies a token, the disconnect didn't actually take — say so
            // honestly rather than a misleading "cleared" (review #2).
            Ok(None) if self.listenbrainz_shadowed_by_higher_layer() => self
                .toast_warn("Cleared the saved token, but a config.toml/env value is still active"),
            Ok(None) => self.toast_success("ListenBrainz token cleared"),
            Ok(Some(name)) if name.is_empty() => self.toast_success("ListenBrainz connected"),
            Ok(Some(name)) => self.toast_success(format!("ListenBrainz connected as {name}")),
            Err(e) => self.toast_error(format!("ListenBrainz: {e}")),
        }
        // Refresh the settings rows so the connection status updates at once.
        self.settings_page.config_dirty = true;
        self.refresh_settings_entries_if_dirty();
        Task::none()
    }

    /// Last.fm desktop-auth step 1: open the authorize URL in a browser and the
    /// confirm dialog that completes the exchange once the user has authorized.
    pub(crate) fn handle_lastfm_auth_started(
        &mut self,
        result: Result<(String, String), String>,
    ) -> Task<Message> {
        match result {
            Ok((token, url)) => {
                debug!(
                    " [SCROBBLE] Last.fm auth: token obtained, opening browser + confirm dialog"
                );
                // The authorize URL carries the request token — a credential that
                // can be exchanged for a session during the auth window. Don't log
                // it on the happy path (the browser already has it). Only emit it
                // when xdg-open fails and the user must open it manually; the log
                // is created 0600 so it stays owner-readable.
                if let Err(e) = std::process::Command::new("xdg-open").arg(&url).spawn() {
                    warn!(" [SCROBBLE] Failed to open Last.fm authorize URL: {e}");
                    tracing::info!(" [SCROBBLE] Open this URL to authorize Last.fm: {url}");
                    self.toast_error("Could not open browser — copy the URL from the log");
                }
                self.text_input_dialog.open_lastfm_auth_confirmation(token);
                Task::none()
            }
            Err(e) => {
                warn!(" [SCROBBLE] Last.fm begin-auth failed: {e}");
                self.toast_error(format!("Last.fm: {e}"));
                Task::none()
            }
        }
    }

    /// Handle a Last.fm credentials-saved / connect result with a toast.
    pub(crate) fn handle_lastfm_auth_result(
        &mut self,
        result: Result<String, String>,
    ) -> Task<Message> {
        match result {
            Ok(name) if name.is_empty() => {
                debug!(" [SCROBBLE] Last.fm credentials saved");
                if self.lastfm_shadowed_by_higher_layer() {
                    self.toast_warn(
                        "Saved, but a config.toml/env Last.fm key/secret still overrides these",
                    );
                } else {
                    self.toast_success("Last.fm credentials saved — now click Connect Last.fm");
                }
            }
            Ok(name) => {
                tracing::info!(" [SCROBBLE] Last.fm connected as {name}");
                self.toast_success(format!("Last.fm connected as {name}"));
            }
            Err(e) => {
                warn!(" [SCROBBLE] Last.fm auth/save failed: {e}");
                self.toast_error(format!("Last.fm: {e}"));
            }
        }
        // Refresh the settings rows so the connection status updates at once.
        self.settings_page.config_dirty = true;
        self.refresh_settings_entries_if_dirty();
        Task::none()
    }

    /// Drive radio scrobbling from the playback tick. Self-gating: clears all
    /// tracking when not in radio playback, otherwise observes the current ICY
    /// track (dispatching a now-playing on a genuine change) and accumulates
    /// listen time (dispatching a scrobble once the absolute threshold is met).
    ///
    /// Called once per `PlaybackStateUpdated` after the queue-scrobble block.
    /// The listen timer is wall-clock based (seconds since the track was first
    /// observed), so it needs no engine position. Title-only / unparseable ICY
    /// is skipped (no artist → not scrobbled), and submissions only accrue
    /// while actually playing.
    pub(crate) fn handle_radio_scrobble_tick(
        &mut self,
        playing: bool,
        paused: bool,
        tasks: &mut Vec<Task<Message>>,
    ) {
        // Feature gate: when radio scrobbling is off, keep no tracking so a
        // later enable starts fresh.
        if !self.settings.radio_scrobbling_enabled {
            self.radio_scrobble.clear();
            return;
        }

        // Borrow the radio metadata. `station_name` is cloned (owned for the
        // action-driven tracks below); the ICY fields stay borrowed so the
        // unchanged-tick fast path allocates nothing. Non-radio clears tracking.
        let (icy_artist, icy_title, station_name) = match &self.active_playback {
            crate::state::ActivePlayback::Radio(r) => (
                r.icy_artist.as_deref(),
                r.icy_title.as_deref(),
                r.station.name.clone(),
            ),
            crate::state::ActivePlayback::Queue => {
                self.radio_scrobble.clear();
                return;
            }
        };

        let Some(now) = unix_now_secs() else {
            // Pre-epoch clock — can't produce a valid scrobble timestamp this tick.
            return;
        };
        // Playback active = playing and not paused. The timer accrues only while
        // active, but `tick` still runs so `last_tick` tracks the pause (a resume
        // doesn't credit the paused gap).
        let active = playing && !paused;
        let now_playing_on = self.settings.radio_now_playing_enabled;
        let threshold_secs = i64::from(self.settings.radio_scrobble_threshold_secs);

        // Flush the CURRENT (possibly outgoing) track FIRST, before `observe`
        // can replace it on a title change — a track that crosses the threshold
        // on the same tick its ICY title flips still scrobbles (review #6). The
        // action carries the already-cleaned artist/title, so no re-clean here.
        if let crate::state::RadioScrobbleAction::Scrobble {
            artist,
            title,
            started_at,
            targets,
        } = self.radio_scrobble.tick(now, active, threshold_secs)
        {
            debug!(" [SCROBBLE] Radio listen threshold met → submitting {artist} - {title}");
            tasks.push(Task::done(Message::Scrobble(ScrobbleMessage::RadioSubmit(
                ScrobbleTrack::from_clean(artist, title, Some(station_name.clone())),
                started_at,
                targets,
            ))));
        }

        // Only rebuild + re-observe when the raw ICY actually changed — skips the
        // clean()/alloc churn on the ~10 Hz ticks where the title is unchanged
        // (review #12).
        if self.radio_scrobble.raw_icy_changed(icy_artist, icy_title)
            && let Some(track) =
                ScrobbleTrack::from_icy(icy_artist, icy_title, None, Some(&station_name))
            && matches!(
                self.radio_scrobble
                    .observe(&track.artist, &track.title, now),
                crate::state::RadioScrobbleAction::NowPlaying { .. }
            )
        {
            debug!(
                " [SCROBBLE] Radio track change: {} - {}",
                track.artist, track.title
            );
            if now_playing_on && active {
                tasks.push(Task::done(Message::Scrobble(
                    ScrobbleMessage::RadioNowPlaying(track),
                )));
            }
        }

        // Now-playing keep-alive: re-send periodically so the service's
        // now-playing indicator doesn't expire on a long single-title segment.
        if now_playing_on
            && let Some((artist, title)) = self.radio_scrobble.now_playing_refresh_due(
                now,
                active,
                NOW_PLAYING_REFRESH_SECS as i64,
            )
        {
            tasks.push(Task::done(Message::Scrobble(
                ScrobbleMessage::RadioNowPlaying(ScrobbleTrack::from_clean(
                    artist,
                    title,
                    Some(station_name),
                )),
            )));
        }
    }

    /// Dispatch a `ScrobbleMessage` to its handler.
    pub(super) fn dispatch_scrobble(&mut self, msg: ScrobbleMessage) -> Task<Message> {
        match msg {
            ScrobbleMessage::NowPlaying(timer_id, song_id) => {
                self.handle_scrobble_now_playing(timer_id, song_id)
            }
            ScrobbleMessage::Submit(song_id) => self.handle_scrobble_submit(song_id),
            ScrobbleMessage::SubmissionResult(result) => {
                self.handle_scrobble_submission_result(result)
            }
            ScrobbleMessage::NowPlayingResult(result) => {
                self.handle_scrobble_now_playing_result(result)
            }
            ScrobbleMessage::TrackLooped(song_id) => self.handle_scrobble_track_looped(song_id),
            ScrobbleMessage::NowPlayingRefresh(timer_id, song_id) => {
                self.handle_scrobble_now_playing_refresh(timer_id, song_id)
            }
            ScrobbleMessage::RadioNowPlaying(track) => self.handle_radio_now_playing(track),
            ScrobbleMessage::RadioSubmit(track, started_at, targets) => {
                self.handle_radio_submit(track, started_at, targets)
            }
            ScrobbleMessage::RadioResult(result) => self.handle_radio_result(result),
            ScrobbleMessage::RadioSubmitResult {
                artist,
                title,
                outcome,
            } => self.handle_radio_submit_result(artist, title, outcome),
            ScrobbleMessage::RadioVerifyResult(result) => self.handle_radio_verify_result(result),
            ScrobbleMessage::LastfmAuthStarted(result) => self.handle_lastfm_auth_started(result),
            ScrobbleMessage::LastfmAuthResult(result) => self.handle_lastfm_auth_result(result),
        }
    }
}
