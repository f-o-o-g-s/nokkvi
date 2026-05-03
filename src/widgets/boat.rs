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
//!   so the boat appears to surf down wave faces. Capped at
//!   `MAX_SLOPE_FORCE` so a single tall bar can't overpower drive +
//!   restoring indefinitely.
//! - **Restoring force** — a soft spring toward `x = 0.5` keeps the boat off
//!   the edges when the drive + slope conspire to push it outward.
//! - **Velocity damping** — friction on `x_velocity` gives the "floating"
//!   feel; the boat lags fast wave changes instead of snapping to them.
//! - **Captain charge** — left to physics alone the slope force always pushes
//!   downhill, so the boat camps on the calmer side of the visualizer. Every
//!   `CHARGE_INTERVAL_*` seconds (randomized) the captain decides to row in
//!   a chosen direction for `CHARGE_DURATION_*` seconds: a small constant
//!   force is applied and the slope force is suppressed for the duration.
//!   Direction is biased toward whichever half of the waveform has taller
//!   bars (the "storm"); falls back to a coin flip on a symmetric waveform.
//!   Force size is tuned so terminal velocity during a charge is only
//!   modestly above normal cruise — feels like determined effort, not a
//!   speed boost.
//! - **Wall bumper** — within `WALL_ZONE` of either edge a quadratic spring
//!   pushes the boat inward, and the slope force is suppressed inside the
//!   zone so wave drift can't keep dragging the boat into the wall (the FFT
//!   waveform almost always slopes downward at the visualizer edges, which
//!   would otherwise pin the boat there). Captain charges also bias toward
//!   center when the boat has drifted noticeably off-center, so the boat
//!   actively explores the middle instead of camping near a wall.
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
    Element, Length,
    widget::{Space, Svg, column, container, row, svg},
};

use crate::widgets::visualizer::state::catmull_rom_1d;

/// Boat height as a fraction of the visualizer height. v1 default.
pub(crate) const BOAT_HEIGHT_FRACTION: f32 = 0.18;

/// Boat aspect ratio (width / height). The placeholder logo SVG is square,
/// so the boat is rendered as a square.
pub(crate) const BOAT_ASPECT_RATIO: f32 = 1.0;

/// Fraction of the boat's height that sits below the wave line ("waterline
/// sink"). Without this offset the boat appears glued to the top of the
/// curve and reads as floating in space rather than displacing water.
pub(crate) const BOAT_SINK_FRACTION: f32 = 0.18;

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

/// Hard cap on `|slope_force|` so a single tall bar near the boat can't
/// overpower drive + restoring indefinitely. Without this the boat could
/// be pinned at an edge by a sustained steep waveform.
const MAX_SLOPE_FORCE: f32 = 0.10;

/// Spring constant for the soft pull back toward `x = 0.5`. Keeps the boat
/// off the edges without obvious snap-back.
const RESTORING_K: f32 = 0.06;

/// Min/max delay between captain charges. The actual interval is sampled
/// uniformly in `[MIN, MAX]` after each charge ends (and at first activation),
/// so charges feel scheduled by mood rather than clockwork.
const CHARGE_INTERVAL_MIN_SECS: f32 = 12.0;
const CHARGE_INTERVAL_MAX_SECS: f32 = 30.0;

/// Min/max duration of a single charge. Sampled uniformly in `[MIN, MAX]`
/// when a charge starts. Long enough that the boat covers a notable fraction
/// of the visualizer width before the charge ends.
const CHARGE_DURATION_MIN_SECS: f32 = 6.0;
const CHARGE_DURATION_MAX_SECS: f32 = 12.0;

/// Constant horizontal force applied during a charge, in the chosen
/// direction. Sized so terminal velocity (`CHARGE_FORCE / X_DAMPING`) is
/// only slightly above normal cruise — `0.06 / 0.9 ≈ 0.067 ratio/sec`,
/// versus the natural `~0.045 ratio/sec` drift speed. Reads as deliberate
/// effort, not a speed boost.
const CHARGE_FORCE: f32 = 0.06;

/// Minimum waveform asymmetry (`mean(right_half) - mean(left_half)`) needed
/// to bias the charge direction toward the "storm" side. Below this the
/// captain just flips a coin so symmetric waveforms still produce variety.
const IMBALANCE_THRESHOLD: f32 = 0.05;

/// Width (in ratio space) of the soft "bumper" zone at each edge. Inside
/// this zone the wall-repulsion spring is active and the slope force is
/// suppressed. Wider zone = larger region where the boat is actively
/// pushed back toward center.
const WALL_ZONE: f32 = 0.15;

/// Peak strength of the wall-repulsion spring (force at the wall itself).
/// Sized to dominate the worst-case outward sum of `DRIVE_FORCE` plus
/// `CHARGE_FORCE` (slope is suppressed inside the zone, so it doesn't
/// factor in here). The boat physically cannot park against an edge.
const WALL_REPULSION: f32 = 0.45;

/// `|x_ratio - 0.5|` past which captain charges are redirected toward
/// center regardless of the storm direction. Stops the captain from
/// charging back into a wall when the boat has already drifted near it.
const CHARGE_RETURN_THRESHOLD: f32 = 0.25;

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
/// - `secs_until_next_charge` counts down to the next captain charge; only
///   meaningful while `charge_remaining_secs == 0`.
/// - `charge_remaining_secs` is the time left in the current charge; > 0
///   means the captain is actively rowing in `charge_direction`.
/// - `charge_direction` is `+1.0` (right) or `-1.0` (left); set when a
///   charge begins, unread otherwise.
/// - `rng_state` seeds a tiny xorshift PRNG used for charge timing and
///   direction. Lazily seeded on first tick (0 → seed constant) so `Default`
///   stays clean and the schedule starts on first activation, not on
///   construction.
/// - `handle` caches the themed logo SVG so we don't rebuild it every frame.
/// - `handle_generation` is the `theme::theme_generation()` snapshot taken at
///   the time `handle` was last built. When the global counter advances —
///   any path that runs `theme::reload_theme()` or `theme::set_light_mode()`
///   — the cache is recognized as stale and rebuilt on the next render. This
///   replaces the previous explicit-invalidation approach, which had to be
///   wired up at every theme-change call site and missed preset switches.
#[derive(Debug, Clone, Default)]
pub struct BoatState {
    pub phase: f32,
    pub x_ratio: f32,
    pub y_ratio: f32,
    pub x_velocity: f32,
    pub y_velocity: f32,
    pub visible: bool,
    pub last_tick: Option<Instant>,
    pub secs_until_next_charge: f32,
    pub charge_remaining_secs: f32,
    pub charge_direction: f32,
    pub rng_state: u32,
    pub handle: Option<svg::Handle>,
    pub handle_generation: u64,
}

impl BoatState {
    /// Lazily build (and cache) the themed boat SVG handle, rebuilding when
    /// the active theme has changed since the cache was populated.
    ///
    /// The cache key is `theme::theme_generation()` — bumped by
    /// `reload_theme()` and `set_light_mode()`. Snapshotting it here means
    /// we don't need to remember to invalidate at every theme-change call
    /// site (preset switch, color picker edit, restore-defaults, etc.).
    pub(crate) fn ensure_handle(&mut self) -> svg::Handle {
        let current_gen = crate::theme::theme_generation();
        if let Some(h) = &self.handle
            && self.handle_generation == current_gen
        {
            return h.clone();
        }
        let bytes = crate::embedded_svg::themed_logo_svg().into_bytes();
        let h = svg::Handle::from_memory(bytes);
        self.handle = Some(h.clone());
        self.handle_generation = current_gen;
        h
    }
}

/// Step the boat physics forward by `dt`, sampling slope and target height
/// from `bars`. Mutates `phase`, `x_velocity`, `x_ratio`, `y_velocity`,
/// `y_ratio`, the charge-state fields, and `rng_state` on `state`.
///
/// Forces on `x` (semi-implicit Euler):
/// - drive: `sin(2π·phase) · DRIVE_FORCE` — slow rhythmic push
/// - slope: `(-slope · SLOPE_GAIN).clamp(±MAX_SLOPE_FORCE)` — surf downhill,
///   capped so a single tall bar can't dominate. Suppressed during a charge
///   (so the captain's heading isn't fighting wave drift) AND inside the
///   wall bumper zone (so the perpetually-outward FFT slope at the edges
///   can't drag the boat into a wall).
/// - restoring: `(0.5 - x) · RESTORING_K` — soft center pull
/// - damping: `-x_velocity · X_DAMPING` — friction
/// - charge: `charge_direction · CHARGE_FORCE` while a charge is active,
///   else 0. Captain charges fire on a randomized timer and prefer the
///   side of the visualizer with taller bars.
/// - wall: quadratic spring active inside `WALL_ZONE` of either edge,
///   peaking at `WALL_REPULSION` at the wall itself. Stops the boat from
///   parking against an edge when other forces happen to align outward.
///
/// Y is a spring-damper tracking `target_y = sample_line_height(...)`:
/// `ay = (target_y - y) · Y_SPRING_K - y_velocity · Y_DAMPING`. Y dynamics
/// are unchanged during a charge — the boat still bobs over the waves it
/// crosses.
///
/// At the edges we clamp `x_ratio` and zero out any outward velocity
/// component so the boat doesn't accumulate wall-pushing momentum.
pub(crate) fn step(state: &mut BoatState, dt: Duration, bars: &[f64], angular: bool) {
    let dt_secs = dt.as_secs_f32();
    if dt_secs <= 0.0 {
        return;
    }

    // Lazy schedule init. `Default` leaves rng_state == 0, which is the only
    // value xorshift won't produce from a non-zero seed — so we use it as a
    // sentinel and seed the very first charge interval here. After this the
    // rng is permanently non-zero.
    if state.rng_state == 0 {
        state.rng_state = 0x9E37_79B9;
        let r = next_rand_unit(&mut state.rng_state);
        state.secs_until_next_charge = lerp(CHARGE_INTERVAL_MIN_SECS, CHARGE_INTERVAL_MAX_SECS, r);
    }

    state.phase = (state.phase + dt_secs / DRIVE_PERIOD_SECS).rem_euclid(1.0);
    let drive = (state.phase * std::f32::consts::TAU).sin() * DRIVE_FORCE;

    let h_left = sample_line_height(bars, (state.x_ratio - SLOPE_DX).max(0.0), angular);
    let h_right = sample_line_height(bars, (state.x_ratio + SLOPE_DX).min(1.0), angular);
    let slope = (h_right - h_left) / (2.0 * SLOPE_DX);
    let slope_force_raw = (-slope * SLOPE_GAIN).clamp(-MAX_SLOPE_FORCE, MAX_SLOPE_FORCE);

    // Slope is suppressed inside the wall zone — FFT waveforms almost always
    // slope outward at the visualizer edges, which without this would keep
    // dragging the boat into whichever wall it's near.
    let in_wall_zone = state.x_ratio < WALL_ZONE || state.x_ratio > 1.0 - WALL_ZONE;
    let slope_force_zoned = if in_wall_zone { 0.0 } else { slope_force_raw };

    // Charge state machine: either rowing (charge_remaining_secs > 0) or
    // counting down to the next charge. Slope is suppressed for the duration
    // of a charge so the captain's heading isn't fought by wave drift.
    let (charge_force, slope_force) = if state.charge_remaining_secs > 0.0 {
        state.charge_remaining_secs -= dt_secs;
        if state.charge_remaining_secs <= 0.0 {
            state.charge_remaining_secs = 0.0;
            let r = next_rand_unit(&mut state.rng_state);
            state.secs_until_next_charge =
                lerp(CHARGE_INTERVAL_MIN_SECS, CHARGE_INTERVAL_MAX_SECS, r);
        }
        (state.charge_direction * CHARGE_FORCE, 0.0)
    } else {
        state.secs_until_next_charge -= dt_secs;
        if state.secs_until_next_charge <= 0.0 {
            state.charge_direction =
                pick_charge_direction(bars, state.x_ratio, &mut state.rng_state);
            let r = next_rand_unit(&mut state.rng_state);
            state.charge_remaining_secs =
                lerp(CHARGE_DURATION_MIN_SECS, CHARGE_DURATION_MAX_SECS, r);
            (state.charge_direction * CHARGE_FORCE, 0.0)
        } else {
            (0.0, slope_force_zoned)
        }
    };

    let restoring_force = (0.5 - state.x_ratio) * RESTORING_K;
    let damping_force = -state.x_velocity * X_DAMPING;

    // Wall bumper: quadratic spring active only inside `WALL_ZONE`. `depth`
    // is 0 at the zone boundary and 1 at the wall, so force ramps from 0 up
    // to `WALL_REPULSION`. Quadratic (`depth²`) gives a soft cushion entering
    // the zone, then bites hard near the wall — the user-facing "bounce".
    let wall_force = if state.x_ratio < WALL_ZONE {
        let depth = 1.0 - state.x_ratio / WALL_ZONE;
        WALL_REPULSION * depth * depth
    } else if state.x_ratio > 1.0 - WALL_ZONE {
        let depth = (state.x_ratio - (1.0 - WALL_ZONE)) / WALL_ZONE;
        -WALL_REPULSION * depth * depth
    } else {
        0.0
    };

    let ax = drive + slope_force + restoring_force + damping_force + charge_force + wall_force;
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
    state.y_ratio += state.y_velocity * dt_secs;
    if state.y_ratio <= 0.0 {
        state.y_ratio = 0.0;
        if state.y_velocity < 0.0 {
            state.y_velocity = 0.0;
        }
    } else if state.y_ratio >= 1.0 {
        state.y_ratio = 1.0;
        if state.y_velocity > 0.0 {
            state.y_velocity = 0.0;
        }
    }
}

/// Pick the bars buffer the boat physics should sample for this tick.
///
/// When audio isn't actively producing samples (`playing == false`) we
/// return an empty slice so `sample_line_height` reports 0 and the spring
/// pulls `y_ratio` toward the bottom. The visualizer's `display.bars`
/// buffer can stay frozen at non-zero values during silence — the FFT
/// thread's gravity-falloff path only runs when a full sample chunk is
/// available — and we don't want the boat tracking those stale values
/// while the user perceives "no waves".
pub(crate) fn effective_bars(playing: bool, raw_bars: &[f64]) -> &[f64] {
    if playing { raw_bars } else { &[] }
}

/// Sample the visible waveform height at a given normalized horizontal
/// position `x_ratio` ∈ `[0, 1]` from the live bar buffer.
///
/// In `smooth` (Catmull-Rom) mode this matches the curve the shader draws.
/// In `angular` mode it's a straight-line lerp between the two flanking
/// control points. Returns 0.0 for empty / 1-element buffers. Output is
/// clamped to `[0, 1]` to match the lines shader's `clamp(value, 0, 1)`
/// at `shaders/lines.wgsl:202` — Catmull-Rom can extrapolate outside the
/// control-point range when neighbours are sharply peaked, and a transient
/// spectrum-engine overshoot above 1.0 (auto-sensitivity hasn't yet
/// adapted) would otherwise push the boat above the rendered wave.
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
        return ((p1 + (p2 - p1) * t) as f32).clamp(0.0, 1.0);
    }

    // Edge-clamped Catmull-Rom: the four flanking control points around `pos`.
    let p0 = bars[segment.saturating_sub(1)];
    let p1 = bars[segment];
    let p2 = bars[(segment + 1).min(n - 1)];
    let p3 = bars[(segment + 2).min(n - 1)];

    (catmull_rom_1d(p0, p1, p2, p3, t) as f32).clamp(0.0, 1.0)
}

/// Tiny xorshift PRNG used to schedule captain charges and pick directions.
/// Self-contained so no extra dependency is needed in the UI crate. The
/// caller is responsible for seeding `state` to non-zero before the first
/// call; xorshift's only fixed point is 0, so as long as the seed is
/// non-zero the sequence stays non-zero. Deterministic per seed — fine for
/// timing variation, would be wrong for anything cryptographic.
fn next_rand_unit(state: &mut u32) -> f32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    (x as f32) / (u32::MAX as f32)
}

/// Linear interpolation `a → b` by `t ∈ [0, 1]`. Used to map a uniform
/// `[0, 1)` random sample into a charge interval/duration window.
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Mean amplitude of the right half of `bars` minus mean of the left half.
/// Positive → right side has taller bars (storm to the right). Returns 0
/// for buffers shorter than 4 (not enough data to split).
fn waveform_imbalance(bars: &[f64]) -> f32 {
    if bars.len() < 4 {
        return 0.0;
    }
    let mid = bars.len() / 2;
    let left_avg: f64 = bars[..mid].iter().sum::<f64>() / mid as f64;
    let right_avg: f64 = bars[mid..].iter().sum::<f64>() / (bars.len() - mid) as f64;
    (right_avg - left_avg) as f32
}

/// Pick a charge direction (`±1`). Priority order:
/// 1. If the boat has drifted past `CHARGE_RETURN_THRESHOLD` from center,
///    head toward center regardless of the storm. Stops the captain from
///    charging back into a wall the boat has already drifted near.
/// 2. Otherwise, if the waveform is meaningfully asymmetric
///    (`|imbalance| > IMBALANCE_THRESHOLD`), head toward the storm.
/// 3. Otherwise coin flip — symmetric tracks still produce variety.
fn pick_charge_direction(bars: &[f64], x_ratio: f32, rng_state: &mut u32) -> f32 {
    let off_center = x_ratio - 0.5;
    if off_center.abs() > CHARGE_RETURN_THRESHOLD {
        return -off_center.signum();
    }
    let imbalance = waveform_imbalance(bars);
    if imbalance.abs() > IMBALANCE_THRESHOLD {
        imbalance.signum()
    } else if next_rand_unit(rng_state) < 0.5 {
        -1.0
    } else {
        1.0
    }
}

/// Build the boat overlay element. Returned as a plain `Element` (not
/// `Option<Element>`) so the visibility branch lives at the call site —
/// `column!` and `Stack::push` expect `impl Into<Element>`.
///
/// `area_width` / `area_height` are the pixel dimensions of the visualizer
/// area the boat rides over. They size the outer clipping container and let
/// us compute the boat's pixel position from `(x_ratio, y_ratio)`.
///
/// Layout: a fixed-size `container.clip(true)` framing the visualizer area,
/// containing a column [top spacer, row [left spacer, boat svg]]. Spacers
/// position the boat at `(target_x, target_y)`; the outer container scissors
/// any overflow.
///
/// Why this and not `Float`: `iced::widget::Float` renders translated content
/// via an overlay layer (`reference-iced/widget/src/float.rs:204-244`) that
/// calls `renderer.with_layer(self.viewport, ...)` with the **full window
/// viewport** — so a parent `container.clip(true)` is silently ignored and
/// the boat draws over neighbouring overlays (the player bar). Positioning
/// the boat as a normal in-flow widget lets the parent clip actually take
/// effect, the same way the lines/bars shader respects its scissor rect.
pub(crate) fn boat_overlay<'a, M: 'a>(
    state: &BoatState,
    area_width: f32,
    area_height: f32,
) -> Element<'a, M> {
    // The handler is responsible for calling `ensure_handle()` on the first
    // visible tick, so by the time we render the handle is cached. The
    // fallback here keeps the contract robust if a render somehow precedes
    // the tick OR if the theme just changed and the next BoatTick hasn't
    // refreshed the cache yet — in that case the cached handle's generation
    // won't match `theme::theme_generation()` and we rebuild inline rather
    // than ship a stale-color frame.
    let current_gen = crate::theme::theme_generation();
    let cached = state
        .handle
        .clone()
        .filter(|_| state.handle_generation == current_gen);
    let handle = cached.unwrap_or_else(|| {
        svg::Handle::from_memory(crate::embedded_svg::themed_logo_svg().into_bytes())
    });
    let boat_h = (area_height * BOAT_HEIGHT_FRACTION).max(8.0);
    let boat_w = boat_h * BOAT_ASPECT_RATIO;

    // Pixel offsets within the visualizer area. The waterline is
    // `(1 - y_ratio) * area_height` from the top (visualizer draws upward
    // from the bottom). `BOAT_SINK_FRACTION` of the boat's height sits below
    // the waterline; the rest sits above. Spacers can't take negative sizes,
    // so we clamp at 0 — overflow above is then handled by the clip on the
    // outer container, mirroring the bottom edge.
    let cx = state.x_ratio * area_width;
    let target_x = (cx - boat_w * 0.5).max(0.0);
    let line_y = area_height * (1.0 - state.y_ratio);
    let target_y = (line_y - boat_h + boat_h * BOAT_SINK_FRACTION).max(0.0);

    let boat_svg = container(Svg::new(handle).width(Length::Fill).height(Length::Fill))
        .width(Length::Fixed(boat_w))
        .height(Length::Fixed(boat_h));

    container(column![
        Space::new().height(Length::Fixed(target_y)),
        row![Space::new().width(Length::Fixed(target_x)), boat_svg],
    ])
    .width(Length::Fixed(area_width))
    .height(Length::Fixed(area_height))
    .clip(true)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- handle caching --------------------------------------------------------

    /// Sequential guard. The handle-cache tests poke `theme::set_light_mode`
    /// (a global atomic), so they must not run interleaved with each other.
    /// `cargo test` is multi-threaded by default — this mutex serializes the
    /// whole group without forcing the entire suite to single-threaded mode.
    /// `parking_lot::Mutex` is used because a test panic in one test would
    /// otherwise poison the std lock and cascade-fail the rest of the group.
    static THEME_MUTATION_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

    /// Theme-change behavior: when the active palette changes, the next
    /// `ensure_handle()` must return a handle whose bytes (and therefore
    /// `id()`) reflect the new colors. Without a generation check this is a
    /// stale-cache bug — the user changes themes and the boat keeps showing
    /// the old palette until restart.
    #[test]
    fn ensure_handle_rebuilds_when_active_theme_changes() {
        let _guard = THEME_MUTATION_LOCK.lock();

        let mut state = BoatState::default();
        let initial_mode = crate::theme::is_light_mode();

        let id_before = state.ensure_handle().id();

        // Flip light/dark — `themed_logo_svg()` now substitutes different
        // colors, so a freshly-built handle has different bytes (and id).
        crate::theme::set_light_mode(!initial_mode);

        let id_after = state.ensure_handle().id();

        // Restore before any assertion fires so a panic still leaves global
        // state clean for other tests in this group.
        crate::theme::set_light_mode(initial_mode);

        assert_ne!(
            id_before, id_after,
            "ensure_handle must rebuild after a theme/mode change \
             (got id_before = id_after = {id_before}, meaning stale cache)"
        );
    }

    /// No theme change: the cache should be reused. Guards the optimization
    /// the whole point of the cache exists for — without it we'd churn
    /// GPU cache keys on every frame (the gotcha called out in this module's
    /// doc comment).
    #[test]
    fn ensure_handle_returns_cached_when_theme_unchanged() {
        let _guard = THEME_MUTATION_LOCK.lock();

        let mut state = BoatState::default();
        let id1 = state.ensure_handle().id();
        let id2 = state.ensure_handle().id();
        assert_eq!(
            id1, id2,
            "two consecutive ensure_handle calls without a theme change \
             must return the same cached handle"
        );
    }

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
    fn step_wall_bumper_prevents_edge_pinning() {
        // Waveform that would normally pin the boat at the right wall: bars
        // are tall everywhere except the last few slots, so slope sampling
        // near x = 1.0 sees a steep negative slope → strong rightward force.
        // Suppress the captain charge so this test isolates the bumper.
        let bars: Vec<f64> = (0..32).map(|i| if i >= 30 { 0.05 } else { 0.95 }).collect();
        let mut state = BoatState {
            x_ratio: 1.0,
            x_velocity: 0.0,
            // Pre-seed rng + push next charge way out so the lazy-init branch
            // doesn't reschedule mid-test.
            rng_state: 0x12345,
            secs_until_next_charge: 1e6,
            ..Default::default()
        };

        // Settle for 30 s. The bumper should evict the boat from the wall
        // and keep it outside the wall zone.
        let dt = Duration::from_millis(16);
        for _ in 0..(30 * 60) {
            step(&mut state, dt, &bars, false);
        }

        let zone_inner = 1.0 - WALL_ZONE * 0.5;
        assert!(
            state.x_ratio < zone_inner,
            "wall bumper should keep boat off right wall (got x_ratio = {}, expected < {})",
            state.x_ratio,
            zone_inner
        );
    }

    #[test]
    fn step_charge_eventually_fires() {
        // Simulate 60 s of flat-bar physics from rest. The first charge must
        // fire well within `CHARGE_INTERVAL_MAX_SECS` of 30 s.
        let bars = vec![0.5; 16];
        let mut state = BoatState {
            x_ratio: 0.5,
            ..Default::default()
        };
        let dt = Duration::from_millis(16);
        let mut fired_at: Option<f32> = None;
        for tick in 0..(60 * 60) {
            step(&mut state, dt, &bars, false);
            if state.charge_remaining_secs > 0.0 {
                fired_at = Some(tick as f32 * 0.016);
                break;
            }
        }
        let t = fired_at.expect("charge should fire within 60 s");
        assert!(
            t <= CHARGE_INTERVAL_MAX_SECS + 0.5,
            "first charge fired at {t}s, expected <= {CHARGE_INTERVAL_MAX_SECS}s",
        );
    }

    #[test]
    fn step_charge_overrides_slope_to_climb_uphill() {
        // Linear ramp upward to the right: slope force normally pushes the
        // boat *left*. Pre-arm a rightward charge and verify the boat moves
        // *right* — the slope-suppression-during-charge property the user
        // depends on.
        let bars: Vec<f64> = (0..32).map(|i| i as f64 / 31.0).collect();
        let mut state = BoatState {
            x_ratio: 0.5,
            charge_remaining_secs: 4.0,
            charge_direction: 1.0,
            // Pre-seed rng + push next charge far away so the lazy-init
            // branch doesn't reschedule mid-test.
            rng_state: 0x12345,
            secs_until_next_charge: 100.0,
            ..Default::default()
        };
        let dt = Duration::from_millis(16);
        for _ in 0..(3 * 60) {
            step(&mut state, dt, &bars, false);
        }
        assert!(
            state.x_ratio > 0.6,
            "rightward charge should overcome leftward slope force \
             (got x_ratio = {})",
            state.x_ratio
        );
    }

    #[test]
    fn pick_charge_direction_prefers_storm_side() {
        // Asymmetric bars: tall right half, calm left half. With the boat
        // near center, direction must come back as +1 regardless of rng seed.
        let bars: Vec<f64> = (0..32).map(|i| if i > 16 { 0.8 } else { 0.1 }).collect();
        for seed in [1, 2, 3, 999, 0x9E37_79B9] {
            let mut rng = seed;
            assert_eq!(
                pick_charge_direction(&bars, 0.5, &mut rng),
                1.0,
                "should pick right (storm) for seed {seed}"
            );
        }

        // Mirrored: tall left half. Must pick -1.
        let bars: Vec<f64> = (0..32).map(|i| if i < 15 { 0.8 } else { 0.1 }).collect();
        for seed in [1, 2, 3, 999, 0x9E37_79B9] {
            let mut rng = seed;
            assert_eq!(
                pick_charge_direction(&bars, 0.5, &mut rng),
                -1.0,
                "should pick left (storm) for seed {seed}"
            );
        }
    }

    #[test]
    fn pick_charge_direction_returns_to_center_when_off_center() {
        // Storm is on the right but the boat has already drifted into the
        // right side: captain must override the storm-bias and head back
        // toward center.
        let storm_right: Vec<f64> = (0..32).map(|i| if i > 16 { 0.8 } else { 0.1 }).collect();
        let mut rng = 0x9E37_79B9;
        assert_eq!(
            pick_charge_direction(&storm_right, 0.85, &mut rng),
            -1.0,
            "boat already on right side should charge left even toward storm"
        );

        let storm_left: Vec<f64> = (0..32).map(|i| if i < 15 { 0.8 } else { 0.1 }).collect();
        let mut rng = 0x9E37_79B9;
        assert_eq!(
            pick_charge_direction(&storm_left, 0.15, &mut rng),
            1.0,
            "boat already on left side should charge right even toward storm"
        );
    }

    #[test]
    fn pick_charge_direction_random_on_symmetric_waveform() {
        // Flat bars + boat near center: imbalance is 0 and the off-center
        // override doesn't fire, so we land in the rng coin-flip branch.
        // Both directions must be reachable across different seeds. Use
        // production-shaped seeds (xorshift's first output is degenerate
        // for tiny seeds, so seeding from a single counter would hit one
        // side only — irrelevant in real use where the seed is the golden
        // ratio constant).
        let bars = vec![0.5; 16];
        let mut saw_left = false;
        let mut saw_right = false;
        for i in 0u32..200 {
            let mut rng = 0x9E37_79B9_u32.wrapping_add(i.wrapping_mul(0x6789_ABCD));
            match pick_charge_direction(&bars, 0.5, &mut rng) {
                d if d < 0.0 => saw_left = true,
                d if d > 0.0 => saw_right = true,
                _ => {}
            }
            if saw_left && saw_right {
                break;
            }
        }
        assert!(
            saw_left && saw_right,
            "coin flip must reach both directions"
        );
    }

    #[test]
    fn step_slope_suppressed_inside_wall_zone() {
        // Steep upward ramp: outside the wall zone the boat would build a
        // strong leftward velocity from slope force. Inside the wall zone
        // (x_ratio = 0.05) the slope must be zeroed, so velocity comes from
        // the inward wall bumper instead.
        let bars: Vec<f64> = (0..16).map(|i| i as f64 / 15.0).collect();
        let mut state = BoatState {
            x_ratio: 0.05,
            phase: 0.0,
            // Pre-seed rng + push next charge way out so this test isolates
            // the slope-suppression branch.
            rng_state: 0x12345,
            secs_until_next_charge: 1e6,
            ..Default::default()
        };

        // One short tick: if slope had fired (~ -0.04 leftward) it would
        // dominate the small wall force at this depth and produce negative
        // velocity. Instead we expect rightward velocity from the bumper.
        step(&mut state, Duration::from_millis(16), &bars, false);

        assert!(
            state.x_velocity > 0.0,
            "slope must be suppressed inside wall zone — velocity should be \
             positive (rightward) from wall bumper, got {}",
            state.x_velocity
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

    #[test]
    fn sample_line_height_clamps_overshoot_to_unit_range() {
        // Mirror the lines shader's `clamp(value, 0.0, 1.0)`. Catmull-Rom
        // extrapolates outside the control-point range when neighbours
        // are sharply peaked — without an explicit clamp the boat would
        // ride above where the wave can actually be drawn.
        let bars = vec![0.0, 1.0, 1.0, 0.0];
        let v = sample_line_height(&bars, 0.5, false);
        assert!(
            (0.0..=1.0).contains(&v),
            "Catmull-Rom overshoot above 1.0 must clamp to 1.0 (got {v})"
        );

        // Negative extrapolation past low control points must also clamp
        // to 0 (matches the shader at the bottom edge).
        let bars = vec![1.0, 0.0, 0.0, 1.0];
        let v = sample_line_height(&bars, 0.5, false);
        assert!(
            (0.0..=1.0).contains(&v),
            "Catmull-Rom undershoot below 0.0 must clamp to 0.0 (got {v})"
        );

        // Raw bar overshoot above 1.0 (e.g., spectrum-engine transient
        // before auto-sensitivity adapts) must also clamp.
        let bars = vec![1.5; 8];
        let v = sample_line_height(&bars, 0.5, false);
        assert!(v <= 1.0, "raw bar value > 1.0 must clamp to 1.0 (got {v})");
    }

    // --- effective_bars (playing-state gate) -----------------------------------

    #[test]
    fn effective_bars_passes_raw_through_when_playing() {
        let raw = [0.4, 0.7, 0.9];
        let got = effective_bars(true, &raw);
        assert_eq!(
            got,
            &raw[..],
            "playing must hand the raw bar buffer to the boat unchanged"
        );
    }

    #[test]
    fn effective_bars_drops_raw_when_not_playing() {
        // Silence override: the visualizer's bar buffer can stay frozen
        // at non-zero values when audio isn't producing samples (the FFT
        // thread's gravity-falloff path requires a full sample chunk to
        // run). Returning empty bars makes sample_line_height report 0,
        // which lets the spring pull the boat to the bottom instead of
        // tracking stale data.
        let raw = [0.4, 0.7, 0.9];
        let got = effective_bars(false, &raw);
        assert!(
            got.is_empty(),
            "not-playing must produce empty bars regardless of input \
             (got len {})",
            got.len()
        );
    }

    #[test]
    fn step_with_silence_override_decays_y_ratio_to_bottom() {
        // End-to-end of the silence path: bars are frozen high (last
        // loud chunk before audio stopped), playback is not playing →
        // effective_bars() returns empty → step() targets y_ratio=0 →
        // the spring decays toward 0. Without the gate the boat would
        // settle near 0.8 (the frozen bar height).
        let raw = vec![0.8; 32];
        let mut state = BoatState {
            y_ratio: 0.9,
            ..Default::default()
        };
        let dt = Duration::from_millis(16);

        for _ in 0..120 {
            // 120 ticks = ~2 s
            let bars = effective_bars(false, &raw);
            step(&mut state, dt, bars, false);
        }

        assert!(
            state.y_ratio < 0.05,
            "boat must sink to ~0 within 2 s when not-playing despite frozen-high \
             raw bars (y_ratio = {})",
            state.y_ratio
        );
    }

    // --- y-velocity edge clamping ---------------------------------------------

    #[test]
    fn step_zeros_outward_y_velocity_at_top_clamp() {
        // Mirror the x-axis edge-clamp pattern: when y_ratio hits 1.0
        // with positive (outward) velocity, that velocity must be zeroed
        // so the boat can drop the moment the target lowers — instead
        // of holding upward momentum and sticking at the top until
        // damping eats it.
        let bars = vec![1.0; 16];
        let mut state = BoatState {
            y_ratio: 0.99,
            y_velocity: 1.0,
            ..Default::default()
        };

        step(&mut state, Duration::from_millis(16), &bars, false);

        assert_eq!(state.y_ratio, 1.0, "y_ratio must clamp at top");
        assert!(
            state.y_velocity <= 0.0,
            "outward (positive) y_velocity must be zeroed once y_ratio \
             reaches 1.0 (got {})",
            state.y_velocity
        );
    }

    #[test]
    fn step_zeros_outward_y_velocity_at_bottom_clamp() {
        let bars = vec![0.0; 16];
        let mut state = BoatState {
            y_ratio: 0.01,
            y_velocity: -1.0,
            ..Default::default()
        };

        step(&mut state, Duration::from_millis(16), &bars, false);

        assert_eq!(state.y_ratio, 0.0, "y_ratio must clamp at bottom");
        assert!(
            state.y_velocity >= 0.0,
            "outward (negative) y_velocity must be zeroed once y_ratio \
             reaches 0.0 (got {})",
            state.y_velocity
        );
    }

    #[test]
    fn step_preserves_inward_y_velocity_at_clamp() {
        // The edge-clamp zeroing must only apply to *outward* velocity —
        // inward velocity (pulling the boat back into [0,1]) must pass
        // through, otherwise the spring loses its restoring kick.
        let bars = vec![0.5; 16];
        let mut state = BoatState {
            y_ratio: 1.0,
            y_velocity: -0.5, // inward (downward at top)
            ..Default::default()
        };

        step(&mut state, Duration::from_millis(16), &bars, false);

        assert!(
            state.y_velocity < 0.0,
            "inward (negative) y_velocity at top must be preserved — \
             only outward velocity is zeroed (got {})",
            state.y_velocity
        );
    }
}
