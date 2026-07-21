//! Interface tab setting entries — navigation, lists, player bar, font, and
//! metadata strip.
//!
//! 19 rows come from `define_settings!` via `build_interface_tab_settings_items`
//! (2 Navigation + 7 Slot List + 1 Player Bar + 1 Font & Icons (`icon_set`) +
//! 5 Metadata Strip + 3 Artwork Column: mode dropdown, `artwork_auto_max_pct`
//! slider, `artwork_vertical_height_pct` slider). The Slot List count includes the
//! `scrollbar_visibility` dropdown plus the three auto-hide sub-controls
//! (`autohide_collapsed_appearance` / `autohide_toolbar_height` /
//! `autohide_toolbar_grip`), which are inserted beneath the Auto-hide Toolbar
//! toggle only while it's enabled (height + grip only for the Hairline
//! appearance).
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
        macro_rows.take("general.scrollbar_visibility"),
        macro_rows.take("general.autohide_toolbar"),
        // --- Player Bar ---
        SettingsEntry::Header {
            label: "Player Bar",
            icon: PLAYER_BAR,
        },
        macro_rows.take("general.horizontal_volume"),
        // The MiniPlayer-only "Visible Controls" ToggleSet is conditionally
        // appended below (only in MiniPlayer mode) — see the trailing block.
        // --- Font & Icons (font row theme-routed + hand-written; icon set is a
        //     macro row) ---
        SettingsEntry::Header {
            label: "Font & Icons",
            icon: FONT,
        },
        SettingItem::text(
            SettingMeta::new("font_family", "Font Family", "Font & Icons")
                .with_subtitle("Enter to browse installed fonts"),
            font_display,
            "(system default)",
        )
        .with_enter_hint()
        .with_activate(ActivateKind::FontPicker),
        macro_rows.take("general.icon_set"),
        // --- Metadata Strip ---
        SettingsEntry::Header {
            label: "Metadata Strip",
            icon: STRIP,
        },
        macro_rows.take("general.track_info_display"),
        SettingItem::toggle_set(
            SettingMeta::new("__toggle_strip_fields", "Visible Fields", "Metadata Strip")
                .with_subtitle("Choose which metadata fields appear in the strip"),
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
                "Artwork Overlays",
            )
            .with_subtitle("Show the metadata text overlay on the large artwork in each view"),
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
                "Player Bar",
            )
            .with_subtitle("Choose which controls appear in the mini-player bar"),
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
                "Artwork Column",
            )
            .with_subtitle(
                "Cover: crop to fill, preserve aspect · Fill: true stretch, distorts \
                 album art",
            ),
            data.artwork_column_stretch_fit.as_ref(),
            "Cover",
            vec!["Cover", "Fill"],
        ));
    }

    // Auto-hide toolbar sub-controls render directly beneath the toggle and
    // only while it's enabled. The "Collapsed appearance" picker always shows;
    // the height + grip refinements apply only to the Hairline appearance.
    // The three rows are taken unconditionally and pushed conditionally so
    // `finish()` can assert full consumption regardless of data — rows not
    // pushed just drop, emitting the same UI as before.
    let appearance_row = macro_rows.take("general.autohide_collapsed_appearance");
    let height_row = macro_rows.take("general.autohide_toolbar_height");
    let grip_row = macro_rows.take("general.autohide_toolbar_grip");
    if data.autohide_toolbar {
        use nokkvi_data::types::player_settings::CollapsedAppearance;
        let insert_at = items
            .iter()
            .position(|e| {
                matches!(e, SettingsEntry::Item(it) if it.key.as_ref() == "general.autohide_toolbar")
            })
            .map_or(items.len(), |pos| pos + 1);
        items.insert(insert_at, appearance_row);
        if CollapsedAppearance::from_label(data.autohide_collapsed_appearance.as_ref())
            == CollapsedAppearance::Hairline
        {
            items.insert(insert_at + 1, height_row);
            items.insert(insert_at + 2, grip_row);
        }
    }

    macro_rows.finish();
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

    /// Auto-hide sub-controls render beneath the toggle only while enabled, and
    /// the height + grip refinements appear only for the Hairline appearance.
    #[test]
    fn autohide_subcontrols_track_toggle_and_appearance() {
        // Disabled (default): no sub-controls at all.
        let off = build_interface_items(&InterfaceSettingsData::default());
        assert!(key_pos(&off, "general.autohide_collapsed_appearance").is_none());
        assert!(key_pos(&off, "general.autohide_toolbar_height").is_none());
        assert!(key_pos(&off, "general.autohide_toolbar_grip").is_none());

        // Hairline: appearance, then height, then grip directly after the toggle.
        let hairline = build_interface_items(&InterfaceSettingsData {
            autohide_toolbar: true,
            autohide_collapsed_appearance: "Hairline".into(),
            ..Default::default()
        });
        let toggle = key_pos(&hairline, "general.autohide_toolbar").expect("toggle present");
        assert_eq!(
            key_pos(&hairline, "general.autohide_collapsed_appearance"),
            Some(toggle + 1),
            "appearance picker directly after the toggle"
        );
        assert_eq!(
            key_pos(&hairline, "general.autohide_toolbar_height"),
            Some(toggle + 2),
        );
        assert_eq!(
            key_pos(&hairline, "general.autohide_toolbar_grip"),
            Some(toggle + 3),
        );

        // Hidden / Count strip: appearance picker shows, but no height/grip.
        for mode in ["Hidden", "Count strip"] {
            let items = build_interface_items(&InterfaceSettingsData {
                autohide_toolbar: true,
                autohide_collapsed_appearance: mode.into(),
                ..Default::default()
            });
            assert!(
                key_pos(&items, "general.autohide_collapsed_appearance").is_some(),
                "{mode}: appearance picker present"
            );
            assert!(
                key_pos(&items, "general.autohide_toolbar_height").is_none(),
                "{mode}: no height refinement"
            );
            assert!(
                key_pos(&items, "general.autohide_toolbar_grip").is_none(),
                "{mode}: no grip refinement"
            );
        }
    }
}
