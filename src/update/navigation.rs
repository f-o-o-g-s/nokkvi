//! Navigation message handlers

use std::time::Duration;

use iced::Task;
use nokkvi_data::{audio, backend::app_service::AppService};
use tracing::{debug, error, info, warn};

use crate::{
    Nokkvi, Screen, View,
    app_message::{Message, PlaybackMessage},
    services, views, widgets,
};

/// 2s delayed task that emits `PendingExpandAlbumTimeout` so the handler can
/// surface a "Finding album…" toast only when the find chain hasn't already
/// resolved. The handler verifies the id still matches the active target
/// before toasting, so superseded clicks stay silent.
fn expand_album_timeout_task(album_id: String) -> Task<Message> {
    Task::perform(
        async {
            tokio::time::sleep(Duration::from_secs(2)).await;
        },
        move |()| Message::PendingExpandAlbumTimeout(album_id.clone()),
    )
}

/// Artist-side mirror of `expand_album_timeout_task`.
fn expand_artist_timeout_task(artist_id: String) -> Task<Message> {
    Task::perform(
        async {
            tokio::time::sleep(Duration::from_secs(2)).await;
        },
        move |()| Message::PendingExpandArtistTimeout(artist_id.clone()),
    )
}

/// Genre-side mirror — see `expand_album_timeout_task`.
fn expand_genre_timeout_task(genre_id: String) -> Task<Message> {
    Task::perform(
        async {
            tokio::time::sleep(Duration::from_secs(2)).await;
        },
        move |()| Message::PendingExpandGenreTimeout(genre_id.clone()),
    )
}

impl Nokkvi {
    pub(crate) fn handle_session_expired(&mut self) -> Task<Message> {
        info!(" [SESSION] Session expired (401 Unauthorized)");
        let stop_task = if let Some(ref shell) = self.app_service {
            shell.task_manager().shutdown();
            if let Err(e) = nokkvi_data::credentials::clear_session(shell.storage()) {
                warn!(" [SESSION] Failed to clear session: {e}");
            }
            self.cached_storage = Some(shell.storage().clone());

            let engine = shell.audio_engine();
            Task::perform(
                async move {
                    let mut guard = engine.lock().await;
                    guard.stop().await;
                    debug!(" [SESSION] Audio engine stopped after expiry");
                },
                |_| Message::NoOp,
            )
        } else {
            Task::none()
        };

        self.app_service = None;
        self.stored_session = None;
        self.should_auto_login = false;
        self.screen = crate::Screen::Login;
        self.open_menu = None;

        // Reset library state to clear any stale data from the previous session
        self.library = crate::state::LibraryData::default();

        self.toast_info("Session expired. Please log in again.");

        stop_task
    }

    pub(crate) fn handle_switch_view(&mut self, view: View) -> Task<Message> {
        // Close any open overlay menu — its anchor (cursor position, trigger
        // bounds) is tied to the previous view's layout.
        self.open_menu = None;
        // Save current view before entering Settings so we can restore it on close
        if view == View::Settings && self.current_view != View::Settings {
            self.pre_settings_view = self.current_view;
        }
        // When leaving Settings via SwitchView, redirect to the saved pre-settings view
        // unless the user explicitly picked a specific tab (not Queue, which is the
        // default dummy emitted by the settings tab button).
        let view = if self.current_view == View::Settings && view != View::Settings {
            self.pre_settings_view
        } else {
            view
        };
        // Cancel any in-flight find-and-expand chain when navigating away
        // from its host view. PendingExpand::host_view() collapses the
        // per-kind logic — top-pane chains host on Albums/Artists/Genres,
        // browsing-pane chains all host on Queue (the panel is destroyed
        // when leaving Queue).
        if let Some(host_view) = self.pending_expand.as_ref().map(|p| p.host_view())
            && view != host_view
        {
            self.cancel_pending_expand();
        }
        // The top-pin can outlive the target by the brief window between
        // try_resolve_*  consuming the target and TracksLoaded/AlbumsLoaded
        // re-pinning. Drop it on the same navigate-away condition.
        if let Some(pin) = self.pending_top_pin.as_ref() {
            let host_view = match pin {
                crate::state::PendingTopPin::Album(_) => View::Albums,
                crate::state::PendingTopPin::Artist(_) => View::Artists,
                crate::state::PendingTopPin::Genre(_) => View::Genres,
            };
            let in_browsing_pane = self.browsing_panel.is_some() && view == View::Queue;
            if view != host_view && !in_browsing_pane {
                self.pending_top_pin = None;
            }
        }
        // Play view select SFX for tab/hotkey switching
        self.sfx_engine.play(audio::SfxType::ViewSelect);
        self.current_view = view;
        match view {
            View::Albums if self.library.albums.is_empty() => Task::done(Message::LoadAlbums),
            View::Artists if self.library.artists.is_empty() => Task::done(Message::LoadArtists),
            View::Songs if self.library.songs.is_empty() => Task::done(Message::LoadSongs),
            View::Genres if self.library.genres.is_empty() => Task::done(Message::LoadGenres),
            View::Playlists if self.library.playlists.is_empty() => {
                Task::done(Message::LoadPlaylists)
            }
            View::Radios if self.library.radio_stations.is_empty() => {
                Task::done(Message::LoadRadioStations)
            }
            View::Queue => Task::done(Message::LoadQueue), // Always reload queue to reflect changes
            View::Settings => Task::none(),                // Settings don't need data loading
            // Data already loaded — re-prefetch artwork for the current slot_count
            // in case the window was resized since the data was first loaded.
            _ => self.prefetch_viewport_artwork(),
        }
    }

    /// Close Settings and return to the view that was active before Settings was opened.
    pub(crate) fn handle_close_settings(&mut self) -> Task<Message> {
        let target = self.pre_settings_view;
        self.handle_switch_view(target)
    }

    pub(crate) fn handle_login(&mut self, login_msg: views::LoginMessage) -> Task<Message> {
        let (task, action) = self.login_page.update(login_msg);

        // Handle the returned Task from LoginPage
        let mapped_task = task.map(Message::Login);

        // Handle the returned Action
        match action {
            views::LoginAction::None => mapped_task,
            views::LoginAction::AttemptLogin {
                server_url,
                username,
                password,
            } => {
                // Take cached storage from previous logout (if any)
                let cached = self.cached_storage.take();
                // Chain the login task with the mapped task
                Task::batch([
                    mapped_task,
                    Task::perform(
                        async move {
                            let shell = match cached {
                                Some(storage) => AppService::new_with_storage(storage).await,
                                None => AppService::new().await,
                            }
                            .map_err(|e| format!("{e:#}"))?;
                            shell
                                .auth()
                                .login(server_url, username, password)
                                .await
                                .map_err(|e| format!("{e:#}"))?;

                            // Wire up token refresh persistence callback
                            let storage = shell.storage().clone();
                            shell
                                .auth()
                                .set_token_refresh_callback(std::sync::Arc::new(
                                    move |new_token: &str| {
                                        if let Err(e) = nokkvi_data::credentials::save_jwt_token(
                                            &storage, new_token,
                                        ) {
                                            tracing::warn!("Failed to persist refreshed JWT: {e}");
                                        }
                                    },
                                ))
                                .await;

                            Ok(shell)
                        },
                        Message::LoginResult,
                    ),
                ])
            }
        }
    }

    /// Resume session from stored JWT + subsonic credential (no password needed).
    ///
    /// Creates an AppService and uses resume_session() instead of login().
    /// If the JWT is expired, the first API call will fail with 401.
    pub(crate) fn handle_resume_session(&mut self) -> Task<Message> {
        let session = self.stored_session.take();

        let Some(crate::state::StoredSession {
            server_url,
            username,
            jwt_token,
            subsonic_credential,
        }) = session
        else {
            warn!("Resume session called but no stored session found");
            return Task::none();
        };

        info!(target: "nokkvi::auth", "Resuming session for {username}@{server_url}");

        // Take cached storage from previous logout (if any)
        let cached = self.cached_storage.take();

        Task::perform(
            async move {
                let shell = match cached {
                    Some(storage) => AppService::new_with_storage(storage).await,
                    None => AppService::new().await,
                }
                .map_err(|e| format!("{e:#}"))?;
                shell
                    .auth()
                    .resume_session(server_url, username, jwt_token, subsonic_credential)
                    .await
                    .map_err(|e| format!("{e:#}"))?;

                // Wire up token refresh persistence callback
                let storage = shell.storage().clone();
                shell
                    .auth()
                    .set_token_refresh_callback(std::sync::Arc::new(move |new_token: &str| {
                        if let Err(e) =
                            nokkvi_data::credentials::save_jwt_token(&storage, new_token)
                        {
                            tracing::warn!("Failed to persist refreshed JWT: {e}");
                        }
                    }))
                    .await;

                Ok(shell)
            },
            Message::LoginResult,
        )
    }

    pub(crate) fn handle_login_result(
        &mut self,
        result: Result<AppService, String>,
    ) -> Task<Message> {
        match result {
            Ok(shell) => {
                info!(target: "nokkvi::auth", "Login successful");

                // Save server_url + username to config.toml (no password)
                if let Err(e) = nokkvi_data::credentials::save_credentials(
                    &self.login_page.server_url,
                    &self.login_page.username,
                ) {
                    warn!(" Failed to save credentials: {}", e);
                }

                // Save session tokens (JWT + subsonic credential) to redb
                let shell_for_session = shell.clone();
                shell
                    .task_manager()
                    .spawn("save_session", move || async move {
                        let token = shell_for_session.auth().get_token().await;
                        let subsonic = shell_for_session.auth().get_subsonic_credential().await;
                        if let Err(e) = nokkvi_data::credentials::save_session(
                            shell_for_session.storage(),
                            &token,
                            &subsonic,
                        ) {
                            warn!(" Failed to save session: {}", e);
                        } else {
                            debug!(" Session tokens saved for auto-login");
                        }
                    });

                self.login_page.on_login_success();

                // Initialize visualizer with shared config for hot-reload
                let visualizer =
                    widgets::visualizer::visualizer(192, self.visualizer_config.clone());
                // Connect audio callback to engine. The visualizer's audio_callback()
                // now accepts &[f32] directly — no adapter or allocation needed.
                let audio_callback = visualizer.clone().audio_callback();
                let viz_callback: nokkvi_data::audio::VisualizerCallback =
                    std::sync::Arc::new(move |samples: &[f32], sample_rate: u32| {
                        audio_callback(samples, sample_rate);
                    });

                self.visualizer = Some(visualizer);

                let audio_engine = shell.audio_engine();
                // Share the SFX engine's mixer with the music engine so both use
                // one cpal output stream (avoids dual-ALSA-stream silence bug).
                // Returns None if no audio device is available — music will also be disabled.
                let shared_mixer = self.sfx_engine.mixer();
                let pw_volume = self.sfx_engine.has_native_volume();
                shell
                    .task_manager()
                    .spawn("setup_audio", move || async move {
                        let mut engine = audio_engine.lock().await;
                        engine.set_visualizer_callback(viz_callback);
                        if let Some(mixer) = shared_mixer {
                            engine.set_shared_mixer(mixer);
                        }
                        engine.set_pw_volume_active(pw_volume);
                    });

                self.app_service = Some(shell.clone());
                self.screen = Screen::Home;

                // Take the repeat-one loop receiver from the shell and register it
                // with the loop subscription service (global OnceLock) so the
                // `loop_sub` in `subscription()` can receive loop events.
                if let Some(rx) = shell.take_loop_receiver() {
                    services::loop_subscription::register_receiver(rx);
                }
                if let Some(rx) = shell.take_queue_changed_receiver() {
                    services::queue_changed_subscription::register_receiver(rx);
                }
                if let Some(rx) = shell.take_task_status_receiver() {
                    services::task_subscription::register_receiver(rx);
                }

                // Register Navidrome SSE connection for library auto-refresh
                services::navidrome_sse::register(services::navidrome_sse::SseConnectionInfo {
                    server_url: self.login_page.server_url.clone(),
                    auth_gateway: shell.auth().clone(),
                });

                // Initialize scrobble state from persisted queue to prevent spurious
                // now-playing scrobbles on startup
                let shell_for_scrobble = shell.clone();
                let init_scrobble_task = Task::perform(
                    async move {
                        let queue_manager = shell_for_scrobble.queue().queue_manager();
                        let qm = queue_manager.lock().await;
                        let queue = qm.get_queue();
                        queue
                            .current_index
                            .and_then(|idx| queue.song_ids.get(idx))
                            .and_then(|id| qm.get_song(id))
                            .map(|song| song.id.clone())
                    },
                    |song_id| Message::Playback(PlaybackMessage::InitializeScrobbleState(song_id)),
                );

                // Queue data and player settings are loaded eagerly here.
                // Library data loads (Albums, Artists, Songs, etc.) are deferred
                // to ViewPreferencesLoaded so the correct sort mode is applied
                // *before* the first API request — prevents a race where
                // default-sort results overwrite correctly-sorted data.
                // Note: PlayerSettingsLoaded may also trigger a load for start_view
                // to avoid an empty-state flash before prefs arrive.
                let shell_for_player = shell.clone();
                let shell_for_prefs = shell.clone();
                let shell_for_hotkeys = shell.clone();
                return Task::batch([
                    Task::done(Message::LoadQueue), // Queue is always needed (sort-independent)
                    Task::perform(
                        async move { shell_for_player.settings().get_player_settings().await },
                        |settings| {
                            Message::Playback(PlaybackMessage::PlayerSettingsLoaded(Box::new(
                                settings,
                            )))
                        },
                    ),
                    Task::perform(
                        async move { shell_for_prefs.settings().get_view_preferences().await },
                        Message::ViewPreferencesLoaded,
                    ),
                    Task::perform(
                        async move { shell_for_hotkeys.settings().get_hotkey_config().await },
                        Message::HotkeyConfigUpdated,
                    ),
                    init_scrobble_task,
                    Task::perform(
                        {
                            let shell_for_version = shell.clone();
                            async move { shell_for_version.auth().fetch_server_version().await.ok() }
                        },
                        Message::ServerVersionFetched,
                    ),
                ]);
            }
            Err(e) => {
                error!(target: "nokkvi::auth", "Login failed: {e}");
                self.login_page.on_login_error(format!("Login failed: {e}"));
            }
        }
        Task::none()
    }

    /// Handles clicking an inline link (like Artist or Album) from a slot text column.
    pub(crate) fn handle_navigate_and_filter(
        &mut self,
        view: crate::View,
        filter: nokkvi_data::types::filter::LibraryFilter,
    ) -> Task<Message> {
        self.cancel_pending_expand();
        let switch_task = self.handle_switch_view(view);

        // Defocus search input
        if let Some(page) = self.current_view_page_mut() {
            page.common_mut().search_input_focused = false;
        }

        // Set the active filter on the target view and update search display text
        let display = filter.display_text();
        match view {
            View::Albums => {
                self.albums_page.common.active_filter = Some(filter);
                self.albums_page.common.search_query = display;
            }
            View::Songs => {
                self.songs_page.common.active_filter = Some(filter);
                self.songs_page.common.search_query = display;
            }
            View::Artists => {
                self.artists_page.common.active_filter = Some(filter);
                self.artists_page.common.search_query = display;
            }
            View::Genres => {
                self.genres_page.common.active_filter = Some(filter);
                self.genres_page.common.search_query = display;
            }
            _ => {}
        }

        // Trigger a data reload with the active filter
        let load_task = match view {
            View::Albums => Task::done(Message::LoadAlbums),
            View::Songs => Task::done(Message::LoadSongs),
            View::Artists => Task::done(Message::LoadArtists),
            View::Genres => Task::done(Message::LoadGenres),
            _ => Task::none(),
        };

        Task::batch([switch_task, load_task])
    }

    /// Cross-view navigation that lands in the Albums view, clears any active
    /// search/filter, and primes a "find-and-expand" target. The albums load
    /// handlers consume the target after each page arrives — paging through
    /// the unfiltered list until the album appears, then dispatching
    /// `FocusAndExpand` so its tracks render inline.
    pub(crate) fn handle_navigate_and_expand_album(&mut self, album_id: String) -> Task<Message> {
        self.prime_expand_album_target(album_id.clone(), false);
        // Clearing `library.albums` above flips `is_empty()` to true, so
        // handle_switch_view's Albums arm dispatches LoadAlbums for us.
        let switch_task = self.handle_switch_view(View::Albums);
        Task::batch([switch_task, expand_album_timeout_task(album_id)])
    }

    /// Browsing-pane variant of `handle_navigate_and_expand_album` — switches
    /// the browsing panel to its Albums tab and runs the same find chain
    /// without disrupting the top-pane Queue.
    pub(crate) fn handle_browser_pane_navigate_and_expand_album(
        &mut self,
        album_id: String,
    ) -> Task<Message> {
        self.prime_expand_album_target(album_id.clone(), true);
        let switch_task = self.handle_browsing_panel_message(
            crate::views::BrowsingPanelMessage::SwitchView(crate::views::BrowsingView::Albums),
        );
        Task::batch([
            switch_task,
            Task::done(Message::LoadAlbums),
            expand_album_timeout_task(album_id),
        ])
    }

    /// Shared setup for both navigate-and-expand variants: reset Albums page
    /// state, defocus its search input (so the user isn't typing into it on
    /// arrival), drop the current buffer, and install the pending target.
    /// Caller dispatches the actual load + timeout tasks.
    fn prime_expand_album_target(&mut self, album_id: String, for_browsing_pane: bool) {
        self.albums_page.common.search_input_focused = false;
        self.albums_page.common.active_filter = None;
        self.albums_page.common.search_query.clear();
        self.albums_page.expansion.clear();
        self.albums_page.common.slot_list.viewport_offset = 0;
        self.albums_page.common.slot_list.selected_indices.clear();
        self.albums_page.common.slot_list.selected_offset = None;
        self.library.albums.clear();
        self.pending_expand = Some(crate::state::PendingExpand::Album {
            album_id,
            for_browsing_pane,
        });
    }

    /// Drop the in-flight find-and-expand chain (whichever kind) plus any
    /// pending top-pin. Called from cancellation hooks (search edit, sort
    /// change, navigation away, refresh) so the chain doesn't continue
    /// after the user has moved on. The pin only outlives the target by
    /// the brief window between `try_resolve` and the corresponding
    /// `set_children`, so unconditional pin clearing is correct here.
    pub(crate) fn cancel_pending_expand(&mut self) {
        self.pending_expand = None;
        self.pending_top_pin = None;
    }

    /// Genre-side mirror of `handle_navigate_and_expand_album`. The find
    /// chain is single-shot since genres don't paginate.
    pub(crate) fn handle_navigate_and_expand_genre(&mut self, genre_id: String) -> Task<Message> {
        self.prime_expand_genre_target(genre_id.clone(), false);
        let switch_task = self.handle_switch_view(View::Genres);
        Task::batch([switch_task, expand_genre_timeout_task(genre_id)])
    }

    /// Browsing-pane variant of `handle_navigate_and_expand_genre`.
    pub(crate) fn handle_browser_pane_navigate_and_expand_genre(
        &mut self,
        genre_id: String,
    ) -> Task<Message> {
        self.prime_expand_genre_target(genre_id.clone(), true);
        let switch_task = self.handle_browsing_panel_message(
            crate::views::BrowsingPanelMessage::SwitchView(crate::views::BrowsingView::Genres),
        );
        Task::batch([
            switch_task,
            Task::done(Message::LoadGenres),
            expand_genre_timeout_task(genre_id),
        ])
    }

    /// Shared setup for both genre navigate-and-expand variants.
    fn prime_expand_genre_target(&mut self, genre_id: String, for_browsing_pane: bool) {
        self.genres_page.common.search_input_focused = false;
        self.genres_page.common.active_filter = None;
        self.genres_page.common.search_query.clear();
        self.genres_page.expansion.clear();
        self.genres_page.sub_expansion.clear();
        self.genres_page.common.slot_list.viewport_offset = 0;
        self.genres_page.common.slot_list.selected_indices.clear();
        self.genres_page.common.slot_list.selected_offset = None;
        self.library.genres.clear();
        self.pending_expand = Some(crate::state::PendingExpand::Genre {
            genre_id,
            for_browsing_pane,
        });
    }

    /// Genre-side mirror of `handle_pending_expand_album_timeout`.
    pub(crate) fn handle_pending_expand_genre_timeout(
        &mut self,
        genre_id: String,
    ) -> Task<Message> {
        if matches!(
            &self.pending_expand,
            Some(crate::state::PendingExpand::Genre { genre_id: pending, .. }) if pending == &genre_id
        ) {
            self.toast_info("Finding genre…");
        }
        Task::none()
    }

    /// Genre-side mirror of `try_resolve_pending_expand_album`. Single-shot:
    /// once the load completes, the target is either in the buffer or
    /// genuinely not in the library — there are no further pages to await.
    ///
    /// Match is by `name`, not `id`. Navidrome's `/api/genre` returns proper
    /// internal IDs (UUIDs) that differ from the display names, but the
    /// click sites only have access to the displayed string (`extra_value`
    /// / `genre`) — that's the dispatched target. The convention mirrors
    /// the existing `LibraryFilter::GenreId` which also passes the name in
    /// both id and name fields.
    ///
    /// The resolved internal id IS what we store in the pin, though, because
    /// the downstream `GenresMessage::AlbumsLoaded(genre_id, …)` carries
    /// that internal id — the post-hook in `handle_genres` matches against
    /// it to decide whether to re-pin the highlight.
    pub(crate) fn try_resolve_pending_expand_genre(&mut self) -> Option<Task<Message>> {
        let target_id = match &self.pending_expand {
            Some(crate::state::PendingExpand::Genre { genre_id, .. }) => genre_id.clone(),
            _ => return None,
        };

        let found = self
            .library
            .genres
            .iter()
            .enumerate()
            .find_map(|(i, g)| (g.name == target_id).then(|| (i, g.id.clone())));
        if let Some((idx, resolved_id)) = found {
            debug!(
                " [EXPAND] Found genre '{}' at index {} (id={}) — scrolling + dispatching FocusAndExpand",
                target_id, idx, resolved_id
            );
            self.pending_expand = None;
            let total = self.library.genres.len();
            let center_slot = self.genres_page.common.slot_list.slot_count.max(2) / 2;
            let target_offset = idx.saturating_add(center_slot).min(total.saturating_sub(1));
            self.genres_page
                .common
                .slot_list
                .set_offset(target_offset, total);
            self.genres_page.common.slot_list.set_selected(idx, total);
            self.genres_page.common.slot_list.flash_center();
            self.pending_top_pin = Some(crate::state::PendingTopPin::Genre(resolved_id));
            let prefetch_task = self.prefetch_viewport_artwork();
            return Some(Task::batch([
                prefetch_task,
                Task::done(Message::Genres(views::GenresMessage::FocusAndExpand(idx))),
            ]));
        }

        if self.library.genres.is_loading() {
            return None;
        }

        // Single-shot: idle + not-found means the genre genuinely isn't in
        // the library — no more pages will arrive.
        warn!(
            " [EXPAND] Genre '{}' not found after load — clearing target",
            target_id
        );
        self.toast_warn("Genre not found in library");
        self.pending_expand = None;
        Some(Task::none())
    }

    /// Artist-side mirror of `handle_navigate_and_expand_album`. Lands on
    /// the Artists view, clears any active search/filter, and primes a
    /// find-and-expand target. The artists load handlers consume it after
    /// each page arrives.
    pub(crate) fn handle_navigate_and_expand_artist(&mut self, artist_id: String) -> Task<Message> {
        self.prime_expand_artist_target(artist_id.clone(), false);
        let switch_task = self.handle_switch_view(View::Artists);
        Task::batch([switch_task, expand_artist_timeout_task(artist_id)])
    }

    /// Browsing-pane variant of `handle_navigate_and_expand_artist`.
    pub(crate) fn handle_browser_pane_navigate_and_expand_artist(
        &mut self,
        artist_id: String,
    ) -> Task<Message> {
        self.prime_expand_artist_target(artist_id.clone(), true);
        let switch_task = self.handle_browsing_panel_message(
            crate::views::BrowsingPanelMessage::SwitchView(crate::views::BrowsingView::Artists),
        );
        Task::batch([
            switch_task,
            Task::done(Message::LoadArtists),
            expand_artist_timeout_task(artist_id),
        ])
    }

    /// Shared setup for both artist navigate-and-expand variants.
    fn prime_expand_artist_target(&mut self, artist_id: String, for_browsing_pane: bool) {
        self.artists_page.common.search_input_focused = false;
        self.artists_page.common.active_filter = None;
        self.artists_page.common.search_query.clear();
        self.artists_page.expansion.clear();
        self.artists_page.sub_expansion.clear();
        self.artists_page.common.slot_list.viewport_offset = 0;
        self.artists_page.common.slot_list.selected_indices.clear();
        self.artists_page.common.slot_list.selected_offset = None;
        self.library.artists.clear();
        self.pending_expand = Some(crate::state::PendingExpand::Artist {
            artist_id,
            for_browsing_pane,
        });
    }

    /// Artist-side mirror of `handle_pending_expand_album_timeout`.
    pub(crate) fn handle_pending_expand_artist_timeout(
        &mut self,
        artist_id: String,
    ) -> Task<Message> {
        if matches!(
            &self.pending_expand,
            Some(crate::state::PendingExpand::Artist { artist_id: pending, .. }) if pending == &artist_id
        ) {
            self.toast_info("Finding artist…");
        }
        Task::none()
    }

    /// Artist-side mirror of `try_resolve_pending_expand_album`. After each
    /// artists page lands, look for the pending target in `library.artists`
    /// and either dispatch FocusAndExpand at the right viewport position,
    /// give up if fully loaded, wait if still loading, or kick the next
    /// page (force-loaded) if more remain.
    pub(crate) fn try_resolve_pending_expand_artist(&mut self) -> Option<Task<Message>> {
        let target_id = match &self.pending_expand {
            Some(crate::state::PendingExpand::Artist { artist_id, .. }) => artist_id.clone(),
            _ => return None,
        };

        if let Some(idx) = self.library.artists.iter().position(|a| a.id == target_id) {
            debug!(
                " [EXPAND] Found artist '{}' at index {} — scrolling + dispatching FocusAndExpand",
                target_id, idx
            );
            self.pending_expand = None;
            let total = self.library.artists.len();
            let center_slot = self.artists_page.common.slot_list.slot_count.max(2) / 2;
            let target_offset = idx.saturating_add(center_slot).min(total.saturating_sub(1));
            self.artists_page
                .common
                .slot_list
                .set_offset(target_offset, total);
            self.artists_page.common.slot_list.set_selected(idx, total);
            self.artists_page.common.slot_list.flash_center();
            // Pin the highlight onto the target so it survives `set_children`
            // when albums land — handle_artists' AlbumsLoaded post-hook
            // re-runs set_selected for this id.
            self.pending_top_pin = Some(crate::state::PendingTopPin::Artist(target_id.clone()));
            let prefetch_task = self.prefetch_viewport_artwork();
            return Some(Task::batch([
                prefetch_task,
                Task::done(Message::Artists(views::ArtistsMessage::FocusAndExpand(idx))),
            ]));
        }

        if self.library.artists.fully_loaded() {
            warn!(
                " [EXPAND] Artist '{}' not found after full load — clearing target",
                target_id
            );
            self.toast_warn("Artist not found in library");
            self.pending_expand = None;
            return Some(Task::none());
        }

        if self.library.artists.is_loading() {
            return None;
        }

        let next_offset = self.library.artists.loaded_count();
        debug!(
            " [EXPAND] Artist '{}' not in buffer — force-fetching next page at offset {}",
            target_id, next_offset
        );
        Some(self.force_load_artists_page(next_offset))
    }

    /// After each albums page lands, look for the pending expand target in
    /// `library.albums`. Returns `Some(task)` if the helper acted (found and
    /// dispatched, fully-loaded miss, or kicked the next page) and `None` if
    /// it should be retried after the next page arrives.
    pub(crate) fn try_resolve_pending_expand_album(&mut self) -> Option<Task<Message>> {
        let target_id = match &self.pending_expand {
            Some(crate::state::PendingExpand::Album { album_id, .. }) => album_id.clone(),
            _ => return None,
        };

        if let Some(idx) = self.library.albums.iter().position(|a| a.id == target_id) {
            debug!(
                " [EXPAND] Found album '{}' at index {} — scrolling + dispatching FocusAndExpand",
                target_id, idx
            );
            self.pending_expand = None;
            // Position the target at slot 0 (top of the visible list) — fewer
            // distractions above the expansion, and most visible rows are
            // tracks instead of unrelated albums. viewport_offset is the
            // index of the item rendered at the *center slot*, so adding
            // center_slot shifts the displayed window down by that many
            // positions, leaving the target at slot 0. Falls back to
            // (total-1) when target is near the end of the library.
            let total = self.library.albums.len();
            let center_slot = self.albums_page.common.slot_list.slot_count.max(2) / 2;
            let target_offset = idx.saturating_add(center_slot).min(total.saturating_sub(1));
            self.albums_page
                .common
                .slot_list
                .set_offset(target_offset, total);
            // set_offset clears selected_offset; re-set so the target keeps
            // the highlight styling (effective center derives from
            // selected_offset before falling back to viewport_offset).
            self.albums_page.common.slot_list.set_selected(idx, total);
            self.albums_page.common.slot_list.flash_center();
            // Pin the highlight onto the target so it survives `set_children`
            // when tracks land — handle_albums' TracksLoaded post-hook
            // re-runs set_selected for this id.
            self.pending_top_pin = Some(crate::state::PendingTopPin::Album(target_id.clone()));
            // Mini-artwork prefetch follows the viewport. The page-load
            // prefetch ran for viewport=0 (and page-2/3 loads don't prefetch
            // at all), so the rows around the new viewport would render
            // as empty placeholders without an explicit kick here.
            let prefetch_task = self.prefetch_viewport_artwork();
            return Some(Task::batch([
                prefetch_task,
                Task::done(Message::Albums(views::AlbumsMessage::FocusAndExpand(idx))),
            ]));
        }

        if self.library.albums.fully_loaded() {
            warn!(
                " [EXPAND] Album '{}' not found after full load — clearing target",
                target_id
            );
            self.toast_warn("Album not found in library");
            self.pending_expand = None;
            return Some(Task::none());
        }

        if self.library.albums.is_loading() {
            return None;
        }

        let next_offset = self.library.albums.loaded_count();
        debug!(
            " [EXPAND] Album '{}' not in buffer — force-fetching next page at offset {}",
            target_id, next_offset
        );
        Some(self.force_load_albums_page(next_offset))
    }

    /// Fired ~2s after `handle_navigate_and_expand_album` to surface a
    /// "Finding album…" toast when the chain is still hunting. Compares
    /// `album_id` against the currently pending target so a stale timeout
    /// from a superseded click does not toast.
    pub(crate) fn handle_pending_expand_album_timeout(
        &mut self,
        album_id: String,
    ) -> Task<Message> {
        if matches!(
            &self.pending_expand,
            Some(crate::state::PendingExpand::Album { album_id: pending, .. }) if pending == &album_id
        ) {
            self.toast_info("Finding album…");
        }
        Task::none()
    }

    /// Handles cross-view navigation specifically intercepted from within the right-side browsing pane.
    /// Operates identically to `handle_navigate_and_filter` but targets `BrowsingPane` tab state
    /// rather than disrupting the main structural `current_view` state (which stays as Queue).
    pub(crate) fn handle_browser_pane_navigate_and_filter(
        &mut self,
        view: crate::View,
        filter: nokkvi_data::types::filter::LibraryFilter,
    ) -> Task<Message> {
        self.cancel_pending_expand();
        let browse_view = match view {
            View::Albums => Some(crate::views::BrowsingView::Albums),
            View::Songs => Some(crate::views::BrowsingView::Songs),
            View::Artists => Some(crate::views::BrowsingView::Artists),
            View::Genres => Some(crate::views::BrowsingView::Genres),
            _ => None,
        };

        let Some(bv) = browse_view else {
            return Task::none();
        };

        let switch_task =
            self.handle_browsing_panel_message(crate::views::BrowsingPanelMessage::SwitchView(bv));

        // Defocus search input
        if let Some(page) = self.current_view_page_mut() {
            page.common_mut().search_input_focused = false;
        }

        // Set the active filter on the target view and update search display text
        let display = filter.display_text();
        match view {
            View::Albums => {
                self.albums_page.common.active_filter = Some(filter);
                self.albums_page.common.search_query = display;
            }
            View::Songs => {
                self.songs_page.common.active_filter = Some(filter);
                self.songs_page.common.search_query = display;
            }
            View::Artists => {
                self.artists_page.common.active_filter = Some(filter);
                self.artists_page.common.search_query = display;
            }
            View::Genres => {
                self.genres_page.common.active_filter = Some(filter);
                self.genres_page.common.search_query = display;
            }
            _ => {}
        }

        // Trigger a data reload with the active filter
        let load_task = match view {
            View::Albums => Task::done(Message::LoadAlbums),
            View::Songs => Task::done(Message::LoadSongs),
            View::Artists => Task::done(Message::LoadArtists),
            View::Genres => Task::done(Message::LoadGenres),
            _ => Task::none(),
        };

        Task::batch([switch_task, load_task])
    }
}
