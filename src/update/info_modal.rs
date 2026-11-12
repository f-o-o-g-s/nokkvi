//! Info modal handler — open/close, copy, URL/folder open, row interaction.

use iced::Task;
use nokkvi_data::types::info_modal::InfoModalItem;

use crate::{Nokkvi, app_message::Message, widgets::info_modal::InfoModalMessage};

impl Nokkvi {
    /// `item` with what this session fetched about it filled in: an album's
    /// notes and page, where the album row it was built from had none.
    /// Navidrome fills the row only once a client has asked `getAlbumInfo2`,
    /// and the row on screen only at its next reload.
    fn with_fetched_album_info(&self, mut item: InfoModalItem) -> InfoModalItem {
        let fetched = match &item {
            InfoModalItem::Album { id, .. } => self.album_info.cache.peek(id),
            InfoModalItem::Song { .. }
            | InfoModalItem::Artist { .. }
            | InfoModalItem::Playlist { .. } => None,
        };
        if let Some(fetched) = fetched {
            item.fill_album_info(fetched);
        }
        item
    }

    /// The info for `album_id` just landed: Get Info opened on that album
    /// before the answer was back gains its notes and page. An answer that
    /// fills nothing leaves the modal, and the rows the user expanded,
    /// alone. That is judged by the fill, never by comparing the rows' text:
    /// "Last Played: 5 minutes ago" reads differently a minute later.
    pub(crate) fn refresh_open_album_info(&mut self, album_id: &str) {
        let Some(item) = &self.info_modal.item else {
            return;
        };
        if !matches!(item, InfoModalItem::Album { id, .. } if id == album_id) {
            return;
        }
        let Some(fetched) = self.album_info.cache.peek(&album_id.to_string()) else {
            return;
        };
        let mut filled = item.clone();
        if filled.fill_album_info(fetched) {
            self.info_modal.open(filled);
        }
    }

    /// Handle info modal messages (open, close, copy, URL/folder launch, editor actions).
    pub(crate) fn handle_info_modal(&mut self, msg: InfoModalMessage) -> Task<Message> {
        match msg {
            InfoModalMessage::Open(item) => {
                let item = self.with_fetched_album_info(*item);
                self.info_modal.open(item);
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
