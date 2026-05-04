//! Surfing-boat overlay handler — see `widgets/boat.rs` for the physics
//! model and pure helpers.
//!
//! Per-frame: derive visibility, then run one physics step that integrates
//! the boat's position from the live bar buffer. Bails cheaply when not in
//! lines mode so the always-on `iced::window::frames()` subscription isn't
//! expensive.

use std::time::Instant;

use iced::Task;
use nokkvi_data::types::player_settings::VisualizationMode;

use crate::{
    Nokkvi,
    app_message::Message,
    widgets::{
        boat::{self, MusicSignals},
        visualizer::visualizer_area_height,
    },
};

/// Handle a per-frame boat tick. Visibility is derived; physics step runs
/// against the live bar buffer. When hidden, position/velocity/phase are
/// preserved so the boat resumes mid-stroke when re-shown.
pub(crate) fn handle_boat_tick(app: &mut Nokkvi, now: Instant) -> Task<Message> {
    // Read mode + config snapshot once per tick. The "visualizer enabled"
    // check is `engine.visualization_mode != VisualizationMode::Off` — that's
    // what gates the shader element in `app_view.rs:319`. There is no
    // separate `cfg.enabled` flag, so the `Lines` discriminator covers both
    // "visualizer on" and "lines mode" in a single check.
    let in_lines_mode = app.engine.visualization_mode == VisualizationMode::Lines;
    let cfg = app.visualizer_config.read();
    let cfg_boat_on = cfg.lines.boat;
    let angular = cfg.lines.style.eq_ignore_ascii_case("angular");
    let height_percent = cfg.height_percent;
    drop(cfg);

    let visible = in_lines_mode && cfg_boat_on;

    if !visible {
        // Drop the dt baseline so the next visible frame doesn't see a stale gap.
        app.boat.visible = false;
        app.boat.last_tick = None;
        return Task::none();
    }

    // Audio pause: the FFT thread's sample buffer drains and the visualizer
    // waveform decays to silence, so integrating the drive oscillator against
    // a flat line walks the boat off the wave with no spring force to pull
    // it back. Hold every dynamic field while paused; clearing `last_tick`
    // gives the first tick after resume a dt=0 baseline (same contract as the
    // hidden branch above). The handle is still primed so the boat keeps
    // rendering at its frozen position.
    if app.playback.paused {
        app.boat.visible = true;
        app.boat.last_tick = None;
        // Keep the current orientation's handle warm while paused so the
        // frozen frame doesn't re-rasterize on resume.
        let tilt = app.boat.tilt;
        let facing = app.boat.facing;
        let _ = app.boat.cache_handle_for(tilt, facing);
        return Task::none();
    }

    let dt = match app.boat.last_tick {
        Some(prev) => now.saturating_duration_since(prev),
        None => std::time::Duration::ZERO,
    };
    app.boat.last_tick = Some(now);

    // Sample the live waveform. When the visualizer hasn't been mounted yet
    // (pre-login, or visualizer disabled mid-flight) `app.visualizer` is
    // None — feed an empty slice so y_ratio settles toward 0.
    let raw_bars = if let Some(viz) = &app.visualizer {
        viz.current_bars()
    } else {
        Vec::new()
    };
    // Silence override: when audio isn't actively producing samples, drop
    // the raw bars to empty so the boat sinks to the bottom rather than
    // tracking the visualizer's frozen-high `display.bars` (the FFT
    // thread's gravity-falloff path only runs when a full sample chunk is
    // available, so the buffer can stay elevated for the entire silence).
    let bars = boat::effective_bars(app.playback.playing, &raw_bars);

    // Size the off-screen wrap margin from the live boat sprite width so the
    // boat clears the visible area before reappearing on the opposite side
    // (`widgets::boat::BOAT_WRAP_MARGIN_BOAT_WIDTHS` boat-widths of pixel
    // travel beyond the edge). The visualizer area height is computed by
    // the same helper `app_view::view()` uses, so the margin tracks any
    // future scaling-curve changes.
    let area_width = app.window.width;
    let area_height = visualizer_area_height(app.window.width, app.window.height, height_percent);
    let (boat_w, _boat_h) = boat::boat_pixel_size(area_height);
    app.boat.x_wrap_margin = if area_width > 0.0 {
        (boat_w * boat::BOAT_WRAP_MARGIN_BOAT_WIDTHS) / area_width
    } else {
        0.0
    };

    // Music signals: tagged BPM (when the current song reports one) +
    // smoothed spectral-flux onset envelope (always). The boat physics
    // composes these on top of the baseline `SAIL_THRUST` so the boat
    // surges on hits and pulses to the beat. Both fall back to "no
    // modulation" when their source isn't available — silence /
    // un-tagged tracks behave like the pre-music constant-thrust model.
    // `bar_energy` is the average of the *effective* bars (the same
    // buffer the boat samples for slope/local height). When playback
    // is paused/stopped, `effective_bars` returns an empty slice and
    // average → 0, so silence correctly drives no presence. The
    // visualizer's auto-sensitivity has already normalized bars into
    // a useful range, so this metric reads "how full the wave looks"
    // and tracks the boat to what the user perceives.
    let bar_energy = if bars.is_empty() {
        0.0
    } else {
        (bars.iter().sum::<f64>() / bars.len() as f64) as f32
    };
    let music = MusicSignals {
        bpm: app.playback.bpm,
        onset_energy: app
            .visualizer
            .as_ref()
            .map_or(0.0, |v| v.current_onset_energy()),
        long_onset_energy: app
            .visualizer
            .as_ref()
            .map_or(0.0, |v| v.current_long_onset_energy()),
        bar_energy,
    };
    boat::step(&mut app.boat, dt, bars, angular, music);
    app.boat.visible = true;
    // Pre-build (and cache) the SVG handle for the current quantized
    // tilt + facing so the immutable `view()` render path can clone it
    // cheaply. The cache is keyed by `(quantized_tilt_index, mirrored)`,
    // so each visibly-distinct orientation pays a one-time resvg cost
    // and every subsequent tick at that same quantized angle is a free
    // hashmap lookup. While anchored, also prime the single themed
    // anchor handle so the doodad's render path is the same cheap
    // lookup (the anchor sprite doesn't rotate — the rope's swing
    // lives in the canvas path, not in the SVG).
    let tilt = app.boat.tilt;
    let facing = app.boat.facing;
    let _ = app.boat.cache_handle_for(tilt, facing);
    if app.boat.anchor_remaining_secs > 0.0 {
        let _ = app.boat.cache_anchor_handle();
    }

    Task::none()
}
