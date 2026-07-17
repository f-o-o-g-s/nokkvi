//! Procedural sea + trawling-longship scene for the Harbour Trawl panel.
//!
//! The Harbour landing view opens centered on the Trawl mix-builder row, whose
//! artwork panel used to show a static anchor glyph. This module replaces it
//! with a living scene: a gently travelling two-layer sea (drawn by
//! [`SeaCanvas`]) with the nokkvi longship sailing across it, perpetually
//! dragging its anchor along the seabed — trawling. The boat itself is the
//! Lines-visualizer surfing boat reused verbatim ([`boat_overlay`] with a
//! `trail` offset); only the wave source is new.
//!
//! Coherence contract: [`sea_bars`] produces ONE array per tick
//! (`update::boat::step_harbour_scene`), which is BOTH fed to
//! `boat_physics::step()` and stored on `Nokkvi.harbour_sea_bars` for
//! [`SeaCanvas`] to draw through the same [`sample_line_height`] sampler the
//! physics used. A phase or sampler mismatch would desync the hull from the
//! drawn water invisibly to tests/clippy — always route both sides through
//! this module.
//!
//! Everything here is silence-proof by construction: the sea is a pure
//! function of a phase the boat tick advances, and the physics' presence
//! cruise is fed a fixed [`HARBOUR_CRUISE_BAR_ENERGY`] instead of live audio,
//! so the scene breathes identically with the player stopped, paused, or
//! playing.

use iced::{Color, Element, Length, Point, Rectangle, Size, widget::canvas};

use crate::widgets::{
    boat::{BoatState, boat_overlay, parse_hex_color, sample_line_height},
    harbour_runes,
};

/// Samples in the sea height field — enough that the Catmull-Rom resample
/// reads as a smooth swell at panel widths, few enough that building the
/// array per frame is negligible.
pub(crate) const SEA_POINTS: usize = 96;

/// Travelling-phase advance rate in cycles/sec. The front swell's crest
/// speed is `(SWELL_PHASE_K / SWELL_CYCLES) · SEA_DRIFT_HZ` panel-widths
/// per second — 0.05 gives a ~20 s crest crossing, the calm baseline.
// TUNE: raise for a livelier sea, lower for glassier water.
pub(crate) const SEA_DRIFT_HZ: f32 = 0.05;

/// Fixed `MusicSignals::bar_energy` fed to the harbour boat's physics step.
/// This is the scene's calm lever, deliberately NOT the sea's true mean
/// (~0.45): presence cruise = `(0.20 − 0.10) · 1.5 = 0.15` → terminal
/// velocity ≈ 0.017 ratio/sec (a ~60 s crossing) with a 0.006 velocity
/// floor, so the boat always creeps forward but never hurries.
// TUNE: the single strongest calm↔alive dial. 0.30 ≈ 30 s crossings.
pub(crate) const HARBOUR_CRUISE_BAR_ENERGY: f32 = 0.20;

/// How far behind the hull (in `x_ratio` units) the trawled anchor trails at
/// cruise speed. The render eases this by `|x_velocity| / TRAIL_V_REF`, so
/// the anchor slides under the hull as the boat stalls through a tack.
// TUNE: longer reads as a heavier drag; shorter tucks the anchor under the stern.
pub(crate) const TRAIL_OFFSET: f32 = 0.08;

/// Sea shape — one slow swell plus a faster low ripple, both travelling.
/// Every layer's phase multiplier is an INTEGER so the field is exactly
/// periodic in the `[0, 1)` phase (`sea_bars(0) == sea_bars(1)`): the tick
/// wraps the phase with `rem_euclid(1.0)` to dodge long-session f32 sin
/// precision decay, and integer multipliers make that wrap seamless.
/// `SWELL_CYCLES` / `RIPPLE_CYCLES` are integers too, which additionally
/// makes the field periodic in X — the boat's toroidal slope sampling near
/// the wrap seam then reads a REAL gradient instead of a fake edge cliff.
// TUNE: DC sets the waterline height (fraction of the scene); amps set chop.
const SEA_DC: f64 = 0.45;
const SWELL_AMP: f64 = 0.06;
const SWELL_CYCLES: f64 = 2.0;
const SWELL_PHASE_K: f64 = 2.0;
const RIPPLE_AMP: f64 = 0.025;
const RIPPLE_CYCLES: f64 = 5.0;
const RIPPLE_PHASE_K: f64 = 8.0;
/// Fixed phase offset decorrelating the ripple from the swell so their
/// crests don't align every cycle.
const RIPPLE_SHIFT: f64 = 1.3;

/// Back parallax layer — drawn only (the physics never samples it), a dimmer
/// swell riding higher on the panel. Crest speed `(1 / 2) · SEA_DRIFT_HZ` is
/// HALF the front's — that speed difference is the whole parallax read.
// TUNE: BACK_RAISE lifts the horizon; BACK_AMP sets the far swell's chop.
const BACK_RAISE: f64 = 0.12;
const BACK_AMP: f64 = 0.04;
const BACK_CYCLES: f64 = 2.0;
const BACK_PHASE_K: f64 = 1.0;
const BACK_SHIFT: f64 = 0.7;

/// Scene lighting — the design-panel repaint. The old flat washes (back
/// 0.10 / front 0.16 / a 0.38-alpha ink crest that measured ~2/255 of
/// effect) read as paper cutouts; these consts drive the gradient passes
/// that replace them. The value story: a faint cold airglow gathers at the
/// horizon (motivating every highlight below it), the far swell hazes into
/// the sky, the near water is brightest at its lit surface and sinks
/// toward a dark seabed that grounds the trawled anchor and mediates the
/// old razor cut into the pill band. All light is the theme's starlight;
/// all darkness is the theme's ink, dialed by `border_opacity` (the same
/// light-mode legibility knob the rope uses).
// TUNE: the sky's value story — 0.0 removes it (keep below the header wash 0.07).
const SKY_GLOW_ALPHA: f32 = 0.045;
// TUNE: far-swell haze — how fast the distance dissolves into sky.
const SEA_BACK_TOP_ALPHA: f32 = 0.035;
const SEA_BACK_BODY_ALPHA: f32 = 0.10;
const SEA_BACK_FADE_STOP: f32 = 0.18;
// TUNE: front water's depth ramp — lit surface, mid body, sinking deep.
const SEA_LIT_ALPHA: f32 = 0.20;
const SEA_MID_ALPHA: f32 = 0.13;
const SEA_DEEP_ALPHA: f32 = 0.08;
// TUNE: seabed ink vignette — where the darkening starts (fraction of
// height) and its floor alpha. Cap ~0.26: beyond that the anchor's ink
// starts to drown in its own ground.
const SEA_BED_TOP: f32 = 0.70;
const SEA_BED_ALPHA: f32 = 0.18;
// TUNE: the moonlit crest — committed sprite-weight ink, a soft starlight
// halo, and a bright catch-light that fades into the panel edges over
// CREST_LIGHT_EDGE of the width.
const CREST_INK_ALPHA: f32 = 0.55;
const CREST_HALO_ALPHA: f32 = 0.05;
const CREST_LIGHT_ALPHA: f32 = 0.18;
const CREST_LIGHT_EDGE: f32 = 0.12;
/// Day gain for the crest light passes once they render in sun gold —
/// by day the sun is the scene's declared light source, so the
/// catch-light and shimmer sweep GILD instead of going invisible
/// (starlight-on-light measured ~0; day's waterline was a bare ink
/// line). Gold here is the same `logo_wood()` the sun fan and lantern
/// glint already draw with — day's one warm hue extended, not a second
/// warm note; night's passes stay starlight untouched. Keep ≤ 1.0 (the
/// sweep-stops test's alpha ceiling assumes it).
// TUNE: drop to 0.7 if gold sweep + gold glint stack loud where the
// boat crosses the sweep.
const CREST_LIGHT_GAIN_DAY: f32 = 1.0;
const _: () = assert!(CREST_LIGHT_GAIN_DAY <= 1.0);

/// Horizontal pixel step between sampled points when drawing the water
/// polylines. 3 px keeps the Catmull-Rom curve smooth without building
/// long paths on wide panels.
const SEA_DRAW_STEP_PX: f32 = 3.0;

/// Night sky above the waves — a sparse constellation of star dots, sparkle
/// crosses, and small music-note glyphs, each twinkling gently. Behavioural
/// kin of the Scope visualizer's particle dust, but deliberately NOT that
/// system: the Scope field is a stateful CPU ember sim (drift + recycle)
/// feeding a wgpu shader, while a night sky wants STATIC, deterministic
/// positions with only brightness moving — so this is a pure hash-scattered
/// field drawn in the same canvas pass as the water, borrowing just the
/// twinkle idea. Positions come from a const-seeded xorshift so every frame
/// (and every launch) sees the same constellation.
// TUNE: counts set density; alphas set how loud the sky reads.
const SKY_STAR_COUNT: usize = 40;
const SKY_SPARKLE_COUNT: usize = 3;
/// Faint tier: tiny stars whose twinkle depth is 1.0 — they fade all the
/// way OUT and back, so the field's population visibly breathes instead of
/// every star merely dimming.
const SKY_FAINT_COUNT: usize = 14;
const SKY_FAINT_SIZE_MIN: f32 = 0.35;
const SKY_FAINT_SIZE_SPAN: f32 = 0.30;
/// Wandering notes: the sky's music glyphs are TRANSIENT — each cycle a few
/// notes fade in at a cycle-hashed spot, drift gently upward, and fade out,
/// never appearing in the same place twice.
const SKY_WANDER_NOTES: usize = 3;
const SKY_NOTE_DUR: f32 = 0.22;
/// Vertical band the sky occupies, as fractions of the scene height from the
/// top — kept above the back swell's highest crest (~0.39 from the top) so
/// glyphs never sit IN the water.
const SKY_BAND_TOP: f32 = 0.04;
const SKY_BAND_BOTTOM: f32 = 0.36;
/// Twinkle: per-glyph brightness shimmer
/// `1 − depth·(0.5 + 0.5·sin(2π·(k·phase + offset)))`. Each glyph's rate
/// `k` is an INTEGER (same wrap-safety rule as the sea layers) so the
/// phase's `rem_euclid(1.0)` wrap never pops a star. Twice retuned: the
/// original 0.6 depth at up to ~1.2 Hz BLINKED; the calm-panel floor
/// (0.25 depth, 3.3–6.7 s) read as static. The full-send setting lands
/// between them: ~55% of glyphs breathe at 0.45 depth over 2.2–5 s, the
/// rest sit near-still — alive, star-by-star, never a strobe.
// TUNE: depth = shimmer strength; K range = breath rate (MAX is exclusive).
// Full-send retune: livelier than the panel's calm floor (0.25 / 3.3-6.7 s
// read as static to the owner) while keeping the star-by-star hierarchy
// that separates twinkling from blinking.
const SKY_TWINKLE_DEPTH: f32 = 0.45;
const SKY_TWINKLE_K_MIN: u32 = 4;
const SKY_TWINKLE_K_MAX: u32 = 10;
/// Fraction of glyphs assigned the full breathing depth; the rest stay
/// near-still at `SKY_STILL_DEPTH_FACTOR` of it — a motion hierarchy, so
/// the sky shimmers star-by-star instead of blinking as a block.
const SKY_BREATHER_FRACTION: f32 = 0.55;
const SKY_STILL_DEPTH_FACTOR: f32 = 0.4;
/// Peak alphas per glyph kind. Notes sit dimmest and stillest — objects
/// don't glow, light sources do (the bloom-threshold rule), so the note
/// glyphs are atmosphere, not beacons.
// TUNE: SKY_NOTE_ALPHA 0.0 hides the notes without deleting them (quick A/B).
const SKY_STAR_ALPHA: f32 = 0.45;
const SKY_SPARKLE_ALPHA: f32 = 0.45;
const SKY_NOTE_ALPHA: f32 = 0.32;
/// Extra top inset for note glyphs: their stems extend ~0.03h ABOVE the
/// glyph center, and a note whose center lands at the raw band top clips
/// mid-glyph against the panel edge (a shipped capture caught exactly
/// that). Dots/sparkles have no such reach, so only notes take the inset.
const SKY_NOTE_TOP_INSET: f32 = 0.06;
/// Seed for the constellation scatter. Changing it deals a new sky.
const SKY_SEED: u32 = 0x5EA_57A5;

/// Aurora — two translucent ribbon bands undulating across the upper sky.
/// The default theme is literally named Svalbard; an aurora is the most
/// on-brand celestial object this scene could carry, and the theme's own
/// seafoam ramp IS aurora-colored. Each ribbon is a closed band between two
/// long sinusoids, filled with a vertical gradient that fades to nothing at
/// both edges (a soft curtain, not a stripe). Phase multipliers are
/// INTEGERS (wrap-safety); the two bands drift at different rates for
/// depth. Alpha breathes gently on its own integer rate.
// TUNE: alphas set how loud the curtain reads; amps/thickness its shape.
const AURORA_ALPHA_A: f32 = 0.07;
const AURORA_ALPHA_B: f32 = 0.05;
const AURORA_BREATH_DEPTH: f32 = 0.25;
const AURORA_BREATH_K: f64 = 2.0;

/// Moonbeam shafts — three slanted columns of starlight through the
/// night water, fanning away from the moon: the lit volume that
/// retroactively explains every starlight rim below the surface (the
/// school's catch-rims, the rock and crate rims, the starfish
/// overprint). Night-only (the aurora precedent) and near-threshold
/// QUIET by contract: each shaft is two nested gradient quads whose
/// exposed side-edge alpha steps sit under the ~2/255 banding floor —
/// you notice the water is LIT, never that rays are drawn. This is the
/// 0.72–0.86 separator band's FURNITURE budget: nothing else ever SITS
/// there — rare transients (the serpent) may pass through; see the
/// `SERPENT_*` docs for that half of the band contract.
// TUNE: GAIN is the single dial and kill switch (0.0 = delete). Never
// brighten to "make it visible" — loud god-rays are the kitsch kill
// vector; the answer to any squint is DOWN.
const MOONBEAM_GAIN: f32 = 1.0;
const MOONBEAM_ALPHA_OUTER: f32 = 0.006;
const MOONBEAM_ALPHA_INNER: f32 = 0.007;
/// Entry offsets from `MOON_X` — the shafts stay slaved to the moon
/// consts (moving the moon moves its light). Entries land at
/// x ≈ 0.24 / 0.33 / 0.44.
const MOONBEAM_ENTRY_DX: [f32; 3] = [0.09, 0.18, 0.29];
const MOONBEAM_SEED: u32 = 0xB3A3_0001;
// The banding floor: exposed steps stay under ~2/255 at full gain.
const _: () = assert!(MOONBEAM_ALPHA_OUTER * MOONBEAM_GAIN <= 0.0078);
const _: () = assert!(MOONBEAM_ALPHA_INNER * MOONBEAM_GAIN <= 0.0078);

/// The moon — a bare starlit disc at rest, themed live (disc fill =
/// starlight, rim = the boat outline's ink; see
/// `embedded_svg::themed_moon_face_veiled`), anchoring the sky's upper
/// left and motivating every starlight highlight below it. The owner's
/// face marks live in the same asset but appear ONLY during the moon
/// dream (see MOON_DREAM_*). The disc renders as an `Svg` layer in
/// `trawl_scene` (a canvas can't draw SVGs); the canvas keeps its halo
/// rings underneath, breathing on integer k=1.
// TUNE: alpha 0.0 hides moon AND halo; X/Y position it (fractions of the
// panel); radius scales the face and its halo together.
const MOON_ALPHA: f32 = 0.60;
const MOON_X: f32 = 0.15;
const MOON_Y: f32 = 0.16;
const MOON_RADIUS_PX: f32 = 16.0;

/// Day scene — in LIGHT mode the night vocabulary goes invisible
/// (starlight on a light background), so the sky trades it for daylight:
/// the avatar becomes the SUN — a vexel fan of filled, tapered, gently
/// bellied gold wedges over a discretized radial glow (the owner's
/// sunburst reference is stroke-free translucent fills; the old thin ink
/// spokes spoke the opposite language) — and seagulls glide where the
/// stars were. Water, ship, notes, fish, and glint are shared by both
/// scenes; notes and risers swap starlight for ink so they stay legible.
///
/// WRAP-SAFETY of the fan: seamlessness needs the rotation per cycle to be
/// a multiple of the pattern's FULL symmetry angle. With major/minor rays
/// alternating (period 2), the 12-ray fan is only 6-fold symmetric, so the
/// spin is `TAU / 6` per cycle — `TAU / 12` would land majors on minor
/// slots at every wrap (a visible snap every 20 s). The travelling tip
/// wave inherits the same proof: at the wrap each ray's (angle, tip,
/// alpha) equals ray i+2's start-of-cycle state exactly (pinned by
/// `sun_wedge_field_is_seamless_at_the_phase_wrap`).
// TUNE: wedge alphas = the sun's presence (keep major:minor near 2:1);
// TAN = plumpness; BELLY = the static wave silhouette (0 = straight);
// WAVE_DEPTH = the circulating tip wave (0 freezes it); OUTER = footprint.
const SUN_RAY_COUNT: usize = 12;
const SUN_WEDGE_INNER: f32 = 1.15;
const SUN_WEDGE_TAN_MAJOR: f32 = 0.105; // ~tan 6°
const SUN_WEDGE_TAN_MINOR: f32 = 0.070; // ~tan 4°
const SUN_WEDGE_BELLY_MAJOR: f32 = 0.12;
const SUN_WEDGE_BELLY_MINOR: f32 = 0.08;
const SUN_WEDGE_ALPHA_MAJOR: f32 = 0.20;
const SUN_WEDGE_ALPHA_MINOR: f32 = 0.11;
const SUN_WEDGE_OUTER_MAJOR: f32 = 1.88;
const SUN_WEDGE_OUTER_MINOR: f32 = 1.58;
const SUN_WAVE_DEPTH_MAJOR: f32 = 0.12;
const SUN_WAVE_DEPTH_MINOR: f32 = 0.08;

/// Discretized radial glow stacks — (radius in face-radii, raw alpha),
/// largest-first so the fills stack inward. Banding-proofed by contract
/// (pinned by `glow_stacks_hold_the_banding_contract`): every EXPOSED rim
/// (radius > 1.05, outside the 0.60-opaque avatar) steps at most 0.015
/// (day, ~0.45 gold-on-pastel contrast) / 0.011 (night, ~0.7 starlight-
/// on-dark) — under the ~2/255 visibility floor — the two bright inner
/// steps hide beneath the avatar, and no rim sits in [0.95, 1.05] where
/// it would coincide with the face's own edge.
// TUNE: scale all alphas by one gain for glow strength — but keep the
// exposed-step caps or the rings return as visible vector circles.
const SUN_GLOW_STACK: [(f32, f32); 12] = [
    (2.00, 0.005),
    (1.90, 0.006),
    (1.80, 0.008),
    (1.70, 0.009),
    (1.60, 0.011),
    (1.50, 0.012),
    (1.40, 0.013),
    (1.30, 0.014),
    (1.20, 0.015),
    (1.10, 0.015),
    (0.90, 0.030),
    (0.72, 0.040),
];
const MOON_GLOW_STACK: [(f32, f32); 12] = [
    (2.00, 0.004),
    (1.90, 0.005),
    (1.80, 0.006),
    (1.70, 0.007),
    (1.60, 0.008),
    (1.50, 0.009),
    (1.40, 0.010),
    (1.30, 0.010),
    (1.20, 0.011),
    (1.10, 0.011),
    (0.90, 0.022),
    (0.72, 0.030),
];

/// Moon-halo motion. The CASCADE: each ring breathes on an integer k=1
/// sine with a per-ring lag growing from the innermost ring outward, so
/// one brightness swell is born at the face and rolls out through the
/// stack (~11 s to cross, one exhale per cycle) — deliberately near-
/// threshold quiet. The PULSE is what carries "transient": some cycles a
/// soft two-stroke ring detaches at the halo's shoulder, expands past the
/// rim, and dissolves (cycle-hashed timing, alpha-zero at both ends).
// TUNE: WASH_DEPTH 0 = static halo; PULSE_CHANCE/DUR = exhale cadence.
const MOON_WASH_LAG: f32 = 0.05;
const MOON_WASH_DEPTH: f32 = 0.30;
const MOON_PULSE_SALT: u32 = 0x4A10_5EE1;
const MOON_PULSE_CHANCE: f32 = 0.30;
const MOON_PULSE_DUR: f32 = 0.20;
// The pulse window must sit fully inside the cycle — its hash would
// change mid-exhale at a straddled boundary.
const _: () = assert!(0.15 + 0.45 + MOON_PULSE_DUR < 1.0);

const GULL_COUNT: usize = 6;
const GULL_ALPHA: f32 = 0.50;
/// Off-panel margin (px) a gliding gull fully clears before its travel
/// fraction wraps — the same no-edge-pop contract as the boat's wrap.
const GULL_MARGIN_PX: f32 = 30.0;
/// Wingbeat rate (integer — wrap-safe) and depth of the glide's flap.
const GULL_FLAP_K: f32 = 6.0;
/// Seed for the flock's parameter stream.
const GULL_SEED: u32 = 0x6011_5EA5;

/// Edge fade for the boat-coupled passes (rising notes, lantern glint):
/// their alpha scales by `distance-to-nearer-panel-edge / BOAT_EDGE_FADE`,
/// so they dim out as the hull slides off and dim back in on re-entry — a
/// hard `[0, 1]` gate would cut every mid-flight note and the glint pool
/// in a single frame while the sprite is still half on-screen (x_ratio
/// legitimately roams the wrap margin beyond `[0, 1]`).
// TUNE: wider = earlier, gentler dimming near the edges.
const BOAT_EDGE_FADE: f32 = 0.10;

/// Rising notes — the longship sings. A small pool of note glyphs
/// continuously rises from the boat's mast, swaying as they climb and
/// fading out near the top of their run: the scene's music made visible,
/// and the trawl's catch coming up the line. Each rider loops on an
/// integer multiple of the sea phase; alpha is zero at both ends of its
/// run, so the cycle wrap (a position jump) is never visible.
// TUNE: count/alpha set how songful the boat is; rise/sway set the path.
const RISER_COUNT: usize = 5;
const RISER_ALPHA: f32 = 0.55;
const RISER_RISE_FRAC: f32 = 0.34;
const RISER_SWAY_PX: f32 = 6.0;
const RISER_FADE_IN: f32 = 0.12;
const RISER_FADE_OUT: f32 = 0.30;
/// Seed for the riser parameter stream (offsets, spreads, kinds).
const RISER_SEED: u32 = 0xB0A7_5016;

/// Lantern glint — the boat pools warm light on the water it rides,
/// breathing on a ~5 s cycle (integer k=4 at the 20 s phase). The one warm
/// note in the scene, answering the sprite's gold trim with the SAME
/// mode-stable accessor the logo uses.
// TUNE: alpha sets the pool's brightness; 0.0 removes it.
const GLINT_ALPHA: f32 = 0.10;
const GLINT_BREATH_K: f32 = 4.0;

/// Crest shimmer sweep — once per ~20 s cycle a soft band of light glides
/// along the crest for ~6 s (30% duty), brightest where the wave actually
/// peaks, then the sea rests. The slot-list shimmer's sweep-then-idle
/// grammar ported to the waterline.
// TUNE: alpha = sweep brightness; fraction = active duty; off = phase slot.
const CREST_SHIMMER_ALPHA: f32 = 0.20;
const CREST_SWEEP_FRACTION: f32 = 0.30;
const CREST_SWEEP_OFF: f32 = 0.55;
const CREST_SWEEP_HALF_WIDTH: f32 = 0.10;

/// The black hole — the night sky's rarest event, and INVISIBLE, as a
/// black hole should be: no ring, no halo, no ink — its only signature
/// is what gravity does to the stars. The stars NEAR it are captured —
/// gravity falls off, distant stars never stir — and plunge inward on
/// ACCELERATING spirals (slow drift at first, then the dive; winding
/// tighter as they fall). Nearing the event horizon their light stops
/// escaping: each star SHRINKS AND DIMS TO NOTHING as it crosses in
/// (the owner's brief: "the light shouldn't be able to escape").
/// Trapped survivors orbit through the catch, and then the well spits
/// everything back out — a fast ejection that re-lights each star as
/// it re-crosses the horizon, sails past home, and settles gently
/// back. For ~14 s the sky simply develops a slow whirlpool and a
/// star-shaped absence, then heals.
///
/// The one sanctioned exception to the sky's static-positions
/// contract, and only apparently: displaced positions and the horizon
/// fade are a PURE function of (cycle hash, phase), with displacement
/// exactly zero and visibility exactly 1 at both window ends — at
/// every event boundary, and on every non-event frame, the
/// constellation renders byte-identical to the fixed field. The moon
/// (the avatar) is never pulled, and the shooting star + wandering
/// notes skip hole cycles so the sky carries one drama at a time.
/// Night-only. (History: v1 was a spiral galaxy swallowing the WHOLE
/// sky on an eased glide — "goofy"; v2 added local gravity but marked
/// the hole with a lit accretion ring and piled swallowed stars into
/// a bright knot — both inverted the void. Invisibility + the horizon
/// swallow are the owner's own fixes.)
// TUNE: CHANCE gates rarity (~once per 5 min at 0.07; 0.0 = none);
// WINDOW the whole drama (0.70 ≈ 14 s); CAPTURE_FRAC the well's
// reach; HORIZON_PX where light stops escaping; OVERSHOOT the
// spit-out's sail-past-home punch; HOLD_WHIRL the trapped orbit rate.
const BLACKHOLE_CHANCE: f32 = 0.07;
const BLACKHOLE_WINDOW: f32 = 0.70;
const BLACKHOLE_SALT: u32 = 0x6A1A_C57A;
// Max hashed start (0.05 + 0.20) + the window stays inside the cycle.
const _: () = assert!(0.05 + 0.20 + BLACKHOLE_WINDOW < 1.0);
/// Gravity's reach as a fraction of min(w, h): full capture inside
/// 35% of this radius, fading to zero influence at the full radius —
/// only the NEIGHBORHOOD falls in.
const BLACKHOLE_CAPTURE_FRAC: f32 = 0.30;
/// Captured stars converge to this fraction of their home radius.
const BLACKHOLE_CONVERGE: f32 = 0.06;
/// Swirl gained over a full plunge (radians); inner stars wind up to
/// ~1.7× more (differential rotation — the vortex read).
const BLACKHOLE_SWIRL: f32 = 2.6;
/// Event phasing: the plunge accelerates through PLUNGE_END, the catch
/// holds through HOLD_END, the remainder is the spit-out.
const BLACKHOLE_PLUNGE_END: f32 = 0.45;
const BLACKHOLE_HOLD_END: f32 = 0.58;
/// Plunge acceleration exponent (higher = lazier drift, harder dive).
const BLACKHOLE_PLUNGE_POW: f32 = 2.6;
/// Spit-out overshoot factor `b` in `(1-q)²·(1-b·q)`: stars sail past
/// home (s goes negative → radius beyond home) and settle back. At 3.0
/// the sail-past peaks ~12% beyond home.
const BLACKHOLE_OVERSHOOT: f32 = 3.0;
/// The event horizon in glyph-scale px: a star's light dies out
/// between 1.6× and 0.6× this distance from the hole, and returns the
/// same way on the way out.
const BLACKHOLE_HORIZON_PX: f32 = 9.0;
/// Extra orbital winding through the catch (radians per unit p) —
/// trapped survivors keep circling instead of freezing (the strongest
/// remaining choreography tell in v2). Scales by s, so the spit-out
/// unwinds it into the outward whip.
const BLACKHOLE_HOLD_WHIRL: f32 = 6.0;

/// The moon's dream — the sky's other long ritual, and the only one the
/// moon itself takes part in. The moon RESTS as a bare disc; the face
/// exists only inside the dream: four short verses in the old tongue
/// drift through the upper air, each verse drawing one mark onto the
/// disc — the grin first — until the face is whole; it holds for a
/// breath, then lets go again, the strap first and the grin lingering
/// last, and the plain moon sails on until the next dream. The verses
/// are carved long-branch staves (`harbour_runes`); the scene keeps
/// their meaning to itself. Cycle 0 always dreams (the app lands on
/// Harbour, so every launch is greeted by the ritual once), then the
/// gate rolls the usual hashed dice. Day dreams too — the sun is the
/// same disc, and grows the same face.
///
/// PURITY: mark alphas are a pure function of (cycle hash, phase),
/// EXACTLY 0.0 at both window ends — every non-dream frame renders the
/// bare resting disc from one cached handle. The eyepatch and its strap
/// never hold intermediate alpha at the same instant (their ink
/// overlaps where the strap crosses the patch; a simultaneous half-fade
/// would double-expose the seam) — the arrival and farewell windows are
/// staggered to keep them disjoint, pinned by
/// `moon_dream_patch_and_strap_never_fade_together`. The black hole,
/// the shooting star, and the wandering notes all sit dream cycles out
/// (the one-drama rule).
// TUNE: CHANCE gates rarity (~once per 5 min at 0.07; 0.0 = never);
// GREETS_LAUNCH the cycle-0 ritual; WINDOW the whole dream (0.70 = 14 s
// of the 20 s cycle); MARK_LAG how long a verse sounds before its mark
// answers; VERSE_CX/Y/STAVE_PX/ALPHA place, size, and weight the runes.
const MOON_DREAM_CHANCE: f32 = 0.07;
const MOON_DREAM_GREETS_LAUNCH: bool = true;
const MOON_DREAM_WINDOW: f32 = 0.70;
const MOON_DREAM_SALT: u32 = 0xD5EA_0117;
// Max hashed start (0.10 + 0.15) + the window stays inside the cycle.
const _: () = assert!(0.10 + 0.15 + MOON_DREAM_WINDOW < 1.0);
/// The dream in seconds — its window over the sea drift rate.
const MOON_DREAM_SECS: f32 = MOON_DREAM_WINDOW / SEA_DRIFT_HZ;
/// Verse timing: verse `i` owns `[START + i·SPAN, START + (i+1)·SPAN]`,
/// fading in and out by FADE inside its own span. The recital leads the
/// window; FAREWELL seconds at the end belong to the face letting go.
const MOON_DREAM_VERSE_START: f32 = 0.8;
const MOON_DREAM_FAREWELL: f32 = 3.4;
const MOON_DREAM_VERSE_SPAN: f32 =
    (MOON_DREAM_SECS - MOON_DREAM_VERSE_START - MOON_DREAM_FAREWELL) / 4.0;
const MOON_DREAM_VERSE_FADE: f32 = 0.50;
/// Each mark answers its verse MARK_LAG seconds in, easing over IN_SECS.
const MOON_DREAM_MARK_LAG: f32 = 1.0;
const MOON_DREAM_IN_SECS: f32 = 1.10;
// A mark settles before the next verse's mark begins (IN fits in SPAN).
const _: () = assert!(MOON_DREAM_IN_SECS < MOON_DREAM_VERSE_SPAN);
/// The farewell: fade-out starts (seconds into the window), one per
/// mark in [smile, eye, patch, strap] order — marks leave in REVERSE
/// order, the strap first and the grin lingering last.
const MOON_DREAM_OUT_START: [f32; 4] = [12.70, 12.20, 11.60, 10.90];
const MOON_DREAM_OUT_SECS: f32 = 0.66;
// The strap is fully gone before the patch starts to fade (disjoint
// windows — see the seam note in the doc above).
const _: () = assert!(MOON_DREAM_OUT_START[3] + MOON_DREAM_OUT_SECS < MOON_DREAM_OUT_START[2]);
// The face is whole (last arrival settled) before the farewell begins,
// and the grin's farewell completes inside the window (bare at both
// ends).
const _: () = assert!(
    MOON_DREAM_VERSE_START + 3.0 * MOON_DREAM_VERSE_SPAN + MOON_DREAM_MARK_LAG + MOON_DREAM_IN_SECS
        < MOON_DREAM_OUT_START[3]
);
const _: () = assert!(MOON_DREAM_OUT_START[0] + MOON_DREAM_OUT_SECS < MOON_DREAM_SECS);
// TUNE: the verses' place and presence. STAVE_PX is the rune height at
// the 300 px reference panel (rides the shared glyph scale, then capped
// by the draw pass so the longest line fits between the moon's pixel
// extent and the right edge on every panel shape); CX centers each
// line in the open right sky.
const MOON_DREAM_VERSE_CX: f32 = 0.58;
const MOON_DREAM_VERSE_Y: f32 = 0.14;
const MOON_DREAM_STAVE_PX: f32 = 10.0;
const MOON_DREAM_VERSE_ALPHA: f32 = 0.62;

/// Shooting star — a rare streak across the upper sky. Timing, start
/// point, and heading all hash the CYCLE COUNTER, so no two cycles replay
/// the same streak (the fix for the identical-loop objection that got the
/// pure-phase version declined).
// TUNE: chance gates how many cycles get one; window is its duration.
// Travel and length scale off the panel HEIGHT (the sky band is
// height-proportioned) — width-scaling would dive the streak into the
// water on wide panels.
const SHOOT_CHANCE: f32 = 0.6;
const SHOOT_WINDOW: f32 = 0.05;
const SHOOT_TRAVEL_FRAC: f32 = 0.35;
const SHOOT_LEN_FRAC: f32 = 0.20;
const SHOOT_ALPHA: f32 = 0.7;

/// Sun glitter — day's twinkle vocabulary, on the water where day
/// light actually lives: a sparse fixed field of tiny gold dashes
/// riding the front waterline under the sun's azimuth, each flashing
/// briefly on its own integer rate. The ^4 brightness profile keeps
/// flashes spiky-and-rare (glitter statistics, not blinking — the
/// sky's twinkle-retune lesson). Day-only; gold is LIGHT, never dialed
/// by border_opacity.
// TUNE: COUNT/ALPHA set the lane's presence. If the gilt crest +
// glitter combo reads busy, thin COUNT before dimming ALPHA. 0.0
// alpha = none.
const GLITTER_COUNT: usize = 8;
const GLITTER_ALPHA: f32 = 0.30;
const GLITTER_SEED: u32 = 0x501A_D115;

/// Distant sail — day's rare event (night keeps the shooting star):
/// some cycles a tiny hazed ink sail crosses the back parallax swell,
/// always running toward panel center, riding the far swell's own
/// heave. A fraction of the hero sprite by construction; the first cut
/// on any "two ships" owner verdict.
// TUNE: CHANCE·DUR ≈ the on-screen fraction of day cycles (~7% as
// shipped — a luck moment, not a shipping lane). 0.0 chance = none.
const SAIL_CHANCE: f32 = 0.25;
const SAIL_DUR: f32 = 0.30;
const SAIL_ALPHA: f32 = 0.35;
const SAIL_SALT: u32 = 0x5A11_D157;
// Max hashed start (0.08 + 0.30) + the window stays inside the cycle.
const _: () = assert!(0.08 + 0.30 + SAIL_DUR < 1.0);

/// Leaping fish — occasionally the trawl stirs one up: a small ink
/// silhouette arcs out of the water and dives back. Cycle-hashed position
/// and appearance chance; drawn under the boat layer, dialed by
/// border_opacity like every other ink in the scene.
// TUNE: chance/window/jump set how often and how high; 0.0 chance = none.
const FISH_CHANCE: f32 = 0.45;
const FISH_WINDOW: f32 = 0.07;
const FISH_OFF: f32 = 0.30;
const FISH_JUMP_FRAC: f32 = 0.10;
const FISH_SIZE: f32 = 13.0;
const FISH_ALPHA: f32 = 0.55;

/// Bubbles — the drag aerates the bed: a sparse pool of riders climbs
/// from the trawled anchor, swaying as they rise, fading in at birth
/// and out before the top of the run (alpha zero at both ends — the
/// riser contract, so the loop wrap never shows). The larger ones draw
/// as stroked rings, the small ones as flecks. A second, slower seep
/// rises from each kelp root. History: the seabed's first ship carried
/// data-bound "treasure gems" the anchor kindled; on sight the owner
/// read them as fallen stars and retired the metaphor — what the scene
/// actually wanted was more LIFE at the bottom, and the one kicked-up
/// mote (read as a bubble) was the keeper. This is that mote,
/// densified into the bed's breath.
// TUNE: COUNT/ALPHA set the stream's presence; RISE_FRAC the climb;
// RING_FRACTION how many draw as rings.
const BUBBLE_COUNT: usize = 7;
const BUBBLE_ALPHA: f32 = 0.38;
const BUBBLE_RISE_FRAC: f32 = 0.16;
const BUBBLE_SWAY_PX: f32 = 3.0;
const BUBBLE_RING_FRACTION: f32 = 0.35;
const BUBBLE_FADE_IN: f32 = 0.15;
const BUBBLE_FADE_OUT: f32 = 0.25;
const BUBBLE_SEED: u32 = 0x00B0_BB1E;
/// Kelp-root seep: rise fraction and the fleck's base radius factor.
const SEEP_RISE_FRAC: f32 = 0.11;
const SEEP_ALPHA: f32 = 0.30;

/// The Deep Passage — some cycles Jörmungandr glides once through the
/// deep lane beneath the trawl: a firmly-inked undulating body with a
/// wedge head, exactly three tail-beats, then gone. All randomness
/// (timing, depth, heading) hashes the cycle counter; the window sits
/// fully inside the cycle and the envelope is zero at both ends. The
/// mid-water school draws in FRONT of it (depth) and the anchor sprite
/// rides the layer above the canvas — deference to the focal chain is
/// structural. Ink is committed (the whale lesson: soft = invisible);
/// night adds a starlight dorsal catch-rim (the school's rim lesson at
/// scale). A rare TRANSIENT may cross the protected 0.72–0.86
/// separator band — furniture may not sit there, events may pass
/// through.
// TUNE: CHANCE gates rarity (~1 passage per 2+ min expected at 0.15;
// 0.0 = none); WINDOW the traverse duration (0.18 ≈ 3.6 s). If the
// owner reads "worm", widen the head wedge half-base 2.6 → 3.2 first.
const SERPENT_CHANCE: f32 = 0.15;
const SERPENT_WINDOW: f32 = 0.18;
const SERPENT_ALPHA: f32 = 0.52;
const SERPENT_RIM_ALPHA: f32 = 0.30;
/// The glide lane: hashed depth spans `LANE_TOP..LANE_TOP + LANE_SPAN`
/// (fractions of h), with ~0.02h of undulation headroom below it.
const SERPENT_LANE_TOP: f32 = 0.78;
const SERPENT_LANE_SPAN: f32 = 0.06;
// Max hashed start (0.06 + 0.72) + the window stays inside the cycle.
const _: () = assert!(0.06 + 0.72 + SERPENT_WINDOW < 1.0);
// The lane sits below the school band and above the bubble origin at
// 0.955h (undulation headroom included).
const _: () = assert!(SCHOOL_BAND_BOTTOM < SERPENT_LANE_TOP);
const _: () = assert!(SERPENT_LANE_TOP + SERPENT_LANE_SPAN + 0.02 < 0.955);

/// Drifting school — small ink fish gliding through the mid-water, the
/// swimming counterpart of the rare leaping fish (which keeps its
/// rarity; the school is ambient). The band sits BELOW the deepest wave
/// trough (front crest y bottoms out at ~0.638h) so a drifter can never
/// fly in air, and above the bed so the floor keeps its own layer.
/// Night legibility: dark ink drowns in the dark deep, so each fish
/// carries a faint starlight catch-rim along its back — moonlight
/// through water — the crisp-core-plus-halo grammar at minimum form.
// TUNE: count/alpha set presence; RIM_ALPHA the night moonlight.
const SCHOOL_COUNT: usize = 3;
const SCHOOL_ALPHA: f32 = 0.50;
const SCHOOL_BAND_TOP: f32 = 0.65;
const SCHOOL_BAND_BOTTOM: f32 = 0.72;
const SCHOOL_MARGIN_PX: f32 = 26.0;
const SCHOOL_RIM_ALPHA: f32 = 0.30;
const SCHOOL_SEED: u32 = 0x0005_C001;

/// Kelp — beds of fronds along the floor, swaying on slow integer-rate
/// sines (wrap-safe): clusters on both flanks plus a couple of shorter
/// loners toward the middle, each with its own height so the beds read
/// as growth, not a fence. Ink by day, a dim seafoam by night (dark ink
/// on the night bed would vanish — the same lesson as the school's
/// rim). The anchor drags PAST the mid fronds — it draws on the layer
/// above, which reads as the tackle brushing through the weed.
// TUNE: SWAY_PX = tip travel; alphas per mode; roots/heights in
// `kelp_params`.
const KELP_ALPHA_DAY: f32 = 0.40;
const KELP_ALPHA_NIGHT: f32 = 0.22;
const KELP_SWAY_PX: f32 = 7.0;
const KELP_SEED: u32 = 0xCE1F;

/// Bed dressing — static ink furniture grounding the floor: low rock
/// mounds and one resting starfish, each with a faint starlight rim at
/// night (moonlit tops; plain ink silhouettes vanish on the night bed).
/// Deliberately motionless — rocks don't move, and the still floor is
/// what makes the fish, kelp, and bubbles read as ALIVE against it.
// TUNE: counts/alphas; reseed BED_SEED to re-deal the arrangement.
const ROCK_COUNT: usize = 3;
const BED_INK_ALPHA: f32 = 0.50;
const BED_RIM_ALPHA: f32 = 0.14;
const STARFISH_ARM_PX: f32 = 5.0;
const BED_SEED: u32 = 0x0BED;

/// Sunken cargo crate — the bed's one mid-size landmark, answering the
/// owner's ask ("crates at the bottom") in the readable-solid-object
/// class. Settled at a tilt on the open right bed (the audit's
/// emptiest zone, right-balancing the moon's upper-left weight),
/// half-buried, STATIC on the rocks' stillness contract. Straight
/// edges + the night starlight rim say "crate, not rock" through
/// GEOMETRY; every value stays at or under the bed ink ceiling so the
/// trawled anchor remains the loudest resident of the floor. Placement
/// is a deliberate composition decision (fixed consts, the
/// kelp-root-table pattern), not a scatter.
// TUNE: SIZE_PX sets the landmark scale (rocks are ~9 px tall; keep
// well under the anchor sprite); TILT the settled read; RIM_ALPHA has
// headroom to 0.18 before it competes with the school's 0.30 catch-rim.
const CRATE_X: f32 = 0.78;
const CRATE_SIZE_PX: f32 = 19.0;
const CRATE_TILT_DEG: f32 = -9.0;
const CRATE_SLAT_ALPHA: f32 = 0.30;
const CRATE_RIM_ALPHA: f32 = 0.15;
const _: () = assert!(CRATE_RIM_ALPHA <= 0.18);
const _: () = assert!(CRATE_SLAT_ALPHA <= BED_INK_ALPHA);
// The open right-bed lane: inside the audit's empty zone, clear of the
// 0.68 kelp loner's sway reach. (Starfish clearance is runtime — it is
// dealt from BED_SEED — and lives in the tests.)
const _: () = assert!(0.70 <= CRATE_X && CRATE_X <= 0.88);
const _: () = assert!(CRATE_X - 0.68 >= 0.06);

/// What a sky glyph draws as. Music glyphs are NOT constellation members —
/// they wander (a transient per-cycle pass in the draw, never twice in the
/// same place); the fixed field is stars and sparkles only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SkyGlyphKind {
    /// A filled dot — the bulk of the field.
    Dot,
    /// A 4-point plus-shaped sparkle — occasional accent.
    Sparkle,
}

/// One member of the constellation. `x` spans `[0, 1]` of the width; `y`
/// spans the sky band. `size` is a unit scale multiplied by the pixel base
/// per kind at draw time. `twinkle_k` / `twinkle_off` / `twinkle_depth`
/// drive the shimmer — depth is per-glyph so most stars sit near-still
/// while a minority breathe (the motion hierarchy).
#[derive(Debug, Clone, Copy)]
struct SkyGlyph {
    x: f32,
    y: f32,
    size: f32,
    twinkle_k: u32,
    twinkle_off: f32,
    twinkle_depth: f32,
    kind: SkyGlyphKind,
}

/// Tiny xorshift32 — deterministic visual scatter, no dependency (the same
/// tool `visualizer::particles` and `boat_physics` reach for).
fn xorshift(state: &mut u32) -> f32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    (x as f32) / (u32::MAX as f32)
}

/// Build the constellation — deterministic (const seed), so the sky is the
/// same every frame and every launch; only the twinkle moves.
fn sky_glyphs() -> Vec<SkyGlyph> {
    let mut rng = SKY_SEED;
    let mut make = |kind: SkyGlyphKind| {
        SkyGlyph {
            x: xorshift(&mut rng),
            y: SKY_BAND_TOP + xorshift(&mut rng) * (SKY_BAND_BOTTOM - SKY_BAND_TOP),
            size: 0.7 + xorshift(&mut rng) * 0.6,
            twinkle_k: SKY_TWINKLE_K_MIN
                + (xorshift(&mut rng) * (SKY_TWINKLE_K_MAX - SKY_TWINKLE_K_MIN) as f32) as u32,
            twinkle_off: xorshift(&mut rng),
            // Motion hierarchy: a minority breathe at full depth, the rest
            // sit near-still — the sky shimmers star-by-star, never as a
            // block.
            twinkle_depth: if xorshift(&mut rng) < SKY_BREATHER_FRACTION {
                SKY_TWINKLE_DEPTH
            } else {
                SKY_TWINKLE_DEPTH * SKY_STILL_DEPTH_FACTOR
            },
            kind,
        }
    };
    let mut glyphs = Vec::with_capacity(SKY_STAR_COUNT + SKY_SPARKLE_COUNT + SKY_FAINT_COUNT);
    for _ in 0..SKY_STAR_COUNT {
        glyphs.push(make(SkyGlyphKind::Dot));
    }
    for _ in 0..SKY_SPARKLE_COUNT {
        glyphs.push(make(SkyGlyphKind::Sparkle));
    }
    // Faint tier: smaller than the main field's size floor and at FULL
    // twinkle depth, so each one fades entirely out of existence and back
    // — the field's population itself breathes.
    for _ in 0..SKY_FAINT_COUNT {
        glyphs.push(SkyGlyph {
            x: xorshift(&mut rng),
            y: SKY_BAND_TOP + xorshift(&mut rng) * (SKY_BAND_BOTTOM - SKY_BAND_TOP),
            size: SKY_FAINT_SIZE_MIN + xorshift(&mut rng) * SKY_FAINT_SIZE_SPAN,
            twinkle_k: SKY_TWINKLE_K_MIN
                + (xorshift(&mut rng) * (SKY_TWINKLE_K_MAX - SKY_TWINKLE_K_MIN) as f32) as u32,
            twinkle_off: xorshift(&mut rng),
            twinkle_depth: 1.0,
            kind: SkyGlyphKind::Dot,
        });
    }
    glyphs
}

/// Pixel scale for the scene's hand-drawn furniture (stars, notes, fish,
/// moon), derived from the scene height. ONE helper shared by the canvas
/// pass and the `trawl_scene` Svg moon layer, so the halo the canvas draws
/// and the face the Svg places can never size apart.
fn scene_glyph_scale(h: f32) -> f32 {
    (h / 300.0).clamp(0.7, 1.6)
}

/// Even rays are the fan's MAJOR tier; odd rays are minor.
fn sun_ray_is_major(i: usize) -> bool {
    i.is_multiple_of(2)
}

/// A sun ray's world angle: the fan spins `TAU / 6` per cycle (the fan's
/// FULL symmetry angle — see the day-scene const docs) plus the ray's
/// fixed slot.
fn sun_ray_theta(i: usize, phase: f32) -> f32 {
    use std::f32::consts::TAU;
    phase * (TAU / 6.0) + i as f32 * (TAU / SUN_RAY_COUNT as f32)
}

/// A sun ray's animated (angle, tip radius in face-radii, fill alpha).
/// The travelling wave `sin(2θ + TAU·phase)` circulates AGAINST the spin
/// (a swell washing around the ring); tip radius and brightness ride the
/// SAME wave term. Pure — shared by the draw and the wrap-seam kin test.
fn sun_ray_geometry(i: usize, phase: f32) -> (f32, f32, f32) {
    use std::f32::consts::TAU;
    let theta = sun_ray_theta(i, phase);
    let wave = (2.0 * theta + TAU * phase).sin();
    let (base, depth, alpha) = if sun_ray_is_major(i) {
        (
            SUN_WEDGE_OUTER_MAJOR,
            SUN_WAVE_DEPTH_MAJOR,
            SUN_WEDGE_ALPHA_MAJOR,
        )
    } else {
        (
            SUN_WEDGE_OUTER_MINOR,
            SUN_WAVE_DEPTH_MINOR,
            SUN_WEDGE_ALPHA_MINOR,
        )
    };
    (theta, base + depth * wave, alpha * (0.90 + 0.10 * wave))
}

/// Fill a discretized radial glow: each `(radius, alpha)` entry of a
/// largest-first table becomes one solid circle at `alpha · mult(index)`.
/// Shared by the sun and moon so both bodies' glow obeys one banding
/// contract; `mult` injects the per-mode motion (uniform breath for the
/// sun, the outward-rolling cascade for the moon).
fn draw_glow_stack(
    frame: &mut canvas::Frame,
    center: Point,
    m: f32,
    table: &[(f32, f32)],
    color: Color,
    mult: impl Fn(usize) -> f32,
) {
    for (i, (radius, alpha)) in table.iter().enumerate() {
        frame.fill(
            &canvas::Path::circle(center, radius * m),
            Color {
                a: alpha * mult(i),
                ..color
            },
        );
    }
}

/// One gull of the day scene's flock. `x0` is the travel-phase offset;
/// the glide loops on `k` integer crossings per sea cycle (wrap-safe),
/// leftward or rightward, with a gentle integer-rate bob.
#[derive(Debug, Clone, Copy)]
struct GullParam {
    x0: f32,
    y: f32,
    k: u32,
    leftward: bool,
    size: f32,
    bob_k: u32,
    bob_off: f32,
    flap_off: f32,
}

/// Deal the flock — deterministic, const-seeded, same contract as the sky.
fn gull_params() -> Vec<GullParam> {
    let mut rng = GULL_SEED;
    (0..GULL_COUNT)
        .map(|_| GullParam {
            x0: xorshift(&mut rng),
            y: 0.06 + xorshift(&mut rng) * 0.24,
            k: 1 + (xorshift(&mut rng) * 2.0) as u32,
            leftward: xorshift(&mut rng) < 0.5,
            size: 0.7 + xorshift(&mut rng) * 0.6,
            bob_k: 2 + (xorshift(&mut rng) * 3.0) as u32,
            bob_off: xorshift(&mut rng),
            flap_off: xorshift(&mut rng),
        })
        .collect()
}

/// Draw one gliding gull: the classic two-arc silhouette, wings meeting at
/// `center`, arc height modulated by `flap` for a lazy wingbeat.
fn draw_gull(frame: &mut canvas::Frame, center: Point, s: f32, flap: f32, color: Color) {
    let wing = |dir: f32| {
        canvas::Path::new(|b| {
            b.move_to(Point::new(center.x + dir * s, center.y + 0.12 * s));
            b.quadratic_curve_to(
                Point::new(center.x + dir * 0.45 * s, center.y - flap * s),
                center,
            );
        })
    };
    for dir in [-1.0, 1.0] {
        frame.stroke(
            &wing(dir),
            canvas::Stroke::default()
                .with_color(color)
                .with_width(1.3)
                .with_line_cap(canvas::LineCap::Round),
        );
    }
}

/// One moonbeam shaft. `entry_dx` is the fixed offset from `MOON_X`
/// where the shaft enters the water; breath and sway loop on integer
/// rates (wrap-safe).
#[derive(Debug, Clone, Copy)]
struct MoonbeamParam {
    entry_dx: f32,
    k_breath: u32,
    off_breath: f32,
    k_sway: u32,
    off_sway: f32,
}

/// Deal the shafts — the kelp fixed-table × stream-jitter pattern. The
/// breath offsets are staggered by construction (i·0.33 + jitter) so
/// the three shafts never pulse together; `.min(1.999)` guards
/// xorshift's inclusive 1.0 so the integer rates stay in 1..=2.
fn moonbeam_params() -> Vec<MoonbeamParam> {
    let mut rng = MOONBEAM_SEED;
    MOONBEAM_ENTRY_DX
        .into_iter()
        .enumerate()
        .map(|(i, entry_dx)| MoonbeamParam {
            entry_dx,
            k_breath: 1 + ((xorshift(&mut rng) * 2.0).min(1.999)) as u32,
            off_breath: i as f32 * 0.33 + 0.2 * xorshift(&mut rng),
            k_sway: 1 + ((xorshift(&mut rng) * 2.0).min(1.999)) as u32,
            off_sway: xorshift(&mut rng),
        })
        .collect()
}

/// One dash of the day's sun glitter. `x` is a width fraction packed
/// toward the sun's azimuth; `depth_px` sits the dash just under the
/// crest ink; the flash loops on an integer rate (wrap-safe).
#[derive(Debug, Clone, Copy)]
struct GlitterParam {
    x: f32,
    depth_px: f32,
    len: f32,
    k: u32,
    off: f32,
}

/// Deal the glitter lane — deterministic, const-seeded. The `powf(1.6)`
/// packs the dashes under the sun (at `MOON_X` 0.15) and thins them
/// toward mid-panel; `.min(5)` clamps xorshift's inclusive 1.0 so `k`
/// stays in 6..=11.
fn glitter_params() -> Vec<GlitterParam> {
    let mut rng = GLITTER_SEED;
    (0..GLITTER_COUNT)
        .map(|_| {
            let r = xorshift(&mut rng);
            GlitterParam {
                x: 0.03 + 0.52 * r.powf(1.6),
                depth_px: 1.5 + 2.5 * xorshift(&mut rng),
                len: 2.5 + 2.0 * xorshift(&mut rng),
                k: 6 + ((xorshift(&mut rng) * 6.0) as u32).min(5),
                off: xorshift(&mut rng),
            }
        })
        .collect()
}

/// Hash a `(cycle, salt)` pair into `[0, 1)` — the deterministic dice the
/// rare events (shooting star, fish) roll once per sea cycle. Three
/// xorshift rounds decorrelate consecutive cycle values; the multiply-mix
/// keeps a zero cycle from collapsing the stream.
fn hash01(cycle: u32, salt: u32) -> f32 {
    let mut s = cycle.wrapping_mul(0x9E37_79B9) ^ salt;
    if s == 0 {
        s = salt | 1;
    }
    // Murmur3's multiplicative finalizer — NOT plain xorshift rounds.
    // Xorshift is GF(2)-linear, which made sibling-salted hashes differ
    // by a cycle-independent XOR constant: CONDITIONED on a rare-event
    // gate passing (top bits of the gate hash pinned near zero), every
    // derived deal (center, start, depth…) collapsed to a sliver of its
    // range — the black hole opened at the same spot at the same moment
    // every event, forever. The multiplies break the linearity, so
    // deals stay independent even under the gate condition (pinned by
    // `hashed_deals_spread_even_conditioned_on_the_gate`).
    s ^= s >> 16;
    s = s.wrapping_mul(0x85EB_CA6B);
    s ^= s >> 13;
    s = s.wrapping_mul(0xC2B2_AE35);
    s ^= s >> 16;
    (s as f32) / (u32::MAX as f32)
}

/// Hermite smoothstep on `[e0, e1]` — the black-hole event's easing
/// brick (envelope + grip falloff; the plunge itself is a power law).
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The capture profile over the black hole event — GRAVITY's time
/// signature, not an ease: the plunge ACCELERATES (a lazy drift that
/// becomes a dive, `p^PLUNGE_POW`), the catch holds the stars trapped
/// at the core, and the spit-out `(1-q)²·(1-b·q)` starts fast, sails
/// PAST home (the negative dip — radius beyond the star's rest
/// position), and settles gently back. EXACTLY zero at both window
/// ends — f32-exact, which is what lets the constellation's
/// static-positions contract survive the event boundary.
fn blackhole_s(p: f32) -> f32 {
    if p <= 0.0 {
        0.0
    } else if p < BLACKHOLE_PLUNGE_END {
        (p / BLACKHOLE_PLUNGE_END).powf(BLACKHOLE_PLUNGE_POW)
    } else if p < BLACKHOLE_HOLD_END {
        1.0
    } else if p < 1.0 {
        let q = (p - BLACKHOLE_HOLD_END) / (1.0 - BLACKHOLE_HOLD_END);
        (1.0 - q) * (1.0 - q) * (1.0 - BLACKHOLE_OVERSHOOT * q)
    } else {
        0.0
    }
}

/// This cycle's hole center as width/height fractions — hashed into
/// the open upper-right sky, structurally clear of the moon at
/// (`MOON_X`, `MOON_Y`) and inside the sky band.
fn blackhole_center(cycle: u32) -> (f32, f32) {
    (
        0.52 + 0.30 * hash01(cycle, BLACKHOLE_SALT ^ 0x0C31),
        0.09 + 0.13 * hash01(cycle, BLACKHOLE_SALT ^ 0x0C32),
    )
}

/// How strongly the well at distance `dist` grips a star: full capture
/// inside 35% of `capture_px`, fading smoothly to ZERO at the full
/// radius — gravity is LOCAL; the rest of the sky never stirs.
fn blackhole_grip(dist: f32, capture_px: f32) -> f32 {
    smoothstep(capture_px, 0.35 * capture_px, dist)
}

/// Displace a sky glyph under the hole's gravity: effective pull =
/// `s · grip(dist)`, radius collapsing toward `BLACKHOLE_CONVERGE` of
/// home while the angle winds by up to `BLACKHOLE_SWIRL` — inner stars
/// wind ~1.7× more (differential rotation). `spin` adds the catch's
/// orbital winding (computed once per frame from `s` and `p`, zero at
/// both window ends), scaled per star by its grip so untouched stars
/// stay untouched. During the spit-out `s` goes NEGATIVE and the same
/// formula throws the star past home (and unwinds past its rest angle
/// — the outward whip). At `s == 0` this returns `home` EXACTLY (early
/// return — no atan2 round-trip error): the bit-identical-boundary
/// guarantee, and the untouched-sky guarantee for everything beyond
/// the capture radius.
fn blackhole_displace(home: Point, hole: Point, s: f32, spin: f32, capture_px: f32) -> Point {
    if s == 0.0 {
        return home;
    }
    let dx = home.x - hole.x;
    let dy = home.y - hole.y;
    let r = (dx * dx + dy * dy).sqrt();
    if r <= f32::EPSILON {
        return home;
    }
    let grip = blackhole_grip(r, capture_px);
    if grip <= 0.0 {
        return home;
    }
    let eff = s * grip;
    let wind = 1.0 + 0.7 * (1.0 - (r / capture_px).min(1.0));
    let theta = dy.atan2(dx) + (eff * BLACKHOLE_SWIRL + spin * grip) * wind;
    let r2 = r * (1.0 - eff * (1.0 - BLACKHOLE_CONVERGE));
    Point::new(hole.x + theta.cos() * r2, hole.y + theta.sin() * r2)
}

/// How much of a glyph's light survives the horizon: `1.0` untouched,
/// `0.0` fully swallowed. The fade keys on the CURRENT (displaced)
/// distance — light dies between 1.6× and 0.6× the horizon radius —
/// and its depth gates on `|s|·grip`, so a star whose HOME happens to
/// sit beside a hashed center is untouched at the window boundaries
/// and on non-event frames (visibility exactly 1 when displacement is
/// exactly zero — the same bit-identity contract as position).
fn blackhole_visibility(dist_now: f32, horizon_px: f32, s: f32, grip: f32) -> f32 {
    let proximity = smoothstep(1.6 * horizon_px, 0.6 * horizon_px, dist_now);
    let gate = (s.abs() * grip * 4.0).min(1.0);
    1.0 - proximity * gate
}

/// Does this cycle dream? Cycle 0 always does while the launch greeting
/// is on — the app opens on Harbour centred on the Trawl row, so the
/// first full cycle after launch carries the ritual — then the usual
/// hashed dice, at the black hole's rarity.
fn moon_dream_cycle(cycle: u32) -> bool {
    (MOON_DREAM_GREETS_LAUNCH && cycle == 0) || hash01(cycle, MOON_DREAM_SALT) < MOON_DREAM_CHANCE
}

/// Progress through this cycle's dream window: `Some(p ∈ [0, 1])` while
/// the ritual plays, `None` on every other frame (including all of a
/// non-dream cycle). The hashed start (2.0–5.0 s in) also buys a cold
/// launch time to land its shelves before the first verse sounds.
fn moon_dream_progress(phase: f32, cycle: u32) -> Option<f32> {
    if !moon_dream_cycle(cycle) {
        return None;
    }
    let start = 0.10 + 0.15 * hash01(cycle, MOON_DREAM_SALT ^ 0x9E37);
    let p = (phase - start) / MOON_DREAM_WINDOW;
    (0.0..=1.0).contains(&p).then_some(p)
}

/// The four mark alphas [smile, eye, patch, strap] at dream progress
/// `p` — each `min(easing in, fading out)`: zero before its verse
/// summons it, one through the whole-face hold, zero again after its
/// farewell. EXACTLY 0.0 at both window ends (the boundary identity
/// the purity contract rides on — the frames either side of a dream
/// are the bare resting disc).
fn moon_dream_alphas(p: f32) -> [f32; 4] {
    let t = p.clamp(0.0, 1.0) * MOON_DREAM_SECS;
    let mut alphas = [0.0f32; 4];
    for (i, alpha) in alphas.iter_mut().enumerate() {
        let in_start =
            MOON_DREAM_VERSE_START + i as f32 * MOON_DREAM_VERSE_SPAN + MOON_DREAM_MARK_LAG;
        let rise = smoothstep(in_start, in_start + MOON_DREAM_IN_SECS, t);
        let out_start = MOON_DREAM_OUT_START[i];
        let fall = 1.0 - smoothstep(out_start, out_start + MOON_DREAM_OUT_SECS, t);
        *alpha = rise.min(fall);
    }
    alphas
}

/// Verse `line`'s alpha at dream progress `p`. The four verse windows
/// tile the recital exactly (each fades to zero at its own ends), so at
/// most one verse is ever audible — pinned by
/// `moon_dream_verses_speak_one_at_a_time`.
fn moon_dream_verse_alpha(p: f32, line: usize) -> f32 {
    let t = p.clamp(0.0, 1.0) * MOON_DREAM_SECS;
    let s = MOON_DREAM_VERSE_START + line as f32 * MOON_DREAM_VERSE_SPAN;
    let e = s + MOON_DREAM_VERSE_SPAN;
    smoothstep(s, s + MOON_DREAM_VERSE_FADE, t).min(smoothstep(e, e - MOON_DREAM_VERSE_FADE, t))
}

/// The veil cache key for this frame: the dream's mark alphas quantized
/// to [`crate::embedded_svg::MOON_VEIL_STEPS`] steps — or the resting
/// BARE key (the plain disc) on every frame outside a dream. Quantizing
/// keeps the per-key handle cache on `BoatState` bounded (~131 distinct
/// documents across the whole choreography instead of one per frame);
/// one step is sub-JND after the scene's own `MOON_ALPHA` scaling.
/// Shared by the tick (which warms the handle) and `trawl_scene` (which
/// renders it) — one source, no drift.
pub(crate) fn moon_dream_veil_key(phase: f32, cycle: u32) -> [u8; 4] {
    let Some(p) = moon_dream_progress(phase, cycle) else {
        return crate::embedded_svg::MOON_VEIL_BARE;
    };
    let steps = f32::from(crate::embedded_svg::MOON_VEIL_STEPS);
    moon_dream_alphas(p).map(|a| (a * steps).round() as u8)
}

/// One rising note's fixed parameters (the per-frame position falls out of
/// the phase). Deterministic, const-seeded — same contract as the sky.
#[derive(Debug, Clone, Copy)]
struct RiserParam {
    /// Integer phase multiplier: how many rises per 20 s sea cycle.
    k: u32,
    /// Phase offset staggering the pool.
    off: f32,
    /// Horizontal offset from the mast, in glyph-scale pixels.
    dx: f32,
    /// Sway phase offset.
    sway_off: f32,
    /// `true` = beamed pair, `false` = single quaver.
    beamed: bool,
}

/// Deal the riser pool. Alternating rise rates (1 or 2 per cycle) keep the
/// stream from ever synchronizing into a volley.
fn riser_params() -> Vec<RiserParam> {
    let mut rng = RISER_SEED;
    (0..RISER_COUNT)
        .map(|i| RiserParam {
            k: 1 + (i as u32 % 2),
            off: xorshift(&mut rng),
            dx: (xorshift(&mut rng) - 0.5) * 30.0,
            sway_off: xorshift(&mut rng),
            beamed: xorshift(&mut rng) < 0.5,
        })
        .collect()
}

/// One rising bubble of the anchor's stream. Fixed pool, dealt once
/// from `BUBBLE_SEED`; per-frame position falls out of the phase (the
/// riser contract).
#[derive(Debug, Clone, Copy)]
struct BubbleParam {
    /// Integer rises per sea cycle (wrap-safety).
    k: u32,
    /// Phase offset staggering the stream.
    off: f32,
    /// Horizontal offset from the anchor, in glyph-scale pixels.
    dx: f32,
    /// Unit radius scale.
    size: f32,
    /// Sway phase offset.
    sway_off: f32,
    /// `true` = stroked ring (the big ones), `false` = filled fleck.
    ring: bool,
}

/// Deal the bubble pool. Alternating rise rates (2 or 3 per cycle) keep
/// the stream from synchronizing into a volley — the riser rule.
fn bubble_params() -> Vec<BubbleParam> {
    let mut rng = BUBBLE_SEED;
    (0..BUBBLE_COUNT)
        .map(|i| BubbleParam {
            k: 2 + (i as u32 % 2),
            off: xorshift(&mut rng),
            dx: (xorshift(&mut rng) - 0.5) * 22.0,
            size: 0.6 + xorshift(&mut rng) * 0.7,
            sway_off: xorshift(&mut rng),
            ring: xorshift(&mut rng) < BUBBLE_RING_FRACTION,
        })
        .collect()
}

/// One drifter of the mid-water school. Same glide contract as the day
/// scene's gulls: integer crossings per cycle over an off-panel margin.
#[derive(Debug, Clone, Copy)]
struct SchoolFishParam {
    x0: f32,
    y: f32,
    k: u32,
    leftward: bool,
    size: f32,
    bob_k: u32,
    bob_off: f32,
}

/// Deal the school — deterministic, const-seeded, gull rules underwater.
fn school_params() -> Vec<SchoolFishParam> {
    let mut rng = SCHOOL_SEED;
    (0..SCHOOL_COUNT)
        .map(|_| SchoolFishParam {
            x0: xorshift(&mut rng),
            y: SCHOOL_BAND_TOP + xorshift(&mut rng) * (SCHOOL_BAND_BOTTOM - SCHOOL_BAND_TOP),
            k: 1 + (xorshift(&mut rng) * 2.0) as u32,
            leftward: xorshift(&mut rng) < 0.5,
            size: 0.8 + xorshift(&mut rng) * 0.4,
            bob_k: 2 + (xorshift(&mut rng) * 3.0) as u32,
            bob_off: xorshift(&mut rng),
        })
        .collect()
}

/// One kelp frond. `x` is the root as a width fraction; `height` a
/// scene-height fraction; sway loops on an integer rate (wrap-safe);
/// `lean` is a static tip bias in glyph-scale pixels so the fronds
/// don't all stand at attention. `seep_k`/`seep_off` drive the slow
/// bubble seeping from the root.
#[derive(Debug, Clone, Copy)]
struct KelpParam {
    x: f32,
    height: f32,
    sway_k: u32,
    sway_off: f32,
    lean: f32,
    seep_k: u32,
    seep_off: f32,
}

/// Deal the kelp beds — a cluster on each flank plus two shorter loners
/// toward the middle (fixed roots × height factors; jitter from the
/// stream). The variety is what makes the beds read as growth.
fn kelp_params() -> Vec<KelpParam> {
    let mut rng = KELP_SEED;
    [
        (0.035_f32, 1.0_f32),
        (0.075, 1.2),
        (0.115, 0.8),
        (0.30, 0.55),
        (0.68, 0.6),
        (0.91, 1.1),
        (0.955, 0.75),
    ]
    .into_iter()
    .map(|(base, tall)| KelpParam {
        x: base + 0.015 * (xorshift(&mut rng) - 0.5),
        height: (0.14 + 0.05 * xorshift(&mut rng)) * tall,
        sway_k: 1 + (xorshift(&mut rng) * 2.0) as u32,
        sway_off: xorshift(&mut rng),
        lean: (xorshift(&mut rng) - 0.5) * 6.0,
        seep_k: 1 + (xorshift(&mut rng) * 2.0) as u32,
        seep_off: xorshift(&mut rng),
    })
    .collect()
}

/// One rock mound of the bed dressing. `x` a width fraction; `w`/`ht`
/// unit scales for the dome's pixel base.
#[derive(Debug, Clone, Copy)]
struct RockParam {
    x: f32,
    w: f32,
    ht: f32,
}

/// The bed's static furniture: rock mounds plus one resting starfish
/// (position, rotation, arm scale). One seed stream deals everything,
/// so the arrangement is identical every frame and launch.
#[derive(Debug, Clone)]
struct BedDressing {
    rocks: Vec<RockParam>,
    star_x: f32,
    star_rot: f32,
    star_size: f32,
}

/// Deal the bed dressing — deterministic, const-seeded. Rocks spread
/// across the middle of the lane; the starfish rests near (but off)
/// them.
fn bed_dressing() -> BedDressing {
    let mut rng = BED_SEED;
    let rocks = (0..ROCK_COUNT)
        .map(|_| RockParam {
            x: 0.15 + 0.60 * xorshift(&mut rng),
            w: 0.7 + 0.6 * xorshift(&mut rng),
            ht: 0.55 + 0.45 * xorshift(&mut rng),
        })
        .collect();
    BedDressing {
        rocks,
        star_x: 0.78 + 0.12 * xorshift(&mut rng),
        star_rot: xorshift(&mut rng) * std::f32::consts::TAU,
        star_size: 0.85 + 0.3 * xorshift(&mut rng),
    }
}

/// Fill a resting starfish: a fat five-arm star polygon (alternating
/// outer/inner vertices, inner at 0.55 of the arm) lying flat on the
/// bed. Chunky arms keep it unmistakably a CREATURE — the sky's stars
/// are dots, so the silhouettes never collide.
fn fill_starfish(frame: &mut canvas::Frame, center: Point, arm: f32, rot: f32, color: Color) {
    use std::f32::consts::TAU;
    let star = canvas::Path::new(|b| {
        for i in 0..10 {
            let ang = rot + i as f32 * (TAU / 10.0) - TAU / 4.0;
            let rad = if i % 2 == 0 { arm } else { 0.55 * arm };
            let p = Point::new(center.x + ang.cos() * rad, center.y + ang.sin() * rad);
            if i == 0 {
                b.move_to(p);
            } else {
                b.line_to(p);
            }
        }
        b.close();
    });
    frame.fill(&star, color);
}

/// Fill the fish silhouette — teardrop body + notched tail — at the
/// current frame origin, `l` px long, nose toward +x when `dir` is
/// `1.0` (pass `-1.0` to mirror). Shared by the rare leaping fish and
/// the drifting school so the two can never drift apart in shape.
fn fill_fish_silhouette(frame: &mut canvas::Frame, l: f32, dir: f32, color: Color) {
    let body = canvas::Path::new(|b| {
        b.move_to(Point::new(dir * -0.50 * l, 0.0));
        b.quadratic_curve_to(
            Point::new(dir * -0.10 * l, -0.35 * l),
            Point::new(dir * 0.45 * l, 0.0),
        );
        b.quadratic_curve_to(
            Point::new(dir * -0.10 * l, 0.35 * l),
            Point::new(dir * -0.50 * l, 0.0),
        );
        b.close();
    });
    let tail = canvas::Path::new(|b| {
        b.move_to(Point::new(dir * -0.45 * l, 0.0));
        b.line_to(Point::new(dir * -0.78 * l, -0.24 * l));
        b.line_to(Point::new(dir * -0.70 * l, 0.0));
        b.line_to(Point::new(dir * -0.78 * l, 0.24 * l));
        b.close();
    });
    frame.fill(&body, color);
    frame.fill(&tail, color);
}

/// Gradient stops for the crest shimmer sweep: a triangular brightness
/// profile of half-width `half` centered at `c` (which sweeps from off-left
/// to off-right), clipped to the drawable `[0, 1]` gradient domain with the
/// boundary values interpolated — stops stay ascending and the last lands
/// at exactly 1.0 (the packed-gradient contract).
fn sweep_stops(c: f32, half: f32, peak: f32) -> Vec<(f32, f32)> {
    let profile = |x: f32| (1.0 - ((x - c).abs() / half)).max(0.0) * peak;
    let mut xs = vec![0.0_f32, 1.0];
    // Candidates are kept an epsilon INSIDE (0, 1) so none can collide
    // with a boundary stop — a naive dedup could otherwise keep a
    // 0.9999… candidate and DROP the exact-1.0 boundary, breaking the
    // packed-gradient "last stop at 1.0" contract. Candidates can't
    // collide with each other (they are `half` apart, ≫ epsilon).
    const EDGE_EPS: f32 = 1e-3;
    for cand in [c - half, c, c + half] {
        if cand > EDGE_EPS && cand < 1.0 - EDGE_EPS {
            xs.push(cand);
        }
    }
    xs.sort_by(f32::total_cmp);
    xs.into_iter().map(|x| (x, profile(x))).collect()
}

/// Draw a beamed eighth-note pair (the `music-2` icon's shape) as canvas
/// paths: two filled heads, a stem off each head's right edge, and a
/// slanted beam joining the stem tops. Shared by the static sky glyphs and
/// the rising notes so the two can never drift apart in style.
fn draw_note_pair(frame: &mut canvas::Frame, center: Point, s: f32, color: Color) {
    let head_r = 0.16 * s;
    let dx = 0.55 * s; // second head sits right + slightly up
    let dy = 0.12 * s;
    let stem_h = 0.85 * s;
    let head_a = center;
    let head_b = Point::new(center.x + dx, center.y - dy);
    frame.fill(&canvas::Path::circle(head_a, head_r), color);
    frame.fill(&canvas::Path::circle(head_b, head_r), color);
    let stems = canvas::Path::new(|b| {
        b.move_to(Point::new(head_a.x + head_r, head_a.y));
        b.line_to(Point::new(head_a.x + head_r, head_a.y - stem_h));
        b.move_to(Point::new(head_b.x + head_r, head_b.y));
        b.line_to(Point::new(head_b.x + head_r, head_b.y - stem_h));
    });
    frame.stroke(
        &stems,
        canvas::Stroke::default()
            .with_color(color)
            .with_width(1.0)
            .with_line_cap(canvas::LineCap::Round),
    );
    let beam = canvas::Path::new(|b| {
        b.move_to(Point::new(head_a.x + head_r, head_a.y - stem_h));
        b.line_to(Point::new(head_b.x + head_r, head_b.y - stem_h));
    });
    frame.stroke(
        &beam,
        canvas::Stroke::default()
            .with_color(color)
            .with_width(0.16 * s)
            .with_line_cap(canvas::LineCap::Round),
    );
}

/// Draw a single flagged eighth note: filled head, stem, and a little
/// quadratic flag curling off the stem top.
fn draw_quaver(frame: &mut canvas::Frame, center: Point, s: f32, color: Color) {
    let head_r = 0.18 * s;
    let stem_h = 0.95 * s;
    let stem_x = center.x + head_r * 0.9;
    frame.fill(&canvas::Path::circle(center, head_r), color);
    let stem_and_flag = canvas::Path::new(|b| {
        b.move_to(Point::new(stem_x, center.y));
        b.line_to(Point::new(stem_x, center.y - stem_h));
        b.quadratic_curve_to(
            Point::new(stem_x + 0.38 * s, center.y - 0.78 * s),
            Point::new(stem_x + 0.30 * s, center.y - 0.45 * s),
        );
    });
    frame.stroke(
        &stem_and_flag,
        canvas::Stroke::default()
            .with_color(color)
            .with_width(1.0)
            .with_line_cap(canvas::LineCap::Round),
    );
}

/// Build the front sea height field for `phase ∈ [0, 1)` — heights in
/// `[0, 1]` of panel height, `SEA_POINTS` samples. This is the ONE array
/// the physics steps against and the canvas draws; see the module docs'
/// coherence contract.
pub(crate) fn sea_bars(phase: f32) -> Vec<f64> {
    use std::f64::consts::TAU;
    let ph = phase as f64;
    (0..SEA_POINTS)
        .map(|i| {
            let x = i as f64 / (SEA_POINTS - 1) as f64;
            let swell = SWELL_AMP * (TAU * (x * SWELL_CYCLES - SWELL_PHASE_K * ph)).sin();
            let ripple =
                RIPPLE_AMP * (TAU * (x * RIPPLE_CYCLES - RIPPLE_PHASE_K * ph) + RIPPLE_SHIFT).sin();
            (SEA_DC + swell + ripple).clamp(0.0, 1.0)
        })
        .collect()
}

/// Height of the decorative back swell at `x ∈ [0, 1]` — drawn behind the
/// front waterline at half its crest speed for the parallax depth read.
/// Analytic (no array) because only the canvas consumes it.
fn back_swell_height(x: f64, phase: f32) -> f64 {
    use std::f64::consts::TAU;
    let ph = phase as f64;
    (SEA_DC
        + BACK_RAISE
        + BACK_AMP * (TAU * (x * BACK_CYCLES - BACK_PHASE_K * ph) + BACK_SHIFT).sin())
    .clamp(0.0, 1.0)
}

/// The Harbour Trawl panel: the animated sea with the longship trawling
/// across it, docked above the banded TRAWL pill.
///
/// A COLUMN (not the overlay stack every art-backed panel uses): the pill
/// reserves its own height, so the sea's canvas bottom — the seabed the
/// anchor drags along — lands exactly on the pill's top rail instead of
/// hiding behind the opaque `bg0_hard` band. The inner `responsive` gives
/// the boat and sea the real pixels of the region ABOVE the pill, keeping
/// the sprite sized to the visible water (and dodging the Fill-in-Shrink
/// flex-compression gotcha by carrying bounded sizes itself).
///
/// MODE PARITY with the sibling panels is load-bearing: in Auto / native
/// artwork modes, `horizontal_layout` passes the panel through RAW and
/// sizes the whole artwork column off the panel's natural size — the
/// contract is "panels shrink to a `min(w, h)` square" (see
/// `single_artwork_panel_inner`'s square arm). A Fill panel here balloons
/// the column to the reserved maximum, wider than every sibling, and the
/// elevated-mode nav overlay then juts INTO the scene. Only the stretched
/// modes (where the layout wraps the panel in a user-tuned
/// `Length::Fixed(extent)`) get the full-bleed Fill treatment.
///
/// `pill` is a FACTORY (not an element): the square arm builds the panel
/// inside a `responsive` closure, which is a `Fn` the runtime may invoke
/// repeatedly — a moved-in element could be consumed only once.
pub(crate) fn trawl_scene<'a, M: 'a>(
    boat: &'a BoatState,
    sea_bars: &'a [f64],
    sea_phase: f32,
    sea_cycle: u32,
    pill: impl Fn() -> Element<'a, M> + 'a,
) -> Element<'a, M> {
    use iced::widget::{column, container, stack};

    // The bubble stream's source, snapshotted as a scalar so the `Copy`
    // scene closure can capture it. `trawled_anchor_x` is the SAME
    // source `boat_overlay` places the anchor sprite at — one method,
    // no drift.
    let anchor_x = boat.trawled_anchor_x(TRAIL_OFFSET);

    // The scene's layer stack at known pixel dimensions. A `Copy` closure
    // (all captures are shared refs / scalars) so both mode arms — and the
    // square arm's NESTED responsive — can each take their own copy.
    let scene_layers = move |w: f32, h: f32| -> Element<'a, M> {
        // Sky + sea backdrop on the shared artwork background so the panel
        // reads as a sibling of every other artwork column state.
        let backdrop = container(iced::widget::Space::new())
            .width(Length::Fixed(w))
            .height(Length::Fixed(h))
            .style(|_theme| container::Style {
                background: Some(crate::widgets::base_slot_list_layout::artwork_outer_bg().into()),
                ..Default::default()
            });

        let sea = canvas::Canvas::new(SeaCanvas {
            bars: sea_bars,
            phase: sea_phase,
            cycle: sea_cycle,
            boat_x: boat.x_ratio,
            anchor_x,
        })
        .width(Length::Fixed(w))
        .height(Length::Fixed(h));

        // The longship, trawling: full opacity (it's the panel's content,
        // not an overlay dimmed against art), mirror off (the harbour sea
        // has no lower reflection), anchor trailed on the seabed.
        let boat_el = boat_overlay::<M>(boat, w, h, w.min(h), 1.0, false, Some(TRAIL_OFFSET));

        let mut layers = stack![backdrop, sea];
        // The moon: a bare disc at rest, themed live via the shared LOGO
        // tokens and cached on BoatState beside the boat/anchor handles
        // (same theme-generation invalidation; warmed by the tick, with
        // the boat overlay's rebuild-on-miss fallback). Placed over the
        // canvas — its halo is drawn there at the same shared consts —
        // and under the ship. During a moon dream the handle is the
        // veiled document for this frame's quantized key (the SAME
        // `moon_dream_veil_key(phase, cycle)` the canvas verses and the
        // tick's cache-warm read — one clock, no drift); every other
        // frame it is the bare resting disc.
        if MOON_ALPHA > 0.0 {
            let moon_r = MOON_RADIUS_PX * scene_glyph_scale(h);
            let veil = moon_dream_veil_key(sea_phase, sea_cycle);
            let handle = boat.cached_moon_veil_handle(veil).unwrap_or_else(|| {
                iced::widget::svg::Handle::from_memory(
                    crate::embedded_svg::themed_moon_face_veiled(veil).into_bytes(),
                )
            });
            layers = layers.push(
                container(
                    iced::widget::Svg::new(handle)
                        .width(Length::Fixed(2.0 * moon_r))
                        .height(Length::Fixed(2.0 * moon_r))
                        .opacity(MOON_ALPHA),
                )
                .padding(
                    iced::Padding::new(0.0)
                        .left((MOON_X * w - moon_r).max(0.0))
                        .top((MOON_Y * h - moon_r).max(0.0)),
                ),
            );
        }
        layers.push(boat_el).into()
    };

    if crate::theme::artwork_column_mode().is_stretched() {
        // Stretched modes: the layout bounds the column at the user-tuned
        // extent, so Fill is authoritative and the scene runs full-bleed.
        let scene = iced::widget::responsive(move |size| {
            scene_layers(size.width.max(1.0), size.height.max(1.0))
        });
        column![
            container(scene).width(Length::Fill).height(Length::Fill),
            crate::widgets::base_slot_list_layout::banded_pill(pill()),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    } else {
        // Auto / native: the sibling square contract — resolve to a
        // `min(w, h)` square via Shrink so the artwork column sizes off
        // the same natural square as every other panel.
        iced::widget::responsive(move |size| {
            let s = size.width.min(size.height).max(1.0);
            let scene = iced::widget::responsive(move |scene_size| {
                scene_layers(scene_size.width.max(1.0), scene_size.height.max(1.0))
            });
            let panel: Element<'_, M> = container(
                column![
                    container(scene).width(Length::Fill).height(Length::Fill),
                    crate::widgets::base_slot_list_layout::banded_pill(pill()),
                ]
                .width(Length::Fixed(s))
                .height(Length::Fixed(s)),
            )
            .into();
            panel
        })
        .width(Length::Shrink)
        .height(Length::Shrink)
        .into()
    }
}

/// Canvas program drawing the two water layers. Inert and event-transparent
/// (a structural sibling of the boat's `RopeCanvas` — no `Cache`, geometry
/// rebuilt per frame, which is correct for a field that changes every tick).
///
/// The FRONT layer is sampled from the SAME bars array the boat physics
/// stepped against, through the SAME [`sample_line_height`] Catmull-Rom
/// sampler — that is what keeps the hull visually sitting ON the water. The
/// BACK layer is decorative parallax, computed analytically from the phase.
struct SeaCanvas<'a> {
    bars: &'a [f64],
    phase: f32,
    /// Completed phase cycles — dice for the rare events.
    cycle: u32,
    /// The boat's live `x_ratio` — anchors the lantern glint and the
    /// rising notes to the hull. May exceed `[0, 1]` in the wrap margin;
    /// the boat-coupled passes gate on that.
    boat_x: f32,
    /// The trawled anchor's x (from `BoatState::trawled_anchor_x` — the
    /// same source the sprite is placed at). The bubble stream's source;
    /// roams the wrap margin like `boat_x`, so the bubble pass
    /// edge-fades on it.
    anchor_x: f32,
}

impl<Message> canvas::Program<Message> for SeaCanvas<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let size = bounds.size();
        let (w, h) = (size.width, size.height);
        if w <= 0.0 || h <= 0.0 || self.bars.is_empty() {
            return Vec::new();
        }

        let mut frame = canvas::Frame::new(renderer, size);

        // Water palette from the dark-variant visualizer colors — the same
        // mode-stable family the boat outline, rope, and anchor are themed
        // with, so the whole doodad reads as one system in light AND dark.
        let viz = crate::theme::get_visualizer_colors_dark();
        let water = viz
            .bar_gradient_colors
            .first()
            .and_then(|c| parse_hex_color(c))
            .unwrap_or(Color::from_rgb(0.35, 0.5, 0.6));
        let crest = parse_hex_color(&viz.border_color).unwrap_or(Color::from_rgb(0.5, 0.5, 0.5));
        // Starlight: the PEAK gradient's first stop — the visualizer's
        // bright sparkle-top, the one reliably light color in every theme's
        // visualizer palette. The border color the rope/boat use is a DARK
        // stroke in most themes (Svalbard: #111817), which vanishes on the
        // dark sky; the water gradient is mid-tone. Peaks read as stars.
        let starlight = viz
            .peak_gradient_colors
            .first()
            .and_then(|c| parse_hex_color(c))
            .or_else(|| {
                viz.bar_gradient_colors
                    .last()
                    .and_then(|c| parse_hex_color(c))
            })
            .unwrap_or(Color::from_rgb(0.9, 0.92, 0.92));

        let steps = ((w / SEA_DRAW_STEP_PX).ceil() as usize).max(2);

        // A closed fill path under a height function: crest polyline, then
        // down the right edge, along the bottom, and back up to the start.
        let fill_under = |height_at: &dyn Fn(f32) -> f32| {
            canvas::Path::new(|b| {
                b.move_to(Point::new(0.0, height_at(0.0)));
                for i in 1..=steps {
                    let x = w * (i as f32 / steps as f32);
                    b.line_to(Point::new(x, height_at(x)));
                }
                b.line_to(Point::new(w, h));
                b.line_to(Point::new(0.0, h));
                b.close();
            })
        };

        let phase = self.phase;
        let bars = self.bars;
        let back_y = move |x: f32| h - (back_swell_height((x / w) as f64, phase) as f32) * h;
        let front_y = move |x: f32| h - sample_line_height(bars, x / w, false) * h;

        // ── Gradient block A ─────────────────────────────────────────────
        // The four gradient fills draw contiguously (mesh batching: a
        // solid↔gradient alternation splits vertex buffers; grouping keeps
        // the whole frame at three buffers).
        //
        // (1) Sky airglow: a barely-there cold luminance gathering toward
        // the waterline and fading back out below it — the value story that
        // motivates every highlight in the scene (light lands at the
        // surface). Fading to alpha-0 at BOTH ends leaves no seam anywhere.
        let glow_peak = 1.0 - SEA_DC as f32;
        frame.fill_rectangle(
            Point::ORIGIN,
            size,
            canvas::gradient::Linear::new(Point::ORIGIN, Point::new(0.0, h))
                .add_stop(
                    0.0,
                    Color {
                        a: 0.0,
                        ..starlight
                    },
                )
                .add_stop(
                    glow_peak,
                    Color {
                        a: SKY_GLOW_ALPHA,
                        ..starlight
                    },
                )
                .add_stop(
                    1.0,
                    Color {
                        a: 0.0,
                        ..starlight
                    },
                ),
        );

        // (2) Back swell — atmospheric perspective: its crest dissolves
        // into the sky like distance haze while its body keeps its weight,
        // reaching full strength ABOVE the front waterline so the
        // transition is visible. Geometry unchanged.
        let water_far = viz
            .bar_gradient_colors
            .get(1)
            .and_then(|c| parse_hex_color(c))
            .unwrap_or(water);
        let back_top = (1.0 - (SEA_DC + BACK_RAISE + BACK_AMP) as f32) * h;
        frame.fill(
            &fill_under(&back_y),
            canvas::gradient::Linear::new(Point::new(0.0, back_top), Point::new(0.0, h))
                .add_stop(
                    0.0,
                    Color {
                        a: SEA_BACK_TOP_ALPHA,
                        ..water_far
                    },
                )
                .add_stop(
                    SEA_BACK_FADE_STOP,
                    Color {
                        a: SEA_BACK_BODY_ALPHA,
                        ..water
                    },
                )
                .add_stop(
                    1.0,
                    Color {
                        a: SEA_BACK_BODY_ALPHA,
                        ..water
                    },
                ),
        );

        // (3) Front water walks the theme ramp: brightest at the lit
        // surface (a brighter gradient slot), sinking through the sea-teal
        // toward the deep. Static endpoints (the max-crest line) — no
        // per-frame gradient churn, and above-start pixels clamp to stop 0,
        // which also absorbs any Catmull-Rom overshoot. Deliberately NO ink
        // stop here: all darkness lives in the bed rect below, capping the
        // total bottom darkening so the anchor keeps its contrast.
        let water_lit = viz
            .bar_gradient_colors
            .get(2)
            .and_then(|c| parse_hex_color(c))
            .unwrap_or(water);
        let surface_y = (1.0 - (SEA_DC + SWELL_AMP + RIPPLE_AMP) as f32) * h;
        frame.fill(
            &fill_under(&front_y),
            canvas::gradient::Linear::new(Point::new(0.0, surface_y), Point::new(0.0, h))
                .add_stop(
                    0.0,
                    Color {
                        a: SEA_LIT_ALPHA,
                        ..water_lit
                    },
                )
                .add_stop(
                    0.5,
                    Color {
                        a: SEA_MID_ALPHA,
                        ..water
                    },
                )
                .add_stop(
                    1.0,
                    Color {
                        a: SEA_DEEP_ALPHA,
                        ..water
                    },
                ),
        );

        // (4) Seabed ink vignette: the bottom of the water deepens toward
        // ink, grounding the trawled anchor and easing the old razor cut
        // into the pill band's bg0_hard. Starts below the deepest possible
        // trough so the darkening never rides the surface; dialed by
        // border_opacity, the theme's light-mode legibility knob.
        let bed_top = SEA_BED_TOP * h;
        frame.fill_rectangle(
            Point::new(0.0, bed_top),
            Size::new(w, h - bed_top),
            canvas::gradient::Linear::new(Point::new(0.0, bed_top), Point::new(0.0, h))
                .add_stop(0.0, Color { a: 0.0, ..crest })
                .add_stop(
                    1.0,
                    Color {
                        a: SEA_BED_ALPHA * viz.border_opacity,
                        ..crest
                    },
                ),
        );

        // (5) Aurora — two seafoam curtains undulating across the upper
        // sky (the default theme is named Svalbard; this is its light).
        // Each ribbon is a closed band between two travelling sinusoids,
        // filled with a vertical gradient fading to nothing at both edges.
        // Phase multipliers are integers (wrap-safe); the bands drift at
        // different rates and breathe gently out of step.
        let aurora_a = viz
            .bar_gradient_colors
            .get(3)
            .and_then(|c| parse_hex_color(c))
            .unwrap_or(water);
        let aurora_b = viz
            .bar_gradient_colors
            .get(4)
            .and_then(|c| parse_hex_color(c))
            .unwrap_or(water);
        let breath = 1.0
            - AURORA_BREATH_DEPTH
                * (0.5 + 0.5 * (std::f64::consts::TAU * AURORA_BREATH_K * phase as f64).sin())
                    as f32;
        let mut aurora_ribbon = |base: f32,
                                 amp: f32,
                                 thick: f32,
                                 cyc: f64,
                                 k: f64,
                                 shift: f64,
                                 color: Color,
                                 alpha: f32| {
            let top_at = move |x: f32| {
                base * h
                    + amp
                        * h
                        * ((std::f64::consts::TAU * ((x / w) as f64 * cyc + k * phase as f64)
                            + shift)
                            .sin() as f32)
            };
            let ribbon = canvas::Path::new(|b| {
                b.move_to(Point::new(0.0, top_at(0.0)));
                for i in 1..=steps {
                    let x = w * (i as f32 / steps as f32);
                    b.line_to(Point::new(x, top_at(x)));
                }
                for i in (0..=steps).rev() {
                    let x = w * (i as f32 / steps as f32);
                    b.line_to(Point::new(x, top_at(x) + thick * h));
                }
                b.close();
            });
            let span_top = (base - amp) * h;
            let span_bot = (base + amp + thick) * h;
            frame.fill(
                &ribbon,
                canvas::gradient::Linear::new(Point::new(0.0, span_top), Point::new(0.0, span_bot))
                    .add_stop(0.0, Color { a: 0.0, ..color })
                    .add_stop(
                        0.5,
                        Color {
                            a: alpha * breath,
                            ..color
                        },
                    )
                    .add_stop(1.0, Color { a: 0.0, ..color }),
            );
        };
        // amp is kept WELL below thick: the fade gradient spans the static
        // sinusoid envelope while the drawn edges undulate inside it, so an
        // edge sits at gradient offset amp·(1+sin)/(2·amp+thick) off the
        // zero stop — a large amp leaves the cut edge carrying visible
        // alpha (a hard travelling contour line). At amp ≈ thick/10 the
        // worst-case edge alpha is ~1-2/255: an actual soft curtain.
        //
        // Aurora is NIGHT furniture — the day scene (light mode) skips it.
        let day = crate::theme::is_light_mode();
        if !day {
            aurora_ribbon(0.10, 0.012, 0.12, 1.4, 1.0, 0.0, aurora_a, AURORA_ALPHA_A);
            aurora_ribbon(0.17, 0.010, 0.09, 2.1, 2.0, 2.4, aurora_b, AURORA_ALPHA_B);

            // (6) Moonbeam shafts — appended here so gradient block A
            // stays contiguous (zero new buffer splits); everything
            // solid (school, kelp, bubbles, crest, boat) draws over the
            // shafts, so the scene swims THROUGH the light. Each shaft
            // continues the moon→entry ray downward, breathing and
            // swaying on integer rates; the top zero-stop anchors to
            // the deepest possible trough so no lit air ever shows
            // above a passing wave.
            if MOONBEAM_GAIN > 0.0 {
                let beam_top = (1.0 - (SEA_DC - SWELL_AMP - RIPPLE_AMP) as f32) * h;
                let peak_y = SEA_BED_TOP * h;
                let bot_y = 0.92 * h;
                let peak_frac = (peak_y - beam_top) / (bot_y - beam_top);
                for beam in moonbeam_params() {
                    let breath = 0.72
                        + 0.28
                            * (std::f32::consts::TAU
                                * (beam.k_breath as f32 * phase + beam.off_breath))
                                .sin();
                    let sway = 0.006
                        * w
                        * (std::f32::consts::TAU * (beam.k_sway as f32 * phase + beam.off_sway))
                            .sin();
                    let top_cx = (MOON_X + beam.entry_dx) * w + sway;
                    let dx_per_dy = (beam.entry_dx * w) / (beam_top - MOON_Y * h);
                    let dx = (dx_per_dy * (bot_y - beam_top)).clamp(-0.12 * w, 0.12 * w);
                    let bot_cx = top_cx + dx;
                    // Two nested quads split the side-edge step under
                    // the banding floor (the discretized-glow grammar).
                    for (half_top, half_bot, alpha) in [
                        (0.026 * w, 0.045 * w, MOONBEAM_ALPHA_OUTER),
                        (0.0143 * w, 0.02475 * w, MOONBEAM_ALPHA_INNER),
                    ] {
                        let quad = canvas::Path::new(|b| {
                            b.move_to(Point::new(top_cx - half_top, beam_top));
                            b.line_to(Point::new(top_cx + half_top, beam_top));
                            b.line_to(Point::new(bot_cx + half_bot, bot_y));
                            b.line_to(Point::new(bot_cx - half_bot, bot_y));
                            b.close();
                        });
                        frame.fill(
                            &quad,
                            canvas::gradient::Linear::new(
                                Point::new(0.0, beam_top),
                                Point::new(0.0, bot_y),
                            )
                            .add_stop(
                                0.0,
                                Color {
                                    a: 0.0,
                                    ..starlight
                                },
                            )
                            .add_stop(
                                peak_frac,
                                Color {
                                    a: alpha * breath * MOONBEAM_GAIN,
                                    ..starlight
                                },
                            )
                            .add_stop(
                                1.0,
                                Color {
                                    a: 0.0,
                                    ..starlight
                                },
                            ),
                        );
                    }
                }
            }
        }

        // ── Solid block: the sky's inhabitants ──────────────────────────
        // NIGHT: star dots, sparkle crosses (arms deferred to gradient
        // block B where they taper via gradient strokes). DAY: the
        // starlight field would be invisible on a light background, so
        // seagulls glide in its place — each one loops on integer
        // crossings per cycle over an off-panel margin (no edge pop),
        // bobbing and beating its wings at integer rates.
        let glyph_scale = scene_glyph_scale(h);
        let mut deferred_arms: Vec<(Point, f32, f32)> = Vec::new();
        if day {
            // Distant sail — day's shooting star: some cycles a tiny
            // hazed ink sail crosses the back parallax swell, riding the
            // far swell's own heave (phase-coherent for free), always
            // running toward panel center so the full run stays
            // in-panel for BOTH headings. Alpha-zero at both window
            // ends; the later crest ink stroke stays in front, so the
            // strongest depth cue survives.
            if hash01(self.cycle, SAIL_SALT) < SAIL_CHANCE {
                let start = 0.08 + 0.30 * hash01(self.cycle, SAIL_SALT ^ 0x9E37);
                let t = phase - start;
                if (0.0..SAIL_DUR).contains(&t) {
                    let p = t / SAIL_DUR;
                    let env = (std::f32::consts::PI * p).sin();
                    let dir = if hash01(self.cycle, SAIL_SALT.wrapping_add(1)) < 0.5 {
                        -1.0
                    } else {
                        1.0
                    };
                    let span = hash01(self.cycle, SAIL_SALT.wrapping_add(3));
                    let x0 = if dir > 0.0 {
                        0.13 + 0.36 * span
                    } else {
                        0.87 - 0.36 * span
                    };
                    let xf = x0 + dir * 0.28 * p;
                    let x = xf * w;
                    let y = h - (back_swell_height(xf as f64, phase) as f32) * h + 1.0;
                    let s = 7.0 * glyph_scale;
                    let ink = Color {
                        a: SAIL_ALPHA * viz.border_opacity * env,
                        ..crest
                    };
                    frame.stroke(
                        &canvas::Path::line(Point::new(x - 0.8 * s, y), Point::new(x + 0.8 * s, y)),
                        canvas::Stroke::default()
                            .with_color(ink)
                            .with_width(1.6)
                            .with_line_cap(canvas::LineCap::Round),
                    );
                    // Vertical luff on the mast, belly toward travel.
                    let sail = canvas::Path::new(|b| {
                        b.move_to(Point::new(x, y - 0.3 * s));
                        b.line_to(Point::new(x, y - 1.5 * s));
                        b.line_to(Point::new(x + dir * 0.9 * s, y - 0.35 * s));
                        b.close();
                    });
                    frame.fill(&sail, ink);
                }
            }
            let gull_ink = Color {
                a: GULL_ALPHA * viz.border_opacity,
                ..crest
            };
            for gull in gull_params() {
                let dir = if gull.leftward { -1.0 } else { 1.0 };
                let travel = (gull.x0 + dir * gull.k as f32 * phase).rem_euclid(1.0);
                let gx = travel * (w + 2.0 * GULL_MARGIN_PX) - GULL_MARGIN_PX;
                let gy = (gull.y
                    + 0.012
                        * (std::f32::consts::TAU * (gull.bob_k as f32 * phase + gull.bob_off))
                            .sin())
                    * h;
                let flap = 0.42
                    + 0.16 * (std::f32::consts::TAU * (GULL_FLAP_K * phase + gull.flap_off)).sin();
                let s = 7.0 * gull.size * glyph_scale;
                draw_gull(&mut frame, Point::new(gx, gy), s, flap, gull_ink);
            }
        }
        // ── The black hole — the sky's rarest event ─────────────────────
        // INVISIBLE by design (the owner's brief: a black hole isn't
        // really visible) — nothing is drawn for the hole itself. A
        // cycle that rolls one computes (center, s, spin, capture)
        // once; the star loop below routes every glyph through
        // `blackhole_displace` + `blackhole_visibility` (identity /
        // full visibility at s 0 or beyond the capture radius — the
        // boundary + locality guarantees), and the shooting star +
        // wandering notes skip such cycles so the sky carries one
        // drama at a time. Dream cycles pre-empt the hole under the
        // same rule — the moon's ritual owns its sky.
        let dream_cycle = moon_dream_cycle(self.cycle);
        let blackhole_cycle =
            !day && !dream_cycle && hash01(self.cycle, BLACKHOLE_SALT) < BLACKHOLE_CHANCE;
        let blackhole: Option<(Point, f32, f32, f32)> = if blackhole_cycle {
            let start = 0.05 + 0.20 * hash01(self.cycle, BLACKHOLE_SALT ^ 0x9E37);
            let t = phase - start;
            if (0.0..BLACKHOLE_WINDOW).contains(&t) {
                let p = t / BLACKHOLE_WINDOW;
                let (fx, fy) = blackhole_center(self.cycle);
                let hole = Point::new(fx * w, fy * h);
                // Constrained-axis capture radius: identical in the
                // square modes; a dragged-narrow column shrinks the
                // well instead of reaching off-panel.
                let capture = BLACKHOLE_CAPTURE_FRAC * h.min(w);
                let s = blackhole_s(p);
                // The catch's orbital winding: grows from the plunge's
                // end, scaled by s so the spit-out unwinds it into the
                // outward whip and it is exactly zero at both window
                // ends (s is).
                let spin = s.max(0.0) * BLACKHOLE_HOLD_WHIRL * (p - BLACKHOLE_PLUNGE_END).max(0.0);
                Some((hole, s, spin, capture))
            } else {
                None
            }
        } else {
            None
        };

        let night_glyphs = if day { Vec::new() } else { sky_glyphs() };
        for glyph in night_glyphs {
            let twinkle = 1.0
                - glyph.twinkle_depth
                    * (0.5
                        + 0.5
                            * (std::f32::consts::TAU
                                * (glyph.twinkle_k as f32 * phase + glyph.twinkle_off))
                                .sin());
            let home = Point::new(glyph.x * w, glyph.y * h);
            // The hole's gravity, when one is open: identity and full
            // visibility at s 0 and beyond the capture radius, so
            // non-event frames, window boundaries, and the un-captured
            // sky all render the fixed field bit-identically. Nearing
            // the horizon a star's light stops escaping — it shrinks
            // and dims to NOTHING, and re-lights crossing back out on
            // the spit — so the hole is drawn by ABSENCE.
            let (center, vis) = match blackhole {
                Some((hole, s, spin, capture)) => {
                    let pos = blackhole_displace(home, hole, s, spin, capture);
                    let dx = home.x - hole.x;
                    let dy = home.y - hole.y;
                    let grip = blackhole_grip((dx * dx + dy * dy).sqrt(), capture);
                    let dnx = pos.x - hole.x;
                    let dny = pos.y - hole.y;
                    let vis = blackhole_visibility(
                        (dnx * dnx + dny * dny).sqrt(),
                        BLACKHOLE_HORIZON_PX * glyph_scale,
                        s,
                        grip,
                    );
                    (pos, vis)
                }
                None => (home, 1.0),
            };
            if vis <= 0.003 {
                // Fully swallowed — beyond the horizon nothing shines.
                continue;
            }
            let twinkle = twinkle * vis;
            // Swallowed light also LOSES SIZE (the owner's scale-down):
            // glyph geometry shrinks toward nothing alongside the fade.
            let swallow_scale = 0.3 + 0.7 * vis;
            match glyph.kind {
                SkyGlyphKind::Dot => {
                    // Crisp core + concentric halo rings — the visualizer
                    // family's squared-falloff glow discretized to solid
                    // circles (no blur primitive on a canvas). Brightness
                    // correlates with size so the sky gains a magnitude
                    // hierarchy instead of N identical LEDs; the brightest
                    // tier earns a second, wider ring.
                    let r = 1.5 * glyph.size * glyph_scale * swallow_scale;
                    let norm = ((glyph.size - 0.7) / 0.6).clamp(0.0, 1.0);
                    let peak = SKY_STAR_ALPHA * (0.45 + 0.55 * norm);
                    frame.fill(
                        &canvas::Path::circle(center, 1.8 * r),
                        Color {
                            a: 0.30 * peak * twinkle,
                            ..starlight
                        },
                    );
                    if norm > 0.7 {
                        frame.fill(
                            &canvas::Path::circle(center, 2.8 * r),
                            Color {
                                a: 0.10 * peak * twinkle,
                                ..starlight
                            },
                        );
                    }
                    frame.fill(
                        &canvas::Path::circle(center, r),
                        Color {
                            a: peak * twinkle,
                            ..starlight
                        },
                    );
                }
                SkyGlyphKind::Sparkle => {
                    // A lens glint: soft under-glow + bright nucleus here
                    // (solid), arms deferred to gradient block B so they
                    // taper to nothing at the tips.
                    let arm = 3.2 * glyph.size * glyph_scale * swallow_scale;
                    frame.fill(
                        &canvas::Path::circle(
                            center,
                            2.2 * glyph.size * glyph_scale * swallow_scale,
                        ),
                        Color {
                            a: 0.10 * twinkle,
                            ..starlight
                        },
                    );
                    frame.fill(
                        &canvas::Path::circle(
                            center,
                            1.1 * glyph.size * glyph_scale * swallow_scale,
                        ),
                        Color {
                            a: 0.35 * twinkle,
                            ..starlight
                        },
                    );
                    deferred_arms.push((center, arm, SKY_SPARKLE_ALPHA * twinkle));
                }
            }
        }

        // ── Wandering notes ──────────────────────────────────────────────
        // The sky's music glyphs are transient: each cycle a few notes
        // fade in at a CYCLE-HASHED spot, drift gently upward, and fade
        // back out — never twice in the same place. Windows sit fully
        // inside the cycle (max start 0.73 + 0.22 < 1.0), so a window can
        // never straddle the cycle boundary where its hash would change.
        // Black-hole cycles skip the notes: every note window
        // arithmetically overlaps the hole's, and a glyph hovering serene
        // beside a feeding gravity well reads as a bug — notes are not
        // constellation members (they don't get pulled), so they sit the
        // drama out entirely (the shooting star's one-drama rule). Dream
        // cycles skip them too: the verses take the same upper air the
        // notes wander through.
        for i in 0..if blackhole_cycle || dream_cycle {
            0
        } else {
            SKY_WANDER_NOTES
        } {
            let salt = 0x407E + (i as u32) * 4;
            let start = 0.05 + 0.68 * hash01(self.cycle, salt);
            let t = phase - start;
            if !(0.0..SKY_NOTE_DUR).contains(&t) {
                continue;
            }
            let p = t / SKY_NOTE_DUR;
            let fade = (std::f32::consts::PI * p).sin();
            let x = (0.06 + 0.88 * hash01(self.cycle, salt + 1)) * w;
            let y_base = (SKY_BAND_TOP
                + SKY_NOTE_TOP_INSET
                + (SKY_BAND_BOTTOM - SKY_BAND_TOP - SKY_NOTE_TOP_INSET)
                    * hash01(self.cycle, salt + 2))
                * h;
            let y = y_base - 6.0 * glyph_scale * p;
            let s = (7.5 + 2.0 * hash01(self.cycle, salt + 3)) * glyph_scale;
            // Notes glow starlight by night, print in ink by day —
            // starlight on a light background is invisible.
            let color = if day {
                Color {
                    a: SKY_NOTE_ALPHA * fade * viz.border_opacity,
                    ..crest
                }
            } else {
                Color {
                    a: SKY_NOTE_ALPHA * fade,
                    ..starlight
                }
            };
            if i % 2 == 0 {
                draw_note_pair(&mut frame, Point::new(x, y), s, color);
            } else {
                draw_quaver(&mut frame, Point::new(x, y), s, color);
            }
        }

        // ── The moon's dream — verses in the old tongue ──────────────────
        // While the face's marks slip away and return on the Svg layer
        // above (`trawl_scene` keys the moon handle off the same
        // progress), four carved verses take the open right sky one at a
        // time. Starlight by night, ink by day — the wandering notes'
        // swap. The stave height rides the shared glyph scale, capped so
        // the longest line clears both the moon and the right edge on
        // any panel shape; each line center-clamps into the same band.
        if let Some(p) = moon_dream_progress(phase, self.cycle) {
            let longest = harbour_runes::DREAM_VERSES
                .iter()
                .map(|v| harbour_runes::verse_advance(v))
                .fold(0.0f32, f32::max);
            // The verse band vertically overlaps the moon (both live in
            // the upper sky), so the LEFT floor must clear the moon's
            // PIXEL extent — a width-fraction floor alone fails on a
            // dragged-narrow stretched panel where the moon's radius
            // outgrows its width share. The stave cap absorbs the floor
            // so the longest capped line still fits the [floor, 0.95 w]
            // band on every shape.
            let left_floor = (MOON_X * w + MOON_RADIUS_PX * glyph_scale + 6.0).max(0.24 * w);
            let stave = (MOON_DREAM_STAVE_PX * glyph_scale)
                .min((0.95 * w - left_floor).max(0.0) / longest.max(f32::EPSILON));
            for (line, verse) in harbour_runes::DREAM_VERSES.iter().enumerate() {
                let fade = moon_dream_verse_alpha(p, line);
                if fade <= 0.0 {
                    continue;
                }
                let width = harbour_runes::verse_advance(verse) * stave;
                let x0 = (MOON_DREAM_VERSE_CX * w - 0.5 * width)
                    .clamp(left_floor, (0.95 * w - width).max(left_floor));
                let y0 = MOON_DREAM_VERSE_Y * h;
                let color = if day {
                    Color {
                        a: MOON_DREAM_VERSE_ALPHA * fade * viz.border_opacity,
                        ..crest
                    }
                } else {
                    Color {
                        a: MOON_DREAM_VERSE_ALPHA * fade,
                        ..starlight
                    }
                };
                let staves = canvas::Path::new(|b| {
                    let mut pen = x0;
                    for c in verse.chars() {
                        if c == ' ' {
                            pen += harbour_runes::RUNE_WORD_SPACE * stave;
                            continue;
                        }
                        let Some(glyph) = harbour_runes::rune_glyph(c) else {
                            continue;
                        };
                        let lb = harbour_runes::left_bearing(glyph) * stave;
                        for seg in glyph.segments {
                            b.move_to(Point::new(pen + lb + seg[0] * stave, y0 + seg[1] * stave));
                            b.line_to(Point::new(pen + lb + seg[2] * stave, y0 + seg[3] * stave));
                        }
                        pen += glyph.width * stave;
                    }
                });
                frame.stroke(
                    &staves,
                    canvas::Stroke::default()
                        .with_color(color)
                        .with_width((0.10 * stave).max(0.7))
                        .with_line_cap(canvas::LineCap::Round),
                );
            }
        }

        // ── The moon's glow / the sun's vexel fan ────────────────────────
        // The face itself is the owner's avatar, rendered as a themed Svg
        // layer in `trawl_scene` (a canvas can't draw SVGs) at the shared
        // MOON_X/MOON_Y/MOON_RADIUS_PX consts. The canvas draws the light
        // AROUND it — a discretized radial glow whose exposed steps sit
        // under the visibility floor (see the glow-stack const docs).
        // NIGHT: the starlight stack with a cascaded breath (one swell
        // rolling from the face outward), plus a rare cycle-hashed exhale
        // pulse. DAY: the gold stack breathing uniformly, under a vexel
        // fan of 12 filled, bellied, tapered wedges — 6 major + 6 minor —
        // whose tips and brightness ride a travelling wave circulating
        // AGAINST the TAU/6 spin. Nothing structured sits under the face:
        // the avatar is 0.60-opaque, so wedges start OUTSIDE it while the
        // bright innermost glow steps hide beneath it.
        if MOON_ALPHA > 0.0 {
            let m = MOON_RADIUS_PX * glyph_scale;
            let mc = Point::new(MOON_X * w, MOON_Y * h);
            let moon_breath = 0.9 + 0.1 * (std::f32::consts::TAU * phase).sin();
            if day {
                let gold = crate::theme::logo_wood();
                // Core glow: uniform k=1 breath across the stack. Gold is
                // the scene's warm LIGHT — never dialed by border_opacity
                // (that knob scales ink legibility, not light).
                draw_glow_stack(&mut frame, mc, m, &SUN_GLOW_STACK, gold, |_| moon_breath);
                // The vexel fan: filled tapered wedges whose sides AIM at
                // the center (the reference's center-apex silhouette) but
                // start at SUN_WEDGE_INNER, outside the translucent face.
                // Both edges bow OUTWARD symmetrically (the static half of
                // "wave like" — a plump curved ray, no pinwheel handedness).
                for i in 0..SUN_RAY_COUNT {
                    let (theta, r_out_m, alpha) = sun_ray_geometry(i, phase);
                    let (sin_t, cos_t) = theta.sin_cos();
                    let (tan_t, belly) = if sun_ray_is_major(i) {
                        (SUN_WEDGE_TAN_MAJOR, SUN_WEDGE_BELLY_MAJOR)
                    } else {
                        (SUN_WEDGE_TAN_MINOR, SUN_WEDGE_BELLY_MINOR)
                    };
                    let r_in = SUN_WEDGE_INNER * m;
                    let r_out = r_out_m * m;
                    let rm = (r_in + r_out) * 0.5;
                    let at = |r: f32, side: f32, extra: f32| {
                        Point::new(
                            mc.x + cos_t * r - sin_t * side * (r * tan_t + extra),
                            mc.y + sin_t * r + cos_t * side * (r * tan_t + extra),
                        )
                    };
                    let wedge = canvas::Path::new(|b| {
                        b.move_to(at(r_in, -1.0, 0.0));
                        b.quadratic_curve_to(at(rm, -1.0, belly * m), at(r_out, -1.0, 0.0));
                        b.line_to(at(r_out, 1.0, 0.0));
                        b.quadratic_curve_to(at(rm, 1.0, belly * m), at(r_in, 1.0, 0.0));
                        b.close();
                    });
                    frame.fill(&wedge, Color { a: alpha, ..gold });
                }
            } else {
                // Cascaded breath: rank counted from the INNERMOST entry,
                // larger rank = larger lag = peaks later — the swell is
                // born at the face and rolls outward (~11 s to cross).
                // Each ring is a k=1 integer-rate sine with a constant
                // offset, so phase 0 and phase 1 render identically.
                let last = MOON_GLOW_STACK.len() - 1;
                draw_glow_stack(&mut frame, mc, m, &MOON_GLOW_STACK, starlight, |i| {
                    let rank = (last - i) as f32;
                    (1.0 - MOON_WASH_DEPTH)
                        + MOON_WASH_DEPTH
                            * (0.5
                                + 0.5
                                    * (std::f32::consts::TAU * (phase - rank * MOON_WASH_LAG))
                                        .sin())
                });
                // The exhale: some cycles a soft two-stroke ring detaches
                // at the halo's shoulder, expands past the rim, and
                // dissolves — wide faint stroke under a narrow brighter
                // one reads as one soft band, not a crisp vector circle.
                if hash01(self.cycle, MOON_PULSE_SALT) < MOON_PULSE_CHANCE {
                    let start = 0.15 + 0.45 * hash01(self.cycle, MOON_PULSE_SALT ^ 0x9E37);
                    let t = phase - start;
                    if (0.0..MOON_PULSE_DUR).contains(&t) {
                        let p = t / MOON_PULSE_DUR;
                        let env = (std::f32::consts::PI * p).sin();
                        let r = m * (1.20 + 1.00 * p);
                        for (width, alpha) in [(0.42 * m, 0.016), (0.18 * m, 0.045)] {
                            frame.stroke(
                                &canvas::Path::circle(mc, r),
                                canvas::Stroke::default()
                                    .with_color(Color {
                                        a: alpha * env,
                                        ..starlight
                                    })
                                    .with_width(width),
                            );
                        }
                    }
                }
            }
        }

        // ── Rising notes — the longship sings ───────────────────────────
        // A small pool of note glyphs climbs from the mast, swaying as
        // they rise, fading in at birth and out near the top. Each rider
        // loops on an integer multiple of the phase; alpha hits zero at
        // both ends of its run, so the cycle wrap (a position jump) never
        // shows. Anchored to the live hull x, and DIMMED by edge proximity
        // (boat_edge_fade) so the song fades out with the departing sprite
        // instead of cutting in one frame at the panel edge while the hull
        // is still half on-screen.
        let boat_edge_fade = (self.boat_x.min(1.0 - self.boat_x) / BOAT_EDGE_FADE).clamp(0.0, 1.0);
        if boat_edge_fade > 0.0 {
            let boat_cx = self.boat_x * w;
            let start_y = front_y(boat_cx) - 0.10 * h;
            for rider in riser_params() {
                let t = (rider.k as f32 * phase + rider.off).fract();
                let fade_in = (t / RISER_FADE_IN).min(1.0);
                let fade_out = ((1.0 - t) / RISER_FADE_OUT).min(1.0);
                let alpha = RISER_ALPHA * fade_in * fade_out * boat_edge_fade;
                if alpha <= 0.01 {
                    continue;
                }
                let sway = RISER_SWAY_PX
                    * glyph_scale
                    * (std::f32::consts::TAU * (2.0 * t + rider.sway_off)).sin();
                let x = boat_cx + rider.dx * glyph_scale + sway;
                let y = start_y - t * RISER_RISE_FRAC * h;
                let s = (7.0 + 4.0 * t) * glyph_scale;
                // Starlight song by night, ink by day (see wandering notes).
                let color = if day {
                    Color {
                        a: alpha * viz.border_opacity,
                        ..crest
                    }
                } else {
                    Color {
                        a: alpha,
                        ..starlight
                    }
                };
                if rider.beamed {
                    draw_note_pair(&mut frame, Point::new(x, y), s, color);
                } else {
                    draw_quaver(&mut frame, Point::new(x, y), s, color);
                }
            }
        }

        // ── Leaping fish — the trawl stirs one up ───────────────────────
        // Some cycles, a small ink silhouette arcs out of the water at a
        // cycle-hashed spot and dives back. Rotated along its flight
        // tangent via the frame transform stack; alpha eases in and out
        // over the hop so it surfaces and re-enters softly.
        let fish_t = (phase + FISH_OFF).rem_euclid(1.0);
        if hash01(self.cycle, 0xF1_5E) < FISH_CHANCE && fish_t < FISH_WINDOW {
            let p = fish_t / FISH_WINDOW;
            let fx = (0.15 + 0.70 * hash01(self.cycle, 0xF1_5F)) * w;
            let arc_w = 0.06 * w;
            let jump_h = FISH_JUMP_FRAC * h;
            let x = fx + (p - 0.5) * arc_w;
            let y = front_y(fx) + 2.0 - jump_h * 4.0 * p * (1.0 - p);
            // Flight tangent: d/dp of (x, y) — horizontal speed is
            // constant, vertical follows the parabola.
            let angle = (-(jump_h * 4.0 * (1.0 - 2.0 * p))).atan2(arc_w);
            let fade = (std::f32::consts::PI * p).sin();
            let l = FISH_SIZE * glyph_scale;
            let fish_color = Color {
                a: FISH_ALPHA * viz.border_opacity * fade,
                ..crest
            };
            frame.with_save(|frame| {
                frame.translate(iced::Vector::new(x, y));
                frame.rotate(angle);
                // Body: a little teardrop; tail: a notched triangle —
                // the shared silhouette, nose along the rotated +x.
                fill_fish_silhouette(frame, l, 1.0, fish_color);
            });
        }

        // ── The seabed — rocks, starfish, kelp beds, bubbles ────────────
        // The trawl's floor, alive without a metaphor to decode: static
        // furniture (rock mounds, one resting starfish) grounds the
        // bottom, kelp beds sway over it, and the dragged anchor aerates
        // the bed with a stream of rising bubbles. All cold (starlight /
        // seafoam night, ink day); the lantern keeps the one warm note.
        //
        // Bed dressing first — it lies UNDER the kelp roots.
        let dressing = bed_dressing();
        for rock in &dressing.rocks {
            let rw = 9.0 * rock.w * glyph_scale;
            let rh = 4.5 * rock.ht * glyph_scale;
            let base = Point::new(rock.x * w, 0.982 * h);
            let dome = canvas::Path::new(|b| {
                b.move_to(Point::new(base.x - rw, base.y));
                b.quadratic_curve_to(
                    Point::new(base.x, base.y - 2.0 * rh),
                    Point::new(base.x + rw, base.y),
                );
                b.close();
            });
            frame.fill(
                &dome,
                Color {
                    a: BED_INK_ALPHA * viz.border_opacity,
                    ..crest
                },
            );
            if !day {
                // Moonlit top: ink mounds vanish on the night bed, so a
                // faint starlight rim carries the silhouette (the
                // school's catch-rim lesson).
                let rim = canvas::Path::new(|b| {
                    b.move_to(Point::new(base.x - 0.8 * rw, base.y - 0.35 * rh));
                    b.quadratic_curve_to(
                        Point::new(base.x, base.y - 2.0 * rh),
                        Point::new(base.x + 0.8 * rw, base.y - 0.35 * rh),
                    );
                });
                frame.stroke(
                    &rim,
                    canvas::Stroke::default()
                        .with_color(Color {
                            a: BED_RIM_ALPHA,
                            ..starlight
                        })
                        .with_width(1.0)
                        .with_line_cap(canvas::LineCap::Round),
                );
            }
        }
        let star_center = Point::new(dressing.star_x * w, 0.972 * h);
        let star_arm = STARFISH_ARM_PX * dressing.star_size * glyph_scale;
        fill_starfish(
            &mut frame,
            star_center,
            star_arm,
            dressing.star_rot,
            Color {
                a: BED_INK_ALPHA * viz.border_opacity,
                ..crest
            },
        );
        if !day {
            // The same moonlit treatment: a dim seafoam overprint lifts
            // the creature off the night bed without lighting it up.
            fill_starfish(
                &mut frame,
                star_center,
                star_arm,
                dressing.star_rot,
                Color {
                    a: BED_RIM_ALPHA,
                    ..water_far
                },
            );
        }

        // The sunken crate — grounds like a rock (kelp sways in front of
        // it, the anchor sprite drags over it on the layer above), so it
        // draws with the rest of the bed dressing.
        {
            let s = CRATE_SIZE_PX * glyph_scale;
            let crate_ink = Color {
                a: BED_INK_ALPHA * viz.border_opacity,
                ..crest
            };
            frame.with_save(|frame| {
                // Center sits low enough that the tilted bottom corners
                // dip a few px below the base line — the half-buried
                // read, same trick as the rock bases.
                frame.translate(iced::Vector::new(CRATE_X * w, 0.988 * h - 0.42 * s));
                frame.rotate(CRATE_TILT_DEG.to_radians());
                let face = canvas::Path::new(|b| {
                    b.move_to(Point::new(-0.5 * s, -0.5 * s));
                    b.line_to(Point::new(0.5 * s, -0.5 * s));
                    b.line_to(Point::new(0.5 * s, 0.5 * s));
                    b.line_to(Point::new(-0.5 * s, 0.5 * s));
                    b.close();
                });
                frame.fill(&face, crate_ink);
                // Frame stroke at the SAME alpha as the fill — the bed
                // ink ceiling; the outline still reads via double
                // coverage.
                frame.stroke(
                    &face,
                    canvas::Stroke::default()
                        .with_color(crate_ink)
                        .with_width(1.2),
                );
                // Slats: two horizontal boards across the face.
                let slats = canvas::Path::new(|b| {
                    b.move_to(Point::new(-0.5 * s, -s / 6.0));
                    b.line_to(Point::new(0.5 * s, -s / 6.0));
                    b.move_to(Point::new(-0.5 * s, s / 6.0));
                    b.line_to(Point::new(0.5 * s, s / 6.0));
                });
                frame.stroke(
                    &slats,
                    canvas::Stroke::default()
                        .with_color(Color {
                            a: CRATE_SLAT_ALPHA * viz.border_opacity,
                            ..crest
                        })
                        .with_width(1.0),
                );
                if !day {
                    // Moonlit rim on the up-facing edges: the STRAIGHT
                    // lit lines are what say crate-not-rock on the dark
                    // bed (the rocks' rim lesson, squared off).
                    let rim = canvas::Path::new(|b| {
                        b.move_to(Point::new(-0.5 * s, -0.5 * s));
                        b.line_to(Point::new(0.5 * s, -0.5 * s));
                        b.move_to(Point::new(-0.5 * s, -0.5 * s));
                        b.line_to(Point::new(-0.5 * s, -0.15 * s));
                    });
                    frame.stroke(
                        &rim,
                        canvas::Stroke::default()
                            .with_color(Color {
                                a: CRATE_RIM_ALPHA,
                                ..starlight
                            })
                            .with_width(1.0)
                            .with_line_cap(canvas::LineCap::Round),
                    );
                }
            });
        }

        let kelp_color = if day {
            Color {
                a: KELP_ALPHA_DAY * viz.border_opacity,
                ..crest
            }
        } else {
            Color {
                a: KELP_ALPHA_NIGHT,
                ..water_far
            }
        };
        for kelp in kelp_params() {
            let root = Point::new(kelp.x * w, 0.985 * h);
            let height = kelp.height * h;
            let sway = (std::f32::consts::TAU * (kelp.sway_k as f32 * phase + kelp.sway_off)).sin();
            // Spine: root → tip, bending progressively (f^1.7) so the
            // base stays planted while the tip travels.
            let spine = |f: f32| {
                let bend = f.powf(1.7);
                Point::new(
                    root.x + (kelp.lean + KELP_SWAY_PX * sway) * glyph_scale * bend,
                    root.y - height * f,
                )
            };
            // Three tapering width tiers over the frond's thirds.
            for (tier, width) in [2.6_f32, 1.8, 1.0].into_iter().enumerate() {
                let f0 = tier as f32 / 3.0;
                let f1 = (tier as f32 + 1.0) / 3.0;
                let seg = canvas::Path::new(|b| {
                    b.move_to(spine(f0));
                    b.line_to(spine((f0 + f1) * 0.5));
                    b.line_to(spine(f1));
                });
                frame.stroke(
                    &seg,
                    canvas::Stroke::default()
                        .with_color(kelp_color)
                        .with_width(width * glyph_scale)
                        .with_line_cap(canvas::LineCap::Round),
                );
            }
        }

        // Kelp-root seeps: one slow bubble per frond, rising on its own
        // integer rate — the beds breathe even when the anchor is far.
        // Alpha zero at both ends of each run (the riser contract).
        for kelp in kelp_params() {
            let t = (kelp.seep_k as f32 * phase + kelp.seep_off).fract();
            let fade = ((t / 0.20).min(1.0)) * (((1.0 - t) / 0.30).min(1.0));
            if fade <= 0.01 {
                continue;
            }
            let x = kelp.x * w
                + 1.5 * glyph_scale * (std::f32::consts::TAU * (2.0 * t + kelp.sway_off)).sin();
            let y = 0.975 * h - t * SEEP_RISE_FRAC * h;
            let color = if day {
                Color {
                    a: SEEP_ALPHA * fade * viz.border_opacity,
                    ..crest
                }
            } else {
                Color {
                    a: SEEP_ALPHA * fade,
                    ..starlight
                }
            };
            frame.fill(&canvas::Path::circle(Point::new(x, y), glyph_scale), color);
        }

        // Bubbles — the drag aerates the bed: a sparse stream climbs
        // from the trawled anchor, swaying as it rises. Each rider loops
        // on an integer multiple of the phase with alpha zero at both
        // ends, and the whole stream dims by the ANCHOR's edge proximity
        // (the risers' rule) so it departs with the sprite instead of
        // cutting at the panel edge — and the wrap seam, where the
        // anchor teleports margins, can't pop a mid-flight bubble.
        let anchor_fade = (self.anchor_x.min(1.0 - self.anchor_x) / BOAT_EDGE_FADE).clamp(0.0, 1.0);
        if anchor_fade > 0.0 {
            let base_x = self.anchor_x * w;
            for bubble in bubble_params() {
                let t = (bubble.k as f32 * phase + bubble.off).fract();
                let fade_in = (t / BUBBLE_FADE_IN).min(1.0);
                let fade_out = ((1.0 - t) / BUBBLE_FADE_OUT).min(1.0);
                let alpha = BUBBLE_ALPHA * fade_in * fade_out * anchor_fade;
                if alpha <= 0.01 {
                    continue;
                }
                let sway = BUBBLE_SWAY_PX
                    * glyph_scale
                    * (std::f32::consts::TAU * (2.0 * t + bubble.sway_off)).sin();
                let x = base_x + bubble.dx * glyph_scale + sway;
                // Grow slightly as they rise (decompression) — a small
                // touch that reads "bubble", not "spark".
                let r = (1.0 + 0.6 * t) * bubble.size * glyph_scale * 1.4;
                let y = 0.955 * h - t * BUBBLE_RISE_FRAC * h;
                let color = if day {
                    Color {
                        a: alpha * viz.border_opacity,
                        ..crest
                    }
                } else {
                    Color {
                        a: alpha,
                        ..starlight
                    }
                };
                if bubble.ring {
                    frame.stroke(
                        &canvas::Path::circle(Point::new(x, y), r),
                        canvas::Stroke::default().with_color(color).with_width(0.8),
                    );
                } else {
                    frame.fill(&canvas::Path::circle(Point::new(x, y), 0.7 * r), color);
                }
            }
        }

        // The Deep Passage — Jörmungandr's rare glide through the deep
        // lane (y 0.78–0.855h: below the school band, above the bubble
        // source and every bed silhouette; mid-kelp tips stop ~0.875h).
        // Drawn BEFORE the school so the school glides in front — depth.
        if hash01(self.cycle, 0xDEE9) < SERPENT_CHANCE {
            let start = 0.06 + 0.72 * hash01(self.cycle, 0xDEEA);
            let t = phase - start;
            if (0.0..SERPENT_WINDOW).contains(&t) {
                let p = t / SERPENT_WINDOW;
                let env = (std::f32::consts::PI * p).sin();
                let dir = if hash01(self.cycle, 0xDEEC) < 0.5 {
                    -1.0_f32
                } else {
                    1.0
                };
                let y0 = (SERPENT_LANE_TOP + SERPENT_LANE_SPAN * hash01(self.cycle, 0xDEEB)) * h;
                let gs = glyph_scale;
                let l = 80.0 * gs;
                // Head traverses 0.62w centered on the panel.
                let hx = w * (0.5 + dir * (p - 0.5) * 0.62);
                // Spine: amplitude tapers TOWARD the head (the head runs
                // steady, the tail whips); 3·p = exactly three tail-beats
                // per appearance (windowed-event precedent — wrap-safety
                // is moot under the zero-end envelope).
                let spine: Vec<Point> = (0..=12)
                    .map(|i| {
                        let u = i as f32 / 12.0;
                        Point::new(
                            hx - dir * u * l,
                            y0 + 4.5
                                * gs
                                * (0.4 + 0.6 * u)
                                * (std::f32::consts::TAU * (2.0 * u + 3.0 * p)).sin(),
                        )
                    })
                    .collect();
                let ink = Color {
                    a: SERPENT_ALPHA * viz.border_opacity * env,
                    ..crest
                };
                // Body — the kelp width-tier trick: three stroked
                // polylines over the spine thirds with SHARED endpoints,
                // widths tapering toward the tail (no fill mesh).
                for (range, width) in [(0..=4_usize, 3.2_f32), (4..=8, 2.2), (8..=12, 1.2)] {
                    let seg = canvas::Path::new(|b| {
                        let mut first = true;
                        for i in range.clone() {
                            if first {
                                b.move_to(spine[i]);
                                first = false;
                            } else {
                                b.line_to(spine[i]);
                            }
                        }
                    });
                    frame.stroke(
                        &seg,
                        canvas::Stroke::default()
                            .with_color(ink)
                            .with_width(width * gs)
                            .with_line_cap(canvas::LineCap::Round),
                    );
                }
                // Head: filled wedge — nose forward of the first spine
                // point. No dorsal spikes (clutter at this scale).
                let head = canvas::Path::new(|b| {
                    b.move_to(Point::new(hx + dir * 6.0 * gs, spine[0].y));
                    b.line_to(Point::new(spine[0].x, spine[0].y - 2.6 * gs));
                    b.line_to(Point::new(spine[0].x, spine[0].y + 2.6 * gs));
                    b.close();
                });
                frame.fill(&head, ink);
                if !day {
                    // Dorsal catch-rim: moonlight along the back carries
                    // the silhouette through the bed vignette's darkening
                    // — the school's rim lesson at scale.
                    let rim = canvas::Path::new(|b| {
                        let mut first = true;
                        for pt in spine.iter().take(10).skip(1) {
                            let above = Point::new(pt.x, pt.y - 2.0 * gs);
                            if first {
                                b.move_to(above);
                                first = false;
                            } else {
                                b.line_to(above);
                            }
                        }
                    });
                    frame.stroke(
                        &rim,
                        canvas::Stroke::default()
                            .with_color(Color {
                                a: SERPENT_RIM_ALPHA * env,
                                ..starlight
                            })
                            .with_width(0.9)
                            .with_line_cap(canvas::LineCap::Round),
                    );
                }
            }
        }

        // Drifting school — mid-water gliders on the gull idiom, under
        // the crest so the waterline still draws over them.
        for fish in school_params() {
            let dir = if fish.leftward { -1.0_f32 } else { 1.0 };
            let travel = (fish.x0 + dir * fish.k as f32 * phase).rem_euclid(1.0);
            let fx = travel * (w + 2.0 * SCHOOL_MARGIN_PX) - SCHOOL_MARGIN_PX;
            let fy = (fish.y
                + 0.008
                    * (std::f32::consts::TAU * (fish.bob_k as f32 * phase + fish.bob_off)).sin())
                * h;
            let l = 11.0 * fish.size * glyph_scale;
            let ink = Color {
                a: SCHOOL_ALPHA * viz.border_opacity,
                ..crest
            };
            frame.with_save(|frame| {
                frame.translate(iced::Vector::new(fx, fy));
                fill_fish_silhouette(frame, l, dir, ink);
                if !day {
                    // Starlight catch-rim along the back — moonlight
                    // through water, or the school drowns in the deep.
                    let rim = canvas::Path::new(|b| {
                        b.move_to(Point::new(dir * -0.42 * l, -0.10 * l));
                        b.quadratic_curve_to(
                            Point::new(dir * -0.05 * l, -0.33 * l),
                            Point::new(dir * 0.38 * l, -0.05 * l),
                        );
                    });
                    frame.stroke(
                        &rim,
                        canvas::Stroke::default()
                            .with_color(Color {
                                a: SCHOOL_RIM_ALPHA,
                                ..starlight
                            })
                            .with_width(0.9)
                            .with_line_cap(canvas::LineCap::Round),
                    );
                }
            });
        }

        // ── The moonlit crest — the line the boat rides ─────────────────
        // Three passes replace the old single 1.5px ink stroke (which
        // measured ~2/255 of visible effect): a soft starlight halo, then
        // committed sprite-weight ink (the boat's own outline language and
        // the light-mode legibility mechanism), then a bright catch-light
        // that fades into the panel edges. Crisp-core-plus-halo is the
        // visualizer family's glow grammar at minimum viable form.
        let crest_path = canvas::Path::new(|b| {
            b.move_to(Point::new(0.0, front_y(0.0)));
            for i in 1..=steps {
                let x = w * (i as f32 / steps as f32);
                b.line_to(Point::new(x, front_y(x)));
            }
        });
        // The crest light passes render in the MODE's light: starlight
        // by night, sun gold by day (starlight-on-light measured ~0 —
        // day's waterline was a bare ink line until the gilding).
        let crest_light = if day {
            crate::theme::logo_wood()
        } else {
            starlight
        };
        let crest_light_gain = if day { CREST_LIGHT_GAIN_DAY } else { 1.0 };
        // Halo (solid, deliberately: gradient edge-stubs at 0.05 alpha are
        // ~1/255 — invisible — and a second heavy gradient stroke of this
        // long polyline isn't worth the buffer). Night-only: a wide gold
        // halo reads muddy on a light sea, so day skips it rather than
        // recoloring.
        if !day {
            frame.stroke(
                &crest_path,
                canvas::Stroke::default()
                    .with_color(Color {
                        a: CREST_HALO_ALPHA,
                        ..starlight
                    })
                    .with_width(4.0)
                    .with_line_cap(canvas::LineCap::Round),
            );
        }
        // Ink.
        frame.stroke(
            &crest_path,
            canvas::Stroke::default()
                .with_color(Color {
                    a: CREST_INK_ALPHA * viz.border_opacity,
                    ..crest
                })
                .with_width(2.0)
                .with_line_cap(canvas::LineCap::Round),
        );

        // Sun glitter (day only): sparse gold dashes riding the front
        // waterline, each flashing on its own integer rate through a ^4
        // profile — spiky glitter statistics, never a blink field. The
        // height gate favors passing crests (the sweep's grammar); the
        // hull occludes dashes it crosses for free (the ship Svg layers
        // above the canvas).
        if day && GLITTER_ALPHA > 0.0 {
            let gold = crate::theme::logo_wood();
            for gp in glitter_params() {
                let tw = 0.5 + 0.5 * (std::f32::consts::TAU * (gp.k as f32 * phase + gp.off)).sin();
                let bright = tw * tw * tw * tw;
                let gate = 0.4
                    + 0.6
                        * ((sample_line_height(bars, gp.x, false) - SEA_DC as f32)
                            / (SWELL_AMP + RIPPLE_AMP) as f32)
                            .clamp(0.0, 1.0);
                let a = GLITTER_ALPHA * bright * gate;
                if a <= 0.01 {
                    continue;
                }
                let x = gp.x * w;
                let y = front_y(x) + gp.depth_px * glyph_scale;
                let half = 0.5 * gp.len * glyph_scale;
                frame.stroke(
                    &canvas::Path::line(Point::new(x - half, y), Point::new(x + half, y)),
                    canvas::Stroke::default()
                        .with_color(Color { a, ..gold })
                        .with_width(1.2)
                        .with_line_cap(canvas::LineCap::Round),
                );
            }
        }

        // ── Gradient block B: the catch-light ──────────────────────────
        // A struct literal, NOT with_color (which clobbers the style back
        // to Solid): a 1px lit line breathing into the edges instead of
        // hitting them.
        let catch_alpha = CREST_LIGHT_ALPHA * crest_light_gain;
        let catch =
            canvas::gradient::Linear::new(Point::new(0.0, surface_y), Point::new(w, surface_y))
                .add_stop(
                    0.0,
                    Color {
                        a: 0.0,
                        ..crest_light
                    },
                )
                .add_stop(
                    CREST_LIGHT_EDGE,
                    Color {
                        a: catch_alpha,
                        ..crest_light
                    },
                )
                .add_stop(
                    1.0 - CREST_LIGHT_EDGE,
                    Color {
                        a: catch_alpha,
                        ..crest_light
                    },
                )
                .add_stop(
                    1.0,
                    Color {
                        a: 0.0,
                        ..crest_light
                    },
                );
        frame.stroke(
            &crest_path,
            canvas::Stroke {
                style: canvas::Style::Gradient(canvas::Gradient::Linear(catch)),
                width: 1.0,
                line_cap: canvas::LineCap::Round,
                ..canvas::Stroke::default()
            },
        );

        // Crest shimmer sweep: for 30% of each cycle a band of light
        // glides along the crest (entering and exiting off-panel, so no
        // edge pop), brightest where the wave actually peaks — the height
        // gate derives from the same field the boat rides.
        let sweep_t = (phase + CREST_SWEEP_OFF).rem_euclid(1.0);
        if sweep_t < CREST_SWEEP_FRACTION {
            let c = -CREST_SWEEP_HALF_WIDTH
                + (sweep_t / CREST_SWEEP_FRACTION) * (1.0 + 2.0 * CREST_SWEEP_HALF_WIDTH);
            let gate = ((sample_line_height(bars, c.clamp(0.0, 1.0), false) - SEA_DC as f32)
                / (SWELL_AMP + RIPPLE_AMP) as f32)
                .clamp(0.0, 1.0);
            let peak = CREST_SHIMMER_ALPHA * gate * crest_light_gain;
            if peak > 0.005 {
                let mut sweep = canvas::gradient::Linear::new(
                    Point::new(0.0, surface_y),
                    Point::new(w, surface_y),
                );
                for (offset, alpha) in sweep_stops(c, CREST_SWEEP_HALF_WIDTH, peak) {
                    sweep = sweep.add_stop(
                        offset,
                        Color {
                            a: alpha,
                            ..crest_light
                        },
                    );
                }
                frame.stroke(
                    &crest_path,
                    canvas::Stroke {
                        style: canvas::Style::Gradient(canvas::Gradient::Linear(sweep)),
                        width: 3.0,
                        line_cap: canvas::LineCap::Round,
                        ..canvas::Stroke::default()
                    },
                );
            }
        }

        // Lantern glint: the boat pools warm light on the water it rides —
        // the scene's one warm note, answering the sprite's gold trim with
        // the logo's own mode-stable accessor. Breathes on an integer-rate
        // ~5 s cycle; dims by edge proximity so the pool departs with the
        // sprite instead of cutting at the panel edge.
        if boat_edge_fade > 0.0 {
            let gold = crate::theme::logo_wood();
            let cx = self.boat_x * w;
            let cy = front_y(cx) + 1.5;
            let r_w = 0.5 * crate::widgets::boat::boat_pixel_size(w.min(h)).0;
            let glint_breath = (0.85
                + 0.15 * (std::f32::consts::TAU * GLINT_BREATH_K * phase).sin())
                * boat_edge_fade;
            // Core pool.
            frame.fill_rectangle(
                Point::new(cx - r_w, cy - 1.5),
                Size::new(2.0 * r_w, 3.0),
                canvas::gradient::Linear::new(Point::new(cx - r_w, cy), Point::new(cx + r_w, cy))
                    .add_stop(0.0, Color { a: 0.0, ..gold })
                    .add_stop(
                        0.5,
                        Color {
                            a: GLINT_ALPHA * glint_breath,
                            ..gold
                        },
                    )
                    .add_stop(1.0, Color { a: 0.0, ..gold }),
            );
            // Wider faint spread.
            frame.fill_rectangle(
                Point::new(cx - 1.8 * r_w, cy - 2.5),
                Size::new(3.6 * r_w, 5.0),
                canvas::gradient::Linear::new(
                    Point::new(cx - 1.8 * r_w, cy),
                    Point::new(cx + 1.8 * r_w, cy),
                )
                .add_stop(0.0, Color { a: 0.0, ..gold })
                .add_stop(
                    0.5,
                    Color {
                        a: 0.04 * glint_breath,
                        ..gold
                    },
                )
                .add_stop(1.0, Color { a: 0.0, ..gold }),
            );
            // A short fading smear sinking below the waterline.
            let smear = canvas::Path::new(|b| {
                b.move_to(Point::new(cx, cy));
                b.line_to(Point::new(cx, cy + 0.08 * h));
            });
            frame.stroke(
                &smear,
                canvas::Stroke {
                    style: canvas::Style::Gradient(canvas::Gradient::Linear(
                        canvas::gradient::Linear::new(
                            Point::new(cx, cy),
                            Point::new(cx, cy + 0.08 * h),
                        )
                        .add_stop(
                            0.0,
                            Color {
                                a: 0.07 * glint_breath,
                                ..gold
                            },
                        )
                        .add_stop(1.0, Color { a: 0.0, ..gold }),
                    )),
                    width: 2.0,
                    line_cap: canvas::LineCap::Round,
                    ..canvas::Stroke::default()
                },
            );
        }

        // Shooting star: some cycles carry one streak across the upper
        // sky, its timing, origin, and heading all hashed from the cycle
        // counter — no two cycles replay the same streak. Night only (a
        // meteor at noon reads as a rendering bug, and the starlight
        // streak would be invisible anyway). Black-hole and dream cycles
        // skip it — one sky drama at a time.
        if !day && !blackhole_cycle && !dream_cycle && hash01(self.cycle, 0x57A2) < SHOOT_CHANCE {
            let start = 0.15 + 0.60 * hash01(self.cycle, 0x57A3);
            if phase >= start && phase < start + SHOOT_WINDOW {
                let p = (phase - start) / SHOOT_WINDOW;
                let x0 = (0.25 + 0.60 * hash01(self.cycle, 0x57A4)) * w;
                let y0 = (0.05 + 0.12 * hash01(self.cycle, 0x57A5)) * h;
                let theta = (20.0 + 15.0 * hash01(self.cycle, 0x57A6)).to_radians();
                let dir = iced::Vector::new(-theta.cos(), theta.sin());
                // Height-scaled: max head depth = y0 + sin(35°)·0.35h ≈
                // 0.37h, safely above the back swell's highest crest.
                let travel = p * SHOOT_TRAVEL_FRAC * h;
                let head = Point::new(x0 + dir.x * travel, y0 + dir.y * travel);
                let len = SHOOT_LEN_FRAC * h;
                let tail = Point::new(head.x - dir.x * len, head.y - dir.y * len);
                let fade = (std::f32::consts::PI * p).sin();
                let streak = canvas::Path::line(tail, head);
                frame.stroke(
                    &streak,
                    canvas::Stroke {
                        style: canvas::Style::Gradient(canvas::Gradient::Linear(
                            canvas::gradient::Linear::new(tail, head)
                                .add_stop(
                                    0.0,
                                    Color {
                                        a: 0.0,
                                        ..starlight
                                    },
                                )
                                .add_stop(
                                    1.0,
                                    Color {
                                        a: SHOOT_ALPHA * fade,
                                        ..starlight
                                    },
                                ),
                        )),
                        width: 1.5,
                        line_cap: canvas::LineCap::Round,
                        ..canvas::Stroke::default()
                    },
                );
                // Bright head with a small halo.
                frame.fill(
                    &canvas::Path::circle(head, 3.4),
                    Color {
                        a: 0.25 * fade,
                        ..starlight
                    },
                );
                frame.fill(
                    &canvas::Path::circle(head, 1.6),
                    Color {
                        a: SHOOT_ALPHA * fade,
                        ..starlight
                    },
                );
            }
        }

        // Deferred sparkle arms: gradient strokes tapering to nothing at
        // the tips (a lens glint, not an aliased butt-capped cross). The
        // horizontal arm runs at 0.6× the vertical — classic glint
        // proportions.
        for (center, arm, alpha) in deferred_arms {
            let vertical = canvas::Path::line(
                Point::new(center.x, center.y - arm),
                Point::new(center.x, center.y + arm),
            );
            frame.stroke(
                &vertical,
                canvas::Stroke {
                    style: canvas::Style::Gradient(canvas::Gradient::Linear(
                        canvas::gradient::Linear::new(
                            Point::new(center.x, center.y - arm),
                            Point::new(center.x, center.y + arm),
                        )
                        .add_stop(
                            0.0,
                            Color {
                                a: 0.0,
                                ..starlight
                            },
                        )
                        .add_stop(
                            0.5,
                            Color {
                                a: alpha,
                                ..starlight
                            },
                        )
                        .add_stop(
                            1.0,
                            Color {
                                a: 0.0,
                                ..starlight
                            },
                        ),
                    )),
                    width: 1.5,
                    line_cap: canvas::LineCap::Round,
                    ..canvas::Stroke::default()
                },
            );
            let harm = 0.6 * arm;
            let horizontal = canvas::Path::line(
                Point::new(center.x - harm, center.y),
                Point::new(center.x + harm, center.y),
            );
            frame.stroke(
                &horizontal,
                canvas::Stroke {
                    style: canvas::Style::Gradient(canvas::Gradient::Linear(
                        canvas::gradient::Linear::new(
                            Point::new(center.x - harm, center.y),
                            Point::new(center.x + harm, center.y),
                        )
                        .add_stop(
                            0.0,
                            Color {
                                a: 0.0,
                                ..starlight
                            },
                        )
                        .add_stop(
                            0.5,
                            Color {
                                a: alpha,
                                ..starlight
                            },
                        )
                        .add_stop(
                            1.0,
                            Color {
                                a: 0.0,
                                ..starlight
                            },
                        ),
                    )),
                    width: 1.5,
                    line_cap: canvas::LineCap::Round,
                    ..canvas::Stroke::default()
                },
            );
        }

        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sea_bars_shape_and_range() {
        let bars = sea_bars(0.37);
        assert_eq!(bars.len(), SEA_POINTS);
        assert!(
            bars.iter().all(|&v| (0.0..=1.0).contains(&v)),
            "every sample must stay in [0, 1]"
        );
        // The field must be a real wave, not a flat line.
        let min = bars.iter().copied().fold(f64::INFINITY, f64::min);
        let max = bars.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert!(
            max - min > 0.01,
            "the sea must undulate (span {})",
            max - min
        );
    }

    #[test]
    fn sea_bars_deterministic() {
        assert_eq!(sea_bars(0.5), sea_bars(0.5));
    }

    #[test]
    fn sea_bars_periodic_across_phase_wrap() {
        // The tick wraps phase with rem_euclid(1.0); integer phase
        // multipliers make sea_bars(1.0) ≡ sea_bars(0.0), so the wrap
        // frame can't visibly jump.
        let a = sea_bars(0.0);
        let b = sea_bars(1.0);
        let max_diff = a
            .iter()
            .zip(&b)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0, f64::max);
        assert!(
            max_diff < 1e-9,
            "phase 0 and phase 1 fields must match (max diff {max_diff})"
        );
    }

    #[test]
    fn sea_bars_travel_with_phase() {
        assert_ne!(
            sea_bars(0.0),
            sea_bars(0.25),
            "advancing the phase must move the wave"
        );
    }

    #[test]
    fn sky_glyphs_deterministic_and_in_band() {
        let a = sky_glyphs();
        let b = sky_glyphs();
        assert_eq!(
            a.len(),
            SKY_STAR_COUNT + SKY_SPARKLE_COUNT + SKY_FAINT_COUNT,
            "constellation size must match its consts"
        );
        for (ga, gb) in a.iter().zip(&b) {
            assert_eq!(
                (
                    ga.x,
                    ga.y,
                    ga.size,
                    ga.twinkle_k,
                    ga.twinkle_off,
                    ga.twinkle_depth
                ),
                (
                    gb.x,
                    gb.y,
                    gb.size,
                    gb.twinkle_k,
                    gb.twinkle_off,
                    gb.twinkle_depth
                ),
                "the constellation must be identical every build"
            );
        }
        for g in &a {
            assert!((0.0..=1.0).contains(&g.x), "x out of range: {}", g.x);
            assert!(
                (SKY_BAND_TOP..=SKY_BAND_BOTTOM).contains(&g.y),
                "glyph must stay in the sky band, got y {}",
                g.y
            );
            assert!(
                g.twinkle_k >= SKY_TWINKLE_K_MIN,
                "twinkle rate must stay an integer at or above the min"
            );
            assert!(g.twinkle_depth > 0.0, "every glyph carries a shimmer depth");
        }
        // The faint tier must exist: tiny stars at FULL twinkle depth, so
        // they fade entirely out of the sky and back.
        let faint: Vec<_> = a.iter().filter(|g| g.twinkle_depth >= 1.0).collect();
        assert_eq!(
            faint.len(),
            SKY_FAINT_COUNT,
            "exactly the faint tier runs at full fade depth"
        );
        for g in &faint {
            assert!(
                g.size < 0.7,
                "faint stars must sit below the main field's size floor, got {}",
                g.size
            );
        }
    }

    #[test]
    fn glow_stacks_hold_the_banding_contract() {
        // The glow reads as light (not stacked vector circles) only while
        // every EXPOSED rim's alpha step stays under the visibility floor,
        // the bright steps hide beneath the 0.60-opaque avatar, and no rim
        // coincides with the face's own edge.
        for (name, table, exposed_cap) in [
            ("sun", &SUN_GLOW_STACK[..], 0.015_f32),
            ("moon", &MOON_GLOW_STACK[..], 0.011),
        ] {
            for pair in table.windows(2) {
                assert!(
                    pair[0].0 > pair[1].0,
                    "{name}: radii must strictly descend (largest-first stack)"
                );
            }
            for (radius, alpha) in table {
                assert!(
                    !(0.95..=1.05).contains(radius),
                    "{name}: no rim may coincide with the face edge, got {radius}"
                );
                if *radius > 1.05 {
                    assert!(
                        *alpha <= exposed_cap + 1e-6,
                        "{name}: exposed rim at {radius} steps {alpha} > cap {exposed_cap}"
                    );
                }
            }
            let cumulative = 1.0 - table.iter().fold(1.0_f32, |acc, (_, a)| acc * (1.0 - a));
            assert!(
                (0.10..=0.20).contains(&cumulative),
                "{name}: cumulative center presence out of budget: {cumulative}"
            );
        }
    }

    #[test]
    fn sun_wedge_field_is_seamless_at_the_phase_wrap() {
        // At the wrap, ray i's full animated state must equal ray i+2's
        // start-of-cycle state (the TAU/6 spin advances exactly two ray
        // slots, and i and i+2 share tier parity) — otherwise the fan
        // snaps every 20 s.
        for i in 0..SUN_RAY_COUNT {
            let (t_end, r_end, a_end) = sun_ray_geometry(i, 1.0 - 1e-6);
            let (t_start, r_start, a_start) = sun_ray_geometry((i + 2) % SUN_RAY_COUNT, 0.0);
            // Angles may differ by whole turns — compare on the circle.
            assert!(
                (t_end.sin() - t_start.sin()).abs() < 1e-3
                    && (t_end.cos() - t_start.cos()).abs() < 1e-3,
                "ray {i}: wrap angle mismatch"
            );
            assert!((r_end - r_start).abs() < 1e-3, "ray {i}: wrap tip mismatch");
            assert!(
                (a_end - a_start).abs() < 1e-3,
                "ray {i}: wrap alpha mismatch"
            );
        }
    }

    #[test]
    fn gull_params_deterministic_and_sane() {
        let a = gull_params();
        assert_eq!(a.len(), GULL_COUNT);
        for (ga, gb) in a.iter().zip(&gull_params()) {
            assert_eq!(
                (ga.x0, ga.y, ga.k, ga.leftward, ga.size, ga.bob_k),
                (gb.x0, gb.y, gb.k, gb.leftward, gb.size, gb.bob_k),
                "the flock must be identical every build"
            );
        }
        for g in &a {
            assert!(
                g.k >= 1,
                "glide rate must be a positive integer (wrap-safety)"
            );
            assert!(g.bob_k >= 1, "bob rate must be a positive integer");
            assert!(
                (0.06..=0.30).contains(&g.y),
                "gulls must stay in the sky band, got y {}",
                g.y
            );
        }
    }

    #[test]
    fn wandering_note_windows_stay_inside_the_cycle() {
        // A wandering note's window must never straddle the cycle boundary
        // — its position hash would change mid-appearance. Max start
        // (0.05 + 0.68) + SKY_NOTE_DUR must stay below 1.0.
        const _: () = assert!(0.05 + 0.68 + SKY_NOTE_DUR < 1.0);
        // And the note's hashed spot must vary across cycles.
        let salt = 0x407E;
        assert_ne!(
            hash01(1, salt + 1),
            hash01(2, salt + 1),
            "consecutive cycles must deal different note positions"
        );
    }

    #[test]
    fn hash01_deterministic_and_unit_range() {
        for cycle in [0_u32, 1, 2, 17, 9999, u32::MAX] {
            for salt in [0x57A2_u32, 0xF1_5E, 1] {
                let v = hash01(cycle, salt);
                assert_eq!(v, hash01(cycle, salt), "hash must be deterministic");
                assert!((0.0..=1.0).contains(&v), "hash out of range: {v}");
            }
        }
        // Consecutive cycles must not collapse to the same draw.
        assert_ne!(hash01(1, 0x57A2), hash01(2, 0x57A2));
    }

    #[test]
    fn riser_params_deterministic_and_sane() {
        let a = riser_params();
        assert_eq!(a.len(), RISER_COUNT);
        for (ra, rb) in a.iter().zip(&riser_params()) {
            assert_eq!(
                (ra.k, ra.off, ra.dx, ra.sway_off, ra.beamed),
                (rb.k, rb.off, rb.dx, rb.sway_off, rb.beamed),
                "riser pool must be identical every build"
            );
        }
        for r in &a {
            assert!(
                r.k >= 1,
                "riser rate must be a positive integer (wrap-safety)"
            );
            assert!((0.0..1.0).contains(&r.off));
        }
    }

    #[test]
    fn sweep_stops_stay_in_gradient_domain() {
        // Sweep the band center across its full off-panel-to-off-panel
        // run and hold the packed-gradient contract at every position:
        // ascending offsets in [0, 1], first at 0.0, last at exactly 1.0.
        // The extra probes sit an epsilon off the boundaries — the case
        // where a naive dedup once dropped the exact-1.0 boundary stop.
        let grid = (0..=48).map(|i| -0.10 + (i as f32 / 48.0) * 1.20);
        for c in grid.chain([0.89995, 0.000_4, 0.999_6, -0.099_9, 1.099_9]) {
            let stops = sweep_stops(c, CREST_SWEEP_HALF_WIDTH, CREST_SHIMMER_ALPHA);
            assert!(stops.len() >= 2, "at least the two boundary stops");
            assert_eq!(stops.first().map(|s| s.0), Some(0.0));
            assert_eq!(stops.last().map(|s| s.0), Some(1.0));
            for pair in stops.windows(2) {
                assert!(
                    pair[0].0 < pair[1].0,
                    "stops must ascend strictly: {} then {}",
                    pair[0].0,
                    pair[1].0
                );
            }
            for (offset, alpha) in &stops {
                assert!((0.0..=1.0).contains(offset));
                assert!((0.0..=CREST_SHIMMER_ALPHA + 1e-6).contains(alpha));
            }
        }
    }

    #[test]
    fn back_swell_periodic_and_bounded() {
        for i in 0..=20 {
            let x = i as f64 / 20.0;
            let v = back_swell_height(x, 0.7);
            assert!((0.0..=1.0).contains(&v));
        }
        assert!((back_swell_height(0.3, 0.0) - back_swell_height(0.3, 1.0)).abs() < 1e-9);
    }

    #[test]
    fn bubble_params_deterministic_and_wrap_safe() {
        let a = bubble_params();
        assert_eq!(a.len(), BUBBLE_COUNT);
        for (ba, bb) in a.iter().zip(&bubble_params()) {
            assert_eq!(
                (ba.k, ba.off, ba.dx, ba.size, ba.sway_off, ba.ring),
                (bb.k, bb.off, bb.dx, bb.size, bb.sway_off, bb.ring),
                "the bubble pool must be identical every build"
            );
        }
        for b in &a {
            assert!(
                b.k >= 1,
                "rise rate must be a positive integer (wrap-safety)"
            );
            assert!((0.0..1.0).contains(&b.off));
            assert!(b.size > 0.0);
        }
        // Both kinds must be dealt: the stream reads as bubbles because
        // rings and flecks mix.
        assert!(a.iter().any(|b| b.ring), "at least one ring bubble");
        assert!(a.iter().any(|b| !b.ring), "at least one fleck bubble");
    }

    #[test]
    fn blackhole_s_is_zero_at_ends_accelerates_and_spits_past_home() {
        // The static-positions contract survives the event boundary
        // only if displacement is exactly zero as the window opens and
        // closes.
        assert_eq!(blackhole_s(0.0), 0.0);
        assert_eq!(blackhole_s(1.0), 0.0);
        // The plunge ACCELERATES — gravity, not an ease: the second
        // half of the fall covers far more than the first.
        let early = blackhole_s(BLACKHOLE_PLUNGE_END * 0.5);
        let late = blackhole_s(BLACKHOLE_PLUNGE_END * 0.999);
        assert!(
            late > 3.0 * early,
            "the dive must accelerate (early {early}, late {late})"
        );
        // The catch holds full capture...
        let mid = (BLACKHOLE_PLUNGE_END + BLACKHOLE_HOLD_END) * 0.5;
        assert!((blackhole_s(mid) - 1.0).abs() < 1e-6);
        // ...and the spit-out sails PAST home: s dips negative (radius
        // beyond the star's rest position) before settling to zero.
        let mut dip = 0.0_f32;
        for i in 0..=100 {
            let q = i as f32 / 100.0;
            let p = BLACKHOLE_HOLD_END + q * (1.0 - BLACKHOLE_HOLD_END);
            dip = dip.min(blackhole_s(p.min(1.0)));
        }
        assert!(
            dip < -0.08,
            "the ejection must overshoot past home, got min s {dip}"
        );
        for i in 0..=40 {
            let p = i as f32 / 40.0;
            assert!(blackhole_s(p) <= 1.0 && blackhole_s(p) > -0.5);
        }
    }

    #[test]
    fn blackhole_gravity_is_local_and_boundary_exact() {
        let hole = Point::new(150.0, 60.0);
        let capture = 100.0;
        let near = Point::new(170.0, 60.0); // dist 20 — full grip
        let far = Point::new(150.0 + capture + 1.0, 60.0); // beyond reach
        // Bit-exact identity at s 0 (the early return) — no atan2
        // round-trip error can leak into non-event frames.
        let rest = blackhole_displace(near, hole, 0.0, 0.0, capture);
        assert_eq!((rest.x, rest.y), (near.x, near.y));
        // Gravity is LOCAL: a star beyond the capture radius never
        // stirs, even at full capture — bit-exact.
        let unmoved = blackhole_displace(far, hole, 1.0, 0.4, capture);
        assert_eq!((unmoved.x, unmoved.y), (far.x, far.y));
        assert_eq!(blackhole_grip(0.0, capture), 1.0);
        assert_eq!(blackhole_grip(capture, capture), 0.0);
        // A fully-gripped star at full capture converges to the core,
        // spin or no spin (the whirl moves the angle, not the radius).
        let pulled = blackhole_displace(near, hole, 1.0, 0.7, capture);
        let r1 = ((pulled.x - hole.x).powi(2) + (pulled.y - hole.y).powi(2)).sqrt();
        assert!(
            (r1 - 20.0 * BLACKHOLE_CONVERGE).abs() < 1e-3,
            "full grip converges to the core, got r {r1}"
        );
        // Negative s (the spit-out) throws it PAST home.
        let spat = blackhole_displace(near, hole, -0.13, 0.0, capture);
        let r2 = ((spat.x - hole.x).powi(2) + (spat.y - hole.y).powi(2)).sqrt();
        assert!(
            r2 > 20.0,
            "ejection must overshoot the rest radius, got {r2}"
        );
        // A star already at the center stays put (no NaN from atan2).
        let centered = blackhole_displace(hole, hole, 0.7, 0.3, capture);
        assert_eq!((centered.x, centered.y), (hole.x, hole.y));
    }

    #[test]
    fn blackhole_visibility_swallows_at_the_horizon_and_never_at_rest() {
        let horizon = 12.0;
        // At rest (s = 0) light is untouched at ANY distance — even a
        // star whose HOME sits beside a hashed center renders the fixed
        // field bit-identically outside events.
        assert_eq!(blackhole_visibility(0.0, horizon, 0.0, 1.0), 1.0);
        // Beyond the fade band, untouched even at full capture.
        assert_eq!(blackhole_visibility(2.0 * horizon, horizon, 1.0, 1.0), 1.0);
        // At the horizon with full grip and capture: fully swallowed —
        // the light does not escape.
        assert!(blackhole_visibility(0.5 * horizon, horizon, 1.0, 1.0) < 0.01);
        // Monotone re-lighting on the way back out.
        let deep = blackhole_visibility(0.7 * horizon, horizon, 1.0, 1.0);
        let shallow = blackhole_visibility(1.3 * horizon, horizon, 1.0, 1.0);
        assert!(deep < shallow, "light must return crossing back out");
    }

    #[test]
    fn hashed_deals_spread_even_conditioned_on_the_gate() {
        // The regression the GF(2)-linear hash hid: CONDITIONED on the
        // rare-event gate passing, sibling-salted deals (center, start)
        // must still span their ranges — under the old xorshift-only
        // mixer every gate-passing cycle dealt the hole into a ~7 px
        // box at the same start phase, forever.
        let mut fxs: Vec<f32> = Vec::new();
        let mut starts: Vec<f32> = Vec::new();
        for cycle in 0..4000_u32 {
            if hash01(cycle, BLACKHOLE_SALT) < BLACKHOLE_CHANCE {
                fxs.push(blackhole_center(cycle).0);
                starts.push(hash01(cycle, BLACKHOLE_SALT ^ 0x9E37));
            }
        }
        assert!(
            fxs.len() > 50,
            "the gate must pass often enough to sample ({} hits)",
            fxs.len()
        );
        let spread = |v: &[f32]| {
            v.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b))
                - v.iter().fold(f32::INFINITY, |a, &b| a.min(b))
        };
        assert!(
            spread(&fxs) > 0.15,
            "gate-conditioned centers must spread across the sky, got {}",
            spread(&fxs)
        );
        assert!(
            spread(&starts) > 0.5,
            "gate-conditioned start phases must spread, got {}",
            spread(&starts)
        );
    }

    #[test]
    fn blackhole_deals_vary_and_center_stays_in_the_open_sky() {
        assert_ne!(
            blackhole_center(1),
            blackhole_center(2),
            "consecutive cycles must deal different centers"
        );
        for cycle in 0..200_u32 {
            let (fx, fy) = blackhole_center(cycle);
            assert!(
                (0.52..=0.82).contains(&fx),
                "center stays in the upper-right sky, clear of the moon: {fx}"
            );
            assert!(
                (0.09..=0.22).contains(&fy),
                "center stays inside the sky band: {fy}"
            );
        }
    }

    #[test]
    fn moonbeam_params_deterministic_and_wrap_safe() {
        let a = moonbeam_params();
        assert_eq!(a.len(), MOONBEAM_ENTRY_DX.len());
        for (ma, mb) in a.iter().zip(&moonbeam_params()) {
            assert_eq!(
                (
                    ma.entry_dx,
                    ma.k_breath,
                    ma.off_breath,
                    ma.k_sway,
                    ma.off_sway
                ),
                (
                    mb.entry_dx,
                    mb.k_breath,
                    mb.off_breath,
                    mb.k_sway,
                    mb.off_sway
                ),
                "the shafts must be identical every build"
            );
        }
        for (m, dx) in a.iter().zip(MOONBEAM_ENTRY_DX) {
            assert_eq!(m.entry_dx, dx, "shafts stay slaved to the entry table");
            assert!(
                (1..=2).contains(&m.k_breath),
                "breath rate stays an integer in 1..=2 (wrap-safety)"
            );
            assert!((1..=2).contains(&m.k_sway));
            assert!((0.0..1.0).contains(&m.off_breath));
            assert!((0.0..1.0).contains(&m.off_sway));
        }
        // The vertical run: top zero-stop at the deepest trough, peak
        // inside the run at the bed vignette's start, bottom dissolving
        // above the bed floor (the glow-stack banding idiom).
        let trough = 1.0 - (SEA_DC - SWELL_AMP - RIPPLE_AMP) as f32;
        assert!(trough < SEA_BED_TOP && SEA_BED_TOP < 0.92);
    }

    #[test]
    fn glitter_params_deterministic_and_wrap_safe() {
        let a = glitter_params();
        assert_eq!(a.len(), GLITTER_COUNT);
        for (ga, gb) in a.iter().zip(&glitter_params()) {
            assert_eq!(
                (ga.x, ga.depth_px, ga.len, ga.k, ga.off),
                (gb.x, gb.depth_px, gb.len, gb.k, gb.off),
                "the glitter lane must be identical every build"
            );
        }
        for g in &a {
            assert!(
                (6..=11).contains(&g.k),
                "flash rate stays an integer in 6..=11 (wrap-safety + the sub-0.6 Hz twinkle law)"
            );
            assert!((0.0..1.0).contains(&g.off));
            assert!(
                (0.03..=0.56).contains(&g.x),
                "dashes pack under the sun's azimuth, got x {}",
                g.x
            );
        }
    }

    #[test]
    fn sail_run_stays_inside_the_panel_for_both_headings() {
        // Recompute dir/span/x0 exactly as the draw does for many cycles
        // and pin the full run (sprite half-width ~0.03w) inside the
        // panel for both headings.
        for cycle in 0..500_u32 {
            let dir = if hash01(cycle, SAIL_SALT.wrapping_add(1)) < 0.5 {
                -1.0_f32
            } else {
                1.0
            };
            let span = hash01(cycle, SAIL_SALT.wrapping_add(3));
            let x0 = if dir > 0.0 {
                0.13 + 0.36 * span
            } else {
                0.87 - 0.36 * span
            };
            for p in [0.0_f32, 0.5, 1.0] {
                let xf = x0 + dir * 0.28 * p;
                assert!(
                    (0.08..=0.92).contains(&xf),
                    "cycle {cycle} heading {dir}: sail at {xf} leaves the panel"
                );
            }
        }
    }

    #[test]
    fn serpent_deals_vary_across_cycles() {
        // Consecutive cycles must not replay the same passage — timing
        // and depth both re-hash (the wandering-note contract). Band
        // clearances are const-asserted beside the SERPENT_* consts.
        assert_ne!(hash01(1, 0xDEEA), hash01(2, 0xDEEA));
        assert_ne!(hash01(1, 0xDEEB), hash01(2, 0xDEEB));
    }

    #[test]
    fn crate_landmark_clears_the_dealt_starfish() {
        // Lane bounds + kelp-loner clearance are const-asserted beside
        // the CRATE_* consts; the starfish is dealt live from BED_SEED,
        // so its clearance is the runtime pin.
        assert!(
            (bed_dressing().star_x - CRATE_X).abs() >= 0.10,
            "starfish clearance"
        );
    }

    #[test]
    fn bed_dressing_deterministic_and_grounded() {
        let a = bed_dressing();
        let b = bed_dressing();
        assert_eq!(a.rocks.len(), ROCK_COUNT);
        for (ra, rb) in a.rocks.iter().zip(&b.rocks) {
            assert_eq!(
                (ra.x, ra.w, ra.ht),
                (rb.x, rb.w, rb.ht),
                "the rocks must be identical every build"
            );
        }
        assert_eq!(
            (a.star_x, a.star_rot, a.star_size),
            (b.star_x, b.star_rot, b.star_size),
            "the starfish must be identical every build"
        );
        for r in &a.rocks {
            assert!(
                (0.10..=0.90).contains(&r.x),
                "rocks stay inside the panel, got x {}",
                r.x
            );
            assert!(r.w > 0.0 && r.ht > 0.0);
        }
        assert!(
            (0.05..=0.95).contains(&a.star_x),
            "starfish stays inside the panel, got x {}",
            a.star_x
        );
    }

    #[test]
    fn school_params_deterministic_and_under_the_trough() {
        let a = school_params();
        assert_eq!(a.len(), SCHOOL_COUNT);
        for (fa, fb) in a.iter().zip(&school_params()) {
            assert_eq!(
                (
                    fa.x0,
                    fa.y,
                    fa.k,
                    fa.leftward,
                    fa.size,
                    fa.bob_k,
                    fa.bob_off
                ),
                (
                    fb.x0,
                    fb.y,
                    fb.k,
                    fb.leftward,
                    fb.size,
                    fb.bob_k,
                    fb.bob_off
                ),
                "the school must be identical every build"
            );
        }
        // The front crest bottoms out at y ≈ 1 − (DC − amps) ≈ 0.638h;
        // the band (minus bob headroom) must sit below it so a drifter
        // can never fly in air.
        let trough = 1.0 - (SEA_DC - SWELL_AMP - RIPPLE_AMP) as f32;
        assert!(
            SCHOOL_BAND_TOP - 0.008 > trough,
            "school band must clear the deepest trough ({trough})"
        );
        for f in &a {
            assert!(f.k >= 1, "glide rate must be a positive integer");
            assert!(f.bob_k >= 1, "bob rate must be a positive integer");
            assert!(
                (SCHOOL_BAND_TOP..=SCHOOL_BAND_BOTTOM).contains(&f.y),
                "drifter must stay in the mid-water band, got y {}",
                f.y
            );
        }
    }

    #[test]
    fn kelp_params_deterministic_and_varied() {
        let a = kelp_params();
        assert_eq!(a.len(), 7, "flank clusters plus two mid loners");
        for (ka, kb) in a.iter().zip(&kelp_params()) {
            assert_eq!(
                (
                    ka.x,
                    ka.height,
                    ka.sway_k,
                    ka.sway_off,
                    ka.lean,
                    ka.seep_k,
                    ka.seep_off
                ),
                (
                    kb.x,
                    kb.height,
                    kb.sway_k,
                    kb.sway_off,
                    kb.lean,
                    kb.seep_k,
                    kb.seep_off
                ),
                "the kelp must be identical every build"
            );
        }
        for k in &a {
            assert!(
                (0.02..=0.97).contains(&k.x),
                "kelp roots stay inside the panel, got x {}",
                k.x
            );
            assert!(k.sway_k >= 1, "sway rate integer ≥ 1 (wrap-safety)");
            assert!(k.seep_k >= 1, "seep rate integer ≥ 1 (wrap-safety)");
            assert!((0.05..=0.25).contains(&k.height));
        }
        // The beds must read as growth, not a fence: real height spread.
        let min = a.iter().map(|k| k.height).fold(f32::INFINITY, f32::min);
        let max = a.iter().map(|k| k.height).fold(f32::NEG_INFINITY, f32::max);
        assert!(
            max > min * 1.4,
            "frond heights must vary (min {min}, max {max})"
        );
    }

    /// The dream's boundary identity: at both window ends every mark is
    /// EXACTLY 0.0 — the frames either side of a ritual render the bare
    /// resting disc. A botched envelope here would leave a stray mark on
    /// the moon that is supposed to sail faceless between dreams.
    #[test]
    fn moon_dream_alphas_are_bare_at_both_window_ends() {
        assert_eq!(moon_dream_alphas(0.0), [0.0; 4]);
        assert_eq!(moon_dream_alphas(1.0), [0.0; 4]);
    }

    /// Mid-ritual the face is genuinely whole: after the last mark
    /// settles and before the farewell begins, all four alphas are one.
    #[test]
    fn moon_dream_completes_the_face_before_the_farewell() {
        let t = (MOON_DREAM_VERSE_START
            + 3.0 * MOON_DREAM_VERSE_SPAN
            + MOON_DREAM_MARK_LAG
            + MOON_DREAM_IN_SECS
            + MOON_DREAM_OUT_START[3])
            / 2.0;
        assert_eq!(moon_dream_alphas(t / MOON_DREAM_SECS), [1.0; 4]);
    }

    /// The eyepatch and its strap never hold intermediate alpha at the
    /// same instant — their ink overlaps where the strap crosses the
    /// patch, and a simultaneous half-fade would double-expose the seam.
    /// Nobody would think to look for this by eye; it is the one guard
    /// against an invisible compositing artifact.
    #[test]
    fn moon_dream_patch_and_strap_never_fade_together() {
        let mid = |x: f32| x > 1e-4 && x < 1.0 - 1e-4;
        for i in 0..=4000 {
            let p = i as f32 / 4000.0;
            let a = moon_dream_alphas(p);
            assert!(
                !(mid(a[2]) && mid(a[3])),
                "patch {} and strap {} both mid-fade at p {p}",
                a[2],
                a[3]
            );
        }
    }

    /// Marks arrive in verse order — the grin first, the strap last —
    /// and leave in REVERSE order in the farewell, the strap first and
    /// the grin lingering last.
    #[test]
    fn moon_dream_marks_arrive_in_verse_order_and_leave_in_reverse() {
        let sweep = || (0..=4000).map(|i| i as f32 / 4000.0);
        let arrived: Vec<f32> = (0..4)
            .map(|m| {
                sweep()
                    .find(|&p| moon_dream_alphas(p)[m] >= 1.0 - 1e-6)
                    .unwrap_or_else(|| panic!("mark {m} never arrives"))
            })
            .collect();
        for pair in arrived.windows(2) {
            assert!(
                pair[0] < pair[1],
                "marks must arrive in order, got {arrived:?}"
            );
        }
        let departed: Vec<f32> = (0..4)
            .map(|m| {
                sweep()
                    .find(|&p| p > arrived[m] && moon_dream_alphas(p)[m] <= 1e-6)
                    .unwrap_or_else(|| panic!("mark {m} never departs"))
            })
            .collect();
        for pair in departed.windows(2) {
            assert!(
                pair[0] > pair[1],
                "marks must depart in reverse, got {departed:?}"
            );
        }
        assert!(
            departed[0] < 1.0,
            "the grin's farewell completes before the window closes"
        );
    }

    /// The verse windows tile the recital: at most one verse is audible
    /// at any instant, and the last has faded fully out before the
    /// window ends.
    #[test]
    fn moon_dream_verses_speak_one_at_a_time() {
        for i in 0..=4000 {
            let p = i as f32 / 4000.0;
            let audible = (0..4)
                .filter(|&line| moon_dream_verse_alpha(p, line) > 1e-4)
                .count();
            assert!(audible <= 1, "{audible} verses audible at p {p}");
        }
        assert!(moon_dream_verse_alpha(1.0, 3) <= f32::EPSILON);
    }

    /// The launch greeting: cycle 0 always dreams, and its hashed window
    /// sits fully inside the cycle (no dream can straddle a wrap, where
    /// its hash — and choreography — would change mid-ritual).
    #[test]
    fn moon_dream_greets_the_launch_inside_its_cycle() {
        assert!(moon_dream_cycle(0), "cycle 0 must carry the greeting");
        for cycle in 0..10_000u32 {
            if !moon_dream_cycle(cycle) {
                continue;
            }
            let start = 0.10 + 0.15 * hash01(cycle, MOON_DREAM_SALT ^ 0x9E37);
            assert!(
                start + MOON_DREAM_WINDOW < 1.0,
                "cycle {cycle} straddles the wrap"
            );
            assert!(moon_dream_progress(0.0, cycle).is_none());
        }
    }

    /// Outside the window — and on every non-dream cycle — the veil key
    /// is the resting BARE key: the render path takes the ordinary
    /// cached bare-disc handle and the dream machinery is invisible.
    #[test]
    fn moon_dream_veil_key_rests_bare_outside_the_window() {
        use crate::embedded_svg::MOON_VEIL_BARE;
        let start = 0.10 + 0.15 * hash01(0, MOON_DREAM_SALT ^ 0x9E37);
        assert_eq!(moon_dream_veil_key(start - 0.01, 0), MOON_VEIL_BARE);
        assert_eq!(
            moon_dream_veil_key(start + MOON_DREAM_WINDOW + 0.01, 0),
            MOON_VEIL_BARE
        );
        let quiet = (1u32..)
            .find(|&c| !moon_dream_cycle(c))
            .expect("some cycle must not dream");
        for i in 0..=20 {
            assert_eq!(moon_dream_veil_key(i as f32 / 20.0, quiet), MOON_VEIL_BARE);
        }
    }

    /// At the whole-face hold the veil key is the fully-opaque key — the
    /// guard that the quantizer actually engages (a broken progress gate
    /// would leave the moon permanently bare and the dream silently
    /// invisible, the one failure mode nobody would notice).
    #[test]
    fn moon_dream_veil_key_engages_inside_the_window() {
        let start = 0.10 + 0.15 * hash01(0, MOON_DREAM_SALT ^ 0x9E37);
        let hold_t = (MOON_DREAM_VERSE_START
            + 3.0 * MOON_DREAM_VERSE_SPAN
            + MOON_DREAM_MARK_LAG
            + MOON_DREAM_IN_SECS
            + MOON_DREAM_OUT_START[3])
            / 2.0;
        let hold = start + hold_t * MOON_DREAM_WINDOW / MOON_DREAM_SECS;
        assert_eq!(
            moon_dream_veil_key(hold, 0),
            crate::embedded_svg::MOON_VEIL_OPAQUE
        );
    }
}
