//! Surfing-boat overlay for the lines-mode visualizer.
//!
//! Pure CPU helpers + a tiny stateful struct that the root TEA owns. The
//! widget does not touch the WGSL pipeline or the FFT thread — it only reads
//! the bar buffer (`VisualizerState::get_bars()`) the shader already consumes,
//! resamples it via the same Catmull-Rom basis (`catmull_rom_1d`) the lines
//! shader uses, and rides on top of the rendered waveform.
//!
//! Motion is a small force-based physics step rather than a fixed-period
//! oscillation, so the boat actually responds to the wave it's riding:
//!
//! - **Drive oscillator** — a slow sine of `phase` provides the baseline
//!   left/right rhythm. Period is long (`DRIVE_PERIOD_SECS`) so the boat
//!   drifts unhurriedly across the visualizer.
//! - **Slope drift** — the local wave gradient at the boat's x position
//!   pushes it downhill (positive slope → push left, negative → push right),
//!   so the boat appears to surf down wave faces.
//! - **Restoring force** — a soft spring toward `x = 0.5` keeps the boat off
//!   the edges when the drive + slope conspire to push it outward.
//! - **Velocity damping** — friction on `x_velocity` gives the "floating"
//!   feel; the boat lags fast wave changes instead of snapping to them.
//! - **Y dynamics** — `y_ratio` follows the sampled wave height through a
//!   spring-damper rather than tracking it exactly, so the boat bobs with
//!   buoyancy rather than glued to the curve.
//!
//! `BoatState.handle` is built lazily on first use from
//! `embedded_svg::themed_logo_svg()` and reused thereafter. `Handle::from_memory`
//! re-hashes input bytes per call (see `reference-iced/core/src/svg.rs:89`),
//! so per-frame construction would churn GPU cache keys — the same class of
//! bug as the `image::Handle::from_path` gotcha called out in `CLAUDE.md`.

use std::time::{Duration, Instant};

use iced::{
    Element, Length, Vector,
    widget::{Float, Svg, container, svg},
};

use crate::widgets::visualizer::state::catmull_rom_1d;

/// Boat height as a fraction of the visualizer height. v1 default.
pub(crate) const BOAT_HEIGHT_FRACTION: f32 = 0.18;

/// Boat aspect ratio (width / height). The placeholder logo SVG is square,
/// so the boat is rendered as a square.
pub(crate) const BOAT_ASPECT_RATIO: f32 = 1.0;

// --- physics tuning constants ---------------------------------------------
//
// All forces operate in normalized ratio-space (`x_ratio` ∈ [0, 1], time in
// seconds). At equilibrium with `DRIVE_FORCE` and `X_DAMPING` alone the boat
// drifts at ~`DRIVE_FORCE / X_DAMPING` ratio/sec — currently ~22 s edge to
// edge, much slower than the old fixed 12 s round-trip.

/// Drive oscillator period: time for the sine to complete one full cycle
/// (push right → push left → back). Longer than the visible drift period
/// because damping eats most of the impulse.
const DRIVE_PERIOD_SECS: f32 = 25.0;

/// Peak amplitude of the drive force (sine of phase). Chosen so equilibrium
/// drift speed is calm but visible.
const DRIVE_FORCE: f32 = 0.04;

/// Sample distance (in ratio space) on either side of the boat for the
/// finite-difference slope estimate. Small enough to capture local curvature,
/// large enough to smooth out single-bar jitter.
const SLOPE_DX: f32 = 0.05;

/// Slope force gain — converts wave gradient into horizontal force.
const SLOPE_GAIN: f32 = 0.04;

/// Spring constant for the soft pull back toward `x = 0.5`. Keeps the boat
/// off the edges without obvious snap-back.
const RESTORING_K: f32 = 0.06;

/// Friction on `x_velocity`. The dominant source of the "floating" feel —
/// without this, the boat would build up arbitrary speed.
const X_DAMPING: f32 = 0.9;

/// Hard cap on `|x_velocity|` to keep numerical extremes from launching the
/// boat across the screen in a single tick.
const MAX_X_V: f32 = 0.15;

/// Spring constant for `y_ratio` tracking the sampled wave height. Higher =
/// boat sticks tighter to the curve.
const Y_SPRING_K: f32 = 80.0;

/// Damping on `y_velocity`. With `Y_SPRING_K = 80` and damping `12` the
/// damping ratio ζ ≈ 0.67 — slightly underdamped, so a quick wave change
/// produces a small bob before settling.
const Y_DAMPING: f32 = 12.0;

/// Per-frame UI-thread state for the surfing boat.
///
/// - `phase` is in `[0, 1)` and ticks linearly with time. Drives the slow
///   sine that biases horizontal motion left vs right.
/// - `x_ratio` / `y_ratio` are the boat's normalized position in `[0, 1]`,
///   integrated from the velocity fields below.
/// - `x_velocity` / `y_velocity` are persisted across ticks so the physics
///   has memory (inertia → floating feel).
/// - `visible` is derived per tick by the handler — it is *not* the user's
///   on/off toggle (that lives in `LinesConfig.boat`).
/// - `last_tick` is consumed to compute `dt` between ticks; cleared when the
///   boat is hidden so the first frame back doesn't see a stale gap.
/// - `handle` caches the themed logo SVG so we don't rebuild it every frame.
#[derive(Debug, Clone, Default)]
pub struct BoatState {
    pub phase: f32,
    pub x_ratio: f32,
    pub y_ratio: f32,
    pub x_velocity: f32,
    pub y_velocity: f32,
    pub visible: bool,
    pub last_tick: Option<Instant>,
    pub handle: Option<svg::Handle>,
}

impl BoatState {
    /// Lazily build (and cache) the themed boat SVG handle. Call once per
    /// render — the handle is reused thereafter.
    pub(crate) fn ensure_handle(&mut self) -> svg::Handle {
        if let Some(h) = &self.handle {
            return h.clone();
        }
        let bytes = crate::embedded_svg::themed_logo_svg().into_bytes();
        let h = svg::Handle::from_memory(bytes);
        self.handle = Some(h.clone());
        h
    }

    /// Drop the cached handle so the next render rebuilds it from the
    /// freshly-themed SVG. Used when the active theme changes — out of v1
    /// scope but cheap to expose.
    #[allow(dead_code)]
    pub(crate) fn invalidate_handle(&mut self) {
        self.handle = None;
    }
}

/// Step the boat physics forward by `dt`, sampling slope and target height
/// from `bars`. Mutates `phase`, `x_velocity`, `x_ratio`, `y_velocity`, and
/// `y_ratio` on `state`.
///
/// Forces on `x` (semi-implicit Euler):
/// - drive: `sin(2π·phase) · DRIVE_FORCE` — slow rhythmic push
/// - slope: `-slope · SLOPE_GAIN` — surf downhill
/// - restoring: `(0.5 - x) · RESTORING_K` — soft center pull
/// - damping: `-x_velocity · X_DAMPING` — friction
///
/// Y is a spring-damper tracking `target_y = sample_line_height(...)`:
/// `ay = (target_y - y) · Y_SPRING_K - y_velocity · Y_DAMPING`.
///
/// At the edges we clamp `x_ratio` and zero out any outward velocity
/// component so the boat doesn't accumulate wall-pushing momentum.
pub(crate) fn step(state: &mut BoatState, dt: Duration, bars: &[f64], angular: bool) {
    let dt_secs = dt.as_secs_f32();
    if dt_secs <= 0.0 {
        return;
    }

    state.phase = (state.phase + dt_secs / DRIVE_PERIOD_SECS).rem_euclid(1.0);
    let drive = (state.phase * std::f32::consts::TAU).sin() * DRIVE_FORCE;

    let h_left = sample_line_height(bars, (state.x_ratio - SLOPE_DX).max(0.0), angular);
    let h_right = sample_line_height(bars, (state.x_ratio + SLOPE_DX).min(1.0), angular);
    let slope = (h_right - h_left) / (2.0 * SLOPE_DX);
    let slope_force = -slope * SLOPE_GAIN;

    let restoring_force = (0.5 - state.x_ratio) * RESTORING_K;
    let damping_force = -state.x_velocity * X_DAMPING;

    let ax = drive + slope_force + restoring_force + damping_force;
    state.x_velocity = (state.x_velocity + ax * dt_secs).clamp(-MAX_X_V, MAX_X_V);
    state.x_ratio += state.x_velocity * dt_secs;
    if state.x_ratio <= 0.0 {
        state.x_ratio = 0.0;
        if state.x_velocity < 0.0 {
            state.x_velocity = 0.0;
        }
    } else if state.x_ratio >= 1.0 {
        state.x_ratio = 1.0;
        if state.x_velocity > 0.0 {
            state.x_velocity = 0.0;
        }
    }

    let target_y = sample_line_height(bars, state.x_ratio, angular);
    let ay = (target_y - state.y_ratio) * Y_SPRING_K - state.y_velocity * Y_DAMPING;
    state.y_velocity += ay * dt_secs;
    state.y_ratio = (state.y_ratio + state.y_velocity * dt_secs).clamp(0.0, 1.0);
}

/// Sample the visible waveform height at a given normalized horizontal
/// position `x_ratio` ∈ `[0, 1]` from the live bar buffer.
///
/// In `smooth` (Catmull-Rom) mode this matches the curve the shader draws.
/// In `angular` mode it's a straight-line lerp between the two flanking
/// control points. Returns 0.0 for empty / 1-element buffers.
pub(crate) fn sample_line_height(bars: &[f64], x_ratio: f32, angular: bool) -> f32 {
    if bars.is_empty() {
        return 0.0;
    }
    if bars.len() == 1 {
        return bars[0] as f32;
    }

    let n = bars.len();
    let last_idx = (n - 1) as f32;
    let pos = x_ratio.clamp(0.0, 1.0) * last_idx;
    let segment = (pos.floor() as usize).min(n - 2);
    let t = (pos - segment as f32) as f64;

    if angular {
        let p1 = bars[segment];
        let p2 = bars[segment + 1];
        return (p1 + (p2 - p1) * t) as f32;
    }

    // Edge-clamped Catmull-Rom: the four flanking control points around `pos`.
    let p0 = bars[segment.saturating_sub(1)];
    let p1 = bars[segment];
    let p2 = bars[(segment + 1).min(n - 1)];
    let p3 = bars[(segment + 2).min(n - 1)];

    catmull_rom_1d(p0, p1, p2, p3, t) as f32
}

/// Build the boat overlay element. Returned as a plain `Element` (not
/// `Option<Element>`) so the visibility branch lives at the call site —
/// `column!` and `Stack::push` expect `impl Into<Element>`.
///
/// `area_width` / `area_height` are the pixel dimensions of the visualizer
/// area the boat rides over. They are needed because Float's `translate`
/// closure receives only the bounds of its own content (the boat itself,
/// here `boat_w × boat_h`), not the surrounding area — so the caller must
/// supply the area dimensions explicitly.
pub(crate) fn boat_overlay<'a, M: 'a>(
    state: &BoatState,
    area_width: f32,
    area_height: f32,
) -> Element<'a, M> {
    // The handler is responsible for calling `ensure_handle()` on the first
    // visible tick, so by the time we render the handle is cached. The fallback
    // here keeps the contract robust if a render somehow precedes the tick.
    let handle = state.handle.clone().unwrap_or_else(|| {
        svg::Handle::from_memory(crate::embedded_svg::themed_logo_svg().into_bytes())
    });
    let boat_h = (area_height * BOAT_HEIGHT_FRACTION).max(8.0);
    let boat_w = boat_h * BOAT_ASPECT_RATIO;
    let x_ratio = state.x_ratio;
    let y_ratio = state.y_ratio;

    Float::new(
        container(Svg::new(handle).width(Length::Fill).height(Length::Fill))
            .width(Length::Fixed(boat_w))
            .height(Length::Fixed(boat_h)),
    )
    .translate(move |_content_bounds, _viewport| {
        // Float lays out its content at the top-left of the surrounding
        // container, then this translate shifts it. Target offset within the
        // visualizer area:
        //   centered horizontally at x_ratio * area_width
        //   bottom of boat sits on the waveform line, which is at
        //     (1 - y_ratio) * area_height (visualizer draws upward from bottom)
        let cx = x_ratio * area_width;
        let target_x = cx - boat_w * 0.5;
        let line_y = area_height * (1.0 - y_ratio);
        let target_y = line_y - boat_h;
        Vector::new(target_x, target_y)
    })
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- step physics ----------------------------------------------------------

    /// Run `step` repeatedly with a small dt, returning the final state.
    fn run(initial: BoatState, ticks: usize, dt: Duration, bars: &[f64]) -> BoatState {
        let mut state = initial;
        for _ in 0..ticks {
            step(&mut state, dt, bars, false);
        }
        state
    }

    #[test]
    fn step_zero_dt_is_noop() {
        let bars = vec![0.5; 10];
        let mut state = BoatState {
            x_ratio: 0.3,
            x_velocity: 0.05,
            ..Default::default()
        };
        let snapshot = state.clone();
        step(&mut state, Duration::ZERO, &bars, false);
        assert_eq!(state.x_ratio, snapshot.x_ratio);
        assert_eq!(state.x_velocity, snapshot.x_velocity);
        assert_eq!(state.phase, snapshot.phase);
    }

    #[test]
    fn step_advances_phase_linearly() {
        let bars = vec![0.5; 10];
        let mut state = BoatState::default();
        // 1 second worth of ticks should advance phase by 1 / DRIVE_PERIOD_SECS.
        let dt = Duration::from_millis(10);
        for _ in 0..100 {
            step(&mut state, dt, &bars, false);
        }
        let expected = 1.0 / DRIVE_PERIOD_SECS;
        assert!(
            (state.phase - expected).abs() < 1e-3,
            "phase after 1s should be ~1/DRIVE_PERIOD_SECS (expected {expected}, got {phase})",
            phase = state.phase
        );
    }

    #[test]
    fn step_drives_motion_from_rest_on_flat_waves() {
        // Flat bars + center start: only the drive force should act; after a
        // few seconds the boat must have moved measurably.
        let bars = vec![0.5; 16];
        let initial = BoatState {
            x_ratio: 0.5,
            ..Default::default()
        };
        let final_state = run(initial, 600, Duration::from_millis(10), &bars);
        assert!(
            (final_state.x_ratio - 0.5).abs() > 0.01,
            "drive force should have moved the boat off-center after 6s (got {x})",
            x = final_state.x_ratio
        );
    }

    #[test]
    fn step_keeps_x_in_unit_range_under_extreme_slope() {
        // Large amplitude waveform that would produce big slope forces in
        // every direction. After many ticks `x_ratio` must stay clamped.
        let bars: Vec<f64> = (0..32)
            .map(|i| ((i as f64 * 0.7).sin() * 0.5 + 0.5).clamp(0.0, 1.0))
            .collect();
        let initial = BoatState {
            x_ratio: 0.5,
            x_velocity: 0.1,
            ..Default::default()
        };
        let final_state = run(initial, 5000, Duration::from_millis(10), &bars);
        assert!(
            (0.0..=1.0).contains(&final_state.x_ratio),
            "x_ratio must remain in [0, 1] (got {x})",
            x = final_state.x_ratio
        );
    }

    #[test]
    fn step_slope_creates_downhill_velocity() {
        // Bars that ramp upward to the right: at the boat's position the
        // sampled height to the right is greater → positive slope → downhill
        // is to the LEFT → x_velocity should become negative.
        // Disable the drive contribution by sitting at phase = 0 (sin = 0)
        // and the restoring contribution by sitting at x = 0.5.
        let bars: Vec<f64> = (0..16).map(|i| i as f64 / 15.0).collect();
        let mut state = BoatState {
            x_ratio: 0.5,
            phase: 0.0,
            ..Default::default()
        };
        // One short tick — slope dominates.
        step(&mut state, Duration::from_millis(50), &bars, false);
        assert!(
            state.x_velocity < 0.0,
            "upward-ramp bars should push boat left (got x_velocity = {v})",
            v = state.x_velocity
        );
    }

    #[test]
    fn step_y_ratio_lags_target_then_settles() {
        // Constant-height bars far above current y_ratio. Y dynamics should
        // approach but not snap to the target on a single tick, then settle
        // close to it after enough simulated time.
        let bars = vec![0.8; 16];
        let mut state = BoatState {
            x_ratio: 0.5,
            y_ratio: 0.0,
            ..Default::default()
        };

        // Single short tick: y_ratio moves toward 0.8 but is far from it.
        step(&mut state, Duration::from_millis(16), &bars, false);
        let y_after_one = state.y_ratio;
        assert!(
            y_after_one > 0.0 && y_after_one < 0.5,
            "y_ratio must lag the target on a single tick (got {y_after_one})"
        );

        // Many ticks (~3 s): y_ratio should be near the target.
        for _ in 0..300 {
            step(&mut state, Duration::from_millis(10), &bars, false);
        }
        assert!(
            (state.y_ratio - 0.8).abs() < 0.05,
            "y_ratio must settle near the target after enough time (got {y})",
            y = state.y_ratio
        );
    }

    // --- sample_line_height ----------------------------------------------------

    #[test]
    fn sample_line_height_clamps_edges() {
        let bars = vec![0.1, 0.4, 0.7, 0.9];
        // x_ratio = 0.0 lands exactly on bars[0]; smooth output must equal it.
        let v0 = sample_line_height(&bars, 0.0, false);
        assert!(
            (v0 - 0.1).abs() < 1e-5,
            "x_ratio=0 should sample first control point ({v0})"
        );

        // x_ratio = 1.0 lands exactly on bars[n-1].
        let v1 = sample_line_height(&bars, 1.0, false);
        assert!(
            (v1 - 0.9).abs() < 1e-5,
            "x_ratio=1 should sample last control point ({v1})"
        );
    }

    #[test]
    fn sample_line_height_matches_shader_for_smooth() {
        // Hand-computed Catmull-Rom value for a known control-point pattern.
        // Using bars = [0.0, 0.5, 1.0, 0.5, 0.0] and x_ratio at the mid of
        // segment (1, 2) i.e. position = 1.5 → t = 0.5.
        // Control points are (p0=0.0, p1=0.5, p2=1.0, p3=0.5).
        // catmull_rom_1d(0.0, 0.5, 1.0, 0.5, 0.5) = 0.5 * (1.0 + 1.0 * 0.5
        //   + (0.0 - 2.5 + 4.0 - 0.5) * 0.25
        //   + (0.0 + 1.5 - 3.0 + 0.5) * 0.125)
        // = 0.5 * (1.5 + 1.0 * 0.25 + (-1.0) * 0.125)
        // = 0.5 * (1.5 + 0.25 - 0.125) = 0.5 * 1.625 = 0.8125
        let bars = vec![0.0, 0.5, 1.0, 0.5, 0.0];
        // x_ratio of 1.5 / 4.0 = 0.375
        let got = sample_line_height(&bars, 0.375, false);
        let expected = 0.8125_f32;
        assert!(
            (got - expected).abs() < 1e-5,
            "smooth mode should match hand-computed Catmull-Rom (got {got}, expected {expected})"
        );
    }

    #[test]
    fn sample_line_height_linear_in_angular_mode() {
        let bars = vec![0.0, 1.0];
        // x_ratio = 0.5 in a 2-point buffer is a linear midpoint.
        let v = sample_line_height(&bars, 0.5, true);
        assert!(
            (v - 0.5).abs() < 1e-5,
            "angular mode at midpoint should be linear lerp (got {v})"
        );

        // Quarter point.
        let v = sample_line_height(&bars, 0.25, true);
        assert!(
            (v - 0.25).abs() < 1e-5,
            "angular mode at 0.25 should be 0.25 (got {v})"
        );
    }

    #[test]
    fn sample_line_height_handles_short_buffers() {
        assert_eq!(sample_line_height(&[], 0.5, false), 0.0);
        assert_eq!(sample_line_height(&[0.42], 0.5, false), 0.42);
        assert_eq!(sample_line_height(&[0.42], 0.5, true), 0.42);
    }
}
