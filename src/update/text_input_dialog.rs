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
                // Live duplicate-name warning on every playlist-CREATE flow
                // (warn, never block — a duplicate name is legal server-side).
                // Immediate like search; the shared helper trims + case-folds.
                if matches!(
                    self.text_input_dialog.action,
                    Some(
                        TextInputDialogAction::CreatePlaylistFromQueue
                            | TextInputDialogAction::CreatePlaylistWithSongs(_)
                    )
                ) {
                    let dup = nokkvi_data::services::api::playlists::duplicate_playlist_name(
                        &self.text_input_dialog.value,
                        self.library.playlists.iter().map(|p| p.name.as_str()),
                    );
                    self.text_input_dialog.note = dup.map(|i| {
                        format!(
                            "A playlist named \"{}\" already exists",
                            self.library.playlists[i].name
                        )
                    });
                }
                Task::none()
            }
            TextInputDialogMessage::SecondaryValueChanged(val) => {
                if self.text_input_dialog.secondary_value.is_some() {
                    self.text_input_dialog.secondary_value = Some(val);
                }
                Task::none()
            }
            TextInputDialogMessage::Cancel => {
                // The Trawl save dialog reopens the mix builder on cancel —
                // the user backed out of NAMING, not out of the mix (the
                // crate lives on root state either way).
                let reopen_trawl = matches!(
                    self.text_input_dialog.action,
                    Some(TextInputDialogAction::CreatePlaylistFromTrawl(_))
                );
                self.text_input_dialog.close();
                if reopen_trawl {
                    return self
                        .handle_trawl_modal(crate::widgets::trawl_modal::TrawlModalMessage::Open);
                }
                Task::none()
            }
            TextInputDialogMessage::PlaylistSelected(option) => {
                self.handle_playlist_selected(option)
            }
            TextInputDialogMessage::PublicToggled(value) => {
                self.text_input_dialog.public = value;
                Task::none()
            }
            TextInputDialogMessage::Submit => self.handle_text_input_submit(),
            TextInputDialogMessage::SubmitExtra => {
                // Swap the third button's action into the primary slot and
                // run the shared submit path (same validation + close).
                if let Some((_, action)) = self.text_input_dialog.extra_action.take() {
                    self.text_input_dialog.action = Some(action);
                }
                self.handle_text_input_submit()
            }
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
                // Look up the playlist's current visibility so rename round-trips
                // it unchanged. Falling back to `true` matches the default-public
                // policy when the cache hasn't loaded yet.
                //
                // Rename deliberately RE-SENDS `public` (the 403-probe behavior):
                // the rename path has no public-dirty concept, so always sending
                // the current visibility surfaces a non-owner edit as a 403 rather
                // than silently succeeding. The editor save path (N21) drops this
                // probe and sends `public` only when the user changed it.
                let current_public = self
                    .library
                    .playlists
                    .iter()
                    .find(|p| p.id == playlist_id)
                    .is_none_or(|p| p.public);
                self.text_input_dialog.close();
                let name = value.clone();
                self.shell_action_task(
                    move |shell| async move {
                        let service = shell.playlists_api().await?;
                        service
                            .update_playlist(&playlist_id, Some(&name), None, Some(current_public))
                            .await
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
                let public = self.text_input_dialog.public;
                self.text_input_dialog.close();
                let song_ids = self.queue_song_ids();
                let name = value.clone();
                self.shell_task(
                    move |shell| async move {
                        let service = shell.playlists_api().await?;
                        let playlist_id = service
                            .create_playlist(&name, "", &song_ids, public)
                            .await?;
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
                        PlaylistOption::NewPlaylist => None,
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
                let public = self.text_input_dialog.public;
                self.text_input_dialog.close();
                let name = value.clone();
                self.shell_task(
                    move |shell| async move {
                        let service = shell.playlists_api().await?;
                        service.create_playlist(&name, "", &song_ids, public).await
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
                        PlaylistOption::NewPlaylist => None,
                    })
                    .unwrap_or_default();
                self.text_input_dialog.close();
                let id_for_msg = playlist_id.clone();
                self.shell_action_task(
                    move |shell| async move {
                        let service = shell.playlists_api().await?;
                        service.add_songs_to_playlist(&playlist_id, &song_ids).await
                    },
                    Message::PlaylistMutated(PlaylistMutation::Appended {
                        name: playlist_name,
                        id: id_for_msg,
                    }),
                    "add songs to playlist",
                )
            }
            Some(TextInputDialogAction::WriteGeneralSetting { key }) => {
                let new_value = self.text_input_dialog.value.clone();
                self.text_input_dialog.close();
                // Update local state and persist for known general settings
                if key == "general.local_music_path" {
                    self.settings.local_music_path = new_value.clone();
                    tracing::info!(" [SETTINGS] Local music path set to: {new_value:?}");
                    // The dialog commits outside `handle_settings`, so the
                    // cached "Local Music Path" row needs an explicit
                    // refresh (the user is sitting on Settings here).
                    self.settings_page.config_dirty = true;
                    self.refresh_settings_entries_if_dirty();
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
                    // The reset wrote [visualizer] via write_atomic (an
                    // internal write, so the watcher suppresses it) —
                    // re-read JUST that section into the manager and apply
                    // via the slim visualizer path. A global
                    // SettingsConfigReloaded would re-apply every section
                    // and reload the library views, far outside this
                    // button's scope.
                    self.toast_success("Visualizer settings reset to defaults");
                    let reloaded = self.app_service.as_ref().map(|shell| {
                        let mgr_arc = shell.settings().settings_manager();
                        let mut mgr = mgr_arc.blocking_lock();
                        mgr.reload_visualizer_from_toml();
                        mgr.visualizer().clone()
                    });
                    match reloaded {
                        Some(visualizer) => self.apply_visualizer_settings(visualizer),
                        None => Task::none(),
                    }
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
            Some(TextInputDialogAction::WriteListenBrainzToken) => {
                let token = self.text_input_dialog.value.trim().to_string();
                self.text_input_dialog.close();
                self.shell_task(
                    move |shell| async move {
                        // Empty value clears the token (disconnect) → Ok(None).
                        if token.is_empty() {
                            shell
                                .set_listenbrainz_token("")
                                .map_err(|e| e.to_string())?;
                            return Ok(None);
                        }
                        // Validate FIRST; only persist a token that works, so a
                        // rejected token never lingers in redb looking "saved".
                        let name = shell
                            .validate_listenbrainz_token_to_name(token.clone())
                            .await?;
                        shell
                            .set_listenbrainz_token(&token)
                            .map_err(|e| e.to_string())?;
                        Ok(Some(name))
                    },
                    |result| {
                        Message::Scrobble(crate::app_message::ScrobbleMessage::RadioVerifyResult(
                            result,
                        ))
                    },
                )
            }
            Some(TextInputDialogAction::WriteLastfmCredentials) => {
                let api_key = self.text_input_dialog.value.trim().to_string();
                let api_secret = self
                    .text_input_dialog
                    .secondary_value
                    .clone()
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                self.text_input_dialog.close();
                if api_key.is_empty() || api_secret.is_empty() {
                    self.toast_warn("Both API key and secret are required");
                    return Task::none();
                }
                self.shell_task(
                    move |shell| async move {
                        shell
                            .set_lastfm_credentials(&api_key, &api_secret)
                            .map_err(|e| e.to_string())
                            // Empty string => "credentials saved" toast.
                            .map(|()| String::new())
                    },
                    |result| {
                        Message::Scrobble(crate::app_message::ScrobbleMessage::LastfmAuthResult(
                            result,
                        ))
                    },
                )
            }
            Some(TextInputDialogAction::CreatePlaylistFromTrawl(song_ids)) => {
                let name = self.text_input_dialog.value.trim().to_string();
                if name.is_empty() {
                    self.text_input_dialog.action =
                        Some(TextInputDialogAction::CreatePlaylistFromTrawl(song_ids));
                    self.toast_warn("Name cannot be empty");
                    return Task::none();
                }
                let public = self.text_input_dialog.public;
                self.text_input_dialog.close();
                let toast_name = name.clone();
                self.shell_task(
                    move |shell| async move {
                        let service = shell.playlists_api().await?;
                        service.create_playlist(&name, "", &song_ids, public).await
                    },
                    move |result: Result<String, anyhow::Error>| match result {
                        // `None` id on purpose: the id-carrying arm sets the
                        // queue's playlist-context header, and the queue is
                        // NOT this playlist (the mix was saved, not played).
                        Ok(_playlist_id) => {
                            Message::PlaylistMutated(PlaylistMutation::Created(toast_name, None))
                        }
                        Err(e) => {
                            tracing::error!(" Failed to save mix as playlist: {e}");
                            Message::Toast(crate::app_message::ToastMessage::Push(
                                nokkvi_data::types::toast::Toast::new(
                                    format!("Failed to save mix as playlist: {e}"),
                                    nokkvi_data::types::toast::ToastLevel::Error,
                                ),
                            ))
                        }
                    },
                )
            }
            Some(TextInputDialogAction::ImportNspCreate { comment, rules }) => {
                let name = self.text_input_dialog.value.trim().to_string();
                if name.is_empty() {
                    self.text_input_dialog.action =
                        Some(TextInputDialogAction::ImportNspCreate { comment, rules });
                    self.toast_warn("Name cannot be empty");
                    return Task::none();
                }
                let public = self.text_input_dialog.public;
                self.text_input_dialog.close();
                self.import_create_task(crate::update::nsp_import::NspImportPayload {
                    name,
                    comment,
                    public,
                    rules,
                })
            }
            Some(TextInputDialogAction::ImportNspUpdate {
                playlist_id,
                detach_sync,
                comment,
                public,
                rules,
            }) => {
                let name = self.text_input_dialog.value.trim().to_string();
                if name.is_empty() {
                    self.text_input_dialog.action = Some(TextInputDialogAction::ImportNspUpdate {
                        playlist_id,
                        detach_sync,
                        comment,
                        public,
                        rules,
                    });
                    self.toast_warn("Name cannot be empty");
                    return Task::none();
                }
                // `public`/`comment` are the target's own (rules-only update);
                // the hidden toggle's value is deliberately ignored here.
                self.text_input_dialog.close();
                let toast_name = name.clone();
                self.shell_action_task(
                    move |shell| async move {
                        let service = shell.playlists_api().await?;
                        service
                            .put_playlist_full(
                                &playlist_id,
                                &name,
                                &comment,
                                public,
                                Some(&rules),
                                detach_sync.then_some(false),
                            )
                            .await
                    },
                    Message::PlaylistMutated(PlaylistMutation::RulesSaved { name: toast_name }),
                    "import rules onto existing playlist",
                )
            }
            Some(TextInputDialogAction::CompleteLastfmAuth(token)) => {
                self.text_input_dialog.close();
                self.shell_task(
                    move |shell| async move {
                        shell
                            .lastfm_complete_auth(token)
                            .await
                            .map_err(|e| e.to_string())
                    },
                    |result| {
                        Message::Scrobble(crate::app_message::ScrobbleMessage::LastfmAuthResult(
                            result,
                        ))
                    },
                )
            }
            None => Task::none(),
        }
    }
}
