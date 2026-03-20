//! Scrobbling handlers

use iced::Task;
use tracing::debug;

use crate::{
    Nokkvi,
    app_message::{Message, ScrobbleMessage},
};

impl Nokkvi {
    pub(crate) fn handle_scrobble_now_playing(
        &mut self,
        timer_id: u64,
        song_id: String,
    ) -> Task<Message> {
        // Skip if scrobbling is disabled
        if !self.scrobbling_enabled {
            return Task::none();
        }
        // Only send if timer_id matches (not stale from rapid song changes)
        if timer_id != self.scrobble.now_playing_timer_id {
            return Task::none();
        }
        debug!(" [SCROBBLE] Sending now-playing for: {}", song_id);
        self.shell_task(
            move |shell| async move {
                let auth = shell.auth();
                let server_url = auth.get_server_url().await;
                let cred = auth.get_subsonic_credential().await;

                if let Some(api_client) = auth.get_client().await {
                    let http_client = api_client.http_client();
                    let url = format!(
                        "{server_url}/rest/scrobble?id={song_id}&submission=false&{cred}&f=json&v=1.8.0&c=nokkvi"
                    );
                    match http_client.get(&url).send().await {
                        Ok(resp) => {
                            if resp.status().is_success() {
                                Ok(format!("Now-playing sent for {song_id}"))
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
            |result| Message::Scrobble(ScrobbleMessage::Result(result.map(|_| ()))),
        )
    }

    pub(crate) fn handle_scrobble_submit(&mut self, song_id: String) -> Task<Message> {
        // Skip if scrobbling is disabled
        if !self.scrobbling_enabled {
            return Task::none();
        }
        debug!(" [SCROBBLE] Submitting scrobble for: {}", song_id);
        self.shell_task(
            move |shell| async move {
                let auth = shell.auth();
                let server_url = auth.get_server_url().await;
                let cred = auth.get_subsonic_credential().await;

                if let Some(api_client) = auth.get_client().await {
                    let http_client = api_client.http_client();
                    let url = format!(
                        "{server_url}/rest/scrobble?id={song_id}&submission=true&{cred}&f=json&v=1.8.0&c=nokkvi"
                    );
                    match http_client.get(&url).send().await {
                        Ok(resp) => {
                            if resp.status().is_success() {
                                Ok(format!("Scrobble submitted for {song_id}"))
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
            |result| Message::Scrobble(ScrobbleMessage::Result(result.map(|_| ()))),
        )
    }

    pub(crate) fn handle_scrobble_result(&mut self, result: Result<(), String>) -> Task<Message> {
        match result {
            Ok(()) => debug!(" [SCROBBLE] ✅ Success"),
            Err(e) => debug!(" [SCROBBLE] ❌ Error: {}", e),
        }
        Task::none()
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
        let submit_task = if self.scrobble.should_scrobble(dur, self.scrobble_threshold) {
            debug!(
                "📊 [SCROBBLE] Submitting on loop: {} (listened {:.0}s / {}s total)",
                song_id, self.scrobble.listening_time, dur
            );
            Task::done(Message::Scrobble(ScrobbleMessage::Submit(song_id.clone())))
        } else {
            Task::none()
        };

        // Reset for the next loop (position 0, fresh listening time, submitted=false)
        self.scrobble.reset_for_new_song(Some(song_id.clone()), 0.0);

        // Debounced "now playing" — mirrors handle_scrobble_on_song_change exactly:
        // increment timer ID first so any in-flight timer from a previous context
        // is invalidated, then wait 2 seconds before submitting.
        self.scrobble.now_playing_timer_id += 1;
        let timer_id = self.scrobble.now_playing_timer_id;
        let sid = song_id.clone();
        debug!(
            "📊 [SCROBBLE] Loop: starting now-playing timer (id={}) for: {}",
            timer_id, sid
        );
        let now_playing_task = Task::perform(
            async move {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                (timer_id, sid)
            },
            |(timer_id, song_id)| Message::Scrobble(ScrobbleMessage::NowPlaying(timer_id, song_id)),
        );

        Task::batch(vec![submit_task, now_playing_task])
    }
}
