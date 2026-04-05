//! Genre data loading and component message handlers

use iced::Task;
use nokkvi_data::backend::genres::GenreUIViewData;
use tracing::{debug, error, info};

use crate::{
    Nokkvi, View,
    app_message::{ArtworkMessage, CollageTarget, Message},
    views::{self, GenresAction, GenresMessage, HasCommonAction},
};

impl Nokkvi {
    pub(crate) fn handle_load_genres(&mut self) -> Task<Message> {
        debug!(" LoadGenres message received, loading genres...");
        let view_str =
            views::GenresPage::sort_mode_to_api_string(self.genres_page.common.current_sort_mode);
        let sort_ascending = self.genres_page.common.sort_ascending;
        let search_query_clone = self.genres_page.common.search_query.clone();

        // Mark buffer as loading to prevent duplicate fetches
        self.library.genres.set_loading(true);

        self.shell_task(
            move |shell| async move {
                let service = match shell.genres_api().await {
                    Ok(s) => s,
                    Err(e) => return (Err(e.to_string()), 0),
                };

                let sort_order_str = if sort_ascending { "ASC" } else { "DESC" };
                match service
                    .load_genres(
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
                    Ok((genres, total_count)) => {
                        let ui_genres: Vec<GenreUIViewData> =
                            genres.into_iter().map(GenreUIViewData::from).collect();
                        (Ok(ui_genres), total_count as usize)
                    }
                    Err(e) => (Err(e.to_string()), 0),
                }
            },
            |(result, total_count)| {
                Message::Genres(views::GenresMessage::GenresLoaded(result, total_count))
            },
        )
    }

    pub(crate) fn handle_genres_loaded(
        &mut self,
        result: Result<Vec<GenreUIViewData>, String>,
        total_count: usize,
    ) -> Task<Message> {
        self.library.counts.genres = total_count;
        match result {
            Ok(new_genres) => {
                info!(
                    " Loaded {} genres (total: {})",
                    new_genres.len(),
                    total_count
                );
                self.library.genres.set_first_page(new_genres, total_count);
                self.genres_page.common.slot_list.viewport_offset = 0;

                let mut tasks: Vec<Task<Message>> = Vec::new();

                // NOTE: Don't re-focus search field here - text_input maintains its own focus state.
                // Re-focusing here causes issues when users press Escape (widget unfocuses but we'd re-focus).

                // Start batch artwork prefetch for all genres
                tasks.push(Task::done(Message::Artwork(
                    ArtworkMessage::StartCollagePrefetch(CollageTarget::Genre),
                )));

                // Also trigger collage load for initially centered genre (index 0)
                if !self.library.genres.is_empty() {
                    tasks.push(Task::done(Message::Genres(
                        views::GenresMessage::SlotListSetOffset(
                            0,
                            iced::keyboard::Modifiers::default(),
                        ),
                    )));
                }

                // If CenterOnPlaying triggered this reload, re-dispatch.
                if self.pending_center_on_playing {
                    self.pending_center_on_playing = false;
                    tasks.push(Task::done(Message::Hotkey(
                        crate::app_message::HotkeyMessage::CenterOnPlaying,
                    )));
                }

                if !tasks.is_empty() {
                    return Task::batch(tasks);
                }
            }
            Err(e) => {
                error!("Error loading genres: {}", e);
                self.library.genres.set_loading(false);
                self.toast_error(format!("Failed to load genres: {e}"));
            }
        }
        Task::none()
    }

    pub(crate) fn handle_genres(&mut self, msg: views::GenresMessage) -> Task<Message> {
        self.play_view_sfx(
            matches!(
                msg,
                GenresMessage::SlotListNavigateUp | GenresMessage::SlotListNavigateDown
            ),
            matches!(
                msg,
                GenresMessage::CollapseExpansion | GenresMessage::ExpandCenter
            ),
        );
        let (cmd, action) =
            self.genres_page
                .update(msg, self.library.genres.len(), &self.library.genres);

        // Handle common actions (SearchChanged, SortModeChanged, SortOrderChanged)
        if let Some(task) = self.handle_common_view_action(
            action.as_common(),
            Message::LoadGenres,
            "persist_genres_prefs",
            self.genres_page.common.current_sort_mode,
            self.genres_page.common.sort_ascending,
            |shell, vt, asc| async move { shell.settings().set_genres_prefs(vt, asc).await },
        ) {
            return task;
        }

        match action {
            GenresAction::PlayGenre(genre_name) => {
                if let Some(task) = self.guard_play_action() {
                    return task;
                }
                // Browsing panel: redirect play → add to queue
                if self.browsing_panel.is_some() {
                    if let Some(pos) = self.pending_queue_insert_position.take() {
                        let label = format!("Inserted '{genre_name}' at position {}", pos + 1);
                        let name = genre_name.clone();
                        return self.shell_fire_and_forget_task(
                            move |shell| async move {
                                shell.insert_genre_at_position(&name, pos).await
                            },
                            label,
                            "insert genre to queue",
                        );
                    }
                    let label = format!("Added '{genre_name}' to queue");
                    return self.shell_fire_and_forget_task(
                        move |shell| async move { shell.add_genre_to_queue(&genre_name).await },
                        label,
                        "add genre to queue",
                    );
                }
                // AppendAndPlay: append genre songs to queue and start playing
                use nokkvi_data::types::player_settings::EnterBehavior;
                if self.enter_behavior == EnterBehavior::AppendAndPlay {
                    self.active_playlist_info = None;
                    self.persist_active_playlist_info();
                    let name = genre_name.clone();
                    return self.shell_fire_and_forget_task(
                        move |shell| async move { shell.add_genre_and_play(&name).await },
                        format!("Playing '{genre_name}'"),
                        "append genre and play",
                    );
                }
                // PlayAll / PlaySingle: replace queue with genre
                return self.shell_action_task(
                    move |shell| async move { shell.play_genre(&genre_name).await },
                    Message::SwitchView(View::Queue),
                    "play genre",
                );
            }
            GenresAction::AddBatchToQueue(payload) => {
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
            GenresAction::PlayAlbum(album_id) => {
                if let Some(task) = self.guard_play_action() {
                    return task;
                }
                // Browsing panel: redirect play → add to queue
                if self.browsing_panel.is_some() {
                    let name = self
                        .genres_page
                        .expansion
                        .children
                        .iter()
                        .find(|a| a.id == album_id)
                        .map_or_else(|| "album".to_string(), |a| a.name.clone());
                    return self.shell_fire_and_forget_task(
                        move |shell| async move { shell.add_album_to_queue(&album_id).await },
                        format!("Added '{name}' to queue"),
                        "add album to queue from genre",
                    );
                }
                return self.shell_action_task(
                    move |shell| async move { shell.play_album(&album_id).await },
                    Message::SwitchView(View::Queue),
                    "play album from genre",
                );
            }

            GenresAction::ExpandGenre(genre_name, genre_id) => {
                // Load albums for the genre and send them back to the view
                let name = genre_name.clone();
                let gid = genre_id.clone();
                return self.shell_task(
                    move |shell| async move {
                        let genres_service = shell.genres_api().await?;
                        let albums: Vec<nokkvi_data::types::album::Album> =
                            genres_service.load_genre_albums_full(&name).await?;

                        // Convert Album -> AlbumUIViewData
                        let albums_vm = shell.albums().clone();
                        let (url, cred) = albums_vm.get_server_config().await;
                        let ui_albums: Vec<nokkvi_data::backend::albums::AlbumUIViewData> = albums
                            .iter()
                            .map(|album| {
                                nokkvi_data::backend::albums::AlbumUIViewData::from_album(
                                    album, &url, &cred,
                                )
                            })
                            .collect();
                        Ok((gid, ui_albums))
                    },
                    move |result: Result<
                        (String, Vec<nokkvi_data::backend::albums::AlbumUIViewData>),
                        anyhow::Error,
                    >| {
                        match result {
                            Ok((genre_id, albums)) => {
                                Message::Genres(GenresMessage::AlbumsLoaded(genre_id, albums))
                            }
                            Err(e) => {
                                tracing::error!(" Failed to load genre albums: {}", e);
                                Message::NoOp
                            }
                        }
                    },
                );
            }
            GenresAction::ExpandAlbum(album_id) => {
                // Load tracks for the expanded album and send them back to the view
                let id = album_id.clone();
                return self.shell_task(
                    move |shell| async move {
                        let albums_vm = shell.albums().clone();
                        let songs = albums_vm.load_album_songs(&id).await?;
                        let ui_songs: Vec<nokkvi_data::backend::songs::SongUIViewData> = songs
                            .into_iter()
                            .map(nokkvi_data::backend::songs::SongUIViewData::from)
                            .collect();
                        Ok((album_id, ui_songs))
                    },
                    move |result: Result<
                        (String, Vec<nokkvi_data::backend::songs::SongUIViewData>),
                        anyhow::Error,
                    >| match result {
                        Ok((aid, songs)) => {
                            Message::Genres(GenresMessage::TracksLoaded(aid, songs))
                        }
                        Err(e) => {
                            tracing::error!(" Failed to load album tracks for genre: {}", e);
                            Message::NoOp
                        }
                    },
                );
            }
            GenresAction::PlayTrack(song_id) => {
                if let Some(task) = self.guard_play_action() {
                    return task;
                }
                // Find the song in sub_expansion children to build a single-song queue
                if let Some(song) = self
                    .genres_page
                    .sub_expansion
                    .children
                    .iter()
                    .find(|s| s.id == song_id)
                    .cloned()
                {
                    let song_data: nokkvi_data::types::song::Song = song.into();
                    self.active_playlist_info = None;
                    self.persist_active_playlist_info();
                    return self.shell_action_task(
                        move |shell| async move { shell.play_songs(vec![song_data], 0).await },
                        Message::SwitchView(View::Queue),
                        "play track from genre expansion",
                    );
                }
            }
            GenresAction::LoadArtwork(genre_index_str) => {
                // Load artwork for all visible slot list slots using collage artwork service
                use crate::services::collage_artwork::{self, CollageArtworkContext};

                if let Ok(_center_index) = genre_index_str.parse::<usize>()
                    && let Some(shell) = &self.app_service
                {
                    let total = self.library.genres.len();
                    if total == 0 {
                        return Task::none();
                    }

                    let ctx = CollageArtworkContext {
                        slot_list: &self.genres_page.common.slot_list,
                        disk_cache: self.artwork.genre_disk_cache.as_ref(),
                        pending_ids: &self.artwork.genre.pending,
                        memory_artwork: &self.artwork.genre.mini,
                        memory_collage: &self.artwork.genre.collage,
                    };

                    let (pending_inserts, cache_inserts, tasks) =
                        collage_artwork::load_visible_artwork(
                            &self.library.genres,
                            &ctx,
                            300,
                            shell.auth().clone(),
                            |a, b, c, d| {
                                Message::Artwork(ArtworkMessage::LoadCollage(
                                    CollageTarget::Genre,
                                    a,
                                    b,
                                    c,
                                    d,
                                ))
                            },
                        );

                    // Insert disk-cached items and mark all as pending
                    for (id, handle) in cache_inserts {
                        self.artwork.genre.mini.insert(id, handle);
                    }
                    for id in pending_inserts {
                        self.artwork.genre.pending.insert(id);
                    }

                    if !tasks.is_empty() {
                        return Task::batch(tasks);
                    }
                }
            }
            GenresAction::PreloadArtwork(_viewport_offset) => {
                // Preload artwork for visible genres around viewport using collage artwork service
                use crate::services::collage_artwork::{self, CollageArtworkContext};

                let total = self.library.genres.len();
                if total == 0 {
                    return Task::none();
                }

                if let Some(shell) = &self.app_service {
                    let ctx = CollageArtworkContext {
                        slot_list: &self.genres_page.common.slot_list,
                        disk_cache: self.artwork.genre_disk_cache.as_ref(),
                        pending_ids: &self.artwork.genre.pending,
                        memory_artwork: &self.artwork.genre.mini,
                        memory_collage: &self.artwork.genre.collage,
                    };

                    let (pending_inserts, task) = collage_artwork::preload_artwork(
                        &self.library.genres,
                        &ctx,
                        shell.auth().clone(),
                        |ids, url, cred| {
                            Message::Artwork(ArtworkMessage::CollageBatchReady(
                                CollageTarget::Genre,
                                ids,
                                url,
                                cred,
                            ))
                        },
                    );

                    // Mark items as pending
                    for id in pending_inserts {
                        self.artwork.genre.pending.insert(id);
                    }

                    if let Some(task) = task {
                        return task;
                    }
                }
            }
            GenresAction::ToggleStar(item_id, item_type, star) => {
                let optimistic_msg = Self::starred_revert_message(item_id.clone(), item_type, star);
                return Task::batch(vec![
                    Task::done(optimistic_msg),
                    self.star_item_task(item_id, item_type, star),
                ]);
            }
            GenresAction::AddBatchToPlaylist(payload) => {
                return self.handle_add_batch_to_playlist(payload);
            }
            GenresAction::PlayNextBatch(payload) => {
                if self.modes.random {
                    self.toast_warn("Shuffle is on — next tracks will be random, not these");
                }
                return self.shell_fire_and_forget_task(
                    move |shell| async move { shell.play_next_batch(payload).await },
                    "Added batch to play next".to_string(),
                    "play next batch",
                );
            }
            GenresAction::FindSimilar(id, label) => {
                return Task::done(Message::FindSimilar { id, label });
            }
            _ => {} // None + already-handled common actions
        }

        cmd.map(Message::Genres)
    }
}
