//! System tray event handlers.
//!
//! Translates `TrayEvent`s emitted by the ksni service into application
//! messages, and bridges window-close interception (`WindowCloseRequested`)
//! into either a window-destroy or a quit, depending on user settings.
//!
//! ## Why we destroy + reopen instead of hiding
//!
//! Wayland intentionally does not let an application hide its own surface —
//! the compositor controls visibility. `winit::Window::set_visible(false)`
//! is a documented no-op on Wayland, so `iced::window::set_mode(id, Hidden)`
//! does nothing on Hyprland / KDE / GNOME / sway. The only portable Wayland
//! pattern is to `iced::window::close(id)` on hide and
//! `iced::window::open(settings)` on show — the runtime stays alive across
//! the gap because `iced::daemon` (see `main.rs`) doesn't treat
//! "no windows" as an exit condition. Audio / MPRIS / tray subscriptions
//! all keep running while the window is gone.

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

    /// Window close (X button) interception. Destroys the window into the
    /// tray when `show_tray_icon && close_to_tray`, otherwise quits.
    ///
    /// Destroying (vs. hiding) is required for Wayland — see the module
    /// docs above.
    pub fn handle_window_close_requested(&mut self, id: window::Id) -> Task<Message> {
        if self.show_tray_icon && self.close_to_tray {
            debug!(" Close requested → destroying window (will reopen via tray)");
            self.tray_window_hidden = true;
            self.main_window_id = None;
            window::close(id)
        } else {
            debug!(" Close requested → quitting app");
            Task::done(Message::QuitApp)
        }
    }

    pub fn handle_window_opened(&mut self, id: window::Id) -> Task<Message> {
        // Daemon mode: a fresh open after close-to-tray creates a window with
        // a new id. Always replace, then mark the app as visible again.
        self.main_window_id = Some(id);
        self.tray_window_hidden = false;
        Task::none()
    }

    fn toggle_window_visibility(&mut self) -> Task<Message> {
        let target_hidden = !self.tray_window_hidden;
        self.set_window_hidden(target_hidden)
    }

    fn set_window_hidden(&mut self, hidden: bool) -> Task<Message> {
        if hidden {
            // Destroy the window. main_window_id is consumed; the next show
            // path opens a fresh window with a new id (delivered via the
            // open_events subscription → handle_window_opened).
            let Some(id) = self.main_window_id.take() else {
                debug!(" Tray hide requested but no main window — nothing to close");
                return Task::none();
            };
            self.tray_window_hidden = true;
            window::close(id)
        } else {
            // Open a fresh window. handle_window_opened will set
            // main_window_id and clear tray_window_hidden once it arrives,
            // but we flip the flag here too so a rapid second Activate
            // (before WindowOpened arrives) reads the right intent.
            debug!(" Tray show requested — opening new window");
            self.tray_window_hidden = false;
            let (_id, open_task) = window::open(crate::main_window_settings());
            open_task.discard()
        }
    }
}
