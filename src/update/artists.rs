//! Artist data loading and component message handlers

use iced::{Task, widget::image};
use nokkvi_data::{backend::artists::ArtistUIViewData, types::ItemKind};
use tracing::debug;

use crate::{
    Nokkvi, View,
    app_message::{ArtworkMessage, Message},
    update::{ArtistsTarget, components::PaginatedFetch},
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

    /// Shared paginated fetch for Artists. Used by both the initial load
    /// (`handle_load_artists`, offset 0) and follow-up page loads
    /// (`handle_artists_load_page`, offset N). Preserves the rating-sort
    /// carve-out: when the user picks "Rating" sort, the API can't sort
    /// for us, so we sort client-side after each page completes.
    ///
    /// `force = true` skips the scroll-edge `needs_fetch` gate. Used by the
    /// artist-find-and-expand chain, which leaves viewport at 0 while
    /// paging through the library.
    fn load_artists_internal<M>(&mut self, offset: usize, force: bool, msg_ctor: M) -> Task<Message>
    where
        M: FnOnce((Result<Vec<ArtistUIViewData>, String>, usize)) -> Message + Send + 'static,
    {
        let page_size = self.library_page_size.to_usize();
        // Phase 5A defensive gate — see load_albums_internal for rationale.
        if !force
            && offset > 0
            && self
                .library
                .artists
                .needs_fetch(
                    self.artists_page.common.slot_list.viewport_offset,
                    page_size,
                )
                .is_none()
        {
            return Task::none();
        }
        let params = PaginatedFetch::from_common(
            &self.artists_page.common,
            views::ArtistsPage::sort_mode_to_api_string,
            offset,
            page_size,
        );
        let is_rating_sort =
            self.artists_page.common.current_sort_mode == widgets::view_header::SortMode::Rating;
        let album_artists_only = self.show_album_artists_only;

        debug!(
            " LoadArtists: offset={}, page_size={}, view={}, sort={}, search={:?}, album_artists_only={}",
            params.offset,
            params.page_size,
            params.view_str,
            params.sort_order,
            params.search_query,
            album_artists_only,
        );

        self.library.artists.set_loading(true);

        self.shell_task(
            move |shell| async move {
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
            },
            msg_ctor,
        )
    }

    pub(crate) fn handle_load_artists(
        &mut self,
        background: bool,
        anchor_id: Option<String>,
    ) -> Task<Message> {
        self.load_artists_internal(0, false, move |(result, total_count)| {
            Message::ArtistsLoader(crate::app_message::ArtistsLoaderMessage::Loaded {
                result,
                total_count,
                background,
                anchor_id: anchor_id.clone(),
            })
        })
    }

    /// Load a subsequent page of artists (triggered by scroll near edge of loaded data)
    pub(crate) fn handle_artists_load_page(&mut self, offset: usize) -> Task<Message> {
        self.load_artists_internal(offset, false, |(result, total_count)| {
            Message::ArtistsLoader(crate::app_message::ArtistsLoaderMessage::PageLoaded(
                result,
                total_count,
            ))
        })
    }

    /// Force-load an artists page regardless of the scroll-edge gate. Used
    /// by `try_resolve_pending_expand_artist` to walk the full library.
    pub(crate) fn force_load_artists_page(&mut self, offset: usize) -> Task<Message> {
        self.load_artists_internal(offset, true, |(result, total_count)| {
            Message::ArtistsLoader(crate::app_message::ArtistsLoaderMessage::PageLoaded(
                result,
                total_count,
            ))
        })
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
                // Browsing panel: redirect play → add to queue
                if self.browsing_panel.is_some() {
                    if let Some(pos) = self.pending_queue_insert_position.take() {
                        return self.insert_entity_to_queue_at_position_task(
                            &self.library.artists,
                            &artist_id_str,
                            "artist",
                            pos,
                            |a| a.id.clone(),
                            |a| a.name.clone(),
                            |shell, id, position| async move {
                                shell.insert_artist_at_position(&id, position).await
                            },
                        );
                    }
                    return self.add_entity_to_queue_task(
                        &self.library.artists,
                        &artist_id_str,
                        "artist",
                        |a| a.id.clone(),
                        |a| a.name.clone(),
                        |shell, id| async move { shell.add_artist_to_queue(&id).await },
                    );
                }
                // AppendAndPlay: append artist songs to queue and start playing
                use nokkvi_data::types::player_settings::EnterBehavior;
                if self.enter_behavior == EnterBehavior::AppendAndPlay
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
                    Message::SwitchView(View::Queue),
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
                return Task::done(Message::FindSimilar { id, label });
            }
            ArtistsAction::TopSongs(artist_name, label) => {
                return Task::done(Message::FindTopSongs { artist_name, label });
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
                let mut batch: Vec<Task<Message>> = vec![
                    cmd.map(Message::Artists),
                    self.prefetch_artist_mini_artwork_tasks(),
                ];

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

                let page_size = self.library_page_size.to_usize();
                if !self.library.artists.is_empty()
                    && let Some((offset, _)) = self.library.artists.needs_fetch(
                        self.artists_page.common.slot_list.viewport_offset,
                        page_size,
                    )
                {
                    batch.push(self.handle_artists_load_page(offset));
                }

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
