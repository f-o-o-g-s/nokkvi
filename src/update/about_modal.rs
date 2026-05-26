//! About modal handler — open, close, copy.

use iced::Task;

use crate::{
    Nokkvi,
    app_message::Message,
    widgets::about_modal::{AboutModalMessage, AboutViewData, build_about_rows},
};

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
                // Share `build_about_rows` with the view so every row the
                // user sees lands in the clipboard (Captain + Shipwrights
                // attribution included). Previously this open-coded a
                // 6-line subset that dropped attribution and shuffled
                // User/Navidrome ordering.
                let data = AboutViewData {
                    server_url: &self.login_page.server_url,
                    username: &self.login_page.username,
                    server_version: self.server_version.as_deref(),
                };
                let text = build_about_rows(&data)
                    .into_iter()
                    .map(|(label, value)| format!("{label}: {value}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                self.toast_info("Copied to clipboard");
                return iced::clipboard::write(text).discard();
            }
            AboutModalMessage::OpenKofi => {
                let url = "https://ko-fi.com/foogsnokkvi";
                if let Err(e) = std::process::Command::new("xdg-open").arg(url).spawn() {
                    tracing::warn!("Failed to open Ko-fi URL: {}", e);
                    self.toast_warn(format!("Could not open browser: {e}"));
                }
            }
        }
        Task::none()
    }
}
