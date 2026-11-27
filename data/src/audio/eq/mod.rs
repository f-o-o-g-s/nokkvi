use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

use biquad::{Biquad, DirectForm1};

mod band;
mod filter;

pub use band::{
    EQ_BAND_GAIN_DB_MAX, EQ_DEFAULT_Q, EQ_FREQ_MAX, EQ_FREQ_MIN, EQ_GRAPHIC_Q, EQ_MAX_BANDS,
    EQ_Q_MAX, EQ_Q_MIN, EqBand, EqBandKind, graphic_bands,
};
pub use filter::{Coefficients, Response, preamp_db};

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

/// Shared EQ state for lock-free gain transport between UI and audio threads.
/// Cloned into each StreamingSource instance.
#[derive(Clone, Debug)]
pub struct EqState {
    /// Per-band gain in dB, encoded via f32::to_bits(). Range:
    /// `EQ_GAIN_DB_MIN..=EQ_GAIN_DB_MAX`.
    pub gains: Arc<[AtomicU32; EQ_BAND_COUNT]>,
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
        if band < EQ_BAND_COUNT {
            self.gains[band].store(clamp_gain(gain_db).to_bits(), Ordering::Relaxed);
        }
    }

    /// Set all 10 bands at once (for presets).
    pub fn set_all_gains(&self, gains: &[f32; EQ_BAND_COUNT]) {
        for (i, &g) in gains.iter().enumerate() {
            self.gains[i].store(clamp_gain(g).to_bits(), Ordering::Relaxed);
        }
    }

    /// Read a single band's gain (for UI display).
    pub fn get_band_gain(&self, band: usize) -> f32 {
        if band < EQ_BAND_COUNT {
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

/// Check for gain changes every 1024 samples (~23ms at 44.1kHz stereo).
const EQ_CHECK_INTERVAL: usize = 1024;

/// Time constant of the preamp's glide to a new value, in seconds: a gain
/// that stepped under a running waveform would click.
const PREAMP_GLIDE_SECS: f32 = 0.005;

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

/// Per-stream 10-band stereo biquad filter bank.
pub struct EqProcessor {
    /// [band][channel] — 10 bands × 2 channels = 20 DirectForm1 biquads.
    filters: [[DirectForm1<f32>; 2]; EQ_BAND_COUNT],
    /// Cached gains for dirty-checking.
    current_gains: [f32; EQ_BAND_COUNT],
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
    /// The automatic preamp for the current gains, linear: minus the
    /// response peak, so no frequency ends above full scale.
    preamp_target: f32,
    /// `preamp_target` eased per frame, so a gain change never steps.
    preamp: f32,
    /// Per-frame easing coefficient of `preamp`.
    preamp_glide: f32,
}

impl EqProcessor {
    /// Create a new processor with flat (0dB) coefficients.
    pub fn new(state: EqState, sample_rate: u32, channels: u16) -> Self {
        let flat = to_biquad(&Coefficients::IDENTITY);
        let glide_frames = PREAMP_GLIDE_SECS * sample_rate as f32;
        Self {
            filters: [[DirectForm1::<f32>::new(flat); 2]; EQ_BAND_COUNT],
            state,
            sample_rate,
            channels,
            current_gains: [0.0; EQ_BAND_COUNT],
            channel_idx: 0,
            sample_counter: 0,
            preamp_target: 1.0,
            preamp: 1.0,
            preamp_glide: if glide_frames > 0.0 {
                1.0 - (-1.0 / glide_frames).exp()
            } else {
                1.0
            },
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
        if self.channel_idx == 0 {
            // Once per frame, so every channel of a frame takes one preamp.
            self.preamp += self.preamp_glide * (self.preamp_target - self.preamp);
        }
        self.channel_idx = (self.channel_idx + 1) % self.channels as usize;

        // Cascade through all 10 bands
        let mut s = sample;
        for band_filters in &mut self.filters {
            s = band_filters[ch].run(s);
        }

        // No clamp here: this stage sits before volume, ReplayGain and the
        // chain's limiter, and a clamp would flatten an over before the
        // limiter could see it. The preamp keeps the curve at or under 0 dB.
        s * self.preamp
    }

    /// Dirty-check gains, redesign the filters of changed bands and retarget
    /// the preamp.
    fn refresh_if_needed(&mut self) {
        let mut changed = false;
        for band in 0..EQ_BAND_COUNT {
            let gain_db = f32::from_bits(self.state.gains[band].load(Ordering::Relaxed));
            if (gain_db - self.current_gains[band]).abs() > 0.01 {
                self.current_gains[band] = gain_db;
                changed = true;
            }
        }
        if !changed {
            return;
        }
        let rate = f64::from(self.sample_rate);
        let bands = graphic_bands(&self.current_gains);
        for (band, filters) in bands.iter().zip(&mut self.filters) {
            let coeffs = to_biquad(&Coefficients::design(band, rate));
            for filter in filters {
                filter.update_coefficients(coeffs);
            }
        }
        self.preamp_target = 10f64.powf(preamp_db(&bands, rate) / 20.0) as f32;
        if self.sample_counter == 0 {
            // The stream's first samples: nothing is playing to glide under.
            self.preamp = self.preamp_target;
        }
    }
}

/// The designed coefficients in the `biquad` crate's shape.
fn to_biquad(c: &Coefficients) -> biquad::Coefficients<f32> {
    biquad::Coefficients {
        a1: c.a1 as f32,
        a2: c.a2 as f32,
        b0: c.b0 as f32,
        b1: c.b1 as f32,
        b2: c.b2 as f32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Gains are held to the shared bounds, and a NaN never reaches the
    /// filter bank (it would turn every sample of the band into NaN).
    #[test]
    fn band_gains_are_clamped_and_never_nan() {
        let state = EqState::new();
        state.set_band_gain(0, 15.0);
        state.set_band_gain(1, -15.0);
        state.set_band_gain(2, f32::NAN);
        assert_eq!(state.get_band_gain(0), EQ_GAIN_DB_MAX);
        assert_eq!(state.get_band_gain(1), EQ_GAIN_DB_MIN);
        assert!(state.get_band_gain(2).is_finite());

        let mut gains = [0.0; EQ_BAND_COUNT];
        gains[3] = 99.0;
        gains[4] = f32::NAN;
        state.set_all_gains(&gains);
        assert_eq!(state.get_band_gain(3), EQ_GAIN_DB_MAX);
        assert!(state.get_band_gain(4).is_finite());
    }

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

    /// Run a full-scale stereo sine through an enabled processor set to
    /// `gains` and return the left channel after the filters have settled.
    fn settled_sine(gains: &[f32; EQ_BAND_COUNT], freq_hz: f32, rate: u32) -> Vec<f32> {
        let state = EqState::new();
        state.set_all_gains(gains);
        state.set_enabled(true);
        let mut eq = EqProcessor::new(state, rate, 2);
        let frames = rate as usize * 2;
        let mut left = Vec::with_capacity(frames);
        for n in 0..frames {
            let x = (std::f32::consts::TAU * freq_hz * n as f32 / rate as f32).sin();
            left.push(eq.process_sample(x));
            eq.process_sample(x);
        }
        left.split_off(frames / 4)
    }

    /// Longest run of bit-identical consecutive samples above half scale: a
    /// sine never repeats a value three times, a clipped one sits on the rail.
    fn longest_flat_run(samples: &[f32]) -> usize {
        let mut longest = 1;
        let mut run = 1;
        for pair in samples.windows(2) {
            if pair[0] == pair[1] && pair[0].abs() > 0.5 {
                run += 1;
                longest = longest.max(run);
            } else {
                run = 1;
            }
        }
        longest
    }

    /// A boosting preset must leave a full-scale bass tone unclipped: the
    /// preamp takes the response peak off, so the loudest tone comes out at
    /// full scale at most, and nothing is flattened against a clamp.
    #[test]
    fn boosting_presets_do_not_clip_a_full_scale_bass_sine() {
        for name in ["Hip-Hop", "Bass Boost"] {
            let preset = BUILTIN_PRESETS
                .iter()
                .find(|p| p.name == name)
                .expect("built-in preset");
            for rate in [44_100, 48_000, 96_000] {
                let out = settled_sine(&preset.gains, 50.0, rate);
                let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
                assert!(
                    peak <= 1.0,
                    "{name} at {rate} Hz: a full-scale 50 Hz sine peaks at {peak}"
                );
                assert!(peak > 0.5, "{name} at {rate} Hz: the tone is still there");
                let flat = longest_flat_run(&out);
                assert!(
                    flat < 3,
                    "{name} at {rate} Hz: {flat} identical samples in a row (flat-topped)"
                );
            }
        }
    }
}
