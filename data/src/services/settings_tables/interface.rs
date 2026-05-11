//! Interface-tab settings table.
//!
//! Layout / Views / Strip / Artwork Column knobs and the four ToggleSet sub-keys
//! (`strip_show_*`, `*_artwork_overlay`) live here. Theme-atomic re-sync after
//! a setter runs is handled by `PlayerSettingsLoaded` in the UI crate, so this
//! table only persists the values via `SettingsManager`.
//!
//! `font_family` is intentionally absent — it routes through `Message::ApplyFont`,
//! not `handle_settings_general`. The pixel-drag-driven `artwork_column_width_pct`
//! is also absent: it has a setter but no UI dispatch arm.

use crate::{
    define_settings,
    types::{
        player_settings::{
            ArtworkColumnMode, ArtworkStretchFit, NavDisplayMode, NavLayout, SlotRowHeight,
            StripClickAction, StripSeparator, TrackInfoDisplay,
        },
        setting_def::Tab,
        settings_data::InterfaceSettingsData,
    },
};

define_settings! {
    tab: Tab::Interface,
    data_type: InterfaceSettingsData<'_>,
    items_fn: build_interface_tab_settings_items,
    settings_const: TAB_INTERFACE_SETTINGS,
    contains_fn: tab_interface_contains,
    dispatch_fn: dispatch_interface_tab_setting,
    apply_fn: apply_toml_interface_tab,
    dump_fn: dump_interface_tab_player_settings,
    settings: [
        // --- Layout ---
        NavLayoutSetting {
            key: "general.nav_layout",
            value_type: Enum,
            setter: |mgr, v: String| mgr.set_nav_layout(NavLayout::from_label(&v)),
            toml_apply: |ts, p| p.nav_layout = ts.nav_layout,
            read: |src, out| out.nav_layout = src.nav_layout,
            ui_meta: {
                label: "Navigation Layout",
                category: "Layout",
                subtitle: Some("Top bar tabs, vertical sidebar, or no navigation chrome"),
                default: "Top",
                options: &["Top", "Side", "None"],
                read_field: |d| d.nav_layout,
            },
        },
        NavDisplayModeSetting {
            key: "general.nav_display_mode",
            value_type: Enum,
            setter: |mgr, v: String| mgr.set_nav_display_mode(NavDisplayMode::from_label(&v)),
            toml_apply: |ts, p| p.nav_display_mode = ts.nav_display_mode,
            read: |src, out| out.nav_display_mode = src.nav_display_mode,
            ui_meta: {
                label: "Nav Display",
                category: "Layout",
                subtitle: Some("Show text, icons, or both in navigation tabs"),
                default: "Text Only",
                options: &["Text Only", "Text + Icons", "Icons Only"],
                read_field: |d| d.nav_display_mode,
            },
        },
        TrackInfoDisplaySetting {
            key: "general.track_info_display",
            value_type: Enum,
            setter: |mgr, v: String| mgr.set_track_info_display(TrackInfoDisplay::from_label(&v)),
            toml_apply: |ts, p| p.track_info_display = ts.track_info_display,
            read: |src, out| out.track_info_display = src.track_info_display,
            ui_meta: {
                label: "Metadata Strip",
                category: "Layout",
                subtitle: Some("Where to show the now-playing metadata strip"),
                default: "Off",
                options: &["Off", "Player Bar", "Top Bar", "Progress Track"],
                read_field: |d| d.track_info_display,
            },
        },
        SlotRowHeightSetting {
            key: "general.slot_row_height",
            value_type: Enum,
            setter: |mgr, v: String| mgr.set_slot_row_height(SlotRowHeight::from_label(&v)),
            toml_apply: |ts, p| p.slot_row_height = ts.slot_row_height,
            read: |src, out| out.slot_row_height = src.slot_row_height,
            ui_meta: {
                label: "Row Density",
                category: "Layout",
                subtitle: Some(
                    "Controls how many rows are visible · fewer rows = larger artwork & text",
                ),
                default: "Default",
                options: &["Compact", "Default", "Comfortable", "Spacious"],
                read_field: |d| d.slot_row_height,
            },
        },
        HorizontalVolume {
            key: "general.horizontal_volume",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_horizontal_volume(v),
            toml_apply: |ts, p| p.horizontal_volume = ts.horizontal_volume,
            read: |src, out| out.horizontal_volume = src.horizontal_volume,
            ui_meta: {
                label: "Horizontal Volume Controls",
                category: "Layout",
                subtitle: Some("Stack volume sliders horizontally in the player bar"),
                default: false,
                read_field: |d| d.horizontal_volume,
            },
        },
        // --- Views ---
        SlotTextLinks {
            key: "general.slot_text_links",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_slot_text_links(v),
            toml_apply: |ts, p| p.slot_text_links = ts.slot_text_links,
            read: |src, out| out.slot_text_links = src.slot_text_links,
            ui_meta: {
                label: "Slot Text Links",
                category: "Views",
                subtitle: Some(
                    "Make title and artist text clickable to navigate to albums and artists",
                ),
                default: true,
                read_field: |d| d.slot_text_links,
            },
        },
        AlbumsArtworkOverlay {
            key: "general.albums_artwork_overlay",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_albums_artwork_overlay(v),
            toml_apply: |ts, p| p.albums_artwork_overlay = ts.albums_artwork_overlay,
            read: |src, out| out.albums_artwork_overlay = src.albums_artwork_overlay,
        },
        ArtistsArtworkOverlay {
            key: "general.artists_artwork_overlay",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_artists_artwork_overlay(v),
            toml_apply: |ts, p| p.artists_artwork_overlay = ts.artists_artwork_overlay,
            read: |src, out| out.artists_artwork_overlay = src.artists_artwork_overlay,
        },
        SongsArtworkOverlay {
            key: "general.songs_artwork_overlay",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_songs_artwork_overlay(v),
            toml_apply: |ts, p| p.songs_artwork_overlay = ts.songs_artwork_overlay,
            read: |src, out| out.songs_artwork_overlay = src.songs_artwork_overlay,
        },
        PlaylistsArtworkOverlay {
            key: "general.playlists_artwork_overlay",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_playlists_artwork_overlay(v),
            toml_apply: |ts, p| p.playlists_artwork_overlay = ts.playlists_artwork_overlay,
            read: |src, out| out.playlists_artwork_overlay = src.playlists_artwork_overlay,
        },
        // --- Strip ---
        StripShowTitle {
            key: "general.strip_show_title",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_strip_show_title(v),
            toml_apply: |ts, p| p.strip_show_title = ts.strip_show_title,
            read: |src, out| out.strip_show_title = src.strip_show_title,
        },
        StripShowArtist {
            key: "general.strip_show_artist",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_strip_show_artist(v),
            toml_apply: |ts, p| p.strip_show_artist = ts.strip_show_artist,
            read: |src, out| out.strip_show_artist = src.strip_show_artist,
        },
        StripShowAlbum {
            key: "general.strip_show_album",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_strip_show_album(v),
            toml_apply: |ts, p| p.strip_show_album = ts.strip_show_album,
            read: |src, out| out.strip_show_album = src.strip_show_album,
        },
        StripShowFormatInfo {
            key: "general.strip_show_format_info",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_strip_show_format_info(v),
            toml_apply: |ts, p| p.strip_show_format_info = ts.strip_show_format_info,
            read: |src, out| out.strip_show_format_info = src.strip_show_format_info,
        },
        StripMergedMode {
            key: "general.strip_merged_mode",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_strip_merged_mode(v),
            toml_apply: |ts, p| p.strip_merged_mode = ts.strip_merged_mode,
            read: |src, out| out.strip_merged_mode = src.strip_merged_mode,
            ui_meta: {
                label: "Merged Mode",
                category: "Metadata Strip",
                subtitle: Some(
                    "Render artist/album/title as a single scrolling unit \
                     with one set of bookends",
                ),
                default: false,
                read_field: |d| d.strip_merged_mode,
            },
        },
        StripShowLabels {
            key: "general.strip_show_labels",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_strip_show_labels(v),
            toml_apply: |ts, p| p.strip_show_labels = ts.strip_show_labels,
            read: |src, out| out.strip_show_labels = src.strip_show_labels,
            ui_meta: {
                label: "Show Labels",
                category: "Metadata Strip",
                subtitle: Some(
                    "Prefix each field with its name (title:, artist:, album:)",
                ),
                default: true,
                read_field: |d| d.strip_show_labels,
            },
        },
        StripSeparatorSetting {
            key: "general.strip_separator",
            value_type: Enum,
            setter: |mgr, v: String| mgr.set_strip_separator(StripSeparator::from_label(&v)),
            toml_apply: |ts, p| p.strip_separator = ts.strip_separator,
            read: |src, out| out.strip_separator = src.strip_separator,
            ui_meta: {
                label: "Field Separator",
                category: "Metadata Strip",
                subtitle: Some("Character used to join fields in merged mode"),
                default: "Dot ·",
                options: &[
                    "Dot ·",
                    "Bullet •",
                    "Pipe |",
                    "Em dash —",
                    "Slash /",
                    "Bar │",
                ],
                read_field: |d| d.strip_separator,
            },
        },
        StripClickActionSetting {
            key: "general.strip_click_action",
            value_type: Enum,
            setter: |mgr, v: String| mgr.set_strip_click_action(StripClickAction::from_label(&v)),
            toml_apply: |ts, p| p.strip_click_action = ts.strip_click_action,
            read: |src, out| out.strip_click_action = src.strip_click_action,
            ui_meta: {
                label: "Click Action",
                category: "Metadata Strip",
                subtitle: Some(
                    "What happens when you click the track info strip · \
                     no effect in Progress Track mode",
                ),
                default: "Go to Queue",
                options: &[
                    "Go to Queue",
                    "Go to Album",
                    "Go to Artist",
                    "Copy Track Info",
                    "Do Nothing",
                ],
                read_field: |d| d.strip_click_action,
            },
        },
        // --- Artwork Column ---
        ArtworkColumnModeSetting {
            key: "general.artwork_column_mode",
            value_type: Enum,
            setter: |mgr, v: String| mgr.set_artwork_column_mode(ArtworkColumnMode::from_label(&v)),
            toml_apply: |ts, p| p.artwork_column_mode = ts.artwork_column_mode,
            read: |src, out| out.artwork_column_mode = src.artwork_column_mode,
            ui_meta: {
                label: "Display Mode",
                category: "Artwork Column",
                subtitle: Some(
                    "Auto: hides on narrow windows · Always: drag the handle to resize · \
                     Always (Vertical): stack artwork above the slot list · \
                     Never: hidden everywhere",
                ),
                default: "Auto",
                options: &[
                    "Auto",
                    "Always (Native)",
                    "Always (Stretched)",
                    "Always (Vertical Native)",
                    "Always (Vertical Stretched)",
                    "Never",
                ],
                read_field: |d| d.artwork_column_mode,
            },
        },
        // The stretch-fit knob is conditional (only when artwork_column_mode
        // is "Always (Stretched)"); it stays hand-written in the UI items
        // builder so the if-check is colocated with the row construction.
        // No ui_meta here — the macro skips it.
        ArtworkColumnStretchFitSetting {
            key: "general.artwork_column_stretch_fit",
            value_type: Enum,
            setter: |mgr, v: String| {
                mgr.set_artwork_column_stretch_fit(ArtworkStretchFit::from_label(&v))
            },
            toml_apply: |ts, p| p.artwork_column_stretch_fit = ts.artwork_column_stretch_fit,
            read: |src, out| out.artwork_column_stretch_fit = src.artwork_column_stretch_fit,
        },
        ArtworkAutoMaxPctSetting {
            key: "general.artwork_auto_max_pct",
            value_type: Float,
            setter: |mgr, v: f64| mgr.set_artwork_auto_max_pct(v as f32),
            toml_apply: |ts, p| p.artwork_auto_max_pct = ts.artwork_auto_max_pct,
            read: |src, out| out.artwork_auto_max_pct = src.artwork_auto_max_pct,
            ui_meta: {
                label: "Auto-mode artwork size",
                category: "Artwork Column",
                subtitle: Some(
                    "Maximum fraction of the window's short axis the Auto-mode artwork can grow to",
                ),
                default: 0.40_f64,
                min: 0.30_f64, max: 0.70_f64, step: 0.05_f64, unit: "",
                read_field: |d| d.artwork_auto_max_pct,
            },
        },
        ArtworkVerticalHeightPctSetting {
            key: "general.artwork_vertical_height_pct",
            value_type: Float,
            setter: |mgr, v: f64| mgr.set_artwork_vertical_height_pct(v as f32),
            toml_apply: |ts, p| p.artwork_vertical_height_pct = ts.artwork_vertical_height_pct,
            read: |src, out| out.artwork_vertical_height_pct = src.artwork_vertical_height_pct,
            ui_meta: {
                label: "Always-Vertical artwork height",
                category: "Artwork Column",
                subtitle: Some(
                    "Fraction of window height used by the stacked artwork in \
                     Always (Vertical Native / Stretched) modes · drag the handle \
                     below the artwork for live resize",
                ),
                default: 0.40_f64,
                min: 0.10_f64, max: 0.80_f64, step: 0.05_f64, unit: "",
                read_field: |d| d.artwork_vertical_height_pct,
            },
        },
    ]
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::{
        services::{settings::SettingsManager, state_storage::StateStorage},
        types::{
            setting_item::SettingsEntry, setting_value::SettingValue, settings::PlayerSettings,
            settings_data::InterfaceSettingsData, toml_settings::TomlSettings,
        },
    };

    fn make_test_manager() -> (SettingsManager, TempDir) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("test_settings.redb");
        let storage = StateStorage::new(path).expect("StateStorage::new");
        (SettingsManager::for_test(storage), tmp)
    }

    fn default_interface_data() -> InterfaceSettingsData<'static> {
        InterfaceSettingsData {
            nav_layout: "Top",
            nav_display_mode: "Text Only",
            track_info_display: "Off",
            slot_row_height: "Default",
            horizontal_volume: false,
            slot_text_links: true,
            font_family: "",
            strip_show_title: true,
            strip_show_artist: true,
            strip_show_album: true,
            strip_show_format_info: true,
            strip_merged_mode: false,
            strip_show_labels: true,
            strip_separator: "Dot ·",
            strip_click_action: "Go to Queue",
            albums_artwork_overlay: true,
            artists_artwork_overlay: true,
            songs_artwork_overlay: true,
            playlists_artwork_overlay: true,
            artwork_column_mode: "Auto",
            artwork_column_stretch_fit: "Cover",
            artwork_auto_max_pct: 0.40,
            artwork_vertical_height_pct: 0.40,
        }
    }

    /// 13 entries get ui_meta — 5 Layout + 1 Views + 4 Metadata Strip + 3
    /// Artwork Column (mode dropdown + auto-max-pct slider + vertical-height
    /// slider). The 8 ToggleSet sub-keys (`strip_show_*`, `*_artwork_overlay`)
    /// and the conditional `artwork_column_stretch_fit` stay hand-written.
    #[test]
    fn build_interface_tab_settings_items_emits_thirteen_rows() {
        let data = default_interface_data();
        let entries = build_interface_tab_settings_items(&data);
        assert_eq!(entries.len(), 13);
        for e in &entries {
            assert!(matches!(e, SettingsEntry::Item(_)));
        }
    }

    #[test]
    fn dispatch_interface_nav_layout_persists_via_setter() {
        let (mut mgr, _tmp) = make_test_manager();
        // Default is `Top`; flip to `Side` and confirm the setter ran.
        assert_eq!(mgr.get_player_settings().nav_layout, NavLayout::Top);

        let result = dispatch_interface_tab_setting(
            "general.nav_layout",
            SettingValue::Enum {
                val: "Side".to_string(),
                options: vec!["Top", "Side", "None"],
            },
            &mut mgr,
        );

        assert!(matches!(
            result,
            Some(Ok(
                crate::types::settings_side_effect::SettingsSideEffect::None
            ))
        ));
        assert_eq!(mgr.get_player_settings().nav_layout, NavLayout::Side);
    }

    #[test]
    fn dispatch_interface_strip_show_title_persists_via_setter() {
        let (mut mgr, _tmp) = make_test_manager();
        // Default is `true`; flip to `false` and confirm the setter ran.
        assert!(mgr.get_player_settings().strip_show_title);

        let result = dispatch_interface_tab_setting(
            "general.strip_show_title",
            SettingValue::Bool(false),
            &mut mgr,
        );

        assert!(matches!(
            result,
            Some(Ok(
                crate::types::settings_side_effect::SettingsSideEffect::None
            ))
        ));
        assert!(!mgr.get_player_settings().strip_show_title);
    }

    #[test]
    fn dispatch_interface_artwork_column_mode_persists_via_setter() {
        let (mut mgr, _tmp) = make_test_manager();
        assert_eq!(
            mgr.get_player_settings().artwork_column_mode,
            ArtworkColumnMode::default()
        );

        let result = dispatch_interface_tab_setting(
            "general.artwork_column_mode",
            SettingValue::Enum {
                val: "Always (Stretched)".to_string(),
                options: vec![
                    "Auto",
                    "Always (Native)",
                    "Always (Stretched)",
                    "Always (Vertical Native)",
                    "Always (Vertical Stretched)",
                    "Never",
                ],
            },
            &mut mgr,
        );

        assert!(matches!(
            result,
            Some(Ok(
                crate::types::settings_side_effect::SettingsSideEffect::None
            ))
        ));
        assert_eq!(
            mgr.get_player_settings().artwork_column_mode,
            ArtworkColumnMode::from_label("Always (Stretched)")
        );
    }

    #[test]
    fn dispatch_interface_returns_none_for_unknown_key() {
        let (mut mgr, _tmp) = make_test_manager();
        let result =
            dispatch_interface_tab_setting("nonexistent.key", SettingValue::Bool(false), &mut mgr);
        assert!(result.is_none());
    }

    #[test]
    fn dispatch_interface_returns_err_on_type_mismatch() {
        let (mut mgr, _tmp) = make_test_manager();
        let result = dispatch_interface_tab_setting(
            "general.nav_layout",
            SettingValue::Bool(false),
            &mut mgr,
        );
        assert!(matches!(result, Some(Err(_))));
    }

    #[test]
    fn apply_toml_interface_copies_nav_layout() {
        let mut ts = TomlSettings::default();
        ts.nav_layout = NavLayout::Side;
        let mut p = PlayerSettings::default();
        p.nav_layout = NavLayout::Top;
        apply_toml_interface_tab(&ts, &mut p);
        assert_eq!(p.nav_layout, NavLayout::Side);
    }

    #[test]
    fn apply_toml_interface_copies_strip_show_title() {
        let mut ts = TomlSettings::default();
        ts.strip_show_title = false;
        let mut p = PlayerSettings::default();
        p.strip_show_title = true;
        apply_toml_interface_tab(&ts, &mut p);
        assert!(!p.strip_show_title);
    }

    #[test]
    fn apply_toml_interface_copies_artwork_column_mode() {
        let mut ts = TomlSettings::default();
        ts.artwork_column_mode = ArtworkColumnMode::from_label("Never");
        let mut p = PlayerSettings::default();
        p.artwork_column_mode = ArtworkColumnMode::default();
        apply_toml_interface_tab(&ts, &mut p);
        assert_eq!(
            p.artwork_column_mode,
            ArtworkColumnMode::from_label("Never")
        );
    }

    #[test]
    fn tab_interface_contains_recognizes_declared_keys() {
        assert!(tab_interface_contains("general.nav_layout"));
        assert!(tab_interface_contains("general.strip_show_title"));
        assert!(tab_interface_contains("general.artwork_column_mode"));
        assert!(!tab_interface_contains("general.stable_viewport"));
        assert!(!tab_interface_contains("font_family"));
    }

    #[test]
    fn tab_interface_settings_lists_migrated_keys() {
        let keys: Vec<&str> = TAB_INTERFACE_SETTINGS.iter().map(|d| d.key).collect();
        assert!(keys.contains(&"general.nav_layout"));
        assert!(keys.contains(&"general.strip_separator"));
        assert!(keys.contains(&"general.playlists_artwork_overlay"));
        assert!(keys.contains(&"general.artwork_auto_max_pct"));
        assert!(keys.contains(&"general.artwork_vertical_height_pct"));
        assert_eq!(keys.len(), 22);
    }

    /// Read-side: `dump_interface_tab_player_settings` copies the migrated
    /// fields onto the UI-facing struct. Spot-check one Layout enum, one
    /// Strip bool, one Artwork-overlay bool, and the artwork-column-mode
    /// enum — covers the three field-shape clusters owned by this tab.
    #[test]
    fn dump_interface_round_trip_copies_migrated_fields() {
        let (mgr, _tmp) = make_test_manager();
        let mut ui = mgr.get_player_settings();

        let mut src = PlayerSettings::default();
        src.nav_layout = NavLayout::Side;
        src.strip_show_title = false;
        src.albums_artwork_overlay = false;
        src.artwork_column_mode = ArtworkColumnMode::from_label("Never");

        dump_interface_tab_player_settings(&src, &mut ui);

        assert_eq!(ui.nav_layout, NavLayout::Side);
        assert!(!ui.strip_show_title);
        assert!(!ui.albums_artwork_overlay);
        assert_eq!(
            ui.artwork_column_mode,
            ArtworkColumnMode::from_label("Never")
        );
    }
}
