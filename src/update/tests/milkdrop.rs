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
    app
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
