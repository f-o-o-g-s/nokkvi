//! System tray event handlers.
//!
//! Translates `TrayEvent`s emitted by the ksni service into application
//! messages, and bridges window-close interception (`WindowCloseRequested`)
//! into either a window-hide or a quit, depending on user settings.

use iced::{Task, window};
use tracing::debug;

use crate::{Message, Nokkvi, app_message::PlaybackMessage, services::tray::TrayEvent};

impl Nokkvi {
    /// Handle a tray menu activation or icon click.
    pub fn handle_tray(&mut self, event: TrayEvent) -> Task<Message> {
        match event {
            TrayEvent::Connected(connection) => {
                debug!(" Tray connected — storing handle");
                let title = if self.playback.title.is_empty() {
                    "Nokkvi".to_string()
                } else {
                    format!("{} — {}", self.playback.title, self.playback.artist)
                };
                connection.set_playing_state(self.playback.playing, title);
                self.tray_connection = Some(connection);
                Task::none()
            }
            TrayEvent::Activate => self.toggle_window_visibility(),
            TrayEvent::PlayPause => Task::done(Message::Playback(PlaybackMessage::TogglePlay)),
            TrayEvent::Next => Task::done(Message::Playback(PlaybackMessage::NextTrack)),
            TrayEvent::Previous => Task::done(Message::Playback(PlaybackMessage::PrevTrack)),
            TrayEvent::Quit => Task::done(Message::QuitApp),
        }
    }

    /// Window close (X button) interception. Hides into the tray when
    /// `show_tray_icon && close_to_tray`, otherwise quits.
    pub fn handle_window_close_requested(&mut self, id: window::Id) -> Task<Message> {
        self.main_window_id = Some(id);
        if self.show_tray_icon && self.close_to_tray {
            debug!(" Close requested → hiding window into tray");
            self.tray_window_hidden = true;
            window::set_mode(id, window::Mode::Hidden)
        } else {
            debug!(" Close requested → quitting app");
            Task::done(Message::QuitApp)
        }
    }

    pub fn handle_window_opened(&mut self, id: window::Id) -> Task<Message> {
        if self.main_window_id.is_none() {
            self.main_window_id = Some(id);
        }
        Task::none()
    }

    fn toggle_window_visibility(&mut self) -> Task<Message> {
        let target_hidden = !self.tray_window_hidden;
        self.set_window_hidden(target_hidden)
    }

    fn set_window_hidden(&mut self, hidden: bool) -> Task<Message> {
        let Some(id) = self.main_window_id else {
            debug!(" Tray show/hide requested before window id captured");
            return Task::none();
        };
        self.tray_window_hidden = hidden;
        let mode = if hidden {
            window::Mode::Hidden
        } else {
            window::Mode::Windowed
        };
        window::set_mode(id, mode)
    }
}
