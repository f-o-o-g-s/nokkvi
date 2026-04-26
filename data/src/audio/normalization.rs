//! Volume normalization config — resolved per-stream before being handed
//! to the rodio chain.
//!
//! `AudioRenderer` resolves the user's [`VolumeNormalizationMode`],
//! `replay_gain_*` settings, and the current track's optional
//! [`ReplayGain`] tags into a [`NormalizationConfig`] which the
//! [`RodioOutput`](crate::audio::rodio_output::RodioOutput) consumes
//! directly. Keeping the resolution upstream of `RodioOutput` means the
//! audio layer doesn't need to know about modes, fallbacks, or peak
//! tags — it just sees one of three terminal shapes.
//!
//! [`VolumeNormalizationMode`]: crate::types::player_settings::VolumeNormalizationMode
//! [`ReplayGain`]: crate::types::song::ReplayGain

use crate::types::{
    player_settings::VolumeNormalizationMode,
    song::ReplayGain,
};

/// Resolved per-stream normalization decision.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NormalizationConfig {
    /// No normalization. Decoded audio passes through unchanged
    /// (limiter still applies — the limiter is part of every chain).
    Off,
    /// Real-time AGC with the given target level (rodio's
    /// `AutomaticGainControlSettings::target_level`).
    Agc { target_level: f32 },
    /// Static linear gain factor. Pre-amp and peak-aware clamping have
    /// already been baked in upstream — this is the final scalar.
    Static(f32),
}

impl NormalizationConfig {
    pub fn off() -> Self {
        Self::Off
    }

    pub fn agc(target_level: f32) -> Self {
        Self::Agc { target_level }
    }

    pub fn static_gain(linear: f32) -> Self {
        Self::Static(linear)
    }
}

/// Inputs to the per-track normalization resolver. Cheaper to pass a
/// borrowed bundle than a long argument list.
#[derive(Debug, Clone, Copy)]
pub struct NormalizationContext<'a> {
    pub mode: VolumeNormalizationMode,
    pub agc_target_level: f32,
    pub replay_gain_preamp_db: f32,
    pub replay_gain_fallback_db: f32,
    pub replay_gain_fallback_to_agc: bool,
    pub replay_gain_prevent_clipping: bool,
    pub replay_gain: Option<&'a ReplayGain>,
}

/// Resolve mode + settings + per-track tags into a final
/// [`NormalizationConfig`].
///
/// Behavior:
///
/// 1. `Off` and `Agc` are returned directly; `replay_gain_*` is ignored.
/// 2. `ReplayGain*` modes select gain and peak from the requested scope
///    (`track_*` for `ReplayGainTrack`, `album_*` for `ReplayGainAlbum`)
///    with silent cross-fallback to the other scope when the requested
///    field is missing.
/// 3. If neither scope has a gain value:
///    - `replay_gain_fallback_to_agc == true` → AGC kicks in.
///    - otherwise → `replay_gain_fallback_db` is used (default 0.0 = unity).
/// 4. The pre-amp dB is added to the resolved gain.
/// 5. The result is converted to linear; if `replay_gain_prevent_clipping`
///    is true and a peak is available, the gain is clamped so
///    `peak * gain <= 1.0`.
pub fn resolve_normalization(ctx: NormalizationContext<'_>) -> NormalizationConfig {
    use VolumeNormalizationMode::*;

    if !ctx.mode.is_replay_gain() {
        return match ctx.mode {
            Off => NormalizationConfig::off(),
            Agc => NormalizationConfig::agc(ctx.agc_target_level),
            // `is_replay_gain` was false so this is unreachable in practice —
            // map to Off as a safe default rather than panic.
            ReplayGainTrack | ReplayGainAlbum => NormalizationConfig::off(),
        };
    }

    let prefer_album = ctx.mode.prefers_album();
    let (gain_db, peak) = match ctx.replay_gain {
        Some(r) if prefer_album => (
            r.album_gain.or(r.track_gain),
            r.album_peak.or(r.track_peak),
        ),
        Some(r) => (
            r.track_gain.or(r.album_gain),
            r.track_peak.or(r.album_peak),
        ),
        None => (None, None),
    };

    let resolved_db = match gain_db {
        Some(db) => db as f32,
        None if ctx.replay_gain_fallback_to_agc => {
            return NormalizationConfig::agc(ctx.agc_target_level);
        }
        None => ctx.replay_gain_fallback_db,
    };

    let total_db = resolved_db + ctx.replay_gain_preamp_db;
    let linear = 10f32.powf(total_db / 20.0);

    let effective = match (ctx.replay_gain_prevent_clipping, peak) {
        (true, Some(p)) if p > 0.0 => linear.min(1.0 / p as f32),
        _ => linear,
    };

    NormalizationConfig::static_gain(effective)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_default<'a>(
        mode: VolumeNormalizationMode,
        rg: Option<&'a ReplayGain>,
    ) -> NormalizationContext<'a> {
        NormalizationContext {
            mode,
            agc_target_level: 1.0,
            replay_gain_preamp_db: 0.0,
            replay_gain_fallback_db: 0.0,
            replay_gain_fallback_to_agc: false,
            replay_gain_prevent_clipping: true,
            replay_gain: rg,
        }
    }

    fn rg(track: Option<f64>, album: Option<f64>, t_peak: Option<f64>, a_peak: Option<f64>) -> ReplayGain {
        ReplayGain {
            track_gain: track,
            album_gain: album,
            track_peak: t_peak,
            album_peak: a_peak,
        }
    }

    #[test]
    fn off_mode_ignores_replay_gain() {
        let r = rg(Some(-6.0), Some(-9.0), Some(0.95), Some(0.9));
        let cfg = resolve_normalization(ctx_default(VolumeNormalizationMode::Off, Some(&r)));
        assert_eq!(cfg, NormalizationConfig::Off);
    }

    #[test]
    fn agc_mode_returns_target_level() {
        let cfg = resolve_normalization(ctx_default(VolumeNormalizationMode::Agc, None));
        assert_eq!(cfg, NormalizationConfig::Agc { target_level: 1.0 });
    }

    #[test]
    fn track_mode_uses_track_gain_first() {
        let r = rg(Some(-6.0), Some(-9.0), None, None);
        let cfg = resolve_normalization(ctx_default(
            VolumeNormalizationMode::ReplayGainTrack,
            Some(&r),
        ));
        let expected = 10f32.powf(-6.0 / 20.0);
        assert!(matches!(cfg, NormalizationConfig::Static(g) if (g - expected).abs() < 1e-6));
    }

    #[test]
    fn album_mode_uses_album_gain_first() {
        let r = rg(Some(-6.0), Some(-9.0), None, None);
        let cfg = resolve_normalization(ctx_default(
            VolumeNormalizationMode::ReplayGainAlbum,
            Some(&r),
        ));
        let expected = 10f32.powf(-9.0 / 20.0);
        assert!(matches!(cfg, NormalizationConfig::Static(g) if (g - expected).abs() < 1e-6));
    }

    #[test]
    fn track_mode_falls_back_to_album_gain() {
        let r = rg(None, Some(-9.0), None, None);
        let cfg = resolve_normalization(ctx_default(
            VolumeNormalizationMode::ReplayGainTrack,
            Some(&r),
        ));
        let expected = 10f32.powf(-9.0 / 20.0);
        assert!(matches!(cfg, NormalizationConfig::Static(g) if (g - expected).abs() < 1e-6));
    }

    #[test]
    fn album_mode_falls_back_to_track_gain() {
        let r = rg(Some(-6.0), None, None, None);
        let cfg = resolve_normalization(ctx_default(
            VolumeNormalizationMode::ReplayGainAlbum,
            Some(&r),
        ));
        let expected = 10f32.powf(-6.0 / 20.0);
        assert!(matches!(cfg, NormalizationConfig::Static(g) if (g - expected).abs() < 1e-6));
    }

    #[test]
    fn untagged_track_uses_fallback_db() {
        let mut ctx = ctx_default(VolumeNormalizationMode::ReplayGainTrack, None);
        ctx.replay_gain_fallback_db = -3.0;
        let cfg = resolve_normalization(ctx);
        let expected = 10f32.powf(-3.0 / 20.0);
        assert!(matches!(cfg, NormalizationConfig::Static(g) if (g - expected).abs() < 1e-6));
    }

    #[test]
    fn untagged_track_falls_through_to_agc_when_enabled() {
        let mut ctx = ctx_default(VolumeNormalizationMode::ReplayGainTrack, None);
        ctx.replay_gain_fallback_to_agc = true;
        ctx.agc_target_level = 1.4;
        let cfg = resolve_normalization(ctx);
        assert_eq!(cfg, NormalizationConfig::Agc { target_level: 1.4 });
    }

    #[test]
    fn preamp_adds_to_resolved_db() {
        let r = rg(Some(-6.0), None, None, None);
        let mut ctx = ctx_default(VolumeNormalizationMode::ReplayGainTrack, Some(&r));
        ctx.replay_gain_preamp_db = 6.0;
        let cfg = resolve_normalization(ctx);
        let expected = 10f32.powf((-6.0 + 6.0) / 20.0);
        assert!(matches!(cfg, NormalizationConfig::Static(g) if (g - expected).abs() < 1e-6));
    }

    #[test]
    fn peak_clamping_caps_amplification() {
        // +6 dB on a track with peak=0.95 would clip; clamp to 1/peak.
        let r = rg(Some(6.0), None, Some(0.95), None);
        let ctx = ctx_default(VolumeNormalizationMode::ReplayGainTrack, Some(&r));
        let cfg = resolve_normalization(ctx);
        let expected = (1.0_f32 / 0.95).min(10f32.powf(6.0 / 20.0));
        assert!(matches!(cfg, NormalizationConfig::Static(g) if (g - expected).abs() < 1e-6));
    }

    #[test]
    fn peak_clamping_disabled_lets_overshoot_through() {
        let r = rg(Some(6.0), None, Some(0.95), None);
        let mut ctx = ctx_default(VolumeNormalizationMode::ReplayGainTrack, Some(&r));
        ctx.replay_gain_prevent_clipping = false;
        let cfg = resolve_normalization(ctx);
        let expected = 10f32.powf(6.0 / 20.0);
        assert!(matches!(cfg, NormalizationConfig::Static(g) if (g - expected).abs() < 1e-6));
    }

    #[test]
    fn track_peak_falls_back_to_album_peak() {
        let r = rg(Some(6.0), None, None, Some(0.5));
        let ctx = ctx_default(VolumeNormalizationMode::ReplayGainTrack, Some(&r));
        let cfg = resolve_normalization(ctx);
        // Clamped by 1/0.5 = 2.0 (lower than 10^(6/20) = 1.995... ).
        // 10^(6/20) ≈ 1.995, 1/0.5 = 2.0; min picks the gain (1.995).
        let raw = 10f32.powf(6.0 / 20.0);
        let expected = raw.min(2.0);
        assert!(matches!(cfg, NormalizationConfig::Static(g) if (g - expected).abs() < 1e-6));
    }

    #[test]
    fn no_peak_means_no_clamp() {
        let r = rg(Some(6.0), None, None, None);
        let ctx = ctx_default(VolumeNormalizationMode::ReplayGainTrack, Some(&r));
        let cfg = resolve_normalization(ctx);
        let expected = 10f32.powf(6.0 / 20.0);
        assert!(matches!(cfg, NormalizationConfig::Static(g) if (g - expected).abs() < 1e-6));
    }

    #[test]
    fn untagged_track_with_zero_fallback_is_unity() {
        let ctx = ctx_default(VolumeNormalizationMode::ReplayGainTrack, None);
        let cfg = resolve_normalization(ctx);
        assert!(matches!(cfg, NormalizationConfig::Static(g) if (g - 1.0).abs() < 1e-6));
    }
}
