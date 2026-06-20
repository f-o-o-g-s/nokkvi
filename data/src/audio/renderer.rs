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
        format::samples_for_duration,
        resolve_normalization,
        rodio_output::{ActiveStream, RodioOutput},
        streaming_source::SharedVisualizerCallback,
    },
    types::{
        player_settings::{BitPerfectMode, VolumeNormalizationMode},
        song::ReplayGain,
    },
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
        /// Total time spent paused since the crossfade started. Subtracted
        /// from wall-clock elapsed so progress only advances while audio is
        /// actually produced (the streams are paused while `paused_at` is set).
        paused_accum: std::time::Duration,
        /// When `Some`, the crossfade is currently paused; the span since this
        /// instant is folded into `paused_accum` on resume.
        paused_at: Option<std::time::Instant>,
    },
}

/// Outcome of a single [`AudioRenderer::tick_crossfade`] call.
///
/// Replaces the historical bare `bool` so the render loop can distinguish a
/// healthy completion (`Finalize`) from a fade that reached 100% wall-clock
/// progress while the incoming stream never produced audio (`IncomingStalled`).
/// Without the stall case a stalled/failed incoming decoder still completed the
/// fade — dropping the audible outgoing track and promoting a silent decoder to
/// primary (music faded into silence with no recovery).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CrossfadeTick {
    /// Fade still in progress — keep ticking.
    Continue,
    /// Fade complete and the incoming stream produced audio — promote it.
    Finalize,
    /// Fade reached completion but the incoming stream is empty (stalled /
    /// failed decode) — recover by restoring the outgoing and skipping the
    /// bad track via the normal end-of-track path.
    IncomingStalled,
}

/// Pure crossfade-progress accessor — single source of truth shared by
/// `tick_crossfade`, `finalize_crossfade`, and the `cancel_crossfade` log so
/// the three readers can never drift. Subtracts paused time from wall-clock
/// elapsed and clamps to `[0.0, 1.0]`. A zero duration is treated as complete.
fn crossfade_progress(elapsed_ms: u64, paused_accum_ms: u64, duration_ms: u64) -> f64 {
    if duration_ms > 0 {
        (elapsed_ms.saturating_sub(paused_accum_ms) as f64 / duration_ms as f64).clamp(0.0, 1.0)
    } else {
        1.0
    }
}

/// Rebuffer resume target, in MILLISECONDS of audio: how much decoded audio to
/// refill (output paused) before resuming after a mid-track underrun. Mirrors
/// mpv `cache-pause-wait` / MPD `buffer_before_play` (both ~1s). Scaled by the
/// stream's `frame_rate` at use so the target is a constant DURATION at every
/// sample rate — a fixed sample count shrank it to ~0.4s at 96k. MUST stay below
/// the decode-loop cushion (`engine::CUSHION_MS`): during a rebuffer the output
/// is paused and the decode loop fills the ring only up to that cushion, so a
/// resume target above it could never be reached (the original issue-9 hang).
const REBUFFER_RESUME_MS: u64 = 800;

/// Rebuffer entry watermark, in MILLISECONDS of audio: enter pause-and-rebuffer
/// when the decoded ring drains below this much audio mid-track on a FINITE
/// stream (issue #9). MUST stay strictly below the decode-loop backpressure
/// RELEASE point (`engine::CUSHION_MS / engine::BACKPRESSURE_RELEASE_DIVISOR`):
/// the loop stops decoding while the ring sits above that release point, so a
/// rebuffer that pauses the output above it freezes the ring in the backpressure
/// band and hangs until `MAX_REBUFFER_TICKS` — the issue-9 hi-res deadlock.
/// Below the release point the decode loop is actively refilling, so the pause
/// genuinely accumulates toward the resume target.
const REBUFFER_ENTER_MS: u64 = 200;

// Load-bearing invariants, enforced at compile time (mirrors engine.rs's
// `PLAY_PREBUFFER_COUNT > SEEK_PREBUFFER_COUNT`). The first is THE fix for the
// issue-9 hi-res rebuffer deadlock: entry stays strictly below the decode-loop
// backpressure release at EVERY sample rate — which holds iff it holds for the
// durations, since both thresholds scale by the same `frame_rate`. The second
// keeps the resume target reachable below the paused-refill cushion; the third
// keeps a clean enter→resume hysteresis gap.
const _: () = assert!(
    REBUFFER_ENTER_MS * super::engine::BACKPRESSURE_RELEASE_DIVISOR < super::engine::CUSHION_MS
);
const _: () = assert!(REBUFFER_RESUME_MS < super::engine::CUSHION_MS);
const _: () = assert!(REBUFFER_ENTER_MS < REBUFFER_RESUME_MS);

/// Rebuffer resume target in samples (see [`REBUFFER_RESUME_MS`]).
fn rebuffer_resume_samples(frame_rate: u32) -> usize {
    samples_for_duration(frame_rate, REBUFFER_RESUME_MS)
}

/// Rebuffer entry watermark in samples (see [`REBUFFER_ENTER_MS`]).
fn rebuffer_low_samples(frame_rate: u32) -> usize {
    samples_for_duration(frame_rate, REBUFFER_ENTER_MS)
}

/// Whether bit-perfect mode must skip a crossfade for this transition.
/// Default music-sink sample rate (Hz). The sink is built here on the first
/// probe and whenever bit-perfect is off; only bit-perfect on the native
/// PipeWire path rebuilds it at the track's native rate. The cpal fallback also
/// runs here (it never re-clocks). Single source for the literal so the build
/// sites and `ActiveSink::rate()`'s cpal arm can't drift.
pub(crate) const MUSIC_SINK_DEFAULT_RATE: u32 = 48_000;

/// Safety valve: if a finite stream stays drained this many render ticks (~10s at
/// 20ms/tick) it likely never recovers (a dead socket that never signals EOF —
/// the decode loop's empty-buffer branch loops forever with `decoder_eof=false`),
/// so give up rebuffering and let the completion/empty-buffer path run instead of
/// pausing indefinitely.
const MAX_REBUFFER_TICKS: u32 = 500;

/// What `render_tick` should do about the network rebuffer this tick.
#[derive(PartialEq, Eq, Debug)]
enum RebufferAction {
    /// Enter rebuffer: pause the output stream, then early-return.
    Enter,
    /// Stay in rebuffer: early-return (skip the completion gate on the empty ring).
    Hold,
    /// Leave rebuffer: resume the output stream, then continue.
    Exit,
    /// Nothing to do; continue `render_tick` normally.
    None,
}

/// Pure pause-and-rebuffer state machine for the FINITE (seekable, non-infinite)
/// path. Mutates the latch fields and returns the action `render_tick` applies.
/// Never fires during a crossfade, on radio, before the ring has primed once
/// after a start/seek, at genuine end-of-track (decoder EOF), or with an invalid
/// format — the guards that keep it from disrupting normal playback.
#[allow(clippy::too_many_arguments)]
fn rebuffer_action(
    playing: bool,
    is_infinite: bool,
    crossfade_idle: bool,
    eof: bool,
    frame_rate: u32,
    buffer: usize,
    rebuffering: &mut bool,
    primed: &mut bool,
    ticks: &mut u32,
) -> RebufferAction {
    if frame_rate == 0 {
        return RebufferAction::None; // no valid format yet — never rebuffer
    }
    let low = rebuffer_low_samples(frame_rate);
    let resume = rebuffer_resume_samples(frame_rate);

    // Prime once the ring has reached the resume target, so a cold track start
    // (ring at 0 during prebuffer) cannot false-pause at 0:00.
    if !*primed && buffer >= resume {
        *primed = true;
    }

    // Never rebuffer during a crossfade/gapless transition or on radio.
    if !crossfade_idle || is_infinite {
        if *rebuffering {
            *rebuffering = false;
            return RebufferAction::Exit;
        }
        return RebufferAction::None;
    }

    if *rebuffering {
        if buffer >= resume || eof {
            *rebuffering = false;
            return RebufferAction::Exit;
        }
        *ticks += 1;
        if *ticks > MAX_REBUFFER_TICKS {
            *rebuffering = false; // give up — let the completion path run
            return RebufferAction::Exit;
        }
        return RebufferAction::Hold;
    }

    // Enter only if primed, actively playing, mid-track (not EOF), drained below low.
    if playing && *primed && !eof && buffer < low {
        *rebuffering = true;
        *ticks = 0;
        return RebufferAction::Enter;
    }

    RebufferAction::None
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
    /// Set by the engine's decode loop to the cached stream type. The mid-track
    /// network rebuffer only runs on FINITE (seekable) streams, never radio.
    stream_is_infinite: Arc<AtomicBool>,
    /// True while pausing the output to refill the decoded ring after a mid-track
    /// underrun (issue #9). Distinct from `paused` (user pause): it silences only
    /// the stream output, leaving `render_tick` bookkeeping and the UI
    /// PlaybackState as "playing".
    rebuffering: bool,
    /// The ring must reach the resume target once after a start/seek before a
    /// rebuffer can fire, so a cold track start does not false-pause at 0:00.
    rebuffer_primed: bool,
    /// Consecutive ticks held in rebuffer; the safety valve gives up after
    /// `MAX_REBUFFER_TICKS` so a dead finite socket can't pause forever.
    rebuffer_ticks: u32,

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

    /// Shared master visualizer on/off gate. Owned by the renderer, cloned into
    /// every `RodioOutput`/`StreamingSource`. Flipped `false` (via
    /// [`Self::set_visualizer_enabled`]) when the user turns the visualizer off
    /// so the real-time audio thread skips the per-sample tap; `true` otherwise.
    viz_enabled: Arc<std::sync::atomic::AtomicBool>,

    /// The music output sink (PipeWire node + its rodio mixer), owned here so
    /// it can be rebuilt at each track's native rate in bit-perfect mode.
    /// `None` until [`Self::ensure_music_output`] first builds it (at login).
    music_sink: Option<crate::audio::sfx_engine::ActiveSink>,
    /// Lock-free bridge that lets the SFX engine + the volume-drag UI reach the
    /// current music sink (its mixer + IPC). The renderer publishes into it on
    /// every (re)build. `None` until `set_music_bridge` runs at login.
    music_bridge: Option<Arc<super::music_bridge::MusicOutputBridge>>,

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
    /// Shared EQ state — cloned into every new `StreamingSource` so each stream
    /// always carries an `EqProcessor` bound to the canonical shared atomics.
    /// Seeded with a default (disabled) instance so a stream created before
    /// `set_eq_state` lands still has a processor (a disabled processor is a
    /// true no-op); the engine pushes the canonical UI-owned instance at login
    /// via `set_eq_state` before the first stream is created.
    eq_state: super::eq::EqState,
    /// When `true`, PipeWire handles the user's volume via `channelVolumes`.
    /// Software volume is kept at 1.0 during normal playback; crossfade
    /// ramps use only the fade factor (PipeWire applies user volume on top).
    pw_volume_active: bool,

    /// Bit-perfect output mode (Off / Strict / Relaxed). When Strict or Relaxed,
    /// every new stream bypasses the DSP chain (EQ / software volume / limiter)
    /// so the decoded PCM reaches the sink untouched, and native-rate device
    /// switching is handled at the sink. The two differ only on whether a
    /// same-format crossfade is allowed (see [`Self::crossfade_blocked`]).
    bit_perfect_mode: BitPerfectMode,

    /// Whether the CURRENTLY-PLAYING primary stream was actually built
    /// bit-perfect (captured from `bit_perfect_active()` at stream-build time),
    /// NOT the live setting. The honest now-playing badge reads this so a
    /// mid-track toggle — which only takes effect on the next track — can't make
    /// the badge claim BIT-PERFECT while the running stream is still on the DSP
    /// path. Stays put until the next stream (re)build.
    current_stream_bit_perfect: bool,

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

    /// Update shared EQ state. Replaces the stored instance, taking effect on
    /// new streams. The engine pushes the canonical UI-owned `EqState` here at
    /// login (before first play) so streams track the live `enabled`/gain
    /// atomics rather than the seeded default's always-disabled atomics.
    pub fn set_eq_state(&mut self, state: super::eq::EqState) {
        self.eq_state = state;
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
            stream_is_infinite: Arc::new(AtomicBool::new(false)),
            rebuffering: false,
            rebuffer_primed: false,
            rebuffer_ticks: 0,
            crossfade_state: CrossfadeState::Idle,
            crossfade_finalized_elapsed_ms: 0,
            viz_callback: std::sync::Arc::new(parking_lot::RwLock::new(None)),
            viz_enabled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
            music_sink: None,
            music_bridge: None,
            volume_normalization_mode: VolumeNormalizationMode::Off,
            normalization_target_level: 1.0,
            replay_gain_preamp_db: 0.0,
            replay_gain_fallback_db: 0.0,
            replay_gain_fallback_to_agc: false,
            replay_gain_prevent_clipping: true,
            pending_replay_gain: None,
            pending_crossfade_replay_gain: None,
            current_replay_gain: None,
            eq_state: super::eq::EqState::default(),
            pw_volume_active: false,
            bit_perfect_mode: BitPerfectMode::Off,
            current_stream_bit_perfect: false,
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
        stream_is_infinite: Arc<AtomicBool>,
    ) {
        self.engine = engine;
        self.source_generation = source_generation;
        self.decoder_eof = decoder_eof;
        self.stream_is_infinite = stream_is_infinite;
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

        // Stop the old primary stream BEFORE any sink rebuild: its ring buffer
        // lives on the old mixer, which a bit-perfect rate change drops below.
        if let Some(old_stream) = self.primary_stream.take() {
            old_stream.silence_and_stop();
        }

        // Ensure the music output exists at the right rate. In bit-perfect mode
        // this rebuilds the sink + mixer at the track's native rate so PipeWire
        // switches the device clock; otherwise it stays at 48 kHz.
        self.ensure_music_output(format.sample_rate())?;

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
            Some(self.eq_state.clone()),
            self.consumed_notify.clone(),
            true,
            self.bit_perfect_active(),
        );

        if self.bit_perfect_active() {
            debug!(
                "📡 Renderer::init() NEW STREAM created: {}ch, {}Hz, BIT-PERFECT (DSP bypassed, \
                 volume at PipeWire node)",
                format.channel_count(),
                format.sample_rate(),
            );
        } else {
            debug!(
                "📡 Renderer::init() NEW STREAM created: {}ch, {}Hz, vol={:.2}, norm={:?}",
                format.channel_count(),
                format.sample_rate(),
                self.volume,
                norm
            );
        }

        self.primary_stream = Some(stream);
        // Record the build-time bit-perfect fact for the honest badge (the
        // stream captured this at construction; a later mid-track toggle won't
        // change the running stream, so the badge must not read the live flag).
        self.current_stream_bit_perfect = self.bit_perfect_active();
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

        // Network rebuffer (issue #9): a new track / seek / unpause must never
        // resume-as-rebuffer a stale stream. Clear the latch unconditionally (so
        // it resets even on the early-return path) and unpause the output in case
        // a rebuffer had paused it.
        self.reset_rebuffer_latch();
        if let Some(ref stream) = self.primary_stream {
            stream.resume();
        }

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
        }
        if let CrossfadeState::Active {
            stream,
            paused_accum,
            paused_at,
            ..
        } = &mut self.crossfade_state
        {
            stream.resume();
            // start() — NOT the (unused) resume() — is the engine's unpause
            // path (engine.play() → renderer.start()). Fold the in-progress
            // pause span into paused_accum and clear paused_at so
            // tick_crossfade excludes the silent interval. Without this,
            // paused_at stays Some and `live_paused` tracks wall clock in
            // lockstep with `elapsed_ms`, freezing crossfade progress below
            // 1.0 forever: the fade never finalizes, the UI position pins at
            // the outgoing track's end, and the queue never advances.
            if let Some(t) = paused_at.take() {
                *paused_accum += t.elapsed();
            }
        }
        // Restore primary volume (it may have been changed via set_volume while
        // paused) only when NOT crossfading — during an active crossfade the
        // stream volumes are owned by the crossfade tick.
        if !matches!(self.crossfade_state, CrossfadeState::Active { .. })
            && let Some(ref stream) = self.primary_stream
        {
            stream.set_volume(self.stream_volume());
        }

        trace!("▶ Renderer::start() completed — stream active");
    }

    /// Stop playback.
    pub fn stop(&mut self) {
        trace!("⏹ Renderer: stop() called");
        self.playing = false;
        self.paused = false;
        self.finished_called = false;
        self.reset_rebuffer_latch();

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

    /// Zero the network-rebuffer latch (issue #9). Called by every lifecycle
    /// transition that starts a fresh ring (start/stop/seek/finalize_crossfade)
    /// so a promoted/new stream re-primes against its OWN format instead of
    /// carrying a stale `rebuffer_primed` into an unreachable resume target.
    /// pause() deliberately does NOT use this — it only clears `rebuffering`.
    fn reset_rebuffer_latch(&mut self) {
        self.rebuffering = false;
        self.rebuffer_primed = false;
        self.rebuffer_ticks = 0;
    }

    /// Pause playback.
    pub fn pause(&mut self) {
        self.paused = true;
        // A user pause subsumes any in-progress network rebuffer (the stream is
        // paused either way); clear the flag so resume goes through start().
        self.rebuffering = false;
        // Pause the streaming source — it will emit silence and stop
        // counting samples, so position freezes correctly.
        if let Some(ref stream) = self.primary_stream {
            stream.pause();
        }
        if let CrossfadeState::Active {
            stream, paused_at, ..
        } = &mut self.crossfade_state
        {
            stream.pause();
            // Stamp the pause start so start() (the unpause path) can fold the
            // paused span into paused_accum — without this the wall-clock
            // progress keeps advancing while audio is silent.
            if paused_at.is_none() {
                *paused_at = Some(std::time::Instant::now());
            }
        }
    }

    /// Seek to position (milliseconds).
    pub fn seek(&mut self, position_ms: u64) {
        // Cancel crossfade on seek
        self.cancel_crossfade();

        self.position_offset = position_ms;
        self.finished_called = false;
        // The ring is about to be cleared + the stream recreated, so reset the
        // rebuffer latch (a fresh ring must re-prime before it can pause).
        self.reset_rebuffer_latch();

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
                Some(self.eq_state.clone()),
                self.consumed_notify.clone(),
                true,
                self.bit_perfect_active(),
            );
            self.primary_stream = Some(stream);
            // Seeking while paused must NOT resume audio. The stream we just
            // created starts unpaused (its `paused` atomic defaults to false in
            // StreamingSource), so without this it would immediately play the
            // seek prebuffer while the engine still reports paused — audible
            // audio beneath a frozen progress bar / play-pause button. Mirror
            // pause(): hold the new stream silent at the seek target until the
            // user resumes. Gated on `self.paused` so seeking while PLAYING is
            // unaffected.
            if self.paused
                && let Some(ref stream) = self.primary_stream
            {
                stream.pause();
                trace!("🔍 [SEEK] paused — re-paused recreated stream to hold at seek target");
            }
            self.current_stream_bit_perfect = self.bit_perfect_active();
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
            crate::audio::RING_BUFFER_CAPACITY.saturating_sub(s.available_space())
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

    /// Toggle the master visualizer gate. When `false`, every stream skips the
    /// per-sample visualizer tap (no S16 push, no callback) — so turning the
    /// visualizer off stops the audio-thread DSP feed, not just the GPU render.
    /// Works even if the output doesn't exist yet (the gate is owned here and
    /// cloned into streams at creation).
    pub fn set_visualizer_enabled(&self, enabled: bool) {
        self.viz_enabled
            .store(enabled, std::sync::atomic::Ordering::Release);
    }

    /// Connect the music-output bridge (shared with the SFX engine + the
    /// volume-drag UI) at login, and make the music output live. Builds the
    /// initial 48 kHz sink if none exists yet (so SFX + node volume work before
    /// the first track); if a sink is already live (e.g. a track started during
    /// login wiring), it is republished to the new bridge UNCHANGED rather than
    /// rebuilt — never downgrading a hi-res sink to 48 kHz.
    pub fn set_music_bridge(&mut self, bridge: Arc<super::music_bridge::MusicOutputBridge>) {
        self.music_bridge = Some(bridge);
        if self.music_sink.is_none() {
            if let Err(e) = self.build_music_sink(MUSIC_SINK_DEFAULT_RATE, false) {
                tracing::error!("🔊 Renderer: initial music sink build failed: {e}");
            }
        } else {
            self.republish_to_bridge();
        }
    }

    /// Publish the live sink's mixer + IPC forwarder to the current bridge
    /// without rebuilding it (used when a fresh bridge attaches to an
    /// already-running sink).
    fn republish_to_bridge(&self) {
        if let (Some(bridge), Some(sink)) = (&self.music_bridge, &self.music_sink) {
            bridge.publish(
                sink.mixer(),
                sink.command_forwarder(),
                sink.has_native_volume(),
            );
        }
    }

    /// Ensure the music sink exists and runs at the right rate for `format_rate`.
    /// First build (or backend probe) is always 48 kHz; then, in bit-perfect
    /// mode on the native-PipeWire path, it rebuilds at the track's native rate
    /// so PipeWire switches the device clock. A no-op when the rate already
    /// matches (the common, same-rate case — gapless and normal playback).
    pub(crate) fn ensure_music_output(&mut self, format_rate: u32) -> anyhow::Result<()> {
        // Build once at the default rate so we know the backend (native PipeWire
        // vs cpal).
        if self.music_sink.is_none() {
            self.build_music_sink(MUSIC_SINK_DEFAULT_RATE, false)?;
        }
        // Native-rate switching only applies on the native PipeWire path, and
        // for both Strict and Relaxed (both build bit-perfect streams).
        let want_native = self.bit_perfect_mode.builds_bit_perfect() && self.pw_volume_active;
        let target_rate = if want_native {
            format_rate
        } else {
            MUSIC_SINK_DEFAULT_RATE
        };
        let current_rate = self.music_sink.as_ref().map_or(0, |s| s.rate());
        if current_rate != target_rate
            && let Err(e) = self.build_music_sink(target_rate, want_native)
        {
            if want_native {
                // The native-rate rebuild failed (device transiently busy mid
                // re-clock). `build_music_sink` has already torn the old sink
                // down, so without a fallback the renderer would have NO output
                // AND no primary stream (init() stopped the old one) — a dead,
                // silent track until the NEXT track change. Fall back to the
                // default-rate sink so the track still plays. The stream is still
                // built DSP-bypassed (bit-perfect intent — pw_volume_active stays
                // true), but the device is now at the default rate, so the honest
                // device-rate badge resolves to RESAMPLED, never BIT-PERFECT
                // (Verified requires the probed device rate to equal the track
                // rate). Re-propagate only if even the fallback can't open.
                warn!(
                    "🔊 Native-rate music sink rebuild at {target_rate}Hz failed ({e:#}); \
                     falling back to {MUSIC_SINK_DEFAULT_RATE}Hz (device resamples)"
                );
                self.build_music_sink(MUSIC_SINK_DEFAULT_RATE, false)?;
            } else {
                return Err(e);
            }
        }
        Ok(())
    }

    /// (Re)build the music sink + its rodio output at `rate`, then publish the
    /// new mixer + IPC forwarder to the bridge (which re-applies the current
    /// title + volume to the fresh node). `request_native` sets `node.rate` so
    /// PipeWire follows the rate. Callers MUST stop the old primary stream first.
    ///
    /// LOAD-BEARING ORDER — the old sink is torn down (synchronously: its `Drop`
    /// joins the PipeWire thread, disconnecting the stream and releasing the
    /// ALSA device) BEFORE `open_preferred_sink` opens the new one. This is what
    /// makes native-rate switching work: PipeWire only re-clocks a device on a
    /// FRESH open — while any stream holds the card open (`format_ref > 0`) the
    /// rate is latched and a new stream is resampled to it, in EITHER direction
    /// (see `local/bitperfect/verification-findings.md`). So do NOT turn this
    /// into make-before-break or drop the old sink off-thread: opening the new
    /// node while the old is still alive pins the card at the old rate and
    /// silently loses bit-perfect (the hardware-proven up-switch regresses to
    /// resampled).
    ///
    /// COST: the synchronous teardown (`Drop` blocking-joins the old PipeWire
    /// thread) + device reopen runs while the renderer lock is held, INSIDE the
    /// held async engine lock. So for its duration — a brief output-less window
    /// in the common case, longer if the join is slow (device suspended /
    /// contended) — the 20ms render thread, both decode loops, and every other
    /// engine consumer (position/MPRIS/next user action) block on the lock; only
    /// the FFT `try_lock` degrades gracefully. This is the accepted price of a
    /// fresh re-clocking open. On a native-rate open failure `ensure_music_output`
    /// falls back to the default-rate sink so the track still plays.
    fn build_music_sink(&mut self, rate: u32, request_native: bool) -> anyhow::Result<()> {
        use crate::audio::sfx_engine::open_preferred_sink;

        // Tear the old sink fully down (releasing the device) BEFORE opening the
        // new one — see the LOAD-BEARING ORDER note above. Old output first so
        // its rodio mixer clone is released before the PipeWire thread is joined.
        self.output = None;
        self.music_sink = None;

        let mut sink = open_preferred_sink("Nokkvi", rate, request_native).map_err(|e| {
            // The old sink + its mixer/IPC are already gone (torn down above);
            // the replacement failed to open. Clear the bridge so SFX + the
            // volume UI stop proxying to the dead mixer/node — SFX silently
            // no-ops (mixer() == None) instead of feeding a dropped mixer, and
            // volume/title sends become no-ops — until the next track rebuilds
            // the sink and republishes. The stored title/volume are kept.
            if let Some(bridge) = &self.music_bridge {
                bridge.clear();
            }
            // There is no live sink anymore, so drop the renderer-side facts that
            // the honest badge and bit-perfect gating read: `pw_volume_active`
            // (no node volume) and `current_stream_bit_perfect` (no live stream).
            // Otherwise the badge could keep claiming BIT-PERFECT over a dead
            // sink. Both are re-derived from the next successful build.
            self.pw_volume_active = false;
            self.current_stream_bit_perfect = false;
            tracing::error!(
                "🔊 Renderer: music sink build at {rate}Hz failed ({e}); output is down until the \
                 next track rebuilds it"
            );
            e
        })?;
        sink.log_on_drop(false);
        self.pw_volume_active = sink.has_native_volume();

        let mixer = sink.mixer();
        self.output = Some(RodioOutput::new(
            mixer.clone(),
            self.viz_callback.clone(),
            self.viz_enabled.clone(),
        )?);

        if let Some(bridge) = &self.music_bridge {
            bridge.publish(mixer, sink.command_forwarder(), sink.has_native_volume());
        }
        tracing::info!(
            "🔊 Renderer: music sink built at {}Hz (native-rate request: {})",
            rate,
            request_native
        );
        self.music_sink = Some(sink);
        Ok(())
    }

    /// Set the bit-perfect output mode. Stored for the next stream creation;
    /// streams created while Strict/Relaxed bypass the DSP chain (EQ / software
    /// volume / limiter). Takes effect on the next track / format change, not
    /// mid-stream.
    ///
    /// Returns whether the mode actually CHANGED — the engine drives its
    /// `reset_next_track()` off this so it doesn't keep a mirrored copy of the
    /// mode, and a routine settings re-apply (same value) stays a no-op.
    pub fn set_bit_perfect(&mut self, mode: BitPerfectMode) -> bool {
        let changed = self.bit_perfect_mode != mode;
        self.bit_perfect_mode = mode;
        if changed {
            tracing::info!("🔊 Renderer: bit-perfect mode {}", mode);
        }
        changed
    }

    /// Whether bit-perfect stream-building is both requested AND viable. True
    /// for both Strict and Relaxed (both feed the DAC untouched PCM). It is only
    /// honored on the native PipeWire path (`pw_volume_active`): bit-perfect
    /// routes volume to the PipeWire node, so on the cpal fallback — which has no
    /// node volume — engaging it would leave the volume slider dead. There, the
    /// mode is a no-op and playback stays on the normal DSP path.
    pub(crate) fn bit_perfect_active(&self) -> bool {
        self.bit_perfect_mode.builds_bit_perfect() && self.pw_volume_active
    }

    /// Whether bit-perfect mode blocks the crossfade for THIS transition, given
    /// the outgoing (`current`) and incoming format. A crossfade blends two
    /// streams on one mixer with a gain envelope, which inherently alters
    /// samples — it can never be bit-perfect during the blend.
    ///
    /// - **Off / not viable** → never blocks (crossfade follows the normal path).
    /// - **Strict** → blocks EVERY transition (hard-cut). Falls through to the
    ///   normal non-crossfade path — gapless for a same-rate change, a sink
    ///   rebuild at the native rate for a cross-rate change.
    /// - **Relaxed** → blocks only when the formats differ (sample rate OR
    ///   channel count). Same-format tracks may crossfade (only the few-second
    ///   blend isn't bit-perfect); a cross-rate change still hard-cuts, because
    ///   the device can't re-clock mid-blend without resampling the incoming
    ///   track.
    ///
    /// BOTH crossfade triggers gate on THIS one method with the SAME
    /// (current, incoming) pair — the renderer's `arm_crossfade` (passing
    /// `self.format` + the armed incoming format) and the engine's EOF-fallback
    /// `try_start_crossfade_transition` (passing `current_format` + `next_format`,
    /// which describe the same two tracks) — so the two can never disagree (a
    /// renderer that arms while the engine refuses would swap `crossfade_state`
    /// to `Active` with no blend, orphaning the incoming stream).
    pub(crate) fn crossfade_blocked(&self, current: &AudioFormat, incoming: &AudioFormat) -> bool {
        if !self.bit_perfect_active() {
            return false;
        }
        match self.bit_perfect_mode {
            BitPerfectMode::Off => false,
            BitPerfectMode::Strict => true,
            BitPerfectMode::Relaxed => {
                current.sample_rate() != incoming.sample_rate()
                    || current.channel_count() != incoming.channel_count()
            }
        }
    }

    /// Whether the CURRENTLY-PLAYING primary stream was built bit-perfect
    /// (captured at stream-build time, not the live setting). The honest badge
    /// reads this so a mid-track toggle can't claim BIT-PERFECT for a stream
    /// still on the DSP path.
    pub(crate) fn current_stream_bit_perfect(&self) -> bool {
        self.current_stream_bit_perfect
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
        // Bit-perfect gates the crossfade: Strict hard-cuts EVERY transition;
        // Relaxed hard-cuts only when the incoming format differs from the
        // outgoing one (`self.format`). A crossfade applies a gain envelope to
        // two mixed streams (never bit-perfect), and a cross-rate change would
        // also resample the incoming track. Skipping arm here lets the
        // transition fall through to the normal non-crossfade path — gapless for
        // a same-rate change, a sink rebuild at native rate for a cross-rate
        // change. (The engine's EOF-fallback transition gates on the SAME method
        // with the SAME (current, incoming) pair, so neither trigger can start a
        // blend the other refuses and orphan the incoming stream.)
        if self.crossfade_blocked(&self.format, incoming_format) {
            debug!("🔀 [RENDERER] Crossfade SKIPPED (bit-perfect): hard-cut transition");
            return;
        }

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
            Some(self.eq_state.clone()),
            self.consumed_notify.clone(),
            false,
            self.bit_perfect_active(),
        );

        self.crossfade_state = CrossfadeState::Active {
            stream: cf_stream,
            started_at: std::time::Instant::now(),
            duration_ms,
            incoming_format: incoming_format.clone(),
            paused_accum: std::time::Duration::ZERO,
            paused_at: None,
        };
        // The blend itself is never bit-perfect — two streams mixed under a gain
        // envelope. Drop the honest badge for the duration of the overlap (even
        // under Relaxed, where both bodies are bit-perfect); `finalize_crossfade`
        // restores it to the promoted stream's build-time fact.
        self.current_stream_bit_perfect = false;

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
            paused_accum,
            paused_at,
            ..
        } = prior
        {
            let live_paused = paused_at.map_or(paused_accum, |t| paused_accum + t.elapsed());
            debug!(
                "🔀 [RENDERER] Crossfade CANCELLED: elapsed={}ms/{}ms",
                started_at.elapsed().saturating_sub(live_paused).as_millis(),
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

        // The outgoing primary is restored as the sole live stream. Restore the
        // honest badge to its build-time fact (`start_crossfade` dropped it to
        // false for the blend); without this a cancelled Relaxed crossfade would
        // leave the badge stuck reading "not bit-perfect" for a bit-perfect
        // outgoing track. Mirrors `finalize_crossfade`.
        self.current_stream_bit_perfect = self.bit_perfect_active();
    }

    /// Write decoded f32 samples to the crossfade (incoming) stream.
    pub fn write_crossfade_samples(&mut self, samples: &[f32]) -> usize {
        if let CrossfadeState::Active { stream, .. } = &mut self.crossfade_state {
            stream.write_samples(samples)
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
            crate::audio::RING_BUFFER_CAPACITY.saturating_sub(stream.available_space())
        } else {
            0
        }
    }

    /// Tick the crossfade: update volumes based on elapsed time.
    /// Called periodically (e.g., from the decode loop or a timer).
    ///
    /// Returns [`CrossfadeTick`]: `Continue` while the fade is in progress,
    /// `Finalize` when it completes and the incoming stream produced audio, and
    /// `IncomingStalled` when it reaches completion but the incoming ring is
    /// still empty (a stalled/failed incoming decoder) so the caller can
    /// restore the outgoing track and skip the bad one instead of fading into
    /// silence.
    pub(crate) fn tick_crossfade(&mut self) -> CrossfadeTick {
        // Compute fade coefficients first (immutable borrow), then apply them
        // to both streams in a separate pass so the variant's stream and the
        // primary stream can be touched without overlapping borrows.
        let (fade_out, fade_in, progress) = match &self.crossfade_state {
            CrossfadeState::Active {
                started_at,
                duration_ms,
                paused_accum,
                paused_at,
                ..
            } => {
                // Include any in-progress pause span (paused_at) so a tick that
                // somehow runs mid-pause still reports pause-corrected progress.
                let live_paused = paused_at.map_or(*paused_accum, |t| *paused_accum + t.elapsed());
                let elapsed_ms = started_at.elapsed().as_millis() as u64;
                let progress =
                    crossfade_progress(elapsed_ms, live_paused.as_millis() as u64, *duration_ms);
                // Equal-power crossfade using cos²/sin² curves.
                let fade_out = (progress * std::f64::consts::FRAC_PI_2).cos().powi(2);
                let fade_in = (progress * std::f64::consts::FRAC_PI_2).sin().powi(2);
                (fade_out, fade_in, progress)
            }
            _ => return CrossfadeTick::Continue,
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

        // Hand off the visualizer feed at the equal-power midpoint. Before
        // 50% the outgoing dominates audio and drives viz; after 50% the
        // incoming dominates and takes over. Without this swap the outgoing
        // primary's ring buffer drains during the crossfade tail (its decoder
        // is already at EOF for tracks long enough to fill the ring) and the
        // visualizer freezes — the incoming's viz feed is gated off until
        // finalize to prevent the two-rate atomic thrash that the prior viz
        // fix addressed. Switching flags in this order (off then on) means at
        // most one batch is skipped during the handoff; the reverse order
        // would briefly have both streams feeding the shared callback and
        // re-introduce a single rate flip.
        if progress >= 0.5 {
            if let Some(ref primary) = self.primary_stream {
                primary.set_feeds_visualizer(false);
            }
            if let CrossfadeState::Active { stream, .. } = &self.crossfade_state {
                stream.set_feeds_visualizer(true);
            }
        }

        if progress < 1.0 {
            return CrossfadeTick::Continue;
        }

        // Fade reached completion. Gate the promotion on the incoming stream
        // having actually produced audio: a stalled/failed incoming decoder
        // writes nothing, so its ring stays empty. Promoting it would fade the
        // audible outgoing track into silence with no recovery, so report the
        // stall instead and let the caller restore the outgoing + skip.
        if self.crossfade_buffer_count() == 0 {
            CrossfadeTick::IncomingStalled
        } else {
            CrossfadeTick::Finalize
        }
    }

    /// Finalize the crossfade: swap crossfade stream → primary stream.
    /// Returns the elapsed crossfade time in milliseconds (for position offset).
    pub fn finalize_crossfade(&mut self) -> u64 {
        let CrossfadeState::Active {
            stream,
            started_at,
            duration_ms,
            incoming_format,
            paused_accum,
            paused_at,
        } = std::mem::replace(&mut self.crossfade_state, CrossfadeState::Idle)
        else {
            return 0;
        };

        // Subtract paused time so the position offset reflects only the audio
        // the incoming track actually produced during the fade.
        let live_paused = paused_at.map_or(paused_accum, |t| paused_accum + t.elapsed());
        let elapsed_ms = started_at.elapsed().saturating_sub(live_paused).as_millis() as u64;

        debug!(
            "🔀 [RENDERER] Crossfade FINALIZED: elapsed={}ms/{}ms",
            elapsed_ms, duration_ms,
        );

        // Stop old primary, promote crossfade stream to primary
        if let Some(old_primary) = self.primary_stream.take() {
            old_primary.silence_and_stop();
        }
        self.primary_stream = Some(stream);
        // Restore the honest badge to the promoted stream's build-time fact.
        // Under Relaxed the incoming crossfade stream WAS built bit-perfect
        // (same-format, DSP-bypassed) and now plays alone at unity — its body is
        // bit-perfect again, so the badge should reflect that. Under Off the
        // promoted stream was built on the DSP path (false). (Strict never
        // crossfades, so it never reaches here.) `bit_perfect_active()` equals
        // what `start_crossfade` built the stream with — a mode change mid-fade
        // cancels the crossfade via `reset_next_track`, so it can't drift.
        self.current_stream_bit_perfect = self.bit_perfect_active();

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
        // The promoted track must re-prime the network rebuffer against ITS OWN
        // format — carrying a stale `rebuffer_primed` across a sample-rate change
        // is the one path that can enter rebuffer on a format whose resume target
        // is unreachable (issue #9 crossfade carryover). Reset the latch exactly
        // as start()/stop()/seek() do.
        self.reset_rebuffer_latch();
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

    /// The OUTGOING track duration stored when the crossfade was armed, or
    /// `None` if not currently `Armed`. Test-only: lets the engine's arm tests
    /// pin that `track_duration_ms` (the 3rd `arm_crossfade` arg) and the
    /// incoming duration (the 4th) did not get transposed.
    #[cfg(test)]
    pub fn armed_track_duration_ms(&self) -> Option<u64> {
        match self.crossfade_state {
            CrossfadeState::Armed {
                track_duration_ms, ..
            } => Some(track_duration_ms),
            _ => None,
        }
    }

    /// Force the renderer into an `Active` crossfade state for tests, WITHOUT a
    /// real audio output (`start_crossfade` needs `self.output`, which the unit
    /// tests don't build). Wires a detached ring buffer to a throwaway
    /// `StreamingSource`, which the caller MUST keep alive for the duration of
    /// the test so the stream handle's shared atomics stay valid. Used by the
    /// engine's `is_crossfade_live` window test (engine phase Idle + renderer
    /// Active ⇒ live).
    #[cfg(test)]
    pub fn force_crossfade_active_for_test(
        &mut self,
    ) -> crate::audio::streaming_source::StreamingSource {
        use std::num::NonZero;

        use ringbuf::{HeapRb, traits::Split};
        let rb = HeapRb::<f32>::new(crate::audio::RING_BUFFER_CAPACITY);
        let (producer, consumer) = rb.split();
        let viz: SharedVisualizerCallback = Arc::new(parking_lot::RwLock::new(None));
        let (source, handle) = crate::audio::streaming_source::StreamingSource::new(
            consumer,
            NonZero::new(2).expect("2 is nonzero"),
            NonZero::new(48_000).expect("48000 is nonzero"),
            viz,
            0.0,
            None,
            Arc::new(Notify::new()),
            false,
            Arc::new(std::sync::atomic::AtomicBool::new(true)),
            false,
        );
        let stream = crate::audio::ActiveStream {
            producer,
            handle,
            sample_rate: 48_000,
            channels: 2,
        };
        self.crossfade_state = CrossfadeState::Active {
            stream,
            started_at: std::time::Instant::now(),
            duration_ms: 1_000,
            incoming_format: AudioFormat::invalid(),
            paused_accum: std::time::Duration::ZERO,
            paused_at: None,
        };
        source
    }

    /// Renderer's copy of the engine-shared `source_generation`. The wiring
    /// interlock test compares its identity against the engine's via
    /// `SourceGeneration::ptr_eq` to prove `set_engine_link` shared (not cloned
    /// a fresh) counter.
    #[cfg(test)]
    pub fn source_generation_handle(&self) -> &SourceGeneration {
        &self.source_generation
    }

    /// Renderer's copy of the engine-shared `decoder_eof` Arc, for the wiring
    /// interlock test's `Arc::ptr_eq` identity check.
    #[cfg(test)]
    pub fn decoder_eof_handle(&self) -> &Arc<AtomicBool> {
        &self.decoder_eof
    }

    /// Renderer's copy of the engine-shared `stream_is_infinite` Arc, for the
    /// wiring interlock test's `Arc::ptr_eq` identity check.
    #[cfg(test)]
    pub fn stream_is_infinite_handle(&self) -> &Arc<AtomicBool> {
        &self.stream_is_infinite
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
        if matches!(self.crossfade_state, CrossfadeState::Active { .. }) {
            match self.tick_crossfade() {
                CrossfadeTick::Continue => {}
                CrossfadeTick::Finalize => {
                    // Crossfade duration expired and the incoming stream produced
                    // audio — finalize the renderer-side crossfade synchronously
                    // (we already hold the lock). This swaps the crossfade stream
                    // to primary and resets the state to Idle, then signals the
                    // engine to swap decoders/sources.
                    debug!(
                        "🔀 [RENDER_TICK] Crossfade complete — finalizing renderer + signaling engine"
                    );
                    self.finalize_crossfade();
                    self.on_renderer_finished();
                    return; // Don't run further checks this tick
                }
                CrossfadeTick::IncomingStalled => {
                    // Fade completed on wall clock but the incoming decoder never
                    // produced audio. Do NOT promote the silent stream. Signal the
                    // engine to cancel the crossfade (restoring the outgoing as
                    // primary at full volume) and skip the bad track via the
                    // normal end-of-track path.
                    warn!(
                        "🔀 [RENDER_TICK] Crossfade completed but incoming stream is empty \
                         (stalled decode) — recovering by restoring outgoing + skipping"
                    );
                    self.on_renderer_crossfade_stalled();
                    return; // Don't run further checks this tick
                }
            }
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

        // ---- Network rebuffer (issue #9): on a mid-track underrun on a FINITE
        // stream, pause the output and refill ~1s before resuming, instead of
        // emitting silence. Mirrors mpv/MPD/GStreamer pause-and-rebuffer; the
        // try_pop().unwrap_or(0.0) silence in StreamingSource stays as the xrun
        // backstop only. Runs AFTER the crossfade trigger (never fights a fade)
        // and BEFORE the completion gate (never pauses a finishing track). ----
        let frame_rate = self.format.frame_rate();
        let is_infinite = self.stream_is_infinite.load(Ordering::Acquire);
        let eof = self.decoder_eof.load(Ordering::Acquire);
        let cf_idle = matches!(self.crossfade_state, CrossfadeState::Idle);
        match rebuffer_action(
            self.playing,
            is_infinite,
            cf_idle,
            eof,
            frame_rate,
            self.buffer_count(),
            &mut self.rebuffering,
            &mut self.rebuffer_primed,
            &mut self.rebuffer_ticks,
        ) {
            RebufferAction::Enter => {
                if let Some(ref stream) = self.primary_stream {
                    stream.pause();
                }
                debug!(
                    "🔌 [REBUFFER] ring drained mid-track ({} samples) — pausing to refill",
                    self.buffer_count()
                );
                return;
            }
            RebufferAction::Hold => return,
            RebufferAction::Exit => {
                if let Some(ref stream) = self.primary_stream {
                    stream.resume();
                }
                debug!("🔌 [REBUFFER] refilled to resume target — resuming output");
                // fall through to the completion gate
            }
            RebufferAction::None => {}
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

    /// Called from `render_tick` when a crossfade reached completion but the
    /// incoming stream never produced audio (stalled / failed decode).
    ///
    /// Unlike `on_renderer_finished`, this does NOT call `finalize_crossfade`
    /// on the renderer (that would promote the silent stream). It signals the
    /// engine to cancel the crossfade — restoring the outgoing stream as the
    /// primary at full volume and clearing the incoming decoder — then run a
    /// normal end-of-track transition so the bad track is skipped via the
    /// standard `peek_next_song` path. Spawned (rather than synchronous) so the
    /// engine lock is acquired off the render thread, matching the
    /// deadlock-safety rationale on `on_renderer_finished`.
    fn on_renderer_crossfade_stalled(&mut self) {
        let generation = self.source_generation.current();
        warn!("🔀 [RENDERER] Crossfade incoming stalled (generation={generation}) — recovering");

        if let Some(engine_ref) = self.engine.upgrade() {
            let handle = self.tokio_handle.clone();
            let src_gen = generation;
            handle.spawn(async move {
                let mut engine = engine_ref.lock().await;
                // Skip if a user action already moved the source on (the
                // abandoned incoming's late callbacks are discarded anyway).
                if engine.source_generation() == src_gen {
                    engine.recover_stalled_crossfade().await;
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
    use std::num::NonZero;

    use ringbuf::{
        HeapRb,
        traits::{Producer, Split},
    };

    use super::*;
    use crate::audio::streaming_source::{SharedVisualizerCallback, StreamingSource};

    /// `crossfade_blocked(current, incoming)` — the single gate both crossfade
    /// triggers share — encodes the three bit-perfect modes:
    /// - Off: never blocks.
    /// - Strict: blocks every transition (same-rate AND cross-rate alike).
    /// - Relaxed: blocks only a cross-FORMAT transition (different sample rate
    ///   or channel count); same-format tracks may crossfade.
    /// And it is inert when bit-perfect is requested-but-not-viable (cpal: no
    /// PipeWire node volume). `#[tokio::test]` because `AudioRenderer::new()`
    /// needs a running reactor.
    #[tokio::test]
    async fn bit_perfect_crossfade_gate_per_mode_and_format() {
        use crate::audio::format::SampleFormat;
        let f44 = AudioFormat::new(SampleFormat::S16, 44_100, 2);
        let f96 = AudioFormat::new(SampleFormat::S16, 96_000, 2);
        let f44_mono = AudioFormat::new(SampleFormat::S16, 44_100, 1);

        let mut renderer = AudioRenderer::new();

        // Off → never blocked. set_bit_perfect reports the change.
        assert!(!renderer.crossfade_blocked(&f44, &f44));
        assert!(!renderer.crossfade_blocked(&f44, &f96));

        // Strict, requested but not viable (cpal: no node volume) → inert.
        assert!(
            renderer.set_bit_perfect(BitPerfectMode::Strict),
            "Off → Strict is a real change"
        );
        assert!(
            !renderer.set_bit_perfect(BitPerfectMode::Strict),
            "re-applying the same mode is a no-op change"
        );
        assert!(!renderer.crossfade_blocked(&f44, &f44));

        // Strict, viable (native PipeWire volume) → blocks EVERYTHING.
        renderer.pw_volume_active = true;
        assert!(renderer.crossfade_blocked(&f44, &f44), "Strict same-rate");
        assert!(renderer.crossfade_blocked(&f44, &f96), "Strict cross-rate");

        // Relaxed, viable → same format passes, differing rate/channels block.
        assert!(renderer.set_bit_perfect(BitPerfectMode::Relaxed));
        assert!(
            !renderer.crossfade_blocked(&f44, &f44),
            "Relaxed allows a same-format crossfade"
        );
        assert!(
            renderer.crossfade_blocked(&f44, &f96),
            "Relaxed hard-cuts a sample-rate change"
        );
        assert!(
            renderer.crossfade_blocked(&f44, &f44_mono),
            "Relaxed hard-cuts a channel-count change"
        );

        // Relaxed but not viable → inert again.
        renderer.pw_volume_active = false;
        assert!(!renderer.crossfade_blocked(&f44, &f96));
    }

    /// Seeking while PAUSED must keep playback paused and hold the playhead at
    /// the seek target — never resume. This is the standard media-player
    /// convention (the WHATWG media element, mpd, mpv et al. leave the paused
    /// state unchanged on seek).
    ///
    /// Scope: this guards the device-free FIELD invariant `seek()` relies on —
    /// it must not clear `paused` and must record the target in
    /// `position_offset`, so `position()` reports the held target while paused.
    /// The stream-level re-pause the fix adds (silencing the recreated output
    /// stream) is device-bound — `AudioRenderer::new()` leaves `output: None`,
    /// so no stream is created here — and is owner-verified via `cargo run`.
    /// `#[tokio::test]` because `new()` needs a running reactor.
    #[tokio::test]
    async fn seek_while_paused_holds_playhead_and_stays_paused() {
        let mut renderer = AudioRenderer::new();
        renderer.pause();
        assert!(renderer.paused, "pause() sets the renderer paused flag");

        renderer.seek(5_000);

        assert!(
            renderer.paused,
            "seek must NOT clear the paused flag — seeking while paused stays paused"
        );
        assert_eq!(
            renderer.position_offset, 5_000,
            "seek records the target offset"
        );
        assert_eq!(
            renderer.position(),
            5_000,
            "while paused, position reports the held seek target, not an advancing clock"
        );
    }

    const FR: u32 = 44_100 * 2; // 44.1k stereo frame rate
    const FR_S: usize = FR as usize; // same, as a sample count for the buffer arg
    const HR_FR: u32 = 96_000 * 2; // 96k stereo frame rate (hi-res) = 192_000
    // Decode-loop backpressure cushion (engine `compute_watermarks` high) at the
    // hi-res frame rate — now time-based (~CUSHION_MS of audio), so it scales
    // with the sample rate instead of a fixed 96_000 samples. The most the
    // decoded ring ever holds at 96k stereo.
    const HR_BACKPRESSURE_CAP: usize =
        ((HR_FR as u64) * crate::audio::engine::CUSHION_MS / 1000) as usize;

    #[test]
    fn rebuffer_does_not_enter_before_primed() {
        // Cold start: ring at 0 but never primed → must NOT pause (no 0:00 hitch).
        let (mut reb, mut primed, mut ticks) = (false, false, 0);
        let a = rebuffer_action(
            true,
            false,
            true,
            false,
            FR,
            0,
            &mut reb,
            &mut primed,
            &mut ticks,
        );
        assert_eq!(a, RebufferAction::None);
        assert!(!reb);
    }

    #[test]
    fn rebuffer_primes_then_enters_on_mid_track_drain() {
        let (mut reb, mut primed, mut ticks) = (false, false, 0);
        // Reach the resume target → primes (buffer high, no pause).
        rebuffer_action(
            true,
            false,
            true,
            false,
            FR,
            FR_S,
            &mut reb,
            &mut primed,
            &mut ticks,
        );
        assert!(primed && !reb);
        // Now drain below the low mark mid-track → enter rebuffer.
        let a = rebuffer_action(
            true,
            false,
            true,
            false,
            FR,
            FR_S / 10,
            &mut reb,
            &mut primed,
            &mut ticks,
        );
        assert_eq!(a, RebufferAction::Enter);
        assert!(reb);
    }

    #[test]
    fn rebuffer_resumes_at_target() {
        let (mut reb, mut primed, mut ticks) = (true, true, 3);
        let a = rebuffer_action(
            true,
            false,
            true,
            false,
            FR,
            FR_S,
            &mut reb,
            &mut primed,
            &mut ticks,
        );
        assert_eq!(a, RebufferAction::Exit);
        assert!(!reb);
    }

    #[test]
    fn rebuffer_never_enters_at_eof_or_crossfade_or_radio_or_invalid_format() {
        // primed + drained, but EOF → genuine end-of-track, never rebuffer.
        let (mut reb, mut primed, mut ticks) = (false, true, 0);
        assert_eq!(
            rebuffer_action(
                true,
                false,
                true,
                true,
                FR,
                0,
                &mut reb,
                &mut primed,
                &mut ticks
            ),
            RebufferAction::None
        );
        // crossfade not idle → never rebuffer.
        let (mut reb, mut primed, mut ticks) = (false, true, 0);
        assert_eq!(
            rebuffer_action(
                true,
                false,
                false,
                false,
                FR,
                0,
                &mut reb,
                &mut primed,
                &mut ticks
            ),
            RebufferAction::None
        );
        // radio (infinite) → never rebuffer.
        let (mut reb, mut primed, mut ticks) = (false, true, 0);
        assert_eq!(
            rebuffer_action(
                true,
                true,
                true,
                false,
                FR,
                0,
                &mut reb,
                &mut primed,
                &mut ticks
            ),
            RebufferAction::None
        );
        // invalid format (frame_rate 0) → no-op.
        let (mut reb, mut primed, mut ticks) = (false, true, 0);
        assert_eq!(
            rebuffer_action(
                true,
                false,
                true,
                false,
                0,
                0,
                &mut reb,
                &mut primed,
                &mut ticks
            ),
            RebufferAction::None
        );
    }

    #[test]
    fn rebuffer_exits_if_crossfade_starts_mid_rebuffer() {
        // Already rebuffering, then a crossfade arms (cf_idle=false) → resume.
        let (mut reb, mut primed, mut ticks) = (true, true, 2);
        let a = rebuffer_action(
            true,
            false,
            false,
            false,
            FR,
            0,
            &mut reb,
            &mut primed,
            &mut ticks,
        );
        assert_eq!(a, RebufferAction::Exit);
        assert!(!reb);
    }

    #[test]
    fn rebuffer_safety_valve_gives_up_after_max_ticks() {
        let (mut reb, mut primed, mut ticks) = (true, true, MAX_REBUFFER_TICKS);
        // One more tick past the cap on a still-drained stream → give up.
        let a = rebuffer_action(
            true,
            false,
            true,
            false,
            FR,
            0,
            &mut reb,
            &mut primed,
            &mut ticks,
        );
        assert_eq!(a, RebufferAction::Exit);
        assert!(!reb);
    }

    #[test]
    fn rebuffer_functions_on_hi_res_within_backpressure_cap() {
        // The decode loop fills the ring to a TIME-based cushion (~CUSHION_MS of
        // audio), so on hi-res (96k stereo, frame_rate 192_000) the resume target
        // MUST stay reachable under that cushion. Otherwise the ring can never
        // reach `resume`, so it never primes / never exits and hangs until
        // MAX_REBUFFER_TICKS (issue #9 hi-res unit mismatch).
        let (mut reb, mut primed, mut ticks) = (false, false, 0);
        // Ring filled to the backpressure cushion (the most it can ever hold) →
        // must prime even at hi-res.
        rebuffer_action(
            true,
            false,
            true,
            false,
            HR_FR,
            HR_BACKPRESSURE_CAP,
            &mut reb,
            &mut primed,
            &mut ticks,
        );
        assert!(
            primed,
            "hi-res must prime once the ring reaches the backpressure cushion"
        );
        // Drain below low mid-track → enter rebuffer.
        let a = rebuffer_action(
            true,
            false,
            true,
            false,
            HR_FR,
            0,
            &mut reb,
            &mut primed,
            &mut ticks,
        );
        assert_eq!(a, RebufferAction::Enter);
        // Refill to the cushion (the most the ring can hold) → must EXIT, not hang.
        let a = rebuffer_action(
            true,
            false,
            true,
            false,
            HR_FR,
            HR_BACKPRESSURE_CAP,
            &mut reb,
            &mut primed,
            &mut ticks,
        );
        assert_eq!(
            a,
            RebufferAction::Exit,
            "hi-res rebuffer must resume within the backpressure cushion, not hang"
        );
        assert!(!reb);
    }

    /// REGRESSION (issue-9 hi-res rebuffer deadlock): the rebuffer must ENTER the
    /// pause strictly below the decode loop's backpressure-RELEASE point. The
    /// decode loop stops decoding while the ring sits above that release point
    /// (its hysteresis latch stays set), so a rebuffer that pauses the output
    /// above it freezes the ring inside the backpressure band — the decode loop
    /// never refills, and the pause hangs until MAX_REBUFFER_TICKS (~10 s). At
    /// 96 k stereo the old fixed-sample entry watermark (38_400) sat ABOVE the
    /// fixed release point (32_000); this asserts the entry is below release at
    /// every sample rate (the now-time-based thresholds make it hold universally).
    #[test]
    fn rebuffer_entry_stays_below_decode_backpressure_release() {
        // Decode-loop release point = compute_watermarks low = cushion/3, scaled.
        let release = |fr: u32| {
            (fr as u64 * crate::audio::engine::CUSHION_MS
                / (1000 * crate::audio::engine::BACKPRESSURE_RELEASE_DIVISOR)) as usize
        };
        assert!(
            rebuffer_low_samples(FR) < release(FR),
            "44.1k: rebuffer entry {} must be below decode release {}",
            rebuffer_low_samples(FR),
            release(FR),
        );
        assert!(
            rebuffer_low_samples(HR_FR) < release(HR_FR),
            "96k: rebuffer entry {} must be below decode release {} (issue-9 deadlock)",
            rebuffer_low_samples(HR_FR),
            release(HR_FR),
        );
    }

    /// The extracted `reset_rebuffer_latch` helper must zero all three latch
    /// fields. This guards the helper BODY itself; it does not assert that
    /// start/stop/seek call it (that call-site coverage is unchanged from before
    /// the extraction — finalize_crossfade keeps its own dedicated assertion
    /// below). `#[tokio::test]` because `AudioRenderer::new()` calls
    /// `tokio::runtime::Handle::current()` and needs a running reactor.
    #[tokio::test]
    async fn reset_rebuffer_latch_zeroes_all_three_fields() {
        let mut renderer = AudioRenderer::new();
        renderer.rebuffering = true;
        renderer.rebuffer_primed = true;
        renderer.rebuffer_ticks = 42;

        renderer.reset_rebuffer_latch();

        assert!(!renderer.rebuffering, "must clear the rebuffering flag");
        assert!(
            !renderer.rebuffer_primed,
            "must clear rebuffer_primed so a fresh ring re-primes"
        );
        assert_eq!(renderer.rebuffer_ticks, 0, "must reset rebuffer_ticks");
    }

    /// A crossfade from a primeable (<=48k) track into a hi-res track must NOT
    /// carry a stale `rebuffer_primed` across the format change — otherwise the
    /// hi-res track could enter rebuffer on an unreachable resume target and hang
    /// ~10s (issue #9 crossfade carryover). `finalize_crossfade` must reset the
    /// rebuffer latch like `start()`/`stop()`/`seek()` do.
    #[tokio::test]
    async fn finalize_crossfade_resets_rebuffer_latch() {
        let mut renderer = AudioRenderer::new();
        let (incoming, _src) = test_active_stream(0);
        // Simulate the latch carried over from the outgoing low-rate track.
        renderer.rebuffering = true;
        renderer.rebuffer_primed = true;
        renderer.rebuffer_ticks = 42;
        // Completed crossfade whose incoming track is hi-res (96k stereo).
        renderer.crossfade_state = CrossfadeState::Active {
            stream: incoming,
            started_at: std::time::Instant::now() - std::time::Duration::from_secs(60),
            duration_ms: 1_000,
            incoming_format: AudioFormat::new(crate::audio::format::SampleFormat::S16, 96_000, 2),
            paused_accum: std::time::Duration::ZERO,
            paused_at: None,
        };

        renderer.finalize_crossfade();

        assert!(
            !renderer.rebuffer_primed,
            "finalize_crossfade must clear rebuffer_primed so the promoted track re-primes"
        );
        assert!(
            !renderer.rebuffering,
            "finalize_crossfade must clear the rebuffering flag"
        );
        assert_eq!(
            renderer.rebuffer_ticks, 0,
            "finalize_crossfade must reset rebuffer_ticks"
        );
    }

    /// Build a detached `ActiveStream` for renderer crossfade unit tests.
    ///
    /// Splits a `HeapRb` (the renderer's real ring buffer type), wires the
    /// consumer to a throwaway `StreamingSource` (kept alive so the handle's
    /// shared state stays valid), and returns the producer-side `ActiveStream`.
    /// Pre-loads `prefill` samples into the ring so `crossfade_buffer_count()`
    /// reports a non-zero fill when the test needs a "healthy incoming" stream.
    fn test_active_stream(prefill: usize) -> (ActiveStream, StreamingSource) {
        let rb = HeapRb::<f32>::new(crate::audio::RING_BUFFER_CAPACITY);
        let (mut producer, consumer) = rb.split();
        if prefill > 0 {
            let data: Vec<f32> = (0..prefill).map(|i| i as f32 * 0.001).collect();
            producer.push_slice(&data);
        }
        let viz: SharedVisualizerCallback = Arc::new(parking_lot::RwLock::new(None));
        let (source, handle) = StreamingSource::new(
            consumer,
            NonZero::new(2).expect("2 is nonzero"),
            NonZero::new(48_000).expect("48000 is nonzero"),
            viz,
            0.0,
            None,
            Arc::new(Notify::new()),
            false,
            Arc::new(std::sync::atomic::AtomicBool::new(true)),
            false,
        );
        let stream = ActiveStream {
            producer,
            handle,
            sample_rate: 48_000,
            channels: 2,
        };
        (stream, source)
    }

    /// Build an `Active` crossfade state that is already past completion on the
    /// wall clock (`started_at` 60s ago, 1s fade), with the supplied incoming
    /// stream. The returned `StreamingSource` must be kept alive for the
    /// duration of the test so the handle's atomics remain valid.
    fn completed_active_state(stream: ActiveStream) -> CrossfadeState {
        CrossfadeState::Active {
            stream,
            started_at: std::time::Instant::now() - std::time::Duration::from_secs(60),
            duration_ms: 1_000,
            incoming_format: AudioFormat::invalid(),
            paused_accum: std::time::Duration::ZERO,
            paused_at: None,
        }
    }

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

    /// Regression (Relaxed bit-perfect): `cancel_crossfade` must restore the
    /// honest badge to the outgoing stream's build-time fact. `start_crossfade`
    /// drops `current_stream_bit_perfect` to false for the (non-bit-perfect)
    /// blend; cancelling the fade (skip / seek / mid-fade mode toggle) restores
    /// the outgoing as the sole live stream, so the badge must read its
    /// bit-perfect-ness again instead of staying stuck false.
    #[tokio::test]
    async fn cancel_crossfade_restores_bit_perfect_badge() {
        let mut renderer = AudioRenderer::new();
        renderer.set_bit_perfect(BitPerfectMode::Relaxed);
        renderer.pw_volume_active = true; // makes bit_perfect_active() viable
        let (incoming, _isrc) = test_active_stream(0);
        renderer.crossfade_state = completed_active_state(incoming);
        // start_crossfade had dropped the badge for the blend.
        renderer.current_stream_bit_perfect = false;

        renderer.cancel_crossfade();

        assert!(
            renderer.current_stream_bit_perfect,
            "cancel_crossfade under viable Relaxed must restore the bit-perfect badge"
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

    /// I5: crossfade progress must exclude time spent paused. Pausing mid-fade
    /// then resuming previously skipped the fade forward by the pause duration
    /// (or hard-cut to the incoming track), because progress was pure wall
    /// clock. With pause-aware accounting, 8s wall clock minus 4s paused over a
    /// 5s fade is 4s of real playing time → 0.8 progress, NOT a completed fade.
    #[test]
    fn crossfade_progress_excludes_paused_time() {
        assert!(
            (crossfade_progress(8000, 4000, 5000) - 0.8).abs() < 1e-9,
            "8000ms elapsed − 4000ms paused over a 5000ms fade must be 0.8 progress"
        );
    }

    /// I5 control: when no pause occurred and real playing time exceeds the
    /// fade duration, progress finalizes correctly at 1.0 (clamped).
    #[test]
    fn crossfade_progress_finalizes_only_on_real_playing_time() {
        assert!(
            (crossfade_progress(6000, 0, 5000) - 1.0).abs() < 1e-9,
            "6000ms of real playing time over a 5000ms fade must clamp to 1.0"
        );
    }

    /// I5: a zero-duration fade is treated as immediately complete.
    #[test]
    fn crossfade_progress_zero_duration_is_complete() {
        assert!((crossfade_progress(0, 0, 0) - 1.0).abs() < 1e-9);
    }

    /// I6: when the fade reaches completion but the incoming ring is empty (the
    /// incoming decoder stalled / never produced audio), `tick_crossfade` must
    /// report `IncomingStalled` so the caller can restore the outgoing track —
    /// NOT silently `Finalize` and promote a silent stream.
    #[tokio::test]
    async fn tick_crossfade_reports_stall_when_incoming_empty_at_completion() {
        let (stream, _source) = test_active_stream(0);
        let mut renderer = AudioRenderer::new();
        renderer.playing = true;
        renderer.crossfade_state = completed_active_state(stream);

        assert_eq!(
            renderer.crossfade_buffer_count(),
            0,
            "incoming ring is empty"
        );
        assert_eq!(
            renderer.tick_crossfade(),
            CrossfadeTick::IncomingStalled,
            "completed fade with empty incoming must report a stall"
        );
    }

    /// I6 control: when the fade completes and the incoming stream produced
    /// audio (non-empty ring), `tick_crossfade` reports `Finalize` so a healthy
    /// crossfade still promotes the incoming track exactly as before.
    #[tokio::test]
    async fn tick_crossfade_finalizes_when_incoming_filled_at_completion() {
        let (stream, _source) = test_active_stream(4_096);
        let mut renderer = AudioRenderer::new();
        renderer.playing = true;
        renderer.crossfade_state = completed_active_state(stream);

        assert!(
            renderer.crossfade_buffer_count() > 0,
            "incoming ring is filled"
        );
        assert_eq!(
            renderer.tick_crossfade(),
            CrossfadeTick::Finalize,
            "completed fade with filled incoming must finalize"
        );
    }

    /// N16: a fresh renderer always carries an `EqState` (no `Option`), so
    /// every stream it creates gets an `EqProcessor` even before `set_eq_state`
    /// is called. Previously the field defaulted to `None`, so a stream created
    /// before login's settings push had a `None` processor for its entire life
    /// and ignored EQ even after the user enabled it.
    #[tokio::test]
    async fn fresh_renderer_seeds_eq_state() {
        let renderer = AudioRenderer::new();
        // The seeded default starts disabled (a true no-op for the audio path).
        assert!(
            !renderer.eq_state.is_enabled(),
            "seeded EQ state must default to disabled",
        );
    }

    /// N16: `set_eq_state` replaces the stored instance with the canonical
    /// UI-owned `EqState` so streams created afterwards track the live atomics
    /// (enabling EQ via the shared handle becomes visible to the renderer).
    #[tokio::test]
    async fn set_eq_state_shares_canonical_atomics() {
        let mut renderer = AudioRenderer::new();
        let canonical = super::super::eq::EqState::new();
        renderer.set_eq_state(canonical.clone());

        // Toggling the canonical instance (as the UI would) must be visible
        // through the renderer's stored state — proving they share atomics.
        canonical.set_enabled(true);
        assert!(
            renderer.eq_state.is_enabled(),
            "renderer must store the shared instance, not a default copy",
        );
    }

    /// I6: while the fade is still in progress, `tick_crossfade` reports
    /// `Continue` regardless of incoming fill.
    #[tokio::test]
    async fn tick_crossfade_continues_before_completion() {
        let (stream, _source) = test_active_stream(0);
        let mut renderer = AudioRenderer::new();
        renderer.playing = true;
        renderer.crossfade_state = CrossfadeState::Active {
            stream,
            started_at: std::time::Instant::now(),
            duration_ms: 10_000,
            incoming_format: AudioFormat::invalid(),
            paused_accum: std::time::Duration::ZERO,
            paused_at: None,
        };

        assert_eq!(renderer.tick_crossfade(), CrossfadeTick::Continue);
    }

    /// Regression (crossfade-pause-freeze): the engine unpauses via
    /// `engine.play() → renderer.start()`, NOT `renderer.resume()`. So
    /// `start()` MUST fold any in-progress crossfade pause span into
    /// `paused_accum` and clear `paused_at`. If it leaves `paused_at` set,
    /// `tick_crossfade`'s `live_paused` tracks wall clock in lockstep with
    /// `elapsed_ms`, freezing crossfade progress below 1.0 forever — the fade
    /// never finalizes, the UI position pins at the outgoing track's end, and
    /// the queue never advances even though the incoming stream is audible.
    #[tokio::test]
    async fn start_folds_active_crossfade_pause_into_accum() {
        let (stream, _source) = test_active_stream(4_096);
        let mut renderer = AudioRenderer::new();
        // Mirror the renderer state when the engine calls start() to resume:
        // still flagged playing (pause() never cleared it) and paused.
        renderer.playing = true;
        renderer.paused = true;
        // A crossfade that began 10s ago and was paused 8s ago (2s into a 5s fade).
        renderer.crossfade_state = CrossfadeState::Active {
            stream,
            started_at: std::time::Instant::now() - std::time::Duration::from_secs(10),
            duration_ms: 5_000,
            incoming_format: AudioFormat::invalid(),
            paused_accum: std::time::Duration::ZERO,
            paused_at: Some(std::time::Instant::now() - std::time::Duration::from_secs(8)),
        };

        // Unpause through the real engine path.
        renderer.start();

        let CrossfadeState::Active {
            paused_at,
            paused_accum,
            ..
        } = &renderer.crossfade_state
        else {
            panic!("crossfade must remain Active across an unpause");
        };
        assert!(
            paused_at.is_none(),
            "start() must clear paused_at on unpause (a dangling paused_at freezes progress)"
        );
        assert!(
            *paused_accum >= std::time::Duration::from_secs(7),
            "the ~8s paused span must be folded into paused_accum, got {paused_accum:?}"
        );
    }

    /// Regression: repeated pause/resume cycles within a single crossfade must
    /// record EACH paused span. `pause()` only stamps `paused_at` when it is
    /// `None`, so if `start()` fails to clear it on the first resume, the second
    /// pause is silently dropped and `paused_accum` undercounts — letting
    /// progress run ahead. Driving pause→start→pause→start through the real
    /// engine path must clear `paused_at` on every resume and allow every pause
    /// to re-stamp.
    #[tokio::test]
    async fn repeated_pause_resume_via_start_clears_paused_at_each_cycle() {
        let (stream, _source) = test_active_stream(4_096);
        let mut renderer = AudioRenderer::new();
        renderer.playing = true;
        renderer.crossfade_state = CrossfadeState::Active {
            stream,
            started_at: std::time::Instant::now() - std::time::Duration::from_secs(2),
            duration_ms: 10_000,
            incoming_format: AudioFormat::invalid(),
            paused_accum: std::time::Duration::ZERO,
            paused_at: None,
        };

        // First pause/resume cycle.
        renderer.pause();
        assert!(
            matches!(
                renderer.crossfade_state,
                CrossfadeState::Active {
                    paused_at: Some(_),
                    ..
                }
            ),
            "first pause must stamp paused_at"
        );
        renderer.start();
        assert!(
            matches!(
                renderer.crossfade_state,
                CrossfadeState::Active {
                    paused_at: None,
                    ..
                }
            ),
            "first unpause via start() must clear paused_at"
        );

        // Second pause/resume cycle: pause must be able to re-stamp.
        renderer.pause();
        assert!(
            matches!(
                renderer.crossfade_state,
                CrossfadeState::Active {
                    paused_at: Some(_),
                    ..
                }
            ),
            "second pause must re-stamp paused_at (impossible if the first was never cleared)"
        );
        renderer.start();
        let CrossfadeState::Active { paused_at, .. } = &renderer.crossfade_state else {
            panic!("crossfade must remain Active");
        };
        assert!(
            paused_at.is_none(),
            "second unpause via start() must clear paused_at"
        );
    }
}
