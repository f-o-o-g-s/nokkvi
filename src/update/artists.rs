//! Artist data loading and component message handlers

use iced::{Task, widget::image};
use nokkvi_data::{backend::artists::ArtistUIViewData, types::ItemKind};
use tracing::debug;

use crate::{
    Nokkvi, View,
    app_message::{ArtworkMessage, FindMessage, Message, NavigationMessage},
    update::ArtistsTarget,
    views::{self, ArtistsAction, ArtistsMessage, HasCommonAction},
    widgets,
};

impl Nokkvi {
    /// Sort UI artists in-place by rating (Some > None, then desc by value).
    /// Used by the rating-sort carve-out after every page load — the
    /// Subsonic API doesn't expose rating sort, so we emulate it client-side.
    pub(crate) fn artists_rating_sort(ui_artists: &mut [ArtistUIViewData]) {
        ui_artists.sort_by(|a, b| match (a.rating, b.rating) {
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (Some(ra), Some(rb)) => rb.cmp(&ra),
            (None, None) => std::cmp::Ordering::Equal,
        });
    }

    /// Per-entity fetch body for `Nokkvi::load_paged::<ArtistsTarget>`.
    ///
    /// Takes the rating-sort and album-artists-only flags as explicit args (the
    /// call sites snapshot them before invoking `load_paged` so each dispatch
    /// sees a consistent value); the shared invariant body (page_size,
    /// defensive gate, `PaginatedFetch`, debug log, `set_loading(true)`) lives
    /// in `loader_target.rs`. The `album_artists_only` plumbing stays on this
    /// fn rather than on `PaginatedFetch` so the Albums/Songs fetch paths keep
    /// their clean signature — Artists is the only entity with this carve-out.
    async fn fetch_artists_page(
        shell: nokkvi_data::backend::app_service::AppService,
        params: super::components::PaginatedFetch,
        is_rating_sort: bool,
        album_artists_only: bool,
    ) -> (Result<Vec<ArtistUIViewData>, String>, usize) {
        let artists_vm = shell.artists().clone();
        match artists_vm
            .load_raw_artists_page(
                Some(params.view_str),
                Some(params.sort_order),
                params.search_query.as_deref(),
                params.filter.as_ref(),
                album_artists_only,
                params.offset,
                params.page_size,
            )
            .await
        {
            Ok(artists) => {
                let mut ui_artists: Vec<ArtistUIViewData> =
                    artists.into_iter().map(ArtistUIViewData::from).collect();
                if is_rating_sort {
                    Nokkvi::artists_rating_sort(&mut ui_artists);
                }
                (Ok(ui_artists), artists_vm.get_total_count() as usize)
            }
            Err(e) => (Err(format!("{e:#}")), 0),
        }
    }

    pub(crate) fn handle_load_artists(
        &mut self,
        background: bool,
        anchor_id: Option<String>,
    ) -> Task<Message> {
        let is_rating_sort =
            self.artists_page.common.current_sort_mode == widgets::view_header::SortMode::Rating;
        let album_artists_only = self.settings.show_album_artists_only;
        debug!(album_artists_only, "LoadArtists per-call flags");
        self.load_paged::<ArtistsTarget, _, _, _>(
            0,
            false,
            move |(result, total_count)| {
                Message::ArtistsLoader(crate::app_message::ArtistsLoaderMessage::Loaded {
                    result,
                    total_count,
                    background,
                    anchor_id: anchor_id.clone(),
                })
            },
            move |shell, params| {
                Self::fetch_artists_page(shell, params, is_rating_sort, album_artists_only)
            },
        )
    }

    /// Load a subsequent page of artists (triggered by scroll near edge of loaded data)
    pub(crate) fn handle_artists_load_page(&mut self, offset: usize) -> Task<Message> {
        let is_rating_sort =
            self.artists_page.common.current_sort_mode == widgets::view_header::SortMode::Rating;
        let album_artists_only = self.settings.show_album_artists_only;
        self.load_paged::<ArtistsTarget, _, _, _>(
            offset,
            false,
            |(result, total_count)| {
                Message::ArtistsLoader(crate::app_message::ArtistsLoaderMessage::PageLoaded(
                    result,
                    total_count,
                ))
            },
            move |shell, params| {
                Self::fetch_artists_page(shell, params, is_rating_sort, album_artists_only)
            },
        )
    }

    /// Force-load an artists page regardless of the scroll-edge gate. Used
    /// by `try_resolve_pending_expand_artist` to walk the full library.
    pub(crate) fn force_load_artists_page(&mut self, offset: usize) -> Task<Message> {
        let is_rating_sort =
            self.artists_page.common.current_sort_mode == widgets::view_header::SortMode::Rating;
        let album_artists_only = self.settings.show_album_artists_only;
        self.load_paged::<ArtistsTarget, _, _, _>(
            offset,
            true,
            |(result, total_count)| {
                Message::ArtistsLoader(crate::app_message::ArtistsLoaderMessage::PageLoaded(
                    result,
                    total_count,
                ))
            },
            move |shell, params| {
                Self::fetch_artists_page(shell, params, is_rating_sort, album_artists_only)
            },
        )
    }

    /// Fetch the 500 px artist artwork plus its dominant color and stash both
    /// in `large_artwork` / `album_dominant_colors`. Skipped when already
    /// cached; the artist must be present in `library.artists` so we can
    /// resolve `image_url` (Navidrome may return an external poster URL).
    ///
    /// Shared by `LoadLargeArtwork` (settled-scroll / hotkey navigation) and
    /// the `ExpandArtist` action — `FocusAndExpand` from a queue/songs link
    /// click bypasses every scroll-driven trigger, so the expand path has to
    /// kick the fetch itself or the artwork column would stay blank until
    /// the user scrolled away and back.
    pub(crate) fn handle_load_artist_large_artwork(&mut self, artist_id: String) -> Task<Message> {
        if self.artwork.large_artwork.peek(&artist_id).is_some() {
            return Task::none();
        }

        let external_url = self
            .library
            .artists
            .iter()
            .find(|a| a.id == artist_id)
            .and_then(|a| a.image_url.clone());

        // Set the in-flight marker before the `app_service` check so it
        // matches the Albums helper's ordering — the marker is the
        // observable side-effect tests rely on.
        self.artwork.loading_large_artwork = Some(artist_id.clone());

        let Some(shell) = &self.app_service else {
            return Task::none();
        };
        let vm = shell.albums().clone();

        let id = artist_id;
        Task::perform(
            async move {
                let art_id = external_url.unwrap_or_else(|| format!("ar-{id}"));
                match vm.fetch_album_artwork(&art_id, Some(500), None).await {
                    Ok(bytes) if bytes.len() > 100 => {
                        let dominant = tokio::task::spawn_blocking({
                            let b = bytes.clone();
                            move || nokkvi_data::utils::dominant_color::extract_dominant_color(&b)
                        })
                        .await
                        .unwrap_or(None);
                        (id, Some(image::Handle::from_bytes(bytes)), dominant)
                    }
                    _ => (id, None, None),
                }
            },
            |(id, handle, color)| {
                if handle.is_none() && color.is_none() {
                    Message::NoOp
                } else {
                    Message::Artwork(ArtworkMessage::LargeArtistLoaded(
                        id,
                        handle,
                        color.map(|(r, g, b)| iced::Color::from_rgb8(r, g, b)),
                    ))
                }
            },
        )
    }

    pub(crate) fn handle_artists_page_loaded(
        &mut self,
        result: Result<Vec<ArtistUIViewData>, String>,
        total_count: usize,
    ) -> Task<Message> {
        self.handle_page_loaded_with::<ArtistsTarget>(result, total_count)
    }

    pub(crate) fn handle_artists_loaded(
        &mut self,
        result: Result<Vec<ArtistUIViewData>, String>,
        total_count: usize,
        background: bool,
        anchor_id: Option<String>,
    ) -> Task<Message> {
        self.handle_loaded_with::<ArtistsTarget>(result, total_count, background, anchor_id)
    }

    pub(crate) fn handle_artists(&mut self, msg: views::ArtistsMessage) -> Task<Message> {
        if let Some(task) = crate::update::dispatch_view_chrome(self, &msg, crate::View::Artists) {
            return task;
        }
        if let ArtistsMessage::OpenExternalUrl(url) = msg {
            if let Err(e) = std::process::Command::new("xdg-open").arg(&url).spawn() {
                tracing::warn!("Failed to open URL '{}': {}", url, e);
            }
            return Task::none();
        }
        // Capture child album ids before consuming `msg` so we can fan out
        // mini-artwork fetches for the newly-loaded expansion children.
        let expansion_album_ids: Vec<(String, String)> = match &msg {
            ArtistsMessage::AlbumsLoaded(_, albums) => albums
                .iter()
                .map(|a| (a.id.clone(), a.artwork_url.clone()))
                .collect(),
            _ => Vec::new(),
        };
        // Capture the loaded artist id too — the page's `update` runs
        // `set_children` (which clears `selected_offset`), and the find-
        // chain pin needs to re-pin the highlight on the target afterwards.
        let pin_after_albums = if let ArtistsMessage::AlbumsLoaded(ref id, _) = msg {
            Some(id.clone())
        } else {
            None
        };
        let (cmd, action) =
            self.artists_page
                .update(msg, self.library.artists.len(), &self.library.artists);

        if let Some(loaded_id) = pin_after_albums
            && matches!(
                self.pending_top_pin,
                Some(crate::state::PendingTopPin::Artist(ref pinned)) if pinned == &loaded_id
            )
            && let Some(idx) = self.library.artists.iter().position(|a| a.id == loaded_id)
        {
            let total = self
                .artists_page
                .expansion
                .flattened_len(&self.library.artists);
            self.artists_page.common.slot_list.pin_selected(idx, total);
            self.pending_top_pin = None;
        }

        // User-driven changes supersede any in-flight find-and-expand chain.
        if matches!(
            action,
            ArtistsAction::SearchChanged(_)
                | ArtistsAction::SortModeChanged(_)
                | ArtistsAction::SortOrderChanged(_)
                | ArtistsAction::RefreshViewData
        ) {
            self.cancel_pending_expand();
        }

        // Handle common actions (SearchChanged, SortModeChanged, SortOrderChanged)
        if let Some(task) = self.handle_common_view_action(
            action.as_common(),
            Message::LoadArtists,
            "persist_artists_prefs",
            self.artists_page.common.current_sort_mode,
            self.artists_page.common.sort_ascending,
            |shell, vt, asc| async move { shell.settings().set_artists_prefs(vt, asc).await },
        ) {
            return task;
        }

        match action {
            ArtistsAction::PlayArtist(artist_id_str) => {
                if let Some(task) = self.guard_play_action() {
                    return task;
                }
                self.enter_new_playback_context();
                // Browsing panel: redirect play → add to queue (insert at
                // drag-drop position when one is pending, else append).
                let id_ref = artist_id_str.as_str();
                if let Some(task) = self.redirect_play_to_queue_in_browsing_panel(
                    |app| {
                        app.add_entity_to_queue_task(
                            &app.library.artists,
                            id_ref,
                            "artist",
                            |a| a.id.clone(),
                            |a| a.name.clone(),
                            |shell, id| async move { shell.add_artist_to_queue(&id).await },
                        )
                    },
                    |app, pos| {
                        app.insert_entity_to_queue_at_position_task(
                            &app.library.artists,
                            id_ref,
                            "artist",
                            pos,
                            |a| a.id.clone(),
                            |a| a.name.clone(),
                            |shell, id, position| async move {
                                shell.insert_artist_at_position(&id, position).await
                            },
                        )
                    },
                ) {
                    return task;
                }
                // AppendAndPlay: append artist songs to queue and start playing
                use nokkvi_data::types::player_settings::EnterBehavior;
                if self.settings.enter_behavior == EnterBehavior::AppendAndPlay
                    && let Ok(index) = artist_id_str.parse::<usize>()
                    && let Some(artist) = self.library.artists.get(index)
                {
                    let id = artist.id.clone();
                    let name = artist.name.clone();
                    self.clear_active_playlist();
                    return self.shell_fire_and_forget_task(
                        move |shell| async move { shell.add_artist_and_play(&id).await },
                        format!("Playing '{name}'"),
                        "append artist and play",
                    );
                }
                // PlayAll / PlaySingle: replace queue with artist (PlaySingle = PlayAll for artists)
                return self.play_entity_task(
                    &self.library.artists,
                    &artist_id_str,
                    "artist",
                    |a| a.id.clone(),
                    |shell, id| async move { shell.play_artist(&id).await },
                );
            }
            ArtistsAction::AddBatchToQueue(payload) => {
                return self.add_or_insert_batch_to_queue_task(payload);
            }
            ArtistsAction::PlayAlbum(album_id) => {
                if let Some(task) = self.guard_play_action() {
                    return task;
                }
                self.enter_new_playback_context();
                // Browsing panel: redirect play → add to queue
                if self.browsing_panel.is_some() {
                    let name = self
                        .artists_page
                        .expansion
                        .children
                        .iter()
                        .find(|a| a.id == album_id)
                        .map_or_else(|| "album".to_string(), |a| a.name.clone());
                    return self.shell_fire_and_forget_task(
                        move |shell| async move { shell.add_album_to_queue(&album_id).await },
                        format!("Added '{name}' to queue"),
                        "add album to queue",
                    );
                }
                return self.shell_action_task(
                    move |shell| async move { shell.play_album(&album_id).await },
                    Message::Navigation(NavigationMessage::SwitchView(View::Queue)),
                    "play album",
                );
            }

            ArtistsAction::ExpandArtist(artist_id) => {
                // Load albums for the artist and send them back to the view
                let id = artist_id.clone();

                // FocusAndExpand (link-text click in queue/songs) skips the
                // scroll-driven `LoadLargeArtwork` path, so the artwork
                // column would stay blank until the user nudged the list.
                // Mirror the Albums fix and kick the fetch from here.
                let artwork_task = self.handle_load_artist_large_artwork(id.clone());

                let albums_task = self.shell_task(
                    move |shell| async move {
                        let artists_vm = shell.artists().clone();
                        let albums_vm = shell.albums().clone();
                        let albums = artists_vm.load_artist_albums(&id).await?;
                        let (url, cred) = albums_vm.get_server_config().await;
                        let ui_albums: Vec<nokkvi_data::backend::albums::AlbumUIViewData> = albums
                            .iter()
                            .map(|album| {
                                nokkvi_data::backend::albums::AlbumUIViewData::from_album(
                                    album, &url, &cred,
                                )
                            })
                            .collect();
                        Ok(ui_albums)
                    },
                    move |result: Result<
                        Vec<nokkvi_data::backend::albums::AlbumUIViewData>,
                        anyhow::Error,
                    >| {
                        match result {
                            Ok(albums) => Message::Artists(ArtistsMessage::AlbumsLoaded(
                                artist_id.clone(),
                                albums,
                            )),
                            Err(e) => {
                                if e.downcast_ref::<nokkvi_data::types::error::NokkviError>()
                                    .is_some_and(|err| {
                                        matches!(
                                            err,
                                            nokkvi_data::types::error::NokkviError::Unauthorized
                                        )
                                    })
                                {
                                    return Message::SessionExpired;
                                }
                                tracing::error!(" Failed to load artist albums: {}", e);
                                Message::Toast(crate::app_message::ToastMessage::Push(
                                    nokkvi_data::types::toast::Toast::new(
                                        format!("Failed to load artist albums: {e}"),
                                        nokkvi_data::types::toast::ToastLevel::Error,
                                    ),
                                ))
                            }
                        }
                    },
                );

                return Task::batch([artwork_task, albums_task]);
            }
            ArtistsAction::StarArtist(artist_id) => {
                return self.toggle_star_with_revert_task(artist_id, ItemKind::Artist, true);
            }
            ArtistsAction::UnstarArtist(artist_id) => {
                return self.toggle_star_with_revert_task(artist_id, ItemKind::Artist, false);
            }
            ArtistsAction::SetRating(item_id, kind, new_rating) => {
                let current = if matches!(kind, ItemKind::Artist) {
                    self.library
                        .artists
                        .iter()
                        .find(|a| a.id == item_id)
                        .and_then(|a| a.rating)
                        .unwrap_or(0)
                } else {
                    // Album within expanded artist
                    self.artists_page
                        .expansion
                        .children
                        .iter()
                        .find(|a| a.id == item_id)
                        .and_then(|a| a.rating)
                        .unwrap_or(0)
                };
                return self.set_item_rating_task(item_id, kind, new_rating, current);
            }
            ArtistsAction::ToggleStar(item_id, kind, star) => {
                return self.toggle_star_with_revert_task(item_id, kind, star);
            }
            ArtistsAction::LoadPage(offset) => {
                return self.handle_artists_load_page(offset);
            }
            ArtistsAction::AddBatchToPlaylist(payload) => {
                return self.handle_add_batch_to_playlist(payload);
            }
            ArtistsAction::PlayNextBatch(payload) => {
                return self.play_next_batch_task(payload);
            }
            ArtistsAction::ShowInfo(item) => {
                return self.update(Message::InfoModal(
                    crate::widgets::info_modal::InfoModalMessage::Open(item),
                ));
            }
            ArtistsAction::ShowAlbumInFolder(album_id) => {
                return self.show_album_in_folder_task(album_id);
            }
            ArtistsAction::ShowSongInFolder(path) => {
                return self.handle_show_in_folder(path);
            }
            ArtistsAction::FindSimilar(id, label) => {
                return Task::done(Message::Find(FindMessage::Similar { id, label }));
            }
            ArtistsAction::TopSongs(artist_name, label) => {
                return Task::done(Message::Find(FindMessage::TopSongs { artist_name, label }));
            }
            ArtistsAction::ColumnVisibilityChanged(col, value) => {
                return self.persist_column_visibility(col, value);
            }
            ArtistsAction::LoadLargeArtwork => {
                // Settled-scroll / hotkey navigation: refresh viewport mini
                // artwork, fetch 500px artwork + dominant color for the new
                // center artist, and chain a page-fetch if the viewport is
                // near the loaded edge. Mid-drag scrolling does NOT enter
                // here — `SlotListScrollSeek` returns `None` and lets the
                // 150 ms `SeekSettled` debounce synthesise a `SetOffset`
                // that lands in this arm exactly once per drag.
                let mut batch: Vec<Task<Message>> = vec![cmd.map(Message::Artists)];

                let total = self.library.artists.len();
                if total > 0
                    && let Some(center_idx) = self
                        .artists_page
                        .common
                        .slot_list
                        .get_center_item_index(total)
                    && let Some(artist) = self.library.artists.get(center_idx)
                {
                    let id = artist.id.clone();
                    batch.push(self.handle_load_artist_large_artwork(id));
                }

                // Prefetch viewport mini artwork + chain a page-load if
                // scrolling near the loaded edge.
                batch.extend(self.prefetch_and_maybe_load_next_page::<ArtistsTarget>(
                    Self::handle_artists_load_page,
                ));

                return Task::batch(batch);
            }
            _ => {} // None + already-handled common actions
        }

        let cmd_task = cmd.map(Message::Artists);
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

    /// Routes `Message::ArtistsLoader(...)` arrivals to the existing
    /// `handle_artists_loaded` / `handle_artists_page_loaded` handlers.
    /// Mirrors `dispatch_genres_loader` — the per-domain dispatcher pattern
    /// keeps the loader-result routing co-located with its handlers.
    pub(crate) fn dispatch_artists_loader(
        &mut self,
        msg: crate::app_message::ArtistsLoaderMessage,
    ) -> Task<Message> {
        use crate::app_message::ArtistsLoaderMessage;
        match msg {
            ArtistsLoaderMessage::Loaded {
                result,
                total_count,
                background,
                anchor_id,
            } => self.handle_artists_loaded(result, total_count, background, anchor_id),
            ArtistsLoaderMessage::PageLoaded(result, total_count) => {
                self.handle_artists_page_loaded(result, total_count)
            }
        }
    }
}
