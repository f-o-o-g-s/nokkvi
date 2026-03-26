//! About modal handler — open, close, copy.

use iced::Task;

use crate::{Nokkvi, app_message::Message, widgets::about_modal::AboutModalMessage};

impl Nokkvi {
    /// Handle about modal messages (open, close, copy).
    pub(crate) fn handle_about_modal(&mut self, msg: AboutModalMessage) -> Task<Message> {
        match msg {
            AboutModalMessage::Open => {
                self.about_modal.open();
            }
            AboutModalMessage::Close => {
                self.about_modal.close();
            }
            AboutModalMessage::CopyAll => {
                let version = env!("CARGO_PKG_VERSION");
                let git_hash = env!("GIT_HASH");
                let server_url = &self.login_page.server_url;
                let username = &self.login_page.username;

                let mut lines = vec![format!("Nokkvi v{version}")];
                if !git_hash.is_empty() {
                    lines.push(format!("Commit: {git_hash}"));
                }
                if !server_url.is_empty() {
                    lines.push(format!("Server: {server_url}"));
                }
                if !username.is_empty() {
                    lines.push(format!("User: {username}"));
                }
                lines.push("Toolkit: Iced (wgpu)".to_string());

                let text = lines.join("\n");
                self.toast_info("Copied to clipboard");
                return iced::clipboard::write(text).discard();
            }
        }
        Task::none()
    }
}
