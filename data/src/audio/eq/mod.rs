//! The equalizer: one engine on a band list.
//!
//! - `band.rs`: a band (`EqBand`, `EqBandKind`), its limits, the graphic layout.
//! - `filter.rs`: RBJ biquad design, the response a UI draws, the automatic preamp.
//! - `state.rs`: `EqState`, the lock-free transport from the UI to the audio thread.
//! - `processor.rs`: `EqProcessor`, the per-stream filter chain.
//!
//! The ten sliders are the Graphic layout of the band list (ten peaks at the
//! ISO centres); the Parametric layout is a free list of up to
//! [`EQ_MAX_BANDS`] bands.

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
