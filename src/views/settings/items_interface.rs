//! Interface tab setting entries — layout, display, and metadata strip.
//!
//! 13 flat rows come from `define_settings!` via
//! `build_interface_tab_settings_items` (5 Layout + 1 Views + 4 Metadata
//! Strip + 3 Artwork Column: mode dropdown, `artwork_auto_max_pct` slider,
//! `artwork_vertical_height_pct` slider). Section headers, the two ToggleSet
//! rows (`__toggle_artwork_overlays`, `__toggle_strip_fields`), the
//! theme-routed `font_family` text row, and the conditional
//! `general.artwork_column_stretch_fit` knob (shown when
//! `ArtworkColumnMode::is_stretched()` — i.e. either horizontal or vertical
//! stretched mode) stay hand-written.

// See `items_general.rs` for why the data struct lives in the data crate.
pub(crate) use nokkvi_data::types::settings_data::InterfaceSettingsData;
use nokkvi_data::{
    services::settings_tables::interface::build_interface_tab_settings_items,
    types::player_settings::ArtworkColumnMode,
};

use super::items::{SettingItem, SettingsEntry};

/// Build settings entries for the Interface tab.
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

    let mut macro_rows = build_interface_tab_settings_items(data);
    let mut take = |key: &str| -> SettingsEntry {
        let pos = macro_rows
            .iter()
            .position(|e| matches!(e, SettingsEntry::Item(it) if it.key.as_ref() == key))
            .unwrap_or_else(|| panic!("missing macro row for {key}"));
        macro_rows.remove(pos)
    };

    let mut items: Vec<SettingsEntry> = vec![
        // --- Layout ---
        SettingsEntry::Header {
            label: "Layout",
            icon: LAYOUT,
        },
        take("general.nav_layout"),
        take("general.nav_display_mode"),
        take("general.track_info_display"),
        take("general.slot_row_height"),
        take("general.horizontal_volume"),
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
        take("general.slot_text_links"),
        // --- Font (theme-routed; hand-written) ---
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
        take("general.strip_merged_mode"),
        take("general.strip_show_labels"),
        take("general.strip_separator"),
        take("general.strip_click_action"),
        // --- Artwork Column ---
        SettingsEntry::Header {
            label: "Artwork Column",
            icon: ARTWORK_COL,
        },
        take("general.artwork_column_mode"),
        take("general.artwork_auto_max_pct"),
        take("general.artwork_vertical_height_pct"),
    ];

    // Stretched-only knob: image fit applies only when the column is
    // stretched (horizontal or vertical).
    if ArtworkColumnMode::from_label(data.artwork_column_mode).is_stretched() {
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
