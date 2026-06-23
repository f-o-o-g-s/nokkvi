//! General tab setting entries.
//!
//! Section layout (top-to-bottom):
//!   1. Library  — library_page_size, artwork_resolution, show_album_artists_only
//!   2. Display  — start_view, suppress_library_refresh_toasts
//!   3. Behavior — enter_behavior, enter_shuffle, stable_viewport, auto_follow_playing
//!   4. Window & Tray — show_tray_icon, close_to_tray
//!   5. Advanced — local_music_path, verbose_config
//!   6. Account  — read-only server URL + username mirrors and the logout
//!      dialog sentinel
//!
//! 12 of the 15 visible rows come from `define_settings!` via the
//! macro-emitted `build_general_tab_settings_items` helper. Section headers
//! and the read-only Account section (server URL, username, logout dialog
//! sentinel) stay hand-written here — the dialog row uses the special
//! `__action_logout` key and the read-only mirrors are not first-class
//! settings.

// `GeneralSettingsData` lives in the data crate so the macro-emitted
// `build_general_tab_settings_items` (also in the data crate) can read its
// fields. Re-exported here so existing `crate::views::settings::items_general::
// GeneralSettingsData` import paths keep resolving.
use nokkvi_data::services::settings_tables::general::build_general_tab_settings_items;
pub(crate) use nokkvi_data::types::settings_data::GeneralSettingsData;

use super::{
    items::{ActivateKind, MacroRows, SettingItem, SettingMeta, SettingsEntry},
    sentinel::SentinelKind,
};

/// Build settings entries for the General tab.
pub(crate) fn build_general_items(data: &GeneralSettingsData) -> Vec<SettingsEntry> {
    const LIBRARY: &str = "assets/icons/library.svg";
    const DISPLAY: &str = "assets/icons/monitor.svg";
    const BEHAVIOR: &str = "assets/icons/mouse-pointer.svg";
    const TRAY: &str = "assets/icons/panels-top-left.svg";
    const ADVANCED: &str = "assets/icons/sliders-horizontal.svg";
    const ACC: &str = "assets/icons/user-round.svg";
    const LOGOUT: &str = "assets/icons/log-out.svg";

    // Drain the macro-emitted rows by key so the explicit UI display order
    // below is decoupled from the macro entry order in `define_settings!`.
    let mut macro_rows = MacroRows::new(build_general_tab_settings_items(data));

    let items = vec![
        // --- Library ---
        SettingsEntry::Header {
            label: "Library",
            icon: LIBRARY,
        },
        macro_rows.take("general.library_page_size"),
        macro_rows.take("general.artwork_resolution"),
        macro_rows.take("general.show_album_artists_only"),
        // --- Display ---
        SettingsEntry::Header {
            label: "Display",
            icon: DISPLAY,
        },
        macro_rows.take("general.start_view"),
        macro_rows.take("general.suppress_library_refresh_toasts"),
        // --- Behavior ---
        SettingsEntry::Header {
            label: "Behavior",
            icon: BEHAVIOR,
        },
        macro_rows.take("general.enter_behavior"),
        macro_rows.take("general.enter_shuffle"),
        macro_rows.take("general.stable_viewport"),
        macro_rows.take("general.auto_follow_playing"),
        // --- Window & Tray ---
        SettingsEntry::Header {
            label: "Window & Tray",
            icon: TRAY,
        },
        macro_rows.take("general.show_tray_icon"),
        macro_rows.take("general.close_to_tray"),
        // --- Advanced ---
        SettingsEntry::Header {
            label: "Advanced",
            icon: ADVANCED,
        },
        // Local music path opens a free-text input dialog (see
        // settings/mod.rs::SettingsAction::OpenTextInput) so the renderer
        // should show the "Enter ↵" affordance on this row.
        macro_rows
            .take("general.local_music_path")
            .with_enter_hint()
            .with_activate(ActivateKind::TextInputDialog),
        macro_rows.take("general.verbose_config"),
        // --- Account (hand-written: read-only mirrors + logout dialog sentinel) ---
        SettingsEntry::Header {
            label: "Account",
            icon: ACC,
        },
        SettingItem::text(
            SettingMeta::new(
                "general.server_url",
                "Server URL",
                "Read-only · configured at login",
            ),
            data.server_url.as_ref(),
            data.server_url.as_ref(),
        ),
        SettingItem::text(
            SettingMeta::new(
                "general.username",
                "Username",
                "Read-only · configured at login",
            ),
            data.username.as_ref(),
            data.username.as_ref(),
        ),
        SettingItem::text_with_icon(
            SettingMeta::new(SentinelKind::Logout.to_key(), "Logout", "Account")
                .with_subtitle("Sign out and return to login screen"),
            "Press Enter to logout",
            "",
            LOGOUT,
        ),
    ];

    macro_rows.finish();
    items
}
