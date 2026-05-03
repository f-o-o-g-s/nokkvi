//! Song data loading and component message handlers

use std::collections::HashSet;

use iced::{Task, widget::image};
use nokkvi_data::backend::songs::SongUIViewData;
use tracing::{debug, error};

use super::components::{PaginatedFetch, prefetch_song_artwork_tasks};
use crate::{
    Nokkvi, View,
    app_message::{ArtworkMessage, Message},
    views::{self, HasCommonAction, SongsAction, SongsMessage},
};

impl Nokkvi {
    /// Shared paginated fetch for Songs. Used by both the initial load
    /// (`handle_load_songs`, offset 0) and follow-up page loads
    /// (`handle_songs_load_page`, offset N).
    fn load_songs_internal<M>(&mut self, offset: usize, msg_ctor: M) -> Task<Message>
    where
        M: FnOnce((Result<Vec<SongUIViewData>, String>, usize)) -> Message + Send + 'static,
    {
        let page_size = self.library_page_size.to_usize();
        // Phase 5A defensive gate — see load_albums_internal for rationale.
        if offset > 0
            && self
                .library
                .songs
                .needs_fetch(self.songs_page.common.slot_list.viewport_offset, page_size)
                .is_none()
        {
            return Task::none();
        }
        let params = PaginatedFetch::from_common(
            &self.songs_page.common,
            views::SongsPage::sort_mode_to_api_string,
            offset,
            page_size,
        );
        debug!(
            " LoadSongs: offset={}, page_size={}, view={}, sort={}, search={:?}",
            params.offset,
            params.page_size,
            params.view_str,
            params.sort_order,
            params.search_query,
        );

        self.library.songs.set_loading(true);

        self.shell_task(
            move |shell| async move {
                let songs_vm = shell.songs().clone();
                match songs_vm
                    .load_raw_songs_page(
                        Some(params.view_str),
                        Some(params.sort_order),
                        params.search_query.as_deref(),
                        params.filter.as_ref(),
                        params.offset,
                        params.page_size,
                    )
                    .await
                {
                    Ok(songs) => {
                        let ui_songs: Vec<SongUIViewData> =
                            songs.into_iter().map(SongUIViewData::from).collect();
                        (Ok(ui_songs), songs_vm.get_total_count() as usize)
                    }
                    Err(e) => (Err(format!("{e:#}")), 0),
                }
            },
            msg_ctor,
        )
    }

    pub(crate) fn handle_load_songs(
        &mut self,
        background: bool,
        anchor_id: Option<String>,
    ) -> Task<Message> {
        self.load_songs_internal(0, move |(result, total_count)| {
            Message::Songs(SongsMessage::SongsLoaded {
                result,
                total_count,
                background,
                anchor_id: anchor_id.clone(),
            })
        })
    }

    /// Load a subsequent page of songs (triggered by scroll near edge of loaded data)
    pub(crate) fn handle_songs_load_page(&mut self, offset: usize) -> Task<Message> {
        self.load_songs_internal(offset, |(result, total_count)| {
            Message::Songs(SongsMessage::SongsPageLoaded(result, total_count))
        })
    }

    /// Handle a subsequent page of songs being loaded (appends to buffer)
    pub(crate) fn handle_songs_page_loaded(
        &mut self,
        result: Result<Vec<SongUIViewData>, String>,
        total_count: usize,
    ) -> Task<Message> {
        impl_page_loaded_handler!(self, songs, "Songs", result, total_count)
    }

    pub(crate) fn handle_songs_loaded(
        &mut self,
        result: Result<Vec<SongUIViewData>, String>,
        total_count: usize,
        background: bool,
        anchor_id: Option<String>,
    ) -> Task<Message> {
        self.library.counts.songs = total_count;
        match result {
            Ok(new_songs) => {
                debug!(
                    "✅ Loaded {} songs (total in library: {})",
                    new_songs.len(),
                    total_count
                );
                self.library.songs.set_first_page(new_songs, total_count);

                if !background {
                    self.songs_page.common.slot_list.viewport_offset = 0;
                    self.songs_page.common.slot_list.selected_indices.clear();
                } else if let Some(ref id) = anchor_id {
                    let songs = &self.library.songs;
                    if let Some(new_idx) = songs.iter().position(|a| a.id == *id) {
                        self.songs_page.common.slot_list.viewport_offset = new_idx;
                    } else {
                        // Anchor not found in this page (expected with Random sort — the new
                        // first page is a different random sample). Reset rather than leaving
                        // viewport_offset pointing at whoever now occupies the old index.
                        self.songs_page.common.slot_list.viewport_offset = 0;
                    }
                    // Clear stale selected_offset: after re-ordering, the old absolute index
                    // maps to a different song and would highlight the wrong slot.
                    self.songs_page.common.slot_list.selected_offset = None;
                }
                let mut tasks: Vec<Task<Message>> = Vec::new();

                // Load artwork for visible songs using canonical prefetch
                if let Some(shell) = &self.app_service {
                    let albums_vm = shell.albums().clone();
                    let total = self.library.songs.len();
                    if total > 0 {
                        // Track already-queued album IDs to avoid duplicates
                        let mut loaded_album_ids = std::collections::HashSet::new();

                        for idx in self.songs_page.common.slot_list.prefetch_indices(total) {
                            if let Some(song) = self.library.songs.get(idx)
                                && let Some(album_id) = &song.album_id
                            {
                                // Skip if already cached or already queued for loading
                                if self.artwork.album_art.contains(album_id)
                                    || loaded_album_ids.contains(album_id)
                                {
                                    continue;
                                }
                                loaded_album_ids.insert(album_id.clone());

                                let art_id = album_id.clone();
                                let vm = albums_vm.clone();
                                tasks.push(Task::perform(
                                    async move {
                                        let bytes = vm
                                            .fetch_album_artwork(&art_id, Some(80), None)
                                            .await
                                            .ok();
                                        (art_id, bytes.map(image::Handle::from_bytes))
                                    },
                                    |(id, handle)| {
                                        Message::Artwork(ArtworkMessage::SongMiniLoaded(id, handle))
                                    },
                                ));
                            }
                        }
                    }
                }

                // Load large artwork for centered song
                if let Some(center_idx) = self
                    .songs_page
                    .common
                    .slot_list
                    .get_center_item_index(self.library.songs.len())
                    && let Some(song) = self.library.songs.get(center_idx)
                    && let Some(album_id) = &song.album_id
                {
                    tasks.push(Task::done(Message::Artwork(ArtworkMessage::LoadLarge(
                        album_id.clone(),
                    ))));
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
                if e.contains("Unauthorized") {
                    self.library.songs.set_loading(false);
                    return self.handle_session_expired();
                }
                error!("Error loading songs: {}", e);
                self.library.songs.set_loading(false);
                self.toast_error(format!("Failed to load songs: {e}"));
            }
        }
        Task::none()
    }

    pub(crate) fn handle_song_artwork_loaded(
        &mut self,
        album_id: String,
        handle: Option<image::Handle>,
    ) -> Task<Message> {
        if let Some(h) = handle {
            self.artwork.album_art.put(album_id, h);
            self.artwork.refresh_album_art_snapshot();
        }
        Task::none()
    }

    pub(crate) fn handle_songs(&mut self, msg: views::SongsMessage) -> Task<Message> {
        if let SongsMessage::SetOpenMenu(next) = msg {
            return Task::done(Message::SetOpenMenu(next));
        }
        self.play_view_sfx(
            matches!(
                msg,
                SongsMessage::SlotListNavigateUp | SongsMessage::SlotListNavigateDown
            ),
            false,
        );
        let (cmd, action) = self.songs_page.update(msg, &self.library.songs);

        // Handle common actions (SearchChanged, SortModeChanged, SortOrderChanged)
        if let Some(task) = self.handle_common_view_action(
            action.as_common(),
            Message::LoadSongs,
            "persist_songs_prefs",
            self.songs_page.common.current_sort_mode,
            self.songs_page.common.sort_ascending,
            |shell, vt, asc| async move { shell.settings().set_songs_prefs(vt, asc).await },
        ) {
            return task;
        }

        match action {
            SongsAction::PlaySongFromIndex(index) => {
                if let Some(task) = self.guard_play_action() {
                    return task;
                }
                // Browsing panel: redirect play → add to queue
                if self.browsing_panel.is_some() {
                    if let Some(song) = self.library.songs.get(index) {
                        let title = song.title.clone();
                        let song: nokkvi_data::types::song::Song = song.clone().into();
                        if let Some(pos) = self.pending_queue_insert_position.take() {
                            return self.shell_fire_and_forget_task(
                                move |shell| async move {
                                    shell.insert_song_at_position(song, pos).await
                                },
                                format!("Inserted '{title}' at position {}", pos + 1),
                                "insert song to queue",
                            );
                        }
                        return self.shell_fire_and_forget_task(
                            move |shell| async move { shell.add_song_to_queue(song).await },
                            format!("Added '{title}' to queue"),
                            "add song to queue",
                        );
                    }
                    return Task::none();
                }
                if let Some(song) = self.library.songs.get(index) {
                    debug!(" Playing song from index: {} - {}", song.title, song.artist);

                    use nokkvi_data::types::player_settings::EnterBehavior;
                    match self.enter_behavior {
                        EnterBehavior::PlaySingle => {
                            // Replace queue with just this song
                            let song: nokkvi_data::types::song::Song = song.clone().into();
                            self.clear_active_playlist();
                            let play_task = self.shell_task(
                                move |shell| async move { shell.play_songs(vec![song], 0).await },
                                |result| match result {
                                    Ok(()) => Message::SwitchView(View::Queue),
                                    Err(e) => {
                                        error!(" Failed to play song: {}", e);
                                        Message::Toast(crate::app_message::ToastMessage::Push(
                                            nokkvi_data::types::toast::Toast::new(
                                                format!("Failed to play song: {e}"),
                                                nokkvi_data::types::toast::ToastLevel::Error,
                                            ),
                                        ))
                                    }
                                },
                            );
                            return play_task;
                        }
                        EnterBehavior::AppendAndPlay => {
                            // Append to existing queue and start playing
                            let song: nokkvi_data::types::song::Song = song.clone().into();
                            let title = song.title.clone();
                            return self.shell_fire_and_forget_task(
                                move |shell| async move { shell.add_song_and_play(song).await },
                                format!("Playing '{title}'"),
                                "append and play song",
                            );
                        }
                        EnterBehavior::PlayAll => {
                            // Current behavior: replace queue with all songs
                            // CRITICAL FIX: Use the already-displayed songs list directly.
                            // Re-fetching would return a different random order for "random" sort mode,
                            // causing the wrong song to play. Convert SongUIViewData -> Song.
                            let songs: Vec<nokkvi_data::types::song::Song> = self
                                .library
                                .songs
                                .iter()
                                .cloned()
                                .map(|ui| ui.into())
                                .collect();

                            // Capture pagination state for progressive queue building
                            let loaded_count = self.library.songs.loaded_count();
                            let total_count = self.library.songs.total_count();
                            let needs_more = loaded_count < total_count;

                            // Capture sort/search params for fetching remaining pages
                            let sort_mode = views::SongsPage::sort_mode_to_api_string(
                                self.songs_page.common.current_sort_mode,
                            )
                            .to_string();
                            let sort_order = if self.songs_page.common.sort_ascending {
                                "ASC".to_string()
                            } else {
                                "DESC".to_string()
                            };
                            let search_query = if self.songs_page.common.search_query.is_empty() {
                                None
                            } else {
                                Some(self.songs_page.common.search_query.clone())
                            };

                            // Clear playlist context
                            self.clear_active_playlist();

                            // Phase 1: Play immediately with loaded songs
                            let play_task = self.shell_task(
                                move |shell| async move { shell.play_songs(songs, index).await },
                                |result| match result {
                                    Ok(()) => Message::SwitchView(View::Queue),
                                    Err(e) => {
                                        error!(" Failed to play song: {}", e);
                                        Message::Toast(crate::app_message::ToastMessage::Push(
                                            nokkvi_data::types::toast::Toast::new(
                                                format!("Failed to play song: {e}"),
                                                nokkvi_data::types::toast::ToastLevel::Error,
                                            ),
                                        ))
                                    }
                                },
                            );

                            // Phase 2: Background-fetch remaining pages and append to queue
                            if needs_more {
                                // Set loading target so queue header shows "X of Y songs"
                                self.library.queue_loading_target = Some(total_count);
                                // Increment generation so any stale chain from a previous play self-cancels
                                self.library.progressive_queue_generation += 1;
                                let generation = self.library.progressive_queue_generation;

                                debug!(
                                    "📄 Progressive queue: will fetch {} remaining songs in background (generation={})",
                                    total_count - loaded_count,
                                    generation
                                );
                                let first_page = Task::done(Message::ProgressiveQueueAppendPage {
                                    sort_mode,
                                    sort_order,
                                    search_query,
                                    offset: loaded_count,
                                    total_count,
                                    generation,
                                });
                                return Task::batch(vec![play_task, first_page]);
                            }

                            return play_task;
                        }
                    }
                }
            }
            SongsAction::AddBatchToQueue(payload) => {
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
            SongsAction::ToggleStar(song_id, star) => {
                let optimistic_msg = Self::starred_revert_message(song_id.clone(), "song", star);
                return Task::batch(vec![
                    Task::done(optimistic_msg),
                    self.star_item_task(song_id, "song", star),
                ]);
            }

            SongsAction::SetRating(song_id, new_rating) => {
                let current = self
                    .library
                    .songs
                    .iter()
                    .find(|s| s.id == song_id)
                    .and_then(|s| s.rating)
                    .unwrap_or(0);
                return self.set_item_rating_task(song_id, "song", new_rating, current);
            }
            SongsAction::LoadLargeArtwork(album_id) => {
                // Direct call (rather than dispatching Message::Artwork(LoadLarge))
                // so loading_large_artwork is set in the same tick as the action.
                let mut tasks = vec![self.handle_load_large_artwork(album_id)];

                // Prefetch mini artwork for visible viewport using canonical helper
                if let Some(shell) = &self.app_service {
                    let cached: HashSet<&String> =
                        self.artwork.album_art.iter().map(|(k, _)| k).collect();
                    let prefetch_tasks = prefetch_song_artwork_tasks(
                        &self.songs_page.common.slot_list,
                        &self.library.songs,
                        &cached,
                        shell.albums().clone(),
                        |s| s.album_id.as_ref(),
                    );
                    tasks.extend(prefetch_tasks);
                }

                // Check if we need to fetch more pages while scrolling
                let page_size = self.library_page_size.to_usize();
                if let Some((offset, _)) = self
                    .library
                    .songs
                    .needs_fetch(self.songs_page.common.slot_list.viewport_offset, page_size)
                {
                    tasks.push(self.handle_songs_load_page(offset));
                }

                return Task::batch(tasks);
            }
            SongsAction::LoadPage(offset) => {
                return self.handle_songs_load_page(offset);
            }
            SongsAction::AddBatchToPlaylist(payload) => {
                // Wait! To add to playlist we need a flat list of `song_ids`.
                // Currently `fetch_playlists_for_add_to_playlist` takes a `Vec<String>` of song_ids!
                // To cleanly integrate, we can resolve the payload strings.
                // However, resolving full batches (Artists/Albums) requires an async call before we show
                // the "Choose a playlist" dialog?
                // Let's resolve the batch *after* the user chooses a playlist, but `fetch_playlists_for_add_to_playlist`
                // just stores the IDs in `self.pending_add_to_playlist`.
                return self.handle_add_batch_to_playlist(payload);
            }
            SongsAction::PlayNextBatch(payload) => {
                debug!(" Playing batch of {} items next", payload.items.len());
                if self.modes.random {
                    self.toast_warn("Shuffle is on — next tracks will be random, not these");
                }
                return self.shell_fire_and_forget_task(
                    move |shell| async move { shell.play_next_batch(payload).await },
                    "Added batch to play next".to_string(),
                    "play next batch",
                );
            }
            SongsAction::PlayBatch(payload) => {
                let len = payload.items.len();
                debug!(" Playing batch of {} items", len);
                self.clear_active_playlist();
                self.songs_page.common.slot_list.selected_indices.clear();
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
            SongsAction::ShowInfo(item) => {
                return self.update(Message::InfoModal(
                    crate::widgets::info_modal::InfoModalMessage::Open(item),
                ));
            }
            SongsAction::ShowInFolder(path) => {
                return self.handle_show_in_folder(path);
            }
            SongsAction::RefreshArtwork(album_id) => {
                return self.update(Message::Artwork(ArtworkMessage::RefreshAlbumArtwork(
                    album_id,
                )));
            }
            SongsAction::FindSimilar(song_id, label) => {
                return self.handle_find_similar(song_id, label);
            }
            SongsAction::TopSongs(artist_name, label) => {
                return self.handle_find_top_songs(artist_name, label);
            }
            SongsAction::ColumnVisibilityChanged(col, value) => {
                return self.persist_songs_column_visibility(col, value);
            }
            _ => {} // None + already-handled common actions
        }

        cmd.map(Message::Songs)
    }

    /// Persist the user's songs column visibility toggle to config.toml +
    /// redb via `AppService::settings()`. The page's in-memory state was
    /// already mutated in `SongsPage::update`.
    pub(crate) fn persist_songs_column_visibility(
        &self,
        col: views::SongsColumn,
        value: bool,
    ) -> Task<Message> {
        match col {
            views::SongsColumn::Stars => {
                self.shell_spawn("persist_songs_show_stars", move |shell| async move {
                    shell.settings().set_songs_show_stars(value).await
                });
            }
            views::SongsColumn::Album => {
                self.shell_spawn("persist_songs_show_album", move |shell| async move {
                    shell.settings().set_songs_show_album(value).await
                });
            }
            views::SongsColumn::Duration => {
                self.shell_spawn("persist_songs_show_duration", move |shell| async move {
                    shell.settings().set_songs_show_duration(value).await
                });
            }
            views::SongsColumn::Plays => {
                self.shell_spawn("persist_songs_show_plays", move |shell| async move {
                    shell.settings().set_songs_show_plays(value).await
                });
            }
            views::SongsColumn::Love => {
                self.shell_spawn("persist_songs_show_love", move |shell| async move {
                    shell.settings().set_songs_show_love(value).await
                });
            }
            views::SongsColumn::Index => {
                self.shell_spawn("persist_songs_show_index", move |shell| async move {
                    shell.settings().set_songs_show_index(value).await
                });
            }
            views::SongsColumn::Thumbnail => {
                self.shell_spawn("persist_songs_show_thumbnail", move |shell| async move {
                    shell.settings().set_songs_show_thumbnail(value).await
                });
            }
            views::SongsColumn::Genre => {
                self.shell_spawn("persist_songs_show_genre", move |shell| async move {
                    shell.settings().set_songs_show_genre(value).await
                });
            }
            views::SongsColumn::Select => {
                self.shell_spawn("persist_songs_show_select", move |shell| async move {
                    shell.settings().set_songs_show_select(value).await
                });
            }
        }
        Task::none()
    }
}
