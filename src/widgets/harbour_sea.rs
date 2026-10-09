//! Procedural sea + trawling-longship scene for the Harbour Trawl panel.
//!
//! The Harbour landing view opens centered on the Trawl mix-builder row, whose
//! artwork panel used to show a static anchor glyph. This module replaces it
//! with a living scene: a gently swaying two-layer sea (lit per pixel by
//! `harbour_light`, night or day, and furnished by [`SeaCanvas`]) with the
//! nokkvi longship sailing across it, perpetually
//! dragging its anchor along the seabed — trawling. The boat itself is the
//! Lines-visualizer surfing boat reused verbatim ([`boat_overlay`] with a
//! `trail` offset); only the wave source is new.
//!
//! Coherence contract: [`sea_bars`] produces ONE array per tick
//! (`update::boat::step_harbour_scene`), which is BOTH fed to
//! `boat_physics::step()` and stored on `Nokkvi.harbour_scene.sea_bars`,
//! which [`sea_light`] resamples for the shader through the same
//! [`sample_line_height`] sampler the physics used. A phase or sampler mismatch would desync the hull from the
//! drawn water invisibly to tests/clippy — always route both sides through
//! this module.
//!
//! The motion is silence-proof by construction: the sea is a pure function
//! of a phase the frame tick advances, and the physics' presence cruise is
//! fed a fixed [`HARBOUR_CRUISE_BAR_ENERGY`] instead of live audio, so the
//! scene moves identically with the player stopped, paused, or playing. Only
//! the night LIGHT follows the music (`harbour_light::HarbourMusic`).

use iced::{Color, Element, Length, Point, Rectangle, Size, Widget as _, widget::canvas};

use crate::widgets::{
    boat::{BoatState, LineGeometry, boat_overlay, parse_hex_color, sample_line_height},
    harbour_runes,
};

/// Samples in the sea height field — enough that the Catmull-Rom resample
/// reads as a smooth swell at panel widths, few enough that building the
/// array per frame is negligible.
pub(crate) const SEA_POINTS: usize = 96;

/// Travelling-phase advance rate in cycles/sec. The front swell's crest
/// speed is `(phase_k / cycles) · SEA_DRIFT_HZ` panel-widths
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

/// Sea shape — a few long, low folds drifting in opposite directions, so
/// the water sways like the aurora curtain's arc instead of rolling hills
/// past the boat. Every fold's phase multiplier is an INTEGER so the field is
/// exactly periodic in the `[0, 1)` phase (`sea_bars(0) == sea_bars(1)`):
/// the tick wraps the phase with `rem_euclid(1.0)` to dodge long-session f32
/// sin precision decay, and integer multipliers make that wrap seamless.
/// Cycle counts are integers too, which additionally makes the field
/// periodic in X — the boat's toroidal slope sampling near the wrap seam
/// then reads a REAL gradient instead of a fake edge cliff. A fold's crest
/// speed is `(phase_k / cycles) · SEA_DRIFT_HZ` panel-widths per second; a
/// negative `phase_k` drifts it the other way.
// TUNE: DC sets the waterline height (fraction of the scene); the fold amps
// set how far the water sways (their sum is the reach above/below DC).
const SEA_DC: f64 = 0.45;

/// One drifting sine fold of a waterline.
struct SeaFold {
    amp: f64,
    cycles: f64,
    phase_k: f64,
    shift: f64,
}

const FRONT_FOLDS: [SeaFold; 4] = [
    SeaFold {
        amp: 0.022,
        cycles: 1.0,
        phase_k: 1.0,
        shift: 0.0,
    },
    SeaFold {
        amp: 0.013,
        cycles: 2.0,
        phase_k: -1.0,
        shift: 1.3,
    },
    SeaFold {
        amp: 0.007,
        cycles: 3.0,
        phase_k: 2.0,
        shift: 2.6,
    },
    SeaFold {
        amp: 0.0035,
        cycles: 5.0,
        phase_k: -3.0,
        shift: 0.4,
    },
];

/// Back parallax layer — drawn only (the physics never samples it), a
/// dimmer, slower sway riding higher on the panel.
// TUNE: BACK_RAISE lifts the horizon; the fold amps set the far sway.
const BACK_RAISE: f64 = 0.12;
const BACK_FOLDS: [SeaFold; 3] = [
    SeaFold {
        amp: 0.014,
        cycles: 1.0,
        phase_k: -1.0,
        shift: 0.7,
    },
    SeaFold {
        amp: 0.008,
        cycles: 3.0,
        phase_k: 1.0,
        shift: 2.0,
    },
    SeaFold {
        amp: 0.004,
        cycles: 4.0,
        phase_k: -2.0,
        shift: 4.1,
    },
];

/// The farthest a waterline built from `folds` reaches above or below its
/// rest height (every fold at its crest at once).
const fn fold_reach(folds: &[SeaFold]) -> f64 {
    let mut sum = 0.0;
    let mut i = 0;
    while i < folds.len() {
        sum += folds[i].amp;
        i += 1;
    }
    sum
}

/// Reach of the front water (the line the boat rides) and the far swell.
const SEA_REACH: f64 = fold_reach(&FRONT_FOLDS);

/// Height offset of a waterline built from `folds` at `x ∈ [0, 1]`.
fn folds_height(folds: &[SeaFold], x: f64, phase: f64) -> f64 {
    use std::f64::consts::TAU;
    folds
        .iter()
        .map(|f| f.amp * (TAU * (x * f.cycles - f.phase_k * phase) + f.shift).sin())
        .sum()
}

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
const _: () = assert!(
    SKY_STAR_COUNT + SKY_SPARKLE_COUNT + SKY_FAINT_COUNT
        <= crate::widgets::harbour_light::MAX_STARS
);
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
/// The lit (night) scene's underwater furniture: dark silhouettes whose
/// edges catch the aurora's light (`NightInk`), and how much the notes keep
/// of their alpha once the shader adds their glow.
// TUNE: silhouette = how solid the shapes read; rim = how lit their edges.
const NIGHT_SILHOUETTE_ALPHA: f32 = 0.92;
const NIGHT_RIM_ALPHA: f32 = 0.42;
const NIGHT_NOTE_GAIN: f32 = 0.75;
/// Glow under each note (shader), radius per glyph px, and the kelp beads'
/// and the anchor's glints.
const NOTE_GLOW: f32 = 0.22;
const KELP_BEAD_GLOW: f32 = 0.9;
const ANCHOR_GLINT: f32 = 0.18;
/// Extra top inset for note glyphs: their stems extend ~0.03h ABOVE the
/// glyph center, and a note whose center lands at the raw band top clips
/// mid-glyph against the panel edge (a shipped capture caught exactly
/// that). Dots/sparkles have no such reach, so only notes take the inset.
const SKY_NOTE_TOP_INSET: f32 = 0.06;
/// Seed for the constellation scatter. Changing it deals a new sky.
const SKY_SEED: u32 = 0x5EA_57A5;

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

/// The moon's exhale: some cycles a soft two-stroke ring detaches at the
/// halo's shoulder, expands past the rim, and dissolves (cycle-hashed
/// timing, alpha-zero at both ends). The moon's steady light is the night
/// shader's bloom (`harbour_light`).
// TUNE: PULSE_CHANCE/DUR = exhale cadence.
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
/// Flight: bursts of wingbeats between glides. Each gull's burst envelope
/// runs `GULL_BURST_K` times per sea cycle (~4 s) and its wings beat
/// `GULL_BEAT_K` times (~2.5 Hz), both integers (wrap-safe); `GLIDE` is the
/// resting V, `BEAT_AMP` how far a beat swings the wings up and down.
// TUNE: BEAT_K = wingbeat speed; BURST_K = how often a gull flaps.
const GULL_BURST_K: f32 = 5.0;
const GULL_BEAT_K: f32 = 50.0;
const GULL_GLIDE: f32 = 0.42;
const GULL_BEAT_AMP: f32 = 0.45;
/// Fish tails waggle as they swim: `FISH_WAG_K` beats per sea cycle
/// (~2 Hz; a leaping fish thrashes at `FISH_WAG_K_LEAP`), swinging the
/// tail `FISH_WAG_RAD` either way about its root. Integers (wrap-safe).
const FISH_WAG_K: f32 = 40.0;
const FISH_WAG_K_LEAP: f32 = 70.0;
const FISH_WAG_RAD: f32 = 0.35;
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
// The front water bottoms out at y = 1 − (DC − reach) from the top; the
// band (minus bob headroom) sits below it so a drifter never flies in air.
const _: () = assert!(SCHOOL_BAND_TOP - 0.008 > 1.0 - (SEA_DC - SEA_REACH) as f32);
const SCHOOL_BAND_BOTTOM: f32 = 0.72;
const SCHOOL_MARGIN_PX: f32 = 26.0;
const SCHOOL_RIM_ALPHA: f32 = 0.30;
const SCHOOL_SEED: u32 = 0x0005_C001;

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

/// Fill the fish silhouette — teardrop body + notched tail — at the
/// current frame origin, `l` px long, nose toward +x when `dir` is
/// `1.0` (pass `-1.0` to mirror). Shared by the rare leaping fish and
/// the drifting school so the two can never drift apart in shape.
fn fill_fish_silhouette(frame: &mut canvas::Frame, l: f32, dir: f32, wag: f32, color: Color) {
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
    // The tail swings `wag` radians about its root as the fish swims.
    let root = Point::new(dir * -0.45 * l, 0.0);
    let (sw, cw) = wag.sin_cos();
    let at = |x: f32, y: f32| {
        let (dx, dy) = (x - root.x, y - root.y);
        Point::new(root.x + dx * cw - dy * sw, root.y + dx * sw + dy * cw)
    };
    let tail = canvas::Path::new(|b| {
        b.move_to(root);
        b.line_to(at(dir * -0.78 * l, -0.24 * l));
        b.line_to(at(dir * -0.70 * l, 0.0));
        b.line_to(at(dir * -0.78 * l, 0.24 * l));
        b.close();
    });
    frame.fill(&body, color);
    frame.fill(&tail, color);
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
    (0..SEA_POINTS)
        .map(|i| {
            let x = i as f64 / (SEA_POINTS - 1) as f64;
            (SEA_DC + folds_height(&FRONT_FOLDS, x, phase as f64)).clamp(0.0, 1.0)
        })
        .collect()
}

/// Height of the decorative back swell at `x ∈ [0, 1]` — drawn behind the
/// front waterline at half its crest speed for the parallax depth read.
/// Analytic (no array) because only the canvas consumes it.
fn back_swell_height(x: f64, phase: f32) -> f64 {
    (SEA_DC + BACK_RAISE + folds_height(&BACK_FOLDS, x, phase as f64)).clamp(0.0, 1.0)
}

/// Whether the scene is the night one (a dark theme): the shader's night
/// light, the moonlit boat, the night furniture inks. Light themes get the
/// sunlit day scene. The one predicate the view, `sea_light` and the
/// harbour tick (boat paint) read.
pub(crate) fn scene_is_lit() -> bool {
    !crate::theme::is_light_mode()
}

/// A music note in flight this frame: centre, glyph size and alpha (px),
/// beamed pair or single quaver.
#[derive(Debug, Clone, Copy)]
struct NoteFrame {
    center: Point,
    size: f32,
    alpha: f32,
    beamed: bool,
}

fn draw_note(frame: &mut canvas::Frame, note: NoteFrame, color: Color) {
    if note.beamed {
        draw_note_pair(frame, note.center, note.size, color);
    } else {
        draw_quaver(frame, note.center, note.size, color);
    }
}

/// The sky's wandering notes this frame. They are transient: each cycle a
/// few notes fade in at a CYCLE-HASHED spot, drift gently upward, and fade
/// back out — never twice in the same place. Windows sit fully inside the
/// cycle (max start 0.73 + 0.22 < 1.0), so a window can never straddle the
/// cycle boundary where its hash would change. A `busy` sky (a black hole
/// or the moon's dream) has none: one drama at a time.
fn wandering_notes(w: f32, h: f32, phase: f32, cycle: u32, busy: bool) -> Vec<NoteFrame> {
    if busy {
        return Vec::new();
    }
    let glyph_scale = scene_glyph_scale(h);
    (0..SKY_WANDER_NOTES)
        .filter_map(|i| {
            let salt = 0x407E + (i as u32) * 4;
            let start = 0.05 + 0.68 * hash01(cycle, salt);
            let t = phase - start;
            if !(0.0..SKY_NOTE_DUR).contains(&t) {
                return None;
            }
            let p = t / SKY_NOTE_DUR;
            let x = (0.06 + 0.88 * hash01(cycle, salt + 1)) * w;
            let y_base = (SKY_BAND_TOP
                + SKY_NOTE_TOP_INSET
                + (SKY_BAND_BOTTOM - SKY_BAND_TOP - SKY_NOTE_TOP_INSET) * hash01(cycle, salt + 2))
                * h;
            Some(NoteFrame {
                center: Point::new(x, y_base - 6.0 * glyph_scale * p),
                size: (7.5 + 2.0 * hash01(cycle, salt + 3)) * glyph_scale,
                alpha: SKY_NOTE_ALPHA * (std::f32::consts::PI * p).sin(),
                beamed: i % 2 == 0,
            })
        })
        .collect()
}

/// The longship's song this frame: a small pool of notes climbing from the
/// mast (`waterline_y` = the water's y under the hull), swaying as they
/// rise, fading in at birth and out near the top. Each rider loops on an
/// integer multiple of the phase; alpha hits zero at both ends of its run,
/// so the cycle wrap (a position jump) never shows. Anchored to the live
/// hull x and dimmed by edge proximity, so the song leaves with the boat
/// instead of cutting at the panel edge.
fn rising_notes(w: f32, h: f32, phase: f32, boat_x: f32, waterline_y: f32) -> Vec<NoteFrame> {
    let edge_fade = (boat_x.min(1.0 - boat_x) / BOAT_EDGE_FADE).clamp(0.0, 1.0);
    if edge_fade <= 0.0 {
        return Vec::new();
    }
    let glyph_scale = scene_glyph_scale(h);
    let boat_cx = boat_x * w;
    let start_y = waterline_y - 0.10 * h;
    riser_params()
        .into_iter()
        .filter_map(|rider| {
            let t = (rider.k as f32 * phase + rider.off).fract();
            let fade_in = (t / RISER_FADE_IN).min(1.0);
            let fade_out = ((1.0 - t) / RISER_FADE_OUT).min(1.0);
            let alpha = RISER_ALPHA * fade_in * fade_out * edge_fade;
            if alpha <= 0.01 {
                return None;
            }
            let sway = RISER_SWAY_PX
                * glyph_scale
                * (std::f32::consts::TAU * (2.0 * t + rider.sway_off)).sin();
            Some(NoteFrame {
                center: Point::new(
                    boat_cx + rider.dx * glyph_scale + sway,
                    start_y - t * RISER_RISE_FRAC * h,
                ),
                size: (7.0 + 4.0 * t) * glyph_scale,
                alpha,
                beamed: rider.beamed,
            })
        })
        .collect()
}

/// A rising bubble this frame: centre, radius (px) and alpha. The shader
/// draws them (`harbour_light`).
#[derive(Debug, Clone, Copy)]
struct BubbleFrame {
    center: Point,
    radius: f32,
    alpha: f32,
}

/// Kelp-root seeps: one slow bubble per frond, rising on its own integer
/// rate — the beds breathe even when the anchor is far. Alpha zero at both
/// ends of each run (the riser contract).
fn kelp_seeps(w: f32, h: f32, phase: f32) -> Vec<BubbleFrame> {
    let glyph_scale = scene_glyph_scale(h);
    kelp_params()
        .into_iter()
        .filter_map(|kelp| {
            let t = (kelp.seep_k as f32 * phase + kelp.seep_off).fract();
            let fade = ((t / 0.20).min(1.0)) * (((1.0 - t) / 0.30).min(1.0));
            if fade <= 0.01 {
                return None;
            }
            let x = kelp.x * w
                + 1.5 * glyph_scale * (std::f32::consts::TAU * (2.0 * t + kelp.sway_off)).sin();
            Some(BubbleFrame {
                center: Point::new(x, 0.975 * h - t * SEEP_RISE_FRAC * h),
                radius: glyph_scale,
                alpha: SEEP_ALPHA * fade,
            })
        })
        .collect()
}

/// The drag aerates the bed: a sparse stream climbs from the trawled
/// anchor (`anchor_x`, from `BoatState::trawled_anchor_x`), swaying as it
/// rises. Each rider loops on an integer multiple of the phase with alpha
/// zero at both ends, and the whole stream dims by the ANCHOR's edge
/// proximity (the risers' rule) so it departs with the sprite instead of
/// cutting at the panel edge — and the wrap seam, where the anchor
/// teleports margins, can't pop a mid-flight bubble.
fn anchor_bubbles(w: f32, h: f32, phase: f32, anchor_x: f32) -> Vec<BubbleFrame> {
    let anchor_fade = (anchor_x.min(1.0 - anchor_x) / BOAT_EDGE_FADE).clamp(0.0, 1.0);
    if anchor_fade <= 0.0 {
        return Vec::new();
    }
    let glyph_scale = scene_glyph_scale(h);
    let base_x = anchor_x * w;
    bubble_params()
        .into_iter()
        .filter_map(|bubble| {
            let t = (bubble.k as f32 * phase + bubble.off).fract();
            let fade_in = (t / BUBBLE_FADE_IN).min(1.0);
            let fade_out = ((1.0 - t) / BUBBLE_FADE_OUT).min(1.0);
            let alpha = BUBBLE_ALPHA * fade_in * fade_out * anchor_fade;
            if alpha <= 0.01 {
                return None;
            }
            let sway = BUBBLE_SWAY_PX
                * glyph_scale
                * (std::f32::consts::TAU * (2.0 * t + bubble.sway_off)).sin();
            // Grow slightly as they rise (decompression) — a small touch
            // that reads "bubble", not "spark".
            let r = (1.0 + 0.6 * t) * bubble.size * glyph_scale * 1.4;
            Some(BubbleFrame {
                center: Point::new(
                    base_x + bubble.dx * glyph_scale + sway,
                    0.955 * h - t * BUBBLE_RISE_FRAC * h,
                ),
                radius: if bubble.ring { r } else { 0.7 * r },
                alpha,
            })
        })
        .collect()
}

/// A kelp frond's spine at `phase`: root → tip as `f` runs 0 → 1, bending
/// progressively (f^1.7) so the base stays planted while the tip travels.
fn kelp_spine(kelp: &KelpParam, w: f32, h: f32, phase: f32) -> impl Fn(f32) -> Point {
    let glyph_scale = scene_glyph_scale(h);
    let root = Point::new(kelp.x * w, 0.985 * h);
    let height = kelp.height * h;
    let sway = (std::f32::consts::TAU * (kelp.sway_k as f32 * phase + kelp.sway_off)).sin();
    let reach = (kelp.lean + KELP_SWAY_PX * sway) * glyph_scale;
    move |f: f32| Point::new(root.x + reach * f.powf(1.7), root.y - height * f)
}

/// Where each frond's glowing beads sit along it, and how bright each is
/// at `phase` (a slow integer-rate blink, out of step bead to bead).
const KELP_BEADS: [f32; 3] = [0.42, 0.66, 0.9];

fn kelp_beads(w: f32, h: f32, phase: f32) -> Vec<(Point, f32)> {
    kelp_params()
        .iter()
        .flat_map(|kelp| {
            let spine = kelp_spine(kelp, w, h, phase);
            KELP_BEADS.iter().enumerate().map(move |(i, &f)| {
                let blink = 0.5
                    + 0.5
                        * (std::f32::consts::TAU
                            * ((kelp.sway_k + 1 + i as u32) as f32 * phase
                                + kelp.seep_off
                                + 0.37 * i as f32))
                            .sin();
                (spine(f), blink * blink)
            })
        })
        .collect()
}

/// The seabed props for the shader's `PROP_*` slots this frame: the rock
/// mounds, the starfish and the sunken shield (fixed), the kelp (swaying,
/// through the same `kelp_spine` reach the beads ride) and the trawled
/// anchor's shadow. The shader lights, shades and half-buries them in the
/// same sand and light as the floor.
pub(crate) fn floor_props(
    w: f32,
    h: f32,
    phase: f32,
    anchor_x: f32,
) -> [[f32; 4]; crate::widgets::harbour_light::MAX_PROPS] {
    use crate::widgets::harbour_light::{
        KELP_SLOTS, MAX_PROPS, PROP_ANCHOR, PROP_KELP, PROP_ROCKS, PROP_SHIELD, PROP_STARFISH,
    };
    let gs = scene_glyph_scale(h);
    let mut props = [[0.0; 4]; MAX_PROPS];
    let dressing = bed_dressing();
    for (i, rock) in dressing.rocks.iter().take(3).enumerate() {
        props[PROP_ROCKS + i] = [
            rock.x,
            14.0 * rock.w * gs / h,
            9.0 * rock.ht * gs / h,
            i as f32,
        ];
    }
    props[PROP_STARFISH] = [
        dressing.star_x,
        1.5 * STARFISH_ARM_PX * dressing.star_size * gs / h,
        dressing.star_rot,
        0.0,
    ];
    props[PROP_SHIELD] = [
        CRATE_X,
        1.25 * CRATE_SIZE_PX * gs / h,
        CRATE_TILT_DEG.to_radians(),
        0.0,
    ];
    for (slot, kelp) in props[PROP_KELP..PROP_KELP + KELP_SLOTS]
        .iter_mut()
        .zip(kelp_params())
    {
        let spine = kelp_spine(&kelp, w, h, phase);
        let root = spine(0.0);
        let tip = spine(1.0);
        *slot = [kelp.x, kelp.height, (tip.x - root.x) / h, 1.0];
    }
    let fade = (anchor_x.min(1.0 - anchor_x) / BOAT_EDGE_FADE).clamp(0.0, 1.0);
    let (_, boat_h) = crate::widgets::boat::boat_pixel_size(w.min(h));
    props[PROP_ANCHOR] = [
        anchor_x,
        0.45 * boat_h * crate::widgets::boat::ANCHOR_HEIGHT_MULTIPLE_OF_BOAT / h,
        0.0,
        fade,
    ];
    props
}

/// A star as the night shader draws it: centre and radius in pixels,
/// peak alpha (twinkle and the black hole's swallow already applied), and
/// whether it is one of the bright sparkles (drawn with cross spikes).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct StarLight {
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    pub alpha: f32,
    pub sparkle: bool,
}

/// The black hole open at `(phase, cycle)` in a `w × h` scene, if any:
/// `(center, s, spin, capture)` for `blackhole_displace` / `_visibility`.
/// Night only, never on a dream cycle (the moon's ritual owns its sky).
fn open_blackhole(w: f32, h: f32, phase: f32, cycle: u32) -> Option<(Point, f32, f32, f32)> {
    if moon_dream_cycle(cycle) || hash01(cycle, BLACKHOLE_SALT) >= BLACKHOLE_CHANCE {
        return None;
    }
    let start = 0.05 + 0.20 * hash01(cycle, BLACKHOLE_SALT ^ 0x9E37);
    let t = phase - start;
    if !(0.0..BLACKHOLE_WINDOW).contains(&t) {
        return None;
    }
    let p = t / BLACKHOLE_WINDOW;
    let (fx, fy) = blackhole_center(cycle);
    let hole = Point::new(fx * w, fy * h);
    // Constrained-axis capture radius: identical in the square modes; a
    // dragged-narrow column shrinks the well instead of reaching off-panel.
    let capture = BLACKHOLE_CAPTURE_FRAC * h.min(w);
    let s = blackhole_s(p);
    // The catch's orbital winding: grows from the plunge's end, scaled by
    // s so the spit-out unwinds it into the outward whip and it is exactly
    // zero at both window ends (s is).
    let spin = s.max(0.0) * BLACKHOLE_HOLD_WHIRL * (p - BLACKHOLE_PLUNGE_END).max(0.0);
    Some((hole, s, spin, capture))
}

/// The night sky's stars for this frame in a `w × h` scene: the fixed
/// constellation (`sky_glyphs`) with each star's twinkle, routed through the
/// black hole when one is open. The hole is drawn by ABSENCE: identity and
/// full visibility at s 0 and beyond the capture radius, so non-event
/// frames, window boundaries and the un-captured sky are the fixed field
/// exactly; nearing the horizon a star's light stops escaping, so it
/// shrinks and dims to nothing, and re-lights crossing back out on the
/// spit.
pub(crate) fn night_stars(w: f32, h: f32, phase: f32, cycle: u32) -> Vec<StarLight> {
    let glyph_scale = scene_glyph_scale(h);
    let blackhole = open_blackhole(w, h, phase, cycle);
    let mut stars = Vec::with_capacity(SKY_STAR_COUNT + SKY_SPARKLE_COUNT + SKY_FAINT_COUNT);
    for glyph in sky_glyphs() {
        let twinkle = 1.0
            - glyph.twinkle_depth
                * (0.5
                    + 0.5
                        * (std::f32::consts::TAU
                            * (glyph.twinkle_k as f32 * phase + glyph.twinkle_off))
                            .sin());
        let home = Point::new(glyph.x * w, glyph.y * h);
        let (center, vis) = match blackhole {
            Some((hole, s, spin, capture)) => {
                let pos = blackhole_displace(home, hole, s, spin, capture);
                let grip = blackhole_grip(home.distance(hole), capture);
                let vis = blackhole_visibility(
                    pos.distance(hole),
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
        // Swallowed light also LOSES SIZE alongside the fade.
        let swallow_scale = 0.3 + 0.7 * vis;
        let (radius, alpha, sparkle) = match glyph.kind {
            SkyGlyphKind::Dot => {
                // Brightness correlates with size: a magnitude hierarchy
                // instead of N identical LEDs.
                let norm = ((glyph.size - 0.7) / 0.6).clamp(0.0, 1.0);
                (
                    glyph.size * glyph_scale * swallow_scale,
                    SKY_STAR_ALPHA * (0.45 + 0.55 * norm),
                    false,
                )
            }
            SkyGlyphKind::Sparkle => (
                1.1 * glyph.size * glyph_scale * swallow_scale,
                SKY_SPARKLE_ALPHA,
                true,
            ),
        };
        stars.push(StarLight {
            x: center.x,
            y: center.y,
            radius,
            alpha: alpha * twinkle * vis,
            sparkle,
        });
    }
    stars
}

/// The night shader's inputs for this frame: both waterlines resampled
/// through the SAME functions the canvas draws with ([`sample_line_height`]
/// over the physics' bars, [`back_swell_height`]), so the lit surface and the
/// hull agree; a scene clock that runs on across phase wraps; and the boat
/// for the glow behind it.
pub(crate) fn sea_light(
    bars: &[f64],
    phase: f32,
    cycle: u32,
    boat: &BoatState,
    music: &crate::widgets::harbour_light::HarbourMusic,
    w: f32,
    h: f32,
) -> crate::widgets::harbour_light::SeaLight {
    use crate::widgets::harbour_light::{
        LINE_SAMPLES, MAX_BUBBLES, MAX_GLOWS, MAX_STARS, SeaLight,
    };
    let x_at = |i: usize| i as f32 / (LINE_SAMPLES - 1) as f32;
    // Stars and moon in the shader's units: x across the width, height
    // above the bottom and radius in panel heights; a sparkle's radius is
    // sent negative.
    let mut stars = [[0.0; 4]; MAX_STARS];
    let mut star_count = 0;
    for (slot, star) in stars.iter_mut().zip(night_stars(w, h, phase, cycle)) {
        let r = star.radius / h;
        *slot = [
            star.x / w,
            1.0 - star.y / h,
            if star.sparkle { -r } else { r },
            star.alpha,
        ];
        star_count += 1;
    }
    let breath = 0.85 + 0.15 * (std::f32::consts::TAU * phase).sin();

    // Glow points: each note's soft halo, the kelp beads, the anchor's glint.
    let glyph_scale = scene_glyph_scale(h);
    let waterline_y = h - sample_line_height(bars, boat.x_ratio, false) * h;
    let busy = moon_dream_cycle(cycle) || hash01(cycle, BLACKHOLE_SALT) < BLACKHOLE_CHANCE;
    let mut glow_list: Vec<[f32; 4]> = wandering_notes(w, h, phase, cycle, busy)
        .into_iter()
        .chain(rising_notes(w, h, phase, boat.x_ratio, waterline_y))
        .map(|n| {
            [
                n.center.x / w,
                1.0 - n.center.y / h,
                0.9 * n.size / h,
                NOTE_GLOW * n.alpha,
            ]
        })
        .collect();
    glow_list.extend(kelp_beads(w, h, phase).into_iter().map(|(p, b)| {
        [
            p.x / w,
            1.0 - p.y / h,
            3.0 * glyph_scale / h,
            -KELP_BEAD_GLOW * b,
        ]
    }));
    let anchor_x = boat.trawled_anchor_x(TRAIL_OFFSET);
    let anchor_fade = (anchor_x.min(1.0 - anchor_x) / BOAT_EDGE_FADE).clamp(0.0, 1.0);
    if anchor_fade > 0.0 {
        let (_, boat_h) = crate::widgets::boat::boat_pixel_size(w.min(h));
        let anchor_h = boat_h * crate::widgets::boat::ANCHOR_HEIGHT_MULTIPLE_OF_BOAT;
        glow_list.push([
            anchor_x,
            0.5 * anchor_h / h,
            0.6 * anchor_h / h,
            ANCHOR_GLINT * anchor_fade,
        ]);
    }
    let mut glows = [[0.0; 4]; MAX_GLOWS];
    let glow_count = glow_list.len().min(MAX_GLOWS);
    glows[..glow_count].copy_from_slice(&glow_list[..glow_count]);

    let mut bubbles = [[0.0; 4]; MAX_BUBBLES];
    let mut bubble_count = 0;
    for (slot, b) in bubbles.iter_mut().zip(
        kelp_seeps(w, h, phase)
            .into_iter()
            .chain(anchor_bubbles(w, h, phase, anchor_x)),
    ) {
        *slot = [b.center.x / w, 1.0 - b.center.y / h, b.radius / h, b.alpha];
        bubble_count += 1;
    }

    SeaLight {
        front: std::array::from_fn(|i| sample_line_height(bars, x_at(i), false)),
        back: std::array::from_fn(|i| back_swell_height(x_at(i) as f64, phase) as f32),
        time: scene_clock_secs(phase, cycle),
        boat: Some((boat.x_ratio, boat.y_ratio)),
        stars,
        star_count,
        moon: (MOON_ALPHA > 0.0).then(|| {
            [
                MOON_X,
                1.0 - MOON_Y,
                MOON_RADIUS_PX * scene_glyph_scale(h) / h,
                breath,
            ]
        }),
        glows,
        glow_count,
        bubbles,
        bubble_count,
        music: *music,
        day: !scene_is_lit(),
        props: floor_props(w, h, phase, anchor_x),
    }
}

/// Seconds of scene time at `(phase, cycle)`: continuous across the phase
/// wrap, so the shader's slow drifts never jump. The cycle count folds every
/// `SCENE_CLOCK_CYCLES` (~23 h at 20 s cycles) to keep f32 sin arguments
/// precise; the one seam per fold is the only discontinuity.
fn scene_clock_secs(phase: f32, cycle: u32) -> f32 {
    ((cycle % SCENE_CLOCK_CYCLES) as f32 + phase) / SEA_DRIFT_HZ
}

const SCENE_CLOCK_CYCLES: u32 = 4096;

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
    music: &'a crate::widgets::harbour_light::HarbourMusic,
    pill: impl Fn() -> Element<'a, M> + 'a,
) -> Element<'a, M> {
    use iced::widget::{column, container, stack};

    // The scene's layer stack at known pixel dimensions. A `Copy` closure
    // (all captures are shared refs / scalars) so both mode arms — and the
    // square arm's NESTED responsive — can each take their own copy.
    let scene_layers = move |w: f32, h: f32| -> Element<'a, M> {
        // The scene's light (sky, sea, seabed; night or day) is the shader's;
        // the canvas draws the furniture over it.
        let backdrop = crate::widgets::harbour_light::light_backdrop(
            sea_light(sea_bars, sea_phase, sea_cycle, boat, music, w, h),
            w,
            h,
        );

        let sea = canvas::Canvas::new(SeaCanvas {
            bars: sea_bars,
            phase: sea_phase,
            cycle: sea_cycle,
            boat_x: boat.x_ratio,
        })
        .width(Length::Fixed(w))
        .height(Length::Fixed(h));

        // The longship, trawling: full opacity (it's the panel's content,
        // not an overlay dimmed against art), mirror off (the harbour sea
        // has no lower reflection), anchor trailed on the seabed.
        let boat_el = boat_overlay::<M>(
            boat,
            w,
            h,
            w.min(h),
            1.0,
            LineGeometry::default(),
            Some(TRAIL_OFFSET),
        );

        let mut layers = stack![backdrop, sea];
        // The moon's (and by day the sun's) disc is the shader's; during a
        // moon dream the face's marks arrive as an Svg over it, themed via
        // the shared LOGO tokens and cached on BoatState beside the boat /
        // anchor handles (same theme-generation invalidation; warmed by the
        // tick, with a rebuild-on-miss fallback). The handle is the veiled
        // document for this frame's quantized key (the SAME
        // `moon_dream_veil_key(phase, cycle)` the canvas verses and the
        // tick's cache-warm read — one clock, no drift); between dreams the
        // Svg leaves the stack.
        let veil = moon_dream_veil_key(sea_phase, sea_cycle);
        if MOON_ALPHA > 0.0 && veil != crate::embedded_svg::MOON_VEIL_BARE {
            let moon_r = MOON_RADIUS_PX * scene_glyph_scale(h);
            let handle = boat.cached_moon_veil_handle(veil).unwrap_or_else(|| {
                iced::widget::svg::Handle::from_memory(
                    crate::embedded_svg::themed_moon_for_scene(veil).into_bytes(),
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
                )
                .boxed(),
            );
        }
        layers.push(boat_el).boxed()
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
        .boxed()
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
            .boxed();
            panel
        })
        .width(Length::Shrink)
        .height(Length::Shrink)
        .boxed()
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

        // Ink and starlight from the dark-variant visualizer colors — the
        // same mode-stable family the boat outline, rope, and anchor are
        // themed with, so the whole doodad reads as one system.
        let viz = crate::theme::get_visualizer_colors_dark();
        let crest = parse_hex_color(&viz.border_color).unwrap_or(Color::from_rgb(0.5, 0.5, 0.5));
        // Starlight: the PEAK gradient's lightest stop — the visualizer's
        // bright sparkle-top. Chosen by luminance, not position, because a
        // theme may order its peaks dark-to-light (Svalbard does). The border
        // color the rope/boat use is a DARK stroke in most themes (Svalbard:
        // #111817), which vanishes on the dark sky; the water gradient is
        // mid-tone. Peaks read as stars.
        let starlight = crate::theme::brightest_peak_color(&viz)
            .or_else(|| {
                viz.bar_gradient_colors
                    .last()
                    .and_then(|c| parse_hex_color(c))
            })
            .unwrap_or(Color::from_rgb(0.9, 0.92, 0.92));

        let phase = self.phase;
        let bars = self.bars;
        let front_y = move |x: f32| h - sample_line_height(bars, x / w, false) * h;

        let day = crate::theme::is_light_mode();
        let night = crate::widgets::harbour_light::NightInk::from_theme();
        let note_color = |a: f32| {
            if day {
                Color {
                    a: a * viz.border_opacity,
                    ..crest
                }
            } else {
                Color {
                    a: a * NIGHT_NOTE_GAIN,
                    ..night.starlight
                }
            }
        };
        // Underwater furniture: ink by day; by night a dark silhouette
        // whose edges catch the aurora (`rim`).
        let bed_ink = |a_day: f32| {
            if day {
                Color {
                    a: a_day * viz.border_opacity,
                    ..crest
                }
            } else {
                Color {
                    a: NIGHT_SILHOUETTE_ALPHA,
                    ..night.silhouette
                }
            }
        };
        let rim_light = |a_day: f32| {
            if day {
                Color {
                    a: a_day,
                    ..starlight
                }
            } else {
                Color {
                    a: NIGHT_RIM_ALPHA,
                    ..night.rim
                }
            }
        };
        // ── Solid block: the sky's inhabitants ──────────────────────────
        // NIGHT: star dots, sparkle crosses (arms deferred to gradient
        // block B where they taper via gradient strokes). DAY: the
        // starlight field would be invisible on a light background, so
        // seagulls glide in its place — each one loops on integer
        // crossings per cycle over an off-panel margin (no edge pop),
        // bobbing and beating its wings at integer rates.
        let glyph_scale = scene_glyph_scale(h);
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
                let burst = 0.5
                    + 0.5 * (std::f32::consts::TAU * (GULL_BURST_K * phase + gull.flap_off)).sin();
                let beat =
                    (std::f32::consts::TAU * (GULL_BEAT_K * phase + 3.0 * gull.flap_off)).sin();
                let flap = GULL_GLIDE + GULL_BEAT_AMP * smoothstep(0.4, 0.8, burst) * beat;
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
        // The stars themselves (and the hole's pull on them) are the night
        // shader's: `night_stars` feeds `harbour_light`.

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
        for note in wandering_notes(w, h, phase, self.cycle, blackhole_cycle || dream_cycle) {
            draw_note(&mut frame, note, note_color(note.alpha));
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

        // ── The moon's exhale ─────────────────────────────────────────────
        // The moon's (and by day the sun's) disc and light are the shader's;
        // the canvas adds the moon's rare exhale: some cycles a soft
        // two-stroke ring detaches at the halo's shoulder, expands past the
        // rim, and dissolves — wide faint stroke under a narrow brighter one
        // reads as one soft band, not a crisp vector circle.
        if MOON_ALPHA > 0.0 && !day && hash01(self.cycle, MOON_PULSE_SALT) < MOON_PULSE_CHANCE {
            let m = MOON_RADIUS_PX * glyph_scale;
            let mc = Point::new(MOON_X * w, MOON_Y * h);
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

        // ── Rising notes — the longship sings ───────────────────────────
        // A small pool of note glyphs climbs from the mast, swaying as
        // they rise, fading in at birth and out near the top. Each rider
        // loops on an integer multiple of the phase; alpha hits zero at
        // both ends of its run, so the cycle wrap (a position jump) never
        // shows. Anchored to the live hull x, and DIMMED by edge proximity
        // (boat_edge_fade) so the song fades out with the departing sprite
        // instead of cutting in one frame at the panel edge while the hull
        // is still half on-screen.
        for note in rising_notes(w, h, phase, self.boat_x, front_y(self.boat_x * w)) {
            draw_note(&mut frame, note, note_color(note.alpha));
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
            let fish_color = {
                let c = bed_ink(FISH_ALPHA);
                Color { a: c.a * fade, ..c }
            };
            frame.with_save(|frame| {
                frame.translate(iced::Vector::new(x, y));
                frame.rotate(angle);
                // Body: a little teardrop; tail: a notched triangle —
                // the shared silhouette, nose along the rotated +x.
                let wag = FISH_WAG_RAD * (std::f32::consts::TAU * FISH_WAG_K_LEAP * phase).sin();
                fill_fish_silhouette(frame, l, 1.0, wag, fish_color);
            });
        }

        // ── The seabed ─────────────────────────────────────────────────
        // The rocks, starfish, sunken shield and kelp are the shader's
        // (`floor_props` → `harbour_light`), drawn in the floor's own sand
        // and light with contact shadows; their bubbles and the kelp's
        // glowing beads ride the shader's lists too.

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
                let ink = {
                    let c = bed_ink(SERPENT_ALPHA);
                    Color { a: c.a * env, ..c }
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
                            .with_color({
                                let c = rim_light(SERPENT_RIM_ALPHA);
                                Color { a: c.a * env, ..c }
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
            let ink = bed_ink(SCHOOL_ALPHA);
            frame.with_save(|frame| {
                frame.translate(iced::Vector::new(fx, fy));
                let wag = FISH_WAG_RAD
                    * (std::f32::consts::TAU * (FISH_WAG_K * phase + fish.bob_off)).sin();
                fill_fish_silhouette(frame, l, dir, wag, ink);
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
                            .with_color(rim_light(SCHOOL_RIM_ALPHA))
                            .with_width(0.9)
                            .with_line_cap(canvas::LineCap::Round),
                    );
                }
            });
        }

        let boat_edge_fade = (self.boat_x.min(1.0 - self.boat_x) / BOAT_EDGE_FADE).clamp(0.0, 1.0);
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
