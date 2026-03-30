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
    pub settings_page: views::SettingsPage,

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
    /// Stored session for JWT-based auto-login (server_url, username, jwt_token, subsonic_credential)
    pub stored_session: Option<(String, String, String, String)>,

    // -------------------------------------------------------------------------
    // Library Data (consolidated data vectors + counts)
    // -------------------------------------------------------------------------
    pub library: crate::state::LibraryData,

    // -------------------------------------------------------------------------
    // Consolidated State Structs
    // -------------------------------------------------------------------------
    pub playback: crate::state::PlaybackState,
    pub scrobble: crate::state::ScrobbleState,
    pub modes: crate::state::PlaybackModes,
    pub sfx: crate::state::SfxState,
    pub engine: crate::state::EngineState,
    pub artwork: crate::state::ArtworkState,
    pub window: crate::state::WindowState,
    pub toast: crate::state::ToastState,
    pub text_input_dialog: crate::widgets::text_input_dialog::TextInputDialogState,
    pub info_modal: crate::widgets::info_modal::InfoModalState,
    pub about_modal: crate::widgets::about_modal::AboutModalState,

    // -------------------------------------------------------------------------
    // Misc State
    // -------------------------------------------------------------------------
    pub last_queue_current_index: Option<usize>,

    // -------------------------------------------------------------------------
    // Playlist Edit Mode (split-view)
    // -------------------------------------------------------------------------
    pub playlist_edit: Option<nokkvi_data::types::playlist_edit::PlaylistEditState>,
    /// Identity of the playlist currently loaded in the queue (playlist_id, playlist_name, comment).
    /// Set on PlayPlaylist, cleared on non-playlist play.
    pub active_playlist_info: Option<(String, String, String)>,
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
    /// What Enter does in the Songs view (default: PlayAll)
    pub enter_behavior: nokkvi_data::types::player_settings::EnterBehavior,
    /// Local filesystem prefix for opening files in the file manager (default: empty = not set)
    pub local_music_path: String,
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
    /// Whether all settings (including defaults) are written to config.toml
    pub verbose_config: bool,

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
        let stored_session =
            stored_session.map(|(jwt, sub)| (server_url.clone(), username.clone(), jwt, sub));

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
            settings_page: views::SettingsPage::new(),
            app_service: None,
            cached_storage: None,
            sfx_engine: nokkvi_data::audio::SfxEngine::default(),
            screen: Screen::Login,
            current_view: View::Queue,
            pre_settings_view: View::Queue,
            should_auto_login,
            stored_session,
            library: crate::state::LibraryData::default(),
            // General settings defaults (overridden by PlayerSettingsLoaded)
            scrobbling_enabled: true,
            scrobble_threshold: 0.50,
            start_view: "Queue".to_string(),
            start_view_applied: false,
            stable_viewport: true,
            auto_follow_playing: true,
            enter_behavior: nokkvi_data::types::player_settings::EnterBehavior::default(),
            local_music_path: String::new(),
            suppress_next_auto_center: false,
            pending_center_on_playing: false,
            default_playlist_id: None,
            default_playlist_name: String::new(),
            quick_add_to_playlist: false,
            verbose_config: false,
            // Consolidated state structs with defaults
            playback: crate::state::PlaybackState::default(),
            scrobble: crate::state::ScrobbleState::default(),
            modes: crate::state::PlaybackModes::default(),
            sfx: crate::state::SfxState::default(),
            engine: crate::state::EngineState::default(),
            artwork: crate::state::ArtworkState {
                artist_disk_cache: nokkvi_data::utils::cache::DiskCache::new("artist_artwork"),
                genre_disk_cache: nokkvi_data::utils::cache::DiskCache::new("genre_artwork"),
                playlist_disk_cache: nokkvi_data::utils::cache::DiskCache::new("playlist_artwork"),
                ..Default::default()
            },
            window: crate::state::WindowState::default(),
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
            pending_queue_insert_position: None,
            visualizer: None,
            visualizer_config: crate::visualizer_config::create_shared_config(),
            mpris_connection: None,
            last_mpris_position_us: 0,
            hotkey_config: HotkeyConfig::default(),
            toast: crate::state::ToastState::default(),
            text_input_dialog: crate::widgets::text_input_dialog::TextInputDialogState::default(),
            info_modal: crate::widgets::info_modal::InfoModalState::default(),
            about_modal: crate::widgets::about_modal::AboutModalState::default(),
            active_progress: Vec::new(),
        }
    }
}

// ============================================================================
// SECTION: Iced Application Trait Methods (KEEP IN main.rs)
// ============================================================================

impl Nokkvi {
    /// Window title — dynamic based on playback state
    pub fn title(&self) -> String {
        if self.playback.has_track() {
            let status = if self.playback.playing {
                ""
            } else {
                " (Paused)"
            };
            format!(
                "{} - {}{} \u{2014} Nokkvi",
                self.playback.artist, self.playback.title, status
            )
        } else {
            "Nokkvi".to_string()
        }
    }

    /// Application theme — custom Gruvbox palette for default widget styles
    pub fn theme(&self) -> Theme {
        theme::iced_theme()
    }

    /// Global subscriptions: tick timer, keyboard, window events
    pub fn subscription(&self) -> iced::Subscription<Message> {
        let tick = time::every(Duration::from_millis(100))
            .map(|_| Message::Playback(app_message::PlaybackMessage::Tick)); // 10 times per second for smooth position updates

        // Audio rendering no longer driven by iced subscription.
        // It runs on a dedicated std::thread with a 5ms timer (see engine.rs).

        let keyboard = keyboard::listen().filter_map(|event| match event {
            keyboard::Event::KeyPressed { key, modifiers, .. } => {
                Some(Message::RawKeyEvent(key, modifiers))
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

        iced::Subscription::batch(vec![
            tick,
            keyboard,
            window_events,
            login_events,
            mpris,
            config_watcher,
            loop_sub,
            queue_changed_sub,
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
            tracing::warn!(
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

    /// Collect song IDs from the current queue (for dirty detection, save, etc.)
    pub fn queue_song_ids(&self) -> Vec<String> {
        self.library
            .queue_songs
            .iter()
            .map(|s| s.id.clone())
            .collect()
    }

    /// Sort queue songs based on current sort mode and sort order (client-side)
    pub fn sort_queue_songs(&mut self) {
        use views::QueueSortMode;

        let sort_mode = self.queue_page.queue_sort_mode;
        let ascending = self.queue_page.common.sort_ascending;

        debug!(
            " Sorting queue by {:?} ({})",
            sort_mode,
            if ascending { "ASC" } else { "DESC" }
        );

        self.library.queue_songs.sort_by(|a, b| {
            let cmp = match sort_mode {
                QueueSortMode::Title => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
                QueueSortMode::Artist => a.artist.to_lowercase().cmp(&b.artist.to_lowercase()),
                QueueSortMode::Album => a.album.to_lowercase().cmp(&b.album.to_lowercase()),
                QueueSortMode::Duration => a.duration_seconds.cmp(&b.duration_seconds),
                QueueSortMode::Genre => a.genre.to_lowercase().cmp(&b.genre.to_lowercase()),
                QueueSortMode::Rating => {
                    // Sort by rating: rated items first, then by rating value (higher first)
                    let a_rating = a.rating.unwrap_or(0);
                    let b_rating = b.rating.unwrap_or(0);
                    b_rating.cmp(&a_rating)
                }
            };

            if ascending { cmp } else { cmp.reverse() }
        });

        // Reset slot list to first item after resort
        self.queue_page
            .common
            .slot_list
            .set_offset(0, self.library.queue_songs.len());
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
}

// ============================================================================
// SECTION: Entry Point
// ============================================================================

pub fn main() -> iced::Result {
    // Initialize tracing with RUST_LOG filtering
    //
    // Default filter configuration:
    //   - nokkvi crate: debug level (our application logs)
    //   - Third-party crates: info/warn to suppress noise
    //
    // Override with RUST_LOG env var:
    //   RUST_LOG=warn ./nokkvi                    # Quiet mode
    //   RUST_LOG=trace ./nokkvi                   # Full trace (very verbose)
    //   RUST_LOG=nokkvi::audio=trace              # Trace audio only
    //   RUST_LOG=debug,hyper=debug ./nokkvi       # Include HTTP debug
    use tracing_subscriber::{EnvFilter, Layer, layer::SubscriberExt, util::SubscriberInitExt};

    // Build a sensible default filter that suppresses third-party noise
    let default_filter = [
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

    // File log layer: write warn+ to nokkvi.log in the config directory.
    // Captures watchdog warnings, loading errors, and shell_task orphans even
    // when launched from Hyprland keybinds with no visible terminal.
    // File is truncated on each startup to avoid unbounded growth.
    let file_layer = nokkvi_data::utils::paths::get_app_dir()
        .ok()
        .and_then(|dir| {
            std::fs::File::create(dir.join("nokkvi.log"))
                .ok()
                .map(|file| {
                    tracing_subscriber::fmt::layer()
                        .with_target(true)
                        .with_ansi(false)
                        .with_writer(std::sync::Mutex::new(file))
                        .with_filter(EnvFilter::new("warn"))
                })
        });

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&default_filter)))
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .with(file_layer)
        .init();

    iced::application(Nokkvi::default, Nokkvi::update, Nokkvi::view)
        .title(Nokkvi::title)
        .default_font(theme::ui_font())
        .subscription(Nokkvi::subscription)
        .window(iced::window::Settings {
            platform_specific: PlatformSpecific {
                application_id: "org.nokkvi.nokkvi".to_string(),
                ..Default::default()
            },
            ..Default::default()
        })
        .run()
}
