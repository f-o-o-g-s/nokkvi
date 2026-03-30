//! Audio renderer — manages streaming sources on the rodio mixer.
//!
//! Replaces the PipeWire-based push model with a pull-based architecture:
//! - Decoded f32 samples are written to ring buffers via `ActiveStream`
//! - rodio/cpal pulls samples on the audio callback thread
//! - Crossfade uses two concurrent streams with volume ramps
//! - Position is tracked via atomic sample counters
//! - Visualizer tap is built into `StreamingSource`

use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use anyhow::Result;
use tokio::sync::Mutex;
use tracing::{debug, trace, warn};

use crate::audio::{
    AudioFormat,
    rodio_output::{ActiveStream, RodioOutput},
    streaming_source::SharedVisualizerCallback,
};

/// Callback for emitting audio samples to visualizer.
/// The callback receives f32 samples scaled to S16 range, and the sample rate.
pub type VisualizerCallback = crate::audio::streaming_source::VisualizerCallback;

/// Audio renderer that manages streaming sources on the rodio mixer.
pub struct AudioRenderer {
    /// The rodio output — holds the cpal stream and mixer.
    output: Option<RodioOutput>,
    /// Primary audio stream (current track).
    primary_stream: Option<ActiveStream>,
    /// Crossfade audio stream (incoming track during crossfade).
    crossfade_stream: Option<ActiveStream>,

    /// Current audio format from the decoder.
    format: AudioFormat,
    /// Previous track's format (for gapless detection).
    prev_format: AudioFormat,

    /// Playback state.
    playing: bool,
    paused: bool,
    /// Seek offset in milliseconds (added to sample-counter position).
    position_offset: u64,

    /// Volume (0.0–1.0).
    volume: f64,
    gapless_enabled: bool,
    finished_called: bool,

    /// Engine back-reference for completion callbacks.
    pub engine: Weak<Mutex<super::engine::CustomAudioEngine>>,
    tokio_handle: tokio::runtime::Handle,
    /// Source generation counter — shared with engine for stale callback detection.
    pub source_generation: Arc<AtomicU64>,
    /// Set by the engine's decode loop when the primary decoder reaches EOF.
    pub decoder_eof: Arc<AtomicBool>,

    // ---- Crossfade state ----
    crossfade_active: bool,
    crossfade_duration_ms: u64,
    crossfade_start_time: Option<std::time::Instant>,
    crossfade_incoming_format: AudioFormat,
    crossfade_finalized_elapsed_ms: u64,

    // ---- Crossfade trigger (renderer-side, like MPD) ----
    crossfade_armed: bool,
    crossfade_armed_duration_ms: u64,
    crossfade_armed_incoming_format: AudioFormat,
    crossfade_armed_track_duration_ms: u64,

    /// Shared visualizer callback slot. Owned by the renderer, shared with
    /// all `RodioOutput` instances and their `StreamingSource`s.
    viz_callback: SharedVisualizerCallback,

    /// Shared mixer from the app-wide MixerDeviceSink (set after login).
    shared_mixer: Option<rodio::mixer::Mixer>,

    /// Whether volume normalization (AGC) is enabled for new streams.
    volume_normalization: bool,
    /// AGC target level for new streams.
    normalization_target_level: f32,
    /// Shared EQ state — passed to each new StreamingSource.
    eq_state: Option<super::eq::EqState>,
    /// When `true`, PipeWire handles the user's volume via `channelVolumes`.
    /// Software volume is kept at 1.0 during normal playback; crossfade
    /// ramps use only the fade factor (PipeWire applies user volume on top).
    pw_volume_active: bool,
}

// ---- Volume normalization setters (outside the main impl for visibility) ----
impl AudioRenderer {
    /// Update volume normalization settings. Takes effect on the next stream creation.
    pub fn set_volume_normalization(&mut self, enabled: bool, target_level: f32) {
        self.volume_normalization = enabled;
        self.normalization_target_level = target_level;
    }

    /// Update shared EQ state. Replaces existing eq state, taking effect on new streams.
    pub fn set_eq_state(&mut self, state: super::eq::EqState) {
        self.eq_state = Some(state);
    }
}

impl AudioRenderer {
    pub fn new() -> Self {
        Self {
            output: None,
            primary_stream: None,
            crossfade_stream: None,
            format: AudioFormat::invalid(),
            prev_format: AudioFormat::invalid(),
            playing: false,
            paused: false,
            position_offset: 0,
            volume: 1.0,
            gapless_enabled: true,
            finished_called: false,
            engine: Weak::new(),
            tokio_handle: tokio::runtime::Handle::current(),
            source_generation: Arc::new(AtomicU64::new(0)),
            decoder_eof: Arc::new(AtomicBool::new(false)),
            crossfade_active: false,
            crossfade_duration_ms: 0,
            crossfade_start_time: None,
            crossfade_incoming_format: AudioFormat::invalid(),
            crossfade_finalized_elapsed_ms: 0,
            crossfade_armed: false,
            crossfade_armed_duration_ms: 0,
            crossfade_armed_incoming_format: AudioFormat::invalid(),
            crossfade_armed_track_duration_ms: 0,
            viz_callback: std::sync::Arc::new(parking_lot::RwLock::new(None)),
            shared_mixer: None,
            volume_normalization: false,
            normalization_target_level: 1.0,
            eq_state: None,
            pw_volume_active: false,
        }
    }

    // =========================================================================
    // Lifecycle
    // =========================================================================

    /// Initialize with track format.
    /// Creates the rodio output (if needed) and a new primary stream.
    pub fn init(
        &mut self,
        format: &AudioFormat,
        force_reload: bool,
        prev_format: Option<&AudioFormat>,
    ) -> Result<()> {
        debug!(
            "📡 Renderer::init() format={:?}, force_reload={}, output={}, primary={}",
            format,
            force_reload,
            self.output.is_some(),
            self.primary_stream.is_some()
        );
        if !format.is_valid() {
            anyhow::bail!("Cannot initialize with invalid format");
        }

        let old_format = self.format.clone();
        self.prev_format = prev_format.cloned().unwrap_or_else(|| old_format.clone());

        // Check if gapless is possible (formats match and gapless enabled)
        let is_gapless = !force_reload
            && self.gapless_enabled
            && self.prev_format.is_valid()
            && *format == self.prev_format
            && self.primary_stream.is_some();

        if is_gapless {
            // Formats match — reuse existing stream for gapless playback.
            debug!("📡 Renderer::init() GAPLESS path — reusing stream");
            self.format = format.clone();
            self.position_offset = 0;
            if let Some(ref stream) = self.primary_stream {
                stream.reset_position();
            }
            return Ok(());
        }

        // Format changed or first init — create new stream.
        self.format = format.clone();

        // Ensure the rodio output is initialized
        if self.output.is_none() {
            let mixer = self.shared_mixer.clone().ok_or_else(|| {
                anyhow::anyhow!("Shared mixer not set — call set_shared_mixer() first")
            })?;
            self.output = Some(RodioOutput::new(mixer, self.viz_callback.clone())?);
        }

        // Stop old primary stream if any
        if let Some(old_stream) = self.primary_stream.take() {
            old_stream.silence_and_stop();
        }

        // Create new primary stream
        let output = self
            .output
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Audio output not initialized"))?;

        let stream = output.create_stream(
            format.sample_rate(),
            format.channel_count() as u16,
            self.stream_volume(),
            self.volume_normalization,
            self.normalization_target_level,
            self.eq_state.clone(),
        );

        debug!(
            "📡 Renderer::init() NEW STREAM created: {}ch, {}Hz, vol={:.2}",
            format.channel_count(),
            format.sample_rate(),
            self.volume
        );

        self.primary_stream = Some(stream);
        self.position_offset = 0;

        Ok(())
    }

    /// Write decoded f32 samples to the primary stream.
    /// Returns the number of samples actually written.
    pub fn write_samples(&mut self, samples: &[f32]) -> usize {
        if let Some(ref mut stream) = self.primary_stream {
            stream.write_samples(samples)
        } else {
            warn!(
                "📡 Renderer::write_samples({}) — NO primary_stream!",
                samples.len()
            );
            0
        }
    }

    /// Check how many samples can be written to the primary stream.
    pub fn available_space(&self) -> usize {
        self.primary_stream
            .as_ref()
            .map_or(0, |s| s.available_space())
    }

    /// Reset position tracking for a new track.
    pub fn reset_position(&mut self) {
        self.position_offset = 0;
        if let Some(ref stream) = self.primary_stream {
            stream.reset_position();
        }
    }

    /// Reset position tracking with an offset (e.g., after crossfade).
    pub fn reset_position_with_offset(&mut self, offset_ms: u64) {
        self.position_offset = offset_ms;
        if let Some(ref stream) = self.primary_stream {
            stream.reset_position();
        }
    }

    /// Reset the finished_called flag.
    pub fn reset_finished_called(&mut self) {
        self.finished_called = false;
    }

    /// Start playback.
    pub fn start(&mut self) {
        trace!(
            "▶ Renderer::start() playing={}, paused={}, decoder_eof={}, buffer={}, primary={}",
            self.playing,
            self.paused,
            self.decoder_eof.load(Ordering::Acquire),
            self.buffer_count(),
            self.primary_stream.is_some(),
        );

        if self.playing && !self.paused {
            trace!("▶ Renderer::start() already playing, returning early");
            return;
        }

        self.playing = true;
        self.paused = false;
        self.finished_called = false;
        // CRITICAL: Reset decoder_eof HERE, not just in start_decoding_loop().
        // Without this, the render thread can see stale decoder_eof=true between
        // renderer.start() and start_decoding_loop(), triggering false track completion.
        self.decoder_eof.store(false, Ordering::Release);

        // Ensure streams are unpaused (the `paused` atomic on each StreamHandle
        // must be cleared — otherwise the source returns silence).
        if let Some(ref stream) = self.primary_stream {
            stream.resume();
            stream.set_volume(self.stream_volume());
        }
        if let Some(ref stream) = self.crossfade_stream {
            stream.resume();
        }

        trace!("▶ Renderer::start() completed — stream active");
    }

    /// Stop playback.
    pub fn stop(&mut self) {
        trace!("⏹ Renderer: stop() called");
        self.playing = false;
        self.paused = false;
        self.finished_called = false;

        // Cancel crossfade and disarm any pending crossfade trigger
        self.cancel_crossfade();
        self.disarm_crossfade();

        // Stop the primary stream
        if let Some(stream) = self.primary_stream.take() {
            stream.silence_and_stop();
        }

        self.position_offset = 0;
        trace!("⏹ Renderer: stop() completed");
    }

    /// Pause playback.
    pub fn pause(&mut self) {
        if !self.playing {
            return;
        }
        self.paused = true;
        // Pause the streaming source — it will emit silence and stop
        // counting samples, so position freezes correctly.
        if let Some(ref stream) = self.primary_stream {
            stream.pause();
        }
        if let Some(ref stream) = self.crossfade_stream {
            stream.pause();
        }
    }

    /// Resume from pause.
    pub fn resume(&mut self) {
        if !self.paused {
            return;
        }
        self.paused = false;
        // Resume the streaming source — it will start pulling and counting again.
        if let Some(ref stream) = self.primary_stream {
            stream.resume();
        }
        if let Some(ref stream) = self.crossfade_stream {
            stream.resume();
        }
        // Restore volume (may have been changed during pause via set_volume)
        if !self.crossfade_active
            && let Some(ref stream) = self.primary_stream
        {
            stream.set_volume(self.stream_volume());
        }
        // If crossfade is active, volumes are managed by the crossfade tick
    }

    /// Seek to position (milliseconds).
    pub fn seek(&mut self, position_ms: u64) {
        // Cancel crossfade on seek
        self.cancel_crossfade();

        self.position_offset = position_ms;
        self.finished_called = false;

        // Clear the ring buffer by stopping and recreating the stream
        if let Some(old_stream) = self.primary_stream.take() {
            old_stream.silence_and_stop();
        }

        // Recreate the primary stream (if output exists)
        if let Some(ref output) = self.output {
            let stream = output.create_stream(
                self.format.sample_rate(),
                self.format.channel_count() as u16,
                self.stream_volume(),
                self.volume_normalization,
                self.normalization_target_level,
                self.eq_state.clone(),
            );
            self.primary_stream = Some(stream);
        }
    }

    /// Set volume (0.0–1.0).
    ///
    /// When `pw_volume_active` is true, only stores the volume — actual
    /// attenuation is handled by PipeWire channelVolumes. Software volume
    /// on the stream stays at 1.0 during normal playback.
    pub fn set_volume(&mut self, volume: f64) {
        self.volume = volume.clamp(0.0, 1.0);
        if self.pw_volume_active {
            // PipeWire handles user volume — software stays at unity.
            // Nothing to do here; PipeWire volume is set via SfxEngine.
            return;
        }
        if self.crossfade_active {
            // During crossfade, volumes are managed by the crossfade tick.
            // Just store the new volume — next tick applies it proportionally.
        } else if !self.paused
            && let Some(ref stream) = self.primary_stream
        {
            stream.set_volume(volume as f32);
        }
    }

    /// Get current position (milliseconds).
    pub fn position(&self) -> u64 {
        if !self.playing || self.paused {
            return self.position_offset;
        }

        if let Some(ref stream) = self.primary_stream {
            self.position_offset + stream.position_ms()
        } else {
            self.position_offset
        }
    }

    /// Get current format.
    pub fn format(&self) -> &AudioFormat {
        &self.format
    }

    /// Check if a primary stream exists.
    /// Used by engine to detect when init() must be called even if the format matches
    /// (e.g., after stop() destroyed the stream).
    pub fn has_primary_stream(&self) -> bool {
        self.primary_stream.is_some()
    }

    /// Check if buffer queue is empty (for completion detection).
    /// With rodio, we check if the ring buffer is empty.
    pub fn is_buffer_queue_empty(&self) -> bool {
        self.buffer_count() == 0
    }

    /// Get buffer queue count (for backpressure control).
    /// Returns approximate samples in the ring buffer.
    pub fn buffer_count(&self) -> usize {
        self.primary_stream.as_ref().map_or(0, |s| {
            crate::audio::rodio_output::RING_BUFFER_CAPACITY.saturating_sub(s.available_space())
        })
    }

    /// Set visualizer callback. Works even if the output doesn't exist yet
    /// because the shared slot is owned by the renderer.
    pub fn set_visualizer_callback(&self, callback: VisualizerCallback) {
        *self.viz_callback.write() = Some(callback);
    }

    /// Set the shared mixer from the app-wide MixerDeviceSink.
    /// Must be called before the first `init()` / `play()`.
    pub fn set_shared_mixer(&mut self, mixer: rodio::mixer::Mixer) {
        self.shared_mixer = Some(mixer);
    }

    /// Enable/disable PipeWire-native volume control.
    ///
    /// When `true`, the renderer keeps software volume at 1.0 (unity) and
    /// expects the caller to mirror volume changes to PipeWire via
    /// `SfxEngine::set_output_volume()`. Crossfade ramps use only the
    /// fade coefficient — PipeWire applies the user's volume uniformly
    /// to the entire mixed output.
    pub fn set_pw_volume_active(&mut self, active: bool) {
        self.pw_volume_active = active;
        tracing::info!(
            "🔊 Renderer: PipeWire native volume {}",
            if active { "ENABLED" } else { "disabled" }
        );
    }

    /// The software volume to apply to streams during normal playback.
    /// Returns 1.0 when PipeWire handles the user's volume, otherwise
    /// returns the stored user volume.
    fn stream_volume(&self) -> f32 {
        if self.pw_volume_active { 1.0 } else { self.volume as f32 }
    }

    // =========================================================================
    // Crossfade API
    // =========================================================================

    /// Minimum song duration for crossfade eligibility.
    /// Songs shorter than this fall back to gapless transition.
    /// MPD uses 20s; we use 10s since our crossfade is user-configurable (1-12s).
    const MIN_CROSSFADE_TRACK_MS: u64 = 10_000;

    /// Arm the renderer for crossfade with duration clamping.
    ///
    /// Guards (inspired by MPD's `CanCrossFadeSong`):
    /// 1. Both songs must be >= MIN_CROSSFADE_TRACK_MS (10s)
    /// 2. Effective duration is clamped to `min(xfade, track/2)` so the
    ///    outgoing track always has real audio for at least half the fade
    pub fn arm_crossfade(
        &mut self,
        duration_ms: u64,
        incoming_format: &AudioFormat,
        track_duration_ms: u64,
        incoming_duration_ms: u64,
    ) {
        // Guard: skip crossfade for short songs (fall back to gapless)
        let min_dur = track_duration_ms.min(incoming_duration_ms);
        if min_dur < Self::MIN_CROSSFADE_TRACK_MS {
            debug!(
                "🔀 [RENDERER] Crossfade SKIPPED: shortest track {}ms < {}ms minimum",
                min_dur,
                Self::MIN_CROSSFADE_TRACK_MS,
            );
            return;
        }

        // Clamp: effective crossfade ≤ half the shorter track
        let max_xfade = min_dur / 2;
        let effective = duration_ms.min(max_xfade);

        if effective != duration_ms {
            debug!(
                "🔀 [RENDERER] Crossfade CLAMPED: {}ms → {}ms (track={}ms, incoming={}ms)",
                duration_ms, effective, track_duration_ms, incoming_duration_ms,
            );
        }

        self.crossfade_armed = true;
        self.crossfade_armed_duration_ms = effective;
        self.crossfade_armed_incoming_format = incoming_format.clone();
        self.crossfade_armed_track_duration_ms = track_duration_ms;
        debug!(
            "🔀 [RENDERER] Crossfade ARMED: duration={}ms, track={}ms, incoming={:?}",
            effective, track_duration_ms, incoming_format
        );
    }

    /// Disarm the crossfade trigger.
    pub fn disarm_crossfade(&mut self) {
        self.crossfade_armed = false;
        self.crossfade_armed_duration_ms = 0;
        self.crossfade_armed_incoming_format = AudioFormat::invalid();
        self.crossfade_armed_track_duration_ms = 0;
    }

    /// Start a crossfade transition.
    /// Creates a new stream for the incoming track and begins volume ramping.
    pub fn start_crossfade(&mut self, duration_ms: u64, incoming_format: &AudioFormat) {
        if duration_ms == 0 {
            return;
        }

        // Create the crossfade stream
        let output = match self.output.as_ref() {
            Some(o) => o,
            None => {
                warn!("🔀 [RENDERER] Cannot start crossfade — no audio output");
                return;
            }
        };

        let cf_stream = output.create_stream(
            incoming_format.sample_rate(),
            incoming_format.channel_count() as u16,
            0.0, // Start silent, volume will ramp up
            self.volume_normalization,
            self.normalization_target_level,
            self.eq_state.clone(),
        );

        self.crossfade_stream = Some(cf_stream);
        self.crossfade_active = true;
        self.crossfade_duration_ms = duration_ms;
        self.crossfade_start_time = Some(std::time::Instant::now());
        self.crossfade_incoming_format = incoming_format.clone();

        debug!(
            "🔀 [RENDERER] Crossfade STARTED: {}ms, incoming={:?}",
            duration_ms, incoming_format
        );
    }

    /// Cancel an active crossfade.
    pub fn cancel_crossfade(&mut self) {
        if self.crossfade_active {
            debug!(
                "🔀 [RENDERER] Crossfade CANCELLED: elapsed={:?}ms/{}ms",
                self.crossfade_start_time.map(|t| t.elapsed().as_millis()),
                self.crossfade_duration_ms,
            );
        }
        self.crossfade_active = false;
        self.crossfade_duration_ms = 0;
        self.crossfade_start_time = None;
        self.crossfade_incoming_format = AudioFormat::invalid();

        // Stop crossfade stream
        if let Some(stream) = self.crossfade_stream.take() {
            stream.silence_and_stop();
        }

        // Restore primary volume
        if !self.paused
            && let Some(ref stream) = self.primary_stream
        {
            stream.set_volume(self.stream_volume());
        }
    }

    /// Write decoded f32 samples to the crossfade (incoming) stream.
    pub fn write_crossfade_samples(&mut self, samples: &[f32]) -> usize {
        if let Some(ref mut stream) = self.crossfade_stream {
            stream.write_samples(samples)
        } else {
            0
        }
    }

    /// Check available space in the crossfade stream.
    pub fn crossfade_available_space(&self) -> usize {
        self.crossfade_stream
            .as_ref()
            .map_or(0, |s| s.available_space())
    }

    /// Whether a crossfade is currently in progress.
    pub fn is_crossfade_active(&self) -> bool {
        self.crossfade_active
    }

    /// Get crossfade buffer count (approximate samples in crossfade ring buffer).
    pub fn crossfade_buffer_count(&self) -> usize {
        self.crossfade_stream.as_ref().map_or(0, |s| {
            crate::audio::rodio_output::RING_BUFFER_CAPACITY.saturating_sub(s.available_space())
        })
    }

    /// Tick the crossfade: update volumes based on elapsed time.
    /// Called periodically (e.g., from the decode loop or a timer).
    /// Returns `true` if the crossfade should be finalized.
    pub fn tick_crossfade(&mut self) -> bool {
        if !self.crossfade_active {
            return false;
        }

        let elapsed_ms = self
            .crossfade_start_time
            .map_or(0, |t| t.elapsed().as_millis() as u64);

        let progress = if self.crossfade_duration_ms > 0 {
            (elapsed_ms as f64 / self.crossfade_duration_ms as f64).min(1.0)
        } else {
            1.0
        };

        // Equal-power crossfade using cos²/sin² curves
        let fade_out = (progress * std::f64::consts::FRAC_PI_2).cos().powi(2);
        let fade_in = (progress * std::f64::consts::FRAC_PI_2).sin().powi(2);

        // When PipeWire handles user volume, software only applies the fade
        // coefficient. PipeWire applies the user's volume uniformly to the
        // combined mixer output, so the product is correctly fade × user_vol.
        let user_vol = if self.pw_volume_active {
            1.0
        } else {
            self.volume as f32
        };
        if let Some(ref stream) = self.primary_stream {
            stream.set_volume((fade_out * user_vol as f64) as f32);
        }
        if let Some(ref stream) = self.crossfade_stream {
            stream.set_volume((fade_in * user_vol as f64) as f32);
        }

        progress >= 1.0
    }

    /// Finalize the crossfade: swap crossfade stream → primary stream.
    /// Returns the elapsed crossfade time in milliseconds (for position offset).
    pub fn finalize_crossfade(&mut self) -> u64 {
        if !self.crossfade_active {
            return 0;
        }

        let elapsed_ms = self
            .crossfade_start_time
            .map_or(0, |t| t.elapsed().as_millis() as u64);

        debug!(
            "🔀 [RENDERER] Crossfade FINALIZED: elapsed={}ms/{}ms",
            elapsed_ms, self.crossfade_duration_ms,
        );

        // Stop old primary, promote crossfade stream to primary
        if let Some(old_primary) = self.primary_stream.take() {
            old_primary.silence_and_stop();
        }
        self.primary_stream = self.crossfade_stream.take();

        // Set new primary to full user volume
        if let Some(ref stream) = self.primary_stream {
            stream.set_volume(self.stream_volume());
        }

        // Update format to the incoming track's format
        self.format = self.crossfade_incoming_format.clone();

        // Reset crossfade state
        self.crossfade_active = false;
        self.crossfade_duration_ms = 0;
        self.crossfade_start_time = None;
        self.crossfade_incoming_format = AudioFormat::invalid();
        self.disarm_crossfade();

        // Store for engine to read as position offset
        self.crossfade_finalized_elapsed_ms = elapsed_ms;
        elapsed_ms
    }

    /// Consume the stored crossfade elapsed time.
    pub fn take_crossfade_elapsed_ms(&mut self) -> u64 {
        std::mem::take(&mut self.crossfade_finalized_elapsed_ms)
    }

    /// Check if crossfade is armed.
    pub fn is_crossfade_armed(&self) -> bool {
        self.crossfade_armed
    }

    /// Get the armed crossfade duration.
    pub fn crossfade_armed_duration_ms(&self) -> u64 {
        self.crossfade_armed_duration_ms
    }

    /// Get the armed crossfade incoming format.
    pub fn crossfade_armed_incoming_format(&self) -> &AudioFormat {
        &self.crossfade_armed_incoming_format
    }

    // =========================================================================
    // Render loop (called periodically from engine render thread)
    // =========================================================================

    /// Check for track completion and crossfade trigger.
    /// This replaces the old `render_buffers()` — with rodio, the actual audio
    /// rendering is done by the cpal callback thread. This method just handles
    /// the control logic.
    pub fn render_tick(&mut self) {
        // Periodic state dump for diagnostics (every ~5 seconds)
        static TICK_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let tick = TICK_COUNT.fetch_add(1, Ordering::Relaxed);
        if tick.is_multiple_of(250) {
            debug!(
                "🔍 [RENDER_TICK #{tick}] playing={}, paused={}, eof={}, buf_empty={}, \
                 buf_count={}, primary={}, xfade_active={}, xfade_armed={}, finished={}",
                self.playing,
                self.paused,
                self.decoder_eof.load(Ordering::Acquire),
                self.is_buffer_queue_empty(),
                self.buffer_count(),
                self.primary_stream.is_some(),
                self.crossfade_active,
                self.crossfade_armed,
                self.finished_called,
            );
        }

        if !self.playing || self.paused {
            return;
        }

        // Tick crossfade volumes if active
        if self.crossfade_active && self.tick_crossfade() {
            // Crossfade duration expired — finalize the renderer-side crossfade
            // synchronously (we already hold the lock). This swaps the crossfade
            // stream to primary and resets crossfade_active. Then signal the engine
            // to swap decoders/sources.
            debug!("🔀 [RENDER_TICK] Crossfade complete — finalizing renderer + signaling engine");
            self.finalize_crossfade();
            self.on_renderer_finished();
            return; // Don't run further checks this tick
        }

        // Check for crossfade trigger: position-based, NOT EOF-based.
        // We start the crossfade `crossfade_duration_ms` before the track ends,
        // so the outgoing track still has audio in its buffer to fade out.
        // Falls back to EOF if duration is unknown (0).
        if self.crossfade_armed {
            let pos = self.position();
            let track_dur = self.crossfade_armed_track_duration_ms;
            let xfade_dur = self.crossfade_armed_duration_ms;
            let trigger = if track_dur > 0 && xfade_dur > 0 {
                pos >= track_dur.saturating_sub(xfade_dur)
            } else {
                // Unknown duration — fall back to EOF
                self.decoder_eof.load(Ordering::Acquire)
            };
            if trigger {
                debug!(
                    "🔀 [RENDER_TICK] Crossfade trigger — pos={}ms, track={}ms, xfade={}ms",
                    pos, track_dur, xfade_dur
                );
                // Capture armed values before disarm clears them
                let cf_duration = self.crossfade_armed_duration_ms;
                let cf_format = self.crossfade_armed_incoming_format.clone();
                self.disarm_crossfade();

                // Start the renderer-side crossfade SYNCHRONOUSLY so that
                // crossfade_active=true prevents the track-completion check
                // from firing before the async engine task can respond.
                // The engine's start_crossfade() skips renderer.start_crossfade()
                // if we've already activated it here.
                self.start_crossfade(cf_duration, &cf_format);

                // Signal the engine async to set up the decoder and decode loop
                self.on_renderer_finished();
                return;
            }
        }

        // Check for track completion (ring buffer empty + decoder EOF + not crossfading)
        if !self.crossfade_active
            && !self.finished_called
            && self.decoder_eof.load(Ordering::Acquire)
            && self.is_buffer_queue_empty()
        {
            trace!(
                "🏁 [RENDER_TICK] Track finished! decoder_eof=true, buffer_empty=true, \
                 buffer_count={}, primary_stream={}",
                self.buffer_count(),
                self.primary_stream.is_some()
            );
            self.finished_called = true;
            self.on_renderer_finished();
        }
    }

    /// Called when the current track finishes playing.
    ///
    /// Uses `tokio::spawn` instead of `std::thread::spawn` + `block_on` to
    /// avoid deadlocking when the engine lock is already held in the same
    /// call chain (e.g. render_tick → finalize → engine lock).
    fn on_renderer_finished(&mut self) {
        let generation = self.source_generation.load(Ordering::Acquire);
        trace!("🏁 [RENDERER] Track finished (generation={})", generation);

        if let Some(engine_ref) = self.engine.upgrade() {
            let handle = self.tokio_handle.clone();
            let src_gen = generation;
            handle.spawn(async move {
                let mut engine = engine_ref.lock().await;
                // Verify generation hasn't changed (skip raced with completion)
                if engine.source_generation() == src_gen {
                    engine.on_renderer_finished().await;
                }
            });
        }
    }
}

impl Default for AudioRenderer {
    fn default() -> Self {
        Self::new()
    }
}
