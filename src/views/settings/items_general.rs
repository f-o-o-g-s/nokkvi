//! General tab setting entries.
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

use super::items::{SettingItem, SettingsEntry};

/// Build settings entries for the General tab.
pub(crate) fn build_general_items(data: &GeneralSettingsData) -> Vec<SettingsEntry> {
    const APP: &str = "assets/icons/monitor.svg";
    const MOUSE: &str = "assets/icons/mouse-pointer.svg";
    const ACC: &str = "assets/icons/user-round.svg";
    const LOGOUT: &str = "assets/icons/log-out.svg";
    const TRAY: &str = "assets/icons/panels-top-left.svg";

    // Drain the macro-emitted rows by key so the explicit UI display order
    // below is decoupled from the macro entry order in `define_settings!`.
    let mut macro_rows = build_general_tab_settings_items(data);
    let mut take = |key: &str| -> SettingsEntry {
        let pos = macro_rows
            .iter()
            .position(|e| matches!(e, SettingsEntry::Item(it) if it.key.as_ref() == key))
            .unwrap_or_else(|| panic!("missing macro row for {key}"));
        macro_rows.remove(pos)
    };

    vec![
        // --- Application ---
        SettingsEntry::Header {
            label: "Application",
            icon: APP,
        },
        take("general.start_view"),
        take("general.enter_behavior"),
        take("general.local_music_path"),
        take("general.library_page_size"),
        take("general.artwork_resolution"),
        take("general.show_album_artists_only"),
        take("general.suppress_library_refresh_toasts"),
        take("general.verbose_config"),
        // --- Mouse Behavior ---
        SettingsEntry::Header {
            label: "Mouse Behavior",
            icon: MOUSE,
        },
        take("general.stable_viewport"),
        take("general.auto_follow_playing"),
        // --- System Tray ---
        SettingsEntry::Header {
            label: "System Tray",
            icon: TRAY,
        },
        take("general.show_tray_icon"),
        take("general.close_to_tray"),
        // --- Account (hand-written: read-only mirrors + logout dialog sentinel) ---
        SettingsEntry::Header {
            label: "Account",
            icon: ACC,
        },
        SettingItem::text(
            meta!(
                "general.server_url",
                "Server URL",
                "Read-only · configured at login"
            ),
            data.server_url,
            data.server_url,
        ),
        SettingItem::text(
            meta!(
                "general.username",
                "Username",
                "Read-only · configured at login"
            ),
            data.username,
            data.username,
        ),
        SettingItem::text_with_icon(
            meta!(
                "__action_logout",
                "Logout",
                "Account",
                "Sign out and return to login screen"
            ),
            "Press Enter to logout",
            "",
            LOGOUT,
        ),
    ]
}
