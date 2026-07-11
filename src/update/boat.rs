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
    visualizer_config::{LinesStyle, VisualizerPlacement},
    widgets::{
        boat::{self, MusicSignals},
        visualizer::visualizer_area_height,
    },
};

/// Handle a per-frame boat tick. Visibility is derived; physics step runs
/// against the live bar buffer. When hidden, position/velocity/phase are
/// preserved so the boat resumes mid-stroke when re-shown.
pub(crate) fn handle_boat_tick(app: &mut Nokkvi, now: Instant) -> Task<Message> {
    // Drive the now-playing breathing glow off this per-frame frame tick so it
    // stays smooth at any display refresh rate (a fixed-interval timer steps
    // visibly on high-Hz displays). Runs BEFORE the boat's early-outs so the
    // glow animates regardless of visualizer mode; frozen while paused/stopped.
    if app.playback.playing && !app.playback.paused {
        let phase = (now.duration_since(app.glow_epoch).as_secs_f32()
            / crate::widgets::slot_list::GLOW_PERIOD_SECS)
            .fract();
        crate::widgets::slot_list::set_now_playing_phase(phase);
    }

    // The Harbour Trawl scene ticks BEFORE the Lines boat's early-outs: its
    // sea is procedural (a pure function of a phase this handler advances),
    // so it is independent of the visualizer mode, the `lines.boat` toggle,
    // AND the audio-pause freeze below — the scene keeps breathing while the
    // player is paused or stopped.
    step_harbour_scene(app, now);

    // Read mode + config snapshot once per tick. The "visualizer enabled"
    // check is `engine.visualization_mode != VisualizationMode::Off` — that's
    // what gates the shader element in the app_view visualizer-element build
    // (keyed on `engine.visualization_mode != VisualizationMode::Off`). There is no
    // separate `cfg.enabled` flag, so the `Lines` discriminator covers both
    // "visualizer on" and "lines mode" in a single check.
    let in_lines_mode = app.engine.visualization_mode == VisualizationMode::Lines;
    let cfg = app.visualizer_config.read();
    let cfg_boat_on = cfg.lines.boat;
    // The boat rides the Lines wave in whichever slot it's placed: the bottom
    // band (every view) or over the now-playing cover art (Queue). The physics
    // is normalized, so only the off-screen wrap margin differs between the two
    // — set below from the placement. The over-cover boat is rendered by the
    // Queue artwork panel; the bottom-band boat by `app_view`.
    let lines_in_bottom_band = cfg.lines.placement == VisualizerPlacement::BottomBand;
    let angular = cfg.lines.style == LinesStyle::Angular;
    let height_percent = cfg.height_percent;
    let lines_mirror = cfg.lines.mirror;
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
        // frozen frame doesn't re-rasterize on resume. The cached
        // `inverted` flag mirrors the render-time check in `boat.rs`:
        // outside mirrored line mode the renderer always draws the
        // upright sprite, so prewarming a flipped handle would waste a
        // cache slot.
        let tilt = app.boat.tilt;
        let facing = app.boat.facing;
        let render_inverted = lines_mirror && app.boat.inverted;
        let _ = app.boat.cache_handle_for(tilt, facing, render_inverted);
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

    // Size the off-screen wrap margin so the boat clears the visible area
    // before reappearing on the opposite side (`BOAT_WRAP_MARGIN_BOAT_WIDTHS`
    // boat-widths of pixel travel beyond the edge). The bottom band spans the
    // full window width, so the margin is derived from the live sprite width vs
    // that width (the visualizer area height is computed by the same helper
    // `app_view::view()` uses, so it tracks any future scaling-curve changes).
    // Over the cover the panel is ~square, so a size-independent constant
    // applies — see the `else` arm.
    app.boat.x_wrap_margin = if lines_in_bottom_band {
        let area_width = app.window.width;
        let area_height =
            visualizer_area_height(app.window.width, app.window.height, height_percent);
        let (boat_w, _boat_h) = boat::boat_pixel_size(area_height);
        if area_width > 0.0 {
            (boat_w * boat::BOAT_WRAP_MARGIN_BOAT_WIDTHS) / area_width
        } else {
            0.0
        }
    } else {
        // Over the cover the panel is ~square, so the wrap margin is a
        // panel-size-independent constant (the render-time cover width never
        // reaches this handler). See `OVER_COVER_WRAP_MARGIN`.
        boat::OVER_COVER_WRAP_MARGIN
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
    // `step()` is mirror-unaware and toggles `inverted` on every wrap so
    // mirrored mode gets the alternating top/bottom-wave surf. Outside
    // mirrored mode the renderer always draws upright, but the anchor-
    // firing guard in `step()` keys on `inverted` and would otherwise
    // freeze the countdown during every other wrap cycle — clear the
    // flag here so non-mirror sessions see the documented anchor cadence.
    if !lines_mirror {
        app.boat.inverted = false;
    }
    app.boat.visible = true;
    // Pre-build (and cache) the SVG handle for the current quantized
    // tilt + facing + inverted so the immutable `view()` render path
    // can clone it cheaply. The cache is keyed by
    // `(quantized_tilt_index, mirrored, inverted)`, so each
    // visibly-distinct orientation pays a one-time resvg cost and
    // every subsequent tick at that same quantized angle is a free
    // hashmap lookup. `render_inverted` mirrors the renderer's gate
    // (only meaningful when mirrored line mode is on) so we don't
    // burn a cache slot on a flipped sprite that would never be
    // displayed. While anchored, also prime the single themed anchor
    // handle so the doodad's render path is the same cheap lookup
    // (the anchor sprite doesn't rotate — the rope's swing lives in
    // the canvas path, not in the SVG).
    let tilt = app.boat.tilt;
    let facing = app.boat.facing;
    let render_inverted = lines_mirror && app.boat.inverted;
    let _ = app.boat.cache_handle_for(tilt, facing, render_inverted);
    if app.boat.anchor_remaining_secs > 0.0 {
        let _ = app.boat.cache_anchor_handle();
    }

    Task::none()
}

/// Per-frame step for the Harbour Trawl panel's trawling-longship scene.
///
/// A SEPARATE `BoatState` (`app.harbour_boat`) from the Lines boat, stepped
/// against a procedural sea instead of the FFT buffer:
/// - **Gate**: runs only on the Home screen with the Harbour view active and
///   an empty header search (during a search the Trawl row leaves the row
///   list entirely). Off-gate the boat hides and drops its dt baseline,
///   position preserved — the Lines boat's hide contract.
/// - **Sea**: `harbour_sea_phase` advances at `SEA_DRIFT_HZ` (wrapped with
///   `rem_euclid` so long sessions can't decay f32 sin precision), then ONE
///   `sea_bars` array is built, stepped against, and stored for the view —
///   the coherence contract that keeps the hull on the drawn water.
/// - **No audio coupling**: the bars are fed straight to `step()` (never
///   through `effective_bars`, which would empty them in silence and sink
///   the boat) with a fixed `bar_energy` cruise, so the scene animates
///   identically stopped, paused, or playing.
/// - **Trawl, not drop-anchor**: the built-in drop-anchor-and-hold event is
///   the thematic OPPOSITE of trawling, so both its fields are pinned BEFORE
///   `step()` each tick — pinning after would let the fire-check inside the
///   step land first and stall the boat for a frame every 45–120 s.
fn step_harbour_scene(app: &mut Nokkvi, now: Instant) {
    let on_harbour = app.screen == crate::Screen::Home
        && app.current_view == crate::View::Harbour
        && app.harbour.search_query.trim().is_empty();
    if !on_harbour {
        app.harbour_boat.visible = false;
        app.harbour_boat.last_tick = None;
        return;
    }

    let dt = match app.harbour_boat.last_tick {
        Some(prev) => now.saturating_duration_since(prev),
        None => std::time::Duration::ZERO,
    };
    app.harbour_boat.last_tick = Some(now);

    // Advance the travelling sea and build the ONE bars array this frame's
    // physics and render both consume.
    app.harbour_sea_phase = (app.harbour_sea_phase
        + dt.as_secs_f32() * crate::widgets::harbour_sea::SEA_DRIFT_HZ)
        .rem_euclid(1.0);
    let bars = crate::widgets::harbour_sea::sea_bars(app.harbour_sea_phase);

    // Suppress the drop-anchor state machine BEFORE the step (see docs).
    app.harbour_boat.anchor_remaining_secs = 0.0;
    app.harbour_boat.secs_until_next_anchor = boat::ANCHOR_INTERVAL_MAX_SECS;
    // The panel is ~square, so the same panel-size-independent wrap margin
    // the over-cover boat uses applies (the sprite sizes off min(w, h)).
    app.harbour_boat.x_wrap_margin = boat::OVER_COVER_WRAP_MARGIN;

    let music = MusicSignals {
        bpm: None,
        onset_energy: 0.0,
        long_onset_energy: 0.0,
        // Fixed presence cruise — the scene's calm lever (deliberately NOT
        // the sea's mean; see HARBOUR_CRUISE_BAR_ENERGY docs).
        bar_energy: crate::widgets::harbour_sea::HARBOUR_CRUISE_BAR_ENERGY,
    };
    boat::step(&mut app.harbour_boat, dt, &bars, false, music);
    // `step()` toggles `inverted` on every wrap for mirrored Lines mode; the
    // harbour scene never mirrors, so clear it (same as the Lines handler's
    // non-mirror path) to keep the render-cache key stable.
    app.harbour_boat.inverted = false;

    // Warm the SVG caches so the pure view path is a cheap handle clone.
    // Unlike the Lines path, the anchor handle is warmed EVERY tick — the
    // trawl draws the anchor unconditionally.
    let tilt = app.harbour_boat.tilt;
    let facing = app.harbour_boat.facing;
    let _ = app.harbour_boat.cache_handle_for(tilt, facing, false);
    let _ = app.harbour_boat.cache_anchor_handle();

    app.harbour_sea_bars = bars;
    app.harbour_boat.visible = true;
}
