//! Navigation message handlers

use std::time::Duration;

use iced::Task;
use nokkvi_data::{audio, backend::app_service::AppService};
use tracing::{debug, error, info, warn};

use crate::{
    Nokkvi, Screen, View,
    app_message::{Message, NavigationMessage, PlaybackMessage},
    services, views, widgets,
};

// === pending-expand helpers ===

/// 2s delayed task that emits a single
/// `Message::Navigation(NavigationMessage::ExpandTimeout(..))` carrying the
/// chain's `PendingExpand`. The collapsed `handle_pending_expand_timeout`
/// verifies the variant + id still match the active target before toasting,
/// so superseded clicks stay silent. Songs only enter the find-and-expand
/// chain via the CenterOnPlaying (Shift+C) fallback.
pub(crate) fn pending_expand_timeout_task(pending: crate::state::PendingExpand) -> Task<Message> {
    Task::perform(
        async {
            tokio::time::sleep(Duration::from_secs(2)).await;
        },
        move |()| Message::Navigation(NavigationMessage::ExpandTimeout(pending.clone())),
    )
}

/// Toast a "Finding {entity}…" notification iff `expected` still matches the
/// active pending-expand target (same variant + same id; `for_browsing_pane`
/// is ignored). Called by the collapsed `handle_pending_expand_timeout`
/// dispatcher — superseded clicks stay silent because the active target
/// has already been replaced or cleared. Discriminant equality plus
/// `entity_id()` covers the "same variant + same id" contract without an
/// N×N pairing match.
fn pending_expand_timeout_toast(app: &mut Nokkvi, expected: &crate::state::PendingExpand) {
    let id_matches = app.pending_expand.target.as_ref().is_some_and(|active| {
        std::mem::discriminant(active) == std::mem::discriminant(expected)
            && active.entity_id() == expected.entity_id()
    });
    if id_matches {
        let label = match expected {
            crate::state::PendingExpand::Album { .. } => "Finding album…",
            crate::state::PendingExpand::Artist { .. } => "Finding artist…",
            crate::state::PendingExpand::Genre { .. } => "Finding genre…",
            crate::state::PendingExpand::Song { .. } => "Finding song…",
        };
        app.toast_info(label);
    }
}

/// Single source of truth for the find-and-expand priming reset. Called by
/// every `handle_navigate_and_expand_*`, `handle_browser_pane_navigate_and_expand_*`,
/// and `start_center_on_playing_*_chain` site after the caller decides which
/// entity is being targeted. Differences across entities are limited to the
/// page-state field the reset hits; songs additionally skip
/// `expansion.clear()` because songs aren't expandable.
///
/// Always resets `pending_expand.center_only = false`. CenterOnPlaying
/// callers re-arm the flag *after* this returns — keep that order.
pub(crate) fn prime_expand_target(app: &mut Nokkvi, pending: crate::state::PendingExpand) {
    match &pending {
        crate::state::PendingExpand::Album { .. } => {
            app.albums_page.common.search_input_focused = false;
            app.albums_page.common.active_filter = None;
            app.albums_page.common.search_query.clear();
            app.albums_page.expansion.clear();
            app.albums_page.common.slot_list.viewport_offset = 0;
            app.albums_page.common.clear_selection_for_expand_prime();
            app.library.albums.clear();
        }
        crate::state::PendingExpand::Artist { .. } => {
            app.artists_page.common.search_input_focused = false;
            app.artists_page.common.active_filter = None;
            app.artists_page.common.search_query.clear();
            app.artists_page.expansion.clear();
            app.artists_page.common.slot_list.viewport_offset = 0;
            app.artists_page.common.clear_selection_for_expand_prime();
            app.library.artists.clear();
        }
        crate::state::PendingExpand::Genre { .. } => {
            app.genres_page.common.search_input_focused = false;
            app.genres_page.common.active_filter = None;
            app.genres_page.common.search_query.clear();
            app.genres_page.expansion.clear();
            app.genres_page.common.slot_list.viewport_offset = 0;
            app.genres_page.common.clear_selection_for_expand_prime();
            app.library.genres.clear();
        }
        crate::state::PendingExpand::Song { .. } => {
            app.songs_page.common.search_input_focused = false;
            app.songs_page.common.active_filter = None;
            app.songs_page.common.search_query.clear();
            // Songs aren't expandable — no expansion field to clear.
            app.songs_page.common.slot_list.viewport_offset = 0;
            app.songs_page.common.clear_selection_for_expand_prime();
            app.library.songs.clear();
        }
    }
    app.pending_expand.center_only = false;
    app.pending_expand.target = Some(pending);
}

impl Nokkvi {
    /// Dispatch `NavigationMessage` variants to the per-variant handlers below.
    ///
    /// Cross-cutting carrier for view switches and navigate-and-filter /
    /// navigate-and-expand chains. The 10 previously-flat `Message::*`
    /// variants collapsed onto this one carrier; `for_browsing_pane: bool`
    /// (on `NavigateAndFilter` directly, and inside `PendingExpand` for the
    /// expand variants) discriminates the top-pane vs browsing-pane chain.
    pub(crate) fn handle_navigation(&mut self, msg: NavigationMessage) -> Task<Message> {
        use crate::state::PendingExpand;
        match msg {
            NavigationMessage::SwitchView(view) => self.handle_switch_view(view),
            NavigationMessage::NavigateAndFilter {
                view,
                filter,
                for_browsing_pane: false,
            } => self.handle_navigate_and_filter(view, filter),
            NavigationMessage::NavigateAndFilter {
                view,
                filter,
                for_browsing_pane: true,
            } => self.handle_browser_pane_navigate_and_filter(view, filter),
            NavigationMessage::Expand(PendingExpand::Album {
                album_id,
                for_browsing_pane: false,
            }) => self.handle_navigate_and_expand_album(album_id),
            NavigationMessage::Expand(PendingExpand::Album {
                album_id,
                for_browsing_pane: true,
            }) => self.handle_browser_pane_navigate_and_expand_album(album_id),
            NavigationMessage::Expand(PendingExpand::Artist {
                artist_id,
                for_browsing_pane: false,
            }) => self.handle_navigate_and_expand_artist(artist_id),
            NavigationMessage::Expand(PendingExpand::Artist {
                artist_id,
                for_browsing_pane: true,
            }) => self.handle_browser_pane_navigate_and_expand_artist(artist_id),
            NavigationMessage::Expand(PendingExpand::Genre {
                genre_id,
                for_browsing_pane: false,
            }) => self.handle_navigate_and_expand_genre(genre_id),
            NavigationMessage::Expand(PendingExpand::Genre {
                genre_id,
                for_browsing_pane: true,
            }) => self.handle_browser_pane_navigate_and_expand_genre(genre_id),
            // Songs aren't an Expand call site today — `Expand(Song)` is a
            // forward-compatible shape. The CenterOnPlaying flow primes Song
            // targets via `start_center_on_playing_chain` (not Expand).
            NavigationMessage::Expand(PendingExpand::Song { .. }) => Task::none(),
            NavigationMessage::ExpandTimeout(pending) => {
                self.handle_pending_expand_timeout(pending)
            }
        }
    }

    pub(crate) fn handle_session_expired(&mut self) -> Task<Message> {
        info!(" [SESSION] Session expired (401 Unauthorized)");
        // Shared session teardown — see `Nokkvi::reset_session_state` in
        // `update/components.rs`. Session-expired surfaces a user-facing
        // toast (the user didn't take this action, so they need to know
        // why they're back at the Login screen).
        let stop_task = self.reset_session_state();
        self.toast_info("Session expired. Please log in again.");
        stop_task
    }

    /// Mutable references to every slot-list page's shared `SlotListPageState`.
    /// The whole-app fan-outs below share this one array instead of each
    /// repeating the eight page fields. It stays hand-maintained — adding a page
    /// means adding it here; the `; 8` length only forces the count to match the
    /// elements listed, it won't catch a page silently left out.
    fn all_slot_list_commons_mut(&mut self) -> [&mut crate::widgets::SlotListPageState; 8] {
        [
            &mut self.albums_page.common,
            &mut self.artists_page.common,
            &mut self.genres_page.common,
            &mut self.playlists_page.common,
            &mut self.queue_page.common,
            &mut self.songs_page.common,
            &mut self.radios_page.common,
            &mut self.similar_page.common,
        ]
    }

    /// Clear the auto-hide toolbar reveal-locks on every slot-list page.
    ///
    /// Called on unmount edges where a page's header leaves the widget tree with
    /// a reveal-lock still set — chiefly a main-view switch (the outgoing view's
    /// header `mouse_area` / sort `pick_list` unmount, so `on_exit` / `on_close`
    /// can't fire to clear `toolbar_hovered` / `toolbar_dropdown_open`) and
    /// session reset. Clearing every page is idempotent and drift-proof: only
    /// the rendered view reads its own flags, and a genuinely-hovered mounted
    /// header re-fires `on_enter` on the next cursor event. Search state is left
    /// intact (an active filter legitimately keeps the toolbar revealed).
    pub(crate) fn clear_all_toolbar_reveal_locks(&mut self) {
        for common in self.all_slot_list_commons_mut() {
            common.reset_reveal_locks();
        }
    }

    /// Mark every slot-list page as OS-window-focused or not. The auto-hide
    /// toolbar's transient reveals (hover / open dropdown / hotkey timer /
    /// focused-but-empty search) are gated on this in `toolbar_revealed`, so
    /// losing focus collapses a mid-reveal toolbar even if its `on_exit` never
    /// fired (unfocused Wayland surfaces stop delivering pointer events) and even
    /// if the cursor is parked in the hover zone. A non-empty search filter is
    /// not gated.
    pub(crate) fn set_all_window_focused(&mut self, focused: bool) {
        for common in self.all_slot_list_commons_mut() {
            common.set_window_focused(focused);
        }
    }

    /// Drop search-input focus only on pages whose search box actually UNMOUNTS
    /// on focus loss — i.e. when the auto-hide toolbar collapses (auto-hide on
    /// and no active filter). There iced silently drops the text_input focus but
    /// never fires our blur, so a lingering `search_input_focused` would re-reveal
    /// the header on refocus with the cursor nowhere near it. When the box stays
    /// mounted (an active filter, or auto-hide off) it keeps real iced focus, so
    /// clearing the flag would desync it and break Tab-out / Escape, which both
    /// read this flag — leave it. The search *query* is always left intact.
    pub(crate) fn clear_all_search_input_focus(&mut self) {
        if !crate::theme::is_autohide_toolbar() {
            return;
        }
        for common in self.all_slot_list_commons_mut() {
            if common.search_query.is_empty() {
                common.search_input_focused = false;
            }
        }
    }

    pub(crate) fn handle_switch_view(&mut self, view: View) -> Task<Message> {
        // Close any open overlay menu — its anchor (cursor position, trigger
        // bounds) is tied to the previous view's layout.
        self.open_menu = None;

        // The outgoing view's header mouse_area / sort pick_list unmount on the
        // switch, so their on_exit / on_close can't fire to clear a set
        // reveal-lock — a keyboard-driven switch would otherwise leave the
        // toolbar stuck revealed on return. Mirrors clear_browsing_panel_reveal_locks.
        self.clear_all_toolbar_reveal_locks();

        // Cancel any in-progress roulette when leaving its host view —
        // continuing the spin on a different view's slot list would scroll
        // through unrelated rows and dispatch a play action against an
        // index that no longer corresponds to anything visible.
        if let Some(state) = self.roulette.as_ref()
            && state.view != view
        {
            // Restore the original viewport before clearing state so the
            // user lands back where they started rather than mid-spin.
            let prev = state.view;
            let original = state.original_offset;
            let total = state.total_items;
            self.roulette = None;
            self.roulette_apply_offset(prev, original, total);
            self.sfx_engine.play(audio::SfxType::Escape);
        }
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
        // The playlist editor is reachable only while a session is active
        // (via the contextual "Editing" pill). Guard against routing here
        // without one — fall back to the queue rather than rendering an
        // empty editor split.
        let view = if view == View::PlaylistEditor && self.playlist_editor.is_none() {
            View::Queue
        } else {
            view
        };
        // Cancel any in-flight find-and-expand chain when navigating away
        // from its host view. PendingExpand::host_view() collapses the
        // per-kind logic — top-pane chains host on Albums/Artists/Genres,
        // browsing-pane chains all host on Queue (the panel is destroyed
        // when leaving Queue).
        if let Some(host_view) = self.pending_expand.target.as_ref().map(|p| p.host_view())
            && view != host_view
        {
            self.cancel_pending_expand();
        }
        // The top-pin can outlive the target by the brief window between
        // try_resolve_*  consuming the target and TracksLoaded/AlbumsLoaded
        // re-pinning. Drop it on the same navigate-away condition.
        if let Some(pin) = self.pending_expand.top_pin.as_ref() {
            let host_view = match pin {
                crate::state::PendingTopPin::Album(_) => View::Albums,
                crate::state::PendingTopPin::Artist(_) => View::Artists,
                crate::state::PendingTopPin::Genre(_) => View::Genres,
            };
            let in_browsing_pane = self.browsing_panel.is_some() && view == View::Queue;
            if view != host_view && !in_browsing_pane {
                self.pending_expand.top_pin = None;
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
            View::Settings => {
                // Entries are rebuilt in update (not per frame), so entering
                // the view must populate the cache: first entry, re-entry
                // after Escape cleared it, or a dirty mark set while away
                // (current_view is already View::Settings here, so the
                // refresh gate passes).
                self.refresh_settings_entries_if_dirty();
                Task::none()
            }
            View::PlaylistEditor => Task::none(), // Buffer already populated by the enter flow
            // Data already loaded — re-prefetch artwork for the current slot_count
            // in case the window was resized since the data was first loaded.
            View::Albums
            | View::Artists
            | View::Songs
            | View::Genres
            | View::Playlists
            | View::Radios => self.prefetch_viewport_artwork(),
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

                            // The auth service resolved the typed input to the
                            // candidate that actually connected; surface it so
                            // the root persists the resolved URL, not the raw
                            // input.
                            let resolved_url = shell.auth().get_server_url().await;
                            Ok(crate::app_message::LoginSuccess {
                                shell,
                                resolved_url,
                            })
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

        // Mark the form busy for the duration of the resume so a manual
        // login attempt during the brief auto-login window is blocked by the
        // double-submit guard (two concurrent AppService::new() opens would
        // collide on redb's exclusive lock). Cleared by on_login_success /
        // on_login_error via handle_login_result.
        self.login_page.login_in_progress = true;

        // Re-source username from the credential's u= field — login_page.username
        // can be empty when resuming a session predating save_credentials writes.
        if let Some(parsed) =
            nokkvi_data::credentials::parse_username_from_credential(&subsonic_credential)
        {
            self.login_page.username = parsed.to_string();
        }

        // Take cached storage from previous logout (if any)
        let cached = self.cached_storage.take();

        Task::perform(
            async move {
                let shell = match cached {
                    Some(storage) => AppService::new_with_storage(storage).await,
                    None => AppService::new().await,
                }
                .map_err(|e| format!("{e:#}"))?;
                // A resumed session reuses the already-canonical stored URL.
                let resolved_url = server_url.clone();
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

                Ok(crate::app_message::LoginSuccess {
                    shell,
                    resolved_url,
                })
            },
            Message::LoginResult,
        )
    }

    pub(crate) fn handle_login_result(
        &mut self,
        result: Result<crate::app_message::LoginSuccess, String>,
    ) -> Task<Message> {
        match result {
            Ok(success) => {
                let crate::app_message::LoginSuccess {
                    shell,
                    resolved_url,
                } = success;
                // Persist the RESOLVED server URL (the candidate that actually
                // connected) so config save, SSE registration, and the resume
                // path all use it rather than the raw typed input.
                self.login_page.server_url = resolved_url;
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
                let audio_callback = visualizer.audio_callback();
                let viz_callback: nokkvi_data::audio::VisualizerCallback =
                    std::sync::Arc::new(move |samples: &[f32], sample_rate: u32| {
                        audio_callback(samples, sample_rate);
                    });

                self.visualizer = Some(visualizer);

                let audio_engine = shell.audio_engine();
                // Hand the music-output bridge to the engine. The renderer owns
                // the music sink (rebuilt per-track at native rate in bit-perfect
                // mode) and publishes its mixer + IPC into the bridge; the SFX
                // engine + volume UI reach the current sink through it.
                let music_bridge = self.sfx_engine.music_bridge();
                // Push the canonical shared EqState alongside the bridge so the
                // renderer holds the live UI-owned atomics BEFORE the first
                // stream is created — otherwise a track that starts before the
                // async apply_player_settings task lands would track the
                // renderer's seeded default (always-disabled) atomics and
                // ignore a later EQ enable for that stream's lifetime.
                let eq_state = self.playback.eq_state.clone();
                shell
                    .task_manager()
                    .spawn("setup_audio", move || async move {
                        let mut engine = audio_engine.lock().await;
                        engine.set_visualizer_callback(viz_callback);
                        engine.set_music_bridge(music_bridge);
                        engine.set_eq_state(eq_state);
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
                let shell_for_libraries = shell.clone();
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
                    // Fetch the multi-library list at login so the nav-bar
                    // trigger knows the count (and hides itself when
                    // N<=1). Persisted active_library_ids was already
                    // restored by AppService::new_with_storage; this
                    // refresh prunes any now-deleted libraries from that
                    // set against the live server (plan §14.4).
                    Task::perform(
                        async move { shell_for_libraries.refresh_libraries().await },
                        |result: anyhow::Result<Vec<nokkvi_data::types::library::Library>>| {
                            match result {
                                Ok(libs) => Message::Library(
                                    crate::app_message::LibraryMessage::Loaded(libs),
                                ),
                                Err(e) => Message::Library(
                                    crate::app_message::LibraryMessage::LoadFailed(format!(
                                        "{e:#}"
                                    )),
                                ),
                            }
                        },
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
            View::Queue
            | View::Playlists
            | View::Radios
            | View::Settings
            | View::PlaylistEditor => {}
        }

        // Trigger a data reload with the active filter
        let load_task = match view {
            View::Albums => Task::done(Message::LoadAlbums),
            View::Songs => Task::done(Message::LoadSongs),
            View::Artists => Task::done(Message::LoadArtists),
            View::Genres => Task::done(Message::LoadGenres),
            View::Queue
            | View::Playlists
            | View::Radios
            | View::Settings
            | View::PlaylistEditor => Task::none(),
        };

        Task::batch([switch_task, load_task])
    }

    /// Cross-view navigation that lands in the Albums view, clears any active
    /// search/filter, and primes a "find-and-expand" target. The albums load
    /// handlers consume the target after each page arrives — paging through
    /// the unfiltered list until the album appears, then dispatching
    /// `FocusAndExpand` so its tracks render inline.
    pub(crate) fn handle_navigate_and_expand_album(&mut self, album_id: String) -> Task<Message> {
        let pending = crate::state::PendingExpand::Album {
            album_id,
            for_browsing_pane: false,
        };
        self.prime_expand(pending.clone());
        // Clearing `library.albums` above flips `is_empty()` to true, so
        // handle_switch_view's Albums arm dispatches LoadAlbums for us.
        let switch_task = self.handle_switch_view(View::Albums);
        Task::batch([switch_task, pending_expand_timeout_task(pending)])
    }

    /// Browsing-pane variant of `handle_navigate_and_expand_album` — switches
    /// the browsing panel to its Albums tab and runs the same find chain
    /// without disrupting the top-pane Queue.
    pub(crate) fn handle_browser_pane_navigate_and_expand_album(
        &mut self,
        album_id: String,
    ) -> Task<Message> {
        let pending = crate::state::PendingExpand::Album {
            album_id,
            for_browsing_pane: true,
        };
        self.prime_expand(pending.clone());
        let switch_task = self.handle_browsing_panel_message(
            crate::views::BrowsingPanelMessage::SwitchView(crate::views::BrowsingView::Albums),
        );
        Task::batch([
            switch_task,
            Task::done(Message::LoadAlbums),
            pending_expand_timeout_task(pending),
        ])
    }

    /// Single source of truth for the navigate-and-expand priming step.
    /// Resets the matching page (search, expansion, viewport, selection),
    /// drops the corresponding library buffer, clears the center-only flag,
    /// and installs `pending` as the active target. Thin wrapper around the
    /// free-function `prime_expand_target` so callers can stay method-chained.
    fn prime_expand(&mut self, pending: crate::state::PendingExpand) {
        prime_expand_target(self, pending);
    }

    /// Drop the in-flight find-and-expand chain (whichever kind) plus any
    /// pending top-pin. Called from cancellation hooks (search edit, sort
    /// change, navigation away, refresh) so the chain doesn't continue
    /// after the user has moved on. The pin only outlives the target by
    /// the brief window between `try_resolve` and the corresponding
    /// `set_children`, so unconditional pin clearing is correct here.
    pub(crate) fn cancel_pending_expand(&mut self) {
        self.pending_expand = crate::state::PendingExpandState::default();
    }

    /// Genre-side mirror of `handle_navigate_and_expand_album`. The find
    /// chain is single-shot since genres don't paginate.
    pub(crate) fn handle_navigate_and_expand_genre(&mut self, genre_id: String) -> Task<Message> {
        let pending = crate::state::PendingExpand::Genre {
            genre_id,
            for_browsing_pane: false,
        };
        self.prime_expand(pending.clone());
        let switch_task = self.handle_switch_view(View::Genres);
        Task::batch([switch_task, pending_expand_timeout_task(pending)])
    }

    /// Browsing-pane variant of `handle_navigate_and_expand_genre`.
    pub(crate) fn handle_browser_pane_navigate_and_expand_genre(
        &mut self,
        genre_id: String,
    ) -> Task<Message> {
        let pending = crate::state::PendingExpand::Genre {
            genre_id,
            for_browsing_pane: true,
        };
        self.prime_expand(pending.clone());
        let switch_task = self.handle_browsing_panel_message(
            crate::views::BrowsingPanelMessage::SwitchView(crate::views::BrowsingView::Genres),
        );
        Task::batch([
            switch_task,
            Task::done(Message::LoadGenres),
            pending_expand_timeout_task(pending),
        ])
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
        self.try_resolve_pending_expand_with::<crate::update::GenreSpec>()
    }

    /// Artist-side mirror of `handle_navigate_and_expand_album`. Lands on
    /// the Artists view, clears any active search/filter, and primes a
    /// find-and-expand target. The artists load handlers consume it after
    /// each page arrives.
    pub(crate) fn handle_navigate_and_expand_artist(&mut self, artist_id: String) -> Task<Message> {
        let pending = crate::state::PendingExpand::Artist {
            artist_id,
            for_browsing_pane: false,
        };
        self.prime_expand(pending.clone());
        let switch_task = self.handle_switch_view(View::Artists);
        Task::batch([switch_task, pending_expand_timeout_task(pending)])
    }

    /// Browsing-pane variant of `handle_navigate_and_expand_artist`.
    pub(crate) fn handle_browser_pane_navigate_and_expand_artist(
        &mut self,
        artist_id: String,
    ) -> Task<Message> {
        let pending = crate::state::PendingExpand::Artist {
            artist_id,
            for_browsing_pane: true,
        };
        self.prime_expand(pending.clone());
        let switch_task = self.handle_browsing_panel_message(
            crate::views::BrowsingPanelMessage::SwitchView(crate::views::BrowsingView::Artists),
        );
        Task::batch([
            switch_task,
            Task::done(Message::LoadArtists),
            pending_expand_timeout_task(pending),
        ])
    }

    /// Artist-side mirror of `try_resolve_pending_expand_album`. After each
    /// artists page lands, look for the pending target in `library.artists`
    /// and either dispatch FocusAndExpand at the right viewport position,
    /// give up if fully loaded, wait if still loading, or kick the next
    /// page (force-loaded) if more remain.
    pub(crate) fn try_resolve_pending_expand_artist(&mut self) -> Option<Task<Message>> {
        self.try_resolve_pending_expand_with::<crate::update::ArtistSpec>()
    }

    /// After each albums page lands, look for the pending expand target in
    /// `library.albums`. Returns `Some(task)` if the helper acted (found and
    /// dispatched, fully-loaded miss, or kicked the next page) and `None` if
    /// it should be retried after the next page arrives. Albums force-load
    /// pages until the buffer is exhausted; CenterOnPlaying (Shift+C) centers
    /// the row without dispatching FocusAndExpand.
    pub(crate) fn try_resolve_pending_expand_album(&mut self) -> Option<Task<Message>> {
        self.try_resolve_pending_expand_with::<crate::update::AlbumSpec>()
    }

    /// Fired ~2s after a navigate-and-expand handler primes a chain. Compares
    /// the carried `PendingExpand` against the currently pending target via
    /// `pending_expand_timeout_toast` and surfaces a "Finding {entity}…" toast
    /// when they still match — so stale timeouts from superseded clicks stay
    /// silent. Always returns `Task::none()` (the toast is a side effect on
    /// `app.toast`).
    pub(crate) fn handle_pending_expand_timeout(
        &mut self,
        pending: crate::state::PendingExpand,
    ) -> Task<Message> {
        pending_expand_timeout_toast(self, &pending);
        Task::none()
    }

    /// Song-side mirror of `try_resolve_pending_expand_album`. Songs aren't
    /// expandable, so this always centers and never dispatches a
    /// `FocusAndExpand` — `pending_expand.center_only` is implicit here.
    /// `SongSpec::focus_and_expand` returns `None`, which the generic body
    /// treats as effective-center-only (no top pin, viewport_offset = idx).
    pub(crate) fn try_resolve_pending_expand_song(&mut self) -> Option<Task<Message>> {
        self.try_resolve_pending_expand_with::<crate::update::SongSpec>()
    }

    /// CenterOnPlaying (Shift+C) fallback: clear the matching view's search
    /// and filter, drop its loaded buffer, install a center-only
    /// `PendingExpand` target, and kick the find chain. The chain force-loads
    /// pages until the playing item appears, then centers it without
    /// dispatching FocusAndExpand. `PendingExpand::load_message` picks the
    /// right `Message::Load*` for the carried variant — Albums/Artists/Songs
    /// paginate, Genres is single-shot under the hood.
    pub(crate) fn start_center_on_playing_chain(
        &mut self,
        pending: crate::state::PendingExpand,
    ) -> Task<Message> {
        let load = pending.load_message();
        self.prime_expand(pending.clone());
        self.pending_expand.center_only = true;
        Task::batch([Task::done(load), pending_expand_timeout_task(pending)])
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
            View::Queue
            | View::Playlists
            | View::Radios
            | View::Settings
            | View::PlaylistEditor => None,
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
            View::Queue
            | View::Playlists
            | View::Radios
            | View::Settings
            | View::PlaylistEditor => {}
        }

        // Trigger a data reload with the active filter
        let load_task = match view {
            View::Albums => Task::done(Message::LoadAlbums),
            View::Songs => Task::done(Message::LoadSongs),
            View::Artists => Task::done(Message::LoadArtists),
            View::Genres => Task::done(Message::LoadGenres),
            View::Queue
            | View::Playlists
            | View::Radios
            | View::Settings
            | View::PlaylistEditor => Task::none(),
        };

        Task::batch([switch_task, load_task])
    }
}
