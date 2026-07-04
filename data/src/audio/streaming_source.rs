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
use tokio::sync::Notify;

use super::{load_f32, store_f32};

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
    /// Crossfade/fade multiplier (0.0–1.0), applied LINEARLY per-sample —
    /// never re-curved through `perceptual_volume`. Kept separate from
    /// `volume` so the user's perceptual volume taper and the fade envelope
    /// are no longer overloaded onto one atomic (the overload re-curved the
    /// cos²/sin² fade into `perceptual(cos²)`, collapsing every default-path
    /// crossfade midpoint to ~−24 dB). Exactly 1.0 outside a fade.
    pub(super) fade_coeff: Arc<AtomicU32>,
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
    /// Fired whenever samples are consumed from the ring buffer.
    ///
    /// The decode loop's write-retry path awaits this instead of busy-sleeping,
    /// so it wakes as soon as the renderer drains space — and stays asleep
    /// for the full timeout while paused (renderer not consuming → no fires).
    pub(super) consumed_notify: Arc<Notify>,
    /// Whether this stream feeds the shared visualizer callback slot.
    ///
    /// Multiple streams share one callback slot via `SharedVisualizerCallback`.
    /// During a crossfade between tracks with different sample rates, two
    /// concurrent streams would otherwise both fire the callback with their
    /// own rates, flipping the visualizer's stored rate atomic each batch and
    /// thrashing the spectrum engine into constant reinitialization. The
    /// renderer sets this `false` on the crossfade incoming stream and flips
    /// it `true` after promotion in `finalize_crossfade`.
    pub(super) feeds_visualizer: Arc<AtomicBool>,
    /// M8 source-level meter gate (default OFF — the only hot-path cost while
    /// off is one relaxed load per real sample, matching the `viz_enabled`
    /// precedent). The renderer enables it lazily from the trailing-silence
    /// window of `render_tick`'s Armed branch; nothing turns it back off
    /// (streams are per-track and the metering cost is negligible).
    pub(super) level_meter_enabled: Arc<AtomicBool>,
    /// M8 recent source level: the peak |sample| of the last completed
    /// `METER_WINDOW_SAMPLES` window of REAL (pre-EQ, pre-volume, pre-fade)
    /// decoded samples. Seeded LOUD (1.0) so an un-metered stream can never
    /// read as silent; starvation fills never update it (a network stall must
    /// not fake musical silence). Read by the renderer's trailing-silence
    /// detector via [`Self::recent_source_peak`].
    pub(super) recent_source_peak: Arc<AtomicU32>,
}

impl StreamHandle {
    /// Set volume (0.0–1.0).
    pub fn set_volume(&self, vol: f32) {
        store_f32(&self.volume, vol.clamp(0.0, 1.0));
    }

    /// Set the fade multiplier (0.0–1.0). Applied linearly in `next()` on top
    /// of the (perceptual) user volume — the crossfade tick writes the raw
    /// curve coefficient here and it reaches the output un-re-curved.
    pub fn set_fade_coeff(&self, fade: f32) {
        store_f32(&self.fade_coeff, fade.clamp(0.0, 1.0));
    }

    /// Stop this source (it will be removed from the mixer on next pull).
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
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

    /// Return a reference to the consumed-samples notify primitive.
    ///
    /// The decode loop captures this at spawn time and awaits it (with a
    /// timeout) when `push_slice` returns 0. The notifier fires periodically
    /// while samples are being consumed; it is silent while the stream is
    /// paused or stopped, letting the decode loop sleep cheaply.
    pub fn consumed_notify(&self) -> &Arc<Notify> {
        &self.consumed_notify
    }

    /// Whether this stream is currently feeding the shared visualizer callback.
    pub fn feeds_visualizer(&self) -> bool {
        self.feeds_visualizer.load(Ordering::Acquire)
    }

    /// Toggle whether this stream feeds the shared visualizer callback.
    ///
    /// The renderer calls this with `true` on the new primary at
    /// `finalize_crossfade` after promoting the crossfade incoming stream.
    pub fn set_feeds_visualizer(&self, feeds: bool) {
        self.feeds_visualizer.store(feeds, Ordering::Release);
    }

    /// Enable the M8 source-level meter on this stream (idempotent). Until
    /// the first metered window completes, [`Self::recent_source_peak`] keeps
    /// its LOUD seed.
    pub fn enable_level_meter(&self) {
        self.level_meter_enabled.store(true, Ordering::Relaxed);
    }

    /// Peak |sample| of the last completed meter window of real decoded
    /// samples (pre-EQ, pre-volume, pre-fade — content level, not what the
    /// user hears). 1.0 (the loud seed) until the meter is enabled and a
    /// first window completes.
    pub fn recent_source_peak(&self) -> f32 {
        load_f32(&self.recent_source_peak)
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
    /// Master visualizer on/off gate, shared by every stream from the same
    /// output. Distinct from `feeds_visualizer` (the crossfade primary
    /// selector): when the user turns the visualizer OFF, the renderer flips
    /// this `false` so `next()` skips the per-sample S16 push + RwLock read +
    /// callback entirely — no DSP feed for a spectrum nothing renders.
    viz_enabled: Arc<AtomicBool>,
    /// Visualizer sample accumulator.
    viz_buffer: Vec<f32>,
    /// Number of samples to batch before calling visualizer callback.
    /// ~2048 samples ≈ 23ms at 44.1kHz stereo — good FFT window size.
    viz_batch_size: usize,
    /// Current smoothed volume — interpolates toward the atomic target to avoid
    /// step-function discontinuities during crossfade volume ramps.
    /// Seeded at 0 for non-bit-perfect streams (the de-click onset ramp: a
    /// fresh stream rises to target over ~23 ms instead of snapping to an
    /// arbitrary mid-waveform value on seek/scrub/skip); bit-perfect streams
    /// seed at the target — a sub-1.0 onset ramp would violate bit-identical
    /// passthrough, so they keep their honest instant onset.
    /// Advanced only on REAL samples (never on ring-starvation silence
    /// fills), so the onset seed survives the decoder's fill latency — e.g.
    /// an uncached network seek — instead of being consumed on silence.
    smoothed_volume: f32,
    /// Current smoothed fade multiplier — interpolates toward the
    /// `fade_coeff` atomic with the same ~5 ms EMA as `smoothed_volume`.
    /// Seeded from `initial_fade` at construction (0.0 for a crossfade
    /// incoming stream) so the smoother can never chase 1.0 → 0.0 while
    /// audio already flows (an audible onset burst at fade start).
    smoothed_fade: f32,
    /// Per-sample smoothing coefficient (exponential moving average).
    /// Computed from sample rate to give ~5ms time constant.
    smoothing_coeff: f32,
    /// Per-stream EQ filter bank. None if EQ is not configured.
    eq: Option<super::eq::EqProcessor>,
    /// Bit-perfect mode: when true, `next` bypasses EQ and software volume so
    /// the decoded PCM is returned untouched. Fixed per stream (set at
    /// creation); the visualizer tap and underrun tracking still run.
    bit_perfect: bool,
    /// Consecutive silence samples emitted (ring buffer empty). Used for underrun tracking.
    consecutive_silence: u64,
    /// Samples consumed since the last `consumed_notify` fire.
    /// We fire the notify every `CONSUMED_NOTIFY_STRIDE` real samples to wake
    /// the decode loop's write-retry path without per-sample overhead.
    samples_since_notify: u32,
    /// M8 level meter: running peak |sample| of the current window (real
    /// samples only). Published to `StreamHandle::recent_source_peak` every
    /// `METER_WINDOW_SAMPLES`, then reset — one amortized atomic store per
    /// window, not per sample.
    meter_window_peak: f32,
    /// M8 level meter: real samples accumulated into the current window.
    meter_window_count: u32,
}

/// Stride (in samples) between consecutive `consumed_notify` fires.
///
/// At 48 kHz stereo the audio callback pulls ~1920 samples per 20 ms tick.
/// Firing every 512 samples (~5 ms) gives the decode loop a tight enough
/// wake-up granularity without calling `notify_one` per-sample.
const CONSUMED_NOTIFY_STRIDE: u32 = 512;

/// M8 level-meter window length in interleaved samples: ~11 ms at 44.1 kHz
/// stereo, ~5 ms at 96 kHz stereo — at least one fresh reading per 20 ms
/// render tick at every supported rate, with one atomic store per window.
const METER_WINDOW_SAMPLES: u32 = 1024;

impl StreamingSource {
    /// Create a new streaming source.
    ///
    /// - `consumer`: The read end of a ring buffer. The decoder writes to the producer end.
    /// - `channels`: Number of audio channels.
    /// - `sample_rate`: Sample rate in Hz.
    /// - `visualizer`: Shared callback slot for tapping samples (can be set later).
    /// - `consumed_notify`: Notify primitive fired every `CONSUMED_NOTIFY_STRIDE` samples.
    ///   The decode loop awaits this (with a timeout) instead of busy-sleeping when the
    ///   ring buffer is full — it wakes as soon as there is space to write.
    /// - `feeds_visualizer`: whether this stream should push samples to the shared
    ///   visualizer callback. The renderer passes `false` for a crossfade incoming
    ///   stream so two concurrent streams cannot thrash the visualizer's per-batch
    ///   sample-rate atomic, then flips it `true` after promotion in `finalize_crossfade`.
    /// - `viz_enabled`: shared master gate — when `false` the source skips the
    ///   visualizer tap entirely (set when the user turns the visualizer off).
    /// - `smooth_starts`: whether the M2 de-click onset ramp seeds the
    ///   user-volume smoother at 0 (the "Smooth Track Starts" setting, default
    ///   on). `false` restores the instant, honest onset. Never applies to
    ///   bit-perfect streams (their arm ignores `smoothed_volume` entirely).
    #[expect(
        clippy::too_many_arguments,
        reason = "low-level stream constructor; each arg is independent decoder/output config — struct-bundling would just wrap them"
    )]
    pub fn new(
        consumer: HeapCons<f32>,
        channels: NonZero<u16>,
        sample_rate: NonZero<u32>,
        visualizer: SharedVisualizerCallback,
        initial_volume: f32,
        initial_fade: f32,
        eq_state: Option<super::eq::EqState>,
        consumed_notify: Arc<Notify>,
        feeds_visualizer: bool,
        viz_enabled: Arc<AtomicBool>,
        smooth_starts: bool,
        bit_perfect: bool,
    ) -> (Self, StreamHandle) {
        let volume = initial_volume.clamp(0.0, 1.0);
        // Seed BOTH the atomic and the smoother from `initial_fade` so they
        // can never disagree at build time: a crossfade incoming stream starts
        // at 0.0 (true silence, no 1.0 → 0.0 EMA chase = onset burst); fresh
        // play/seek streams start at 1.0.
        let fade = initial_fade.clamp(0.0, 1.0);
        let handle = StreamHandle {
            volume: Arc::new(AtomicU32::new(volume.to_bits())),
            fade_coeff: Arc::new(AtomicU32::new(fade.to_bits())),
            samples_consumed: Arc::new(AtomicU64::new(0)),
            stopped: Arc::new(AtomicBool::new(false)),
            paused: Arc::new(AtomicBool::new(false)),
            underrun_count: Arc::new(AtomicU64::new(0)),
            peak_underrun_samples: Arc::new(AtomicU64::new(0)),
            total_silence_samples: Arc::new(AtomicU64::new(0)),
            consumed_notify,
            feeds_visualizer: Arc::new(AtomicBool::new(feeds_visualizer)),
            level_meter_enabled: Arc::new(AtomicBool::new(false)),
            recent_source_peak: Arc::new(AtomicU32::new(1.0_f32.to_bits())),
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
            viz_enabled,
            viz_buffer: Vec::with_capacity(2048),
            viz_batch_size: 2048,
            // De-click onset ramp (non-bit-perfect only, gated by the
            // "Smooth Track Starts" setting — default on): seed the
            // user-volume smoother at 0 so a fresh stream's first ~23 ms
            // ramps up via the per-sample EMA in `next()` instead of
            // snapping to an arbitrary mid-waveform value (a guaranteed
            // click on seek/scrub, manual skip, first track, and the
            // format-mismatch fallback). MUST stay gated off for
            // bit-perfect: its arm never reads `smoothed_volume`, and a
            // sub-1.0 ramp would violate bit-identical passthrough.
            // `smooth_starts = false` is the purist escape hatch — seed at
            // the target for an instant, honest onset.
            smoothed_volume: if smooth_starts && !bit_perfect {
                0.0
            } else {
                volume
            },
            smoothed_fade: fade,
            smoothing_coeff,
            eq,
            bit_perfect,
            consecutive_silence: 0,
            samples_since_notify: 0,
            meter_window_peak: 0.0,
            meter_window_count: 0,
        };

        (source, handle)
    }

    /// Flush any remaining visualizer samples.
    fn flush_viz(&mut self) {
        if !self.viz_enabled.load(Ordering::Relaxed)
            || !self.handle.feeds_visualizer.load(Ordering::Acquire)
        {
            self.viz_buffer.clear();
            return;
        }
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

        // Capture the raw sample for the visualizer BEFORE the EQ stage. The
        // spectrum should reflect the SOURCE track, not the user's EQ/headroom
        // post-processing chain (matches the volume-independent invariant).
        let viz_sample = sample;

        // Bit-perfect bypasses EQ entirely so the sample stays untouched.
        if !self.bit_perfect
            && let Some(ref mut eq) = self.eq
            && eq.is_enabled()
        {
            sample = eq.process_sample(sample);
        }

        // The fade coefficient (`fade_coeff`) is applied LINEARLY on BOTH arms
        // — the crossfade tick already shaped it (cos²/sin²); re-curving it
        // through `perceptual_volume` collapsed the constant-amplitude sum to
        // a ~−24 dB hole at every default-path crossfade midpoint (the M1 bug).
        //
        // Bit-perfect: the user's volume lives on the PipeWire node, so only
        // the fade applies (no perceptual factor, no EQ). At unity, return the
        // raw decoded sample untouched — no software volume, not even the
        // ~1.0000001 unity-curve multiply — so a settled body stays
        // bit-identical; the fade is smoothed per-sample to avoid step-update
        // crackle, then snaps back to raw passthrough once it resettles.
        //
        // Non-bit-perfect (default path): the perceptual taper applies to the
        // USER volume only — that is what should be perceptually tapered — and
        // the fade multiplies on top, linearly.
        const UNITY_SNAP_EPSILON: f32 = 1e-4;
        let output = if self.bit_perfect {
            let fade = load_f32(&self.handle.fade_coeff);
            if fade >= 1.0 && (self.smoothed_fade - 1.0).abs() < UNITY_SNAP_EPSILON {
                self.smoothed_fade = 1.0;
                sample
            } else {
                self.smoothed_fade += self.smoothing_coeff * (fade - self.smoothed_fade);
                sample * self.smoothed_fade
            }
        } else {
            // Clock the volume EMA on REAL samples only: during ring
            // starvation (raw = None) the output is silence regardless of the
            // smoother, so advancing it would consume the M2 de-click onset
            // seed on silence fills — a seek into an uncached region of a
            // network track (50-500 ms HTTP fill latency ≫ the ~12 ms EMA
            // convergence) would then snap its first decoded sample at full
            // gain, the exact mid-waveform click the ramp exists to remove.
            // Freezing during starvation is output-identical for the starved
            // pulls; the ramp then multiplies real audio from sample one.
            //
            // The FADE smoother stays pull-clocked on purpose: the crossfade
            // envelope is wall-clock-driven by the renderer tick, so it must
            // keep tracking `fade_coeff` through an underrun, not lag it.
            if raw.is_some() {
                let target = perceptual_volume(load_f32(&self.handle.volume));
                self.smoothed_volume += self.smoothing_coeff * (target - self.smoothed_volume);
            }
            let fade = load_f32(&self.handle.fade_coeff);
            self.smoothed_fade += self.smoothing_coeff * (fade - self.smoothed_fade);
            sample * self.smoothed_volume * self.smoothed_fade
        };

        // Track underruns — count consecutive silence episodes for diagnostics
        if raw.is_some() {
            self.handle.samples_consumed.fetch_add(1, Ordering::Relaxed);

            // M8 source-level meter (REAL samples only — starvation silence
            // must never fake musical silence): accumulate the window peak of
            // the raw decoded sample (pre-EQ/volume/fade, same tap point as
            // the visualizer) and publish it once per window. One relaxed
            // load per real sample while disabled; one amortized store per
            // 1024 samples while metering.
            if self.handle.level_meter_enabled.load(Ordering::Relaxed) {
                let amplitude = viz_sample.abs();
                if amplitude > self.meter_window_peak {
                    self.meter_window_peak = amplitude;
                }
                self.meter_window_count += 1;
                if self.meter_window_count >= METER_WINDOW_SAMPLES {
                    store_f32(&self.handle.recent_source_peak, self.meter_window_peak);
                    self.meter_window_peak = 0.0;
                    self.meter_window_count = 0;
                }
            }
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

            // Notify the decode loop every CONSUMED_NOTIFY_STRIDE real samples.
            // The loop awaits this notify (with a 500 ms timeout) instead of
            // busy-sleeping when push_slice returns 0 (ring buffer full).
            // While paused the renderer emits silence without consuming, so the
            // notify never fires and the decode loop sleeps for the full timeout —
            // eliminating the 5 ms livelock observed during paused radio streams.
            self.samples_since_notify += 1;
            if self.samples_since_notify >= CONSUMED_NOTIFY_STRIDE {
                self.samples_since_notify = 0;
                self.handle.consumed_notify.notify_one();
            }
        } else {
            self.consecutive_silence += 1;
            self.handle
                .total_silence_samples
                .fetch_add(1, Ordering::Relaxed);
        }

        // Feed visualizer only with real samples (not silence fill), and only
        // from the stream currently designated the visualizer feeder. During a
        // crossfade the incoming stream is gated off here so two streams at
        // different sample rates cannot flip the visualizer's per-batch rate
        // atomic and thrash the spectrum engine into constant reinit.
        if self.viz_enabled.load(Ordering::Relaxed)
            && raw.is_some()
            && self.handle.feeds_visualizer.load(Ordering::Acquire)
        {
            let guard = self.visualizer.read();
            if guard.is_some() {
                // Feed the pre-volume AND pre-EQ sample scaled to S16 range for
                // the visualizer FFT. Neither volume nor EQ is applied here: the
                // spectrum reflects the source track, not the user's processing
                // chain (the old PipeWire backend tapped pre-EQ/pre-volume too).
                self.viz_buffer.push(viz_sample * 32767.0);
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

#[cfg(test)]
mod tests {
    use ringbuf::{
        HeapRb,
        traits::{Producer, Split},
    };

    use super::*;

    /// Build a `StreamingSource` with `samples` of f32 data already in the ring,
    /// wired to the supplied shared callback slot.
    fn make_source(
        sample_rate: u32,
        samples: usize,
        callback: SharedVisualizerCallback,
        feeds_visualizer: bool,
    ) -> (StreamingSource, StreamHandle) {
        make_source_gated(sample_rate, samples, callback, feeds_visualizer, true)
    }

    /// Like [`make_source`] but with explicit control over the master
    /// `viz_enabled` gate (the off-switch the renderer flips when the user
    /// turns the visualizer off).
    fn make_source_gated(
        sample_rate: u32,
        samples: usize,
        callback: SharedVisualizerCallback,
        feeds_visualizer: bool,
        viz_enabled: bool,
    ) -> (StreamingSource, StreamHandle) {
        let rb = HeapRb::<f32>::new(samples.max(1));
        let (mut producer, consumer) = rb.split();
        let data: Vec<f32> = (0..samples).map(|i| (i as f32 * 0.001).sin()).collect();
        producer.push_slice(&data);

        StreamingSource::new(
            consumer,
            NonZero::new(2).expect("2 is nonzero"),
            NonZero::new(sample_rate).expect("test sample rate is nonzero"),
            callback,
            1.0,
            1.0,
            None,
            Arc::new(Notify::new()),
            feeds_visualizer,
            Arc::new(AtomicBool::new(viz_enabled)),
            true,
            false,
        )
    }

    /// M8 level meter: with a value-filled ring and the meter enabled, one
    /// full window of real pulls publishes the window's peak |sample| to the
    /// handle; without `enable_level_meter` the LOUD 1.0 seed never moves.
    #[test]
    fn level_meter_publishes_window_peak_of_real_samples() {
        let (source_quiet, _obs) = make_constant_source(2e-4, 2_048, None);
        let mut source_quiet = source_quiet;
        let handle = source_quiet.handle.clone();
        assert_eq!(
            handle.recent_source_peak(),
            1.0,
            "the meter must seed LOUD so an un-metered stream never reads silent"
        );

        handle.enable_level_meter();
        for _ in 0..(METER_WINDOW_SAMPLES as usize + 16) {
            let _ = source_quiet.next();
        }
        let peak = handle.recent_source_peak();
        assert!(
            (peak - 2e-4).abs() < 1e-7,
            "one completed window must publish the real content peak, got {peak}"
        );
    }

    #[test]
    fn level_meter_disabled_keeps_loud_seed() {
        let (mut source, _obs) = make_constant_source(2e-4, 2_048, None);
        let handle = source.handle.clone();
        for _ in 0..2_048 {
            let _ = source.next();
        }
        assert_eq!(
            handle.recent_source_peak(),
            1.0,
            "without enable_level_meter the seed must never move"
        );
    }

    /// Starvation fills (ring empty → silence) must NOT update the meter: a
    /// network stall would otherwise fake musical silence and fire the
    /// trailing-silence trigger mid-stall.
    #[test]
    fn level_meter_ignores_starvation_silence() {
        // Exactly one window of loud samples, then the ring runs dry.
        let (mut source, _obs) = make_constant_source(0.5, METER_WINDOW_SAMPLES as usize, None);
        let handle = source.handle.clone();
        handle.enable_level_meter();
        // Pull the real window plus two windows of starvation silence.
        for _ in 0..(3 * METER_WINDOW_SAMPLES as usize) {
            let _ = source.next();
        }
        let peak = handle.recent_source_peak();
        assert!(
            (peak - 0.5).abs() < 1e-7,
            "starved pulls must keep the last REAL window's peak, got {peak}"
        );
    }

    /// Build a callback that records every sample-rate it sees, and the shared
    /// slot wrapping it.
    fn counting_callback() -> (SharedVisualizerCallback, Arc<parking_lot::Mutex<Vec<u32>>>) {
        let observed = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let observed_for_cb = observed.clone();
        let cb: VisualizerCallback = Arc::new(move |_samples: &[f32], rate: u32| {
            observed_for_cb.lock().push(rate);
        });
        let slot: SharedVisualizerCallback = Arc::new(parking_lot::RwLock::new(Some(cb)));
        (slot, observed)
    }

    /// Crossfade thrash regression: two streams at different sample rates
    /// sharing one viz callback slot must only let the active one fire it.
    /// Otherwise the visualizer's stored-rate atomic flips per batch and the
    /// spectrum engine reinitializes itself into a blank state for the entire
    /// crossfade window.
    #[test]
    fn only_active_stream_feeds_visualizer_during_crossfade() {
        let (slot, observed) = counting_callback();

        // Plenty of samples per source so we definitely cross a viz batch
        // boundary (viz_batch_size = 2048) several times.
        let (mut primary, _h_primary) = make_source(44_100, 8_192, slot.clone(), true);
        let (mut incoming, _h_incoming) = make_source(48_000, 8_192, slot.clone(), false);

        // Interleave next() calls — models the cpal callback alternating
        // between the two ActiveStreams during a crossfade.
        for _ in 0..4_096 {
            let _ = primary.next();
            let _ = incoming.next();
        }

        let rates = observed.lock();
        assert!(
            !rates.is_empty(),
            "primary stream should have produced ≥1 viz batch"
        );
        assert!(
            rates.iter().all(|&r| r == 44_100),
            "viz callback must only be fired by the active stream; saw rates {:?}",
            *rates
        );
    }

    /// `finalize_crossfade` promotes the formerly-silent incoming stream to
    /// primary and flips its visualizer flag on. After that flip, the same
    /// source must start feeding the callback.
    #[test]
    fn promoted_stream_feeds_visualizer_after_flag_flip() {
        let (slot, observed) = counting_callback();

        let (mut source, handle) = make_source(48_000, 8_192, slot, false);

        // Drain enough to cross several viz batch boundaries — nothing fed
        // because the source starts inactive.
        for _ in 0..4_096 {
            let _ = source.next();
        }
        assert!(
            observed.lock().is_empty(),
            "inactive stream must not feed visualizer; saw rates {:?}",
            *observed.lock()
        );

        // Promote: models finalize_crossfade's set_feeds_visualizer(true).
        handle.set_feeds_visualizer(true);

        for _ in 0..4_096 {
            let _ = source.next();
        }
        let rates = observed.lock();
        assert!(
            !rates.is_empty(),
            "promoted stream should now drive the visualizer"
        );
        assert!(
            rates.iter().all(|&r| r == 48_000),
            "post-promotion callback fires must carry this stream's rate; saw {:?}",
            *rates
        );
    }

    /// Master-gate regression: when `viz_enabled` is `false` (the user turned
    /// the visualizer off), a primary stream must NOT fire the callback even
    /// while real audio is flowing — the per-sample tap is skipped entirely so
    /// the audio thread stops feeding a spectrum nothing renders.
    #[test]
    fn viz_disabled_suppresses_tap() {
        let (slot, observed) = counting_callback();

        // feeds_visualizer = true (this is the primary stream), but the master
        // gate is OFF — plenty of samples to cross several batch boundaries.
        let (mut source, _handle) = make_source_gated(44_100, 8_192, slot, true, false);
        for _ in 0..4_096 {
            let _ = source.next();
        }

        assert!(
            observed.lock().is_empty(),
            "viz_enabled=false must suppress the tap; saw rates {:?}",
            *observed.lock()
        );
    }

    /// Positive control for [`viz_disabled_suppresses_tap`]: with the master
    /// gate ON, the same primary stream feeds the callback as normal.
    #[test]
    fn viz_enabled_feeds_tap() {
        let (slot, observed) = counting_callback();

        let (mut source, _handle) = make_source_gated(44_100, 8_192, slot, true, true);
        for _ in 0..4_096 {
            let _ = source.next();
        }

        assert!(
            !observed.lock().is_empty(),
            "viz_enabled=true primary stream should feed the visualizer"
        );
    }

    /// Build a callback that records every f32 sample pushed into it.
    fn recording_callback() -> (SharedVisualizerCallback, Arc<parking_lot::Mutex<Vec<f32>>>) {
        let observed = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let observed_for_cb = observed.clone();
        let cb: VisualizerCallback = Arc::new(move |samples: &[f32], _rate: u32| {
            observed_for_cb.lock().extend_from_slice(samples);
        });
        let slot: SharedVisualizerCallback = Arc::new(parking_lot::RwLock::new(Some(cb)));
        (slot, observed)
    }

    /// Build a feeding `StreamingSource` whose ring is filled with a constant
    /// `value`, wired to `eq_state`, recording every pushed visualizer sample.
    fn make_constant_source(
        value: f32,
        samples: usize,
        eq_state: Option<super::super::eq::EqState>,
    ) -> (StreamingSource, Arc<parking_lot::Mutex<Vec<f32>>>) {
        let (slot, observed) = recording_callback();
        let rb = HeapRb::<f32>::new(samples.max(1));
        let (mut producer, consumer) = rb.split();
        let data = vec![value; samples];
        producer.push_slice(&data);

        let (source, _handle) = StreamingSource::new(
            consumer,
            NonZero::new(2).expect("2 is nonzero"),
            NonZero::new(48_000).expect("test sample rate is nonzero"),
            slot,
            1.0,
            1.0,
            eq_state,
            Arc::new(Notify::new()),
            true,
            Arc::new(AtomicBool::new(true)),
            true,
            false,
        );
        (source, observed)
    }

    /// N15 / N18: the visualizer tap must read the RAW (pre-EQ, pre-volume)
    /// sample. With EQ enabled and a band boosted, the post-EQ `sample` differs
    /// from raw by at least the -1 dB headroom factor (0.891254) plus band
    /// shaping; the tap must still report `raw * 32767`, not the EQ'd value.
    #[test]
    fn visualizer_tap_is_pre_eq() {
        const RAW: f32 = 0.5;
        let eq = super::super::eq::EqState::new();
        eq.set_enabled(true);
        eq.set_band_gain(5, 12.0); // boost 1 kHz band — forces headroom + shaping

        // 3 full viz batches' worth of samples so the callback fires.
        let (mut source, observed) = make_constant_source(RAW, 2048 * 3, Some(eq));
        for _ in 0..(2048 * 3) {
            let _ = source.next();
        }

        let pushed = observed.lock();
        assert!(!pushed.is_empty(), "EQ-enabled stream should feed the viz");
        let expected = RAW * 32767.0;
        assert!(
            pushed.iter().all(|&s| (s - expected).abs() < 1e-3),
            "viz tap must be pre-EQ ({expected}); saw e.g. {:?}",
            pushed.first()
        );
    }

    /// Bit-perfect AT UNITY: `next` returns the RAW decoded sample even with EQ
    /// enabled+boosted — EQ and the perceptual software-volume curve are both
    /// bypassed so the PCM reaches the sink untouched. (User volume is applied at
    /// the PipeWire node; the `volume` atomic stays at unity in normal play.)
    /// This is the bit-perfect body invariant for normal (non-crossfade) playback.
    #[test]
    fn bit_perfect_at_unity_outputs_raw_sample() {
        const RAW: f32 = 0.5;
        let rb = HeapRb::<f32>::new(16);
        let (mut producer, consumer) = rb.split();
        producer.push_slice(&[RAW; 8]);

        let eq = super::super::eq::EqState::new();
        eq.set_enabled(true);
        eq.set_band_gain(5, 12.0); // boost — would shape the sample IF applied
        let viz: SharedVisualizerCallback = Arc::new(parking_lot::RwLock::new(None));
        let (mut source, _handle) = StreamingSource::new(
            consumer,
            NonZero::new(2).expect("2 is nonzero"),
            NonZero::new(44_100).expect("44100 is nonzero"),
            viz,
            1.0, // unity (the volume the renderer builds bit-perfect streams with)
            1.0, // no fade in progress
            Some(eq),
            Arc::new(Notify::new()),
            true,
            Arc::new(AtomicBool::new(true)),
            true,
            true, // bit_perfect
        );

        let out = source.next().expect("a sample");
        assert_eq!(
            out, RAW,
            "bit-perfect output at unity must equal the raw decoded sample (EQ + volume bypassed)"
        );
    }

    /// Bit-perfect DURING A CROSSFADE: a crossfade incoming stream is built
    /// bit-perfect AND with `initial_fade = 0.0`, then the crossfade tick ramps
    /// the `fade_coeff` atomic up. The fade MUST apply (otherwise a Relaxed
    /// crossfade would slam both tracks in at full volume), and once it
    /// resettles at unity the body must become bit-exact raw output again (the
    /// snap-to-raw guard). The fade lives on `fade_coeff` — the `volume` atomic
    /// is ignored on the bit-perfect path (user volume is on the PipeWire node).
    #[test]
    fn bit_perfect_applies_crossfade_fade_then_snaps_to_raw_at_unity() {
        const RAW: f32 = 0.5;
        let rb = HeapRb::<f32>::new(8192);
        let (mut producer, consumer) = rb.split();
        producer.push_slice(&[RAW; 8192]);

        let viz: SharedVisualizerCallback = Arc::new(parking_lot::RwLock::new(None));
        let (mut source, handle) = StreamingSource::new(
            consumer,
            NonZero::new(2).expect("2 is nonzero"),
            NonZero::new(44_100).expect("44100 is nonzero"),
            viz,
            1.0, // stream_volume() under bit-perfect (pw-native volume) is 1.0
            0.0, // silent fade start — exactly how the renderer builds a crossfade stream
            None,
            Arc::new(Notify::new()),
            true,
            Arc::new(AtomicBool::new(true)),
            true,
            true, // bit_perfect
        );

        // Silent start: the fade is applied from true silence (smoother seeded
        // at 0.0 — no 1.0 → 0.0 EMA chase, i.e. no onset burst), NOT bypassed
        // to raw.
        let first = source.next().expect("a sample");
        assert!(
            first.abs() < 1e-6,
            "a bit-perfect crossfade stream must start silent (got {first}, raw is {RAW})"
        );

        // Drive the fade to unity (finalize sets the promoted stream to 1.0) and
        // pump enough samples for the smoother to converge + snap to raw.
        handle.set_fade_coeff(1.0);
        let mut last = first;
        for _ in 0..4096 {
            last = source.next().expect("a sample");
        }
        assert_eq!(
            last, RAW,
            "a settled bit-perfect body must return to bit-exact raw output"
        );
    }

    /// N16: a stream constructed with a `Some(EqState)` carries a live
    /// `EqProcessor` — when EQ is enabled and a band boosted, the OUTPUT sample
    /// differs from the raw input (the per-stream processor is applied). This
    /// pins the renderer-side guarantee that every stream gets a processor.
    #[test]
    fn streaming_source_applies_eq_when_state_enabled() {
        const RAW: f32 = 0.5;
        let eq = super::super::eq::EqState::new();
        eq.set_enabled(true);
        eq.set_band_gain(5, 12.0);

        let (mut source, _observed) = make_constant_source(RAW, 4096, Some(eq));
        // Drive past EQ_CHECK_INTERVAL so coefficients refresh, then collect
        // outputs once the biquad cascade has settled toward steady state.
        let mut last = 0.0f32;
        for _ in 0..4096 {
            if let Some(s) = source.next() {
                last = s;
            }
        }
        assert!(
            (last - RAW).abs() > 1e-3,
            "enabled EQ must shape the OUTPUT sample away from raw {RAW}; got {last}",
        );
    }

    /// N15 / N18 control: with EQ disabled the tap is unchanged — raw * 32767.
    #[test]
    fn visualizer_tap_matches_raw_when_eq_disabled() {
        const RAW: f32 = 0.5;
        let (mut source, observed) = make_constant_source(RAW, 2048 * 2, None);
        for _ in 0..(2048 * 2) {
            let _ = source.next();
        }
        let pushed = observed.lock();
        let expected = RAW * 32767.0;
        assert!(
            pushed.iter().all(|&s| (s - expected).abs() < 1e-3),
            "viz tap with no EQ must equal raw * 32767 ({expected})",
        );
    }

    /// Build a NON-bit-perfect source with a constant-`RAW` ring and explicit
    /// `initial_volume` / `initial_fade`, no EQ, no visualizer — the minimal
    /// fixture for the M1 fade-linearity tests on the default path.
    fn make_fade_source(
        raw: f32,
        samples: usize,
        initial_volume: f32,
        initial_fade: f32,
    ) -> (StreamingSource, StreamHandle) {
        let rb = HeapRb::<f32>::new(samples.max(1));
        let (mut producer, consumer) = rb.split();
        let data = vec![raw; samples];
        producer.push_slice(&data);
        let viz: SharedVisualizerCallback = Arc::new(parking_lot::RwLock::new(None));
        StreamingSource::new(
            consumer,
            NonZero::new(2).expect("2 is nonzero"),
            NonZero::new(48_000).expect("48000 is nonzero"),
            viz,
            initial_volume,
            initial_fade,
            None,
            Arc::new(Notify::new()),
            true,
            Arc::new(AtomicBool::new(true)),
            true,
            false, // NOT bit-perfect — the default path the M1 fix exists for
        )
    }

    /// THE M1 bug fix: on the default (non-bit-perfect) path the fade
    /// coefficient must be applied LINEARLY on top of the perceptual user
    /// volume — `raw · fade · perceptual(user_vol)` — never folded into the
    /// volume atomic and re-curved through `perceptual_volume` (which
    /// collapsed the cos²/sin² midpoint to ~−30 dB per stream: the −24 dB
    /// crossfade hole).
    #[test]
    fn fade_coeff_applies_linearly_not_perceptually() {
        const RAW: f32 = 0.5;
        const USER_VOL: f32 = 0.8;
        let (mut source, handle) = make_fade_source(RAW, 16_384, USER_VOL, 1.0);

        // Pump the volume smoother to steady state first (seeded at the linear
        // volume, it chases the perceptual target over the ~5 ms EMA; 4096
        // samples ≫ 5 time constants at 48 kHz).
        for _ in 0..4096 {
            let _ = source.next();
        }

        // Midpoint fade coefficient of the cos² curve: cos²(0.5·π/2) = 0.5.
        handle.set_fade_coeff(0.5);
        let mut last = 0.0f32;
        for _ in 0..4096 {
            last = source.next().expect("a sample");
        }

        let expected = RAW * 0.5 * perceptual_volume(USER_VOL);
        assert!(
            (last - expected).abs() < 1e-3,
            "fade must apply linearly: expected ≈ {expected} (raw·fade·perceptual(vol)), got {last}"
        );
        // The historical bug realized perceptual(fade·user_vol) instead — the
        // output must be FAR from that re-curved value.
        let recurved = RAW * perceptual_volume(0.5 * USER_VOL);
        assert!(
            (last - recurved).abs() > 1e-2,
            "output {last} still matches the re-curved bug value {recurved}"
        );
    }

    /// Onset-burst blocker guard: a crossfade incoming stream is built with
    /// `initial_fade = 0.0`, and its FIRST sample must be ≈ 0 — not merely
    /// `< RAW`. A `smoothed_fade` hardcoded to 1.0 would chase 1.0 → 0.0 over
    /// ~5 ms while audio already flows, playing the first few ms at near-full
    /// amplitude (an audible burst at the START of every crossfade).
    #[test]
    fn crossfade_incoming_first_sample_is_silent() {
        const RAW: f32 = 0.5;
        let (mut source, _handle) = make_fade_source(RAW, 64, 1.0, 0.0);

        let first = source.next().expect("a sample");
        assert!(
            first.abs() < 1e-6,
            "incoming stream's first sample must be ≈ 0 (true silence), got {first}"
        );
    }

    /// Constant-amplitude invariant across the full fade sweep: with the fade
    /// applied linearly, a cos²/sin² pair of realized gains sums to 1 at every
    /// progress point — no −24 dB hole in the middle of the blend. (Under the
    /// old overloaded-volume scheme each side realized perceptual(coefficient),
    /// so the pair summed to ~0.06 at the midpoint instead of 1.)
    #[test]
    fn fade_pair_realized_gains_sum_to_unity() {
        const RAW: f32 = 0.5;
        let realized_gain = |fade: f32| -> f32 {
            let (mut source, _handle) = make_fade_source(RAW, 8192, 1.0, fade);
            // Pump the volume smoother to steady state (M2 seeds it at 0 for
            // the de-click onset ramp; 4096 samples ≫ 5 EMA time constants at
            // 48 kHz) so this asserts the settled realized gain, not the ramp.
            let mut last = 0.0f32;
            for _ in 0..4096 {
                last = source.next().expect("a sample");
            }
            last / RAW
        };

        for i in 0..=10u32 {
            let p = f64::from(i) / 10.0;
            let fade_out = (p * std::f64::consts::FRAC_PI_2).cos().powi(2) as f32;
            let fade_in = (p * std::f64::consts::FRAC_PI_2).sin().powi(2) as f32;
            let sum = realized_gain(fade_out) + realized_gain(fade_in);
            assert!(
                (sum - 1.0).abs() < 1e-3,
                "realized out+in gains must sum to 1 at p={p}: got {sum}"
            );
        }
    }

    /// M2 de-click onset ramp: a fresh NON-bit-perfect stream must ramp up
    /// from silence over ~23 ms instead of snapping to an arbitrary
    /// mid-waveform value — the guaranteed click on seek/scrub, manual skip,
    /// first track, and the format-mismatch fallback. The user-volume
    /// smoother (`smoothed_volume`) seeds at 0 and chases the perceptual
    /// target through the existing per-sample EMA in `next()`.
    #[test]
    fn non_bit_perfect_onset_ramps_from_silence() {
        const RAW: f32 = 0.5;
        // A fresh play/seek stream: full user volume, no fade in progress.
        let (mut source, _handle) = make_fade_source(RAW, 8192, 1.0, 1.0);

        // First sample ≈ 0: at most one EMA step above true silence
        // (raw · coeff ≈ −47 dB at 48 kHz), never the raw mid-waveform value.
        let first = source.next().expect("a sample");
        assert!(
            first.abs() < RAW * 0.01,
            "fresh non-bit-perfect stream must start ≈ silent (de-click ramp), got {first}"
        );

        // Partway up after ~1 EMA time constant (240 samples at 48 kHz):
        // strictly rising, not stuck at silence, not yet at full gain.
        let mut mid = first;
        for _ in 0..240 {
            mid = source.next().expect("a sample");
        }
        assert!(
            mid > RAW * 0.4 && mid < RAW * 0.9,
            "after ~1 EMA time constant the ramp should be partway up, got {mid}"
        );

        // Converges to the full target: raw · perceptual(1.0) · fade(1.0) = raw.
        let mut last = mid;
        for _ in 0..4096 {
            last = source.next().expect("a sample");
        }
        assert!(
            (last - RAW).abs() < 1e-3,
            "onset ramp must converge to full gain {RAW}, got {last}"
        );
    }

    /// M2 review fix: the onset ramp must be clocked on REAL samples, not on
    /// pulls. A seek into an uncached region of a network track while playing
    /// leaves the freshly-recreated stream pulling against an EMPTY ring for
    /// the decoder's HTTP range fetch (50-500 ms) — the mixer adds the source
    /// immediately, the seek path deliberately leaves it unpaused while
    /// playing, and the rebuffer latch is un-primed on a fresh ring. If the
    /// volume EMA advances on those silence fills it converges 0 → target in
    /// ~12 ms wall, consuming the de-click seed before any audio exists, and
    /// the first decoded sample snaps at full gain — the exact mid-waveform
    /// click M2 exists to remove. Freezing the EMA during starvation is
    /// output-identical (silence is 0.0 regardless), so the seed must survive
    /// until real samples flow.
    #[test]
    fn onset_ramp_survives_ring_starvation() {
        const RAW: f32 = 0.5;
        let rb = HeapRb::<f32>::new(8192);
        let (mut producer, consumer) = rb.split();
        let viz: SharedVisualizerCallback = Arc::new(parking_lot::RwLock::new(None));
        let (mut source, _handle) = StreamingSource::new(
            consumer,
            NonZero::new(2).expect("2 is nonzero"),
            NonZero::new(48_000).expect("48000 is nonzero"),
            viz,
            1.0, // full user volume
            1.0, // no fade in progress — a plain play/seek stream
            None,
            Arc::new(Notify::new()),
            true,
            Arc::new(AtomicBool::new(true)),
            true,
            false, // NOT bit-perfect — the default path
        );

        // Starve: 2400 pulls (25 ms at 48 kHz stereo, ≫ the ~12 ms EMA
        // convergence) against the empty ring — models the HTTP range fetch
        // after an uncached seek. Output is silence throughout.
        for _ in 0..2400 {
            let s = source.next().expect("a sample");
            assert_eq!(s, 0.0, "starvation pulls must output silence");
        }

        // The first decoded samples arrive.
        producer.push_slice(&[RAW; 4096]);

        // The first REAL sample still gets the de-click ramp: ≈ silent (at
        // most one EMA step above 0), never a full-gain mid-waveform snap.
        let first = source.next().expect("a sample");
        assert!(
            first.abs() < RAW * 0.01,
            "onset ramp must survive starvation: first real sample ≈ 0, got {first}"
        );

        // And it still converges to full gain on real audio.
        let mut last = first;
        for _ in 0..4095 {
            last = source.next().expect("a sample");
        }
        assert!(
            (last - RAW).abs() < 1e-3,
            "post-starvation ramp must converge to full gain {RAW}, got {last}"
        );
    }

    /// M2 gate (invariant 8): the de-click onset ramp stays OFF for
    /// bit-perfect streams — a sub-1.0 ramp for ~23 ms would violate
    /// bit-identical passthrough. A fresh bit-perfect source outputs the raw
    /// decoded sample at full gain from its very first pull (the honest
    /// instant onset).
    #[test]
    fn bit_perfect_onset_is_instant_full_gain() {
        const RAW: f32 = 0.5;
        let rb = HeapRb::<f32>::new(16);
        let (mut producer, consumer) = rb.split();
        producer.push_slice(&[RAW; 8]);

        let viz: SharedVisualizerCallback = Arc::new(parking_lot::RwLock::new(None));
        let (mut source, _handle) = StreamingSource::new(
            consumer,
            NonZero::new(2).expect("2 is nonzero"),
            NonZero::new(48_000).expect("48000 is nonzero"),
            viz,
            1.0, // unity — the volume the renderer builds bit-perfect streams with
            1.0, // no fade in progress
            None,
            Arc::new(Notify::new()),
            true,
            Arc::new(AtomicBool::new(true)),
            true,
            true, // bit_perfect — the onset ramp must NOT apply
        );

        let first = source.next().expect("a sample");
        assert_eq!(
            first, RAW,
            "bit-perfect onset must be instant full-gain raw output (no de-click ramp)"
        );
    }

    /// M5 "Smooth Track Starts" gate: with the toggle OFF, a fresh
    /// NON-bit-perfect stream keeps the instant, honest onset — the
    /// user-volume smoother seeds at the target instead of 0, so the very
    /// first pulled sample is at full gain (the purist escape hatch for
    /// M2's default-on de-click ramp).
    #[test]
    fn onset_ramp_disabled_restores_instant_start() {
        const RAW: f32 = 0.5;
        let rb = HeapRb::<f32>::new(16);
        let (mut producer, consumer) = rb.split();
        producer.push_slice(&[RAW; 8]);

        let viz: SharedVisualizerCallback = Arc::new(parking_lot::RwLock::new(None));
        let (mut source, _handle) = StreamingSource::new(
            consumer,
            NonZero::new(2).expect("2 is nonzero"),
            NonZero::new(48_000).expect("48000 is nonzero"),
            viz,
            1.0, // full user volume — perceptual(1.0) = 1.0
            1.0, // no fade in progress
            None,
            Arc::new(Notify::new()),
            true,
            Arc::new(AtomicBool::new(true)),
            false, // smooth_starts OFF — the toggle under test
            false, // NOT bit-perfect
        );

        let first = source.next().expect("a sample");
        assert_eq!(
            first, RAW,
            "with Smooth Track Starts off, the first sample must be full gain \
             (instant onset), not the start of a ramp"
        );
    }
}
