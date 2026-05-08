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
        },
        NavDisplayModeSetting {
            key: "general.nav_display_mode",
            value_type: Enum,
            setter: |mgr, v: String| mgr.set_nav_display_mode(NavDisplayMode::from_label(&v)),
            toml_apply: |ts, p| p.nav_display_mode = ts.nav_display_mode,
            read: |src, out| out.nav_display_mode = src.nav_display_mode,
        },
        TrackInfoDisplaySetting {
            key: "general.track_info_display",
            value_type: Enum,
            setter: |mgr, v: String| mgr.set_track_info_display(TrackInfoDisplay::from_label(&v)),
            toml_apply: |ts, p| p.track_info_display = ts.track_info_display,
            read: |src, out| out.track_info_display = src.track_info_display,
        },
        SlotRowHeightSetting {
            key: "general.slot_row_height",
            value_type: Enum,
            setter: |mgr, v: String| mgr.set_slot_row_height(SlotRowHeight::from_label(&v)),
            toml_apply: |ts, p| p.slot_row_height = ts.slot_row_height,
            read: |src, out| out.slot_row_height = src.slot_row_height,
        },
        HorizontalVolume {
            key: "general.horizontal_volume",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_horizontal_volume(v),
            toml_apply: |ts, p| p.horizontal_volume = ts.horizontal_volume,
            read: |src, out| out.horizontal_volume = src.horizontal_volume,
        },
        // --- Views ---
        SlotTextLinks {
            key: "general.slot_text_links",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_slot_text_links(v),
            toml_apply: |ts, p| p.slot_text_links = ts.slot_text_links,
            read: |src, out| out.slot_text_links = src.slot_text_links,
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
        },
        StripShowLabels {
            key: "general.strip_show_labels",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_strip_show_labels(v),
            toml_apply: |ts, p| p.strip_show_labels = ts.strip_show_labels,
            read: |src, out| out.strip_show_labels = src.strip_show_labels,
        },
        StripSeparatorSetting {
            key: "general.strip_separator",
            value_type: Enum,
            setter: |mgr, v: String| mgr.set_strip_separator(StripSeparator::from_label(&v)),
            toml_apply: |ts, p| p.strip_separator = ts.strip_separator,
            read: |src, out| out.strip_separator = src.strip_separator,
        },
        StripClickActionSetting {
            key: "general.strip_click_action",
            value_type: Enum,
            setter: |mgr, v: String| mgr.set_strip_click_action(StripClickAction::from_label(&v)),
            toml_apply: |ts, p| p.strip_click_action = ts.strip_click_action,
            read: |src, out| out.strip_click_action = src.strip_click_action,
        },
        // --- Artwork Column ---
        ArtworkColumnModeSetting {
            key: "general.artwork_column_mode",
            value_type: Enum,
            setter: |mgr, v: String| mgr.set_artwork_column_mode(ArtworkColumnMode::from_label(&v)),
            toml_apply: |ts, p| p.artwork_column_mode = ts.artwork_column_mode,
            read: |src, out| out.artwork_column_mode = src.artwork_column_mode,
        },
        ArtworkColumnStretchFitSetting {
            key: "general.artwork_column_stretch_fit",
            value_type: Enum,
            setter: |mgr, v: String| {
                mgr.set_artwork_column_stretch_fit(ArtworkStretchFit::from_label(&v))
            },
            toml_apply: |ts, p| p.artwork_column_stretch_fit = ts.artwork_column_stretch_fit,
            read: |src, out| out.artwork_column_stretch_fit = src.artwork_column_stretch_fit,
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
            setting_value::SettingValue, settings::PlayerSettings, toml_settings::TomlSettings,
        },
    };

    fn make_test_manager() -> (SettingsManager, TempDir) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("test_settings.redb");
        let storage = StateStorage::new(path).expect("StateStorage::new");
        (SettingsManager::for_test(storage), tmp)
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
                options: vec!["Auto", "Always (Native)", "Always (Stretched)", "Never"],
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
        assert_eq!(keys.len(), 20);
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
