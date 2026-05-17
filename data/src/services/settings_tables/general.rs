//! General-tab settings table.
//!
//! Migrated keys: Application (start_view, enter_behavior, library_page_size,
//! suppress_library_refresh_toasts, artwork_resolution, show_album_artists_only,
//! local_music_path, verbose_config), Mouse Behavior (stable_viewport,
//! auto_follow_playing), System Tray (show_tray_icon, close_to_tray), and the
//! Theme-tab `light_mode` toggle (its persistence is config-file-only — the
//! macro's `on_dispatch:` hook routes the actual write through the UI crate).
//!
//! `general.server_url` and `general.username` are read-only login mirrors
//! with no setter or dispatch arm; they have no migration to perform.
//! `general.default_playlist_name` opens a picker dialog and is dispatched
//! via [`crate::types::settings_side_effect`] — see the lane brief for why
//! it is left on the bespoke action path.

use crate::{
    define_settings,
    types::{
        player_settings::{ArtworkResolution, EnterBehavior, LibraryPageSize},
        setting_def::Tab,
        settings_data::GeneralSettingsData,
        settings_side_effect::SettingsSideEffect,
        toast::ToastLevel,
    },
};

define_settings! {
    tab: Tab::General,
    data_type: GeneralSettingsData,
    items_fn: build_general_tab_settings_items,
    settings_const: TAB_GENERAL_SETTINGS,
    contains_fn: tab_general_contains,
    dispatch_fn: dispatch_general_tab_setting,
    apply_fn: apply_toml_general_tab,
    dump_fn: dump_general_tab_player_settings,
    write_fn: write_general_tab_toml,
    settings: [
        StableViewport {
            key: "general.stable_viewport",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_stable_viewport(v),
            toml_apply: |ts, p| p.stable_viewport = ts.stable_viewport,
            read: |src, out| out.stable_viewport = src.stable_viewport,
            write: |ps, ts| ts.stable_viewport = ps.stable_viewport,
            ui_meta: {
                label: "Stable Viewport",
                category: "Mouse Behavior",
                subtitle: Some("Click highlights in-place without scrolling"),
                default: true,
                read_field: |d| d.stable_viewport,
            },
        },
        StartView {
            key: "general.start_view",
            value_type: Enum,
            setter: |mgr, v: String| mgr.set_start_view(&v),
            toml_apply: |ts, p| p.start_view = ts.start_view.clone(),
            read: |src, out| out.start_view = src.start_view.clone(),
            write: |ps, ts| ts.start_view = ps.start_view.clone(),
            ui_meta: {
                label: "Start View",
                category: "Application",
                subtitle: Some("View shown after login"),
                default: "Queue",
                options: &["Queue", "Albums", "Artists", "Songs", "Genres", "Playlists"],
                read_field: |d| d.start_view.as_ref(),
            },
        },
        EnterBehavior {
            key: "general.enter_behavior",
            value_type: Enum,
            setter: |mgr, v: String| mgr.set_enter_behavior(EnterBehavior::from_label(&v)),
            toml_apply: |ts, p| p.enter_behavior = ts.enter_behavior,
            read: |src, out| out.enter_behavior = src.enter_behavior,
            write: |ps, ts| ts.enter_behavior = ps.enter_behavior,
            ui_meta: {
                label: "Enter Behavior",
                category: "Application",
                subtitle: Some(
                    "Action when pressing Enter on items (all views except Queue)",
                ),
                default: "Play All",
                options: &["Play All", "Play Single", "Append & Play"],
                read_field: |d| d.enter_behavior.as_ref(),
            },
        },
        LibraryPageSize {
            key: "general.library_page_size",
            value_type: Enum,
            setter: |mgr, v: String| mgr.set_library_page_size(LibraryPageSize::from_label(&v)),
            toml_apply: |ts, p| p.library_page_size = ts.library_page_size,
            read: |src, out| out.library_page_size = src.library_page_size,
            write: |ps, ts| ts.library_page_size = ps.library_page_size,
            ui_meta: {
                label: "Library Page Size",
                category: "Application",
                subtitle: Some(
                    "Items fetched per API request · larger = fewer loads, more memory",
                ),
                default: "Default (500)",
                options: &[
                    "Small (100)",
                    "Default (500)",
                    "Large (1,000)",
                    "Massive (5,000)",
                ],
                read_field: |d| d.library_page_size.as_ref(),
            },
        },
        SuppressLibraryRefreshToasts {
            key: "general.suppress_library_refresh_toasts",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_suppress_library_refresh_toasts(v),
            toml_apply: |ts, p| p.suppress_library_refresh_toasts = ts.suppress_library_refresh_toasts,
            read: |src, out| out.suppress_library_refresh_toasts = src.suppress_library_refresh_toasts,
            write: |ps, ts| ts.suppress_library_refresh_toasts = ps.suppress_library_refresh_toasts,
            ui_meta: {
                label: "Suppress Library Refresh Toasts",
                category: "Application",
                subtitle: Some("Hide the notification shown when the server reports a library refresh"),
                default: false,
                read_field: |d| d.suppress_library_refresh_toasts,
            },
        },
        AutoFollowPlaying {
            key: "general.auto_follow_playing",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_auto_follow_playing(v),
            toml_apply: |ts, p| p.auto_follow_playing = ts.auto_follow_playing,
            read: |src, out| out.auto_follow_playing = src.auto_follow_playing,
            write: |ps, ts| ts.auto_follow_playing = ps.auto_follow_playing,
            ui_meta: {
                label: "Auto-Follow Playing Track",
                category: "Mouse Behavior",
                subtitle: Some("Scroll to current track on track changes"),
                default: true,
                read_field: |d| d.auto_follow_playing,
            },
        },
        ShowTrayIcon {
            key: "general.show_tray_icon",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_show_tray_icon(v),
            toml_apply: |ts, p| p.show_tray_icon = ts.show_tray_icon,
            read: |src, out| out.show_tray_icon = src.show_tray_icon,
            write: |ps, ts| ts.show_tray_icon = ps.show_tray_icon,
            ui_meta: {
                label: "Show Tray Icon",
                category: "System Tray",
                subtitle: Some(
                    "Register a system tray icon · requires a status bar with tray support \
                     (e.g. waybar with the `tray` module on Hyprland)",
                ),
                default: false,
                read_field: |d| d.show_tray_icon,
            },
        },
        CloseToTray {
            key: "general.close_to_tray",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_close_to_tray(v),
            toml_apply: |ts, p| p.close_to_tray = ts.close_to_tray,
            read: |src, out| out.close_to_tray = src.close_to_tray,
            write: |ps, ts| ts.close_to_tray = ps.close_to_tray,
            ui_meta: {
                label: "Close to Tray",
                category: "System Tray",
                subtitle: Some(
                    "X button hides the window into the tray instead of quitting · \
                     requires Show Tray Icon",
                ),
                default: false,
                read_field: |d| d.close_to_tray,
            },
        },
        // -- Migrated from the legacy `match key.as_str()` arm via on_dispatch ----
        // `general.light_mode` has no redb persistence path — it lives in
        // the theme module's atomic + `config.toml` only. The setter is a
        // deliberate no-op; `on_dispatch:` returns the bool the UI handler
        // then writes to both. Note: `apply_toml_settings_to_internal` still
        // copies `p.light_mode = ts.light_mode` directly; the duplicated
        // assignment here is idempotent and lives next to the dispatch entry
        // for discoverability.
        LightMode {
            key: "general.light_mode",
            value_type: Enum,
            setter: |_mgr, _v: String| Ok(()),
            toml_apply: |ts, p| p.light_mode = ts.light_mode,
            // `light_mode` lives in the theme atomic + config.toml, not on the
            // UI-facing `PlayerSettings` (see player_settings/mod.rs:25). The
            // dump and write are intentionally no-ops; on_dispatch carries
            // the truth, and the on-disk TOML write is owned by the
            // `SetLightModeAtomic` side-effect handler in the UI crate via a
            // targeted `update_config_value` call (not via
            // `from_player_settings`).
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            on_dispatch: |v: String| SettingsSideEffect::SetLightModeAtomic(v == "Light"),
        },
        // The setter trims user-typed leading/trailing whitespace before
        // persisting, matching the legacy arm. The UI `local_music_path`
        // mirror on `Nokkvi` is repopulated by `handle_player_settings_loaded`
        // after the round-trip, so no explicit `on_dispatch` is needed.
        LocalMusicPath {
            key: "general.local_music_path",
            value_type: Text,
            setter: |mgr, v: String| mgr.set_local_music_path(v.trim().to_string()),
            toml_apply: |ts, p| p.local_music_path = ts.local_music_path.clone(),
            read: |src, out| out.local_music_path = src.local_music_path.clone(),
            write: |ps, ts| ts.local_music_path = ps.local_music_path.clone(),
            ui_meta: {
                label: "Local Music Path",
                category: "Application",
                subtitle: Some(
                    "Path to music on this machine for 'Open in File Manager' · \
                     press Enter to edit",
                ),
                default: "",
                read_field: |d| d.local_music_path.as_ref(),
            },
        },
        ShowAlbumArtistsOnly {
            key: "general.show_album_artists_only",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_show_album_artists_only(v),
            toml_apply: |ts, p| p.show_album_artists_only = ts.show_album_artists_only,
            read: |src, out| out.show_album_artists_only = src.show_album_artists_only,
            write: |ps, ts| ts.show_album_artists_only = ps.show_album_artists_only,
            on_dispatch: |_v: bool| SettingsSideEffect::LoadArtists,
            ui_meta: {
                label: "Album Artists Only",
                category: "Application",
                subtitle: Some(
                    "Only show artists that have explicitly released albums, \
                     hiding compilation/guest artists",
                ),
                default: true,
                read_field: |d| d.show_album_artists_only,
            },
        },
        ArtworkResolutionKey {
            key: "general.artwork_resolution",
            value_type: Enum,
            setter: |mgr, v: String| mgr.set_artwork_resolution(ArtworkResolution::from_label(&v)),
            toml_apply: |ts, p| p.artwork_resolution = ts.artwork_resolution,
            read: |src, out| out.artwork_resolution = src.artwork_resolution,
            write: |ps, ts| ts.artwork_resolution = ps.artwork_resolution,
            on_dispatch: |_v: String| SettingsSideEffect::Toast {
                level: ToastLevel::Info,
                message: "Artwork resolution changed — new artwork will fetch at this size"
                    .to_string(),
            },
            ui_meta: {
                label: "Artwork Resolution",
                category: "Application",
                subtitle: Some(
                    "Panel image quality · higher = sharper on HiDPI, larger cache",
                ),
                default: "Default (1000px)",
                options: &[
                    "Default (1000px)",
                    "High (1500px)",
                    "Ultra (2000px)",
                    "Original (Full Size)",
                ],
                read_field: |d| d.artwork_resolution.as_ref(),
            },
        },
        // The setter writes only redb (via `save_redb_only`); the UI handler
        // owns the synchronous TOML write/strip and the follow-up
        // `write_all_toml_public` flush. See
        // `dispatch_settings_side_effect` in `update/settings.rs`.
        VerboseConfig {
            key: "general.verbose_config",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_verbose_config(v),
            toml_apply: |ts, p| p.verbose_config = ts.verbose_config,
            read: |src, out| out.verbose_config = src.verbose_config,
            write: |ps, ts| ts.verbose_config = ps.verbose_config,
            on_dispatch: |v: bool| SettingsSideEffect::WriteVerboseConfig { enabled: v },
            ui_meta: {
                label: "Verbose Config",
                category: "Application",
                subtitle: Some(
                    "Write all settings to config.toml, including unchanged defaults",
                ),
                default: false,
                read_field: |d| d.verbose_config,
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
            settings_data::GeneralSettingsData, toml_settings::TomlSettings,
        },
    };

    /// Returns a `(SettingsManager, TempDir)` pair. The caller MUST keep the
    /// `TempDir` alive for the duration of the test — its `Drop` deletes the
    /// directory backing the redb file.
    fn make_test_manager() -> (SettingsManager, TempDir) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("test_settings.redb");
        let storage = StateStorage::new(path).expect("StateStorage::new");
        (SettingsManager::for_test(storage), tmp)
    }

    #[test]
    fn dispatch_general_stable_viewport_persists_via_setter() {
        let (mut mgr, _tmp) = make_test_manager();
        // Default is `true`; flip to `false` and confirm the setter ran.
        assert!(mgr.get_player_settings().stable_viewport);

        let result = dispatch_general_tab_setting(
            "general.stable_viewport",
            SettingValue::Bool(false),
            &mut mgr,
        );

        assert!(matches!(result, Some(Ok(SettingsSideEffect::None))));
        assert!(!mgr.get_player_settings().stable_viewport);
    }

    #[test]
    fn dispatch_general_returns_none_for_unknown_key() {
        let (mut mgr, _tmp) = make_test_manager();
        let result =
            dispatch_general_tab_setting("nonexistent.key", SettingValue::Bool(false), &mut mgr);
        assert!(result.is_none());
    }

    #[test]
    fn dispatch_general_returns_err_on_type_mismatch() {
        let (mut mgr, _tmp) = make_test_manager();
        let result = dispatch_general_tab_setting(
            "general.stable_viewport",
            SettingValue::Int {
                val: 1,
                min: 0,
                max: 10,
                step: 1,
                unit: "",
            },
            &mut mgr,
        );
        assert!(matches!(result, Some(Err(_))));
    }

    #[test]
    fn apply_toml_general_copies_stable_viewport() {
        let mut ts = TomlSettings::default();
        ts.stable_viewport = false;
        let mut p = PlayerSettings::default();
        p.stable_viewport = true;
        apply_toml_general_tab(&ts, &mut p);
        assert!(!p.stable_viewport);
    }

    #[test]
    fn tab_general_contains_recognizes_declared_keys() {
        assert!(tab_general_contains("general.stable_viewport"));
        assert!(tab_general_contains("general.start_view"));
        assert!(tab_general_contains("general.enter_behavior"));
        assert!(tab_general_contains("general.library_page_size"));
        assert!(tab_general_contains(
            "general.suppress_library_refresh_toasts"
        ));
        assert!(tab_general_contains("general.auto_follow_playing"));
        assert!(tab_general_contains("general.show_tray_icon"));
        assert!(tab_general_contains("general.close_to_tray"));
        assert!(tab_general_contains("general.light_mode"));
        assert!(tab_general_contains("general.local_music_path"));
        assert!(tab_general_contains("general.show_album_artists_only"));
        assert!(tab_general_contains("general.artwork_resolution"));
        assert!(tab_general_contains("general.verbose_config"));
        assert!(!tab_general_contains("nonexistent.key"));
    }

    #[test]
    fn tab_general_settings_lists_stable_viewport() {
        assert!(
            TAB_GENERAL_SETTINGS
                .iter()
                .any(|d| d.key == "general.stable_viewport")
        );
    }

    // -------------------------------------------------------------------------
    // Round-trip coverage: one Bool, one Enum, one Text family member touched
    // by this slice.
    // -------------------------------------------------------------------------

    #[test]
    fn dispatch_general_auto_follow_playing_round_trip() {
        let (mut mgr, _tmp) = make_test_manager();
        // Default is `true`; flip to `false` then back, confirming both writes
        // hit the setter.
        assert!(mgr.get_player_settings().auto_follow_playing);

        let result = dispatch_general_tab_setting(
            "general.auto_follow_playing",
            SettingValue::Bool(false),
            &mut mgr,
        );
        assert!(matches!(result, Some(Ok(SettingsSideEffect::None))));
        assert!(!mgr.get_player_settings().auto_follow_playing);

        let result = dispatch_general_tab_setting(
            "general.auto_follow_playing",
            SettingValue::Bool(true),
            &mut mgr,
        );
        assert!(matches!(result, Some(Ok(SettingsSideEffect::None))));
        assert!(mgr.get_player_settings().auto_follow_playing);
    }

    #[test]
    fn dispatch_general_enter_behavior_converts_label_via_from_label() {
        let (mut mgr, _tmp) = make_test_manager();
        let result = dispatch_general_tab_setting(
            "general.enter_behavior",
            SettingValue::Enum {
                val: "Append & Play".to_string(),
                options: vec![],
            },
            &mut mgr,
        );
        assert!(matches!(result, Some(Ok(SettingsSideEffect::None))));
        assert_eq!(
            mgr.get_player_settings().enter_behavior,
            EnterBehavior::AppendAndPlay
        );
    }

    #[test]
    fn dispatch_general_start_view_persists_via_str_setter() {
        let (mut mgr, _tmp) = make_test_manager();
        let result = dispatch_general_tab_setting(
            "general.start_view",
            SettingValue::Enum {
                val: "Albums".to_string(),
                options: vec![],
            },
            &mut mgr,
        );
        assert!(matches!(result, Some(Ok(SettingsSideEffect::None))));
        assert_eq!(mgr.get_player_settings().start_view, "Albums");
    }

    #[test]
    fn apply_toml_general_copies_migrated_fields() {
        let mut ts = TomlSettings::default();
        ts.stable_viewport = false;
        ts.start_view = "Songs".to_string();
        ts.enter_behavior = EnterBehavior::PlaySingle;
        ts.library_page_size = LibraryPageSize::Large;
        ts.auto_follow_playing = false;
        ts.suppress_library_refresh_toasts = true;
        ts.show_tray_icon = true;
        ts.close_to_tray = true;

        let mut p = PlayerSettings::default();
        apply_toml_general_tab(&ts, &mut p);

        assert!(!p.stable_viewport);
        assert_eq!(p.start_view, "Songs");
        assert_eq!(p.enter_behavior, EnterBehavior::PlaySingle);
        assert_eq!(p.library_page_size, LibraryPageSize::Large);
        assert!(!p.auto_follow_playing);
        assert!(p.suppress_library_refresh_toasts);
        assert!(p.show_tray_icon);
        assert!(p.close_to_tray);
    }

    /// Write-side: `write_general_tab_toml` copies the migrated fields from
    /// the UI-facing `PlayerSettings` onto `TomlSettings` for serialization
    /// back to `config.toml`. Inverse of `apply_toml_general_tab`. The
    /// `light_mode` entry deliberately declares a no-op `write:` closure
    /// (UI-PS has no light_mode field); this test confirms `ts.light_mode`
    /// keeps the value it had before `write_general_tab_toml` ran.
    #[test]
    fn write_general_round_trip_copies_migrated_fields_to_toml() {
        let mut ps = crate::types::player_settings::PlayerSettings::default();
        ps.stable_viewport = false;
        ps.start_view = "Songs".to_string();
        ps.enter_behavior = EnterBehavior::PlaySingle;
        ps.library_page_size = LibraryPageSize::Large;
        ps.auto_follow_playing = false;
        ps.suppress_library_refresh_toasts = true;
        ps.show_tray_icon = true;
        ps.close_to_tray = true;
        ps.local_music_path = "/tmp/test_lib".to_string();
        ps.show_album_artists_only = false;
        ps.artwork_resolution = ArtworkResolution::Ultra;
        ps.verbose_config = true;

        // Seed the destination with `light_mode = true` so we can confirm the
        // no-op `write:` closure does NOT stomp it.
        let mut ts = TomlSettings {
            light_mode: true,
            ..TomlSettings::default()
        };

        write_general_tab_toml(&ps, &mut ts);

        assert!(!ts.stable_viewport);
        assert_eq!(ts.start_view, "Songs");
        assert_eq!(ts.enter_behavior, EnterBehavior::PlaySingle);
        assert_eq!(ts.library_page_size, LibraryPageSize::Large);
        assert!(!ts.auto_follow_playing);
        assert!(ts.suppress_library_refresh_toasts);
        assert!(ts.show_tray_icon);
        assert!(ts.close_to_tray);
        assert_eq!(ts.local_music_path, "/tmp/test_lib");
        assert!(!ts.show_album_artists_only);
        assert_eq!(ts.artwork_resolution, ArtworkResolution::Ultra);
        assert!(ts.verbose_config);
        assert!(
            ts.light_mode,
            "light_mode entry declares a no-op write — must NOT stomp ts.light_mode",
        );
    }

    /// Read-side: `dump_general_tab_player_settings` copies the migrated
    /// fields from the redb-backed internal `PlayerSettings` onto the
    /// UI-facing struct consumed by `Message::PlayerSettingsLoaded`. Set every
    /// migrated field on the source to a non-default value, dump, and confirm
    /// the destination received each one — including the `String` clone for
    /// `start_view`.
    #[test]
    fn dump_general_round_trip_copies_migrated_fields() {
        let (mgr, _tmp) = make_test_manager();
        let mut ui = mgr.get_player_settings();

        let mut src = PlayerSettings::default();
        src.stable_viewport = false;
        src.start_view = "Songs".to_string();
        src.enter_behavior = EnterBehavior::PlaySingle;
        src.library_page_size = LibraryPageSize::Large;
        src.auto_follow_playing = false;
        src.suppress_library_refresh_toasts = true;
        src.show_tray_icon = true;
        src.close_to_tray = true;

        dump_general_tab_player_settings(&src, &mut ui);

        assert!(!ui.stable_viewport);
        assert_eq!(ui.start_view, "Songs");
        assert_eq!(ui.enter_behavior, EnterBehavior::PlaySingle);
        assert_eq!(ui.library_page_size, LibraryPageSize::Large);
        assert!(!ui.auto_follow_playing);
        assert!(ui.suppress_library_refresh_toasts);
        assert!(ui.show_tray_icon);
        assert!(ui.close_to_tray);
    }

    // -------------------------------------------------------------------------
    // Side-effect coverage: each migrated legacy arm has a distinct
    // `SettingsSideEffect` variant. Verify the dispatcher emits the right
    // variant *and* that the redb-backed setter still round-trips when one
    // exists. `light_mode` deliberately has no redb path; its truth lives in
    // the UI handler that consumes `SetLightModeAtomic`.
    // -------------------------------------------------------------------------

    #[test]
    fn dispatch_general_light_mode_emits_atomic_side_effect() {
        let (mut mgr, _tmp) = make_test_manager();

        let result = dispatch_general_tab_setting(
            "general.light_mode",
            SettingValue::Enum {
                val: "Light".to_string(),
                options: vec![],
            },
            &mut mgr,
        );
        match result {
            Some(Ok(SettingsSideEffect::SetLightModeAtomic(true))) => {}
            other => panic!("expected SetLightModeAtomic(true), got {other:?}"),
        }

        let result = dispatch_general_tab_setting(
            "general.light_mode",
            SettingValue::Enum {
                val: "Dark".to_string(),
                options: vec![],
            },
            &mut mgr,
        );
        match result {
            Some(Ok(SettingsSideEffect::SetLightModeAtomic(false))) => {}
            other => panic!("expected SetLightModeAtomic(false), got {other:?}"),
        }
    }

    #[test]
    fn dispatch_general_local_music_path_trims_before_persist() {
        let (mut mgr, _tmp) = make_test_manager();

        let result = dispatch_general_tab_setting(
            "general.local_music_path",
            SettingValue::Text("  /music/Library  ".to_string()),
            &mut mgr,
        );

        assert!(matches!(result, Some(Ok(SettingsSideEffect::None))));
        assert_eq!(
            mgr.get_player_settings().local_music_path,
            "/music/Library",
            "leading + trailing whitespace must be trimmed before persisting"
        );
    }

    #[test]
    fn dispatch_general_show_album_artists_only_emits_load_artists() {
        let (mut mgr, _tmp) = make_test_manager();
        // Default is `true`; flip to `false` and confirm both the redb side
        // and the side-effect emission.
        assert!(mgr.get_player_settings().show_album_artists_only);

        let result = dispatch_general_tab_setting(
            "general.show_album_artists_only",
            SettingValue::Bool(false),
            &mut mgr,
        );

        match result {
            Some(Ok(SettingsSideEffect::LoadArtists)) => {}
            other => panic!("expected LoadArtists, got {other:?}"),
        }
        assert!(!mgr.get_player_settings().show_album_artists_only);
    }

    #[test]
    fn dispatch_general_artwork_resolution_emits_info_toast() {
        let (mut mgr, _tmp) = make_test_manager();

        let result = dispatch_general_tab_setting(
            "general.artwork_resolution",
            SettingValue::Enum {
                val: "Ultra".to_string(),
                options: vec![],
            },
            &mut mgr,
        );

        match result {
            Some(Ok(SettingsSideEffect::Toast {
                level: ToastLevel::Info,
                ref message,
            })) => {
                assert!(
                    message.contains("fetch at this size"),
                    "toast message should mention new fetches at the new size, got: {message}"
                );
            }
            ref other => panic!("expected Toast{{ Info, … }}, got {other:?}"),
        }
        assert_eq!(
            mgr.get_player_settings().artwork_resolution,
            ArtworkResolution::from_label("Ultra"),
            "redb side-effect of the setter must still run"
        );
    }

    /// Sample data with all defaults, used to exercise the macro-emitted
    /// items helper.
    fn default_general_data() -> GeneralSettingsData {
        GeneralSettingsData {
            server_url: "http://localhost:4533".into(),
            username: "admin".into(),
            start_view: "Queue".into(),
            stable_viewport: true,
            auto_follow_playing: true,
            enter_behavior: "Play All".into(),
            local_music_path: "".into(),
            verbose_config: false,
            library_page_size: "Default (500)".into(),
            artwork_resolution: "Default (1000px)".into(),
            show_album_artists_only: true,
            suppress_library_refresh_toasts: false,
            show_tray_icon: false,
            close_to_tray: false,
        }
    }

    /// Macro-emitted helper returns one entry per ui_meta-bearing setting.
    /// 12 of the 13 declared keys carry ui_meta — light_mode does not (it
    /// renders on the Theme tab, not General), so the helper emits 12 rows.
    #[test]
    fn build_general_tab_settings_items_emits_one_row_per_ui_meta_entry() {
        let data = default_general_data();
        let entries = build_general_tab_settings_items(&data);
        assert_eq!(entries.len(), 12);
        // Every emitted entry is a Item, never a Header — section headers
        // live in the UI crate's hand-written builder.
        for e in &entries {
            assert!(matches!(e, SettingsEntry::Item(_)));
        }
    }

    /// `read_field` reads from the live data borrow so flipping a value
    /// reaches the emitted row.
    #[test]
    fn build_general_tab_settings_items_uses_data_for_values() {
        let mut data = default_general_data();
        data.stable_viewport = false;
        data.start_view = "Albums".into();
        let entries = build_general_tab_settings_items(&data);
        let stable = entries
            .iter()
            .find_map(|e| match e {
                SettingsEntry::Item(item) if item.key.as_ref() == "general.stable_viewport" => {
                    Some(item)
                }
                _ => None,
            })
            .expect("stable_viewport row");
        match &stable.value {
            SettingValue::Bool(v) => assert!(!v),
            other => panic!("expected Bool, got {other:?}"),
        }
        let start = entries
            .iter()
            .find_map(|e| match e {
                SettingsEntry::Item(item) if item.key.as_ref() == "general.start_view" => {
                    Some(item)
                }
                _ => None,
            })
            .expect("start_view row");
        match &start.value {
            SettingValue::Enum { val, .. } => assert_eq!(val, "Albums"),
            other => panic!("expected Enum, got {other:?}"),
        }
    }

    #[test]
    fn dispatch_general_verbose_config_emits_write_side_effect() {
        let (mut mgr, _tmp) = make_test_manager();
        assert!(!mgr.get_player_settings().verbose_config);

        let result = dispatch_general_tab_setting(
            "general.verbose_config",
            SettingValue::Bool(true),
            &mut mgr,
        );

        match result {
            Some(Ok(SettingsSideEffect::WriteVerboseConfig { enabled: true })) => {}
            other => panic!("expected WriteVerboseConfig {{ enabled: true }}, got {other:?}"),
        }
        assert!(
            mgr.get_player_settings().verbose_config,
            "setter must run synchronously even though the TOML write defers to the UI handler"
        );
    }
}
