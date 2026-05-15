//! PlayerBar component message handler

use iced::Task;

use crate::{
    Nokkvi,
    app_message::{Message, PlaybackMessage},
    widgets::PlayerBarMessage,
};

impl Nokkvi {
    pub(crate) fn handle_player_bar(&mut self, msg: PlayerBarMessage) -> Task<Message> {
        match msg {
            PlayerBarMessage::Play => Task::done(Message::Playback(PlaybackMessage::Play)),
            PlayerBarMessage::Pause => Task::done(Message::Playback(PlaybackMessage::Pause)),
            PlayerBarMessage::Stop => Task::done(Message::Playback(PlaybackMessage::Stop)),
            PlayerBarMessage::NextTrack => {
                Task::done(Message::Playback(PlaybackMessage::NextTrack))
            }
            PlayerBarMessage::PrevTrack => {
                Task::done(Message::Playback(PlaybackMessage::PrevTrack))
            }
            PlayerBarMessage::Seek(pos) => {
                Task::done(Message::Playback(PlaybackMessage::Seek(pos)))
            }
            PlayerBarMessage::VolumeChanged(vol) => {
                Task::done(Message::Playback(PlaybackMessage::VolumeChanged(vol)))
            }
            PlayerBarMessage::VolumeCommitted(vol) => {
                Task::done(Message::Playback(PlaybackMessage::VolumeCommitted(vol)))
            }
            PlayerBarMessage::ToggleRandom => {
                Task::done(Message::Playback(PlaybackMessage::ToggleRandom))
            }
            PlayerBarMessage::ToggleRepeat => {
                Task::done(Message::Playback(PlaybackMessage::ToggleRepeat))
            }
            PlayerBarMessage::ToggleConsume => {
                Task::done(Message::Playback(PlaybackMessage::ToggleConsume))
            }
            PlayerBarMessage::ToggleEq => {
                Task::done(Message::EqModal(crate::widgets::EqModalMessage::Toggle))
            }
            PlayerBarMessage::ToggleSoundEffects => {
                Task::done(Message::Playback(PlaybackMessage::ToggleSoundEffects))
            }
            PlayerBarMessage::SfxVolumeChanged(vol) => {
                Task::done(Message::Playback(PlaybackMessage::SfxVolumeChanged(vol)))
            }
            PlayerBarMessage::CycleVisualization => {
                Task::done(Message::Playback(PlaybackMessage::CycleVisualization))
            }
            PlayerBarMessage::ToggleCrossfade => {
                Task::done(Message::Playback(PlaybackMessage::ToggleCrossfade))
            }
            PlayerBarMessage::ScrollVolume(delta) => Task::done(
                scroll_volume_to_committed_message(self.playback.volume, delta),
            ),
            PlayerBarMessage::OpenSettings => {
                Task::done(Message::SwitchView(crate::View::Settings))
            }
            PlayerBarMessage::GoToQueue => Task::done(Message::SwitchView(crate::View::Queue)),
            PlayerBarMessage::StripClicked => Task::done(Message::StripClicked),
            PlayerBarMessage::StripContextAction(entry) => {
                Task::done(Message::StripContextAction(entry))
            }
            PlayerBarMessage::ToggleLightMode => Task::done(Message::ToggleLightMode),
            PlayerBarMessage::SetOpenMenu(next) => Task::done(Message::SetOpenMenu(next)),
            PlayerBarMessage::About => Task::done(Message::AboutModal(
                crate::widgets::about_modal::AboutModalMessage::Open,
            )),
            PlayerBarMessage::Quit => Task::done(Message::QuitApp),
        }
    }
}

/// Map a wheel-scroll delta to the `Message` it should dispatch.
///
/// Wheel scroll over the player bar is a discrete user gesture, not a drag —
/// route through `VolumeCommitted` so each notch force-persists and never
/// gets truncated by the 500 ms `VolumeChanged` throttle. Extracted as a
/// free function so tests can pin the routing variant and the clamp
/// arithmetic without driving the Iced `Task` runtime.
pub(crate) fn scroll_volume_to_committed_message(current_volume: f32, delta: f32) -> Message {
    let new_vol = (current_volume + delta).clamp(0.0, 1.0);
    Message::Playback(PlaybackMessage::VolumeCommitted(new_vol))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract_volume(msg: Message) -> f32 {
        match msg {
            Message::Playback(PlaybackMessage::VolumeCommitted(v)) => v,
            other => panic!("expected Message::Playback(VolumeCommitted), got {other:?}"),
        }
    }

    #[test]
    fn scroll_volume_routes_to_volume_committed() {
        // Pin the bug-fix invariant: wheel scrolls MUST dispatch
        // VolumeCommitted (force-persist) rather than VolumeChanged
        // (throttled). A regression here re-introduces the 500ms
        // truncation bug for wheel events.
        let msg = scroll_volume_to_committed_message(0.30, 0.05);
        let v = extract_volume(msg);
        assert!((v - 0.35).abs() < f32::EPSILON);
    }

    #[test]
    fn scroll_volume_clamps_at_upper_bound() {
        let v = extract_volume(scroll_volume_to_committed_message(0.98, 0.05));
        assert!((v - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn scroll_volume_clamps_at_lower_bound() {
        let v = extract_volume(scroll_volume_to_committed_message(0.02, -0.05));
        assert!(v.abs() < f32::EPSILON);
    }
}
