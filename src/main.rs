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
    /// Harbour home / landing view. A start-view-eligible top-level destination
    /// reached via a pinned longship button (right edge of the top nav, bottom
    /// of the side nav) rather than a regular `NAV_TABS` tab — so it maps to
    /// `None` in the `View ↔ NavView` conversions. It IS a slot-list view: it
    /// impls `ViewPage` and `view_page(View::Harbour)` returns its page (unlike
    /// Settings, which returns `None`), so the generic slot-list handlers
    /// (navigate / activate / seek) drive it. Rendered by `HarbourPage::view`.
    Harbour,
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
        View::Harbour,
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
            View::Harbour => Some("Harbour"),
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
const _: [(); 10 - View::ALL.len()] = [];
const _: [(); View::ALL.len() - 10] = [];

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
    pub harbour_page: views::HarbourPage,
    /// Columns shown in the smart-playlist rules preview/results pane. The
    /// rules session is ephemeral (rebuilt each open), so unlike the 7 view
    /// pages this persistent copy is the source of truth — restored on
    /// `PlayerSettingsLoaded`, toggled optimistically, and read by the view.
    pub preview_column_visibility: crate::state::PreviewColumnVisibility,

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
    /// The authenticated user's Navidrome id, captured at login/resume
    /// success (empty until then, and after logout). The playlist ownership
    /// gate compares this against `PlaylistUIViewData.owner_id` — NEVER
    /// against owner names (Navidrome logins are case-insensitive, so a
    /// "Foogs" login vs a "foogs" owner_name would silently fail).
    pub session_user_id: String,
    /// Smart-playlist capability gate, fetched once post-auth (all-false
    /// until `Fetched`; `FetchFailed` renders the dimmed retry entry).
    pub caps_state: crate::state::CapsState,
    /// Root-owned stale-drop counter for rules-preview loads (the Trawl
    /// `trawl_search_generation` pattern — root-owned so session
    /// close/reopen can't re-mint captured generations).
    pub rules_preview_generation: u64,

    // -------------------------------------------------------------------------
    // Library Data (consolidated data vectors + counts)
    // -------------------------------------------------------------------------
    pub library: crate::state::LibraryData,

    /// Similar songs state — populated by getSimilarSongs2 / getTopSongs API calls
    pub similar_songs: Option<crate::state::SimilarSongsState>,
    /// Generation counter for stale response rejection
    pub similar_songs_generation: u64,

    /// Harbour home view state — discovery shelves + whole-library
    /// search results. Populated on first visit / library-filter change.
    pub harbour: crate::state::HarbourState,

    // -------------------------------------------------------------------------
    // Consolidated State Structs
    // -------------------------------------------------------------------------
    pub active_playback: crate::state::ActivePlayback,
    pub playback: crate::state::PlaybackState,
    /// Synced-lyrics state: resolved document, active-line cursor, store index.
    pub lyrics: crate::state::LyricsState,
    /// Monotonic epoch for the now-playing breathing glow. The per-frame boat
    /// tick (`update::boat::handle_boat_tick`) derives
    /// `phase = (now - glow_epoch) / GLOW_PERIOD_SECS` from it while playing.
    pub glow_epoch: std::time::Instant,
    pub scrobble: crate::state::ScrobbleState,
    /// One-seek-in-flight arbitration plus the live Seek Step setting.
    /// See [`crate::state::SeekState`] for why every seek producer goes
    /// through it.
    pub seek: crate::state::SeekState,
    /// Timing + dedup state for scrobbling internet radio directly to the
    /// configured service (ListenBrainz). Separate from `scrobble` because radio
    /// has no duration and no song id — see [`crate::state::RadioScrobbleState`].
    pub radio_scrobble: crate::state::RadioScrobbleState,
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
    /// Theater Mode: the now-playing layout toggled by F11 (see
    /// `update/theater.rs`). Transient, never persisted.
    pub theater: crate::state::TheaterState,
    pub toast: crate::state::ToastState,
    pub text_input_dialog: crate::widgets::text_input_dialog::TextInputDialogState,
    pub info_modal: crate::widgets::info_modal::InfoModalState,
    pub about_modal: crate::widgets::about_modal::AboutModalState,
    pub eq_modal: crate::widgets::eq_modal::EqModalState,
    /// Default-playlist picker overlay state. `Some` = picker is open.
    pub default_playlist_picker:
        Option<crate::widgets::default_playlist_picker::DefaultPlaylistPickerState>,
    /// Trawl mix-builder modal overlay state. `Some` = modal is open. The
    /// crate being edited lives on `trawl_crate` and survives closing.
    pub trawl_modal: Option<crate::widgets::trawl_modal::TrawlModalState>,
    /// The persistent Trawl crate: seeds + blend + the tray filters.
    /// Root-owned so the Harbour row and context menus can accrue seeds while
    /// the modal is closed; cleared on logout (seeds reference server ids).
    pub trawl_crate: nokkvi_data::types::trawl::TrawlCrate,
    /// Stale-drop generation for the modal's search fan-outs. Root-owned
    /// (NOT on `TrawlModalState`) so close/reopen can never re-mint a
    /// generation an in-flight fan-out already captured.
    pub trawl_search_generation: u64,

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
    /// Frozen first ≤4 distinct album ids backing the queue strip's 2×2 quad
    /// cover. Snapshotted from the queue head when the playlist context is
    /// entered (queue order == playlist track order at that moment) and left
    /// untouched by later queue mutations — consume-mode advances, queue
    /// sorts, and play-next insertions must not morph the "PLAYING FROM"
    /// thumbnail's identity. Cleared with the context; empty = no quad (the
    /// strip falls back to its single cover).
    pub strip_quad_album_ids: Vec<String>,
    pub browsing_panel: Option<views::BrowsingPanel>,
    pub pane_focus: crate::state::PaneFocus,
    /// Cross-pane drag cluster: active drag + press tracking + pending drop
    /// position (per-field docs on `CrossPaneDragUi`).
    pub cross_pane_drag: crate::state::CrossPaneDragUi,

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
    /// Trawling-longship state for the Harbour Trawl panel — a SEPARATE
    /// `BoatState` from the Lines-visualizer `boat` above, driven by the same
    /// per-frame `Message::BoatTick` but stepped against a procedural sea
    /// (`widgets::harbour_sea::sea_bars`) so it sails with no audio playing.
    /// Ticks only while the Harbour view is showing with an empty search
    /// (`update::boat::step_harbour_scene`); hidden otherwise with position
    /// preserved, mirroring the Lines boat's hide contract.
    pub harbour_boat: crate::widgets::boat::BoatState,
    /// Travelling phase of the Harbour panel's procedural sea, in `[0, 1)`.
    /// Advanced by the boat tick at `harbour_sea::SEA_DRIFT_HZ`; wrap-safe
    /// because every layer's phase multiplier is an integer (see
    /// `widgets::harbour_sea::sea_bars`).
    pub harbour_sea_phase: f32,
    /// Completed phase cycles of the harbour sea — incremented each time
    /// `harbour_sea_phase` wraps. Rare scene events (shooting star, leaping
    /// fish) hash THIS to vary their timing and trajectory per ~20 s cycle,
    /// which is what keeps a pure-phase animation from replaying an
    /// identical event loop forever.
    pub harbour_sea_cycle: u32,
    /// The sea heights the harbour boat was stepped against this frame —
    /// stored so the view draws the SAME array the physics sampled (the
    /// coherence guarantee that keeps the hull sitting ON the drawn water).
    pub harbour_sea_bars: Vec<f64>,

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
    /// Handle to push state into the tray. Set by each `TrayEvent::Connected`
    /// and kept after Show Tray Icon goes off: the tray thread shuts itself
    /// down when its subscription is cancelled, sends to it are then dropped
    /// silently, and the next `Connected` replaces the handle.
    pub tray_connection: Option<services::tray::TrayConnection>,
    /// Whether the window is currently hidden into the tray.
    pub tray_window_hidden: bool,
    /// Id of the open main window, adopted from each `WindowOpened`. `None`
    /// while it is closed to the tray and in the boot gap before the first
    /// `WindowOpened`; `show_window` checks it before opening a window.
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
    /// Find-and-expand chain cluster: in-flight target + center-only flag +
    /// post-load top pin (per-field docs on `PendingExpandState`).
    pub pending_expand: crate::state::PendingExpandState,
    /// Extracted backend server version (e.g. from Navidrome)
    pub server_version: Option<String>,
    /// OpenSubsonic extension names advertised by the connected server
    /// (`getOpenSubsonicExtensions`, fetched at login/resume). `None` until
    /// the probe lands — extension-gated features fail safe (hidden).
    pub open_subsonic_extensions: Option<std::collections::HashSet<String>>,

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
            harbour_page: views::HarbourPage::new(),
            preview_column_visibility: crate::state::PreviewColumnVisibility::default(),
            app_service: None,
            cached_storage: None,
            sfx_engine: nokkvi_data::audio::SfxEngine::default(),
            screen: Screen::Login,
            current_view: View::Queue,
            pre_settings_view: View::Queue,
            editor_return_view: View::Queue,
            should_auto_login,
            stored_session,
            session_user_id: String::new(),
            caps_state: crate::state::CapsState::default(),
            rules_preview_generation: 0,
            library: crate::state::LibraryData::default(),
            similar_songs: None,
            similar_songs_generation: 0,
            harbour: crate::state::HarbourState::default(),
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
                start_view: "Harbour".to_string(),
                stable_viewport: true,
                auto_follow_playing: true,
                ..nokkvi_data::types::player_settings::LivePlayerSettings::default()
            },
            // UI runtime flags (not persisted)
            start_view_applied: false,
            suppress_next_auto_center: false,
            pending_mode_commits: 0,
            pending_expand: crate::state::PendingExpandState::default(),
            server_version: None,
            open_subsonic_extensions: None,
            // Consolidated state structs with defaults
            active_playback: crate::state::ActivePlayback::default(),
            playback: crate::state::PlaybackState::default(),
            lyrics: crate::state::LyricsState::default(),
            glow_epoch: std::time::Instant::now(),
            scrobble: crate::state::ScrobbleState::default(),
            seek: crate::state::SeekState::default(),
            radio_scrobble: crate::state::RadioScrobbleState::default(),
            modes: crate::state::PlaybackModes::default(),
            sfx: crate::state::SfxState::default(),
            engine: crate::state::EngineState::default(),
            artwork: crate::state::ArtworkState::default(),
            window: crate::state::WindowState::default(),
            player_bar_layout: crate::widgets::player_bar::PlayerBarLayout::default(),
            theater: crate::state::TheaterState::default(),
            // Misc state
            last_queue_current_index: None,
            last_queue_current_entry_id: None,
            playlist_editor: None,
            active_playlist_info: None,
            strip_quad_album_ids: Vec::new(),
            browsing_panel: None,
            pane_focus: crate::state::PaneFocus::Queue,
            cross_pane_drag: crate::state::CrossPaneDragUi::default(),
            visualizer: None,
            visualizer_config: crate::visualizer_config::create_shared_config(),
            boat: crate::widgets::boat::BoatState::default(),
            harbour_boat: crate::widgets::boat::BoatState {
                // Start mid-panel so the first Harbour open doesn't watch the
                // boat surface from a corner; every other field lazily seeds
                // in `boat_physics::step()` (facing, rng, timers).
                x_ratio: 0.5,
                ..Default::default()
            },
            harbour_sea_phase: 0.0,
            harbour_sea_cycle: 0,
            harbour_sea_bars: Vec::new(),
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
            trawl_modal: None,
            trawl_crate: nokkvi_data::types::trawl::TrawlCrate::default(),
            trawl_search_generation: 0,
            open_menu: None,
            roulette: None,
        }
    }
}

// ============================================================================
// SECTION: Iced Application Trait Methods (KEEP IN main.rs)
// ============================================================================

impl Nokkvi {
    /// Whether the connected server advertises the OpenSubsonic
    /// `indexBasedQueue` extension (queue push/pull to the server).
    /// `false` until the login-time probe lands — the feature fails safe.
    pub(crate) fn supports_index_based_queue(&self) -> bool {
        self.open_subsonic_extensions
            .as_ref()
            .is_some_and(|s| s.contains("indexBasedQueue"))
    }

    /// `songLyrics` extension (structured lyrics via `getLyricsBySongId`).
    /// `false` until the login-time probe lands — the server lyrics channel
    /// fails safe to skipped; the store + LRCLIB channels are unaffected.
    pub(crate) fn supports_song_lyrics(&self) -> bool {
        self.open_subsonic_extensions
            .as_ref()
            .is_some_and(|s| s.contains("songLyrics"))
    }

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
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                repeat,
                ..
            }) => Some(Message::RawKeyEvent(key, modifiers, status, repeat)),
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
            // Window focus changes: gate the auto-hide toolbar's transient
            // reveals so a mid-reveal toolbar can't strand expanded behind
            // another app's window (its `on_exit` can't fire on an unfocused
            // Wayland surface).
            Event::Window(iced::window::Event::Unfocused) => Some(Message::WindowUnfocused),
            Event::Window(iced::window::Event::Focused) => Some(Message::WindowFocused),
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

        // Theater Mode activity: any mouse move, wheel or press (captured or
        // not; a slider drag is activity too) keeps the transient bar and the
        // cursor on screen. Present only while theater is active.
        let theater_events = if self.theater.active {
            event::listen_with(|event, _status, _window| match event {
                Event::Mouse(
                    iced::mouse::Event::CursorMoved { .. }
                    | iced::mouse::Event::WheelScrolled { .. }
                    | iced::mouse::Event::ButtonPressed(_),
                ) => Some(Message::Theater(app_message::TheaterMessage::Activity)),
                _ => None,
            })
        } else {
            iced::Subscription::none()
        };

        // MPRIS D-Bus server for Linux desktop integration
        let mpris = iced::Subscription::run(services::mpris::run).map(Message::Mpris);

        // System tray (StatusNotifierItem). Conditionally spawned: when the
        // user toggles `show_tray_icon` off, the subscription disappears
        // from the batch and iced cancels it, which drops its event
        // receiver; the tray thread sees that and tears down the ksni
        // service (see `services::tray::pump_commands`).
        let tray = if self.settings.show_tray_icon {
            iced::Subscription::run(services::tray::run).map(Message::Tray)
        } else {
            iced::Subscription::none()
        };

        // Rating desktop notifications (the rate-reminder AND the rate-change
        // confirmation share one service). Conditionally spawned like the tray:
        // when BOTH features are off the subscription leaves the batch and iced
        // cancels it, closing the command channel and tearing down the dbus
        // connection. So we never hold a session-bus connection unless at least
        // one feature is on. Each feature still gates its own sends, so the
        // shared connection only carries the commands the user opted into.
        let notifications = if self.settings.rating_reminder_enabled
            || self.settings.rating_change_notification_enabled
            || self.settings.love_change_notification_enabled
        {
            iced::Subscription::run(services::notifications::run).map(Message::Notification)
        } else {
            iced::Subscription::none()
        };

        // Window lifecycle: capture the main window's id on first open and
        // intercept close-button presses so we can branch on the
        // close-to-tray setting.
        let window_open_sub = iced::window::open_events().map(Message::WindowOpened);
        let window_close_sub = iced::window::close_requests().map(Message::WindowCloseRequested);

        // Config file watcher for hot-reloading theme + settings (the
        // [visualizer] section rides the unified SettingsConfigReloaded path:
        // reload_from_toml re-reads it and PlayerSettingsLoaded pushes it to
        // the shared render config).
        let config_watcher = iced::Subscription::run(|| {
            futures::stream::StreamExt::flat_map(
                crate::visualizer_config::config_watcher_subscription(),
                |opt| {
                    if opt.is_some() {
                        futures::stream::iter(vec![
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
            theater_events,
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

    /// Whether `library.queue_songs` is currently in `mode` / `ascending`
    /// order — the read-only verification counterpart to [`Self::sort_queue_songs`].
    ///
    /// `handle_queue_loaded` calls this to decide whether a freshly-loaded
    /// queue still reflects an applied sort (keep the dropdown label) or arrived
    /// from elsewhere — play album/playlist, session restore, add/remove, drag,
    /// consume, an SSE refresh — in which case the label reverts to "Unsorted".
    /// A queue of 0 or 1 songs is trivially sorted. `Random` has no
    /// deterministic order to verify, so it always reports `false`.
    ///
    /// INTERLOCK: the per-mode comparisons here mirror `sort_queue_songs`
    /// exactly (same fields, same `Reverse` for Rating/MostPlayed, same
    /// `ascending` flip). The `queue_is_sorted_matches_sort_queue_songs` parity
    /// test pins the two together.
    pub fn queue_is_sorted(&self, mode: views::QueueSortMode, ascending: bool) -> bool {
        use views::QueueSortMode;

        let songs = &self.library.queue_songs;
        match mode {
            // A shuffled order is not a verifiable sort.
            QueueSortMode::Random => false,
            // 0 or 1 items are trivially in order for any deterministic mode.
            _ if songs.len() < 2 => true,
            QueueSortMode::Title
            | QueueSortMode::Artist
            | QueueSortMode::Album
            | QueueSortMode::Genre => {
                let key = |s: &nokkvi_data::backend::queue::QueueSongUIViewData| match mode {
                    QueueSortMode::Title => s.title.to_lowercase(),
                    QueueSortMode::Artist => s.artist.to_lowercase(),
                    QueueSortMode::Album => s.album.to_lowercase(),
                    QueueSortMode::Genre => s.genre.to_lowercase(),
                    _ => unreachable!("string sort branch covers only string variants"),
                };
                if ascending {
                    songs.windows(2).all(|w| key(&w[0]) <= key(&w[1]))
                } else {
                    songs.windows(2).all(|w| key(&w[0]) >= key(&w[1]))
                }
            }
            QueueSortMode::Duration => {
                if ascending {
                    songs
                        .windows(2)
                        .all(|w| w[0].duration_seconds <= w[1].duration_seconds)
                } else {
                    songs
                        .windows(2)
                        .all(|w| w[0].duration_seconds >= w[1].duration_seconds)
                }
            }
            // `sort_queue_songs` keys Rating/MostPlayed by `Reverse(..)`, so the
            // default (ascending) order is highest-first; the `!ascending` flip
            // makes it lowest-first.
            QueueSortMode::Rating => {
                if ascending {
                    songs
                        .windows(2)
                        .all(|w| w[0].rating.unwrap_or(0) >= w[1].rating.unwrap_or(0))
                } else {
                    songs
                        .windows(2)
                        .all(|w| w[0].rating.unwrap_or(0) <= w[1].rating.unwrap_or(0))
                }
            }
            QueueSortMode::MostPlayed => {
                if ascending {
                    songs
                        .windows(2)
                        .all(|w| w[0].play_count.unwrap_or(0) >= w[1].play_count.unwrap_or(0))
                } else {
                    songs
                        .windows(2)
                        .all(|w| w[0].play_count.unwrap_or(0) <= w[1].play_count.unwrap_or(0))
                }
            }
        }
    }

    /// Re-evaluate the queue's sort state after an out-of-band order change
    /// (external reload or in-place drag reorder). Two jobs:
    ///
    /// 1. Invalidate the `sort_queue_songs` short-circuit cache
    ///    (`last_sort_signature`). The cache keys on `(mode, ascending, len)`,
    ///    so a same-length reorder leaves a stale signature that would make a
    ///    re-applied sort a no-op (rendering the stale order under the sort
    ///    label). Clearing it forces the next deterministic sort to actually
    ///    run — mirroring how `dispatch_random_queue_shuffle` clears it.
    /// 2. Demote the "sorted" state when the new order no longer reflects the
    ///    applied sort. Demote-only: it never promotes (only `apply_queue_sort`
    ///    does), so a queue that merely coincides with a mode is never shown as
    ///    if the user applied it.
    pub fn revalidate_queue_sorted(&mut self) {
        self.queue_page.last_sort_signature = None;
        if self.queue_page.queue_sorted {
            let mode = self.queue_page.queue_sort_mode;
            let ascending = self.queue_page.common.sort_ascending;
            self.queue_page.queue_sorted = self.queue_is_sorted(mode, ascending);
        }
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

        // The reverse (or a reload that changed the count) moves every station
        // to a new index, so the click's absolute positions name other stations
        // now. `handle_set_offset` below clears only the focus marker — the
        // selection set and its anchor have to go too, or the ring lands on the
        // wrong station, or nowhere (a non-empty set suppresses the center ring).
        self.radios_page.common.clear_selection_for_refresh();
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

    // Any path that reaches here is about to start iced: a bare `nokkvi`
    // (the `.desktop` entry, `uwsm app -- nokkvi`, `cargo run`) or args we
    // don't recognize (typos like `nokkvi haha`, unknown flags like
    // `nokkvi --foo`, `cargo run -- whatever`). Probe for a live instance
    // first. A bare launch hands off to it by forwarding `show`, which brings
    // back a window closed to the tray, and exits 0 (an older instance without
    // `show`, or one that never answers, still gets the exit-1 refusal); any
    // other argv shape refuses with exit 1. With nothing running, every shape
    // boots. A second iced startup would waste ~8s of init before crashing on
    // the redb single-instance lock, and before PID-suffixed sockets it also
    // unlinked the live instance's socket on its way down.
    //
    // The launcher's `XDG_ACTIVATION_TOKEN` is not forwarded: iced's Linux
    // window settings have no activation-token field and winit reads one only
    // at window creation, so the running window cannot use it.
    match second_launch_action(&args, nokkvi_ipc::find_live_socket()) {
        SecondLaunch::Boot => {}
        SecondLaunch::ForwardShow(socket) => return hand_off_to_running_instance(&socket),
        SecondLaunch::Refuse(socket) => refuse_second_launch(&socket),
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
            let file = std::fs::File::create(&path).ok()?;
            // The log captures full debug context (and is attached to bug
            // reports); it can carry sensitive material such as a Last.fm
            // authorize URL on the browser-open-failure fallback. Create it
            // owner-only rather than at the default umask. Best-effort.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
            Some(
                tracing_subscriber::fmt::layer()
                    .with_target(true)
                    .with_ansi(false)
                    .with_writer(std::sync::Mutex::new(file))
                    .with_filter(
                        EnvFilter::try_from_default_env()
                            .unwrap_or_else(|_| EnvFilter::new(&file_default_filter)),
                    ),
            )
        });

    tracing_subscriber::registry()
        .with(stderr_layer)
        .with(file_layer)
        .init();

    // Run the legacy → XDG-state-dir migration now that tracing is up,
    // and before iced::daemon spins up `Nokkvi::default()` (which loads
    // the session from app.redb via `credentials::load_session`).
    nokkvi_data::utils::paths::migrate_to_state_dir();

    // Seed the UI font from config.toml BEFORE the daemon captures its
    // default font — `.default_font()` is evaluated exactly once, and the
    // async settings load lands long after. Without this seed the launch
    // frame rendered the built-in Fira Sans wherever no font was set.
    //
    // The seed fixes only that STARTUP snapshot: because `.default_font()`
    // never re-reads, a font change from Settings → Interface repaints
    // `theme::ui_font()` for helper-routed text while anything relying on the
    // daemon default stays on the launch family until restart. So the
    // invariant every text site owes: render through a helper
    // (`slot_list_text*`, `weighted_ui_font`) or set `.font(theme::ui_font())`
    // explicitly — never lean on the daemon default.
    if let Ok(Some(settings)) = nokkvi_data::services::toml_settings_io::read_toml_settings() {
        theme::set_font_family(settings.font_family);
    }

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
    println!("  seek <±N | N>    Seek: ±N seconds relative, N absolute (float seconds)");
    println!("  volume <±N | N>  Volume: ±N relative (clamped), N absolute in [0.0, 1.0]");
    println!("  shuffle          Toggle shuffle (random) mode");
    println!("  repeat           Cycle repeat mode (off → one → queue)");
    println!("  consume          Toggle consume mode (drop played tracks)");
    println!("  clear-queue      Empty the queue and stop playback");
    println!("  queue-push       Push the local queue to the server");
    println!("  queue-pull       Replace the local queue with the server's");
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
    println!("  show             Reopen the window from the tray, or flag it when already open");
    println!();
    println!("Running `{name}` with no arguments while an instance is up forwards `show`");
    println!("to it and exits.");
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
/// The positional is forwarded verbatim under the expected arg name, so the
/// server-side parser owns absolute-vs-relative dispatch and emits the precise
/// error. Only a truly omitted positional yields an empty args object.
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
    let Some(raw) = positional else {
        // Verb expected an arg but the CLI user gave none — forward an
        // empty object so the server's "missing required arg" error fires.
        return serde_json::json!({});
    };

    // Every arg-taking verb forwards its positional VERBATIM as a JSON
    // string. Coercing to a number here would drop the leading `+`/`-` that
    // makes `seek +10`, `volume +0.05` and `rate +1` relative, and it would
    // turn a typo into "missing required arg" instead of the server's precise
    // "must be a number".
    serde_json::json!({ *arg_spec: raw })
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
        Ok(response) => print_ipc_response(verb, response),
        Err(err) => {
            #[allow(clippy::print_stderr)]
            {
                eprintln!("nokkvi {verb}: {err}");
            }
            std::process::exit(1);
        }
    }
}

/// Print a server's answer to `verb` and return, or print its error to
/// stderr and exit 1. Shared by [`forward_ipc_command`] and the bare-launch
/// hand-off.
fn print_ipc_response(verb: &str, response: nokkvi_ipc::IpcResponse) -> iced::Result {
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

/// What a launch that reached the single-instance probe should do.
#[derive(Debug, PartialEq, Eq)]
enum SecondLaunch {
    /// No live instance: start iced, whatever the argv shape.
    Boot,
    /// A bare `nokkvi` while an instance runs: forward `show` to the probed
    /// socket and exit.
    ForwardShow(std::path::PathBuf),
    /// Unrecognized args while an instance runs: refuse with exit 1.
    Refuse(std::path::PathBuf),
}

/// Decide what a launch does once it has got past the known-verb gate and
/// `--version` / `--help`. `live_socket` is the result of
/// [`nokkvi_ipc::find_live_socket`], which enumerates `nokkvi-*.sock` in
/// `$XDG_RUNTIME_DIR` (or the `/tmp` fallback) and connect-probes each, so a
/// dead socket file from a `SIGKILL`'d instance reads as `None`.
///
/// Only a bare `nokkvi` is handed to the running instance: args we don't
/// recognize still refuse, so a typo like `nokkvi nexxt` never quietly shows
/// the window. With no instance running, every argv shape boots as before.
fn second_launch_action(args: &[String], live_socket: Option<std::path::PathBuf>) -> SecondLaunch {
    match live_socket {
        None => SecondLaunch::Boot,
        Some(path) if args.len() <= 1 => SecondLaunch::ForwardShow(path),
        Some(path) => SecondLaunch::Refuse(path),
    }
}

/// Hand a bare second launch to the running instance: forward `show` to the
/// socket the probe found, print its answer and exit, like `nokkvi show`.
///
/// Two answers keep the old refusal instead: an instance started before `show`
/// existed (the binary was upgraded under it) says `unknown_command`, and one
/// that is stopped or wedged never answers, which the client's response
/// timeout turns into an error rather than a launch that blocks forever.
fn hand_off_to_running_instance(socket: &std::path::Path) -> iced::Result {
    let request = nokkvi_ipc::IpcRequest::new(1, "show", serde_json::Value::Null);
    match nokkvi_ipc::client::send_request(socket, &request) {
        Ok(response) if running_instance_predates_show(&response) => refuse_second_launch(socket),
        Ok(response) => print_ipc_response("show", response),
        Err(err) => {
            #[allow(clippy::print_stderr)]
            {
                eprintln!(
                    "nokkvi is already running (socket: {}) but did not answer: {err}",
                    socket.display()
                );
            }
            std::process::exit(1);
        }
    }
}

/// Whether the running instance answered `show` with `unknown_command`,
/// i.e. it predates the verb.
fn running_instance_predates_show(response: &nokkvi_ipc::IpcResponse) -> bool {
    response
        .error
        .as_ref()
        .is_some_and(|err| err.code == "unknown_command")
}

/// Print "already running" and exit with status 1 so this process never
/// reaches iced. Prevents a second instance from tripping redb's exclusive
/// lock at session-load time and crashing partway into boot.
fn refuse_second_launch(socket: &std::path::Path) -> ! {
    #[allow(clippy::print_stderr)]
    {
        eprintln!(
            "nokkvi is already running (socket: {}). Refusing second launch.",
            socket.display()
        );
    }
    std::process::exit(1);
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
    // Build the lyrics-store index once at boot (server-independent, so no
    // re-build on login). On Err only the store channel is lost — the server +
    // LRCLIB channels still resolve; the enabled mirror is left untouched.
    let lyrics_index_task = match nokkvi_data::utils::paths::get_lyrics_dir() {
        Ok(dir) => Task::perform(nokkvi_data::types::lyrics::build_index(dir), |idx| {
            Message::LyricsIndexReady(std::sync::Arc::new(idx))
        }),
        Err(e) => {
            tracing::warn!(error = %e, "lyrics dir unavailable; store channel disabled");
            Task::none()
        }
    };
    let task = if auto_login {
        Task::batch([
            open_task.discard(),
            sweep_task,
            lyrics_index_task,
            Task::done(Message::ResumeSession),
        ])
    } else {
        Task::batch([open_task.discard(), sweep_task, lyrics_index_task])
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
    fn seek_arg_forwards_as_string() {
        // `seek` joined `volume` / `rate` on `CliArgType::String` so the
        // server-side parser owns absolute-vs-relative dispatch. Coercing to
        // a JSON number here would silently drop the leading `+`/`-` that
        // makes an offset an offset.
        assert_eq!(
            build_ipc_cli_args("seek", Some("30")),
            json!({"position": "30"}),
        );
        assert_eq!(
            build_ipc_cli_args("seek", Some("+10")),
            json!({"position": "+10"}),
        );
        assert_eq!(
            build_ipc_cli_args("seek", Some("-5")),
            json!({"position": "-5"}),
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
        // The server's parser returns the precise "must be a number" error
        // rather than the misleading "missing required arg" path.
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
mod second_launch_tests {
    use std::path::PathBuf;

    use nokkvi_ipc::IpcResponse;
    use serde_json::json;

    use super::{SecondLaunch, running_instance_predates_show, second_launch_action};

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| (*a).to_string()).collect()
    }

    fn live() -> Option<PathBuf> {
        Some(PathBuf::from("/run/user/1000/nokkvi-4242.sock"))
    }

    #[test]
    fn bare_launch_with_a_live_instance_forwards_show() {
        // The `.desktop` `Exec=nokkvi`, `uwsm app -- nokkvi` and `cargo run`
        // all arrive as argc == 1.
        assert_eq!(
            second_launch_action(&argv(&["nokkvi"]), live()),
            SecondLaunch::ForwardShow(PathBuf::from("/run/user/1000/nokkvi-4242.sock")),
            "the hand-off goes to the socket the probe found, not a fresh lookup"
        );
    }

    #[test]
    fn bare_launch_without_an_instance_boots() {
        assert_eq!(
            second_launch_action(&argv(&["nokkvi"]), None),
            SecondLaunch::Boot
        );
    }

    #[test]
    fn unknown_args_with_a_live_instance_refuse() {
        // A typo like `nokkvi nexxt` must never quietly show the window.
        for args in [["nokkvi", "haha"], ["nokkvi", "--foo"]] {
            assert_eq!(
                second_launch_action(&argv(&args), live()),
                SecondLaunch::Refuse(PathBuf::from("/run/user/1000/nokkvi-4242.sock")),
                "{args:?}"
            );
        }
    }

    #[test]
    fn an_instance_without_the_show_verb_is_detected() {
        // A binary upgraded under a running older instance: its dispatcher
        // answers `unknown_command`, and the bare launch falls back to the
        // old "already running" refusal instead of echoing a verb the user
        // never typed.
        let old_instance = IpcResponse::err(1, "unknown_command", "unknown command: show");
        assert!(running_instance_predates_show(&old_instance));
    }

    #[test]
    fn a_show_answer_or_another_error_is_not_an_old_instance() {
        let shown = IpcResponse::ok(1, Some(json!({ "window": "opened" })));
        assert!(!running_instance_predates_show(&shown));
        let other = IpcResponse::err(1, "invalid_args", "boom");
        assert!(!running_instance_predates_show(&other));
    }

    #[test]
    fn unknown_args_without_an_instance_boot() {
        // Today's behaviour: with nothing running, stray args start the app.
        for args in [["nokkvi", "haha"], ["nokkvi", "--foo"]] {
            assert_eq!(
                second_launch_action(&argv(&args), None),
                SecondLaunch::Boot,
                "{args:?}"
            );
        }
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
