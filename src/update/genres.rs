//! Genre data loading and component message handlers

use iced::Task;
use nokkvi_data::backend::genres::GenreUIViewData;
use tracing::debug;

use crate::{
    Nokkvi, View,
    app_message::{ArtworkMessage, CollageTarget, Message},
    update::GenresTarget,
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
                    Err(e) => (Err(format!("{e:#}")), 0),
                }
            },
            |(result, total_count)| {
                Message::GenresLoader(crate::app_message::GenresLoaderMessage::Loaded(
                    result,
                    total_count,
                ))
            },
        )
    }

    /// Kick off a single-genre collage fetch through the shared
    /// `LoadCollage` pipeline (which writes both the row mini and the 3×3
    /// artwork-column tiles into `artwork.genre`).
    ///
    /// Skipped if the collage is already cached or a fetch is in flight.
    /// Marks the genre `pending` synchronously so the caller — typically
    /// `ExpandGenre` — can rely on the same de-dup gate the viewport-based
    /// `LoadArtwork` uses; otherwise rapid `FocusAndExpand` clicks could
    /// stack duplicate requests.
    ///
    /// Used by `ExpandGenre` because `FocusAndExpand` from a queue/songs
    /// link click does not flow through any scroll-driven `LoadArtwork`
    /// path — without this the artwork column stays blank until the user
    /// nudges the list.
    pub(crate) fn handle_load_genre_collage(&mut self, genre_id: String) -> Task<Message> {
        if self.artwork.genre.collage.contains_key(&genre_id)
            || self.artwork.genre.pending.contains(&genre_id)
        {
            return Task::none();
        }

        let cached_album_ids = self
            .library
            .genres
            .iter()
            .find(|g| g.id == genre_id)
            .map(|g| g.artwork_album_ids.clone())
            .unwrap_or_default();

        // Mark pending before the `app_service` check so the de-dup gate
        // engages even in tests where `app_service` is None — and so
        // observable state matches the eventual production fetch.
        self.artwork.genre.pending.insert(genre_id.clone());

        let Some(shell) = &self.app_service else {
            return Task::none();
        };
        let auth_vm = shell.auth().clone();

        Task::perform(
            async move {
                let server_url = auth_vm.get_server_url().await;
                let cred = auth_vm.get_subsonic_credential().await;
                (genre_id, server_url, cred, cached_album_ids)
            },
            |(id, url, cred, ids)| {
                Message::Artwork(ArtworkMessage::LoadCollage(
                    CollageTarget::Genre,
                    id,
                    url,
                    cred,
                    ids,
                ))
            },
        )
    }

    pub(crate) fn handle_genres_loaded(
        &mut self,
        result: Result<Vec<GenreUIViewData>, String>,
        total_count: usize,
    ) -> Task<Message> {
        self.handle_loaded_with::<GenresTarget>(result, total_count, false, None)
    }

    pub(crate) fn handle_genres(&mut self, msg: views::GenresMessage) -> Task<Message> {
        if let GenresMessage::SetOpenMenu(next) = msg {
            return Task::done(Message::SetOpenMenu(next));
        }
        if matches!(msg, GenresMessage::Roulette) {
            return Task::done(Message::Roulette(
                crate::app_message::RouletteMessage::Start(crate::View::Genres),
            ));
        }
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
        // Capture child album ids before consuming `msg` so we can fan out
        // mini-artwork fetches for the newly-loaded expansion children.
        let expansion_album_ids: Vec<(String, String)> = match &msg {
            GenresMessage::AlbumsLoaded(_, albums) => albums
                .iter()
                .map(|a| (a.id.clone(), a.artwork_url.clone()))
                .collect(),
            _ => Vec::new(),
        };
        // Capture the loaded genre id too — set_children inside the page
        // update clears `selected_offset`, and a find-chain pin needs to
        // re-pin the highlight on the target afterwards.
        let pin_after_albums = if let GenresMessage::AlbumsLoaded(ref id, _) = msg {
            Some(id.clone())
        } else {
            None
        };
        let (cmd, action) =
            self.genres_page
                .update(msg, self.library.genres.len(), &self.library.genres);

        if let Some(loaded_id) = pin_after_albums
            && matches!(
                self.pending_top_pin,
                Some(crate::state::PendingTopPin::Genre(ref pinned)) if pinned == &loaded_id
            )
            && let Some(idx) = self.library.genres.iter().position(|g| g.id == loaded_id)
        {
            let total = self
                .genres_page
                .expansion
                .flattened_len(&self.library.genres);
            self.genres_page.common.slot_list.pin_selected(idx, total);
            self.pending_top_pin = None;
        }

        // User-driven changes supersede any in-flight find-and-expand chain.
        if matches!(
            action,
            GenresAction::SearchChanged(_)
                | GenresAction::SortModeChanged(_)
                | GenresAction::SortOrderChanged(_)
                | GenresAction::RefreshViewData
        ) {
            self.cancel_pending_expand();
        }

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
                    self.clear_active_playlist();
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
                return self.add_or_insert_batch_to_queue_task(payload);
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

                // FocusAndExpand (link-text click in queue/songs) skips the
                // scroll-driven `LoadArtwork` path, so the 3×3 collage
                // column would stay blank until the user nudged the list.
                // Mirror the Albums fix and kick the fetch from here.
                let collage_task = self.handle_load_genre_collage(genre_id);

                let albums_task = self.shell_task(
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

                return Task::batch([collage_task, albums_task]);
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
                        pending_ids: &self.artwork.genre.pending,
                        memory_artwork: &self.artwork.genre.mini,
                        memory_collage: &self.artwork.genre.collage,
                    };

                    let (pending_inserts, cache_inserts, tasks) =
                        collage_artwork::load_visible_artwork(
                            &self.library.genres,
                            &ctx,
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
            GenresAction::ToggleStar(item_id, kind, star) => {
                return self.toggle_star_with_revert_task(item_id, kind, star);
            }
            GenresAction::AddBatchToPlaylist(payload) => {
                return self.handle_add_batch_to_playlist(payload);
            }
            GenresAction::PlayNextBatch(payload) => {
                return self.play_next_batch_task(payload);
            }
            GenresAction::FindSimilar(id, label) => {
                return Task::done(Message::FindSimilar { id, label });
            }
            GenresAction::ShowInfo(item) => {
                return self.update(Message::InfoModal(
                    crate::widgets::info_modal::InfoModalMessage::Open(item),
                ));
            }
            GenresAction::ShowAlbumInFolder(album_id) => {
                return self.show_album_in_folder_task(album_id);
            }
            GenresAction::ShowSongInFolder(path) => {
                return self.handle_show_in_folder(path);
            }
            GenresAction::ColumnVisibilityChanged(col, value) => {
                return self.persist_column_visibility(col, value);
            }
            _ => {} // None + already-handled common actions
        }

        let cmd_task = cmd.map(Message::Genres);
        if expansion_album_ids.is_empty() {
            return cmd_task;
        }
        let Some(shell) = &self.app_service else {
            return cmd_task;
        };
        let cached: std::collections::HashSet<&String> =
            self.artwork.album_art.iter().map(|(k, _)| k).collect();
        let prefetch = super::components::expansion_album_artwork_tasks(
            &cached,
            shell.albums().clone(),
            expansion_album_ids,
        );
        if prefetch.is_empty() {
            cmd_task
        } else {
            let mut tasks = vec![cmd_task];
            tasks.extend(prefetch);
            Task::batch(tasks)
        }
    }

    /// Routes `Message::GenresLoader(...)` arrivals to the existing
    /// `handle_genres_loaded` handler. Genres is single-shot (not paged),
    /// so there's only one variant — but keeping the dispatcher's match
    /// shape mirrors the paged domains' dispatchers and lets Phase 2 follow
    /// the same template.
    pub(crate) fn dispatch_genres_loader(
        &mut self,
        msg: crate::app_message::GenresLoaderMessage,
    ) -> Task<Message> {
        use crate::app_message::GenresLoaderMessage;
        match msg {
            GenresLoaderMessage::Loaded(result, total_count) => {
                self.handle_genres_loaded(result, total_count)
            }
        }
    }
}
