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
            PlayerBarMessage::ToggleRandom => {
                Task::done(Message::Playback(PlaybackMessage::ToggleRandom))
            }
            PlayerBarMessage::ToggleRepeat => {
                Task::done(Message::Playback(PlaybackMessage::ToggleRepeat))
            }
            PlayerBarMessage::ToggleConsume => {
                Task::done(Message::Playback(PlaybackMessage::ToggleConsume))
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
            PlayerBarMessage::ScrollVolume(delta) => {
                let new_vol = (self.playback.volume + delta).clamp(0.0, 1.0);
                let pct = (new_vol * 100.0) as u32;
                let toast =
                    nokkvi_data::types::toast::Toast::info_short(format!("Volume: {pct}%",))
                        .right_aligned();
                Task::batch([
                    Task::done(Message::Playback(PlaybackMessage::VolumeChanged(new_vol))),
                    Task::done(Message::Toast(crate::app_message::ToastMessage::Push(
                        toast,
                    ))),
                ])
            }
            PlayerBarMessage::OpenSettings => {
                Task::done(Message::SwitchView(crate::View::Settings))
            }
            PlayerBarMessage::GoToQueue => Task::done(Message::SwitchView(crate::View::Queue)),
            PlayerBarMessage::StripClicked => Task::done(Message::StripClicked),
            PlayerBarMessage::StripContextAction(entry) => {
                Task::done(Message::StripContextAction(entry))
            }
            PlayerBarMessage::ToggleLightMode => Task::done(Message::ToggleLightMode),
            PlayerBarMessage::About => Task::done(Message::AboutModal(
                crate::widgets::about_modal::AboutModalMessage::Open,
            )),
            PlayerBarMessage::Quit => Task::done(Message::QuitApp),
        }
    }
}
