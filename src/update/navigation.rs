//! Navigation message handlers

use iced::Task;
use nokkvi_data::{audio, backend::app_service::AppService};
use tracing::{debug, info, warn};

use crate::{
    Nokkvi, Screen, View,
    app_message::{Message, PlaybackMessage},
    services, views, widgets,
};

impl Nokkvi {
    pub(crate) fn handle_switch_view(&mut self, view: View) -> Task<Message> {
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
                            .map_err(|e| e.to_string())?;
                            shell
                                .auth()
                                .login(server_url, username, password)
                                .await
                                .map_err(|e| e.to_string())?;

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

        let Some((server_url, username, jwt_token, subsonic_credential)) = session else {
            warn!("Resume session called but no stored session found");
            return Task::none();
        };

        info!("Resuming session for {}@{}", username, server_url);

        // Take cached storage from previous logout (if any)
        let cached = self.cached_storage.take();

        Task::perform(
            async move {
                let shell = match cached {
                    Some(storage) => AppService::new_with_storage(storage).await,
                    None => AppService::new().await,
                }
                .map_err(|e| e.to_string())?;
                shell
                    .auth()
                    .resume_session(server_url, username, jwt_token, subsonic_credential)
                    .await
                    .map_err(|e| e.to_string())?;

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
                info!(" Login successful!");

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

    /// Handles cross-view navigation specifically intercepted from within the right-side browsing pane.
    /// Operates identically to `handle_navigate_and_filter` but targets `BrowsingPane` tab state
    /// rather than disrupting the main structural `current_view` state (which stays as Queue).
    pub(crate) fn handle_browser_pane_navigate_and_filter(
        &mut self,
        view: crate::View,
        filter: nokkvi_data::types::filter::LibraryFilter,
    ) -> Task<Message> {
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
