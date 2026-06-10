#![warn(unreachable_pub)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::print_stderr))]
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
mod atomic_u8_enum;
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
    /// Playlist editor — a contextual destination with no permanent nav tab
    /// (like `Settings`). Reached only while an edit session is active, via
    /// the contextual "Editing" pill; entering/leaving never disturbs the
    /// live play queue, which keeps its own `Queue` tab.
    PlaylistEditor,
}

impl View {
    /// Every `View` variant. Length-anchored — see the `const _:` lines below.
    pub const ALL: &'static [View] = &[
        View::Albums,
        View::Queue,
        View::Songs,
        View::Artists,
        View::Genres,
        View::Playlists,
        View::Radios,
        View::Settings,
        View::PlaylistEditor,
    ];

    /// The persisted start-view name for this view, or `None` when the view
    /// is not start-view eligible. Exhaustive on purpose — a new view must
    /// decide its eligibility here, and making an existing view eligible is
    /// a product decision (owner sign-off), not a refactor. The user-facing
    /// dropdown options live in the iced-free data crate
    /// (`data/src/services/settings_tables/general.rs`), which cannot
    /// reference `View`; the `view_metadata_tests` drift guard pins the two
    /// lists together.
    pub(crate) const fn start_view_option(self) -> Option<&'static str> {
        match self {
            View::Queue => Some("Queue"),
            View::Albums => Some("Albums"),
            View::Artists => Some("Artists"),
            View::Songs => Some("Songs"),
            View::Genres => Some("Genres"),
            View::Playlists => Some("Playlists"),
            // None = not start-view eligible: Settings and PlaylistEditor
            // are contextual destinations, and Radios has stayed out of the
            // start-view dropdown since the setting shipped.
            View::Radios | View::Settings | View::PlaylistEditor => None,
        }
    }

    /// Inverse of [`Self::start_view_option`]: resolve a persisted start-view
    /// name to its `View`. `None` for ineligible or unknown names.
    pub(crate) fn from_start_view_name(name: &str) -> Option<View> {
        View::ALL
            .iter()
            .copied()
            .find(|v| v.start_view_option().is_some_and(|n| n == name))
    }
}

// Length anchor: adding a `View` variant without extending `ALL` fails to
// compile. Both directions are needed — a single subtraction passes if
// either side is too small.
const _: [(); 9 - View::ALL.len()] = [];
const _: [(); View::ALL.len() - 9] = [];

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
    /// View to restore when leaving the playlist editor (captured on enter).
    /// Mirrors `pre_settings_view` — the editor is a transient destination, so
    /// save/discard returns the user to wherever they launched the edit from.
    pub editor_return_view: View,

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
    /// Monotonic epoch for the now-playing breathing glow. The per-frame boat
    /// tick (`update::boat::handle_boat_tick`) derives
    /// `phase = (now - glow_epoch) / GLOW_PERIOD_SECS` from it while playing.
    pub glow_epoch: std::time::Instant,
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
    pub eq_modal: crate::widgets::eq_modal::EqModalState,
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
    /// Drift-immune mirror of `last_queue_current_index`. Stamped from
    /// `PlaybackStateUpdate::current_entry_id` (read under the same qm
    /// lock as `current_index`) so producers of `FocusCurrentPlaying`
    /// can dispatch a per-row handle that survives the optimistic-
    /// mutation window.
    pub last_queue_current_entry_id: Option<u64>,

    // -------------------------------------------------------------------------
    // Playlist Edit Mode (split-view)
    // -------------------------------------------------------------------------
    /// Active playlist editing session, owning its own track buffer decoupled
    /// from the live play queue. `Some(..)` is the "in edit mode" signal.
    pub playlist_editor: Option<crate::state::PlaylistEditorState>,
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
    /// Surfing-boat overlay state (lines-mode only). Phase + last sampled
    /// (x_ratio, y_ratio) + cached themed-logo SVG handle. Driven by per-frame
    /// `Message::BoatTick`; visibility derived from
    /// `engine.visualization_mode == Lines && config.enabled && config.lines.boat`.
    pub boat: crate::widgets::boat::BoatState,

    // -------------------------------------------------------------------------
    // MPRIS D-Bus Integration
    // -------------------------------------------------------------------------
    pub mpris_connection: Option<services::mpris::MprisConnection>,
    /// Last position (µs) pushed to MPRIS — used to detect seek discontinuities
    pub last_mpris_position_us: i64,
    /// Handle to push rate-this-track reminders to the notification service.
    /// `None` until the subscription connects (or while the feature is off).
    pub notification_connection: Option<services::notifications::NotificationConnection>,
    /// The last song a rating reminder fired for — the once-per-track latch
    /// that keeps repeat-one loops (which clear the scrobble latch each lap)
    /// from re-reminding the same track.
    pub last_reminded_song_id: Option<String>,

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
    // Persisted player settings (mirrors LivePlayerSettings 1:1 — see
    // data/src/types/player_settings/mod.rs). Loaded from redb on login via
    // `PlayerSettingsLoaded`. Adding a new persisted setting is a one-side
    // edit in `LivePlayerSettings`; this substruct picks it up automatically.
    // -------------------------------------------------------------------------
    pub settings: nokkvi_data::types::player_settings::LivePlayerSettings,

    // -------------------------------------------------------------------------
    // UI runtime flags (NOT persisted to LivePlayerSettings)
    // -------------------------------------------------------------------------
    /// One-shot flag: has start_view been applied yet?
    pub start_view_applied: bool,
    /// Transient flag: suppress the next auto-center triggered by a track change.
    /// Set when a click-initiated play fires, cleared after consumption.
    pub suppress_next_auto_center: bool,
    /// Count of in-flight mode-toggle commits (random / repeat / consume).
    /// Each optimistic toggle handler bumps this before spawning its async
    /// backend commit; the matching `*Toggled` result handler decrements it.
    /// While it is non-zero, the periodic tick stops clobbering the optimistic
    /// mode flags with a stale backend snapshot (the snapshot may predate the
    /// commit). Mirrors the `suppress_next_auto_center` idiom.
    pub pending_mode_commits: u32,
    /// In-flight find-and-expand target — at most one chain runs at a time
    /// across album / artist / genre / song. Set by the matching
    /// `handle_navigate_and_expand_*` (click-driven) or by the
    /// `handle_center_on_playing` fallback (Shift+C), and consumed by the
    /// matching `try_resolve_pending_expand_*` after the load resolves
    /// (paginated for album/artist/song, single-shot for genre).
    pub pending_expand: Option<crate::state::PendingExpand>,
    /// When `true`, the active `pending_expand` chain was started by
    /// CenterOnPlaying (Shift+C): `try_resolve_pending_expand_*` should
    /// CENTER the target on the viewport (not pin it to slot 0) and skip
    /// the `FocusAndExpand` dispatch so the row stays collapsed. Cleared
    /// alongside `pending_expand` in `cancel_pending_expand` and in each
    /// `try_resolve_*` once the target is resolved.
    pub pending_expand_center_only: bool,
    /// Re-pin the highlight onto the find-chain target after `set_children`
    /// runs. Set by `try_resolve_pending_expand_*` once the target is
    /// found; consumed by the matching children-loaded handler
    /// (`TracksLoaded` for albums, `AlbumsLoaded` for artists).
    pub pending_top_pin: Option<crate::state::PendingTopPin>,
    /// Extracted backend server version (e.g. from Navidrome)
    pub server_version: Option<String>,

    // -------------------------------------------------------------------------
    // Progress Tracking (polled from Tick for live toast updates)
    // -------------------------------------------------------------------------
    pub active_progress: Vec<nokkvi_data::types::progress::ProgressHandle>,

    // -------------------------------------------------------------------------
    // Roulette (slot-machine random pick across slot-list views)
    // -------------------------------------------------------------------------
    /// In-progress roulette spin, if any. Drives the dedicated tick
    /// subscription in `subscription()`.
    pub roulette: Option<crate::state::RouletteState>,
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
            editor_return_view: View::Queue,
            should_auto_login,
            stored_session,
            library: crate::state::LibraryData::default(),
            similar_songs: None,
            similar_songs_generation: 0,
            // Persisted player settings (overridden by PlayerSettingsLoaded).
            // LivePlayerSettings derives Default, which zeros every scalar
            // field (view_columns carries the real shipped column defaults
            // via ViewColumns::default()) — the 5 fields below are
            // hand-restored to non-zero values so first-launch behavior
            // (before PlayerSettingsLoaded fires) matches the pre-substruct
            // shape. These 5 must stay in agreement with
            // PersistedPlayerSettings::default(); the remaining fields
            // intentionally stay at LivePlayerSettings::default() until
            // PlayerSettingsLoaded overwrites them from redb.
            settings: nokkvi_data::types::player_settings::LivePlayerSettings {
                scrobbling_enabled: true,
                scrobble_threshold: 0.50,
                start_view: "Queue".to_string(),
                stable_viewport: true,
                auto_follow_playing: true,
                ..nokkvi_data::types::player_settings::LivePlayerSettings::default()
            },
            // UI runtime flags (not persisted)
            start_view_applied: false,
            suppress_next_auto_center: false,
            pending_mode_commits: 0,
            pending_expand: None,
            pending_expand_center_only: false,
            pending_top_pin: None,
            server_version: None,
            // Consolidated state structs with defaults
            active_playback: crate::state::ActivePlayback::default(),
            playback: crate::state::PlaybackState::default(),
            glow_epoch: std::time::Instant::now(),
            scrobble: crate::state::ScrobbleState::default(),
            modes: crate::state::PlaybackModes::default(),
            sfx: crate::state::SfxState::default(),
            engine: crate::state::EngineState::default(),
            artwork: crate::state::ArtworkState::default(),
            window: crate::state::WindowState::default(),
            player_bar_layout: crate::widgets::player_bar::PlayerBarLayout::default(),
            // Misc state
            last_queue_current_index: None,
            last_queue_current_entry_id: None,
            playlist_editor: None,
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
            boat: crate::widgets::boat::BoatState::default(),
            mpris_connection: None,
            last_mpris_position_us: 0,
            notification_connection: None,
            last_reminded_song_id: None,
            tray_connection: None,
            tray_window_hidden: false,
            main_window_id: None,
            hotkey_config: HotkeyConfig::default(),
            toast: crate::state::ToastState::default(),
            text_input_dialog: crate::widgets::text_input_dialog::TextInputDialogState::default(),
            info_modal: crate::widgets::info_modal::InfoModalState::default(),
            about_modal: crate::widgets::about_modal::AboutModalState::default(),
            eq_modal: crate::widgets::eq_modal::EqModalState::default(),
            default_playlist_picker: None,
            open_menu: None,
            active_progress: Vec::new(),
            roulette: None,
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
                Some(Message::CrossPaneDrag(
                    app_message::CrossPaneDragMessage::Moved(position),
                ))
            }
            Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) => Some(
                Message::CrossPaneDrag(app_message::CrossPaneDragMessage::Pressed),
            ),
            Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => Some(
                Message::CrossPaneDrag(app_message::CrossPaneDragMessage::Released),
            ),
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
        let tray = if self.settings.show_tray_icon {
            iced::Subscription::run(services::tray::run).map(Message::Tray)
        } else {
            iced::Subscription::none()
        };

        // Rating-reminder desktop notifications. Conditionally spawned like the
        // tray: when `rating_reminder_enabled` is off the subscription leaves
        // the batch and iced cancels it, closing the command channel and
        // tearing down the dbus connection. So we never hold a session-bus
        // connection unless the feature is on.
        let notifications = if self.settings.rating_reminder_enabled {
            iced::Subscription::run(services::notifications::run).map(Message::Notification)
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
                services::navidrome_sse::SseEvent::LibraryChanged(change) => {
                    Message::LibraryChanged(change)
                }
            });

        let task_status_sub = iced::Subscription::run(services::task_subscription::run)
            .map(|(handle, status)| Message::TaskStatusChanged(handle, status));

        // IPC subscription: binds `$XDG_RUNTIME_DIR/nokkvi.sock` at boot and
        // yields each incoming request as a `Message::Ipc`. The single-instance
        // guard in argv parsing keeps the bind from colliding with another
        // live nokkvi process.
        let ipc_sub = iced::Subscription::run(services::ipc::run)
            .map(|incoming| Message::Ipc(Box::new(incoming)));

        // Per-frame redraw events drive the surfing-boat overlay's eased
        // motion. Always-on (cost = one closure call per frame) — the boat
        // handler bails fast when not in lines mode, so the work is trivial
        // when the feature is off.
        let boat_frames = iced::window::frames().map(Message::BoatTick);

        // Roulette spin tick — only armed while a spin is active. Iced
        // tears down the subscription as soon as the batch no longer
        // contains it, so the timer naturally goes dormant on settle/cancel.
        let roulette_tick = if self.roulette.is_some() {
            time::every(Duration::from_millis(16))
                .map(|now| Message::Roulette(app_message::RouletteMessage::Tick(now)))
        } else {
            iced::Subscription::none()
        };

        iced::Subscription::batch(vec![
            tick,
            keyboard,
            window_events,
            login_events,
            mpris,
            tray,
            notifications,
            window_open_sub,
            window_close_sub,
            config_watcher,
            loop_sub,
            queue_changed_sub,
            sse_sub,
            task_status_sub,
            ipc_sub,
            boat_frames,
            roulette_tick,
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

    /// Filter the editor buffer's songs by the editor's own search query.
    ///
    /// Mirrors [`Self::filter_queue_songs`] but reads the editor buffer and the
    /// editor's independent search state. Returns `Cow::Borrowed` (zero-cost)
    /// when no search is active, and an empty borrowed slice when no edit
    /// session exists. Used to map filtered slot indices back to full-buffer
    /// rows during buffer mutations (invariant #1).
    pub fn filter_editor_songs(
        &self,
    ) -> std::borrow::Cow<'_, [nokkvi_data::backend::queue::QueueSongUIViewData]> {
        match self.playlist_editor.as_ref() {
            Some(editor) => {
                nokkvi_data::utils::search::filter_items(&editor.songs, &editor.common.search_query)
            }
            None => std::borrow::Cow::Borrowed(&[]),
        }
    }

    /// Collect song IDs from the playlist editor's buffer, in order.
    ///
    /// Mirrors [`Self::queue_song_ids`] but reads the editor's OWN buffer
    /// (`playlist_editor.songs`) instead of the live queue. Returns an empty
    /// vec when no edit session is active. Always serializes the full ordered
    /// buffer (never the filtered subset) — the save/dirty path relies on this.
    pub fn editor_song_ids(&self) -> Vec<String> {
        self.playlist_editor
            .as_ref()
            .map(|editor| editor.songs.iter().map(|s| s.id.clone()).collect())
            .unwrap_or_default()
    }

    /// Sort queue songs based on current sort mode and sort order (client-side).
    ///
    /// Short-circuits when `(mode, ascending, queue_len)` matches the last
    /// applied signature — re-toggling the same sort with no length change is
    /// a no-op. String sorts use `sort_by_cached_key` so each item's
    /// lowercased key is built exactly once per sort instead of N×log(N) times.
    ///
    /// `QueueSortMode::Random` is dispatched separately via
    /// `dispatch_random_queue_shuffle`; this method skips it so the cached
    /// signature isn't tied to a non-deterministic order.
    pub fn sort_queue_songs(&mut self) {
        use views::QueueSortMode;

        let sort_mode = self.queue_page.queue_sort_mode;
        if matches!(sort_mode, QueueSortMode::Random) {
            return;
        }
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
            // Random is handled by `dispatch_random_queue_shuffle` and
            // early-returned at the top of this method.
            QueueSortMode::Random => {}
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
}

// ============================================================================
// SECTION: Entry Point
// ============================================================================

pub fn main() -> iced::Result {
    // Cap glibc malloc arenas to 2 before any thread spawns. The default cap
    // (8 × num_cores) hoards ~330 MiB across mostly-empty per-thread arenas
    // over a long session — measured PSS drops from ~1042 MiB to ~703 MiB
    // with this single knob, with no observable contention cost on this
    // workload (audio uses preallocated ring buffers; UI rebuilds are tick-rate).
    #[cfg(target_env = "gnu")]
    {
        // Safety: mallopt is MT-unsafe; called as the first statement in main()
        // before any thread spawns, which makes the call sequenced-before any
        // potential concurrent allocator activity.
        unsafe { libc::mallopt(libc::M_ARENA_MAX, 2) };
    }

    // Handle --version / --help / ping / single-instance probe before tracing
    // init so these short-lived invocations don't truncate
    // ~/.local/state/nokkvi/nokkvi.log and so the IPC client path never
    // initializes iced / PipeWire / Symphonia (the §6.1 fork-before-iced
    // pattern — see ~/nokkvi-new-feats.md).
    let args: Vec<String> = std::env::args().collect();

    // `nokkvi <verb> [arg]` — IPC subcommand. Forwards the verb (and any
    // positional arg, parsed as the verb's expected type) to the long-running
    // instance's socket, prints the response, exits. The verb catalog is
    // generated by `define_commands!` in src/update/ipc.rs — there's no
    // separate list to keep in sync.
    if let Some(verb) = args.get(1)
        && update::IPC_KNOWN_COMMANDS.contains(&verb.as_str())
    {
        let cmd_args = build_ipc_cli_args(verb, args.get(2).map(String::as_str));
        return forward_ipc_command(verb, cmd_args);
    }

    for arg in args.iter().skip(1) {
        match arg.as_str() {
            "-V" | "--version" => {
                #[allow(clippy::print_stdout)]
                {
                    println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
                }
                return Ok(());
            }
            "-h" | "--help" => {
                print_cli_help();
                return Ok(());
            }
            _ => {}
        }
    }

    // Any path that reaches here is about to start iced — including args we
    // don't recognize (typos like `nokkvi haha`, unknown flags like
    // `nokkvi --foo`, or `cargo run -- whatever`). Probe for a live daemon
    // and refuse the second launch regardless of argv shape. Without this
    // the second iced startup wastes ~8s of init before crashing on the
    // redb single-instance lock — and prior to PID-suffixed sockets it
    // also unlinked the live daemon's socket on its way down.
    refuse_if_already_running();

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
        .font(include_bytes!("../assets/fonts/FiraSans-Medium.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/FiraSans-Bold.ttf").as_slice())
        .subscription(Nokkvi::subscription)
        .antialiasing(true)
        .run()
}

/// Print `--help` to stdout. Format follows GNU conventions: usage line,
/// option table, environment vars, file paths, then a docs URL.
#[allow(clippy::print_stdout)]
fn print_cli_help() {
    let name = env!("CARGO_PKG_NAME");
    let version = env!("CARGO_PKG_VERSION");
    let description = env!("CARGO_PKG_DESCRIPTION");
    let repo = env!("CARGO_PKG_REPOSITORY");
    println!("{name} {version} — {description}");
    println!();
    println!("Usage: {name} [OPTIONS] [COMMAND]");
    println!();
    println!("Commands:");
    println!("  ping             Probe the running instance over the IPC socket");
    println!("  status           Print playback state, track, volume, and modes (JSON)");
    println!("  next             Skip to the next track in the queue");
    println!("  previous         Return to the previous track in the queue");
    println!("  play             Start playback");
    println!("  pause            Pause playback");
    println!("  play-pause       Toggle between play and pause");
    println!("  stop             Stop playback");
    println!("  seek <seconds>   Seek to absolute position in seconds (float)");
    println!("  volume <0..1>    Set playback volume (clamped to [0.0, 1.0])");
    println!("  shuffle          Toggle shuffle (random) mode");
    println!("  repeat           Cycle repeat mode (off → one → queue)");
    println!("  consume          Toggle consume mode (drop played tracks)");
    println!("  clear-queue      Empty the queue and stop playback");
    println!("  add-to-queue     Add the focused list item to the queue");
    println!("  remove-from-queue  Remove the centered song from the queue (queue view only)");
    println!("  switch-view <v>  Switch the top pane to <v> (albums/queue/songs/");
    println!("                   artists/genres/playlists/radios/settings)");
    println!("  nav-up           Move the focused list selection up (Backspace)");
    println!("  nav-down         Move the focused list selection down (Tab)");
    println!("  enter            Activate the centered item (play/expand/edit)");
    println!("  selection        Print the focused view's centered item (JSON)");
    println!("  love             Toggle star on the currently-playing track");
    println!("  rate <±N | 0-5>  Adjust playing track rating: delta (+1/-1) or 0..5");
    println!();
    println!("Each command prints a compact JSON result on success (mutating verbs echo");
    println!("their resulting state, e.g. {{\"consume\":true}}); errors print to stderr with a");
    println!("non-zero exit status.");
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

/// Build the JSON `args` object for one of the arg-taking IPC verbs from a
/// single positional CLI string. Returns `Value::Null` for verbs that don't
/// take args.
///
/// On parse failure the raw string is forwarded under the expected arg name
/// so the server's `with_f32` arm returns the precise "must be a number"
/// error rather than the misleading "missing required arg" path. Only a
/// truly omitted positional yields an empty args object.
///
/// Per-verb arg mapping is hand-rolled in Phase 1; once the §14D macro grows
/// to also generate this CLI-side parser, the match collapses to a single
/// macro invocation.
fn build_ipc_cli_args(verb: &str, positional: Option<&str>) -> serde_json::Value {
    let Some(arg_spec) = update::IPC_CLI_ARGS
        .iter()
        .find(|(v, _)| *v == verb)
        .and_then(|(_, spec)| spec.as_ref())
    else {
        // Verbs without a CLI arg slot get a null body — the server's
        // dispatcher arm decides whether that's an error.
        return serde_json::Value::Null;
    };
    let (arg_name, arg_type) = arg_spec;

    let Some(raw) = positional else {
        // Verb expected an arg but the CLI user gave none — forward an
        // empty object so the server's "missing required arg" error fires.
        return serde_json::json!({});
    };

    match arg_type {
        update::CliArgType::Number => match raw.parse::<f64>() {
            Ok(n) => serde_json::json!({ *arg_name: n }),
            // Unparseable input goes through as a raw string so the
            // server's `must be a number` path emits the precise error.
            Err(_) => serde_json::json!({ *arg_name: raw }),
        },
        update::CliArgType::String => serde_json::json!({ *arg_name: raw }),
    }
}

/// Forward a single IPC verb (with optional structured args) to the running
/// nokkvi instance, print the response, exit.
///
/// Returns `Ok(())` on success. Errors print to stderr and call
/// [`std::process::exit(1)`] — we don't return `Err(iced::Error)` because the
/// caller would otherwise initialize iced just to surface the error.
///
/// Exit codes:
///   0 — server answered with a non-error response; the `data` payload is
///       printed to stdout (every verb carries one — mutating verbs echo their
///       resulting state, others a JSON ack — so success is never silent).
///   1 — could not reach a running instance, or server returned an error.
fn forward_ipc_command(verb: &str, args: serde_json::Value) -> iced::Result {
    let Some(path) = nokkvi_ipc::find_live_socket() else {
        #[allow(clippy::print_stderr)]
        {
            eprintln!(
                "nokkvi {verb}: no live nokkvi instance found in {}",
                nokkvi_ipc::socket_dir().display()
            );
        }
        std::process::exit(1);
    };
    let request = nokkvi_ipc::IpcRequest::new(1, verb, args);

    match nokkvi_ipc::client::send_request(&path, &request) {
        Ok(response) => {
            if let Some(err) = response.error {
                #[allow(clippy::print_stderr)]
                {
                    eprintln!(
                        "nokkvi {verb}: server returned error: {} ({})",
                        err.message, err.code
                    );
                }
                std::process::exit(1);
            }
            #[allow(clippy::print_stdout)]
            {
                // Every server success now carries a `data` payload (mutating
                // verbs echo their resulting state; others send `{"ok":true}`),
                // so a successful command is never silent. The `None` branch is
                // a belt-and-suspenders fallback for hand-written/older clients:
                // print a JSON ack rather than nothing. String payloads print
                // unquoted; objects/numbers print as compact JSON.
                let payload = match response.data.as_ref() {
                    Some(serde_json::Value::String(s)) => s.clone(),
                    Some(other) => other.to_string(),
                    None => "{\"ok\":true}".to_string(),
                };
                println!("{payload}");
            }
            Ok(())
        }
        Err(err) => {
            #[allow(clippy::print_stderr)]
            {
                eprintln!("nokkvi {verb}: {err}");
            }
            std::process::exit(1);
        }
    }
}

/// Probe for a running nokkvi instance. If one is found, print "already
/// running" and exit with status 1 so this process never reaches iced.
/// Returns normally only when no live socket is enumerated — at which point
/// the caller proceeds to daemon boot. Prevents a second daemon from
/// tripping redb's exclusive lock at session-load time and crashing
/// partway into boot.
///
/// Uses [`nokkvi_ipc::find_live_socket`] to enumerate `nokkvi-*.sock` in
/// `$XDG_RUNTIME_DIR` (or `/tmp` fallback) and connect-probe each. Dead
/// corpse files from `SIGKILL`'d daemons are skipped automatically.
fn refuse_if_already_running() {
    if let Some(path) = nokkvi_ipc::find_live_socket() {
        #[allow(clippy::print_stderr)]
        {
            eprintln!(
                "nokkvi is already running (socket: {}). Refusing second launch.",
                path.display()
            );
        }
        std::process::exit(1);
    }
}

/// Daemon boot: build the initial state and queue a task to open the main
/// window. The resulting window id is delivered through the
/// `iced::window::open_events()` subscription (already wired up), so we
/// `.discard()` the open task's payload here to avoid a double-fire of
/// `Message::WindowOpened`.
///
/// **Auto-login wiring**: when `Nokkvi::default()` finds a stored session in
/// redb it sets `should_auto_login = true`. We fire `Message::ResumeSession`
/// from the boot task here (rather than hijacking the first subscription
/// `Tick` inside `update()`) so the resume kick-off is co-located with state
/// construction. Iced's `run` spawns the boot task BEFORE registering
/// subscriptions (see `reference-iced/winit/src/lib.rs::run` — `runtime.run`
/// happens at line ~119, `runtime.track(... subscriptions ...)` at ~123), so
/// the `ResumeSession` message lands before any `Tick` would fire.
fn boot() -> (Nokkvi, Task<Message>) {
    let state = Nokkvi::default();
    let auto_login = state.should_auto_login;
    let (_id, open_task) = iced::window::open(main_window_settings());
    // Fire-and-forget cleanup of MPRIS art cache files for dead nokkvi PIDs.
    // Covers the crash / SIGKILL path where `begin_shutdown`'s `clear()`
    // never ran, and the pre-NF2 `mpris-art-<pid>.jpg` legacy shape that
    // the per-cover-id naming doesn't supersede on its own.
    let sweep_task =
        Task::future(crate::services::mpris_art_writer::sweep_dead_pid_files()).discard();
    let task = if auto_login {
        Task::batch([
            open_task.discard(),
            sweep_task,
            Task::done(Message::ResumeSession),
        ])
    } else {
        Task::batch([open_task.discard(), sweep_task])
    };
    (state, task)
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

#[cfg(test)]
mod build_ipc_cli_args_tests {
    use serde_json::json;

    use super::build_ipc_cli_args;

    #[test]
    fn seek_numeric_arg_round_trips_as_json_number() {
        assert_eq!(
            build_ipc_cli_args("seek", Some("30")),
            json!({"position": 30.0}),
        );
        assert_eq!(
            build_ipc_cli_args("seek", Some("42.5")),
            json!({"position": 42.5}),
        );
    }

    #[test]
    fn volume_arg_forwards_as_string() {
        // The volume verb's CLI arg is `CliArgType::String` (server-side parser
        // owns absolute-vs-delta dispatch), so the CLI wraps the positional
        // verbatim — no f64 coercion that would silently drop a leading `+`/`-`.
        assert_eq!(
            build_ipc_cli_args("volume", Some("0.7")),
            json!({"value": "0.7"}),
        );
        assert_eq!(
            build_ipc_cli_args("volume", Some("+0.05")),
            json!({"value": "+0.05"}),
        );
        assert_eq!(
            build_ipc_cli_args("volume", Some("-0.1")),
            json!({"value": "-0.1"}),
        );
    }

    #[test]
    fn unparseable_arg_forwards_raw_string_under_expected_key() {
        // Sends the server a wrong-type value so its `with_f32` arm returns
        // the precise "must be a number" error rather than the misleading
        // "missing required arg" path.
        assert_eq!(
            build_ipc_cli_args("seek", Some("not-a-number")),
            json!({"position": "not-a-number"}),
        );
    }

    #[test]
    fn missing_positional_yields_empty_args_object() {
        assert_eq!(build_ipc_cli_args("seek", None), json!({}));
    }

    #[test]
    fn verbs_without_args_return_json_null() {
        assert_eq!(build_ipc_cli_args("ping", None), serde_json::Value::Null);
        assert_eq!(
            build_ipc_cli_args("ping", Some("ignored")),
            serde_json::Value::Null,
        );
    }

    #[test]
    fn switch_view_forwards_view_string_unchanged() {
        assert_eq!(
            build_ipc_cli_args("switch-view", Some("albums")),
            json!({"view": "albums"}),
        );
        // No numeric coercion — strings stay strings so the server's view
        // parser surfaces the proper "unknown view" error.
        assert_eq!(
            build_ipc_cli_args("switch-view", Some("not-a-view")),
            json!({"view": "not-a-view"}),
        );
    }

    #[test]
    fn switch_view_with_no_positional_yields_empty_args() {
        assert_eq!(build_ipc_cli_args("switch-view", None), json!({}));
    }

    #[test]
    fn rate_forwards_positional_as_delta_string() {
        assert_eq!(
            build_ipc_cli_args("rate", Some("+1")),
            json!({"delta": "+1"}),
        );
        assert_eq!(build_ipc_cli_args("rate", Some("3")), json!({"delta": "3"}),);
        // Garbage is forwarded verbatim — server-side parser owns the
        // precise error message.
        assert_eq!(
            build_ipc_cli_args("rate", Some("loud")),
            json!({"delta": "loud"}),
        );
    }
}

#[cfg(test)]
mod view_metadata_tests {
    use nokkvi_data::{
        services::settings_tables::general::build_general_tab_settings_items,
        types::{
            setting_item::SettingsEntry, setting_value::SettingValue,
            settings_data::GeneralSettingsData,
        },
    };

    use super::View;

    #[test]
    fn start_view_names_round_trip_through_from_start_view_name() {
        for view in View::ALL.iter().copied() {
            if let Some(name) = view.start_view_option() {
                assert_eq!(
                    View::from_start_view_name(name),
                    Some(view),
                    "start-view name {name:?} must round-trip to {view:?}"
                );
            }
        }
    }

    #[test]
    fn from_start_view_name_rejects_ineligible_and_unknown_names() {
        for name in ["Radios", "Settings", "PlaylistEditor", "garbage", ""] {
            assert_eq!(
                View::from_start_view_name(name),
                None,
                "{name:?} must not resolve to a start view"
            );
        }
    }

    /// Drift guard: the start-view dropdown options live in the iced-free
    /// data crate (`data/src/services/settings_tables/general.rs`), which
    /// cannot reference `View` — this test is the only sync net between the
    /// two lists. Set-equality, not order: the table is Queue-first while
    /// `View::ALL` is Albums-first, so order is presentation, not contract.
    #[test]
    fn settings_table_start_view_options_match_view_metadata() {
        let entries = build_general_tab_settings_items(&GeneralSettingsData::default());
        let options = entries
            .iter()
            .find_map(|e| match e {
                SettingsEntry::Item(item) if item.key.as_ref() == "general.start_view" => {
                    match &item.value {
                        SettingValue::Enum { options, .. } => Some(options.clone()),
                        _ => None,
                    }
                }
                _ => None,
            })
            .expect("settings table must expose a general.start_view Enum entry");

        let eligible: Vec<&'static str> = View::ALL
            .iter()
            .filter_map(|v| v.start_view_option())
            .collect();

        assert_eq!(
            options.len(),
            eligible.len(),
            "start-view dropdown {options:?} and View metadata {eligible:?} disagree in size"
        );
        for name in &eligible {
            assert!(
                options.contains(name),
                "{name:?} is start-view eligible per View::start_view_option but missing \
                 from the settings-table options"
            );
        }
        for name in &options {
            assert!(
                eligible.contains(name),
                "settings table offers {name:?} but no View claims it via start_view_option"
            );
        }
    }
}
