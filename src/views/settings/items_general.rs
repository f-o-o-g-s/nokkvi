//! General tab setting entries

use super::items::{SettingItem, SettingsEntry};

/// Data needed by the General tab builder
pub(crate) struct GeneralSettingsData<'a> {
    pub server_url: &'a str,
    pub username: &'a str,
    pub start_view: &'a str,
    pub stable_viewport: bool,
    pub auto_follow_playing: bool,
    pub enter_behavior: &'a str,
    pub local_music_path: &'a str,
    pub verbose_config: bool,
    pub library_page_size: &'a str,
    pub artwork_resolution: &'a str,
    pub show_album_artists_only: bool,
}

/// Build settings entries for the General tab
pub(crate) fn build_general_items(data: &GeneralSettingsData) -> Vec<SettingsEntry> {
    const APP: &str = "assets/icons/monitor.svg";
    const MOUSE: &str = "assets/icons/mouse-pointer.svg";
    const ACC: &str = "assets/icons/user-round.svg";
    const CACHE: &str = "assets/icons/database.svg";
    const LOGOUT: &str = "assets/icons/log-out.svg";
    const REBUILD: &str = "assets/icons/refresh-cw.svg";

    vec![
        // --- Application ---
        SettingsEntry::Header {
            label: "Application",
            icon: APP,
        },
        SettingItem::enum_val(
            meta!("general.start_view", "Start View", "View shown after login"),
            data.start_view,
            "Queue",
            vec!["Queue", "Albums", "Artists", "Songs", "Genres", "Playlists"],
        ),
        SettingItem::enum_val(
            meta!(
                "general.enter_behavior",
                "Enter Behavior",
                "Action when pressing Enter on items (all views except Queue)"
            ),
            data.enter_behavior,
            "Play All",
            vec!["Play All", "Play Single", "Append & Play"],
        ),
        SettingItem::text(
            meta!(
                "general.local_music_path",
                "Local Music Path",
                "Path to music on this machine for 'Open in File Manager' · press Enter to edit"
            ),
            data.local_music_path,
            "",
        ),
        SettingItem::enum_val(
            meta!(
                "general.library_page_size",
                "Library Page Size",
                "Items fetched per API request · larger = fewer loads, more memory"
            ),
            data.library_page_size,
            "Default (500)",
            vec![
                "Small (100)",
                "Default (500)",
                "Large (1,000)",
                "Massive (5,000)",
            ],
        ),
        SettingItem::enum_val(
            meta!(
                "general.artwork_resolution",
                "Artwork Resolution",
                "Panel image quality · higher = sharper on HiDPI, larger cache"
            ),
            data.artwork_resolution,
            "Default (1000px)",
            vec![
                "Default (1000px)",
                "High (1500px)",
                "Ultra (2000px)",
                "Original (Full Size)",
            ],
        ),
        SettingItem::bool_val(
            meta!(
                "general.show_album_artists_only",
                "Album Artists Only",
                "Only show artists that have explicitly released albums, hiding compilation/guest artists"
            ),
            data.show_album_artists_only,
            true,
        ),
        SettingItem::bool_val(
            meta!(
                "general.verbose_config",
                "Verbose Config",
                "Write all settings to config.toml, including unchanged defaults"
            ),
            data.verbose_config,
            false,
        ),
        // --- Mouse Behavior ---
        SettingsEntry::Header {
            label: "Mouse Behavior",
            icon: MOUSE,
        },
        SettingItem::bool_val(
            meta!(
                "general.stable_viewport",
                "Stable Viewport",
                "Click highlights in-place without scrolling"
            ),
            data.stable_viewport,
            true,
        ),
        SettingItem::bool_val(
            meta!(
                "general.auto_follow_playing",
                "Auto-Follow Playing Track",
                "Scroll to current track on track changes"
            ),
            data.auto_follow_playing,
            true,
        ),
        // --- Account ---
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
        // --- Cache ---
        SettingsEntry::Header {
            label: "Cache",
            icon: CACHE,
        },
        SettingItem::text_with_icon(
            meta!(
                "__action_rebuild_artwork",
                "Rebuild Artwork Cache",
                "Cache",
                "Re-download album, genre, and playlist artwork"
            ),
            "Press Enter to rebuild",
            "",
            REBUILD,
        ),
        SettingItem::text_with_icon(
            meta!(
                "__action_rebuild_artist",
                "Rebuild Artist Cache",
                "Cache",
                "Re-fetch artist photos from Last.fm (slow)"
            ),
            "Press Enter to rebuild",
            "",
            REBUILD,
        ),
    ]
}
