//! Tests for settings dispatch update handlers.
//!
//! Interface-tab keys (`strip_*`, `*_artwork_overlay`, etc.) and the
//! migrated general-tab side-effect keys (`light_mode`, `local_music_path`,
//! `show_album_artists_only`, `artwork_resolution`, `verbose_config`) all
//! route through `define_settings!` in
//! `nokkvi_data::services::settings_tables`. The data crate owns the
//! dispatch + apply round-trip tests; the UI-side coverage below verifies
//! that `dispatch_settings_side_effect` translates each
//! [`SettingsSideEffect`] variant into the right user-visible follow-up:
//! toast pushed at the right level, `LoadArtists` / Tick task chained,
//! verbose-config TOML toast surfaced.
//!
//! The `artwork_overlay_*` tests below cover the `handle_player_settings_loaded`
//! path: when settings with `albums_artwork_overlay = false` (or any other
//! per-view variant) arrive, the corresponding process-global theme atomic must
//! be flipped. Each test saves the prior value and restores it on exit to avoid
//! bleeding state into the shared `UI_MODE` statics for parallel tests.

use nokkvi_data::{
    services::settings_tables::SettingsSideEffect,
    types::{player_settings::PlayerSettings, toast::ToastLevel},
};

use crate::test_helpers::*;

#[test]
fn side_effect_none_does_not_push_a_toast() {
    let mut app = test_app();
    assert!(app.toast.toasts.is_empty());

    let _task = app.dispatch_settings_side_effect(SettingsSideEffect::None);

    assert!(
        app.toast.toasts.is_empty(),
        "SettingsSideEffect::None must be a pure pass-through"
    );
}

#[test]
fn side_effect_toast_info_pushes_info_toast() {
    let mut app = test_app();

    let _task = app.dispatch_settings_side_effect(SettingsSideEffect::Toast {
        level: ToastLevel::Info,
        message: "hello".to_string(),
    });

    assert_eq!(app.toast.toasts.len(), 1);
    assert_eq!(app.toast.toasts[0].level, ToastLevel::Info);
    assert_eq!(app.toast.toasts[0].message, "hello");
}

#[test]
fn side_effect_toast_success_pushes_success_toast() {
    let mut app = test_app();

    let _task = app.dispatch_settings_side_effect(SettingsSideEffect::Toast {
        level: ToastLevel::Success,
        message: "done".to_string(),
    });

    assert_eq!(app.toast.toasts.len(), 1);
    assert_eq!(app.toast.toasts[0].level, ToastLevel::Success);
}

#[test]
fn side_effect_toast_warning_pushes_warning_toast() {
    let mut app = test_app();

    let _task = app.dispatch_settings_side_effect(SettingsSideEffect::Toast {
        level: ToastLevel::Warning,
        message: "heads up".to_string(),
    });

    assert_eq!(app.toast.toasts.len(), 1);
    assert_eq!(app.toast.toasts[0].level, ToastLevel::Warning);
}

#[test]
fn side_effect_toast_error_pushes_error_toast() {
    let mut app = test_app();

    let _task = app.dispatch_settings_side_effect(SettingsSideEffect::Toast {
        level: ToastLevel::Error,
        message: "boom".to_string(),
    });

    assert_eq!(app.toast.toasts.len(), 1);
    assert_eq!(app.toast.toasts[0].level, ToastLevel::Error);
}

#[test]
fn side_effect_load_artists_does_not_push_a_toast() {
    // The `Task<Message>` returned by `dispatch_settings_side_effect` is
    // opaque to unit tests, so we can only assert on observable state. The
    // important property is that emitting `LoadArtists` does NOT also push
    // a toast — that would be a UX regression on the show-album-artists
    // toggle, where the legacy arm intentionally ran silent.
    let mut app = test_app();

    let _task = app.dispatch_settings_side_effect(SettingsSideEffect::LoadArtists);

    assert!(app.toast.toasts.is_empty());
}

#[test]
fn side_effect_set_light_mode_atomic_does_not_push_a_toast() {
    // The legacy `general.light_mode` arm only flipped the theme atomic and
    // wrote `config.toml`; it never surfaced a toast. The audit warns
    // against asserting on the process-global theme atomic in tests, so we
    // verify the silent contract: no toast surfaces on a side-effect run.
    let mut app = test_app();
    let prior_state = crate::theme::is_light_mode();

    let _task =
        app.dispatch_settings_side_effect(SettingsSideEffect::SetLightModeAtomic(!prior_state));

    assert!(
        app.toast.toasts.is_empty(),
        "SetLightModeAtomic must not push a toast; it only flips the atomic + config.toml"
    );

    // Restore the global atomic so this test does not bleed state into the
    // shared theme module that other tests in this binary may depend on.
    crate::theme::set_light_mode(prior_state);
}

#[test]
fn side_effect_write_verbose_config_enable_emits_success_toast() {
    // `WriteVerboseConfig { enabled: true }` writes the [visualizer]
    // section to `config.toml`. In a unit test the working directory points
    // at the repo, so the write may fail (file permissions / not present);
    // either way one toast (success or warn) MUST be pushed so the user
    // gets feedback.
    let mut app = test_app();

    let _task =
        app.dispatch_settings_side_effect(SettingsSideEffect::WriteVerboseConfig { enabled: true });

    assert_eq!(
        app.toast.toasts.len(),
        1,
        "verbose_config toggle must push exactly one toast (success or warn)"
    );
    let level = app.toast.toasts[0].level;
    assert!(
        matches!(level, ToastLevel::Success | ToastLevel::Warning),
        "expected Success or Warning, got {level:?}"
    );
}

#[test]
fn side_effect_write_verbose_config_disable_emits_success_toast() {
    let mut app = test_app();

    let _task = app
        .dispatch_settings_side_effect(SettingsSideEffect::WriteVerboseConfig { enabled: false });

    assert_eq!(
        app.toast.toasts.len(),
        1,
        "verbose_config toggle must push exactly one toast (success or warn)"
    );
    let level = app.toast.toasts[0].level;
    assert!(
        matches!(level, ToastLevel::Success | ToastLevel::Warning),
        "expected Success or Warning, got {level:?}"
    );
}

// ============================================================================
// Artwork overlay theme-atomic tests (B5)
// ============================================================================

#[test]
fn player_settings_loaded_albums_artwork_overlay_false_clears_atomic() {
    let mut app = test_app();
    let prior = crate::theme::albums_artwork_overlay();

    let _ = app.handle_player_settings_loaded(PlayerSettings {
        albums_artwork_overlay: false,
        ..Default::default()
    });

    assert!(
        !crate::theme::albums_artwork_overlay(),
        "albums_artwork_overlay atomic must be false after loading settings with false"
    );

    // Restore to avoid bleeding state between tests.
    crate::theme::set_albums_artwork_overlay(prior);
}

#[test]
fn player_settings_loaded_albums_artwork_overlay_true_sets_atomic() {
    // Force the atomic to false first so the test is self-contained regardless
    // of the initial global state (avoids false-pass when it's already true).
    crate::theme::set_albums_artwork_overlay(false);
    let mut app = test_app();

    let _ = app.handle_player_settings_loaded(PlayerSettings {
        albums_artwork_overlay: true,
        ..Default::default()
    });

    assert!(
        crate::theme::albums_artwork_overlay(),
        "albums_artwork_overlay atomic must be true after loading settings with true"
    );

    // Restore default (true — the real default from toml_settings.rs).
    crate::theme::set_albums_artwork_overlay(true);
}

#[test]
fn player_settings_loaded_artists_artwork_overlay_flips_atomic() {
    let mut app = test_app();
    let prior = crate::theme::artists_artwork_overlay();

    let _ = app.handle_player_settings_loaded(PlayerSettings {
        artists_artwork_overlay: false,
        ..Default::default()
    });
    assert!(!crate::theme::artists_artwork_overlay());

    crate::theme::set_artists_artwork_overlay(prior);
}

#[test]
fn player_settings_loaded_songs_artwork_overlay_flips_atomic() {
    let mut app = test_app();
    let prior = crate::theme::songs_artwork_overlay();

    let _ = app.handle_player_settings_loaded(PlayerSettings {
        songs_artwork_overlay: false,
        ..Default::default()
    });
    assert!(!crate::theme::songs_artwork_overlay());

    crate::theme::set_songs_artwork_overlay(prior);
}

#[test]
fn player_settings_loaded_playlists_artwork_overlay_flips_atomic() {
    let mut app = test_app();
    let prior = crate::theme::playlists_artwork_overlay();

    let _ = app.handle_player_settings_loaded(PlayerSettings {
        playlists_artwork_overlay: false,
        ..Default::default()
    });
    assert!(!crate::theme::playlists_artwork_overlay());

    crate::theme::set_playlists_artwork_overlay(prior);
}

// ============================================================================
// Restore-defaults sentinel routing (Tier 0 #0.2)
// ============================================================================
//
// The Hotkeys tab "Restore Defaults" row uses the key `__restore_all_hotkeys`,
// which is parsed by `SentinelKind::from_key` and routed into
// `handle_restore_defaults` via the typed dispatch. That function must early-return
// `OpenResetHotkeysDialog` for the all-hotkeys sentinel rather than falling
// through to the HexColor scan path (which is intended for color-group resets
// like `__restore_bg` / `__restore_accent`).

#[test]
fn handle_restore_defaults_all_hotkeys_opens_reset_dialog() {
    use crate::views::settings::SettingsAction;

    let mut app = test_app();
    let action = app
        .settings_page
        .handle_restore_defaults("__restore_all_hotkeys");

    assert!(
        matches!(action, SettingsAction::OpenResetHotkeysDialog),
        "__restore_all_hotkeys must route to OpenResetHotkeysDialog, got {action:?}"
    );
}

#[test]
fn handle_restore_defaults_visualizer_opens_reset_dialog() {
    // Non-regression: __restore_visualizer must still open its dialog.
    use crate::views::settings::SettingsAction;

    let mut app = test_app();
    let action = app
        .settings_page
        .handle_restore_defaults("__restore_visualizer");

    assert!(
        matches!(action, SettingsAction::OpenResetVisualizerDialog),
        "__restore_visualizer must route to OpenResetVisualizerDialog, got {action:?}"
    );
}

#[test]
fn handle_restore_defaults_theme_returns_restore_color_group() {
    // Non-regression: __restore_theme must still return RestoreColorGroup
    // (with empty entries — the side effect is on-disk restore via presets).
    use crate::views::settings::SettingsAction;

    let mut app = test_app();
    let action = app.settings_page.handle_restore_defaults("__restore_theme");

    assert!(
        matches!(action, SettingsAction::RestoreColorGroup { .. }),
        "__restore_theme must route to RestoreColorGroup, got {action:?}"
    );
}

#[test]
fn handle_restore_defaults_color_group_with_no_cached_entries_returns_none() {
    // Non-regression: when called with a generic __restore_* color key
    // (e.g. __restore_bg) and no HexColor entries cached, the function
    // returns SettingsAction::None — the HexColor scan path is preserved.
    use crate::views::settings::SettingsAction;

    let mut app = test_app();
    let action = app.settings_page.handle_restore_defaults("__restore_bg");

    assert!(
        matches!(action, SettingsAction::None),
        "__restore_bg with no cached HexColor entries must return None, got {action:?}"
    );
}

// ============================================================================
// Sentinel-key dispatch characterization
// ============================================================================
//
// These tests pin the `__*` sentinel-key dispatch surface so changes to
// SentinelKind cannot silently change observable routing. They cover:
//
//   * `SentinelKind::from_key` parsing for each registered sentinel,
//   * `__preset_N` numeric index extraction (drives `ApplyPreset`),
//   * `handle_restore_defaults` with a populated `cached_entries` for
//     `__restore_bg` — exercises the HexColor scan path that returns a
//     non-empty `RestoreColorGroup`,
//   * the explicit non-sentinel-ness of `__toggle_*` keys (regular
//     ToggleSet keys that must NOT parse into `SentinelKind`).
//
// The `SentinelKind` enum itself owns from_key/to_key round-trip coverage
// in `settings/sentinel.rs`; these tests are the dispatch-level anchors.

#[test]
fn sentinel_action_logout_parses_to_logout() {
    use crate::views::settings::sentinel::SentinelKind;

    assert_eq!(
        SentinelKind::from_key("__action_logout"),
        Some(SentinelKind::Logout)
    );
}

#[test]
fn sentinel_preset_zero_extracts_index() {
    use crate::views::settings::sentinel::SentinelKind;

    assert_eq!(
        SentinelKind::from_key("__preset_0"),
        Some(SentinelKind::PresetTheme(0))
    );
}

#[test]
fn sentinel_preset_seven_extracts_index() {
    use crate::views::settings::sentinel::SentinelKind;

    assert_eq!(
        SentinelKind::from_key("__preset_7"),
        Some(SentinelKind::PresetTheme(7))
    );
}

#[test]
fn sentinel_toggle_keys_are_not_sentinels() {
    // Regular ToggleSet keys — must NOT parse into SentinelKind, since they
    // route through `ToggleSetToggle`, not the EditActivate sentinel dispatch.
    use crate::views::settings::sentinel::SentinelKind;

    assert_eq!(SentinelKind::from_key("__toggle_artwork_overlays"), None);
    assert_eq!(SentinelKind::from_key("__toggle_strip_fields"), None);
}

#[test]
fn handle_restore_defaults_bg_with_cached_entries_returns_non_empty_group() {
    // When `cached_entries` contains HexColor items in the "Background
    // Colors" category, __restore_bg must collect them into
    // `RestoreColorGroup { entries }` with a non-empty Vec.
    use nokkvi_data::types::setting_item::{SettingItem, SettingMeta};

    use crate::views::settings::SettingsAction;

    let mut app = test_app();

    // Seed cached_entries with the __restore_bg row + a HexColor row in the
    // same "Background Colors" category. The category match drives the
    // scan inside `handle_restore_defaults`.
    let restore_meta = SettingMeta::new("__restore_bg", "⟲ Restore Defaults", "Background Colors");
    let color_meta = SettingMeta::new(
        "dark.background.hard",
        "BG hard (dark)",
        "Background Colors",
    );
    app.settings_page.cached_entries = vec![
        SettingItem::text(restore_meta, "Press Enter", "Press Enter"),
        SettingItem::hex_color(color_meta, "#000000", "#1d2021"),
    ];

    let action = app.settings_page.handle_restore_defaults("__restore_bg");

    match action {
        SettingsAction::RestoreColorGroup { entries } => {
            assert_eq!(
                entries.len(),
                1,
                "__restore_bg must collect the one HexColor entry in its category"
            );
            assert_eq!(entries[0].0, "dark.background.hard");
            assert_eq!(entries[0].1, "#1d2021");
        }
        other => panic!("expected RestoreColorGroup, got {other:?}"),
    }
}
