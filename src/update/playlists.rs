//! Playlist data loading and component message handlers

use iced::Task;
use nokkvi_data::backend::playlists::PlaylistUIViewData;
use tracing::{debug, info};

use crate::{
    Nokkvi, View,
    app_message::{ArtworkMessage, CollageTarget, Message},
    update::PlaylistsTarget,
    views::{self, HasCommonAction, PlaylistsAction, PlaylistsMessage},
};

impl Nokkvi {
    pub(crate) fn handle_load_playlists(&mut self) -> Task<Message> {
        debug!(" LoadPlaylists message received, loading playlists...");
        let view_str = views::PlaylistsPage::sort_mode_to_api_string(
            self.playlists_page.common.current_sort_mode,
        );
        let sort_ascending = self.playlists_page.common.sort_ascending;
        let search_query_clone = self.playlists_page.common.search_query.clone();

        // Mark buffer as loading to prevent duplicate fetches
        self.library.playlists.set_loading(true);

        self.shell_task(
            move |shell| async move {
                let service = match shell.playlists_api().await {
                    Ok(s) => s,
                    Err(e) => return (Err(e.to_string()), 0),
                };

                let sort_order_str = if sort_ascending { "ASC" } else { "DESC" };
                match service
                    .load_playlists(
                        view_str,
                        sort_order_str,
                        if search_query_clone.is_empty() {
                            None
                        } else {
                            Some(search_query_clone.as_str())
                        },
                    )
                    .await
                {
                    Ok((playlists, total_count)) => {
                        let ui_playlists: Vec<PlaylistUIViewData> = playlists
                            .into_iter()
                            .map(PlaylistUIViewData::from)
                            .collect();
                        (Ok(ui_playlists), total_count as usize)
                    }
                    Err(e) => (Err(format!("{e:#}")), 0),
                }
            },
            |(result, total_count)| {
                Message::PlaylistsLoader(crate::app_message::PlaylistsLoaderMessage::Loaded(
                    result,
                    total_count,
                ))
            },
        )
    }

    pub(crate) fn handle_playlists_loaded(
        &mut self,
        result: Result<Vec<PlaylistUIViewData>, String>,
        total_count: usize,
    ) -> Task<Message> {
        self.handle_loaded_with::<PlaylistsTarget>(result, total_count, false, None)
    }

    pub(crate) fn handle_playlists(&mut self, msg: views::PlaylistsMessage) -> Task<Message> {
        if let PlaylistsMessage::SetOpenMenu(next) = msg {
            return Task::done(Message::SetOpenMenu(next));
        }
        if matches!(msg, PlaylistsMessage::Roulette) {
            return Task::done(Message::Roulette(
                crate::app_message::RouletteMessage::Start(crate::View::Playlists),
            ));
        }
        self.play_view_sfx(
            matches!(
                msg,
                PlaylistsMessage::SlotListNavigateUp | PlaylistsMessage::SlotListNavigateDown
            ),
            matches!(
                msg,
                PlaylistsMessage::CollapseExpansion | PlaylistsMessage::ExpandCenter
            ),
        );
        let (cmd, action) =
            self.playlists_page
                .update(msg, self.library.playlists.len(), &self.library.playlists);

        // Handle common actions (SearchChanged, SortModeChanged, SortOrderChanged)
        if let Some(task) = self.handle_common_view_action(
            action.as_common(),
            Message::LoadPlaylists,
            "persist_playlists_prefs",
            self.playlists_page.common.current_sort_mode,
            self.playlists_page.common.sort_ascending,
            |shell, vt, asc| async move { shell.settings().set_playlists_prefs(vt, asc).await },
        ) {
            return task;
        }

        match action {
            views::PlaylistsAction::PlayPlaylist(playlist_id) => {
                if let Some(task) = self.guard_play_action() {
                    return task;
                }
                // Browsing panel: redirect play → add to queue
                if self.browsing_panel.is_some() {
                    let name = self
                        .library
                        .playlists
                        .iter()
                        .find(|p| p.id == playlist_id)
                        .map_or_else(|| "playlist".to_string(), |p| p.name.clone());
                    return self.shell_fire_and_forget_task(
                        move |shell| async move { shell.add_playlist_to_queue(&playlist_id).await },
                        format!("Added '{name}' to queue"),
                        "add playlist to queue",
                    );
                }
                // AppendAndPlay: append playlist songs to queue and start playing
                use nokkvi_data::types::player_settings::EnterBehavior;
                if self.enter_behavior == EnterBehavior::AppendAndPlay {
                    let name = self
                        .library
                        .playlists
                        .iter()
                        .find(|p| p.id == playlist_id)
                        .map_or_else(|| "playlist".to_string(), |p| p.name.clone());
                    self.clear_active_playlist();
                    return self.shell_fire_and_forget_task(
                        move |shell| async move { shell.add_playlist_and_play(&playlist_id).await },
                        format!("Playing '{name}'"),
                        "append playlist and play",
                    );
                }
                // PlayAll / PlaySingle: replace queue with playlist
                // Set the active playlist info for the queue header bar
                self.active_playlist_info = self
                    .library
                    .playlists
                    .iter()
                    .find(|p| p.id == playlist_id)
                    .map(|p| crate::state::ActivePlaylistContext {
                        id: p.id.clone(),
                        name: p.name.clone(),
                        comment: p.comment.clone(),
                    });
                self.persist_active_playlist_info();
                return self.shell_action_task(
                    move |shell| async move { shell.play_playlist(&playlist_id).await },
                    Message::SwitchView(View::Queue),
                    "play playlist",
                );
            }
            views::PlaylistsAction::AddBatchToQueue(payload) => {
                let len = payload.items.len();
                debug!(" Adding batch of {} items to queue", len);
                if let Some(pos) = self.pending_queue_insert_position.take() {
                    return self.shell_fire_and_forget_task(
                        move |shell| async move {
                            shell.insert_batch_at_position(payload, pos).await
                        },
                        format!("Inserted {} items at position {}", len, pos + 1),
                        "insert batch to queue",
                    );
                }
                return self.shell_fire_and_forget_task(
                    move |shell| async move { shell.add_batch_to_queue(payload).await },
                    format!("Added {len} items to queue"),
                    "add batch to queue",
                );
            }
            views::PlaylistsAction::ExpandPlaylist(playlist_id) => {
                // Load tracks for the playlist and send them back to the view
                let id = playlist_id.clone();
                return self.shell_task(
                    move |shell| async move {
                        let playlists_service = shell.playlists_api().await?;
                        playlists_service.load_playlist_songs(&id).await
                    },
                    move |result| match result {
                        Ok(songs) => {
                            let tracks: Vec<nokkvi_data::backend::songs::SongUIViewData> =
                                songs.into_iter().map(|s| s.into()).collect();
                            Message::Playlists(PlaylistsMessage::TracksLoaded(
                                playlist_id.clone(),
                                tracks,
                            ))
                        }
                        Err(e) => {
                            tracing::error!(" Failed to load playlist tracks: {}", e);
                            Message::NoOp
                        }
                    },
                );
            }
            views::PlaylistsAction::PlayPlaylistFromTrack(playlist_id, track_idx) => {
                // Set the active playlist info for the queue header bar
                self.active_playlist_info = self
                    .library
                    .playlists
                    .iter()
                    .find(|p| p.id == playlist_id)
                    .map(|p| crate::state::ActivePlaylistContext {
                        id: p.id.clone(),
                        name: p.name.clone(),
                        comment: p.comment.clone(),
                    });
                self.persist_active_playlist_info();
                return self.shell_action_task(
                    move |shell| async move {
                        shell
                            .play_playlist_from_track(&playlist_id, track_idx)
                            .await
                    },
                    Message::SwitchView(View::Queue),
                    "play playlist from track",
                );
            }

            views::PlaylistsAction::LoadArtwork(playlist_index_str) => {
                // Load artwork for all visible slot list slots using collage artwork service
                use crate::services::collage_artwork::{self, CollageArtworkContext};

                if let Ok(_center_index) = playlist_index_str.parse::<usize>()
                    && let Some(shell) = &self.app_service
                {
                    let total = self.library.playlists.len();
                    if total == 0 {
                        return Task::none();
                    }

                    let ctx = CollageArtworkContext {
                        slot_list: &self.playlists_page.common.slot_list,
                        pending_ids: &self.artwork.playlist.pending,
                        memory_artwork: &self.artwork.playlist.mini,
                        memory_collage: &self.artwork.playlist.collage,
                    };

                    let (pending_inserts, cache_inserts, tasks) =
                        collage_artwork::load_visible_artwork(
                            &self.library.playlists,
                            &ctx,
                            shell.auth().clone(),
                            |a, b, c, d| {
                                Message::Artwork(ArtworkMessage::LoadCollage(
                                    CollageTarget::Playlist,
                                    a,
                                    b,
                                    c,
                                    d,
                                ))
                            },
                        );

                    // Insert disk-cached items and mark all as pending
                    for (id, handle) in cache_inserts {
                        self.artwork.playlist.mini.insert(id, handle);
                    }
                    for id in pending_inserts {
                        self.artwork.playlist.pending.insert(id);
                    }

                    if !tasks.is_empty() {
                        return Task::batch(tasks);
                    }
                }
            }
            views::PlaylistsAction::PreloadArtwork(_viewport_offset) => {
                // Preload artwork for visible playlists around viewport
                use crate::services::collage_artwork::{self, CollageArtworkContext};

                let total = self.library.playlists.len();
                if total == 0 {
                    return Task::none();
                }

                if let Some(shell) = &self.app_service {
                    let ctx = CollageArtworkContext {
                        slot_list: &self.playlists_page.common.slot_list,
                        pending_ids: &self.artwork.playlist.pending,
                        memory_artwork: &self.artwork.playlist.mini,
                        memory_collage: &self.artwork.playlist.collage,
                    };

                    let (pending_inserts, task) = collage_artwork::preload_artwork(
                        &self.library.playlists,
                        &ctx,
                        shell.auth().clone(),
                        |ids, url, cred| {
                            Message::Artwork(ArtworkMessage::CollageBatchReady(
                                CollageTarget::Playlist,
                                ids,
                                url,
                                cred,
                            ))
                        },
                    );

                    // Mark items as pending
                    for id in pending_inserts {
                        self.artwork.playlist.pending.insert(id);
                    }

                    if let Some(task) = task {
                        return task;
                    }
                }
            }
            PlaylistsAction::ToggleStar(item_id, kind, star) => {
                let optimistic_msg = Self::starred_revert_message(item_id.clone(), kind, star);
                return Task::batch(vec![
                    Task::done(optimistic_msg),
                    self.star_item_task(item_id, kind, star),
                ]);
            }
            PlaylistsAction::PlayNextBatch(payload) => {
                if self.modes.random {
                    self.toast_warn("Shuffle is on — next tracks will be random, not these");
                }
                return self.shell_fire_and_forget_task(
                    move |shell| async move { shell.play_next_batch(payload).await },
                    "Added batch to play next".to_string(),
                    "play next batch",
                );
            }
            PlaylistsAction::DeletePlaylist(playlist_id) => {
                let name = self
                    .library
                    .playlists
                    .iter()
                    .find(|p| p.id == playlist_id)
                    .map_or_else(|| "playlist".to_string(), |p| p.name.clone());
                self.text_input_dialog
                    .open_delete_confirmation(playlist_id, name);
            }
            PlaylistsAction::RenamePlaylist(playlist_id) => {
                let current_name = self
                    .library
                    .playlists
                    .iter()
                    .find(|p| p.id == playlist_id)
                    .map_or_else(String::new, |p| p.name.clone());
                self.text_input_dialog.open(
                    "Rename Playlist",
                    current_name,
                    "Playlist name...",
                    crate::widgets::text_input_dialog::TextInputDialogAction::RenamePlaylist(
                        playlist_id,
                    ),
                );
            }
            PlaylistsAction::EditPlaylist(
                playlist_id,
                playlist_name,
                playlist_comment,
                playlist_public,
            ) => {
                return Task::done(Message::EnterPlaylistEditMode {
                    playlist_id,
                    playlist_name,
                    playlist_comment,
                    playlist_public,
                });
            }
            PlaylistsAction::ShowInfo(item) => {
                return self.update(Message::InfoModal(
                    crate::widgets::info_modal::InfoModalMessage::Open(item),
                ));
            }
            PlaylistsAction::SetAsDefaultPlaylist(playlist_id, playlist_name) => {
                info!(
                    " Setting default playlist: '{}' ({})",
                    playlist_name, playlist_id
                );
                self.default_playlist_id = Some(playlist_id.clone());
                self.default_playlist_name = playlist_name.clone();
                self.settings_page.config_dirty = true;
                self.toast_success(format!("Default playlist set to '{playlist_name}'"));
                self.shell_spawn("persist_default_playlist", move |shell| async move {
                    shell
                        .settings()
                        .set_default_playlist(Some(playlist_id), playlist_name)
                        .await
                });
            }
            views::PlaylistsAction::NavigateAndFilter(view, filter) => {
                return Task::done(Message::NavigateAndFilter(view, filter));
            }
            views::PlaylistsAction::OpenDefaultPlaylistPicker => {
                return Task::done(Message::DefaultPlaylistPicker(
                    crate::widgets::default_playlist_picker::DefaultPlaylistPickerMessage::Open,
                ));
            }
            views::PlaylistsAction::OpenCreatePlaylistDialog => {
                // Refuse if already in split-view edit mode — creating a new
                // playlist drops the user into edit mode for it, which would
                // collide with the in-progress edit.
                if self.playlist_edit.is_some() {
                    self.toast_warn("Finish or discard the current playlist edit first");
                    return Task::none();
                }
                self.text_input_dialog.open_create_playlist();
                return Task::none();
            }
            views::PlaylistsAction::ColumnVisibilityChanged(col, value) => {
                return self.persist_playlists_column_visibility(col, value);
            }
            _ => {} // None + already-handled common actions
        }

        cmd.map(Message::Playlists)
    }

    /// Persist the user's playlists column visibility toggle to config.toml +
    /// redb via `AppService::settings()`. The page's in-memory state was
    /// already mutated in `PlaylistsPage::update`.
    pub(crate) fn persist_playlists_column_visibility(
        &self,
        col: views::PlaylistsColumn,
        value: bool,
    ) -> Task<Message> {
        match col {
            views::PlaylistsColumn::Index => {
                self.shell_spawn("persist_playlists_show_index", move |shell| async move {
                    shell.settings().set_playlists_show_index(value).await
                });
            }
            views::PlaylistsColumn::Thumbnail => {
                self.shell_spawn(
                    "persist_playlists_show_thumbnail",
                    move |shell| async move {
                        shell.settings().set_playlists_show_thumbnail(value).await
                    },
                );
            }
            views::PlaylistsColumn::SongCount => {
                self.shell_spawn(
                    "persist_playlists_show_songcount",
                    move |shell| async move {
                        shell.settings().set_playlists_show_songcount(value).await
                    },
                );
            }
            views::PlaylistsColumn::Duration => {
                self.shell_spawn("persist_playlists_show_duration", move |shell| async move {
                    shell.settings().set_playlists_show_duration(value).await
                });
            }
            views::PlaylistsColumn::UpdatedAt => {
                self.shell_spawn(
                    "persist_playlists_show_updatedat",
                    move |shell| async move {
                        shell.settings().set_playlists_show_updatedat(value).await
                    },
                );
            }
            views::PlaylistsColumn::Select => {
                self.shell_spawn("persist_playlists_show_select", move |shell| async move {
                    shell.settings().set_playlists_show_select(value).await
                });
            }
        }
        Task::none()
    }

    /// Routes `Message::PlaylistsLoader(...)` arrivals to the existing
    /// `handle_playlists_loaded` handler. Playlists is single-shot (not
    /// paged), so there's only one variant — same shape as Genres.
    pub(crate) fn dispatch_playlists_loader(
        &mut self,
        msg: crate::app_message::PlaylistsLoaderMessage,
    ) -> Task<Message> {
        use crate::app_message::PlaylistsLoaderMessage;
        match msg {
            PlaylistsLoaderMessage::Loaded(result, total_count) => {
                self.handle_playlists_loaded(result, total_count)
            }
        }
    }
}
