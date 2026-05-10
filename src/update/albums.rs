//! Album data loading and component message handlers

use std::collections::HashSet;

use iced::{Task, widget::image};
use nokkvi_data::{backend::albums::AlbumUIViewData, types::ItemKind};
use tracing::{debug, error, warn};

use super::components::{PaginatedFetch, prefetch_album_artwork_tasks};
use crate::{
    Nokkvi, View,
    app_message::{ArtworkMessage, Message},
    update::AlbumsTarget,
    views::{self, AlbumsAction, AlbumsMessage, HasCommonAction},
};

impl Nokkvi {
    /// Shared paginated fetch for Albums. Used by both the initial load
    /// (`handle_load_albums`, offset 0) and follow-up page loads
    /// (`handle_albums_load_page`, offset N). The caller supplies a
    /// message constructor so the result lands on the right
    /// `AlbumsMessage` variant.
    ///
    /// `force = true` skips the scroll-edge `needs_fetch` gate. The
    /// find-and-expand chain uses this so it can page through the entire
    /// library without first scrolling the viewport to the edge.
    fn load_albums_internal<M>(&mut self, offset: usize, force: bool, msg_ctor: M) -> Task<Message>
    where
        M: FnOnce((Result<Vec<AlbumUIViewData>, String>, usize)) -> Message + Send + 'static,
    {
        let page_size = self.library_page_size.to_usize();
        // Phase 5A defensive gate: page-load follow-ups (offset > 0) must
        // pass needs_fetch. Catches duplicate dispatches that race past
        // the upstream needs_fetch check at the action site. Initial
        // loads (offset 0) always proceed — sort/search changes need a
        // fresh page even if the old one is still in flight.
        if !force
            && offset > 0
            && self
                .library
                .albums
                .needs_fetch(self.albums_page.common.slot_list.viewport_offset, page_size)
                .is_none()
        {
            return Task::none();
        }
        let params = PaginatedFetch::from_common(
            &self.albums_page.common,
            views::AlbumsPage::sort_mode_to_api_string,
            offset,
            page_size,
        );
        debug!(
            " LoadAlbums: offset={}, page_size={}, view={}, sort={}, search={:?}",
            params.offset,
            params.page_size,
            params.view_str,
            params.sort_order,
            params.search_query,
        );

        self.library.albums.set_loading(true);

        self.shell_task(
            move |shell| async move {
                let albums_vm = shell.albums().clone();
                match albums_vm
                    .load_raw_albums_page(
                        Some(params.view_str),
                        Some(params.sort_order),
                        params.search_query.as_deref(),
                        params.filter.as_ref(),
                        params.offset,
                        params.page_size,
                    )
                    .await
                {
                    Ok(albums) => {
                        let (url, cred) = albums_vm.get_server_config().await;
                        let ui_albums: Vec<AlbumUIViewData> = albums
                            .iter()
                            .map(|album| AlbumUIViewData::from_album(album, &url, &cred))
                            .collect();
                        (Ok(ui_albums), albums_vm.get_total_count() as usize)
                    }
                    Err(e) => (Err(format!("{e:#}")), 0),
                }
            },
            msg_ctor,
        )
    }

    pub(crate) fn handle_load_albums(
        &mut self,
        background: bool,
        anchor_id: Option<String>,
    ) -> Task<Message> {
        self.load_albums_internal(0, false, move |(result, total_count)| {
            Message::AlbumsLoader(crate::app_message::AlbumsLoaderMessage::Loaded {
                result,
                total_count,
                background,
                anchor_id: anchor_id.clone(),
            })
        })
    }

    /// Load a subsequent page of albums (triggered by scroll near edge of loaded data)
    pub(crate) fn handle_albums_load_page(&mut self, offset: usize) -> Task<Message> {
        self.load_albums_internal(offset, false, |(result, total_count)| {
            Message::AlbumsLoader(crate::app_message::AlbumsLoaderMessage::PageLoaded(
                result,
                total_count,
            ))
        })
    }

    /// Force-load an albums page regardless of the scroll-edge `needs_fetch`
    /// gate. Used by the find-and-expand chain to page through the full
    /// library while the viewport stays at 0 (the user hasn't scrolled
    /// because they're waiting for the target to appear).
    pub(crate) fn force_load_albums_page(&mut self, offset: usize) -> Task<Message> {
        self.load_albums_internal(offset, true, |(result, total_count)| {
            Message::AlbumsLoader(crate::app_message::AlbumsLoaderMessage::PageLoaded(
                result,
                total_count,
            ))
        })
    }

    pub(crate) fn handle_albums_page_loaded(
        &mut self,
        result: Result<Vec<AlbumUIViewData>, String>,
        total_count: usize,
    ) -> Task<Message> {
        self.handle_page_loaded_with::<AlbumsTarget>(result, total_count)
    }

    pub(crate) fn handle_albums_loaded(
        &mut self,
        result: Result<Vec<AlbumUIViewData>, String>,
        total_count: usize,
        background: bool,
        anchor_id: Option<String>,
    ) -> Task<Message> {
        self.handle_loaded_with::<AlbumsTarget>(result, total_count, background, anchor_id)
    }

    pub(crate) fn handle_artwork_loaded(
        &mut self,
        id: String,
        handle: Option<image::Handle>,
    ) -> Task<Message> {
        if let Some(h) = handle {
            self.artwork.album_art.put(id, h);
            self.artwork.refresh_album_art_snapshot();
        } else {
            warn!(" Mini artwork failed to load for album: {}", id);
        }
        Task::none()
    }

    pub(crate) fn handle_load_large_artwork(&mut self, album_id: String) -> Task<Message> {
        // Skip fetching if already cached - makes back-navigation instant
        if self.artwork.large_artwork.peek(&album_id).is_some() {
            if let Some(&color) = self.artwork.album_dominant_colors.peek(&album_id) {
                return Task::done(Message::Artwork(ArtworkMessage::DominantColorCalculated(
                    album_id, color,
                )));
            }
            let handle = self.artwork.large_artwork.peek(&album_id).cloned();
            return Task::done(Message::Artwork(ArtworkMessage::LargeLoaded(
                album_id, handle,
            )));
        }

        self.artwork.loading_large_artwork = Some(album_id.clone());

        if let Some(shell) = &self.app_service {
            let albums_vm = shell.albums().clone();
            let artwork_size = self.artwork_resolution.to_size();
            // Resolve the art_id (and updated_at, when known) from the albums list
            // first — falls back to the bare album_id which `fetch_album_artwork`
            // will normalize with the `al-` prefix.
            let (art_id, updated_at) = match self.library.albums.iter().find(|a| a.id == album_id) {
                Some(album) => (album.id.clone(), album.updated_at.clone()),
                None => (album_id.clone(), None),
            };

            return Task::perform(
                async move {
                    let bytes = albums_vm
                        .fetch_album_artwork(&art_id, artwork_size, updated_at.as_deref())
                        .await
                        .ok();
                    (art_id, bytes.map(image::Handle::from_bytes))
                },
                |(id, handle)| Message::Artwork(ArtworkMessage::LargeLoaded(id, handle)),
            );
        }
        Task::none()
    }

    /// Force-refresh a specific album's artwork (user-initiated, with toasts).
    pub(crate) fn handle_refresh_album_artwork(&mut self, album_id: String) -> Task<Message> {
        self.refresh_album_artwork_inner(album_id, false)
    }

    /// Same as `handle_refresh_album_artwork` but suppresses progress/success
    /// toasts. Used by the SSE-driven invalidation path so background updates
    /// don't spam the user with notifications.
    pub(crate) fn handle_refresh_album_artwork_silent(
        &mut self,
        album_id: String,
    ) -> Task<Message> {
        self.refresh_album_artwork_inner(album_id, true)
    }

    fn refresh_album_artwork_inner(&mut self, album_id: String, silent: bool) -> Task<Message> {
        use tracing::info;

        info!(
            " [REFRESH] Refreshing artwork for album {} (silent={silent})",
            album_id
        );

        // Only evict large artwork so the panel shows a placeholder during refresh.
        // Do NOT evict from album_art — that would gray out every slot list row
        // sharing this album_id. The old mini thumbnail stays visible until
        // RefreshComplete replaces it atomically.
        self.artwork.large_artwork.pop(&album_id);
        self.artwork.refresh_large_artwork_snapshot();

        let Some(shell) = &self.app_service else {
            return Task::none();
        };
        let albums_vm = shell.albums().clone();
        let id = album_id.clone();
        let artwork_size = self.artwork_resolution.to_size();
        let updated_at = self
            .library
            .albums
            .iter()
            .find(|a| a.id == album_id)
            .and_then(|a| a.updated_at.clone());

        let refresh_task = Task::perform(
            async move {
                use nokkvi_data::utils::artwork_url::THUMBNAIL_SIZE;

                // No client cache to evict — go straight to the server. The
                // server's `Cache-Control: max-age=315360000` is irrelevant on
                // our side now; Navidrome's own ImageCacheSize keeps the
                // response fast.
                let thumb_bytes = albums_vm
                    .fetch_album_artwork(&id, Some(THUMBNAIL_SIZE), updated_at.as_deref())
                    .await
                    .ok();
                let large_bytes = albums_vm
                    .fetch_album_artwork(&id, artwork_size, updated_at.as_deref())
                    .await
                    .ok();

                // `Handle::from_bytes` derives a unique Id per call, busting Iced's
                // GPU texture cache so the new bytes upload (refresh path requires this).
                let thumb_handle = thumb_bytes.map(image::Handle::from_bytes);
                let large_handle = large_bytes.map(image::Handle::from_bytes);

                (id, thumb_handle, large_handle)
            },
            move |(id, thumb, large)| {
                Message::Artwork(ArtworkMessage::RefreshComplete(id, thumb, large, silent))
            },
        );

        if silent {
            refresh_task
        } else {
            let toast_task = Task::done(Message::Toast(crate::app_message::ToastMessage::Push(
                nokkvi_data::types::toast::Toast::new(
                    "Refreshing artwork…".to_string(),
                    nokkvi_data::types::toast::ToastLevel::Info,
                ),
            )));
            Task::batch([toast_task, refresh_task])
        }
    }

    /// Handle the result of an artwork refresh — cache both mini and large atomically.
    pub(crate) fn handle_refresh_complete(
        &mut self,
        album_id: String,
        thumb: Option<image::Handle>,
        large: Option<image::Handle>,
        silent: bool,
    ) -> Task<Message> {
        if thumb.is_none() && large.is_none() {
            if !silent {
                self.toast_warn("No artwork found on server for this album");
            }
            return Task::none();
        }
        if let Some(h) = thumb {
            self.artwork.album_art.put(album_id.clone(), h);
            self.artwork.refresh_album_art_snapshot();
        }
        if let Some(h) = large {
            self.artwork.large_artwork.put(album_id, h);
            self.artwork.refresh_large_artwork_snapshot();
        }
        if !silent {
            self.toast_success("Artwork refreshed");
        }
        Task::none()
    }

    pub(crate) fn handle_large_artwork_loaded(
        &mut self,
        id: String,
        handle: Option<image::Handle>,
    ) -> Task<Message> {
        let mut fetch_color_task = Task::none();

        // Always cache artwork that arrives (even if user navigated away)
        // This fixes the bug where rapid navigation discarded completed loads
        if let Some(h) = handle {
            self.artwork.large_artwork.put(id.clone(), h);
            self.artwork.refresh_large_artwork_snapshot();

            if let Some(shell) = &self.app_service {
                let albums_vm = shell.albums().clone();
                let art_id = id.clone();
                let artwork_size = self.artwork_resolution.to_size();

                fetch_color_task = Task::perform(
                    async move {
                        // Re-fetch through the cached client (warm cache hit, no network)
                        // and run dominant-color extraction on the bytes. Cheaper than
                        // the previous path-read because cacache holds the bytes already.
                        match albums_vm
                            .fetch_album_artwork(&art_id, artwork_size, None)
                            .await
                        {
                            Ok(bytes) => {
                                let dominant = tokio::task::spawn_blocking(move || {
                                    nokkvi_data::utils::dominant_color::extract_dominant_color(
                                        &bytes,
                                    )
                                })
                                .await
                                .unwrap_or(None);
                                dominant.map(|(r, g, b)| (art_id, iced::Color::from_rgb8(r, g, b)))
                            }
                            Err(_) => None,
                        }
                    },
                    |result| {
                        if let Some((id, c)) = result {
                            Message::Artwork(ArtworkMessage::DominantColorCalculated(id, c))
                        } else {
                            Message::NoOp
                        }
                    },
                );
            }
        }
        // Clear loading_large_artwork if this was the most recent request
        if self.artwork.loading_large_artwork.as_ref() == Some(&id) {
            self.artwork.loading_large_artwork = None;
        }
        fetch_color_task
    }

    pub(crate) fn handle_albums(&mut self, msg: views::AlbumsMessage) -> Task<Message> {
        // Bubble menu open/close requests to the root before the page sees
        // them — page state has nothing to do with overlay-menu coordination.
        if let AlbumsMessage::SetOpenMenu(next) = msg {
            return Task::done(Message::SetOpenMenu(next));
        }
        if matches!(msg, AlbumsMessage::Roulette) {
            return Task::done(Message::Roulette(
                crate::app_message::RouletteMessage::Start(crate::View::Albums),
            ));
        }
        self.play_view_sfx(
            matches!(
                msg,
                AlbumsMessage::SlotListNavigateUp | AlbumsMessage::SlotListNavigateDown
            ),
            matches!(
                msg,
                AlbumsMessage::CollapseExpansion | AlbumsMessage::ExpandCenter
            ),
        );
        // The page's `update` calls `set_children` for `TracksLoaded`, which
        // wipes `selected_offset` to clamp the viewport into the post-
        // expansion flat list. For find-chain navigations we want the
        // highlight to stay on the target, so pre-extract the album_id and
        // re-pin afterwards. Cloned because `msg` moves into the page update.
        let pin_after_tracks = if let AlbumsMessage::TracksLoaded(ref id, _) = msg {
            Some(id.clone())
        } else {
            None
        };
        let (cmd, action) =
            self.albums_page
                .update(msg, self.library.albums.len(), &self.library.albums);

        if let Some(loaded_id) = pin_after_tracks
            && matches!(
                self.pending_top_pin,
                Some(crate::state::PendingTopPin::Album(ref pinned)) if pinned == &loaded_id
            )
            && let Some(idx) = self.library.albums.iter().position(|a| a.id == loaded_id)
        {
            let total = self
                .albums_page
                .expansion
                .flattened_len(&self.library.albums);
            self.albums_page.common.slot_list.pin_selected(idx, total);
            self.pending_top_pin = None;
        }

        // User-driven changes (search edit, sort, refresh) supersede any
        // in-flight find-and-expand chain. FocusAndExpand dispatched by the
        // chain itself produces ExpandAlbum, which is not in this list.
        if matches!(
            action,
            AlbumsAction::SearchChanged(_)
                | AlbumsAction::SortModeChanged(_)
                | AlbumsAction::SortOrderChanged(_)
                | AlbumsAction::RefreshViewData
        ) {
            self.cancel_pending_expand();
        }

        // Handle common actions (SearchChanged, SortModeChanged, SortOrderChanged)
        if let Some(task) = self.handle_common_view_action(
            action.as_common(),
            Message::LoadAlbums,
            "persist_albums_prefs",
            self.albums_page.common.current_sort_mode,
            self.albums_page.common.sort_ascending,
            |shell, vt, asc| async move { shell.settings().set_albums_prefs(vt, asc).await },
        ) {
            return task;
        }

        match action {
            AlbumsAction::PlayAlbum(album_id_str) => {
                if let Some(task) = self.guard_play_action() {
                    return task;
                }
                // Browsing panel: redirect play → add to queue
                if self.browsing_panel.is_some() {
                    // Check if this was triggered by a cross-pane drag drop with a target position
                    if let Some(pos) = self.pending_queue_insert_position.take() {
                        return self.insert_entity_to_queue_at_position_task(
                            &self.library.albums,
                            &album_id_str,
                            "album",
                            pos,
                            |a| a.id.clone(),
                            |a| a.name.clone(),
                            |shell, id, position| async move {
                                shell.insert_album_at_position(&id, position).await
                            },
                        );
                    }
                    return self.add_entity_to_queue_task(
                        &self.library.albums,
                        &album_id_str,
                        "album",
                        |a| a.id.clone(),
                        |a| a.name.clone(),
                        |shell, id| async move { shell.add_album_to_queue(&id).await },
                    );
                }
                // AppendAndPlay: append album songs to queue and start playing
                use nokkvi_data::types::player_settings::EnterBehavior;
                if self.enter_behavior == EnterBehavior::AppendAndPlay
                    && let Ok(index) = album_id_str.parse::<usize>()
                    && let Some(album) = self.library.albums.get(index)
                {
                    let id = album.id.clone();
                    let name = album.name.clone();
                    self.clear_active_playlist();
                    return self.shell_fire_and_forget_task(
                        move |shell| async move { shell.add_album_and_play(&id).await },
                        format!("Playing '{name}'"),
                        "append album and play",
                    );
                }
                // PlayAll / PlaySingle: replace queue with album (PlaySingle = PlayAll for albums)
                return self.play_entity_task(
                    &self.library.albums,
                    &album_id_str,
                    "album",
                    |a| a.id.clone(),
                    |shell, id| async move { shell.play_album(&id).await },
                );
            }
            AlbumsAction::AddBatchToQueue(payload) => {
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
            AlbumsAction::PlayBatch(payload) => {
                let len = payload.items.len();
                debug!(" Playing batch of {} items", len);
                self.clear_active_playlist();
                self.albums_page.common.slot_list.selected_indices.clear();
                return self.shell_task(
                    move |shell| async move { shell.play_batch(payload).await },
                    move |result| match result {
                        Ok(()) => Message::SwitchView(View::Queue),
                        Err(e) => {
                            error!(" Failed to play batch: {}", e);
                            Message::Toast(crate::app_message::ToastMessage::Push(
                                nokkvi_data::types::toast::Toast::new(
                                    format!("Failed to play batch: {e}"),
                                    nokkvi_data::types::toast::ToastLevel::Error,
                                ),
                            ))
                        }
                    },
                );
            }
            AlbumsAction::LoadLargeArtwork(album_id_str) => {
                if let Ok(index) = album_id_str.parse::<usize>() {
                    // Resolve the actual album ID using the expansion state and
                    // the passed index. Drop the borrow before calling &mut self
                    // methods below.
                    let resolved_album_id = self
                        .albums_page
                        .expansion
                        .get_entry_at(index, &self.library.albums, |a| &a.id)
                        .map(|entry| match entry {
                            crate::views::expansion::SlotListEntry::Parent(album) => {
                                album.id.clone()
                            }
                            crate::views::expansion::SlotListEntry::Child(_song, parent_id) => {
                                parent_id.clone()
                            }
                        });

                    let mut tasks = Vec::new();
                    // Direct call (rather than dispatching Message::Artwork(LoadLarge))
                    // so loading_large_artwork is set in the same tick as the action.
                    // Prevents racing with subsequent LoadLargeArtwork actions during
                    // rapid scroll/seek-settled cycles.
                    if let Some(album_id) = resolved_album_id {
                        tasks.push(self.handle_load_large_artwork(album_id));
                    }

                    // Prefetch mini artwork for viewport using canonical helper
                    if let Some(shell) = &self.app_service {
                        let cached: HashSet<&String> =
                            self.artwork.album_art.iter().map(|(k, _)| k).collect();
                        let prefetch_tasks = prefetch_album_artwork_tasks(
                            &self.albums_page.common.slot_list,
                            &self.library.albums,
                            &cached,
                            shell.albums().clone(),
                            |album| (album.id.clone(), album.artwork_url.clone()),
                        );
                        tasks.extend(prefetch_tasks);
                    }

                    if !tasks.is_empty() {
                        // Check if we need to fetch more pages while scrolling
                        let page_size = self.library_page_size.to_usize();
                        if let Some((offset, _)) = self.library.albums.needs_fetch(
                            self.albums_page.common.slot_list.viewport_offset,
                            page_size,
                        ) {
                            tasks.push(self.handle_albums_load_page(offset));
                        }
                        return Task::batch(tasks);
                    }
                }
            }
            AlbumsAction::LoadPage(offset) => {
                return self.handle_albums_load_page(offset);
            }
            AlbumsAction::ExpandAlbum(album_id) => {
                // Load tracks for the album and send them back to the view
                let id = album_id.clone();

                // CRITICAL FIX: Ensure large artwork is fetched when expanding an album via mouse click.
                // Standard navigation triggers this automatically, but FocusAndExpand skips standard focus.
                let artwork_task = self.handle_load_large_artwork(id.clone());

                let tracks_task = self.shell_task(
                    move |shell| async move {
                        let albums_vm = shell.albums().clone();
                        albums_vm.load_album_songs(&id).await
                    },
                    move |result| match result {
                        Ok(songs) => {
                            let tracks: Vec<nokkvi_data::backend::songs::SongUIViewData> =
                                songs.into_iter().map(|s| s.into()).collect();
                            Message::Albums(AlbumsMessage::TracksLoaded(album_id.clone(), tracks))
                        }
                        Err(e) => {
                            tracing::error!(" Failed to load album tracks: {}", e);
                            Message::NoOp
                        }
                    },
                );

                return Task::batch([artwork_task, tracks_task]);
            }
            AlbumsAction::PlayAlbumFromTrack(album_id, track_idx) => {
                if let Some(task) = self.guard_play_action() {
                    return task;
                }
                // Browsing panel: redirect play → add song to queue
                if self.browsing_panel.is_some() {
                    let song_id = self
                        .albums_page
                        .expansion
                        .children
                        .get(track_idx)
                        .map(|s| s.id.clone());
                    if let Some(sid) = song_id {
                        let title = self
                            .albums_page
                            .expansion
                            .children
                            .get(track_idx)
                            .map_or_else(|| "song".to_string(), |s| s.title.clone());
                        return self.shell_fire_and_forget_task(
                            move |shell| async move {
                                shell.add_song_to_queue_by_id(&sid, &album_id).await
                            },
                            format!("Added '{title}' to queue"),
                            "add song to queue",
                        );
                    }
                    return Task::none();
                }
                return self.shell_action_task(
                    move |shell| async move { shell.play_album_from_track(&album_id, track_idx).await },
                    Message::SwitchView(View::Queue), "play album from track",
                );
            }

            AlbumsAction::SetRating(item_id, kind, new_rating) => {
                let current = if matches!(kind, ItemKind::Album) {
                    self.library
                        .albums
                        .iter()
                        .find(|a| a.id == item_id)
                        .and_then(|a| a.rating)
                        .unwrap_or(0)
                } else {
                    // Song within expanded album
                    self.albums_page
                        .expansion
                        .children
                        .iter()
                        .find(|s| s.id == item_id)
                        .and_then(|s| s.rating)
                        .unwrap_or(0)
                };
                return self.set_item_rating_task(item_id, kind, new_rating, current);
            }
            AlbumsAction::ToggleStar(item_id, kind, star) => {
                let optimistic_msg = Self::starred_revert_message(item_id.clone(), kind, star);
                return Task::batch(vec![
                    Task::done(optimistic_msg),
                    self.star_item_task(item_id, kind, star),
                ]);
            }
            AlbumsAction::AddBatchToPlaylist(payload) => {
                return self.handle_add_batch_to_playlist(payload);
            }
            AlbumsAction::PlayNext(id) => {
                // To support PlayNext for albums, we just wrap it into a BatchItem
                use nokkvi_data::types::batch::{BatchItem, BatchPayload};
                let payload = BatchPayload::new().with_item(BatchItem::Album(id));

                if self.modes.random {
                    self.toast_warn("Shuffle is on — next tracks will be random, not these");
                }
                return self.shell_fire_and_forget_task(
                    move |shell| async move { shell.play_next_batch(payload).await },
                    "Added batch to play next".to_string(),
                    "play next batch",
                );
            }
            AlbumsAction::ShowInfo(item) => {
                return self.update(Message::InfoModal(
                    crate::widgets::info_modal::InfoModalMessage::Open(item),
                ));
            }
            AlbumsAction::ShowInFolder(album_id) => {
                return self.show_album_in_folder_task(album_id);
            }
            AlbumsAction::ShowSongInFolder(path) => {
                return self.handle_show_in_folder(path);
            }
            AlbumsAction::RefreshArtwork(album_id) => {
                return self.update(Message::Artwork(ArtworkMessage::RefreshAlbumArtwork(
                    album_id,
                )));
            }
            AlbumsAction::FindSimilar(id, label) => {
                return Task::done(Message::FindSimilar { id, label });
            }
            AlbumsAction::ColumnVisibilityChanged(col, value) => {
                return self.persist_albums_column_visibility(col, value);
            }
            _ => {} // None + already-handled common actions
        }

        cmd.map(Message::Albums)
    }

    /// Persist the user's albums column visibility toggle to config.toml +
    /// redb via `AppService::settings()`. The page's in-memory state was
    /// already mutated in `AlbumsPage::update`.
    pub(crate) fn persist_albums_column_visibility(
        &self,
        col: views::AlbumsColumn,
        value: bool,
    ) -> Task<Message> {
        match col {
            views::AlbumsColumn::Stars => {
                self.shell_spawn("persist_albums_show_stars", move |shell| async move {
                    shell.settings().set_albums_show_stars(value).await
                });
            }
            views::AlbumsColumn::SongCount => {
                self.shell_spawn("persist_albums_show_songcount", move |shell| async move {
                    shell.settings().set_albums_show_songcount(value).await
                });
            }
            views::AlbumsColumn::Plays => {
                self.shell_spawn("persist_albums_show_plays", move |shell| async move {
                    shell.settings().set_albums_show_plays(value).await
                });
            }
            views::AlbumsColumn::Love => {
                self.shell_spawn("persist_albums_show_love", move |shell| async move {
                    shell.settings().set_albums_show_love(value).await
                });
            }
            views::AlbumsColumn::Index => {
                self.shell_spawn("persist_albums_show_index", move |shell| async move {
                    shell.settings().set_albums_show_index(value).await
                });
            }
            views::AlbumsColumn::Thumbnail => {
                self.shell_spawn("persist_albums_show_thumbnail", move |shell| async move {
                    shell.settings().set_albums_show_thumbnail(value).await
                });
            }
            views::AlbumsColumn::Select => {
                self.shell_spawn("persist_albums_show_select", move |shell| async move {
                    shell.settings().set_albums_show_select(value).await
                });
            }
        }
        Task::none()
    }

    /// Routes `Message::AlbumsLoader(...)` arrivals to the existing
    /// `handle_albums_loaded` / `handle_albums_page_loaded` handlers.
    /// Mirrors the Genres dispatcher; the paged shape adds a second
    /// variant for `PageLoaded`.
    pub(crate) fn dispatch_albums_loader(
        &mut self,
        msg: crate::app_message::AlbumsLoaderMessage,
    ) -> Task<Message> {
        use crate::app_message::AlbumsLoaderMessage;
        match msg {
            AlbumsLoaderMessage::Loaded {
                result,
                total_count,
                background,
                anchor_id,
            } => self.handle_albums_loaded(result, total_count, background, anchor_id),
            AlbumsLoaderMessage::PageLoaded(result, total_count) => {
                self.handle_albums_page_loaded(result, total_count)
            }
        }
    }
}
