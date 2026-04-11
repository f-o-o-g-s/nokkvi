//! Streaming audio source for rodio.
//!
//! Bridges the decoder's push model (sends `Vec<f32>` chunks via channel) to
//! rodio's pull model (`Iterator<Item = f32>`). This source is added to the
//! mixer and cpal's audio callback pulls samples from it.

use std::{
    num::NonZero,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    time::Duration,
};

use ringbuf::{HeapCons, traits::Consumer};
use rodio::Source;

/// Atomic f32 helpers using bit-level transmutation.
fn store_f32(atomic: &AtomicU32, value: f32) {
    atomic.store(value.to_bits(), Ordering::Relaxed);
}

fn load_f32(atomic: &AtomicU32) -> f32 {
    f32::from_bits(atomic.load(Ordering::Relaxed))
}

/// Convert a linear 0.0–1.0 volume to a perceptually-correct amplitude.
/// Based on <https://www.dr-lex.be/info-stuff/volumecontrols.html>,
/// same curve as rodio's `Source::amplify_normalized()`.
fn perceptual_volume(linear: f32) -> f32 {
    const GROWTH_RATE: f32 = 6.907_755_4;
    const SCALE_FACTOR: f32 = 1000.0;
    let v = linear.clamp(0.0, 1.0);
    let mut amplitude = (GROWTH_RATE * v).exp() / SCALE_FACTOR;
    if v < 0.1 {
        amplitude *= v * 10.0; // smooth rolloff near zero
    }
    amplitude
}

/// A handle for controlling a streaming source after it's been added to the mixer.
///
/// Holds `Arc` references to shared state that the source reads atomically.
/// All operations are lock-free and safe to call from any thread.
#[derive(Clone)]
pub struct StreamHandle {
    /// Dynamically adjustable volume (0.0–1.0). Applied per-sample in the source.
    pub(super) volume: Arc<AtomicU32>,
    /// Number of individual f32 samples consumed (frames × channels).
    pub(super) samples_consumed: Arc<AtomicU64>,
    /// Set to `true` to make the source return `None` (removes it from the mixer).
    pub(super) stopped: Arc<AtomicBool>,
    /// When true, emit silence without consuming from ring buffer.
    pub(super) paused: Arc<AtomicBool>,
    /// Total number of underrun events (consecutive silence episodes > 882 samples = 10ms).
    pub(super) underrun_count: Arc<AtomicU64>,
    /// Peak consecutive silence samples seen in the worst underrun.
    pub(super) peak_underrun_samples: Arc<AtomicU64>,
    /// Total silence samples emitted due to empty ring buffer.
    pub(super) total_silence_samples: Arc<AtomicU64>,
}

impl StreamHandle {
    /// Set volume (0.0–1.0).
    pub fn set_volume(&self, vol: f32) {
        store_f32(&self.volume, vol.clamp(0.0, 1.0));
    }

    /// Get current volume.
    pub fn get_volume(&self) -> f32 {
        load_f32(&self.volume)
    }

    /// Stop this source (it will be removed from the mixer on next pull).
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
    }

    /// Return the total number of f32 samples consumed.
    pub fn samples_consumed(&self) -> u64 {
        self.samples_consumed.load(Ordering::Relaxed)
    }

    /// Calculate playback position in milliseconds given sample rate and channel count.
    pub fn position_ms(&self, sample_rate: u32, channels: u32) -> u64 {
        if sample_rate == 0 || channels == 0 {
            return 0;
        }
        let total_samples = self.samples_consumed.load(Ordering::Relaxed);
        let frames = total_samples / channels as u64;
        (frames * 1000) / sample_rate as u64
    }

    /// Reset the samples-consumed counter (e.g., after seek or new track).
    pub fn reset_position(&self) {
        self.samples_consumed.store(0, Ordering::Relaxed);
    }

    /// Get underrun diagnostics: (count, peak_samples, total_silence).
    pub fn underrun_stats(&self) -> (u64, u64, u64) {
        (
            self.underrun_count.load(Ordering::Relaxed),
            self.peak_underrun_samples.load(Ordering::Relaxed),
            self.total_silence_samples.load(Ordering::Relaxed),
        )
    }

    /// Pause the stream — emits silence, position freezes.
    pub fn pause(&self) {
        self.paused.store(true, Ordering::Release);
    }

    /// Resume the stream — resumes pulling audio from ring buffer.
    pub fn resume(&self) {
        self.paused.store(false, Ordering::Release);
    }
}

/// Visualizer callback type — receives a batch of f32 samples and the sample rate.
/// Samples are interleaved stereo (or mono), scaled to S16 range for the FFT.
pub type VisualizerCallback = Arc<dyn Fn(&[f32], u32) + Send + Sync>;

/// Shared slot for the visualizer callback. All streams created from the same
/// `RodioOutput` share this, so the callback can be set after streams exist.
pub type SharedVisualizerCallback = Arc<parking_lot::RwLock<Option<VisualizerCallback>>>;

/// A streaming audio source that implements `rodio::Source`.
///
/// Reads f32 samples from a lock-free ring buffer fed by the decoder thread.
/// Applies dynamic volume and optionally taps samples for the visualizer.
pub struct StreamingSource {
    /// Lock-free ring buffer consumer (decoder pushes, cpal callback pulls).
    consumer: HeapCons<f32>,
    /// Number of channels (typically 2 for stereo).
    channels: NonZero<u16>,
    /// Sample rate in Hz.
    sample_rate: NonZero<u32>,
    /// Shared state for external control.
    handle: StreamHandle,
    /// Shared visualizer callback slot — dynamically updated, read via RwLock.
    visualizer: SharedVisualizerCallback,
    /// Visualizer sample accumulator.
    viz_buffer: Vec<f32>,
    /// Number of samples to batch before calling visualizer callback.
    /// ~2048 samples ≈ 23ms at 44.1kHz stereo — good FFT window size.
    viz_batch_size: usize,
    /// Current smoothed volume — interpolates toward the atomic target to avoid
    /// step-function discontinuities during crossfade volume ramps.
    smoothed_volume: f32,
    /// Per-sample smoothing coefficient (exponential moving average).
    /// Computed from sample rate to give ~5ms time constant.
    smoothing_coeff: f32,
    /// Per-stream EQ filter bank. None if EQ is not configured.
    eq: Option<super::eq::EqProcessor>,
    /// Consecutive silence samples emitted (ring buffer empty). Used for underrun tracking.
    consecutive_silence: u64,
}

impl StreamingSource {
    /// Create a new streaming source.
    ///
    /// - `consumer`: The read end of a ring buffer. The decoder writes to the producer end.
    /// - `channels`: Number of audio channels.
    /// - `sample_rate`: Sample rate in Hz.
    /// - `visualizer`: Shared callback slot for tapping samples (can be set later).
    pub fn new(
        consumer: HeapCons<f32>,
        channels: NonZero<u16>,
        sample_rate: NonZero<u32>,
        visualizer: SharedVisualizerCallback,
        initial_volume: f32,
        eq_state: Option<super::eq::EqState>,
    ) -> (Self, StreamHandle) {
        let volume = initial_volume.clamp(0.0, 1.0);
        let handle = StreamHandle {
            volume: Arc::new(AtomicU32::new(volume.to_bits())),
            samples_consumed: Arc::new(AtomicU64::new(0)),
            stopped: Arc::new(AtomicBool::new(false)),
            paused: Arc::new(AtomicBool::new(false)),
            underrun_count: Arc::new(AtomicU64::new(0)),
            peak_underrun_samples: Arc::new(AtomicU64::new(0)),
            total_silence_samples: Arc::new(AtomicU64::new(0)),
        };

        // ~5ms time constant for volume smoothing (avoids crossfade crackle).
        // coeff = 1 - exp(-1 / (tau * sample_rate))
        let tau_samples = 0.005 * sample_rate.get() as f32;
        let smoothing_coeff = if tau_samples > 0.0 {
            1.0 - (-1.0 / tau_samples).exp()
        } else {
            1.0
        };

        let eq = eq_state
            .map(|state| super::eq::EqProcessor::new(state, sample_rate.get(), channels.get()));

        let source = Self {
            consumer,
            channels,
            sample_rate,
            handle: handle.clone(),
            visualizer,
            viz_buffer: Vec::with_capacity(2048),
            viz_batch_size: 2048,
            smoothed_volume: volume,
            smoothing_coeff,
            eq,
            consecutive_silence: 0,
        };

        (source, handle)
    }

    /// Flush any remaining visualizer samples.
    fn flush_viz(&mut self) {
        let guard = self.visualizer.read();
        if let Some(ref cb) = *guard
            && !self.viz_buffer.is_empty()
        {
            cb(&self.viz_buffer, self.sample_rate.get());
            self.viz_buffer.clear();
        }
    }
}

impl Iterator for StreamingSource {
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<f32> {
        // Check stop flag
        if self.handle.stopped.load(Ordering::Acquire) {
            self.flush_viz();
            return None;
        }

        // When paused, emit silence without consuming from ring buffer
        // and without counting samples (position freezes).
        if self.handle.paused.load(Ordering::Acquire) {
            return Some(0.0);
        }

        // Pull one sample from the ring buffer.
        // If empty, emit silence but do NOT count it — prevents position drift
        // during transient underruns (especially radio streams at 1.0× rate).
        let raw = self.consumer.try_pop();
        let mut sample = raw.unwrap_or(0.0);

        if let Some(ref mut eq) = self.eq
            && eq.is_enabled()
        {
            sample = eq.process_sample(sample);
        }

        // Smoothly interpolate toward target volume (exponential moving average).
        // This converts the 20ms step-function volume updates from crossfade into
        // a smooth per-sample ramp, eliminating crackling.
        let target = perceptual_volume(load_f32(&self.handle.volume));
        self.smoothed_volume += self.smoothing_coeff * (target - self.smoothed_volume);
        let output = sample * self.smoothed_volume;

        // Track underruns — count consecutive silence episodes for diagnostics
        if raw.is_some() {
            self.handle.samples_consumed.fetch_add(1, Ordering::Relaxed);
            // End of underrun — record if it was significant (>882 samples ≈ 10ms at 44.1kHz stereo)
            if self.consecutive_silence > 882 {
                self.handle.underrun_count.fetch_add(1, Ordering::Relaxed);
                let prev_peak = self.handle.peak_underrun_samples.load(Ordering::Relaxed);
                if self.consecutive_silence > prev_peak {
                    self.handle
                        .peak_underrun_samples
                        .store(self.consecutive_silence, Ordering::Relaxed);
                }
            }
            self.consecutive_silence = 0;
        } else {
            self.consecutive_silence += 1;
            self.handle
                .total_silence_samples
                .fetch_add(1, Ordering::Relaxed);
        }

        // Feed visualizer only with real samples (not silence fill)
        if raw.is_some() {
            let guard = self.visualizer.read();
            if guard.is_some() {
                // Feed the pre-volume sample scaled to S16 range for the visualizer FFT.
                // Volume is not applied here — the old PipeWire backend applied volume at
                // the stream level, so the visualizer always received full-amplitude PCM.
                self.viz_buffer.push(sample * 32767.0);
                if self.viz_buffer.len() >= self.viz_batch_size {
                    if let Some(ref cb) = *guard {
                        cb(&self.viz_buffer, self.sample_rate.get());
                    }
                    self.viz_buffer.clear();
                }
            }
        }

        Some(output)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, None) // Infinite stream
    }
}

impl Source for StreamingSource {
    #[inline]
    fn current_span_len(&self) -> Option<usize> {
        None // Infinite (until stopped)
    }

    #[inline]
    fn channels(&self) -> NonZero<u16> {
        self.channels
    }

    #[inline]
    fn sample_rate(&self) -> NonZero<u32> {
        self.sample_rate
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        None // Streaming — unknown duration
    }
}
