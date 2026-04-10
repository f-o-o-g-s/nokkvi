//! Album data loading and component message handlers

use std::collections::HashSet;

use iced::{Task, widget::image};
use nokkvi_data::backend::albums::AlbumUIViewData;
use tracing::{debug, error, warn};

use super::components::prefetch_album_artwork_tasks;
use crate::{
    Nokkvi, View,
    app_message::{ArtworkMessage, Message},
    views::{self, AlbumsAction, AlbumsMessage, HasCommonAction},
};

impl Nokkvi {
    pub(crate) fn handle_load_albums(&mut self) -> Task<Message> {
        debug!(" LoadAlbums message received, loading from app_service...");
        let view_str =
            views::AlbumsPage::sort_mode_to_api_string(self.albums_page.common.current_sort_mode);
        let sort_order = if self.albums_page.common.sort_ascending {
            "ASC"
        } else {
            "DESC"
        };
        let search_query_clone = self.albums_page.common.search_query.clone();

        // Mark buffer as loading to prevent duplicate fetches
        self.library.albums.set_loading(true);

        self.shell_task(
            move |shell| async move {
                let albums_vm = shell.albums().clone();
                let search_query = if search_query_clone.is_empty() {
                    None
                } else {
                    Some(search_query_clone.as_str())
                };
                debug!(
                    "📥 LoadAlbums: loading with view={}, sort={}, search={:?}",
                    view_str, sort_order, search_query
                );
                match albums_vm
                    .load_raw_albums(Some(view_str), Some(sort_order), search_query)
                    .await
                {
                    Ok(albums) => {
                        let mut ui_albums = Vec::new();
                        let (url, cred) = albums_vm.get_server_config().await;
                        for album in &albums {
                            ui_albums.push(AlbumUIViewData::from_album(album, &url, &cred));
                        }
                        let total_count = albums_vm.get_total_count() as usize;
                        (Ok(ui_albums), total_count)
                    }
                    Err(e) => (Err(e.to_string()), 0),
                }
            },
            |(result, total_count)| {
                Message::Albums(crate::views::AlbumsMessage::AlbumsLoaded(
                    result,
                    total_count,
                ))
            },
        )
    }

    /// Load a subsequent page of albums (triggered by scroll near edge of loaded data)
    pub(crate) fn handle_albums_load_page(&mut self, offset: usize) -> Task<Message> {
        let page_size = self.library_page_size.to_usize();
        debug!(
            " LoadAlbumsPage: offset={}, page_size={}",
            offset, page_size
        );

        let view_str =
            views::AlbumsPage::sort_mode_to_api_string(self.albums_page.common.current_sort_mode);
        let sort_order = if self.albums_page.common.sort_ascending {
            "ASC"
        } else {
            "DESC"
        };
        let search_query_clone = self.albums_page.common.search_query.clone();

        self.library.albums.set_loading(true);

        self.shell_task(
            move |shell| async move {
                let albums_vm = shell.albums().clone();
                let search_query = if search_query_clone.is_empty() {
                    None
                } else {
                    Some(search_query_clone.as_str())
                };
                match albums_vm
                    .load_raw_albums_page(
                        Some(view_str),
                        Some(sort_order),
                        search_query,
                        offset,
                        page_size,
                    )
                    .await
                {
                    Ok(albums) => {
                        let mut ui_albums = Vec::new();
                        let (url, cred) = albums_vm.get_server_config().await;
                        for album in &albums {
                            ui_albums.push(AlbumUIViewData::from_album(album, &url, &cred));
                        }
                        let total_count = albums_vm.get_total_count() as usize;
                        (Ok(ui_albums), total_count)
                    }
                    Err(e) => (Err(e.to_string()), 0),
                }
            },
            |(result, total_count)| {
                Message::Albums(crate::views::AlbumsMessage::AlbumsPageLoaded(
                    result,
                    total_count,
                ))
            },
        )
    }

    /// Handle a subsequent page of albums being loaded (appends to buffer)
    pub(crate) fn handle_albums_page_loaded(
        &mut self,
        result: Result<Vec<AlbumUIViewData>, String>,
        total_count: usize,
    ) -> Task<Message> {
        impl_page_loaded_handler!(self, albums, "Albums", result, total_count)
    }

    pub(crate) fn handle_albums_loaded(
        &mut self,
        result: Result<Vec<AlbumUIViewData>, String>,
        total_count: usize,
    ) -> Task<Message> {
        self.library.counts.albums = total_count;
        match result {
            Ok(new_albums) => {
                debug!(
                    "✅ Loaded {} albums from AlbumsService (total in library: {})",
                    new_albums.len(),
                    total_count
                );
                if new_albums.len() >= 3 {
                    debug!(
                        "📋 First 3 albums: {}, {}, {}",
                        new_albums[0].name, new_albums[1].name, new_albums[2].name
                    );
                }
                self.library.albums.set_first_page(new_albums, total_count);

                // Reset slot list to first item when filtering via search or clearing
                // This ensures the center slot focuses on the first matching result
                self.albums_page.common.slot_list.viewport_offset = 0;
                let mut tasks: Vec<Task<Message>> = Vec::new();

                // NOTE: Don't re-focus search field here - text_input maintains its own focus state.
                // Re-focusing here causes issues when users press Escape (widget unfocuses but we'd re-focus).

                // Load artwork for currently displayed albums using canonical prefetch
                if let Some(shell) = &self.app_service {
                    let cached: HashSet<&String> = self.artwork.album_art.keys().collect();
                    let prefetch_tasks = prefetch_album_artwork_tasks(
                        &self.albums_page.common.slot_list,
                        &self.library.albums,
                        &cached,
                        shell.albums().clone(),
                        |album| (album.id.clone(), album.artwork_url.clone()),
                    );
                    tasks.extend(prefetch_tasks);
                }

                // Large artwork for center
                if let Some(center_idx) = self
                    .albums_page
                    .common
                    .slot_list
                    .get_center_item_index(self.library.albums.len())
                    && let Some(album) = self.library.albums.get(center_idx)
                {
                    tasks.push(Task::done(Message::Artwork(ArtworkMessage::LoadLarge(
                        album.id.clone(),
                    ))));
                }

                // Trigger background prefetch on first load if cache is incomplete
                if !self.artwork.album_prefetch_triggered {
                    self.artwork.album_prefetch_triggered = true;
                    tasks.push(Task::done(Message::Artwork(ArtworkMessage::StartPrefetch)));
                }

                // If CenterOnPlaying triggered this reload (item wasn't in buffer),
                // re-dispatch so the item can be found in the search results.
                if self.pending_center_on_playing {
                    self.pending_center_on_playing = false;
                    tasks.push(Task::done(Message::Hotkey(
                        crate::app_message::HotkeyMessage::CenterOnPlaying,
                    )));
                }

                return Task::batch(tasks);
            }
            Err(e) => {
                error!("Error loading albums: {}", e);
                self.library.albums.set_loading(false);
                self.toast_error(format!("Failed to load albums: {e}"));
            }
        }
        Task::none()
    }

    pub(crate) fn handle_artwork_loaded(
        &mut self,
        id: String,
        handle: Option<image::Handle>,
    ) -> Task<Message> {
        if let Some(h) = handle {
            self.artwork.album_art.insert(id, h);
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

            // First try to find in albums list
            if let Some(album) = self.library.albums.iter().find(|a| a.id == album_id) {
                let art_id = album.id.clone();
                let artwork_size = self.artwork_resolution.to_size();
                return Task::perform(
                    async move {
                        let (url, cred) = albums_vm.get_server_config().await;
                        let artwork_url = nokkvi_data::utils::artwork_url::build_cover_art_url(
                            &art_id,
                            &url,
                            &cred,
                            artwork_size,
                        );
                        let path = albums_vm
                            .get_artwork_cache_path(&artwork_url, artwork_size)
                            .await;
                        let handle = path.map(image::Handle::from_path);
                        (art_id, handle)
                    },
                    |(id, handle)| Message::Artwork(ArtworkMessage::LargeLoaded(id, handle)),
                );
            }

            // Fallback: Check queue songs (fixes artwork loading when albums list is empty/filtered)
            // Construct a full-size URL from the album_id rather than reusing the
            // queue song's pre-baked thumbnail URL (which is only 80px).
            if self
                .library
                .queue_songs
                .iter()
                .any(|s| s.album_id == album_id)
            {
                let art_id = album_id.clone();
                let artwork_size = self.artwork_resolution.to_size();
                return Task::perform(
                    async move {
                        let (url, cred) = albums_vm.get_server_config().await;
                        let artwork_url = nokkvi_data::utils::artwork_url::build_cover_art_url(
                            &art_id,
                            &url,
                            &cred,
                            artwork_size,
                        );
                        let path = albums_vm
                            .get_artwork_cache_path(&artwork_url, artwork_size)
                            .await;
                        let handle = path.map(image::Handle::from_path);
                        (art_id, handle)
                    },
                    |(id, handle)| Message::Artwork(ArtworkMessage::LargeLoaded(id, handle)),
                );
            }

            // Final fallback: construct artwork URL directly from album_id
            // This handles songs whose albums aren't in the paginated buffer
            let art_id = album_id.clone();
            let artwork_size = self.artwork_resolution.to_size();
            return Task::perform(
                async move {
                    let (url, cred) = albums_vm.get_server_config().await;
                    let artwork_url = nokkvi_data::utils::artwork_url::build_cover_art_url(
                        &art_id,
                        &url,
                        &cred,
                        artwork_size,
                    );
                    let path = albums_vm
                        .get_artwork_cache_path(&artwork_url, artwork_size)
                        .await;
                    let handle = path.map(image::Handle::from_path);
                    (art_id, handle)
                },
                |(id, handle)| Message::Artwork(ArtworkMessage::LargeLoaded(id, handle)),
            );
        }
        Task::none()
    }

    /// Refresh a specific album's artwork: evict large cache, re-fetch from server,
    /// and reload both mini and large artwork via `RefreshComplete`.
    pub(crate) fn handle_refresh_album_artwork(&mut self, album_id: String) -> Task<Message> {
        use tracing::info;

        info!(" [REFRESH] Refreshing artwork for album {}", album_id);

        // Only evict large artwork so the panel shows a placeholder during refresh.
        // Do NOT evict from album_art — that would gray out every slot list row
        // sharing this album_id. The old mini thumbnail stays visible until
        // RefreshComplete replaces it atomically.
        self.artwork.large_artwork.pop(&album_id);
        self.artwork.refresh_large_artwork_snapshot();

        if let Some(shell) = &self.app_service {
            let albums_vm = shell.albums().clone();
            let id = album_id.clone();
            let artwork_size = self.artwork_resolution.to_size();

            let refresh_task = Task::perform(
                async move {
                    // Backend: evict caches + re-fetch, returns raw bytes per size
                    let fetched = albums_vm
                        .refresh_single_album_artwork(&id, artwork_size)
                        .await?;

                    // Build handles from fresh bytes (not disk paths)
                    let mut thumb_handle: Option<image::Handle> = None;
                    let mut large_handle: Option<image::Handle> = None;

                    for (size, data) in fetched {
                        // Use Handle::from_bytes — NOT Handle::from_path.
                        // Iced's GPU texture cache keys on the Handle ID, which for
                        // from_path is derived from the file path. Since refresh
                        // overwrites the same disk cache path, from_path produces an
                        // identical ID and Iced serves the stale texture. from_bytes
                        // derives the ID from content, busting the stale cache entry.
                        let handle = image::Handle::from_bytes(data);
                        if size == Some(nokkvi_data::utils::artwork_url::THUMBNAIL_SIZE) {
                            thumb_handle = Some(handle);
                        } else if size == artwork_size {
                            large_handle = Some(handle);
                        }
                    }

                    Ok::<_, anyhow::Error>((id, thumb_handle, large_handle))
                },
                |result| match result {
                    Ok((id, thumb, large)) => {
                        Message::Artwork(ArtworkMessage::RefreshComplete(id, thumb, large))
                    }
                    Err(e) => {
                        tracing::error!(" [REFRESH] Failed to refresh artwork: {e}");
                        Message::Toast(crate::app_message::ToastMessage::Push(
                            nokkvi_data::types::toast::Toast::new(
                                format!("Failed to refresh artwork: {e}"),
                                nokkvi_data::types::toast::ToastLevel::Error,
                            ),
                        ))
                    }
                },
            );

            let toast_task = Task::done(Message::Toast(crate::app_message::ToastMessage::Push(
                nokkvi_data::types::toast::Toast::new(
                    "Refreshing artwork…".to_string(),
                    nokkvi_data::types::toast::ToastLevel::Info,
                ),
            )));

            return Task::batch([toast_task, refresh_task]);
        }
        Task::none()
    }

    /// Handle the result of an artwork refresh — cache both mini and large atomically.
    pub(crate) fn handle_refresh_complete(
        &mut self,
        album_id: String,
        thumb: Option<image::Handle>,
        large: Option<image::Handle>,
    ) -> Task<Message> {
        if thumb.is_none() && large.is_none() {
            self.toast_warn("No artwork found on server for this album");
            return Task::none();
        }
        if let Some(h) = thumb {
            self.artwork.album_art.insert(album_id.clone(), h);
        }
        if let Some(h) = large {
            self.artwork.large_artwork.put(album_id, h);
            self.artwork.refresh_large_artwork_snapshot();
        }
        self.toast_success("Artwork refreshed");
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
                        let (url, cred) = albums_vm.get_server_config().await;
                        let artwork_url = nokkvi_data::utils::artwork_url::build_cover_art_url(
                            &art_id,
                            &url,
                            &cred,
                            artwork_size,
                        );
                        let path = albums_vm
                            .get_artwork_cache_path(&artwork_url, artwork_size)
                            .await;
                        if let Some(p) = path
                            && let Ok(bytes) = tokio::fs::read(p).await
                        {
                            let dominant = tokio::task::spawn_blocking(move || {
                                nokkvi_data::utils::dominant_color::extract_dominant_color(&bytes)
                            })
                            .await
                            .unwrap_or(None);

                            return dominant
                                .map(|(r, g, b)| (art_id, iced::Color::from_rgb8(r, g, b)));
                        }
                        None
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

    pub(crate) fn handle_start_artwork_prefetch(
        &mut self,
        progress: Option<nokkvi_data::types::progress::ProgressHandle>,
    ) -> Task<Message> {
        // Start background prefetch of all album artwork
        let artwork_size = self.artwork_resolution.to_size();
        self.shell_spawn("album_artwork_prefetch", move |shell| async move {
            let albums_vm = shell.albums().clone();
            albums_vm
                .start_artwork_prefetch(progress, artwork_size)
                .await;
            Ok(())
        });
        Task::none()
    }

    pub(crate) fn handle_start_artist_prefetch(
        &mut self,
        progress: Option<nokkvi_data::types::progress::ProgressHandle>,
    ) -> Task<Message> {
        // Start background prefetch of all artist artwork.
        // Fetches the full artist list directly from the API (not the PagedBuffer)
        // to ensure we cover all artists, not just the currently loaded page.
        let disk_cache = self.artwork.artist_disk_cache.clone();

        self.shell_spawn("artist_artwork_prefetch", move |shell| async move {
            let artists_vm = shell.artists().clone();
            let albums_vm = shell.albums().clone();
            let (server_url, subsonic_cred) = albums_vm.get_server_config().await;
            if server_url.is_empty() || subsonic_cred.is_empty() {
                return Ok(());
            }

            // Load ALL artists from the API (limit=None defaults to 999999)
            let all_artists = artists_vm
                .load_raw_artists_page(Some("name"), Some("ASC"), None, 0, 999999)
                .await;

            match all_artists {
                Ok(artists) => {
                    let artist_ids: Vec<(String, String)> =
                        artists.into_iter().map(|a| (a.id, a.name)).collect();
                    debug!(
                        " [PREFETCH] Fetched {} artists from API for prefetch",
                        artist_ids.len()
                    );
                    let _rx = nokkvi_data::services::artwork_prefetch::start_artist_prefetch(
                        artist_ids,
                        server_url,
                        subsonic_cred,
                        disk_cache,
                        progress,
                    );
                }
                Err(e) => {
                    debug!(" [PREFETCH] Failed to load artists for prefetch: {}", e);
                }
            }
            Ok(())
        });
        Task::none()
    }

    pub(crate) fn handle_albums(&mut self, msg: views::AlbumsMessage) -> Task<Message> {
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
        let (cmd, action) =
            self.albums_page
                .update(msg, self.library.albums.len(), &self.library.albums);

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
                    self.active_playlist_info = None;
                    self.persist_active_playlist_info();
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
                self.active_playlist_info = None;
                self.persist_active_playlist_info();
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
                if let Ok(_index) = album_id_str.parse::<usize>() {
                    let mut tasks = Vec::new();

                    // Resolve the actual album ID using the expansion state and the passed index
                    if let Some(entry) =
                        self.albums_page
                            .expansion
                            .get_entry_at(_index, &self.library.albums, |a| &a.id)
                    {
                        let album_id = match entry {
                            crate::views::expansion::SlotListEntry::Parent(album) => {
                                album.id.clone()
                            }
                            crate::views::expansion::SlotListEntry::Child(_song, parent_id) => {
                                parent_id.clone()
                            }
                        };
                        tasks.push(Task::done(Message::Artwork(ArtworkMessage::LoadLarge(
                            album_id,
                        ))));
                    }

                    // Prefetch mini artwork for viewport using canonical helper
                    if let Some(shell) = &self.app_service {
                        let cached: HashSet<&String> = self.artwork.album_art.keys().collect();
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

            AlbumsAction::SetRating(item_id, item_type, new_rating) => {
                let current = if item_type == "album" {
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
                return self.set_item_rating_task(item_id, item_type, new_rating, current);
            }
            AlbumsAction::ToggleStar(item_id, item_type, star) => {
                let optimistic_msg = Self::starred_revert_message(item_id.clone(), item_type, star);
                return Task::batch(vec![
                    Task::done(optimistic_msg),
                    self.star_item_task(item_id, item_type, star),
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
            _ => {} // None + already-handled common actions
        }

        cmd.map(Message::Albums)
    }
}
