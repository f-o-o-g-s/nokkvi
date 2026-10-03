//! NaN-safe range clamps for values read from hand-editable config files.

/// NaN-safe clamp: a hand-written `nan` in config.toml snaps to `lo` instead
/// of propagating. NaN must never survive a `validate()` — it defeats
/// PartialEq change gates (NaN != NaN, so every comparison reads as a change)
/// and panics `clamp` when the NaN field is itself used as a bound
/// (`bar_width_max.clamp(bar_width_min, ..)`).
pub(crate) fn finite_clamp32(v: f32, lo: f32, hi: f32) -> f32 {
    if v.is_nan() { lo } else { v.clamp(lo, hi) }
}

/// See [`finite_clamp32`].
pub(crate) fn finite_clamp64(v: f64, lo: f64, hi: f64) -> f64 {
    if v.is_nan() { lo } else { v.clamp(lo, hi) }
}
