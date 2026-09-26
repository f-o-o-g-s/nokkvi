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
    let preset = compile_preset(
        name.to_string(),
        json,
        &crate::widgets::visualizer::milkdrop::palette::PresetPalette::from_theme(),
    )
    .expect("bundled preset compiles");
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
    let preset = compile_preset(
        name.to_string(),
        json,
        &crate::widgets::visualizer::milkdrop::palette::PresetPalette::from_theme(),
    )
    .expect("bundled preset compiles");
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
    // Let that switch land; a track change never discards a pending build.
    let generation = app.milkdrop.generation;
    built(&mut app, generation, Ok(()));
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
        app.milkdrop
            .shared
            .slot_lost
            .store(app.milkdrop.generation, Ordering::Release);
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

// ----------------------------------------------------------------------------
// Settings
// ----------------------------------------------------------------------------

fn set_milkdrop_config(
    app: &mut Nokkvi,
    f: impl FnOnce(&mut nokkvi_data::types::visualizer_config::MilkdropConfig),
) {
    let mut cfg = app.visualizer_config.read().clone();
    f(&mut cfg.milkdrop);
    use crate::visualizer_config::SharedVisualizerConfigExt;
    app.visualizer_config.apply(cfg);
}

#[test]
fn interval_zero_never_arms() {
    let mut app = md_app();
    set_milkdrop_config(&mut app, |md| md.preset_interval_secs = 0);
    on_screen(&mut app);
    assert_eq!(app.milkdrop.next_switch_at, None);
    tick(&mut app);
    assert_eq!(app.milkdrop.next_switch_at, None);
}

#[test]
fn show_preset_names_off_is_silent() {
    let mut app = md_app();
    set_milkdrop_config(&mut app, |md| md.show_preset_names = false);
    let toasts = app.toast.toasts.len();
    on_screen(&mut app);
    assert_eq!(app.toast.toasts.len(), toasts);
    assert!(
        app.milkdrop.on_screen.is_some(),
        "still tracked, just not toasted"
    );
}

#[test]
fn switch_on_track_change_off_keeps_the_preset() {
    let mut app = md_app();
    set_milkdrop_config(&mut app, |md| md.switch_on_track_change = false);
    on_screen(&mut app);
    let generation = app.milkdrop.generation;
    let mut update = super::playback::make_playback_update();
    update.song_id = Some("song_z".to_string());
    let _ = app.handle_playback_state_updated(update);
    assert_eq!(app.milkdrop.generation, generation);
}

fn crossfade_frames(app: &Nokkvi) -> u32 {
    app.milkdrop.shared.crossfade_frames.load(Ordering::Acquire)
}

#[test]
fn crossfade_setting_reaches_the_renderer_in_frames() {
    let mut app = md_app();
    set_milkdrop_config(&mut app, |md| {
        md.preset_crossfade_secs = 1.5;
        md.preset_interval_secs = 30;
    });
    enter_milkdrop(&mut app);
    assert_eq!(crossfade_frames(&app), 90, "1.5 s at 60 advances a second");

    set_milkdrop_config(&mut app, |md| md.preset_crossfade_secs = 0.0);
    tick(&mut app);
    assert_eq!(crossfade_frames(&app), 0, "0 is a cut");
}

#[test]
fn crossfade_is_clamped_to_half_the_live_interval() {
    let mut app = md_app();
    set_milkdrop_config(&mut app, |md| {
        md.preset_crossfade_secs = 10.0;
        md.preset_interval_secs = 5;
    });
    enter_milkdrop(&mut app);
    assert_eq!(crossfade_frames(&app), 150, "clamped to 2.5 s");

    set_milkdrop_config(&mut app, |md| md.preset_interval_secs = 1);
    tick(&mut app);
    assert_eq!(crossfade_frames(&app), 30, "a 1 s interval allows 0.5 s");

    set_milkdrop_config(&mut app, |md| md.preset_interval_secs = 0);
    tick(&mut app);
    assert_eq!(crossfade_frames(&app), 600, "interval 0 never clamps");
}

#[test]
fn favorites_only_and_quality_reach_the_library_and_renderer() {
    use nokkvi_data::types::visualizer_config::{MilkdropPresetSource, MilkdropRenderQuality};
    let mut app = md_app();
    set_milkdrop_config(&mut app, |md| {
        md.preset_source = MilkdropPresetSource::FavoritesOnly;
        md.render_quality = MilkdropRenderQuality::High;
    });
    let (first, _) = BUNDLED_MILKDROP_PRESETS[0];
    app.milkdrop.library.toggle_favorite(first);
    enter_milkdrop(&mut app);
    assert_eq!(
        app.milkdrop.current.as_deref(),
        Some(first),
        "favorites only"
    );
    assert_eq!(
        app.milkdrop
            .shared
            .quality_short_side
            .load(Ordering::Acquire),
        1080
    );
}

// ----------------------------------------------------------------------------
// Second review's fixes
// ----------------------------------------------------------------------------

#[test]
fn hide_while_paused_loads_another_on_resume() {
    let dir = TestDir::new();
    let mut app = curated_app(&dir);
    on_screen(&mut app);
    let hidden = app.milkdrop.on_screen.clone().expect("a preset on screen");
    app.milkdrop.locked = true;
    app.playback.paused = true;
    tick(&mut app);
    control(&mut app, crate::app_message::MilkdropControl::Hide);
    assert_eq!(
        app.milkdrop.on_screen, None,
        "the hidden preset is no longer the target"
    );
    app.playback.paused = false;
    tick(&mut app);
    assert!(app.milkdrop.current.is_some());
    assert_ne!(app.milkdrop.current.as_deref(), Some(hidden.as_str()));
}

#[test]
fn hiding_the_last_eligible_preset_gives_the_cover_back() {
    let dir = TestDir::new();
    let mut app = curated_app(&dir);
    on_screen(&mut app);
    let shown = app.milkdrop.on_screen.clone().expect("a preset");
    for (name, _) in BUNDLED_MILKDROP_PRESETS {
        if *name != shown {
            app.milkdrop.library.hide(name);
        }
    }
    let generation = app.milkdrop.generation;
    control(&mut app, crate::app_message::MilkdropControl::Hide);
    assert!(released(&app, generation), "the renderer is released");
    assert_eq!(app.milkdrop.on_screen, None);
    let toasts = app.toast.toasts.len();
    let after = app.milkdrop.generation;
    tick(&mut app);
    tick(&mut app);
    assert_eq!(
        app.milkdrop.generation, after,
        "no retry while nothing is eligible"
    );
    control(&mut app, crate::app_message::MilkdropControl::Hide);
    assert_eq!(app.toast.toasts.len(), toasts, "a second Hide does nothing");
}

#[test]
fn a_broken_curation_file_is_never_overwritten() {
    let dir = TestDir::new();
    let mut app = curated_app(&dir);
    std::fs::write(&app.milkdrop.curation_path, "hidden = [unterminated").expect("write");
    app.milkdrop_load_curation();
    on_screen(&mut app);
    control(
        &mut app,
        crate::app_message::MilkdropControl::ToggleFavorite,
    );
    let text = std::fs::read_to_string(&app.milkdrop.curation_path).expect("read");
    assert_eq!(
        text, "hidden = [unterminated",
        "the user's file is left alone"
    );
}

#[test]
fn a_preset_that_fails_is_skipped_for_the_session() {
    let mut app = md_app();
    enter_milkdrop(&mut app);
    let failed = app.milkdrop.current.clone().expect("a preset");
    let generation = app.milkdrop.generation;
    built(&mut app, generation, Err("broken".to_string()));
    assert!(
        !app.milkdrop.library.eligible().contains(&failed.as_str()),
        "a failed preset leaves the rotation"
    );
}

#[test]
fn a_changed_interval_rearms_the_timer() {
    let mut app = md_app();
    on_screen(&mut app);
    assert!(app.milkdrop.next_switch_at.is_some());
    set_milkdrop_config(&mut app, |md| md.preset_interval_secs = 0);
    tick(&mut app);
    assert_eq!(app.milkdrop.next_switch_at, None, "0 now means never");
    set_milkdrop_config(&mut app, |md| md.preset_interval_secs = 10);
    tick(&mut app);
    let at = app.milkdrop.next_switch_at.expect("re-armed");
    assert!(at <= std::time::Instant::now() + std::time::Duration::from_secs(10));
}

#[test]
fn a_lost_renderer_clears_the_preset_on_screen() {
    let mut app = md_app();
    on_screen(&mut app);
    app.milkdrop
        .shared
        .slot_lost
        .store(app.milkdrop.generation, Ordering::Release);
    tick(&mut app);
    assert_eq!(app.milkdrop.on_screen, None);
}

#[test]
fn next_while_paused_says_why() {
    let mut app = md_app();
    on_screen(&mut app);
    app.playback.paused = true;
    tick(&mut app);
    let toasts = app.toast.toasts.len();
    control(&mut app, crate::app_message::MilkdropControl::Next);
    assert_eq!(app.toast.toasts.len(), toasts + 1);
}

// ----------------------------------------------------------------------------
// nokkvi presets: theme colours
// ----------------------------------------------------------------------------

#[test]
fn a_themed_preset_reloads_when_the_palette_changes() {
    let _guard = crate::theme::THEME_MODE_LOCK.lock();
    let mut app = md_app();
    on_screen(&mut app);
    app.milkdrop.current_themed = true;
    tick(&mut app);
    let name = app.milkdrop.current.clone();
    let generation = app.milkdrop.generation;

    // The theme's colours changed since the preset was built.
    let mut before = crate::widgets::visualizer::milkdrop::palette::PresetPalette::from_theme();
    before.bg = [before.bg[0] + 0.1, before.bg[1], before.bg[2]];
    app.milkdrop.palette_used = Some(before);
    app.milkdrop.palette_check_pending = true;
    tick(&mut app);

    assert!(
        app.milkdrop.generation > generation,
        "reloaded in the new colours"
    );
    assert_eq!(app.milkdrop.current, name, "the same preset, recoloured");
    assert!(
        app.milkdrop.history.is_empty(),
        "a recolour is not a new preset"
    );
}

#[test]
fn an_unthemed_preset_ignores_palette_changes() {
    let _guard = crate::theme::THEME_MODE_LOCK.lock();
    let mut app = md_app();
    on_screen(&mut app);
    app.milkdrop.current_themed = false;
    let generation = app.milkdrop.generation;
    let was_light = crate::theme::is_light_mode();
    crate::theme::set_light_mode(!was_light);
    tick(&mut app);
    crate::theme::set_light_mode(was_light);
    assert_eq!(app.milkdrop.generation, generation);
}

#[test]
fn a_theme_bump_without_a_colour_change_reloads_nothing() {
    let _guard = crate::theme::THEME_MODE_LOCK.lock();
    let mut app = md_app();
    on_screen(&mut app);
    app.milkdrop.current_themed = true;
    tick(&mut app);
    let generation = app.milkdrop.generation;
    // A settings reload re-applies the same light mode, bumping the counter.
    crate::theme::set_light_mode(crate::theme::is_light_mode());
    tick(&mut app);
    assert_eq!(app.milkdrop.generation, generation);
}

// ----------------------------------------------------------------------------
// nokkvi presets: the playing cover
// ----------------------------------------------------------------------------

/// A 2x2 PNG, so the handle is byte-backed like a fetched cover.
fn tiny_png() -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(2, 2, image::Rgba([200, 30, 30, 255]))
        .write_to(&mut out, image::ImageFormat::Png)
        .expect("encode png");
    out.into_inner()
}

/// md_app playing song s1 of album_s1 whose large cover is cached.
fn app_with_cover() -> (Nokkvi, iced::advanced::image::Id) {
    let mut app = md_app();
    app.library.queue_songs = vec![make_queue_song("s1", "T", "A", "Al")];
    app.scrobble.current_song_id = Some("s1".to_string());
    let handle = iced::widget::image::Handle::from_bytes(tiny_png());
    let id = handle.id();
    app.artwork
        .large_artwork
        .put("album_s1".to_string(), handle);
    (app, id)
}

#[test]
fn the_playing_cover_is_decoded_for_milkdrop() {
    let (mut app, id) = app_with_cover();
    enter_milkdrop(&mut app);
    assert_eq!(app.milkdrop.cover_pending, Some(id), "decode requested");
    tick(&mut app);
    assert_eq!(
        app.milkdrop.cover_pending,
        Some(id),
        "requested once, not per tick"
    );
}

#[test]
fn a_decoded_cover_reaches_the_renderer_side() {
    let (mut app, id) = app_with_cover();
    enter_milkdrop(&mut app);
    let cover = crate::widgets::visualizer::milkdrop::decode_cover(&tiny_png()).expect("decodes");
    let _ = app.update(Message::Milkdrop(MilkdropMessage::CoverDecoded {
        source: id,
        result: Some(std::sync::Arc::new(cover)),
    }));
    let shared = app.milkdrop.shared.cover.lock().clone().expect("published");
    assert_eq!((shared.width, shared.height), (2, 2));
    assert!(app.milkdrop.shared.cover_version() > 0);
    assert_eq!(app.milkdrop.cover_pending, None);
    tick(&mut app);
    assert_eq!(app.milkdrop.cover_pending, None, "not decoded again");
}

#[test]
fn a_stale_cover_is_dropped() {
    let (mut app, _id) = app_with_cover();
    enter_milkdrop(&mut app);
    let other = iced::widget::image::Handle::from_bytes(tiny_png()).id();
    let cover = crate::widgets::visualizer::milkdrop::decode_cover(&tiny_png()).expect("decodes");
    let _ = app.update(Message::Milkdrop(MilkdropMessage::CoverDecoded {
        source: other,
        result: Some(std::sync::Arc::new(cover)),
    }));
    assert!(app.milkdrop.shared.cover.lock().is_none());
}

#[test]
fn no_cover_work_outside_milkdrop_mode() {
    let (mut app, _id) = app_with_cover();
    app.engine.visualization_mode = VisualizationMode::Bars;
    tick(&mut app);
    assert_eq!(app.milkdrop.cover_pending, None);
}

// ----------------------------------------------------------------------------
// Review fixes for nokkvi presets
// ----------------------------------------------------------------------------

#[test]
fn presets_use_the_themes_dark_colours_even_in_light_mode() {
    use crate::widgets::visualizer::milkdrop::palette::PresetPalette;
    let _guard = crate::theme::THEME_MODE_LOCK.lock();
    let was_light = crate::theme::is_light_mode();
    crate::theme::set_light_mode(true);
    let p = PresetPalette::from_theme();
    let dark_bg = crate::theme::read_dark_color(|t| t.bg0_hard);
    crate::theme::set_light_mode(was_light);
    assert_eq!(
        p.bg,
        [dark_bg.r, dark_bg.g, dark_bg.b],
        "glow presets need a dark canvas"
    );
    assert!(p.light, "presets can still tell light mode apart");
}

#[test]
fn a_new_album_without_art_gets_a_neutral_cover_not_the_last_one() {
    let (mut app, id) = app_with_cover();
    enter_milkdrop(&mut app);
    let cover = crate::widgets::visualizer::milkdrop::decode_cover(&tiny_png()).expect("decodes");
    let _ = app.update(Message::Milkdrop(MilkdropMessage::CoverDecoded {
        source: id,
        result: Some(std::sync::Arc::new(cover)),
    }));
    let version = app.milkdrop.shared.cover_version();
    // Next track: another album, nothing cached for it.
    let mut other = make_queue_song("s2", "T2", "B", "Bl");
    other.album_id = "album_nothing".to_string();
    app.library.queue_songs.push(other);
    app.scrobble.current_song_id = Some("s2".to_string());
    tick(&mut app);
    assert!(
        app.milkdrop.shared.cover_version() > version,
        "the old cover is replaced"
    );
    let shared = app
        .milkdrop
        .shared
        .cover
        .lock()
        .clone()
        .expect("a neutral cover");
    assert_eq!((shared.width, shared.height), (1, 1));
    assert_eq!(
        app.milkdrop.cover_large_requested.as_deref(),
        Some("album_nothing"),
        "and its large cover is asked for"
    );
}

#[test]
fn an_evicted_cover_for_the_same_album_is_kept() {
    let (mut app, id) = app_with_cover();
    enter_milkdrop(&mut app);
    let cover = crate::widgets::visualizer::milkdrop::decode_cover(&tiny_png()).expect("decodes");
    let _ = app.update(Message::Milkdrop(MilkdropMessage::CoverDecoded {
        source: id,
        result: Some(std::sync::Arc::new(cover)),
    }));
    let version = app.milkdrop.shared.cover_version();
    app.artwork.large_artwork = crate::state::ArtworkState::default().large_artwork;
    tick(&mut app);
    assert_eq!(
        app.milkdrop.shared.cover_version(),
        version,
        "same album: keep it"
    );
}

#[test]
fn logout_clears_the_cover() {
    let (mut app, id) = app_with_cover();
    enter_milkdrop(&mut app);
    let cover = crate::widgets::visualizer::milkdrop::decode_cover(&tiny_png()).expect("decodes");
    let _ = app.update(Message::Milkdrop(MilkdropMessage::CoverDecoded {
        source: id,
        result: Some(std::sync::Arc::new(cover)),
    }));
    let _lock = super::SSE_SLOT_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let _ = app.reset_session_state();
    assert!(app.milkdrop.shared.cover.lock().is_none());
    assert_eq!(app.milkdrop.cover_sent, None);
}

#[test]
fn a_recolour_is_not_a_new_preset() {
    let _guard = crate::theme::THEME_MODE_LOCK.lock();
    let mut app = md_app();
    on_screen(&mut app);
    app.milkdrop.current_themed = true;
    let armed = app.milkdrop.next_switch_at;
    tick(&mut app);
    let toasts = app.toast.toasts.len();

    let was_light = crate::theme::is_light_mode();
    crate::theme::set_light_mode(!was_light);
    tick(&mut app);
    crate::theme::set_light_mode(was_light);
    let generation = app.milkdrop.generation;
    // Next while the recolour builds still records the preset on screen.
    let shown = app.milkdrop.on_screen.clone();
    built(&mut app, generation, Ok(()));
    assert_eq!(
        app.milkdrop.next_switch_at, armed,
        "the switch timer is kept"
    );
    first_frame(&mut app);
    assert_eq!(app.toast.toasts.len(), toasts, "no second name toast");
    assert_eq!(app.milkdrop.on_screen, shown);
}

#[test]
fn non_square_covers_are_padded_not_stretched() {
    let cover = crate::widgets::visualizer::milkdrop::cover_from_rgba(4, 2, vec![255; 4 * 2 * 4])
        .expect("pixels");
    assert_eq!((cover.width, cover.height), (4, 4));
}

// ----------------------------------------------------------------------------
// Unreleased-review fixes (2026-09-25)
// ----------------------------------------------------------------------------

/// On screen with `favorites_only` and the preset on screen as the only
/// favorite: the library can only ever draw that same preset again.
fn single_eligible_app() -> Nokkvi {
    use nokkvi_data::types::visualizer_config::MilkdropPresetSource;
    let mut app = md_app();
    on_screen(&mut app);
    let only = app.milkdrop.current.clone().expect("a preset");
    app.milkdrop.library.toggle_favorite(&only);
    set_milkdrop_config(&mut app, |md| {
        md.preset_source = MilkdropPresetSource::FavoritesOnly;
    });
    tick(&mut app);
    app
}

#[test]
fn a_single_eligible_preset_is_not_reloaded_by_the_timer() {
    let mut app = single_eligible_app();
    let generation = app.milkdrop.generation;
    let toasts = app.toast.toasts.len();
    app.milkdrop.next_switch_at = Some(past());
    tick(&mut app);
    tick(&mut app);
    assert_eq!(app.milkdrop.generation, generation, "no rebuild from black");
    assert_eq!(app.toast.toasts.len(), toasts, "no repeated name toast");
    assert!(
        app.milkdrop.next_switch_at.is_some(),
        "the timer re-arms and waits"
    );
}

#[test]
fn a_single_eligible_preset_is_not_reloaded_by_a_track_change() {
    let mut app = single_eligible_app();
    let generation = app.milkdrop.generation;
    let mut update = super::playback::make_playback_update();
    update.song_id = Some("song_single".to_string());
    let _ = app.handle_playback_state_updated(update);
    assert_eq!(app.milkdrop.generation, generation);
}

#[test]
fn a_track_change_during_a_build_keeps_that_build() {
    let mut app = md_app();
    on_screen(&mut app);
    press_plain(&mut app, "n");
    let building = app.milkdrop.generation;
    assert_eq!(app.milkdrop.build_in_flight, Some(building));

    let mut update = super::playback::make_playback_update();
    update.song_id = Some("song_mid_build".to_string());
    let _ = app.handle_playback_state_updated(update);
    assert_eq!(
        app.milkdrop.generation, building,
        "the pending build is not thrown away"
    );
}

#[test]
fn losing_the_old_renderer_does_not_blame_the_new_build() {
    let mut app = md_app();
    on_screen(&mut app);
    let old_generation = app.milkdrop.generation;
    let old = app.milkdrop.current.clone().expect("a preset");

    press_plain(&mut app, "n");
    let new = app.milkdrop.current.clone().expect("a new preset");
    let new_generation = app.milkdrop.generation;
    built(&mut app, new_generation, Ok(()));

    // `prepare` lost the OLD renderer, still in the slot, before the swap.
    app.milkdrop
        .shared
        .slot_lost
        .store(old_generation, Ordering::Release);
    tick(&mut app);
    assert_eq!(
        app.milkdrop.current.as_deref(),
        Some(new.as_str()),
        "the new preset stays current"
    );
    assert!(
        app.milkdrop.library.eligible().contains(&new.as_str()),
        "the new preset is not marked broken"
    );
    assert!(
        !app.milkdrop.library.eligible().contains(&old.as_str()),
        "the old one is"
    );
    assert_eq!(app.milkdrop.consecutive_failures, 1);
}

#[test]
fn a_hidden_preset_never_returns_to_history() {
    let dir = TestDir::new();
    let mut app = curated_app(&dir);
    on_screen(&mut app);
    let shown = app.milkdrop.on_screen.clone().expect("a preset on screen");
    control(&mut app, crate::app_message::MilkdropControl::Hide);
    assert!(
        !app.milkdrop.history.contains(&shown),
        "Previous cannot walk back to it"
    );
}

#[test]
fn a_light_dark_toggle_leaves_a_preset_that_ignores_it() {
    let _guard = crate::theme::THEME_MODE_LOCK.lock();
    let mut app = md_app();
    on_screen(&mut app);
    app.milkdrop.current_themed = true;
    app.milkdrop.current_uses_light = false;
    tick(&mut app);
    let generation = app.milkdrop.generation;

    let was_light = crate::theme::is_light_mode();
    crate::theme::set_light_mode(!was_light);
    tick(&mut app);
    crate::theme::set_light_mode(was_light);
    assert_eq!(
        app.milkdrop.generation, generation,
        "same dark colours: nothing to recolour"
    );
}

#[test]
fn a_light_dark_toggle_reloads_a_preset_that_reads_it() {
    let _guard = crate::theme::THEME_MODE_LOCK.lock();
    let mut app = md_app();
    on_screen(&mut app);
    app.milkdrop.current_themed = true;
    app.milkdrop.current_uses_light = true;
    tick(&mut app);
    let generation = app.milkdrop.generation;

    let was_light = crate::theme::is_light_mode();
    crate::theme::set_light_mode(!was_light);
    tick(&mut app);
    crate::theme::set_light_mode(was_light);
    assert!(app.milkdrop.generation > generation);
}
