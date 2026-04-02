//! Artist data loading and component message handlers

use iced::{Task, widget::image};
use nokkvi_data::backend::artists::ArtistUIViewData;
use tracing::{debug, error};

use crate::{
    Nokkvi, View,
    app_message::{ArtworkMessage, Message},
    views::{self, ArtistsAction, ArtistsMessage, HasCommonAction},
    widgets,
};

impl Nokkvi {
    pub(crate) fn handle_load_artists(&mut self) -> Task<Message> {
        debug!(" LoadArtists message received, loading from app_service...");
        let view_str =
            views::ArtistsPage::sort_mode_to_api_string(self.artists_page.common.current_sort_mode);
        let is_rating_sort =
            self.artists_page.common.current_sort_mode == widgets::view_header::SortMode::Rating;
        let sort_order = if self.artists_page.common.sort_ascending {
            "ASC"
        } else {
            "DESC"
        };
        let search_query_clone = self.artists_page.common.search_query.clone();

        // Mark buffer as loading to prevent duplicate fetches
        self.library.artists.set_loading(true);

        self.shell_task(
            move |shell| async move {
                let artists_vm = shell.artists().clone();
                let search_query = if search_query_clone.is_empty() {
                    None
                } else {
                    Some(search_query_clone.as_str())
                };
                debug!(
                    "📥 LoadArtists: loading with view={}, sort={}, search={:?}",
                    view_str, sort_order, search_query
                );
                match artists_vm
                    .load_raw_artists(Some(view_str), Some(sort_order), search_query)
                    .await
                {
                    Ok(artists) => {
                        let mut ui_artists: Vec<ArtistUIViewData> =
                            artists.into_iter().map(ArtistUIViewData::from).collect();
                        let total_count = artists_vm.get_total_count() as usize;

                        // Client-side sort by rating if needed
                        if is_rating_sort {
                            ui_artists.sort_by(|a, b| match (a.rating, b.rating) {
                                (Some(_), None) => std::cmp::Ordering::Less,
                                (None, Some(_)) => std::cmp::Ordering::Greater,
                                (Some(ra), Some(rb)) => rb.cmp(&ra),
                                (None, None) => std::cmp::Ordering::Equal,
                            });
                        }

                        (Ok(ui_artists), total_count)
                    }
                    Err(e) => (Err(e.to_string()), 0),
                }
            },
            |(result, total_count)| {
                Message::Artists(crate::views::ArtistsMessage::ArtistsLoaded(
                    result,
                    total_count,
                ))
            },
        )
    }

    /// Load a subsequent page of artists (triggered by scroll near edge of loaded data)
    pub(crate) fn handle_artists_load_page(&mut self, offset: usize) -> Task<Message> {
        use nokkvi_data::types::paged_buffer::PAGE_SIZE;
        debug!(
            " LoadArtistsPage: offset={}, page_size={}",
            offset, PAGE_SIZE
        );

        let view_str =
            views::ArtistsPage::sort_mode_to_api_string(self.artists_page.common.current_sort_mode);
        let is_rating_sort =
            self.artists_page.common.current_sort_mode == widgets::view_header::SortMode::Rating;
        let sort_order = if self.artists_page.common.sort_ascending {
            "ASC"
        } else {
            "DESC"
        };
        let search_query_clone = self.artists_page.common.search_query.clone();

        self.library.artists.set_loading(true);

        self.shell_task(
            move |shell| async move {
                let artists_vm = shell.artists().clone();
                let search_query = if search_query_clone.is_empty() {
                    None
                } else {
                    Some(search_query_clone.as_str())
                };
                match artists_vm
                    .load_raw_artists_page(
                        Some(view_str),
                        Some(sort_order),
                        search_query,
                        offset,
                        PAGE_SIZE,
                    )
                    .await
                {
                    Ok(artists) => {
                        let mut ui_artists: Vec<ArtistUIViewData> =
                            artists.into_iter().map(ArtistUIViewData::from).collect();
                        let total_count = artists_vm.get_total_count() as usize;

                        // Client-side sort by rating if needed
                        if is_rating_sort {
                            ui_artists.sort_by(|a, b| match (a.rating, b.rating) {
                                (Some(_), None) => std::cmp::Ordering::Less,
                                (None, Some(_)) => std::cmp::Ordering::Greater,
                                (Some(ra), Some(rb)) => rb.cmp(&ra),
                                (None, None) => std::cmp::Ordering::Equal,
                            });
                        }

                        (Ok(ui_artists), total_count)
                    }
                    Err(e) => (Err(e.to_string()), 0),
                }
            },
            |(result, total_count)| {
                Message::Artists(crate::views::ArtistsMessage::ArtistsPageLoaded(
                    result,
                    total_count,
                ))
            },
        )
    }

    /// Handle a subsequent page of artists being loaded (appends to buffer)
    pub(crate) fn handle_artists_page_loaded(
        &mut self,
        result: Result<Vec<ArtistUIViewData>, String>,
        total_count: usize,
    ) -> Task<Message> {
        impl_page_loaded_handler!(self, artists, "Artists", result, total_count)
    }

    pub(crate) fn handle_artists_loaded(
        &mut self,
        result: Result<Vec<ArtistUIViewData>, String>,
        total_count: usize,
    ) -> Task<Message> {
        self.library.counts.artists = total_count;
        match result {
            Ok(new_artists) => {
                debug!(
                    "✅ Loaded {} artists (total in library: {})",
                    new_artists.len(),
                    total_count
                );
                self.library
                    .artists
                    .set_first_page(new_artists, total_count);
                self.artists_page.common.slot_list.viewport_offset = 0;

                // Load artwork for artists using Navidrome getCoverArt API
                // The API supports ar-{artistId} and auto-falls back to album covers
                let mut tasks: Vec<Task<Message>> = Vec::new();

                // NOTE: Don't re-focus search field here - text_input maintains its own focus state.
                // Re-focusing here causes issues when users press Escape (widget unfocuses but we'd re-focus).

                if let Some(shell) = &self.app_service {
                    let albums_vm = shell.albums().clone();
                    let total = self.library.artists.len();
                    if total > 0 {
                        // Load cached artwork for visible slots
                        self.load_artist_mini_artwork_from_cache();

                        // Collect artists still missing artwork for network fetch
                        let mut artists_to_load: Vec<(String, String)> = Vec::new(); // (id, name)
                        for idx in self.artists_page.common.slot_list.prefetch_indices(total) {
                            if let Some(artist) = self.library.artists.get(idx)
                                && !self.artwork.album_art.contains_key(&artist.id)
                            {
                                artists_to_load.push((artist.id.clone(), artist.name.clone()));
                            }
                        }

                        // Clone disk cache for async use
                        let disk_cache = self.artwork.artist_disk_cache.clone();

                        // Load mini artwork for artists not in cache
                        for (id, name) in artists_to_load {
                            let vm = albums_vm.clone();
                            let cache = disk_cache.clone();
                            tasks.push(Task::perform(
                                async move {
                                    let (url, cred) = vm.get_server_config().await;
                                    if url.is_empty() || cred.is_empty() {
                                        return (id, None);
                                    }
                                    let art_id = format!("ar-{id}");
                                    let client = reqwest::Client::new();
                                    match nokkvi_data::utils::artwork_url::fetch_cover_art(
                                        &client,
                                        &art_id,
                                        &url,
                                        &cred,
                                        Some(80),
                                    )
                                    .await
                                    {
                                        Some(bytes) => {
                                            let bytes_vec = bytes.clone();
                                            if bytes_vec.len() > 100 {
                                                // Save to disk cache
                                                let cache_key = format!("ar-{id}_80");
                                                if let Some(c) = cache.as_ref() {
                                                    let _ = c.insert(&cache_key, &bytes_vec);
                                                    debug!(
                                                        "🖼️ Loaded {} bytes for artist '{}'",
                                                        bytes_vec.len(),
                                                        name
                                                    );
                                                    // Use cache path for stable Handle ID
                                                    return (
                                                        id,
                                                        Some(image::Handle::from_path(
                                                            c.get_path(&cache_key),
                                                        )),
                                                    );
                                                }
                                                // No disk cache — fall back to from_bytes
                                                return (
                                                    id,
                                                    Some(image::Handle::from_bytes(bytes_vec)),
                                                );
                                            }
                                            (id, None)
                                        }
                                        None => (id, None),
                                    }
                                },
                                |(id, handle)| Message::Artwork(ArtworkMessage::Loaded(id, handle)),
                            ));
                        }

                        // Load large artwork for center artist
                        if let Some(center_idx) = self
                            .artists_page
                            .common
                            .slot_list
                            .get_center_item_index(total)
                            && let Some(artist) = self.library.artists.get(center_idx)
                            && self.artwork.large_artwork.peek(&artist.id).is_none()
                        {
                            // Check disk cache first for large artwork
                            let cache_key = format!("ar-{}_500", artist.id);
                            let cache_ref = self.artwork.artist_disk_cache.as_ref();
                            if let Some(cache) = cache_ref {
                                if cache.contains(&cache_key) {
                                    self.artwork.large_artwork.put(
                                        artist.id.clone(),
                                        image::Handle::from_path(cache.get_path(&cache_key)),
                                    );
                                    self.artwork.refresh_large_artwork_snapshot();
                                    // Skip network fetch since loaded from cache
                                } else {
                                    // Not in cache, fetch from network
                                    let id = artist.id.clone();
                                    let name = artist.name.clone();
                                    let external_url = artist.image_url.clone();
                                    let vm = albums_vm.clone();
                                    let disk_cache = self.artwork.artist_disk_cache.clone();
                                    tasks.push(Task::perform(
                                                async move {
                                                    let (url, cred) = vm.get_server_config().await;
                                                    let client = reqwest::Client::new();
                                                    // Use external image if available (GET), otherwise getCoverArt (POST)
                                                    let art_id = external_url.unwrap_or_else(|| format!("ar-{id}"));
                                                    match nokkvi_data::utils::artwork_url::fetch_cover_art(
                                                        &client, &art_id, &url, &cred, Some(500),
                                                    ).await {
                                                        Some(bytes) => {
                                                            let bytes_vec = bytes.clone();
                                                            if bytes_vec.len() > 100 {
                                                                // Save to disk cache
                                                                let cache_key = format!("ar-{id}_500");
                                                                if let Some(c) = disk_cache.as_ref() {
                                                                    let _ = c.insert(&cache_key, &bytes_vec);
                                                                    debug!("🖼️ Loaded large artwork ({} bytes) for artist '{}'", bytes_vec.len(), name);
                                                                    return (
                                                                        id,
                                                                        Some(image::Handle::from_path(
                                                                            c.get_path(&cache_key),
                                                                        )),
                                                                    );
                                                                }
                                                                // No disk cache — fall back to from_bytes
                                                                return (
                                                                    id,
                                                                    Some(image::Handle::from_bytes(bytes_vec)),
                                                                );
                                                            }
                                                            (id, None)
                                                        }
                                                        None => (id, None),
                                                    }
                                                },
                                                |(id, handle)| {
                                                    Message::Artwork(ArtworkMessage::LargeLoaded(id, handle))
                                                },
                                            ));
                                }
                            }
                        }
                    }
                }

                // Trigger background prefetch on first load
                if !self.artwork.artist_prefetch_triggered {
                    self.artwork.artist_prefetch_triggered = true;
                    tasks.push(Task::done(Message::Artwork(
                        ArtworkMessage::StartArtistPrefetch,
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
                error!("Error loading artists: {}", e);
                self.library.artists.set_loading(false);
                self.toast_error(format!("Failed to load artists: {e}"));
            }
        }
        Task::none()
    }

    pub(crate) fn handle_artists(&mut self, msg: views::ArtistsMessage) -> Task<Message> {
        self.play_view_sfx(
            matches!(
                msg,
                ArtistsMessage::SlotListNavigateUp | ArtistsMessage::SlotListNavigateDown
            ),
            matches!(
                msg,
                ArtistsMessage::CollapseExpansion | ArtistsMessage::ExpandCenter
            ),
        );
        let (cmd, action) =
            self.artists_page
                .update(msg, self.library.artists.len(), &self.library.artists);

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
                    self.active_playlist_info = None;
                    self.persist_active_playlist_info();
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
                return self.shell_task(
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
                                tracing::error!(" Failed to load artist albums: {}", e);
                                Message::NoOp
                            }
                        }
                    },
                );
            }
            ArtistsAction::ExpandAlbum(album_id) => {
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
                            Message::Artists(ArtistsMessage::TracksLoaded(aid, songs))
                        }
                        Err(e) => {
                            tracing::error!(" Failed to load album tracks for artist: {}", e);
                            Message::NoOp
                        }
                    },
                );
            }
            ArtistsAction::PlayTrack(song_id) => {
                if let Some(task) = self.guard_play_action() {
                    return task;
                }
                // Find the song in sub_expansion children to get its album_id and build a single-song queue
                if let Some(song) = self
                    .artists_page
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
                        "play track from artist expansion",
                    );
                }
            }
            ArtistsAction::StarArtist(artist_id) => {
                // Route through central update message for cross-view propagation
                let optimistic_msg =
                    Self::starred_revert_message(artist_id.clone(), "artist", true);
                return Task::batch(vec![
                    Task::done(optimistic_msg),
                    self.star_item_task(artist_id, "artist", true),
                ]);
            }
            ArtistsAction::UnstarArtist(artist_id) => {
                // Route through central update message for cross-view propagation
                let optimistic_msg =
                    Self::starred_revert_message(artist_id.clone(), "artist", false);
                return Task::batch(vec![
                    Task::done(optimistic_msg),
                    self.star_item_task(artist_id, "artist", false),
                ]);
            }
            ArtistsAction::SetRating(item_id, item_type, new_rating) => {
                let current = if item_type == "artist" {
                    self.library
                        .artists
                        .iter()
                        .find(|a| a.id == item_id)
                        .and_then(|a| a.rating)
                        .unwrap_or(0)
                } else if item_type == "song" {
                    // Song within sub-expanded album
                    self.artists_page
                        .sub_expansion
                        .children
                        .iter()
                        .find(|s| s.id == item_id)
                        .and_then(|s| s.rating)
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
                return self.set_item_rating_task(item_id, item_type, new_rating, current);
            }
            ArtistsAction::ToggleStar(item_id, item_type, star) => {
                let optimistic_msg = Self::starred_revert_message(item_id.clone(), item_type, star);
                return Task::batch(vec![
                    Task::done(optimistic_msg),
                    self.star_item_task(item_id, item_type, star),
                ]);
            }
            ArtistsAction::LoadPage(offset) => {
                return self.handle_artists_load_page(offset);
            }
            ArtistsAction::AddBatchToPlaylist(payload) => {
                return self.handle_add_batch_to_playlist(payload);
            }
            ArtistsAction::PlayNextBatch(payload) => {
                if self.modes.random {
                    self.toast_warn("Shuffle is on — next tracks will be random, not these");
                }
                return self.shell_fire_and_forget_task(
                    move |shell| async move { shell.play_next_batch(payload).await },
                    "Added batch to play next".to_string(),
                    "play next batch",
                );
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
            _ => {} // None + already-handled common actions
        }

        // Load artwork from disk cache for visible artist slots after any slot list change
        self.load_artist_mini_artwork_from_cache();
        let total = self.library.artists.len();
        if total > 0 {
            let cache_ref = self.artwork.artist_disk_cache.as_ref();

            // Load large artwork for center artist from disk cache
            if let Some(center_idx) = self
                .artists_page
                .common
                .slot_list
                .get_center_item_index(total)
                && let Some(artist) = self.library.artists.get(center_idx)
                && self.artwork.large_artwork.peek(&artist.id).is_none()
                && let Some(cache) = cache_ref
                && cache.contains(&format!("ar-{}_500", artist.id))
            {
                let cache_key = format!("ar-{}_500", artist.id);
                self.artwork.large_artwork.put(
                    artist.id.clone(),
                    image::Handle::from_path(cache.get_path(&cache_key)),
                );
                self.artwork.refresh_large_artwork_snapshot();
            }
        }

        // Check if we need to fetch more pages while scrolling
        if !self.library.artists.is_empty()
            && let Some((offset, _)) = self
                .library
                .artists
                .needs_fetch(self.artists_page.common.slot_list.viewport_offset)
        {
            let page_task = self.handle_artists_load_page(offset);
            return Task::batch(vec![cmd.map(Message::Artists), page_task]);
        }

        cmd.map(Message::Artists)
    }
}
