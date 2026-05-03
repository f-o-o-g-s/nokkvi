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
//!   `MAX_SLOPE_FORCE` so a single tall bar can't dominate the sum.
//! - **Velocity damping** — friction on `x_velocity` gives the "floating"
//!   feel; the boat lags fast wave changes instead of snapping to them.
//! - **Captain charge** — every `CHARGE_INTERVAL_*` seconds (randomized) the
//!   captain decides to row in a chosen direction for `CHARGE_DURATION_*`
//!   seconds. Force traces a half-sine envelope over the charge
//!   (`CHARGE_FORCE * sin(π · progress)`), so the captain ramps up,
//!   peaks mid-stroke, and tapers off — a step function felt motorized.
//!   Slope force is blended at `CHARGE_SLOPE_BLEND` rather than fully
//!   suppressed, so the boat still feels wave faces while rowing
//!   ("battling the waves") instead of plowing through indifferent to
//!   them. Direction is a fair coin flip — earlier revisions biased it
//!   toward the louder half of the spectrum, but on a torus that
//!   systematically favored one wrap direction for any asymmetric track.
//! - **Toroidal X wrap with off-screen margin** — `x_ratio` lives in
//!   `[-x_wrap_margin, 1 + x_wrap_margin)` and wraps via `rem_euclid` over
//!   that extended span; `x_velocity` is preserved across the seam. The
//!   margin is sized in the handler from the live boat sprite width
//!   (`BOAT_WRAP_MARGIN_BOAT_WIDTHS · boat_w / area_width`) so the boat
//!   fully exits the visible area before wrapping — the renderer draws a
//!   single copy at `target_x` and lets the outer clip trim the off-screen
//!   portion, so the boat is never visible in two places at once. The
//!   off-screen stretch also gives the captain a quiet zone to charge
//!   through stuck regions where the visible wave face would otherwise
//!   pin the boat. (Earlier revisions used a quadratic wall-repulsion
//!   spring plus a clamp before any wrap; that behavior — and its
//!   slope-suppression / center-return charge bias — is gone.)
//! - **Y dynamics** — `y_ratio` follows the sampled wave height through a
//!   spring-damper rather than tracking it exactly, so the boat bobs with
//!   buoyancy rather than glued to the curve.
//!
//! `BoatState.handle` is built lazily on first use from
//! `embedded_svg::themed_logo_svg()` and reused thereafter. `Handle::from_memory`
//! re-hashes input bytes per call (see `reference-iced/core/src/svg.rs:89`),
//! so per-frame construction would churn GPU cache keys — the same class of
//! bug as the `image::Handle::from_path` gotcha called out in `CLAUDE.md`.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use iced::{
    Element, Event, Length, Point, Rectangle, Size, Vector,
    advanced::{
        Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{Operation, Tree},
    },
    widget::{Stack, Svg, container, svg},
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

/// Off-screen travel beyond each visible edge, expressed as a multiple of
/// the boat sprite width. The wrap zone on each side spans this many boat
/// widths in pixels — sized so the sprite (centered at `x_ratio = 1`,
/// half-overlapping the right edge) clears the visible area entirely (after
/// `0.5 · boat_w` of travel) and gets ~1.25 boat widths of fully off-screen
/// drift before wrapping. That hidden stretch eliminates the dual-render
/// at the seam and gives the captain time to charge through stuck regions
/// without the visible wave face dragging the boat back.
pub(crate) const BOAT_WRAP_MARGIN_BOAT_WIDTHS: f32 = 1.75;

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
/// overpower the rest of the force budget. Without this the boat could
/// briefly run at maximum velocity in one direction whenever a sharp
/// transient lined up under it.
const MAX_SLOPE_FORCE: f32 = 0.10;

/// Local-height threshold below which the slope force is fully suppressed.
/// "No surfing in calm water": when the wave under the boat is essentially
/// flat (`target_y < FLOOR`), `downhill` has no physical meaning and the
/// gradient at the foot of the basin would otherwise drag the boat further
/// into low-energy edge regions.
const SLOPE_GATE_FLOOR: f32 = 0.05;

/// Width of the linear ramp from "fully suppressed" to "full slope force".
/// At `target_y = FLOOR + RAMP` the gate is 1.0 (full surf force); between
/// FLOOR and FLOOR + RAMP it lerps. Tuned so anything above ~25% of the
/// visualizer height surfs at full force, while the bottom ~5% is dead
/// zone — covers the V-basin at the seam without flattening surfing on
/// real wave faces.
const SLOPE_GATE_RAMP: f32 = 0.20;

/// Min/max delay between captain charges. The actual interval is sampled
/// uniformly in `[MIN, MAX]` after each charge ends (and at first activation),
/// so charges feel scheduled by mood rather than clockwork.
const CHARGE_INTERVAL_MIN_SECS: f32 = 12.0;
const CHARGE_INTERVAL_MAX_SECS: f32 = 30.0;

/// Min/max duration of a single charge. Sampled uniformly in `[MIN, MAX]`
/// when a charge starts. Tuned so a charge covers a notable but bounded
/// fraction of the visualizer — under the half-sine envelope (average
/// thrust ≈ 64% of peak) a 6 s charge crosses roughly a third of the
/// screen, which reads as deliberate rowing rather than traversing the
/// whole ocean.
const CHARGE_DURATION_MIN_SECS: f32 = 4.0;
const CHARGE_DURATION_MAX_SECS: f32 = 8.0;

/// Peak horizontal force during a charge. The actual instantaneous force
/// is `CHARGE_FORCE · sin(π · progress)` where `progress = elapsed /
/// total ∈ [0, 1]` — half-sine envelope, zero at the endpoints, peak
/// mid-stroke. Average force across a charge is `(2/π) · CHARGE_FORCE
/// ≈ 0.038`, well below the peak. Peak terminal velocity
/// (`CHARGE_FORCE / X_DAMPING ≈ 0.067 ratio/sec`) is preserved at the
/// natural rower's mid-stroke strength; average distance covered drops
/// because the captain spends time ramping in and out instead of holding
/// peak thrust.
const CHARGE_FORCE: f32 = 0.06;

/// Fraction of the raw slope force that remains active during a charge.
/// Earlier revisions zeroed slope entirely so the captain could escape
/// any basin no matter how deep — but the height gate
/// (`SLOPE_GATE_FLOOR/RAMP`) already prevents low-amplitude basins from
/// pinning the boat, so full suppression is no longer needed for trap
/// escape. Blending at 50% gives the "battling the waves" feel: a steep
/// wave face still pushes the boat around while the captain rows with
/// intent. Trade-off: charges no longer dominate slope outright, just
/// make net progress against it.
const CHARGE_SLOPE_BLEND: f32 = 0.5;

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

/// Conversion from sampled wave slope to target tilt angle (radians per
/// unit slope). A typical wave gradient is in the `0.5..2.0` range, so a
/// gain of `0.18` lands the boat around `5°..20°` of lean before the cap.
const TILT_GAIN: f32 = 0.18;

/// Hard cap on `|tilt|`. ~17° feels like the boat is genuinely committed
/// to the wave without flipping over. Scales the visible tilt range
/// independently of how aggressive `TILT_GAIN` is.
const MAX_TILT: f32 = 0.30;

/// Spring constant for `tilt` tracking `target_tilt`. Higher = boat snaps
/// to the slope; lower = the lean lags so visible motion is buoyant. Tuned
/// alongside `TILT_DAMPING` for an underdamped feel on quick wave changes.
const TILT_SPRING_K: f32 = 60.0;

/// Damping on `tilt_velocity`. With `TILT_SPRING_K = 60` and damping `10`
/// the damping ratio ζ ≈ 0.65 — same family as Y dynamics: slightly
/// underdamped, a sharp wave produces a small overshoot before settling.
const TILT_DAMPING: f32 = 10.0;

/// Tilt quantization step in degrees. The boat SVG is re-baked with the
/// rotation embedded in its path data each time the quantized angle
/// changes (option A in the SVG-aliasing investigation), so finer
/// quantization gives smoother visible motion at the cost of more cache
/// entries. With `MAX_TILT ≈ 17°` and 0.5° steps that's ~70 entries per
/// facing × theme combo, which fits comfortably in iced's bitmap atlas.
const TILT_QUANT_DEG: f32 = 0.5;

/// Minimum `|x_velocity|` (ratio/sec) needed to flip the boat's facing.
/// Below this, `facing` holds whatever it was, so the brief sign-flips
/// during charge handover (slope reasserting itself when a charge ends)
/// don't make the sail jitter back and forth. ~0.03 ratio/sec is roughly
/// half the captain's terminal velocity, well above the drive oscillator's
/// near-zero average drift.
const FLIP_THRESHOLD: f32 = 0.03;

/// Per-frame UI-thread state for the surfing boat.
///
/// - `phase` is in `[0, 1)` and ticks linearly with time. Drives the slow
///   sine that biases horizontal motion left vs right.
/// - `x_ratio` / `y_ratio` are the boat's normalized position in `[0, 1]`,
///   integrated from the velocity fields below.
/// - `x_velocity` / `y_velocity` are persisted across ticks so the physics
///   has memory (inertia → floating feel).
/// - `tilt` is the boat's current rotation in radians (positive =
///   counterclockwise = bow up when the boat is sailing rightward up a
///   slope). `tilt_velocity` is the spring-damper rate; together they
///   ease the boat into the slope so it doesn't snap to spectrum jitter.
/// - `facing` is `+1.0` (sailing right, sail catches wind from the left)
///   or `-1.0` (sailing left, sail mirrored). Updated with hysteresis
///   from `x_velocity` — see `FLIP_THRESHOLD`.
/// - `visible` is derived per tick by the handler — it is *not* the user's
///   on/off toggle (that lives in `LinesConfig.boat`).
/// - `last_tick` is consumed to compute `dt` between ticks; cleared when the
///   boat is hidden so the first frame back doesn't see a stale gap.
/// - `secs_until_next_charge` counts down to the next captain charge; only
///   meaningful while `charge_remaining_secs == 0`.
/// - `charge_remaining_secs` is the time left in the current charge; > 0
///   means the captain is actively rowing in `charge_direction`.
/// - `charge_total_secs` is the duration set when the current charge began;
///   together with `charge_remaining_secs` it defines `progress` for the
///   half-sine envelope. Stays at the previous charge's value between
///   charges (harmless — only read while `charge_remaining_secs > 0`).
/// - `charge_direction` is `+1.0` (right) or `-1.0` (left); set when a
///   charge begins, unread otherwise.
/// - `rng_state` seeds a tiny xorshift PRNG used for charge timing and
///   direction. Lazily seeded on first tick (0 → seed constant) so `Default`
///   stays clean and the schedule starts on first activation, not on
///   construction.
/// - `tilt_handles` caches the themed boat SVG keyed by quantized tilt
///   angle and facing — see `cache_handle_for`. Because the rotation is
///   baked into the SVG path data (rather than rotating an
///   already-rasterized bitmap in the wgpu shader), we want one handle
///   per visibly-distinct orientation. With a 0.5° quantization step and
///   a ±17° tilt range, that's ~140 entries at the worst case — ~3 KB
///   each in iced's bitmap atlas, so ~400 KB worst-case footprint per
///   theme.
/// - `handle_generation` is the `theme::theme_generation()` snapshot taken
///   at the time the cache was last populated. When the global counter
///   advances — any path that runs `theme::reload_theme()` or
///   `theme::set_light_mode()` — the cache is recognized as stale and
///   cleared on the next access. This replaces the previous explicit-
///   invalidation approach, which had to be wired up at every
///   theme-change call site and missed preset switches.
#[derive(Debug, Clone, Default)]
pub struct BoatState {
    pub phase: f32,
    pub x_ratio: f32,
    pub y_ratio: f32,
    pub x_velocity: f32,
    pub y_velocity: f32,
    pub tilt: f32,
    pub tilt_velocity: f32,
    pub facing: f32,
    pub visible: bool,
    pub last_tick: Option<Instant>,
    pub secs_until_next_charge: f32,
    pub charge_remaining_secs: f32,
    pub charge_total_secs: f32,
    pub charge_direction: f32,
    pub rng_state: u32,
    /// Half-width of the off-screen wrap margin in `x_ratio` units. Set by
    /// the boat-tick handler from the current boat sprite width and
    /// visualizer area width — see `BOAT_WRAP_MARGIN_BOAT_WIDTHS`. `step()`
    /// wraps `x_ratio` over `[-x_wrap_margin, 1 + x_wrap_margin)`. Default
    /// of `0.0` reproduces the legacy `rem_euclid(1.0)` behavior, which is
    /// what the in-crate physics tests rely on (they construct `BoatState`
    /// directly without going through the handler).
    pub x_wrap_margin: f32,
    pub tilt_handles: HashMap<(i16, bool), svg::Handle>,
    pub handle_generation: u64,
}

/// Snap a tilt angle (radians) to its quantized cache index. The index is
/// `round(tilt_degrees / TILT_QUANT_DEG)` clamped into `i16`, which
/// trivially fits the `±MAX_TILT ≈ ±17°` range with 0.5° steps.
fn quantize_tilt(tilt: f32) -> i16 {
    let degrees = tilt.to_degrees();
    (degrees / TILT_QUANT_DEG).round() as i16
}

/// Inverse of `quantize_tilt`: convert a cache index back to the radians
/// the SVG was baked at.
fn dequantize_tilt(idx: i16) -> f32 {
    (idx as f32 * TILT_QUANT_DEG).to_radians()
}

impl BoatState {
    /// Drop all cached SVG handles if the active theme has advanced since
    /// they were built. Called by `cache_handle_for` before any insert, so
    /// the cache stays consistent with `theme::theme_generation()` without
    /// needing explicit invalidation hooks at every theme-change site
    /// (preset switch, color picker edit, restore-defaults, etc.).
    fn clear_if_theme_changed(&mut self) {
        let current_gen = crate::theme::theme_generation();
        if self.handle_generation != current_gen {
            self.tilt_handles.clear();
            self.handle_generation = current_gen;
        }
    }

    /// Build (and cache) the boat SVG handle for the given tilt + facing,
    /// returning a clone of the cached handle. The tilt is quantized to
    /// `TILT_QUANT_DEG`-degree steps so the cache stays bounded; the
    /// requested radians are dequantized back before being baked into the
    /// SVG, so the handle's rotation is exactly what the cache key
    /// represents (no per-frame drift).
    pub(crate) fn cache_handle_for(&mut self, tilt: f32, facing: f32) -> svg::Handle {
        self.clear_if_theme_changed();
        let key = (quantize_tilt(tilt), facing < 0.0);
        if let Some(h) = self.tilt_handles.get(&key) {
            return h.clone();
        }
        let bytes =
            crate::embedded_svg::themed_boat_svg(dequantize_tilt(key.0), key.1).into_bytes();
        let h = svg::Handle::from_memory(bytes);
        self.tilt_handles.insert(key, h.clone());
        h
    }

    /// Look up a cached handle for the given tilt + facing without
    /// mutating state. Returns `None` when the theme has advanced past
    /// the cache, when nothing is cached yet for this orientation, or
    /// when the boat hasn't ticked since `Default`. The render path uses
    /// this and falls back to an inline rebuild on miss.
    pub(crate) fn cached_handle_for(&self, tilt: f32, facing: f32) -> Option<svg::Handle> {
        let current_gen = crate::theme::theme_generation();
        if self.handle_generation != current_gen {
            return None;
        }
        let key = (quantize_tilt(tilt), facing < 0.0);
        self.tilt_handles.get(&key).cloned()
    }
}

/// Step the boat physics forward by `dt`, sampling slope and target height
/// from `bars`. Mutates `phase`, `x_velocity`, `x_ratio`, `y_velocity`,
/// `y_ratio`, the charge-state fields, and `rng_state` on `state`.
///
/// Forces on `x` (semi-implicit Euler):
/// - drive: `sin(2π·phase) · DRIVE_FORCE` — slow rhythmic push
/// - slope: `(-slope · SLOPE_GAIN · surf_gate).clamp(±MAX_SLOPE_FORCE)` —
///   surf downhill, capped so a single tall bar can't dominate. The
///   `surf_gate` (lerped over `SLOPE_GATE_FLOOR` → `+RAMP`) zeroes the
///   force in low-amplitude regions so the boat doesn't surf calm
///   water. Multiplied by `CHARGE_SLOPE_BLEND` while a charge is active
///   — captain still feels wave faces but isn't yanked off course.
/// - damping: `-x_velocity · X_DAMPING` — friction
/// - charge: `charge_direction · CHARGE_FORCE · sin(π · progress)` while
///   a charge is active, else 0. Half-sine envelope ramps the captain's
///   effort in and out — a step function felt motorized. Charges fire
///   on a randomized timer with a fair-coin direction.
///
/// There is no centering / restoring force. A spring toward `x = 0.5` made
/// sense when the boat could get pinned against an edge, but on a torus
/// "the middle" isn't geometrically privileged, and a one-sided pull
/// systematically favored whichever wrap the slope happened to be biasing
/// toward.
///
/// Y is a spring-damper tracking `target_y = sample_line_height(...)`:
/// `ay = (target_y - y) · Y_SPRING_K - y_velocity · Y_DAMPING`. Y dynamics
/// are unchanged during a charge — the boat still bobs over the waves it
/// crosses.
///
/// `tilt` is a spring-damper tracking `target_tilt = (slope · TILT_GAIN)
/// .clamp(±MAX_TILT)` (`a_tilt = (target - tilt) · TILT_SPRING_K -
/// tilt_velocity · TILT_DAMPING`). The sign is facing-independent: a
/// positive slope produces positive (counterclockwise) rotation, which
/// raises whichever end of the sprite is currently displayed on the right
/// — that's the bow when sailing rightward, the stern when sailing
/// leftward (mirrored sprite). So "going uphill = bow up" works for both
/// facings without needing to multiply by `facing` here.
///
/// `facing` flips between `+1` and `-1` based on `sign(x_velocity)`, but
/// only when `|x_velocity| > FLIP_THRESHOLD` so the sail doesn't twitch
/// during the brief velocity-zero crossings when a charge ends and the
/// slope force flips it back.
///
/// `x_ratio` is wrapped over `[-x_wrap_margin, 1 + x_wrap_margin)` so the
/// boat sails fully off one edge — through a hidden stretch sized to a
/// boat-and-a-bit — before reappearing on the opposite side with momentum
/// intact. `y_ratio` still clamps to `[0, 1]` (with outward velocity
/// zeroed) since the wave height is bounded — there's no toroidal Y to
/// wrap into.
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

    // Slope sampling wraps toroidally to match the boat's wrap geometry.
    // Without this, both samples near the seam clamp to the low-energy
    // edge bins (sub-bass on the left, top-treble on the right — quiet in
    // most music), forming a `\/` basin whose downhill slope force can
    // exceed `CHARGE_FORCE` and pin the boat at the seam until the next
    // captain charge fires. Wrapping reads the seam as flat when both
    // ends are low (slope ≈ 0), so drive + inertia carry the boat
    // through. Inside `(0, 1)` `rem_euclid` is a no-op, so mid-screen
    // behavior is unchanged. The "right-edge spectrum bleeds into the
    // left-edge gradient" concern noted earlier is visually irrelevant:
    // those bins are low-energy precisely because there's nothing
    // interesting to surf there.
    let h_left = sample_line_height(bars, (state.x_ratio - SLOPE_DX).rem_euclid(1.0), angular);
    let h_right = sample_line_height(bars, (state.x_ratio + SLOPE_DX).rem_euclid(1.0), angular);
    let slope = (h_right - h_left) / (2.0 * SLOPE_DX);

    // Gate the horizontal slope force on local wave height: full force on
    // visible wave faces, fading to zero in low-energy regions. Real
    // (asymmetric) music spectra still slope downhill toward the seam
    // even with toroidal sampling — the V-shape at the edges extends a
    // few percent inward — and that residual ramp was enough to keep the
    // boat hovering near the edges between captain charges. Tilt is left
    // ungated so the boat still leans naturally on whatever wisp of
    // curve it's on; only the horizontal pull is suppressed.
    let local_height = sample_line_height(bars, state.x_ratio, angular);
    let surf_gate = ((local_height - SLOPE_GATE_FLOOR) / SLOPE_GATE_RAMP).clamp(0.0, 1.0);
    let slope_force_raw =
        (-slope * SLOPE_GAIN * surf_gate).clamp(-MAX_SLOPE_FORCE, MAX_SLOPE_FORCE);

    // Charge state machine: either rowing (charge_remaining_secs > 0) or
    // counting down to the next charge. While rowing, force traces a
    // half-sine envelope (zero at start/end, peak mid-stroke) and slope
    // is blended at `CHARGE_SLOPE_BLEND` instead of fully suppressed —
    // so the boat still feels wave faces while the captain rows.
    let (charge_force, slope_force) = if state.charge_remaining_secs > 0.0 {
        state.charge_remaining_secs -= dt_secs;
        if state.charge_remaining_secs <= 0.0 {
            state.charge_remaining_secs = 0.0;
            let r = next_rand_unit(&mut state.rng_state);
            state.secs_until_next_charge =
                lerp(CHARGE_INTERVAL_MIN_SECS, CHARGE_INTERVAL_MAX_SECS, r);
        }
        let envelope = charge_envelope(state.charge_total_secs, state.charge_remaining_secs);
        (
            state.charge_direction * CHARGE_FORCE * envelope,
            slope_force_raw * CHARGE_SLOPE_BLEND,
        )
    } else {
        state.secs_until_next_charge -= dt_secs;
        if state.secs_until_next_charge <= 0.0 {
            state.charge_direction = pick_charge_direction(&mut state.rng_state);
            let r = next_rand_unit(&mut state.rng_state);
            let duration = lerp(CHARGE_DURATION_MIN_SECS, CHARGE_DURATION_MAX_SECS, r);
            state.charge_remaining_secs = duration;
            state.charge_total_secs = duration;
            // First tick of a brand-new charge: progress = 0, envelope = 0.
            // Force is zero this tick; the next tick's decrement advances
            // progress and the captain starts pulling. Slope is already on
            // the charge-blend regime since the charge is "active".
            (0.0, slope_force_raw * CHARGE_SLOPE_BLEND)
        } else {
            (0.0, slope_force_raw)
        }
    };

    let damping_force = -state.x_velocity * X_DAMPING;

    let ax = drive + slope_force + damping_force + charge_force;
    state.x_velocity = (state.x_velocity + ax * dt_secs).clamp(-MAX_X_V, MAX_X_V);
    // Wrap x_ratio over the extended span `[-x_wrap_margin, 1 + x_wrap_margin)`
    // so the boat slides fully off one edge before reappearing on the other.
    // Margin defaults to 0.0 (collapses to `rem_euclid(1.0)`), which is what
    // the standalone physics tests construct.
    let span = 1.0 + 2.0 * state.x_wrap_margin;
    let raw_x = state.x_ratio + state.x_velocity * dt_secs;
    state.x_ratio = (raw_x + state.x_wrap_margin).rem_euclid(span) - state.x_wrap_margin;

    // Tilt: spring-damper toward `-slope * gain`, clamped. Reuses `slope`
    // computed above (raw, before the during-charge suppression — tilt
    // tracks the wave the boat is on, not the captain's intent). The
    // sign is negated because the angle ultimately feeds an SVG
    // `rotate(deg, cx, cy)` transform, and SVG rotation is clockwise for
    // positive degrees in screen coords (Y-down); a positive slope means
    // uphill to the right, which we want the boat to lean *into* (right
    // side up = counterclockwise = negative angle). The hard `MAX_TILT`
    // clamp after the spring step is the same pattern as y_ratio uses:
    // the underdamped spring would otherwise overshoot the target by a
    // few percent on sharp transients, which would visually exceed the
    // cap.
    let target_tilt = (-slope * TILT_GAIN).clamp(-MAX_TILT, MAX_TILT);
    let a_tilt = (target_tilt - state.tilt) * TILT_SPRING_K - state.tilt_velocity * TILT_DAMPING;
    state.tilt_velocity += a_tilt * dt_secs;
    state.tilt += state.tilt_velocity * dt_secs;
    if state.tilt > MAX_TILT {
        state.tilt = MAX_TILT;
        if state.tilt_velocity > 0.0 {
            state.tilt_velocity = 0.0;
        }
    } else if state.tilt < -MAX_TILT {
        state.tilt = -MAX_TILT;
        if state.tilt_velocity < 0.0 {
            state.tilt_velocity = 0.0;
        }
    }

    // Facing: only update on supra-threshold velocity in a sign different
    // from the current facing. Initial facing of `0.0` (the `Default`)
    // also gets snapped to whichever side has any meaningful motion on
    // the first qualifying tick.
    if state.x_velocity.abs() > FLIP_THRESHOLD {
        let want = state.x_velocity.signum();
        if state.facing == 0.0 || state.facing != want {
            state.facing = want;
        }
    }

    // Reuses `local_height` from the slope-gate computation above —
    // same expression (`sample_line_height(bars, state.x_ratio, angular)`)
    // sampled at the boat's current x, so y dynamics and the surf gate
    // stay in lockstep without a second `sample_line_height` call.
    let ay = (local_height - state.y_ratio) * Y_SPRING_K - state.y_velocity * Y_DAMPING;
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

/// Half-sine envelope over a charge's progress: 0 at `progress = 0` and
/// `progress = 1`, peaks at 1 at `progress = 0.5`. Multiplied onto the
/// peak charge force so the captain ramps in, holds peak briefly, and
/// tapers off — the "step function" feel of constant thrust is the main
/// thing that read as motorized in earlier revisions.
///
/// `progress = 1 - remaining / total`. Returns 0 when `total <= 0` so a
/// `Default`-initialized `BoatState` (with `charge_total_secs == 0`)
/// doesn't divide-by-zero into NaN; the guard fires only when the field
/// is set without a corresponding charge start, which doesn't happen in
/// production.
fn charge_envelope(total_secs: f32, remaining_secs: f32) -> f32 {
    if total_secs <= 0.0 {
        return 0.0;
    }
    let progress = (1.0 - remaining_secs / total_secs).clamp(0.0, 1.0);
    (std::f32::consts::PI * progress).sin()
}

/// Pick a charge direction (`±1`) with a fair coin flip.
///
/// Earlier revisions biased the direction toward the louder half of the
/// spectrum (the "storm side") so the boat would seek out interesting
/// content, but on a torus that biased the wrap direction too: any
/// consistently-asymmetric track (most music — bass on the left,
/// declining toward treble on the right) ended up with the captain
/// always charging toward bass and the slope force always pushing away
/// from it, so the boat only ever wrapped one way.
fn pick_charge_direction(rng_state: &mut u32) -> f32 {
    if next_rand_unit(rng_state) < 0.5 {
        -1.0
    } else {
        1.0
    }
}

/// Position a child element at an arbitrary `(x, y)` (including negative
/// coordinates) inside a parent without shrinking the child.
///
/// `iced::widget::Pin` does almost the right thing — it accepts negative
/// coordinates and respects the parent clip — but it computes the child's
/// available layout space as `parent_max - position`, which silently
/// squashes a `Length::Fixed`-sized child as `position` approaches the
/// parent's far edge (`Length::Fixed(40)` with available `20` clamps to
/// `20` via `Limits::width()` at `core/src/layout/limits.rs:57`). For the
/// boat that produces a visible "shrinking ship" artifact at the wrap
/// seam. `OverflowPin` instead passes the parent's full limits through to
/// the child, then translates the laid-out node — the child keeps its
/// natural size and any portion that falls outside the parent is trimmed
/// by the ancestor clip in `draw()`.
struct OverflowPin<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer>
where
    Renderer: iced::advanced::Renderer,
{
    content: Element<'a, Message, Theme, Renderer>,
    position: Point,
}

impl<'a, Message, Theme, Renderer> OverflowPin<'a, Message, Theme, Renderer>
where
    Renderer: iced::advanced::Renderer,
{
    fn new(content: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        Self {
            content: content.into(),
            position: Point::ORIGIN,
        }
    }

    fn position(mut self, position: Point) -> Self {
        self.position = position;
        self
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for OverflowPin<'_, Message, Theme, Renderer>
where
    Renderer: iced::advanced::Renderer,
{
    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> iced::advanced::widget::tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }

    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Fill,
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let node = self
            .content
            .as_widget_mut()
            .layout(tree, renderer, limits)
            .move_to(self.position);

        let size = limits.resolve(Length::Fill, Length::Fill, node.size());
        layout::Node::with_children(size, vec![node])
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content.as_widget_mut().operate(
            tree,
            layout
                .children()
                .next()
                .expect("OverflowPin always lays out exactly one child"),
            renderer,
            operation,
        );
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            tree,
            event,
            layout
                .children()
                .next()
                .expect("OverflowPin always lays out exactly one child"),
            cursor,
            renderer,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            tree,
            layout
                .children()
                .next()
                .expect("OverflowPin always lays out exactly one child"),
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        if let Some(clipped_viewport) = bounds.intersection(viewport) {
            self.content.as_widget().draw(
                tree,
                renderer,
                theme,
                style,
                layout
                    .children()
                    .next()
                    .expect("OverflowPin always lays out exactly one child"),
                cursor,
                &clipped_viewport,
            );
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            tree,
            layout
                .children()
                .next()
                .expect("OverflowPin always lays out exactly one child"),
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message, Theme, Renderer> From<OverflowPin<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: iced::advanced::Renderer + 'a,
{
    fn from(p: OverflowPin<'a, Message, Theme, Renderer>) -> Self {
        Element::new(p)
    }
}

/// Build the boat overlay element. Returned as a plain `Element` (not
/// `Option<Element>`) so the visibility branch lives at the call site —
/// `Stack::push` expects `impl Into<Element>`.
///
/// `area_width` / `area_height` are the pixel dimensions of the visualizer
/// area the boat rides over. They size the outer clipping container and let
/// us compute the boat's pixel position from `(x_ratio, y_ratio)`.
///
/// Layout: a fixed-size `container.clip(true)` framing the visualizer area,
/// containing a single `OverflowPin`-positioned boat sprite at
/// `(target_x, target_y)`. `target_x` may extend past either edge by up to
/// `BOAT_WRAP_MARGIN_BOAT_WIDTHS · boat_w` — the physics in `step()` wrap
/// only after the boat has fully cleared the visible area, so a second
/// "ghost" copy at `target_x ± area_width` is unnecessary (and would put
/// the boat in two places at once during the off-screen drift). The outer
/// clip handles the fade-out as the sprite slides off; the next visible
/// frame on the opposite edge picks it up after the wrap fires.
///
/// Tilt and facing are read straight from `state`. The boat picks its
/// cached SVG handle via `cached_handle_for(tilt, facing)`, which returns
/// a handle whose path data has the rotation (and optional horizontal
/// mirror) *baked into the SVG itself*. resvg then rasterizes the
/// already-rotated paths at the boat's display resolution — much cleaner
/// than letting iced rasterize an upright sprite and then rotate the
/// bitmap in the wgpu shader, which aliases visibly at small sprite
/// sizes. The tilt is quantized to `TILT_QUANT_DEG`-degree steps so the
/// underlying cache stays bounded.
///
/// `OverflowPin` (defined just above) is used instead of `iced::widget::pin`
/// because the stock `Pin` shrinks `Length::Fixed`-sized content as the
/// position approaches the parent's far edge (silently squashing the
/// boat). `OverflowPin` lets the boat keep its real size and trims the
/// off-screen portion via the ancestor clip in its `draw()` path.
///
/// Why not `Float`: `iced::widget::Float` renders translated content via an
/// overlay layer (`reference-iced/widget/src/float.rs:204-244`) that calls
/// `renderer.with_layer(self.viewport, ...)` with the full window viewport,
/// so a parent `container.clip(true)` is silently ignored and the boat
/// would draw over neighbouring overlays (the player bar).
pub(crate) fn boat_overlay<'a, M: 'a>(
    state: &BoatState,
    area_width: f32,
    area_height: f32,
) -> Element<'a, M> {
    // The handler is responsible for calling `cache_handle_for(tilt,
    // facing)` on the first visible tick, so by the time we render the
    // matching handle is cached. The fallback rebuilds inline if a render
    // somehow precedes the tick OR if the theme just changed and the next
    // BoatTick hasn't refreshed the cache yet — in either case we ship a
    // fresh-rotation, fresh-color frame rather than a stale one.
    let handle = state
        .cached_handle_for(state.tilt, state.facing)
        .unwrap_or_else(|| {
            let bytes =
                crate::embedded_svg::themed_boat_svg(state.tilt, state.facing < 0.0).into_bytes();
            svg::Handle::from_memory(bytes)
        });
    let boat_h = (area_height * BOAT_HEIGHT_FRACTION).max(8.0);
    let boat_w = boat_h * BOAT_ASPECT_RATIO;

    // The boat SVG carries a padded viewBox so a `MAX_TILT` rotation
    // doesn't clip the rotated bounding box's corners. Scale the iced
    // container by the matching factor so the boat *content* still
    // renders at `boat_w × boat_h` pixels — `pad_x`/`pad_y` is the
    // half-padding the rotated corners can occupy on each side.
    let pad_factor = 1.0 + 2.0 * crate::embedded_svg::BOAT_VIEWBOX_PAD_FRACTION;
    let container_w = boat_w * pad_factor;
    let container_h = boat_h * pad_factor;
    let pad_x = (container_w - boat_w) * 0.5;
    let pad_y = (container_h - boat_h) * 0.5;

    // Pixel offsets within the visualizer area. The waterline is
    // `(1 - y_ratio) * area_height` from the top (visualizer draws upward
    // from the bottom). `BOAT_SINK_FRACTION` of the boat's height sits below
    // the waterline; the rest sits above. `OverflowPin` accepts negative
    // `target_x` directly, so we don't clamp it here — the outer clip
    // handles the off-screen portion. `target_y` keeps its `.max(0.0)`
    // because Y has no wrap (wave height is bounded), so the overlap-above
    // case really is just "nudge against the top edge".
    //
    // `target_x` / `target_y` describe where the boat *content* lands.
    // The container is shifted left/up by the half-padding so the content
    // remains at those coordinates regardless of the surrounding margin.
    let cx = state.x_ratio * area_width;
    let target_x = cx - boat_w * 0.5;
    let line_y = area_height * (1.0 - state.y_ratio);
    let target_y = (line_y - boat_h + boat_h * BOAT_SINK_FRACTION).max(0.0);

    let pin_at = |x: f32| {
        OverflowPin::new(
            container(
                Svg::new(handle.clone())
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .width(Length::Fixed(container_w))
            .height(Length::Fixed(container_h)),
        )
        .position(Point::new(x - pad_x, target_y - pad_y))
    };

    // Single sprite. The wrap zone in `step()` is sized so the boat fully
    // exits the visible area before reappearing on the opposite side, so
    // there is never a frame where two copies would be on screen at once
    // — outer clip handles the off-screen portion of the sprite as it
    // slides through the hidden stretch.
    let overlay = Stack::new().push(pin_at(target_x));

    container(overlay)
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
    /// `cache_handle_for` call must produce a handle whose bytes (and
    /// therefore `id()`) reflect the new colors. Without the generation
    /// check this is a stale-cache bug — the user changes themes and the
    /// boat keeps showing the old palette until restart.
    #[test]
    fn cache_handle_for_rebuilds_when_active_theme_changes() {
        let _guard = THEME_MUTATION_LOCK.lock();

        let mut state = BoatState::default();
        let initial_mode = crate::theme::is_light_mode();

        let id_before = state.cache_handle_for(0.0, 1.0).id();

        // Flip light/dark — `themed_boat_svg()` now substitutes different
        // colors, so a freshly-built handle has different bytes (and id).
        crate::theme::set_light_mode(!initial_mode);

        let id_after = state.cache_handle_for(0.0, 1.0).id();

        // Restore before any assertion fires so a panic still leaves global
        // state clean for other tests in this group.
        crate::theme::set_light_mode(initial_mode);

        assert_ne!(
            id_before, id_after,
            "handle must rebuild after a theme/mode change (got \
             id_before = id_after = {id_before}, meaning stale cache)"
        );
    }

    /// No theme change: the cache should be reused for the same quantized
    /// orientation. Guards the optimization the cache exists for —
    /// without it we'd re-run resvg on every frame at the same angle.
    #[test]
    fn cache_handle_for_returns_cached_when_theme_unchanged() {
        let _guard = THEME_MUTATION_LOCK.lock();

        let mut state = BoatState::default();
        let id1 = state.cache_handle_for(0.0, 1.0).id();
        let id2 = state.cache_handle_for(0.0, 1.0).id();
        assert_eq!(
            id1, id2,
            "two consecutive cache_handle_for calls at the same orientation \
             without a theme change must return the same cached handle"
        );
    }

    /// Different (tilt, facing) combinations must produce different
    /// handles — this is the whole reason for the cache key — and the
    /// cache must hold all of them simultaneously.
    #[test]
    fn cache_handle_for_returns_distinct_handles_per_orientation() {
        let _guard = THEME_MUTATION_LOCK.lock();

        let mut state = BoatState::default();
        let upright_right = state.cache_handle_for(0.0, 1.0).id();
        let upright_left = state.cache_handle_for(0.0, -1.0).id();
        let tilted_right = state.cache_handle_for(0.15, 1.0).id();
        let tilted_left = state.cache_handle_for(0.15, -1.0).id();

        assert_ne!(
            upright_right, upright_left,
            "facing flip must produce a distinct handle (mirror transform \
             changes the SVG bytes)"
        );
        assert_ne!(
            upright_right, tilted_right,
            "non-zero tilt must produce a distinct handle (rotation \
             transform changes the SVG bytes)"
        );
        assert_ne!(
            tilted_right, tilted_left,
            "tilt + facing combinations must each get their own handle"
        );

        // The cache should hold all four entries.
        assert_eq!(
            state.tilt_handles.len(),
            4,
            "every distinct orientation must occupy its own cache slot \
             (got {} entries)",
            state.tilt_handles.len()
        );
    }

    /// Tilt quantization: two angles within the same `TILT_QUANT_DEG`
    /// bucket must hit the same cache slot, so spring-damper jitter
    /// doesn't churn the cache.
    #[test]
    fn cache_handle_for_quantizes_close_angles_to_one_entry() {
        let _guard = THEME_MUTATION_LOCK.lock();

        let mut state = BoatState::default();
        // 0.001 rad ≈ 0.057°, well below the 0.5° quantization step.
        let id_a = state.cache_handle_for(0.000, 1.0).id();
        let id_b = state.cache_handle_for(0.001, 1.0).id();
        assert_eq!(
            id_a, id_b,
            "angles within one quantization step must collapse to the \
             same cache entry"
        );
        assert_eq!(
            state.tilt_handles.len(),
            1,
            "only one cache entry should have been allocated"
        );
    }

    /// Read-only `cached_handle_for` must report None when nothing is
    /// cached, and report the handle from `cache_handle_for` afterward.
    /// Render path relies on this: it falls back to a fresh inline build
    /// on miss, so the boat still draws correctly during the very first
    /// frame before the handler primes the cache.
    #[test]
    fn cached_handle_for_misses_then_hits_after_caching() {
        let _guard = THEME_MUTATION_LOCK.lock();

        let mut state = BoatState::default();
        assert!(
            state.cached_handle_for(0.0, 1.0).is_none(),
            "empty cache must miss for any orientation"
        );
        let primed = state.cache_handle_for(0.0, 1.0).id();
        let looked_up = state
            .cached_handle_for(0.0, 1.0)
            .expect("cache primed by cache_handle_for")
            .id();
        assert_eq!(
            primed, looked_up,
            "cached_handle_for must return the same handle that \
             cache_handle_for just inserted"
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
        // every direction. The toroidal wrap (`rem_euclid`) must keep
        // `x_ratio` in `[0, 1)` no matter how aggressively the slope and
        // velocity push past either edge.
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
            (0.0..1.0).contains(&final_state.x_ratio),
            "x_ratio must wrap into [0, 1) after every step (got {x})",
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
    fn step_wraps_x_ratio_off_right_edge() {
        // Boat near the right edge with strong rightward velocity: a single
        // tick should carry it past 1.0, where `rem_euclid` should drop it
        // back near 0.0 with momentum intact (no clamp, no zeroing).
        let bars = vec![0.5; 16];
        let mut state = BoatState {
            x_ratio: 0.99,
            x_velocity: MAX_X_V, // 0.15 ratio/sec
            // Suppress the captain so velocity changes come from physics only.
            rng_state: 0x12345,
            secs_until_next_charge: 1e6,
            ..Default::default()
        };

        // dt large enough that x_velocity * dt clearly exceeds (1.0 - 0.99).
        // 0.15 * 0.5 = 0.075 → unwrapped position would be ~1.065.
        step(&mut state, Duration::from_millis(500), &bars, false);

        assert!(
            state.x_ratio < 0.5,
            "boat should wrap from right edge to near 0 (got x_ratio = {})",
            state.x_ratio
        );
        assert!(
            state.x_velocity > 0.0,
            "rightward velocity must survive the wrap (got {})",
            state.x_velocity
        );
    }

    #[test]
    fn step_wraps_x_ratio_off_left_edge() {
        // Mirror of the right-edge case: boat near 0 with leftward velocity
        // wraps back near 1.0 with momentum intact.
        let bars = vec![0.5; 16];
        let mut state = BoatState {
            x_ratio: 0.01,
            x_velocity: -MAX_X_V,
            rng_state: 0x12345,
            secs_until_next_charge: 1e6,
            ..Default::default()
        };

        step(&mut state, Duration::from_millis(500), &bars, false);

        assert!(
            state.x_ratio > 0.5,
            "boat should wrap from left edge to near 1 (got x_ratio = {})",
            state.x_ratio
        );
        assert!(
            state.x_velocity < 0.0,
            "leftward velocity must survive the wrap (got {})",
            state.x_velocity
        );
    }

    #[test]
    fn step_wrap_does_not_zero_velocity_at_seam() {
        // Specifically guards the regression from the old hard-clamp behavior:
        // the previous code would zero `x_velocity` at the edge, which under
        // a wrap model would visibly stall the boat the moment it touched a
        // seam. Crossing the seam must leave `|x_velocity|` essentially
        // unchanged from one tick to the next.
        let bars = vec![0.5; 16];
        let mut state = BoatState {
            x_ratio: 0.999,
            x_velocity: 0.10,
            rng_state: 0x12345,
            secs_until_next_charge: 1e6,
            ..Default::default()
        };

        let v_before = state.x_velocity;
        step(&mut state, Duration::from_millis(50), &bars, false);

        // After one tick the boat has crossed x = 1.0 and re-entered near 0.
        assert!(
            state.x_ratio < 0.5,
            "precondition: boat should have wrapped (got {})",
            state.x_ratio
        );
        // Velocity may have shifted slightly from drive/damping/restoring,
        // but the hard-clamp regression would zero it outright.
        assert!(
            state.x_velocity > v_before * 0.5,
            "x_velocity must not collapse across the wrap (before {v_before}, after {})",
            state.x_velocity
        );
    }

    #[test]
    fn step_seam_slope_cancels_under_symmetric_low_edges() {
        // Music-shaped FFT spectra are usually low at both ends (sub-bass
        // on the left, top-treble on the right) with content in the
        // middle. Combined with the boat's wrap geometry that creates a
        // `\/` basin at the seam: edge-clamped slope sampling read a
        // strong inward gradient from either side, pinning the boat
        // there until the next captain charge fired. Toroidal slope
        // sampling reads the opposite edge instead of clamping, so on a
        // curve symmetric about the seam the two samples cancel and the
        // slope force vanishes — drive + inertia carry the boat through.
        //
        // Worst case: bars shaped like `sin(π·t)`, low at both ends,
        // peaked in the middle. Place the boat at x=0 with phase tuned
        // so step()'s drive force lands at exactly 0 this tick. With the
        // fix the only force acting is slope; symmetry guarantees it's
        // near zero. Without the fix the slope would push the boat
        // inward at ~0.003 ratio/sec on the very first tick.
        let n = 33;
        let bars: Vec<f64> = (0..n)
            .map(|i| {
                let t = i as f64 / (n - 1) as f64;
                (std::f64::consts::PI * t).sin()
            })
            .collect();
        assert!(
            (bars[0] - bars[n - 1]).abs() < 1e-9,
            "precondition: bars must be symmetric so any non-zero slope \
             at the seam comes from the sampling rule, not the data"
        );

        let dt = Duration::from_millis(50);
        // step() advances phase before sampling drive. Pre-position phase
        // so the post-advance value is exactly 0 → sin(0) = 0 → drive
        // contribution this tick is 0.
        let phase_advance = dt.as_secs_f32() / DRIVE_PERIOD_SECS;
        let mut state = BoatState {
            x_ratio: 0.0,
            x_velocity: 0.0,
            phase: (1.0 - phase_advance).rem_euclid(1.0),
            rng_state: 0x12345,
            secs_until_next_charge: 1e6,
            ..Default::default()
        };

        step(&mut state, dt, &bars, false);

        assert!(
            state.x_velocity.abs() < 1e-3,
            "toroidal slope sampling should cancel at a symmetric seam \
             basin (got x_velocity = {}, expected ~0; pre-fix value \
             would be ~3e-3)",
            state.x_velocity
        );
    }

    #[test]
    fn step_slope_force_is_gated_when_local_height_is_low() {
        // The toroidal-sampling fix removed the *hard* gradient at the
        // exact seam, but on real (asymmetric) spectra the V-shape
        // extends a few percent inward from each edge — enough residual
        // downhill pull to keep the boat dwelling near the edges between
        // captain charges. This test guards the height-gate that
        // suppresses slope force in low-amplitude regions.
        //
        // Construct: flat low baseline (0.01) with a sharp peak in the
        // middle. Place the boat in the flat region close enough that
        // the slope sampler reaches into the peak's rising face — so
        // pre-gate, slope force would saturate at the cap and slam the
        // boat toward the peak. Local height at the boat's position is
        // still essentially 0, well below the gate floor, so the gate
        // multiplies the slope force by zero.
        let mut bars = vec![0.01_f64; 33];
        bars[16] = 1.0;
        bars[15] = 0.7;
        bars[17] = 0.7;
        bars[14] = 0.3;
        bars[18] = 0.3;

        let dt = Duration::from_millis(50);
        let phase_advance = dt.as_secs_f32() / DRIVE_PERIOD_SECS;
        let mut state = BoatState {
            // pos ≈ 8.6 in a 33-bar buffer — far enough below the peak
            // (pos 16) that local_height is the flat baseline, but the
            // right slope sample reaches up the rising face.
            x_ratio: 0.27,
            x_velocity: 0.0,
            phase: (1.0 - phase_advance).rem_euclid(1.0),
            rng_state: 0x12345,
            secs_until_next_charge: 1e6,
            ..Default::default()
        };

        let h_local = sample_line_height(&bars, state.x_ratio, false);
        assert!(
            h_local < SLOPE_GATE_FLOOR,
            "precondition: local height must be below SLOPE_GATE_FLOOR \
             so the gate fully suppresses slope force (got {h_local}, \
             floor {SLOPE_GATE_FLOOR})"
        );

        step(&mut state, dt, &bars, false);

        // Without the gate, slope force would saturate at -MAX_SLOPE_FORCE
        // and produce |x_velocity| ≈ 5e-3 in this single tick. With the
        // gate, only the (phase-zeroed) drive contributes — essentially 0.
        assert!(
            state.x_velocity.abs() < 1e-3,
            "slope force must be gated to ~0 when local_height < FLOOR \
             (got x_velocity = {}, expected ~0; pre-gate value would be \
             ~5e-3)",
            state.x_velocity
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
    fn step_charge_makes_net_progress_against_slope() {
        // Linear ramp upward to the right: slope force normally pushes
        // the boat *left*. Pre-arm a rightward charge and verify the
        // boat still ends up *right* of where it started — the captain's
        // intent dominates net displacement, even though slope is now
        // blended at `CHARGE_SLOPE_BLEND` rather than fully zeroed.
        //
        // Pre-envelope+blend the assertion was `> 0.6` because slope was
        // zeroed and force was constant. With the half-sine envelope
        // (avg ~64% of peak) plus 50% slope blend, net thrust against a
        // max-steepness ramp drops to ~30% of the old value — the boat
        // still moves right, just less aggressively. Run for the full
        // charge duration so the envelope accumulates fully.
        let bars: Vec<f64> = (0..32).map(|i| i as f64 / 31.0).collect();
        let duration = 6.0;
        let mut state = BoatState {
            x_ratio: 0.5,
            charge_remaining_secs: duration,
            charge_total_secs: duration,
            charge_direction: 1.0,
            rng_state: 0x12345,
            secs_until_next_charge: 100.0,
            ..Default::default()
        };
        let dt = Duration::from_millis(16);
        let ticks = (duration / dt.as_secs_f32()) as usize;
        for _ in 0..ticks {
            step(&mut state, dt, &bars, false);
        }
        assert!(
            state.x_ratio > 0.55,
            "rightward charge should still gain ground against a steep \
             leftward slope (got x_ratio = {})",
            state.x_ratio
        );
    }

    #[test]
    fn charge_envelope_zero_at_endpoints_peak_at_midpoint() {
        // Half-sine shape: zero start, full peak mid-stroke, zero end.
        // This is the contract `step()` relies on to ramp the captain's
        // effort in and out instead of slamming on full thrust.
        let total = 4.0;
        // remaining == total → progress = 0 → sin(0) = 0
        assert!(
            charge_envelope(total, total).abs() < 1e-6,
            "envelope at progress=0 must be 0 (got {})",
            charge_envelope(total, total)
        );
        // remaining == total/2 → progress = 0.5 → sin(π/2) = 1
        assert!(
            (charge_envelope(total, total / 2.0) - 1.0).abs() < 1e-6,
            "envelope at progress=0.5 must peak at 1 (got {})",
            charge_envelope(total, total / 2.0)
        );
        // remaining == 0 → progress = 1 → sin(π) ≈ 0
        assert!(
            charge_envelope(total, 0.0).abs() < 1e-5,
            "envelope at progress=1 must be 0 (got {})",
            charge_envelope(total, 0.0)
        );
        // total == 0 sentinel: BoatState::default has charge_total_secs = 0,
        // so the helper must not divide-by-zero into NaN before the first
        // charge starts.
        assert_eq!(charge_envelope(0.0, 4.0), 0.0);
    }

    #[test]
    fn step_charge_force_is_zero_on_first_tick_then_ramps_up() {
        // Half-sine envelope at progress=0 is exactly 0 — so the very
        // first tick of a fresh charge applies no horizontal force from
        // the captain. Subsequent ticks ramp up smoothly. This is the
        // anti-step-function contract.
        let bars = vec![0.5_f64; 16];
        let mut state = BoatState {
            x_ratio: 0.5,
            // Pre-arm: charge fires on the next tick because
            // secs_until_next_charge ticks below 0.
            secs_until_next_charge: 0.0,
            // Stable rng so charge direction is deterministic.
            rng_state: 0x12345,
            ..Default::default()
        };

        let dt = Duration::from_millis(16);
        step(&mut state, dt, &bars, false);

        // Charge has now been started. charge_total_secs is set, but the
        // first tick's progress is 0, so envelope=0 and charge_force=0.
        // x_velocity should be effectively zero (only drive contributes,
        // and at phase ≈ 0 drive is nearly zero from rest).
        assert!(state.charge_remaining_secs > 0.0);
        assert!(state.charge_total_secs > 0.0);
        assert!(
            state.x_velocity.abs() < 1e-3,
            "first tick of a fresh charge must apply ~0 horizontal force \
             (envelope = sin(π·0) = 0; got x_velocity = {})",
            state.x_velocity
        );

        // Run a few more ticks; the captain should be pulling now.
        for _ in 0..30 {
            step(&mut state, dt, &bars, false);
        }
        assert!(
            state.x_velocity.abs() > 1e-3,
            "after ramp-in the captain should be applying meaningful \
             force (got x_velocity = {})",
            state.x_velocity
        );
    }

    #[test]
    fn step_slope_partially_felt_during_charge() {
        // Boat charging leftward: with flat bars only the charge force
        // acts. With a descending ramp (slope pushing rightward against
        // the leftward charge), the slope blend partially cancels the
        // charge — the boat accelerates LESS leftward than on flat
        // water. This is the "battling the waves" property: the captain
        // still feels wave faces while rowing.
        let flat = vec![0.5_f64; 32];
        // Descending ramp: bars[i] high at i=0, low at i=n-1. Slope at
        // x=0.5 is negative; -slope * GAIN > 0 → slope force pushes right.
        let descending: Vec<f64> = (0..32).map(|i| (31 - i) as f64 / 31.0).collect();

        let duration = 4.0;
        let mid_progress_remaining = duration / 2.0;
        let template = BoatState {
            x_ratio: 0.5,
            charge_remaining_secs: mid_progress_remaining,
            charge_total_secs: duration,
            charge_direction: -1.0,
            rng_state: 0x12345,
            secs_until_next_charge: 100.0,
            ..Default::default()
        };

        let dt = Duration::from_millis(50);
        let mut state_flat = template.clone();
        step(&mut state_flat, dt, &flat, false);
        let mut state_against = template.clone();
        step(&mut state_against, dt, &descending, false);

        // On flat water, slope = 0 → only charge force acts.
        // On descending ramp, slope force partially counters the charge
        // (blended at CHARGE_SLOPE_BLEND), so the leftward velocity is
        // smaller in magnitude.
        assert!(
            state_against.x_velocity > state_flat.x_velocity,
            "downhill slope must partially cancel a leftward charge \
             via the blend (flat v = {}, against v = {})",
            state_flat.x_velocity,
            state_against.x_velocity
        );
    }

    #[test]
    fn pick_charge_direction_is_a_fair_coin_flip() {
        // Both directions must be reachable across different seeds. Use
        // production-shaped seeds (xorshift's first output is degenerate
        // for tiny seeds, so seeding from a single counter would hit one
        // side only — irrelevant in real use where the seed is the golden
        // ratio constant).
        let mut saw_left = false;
        let mut saw_right = false;
        for i in 0u32..200 {
            let mut rng = 0x9E37_79B9_u32.wrapping_add(i.wrapping_mul(0x6789_ABCD));
            match pick_charge_direction(&mut rng) {
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

    // --- tilt physics ----------------------------------------------------------

    #[test]
    fn step_tilt_decays_to_zero_on_flat_waves() {
        // Flat bars → slope = 0 → target_tilt = 0. Starting from a
        // non-zero tilt, the spring-damper should pull it toward zero
        // within a couple of seconds.
        let bars = vec![0.5; 16];
        let mut state = BoatState {
            x_ratio: 0.5,
            tilt: 0.25,
            tilt_velocity: 0.0,
            // Suppress charges so they don't perturb anything.
            rng_state: 0x12345,
            secs_until_next_charge: 1e6,
            ..Default::default()
        };
        let final_state = run(state.clone(), 200, Duration::from_millis(10), &bars);
        assert!(
            final_state.tilt.abs() < 0.02,
            "tilt must decay near zero on a flat waveform (got {})",
            final_state.tilt
        );
        // Single tick from rest: tilt moves but doesn't overshoot the
        // target — checks the underdamped tuning isn't *too* underdamped.
        step(&mut state, Duration::from_millis(16), &bars, false);
        assert!(
            state.tilt < 0.25,
            "tilt must have started decaying after one tick (got {})",
            state.tilt
        );
    }

    #[test]
    fn step_tilt_converges_to_signed_value_on_steady_slope() {
        // Linear ramp upward to the right (positive slope everywhere).
        // After enough ticks, tilt should converge to a *negative* value
        // (counterclockwise in iced's CW-positive convention = bow up
        // when sailing rightward up the ramp), bounded by MAX_TILT.
        let bars: Vec<f64> = (0..16).map(|i| i as f64 / 15.0).collect();
        let initial = BoatState {
            x_ratio: 0.5,
            // Pin the boat in place so the slope sample stays steady.
            x_velocity: 0.0,
            rng_state: 0x12345,
            secs_until_next_charge: 1e6,
            ..Default::default()
        };
        let final_state = run(initial, 400, Duration::from_millis(10), &bars);
        assert!(
            final_state.tilt < 0.0,
            "positive slope must produce negative tilt — bow-up = CCW = \
             negative in iced's CW-positive rotation (got {})",
            final_state.tilt
        );
        assert!(
            final_state.tilt.abs() <= MAX_TILT + 1e-3,
            "tilt magnitude must respect MAX_TILT cap (got {}, cap {})",
            final_state.tilt,
            MAX_TILT
        );
    }

    #[test]
    fn step_tilt_stays_bounded_under_extreme_slope() {
        // Very steep oscillating waveform: slope sample swings to large
        // values in both directions. Tilt must never exceed the cap in
        // either direction across many simulated ticks.
        let bars: Vec<f64> = (0..32)
            .map(|i| ((i as f64 * 0.7).sin() * 0.5 + 0.5).clamp(0.0, 1.0))
            .collect();
        let initial = BoatState {
            x_ratio: 0.5,
            x_velocity: 0.05,
            rng_state: 0x12345,
            secs_until_next_charge: 1e6,
            ..Default::default()
        };
        let mut state = initial;
        let dt = Duration::from_millis(10);
        for _ in 0..2000 {
            step(&mut state, dt, &bars, false);
            assert!(
                state.tilt.abs() <= MAX_TILT + 1e-3,
                "tilt must always stay within ±MAX_TILT (got {})",
                state.tilt
            );
        }
    }

    // --- facing flip ----------------------------------------------------------

    #[test]
    fn step_facing_holds_below_threshold() {
        // Flat bars + tiny rightward velocity below FLIP_THRESHOLD: the
        // captain's coin-flip charge can perturb x_velocity, so suppress
        // charges. With pre-set facing = -1, the boat must NOT flip even
        // though sign(x_velocity) > 0, because |v| stays sub-threshold.
        let bars = vec![0.5; 16];
        let mut state = BoatState {
            x_ratio: 0.5,
            x_velocity: FLIP_THRESHOLD * 0.5, // half-threshold rightward
            facing: -1.0,
            rng_state: 0x12345,
            secs_until_next_charge: 1e6,
            ..Default::default()
        };
        // One short tick — drive force at phase=0 is zero, damping shrinks
        // velocity, no charge, no slope. Velocity stays below threshold.
        step(&mut state, Duration::from_millis(16), &bars, false);
        assert_eq!(
            state.facing, -1.0,
            "facing must hold at -1 when |x_velocity| < FLIP_THRESHOLD \
             (x_velocity = {}, FLIP_THRESHOLD = {})",
            state.x_velocity, FLIP_THRESHOLD
        );
    }

    #[test]
    fn step_facing_flips_above_threshold() {
        // Same setup but with velocity well above the threshold — the
        // boat should snap facing to match the velocity sign on the next
        // tick.
        let bars = vec![0.5; 16];
        let mut state = BoatState {
            x_ratio: 0.5,
            x_velocity: 0.10, // well above FLIP_THRESHOLD
            facing: -1.0,
            rng_state: 0x12345,
            secs_until_next_charge: 1e6,
            ..Default::default()
        };
        step(&mut state, Duration::from_millis(16), &bars, false);
        assert_eq!(
            state.facing, 1.0,
            "facing must flip to +1 when x_velocity > FLIP_THRESHOLD \
             (x_velocity = {})",
            state.x_velocity
        );
    }

    #[test]
    fn step_facing_initializes_from_zero_on_first_qualifying_tick() {
        // Default facing is 0.0. First tick with supra-threshold velocity
        // must snap facing to the velocity sign.
        let bars = vec![0.5; 16];
        let mut state = BoatState {
            x_ratio: 0.5,
            x_velocity: -0.10,
            // facing left default of 0.0
            rng_state: 0x12345,
            secs_until_next_charge: 1e6,
            ..Default::default()
        };
        step(&mut state, Duration::from_millis(16), &bars, false);
        assert_eq!(
            state.facing, -1.0,
            "facing must initialize to sign(x_velocity) on first qualifying tick"
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
