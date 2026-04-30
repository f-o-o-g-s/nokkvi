#![warn(unreachable_pub)]
//! Nokkvi
//!
//! A Rust/Iced client for Navidrome music servers.
//!
//! # Module Structure
//!
//! The application is split across multiple files for maintainability:
//! - `main.rs` (this file): Entry point, module declarations, core types
//! - `app_message.rs`: Message enum with all 70+ variants
//! - `update/`: Modular update handlers organized by domain
//! - `app_view.rs`: view() function and rendering helpers
//! - `state.rs`: Consolidated state structs (PlaybackState, ScrobbleState, etc.)

mod app_message;
mod app_view;
mod config_writer;
mod embedded_svg;
mod hotkeys;
mod services;
mod state;
#[cfg(test)]
mod test_helpers;
mod theme;
mod theme_config;
mod update;
mod views;
mod visualizer_config;
mod widgets;

// Re-export Message from app_message for use by other modules
use std::time::Duration;

pub use app_message::Message;
use iced::{Event, Task, Theme, event, keyboard, time, window::settings::PlatformSpecific};
use nokkvi_data::{backend::app_service::AppService, types::hotkey_config::HotkeyConfig};
use tracing::debug;

// ============================================================================
// SECTION: Core Enums (KEEP IN main.rs)
// ============================================================================

/// Top-level screen routing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Login,
    Home,
}

/// Navigation view within Home screen
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Albums,
    Queue,
    Songs,
    Artists,
    Genres,
    Playlists,
    Radios,
    Settings,
}

// ============================================================================
// SECTION: Application State (KEEP IN main.rs)
// ============================================================================

/// Root application state container
///
/// This struct holds all global state. Page-specific state lives in the
/// respective page structs (e.g., `AlbumsPage`, `QueuePage`).
pub struct Nokkvi {
    // -------------------------------------------------------------------------
    // Page Components (extracted pages own their internal state)
    // -------------------------------------------------------------------------
    pub login_page: views::LoginPage,
    pub albums_page: views::AlbumsPage,
    pub artists_page: views::ArtistsPage,
    pub genres_page: views::GenresPage,
    pub playlists_page: views::PlaylistsPage,
    pub queue_page: views::QueuePage,
    pub songs_page: views::SongsPage,
    pub radios_page: views::RadiosPage,
    pub settings_page: views::SettingsPage,
    pub similar_page: views::SimilarPage,

    // -------------------------------------------------------------------------
    // Core Services
    // -------------------------------------------------------------------------
    pub app_service: Option<AppService>,
    pub sfx_engine: nokkvi_data::audio::SfxEngine,
    /// Cached StateStorage handle for reuse after logout (avoids redb exclusive lock conflict)
    pub cached_storage: Option<nokkvi_data::services::state_storage::StateStorage>,

    // -------------------------------------------------------------------------
    // Screen/Navigation State
    // -------------------------------------------------------------------------
    pub screen: Screen,
    pub current_view: View,
    /// View to restore when closing Settings (captured on Settings open)
    pub pre_settings_view: View,

    // -------------------------------------------------------------------------
    // Auto-login flag (credentials stored in LoginPage)
    // -------------------------------------------------------------------------
    pub should_auto_login: bool,
    /// Stored session for JWT-based auto-login.
    pub stored_session: Option<crate::state::StoredSession>,

    // -------------------------------------------------------------------------
    // Library Data (consolidated data vectors + counts)
    // -------------------------------------------------------------------------
    pub library: crate::state::LibraryData,

    /// Similar songs state — populated by getSimilarSongs2 / getTopSongs API calls
    pub similar_songs: Option<crate::state::SimilarSongsState>,
    /// Generation counter for stale response rejection
    pub similar_songs_generation: u64,

    // -------------------------------------------------------------------------
    // Consolidated State Structs
    // -------------------------------------------------------------------------
    pub active_playback: crate::state::ActivePlayback,
    pub playback: crate::state::PlaybackState,
    pub scrobble: crate::state::ScrobbleState,
    pub modes: crate::state::PlaybackModes,
    pub sfx: crate::state::SfxState,
    pub engine: crate::state::EngineState,
    pub artwork: crate::state::ArtworkState,
    pub window: crate::state::WindowState,
    /// Snapshot of the player bar's responsive layout (which modes have folded
    /// into the kebab and whether the transport row has collapsed to 3 buttons).
    /// Recomputed on `WindowResized` with per-mode hysteresis (see
    /// `widgets::player_bar::compute_layout`) so a slow drag near a threshold
    /// doesn't flicker the layout.
    pub player_bar_layout: crate::widgets::player_bar::PlayerBarLayout,
    pub toast: crate::state::ToastState,
    pub text_input_dialog: crate::widgets::text_input_dialog::TextInputDialogState,
    pub info_modal: crate::widgets::info_modal::InfoModalState,
    pub about_modal: crate::widgets::about_modal::AboutModalState,
    /// Default-playlist picker overlay state. `Some` = picker is open.
    pub default_playlist_picker:
        Option<crate::widgets::default_playlist_picker::DefaultPlaylistPickerState>,

    /// The single overlay menu currently open, if any. Mutated only by
    /// `Message::SetOpenMenu` so opening a new menu implicitly closes any
    /// previously open one. See `app_message::OpenMenu` for the variants.
    pub open_menu: Option<crate::app_message::OpenMenu>,

    // -------------------------------------------------------------------------
    // Misc State
    // -------------------------------------------------------------------------
    pub last_queue_current_index: Option<usize>,

    // -------------------------------------------------------------------------
    // Playlist Edit Mode (split-view)
    // -------------------------------------------------------------------------
    pub playlist_edit: Option<nokkvi_data::types::playlist_edit::PlaylistEditState>,
    /// Identity of the playlist currently loaded in the queue.
    /// Set on PlayPlaylist, cleared on non-playlist play.
    pub active_playlist_info: Option<crate::state::ActivePlaylistContext>,
    pub browsing_panel: Option<views::BrowsingPanel>,
    pub pane_focus: crate::state::PaneFocus,
    /// Active cross-pane drag from browsing panel to queue (None when idle)
    pub cross_pane_drag: Option<crate::state::CrossPaneDragState>,
    /// Last known cursor position (tracked via event subscription when panel is open)
    pub last_cursor_position: iced::Point,
    /// Press origin for cross-pane drag threshold detection (cleared on release)
    pub cross_pane_drag_press_origin: Option<iced::Point>,
    /// Center item index snapshotted at press time (before drag activation).
    /// Captured right after the button's SlotListSetOffset processes (widget
    /// messages run before subscription messages in Iced), so selected_offset
    /// is guaranteed accurate. Immune to auto-follow or viewport mutations.
    pub cross_pane_drag_pressed_item: Option<usize>,
    /// Snapshotted selection count at press time. 1 = single item, >1 = batch.
    pub cross_pane_drag_selection_count: usize,
    /// Pending queue insertion position for cross-pane drag drop.
    /// Set by `handle_cross_pane_drag_released` before dispatching the
    /// `AddCenterToQueue` message; consumed by the update handler to
    /// insert at position instead of appending.
    pub pending_queue_insert_position: Option<usize>,

    // -------------------------------------------------------------------------
    // Audio Visualizer
    // -------------------------------------------------------------------------
    pub visualizer: Option<widgets::visualizer::Visualizer>,
    pub visualizer_config: crate::visualizer_config::SharedVisualizerConfig,

    // -------------------------------------------------------------------------
    // MPRIS D-Bus Integration
    // -------------------------------------------------------------------------
    pub mpris_connection: Option<services::mpris::MprisConnection>,
    /// Last position (µs) pushed to MPRIS — used to detect seek discontinuities
    pub last_mpris_position_us: i64,

    // -------------------------------------------------------------------------
    // System Tray (StatusNotifierItem)
    // -------------------------------------------------------------------------
    /// Handle to push state into the running tray (None when disabled).
    pub tray_connection: Option<services::tray::TrayConnection>,
    /// Whether the window is currently hidden into the tray.
    pub tray_window_hidden: bool,
    /// Captured id of the main window — needed to issue `window::set_mode`.
    pub main_window_id: Option<iced::window::Id>,

    // -------------------------------------------------------------------------
    // Hotkey Configuration (loaded from redb, used by subscription)
    // -------------------------------------------------------------------------
    pub hotkey_config: HotkeyConfig,

    // -------------------------------------------------------------------------
    // General Settings (loaded from redb via PlayerSettingsLoaded)
    // -------------------------------------------------------------------------
    /// Whether scrobbling is enabled (default: true)
    pub scrobbling_enabled: bool,
    /// Scrobble threshold as fraction of track duration (default: 0.50)
    pub scrobble_threshold: f32,
    /// Start view name for initial view on login (default: "Queue")
    pub start_view: String,
    /// One-shot flag: has start_view been applied yet?
    pub start_view_applied: bool,
    /// Whether stable viewport mode is enabled (default: true)
    pub stable_viewport: bool,
    /// Whether auto-follow playing track is enabled (default: true)
    pub auto_follow_playing: bool,
    /// Whether the Artists view shows only album artists
    pub show_album_artists_only: bool,
    /// Whether to suppress the toast shown on Navidrome library-refresh events
    /// (default: false = toasts shown).
    pub suppress_library_refresh_toasts: bool,
    /// Whether the system tray icon (StatusNotifierItem) is registered.
    pub show_tray_icon: bool,
    /// Whether the window's close button hides into the tray instead of quitting.
    pub close_to_tray: bool,
    /// What Enter does in the Songs view (default: PlayAll)
    pub enter_behavior: nokkvi_data::types::player_settings::EnterBehavior,
    /// Local filesystem prefix for opening files in the file manager (default: empty = not set)
    pub local_music_path: String,
    /// Page size for library pagination chunks
    pub library_page_size: nokkvi_data::types::player_settings::LibraryPageSize,
    /// Transient flag: suppress the next auto-center triggered by a track change.
    /// Set when a click-initiated play fires, cleared after consumption.
    pub suppress_next_auto_center: bool,
    /// Pending CenterOnPlaying retry: set when the target item isn't in the
    /// loaded PagedBuffer and a search-based reload was dispatched. When the
    /// data-loaded handler fires, it re-dispatches CenterOnPlaying.
    pub pending_center_on_playing: bool,
    /// Default playlist ID for quick-add (None = no default set)
    pub default_playlist_id: Option<String>,
    /// Default playlist display name (for settings UI readout)
    pub default_playlist_name: String,
    /// Whether to skip the Add to Playlist dialog and use the default playlist directly
    pub quick_add_to_playlist: bool,
    /// Whether the queue view's header shows the default playlist chip
    pub queue_show_default_playlist: bool,
    /// Whether all settings (including defaults) are written to config.toml
    pub verbose_config: bool,
    /// Artwork resolution for the large artwork panel (configurable in Settings)
    pub artwork_resolution: nokkvi_data::types::player_settings::ArtworkResolution,
    /// Extracted backend server version (e.g. from Navidrome)
    pub server_version: Option<String>,

    // -------------------------------------------------------------------------
    // Progress Tracking (polled from Tick for live toast updates)
    // -------------------------------------------------------------------------
    pub active_progress: Vec<nokkvi_data::types::progress::ProgressHandle>,
}

// ============================================================================
// SECTION: Default Implementation (KEEP IN main.rs)
// ============================================================================

impl Default for Nokkvi {
    fn default() -> Self {
        // Load server_url + username from config.toml
        let (server_url, username) = nokkvi_data::credentials::load_credentials()
            .unwrap_or_else(|| ("http://localhost:4533".to_string(), String::new()));

        // Try to load stored session (JWT + subsonic credential) from redb
        let stored_session = nokkvi_data::credentials::load_session();
        let should_auto_login = stored_session.is_some();
        let stored_session = stored_session.map(|(jwt, sub)| crate::state::StoredSession {
            server_url: server_url.clone(),
            username: username.clone(),
            jwt_token: jwt,
            subsonic_credential: sub,
        });

        debug!(
            " Auto-login (session resume) enabled: {}",
            should_auto_login
        );

        // Create login page with pre-filled server_url and username (no password)
        let login_page = views::LoginPage::with_credentials(server_url, username, String::new());

        Self {
            login_page,
            albums_page: views::AlbumsPage::new(),
            artists_page: views::ArtistsPage::new(),
            genres_page: views::GenresPage::new(),
            playlists_page: views::PlaylistsPage::new(),
            queue_page: views::QueuePage::new(),
            songs_page: views::SongsPage::new(),
            radios_page: views::RadiosPage::new(),
            settings_page: views::SettingsPage::new(),
            similar_page: views::SimilarPage::new(),
            app_service: None,
            cached_storage: None,
            sfx_engine: nokkvi_data::audio::SfxEngine::default(),
            screen: Screen::Login,
            current_view: View::Queue,
            pre_settings_view: View::Queue,
            should_auto_login,
            stored_session,
            library: crate::state::LibraryData::default(),
            similar_songs: None,
            similar_songs_generation: 0,
            // General settings defaults (overridden by PlayerSettingsLoaded)
            scrobbling_enabled: true,
            scrobble_threshold: 0.50,
            start_view: "Queue".to_string(),
            start_view_applied: false,
            stable_viewport: true,
            auto_follow_playing: true,
            show_album_artists_only: false,
            suppress_library_refresh_toasts: false,
            show_tray_icon: false,
            close_to_tray: false,
            enter_behavior: nokkvi_data::types::player_settings::EnterBehavior::default(),
            local_music_path: String::new(),
            library_page_size: nokkvi_data::types::player_settings::LibraryPageSize::Default,
            suppress_next_auto_center: false,
            pending_center_on_playing: false,
            default_playlist_id: None,
            default_playlist_name: String::new(),
            quick_add_to_playlist: false,
            queue_show_default_playlist: false,
            verbose_config: false,
            artwork_resolution: nokkvi_data::types::player_settings::ArtworkResolution::Default,
            server_version: None,
            // Consolidated state structs with defaults
            active_playback: crate::state::ActivePlayback::default(),
            playback: crate::state::PlaybackState::default(),
            scrobble: crate::state::ScrobbleState::default(),
            modes: crate::state::PlaybackModes::default(),
            sfx: crate::state::SfxState::default(),
            engine: crate::state::EngineState::default(),
            artwork: crate::state::ArtworkState::default(),
            window: crate::state::WindowState::default(),
            player_bar_layout: crate::widgets::player_bar::PlayerBarLayout::default(),
            // Misc state
            last_queue_current_index: None,
            playlist_edit: None,
            active_playlist_info: None,
            browsing_panel: None,
            pane_focus: crate::state::PaneFocus::Queue,
            cross_pane_drag: None,
            last_cursor_position: iced::Point::ORIGIN,
            cross_pane_drag_press_origin: None,
            cross_pane_drag_pressed_item: None,
            cross_pane_drag_selection_count: 1,
            pending_queue_insert_position: None,
            visualizer: None,
            visualizer_config: crate::visualizer_config::create_shared_config(),
            mpris_connection: None,
            last_mpris_position_us: 0,
            tray_connection: None,
            tray_window_hidden: false,
            main_window_id: None,
            hotkey_config: HotkeyConfig::default(),
            toast: crate::state::ToastState::default(),
            text_input_dialog: crate::widgets::text_input_dialog::TextInputDialogState::default(),
            info_modal: crate::widgets::info_modal::InfoModalState::default(),
            about_modal: crate::widgets::about_modal::AboutModalState::default(),
            default_playlist_picker: None,
            open_menu: None,
            active_progress: Vec::new(),
        }
    }
}

// ============================================================================
// SECTION: Iced Application Trait Methods (KEEP IN main.rs)
// ============================================================================

impl Nokkvi {
    /// Window title — dynamic based on playback state.
    ///
    /// Daemon-mode signature: the `_window` id is unused because nokkvi only
    /// ever has a single main window.
    pub fn title(&self, _window: iced::window::Id) -> String {
        if self.active_playback.is_radio() {
            let status = if self.playback.playing {
                ""
            } else {
                " (Paused)"
            };
            if let Some(station) = self.active_playback.radio_station() {
                format!("{}{} \u{2014} Nokkvi", station.name, status)
            } else {
                format!("{}{} \u{2014} Nokkvi", self.playback.title, status)
            }
        } else if self.playback.has_track() {
            let status = if self.playback.playing {
                ""
            } else {
                " (Paused)"
            };
            if self.playback.artist.is_empty() {
                format!("{}{} \u{2014} Nokkvi", self.playback.title, status)
            } else {
                format!(
                    "{} - {}{} \u{2014} Nokkvi",
                    self.playback.artist, self.playback.title, status
                )
            }
        } else {
            "Nokkvi".to_string()
        }
    }

    /// Application theme — custom Gruvbox palette for default widget styles.
    ///
    /// Daemon-mode signature: `_window` is unused (single window only).
    pub fn theme(&self, _window: iced::window::Id) -> Theme {
        theme::iced_theme()
    }

    /// Global subscriptions: tick timer, keyboard, window events
    pub fn subscription(&self) -> iced::Subscription<Message> {
        let tick = time::every(Duration::from_millis(100))
            .map(|_| Message::Playback(app_message::PlaybackMessage::Tick)); // 10 times per second for smooth position updates

        // Audio rendering no longer driven by iced subscription.
        // It runs on a dedicated std::thread with a 5ms timer (see engine.rs).

        // Keyboard events: use event::listen_with (not keyboard::listen) to
        // receive ALL key events regardless of widget capture status.
        // keyboard::listen() filters for Status::Ignored, which means focused
        // text_input widgets silently swallow Escape/Enter before our hotkey
        // system ever sees them.
        let keyboard = event::listen_with(|event, status, _window| match event {
            Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                Some(Message::RawKeyEvent(key, modifiers, status))
            }
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                Some(Message::ModifiersChanged(modifiers))
            }
            _ => None,
        });

        let window_events = event::listen_with(|event, status, _window| match event {
            Event::Window(iced::window::Event::Resized(size)) => {
                Some(Message::WindowResized(size.width, size.height))
            }
            Event::Window(iced::window::Event::Rescaled(scale_factor)) => {
                Some(Message::ScaleFactorChanged(scale_factor))
            }
            // Cross-pane drag mouse tracking (handlers no-op when panel is closed).
            // CursorMoved: skip when a widget (e.g. scrollbar) captured the event —
            // prevents scrollbar drags from exceeding the 5px threshold and
            // activating the cross-pane drag state machine.
            // ButtonPressed/Released: always emit — Iced buttons also capture these,
            // so filtering them would break cross-pane drag initiation.
            Event::Mouse(iced::mouse::Event::CursorMoved { position })
                if status != iced::event::Status::Captured =>
            {
                Some(Message::CrossPaneDragMoved(position))
            }
            Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) => {
                Some(Message::CrossPaneDragPressed)
            }
            Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                Some(Message::CrossPaneDragReleased)
            }
            _ => None,
        });

        // Forward events to login page for tab navigation when on Login screen
        let login_events = if self.screen == Screen::Login {
            event::listen().map(|e| Message::Login(views::LoginMessage::Event(e)))
        } else {
            iced::Subscription::none()
        };

        // MPRIS D-Bus server for Linux desktop integration
        let mpris = iced::Subscription::run(services::mpris::run).map(Message::Mpris);

        // System tray (StatusNotifierItem). Conditionally spawned: when the
        // user toggles `show_tray_icon` off, the subscription disappears
        // from the batch and iced cancels it, which closes the command
        // channel and tears down the ksni service on its dedicated thread.
        let tray = if self.show_tray_icon {
            iced::Subscription::run(services::tray::run).map(Message::Tray)
        } else {
            iced::Subscription::none()
        };

        // Window lifecycle: capture the main window's id on first open and
        // intercept close-button presses so we can branch on the
        // close-to-tray setting.
        let window_open_sub = iced::window::open_events().map(Message::WindowOpened);
        let window_close_sub = iced::window::close_requests().map(Message::WindowCloseRequested);

        // Config file watcher for hot-reloading both visualizer AND theme settings
        let config_watcher = iced::Subscription::run(|| {
            futures::stream::StreamExt::flat_map(
                crate::visualizer_config::config_watcher_subscription(),
                |opt| {
                    if let Some(config) = opt {
                        futures::stream::iter(vec![
                            Message::VisualizerConfigChanged(config),
                            Message::ThemeConfigReloaded,
                            Message::SettingsConfigReloaded,
                        ])
                    } else {
                        futures::stream::iter(vec![])
                    }
                },
            )
        });

        // Subscription for repeat-one loop scrobble events.
        // `loop_subscription::run()` reads from the global OnceLock receiver
        // registered at login time; each emitted String is a looping song ID.
        let loop_sub = iced::Subscription::run(services::loop_subscription::run)
            .map(|song_id| Message::Scrobble(app_message::ScrobbleMessage::TrackLooped(song_id)));

        // Queue-changed subscription: fires after each track auto-advance
        // (post-consume, post-refresh_from_queue). Guarantees the UI gets
        // the correct queue state after consume mode removes a song.
        let queue_changed_sub = iced::Subscription::run(services::queue_changed_subscription::run)
            .map(|()| Message::LoadQueue);

        let sse_sub =
            iced::Subscription::run(services::navidrome_sse::run).map(|event| match event {
                services::navidrome_sse::SseEvent::LibraryChanged {
                    album_ids,
                    is_wildcard,
                } => Message::LibraryChanged {
                    album_ids,
                    is_wildcard,
                },
            });

        let task_status_sub = iced::Subscription::run(services::task_subscription::run)
            .map(|(handle, status)| Message::TaskStatusChanged(handle, status));

        iced::Subscription::batch(vec![
            tick,
            keyboard,
            window_events,
            login_events,
            mpris,
            tray,
            window_open_sub,
            window_close_sub,
            config_watcher,
            loop_sub,
            queue_changed_sub,
            sse_sub,
            task_status_sub,
        ])
    }
}

impl Nokkvi {
    // =========================================================================
    // SECTION: Shell Helpers (KEEP IN main.rs)
    // =========================================================================

    /// Run an async operation on `AppService`, returning a `Task<Message>`.
    ///
    /// Encapsulates the pervasive pattern:
    /// ```ignore
    /// if let Some(shell) = &self.app_service {
    ///     let shell = shell.clone();
    ///     Task::perform(async move { /* use shell */ }, |result| /* map to Message */)
    /// } else { Task::none() }
    /// ```
    ///
    /// Returns `Task::none()` if `app_service` is not yet initialized (pre-login).
    pub(crate) fn shell_task<F, Fut, T>(
        &self,
        f: F,
        map: impl FnOnce(T) -> Message + Send + 'static,
    ) -> Task<Message>
    where
        F: FnOnce(AppService) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = T> + Send,
        T: Send + 'static,
    {
        if let Some(shell) = &self.app_service {
            let shell = shell.clone();
            Task::perform(async move { f(shell).await }, map)
        } else {
            // Logged at debug, not warn: this fires during expected transition
            // windows (boot before session resume completes, logout while
            // subscription-driven messages are still in flight) — not just
            // when a caller invokes us in a context they shouldn't. The file
            // log still captures it for diagnosis; stderr stays clean.
            tracing::debug!(
                "shell_task called before app_service initialized (pre-login?) — task dropped"
            );
            Task::none()
        }
    }

    /// Fire-and-forget an async operation on `AppService` via the task manager.
    ///
    /// Encapsulates:
    /// ```ignore
    /// if let Some(shell) = &self.app_service {
    ///     let shell = shell.clone();
    ///     shell.task_manager().spawn_result("label", move || async move { ... });
    /// }
    /// ```
    pub(crate) fn shell_spawn<F, Fut>(&self, label: &'static str, f: F)
    where
        F: FnOnce(AppService) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
    {
        if let Some(shell) = &self.app_service {
            let shell = shell.clone();
            shell
                .task_manager()
                .spawn_result(label, move || async move { f(shell).await });
        }
    }

    // =========================================================================
    // SECTION: Helpers (KEEP IN main.rs)
    // =========================================================================

    /// Filter albums based on search query (client-side).
    /// Returns `Cow::Borrowed` when no search is active (zero-cost).
    pub fn filter_albums(
        &self,
    ) -> std::borrow::Cow<'_, [nokkvi_data::backend::albums::AlbumUIViewData]> {
        nokkvi_data::utils::search::filter_items(
            &self.library.albums,
            &self.albums_page.common.search_query,
        )
    }

    /// Filter queue songs based on search query (client-side).
    /// Returns `Cow::Borrowed` when no search is active (zero-cost).
    pub fn filter_queue_songs(
        &self,
    ) -> std::borrow::Cow<'_, [nokkvi_data::backend::queue::QueueSongUIViewData]> {
        nokkvi_data::utils::search::filter_items(
            &self.library.queue_songs,
            &self.queue_page.common.search_query,
        )
    }

    /// Filter radio stations based on search query (client-side).
    /// Returns `Cow::Borrowed` when no search is active (zero-cost).
    pub fn filter_radio_stations(
        &self,
    ) -> std::borrow::Cow<'_, [nokkvi_data::types::radio_station::RadioStation]> {
        nokkvi_data::utils::search::filter_items(
            &self.library.radio_stations,
            &self.radios_page.common.search_query,
        )
    }

    /// Collect song IDs from the current queue (for dirty detection, save, etc.)
    pub fn queue_song_ids(&self) -> Vec<String> {
        self.library
            .queue_songs
            .iter()
            .map(|s| s.id.clone())
            .collect()
    }

    /// Sort queue songs based on current sort mode and sort order (client-side).
    ///
    /// Short-circuits when `(mode, ascending, queue_len)` matches the last
    /// applied signature — re-toggling the same sort with no length change is
    /// a no-op. String sorts use `sort_by_cached_key` so each item's
    /// lowercased key is built exactly once per sort instead of N×log(N) times.
    pub fn sort_queue_songs(&mut self) {
        use views::QueueSortMode;

        let sort_mode = self.queue_page.queue_sort_mode;
        let ascending = self.queue_page.common.sort_ascending;
        let len = self.library.queue_songs.len();
        let signature = (sort_mode, ascending, len);

        if self.queue_page.last_sort_signature == Some(signature) {
            return;
        }

        debug!(
            " Sorting queue by {:?} ({}, {} items)",
            sort_mode,
            if ascending { "ASC" } else { "DESC" },
            len
        );

        match sort_mode {
            QueueSortMode::Title
            | QueueSortMode::Artist
            | QueueSortMode::Album
            | QueueSortMode::Genre => {
                self.library.queue_songs.sort_by_cached_key(|s| {
                    let field = match sort_mode {
                        QueueSortMode::Title => &s.title,
                        QueueSortMode::Artist => &s.artist,
                        QueueSortMode::Album => &s.album,
                        QueueSortMode::Genre => &s.genre,
                        _ => unreachable!("string sort branch covers only string variants"),
                    };
                    field.to_lowercase()
                });
                if !ascending {
                    self.library.queue_songs.reverse();
                }
            }
            QueueSortMode::Duration => {
                self.library.queue_songs.sort_by_key(|s| s.duration_seconds);
                if !ascending {
                    self.library.queue_songs.reverse();
                }
            }
            QueueSortMode::Rating => {
                // Highest rating first by default; descending toggle flips.
                self.library
                    .queue_songs
                    .sort_by_key(|s| std::cmp::Reverse(s.rating.unwrap_or(0)));
                if !ascending {
                    self.library.queue_songs.reverse();
                }
            }
            QueueSortMode::MostPlayed => {
                self.library
                    .queue_songs
                    .sort_by_key(|s| std::cmp::Reverse(s.play_count.unwrap_or(0)));
                if !ascending {
                    self.library.queue_songs.reverse();
                }
            }
        }

        self.queue_page.last_sort_signature = Some(signature);

        // Reset slot list to first item after resort
        self.queue_page
            .common
            .slot_list
            .set_offset(0, self.library.queue_songs.len());
    }

    /// Sort radio stations based on current sort order (client-side). Same
    /// short-circuit and `sort_by_cached_key` policy as `sort_queue_songs`.
    pub fn sort_radio_stations(&mut self) {
        let ascending = self.radios_page.common.sort_ascending;
        let len = self.library.radio_stations.len();
        let signature = (ascending, len);

        if self.radios_page.last_sort_signature == Some(signature) {
            return;
        }

        debug!(
            " Sorting radios by Name ({}, {} items)",
            if ascending { "ASC" } else { "DESC" },
            len
        );

        self.library
            .radio_stations
            .sort_by_cached_key(|s| s.name.to_lowercase());
        if !ascending {
            self.library.radio_stations.reverse();
        }

        self.radios_page.last_sort_signature = Some(signature);

        self.radios_page
            .common
            .handle_set_offset(0, self.library.radio_stations.len());
    }

    // =========================================================================
    // SECTION: Toast Convenience Methods
    // =========================================================================

    /// Push an Info-level toast notification
    pub fn toast_info(&mut self, msg: impl Into<String>) {
        self.toast.push(nokkvi_data::types::toast::Toast::new(
            msg,
            nokkvi_data::types::toast::ToastLevel::Info,
        ));
    }

    /// Push a Success-level toast notification
    pub fn toast_success(&mut self, msg: impl Into<String>) {
        self.toast.push(nokkvi_data::types::toast::Toast::new(
            msg,
            nokkvi_data::types::toast::ToastLevel::Success,
        ));
    }

    /// Push a Warning-level toast notification
    pub fn toast_warn(&mut self, msg: impl Into<String>) {
        self.toast.push(nokkvi_data::types::toast::Toast::new(
            msg,
            nokkvi_data::types::toast::ToastLevel::Warning,
        ));
    }

    /// Push an Error-level toast notification
    pub fn toast_error(&mut self, msg: impl Into<String>) {
        self.toast.push(nokkvi_data::types::toast::Toast::new(
            msg,
            nokkvi_data::types::toast::ToastLevel::Error,
        ));
    }

    #[allow(dead_code)]
    pub(crate) fn should_scrobble_current_track(&self) -> bool {
        self.active_playback.is_queue()
            && self
                .scrobble
                .should_scrobble(self.playback.duration, self.scrobble_threshold)
    }
}

// ============================================================================
// SECTION: Entry Point
// ============================================================================

pub fn main() -> iced::Result {
    // Handle --version / --help before tracing init so these short-lived
    // invocations don't truncate ~/.local/state/nokkvi/nokkvi.log.
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-V" | "--version" => {
                println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "-h" | "--help" => {
                print_cli_help();
                return Ok(());
            }
            _ => {}
        }
    }

    // Initialize tracing.
    //
    // Defaults (overridable via RUST_LOG, which applies to both layers):
    //   - stderr: warn+ only — quiet, signal-only output for terminal launches.
    //   - file (~/.local/state/nokkvi/nokkvi.log): full debug context for bug reports.
    //
    //   RUST_LOG=info ./nokkvi             # info+ on terminal and in file
    //   RUST_LOG=debug ./nokkvi            # full debug on both
    //   RUST_LOG=trace ./nokkvi            # very verbose
    //   RUST_LOG=nokkvi::audio=trace       # narrow trace to one module
    use tracing_subscriber::{EnvFilter, Layer, layer::SubscriberExt, util::SubscriberInitExt};

    // Verbose filter used by the file layer when RUST_LOG isn't set. Suppresses
    // third-party noise so the bug-report file stays focused on our own logs.
    let file_default_filter = [
        // Our application: debug level for all diagnostic info
        "nokkvi=debug",
        "nokkvi_data=debug",
        // HTTP client: only show warnings (suppress connection pooling spam)
        "hyper=warn",
        "hyper_util=warn",
        "reqwest=warn",
        // Graphics/GPU: only errors (suppress shader compilation, adapter info)
        "wgpu=error",
        "wgpu_core=error",
        "wgpu_hal=error",
        "naga=error",
        // Iced framework internals: only warnings
        "iced_wgpu=warn",
        "iced_graphics=warn",
        "iced_tiny_skia=warn",
        "iced_core=warn",
        "cosmic_text=warn",
        // Windowing: only warnings (suppress Wayland global binding logs)
        "winit=warn",
        "sctk=warn",
        "calloop=warn",
        // Audio format detection: only warnings
        "symphonia=warn",
        "symphonia_core=warn",
        "symphonia_bundle_flac=warn",
        "symphonia_bundle_mp3=warn",
        // TLS/crypto: only errors
        "rustls=error",
        // Default for anything else not explicitly listed
        "info",
    ]
    .join(",");

    // stderr layer: warn+ by default, plus info-level auth lifecycle and
    // one-time data migrations so terminal launchers see "Resuming session…"
    // / "Login successful" / "Login failed: …" / "moved app.redb …" without
    // needing RUST_LOG. RUST_LOG overrides.
    let stderr_layer =
        tracing_subscriber::fmt::layer()
            .with_target(false)
            .with_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                EnvFilter::new("warn,nokkvi::auth=info,nokkvi::migration=info")
            }));

    // File log layer: full debug context written to ~/.local/state/nokkvi/nokkvi.log.
    // Captures everything for bug reports, including launches from Hyprland keybinds
    // with no visible terminal. Truncated on each startup.
    let file_layer = nokkvi_data::utils::paths::get_log_path()
        .ok()
        .and_then(|path| {
            std::fs::File::create(path).ok().map(|file| {
                tracing_subscriber::fmt::layer()
                    .with_target(true)
                    .with_ansi(false)
                    .with_writer(std::sync::Mutex::new(file))
                    .with_filter(
                        EnvFilter::try_from_default_env()
                            .unwrap_or_else(|_| EnvFilter::new(&file_default_filter)),
                    )
            })
        });

    tracing_subscriber::registry()
        .with(stderr_layer)
        .with(file_layer)
        .init();

    // Run the legacy → XDG-state-dir migration now that tracing is up,
    // and before iced::daemon spins up `Nokkvi::default()` (which loads
    // the session from app.redb via `credentials::load_session`).
    nokkvi_data::utils::paths::migrate_to_state_dir();

    iced::daemon(boot, Nokkvi::update, Nokkvi::view)
        .title(Nokkvi::title)
        .default_font(theme::ui_font())
        .subscription(Nokkvi::subscription)
        .antialiasing(true)
        .run()
}

/// Print `--help` to stdout. Format follows GNU conventions: usage line,
/// option table, environment vars, file paths, then a docs URL.
fn print_cli_help() {
    let name = env!("CARGO_PKG_NAME");
    let version = env!("CARGO_PKG_VERSION");
    let description = env!("CARGO_PKG_DESCRIPTION");
    let repo = env!("CARGO_PKG_REPOSITORY");
    println!("{name} {version} — {description}");
    println!();
    println!("Usage: {name} [OPTIONS]");
    println!();
    println!("Options:");
    println!("  -h, --help       Print this help and exit");
    println!("  -V, --version    Print version and exit");
    println!();
    println!("Environment:");
    println!("  RUST_LOG         Override log filter. Examples:");
    println!("                     RUST_LOG=info                  # info+ on terminal and file");
    println!("                     RUST_LOG=debug                 # full debug on both");
    println!("                     RUST_LOG=trace                 # very verbose");
    println!("                     RUST_LOG=nokkvi::audio=trace   # narrow to one module");
    println!();
    println!("Files:");
    #[cfg(debug_assertions)]
    println!("  ~/.config/nokkvi/config.debug.toml    User configuration (TOML, debug build)");
    #[cfg(not(debug_assertions))]
    println!("  ~/.config/nokkvi/config.toml          User configuration (TOML)");
    println!("  ~/.config/nokkvi/themes/              Theme files (.toml)");
    println!("  ~/.config/nokkvi/sfx/                 Sound effect overrides");
    println!("  ~/.local/state/nokkvi/app.redb        Queue, session tokens, structured state");
    println!("  ~/.local/state/nokkvi/nokkvi.log      Log file (truncated on launch)");
    println!();
    println!("Documentation:");
    println!("  {repo}");
}

/// Daemon boot: build the initial state and queue a task to open the main
/// window. The resulting window id is delivered through the
/// `iced::window::open_events()` subscription (already wired up), so we
/// `.discard()` the open task's payload here to avoid a double-fire of
/// `Message::WindowOpened`.
fn boot() -> (Nokkvi, Task<Message>) {
    let state = Nokkvi::default();
    let (_id, open_task) = iced::window::open(main_window_settings());
    (state, open_task.discard())
}

/// Settings for the main window. Reused by `boot()` and by the tray's
/// "show window" path (`set_window_hidden(false)`), which has to recreate
/// the window because Wayland makes `set_visible(false)` a no-op — true
/// hide-to-tray on Wayland requires destroying the surface and opening a
/// fresh one.
pub(crate) fn main_window_settings() -> iced::window::Settings {
    iced::window::Settings {
        platform_specific: PlatformSpecific {
            application_id: "org.nokkvi.nokkvi".to_string(),
            ..Default::default()
        },
        // Routed via `Message::WindowCloseRequested` so close-to-tray can
        // close + reopen the window instead of exiting the runtime.
        exit_on_close_request: false,
        ..Default::default()
    }
}
