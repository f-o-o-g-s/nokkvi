//! Crossfade gain curves — the pure math behind [`CrossfadeCurve`].
//!
//! One function, no state: [`fade_gains`] maps a curve choice and a fade
//! progress to the (outgoing, incoming) gain pair the renderer writes to the
//! two streams' `fade_coeff` atomics (applied LINEARLY in
//! `StreamingSource::next` — never re-curved through the perceptual volume
//! taper; see the M1 fade/volume split).
//!
//! Curve contracts (pinned by the inline tests):
//! - `EqualPower` — `cos`/`sin` first power: POWER sum `out² + in² = 1`
//!   everywhere, so uncorrelated material (two different songs) holds
//!   constant loudness through the blend.
//! - `ConstantGain` — `cos²`/`sin²`: AMPLITUDE sum `out + in = 1` (constant
//!   amplitude; equal-power only for correlated content, ~3 dB midpoint dip
//!   on uncorrelated material).
//! - `Linear` — straight-line `1−p` / `p`: amplitude sum 1, harder ends.
//! - All three curves cross at `p = 0.5` — the renderer's visualizer-handoff
//!   point stays curve-independent.
//! - The FULL-GAIN endpoints are exactly `1.0` (fade-out at `p = 0`, fade-in
//!   at `p = 1`): the bit-perfect `UNITY_SNAP_EPSILON` snap in
//!   `StreamingSource::next` keys off the coefficient reaching unity, so a
//!   promoted bit-perfect stream settles back to raw passthrough.

use crate::types::player_settings::CrossfadeCurve;

/// Gain pair `(fade_out, fade_in)` for fade progress `p ∈ [0, 1]`.
///
/// `p = 0` is the start of the blend (outgoing at full gain, incoming
/// silent); `p = 1` is the end (incoming at full gain). Callers pass the
/// pause-corrected, clamped progress from `crossfade_progress`.
pub fn fade_gains(curve: CrossfadeCurve, p: f64) -> (f64, f64) {
    use std::f64::consts::FRAC_PI_2;
    match curve {
        CrossfadeCurve::EqualPower => ((p * FRAC_PI_2).cos(), (p * FRAC_PI_2).sin()),
        CrossfadeCurve::ConstantGain => {
            ((p * FRAC_PI_2).cos().powi(2), (p * FRAC_PI_2).sin().powi(2))
        }
        CrossfadeCurve::Linear => (1.0 - p, p),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CURVES: [CrossfadeCurve; 3] = [
        CrossfadeCurve::EqualPower,
        CrossfadeCurve::ConstantGain,
        CrossfadeCurve::Linear,
    ];

    /// Sweep helper: progress samples across the full fade including both
    /// endpoints.
    fn sweep() -> impl Iterator<Item = f64> {
        (0..=100).map(|i| f64::from(i) / 100.0)
    }

    /// EqualPower holds the POWER sum at 1 across the whole fade — the
    /// no-midpoint-dip property for uncorrelated material.
    #[test]
    fn equal_power_power_sum_is_unity() {
        for p in sweep() {
            let (out, inc) = fade_gains(CrossfadeCurve::EqualPower, p);
            let power = out * out + inc * inc;
            assert!(
                (power - 1.0).abs() < 1e-12,
                "EqualPower power sum at p={p}: {power}"
            );
        }
    }

    /// ConstantGain holds the AMPLITUDE sum at 1 across the whole fade — the
    /// historical cos²/sin² constant-amplitude property.
    #[test]
    fn constant_gain_amplitude_sum_is_unity() {
        for p in sweep() {
            let (out, inc) = fade_gains(CrossfadeCurve::ConstantGain, p);
            assert!(
                (out + inc - 1.0).abs() < 1e-12,
                "ConstantGain amplitude sum at p={p}: {}",
                out + inc
            );
        }
    }

    /// Linear also holds the amplitude sum at 1 (1−p + p).
    #[test]
    fn linear_amplitude_sum_is_unity() {
        for p in sweep() {
            let (out, inc) = fade_gains(CrossfadeCurve::Linear, p);
            assert!(
                (out + inc - 1.0).abs() < 1e-12,
                "Linear amplitude sum at p={p}: {}",
                out + inc
            );
        }
    }

    /// All three curves cross (out == in) at p = 0.5 — the renderer's
    /// visualizer handoff at progress ≥ 0.5 stays curve-independent.
    #[test]
    fn all_curves_cross_at_midpoint() {
        for curve in CURVES {
            let (out, inc) = fade_gains(curve, 0.5);
            assert!(
                (out - inc).abs() < 1e-12,
                "{curve:?} gains must cross at p=0.5: out={out}, in={inc}"
            );
        }
    }

    /// EqualPower's midpoint gains are 1/√2 ≈ 0.7071 each (power halves,
    /// amplitude does not) — this is the curve's whole point.
    #[test]
    fn equal_power_midpoint_is_frac_1_sqrt_2() {
        let (out, inc) = fade_gains(CrossfadeCurve::EqualPower, 0.5);
        let expected = std::f64::consts::FRAC_1_SQRT_2;
        assert!((out - expected).abs() < 1e-12, "out={out}");
        assert!((inc - expected).abs() < 1e-12, "in={inc}");
    }

    /// The FULL-GAIN endpoints are EXACTLY 1.0 for every curve — fade-out at
    /// p=0 (`cos(0)`) and fade-in at p=1 (`sin(π/2)`, which rounds to exactly
    /// 1.0 in f64). The bit-perfect unity snap depends on the coefficient
    /// actually reaching unity, so these assert `==`, not an epsilon.
    #[test]
    fn full_gain_endpoints_are_exactly_unity() {
        for curve in CURVES {
            let (out_at_start, _) = fade_gains(curve, 0.0);
            let (_, in_at_end) = fade_gains(curve, 1.0);
            assert_eq!(out_at_start, 1.0, "{curve:?} fade-out at p=0");
            assert_eq!(in_at_end, 1.0, "{curve:?} fade-in at p=1");
        }
    }

    /// The going-to-ZERO endpoints approach 0 within epsilon — deliberately
    /// NOT `== 0.0`: `cos(π/2)` in f64 is ~6.12e-17, not zero.
    #[test]
    fn silent_endpoints_are_within_epsilon_of_zero() {
        for curve in CURVES {
            let (_, in_at_start) = fade_gains(curve, 0.0);
            let (out_at_end, _) = fade_gains(curve, 1.0);
            assert!(
                in_at_start.abs() < 1e-6,
                "{curve:?} fade-in at p=0: {in_at_start}"
            );
            assert!(
                out_at_end.abs() < 1e-6,
                "{curve:?} fade-out at p=1: {out_at_end}"
            );
        }
    }
}
