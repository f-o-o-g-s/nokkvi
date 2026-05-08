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

use nokkvi_data::{services::settings_tables::SettingsSideEffect, types::toast::ToastLevel};

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
