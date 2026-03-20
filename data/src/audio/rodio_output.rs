//! Rodio-based audio output manager.
//!
//! Replaces the PipeWire-specific `AudioOutput` with a cross-platform rodio/cpal
//! implementation. Uses a shared `Mixer` (from the app-wide `MixerDeviceSink`)
//! to add streaming sources. All audio (music + SFX) flows through one cpal stream.

use std::num::NonZero;

use anyhow::Result;
use ringbuf::{HeapRb, traits::Split};
use rodio::mixer::Mixer;
use tracing::{debug, info};

use super::streaming_source::{SharedVisualizerCallback, StreamHandle, StreamingSource};

/// Default ring buffer capacity in samples.
/// 48000 Hz × 2 channels × 2 seconds = 192000 samples.
/// Two seconds of buffering absorbs decoder stalls while keeping memory
/// reasonable (~750KB per stream).
pub const RING_BUFFER_CAPACITY: usize = 192_000;

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
}

impl ActiveStream {
    /// Write decoded f32 samples to the stream.
    /// Returns the number of samples actually written (may be less if the ring buffer is full).
    pub fn write_samples(&mut self, samples: &[f32]) -> usize {
        use ringbuf::traits::Producer;
        self.producer.push_slice(samples)
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
}

impl RodioOutput {
    /// Create a new audio output using a shared mixer.
    ///
    /// The `mixer` should come from the app-wide `MixerDeviceSink` (typically
    /// owned by the SFX engine). The `viz_callback` is the shared visualizer
    /// callback slot owned by the renderer.
    pub fn new(mixer: Mixer, viz_callback: SharedVisualizerCallback) -> Result<Self> {
        info!("🔊 [RODIO] Music output initialized (shared mixer)");

        Ok(Self {
            mixer,
            visualizer_callback: viz_callback,
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
    /// - `volume_normalization`: Whether AGC is enabled.
    /// - `normalization_target_level`: AGC target level (e.g. 0.6, 1.0, 1.4).
    pub fn create_stream(
        &self,
        sample_rate: u32,
        channels: u16,
        initial_volume: f32,
        volume_normalization: bool,
        normalization_target_level: f32,
    ) -> ActiveStream {
        // Create lock-free ring buffer
        let rb = HeapRb::<f32>::new(RING_BUFFER_CAPACITY);
        let (producer, consumer) = rb.split();

        let channels_nz = NonZero::new(channels).unwrap_or(NonZero::new(2).unwrap());
        let sample_rate_nz = NonZero::new(sample_rate).unwrap_or(NonZero::new(44100).unwrap());

        // Create the streaming source with initial volume
        let (source, handle) = StreamingSource::new(
            consumer,
            channels_nz,
            sample_rate_nz,
            self.visualizer_callback.clone(),
            initial_volume,
        );

        // Chain AGC (when enabled) then peak limiter before adding to mixer.
        // AGC runs before the limiter so any gain boost is clamped, preventing clipping.
        use rodio::source::{AutomaticGainControlSettings, LimitSettings, Source};
        if volume_normalization {
            let agc_settings = AutomaticGainControlSettings {
                target_level: normalization_target_level,
                ..AutomaticGainControlSettings::default()
            };
            self.mixer.add(
                source
                    .automatic_gain_control(agc_settings)
                    .limit(LimitSettings::dynamic_content()),
            );
            debug!(
                "🔊 [RODIO] Created stream with AGC (target={:.1}): {}ch, {}Hz, vol={:.2}",
                normalization_target_level, channels, sample_rate, initial_volume
            );
        } else {
            self.mixer
                .add(source.limit(LimitSettings::dynamic_content()));
            debug!(
                "🔊 [RODIO] Created stream: {}ch, {}Hz, vol={:.2}",
                channels, sample_rate, initial_volume
            );
        }

        ActiveStream {
            producer,
            handle,
            sample_rate,
            channels,
        }
    }
}
