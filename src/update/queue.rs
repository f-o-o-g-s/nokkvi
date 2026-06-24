//! Queue data loading and component message handlers

use std::collections::HashSet;

use iced::Task;
use nokkvi_data::{
    backend::queue::QueueSongUIViewData,
    types::{ItemKind, queue::MoveBatchTarget, queue_sort_mode::QueueSortMode},
};
use tracing::{debug, error, trace};

use super::components::{passive_artwork_version, prefetch_album_artwork_tasks};
use crate::{
    Nokkvi, View,
    app_message::{
        ArtworkMessage, FindMessage, Message, NavigationMessage, PlaybackMessage, SplitViewMessage,
    },
    views::{self, QueueAction, QueueMessage},
    widgets::SlotListPageMessage,
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
            |result| Message::QueueLoader(crate::app_message::QueueLoaderMessage::Loaded(result)),
        )
    }

    pub(crate) fn handle_queue_loaded(
        &mut self,
        result: Result<Vec<QueueSongUIViewData>, String>,
    ) -> Task<Message> {
        match result {
            Ok(songs) => {
                self.library.queue_songs = songs;
                // Drop any stale multi-selection — the new contents may not
                // line up with the rows the user had selected before the
                // reload (consume-mode advance, SSE refresh, navigation).
                self.queue_page.common.slot_list.clear_multi_selection();

                // Honest sort label: an applied queue sort survives a reload
                // only while the reloaded order still matches the applied mode.
                // Every external repopulation (play album/playlist, session
                // restore, add/remove, consume advance, SSE refresh) lands here
                // with an order that may no longer match and reverts the
                // dropdown to its "Unsorted" placeholder. Demote-only —
                // `apply_queue_sort` is the sole promoter — so a queue that
                // merely coincides with a mode is never shown as applied.
                self.revalidate_queue_sorted();

                // Freeze the strip quad identity on the FIRST queue that
                // arrives for the active playlist context (PlayPlaylist
                // clears the snapshot; a restored session boots with it
                // empty) — at that moment queue order == playlist track
                // order. Later reloads (consume advance, sort, SSE) leave
                // the frozen ids untouched.
                if self.active_playlist_info.is_some() && self.strip_quad_album_ids.is_empty() {
                    self.snapshot_strip_quad_ids();
                }

                // Load artwork for queue songs using canonical prefetch
                let mut tasks: Vec<Task<Message>> = Vec::new();

                if let Some(shell) = &self.app_service {
                    let total = self.library.queue_songs.len();
                    if total > 0 {
                        // Mini artwork prefetch using canonical helper
                        let cached: HashSet<&String> =
                            self.artwork.album_art.iter().map(|(k, _)| k).collect();
                        tasks.extend(prefetch_album_artwork_tasks(
                            &self.queue_page.common.slot_list,
                            &self.library.queue_songs,
                            &cached,
                            &self.artwork.album_art_versions,
                            &self.artwork.failed_art,
                            shell.albums().clone(),
                            |song| {
                                (
                                    song.album_id.clone(),
                                    passive_artwork_version(&song.updated_at),
                                    song.artwork_url.clone(),
                                )
                            },
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

                // Warm the "Playing From" strip's frozen quad tiles — the
                // restored viewport may sit far past the queue head, so the
                // visible-row prefetch above can miss exactly these albums.
                tasks.extend(self.strip_quad_prefetch_tasks());

                // Always clamp queue slot list offset to valid range after queue data changes.
                // When the queue is replaced (e.g. playing an album), the old offset may
                // exceed the new queue length, causing all slot list slots to render empty.
                let queue_len = self.library.queue_songs.len();
                if queue_len > 0 && self.queue_page.common.slot_list.viewport_offset >= queue_len {
                    self.queue_page.common.slot_list.viewport_offset = queue_len.saturating_sub(1);
                }

                // Focus slot list on current playing row by per-row entry_id
                // (only when auto_follow_playing is ON). entry_id was captured
                // alongside current_index in PlaybackStateUpdate under the qm
                // lock, so it identifies the right duplicate even across the
                // post-reload re-stamp.
                if self.settings.auto_follow_playing
                    && self.current_view == View::Queue
                    && let Some(entry_id) = self.last_queue_current_entry_id
                {
                    tasks.push(Task::done(Message::Queue(
                        views::QueueMessage::FocusCurrentPlaying(entry_id, false),
                    )));
                }

                if !tasks.is_empty() {
                    return Task::batch(tasks);
                }
            }
            Err(e) => {
                if nokkvi_data::types::error::NokkviError::is_unauthorized_str(&e) {
                    return self.handle_session_expired();
                }
                error!("Error loading queue: {}", e);
                self.toast_error("Failed to load queue");
            }
        }
        Task::none()
    }

    pub(crate) fn handle_queue(&mut self, msg: views::QueueMessage) -> Task<Message> {
        if let Some(task) = crate::update::dispatch_view_chrome(self, &msg, crate::View::Queue) {
            return task;
        }
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
        if let QueueMessage::SlotList(SlotListPageMessage::ScrollSeek(offset)) = &msg {
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
            return Task::batch([
                self.scrollbar_fade_timer(View::Queue),
                self.seek_settled_timer(View::Queue),
            ]);
        }

        // Keep slot_count in sync with the rendered slot list so drag index
        // translation uses the correct effective_center. Routes through the
        // vertical-aware resync so the queue's stored slot_count matches the
        // actual rendered count even when artwork is stacked above the list.
        self.resync_slot_counts();

        // ── Fast path for slot hover ──
        // The slot list republishes `HoverEnterSlot` on EVERY `CursorMoved`
        // while the cursor sits inside a row (`slot_list.rs` `on_move`). Hover
        // never moves `viewport_offset`, and `prefetch_indices` is centered
        // solely on the offset, so the prefetch window on a hover frame is
        // identical to the previous non-hover frame's — re-running the tail
        // below would (a) pay an O(n) `filter_queue_songs().into_owned()` clone
        // plus a fresh batch of `Task::perform`s per cursor pixel and (b) thrash
        // the version-aware mini-thumbnail dedup for a single-album queue
        // (album_id-keyed cache fed per-song `updated_at`), re-`put`ting the
        // `album_art` handle with a new `Id::unique()` texture for identical
        // bytes → visible flicker. Mirror the shared `hovered_slot` bookkeeping
        // (`views/mod.rs`) so cross-pane drag still tracks the row, then return.
        match &msg {
            QueueMessage::SlotList(SlotListPageMessage::HoverEnterSlot(h)) => {
                self.queue_page.common.slot_list.hovered_slot = Some(*h);
                return Task::none();
            }
            QueueMessage::SlotList(SlotListPageMessage::HoverExitSlot(h)) => {
                if self.queue_page.common.slot_list.hovered_slot == Some(*h) {
                    self.queue_page.common.slot_list.hovered_slot = None;
                }
                return Task::none();
            }
            _ => {}
        }

        // IMPORTANT: Use filtered queue for all operations since slot list indices are relative to filtered list.
        // `.into_owned()` is required here because this mutable handler needs to mutate `self` later.
        // The zero-cost `Cow::Borrowed` path benefits the render loop in `app_view.rs`, not here.
        let mut filtered_queue = self.filter_queue_songs().into_owned();
        let (cmd, action) = self.queue_page.update(msg, &filtered_queue);

        match action {
            QueueAction::PlaySong(index) => {
                // Guard: block during playlist edit mode + transition radio → queue.
                // Deliberately omit `enter_new_playback_context()` — this path only
                // moves the current-track pointer; queue contents (and the loaded
                // playlist header) must survive.
                if let Some(task) = self.guard_play_action() {
                    return task;
                }

                // Look up from FILTERED list since the slot list index is relative to filtered results
                if let Some(song) = filtered_queue.get(index) {
                    debug!(
                        "🎵 Playing song from queue: {} - {} (filtered index: {})",
                        song.title, song.artist, index
                    );

                    // Suppress auto-center for this track change — the user already
                    // sees the item they clicked/activated in the slot list.
                    self.suppress_next_auto_center = true;

                    // Per-row entry_id is drift-immune across the optimistic-
                    // mutation window AND disambiguates duplicate tracks (the
                    // legacy track_number-1 dance did the latter but not the
                    // former — a stale projection silently picked the wrong row).
                    let entry_id = song.entry_id;
                    return self.shell_task(
                        move |shell| async move { shell.play_entry_from_queue(entry_id).await },
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
                if matches!(sort_mode, QueueSortMode::Random) {
                    return self.dispatch_random_queue_shuffle();
                }
                let ascending = self.queue_page.common.sort_ascending;
                filtered_queue = self.apply_queue_sort(sort_mode, ascending).into_owned();
            }
            QueueAction::SortOrderChanged(ascending) => {
                debug!(
                    "🔄 Queue sort order changed to: {}",
                    if ascending { "ASC" } else { "DESC" }
                );
                let sort_mode = self.queue_page.queue_sort_mode;
                if matches!(sort_mode, QueueSortMode::Random) {
                    // Random treats the order toggle as a re-shuffle trigger,
                    // mirroring how library views refresh their random sort.
                    return self.dispatch_random_queue_shuffle();
                }
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
            QueueAction::FocusOnSong(entry_id, flash) => {
                // Find the row in the FILTERED list by its per-row entry_id —
                // drift-immune across the optimistic-mutation window where
                // `track_number` would still carry stale stamps from the
                // pre-mutation projection.
                if let Some(idx) = filtered_queue.iter().position(|s| s.entry_id == entry_id) {
                    trace!(
                        " [FOCUS] Found entry_id {} at filtered index {}",
                        entry_id, idx
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
                    trace!(" [FOCUS] entry_id {} not found in filtered list", entry_id);
                }
            }
            QueueAction::SetRating(song_id, new_rating) => {
                let current = Self::find_current_rating(
                    &filtered_queue,
                    &song_id,
                    |s| s.id.as_str(),
                    |s| s.rating,
                );
                return self.set_item_rating_task(song_id, ItemKind::Song, new_rating, current);
            }
            QueueAction::ToggleStar(song_id, star) => {
                return self.toggle_star_with_revert_task(song_id, ItemKind::Song, star);
            }
            QueueAction::MoveItem { from, to } => {
                // `from` and `to` are indices into `library.queue_songs` (the
                // FULL queue). This is safe because the drag-reorder dispatch
                // at `views/queue/update.rs::DragReorder` blocks the action
                // entirely while search is active (filtered_queue would
                // otherwise differ from library.queue_songs and the indices
                // could not be reused). The hotkey path also guards on
                // empty search. If a future caller plumbs MoveItem from a
                // filtered context, migrate it to `entry_id` like
                // `MoveBatch` (see `move_queue_batch_by_entry_ids`) — raw
                // indices are also drift-prone across the optimistic-
                // mutation window for a second action issued before the
                // backend acks the first.
                let len = self.library.queue_songs.len();
                if from < len && to <= len && from != to {
                    let item = self.library.queue_songs.remove(from);
                    let insert_at = if from < to { to - 1 } else { to };
                    self.library.queue_songs.insert(insert_at, item);
                }

                self.shell_spawn("queue_move_item", move |shell| async move {
                    shell.move_queue_item(from, to).await
                });

                // Drag reorder mutates the local order without a reload, so
                // re-check whether it still matches the applied sort.
                self.revalidate_queue_sorted();
            }
            QueueAction::MoveBatch { indices, target } => {
                // Multi-selection drag reorder, addressed end-to-end by
                // per-row entry_id: every position lookup resolves through
                // the row's `entry_id` so a stale `track_number` projection
                // (post-optimistic-mutation) can't pick the wrong row to
                // remove or land before.
                let entry_ids: Vec<u64> = indices
                    .iter()
                    .filter_map(|&idx| filtered_queue.get(idx).map(|s| s.entry_id))
                    .collect();
                if entry_ids.is_empty() {
                    return Task::none();
                }

                // Target — either a row's entry_id, or "end of queue" if
                // the user dragged past the last filtered row.
                let target_entry_id = filtered_queue.get(target).map(|s| s.entry_id);
                let target_for_backend =
                    target_entry_id.map_or(MoveBatchTarget::End, MoveBatchTarget::AboveEntry);

                // Resolve every entry_id to its current position in
                // `library.queue_songs` so the optimistic local reorder
                // operates on the same rows the backend will. entry_id
                // lookups survive the previous batch's optimistic shift,
                // closing the rapid-drag drift window.
                let mut raw_indices_desc: Vec<usize> = entry_ids
                    .iter()
                    .filter_map(|&eid| {
                        self.library
                            .queue_songs
                            .iter()
                            .position(|s| s.entry_id == eid)
                    })
                    .collect();
                if raw_indices_desc.is_empty() {
                    return Task::none();
                }
                raw_indices_desc.sort_unstable_by(|a, b| b.cmp(a)); // descending

                let raw_target = target_entry_id
                    .and_then(|eid| {
                        self.library
                            .queue_songs
                            .iter()
                            .position(|s| s.entry_id == eid)
                    })
                    .unwrap_or(self.library.queue_songs.len());

                debug!(
                    "📦 [QUEUE] Batch move: {} items → target_eid {:?} (raw {})",
                    raw_indices_desc.len(),
                    target_entry_id,
                    raw_target,
                );

                // Optimistic local reorder.
                let mut moved = Vec::new();
                for &qi in &raw_indices_desc {
                    if qi < self.library.queue_songs.len() {
                        moved.push(self.library.queue_songs.remove(qi));
                    }
                }
                moved.reverse(); // ascending order matches insertion

                let removed_before_target = raw_indices_desc
                    .iter()
                    .filter(|&&qi| qi < raw_target)
                    .count();
                let adjusted_target = raw_target.saturating_sub(removed_before_target);
                let insert_pos = adjusted_target.min(self.library.queue_songs.len());

                for (i, song) in moved.into_iter().enumerate() {
                    self.library.queue_songs.insert(insert_pos + i, song);
                }

                self.shell_spawn("queue_move_batch", move |shell| async move {
                    shell
                        .move_queue_batch_by_entry_ids(entry_ids, target_for_backend)
                        .await
                });

                // Drag reorder mutates the local order without a reload, so
                // re-check whether it still matches the applied sort.
                self.revalidate_queue_sorted();
            }
            QueueAction::RemoveFromQueue(entry_ids) => {
                if entry_ids.is_empty() {
                    return Task::none();
                }

                let id_set: std::collections::HashSet<u64> = entry_ids.iter().copied().collect();
                let title_text = if entry_ids.len() == 1 {
                    self.library
                        .queue_songs
                        .iter()
                        .find(|s| id_set.contains(&s.entry_id))
                        .map(|s| format!("\"{}\"", s.title))
                        .unwrap_or_default()
                } else {
                    format!("{} songs", entry_ids.len())
                };

                // Optimistic local removal by per-row entry_id — duplicate
                // rows of the same song_id only lose the targeted row(s).
                self.library
                    .queue_songs
                    .retain(|s| !id_set.contains(&s.entry_id));
                self.toast_info(format!("Removed {title_text} from queue"));

                // Goes through `AppService::remove_queue_entries` so the
                // audio engine follows the queue when the playing row is
                // removed — a bare `QueueService` call would leave the
                // engine streaming the deleted track while the UI advertises
                // a different one.
                self.shell_spawn("queue_remove_batch", move |shell| async move {
                    shell.remove_queue_entries(&entry_ids).await
                });
            }
            QueueAction::PlayNext(entry_ids) => {
                if entry_ids.is_empty() {
                    return Task::none();
                }

                let id_set: std::collections::HashSet<u64> = entry_ids.iter().copied().collect();
                let title_text = if entry_ids.len() == 1 {
                    self.library
                        .queue_songs
                        .iter()
                        .find(|s| id_set.contains(&s.entry_id))
                        .map(|s| format!("\"{}\"", s.title))
                        .unwrap_or_default()
                } else {
                    format!("{} songs", entry_ids.len())
                };

                self.toast_info(format!("{title_text} will play next"));
                if self.modes.random {
                    self.toast_warn("Shuffle is on — next tracks will be random, not these");
                }

                // Skip optimistic UI for PlayNext — target slot depends on the
                // current playing index, which lives in the backend.
                self.shell_spawn("queue_play_next_batch", move |shell| async move {
                    shell.play_next_in_queue(entry_ids).await
                });
            }
            QueueAction::ShowToast(msg) => {
                self.toast_info(msg);
            }
            QueueAction::AddToPlaylist(song_ids) => {
                return self.fetch_playlists_for_add_to_playlist(song_ids);
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
                            let library_ids = shell.active_library_ids_vec();
                            let (playlists, _) = service
                                .load_playlists_with_libraries("name", "ASC", None, &library_ids)
                                .await?;
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
            QueueAction::EditPlaylist => {
                // Enter edit mode for the currently-playing playlist. Prefer the
                // freshest cached visibility from the playlists library; fall
                // back to the context's own (played / persisted) flag when the
                // playlists list hasn't loaded yet, rather than defaulting to
                // public — a private playlist must not open looking public.
                if let Some(ref ctx) = self.active_playlist_info {
                    let playlist_public = self
                        .library
                        .playlists
                        .iter()
                        .find(|p| p.id == ctx.id)
                        .map_or(ctx.public, |p| p.public);
                    return Task::done(Message::SplitView(SplitViewMessage::EnterEditMode {
                        playlist_id: ctx.id.clone(),
                        playlist_name: ctx.name.clone(),
                        playlist_comment: ctx.comment.clone(),
                        playlist_public,
                    }));
                }
            }
            QueueAction::OpenBrowsingPanel => {
                return Task::done(Message::SplitView(SplitViewMessage::ToggleBrowsingPanel));
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
                if let Some(song_id) = filtered_queue.get(index).map(|s| s.id.clone()) {
                    return self.show_song_in_folder_task(song_id);
                }
            }
            QueueAction::FindSimilar(index) => {
                if let Some(song) = filtered_queue.get(index) {
                    let id = song.id.clone();
                    let title = song.title.clone();
                    return Task::done(Message::Find(FindMessage::Similar {
                        id,
                        label: format!("Similar to: {title}"),
                    }));
                }
            }
            QueueAction::TopSongs(index) => {
                if let Some(song) = filtered_queue.get(index) {
                    let artist = song.artist.clone();
                    if !artist.is_empty() {
                        return Task::done(Message::Find(FindMessage::TopSongs {
                            artist_name: artist.clone(),
                            label: format!("Top Songs: {artist}"),
                        }));
                    }
                }
            }
            QueueAction::RefreshArtwork(album_id) => {
                return self.update(Message::Artwork(ArtworkMessage::RefreshAlbumArtwork(
                    album_id,
                )));
            }
            QueueAction::NavigateAndFilter(view, filter) => {
                return Task::done(Message::Navigation(NavigationMessage::NavigateAndFilter {
                    view,
                    filter,
                    for_browsing_pane: false,
                }));
            }
            QueueAction::NavigateAndExpandAlbum(album_id) => {
                return Task::done(Message::Navigation(NavigationMessage::Expand(
                    crate::state::PendingExpand::Album {
                        album_id,
                        for_browsing_pane: false,
                    },
                )));
            }
            QueueAction::NavigateAndExpandArtist(artist_id) => {
                return Task::done(Message::Navigation(NavigationMessage::Expand(
                    crate::state::PendingExpand::Artist {
                        artist_id,
                        for_browsing_pane: false,
                    },
                )));
            }
            QueueAction::NavigateAndExpandGenre(genre_id) => {
                return Task::done(Message::Navigation(NavigationMessage::Expand(
                    crate::state::PendingExpand::Genre {
                        genre_id,
                        for_browsing_pane: false,
                    },
                )));
            }
            QueueAction::ColumnVisibilityChanged(col, value) => {
                return self.persist_column_visibility(col, value);
            }
            QueueAction::OpenDefaultPlaylistPicker => {
                return Task::done(Message::DefaultPlaylistPicker(
                    crate::widgets::default_playlist_picker::DefaultPlaylistPickerMessage::Open,
                ));
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
            let cached: HashSet<&String> = self.artwork.album_art.iter().map(|(k, _)| k).collect();
            let prefetch_tasks = prefetch_album_artwork_tasks(
                &self.queue_page.common.slot_list,
                &filtered_queue,
                &cached,
                &self.artwork.album_art_versions,
                &self.artwork.failed_art,
                shell.albums().clone(),
                |song| {
                    (
                        song.album_id.clone(),
                        passive_artwork_version(&song.updated_at),
                        song.artwork_url.clone(),
                    )
                },
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
            let cached: HashSet<&String> = self.artwork.album_art.iter().map(|(k, _)| k).collect();
            tasks.extend(prefetch_album_artwork_tasks(
                &self.queue_page.common.slot_list,
                items,
                &cached,
                &self.artwork.album_art_versions,
                &self.artwork.failed_art,
                shell.albums().clone(),
                |song: &QueueSongUIViewData| {
                    (
                        song.album_id.clone(),
                        passive_artwork_version(&song.updated_at),
                        song.artwork_url.clone(),
                    )
                },
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
            let cached: HashSet<&String> = self.artwork.album_art.iter().map(|(k, _)| k).collect();
            tasks.extend(prefetch_album_artwork_tasks(
                &self.queue_page.common.slot_list,
                &filtered,
                &cached,
                &self.artwork.album_art_versions,
                &self.artwork.failed_art,
                shell.albums().clone(),
                |song| {
                    (
                        song.album_id.clone(),
                        passive_artwork_version(&song.updated_at),
                        song.artwork_url.clone(),
                    )
                },
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
    ///
    /// Callers must route `QueueSortMode::Random` to
    /// `dispatch_random_queue_shuffle` instead — this path's UI sort + backend
    /// sort would each draw their own RNG and produce diverging orders.
    pub(crate) fn apply_queue_sort(
        &mut self,
        sort_mode: QueueSortMode,
        ascending: bool,
    ) -> std::borrow::Cow<'_, [QueueSongUIViewData]> {
        // Drop any multi-selection — the in-place reorder leaves the indices
        // pointing at different songs.
        self.queue_page.common.slot_list.clear_multi_selection();
        // This is the sole promoter of the "sorted" state — the dropdown now
        // shows the applied mode instead of the "Unsorted" placeholder.
        self.queue_page.queue_sorted = true;
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
        } else if !filtered.is_empty() {
            // Playing song not in filtered results — clamp to start
            self.queue_page
                .common
                .slot_list
                .set_offset(0, filtered.len());
        }
        // Physically reorder backend queue so next/prev follows sorted order.
        // `AppService::sort_queue` bundles the mutation, the reactive refresh,
        // the engine gapless-prep reset, and the settings persist.
        self.shell_spawn("sort_backend_queue", move |shell| async move {
            shell.sort_queue(sort_mode, ascending).await
        });
        std::borrow::Cow::Owned(filtered)
    }

    /// Re-shuffle the queue via the backend and reload the UI from the
    /// freshly-shuffled order.
    ///
    /// `Random` is a refresh-style sort: re-selecting it (or toggling the
    /// order button while it's the active mode) re-shuffles. Implemented
    /// backend-first + `LoadQueue` rather than the synchronous-UI-sort path
    /// in `apply_queue_sort` because each side draws its own RNG, so doing
    /// both would produce diverging orders. The new mode is intentionally
    /// not persisted to `config.toml` — the previously-saved deterministic
    /// mode survives a relaunch.
    pub(crate) fn dispatch_random_queue_shuffle(&mut self) -> Task<Message> {
        // Drop multi-selection — indices won't survive the reorder.
        self.queue_page.common.slot_list.clear_multi_selection();
        // The cached signature was keyed against the previous deterministic
        // mode; clear it so the next deterministic pick actually re-sorts
        // the now-randomized list.
        self.queue_page.last_sort_signature = None;
        // A randomized order has no verifiable sort — show "Unsorted" rather
        // than a stale deterministic mode (Random is never persisted either).
        self.queue_page.queue_sorted = false;

        self.shell_task(
            |shell| async move {
                shell.shuffle_queue_randomly().await?;
                Ok::<_, anyhow::Error>(shell.queue().get_songs())
            },
            |result| {
                Message::QueueLoader(crate::app_message::QueueLoaderMessage::Loaded(
                    result.map_err(|e| e.to_string()),
                ))
            },
        )
    }

    /// Routes `Message::QueueLoader(...)` arrivals to the existing
    /// `handle_queue_loaded` handler. Queue is single-shot (the queue *is*
    /// the entire dataset, not paged), so there's only one variant — but
    /// keeping the dispatcher's match shape mirrors the paged domains' and
    /// keeps the per-domain template uniform.
    pub(crate) fn dispatch_queue_loader(
        &mut self,
        msg: crate::app_message::QueueLoaderMessage,
    ) -> Task<Message> {
        use crate::app_message::QueueLoaderMessage;
        match msg {
            QueueLoaderMessage::Loaded(result) => self.handle_queue_loaded(result),
        }
    }
}
