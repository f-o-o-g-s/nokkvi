//! The Waveform seek bar's envelopes: keep one on screen for the playing song.
//!
//! The 100 ms playback tick level-sets everything, like the cover accent: a
//! track change, the style switched on or off, radio, a stop and logout all
//! resolve on the next tick. Each tick starts at most one job (a cache read, a
//! scan, or asking which song plays next), picked by
//! [`WaveformState::plan`](crate::state::WaveformState::plan); results come
//! back as [`WaveformMessage`]s. A scan runs through `AppService` against the
//! song's own stream and is cancelled when its song stops being the playing
//! or next one.

use std::sync::Arc;

use iced::Task;
use nokkvi_data::{
    services::waveform::{ScanEnd, WaveformLookup},
    types::{player_settings::SeekbarStyle, waveform::Waveform},
};
use tracing::warn;

use crate::{
    Nokkvi,
    app_message::{Message, WaveformMessage},
    state::WaveformJob,
    theme,
};

impl Nokkvi {
    /// Level-set the waveform against what is playing. Cheap when nothing
    /// changed: no job starts while one is in flight or once the playing song
    /// and the next are settled.
    pub(crate) fn waveform_tick(&mut self) -> Task<Message> {
        let wanted = theme::active_seekbar_style() == SeekbarStyle::Waveform
            && self.active_playback.is_queue()
            && self.playback.has_track();
        let playing = if wanted {
            self.scrobble.current_song_id.as_deref()
        } else {
            None
        };
        self.waveform.follow(playing);
        match self
            .waveform
            .plan(self.playback.position, std::time::Instant::now())
        {
            Some(job) => self.start_waveform_job(job),
            None => Task::none(),
        }
    }

    fn start_waveform_job(&mut self, job: WaveformJob) -> Task<Message> {
        let Some(shell) = &self.app_service else {
            return Task::none();
        };
        match &job {
            WaveformJob::Lookup(song_id) => {
                let job = self.waveform.started(&job, None);
                let (sid, song_id) = (song_id.clone(), song_id.clone());
                self.shell_task(
                    move |shell| async move { shell.cached_waveform(&sid).await },
                    move |lookup| {
                        Message::Waveform(WaveformMessage::Looked {
                            job,
                            song_id,
                            lookup,
                        })
                    },
                )
            }
            WaveformJob::Scan(song_id) => {
                let cancel = shell.task_manager().child_token();
                let job = self.waveform.started(&job, Some(cancel.clone()));
                let (sid, song_id) = (song_id.clone(), song_id.clone());
                self.shell_task(
                    move |shell| async move {
                        let end = shell
                            .scan_waveform(&sid, cancel)
                            .await
                            .map_err(|e| format!("{e:#}"));
                        (end, shell.waveform_scanning())
                    },
                    move |(end, scanning)| {
                        Message::Waveform(WaveformMessage::Scanned {
                            job,
                            song_id,
                            end,
                            scanning,
                        })
                    },
                )
            }
            WaveformJob::FindNext => {
                let Some(for_current) = self.waveform.current.clone() else {
                    return Task::none();
                };
                let job = self.waveform.started(&job, None);
                self.shell_task(
                    move |shell| async move { shell.next_song_id_in_play_order().await },
                    move |next| {
                        Message::Waveform(WaveformMessage::NextFound {
                            job,
                            for_current,
                            next,
                        })
                    },
                )
            }
        }
    }

    pub(crate) fn handle_waveform(&mut self, message: WaveformMessage) -> Task<Message> {
        let state = &mut self.waveform;
        match message {
            WaveformMessage::Looked {
                job,
                song_id,
                lookup,
            } => {
                state.finished(job);
                match lookup {
                    WaveformLookup::Cached(waveform) => state.place(&song_id, waveform),
                    WaveformLookup::NotCached => {
                        state.not_cached.insert(song_id);
                    }
                    WaveformLookup::NotInQueue => state.song_gone(song_id),
                }
            }
            WaveformMessage::Scanned {
                job,
                song_id,
                end,
                scanning,
            } => {
                state.finished(job);
                state.scans_off = !scanning;
                match end {
                    Ok(ScanEnd::Ready(waveform)) => {
                        state.not_cached.remove(&song_id);
                        state.place(&song_id, waveform);
                    }
                    Ok(ScanEnd::Cancelled) => {}
                    Ok(ScanEnd::NotInQueue) => state.song_gone(song_id),
                    Ok(ScanEnd::Truncated { .. } | ScanEnd::TooSlow { .. }) => {
                        state.gave_up.insert(song_id);
                    }
                    Err(e) => {
                        // Logged here, where the scan finally ends; the bar
                        // keeps drawing Line and the next play retries.
                        warn!(song = %song_id, error = %e, "waveform scan failed");
                        state.gave_up.insert(song_id);
                    }
                }
            }
            WaveformMessage::NextFound {
                job,
                for_current,
                next,
            } => {
                state.finished(job);
                state.next_found(for_current, next, std::time::Instant::now());
            }
        }
        Task::none()
    }

    /// The envelope the player bar draws: the one on screen, while it still
    /// belongs to the song that is playing (between a track change and the
    /// tick that follows it, the old one would draw under the new song).
    pub(crate) fn waveform_for_bar(&self) -> Option<Arc<Waveform>> {
        (self.active_playback.is_queue()
            && self.waveform.current.is_some()
            && self.waveform.current == self.scrobble.current_song_id)
            .then(|| self.waveform.shown.clone())
            .flatten()
    }
}
