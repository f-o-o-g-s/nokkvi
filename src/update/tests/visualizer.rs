//! Visualizer settings-pipeline handler tests (M3).
//!
//! The visualizer behavior config rides the unified settings path:
//! `LivePlayerSettings.visualizer` (sourced from the manager's in-memory
//! config.toml-backed field) is pushed onto the render-path
//! `SharedVisualizerConfig` by `handle_player_settings_loaded`.

use crate::{test_helpers::test_app, visualizer_config::SharedVisualizerConfigExt};

/// Invariant §5 #15 (dual config.toml write): the secondary (auto-disabled)
/// key writes are DERIVED from what the data-crate setter actually changed
/// (before/after diff) — the setter is the single owner of the
/// monstercat↔waves exclusivity rule, and config.toml must receive BOTH keys
/// because config.toml wins on reload.
#[test]
fn visualizer_secondary_writes_derive_from_setter_state() {
    use nokkvi_data::types::visualizer_config::{VisualizerConfig, keys};

    use crate::{
        update::settings::visualizer_secondary_writes, views::settings::items::SettingValue,
    };

    // Enabling monstercat flipped waves off in the setter → waves=false must
    // be persisted alongside.
    let before = VisualizerConfig {
        waves: true,
        monstercat: 0.0,
        ..Default::default()
    };
    let after = VisualizerConfig {
        waves: false,
        monstercat: 1.0,
        ..Default::default()
    };
    let writes = visualizer_secondary_writes(keys::MONSTERCAT, &before, &after);
    assert_eq!(
        writes.len(),
        1,
        "exactly the auto-disabled sibling: {writes:?}"
    );
    assert!(
        matches!(&writes[0], (k, SettingValue::Bool(false)) if *k == keys::WAVES),
        "got {writes:?}"
    );

    // Enabling waves zeroed monstercat in the setter → monstercat=0.0 must be
    // persisted alongside, carrying the setter's actual post-value.
    let before = VisualizerConfig {
        waves: false,
        monstercat: 1.0,
        ..Default::default()
    };
    let after = VisualizerConfig {
        waves: true,
        monstercat: 0.0,
        ..Default::default()
    };
    let writes = visualizer_secondary_writes(keys::WAVES, &before, &after);
    assert_eq!(writes.len(), 1, "got {writes:?}");
    assert!(
        matches!(&writes[0], (k, SettingValue::Float { val, .. }) if *k == keys::MONSTERCAT && *val == 0.0),
        "got {writes:?}"
    );

    // Sub-threshold monstercat snapped to 0.0 by validate, waves untouched —
    // monstercat is the PRIMARY key (its raw value was already written), so
    // no secondary write fires.
    let before = VisualizerConfig {
        waves: true,
        monstercat: 0.0,
        ..Default::default()
    };
    let after = VisualizerConfig {
        waves: true,
        monstercat: 0.0,
        ..Default::default()
    };
    assert!(visualizer_secondary_writes(keys::MONSTERCAT, &before, &after).is_empty());

    // Disabling waves leaves monstercat alone → nothing secondary.
    let before = VisualizerConfig {
        waves: true,
        monstercat: 0.0,
        ..Default::default()
    };
    let after = VisualizerConfig {
        waves: false,
        monstercat: 0.0,
        ..Default::default()
    };
    assert!(visualizer_secondary_writes(keys::WAVES, &before, &after).is_empty());

    // An unrelated key that changed neither field → nothing secondary.
    let same = VisualizerConfig::default();
    assert!(visualizer_secondary_writes(keys::BLOOM, &same, &same.clone()).is_empty());
}

/// `handle_player_settings_loaded` pushes `settings.visualizer` onto the
/// shared render-path config (replacing the legacy standalone
/// `VisualizerConfigChanged` reload).
#[test]
fn player_settings_loaded_applies_visualizer_to_shared_config() {
    let mut app = test_app();
    let mut settings = nokkvi_data::types::player_settings::LivePlayerSettings::default();
    settings.visualizer.noise_reduction = 0.42;
    settings.visualizer.bars.led_bars = true;
    settings.visualizer.scope.point_count = 128;

    let _ = app.handle_player_settings_loaded(settings);

    let snap = app.visualizer_config.snapshot();
    assert_eq!(
        snap.noise_reduction, 0.42,
        "shared config must receive the loaded visualizer settings"
    );
    assert!(snap.bars.led_bars);
    assert_eq!(snap.scope.point_count, 128);
}

/// Review #1: a visualizer-only write must ride the SLIM apply path — it
/// mirrors onto `self.settings.visualizer`, pushes the shared render config,
/// and refreshes entries, WITHOUT re-applying playback/SFX volume (the
/// startup-grade `handle_player_settings_loaded` would clobber an in-flight
/// async volume persist with the manager's stale snapshot).
#[test]
fn apply_visualizer_settings_leaves_playback_state_alone() {
    let mut app = test_app();
    app.playback.volume = 0.77;
    app.sfx.volume = 0.55;
    app.settings_page.config_dirty = false;

    let viz = nokkvi_data::types::visualizer_config::VisualizerConfig {
        noise_reduction: 0.42,
        ..Default::default()
    };
    let _ = app.apply_visualizer_settings(viz);

    assert_eq!(
        app.playback.volume, 0.77,
        "visualizer apply must not touch playback volume"
    );
    assert_eq!(
        app.sfx.volume, 0.55,
        "visualizer apply must not touch SFX volume"
    );
    assert_eq!(
        app.visualizer_config.snapshot().noise_reduction,
        0.42,
        "shared render config must receive the change"
    );
    assert_eq!(
        app.settings.visualizer.noise_reduction, 0.42,
        "the live settings mirror must receive the change"
    );
    assert!(
        app.settings_page.config_dirty,
        "settings entries must be marked for refresh"
    );
}
