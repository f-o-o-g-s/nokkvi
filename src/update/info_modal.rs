//! Info modal handler — open/close, copy, URL/folder open, row interaction.

use iced::Task;

use crate::{Nokkvi, app_message::Message, widgets::info_modal::InfoModalMessage};

impl Nokkvi {
    /// Handle info modal messages (open, close, copy, URL/folder launch, editor actions).
    pub(crate) fn handle_info_modal(&mut self, msg: InfoModalMessage) -> Task<Message> {
        match msg {
            InfoModalMessage::Open(item) => {
                self.info_modal.open(*item);
            }
            InfoModalMessage::Close => {
                self.info_modal.close();
            }
            InfoModalMessage::CopyAll => {
                if let Some(item) = &self.info_modal.item {
                    let text: String = item
                        .properties()
                        .iter()
                        .map(|(label, value)| format!("{label}\t{value}"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    self.toast_info("Copied to clipboard");
                    return iced::clipboard::write(text).discard();
                }
            }
            InfoModalMessage::RowHovered(idx) => {
                self.info_modal.hovered_row = idx;
            }
            InfoModalMessage::CopyRow(idx) => {
                if let Some((label, value)) = self.info_modal.cached_properties.get(idx) {
                    let text = format!("{label}: {value}");
                    return iced::clipboard::write(text).discard();
                }
            }
            InfoModalMessage::ToggleRowExpanded(idx) => {
                self.info_modal.toggle_row(idx);
            }
            InfoModalMessage::OpenUrl(url) => {
                if let Err(e) = std::process::Command::new("xdg-open").arg(&url).spawn() {
                    tracing::warn!("Failed to open URL '{}': {}", url, e);
                }
            }
            InfoModalMessage::OpenFolder(path) => {
                // Resolve the full local path by prepending the user's configured prefix
                let full_path = if self.settings.local_music_path.is_empty() {
                    self.toast_warn(
                        "Set a Local Music Path in Settings → Application to open files in your file manager.",
                    );
                    return Task::none();
                } else {
                    // Trim any trailing slash from prefix before joining
                    let prefix = self.settings.local_music_path.trim_end_matches('/');
                    format!("{prefix}/{path}")
                };

                if !std::path::Path::new(&full_path).exists() {
                    self.toast_warn(format!(
                        "Path not found: {full_path}\nCheck your Local Music Path in Settings."
                    ));
                } else if let Err(e) = std::process::Command::new("xdg-open")
                    .arg(&full_path)
                    .spawn()
                {
                    tracing::warn!("Failed to open folder '{}': {}", full_path, e);
                    self.toast_warn(format!("Could not open file manager: {e}"));
                }
            }
            InfoModalMessage::FetchAndOpenAlbumFolder(album_id) => {
                return self.show_album_in_folder_task(album_id);
            }
            InfoModalMessage::EditorAction(idx, action) => {
                use iced::widget::text_editor;
                // Read-only filter: allow navigation/selection,
                // silently discard mutations to keep text intact.
                let is_mutation = matches!(action, text_editor::Action::Edit(_));
                if !is_mutation && let Some(editor) = self.info_modal.value_editors.get_mut(idx) {
                    editor.perform(action);
                }
            }
        }
        Task::none()
    }
}
