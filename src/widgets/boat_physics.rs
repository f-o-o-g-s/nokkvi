//! Physics model for the sailing-boat overlay: constants, state, and the
//! per-frame `step()` integrator. Pure CPU; no iced types used here.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use iced::widget::svg;
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

/// Absolute pixel floor on the boat sprite's height. Without this,
/// half-screen / short-window configurations drop the boat to a
/// barely-recognizable ~14–30 px dot. 48 px reads as a clear sprite
/// at any window size while still leaving room above for the
/// visualizer waves.
pub(crate) const BOAT_MIN_HEIGHT_PX: f32 = 48.0;

/// Absolute pixel ceiling on the boat sprite's height. On 4K windows
/// the 18% rule otherwise produces a 230+ px boat that overpowers
/// the wave it's surfing on. 160 px stays bold without dominating;
/// the anchor sprite (60% of boat) and the rope (scaled below)
/// inherit the cap automatically.
pub(crate) const BOAT_MAX_HEIGHT_PX: f32 = 160.0;

/// Compute the boat sprite's pixel size from the visualizer area
/// height. Returns `(width, height)` in pixels. Centralizes the
/// formula so the wrap-margin handler in `update/boat.rs` and the
/// render path in `boat_overlay()` can never drift apart — both
/// callers go through this helper. The clamp keeps the boat in a
/// readable range across the full window-size spectrum (tiny short
/// windows can't shrink it below 48 px; 4K windows can't blow it up
/// past 160 px).
pub(crate) fn boat_pixel_size(area_height: f32) -> (f32, f32) {
    let h = (area_height * BOAT_HEIGHT_FRACTION).clamp(BOAT_MIN_HEIGHT_PX, BOAT_MAX_HEIGHT_PX);
    (h * BOAT_ASPECT_RATIO, h)
}

/// Compute the rope's stroke width in pixels, scaled to the boat's
/// height so a 48 px boat keeps a 1.5 px hairline rope while a
/// 160 px boat reads with a thicker 3.5 px stroke. Without scaling,
/// a fixed 1.5 px rope looks invisible against a large boat and
/// chunky against a small one.
pub(crate) fn rope_stroke_for(boat_h: f32) -> f32 {
    (boat_h * 0.025).clamp(1.5, 3.5)
}

/// Compute the visualizer waveform's baseline Y (pixel where `y_ratio = 0`
/// sits) and amplitude scale (pixel span between `y_ratio = 0` and
/// `y_ratio = 1`) for the boat's wave-riding math. Centralizes the
/// geometry difference between normal and mirrored line modes so the
/// boat render path and any future mirror-aware logic stay in lockstep.
///
/// In normal mode the line is drawn from the canvas bottom upward, so
/// baseline = `area_height` and the full canvas height is available for
/// amplitude. In mirrored mode the line draws symmetrically from the
/// canvas vertical center, so baseline = `area_height * 0.5` and only
/// the upper half is available (`scale = area_height * 0.5`). The lower
/// half is a literal reflection in `visualizer/shaders/lines.wgsl`'s
/// vertex shader; the boat doesn't ride on it yet.
///
/// Matches the shader's `get_point()` geometry modulo the small
/// `max_expansion` AA padding (a ~4–12 px inset on the drawable range)
/// — the boat elides that inset because at the boat's Y-spring
/// damping it's not visually distinguishable from the unpadded
/// formula.
pub(crate) fn wave_baseline_and_scale(area_height: f32, mirror: bool) -> (f32, f32) {
    if mirror {
        (area_height * 0.5, area_height * 0.5)
    } else {
        (area_height, area_height)
    }
}

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
pub(crate) const SLOPE_DX: f32 = 0.05;

/// Slope force gain — converts wave gradient into horizontal force.
pub(crate) const SLOPE_GAIN: f32 = 0.04;

/// Hard cap on `|slope_force|`. Sized below `MAX_SAIL_THRUST` so peak
/// slope resistance never overcomes the sail in the facing direction.
/// Slope force is masked to RESIST motion only (never assist), so an
/// uphill flank produces a headwind that slows the sail's terminal
/// velocity from `MAX_SAIL_THRUST / X_DAMPING ≈ 0.10` down to
/// `(MAX_SAIL_THRUST - MAX_SLOPE_FORCE) / X_DAMPING ≈ 0.067`. The
/// velocity floor keeps the boat moving forward when the wave alone
/// would stall it.
pub(crate) const MAX_SLOPE_FORCE: f32 = 0.03;

/// Local-height threshold below which the slope force is fully suppressed.
/// "No surfing in calm water": when the wave under the boat is essentially
/// flat (`target_y < FLOOR`), `downhill` has no physical meaning and the
/// gradient at the foot of the basin would otherwise drag the boat further
/// into low-energy edge regions.
pub(crate) const SLOPE_GATE_FLOOR: f32 = 0.05;

/// Width of the linear ramp from "fully suppressed" to "full slope force".
/// At `target_y = FLOOR + RAMP` the gate is 1.0 (full surf force); between
/// FLOOR and FLOOR + RAMP it lerps. Tuned so anything above ~25% of the
/// visualizer height surfs at full force, while the bottom ~5% is dead
/// zone — covers the V-basin at the seam without flattening surfing on
/// real wave faces.
pub(crate) const SLOPE_GATE_RAMP: f32 = 0.20;

/// Friction on `x_velocity`. The dominant source of the "floating" feel —
/// without this, the boat would build up arbitrary speed.
pub(crate) const X_DAMPING: f32 = 0.9;

/// Hard cap on `|x_velocity|` to keep numerical extremes from launching the
/// boat across the screen in a single tick. Sized to accommodate the
/// stacked-intensity terminal velocity at peak music (cruise + beat +
/// onset → `total_intensity ≈ 2.0` → terminal `≈ 0.22 ratio/sec`) with
/// a small safety margin so the cap doesn't truncate the visible
/// stacking headroom on energetic tracks.
pub(crate) const MAX_X_V: f32 = 0.20;

/// Spring constant for `y_ratio` tracking the sampled wave height. Higher =
/// boat sticks tighter to the curve.
pub(crate) const Y_SPRING_K: f32 = 80.0;

/// Damping applied relative to the wave-surface velocity (slope × x_velocity
/// feed-forward), so the boat tracks steep peaks without lagging. With
/// `Y_SPRING_K = 80` and damping `12` the damping ratio ζ ≈ 0.67 —
/// slightly underdamped, so a quick wave change produces a small bob
/// before settling.
pub(crate) const Y_DAMPING: f32 = 12.0;

/// Conversion from sampled wave slope to target tilt angle (radians per
/// unit slope). A typical wave gradient is in the `0.5..2.0` range, so a
/// gain of `0.18` lands the boat around `5°..20°` of lean before the cap.
pub(crate) const TILT_GAIN: f32 = 0.18;

/// Hard cap on `|tilt|`. ~17° feels like the boat is genuinely committed
/// to the wave without flipping over. Scales the visible tilt range
/// independently of how aggressive `TILT_GAIN` is.
pub(crate) const MAX_TILT: f32 = 0.30;

/// Spring constant for `tilt` tracking `target_tilt`. Higher = boat snaps
/// to the slope; lower = the lean lags so visible motion is buoyant. Tuned
/// alongside `TILT_DAMPING` for an underdamped feel on quick wave changes.
pub(crate) const TILT_SPRING_K: f32 = 60.0;

/// Damping on `tilt_velocity`. With `TILT_SPRING_K = 60` and damping `10`
/// the damping ratio ζ ≈ 0.65 — same family as Y dynamics: slightly
/// underdamped, a sharp wave produces a small overshoot before settling.
pub(crate) const TILT_DAMPING: f32 = 10.0;

/// Tilt quantization step in degrees. The boat SVG is re-baked with the
/// rotation embedded in its path data each time the quantized angle
/// changes (option A in the SVG-aliasing investigation), so finer
/// quantization gives smoother visible motion at the cost of more cache
/// entries. With `MAX_TILT ≈ 17°` and 0.5° steps that's ~70 entries per
/// facing × theme combo, which fits comfortably in iced's bitmap atlas.
pub(crate) const TILT_QUANT_DEG: f32 = 0.5;

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
pub(crate) const MAX_SAIL_THRUST: f32 = 0.10;

/// Maximum velocity floor — the cap on `MIN_SAILING_VELOCITY ·
/// total_intensity`. Asserted only when the boat's velocity is in the
/// SAME direction as `facing`, so a fresh tack can decelerate through
/// zero before the floor re-engages on the new heading (smooth turns).
/// At full music intensity the boat's minimum cruise speed is this
/// value; at silence the floor is 0 and the boat is allowed to stop.
/// Scaled in lockstep with `MAX_SAIL_THRUST` so the floor-to-cruise
/// ratio stays roughly constant across tuning changes.
pub(crate) const MIN_SAILING_VELOCITY: f32 = 0.04;

/// Min/max delay between tack events (random "wind shift" that flips
/// `facing`). Sampled uniformly in `[MIN, MAX]` after each tack and at
/// first activation, so direction changes feel scheduled by mood rather
/// than clockwork. Tuned long enough that a single voyage across the
/// visible area completes before the wind turns — the boat reads as
/// purposefully sailing in one direction, not wandering.
pub(crate) const TACK_INTERVAL_MIN_SECS: f32 = 20.0;
pub(crate) const TACK_INTERVAL_MAX_SECS: f32 = 60.0;

/// Time over which sail thrust + velocity floor ramp from 0 back to
/// full after a tack. The first moment after a flip, sail thrust is
/// 0 so damping alone decelerates the boat; sail thrust ramps in over
/// `TACK_RAMP_SECS`, smoothly accelerating the boat onto its new
/// heading. This is what makes turns visibly gradual rather than the
/// boat "stopping on a dime" and instantly accelerating in reverse.
pub(crate) const TACK_RAMP_SECS: f32 = 4.0;

/// Min/max delay between drop-anchor events. Rarer than tacks (which
/// fire every 20–60 s) so the rest stops read as deliberate moments
/// rather than constant lurking. Sampled uniformly in `[MIN, MAX]`.
pub(crate) const ANCHOR_INTERVAL_MIN_SECS: f32 = 45.0;
pub(crate) const ANCHOR_INTERVAL_MAX_SECS: f32 = 120.0;

/// Anchor-firing safe zone: anchor only fires when `x_ratio` is well
/// within `[ANCHOR_SAFE_LO, ANCHOR_SAFE_HI]`. Outside this zone the
/// boat is too close to the wrap margin — even after the anchor
/// catches the boat, residual rendering at the wrap seam (or the
/// boat re-entering after a wrap) would have the rope stretching
/// across the entire visible area to the dropped anchor on the far
/// side, which reads as a rendering glitch.
pub(crate) const ANCHOR_SAFE_LO: f32 = 0.15;
pub(crate) const ANCHOR_SAFE_HI: f32 = 0.85;

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
pub(crate) const LONG_ONSET_FLOOR: f32 = 0.02;
pub(crate) const LONG_ONSET_AMP: f32 = 18.0;
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
pub(crate) const PRESENCE_FLOOR: f32 = 0.10;
pub(crate) const PRESENCE_AMP: f32 = 1.5;
/// Beat-pulse contribution to total intensity. Range is the half-sine-
/// squared envelope `[0, 1]`; `BEAT_AMP = 0.4` lets a beat add up to
/// 40 percentage points to intensity on top of the cruise level.
pub(crate) const BEAT_AMP: f32 = 0.4;
/// Onset (instant transient) contribution. Range is roughly `[0, 1]`;
/// `ONSET_AMP = 0.6` lets a hit surge intensity by up to 60 points
/// above cruise.
pub(crate) const ONSET_AMP: f32 = 0.6;

/// BPM at which the BPM scale factor equals `1.0`. When a song has a
/// tagged BPM, the cruise component is multiplied by
/// `(bpm / REFERENCE_BPM).clamp(BPM_SCALE_MIN, BPM_SCALE_MAX)` to
/// scale the cruise level by tempo — fast tracks cruise faster than
/// the long_onset alone would predict, slow tracks cruise slower.
pub(crate) const REFERENCE_BPM: f32 = 120.0;
pub(crate) const BPM_SCALE_MIN: f32 = 0.5;
pub(crate) const BPM_SCALE_MAX: f32 = 2.0;

/// Min/max duration of an active anchor. The boat hovers in place,
/// catching the beat as waves roll past underneath. Long enough to feel
/// like a deliberate stop, short enough that the music's character
/// barely changes during it.
pub(crate) const ANCHOR_DURATION_MIN_SECS: f32 = 10.0;
pub(crate) const ANCHOR_DURATION_MAX_SECS: f32 = 15.0;

/// Hard cap on `|anchor_sway|` (radians). At ~6° the rope swing reads as
/// "the water is moving the rope" without ever pulling the anchor far
/// enough to detach visually from below the boat.
pub(crate) const MAX_ANCHOR_SWAY: f32 = 0.10;

/// Spring constant for `anchor_sway` tracking the wave-driven target.
/// Lower than the boat's tilt spring so the rope settles slower —
/// matches the visual intuition that the rope is a heavier, lazier
/// thing than the boat hull.
pub(crate) const ANCHOR_SWAY_SPRING_K: f32 = 8.0;

/// Damping on `anchor_sway_velocity`. With `SPRING_K = 8` and damping
/// `4` the damping ratio ζ ≈ 0.71 — slightly underdamped so a quick
/// wave still produces a small overshoot, then settles. Same family as
/// the boat's tilt and Y springs.
pub(crate) const ANCHOR_SWAY_DAMPING: f32 = 4.0;

/// Frequency (Hz) of the slow oscillator that drives the sway target.
/// 0.4 Hz = a 2.5 s period, which reads as a gentle sea-swell rhythm
/// against the much faster waveform jitter the boat already responds
/// to via tilt.
pub(crate) const ANCHOR_SWAY_DRIVE_HZ: f32 = 0.4;

/// Local-wave-height threshold below which sway target stays at zero.
/// On calm spectrums the rope hangs straight down; the wave has to
/// reach this fraction of the visualizer height before the rope starts
/// to swing. Same `SLOPE_GATE_FLOOR` value the slope force uses, so
/// the "calm water doesn't move stuff around" invariant is uniform
/// across the doodad.
pub(crate) const ANCHOR_SWAY_AMPLITUDE_FLOOR: f32 = SLOPE_GATE_FLOOR;

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
    /// "Inverted boat surfs the lower wave" affordance for mirrored line
    /// mode. Toggled on every off-screen X wrap in `step()`, so each
    /// tack-and-wrap cycle alternates the boat between the upper and
    /// lower wave reflections. Only visually meaningful when the
    /// renderer is rendering with `mirror = true`: outside mirrored
    /// line mode the lower wave doesn't exist, so the render path
    /// ignores `inverted` and draws the boat upright on the canvas
    /// bottom regardless. Anchor firing is suppressed while `inverted`,
    /// and a wrap that flips this from `false` to `true` while an
    /// anchor is active lifts the anchor immediately (V1 punts on
    /// inverted + rope-physics geometry).
    pub inverted: bool,
    pub tilt_handles: HashMap<(i16, bool, bool), svg::Handle>,
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
pub(crate) fn quantize_tilt(tilt: f32) -> i16 {
    let degrees = tilt.to_degrees();
    (degrees / TILT_QUANT_DEG).round() as i16
}

/// Inverse of `quantize_tilt`: convert a cache index back to the radians
/// the SVG was baked at.
pub(crate) fn dequantize_tilt(idx: i16) -> f32 {
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

    /// Build (and cache) the boat SVG handle for the given orientation
    /// (tilt, facing, inverted), returning a clone of the cached
    /// handle. The tilt is quantized to `TILT_QUANT_DEG`-degree steps
    /// so the cache stays bounded; the requested radians are
    /// dequantized back before being baked into the SVG, so the
    /// handle's rotation is exactly what the cache key represents (no
    /// per-frame drift). `inverted` extends the key so a flipped-on-
    /// wrap boat doesn't reuse the upright cache entry; the SVG body
    /// bakes a `scale(1, -1)` transform on top of any rotate or
    /// horizontal-mirror when this is true.
    pub(crate) fn cache_handle_for(
        &mut self,
        tilt: f32,
        facing: f32,
        inverted: bool,
    ) -> svg::Handle {
        self.clear_if_theme_changed();
        let key = (quantize_tilt(tilt), facing < 0.0, inverted);
        if let Some(h) = self.tilt_handles.get(&key) {
            return h.clone();
        }
        let bytes =
            crate::embedded_svg::themed_boat_svg(dequantize_tilt(key.0), key.1, key.2).into_bytes();
        let h = svg::Handle::from_memory(bytes);
        self.tilt_handles.insert(key, h.clone());
        h
    }

    /// Look up a cached handle for the given tilt + facing + inverted
    /// without mutating state. Returns `None` when the theme has
    /// advanced past the cache, when nothing is cached yet for this
    /// orientation, or when the boat hasn't ticked since `Default`. The
    /// render path uses this and falls back to an inline rebuild on
    /// miss.
    pub(crate) fn cached_handle_for(
        &self,
        tilt: f32,
        facing: f32,
        inverted: bool,
    ) -> Option<svg::Handle> {
        let current_gen = crate::theme::theme_generation();
        if self.handle_generation != current_gen {
            return None;
        }
        let key = (quantize_tilt(tilt), facing < 0.0, inverted);
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
    let height_rate = slope * state.x_velocity;
    let ay =
        (local_height - state.y_ratio) * Y_SPRING_K - (state.y_velocity - height_rate) * Y_DAMPING;
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
pub(crate) fn next_rand_unit(state: &mut u32) -> f32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    (x as f32) / (u32::MAX as f32)
}

/// Linear interpolation `a → b` by `t ∈ [0, 1]`. Used to map a uniform
/// `[0, 1)` random sample into a charge interval/duration window.
pub(crate) fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Pick an initial facing (`±1`) with a fair coin flip. Used to seed
/// `BoatState.facing` on the first tick when the field is at its
/// `Default` value of `0.0`, and to pick a fresh facing after each tack
/// (currently a deterministic flip rather than a re-roll, but `pick_facing`
/// is the single point where any future weighting would land).
pub(crate) fn pick_facing(rng_state: &mut u32) -> f32 {
    if next_rand_unit(rng_state) < 0.5 {
        -1.0
    } else {
        1.0
    }
}
