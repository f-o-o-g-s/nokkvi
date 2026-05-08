//! Artist data loading and component message handlers

use iced::{Task, widget::image};
use nokkvi_data::backend::artists::ArtistUIViewData;
use tracing::{debug, error};

use crate::{
    Nokkvi, View,
    app_message::{ArtworkMessage, Message},
    update::components::PaginatedFetch,
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

    /// Handle a subsequent page of artists being loaded (appends to buffer).
    /// Mirror of `handle_albums_page_loaded` — drives
    /// `try_resolve_pending_expand_artist` after the append so the
    /// artist-find-and-expand chain (and Shift+C center-only fallback) can
    /// advance once the new page lands.
    pub(crate) fn handle_artists_page_loaded(
        &mut self,
        result: Result<Vec<ArtistUIViewData>, String>,
        total_count: usize,
    ) -> Task<Message> {
        match result {
            Ok(new_items) => {
                let count = new_items.len();
                let loaded_before = self.library.artists.loaded_count();
                self.library.artists.append_page(new_items, total_count);
                debug!(
                    "📄 Artists page loaded: {} new items ({}→{} of {})",
                    count,
                    loaded_before,
                    self.library.artists.loaded_count(),
                    total_count,
                );
                if let Some(task) = self.try_resolve_pending_expand_artist() {
                    return task;
                }
            }
            Err(e) => {
                if e.contains("Unauthorized") {
                    self.library.artists.set_loading(false);
                    return self.handle_session_expired();
                }
                error!("Error loading Artists page: {}", e);
                self.library.artists.set_loading(false);
                self.cancel_pending_expand();
                self.toast_error(format!("Failed to load Artists: {e}"));
            }
        }
        Task::none()
    }

    pub(crate) fn handle_artists_loaded(
        &mut self,
        result: Result<Vec<ArtistUIViewData>, String>,
        total_count: usize,
        background: bool,
        anchor_id: Option<String>,
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

                if !background {
                    self.artists_page.common.slot_list.viewport_offset = 0;
                    self.artists_page.common.slot_list.selected_indices.clear();
                } else if let Some(ref id) = anchor_id {
                    let artists = &self.library.artists;
                    if let Some(new_idx) = artists.iter().position(|a| a.id == *id) {
                        self.artists_page.common.slot_list.viewport_offset = new_idx;
                    } else {
                        // Anchor not found in this page (expected with Random sort — the new
                        // first page is a different random sample). Reset rather than leaving
                        // viewport_offset pointing at whoever now occupies the old index.
                        self.artists_page.common.slot_list.viewport_offset = 0;
                    }
                    // Clear stale selected_offset: after re-ordering, the old absolute index
                    // maps to a different artist and would highlight the wrong slot.
                    self.artists_page.common.slot_list.selected_offset = None;
                }

                // Load artwork for artists using Navidrome getCoverArt API
                // The API supports ar-{artistId} and auto-falls back to album covers
                let mut tasks: Vec<Task<Message>> = Vec::new();

                // NOTE: Don't re-focus search field here - text_input maintains its own focus state.
                // Re-focusing here causes issues when users press Escape (widget unfocuses but we'd re-focus).

                let total = self.library.artists.len();
                if total > 0 && self.app_service.is_some() {
                    // Mini artwork for visible slots — async fetches via cached HTTP client.
                    tasks.push(self.prefetch_artist_mini_artwork_tasks());

                    // Large artwork for the center artist.
                    if let Some(center_idx) = self
                        .artists_page
                        .common
                        .slot_list
                        .get_center_item_index(total)
                        && let Some(artist) = self.library.artists.get(center_idx)
                    {
                        let id = artist.id.clone();
                        tasks.push(self.handle_load_artist_large_artwork(id));
                    }
                }

                // Drive the artist find-and-expand chain forward (click-driven
                // NavigateAndExpandArtist, or Shift+C CenterOnPlaying fallback).
                if let Some(task) = self.try_resolve_pending_expand_artist() {
                    tasks.push(task);
                }

                if !tasks.is_empty() {
                    return Task::batch(tasks);
                }
            }
            Err(e) => {
                if e.contains("Unauthorized") {
                    self.library.artists.set_loading(false);
                    return self.handle_session_expired();
                }
                error!("Error loading artists: {}", e);
                self.library.artists.set_loading(false);
                self.cancel_pending_expand();
                self.toast_error(format!("Failed to load artists: {e}"));
            }
        }
        Task::none()
    }

    pub(crate) fn handle_artists(&mut self, msg: views::ArtistsMessage) -> Task<Message> {
        if let ArtistsMessage::SetOpenMenu(next) = msg {
            return Task::done(Message::SetOpenMenu(next));
        }
        if matches!(msg, ArtistsMessage::Roulette) {
            return Task::done(Message::Roulette(
                crate::app_message::RouletteMessage::Start(crate::View::Artists),
            ));
        }
        if let ArtistsMessage::OpenExternalUrl(url) = msg {
            if let Err(e) = std::process::Command::new("xdg-open").arg(&url).spawn() {
                tracing::warn!("Failed to open URL '{}': {}", url, e);
            }
            return Task::none();
        }
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
                                tracing::error!(" Failed to load artist albums: {}", e);
                                Message::NoOp
                            }
                        }
                    },
                );

                return Task::batch([artwork_task, albums_task]);
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
            ArtistsAction::FindSimilar(id, label) => {
                return Task::done(Message::FindSimilar { id, label });
            }
            ArtistsAction::TopSongs(artist_name, label) => {
                return Task::done(Message::FindTopSongs { artist_name, label });
            }
            ArtistsAction::ColumnVisibilityChanged(col, value) => {
                return self.persist_artists_column_visibility(col, value);
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

    /// Persist the user's artists column visibility toggle to config.toml +
    /// redb via `AppService::settings()`. The page's in-memory state was
    /// already mutated in `ArtistsPage::update`.
    pub(crate) fn persist_artists_column_visibility(
        &self,
        col: views::ArtistsColumn,
        value: bool,
    ) -> Task<Message> {
        match col {
            views::ArtistsColumn::Stars => {
                self.shell_spawn("persist_artists_show_stars", move |shell| async move {
                    shell.settings().set_artists_show_stars(value).await
                });
            }
            views::ArtistsColumn::AlbumCount => {
                self.shell_spawn("persist_artists_show_albumcount", move |shell| async move {
                    shell.settings().set_artists_show_albumcount(value).await
                });
            }
            views::ArtistsColumn::SongCount => {
                self.shell_spawn("persist_artists_show_songcount", move |shell| async move {
                    shell.settings().set_artists_show_songcount(value).await
                });
            }
            views::ArtistsColumn::Plays => {
                self.shell_spawn("persist_artists_show_plays", move |shell| async move {
                    shell.settings().set_artists_show_plays(value).await
                });
            }
            views::ArtistsColumn::Love => {
                self.shell_spawn("persist_artists_show_love", move |shell| async move {
                    shell.settings().set_artists_show_love(value).await
                });
            }
            views::ArtistsColumn::Index => {
                self.shell_spawn("persist_artists_show_index", move |shell| async move {
                    shell.settings().set_artists_show_index(value).await
                });
            }
            views::ArtistsColumn::Thumbnail => {
                self.shell_spawn("persist_artists_show_thumbnail", move |shell| async move {
                    shell.settings().set_artists_show_thumbnail(value).await
                });
            }
            views::ArtistsColumn::Select => {
                self.shell_spawn("persist_artists_show_select", move |shell| async move {
                    shell.settings().set_artists_show_select(value).await
                });
            }
        }
        Task::none()
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
