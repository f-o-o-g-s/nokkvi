//! The equalizer's band model: one filter of the band list, its limits, and
//! the graphic layout (ten peaks at the ISO centres).
//!
//! Ported from Pawse's `crates/equalizer/src/band.rs`.
//!
//! MIT License
//!
//! Copyright (c) 2026 Aleksandr Popov
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy
//! of this software and associated documentation files (the "Software"), to deal
//! in the Software without restriction, including without limitation the rights
//! to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
//! copies of the Software, and to permit persons to whom the Software is
//! furnished to do so, subject to the following conditions:
//!
//! The above copyright notice and this permission notice shall be included in all
//! copies or substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//! IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//! FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
//! AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//! LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
//! OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
//! SOFTWARE.

use serde::{Deserialize, Serialize};

use super::{EQ_BAND_COUNT, EQ_BANDS_HZ};

/// Lowest centre / corner frequency a band may take, in Hz.
pub const EQ_FREQ_MIN: f64 = 10.0;
/// Highest centre / corner frequency a band may take, in Hz. A filter is
/// designed at most at 0.49 x the stream's sample rate (`filter.rs`).
pub const EQ_FREQ_MAX: f64 = 22_000.0;
/// Q bounds. 0.1 is wider than any graphic band, 20 a surgical notch.
pub const EQ_Q_MIN: f64 = 0.1;
pub const EQ_Q_MAX: f64 = 20.0;
/// Gain bound of a band in the band list, in dB (both signs). Wider than the
/// graphic sliders' `EQ_GAIN_DB_MIN..=EQ_GAIN_DB_MAX`: imported correction
/// profiles cut deeper than a slider travels.
pub const EQ_BAND_GAIN_DB_MAX: f64 = 30.0;
/// Most bands a band list holds. The filters would take far more; each band
/// is a table row and a point on the curve, and AutoEq profiles use ten.
pub const EQ_MAX_BANDS: usize = 32;
/// Q a shelf or pass filter takes when none is given (Butterworth).
pub const EQ_DEFAULT_Q: f64 = std::f64::consts::FRAC_1_SQRT_2;
/// Q of the graphic layout's peaks: about one octave per band.
pub const EQ_GRAPHIC_Q: f64 = 1.41;

/// A gain filter under this many dB is flat: it is skipped, not run.
const FLAT_GAIN_DB: f64 = 0.005;

/// What a band does to the spectrum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EqBandKind {
    Peak,
    LowShelf,
    HighShelf,
    LowPass,
    HighPass,
}

impl EqBandKind {
    pub const ALL: [Self; 5] = [
        Self::Peak,
        Self::LowShelf,
        Self::HighShelf,
        Self::LowPass,
        Self::HighPass,
    ];

    /// Pass filters have no gain: only the corner and the Q shape them.
    pub fn has_gain(self) -> bool {
        match self {
            Self::Peak | Self::LowShelf | Self::HighShelf => true,
            Self::LowPass | Self::HighPass => false,
        }
    }

    /// The name the editor shows.
    pub fn label(self) -> &'static str {
        match self {
            Self::Peak => "Peak",
            Self::LowShelf => "Low Shelf",
            Self::HighShelf => "High Shelf",
            Self::LowPass => "Low Pass",
            Self::HighPass => "High Pass",
        }
    }
}

impl std::fmt::Display for EqBandKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// One filter of the equalizer's band list.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EqBand {
    pub kind: EqBandKind,
    /// Centre (peak) or corner (shelf, pass) frequency in Hz.
    pub freq: f64,
    /// Gain in dB. Ignored by the pass filters.
    #[serde(default)]
    pub gain: f64,
    pub q: f64,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
}

fn enabled_by_default() -> bool {
    true
}

impl EqBand {
    pub const fn new(kind: EqBandKind, freq: f64, gain: f64, q: f64) -> Self {
        Self {
            kind,
            freq,
            gain,
            q,
            enabled: true,
        }
    }

    /// The band with every value inside its bounds. Settings files and
    /// imported profiles are user input: a NaN takes a neutral default.
    #[must_use]
    pub fn clamped(self) -> Self {
        Self {
            freq: finite_or(self.freq, 1000.0).clamp(EQ_FREQ_MIN, EQ_FREQ_MAX),
            gain: finite_or(self.gain, 0.0).clamp(-EQ_BAND_GAIN_DB_MAX, EQ_BAND_GAIN_DB_MAX),
            q: finite_or(self.q, EQ_DEFAULT_Q).clamp(EQ_Q_MIN, EQ_Q_MAX),
            ..self
        }
    }

    /// Whether the band changes nothing: switched off, or a gain filter with
    /// (next to) no gain. Flat bands are never run.
    pub fn is_flat(&self) -> bool {
        !self.enabled
            || (self.kind.has_gain() && (self.gain.is_nan() || self.gain.abs() < FLAT_GAIN_DB))
    }
}

fn finite_or(value: f64, fallback: f64) -> f64 {
    if value.is_finite() { value } else { fallback }
}

/// The graphic layout of the band list: one peak per slider at the ISO
/// centres, each with the slider's gain.
pub fn graphic_bands(gains: &[f32; EQ_BAND_COUNT]) -> [EqBand; EQ_BAND_COUNT] {
    std::array::from_fn(|i| {
        EqBand::new(
            EqBandKind::Peak,
            f64::from(EQ_BANDS_HZ[i]),
            f64::from(gains[i]),
            EQ_GRAPHIC_Q,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamped_holds_every_value_in_bounds_and_drops_nan() {
        let wild = EqBand::new(EqBandKind::Peak, 1e9, -99.0, f64::NAN).clamped();
        assert_eq!(wild.freq, EQ_FREQ_MAX);
        assert_eq!(wild.gain, -EQ_BAND_GAIN_DB_MAX);
        assert_eq!(wild.q, EQ_DEFAULT_Q);
        let nan = EqBand::new(EqBandKind::LowShelf, f64::NAN, f64::INFINITY, 0.0).clamped();
        assert_eq!(nan.freq, 1000.0);
        assert_eq!(nan.gain, 0.0);
        assert_eq!(nan.q, EQ_Q_MIN);
    }

    #[test]
    fn flat_means_off_or_a_gain_filter_without_gain() {
        let peak = EqBand::new(EqBandKind::Peak, 1000.0, 0.0, 1.0);
        assert!(peak.is_flat());
        assert!(!EqBand { gain: 0.5, ..peak }.is_flat());
        assert!(
            EqBand {
                gain: 6.0,
                enabled: false,
                ..peak
            }
            .is_flat()
        );
        // A pass filter shapes the spectrum whatever its gain field holds.
        assert!(!EqBand::new(EqBandKind::HighPass, 30.0, 0.0, EQ_DEFAULT_Q).is_flat());
    }

    #[test]
    fn graphic_layout_is_ten_peaks_at_the_iso_centres() {
        let mut gains = [0.0; EQ_BAND_COUNT];
        gains[1] = 5.0;
        let bands = graphic_bands(&gains);
        assert_eq!(bands.len(), EQ_BAND_COUNT);
        for (band, hz) in bands.iter().zip(EQ_BANDS_HZ) {
            assert_eq!(band.kind, EqBandKind::Peak);
            assert_eq!(band.freq, f64::from(hz));
            assert_eq!(band.q, EQ_GRAPHIC_Q);
            assert!(band.enabled);
        }
        assert_eq!(bands[1].gain, 5.0);
    }

    #[test]
    fn a_band_without_gain_or_enabled_keys_deserializes() {
        let band: EqBand =
            serde_json::from_str(r#"{"kind":"high_pass","freq":30.0,"q":0.7}"#).expect("band");
        assert_eq!(band.kind, EqBandKind::HighPass);
        assert_eq!(band.gain, 0.0);
        assert!(band.enabled);
    }
}
