//! Queue data loading and component message handlers

use std::collections::HashSet;

use iced::Task;
use nokkvi_data::{backend::queue::QueueSongUIViewData, types::queue_sort_mode::QueueSortMode};
use tracing::{debug, error, trace};

use super::components::prefetch_album_artwork_tasks;
use crate::{
    Nokkvi, View,
    app_message::{ArtworkMessage, HotkeyMessage, Message, PlaybackMessage},
    views::{self, QueueAction, QueueMessage},
};

impl Nokkvi {
    pub(crate) fn handle_load_queue(&mut self) -> Task<Message> {
        self.shell_task(
            |shell| async move {
                let queue_vm = shell.queue().clone();
                match queue_vm.refresh_from_queue().await {
                    Ok(_) => Ok(queue_vm.get_songs()),
                    Err(e) => Err(e.to_string()),
                }
            },
            |result| Message::Queue(views::QueueMessage::QueueLoaded(result)),
        )
    }

    pub(crate) fn handle_queue_loaded(
        &mut self,
        result: Result<Vec<QueueSongUIViewData>, String>,
    ) -> Task<Message> {
        match result {
            Ok(songs) => {
                self.library.queue_songs = songs;

                // Load artwork for queue songs using canonical prefetch
                let mut tasks: Vec<Task<Message>> = Vec::new();

                if let Some(shell) = &self.app_service {
                    let total = self.library.queue_songs.len();
                    if total > 0 {
                        // Mini artwork prefetch using canonical helper
                        let cached: HashSet<&String> = self.artwork.album_art.keys().collect();
                        tasks.extend(prefetch_album_artwork_tasks(
                            &self.queue_page.common.slot_list,
                            &self.library.queue_songs,
                            &cached,
                            shell.albums().clone(),
                            |song| (song.album_id.clone(), song.artwork_url.clone()),
                        ));

                        // Load large artwork for center song
                        if let Some(center_idx) = self
                            .queue_page
                            .common
                            .slot_list
                            .get_center_item_index(total)
                            && let Some(song) = self.library.queue_songs.get(center_idx)
                        {
                            tasks.push(Task::done(Message::Artwork(ArtworkMessage::LoadLarge(
                                song.album_id.clone(),
                            ))));
                        }
                    }
                }

                // Always clamp queue slot list offset to valid range after queue data changes.
                // When the queue is replaced (e.g. playing an album), the old offset may
                // exceed the new queue length, causing all slot list slots to render empty.
                let queue_len = self.library.queue_songs.len();
                if queue_len > 0 && self.queue_page.common.slot_list.viewport_offset >= queue_len {
                    self.queue_page.common.slot_list.viewport_offset = queue_len.saturating_sub(1);
                }

                // Focus slot list on current playing track if on Queue view
                // (only when auto_follow_playing is ON)
                if self.auto_follow_playing
                    && self.current_view == View::Queue
                    && let Some(queue_index) = self.last_queue_current_index
                {
                    tasks.push(Task::done(Message::Queue(
                        views::QueueMessage::FocusCurrentPlaying(queue_index, false),
                    )));
                }

                if !tasks.is_empty() {
                    return Task::batch(tasks);
                }
            }
            Err(e) => {
                error!("Error loading queue: {}", e);
                self.toast_error("Failed to load queue");
            }
        }
        Task::none()
    }

    pub(crate) fn handle_queue(&mut self, msg: views::QueueMessage) -> Task<Message> {
        // ── Fast path for scrollbar seek ──
        // During a scrollbar drag, CursorMoved fires on_seek hundreds of times
        // per second. The normal path clones the entire queue (O(n)) for
        // filter_queue_songs(), builds a HashSet for artwork prefetch, and spawns
        // timer tasks — all per event. With 12k items this starves the main
        // thread and can lock the system.
        //
        // The seek handler only needs to move the viewport offset, so we
        // short-circuit here: move the offset, record the scroll for the
        // scrollbar fade animation, and return immediately.
        if let QueueMessage::SlotListScrollSeek(offset) = &msg {
            let total = if self.queue_page.common.search_query.is_empty() {
                self.library.queue_songs.len()
            } else {
                self.filter_queue_songs().len()
            };
            self.queue_page.common.handle_set_offset(*offset, total);
            self.queue_page.common.slot_list.record_scroll();
            // Spawn two lightweight timers (no O(n) work, just gen_id guards):
            // 1. Fade timer (1.5s) — hides the scrollbar after drag ends
            // 2. Seek-settled timer (150ms) — loads artwork for final viewport
            return Task::batch([self.scrollbar_fade_timer(), self.seek_settled_timer()]);
        }

        self.play_view_sfx(
            matches!(
                msg,
                QueueMessage::SlotListNavigateUp | QueueMessage::SlotListNavigateDown
            ),
            false,
        );

        // Keep slot_count in sync with the rendered slot list so drag index
        // translation uses the correct effective_center.
        use crate::widgets::slot_list::{SlotListConfig, chrome_height_with_header};
        let config =
            SlotListConfig::with_dynamic_slots(self.window.height, chrome_height_with_header());
        self.queue_page.common.slot_list.slot_count = config.slot_count;

        // IMPORTANT: Use filtered queue for all operations since slot list indices are relative to filtered list.
        // `.into_owned()` is required here because this mutable handler needs to mutate `self` later.
        // The zero-cost `Cow::Borrowed` path benefits the render loop in `app_view.rs`, not here.
        let mut filtered_queue = self.filter_queue_songs().into_owned();
        let (cmd, action) = self.queue_page.update(msg, &filtered_queue);

        match action {
            QueueAction::PlaySong(index) => {
                // Look up from FILTERED list since the slot list index is relative to filtered results
                if let Some(song) = filtered_queue.get(index) {
                    debug!(
                        "🎵 Playing song from queue: {} - {} (filtered index: {})",
                        song.title, song.artist, index
                    );

                    // Suppress auto-center for this track change — the user already
                    // sees the item they clicked/activated in the slot list.
                    self.suppress_next_auto_center = true;

                    // Use track_number (1-based original queue position) to get the
                    // actual queue index, avoiding the index_of first-match bug
                    // with duplicate tracks.
                    let queue_index = song.track_number as usize - 1;
                    let song_id = song.id.clone();
                    return self.shell_task(
                        move |shell| async move {
                            shell.play_song_from_queue(&song_id, queue_index).await
                        },
                        |result| match result {
                            Ok(()) => Message::Playback(PlaybackMessage::Tick), // Trigger immediate UI update
                            Err(e) => {
                                error!(" Failed to play song from queue: {}", e);
                                Message::Toast(crate::app_message::ToastMessage::Push(
                                    nokkvi_data::types::toast::Toast::new(
                                        format!("Failed to play song: {e}"),
                                        nokkvi_data::types::toast::ToastLevel::Error,
                                    ),
                                ))
                            }
                        },
                    );
                }
            }
            QueueAction::SortModeChanged(sort_mode) => {
                debug!(" Queue sort mode changed to: {:?}", sort_mode);
                let ascending = self.queue_page.common.sort_ascending;
                filtered_queue = self.apply_queue_sort(sort_mode, ascending).into_owned();
            }
            QueueAction::SortOrderChanged(ascending) => {
                debug!(
                    "🔄 Queue sort order changed to: {}",
                    if ascending { "ASC" } else { "DESC" }
                );
                let sort_mode = self.queue_page.queue_sort_mode;
                filtered_queue = self.apply_queue_sort(sort_mode, ascending).into_owned();
            }
            QueueAction::SearchChanged(_query) => {
                // NOTE: Don't set search_input_focused or refocus here - text_input manages its own focus.
                // Setting flag causes race conditions with Escape (text_input captures it, flag stays stale)
                // Re-filter with updated search query for artwork prefetching
                filtered_queue = self.filter_queue_songs().into_owned();
                // Reset slot list offset to 0 for the new filtered count
                self.queue_page
                    .common
                    .slot_list
                    .set_offset(0, filtered_queue.len());
            }
            QueueAction::FocusOnSong(queue_index, flash) => {
                // Find the song in the FILTERED list by its original queue position
                // (track_number is 1-based, queue_index is 0-based)
                let target_track_number = queue_index as i32 + 1;
                if let Some(idx) = filtered_queue
                    .iter()
                    .position(|s| s.track_number == target_track_number)
                {
                    trace!(
                        " [FOCUS] Found queue_index {} at filtered index {}",
                        queue_index, idx
                    );
                    self.queue_page
                        .common
                        .slot_list
                        .set_offset(idx, filtered_queue.len());
                    // Only flash for active user actions (track change, MPRIS).
                    // Suppress for passive callers (view switch, queue reload)
                    // and during progressive queue loading.
                    if flash && self.library.queue_loading_target.is_none() {
                        self.queue_page.common.slot_list.flash_center();
                    }
                } else {
                    trace!(
                        " [FOCUS] Queue index {} not found in filtered list",
                        queue_index
                    );
                }
            }
            QueueAction::ShuffleQueue => {
                debug!(" Queue shuffle action bubbled up");
                return Task::done(Message::Hotkey(HotkeyMessage::ShuffleQueue));
            }
            QueueAction::SetRating(song_id, new_rating) => {
                let current = filtered_queue
                    .iter()
                    .find(|s| s.id == song_id)
                    .and_then(|s| s.rating)
                    .unwrap_or(0);
                return self.set_item_rating_task(song_id, "song", new_rating, current);
            }
            QueueAction::ToggleStar(song_id, star) => {
                let optimistic_msg = Self::starred_revert_message(song_id.clone(), "song", star);
                return Task::batch(vec![
                    Task::done(optimistic_msg),
                    self.star_item_task(song_id, "song", star),
                ]);
            }
            QueueAction::MoveItem { from, to } => {
                // Optimistic local reorder so the UI updates instantly
                let len = self.library.queue_songs.len();
                if from < len && to <= len && from != to {
                    let item = self.library.queue_songs.remove(from);
                    let insert_at = if from < to { to - 1 } else { to };
                    self.library.queue_songs.insert(insert_at, item);

                    // Update current_index tracking in the queue page
                    // (the backend handles its own current_index in QueueManager)
                }

                // Persist to backend and reload queue state
                self.shell_spawn("queue_move_item", move |shell| async move {
                    shell.queue().move_item(from, to).await?;
                    shell.queue().refresh_from_queue().await
                });
            }
            QueueAction::RemoveFromQueue(index) => {
                // Map filtered index → real queue index via track_number
                if let Some(song) = filtered_queue.get(index) {
                    let queue_index = song.track_number as usize - 1;
                    let song_title = song.title.clone();
                    // Optimistic local removal using real queue index
                    if queue_index < self.library.queue_songs.len() {
                        self.library.queue_songs.remove(queue_index);
                    }
                    self.toast_info(format!("Removed \"{song_title}\" from queue"));
                    self.shell_spawn("queue_remove", move |shell| async move {
                        shell.queue().remove_song(queue_index).await?;
                        shell.queue().refresh_from_queue().await
                    });
                }
            }
            QueueAction::MoveToTop(index) => {
                // Map filtered index → real queue index via track_number
                if let Some(song) = filtered_queue.get(index) {
                    let queue_index = song.track_number as usize - 1;
                    // Optimistic local move using real queue index
                    let len = self.library.queue_songs.len();
                    if queue_index > 0 && queue_index < len {
                        let item = self.library.queue_songs.remove(queue_index);
                        self.library.queue_songs.insert(0, item);
                    }
                    self.shell_spawn("queue_move_top", move |shell| async move {
                        shell.queue().move_to_top(queue_index).await?;
                        shell.queue().refresh_from_queue().await
                    });
                }
            }
            QueueAction::MoveToBottom(index) => {
                // Map filtered index → real queue index via track_number
                if let Some(song) = filtered_queue.get(index) {
                    let queue_index = song.track_number as usize - 1;
                    // Optimistic local move using real queue index
                    let len = self.library.queue_songs.len();
                    if queue_index < len {
                        let item = self.library.queue_songs.remove(queue_index);
                        self.library.queue_songs.push(item);
                    }
                    self.shell_spawn("queue_move_bottom", move |shell| async move {
                        shell.queue().move_to_bottom(queue_index).await?;
                        shell.queue().refresh_from_queue().await
                    });
                }
            }
            QueueAction::PlayNext(index) => {
                // Map filtered index → real queue index via track_number
                if let Some(song) = filtered_queue.get(index) {
                    let queue_index = song.track_number as usize - 1;
                    let song_title = song.title.clone();
                    self.toast_info(format!("\"{song_title}\" will play next"));
                    if self.modes.random {
                        self.toast_warn("Shuffle is on — next track will be random, not this one");
                    }
                    self.shell_spawn("queue_play_next", move |shell| async move {
                        let current_idx = shell.queue().current_index().await;
                        let target = current_idx.map_or(0, |i| i + 1);
                        // Only move if not already at the target position
                        if queue_index != target {
                            shell.queue().move_item(queue_index, target).await?;
                        }
                        shell.queue().refresh_from_queue().await
                    });
                }
            }
            QueueAction::ShowToast(msg) => {
                self.toast_info(msg);
            }
            QueueAction::AddToPlaylist(song_id) => {
                return self.fetch_playlists_for_add_to_playlist(vec![song_id]);
            }
            QueueAction::SaveAsPlaylist => {
                if self.library.queue_songs.is_empty() {
                    self.toast_warn("Queue is empty");
                } else {
                    // Fetch all playlists from server before opening the dialog.
                    // library.playlists may not be populated if the user hasn't
                    // visited the Playlists view yet.
                    return self.shell_task(
                        |shell| async move {
                            let service = shell.playlists_api().await?;
                            let (playlists, _) =
                                service.load_playlists("name", "ASC", None).await?;
                            Ok(playlists
                                .into_iter()
                                .map(|p| (p.id, p.name))
                                .collect::<Vec<_>>())
                        },
                        |result: Result<Vec<(String, String)>, anyhow::Error>| match result {
                            Ok(playlists) => Message::PlaylistsFetchedForDialog(playlists),
                            Err(e) => {
                                tracing::error!("Failed to fetch playlists for dialog: {e}");
                                Message::Toast(crate::app_message::ToastMessage::Push(
                                    nokkvi_data::types::toast::Toast::new(
                                        format!("Failed to load playlists: {e}"),
                                        nokkvi_data::types::toast::ToastLevel::Error,
                                    ),
                                ))
                            }
                        },
                    );
                }
            }
            QueueAction::SavePlaylist => {
                return Task::done(Message::SavePlaylistEdits);
            }
            QueueAction::DiscardEdits => {
                return Task::done(Message::ExitPlaylistEditMode);
            }
            QueueAction::PlaylistNameChanged(name) => {
                if let Some(edit_state) = &mut self.playlist_edit {
                    edit_state.set_name(name);
                }
            }
            QueueAction::EditPlaylist => {
                // Enter edit mode for the currently-playing playlist
                if let Some((playlist_id, playlist_name)) = self.active_playlist_info.clone() {
                    return Task::done(Message::EnterPlaylistEditMode {
                        playlist_id,
                        playlist_name,
                    });
                }
            }
            QueueAction::OpenBrowsingPanel => {
                return Task::done(Message::ToggleBrowsingPanel);
            }
            QueueAction::ShowInfo(index) => {
                // Fetch fresh Song data from the API to ensure full field coverage.
                // QueueManager may hold stale Song structs (persisted before new fields
                // like tags, compilation, etc. were added).
                if let Some(song_id) = filtered_queue.get(index).map(|s| s.id.clone()) {
                    return self.shell_task(
                        move |shell| async move {
                            let api = shell.songs_api().await?;
                            let song = api.load_song_by_id(&song_id).await?;
                            Ok(nokkvi_data::types::info_modal::InfoModalItem::from_song(
                                &song,
                            ))
                        },
                        |result: Result<
                            nokkvi_data::types::info_modal::InfoModalItem,
                            anyhow::Error,
                        >| match result {
                            Ok(item) => Message::InfoModal(
                                crate::widgets::info_modal::InfoModalMessage::Open(Box::new(item)),
                            ),
                            Err(e) => {
                                tracing::error!("Failed to load song info: {e}");
                                Message::Toast(crate::app_message::ToastMessage::Push(
                                    nokkvi_data::types::toast::Toast::new(
                                        format!("Failed to load song info: {e}"),
                                        nokkvi_data::types::toast::ToastLevel::Error,
                                    ),
                                ))
                            }
                        },
                    );
                }
            }
            QueueAction::ShowInFolder(index) => {
                // QueueSongUIViewData doesn't carry the file path, so fetch it
                // from the API using the same pattern as ShowInfo.
                if let Some(song_id) = filtered_queue.get(index).map(|s| s.id.clone()) {
                    return self.shell_task(
                        move |shell| async move {
                            let api = shell.songs_api().await?;
                            let song = api.load_song_by_id(&song_id).await?;
                            Ok(song.path)
                        },
                        |result: Result<String, anyhow::Error>| match result {
                            Ok(path) => Message::ShowInFolder(path),
                            Err(e) => {
                                tracing::error!("Failed to load song path: {e}");
                                Message::Toast(crate::app_message::ToastMessage::Push(
                                    nokkvi_data::types::toast::Toast::new(
                                        format!("Failed to load song path: {e}"),
                                        nokkvi_data::types::toast::ToastLevel::Error,
                                    ),
                                ))
                            }
                        },
                    );
                }
            }
            QueueAction::RefreshArtwork(album_id) => {
                return self.update(Message::Artwork(ArtworkMessage::RefreshAlbumArtwork(
                    album_id,
                )));
            }
            QueueAction::None => {}
        }

        // Load artwork from network for visible queue slots after any slot list change
        // Use filtered queue for artwork prefetching since that's what's displayed
        let total = filtered_queue.len();
        let mut tasks: Vec<Task<Message>> = Vec::new();

        if total > 0
            && let Some(shell) = &self.app_service
        {
            // Prefetch mini artwork using canonical helper
            let cached: HashSet<&String> = self.artwork.album_art.keys().collect();
            let prefetch_tasks = prefetch_album_artwork_tasks(
                &self.queue_page.common.slot_list,
                &filtered_queue,
                &cached,
                shell.albums().clone(),
                |song| (song.album_id.clone(), song.artwork_url.clone()),
            );
            tasks.extend(prefetch_tasks);

            // Load large artwork for center song
            if let Some(center_idx) = self
                .queue_page
                .common
                .slot_list
                .get_center_item_index(total)
                && let Some(song) = filtered_queue.get(center_idx)
                && self.artwork.large_artwork.peek(&song.album_id).is_none()
            {
                tasks.push(Task::done(Message::Artwork(ArtworkMessage::LoadLarge(
                    song.album_id.clone(),
                ))));
            }
        }

        // Execute artwork loading tasks in parallel with the command
        if !tasks.is_empty() {
            return Task::batch(vec![cmd.map(Message::Queue), Task::batch(tasks)]);
        }

        cmd.map(Message::Queue)
    }

    /// Load artwork for the current queue viewport. Called by `SeekSettled`
    /// after a scrollbar drag settles, avoiding the per-event O(n) clone.
    pub(crate) fn load_queue_viewport_artwork(&mut self) -> Task<Message> {
        let items: &[_] = if self.queue_page.common.search_query.is_empty() {
            // Borrow directly — no clone needed
            &self.library.queue_songs as &[_]
        } else {
            // Search is active: we must filter once (rare during seek)
            // Store in a temporary to extend lifetime
            return self.load_queue_viewport_artwork_filtered();
        };

        let total = items.len();
        let mut tasks: Vec<Task<Message>> = Vec::new();

        if total > 0
            && let Some(shell) = &self.app_service
        {
            let cached: HashSet<&String> = self.artwork.album_art.keys().collect();
            tasks.extend(prefetch_album_artwork_tasks(
                &self.queue_page.common.slot_list,
                items,
                &cached,
                shell.albums().clone(),
                |song: &QueueSongUIViewData| (song.album_id.clone(), song.artwork_url.clone()),
            ));

            if let Some(center_idx) = self
                .queue_page
                .common
                .slot_list
                .get_center_item_index(total)
                && let Some(song) = items.get(center_idx)
                && self.artwork.large_artwork.peek(&song.album_id).is_none()
            {
                tasks.push(Task::done(Message::Artwork(ArtworkMessage::LoadLarge(
                    song.album_id.clone(),
                ))));
            }
        }

        if tasks.is_empty() {
            Task::none()
        } else {
            Task::batch(tasks)
        }
    }

    /// Filtered variant of `load_queue_viewport_artwork` for when search is active.
    fn load_queue_viewport_artwork_filtered(&mut self) -> Task<Message> {
        let filtered = self.filter_queue_songs();
        let total = filtered.len();
        let mut tasks: Vec<Task<Message>> = Vec::new();

        if total > 0
            && let Some(shell) = &self.app_service
        {
            let cached: HashSet<&String> = self.artwork.album_art.keys().collect();
            tasks.extend(prefetch_album_artwork_tasks(
                &self.queue_page.common.slot_list,
                &filtered,
                &cached,
                shell.albums().clone(),
                |song| (song.album_id.clone(), song.artwork_url.clone()),
            ));

            if let Some(center_idx) = self
                .queue_page
                .common
                .slot_list
                .get_center_item_index(total)
                && let Some(song) = filtered.get(center_idx)
                && self.artwork.large_artwork.peek(&song.album_id).is_none()
            {
                tasks.push(Task::done(Message::Artwork(ArtworkMessage::LoadLarge(
                    song.album_id.clone(),
                ))));
            }
        }

        if tasks.is_empty() {
            Task::none()
        } else {
            Task::batch(tasks)
        }
    }

    /// Sort the queue locally, re-filter, re-center on the playing song,
    /// and dispatch a backend reorder + persist task.
    fn apply_queue_sort(
        &mut self,
        sort_mode: QueueSortMode,
        ascending: bool,
    ) -> std::borrow::Cow<'_, [QueueSongUIViewData]> {
        self.sort_queue_songs();
        let filtered = self.filter_queue_songs().into_owned();
        // Re-center on the currently playing song in the new sort order
        if let Some(song_id) = self.scrobble.current_song_id.clone()
            && let Some(idx) = filtered.iter().position(|s| s.id == song_id)
        {
            self.queue_page
                .common
                .slot_list
                .set_offset(idx, filtered.len());
        }
        // Physically reorder backend queue so next/prev follows sorted order
        self.shell_spawn("sort_backend_queue", move |shell| async move {
            let qm_arc = shell.queue().queue_manager();
            let mut qm = qm_arc.lock().await;
            qm.sort_queue(sort_mode, ascending)?;
            drop(qm);
            shell.queue().refresh_from_queue().await?;
            shell.settings().set_queue_prefs(sort_mode, ascending).await
        });
        std::borrow::Cow::Owned(filtered)
    }
}
