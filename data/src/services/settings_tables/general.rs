//! General-tab settings table.
//!
//! Foundation slice declares only `general.stable_viewport` to prove the
//! macro-generated dispatch + apply path end-to-end. Per-tab follow-up
//! commits will migrate the remaining `general.*` keys (Application, Mouse
//! Behavior, System Tray, Account) into this table.

use crate::{define_settings, types::setting_def::Tab};

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
        assert!(!tab_general_contains("general.start_view"));
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
}
