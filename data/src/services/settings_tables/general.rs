//! General-tab settings table.
//!
//! Migrated keys: Application (start_view, enter_behavior, library_page_size,
//! suppress_library_refresh_toasts), Mouse Behavior (stable_viewport,
//! auto_follow_playing), System Tray (show_tray_icon, close_to_tray).
//!
//! Deferred — kept in the legacy `match` arm because of UI side effects beyond
//! a plain setter call:
//!
//! - `general.local_music_path` — trims input before persisting.
//! - `general.artwork_resolution` — emits a toast prompting cache rebuild.
//! - `general.show_album_artists_only` — additionally dispatches `LoadArtists`.
//! - `general.verbose_config` — writes/strips the full TOML config and emits
//!   toasts.
//!
//! `general.server_url` and `general.username` are read-only login mirrors
//! with no setter or dispatch arm; they have no migration to perform.

use crate::{
    define_settings,
    types::{
        player_settings::{EnterBehavior, LibraryPageSize},
        setting_def::Tab,
    },
};

define_settings! {
    tab: Tab::General,
    settings_const: TAB_GENERAL_SETTINGS,
    contains_fn: tab_general_contains,
    dispatch_fn: dispatch_general_tab_setting,
    apply_fn: apply_toml_general_tab,
    settings: [
        StableViewport {
            key: "general.stable_viewport",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_stable_viewport(v),
            toml_apply: |ts, p| p.stable_viewport = ts.stable_viewport,
        },
        StartView {
            key: "general.start_view",
            value_type: Enum,
            setter: |mgr, v: String| mgr.set_start_view(&v),
            toml_apply: |ts, p| p.start_view = ts.start_view.clone(),
        },
        EnterBehavior {
            key: "general.enter_behavior",
            value_type: Enum,
            setter: |mgr, v: String| mgr.set_enter_behavior(EnterBehavior::from_label(&v)),
            toml_apply: |ts, p| p.enter_behavior = ts.enter_behavior,
        },
        LibraryPageSize {
            key: "general.library_page_size",
            value_type: Enum,
            setter: |mgr, v: String| mgr.set_library_page_size(LibraryPageSize::from_label(&v)),
            toml_apply: |ts, p| p.library_page_size = ts.library_page_size,
        },
        SuppressLibraryRefreshToasts {
            key: "general.suppress_library_refresh_toasts",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_suppress_library_refresh_toasts(v),
            toml_apply: |ts, p| p.suppress_library_refresh_toasts = ts.suppress_library_refresh_toasts,
        },
        AutoFollowPlaying {
            key: "general.auto_follow_playing",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_auto_follow_playing(v),
            toml_apply: |ts, p| p.auto_follow_playing = ts.auto_follow_playing,
        },
        ShowTrayIcon {
            key: "general.show_tray_icon",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_show_tray_icon(v),
            toml_apply: |ts, p| p.show_tray_icon = ts.show_tray_icon,
        },
        CloseToTray {
            key: "general.close_to_tray",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.set_close_to_tray(v),
            toml_apply: |ts, p| p.close_to_tray = ts.close_to_tray,
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

        assert!(matches!(result, Some(Ok(()))));
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
        assert!(!tab_general_contains("general.local_music_path")); // deferred
        assert!(!tab_general_contains("general.verbose_config")); // deferred
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
    // by this slice. The Text family has no migrated keys yet (local_music_path
    // is deferred), so it's exercised via the type-mismatch path on an existing
    // Bool entry — the macro's Text arm is already covered by the foundation's
    // dispatch_general_returns_err_on_type_mismatch when expanded for any
    // declared key, but we add a direct assertion below for clarity.
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
        assert!(matches!(result, Some(Ok(()))));
        assert!(!mgr.get_player_settings().auto_follow_playing);

        let result = dispatch_general_tab_setting(
            "general.auto_follow_playing",
            SettingValue::Bool(true),
            &mut mgr,
        );
        assert!(matches!(result, Some(Ok(()))));
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
        assert!(matches!(result, Some(Ok(()))));
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
        assert!(matches!(result, Some(Ok(()))));
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
}
