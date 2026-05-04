//! Sailing-boat overlay for the lines-mode visualizer.
//!
//! Pure CPU helpers + a tiny stateful struct that the root TEA owns. The
//! widget does not touch the WGSL pipeline or the FFT thread — it only reads
//! the bar buffer (`VisualizerState::get_bars()`) the shader already consumes,
//! resamples it via the same Catmull-Rom basis (`catmull_rom_1d`) the lines
//! shader uses, and rides on top of the rendered waveform.
//!
//! The model borrows the "always-forward, never-stalled" trick from Tiny
//! Wings (`reference-tiny-wings/Hero.mm`'s `minVelocityX`): a constant sail
//! thrust along `facing` plus a hard forward-velocity floor that no wave
//! shape can overcome. Wave heights still affect the boat — they just do so
//! through tilt and buoyancy rather than by blocking horizontal travel.
//!
//! - **Sail thrust** — `facing · MAX_SAIL_THRUST · total_intensity`.
//!   Music intensity (cruise + beat + onset, all in `[0, 1]`) is the
//!   sole driver of forward motion: silence produces zero thrust and
//!   the boat coasts to a stop. There is no constant baseline; the
//!   boat is propelled entirely by what's playing.
//! - **Forward-velocity floor** — after every integration, `x_velocity` is
//!   reasserted to at least `MIN_SAILING_VELOCITY` in the facing direction.
//!   This single clamp is what guarantees the boat clears every wave: no
//!   slope, damping, or numerical extreme can drop forward speed below the
//!   floor.
//! - **Slope force** — the local wave gradient at the boat's x position
//!   pushes it downhill (positive slope → push left, negative → push right),
//!   capped at `MAX_SLOPE_FORCE`. Sized below `MAX_SAIL_THRUST` so an uphill
//!   slows the boat noticeably without ever reversing it. Gated to zero in
//!   low-amplitude regions so the boat doesn't drift on calm water.
//! - **Velocity damping** — friction on `x_velocity` gives the "floating"
//!   feel; the boat lags fast wave changes instead of snapping to them.
//! - **Tack events** — every `[TACK_INTERVAL_MIN_SECS,
//!   TACK_INTERVAL_MAX_SECS]` seconds the wind shifts: `facing` flips and
//!   the boat sails the other way. The countdown only ticks down in the
//!   visible area so a tack can't fire while the boat is mid-margin
//!   (where it would briefly disagree with the latched eject direction).
//! - **Y dynamics** — `y_ratio` follows the sampled wave height through a
//!   spring-damper rather than tracking it exactly, so the boat bobs with
//!   buoyancy rather than gluing to the curve. This is the half of the
//!   wave interaction that actually carries the boat (vertically), even
//!   while sail thrust handles horizontal travel.
//! - **Tilt** — a spring-damper toward `-slope · TILT_GAIN`, capped at
//!   `MAX_TILT`. Independent of facing because the SVG mirrors when
//!   facing flips, so "uphill on the right = bow-up" works for both sides.
//! - **Toroidal X wrap with off-screen margin** — `x_ratio` lives in
//!   `[-x_wrap_margin, 1 + x_wrap_margin)` and wraps via `rem_euclid` over
//!   that extended span; `x_velocity` is preserved across the seam. The
//!   margin is sized in the handler from the live boat sprite width
//!   (`BOAT_WRAP_MARGIN_BOAT_WIDTHS · boat_w / area_width`) so the boat
//!   fully exits the visible area before wrapping — the renderer draws a
//!   single copy at `target_x` and lets the outer clip trim the off-screen
//!   portion, so the boat is never visible in two places at once.
//! - **Margin deadspace** — while in the margin, slope force is muted
//!   so toroidal "across the seam" gradients can't drag the boat back
//!   into the edge it just left. No special force is applied; sail
//!   thrust + the velocity floor carry the boat through the margin at
//!   terminal velocity in its facing direction. (An earlier revision
//!   used a constant `EJECT_FORCE` here as a stall-prevention; with the
//!   floor in place it became redundant and was removed.)
//!
//! `BoatState.tilt_handles` caches themed boat SVGs lazily on first use,
//! keyed by quantized `(tilt, facing)`. `Handle::from_memory` re-hashes
//! input bytes per call (see `reference-iced/core/src/svg.rs:89`), so
//! per-frame construction would churn GPU cache keys — the same class of
//! bug as the `image::Handle::from_path` gotcha called out in `CLAUDE.md`.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use iced::{
    Color, Element, Event, Length, Point, Rectangle, Size, Vector,
    advanced::{
        Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{Operation, Tree},
    },
    widget::{Stack, Svg, canvas, container, svg},
};
use tracing::trace;

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

/// Anchor sprite height as a fraction of the boat's height. The boat
/// SVG fills its full 80-unit viewBox; the lucide anchor fills its
/// 24-unit viewBox; both render at `_h × _h` since both are square in
/// content. With the rope drawn separately on the canvas (no rope
/// inside the SVG), the anchor sprite is purely the lucide icon —
/// scale by feel: 0.6 × boat_h reads as a clearly recognizable
/// doodad without rivaling the boat hull for visual weight.
pub(crate) const ANCHOR_HEIGHT_MULTIPLE_OF_BOAT: f32 = 0.6;

// --- physics tuning constants ---------------------------------------------
//
// All forces operate in normalized ratio-space (`x_ratio` ∈ [0, 1], time in
// seconds). Sail thrust along `facing` is the dominant horizontal force,
// driven by music intensity in `[0, 1]`. Terminal velocity at saturating
// music is `MAX_SAIL_THRUST / X_DAMPING ≈ 0.10 ratio/sec` (roughly a 10 s
// crossing); at silence the boat coasts to rest.

/// Sample distance (in ratio space) on either side of the boat for the
/// finite-difference slope estimate. Small enough to capture local curvature,
/// large enough to smooth out single-bar jitter.
const SLOPE_DX: f32 = 0.05;

/// Slope force gain — converts wave gradient into horizontal force.
const SLOPE_GAIN: f32 = 0.04;

/// Hard cap on `|slope_force|`. Sized below `MAX_SAIL_THRUST` so peak
/// slope resistance never overcomes the sail in the facing direction.
/// Slope force is masked to RESIST motion only (never assist), so an
/// uphill flank produces a headwind that slows the sail's terminal
/// velocity from `MAX_SAIL_THRUST / X_DAMPING ≈ 0.10` down to
/// `(MAX_SAIL_THRUST - MAX_SLOPE_FORCE) / X_DAMPING ≈ 0.067`. The
/// velocity floor keeps the boat moving forward when the wave alone
/// would stall it.
const MAX_SLOPE_FORCE: f32 = 0.03;

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

/// Friction on `x_velocity`. The dominant source of the "floating" feel —
/// without this, the boat would build up arbitrary speed.
const X_DAMPING: f32 = 0.9;

/// Hard cap on `|x_velocity|` to keep numerical extremes from launching the
/// boat across the screen in a single tick. Sized to accommodate the
/// stacked-intensity terminal velocity at peak music (cruise + beat +
/// onset → `total_intensity ≈ 2.0` → terminal `≈ 0.22 ratio/sec`) with
/// a small safety margin so the cap doesn't truncate the visible
/// stacking headroom on energetic tracks.
const MAX_X_V: f32 = 0.20;

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

// --- new sailing model constants ------------------------------------------
//
// The physics that follows is the "Tiny Wings on water" rewrite: a constant
// sail thrust along `facing`, a forward-velocity floor that guarantees the
// boat clears every wave, and a tack-event timer that occasionally flips
// `facing` (the wind shifts). See module-level docs for the full force
// budget and the references the design borrows from.

/// Maximum horizontal force the sails can produce when the music is
/// fully saturating. The actual sail force each tick is `MAX_SAIL_THRUST
/// · total_intensity` where `total_intensity ∈ [0, 2]` (cruise saturates
/// at 1, beat + onset can stack ~1.0 above it for a peak of 2.0 on
/// energetic tracks). At `intensity = 1` (saturating cruise alone) the
/// terminal velocity is `MAX_SAIL_THRUST / X_DAMPING ≈ 0.11 ratio/sec`
/// — a roughly 9 s crossing. At `intensity = 2` (cruise + saturating
/// beat + onset) terminal climbs to `≈ 0.22 ratio/sec` ≈ 4.5 s
/// crossing, hitting the `MAX_X_V` cap. This is the headroom that
/// gives Mother-North-class energetic tracks a visibly faster
/// presentation than Sea-Pictures-class punchy tracks.
const MAX_SAIL_THRUST: f32 = 0.10;

/// Maximum velocity floor — the cap on `MIN_SAILING_VELOCITY ·
/// total_intensity`. Asserted only when the boat's velocity is in the
/// SAME direction as `facing`, so a fresh tack can decelerate through
/// zero before the floor re-engages on the new heading (smooth turns).
/// At full music intensity the boat's minimum cruise speed is this
/// value; at silence the floor is 0 and the boat is allowed to stop.
/// Scaled in lockstep with `MAX_SAIL_THRUST` so the floor-to-cruise
/// ratio stays roughly constant across tuning changes.
const MIN_SAILING_VELOCITY: f32 = 0.04;

/// Min/max delay between tack events (random "wind shift" that flips
/// `facing`). Sampled uniformly in `[MIN, MAX]` after each tack and at
/// first activation, so direction changes feel scheduled by mood rather
/// than clockwork. Tuned long enough that a single voyage across the
/// visible area completes before the wind turns — the boat reads as
/// purposefully sailing in one direction, not wandering.
const TACK_INTERVAL_MIN_SECS: f32 = 20.0;
const TACK_INTERVAL_MAX_SECS: f32 = 60.0;

/// Time over which sail thrust + velocity floor ramp from 0 back to
/// full after a tack. The first moment after a flip, sail thrust is
/// 0 so damping alone decelerates the boat; sail thrust ramps in over
/// `TACK_RAMP_SECS`, smoothly accelerating the boat onto its new
/// heading. This is what makes turns visibly gradual rather than the
/// boat "stopping on a dime" and instantly accelerating in reverse.
const TACK_RAMP_SECS: f32 = 4.0;

/// Min/max delay between drop-anchor events. Rarer than tacks (which
/// fire every 20–60 s) so the rest stops read as deliberate moments
/// rather than constant lurking. Sampled uniformly in `[MIN, MAX]`.
const ANCHOR_INTERVAL_MIN_SECS: f32 = 45.0;
const ANCHOR_INTERVAL_MAX_SECS: f32 = 120.0;

/// Anchor-firing safe zone: anchor only fires when `x_ratio` is well
/// within `[ANCHOR_SAFE_LO, ANCHOR_SAFE_HI]`. Outside this zone the
/// boat is too close to the wrap margin — even after the anchor
/// catches the boat, residual rendering at the wrap seam (or the
/// boat re-entering after a wrap) would have the rope stretching
/// across the entire visible area to the dropped anchor on the far
/// side, which reads as a rendering glitch.
const ANCHOR_SAFE_LO: f32 = 0.15;
const ANCHOR_SAFE_HI: f32 = 0.85;

/// Music-driven thrust composition.
///
/// `total_intensity ∈ [0, 2]` is built from four signals and drives
/// `MAX_SAIL_THRUST` linearly (the velocity floor uses `min(intensity,
/// 1)` so it doesn't spike on stacked tracks). The composition is:
///
///   `total_intensity = (cruise + BEAT_AMP·beat + ONSET_AMP·onset).clamp(0, 2)`
///
/// The `[0, 2]` range (rather than the old `[0, 1]`) is the key to
/// differentiating energetic-but-clamped tracks from one another:
/// once cruise saturates at 1.0, beat + onset can still stack
/// another ~1.0 of thrust on top, so a brick-walled black-metal
/// track lands above a moderately-punchy techno track instead of
/// both pinning at the same ceiling.
///
/// where `cruise = max(flux_cruise, presence_cruise)` —
/// - `flux_cruise = ((long_onset - LONG_ONSET_FLOOR).max(0) ·
///    LONG_ONSET_AMP · bpm_scale).clamp(0, 1)`
/// - `presence_cruise = ((bar_energy - PRESENCE_FLOOR).max(0) ·
///    PRESENCE_AMP · bpm_scale).clamp(0, 1)`
///
/// The two cruise inputs are **complementary**: `long_onset` is a
/// spectral-flux EMA (good on transient/percussive material, dies on
/// sustained pads) while `bar_energy` is the average of the visible
/// bar buffer (good on sustained material, dies only at silence).
/// `max()` lets whichever signal has more to say drive the boat
/// without the two diluting each other on punchy tracks. Both
/// signals → 0 at silence so the boat still coasts to rest with no
/// audio. Different songs settle each signal at different values,
/// producing visibly different cruise speeds; transient hits and
/// beat pulses surge above the cruise level.
const LONG_ONSET_FLOOR: f32 = 0.02;
const LONG_ONSET_AMP: f32 = 18.0;
/// Spectrum-presence cruise — average of the visible bars (the same
/// buffer the lines shader paints, already auto-sensitivity
/// normalized in `[0, ~1]`). Catches sustained material that
/// produces low spectral flux but fills the screen — pads, drones,
/// organs, vocal-only tracks. `PRESENCE_FLOOR = 0.10` is a deadzone
/// so a barely-visible spectrum doesn't propel the boat;
/// `PRESENCE_AMP = 1.5` shapes the response so a comfortably-full
/// spectrum (~0.6 avg) lands near 75% cruise without saturating, and
/// only a wall-of-sound spectrum reaches the cap. Tuned so the boat
/// tracks "what the user perceives on screen" — visible waves =
/// boat moves, empty visualizer = boat stops.
const PRESENCE_FLOOR: f32 = 0.10;
const PRESENCE_AMP: f32 = 1.5;
/// Beat-pulse contribution to total intensity. Range is the half-sine-
/// squared envelope `[0, 1]`; `BEAT_AMP = 0.4` lets a beat add up to
/// 40 percentage points to intensity on top of the cruise level.
const BEAT_AMP: f32 = 0.4;
/// Onset (instant transient) contribution. Range is roughly `[0, 1]`;
/// `ONSET_AMP = 0.6` lets a hit surge intensity by up to 60 points
/// above cruise.
const ONSET_AMP: f32 = 0.6;

/// BPM at which the BPM scale factor equals `1.0`. When a song has a
/// tagged BPM, the cruise component is multiplied by
/// `(bpm / REFERENCE_BPM).clamp(BPM_SCALE_MIN, BPM_SCALE_MAX)` to
/// scale the cruise level by tempo — fast tracks cruise faster than
/// the long_onset alone would predict, slow tracks cruise slower.
const REFERENCE_BPM: f32 = 120.0;
const BPM_SCALE_MIN: f32 = 0.5;
const BPM_SCALE_MAX: f32 = 2.0;

/// Min/max duration of an active anchor. The boat hovers in place,
/// catching the beat as waves roll past underneath. Long enough to feel
/// like a deliberate stop, short enough that the music's character
/// barely changes during it.
const ANCHOR_DURATION_MIN_SECS: f32 = 10.0;
const ANCHOR_DURATION_MAX_SECS: f32 = 15.0;

/// Hard cap on `|anchor_sway|` (radians). At ~6° the rope swing reads as
/// "the water is moving the rope" without ever pulling the anchor far
/// enough to detach visually from below the boat.
const MAX_ANCHOR_SWAY: f32 = 0.10;

/// Spring constant for `anchor_sway` tracking the wave-driven target.
/// Lower than the boat's tilt spring so the rope settles slower —
/// matches the visual intuition that the rope is a heavier, lazier
/// thing than the boat hull.
const ANCHOR_SWAY_SPRING_K: f32 = 8.0;

/// Damping on `anchor_sway_velocity`. With `SPRING_K = 8` and damping
/// `4` the damping ratio ζ ≈ 0.71 — slightly underdamped so a quick
/// wave still produces a small overshoot, then settles. Same family as
/// the boat's tilt and Y springs.
const ANCHOR_SWAY_DAMPING: f32 = 4.0;

/// Frequency (Hz) of the slow oscillator that drives the sway target.
/// 0.4 Hz = a 2.5 s period, which reads as a gentle sea-swell rhythm
/// against the much faster waveform jitter the boat already responds
/// to via tilt.
const ANCHOR_SWAY_DRIVE_HZ: f32 = 0.4;

/// Local-wave-height threshold below which sway target stays at zero.
/// On calm spectrums the rope hangs straight down; the wave has to
/// reach this fraction of the visualizer height before the rope starts
/// to swing. Same `SLOPE_GATE_FLOOR` value the slope force uses, so
/// the "calm water doesn't move stuff around" invariant is uniform
/// across the doodad.
const ANCHOR_SWAY_AMPLITUDE_FLOOR: f32 = SLOPE_GATE_FLOOR;

/// Per-frame music signals fed to `step()` so the boat's sail thrust
/// can react to what's playing. `bpm` (when present) drives a beat-
/// locked sine envelope; `onset_energy` (smoothed spectral flux,
/// always available) scales thrust with instantaneous spectral
/// surprise. Both feed into `total_intensity` —
/// with `MusicSignals::default()` (no BPM, zero onset) the boat
/// behaves exactly as it did before the music-driven rewrite.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct MusicSignals {
    pub(crate) bpm: Option<u32>,
    /// Fast onset envelope (~50 ms time constant) — instantaneous
    /// transient energy. Drives short surges in sail thrust.
    pub(crate) onset_energy: f32,
    /// Slow onset envelope (~10 s time constant) — song-level average
    /// energy. Drives the *baseline* sail thrust on un-tagged tracks
    /// (and stacks multiplicatively with the BPM scale on tagged
    /// tracks), so the boat's resting cruise speed differs between
    /// songs.
    pub(crate) long_onset_energy: f32,
    /// Average of the visible bar buffer (the same data the lines
    /// shader paints), in `[0, ~1]`. Captures *sustained* spectrum
    /// presence that complements `long_onset_energy`'s flux-based
    /// signal: ambient pads / drones / organs that produce loud but
    /// flat spectra still drive the boat through this channel. Goes
    /// to 0 only at true silence (empty bars). The boat handler
    /// computes this from `current_bars()` so the boat propulsion
    /// stays in lockstep with the visible waveform.
    pub(crate) bar_energy: f32,
}

/// Per-frame UI-thread state for the surfing boat.
///
/// - `x_ratio` / `y_ratio` are the boat's normalized position in `[0, 1]`,
///   integrated from the velocity fields below.
/// - `x_velocity` / `y_velocity` are persisted across ticks so the physics
///   has memory (inertia → floating feel).
/// - `tilt` is the boat's current rotation in radians (positive =
///   counterclockwise = bow up when the boat is sailing rightward up a
///   slope). `tilt_velocity` is the spring-damper rate; together they
///   ease the boat into the slope so it doesn't snap to spectrum jitter.
/// - `facing` is `+1.0` (sailing right, sail catches wind from the left)
///   or `-1.0` (sailing left, sail mirrored). Set to a random ±1 on the
///   first tick when defaulted to `0.0`, then flipped only by tack events.
///   Sail thrust is applied along this direction every tick.
/// - `visible` is derived per tick by the handler — it is *not* the user's
///   on/off toggle (that lives in `LinesConfig.boat`).
/// - `last_tick` is consumed to compute `dt` between ticks; cleared when the
///   boat is hidden so the first frame back doesn't see a stale gap.
/// - `secs_until_next_tack` counts down to the next direction flip ("wind
///   shift"). Lazily seeded on the first tick alongside `rng_state` and
///   reseeded uniformly from `[TACK_INTERVAL_MIN_SECS,
///   TACK_INTERVAL_MAX_SECS]` after each flip. Only ticks down while the
///   boat is in the visible area and not currently anchored, so neither
///   a tack mid-margin nor a tack mid-anchor can fire.
/// - `secs_until_next_anchor` counts down to the next drop-anchor event.
///   Like the tack timer, only ticks while the boat is in the visible
///   area; reseeded uniformly from `[ANCHOR_INTERVAL_MIN_SECS,
///   ANCHOR_INTERVAL_MAX_SECS]` after the anchor lifts.
/// - `anchor_remaining_secs` is the time left in the current anchor (>0
///   means the boat is anchored; 0 means sailing). Sampled uniformly
///   from `[ANCHOR_DURATION_MIN_SECS, ANCHOR_DURATION_MAX_SECS]` when an
///   anchor event fires. While anchored, sail thrust is suspended,
///   `x_velocity` damps toward zero, slope force is suppressed (so tall
///   waves don't drag the boat away from the dropped anchor), and
///   `secs_until_next_tack` is frozen so the wind doesn't shift while
///   the boat is paused.
/// - `anchor_drop_x` is the boat's `x_ratio` at the moment the current
///   anchor fired. The renderer pins the anchor sprite to this x and
///   the bottom of the visualizer area, so the anchor stays planted on
///   the floor even as the boat bobs and drifts slightly above.
/// - `anchor_sway` / `anchor_sway_velocity` are a spring-damper pendulum
///   that drives the rope's swing angle while anchored. The target is
///   `local_wave_amplitude · sin(2π · sway_phase)`, gated below
///   `ANCHOR_SWAY_AMPLITUDE_FLOOR` so calm spectrums leave the rope
///   straight down. Capped at `±MAX_ANCHOR_SWAY` radians.
/// - `sway_phase` is in `[0, 1)` and ticks linearly at
///   `ANCHOR_SWAY_DRIVE_HZ`. Drives the slow oscillator that determines
///   which side the wave is currently pushing the rope toward. Holds
///   while the boat is hidden or paused, the same way `last_tick` does.
/// - `rng_state` seeds a tiny xorshift PRNG used for tack/anchor timing
///   and the initial facing pick. Lazily seeded on first tick (0 → seed
///   constant) so `Default` stays clean and the schedule starts on first
///   activation, not on construction.
/// - `tilt_handles` caches the themed boat SVG keyed by quantized tilt
///   angle and facing — see `cache_handle_for`. Because the rotation is
///   baked into the SVG path data (rather than rotating an
///   already-rasterized bitmap in the wgpu shader), we want one handle
///   per visibly-distinct orientation. With a 0.5° quantization step and
///   a ±17° tilt range, that's ~140 entries at the worst case — ~3 KB
///   each in iced's bitmap atlas, so ~400 KB worst-case footprint per
///   theme.
/// - `anchor_handle` is a single themed lucide-anchor SVG handle (no
///   rotation — the rope's swing lives on the canvas, not in the
///   sprite). Rebuilt only on theme change, sharing `handle_generation`
///   with the boat cache so a single bump invalidates both atomically.
/// - `handle_generation` is the `theme::theme_generation()` snapshot taken
///   at the time the cache was last populated. When the global counter
///   advances — any path that runs `theme::reload_theme()` or
///   `theme::set_light_mode()` — the cache is recognized as stale and
///   cleared on the next access. This replaces the previous explicit-
///   invalidation approach, which had to be wired up at every
///   theme-change call site and missed preset switches.
#[derive(Debug, Clone, Default)]
pub struct BoatState {
    pub x_ratio: f32,
    pub y_ratio: f32,
    pub x_velocity: f32,
    pub y_velocity: f32,
    pub tilt: f32,
    pub tilt_velocity: f32,
    pub facing: f32,
    pub visible: bool,
    pub last_tick: Option<Instant>,
    pub secs_until_next_tack: f32,
    pub secs_until_next_anchor: f32,
    pub anchor_remaining_secs: f32,
    /// `x_ratio` of the boat at the moment the current anchor event
    /// fired, captured so the anchor sprite stays put on the ocean
    /// floor even as the boat continues to bob and drift slightly.
    /// Only meaningful while `anchor_remaining_secs > 0`; default `0.0`
    /// is harmless because the renderer gates on the remaining-secs
    /// field anyway.
    pub anchor_drop_x: f32,
    pub anchor_sway: f32,
    pub anchor_sway_velocity: f32,
    pub sway_phase: f32,
    /// Beat-locked oscillator phase in `[0, 1)`, advancing at
    /// `bpm / 60` Hz when the current song has a tagged BPM. Drives
    /// `beat_factor = max(0, sin(2π · beat_phase))^2` for the sail-
    /// thrust modulation. Holds while `bpm` is `None` so the next
    /// tagged track resumes cleanly mid-phase rather than snapping to
    /// zero.
    pub beat_phase: f32,
    /// Seconds since the last tack flipped `facing`, or `None` when no
    /// tack has fired yet (or the most recent tack's ramp has
    /// completed). When `Some(secs)`, sail thrust + the velocity floor
    /// are scaled by `(secs / TACK_RAMP_SECS).clamp(0, 1)` — at the
    /// moment of a tack thrust is 0 (boat decelerates on damping
    /// alone), and over the next `TACK_RAMP_SECS` the thrust ramps
    /// back to full so the boat accelerates smoothly onto its new
    /// heading. Once the ramp completes the field returns to `None`,
    /// representing full-thrust steady state.
    pub secs_since_tack: Option<f32>,
    /// Throttle accumulator for the periodic music-signal diagnostic
    /// trace. Increments by `dt` each tick; when it crosses 1.0 we
    /// emit one `trace!` line summarizing the music modulation
    /// values and reset to 0. Lets a user grep `nokkvi.log` for
    /// `boat-music` to verify the physics is seeing the signals
    /// they expect, at one line per second instead of per-frame
    /// spam.
    pub trace_accum: f32,
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
    /// Single themed anchor-body SVG, rebuilt only on theme change.
    /// (The anchor doesn't rotate — the rope's sway lives in the canvas
    /// path, not the SVG, so we don't need a per-quantized-angle map
    /// like the boat does.)
    pub anchor_handle: Option<svg::Handle>,
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
    /// they were built. Called by `cache_handle_for` and `cache_anchor_handle`
    /// before any insert, so both caches stay consistent with
    /// `theme::theme_generation()` without needing explicit invalidation
    /// hooks at every theme-change site (preset switch, color picker edit,
    /// restore-defaults, etc.).
    fn clear_if_theme_changed(&mut self) {
        let current_gen = crate::theme::theme_generation();
        if self.handle_generation != current_gen {
            self.tilt_handles.clear();
            self.anchor_handle = None;
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

    /// Build (and cache) the themed anchor SVG handle. Single entry per
    /// theme generation — the anchor doesn't rotate, so there's nothing
    /// to key on. Mirrors `cache_handle_for`'s theme-generation
    /// invalidation via `clear_if_theme_changed`.
    pub(crate) fn cache_anchor_handle(&mut self) -> svg::Handle {
        self.clear_if_theme_changed();
        if let Some(h) = &self.anchor_handle {
            return h.clone();
        }
        let bytes = crate::embedded_svg::themed_anchor_svg().into_bytes();
        let h = svg::Handle::from_memory(bytes);
        self.anchor_handle = Some(h.clone());
        h
    }

    /// Read-only sibling of `cache_anchor_handle` — same fall-through
    /// contract as `cached_handle_for` for the boat. The render path uses
    /// it for the on-screen lookup and falls back to a rebuild on miss.
    pub(crate) fn cached_anchor_handle(&self) -> Option<svg::Handle> {
        let current_gen = crate::theme::theme_generation();
        if self.handle_generation != current_gen {
            return None;
        }
        self.anchor_handle.clone()
    }
}

/// Step the boat physics forward by `dt`, sampling slope and target height
/// from `bars`. Mutates `x_velocity`, `x_ratio`, `y_velocity`, `y_ratio`,
/// `tilt` / `tilt_velocity`, `facing`, `secs_until_next_tack`,
/// `secs_until_next_anchor`, `anchor_remaining_secs`, `anchor_sway` /
/// `anchor_sway_velocity`, `sway_phase`, `beat_phase`, and `rng_state`
/// on `state`.
///
/// Forces on `x` (semi-implicit Euler):
/// - sail thrust: `facing · MAX_SAIL_THRUST · total_intensity` while
///   sailing, `0` while anchored. `total_intensity ∈ [0, 1]` is
///   composed from music signals (cruise + beat + onset) — silence
///   gives 0 intensity, so silence gives 0 thrust. There is no
///   constant baseline thrust.
/// - slope: `(-slope · SLOPE_GAIN · surf_gate).clamp(±MAX_SLOPE_FORCE)` —
///   downhill push, capped well below `MAX_SAIL_THRUST` so an uphill
///   can slow but not reverse the boat. The `surf_gate` (lerped over
///   `SLOPE_GATE_FLOOR` → `+RAMP`) zeroes the force in low-amplitude
///   regions so the boat doesn't surf calm water. Suppressed entirely
///   in the wrap margin and while anchored.
/// - damping: `-x_velocity · X_DAMPING` — friction. With no other
///   force (silence, anchored), this is what brings the boat to rest.
///
/// `x_velocity` is clamped into `[-MAX_X_V, MAX_X_V]` after every
/// integration. A music-scaled velocity floor (`MIN_SAILING_VELOCITY ·
/// total_intensity`) is then asserted ONLY when `x_velocity` and
/// `facing` have the same sign — so when a tack flips facing,
/// momentum in the old direction decelerates through zero (via
/// damping + reversed sail thrust) before the floor re-engages on the
/// new heading. This produces a smooth, visible deceleration during
/// a turn rather than an instant teleport from one cruise speed to
/// its mirror.
///
/// Y is a spring-damper tracking `target_y = sample_line_height(...)`:
/// `ay = (target_y - y) · Y_SPRING_K - y_velocity · Y_DAMPING`. The boat
/// bobs over the waves it crosses — wave height carries it vertically
/// while sail thrust carries it horizontally.
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
/// `facing` is seeded to a random ±1 on the first tick (when defaulted
/// to `0.0`), then flipped only by tack events: every
/// `[TACK_INTERVAL_MIN_SECS, TACK_INTERVAL_MAX_SECS]` seconds the wind
/// shifts. The countdown only ticks down in the visible area AND while
/// not anchored, so neither a tack mid-margin nor a tack mid-anchor can
/// fire (sail thrust would briefly disagree with the latched eject
/// direction in the first case, the wind shouldn't shift while the
/// captain naps in the second).
///
/// Anchor events fire every `[ANCHOR_INTERVAL_MIN_SECS,
/// ANCHOR_INTERVAL_MAX_SECS]` seconds (much rarer than tacks) and last
/// `[ANCHOR_DURATION_MIN_SECS, ANCHOR_DURATION_MAX_SECS]` seconds. Like
/// tacks, anchors don't fire in the wrap margin (the doodad would
/// happen off-screen). While anchored, sail thrust + the velocity floor
/// are off so the boat coasts to a stop, but Y-bobbing and tilt continue
/// so the boat catches the beat as waves roll past underneath. The rope
/// sway is a spring-damper toward `local_height · MAX_ANCHOR_SWAY ·
/// sin(2π · sway_phase)`, gated below `ANCHOR_SWAY_AMPLITUDE_FLOOR` so
/// calm spectrums leave the rope hanging straight. `facing` is preserved
/// across the anchor lift — the wind didn't change while the boat
/// rested.
///
/// `x_ratio` is wrapped over `[-x_wrap_margin, 1 + x_wrap_margin)` so the
/// boat sails fully off one edge — through a hidden stretch sized to a
/// boat-and-a-bit — before reappearing on the opposite side with momentum
/// intact. `y_ratio` still clamps to `[0, 1]` (with outward velocity
/// zeroed) since the wave height is bounded — there's no toroidal Y to
/// wrap into.
pub(crate) fn step(
    state: &mut BoatState,
    dt: Duration,
    bars: &[f64],
    angular: bool,
    music: MusicSignals,
) {
    let dt_secs = dt.as_secs_f32();
    if dt_secs <= 0.0 {
        return;
    }

    // Lazy state init. `Default` leaves `rng_state` and `facing` at 0; the
    // xorshift PRNG can't produce 0 from any non-zero seed, so we use it
    // as a sentinel and seed all randomized fields here. `secs_until_next_tack`
    // and `secs_until_next_anchor` are also reseeded if they start at zero
    // (not explicitly initialized) so a `Default`-built `BoatState` doesn't
    // immediately tack or anchor on its first tick.
    if state.rng_state == 0 {
        state.rng_state = 0x9E37_79B9;
    }
    if state.facing == 0.0 {
        state.facing = pick_facing(&mut state.rng_state);
    }
    if state.secs_until_next_tack <= 0.0 && state.anchor_remaining_secs <= 0.0 {
        let r = next_rand_unit(&mut state.rng_state);
        state.secs_until_next_tack = lerp(TACK_INTERVAL_MIN_SECS, TACK_INTERVAL_MAX_SECS, r);
    }
    if state.secs_until_next_anchor <= 0.0 && state.anchor_remaining_secs <= 0.0 {
        let r = next_rand_unit(&mut state.rng_state);
        state.secs_until_next_anchor = lerp(ANCHOR_INTERVAL_MIN_SECS, ANCHOR_INTERVAL_MAX_SECS, r);
    }

    // The off-screen wrap margin is "deadspace" — slope force is muted
    // here so wave gradients at the seam can't drag the boat back into
    // the edge it just left. Sail thrust + the velocity floor carry the
    // boat through the deadspace at terminal velocity in its facing
    // direction; no special margin force is needed.
    let in_margin = !(0.0..=1.0).contains(&state.x_ratio);

    // Slope sampling wraps toroidally to match the boat's wrap geometry.
    // Without this, both samples near the seam clamp to the low-energy
    // edge bins (sub-bass on the left, top-treble on the right — quiet in
    // most music), forming a `\/` basin whose downhill slope force would
    // briefly point back across the seam. Wrapping reads symmetric edges
    // as flat (slope ≈ 0), so the boat doesn't get yanked at the wrap.
    // Inside `(0, 1)` `rem_euclid` is a no-op, so mid-screen behavior is
    // unchanged.
    let h_left = sample_line_height(bars, (state.x_ratio - SLOPE_DX).rem_euclid(1.0), angular);
    let h_right = sample_line_height(bars, (state.x_ratio + SLOPE_DX).rem_euclid(1.0), angular);
    let slope = (h_right - h_left) / (2.0 * SLOPE_DX);

    // Gate the horizontal slope force on local wave height: full force on
    // visible wave faces, fading to zero in low-energy regions. Tilt is
    // left ungated so the boat still leans on whatever wisp of curve it's
    // on; only the horizontal pull is suppressed.
    let local_height = sample_line_height(bars, state.x_ratio, angular);
    let surf_gate = ((local_height - SLOPE_GATE_FLOOR) / SLOPE_GATE_RAMP).clamp(0.0, 1.0);
    let slope_force = if in_margin || state.anchor_remaining_secs > 0.0 {
        // Two cases zero out slope force:
        // - Deadspace (in_margin): the boat is between the visible
        //   boundary and the wrap seam, where toroidal slope sampling
        //   reads "across the seam" and routinely points back toward
        //   the edge the boat just crossed. Zeroing it here keeps that
        //   residual force from re-grabbing the boat before the eject
        //   can flush it through.
        // - Anchored: the anchor is supposed to hold the boat in place;
        //   slope force on tall waves would slowly drag the boat away
        //   from the dropped anchor, breaking the visual contract.
        0.0
    } else {
        // Slope force only RESISTS motion — never assists. Going down a
        // wave should not accelerate the boat (a sailboat doesn't
        // surf the way a board does). Going up a wave still produces
        // a headwind that slows the boat. We compute the raw slope
        // force first, then mask off any component that's in the
        // SAME direction as `facing` (i.e., a tailwind that would
        // speed the boat up).
        let raw = (-slope * SLOPE_GAIN * surf_gate).clamp(-MAX_SLOPE_FORCE, MAX_SLOPE_FORCE);
        if state.facing > 0.0 {
            raw.min(0.0)
        } else if state.facing < 0.0 {
            raw.max(0.0)
        } else {
            0.0
        }
    };

    // Anchor state machine: either anchored (anchor_remaining_secs > 0)
    // or counting down to the next anchor. The anchor doesn't fire in
    // the off-screen wrap margin (the doodad would happen out of sight)
    // — same gate the tack uses. When the anchor lifts we reseed the
    // next-anchor countdown so the boat doesn't immediately drop again.
    if state.anchor_remaining_secs > 0.0 {
        state.anchor_remaining_secs -= dt_secs;
        if state.anchor_remaining_secs <= 0.0 {
            state.anchor_remaining_secs = 0.0;
            let r = next_rand_unit(&mut state.rng_state);
            state.secs_until_next_anchor =
                lerp(ANCHOR_INTERVAL_MIN_SECS, ANCHOR_INTERVAL_MAX_SECS, r);
        }
    } else if !in_margin {
        let in_safe_zone = state.x_ratio > ANCHOR_SAFE_LO && state.x_ratio < ANCHOR_SAFE_HI;
        state.secs_until_next_anchor -= dt_secs;
        if state.secs_until_next_anchor <= 0.0 && in_safe_zone {
            let r = next_rand_unit(&mut state.rng_state);
            state.anchor_remaining_secs =
                lerp(ANCHOR_DURATION_MIN_SECS, ANCHOR_DURATION_MAX_SECS, r);
            // Capture where the anchor dropped. Renderer pins the
            // anchor sprite to this x for the entire event so the
            // anchor stays planted even as Y-bobbing carries the boat
            // above it.
            state.anchor_drop_x = state.x_ratio;
            // The rope catches the boat: stop forward momentum
            // immediately so the boat doesn't drift toward the wrap
            // margin during the anchor (which would render as a rope
            // stretching across the entire visible area when the boat
            // re-emerges on the opposite side).
            state.x_velocity = 0.0;
        }
    }
    let anchored = state.anchor_remaining_secs > 0.0;

    // Beat phase: advance at `bpm/60` Hz when the current song has a
    // tagged BPM, hold otherwise so the next tagged track resumes
    // mid-phase rather than snapping back to zero. The phase is in
    // `[0, 1)` and feeds the half-sine envelope below; we wrap with
    // `rem_euclid` to stay in range across long sessions.
    if let Some(bpm) = music.bpm {
        let beat_hz = bpm as f32 / 60.0;
        state.beat_phase = (state.beat_phase + dt_secs * beat_hz).rem_euclid(1.0);
    }

    // Music-driven thrust composition. The boat is purely propelled by
    // music — silence produces zero sail force AND zero velocity floor,
    // so the boat coasts to rest with no audio. Three components feed a
    // single `total_intensity ∈ [0, 2]`:
    //
    // 1. **Cruise** — slow onset envelope (`long_onset`), lifted above a
    //    noise floor and amplified, then optionally scaled by tagged
    //    BPM. This is the song-level energy contour: an energetic
    //    track sits at high cruise, an acoustic track sits low. Different
    //    songs end up at visibly different cruise levels.
    // 2. **Beat** — half-sine-squared envelope keyed to `beat_phase`,
    //    only active when BPM is tagged. Pulses each downbeat.
    // 3. **Onset** — instantaneous spectral flux. Surges on transients.
    //
    // Sail thrust and velocity floor scale linearly by `total_intensity`,
    // so the boat's behavior tracks the music end-to-end with no
    // hardcoded "always-on" baseline.
    let bpm_scale = music.bpm.map_or(1.0, |bpm| {
        (bpm as f32 / REFERENCE_BPM).clamp(BPM_SCALE_MIN, BPM_SCALE_MAX)
    });
    // Flux-based cruise: positive spectral flux EMA. Strong on
    // percussive / transient-rich material, ~0 on sustained pads.
    let lifted_long_onset = (music.long_onset_energy - LONG_ONSET_FLOOR).max(0.0);
    let flux_cruise = (lifted_long_onset * LONG_ONSET_AMP * bpm_scale).clamp(0.0, 1.0);
    // Presence-based cruise: average bar height. Strong on sustained
    // material (pads, drones), ~0 only at true silence. Both inputs
    // share `bpm_scale` so a tagged tempo lifts whichever path is
    // active for the current song.
    let lifted_presence = (music.bar_energy - PRESENCE_FLOOR).max(0.0);
    let presence_cruise = (lifted_presence * PRESENCE_AMP * bpm_scale).clamp(0.0, 1.0);
    // Take the max so whichever cruise signal has more to say wins —
    // sum/blend would dilute each one's strength on material it's
    // good at (a punchy techno track shouldn't read slower than a
    // pad-only ambient track just because flux + presence happen to
    // saturate at the same combined level).
    let cruise_intensity = flux_cruise.max(presence_cruise);
    let beat_intensity = if music.bpm.is_some() {
        let s = (state.beat_phase * std::f32::consts::TAU).sin();
        if s > 0.0 { s * s } else { 0.0 }
    } else {
        0.0
    };
    let onset_intensity = music.onset_energy.clamp(0.0, 1.0);
    // `total_intensity` clamps at 2.0, not 1.0. Cruise saturates at 1.0
    // (it's a unit signal), but beat + onset can stack ~1.0 of
    // additional thrust on top — so an already-saturating-cruise
    // black-metal track with full-onset blast beats reads visibly
    // faster than a same-cruise techno track with quieter onsets,
    // instead of both pinning at the same ceiling.
    let total_intensity =
        (cruise_intensity + BEAT_AMP * beat_intensity + ONSET_AMP * onset_intensity)
            .clamp(0.0, 2.0);

    // Tack ramp: while `secs_since_tack` is `Some`, scale sail thrust
    // + velocity floor by `(secs / TACK_RAMP_SECS).clamp(0, 1)`. Right
    // at the flip moment the value is `Some(0.0)`, so sail thrust is 0
    // and damping alone decelerates the boat through zero. The value
    // climbs back to 1 over the ramp window, letting the boat
    // accelerate smoothly onto its new heading. Once the ramp finishes
    // the field returns to `None` — full thrust steady state, no
    // ramp scaling.
    let tack_progress = match state.secs_since_tack {
        None => 1.0,
        Some(secs) => {
            let next = secs + dt_secs;
            if next >= TACK_RAMP_SECS {
                state.secs_since_tack = None;
                1.0
            } else {
                state.secs_since_tack = Some(next);
                (next / TACK_RAMP_SECS).clamp(0.0, 1.0)
            }
        }
    };

    // While anchored: sail thrust is off (the wind doesn't push a
    // stopped boat) and the velocity floor below is suspended (so
    // damping can take `x_velocity` to zero without the floor
    // reasserting forward motion). Music thrust scales with intensity,
    // so silence is also automatically a no-thrust state.
    let sail_force = if anchored {
        0.0
    } else {
        state.facing * MAX_SAIL_THRUST * total_intensity * tack_progress
    };
    let damping_force = -state.x_velocity * X_DAMPING;

    let ax = sail_force + slope_force + damping_force;
    state.x_velocity = (state.x_velocity + ax * dt_secs).clamp(-MAX_X_V, MAX_X_V);

    // Forward-velocity floor — applied only when current velocity is
    // ALIGNED with `facing` (same sign or zero). When a tack flips
    // facing, velocity carries momentum in the OLD direction; we let
    // damping decelerate it through zero before the floor re-engages
    // on the new heading. That produces a smooth, visible deceleration
    // during a turn rather than the boat snapping from one cruise speed
    // to its mirror. Floor scales linearly by `total_intensity` (capped
    // at 1.0 even when stacking pushes total_intensity higher) so
    // silence gives a 0 floor and the floor doesn't spike weirdly on
    // brick-walled tracks — stacking lifts the *ceiling* (terminal
    // velocity), not the *floor* (minimum cruise). Also scales by
    // `tack_progress` so it ramps in alongside sail thrust after a tack.
    let effective_floor = MIN_SAILING_VELOCITY * total_intensity.min(1.0) * tack_progress;
    if !anchored {
        if state.facing > 0.0 && state.x_velocity >= 0.0 && state.x_velocity < effective_floor {
            state.x_velocity = effective_floor;
        } else if state.facing < 0.0
            && state.x_velocity <= 0.0
            && state.x_velocity > -effective_floor
        {
            state.x_velocity = -effective_floor;
        }
    }

    // Per-second diagnostic trace. Lets a user verify the music
    // signals are flowing — `RUST_LOG=nokkvi::widgets::boat=trace`
    // (or any wider trace filter) in the env emits one line per
    // second with the modulation values feeding the boat. Rate-limited
    // so we never spam the log at 60 Hz.
    state.trace_accum += dt_secs;
    if state.trace_accum >= 1.0 {
        state.trace_accum = 0.0;
        trace!(
            target: "nokkvi::boat::music",
            bpm = ?music.bpm,
            onset = music.onset_energy,
            long_onset = music.long_onset_energy,
            bar_energy = music.bar_energy,
            bpm_scale,
            flux_cruise,
            presence_cruise,
            cruise_intensity,
            beat_intensity,
            onset_intensity,
            total_intensity,
            effective_floor,
            x_velocity = state.x_velocity,
            facing = state.facing,
            "boat-music diagnostic"
        );
    }

    // Wrap x_ratio over the extended span `[-x_wrap_margin, 1 + x_wrap_margin)`
    // so the boat slides fully off one edge before reappearing on the other.
    // Margin defaults to 0.0 (collapses to `rem_euclid(1.0)`), which is what
    // the standalone physics tests construct. Sail thrust keeps the boat
    // moving through the margin at terminal velocity in its facing
    // direction, so traversal completes naturally without any
    // margin-specific force.
    let span = 1.0 + 2.0 * state.x_wrap_margin;
    let raw_x = state.x_ratio + state.x_velocity * dt_secs;
    state.x_ratio = (raw_x + state.x_wrap_margin).rem_euclid(span) - state.x_wrap_margin;

    // Tack countdown. Only ticks while the boat is in the visible area
    // and not anchored — a tack mid-margin would have sail thrust briefly
    // opposing the latched eject direction (a visible "boat hesitates
    // while crossing the seam" hiccup), and a tack mid-anchor would
    // change the heading the boat resumes on after the rest stop, which
    // breaks the "the wind didn't change while you napped" intuition.
    if !in_margin && !anchored {
        state.secs_until_next_tack -= dt_secs;
        if state.secs_until_next_tack <= 0.0 {
            state.facing = -state.facing;
            // Reset the ramp clock so sail thrust + floor drop to 0
            // and ramp back over `TACK_RAMP_SECS` — the visible-
            // gradual turn.
            state.secs_since_tack = Some(0.0);
            let r = next_rand_unit(&mut state.rng_state);
            state.secs_until_next_tack = lerp(TACK_INTERVAL_MIN_SECS, TACK_INTERVAL_MAX_SECS, r);
        }
    }

    // Anchor sway: spring-damper toward a wave-driven target. The drive
    // is `local_height_factor · MAX_ANCHOR_SWAY · sin(2π · sway_phase)`,
    // so on tall waves the rope swings up to ~MAX_ANCHOR_SWAY, on calm
    // water the target stays at 0 and the spring eases the rope back
    // straight. The drive is only non-zero while anchored — between
    // sessions the spring decays whatever sway was left over toward 0
    // so a fresh anchor doesn't start mid-swing.
    state.sway_phase = (state.sway_phase + dt_secs * ANCHOR_SWAY_DRIVE_HZ).rem_euclid(1.0);
    let sway_amplitude = if anchored {
        ((local_height - ANCHOR_SWAY_AMPLITUDE_FLOOR) / (1.0 - ANCHOR_SWAY_AMPLITUDE_FLOOR))
            .clamp(0.0, 1.0)
    } else {
        0.0
    };
    let sway_target =
        sway_amplitude * MAX_ANCHOR_SWAY * (state.sway_phase * std::f32::consts::TAU).sin();
    let a_sway = (sway_target - state.anchor_sway) * ANCHOR_SWAY_SPRING_K
        - state.anchor_sway_velocity * ANCHOR_SWAY_DAMPING;
    state.anchor_sway_velocity += a_sway * dt_secs;
    state.anchor_sway += state.anchor_sway_velocity * dt_secs;
    if state.anchor_sway > MAX_ANCHOR_SWAY {
        state.anchor_sway = MAX_ANCHOR_SWAY;
        if state.anchor_sway_velocity > 0.0 {
            state.anchor_sway_velocity = 0.0;
        }
    } else if state.anchor_sway < -MAX_ANCHOR_SWAY {
        state.anchor_sway = -MAX_ANCHOR_SWAY;
        if state.anchor_sway_velocity < 0.0 {
            state.anchor_sway_velocity = 0.0;
        }
    }

    // Tilt: spring-damper toward `-slope * gain`, clamped. The sign is
    // negated because the angle ultimately feeds an SVG `rotate(deg, cx,
    // cy)` transform, and SVG rotation is clockwise for positive degrees
    // in screen coords (Y-down); a positive slope means uphill to the
    // right, which we want the boat to lean *into* (right side up =
    // counterclockwise = negative angle). The hard `MAX_TILT` clamp
    // after the spring step is the same pattern as y_ratio uses: the
    // underdamped spring would otherwise overshoot the target by a few
    // percent on sharp transients, which would visually exceed the cap.
    // In the deadspace there's no wave under the boat, so target a
    // neutral tilt and let the spring-damper ease the boat to flat as
    // it transits the margin.
    let target_tilt = if in_margin {
        0.0
    } else {
        (-slope * TILT_GAIN).clamp(-MAX_TILT, MAX_TILT)
    };
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

/// Pick an initial facing (`±1`) with a fair coin flip. Used to seed
/// `BoatState.facing` on the first tick when the field is at its
/// `Default` value of `0.0`, and to pick a fresh facing after each tack
/// (currently a deterministic flip rather than a re-roll, but `pick_facing`
/// is the single point where any future weighting would land).
fn pick_facing(rng_state: &mut u32) -> f32 {
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
    let mut overlay = Stack::new().push(pin_at(target_x));

    // Anchor sprite + rope canvas: only rendered while anchored. The
    // anchor sprite sits at the bottom of the visualizer area, pinned
    // to the x where the boat dropped the anchor — it does NOT move
    // with the boat, so wave-driven Y-bobbing leaves the anchor
    // planted on the floor while the boat rides above it. The rope is
    // a curved canvas path drawn from the boat's bottom-center to the
    // top of the anchor's ring; its bend is driven by `anchor_sway`,
    // which the physics still oscillates from local wave amplitude.
    if state.anchor_remaining_secs > 0.0 {
        let anchor_handle = state.cached_anchor_handle().unwrap_or_else(|| {
            let bytes = crate::embedded_svg::themed_anchor_svg().into_bytes();
            svg::Handle::from_memory(bytes)
        });

        // Anchor sprite sized as a fraction of the boat — small enough
        // that it reads as a doodad rather than a second focal point.
        let anchor_total_h = boat_h * ANCHOR_HEIGHT_MULTIPLE_OF_BOAT;
        let anchor_total_w = anchor_total_h; // lucide anchor's viewBox is square
        let anchor_left_x = state.anchor_drop_x * area_width - anchor_total_w * 0.5;
        let anchor_top_y = area_height - anchor_total_h;

        overlay = overlay.push(
            OverflowPin::new(
                container(
                    Svg::new(anchor_handle)
                        .width(Length::Fill)
                        .height(Length::Fill),
                )
                .width(Length::Fixed(anchor_total_w))
                .height(Length::Fixed(anchor_total_h)),
            )
            .position(Point::new(anchor_left_x, anchor_top_y)),
        );

        // Rope canvas: draws a single quadratic Bezier from the boat's
        // bottom-center to the top of the anchor's ring. The control
        // point sits at the rope's midpoint, offset perpendicular to
        // the rope axis by `anchor_sway · rope_length` so the bend
        // amplitude scales with the rope's current length (longer rope
        // = bigger swing arc, which reads more naturally than a fixed
        // pixel offset on a stretched line).
        let viz_colors = crate::theme::get_visualizer_colors();
        let rope_color =
            parse_hex_color(&viz_colors.border_color).unwrap_or(Color::from_rgb(0.5, 0.5, 0.5));
        let rope_alpha = viz_colors.border_opacity;

        let boat_bottom_x = cx;
        let boat_bottom_y = target_y + boat_h - boat_h * BOAT_SINK_FRACTION;
        let anchor_ring_x = state.anchor_drop_x * area_width;
        let anchor_ring_y =
            anchor_top_y + anchor_total_h * crate::embedded_svg::anchor_svg_ring_top_fraction();

        let rope = RopeCanvas {
            start: Point::new(boat_bottom_x, boat_bottom_y),
            end: Point::new(anchor_ring_x, anchor_ring_y),
            sway: state.anchor_sway,
            stroke_color: Color {
                a: rope_alpha,
                ..rope_color
            },
            stroke_width: ROPE_STROKE_WIDTH_PX,
        };
        overlay = overlay.push(
            canvas::Canvas::new(rope)
                .width(Length::Fixed(area_width))
                .height(Length::Fixed(area_height)),
        );
    }

    container(overlay)
        .width(Length::Fixed(area_width))
        .height(Length::Fixed(area_height))
        .clip(true)
        .into()
}

/// Stroke width of the rope canvas path, in display pixels. Sized to
/// match the boat outline's apparent visual weight (~0.5–1 px after
/// the SVG-to-pixel ratio shakes out at typical visualizer sizes).
const ROPE_STROKE_WIDTH_PX: f32 = 1.5;

/// Parse a `#rrggbb` hex color string into an `iced::Color`. Returns
/// `None` if the string isn't a 7-character hex form. The visualizer
/// theme's `border_color` is always emitted in this form by
/// `embedded_svg::color_to_hex`, so this is the inverse — used by the
/// rope canvas to translate a string-formatted theme color back into
/// an iced color for `Stroke`.
fn parse_hex_color(s: &str) -> Option<Color> {
    let s = s.strip_prefix('#')?;
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some(Color::from_rgb(
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
    ))
}

/// Canvas program for the anchor rope. Draws a single quadratic Bezier
/// from `start` to `end`, with the control point at the midpoint
/// offset perpendicular to the rope axis by `sway · rope_length`. The
/// physics in `step()` drives `sway` from local wave amplitude, so a
/// loud spectrum produces a visibly bowing rope while calm music
/// leaves it nearly straight.
struct RopeCanvas {
    start: Point,
    end: Point,
    sway: f32,
    stroke_color: Color,
    stroke_width: f32,
}

impl<Message> canvas::Program<Message> for RopeCanvas {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        // Rope axis: from start to end. Length and unit-perpendicular
        // give us the bend offset direction. Perpendicular is rotate
        // axis 90° CCW: `(dx, dy) → (-dy, dx)`. Sign of `sway` picks
        // which side the bow hangs on — positive sway = bow to the
        // right (canvas x+), matching the boat's convention.
        let dx = self.end.x - self.start.x;
        let dy = self.end.y - self.start.y;
        let length = (dx * dx + dy * dy).sqrt();
        if length <= 0.0 {
            return Vec::new();
        }
        let perp_x = -dy / length;
        let perp_y = dx / length;
        let bend_offset = self.sway * length;
        let mid_x = (self.start.x + self.end.x) * 0.5 + perp_x * bend_offset;
        let mid_y = (self.start.y + self.end.y) * 0.5 + perp_y * bend_offset;

        let path = canvas::Path::new(|builder| {
            builder.move_to(self.start);
            builder.quadratic_curve_to(Point::new(mid_x, mid_y), self.end);
        });

        frame.stroke(
            &path,
            canvas::Stroke::default()
                .with_color(self.stroke_color)
                .with_width(self.stroke_width)
                .with_line_cap(canvas::LineCap::Round),
        );

        vec![frame.into_geometry()]
    }
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

    /// A saturating music signal — boat is propelled by music in the
    /// new model, so most physics tests need a non-zero `MusicSignals`
    /// to produce any motion at all. `music_on()` sets every signal to
    /// its saturating value so `total_intensity` clamps to the cap,
    /// putting sail thrust and the velocity floor at full strength.
    /// Tests that explicitly care about silence (or about individual
    /// signals) build their own `MusicSignals` literal.
    fn music_on() -> MusicSignals {
        MusicSignals {
            bpm: None,
            onset_energy: 1.0,
            long_onset_energy: 1.0,
            bar_energy: 1.0,
        }
    }

    /// Run `step` repeatedly with a small dt, returning the final state.
    /// Defaults to a saturating music signal so legacy motion tests
    /// (which were written under the old "constant baseline thrust"
    /// model) keep producing motion under the new music-driven model.
    fn run(initial: BoatState, ticks: usize, dt: Duration, bars: &[f64]) -> BoatState {
        let mut state = initial;
        for _ in 0..ticks {
            step(&mut state, dt, bars, false, music_on());
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
        step(
            &mut state,
            Duration::ZERO,
            &bars,
            false,
            MusicSignals::default(),
        );
        assert_eq!(state.x_ratio, snapshot.x_ratio);
        assert_eq!(state.x_velocity, snapshot.x_velocity);
        assert_eq!(state.facing, snapshot.facing);
    }

    #[test]
    fn step_drives_motion_from_rest_on_flat_waves() {
        // Flat bars + center start: sail thrust along the seeded facing must
        // move the boat measurably off-center within a couple of seconds.
        let bars = vec![0.5; 16];
        let initial = BoatState {
            x_ratio: 0.5,
            ..Default::default()
        };
        let final_state = run(initial, 600, Duration::from_millis(10), &bars);
        assert!(
            (final_state.x_ratio - 0.5).abs() > 0.01,
            "sail thrust should have moved the boat off-center after 6s (got {x})",
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
    fn step_slope_decelerates_against_uphill() {
        // The slope force still has a directional effect — it just can't
        // dominate sail thrust under the new model. Sailing right (facing
        // = +1) into an upward ramp must produce *slower* terminal velocity
        // than sailing right across flat water. Same starting state for
        // both runs; only the bar shape differs.
        let flat = vec![0.5_f64; 16];
        let uphill: Vec<f64> = (0..16).map(|i| i as f64 / 15.0).collect();

        let template = BoatState {
            x_ratio: 0.5,
            x_velocity: 0.0,
            facing: 1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            ..Default::default()
        };

        let mut state_flat = template.clone();
        for _ in 0..30 {
            step(
                &mut state_flat,
                Duration::from_millis(16),
                &flat,
                false,
                music_on(),
            );
        }
        let mut state_uphill = template;
        for _ in 0..30 {
            step(
                &mut state_uphill,
                Duration::from_millis(16),
                &uphill,
                false,
                music_on(),
            );
        }

        assert!(
            state_uphill.x_velocity < state_flat.x_velocity,
            "uphill slope must reduce forward velocity vs. flat water \
             (flat v = {}, uphill v = {})",
            state_flat.x_velocity,
            state_uphill.x_velocity
        );
        assert!(
            state_uphill.x_velocity > 0.0,
            "but slope must NOT reverse the boat — sail thrust + floor \
             keep it moving forward (got uphill v = {})",
            state_uphill.x_velocity
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
            facing: 1.0,
            // Suppress tacks so direction changes come from physics only.
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            ..Default::default()
        };

        // dt large enough that x_velocity * dt clearly exceeds (1.0 - 0.99).
        // 0.15 * 0.5 = 0.075 → unwrapped position would be ~1.065.
        step(
            &mut state,
            Duration::from_millis(500),
            &bars,
            false,
            MusicSignals::default(),
        );

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
            facing: -1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            ..Default::default()
        };

        step(
            &mut state,
            Duration::from_millis(500),
            &bars,
            false,
            MusicSignals::default(),
        );

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
    fn step_sail_thrust_drives_boat_through_deadspace() {
        // Replaces the old eject-force test: with no eject, the boat
        // must still traverse the off-screen wrap margin via sail
        // thrust + velocity floor alone. Place the boat just inside
        // the right margin facing right; over a few seconds it must
        // wrap and re-enter the visible area on the left side.
        let bars = vec![0.5; 16];
        let initial = BoatState {
            x_ratio: 1.01,
            x_velocity: 0.0,
            facing: 1.0,
            x_wrap_margin: 0.05,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            secs_until_next_anchor: 1e6,
            ..Default::default()
        };
        let final_state = run(initial, 400, Duration::from_millis(10), &bars);

        assert!(
            (0.0..=1.0).contains(&final_state.x_ratio),
            "boat must traverse the margin via sail thrust alone and \
             re-enter the visible area (final x_ratio = {})",
            final_state.x_ratio
        );
        assert!(
            final_state.x_ratio < 0.5,
            "post-wrap re-entry should land near the left side (got \
             x_ratio = {})",
            final_state.x_ratio
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
            secs_until_next_tack: 1e6,
            ..Default::default()
        };

        let v_before = state.x_velocity;
        step(
            &mut state,
            Duration::from_millis(50),
            &bars,
            false,
            MusicSignals::default(),
        );

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

        // Compare two runs: one with the symmetric-seam basin, one with
        // dead-flat water. With the toroidal sampling fix, slope at the
        // seam is ≈ 0, so the boat's velocity must match the flat-water
        // baseline within numerical noise. Without the fix the basin
        // would add a measurable slope contribution.
        let flat = vec![0.5_f64; bars.len()];
        let dt = Duration::from_millis(50);
        let template = BoatState {
            x_ratio: 0.0,
            x_velocity: 0.0,
            facing: 1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            ..Default::default()
        };

        let mut state_seam = template.clone();
        step(&mut state_seam, dt, &bars, false, MusicSignals::default());
        let mut state_flat = template;
        step(&mut state_flat, dt, &flat, false, MusicSignals::default());

        assert!(
            (state_seam.x_velocity - state_flat.x_velocity).abs() < 1e-3,
            "toroidal slope sampling should cancel at a symmetric seam \
             basin — x_velocity must match the flat-water baseline \
             within noise (seam v = {}, flat v = {})",
            state_seam.x_velocity,
            state_flat.x_velocity
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

        // Compare against a dead-flat baseline. With the gate active,
        // slope force is zero at this low-energy position, so the boat's
        // velocity must match the flat-water case within numerical noise.
        let flat = vec![0.01_f64; bars.len()];
        let dt = Duration::from_millis(50);
        let template = BoatState {
            // pos ≈ 8.6 in a 33-bar buffer — far enough below the peak
            // (pos 16) that local_height is the flat baseline, but the
            // right slope sample reaches up the rising face.
            x_ratio: 0.27,
            x_velocity: 0.0,
            facing: 1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            ..Default::default()
        };

        let h_local = sample_line_height(&bars, template.x_ratio, false);
        assert!(
            h_local < SLOPE_GATE_FLOOR,
            "precondition: local height must be below SLOPE_GATE_FLOOR \
             so the gate fully suppresses slope force (got {h_local}, \
             floor {SLOPE_GATE_FLOOR})"
        );

        let mut state_peaky = template.clone();
        step(&mut state_peaky, dt, &bars, false, MusicSignals::default());
        let mut state_flat = template;
        step(&mut state_flat, dt, &flat, false, MusicSignals::default());

        // Without the gate, slope force would saturate at -MAX_SLOPE_FORCE
        // and pull the boat noticeably toward the peak. With the gate the
        // peaky-bars run must match the flat baseline within numerical
        // noise.
        assert!(
            (state_peaky.x_velocity - state_flat.x_velocity).abs() < 1e-3,
            "slope force must be gated to ~0 when local_height < FLOOR \
             (peaky v = {}, flat v = {})",
            state_peaky.x_velocity,
            state_flat.x_velocity
        );
    }

    #[test]
    fn pick_facing_is_a_fair_coin_flip() {
        // Both directions must be reachable across different seeds. Use
        // production-shaped seeds (xorshift's first output is degenerate
        // for tiny seeds, so seeding from a single counter would hit one
        // side only — irrelevant in real use where the seed is the golden
        // ratio constant).
        let mut saw_left = false;
        let mut saw_right = false;
        for i in 0u32..200 {
            let mut rng = 0x9E37_79B9_u32.wrapping_add(i.wrapping_mul(0x6789_ABCD));
            match pick_facing(&mut rng) {
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
        step(
            &mut state,
            Duration::from_millis(16),
            &bars,
            false,
            MusicSignals::default(),
        );
        let y_after_one = state.y_ratio;
        assert!(
            y_after_one > 0.0 && y_after_one < 0.5,
            "y_ratio must lag the target on a single tick (got {y_after_one})"
        );

        // Many ticks (~3 s): y_ratio should be near the target.
        for _ in 0..300 {
            step(
                &mut state,
                Duration::from_millis(10),
                &bars,
                false,
                MusicSignals::default(),
            );
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
            secs_until_next_tack: 1e6,
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
        step(
            &mut state,
            Duration::from_millis(16),
            &bars,
            false,
            MusicSignals::default(),
        );
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
            secs_until_next_tack: 1e6,
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
            secs_until_next_tack: 1e6,
            ..Default::default()
        };
        let mut state = initial;
        let dt = Duration::from_millis(10);
        for _ in 0..2000 {
            step(&mut state, dt, &bars, false, MusicSignals::default());
            assert!(
                state.tilt.abs() <= MAX_TILT + 1e-3,
                "tilt must always stay within ±MAX_TILT (got {})",
                state.tilt
            );
        }
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
            step(&mut state, dt, bars, false, MusicSignals::default());
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

        step(
            &mut state,
            Duration::from_millis(16),
            &bars,
            false,
            MusicSignals::default(),
        );

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

        step(
            &mut state,
            Duration::from_millis(16),
            &bars,
            false,
            MusicSignals::default(),
        );

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

        step(
            &mut state,
            Duration::from_millis(16),
            &bars,
            false,
            MusicSignals::default(),
        );

        assert!(
            state.y_velocity < 0.0,
            "inward (negative) y_velocity at top must be preserved — \
             only outward velocity is zeroed (got {})",
            state.y_velocity
        );
    }

    // --- new sailing-model contract --------------------------------------------
    //
    // These tests pin the "Tiny Wings on water" rewrite: constant sail thrust
    // along `facing`, a forward-velocity floor that no slope can defeat, and
    // a tack timer that flips `facing` on a randomized interval. They are red
    // against the old drive-oscillator + captain-charge model and stay green
    // after the rewrite.

    #[test]
    fn step_sail_thrust_pushes_along_facing_from_rest() {
        // Flat bars, boat at rest, facing right, music intensity at the
        // cap. Sail thrust must push the boat rightward.
        let bars = vec![0.5; 16];
        let mut state = BoatState {
            x_ratio: 0.5,
            facing: 1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            ..Default::default()
        };
        step(
            &mut state,
            Duration::from_millis(50),
            &bars,
            false,
            music_on(),
        );
        assert!(
            state.x_velocity > 0.0,
            "sail thrust must push the boat in the facing direction \
             when music is playing (facing = +1, got x_velocity = {}, \
             expected > 0)",
            state.x_velocity
        );
    }

    #[test]
    fn step_sail_thrust_pushes_along_facing_left() {
        // Mirror: facing = -1 with saturating music produces leftward thrust.
        let bars = vec![0.5; 16];
        let mut state = BoatState {
            x_ratio: 0.5,
            facing: -1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            ..Default::default()
        };
        step(
            &mut state,
            Duration::from_millis(50),
            &bars,
            false,
            music_on(),
        );
        assert!(
            state.x_velocity < 0.0,
            "sail thrust must push the boat leftward when facing = -1 \
             (got x_velocity = {}, expected < 0)",
            state.x_velocity
        );
    }

    #[test]
    fn step_velocity_floor_holds_against_max_uphill() {
        // The whole point of the rewrite: no wave shape can stall the boat
        // when music is playing. Bars ramp upward steeply to the right
        // (slope force pushes left at full magnitude). With facing = +1,
        // starting at the music-scaled floor, x_velocity after the tick
        // must remain at least at the floor — slope can shave excess
        // speed but never take the boat below the floor while music is on.
        let bars: Vec<f64> = (0..16).map(|i| i as f64 / 15.0).collect();
        let mut state = BoatState {
            x_ratio: 0.5,
            x_velocity: MIN_SAILING_VELOCITY,
            facing: 1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            ..Default::default()
        };
        step(
            &mut state,
            Duration::from_millis(50),
            &bars,
            false,
            music_on(),
        );
        assert!(
            state.x_velocity >= MIN_SAILING_VELOCITY - 1e-6,
            "velocity floor must hold under max uphill slope force when \
             music is at full intensity (facing = +1, floor = {}, got \
             x_velocity = {})",
            MIN_SAILING_VELOCITY,
            state.x_velocity
        );
    }

    #[test]
    fn step_velocity_floor_holds_against_max_uphill_left() {
        // Mirror: bars ramp upward to the LEFT (slope force pushes right),
        // facing = -1, music at saturation. Floor must keep x_velocity
        // at or below -MIN_SAILING_VELOCITY.
        let bars: Vec<f64> = (0..16).map(|i| (15 - i) as f64 / 15.0).collect();
        let mut state = BoatState {
            x_ratio: 0.5,
            x_velocity: -MIN_SAILING_VELOCITY,
            facing: -1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            ..Default::default()
        };
        step(
            &mut state,
            Duration::from_millis(50),
            &bars,
            false,
            music_on(),
        );
        assert!(
            state.x_velocity <= -MIN_SAILING_VELOCITY + 1e-6,
            "velocity floor must hold under max uphill slope force \
             (facing = -1, music saturating, floor = {}, got x_velocity \
             = {})",
            -MIN_SAILING_VELOCITY,
            state.x_velocity
        );
    }

    #[test]
    fn step_boat_clears_steep_uphill_voyage() {
        // End-to-end of the user's original bug: a boat sailing right
        // across an upward ramp must traverse it without stalling — even
        // when slope force is fighting the sail. The music-scaled floor
        // guarantees forward progress every tick the music is playing.
        let bars: Vec<f64> = (0..16).map(|i| i as f64 / 15.0).collect();
        let mut state = BoatState {
            x_ratio: 0.3,
            x_velocity: 0.0,
            facing: 1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            ..Default::default()
        };
        let dt = Duration::from_millis(16);
        for _ in 0..(30 * 60) {
            step(&mut state, dt, &bars, false, music_on());
            // Bail out early if the boat has clearly crossed the segment
            // (and may have wrapped — `x_ratio` is in `[0,1)`).
            if state.x_ratio > 0.7 || state.x_ratio < 0.3 {
                return;
            }
        }
        panic!(
            "boat must clear a steep uphill voyage of 0.4 ratio in 30 s \
             at facing = +1 with music on (final x_ratio = {}, expected \
             > 0.7 or wrapped)",
            state.x_ratio
        );
    }

    #[test]
    fn step_no_back_and_forth_oscillation_on_flat_water() {
        // Sail thrust (when music is on) is unidirectional in the facing
        // direction — terminal velocity is a single sign, no zero
        // crossings during a long flat-water run.
        let bars = vec![0.5_f64; 16];
        let mut state = BoatState {
            x_ratio: 0.5,
            x_velocity: 0.0,
            facing: 1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            ..Default::default()
        };

        let dt = Duration::from_millis(16);
        for tick in 0..(10 * 60) {
            step(&mut state, dt, &bars, false, music_on());
            if tick > 5 {
                assert!(
                    state.x_velocity >= -1e-6,
                    "sail-thrust model must not produce negative velocity \
                     while facing right on flat water (tick {tick}, \
                     x_velocity = {})",
                    state.x_velocity
                );
            }
        }
    }

    #[test]
    fn step_tack_event_flips_facing() {
        // Tack timer hits zero → facing flips and a new countdown is
        // sampled. With `secs_until_next_tack` set just above the tick
        // dt, one tick advances the timer past zero and the next tick
        // observes the flip.
        let bars = vec![0.5; 16];
        let mut state = BoatState {
            x_ratio: 0.5,
            facing: 1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 0.01,
            ..Default::default()
        };
        let dt = Duration::from_millis(50);
        step(&mut state, dt, &bars, false, MusicSignals::default());
        assert_eq!(
            state.facing, -1.0,
            "tack must flip facing from +1 to -1 once the countdown \
             reaches zero (got facing = {})",
            state.facing
        );
        assert!(
            state.secs_until_next_tack >= TACK_INTERVAL_MIN_SECS - 1e-3,
            "tack must reseed a fresh countdown in [{}, {}] (got {})",
            TACK_INTERVAL_MIN_SECS,
            TACK_INTERVAL_MAX_SECS,
            state.secs_until_next_tack
        );
    }

    #[test]
    fn step_tack_does_not_fire_during_pending_interval() {
        // With a long `secs_until_next_tack` and no other source of
        // facing change, the boat must stay on its current heading.
        let bars = vec![0.5; 16];
        let mut state = BoatState {
            x_ratio: 0.5,
            facing: 1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            ..Default::default()
        };
        let dt = Duration::from_millis(16);
        for _ in 0..(5 * 60) {
            step(&mut state, dt, &bars, false, MusicSignals::default());
        }
        assert_eq!(
            state.facing, 1.0,
            "facing must hold while the tack countdown is far from zero \
             (got facing = {} after 5 s)",
            state.facing
        );
    }

    #[test]
    fn step_seeds_facing_on_first_tick_when_default() {
        // Default `BoatState` has `facing = 0.0`. The new model needs a
        // non-zero facing to apply sail thrust, so the first tick must
        // pick a side from the rng.
        let bars = vec![0.5; 16];
        let mut state = BoatState {
            x_ratio: 0.5,
            // facing left at default = 0.0
            ..Default::default()
        };
        step(
            &mut state,
            Duration::from_millis(16),
            &bars,
            false,
            MusicSignals::default(),
        );
        assert!(
            state.facing == 1.0 || state.facing == -1.0,
            "first tick must pick a facing (±1) when starting from the \
             default 0.0 (got facing = {})",
            state.facing
        );
    }

    // --- anchor doodad ----------------------------------------------------------
    //
    // The boat occasionally drops anchor for 10–15 s and hovers in place,
    // catching the beat as waves roll past. These tests pin the anchor
    // event lifecycle: it suspends X motion, leaves Y bobbing intact,
    // doesn't fire mid-margin, preserves facing on lift, and drives the
    // rope sway from the local wave amplitude.

    #[test]
    fn step_anchor_event_suspends_x_velocity() {
        // Pre-arm an anchor: with the countdown already at zero, the
        // first tick must transition into the anchored state and stop
        // applying sail thrust. Within a couple of ticks the boat's
        // x_velocity must collapse toward 0 — sail off, damping eats
        // whatever momentum was there.
        let bars = vec![0.5; 16];
        let mut state = BoatState {
            x_ratio: 0.5,
            x_velocity: 0.05,
            facing: 1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            // Pre-arm anchor: countdown ticks below 0 on first step,
            // the anchor fires, and `anchor_remaining_secs` becomes
            // some value in `[ANCHOR_DURATION_MIN, ANCHOR_DURATION_MAX]`.
            secs_until_next_anchor: 0.001,
            ..Default::default()
        };

        // First tick: anchor fires.
        step(
            &mut state,
            Duration::from_millis(16),
            &bars,
            false,
            MusicSignals::default(),
        );
        assert!(
            state.anchor_remaining_secs > 0.0,
            "anchor must fire when secs_until_next_anchor reaches zero \
             (got anchor_remaining_secs = {})",
            state.anchor_remaining_secs
        );

        // A few seconds of ticking — x_velocity must collapse to ~0
        // without sail thrust to maintain it.
        for _ in 0..(2 * 60) {
            step(
                &mut state,
                Duration::from_millis(16),
                &bars,
                false,
                MusicSignals::default(),
            );
        }
        assert!(
            state.x_velocity.abs() < MIN_SAILING_VELOCITY,
            "anchored boat must NOT be subject to the velocity floor — \
             x_velocity should decay toward 0 (got {})",
            state.x_velocity
        );
    }

    #[test]
    fn step_anchor_y_dynamics_continue_to_bob() {
        // While anchored, the boat must keep tracking the local wave
        // height — the whole point is "catch a beat" while waves pass
        // underneath. Place the boat at y=0 with a tall constant wave;
        // y_ratio must climb toward the wave height even while anchored.
        let bars = vec![0.8; 16];
        let mut state = BoatState {
            x_ratio: 0.5,
            y_ratio: 0.0,
            facing: 1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            anchor_remaining_secs: 12.0,
            secs_until_next_anchor: 1e6,
            ..Default::default()
        };

        for _ in 0..200 {
            step(
                &mut state,
                Duration::from_millis(10),
                &bars,
                false,
                MusicSignals::default(),
            );
        }
        assert!(
            state.y_ratio > 0.7,
            "Y-spring must keep tracking the wave while anchored \
             (got y_ratio = {} after 2 s of constant-0.8 bars; anchor \
             physics must not gate Y dynamics)",
            state.y_ratio
        );
    }

    #[test]
    fn step_anchor_does_not_fire_in_margin() {
        // Mirror of the tack-suppression-in-margin rule: an anchor event
        // mid-margin would have the boat hovering off-screen, which
        // wastes the doodad. Hold the timer until the boat is back in
        // the visible area.
        let bars = vec![0.5; 16];
        let mut state = BoatState {
            // In the right margin (off-screen).
            x_ratio: 1.05,
            x_wrap_margin: 0.10,
            facing: 1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            // Pre-arm: would fire next tick if margin gating didn't hold.
            secs_until_next_anchor: 0.001,
            ..Default::default()
        };

        step(
            &mut state,
            Duration::from_millis(16),
            &bars,
            false,
            MusicSignals::default(),
        );
        assert_eq!(
            state.anchor_remaining_secs, 0.0,
            "anchor must NOT fire while the boat is in the off-screen \
             wrap margin (got anchor_remaining_secs = {})",
            state.anchor_remaining_secs
        );
    }

    #[test]
    fn step_anchor_lift_preserves_facing() {
        // After the anchor lifts, the boat should resume sailing in the
        // direction it was facing when it dropped — no random re-roll.
        // Set anchor_remaining_secs to one tick's worth so it lifts on
        // the next step.
        let bars = vec![0.5; 16];
        let mut state = BoatState {
            x_ratio: 0.5,
            facing: -1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            anchor_remaining_secs: 0.005,
            secs_until_next_anchor: 1e6,
            ..Default::default()
        };

        step(
            &mut state,
            Duration::from_millis(16),
            &bars,
            false,
            MusicSignals::default(),
        );
        assert_eq!(
            state.anchor_remaining_secs, 0.0,
            "precondition: anchor should have just lifted"
        );
        assert_eq!(
            state.facing, -1.0,
            "facing must be preserved across an anchor lift — the wind \
             didn't change while the boat rested (got facing = {})",
            state.facing
        );
    }

    #[test]
    fn step_anchor_reseeds_countdown_after_lift() {
        // When the anchor lifts, `secs_until_next_anchor` must be reset
        // to a value within the `[MIN, MAX]` window — otherwise the
        // boat would immediately drop anchor again on the very next tick.
        let bars = vec![0.5; 16];
        let mut state = BoatState {
            x_ratio: 0.5,
            facing: 1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            anchor_remaining_secs: 0.005,
            secs_until_next_anchor: 0.0,
            ..Default::default()
        };

        step(
            &mut state,
            Duration::from_millis(16),
            &bars,
            false,
            MusicSignals::default(),
        );
        assert!(
            state.secs_until_next_anchor >= ANCHOR_INTERVAL_MIN_SECS - 1e-3,
            "post-lift countdown must be reseeded into [{}, {}] (got {})",
            ANCHOR_INTERVAL_MIN_SECS,
            ANCHOR_INTERVAL_MAX_SECS,
            state.secs_until_next_anchor
        );
    }

    #[test]
    fn step_tack_does_not_fire_while_anchored() {
        // The wind shouldn't shift while the boat is taking a break —
        // the tack countdown must hold while anchored, the same way it
        // holds while in the off-screen margin.
        let bars = vec![0.5; 16];
        let mut state = BoatState {
            x_ratio: 0.5,
            facing: 1.0,
            rng_state: 0x12345,
            // Pre-arm tack: would fire next tick if anchor gating didn't hold.
            secs_until_next_tack: 0.001,
            anchor_remaining_secs: 12.0,
            secs_until_next_anchor: 1e6,
            ..Default::default()
        };

        step(
            &mut state,
            Duration::from_millis(16),
            &bars,
            false,
            MusicSignals::default(),
        );
        assert_eq!(
            state.facing, 1.0,
            "tack must not fire while the boat is anchored (got facing = {})",
            state.facing
        );
    }

    #[test]
    fn step_anchor_sway_responds_to_wave_amplitude() {
        // While anchored on tall waves, the rope sway must build up.
        // The drive is `local_height · sin(2π · sway_phase)`, so over
        // a couple of seconds the spring-damper must produce a non-zero
        // |anchor_sway|.
        let tall_bars = vec![0.8_f64; 16];
        let mut state = BoatState {
            x_ratio: 0.5,
            facing: 1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            anchor_remaining_secs: 12.0,
            secs_until_next_anchor: 1e6,
            ..Default::default()
        };

        // Sample over several seconds; sway oscillates, so look at the
        // peak magnitude reached, not just the final value.
        let mut peak_sway = 0.0_f32;
        let dt = Duration::from_millis(16);
        for _ in 0..(5 * 60) {
            step(&mut state, dt, &tall_bars, false, MusicSignals::default());
            peak_sway = peak_sway.max(state.anchor_sway.abs());
        }
        assert!(
            peak_sway > 0.01,
            "anchor sway must build up on tall waves while anchored \
             (peak |sway| = {peak_sway} rad; expected > 0.01)",
        );
        assert!(
            peak_sway <= MAX_ANCHOR_SWAY + 1e-3,
            "anchor sway must respect MAX_ANCHOR_SWAY cap (peak = {peak_sway})",
        );
    }

    #[test]
    fn step_anchor_sway_stays_quiet_on_calm_water() {
        // Calm spectrum (local height below the gate floor): the rope
        // should hang nearly straight, not drift around. Sway must
        // remain tiny across the whole anchor duration.
        let calm_bars = vec![0.01_f64; 16];
        let mut state = BoatState {
            x_ratio: 0.5,
            facing: 1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            anchor_remaining_secs: 12.0,
            secs_until_next_anchor: 1e6,
            ..Default::default()
        };

        let mut peak_sway = 0.0_f32;
        let dt = Duration::from_millis(16);
        for _ in 0..(5 * 60) {
            step(&mut state, dt, &calm_bars, false, MusicSignals::default());
            peak_sway = peak_sway.max(state.anchor_sway.abs());
        }
        assert!(
            peak_sway < 0.005,
            "anchor sway must stay near 0 on calm water — the gate at \
             ANCHOR_SWAY_AMPLITUDE_FLOOR must zero out the drive \
             target (peak |sway| = {peak_sway} rad)",
        );
    }

    // --- anchor handle cache ---------------------------------------------------

    #[test]
    fn cache_anchor_handle_returns_cached_when_theme_unchanged() {
        let _guard = THEME_MUTATION_LOCK.lock();

        let mut state = BoatState::default();
        let id1 = state.cache_anchor_handle().id();
        let id2 = state.cache_anchor_handle().id();
        assert_eq!(
            id1, id2,
            "two consecutive cache_anchor_handle calls without a theme \
             change must return the same cached handle"
        );
    }

    #[test]
    fn cache_anchor_handle_rebuilds_when_active_theme_changes() {
        let _guard = THEME_MUTATION_LOCK.lock();

        let mut state = BoatState::default();
        let initial_mode = crate::theme::is_light_mode();

        let id_before = state.cache_anchor_handle().id();
        crate::theme::set_light_mode(!initial_mode);
        let id_after = state.cache_anchor_handle().id();
        crate::theme::set_light_mode(initial_mode);

        assert_ne!(
            id_before, id_after,
            "anchor handle must rebuild after a theme/mode change \
             (got id_before = id_after = {id_before}, meaning stale cache)"
        );
    }

    #[test]
    fn cached_anchor_handle_misses_then_hits_after_caching() {
        let _guard = THEME_MUTATION_LOCK.lock();

        let mut state = BoatState::default();
        assert!(
            state.cached_anchor_handle().is_none(),
            "empty cache must miss before priming"
        );
        let primed = state.cache_anchor_handle().id();
        let looked_up = state
            .cached_anchor_handle()
            .expect("cache primed by cache_anchor_handle")
            .id();
        assert_eq!(
            primed, looked_up,
            "cached_anchor_handle must return the same handle that \
             cache_anchor_handle just inserted"
        );
    }

    // --- anchor drop position --------------------------------------------------

    #[test]
    fn step_anchor_event_captures_drop_x() {
        // When the anchor fires, `anchor_drop_x` must equal the boat's
        // x_ratio at that exact moment — that's what pins the anchor
        // sprite to the ocean floor for the rest of the event.
        let bars = vec![0.5; 16];
        let mut state = BoatState {
            x_ratio: 0.62,
            facing: 1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            secs_until_next_anchor: 0.001,
            ..Default::default()
        };

        step(
            &mut state,
            Duration::from_millis(16),
            &bars,
            false,
            MusicSignals::default(),
        );
        assert!(
            state.anchor_remaining_secs > 0.0,
            "precondition: anchor must have fired"
        );
        assert!(
            (state.anchor_drop_x - 0.62).abs() < 1e-3,
            "anchor_drop_x must capture x_ratio at the moment the anchor \
             fired (expected ≈ 0.62, got {})",
            state.anchor_drop_x
        );
    }

    #[test]
    fn step_anchor_suppresses_slope_force() {
        // While anchored on a steep ramp, the boat must not drift —
        // slope force is gated to 0 so the anchor genuinely holds the
        // boat in place. Compare against an un-anchored run on the
        // same ramp: that one drifts; the anchored one stays put.
        let ramp: Vec<f64> = (0..16).map(|i| i as f64 / 15.0).collect();

        let template = BoatState {
            x_ratio: 0.5,
            x_velocity: 0.0,
            facing: 1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            secs_until_next_anchor: 1e6,
            ..Default::default()
        };

        let mut sailing = template.clone();
        let mut anchored = template;
        anchored.anchor_remaining_secs = 12.0;

        let dt = Duration::from_millis(16);
        for _ in 0..(3 * 60) {
            step(&mut sailing, dt, &ramp, false, MusicSignals::default());
            step(&mut anchored, dt, &ramp, false, MusicSignals::default());
        }

        assert!(
            (anchored.x_ratio - 0.5).abs() < 0.01,
            "anchored boat must stay near its starting x — slope must \
             not drift it (got x_ratio = {})",
            anchored.x_ratio
        );
        assert!(
            (sailing.x_ratio - 0.5).abs() > 0.05,
            "control: sailing boat on the same ramp must have moved \
             measurably (got x_ratio = {})",
            sailing.x_ratio
        );
    }

    // --- music-driven sail thrust ---------------------------------------------

    /// Helper: run `step()` long enough on flat water that `x_velocity`
    /// reaches its terminal value (sail thrust balanced against
    /// damping). Returns that terminal velocity. Used to compare music
    /// modulations without the velocity floor masking small differences
    /// — at terminal velocity the boat is well above the floor and any
    /// music-driven thrust change is directly observable.
    fn terminal_velocity_under(music: MusicSignals) -> f32 {
        let bars = vec![0.5_f64; 16];
        let mut state = BoatState {
            x_ratio: 0.5,
            x_velocity: 0.0,
            facing: 1.0,
            // Pin beat_phase so the half-sine envelope value is
            // constant across the run (otherwise the comparison would
            // average over a beat cycle, blurring the modulation).
            beat_phase: 0.25,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            secs_until_next_anchor: 1e6,
            ..Default::default()
        };
        // Many short ticks to stay close to the steady-state behavior
        // (small dt → semi-implicit Euler tracks the analytic solution
        // closely). Sample music with bpm=None inside this helper if the
        // caller uses bpm — we override beat_phase advancement by
        // resetting it each tick so the envelope stays at peak.
        let dt = Duration::from_millis(16);
        for _ in 0..(3 * 60) {
            // 3 s
            state.beat_phase = 0.25;
            step(&mut state, dt, &bars, false, music);
        }
        state.x_velocity
    }

    #[test]
    fn step_silence_brings_boat_to_rest() {
        // `MusicSignals::default()` produces zero `total_intensity` —
        // there is NO baseline thrust. Sail force is zero, the velocity
        // floor is zero, and damping is the only horizontal force. From
        // any starting velocity the boat must coast to a near-stop
        // within a few seconds. This is the "music propels everything"
        // contract: silence = no motion.
        let bars = vec![0.5_f64; 16];
        let mut state = BoatState {
            x_ratio: 0.5,
            x_velocity: 0.10,
            facing: 1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            secs_until_next_anchor: 1e6,
            ..Default::default()
        };
        let dt = Duration::from_millis(16);
        for _ in 0..(5 * 60) {
            // 5 s
            step(&mut state, dt, &bars, false, MusicSignals::default());
        }
        assert!(
            state.x_velocity.abs() < 0.005,
            "boat must coast to rest under silence (got x_velocity = {})",
            state.x_velocity
        );
    }

    #[test]
    fn step_onset_energy_drives_thrust_from_silence() {
        // Spectral-flux onset is one of three signals that drive
        // `total_intensity`. With BPM none, long_onset zero, and onset
        // alone non-zero, the boat must still move — onset alone can
        // produce thrust above the baseline-of-zero.
        let v_silent = terminal_velocity_under(MusicSignals::default());
        let v_loud = terminal_velocity_under(MusicSignals {
            bpm: None,
            onset_energy: 1.0,
            long_onset_energy: 0.0,
            bar_energy: 0.0,
        });
        assert!(
            v_silent.abs() < 0.005,
            "silence must give a near-zero terminal velocity (got {v_silent})"
        );
        assert!(
            v_loud > 0.03,
            "saturating onset alone must produce visible forward motion \
             (got v_loud = {v_loud})"
        );
    }

    #[test]
    fn step_bpm_advances_beat_phase() {
        // With a tagged BPM, `beat_phase` must tick forward at
        // `bpm/60` Hz. At 120 BPM (2 Hz) and dt=500 ms, phase advances
        // by 2 · 0.5 = 1.0 cycle — wraps back near 0.
        let bars = vec![0.5_f64; 16];
        let mut state = BoatState {
            x_ratio: 0.5,
            facing: 1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            secs_until_next_anchor: 1e6,
            ..Default::default()
        };
        let music = MusicSignals {
            bpm: Some(120),
            onset_energy: 0.0,
            long_onset_energy: 0.0,
            bar_energy: 0.0,
        };
        step(&mut state, Duration::from_millis(500), &bars, false, music);
        // After exactly one cycle, phase wraps to ~0. Allow some slack
        // for floating-point and the integration ordering.
        assert!(
            state.beat_phase < 0.05 || state.beat_phase > 0.95,
            "beat_phase must complete one cycle at 120 BPM in 500 ms \
             (got {})",
            state.beat_phase
        );
    }

    #[test]
    fn step_bpm_modulates_thrust_at_peak() {
        // With a tagged BPM, the half-sine-squared envelope at
        // `beat_phase = 0.25` adds `BEAT_AMP` (0.4) to total_intensity.
        // Compared against no-BPM-no-onset (intensity = 0), terminal
        // velocity must rise from ~0 to a meaningful positive value.
        let v_no_beat = terminal_velocity_under(MusicSignals::default());
        let v_with_beat = terminal_velocity_under(MusicSignals {
            bpm: Some(120),
            onset_energy: 0.0,
            long_onset_energy: 0.0,
            bar_energy: 0.0,
        });
        assert!(
            v_no_beat.abs() < 0.005,
            "silence must give a near-zero terminal velocity (got {v_no_beat})"
        );
        assert!(
            v_with_beat > 0.02,
            "beat-pulsed thrust at envelope peak must produce visible \
             motion (got v_with_beat = {v_with_beat})"
        );
    }

    #[test]
    fn step_bpm_holds_phase_when_bpm_disappears() {
        // If the next song has no tagged BPM, `beat_phase` must hold
        // its current value — not advance against a zero rate, not
        // snap to zero. The next tagged track resumes mid-phase
        // cleanly.
        let bars = vec![0.5_f64; 16];
        let mut state = BoatState {
            x_ratio: 0.5,
            facing: 1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            secs_until_next_anchor: 1e6,
            beat_phase: 0.42,
            ..Default::default()
        };
        let music = MusicSignals {
            bpm: None,
            onset_energy: 0.0,
            long_onset_energy: 0.0,
            bar_energy: 0.0,
        };
        step(&mut state, Duration::from_millis(500), &bars, false, music);
        assert!(
            (state.beat_phase - 0.42).abs() < 1e-6,
            "beat_phase must hold when bpm is None (got {})",
            state.beat_phase
        );
    }

    #[test]
    fn step_bpm_scales_cruise_thrust() {
        // BPM scales the cruise component (long_onset · BPM scale).
        // With long_onset just above the noise floor (so cruise
        // doesn't immediately saturate at the intensity cap), a 60 BPM
        // song must produce slower cruise than a 180 BPM song.
        let v_slow = terminal_velocity_under(MusicSignals {
            bpm: Some(60),
            onset_energy: 0.0,
            long_onset_energy: 0.04,
            bar_energy: 0.0,
        });
        let v_fast = terminal_velocity_under(MusicSignals {
            bpm: Some(180),
            onset_energy: 0.0,
            long_onset_energy: 0.04,
            bar_energy: 0.0,
        });
        assert!(
            v_fast > v_slow + 0.005,
            "180 BPM terminal velocity must exceed 60 BPM by a visible \
             margin when long_onset is just above the noise floor (slow \
             v = {v_slow}, fast v = {v_fast})"
        );
    }

    #[test]
    fn step_bpm_clamps_extreme_tags() {
        // The point of `bpm_scale.clamp(BPM_SCALE_MIN,
        // BPM_SCALE_MAX)` is to keep outlier BPM tags (a 30-BPM
        // dirge or a 400-BPM error) from stranding the boat or
        // launching it off-screen. The terminal velocities must
        // stay in a sane band — above the floor (boat is moving),
        // below the hard cap (boat isn't pinned at MAX_X_V).
        //
        // (An older form of this test asserted strict equality
        // between clamped and at-clamp-boundary BPMs. That worked
        // under the old `total_intensity.clamp(0, 1)` because
        // saturation hid all residuals. With un-clamped stacking,
        // the beat-phase advance per tick differs by BPM, landing
        // each BPM at a different point on the half-sine envelope
        // — a real per-tick residual that's larger than the
        // scale-clamp's effect. The test now checks the actual
        // pathology guard: bounded behavior, not exact equality.)
        let v_clamped_min = terminal_velocity_under(MusicSignals {
            bpm: Some(30),
            onset_energy: 0.0,
            long_onset_energy: 0.1,
            bar_energy: 0.0,
        });
        let v_clamped_max = terminal_velocity_under(MusicSignals {
            bpm: Some(400),
            onset_energy: 0.0,
            long_onset_energy: 0.1,
            bar_energy: 0.0,
        });
        assert!(
            v_clamped_min > MIN_SAILING_VELOCITY * 0.5,
            "extreme-low BPM (30) must NOT strand the boat below the \
             floor (got {v_clamped_min})"
        );
        assert!(
            v_clamped_max < MAX_X_V,
            "extreme-high BPM (400) must NOT exceed the hard velocity \
             cap (got {v_clamped_max} vs MAX_X_V={MAX_X_V})"
        );
    }

    #[test]
    fn step_bar_energy_drives_cruise_on_flux_poor_material() {
        // Spectrum-presence cruise covers the case the spectral-flux
        // signal misses: sustained pads / drones / soundtrack
        // material with high RMS but near-zero bin-to-bin delta. The
        // M83 "A Necessary Escape" case in production: BPM=0 (None),
        // very low onset & long_onset (sustained synths), but a
        // visibly full spectrum. Without `bar_energy` driving cruise
        // the boat would coast to rest under this signal — with it,
        // the boat must produce visible motion.
        let v_silent = terminal_velocity_under(MusicSignals::default());
        let v_pad = terminal_velocity_under(MusicSignals {
            bpm: None,
            onset_energy: 0.0,
            long_onset_energy: 0.0,
            bar_energy: 0.5,
        });
        assert!(
            v_silent.abs() < 0.005,
            "silence (all signals zero) must give near-zero terminal \
             velocity (got {v_silent})"
        );
        assert!(
            v_pad > 0.03,
            "a half-full visible spectrum must propel the boat through \
             the presence channel even when flux is zero (got v_pad = \
             {v_pad})"
        );
    }

    #[test]
    fn step_bar_energy_below_floor_yields_no_thrust() {
        // The presence floor (`PRESENCE_FLOOR`) is a deadzone: a
        // barely-visible spectrum must not propel the boat. This
        // protects the "pre-roll silence with FFT noise" case where
        // a couple of bars wiggle near zero.
        let v_below_floor = terminal_velocity_under(MusicSignals {
            bpm: None,
            onset_energy: 0.0,
            long_onset_energy: 0.0,
            bar_energy: 0.05, // strictly below PRESENCE_FLOOR (0.10)
        });
        assert!(
            v_below_floor.abs() < 0.005,
            "bar_energy below the presence floor must produce no thrust \
             — silence-equivalent (got {v_below_floor})"
        );
    }

    #[test]
    fn step_onset_stacks_above_saturating_cruise() {
        // The whole point of un-clamping `total_intensity` at 2.0 (vs
        // the old 1.0): an already-saturating-cruise track must read
        // VISIBLY faster when onset stacks on top, instead of pinning
        // at the same ceiling as a cruise-only track. This is the
        // Mother-North-vs-Sea-Pictures differentiation symptom: both
        // tracks saturate cruise, but Mother North's denser onset
        // stream should produce a higher terminal velocity.
        let v_cruise_only = terminal_velocity_under(MusicSignals {
            bpm: None,
            onset_energy: 0.0,
            long_onset_energy: 1.0,
            bar_energy: 1.0,
        });
        let v_cruise_plus_onset = terminal_velocity_under(MusicSignals {
            bpm: None,
            onset_energy: 1.0,
            long_onset_energy: 1.0,
            bar_energy: 1.0,
        });
        assert!(
            v_cruise_plus_onset > v_cruise_only + 0.02,
            "onset must stack above saturating cruise to give energetic \
             tracks visible headroom (cruise-only = {v_cruise_only}, \
             cruise+onset = {v_cruise_plus_onset})"
        );
    }

    #[test]
    fn step_floor_does_not_spike_on_stacked_intensity() {
        // The velocity floor is clamped to `min(intensity, 1.0)` so
        // a stacked-intensity track (e.g. cruise=1 + onset=1 →
        // intensity=1.6) doesn't get a surprise high floor. Without
        // the clamp the floor would scale to ~0.064 on Mother-North-
        // class tracks, snapping the boat onto a fast cruise the
        // instant any music plays. Floor must stay at ~MIN_SAILING_VELOCITY
        // (0.04) regardless of how high intensity stacks.
        let bars = vec![0.5_f64; 16];
        let mut state = BoatState {
            x_ratio: 0.5,
            x_velocity: 0.0,
            facing: 1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            secs_until_next_anchor: 1e6,
            ..Default::default()
        };
        // Single tick from rest: the floor immediately clamps velocity
        // to `MIN_SAILING_VELOCITY * intensity_clamped`. With stacked
        // intensity ~1.6, the un-clamped product would be 0.064; the
        // clamped one is exactly MIN_SAILING_VELOCITY = 0.04.
        step(
            &mut state,
            Duration::from_millis(16),
            &bars,
            false,
            MusicSignals {
                bpm: None,
                onset_energy: 1.0,
                long_onset_energy: 1.0,
                bar_energy: 1.0,
            },
        );
        assert!(
            state.x_velocity <= MIN_SAILING_VELOCITY + 1e-6,
            "stacked intensity must NOT lift the velocity floor above \
             `MIN_SAILING_VELOCITY` — floor must clamp at intensity = 1 \
             (got x_velocity = {})",
            state.x_velocity
        );
    }

    #[test]
    fn step_cruise_signals_compose_via_max_not_sum() {
        // `cruise_intensity = max(flux_cruise, presence_cruise)`.
        // A track with both signals saturating must not exceed the
        // single-saturating-signal terminal velocity — sum/blend
        // semantics would push above the cap and erase the dynamic
        // range across genres. (Beat / onset still surge ABOVE
        // cruise via BEAT_AMP / ONSET_AMP; this test isolates the
        // cruise composition only.)
        let v_flux_only = terminal_velocity_under(MusicSignals {
            bpm: None,
            onset_energy: 0.0,
            long_onset_energy: 1.0,
            bar_energy: 0.0,
        });
        let v_presence_only = terminal_velocity_under(MusicSignals {
            bpm: None,
            onset_energy: 0.0,
            long_onset_energy: 0.0,
            bar_energy: 1.0,
        });
        let v_both = terminal_velocity_under(MusicSignals {
            bpm: None,
            onset_energy: 0.0,
            long_onset_energy: 1.0,
            bar_energy: 1.0,
        });
        // `max` semantics: both-signals terminal must equal the
        // higher of the two single-signal terminals (within a small
        // numerical tolerance), not their sum.
        let single_cap = v_flux_only.max(v_presence_only);
        assert!(
            (v_both - single_cap).abs() < 1e-3,
            "max(flux, presence) cruise must not exceed single-signal \
             saturation (flux-only = {v_flux_only}, presence-only = \
             {v_presence_only}, both = {v_both})"
        );
    }

    #[test]
    fn step_long_onset_drives_cruise_thrust() {
        // Long-onset envelope is what makes un-tagged tracks differ from
        // each other: an energetic instrumental track settles the slow
        // envelope high → faster cruise. With silence baseline at zero,
        // a saturating long_onset must produce visible motion.
        let v_silent = terminal_velocity_under(MusicSignals::default());
        let v_energetic = terminal_velocity_under(MusicSignals {
            bpm: None,
            onset_energy: 0.0,
            long_onset_energy: 1.0,
            bar_energy: 0.0,
        });
        assert!(
            v_silent.abs() < 0.005,
            "silence must give near-zero terminal velocity (got {v_silent})"
        );
        assert!(
            v_energetic > 0.05,
            "saturating long_onset must produce visible cruise thrust \
             (got v_energetic = {v_energetic})"
        );
    }

    #[test]
    fn step_cruise_floor_scales_with_music() {
        // The velocity floor itself scales by `cruise_scale` so the
        // boat's MINIMUM cruise speed differs by song. Compare two
        // single-tick observations from rest: with no music signals
        // the boat clamps to the baseline floor; with high cruise
        // signals (high BPM, high long-onset) the boat clamps to a
        // measurably higher floor.
        let bars = vec![0.5_f64; 16];
        let template = BoatState {
            x_ratio: 0.5,
            x_velocity: 0.0,
            facing: 1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            secs_until_next_anchor: 1e6,
            ..Default::default()
        };

        let mut quiet = template.clone();
        step(
            &mut quiet,
            Duration::from_millis(16),
            &bars,
            false,
            MusicSignals::default(),
        );

        let mut energetic = template;
        step(
            &mut energetic,
            Duration::from_millis(16),
            &bars,
            false,
            MusicSignals {
                bpm: Some(180),
                onset_energy: 0.0,
                long_onset_energy: 1.0,
                bar_energy: 0.0,
            },
        );

        assert!(
            quiet.x_velocity.abs() < 1e-3,
            "no music signals must produce zero floor — boat sits still \
             at silence (got {})",
            quiet.x_velocity
        );
        assert!(
            energetic.x_velocity > 0.02,
            "high BPM + high long-onset must clamp velocity to a \
             meaningful floor (got {})",
            energetic.x_velocity
        );
    }

    #[test]
    fn step_tack_does_not_snap_to_opposite_floor() {
        // Smooth-turn contract: when a tack flips facing, momentum in
        // the OLD direction must decelerate through zero before the
        // floor re-engages on the new heading. We verify this by
        // arming a near-immediate tack and checking that one tick
        // after the flip, x_velocity is NOT clamped to the new
        // direction's floor — it's still in the old direction (or
        // near zero), continuing to decelerate.
        let bars = vec![0.5_f64; 16];
        let mut state = BoatState {
            x_ratio: 0.5,
            // Boat sailing right at near-terminal velocity.
            x_velocity: 0.10,
            facing: 1.0,
            rng_state: 0x12345,
            // Pre-arm a tack: countdown ticks below 0 on first step,
            // facing flips to -1 inside that same tick.
            secs_until_next_tack: 0.001,
            secs_until_next_anchor: 1e6,
            ..Default::default()
        };
        let music = music_on();
        let dt = Duration::from_millis(16);
        step(&mut state, dt, &bars, false, music);

        assert_eq!(
            state.facing, -1.0,
            "precondition: tack should have flipped facing"
        );
        // The pre-rewrite "snap" bug: floor would have clamped
        // x_velocity to -MIN_SAILING_VELOCITY immediately after the
        // flip. Smooth-turn fix: floor only applies when velocity is
        // already aligned with the new facing (i.e., negative). At
        // the first tick post-flip, momentum is still rightward
        // (positive), so the floor does NOT engage.
        assert!(
            state.x_velocity > -MIN_SAILING_VELOCITY * total_intensity_for(music),
            "post-tack velocity must NOT be snapped to the new heading's \
             floor — momentum should still carry the boat in the old \
             direction while damping decelerates it (got x_velocity = {})",
            state.x_velocity
        );
    }

    #[test]
    fn step_tack_ramp_dampens_thrust_immediately_after_flip() {
        // Right after a tack, `secs_since_tack` is `Some(0.0)` and the
        // ramp scales sail thrust + floor by 0. So the boat must NOT
        // be at full sail force on the very next tick — its velocity
        // change should match damping-only (no thrust contribution).
        let bars = vec![0.5_f64; 16];
        let mut at_rest = BoatState {
            x_ratio: 0.5,
            x_velocity: 0.0,
            facing: 1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            secs_until_next_anchor: 1e6,
            // Pretend a tack JUST happened.
            secs_since_tack: Some(0.0),
            ..Default::default()
        };
        let mut at_rest_no_tack = at_rest.clone();
        at_rest_no_tack.secs_since_tack = None;

        let dt = Duration::from_millis(16);
        step(&mut at_rest, dt, &bars, false, music_on());
        step(&mut at_rest_no_tack, dt, &bars, false, music_on());

        assert!(
            at_rest.x_velocity < at_rest_no_tack.x_velocity,
            "post-tack ramp must produce less thrust than steady-state \
             on the first tick (post-tack v = {}, steady v = {})",
            at_rest.x_velocity,
            at_rest_no_tack.x_velocity
        );
    }

    #[test]
    fn step_tack_ramp_recovers_to_full_thrust_after_window() {
        // After `TACK_RAMP_SECS` of ticking, `secs_since_tack` clears
        // back to `None` and the boat is at full thrust again.
        let bars = vec![0.5_f64; 16];
        let mut state = BoatState {
            x_ratio: 0.5,
            facing: 1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            secs_until_next_anchor: 1e6,
            secs_since_tack: Some(0.0),
            ..Default::default()
        };
        let dt = Duration::from_millis(16);
        let ramp_ticks = ((TACK_RAMP_SECS / dt.as_secs_f32()) as usize) + 5;
        for _ in 0..ramp_ticks {
            step(&mut state, dt, &bars, false, music_on());
        }
        assert!(
            state.secs_since_tack.is_none(),
            "ramp must clear secs_since_tack to None after the window \
             completes (got {:?})",
            state.secs_since_tack
        );
    }

    #[test]
    fn step_slope_force_does_not_accelerate_downhill() {
        // Boat sailing right (facing = +1) on a DESCENDING ramp (slope
        // negative) — the raw slope force would push right (downhill)
        // and accelerate the boat. The "only resists" rule must mask
        // this off, so terminal velocity equals the flat-water case.
        let flat = vec![0.5_f64; 16];
        let descending: Vec<f64> = (0..16).map(|i| (15 - i) as f64 / 15.0).collect();

        let template = BoatState {
            x_ratio: 0.5,
            x_velocity: 0.0,
            facing: 1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            secs_until_next_anchor: 1e6,
            ..Default::default()
        };

        let mut state_flat = template.clone();
        for _ in 0..30 {
            step(
                &mut state_flat,
                Duration::from_millis(16),
                &flat,
                false,
                music_on(),
            );
        }
        let mut state_downhill = template;
        for _ in 0..30 {
            step(
                &mut state_downhill,
                Duration::from_millis(16),
                &descending,
                false,
                music_on(),
            );
        }

        assert!(
            (state_downhill.x_velocity - state_flat.x_velocity).abs() < 0.005,
            "downhill slope must NOT accelerate the boat — terminal \
             velocity must match the flat-water case (flat v = {}, \
             downhill v = {})",
            state_flat.x_velocity,
            state_downhill.x_velocity
        );
    }

    #[test]
    fn step_anchor_does_not_fire_outside_safe_zone() {
        // Boat near the right edge of the visible area (x = 0.9, well
        // inside [0, 1] but outside the [0.15, 0.85] anchor safe zone)
        // with the anchor countdown pre-armed. Anchor must NOT fire —
        // letting it would risk the boat drifting / wrapping during
        // the anchor and producing a rope rendering across the screen.
        let bars = vec![0.5; 16];
        let mut state = BoatState {
            x_ratio: 0.9,
            x_velocity: 0.05,
            facing: 1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            secs_until_next_anchor: 0.001,
            ..Default::default()
        };
        step(
            &mut state,
            Duration::from_millis(16),
            &bars,
            false,
            music_on(),
        );
        assert_eq!(
            state.anchor_remaining_secs, 0.0,
            "anchor must NOT fire while the boat is outside the safe \
             zone (got anchor_remaining_secs = {})",
            state.anchor_remaining_secs
        );
    }

    #[test]
    fn step_anchor_zeros_velocity_on_drop() {
        // The anchor catches the boat: when the event fires, the
        // boat's x_velocity must immediately drop to zero so it
        // doesn't drift forward into the wrap margin during the
        // anchor.
        let bars = vec![0.5; 16];
        let mut state = BoatState {
            x_ratio: 0.5,
            x_velocity: 0.10,
            facing: 1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            secs_until_next_anchor: 0.001,
            ..Default::default()
        };
        step(
            &mut state,
            Duration::from_millis(16),
            &bars,
            false,
            music_on(),
        );
        assert!(
            state.anchor_remaining_secs > 0.0,
            "precondition: anchor must have fired at x = 0.5 (in safe zone)"
        );
        assert_eq!(
            state.x_velocity, 0.0,
            "anchor catch must zero x_velocity on the firing tick \
             (got {})",
            state.x_velocity
        );
    }

    /// Mirror of the inline computation in `step()`: composes the
    /// total-intensity scalar from the same three signals so tests can
    /// reason about the expected effective floor without duplicating
    /// the formula. Pinned to the same constants the production path
    /// uses.
    fn total_intensity_for(music: MusicSignals) -> f32 {
        let bpm_scale = music.bpm.map_or(1.0, |bpm| {
            (bpm as f32 / REFERENCE_BPM).clamp(BPM_SCALE_MIN, BPM_SCALE_MAX)
        });
        let lifted = (music.long_onset_energy - LONG_ONSET_FLOOR).max(0.0);
        let cruise = (lifted * LONG_ONSET_AMP * bpm_scale).clamp(0.0, 1.0);
        // Tests don't pin beat_phase here, so assume "no beat
        // contribution" — adequate for the smooth-turn floor check.
        let onset = music.onset_energy.clamp(0.0, 1.0);
        (cruise + ONSET_AMP * onset).clamp(0.0, 1.0)
    }

    #[test]
    fn step_anchor_suppresses_music_thrust() {
        // While anchored, sail thrust is off — music modulations ride on
        // sail thrust so they're naturally suppressed too. The boat must
        // still come to rest under loud music.
        let bars = vec![0.5_f64; 16];
        let mut state = BoatState {
            x_ratio: 0.5,
            x_velocity: 0.05,
            facing: 1.0,
            rng_state: 0x12345,
            secs_until_next_tack: 1e6,
            secs_until_next_anchor: 1e6,
            anchor_remaining_secs: 12.0,
            ..Default::default()
        };
        let music = MusicSignals {
            bpm: Some(140),
            onset_energy: 1.0,
            long_onset_energy: 1.0,
            bar_energy: 1.0,
        };
        // 5 s of damping at X_DAMPING = 0.9 takes velocity from 0.05
        // down to ~0.05 · exp(-4.5) ≈ 5e-4. Anything still moving
        // visibly after that points to a sail-thrust leak through the
        // anchored gate.
        for _ in 0..(5 * 60) {
            step(&mut state, Duration::from_millis(16), &bars, false, music);
        }
        assert!(
            state.x_velocity.abs() < 0.005,
            "anchored boat must come to rest even with loud music — beat \
             pulses and onset energy must NOT push it (got x_velocity = {})",
            state.x_velocity
        );
    }
}
