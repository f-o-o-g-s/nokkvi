//! MPRIS event handlers
//!
//! Converts MPRIS D-Bus events to existing app messages.

use iced::Task;
use mpris_server::LoopStatus;
use tracing::debug;

use crate::{Message, Nokkvi, app_message::PlaybackMessage, services::mpris::MprisEvent};

impl Nokkvi {
    /// Handle MPRIS events from D-Bus
    pub fn handle_mpris(&mut self, event: MprisEvent) -> Task<Message> {
        match event {
            MprisEvent::Connected(connection) => {
                debug!(" MPRIS connected - storing connection handle");
                self.mpris_connection = Some(connection);
                Task::none()
            }

            MprisEvent::PlayPause => {
                debug!(" MPRIS: PlayPause");
                Task::done(Message::Playback(PlaybackMessage::TogglePlay))
            }

            MprisEvent::Play => {
                debug!(" MPRIS: Play");
                Task::done(Message::Playback(PlaybackMessage::Play))
            }

            MprisEvent::Pause => {
                debug!(" MPRIS: Pause");
                Task::done(Message::Playback(PlaybackMessage::Pause))
            }

            MprisEvent::Stop => {
                debug!(" MPRIS: Stop");
                Task::done(Message::Playback(PlaybackMessage::Stop))
            }

            MprisEvent::Next => {
                debug!(" MPRIS: Next");
                Task::done(Message::Playback(PlaybackMessage::NextTrack))
            }

            MprisEvent::Previous => {
                debug!(" MPRIS: Previous");
                Task::done(Message::Playback(PlaybackMessage::PrevTrack))
            }

            MprisEvent::Seek(offset_us) => {
                // Convert offset to new absolute position
                let current_pos_s = self.playback.position as f32;
                let offset_s = (offset_us as f64 / 1_000_000.0) as f32;
                let new_pos_s = (current_pos_s + offset_s).max(0.0);
                debug!(" MPRIS: Seek offset={offset_us}µs → new_pos={new_pos_s}s");
                Task::done(Message::Playback(PlaybackMessage::Seek(new_pos_s)))
            }

            MprisEvent::SetPosition(position_us) => {
                let position_s = (position_us as f64 / 1_000_000.0) as f32;
                debug!(" MPRIS: SetPosition {position_us}µs → {position_s}s");
                Task::done(Message::Playback(PlaybackMessage::Seek(position_s)))
            }

            MprisEvent::SetVolume(volume) => {
                // Clamp to valid 0.0-1.0 range to prevent playerctl from going above 100%.
                // Route through VolumeCommitted because external D-Bus volume sets
                // (headset buttons, playerctl, hardware controls) are discrete user
                // commands, not drag intermediates — they must bypass the 500ms
                // VolumeChanged throttle so rapid presses don't drop on next launch.
                let volume_f32 = (volume as f32).clamp(0.0, 1.0);
                debug!(" MPRIS: SetVolume {volume} → {volume_f32}");
                Task::done(Message::Playback(PlaybackMessage::VolumeCommitted(
                    volume_f32,
                )))
            }

            MprisEvent::SetLoopStatus(status) => {
                debug!(" MPRIS: SetLoopStatus {:?}", status);
                // Map MPRIS LoopStatus to our repeat toggle
                // None = no repeat, Track = repeat current, Playlist = repeat queue
                match status {
                    LoopStatus::None => {
                        // Turn off repeat modes if currently enabled
                        if self.modes.repeat || self.modes.repeat_queue {
                            Task::done(Message::Playback(PlaybackMessage::ToggleRepeat))
                        } else {
                            Task::none()
                        }
                    }
                    LoopStatus::Track | LoopStatus::Playlist => {
                        // Toggle repeat to cycle to requested mode
                        Task::done(Message::Playback(PlaybackMessage::ToggleRepeat))
                    }
                }
            }

            MprisEvent::SetShuffle(shuffle) => {
                debug!(" MPRIS: SetShuffle {shuffle}");
                // Only toggle if the requested state differs from current
                if shuffle != self.modes.random {
                    Task::done(Message::Playback(PlaybackMessage::ToggleRandom))
                } else {
                    Task::none()
                }
            }
        }
    }
}
