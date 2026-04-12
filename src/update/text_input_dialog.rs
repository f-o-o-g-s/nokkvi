//! Text input dialog handler — playlist operations and general setting edits.

use iced::Task;

use crate::{
    Nokkvi,
    app_message::{Message, PlaylistMutation},
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
            TextInputDialogMessage::SecondaryValueChanged(val) => {
                if self.text_input_dialog.secondary_value.is_some() {
                    self.text_input_dialog.secondary_value = Some(val);
                }
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
                    Message::PlaylistMutated(PlaylistMutation::Renamed(value)),
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
                self.shell_task(
                    move |shell| async move {
                        let service = shell.playlists_api().await?;
                        let playlist_id = service.create_playlist(&name, &song_ids).await?;
                        Ok(playlist_id)
                    },
                    move |result: Result<String, anyhow::Error>| match result {
                        Ok(playlist_id) => Message::PlaylistMutated(PlaylistMutation::Created(
                            value,
                            Some(playlist_id),
                        )),
                        Err(e) => {
                            tracing::error!(" Failed to create playlist from queue: {e}");
                            Message::Toast(crate::app_message::ToastMessage::Push(
                                nokkvi_data::types::toast::Toast::new(
                                    format!("Failed to create playlist: {e}"),
                                    nokkvi_data::types::toast::ToastLevel::Error,
                                ),
                            ))
                        }
                    },
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
                let id_for_msg = playlist_id.clone();
                self.shell_action_task(
                    move |shell| async move {
                        let service = shell.playlists_api().await?;
                        service
                            .replace_playlist_tracks(&playlist_id, &song_ids)
                            .await
                    },
                    Message::PlaylistMutated(PlaylistMutation::Overwritten(
                        playlist_name,
                        Some(id_for_msg),
                    )),
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
                    Message::PlaylistMutated(PlaylistMutation::Deleted(name)),
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
                self.shell_task(
                    move |shell| async move {
                        let service = shell.playlists_api().await?;
                        service.create_playlist(&name, &song_ids).await
                    },
                    move |result: Result<String, anyhow::Error>| match result {
                        Ok(_playlist_id) => {
                            Message::PlaylistMutated(PlaylistMutation::Created(value, None))
                        }
                        Err(e) => {
                            tracing::error!(" Failed to create playlist with songs: {e}");
                            Message::Toast(crate::app_message::ToastMessage::Push(
                                nokkvi_data::types::toast::Toast::new(
                                    format!("Failed to create playlist: {e}"),
                                    nokkvi_data::types::toast::ToastLevel::Error,
                                ),
                            ))
                        }
                    },
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
                    Message::PlaylistMutated(PlaylistMutation::Appended(playlist_name)),
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
                    Task::none()
                } else {
                    self.reload_visualizer_config();
                    self.toast_success("Visualizer settings reset to defaults");
                    Task::done(Message::Playback(crate::app_message::PlaybackMessage::Tick))
                }
            }
            Some(TextInputDialogAction::CreateRadioStation) => {
                let name = self.text_input_dialog.value.trim().to_string();
                let stream_url = self
                    .text_input_dialog
                    .secondary_value
                    .clone()
                    .unwrap_or_default()
                    .trim()
                    .to_string();

                if name.is_empty() || stream_url.is_empty() {
                    self.toast_warn("Name and Stream URL are required");
                    return Task::none();
                }

                self.text_input_dialog.close();
                self.radio_mutation_task(
                    move |service| async move {
                        service.create_radio_station(&name, &stream_url, None).await
                    },
                    "Radio station created",
                    "create radio station",
                )
            }
            Some(TextInputDialogAction::DeleteRadioStation(station_id, name)) => {
                self.text_input_dialog.close();
                let success_msg = format!("Deleted radio station '{name}'");
                self.radio_mutation_task(
                    move |service| async move { service.delete_radio_station(&station_id).await },
                    success_msg,
                    "delete radio station",
                )
            }
            Some(TextInputDialogAction::EditRadioStation(station_id)) => {
                let name = self.text_input_dialog.value.trim().to_string();
                let stream_url = self
                    .text_input_dialog
                    .secondary_value
                    .clone()
                    .unwrap_or_default()
                    .trim()
                    .to_string();

                if name.is_empty() || stream_url.is_empty() {
                    self.toast_warn("Name and Stream URL are required");
                    return Task::none();
                }

                self.text_input_dialog.close();
                self.radio_mutation_task(
                    move |service| async move {
                        service
                            .update_radio_station(&station_id, &name, &stream_url, None)
                            .await
                    },
                    "Radio station updated",
                    "update radio station",
                )
            }
            None => Task::none(),
        }
    }
}
