use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

use biquad::{Biquad, Coefficients, DirectForm1, ToHertz, Type as FilterType};

/// Shared EQ state for lock-free gain transport between UI and audio threads.
/// Cloned into each StreamingSource instance.
#[derive(Clone, Debug)]
pub struct EqState {
    /// Per-band gain in dB, encoded via f32::to_bits(). Range: -12.0 to +12.0.
    pub gains: Arc<[AtomicU32; 10]>,
    /// Master bypass toggle.
    pub enabled: Arc<AtomicBool>,
}

impl Default for EqState {
    fn default() -> Self {
        Self::new()
    }
}

impl EqState {
    pub fn new() -> Self {
        Self {
            gains: Arc::new(std::array::from_fn(|_| AtomicU32::new(0f32.to_bits()))),
            enabled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Set a single band's gain (called from UI thread).
    pub fn set_band_gain(&self, band: usize, gain_db: f32) {
        if band < 10 {
            self.gains[band].store(gain_db.clamp(-12.0, 12.0).to_bits(), Ordering::Relaxed);
        }
    }

    /// Set all 10 bands at once (for presets).
    pub fn set_all_gains(&self, gains: &[f32; 10]) {
        for (i, &g) in gains.iter().enumerate() {
            self.gains[i].store(g.clamp(-12.0, 12.0).to_bits(), Ordering::Relaxed);
        }
    }

    /// Read a single band's gain (for UI display).
    pub fn get_band_gain(&self, band: usize) -> f32 {
        if band < 10 {
            f32::from_bits(self.gains[band].load(Ordering::Relaxed))
        } else {
            0.0
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }
}

/// ISO standard 10-band graphic EQ center frequencies.
const EQ_BANDS_HZ: [f32; 10] = [
    31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];

/// Q factor — 1.41 gives approximately 1-octave bandwidth per band.
const EQ_Q: f32 = 1.41;

/// Check for gain changes every 1024 samples (~23ms at 44.1kHz stereo).
const EQ_CHECK_INTERVAL: usize = 1024;

/// -1dB headroom: 10^(-1/20) ≈ 0.891. Applied only when max boost > 0dB.
const HEADROOM_LINEAR: f32 = 0.891_254;

/// Flat EQ preset (all bands at 0 dB).
pub const PRESET_FLAT: [f32; 10] = [0.0; 10];

#[derive(Debug, Clone, PartialEq)]
pub struct EqPreset {
    pub name: &'static str,
    pub gains: [f32; 10],
}

impl std::fmt::Display for EqPreset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

/// User-created EQ preset with owned name. Persisted in redb.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CustomEqPreset {
    pub name: String,
    pub gains: [f32; 10],
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

/// Per-stream 10-band stereo biquad filter bank.
pub struct EqProcessor {
    /// [band][channel] — 10 bands × 2 channels = 20 DirectForm1 biquads.
    filters: [[DirectForm1<f32>; 2]; 10],
    /// Cached gains for dirty-checking.
    current_gains: [f32; 10],
    /// Shared state from UI thread.
    state: EqState,
    /// Sample rate for coefficient calculation.
    sample_rate: u32,
    /// Channel count for channel indexing.
    channels: u16,
    /// Running channel index (wraps around channels).
    channel_idx: usize,
    /// Samples since last coefficient check.
    sample_counter: usize,
    /// Whether any band has positive gain (headroom needed).
    needs_headroom: bool,
}

impl EqProcessor {
    /// Create a new processor with flat (0dB) coefficients.
    pub fn new(state: EqState, sample_rate: u32, channels: u16) -> Self {
        let filters = std::array::from_fn(|band| {
            let freq = EQ_BANDS_HZ[band].clamp(20.0, (sample_rate as f32 / 2.0) - 100.0);
            std::array::from_fn(|_| {
                let coeffs = Coefficients::<f32>::from_params(
                    FilterType::PeakingEQ(0.0),
                    (sample_rate as f32).hz(),
                    freq.hz(),
                    EQ_Q,
                )
                .unwrap_or_else(|_| {
                    Coefficients::<f32>::from_params(
                        FilterType::PeakingEQ(0.0),
                        (sample_rate as f32).hz(),
                        1000.0f32.hz(),
                        EQ_Q,
                    )
                    .expect("fallback coefficient generation must succeed")
                });
                DirectForm1::<f32>::new(coeffs)
            })
        });
        Self {
            filters,
            state,
            sample_rate,
            channels,
            current_gains: [0.0; 10],
            channel_idx: 0,
            sample_counter: 0,
            needs_headroom: false,
        }
    }

    /// Check if EQ is enabled (reads the bypass atomic).
    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.state.is_enabled()
    }

    /// Process a single interleaved sample through all 10 bands.
    #[inline]
    pub fn process_sample(&mut self, sample: f32) -> f32 {
        // Periodic dirty-check for gain changes
        if self.sample_counter.is_multiple_of(EQ_CHECK_INTERVAL) {
            self.refresh_if_needed();
        }
        self.sample_counter = self.sample_counter.wrapping_add(1);

        // Channel routing: stereo uses ch 0/1, mono uses ch 0, >2ch clamps to 1
        let ch = self.channel_idx.min(1);
        self.channel_idx = (self.channel_idx + 1) % self.channels as usize;

        // Cascade through all 10 bands
        let mut s = sample;
        for band in 0..10 {
            s = self.filters[band][ch].run(s);
        }

        // Apply headroom if any band is boosting
        if self.needs_headroom {
            s *= HEADROOM_LINEAR;
        }

        s.clamp(-1.0, 1.0)
    }

    /// Dirty-check gains and recalculate coefficients for changed bands.
    fn refresh_if_needed(&mut self) {
        let mut max_gain = 0.0f32;
        for (band, &freq) in EQ_BANDS_HZ.iter().enumerate() {
            let gain_db = f32::from_bits(self.state.gains[band].load(Ordering::Relaxed));
            max_gain = max_gain.max(gain_db);
            if (gain_db - self.current_gains[band]).abs() > 0.01 {
                self.current_gains[band] = gain_db;
                let freq = freq.clamp(20.0, (self.sample_rate as f32 / 2.0) - 100.0);
                if let Ok(coeffs) = Coefficients::<f32>::from_params(
                    FilterType::PeakingEQ(gain_db),
                    (self.sample_rate as f32).hz(),
                    freq.hz(),
                    EQ_Q,
                ) {
                    for ch in 0..2 {
                        self.filters[band][ch].update_coefficients(coeffs);
                    }
                }
            }
        }
        self.needs_headroom = max_gain > 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coefficient_generation_succeeds() {
        let state = EqState::new();
        // Test different sample rates
        for sr in [44100, 48000, 96000] {
            let _eq = EqProcessor::new(state.clone(), sr, 2);
        }
    }

    #[test]
    fn test_flat_eq_passthrough() {
        let state = EqState::new();
        let mut eq = EqProcessor::new(state, 44100, 2);

        let tests = [0.0, 0.5, -0.5, 1.0, -1.0];
        for val in tests {
            let out = eq.process_sample(val);
            assert!((out - val).abs() < 0.01, "Expected roughly {val} got {out}");
        }
    }

    #[test]
    fn test_nyquist_guard() {
        let state = EqState::new();
        let mut eq = EqProcessor::new(state, 32000, 2); // 16KHz band requires clamping
        eq.process_sample(0.0);
    }
}
