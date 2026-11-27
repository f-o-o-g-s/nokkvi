//! The filters behind the band list: RBJ Audio EQ Cookbook biquad
//! coefficients, the magnitude response a UI draws, and the automatic preamp.
//!
//! Ported from Pawse's `crates/equalizer/src/filter.rs`. Changed here: the
//! response keeps its filters in a fixed array, so the audio thread can
//! design a chain and its preamp without allocating.
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

use std::f64::consts::PI;

use super::band::{EQ_FREQ_MIN, EQ_MAX_BANDS, EqBand, EqBandKind};

/// A filter is designed at most this far up the sample rate: at Nyquist the
/// cookbook formulas degenerate.
const NYQUIST_GUARD: f64 = 0.49;
/// Grid density of the peak search.
const PEAK_STEPS_PER_OCTAVE: f64 = 48.0;

/// Normalized biquad coefficients (`a0` = 1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coefficients {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl Coefficients {
    /// A filter that passes its input through.
    pub const IDENTITY: Self = Self {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    };

    /// Design `band` for a stream at `rate` Hz. The band is clamped first and
    /// its frequency held under Nyquist. Shelves take a Q, as Equalizer APO's
    /// `LSC` / `HSC` do, so an AutoEq profile sounds as it does there.
    pub fn design(band: &EqBand, rate: f64) -> Self {
        let band = band.clamped();
        let freq = band.freq.min(rate * NYQUIST_GUARD);
        let gain = if band.kind.has_gain() { band.gain } else { 0.0 };
        let a = 10f64.powf(gain / 40.0);
        let (sin, cos) = (2.0 * PI * freq / rate).sin_cos();
        let alpha = sin / (2.0 * band.q);
        let shelf = 2.0 * a.sqrt() * alpha;
        let [b0, b1, b2, a0, a1, a2] = match band.kind {
            EqBandKind::Peak => [
                1.0 + alpha * a,
                -2.0 * cos,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cos,
                1.0 - alpha / a,
            ],
            EqBandKind::LowShelf => [
                a * ((a + 1.0) - (a - 1.0) * cos + shelf),
                2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                a * ((a + 1.0) - (a - 1.0) * cos - shelf),
                (a + 1.0) + (a - 1.0) * cos + shelf,
                -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                (a + 1.0) + (a - 1.0) * cos - shelf,
            ],
            EqBandKind::HighShelf => [
                a * ((a + 1.0) + (a - 1.0) * cos + shelf),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                a * ((a + 1.0) + (a - 1.0) * cos - shelf),
                (a + 1.0) - (a - 1.0) * cos + shelf,
                2.0 * ((a - 1.0) - (a + 1.0) * cos),
                (a + 1.0) - (a - 1.0) * cos - shelf,
            ],
            EqBandKind::LowPass => [
                (1.0 - cos) / 2.0,
                1.0 - cos,
                (1.0 - cos) / 2.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ],
            EqBandKind::HighPass => [
                (1.0 + cos) / 2.0,
                -(1.0 + cos),
                (1.0 + cos) / 2.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ],
        };
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    /// |H|^2 at the angle whose sine/cosine (and those of twice it) are given.
    fn power(&self, (s1, c1): (f64, f64), (s2, c2): (f64, f64)) -> f64 {
        let (nr, ni) = (
            self.b0 + self.b1 * c1 + self.b2 * c2,
            self.b1 * s1 + self.b2 * s2,
        );
        let (dr, di) = (
            1.0 + self.a1 * c1 + self.a2 * c2,
            self.a1 * s1 + self.a2 * s2,
        );
        (nr * nr + ni * ni) / (dr * dr + di * di)
    }

    /// Magnitude in dB at `freq` Hz for a stream at `rate` Hz.
    pub fn magnitude_db(&self, freq: f64, rate: f64) -> f64 {
        let w = 2.0 * PI * freq / rate;
        power_db(self.power(w.sin_cos(), (2.0 * w).sin_cos()))
    }
}

/// A power ratio in dB. Held above the smallest positive float, so a stack
/// of pass filters deep in their stop band reads very low, never `-inf`.
fn power_db(power: f64) -> f64 {
    10.0 * power.max(f64::MIN_POSITIVE).log10()
}

/// The combined magnitude response of a band list at one sample rate.
///
/// Flat bands are left out; at most [`EQ_MAX_BANDS`] are taken. Holds its
/// filters inline: building one allocates nothing.
#[derive(Debug, Clone)]
pub struct Response {
    /// (design frequency, filter) of every band that does something.
    filters: [(f64, Coefficients); EQ_MAX_BANDS],
    len: usize,
    rate: f64,
}

impl Response {
    pub fn new(bands: &[EqBand], rate: f64) -> Self {
        let mut filters = [(0.0, Coefficients::IDENTITY); EQ_MAX_BANDS];
        let mut len = 0;
        for band in bands
            .iter()
            .filter(|band| !band.is_flat())
            .take(EQ_MAX_BANDS)
        {
            filters[len] = (band.clamped().freq, Coefficients::design(band, rate));
            len += 1;
        }
        Self { filters, len, rate }
    }

    /// Whether no band does anything: the response is 0 dB everywhere.
    pub fn is_flat(&self) -> bool {
        self.len == 0
    }

    /// Combined magnitude in dB at `freq` Hz.
    pub fn db(&self, freq: f64) -> f64 {
        let w = 2.0 * PI * freq / self.rate;
        let (one, two) = (w.sin_cos(), (2.0 * w).sin_cos());
        // One log for the whole stack: the product of the power ratios stays
        // far inside f64's range for any band list the limits allow.
        power_db(
            self.filters[..self.len]
                .iter()
                .map(|(_, c)| c.power(one, two))
                .product(),
        )
    }

    /// The response maximum in dB, sampled every 1/48 octave from
    /// [`EQ_FREQ_MIN`] to 0.49 x the sample rate, plus each band's own
    /// frequency (a narrow peak can sit between two grid points). Covers the
    /// range above 20 kHz too: hi-res files carry ultrasonic noise there and
    /// a high shelf keeps rising.
    pub fn peak_db(&self) -> f64 {
        if self.is_flat() {
            return 0.0;
        }
        let top = self.rate * NYQUIST_GUARD;
        let octaves = (top / EQ_FREQ_MIN).log2();
        let steps = (octaves * PEAK_STEPS_PER_OCTAVE).ceil().max(1.0) as usize;
        let grid =
            (0..=steps).map(|i| EQ_FREQ_MIN * (top / EQ_FREQ_MIN).powf(i as f64 / steps as f64));
        let centers = self.filters[..self.len]
            .iter()
            .map(|(freq, _)| freq.min(top));
        grid.chain(centers)
            .map(|freq| self.db(freq))
            .fold(f64::NEG_INFINITY, f64::max)
    }
}

/// The automatic preamp for `bands` at `rate` Hz, in dB: minus the response
/// maximum, never above 0. With it the curve never boosts any frequency, so
/// a full-scale tone stays at full scale at most.
pub fn preamp_db(bands: &[EqBand], rate: f64) -> f64 {
    // `+ 0.0` turns the -0.0 of a flat list into 0.0 for anything printing it.
    -Response::new(bands, rate).peak_db().max(0.0) + 0.0
}

#[cfg(test)]
mod tests {
    use super::{
        super::{BUILTIN_PRESETS, band::graphic_bands},
        *,
    };

    const RATE: f64 = 48_000.0;

    fn peak(freq: f64, gain: f64, q: f64) -> EqBand {
        EqBand::new(EqBandKind::Peak, freq, gain, q)
    }

    #[test]
    fn a_peak_reaches_its_gain_at_its_centre_and_fades_away_from_it() {
        let c = Coefficients::design(&peak(1000.0, 6.0, 1.41), RATE);
        assert!((c.magnitude_db(1000.0, RATE) - 6.0).abs() < 1e-9);
        assert!(c.magnitude_db(20.0, RATE).abs() < 0.05);
        assert!(c.magnitude_db(20_000.0, RATE).abs() < 0.05);
        // Half the gain at the bandwidth edges is the cookbook's Q definition.
        let cut = Coefficients::design(&peak(1000.0, -6.0, 1.41), RATE);
        assert!((cut.magnitude_db(1000.0, RATE) + 6.0).abs() < 1e-9);
    }

    #[test]
    fn shelves_reach_their_gain_on_their_side_and_half_of_it_at_the_corner() {
        let low = Coefficients::design(&EqBand::new(EqBandKind::LowShelf, 105.0, 6.0, 0.7), RATE);
        assert!((low.magnitude_db(10.0, RATE) - 6.0).abs() < 0.1);
        assert!((low.magnitude_db(105.0, RATE) - 3.0).abs() < 1e-6);
        assert!(low.magnitude_db(10_000.0, RATE).abs() < 0.01);

        let high = Coefficients::design(
            &EqBand::new(EqBandKind::HighShelf, 10_000.0, -4.0, 0.7),
            RATE,
        );
        assert!((high.magnitude_db(23_000.0, RATE) + 4.0).abs() < 0.1);
        assert!((high.magnitude_db(10_000.0, RATE) + 2.0).abs() < 1e-6);
        assert!(high.magnitude_db(50.0, RATE).abs() < 0.01);
    }

    #[test]
    fn pass_filters_are_minus_three_db_at_the_corner_and_ignore_gain() {
        for kind in [EqBandKind::LowPass, EqBandKind::HighPass] {
            let butterworth = EqBand::new(kind, 1000.0, 12.0, std::f64::consts::FRAC_1_SQRT_2);
            let c = Coefficients::design(&butterworth, RATE);
            assert!(
                (c.magnitude_db(1000.0, RATE) + 3.0103).abs() < 1e-3,
                "{kind:?}"
            );
        }
        let low = Coefficients::design(&EqBand::new(EqBandKind::LowPass, 1000.0, 0.0, 0.707), RATE);
        assert!(low.magnitude_db(20.0, RATE).abs() < 0.01);
        assert!(low.magnitude_db(8000.0, RATE) < -30.0);
        let high =
            Coefficients::design(&EqBand::new(EqBandKind::HighPass, 1000.0, 0.0, 0.707), RATE);
        assert!(high.magnitude_db(20_000.0, RATE).abs() < 0.01);
        assert!(high.magnitude_db(125.0, RATE) < -30.0);
    }

    /// The filter is designed under Nyquist whatever the band asks for, so a
    /// 16 kHz band on a 22.05 kHz stream still yields finite, stable numbers.
    #[test]
    fn a_band_above_nyquist_is_designed_under_it() {
        let c = Coefficients::design(&peak(16_000.0, 6.0, 1.41), 22_050.0);
        for value in [c.b0, c.b1, c.b2, c.a1, c.a2] {
            assert!(value.is_finite());
        }
        // Stable: both poles inside the unit circle.
        assert!(c.a2.abs() < 1.0 && c.a1.abs() < 1.0 + c.a2);
        assert!((c.magnitude_db(22_050.0 * 0.49, 22_050.0) - 6.0).abs() < 1e-6);
    }

    #[test]
    fn the_response_sums_its_bands_and_skips_flat_ones() {
        let bands = [
            peak(100.0, 4.0, 1.0),
            peak(100.0, 2.0, 1.0),
            peak(5000.0, 0.0, 1.0),
            EqBand {
                enabled: false,
                ..peak(100.0, 12.0, 1.0)
            },
        ];
        let response = Response::new(&bands, RATE);
        assert!((response.db(100.0) - 6.0).abs() < 1e-9);
        assert!(Response::new(&bands[2..], RATE).is_flat());
        assert_eq!(Response::new(&[], RATE).peak_db(), 0.0);
    }

    /// The numbers behind the clipping fix: neighbouring boosts stack, so the
    /// response peaks above the highest slider.
    #[test]
    fn stacked_graphic_boosts_peak_above_the_highest_slider() {
        let peak_of = |name: &str| {
            let preset = BUILTIN_PRESETS
                .iter()
                .find(|p| p.name == name)
                .expect("built-in preset");
            Response::new(&graphic_bands(&preset.gains), RATE).peak_db()
        };
        assert!(
            (peak_of("Hip-Hop") - 6.2).abs() < 0.1,
            "{}",
            peak_of("Hip-Hop")
        );
        assert!(
            (peak_of("Bass Boost") - 5.9).abs() < 0.1,
            "{}",
            peak_of("Bass Boost")
        );
    }

    #[test]
    fn the_preamp_is_minus_the_peak_and_never_a_boost() {
        let boost = [peak(60.0, 6.0, 1.0), peak(8000.0, 3.0, 1.0)];
        let response = Response::new(&boost, RATE);
        assert!((preamp_db(&boost, RATE) + response.peak_db()).abs() < 1e-12);
        // With the preamp on top, no frequency ends above 0 dB.
        let preamp = preamp_db(&boost, RATE);
        let mut freq = 10.0;
        while freq < 23_000.0 {
            assert!(response.db(freq) + preamp <= 1e-9, "{freq} Hz");
            freq *= 1.01;
        }
        // A pure cut needs no make-up and gets none.
        assert_eq!(preamp_db(&[peak(1000.0, -6.0, 1.0)], RATE), 0.0);
        assert_eq!(preamp_db(&[], RATE), 0.0);
        assert!(preamp_db(&[], RATE).is_sign_positive());
    }

    /// A peak narrower than the grid is still found, through its own centre.
    #[test]
    fn a_needle_between_grid_points_is_found() {
        let needle = [peak(1234.5, 12.0, 20.0)];
        assert!((Response::new(&needle, RATE).peak_db() - 12.0).abs() < 1e-6);
    }

    /// Thirty-two low passes far into their stop band read very low, not -inf
    /// or NaN, so a curve drawn from the response stays drawable.
    #[test]
    fn a_deep_stop_band_stays_finite() {
        let wall = [EqBand::new(EqBandKind::LowPass, 10.0, 0.0, 0.707); EQ_MAX_BANDS];
        let db = Response::new(&wall, RATE).db(20_000.0);
        assert!(db.is_finite() && db < -1000.0, "{db}");
    }

    #[test]
    fn a_response_takes_at_most_the_band_cap() {
        let many = vec![peak(1000.0, 1.0, 1.0); EQ_MAX_BANDS + 8];
        assert!((Response::new(&many, RATE).db(1000.0) - EQ_MAX_BANDS as f64).abs() < 1e-9);
    }
}
