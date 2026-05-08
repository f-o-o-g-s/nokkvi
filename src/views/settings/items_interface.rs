//! Interface tab setting entries — layout, display, and metadata strip

// See `items_general.rs` for why the data struct lives in the data crate.
pub(crate) use nokkvi_data::types::settings_data::InterfaceSettingsData;

use super::items::{SettingItem, SettingsEntry};

/// Build settings entries for the Interface tab
pub(crate) fn build_interface_items(data: &InterfaceSettingsData) -> Vec<SettingsEntry> {
    const LAYOUT: &str = "assets/icons/panels-top-left.svg";
    const VIEWS: &str = "assets/icons/layout-grid.svg";
    const FONT: &str = "assets/icons/type.svg";
    const STRIP: &str = "assets/icons/radio-tower.svg";
    const ARTWORK_COL: &str = "assets/icons/panel-right-open.svg";

    let font_display = if data.font_family.is_empty() {
        "(system default)"
    } else {
        data.font_family
    };

    let mut items: Vec<SettingsEntry> = vec![
        // --- Layout ---
        SettingsEntry::Header {
            label: "Layout",
            icon: LAYOUT,
        },
        SettingItem::enum_val(
            meta!(
                "general.nav_layout",
                "Navigation Layout",
                "Top bar tabs, vertical sidebar, or no navigation chrome"
            ),
            data.nav_layout,
            "Top",
            vec!["Top", "Side", "None"],
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
        // --- Views ---
        SettingsEntry::Header {
            label: "Views",
            icon: VIEWS,
        },
        SettingItem::toggle_set(
            meta!(
                "__toggle_artwork_overlays",
                "Text Overlay On Artwork",
                "Show the metadata text overlay on the large artwork in each view"
            ),
            vec![
                (
                    "Albums".to_string(),
                    "general.albums_artwork_overlay".to_string(),
                    data.albums_artwork_overlay,
                ),
                (
                    "Artists".to_string(),
                    "general.artists_artwork_overlay".to_string(),
                    data.artists_artwork_overlay,
                ),
                (
                    "Songs".to_string(),
                    "general.songs_artwork_overlay".to_string(),
                    data.songs_artwork_overlay,
                ),
                (
                    "Playlists".to_string(),
                    "general.playlists_artwork_overlay".to_string(),
                    data.playlists_artwork_overlay,
                ),
            ],
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
        SettingItem::bool_val(
            meta!(
                "general.strip_merged_mode",
                "Merged Mode",
                "Render artist/album/title as a single scrolling unit with one set of bookends"
            ),
            data.strip_merged_mode,
            false,
        ),
        SettingItem::bool_val(
            meta!(
                "general.strip_show_labels",
                "Show Labels",
                "Prefix each field with its name (title:, artist:, album:)"
            ),
            data.strip_show_labels,
            true,
        ),
        SettingItem::enum_val(
            meta!(
                "general.strip_separator",
                "Field Separator",
                "Character used to join fields in merged mode"
            ),
            data.strip_separator,
            "Dot ·",
            vec![
                "Dot ·",
                "Bullet •",
                "Pipe |",
                "Em dash —",
                "Slash /",
                "Bar │",
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
        // --- Artwork Column ---
        SettingsEntry::Header {
            label: "Artwork Column",
            icon: ARTWORK_COL,
        },
        SettingItem::enum_val(
            meta!(
                "general.artwork_column_mode",
                "Display Mode",
                "Auto: hides on narrow windows · Always: drag the handle to resize · Never: hidden everywhere"
            ),
            data.artwork_column_mode,
            "Auto",
            vec!["Auto", "Always (Native)", "Always (Stretched)", "Never"],
        ),
    ];

    // Stretched-only knob: image fit applies only when the column is stretched.
    if data.artwork_column_mode == "Always (Stretched)" {
        items.push(SettingItem::enum_val(
            meta!(
                "general.artwork_column_stretch_fit",
                "Stretch Fit",
                "Cover: crop to fill, preserve aspect · Fill: true stretch, distorts album art"
            ),
            data.artwork_column_stretch_fit,
            "Cover",
            vec!["Cover", "Fill"],
        ));
    }

    items
}
