//! Pure-Rust spectrum analyzer engine using RustFFT.
//!
//! Drop-in replacement for the vendored cava C library. Implements the same
//! dual-band FFT pipeline (bass + mid/treble), logarithmic frequency mapping,
//! auto-sensitivity, gravity falloff, and integral smoothing — all ported
//! line-by-line from cavacore.c.

use std::sync::Arc;

use num_complex::Complex;
use rustfft::FftPlanner;
use tracing::debug;

/// Errors during spectrum engine initialization.
#[derive(Debug, thiserror::Error)]
pub enum SpectrumError {
    #[error("invalid bar count {0}: must be >= 1 and <= {1} for sample rate {2}")]
    InvalidBarCount(usize, usize, u32),

    #[error("invalid sample rate {0}: must be 1..=384000")]
    InvalidSampleRate(u32),

    #[error("invalid cutoff: lower ({0}) must be < higher ({1}) and higher <= rate/2 ({2})")]
    InvalidCutoff(u32, u32, u32),
}

/// Compute FFT buffer size for a given sample rate (matches cavacore.c lines 37-50).
fn fft_buffer_size_for_rate(rate: u32) -> usize {
    let base = 512;
    if rate > 300_000 {
        base * 64
    } else if rate > 150_000 {
        base * 32
    } else if rate > 75_000 {
        base * 16
    } else if rate > 32_500 {
        base * 8
    } else if rate > 16_250 {
        base * 4
    } else if rate > 8_125 {
        base * 2
    } else {
        base
    }
}

/// Maximum number of bars the FFT can meaningfully produce for a given sample rate.
/// The treble (smaller) buffer is the bottleneck: max_bars = treble_buffer_size / 2.
pub fn max_bars_for_sample_rate(sample_rate: u32) -> usize {
    fft_buffer_size_for_rate(sample_rate) / 2
}

/// Precompute Hann window coefficients for a given size.
fn hann_window(size: usize) -> Vec<f64> {
    (0..size)
        .map(|i| 0.5 * (1.0 - (2.0 * std::f64::consts::PI * i as f64 / (size - 1) as f64).cos()))
        .collect()
}

/// Pure-Rust spectrum analyzer engine.
///
/// Implements cava's full DSP pipeline:
/// 1. Input buffering (sliding window)
/// 2. Dual-band FFT (bass at 2× resolution, treble at 1×)
/// 3. Logarithmic frequency band mapping with per-bar EQ
/// 4. Auto-sensitivity (adaptive gain control)
/// 5. Gravity falloff + integral smoothing
pub struct SpectrumEngine {
    // FFT plans (Send + Sync — no thread-safety workaround needed)
    bass_fft: Arc<dyn rustfft::Fft<f64>>,
    treble_fft: Arc<dyn rustfft::Fft<f64>>,

    // Buffer sizes
    bass_buffer_size: usize,   // 2 × treble (e.g. 8192 at 44.1kHz)
    treble_buffer_size: usize, // e.g. 4096 at 44.1kHz

    // Precomputed Hann window coefficients
    bass_window: Vec<f64>,
    treble_window: Vec<f64>,

    // Input sliding buffer (cavacore.c input_buffer)
    input_buffer: Vec<f64>,

    // FFT working buffers (reused across calls — zero per-frame allocation)
    bass_complex: Vec<Complex<f64>>,
    treble_complex: Vec<Complex<f64>>,
    bass_scratch: Vec<Complex<f64>>,
    treble_scratch: Vec<Complex<f64>>,

    // Frequency band mapping (cavacore.c lines 178-296)
    lower_cutoffs: Vec<usize>,
    upper_cutoffs: Vec<usize>,
    eq: Vec<f64>,
    bass_cut_off_bar: usize,

    // Smoothing state (cavacore.c lines 404-438)
    fall: Vec<f64>,
    mem: Vec<f64>,
    peak: Vec<f64>,
    prev_out: Vec<f64>,

    // Auto-sensitivity state (cavacore.c lines 398-452)
    sens: f64,
    sens_init: bool,
    auto_sensitivity: bool,

    // Framerate tracking
    noise_reduction: f64,
    framerate: f64,
    frame_skip: u32,
    bar_count: usize,
    sample_rate: u32,
}

// RustFFT's Arc<dyn Fft> is Send + Sync, and all other fields are plain data.
// SpectrumEngine is used behind Arc<Mutex<>> in state.rs.
unsafe impl Send for SpectrumEngine {}

impl SpectrumEngine {
    /// Create a new spectrum engine with the given parameters.
    ///
    /// Parameters match the cava Builder API:
    /// - `bar_count`: Number of output bars (must be >= 1 and <= max_bars_for_sample_rate)
    /// - `sample_rate`: Input sample rate in Hz (1..=384000)
    /// - `auto_sensitivity`: If true, output is dynamically scaled to [0, 1]
    /// - `noise_reduction`: 0.0 (fast/noisy) to 1.0 (slow/smooth)
    /// - `lower_cutoff`: Lower frequency bound in Hz
    /// - `higher_cutoff`: Upper frequency bound in Hz (must be <= sample_rate / 2)
    pub fn new(
        bar_count: usize,
        sample_rate: u32,
        auto_sensitivity: bool,
        noise_reduction: f64,
        lower_cutoff: u32,
        higher_cutoff: u32,
    ) -> Result<Self, SpectrumError> {
        // Validation (matches cavacore.c sanity checks)
        if !(1..=384_000).contains(&sample_rate) {
            return Err(SpectrumError::InvalidSampleRate(sample_rate));
        }

        let treble_buffer_size = fft_buffer_size_for_rate(sample_rate);
        let bass_buffer_size = treble_buffer_size * 2;
        let max_bars = treble_buffer_size / 2;

        if bar_count < 1 || bar_count > max_bars {
            return Err(SpectrumError::InvalidBarCount(
                bar_count,
                max_bars,
                sample_rate,
            ));
        }

        if lower_cutoff < 1 || higher_cutoff < 1 || lower_cutoff >= higher_cutoff {
            return Err(SpectrumError::InvalidCutoff(
                lower_cutoff,
                higher_cutoff,
                sample_rate / 2,
            ));
        }

        if higher_cutoff > sample_rate / 2 {
            return Err(SpectrumError::InvalidCutoff(
                lower_cutoff,
                higher_cutoff,
                sample_rate / 2,
            ));
        }

        // Create FFT plans
        let mut planner = FftPlanner::new();
        let bass_fft = planner.plan_fft_forward(bass_buffer_size);
        let treble_fft = planner.plan_fft_forward(treble_buffer_size);

        // Precompute Hann windows
        let bass_window = hann_window(bass_buffer_size);
        let treble_window = hann_window(treble_buffer_size);

        // Input buffer (mono only — bass_buffer_size samples)
        let input_buffer = vec![0.0; bass_buffer_size];

        // FFT scratch buffers
        let bass_complex = vec![Complex::new(0.0, 0.0); bass_buffer_size];
        let treble_complex = vec![Complex::new(0.0, 0.0); treble_buffer_size];
        let bass_scratch = vec![Complex::new(0.0, 0.0); bass_fft.get_inplace_scratch_len()];
        let treble_scratch = vec![Complex::new(0.0, 0.0); treble_fft.get_inplace_scratch_len()];

        // Compute frequency band mapping (cavacore.c lines 178-296)
        let (lower_cutoffs, upper_cutoffs, eq, bass_cut_off_bar) = Self::compute_band_mapping(
            bar_count,
            sample_rate,
            bass_buffer_size,
            treble_buffer_size,
            lower_cutoff,
            higher_cutoff,
        );

        // Smoothing state
        let fall = vec![0.0; bar_count];
        let mem = vec![0.0; bar_count];
        let peak = vec![0.0; bar_count];
        let prev_out = vec![0.0; bar_count];

        debug!(
            "📊 SpectrumEngine initialized: {} bars, {}Hz, bass_fft={}, treble_fft={}, bass_cutoff_bar={}",
            bar_count, sample_rate, bass_buffer_size, treble_buffer_size, bass_cut_off_bar
        );

        Ok(Self {
            bass_fft,
            treble_fft,
            bass_buffer_size,
            treble_buffer_size,
            bass_window,
            treble_window,
            input_buffer,
            bass_complex,
            treble_complex,
            bass_scratch,
            treble_scratch,
            lower_cutoffs,
            upper_cutoffs,
            eq,
            bass_cut_off_bar,
            fall,
            mem,
            peak,
            prev_out,
            sens: 1.0,
            sens_init: true,
            auto_sensitivity,
            noise_reduction,
            framerate: 75.0,
            frame_skip: 1,
            bar_count,
            sample_rate,
        })
    }

    /// Compute logarithmic frequency band mapping and per-bar EQ.
    ///
    /// Faithful port of cavacore.c lines 178-296.
    #[allow(clippy::needless_range_loop)]
    fn compute_band_mapping(
        bar_count: usize,
        sample_rate: u32,
        bass_buffer_size: usize,
        treble_buffer_size: usize,
        lower_cutoff: u32,
        higher_cutoff: u32,
    ) -> (Vec<usize>, Vec<usize>, Vec<f64>, usize) {
        let bass_cut_off_freq: f32 = 100.0;

        // Frequency constant for logarithmic distribution
        let frequency_constant = (lower_cutoff as f64 / higher_cutoff as f64).log10()
            / (1.0 / (bar_count as f64 + 1.0) - 1.0);

        let mut cut_off_frequency = vec![0.0_f32; bar_count + 1];
        let mut relative_cut_off = vec![0.0_f32; bar_count + 1];
        let mut lower_cutoffs = vec![0_usize; bar_count + 1];
        let mut upper_cutoffs = vec![0_usize; bar_count + 1];

        let mut bass_cut_off_bar: usize = 0;
        let mut first_bar: bool;

        let min_bandwidth = sample_rate as f32 / bass_buffer_size as f32;

        for n in 0..bar_count + 1 {
            let bar_distribution_coefficient = -frequency_constant
                + (n as f64 + 1.0) / (bar_count as f64 + 1.0) * frequency_constant;
            cut_off_frequency[n] =
                higher_cutoff as f32 * 10.0_f64.powf(bar_distribution_coefficient) as f32;

            if n > 0 && cut_off_frequency[n - 1] >= cut_off_frequency[n] {
                cut_off_frequency[n] = cut_off_frequency[n - 1] + min_bandwidth;
            }

            // Nyquist-relative frequency
            relative_cut_off[n] = cut_off_frequency[n] / (sample_rate as f32 / 2.0);

            if cut_off_frequency[n] < bass_cut_off_freq {
                // BASS band — uses larger FFT buffer
                lower_cutoffs[n] = (relative_cut_off[n] * (bass_buffer_size as f32 / 2.0)) as usize;
                bass_cut_off_bar += 1;
                first_bar = bass_cut_off_bar <= 1;

                if lower_cutoffs[n] > bass_buffer_size / 2 {
                    lower_cutoffs[n] = bass_buffer_size / 2;
                }
            } else {
                // MID + TREBLE band — uses smaller FFT buffer
                lower_cutoffs[n] =
                    (relative_cut_off[n] * (treble_buffer_size as f32 / 2.0)).ceil() as usize;

                if n == bass_cut_off_bar {
                    first_bar = true;
                    if n > 0 {
                        upper_cutoffs[n - 1] =
                            (relative_cut_off[n] * (bass_buffer_size as f32 / 2.0)) as usize;
                        // Saturating sub to avoid underflow
                        upper_cutoffs[n - 1] = upper_cutoffs[n - 1].saturating_sub(1);
                    }
                } else {
                    first_bar = false;
                }

                if lower_cutoffs[n] > treble_buffer_size / 2 {
                    lower_cutoffs[n] = treble_buffer_size / 2;
                }
            }

            if n > 0 {
                if !first_bar {
                    upper_cutoffs[n - 1] = lower_cutoffs[n].saturating_sub(1);

                    // Push spectrum up if exponential gets clumped in bass
                    if lower_cutoffs[n] <= lower_cutoffs[n - 1] {
                        let max_bin = if n < bass_cut_off_bar {
                            bass_buffer_size / 2
                        } else {
                            treble_buffer_size / 2
                        };

                        if lower_cutoffs[n - 1] + 1 < max_bin + 1 {
                            lower_cutoffs[n] = lower_cutoffs[n - 1] + 1;
                            upper_cutoffs[n - 1] = lower_cutoffs[n] - 1;
                        }
                    }
                } else if upper_cutoffs[n - 1] < lower_cutoffs[n - 1] {
                    upper_cutoffs[n - 1] = lower_cutoffs[n - 1] + 1;
                }
            }

            // Recalculate actual cutoff frequency from quantized bin position
            if n < bass_cut_off_bar {
                relative_cut_off[n] = lower_cutoffs[n] as f32 / (bass_buffer_size as f32 / 2.0);
            } else {
                relative_cut_off[n] = lower_cutoffs[n] as f32 / (treble_buffer_size as f32 / 2.0);
            }
            cut_off_frequency[n] = relative_cut_off[n] * (sample_rate as f32 / 2.0);
        }

        // Compute per-bar EQ (cavacore.c lines 278-295)
        let mut eq = vec![0.0_f64; bar_count];
        for n in 0..bar_count {
            // Normalize the huge FFT magnitudes
            eq[n] = 1.0 / 2.0_f64.powi(28);

            // Boost higher frequencies
            eq[n] *= (cut_off_frequency[n + 1] as f64).powf(0.85);

            // Divide by log2 of the relevant buffer size
            if n < bass_cut_off_bar {
                eq[n] /= (bass_buffer_size as f64).log2();
            } else {
                eq[n] /= (treble_buffer_size as f64).log2();
            }

            // Average over the number of bins in this bar
            let bin_count = upper_cutoffs[n] as f64 - lower_cutoffs[n] as f64 + 1.0;
            if bin_count > 0.0 {
                eq[n] /= bin_count;
            }
        }

        (lower_cutoffs, upper_cutoffs, eq, bass_cut_off_bar)
    }

    /// Process input samples and produce bar values in `output`.
    ///
    /// Faithful port of `cava_execute()` from cavacore.c lines 300-452.
    /// Input is mono f64 PCM samples. Output slice must be at least `bar_count` long.
    #[allow(clippy::needless_range_loop)]
    pub fn execute(&mut self, input: &[f64], output: &mut [f64]) {
        let new_samples = input.len().min(self.input_buffer.len());

        let mut silence = true;

        if new_samples > 0 {
            // Update framerate estimate
            self.framerate -= self.framerate / 64.0;
            self.framerate +=
                (self.sample_rate as f64 * self.frame_skip as f64 / new_samples as f64) / 64.0;
            self.frame_skip = 1;

            // Shift input buffer (sliding window)
            let buf_len = self.input_buffer.len();
            self.input_buffer
                .copy_within(..buf_len - new_samples, new_samples);

            // Insert new samples (reversed, matching cavacore.c)
            for n in 0..new_samples {
                self.input_buffer[new_samples - n - 1] = input[n];
                if input[n] != 0.0 {
                    silence = false;
                }
            }
        } else {
            self.frame_skip += 1;
        }

        // Fill bass buffer from input (mono path)
        for i in 0..self.bass_buffer_size {
            self.bass_complex[i] = Complex::new(self.bass_window[i] * self.input_buffer[i], 0.0);
        }

        // Fill treble buffer from input (mono path)
        for i in 0..self.treble_buffer_size {
            self.treble_complex[i] =
                Complex::new(self.treble_window[i] * self.input_buffer[i], 0.0);
        }

        // Execute FFTs
        self.bass_fft
            .process_with_scratch(&mut self.bass_complex, &mut self.bass_scratch);
        self.treble_fft
            .process_with_scratch(&mut self.treble_complex, &mut self.treble_scratch);

        // Frequency band mapping — sum magnitudes within each bar's bin range
        for n in 0..self.bar_count {
            let mut temp = 0.0;

            for i in self.lower_cutoffs[n]..=self.upper_cutoffs[n] {
                if n < self.bass_cut_off_bar {
                    // Bass band — read from bass FFT output
                    if i < self.bass_complex.len() {
                        temp += self.bass_complex[i].norm(); // hypot(re, im)
                    }
                } else {
                    // Mid + treble — read from treble FFT output
                    if i < self.treble_complex.len() {
                        temp += self.treble_complex[i].norm();
                    }
                }
            }

            // Apply per-bar EQ
            output[n] = temp * self.eq[n];
        }

        // Apply auto-sensitivity
        if self.auto_sensitivity {
            for n in 0..self.bar_count {
                output[n] *= self.sens;
            }
        }

        // Smoothing (gravity falloff + integral)
        let mut overshoot = false;
        let gravity_mod =
            ((60.0 / self.framerate).powf(2.5) * 1.54 / self.noise_reduction).max(1.0);

        for n in 0..self.bar_count {
            // Gravity falloff
            if output[n] < self.prev_out[n] && self.noise_reduction > 0.1 {
                output[n] = self.peak[n] * (1.0 - (self.fall[n] * self.fall[n] * gravity_mod));
                if output[n] < 0.0 {
                    output[n] = 0.0;
                }
                self.fall[n] += 0.028;
            } else {
                self.peak[n] = output[n];
                self.fall[n] = 0.0;
            }
            self.prev_out[n] = output[n];

            // Integral smoothing
            output[n] += self.mem[n] * self.noise_reduction;
            self.mem[n] = output[n];

            if self.auto_sensitivity && output[n] > 1.0 {
                overshoot = true;
                output[n] = 1.0;
            }
        }

        // Auto-sensitivity adjustment
        if self.auto_sensitivity {
            if overshoot {
                self.sens *= 0.98;
                self.sens_init = false;
            } else if !silence {
                self.sens *= 1.001;
                if self.sens_init {
                    self.sens *= 1.1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_max_bars_for_sample_rate() {
        // Standard sample rates
        assert_eq!(max_bars_for_sample_rate(44100), 2048); // 4096 / 2
        assert_eq!(max_bars_for_sample_rate(48000), 2048);
        assert_eq!(max_bars_for_sample_rate(96000), 4096);
        assert_eq!(max_bars_for_sample_rate(8000), 256);
    }

    #[test]
    fn test_hann_window_symmetry() {
        let window = hann_window(256);
        assert_eq!(window.len(), 256);

        // Hann window is symmetric
        for i in 0..128 {
            assert!(
                (window[i] - window[255 - i]).abs() < 1e-10,
                "Window not symmetric at index {i}"
            );
        }

        // Endpoints should be ~0
        assert!(window[0].abs() < 1e-10);
        assert!(window[255].abs() < 1e-10);

        // Middle should be ~1
        assert!((window[127] - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_silence_produces_zeros() {
        let mut engine =
            SpectrumEngine::new(16, 44100, true, 0.77, 50, 10000).expect("init failed");
        let input = vec![0.0; 2048];
        let mut output = vec![0.0; 16];

        engine.execute(&input, &mut output);

        for (i, val) in output.iter().enumerate() {
            assert!(
                *val == 0.0,
                "Bar {i} should be 0 for silent input, got {val}"
            );
        }
    }

    #[test]
    fn test_engine_new_validates_params() {
        // Invalid sample rate
        assert!(SpectrumEngine::new(16, 0, true, 0.77, 50, 10000).is_err());

        // Too many bars
        assert!(SpectrumEngine::new(99999, 44100, true, 0.77, 50, 10000).is_err());

        // Invalid cutoffs
        assert!(SpectrumEngine::new(16, 44100, true, 0.77, 10000, 9000).is_err());

        // Cutoff above Nyquist
        assert!(SpectrumEngine::new(16, 30000, true, 0.77, 50, 16000).is_err());

        // Valid params should succeed
        assert!(SpectrumEngine::new(16, 44100, true, 0.77, 50, 10000).is_ok());
    }

    #[test]
    fn test_band_mapping_coverage() {
        let engine = SpectrumEngine::new(32, 44100, true, 0.77, 50, 10000).expect("init failed");

        // Every bar should have lower <= upper
        for n in 0..32 {
            assert!(
                engine.lower_cutoffs[n] <= engine.upper_cutoffs[n],
                "Bar {n}: lower {} > upper {}",
                engine.lower_cutoffs[n],
                engine.upper_cutoffs[n]
            );
        }

        // EQ values should all be positive
        for (n, eq_val) in engine.eq.iter().enumerate() {
            assert!(*eq_val > 0.0, "EQ[{n}] should be positive, got {eq_val}");
        }
    }

    #[test]
    fn test_auto_sensitivity_ramp() {
        let mut engine =
            SpectrumEngine::new(16, 44100, true, 0.77, 50, 10000).expect("init failed");

        // Feed a constant-amplitude sine wave — sensitivity should ramp up
        let freq = 440.0;
        let amplitude = 0.001; // Very quiet — should trigger sens_init fast ramp
        let chunk_size = 1470; // ~16ms at 44100Hz
        let mut input = vec![0.0; chunk_size];
        let mut output = vec![0.0; 16];

        let initial_sens = engine.sens;

        for frame in 0..100 {
            for (i, sample) in input.iter_mut().enumerate() {
                let t = (frame * chunk_size + i) as f64 / 44100.0;
                *sample = amplitude * (2.0 * std::f64::consts::PI * freq * t).sin();
            }
            engine.execute(&input, &mut output);
        }

        // Sensitivity should have increased from the quiet signal
        assert!(
            engine.sens > initial_sens,
            "Sensitivity {} should be > initial {}",
            engine.sens,
            initial_sens
        );
    }

    #[test]
    fn test_execute_pure_tone() {
        let mut engine =
            SpectrumEngine::new(32, 44100, false, 0.3, 50, 10000).expect("init failed");

        let freq = 440.0;
        let amplitude = 1.0;
        let chunk_size = 4096;
        let mut output = vec![0.0; 32];

        // Feed several frames to fill the sliding window
        for frame in 0..10 {
            let input: Vec<f64> = (0..chunk_size)
                .map(|i| {
                    let t = (frame * chunk_size + i) as f64 / 44100.0;
                    amplitude * (2.0 * std::f64::consts::PI * freq * t).sin()
                })
                .collect();
            engine.execute(&input, &mut output);
        }

        // Find the bar with the highest energy
        let max_bar = output
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(idx, _)| idx)
            .unwrap_or(0);

        // 440Hz with 50-10000Hz range across 32 bars should be in the lower bars
        // (logarithmic distribution means most bars are in upper frequencies)
        assert!(
            max_bar < 16,
            "440Hz peak should be in lower half of bars, was at bar {max_bar}"
        );

        // The peak bar should have significant energy
        assert!(
            output[max_bar] > 0.0,
            "Peak bar {max_bar} should have nonzero energy"
        );
    }
}
