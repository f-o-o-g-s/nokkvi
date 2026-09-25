//! MilkDrop mode: the load pipeline's bookkeeping (generations, failures,
//! release) and the mode edges. The GPU half (a real `wgpu::Device`, the swap
//! into `prepare`, the blit) cannot run here; the review pass and the owner's
//! run cover it.

use std::{path::Path, sync::atomic::Ordering};

use nokkvi_data::{
    services::milkdrop_presets::{Curation, PresetLibrary},
    types::player_settings::VisualizationMode,
};

use crate::{
    Nokkvi, Screen, View,
    app_message::{Message, MilkdropMessage, PlaybackMessage},
    test_helpers::*,
    widgets::visualizer::milkdrop::{BUNDLED_MILKDROP_PRESETS, compile_preset},
};

/// A logged-in, playing app on the Queue with a real visualizer sharing the
/// app's MilkDrop handle and the bundled pack (no user dir on disk).
fn md_app() -> Nokkvi {
    let mut app = test_app();
    app.screen = Screen::Home;
    app.current_view = View::Queue;
    app.visualizer = Some(crate::widgets::visualizer::Visualizer::new(
        192,
        app.visualizer_config.clone(),
        app.milkdrop.shared.clone(),
    ));
    app.milkdrop.library = PresetLibrary::new(
        BUNDLED_MILKDROP_PRESETS,
        Path::new("/nonexistent/nokkvi-milkdrop-tests"),
        Curation::default(),
    );
    app.playback.playing = true;
    app.playback.paused = false;
    app.main_window_id = Some(iced::window::Id::unique());
    // Stand-in for the panel's `prepare` having run this frame.
    app.milkdrop.shared.mark_mounted();
    app
}

/// Stand-in for the pipeline rendering the current load's first frame.
fn first_frame(app: &mut Nokkvi) {
    let generation = app.milkdrop.generation;
    app.milkdrop.shared.mark_shown(generation);
    tick(app);
}

/// Whether a renderer from load `generation` is below the release watermark.
fn released(app: &Nokkvi, generation: u64) -> bool {
    generation < app.milkdrop.shared.released_below.load(Ordering::Acquire)
}

fn cycle(app: &mut Nokkvi) {
    let _ = app.update(Message::Playback(PlaybackMessage::CycleVisualization));
}

fn tick(app: &mut Nokkvi) {
    let _ = app.update(Message::Playback(PlaybackMessage::Tick));
}

/// Cycle from Scope into MilkDrop and run one tick, which picks a preset.
fn enter_milkdrop(app: &mut Nokkvi) {
    app.engine.visualization_mode = VisualizationMode::Scope;
    cycle(app);
    assert_eq!(app.engine.visualization_mode, VisualizationMode::Milkdrop);
    tick(app);
}

fn built(app: &mut Nokkvi, generation: u64, result: Result<(), String>) {
    let _ = app.update(Message::Milkdrop(MilkdropMessage::Built {
        generation,
        result,
    }));
}

#[test]
fn cycle_reaches_milkdrop_after_scope_and_wraps_to_off() {
    let mut app = md_app();
    app.engine.visualization_mode = VisualizationMode::Scope;
    cycle(&mut app);
    assert_eq!(app.engine.visualization_mode, VisualizationMode::Milkdrop);
    cycle(&mut app);
    assert_eq!(app.engine.visualization_mode, VisualizationMode::Off);
}

#[test]
fn entering_milkdrop_picks_a_preset() {
    let mut app = md_app();
    enter_milkdrop(&mut app);
    assert!(app.milkdrop.current.is_some());
    assert_eq!(app.milkdrop.generation, 1);
    assert_eq!(app.milkdrop.build_in_flight, Some(1));
    assert_eq!(
        app.milkdrop
            .shared
            .current_generation
            .load(Ordering::Acquire),
        1
    );
    assert!(app.milkdrop.shared.running.load(Ordering::Acquire));
}

#[test]
fn stale_compiled_is_dropped() {
    let mut app = md_app();
    enter_milkdrop(&mut app);
    // Force a second load so generation 1 is stale.
    app.milkdrop.current = None;
    app.milkdrop.build_in_flight = None;
    tick(&mut app);
    assert_eq!(app.milkdrop.generation, 2);

    let (name, json) = BUNDLED_MILKDROP_PRESETS[0];
    let preset = compile_preset(name.to_string(), json).expect("bundled preset compiles");
    let toasts = app.toast.toasts.len();
    let _ = app.update(Message::Milkdrop(MilkdropMessage::Compiled {
        generation: 1,
        result: Ok(std::sync::Arc::new(preset)),
    }));
    assert!(app.milkdrop.awaiting_gpu.is_none());
    assert_eq!(app.milkdrop.build_in_flight, Some(2));
    assert_eq!(app.toast.toasts.len(), toasts);
}

/// The drain side (the tick handing the compiled preset to a build task once
/// `prepare` has captured the device) needs a real `wgpu::Device`.
#[test]
fn compiled_before_gpu_waits() {
    let mut app = md_app();
    enter_milkdrop(&mut app);
    let generation = app.milkdrop.generation;
    let (name, json) = BUNDLED_MILKDROP_PRESETS[0];
    let preset = compile_preset(name.to_string(), json).expect("bundled preset compiles");
    let _ = app.update(Message::Milkdrop(MilkdropMessage::Compiled {
        generation,
        result: Ok(std::sync::Arc::new(preset)),
    }));
    assert!(app.milkdrop.awaiting_gpu.is_some());
    assert_eq!(app.milkdrop.build_in_flight, Some(generation));
}

#[test]
fn five_failures_stop_with_one_warning() {
    let mut app = md_app();
    enter_milkdrop(&mut app);
    let toasts = app.toast.toasts.len();
    for _ in 0..5 {
        let generation = app.milkdrop.generation;
        built(&mut app, generation, Err("boom".to_string()));
    }
    assert_eq!(app.toast.toasts.len(), toasts + 1, "one warning after five");
    assert_eq!(app.milkdrop.build_in_flight, None);

    let generation = app.milkdrop.generation;
    built(&mut app, generation, Err("boom".to_string()));
    assert_eq!(app.toast.toasts.len(), toasts + 1, "a sixth adds no toast");

    tick(&mut app);
    assert_eq!(app.milkdrop.current, None);
    assert_eq!(app.milkdrop.build_in_flight, None);
}

#[test]
fn release_clears_current_and_bumps_generation() {
    let mut app = md_app();
    enter_milkdrop(&mut app);
    let generation = app.milkdrop.generation;
    cycle(&mut app); // MilkDrop → Off
    assert!(
        released(&app, generation),
        "the loaded renderer is released"
    );
    assert!(!app.milkdrop.shared.running.load(Ordering::Acquire));
    assert_eq!(app.milkdrop.current, None);
    assert!(app.milkdrop.generation > generation);
    assert_eq!(
        app.milkdrop
            .shared
            .current_generation
            .load(Ordering::Acquire),
        app.milkdrop.generation
    );
}

#[test]
fn built_after_leaving_the_mode_is_silent() {
    let mut app = md_app();
    enter_milkdrop(&mut app);
    let generation = app.milkdrop.generation;
    app.engine.visualization_mode = VisualizationMode::Lines;
    cycle(&mut app); // Lines → Scope; mode is no longer MilkDrop either way
    app.engine.visualization_mode = VisualizationMode::Bars;
    let toasts = app.toast.toasts.len();
    built(&mut app, generation, Ok(()));
    assert_eq!(app.toast.toasts.len(), toasts);
    assert_eq!(app.milkdrop.next_switch_at, None);
}

#[test]
fn tick_reloads_when_running_without_a_preset() {
    let mut app = md_app();
    enter_milkdrop(&mut app);
    app.milkdrop_release();
    assert_eq!(app.engine.visualization_mode, VisualizationMode::Milkdrop);
    tick(&mut app);
    assert!(app.milkdrop.current.is_some());
    assert!(app.milkdrop.build_in_flight.is_some());
}

#[test]
fn stop_releases() {
    let mut app = md_app();
    enter_milkdrop(&mut app);
    let generation = app.milkdrop.generation;
    let _ = app.update(Message::Playback(PlaybackMessage::Stop));
    assert!(released(&app, generation));
    assert_eq!(app.milkdrop.current, None);
}

#[test]
fn natural_end_of_playback_releases() {
    let mut app = md_app();
    enter_milkdrop(&mut app);
    let generation = app.milkdrop.generation;
    let mut update = super::playback::make_playback_update();
    update.playing = false;
    update.paused = false;
    let _ = app.handle_playback_state_updated(update);
    assert!(released(&app, generation));
}

#[test]
fn paused_does_not_load_or_run() {
    let mut app = md_app();
    app.playback.paused = true;
    enter_milkdrop(&mut app);
    assert!(!app.milkdrop.shared.running.load(Ordering::Acquire));
    assert_eq!(app.milkdrop.current, None, "nothing builds while paused");
}

#[test]
fn off_the_panel_nothing_loads() {
    let mut app = md_app();
    app.current_view = View::Albums;
    enter_milkdrop(&mut app);
    assert!(!app.milkdrop.shared.running.load(Ordering::Acquire));
    assert_eq!(app.milkdrop.current, None);
}

#[test]
fn settings_reload_fires_no_spurious_edge() {
    let mut app = md_app();
    enter_milkdrop(&mut app);
    let generation = app.milkdrop.generation;
    let current = app.milkdrop.current.clone();
    // A hot reload re-delivers the same mode: no release, no reload.
    app.milkdrop_mode_edge(VisualizationMode::Milkdrop, VisualizationMode::Milkdrop);
    assert_eq!(app.milkdrop.generation, generation);
    assert_eq!(app.milkdrop.current, current);
    assert!(!released(&app, generation));
}

#[test]
fn a_release_never_covers_a_later_load() {
    let mut app = md_app();
    enter_milkdrop(&mut app);
    let first = app.milkdrop.generation;
    cycle(&mut app); // MilkDrop → Off: releases `first`
    app.engine.visualization_mode = VisualizationMode::Scope;
    cycle(&mut app); // back into MilkDrop
    tick(&mut app); // loads again
    let second = app.milkdrop.generation;
    assert!(second > first);
    assert!(released(&app, first));
    assert!(
        !released(&app, second),
        "a release no frame has enforced yet must not drop the new load"
    );
}

// ----------------------------------------------------------------------------
// Switching: keys, timer, track change
// ----------------------------------------------------------------------------

fn press(app: &mut Nokkvi, c: &str, modifiers: iced::keyboard::Modifiers) {
    let _ = app.handle_raw_key_event(
        iced::keyboard::Key::Character(c.into()),
        modifiers,
        iced::event::Status::Ignored,
    );
}

fn press_plain(app: &mut Nokkvi, c: &str) {
    press(app, c, iced::keyboard::Modifiers::default());
}

/// Enter MilkDrop and land the first build, which arms the timer.
fn on_screen(app: &mut Nokkvi) {
    enter_milkdrop(app);
    let generation = app.milkdrop.generation;
    built(app, generation, Ok(()));
    assert!(app.milkdrop.build_in_flight.is_none());
    first_frame(app);
}

fn past() -> std::time::Instant {
    std::time::Instant::now()
        .checked_sub(std::time::Duration::from_secs(1))
        .expect("monotonic clock is past boot")
}

#[test]
fn next_key_advances_and_previous_returns() {
    let mut app = md_app();
    on_screen(&mut app);
    let first = app.milkdrop.current.clone().expect("a preset");

    press_plain(&mut app, "n");
    let second = app.milkdrop.current.clone().expect("a preset");
    assert_ne!(second, first);
    assert_eq!(app.milkdrop.history, std::slice::from_ref(&first));

    press_plain(&mut app, "p");
    assert_eq!(app.milkdrop.current.as_deref(), Some(first.as_str()));
    assert!(app.milkdrop.history.is_empty());
}

#[test]
fn preset_keys_are_inert_outside_milkdrop_mode() {
    let mut app = md_app();
    app.engine.visualization_mode = VisualizationMode::Bars;
    let toasts = app.toast.toasts.len();
    press_plain(&mut app, "n");
    press_plain(&mut app, "p");
    press(&mut app, "m", iced::keyboard::Modifiers::SHIFT);
    assert_eq!(app.milkdrop.current, None);
    assert_eq!(app.milkdrop.generation, 0);
    assert!(!app.milkdrop.locked);
    assert_eq!(app.toast.toasts.len(), toasts);
}

#[test]
fn lock_clears_the_timer_and_unlock_rearms() {
    let mut app = md_app();
    on_screen(&mut app);
    assert!(
        app.milkdrop.next_switch_at.is_some(),
        "the first build arms the timer"
    );

    press(&mut app, "m", iced::keyboard::Modifiers::SHIFT);
    assert!(app.milkdrop.locked);
    assert_eq!(app.milkdrop.next_switch_at, None);
    tick(&mut app);
    assert_eq!(
        app.milkdrop.next_switch_at, None,
        "a locked preset never arms"
    );

    press(&mut app, "m", iced::keyboard::Modifiers::SHIFT);
    assert!(!app.milkdrop.locked);
    assert!(app.milkdrop.next_switch_at.is_some());
}

#[test]
fn timer_advances_only_while_running() {
    let mut app = md_app();
    on_screen(&mut app);
    let first = app.milkdrop.current.clone();

    app.playback.paused = true;
    app.milkdrop.next_switch_at = Some(past());
    tick(&mut app);
    assert_eq!(app.milkdrop.current, first, "paused: no switch");

    app.playback.paused = false;
    app.milkdrop.next_switch_at = Some(past());
    tick(&mut app);
    let second = app.milkdrop.current.clone();
    assert_ne!(second, first, "playing: one switch");
    assert!(app.milkdrop.build_in_flight.is_some());
    assert_eq!(
        app.milkdrop.next_switch_at, None,
        "disarmed until the build lands"
    );

    let generation = app.milkdrop.generation;
    tick(&mut app);
    assert_eq!(
        app.milkdrop.current, second,
        "no re-fire while the build is pending"
    );
    assert_eq!(app.milkdrop.generation, generation);

    built(&mut app, generation, Ok(()));
    assert!(
        app.milkdrop.next_switch_at.is_some(),
        "the landed build re-arms"
    );
}

#[test]
fn timer_is_idle_off_the_panel() {
    let mut app = md_app();
    on_screen(&mut app);
    let first = app.milkdrop.current.clone();
    app.current_view = View::Albums;
    app.milkdrop.next_switch_at = Some(past());
    let toasts = app.toast.toasts.len();
    tick(&mut app);
    assert_eq!(app.milkdrop.current, first);
    assert_eq!(app.toast.toasts.len(), toasts);
}

#[test]
fn track_change_switches_presets() {
    let mut app = md_app();
    on_screen(&mut app);
    let start = app.milkdrop.generation;

    let mut update = super::playback::make_playback_update();
    update.song_id = Some("song_a".to_string());
    let _ = app.handle_playback_state_updated(update.clone());
    update.song_id = Some("song_b".to_string());
    let _ = app.handle_playback_state_updated(update.clone());
    assert_eq!(
        app.milkdrop.generation,
        start + 2,
        "one switch per new track"
    );

    let _ = app.handle_playback_state_updated(update);
    assert_eq!(
        app.milkdrop.generation,
        start + 2,
        "the same track switches nothing"
    );
}

#[test]
fn stopped_clears_the_timer() {
    let mut app = md_app();
    on_screen(&mut app);
    assert!(app.milkdrop.next_switch_at.is_some());
    let _ = app.update(Message::Playback(PlaybackMessage::Stop));
    assert_eq!(app.milkdrop.next_switch_at, None);
}

#[test]
fn theater_policy_passes_preset_keys() {
    use nokkvi_data::types::hotkey_config::HotkeyAction as A;

    use crate::update::theater::{TheaterKeyPolicy, theater_key_policy};
    for action in [
        A::NextVisualizerPreset,
        A::PreviousVisualizerPreset,
        A::ToggleVisualizerPresetLock,
    ] {
        assert_eq!(theater_key_policy(action), TheaterKeyPolicy::Passthrough);
    }
}

// ----------------------------------------------------------------------------
// Review fixes: only work (and toast) for a preset someone can see
// ----------------------------------------------------------------------------

#[test]
fn hidden_window_stops_running_and_releases() {
    let mut app = md_app();
    on_screen(&mut app);
    let generation = app.milkdrop.generation;
    app.milkdrop_on_window_closed();
    app.main_window_id = None;
    tick(&mut app);
    assert!(!app.milkdrop.shared.running.load(Ordering::Acquire));
    assert!(released(&app, generation));
    assert!(
        app.milkdrop.shared.gpu_handles().is_none(),
        "the old device is let go"
    );
    assert_eq!(app.milkdrop.current, None, "nothing loads while hidden");
}

#[test]
fn an_unmounted_panel_is_not_running() {
    let mut app = md_app();
    app.milkdrop.shared.forget_mounted();
    enter_milkdrop(&mut app);
    assert!(!app.milkdrop.shared.running.load(Ordering::Acquire));
    assert_eq!(
        app.milkdrop.current, None,
        "no panel drew MilkDrop: nothing builds"
    );
}

#[test]
fn the_failure_cap_never_rewarns_on_the_timer() {
    let mut app = md_app();
    enter_milkdrop(&mut app);
    for _ in 0..5 {
        let generation = app.milkdrop.generation;
        built(&mut app, generation, Err("boom".to_string()));
    }
    let toasts = app.toast.toasts.len();
    for _ in 0..3 {
        tick(&mut app);
        if let Some(at) = app.milkdrop.next_switch_at.as_mut() {
            *at = past();
        }
        tick(&mut app);
    }
    assert_eq!(app.toast.toasts.len(), toasts);
    assert_eq!(
        app.milkdrop.next_switch_at, None,
        "no timer without a preset"
    );
}

#[test]
fn the_name_toasts_on_the_first_frame_not_on_build() {
    let mut app = md_app();
    enter_milkdrop(&mut app);
    let generation = app.milkdrop.generation;
    let toasts = app.toast.toasts.len();
    built(&mut app, generation, Ok(()));
    assert_eq!(app.toast.toasts.len(), toasts, "built but not yet drawn");
    first_frame(&mut app);
    assert_eq!(app.toast.toasts.len(), toasts + 1);
    tick(&mut app);
    assert_eq!(app.toast.toasts.len(), toasts + 1, "announced once");
}

#[test]
fn a_lost_renderer_counts_as_a_failure() {
    let mut app = md_app();
    enter_milkdrop(&mut app);
    for _ in 0..5 {
        let generation = app.milkdrop.generation;
        built(&mut app, generation, Ok(()));
        app.milkdrop.shared.slot_lost.store(true, Ordering::Release);
        tick(&mut app);
    }
    assert_eq!(
        app.milkdrop.current, None,
        "five lost renderers stop the loop"
    );
    let generation = app.milkdrop.generation;
    tick(&mut app);
    assert_eq!(app.milkdrop.generation, generation);
}

#[test]
fn preset_keys_wait_for_a_running_panel() {
    let mut app = md_app();
    on_screen(&mut app);
    let generation = app.milkdrop.generation;
    app.playback.paused = true;
    tick(&mut app);
    press_plain(&mut app, "n");
    press_plain(&mut app, "p");
    assert_eq!(
        app.milkdrop.generation, generation,
        "paused: nothing builds"
    );
}

#[test]
fn history_only_records_presets_that_were_shown() {
    let mut app = md_app();
    on_screen(&mut app);
    let shown = app.milkdrop.current.clone().expect("a preset");
    press_plain(&mut app, "n"); // B requested, never drawn
    press_plain(&mut app, "n"); // C requested
    assert_eq!(app.milkdrop.history, std::slice::from_ref(&shown));
}

#[test]
fn off_the_panel_a_failure_does_not_retry() {
    let mut app = md_app();
    enter_milkdrop(&mut app);
    let generation = app.milkdrop.generation;
    app.current_view = View::Albums;
    tick(&mut app);
    built(&mut app, generation, Err("boom".to_string()));
    assert_eq!(
        app.milkdrop.generation, generation,
        "no retry off the panel"
    );
}

// ----------------------------------------------------------------------------
// Curation from the panel menu
// ----------------------------------------------------------------------------

/// A unique scratch directory under the system temp dir, removed on drop.
struct TestDir(std::path::PathBuf);

impl TestDir {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("nokkvi-milkdrop-test-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&path).expect("create test dir");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn control(app: &mut Nokkvi, c: crate::app_message::MilkdropControl) {
    let _ = app.update(Message::Milkdrop(MilkdropMessage::Control(c)));
}

/// `md_app` with the curation file in a temp dir (never the real one).
fn curated_app(dir: &TestDir) -> Nokkvi {
    let mut app = md_app();
    app.milkdrop.user_dir = dir.path().to_path_buf();
    app.milkdrop.curation_path = dir.path().join("curation.toml");
    app
}

#[test]
fn hide_persists_and_advances() {
    let dir = TestDir::new();
    let mut app = curated_app(&dir);
    on_screen(&mut app);
    let shown = app.milkdrop.on_screen.clone().expect("a preset on screen");

    control(&mut app, crate::app_message::MilkdropControl::Hide);
    assert!(app.milkdrop.library.is_hidden(&shown));
    assert_ne!(
        app.milkdrop.current.as_deref(),
        Some(shown.as_str()),
        "moved on"
    );
    let saved =
        nokkvi_data::services::milkdrop_presets::Curation::load(&app.milkdrop.curation_path);
    assert!(saved.hidden.contains(&shown), "written to curation.toml");
}

#[test]
fn favorite_toggles_and_persists() {
    let dir = TestDir::new();
    let mut app = curated_app(&dir);
    on_screen(&mut app);
    let shown = app.milkdrop.on_screen.clone().expect("a preset on screen");

    control(
        &mut app,
        crate::app_message::MilkdropControl::ToggleFavorite,
    );
    assert!(app.milkdrop.library.is_favorite(&shown));
    let load = |app: &Nokkvi| {
        nokkvi_data::services::milkdrop_presets::Curation::load(&app.milkdrop.curation_path)
    };
    assert!(load(&app).favorites.contains(&shown));

    control(
        &mut app,
        crate::app_message::MilkdropControl::ToggleFavorite,
    );
    assert!(!app.milkdrop.library.is_favorite(&shown));
    assert!(!load(&app).favorites.contains(&shown));
}

#[test]
fn curation_controls_are_inert_outside_milkdrop_mode() {
    let dir = TestDir::new();
    let mut app = curated_app(&dir);
    on_screen(&mut app);
    let shown = app.milkdrop.on_screen.clone().expect("a preset on screen");
    app.engine.visualization_mode = VisualizationMode::Bars;
    control(&mut app, crate::app_message::MilkdropControl::Hide);
    control(
        &mut app,
        crate::app_message::MilkdropControl::ToggleFavorite,
    );
    assert!(!app.milkdrop.library.is_hidden(&shown));
    assert!(!app.milkdrop.library.is_favorite(&shown));
    assert!(!app.milkdrop.curation_path.exists());
}

#[test]
fn refresh_rescans_the_user_dir_in_milkdrop_mode() {
    let dir = TestDir::new();
    let mut app = curated_app(&dir);
    app.milkdrop.library =
        PresetLibrary::new(BUNDLED_MILKDROP_PRESETS, dir.path(), Curation::default());
    enter_milkdrop(&mut app);
    let before = app.milkdrop.library.len();
    std::fs::write(dir.path().join("zz my own preset.json"), "{}").expect("write");
    let _ = app.update(Message::Hotkey(
        crate::app_message::HotkeyMessage::RefreshView,
    ));
    assert_eq!(app.milkdrop.library.len(), before + 1);
}

#[test]
fn queue_panel_menu_rows_reach_the_same_handlers() {
    let dir = TestDir::new();
    let mut app = curated_app(&dir);
    on_screen(&mut app);
    let _ = app.update(Message::Queue(crate::views::QueueMessage::Milkdrop(
        crate::app_message::MilkdropControl::ToggleLock,
    )));
    assert!(app.milkdrop.locked);
}

#[test]
fn milkdrop_menu_rows_carry_the_shipped_icons() {
    use crate::{app_message::MilkdropControl as C, widgets::context_menu::milkdrop_panel_entries};
    let rows = milkdrop_panel_entries(false, false, |c| c);
    let pairs: Vec<(&str, &str)> = rows.iter().map(|r| (r.icon, r.label)).collect();
    assert_eq!(
        pairs,
        [
            ("assets/icons/skip-forward.svg", "Next Preset"),
            ("assets/icons/skip-back.svg", "Previous Preset"),
            ("assets/icons/lock.svg", "Lock Preset"),
            ("assets/icons/heart.svg", "Favorite Preset"),
            ("assets/icons/eye-off.svg", "Never Show This Preset"),
        ]
    );
    assert!(matches!(rows[4].message, C::Hide));
    let rows = milkdrop_panel_entries(true, true, |c| c);
    assert_eq!(rows[2].label, "Unlock Preset");
    assert_eq!(rows[2].icon, "assets/icons/lock-open.svg");
    assert_eq!(rows[3].label, "Unfavorite Preset");
}
