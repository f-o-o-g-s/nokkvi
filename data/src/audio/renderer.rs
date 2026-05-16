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
    atomic::{AtomicBool, Ordering},
};

use anyhow::Result;
use tokio::sync::{Mutex, Notify};
use tracing::{debug, trace, warn};

use crate::{
    audio::{
        AudioFormat, NormalizationConfig, NormalizationContext, SourceGeneration,
        resolve_normalization,
        rodio_output::{ActiveStream, RodioOutput},
        streaming_source::SharedVisualizerCallback,
    },
    types::{player_settings::VolumeNormalizationMode, song::ReplayGain},
};

/// Callback for emitting audio samples to visualizer.
/// The callback receives f32 samples scaled to S16 range, and the sample rate.
pub type VisualizerCallback = crate::audio::streaming_source::VisualizerCallback;

/// Renderer-side crossfade state machine.
///
/// Variants carry the data each phase needs (the crossfade `ActiveStream`,
/// timing, formats), so impossible states are unrepresentable: `Idle` has no
/// stream, `Armed` has no stream and no start time, and `Active` always has
/// both. Replaces the eight parallel `crossfade_*` fields that previously
/// had to be reset in lockstep.
enum CrossfadeState {
    Idle,
    Armed {
        duration_ms: u64,
        incoming_format: AudioFormat,
        track_duration_ms: u64,
    },
    Active {
        stream: ActiveStream,
        started_at: std::time::Instant,
        duration_ms: u64,
        incoming_format: AudioFormat,
    },
}

/// Audio renderer that manages streaming sources on the rodio mixer.
pub struct AudioRenderer {
    /// The rodio output — holds the cpal stream and mixer.
    output: Option<RodioOutput>,
    /// Primary audio stream (current track).
    primary_stream: Option<ActiveStream>,

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
    engine: Weak<Mutex<super::engine::CustomAudioEngine>>,
    tokio_handle: tokio::runtime::Handle,
    /// Source generation counter — shared with engine for stale callback detection.
    source_generation: SourceGeneration,
    /// Set by the engine's decode loop when the primary decoder reaches EOF.
    decoder_eof: Arc<AtomicBool>,

    /// Crossfade phase + per-phase data. See [`CrossfadeState`].
    crossfade_state: CrossfadeState,
    /// Elapsed crossfade time (ms) staged after `finalize_crossfade` so the
    /// engine can read it on the next render tick as a position offset.
    /// Lives outside `CrossfadeState` because it survives the Active→Idle
    /// transition by exactly one tick.
    crossfade_finalized_elapsed_ms: u64,

    /// Shared visualizer callback slot. Owned by the renderer, shared with
    /// all `RodioOutput` instances and their `StreamingSource`s.
    viz_callback: SharedVisualizerCallback,

    /// Shared mixer from the app-wide MixerDeviceSink (set after login).
    shared_mixer: Option<rodio::mixer::Mixer>,

    /// Volume normalization mode applied to new streams.
    volume_normalization_mode: VolumeNormalizationMode,
    /// AGC target level — only used when mode == Agc (or RG fallback to AGC).
    normalization_target_level: f32,
    /// ReplayGain pre-amp dB applied on top of resolved gain.
    replay_gain_preamp_db: f32,
    /// Fallback dB for tracks with no ReplayGain tags.
    replay_gain_fallback_db: f32,
    /// When true, untagged tracks fall through to AGC instead of fallback dB.
    replay_gain_fallback_to_agc: bool,
    /// When true, clamp gain so `peak * gain <= 1.0`.
    replay_gain_prevent_clipping: bool,
    /// ReplayGain tags to apply to the *next* primary-stream creation
    /// (set by the engine immediately before `init()`/`seek()`).
    pending_replay_gain: Option<ReplayGain>,
    /// ReplayGain tags to apply to the *next* crossfade-stream creation
    /// (set by the engine immediately before `start_crossfade`).
    pending_crossfade_replay_gain: Option<ReplayGain>,
    /// ReplayGain tags currently baked into the live primary stream.
    /// Used by the gapless-reuse guard to detect when a track-mode
    /// transition would leave the wrong gain applied to the new track.
    current_replay_gain: Option<ReplayGain>,
    /// Shared EQ state — passed to each new StreamingSource.
    eq_state: Option<super::eq::EqState>,
    /// When `true`, PipeWire handles the user's volume via `channelVolumes`.
    /// Software volume is kept at 1.0 during normal playback; crossfade
    /// ramps use only the fade factor (PipeWire applies user volume on top).
    pw_volume_active: bool,

    /// Shared notify primitive passed to every `StreamingSource` created by
    /// this renderer.  Fired every `CONSUMED_NOTIFY_STRIDE` real samples; the
    /// decode loop awaits it instead of busy-sleeping when `push_slice`
    /// returns 0 (ring buffer full).  A single `Arc` survives across seek /
    /// stream-recreation so the decode loop never needs to re-capture it.
    consumed_notify: Arc<Notify>,
}

// ---- Volume normalization setters (outside the main impl for visibility) ----
impl AudioRenderer {
    /// Update volume normalization settings. Takes effect on the next stream creation.
    pub fn set_volume_normalization(
        &mut self,
        mode: VolumeNormalizationMode,
        target_level: f32,
        preamp_db: f32,
        fallback_db: f32,
        fallback_to_agc: bool,
        prevent_clipping: bool,
    ) {
        self.volume_normalization_mode = mode;
        self.normalization_target_level = target_level;
        self.replay_gain_preamp_db = preamp_db;
        self.replay_gain_fallback_db = fallback_db;
        self.replay_gain_fallback_to_agc = fallback_to_agc;
        self.replay_gain_prevent_clipping = prevent_clipping;
    }

    /// Set the ReplayGain tags for the next primary-stream creation
    /// (`init` or `seek`). Called by the engine immediately before the
    /// stream is rebuilt; ignored otherwise.
    pub fn set_pending_replay_gain(&mut self, rg: Option<ReplayGain>) {
        self.pending_replay_gain = rg;
    }

    /// Set the ReplayGain tags for the next crossfade-stream creation.
    pub fn set_pending_crossfade_replay_gain(&mut self, rg: Option<ReplayGain>) {
        self.pending_crossfade_replay_gain = rg;
    }

    /// Move the staged crossfade RG into the current slot. Called after a
    /// successful gapless decoder-swap that reuses the same rodio stream.
    pub fn adopt_pending_crossfade_replay_gain(&mut self) {
        self.current_replay_gain = self.pending_crossfade_replay_gain.take();
    }

    /// In ReplayGain-track mode, the rodio chain's `amplify` factor is
    /// baked in at stream creation. The decode-loop's gapless swap reuses
    /// the same primary stream, which would mis-level the next track.
    ///
    /// Returns `false` only when mode is `ReplayGainTrack` *and* the
    /// staged next track has a different `track_gain` than the live
    /// stream — denying the swap forces the engine to take the natural
    /// EOF → reload path, which calls `init()` and creates a fresh
    /// stream with the correct gain.
    ///
    /// Album mode is unaffected (same album → same album_gain).
    pub fn gapless_swap_allowed(&self) -> bool {
        if self.volume_normalization_mode != VolumeNormalizationMode::ReplayGainTrack {
            return true;
        }
        !rg_track_gains_differ(
            self.current_replay_gain.as_ref(),
            self.pending_crossfade_replay_gain.as_ref(),
        )
    }

    /// Resolve mode + settings + an optional `ReplayGain` into the final
    /// per-stream config consumed by `RodioOutput::create_stream`.
    fn resolve_norm_for(&self, rg: Option<&ReplayGain>) -> NormalizationConfig {
        resolve_normalization(NormalizationContext {
            mode: self.volume_normalization_mode,
            agc_target_level: self.normalization_target_level,
            replay_gain_preamp_db: self.replay_gain_preamp_db,
            replay_gain_fallback_db: self.replay_gain_fallback_db,
            replay_gain_fallback_to_agc: self.replay_gain_fallback_to_agc,
            replay_gain_prevent_clipping: self.replay_gain_prevent_clipping,
            replay_gain: rg,
        })
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
            source_generation: SourceGeneration::new(),
            decoder_eof: Arc::new(AtomicBool::new(false)),
            crossfade_state: CrossfadeState::Idle,
            crossfade_finalized_elapsed_ms: 0,
            viz_callback: std::sync::Arc::new(parking_lot::RwLock::new(None)),
            shared_mixer: None,
            volume_normalization_mode: VolumeNormalizationMode::Off,
            normalization_target_level: 1.0,
            replay_gain_preamp_db: 0.0,
            replay_gain_fallback_db: 0.0,
            replay_gain_fallback_to_agc: false,
            replay_gain_prevent_clipping: true,
            pending_replay_gain: None,
            pending_crossfade_replay_gain: None,
            current_replay_gain: None,
            eq_state: None,
            pw_volume_active: false,
            consumed_notify: Arc::new(Notify::new()),
        }
    }

    /// Return the consume-notify primitive so the decode loop can await it.
    ///
    /// The returned `Arc` is stable across seek / stream-recreation; the decode
    /// loop captures it once at spawn time and never needs to refresh it.
    pub fn consumed_notify(&self) -> &Arc<Notify> {
        &self.consumed_notify
    }

    /// Sealed setter — the only path to install the engine back-link, the
    /// source-generation handle, and the EOF flag. Replaces the historical
    /// pub-field assignment in `engine.set_engine_reference`.
    pub fn set_engine_link(
        &mut self,
        engine: Weak<Mutex<super::engine::CustomAudioEngine>>,
        source_generation: SourceGeneration,
        decoder_eof: Arc<AtomicBool>,
    ) {
        self.engine = engine;
        self.source_generation = source_generation;
        self.decoder_eof = decoder_eof;
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
        trace!(
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

        // Check if gapless is possible (formats match and gapless enabled).
        // RG-track mode also requires the per-track gain to be unchanged —
        // gapless reuse keeps the existing rodio chain (with the previous
        // track's `amplify` factor), which would mis-level the new track.
        // Album mode is unaffected: same album → same album_gain by definition.
        let rg_blocks_gapless = self.volume_normalization_mode
            == VolumeNormalizationMode::ReplayGainTrack
            && rg_track_gains_differ(
                self.current_replay_gain.as_ref(),
                self.pending_replay_gain.as_ref(),
            );
        let is_gapless = !force_reload
            && self.gapless_enabled
            && self.prev_format.is_valid()
            && *format == self.prev_format
            && self.primary_stream.is_some()
            && !rg_blocks_gapless;

        if rg_blocks_gapless {
            debug!("📡 Renderer::init() RG-track gain differs — forcing fresh stream");
        }

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

        let norm = self.resolve_norm_for(self.pending_replay_gain.as_ref());
        let stream = output.create_stream(
            format.sample_rate(),
            format.channel_count() as u16,
            self.stream_volume(),
            norm,
            self.eq_state.clone(),
            self.consumed_notify.clone(),
            true,
        );

        debug!(
            "📡 Renderer::init() NEW STREAM created: {}ch, {}Hz, vol={:.2}, norm={:?}",
            format.channel_count(),
            format.sample_rate(),
            self.volume,
            norm
        );

        self.primary_stream = Some(stream);
        self.current_replay_gain = self.pending_replay_gain.clone();
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

        // Always reset the completion + EOF flags BEFORE the early-return.
        //
        // `load_prepared_track`'s format-change path (`force_reload=true`)
        // calls us while `self.playing=true` is still set from the previous
        // track — so without these unconditional resets, the previous track's
        // `finished_called=true` (set by the render_tick gate when its decoder
        // hit EOF) leaks into the new track. The new track's gate condition
        // `!self.finished_called && eof && buf_empty` is then permanently
        // blocked when the new track's decoder EOFs, `on_renderer_finished`
        // is never called, and playback silently halts.
        //
        // `start_decoding_loop()` independently resets `decoder_eof`, but it
        // can't reset `finished_called` (cross-module concern). So the leak
        // was specifically of `finished_called`. Resetting both here for
        // symmetry — they belong together as "new track lifecycle starts".
        //
        // Reproduces with: stereo → mono → stereo natural auto-advance, no
        // `engine.seek()` between tracks (since `renderer.seek` also resets
        // `finished_called` and accidentally masks the leak). Discovered via
        // an overnight burn-in on 2026-05-14 where playback halted at queue
        // index 2870/13473 on a 1ch → 2ch transition after ~5 minutes.
        self.finished_called = false;
        self.decoder_eof.store(false, Ordering::Release);

        if self.playing && !self.paused {
            trace!("▶ Renderer::start() already playing, returning early");
            return;
        }

        self.playing = true;
        self.paused = false;

        // Ensure streams are unpaused (the `paused` atomic on each StreamHandle
        // must be cleared — otherwise the source returns silence).
        if let Some(ref stream) = self.primary_stream {
            stream.resume();
            stream.set_volume(self.stream_volume());
        }
        if let CrossfadeState::Active { stream, .. } = &self.crossfade_state {
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
        self.paused = true;
        // Pause the streaming source — it will emit silence and stop
        // counting samples, so position freezes correctly.
        if let Some(ref stream) = self.primary_stream {
            stream.pause();
        }
        if let CrossfadeState::Active { stream, .. } = &self.crossfade_state {
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
        if let CrossfadeState::Active { stream, .. } = &self.crossfade_state {
            stream.resume();
        }
        // Restore volume (may have been changed during pause via set_volume)
        if !matches!(self.crossfade_state, CrossfadeState::Active { .. })
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

        // Recreate the primary stream (if output exists). Seek reuses the
        // current track's RG since we're not switching tracks — leave
        // pending_replay_gain alone and resolve from current_replay_gain.
        if let Some(ref output) = self.output {
            let rg_for_seek = self
                .pending_replay_gain
                .as_ref()
                .or(self.current_replay_gain.as_ref());
            let norm = self.resolve_norm_for(rg_for_seek);
            let stream = output.create_stream(
                self.format.sample_rate(),
                self.format.channel_count() as u16,
                self.stream_volume(),
                norm,
                self.eq_state.clone(),
                self.consumed_notify.clone(),
                true,
            );
            self.primary_stream = Some(stream);
            // Keep current_replay_gain consistent — don't blow it away.
            if let Some(rg) = rg_for_seek {
                self.current_replay_gain = Some(rg.clone());
            }
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
        if matches!(self.crossfade_state, CrossfadeState::Active { .. }) {
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

    /// Get underrun diagnostics from the primary stream.
    /// Returns `(count, peak_samples, total_silence)`.
    pub fn underrun_stats(&self) -> (u64, u64, u64) {
        self.primary_stream
            .as_ref()
            .map_or((0, 0, 0), |s| s.handle.underrun_stats())
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
        if self.pw_volume_active {
            1.0
        } else {
            self.volume as f32
        }
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

        self.crossfade_state = CrossfadeState::Armed {
            duration_ms: effective,
            incoming_format: incoming_format.clone(),
            track_duration_ms,
        };
        debug!(
            "🔀 [RENDERER] Crossfade ARMED: duration={}ms, track={}ms, incoming={:?}",
            effective, track_duration_ms, incoming_format
        );
    }

    /// Disarm the crossfade trigger.
    ///
    /// Only resets when currently `Armed` — leaves `Active` and `Idle`
    /// states untouched, matching the previous field-based semantics
    /// where this only zeroed the `crossfade_armed_*` group.
    pub fn disarm_crossfade(&mut self) {
        if matches!(self.crossfade_state, CrossfadeState::Armed { .. }) {
            self.crossfade_state = CrossfadeState::Idle;
        }
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

        let cf_norm = self.resolve_norm_for(self.pending_crossfade_replay_gain.as_ref());
        // `feeds_visualizer = false` — see `StreamHandle::feeds_visualizer`.
        // Two concurrent streams sharing the visualizer callback would otherwise
        // flip its rate atomic every batch, thrashing the spectrum engine into
        // constant reinit. `finalize_crossfade` flips this `true` after promotion.
        let cf_stream = output.create_stream(
            incoming_format.sample_rate(),
            incoming_format.channel_count() as u16,
            0.0, // Start silent, volume will ramp up
            cf_norm,
            self.eq_state.clone(),
            self.consumed_notify.clone(),
            false,
        );

        self.crossfade_state = CrossfadeState::Active {
            stream: cf_stream,
            started_at: std::time::Instant::now(),
            duration_ms,
            incoming_format: incoming_format.clone(),
        };

        debug!(
            "🔀 [RENDERER] Crossfade STARTED: {}ms, incoming={:?}",
            duration_ms, incoming_format
        );
    }

    /// Cancel an in-flight (Active) crossfade.
    ///
    /// Only handles `CrossfadeState::Active` — `Armed` is preserved (the
    /// gapless prep is still valid; the position-based trigger should still
    /// fire from `render_tick`). To clear `Armed` explicitly, use
    /// [`Self::disarm_crossfade`]. The engine's `cancel_crossfade` pairs the
    /// two for stop / skip flows.
    ///
    /// The seek path used to call this and silently drop the `Armed` state,
    /// which left the engine's gapless slot prepared but the renderer with
    /// nothing to fire — forcing the EOF-fallback crossfade against an
    /// already-drained outgoing (audible as a multi-second pause / fade-in
    /// from silence on tracks where seek landed during the armed window).
    pub fn cancel_crossfade(&mut self) {
        if !matches!(self.crossfade_state, CrossfadeState::Active { .. }) {
            return;
        }
        let prior = std::mem::replace(&mut self.crossfade_state, CrossfadeState::Idle);
        if let CrossfadeState::Active {
            stream,
            started_at,
            duration_ms,
            ..
        } = prior
        {
            debug!(
                "🔀 [RENDERER] Crossfade CANCELLED: elapsed={}ms/{}ms",
                started_at.elapsed().as_millis(),
                duration_ms,
            );
            stream.silence_and_stop();
        }
        // Drop the staged RG since the incoming stream is being thrown away.
        self.pending_crossfade_replay_gain = None;

        // Restore primary volume
        if !self.paused
            && let Some(ref stream) = self.primary_stream
        {
            stream.set_volume(self.stream_volume());
        }
    }

    /// Write decoded f32 samples to the crossfade (incoming) stream.
    pub fn write_crossfade_samples(&mut self, samples: &[f32]) -> usize {
        if let CrossfadeState::Active { stream, .. } = &mut self.crossfade_state {
            stream.write_samples(samples)
        } else {
            0
        }
    }

    /// Check available space in the crossfade stream.
    pub fn crossfade_available_space(&self) -> usize {
        if let CrossfadeState::Active { stream, .. } = &self.crossfade_state {
            stream.available_space()
        } else {
            0
        }
    }

    /// Whether a crossfade is currently in progress.
    pub fn is_crossfade_active(&self) -> bool {
        matches!(self.crossfade_state, CrossfadeState::Active { .. })
    }

    /// Get crossfade buffer count (approximate samples in crossfade ring buffer).
    pub fn crossfade_buffer_count(&self) -> usize {
        if let CrossfadeState::Active { stream, .. } = &self.crossfade_state {
            crate::audio::rodio_output::RING_BUFFER_CAPACITY
                .saturating_sub(stream.available_space())
        } else {
            0
        }
    }

    /// Tick the crossfade: update volumes based on elapsed time.
    /// Called periodically (e.g., from the decode loop or a timer).
    /// Returns `true` if the crossfade should be finalized.
    pub fn tick_crossfade(&mut self) -> bool {
        // Compute fade coefficients first (immutable borrow), then apply them
        // to both streams in a separate pass so the variant's stream and the
        // primary stream can be touched without overlapping borrows.
        let (fade_out, fade_in, progress) = match &self.crossfade_state {
            CrossfadeState::Active {
                started_at,
                duration_ms,
                ..
            } => {
                let elapsed_ms = started_at.elapsed().as_millis() as u64;
                let progress = if *duration_ms > 0 {
                    (elapsed_ms as f64 / *duration_ms as f64).min(1.0)
                } else {
                    1.0
                };
                // Equal-power crossfade using cos²/sin² curves.
                let fade_out = (progress * std::f64::consts::FRAC_PI_2).cos().powi(2);
                let fade_in = (progress * std::f64::consts::FRAC_PI_2).sin().powi(2);
                (fade_out, fade_in, progress)
            }
            _ => return false,
        };

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
        if let CrossfadeState::Active { stream, .. } = &self.crossfade_state {
            stream.set_volume((fade_in * user_vol as f64) as f32);
        }

        progress >= 1.0
    }

    /// Finalize the crossfade: swap crossfade stream → primary stream.
    /// Returns the elapsed crossfade time in milliseconds (for position offset).
    pub fn finalize_crossfade(&mut self) -> u64 {
        let CrossfadeState::Active {
            stream,
            started_at,
            duration_ms,
            incoming_format,
        } = std::mem::replace(&mut self.crossfade_state, CrossfadeState::Idle)
        else {
            return 0;
        };

        let elapsed_ms = started_at.elapsed().as_millis() as u64;

        debug!(
            "🔀 [RENDERER] Crossfade FINALIZED: elapsed={}ms/{}ms",
            elapsed_ms, duration_ms,
        );

        // Stop old primary, promote crossfade stream to primary
        if let Some(old_primary) = self.primary_stream.take() {
            old_primary.silence_and_stop();
        }
        self.primary_stream = Some(stream);

        // Set new primary to full user volume and promote it to visualizer
        // feeder (it was created silent for the viz to avoid the two-stream
        // rate thrash; now that it's the only stream alive, it should drive
        // the spectrum).
        if let Some(ref stream) = self.primary_stream {
            stream.set_volume(self.stream_volume());
            stream.set_feeds_visualizer(true);
        }

        // Update format to the incoming track's format
        self.format = incoming_format;
        // Promote the crossfade RG to "current" — it's now baked into the
        // new primary stream's `amplify` factor.
        self.current_replay_gain = self.pending_crossfade_replay_gain.take();

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
        matches!(self.crossfade_state, CrossfadeState::Armed { .. })
    }

    /// Get the armed crossfade duration, or `0` if not currently armed.
    pub fn crossfade_armed_duration_ms(&self) -> u64 {
        match &self.crossfade_state {
            CrossfadeState::Armed { duration_ms, .. } => *duration_ms,
            _ => 0,
        }
    }

    /// Get the armed crossfade incoming format, or `None` if not currently armed.
    pub fn crossfade_armed_incoming_format(&self) -> Option<&AudioFormat> {
        match &self.crossfade_state {
            CrossfadeState::Armed {
                incoming_format, ..
            } => Some(incoming_format),
            _ => None,
        }
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
            trace!(
                "🔍 [RENDER_TICK #{tick}] playing={}, paused={}, eof={}, buf_empty={}, \
                 buf_count={}, primary={}, xfade_active={}, xfade_armed={}, finished={}",
                self.playing,
                self.paused,
                self.decoder_eof.load(Ordering::Acquire),
                self.is_buffer_queue_empty(),
                self.buffer_count(),
                self.primary_stream.is_some(),
                self.is_crossfade_active(),
                self.is_crossfade_armed(),
                self.finished_called,
            );
        }

        if !self.playing || self.paused {
            return;
        }

        // Tick crossfade volumes if active
        if matches!(self.crossfade_state, CrossfadeState::Active { .. }) && self.tick_crossfade() {
            // Crossfade duration expired — finalize the renderer-side crossfade
            // synchronously (we already hold the lock). This swaps the crossfade
            // stream to primary and resets the state to Idle. Then signal the engine
            // to swap decoders/sources.
            debug!("🔀 [RENDER_TICK] Crossfade complete — finalizing renderer + signaling engine");
            self.finalize_crossfade();
            self.on_renderer_finished();
            return; // Don't run further checks this tick
        }

        // Check for crossfade trigger: position-based, NOT EOF-based.
        // We start the crossfade `duration_ms` before the track ends, so the
        // outgoing track still has audio in its buffer to fade out. Falls back
        // to EOF if duration is unknown (0).
        if let CrossfadeState::Armed {
            duration_ms,
            track_duration_ms,
            ..
        } = &self.crossfade_state
        {
            let pos = self.position();
            let track_dur = *track_duration_ms;
            let xfade_dur = *duration_ms;
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
                // Capture armed values by replacing the state with Idle, then
                // immediately start the active crossfade. The mem::replace
                // serves as the disarm.
                let armed = std::mem::replace(&mut self.crossfade_state, CrossfadeState::Idle);
                let CrossfadeState::Armed {
                    duration_ms: cf_duration,
                    incoming_format: cf_format,
                    ..
                } = armed
                else {
                    // Unreachable — we already matched Armed above. Restore
                    // state and bail out.
                    self.crossfade_state = armed;
                    return;
                };

                // Start the renderer-side crossfade SYNCHRONOUSLY so that the
                // Active state prevents the track-completion check from firing
                // before the async engine task can respond. The engine's
                // start_crossfade() skips renderer.start_crossfade() if we've
                // already activated it here.
                self.start_crossfade(cf_duration, &cf_format);

                // Signal the engine async to set up the decoder and decode loop
                self.on_renderer_finished();
                return;
            }
        }

        // Check for track completion (ring buffer empty + decoder EOF + not crossfading)
        if !matches!(self.crossfade_state, CrossfadeState::Active { .. })
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
        let generation = self.source_generation.current();
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

/// Compare track_gain values across two optional `ReplayGain` snapshots.
/// Returns `true` when the new track would need a different `amplify`
/// factor than the live stream is currently applying.
fn rg_track_gains_differ(current: Option<&ReplayGain>, pending: Option<&ReplayGain>) -> bool {
    match (current, pending) {
        (None, None) => false,
        (None, Some(p)) => p.track_gain.is_some(),
        (Some(c), None) => c.track_gain.is_some(),
        (Some(c), Some(p)) => c.track_gain != p.track_gain,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `cancel_crossfade` historically blasted ANY prior state to `Idle`, which
    /// silently disarmed a pending gapless crossfade when the user seeked during
    /// the armed window. After the seek, the engine's gapless slot was still
    /// "prepared", so the position-based trigger never fired and the crossfade
    /// got kicked into the EOF-fallback path — producing a ~5 s silent fade-in
    /// because the outgoing was already at EOF.
    ///
    /// The contract is now: `cancel_crossfade` handles `Active` only;
    /// `disarm_crossfade` handles `Armed` only. Callers must pair them when
    /// they want both (see `engine.cancel_crossfade`).
    #[tokio::test]
    async fn cancel_crossfade_preserves_armed_state() {
        let mut renderer = AudioRenderer::new();
        renderer.crossfade_state = CrossfadeState::Armed {
            duration_ms: 10_000,
            incoming_format: AudioFormat::invalid(),
            track_duration_ms: 230_315,
        };

        renderer.cancel_crossfade();

        assert!(
            matches!(renderer.crossfade_state, CrossfadeState::Armed { .. }),
            "cancel_crossfade must leave Armed alone — use disarm_crossfade to clear Armed"
        );
    }

    /// Regression: `cancel_crossfade` on a fresh `Idle` renderer must remain a
    /// no-op (no panic, state stays Idle).
    #[tokio::test]
    async fn cancel_crossfade_on_idle_is_noop() {
        let mut renderer = AudioRenderer::new();
        renderer.cancel_crossfade();
        assert!(matches!(renderer.crossfade_state, CrossfadeState::Idle));
    }

    /// Sanity: `disarm_crossfade` still clears `Armed → Idle` (the complementary
    /// half of the split). Without this the engine's stop / cancel paths would
    /// leak a stale Armed state across track changes.
    #[tokio::test]
    async fn disarm_crossfade_clears_armed_to_idle() {
        let mut renderer = AudioRenderer::new();
        renderer.crossfade_state = CrossfadeState::Armed {
            duration_ms: 10_000,
            incoming_format: AudioFormat::invalid(),
            track_duration_ms: 230_315,
        };

        renderer.disarm_crossfade();

        assert!(matches!(renderer.crossfade_state, CrossfadeState::Idle));
    }
}
