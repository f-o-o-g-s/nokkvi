//! Text input dialog handler — playlist operations and general setting edits.

use iced::Task;

use crate::{
    Nokkvi,
    app_message::Message,
    widgets::text_input_dialog::{PlaylistOption, TextInputDialogAction, TextInputDialogMessage},
};

impl Nokkvi {
    /// Handle text input dialog messages (playlist create/rename/delete/overwrite, settings edits).
    pub(crate) fn handle_text_input_dialog(
        &mut self,
        msg: TextInputDialogMessage,
    ) -> Task<Message> {
        match msg {
            TextInputDialogMessage::ValueChanged(val) => {
                self.text_input_dialog.value = val;
                Task::none()
            }
            TextInputDialogMessage::Cancel => {
                self.text_input_dialog.close();
                Task::none()
            }
            TextInputDialogMessage::PlaylistSelected(option) => {
                self.handle_playlist_selected(option)
            }
            TextInputDialogMessage::Submit => self.handle_text_input_submit(),
        }
    }

    /// Handle playlist option selection (switch between create/overwrite/append mode).
    fn handle_playlist_selected(&mut self, option: PlaylistOption) -> Task<Message> {
        // Switch between create/overwrite mode based on selection
        match &option {
            PlaylistOption::NewPlaylist => {
                // Preserve song IDs if in Add to Playlist mode
                self.text_input_dialog.action = match self.text_input_dialog.action.take() {
                    Some(
                        TextInputDialogAction::CreatePlaylistWithSongs(ids)
                        | TextInputDialogAction::AppendToPlaylist(_, ids),
                    ) => Some(TextInputDialogAction::CreatePlaylistWithSongs(ids)),
                    _ => Some(TextInputDialogAction::CreatePlaylistFromQueue),
                };
            }
            PlaylistOption::Existing { id, .. } => {
                // Preserve song IDs if in Add to Playlist mode
                self.text_input_dialog.action = match self.text_input_dialog.action.take() {
                    Some(
                        TextInputDialogAction::CreatePlaylistWithSongs(ids)
                        | TextInputDialogAction::AppendToPlaylist(_, ids),
                    ) => Some(TextInputDialogAction::AppendToPlaylist(id.clone(), ids)),
                    _ => Some(TextInputDialogAction::OverwritePlaylistFromQueue(
                        id.clone(),
                    )),
                };
            }
        }
        self.text_input_dialog.selected_playlist = Some(option);
        Task::none()
    }

    /// Handle text input dialog submission — dispatch based on the current action.
    fn handle_text_input_submit(&mut self) -> Task<Message> {
        let action = self.text_input_dialog.action.take();

        match action {
            Some(TextInputDialogAction::RenamePlaylist(playlist_id)) => {
                let value = self.text_input_dialog.value.trim().to_string();
                if value.is_empty() {
                    self.toast_warn("Name cannot be empty");
                    return Task::none();
                }
                self.text_input_dialog.close();
                let name = value.clone();
                self.shell_action_task(
                    move |shell| async move {
                        let service = shell.playlists_api().await?;
                        service.update_playlist(&playlist_id, &name, None).await
                    },
                    Message::PlaylistRenamed(value),
                    "rename playlist",
                )
            }
            Some(TextInputDialogAction::CreatePlaylistFromQueue) => {
                let value = self.text_input_dialog.value.trim().to_string();
                if value.is_empty() {
                    self.toast_warn("Name cannot be empty");
                    return Task::none();
                }
                self.text_input_dialog.close();
                let song_ids = self.queue_song_ids();
                let name = value.clone();
                self.shell_action_task(
                    move |shell| async move {
                        let service = shell.playlists_api().await?;
                        service.create_playlist(&name, &song_ids).await
                    },
                    Message::PlaylistCreated(value),
                    "create playlist from queue",
                )
            }
            Some(TextInputDialogAction::OverwritePlaylistFromQueue(playlist_id)) => {
                // Get the playlist name for the toast
                let playlist_name = self
                    .text_input_dialog
                    .selected_playlist
                    .as_ref()
                    .and_then(|opt| match opt {
                        PlaylistOption::Existing { name, .. } => Some(name.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                self.text_input_dialog.close();
                let song_ids = self.queue_song_ids();
                self.shell_action_task(
                    move |shell| async move {
                        let service = shell.playlists_api().await?;
                        service
                            .replace_playlist_tracks(&playlist_id, &song_ids)
                            .await
                    },
                    Message::PlaylistOverwritten(playlist_name),
                    "overwrite playlist from queue",
                )
            }
            Some(TextInputDialogAction::DeletePlaylist(playlist_id, name)) => {
                self.text_input_dialog.close();
                self.shell_action_task(
                    move |shell| async move {
                        let service = shell.playlists_api().await?;
                        service.delete_playlist(&playlist_id).await
                    },
                    Message::PlaylistDeleted(name),
                    "delete playlist",
                )
            }
            Some(TextInputDialogAction::CreatePlaylistWithSongs(song_ids)) => {
                let value = self.text_input_dialog.value.trim().to_string();
                if value.is_empty() {
                    self.toast_warn("Name cannot be empty");
                    return Task::none();
                }
                self.text_input_dialog.close();
                let name = value.clone();
                self.shell_action_task(
                    move |shell| async move {
                        let service = shell.playlists_api().await?;
                        service.create_playlist(&name, &song_ids).await
                    },
                    Message::PlaylistCreated(value),
                    "create playlist with songs",
                )
            }
            Some(TextInputDialogAction::AppendToPlaylist(playlist_id, song_ids)) => {
                let playlist_name = self
                    .text_input_dialog
                    .selected_playlist
                    .as_ref()
                    .and_then(|opt| match opt {
                        PlaylistOption::Existing { name, .. } => Some(name.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                self.text_input_dialog.close();
                self.shell_action_task(
                    move |shell| async move {
                        let service = shell.playlists_api().await?;
                        service.add_songs_to_playlist(&playlist_id, &song_ids).await
                    },
                    Message::PlaylistAppended(playlist_name),
                    "add songs to playlist",
                )
            }
            Some(TextInputDialogAction::WriteGeneralSetting { key }) => {
                let new_value = self.text_input_dialog.value.clone();
                self.text_input_dialog.close();
                // Update local state and persist for known general settings
                if key == "general.local_music_path" {
                    self.local_music_path = new_value.clone();
                    tracing::info!(" [SETTINGS] Local music path set to: {new_value:?}");
                    self.shell_fire_and_forget_task(
                        move |shell| async move {
                            shell.settings().set_local_music_path(new_value).await
                        },
                        "Local music path saved".to_string(),
                        "set local music path",
                    )
                } else {
                    tracing::warn!(" [SETTINGS] WriteGeneralSetting: unhandled key {key:?}");
                    Task::none()
                }
            }
            Some(TextInputDialogAction::ResetAllHotkeys) => {
                self.text_input_dialog.close();
                self.handle_settings_reset_all_hotkeys()
            }
            Some(TextInputDialogAction::ResetVisualizerSettings) => {
                self.text_input_dialog.close();
                if let Err(e) = crate::config_writer::reset_visualizer_defaults_preserving_colors()
                {
                    tracing::warn!(" [SETTINGS] Failed to reset visualizer settings: {e}");
                    self.toast_warn(format!("Failed to reset visualizer settings: {e}"));
                } else {
                    self.toast_success("Visualizer settings reset to defaults");
                }
                Task::none()
            }
            None => Task::none(),
        }
    }
}
