//! Scrobbling handlers

use iced::Task;
use tracing::{debug, warn};

use crate::{
    Nokkvi,
    app_message::{HotkeyMessage, Message, ScrobbleMessage},
};

/// Debounce before the FIRST now-playing emit after a song change / loop.
const NOW_PLAYING_DEBOUNCE_SECS: u64 = 2;
/// Interval between periodic now-playing heartbeats, comfortably under
/// Navidrome's ephemeral now-playing TTL so a long single track keeps its
/// server-side entry alive.
const NOW_PLAYING_REFRESH_SECS: u64 = 30;

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
        }
    }
}
