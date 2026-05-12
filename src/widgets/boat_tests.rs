#![allow(dead_code)]
use std::time::Duration;

use super::{
    boat_physics::{
        ANCHOR_INTERVAL_MAX_SECS, ANCHOR_INTERVAL_MIN_SECS, BPM_SCALE_MAX, BPM_SCALE_MIN,
        LONG_ONSET_AMP, LONG_ONSET_FLOOR, MAX_ANCHOR_SWAY, MAX_TILT, MAX_X_V, MIN_SAILING_VELOCITY,
        ONSET_AMP, REFERENCE_BPM, SLOPE_GATE_FLOOR, TACK_INTERVAL_MAX_SECS, TACK_INTERVAL_MIN_SECS,
        TACK_RAMP_SECS, pick_facing, sample_line_height,
    },
    *,
};

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

    let id_before = state.cache_handle_for(0.0, 1.0, false).id();

    // Flip light/dark — `themed_boat_svg()` now substitutes different
    // colors, so a freshly-built handle has different bytes (and id).
    crate::theme::set_light_mode(!initial_mode);

    let id_after = state.cache_handle_for(0.0, 1.0, false).id();

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
    let id1 = state.cache_handle_for(0.0, 1.0, false).id();
    let id2 = state.cache_handle_for(0.0, 1.0, false).id();
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
    let upright_right = state.cache_handle_for(0.0, 1.0, false).id();
    let upright_left = state.cache_handle_for(0.0, -1.0, false).id();
    let tilted_right = state.cache_handle_for(0.15, 1.0, false).id();
    let tilted_left = state.cache_handle_for(0.15, -1.0, false).id();

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
    let id_a = state.cache_handle_for(0.000, 1.0, false).id();
    let id_b = state.cache_handle_for(0.001, 1.0, false).id();
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
        state.cached_handle_for(0.0, 1.0, false).is_none(),
        "empty cache must miss for any orientation"
    );
    let primed = state.cache_handle_for(0.0, 1.0, false).id();
    let looked_up = state
        .cached_handle_for(0.0, 1.0, false)
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

// --- inverted-on-wrap (mirrored line mode affordance) ---------------------

/// A rightward wrap must toggle `state.inverted`. The boat starts near
/// the right wrap edge with rightward momentum; a single big step
/// pushes it past the margin and into the wrap path. The flag toggles
/// on every wrap unconditionally — `step()` doesn't know about the
/// mirror flag, so the render path is responsible for ignoring the
/// flag outside mirrored line mode.
#[test]
fn step_wrap_toggles_inverted_on_right_edge() {
    let bars = vec![0.5; 16];
    let mut state = BoatState {
        x_ratio: 1.04,
        x_velocity: MAX_X_V,
        facing: 1.0,
        x_wrap_margin: 0.05,
        rng_state: 0x12345,
        secs_until_next_tack: 1e6,
        secs_until_next_anchor: 1e6,
        inverted: false,
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
        state.inverted,
        "right-edge wrap must flip inverted from false to true \
         (got inverted = {}, x_ratio = {}, x_velocity = {})",
        state.inverted, state.x_ratio, state.x_velocity
    );
    // Sanity: a wrap actually fired (boat re-entered the left side).
    assert!(
        state.x_ratio < 0.5,
        "precondition: boat must have wrapped through the right edge \
         to land near 0 (got x_ratio = {})",
        state.x_ratio
    );
}

/// A leftward wrap must toggle `state.inverted`. Mirror of the
/// right-edge case: boat near the left wrap edge with leftward
/// momentum; a single big step pushes it past the margin and the
/// flag flips.
#[test]
fn step_wrap_toggles_inverted_on_left_edge() {
    let bars = vec![0.5; 16];
    let mut state = BoatState {
        x_ratio: -0.04,
        x_velocity: -MAX_X_V,
        facing: -1.0,
        x_wrap_margin: 0.05,
        rng_state: 0x12345,
        secs_until_next_tack: 1e6,
        secs_until_next_anchor: 1e6,
        inverted: false,
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
        state.inverted,
        "left-edge wrap must flip inverted from false to true \
         (got inverted = {}, x_ratio = {}, x_velocity = {})",
        state.inverted, state.x_ratio, state.x_velocity
    );
    assert!(
        state.x_ratio > 0.5,
        "precondition: boat must have wrapped through the left edge \
         to land near 1 (got x_ratio = {})",
        state.x_ratio
    );
}

/// Two consecutive wraps must toggle `inverted` back to its starting
/// state. Confirms the toggle is symmetric rather than a latch — every
/// tack-and-wrap cycle alternates the boat between upper and lower
/// wave in mirrored line mode.
#[test]
fn step_two_wraps_restore_inverted_to_starting_value() {
    let bars = vec![0.5; 16];
    let mut state = BoatState {
        x_ratio: 1.04,
        x_velocity: MAX_X_V,
        facing: 1.0,
        x_wrap_margin: 0.05,
        rng_state: 0x12345,
        secs_until_next_tack: 1e6,
        secs_until_next_anchor: 1e6,
        inverted: false,
        ..Default::default()
    };
    step(
        &mut state,
        Duration::from_millis(500),
        &bars,
        false,
        MusicSignals::default(),
    );
    assert!(state.inverted, "first wrap should have flipped to true");
    // Push past the left wrap edge for the second wrap.
    state.x_ratio = -0.04;
    state.x_velocity = -MAX_X_V;
    state.facing = -1.0;
    step(
        &mut state,
        Duration::from_millis(500),
        &bars,
        false,
        MusicSignals::default(),
    );
    assert!(
        !state.inverted,
        "second wrap must flip inverted back to false (got inverted = {})",
        state.inverted
    );
}

/// While `state.inverted` is true, the anchor-firing countdown must
/// not fire even when all other preconditions are met (safe-zone
/// position, countdown elapsed, boat in the visible area). V1 punts
/// on anchor + rope geometry for inverted boats; the rendering would
/// have the rope reaching out from an upside-down sprite to a
/// floor-bound anchor, which doesn't read.
#[test]
fn step_anchor_does_not_fire_while_inverted() {
    let bars = vec![0.5; 16];
    let mut state = BoatState {
        x_ratio: 0.5, // mid-canvas, well inside the safe zone
        facing: 1.0,
        rng_state: 0x12345,
        secs_until_next_tack: 1e6,
        // Pre-arm: would fire next tick if the inverted gate didn't hold.
        secs_until_next_anchor: 0.001,
        inverted: true,
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
        "anchor must NOT fire while inverted (got anchor_remaining_secs = {})",
        state.anchor_remaining_secs
    );
}

/// A wrap that flips `inverted` from false to true while an anchor is
/// active must lift the anchor on the same tick. The rope physics is
/// not retrofit for inverted boats in V1, so an anchor must never
/// survive the toggle. The countdown to the next anchor is reseeded
/// so the boat doesn't immediately drop again on the next tick.
#[test]
fn step_wrap_lifts_active_anchor_when_toggling_inverted() {
    let bars = vec![0.5; 16];
    let mut state = BoatState {
        x_ratio: 1.04,
        x_velocity: MAX_X_V,
        facing: 1.0,
        x_wrap_margin: 0.05,
        rng_state: 0x12345,
        secs_until_next_tack: 1e6,
        // Active anchor with plenty of time remaining; the lift logic
        // must zero it out on the wrap.
        anchor_remaining_secs: 8.0,
        // Pre-set so we can check the reseed lands in [MIN, MAX].
        secs_until_next_anchor: 0.0,
        inverted: false,
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
        state.inverted,
        "precondition: the wrap must have toggled inverted to true \
         (got inverted = {}, x_ratio = {})",
        state.inverted, state.x_ratio
    );
    assert_eq!(
        state.anchor_remaining_secs, 0.0,
        "active anchor must be lifted on the wrap that toggles \
         inverted to true (got anchor_remaining_secs = {})",
        state.anchor_remaining_secs
    );
    assert!(
        (ANCHOR_INTERVAL_MIN_SECS..=ANCHOR_INTERVAL_MAX_SECS)
            .contains(&state.secs_until_next_anchor),
        "next-anchor countdown must be reseeded into [{}, {}] after \
         the lift (got secs_until_next_anchor = {})",
        ANCHOR_INTERVAL_MIN_SECS,
        ANCHOR_INTERVAL_MAX_SECS,
        state.secs_until_next_anchor,
    );
}
