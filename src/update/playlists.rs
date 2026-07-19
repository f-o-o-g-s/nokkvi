//! Playlist data loading and component message handlers

use iced::Task;
use nokkvi_data::backend::playlists::PlaylistUIViewData;
use tracing::{debug, info};

use crate::{
    Nokkvi, View,
    app_message::{ArtworkMessage, CollageTarget, Message, NavigationMessage, SplitViewMessage},
    update::PlaylistsTarget,
    views::{self, HasCommonAction, PlaylistsAction, PlaylistsMessage},
};

impl Nokkvi {
    pub(crate) fn handle_load_playlists(&mut self) -> Task<Message> {
        debug!(" LoadPlaylists message received, loading playlists...");
        let view_str = views::PlaylistsPage::sort_mode_to_api_string(
            self.playlists_page.common.current_sort_mode,
        );
        let sort_ascending = self.playlists_page.common.sort_ascending;
        let search_query_clone = self.playlists_page.common.search_query.clone();

        // Mark buffer as loading to prevent duplicate fetches
        self.library.playlists.set_loading(true);

        self.shell_task(
            move |shell| async move {
                let service = match shell.playlists_api().await {
                    Ok(s) => s,
                    Err(e) => return (Err(format!("{e:#}")), 0),
                };

                let sort_order_str = if sort_ascending { "ASC" } else { "DESC" };
                let library_ids = shell.active_library_ids_vec();
                match service
                    .load_playlists_with_libraries(
                        view_str,
                        sort_order_str,
                        if search_query_clone.is_empty() {
                            None
                        } else {
                            Some(search_query_clone.as_str())
                        },
                        &library_ids,
                    )
                    .await
                {
                    Ok((playlists, total_count)) => {
                        let ui_playlists: Vec<PlaylistUIViewData> = playlists
                            .into_iter()
                            .map(PlaylistUIViewData::from)
                            .collect();
                        (Ok(ui_playlists), total_count as usize)
                    }
                    Err(e) => (Err(format!("{e:#}")), 0),
                }
            },
            |(result, total_count)| {
                Message::PlaylistsLoader(crate::app_message::PlaylistsLoaderMessage::Loaded(
                    result,
                    total_count,
                ))
            },
        )
    }

    pub(crate) fn handle_playlists_loaded(
        &mut self,
        result: Result<Vec<PlaylistUIViewData>, String>,
        total_count: usize,
    ) -> Task<Message> {
        let task = self.handle_loaded_with::<PlaylistsTarget>(result, total_count, false, None);
        // The restored banner context can be stale (persisted at last play);
        // re-sync it against the freshly loaded metadata so count / duration /
        // updated-date / visibility always reflect the server.
        self.resync_active_playlist_context();
        task
    }

    /// Refresh `active_playlist_info` from the freshly loaded playlists list.
    ///
    /// Restore seeds the banner context from persisted settings; once the
    /// authoritative playlists metadata loads, upgrade the context to it (and
    /// re-persist) so a server-side edit between sessions is reflected. No-ops
    /// when no playlist is active or the active one isn't in the loaded page.
    pub(crate) fn resync_active_playlist_context(&mut self) {
        let Some(active_id) = self.active_playlist_info.as_ref().map(|ctx| ctx.id.clone()) else {
            return;
        };
        if let Some(playlist) = self.library.playlists.iter().find(|p| p.id == active_id) {
            let fresh = crate::state::ActivePlaylistContext::from_playlist(playlist);
            if self.active_playlist_info.as_ref() != Some(&fresh) {
                self.active_playlist_info = Some(fresh);
                self.persist_active_playlist_info();
            }
        }
    }

    pub(crate) fn handle_playlists(&mut self, msg: views::PlaylistsMessage) -> Task<Message> {
        if let Some(task) = crate::update::dispatch_view_chrome(self, &msg, crate::View::Playlists)
        {
            return task;
        }
        let (cmd, action) =
            self.playlists_page
                .update(msg, self.library.playlists.len(), &self.library.playlists);

        // Handle common actions (SearchChanged, SortModeChanged, SortOrderChanged)
        if let Some(task) = self.handle_common_view_action(
            action.as_common(),
            Message::LoadPlaylists,
            "persist_playlists_prefs",
            self.playlists_page.common.current_sort_mode,
            self.playlists_page.common.sort_ascending,
            |shell, vt, asc| async move { shell.settings().set_playlists_prefs(vt, asc).await },
        ) {
            return task;
        }

        match action {
            views::PlaylistsAction::PlayPlaylist(playlist_id, force) => {
                if let Some(task) = self.guard_play_action() {
                    return task;
                }
                self.enter_new_playback_context();
                // Browsing panel: redirect play → add to queue. Playlists has
                // no cross-pane-drag insert variant today, so the redirect is
                // unconditional (we do NOT consume
                // `cross_pane_drag.pending_queue_insert_position` to preserve
                // the pre-helper behavior).
                if self.browsing_panel.is_some() {
                    let name = self
                        .library
                        .playlists
                        .iter()
                        .find(|p| p.id == playlist_id)
                        .map_or_else(|| "playlist".to_string(), |p| p.name.clone());
                    return self.shell_fire_and_forget_task(
                        move |shell| async move { shell.add_playlist_to_queue(&playlist_id).await },
                        format!("Added '{name}' to queue"),
                        "add playlist to queue",
                    );
                }
                // AppendAndPlay: append playlist songs to queue and start playing
                use nokkvi_data::types::player_settings::EnterBehavior;
                if self.settings.enter_behavior == EnterBehavior::AppendAndPlay {
                    let name = self
                        .library
                        .playlists
                        .iter()
                        .find(|p| p.id == playlist_id)
                        .map_or_else(|| "playlist".to_string(), |p| p.name.clone());
                    let shuffle = self.activate_shuffle_directive(force, false);
                    self.clear_active_playlist();
                    return self.shell_fire_and_forget_task(
                        move |shell| async move {
                            shell.add_playlist_and_play(&playlist_id, shuffle).await
                        },
                        format!("Playing '{name}'"),
                        "append playlist and play",
                    );
                }
                // PlayAll / PlaySingle: replace queue with playlist
                // Set the active playlist info for the queue header bar
                self.active_playlist_info = self
                    .library
                    .playlists
                    .iter()
                    .find(|p| p.id == playlist_id)
                    .map(crate::state::ActivePlaylistContext::from_playlist);
                self.persist_active_playlist_info();
                let shuffle = self.activate_shuffle_directive(force, false);
                return self.shell_action_task(
                    move |shell| async move { shell.play_playlist(&playlist_id, shuffle).await },
                    Message::Navigation(NavigationMessage::SwitchView(View::Queue)),
                    "play playlist",
                );
            }
            views::PlaylistsAction::PlayBatch(payload, force) => {
                self.playlists_page
                    .common
                    .slot_list
                    .clear_selection_indices_only();
                return self.play_batch_task(payload, force);
            }
            views::PlaylistsAction::AddBatchToMix(seeds) => {
                return self.add_seeds_to_mix(seeds);
            }
            views::PlaylistsAction::AddBatchToQueue(payload) => {
                return self.add_or_insert_batch_to_queue_task(payload);
            }
            views::PlaylistsAction::ExpandPlaylist(playlist_id) => {
                // Load tracks for the playlist and send them back to the view
                let id = playlist_id.clone();
                return self.expand_load_children_task(
                    move |shell| async move {
                        let playlists_service = shell.playlists_api().await?;
                        // Playlist-level attrs (OpenSubsonic readonly) are a
                        // play-flow signal, not an expansion one — drop them.
                        let (songs, _attrs) = playlists_service.load_playlist_songs(&id).await?;
                        Ok(songs)
                    },
                    move |songs: Vec<nokkvi_data::types::song::Song>| {
                        let tracks: Vec<nokkvi_data::backend::songs::SongUIViewData> =
                            songs.into_iter().map(Into::into).collect();
                        Message::Playlists(PlaylistsMessage::TracksLoaded(playlist_id, tracks))
                    },
                    "load playlist tracks",
                );
            }
            views::PlaylistsAction::PlayPlaylistFromTrack(playlist_id, track_idx, force) => {
                // Mirror the album-sibling prologue (`AlbumsAction::PlayAlbumFromTrack`):
                // guard the play (edit-mode block + radio→queue transition) then
                // enter a new playback context (clears a stale queue_loading_target).
                // Ordering is load-bearing — `enter_new_playback_context()` calls
                // `clear_active_playlist()` which NULLs `active_playlist_info`, so it
                // MUST run before the set+persist below.
                if let Some(task) = self.guard_play_action() {
                    return task;
                }
                self.enter_new_playback_context();
                // Set the active playlist info for the queue header bar
                self.active_playlist_info = self
                    .library
                    .playlists
                    .iter()
                    .find(|p| p.id == playlist_id)
                    .map(crate::state::ActivePlaylistContext::from_playlist);
                self.persist_active_playlist_info();
                let shuffle = self.activate_shuffle_directive(force, true);
                return self.shell_action_task(
                    move |shell| async move {
                        shell
                            .play_playlist_from_track(&playlist_id, track_idx, shuffle)
                            .await
                    },
                    Message::Navigation(NavigationMessage::SwitchView(View::Queue)),
                    "play playlist from track",
                );
            }

            views::PlaylistsAction::LoadArtwork(playlist_index_str) => {
                // Load artwork for all visible slot list slots using collage artwork service
                use crate::services::collage_artwork::{self, CollageArtworkContext};

                if let Ok(center_index) = playlist_index_str.parse::<usize>()
                    && let Some(shell) = &self.app_service
                {
                    let total = self.library.playlists.len();
                    if total == 0 {
                        return Task::none();
                    }

                    // Centered playlist gets the full collage fetch (mini +
                    // 9 tiles for the right-side panel); every other visible
                    // slot only renders a mini in its slot row.
                    let center_id = self
                        .library
                        .playlists
                        .get(center_index)
                        .map(|p| p.id.clone());

                    let ctx = CollageArtworkContext {
                        slot_list: &self.playlists_page.common.slot_list,
                        pending_ids: &self.artwork.playlist.pending,
                        memory_artwork: &self.artwork.playlist.mini.snapshot,
                        memory_collage: &self.artwork.playlist.collage.snapshot,
                    };

                    let (pending_inserts, mut tasks) = collage_artwork::load_visible_artwork(
                        &self.library.playlists,
                        &ctx,
                        shell.auth().clone(),
                        center_id.as_deref(),
                        |a, b, c, d| {
                            Message::Artwork(ArtworkMessage::LoadCollage(
                                CollageTarget::Playlist,
                                a,
                                b,
                                c,
                                d,
                            ))
                        },
                        |a, b, c, d| {
                            Message::Artwork(ArtworkMessage::LoadCollageMini(
                                CollageTarget::Playlist,
                                a,
                                b,
                                c,
                                d,
                            ))
                        },
                    );

                    for id in pending_inserts {
                        self.artwork.playlist.pending.insert(id);
                    }

                    // Row 2×2 quads source their tiles from the shared 80px
                    // album_art cache — warm the viewport's tile ids in the
                    // same pass (dedup-gated, so warm viewports add nothing).
                    tasks.extend(self.quad_prefetch_tasks(CollageTarget::Playlist));

                    // Custom (user-uploaded) covers: warm the viewport's
                    // minis, plus the centered playlist's large panel image
                    // (the handler gates on the live uploaded_image field,
                    // so this is a no-op for collage playlists).
                    tasks.push(self.prefetch_playlist_custom_art_tasks());
                    if let Some(id) = center_id {
                        tasks.push(Task::done(Message::Artwork(
                            ArtworkMessage::LoadPlaylistCustomLarge(id),
                        )));
                    }

                    if !tasks.is_empty() {
                        return Task::batch(tasks);
                    }
                }
            }
            views::PlaylistsAction::PreloadArtwork(_viewport_offset) => {
                // Preload artwork for visible playlists around viewport
                use crate::services::collage_artwork::{self, CollageArtworkContext};

                let total = self.library.playlists.len();
                if total == 0 {
                    return Task::none();
                }

                if let Some(shell) = &self.app_service {
                    let ctx = CollageArtworkContext {
                        slot_list: &self.playlists_page.common.slot_list,
                        pending_ids: &self.artwork.playlist.pending,
                        memory_artwork: &self.artwork.playlist.mini.snapshot,
                        memory_collage: &self.artwork.playlist.collage.snapshot,
                    };

                    let (pending_inserts, task) = collage_artwork::preload_artwork(
                        &self.library.playlists,
                        &ctx,
                        shell.auth().clone(),
                        |ids, url, cred| {
                            Message::Artwork(ArtworkMessage::CollageBatchReady(
                                CollageTarget::Playlist,
                                ids,
                                url,
                                cred,
                            ))
                        },
                    );

                    // Mark items as pending
                    for id in pending_inserts {
                        self.artwork.playlist.pending.insert(id);
                    }

                    if let Some(task) = task {
                        return task;
                    }
                }
            }
            PlaylistsAction::ToggleStar(item_id, kind, star) => {
                return self.toggle_star_with_revert_task(item_id, kind, star);
            }
            PlaylistsAction::PlayNextBatch(payload) => {
                return self.play_next_batch_task(payload);
            }
            PlaylistsAction::AddBatchToPlaylist(payload) => {
                return self.handle_add_batch_to_playlist(payload);
            }
            PlaylistsAction::RemoveTrackFromPlaylist {
                playlist_id,
                song_id,
                position,
            } => {
                // Defensively built (the silent-no-op class): (i) verify-read
                // confirms the song at the 1-based position, narrowing the
                // reorder TOCTOU window; (ii) single-id DELETE — a stale
                // position 404s server-side instead of deleting silently;
                // (iii) the echoed-id check inside remove_playlist_track_at
                // treats a no-op 200 as failure; (iv) the settle handler
                // refreshes unconditionally, bounding any residual race.
                let pid = playlist_id.clone();
                return self.shell_task(
                    move |shell| async move {
                        let service = shell.playlists_api().await?;
                        let (songs, _attrs) = service.load_playlist_songs(&playlist_id).await?;
                        let at = songs.get(position.saturating_sub(1) as usize);
                        if at.map(|s| s.id.as_str()) != Some(song_id.as_str()) {
                            return Ok(false); // playlist changed under us
                        }
                        match service
                            .remove_playlist_track_at(&playlist_id, position)
                            .await
                        {
                            Ok(()) => Ok(true),
                            // A 404 is the stale-position lane (the server's
                            // len(ids)==1 ErrNotFound branch), not a hard
                            // failure — same "changed" recovery.
                            Err(e) if format!("{e:#}").contains("status 404") => Ok(false),
                            Err(e) => Err(e),
                        }
                    },
                    move |result: Result<bool, anyhow::Error>| match result {
                        Ok(removed) => Message::Playlists(PlaylistsMessage::TrackRemovalSettled {
                            playlist_id: pid.clone(),
                            removed,
                        }),
                        Err(e) => {
                            if let Some(msg) =
                                crate::update::components::session_expired_message(&e)
                            {
                                return msg;
                            }
                            tracing::error!("Failed to remove track from playlist: {e:#}");
                            Message::Toast(crate::app_message::ToastMessage::Push(
                                nokkvi_data::types::toast::Toast::new(
                                    format!("Failed to remove track: {e}"),
                                    nokkvi_data::types::toast::ToastLevel::Error,
                                ),
                            ))
                        }
                    },
                );
            }
            PlaylistsAction::TrackRemovalSettled {
                playlist_id,
                removed,
            } => {
                if removed {
                    self.toast_success("Removed from playlist");
                } else {
                    self.toast_warn("Playlist changed — refresh and retry");
                }
                // Unconditional refresh: reload the list (counts) and re-pull
                // the expansion children so the rows reflect server truth.
                let id = playlist_id.clone();
                let refetch = self.expand_load_children_task(
                    move |shell| async move {
                        let playlists_service = shell.playlists_api().await?;
                        let (songs, _attrs) = playlists_service.load_playlist_songs(&id).await?;
                        Ok(songs)
                    },
                    move |songs: Vec<nokkvi_data::types::song::Song>| {
                        let tracks: Vec<nokkvi_data::backend::songs::SongUIViewData> =
                            songs.into_iter().map(Into::into).collect();
                        Message::Playlists(PlaylistsMessage::TracksLoaded(playlist_id, tracks))
                    },
                    "reload playlist tracks",
                );
                return Task::batch([Task::done(Message::LoadPlaylists), refetch]);
            }
            PlaylistsAction::EditRules(playlist_id) => {
                return self.handle_enter_rules_mode(crate::app_message::RulesEntryTarget::Edit {
                    playlist_id,
                });
            }
            PlaylistsAction::NewSmartPlaylist => {
                return self.handle_enter_rules_mode(crate::app_message::RulesEntryTarget::Create);
            }
            PlaylistsAction::ImportNsp => {
                return self.handle_import_nsp();
            }
            PlaylistsAction::RetryCapsFetch => {
                // Re-attempt the post-auth version fetch (the dimmed entry's
                // whole purpose). Failure keeps FetchFailed → retry stays.
                if let Some(shell) = self.app_service.clone() {
                    return Task::perform(
                        async move { shell.auth().fetch_server_version().await.ok() },
                        Message::ServerVersionFetched,
                    );
                }
            }
            PlaylistsAction::DeletePlaylist(playlist_id) => {
                let row = self.library.playlists.iter().find(|p| p.id == playlist_id);
                let name = row.map_or_else(|| "playlist".to_string(), |p| p.name.clone());
                // File-backed honesty (verified against the server's scan
                // import: a deleted row leaves nothing for the path lookup
                // to find, so the file re-imports UNCONDITIONALLY — no
                // client-side action can prevent it while the file exists).
                let file_backed = row.is_some_and(|p| p.is_file_backed);
                self.text_input_dialog
                    .open_delete_confirmation(playlist_id, name, file_backed);
            }
            PlaylistsAction::RenamePlaylist(playlist_id) => {
                let row = self.library.playlists.iter().find(|p| p.id == playlist_id);
                let current_name = row.map_or_else(String::new, |p| p.name.clone());
                // File-backed truth (verified against the server's scan
                // re-sync, which PRESERVES the API-set name of a
                // path-matched playlist): an in-app rename IS durable. The
                // true residual is that a still-synced file's RULES keep
                // overwriting rule edits on every scan — stated as a dimmed
                // note, never a false resurrection warning. (The Detach
                // offer lands with M4's ServerCaps — the sync PUT is a
                // 0.62+ capability.)
                let synced_file = row.is_some_and(|p| p.is_file_backed && p.sync);
                self.text_input_dialog.open(
                    "Rename Playlist",
                    current_name,
                    "Playlist name...",
                    crate::widgets::text_input_dialog::TextInputDialogAction::RenamePlaylist(
                        playlist_id,
                    ),
                );
                if synced_file {
                    self.text_input_dialog.set_note(
                        "Renaming here won't rename the server-side file — and that \
                         file's rules keep overwriting rule edits on every scan.",
                    );
                }
            }
            PlaylistsAction::EditPlaylist(
                playlist_id,
                playlist_name,
                playlist_comment,
                playlist_public,
            ) => {
                // Smart gate (D4): the Tracks editor's saves would 403 —
                // smart rows route into the RULES session instead (owned;
                // the enter handler owns the ownership refusal).
                if self
                    .library
                    .playlists
                    .iter()
                    .any(|p| p.id == playlist_id && p.is_smart)
                {
                    return self.handle_enter_rules_mode(
                        crate::app_message::RulesEntryTarget::Edit { playlist_id },
                    );
                }
                return Task::done(Message::SplitView(SplitViewMessage::EnterEditMode {
                    playlist_id,
                    playlist_name,
                    playlist_comment,
                    playlist_public,
                }));
            }
            PlaylistsAction::ShowInfo(item) => {
                return self.update(Message::InfoModal(
                    crate::widgets::info_modal::InfoModalMessage::Open(item),
                ));
            }
            PlaylistsAction::SetCustomArtwork(playlist_id, playlist_name) => {
                return self.handle_set_playlist_artwork(playlist_id, playlist_name);
            }
            PlaylistsAction::ResetCustomArtwork(playlist_id, playlist_name) => {
                return self.handle_reset_playlist_artwork(playlist_id, playlist_name);
            }
            PlaylistsAction::SetAsDefaultPlaylist(playlist_id, playlist_name) => {
                info!(
                    " Setting default playlist: '{}' ({})",
                    playlist_name, playlist_id
                );
                self.settings.default_playlist_id = Some(playlist_id.clone());
                self.settings.default_playlist_name = playlist_name.clone();
                self.settings_page.config_dirty = true;
                self.toast_success(format!("Default playlist set to '{playlist_name}'"));
                self.shell_spawn("persist_default_playlist", move |shell| async move {
                    shell
                        .settings()
                        .set_default_playlist(Some(playlist_id), playlist_name)
                        .await
                });
            }
            views::PlaylistsAction::NavigateAndFilter(view, filter) => {
                return Task::done(Message::Navigation(NavigationMessage::NavigateAndFilter {
                    view,
                    filter,
                    for_browsing_pane: false,
                }));
            }
            views::PlaylistsAction::OpenDefaultPlaylistPicker => {
                return Task::done(Message::DefaultPlaylistPicker(
                    crate::widgets::default_playlist_picker::DefaultPlaylistPickerMessage::Open,
                ));
            }
            views::PlaylistsAction::NewPlaylistInEditor => {
                // Drop straight into a blank track editor (no naming modal) —
                // the editor's own guard refuses if an edit is already open.
                return self.handle_enter_playlist_create_mode();
            }
            views::PlaylistsAction::ColumnVisibilityChanged(col, value) => {
                return self.persist_column_visibility(col, value);
            }
            _ => {} // None + already-handled common actions
        }

        cmd.map(Message::Playlists)
    }

    /// Routes `Message::PlaylistsLoader(...)` arrivals to the existing
    /// `handle_playlists_loaded` handler. Playlists is single-shot (not
    /// paged), so there's only one variant — same shape as Genres.
    pub(crate) fn dispatch_playlists_loader(
        &mut self,
        msg: crate::app_message::PlaylistsLoaderMessage,
    ) -> Task<Message> {
        use crate::app_message::PlaylistsLoaderMessage;
        match msg {
            PlaylistsLoaderMessage::Loaded(result, total_count) => {
                self.handle_playlists_loaded(result, total_count)
            }
        }
    }
}
