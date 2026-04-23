//! Interface tab setting entries — layout, display, and metadata strip

use super::items::{SettingItem, SettingsEntry};

/// Data needed by the Interface tab builder
pub(crate) struct InterfaceSettingsData<'a> {
    pub nav_layout: &'a str,
    pub nav_display_mode: &'a str,
    pub track_info_display: &'a str,
    pub slot_row_height: &'a str,
    pub horizontal_volume: bool,
    pub slot_text_links: bool,
    pub font_family: &'a str,
    pub strip_show_title: bool,
    pub strip_show_artist: bool,
    pub strip_show_album: bool,
    pub strip_show_format_info: bool,
    pub strip_click_action: &'a str,
}

/// Build settings entries for the Interface tab
pub(crate) fn build_interface_items(data: &InterfaceSettingsData) -> Vec<SettingsEntry> {
    const LAYOUT: &str = "assets/icons/panels-top-left.svg";
    const FONT: &str = "assets/icons/type.svg";
    const STRIP: &str = "assets/icons/radio-tower.svg";

    let font_display = if data.font_family.is_empty() {
        "(system default)"
    } else {
        data.font_family
    };

    vec![
        // --- Layout ---
        SettingsEntry::Header {
            label: "Layout",
            icon: LAYOUT,
        },
        SettingItem::enum_val(
            meta!(
                "general.nav_layout",
                "Navigation Layout",
                "Top bar tabs or vertical sidebar"
            ),
            data.nav_layout,
            "Top",
            vec!["Top", "Side"],
        ),
        SettingItem::enum_val(
            meta!(
                "general.nav_display_mode",
                "Nav Display",
                "Show text, icons, or both in navigation tabs"
            ),
            data.nav_display_mode,
            "Text Only",
            vec!["Text Only", "Text + Icons", "Icons Only"],
        ),
        SettingItem::enum_val(
            meta!(
                "general.track_info_display",
                "Metadata Strip",
                "Where to show the now-playing metadata strip"
            ),
            data.track_info_display,
            "Off",
            vec!["Off", "Player Bar", "Top Bar", "Progress Track"],
        ),
        SettingItem::enum_val(
            meta!(
                "general.slot_row_height",
                "Row Density",
                "Controls how many rows are visible · fewer rows = larger artwork & text"
            ),
            data.slot_row_height,
            "Default",
            vec!["Compact", "Default", "Comfortable", "Spacious"],
        ),
        SettingItem::bool_val(
            meta!(
                "general.horizontal_volume",
                "Horizontal Volume Controls",
                "Stack volume sliders horizontally in the player bar"
            ),
            data.horizontal_volume,
            false,
        ),
        SettingItem::bool_val(
            meta!(
                "general.slot_text_links",
                "Slot Text Links",
                "Make title and artist text clickable to navigate to albums and artists"
            ),
            data.slot_text_links,
            true,
        ),
        // --- Font ---
        SettingsEntry::Header {
            label: "Font",
            icon: FONT,
        },
        SettingItem::text(
            meta!(
                "font_family",
                "Font Family",
                "Font",
                "Enter to browse installed fonts"
            ),
            font_display,
            "(system default)",
        ),
        // --- Metadata Strip ---
        SettingsEntry::Header {
            label: "Metadata Strip",
            icon: STRIP,
        },
        SettingItem::toggle_set(
            meta!(
                "__toggle_strip_fields",
                "Visible Fields",
                "Choose which metadata fields appear in the strip"
            ),
            vec![
                (
                    "Title".to_string(),
                    "general.strip_show_title".to_string(),
                    data.strip_show_title,
                ),
                (
                    "Artist".to_string(),
                    "general.strip_show_artist".to_string(),
                    data.strip_show_artist,
                ),
                (
                    "Album".to_string(),
                    "general.strip_show_album".to_string(),
                    data.strip_show_album,
                ),
                (
                    "Format Info".to_string(),
                    "general.strip_show_format_info".to_string(),
                    data.strip_show_format_info,
                ),
            ],
        ),
        SettingItem::enum_val(
            meta!(
                "general.strip_click_action",
                "Click Action",
                "What happens when you click the track info strip · no effect in Progress Track mode"
            ),
            data.strip_click_action,
            "Go to Queue",
            vec![
                "Go to Queue",
                "Go to Album",
                "Go to Artist",
                "Copy Track Info",
                "Do Nothing",
            ],
        ),
    ]
}
