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
        AudioFormat, IncomingLiveness, NormalizationConfig, NormalizationContext, SourceGeneration,
        format::samples_for_duration,
        resolve_normalization,
        rodio_output::{ActiveStream, RodioOutput},
        streaming_source::SharedVisualizerCallback,
    },
    types::{
        player_settings::{BitPerfectMode, CrossfadeCurve, VolumeNormalizationMode},
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
        /// Fade curve CAPTURED from the live setting at `start_crossfade`.
        /// `tick_crossfade` reads this, never `self.crossfade_curve`, so a
        /// mid-fade settings change cannot tear the in-flight envelope (the
        /// new curve applies from the next crossfade).
        curve: CrossfadeCurve,
    },
}

/// What a completed transport fade-out resolves into (M5).
///
/// `Pause` ends in the real stream-level `pause()` (position freezes, ring
/// kept); `Stop` ends in `silence_and_stop()` (stream removed from the mixer
/// — the engine's teardown then proceeds). The M7 boundary skip fade
/// (`engine.run_skip_out_fade`) reuses the `Stop` target — its end action
/// is identical, and the engine's follow-up (`set_source`'s internal stop +
/// fresh load) owns the difference — so no separate `Skip` variant exists
/// (M6's radio-switch fade set the precedent).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransportFadeTarget {
    Pause,
    Stop,
}

/// Renderer-side transport-fade state machine (M5): short gain ramps for
/// pause / resume / stop instead of flipping the `paused`/`stopped` atomics
/// mid-waveform (a guaranteed click). Driven by the 20 ms render thread.
///
/// SEPARATE from [`CrossfadeState`] by design (mirroring the deliberate
/// engine-phase/renderer-state split): a transport fade engages only while
/// the crossfade machine is `Idle` and never fights a blend.
///
/// Guard-lift design: `begin_pause_fade` / `begin_stop_fade` set
/// `self.paused = true` IMMEDIATELY (freezing the completion gate, the
/// rebuffer path, and the armed crossfade trigger below `render_tick`'s
/// early-return), and the ramp itself is ticked ABOVE that early-return.
/// Deferring `paused` until ramp completion would leave `render_tick` fully
/// live for the whole ~100 ms — a pause near end-of-track could then fire
/// `on_renderer_finished` and advance the queue mid-ramp.
///
/// `gen` is the per-fade generation token: a completion applies only when it
/// matches the renderer's live counter, so a ramp that was interrupted (or
/// cancelled) can never apply its stale end-state action.
enum TransportFade {
    Idle,
    FadingOut {
        target: TransportFadeTarget,
        started_at: std::time::Instant,
        duration_ms: u64,
        generation: u64,
        /// Volume seed captured from the stream's last-written `volume`
        /// atomic at begin time — an interrupting ramp continues from the
        /// audible level instead of snapping.
        from: f32,
        /// One-tick hold between the ramp floor and the end-state action.
        /// The audible gain is the consumer-side ~5 ms-tau EMA in
        /// `StreamingSource::next()`, which LAGS the `volume` atomic — and
        /// `next()` emits silence the instant the paused/stopped atomic
        /// flips, so applying the end action in the same tick as the final
        /// 0.0 write cuts at the EMA's pre-floor gain (at the 20 ms slider
        /// minimum ≈ one tick period, a near/full-amplitude hard cut — the
        /// exact click the ramp exists to remove). The floor tick sets this
        /// and returns; the NEXT tick (20 ms ≈ 4 EMA time constants at
        /// silence) applies the real pause / silence_and_stop.
        settling: bool,
    },
    FadingIn {
        started_at: std::time::Instant,
        duration_ms: u64,
        generation: u64,
        /// See `FadingOut::from`.
        from: f32,
    },
    /// M6 radio-switch fade-in: hold the fresh primary SILENT — ignoring
    /// wall clock — until the mixer pulls real samples past
    /// `baseline_samples` (`samples_consumed` counts only real pulls, never
    /// paused or ring-starvation silence), then ramp 0 → `stream_volume()`
    /// over [`RADIO_SWITCH_FADE_MS`]. A wall-clock ramp from stream creation
    /// would finish during the radio prebuffer silence and pop to full gain
    /// when audio finally arrives.
    ///
    /// Unlike the M5 ramps, this fade SURVIVES pause/start cycles: the radio
    /// jitter prebuffer re-issues `pause()` every decoded chunk until ~5 s
    /// is buffered, then unpauses via `start()`. `pause()` RE-ARMS the fade
    /// (back to holding, with a fresh baseline) instead of cancelling —
    /// play()'s prebuffer lets the mixer pull a silent TRICKLE of real
    /// samples before the first jitter pause, and without the re-baseline
    /// that trickle would start (and silently burn) the ramp during the
    /// hold, popping the true onset at full gain.
    SwitchFadeIn {
        /// `None` while holding for real consumption past
        /// `baseline_samples`; `Some(ramp start)` once audio flows.
        started_at: Option<std::time::Instant>,
        /// `samples_consumed` snapshot at (re)arm time — the ramp starts
        /// only once consumption moves PAST this. A read BELOW it means the
        /// counter was reset (new-track bookkeeping); re-baseline then, or
        /// the fade could hold at silence forever.
        baseline_samples: u64,
        generation: u64,
    },
}

/// Radio-switch fade length in milliseconds (M6, "Fade Radio Switches").
/// Fixed by design — the setting is a single Bool with no duration knob: a
/// deliberate soft edge for the out-ramp and the first-audio in-ramp without
/// noticeably delaying the switch (the out-ramp bounds the engine's
/// switch-stop at ~270 ms including the settle tick).
pub(crate) const RADIO_SWITCH_FADE_MS: u64 = 250;

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

/// M8 trailing-silence trim: how far before the NORMAL trigger point
/// (`track_dur − fade`) the Armed branch starts watching the level meter.
/// Bounds the early trigger to a genuine trailing tail — a quiet passage
/// mid-song can never fire it. 15 s covers real-world fade-outs and hidden
/// pregap silence without swallowing whole quiet outros.
const TRAILING_SILENCE_WINDOW_MS: u64 = 15_000;

/// M8 trailing-silence trim: consecutive sub-threshold render ticks (20 ms
/// each ⇒ 500 ms of sustained near-digital silence) required before the
/// early trigger fires. One noisy meter window resets the count.
const TRAILING_SILENCE_SUSTAIN_TICKS: u32 = 25;

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
    /// Fade curve applied to NEW crossfades (pushed from settings via
    /// [`Self::set_crossfade_curve`]). `start_crossfade` captures it into the
    /// `Active` variant; an in-flight fade keeps its captured curve.
    crossfade_curve: CrossfadeCurve,
    /// Minimum track length (ms) for crossfade eligibility — the
    /// [`Self::arm_crossfade`] floor, pushed from settings via
    /// [`Self::set_crossfade_min_track_secs`] (M4; historically the
    /// hardcoded 10 s `MIN_CROSSFADE_TRACK_MS`). Tracks shorter than this
    /// fall back to a gapless transition.
    crossfade_min_track_ms: u64,
    /// Elapsed crossfade time (ms) staged after `finalize_crossfade` so the
    /// engine can read it on the next render tick as a position offset.
    /// Lives outside `CrossfadeState` because it survives the Active→Idle
    /// transition by exactly one tick.
    crossfade_finalized_elapsed_ms: u64,
    /// M8 negative "Gap / Overlap Trim": how much EARLIER (ms) the Armed
    /// position trigger fires than `track_dur − fade` — the blend starts
    /// early and `try_finalize_crossfade` discards the outgoing's last
    /// `crossfade_lead_ms` of tail (invariant 5's finalize-before-EOF path).
    /// 0 for a zero/positive offset (the gap side lives in the engine decode
    /// loop, not here). Pushed from settings via
    /// [`Self::set_crossfade_lead_ms`].
    crossfade_lead_ms: u64,
    /// M8 "Skip Silence Between Tracks" renderer mirror — gates the
    /// trailing-silence early trigger in `render_tick`'s Armed branch (the
    /// leading-trim half lives decoder-side). Pushed from settings via
    /// [`Self::set_skip_silence`].
    skip_silence: bool,
    /// M8 trailing-silence sustain counter: consecutive render ticks (20 ms
    /// each) inside the trailing window whose meter reading sat below
    /// [`crate::audio::SOURCE_SILENCE_THRESHOLD`]. Reset by any loud
    /// reading, by leaving the window, and by `arm_crossfade` (each armed
    /// transition starts fresh). The early trigger fires at
    /// [`TRAILING_SILENCE_SUSTAIN_TICKS`].
    trailing_silence_ticks: u32,
    /// M9 "already-signalled" latch: set the first time a completed fade's
    /// stall is signalled to the engine, so `render_tick` (20 ms cadence)
    /// cannot respawn `recover_stalled_crossfade` every tick while the one
    /// spawned recovery task waits on the engine lock. Cleared whenever the
    /// Active fade it guards is torn down or replaced (`start_crossfade`,
    /// `cancel_crossfade`, `finalize_crossfade` — every cancel path,
    /// including recovery itself and `stop()`, funnels through
    /// `cancel_crossfade`, so a stranded latch is structurally impossible).
    stall_recovery_signalled: bool,
    /// M9 Part B liveness handle for the CURRENT fade's incoming decode
    /// loop, installed by the engine's `start_crossfade_decode_loop` right
    /// after it spawns the loop (a fresh per-fade instance, so a superseded
    /// loop's late writes can never pollute a newer fade's verdict) and
    /// cleared on the same lifecycle edges as the stall latch. `None` (no
    /// loop installed yet, or a fade whose engine half never started) always
    /// reads as live — the empty-ring completion gate still covers that.
    incoming_liveness: Option<Arc<IncomingLiveness>>,

    /// Transport-fade phase + per-phase data (M5). See [`TransportFade`].
    transport_fade: TransportFade,
    /// Live generation counter for transport fades. Every `begin_*` bumps it
    /// and stamps the new value into the state; `cancel_transport_fade` bumps
    /// it without starting a ramp. A completion whose stamped `gen` no longer
    /// matches is stale and applies no end-state action.
    transport_fade_gen: u64,
    /// Renderer-side mirror of the "Fade on Pause / Resume" setting — the
    /// renderer owns the enforcing copy because BOTH consumers are here
    /// (`begin_pause_fade` and `start()`'s resume fade-in). The stop pair
    /// lives on the engine's `FadeCoordinator` (its consumer is
    /// `engine.stop()`). Pushed via [`Self::set_pause_fade`].
    fade_on_pause: bool,
    /// Pause/resume ramp length in milliseconds (mirror, see `fade_on_pause`).
    fade_pause_ms: u64,
    /// Whether new NON-bit-perfect streams get the M2 de-click onset ramp
    /// (the "Smooth Track Starts" setting, default on). Threaded into every
    /// stream build; `false` restores the instant, honest onset. Bit-perfect
    /// streams never ramp regardless (invariant 8).
    smooth_track_starts: bool,
    /// M6 "Fade Radio Switches": one-shot request set by the engine after a
    /// radio-switch teardown (queue→radio via `stop_for_radio_switch`,
    /// radio→queue via `set_source`'s internal stop). Consumed at the next
    /// fresh primary-stream build (`init()`, with a fresh-`start()`
    /// belt-and-braces) — which arms the first-audio fade-in on the new
    /// primary — and cleared by `stop()` so it can never leak past a
    /// teardown into an unrelated later play.
    pending_switch_fade_in: bool,
    /// Count of REAL transport-fade completions (ramp reached 1.0 and its
    /// end-state action applied — never cancels, never stale discards).
    /// Test observability only: the engine's stop-ordering test asserts the
    /// fade completed via live render ticks rather than timing heuristics.
    #[cfg(test)]
    transport_fade_completions: u64,
    /// Count of stall-recovery signals actually sent past the M9 latch.
    /// Test observability only: pins that a stalled fade signals recovery
    /// exactly once, not once per 20 ms tick.
    #[cfg(test)]
    stall_signals_sent: u64,

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

    /// Take the staged crossfade RG out (leaving `None`). Used by the
    /// engine's skip-fade stall recovery to carry the skip target's RG into
    /// the hard reload BEFORE `cancel_crossfade` drops the staged copy.
    pub fn take_pending_crossfade_replay_gain(&mut self) -> Option<ReplayGain> {
        self.pending_crossfade_replay_gain.take()
    }

    /// Test-only: the promoted stream's ReplayGain bookkeeping, so engine
    /// tests can pin the finalize-time RG promotion.
    #[cfg(test)]
    pub fn current_replay_gain_for_test(&self) -> Option<ReplayGain> {
        self.current_replay_gain.clone()
    }

    /// Test-only: the staged next-transition ReplayGain, so engine tests can
    /// pin that a mid-blend prep never clobbers the live blend's staged copy.
    #[cfg(test)]
    pub fn pending_crossfade_replay_gain_for_test(&self) -> Option<ReplayGain> {
        self.pending_crossfade_replay_gain.clone()
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
            crossfade_curve: CrossfadeCurve::default(),
            crossfade_min_track_ms: u64::from(
                crate::types::player_settings::CROSSFADE_MIN_TRACK_DEFAULT_SECS,
            ) * 1000,
            crossfade_finalized_elapsed_ms: 0,
            crossfade_lead_ms: 0,
            skip_silence: false,
            trailing_silence_ticks: 0,
            stall_recovery_signalled: false,
            incoming_liveness: None,
            transport_fade: TransportFade::Idle,
            transport_fade_gen: 0,
            fade_on_pause: false,
            fade_pause_ms: u64::from(crate::types::player_settings::TRANSPORT_FADE_MS_DEFAULT),
            smooth_track_starts: true,
            pending_switch_fade_in: false,
            #[cfg(test)]
            transport_fade_completions: 0,
            #[cfg(test)]
            stall_signals_sent: 0,
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

        // New-track lifecycle: any in-flight transport ramp (M5) belonged to
        // the previous stream — abandon it so it can't ramp/pause/stop the
        // stream this init installs (or reuses, on the gapless path). On the
        // GAPLESS-REUSE path the volume atomic must also be restored: this is
        // the one cancel site that keeps the existing stream, and under
        // `pw_volume_active` nothing downstream rewrites the atomic
        // (`set_volume` early-returns; `start()` early-returns while already
        // playing) — a resume fade-in interrupted here would otherwise strand
        // the next track at the mid-ramp level until the next rebuild. The
        // fresh-stream path overwrites the volume at `create_stream` anyway.
        if !self.transport_fade_idle() {
            self.cancel_transport_fade();
            if let Some(ref stream) = self.primary_stream {
                stream.set_volume(self.stream_volume());
            }
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
            1.0, // fresh stream — no fade in progress
            norm,
            Some(self.eq_state.clone()),
            self.consumed_notify.clone(),
            true,
            self.smooth_track_starts,
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

        // M6: arm a requested switch fade-in HERE, on the just-built stream —
        // the mixer starts pulling the moment play()'s prebuffer writes
        // samples (before `start()`), so arming any later would let the
        // switch onset escape at full gain ahead of the hold.
        self.maybe_arm_pending_switch_fade();

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

        // Captured BEFORE clearing: `start()` is both the fresh-play and the
        // resume path, and only a resume (paused → playing) may fade back in.
        let was_paused = self.paused;

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
        //
        // M5 resume fade-in: when this start() is a RESUME and the pause fade
        // is enabled (and the stream is rampable — never bit-perfect, never
        // mid-crossfade), ramp back up from the stream's last-written volume
        // atomic instead of snapping: 0.0 after a completed pause fade, the
        // interrupted mid-level when resuming during the out-ramp, and a
        // no-op ramp (from == target) after an instant pause. Otherwise any
        // stale ramp is cancelled and the volume restored instantly, exactly
        // as before.
        if !matches!(self.crossfade_state, CrossfadeState::Active { .. })
            && self.primary_stream.is_some()
        {
            if matches!(self.transport_fade, TransportFade::SwitchFadeIn { .. }) {
                // M6: an armed switch fade-in survives pause/start cycles —
                // the radio jitter prebuffer pauses and restarts the
                // renderer before real playback begins (pause() re-armed it
                // back to holding), and cancelling or restoring full volume
                // here would snap the eventual onset to full gain.
            } else if !was_paused && self.pending_switch_fade_in {
                // M6 belt-and-braces: in production the init-time arm has
                // already consumed the request; this covers a fresh start
                // that somehow skipped init (and is the unit-testable arm).
                self.maybe_arm_pending_switch_fade();
            } else if was_paused && self.fade_on_pause && self.transport_fade_engageable() {
                self.begin_fade_in(self.fade_pause_ms);
            } else {
                self.cancel_transport_fade();
                if let Some(ref stream) = self.primary_stream {
                    stream.set_volume(self.stream_volume());
                }
            }
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
        // Abandon any in-flight transport ramp (M5) — the stream is being
        // torn down; its deferred end-state action must not apply. Also drop
        // an unconsumed switch fade-in request (M6): a torn-down switch must
        // never leak a soft start into an unrelated later play.
        self.cancel_transport_fade();
        self.pending_switch_fade_in = false;

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
        // An instant pause supersedes any in-flight transport ramp (M5) — the
        // stream flips silent right here, so the ramp's deferred end-state
        // action must never apply afterwards. EXCEPTION (M6): a switch
        // fade-in is RE-ARMED back to holding (fresh baseline) instead — the
        // radio jitter prebuffer re-issues pause() every decoded chunk until
        // ~5 s is buffered, and play()'s prebuffer lets the mixer pull a
        // silent trickle beforehand; cancelling (or letting a
        // trickle-started ramp burn off during the hold) would pop the true
        // onset at full gain. It has no deferred end action, so keeping it
        // is safe.
        if let TransportFade::SwitchFadeIn {
            started_at,
            baseline_samples,
            ..
        } = &mut self.transport_fade
        {
            *started_at = None;
            *baseline_samples = self
                .primary_stream
                .as_ref()
                .map_or(0, |s| s.handle.samples_consumed.load(Ordering::Relaxed));
            if let Some(ref stream) = self.primary_stream {
                stream.set_volume(0.0);
            }
        } else {
            self.cancel_transport_fade();
        }
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
        // The primary is about to be recreated — a stale transport ramp (M5)
        // must not keep writing volumes to (or pause/stop) the NEW stream.
        self.cancel_transport_fade();

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
                1.0, // seek recreates a non-fading stream
                norm,
                Some(self.eq_state.clone()),
                self.consumed_notify.clone(),
                true,
                self.smooth_track_starts,
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
            // During a crossfade the tick writes only `fade_coeff`, so a
            // mid-fade user-volume change must be pushed to BOTH streams'
            // `volume` atomics here — and UNCONDITIONALLY (no `!self.paused`
            // gate): the tick doesn't run while paused and `start()` skips the
            // volume restore when Active, so a change made while paused
            // mid-crossfade would otherwise be permanently lost. Pushing to a
            // paused stream is harmless (it emits silence regardless).
            if let Some(ref stream) = self.primary_stream {
                stream.set_volume(volume as f32);
            }
            if let CrossfadeState::Active { stream, .. } = &self.crossfade_state {
                stream.set_volume(volume as f32);
            }
        } else if !self.paused
            && !self.transport_fade_awaiting_first_audio()
            && let Some(ref stream) = self.primary_stream
        {
            // The awaiting-first-audio guard (M6): while a switch fade-in
            // holds the fresh stream silent, a user volume change must not
            // punch through the hold — `self.volume` is updated above, and
            // the fade's target (`stream_volume()`) picks it up when the
            // ramp runs.
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

    /// Set the crossfade fade curve from settings. Applies to the NEXT
    /// crossfade: `start_crossfade` captures the live value into the `Active`
    /// variant, so an in-flight fade keeps the curve it started with (a
    /// mid-fade change must not tear the envelope). A curve change never
    /// flips crossfade eligibility, so — like `set_crossfade_duration` — this
    /// is a bare write with no `reset_next_track`.
    pub fn set_crossfade_curve(&mut self, curve: CrossfadeCurve) {
        if self.crossfade_curve != curve {
            tracing::info!("🔀 Renderer: crossfade curve {}", curve);
        }
        self.crossfade_curve = curve;
    }

    /// Set the minimum-track-length crossfade floor from settings (seconds).
    /// Applies at the NEXT [`Self::arm_crossfade`]; like
    /// [`Self::set_crossfade_curve`] and the duration slider this is a bare
    /// write with no `reset_next_track` — an already-armed transition keeps
    /// the floor it was armed under (fires once), and cancelling a live
    /// blend on every slider step would hard-cut audio.
    pub fn set_crossfade_min_track_secs(&mut self, secs: u32) {
        let ms = u64::from(secs) * 1000;
        if self.crossfade_min_track_ms != ms {
            tracing::info!("🔀 Renderer: crossfade min track length {}s", secs);
        }
        self.crossfade_min_track_ms = ms;
    }

    /// Set the M8 negative-offset lead (ms): how much earlier than
    /// `track_dur − fade` the Armed trigger fires. The engine's
    /// `set_crossfade_offset` pushes the NEGATIVE side of the "Gap / Overlap
    /// Trim" knob here (0 for zero/positive offsets — the gap side lives in
    /// the decode loop). Bare write, like the sibling sliders: it never
    /// flips eligibility. Unlike the duration (captured into the Armed
    /// variant), the trigger reads this field live, so a slider change
    /// applies to an already-armed transition too — moving a trigger point
    /// is tear-free (nothing in-flight to tear).
    pub fn set_crossfade_lead_ms(&mut self, lead_ms: u64) {
        if self.crossfade_lead_ms != lead_ms {
            tracing::info!("🔀 Renderer: crossfade overlap lead {}ms", lead_ms);
        }
        self.crossfade_lead_ms = lead_ms;
    }

    /// Set the M8 "Skip Silence Between Tracks" renderer mirror (gates the
    /// trailing-silence early trigger; the engine setter owns the
    /// reset-on-change mode-toggle contract).
    pub fn set_skip_silence(&mut self, enabled: bool) {
        if self.skip_silence != enabled {
            tracing::info!("🤫 Renderer: skip silence between tracks {}", enabled);
        }
        self.skip_silence = enabled;
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

    /// Arm the renderer for crossfade with duration clamping.
    ///
    /// Guards (inspired by MPD's `CanCrossFadeSong`):
    /// 1. Both durations must be KNOWN (non-zero) — a zero
    ///    `track_duration_ms` would make the Armed position trigger
    ///    (`pos >= track_duration − fade`) fire immediately at track start
    /// 2. Both songs must be >= the configured minimum track length
    ///    (`crossfade_min_track_ms`, default 10s; 0 = blend everything known)
    /// 3. Effective duration is clamped to `min(xfade, track/2)` so the
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

        // Guard: unknown durations can't crossfade at ANY floor — with a zero
        // outgoing duration the position trigger would fire immediately, and a
        // zero incoming duration degenerates the `shorter/2` clamp to a 0ms
        // fade. Unreachable while the floor was a hardcoded 10s; load-bearing
        // now that the configured floor may be 0.
        if track_duration_ms == 0 || incoming_duration_ms == 0 {
            debug!(
                "🔀 [RENDERER] Crossfade SKIPPED: unknown duration (track={}ms, incoming={}ms)",
                track_duration_ms, incoming_duration_ms,
            );
            return;
        }

        // Guard: skip crossfade for short songs (fall back to gapless)
        let min_dur = track_duration_ms.min(incoming_duration_ms);
        if min_dur < self.crossfade_min_track_ms {
            debug!(
                "🔀 [RENDERER] Crossfade SKIPPED: shortest track {}ms < {}ms minimum",
                min_dur, self.crossfade_min_track_ms,
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
        // M8: each armed transition's trailing-silence sustain starts fresh.
        self.trailing_silence_ticks = 0;
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

        // Belt-and-braces vs the transport machine (M5): a resume fade-in can
        // still be mid-ramp when the EOF-fallback trigger fires near track
        // end. During `Active` the crossfade tick owns the streams' fades and
        // `set_volume`'s Active branch owns the volume pushes — cancel the
        // transport ramp and restore full user volume so two writers never
        // fight over the primary's `volume` atomic.
        if !self.transport_fade_idle() {
            self.cancel_transport_fade();
            if let Some(ref stream) = self.primary_stream {
                stream.set_volume(self.stream_volume());
            }
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
            self.stream_volume(), // correct user volume from the start
            0.0,                  // incoming fades in via its fade_coeff, from true silence
            cf_norm,
            Some(self.eq_state.clone()),
            self.consumed_notify.clone(),
            false,
            self.smooth_track_starts,
            self.bit_perfect_active(),
        );
        // Belt-and-braces: the constructor already seeded both the atomic and
        // the smoother from `initial_fade = 0.0`, so they cannot disagree.
        cf_stream.set_fade_coeff(0.0);

        self.crossfade_state = CrossfadeState::Active {
            stream: cf_stream,
            started_at: std::time::Instant::now(),
            duration_ms,
            incoming_format: incoming_format.clone(),
            paused_accum: std::time::Duration::ZERO,
            paused_at: None,
            // Capture the curve for the whole fade — the tick reads this,
            // never the live setting, so a mid-fade change can't tear it.
            curve: self.crossfade_curve,
        };
        // Fresh fade, fresh watchdog (M9): un-latch the stall signal and
        // drop any stale liveness handle — the engine installs this fade's
        // own handle right after it spawns the incoming decode loop
        // (belt-and-braces; cancel/finalize already cleared both).
        self.stall_recovery_signalled = false;
        self.incoming_liveness = None;
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

        // Reset the restored outgoing's fade multiplier UNCONDITIONALLY —
        // outside the `!self.paused` guard below. The resume path `start()`
        // restores only `volume`, never `fade_coeff`, so a cancel that runs
        // while paused (reset_next_track / mode toggle mid-crossfade) would
        // otherwise leave the outgoing stuck at reduced gain with nothing to
        // reset it until the next track change.
        //
        // Restore the visualizer feed too (M9 / invariant 11): a cancel past
        // the fade midpoint lands after `tick_crossfade` handed the feed to
        // the (now discarded) incoming — without this the restored outgoing
        // plays with a frozen spectrum until the next track change. Mirrors
        // `finalize_crossfade`'s reaffirmation on the promoted primary.
        if let Some(ref stream) = self.primary_stream {
            stream.set_fade_coeff(1.0);
            stream.set_feeds_visualizer(true);
        }

        // The fade this watchdog state described is gone: unlatch the stall
        // signal and drop the per-fade liveness handle so the NEXT fade
        // starts with a clean watchdog (M9).
        self.stall_recovery_signalled = false;
        self.incoming_liveness = None;

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

    /// Install (or clear) the M9 liveness handle for the current fade's
    /// incoming decode loop. The engine calls this with a fresh per-fade
    /// instance right after spawning `start_crossfade_decode_loop`;
    /// `start_crossfade` / `cancel_crossfade` / `finalize_crossfade` clear it
    /// with the fade it described.
    pub(crate) fn set_incoming_liveness(&mut self, liveness: Option<Arc<IncomingLiveness>>) {
        self.incoming_liveness = liveness;
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
                curve,
                ..
            } => {
                // Include any in-progress pause span (paused_at) so a tick that
                // somehow runs mid-pause still reports pause-corrected progress.
                let live_paused = paused_at.map_or(*paused_accum, |t| *paused_accum + t.elapsed());
                let elapsed_ms = started_at.elapsed().as_millis() as u64;
                let progress =
                    crossfade_progress(elapsed_ms, live_paused.as_millis() as u64, *duration_ms);
                // Gains from the curve CAPTURED at `start_crossfade` — never
                // the live `self.crossfade_curve` — so a mid-fade settings
                // change can't tear the in-flight envelope. See `fade_curve`
                // for the per-curve contracts (Equal Power / Constant Gain /
                // Linear).
                let (fade_out, fade_in) = crate::audio::fade_curve::fade_gains(*curve, progress);
                (fade_out, fade_in, progress)
            }
            _ => return CrossfadeTick::Continue,
        };

        // The fade is independent of user volume: write the raw curve
        // coefficients to the streams' `fade_coeff` atomic (applied linearly
        // in the source — never re-curved through the perceptual taper) and
        // leave the user volume on `volume`. Mid-fade volume changes are
        // pushed by `set_volume`'s Active branch.
        if let Some(ref stream) = self.primary_stream {
            stream.set_fade_coeff(fade_out as f32);
        }
        if let CrossfadeState::Active { stream, .. } = &self.crossfade_state {
            stream.set_fade_coeff(fade_in as f32);
        }

        // Hand off the visualizer feed at the fade midpoint (progress 0.5 —
        // where every curve's gains cross, so the handoff stays
        // curve-independent). Before 50% the outgoing dominates audio and
        // drives viz; after 50% the incoming dominates and takes over. Without this swap the outgoing
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

        // Fade reached completion. Gate the promotion on the incoming
        // producer being healthy, on two discriminators:
        // - Empty ring: a stalled/failed incoming decoder that never produced
        //   audio (or whose few KB already drained) — promoting it would fade
        //   the audible outgoing into silence with no recovery.
        // - Read liveness (M9 Part B): a producer blocked inside ONE
        //   decode/network read past the stall threshold, even though the
        //   ring still holds residue. Promoting it plays the residue and
        //   then hangs — the ring may never reach the rebuffer prime target,
        //   and EOF never fires, so nothing can ever finish the track. The
        //   flag comes from the decode loop (`IncomingLiveness`), NOT from a
        //   buffer-count heuristic — counts grow and shrink for healthy
        //   reasons; sleeping on backpressure and EOF both read as live.
        // Either way, report the stall and let the caller restore the
        // outgoing + skip.
        let producer_stalled = self
            .incoming_liveness
            .as_ref()
            .is_some_and(|liveness| liveness.is_stalled());
        if self.crossfade_buffer_count() == 0 || producer_stalled {
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
            curve: _,
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

        // Set new primary to full user volume, reset its fade multiplier to
        // unity (it was built with `initial_fade = 0.0` and the tick drove it
        // toward 1; nothing else re-establishes it), and promote it to
        // visualizer feeder (it was created viz-gated to avoid the two-stream
        // rate thrash; now that it's the only stream alive, it should drive
        // the spectrum).
        if let Some(ref stream) = self.primary_stream {
            stream.set_volume(self.stream_volume());
            stream.set_fade_coeff(1.0);
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
        // The completed fade's stall watchdog is done with (M9): the promoted
        // primary is fed by the PRIMARY decode loop from here on, which the
        // fade's liveness handle never described.
        self.stall_recovery_signalled = false;
        self.incoming_liveness = None;
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
            0.0,
            None,
            Arc::new(Notify::new()),
            false,
            Arc::new(std::sync::atomic::AtomicBool::new(true)),
            true,
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
            curve: self.crossfade_curve,
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

    /// The effective duration captured into the Armed variant, for engine
    /// tests pinning that the arm read the M8 bar-snap override.
    #[cfg(test)]
    pub(crate) fn armed_duration_ms_for_test(&self) -> Option<u64> {
        match &self.crossfade_state {
            CrossfadeState::Armed { duration_ms, .. } => Some(*duration_ms),
            _ => None,
        }
    }

    /// The M8 negative-offset lead mirror, for engine setter-push tests.
    #[cfg(test)]
    pub(crate) fn crossfade_lead_ms_for_test(&self) -> u64 {
        self.crossfade_lead_ms
    }

    /// The M8 skip-silence mirror, for engine setter-push tests.
    #[cfg(test)]
    pub(crate) fn skip_silence_for_test(&self) -> bool {
        self.skip_silence
    }

    /// Whether an M9 incoming-liveness handle is installed, for the engine's
    /// `start_crossfade_decode_loop` wiring test.
    #[cfg(test)]
    pub(crate) fn has_incoming_liveness_for_test(&self) -> bool {
        self.incoming_liveness.is_some()
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
    // Transport fades (M5) — pause / resume / stop gain ramps
    // =========================================================================

    /// Push the pause/resume fade config from settings. The renderer owns the
    /// enforcing copy (both consumers — `begin_pause_fade` and the resume
    /// fade-in in `start()` — live here); the stop pair stays on the engine's
    /// `FadeCoordinator`. Bare write: transport fades never flip crossfade
    /// eligibility, so no `reset_next_track` contract applies.
    pub(crate) fn set_pause_fade(&mut self, enabled: bool, duration_ms: u64) {
        self.fade_on_pause = enabled;
        self.fade_pause_ms = duration_ms;
    }

    /// Push the "Smooth Track Starts" setting (M2's de-click onset ramp gate,
    /// default on). Bare write — takes effect on the next stream creation
    /// (play / seek / crossfade incoming), like the normalization settings.
    pub(crate) fn set_smooth_track_starts(&mut self, enabled: bool) {
        if self.smooth_track_starts != enabled {
            tracing::info!("🎚️ Renderer: smooth track starts {}", enabled);
        }
        self.smooth_track_starts = enabled;
    }

    /// Whether a transport fade can ramp the current primary stream at all:
    /// - `CrossfadeState` must be `Idle` — a transport fade never fights a
    ///   blend (the crossfade tick owns the streams during `Active`).
    /// - The primary must NOT be bit-perfect (invariant 8): its `next()` arm
    ///   reads only `fade_coeff`, so the `volume` ramp would be inaudible —
    ///   a delay ending in the same hard cut. Bit-perfect keeps its honest
    ///   instant transport edges.
    /// - The ring must hold audio: a drained ring (natural end-of-queue
    ///   stop, mid-rebuffer) has nothing audible to fade, so the ramp would
    ///   only delay teardown for silence.
    fn transport_fade_engageable(&self) -> bool {
        matches!(self.crossfade_state, CrossfadeState::Idle)
            && !self.current_stream_bit_perfect
            && self.primary_stream.is_some()
            && self.buffer_count() > 0
    }

    /// Whether the M6 first-audio switch fade-in can arm on the fresh
    /// primary. Same gates as [`Self::transport_fade_engageable`] MINUS the
    /// ring check — the whole point of the first-audio trigger is that the
    /// ring may still be empty (radio prebuffer / decoder fill latency). The
    /// bit-perfect refusal is the M6 "skip the fade on a bit-perfect queue
    /// edge" conservative default (the `volume` ramp would be inaudible
    /// there anyway; ramping `fade_coeff` instead would break bit-identity).
    fn switch_fade_engageable(&self) -> bool {
        matches!(self.crossfade_state, CrossfadeState::Idle)
            && !self.current_stream_bit_perfect
            && self.primary_stream.is_some()
    }

    /// One-shot M6 request: the next fresh primary-stream build arms the
    /// first-audio switch fade-in. Set by the engine AFTER a radio-switch
    /// teardown (so the `stop()` inside that teardown cannot clear it);
    /// consumed by `maybe_arm_pending_switch_fade` (init + fresh start),
    /// cleared by `stop()`.
    pub(crate) fn request_switch_fade_in(&mut self) {
        self.pending_switch_fade_in = true;
    }

    /// Whether the in-flight transport fade is an M6 switch fade-in still
    /// HOLDING for real consumption past its baseline. Such a fade survives
    /// pause/start cycles (the radio jitter prebuffer dance) — see
    /// [`TransportFade::SwitchFadeIn`].
    fn transport_fade_awaiting_first_audio(&self) -> bool {
        matches!(
            self.transport_fade,
            TransportFade::SwitchFadeIn {
                started_at: None,
                ..
            }
        )
    }

    /// The primary stream's real-samples-consumed counter (0 without a
    /// stream) — the M6 switch fade's baseline/flip source.
    fn primary_samples_consumed(&self) -> u64 {
        self.primary_stream
            .as_ref()
            .map_or(0, |s| s.handle.samples_consumed.load(Ordering::Relaxed))
    }

    /// Arm the M6 switch fade-in: hold the fresh primary silent; the ramp
    /// clock starts only when the mixer pulls real samples past the baseline
    /// (the hold + flip live in `tick_transport_fade`).
    fn begin_switch_fade_in(&mut self) {
        if let Some(ref stream) = self.primary_stream {
            stream.set_volume(0.0);
        }
        self.transport_fade_gen = self.transport_fade_gen.wrapping_add(1);
        self.transport_fade = TransportFade::SwitchFadeIn {
            started_at: None,
            baseline_samples: self.primary_samples_consumed(),
            generation: self.transport_fade_gen,
        };
        debug!(
            "🎚️ [TRANSPORT FADE] switch fade-in armed — awaiting first audio \
             ({RADIO_SWITCH_FADE_MS}ms ramp)"
        );
    }

    /// Consume the one-shot switch fade-in request and arm it when the fresh
    /// primary can take it (never bit-perfect, never mid-crossfade). Called
    /// from `init()` — BEFORE `play()`'s prebuffer can feed the mixer, so
    /// not a single full-gain sample escapes ahead of the hold — and from
    /// `start()`'s fresh-start path as the unit-tested belt-and-braces (in
    /// production the init-time arm has already consumed the request by
    /// then). The request is consumed even when the arm is refused, so a
    /// refused request can't linger into an unrelated later play.
    fn maybe_arm_pending_switch_fade(&mut self) {
        if !std::mem::take(&mut self.pending_switch_fade_in) {
            return;
        }
        if self.switch_fade_engageable() {
            self.begin_switch_fade_in();
        }
    }

    /// Test observability: whether the one-shot switch fade-in request is
    /// pending (set by the engine, not yet consumed by `start()`).
    #[cfg(test)]
    pub(crate) fn switch_fade_in_pending(&self) -> bool {
        self.pending_switch_fade_in
    }

    /// Test observability: whether an armed switch fade-in is still holding
    /// for its stream's first real sample.
    #[cfg(test)]
    pub(crate) fn transport_fade_is_awaiting_first_audio(&self) -> bool {
        self.transport_fade_awaiting_first_audio()
    }

    /// Arm a fade-out ramp: bump the generation, seed `from` from the
    /// stream's last-written `volume` atomic (so an interrupted in-ramp
    /// continues from the audible level), and stamp the state.
    fn begin_fade_out(&mut self, target: TransportFadeTarget, duration_ms: u64) {
        let from = self
            .primary_stream
            .as_ref()
            .map_or(1.0, |s| crate::audio::load_f32(&s.handle.volume));
        self.transport_fade_gen = self.transport_fade_gen.wrapping_add(1);
        self.transport_fade = TransportFade::FadingOut {
            target,
            started_at: std::time::Instant::now(),
            duration_ms: duration_ms.max(1),
            generation: self.transport_fade_gen,
            from,
            settling: false,
        };
        debug!(
            "🎚️ [TRANSPORT FADE] out-ramp begun: target={target:?}, {duration_ms}ms, from={from:.3}"
        );
    }

    /// Arm a fade-in ramp (resume). Seeds `from` from the last-written
    /// `volume` atomic — 0.0 after a completed pause fade, mid-level when
    /// interrupting an out-ramp, `stream_volume()` (no-op ramp) after an
    /// instant pause.
    fn begin_fade_in(&mut self, duration_ms: u64) {
        let from = self
            .primary_stream
            .as_ref()
            .map_or(0.0, |s| crate::audio::load_f32(&s.handle.volume));
        self.transport_fade_gen = self.transport_fade_gen.wrapping_add(1);
        self.transport_fade = TransportFade::FadingIn {
            started_at: std::time::Instant::now(),
            duration_ms: duration_ms.max(1),
            generation: self.transport_fade_gen,
            from,
        };
        debug!("🎚️ [TRANSPORT FADE] in-ramp begun: {duration_ms}ms, from={from:.3}");
    }

    /// Begin the pause fade-out (guard-lift). Returns whether the ramp
    /// engaged; `false` means the caller must fall back to the instant
    /// [`Self::pause`]. Gated on the renderer-owned "Fade on Pause / Resume"
    /// mirror; the duration comes from the same mirror.
    pub(crate) fn begin_pause_fade(&mut self) -> bool {
        if !self.fade_on_pause || !self.playing || self.paused || !self.transport_fade_engageable()
        {
            return false;
        }
        // Guard-lift: freeze the completion gate / rebuffer / armed trigger
        // NOW (they all sit below render_tick's paused early-return), while
        // the stream keeps producing audio for the ramp. Mirrors pause()'s
        // bookkeeping except the stream-level flip, which the ramp completion
        // applies.
        self.paused = true;
        self.rebuffering = false;
        self.begin_fade_out(TransportFadeTarget::Pause, self.fade_pause_ms);
        true
    }

    /// Begin the stop fade-out (guard-lift). Returns whether the ramp
    /// engaged; on `false` the engine proceeds straight to teardown. The
    /// duration is passed in because the stop pair lives on the engine's
    /// `FadeCoordinator` (its sole consumer is `engine.stop()`, which also
    /// bounds its wait with the same value).
    pub(crate) fn begin_stop_fade(&mut self, duration_ms: u64) -> bool {
        if !self.playing || self.paused || !self.transport_fade_engageable() {
            return false;
        }
        // Same guard-lift as the pause ramp: a stop near end-of-track must
        // not let the completion gate advance the queue mid-ramp.
        self.paused = true;
        self.rebuffering = false;
        self.begin_fade_out(TransportFadeTarget::Stop, duration_ms);
        true
    }

    /// Whether no transport fade is in flight. The engine's `stop()` polls
    /// this to bound its wait for the out-ramp.
    pub(crate) fn transport_fade_idle(&self) -> bool {
        matches!(self.transport_fade, TransportFade::Idle)
    }

    /// Abandon any in-flight transport fade without applying its end-state
    /// action. Bumps the generation so an already-matched completion in the
    /// same tick can't apply either. Called by every lifecycle transition
    /// that invalidates the ramp's stream (stop / seek / fresh init /
    /// crossfade start / instant pause).
    fn cancel_transport_fade(&mut self) {
        self.transport_fade_gen = self.transport_fade_gen.wrapping_add(1);
        self.transport_fade = TransportFade::Idle;
    }

    /// Drive the in-flight transport fade one tick: write the ramped volume
    /// to the primary stream and apply the end-state action on completion.
    /// An out-ramp completes one tick AFTER reaching its floor (the settle
    /// tick — see `TransportFade::FadingOut::settling`), so the source-side
    /// EMA realizes silence before the paused/stopped atomic flips.
    /// Called from `render_tick` ABOVE the playing/paused early-return (the
    /// guard-lift sets `self.paused` at fade START, so the ramp must be
    /// ticked from a point that gate cannot freeze).
    ///
    /// The ramp writes the `volume` atomic linearly; the source's perceptual
    /// taper then realizes it as a perceptually-even fade on the software
    /// path (and a plain amplitude ramp under `pw_volume_active`, where the
    /// atomic sits at 1.0 and PipeWire owns the user volume on top).
    fn tick_transport_fade(&mut self) {
        /// One tick's verdict on the in-flight ramp.
        enum RampTick {
            /// Still ramping — volume written, nothing else to do.
            Running,
            /// Out-ramp floor reached THIS tick — hold one settle tick.
            EnterSettle,
            /// Apply the end-state action.
            Complete,
        }

        // M6: a HOLDING switch fade-in ignores wall clock entirely — it
        // keeps the stream silent until the mixer pulls real samples PAST
        // its baseline (`samples_consumed` counts only real pulls, never
        // paused or ring-starvation silence), then stamps the ramp clock
        // from that moment. Holding on wall clock instead would let the
        // ramp finish during the radio prebuffer silence and pop to full
        // gain when audio finally arrives.
        if let TransportFade::SwitchFadeIn {
            started_at: started_at @ None,
            baseline_samples,
            ..
        } = &mut self.transport_fade
        {
            let consumed = self
                .primary_stream
                .as_ref()
                .map_or(0, |s| s.handle.samples_consumed.load(Ordering::Relaxed));
            if consumed < *baseline_samples {
                // The counter was reset (new-track bookkeeping) — without a
                // re-baseline the fade would hold at silence forever.
                *baseline_samples = consumed;
            }
            if consumed <= *baseline_samples {
                // Still holding. Re-assert the silent hold: on the
                // software-volume path a concurrent user volume write can
                // land on the stream atomic between ticks; the worst-case
                // exposure is one tick (≤ 20 ms), softened further by the
                // M2 onset EMA.
                if let Some(ref stream) = self.primary_stream {
                    stream.set_volume(0.0);
                }
                return;
            }
            *started_at = Some(std::time::Instant::now());
            debug!("🎚️ [TRANSPORT FADE] first audio pulled — switch fade-in ramp started");
            // Fall through to the normal ramp tick below (progress ≈ 0).
        }

        let verdict = match &self.transport_fade {
            TransportFade::Idle => return,
            TransportFade::FadingOut {
                started_at,
                duration_ms,
                from,
                settling,
                ..
            } => {
                let progress =
                    crossfade_progress(started_at.elapsed().as_millis() as u64, 0, *duration_ms);
                if let Some(ref stream) = self.primary_stream {
                    stream.set_volume(*from * (1.0 - progress as f32));
                }
                if progress < 1.0 {
                    RampTick::Running
                } else if *settling {
                    RampTick::Complete
                } else {
                    RampTick::EnterSettle
                }
            }
            TransportFade::FadingIn {
                started_at,
                duration_ms,
                from,
                ..
            } => {
                let progress =
                    crossfade_progress(started_at.elapsed().as_millis() as u64, 0, *duration_ms);
                let target = self.stream_volume();
                if let Some(ref stream) = self.primary_stream {
                    stream.set_volume(*from + (target - *from) * progress as f32);
                }
                // No settle phase: the fade-in end action only pins the
                // target volume — no paused/stopped atomic flips, no cut.
                if progress >= 1.0 {
                    RampTick::Complete
                } else {
                    RampTick::Running
                }
            }
            TransportFade::SwitchFadeIn {
                started_at: Some(started_at),
                ..
            } => {
                // M6 running phase: ramp 0 → target from the first-audio
                // stamp. Same no-settle rule as `FadingIn`.
                let progress = crossfade_progress(
                    started_at.elapsed().as_millis() as u64,
                    0,
                    RADIO_SWITCH_FADE_MS,
                );
                let target = self.stream_volume();
                if let Some(ref stream) = self.primary_stream {
                    stream.set_volume(target * progress as f32);
                }
                if progress >= 1.0 {
                    RampTick::Complete
                } else {
                    RampTick::Running
                }
            }
            // Unreachable: the holding pre-step above always returns (or
            // stamps `Some` and falls through to the arm above).
            TransportFade::SwitchFadeIn {
                started_at: None, ..
            } => return,
        };
        match verdict {
            RampTick::Running => return,
            RampTick::EnterSettle => {
                // The volume atomic now reads 0.0, but the realized gain is
                // the source's ~5 ms-tau EMA, which lags it. Hold the end
                // action one render tick (20 ms ≈ 4 EMA time constants at
                // silence) so the audible level settles to zero BEFORE the
                // paused/stopped atomic flips — otherwise short ramps end in
                // the very hard cut they exist to remove (see the `settling`
                // field doc).
                if let TransportFade::FadingOut { settling, .. } = &mut self.transport_fade {
                    *settling = true;
                }
                debug!("🎚️ [TRANSPORT FADE] out-ramp floor reached — settling one tick");
                return;
            }
            RampTick::Complete => {}
        }

        // Completion: take the state, apply the end action only when the
        // stamped generation is still live — a superseded ramp (interrupt /
        // cancel raced with this tick) must not apply its stale end-state.
        let state = std::mem::replace(&mut self.transport_fade, TransportFade::Idle);
        let (target, generation) = match state {
            TransportFade::FadingOut {
                target, generation, ..
            } => (Some(target), generation),
            TransportFade::FadingIn { generation, .. }
            | TransportFade::SwitchFadeIn { generation, .. } => (None, generation),
            TransportFade::Idle => return, // unreachable — matched above
        };
        if generation != self.transport_fade_gen {
            debug!("🎚️ [TRANSPORT FADE] stale completion discarded (superseded ramp)");
            return;
        }

        match target {
            Some(TransportFadeTarget::Pause) => {
                // The REAL pause the guard-lift deferred: silence is already
                // reached, now freeze the stream (position stops counting).
                if let Some(ref stream) = self.primary_stream {
                    stream.set_volume(0.0);
                    stream.pause();
                }
                debug!("🎚️ [TRANSPORT FADE] pause ramp complete — stream paused");
            }
            Some(TransportFadeTarget::Stop) => {
                // The REAL silence_and_stop the engine's teardown expects.
                if let Some(stream) = self.primary_stream.take() {
                    stream.silence_and_stop();
                }
                debug!("🎚️ [TRANSPORT FADE] stop ramp complete — stream stopped");
            }
            None => {
                // Fade-in settled — pin the exact target so EMA drift or a
                // coarse final tick can't leave it fractionally short.
                if let Some(ref stream) = self.primary_stream {
                    stream.set_volume(self.stream_volume());
                }
                debug!("🎚️ [TRANSPORT FADE] resume ramp complete");
            }
        }
        #[cfg(test)]
        {
            self.transport_fade_completions += 1;
        }
    }

    /// Test observability: whether a fade-out ramp is in flight.
    #[cfg(test)]
    pub(crate) fn transport_fade_is_fading_out(&self) -> bool {
        matches!(self.transport_fade, TransportFade::FadingOut { .. })
    }

    /// Test-only: force the native-PipeWire volume flag so the bit-perfect
    /// gates (`crossfade_blocked`, bit-perfect stream builds) engage in
    /// engine-level tests without a real PipeWire sink.
    #[cfg(test)]
    pub(crate) fn force_pw_volume_active_for_test(&mut self) {
        self.pw_volume_active = true;
    }

    /// Test observability: count of REAL ramp completions (see field doc).
    #[cfg(test)]
    pub(crate) fn transport_fade_completions(&self) -> u64 {
        self.transport_fade_completions
    }

    /// Force a detached primary stream for tests WITHOUT a real audio output
    /// (mirrors [`Self::force_crossfade_active_for_test`]). Pre-fills the
    /// ring with `prefill` samples (the transport-fade engageable gate skips
    /// a drained ring), marks the renderer playing, and returns the throwaway
    /// source (keep it alive — it owns the handle's shared atomics) plus a
    /// clone of the stream handle for atomic-level assertions.
    #[cfg(test)]
    pub(crate) fn force_primary_stream_for_test(
        &mut self,
        prefill: usize,
    ) -> (
        crate::audio::streaming_source::StreamingSource,
        crate::audio::streaming_source::StreamHandle,
    ) {
        use std::num::NonZero;

        use ringbuf::{HeapRb, traits::Split};
        let rb = HeapRb::<f32>::new(crate::audio::RING_BUFFER_CAPACITY);
        let (mut producer, consumer) = rb.split();
        if prefill > 0 {
            use ringbuf::traits::Producer;
            let data: Vec<f32> = (0..prefill).map(|i| i as f32 * 0.001).collect();
            producer.push_slice(&data);
        }
        let viz: SharedVisualizerCallback = Arc::new(parking_lot::RwLock::new(None));
        let (source, handle) = crate::audio::streaming_source::StreamingSource::new(
            consumer,
            NonZero::new(2).expect("2 is nonzero"),
            NonZero::new(48_000).expect("48000 is nonzero"),
            viz,
            1.0,
            1.0,
            None,
            Arc::new(Notify::new()),
            true,
            Arc::new(std::sync::atomic::AtomicBool::new(true)),
            true,
            false,
        );
        self.primary_stream = Some(crate::audio::ActiveStream {
            producer,
            handle: handle.clone(),
            sample_rate: 48_000,
            channels: 2,
        });
        self.playing = true;
        self.paused = false;
        (source, handle)
    }

    // =========================================================================
    // Render loop (called periodically from engine render thread)
    // =========================================================================

    /// M8 trailing-silence detector, ticked from `render_tick`'s Armed branch
    /// (20 ms cadence, playing and un-paused by construction). Returns `true`
    /// when the early trigger should fire: the "Skip Silence Between Tracks"
    /// setting is on, the position is inside the trailing window
    /// (`track_dur − fade − TRAILING_SILENCE_WINDOW_MS` onward — a mid-song
    /// quiet passage can never fire), and the primary's level meter has read
    /// below [`crate::audio::SOURCE_SILENCE_THRESHOLD`] for
    /// [`TRAILING_SILENCE_SUSTAIN_TICKS`] consecutive ticks.
    ///
    /// The meter is enabled LAZILY on the first in-window tick, so streams
    /// carry zero metering cost until a fade is armed near a track end with
    /// the feature on. Until the first metered window completes the handle
    /// reports its LOUD seed, which simply delays the count by one window
    /// (~11 ms). Ring starvation never updates the meter (see
    /// `StreamingSource`), so a network stall holds the last REAL reading
    /// instead of faking silence.
    ///
    /// Stands down under bit-perfect (Strict AND Relaxed — mode intent, not
    /// the live `pw_volume_active` viability), mirroring the leading-trim
    /// gate in the engine's `transition_prep_cfg`: discarding decoded tail
    /// samples is a content change a bit-perfect listener opted out of, and
    /// Relaxed self-arms same-format crossfades, so this branch IS reachable
    /// there. The two halves of the one setting must agree.
    fn tick_trailing_silence(&mut self, pos: u64, track_dur: u64, xfade_dur: u64) -> bool {
        if !self.skip_silence || self.bit_perfect_mode.builds_bit_perfect() {
            return false;
        }
        let window_start = track_dur.saturating_sub(xfade_dur + TRAILING_SILENCE_WINDOW_MS);
        if pos < window_start {
            self.trailing_silence_ticks = 0;
            return false;
        }
        let Some(ref stream) = self.primary_stream else {
            self.trailing_silence_ticks = 0;
            return false;
        };
        stream.handle.enable_level_meter();
        if stream.handle.recent_source_peak() < crate::audio::SOURCE_SILENCE_THRESHOLD {
            self.trailing_silence_ticks += 1;
        } else {
            self.trailing_silence_ticks = 0;
        }
        if self.trailing_silence_ticks >= TRAILING_SILENCE_SUSTAIN_TICKS {
            debug!(
                "🤫 [RENDER_TICK] Trailing silence sustained {}ms at pos={}ms — early crossfade trigger",
                u64::from(TRAILING_SILENCE_SUSTAIN_TICKS) * 20,
                pos
            );
            return true;
        }
        false
    }

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

        // Transport fades tick ABOVE the playing/paused early-return: the
        // guard-lift design flips `self.paused = true` at fade START (so the
        // completion gate, the rebuffer path, and the armed crossfade trigger
        // below are frozen for the whole ramp), which means the early-return
        // would starve the ramp if it were ticked any later.
        self.tick_transport_fade();

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
                    // Fade completed on wall clock but the incoming producer
                    // stalled (empty ring, or blocked on the socket past the
                    // liveness threshold). Do NOT promote the stalled stream.
                    // Signal the engine to cancel the crossfade (restoring the
                    // outgoing as primary at full volume) and skip the bad
                    // track via the normal end-of-track path. The state stays
                    // Active until that recovery lands, so this arm re-runs
                    // every 20 ms tick — the signal itself (and its warn) is
                    // latched to fire once per fade.
                    self.on_renderer_crossfade_stalled();
                    return; // Don't run further checks this tick
                }
            }
        }

        // Check for crossfade trigger: position-based, NOT EOF-based.
        // We start the crossfade `duration_ms` before the track ends, so the
        // outgoing track still has audio in its buffer to fade out. Falls back
        // to EOF if duration is unknown (0). (The params are copied out of the
        // Armed variant so the M8 helpers below can borrow `self` mutably;
        // the synchronous `mem::replace` trigger itself is unchanged —
        // invariant 2.)
        let armed_params = if let CrossfadeState::Armed {
            duration_ms,
            track_duration_ms,
            ..
        } = &self.crossfade_state
        {
            Some((*duration_ms, *track_duration_ms))
        } else {
            None
        };
        if let Some((xfade_dur, track_dur)) = armed_params {
            let pos = self.position();
            let trigger = if track_dur > 0 && xfade_dur > 0 {
                // M8 negative "Gap / Overlap Trim": the lead moves the trigger
                // point earlier; the outgoing's trailing `lead` ms is discarded
                // at finalize (invariant 5's finalize-before-EOF path). The
                // trailing-silence detector may fire even earlier, but only
                // inside its bounded window on sustained sub-threshold content.
                pos >= track_dur.saturating_sub(xfade_dur + self.crossfade_lead_ms)
                    || self.tick_trailing_silence(pos, track_dur, xfade_dur)
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
        // M9 "already-signalled" latch: the stalled state stays Active (and
        // keeps reporting `IncomingStalled`) until the ONE spawned recovery
        // task wins the engine lock and cancels it — without the latch every
        // 20 ms tick would respawn `recover_stalled_crossfade`. Cleared with
        // the fade at `start_crossfade` / `cancel_crossfade` /
        // `finalize_crossfade`, so recovery can never be starved: every path
        // out of the latched state runs one of those.
        if self.stall_recovery_signalled {
            return;
        }
        self.stall_recovery_signalled = true;
        #[cfg(test)]
        {
            self.stall_signals_sent += 1;
        }

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
            curve: CrossfadeCurve::default(),
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
            0.0,
            None,
            Arc::new(Notify::new()),
            false,
            Arc::new(std::sync::atomic::AtomicBool::new(true)),
            true,
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
            curve: CrossfadeCurve::default(),
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

    /// M8 negative offset: with a crossfade lead pushed, the Armed position
    /// trigger fires `lead` ms EARLIER than `track_dur − fade`. The baseline
    /// tick (no lead) at the same position must leave Armed untouched — the
    /// consumed Armed state is the observable for "the trigger fired".
    #[tokio::test]
    async fn armed_trigger_fires_early_with_crossfade_lead() {
        let mut renderer = AudioRenderer::new();
        let (_src, _handle) = renderer.force_primary_stream_for_test(4_096);
        renderer.crossfade_state = CrossfadeState::Armed {
            duration_ms: 2_000,
            incoming_format: AudioFormat::invalid(),
            track_duration_ms: 100_000,
        };
        // Normal trigger point is 98_000; the lead moves it to 96_000.
        renderer.reset_position_with_offset(96_500);

        renderer.render_tick();
        assert!(
            renderer.is_crossfade_armed(),
            "without a lead, 96.5s of 100s (fade 2s) must NOT trigger yet"
        );

        renderer.set_crossfade_lead_ms(2_000);
        renderer.render_tick();
        assert!(
            !renderer.is_crossfade_armed(),
            "a 2s lead moves the trigger to 96s — the tick at 96.5s must fire (consume Armed)"
        );
    }

    /// M8 trailing-silence trim: inside the trailing window with the setting
    /// on, a sustained sub-threshold meter reading fires the trigger early —
    /// and the first windowed tick lazily enables the stream's level meter.
    #[tokio::test]
    async fn trailing_silence_fires_early_trigger_after_sustain() {
        let mut renderer = AudioRenderer::new();
        renderer.set_skip_silence(true);
        let (_src, handle) = renderer.force_primary_stream_for_test(4_096);
        renderer.crossfade_state = CrossfadeState::Armed {
            duration_ms: 2_000,
            incoming_format: AudioFormat::invalid(),
            track_duration_ms: 100_000,
        };
        // Window opens at 100_000 − 2_000 − 15_000 = 83_000; normal trigger
        // at 98_000. 90s is inside the window, well before the trigger.
        renderer.reset_position_with_offset(90_000);
        // Fake a silent meter reading (the meter itself is unit-tested in
        // streaming_source; here we pin the renderer's sustain logic).
        handle
            .recent_source_peak
            .store(1e-4_f32.to_bits(), Ordering::Relaxed);

        renderer.render_tick();
        assert!(
            handle
                .level_meter_enabled
                .load(std::sync::atomic::Ordering::Relaxed),
            "the first in-window tick must lazily enable the primary's level meter"
        );

        for _ in 0..(TRAILING_SILENCE_SUSTAIN_TICKS - 2) {
            renderer.render_tick();
        }
        assert!(
            renderer.is_crossfade_armed(),
            "one tick short of the sustain threshold must not fire"
        );
        renderer.render_tick();
        assert!(
            !renderer.is_crossfade_armed(),
            "{TRAILING_SILENCE_SUSTAIN_TICKS} sustained silent ticks must fire the early trigger"
        );
    }

    /// M8 trailing-silence trim: a silent reading OUTSIDE the trailing window
    /// (mid-song quiet passage) must never fire, no matter how long.
    #[tokio::test]
    async fn trailing_silence_outside_window_never_fires() {
        let mut renderer = AudioRenderer::new();
        renderer.set_skip_silence(true);
        let (_src, handle) = renderer.force_primary_stream_for_test(4_096);
        renderer.crossfade_state = CrossfadeState::Armed {
            duration_ms: 2_000,
            incoming_format: AudioFormat::invalid(),
            track_duration_ms: 100_000,
        };
        renderer.reset_position_with_offset(50_000); // far from the window
        handle
            .recent_source_peak
            .store(1e-4_f32.to_bits(), Ordering::Relaxed);

        for _ in 0..(2 * TRAILING_SILENCE_SUSTAIN_TICKS) {
            renderer.render_tick();
        }
        assert!(
            renderer.is_crossfade_armed(),
            "a mid-song quiet passage must never fire the trailing-silence trigger"
        );
    }

    /// M8 trailing-silence trim is gated on the setting (default off).
    #[tokio::test]
    async fn trailing_silence_requires_skip_silence_setting() {
        let mut renderer = AudioRenderer::new();
        let (_src, handle) = renderer.force_primary_stream_for_test(4_096);
        renderer.crossfade_state = CrossfadeState::Armed {
            duration_ms: 2_000,
            incoming_format: AudioFormat::invalid(),
            track_duration_ms: 100_000,
        };
        renderer.reset_position_with_offset(90_000);
        handle
            .recent_source_peak
            .store(1e-4_f32.to_bits(), Ordering::Relaxed);

        for _ in 0..(2 * TRAILING_SILENCE_SUSTAIN_TICKS) {
            renderer.render_tick();
        }
        assert!(
            renderer.is_crossfade_armed(),
            "with Skip Silence OFF the trailing trigger must never fire"
        );
    }

    /// M8 trailing-silence trim stands down under bit-perfect — mode intent
    /// (`builds_bit_perfect()`, Strict AND Relaxed), mirroring the engine's
    /// leading-trim gate in `transition_prep_cfg` so the two halves of the
    /// same setting can never disagree. Dropping decoded samples is a content
    /// change a bit-perfect listener opted out of. Relaxed is the live case:
    /// it self-arms same-format crossfades, so without this gate a silent
    /// outgoing tail would fire the blend early and discard the remainder.
    #[tokio::test]
    async fn trailing_silence_stands_down_under_bit_perfect_relaxed() {
        let mut renderer = AudioRenderer::new();
        renderer.set_skip_silence(true);
        renderer.set_bit_perfect(BitPerfectMode::Relaxed);
        let (_src, handle) = renderer.force_primary_stream_for_test(4_096);
        renderer.crossfade_state = CrossfadeState::Armed {
            duration_ms: 2_000,
            incoming_format: AudioFormat::invalid(),
            track_duration_ms: 100_000,
        };
        renderer.reset_position_with_offset(90_000);
        handle
            .recent_source_peak
            .store(1e-4_f32.to_bits(), Ordering::Relaxed);

        for _ in 0..(2 * TRAILING_SILENCE_SUSTAIN_TICKS) {
            renderer.render_tick();
        }
        assert!(
            renderer.is_crossfade_armed(),
            "bit-perfect Relaxed must never fire the trailing-silence trigger — \
             trimming decoded tail samples violates the bit-perfect contract"
        );
        assert!(
            !handle
                .level_meter_enabled
                .load(std::sync::atomic::Ordering::Relaxed),
            "the stand-down must return before lazily enabling the level meter"
        );
    }

    /// M8 trailing-silence trim: one loud meter window resets the sustain
    /// count — the silence must be CONSECUTIVE.
    #[tokio::test]
    async fn trailing_silence_loud_reading_resets_sustain() {
        let mut renderer = AudioRenderer::new();
        renderer.set_skip_silence(true);
        let (_src, handle) = renderer.force_primary_stream_for_test(4_096);
        renderer.crossfade_state = CrossfadeState::Armed {
            duration_ms: 2_000,
            incoming_format: AudioFormat::invalid(),
            track_duration_ms: 100_000,
        };
        renderer.reset_position_with_offset(90_000);

        // Almost-sustained silence…
        handle
            .recent_source_peak
            .store(1e-4_f32.to_bits(), Ordering::Relaxed);
        for _ in 0..(TRAILING_SILENCE_SUSTAIN_TICKS - 1) {
            renderer.render_tick();
        }
        // …interrupted by one loud window…
        handle
            .recent_source_peak
            .store(0.5_f32.to_bits(), Ordering::Relaxed);
        renderer.render_tick();
        // …then silence again: the count must restart from zero.
        handle
            .recent_source_peak
            .store(1e-4_f32.to_bits(), Ordering::Relaxed);
        for _ in 0..(TRAILING_SILENCE_SUSTAIN_TICKS - 1) {
            renderer.render_tick();
        }
        assert!(
            renderer.is_crossfade_armed(),
            "a loud window mid-run must reset the sustain counter"
        );
        renderer.render_tick();
        assert!(
            !renderer.is_crossfade_armed(),
            "a fresh full sustain run after the reset fires normally"
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

    /// M1: `tick_crossfade` must write the raw curve coefficients to the
    /// streams' `fade_coeff` atomic (applied linearly in the source) and leave
    /// the `volume` atomic alone — user volume and fade are no longer
    /// overloaded onto one value.
    #[tokio::test]
    async fn tick_crossfade_writes_fade_coeff_and_leaves_volume_alone() {
        let mut renderer = AudioRenderer::new();
        renderer.playing = true;
        let (primary, _psrc) = test_active_stream(4_096);
        let primary_handle = primary.handle.clone();
        primary_handle.set_volume(0.77);
        renderer.primary_stream = Some(primary);
        let (incoming, _isrc) = test_active_stream(4_096);
        let incoming_handle = incoming.handle.clone();
        // Exactly mid-fade (progress ≈ 0.5): the long duration makes the
        // wall-clock skew between `Instant::now()` here and the tick negligible
        // (1 ms of skew = 1e-5 progress). ConstantGain is the cos²/sin² pair
        // this test's 0.5-midpoint assertions are written against.
        renderer.crossfade_state = CrossfadeState::Active {
            stream: incoming,
            started_at: std::time::Instant::now() - std::time::Duration::from_secs(50),
            duration_ms: 100_000,
            incoming_format: AudioFormat::invalid(),
            paused_accum: std::time::Duration::ZERO,
            paused_at: None,
            curve: CrossfadeCurve::ConstantGain,
        };

        assert_eq!(renderer.tick_crossfade(), CrossfadeTick::Continue);

        // cos²/sin² midpoint = 0.5 each.
        let out = crate::audio::load_f32(&primary_handle.fade_coeff);
        let inc = crate::audio::load_f32(&incoming_handle.fade_coeff);
        assert!(
            (out - 0.5).abs() < 0.01,
            "outgoing fade_coeff must carry cos²(p·π/2) ≈ 0.5, got {out}"
        );
        assert!(
            (inc - 0.5).abs() < 0.01,
            "incoming fade_coeff must carry sin²(p·π/2) ≈ 0.5, got {inc}"
        );
        assert!(
            (crate::audio::load_f32(&primary_handle.volume) - 0.77).abs() < 1e-6,
            "the tick must not fold the fade into the volume atomic any more"
        );
    }

    /// M1: `finalize_crossfade` must reset the promoted primary's `fade_coeff`
    /// to 1.0 — the incoming stream was built with `initial_fade = 0.0`, and
    /// nothing else ever re-establishes the fade on a promoted stream.
    #[tokio::test]
    async fn finalize_crossfade_resets_promoted_fade_coeff() {
        let mut renderer = AudioRenderer::new();
        let (incoming, _isrc) = test_active_stream(4_096);
        let incoming_handle = incoming.handle.clone();
        renderer.crossfade_state = completed_active_state(incoming);

        renderer.finalize_crossfade();

        assert_eq!(
            crate::audio::load_f32(&incoming_handle.fade_coeff),
            1.0,
            "finalize must reset the promoted primary's fade_coeff to unity"
        );
    }

    /// M1: `cancel_crossfade` (the seek-during-crossfade path) must reset the
    /// restored outgoing primary's `fade_coeff` to 1.0 — the tick had been
    /// driving it toward 0.
    #[tokio::test]
    async fn cancel_crossfade_resets_outgoing_fade_coeff() {
        let mut renderer = AudioRenderer::new();
        let (primary, _psrc) = test_active_stream(0);
        let primary_handle = primary.handle.clone();
        primary_handle.set_fade_coeff(0.3); // mid-fade attenuation
        renderer.primary_stream = Some(primary);
        let (incoming, _isrc) = test_active_stream(0);
        renderer.crossfade_state = completed_active_state(incoming);

        renderer.cancel_crossfade();

        assert_eq!(
            crate::audio::load_f32(&primary_handle.fade_coeff),
            1.0,
            "cancel must reset the restored outgoing's fade_coeff to unity"
        );
    }

    /// M1 blocker guard: the `fade_coeff` reset in `cancel_crossfade` must be
    /// UNCONDITIONAL — outside the `!self.paused` guard that gates the volume
    /// restore. The resume path `start()` restores only `volume` and never
    /// re-establishes `fade_coeff`, so a `reset_next_track` / mode toggle that
    /// cancels a crossfade WHILE PAUSED would otherwise leave the outgoing
    /// stuck at reduced gain until the next track change.
    #[tokio::test]
    async fn cancel_crossfade_resets_outgoing_fade_coeff_while_paused() {
        let mut renderer = AudioRenderer::new();
        renderer.paused = true;
        let (primary, _psrc) = test_active_stream(0);
        let primary_handle = primary.handle.clone();
        primary_handle.set_fade_coeff(0.3);
        renderer.primary_stream = Some(primary);
        let (incoming, _isrc) = test_active_stream(0);
        renderer.crossfade_state = completed_active_state(incoming);

        renderer.cancel_crossfade();

        assert_eq!(
            crate::audio::load_f32(&primary_handle.fade_coeff),
            1.0,
            "the fade_coeff reset must run even while paused"
        );
    }

    /// M1: after the volume/fade split the crossfade tick writes only
    /// `fade_coeff`, so a mid-crossfade user-volume change must be pushed to
    /// BOTH streams' `volume` atomics by `set_volume` itself — unconditionally.
    /// Paused is the harder case: the tick doesn't run while paused and
    /// `start()` skips the volume restore when Active, so this push is the
    /// only way the change ever reaches the streams.
    #[tokio::test]
    async fn set_volume_during_active_crossfade_pushes_to_both_streams() {
        let mut renderer = AudioRenderer::new();
        let (primary, _psrc) = test_active_stream(0);
        let primary_handle = primary.handle.clone();
        renderer.primary_stream = Some(primary);
        let (incoming, _isrc) = test_active_stream(0);
        let incoming_handle = incoming.handle.clone();
        renderer.crossfade_state = completed_active_state(incoming);
        renderer.paused = true;

        renderer.set_volume(0.42);

        assert!(
            (crate::audio::load_f32(&primary_handle.volume) - 0.42).abs() < 1e-6,
            "set_volume during Active must push the user volume to the primary"
        );
        assert!(
            (crate::audio::load_f32(&incoming_handle.volume) - 0.42).abs() < 1e-6,
            "set_volume during Active must push the user volume to the incoming stream"
        );
    }

    /// M3: an `Active` crossfade carrying `curve: EqualPower` must realize the
    /// true equal-power gains — 1/√2 ≈ 0.7071 each at the midpoint (power sum
    /// 1), NOT the cos²/sin² 0.5 that would dip ~3 dB for uncorrelated
    /// material.
    #[tokio::test]
    async fn tick_crossfade_equal_power_midpoint_holds_power() {
        let mut renderer = AudioRenderer::new();
        renderer.playing = true;
        let (primary, _psrc) = test_active_stream(4_096);
        let primary_handle = primary.handle.clone();
        renderer.primary_stream = Some(primary);
        let (incoming, _isrc) = test_active_stream(4_096);
        let incoming_handle = incoming.handle.clone();
        renderer.crossfade_state = CrossfadeState::Active {
            stream: incoming,
            started_at: std::time::Instant::now() - std::time::Duration::from_secs(50),
            duration_ms: 100_000,
            incoming_format: AudioFormat::invalid(),
            paused_accum: std::time::Duration::ZERO,
            paused_at: None,
            curve: CrossfadeCurve::EqualPower,
        };

        assert_eq!(renderer.tick_crossfade(), CrossfadeTick::Continue);

        let expected = std::f64::consts::FRAC_1_SQRT_2 as f32;
        let out = crate::audio::load_f32(&primary_handle.fade_coeff);
        let inc = crate::audio::load_f32(&incoming_handle.fade_coeff);
        assert!(
            (out - expected).abs() < 0.01,
            "EqualPower outgoing midpoint gain must be ≈ 0.707, got {out}"
        );
        assert!(
            (inc - expected).abs() < 0.01,
            "EqualPower incoming midpoint gain must be ≈ 0.707, got {inc}"
        );
        let power = out * out + inc * inc;
        assert!(
            (power - 1.0).abs() < 0.03,
            "EqualPower midpoint power sum must be ≈ 1, got {power}"
        );
    }

    /// M3 no-tear guard: the tick must apply the curve CAPTURED in the
    /// `Active` variant, not the live `self.crossfade_curve` setting — a
    /// mid-fade settings change must leave the in-flight envelope on the
    /// curve it started with.
    #[tokio::test]
    async fn tick_crossfade_uses_captured_curve_not_live_setting() {
        let mut renderer = AudioRenderer::new();
        renderer.playing = true;
        let (primary, _psrc) = test_active_stream(4_096);
        let primary_handle = primary.handle.clone();
        renderer.primary_stream = Some(primary);
        let (incoming, _isrc) = test_active_stream(4_096);
        renderer.crossfade_state = CrossfadeState::Active {
            stream: incoming,
            started_at: std::time::Instant::now() - std::time::Duration::from_secs(50),
            duration_ms: 100_000,
            incoming_format: AudioFormat::invalid(),
            paused_accum: std::time::Duration::ZERO,
            paused_at: None,
            curve: CrossfadeCurve::EqualPower,
        };

        // Mid-fade settings change: must NOT reach the in-flight fade.
        renderer.set_crossfade_curve(CrossfadeCurve::ConstantGain);
        assert_eq!(renderer.tick_crossfade(), CrossfadeTick::Continue);

        let out = crate::audio::load_f32(&primary_handle.fade_coeff);
        let expected = std::f64::consts::FRAC_1_SQRT_2 as f32;
        assert!(
            (out - expected).abs() < 0.01,
            "mid-fade curve change must not tear: expected the captured \
             EqualPower midpoint ≈ 0.707, got {out} (ConstantGain would be 0.5)"
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

    /// M4: the arm gate's minimum-track floor follows the configured setting,
    /// not the historical hardcoded 10 s — a raised floor (30 s) must refuse a
    /// 20 s pair that the default floor accepts.
    #[tokio::test]
    async fn arm_crossfade_honors_configured_min_track_floor() {
        use crate::audio::format::SampleFormat;
        let f44 = AudioFormat::new(SampleFormat::S16, 44_100, 2);
        let mut renderer = AudioRenderer::new();

        renderer.set_crossfade_min_track_secs(30);
        renderer.arm_crossfade(5_000, &f44, 20_000, 20_000);
        assert!(
            !renderer.is_crossfade_armed(),
            "a 20s pair must NOT arm under a 30s configured floor"
        );

        renderer.set_crossfade_min_track_secs(10);
        renderer.arm_crossfade(5_000, &f44, 20_000, 20_000);
        assert!(
            renderer.is_crossfade_armed(),
            "the same 20s pair must arm once the floor drops back to 10s"
        );
    }

    /// M4: a floor of 0 means "blend everything with a KNOWN duration" —
    /// short tracks arm, but an unknown (zero) duration must still refuse:
    /// a zero `track_duration_ms` would make the Armed position trigger
    /// (`pos >= track_duration − fade`) fire immediately at track start.
    /// Unreachable under the historical 10s constant, exposed by the
    /// configurable 0 floor.
    #[tokio::test]
    async fn arm_crossfade_zero_floor_blends_short_tracks_but_skips_unknown_duration() {
        use crate::audio::format::SampleFormat;
        let f44 = AudioFormat::new(SampleFormat::S16, 44_100, 2);
        let mut renderer = AudioRenderer::new();
        renderer.set_crossfade_min_track_secs(0);

        renderer.arm_crossfade(5_000, &f44, 0, 20_000);
        assert!(
            !renderer.is_crossfade_armed(),
            "an unknown (0) outgoing duration must never arm — the position \
             trigger would fire immediately"
        );
        renderer.arm_crossfade(5_000, &f44, 20_000, 0);
        assert!(
            !renderer.is_crossfade_armed(),
            "an unknown (0) incoming duration must never arm"
        );

        renderer.arm_crossfade(5_000, &f44, 5_000, 5_000);
        assert!(
            renderer.is_crossfade_armed(),
            "a 5s pair must arm under a 0 floor (blend everything known)"
        );
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

    /// M9 Part B: a completed fade whose incoming ring still holds residue
    /// (the old empty-only gate would promote) but whose producer has been
    /// blocked inside ONE network read past the stall threshold must report
    /// `IncomingStalled` — promoting it yields a stream that plays its
    /// residue and then hangs silent with no completion path. The tick only
    /// REPORTS; teardown happens later in `cancel_crossfade` (driven by the
    /// engine's `recover_stalled_crossfade`).
    #[tokio::test]
    async fn tick_crossfade_reports_stall_when_incoming_liveness_dead() {
        let (stream, _source) = test_active_stream(4_096);
        let mut renderer = AudioRenderer::new();
        renderer.playing = true;
        renderer.crossfade_state = completed_active_state(stream);
        renderer.set_incoming_liveness(Some(Arc::new(
            crate::audio::IncomingLiveness::new_blocked_for_test(
                crate::audio::crossfade_liveness::CROSSFADE_STALL_READ_MS + 500,
            ),
        )));

        assert!(
            renderer.crossfade_buffer_count() > 0,
            "residue in the ring — the empty-only gate alone would promote"
        );
        assert_eq!(
            renderer.tick_crossfade(),
            CrossfadeTick::IncomingStalled,
            "a producer blocked on the socket past the threshold must not be promoted"
        );
        assert!(
            matches!(renderer.crossfade_state, CrossfadeState::Active { .. }),
            "the tick only reports the stall; the state stays Active for the recovery"
        );
    }

    /// M9 Part B control: an in-flight read BELOW the stall threshold is a
    /// healthy fetch mid-flight (slow networks routinely hold a read for
    /// hundreds of ms) — the completed fade must promote exactly as before.
    #[tokio::test]
    async fn tick_crossfade_finalizes_when_incoming_liveness_alive() {
        let (stream, _source) = test_active_stream(4_096);
        let mut renderer = AudioRenderer::new();
        renderer.playing = true;
        renderer.crossfade_state = completed_active_state(stream);
        let liveness = Arc::new(crate::audio::IncomingLiveness::new());
        liveness.mark_read_start();
        renderer.set_incoming_liveness(Some(liveness));

        assert_eq!(
            renderer.tick_crossfade(),
            CrossfadeTick::Finalize,
            "an in-flight read below the stall threshold is healthy — promote as before"
        );
    }

    /// M9 Part A: a late-stage cancel (seek / mode toggle / stall recovery
    /// after the midpoint handoff) restores the outgoing as the SOLE live
    /// stream — it must feed the visualizer again, or the spectrum freezes
    /// until the next track change.
    #[tokio::test]
    async fn cancel_crossfade_restores_visualizer_feed_on_outgoing() {
        let mut renderer = AudioRenderer::new();
        let (_primary_src, primary_handle) = renderer.force_primary_stream_for_test(4_096);
        let (stream, _incoming_src) = test_active_stream(4_096);
        renderer.crossfade_state = completed_active_state(stream);

        // Drive the REAL midpoint handoff (progress >= 0.5): the tick mutes
        // the outgoing primary's feed and promotes the incoming's.
        renderer.tick_crossfade();
        assert!(
            !primary_handle.feeds_visualizer(),
            "precondition: the midpoint handoff muted the outgoing's viz feed"
        );

        renderer.cancel_crossfade();

        assert!(
            primary_handle.feeds_visualizer(),
            "cancel_crossfade must restore the visualizer feed on the restored outgoing"
        );
    }

    /// M9 Part A: once a stalled fade signals recovery, the 20 ms render tick
    /// must NOT respawn `recover_stalled_crossfade` every tick while the one
    /// spawned task waits on the engine lock — the signal latches.
    #[tokio::test]
    async fn stalled_fade_signals_recovery_once_not_every_tick() {
        let (stream, _source) = test_active_stream(0);
        let mut renderer = AudioRenderer::new();
        renderer.playing = true;
        renderer.crossfade_state = completed_active_state(stream);

        renderer.render_tick();
        renderer.render_tick();
        renderer.render_tick();

        assert!(
            matches!(renderer.crossfade_state, CrossfadeState::Active { .. }),
            "the stalled fade stays Active until the engine's recovery lands"
        );
        assert_eq!(
            renderer.stall_signals_sent, 1,
            "recovery must be signalled exactly once per stalled fade, not per tick"
        );
        assert!(renderer.stall_recovery_signalled);
    }

    /// M9 Part A: `cancel_crossfade` (the renderer half of every recovery /
    /// teardown path, including `stop()`) resets the stall latch and drops
    /// the fade's liveness handle so the NEXT fade starts with a clean
    /// watchdog.
    #[tokio::test]
    async fn cancel_crossfade_clears_stall_watchdog_state() {
        let (stream, _source) = test_active_stream(0);
        let mut renderer = AudioRenderer::new();
        renderer.playing = true;
        renderer.crossfade_state = completed_active_state(stream);
        renderer.set_incoming_liveness(Some(Arc::new(crate::audio::IncomingLiveness::new())));
        renderer.render_tick(); // stalls + latches
        assert!(
            renderer.stall_recovery_signalled,
            "precondition: the stalled tick latched the signal"
        );

        renderer.cancel_crossfade();

        assert!(
            !renderer.stall_recovery_signalled,
            "the latch must reset with the fade it guards"
        );
        assert!(
            renderer.incoming_liveness.is_none(),
            "the dead fade's liveness handle must not describe a later fade"
        );
    }

    /// M9 Part A: `finalize_crossfade` (healthy promotion) clears the same
    /// watchdog state — the promoted primary is no longer the stream the
    /// fade's liveness handle described.
    #[tokio::test]
    async fn finalize_crossfade_clears_stall_watchdog_state() {
        let (stream, _source) = test_active_stream(4_096);
        let mut renderer = AudioRenderer::new();
        renderer.playing = true;
        renderer.crossfade_state = completed_active_state(stream);
        renderer.set_incoming_liveness(Some(Arc::new(crate::audio::IncomingLiveness::new())));
        renderer.stall_recovery_signalled = true;

        renderer.finalize_crossfade();

        assert!(renderer.incoming_liveness.is_none());
        assert!(!renderer.stall_recovery_signalled);
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
            curve: CrossfadeCurve::default(),
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
            curve: CrossfadeCurve::default(),
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
            curve: CrossfadeCurve::default(),
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

    // ═════════════════════════════════════════════════════════════════════
    //  M5 — transport fades (pause / resume / stop gain ramps)
    // ═════════════════════════════════════════════════════════════════════

    /// Rewind the in-flight transport fade's `started_at` by `ms` so a test
    /// can place the ramp at an exact wall-clock progress without sleeping.
    fn rewind_transport_fade(renderer: &mut AudioRenderer, ms: u64) {
        match &mut renderer.transport_fade {
            TransportFade::FadingOut { started_at, .. }
            | TransportFade::FadingIn { started_at, .. } => {
                *started_at = std::time::Instant::now() - std::time::Duration::from_millis(ms);
            }
            TransportFade::SwitchFadeIn {
                started_at: Some(started_at),
                ..
            } => {
                *started_at = std::time::Instant::now() - std::time::Duration::from_millis(ms);
            }
            TransportFade::SwitchFadeIn {
                started_at: None, ..
            } => panic!("switch fade still holding — no ramp clock to rewind"),
            TransportFade::Idle => panic!("no transport fade in flight to rewind"),
        }
    }

    /// M7 interlock (M5 review-cycle-2 survivor): `init()`'s GAPLESS-REUSE
    /// branch is the one transport-fade cancel site that reuses the existing
    /// stream — cancelling an in-flight resume fade-in there without
    /// restoring the `volume` atomic strands the next track at the mid-ramp
    /// level (under `pw_volume_active` nothing downstream rewrites it:
    /// `set_volume` early-returns and `start()` early-returns while already
    /// playing). The cancel must restore `stream_volume()` on the reused
    /// primary.
    #[tokio::test]
    async fn init_gapless_reuse_restores_volume_after_interrupted_transport_fade() {
        use crate::audio::format::SampleFormat;
        let mut renderer = AudioRenderer::new();
        renderer.set_pause_fade(true, 200);
        let (_src, handle) = renderer.force_primary_stream_for_test(4_096);
        let fmt = AudioFormat::new(SampleFormat::S16, 48_000, 2);
        renderer.format = fmt.clone();

        // A resume fade-in mid-ramp: paused at some point, volume atomic at
        // its mid-ramp value, start() re-arms the in-ramp from it.
        handle.set_volume(0.25);
        renderer.playing = false;
        renderer.paused = true;
        renderer.start();
        assert!(
            matches!(renderer.transport_fade, TransportFade::FadingIn { .. }),
            "precondition: a resume fade-in is in flight"
        );

        // Gapless-reuse init (same format, no force_reload) — the EOF-fallback
        // `load_prepared_track` path for a format-matched next track.
        renderer
            .init(&fmt, false, None)
            .expect("gapless-reuse init must succeed");

        assert!(
            renderer.transport_fade_idle(),
            "init must abandon the previous track's ramp"
        );
        let vol = crate::audio::load_f32(&handle.volume);
        assert!(
            (vol - renderer.stream_volume()).abs() < f32::EPSILON,
            "the reused stream's volume must be restored (got {vol}, want {})",
            renderer.stream_volume()
        );
    }

    /// M5 guard-lift: `begin_pause_fade` flips `self.paused` IMMEDIATELY
    /// (freezing the completion gate / rebuffer / armed trigger) while the
    /// stream-level pause is deferred to ramp completion — the audio keeps
    /// flowing for the ramp.
    #[tokio::test]
    async fn begin_pause_fade_lifts_paused_guard_and_defers_stream_pause() {
        let mut renderer = AudioRenderer::new();
        renderer.set_pause_fade(true, 100);
        let (_src, handle) = renderer.force_primary_stream_for_test(4_096);
        handle.set_volume(1.0);

        assert!(renderer.begin_pause_fade(), "the pause fade must engage");

        assert!(
            renderer.paused,
            "guard-lift: self.paused must flip at fade START"
        );
        assert!(
            !handle.paused.load(Ordering::Acquire),
            "the stream-level pause is deferred until the ramp completes"
        );
        assert!(renderer.transport_fade_is_fading_out());
    }

    /// M5: the pause ramp writes intermediate volumes toward 0 and invokes
    /// the REAL stream-level pause only after reaching completion.
    #[tokio::test]
    async fn pause_fade_ramps_volume_then_pauses_stream() {
        let mut renderer = AudioRenderer::new();
        renderer.set_pause_fade(true, 100);
        let (_src, handle) = renderer.force_primary_stream_for_test(4_096);
        handle.set_volume(1.0);
        assert!(renderer.begin_pause_fade());

        // Mid-ramp (~50%): volume ≈ from · (1 − progress) = 0.5, no pause yet.
        rewind_transport_fade(&mut renderer, 50);
        renderer.tick_transport_fade();
        let mid = crate::audio::load_f32(&handle.volume);
        assert!(
            (mid - 0.5).abs() < 0.1,
            "mid-ramp volume must be ≈ 0.5, got {mid}"
        );
        assert!(
            !handle.paused.load(Ordering::Acquire),
            "the stream must keep playing mid-ramp"
        );

        // Past completion: volume 0, then (after the one-tick EMA settle)
        // stream paused, machine Idle.
        rewind_transport_fade(&mut renderer, 200);
        renderer.tick_transport_fade(); // floor — writes 0.0, enters settle
        renderer.tick_transport_fade(); // settle — applies the real pause
        assert!(
            handle.paused.load(Ordering::Acquire),
            "ramp completion must invoke the real stream-level pause"
        );
        assert!(
            crate::audio::load_f32(&handle.volume) < 1e-6,
            "the completed out-ramp must end at volume 0"
        );
        assert!(renderer.transport_fade_idle());
        assert_eq!(renderer.transport_fade_completions(), 1);
    }

    /// M5 review fix (settle tick): the out-ramp's end action must NOT fire
    /// in the same render tick as the final volume write. The audible gain
    /// is the consumer-side ~5 ms-tau EMA in `StreamingSource::next()`,
    /// which lags the atomic write — and `next()` emits silence the instant
    /// the paused atomic flips, so a same-tick pause cuts at the EMA's
    /// pre-floor gain. At the 20 ms slider minimum (= one 20 ms tick period)
    /// that degenerates to a near/full-amplitude hard cut — the exact click
    /// the fade exists to remove. The floor tick must instead HOLD one
    /// settle tick (20 ms ≈ 4 EMA time constants at volume 0.0) before the
    /// real stream-level pause.
    #[tokio::test]
    async fn pause_fade_floor_holds_one_settle_tick_before_stream_pause() {
        let mut renderer = AudioRenderer::new();
        renderer.set_pause_fade(true, 20); // slider minimum — the worst case
        let (_src, handle) = renderer.force_primary_stream_for_test(4_096);
        handle.set_volume(1.0);
        assert!(renderer.begin_pause_fade());

        // Floor tick: the ramp reaches volume 0.0, but the stream must NOT
        // pause yet — the EMA needs a full tick at silence first.
        rewind_transport_fade(&mut renderer, 20);
        renderer.tick_transport_fade();
        assert!(
            crate::audio::load_f32(&handle.volume) < 1e-6,
            "the floor tick must write volume 0.0"
        );
        assert!(
            !handle.paused.load(Ordering::Acquire),
            "the stream-level pause must be deferred one settle tick past \
             the floor (a same-tick pause cuts at the EMA's lagging gain)"
        );
        assert!(
            !renderer.transport_fade_idle(),
            "the fade must stay in flight through the settle tick"
        );
        assert_eq!(renderer.transport_fade_completions(), 0);

        // Settle tick: NOW the real pause applies.
        renderer.tick_transport_fade();
        assert!(
            handle.paused.load(Ordering::Acquire),
            "the settle tick must invoke the real stream-level pause"
        );
        assert!(renderer.transport_fade_idle());
        assert_eq!(renderer.transport_fade_completions(), 1);
    }

    /// M5 call-site pin: `render_tick` must drive the ramp even though the
    /// guard-lift already set `self.paused = true` — i.e. the transport-fade
    /// tick sits ABOVE the playing/paused early-return. Placing it below
    /// starves the ramp forever (this test then hangs at volume 1.0).
    #[tokio::test]
    async fn render_tick_drives_pause_fade_while_paused() {
        let mut renderer = AudioRenderer::new();
        renderer.set_pause_fade(true, 100);
        let (_src, handle) = renderer.force_primary_stream_for_test(4_096);
        handle.set_volume(1.0);
        assert!(renderer.begin_pause_fade());
        assert!(renderer.paused, "precondition: guard already lifted");

        rewind_transport_fade(&mut renderer, 200);
        renderer.render_tick(); // floor — writes 0.0, enters settle
        renderer.render_tick(); // settle — applies the real pause

        assert!(
            handle.paused.load(Ordering::Acquire),
            "render_tick must complete the ramp despite self.paused being set"
        );
        assert!(renderer.transport_fade_idle());
    }

    /// M5: resuming mid-fade-out (or after a completed one) fades back in,
    /// seeded from the stream's LAST-WRITTEN `volume` atomic — the observable
    /// value the renderer can actually read — so an interrupted ramp
    /// continues from the audible level instead of snapping.
    #[tokio::test]
    async fn resume_fade_in_seeds_from_last_written_volume() {
        let mut renderer = AudioRenderer::new();
        renderer.set_pause_fade(true, 100);
        let (_src, handle) = renderer.force_primary_stream_for_test(4_096);
        handle.set_volume(1.0);
        assert!(renderer.begin_pause_fade());
        rewind_transport_fade(&mut renderer, 50);
        renderer.tick_transport_fade();
        let interrupted_at = crate::audio::load_f32(&handle.volume);
        assert!(
            (interrupted_at - 0.5).abs() < 0.1,
            "precondition: mid-ramp volume ≈ 0.5"
        );

        // Resume: start() is the engine's unpause path.
        renderer.start();

        assert!(!renderer.paused, "start() must clear the paused guard");
        assert!(
            !handle.paused.load(Ordering::Acquire),
            "start() must resume the stream immediately (audio under the in-ramp)"
        );
        match renderer.transport_fade {
            TransportFade::FadingIn { from, .. } => {
                assert!(
                    (from - interrupted_at).abs() < 1e-6,
                    "the in-ramp must seed from the last-written volume atomic \
                     ({interrupted_at}), got {from}"
                );
            }
            _ => panic!("start() with the pause fade enabled must begin a FadingIn ramp"),
        }

        // Completion restores the exact target volume.
        rewind_transport_fade(&mut renderer, 200);
        renderer.tick_transport_fade();
        let restored = crate::audio::load_f32(&handle.volume);
        assert!(
            (restored - 1.0).abs() < 1e-6,
            "the completed in-ramp must end at stream_volume(), got {restored}"
        );
        assert!(renderer.transport_fade_idle());
    }

    /// M5 generation token: a completion whose stamped generation no longer
    /// matches the live counter is stale — it must apply NO end-state action
    /// (no stream pause, no completion count), only clear itself.
    #[tokio::test]
    async fn stale_transport_fade_completion_is_ignored() {
        let mut renderer = AudioRenderer::new();
        let (_src, handle) = renderer.force_primary_stream_for_test(4_096);
        handle.set_volume(1.0);
        renderer.transport_fade_gen = 5;
        renderer.transport_fade = TransportFade::FadingOut {
            target: TransportFadeTarget::Pause,
            started_at: std::time::Instant::now() - std::time::Duration::from_millis(200),
            duration_ms: 100,
            generation: 0, // stale — a newer begin/cancel superseded this ramp
            from: 1.0,
            settling: true, // already settled — this tick reaches completion
        };

        renderer.tick_transport_fade();

        assert!(
            !handle.paused.load(Ordering::Acquire),
            "a stale completion must not pause the stream"
        );
        assert!(
            renderer.transport_fade_idle(),
            "the stale state must still be discarded"
        );
        assert_eq!(renderer.transport_fade_completions(), 0);
    }

    /// M5 / invariant 8: the volume atomic is inert on the bit-perfect arm
    /// (`next()` reads only `fade_coeff` there), so a transport fade on a
    /// bit-perfect stream would be an inaudible delay ending in the same hard
    /// cut. Skip the ramp — the caller falls back to the instant pause.
    #[tokio::test]
    async fn pause_fade_not_engaged_for_bit_perfect_stream() {
        let mut renderer = AudioRenderer::new();
        renderer.set_pause_fade(true, 100);
        let (_src, _handle) = renderer.force_primary_stream_for_test(4_096);
        renderer.current_stream_bit_perfect = true;

        assert!(!renderer.begin_pause_fade());
        assert!(
            !renderer.paused,
            "a refused fade must not lift the guard — the instant pause() does"
        );
        assert!(renderer.transport_fade_idle());
    }

    /// M5 guard: a transport fade never fights a live crossfade — engage only
    /// while `CrossfadeState` is `Idle`.
    #[tokio::test]
    async fn pause_fade_not_engaged_during_active_crossfade() {
        let mut renderer = AudioRenderer::new();
        renderer.set_pause_fade(true, 100);
        let (_src, _handle) = renderer.force_primary_stream_for_test(4_096);
        let (incoming, _isrc) = test_active_stream(0);
        renderer.crossfade_state = completed_active_state(incoming);

        assert!(!renderer.begin_pause_fade());
        assert!(renderer.transport_fade_idle());
    }

    /// M5 default: with "Fade on Pause / Resume" OFF (the shipped default),
    /// the fade never engages — pause stays the instant flip it is today.
    #[tokio::test]
    async fn pause_fade_not_engaged_when_disabled() {
        let mut renderer = AudioRenderer::new();
        let (_src, _handle) = renderer.force_primary_stream_for_test(4_096);

        assert!(!renderer.begin_pause_fade());
        assert!(renderer.transport_fade_idle());
    }

    /// M5: the stop ramp completes into the REAL `silence_and_stop()` — the
    /// primary is taken and removed from the mixer, so the engine's teardown
    /// that follows the bounded wait finds a silent renderer.
    #[tokio::test]
    async fn stop_fade_completion_silences_and_stops_primary() {
        let mut renderer = AudioRenderer::new();
        let (_src, handle) = renderer.force_primary_stream_for_test(4_096);
        handle.set_volume(1.0);

        assert!(renderer.begin_stop_fade(100), "the stop fade must engage");
        assert!(
            renderer.paused,
            "guard-lift applies to the stop ramp too (a stop near end-of-track \
             must not let the completion gate advance the queue mid-ramp)"
        );

        rewind_transport_fade(&mut renderer, 200);
        renderer.tick_transport_fade(); // floor — writes 0.0, enters settle
        renderer.tick_transport_fade(); // settle — applies silence_and_stop

        assert!(
            renderer.primary_stream.is_none(),
            "stop completion must take the primary via silence_and_stop()"
        );
        assert!(
            handle.stopped.load(Ordering::Acquire),
            "the stream must be flagged stopped (removed from the mixer)"
        );
        assert!(renderer.transport_fade_idle());
        assert_eq!(renderer.transport_fade_completions(), 1);
    }

    /// M5 review fix (settle tick), stop flavor: `silence_and_stop()` flips
    /// the stopped atomic, which ends the source instantly — same lagging-EMA
    /// hard cut as the pause flavor when it fires in the floor tick. The
    /// stop ramp must also hold one settle tick at volume 0.0 before the
    /// real `silence_and_stop()`.
    #[tokio::test]
    async fn stop_fade_floor_holds_one_settle_tick_before_silence_and_stop() {
        let mut renderer = AudioRenderer::new();
        let (_src, handle) = renderer.force_primary_stream_for_test(4_096);
        handle.set_volume(1.0);
        assert!(renderer.begin_stop_fade(20)); // slider minimum — worst case

        // Floor tick: volume 0.0, stream still live (not stopped, not taken).
        rewind_transport_fade(&mut renderer, 20);
        renderer.tick_transport_fade();
        assert!(
            crate::audio::load_f32(&handle.volume) < 1e-6,
            "the floor tick must write volume 0.0"
        );
        assert!(
            renderer.primary_stream.is_some(),
            "the primary must survive the floor tick (settle at silence)"
        );
        assert!(
            !handle.stopped.load(Ordering::Acquire),
            "silence_and_stop must be deferred one settle tick past the floor"
        );
        assert_eq!(renderer.transport_fade_completions(), 0);

        // Settle tick: NOW the real silence_and_stop applies.
        renderer.tick_transport_fade();
        assert!(
            renderer.primary_stream.is_none(),
            "the settle tick must take the primary via silence_and_stop()"
        );
        assert!(handle.stopped.load(Ordering::Acquire));
        assert!(renderer.transport_fade_idle());
        assert_eq!(renderer.transport_fade_completions(), 1);
    }

    /// M5: a drained ring has nothing audible to fade (natural end-of-queue
    /// stop, mid-rebuffer) — skip the ramp so teardown isn't delayed for
    /// silence.
    #[tokio::test]
    async fn stop_fade_not_engaged_on_drained_ring() {
        let mut renderer = AudioRenderer::new();
        let (_src, _handle) = renderer.force_primary_stream_for_test(0);

        assert!(!renderer.begin_stop_fade(100));
        assert!(renderer.transport_fade_idle());
    }

    /// M5: `stop()` abandons any in-flight transport fade without applying
    /// its end-state action (the stream is being torn down anyway).
    #[tokio::test]
    async fn renderer_stop_cancels_transport_fade() {
        let mut renderer = AudioRenderer::new();
        renderer.set_pause_fade(true, 100);
        let (_src, _handle) = renderer.force_primary_stream_for_test(4_096);
        assert!(renderer.begin_pause_fade());

        renderer.stop();

        assert!(renderer.transport_fade_idle());
        assert_eq!(
            renderer.transport_fade_completions(),
            0,
            "a cancel is not a completion"
        );
    }

    /// M5 / invariant 8: with the pause fade enabled but a BIT-PERFECT
    /// primary, resume must restore volume instantly (the in-ramp would be
    /// inert on that arm) — same conservative skip as the out-ramp.
    #[tokio::test]
    async fn resume_fade_skipped_for_bit_perfect_stream() {
        let mut renderer = AudioRenderer::new();
        renderer.set_pause_fade(true, 100);
        let (_src, handle) = renderer.force_primary_stream_for_test(4_096);
        renderer.current_stream_bit_perfect = true;
        renderer.pause();

        renderer.start();

        assert!(
            renderer.transport_fade_idle(),
            "no in-ramp may start for a bit-perfect stream"
        );
        assert!(
            (crate::audio::load_f32(&handle.volume) - 1.0).abs() < 1e-6,
            "resume must restore full volume instantly"
        );
    }

    // ═════════════════════════════════════════════════════════════════════
    //  M6 — radio-switch fade (first-audio fade-in)
    // ═════════════════════════════════════════════════════════════════════

    /// M6: a pending switch fade-in request is consumed by the next FRESH
    /// `start()`, which holds the new primary silent — ignoring wall clock —
    /// until the mixer pulls the stream's first REAL sample, and only then
    /// runs the ramp to the target volume. A wall-clock ramp from stream
    /// creation would burn off during the radio prebuffer silence and pop to
    /// full gain when audio finally arrives.
    #[tokio::test]
    async fn switch_fade_in_holds_silence_until_first_audio_then_ramps() {
        let mut renderer = AudioRenderer::new();
        // Empty ring — the radio decoder hasn't produced anything yet.
        let (mut src, handle) = renderer.force_primary_stream_for_test(0);
        renderer.playing = false; // fresh start, not the helper's "already playing"
        renderer.request_switch_fade_in();

        renderer.start();

        assert!(
            !renderer.switch_fade_in_pending(),
            "start() must consume the one-shot request"
        );
        assert!(
            renderer.transport_fade_is_awaiting_first_audio(),
            "the switch fade must arm in the awaiting-first-audio state"
        );
        assert!(
            crate::audio::load_f32(&handle.volume) < 1e-6,
            "the fresh stream must be held silent while awaiting"
        );

        // Starved pulls (ring empty → silence) must NOT count as audio, and
        // the holding phase carries NO ramp clock at all — wall clock spent
        // buffering can never advance it.
        assert_eq!(src.next(), Some(0.0));
        renderer.tick_transport_fade();
        assert!(
            renderer.transport_fade_is_awaiting_first_audio(),
            "silence pulls must not start the ramp"
        );
        assert!(
            crate::audio::load_f32(&handle.volume) < 1e-6,
            "the hold must keep the stream silent through buffering"
        );

        // First REAL sample pulled → the tick starts the ramp from zero.
        renderer.write_samples(&[0.25; 256]);
        for _ in 0..8 {
            let _ = src.next();
        }
        renderer.tick_transport_fade();
        assert!(
            !renderer.transport_fade_is_awaiting_first_audio(),
            "the first real pull must start the ramp"
        );
        assert!(
            !renderer.transport_fade_idle(),
            "the ramp must now be running"
        );
        assert!(
            crate::audio::load_f32(&handle.volume) < 0.2,
            "a just-started ramp must begin near silence (clock re-stamped at first audio)"
        );

        // Completion pins the exact target.
        rewind_transport_fade(&mut renderer, RADIO_SWITCH_FADE_MS + 20);
        renderer.tick_transport_fade();
        assert!(renderer.transport_fade_idle());
        assert_eq!(renderer.transport_fade_completions(), 1);
        assert!(
            (crate::audio::load_f32(&handle.volume) - 1.0).abs() < 1e-6,
            "the completed fade must pin the target volume"
        );
    }

    /// M6: the radio jitter prebuffer repeatedly calls `renderer.pause()`
    /// until ~5 s is buffered, then `renderer.start()`. Both would normally
    /// cancel a transport fade / restore full volume — an awaiting switch
    /// fade-in must SURVIVE that dance (its audio has not flowed yet, and it
    /// has no deferred end action), otherwise the radio onset pops at full
    /// gain. `fade_on_pause` is enabled here deliberately: it is the config
    /// where `start()`'s resume branch is most eager to hijack the fade.
    #[tokio::test]
    async fn switch_fade_survives_radio_jitter_pause_start_dance() {
        let mut renderer = AudioRenderer::new();
        renderer.set_pause_fade(true, 100);
        let (mut src, handle) = renderer.force_primary_stream_for_test(0);
        renderer.playing = false;
        renderer.request_switch_fade_in();
        renderer.start();
        assert!(renderer.transport_fade_is_awaiting_first_audio());

        // Jitter prebuffer: pause until the buffer target is reached…
        renderer.pause();
        assert!(
            renderer.transport_fade_is_awaiting_first_audio(),
            "pause() must NOT cancel an awaiting switch fade"
        );
        renderer.tick_transport_fade();
        assert!(
            crate::audio::load_f32(&handle.volume) < 1e-6,
            "the hold must persist while paused"
        );

        // …buffer filled, decode loop unpauses via start().
        renderer.write_samples(&[0.5; 4_096]);
        renderer.start();
        assert!(
            renderer.transport_fade_is_awaiting_first_audio(),
            "start() must leave the armed switch fade alone"
        );
        assert!(
            crate::audio::load_f32(&handle.volume) < 1e-6,
            "the unpause must not snap the volume up past the hold"
        );

        // Audio finally flows → ramp runs to the target.
        for _ in 0..8 {
            let _ = src.next();
        }
        renderer.tick_transport_fade();
        assert!(!renderer.transport_fade_is_awaiting_first_audio());
        rewind_transport_fade(&mut renderer, RADIO_SWITCH_FADE_MS + 20);
        renderer.tick_transport_fade();
        assert!(renderer.transport_fade_idle());
        assert!(
            (crate::audio::load_f32(&handle.volume) - 1.0).abs() < 1e-6,
            "the fade must complete at the target volume after the dance"
        );
    }

    /// M6 / invariant 8: a bit-perfect fresh stream drops the pending switch
    /// fade-in (the `volume` ramp is inaudible on that arm, and ramping
    /// `fade_coeff` would break bit-identity) — the one-shot is still
    /// consumed and the volume restored instantly, exactly like the M5
    /// bit-perfect skips.
    #[tokio::test]
    async fn switch_fade_dropped_for_bit_perfect_stream() {
        let mut renderer = AudioRenderer::new();
        let (_src, handle) = renderer.force_primary_stream_for_test(4_096);
        renderer.current_stream_bit_perfect = true;
        renderer.playing = false;
        renderer.request_switch_fade_in();

        renderer.start();

        assert!(
            !renderer.switch_fade_in_pending(),
            "the one-shot request must be consumed even when refused"
        );
        assert!(
            renderer.transport_fade_idle(),
            "no fade may arm on a bit-perfect stream"
        );
        assert!(
            (crate::audio::load_f32(&handle.volume) - 1.0).abs() < 1e-6,
            "volume must be restored instantly"
        );
    }

    /// M6: play()'s prebuffer lets the mixer pull a TRICKLE of real samples
    /// (silently, at the volume-0 hold) BEFORE the decode loop's jitter
    /// prebuffer pauses the renderer for ~5 s — flipping the fade to running
    /// too early. `pause()` must RE-ARM the switch fade with a fresh
    /// consumed-samples baseline instead of cancelling it (or letting the
    /// ramp burn off silently during the hold), so the ramp starts at the
    /// TRUE audible onset when the jitter fill unpauses.
    #[tokio::test]
    async fn switch_fade_rearms_on_pause_after_pre_jitter_trickle() {
        let mut renderer = AudioRenderer::new();
        let (mut src, handle) = renderer.force_primary_stream_for_test(0);
        renderer.playing = false;
        renderer.request_switch_fade_in();
        renderer.start();
        assert!(renderer.transport_fade_is_awaiting_first_audio());

        // Pre-jitter trickle: a few real samples pulled at the silent hold.
        renderer.write_samples(&[0.5; 512]);
        for _ in 0..16 {
            let _ = src.next();
        }
        renderer.tick_transport_fade();
        assert!(
            !renderer.transport_fade_is_awaiting_first_audio(),
            "the trickle flips the fade to running (precondition)"
        );

        // Jitter prebuffer hold: pause must RE-ARM, not cancel/burn.
        renderer.pause();
        assert!(
            renderer.transport_fade_is_awaiting_first_audio(),
            "pause() must re-arm the running switch fade to awaiting"
        );
        renderer.tick_transport_fade();
        assert!(
            crate::audio::load_f32(&handle.volume) < 1e-6,
            "the re-armed hold must keep the stream silent through the jitter window"
        );

        // Jitter fill: buffer builds while paused (no pulls), then unpause.
        renderer.write_samples(&[0.5; 4_096]);
        renderer.start();
        assert!(
            renderer.transport_fade_is_awaiting_first_audio(),
            "start() must leave the re-armed switch fade holding"
        );

        // TRUE onset: real playback begins → ramp restarts from silence.
        for _ in 0..16 {
            let _ = src.next();
        }
        renderer.tick_transport_fade();
        assert!(
            !renderer.transport_fade_is_awaiting_first_audio(),
            "consumption past the re-baselined count must start the ramp"
        );
        rewind_transport_fade(&mut renderer, RADIO_SWITCH_FADE_MS + 20);
        renderer.tick_transport_fade();
        assert!(renderer.transport_fade_idle());
        assert!(
            (crate::audio::load_f32(&handle.volume) - 1.0).abs() < 1e-6,
            "the fade must complete at the target after the re-armed onset"
        );
    }

    /// M6: `stop()` clears an unconsumed switch fade-in request — a torn-down
    /// switch must never leak a soft start into an unrelated later play.
    #[tokio::test]
    async fn renderer_stop_clears_pending_switch_fade() {
        let mut renderer = AudioRenderer::new();
        renderer.request_switch_fade_in();

        renderer.stop();

        assert!(
            !renderer.switch_fade_in_pending(),
            "stop() must clear the pending switch fade-in"
        );
    }
}
