//! Interface tab setting entries — navigation, lists, player bar, font, and
//! metadata strip.
//!
//! 13 flat rows come from `define_settings!` via
//! `build_interface_tab_settings_items` (2 Navigation + 2 Slot List + 1
//! Player Bar + 5 Metadata Strip + 3 Artwork Column: mode dropdown,
//! `artwork_auto_max_pct` slider, `artwork_vertical_height_pct` slider).
//! A MiniPlayer-only "Visible Controls" ToggleSet (`__toggle_mini_player_controls`
//! → the data-only `general.mini_player_show_volume` / `general.mini_player_show_modes`
//! settings) is hand-inserted beneath the Player Bar row only when
//! `TrackInfoDisplay::MiniPlayer` is the active strip mode. Section headers, the
//! three ToggleSet rows (`__toggle_artwork_overlays`, `__toggle_strip_fields`,
//! `__toggle_mini_player_controls`), the theme-routed `font_family` text row, and
//! the conditional `general.artwork_column_stretch_fit` knob (shown when
//! `ArtworkColumnMode::is_stretched()` — i.e. either horizontal or vertical
//! stretched mode) stay hand-written.

// See `items_general.rs` for why the data struct lives in the data crate.
pub(crate) use nokkvi_data::types::settings_data::InterfaceSettingsData;
use nokkvi_data::{
    services::settings_tables::interface::build_interface_tab_settings_items,
    types::player_settings::{ArtworkColumnMode, TrackInfoDisplay},
};

use super::items::{ActivateKind, MacroRows, SettingItem, SettingMeta, SettingsEntry};

/// Build settings entries for the Interface tab.
pub(crate) fn build_interface_items(data: &InterfaceSettingsData) -> Vec<SettingsEntry> {
    const NAVIGATION: &str = "assets/icons/compass.svg";
    const SLOT_LIST: &str = "assets/icons/list-filter.svg";
    const PLAYER_BAR: &str = "assets/icons/audio-waveform.svg";
    const FONT: &str = "assets/icons/type.svg";
    const STRIP: &str = "assets/icons/radio-tower.svg";
    const ARTWORK_OVERLAYS: &str = "assets/icons/layout-grid.svg";
    const ARTWORK_COL: &str = "assets/icons/panel-right-open.svg";

    let font_display = if data.font_family.is_empty() {
        "(system default)"
    } else {
        data.font_family.as_ref()
    };

    let mut macro_rows = MacroRows::new(build_interface_tab_settings_items(data));

    let mut items: Vec<SettingsEntry> = vec![
        // --- Navigation ---
        SettingsEntry::Header {
            label: "Navigation",
            icon: NAVIGATION,
        },
        macro_rows.take("general.nav_layout"),
        macro_rows.take("general.nav_display_mode"),
        // --- Slot List ---
        SettingsEntry::Header {
            label: "Slot List",
            icon: SLOT_LIST,
        },
        macro_rows.take("general.slot_row_height"),
        macro_rows.take("general.slot_text_links"),
        // --- Player Bar ---
        SettingsEntry::Header {
            label: "Player Bar",
            icon: PLAYER_BAR,
        },
        macro_rows.take("general.horizontal_volume"),
        // The MiniPlayer-only "Visible Controls" ToggleSet is conditionally
        // appended below (only in MiniPlayer mode) — see the trailing block.
        // --- Font (theme-routed; hand-written) ---
        SettingsEntry::Header {
            label: "Font",
            icon: FONT,
        },
        SettingItem::text(
            SettingMeta::new("font_family", "Font Family", "Font")
                .with_subtitle("Enter to browse installed fonts"),
            font_display,
            "(system default)",
        )
        .with_enter_hint()
        .with_activate(ActivateKind::FontPicker),
        // --- Metadata Strip ---
        SettingsEntry::Header {
            label: "Metadata Strip",
            icon: STRIP,
        },
        macro_rows.take("general.track_info_display"),
        SettingItem::toggle_set(
            SettingMeta::new(
                "__toggle_strip_fields",
                "Visible Fields",
                "Choose which metadata fields appear in the strip",
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
        macro_rows.take("general.strip_merged_mode"),
        macro_rows.take("general.strip_show_labels"),
        macro_rows.take("general.strip_separator"),
        macro_rows.take("general.strip_click_action"),
        // --- Artwork Overlays ---
        SettingsEntry::Header {
            label: "Artwork Overlays",
            icon: ARTWORK_OVERLAYS,
        },
        SettingItem::toggle_set(
            SettingMeta::new(
                "__toggle_artwork_overlays",
                "Text Overlay On Artwork",
                "Show the metadata text overlay on the large artwork in each view",
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
        // --- Artwork Column ---
        SettingsEntry::Header {
            label: "Artwork Column",
            icon: ARTWORK_COL,
        },
        macro_rows.take("general.artwork_column_mode"),
        macro_rows.take("general.artwork_auto_max_pct"),
        macro_rows.take("general.artwork_vertical_height_pct"),
    ];

    // MiniPlayer-only controls: the volume slider and the mode-toggle / kebab
    // menu can each be hidden independently in the mini-player bar, so a
    // "Visible Controls" ToggleSet (mirroring the metadata strip's "Visible
    // Fields") is shown only when MiniPlayer is the active metadata-strip mode.
    // Inserted directly beneath `horizontal_volume` in the Player Bar section.
    // The two settings are data-only (no standalone macro row); the ToggleSet
    // references their keys + current values directly. Reads
    // `data.track_info_display` (mirroring the `artwork_column_mode` conditional
    // below) rather than the live theme atomic, so the gate is deterministic in
    // tests.
    if TrackInfoDisplay::from_label(data.track_info_display.as_ref())
        == TrackInfoDisplay::MiniPlayer
    {
        let controls_row = SettingItem::toggle_set(
            SettingMeta::new(
                "__toggle_mini_player_controls",
                "Visible Controls",
                "Choose which controls appear in the mini-player bar",
            ),
            vec![
                (
                    "Volume".to_string(),
                    "general.mini_player_show_volume".to_string(),
                    data.mini_player_show_volume,
                ),
                (
                    "Mode Menu".to_string(),
                    "general.mini_player_show_modes".to_string(),
                    data.mini_player_show_modes,
                ),
            ],
        );
        let insert_at = items
            .iter()
            .position(|e| {
                matches!(e, SettingsEntry::Item(it) if it.key.as_ref() == "general.horizontal_volume")
            })
            .map_or(items.len(), |pos| pos + 1);
        items.insert(insert_at, controls_row);
    }

    // Stretched-only knob: image fit applies only when the column is
    // stretched (horizontal or vertical).
    if ArtworkColumnMode::from_label(data.artwork_column_mode.as_ref()).is_stretched() {
        items.push(SettingItem::enum_val(
            SettingMeta::new(
                "general.artwork_column_stretch_fit",
                "Stretch Fit",
                "Cover: crop to fill, preserve aspect · Fill: true stretch, distorts album art",
            ),
            data.artwork_column_stretch_fit.as_ref(),
            "Cover",
            vec!["Cover", "Fill"],
        ));
    }

    items
}

#[cfg(test)]
mod tests {
    use nokkvi_data::types::player_settings::TrackInfoDisplay;

    use super::{InterfaceSettingsData, SettingsEntry, build_interface_items};

    fn data_for(mode: &'static str) -> InterfaceSettingsData {
        InterfaceSettingsData {
            track_info_display: mode.into(),
            ..Default::default()
        }
    }

    fn key_pos(items: &[SettingsEntry], key: &str) -> Option<usize> {
        items
            .iter()
            .position(|e| matches!(e, SettingsEntry::Item(it) if it.key.as_ref() == key))
    }

    /// The MiniPlayer-only "Visible Controls" ToggleSet renders directly after
    /// the horizontal-volume row. Guards the position(...).map_or(append) insert
    /// against a silent append-to-end if either key is ever renamed.
    #[test]
    fn visible_controls_toggleset_sits_after_horizontal_volume_in_mini_player() {
        let items = build_interface_items(&data_for(TrackInfoDisplay::MiniPlayer.as_label()));
        let hv =
            key_pos(&items, "general.horizontal_volume").expect("horizontal_volume row present");
        let ts = key_pos(&items, "__toggle_mini_player_controls")
            .expect("Visible Controls ToggleSet present in MiniPlayer mode");
        assert_eq!(
            ts,
            hv + 1,
            "ToggleSet must sit directly after horizontal_volume"
        );
    }

    /// The ToggleSet appears ONLY in MiniPlayer mode.
    #[test]
    fn visible_controls_toggleset_absent_outside_mini_player() {
        for mode in [
            TrackInfoDisplay::Off,
            TrackInfoDisplay::PlayerBar,
            TrackInfoDisplay::TopBar,
        ] {
            let items = build_interface_items(&data_for(mode.as_label()));
            assert!(
                key_pos(&items, "__toggle_mini_player_controls").is_none(),
                "{mode:?}: no Visible Controls ToggleSet outside MiniPlayer",
            );
        }
    }
}
