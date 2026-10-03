//! Rodio-based audio output manager.
//!
//! Replaces the PipeWire-specific `AudioOutput` with a cross-platform rodio/cpal
//! implementation. Uses a shared `Mixer` (from the app-wide `MixerDeviceSink`)
//! to add streaming sources. All audio (music + SFX) flows through one cpal stream.

use std::{
    num::NonZero,
    sync::{Arc, atomic::AtomicBool},
};

use anyhow::Result;
use ringbuf::{HeapRb, traits::Split};
use rodio::mixer::Mixer;
use tokio::sync::Notify;
use tracing::{debug, info, warn};

use super::{
    NormalizationConfig,
    streaming_source::{
        GainSwitch, SharedVisualizerCallback, StreamHandle, StreamingSource, gain_switch_queue,
    },
};

/// Default ring buffer capacity in samples.
/// 48000 Hz × 2 channels × ~52 seconds = 5,000,000 samples.
/// Large buffer size allows massive network jitter pre-buffering for internet radios.
pub const RING_BUFFER_CAPACITY: usize = 5_000_000;

/// A handle to an active audio stream on the mixer.
///
/// Holds the producer side of the ring buffer (for feeding decoded audio)
/// and the control handle (for volume, position, stop).
pub struct ActiveStream {
    /// Push decoded f32 samples here. The `StreamingSource` on the mixer reads from the other end.
    pub producer: ringbuf::HeapProd<f32>,
    /// Control handle for volume, position tracking, and stop.
    pub handle: StreamHandle,
    /// Sample rate that this stream was created with.
    pub sample_rate: u32,
    /// Channel count that this stream was created with.
    pub channels: u16,
    /// Whether this stream was BUILT bit-perfect (DSP bypassed). Fixed at
    /// construction, so the honest badge can stamp the promoted stream's own
    /// fact instead of the live setting, which may have flipped since.
    pub bit_perfect: bool,
    /// Normalization bookkeeping on the writer side (boxed: it rides inside
    /// the renderer's `CrossfadeState::Active` too).
    gain: Box<WriterGain>,
}

/// A stream's normalization state on the writer side.
struct WriterGain {
    /// The normalization the stream was built with. Its chain (AGC or not)
    /// is fixed for the stream's life; a static gain can still change at a
    /// gapless join ([`ActiveStream::schedule_norm_gain`]).
    norm: NormalizationConfig,
    /// Samples pushed into the ring since the stream was built: the ring
    /// index the next sample written takes, where a gain switch scheduled
    /// now applies from.
    written: u64,
    /// Normalization gain applied to the samples being written now.
    write_gain: f32,
    /// Producer end of the source's gain-switch queue.
    switches: ringbuf::HeapProd<GainSwitch>,
}

impl ActiveStream {
    /// Wrap a stream's ring producer and control handle. `gain_switches` is
    /// the producer end of the queue whose consumer the source was built
    /// with ([`StreamingSource::with_normalization`]), and `norm` the
    /// normalization it was built at.
    pub(crate) fn new(
        producer: ringbuf::HeapProd<f32>,
        handle: StreamHandle,
        sample_rate: u32,
        channels: u16,
        bit_perfect: bool,
        norm: NormalizationConfig,
        gain_switches: ringbuf::HeapProd<GainSwitch>,
    ) -> Self {
        Self {
            producer,
            handle,
            sample_rate,
            channels,
            bit_perfect,
            gain: Box::new(WriterGain {
                norm,
                written: 0,
                write_gain: norm.stream_gain().unwrap_or(1.0),
                switches: gain_switches,
            }),
        }
    }

    /// Write decoded f32 samples to the stream.
    /// Returns the number of samples actually written (may be less if the ring buffer is full).
    pub fn write_samples(&mut self, samples: &[f32]) -> usize {
        use ringbuf::traits::Producer;
        let written = self.producer.push_slice(samples);
        self.gain.written += written as u64;
        written
    }

    /// Play every sample written from now on at normalization `gain`: queue
    /// a switch at the ring index the next sample written takes, so playback
    /// changes gain exactly where the next track starts (a gapless join). A
    /// no-op when the gain is unchanged. Only a stream that
    /// [`NormalizationConfig::can_follow`]s the next track's normalization
    /// should be asked.
    pub(crate) fn schedule_norm_gain(&mut self, gain: f32) {
        use ringbuf::traits::Producer;
        let writer = &mut *self.gain;
        if gain.to_bits() == writer.write_gain.to_bits() {
            return;
        }
        if writer.switches.try_push((writer.written, gain)).is_ok() {
            writer.write_gain = gain;
        } else {
            warn!(
                "🔊 [RODIO] Gain-switch queue full; the next track keeps {:.3}× instead of {:.3}×",
                writer.write_gain, gain
            );
        }
    }

    /// Whether this stream can play a track that resolves to `next` by a
    /// gain switch alone ([`NormalizationConfig::can_follow`] from the
    /// normalization it was built with).
    pub(crate) fn can_follow(&self, next: NormalizationConfig) -> bool {
        self.gain.norm.can_follow(next)
    }

    /// Normalization gain applied to the samples being written now.
    #[cfg(test)]
    pub(crate) fn write_gain(&self) -> f32 {
        self.gain.write_gain
    }

    /// Check how many samples can be written without blocking.
    pub fn available_space(&self) -> usize {
        use ringbuf::traits::Observer;
        self.producer.vacant_len()
    }

    /// Set the stream volume (0.0–1.0).
    pub fn set_volume(&self, vol: f32) {
        self.handle.set_volume(vol);
    }

    /// Set the fade multiplier (0.0–1.0). The crossfade tick writes the raw
    /// curve coefficient here; it is applied linearly in the source (never
    /// re-curved through the perceptual volume taper).
    pub fn set_fade_coeff(&self, fade: f32) {
        self.handle.set_fade_coeff(fade);
    }

    /// Get playback position in milliseconds.
    pub fn position_ms(&self) -> u64 {
        self.handle
            .position_ms(self.sample_rate, self.channels as u32)
    }

    /// Reset position counter (e.g., after seek).
    pub fn reset_position(&self) {
        self.handle.reset_position();
    }

    /// Stop and remove this stream from the mixer.
    pub fn stop(&self) {
        self.handle.stop();
    }

    /// Silence the stream (zero residual resampler buffer), then stop and remove from mixer.
    /// Takes `self` by value since callers always `.take()` the stream from its `Option` first.
    pub fn silence_and_stop(self) {
        self.set_volume(0.0);
        // Bit-perfect streams apply only `fade_coeff` (the `volume` atomic is
        // ignored there), so zero the fade too for silencing parity.
        self.set_fade_coeff(0.0);
        self.stop();
    }

    /// Pause the stream — emits silence, position freezes.
    pub fn pause(&self) {
        self.handle.pause();
    }

    /// Resume the stream — resumes pulling audio from ring buffer.
    pub fn resume(&self) {
        self.handle.resume();
    }

    /// Toggle whether this stream feeds the shared visualizer callback.
    pub fn set_feeds_visualizer(&self, feeds: bool) {
        self.handle.set_feeds_visualizer(feeds);
    }
}

/// The audio output manager for music playback.
///
/// Uses a shared `Mixer` from the app-wide `MixerDeviceSink` (owned by the
/// SFX engine). This ensures all audio goes through a single cpal output stream,
/// avoiding conflicts with ALSA/PipeWire when multiple streams are opened.
pub struct RodioOutput {
    /// Shared mixer — add sources here to play them through the device.
    mixer: Mixer,
    /// Shared visualizer callback slot. All streams read from this; updated dynamically.
    visualizer_callback: SharedVisualizerCallback,
    /// Shared master visualizer on/off gate. Cloned into every stream so the
    /// renderer can suppress the per-sample tap on all of them at once when
    /// the user turns the visualizer off.
    viz_enabled: Arc<AtomicBool>,
}

impl RodioOutput {
    /// Create a new audio output using a shared mixer.
    ///
    /// The `mixer` should come from the app-wide `MixerDeviceSink` (typically
    /// owned by the SFX engine). The `viz_callback` is the shared visualizer
    /// callback slot owned by the renderer.
    pub fn new(
        mixer: Mixer,
        viz_callback: SharedVisualizerCallback,
        viz_enabled: Arc<AtomicBool>,
    ) -> Result<Self> {
        info!("🔊 [RODIO] Music output initialized (shared mixer)");

        Ok(Self {
            mixer,
            visualizer_callback: viz_callback,
            viz_enabled,
        })
    }

    /// Create a new audio stream on the mixer.
    ///
    /// Returns an `ActiveStream` that you can feed decoded f32 samples into.
    /// The stream is immediately active on the output's mixer.
    ///
    /// - `sample_rate`: Sample rate of the decoded audio.
    /// - `channels`: Channel count of the decoded audio.
    /// - `initial_volume`: Starting volume (0.0–1.0).
    /// - `initial_fade`: Starting fade multiplier (0.0–1.0). Pass `1.0` for
    ///   fresh play/seek streams and `0.0` for a crossfade incoming stream
    ///   (it fades in via its `fade_coeff`, from true silence).
    /// - `norm`: Resolved normalization decision for this stream
    ///   (off, AGC at target level, or static linear gain).
    /// - `consumed_notify`: Notify primitive fired every ~512 consumed samples.
    ///   The decode loop awaits this to avoid busy-sleeping when the ring is full.
    /// - `feeds_visualizer`: whether this stream should push samples into the
    ///   shared visualizer callback. Pass `true` for primary streams; pass
    ///   `false` for a crossfade incoming stream, then call
    ///   `ActiveStream::set_feeds_visualizer(true)` after promotion to primary.
    /// - `smooth_starts`: whether the M2 de-click onset ramp applies (the
    ///   "Smooth Track Starts" setting); inert for bit-perfect streams.
    #[expect(
        clippy::too_many_arguments,
        reason = "thin pass-through to StreamingSource::new; same independent-config rationale applies"
    )]
    pub fn create_stream(
        &self,
        sample_rate: u32,
        channels: u16,
        initial_volume: f32,
        initial_fade: f32,
        norm: super::NormalizationConfig,
        eq_state: Option<super::eq::EqState>,
        consumed_notify: Arc<Notify>,
        feeds_visualizer: bool,
        smooth_starts: bool,
        bit_perfect: bool,
    ) -> ActiveStream {
        // Create lock-free ring buffer
        let rb = HeapRb::<f32>::new(RING_BUFFER_CAPACITY);
        let (producer, consumer) = rb.split();

        let channels_nz = NonZero::new(channels).unwrap_or(NonZero::new(2).expect("2 is nonzero"));
        let sample_rate_nz =
            NonZero::new(sample_rate).unwrap_or(NonZero::new(44100).expect("44100 is nonzero"));

        // Create the streaming source with initial volume
        let (source, handle) = StreamingSource::new(
            consumer,
            channels_nz,
            sample_rate_nz,
            self.visualizer_callback.clone(),
            initial_volume,
            initial_fade,
            eq_state,
            consumed_notify,
            feeds_visualizer,
            self.viz_enabled.clone(),
            smooth_starts,
            bit_perfect,
        );
        // A static gain (ReplayGain / fallback dB; Off is unity) is applied
        // inside the source, where a gapless join can change it at the next
        // track's first sample (`ActiveStream::schedule_norm_gain`).
        let (gain_switches, switch_reader) = gain_switch_queue();
        let source = source.with_normalization(norm.stream_gain().unwrap_or(1.0), switch_reader);

        // Bit-perfect: add the source to the mixer with NO post-processing —
        // no AGC, no static gain, no peak limiter. The StreamingSource itself
        // also bypasses EQ and software volume (see its `next`), so the decoded
        // PCM reaches the mixer untouched. User volume is applied at the
        // PipeWire node instead. Safe to drop the limiter: a lossless source is
        // already within full scale.
        if bit_perfect {
            self.mixer.add(source);
            debug!(
                "🔊 [RODIO] Created BIT-PERFECT stream (DSP bypassed): {}ch, {}Hz",
                channels, sample_rate
            );
            return ActiveStream::new(
                producer,
                handle,
                sample_rate,
                channels,
                bit_perfect,
                norm,
                gain_switches,
            );
        }

        // Pre-mixer chain. The peak limiter sits at the end of every variant so
        // any AGC overshoot or static-gain boost (applied in the source) is
        // clamped before mixing.
        use rodio::source::{AutomaticGainControlSettings, LimitSettings, Source};
        match norm {
            super::NormalizationConfig::Off => {
                self.mixer
                    .add(source.limit(LimitSettings::dynamic_content()));
                debug!(
                    "🔊 [RODIO] Created stream: {}ch, {}Hz, vol={:.2}",
                    channels, sample_rate, initial_volume
                );
            }
            super::NormalizationConfig::Agc { target_level } => {
                let agc_settings = AutomaticGainControlSettings {
                    target_level,
                    ..AutomaticGainControlSettings::default()
                };
                self.mixer.add(
                    source
                        .automatic_gain_control(agc_settings)
                        .limit(LimitSettings::dynamic_content()),
                );
                debug!(
                    "🔊 [RODIO] Created stream with AGC (target={:.1}): {}ch, {}Hz, vol={:.2}",
                    target_level, channels, sample_rate, initial_volume
                );
            }
            super::NormalizationConfig::Static(gain) => {
                self.mixer
                    .add(source.limit(LimitSettings::dynamic_content()));
                debug!(
                    "🔊 [RODIO] Created stream with static gain ({:.3}× ≈ {:+.2} dB): {}ch, {}Hz, vol={:.2}",
                    gain,
                    20.0 * gain.max(f32::MIN_POSITIVE).log10(),
                    channels,
                    sample_rate,
                    initial_volume
                );
            }
        }

        ActiveStream::new(
            producer,
            handle,
            sample_rate,
            channels,
            bit_perfect,
            norm,
            gain_switches,
        )
    }
}

#[cfg(test)]
mod tests {
    use rodio::mixer::MixerSource;

    use super::*;

    /// A music output on a device-less mixer, plus the mixer's source end
    /// (what the audio device would pull).
    fn output() -> (RodioOutput, MixerSource) {
        let (mixer, source) = rodio::mixer::mixer(
            NonZero::new(2).expect("2 is nonzero"),
            NonZero::new(48_000).expect("48000 is nonzero"),
        );
        let output = RodioOutput::new(
            mixer,
            Arc::new(parking_lot::RwLock::new(None)),
            Arc::new(AtomicBool::new(true)),
        )
        .expect("a detached output builds");
        (output, source)
    }

    fn stream(output: &RodioOutput, norm: NormalizationConfig) -> ActiveStream {
        output.create_stream(
            48_000,
            2,
            1.0,
            1.0,
            norm,
            None,
            Arc::new(Notify::new()),
            false,
            false, // no onset ramp: the first sample is already at full level
            false,
        )
    }

    /// Pull `n` samples of output, starting from the stream's first sample.
    fn played(mixed: &mut MixerSource, n: usize) -> Vec<f32> {
        let mut out = Vec::with_capacity(n);
        let mut started = false;
        for _ in 0..(n + 4_096) {
            let sample = mixed.next().expect("the mixer never ends");
            started |= sample != 0.0;
            if started {
                out.push(sample);
                if out.len() == n {
                    break;
                }
            }
        }
        out
    }

    /// A static ReplayGain gain is applied to the stream from its first
    /// sample.
    #[test]
    fn static_gain_applies_from_the_first_sample() {
        let (output, mut mixed) = output();
        let mut active = stream(&output, NormalizationConfig::Static(0.5));
        active.write_samples(&[0.2; 1_000]);

        let out = played(&mut mixed, 1_000);

        assert!(
            (out[0] - 0.1).abs() < 1e-4,
            "0.2 at gain 0.5, got {}",
            out[0]
        );
        assert!((out[999] - 0.1).abs() < 1e-4, "got {}", out[999]);
    }

    /// A gain scheduled after a track's last sample is written changes the
    /// level exactly where the next track's first sample plays: the gapless
    /// join keeps one stream and still gives each track its own gain.
    #[test]
    fn scheduled_gain_lands_where_the_next_track_starts() {
        let (output, mut mixed) = output();
        let mut active = stream(&output, NormalizationConfig::Static(0.5));
        active.write_samples(&[0.2; 2_000]);
        active.schedule_norm_gain(0.25);
        active.write_samples(&[0.2; 2_000]);

        let out = played(&mut mixed, 4_000);

        assert!(
            out[..2_000].iter().all(|s| (s - 0.1).abs() < 1e-4),
            "the outgoing track keeps its gain to its last sample"
        );
        assert!(
            out[2_000] < 0.0999,
            "the next track's first sample starts the switch"
        );
        assert!(
            (out[3_999] - 0.05).abs() < 1e-4,
            "the next track settles at its own gain, got {}",
            out[3_999]
        );
        assert!((active.write_gain() - 0.25).abs() < f32::EPSILON);
    }
}
