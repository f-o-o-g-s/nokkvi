//! The equalizer: one engine on a band list.
//!
//! - `band.rs`: a band (`EqBand`, `EqBandKind`), its limits, the graphic layout.
//! - `filter.rs`: RBJ biquad design, the response a UI draws, the automatic preamp.
//! - `state.rs`: `EqState`, the lock-free transport from the UI to the audio thread.
//! - `processor.rs`: `EqProcessor`, the per-stream filter chain.
//! - `apo.rs`: Equalizer APO / AutoEq text (`ParametricEQ.txt`) in and out.
//!
//! The ten sliders are the Graphic layout of the band list (ten peaks at the
//! ISO centres); the Parametric layout is a free list of up to
//! [`EQ_MAX_BANDS`] bands.

pub mod apo;
mod band;
mod filter;
mod processor;
mod state;

pub use band::{
    EQ_BAND_GAIN_DB_MAX, EQ_DEFAULT_Q, EQ_FREQ_MAX, EQ_FREQ_MIN, EQ_GRAPHIC_Q, EQ_MAX_BANDS,
    EQ_Q_MAX, EQ_Q_MIN, EqBand, EqBandKind, graphic_bands,
};
pub use filter::{Coefficients, Response, preamp_db};
pub use processor::EqProcessor;
pub use state::{EqMode, EqState};

/// Per-band gain bounds, in dB. Single source shared by the engine clamps
/// (`set_band_gain` / `set_all_gains`), the EQ modal sliders' travel, and the
/// load-time `PersistedPlayerSettings::validate`.
pub const EQ_GAIN_DB_MIN: f32 = -12.0;
pub const EQ_GAIN_DB_MAX: f32 = 12.0;

/// Hold a band gain to `EQ_GAIN_DB_MIN..=EQ_GAIN_DB_MAX`; NaN snaps to the
/// low bound so it never reaches the filter bank.
pub(crate) fn clamp_gain(gain_db: f32) -> f32 {
    crate::utils::clamp::finite_clamp32(gain_db, EQ_GAIN_DB_MIN, EQ_GAIN_DB_MAX)
}

/// A band rounded to the precision it is written with as text (config.toml's
/// `[[settings.eq_bands]]` and an exported `ParametricEQ.txt`): hundredths of
/// a hertz and a decibel, thousandths of Q. One precision for both, so a
/// saved band exported and imported again comes back the same, and rounding
/// what was read back changes nothing (re-saving never drifts).
pub(crate) fn tidied(band: EqBand) -> EqBand {
    fn round(value: f64, scale: f64) -> f64 {
        // `+ 0.0` turns a rounded -0.0 into 0.0, which prints without a sign.
        (value * scale).round() / scale + 0.0
    }
    EqBand {
        freq: round(band.freq, 100.0),
        gain: round(band.gain, 100.0),
        q: round(band.q, 1000.0),
        ..band
    }
}

/// Whether two band lists are the same at the precision they are written
/// with ([`tidied`]): a list read back from config.toml matches the one that
/// was saved. The EQ modal shows a saved profile as selected by it.
pub fn bands_match(a: &[EqBand], b: &[EqBand]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| tidied(*a) == tidied(*b))
}

/// Hold a band list to the band limits: every band clamped, the list cut at
/// [`EQ_MAX_BANDS`]. The settings setter and the load-time `validate` share
/// it (settings files are user input).
pub(crate) fn clamp_bands(bands: &mut Vec<EqBand>) {
    bands.truncate(EQ_MAX_BANDS);
    for band in bands.iter_mut() {
        *band = band.clamped();
    }
}

/// Number of graphic-EQ bands — single source of truth for every gain-array
/// width on the audio and settings/types sides. The 10-element `EQ_BANDS_HZ`
/// literal below compile-pins the value (its declared type uses this const).
pub const EQ_BAND_COUNT: usize = 10;

/// ISO standard 10-band graphic EQ center frequencies.
///
/// Cross-crate length source of truth: the UI's `BAND_FREQS` label array in
/// `src/widgets/eq_modal.rs` is pinned to this array by a `const _: ()` assert
/// and a unit test there, so band count/value changes here force a label review.
pub const EQ_BANDS_HZ: [f32; EQ_BAND_COUNT] = [
    31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];

/// Flat EQ preset (all bands at 0 dB).
pub const PRESET_FLAT: [f32; EQ_BAND_COUNT] = [0.0; EQ_BAND_COUNT];

#[derive(Debug, Clone, PartialEq)]
pub struct EqPreset {
    pub name: &'static str,
    pub gains: [f32; EQ_BAND_COUNT],
}

impl std::fmt::Display for EqPreset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

/// User-created EQ preset with owned name. Persisted with the settings
/// (`custom_eq_presets` in config.toml).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CustomEqPreset {
    pub name: String,
    pub gains: [f32; EQ_BAND_COUNT],
}

impl std::fmt::Display for CustomEqPreset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

/// A saved, named band list for the Parametric layout (headphones, speakers).
/// Persisted with the settings as `eq_profiles` in config.toml, a key of its
/// own: a released nokkvi requires `gains` on every `custom_eq_presets` entry
/// and would refuse the whole `[settings]` section over a band list there.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EqProfile {
    pub name: String,
    /// At most [`EQ_MAX_BANDS`] bands (`clamp_bands` on save and load). A
    /// hand-written profile without any reads as an empty list.
    #[serde(default)]
    pub bands: Vec<EqBand>,
}

impl std::fmt::Display for EqProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

pub const BUILTIN_PRESETS: &[EqPreset] = &[
    EqPreset {
        name: "Bass Boost",
        gains: [5.0, 4.0, 3.0, 1.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    },
    EqPreset {
        name: "Treble Boost",
        gains: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.5, 3.0, 4.0, 5.0],
    },
    EqPreset {
        name: "Rock",
        gains: [3.0, 2.0, 1.0, 0.0, -1.0, -0.5, 1.0, 2.5, 3.0, 3.0],
    },
    EqPreset {
        name: "Pop",
        gains: [1.0, 1.5, 1.0, 0.0, 0.0, 1.0, 2.0, 2.0, 1.5, 1.0],
    },
    EqPreset {
        name: "Jazz",
        gains: [2.0, 1.5, 1.0, 0.0, 0.0, 0.5, 1.0, 1.5, 2.0, 3.0],
    },
    EqPreset {
        name: "Classical",
        gains: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.5],
    },
    EqPreset {
        name: "Electronic",
        gains: [4.0, 3.5, 1.5, 0.0, -1.5, -0.5, 0.5, 2.0, 3.5, 4.0],
    },
    EqPreset {
        name: "Vocal",
        gains: [-2.0, -1.5, -0.5, 0.0, 1.0, 2.5, 3.0, 1.5, 0.0, -1.0],
    },
    EqPreset {
        name: "Acoustic",
        gains: [2.0, 1.5, 1.0, 0.5, 0.0, 0.0, 0.5, 1.5, 2.0, 2.0],
    },
    EqPreset {
        name: "R&B",
        gains: [3.0, 4.0, 2.0, 0.0, -1.0, 0.0, 1.0, 1.5, 2.0, 2.0],
    },
    EqPreset {
        name: "Hip-Hop",
        gains: [4.0, 5.0, 2.5, 0.0, -1.5, 0.0, 0.5, 1.0, 2.5, 3.0],
    },
    EqPreset {
        name: "Loudness",
        gains: [3.0, 2.5, 0.0, 0.0, -1.0, -1.0, 0.0, 1.0, 2.5, 3.0],
    },
    EqPreset {
        name: "Small Speakers",
        gains: [-2.0, -1.0, 1.5, 0.0, -1.0, 1.0, 1.5, 1.0, 0.5, 0.0],
    },
];

#[cfg(test)]
mod tests {
    use super::{EqBand, EqBandKind, bands_match};

    #[test]
    fn band_lists_match_at_the_written_precision() {
        let saved = [
            EqBand::new(EqBandKind::LowShelf, 105.0, 6.5, 0.7),
            EqBand::new(EqBandKind::Peak, 2263.0, -3.1, 0.85),
        ];
        // What a reload reads back: the same values off by rounding noise.
        let reloaded = [
            EqBand::new(EqBandKind::LowShelf, 105.000_4, 6.500_2, 0.700_1),
            EqBand::new(EqBandKind::Peak, 2263.001, -3.100_3, 0.850_2),
        ];
        assert!(bands_match(&saved, &reloaded));
        assert!(bands_match(&[], &[]));

        let mut nudged = saved;
        nudged[1].gain = -3.11;
        assert!(!bands_match(&saved, &nudged), "a hundredth of a dB counts");
        let mut off = saved;
        off[0].enabled = false;
        assert!(!bands_match(&saved, &off), "a switched-off band counts");
        let mut kind = saved;
        kind[0].kind = EqBandKind::HighShelf;
        assert!(!bands_match(&saved, &kind));
        assert!(!bands_match(&saved, &saved[..1]), "so does the length");
    }
}
