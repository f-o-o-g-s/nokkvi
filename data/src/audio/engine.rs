use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
};

use anyhow::Result;
use parking_lot::Mutex as PlMutex;
use tokio::sync::Notify;
use tracing::{debug, error, trace, warn};

use crate::{
    audio::{
        AudioDecoder, AudioFormat, AudioRenderer, DecodeLoopHandle, SourceGeneration,
        format::samples_for_duration,
    },
    utils::url_redaction::redact_subsonic_url,
};

/// Reinterpret the decoder's interleaved f32 (native-endian) PCM bytes as f32
/// samples. The decoder emits full-precision f32 via `RawSampleBuffer::<f32>`,
/// so this is a lossless reinterpret — no quantization. It replaces the former
/// S16 path that truncated 24-bit / float sources to 16-bit before widening,
/// which made hi-res content impossible to play back bit-perfectly.
///
/// `from_ne_bytes` over `chunks_exact(4)` is alignment-free (a `Vec<u8>` is not
/// guaranteed f32-aligned) and matches Symphonia's native-endian raw output;
/// `output_data` is always a whole number of f32 frames so no bytes are dropped.
fn decoded_bytes_to_f32(bytes: &[u8]) -> Vec<f32> {
    // `chunks_exact` silently drops a trailing 1–3 byte remainder. Every caller
    // feeds whole f32 frames today (see the module doc), so this never fires;
    // the assert pins that invariant so a future decode/silence path that ever
    // leaks a partial sample trips loudly in debug/tests instead of silently
    // shifting the channel interleave for the rest of the buffer.
    debug_assert_eq!(
        bytes.len() % std::mem::size_of::<f32>(),
        0,
        "decoded byte count must be a whole number of f32 samples"
    );
    bytes
        .chunks_exact(std::mem::size_of::<f32>())
        .map(|b| f32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
        .collect()
}

/// Playback state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

/// Crossfade transition phase.
///
/// Variants carry the crossfade decoder + incoming source URL — these
/// fields used to live as parallel `Arc<Mutex<Option<AudioDecoder>>>` /
/// `String` on `CustomAudioEngine`, where every transition reset them
/// in lockstep with the phase flag. Now the data lives WITH the phase
/// so transitions are one `mem::replace` and impossible states are
/// unrepresentable (e.g. `Idle` carries no decoder; `Active` has it).
pub enum CrossfadePhase {
    /// Normal single-track playback.
    Idle,
    /// Two decoders active, blending audio in renderer.
    Active {
        decoder: Arc<tokio::sync::Mutex<Option<AudioDecoder>>>,
        incoming_source: String,
    },
    /// Outgoing decoder finished, incoming still draining.
    OutgoingFinished {
        decoder: Arc<tokio::sync::Mutex<Option<AudioDecoder>>>,
        incoming_source: String,
    },
}

impl CrossfadePhase {
    pub(crate) fn is_idle(&self) -> bool {
        matches!(self, CrossfadePhase::Idle)
    }

    /// Short label for diagnostic logs (the variants' inner `Mutex`
    /// makes a derived `Debug` impl impractical).
    fn label(&self) -> &'static str {
        match self {
            CrossfadePhase::Idle => "idle",
            CrossfadePhase::Active { .. } => "active",
            CrossfadePhase::OutgoingFinished { .. } => "outgoing_finished",
        }
    }
}

/// Outcome of a manual-skip crossfade attempt
/// ([`CustomAudioEngine::crossfade_to_next`], M7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipFadeOutcome {
    /// The blend started — the incoming is fading in; finalize promotes it.
    Fired,
    /// A gate refused (format / durations / min-track floor / not audibly
    /// playing a finite stream). The caller should fall back to the boundary
    /// fade + hard load so the skip still lands.
    Blocked,
    /// A competing user action superseded this skip while its decoder was
    /// building (source generation moved). The caller must do NOTHING — the
    /// competing action owns the engine state.
    Stale,
}

/// Effective duration for a manual-skip crossfade, or `None` when the
/// duration gates refuse (M7). Pure — the direct-`Active` fire bypasses
/// `arm_crossfade`, so this re-applies its duration gates (known durations,
/// the configured minimum-track floor, the `shorter/2` clamp) plus the
/// skip-specific remaining-audio clamp: the position trigger fires exactly
/// `fade` before the end, but a manual skip can land anywhere — a fade
/// longer than the outgoing's remaining audio would EOF mid-blend, drain the
/// ring, and cut to silence.
fn skip_fade_duration_ms(
    requested_ms: u64,
    outgoing_duration_ms: u64,
    incoming_duration_ms: u64,
    position_ms: u64,
    min_track_ms: u64,
) -> Option<u64> {
    // Unknown durations can't blend: a zero incoming degenerates the
    // `shorter/2` clamp to 0 and a zero outgoing has no known remainder.
    if outgoing_duration_ms == 0 || incoming_duration_ms == 0 {
        return None;
    }
    // The configured minimum-track floor applies to skips too — silently
    // ignoring it here would make `crossfade_min_track_secs` a lie on
    // every manual skip.
    let min_dur = outgoing_duration_ms.min(incoming_duration_ms);
    if min_dur < min_track_ms {
        return None;
    }
    let remaining = outgoing_duration_ms.saturating_sub(position_ms);
    let effective = requested_ms.min(min_dur / 2).min(remaining);
    (effective > 0).then_some(effective)
}

/// Owns the engine-side crossfade cluster: the live phase + the crossfade
/// config fields that gate arming/triggering. Bundling them keeps the
/// eligibility predicate (`crossfade_eligible`) and the renderer↔engine
/// liveness reconciliation (`is_crossfade_live`) next to the state they read,
/// instead of as free functions reaching across `CustomAudioEngine`'s fields.
///
/// `bit_perfect_mode` is the engine-side mirror of the renderer's bit-perfect
/// mode — its SOLE engine reader is `crossfade_eligible` (Relaxed self-arms a
/// same-rate crossfade even with the Crossfade toggle off), so it lives here
/// with the predicate that consumes it. `set_bit_perfect` keeps it in sync.
///
/// This struct does NOT own the renderer-side `CrossfadeState` machine — the
/// engine `CrossfadePhase` and renderer `CrossfadeState` are deliberately split
/// and load-bearing (the renderer goes Active strictly BEFORE the engine phase;
/// `is_crossfade_live` reconciles that exact window). It also does NOT own the
/// lock-free `crossfade_duration_shared` atomic, which is a decode-loop channel
/// living in `DecodeLoopChannels`.
pub(crate) struct CrossfadeCoordinator {
    /// Current crossfade phase + per-phase data (decoder + incoming source
    /// live inside `Active` / `OutgoingFinished` variants).
    phase: CrossfadePhase,
    /// Whether crossfade is enabled (from settings).
    enabled: bool,
    /// Crossfade duration in milliseconds (from settings).
    duration_ms: u64,
    /// Engine-side mirror of the bit-perfect mode (the renderer owns the
    /// canonical copy for the DSP path; this copy lets the arm gate decide
    /// whether to self-arm a crossfade under Relaxed without taking the renderer
    /// lock). Kept in sync by `set_bit_perfect`.
    bit_perfect_mode: crate::types::player_settings::BitPerfectMode,
    /// Engine-side mirror of the minimum-track-length floor, in seconds (the
    /// renderer owns the enforcing copy at its `arm_crossfade` gate). This
    /// copy feeds `crossfade_policy_cfg()` so the controller's prep-time
    /// policy decision reads the same floor without taking the renderer lock.
    /// Kept in sync by `set_crossfade_min_track_secs`.
    min_track_secs: u32,
    /// The opt-in album-continuity gate (M4): sequential same-album tracks
    /// transition gapless instead of crossfading. Consumed controller-side
    /// via `crossfade_policy_cfg()`; kept in sync by
    /// `set_crossfade_album_gapless`.
    album_continuity: bool,
    /// Per-transition policy verdict for the CURRENTLY-PREPARED next track:
    /// `true` suppresses the crossfade arm/trigger so the transition falls
    /// through to the gapless path. Set by `store_prepared_decoder` (AFTER
    /// its internal `reset_next_track`, BEFORE the arm), cleared by
    /// `reset_next_track`, and gated at BOTH trigger sites
    /// (`arm_renderer_crossfade` — which also covers the rearm-after-seek
    /// path — and `try_start_crossfade_transition`), mirroring the
    /// `crossfade_blocked` dual-site pairing.
    suppress_this_transition: bool,
    /// M7: whether the live phase is a MANUAL-SKIP fade
    /// ([`CustomAudioEngine::crossfade_to_next`]) rather than an
    /// auto-advance blend. A skip already advanced the queue cursor /
    /// history / consume at skip time, so `finalize_crossfade_engine` must
    /// NOT fire the completion callback for it (the callback's
    /// `decide_transition` would advance the queue a second time, silently
    /// skipping a track) and must not stamp `last_transition_was_crossfade`
    /// (nothing consumes it — it would leak into the next transition's
    /// label). Set by `crossfade_to_next`, cleared by `cancel_crossfade`,
    /// read-and-cleared by `finalize_crossfade_engine`, and read (BEFORE
    /// the cancel clears it) by `recover_stalled_crossfade`, whose
    /// skip-aware branch hard-loads the skip target instead of routing
    /// through the end-of-track machinery (which would advance the
    /// already-advanced cursor past the target).
    skip_fade: bool,
    /// M8 "Gap / Overlap Trim" in seconds (−2..+2, default 0). Negative =
    /// extra overlap: the renderer's Armed trigger fires |offset| early
    /// (mirrored to `renderer.crossfade_lead_ms` by `set_crossfade_offset`).
    /// Positive = gap: `transition_prep_cfg()` hands it to the controller,
    /// which threads it down per-transition for the decode loop's EOF
    /// silence injection (`inject_transition_gap`).
    offset_secs: i32,
    /// M8 "Snap Crossfade to Musical Bars" (default off — opt-in). Consumed
    /// controller-side via `transition_prep_cfg()`; the snapped value rides
    /// back down as `duration_override_ms`. Kept in sync by
    /// `set_crossfade_bar_snap`.
    bar_snap: bool,
    /// M8 "Skip Silence Between Tracks" engine mirror (default off —
    /// opt-in). Feeds `transition_prep_cfg()`'s leading-trim verdict (gated
    /// off under bit-perfect modes: dropping samples is a content change);
    /// the renderer holds its own mirror for the trailing trigger. Kept in
    /// sync by `set_skip_silence`.
    skip_silence: bool,
    /// M8 per-transition crossfade-duration override (bar-snap), following
    /// the `suppress_this_transition` lifecycle EXACTLY: set by
    /// `store_prepared_decoder` (AFTER its internal `reset_next_track`,
    /// BEFORE the arm), cleared by `reset_next_track`. Every duration read
    /// on the arm/trigger/fire paths goes through
    /// [`Self::effective_duration_ms`]; the `crossfade_duration_shared`
    /// decode-loop mirror is stored/restored in lockstep so the watermark
    /// cushion always covers the duration that will actually play
    /// (invariant 9).
    duration_override_ms: Option<u64>,
}

impl CrossfadeCoordinator {
    fn new() -> Self {
        Self {
            phase: CrossfadePhase::Idle,
            enabled: false,
            duration_ms: DEFAULT_CROSSFADE_DURATION_MS,
            bit_perfect_mode: crate::types::player_settings::BitPerfectMode::Off,
            min_track_secs: crate::types::player_settings::CROSSFADE_MIN_TRACK_DEFAULT_SECS,
            album_continuity: false,
            suppress_this_transition: false,
            skip_fade: false,
            offset_secs: 0,
            bar_snap: false,
            skip_silence: false,
            duration_override_ms: None,
        }
    }

    /// The crossfade duration that applies to the CURRENTLY-PREPARED
    /// transition: the per-transition bar-snap override when one is staged,
    /// else the user's global setting. Every arm/trigger/fire duration read
    /// (`arm_renderer_crossfade`, `rearm_crossfade_if_prepared`,
    /// `try_start_crossfade_transition`, engine `start_crossfade`, and the
    /// store's own eligibility check) goes through this accessor — reading
    /// `duration_ms` directly would silently miss the override on the
    /// seek-rearm / EOF-fallback paths.
    fn effective_duration_ms(&self) -> u64 {
        self.duration_override_ms.unwrap_or(self.duration_ms)
    }

    /// Whether crossfade arming is eligible at all: the user's Crossfade toggle
    /// is on, OR bit-perfect Relaxed (which runs its own same-rate crossfade even
    /// though its mutually-exclusive Crossfade toggle is off). This is only the
    /// coarse eligibility core shared by the three trigger sites; per-transition
    /// vetoes (duration, idle phase, `crossfade_blocked` format gate) stay at
    /// their respective call sites.
    fn crossfade_eligible(&self) -> bool {
        self.enabled
            || self.bit_perfect_mode == crate::types::player_settings::BitPerfectMode::Relaxed
    }

    /// Whether a crossfade is live from EITHER side: the engine phase is not
    /// Idle OR the renderer is mid-fade. Centralizes the reconciliation that
    /// `reset_next_track` (and any future cancel site) needs: `render_tick`
    /// swaps the renderer Armed → Active synchronously and creates the live
    /// incoming stream a tick BEFORE the spawned `on_renderer_finished` task
    /// sets the engine phase, so checking the engine phase alone would miss
    /// that window and orphan the renderer's live incoming stream.
    ///
    /// DISTINCT from `try_finalize_crossfade`'s `renderer_fade_active` input,
    /// which is renderer-Active-ONLY: routing that input through here would
    /// re-create the torn-state next-track-instantly-skipped bug on coarse-VBR
    /// seeks (engine Active + renderer already finalized must still finalize).
    ///
    /// Locks the renderer (`parking_lot`, sync), reads, and DROPS the guard
    /// before returning a plain `bool` — never hands the guard back to a caller
    /// that then `.await`s (which would hold a `parking_lot` mutex across an
    /// await → deadlock; see the `reset_next_track` → `cancel_crossfade().await`
    /// path).
    fn is_crossfade_live(&self, renderer: &PlMutex<AudioRenderer>) -> bool {
        let renderer_active = renderer.lock().is_crossfade_active();
        !self.phase.is_idle() || renderer_active
    }
}

/// Engine-side mirror of the transport-fade settings — the "Fading"
/// section's STOP ramp knobs (M5, pushed via
/// [`CustomAudioEngine::set_transport_fades`]) plus the radio-switch flag
/// (M6, pushed via [`CustomAudioEngine::set_fade_radio_transitions`]).
///
/// Ownership split mirrors the min-track pattern: each side keeps exactly
/// the knobs it consumes. The PAUSE pair lives on the renderer (its two
/// consumers — `begin_pause_fade` and the resume fade-in in `start()` — are
/// there; `set_transport_fades` pushes it down), while the STOP pair's sole
/// consumer is `engine.stop()`, which both starts the out-ramp and bounds
/// its wait with the same duration — so it mirrors here. Values are clamped
/// to the `TRANSPORT_FADE_MS_{MIN,MAX}` bounds at the setter, so a
/// hand-edited config.toml can't stretch the stop ramp (and therefore the
/// engine-lock hold inside `stop()`) past the slider ceiling.
pub(crate) struct FadeCoordinator {
    /// Whether the stop out-ramp is enabled (default off — opt-in).
    fade_on_stop: bool,
    /// Stop ramp length in milliseconds.
    fade_stop_ms: u32,
    /// M6 "Fade Radio Switches" (default off — opt-in): radio↔queue
    /// switches run a fixed `RADIO_SWITCH_FADE_MS` out-ramp and arm the
    /// renderer's first-audio fade-in, instead of hard-cutting. Consumed by
    /// [`CustomAudioEngine::stop_for_radio_switch`] (queue→radio, the UI
    /// radio-start paths) and `set_source`'s internal stop (radio→queue,
    /// detected via `stream_is_infinite`).
    fade_radio_transitions: bool,
    /// M7/M10 "Fade on Skip" (default Off — opt-in): what a manual skip
    /// (Next/Previous, or a click that starts a track) does to the sound.
    /// Read by the manual-skip and click paths (via
    /// [`CustomAudioEngine::skip_fade_mode`]) to pick hard cut / boundary
    /// fade / skip-crossfade.
    fade_on_skip: crate::types::player_settings::FadeOnSkip,
    /// "Fade on Skip" length in milliseconds — the skip-crossfade overlap
    /// ([`CustomAudioEngine::crossfade_to_next`]) and the boundary ease-out
    /// ([`CustomAudioEngine::run_skip_out_fade`]) share it. Clamped to the
    /// `FADE_SKIP_SECS_{MIN,MAX}` bounds at the setter.
    fade_skip_ms: u32,
}

impl FadeCoordinator {
    fn new() -> Self {
        Self {
            fade_on_stop: false,
            fade_stop_ms: crate::types::player_settings::TRANSPORT_FADE_MS_DEFAULT,
            fade_radio_transitions: false,
            fade_on_skip: crate::types::player_settings::FadeOnSkip::Off,
            fade_skip_ms: crate::types::player_settings::FADE_SKIP_SECS_DEFAULT * 1000,
        }
    }
}

/// Per-transition verdicts the controller computes at gapless-prep time from
/// `Song` metadata (which never crosses the engine boundary) and threads down
/// into [`CustomAudioEngine::store_prepared_decoder`]. All three share the M4
/// suppress-flag lifecycle: applied AFTER the store's internal
/// `reset_next_track`, cleared BY `reset_next_track`, re-derived on every
/// prep.
#[derive(Debug, Clone, Copy, Default)]
pub struct PreparedTransitionDirectives {
    /// M4 crossfade-vs-gapless policy verdict: `true` keeps the gapless slot
    /// prepared but suppresses the crossfade arm/trigger (hard-join).
    pub suppress_crossfade: bool,
    /// M8 bar-snap: the snapped crossfade duration for THIS transition, or
    /// `None` to use the global setting. Mirrored into
    /// `crossfade_duration_shared` so the decode cushion covers it
    /// (invariant 9).
    pub duration_override_ms: Option<u64>,
    /// M8 positive gap offset: milliseconds of silence to inject between the
    /// tracks at decoder EOF (0 = seamless as before). The controller zeroes
    /// it for album-continuity joins — "Keep Gapless Albums Seamless" means
    /// no gap either.
    pub gap_offset_ms: u64,
}

impl PreparedTransitionDirectives {
    /// The M4-only shape: a policy suppress verdict with no M8 facets.
    pub fn from_suppress(suppress_crossfade: bool) -> Self {
        Self {
            suppress_crossfade,
            ..Self::default()
        }
    }
}

/// Snapshot of the transition-shaping settings the controller needs at
/// gapless-prep time, read under one brief engine lock via
/// [`CustomAudioEngine::transition_prep_cfg`]. Extends M4's
/// `crossfade_policy_cfg` with the M8 knobs.
#[derive(Debug, Clone, Copy)]
pub struct TransitionPrepCfg {
    /// Inputs to `crossfade_policy::crossfade_decision` (M4).
    pub policy: crate::audio::crossfade_policy::CrossfadePolicyCfg,
    /// M8 "Snap Crossfade to Musical Bars" (controller computes the snapped
    /// value from the OUTGOING song's BPM tag).
    pub bar_snap: bool,
    /// The user's global crossfade duration (ms) — the bar-snap input.
    pub crossfade_duration_ms: u64,
    /// M8 positive gap side of "Gap / Overlap Trim" in ms (0 when the offset
    /// is zero or negative — the negative side lives renderer-side).
    pub gap_offset_ms: u64,
    /// M8 leading-silence trim verdict for the prepared decoder: the
    /// skip-silence setting, gated off under bit-perfect modes (dropping
    /// samples is a content change a bit-perfect listener didn't ask for).
    pub trim_leading_silence: bool,
}

/// Info about a gapless transition that occurred in the decode loop.
/// The decode loop writes this, and the engine reads it to update its metadata.
#[derive(Debug, Clone)]
pub struct GaplessTransitionInfo {
    pub source: String,
    pub duration: u64,
    pub format: AudioFormat,
    pub codec: Option<String>,
}

/// Bundled gapless-prep state for the next track. Replaces the three
/// independent tokio mutexes (`next_decoder`, `next_track_prepared`,
/// `next_source_shared`) that the audit (`backend-boundary.md` §4 IG-13)
/// flagged as enforced only by reading every site.
///
/// Lock order: this struct lives behind one `Arc<tokio::sync::Mutex<…>>`,
/// so all three fields are acquired together. The decode loop, the engine
/// async path, and `cancel_crossfade` all take the same mutex in the same
/// order — the order question disappears.
pub(crate) struct GaplessSlot {
    /// Decoder for the prepared next track. `None` when nothing is staged.
    pub decoder: Option<AudioDecoder>,
    /// Source URL of the prepared track. Empty when not staged.
    pub source: String,
    /// True when the slot is fully prepared and the renderer can use it
    /// for gapless transition. Distinct from `decoder.is_some()` because
    /// the decode loop sets `prepared = false` AFTER `take`-ing the
    /// decoder (so the next loop iteration knows the slot is mid-swap).
    pub prepared: bool,
    /// ReplayGain tags of the prepared track, carried WITH the slot so a
    /// prep landing while a blend is live never overwrites the renderer's
    /// `pending_crossfade_replay_gain` (still owned by the LIVE blend —
    /// finalize promotes it into `current_replay_gain`). Re-staged into the
    /// renderer by every consumer of the slot: `rearm_crossfade_if_prepared`
    /// (finalize-time re-arm + seek re-arm) and engine `start_crossfade`
    /// (EOF-fallback trigger). The ordinary store path stages the renderer
    /// copy immediately AND records it here, so the two never disagree.
    pub replay_gain: Option<crate::types::song::ReplayGain>,
}

impl GaplessSlot {
    pub fn new() -> Self {
        Self {
            decoder: None,
            source: String::new(),
            prepared: false,
            replay_gain: None,
        }
    }

    pub(crate) fn is_prepared(&self) -> bool {
        self.prepared && self.decoder.is_some()
    }

    pub fn clear(&mut self) {
        self.decoder = None;
        self.source.clear();
        self.prepared = false;
        self.replay_gain = None;
    }
}

/// Calculate buffer size for one decode chunk (~100ms of audio).
///
/// Returns bytes for 100ms of the given format, clamped to [4096, 65_536], or
/// 8192 if the format is not yet known. With full-precision f32 output (8
/// bytes/frame stereo) the 65_536 ceiling holds a full ~100ms up to ~82k stereo
/// and ~85ms/iteration at 96k stereo; a lower ceiling would shrink hi-res reads
/// further, forcing the decode loop to run more iterations to keep pace.
fn decode_buffer_size(format: &AudioFormat) -> usize {
    if format.is_valid() {
        format.bytes_for_duration(100).clamp(4096, 65_536)
    } else {
        8192
    }
}

/// Decode one ~100ms chunk and convert it to f32 samples — the shared
/// read→validate→convert step of the play/seek prebuffers and both decode
/// loops. Returns `None` when the decoder produced no usable data this call
/// (uninitialized → `AudioBuffer::invalid()`, EOF, or an empty read).
///
/// Synchronous: the decoder does blocking HTTP I/O, so async-runtime callers
/// wrap the call in `tokio::task::block_in_place`; the seek prebuffer calls it
/// bare from inside an existing `block_in_place` closure. Takes only
/// `&mut AudioDecoder` and acquires NO locks, so callers keep their decoder
/// guard and the lock ordering is untouched.
fn decode_one_chunk(decoder: &mut AudioDecoder) -> Option<Vec<f32>> {
    let buffer_size = decode_buffer_size(decoder.format());
    let buffer = decoder.read_buffer(buffer_size);
    if buffer.is_valid() && buffer.byte_count() > 0 {
        Some(decoded_bytes_to_f32(buffer.data()))
    } else {
        None
    }
}

/// Buffers to fill before starting playback. `play` cold-starts the decoder
/// and renderer together so it needs more buffers to absorb the worst-case
/// network latency before the first sample feeds out; `seek` runs against a
/// renderer that's already initialized so it can prime with fewer buffers.
const PLAY_PREBUFFER_COUNT: usize = 15;
const SEEK_PREBUFFER_COUNT: usize = 10;

// Load-bearing invariant: `play` must prime more buffers than `seek` (see the
// rationale above). Enforced at compile time per the `assertions_on_constants`
// convention so a future tuning can't silently invert the relationship.
const _: () = assert!(PLAY_PREBUFFER_COUNT > SEEK_PREBUFFER_COUNT);

/// Target decoded-ring cushion, in MILLISECONDS of audio — the decode-loop
/// backpressure HIGH watermark. The loop fills the ring to ~this much decoded
/// audio, then idles until it drains to the release point
/// (`CUSHION_MS / BACKPRESSURE_RELEASE_DIVISOR`). Expressed in TIME and scaled by
/// the stream's `frame_rate` (`compute_watermarks`) so the cushion holds a
/// constant ~1.1s at EVERY sample rate. The old fixed-sample floor
/// (`BASE_HIGH(120) * 800` = 96_000 samples) silently shrank to ~0.5s at 96k,
/// which — paired with the renderer's rebuffer entry watermark — caused the
/// issue-9 hi-res pause-and-rebuffer deadlock. ~1.1s mirrors mpv cache /
/// MPD `buffer_before_play` / GStreamer queue2. `pub(crate)` so the renderer
/// derives matching, also-time-based rebuffer thresholds.
pub(crate) const CUSHION_MS: u64 = 1100;

/// The decode loop releases backpressure (resumes decoding) once the ring drains
/// to `CUSHION_MS / BACKPRESSURE_RELEASE_DIVISOR` of audio. `pub(crate)` so the
/// renderer can assert at compile time that its rebuffer entry watermark stays
/// strictly below this release point at every sample rate (the load-bearing
/// interlock that keeps the issue-9 hi-res deadlock from returning).
pub(crate) const BACKPRESSURE_RELEASE_DIVISOR: u64 = 3;

/// Legacy base cushion in buffer units (the old `BASE_HIGH`). Retained ONLY to
/// reproduce the historical crossfade-cushion crossover (~11s) now that the
/// cushion is time-based: one legacy unit == `CUSHION_MS / CUSHION_BASE_UNITS`.
const CUSHION_BASE_UNITS: u64 = 120;

/// Radio jitter prebuffer: keep output paused after (re)connect until about
/// this much audio is buffered.
const RADIO_JITTER_PREBUFFER_MS: u64 = 5000;

/// Default crossfade duration (ms) seeded at construction, before settings
/// apply. Keeps the shared atomic (decode-loop watermarks) and the plain field
/// (arm_crossfade) in lockstep.
const DEFAULT_CROSSFADE_DURATION_MS: u64 = 5000;

/// The lock-free channels the decode loop reads/writes (and that the renderer
/// shares for crossfade/EOF gating). Bundled so the cross-thread wiring lives in
/// one place and `clone_for_decode_loop()` hands the spawned task exactly these
/// Arcs (same identity, never a freshly-allocated look-alike).
///
/// Membership is deliberately exactly the atomics cloned into the primary decode
/// task. `live_sample_rate` is intentionally NOT here: it is written purely from
/// engine self-methods (never cloned into the loop), so it is not a decode-loop
/// channel and stays a direct field on `CustomAudioEngine`.
struct DecodeLoopChannels {
    /// Incremented on every source change. Shared with the renderer so
    /// completion callbacks can detect staleness (e.g. manual skip raced
    /// with track-end) without needing the engine lock.
    source_generation: SourceGeneration,
    /// Set by the decode loop when the primary decoder reaches EOF.
    /// Shared with the renderer to gate crossfade trigger: prevents false
    /// triggers from transiently empty buffers after a seek.
    decoder_eof: Arc<AtomicBool>,
    /// Set by the decode loop to the cached stream type. Shared with the renderer
    /// so the mid-track network rebuffer only runs on FINITE (seekable) streams.
    stream_is_infinite: Arc<AtomicBool>,
    /// Lock-free crossfade duration for the decode loop's dynamic backpressure.
    /// Updated by `set_crossfade_duration()`, read by the spawned decode task.
    crossfade_duration_shared: Arc<AtomicU64>,
    /// Live compressed bitrate from decoder (updated per-packet in decode loop).
    live_bitrate: Arc<AtomicU32>,
    /// M7 skip-fade plan window latch: the source generation a pending
    /// [`SkipFadePlan`](crate::services::playback::SkipFadePlan) was stamped
    /// with at plan time (`plan_skip_fade`), or `NO_SKIP_FADE_PENDING` when
    /// no plan is in flight. While `latch == source_generation.current()`
    /// the queue has ALREADY advanced for a skip whose audio transition is
    /// still building (locks released), so track-completion machinery must
    /// stand down — advancing again would double-advance the queue.
    /// Self-invalidating: every path out of the window either bumps the
    /// generation (the fire, the fallback's `set_source`, any competing
    /// source change) or closes the latch explicitly
    /// (`close_skip_fade_window` on the seq-abandon exit, whose superseding
    /// no-op skip never touches the engine), so a stale latch can never
    /// suppress a later completion.
    skip_fade_pending: Arc<AtomicU64>,
    /// M8 positive "Gap / Overlap Trim": milliseconds of silence to inject
    /// between the outgoing track's last sample and the next track at the
    /// NEXT decoder EOF, or 0. Per-transition, mirroring the M4 suppress-flag
    /// lifecycle: set by `store_prepared_decoder` (after its internal
    /// `reset_next_track`), cleared by `reset_next_track` (the invariant-3
    /// funnel every transition abandonment goes through), and consumed
    /// one-shot (`swap(0)`) by the decode loop's EOF branch
    /// (`inject_transition_gap`). Deliberately NOT cleared at
    /// `start_decoding_loop`: the finalize-time loop restart must preserve a
    /// gap staged by a mid-blend prep for the transition AFTER the promoted
    /// track.
    gap_offset_ms: Arc<AtomicU64>,
}

/// Sentinel for `DecodeLoopChannels::skip_fade_pending`: no skip-fade plan
/// window is open. `u64::MAX` can never equal a real generation (the counter
/// starts at 0 and increments by 1 per user action).
const NO_SKIP_FADE_PENDING: u64 = u64::MAX;

/// The subset of `DecodeLoopChannels` cloned into one spawned decode task,
/// produced by `clone_for_decode_loop()`. Keeping the clone in one method
/// guarantees the task gets the SAME Arcs the engine and renderer hold — a
/// fresh `Arc::new(...)` here would lint clean yet silently break EOF/crossfade.
struct ClonedDecodeLoopChannels {
    source_generation: SourceGeneration,
    decoder_eof: Arc<AtomicBool>,
    stream_is_infinite: Arc<AtomicBool>,
    crossfade_duration_shared: Arc<AtomicU64>,
    live_bitrate: Arc<AtomicU32>,
    skip_fade_pending: Arc<AtomicU64>,
    gap_offset_ms: Arc<AtomicU64>,
}

impl DecodeLoopChannels {
    fn new() -> Self {
        Self {
            source_generation: SourceGeneration::new(),
            decoder_eof: Arc::new(AtomicBool::new(false)),
            stream_is_infinite: Arc::new(AtomicBool::new(false)),
            crossfade_duration_shared: Arc::new(AtomicU64::new(DEFAULT_CROSSFADE_DURATION_MS)),
            live_bitrate: Arc::new(AtomicU32::new(0)),
            skip_fade_pending: Arc::new(AtomicU64::new(NO_SKIP_FADE_PENDING)),
            gap_offset_ms: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Clone the Arcs the primary decode loop captures. Identity-preserving by
    /// construction (`.clone()` on each Arc, never a new allocation).
    fn clone_for_decode_loop(&self) -> ClonedDecodeLoopChannels {
        ClonedDecodeLoopChannels {
            source_generation: self.source_generation.clone(),
            decoder_eof: self.decoder_eof.clone(),
            stream_is_infinite: self.stream_is_infinite.clone(),
            crossfade_duration_shared: self.crossfade_duration_shared.clone(),
            live_bitrate: self.live_bitrate.clone(),
            skip_fade_pending: self.skip_fade_pending.clone(),
            gap_offset_ms: self.gap_offset_ms.clone(),
        }
    }
}

/// Compute backpressure watermarks `(high, low)` in interleaved SAMPLES, scaled
/// to the stream's `frame_rate` (`sample_rate * channels`) so the cushion is a
/// constant TIME at any rate. Shared by the primary and crossfade decode loops.
/// `frame_rate == 0` (format not yet known) yields a non-triggering `high` so the
/// loop never backpressures before the first decoded buffer establishes the rate.
fn compute_watermarks(frame_rate: u32, crossfade_ms: u64) -> (usize, usize) {
    if frame_rate == 0 {
        return (usize::MAX, 0);
    }
    // Cushion in TIME. Base ~CUSHION_MS; for long crossfades grow it so the
    // outgoing stream keeps a fade-length decode lead — a faithful time port of
    // the legacy `max(BASE_HIGH, crossfade_ms/100 + 10)` buffer-unit formula
    // (crossover near ~11s), so realistic crossfades are unchanged.
    let crossfade_cushion_ms = if crossfade_ms > 0 {
        (crossfade_ms / 100 + 10) * CUSHION_MS / CUSHION_BASE_UNITS
    } else {
        0
    };
    let cushion_ms = CUSHION_MS.max(crossfade_cushion_ms);
    let high = samples_for_duration(frame_rate, cushion_ms);
    let low = high / BACKPRESSURE_RELEASE_DIVISOR as usize;
    (high, low)
}

/// Periodic stream-health observability for the primary decode loop.
/// Observability only — no control flow. Fires at most once every 5 s and
/// emits a debug line ONLY on anomaly (an underrun was recorded or at least
/// one empty/invalid buffer was seen since the last reset); silent ticks are
/// noise.
///
/// `last_heartbeat` and `empty_buffer_count` are SHARED with the decode loop
/// (the heartbeat clock is reused by the loop's 10 s liveness trace, and the
/// counter is incremented by the sibling empty-buffer branch), so both are
/// taken by `&mut`: this helper reads the heartbeat gate, then on fire resets
/// the counter to 0 and the heartbeat to now — exactly as the inlined block
/// did. Passing either by value would silently break heartbeat cadence and
/// latch the anomaly gate forever.
///
/// Takes the renderer's parking_lot mutex briefly to snapshot buffer/underrun
/// stats and drops the guard before logging — no `.await`, no lock held across
/// a suspension point.
fn log_stream_health(
    renderer: &PlMutex<AudioRenderer>,
    frame_rate: u32,
    crossfade_ms: u64,
    last_heartbeat: &mut std::time::Instant,
    empty_buffer_count: &mut u64,
) {
    if last_heartbeat.elapsed().as_secs() < 5 {
        return;
    }
    let guard = renderer.lock();
    let buffered = guard.buffer_count();
    let (ur_count, ur_peak, ur_total) = guard.underrun_stats();
    drop(guard);
    // Emit only on anomaly — silent ticks are noise.
    if ur_count > 0 || *empty_buffer_count > 0 {
        // Pure + cheap recompute (fires at most every 5s,
        // and only on anomaly).
        let (high_watermark, low_watermark) = compute_watermarks(frame_rate, crossfade_ms);
        let frame_rate_f = frame_rate.max(1) as f32;
        let sec_rem = buffered as f32 / frame_rate_f;
        let peak_ms = ur_peak as f32 * 1000.0 / frame_rate_f;
        tracing::debug!(
            "🔌 [STREAM HEALTH] Buffer: {} ({:.1}s) | Underruns: {} (peak {:.0}ms) | Silence: {} | EmptyBufs: {} | HW: {} LW: {}",
            buffered,
            sec_rem,
            ur_count,
            peak_ms,
            ur_total,
            *empty_buffer_count,
            high_watermark,
            low_watermark,
        );
    }
    *empty_buffer_count = 0;
    *last_heartbeat = std::time::Instant::now();
}

/// Radio jitter prebuffer step: on the initial (re)connect of an infinite
/// stream, keep the renderer paused until ~`RADIO_JITTER_PREBUFFER_MS` of audio
/// has accumulated, then start playback exactly once.
///
/// `radio_music_jitter_filled` is the SHARED latch the reconnect path resets to
/// `false`, so it is taken by `&mut`: the helper flips it `true` on fill, and a
/// later radio reconnect clears it to re-arm the prebuffer. Passing it by value
/// would desync the latch from the reset and leave a silent gap after a radio
/// reconnect.
///
/// Pause-continuously-until-full is preserved: this runs every decoded chunk
/// while the buffer is short of the target and re-issues `pause()` each time, so
/// a racing front-end `engine.play()` cannot unpause the stream prematurely —
/// there is no run-once guard or hoist short-circuiting that re-pause.
fn radio_jitter_prebuffer_step(
    renderer: &PlMutex<AudioRenderer>,
    frame_rate: u32,
    is_infinite: bool,
    radio_music_jitter_filled: &mut bool,
) {
    // Radio jitter buffer: initial prebuffer only, then never pause.
    // SomaFM sends at exactly 1.0× realtime, so the buffer level
    // will hover near the consumption rate. Pausing playback to
    // re-buffer causes audible gaps — instead, let transient
    // underruns produce natural silence via try_pop().unwrap_or(0.0).
    if is_infinite && !*radio_music_jitter_filled {
        let buffered_samples = renderer.lock().buffer_count();
        let jitter_target = samples_for_duration(frame_rate, RADIO_JITTER_PREBUFFER_MS);
        if buffered_samples < jitter_target {
            // Enforce pause continuously until full. This prevents front-end
            // UI events (like `engine.play()`) from unpausing prematurely.
            renderer.lock().pause();
        } else {
            tracing::info!("📻 [DECODE LOOP] Pre-buffered 5+ seconds of radio, starting playback.");
            *radio_music_jitter_filled = true;
            renderer.lock().start();
        }
    }
}

/// Outcome of the radio-reconnect loop, returned to the decode loop so the
/// caller owns the `continue 'decode_loop` / `break` control flow (the helper
/// stays a plain `async fn` with no labeled-break visibility).
///
/// EOF-store semantics are SACRED and asymmetric:
/// - `GaveUp` HAS ALREADY stored `decoder_eof = true` inside the helper (the
///   reconnect exhausted its retries; the renderer must learn the radio is
///   dead). Forgetting this store leaves the radio permanently silent.
/// - `Superseded` does NOT store EOF and the caller must `break` immediately: a
///   newer decode loop (user skip/stop) owns the source now, so storing EOF
///   here would be attributed to the new track and end it prematurely
///   (stale-EOF-skips-next-track bug).
/// - `Reconnected` resumes the SAME decode loop: the caller resets the
///   stream-type / jitter latches and `continue 'decode_loop`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RadioReconnectOutcome {
    /// Re-init succeeded; resume decoding (caller re-arms latches + continues).
    Reconnected,
    /// A newer decode loop superseded this one; abort WITHOUT storing EOF.
    Superseded,
    /// Retries exhausted; EOF already stored. Caller exits the decode loop.
    GaveUp,
}

/// Radio reconnect loop: the infinite-stream EOF path (connection dropped /
/// server closed). Takes the live `decoder_guard` BY VALUE and drops it BEFORE
/// the first backoff sleep — holding the primary decoder lock across the sleep
/// would deadlock the UI tick (which reads position/duration via
/// `decoder.lock()`). The drop-before-sleep is therefore structural and cannot
/// be reordered by a future edit.
///
/// Backoff is `2^min(retry, 4)` seconds, up to `MAX_RETRIES` attempts. Each
/// iteration re-checks `decode_gen.current() == my_gen` first and aborts
/// (`Superseded`) if a user skip/stop spawned a newer loop, BEFORE consuming a
/// retry or touching the `reconnect_url` re-init — so a superseded loop never
/// stores EOF. See `RadioReconnectOutcome` for the EOF-store contract.
async fn radio_reconnect_loop(
    decoder: &tokio::sync::Mutex<AudioDecoder>,
    decoder_guard: tokio::sync::MutexGuard<'_, AudioDecoder>,
    decode_gen: &DecodeLoopHandle,
    my_gen: u64,
    decoder_eof: &AtomicBool,
    reconnect_url: &str,
) -> RadioReconnectOutcome {
    tracing::warn!("📻 [DECODE LOOP] Radio stream dropped, attempting reconnect...");
    // CRITICAL: Drop decoder_guard BEFORE sleeping!
    // Holding this lock during the backoff would deadlock the UI tick
    // (which reads position/duration via decoder.lock()).
    drop(decoder_guard);

    let mut retry_count = 0u32;
    const MAX_RETRIES: u32 = 5;

    loop {
        // Abort reconnect if source changed (user skipped/stopped)
        if decode_gen.current() != my_gen {
            tracing::debug!("📻 [RECONNECT] Aborted — generation superseded");
            return RadioReconnectOutcome::Superseded;
        }
        retry_count += 1;
        if retry_count > MAX_RETRIES {
            tracing::error!(
                "📻 [RECONNECT] Failed after {} attempts, giving up",
                MAX_RETRIES
            );
            decoder_eof.store(true, Ordering::Release);
            return RadioReconnectOutcome::GaveUp;
        }
        let backoff = std::time::Duration::from_secs(1u64 << retry_count.min(4));
        tracing::debug!(
            "📻 [RECONNECT] Attempt {}/{} in {:?}",
            retry_count,
            MAX_RETRIES,
            backoff
        );
        tokio::time::sleep(backoff).await;

        // Re-acquire decoder lock for re-init
        let mut dec = decoder.lock().await;
        match dec.init(reconnect_url).await {
            Ok(()) => {
                tracing::info!("📻 [RECONNECT] Success!");
                drop(dec);
                return RadioReconnectOutcome::Reconnected;
            }
            Err(e) => {
                tracing::debug!("📻 [RECONNECT] Failed: {}", e);
                drop(dec);
                // Continue retry loop
            }
        }
    }
}

/// What a decode loop should do this iteration about ring-buffer backpressure.
/// Mirrors the renderer's `RebufferAction` pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackpressureAction {
    /// Ring is above the high watermark (or still draining toward the low
    /// watermark): sleep this long, then re-check. The caller owns the actual
    /// `sleep().await` + `continue` so the helper stays sync and lock-free.
    Sleep(std::time::Duration),
    /// Not backpressured (or just released): go decode a chunk.
    Proceed,
}

/// Shared dual-watermark backpressure step for the primary and crossfade
/// decode loops. Computes the time-based watermarks (`compute_watermarks`),
/// flips the caller's `backpressure_active` latch, and returns the action.
///
/// Synchronous, takes NO locks, never awaits: the caller snapshots
/// `buffer_count` with the renderer lock already dropped, then performs the
/// returned sleep itself.
///
/// `is_infinite` MUST be `false` for the crossfade loop (incoming crossfade
/// tracks are always finite). Radio (`is_infinite == true`) NEVER backpressures:
/// radio streams send data precisely at 1x speed, and sleeping the decode task
/// would neglect the raw TCP socket — the resulting TCP zero window makes
/// Icecast drop the connection with 1-2 second reconnect delays. Symphonia must
/// read the Icecast stream continuously to maintain network stability. Since
/// the latch can then never engage for radio, the OFF/hold branches are
/// unreachable there.
fn backpressure_step(
    label: &'static str,
    buffer_count: usize,
    frame_rate: u32,
    crossfade_ms: u64,
    is_infinite: bool,
    backpressure_active: &mut bool,
) -> BackpressureAction {
    let (high_watermark, low_watermark) = compute_watermarks(frame_rate, crossfade_ms);
    if buffer_count >= high_watermark && !is_infinite {
        if !*backpressure_active {
            tracing::trace!(
                "⏸️ [{label}] Backpressure ON: buffer count {buffer_count} >= {high_watermark} (high watermark, cf={crossfade_ms}ms)"
            );
            *backpressure_active = true;
        }
        // Sleep longer while waiting for buffers to drain
        BackpressureAction::Sleep(std::time::Duration::from_millis(100))
    } else if *backpressure_active && buffer_count <= low_watermark {
        tracing::trace!(
            "▶️ [{label}] Backpressure OFF: buffer count {buffer_count} <= {low_watermark} (low watermark)"
        );
        *backpressure_active = false;
        BackpressureAction::Proceed
    } else if *backpressure_active {
        // Still in backpressure mode, waiting for low_watermark
        BackpressureAction::Sleep(std::time::Duration::from_millis(50))
    } else {
        BackpressureAction::Proceed
    }
}

/// The inline gapless swap stands down when a crossfade is armed OR active, so
/// the renderer's position-based crossfade trigger owns the transition. This
/// decouples the decoded-cushion size (BASE_HIGH / decode_lead) from the
/// crossfade lead — without the gate, a cushion >= the crossfade duration would
/// let the EOF gapless swap win the race and strand a phantom (dead-air)
/// crossfade. BOTH predicates are required: render_tick flips Armed->Active
/// synchronously while the engine clears the prepared slot asynchronously, so a
/// gate on `armed` alone leaves a window where `armed==false` but the slot is
/// still prepared.
fn should_attempt_gapless_swap(
    renderer_crossfade_armed: bool,
    renderer_crossfade_active: bool,
) -> bool {
    !renderer_crossfade_armed && !renderer_crossfade_active
}

/// Outcome of an inline EOF gapless-swap attempt (`try_gapless_swap`). The five
/// variants are the five mutually-exclusive exit paths of the original inline
/// block; the caller branches on them so each path's side effects stay explicit.
///
/// Only `Swapped` actually advanced to the next track. The caller MUST clear its
/// loop-local `backpressure_active` latch on `Swapped` ONLY (the new track's
/// freshly-emptied ring would otherwise inherit a stale "backpressured" latch and
/// take an unwarranted sleep before the next decode). Every other variant left
/// the primary decoder untouched and put any taken decoder BACK in the slot, so
/// the caller falls through to the EOF-signal path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GaplessSwapOutcome {
    /// The next decoder was installed as the primary decoder: source generation
    /// bumped (`bump_for_gapless`, the SOLE decode-loop bump), the slot cleared,
    /// transition info populated, and the completion callback fired. The caller
    /// clears `backpressure_active` and `continue`s the decode loop with no gap.
    Swapped,
    /// Nothing was staged (`!slot.is_prepared()`) — no swap possible.
    NotPrepared,
    /// The staged decoder's format didn't match the live stream's (or the
    /// RG-track gain differs): the decoder was put BACK in the slot for a later
    /// retry / the renderer's crossfade trigger.
    FormatMismatch,
    /// A crossfade is armed or active, so the renderer's position-based trigger
    /// owns the transition: the staged decoder was put BACK so that trigger can
    /// take it.
    CrossfadeActive,
    /// A planned manual skip-crossfade's build window is open (M7,
    /// `plan_skip_fade`): the queue cursor has ALREADY advanced for the skip,
    /// so an inline swap here would play the wrong track and its completion
    /// callback would advance the cursor a second time. The staged decoder
    /// was put BACK; the skip's fire (or its hard fallback) owns the
    /// transition.
    SkipFadePlanPending,
    /// The slot claimed `prepared` but its decoder was missing — the stale
    /// `prepared` flag was cleared.
    DecoderMissing,
}

/// Inline EOF gapless swap: when the primary decoder hits EOF on a finite
/// stream, try to swap the prepared next-track decoder straight into the primary
/// slot so the decode loop continues with NO gap. Extracted verbatim from the
/// decode loop; see `GaplessSwapOutcome` for the five exit paths.
///
/// Lock discipline (preserved exactly): the caller has ALREADY dropped the
/// primary `decoder_guard` and passes `current_format` (snapshotted from it) by
/// value. This function then takes the `gapless` slot lock (OUTER) and, while
/// holding it, briefly takes the sync `renderer` lock (INNER) for the swap-allow
/// / crossfade-state read — slot-outer / renderer-inner nesting. On the success
/// path it drops the slot lock BEFORE locking the primary `decoder`, then bumps
/// `source_generation` (the SOLE decode-loop bump, AFTER the decoder install and
/// BEFORE the renderer position reset), resets the renderer, stores the
/// transition info, and fires the completion callback. The `renderer` lock is a
/// `parking_lot` (sync) mutex and is always scoped + dropped before any `.await`.
#[allow(clippy::too_many_arguments)]
async fn try_gapless_swap(
    decoder: &tokio::sync::Mutex<AudioDecoder>,
    renderer: &PlMutex<AudioRenderer>,
    gapless: &tokio::sync::Mutex<GaplessSlot>,
    gapless_info: &tokio::sync::Mutex<Option<GaplessTransitionInfo>>,
    source_generation: &SourceGeneration,
    completion_callback: &Option<Arc<dyn Fn(bool) + Send + Sync>>,
    current_format: &AudioFormat,
    skip_fade_pending: &Arc<AtomicU64>,
) -> GaplessSwapOutcome {
    let mut slot = gapless.lock().await;
    if !slot.is_prepared() {
        drop(slot);
        GaplessSwapOutcome::NotPrepared
    } else if let Some(next_dec) = slot.decoder.take() {
        // Hold the slot lock through the format check + ownership
        // transition so `prepared` and `decoder` flip atomically.
        let next_fmt = next_dec.format().clone();
        let formats_match = current_format.is_valid()
            && next_fmt.is_valid()
            && current_format.sample_rate() == next_fmt.sample_rate()
            && current_format.channel_count() == next_fmt.channel_count();
        // RG-track mode: the live stream's amplify factor is baked
        // at create time; deny gapless when the next track needs a
        // different gain.
        let (rg_allows_swap, cf_armed, cf_active) = {
            let r = renderer.lock();
            (
                r.gapless_swap_allowed(),
                r.is_crossfade_armed(),
                r.is_crossfade_active(),
            )
        };
        if !rg_allows_swap {
            tracing::debug!("🔄 [DECODE LOOP] RG-track gain differs — denying gapless swap");
        }

        // M7: a planned manual skip's build window is open (latch matches
        // the live generation) — the queue cursor ALREADY advanced for the
        // skip, so swapping here would audibly play the wrong track and the
        // completion callback below would advance the cursor a second time.
        // Stand down; the skip's fire (or its hard fallback) owns the
        // transition. Checked FIRST so the put-back happens before any
        // side effects.
        if skip_fade_pending.load(Ordering::Acquire) == source_generation.current() {
            tracing::debug!(
                "🔀 [DECODE LOOP] Skip-fade plan pending — standing down (the skip owns the transition)"
            );
            slot.decoder = Some(next_dec);
            drop(slot);
            return GaplessSwapOutcome::SkipFadePlanPending;
        }

        // Crossfade owns the transition: when a crossfade is
        // armed or active, the inline gapless swap stands down
        // (should_attempt_gapless_swap) so the renderer's
        // position-based trigger fires the configured fade. This
        // decouples the decoded cushion (BASE_HIGH=120, ~1.1s)
        // from the crossfade lead; reverting the gate would let a
        // cushion >= the crossfade duration re-strand a phantom
        // crossfade (the dead-air bug).
        if formats_match && rg_allows_swap && should_attempt_gapless_swap(cf_armed, cf_active) {
            let next_duration = next_dec.duration();
            let next_source_url = std::mem::take(&mut slot.source);
            let next_codec = next_dec.live_codec();
            slot.prepared = false;
            drop(slot); // release before locking decoder + renderer

            // Swap into primary decoder
            *decoder.lock().await = next_dec;

            // Increment source generation for stale callback detection
            source_generation.bump_for_gapless();

            // Reset renderer position for the new track and
            // promote the staged crossfade RG to "current"
            // (since we're keeping the same stream, the
            // amplify factor is already correct — we just
            // need our bookkeeping to reflect the new track).
            {
                let mut r = renderer.lock();
                r.reset_position();
                r.reset_finished_called();
                r.adopt_pending_crossfade_replay_gain();
            }

            // Store transition info for the engine to pick up
            {
                let mut info = gapless_info.lock().await;
                *info = Some(GaplessTransitionInfo {
                    source: next_source_url,
                    duration: next_duration,
                    format: next_fmt,
                    codec: next_codec,
                });
            }

            // Fire completion callback so the UI updates
            // (queue advances, track info refreshes)
            if let Some(cb) = completion_callback {
                cb(false);
            }

            tracing::info!("🎵 [DECODE LOOP] Gapless transition — continuing decode loop");
            GaplessSwapOutcome::Swapped
        } else {
            let crossfade_owns = !should_attempt_gapless_swap(cf_armed, cf_active);
            if crossfade_owns {
                tracing::debug!(
                    "🔀 [DECODE LOOP] Crossfade armed/active — deferring transition to the renderer trigger (skipping inline gapless)"
                );
            } else {
                tracing::debug!(
                    "🔄 [DECODE LOOP] Format mismatch for gapless: {:?} → {:?}",
                    current_format,
                    next_fmt
                );
            }
            // Put the decoder back so a future swap can retry,
            // or so the renderer's crossfade trigger can take it.
            slot.decoder = Some(next_dec);
            drop(slot);
            if crossfade_owns {
                GaplessSwapOutcome::CrossfadeActive
            } else {
                GaplessSwapOutcome::FormatMismatch
            }
        }
    } else {
        // Slot said prepared but decoder was missing — clear.
        slot.prepared = false;
        drop(slot);
        GaplessSwapOutcome::DecoderMissing
    }
}

/// M8 positive "Gap / Overlap Trim": at the outgoing decoder's EOF, write the
/// pending per-transition gap into the primary ring as silence, so exactly
/// `gap_offset_ms` of quiet sits between the outgoing's last sample and
/// whatever follows (the inline gapless swap's first samples, or — when no
/// swap happens — the ring simply drains `gap` ms later, delaying the
/// completion gate and the fresh load by the same amount).
///
/// One-shot: the pending value is consumed (`swap(0)`) whether or not the
/// injection proceeds — the transition it described is being resolved right
/// now either way. Stands down (after consuming) when:
/// - the renderer crossfade is armed/active: the blend owns this transition
///   (a crossfade and a gap are mutually exclusive; the crossfade wins), or
/// - a manual skip-fade plan window is open (the skip owns the transition), or
/// - the format is unknown (`frame_rate == 0` — nothing was ever decoded).
///
/// Lock discipline: brief renderer locks only (state read, chunked
/// `write_samples`), dropped before every await; the write-retry wait mirrors
/// the decode loop's `consumed_notify` pattern and re-checks the loop
/// generation so a superseding action aborts the injection mid-way.
#[expect(
    clippy::too_many_arguments,
    reason = "decode-loop helper threading the loop's own captured channels; bundling them into a one-off struct would just rename the call site"
)]
async fn inject_transition_gap(
    renderer: &PlMutex<AudioRenderer>,
    gap_offset_ms: &Arc<AtomicU64>,
    skip_fade_pending: &Arc<AtomicU64>,
    source_generation: &SourceGeneration,
    decode_gen: &DecodeLoopHandle,
    my_gen: u64,
    frame_rate: u32,
    consumed_notify: &Arc<Notify>,
) {
    // One-shot consume FIRST: whichever way this EOF resolves, the pending
    // value described exactly this transition.
    let gap_ms = gap_offset_ms.swap(0, Ordering::AcqRel);
    if gap_ms == 0 || frame_rate == 0 {
        return;
    }
    if skip_fade_pending.load(Ordering::Acquire) == source_generation.current() {
        debug!("⏸️ [DECODE LOOP] Gap offset stands down — skip-fade plan window open");
        return;
    }
    let (cf_armed, cf_active) = {
        let r = renderer.lock();
        (r.is_crossfade_armed(), r.is_crossfade_active())
    };
    if cf_armed || cf_active {
        debug!("⏸️ [DECODE LOOP] Gap offset stands down — crossfade owns this transition");
        return;
    }

    let total = samples_for_duration(frame_rate, gap_ms);
    debug!(
        "⏸️ [DECODE LOOP] Injecting {}ms transition gap ({} silence samples)",
        gap_ms, total
    );
    // Chunked writes with the decode loop's own write-retry shape: wait on
    // consumed_notify (bounded) when the ring is full, abort if superseded.
    const GAP_CHUNK: usize = 4_096;
    let zeros = [0.0f32; GAP_CHUNK];
    let mut remaining = total;
    while remaining > 0 {
        if decode_gen.current() != my_gen {
            debug!("⏸️ [DECODE LOOP] Gap injection superseded — aborting");
            return;
        }
        let n = remaining.min(GAP_CHUNK);
        let written = {
            let mut renderer_guard = renderer.lock();
            renderer_guard.write_samples(&zeros[..n])
        };
        remaining -= written;
        if written < n {
            let _ = tokio::time::timeout(
                tokio::time::Duration::from_millis(500),
                consumed_notify.notified(),
            )
            .await;
        }
    }
}

/// Thread-safe slot holding an optional metadata string with non-blocking
/// reset (B11 fix — `reset` must never block the audio hot path) and a
/// blocking writer for decoder-init updates that must land before the next
/// packet. Replaces ad-hoc `Arc<RwLock<Option<String>>>` field patterns on
/// the engine so the reset/write asymmetry is encoded structurally and
/// impossible to drift.
pub(super) struct LiveStringSlot {
    inner: Arc<std::sync::RwLock<Option<String>>>,
}

impl LiveStringSlot {
    pub(super) fn new() -> Self {
        Self {
            inner: Arc::new(std::sync::RwLock::new(None)),
        }
    }

    /// Clone the inner `Arc` handle. Used to pass the slot into the decoder's
    /// IcyMetadataReader callback, which writes through the cloned Arc on its
    /// own thread.
    pub(super) fn clone_arc(&self) -> Arc<std::sync::RwLock<Option<String>>> {
        self.inner.clone()
    }

    /// Non-blocking reset. Used on the audio hot path (`set_source`) — drops
    /// the write attempt silently if a reader/writer is mid-flight rather
    /// than stalling. B11 fix encoded structurally.
    pub(super) fn reset(&self) {
        if let Ok(mut guard) = self.inner.try_write() {
            *guard = None;
        }
    }

    /// Blocking write. Used during decoder-init paths where the new codec
    /// name MUST land before the next read so downstream readers don't
    /// observe the previous track's value.
    pub(super) fn set(&self, value: Option<String>) {
        if let Ok(mut guard) = self.inner.write() {
            *guard = value;
        }
    }

    /// Clone-on-read getter. Returns `None` on poisoned lock (same semantics
    /// as the prior bare `.read().ok().and_then(...)` pattern).
    pub(super) fn get(&self) -> Option<String> {
        self.inner.read().ok().and_then(|guard| guard.clone())
    }
}

/// Custom audio engine - main orchestrator
pub struct CustomAudioEngine {
    source: String,
    playing: bool,
    paused: bool,
    position: u64, // milliseconds
    duration: u64, // milliseconds
    volume: f64,   // 0.0-1.0

    // Decoder
    decoder: Arc<tokio::sync::Mutex<AudioDecoder>>,

    // Format tracking for gapless
    current_format: AudioFormat,
    next_format: AudioFormat,

    // Next track source
    next_source: String,

    /// One-shot start offset (ms) consumed by `play()`'s fresh-start branch.
    /// Armed when a pulled server queue is staged on a paused/stopped engine
    /// (`PlaybackController::cue_pulled_queue`) so the next Play resumes
    /// mid-song at the server-saved position — the decoder seeks BEFORE the
    /// renderer starts, so no position-0 audio is ever rendered. Cleared by
    /// `set_source` (any new load intent invalidates a stale offset).
    pending_start_ms: Option<u64>,

    // Renderer
    renderer: Arc<PlMutex<AudioRenderer>>,

    // State
    state: PlaybackState,

    // Decoding loop cancellation: each spawned loop captures the current
    // generation at spawn time and exits when the generation no longer matches.
    // This prevents the old loop from continuing when a new loop starts.
    decode_loop: DecodeLoopHandle,

    // Gapless preloading state — bundles `decoder`, `source`, `prepared`
    // under one tokio mutex so the lock order across the decode loop,
    // engine async path, and crossfade cancel is enforced structurally.
    gapless: Arc<tokio::sync::Mutex<GaplessSlot>>,

    // Completion callback — called when a track ends.
    // The bool argument is `true` when the same track is looping (repeat-one),
    // `false` when advancing to a new track.
    completion_callback: Option<Arc<dyn Fn(bool) + Send + Sync>>,

    // Seeking flag - prevents EOF detection during seek
    seeking: Arc<AtomicBool>,

    // Live sample rate from decoder (updated when format is set, atomic for threading consistency)
    live_sample_rate: Arc<AtomicU32>,

    // Dedicated render thread (decoupled from iced event loop)
    render_thread: Option<std::thread::JoinHandle<()>>,
    render_running: Arc<AtomicBool>,

    /// Lock-free channels the decode loop reads/writes, with the renderer-shared
    /// subset (`source_generation` / `decoder_eof` / `stream_is_infinite`)
    /// installed into the renderer by `set_engine_link`. See `DecodeLoopChannels`.
    channels: DecodeLoopChannels,

    // ---- Crossfade state ----
    /// Engine-side crossfade cluster: live phase + config (enabled / duration /
    /// bit-perfect mirror) + the `crossfade_eligible` / `is_crossfade_live`
    /// predicates. See [`CrossfadeCoordinator`].
    crossfade: CrossfadeCoordinator,
    /// Transport-fade settings mirror (M5): pause/stop ramp enables +
    /// durations. See [`FadeCoordinator`].
    fade: FadeCoordinator,
    /// Set by `finalize_crossfade_engine` so the completion path can label the
    /// "Now Playing" log line `crossfade` vs `gapless` (both reach the engine
    /// "already playing" branch). Read-and-reset via
    /// `take_last_transition_was_crossfade`.
    last_transition_was_crossfade: bool,

    // ---- Gapless transition state ----
    /// Transition info written by the decode loop, consumed by the engine.
    gapless_transition_info: Arc<tokio::sync::Mutex<Option<GaplessTransitionInfo>>>,

    /// Raw ICY-metadata parsed by IcyMetadataReader
    live_icy_metadata: LiveStringSlot,

    /// Extracted stream codec based on Symphonia probing (e.g. mp3, aac)
    live_codec_name: LiveStringSlot,
}

impl CustomAudioEngine {
    pub fn new() -> Self {
        let live_icy_metadata = LiveStringSlot::new();
        Self {
            source: String::new(),
            playing: false,
            paused: false,
            position: 0,
            duration: 0,
            volume: 1.0,
            decoder: Arc::new(tokio::sync::Mutex::new(AudioDecoder::new(
                live_icy_metadata.clone_arc(),
            ))),
            current_format: AudioFormat::invalid(),
            next_format: AudioFormat::invalid(),
            next_source: String::new(),
            pending_start_ms: None,
            renderer: Arc::new(PlMutex::new(AudioRenderer::new())),
            state: PlaybackState::Stopped,
            decode_loop: DecodeLoopHandle::new(),
            gapless: Arc::new(tokio::sync::Mutex::new(GaplessSlot::new())),
            completion_callback: None,
            seeking: Arc::new(AtomicBool::new(false)),
            render_thread: None,
            render_running: Arc::new(AtomicBool::new(false)),
            live_sample_rate: Arc::new(AtomicU32::new(0)),
            channels: DecodeLoopChannels::new(),
            crossfade: CrossfadeCoordinator::new(),
            fade: FadeCoordinator::new(),
            last_transition_was_crossfade: false,
            gapless_transition_info: Arc::new(tokio::sync::Mutex::new(None)),
            live_icy_metadata,
            live_codec_name: LiveStringSlot::new(),
        }
    }

    /// Get current source URL
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Set source URL
    pub async fn set_source(&mut self, source: String, expected_duration_ms: Option<u64>) {
        trace!(
            " AudioEngine: set_source called with: {}",
            redact_subsonic_url(&source)
        );
        // Any (re)load intent invalidates a staged pulled-queue start offset —
        // cleared BEFORE the same-source early return so a stale offset can
        // never survive a reload of the track it was armed for.
        self.pending_start_ms = None;
        if self.source == source {
            trace!(" AudioEngine: source unchanged, returning early");
            return;
        }

        if self.playing || self.paused {
            trace!(" AudioEngine: stopping current playback before changing source");
            // M6 radio→queue edge: leaving a PLAYING radio stream (the
            // outgoing is radio iff `stream_is_infinite` — still reflecting
            // the current stream here, before the new decode loop stores its
            // own probe) is the return half of "Fade Radio Switches": fade
            // the radio out, then arm the first-audio fade-in for the stream
            // the new source will build. Everything else keeps the instant
            // internal stop: track-change fades are M7's domain (its own
            // mode + duration), not "Fade on Stop" and not this flag.
            let radio_switch_fade = self.fade.fade_radio_transitions
                && self.playing
                && !self.paused
                && self.channels.stream_is_infinite.load(Ordering::Acquire);
            if radio_switch_fade {
                self.run_bounded_out_fade(crate::audio::renderer::RADIO_SWITCH_FADE_MS)
                    .await;
            }
            self.stop_without_fade().await;
            if radio_switch_fade {
                // AFTER the teardown — renderer.stop() clears the request.
                self.renderer.lock().request_switch_fade_in();
            }
        }

        // CRITICAL FIX: Reset fields *BEFORE* creating AudioDecoder.
        // During `AudioDecoder::new`, Symphonia's probe reads the first chunk of the stream synchronously.
        // If this stream contains ICY metadata, the callback fires during `new()` and populates `live_icy_metadata`.
        // If we reset this to `None` after `new()`, we will permanently discard the first stream title!
        self.channels.live_bitrate.store(0, Ordering::Relaxed);
        self.live_sample_rate.store(0, Ordering::Relaxed);
        self.channels.decoder_eof.store(false, Ordering::Release);
        self.live_icy_metadata.reset();
        // Non-blocking like the icy_metadata reset above: stale codec data
        // is acceptable here, and `set_source` must not block on UI readers.
        self.live_codec_name.reset();

        trace!(" AudioEngine: creating fresh decoder for new source");
        let mut fresh_decoder = AudioDecoder::new(self.live_icy_metadata.clone_arc());
        fresh_decoder.set_expected_duration_ms(expected_duration_ms);
        self.decoder = Arc::new(tokio::sync::Mutex::new(fresh_decoder));

        self.duration = 0;
        self.position = 0;

        self.source = source;
        self.channels.source_generation.bump_for_user_action();
        trace!(" AudioEngine: source set successfully");
    }

    /// Arm a one-shot start offset (ms) for the next fresh `play()` of the
    /// just-staged source (see the `pending_start_ms` field doc). Callers
    /// stage via `load_track_with_rg` FIRST — `set_source` clears any stale
    /// offset, so the arm is always scoped to exactly one staged track.
    pub fn set_pending_start_ms(&mut self, position_ms: u64) {
        self.pending_start_ms = Some(position_ms);
    }

    /// Get current parsed ICY-metadata from the stream buffer
    pub fn live_icy_metadata(&self) -> Option<String> {
        self.live_icy_metadata.get()
    }

    /// True when the engine's current source is exactly `url` (the string last
    /// handed to [`Self::set_source`], set at the END of that method). Used by
    /// the UI to verify that live ICY metadata belongs to the station it thinks
    /// is playing: during a station switch `active_playback` flips to the new
    /// station synchronously, but `set_source` runs async after `stop()`, so for
    /// a window the engine is still streaming — and reporting ICY for — the
    /// PREVIOUS source. Pairing that stale ICY with the new station would
    /// misattribute its now-playing art/title.
    pub fn is_playing_source(&self, url: &str) -> bool {
        self.source == url
    }

    /// Get current live codec name
    pub fn live_codec(&self) -> Option<String> {
        self.live_codec_name.get()
    }

    /// Get playing state
    pub fn playing(&self) -> bool {
        self.playing
    }

    /// Get position (milliseconds)
    /// Reads from renderer if playing, otherwise returns stored position
    pub fn position(&self) -> u64 {
        if self.playing && !self.paused {
            let renderer = self.renderer.lock();
            renderer.position()
        } else {
            self.position
        }
    }

    /// Get duration (milliseconds)
    pub fn duration(&self) -> u64 {
        self.duration
    }

    /// Get volume (0.0-1.0)
    pub fn volume(&self) -> f64 {
        self.volume
    }

    /// Set volume (0.0-1.0)
    pub fn set_volume(&mut self, volume: f64) {
        self.volume = volume.clamp(0.0, 1.0);

        // Apply volume to renderer
        let mut renderer = self.renderer.lock();
        renderer.set_volume(self.volume);
    }

    /// Play
    pub async fn play(&mut self) -> Result<()> {
        debug!(
            "🎵 AudioEngine: play() called, source: '{}', playing: {}, paused: {}",
            redact_subsonic_url(&self.source),
            self.playing,
            self.paused
        );
        if self.source.is_empty() {
            trace!(" AudioEngine: ERROR - cannot play, source is empty");
            anyhow::bail!("Cannot play - source is empty");
        }

        if self.playing && !self.paused {
            // Check if a gapless transition happened in the decode loop.
            // If so, consume the transition info to update engine metadata
            // (source, duration, format). The decode loop already swapped the
            // decoder and the stream is still feeding data — no restart needed.
            self.consume_gapless_transition().await;
            trace!(" AudioEngine: already playing, returning (gapless info consumed if pending)");
            return Ok(());
        }

        if self.paused {
            // Resume from pause
            self.paused = false;
            self.playing = true;
            {
                let mut renderer = self.renderer.lock();
                renderer.start();
            } // renderer guard dropped before .await
            self.state = PlaybackState::Playing;
            // Restart the decoding loop so new buffers are produced after resume
            self.start_decoding_loop();
            // Restart render thread
            self.start_render_thread();
            return Ok(());
        }

        // Start new playback
        trace!(" AudioEngine: starting new playback");
        // Ungate any prepared slot for this new track. Decoder ownership
        // stays so a concurrent prep is not silently dropped.
        self.gapless.lock().await.prepared = false;
        let mut decoder = self.decoder.lock().await;
        if !decoder.is_initialized() {
            trace!(" AudioEngine: decoder not initialized, initializing with source");
            match decoder.init(&self.source).await {
                Ok(()) => {
                    debug!(
                        "🎵 AudioEngine: decoder initialized successfully, duration: {}",
                        decoder.duration()
                    );
                    self.duration = decoder.duration();
                    self.live_codec_name.set(decoder.live_codec());
                }
                Err(e) => {
                    error!(" AudioEngine: decoder initialization FAILED: {:?}", e);
                    return Err(e);
                }
            }
        } else {
            trace!(" AudioEngine: decoder already initialized, seeking to start");
            // Seek back to the beginning for replay
            if !decoder.seek(0) {
                trace!(" AudioEngine: seek to start failed");
            } else {
                trace!(" AudioEngine: seek to start completed");
            }
            // CRITICAL: Restore duration from decoder (may have been cleared by stop())
            self.duration = decoder.duration();
            trace!(" AudioEngine: duration restored: {}", self.duration);
        }

        // One-shot pulled-queue start offset (see `pending_start_ms`): seek
        // the just-initialized decoder BEFORE the renderer starts, so a
        // paused-pull Play resumes mid-song with no position-0 audio at all.
        // Clamped to the real duration, mirroring `seek()`.
        if let Some(pending_ms) = self.pending_start_ms.take() {
            let target = pending_ms.min(self.duration);
            if target > 0 {
                if decoder.seek(target) {
                    self.position = target;
                    debug!("🎵 AudioEngine: pulled-queue start offset applied: {target}ms");
                } else {
                    warn!("Pulled-queue start offset seek to {target}ms failed; starting at 0");
                }
            }
        }

        // Initialize renderer with format (only if needed)
        self.current_format = decoder.format().clone();
        self.live_sample_rate
            .store(self.current_format.sample_rate(), Ordering::Relaxed);
        trace!(" AudioEngine: format set: {:?}", self.current_format);
        drop(decoder);

        {
            let mut renderer = self.renderer.lock();

            let needs_init = !renderer.format().is_valid()
                || renderer.format() != &self.current_format
                || !renderer.has_primary_stream();

            if needs_init {
                trace!(" AudioEngine: initializing renderer (format changed or first init)");
                let init_result = renderer.init(&self.current_format, false, None);
                match init_result {
                    Ok(_) => trace!(" AudioEngine: renderer initialized successfully"),
                    Err(e) => {
                        trace!(" AudioEngine: renderer initialization failed: {:?}", e);
                        return Err(e);
                    }
                }
            } else {
                trace!(
                    " AudioEngine: renderer already initialized with correct format, skipping init"
                );
            }

            // Apply current volume to renderer
            renderer.set_volume(self.volume);

            // Set playing state BEFORE starting decoding
            self.playing = true;
            trace!(" AudioEngine: set playing state to true");
            self.paused = false;
            self.state = PlaybackState::Playing;
            trace!(" AudioEngine: set paused=false, state=Playing");
        } // Drop renderer lock before acquiring decoder lock

        // PREBUFFERING: Queue initial buffers before starting renderer
        // This prevents buffer starvation at playback start
        // (`PLAY_PREBUFFER_COUNT` is module-scope — see top of file.)
        trace!(
            " AudioEngine: prebuffering {} buffers before playback",
            PLAY_PREBUFFER_COUNT
        );

        {
            let mut decoder_guard = self.decoder.lock().await;
            for i in 0..PLAY_PREBUFFER_COUNT {
                // Use block_in_place for blocking HTTP I/O
                match tokio::task::block_in_place(|| decode_one_chunk(&mut decoder_guard)) {
                    Some(samples) => {
                        let mut renderer = self.renderer.lock();
                        renderer.write_samples(&samples);
                        drop(renderer);
                        trace!(
                            " AudioEngine: queued prebuffer {}/{}",
                            i + 1,
                            PLAY_PREBUFFER_COUNT
                        );
                    }
                    None => {
                        warn!(
                            "  AudioEngine: prebuffering stopped at {}/{} (no more data)",
                            i + 1,
                            PLAY_PREBUFFER_COUNT
                        );
                        break;
                    }
                }
            }
            drop(decoder_guard);
        }

        // Start rendering with buffers already queued
        {
            trace!(" AudioEngine: starting renderer");
            let mut renderer = self.renderer.lock();
            renderer.start();
            trace!(" AudioEngine: renderer started");
            // Renderer started, starting decoding loop
        }

        // Start decoding loop
        trace!(" AudioEngine: starting decoding loop");
        self.start_decoding_loop();
        trace!(" AudioEngine: decoding loop started");

        // Start dedicated render thread (decoupled from iced event loop)
        self.start_render_thread();
        trace!(" AudioEngine: render thread started");

        debug!(" AudioEngine: play() completed successfully");
        Ok(())
    }

    /// Start the decoding loop in a background task
    fn start_decoding_loop(&mut self) {
        let decoder = self.decoder.clone();
        let renderer = self.renderer.clone();
        // Same-identity clone of the decode-loop channels (never a fresh Arc).
        let ClonedDecodeLoopChannels {
            source_generation,
            decoder_eof,
            stream_is_infinite: stream_is_infinite_arc,
            crossfade_duration_shared,
            live_bitrate,
            skip_fade_pending,
            gap_offset_ms,
        } = self.channels.clone_for_decode_loop();

        // Gapless: pass next-track state so the decode loop can swap inline
        let gapless = self.gapless.clone();
        let completion_callback = self.completion_callback.clone();
        let gapless_info = self.gapless_transition_info.clone();
        let reconnect_url = self.source.clone();

        // Capture the renderer's consume-notify so the write-retry loop can
        // await it instead of busy-sleeping. The Arc is stable across seek /
        // stream-recreation, so a single capture here is always valid.
        let consumed_notify = renderer.lock().consumed_notify().clone();

        // Clear EOF flag — this decoder is starting fresh
        self.channels.decoder_eof.store(false, Ordering::Release);

        // Increment decode generation — invalidates any previous decode loop.
        // Each loop captures its generation at spawn time and exits when
        // the generation no longer matches (i.e. a newer loop superseded it).
        let my_gen = self.decode_loop.supersede();
        let decode_gen = self.decode_loop.clone();

        // Spawn decoding task
        tokio::spawn(async move {
            let mut loop_count = 0;
            let mut backpressure_active = false;
            // Cached stream frame_rate (sample_rate * channels), captured after
            // each decode so the backpressure watermarks — computed before the
            // decoder lock — stay time-based. 0 until the first decoded buffer.
            let mut frame_rate: u32 = 0;
            let mut stream_type_checked = false;
            let mut stream_is_infinite_cached = false;
            let mut radio_music_jitter_filled = false;
            let mut last_heartbeat = std::time::Instant::now();
            let mut empty_buffer_count: u64 = 0;

            'decode_loop: loop {
                loop_count += 1;

                // Heartbeat every 10 seconds to confirm loop is still running
                if last_heartbeat.elapsed() > std::time::Duration::from_secs(10) {
                    tracing::trace!(
                        "💓 [DECODE LOOP] Heartbeat: {} iterations, still running",
                        loop_count
                    );
                    last_heartbeat = std::time::Instant::now();
                }

                // Check if this loop has been superseded by a newer one.
                // Uses a lock-free atomic check instead of a mutex.
                if decode_gen.current() != my_gen {
                    tracing::trace!(
                        "🔄 [DECODE LOOP] Exiting - generation superseded (my={}, current={}) after {} iterations",
                        my_gen,
                        decode_gen.current(),
                        loop_count
                    );
                    break;
                }

                // BACKPRESSURE CHECK: If ring buffer is full, wait for it to drain
                let buffer_count = {
                    let renderer_guard = renderer.lock();
                    renderer_guard.buffer_count() // interleaved samples in the ring
                }; // renderer lock dropped here, before any .await

                // Time-based watermarks: scale with the stream's frame_rate so the
                // cushion is a constant duration at every sample rate, and grow
                // with crossfade duration so the buffer can hold a full fade-out.
                // Radio never backpressures — see `backpressure_step`.
                let cf_ms = crossfade_duration_shared.load(Ordering::Relaxed);
                match backpressure_step(
                    "DECODE LOOP",
                    buffer_count,
                    frame_rate,
                    cf_ms,
                    stream_is_infinite_cached,
                    &mut backpressure_active,
                ) {
                    BackpressureAction::Sleep(duration) => {
                        tokio::time::sleep(duration).await;
                        continue;
                    }
                    BackpressureAction::Proceed => {}
                }

                // Try to acquire decoder lock with a short timeout
                // This allows the loop to check the flag frequently even if lock is contested
                let mut decoder_guard = decoder.lock().await;

                // CRITICAL: Check generation AGAIN after acquiring lock, before doing I/O!
                // If a new loop started while we were waiting for the lock,
                // we release the lock immediately instead of starting a long HTTP read.
                if decode_gen.current() != my_gen {
                    tracing::trace!("🔄 [DECODE LOOP] Exiting after lock - generation superseded");
                    drop(decoder_guard);
                    break;
                }

                if !decoder_guard.is_initialized() {
                    drop(decoder_guard);
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                    continue;
                }

                // Cache the stream frame_rate for the next iteration's (pre-lock)
                // backpressure watermark computation.
                frame_rate = decoder_guard.format().frame_rate();

                // Decode one chunk - this is where HTTP I/O happens
                // CRITICAL: Use block_in_place to prevent blocking the async runtime!
                // The decoder uses reqwest::blocking::Client for HTTP which would otherwise
                // starve the tokio runtime, causing timeouts and deadlocks.
                let chunk = tokio::task::block_in_place(|| decode_one_chunk(&mut decoder_guard));

                // Propagate live bitrate from decoder to engine atomic
                let current_bitrate = decoder_guard.live_bitrate();
                if current_bitrate > 0 {
                    live_bitrate.store(current_bitrate, Ordering::Relaxed);
                }

                // Check stream type once and cache it so we can safely disable backpressure.
                if !stream_type_checked {
                    stream_is_infinite_cached = decoder_guard.is_infinite_stream();
                    stream_type_checked = true;
                    // Publish to the renderer so its mid-track rebuffer skips radio.
                    stream_is_infinite_arc.store(stream_is_infinite_cached, Ordering::Release);
                }

                let is_eof = decoder_guard.is_eof();
                let is_infinite = stream_is_infinite_cached;

                if let Some(samples) = chunk {
                    // Release decoder lock before acquiring renderer lock
                    drop(decoder_guard);

                    let mut samples_to_write = samples.as_slice();
                    while !samples_to_write.is_empty() {
                        if decode_gen.current() != my_gen {
                            break;
                        }

                        let written = {
                            let mut renderer_guard = renderer.lock();
                            renderer_guard.write_samples(samples_to_write)
                        };

                        if written < samples_to_write.len() {
                            samples_to_write = &samples_to_write[written..];
                            // Ring buffer is full (or partially so). Instead of busy-sleeping
                            // every 5 ms, wait for the renderer to consume samples.
                            //
                            // When playing:  StreamingSource::next() fires consumed_notify every
                            //   ~512 samples (~5 ms at 48 kHz stereo) → we wake up promptly.
                            // When paused:   renderer emits silence without consuming the ring
                            //   buffer → consumed_notify never fires → timeout elapses → we
                            //   re-check generation and sleep again. 500 ms per cycle ≈ 2
                            //   wake-ups/s instead of 200 wake-ups/s — no more livelock.
                            // On supersede:  generation check fires immediately after the timeout
                            //   (or after a spurious wake), bounding exit latency to ≤500 ms.
                            let _ = tokio::time::timeout(
                                tokio::time::Duration::from_millis(500),
                                consumed_notify.notified(),
                            )
                            .await;
                        } else {
                            break;
                        }
                    }

                    radio_jitter_prebuffer_step(
                        &renderer,
                        frame_rate,
                        is_infinite,
                        &mut radio_music_jitter_filled,
                    );

                    log_stream_health(
                        &renderer,
                        frame_rate,
                        cf_ms,
                        &mut last_heartbeat,
                        &mut empty_buffer_count,
                    );
                } else if is_eof {
                    // =========================================================
                    // RADIO STREAM EOF: connection dropped or server closed.
                    // Skip gapless transition — radio has no "next track".
                    // =========================================================
                    if is_infinite {
                        match radio_reconnect_loop(
                            &decoder,
                            decoder_guard,
                            &decode_gen,
                            my_gen,
                            &decoder_eof,
                            &reconnect_url,
                        )
                        .await
                        {
                            RadioReconnectOutcome::Reconnected => {
                                // Reset stream-type check so jitter buffer and
                                // backpressure caching are re-evaluated
                                stream_type_checked = false;
                                radio_music_jitter_filled = false;
                                continue 'decode_loop;
                            }
                            // Either exhausted (EOF already stored by the helper)
                            // or generation superseded (no EOF store) — exit.
                            RadioReconnectOutcome::GaveUp | RadioReconnectOutcome::Superseded => {
                                break;
                            }
                        }
                    }

                    // =========================================================
                    // GAPLESS TRANSITION: try to swap the next decoder inline
                    // =========================================================
                    let current_format = decoder_guard.format().clone();
                    drop(decoder_guard); // release primary decoder lock

                    // M8 positive "Gap / Overlap Trim": inject the pending
                    // per-transition silence between the outgoing's last
                    // sample and whatever follows — BEFORE the swap attempt
                    // so the gap also materializes on the no-swap path (the
                    // ring drains `gap` ms later, delaying the completion
                    // gate / fresh load by the same amount). Stands down
                    // when a crossfade or a skip-fade plan owns the
                    // transition.
                    inject_transition_gap(
                        &renderer,
                        &gap_offset_ms,
                        &skip_fade_pending,
                        &source_generation,
                        &decode_gen,
                        my_gen,
                        frame_rate,
                        &consumed_notify,
                    )
                    .await;

                    let swap_outcome = try_gapless_swap(
                        &decoder,
                        &renderer,
                        &gapless,
                        &gapless_info,
                        &source_generation,
                        &completion_callback,
                        &current_format,
                        &skip_fade_pending,
                    )
                    .await;

                    if swap_outcome == GaplessSwapOutcome::Swapped {
                        // Successfully swapped — the new track's ring starts empty,
                        // so clear the loop-local backpressure latch (it would
                        // otherwise inflict an unwarranted sleep on the fresh track),
                        // then continue the decode loop with the new decoder (no gap!)
                        backpressure_active = false;
                        continue;
                    }

                    // No gapless possible — signal EOF and exit. Re-check the
                    // generation first: read_buffer can block for seconds, so
                    // this loop may have been superseded mid-iteration (e.g.
                    // crossfade finalize promoting the next track) — a stale
                    // EOF stored here would be attributed to the new track
                    // and end it moments after its transition.
                    if decode_gen.current() == my_gen {
                        decoder_eof.store(true, Ordering::Release);
                        tracing::debug!("📭 [DECODE LOOP] Decoder EOF — signaling renderer");
                    } else {
                        tracing::debug!(
                            "📭 [DECODE LOOP] Decoder EOF after supersession — discarding signal"
                        );
                    }
                    break;
                } else {
                    // Release decoder lock before sleeping
                    drop(decoder_guard);

                    // Temporary empty buffer (network stall, seek refill, etc.)
                    empty_buffer_count += 1;
                    tracing::trace!(
                        "📭 [DECODE LOOP] Empty/invalid buffer received, waiting for decoder"
                    );
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                    continue;
                }
            }
        });
    }

    /// Pause
    pub fn pause(&mut self) {
        if !self.playing {
            return;
        }

        // Capture current position from renderer before pausing
        // This ensures position() returns the correct paused position
        {
            let renderer = self.renderer.lock();
            self.position = renderer.position();
        }

        self.paused = true;
        self.playing = false;
        {
            // M5: with "Fade on Pause / Resume" on, hand the renderer the
            // out-ramp — engine state flips to Paused immediately (position
            // captured above, UI reads paused at once) while the render
            // thread ramps the stream down and applies the real stream-level
            // pause at completion. When the ramp can't engage (disabled,
            // bit-perfect stream, live crossfade, drained ring), fall back to
            // the instant pause exactly as before.
            let mut renderer = self.renderer.lock();
            if !renderer.begin_pause_fade() {
                renderer.pause();
            }
        }
        self.state = PlaybackState::Paused;
    }

    /// Run the M5 stop out-ramp, bounded, BEFORE any teardown — the render
    /// thread (still alive here) is what drives the ramp. Skipped entirely
    /// when the fade is disabled, when already paused (the stream has been
    /// silent since the pause; a ramp has nothing audible to fade and a wait
    /// would just burn its timeout), or when the renderer refuses to engage
    /// (bit-perfect stream, live crossfade, drained ring).
    ///
    /// The bounded wait polls `transport_fade_idle()` at 10 ms; the deadline
    /// is the (clamped ≤ 500 ms) ramp length + a 250 ms margin, so a stuck
    /// ramp can only delay — never wedge — teardown (the logout/redb-relock
    /// path runs through here).
    async fn run_stop_fade(&mut self) {
        if !self.fade.fade_on_stop || self.paused || !self.playing {
            return;
        }
        self.run_bounded_out_fade(u64::from(self.fade.fade_stop_ms))
            .await;
    }

    /// The shared bounded out-ramp body of the M5 stop fade and the M6
    /// radio-switch fade: engage the renderer's stop ramp (which may refuse
    /// — bit-perfect stream, live crossfade, drained ring — in which case
    /// this returns immediately) and poll `transport_fade_idle()` at 10 ms
    /// until it completes or the deadline (`dur_ms` + 250 ms margin) trips.
    /// Callers gate on their own enable + playing/paused state.
    async fn run_bounded_out_fade(&mut self, dur_ms: u64) {
        let engaged = { self.renderer.lock().begin_stop_fade(dur_ms) };
        if !engaged {
            return;
        }
        let deadline = std::time::Duration::from_millis(dur_ms + 250);
        let started = std::time::Instant::now();
        while started.elapsed() < deadline {
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            if self.renderer.lock().transport_fade_idle() {
                debug!("🎚️ [ENGINE] out-ramp completed in {:?}", started.elapsed());
                return;
            }
        }
        warn!(
            "🎚️ [ENGINE] out-ramp did not complete within {:?} — proceeding with teardown",
            deadline
        );
    }

    /// Stop
    ///
    /// Runs the M5 stop out-ramp first (see [`Self::run_stop_fade`]) — the
    /// ramp needs the render thread, which the teardown below joins.
    pub async fn stop(&mut self) {
        if !self.playing && !self.paused {
            return;
        }

        self.run_stop_fade().await;
        self.stop_without_fade().await;
    }

    /// Stop for a RADIO SWITCH (the UI radio-start paths: play station /
    /// cycle station). With "Fade Radio Switches" enabled and something
    /// audibly playing, runs the fixed `RADIO_SWITCH_FADE_MS` out-ramp and
    /// arms the renderer's first-audio fade-in for the stream the upcoming
    /// `set_source` + `play` will build. Otherwise delegates to
    /// [`Self::stop`] — including its "Fade on Stop" semantics — so the
    /// switched-off path is byte-identical to the historical explicit stop
    /// these call sites used.
    pub async fn stop_for_radio_switch(&mut self) {
        if self.fade.fade_radio_transitions && self.playing && !self.paused {
            self.run_bounded_out_fade(crate::audio::renderer::RADIO_SWITCH_FADE_MS)
                .await;
            self.stop_without_fade().await;
            // Armed AFTER the teardown (renderer.stop() clears it) so the
            // next fresh start() — the radio stream `set_source` + `play`
            // are about to build — fades in from its first real sample.
            self.renderer.lock().request_switch_fade_in();
        } else {
            self.stop().await;
        }
    }

    /// The teardown body of [`Self::stop`], with no transport fade. This is
    /// also the variant `set_source` uses for its internal stop on a track
    /// change: skip transitions are M7's domain ("Fade on Skip" gets its own
    /// mode + duration there), so "Fade on Stop" must not leak a ramp into
    /// every manual track switch.
    async fn stop_without_fade(&mut self) {
        if !self.playing && !self.paused {
            return;
        }

        // Cancel any active crossfade
        self.cancel_crossfade().await;

        // Unconditionally disarm renderer's crossfade trigger.
        // cancel_crossfade() skips when phase is Idle, but the renderer
        // may still be armed from prepare_next_for_gapless().
        {
            self.renderer.lock().disarm_crossfade();
        }

        // Stop decoding loop by advancing the generation counter.
        // Any running loop will see the mismatch and exit.
        self.decode_loop.supersede();

        // Stop render thread
        self.stop_render_thread();

        self.reset_next_track().await;
        {
            let mut renderer = self.renderer.lock();
            renderer.stop();
        }

        self.playing = false;
        self.paused = false;
        self.position = 0;
        self.duration = 0;
        self.channels.live_bitrate.store(0, Ordering::Relaxed);
        self.live_sample_rate.store(0, Ordering::Relaxed);
        self.state = PlaybackState::Stopped;
    }

    /// Seek to position (milliseconds)
    ///
    /// Stops the decoding loop temporarily, performs the seek, then restarts.
    /// This ensures the decoder lock is available for seeking.
    pub async fn seek(&mut self, position_ms: u64) {
        use tracing::{debug, trace, warn};

        let seek_start = std::time::Instant::now();
        debug!(
            "🔍 [SEEK] Starting seek to {}ms (duration={}ms)",
            position_ms, self.duration
        );

        if self.duration == 0 {
            debug!("🔍 [SEEK] Aborting - duration is 0");
            return;
        }

        // CRITICAL FIX: Stop the decoding loop FIRST, before trying to acquire decoder lock!
        // The decoding loop holds the decoder lock while doing HTTP I/O (which can take 20+ seconds).
        // If we try to acquire the lock before stopping the loop, we'll block for the entire I/O duration.
        trace!("🔍 [SEEK] Stopping decoding loop FIRST");

        // Cancel any active crossfade before seeking
        self.cancel_crossfade().await;

        // Clear EOF — decoder will restart from seek position
        self.channels.decoder_eof.store(false, Ordering::Release);

        self.decode_loop.supersede();

        // Give the decoding loop time to notice the flag and release the lock
        trace!("🔍 [SEEK] Waiting for decoding loop to release lock");
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;

        // NOW we can safely acquire the lock for the init check
        let decoder_initialized = {
            trace!("🔍 [SEEK] Acquiring decoder lock for init check...");
            let lock_start = std::time::Instant::now();
            let decoder = self.decoder.lock().await;
            trace!(
                "🔍 [SEEK] Decoder lock acquired in {:?}",
                lock_start.elapsed()
            );
            decoder.is_initialized()
        };

        if !decoder_initialized {
            debug!("🔍 [SEEK] Aborting - decoder not initialized");
            // Restart the decoding loop (start_decoding_loop handles generation)
            self.start_decoding_loop();
            return;
        }

        // Set seeking flag to prevent EOF detection during seek
        trace!("🔍 [SEEK] Setting seeking flag");
        self.seeking.store(true, Ordering::Release);

        let pos = position_ms.min(self.duration);

        // Acquire the async decoder lock natively, then use block_in_place
        // for the blocking HTTP I/O (RangeHttpReader uses reqwest::blocking).
        // This matches the proven pattern in play() and the decode loop.
        trace!("🔍 [SEEK] Acquiring decoder lock...");
        let lock_start = std::time::Instant::now();
        let mut decoder = self.decoder.lock().await;
        trace!(
            "🔍 [SEEK] Decoder lock acquired in {:?}",
            lock_start.elapsed()
        );

        let blocking_start = std::time::Instant::now();
        let seek_result = tokio::task::block_in_place(|| {
            trace!("🔍 [SEEK] Calling decoder.seek({})", pos);
            let seek_op_start = std::time::Instant::now();
            let seek_ok = decoder.seek(pos);
            debug!(
                "🔍 [SEEK] decoder.seek() completed in {:?}, success={}",
                seek_op_start.elapsed(),
                seek_ok
            );

            if seek_ok {
                trace!("🔍 [SEEK] Acquiring renderer lock...");
                let mut renderer = self.renderer.lock();
                renderer.seek(pos);

                // PREBUFFERING: Queue initial buffers after seek
                // (`SEEK_PREBUFFER_COUNT` is module-scope — see top of file.)
                trace!("🔍 [SEEK] Prebuffering {} buffers", SEEK_PREBUFFER_COUNT);

                for i in 0..SEEK_PREBUFFER_COUNT {
                    // Bare call — already inside the outer block_in_place closure.
                    if let Some(samples) = decode_one_chunk(&mut decoder) {
                        renderer.write_samples(&samples);
                        trace!(
                            "🔍 [SEEK] Queued prebuffer {}/{}",
                            i + 1,
                            SEEK_PREBUFFER_COUNT
                        );
                    } else {
                        trace!(
                            "🔍 [SEEK] Prebuffering stopped at {}/{} (no more data)",
                            i + 1,
                            SEEK_PREBUFFER_COUNT
                        );
                        break;
                    }
                }
            }

            seek_ok
        });
        drop(decoder);
        debug!(
            "🔍 [SEEK] Seek + prebuffer completed in {:?}, success={}",
            blocking_start.elapsed(),
            seek_result
        );

        if seek_result {
            self.position = pos;
        } else {
            warn!("🔍 [SEEK] Seek operation failed!");
        }

        // Restart the decoding loop
        trace!("🔍 [SEEK] Restarting decoding loop");
        self.start_decoding_loop();

        // Clear seeking flag
        trace!("🔍 [SEEK] Clearing seeking flag");
        self.seeking.store(false, Ordering::Release);

        // Re-arm the crossfade against the already-prepared next track. The seek
        // cancelled the in-flight crossfade above (its timing was stale), but the
        // prepared decoder is still valid. Without this, seeking near the track
        // end disarms the crossfade and the transition falls back to a gapless
        // hard-cut — and a single user seek often arrives as two seek events, so
        // a second seek cancels the arm a fresh prep just set while no new prep
        // re-arms (the slot is already prepared). Re-arming here closes that race.
        self.rearm_crossfade_if_prepared().await;

        debug!(
            "🔍 [SEEK] Seek completed in {:?} total",
            seek_start.elapsed()
        );
    }

    /// Arm the renderer crossfade trigger from the engine's current crossfade
    /// settings against an incoming track of `incoming_duration_ms`.
    ///
    /// Reads `crossfade_duration_ms` / `next_format` / `duration` (the OUTGOING
    /// track's duration) from `&self` and hands them, with `incoming_duration_ms`
    /// last, to [`AudioRenderer::arm_crossfade`] in a single statement so the
    /// sync `parking_lot` guard is born and dropped without ever straddling an
    /// `.await`. Callers keep both the eligibility (`crossfade_eligible`) and the
    /// non-zero-duration guards at their own sites; the one gate owned HERE is
    /// the per-transition policy suppress flag (M4), because this helper is the
    /// single funnel for BOTH arm sites — `store_prepared_decoder` and the
    /// rearm-after-seek `rearm_crossfade_if_prepared` (gating only at the store
    /// would let a seek re-arm a suppressed transition). A suppressed arm also
    /// disarms any stale Armed state so the renderer can never disagree with
    /// the flag (invariant 4's dual-site agreement, extended to the policy
    /// gate). Does not touch `self.gapless`. The renderer's own
    /// `crossfade_blocked` / min-duration gates decide whether the arm
    /// actually takes.
    fn arm_renderer_crossfade(&self, incoming_duration_ms: u64) {
        if self.crossfade.suppress_this_transition {
            debug!(
                "🔀 [ENGINE] Crossfade arm SUPPRESSED for this transition (policy: gapless join)"
            );
            self.renderer.lock().disarm_crossfade();
            return;
        }
        self.renderer.lock().arm_crossfade(
            // Effective = per-transition bar-snap override when staged (M8),
            // else the global setting.
            self.crossfade.effective_duration_ms(),
            &self.next_format,
            self.duration,
            incoming_duration_ms,
        );
    }

    /// Re-arm the renderer crossfade against an already-prepared next track,
    /// no-op when nothing is prepared, crossfade is ineligible, or the format
    /// pair can't crossfade (the renderer's `arm_crossfade` gate decides the
    /// last part). Mirrors the arm in [`Self::store_prepared_decoder`] but reads
    /// the incoming duration from the existing prepared slot rather than a fresh
    /// decoder, so a seek doesn't need a re-prep to restore the armed trigger.
    async fn rearm_crossfade_if_prepared(&mut self) {
        let prepared = {
            let slot = self.gapless.lock().await;
            if !slot.is_prepared() {
                return;
            }
            slot.decoder
                .as_ref()
                .map(|d| (d.duration(), d.format().clone(), slot.replay_gain.clone()))
        };
        let Some((incoming_duration, incoming_format, replay_gain)) = prepared else {
            return;
        };
        // Re-stage the slot's ReplayGain BEFORE the eligibility gate: the
        // finalize-time re-arm (M7 mid-fade store) runs AFTER
        // `finalize_crossfade` consumed the live blend's staged copy into
        // `current_replay_gain`, and the NON-crossfade gapless consumers
        // (inline swap adopt / EOF fallback) need the staged copy too —
        // exactly as the ordinary store path stages it regardless of
        // crossfade eligibility. Redundant-but-consistent on the seek-rearm
        // path (same value the store already staged).
        self.renderer
            .lock()
            .set_pending_crossfade_replay_gain(replay_gain);
        if !self.crossfade.crossfade_eligible() || self.crossfade.effective_duration_ms() == 0 {
            return;
        }
        // Re-derive the incoming format from the slot itself: the
        // finalize-time re-arm (M7 mid-fade store) runs AFTER
        // `finalize_crossfade_engine` cleared `next_format`, and the arm
        // reads that field. Redundant-but-consistent on the seek-rearm path.
        self.next_format = incoming_format;
        self.arm_renderer_crossfade(incoming_duration);
    }

    /// Atomic three-step: stash ReplayGain → set source. The caller still
    /// invokes `play()` afterward, but the RG-stash + source-update pair is
    /// uncuttable. Replaces the historical `set_pending_replay_gain` +
    /// `load_track` / `set_source` pairing in `PlaybackController`.
    pub async fn load_track_with_rg(
        &mut self,
        url: &str,
        rg: Option<crate::types::song::ReplayGain>,
        expected_duration_ms: Option<u64>,
    ) {
        self.renderer.lock().set_pending_replay_gain(rg);
        self.set_source(url.to_string(), expected_duration_ms).await;
    }

    /// Apply the controller's per-transition verdicts (M4 suppress + M8
    /// bar-snap override + M8 gap offset) to the coordinator/channels. Called
    /// from BOTH `store_prepared_decoder` branches, after any internal
    /// `reset_next_track`; the counterpart clears live in `reset_next_track`.
    fn apply_transition_directives(&mut self, directives: &PreparedTransitionDirectives) {
        self.crossfade.suppress_this_transition = directives.suppress_crossfade;
        self.crossfade.duration_override_ms = directives.duration_override_ms;
        // Invariant 9: the decode-loop watermark mirror must cover the
        // duration that will actually play — a +1-bar snap needs the bigger
        // cushion BEFORE the fade fires. Stored unconditionally at the
        // EFFECTIVE value (override or global) so the mid-blend store branch
        // — which skips the internal `reset_next_track` and therefore its
        // restore — can never leave the PREVIOUS transition's override
        // leaking under a no-override prep. `reset_next_track` restores the
        // global.
        self.channels
            .crossfade_duration_shared
            .store(self.crossfade.effective_duration_ms(), Ordering::Relaxed);
        self.channels
            .gap_offset_ms
            .store(directives.gap_offset_ms, Ordering::Release);
    }

    /// Store an already-initialized decoder for gapless playback.
    /// This is the preferred method for gapless prep because it doesn't block
    /// the engine lock during network I/O, allowing the visualizer to continue.
    ///
    /// `directives` carries the controller's per-transition verdicts
    /// (computed at prep time from `Song` metadata the engine boundary
    /// doesn't have): the M4 crossfade-vs-gapless suppress flag, the M8
    /// bar-snap duration override, and the M8 gap-offset silence length. All
    /// three follow the same lifecycle — applied AFTER this method's internal
    /// `reset_next_track`, cleared BY `reset_next_track`.
    ///
    /// Caller should:
    /// 1. Create and init the decoder OUTSIDE of engine lock (do the download)
    /// 2. Call this method briefly to store the ready decoder
    pub async fn store_prepared_decoder(
        &mut self,
        decoder: AudioDecoder,
        url: String,
        replay_gain: Option<crate::types::song::ReplayGain>,
        directives: PreparedTransitionDirectives,
    ) {
        // Check if we should store this decoder
        if url.is_empty() || url == self.source {
            return;
        }

        // M7: a prep landing while a blend is LIVE — or while a planned
        // manual skip's build window is open — must not reset/cancel/arm.
        // The manual-skip fade exposes both windows for real: the cursor
        // advances at skip time, the UI's song-change re-opens its gapless
        // prep latch, and a prep for the track AFTER the skip target can
        // complete mid-fade (the internal `reset_next_track` below would
        // cancel the blend, restoring the outgoing while the queue already
        // moved on) or mid-BUILD (arming would let the position trigger fire
        // an auto blend against the already-advanced cursor before the
        // skip's own fire). Store the slot WITHOUT the reset and WITHOUT
        // arming (`arm_crossfade` would overwrite the Active variant —
        // Armed and Active are one enum); `finalize_crossfade_engine`
        // re-arms from the slot once the incoming is promoted.
        //
        // The prep's ReplayGain rides the SLOT here, never the renderer's
        // `pending_crossfade_replay_gain`: that staging slot is owned by the
        // LIVE (or about-to-fire) blend's incoming — finalize promotes it
        // into `current_replay_gain`, so overwriting it would hand the
        // promoted track the WRONG tags and leave the re-armed next blend
        // with none. `rearm_crossfade_if_prepared` stages it after the
        // promotion consumed the live copy.
        if self.crossfade.is_crossfade_live(&self.renderer) || self.skip_fade_window_pending() {
            debug!(
                "🔀 [GAPLESS] Prep landed mid-blend/mid-skip-window — storing without reset/arm"
            );
            self.next_format = decoder.format().clone();
            self.next_source = url;
            {
                let mut slot = self.gapless.lock().await;
                slot.decoder = Some(decoder);
                slot.source = self.next_source.clone();
                slot.prepared = true;
                slot.replay_gain = replay_gain;
            }
            self.apply_transition_directives(&directives);
            return;
        }

        // Only reset if we're actually going to store something new
        if self.next_source != url {
            self.reset_next_track().await;
        }

        // Per-transition policy verdicts (M4 suppress + M8 override/gap).
        // ORDERING IS LOAD-BEARING: applied AFTER the internal
        // `reset_next_track` above (which clears all three — setting earlier
        // would be silently wiped, losing a suppress verdict) and BEFORE the
        // `arm_renderer_crossfade` below (which gates on the flag and reads
        // the effective duration). Re-applied on EVERY store so a
        // re-preparation re-derives the verdicts in both directions.
        self.apply_transition_directives(&directives);

        self.next_format = decoder.format().clone();
        let incoming_duration = decoder.duration();
        self.next_source = url;
        {
            let mut slot = self.gapless.lock().await;
            slot.decoder = Some(decoder);
            slot.source = self.next_source.clone();
            slot.prepared = true;
            // Recorded on the slot too so every later slot consumer
            // (finalize/seek re-arm, EOF-fallback fire) can re-stage it —
            // e.g. after a cancel dropped the renderer's staged copy.
            slot.replay_gain = replay_gain.clone();
        }

        // Stash the incoming track's ReplayGain so the next crossfade
        // (or gapless transition) applies the right amplify factor.
        self.renderer
            .lock()
            .set_pending_crossfade_replay_gain(replay_gain);

        // Arm the renderer to trigger crossfade when the queue drains. Crossfade
        // fires when the user's Crossfade toggle is on (bit-perfect Off) OR under
        // bit-perfect Relaxed, which runs its own same-rate crossfade even though
        // its mutually-exclusive Crossfade toggle is off. The renderer's
        // `crossfade_blocked` gate then decides per-transition (Relaxed crossfades
        // only a same-format change; Strict, which never reaches here, would
        // hard-cut all).
        if self.crossfade.crossfade_eligible() && self.crossfade.effective_duration_ms() > 0 {
            self.arm_renderer_crossfade(incoming_duration);
        }
    }

    /// Consume gapless transition info that was set by the decode loop.
    /// Updates the engine's metadata (source, duration, format) to reflect
    /// the track that the decode loop has already swapped to.
    pub async fn consume_gapless_transition(&mut self) {
        let info = self.gapless_transition_info.lock().await.take();
        if let Some(info) = info {
            debug!(
                "🎵 [GAPLESS] Consuming transition: source={}, duration={}, format={:?}",
                redact_subsonic_url(&info.source),
                info.duration,
                info.format
            );
            self.source = info.source;
            self.duration = info.duration;
            self.position = 0;
            self.current_format = info.format;
            self.live_codec_name.set(info.codec);
            self.next_source.clear();
            self.gapless.lock().await.source.clear();
            self.live_sample_rate
                .store(self.current_format.sample_rate(), Ordering::Relaxed);
        }
    }

    // =========================================================================
    // Crossfade Engine API
    // =========================================================================

    /// Set crossfade enabled from settings.
    ///
    /// On a REAL change this also abandons any prepared/armed/in-flight
    /// transition via `reset_next_track`: the toggle flips
    /// `crossfade_eligible`, and the renderer's armed trigger fires on
    /// position alone — a blend armed under the old setting would start
    /// against an engine gate that now refuses (routing into the
    /// buffer-starvation wait) and orphan a silent incoming stream. Mirrors
    /// the `set_bit_perfect` contract below and the shuffle/repeat/consume
    /// mode-toggle contract. No-op when unchanged so a routine settings save
    /// (which re-applies every field) never disturbs an in-flight transition.
    ///
    /// `set_crossfade_duration` deliberately keeps its bare write: a duration
    /// change never flips eligibility (an armed trigger just fires once at
    /// the old offset), and cancelling a live blend on every slider step
    /// would hard-cut audio for a cosmetic knob.
    pub async fn set_crossfade_enabled(&mut self, enabled: bool) {
        let changed = self.crossfade.enabled != enabled;
        self.crossfade.enabled = enabled;
        if changed {
            self.reset_next_track().await;
        }
    }

    /// Set crossfade duration from settings (in seconds). While an M8
    /// bar-snap override is live the shared watermark mirror keeps the
    /// override (it describes the transition that will actually play);
    /// `reset_next_track` re-syncs the mirror to the new global when the
    /// override clears.
    pub fn set_crossfade_duration(&mut self, duration_secs: u32) {
        let ms = duration_secs as u64 * 1000;
        self.crossfade.duration_ms = ms;
        if self.crossfade.duration_override_ms.is_none() {
            self.channels
                .crossfade_duration_shared
                .store(ms, Ordering::Relaxed);
        }
    }

    /// Set the crossfade fade curve from settings — pushed to the renderer,
    /// which owns the curve (captured into the `Active` variant at
    /// `start_crossfade`). Like `set_crossfade_duration`, this is a bare
    /// write with no `reset_next_track`: a curve change never flips
    /// crossfade eligibility, and an in-flight fade keeps the curve it
    /// started with (the renderer's capture prevents mid-fade tearing), so
    /// cancelling a live blend would hard-cut audio for a cosmetic knob.
    pub fn set_crossfade_curve(&mut self, curve: crate::types::player_settings::CrossfadeCurve) {
        self.renderer.lock().set_crossfade_curve(curve);
    }

    /// Set the minimum-track-length crossfade floor from settings (seconds) —
    /// pushed to the renderer, which owns the enforcing copy at its
    /// `arm_crossfade` gate; the engine mirror feeds the controller's
    /// prep-time policy decision. Like `set_crossfade_duration`, a bare
    /// write with no `reset_next_track`: an armed transition keeps the floor
    /// it was armed under (fires once), and cancelling a live blend on every
    /// slider step would hard-cut audio.
    pub fn set_crossfade_min_track_secs(&mut self, secs: u32) {
        self.crossfade.min_track_secs = secs;
        self.renderer.lock().set_crossfade_min_track_secs(secs);
    }

    /// Set the album-continuity gate from settings (sequential same-album
    /// tracks transition gapless).
    ///
    /// On a REAL change this abandons any prepared/armed/in-flight transition
    /// via `reset_next_track`, mirroring the `set_crossfade_enabled` /
    /// `set_bit_perfect` mode-toggle contract: the toggle flips the policy
    /// verdict for the prepared pair, and the next prep re-derives it under
    /// the new setting (a same-album segue prepared as a blend must not still
    /// blend right after the user asks for seamless albums). No-op when
    /// unchanged so a routine settings save never disturbs an in-flight
    /// transition.
    pub async fn set_crossfade_album_gapless(&mut self, enabled: bool) {
        let changed = self.crossfade.album_continuity != enabled;
        self.crossfade.album_continuity = enabled;
        if changed {
            self.reset_next_track().await;
        }
    }

    /// Set the transport-fade (pause/resume/stop ramp) settings — the M5
    /// "Fading" section knobs. Stores the engine mirror (the stop pair's sole
    /// consumer is [`Self::stop`]) and pushes the pause pair down to the
    /// renderer (its consumers — `begin_pause_fade` and the resume fade-in in
    /// `start()` — live there). Durations are defensively clamped to the
    /// `TRANSPORT_FADE_MS_{MIN,MAX}` bounds so a hand-edited config can't
    /// stretch the bounded wait inside `stop()`.
    ///
    /// Bare write, like `set_crossfade_duration`: transport fades never flip
    /// crossfade eligibility, so the mode-toggle `reset_next_track` contract
    /// does not apply.
    pub fn set_transport_fades(
        &mut self,
        fade_on_pause: bool,
        fade_pause_ms: u32,
        fade_on_stop: bool,
        fade_stop_ms: u32,
    ) {
        use crate::types::player_settings::{TRANSPORT_FADE_MS_MAX, TRANSPORT_FADE_MS_MIN};
        let pause_ms = fade_pause_ms.clamp(TRANSPORT_FADE_MS_MIN, TRANSPORT_FADE_MS_MAX);
        let stop_ms = fade_stop_ms.clamp(TRANSPORT_FADE_MS_MIN, TRANSPORT_FADE_MS_MAX);
        self.fade.fade_on_stop = fade_on_stop;
        self.fade.fade_stop_ms = stop_ms;
        self.renderer
            .lock()
            .set_pause_fade(fade_on_pause, u64::from(pause_ms));
    }

    /// Set the M7 "Fade on Skip" settings (default Off). Bare write, like the
    /// other transport-fade knobs: it never flips crossfade eligibility, so
    /// the mode-toggle `reset_next_track` contract does not apply. Consumed
    /// at the next manual skip. The duration is defensively clamped to the
    /// `FADE_SKIP_SECS_{MIN,MAX}` bounds so a hand-edited config can't
    /// stretch the skip overlap (or the boundary fade's bounded wait) past
    /// the slider ceiling.
    pub fn set_skip_fade(
        &mut self,
        mode: crate::types::player_settings::FadeOnSkip,
        duration_secs: u32,
    ) {
        use crate::types::player_settings::{FADE_SKIP_SECS_MAX, FADE_SKIP_SECS_MIN};
        self.fade.fade_on_skip = mode;
        self.fade.fade_skip_ms = duration_secs.clamp(FADE_SKIP_SECS_MIN, FADE_SKIP_SECS_MAX) * 1000;
    }

    /// The current "Fade on Skip" mode — read by the manual-skip path
    /// (`PlaybackController::next`/`previous`) to pick the skip treatment.
    pub fn skip_fade_mode(&self) -> crate::types::player_settings::FadeOnSkip {
        self.fade.fade_on_skip
    }

    /// Set the M6 "Fade Radio Switches" setting (default off). Bare write,
    /// like the other transport-fade knobs: it never flips crossfade
    /// eligibility, so the mode-toggle `reset_next_track` contract does not
    /// apply. Consumed at the next radio↔queue switch.
    pub fn set_fade_radio_transitions(&mut self, enabled: bool) {
        self.fade.fade_radio_transitions = enabled;
    }

    /// Set the "Smooth Track Starts" setting (M2's de-click onset ramp gate,
    /// default on) — pushed to the renderer, which threads it into every new
    /// stream build. Bare write; takes effect on the next stream creation.
    pub fn set_smooth_track_starts(&mut self, enabled: bool) {
        self.renderer.lock().set_smooth_track_starts(enabled);
    }

    /// The controller-facing policy inputs for
    /// [`crate::audio::crossfade_policy::crossfade_decision`], read at
    /// gapless-prep time. `format_blocked` is always `false` here — format
    /// gating stays owned by the dual `crossfade_blocked` sites (renderer
    /// `arm_crossfade` + engine `try_start_crossfade_transition`), which see
    /// the real decoded formats.
    pub fn crossfade_policy_cfg(&self) -> crate::audio::crossfade_policy::CrossfadePolicyCfg {
        crate::audio::crossfade_policy::CrossfadePolicyCfg {
            min_track_secs: self.crossfade.min_track_secs,
            album_continuity: self.crossfade.album_continuity,
            format_blocked: false,
        }
    }

    /// One-lock snapshot of everything the controller's gapless prep needs to
    /// derive the per-transition directives (M4 policy inputs + the M8
    /// transition-shaping knobs). See [`TransitionPrepCfg`].
    pub fn transition_prep_cfg(&self) -> TransitionPrepCfg {
        TransitionPrepCfg {
            policy: self.crossfade_policy_cfg(),
            bar_snap: self.crossfade.bar_snap,
            crossfade_duration_ms: self.crossfade.duration_ms,
            gap_offset_ms: if self.crossfade.offset_secs > 0 {
                self.crossfade.offset_secs as u64 * 1000
            } else {
                0
            },
            // Dropping decoded samples is a content change — a bit-perfect
            // listener opted into untouched playback, so the trim stands
            // down under Strict AND Relaxed (mode intent, not the live
            // `pw_volume_active` viability, so the verdict can't flap with
            // the output backend).
            trim_leading_silence: self.crossfade.skip_silence
                && !self.crossfade.bit_perfect_mode.builds_bit_perfect(),
        }
    }

    /// Set the M8 "Gap / Overlap Trim" offset from settings (seconds,
    /// clamped to −2..+2). Negative = extra overlap (pushed to the
    /// renderer's Armed-trigger lead); positive = gap (consumed per
    /// transition via [`Self::transition_prep_cfg`] → the decode loop's EOF
    /// silence injection); 0 = untouched transitions. Bare write, like the
    /// sibling sliders: it never flips crossfade eligibility, and an armed
    /// transition fires once at the old offset.
    pub fn set_crossfade_offset(&mut self, secs: i32) {
        use crate::types::player_settings::{CROSSFADE_OFFSET_MAX_SECS, CROSSFADE_OFFSET_MIN_SECS};
        let clamped = secs.clamp(CROSSFADE_OFFSET_MIN_SECS, CROSSFADE_OFFSET_MAX_SECS);
        self.crossfade.offset_secs = clamped;
        let lead_ms = if clamped < 0 {
            clamped.unsigned_abs() as u64 * 1000
        } else {
            0
        };
        self.renderer.lock().set_crossfade_lead_ms(lead_ms);
    }

    /// Set the M8 "Snap Crossfade to Musical Bars" gate from settings.
    ///
    /// On a REAL change this abandons any prepared/armed transition via
    /// `reset_next_track` (the `set_crossfade_album_gapless` contract): the
    /// toggle changes the prepared pair's effective duration, and the next
    /// prep re-derives the override under the new setting. No-op when
    /// unchanged so a routine settings save never disturbs an in-flight
    /// transition.
    pub async fn set_crossfade_bar_snap(&mut self, enabled: bool) {
        let changed = self.crossfade.bar_snap != enabled;
        self.crossfade.bar_snap = enabled;
        if changed {
            self.reset_next_track().await;
        }
    }

    /// Set the M8 "Skip Silence Between Tracks" gate from settings — pushed
    /// to the renderer (trailing-tail trigger) and mirrored here for the
    /// prep-time leading-trim verdict.
    ///
    /// On a REAL change this abandons any prepared/armed transition via
    /// `reset_next_track` (the `set_crossfade_album_gapless` contract): the
    /// prepared decoder was built with the OLD trim verdict baked in, and
    /// the next prep re-derives it. No-op when unchanged.
    pub async fn set_skip_silence(&mut self, enabled: bool) {
        let changed = self.crossfade.skip_silence != enabled;
        self.crossfade.skip_silence = enabled;
        self.renderer.lock().set_skip_silence(enabled);
        if changed {
            self.reset_next_track().await;
        }
    }

    /// Whether crossfade is enabled
    pub fn crossfade_enabled(&self) -> bool {
        self.crossfade.enabled
    }

    /// Set bit-perfect output from settings — applied to the renderer, which
    /// owns the bit (native-rate output + DSP bypass) and the viability check
    /// (`pw_volume_active`); the engine keeps no mirrored copy.
    ///
    /// On a REAL change (the renderer reports whether the flag flipped) this
    /// also abandons any prepared/armed/in-flight transition via
    /// `reset_next_track`: bit-perfect flips crossfade eligibility, and a
    /// crossfade armed under the old mode could otherwise desync — `render_tick`
    /// fires it synchronously (renderer → Active + an incoming stream) while the
    /// engine's gate now refuses, orphaning the blend. Mirrors the
    /// shuffle/repeat/consume mode-toggle contract. No-op when unchanged so a
    /// routine settings save (which re-applies every field) never disturbs an
    /// in-flight transition.
    pub async fn set_bit_perfect(&mut self, mode: crate::types::player_settings::BitPerfectMode) {
        self.crossfade.bit_perfect_mode = mode;
        let changed = self.renderer.lock().set_bit_perfect(mode);
        if changed {
            self.reset_next_track().await;
        }
    }

    /// Whether the CURRENTLY-PLAYING primary stream was actually built
    /// bit-perfect (the renderer captures this at stream-build time, not the
    /// live setting). The honest now-playing badge reads this so a mid-track
    /// toggle — which only takes effect on the next track — can't make the badge
    /// claim BIT-PERFECT while the running stream is still on the DSP path. The
    /// brief renderer lock matches `position()`, already taken every snapshot.
    pub fn current_stream_bit_perfect(&self) -> bool {
        self.renderer.lock().current_stream_bit_perfect()
    }

    // =========================================================================
    // Volume Normalization API
    // =========================================================================

    /// Update volume normalization settings on the renderer.
    ///
    /// Takes effect on the next stream creation (play, seek, crossfade).
    pub fn set_volume_normalization(
        &mut self,
        mode: crate::types::player_settings::VolumeNormalizationMode,
        target_level: f32,
        preamp_db: f32,
        fallback_db: f32,
        fallback_to_agc: bool,
        prevent_clipping: bool,
    ) {
        let mut renderer = self.renderer.lock();
        renderer.set_volume_normalization(
            mode,
            target_level,
            preamp_db,
            fallback_db,
            fallback_to_agc,
            prevent_clipping,
        );
    }

    /// Update shared EQ state. Replaces existing eq state, taking effect on new streams.
    pub fn set_eq_state(&mut self, state: super::eq::EqState) {
        let mut renderer = self.renderer.lock();
        renderer.set_eq_state(state);
    }

    /// Start a crossfade transition using the prepared next decoder.
    /// Returns `true` if crossfade was started successfully.
    pub async fn start_crossfade(&mut self) -> bool {
        if !self.crossfade.phase.is_idle() {
            debug!("🔀 [CROSSFADE] Already active, skipping");
            return false;
        }

        // Take the prepared decoder for crossfade use, ungating the slot
        // and decoder ownership atomically. The slot's ReplayGain rides
        // along so it can be re-staged below.
        let (next_decoder, slot_replay_gain) = {
            let mut slot = self.gapless.lock().await;
            if !slot.is_prepared() {
                drop(slot);
                debug!("🔀 [CROSSFADE] No prepared decoder, cannot start");
                return false;
            }
            let dec = slot.decoder.take();
            slot.prepared = false;
            let rg = slot.replay_gain.clone();
            match dec {
                Some(d) => (d, rg),
                None => {
                    debug!("🔀 [CROSSFADE] Prepared flag set but no decoder, skipping");
                    return false;
                }
            }
        };

        let incoming_format = next_decoder.format().clone();
        // Effective = per-transition bar-snap override when staged (M8): the
        // EOF-fallback fire must blend at the same length the arm would have.
        let duration_ms = self.crossfade.effective_duration_ms();
        let incoming_source = self.next_source.clone();
        self.next_source.clear();

        debug!(
            "🔀 [CROSSFADE] Starting: outgoing={:?}, incoming={:?}, duration={}ms",
            self.current_format, incoming_format, duration_ms
        );

        // Wrap the decoder in a shared Arc<Mutex<Option<...>>> — this same Arc
        // is stored inside the `Active` variant AND captured by the spawned
        // decode loop. The loop watches its inner `Option` for `None` as the
        // signal to exit (see `cancel_crossfade` / `finalize_crossfade_engine`).
        let decoder_arc = Arc::new(tokio::sync::Mutex::new(Some(next_decoder)));

        // Only tell the renderer to start crossfade if it hasn't already
        // been activated synchronously by the renderer's queue-threshold
        // trigger. The renderer may have already called start_crossfade()
        // on itself before this async path runs.
        {
            let mut renderer = self.renderer.lock();
            // Re-stage the slot's ReplayGain: redundant-but-consistent on
            // the ordinary path (the store staged the same value), and
            // CORRECTIVE after a cancel dropped the renderer's staged copy
            // while the slot kept the prep (e.g. a seek mid-fade) — firing
            // with `None` would fade the incoming up at the untagged
            // fallback gain. When the renderer already went Active it built
            // the incoming from the identical store-time value, so this
            // write cannot tear it (start-time reads only).
            renderer.set_pending_crossfade_replay_gain(slot_replay_gain);
            if !renderer.is_crossfade_active() {
                renderer.start_crossfade(duration_ms, &incoming_format);
            }
        }

        self.crossfade.phase = CrossfadePhase::Active {
            decoder: decoder_arc.clone(),
            incoming_source,
        };

        // Start a decode loop for the incoming track
        self.start_crossfade_decode_loop(decoder_arc);

        true
    }

    /// Plan-time invalidation for a manual skip-crossfade (M7): called by the
    /// queue layer (`QueueNavigator::skip_to_song`) UNDER the engine lock,
    /// BEFORE it returns a [`SkipFadePlan`](crate::services::playback::SkipFadePlan)
    /// and the locks are released for the incoming decoder build.
    ///
    /// Three steps, in order:
    /// 1. `reset_next_track` — the pre-skip prepared/armed/in-flight
    ///    transition is void (the queue is about to re-sequence past it);
    ///    a LIVE blend is cancelled here so nothing can finalize — and
    ///    advance the queue a second time — during the unlocked build.
    /// 2. `bump_for_user_action` — the skip IS the user-driven source change
    ///    (the audible source's fate is sealed at plan time, even though the
    ///    actual `set_source`/fire happens later). Every completion dispatch
    ///    snapshotted BEFORE this instant is discarded by the renderer's
    ///    staleness gate.
    /// 3. Latch the pending window with the post-bump generation so
    ///    completions dispatched DURING the build (they snapshot the new
    ///    value) are deferred by `on_renderer_finished` / stood down by the
    ///    inline gapless swap instead of advancing the already-advanced
    ///    cursor. The latch self-invalidates: every exit from the window
    ///    either moves the generation past it (the fire's bump, the
    ///    fallback's `set_source`, any competing source change) or closes it
    ///    explicitly ([`Self::close_skip_fade_window`] on the seq-abandon
    ///    exit — a superseding NO-OP skip stamps the sequence without ever
    ///    bumping the generation).
    pub async fn plan_skip_fade(&mut self) {
        self.reset_next_track().await;
        let generation = self.channels.source_generation.bump_for_user_action();
        self.channels
            .skip_fade_pending
            .store(generation, Ordering::Release);
        debug!("🔀 [SKIP FADE] Planned — window latched at generation {generation}");
    }

    /// Whether a planned skip-crossfade's build window is still open: the
    /// plan-time latch matches the CURRENT source generation. See
    /// [`Self::plan_skip_fade`]. `pub(crate)` for the controller-side
    /// regression tests over the seq-abandon exit.
    pub(crate) fn skip_fade_window_pending(&self) -> bool {
        self.channels.skip_fade_pending.load(Ordering::Acquire)
            == self.channels.source_generation.current()
    }

    /// Close a planned skip fade's pending window WITHOUT a source change —
    /// the controller's `complete_skip_fade` seq-abandon exit. The
    /// superseding action normally owns the engine and bumps the generation
    /// itself, but a NO-OP skip (Next at the end of the queue, Previous with
    /// nothing to step back to) stamps the sequence counter without ever
    /// touching the engine: no bump will un-match the abandoned plan's
    /// latch, so the abandon must close it here or every end-of-track
    /// completion defers forever at [`Self::on_renderer_finished`]. Gated on
    /// the abandoned plan's OWN generation snapshot: a newer plan re-latches
    /// at a strictly greater generation (its `plan_skip_fade` bump), so a
    /// live newer window is never touched.
    pub(crate) fn close_skip_fade_window(&self, plan_generation: u64) {
        if self.channels.source_generation.current() == plan_generation {
            self.channels
                .skip_fade_pending
                .store(NO_SKIP_FADE_PENDING, Ordering::Release);
            debug!(
                "🔀 [SKIP FADE] Abandoned plan closed its window (generation {plan_generation})"
            );
        }
    }

    /// Start an immediate manual-skip crossfade to `incoming_url` using an
    /// already-initialized on-demand decoder (M7 "Fade on Skip: Crossfade").
    ///
    /// Unlike the auto-advance path there is no `Armed` state to trigger —
    /// the fade starts `Active` DIRECTLY, preserving the load-bearing
    /// ordering (renderer Active strictly BEFORE the engine phase); the
    /// cancel-live-first contract ran at PLAN time (`plan_skip_fade`, under
    /// the locks). Because the direct fire bypasses the renderer's
    /// `arm_crossfade`, its gates are re-applied here: the
    /// `crossfade_blocked` format gate, the known-durations guard, the
    /// minimum-track floor, and the `shorter/2` clamp — plus a
    /// remaining-audio clamp `arm_crossfade` never needs (the position
    /// trigger fires exactly `fade` before the end; a manual skip can land
    /// anywhere, and a fade longer than the outgoing's remaining audio
    /// would EOF mid-blend and cut to silence).
    ///
    /// `generation` is the caller's `source_generation()` snapshot from
    /// skip time — taken AFTER `plan_skip_fade` bumped it under the locks
    /// (the decoder build then runs with no locks held — invariant 14). A
    /// mismatch means a competing user action owns the engine, so the skip
    /// fade is abandoned (`Stale`). On `Fired` the generation is bumped
    /// AGAIN, closing the pending window: completions dispatched during the
    /// build snapshotted the plan generation and are discarded.
    pub async fn crossfade_to_next(
        &mut self,
        decoder: AudioDecoder,
        incoming_url: String,
        replay_gain: Option<crate::types::song::ReplayGain>,
        generation: u64,
    ) -> SkipFadeOutcome {
        if self.channels.source_generation.current() != generation {
            debug!("🔀 [SKIP FADE] Superseded (generation moved) — abandoning");
            return SkipFadeOutcome::Stale;
        }
        if !self.skip_crossfade_viable() {
            debug!("🔀 [SKIP FADE] Not viable (not audibly playing a finite stream)");
            return SkipFadeOutcome::Blocked;
        }
        // Outgoing drained during the unlocked build (its completion was
        // deferred by the pending window, so `playing` is still true): there
        // is nothing left to blend — refuse, so the caller's fallback
        // hard-loads the target NOW instead of fading it in over 1-4s of
        // silence.
        if self.channels.decoder_eof.load(Ordering::Acquire)
            && self.renderer.lock().is_buffer_queue_empty()
        {
            debug!("🔀 [SKIP FADE] Outgoing drained during the build — falling back to hard load");
            return SkipFadeOutcome::Blocked;
        }

        // The PRE-SKIP transition (live blend + prepared slot) was already
        // cancelled at plan time (`plan_skip_fade`, under the locks), so
        // anything in the gapless slot NOW was stored DURING the window and
        // targets the track AFTER the skip target — still valid; finalize
        // re-arms from it. Only a live blend (structurally impossible while
        // the generation still matches, but belt-and-braces) must die before
        // the direct fire; `cancel_crossfade` leaves the slot alone.
        if self.crossfade.is_crossfade_live(&self.renderer) {
            self.cancel_crossfade().await;
        }

        let incoming_format = decoder.format().clone();
        if self
            .renderer
            .lock()
            .crossfade_blocked(&self.current_format, &incoming_format)
        {
            debug!("🔀 [SKIP FADE] Blocked by format gate (bit-perfect) — falling back");
            return SkipFadeOutcome::Blocked;
        }
        let Some(fade_ms) = skip_fade_duration_ms(
            u64::from(self.fade.fade_skip_ms),
            self.duration,
            decoder.duration(),
            self.position(),
            u64::from(self.crossfade.min_track_secs) * 1000,
        ) else {
            debug!("🔀 [SKIP FADE] Blocked by duration gates — falling back");
            return SkipFadeOutcome::Blocked;
        };

        debug!(
            "🔀 [SKIP FADE] Starting: outgoing={:?}, incoming={:?}, duration={}ms",
            self.current_format, incoming_format, fade_ms
        );

        let decoder_arc = Arc::new(tokio::sync::Mutex::new(Some(decoder)));

        // Renderer goes Active FIRST (the same ordering the auto trigger
        // guarantees); the incoming's ReplayGain is staged so the stream
        // build resolves the right amplify factor.
        {
            let mut renderer = self.renderer.lock();
            renderer.set_pending_crossfade_replay_gain(replay_gain);
            renderer.start_crossfade(fade_ms, &incoming_format);
        }

        self.crossfade.phase = CrossfadePhase::Active {
            decoder: decoder_arc.clone(),
            incoming_source: incoming_url,
        };
        self.crossfade.skip_fade = true;

        // A manual skip is a user-driven source change: invalidate stale
        // completion dispatches for the outgoing (set_source does the same
        // on the hard-cut path). The outgoing's PRIMARY decode loop is
        // unaffected — its liveness rides `decode_loop`, not this counter —
        // and every renderer dispatch from here on snapshots the new value.
        self.channels.source_generation.bump_for_user_action();

        self.start_crossfade_decode_loop(decoder_arc);

        SkipFadeOutcome::Fired
    }

    /// Whether a manual-skip crossfade can even be attempted: something must
    /// be audibly playing (there is no outgoing to blend otherwise) and the
    /// current stream must be finite (radio switches are M6's domain — its
    /// `set_source` fade handles the infinite-stream edge).
    pub fn skip_crossfade_viable(&self) -> bool {
        self.immediate_playing() && !self.channels.stream_is_infinite.load(Ordering::Acquire)
    }

    /// Whether a CLICK-initiated track start (play-from-queue /
    /// play-from-browse, M10) should even PLAN a skip-crossfade: M7's
    /// viability ([`Self::skip_crossfade_viable`]) plus a bit-perfect
    /// **Strict** pre-gate. Strict refuses every blend at the fire's format
    /// gate regardless of the incoming format, so planning would only buy
    /// the click a wasted network decoder build before the same hard cut it
    /// takes today — the pre-gate keeps that path byte-identical. Relaxed
    /// must still plan (its verdict needs the incoming format).
    pub fn click_skip_crossfade_viable(&self) -> bool {
        self.skip_crossfade_viable()
            && self.crossfade.bit_perfect_mode
                != crate::types::player_settings::BitPerfectMode::Strict
    }

    /// The M7 boundary out-fade: ramp the outgoing to silence over the
    /// "Fade on Skip" duration before the caller hard-loads the next track
    /// (M2's onset ramp then softens the incoming edge). Self-refusing like
    /// the M5/M6 out-ramps — nothing audible to fade when stopped or paused,
    /// and `begin_stop_fade` refuses on bit-perfect streams, live
    /// crossfades, and drained rings (the refusal degrades to the honest
    /// instant cut).
    pub async fn run_skip_out_fade(&mut self) {
        if !self.playing || self.paused {
            return;
        }
        self.run_bounded_out_fade(u64::from(self.fade.fade_skip_ms))
            .await;
    }

    /// Cancel an active crossfade (e.g., on skip, seek, or stop).
    pub async fn cancel_crossfade(&mut self) {
        let phase = std::mem::replace(&mut self.crossfade.phase, CrossfadePhase::Idle);
        // A cancelled skip fade dies with its phase — a stale marker would
        // suppress the completion callback of a LATER auto-advance finalize.
        self.crossfade.skip_fade = false;
        // Clear the engine-side incoming decoder if the engine had already
        // acknowledged the crossfade. When only the renderer is Active (the
        // engine hasn't acked yet — see `reset_next_track`), there is no decoder
        // to clear, but the renderer's live incoming stream must STILL be torn
        // down, so fall through to the renderer cancel rather than early-return.
        if let CrossfadePhase::Active { decoder, .. }
        | CrossfadePhase::OutgoingFinished { decoder, .. } = phase
        {
            // Signal the spawned decode loop to exit by clearing its inner Option.
            *decoder.lock().await = None;
        }
        debug!("🔀 [CROSSFADE] Cancelling");
        let mut renderer = self.renderer.lock();
        renderer.cancel_crossfade();
        renderer.disarm_crossfade();
    }

    /// Recover from a crossfade whose incoming stream stalled at completion.
    ///
    /// Driven by the renderer's `on_renderer_crossfade_stalled` path (the fade
    /// reached 100% wall-clock progress but the incoming ring is empty, so the
    /// incoming decoder never produced audio). Rather than promoting the silent
    /// decoder — which would fade the audible outgoing track into silence — this
    /// cancels the crossfade (restoring the outgoing stream as primary at full
    /// volume and clearing the incoming decoder), bumps the source generation so
    /// any late callbacks from the abandoned incoming source are discarded by
    /// the renderer's staleness gate, then runs the normal end-of-track
    /// transition so the bad track is skipped via the standard prepared/next
    /// path. No decoder is created while a lock is held (cancel only clears).
    pub async fn recover_stalled_crossfade(&mut self) {
        // Gate on the LIVE predicate, not the engine phase alone: in the
        // desynced case (renderer Active mid-fade while the engine half never
        // started, so its phase is still Idle) a phase-idle early-return
        // no-ops without resetting the renderer — render_tick then re-reports
        // the stall every tick, an unrecoverable warn livelock.
        // `cancel_crossfade` tolerates engine-Idle and tears the renderer
        // down regardless.
        if !self.crossfade.is_crossfade_live(&self.renderer) {
            return;
        }

        // M7: a stalled SKIP fade recovers differently — the queue cursor,
        // history, and consume already advanced to the incoming at skip
        // time, so routing through `on_decoder_finished` (whose completion
        // callback runs `decide_transition` against that already-advanced
        // cursor) would advance PAST the target: one Next press lands two
        // tracks ahead and the skipped-to track never plays. Capture the
        // target + its staged RG BEFORE the cancel wipes them (`skip_fade`
        // is cleared with the phase; the renderer cancel drops the staged
        // RG), then hard-load the target — the queue's current row — via
        // the standard load path. On a dead network the reload fails
        // honestly (engine stops, queue still names the target; Play
        // retries it).
        let skip_target = if self.crossfade.skip_fade {
            match &self.crossfade.phase {
                CrossfadePhase::Active {
                    incoming_source, ..
                }
                | CrossfadePhase::OutgoingFinished {
                    incoming_source, ..
                } => Some((
                    incoming_source.clone(),
                    self.renderer.lock().take_pending_crossfade_replay_gain(),
                )),
                CrossfadePhase::Idle => None,
            }
        } else {
            None
        };

        warn!("🔀 [CROSSFADE] Incoming stalled at completion — restoring outgoing and skipping");
        self.cancel_crossfade().await;
        // The abandoned incoming source is being thrown away; invalidate any of
        // its in-flight completion callbacks.
        self.channels.source_generation.bump_for_user_action();

        if let Some((target, replay_gain)) = skip_target {
            warn!(
                "🔀 [SKIP FADE] Stalled blend was a manual skip — hard-loading the target \
                 (queue already advanced): {}",
                redact_subsonic_url(&target)
            );
            self.load_track_with_rg(&target, replay_gain, None).await;
            if let Err(e) = self.play().await {
                warn!("🔀 [SKIP FADE] Stall-recovery reload failed: {e}");
            }
            return;
        }

        // Auto-advance blend: skip the stalled track via the standard
        // end-of-track machinery (the cursor has NOT advanced yet).
        self.on_decoder_finished().await;
    }

    /// Finalize crossfade: promote the incoming track to become the current track.
    /// Called when the renderer finishes mixing (crossfade progress reaches 1.0)
    /// or when the outgoing decoder's buffers are fully consumed.
    pub(crate) async fn finalize_crossfade_engine(&mut self) {
        let phase = std::mem::replace(&mut self.crossfade.phase, CrossfadePhase::Idle);
        let (decoder_arc, incoming_source) = match phase {
            CrossfadePhase::Idle => return,
            CrossfadePhase::Active {
                decoder,
                incoming_source,
            }
            | CrossfadePhase::OutgoingFinished {
                decoder,
                incoming_source,
            } => (decoder, incoming_source),
        };

        debug!("🔀 [CROSSFADE] Finalizing — incoming becomes current");

        // M7: a manual-skip fade already advanced the queue (cursor, history,
        // consume) at skip time — read-and-clear the marker so the completion
        // callback below is skipped for it (decide_transition would advance
        // AGAIN, silently skipping a track).
        let was_skip_fade = std::mem::take(&mut self.crossfade.skip_fade);

        // Mark this advance as a crossfade so the completion path labels the
        // "Now Playing" log line `crossfade` (both gapless and crossfade reach
        // the engine "already playing" branch). Read-and-reset by the
        // controller — which never runs for a skip fade, so the label is only
        // stamped for auto-advances (a skip stamp would leak into the NEXT
        // transition's label).
        if !was_skip_fade {
            self.last_transition_was_crossfade = true;
        }

        // Stop outgoing decode loop by advancing generation
        self.decode_loop.supersede();

        // Take the crossfade decoder and make it the primary
        let crossfade_dec = decoder_arc.lock().await.take();
        if let Some(decoder) = crossfade_dec {
            // Swap decoders
            *self.decoder.lock().await = decoder;
            let dec = self.decoder.lock().await;

            // Update engine state to reflect the incoming track
            self.source = incoming_source;
            self.current_format = dec.format().clone();
            self.live_sample_rate
                .store(self.current_format.sample_rate(), Ordering::Relaxed);
            self.duration = dec.duration();
            self.position = 0;
            self.next_format = AudioFormat::invalid();
            drop(dec);

            // Read the stored crossfade elapsed time and apply state resets.
            // The renderer already finalized (from render_buffers), so we just
            // read the stored elapsed time and reset position tracking.
            //
            // Do NOT call renderer.init() here — it clears the primary queue,
            // wiping the crossfade buffers that finalize_crossfade() just
            // transferred. Instead, do targeted state resets.
            let crossfade_elapsed_ms;
            {
                let mut renderer = self.renderer.lock();
                // Finalize the renderer-side crossfade: swap crossfade stream → primary,
                // reset crossfade_active. In the PipeWire architecture this was done by
                // render_buffers(), but in rodio we must do it explicitly here.
                renderer.finalize_crossfade();
                // Read the stored elapsed time for position offset.
                crossfade_elapsed_ms = renderer.take_crossfade_elapsed_ms();
                // Reset position tracking with offset: the incoming track has
                // been playing for crossfade_elapsed_ms already.
                renderer.reset_position_with_offset(crossfade_elapsed_ms);
                // Reset finished_called so on_renderer_finished can fire again
                renderer.reset_finished_called();
                renderer.set_volume(self.volume);
            }
            // Engine position also starts at the crossfade offset
            self.position = crossfade_elapsed_ms;

            // Intentional no-op (was: "Don't increment source_generation here")
            // — the crossfade was an intentional transition, not a user-
            // initiated skip.
            self.channels.source_generation.accept_internal_swap();

            // Restart the primary decode loop with the new decoder
            self.start_decoding_loop();
        }

        // Re-arm the next transition from a slot stored MID-fade (see the
        // live-blend branch in `store_prepared_decoder` — arming there would
        // overwrite the Active variant). No-op when the slot is empty, i.e.
        // for every ordinary auto-advance finalize.
        self.rearm_crossfade_if_prepared().await;

        if was_skip_fade {
            // The queue advanced at skip time; the UI refresh ran there too.
            // Firing the callback would run decide_transition against an
            // already-advanced cursor — a double advance.
            debug!(
                "🔀 [SKIP FADE] Finalized — completion callback suppressed (queue already advanced)"
            );
        } else if let Some(callback) = &self.completion_callback {
            // Notify completion callback (gapless-style: a new track started)
            callback(false);
        }
    }

    /// Start a decode loop for the incoming crossfade track.
    /// Similar to `start_decoding_loop` but writes to the renderer's crossfade buffer queue.
    ///
    /// `decoder_arc` is the same Arc stored inside `CrossfadePhase::Active.decoder`
    /// — the spawned loop watches its inner `Option` and exits when it
    /// becomes `None` (which `cancel_crossfade` / `finalize_crossfade_engine`
    /// trigger by clearing or taking the decoder respectively).
    fn start_crossfade_decode_loop(
        &mut self,
        decoder_arc: Arc<tokio::sync::Mutex<Option<AudioDecoder>>>,
    ) {
        let decoder = decoder_arc;
        let renderer = self.renderer.clone();
        let crossfade_duration_shared = self.channels.crossfade_duration_shared.clone();

        // M9 Part B: a fresh per-fade liveness handle. The loop brackets its
        // blocking decode/network call with it; the renderer's completion
        // gate reads it to tell blocked-on-socket from sleeping-on-
        // backpressure. Per-fade (not renderer-persistent) so a superseded
        // loop's late writes can never pollute a newer fade's verdict. Both
        // call sites run with NO renderer lock held (their renderer scopes
        // closed above), so this install cannot deadlock.
        let liveness = Arc::new(crate::audio::IncomingLiveness::new());
        self.renderer
            .lock()
            .set_incoming_liveness(Some(liveness.clone()));

        tokio::spawn(async move {
            trace!("🔀 [CROSSFADE DECODE] Loop started");

            // Backpressure: shared dual-watermark step (`backpressure_step`)
            // matching the primary decode loop; watermarks scale with crossfade
            // duration so the ring buffer can hold the full fade-in ramp.
            let mut backpressure_active = false;
            // Cached incoming-stream frame_rate for the time-based watermarks
            // (see the primary loop). 0 until the first decoded buffer.
            let mut frame_rate: u32 = 0;

            loop {
                // Check if crossfade is still active by checking if decoder still exists
                let decoder_guard = decoder.lock().await;
                let decoder_exists = decoder_guard.is_some();
                drop(decoder_guard);

                if !decoder_exists {
                    trace!("🔀 [CROSSFADE DECODE] Decoder removed, exiting loop");
                    break;
                }

                // Backpressure check — time-based watermarks (same as primary loop)
                let buffer_count = {
                    let renderer_guard = renderer.lock();
                    renderer_guard.crossfade_buffer_count() // interleaved samples
                };

                let cf_ms = crossfade_duration_shared.load(Ordering::Relaxed);
                // is_infinite = false: incoming crossfade tracks are always finite.
                match backpressure_step(
                    "CROSSFADE DECODE",
                    buffer_count,
                    frame_rate,
                    cf_ms,
                    false,
                    &mut backpressure_active,
                ) {
                    BackpressureAction::Sleep(duration) => {
                        tokio::time::sleep(duration).await;
                        continue;
                    }
                    BackpressureAction::Proceed => {}
                }

                // Decode a buffer from the incoming track
                let mut decoder_guard = decoder.lock().await;
                let dec = match decoder_guard.as_mut() {
                    Some(d) => d,
                    None => break,
                };

                if !dec.is_initialized() || dec.is_eof() {
                    trace!("🔀 [CROSSFADE DECODE] EOF or not initialized, exiting loop");
                    drop(decoder_guard);
                    break;
                }

                frame_rate = dec.format().frame_rate();

                // Bracket ONLY the blocking decode/network call: a returned
                // call (data, error, or EOF alike) is a live socket, while
                // the backpressure sleep above and the lock handoffs stay
                // outside the bracket and always read as live.
                liveness.mark_read_start();
                let chunk = tokio::task::block_in_place(|| decode_one_chunk(dec));
                liveness.mark_read_end();
                drop(decoder_guard);

                if let Some(samples) = chunk {
                    let mut renderer_guard = renderer.lock();
                    renderer_guard.write_crossfade_samples(&samples);
                    drop(renderer_guard);
                } else {
                    // No data, wait a bit
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                }
            }

            trace!("🔀 [CROSSFADE DECODE] Loop finished");
        });
    }

    /// Load prepared track (for gapless transition)
    pub async fn load_prepared_track(&mut self) -> Result<()> {
        // Drain the slot atomically: take ownership of the decoder, clear
        // prepared + source so the slot can't be reused mid-swap.
        let next_decoder = {
            let mut slot = self.gapless.lock().await;
            let dec = match slot.decoder.take() {
                Some(d) => d,
                None => anyhow::bail!("No prepared track to load"),
            };
            slot.prepared = false;
            slot.source.clear();
            dec
        };

        // Stop current decoding loop before swapping decoders
        self.decode_loop.supersede();

        // Store previous format for gapless detection
        let prev_format = self.current_format.clone();

        // Switch decoders
        *self.decoder.lock().await = next_decoder;
        let decoder = self.decoder.lock().await;

        // Update source and format
        self.source = self.next_source.clone();
        self.next_source.clear();
        self.current_format = decoder.format().clone();
        self.live_sample_rate
            .store(self.current_format.sample_rate(), Ordering::Relaxed);
        self.next_format = AudioFormat::invalid();

        // Update duration
        self.duration = decoder.duration();
        self.position = 0;
        drop(decoder);

        // Check if formats match for gapless playback
        let formats_match = prev_format.is_valid()
            && self.current_format.is_valid()
            && prev_format == self.current_format;
        let force_reload = !formats_match;

        debug!(
            "🔄 [GAPLESS] Transition: prev={:?} → cur={:?}, formats_match={}, force_reload={}, source={}",
            prev_format,
            self.current_format,
            formats_match,
            force_reload,
            redact_subsonic_url(&self.source)
        );

        // Initialize renderer with format-aware gapless logic
        let should_start = {
            let mut renderer = self.renderer.lock();
            renderer.init(&self.current_format, force_reload, Some(&prev_format))?;

            // Apply current volume to renderer
            renderer.set_volume(self.volume);

            // If we were playing, continue playing
            if self.playing && !self.paused {
                renderer.start();
                true
            } else {
                false
            }
        }; // renderer lock dropped here, before any .await

        if should_start {
            // Restart decoding loop for the new track
            self.start_decoding_loop();
            // Restart render thread for new track
            self.start_render_thread();
        }

        Ok(())
    }

    /// Immediate state access methods for UI-critical operations
    /// These avoid async locks for better responsiveness
    /// Get immediate playing state (for UI updates that need instant response)
    pub fn immediate_playing(&self) -> bool {
        self.playing && !self.paused
    }

    /// Read-and-reset whether the most recent auto-advance was a crossfade
    /// (set by `finalize_crossfade_engine`). The completion path uses this to
    /// label the "Now Playing" log line; resetting on read keeps it from
    /// leaking into the next (gapless) transition.
    pub fn take_last_transition_was_crossfade(&mut self) -> bool {
        std::mem::replace(&mut self.last_transition_was_crossfade, false)
    }

    /// Get immediate paused state
    pub fn immediate_paused(&self) -> bool {
        self.paused
    }

    /// Test-only: force the transport flags to "audibly playing" so
    /// navigator-level tests can drive the skip-fade eligibility path
    /// without a real audio device.
    #[cfg(test)]
    pub fn force_playing_for_test(&mut self) {
        self.playing = true;
        self.paused = false;
    }

    /// Test-only: mark the current source as an infinite (radio) stream so
    /// cross-module tests can drive the radio arm of the skip-fade gates
    /// (the `channels` field is private to this module).
    #[cfg(test)]
    pub fn force_infinite_for_test(&mut self) {
        self.channels
            .stream_is_infinite
            .store(true, Ordering::Release);
    }

    /// Get current sample rate in Hz (for UI display)
    /// Uses lock-free atomic for threading consistency with live_bitrate.
    pub fn sample_rate(&self) -> u32 {
        self.live_sample_rate.load(Ordering::Relaxed)
    }

    /// Get live compressed bitrate in kbps (updated per-packet from decoder)
    pub fn live_bitrate(&self) -> u32 {
        self.channels.live_bitrate.load(Ordering::Relaxed)
    }

    /// Current source generation (incremented on every `set_source` call).
    /// Used by the renderer's stale-callback guard.
    pub fn source_generation(&self) -> u64 {
        self.channels.source_generation.current()
    }

    /// Clear the prepared next-track decoder and all associated state.
    ///
    /// Call this whenever the play order changes (shuffle/repeat/consume toggle)
    /// to prevent a stale gapless transition to the wrong song.
    pub async fn reset_next_track(&mut self) {
        // Cancel an in-flight crossfade FIRST. A mode toggle (shuffle / repeat /
        // consume / bit-perfect) during an active fade must abandon the
        // prepared/in-flight next track so the engine re-derives it under the new
        // mode — otherwise finalize_crossfade_engine would still promote the
        // now-wrong incoming track. cancel_crossfade resets crossfade_phase →
        // Idle, clears the incoming decoder, and restores the outgoing as primary.
        //
        // Check the RENDERER's state too, not just the engine's: render_tick
        // swaps the renderer Armed → Active synchronously and creates the live
        // incoming stream a tick BEFORE the spawned `on_renderer_finished` task
        // sets the engine's `crossfade_phase`. A toggle landing in that window
        // would otherwise skip the cancel (engine still Idle) and orphan the
        // renderer's live incoming stream — fading the outgoing into silence with
        // no recovery. `cancel_crossfade` tolerates the engine-Idle case and
        // tears down the renderer regardless.
        //
        // `is_crossfade_live` brings both checks together: it locks the renderer,
        // reads its crossfade-active flag, drops the guard, and returns a plain
        // bool BEFORE the `cancel_crossfade().await` below (which re-locks the
        // renderer) — so no `parking_lot` guard ever straddles the await.
        if self.crossfade.is_crossfade_live(&self.renderer) {
            self.cancel_crossfade().await;
        }
        self.gapless.lock().await.clear();
        self.next_source.clear();
        self.next_format = AudioFormat::invalid();
        // The per-transition verdicts die with the transition they were
        // derived for; the next `store_prepared_decoder` re-derives them.
        // The M8 override restore covers the shared watermark mirror too
        // (invariant 9 — a stale snapped value must never leak into a later
        // spawn's `compute_watermarks`), and the pending gap is voided.
        self.crossfade.suppress_this_transition = false;
        self.crossfade.duration_override_ms = None;
        self.channels
            .crossfade_duration_shared
            .store(self.crossfade.duration_ms, Ordering::Relaxed);
        self.channels.gap_offset_ms.store(0, Ordering::Release);
        // Still needed for the Armed-but-not-Active case (cancel_crossfade only
        // touches Active); harmless when cancel_crossfade already disarmed.
        self.renderer.lock().disarm_crossfade();
    }

    /// Get playback state
    pub fn state(&self) -> PlaybackState {
        self.state
    }

    /// Set completion callback.
    ///
    /// The callback receives `true` when the same track is looping (repeat-one),
    /// `false` when a different track starts.
    pub fn set_completion_callback<F>(&mut self, callback: F)
    where
        F: Fn(bool) + Send + Sync + 'static,
    {
        self.completion_callback = Some(Arc::new(callback));
    }

    /// Set visualizer callback
    pub fn set_visualizer_callback(
        &mut self,
        callback: crate::audio::renderer::VisualizerCallback,
    ) {
        let renderer = self.renderer.lock();
        renderer.set_visualizer_callback(callback);
    }

    /// Toggle the master visualizer gate on every stream.
    ///
    /// When `false`, the real-time audio thread skips the per-sample visualizer
    /// tap entirely — so turning the visualizer off stops the audio-thread DSP
    /// feed, not just the GPU render. The UI calls this from the
    /// cycle-visualization handler (Off → `false`, Bars/Lines → `true`).
    pub fn set_visualizer_enabled(&self, enabled: bool) {
        let renderer = self.renderer.lock();
        renderer.set_visualizer_enabled(enabled);
    }

    /// Connect the music-output bridge (shared with the SFX engine + volume UI),
    /// then build the initial 48 kHz music sink so SFX + node volume work before
    /// the first track. The renderer rebuilds it at native rate per track in
    /// bit-perfect mode. Call once at login.
    pub fn set_music_bridge(
        &mut self,
        bridge: std::sync::Arc<crate::audio::music_bridge::MusicOutputBridge>,
    ) {
        let mut renderer = self.renderer.lock();
        renderer.set_music_bridge(bridge);
    }

    /// Set engine reference in renderer
    pub fn set_engine_reference(&mut self, engine: Weak<tokio::sync::Mutex<CustomAudioEngine>>) {
        let mut renderer = self.renderer.lock();
        renderer.set_engine_link(
            engine,
            self.channels.source_generation.clone(),
            self.channels.decoder_eof.clone(),
            self.channels.stream_is_infinite.clone(),
        );
    }

    /// Check if next track is prepared for gapless playback
    pub async fn is_next_track_prepared(&self) -> bool {
        self.gapless.lock().await.is_prepared()
    }

    /// Handle renderer finished (called when renderer runs out of buffers)
    /// This matches the C++ onRendererFinished implementation
    /// Returns true if the track was actually finished
    pub async fn on_renderer_finished(&mut self) -> bool {
        // Don't trigger track end if we're in the middle of seeking
        if self.seeking.load(Ordering::Acquire) {
            trace!(" [RENDERER FINISHED] Ignoring - seek in progress");
            return false;
        }

        // M7 skip-fade build window: the queue cursor ALREADY advanced for a
        // manual skip whose blend is still building (locks released, see
        // `plan_skip_fade`). Running the completion machinery here would
        // advance the already-advanced cursor — a double advance (silently
        // skipped track / now-playing desync). Defer: the skip's fire (or
        // its hard fallback) owns the transition, and if the outgoing
        // drained here, the fire detects the drained ring and falls back to
        // an immediate hard load of the skip target. The latch cannot
        // strand — every exit from the window bumps the generation (which
        // un-matches it), except the seq-abandon exit, which closes the
        // latch explicitly via `close_skip_fade_window` (its superseding
        // no-op skip never touches the engine).
        if self.skip_fade_window_pending() {
            debug!(
                " [RENDERER FINISHED] Deferred — a planned skip fade owns the transition \
                 (queue already advanced)"
            );
            return false;
        }

        // Renderer finished all its buffers - check if track is truly finished
        let decoder = self.decoder.lock().await;
        let is_eof = decoder.is_eof();
        let duration = decoder.duration();
        drop(decoder);

        let position = self.position();

        debug!(
            " [RENDERER FINISHED] EOF={}, position={}ms, duration={}ms, playing={}, paused={}",
            is_eof, position, duration, self.playing, self.paused
        );

        // Phase 1: Crossfade finalization — outgoing queue drained
        let renderer_fade_active = self.renderer.lock().is_crossfade_active();
        if self
            .try_finalize_crossfade(is_eof, renderer_fade_active)
            .await
        {
            return false;
        }

        // Phase 2: Crossfade initiation — position-based trigger fired
        if self.try_start_crossfade_transition(is_eof).await {
            return false;
        }

        // Phase 3: Normal track completion or buffer starvation
        let position_indicates_finished = duration > 0 && position >= duration;
        if is_eof || position_indicates_finished {
            debug!(
                " [RENDERER FINISHED] Track finished (EOF={}, pos={} >= dur={}, pos_finished={})",
                is_eof, position, duration, position_indicates_finished
            );
            self.on_decoder_finished().await;
            true
        } else if !is_eof && self.playing && !self.paused {
            self.handle_buffer_starvation(position, duration).await
        } else {
            trace!(" [RENDERER FINISHED] Not playing or paused, no action taken");
            false
        }
    }

    /// Check if an active crossfade has run its course and finalize it.
    ///
    /// Handles three cases:
    /// - `Active + is_eof`: queue drained BEFORE decoder signaled EOF (race)
    /// - `OutgoingFinished`: decoder already signaled EOF, queue drained after
    /// - `Active + !renderer_fade_active`: the renderer's fade ELAPSED and it
    ///   already finalized its side, but the outgoing decoder has not reached
    ///   EOF — e.g. a coarse VBR seek landed earlier in the audio than the
    ///   position implied, leaving a long tail. The outgoing is at zero
    ///   volume past the fade, so the tail is inaudible by construction:
    ///   finalize and discard it. Waiting for EOF instead leaves a torn
    ///   state (renderer promoted, engine still Active) in which the
    ///   crossfade decode loop free-runs the incoming decoder against stale
    ///   queue bookkeeping and its premature EOF then kills the promoted
    ///   track moments after its transition.
    ///
    /// The renderer goes Active strictly BEFORE the engine phase does
    /// (render_tick swaps Armed→Active synchronously, then signals the
    /// engine), and cancel paths clear both sides together, so engine-Active
    /// with renderer-not-Active can only mean the fade completed.
    async fn try_finalize_crossfade(&mut self, is_eof: bool, renderer_fade_active: bool) -> bool {
        let should_finalize = match (&self.crossfade.phase, is_eof) {
            (CrossfadePhase::OutgoingFinished { .. }, _) => true,
            (CrossfadePhase::Active { .. }, true) => true,
            (CrossfadePhase::Active { .. }, false) => !renderer_fade_active,
            (CrossfadePhase::Idle, _) => false,
        };

        if should_finalize {
            debug!(
                "🔀 [RENDERER FINISHED] Crossfade ran its course (phase={}, eof={}, renderer_fade_active={}) — finalizing",
                self.crossfade.phase.label(),
                is_eof,
                renderer_fade_active
            );
            self.finalize_crossfade_engine().await;
        }
        should_finalize
    }

    /// Try to start a crossfade transition if conditions are met.
    ///
    /// This is the main crossfade entry point: render_tick's position-based trigger
    /// fired (pos >= track_duration - crossfade_duration), disarmed the trigger,
    /// and signaled us. We start the crossfade from the engine so the decode loop
    /// and stream creation happen together.
    ///
    /// NOTE: Does NOT gate on is_eof — the position-based trigger fires
    /// intentionally BEFORE EOF so both tracks can overlap during the fade.
    async fn try_start_crossfade_transition(&mut self, is_eof: bool) -> bool {
        // Crossfade is eligible when the Crossfade toggle is on OR under Relaxed
        // bit-perfect (which self-crossfades same-rate tracks). Mirror the
        // `store_prepared_decoder` arm condition so both triggers agree.
        if !self.crossfade.phase.is_idle()
            || !self.crossfade.crossfade_eligible()
            || self.crossfade.effective_duration_ms() == 0
        {
            return false;
        }

        // Per-transition policy suppression (M4: album continuation /
        // too-short). Mirrors the gate in `arm_renderer_crossfade` — every
        // arm/trigger site must agree (invariant 4) — so a suppressed
        // transition can never blend through the EOF fallback. Returning
        // false falls through to the normal gapless/track-end path.
        if self.crossfade.suppress_this_transition {
            debug!(
                "🔀 [RENDERER FINISHED] Crossfade SUPPRESSED for this transition \
                 (policy: gapless join)"
            );
            return false;
        }

        // Bit-perfect gates the crossfade per-transition: Strict hard-cuts
        // everything, Relaxed hard-cuts only a cross-format change. Returning
        // false here falls through to the normal gapless/hard-cut path. Gates on
        // the SAME `crossfade_blocked()` the renderer's `arm_crossfade` uses,
        // with the SAME (outgoing, incoming) format pair — `current_format` is
        // the outgoing track, `next_format` the prepared incoming one — so the
        // two triggers can't disagree and start/orphan a blend.
        if self
            .renderer
            .lock()
            .crossfade_blocked(&self.current_format, &self.next_format)
        {
            return false;
        }

        let has_prepared = self.gapless.lock().await.is_prepared();
        if has_prepared {
            debug!(
                "🔀 [RENDERER FINISHED] Starting crossfade (prepared={}, eof={})",
                has_prepared, is_eof
            );
            self.start_crossfade().await;
        }
        has_prepared
    }

    /// Handle temporary buffer starvation when decoder hasn't reached EOF.
    ///
    /// Waits briefly for the decoder to produce more buffers (e.g., after seek
    /// or transient network stall). Returns `true` if the track actually ended.
    async fn handle_buffer_starvation(&mut self, position: u64, duration: u64) -> bool {
        debug!(
            " [RENDERER FINISHED] Buffer starvation detected (pos={}, dur={}), waiting",
            position, duration
        );

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let has_buffers = !self.renderer.lock().is_buffer_queue_empty();
        if has_buffers {
            trace!(" [RENDERER FINISHED] Buffers recovered after wait");
            return false;
        }

        // Still no buffers — re-check decoder state
        let decoder = self.decoder.lock().await;
        let still_eof = decoder.is_eof();
        drop(decoder);

        if still_eof {
            debug!("🎵 [RENDERER FINISHED] Decoder reached EOF after wait, finishing track");
            self.on_decoder_finished().await;
            true
        } else {
            trace!(" [RENDERER FINISHED] Decoder still producing, continuing to wait");
            false
        }
    }

    /// Handle decoder finished (track completed)
    async fn on_decoder_finished(&mut self) {
        debug!(
            "🎵 [DECODER FINISHED] source={}, crossfade_phase={}, playing={}, paused={}",
            redact_subsonic_url(&self.source),
            self.crossfade.phase.label(),
            self.playing,
            self.paused
        );

        // If crossfade is active and the outgoing decoder finished, that's expected.
        // The renderer still has buffered outgoing audio to mix — transition to
        // OutgoingFinished to let the renderer continue the crossfade using
        // already-buffered data. Engine finalization happens when the renderer
        // completes the crossfade (crossfade_done) or the outgoing queue drains.
        match std::mem::replace(&mut self.crossfade.phase, CrossfadePhase::Idle) {
            CrossfadePhase::Active {
                decoder,
                incoming_source,
            } => {
                debug!(
                    "🔀 [DECODER FINISHED] Outgoing EOF during crossfade — phase → OutgoingFinished"
                );
                self.crossfade.phase = CrossfadePhase::OutgoingFinished {
                    decoder,
                    incoming_source,
                };
                return;
            }
            phase @ CrossfadePhase::OutgoingFinished { .. } => {
                debug!("🔀 [DECODER FINISHED] Ignoring — OutgoingFinished, waiting for renderer");
                self.crossfade.phase = phase;
                return;
            }
            CrossfadePhase::Idle => {
                // Already restored to Idle by the mem::replace above; fall
                // through to the normal track-completion path.
            }
        }

        // Snapshot the current source so we can detect repeat-one loops after the
        // completion callback selects the next track.
        let source_before = self.source.clone();

        // Check if we have a prepared next track
        let has_prepared = self.gapless.lock().await.decoder.is_some();

        if has_prepared {
            debug!(" Track finished, loading prepared next track");
            if self.load_prepared_track().await.is_ok() {
                // Gapless transition successful - continue playing
                // NOTE: load_prepared_track() already starts the decoding loop
                // and render thread, so we do NOT call start_decoding_loop() here.
                debug!(
                    " Gapless transition successful (source: {})",
                    redact_subsonic_url(&self.source)
                );
                // IMPORTANT: Still call completion callback so playback controller updates queue index!
                // Gapless always means a new track (we skip same-URL gapless prep), so is_loop=false.
                if let Some(callback) = &self.completion_callback {
                    callback(false);
                }
                return;
            }
            warn!(" Gapless transition failed, falling back to normal next song");
        } else {
            debug!(
                " [DECODER FINISHED] No prepared decoder available — will fall through to stop+callback"
            );
        }

        // No next track prepared, stop and emit finished
        debug!(" No prepared track, stopping playback");
        self.stop().await;
        if let Some(callback) = &self.completion_callback {
            // If the new source equals the old source, this is a repeat-one loop.
            let is_loop = !self.source.is_empty() && self.source == source_before;
            debug!(
                " [DECODER FINISHED] Calling completion callback (is_loop={})",
                is_loop
            );
            callback(is_loop);
        }
    }

    /// Start the dedicated render thread.
    /// With rodio, the actual audio rendering is done by the cpal callback.
    /// This thread just handles control logic: crossfade ticking, completion
    /// detection, etc. Runs at 20ms intervals (50Hz — sufficient for smooth
    /// crossfade curves and responsive completion detection).
    fn start_render_thread(&mut self) {
        // Stop any existing render thread first
        self.stop_render_thread();

        let renderer = self.renderer.clone();
        let running = self.render_running.clone();
        running.store(true, Ordering::Release);

        let handle = std::thread::Builder::new()
            .name("audio-render".into())
            .spawn(move || {
                trace!("🔊 [RENDER THREAD] Started");
                while running.load(Ordering::Acquire) {
                    {
                        let mut r = renderer.lock();
                        r.render_tick();
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                trace!("🔊 [RENDER THREAD] Stopped");
            })
            .expect("Failed to spawn audio render thread");

        self.render_thread = Some(handle);
    }

    /// Stop the dedicated render thread
    fn stop_render_thread(&mut self) {
        self.render_running.store(false, Ordering::Release);
        if let Some(handle) = self.render_thread.take() {
            let _ = handle.join();
        }
    }
}

impl Default for CustomAudioEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for CustomAudioEngine {
    fn drop(&mut self) {
        // Kill the decode loop immediately (lock-free atomic check).
        self.decode_loop.supersede();

        // Stop the render thread — sets render_running=false and joins the OS thread.
        // Without this, the render thread keeps feeding buffered audio to CPAL/PipeWire
        // for several seconds after the window closes.
        self.stop_render_thread();

        // Stop the audio renderer — sets `stopped=true` on the StreamingSourceHandle,
        // causing the CPAL callback to immediately emit silence instead of draining
        // the ring buffer.
        self.renderer.lock().stop();
    }
}

impl CustomAudioEngine {
    /// Signal the engine to stop all active audio work and prepare for process
    /// exit. This is the **async** counterpart to `Drop` — it performs the same
    /// cleanup steps explicitly so the caller can impose a deadline via
    /// `tokio::time::timeout` rather than relying on Drop timing.
    ///
    /// Sequence:
    /// 1. Supersede the decode-loop generation counter — the running loop will
    ///    see the mismatch within its next 5 ms sleep and exit cooperatively.
    /// 2. Stop and join the render std::thread (bounded — the loop sleeps 20 ms
    ///    and checks the atomic flag each tick, so join returns in ≤ 40 ms).
    /// 3. Stop the audio renderer (sets `stopped=true` on every StreamingSource,
    ///    making PipeWire emit silence instead of draining the ring buffer).
    ///
    /// The `AsyncNetworkBuffer` tokio task exits implicitly: once the decode loop
    /// superseded in step 1 stops calling `read_buffer`, the sync channel's
    /// receiver side is effectively drained no further; the F4 `CancellationToken`
    /// fires on the producer side within its next 15 s read timeout. This method
    /// does **not** await that exit — the bounded timeout in the caller handles it.
    ///
    /// Idempotent: calling twice does not panic (supersede is monotonic, joining
    /// a completed thread is a no-op, renderer stop is idempotent).
    pub fn request_shutdown(&mut self) {
        debug!(" [ENGINE] request_shutdown: superseding decode loop");
        self.decode_loop.supersede();

        debug!(" [ENGINE] request_shutdown: stopping render thread");
        self.stop_render_thread();

        debug!(" [ENGINE] request_shutdown: stopping renderer");
        self.renderer.lock().stop();

        debug!(" [ENGINE] request_shutdown: complete");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pulled-queue start-offset lifecycle: `set_source` must clear a staged
    /// offset BEFORE its same-source early return, so a stale offset can
    /// never fire on a later fresh start of a reloaded (or unrelated) track.
    ///
    /// `#[tokio::test]`: `CustomAudioEngine::new()` builds the renderer,
    /// which captures `tokio::runtime::Handle::current()` at construction.
    #[tokio::test]
    async fn pending_start_ms_cleared_by_set_source_including_same_source() {
        let mut engine = CustomAudioEngine::new();

        // A fresh-source load clears a previously-armed offset.
        engine.set_pending_start_ms(5_000);
        engine
            .set_source("http://server/rest/stream?id=a".into(), None)
            .await;
        assert_eq!(
            engine.pending_start_ms, None,
            "fresh load clears the offset"
        );

        // Armed AFTER staging (the cue_pulled_queue order) — it survives
        // until the next play()/set_source.
        engine.set_pending_start_ms(42_000);
        assert_eq!(engine.pending_start_ms, Some(42_000));

        // A same-source reload takes the early return — the offset must
        // STILL clear (the clear sits above the return).
        engine
            .set_source("http://server/rest/stream?id=a".into(), None)
            .await;
        assert_eq!(
            engine.pending_start_ms, None,
            "same-source early return clears too"
        );
    }

    /// Wiring interlock for `DecodeLoopChannels`: after `set_engine_reference`
    /// installs the renderer link, the engine and the renderer must share the
    /// SAME `source_generation` / `decoder_eof` / `stream_is_infinite` handles —
    /// not equal-valued but separately-allocated look-alikes. A fresh
    /// `Arc::new(...)` on either side would compile, lint, and format clean yet
    /// silently break crossfade/EOF gating (the renderer would watch a flag the
    /// decode loop never writes). `Arc::ptr_eq` is the only check that catches it.
    ///
    /// `#[tokio::test]`: `CustomAudioEngine::new()` builds the renderer, which
    /// captures `tokio::runtime::Handle::current()` at construction.
    #[tokio::test]
    async fn set_engine_link_shares_decode_loop_channel_identity() {
        let mut engine = CustomAudioEngine::new();
        // The back-reference is irrelevant to the atomic wiring; a dangling weak
        // is sufficient because this test only inspects shared-handle identity.
        engine.set_engine_reference(Weak::new());

        let renderer = engine.renderer.lock();

        assert!(
            engine
                .channels
                .source_generation
                .ptr_eq(renderer.source_generation_handle()),
            "engine and renderer must share the SAME source_generation Arc"
        );
        assert!(
            Arc::ptr_eq(&engine.channels.decoder_eof, renderer.decoder_eof_handle()),
            "engine and renderer must share the SAME decoder_eof Arc"
        );
        assert!(
            Arc::ptr_eq(
                &engine.channels.stream_is_infinite,
                renderer.stream_is_infinite_handle()
            ),
            "engine and renderer must share the SAME stream_is_infinite Arc"
        );
    }

    /// The decoder now emits full-precision f32 (`RawSampleBuffer::<f32>`) and
    /// `decoded_bytes_to_f32` must reinterpret those bytes losslessly. This pins
    /// the bit-perfect prerequisite: samples that 16-bit quantization (the old
    /// `s16_bytes_to_f32` path) could not represent must survive untouched.
    #[test]
    fn decoded_bytes_to_f32_is_lossless_for_sub_16bit_detail() {
        // Includes values that fall strictly between adjacent 16-bit codes, plus
        // exact endpoints. A 24-bit/float source carries exactly this kind of
        // detail; the former i16 funnel would have flattened it.
        let samples: [f32; 6] = [0.0, 1.0, -1.0, 0.123_456_79, -0.000_001_5, 0.499_998_4];
        let mut bytes = Vec::with_capacity(samples.len() * 4);
        for s in samples {
            bytes.extend_from_slice(&s.to_ne_bytes());
        }

        let round_tripped = decoded_bytes_to_f32(&bytes);
        assert_eq!(round_tripped, samples, "f32 reinterpret must be bit-exact");

        // Contrast: the old S16 path quantized to i16 and back, which is lossy
        // for at least one of these values — proving the change is meaningful.
        let i16_quantized: Vec<f32> = samples
            .iter()
            .map(|&s| ((s.clamp(-1.0, 1.0) * 32768.0) as i16) as f32 / 32768.0)
            .collect();
        assert_ne!(
            round_tripped, i16_quantized,
            "the new path must preserve detail the 16-bit path destroyed"
        );
    }

    /// IG-8: the typestate makes `Idle → OutgoingFinished` direct transition
    /// impossible. Previously the bool-flag representation could be set to
    /// any value at any time; now `OutgoingFinished` only exists by destructuring
    /// `Active` (its `decoder` and `incoming_source` are non-`Default`,
    /// so they can only come from a prior `Active` state).
    ///
    /// This test exercises the runtime side: feeding `on_decoder_finished`
    /// to a fresh engine (which starts in `Idle`) must NOT promote the phase
    /// to `OutgoingFinished` — the match arm in `on_decoder_finished` falls
    /// through to the normal track-completion path instead.
    #[tokio::test]
    async fn crossfade_idle_cannot_transition_directly_to_outgoing_finished() {
        let mut engine = CustomAudioEngine::new();

        assert!(
            engine.crossfade.phase.is_idle(),
            "fresh engine must start in Idle"
        );

        engine.on_decoder_finished().await;

        assert!(
            engine.crossfade.phase.is_idle(),
            "phase must remain Idle when no crossfade is active",
        );
    }

    fn fresh_decoder() -> AudioDecoder {
        AudioDecoder::new(Arc::new(std::sync::RwLock::new(None)))
    }

    /// Pins the `None` contract of the shared decode step: an uninitialized
    /// decoder yields `AudioBuffer::invalid()` from `read_buffer`, which the
    /// prebuffers and decode loops rely on mapping to `None` (stop / EOF /
    /// empty handling) rather than an empty sample vec.
    #[test]
    fn decode_one_chunk_returns_none_for_uninitialized_decoder() {
        let mut decoder = fresh_decoder();
        assert!(decode_one_chunk(&mut decoder).is_none());
    }

    /// N20: seek must NOT bump the source generation — the source URL is
    /// unchanged, so renderer.seek recreates the primary stream under the same
    /// generation (gated by the `seeking` flag + decode_loop.supersede). This
    /// pins the invariant the corrected `bump_for_user_action` doc now
    /// documents, so a future maintainer can't quietly add a spurious bump into
    /// seek and invalidate the render/visualizer staleness gating mid-seek.
    #[tokio::test]
    async fn seek_preserves_source_generation() {
        let mut engine = CustomAudioEngine::new();
        let before = engine.source_generation();
        // Fresh engine has duration 0, so seek returns at its early guard
        // without spawning a decode loop — but crucially without bumping the
        // generation either (the abort path and the real seek path both leave
        // it untouched by design).
        engine.seek(5_000).await;
        assert_eq!(
            engine.source_generation(),
            before,
            "seek must not bump the source generation",
        );
    }

    /// Fade-completes-before-outgoing-EOF: when the renderer has already
    /// finalized its side of the crossfade (elapsed hit the fade duration)
    /// but the outgoing decoder has not reached EOF — e.g. a coarse VBR seek
    /// landed earlier in the audio than the position implied — the engine
    /// must finalize anyway. The outgoing is at zero volume past the fade,
    /// so its tail is inaudible by construction; waiting for EOF leaves a
    /// torn state (renderer promoted, engine Active) where the crossfade
    /// decode loop free-runs the incoming decoder to a stale EOF that then
    /// kills the next track seconds after promotion (observed live:
    /// In the House skipped 33ms after its transition).
    #[tokio::test]
    async fn fade_completed_before_outgoing_eof_finalizes() {
        let mut engine = CustomAudioEngine::new();
        engine.crossfade.phase = CrossfadePhase::Active {
            decoder: Arc::new(tokio::sync::Mutex::new(Some(fresh_decoder()))),
            incoming_source: "http://example.test/next".to_string(),
        };
        // renderer_fade_active=false mirrors the post-render-tick-finalize
        // reality: the renderer already promoted the incoming and went Idle.
        let finalized = engine.try_finalize_crossfade(false, false).await;

        assert!(
            finalized,
            "engine must finalize when the renderer's fade already completed, \
             even though the outgoing decoder is not at EOF"
        );
        assert!(
            engine.crossfade.phase.is_idle(),
            "finalize must clear the engine-side crossfade phase"
        );
    }

    /// OutgoingFinished finalize: once the outgoing decoder has hit EOF the
    /// phase is `OutgoingFinished` and `try_finalize_crossfade` finalizes
    /// unconditionally (the `(OutgoingFinished, _) => true` arm), regardless of
    /// the eof/renderer_fade_active inputs. This is the only `try_finalize`
    /// arm the other crossfade tests don't exercise.
    ///
    /// Critically, the injected decoder is `Some(fresh_decoder())`, NOT `None`:
    /// `finalize_crossfade_engine` `take()`s the inner Option, and a `None`
    /// would short-circuit the whole promotion block — skipping
    /// `accept_internal_swap()` and giving false coverage. With `Some`, the
    /// promotion runs end to end:
    ///   * phase → Idle,
    ///   * `last_transition_was_crossfade` set true (read-reset exactly once),
    ///   * `accept_internal_swap()` is a NO-OP, so `source_generation()` is
    ///     UNCHANGED across the finalize (the crossfade is an intentional
    ///     transition, not a user skip — a stray bump here would invalidate the
    ///     just-promoted incoming source's in-flight render/visualizer
    ///     callbacks).
    #[tokio::test]
    async fn outgoing_finished_finalizes_and_marks_crossfade() {
        let mut engine = CustomAudioEngine::new();
        engine.crossfade.phase = CrossfadePhase::OutgoingFinished {
            decoder: Arc::new(tokio::sync::Mutex::new(Some(fresh_decoder()))),
            incoming_source: "http://example.test/next".to_string(),
        };
        let gen_before = engine.source_generation();

        // eof/renderer_fade_active are irrelevant in OutgoingFinished — it
        // always finalizes. Pass the "least likely to finalize" inputs to prove
        // the arm, not the inputs, drove the decision.
        let finalized = engine.try_finalize_crossfade(false, true).await;

        assert!(
            finalized,
            "OutgoingFinished must finalize unconditionally, regardless of eof / renderer_fade_active"
        );
        assert!(
            engine.crossfade.phase.is_idle(),
            "finalize must clear the engine-side crossfade phase"
        );
        assert_eq!(
            engine.source_generation(),
            gen_before,
            "accept_internal_swap is a no-op: the internal crossfade promotion must NOT bump the source generation"
        );
        assert!(
            engine.take_last_transition_was_crossfade(),
            "finalize must record the advance as a crossfade transition"
        );
        // take_* read-resets, so the flag is now false: a second read must NOT
        // still report a crossfade (otherwise it would leak into the next
        // gapless transition's log label).
        assert!(
            !engine.take_last_transition_was_crossfade(),
            "the crossfade-transition flag must read-reset after a single take"
        );
    }

    /// `decode_buffer_size` characterization for full-precision F32 stereo
    /// (8 bytes/frame): one ~100ms chunk is `0.1s * sample_rate * 8`, clamped
    /// to [4096, 65_536]. The 96k case lands at 76_800 raw and must clamp DOWN
    /// to the 65_536 ceiling — a clamp-unaware expectation (76_800) would
    /// wrongly fail, and a lower ceiling would silently shrink hi-res reads and
    /// force the decode loop to run more iterations to keep pace.
    #[test]
    fn decode_buffer_size_f32_stereo_clamps_at_ceiling() {
        use crate::audio::format::SampleFormat;

        let buf = |rate: u32| decode_buffer_size(&AudioFormat::new(SampleFormat::F32, rate, 2));

        // 0.1s * 44100 * 8 = 35_280 (within range, unclamped)
        assert_eq!(buf(44_100), 35_280);
        // 0.1s * 48000 * 8 = 38_400 (within range, unclamped)
        assert_eq!(buf(48_000), 38_400);
        // 0.1s * 96000 * 8 = 76_800 → clamped to the 65_536 ceiling
        assert_eq!(buf(96_000), 65_536);
    }

    /// Counterpart guard: while the renderer's fade is STILL ACTIVE, an
    /// on_renderer_finished with eof=false (e.g. transient outgoing-ring
    /// starvation mid-fade) must NOT finalize early.
    #[tokio::test]
    async fn fade_still_running_does_not_finalize_without_eof() {
        let mut engine = CustomAudioEngine::new();
        engine.crossfade.phase = CrossfadePhase::Active {
            decoder: Arc::new(tokio::sync::Mutex::new(Some(fresh_decoder()))),
            incoming_source: "http://example.test/next".to_string(),
        };

        let finalized = engine.try_finalize_crossfade(false, true).await;

        assert!(
            !finalized,
            "a mid-fade starvation signal must not finalize the crossfade"
        );
        assert!(
            !engine.crossfade.phase.is_idle(),
            "the engine-side phase must stay Active while the fade runs"
        );
    }

    /// N4: toggling shuffle / repeat / consume mid-crossfade must cancel the
    /// in-flight fade, not just clear the gapless slot. Previously
    /// reset_next_track only cleared the gapless slot + disarmed the Armed
    /// flag, leaving an Active crossfade running so finalize_crossfade_engine
    /// would still promote the (now wrong-under-new-mode) incoming track.
    #[tokio::test]
    async fn reset_next_track_cancels_active_crossfade() {
        let mut engine = CustomAudioEngine::new();
        engine.crossfade.phase = CrossfadePhase::Active {
            decoder: Arc::new(tokio::sync::Mutex::new(Some(fresh_decoder()))),
            incoming_source: "http://example.test/next".to_string(),
        };

        assert!(
            !engine.crossfade.phase.is_idle(),
            "precondition: engine must start mid-crossfade",
        );

        engine.reset_next_track().await;

        assert!(
            engine.crossfade.phase.is_idle(),
            "reset_next_track must cancel the active crossfade (phase → Idle)",
        );
    }

    /// M4: the per-transition policy flag lifecycle. `store_prepared_decoder`
    /// sets the flag from its argument, a suppressed prepare must NOT arm the
    /// renderer (while the gapless slot stays prepared — invariant 10: the
    /// suppressed transition falls through to the gapless swap, it doesn't
    /// fight it), and `reset_next_track` clears the flag.
    #[tokio::test]
    async fn suppressed_prepare_sets_flag_keeps_gapless_and_does_not_arm() {
        let mut engine = CustomAudioEngine::new();
        engine.crossfade.enabled = true;
        engine.crossfade.duration_ms = 5_000;
        engine.duration = 200_000;
        let mut decoder = fresh_decoder();
        decoder.set_duration_for_test(15_000);

        engine
            .store_prepared_decoder(
                decoder,
                "http://example.test/next".to_string(),
                None,
                PreparedTransitionDirectives::from_suppress(true),
            )
            .await;

        assert!(
            engine.crossfade.suppress_this_transition,
            "store_prepared_decoder must set the suppress flag from its argument"
        );
        assert!(
            !engine.renderer.lock().is_crossfade_armed(),
            "a suppressed transition must not arm the renderer crossfade"
        );
        assert!(
            engine.gapless.lock().await.is_prepared(),
            "the gapless slot must stay prepared — suppression means hard-JOIN, not hard-cut"
        );

        engine.reset_next_track().await;
        assert!(
            !engine.crossfade.suppress_this_transition,
            "reset_next_track must clear the suppress flag"
        );
    }

    /// M4: the flag re-derives on EVERY prepare, in both directions. The
    /// ordering trap this pins: `store_prepared_decoder` runs an internal
    /// `reset_next_track` (which clears the flag) partway through — a flag
    /// set before that reset would be silently wiped, losing a suppress
    /// verdict (an album segue would blend) or stranding a stale one (the
    /// next transition could never crossfade again).
    #[tokio::test]
    async fn repreparation_rederives_suppress_flag_in_both_directions() {
        let mut engine = CustomAudioEngine::new();
        engine.crossfade.enabled = true;
        engine.crossfade.duration_ms = 5_000;
        engine.duration = 200_000;

        let mut dec_a = fresh_decoder();
        dec_a.set_duration_for_test(15_000);
        engine
            .store_prepared_decoder(
                dec_a,
                "http://example.test/a".to_string(),
                None,
                PreparedTransitionDirectives::from_suppress(false),
            )
            .await;
        assert!(!engine.crossfade.suppress_this_transition);
        assert!(
            engine.renderer.lock().is_crossfade_armed(),
            "an unsuppressed prepare arms as before"
        );

        // A different next track re-preps with a SUPPRESS verdict: the new
        // flag must survive store's internal reset_next_track, and the stale
        // arm from prep A must not survive either.
        let mut dec_b = fresh_decoder();
        dec_b.set_duration_for_test(15_000);
        engine
            .store_prepared_decoder(
                dec_b,
                "http://example.test/b".to_string(),
                None,
                PreparedTransitionDirectives::from_suppress(true),
            )
            .await;
        assert!(
            engine.crossfade.suppress_this_transition,
            "the suppress verdict must survive store's internal reset_next_track"
        );
        assert!(!engine.renderer.lock().is_crossfade_armed());

        // And back: a suppressed pair followed by a blendable pair re-arms —
        // the flag is per-transition, never sticky.
        let mut dec_c = fresh_decoder();
        dec_c.set_duration_for_test(15_000);
        engine
            .store_prepared_decoder(
                dec_c,
                "http://example.test/c".to_string(),
                None,
                PreparedTransitionDirectives::from_suppress(false),
            )
            .await;
        assert!(!engine.crossfade.suppress_this_transition);
        assert!(
            engine.renderer.lock().is_crossfade_armed(),
            "the transition after a suppressed one must be able to crossfade again"
        );
    }

    /// M4: the EOF-fallback trigger gates on the suppress flag too
    /// (invariant 4 — every arm/trigger site must agree, exactly like the
    /// `crossfade_blocked` pairing): with an eligible mode and a prepared
    /// slot, a suppressed transition returns `false` so the completion path
    /// falls through to the normal gapless/track-end handling.
    #[tokio::test]
    async fn try_start_crossfade_transition_suppressed_returns_false() {
        let mut engine = CustomAudioEngine::new();
        engine.crossfade.enabled = true;
        engine.crossfade.duration_ms = 5_000;
        engine.duration = 200_000;
        engine.next_format = AudioFormat::new(crate::audio::format::SampleFormat::S16, 44_100, 2);
        {
            let mut slot = engine.gapless.lock().await;
            slot.decoder = Some(fresh_decoder());
            slot.source = "http://example.test/next".to_string();
            slot.prepared = true;
        }
        engine.crossfade.suppress_this_transition = true;

        let started = engine.try_start_crossfade_transition(false).await;

        assert!(
            !started,
            "a suppressed transition must fall through to the gapless path"
        );
        assert!(engine.crossfade.phase.is_idle());
    }

    /// M4: the configured minimum-track floor reaches the renderer's arm gate
    /// through the engine setter — a raised floor refuses a prepare the
    /// default 10s floor accepts (end-to-end pin for the settings push).
    #[tokio::test]
    async fn store_prepared_decoder_honors_configured_min_track_floor() {
        let mut engine = CustomAudioEngine::new();
        engine.crossfade.enabled = true;
        engine.crossfade.duration_ms = 5_000;
        engine.duration = 200_000;
        engine.set_crossfade_min_track_secs(30);

        let mut decoder = fresh_decoder();
        decoder.set_duration_for_test(15_000);
        engine
            .store_prepared_decoder(
                decoder,
                "http://example.test/next".to_string(),
                None,
                PreparedTransitionDirectives::from_suppress(false),
            )
            .await;

        assert!(
            !engine.renderer.lock().is_crossfade_armed(),
            "a 15s incoming track under a 30s configured floor must not arm"
        );
    }

    /// M4: toggling the album-continuity gate mid-transition must abandon the
    /// prepared/armed/in-flight transition (the shuffle/repeat/consume /
    /// crossfade-enable / bit-perfect mode-toggle contract): the toggle flips
    /// the policy verdict for the prepared pair, and the next prep re-derives
    /// it under the new setting.
    #[tokio::test]
    async fn set_crossfade_album_gapless_change_cancels_active_crossfade() {
        let mut engine = CustomAudioEngine::new();
        engine.crossfade.phase = CrossfadePhase::Active {
            decoder: Arc::new(tokio::sync::Mutex::new(Some(fresh_decoder()))),
            incoming_source: "http://example.test/next".to_string(),
        };

        engine.set_crossfade_album_gapless(true).await;

        assert!(
            engine.crossfade.phase.is_idle(),
            "a REAL album-gate change must cancel the in-flight transition"
        );
        assert!(engine.crossfade.album_continuity);
    }

    /// M4: an UNCHANGED album-gate value (routine settings save re-applies
    /// every field) must not disturb an in-flight transition.
    #[tokio::test]
    async fn set_crossfade_album_gapless_unchanged_is_noop() {
        let mut engine = CustomAudioEngine::new();
        engine.crossfade.phase = CrossfadePhase::Active {
            decoder: Arc::new(tokio::sync::Mutex::new(Some(fresh_decoder()))),
            incoming_source: "http://example.test/next".to_string(),
        };

        // album_continuity already defaults false; re-applying false is a no-op.
        engine.set_crossfade_album_gapless(false).await;

        assert!(
            !engine.crossfade.phase.is_idle(),
            "an unchanged album-gate value must not cancel the blend"
        );
    }

    /// M8 bar-snap override lifecycle: `store_prepared_decoder` stages the
    /// override + gap (after its internal reset), mirrors the override into
    /// `crossfade_duration_shared` (invariant 9 — the watermark cushion must
    /// cover the duration that will actually play), leaves the GLOBAL setting
    /// untouched, and arms the renderer with the OVERRIDE. `reset_next_track`
    /// restores all of it.
    #[tokio::test]
    async fn store_prepared_decoder_applies_override_and_gap_then_reset_restores() {
        let mut engine = CustomAudioEngine::new();
        engine.crossfade.enabled = true;
        engine.set_crossfade_duration(7);
        engine.duration = 200_000;

        let mut decoder = fresh_decoder();
        decoder.set_duration_for_test(150_000);
        engine
            .store_prepared_decoder(
                decoder,
                "http://example.test/next".to_string(),
                None,
                PreparedTransitionDirectives {
                    suppress_crossfade: false,
                    duration_override_ms: Some(4_000),
                    gap_offset_ms: 1_500,
                },
            )
            .await;

        assert_eq!(engine.crossfade.duration_override_ms, Some(4_000));
        assert_eq!(
            engine.crossfade.duration_ms, 7_000,
            "the global setting must survive a per-transition override"
        );
        assert_eq!(
            engine
                .channels
                .crossfade_duration_shared
                .load(Ordering::Relaxed),
            4_000,
            "the decode-loop watermark mirror must follow the override (invariant 9)"
        );
        assert_eq!(
            engine.channels.gap_offset_ms.load(Ordering::Relaxed),
            1_500,
            "the pending transition gap must be staged for the decode loop"
        );
        assert_eq!(
            engine.renderer.lock().armed_duration_ms_for_test(),
            Some(4_000),
            "the renderer must be armed with the snapped duration, not the global"
        );

        engine.reset_next_track().await;
        assert_eq!(engine.crossfade.duration_override_ms, None);
        assert_eq!(
            engine
                .channels
                .crossfade_duration_shared
                .load(Ordering::Relaxed),
            7_000,
            "reset must restore the shared mirror to the global setting"
        );
        assert_eq!(
            engine.channels.gap_offset_ms.load(Ordering::Relaxed),
            0,
            "reset must void the pending transition gap"
        );
    }

    /// M8: a mid-override duration-slider change updates the GLOBAL but must
    /// not clobber the live override's shared mirror; the next reset restores
    /// the shared mirror to the NEW global.
    #[tokio::test]
    async fn set_crossfade_duration_defers_to_live_override() {
        let mut engine = CustomAudioEngine::new();
        engine.crossfade.enabled = true;
        engine.set_crossfade_duration(7);
        engine.duration = 200_000;
        let mut decoder = fresh_decoder();
        decoder.set_duration_for_test(150_000);
        engine
            .store_prepared_decoder(
                decoder,
                "http://example.test/next".to_string(),
                None,
                PreparedTransitionDirectives {
                    suppress_crossfade: false,
                    duration_override_ms: Some(4_000),
                    gap_offset_ms: 0,
                },
            )
            .await;

        engine.set_crossfade_duration(9);
        assert_eq!(engine.crossfade.duration_ms, 9_000);
        assert_eq!(
            engine
                .channels
                .crossfade_duration_shared
                .load(Ordering::Relaxed),
            4_000,
            "a slider change must not clobber a live per-transition override"
        );

        engine.reset_next_track().await;
        assert_eq!(
            engine
                .channels
                .crossfade_duration_shared
                .load(Ordering::Relaxed),
            9_000,
            "after the override clears, the shared mirror follows the new global"
        );
    }

    /// M8 "Gap / Overlap Trim" setter: clamps to ±2 s, pushes the NEGATIVE
    /// side to the renderer's Armed-trigger lead, and surfaces the POSITIVE
    /// side through `transition_prep_cfg` (each side exactly one consumer).
    #[tokio::test]
    async fn set_crossfade_offset_clamps_and_routes_both_sides() {
        let mut engine = CustomAudioEngine::new();

        engine.set_crossfade_offset(-5);
        assert_eq!(
            engine.renderer.lock().crossfade_lead_ms_for_test(),
            2_000,
            "-5s must clamp to the -2s floor and push a 2s lead"
        );
        assert_eq!(engine.transition_prep_cfg().gap_offset_ms, 0);

        engine.set_crossfade_offset(1);
        assert_eq!(
            engine.renderer.lock().crossfade_lead_ms_for_test(),
            0,
            "a positive offset must clear the renderer lead"
        );
        assert_eq!(engine.transition_prep_cfg().gap_offset_ms, 1_000);

        engine.set_crossfade_offset(5);
        assert_eq!(engine.transition_prep_cfg().gap_offset_ms, 2_000);
    }

    /// M8: toggling "Skip Silence Between Tracks" follows the mode-toggle
    /// contract (reset on real change — the prepared decoder was built with
    /// the OLD trim verdict; no-op when unchanged) and pushes the renderer
    /// mirror.
    #[tokio::test]
    async fn set_skip_silence_change_cancels_active_and_pushes_mirror() {
        let mut engine = CustomAudioEngine::new();
        engine.crossfade.phase = CrossfadePhase::Active {
            decoder: Arc::new(tokio::sync::Mutex::new(Some(fresh_decoder()))),
            incoming_source: "http://example.test/next".to_string(),
        };

        engine.set_skip_silence(true).await;
        assert!(
            engine.crossfade.phase.is_idle(),
            "a REAL skip-silence change must cancel the in-flight transition"
        );
        assert!(engine.renderer.lock().skip_silence_for_test());

        engine.crossfade.phase = CrossfadePhase::Active {
            decoder: Arc::new(tokio::sync::Mutex::new(Some(fresh_decoder()))),
            incoming_source: "http://example.test/next".to_string(),
        };
        engine.set_skip_silence(true).await;
        assert!(
            !engine.crossfade.phase.is_idle(),
            "an unchanged skip-silence value must not cancel the blend"
        );
    }

    /// M8: toggling "Snap Crossfade to Musical Bars" follows the same
    /// mode-toggle contract (the prepared pair's effective duration changes).
    #[tokio::test]
    async fn set_crossfade_bar_snap_change_cancels_active_and_noop_when_unchanged() {
        let mut engine = CustomAudioEngine::new();
        engine.crossfade.phase = CrossfadePhase::Active {
            decoder: Arc::new(tokio::sync::Mutex::new(Some(fresh_decoder()))),
            incoming_source: "http://example.test/next".to_string(),
        };

        engine.set_crossfade_bar_snap(true).await;
        assert!(
            engine.crossfade.phase.is_idle(),
            "a REAL bar-snap change must cancel the in-flight transition"
        );
        assert!(engine.crossfade.bar_snap);

        engine.crossfade.phase = CrossfadePhase::Active {
            decoder: Arc::new(tokio::sync::Mutex::new(Some(fresh_decoder()))),
            incoming_source: "http://example.test/next".to_string(),
        };
        engine.set_crossfade_bar_snap(true).await;
        assert!(
            !engine.crossfade.phase.is_idle(),
            "an unchanged bar-snap value must not cancel the blend"
        );
    }

    /// M8: the prep-time leading-trim verdict stands down under bit-perfect
    /// modes (dropping decoded samples is a content change).
    #[tokio::test]
    async fn transition_prep_cfg_gates_leading_trim_under_bit_perfect() {
        let mut engine = CustomAudioEngine::new();
        engine.set_skip_silence(true).await;
        assert!(engine.transition_prep_cfg().trim_leading_silence);

        engine
            .set_bit_perfect(crate::types::player_settings::BitPerfectMode::Strict)
            .await;
        assert!(
            !engine.transition_prep_cfg().trim_leading_silence,
            "Strict must gate the leading trim off"
        );
        engine
            .set_bit_perfect(crate::types::player_settings::BitPerfectMode::Relaxed)
            .await;
        assert!(
            !engine.transition_prep_cfg().trim_leading_silence,
            "Relaxed must gate the leading trim off too"
        );
    }

    /// M8 gap injection: the pending gap becomes exactly `gap` ms of ring
    /// silence at the outgoing's frame rate, and the one-shot value is
    /// consumed.
    #[tokio::test(flavor = "multi_thread")]
    async fn inject_transition_gap_writes_silence_and_consumes_value() {
        let engine = CustomAudioEngine::new();
        let (_src, _handle) = engine.renderer.lock().force_primary_stream_for_test(0);
        let gap = Arc::new(AtomicU64::new(500));
        let skip = Arc::new(AtomicU64::new(NO_SKIP_FADE_PENDING));
        let source_generation = SourceGeneration::new();
        let decode_gen = DecodeLoopHandle::new();
        let my_gen = decode_gen.current();
        let notify = Arc::new(Notify::new());

        inject_transition_gap(
            &engine.renderer,
            &gap,
            &skip,
            &source_generation,
            &decode_gen,
            my_gen,
            96_000, // 48 kHz stereo
            &notify,
        )
        .await;

        assert_eq!(
            engine.renderer.lock().buffer_count(),
            48_000,
            "500ms at 96k samples/s must land as 48_000 silence samples"
        );
        assert_eq!(gap.load(Ordering::Relaxed), 0, "the gap is one-shot");
    }

    /// M8 gap injection stands down (but still consumes the one-shot) when a
    /// crossfade owns the transition or a skip-fade plan window is open.
    #[tokio::test(flavor = "multi_thread")]
    async fn inject_transition_gap_stands_down_for_crossfade_and_skip_window() {
        let engine = CustomAudioEngine::new();
        let (_src, _handle) = engine.renderer.lock().force_primary_stream_for_test(0);
        let source_generation = SourceGeneration::new();
        let decode_gen = DecodeLoopHandle::new();
        let my_gen = decode_gen.current();
        let notify = Arc::new(Notify::new());

        // Armed crossfade — the blend owns the transition.
        engine.renderer.lock().arm_crossfade(
            5_000,
            &AudioFormat::new(crate::audio::format::SampleFormat::F32, 48_000, 2),
            200_000,
            200_000,
        );
        assert!(engine.renderer.lock().is_crossfade_armed());
        let gap = Arc::new(AtomicU64::new(500));
        let skip = Arc::new(AtomicU64::new(NO_SKIP_FADE_PENDING));
        inject_transition_gap(
            &engine.renderer,
            &gap,
            &skip,
            &source_generation,
            &decode_gen,
            my_gen,
            96_000,
            &notify,
        )
        .await;
        assert_eq!(
            engine.renderer.lock().buffer_count(),
            0,
            "an armed crossfade owns the transition — no silence injected"
        );
        assert_eq!(gap.load(Ordering::Relaxed), 0, "still consumed one-shot");

        // Skip-fade window open (latch == live generation).
        engine.renderer.lock().disarm_crossfade();
        let gap = Arc::new(AtomicU64::new(500));
        let skip = Arc::new(AtomicU64::new(source_generation.current()));
        inject_transition_gap(
            &engine.renderer,
            &gap,
            &skip,
            &source_generation,
            &decode_gen,
            my_gen,
            96_000,
            &notify,
        )
        .await;
        assert_eq!(
            engine.renderer.lock().buffer_count(),
            0,
            "an open skip-fade window owns the transition — no silence injected"
        );
        assert_eq!(gap.load(Ordering::Relaxed), 0);
    }

    /// `is_crossfade_live` reconciles the one-tick window where the renderer has
    /// already gone Active (render_tick swaps Armed → Active synchronously and
    /// creates the live incoming stream) but the spawned `on_renderer_finished`
    /// task hasn't yet set the engine phase. With the engine phase STILL Idle and
    /// the renderer Active, `is_crossfade_live` must report `true` — otherwise
    /// `reset_next_track` would skip the cancel (engine still Idle) and orphan the
    /// renderer's live incoming stream, fading the outgoing into silence.
    ///
    /// This is the predicate that must stay DISTINCT from
    /// `try_finalize_crossfade`'s renderer-Active-ONLY `renderer_fade_active`
    /// input: `is_crossfade_live` is engine-not-Idle OR renderer-Active.
    #[tokio::test]
    async fn is_crossfade_live_true_when_renderer_active_engine_idle() {
        let engine = CustomAudioEngine::new();

        // Engine phase is Idle on a fresh engine.
        assert!(
            engine.crossfade.phase.is_idle(),
            "precondition: engine phase must be Idle",
        );

        // Force ONLY the renderer side Active (keep the throwaway source alive so
        // the stream handle's atomics stay valid for the duration of the read).
        let _keepalive = engine.renderer.lock().force_crossfade_active_for_test();

        assert!(
            engine.crossfade.is_crossfade_live(&engine.renderer),
            "engine phase Idle + renderer Active must read as a LIVE crossfade",
        );
    }

    /// Toggling the Crossfade SETTING mid-transition must abandon the
    /// prepared/armed/in-flight transition, exactly like bit-perfect below:
    /// it flips `crossfade_eligible`, and the renderer's armed trigger fires
    /// on position alone — a blend armed under the old setting would start
    /// against an engine gate that now refuses (routing into the
    /// buffer-starvation wait), orphaning a silent incoming stream.
    #[tokio::test]
    async fn set_crossfade_enabled_change_cancels_active_crossfade() {
        let mut engine = CustomAudioEngine::new();
        engine.crossfade.enabled = true;
        engine.crossfade.phase = CrossfadePhase::Active {
            decoder: Arc::new(tokio::sync::Mutex::new(Some(fresh_decoder()))),
            incoming_source: "http://example.test/next".to_string(),
        };

        engine.set_crossfade_enabled(false).await;

        assert!(
            engine.crossfade.phase.is_idle(),
            "a REAL crossfade-setting change must cancel the in-flight transition",
        );
        assert!(!engine.crossfade.enabled);
    }

    /// A settings save re-applies every field; an UNCHANGED crossfade value
    /// must not disturb an in-flight transition (same no-op contract as
    /// `set_bit_perfect`).
    #[tokio::test]
    async fn set_crossfade_enabled_unchanged_is_noop() {
        let mut engine = CustomAudioEngine::new();
        engine.crossfade.enabled = true;
        engine.crossfade.phase = CrossfadePhase::Active {
            decoder: Arc::new(tokio::sync::Mutex::new(Some(fresh_decoder()))),
            incoming_source: "http://example.test/next".to_string(),
        };

        engine.set_crossfade_enabled(true).await;

        assert!(
            !engine.crossfade.phase.is_idle(),
            "an unchanged value (routine settings save) must not cancel the blend",
        );
    }

    // ═════════════════════════════════════════════════════════════════════
    //  M5 — transport fades: engine pause/stop wiring
    // ═════════════════════════════════════════════════════════════════════

    /// M5: with "Fade on Pause / Resume" enabled, `engine.pause()` captures
    /// position + flips its own state immediately (UI shows paused at once)
    /// but hands the renderer the out-ramp instead of the instant
    /// stream-level pause.
    #[tokio::test]
    async fn pause_with_fade_hands_renderer_the_out_ramp() {
        let mut engine = CustomAudioEngine::new();
        engine.set_transport_fades(true, 100, false, 100);
        let (_src, handle) = engine.renderer.lock().force_primary_stream_for_test(4_096);
        handle.set_volume(1.0);
        engine.playing = true;

        engine.pause();

        assert!(engine.paused && !engine.playing);
        assert!(matches!(engine.state, PlaybackState::Paused));
        assert!(
            engine.renderer.lock().transport_fade_is_fading_out(),
            "pause must hand the renderer the out-ramp"
        );
        assert!(
            !handle.paused.load(Ordering::Acquire),
            "the stream-level pause is deferred to ramp completion"
        );
    }

    /// M5 default: with the fade OFF (shipped default), `engine.pause()`
    /// stays the instant stream-level flip it is today.
    #[tokio::test]
    async fn pause_without_fade_pauses_stream_instantly() {
        let mut engine = CustomAudioEngine::new();
        let (_src, handle) = engine.renderer.lock().force_primary_stream_for_test(4_096);
        engine.playing = true;

        engine.pause();

        assert!(
            handle.paused.load(Ordering::Acquire),
            "default pause must flip the stream atomic immediately"
        );
        assert!(engine.renderer.lock().transport_fade_idle());
    }

    /// M5 stop ordering: the out-ramp must run BEFORE `stop_render_thread()`
    /// — the render thread is what drives the ramp. A completion count of 1
    /// proves the ramp was ticked to its end by a LIVE render thread during
    /// `stop()`'s bounded wait; tearing the thread down first would strand
    /// the ramp and burn the full timeout with zero completions.
    #[tokio::test(flavor = "multi_thread")]
    async fn stop_fade_completes_before_teardown_via_live_render_thread() {
        let mut engine = CustomAudioEngine::new();
        engine.set_transport_fades(false, 100, true, 60);
        let (_src, handle) = engine.renderer.lock().force_primary_stream_for_test(48_000);
        handle.set_volume(1.0);
        engine.playing = true;
        engine.start_render_thread();

        engine.stop().await;

        assert_eq!(
            engine.renderer.lock().transport_fade_completions(),
            1,
            "the stop ramp must complete via live render ticks before teardown"
        );
        assert!(matches!(engine.state, PlaybackState::Stopped));
        assert!(
            handle.stopped.load(Ordering::Acquire),
            "teardown must still stop the stream after the ramp"
        );
    }

    /// M5: stop-from-pause has nothing audible to fade — `render_tick`'s
    /// early-return froze the stream long ago — so `stop()` must go straight
    /// to teardown instead of waiting out a ramp that can't play.
    #[tokio::test]
    async fn stop_from_paused_skips_the_ramp() {
        let mut engine = CustomAudioEngine::new();
        engine.set_transport_fades(false, 100, true, 400);
        let (_src, _handle) = engine.renderer.lock().force_primary_stream_for_test(4_096);
        engine.playing = false;
        engine.paused = true;

        let t0 = std::time::Instant::now();
        engine.stop().await;

        assert!(
            t0.elapsed() < std::time::Duration::from_millis(300),
            "stop-from-pause must not wait for a {}ms ramp (took {:?})",
            400,
            t0.elapsed()
        );
        assert_eq!(engine.renderer.lock().transport_fade_completions(), 0);
        assert!(matches!(engine.state, PlaybackState::Stopped));
    }

    /// M5 scope pin: the internal stop inside `set_source` (every track
    /// change while playing) must NOT take the stop fade — M7 owns skip
    /// fades, and "Fade on Stop" means the user-facing stop. With no render
    /// thread running, a leaked fade would burn the full bounded wait
    /// (~650 ms); the no-fade path returns immediately.
    #[tokio::test]
    async fn set_source_track_change_keeps_instant_stop() {
        let mut engine = CustomAudioEngine::new();
        engine.set_transport_fades(false, 100, true, 400);
        let (_src, _handle) = engine.renderer.lock().force_primary_stream_for_test(4_096);
        engine.playing = true;
        engine.source = "http://example.test/old".to_string();

        let t0 = std::time::Instant::now();
        engine
            .set_source("http://example.test/new".to_string(), None)
            .await;

        assert!(
            t0.elapsed() < std::time::Duration::from_millis(300),
            "set_source's internal stop must skip the stop fade (took {:?})",
            t0.elapsed()
        );
    }

    // ═════════════════════════════════════════════════════════════════════
    //  M6 — radio-switch fade: engine wiring
    // ═════════════════════════════════════════════════════════════════════

    /// M6 queue→radio edge: with "Fade Radio Switches" on and something
    /// audibly playing, `stop_for_radio_switch()` (the UI radio-start paths)
    /// must run the switch out-ramp to completion via live render ticks
    /// BEFORE teardown, then arm the renderer's first-audio fade-in for the
    /// radio stream the upcoming `set_source` + `play` will build.
    #[tokio::test(flavor = "multi_thread")]
    async fn radio_switch_stop_fades_out_and_arms_first_audio_fade_in() {
        let mut engine = CustomAudioEngine::new();
        engine.set_fade_radio_transitions(true);
        let (_src, handle) = engine.renderer.lock().force_primary_stream_for_test(48_000);
        handle.set_volume(1.0);
        engine.playing = true;
        engine.start_render_thread();

        engine.stop_for_radio_switch().await;

        assert_eq!(
            engine.renderer.lock().transport_fade_completions(),
            1,
            "the switch out-ramp must complete via live render ticks before teardown"
        );
        assert!(matches!(engine.state, PlaybackState::Stopped));
        assert!(
            handle.stopped.load(Ordering::Acquire),
            "teardown must still stop the stream after the ramp"
        );
        assert!(
            engine.renderer.lock().switch_fade_in_pending(),
            "the switch must arm the first-audio fade-in for the upcoming source"
        );
    }

    /// M6 default pin: with "Fade Radio Switches" off (shipped default),
    /// `stop_for_radio_switch()` is byte-identical to the historical
    /// explicit `stop()` these call sites used — instant, no ramp, no
    /// pending fade-in.
    #[tokio::test]
    async fn radio_switch_stop_without_flag_is_plain_instant_stop() {
        let mut engine = CustomAudioEngine::new();
        let (_src, _handle) = engine.renderer.lock().force_primary_stream_for_test(4_096);
        engine.playing = true;

        let t0 = std::time::Instant::now();
        engine.stop_for_radio_switch().await;

        assert!(
            t0.elapsed() < std::time::Duration::from_millis(300),
            "the default switch stop must not wait out a ramp (took {:?})",
            t0.elapsed()
        );
        assert_eq!(engine.renderer.lock().transport_fade_completions(), 0);
        assert!(!engine.renderer.lock().switch_fade_in_pending());
        assert!(matches!(engine.state, PlaybackState::Stopped));
    }

    /// M6 radio→queue edge: leaving a PLAYING radio stream (the engine knows
    /// via `stream_is_infinite`) for a new source must fade the radio out via
    /// live render ticks inside `set_source`'s internal stop, then arm the
    /// first-audio fade-in for the queue stream about to be built.
    #[tokio::test(flavor = "multi_thread")]
    async fn set_source_fades_out_playing_radio_and_arms_fade_in() {
        let mut engine = CustomAudioEngine::new();
        engine.set_fade_radio_transitions(true);
        let (_src, handle) = engine.renderer.lock().force_primary_stream_for_test(48_000);
        handle.set_volume(1.0);
        engine.playing = true;
        engine.source = "http://radio.test/stream".to_string();
        engine
            .channels
            .stream_is_infinite
            .store(true, Ordering::Release);
        engine.start_render_thread();

        engine
            .set_source("http://example.test/next-track".to_string(), None)
            .await;

        assert_eq!(
            engine.renderer.lock().transport_fade_completions(),
            1,
            "leaving a playing radio stream must fade it out before the internal stop"
        );
        assert!(
            engine.renderer.lock().switch_fade_in_pending(),
            "the radio→queue switch must arm the first-audio fade-in"
        );
    }

    /// M6 scope pin: with "Fade Radio Switches" ON but a FINITE current
    /// stream, `set_source` keeps its instant internal stop — the radio flag
    /// must not leak fades into ordinary track changes (skip-transition
    /// fades are M7's domain).
    #[tokio::test]
    async fn set_source_finite_source_keeps_instant_stop_with_radio_fade_on() {
        let mut engine = CustomAudioEngine::new();
        engine.set_fade_radio_transitions(true);
        let (_src, _handle) = engine.renderer.lock().force_primary_stream_for_test(4_096);
        engine.playing = true;
        engine.source = "http://example.test/old".to_string();

        let t0 = std::time::Instant::now();
        engine
            .set_source("http://example.test/new".to_string(), None)
            .await;

        assert!(
            t0.elapsed() < std::time::Duration::from_millis(300),
            "a finite→finite track change must keep the instant stop (took {:?})",
            t0.elapsed()
        );
        assert_eq!(engine.renderer.lock().transport_fade_completions(), 0);
        assert!(!engine.renderer.lock().switch_fade_in_pending());
    }

    // ═════════════════════════════════════════════════════════════════════
    //  M7 — manual-skip fades: crossfade_to_next + boundary fallback
    // ═════════════════════════════════════════════════════════════════════

    /// A test decoder whose duration + format clear the skip-fade gates.
    fn skip_ready_decoder(duration_ms: u64) -> AudioDecoder {
        use crate::audio::format::SampleFormat;
        let mut d = fresh_decoder();
        d.set_duration_for_test(duration_ms);
        d.set_format_for_test(AudioFormat::new(SampleFormat::S16, 48_000, 2));
        d
    }

    /// Prime an engine as "audibly playing a finite 4-minute track" so the
    /// skip-fade viability + duration gates can pass.
    fn prime_playing_engine(engine: &mut CustomAudioEngine) {
        use crate::audio::format::SampleFormat;
        engine.playing = true;
        engine.paused = false;
        engine.duration = 240_000;
        engine.source = "http://example.test/current".to_string();
        engine.current_format = AudioFormat::new(SampleFormat::S16, 48_000, 2);
    }

    /// M7 duration math: normal case passes the requested length through;
    /// the `shorter/2` clamp and the remaining-audio clamp shrink it; the
    /// min-track floor and unknown durations refuse outright.
    #[test]
    fn skip_fade_duration_gates_and_clamps() {
        // Normal: 2s fade, both tracks long, mid-track position.
        assert_eq!(
            skip_fade_duration_ms(2_000, 240_000, 180_000, 100_000, 10_000),
            Some(2_000)
        );
        // shorter/2 clamp: 4s requested, shorter track 6s (floor 0) → 3s.
        assert_eq!(
            skip_fade_duration_ms(4_000, 240_000, 6_000, 0, 0),
            Some(3_000)
        );
        // Min-track floor: shorter track under the floor → refuse.
        assert_eq!(
            skip_fade_duration_ms(2_000, 240_000, 5_000, 0, 10_000),
            None
        );
        // Unknown durations → refuse (either side).
        assert_eq!(skip_fade_duration_ms(2_000, 0, 180_000, 0, 0), None);
        assert_eq!(skip_fade_duration_ms(2_000, 240_000, 0, 0, 0), None);
        // Remaining-audio clamp: 1s left of the outgoing → 1s fade.
        assert_eq!(
            skip_fade_duration_ms(4_000, 240_000, 180_000, 239_000, 10_000),
            Some(1_000)
        );
        // Nothing left of the outgoing → refuse (hard cut is honest there).
        assert_eq!(
            skip_fade_duration_ms(4_000, 240_000, 180_000, 240_000, 10_000),
            None
        );
    }

    /// M7 fire path: with the gates clear, `crossfade_to_next` starts the
    /// engine phase `Active` DIRECTLY (no Armed hop), marks it as a skip
    /// fade, and bumps the source generation exactly once so a racing
    /// natural-EOF completion for the outgoing is discarded.
    #[tokio::test]
    async fn crossfade_to_next_fires_direct_active_marks_skip_and_bumps_generation() {
        let mut engine = CustomAudioEngine::new();
        prime_playing_engine(&mut engine);
        let gen_before = engine.source_generation();

        let outcome = engine
            .crossfade_to_next(
                skip_ready_decoder(180_000),
                "http://example.test/next".to_string(),
                None,
                gen_before,
            )
            .await;

        assert_eq!(outcome, SkipFadeOutcome::Fired);
        assert!(
            !engine.crossfade.phase.is_idle(),
            "the skip fade must go engine-Active directly"
        );
        assert!(engine.crossfade.skip_fade, "the phase must be skip-marked");
        assert_eq!(
            engine.source_generation(),
            gen_before + 1,
            "a fired skip fade is a user-driven source change (stale completions discarded)"
        );
    }

    /// M7 min-track gate: the direct fire bypasses `arm_crossfade`, so the
    /// configured floor must be re-applied — a skip to a track under the
    /// floor refuses the blend (caller falls back).
    #[tokio::test]
    async fn crossfade_to_next_blocked_by_min_track_floor() {
        let mut engine = CustomAudioEngine::new();
        prime_playing_engine(&mut engine);
        engine.set_crossfade_min_track_secs(10);
        let generation = engine.source_generation();

        let outcome = engine
            .crossfade_to_next(
                skip_ready_decoder(5_000),
                "http://example.test/short".to_string(),
                None,
                generation,
            )
            .await;

        assert_eq!(outcome, SkipFadeOutcome::Blocked);
        assert!(engine.crossfade.phase.is_idle());
        assert!(!engine.crossfade.skip_fade);
    }

    /// M7 format gate (invariant 8): under viable bit-perfect Strict the
    /// skip blend is refused via the SAME `crossfade_blocked` the two auto
    /// triggers share — the caller falls back to the boundary fade / cut.
    #[tokio::test]
    async fn crossfade_to_next_blocked_under_bit_perfect_strict() {
        let mut engine = CustomAudioEngine::new();
        prime_playing_engine(&mut engine);
        {
            let mut renderer = engine.renderer.lock();
            renderer.force_pw_volume_active_for_test();
            renderer.set_bit_perfect(crate::types::player_settings::BitPerfectMode::Strict);
        }
        let generation = engine.source_generation();

        let outcome = engine
            .crossfade_to_next(
                skip_ready_decoder(180_000),
                "http://example.test/next".to_string(),
                None,
                generation,
            )
            .await;

        assert_eq!(outcome, SkipFadeOutcome::Blocked);
        assert!(engine.crossfade.phase.is_idle());
    }

    /// M7 supersession: a stale generation snapshot (a competing user action
    /// landed while the skip's decoder was building, locks released) must
    /// abandon the fade WITHOUT touching engine state.
    #[tokio::test]
    async fn crossfade_to_next_stale_generation_is_noop() {
        let mut engine = CustomAudioEngine::new();
        prime_playing_engine(&mut engine);
        let stale = engine.source_generation();
        engine.channels.source_generation.bump_for_user_action();

        let outcome = engine
            .crossfade_to_next(
                skip_ready_decoder(180_000),
                "http://example.test/next".to_string(),
                None,
                stale,
            )
            .await;

        assert_eq!(outcome, SkipFadeOutcome::Stale);
        assert!(engine.crossfade.phase.is_idle());
        assert!(engine.playing, "a stale skip must not disturb playback");
    }

    /// M7 viability: nothing audibly playing (or an infinite/radio stream)
    /// refuses the blend — the caller's hard path handles those.
    #[tokio::test]
    async fn crossfade_to_next_blocked_when_not_playing_or_infinite() {
        let mut engine = CustomAudioEngine::new();
        let generation = engine.source_generation();
        let outcome = engine
            .crossfade_to_next(
                skip_ready_decoder(180_000),
                "http://example.test/next".to_string(),
                None,
                generation,
            )
            .await;
        assert_eq!(outcome, SkipFadeOutcome::Blocked, "stopped engine");

        prime_playing_engine(&mut engine);
        engine
            .channels
            .stream_is_infinite
            .store(true, Ordering::Release);
        let generation = engine.source_generation();
        let outcome = engine
            .crossfade_to_next(
                skip_ready_decoder(180_000),
                "http://example.test/next".to_string(),
                None,
                generation,
            )
            .await;
        assert_eq!(outcome, SkipFadeOutcome::Blocked, "infinite outgoing");
    }

    /// M7 finalize split: a skip fade already advanced the queue at skip
    /// time, so its finalize must NOT fire the completion callback (the
    /// callback's decide_transition would advance AGAIN — a silently skipped
    /// track) and must not stamp the crossfade label. A normal auto-advance
    /// finalize keeps both.
    #[tokio::test]
    async fn finalize_after_skip_fade_suppresses_completion_callback_and_label() {
        let fired = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut engine = CustomAudioEngine::new();
        let fired_cb = fired.clone();
        engine.set_completion_callback(move |_| {
            fired_cb.fetch_add(1, Ordering::SeqCst);
        });

        // Skip-fade finalize: callback suppressed, label unset, marker cleared.
        engine.crossfade.phase = CrossfadePhase::Active {
            decoder: Arc::new(tokio::sync::Mutex::new(Some(skip_ready_decoder(180_000)))),
            incoming_source: "http://example.test/next".to_string(),
        };
        engine.crossfade.skip_fade = true;
        engine.finalize_crossfade_engine().await;
        assert_eq!(
            fired.load(Ordering::SeqCst),
            0,
            "a skip-fade finalize must not fire the completion callback (double advance)"
        );
        assert!(
            !engine.take_last_transition_was_crossfade(),
            "a skip-fade finalize must not stamp the auto-advance crossfade label"
        );
        assert!(!engine.crossfade.skip_fade, "marker is read-and-cleared");

        // Normal auto-advance finalize: callback + label intact.
        engine.crossfade.phase = CrossfadePhase::Active {
            decoder: Arc::new(tokio::sync::Mutex::new(Some(skip_ready_decoder(180_000)))),
            incoming_source: "http://example.test/next2".to_string(),
        };
        engine.finalize_crossfade_engine().await;
        assert_eq!(fired.load(Ordering::SeqCst), 1);
        assert!(engine.take_last_transition_was_crossfade());
    }

    /// M7 mid-fade gapless prep: a background prep landing while a blend is
    /// LIVE must not reset/cancel it (the skip fade re-opens the UI's prep
    /// latch, so this window is real). The slot is stored WITHOUT the
    /// internal reset and WITHOUT arming (Armed would overwrite the Active
    /// variant); finalize then re-arms from the stored slot.
    #[tokio::test]
    async fn store_prepared_decoder_mid_fade_stores_without_cancelling_the_blend() {
        let mut engine = CustomAudioEngine::new();
        prime_playing_engine(&mut engine);
        engine.crossfade.enabled = true;
        engine.crossfade.phase = CrossfadePhase::Active {
            decoder: Arc::new(tokio::sync::Mutex::new(Some(skip_ready_decoder(180_000)))),
            incoming_source: "http://example.test/inbound".to_string(),
        };
        // The LIVE blend's incoming RG is staged on the renderer — finalize
        // will promote it into `current_replay_gain`.
        engine
            .renderer
            .lock()
            .set_pending_crossfade_replay_gain(Some(rg(-1.0)));

        engine
            .store_prepared_decoder(
                skip_ready_decoder(200_000),
                "http://example.test/after-next".to_string(),
                Some(rg(-7.0)),
                PreparedTransitionDirectives {
                    suppress_crossfade: false,
                    duration_override_ms: Some(6_000),
                    gap_offset_ms: 1_000,
                },
            )
            .await;

        assert!(
            !engine.crossfade.phase.is_idle(),
            "a mid-fade store must not cancel the live blend"
        );
        // M8: the mid-blend branch stages the directives too — the finalize
        // re-arm reads the override, and the stored transition's gap must
        // survive the finalize-time decode-loop restart.
        assert_eq!(
            engine.crossfade.duration_override_ms,
            Some(6_000),
            "the mid-fade store must stage the bar-snap override for the re-arm"
        );
        assert_eq!(
            engine.channels.gap_offset_ms.load(Ordering::Relaxed),
            1_000,
            "the mid-fade store must stage the stored transition's gap"
        );
        assert!(
            engine.is_next_track_prepared().await,
            "the prep must still land in the gapless slot"
        );
        assert!(
            !engine.renderer.lock().is_crossfade_armed(),
            "arming while Active would overwrite the Active variant"
        );
        assert_eq!(engine.next_source, "http://example.test/after-next");
        assert_eq!(
            engine
                .renderer
                .lock()
                .pending_crossfade_replay_gain_for_test(),
            Some(rg(-1.0)),
            "the live blend's staged RG must survive the mid-fade store — \
             overwriting it makes finalize promote the WRONG track's tags"
        );
        assert_eq!(
            engine.gapless.lock().await.replay_gain,
            Some(rg(-7.0)),
            "the mid-fade prep's RG rides the slot until the finalize re-arm"
        );
    }

    /// M7 finalize re-arm: after a finalize that promoted the incoming, a
    /// slot stored mid-fade must arm the NEXT transition (the promoted track
    /// → the stored one), re-deriving the incoming format from the slot
    /// (finalize clears `next_format`) and re-staging the slot's RG (the
    /// promoted track keeps ITS OWN tags — `current_replay_gain` — while the
    /// re-armed blend gets the stored track's).
    #[tokio::test]
    async fn finalize_rearms_mid_fade_stored_prep() {
        let mut engine = CustomAudioEngine::new();
        prime_playing_engine(&mut engine);
        engine.crossfade.enabled = true;
        engine.crossfade.phase = CrossfadePhase::Active {
            decoder: Arc::new(tokio::sync::Mutex::new(Some(skip_ready_decoder(180_000)))),
            incoming_source: "http://example.test/inbound".to_string(),
        };
        // The renderer half must be genuinely Active too, so its
        // `finalize_crossfade` runs the real promotion (pending RG →
        // current) instead of early-returning.
        let _keepalive = engine.renderer.lock().force_crossfade_active_for_test();
        // The LIVE blend's incoming RG, staged when that blend was fired.
        engine
            .renderer
            .lock()
            .set_pending_crossfade_replay_gain(Some(rg(-1.0)));
        engine
            .store_prepared_decoder(
                skip_ready_decoder(200_000),
                "http://example.test/after-next".to_string(),
                Some(rg(-7.0)),
                PreparedTransitionDirectives::from_suppress(false),
            )
            .await;

        engine.finalize_crossfade_engine().await;

        assert!(engine.crossfade.phase.is_idle());
        assert!(
            engine.renderer.lock().is_crossfade_armed(),
            "finalize must re-arm the transition for a slot stored mid-fade"
        );
        assert_eq!(
            engine.renderer.lock().current_replay_gain_for_test(),
            Some(rg(-1.0)),
            "finalize must promote the LIVE blend's RG — a seek in the \
             promoted track rebuilds its stream from current_replay_gain"
        );
        assert_eq!(
            engine
                .renderer
                .lock()
                .pending_crossfade_replay_gain_for_test(),
            Some(rg(-7.0)),
            "the re-armed next blend must carry the stored track's RG, not \
             fire later with none"
        );
    }

    /// M7 invariant-3 route: `reset_next_track` (every mode toggle / queue
    /// mutation) still cancels a LIVE skip fade and clears its marker, so a
    /// stale marker can never suppress a later auto-advance finalize.
    #[tokio::test]
    async fn reset_next_track_cancels_skip_fade_and_clears_marker() {
        let mut engine = CustomAudioEngine::new();
        engine.crossfade.phase = CrossfadePhase::Active {
            decoder: Arc::new(tokio::sync::Mutex::new(Some(fresh_decoder()))),
            incoming_source: "http://example.test/next".to_string(),
        };
        engine.crossfade.skip_fade = true;

        engine.reset_next_track().await;

        assert!(engine.crossfade.phase.is_idle());
        assert!(
            !engine.crossfade.skip_fade,
            "a cancelled skip fade must clear its marker"
        );
    }

    /// A distinct ReplayGain per track so the RG-lifecycle tests can tell
    /// exactly whose tags ended up where.
    fn rg(track_gain: f64) -> crate::types::song::ReplayGain {
        crate::types::song::ReplayGain {
            album_gain: None,
            track_gain: Some(track_gain),
            album_peak: None,
            track_peak: None,
        }
    }

    /// M7 review cycle 1 — plan-time invalidation: `plan_skip_fade` must
    /// cancel the pre-skip transition (live blend + prepared slot), bump the
    /// source generation (discarding every completion dispatch snapshotted
    /// before plan time), and latch the pending window with the post-bump
    /// generation. The latch must self-invalidate when any later action
    /// bumps the generation again.
    #[tokio::test]
    async fn plan_skip_fade_invalidates_and_latches_window() {
        let mut engine = CustomAudioEngine::new();
        prime_playing_engine(&mut engine);
        engine.crossfade.phase = CrossfadePhase::Active {
            decoder: Arc::new(tokio::sync::Mutex::new(Some(skip_ready_decoder(180_000)))),
            incoming_source: "http://example.test/live-incoming".to_string(),
        };
        {
            let mut slot = engine.gapless.lock().await;
            slot.decoder = Some(skip_ready_decoder(200_000));
            slot.source = "http://example.test/pre-skip-next".to_string();
            slot.prepared = true;
        }
        let gen_before = engine.source_generation();

        engine.plan_skip_fade().await;

        assert!(
            engine.crossfade.phase.is_idle(),
            "a live blend must be cancelled at PLAN time — its finalize during \
             the unlocked build window would advance the queue a second time"
        );
        assert!(
            !engine.is_next_track_prepared().await,
            "the pre-skip prepared slot is void once the queue re-sequences"
        );
        assert_eq!(
            engine.source_generation(),
            gen_before + 1,
            "the skip is the user-driven source change — bump at plan time, \
             under the lock, not at fire time"
        );
        assert!(
            engine.skip_fade_window_pending(),
            "the pending window must be latched with the post-bump generation"
        );

        // Self-invalidation: any competing bump closes the window.
        engine.channels.source_generation.bump_for_user_action();
        assert!(
            !engine.skip_fade_window_pending(),
            "a competing generation bump must invalidate the latch (no \
             stranded suppression possible)"
        );
    }

    /// M7 review cycle 1 — deferred completion: a track completion processed
    /// while the skip-fade build window is open must NOT run the completion
    /// machinery (the queue cursor already advanced at skip time; advancing
    /// again silently skips a track / desyncs now-playing). After the window
    /// closes (generation moves), completions flow normally again.
    #[tokio::test]
    async fn completion_deferred_while_skip_fade_window_pending() {
        let fired = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut engine = CustomAudioEngine::new();
        let fired_cb = fired.clone();
        engine.set_completion_callback(move |_| {
            fired_cb.fetch_add(1, Ordering::SeqCst);
        });
        engine.plan_skip_fade().await;
        // Prime a state that WOULD complete: the primary decoder reports EOF
        // (the completion path then routes through `on_decoder_finished`,
        // whose no-prep branch fires the completion callback).
        engine.decoder.lock().await.set_eof_for_test(true);

        let finished = engine.on_renderer_finished().await;

        assert!(!finished, "the completion must be deferred, not consumed");
        assert_eq!(
            fired.load(Ordering::SeqCst),
            0,
            "no completion callback while the skip plan owns the transition"
        );

        // Window closes (fire / fallback / competing action bumps) →
        // completions run normally again.
        engine.channels.source_generation.bump_for_user_action();
        engine.on_renderer_finished().await;
        assert_eq!(
            fired.load(Ordering::SeqCst),
            1,
            "a stale latch must never suppress completions after the window"
        );
    }

    /// M7 review cycle 1 — prep landing during the build window: stored
    /// WITHOUT arming (the armed position trigger could fire an auto blend
    /// against the already-advanced cursor before the skip's own fire) and
    /// WITHOUT staging its RG on the renderer (the fire stages the skip
    /// target's RG; the slot carries this one until the finalize re-arm).
    #[tokio::test]
    async fn store_prepared_decoder_during_skip_window_stores_without_arming() {
        let mut engine = CustomAudioEngine::new();
        prime_playing_engine(&mut engine);
        engine.crossfade.enabled = true;
        engine.plan_skip_fade().await;

        engine
            .store_prepared_decoder(
                skip_ready_decoder(200_000),
                "http://example.test/after-skip-target".to_string(),
                Some(rg(-3.0)),
                PreparedTransitionDirectives::from_suppress(false),
            )
            .await;

        assert!(
            engine.is_next_track_prepared().await,
            "the prep must still land in the gapless slot"
        );
        assert!(
            !engine.renderer.lock().is_crossfade_armed(),
            "arming during the window would let the auto trigger fire against \
             the already-advanced cursor"
        );
        assert!(
            engine.skip_fade_window_pending(),
            "the store must not disturb the pending window"
        );
        assert_eq!(
            engine.gapless.lock().await.replay_gain,
            Some(rg(-3.0)),
            "the slot must carry the prep's RG"
        );
        assert_eq!(
            engine
                .renderer
                .lock()
                .pending_crossfade_replay_gain_for_test(),
            None,
            "the window store must not stage RG on the renderer (the skip \
             fire stages the skip target's own RG there)"
        );
    }

    /// M7 review cycle 1 — the inline EOF gapless swap must stand down while
    /// a skip-fade build window is open: the cursor already advanced for the
    /// skip, so a swap would audibly play the wrong track and its completion
    /// callback would advance the cursor a second time.
    #[tokio::test]
    async fn gapless_swap_stands_down_while_skip_window_pending() {
        let mut engine = CustomAudioEngine::new();
        let cb_count = install_callback_counter(&mut engine);
        {
            let mut slot = engine.gapless.lock().await;
            slot.decoder = Some(staged_decoder(matching_format(), 222_000, "flac"));
            slot.source = "http://example.test/window-prep".to_string();
            slot.prepared = true;
        }
        // Open the window: latch == current generation.
        engine
            .channels
            .skip_fade_pending
            .store(engine.source_generation(), Ordering::Release);
        let gen_before = engine.source_generation();

        let outcome = try_gapless_swap(
            &engine.decoder,
            &engine.renderer,
            &engine.gapless,
            &engine.gapless_transition_info,
            &engine.channels.source_generation,
            &engine.completion_callback,
            &matching_format(),
            &engine.channels.skip_fade_pending,
        )
        .await;

        assert_eq!(outcome, GaplessSwapOutcome::SkipFadePlanPending);
        assert!(
            engine.gapless.lock().await.is_prepared(),
            "the staged decoder must be put back for the fire/finalize to use"
        );
        assert_eq!(cb_count.load(Ordering::SeqCst), 0, "no completion callback");
        assert_eq!(
            engine.source_generation(),
            gen_before,
            "a stood-down swap must not bump the generation"
        );
    }

    /// M7 review cycle 1 — the fire must PRESERVE a slot stored during the
    /// build window: it targets the track AFTER the skip target (the UI's
    /// prep re-dispatched at skip time), so finalize can re-arm from it.
    /// Only the pre-skip slot is void — and that was cleared at plan time.
    #[tokio::test]
    async fn crossfade_to_next_preserves_window_stored_prep() {
        let mut engine = CustomAudioEngine::new();
        prime_playing_engine(&mut engine);
        {
            let mut slot = engine.gapless.lock().await;
            slot.decoder = Some(skip_ready_decoder(200_000));
            slot.source = "http://example.test/after-skip-target".to_string();
            slot.prepared = true;
            slot.replay_gain = Some(rg(-4.5));
        }
        let generation = engine.source_generation();

        let outcome = engine
            .crossfade_to_next(
                skip_ready_decoder(180_000),
                "http://example.test/skip-target".to_string(),
                None,
                generation,
            )
            .await;

        assert_eq!(outcome, SkipFadeOutcome::Fired);
        assert!(
            engine.is_next_track_prepared().await,
            "a window-stored prep targets the post-skip next track — the fire \
             must not wipe it"
        );
    }

    /// M7 review cycle 1 — outgoing drained during the build window (its
    /// completion was deferred, so `playing` is still true): there is
    /// nothing left to blend, so the fire must refuse and let the caller's
    /// hard fallback load the target instantly instead of fading in over
    /// 1–4s of silence.
    #[tokio::test]
    async fn crossfade_to_next_blocked_when_outgoing_drained() {
        let mut engine = CustomAudioEngine::new();
        prime_playing_engine(&mut engine);
        engine.channels.decoder_eof.store(true, Ordering::Release);
        assert!(
            engine.renderer.lock().is_buffer_queue_empty(),
            "precondition: the outgoing ring is drained"
        );
        let generation = engine.source_generation();

        let outcome = engine
            .crossfade_to_next(
                skip_ready_decoder(180_000),
                "http://example.test/next".to_string(),
                None,
                generation,
            )
            .await;

        assert_eq!(outcome, SkipFadeOutcome::Blocked);
        assert!(engine.crossfade.phase.is_idle());
    }

    /// M7 review cycle 1 — engine `start_crossfade` (the EOF-fallback
    /// trigger) must stage the SLOT's ReplayGain before building the
    /// incoming stream: a cancelled blend nulls the renderer's staged copy
    /// while the slot retains the prep, and firing with `None` would fade
    /// the incoming up at the untagged-fallback gain.
    #[tokio::test]
    async fn start_crossfade_stages_slot_replay_gain() {
        let mut engine = CustomAudioEngine::new();
        prime_playing_engine(&mut engine);
        engine.crossfade.enabled = true;
        {
            let mut slot = engine.gapless.lock().await;
            slot.decoder = Some(skip_ready_decoder(200_000));
            slot.source = "http://example.test/next".to_string();
            slot.prepared = true;
            slot.replay_gain = Some(rg(-6.0));
        }
        engine.next_source = "http://example.test/next".to_string();
        assert_eq!(
            engine
                .renderer
                .lock()
                .pending_crossfade_replay_gain_for_test(),
            None,
            "precondition: the renderer's staged copy was dropped (cancel path)"
        );

        engine.start_crossfade().await;

        assert_eq!(
            engine
                .renderer
                .lock()
                .pending_crossfade_replay_gain_for_test(),
            Some(rg(-6.0)),
            "the EOF-fallback fire must re-stage the slot's RG"
        );
    }

    /// M7 review cycle 1 — stall recovery during a SKIP fade must hard-load
    /// the skip target (the queue's already-current row) instead of routing
    /// through the end-of-track machinery, whose completion callback would
    /// advance the already-advanced cursor: one Next press would land two
    /// tracks ahead and the target would never play.
    #[tokio::test(flavor = "multi_thread")]
    async fn recover_stalled_skip_fade_hard_loads_target_without_advancing() {
        let fired = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut engine = CustomAudioEngine::new();
        let fired_cb = fired.clone();
        engine.set_completion_callback(move |_| {
            fired_cb.fetch_add(1, Ordering::SeqCst);
        });
        prime_playing_engine(&mut engine);
        // The target port never listens (connection refused, fast + offline
        // safe): the reload attempt fails, which is the honest outcome on a
        // dead network — what matters is WHERE the engine points afterwards.
        let target = "http://127.0.0.1:9/skip-target".to_string();
        engine.crossfade.phase = CrossfadePhase::Active {
            decoder: Arc::new(tokio::sync::Mutex::new(Some(skip_ready_decoder(180_000)))),
            incoming_source: target.clone(),
        };
        engine.crossfade.skip_fade = true;
        engine
            .renderer
            .lock()
            .set_pending_crossfade_replay_gain(Some(rg(-2.0)));

        engine.recover_stalled_crossfade().await;

        assert_eq!(
            fired.load(Ordering::SeqCst),
            0,
            "no completion callback — the queue already advanced at skip time"
        );
        assert!(engine.crossfade.phase.is_idle());
        assert!(!engine.crossfade.skip_fade, "marker cleared with the phase");
        assert!(
            engine.is_playing_source(&target),
            "recovery must retry the SKIP TARGET (the queue's current row), \
             not advance past it; engine source is {}",
            redact_subsonic_url(engine.source())
        );
    }

    /// M7 boundary fade: from an audible playing state the skip out-ramp
    /// completes via live render ticks (same contract as the M5 stop fade);
    /// from paused it returns immediately — nothing audible to fade.
    #[tokio::test(flavor = "multi_thread")]
    async fn run_skip_out_fade_completes_via_live_render_thread() {
        let mut engine = CustomAudioEngine::new();
        engine.set_skip_fade(crate::types::player_settings::FadeOnSkip::BoundaryFade, 1);
        let (_src, handle) = engine.renderer.lock().force_primary_stream_for_test(48_000);
        handle.set_volume(1.0);
        engine.playing = true;
        engine.start_render_thread();

        engine.run_skip_out_fade().await;

        assert_eq!(
            engine.renderer.lock().transport_fade_completions(),
            1,
            "the skip out-ramp must complete via live render ticks"
        );
        engine.stop_render_thread();
    }

    /// M7 boundary fade from paused: `render_tick`'s early-return froze the
    /// stream at pause time, so the out-ramp must return immediately instead
    /// of burning its bounded wait.
    #[tokio::test]
    async fn run_skip_out_fade_from_paused_returns_immediately() {
        let mut engine = CustomAudioEngine::new();
        engine.set_skip_fade(crate::types::player_settings::FadeOnSkip::BoundaryFade, 4);
        let (_src, _handle) = engine.renderer.lock().force_primary_stream_for_test(4_096);
        engine.playing = false;
        engine.paused = true;

        let t0 = std::time::Instant::now();
        engine.run_skip_out_fade().await;

        assert!(
            t0.elapsed() < std::time::Duration::from_millis(300),
            "paused skip fade must not wait out a 4s ramp (took {:?})",
            t0.elapsed()
        );
        assert_eq!(engine.renderer.lock().transport_fade_completions(), 0);
    }

    /// The stall recovery must recover the DESYNCED case: renderer Active
    /// (mid-fade, incoming ring empty) while the engine phase is Idle because
    /// its half never started (e.g. the settings-toggle desync routed the
    /// trigger into the buffer-starvation wait). The old phase-idle
    /// early-return no-op'd here, leaving the renderer Active so render_tick
    /// re-reported the stall every 20ms tick — an unrecoverable warn livelock.
    #[tokio::test]
    async fn recover_stalled_crossfade_tears_down_renderer_only_stall() {
        let mut engine = CustomAudioEngine::new();
        let _keepalive = engine.renderer.lock().force_crossfade_active_for_test();
        assert!(
            engine.crossfade.phase.is_idle(),
            "precondition: engine half never started",
        );
        assert!(
            engine.renderer.lock().is_crossfade_active(),
            "precondition: renderer is mid-fade",
        );

        engine.recover_stalled_crossfade().await;

        assert!(
            !engine.renderer.lock().is_crossfade_active(),
            "recovery must tear down the renderer's orphaned blend, not no-op",
        );
        assert!(engine.crossfade.phase.is_idle());
    }

    /// M9 Part B wiring: `start_crossfade_decode_loop` must install its
    /// per-fade `IncomingLiveness` handle on the renderer so `tick_crossfade`
    /// can read the socket-blocked-vs-backpressure discriminator at fade
    /// completion. A decoder-less Arc makes the spawned loop exit
    /// immediately; the install is synchronous and must land regardless.
    #[tokio::test]
    async fn start_crossfade_decode_loop_installs_liveness_on_renderer() {
        let mut engine = CustomAudioEngine::new();
        assert!(!engine.renderer.lock().has_incoming_liveness_for_test());

        engine.start_crossfade_decode_loop(Arc::new(tokio::sync::Mutex::new(None)));

        assert!(
            engine.renderer.lock().has_incoming_liveness_for_test(),
            "the loop's per-fade liveness handle must be installed on the renderer"
        );
    }

    /// Toggling bit-perfect mid-transition must abandon the in-flight crossfade
    /// (it flips crossfade eligibility — a blend armed/active under the old mode
    /// would otherwise desync against the engine's now-refusing gate).
    #[tokio::test]
    async fn set_bit_perfect_change_cancels_active_crossfade() {
        let mut engine = CustomAudioEngine::new();
        engine.crossfade.phase = CrossfadePhase::Active {
            decoder: Arc::new(tokio::sync::Mutex::new(Some(fresh_decoder()))),
            incoming_source: "http://example.test/next".to_string(),
        };
        assert!(
            !engine.crossfade.phase.is_idle(),
            "precondition: engine must start mid-crossfade",
        );

        // Default bit-perfect mode is Off; switching to Strict is a real change.
        engine
            .set_bit_perfect(crate::types::player_settings::BitPerfectMode::Strict)
            .await;

        assert!(
            engine.crossfade.phase.is_idle(),
            "toggling bit-perfect must cancel the in-flight crossfade",
        );
    }

    /// A no-op `set_bit_perfect` (same value) must NOT disturb an in-flight
    /// transition — `apply_player_settings` re-applies every field on each save.
    #[tokio::test]
    async fn set_bit_perfect_no_change_preserves_active_crossfade() {
        let mut engine = CustomAudioEngine::new();
        engine.crossfade.phase = CrossfadePhase::Active {
            decoder: Arc::new(tokio::sync::Mutex::new(Some(fresh_decoder()))),
            incoming_source: "http://example.test/next".to_string(),
        };

        // bit-perfect mode already defaults to Off; setting Off again is a no-op.
        engine
            .set_bit_perfect(crate::types::player_settings::BitPerfectMode::Off)
            .await;

        assert!(
            !engine.crossfade.phase.is_idle(),
            "an unchanged bit-perfect value must not cancel a crossfade",
        );
    }

    /// `rearm_crossfade_if_prepared` (run at the end of `seek` to close the
    /// double-seek disarm race) must NOT arm when crossfade is ineligible — even
    /// with a next track prepared. Off mode + Crossfade toggle off → no arm.
    #[tokio::test]
    async fn rearm_crossfade_noop_when_ineligible() {
        use crate::types::player_settings::BitPerfectMode;
        let mut engine = CustomAudioEngine::new();
        engine.crossfade.enabled = false;
        engine.crossfade.bit_perfect_mode = BitPerfectMode::Off;
        engine.crossfade.duration_ms = 5_000;
        engine.duration = 200_000;
        engine.next_format = AudioFormat::new(crate::audio::format::SampleFormat::S16, 44_100, 2);
        {
            let mut slot = engine.gapless.lock().await;
            slot.decoder = Some(fresh_decoder());
            slot.source = "http://example.test/next".to_string();
            slot.prepared = true;
        }

        engine.rearm_crossfade_if_prepared().await;

        assert!(
            !engine.renderer.lock().is_crossfade_armed(),
            "an ineligible mode must not arm a crossfade even with a prepared next track"
        );
    }

    /// `rearm_crossfade_if_prepared` must be a no-op when nothing is prepared,
    /// even under an eligible mode (Relaxed) — there is no incoming track to
    /// crossfade into yet.
    #[tokio::test]
    async fn rearm_crossfade_noop_when_nothing_prepared() {
        use crate::types::player_settings::BitPerfectMode;
        let mut engine = CustomAudioEngine::new();
        engine.crossfade.bit_perfect_mode = BitPerfectMode::Relaxed;
        engine.crossfade.duration_ms = 5_000;
        engine.duration = 200_000;

        engine.rearm_crossfade_if_prepared().await;

        assert!(
            !engine.renderer.lock().is_crossfade_armed(),
            "no prepared next track means nothing to re-arm"
        );
    }

    /// Positive arm path through `store_prepared_decoder` (Site B): an eligible
    /// mode with a long-enough next track must actually ARM the renderer, and the
    /// OUTGOING track duration (`self.duration`) must land in the Armed state's
    /// `track_duration_ms` — NOT the incoming track's duration. The two
    /// `arm_crossfade` duration args are both `u64`, so a transposition compiles
    /// and clears the symmetric min-duration gate (both ≥ 10s here); only this
    /// asymmetric check (200_000 ≠ 15_000) catches it. The renderer defaults to
    /// bit-perfect Off, so `crossfade_blocked` is bypassed and the arm takes.
    #[tokio::test]
    async fn store_prepared_decoder_arms_crossfade_with_outgoing_track_duration() {
        let mut engine = CustomAudioEngine::new();
        engine.crossfade.enabled = true;
        engine.crossfade.duration_ms = 5_000;
        // Outgoing (current) track duration — the 3rd arm_crossfade arg.
        engine.duration = 200_000;

        // Incoming next track: a DIFFERENT non-zero duration, both it and the
        // outgoing track clear the renderer's 10s minimum.
        let mut decoder = fresh_decoder();
        decoder.set_duration_for_test(15_000);

        engine
            .store_prepared_decoder(
                decoder,
                "http://example.test/next".to_string(),
                None,
                PreparedTransitionDirectives::from_suppress(false),
            )
            .await;

        let renderer = engine.renderer.lock();
        assert!(
            renderer.is_crossfade_armed(),
            "an eligible mode with a long-enough next track must arm the crossfade"
        );
        assert_eq!(
            renderer.armed_track_duration_ms(),
            Some(200_000),
            "the OUTGOING track duration (self.duration) must land in track_duration_ms, \
             not the incoming 15_000 — guards against transposing the two u64 arm args"
        );
    }

    /// Table test for the shared `crossfade_eligible` predicate: eligible when
    /// the Crossfade toggle is on OR bit-perfect Relaxed; ineligible otherwise.
    /// In particular Strict (with the toggle off) must be `false` — Strict is not
    /// Relaxed and hard-cuts everything.
    #[tokio::test]
    async fn crossfade_eligible_table() {
        use crate::types::player_settings::BitPerfectMode;
        let cases = [
            // (crossfade_enabled, bit_perfect_mode, expected)
            (false, BitPerfectMode::Off, false),
            (false, BitPerfectMode::Strict, false),
            (false, BitPerfectMode::Relaxed, true),
            (true, BitPerfectMode::Off, true),
            (true, BitPerfectMode::Strict, true),
            (true, BitPerfectMode::Relaxed, true),
        ];
        for (enabled, mode, expected) in cases {
            let mut engine = CustomAudioEngine::new();
            engine.crossfade.enabled = enabled;
            engine.crossfade.bit_perfect_mode = mode;
            assert_eq!(
                engine.crossfade.crossfade_eligible(),
                expected,
                "crossfade_eligible(enabled={enabled}, mode={mode:?}) should be {expected}"
            );
        }
    }

    #[test]
    fn gapless_slot_new_is_not_prepared() {
        let slot = GaplessSlot::new();
        assert!(!slot.is_prepared());
        assert!(slot.decoder.is_none());
        assert!(slot.source.is_empty());
    }

    #[test]
    fn gapless_slot_prepared_flag_alone_does_not_count_as_prepared() {
        let mut slot = GaplessSlot::new();
        slot.prepared = true;
        assert!(!slot.is_prepared());
    }

    #[test]
    fn gapless_slot_decoder_alone_does_not_count_as_prepared() {
        let mut slot = GaplessSlot::new();
        slot.decoder = Some(fresh_decoder());
        assert!(!slot.is_prepared());
    }

    #[test]
    fn gapless_slot_prepared_requires_both_flag_and_decoder() {
        let mut slot = GaplessSlot::new();
        slot.decoder = Some(fresh_decoder());
        slot.prepared = true;
        slot.source = "http://example.test/track".to_string();
        assert!(slot.is_prepared());
    }

    #[test]
    fn gapless_slot_clear_resets_all_fields() {
        let mut slot = GaplessSlot::new();
        slot.decoder = Some(fresh_decoder());
        slot.prepared = true;
        slot.source = "http://example.test/track".to_string();
        slot.clear();
        assert!(!slot.is_prepared());
        assert!(slot.decoder.is_none());
        assert!(slot.source.is_empty());
        assert!(!slot.prepared);
    }

    /// A valid stereo format used as both the "live stream" format and the
    /// staged next-track format in the success path so the gapless equality gate
    /// (`is_valid` + sample_rate + channel_count) clears.
    fn matching_format() -> AudioFormat {
        AudioFormat::new(crate::audio::SampleFormat::F32, 44_100, 2)
    }

    /// Build a decoder whose `format()` is the given (valid) format, with an
    /// identifiable duration + live codec so the success path's
    /// `GaplessTransitionInfo` can be asserted field-by-field.
    fn staged_decoder(format: AudioFormat, duration_ms: u64, codec: &str) -> AudioDecoder {
        let mut dec = fresh_decoder();
        dec.set_format_for_test(format);
        dec.set_duration_for_test(duration_ms);
        dec.set_live_codec_for_test(Some(codec.to_string()));
        dec
    }

    /// Install a completion-callback counter on the engine. The returned Arc
    /// counts how many times the callback fired (the gapless success path fires
    /// it exactly once with `is_loop=false`).
    fn install_callback_counter(engine: &mut CustomAudioEngine) -> Arc<AtomicU32> {
        let counter = Arc::new(AtomicU32::new(0));
        let c = counter.clone();
        engine.completion_callback = Some(Arc::new(move |is_loop: bool| {
            // Gapless always advances to a NEW track — never a loop.
            assert!(
                !is_loop,
                "gapless completion callback must fire with is_loop=false"
            );
            c.fetch_add(1, Ordering::SeqCst);
        }));
        counter
    }

    /// S9 characterization — NotPrepared: an empty slot (nothing staged) yields
    /// `NotPrepared`, fires no callback, and leaves the source generation
    /// untouched (no `bump_for_gapless`).
    #[tokio::test]
    async fn try_gapless_swap_not_prepared() {
        let mut engine = CustomAudioEngine::new();
        let cb_count = install_callback_counter(&mut engine);
        let gen_before = engine.source_generation();

        let outcome = try_gapless_swap(
            &engine.decoder,
            &engine.renderer,
            &engine.gapless,
            &engine.gapless_transition_info,
            &engine.channels.source_generation,
            &engine.completion_callback,
            &matching_format(),
            &engine.channels.skip_fade_pending,
        )
        .await;

        assert_eq!(outcome, GaplessSwapOutcome::NotPrepared);
        assert_eq!(
            cb_count.load(Ordering::SeqCst),
            0,
            "no callback on NotPrepared"
        );
        assert_eq!(
            engine.source_generation(),
            gen_before,
            "NotPrepared must not bump the source generation"
        );
        assert!(
            engine.gapless_transition_info.lock().await.is_none(),
            "NotPrepared must not populate transition info"
        );
    }

    /// S9 characterization — Swapped (the success path): a matching-format staged
    /// decoder is installed, the source generation advances by exactly one
    /// (`bump_for_gapless`), the slot is cleared, the transition info is populated
    /// from the staged decoder, and the completion callback fires once.
    #[tokio::test]
    async fn try_gapless_swap_success() {
        let mut engine = CustomAudioEngine::new();
        let cb_count = install_callback_counter(&mut engine);

        // A fresh renderer reports `gapless_swap_allowed() == true` and no
        // crossfade armed/active, so the matching-format slot swaps cleanly.
        let current_format = matching_format();

        // Stage the next track with an identifiable URL / duration / codec.
        {
            let mut slot = engine.gapless.lock().await;
            slot.decoder = Some(staged_decoder(matching_format(), 222_000, "flac"));
            slot.source = "http://example.test/next-gapless".to_string();
            slot.prepared = true;
        }

        let gen_before = engine.source_generation();

        let outcome = try_gapless_swap(
            &engine.decoder,
            &engine.renderer,
            &engine.gapless,
            &engine.gapless_transition_info,
            &engine.channels.source_generation,
            &engine.completion_callback,
            &current_format,
            &engine.channels.skip_fade_pending,
        )
        .await;

        assert_eq!(outcome, GaplessSwapOutcome::Swapped);
        assert_eq!(
            engine.source_generation(),
            gen_before + 1,
            "Swapped must bump the source generation by exactly one (bump_for_gapless)"
        );
        assert_eq!(
            cb_count.load(Ordering::SeqCst),
            1,
            "Swapped must fire the completion callback exactly once"
        );

        // Slot fully cleared: decoder taken, prepared flipped off, source drained.
        {
            let slot = engine.gapless.lock().await;
            assert!(
                slot.decoder.is_none(),
                "Swapped must take the staged decoder"
            );
            assert!(!slot.prepared, "Swapped must clear the prepared flag");
            assert!(
                slot.source.is_empty(),
                "Swapped must drain the staged source"
            );
        }

        // Transition info populated from the staged decoder.
        let info = engine
            .gapless_transition_info
            .lock()
            .await
            .clone()
            .expect("Swapped must populate transition info");
        assert_eq!(info.source, "http://example.test/next-gapless");
        assert_eq!(info.duration, 222_000);
        assert_eq!(info.format, matching_format());
        assert_eq!(info.codec.as_deref(), Some("flac"));

        // The staged decoder is now the primary decoder (format carried over).
        assert_eq!(
            engine.decoder.lock().await.format(),
            &matching_format(),
            "Swapped must install the staged decoder as the primary decoder"
        );
    }

    /// S9 characterization — FormatMismatch: a staged decoder whose format does
    /// NOT match the live stream is put BACK in the slot (`prepared` stays true)
    /// for a later retry, no generation bump, no callback.
    #[tokio::test]
    async fn try_gapless_swap_format_mismatch_puts_decoder_back() {
        let mut engine = CustomAudioEngine::new();
        let cb_count = install_callback_counter(&mut engine);

        // Live stream is 44.1k stereo; staged track is 96k stereo — mismatch.
        let current_format = matching_format();
        {
            let mut slot = engine.gapless.lock().await;
            slot.decoder = Some(staged_decoder(
                AudioFormat::new(crate::audio::SampleFormat::F32, 96_000, 2),
                180_000,
                "flac",
            ));
            slot.source = "http://example.test/next-mismatch".to_string();
            slot.prepared = true;
        }
        let gen_before = engine.source_generation();

        let outcome = try_gapless_swap(
            &engine.decoder,
            &engine.renderer,
            &engine.gapless,
            &engine.gapless_transition_info,
            &engine.channels.source_generation,
            &engine.completion_callback,
            &current_format,
            &engine.channels.skip_fade_pending,
        )
        .await;

        assert_eq!(outcome, GaplessSwapOutcome::FormatMismatch);
        assert_eq!(
            engine.source_generation(),
            gen_before,
            "FormatMismatch must not bump the source generation"
        );
        assert_eq!(
            cb_count.load(Ordering::SeqCst),
            0,
            "no callback on FormatMismatch"
        );
        // Decoder put BACK; slot still prepared with its source intact.
        let slot = engine.gapless.lock().await;
        assert!(
            slot.decoder.is_some(),
            "FormatMismatch must put the staged decoder BACK for a later retry"
        );
        assert!(slot.prepared, "FormatMismatch must leave prepared set");
        assert_eq!(slot.source, "http://example.test/next-mismatch");
    }

    /// S9 characterization — CrossfadeActive: when the renderer reports a
    /// crossfade armed (or active), the inline swap stands down and puts the
    /// staged decoder BACK so the renderer's position-based trigger can take it.
    /// No generation bump, no callback.
    #[tokio::test]
    async fn try_gapless_swap_crossfade_active_puts_decoder_back() {
        let mut engine = CustomAudioEngine::new();
        let cb_count = install_callback_counter(&mut engine);

        // Arm the renderer's crossfade via the engine's positive-arm path so
        // `is_crossfade_armed()` returns true. (Mirrors
        // `store_prepared_decoder_arms_crossfade_with_outgoing_track_duration`.)
        engine.crossfade.enabled = true;
        engine.crossfade.duration_ms = 5_000;
        engine.duration = 200_000;
        let mut arming_dec = fresh_decoder();
        arming_dec.set_duration_for_test(180_000);
        engine
            .store_prepared_decoder(
                arming_dec,
                "http://example.test/armer".to_string(),
                None,
                PreparedTransitionDirectives::from_suppress(false),
            )
            .await;
        assert!(
            engine.renderer.lock().is_crossfade_armed(),
            "precondition: renderer crossfade must be armed",
        );

        // Now stage a FORMAT-MATCHING decoder so the ONLY thing standing the swap
        // down is the armed crossfade (not a format mismatch).
        let current_format = matching_format();
        {
            let mut slot = engine.gapless.lock().await;
            slot.decoder = Some(staged_decoder(matching_format(), 200_000, "flac"));
            slot.source = "http://example.test/next-during-crossfade".to_string();
            slot.prepared = true;
        }
        let gen_before = engine.source_generation();

        let outcome = try_gapless_swap(
            &engine.decoder,
            &engine.renderer,
            &engine.gapless,
            &engine.gapless_transition_info,
            &engine.channels.source_generation,
            &engine.completion_callback,
            &current_format,
            &engine.channels.skip_fade_pending,
        )
        .await;

        assert_eq!(outcome, GaplessSwapOutcome::CrossfadeActive);
        assert_eq!(
            engine.source_generation(),
            gen_before,
            "CrossfadeActive must not bump the source generation"
        );
        assert_eq!(
            cb_count.load(Ordering::SeqCst),
            0,
            "no callback on CrossfadeActive"
        );
        let slot = engine.gapless.lock().await;
        assert!(
            slot.decoder.is_some(),
            "CrossfadeActive must put the staged decoder BACK for the renderer trigger"
        );
        assert!(slot.prepared, "CrossfadeActive must leave prepared set");
    }

    /// S9 characterization — prepared-but-decoder-missing (torn slot): the slot
    /// claims `prepared` but holds no decoder. `is_prepared()` requires BOTH the
    /// flag AND the decoder, so it returns false and the function short-circuits
    /// to `NotPrepared` — the `DecoderMissing` arm (the `take()`-returns-`None`
    /// branch) is defensive and unreachable while `is_prepared()` gates entry.
    /// This pins `is_prepared()` as the authoritative gate so a stale `prepared`
    /// flag alone never triggers a phantom swap (no generation bump, no callback),
    /// and that the original inline block's torn-slot semantics survive the
    /// extraction.
    #[tokio::test]
    async fn try_gapless_swap_prepared_but_decoder_missing() {
        let mut engine = CustomAudioEngine::new();
        let cb_count = install_callback_counter(&mut engine);

        // Torn slot: prepared set, but no decoder. `is_prepared()` is false, so
        // the function returns NotPrepared (the prepared+no-decoder combination
        // can't reach the take()-else arm because is_prepared() gates it). This
        // pins that `is_prepared()` is the authoritative gate, so a stale
        // `prepared` flag alone never triggers a phantom swap.
        {
            let mut slot = engine.gapless.lock().await;
            slot.decoder = None;
            slot.source = "http://example.test/torn".to_string();
            slot.prepared = true;
            assert!(
                !slot.is_prepared(),
                "prepared flag alone is not is_prepared()"
            );
        }
        let gen_before = engine.source_generation();

        let outcome = try_gapless_swap(
            &engine.decoder,
            &engine.renderer,
            &engine.gapless,
            &engine.gapless_transition_info,
            &engine.channels.source_generation,
            &engine.completion_callback,
            &matching_format(),
            &engine.channels.skip_fade_pending,
        )
        .await;

        // is_prepared() short-circuits to NotPrepared; the DecoderMissing arm is
        // reachable only if a future edit loosens that gate. Either way, no swap.
        assert_eq!(outcome, GaplessSwapOutcome::NotPrepared);
        assert_eq!(
            engine.source_generation(),
            gen_before,
            "a torn prepared-but-no-decoder slot must not bump the generation"
        );
        assert_eq!(
            cb_count.load(Ordering::SeqCst),
            0,
            "no callback on the torn-slot path"
        );
    }

    /// Characterization (S2): `consume_gapless_transition` performs EIGHT
    /// writes when the in-memory gapless slot holds an `Some(info)`. This pins
    /// every one so a decomposition that drops a write — especially the two
    /// most-droppable, `gapless.lock().source.clear()` and the
    /// `live_sample_rate` store — fails loudly instead of silently regressing
    /// gapless metadata pickup.
    ///
    /// Each field is pre-seeded to a DISTINCT non-default sentinel that differs
    /// from the value the consume writes, so every assertion can genuinely fail.
    /// `playing` stays false (fresh-engine default) so the cleared `position`
    /// is read straight from the private field rather than the renderer-gated
    /// `position()` branch.
    #[tokio::test]
    async fn consume_gapless_transition_applies_all_eight_writes() {
        let mut engine = CustomAudioEngine::new();

        // The info the decode loop staged, with identifiable values.
        let staged_format = AudioFormat::new(crate::audio::SampleFormat::F32, 96_000, 2);
        *engine.gapless_transition_info.lock().await = Some(GaplessTransitionInfo {
            source: "http://example.test/incoming-gapless".to_string(),
            duration: 222_222,
            format: staged_format.clone(),
            codec: Some("flac-incoming".to_string()),
        });

        // Pre-seed every destination to a DIFFERENT sentinel so each of the
        // eight writes is observable (not masked by an already-equal value).
        engine.source = "http://example.test/STALE-current".to_string();
        engine.duration = 111_111;
        engine.position = 77_777; // must be cleared to 0
        engine.current_format = AudioFormat::new(crate::audio::SampleFormat::S16, 44_100, 2);
        engine.live_codec_name.set(Some("mp3-stale".to_string()));
        engine.next_source = "http://example.test/STALE-next".to_string();
        engine.gapless.lock().await.source = "http://example.test/STALE-slot".to_string();
        engine.live_sample_rate.store(44_100, Ordering::Relaxed);
        // Keep playing=false so `position` is read from the private field.
        engine.playing = false;

        engine.consume_gapless_transition().await;

        // 1. source <- info.source
        assert_eq!(
            engine.source, "http://example.test/incoming-gapless",
            "source must be replaced with the staged gapless source",
        );
        // 2. duration <- info.duration
        assert_eq!(
            engine.duration, 222_222,
            "duration must be replaced with the staged duration",
        );
        // 3. position <- 0 (read the private field; engine is not playing)
        assert_eq!(
            engine.position, 0,
            "position must reset to 0 on gapless pickup",
        );
        // 4. current_format <- info.format
        assert_eq!(
            engine.current_format, staged_format,
            "current_format must be replaced with the staged format",
        );
        // 5. live_codec_name <- info.codec
        assert_eq!(
            engine.live_codec(),
            Some("flac-incoming".to_string()),
            "live codec must be replaced with the staged codec",
        );
        // 6. next_source cleared
        assert!(
            engine.next_source.is_empty(),
            "next_source must be cleared on gapless pickup",
        );
        // 7. gapless slot source cleared (most-droppable write A)
        assert!(
            engine.gapless.lock().await.source.is_empty(),
            "gapless slot source must be cleared on gapless pickup",
        );
        // 8. live_sample_rate <- current_format.sample_rate() (most-droppable B)
        assert_eq!(
            engine.live_sample_rate.load(Ordering::Relaxed),
            96_000,
            "live_sample_rate must be stored from the new current_format",
        );
    }

    /// Characterization (S2): the `None` path. With an EMPTY
    /// `gapless_transition_info` slot, `consume_gapless_transition` is a no-op —
    /// every engine field it would otherwise overwrite is left untouched. This
    /// pins that the `if let Some(info)` guard genuinely gates all eight writes.
    #[tokio::test]
    async fn consume_gapless_transition_none_path_is_noop() {
        let mut engine = CustomAudioEngine::new();

        // Slot is None by default on a fresh engine; assert that and leave it.
        assert!(
            engine.gapless_transition_info.lock().await.is_none(),
            "precondition: no staged transition",
        );

        // Distinctive pre-state that the no-op must preserve verbatim.
        engine.source = "http://example.test/keep-current".to_string();
        engine.duration = 333_333;
        engine.position = 44_444;
        engine.current_format = AudioFormat::new(crate::audio::SampleFormat::S24, 88_200, 2);
        engine.live_codec_name.set(Some("opus-keep".to_string()));
        engine.next_source = "http://example.test/keep-next".to_string();
        engine.gapless.lock().await.source = "http://example.test/keep-slot".to_string();
        engine.live_sample_rate.store(88_200, Ordering::Relaxed);
        engine.playing = false;

        engine.consume_gapless_transition().await;

        assert_eq!(engine.source, "http://example.test/keep-current");
        assert_eq!(engine.duration, 333_333);
        assert_eq!(
            engine.position, 44_444,
            "position must be untouched on the None path"
        );
        assert_eq!(
            engine.current_format,
            AudioFormat::new(crate::audio::SampleFormat::S24, 88_200, 2),
        );
        assert_eq!(engine.live_codec(), Some("opus-keep".to_string()));
        assert_eq!(engine.next_source, "http://example.test/keep-next");
        assert_eq!(
            engine.gapless.lock().await.source,
            "http://example.test/keep-slot",
            "gapless slot source must be untouched on the None path",
        );
        assert_eq!(engine.live_sample_rate.load(Ordering::Relaxed), 88_200);
    }

    // -----------------------------------------------------------------------
    // F3: pause-aware decode-loop — consumed_notify unit tests
    //
    // These tests exercise the Notify primitive added to StreamingSource
    // without requiring a live PipeWire device or a real HTTP radio stream.
    // The integration behaviour (decode loop sleeping during pause) follows
    // directly from: paused → no consume → no notify fire → 500 ms timeout.
    // -----------------------------------------------------------------------

    /// Build a minimal `StreamingSource` backed by a filled ring buffer so we
    /// can drive `next()` manually in tests.
    fn make_source_with_data(
        samples: usize,
        paused: bool,
    ) -> (
        crate::audio::streaming_source::StreamingSource,
        crate::audio::streaming_source::StreamHandle,
    ) {
        use std::{num::NonZero, sync::Arc};

        use ringbuf::{HeapRb, traits::Split};
        use tokio::sync::Notify;

        use crate::audio::streaming_source::{SharedVisualizerCallback, StreamingSource};

        let rb = HeapRb::<f32>::new(samples.max(1));
        let (mut producer, consumer) = rb.split();
        {
            use ringbuf::traits::Producer;
            let data: Vec<f32> = (0..samples).map(|i| i as f32 * 0.001).collect();
            producer.push_slice(&data);
        }

        let viz: SharedVisualizerCallback = Arc::new(parking_lot::RwLock::new(None));
        let notify = Arc::new(Notify::new());

        let (source, handle) = StreamingSource::new(
            consumer,
            NonZero::new(2).unwrap(),
            NonZero::new(48000).unwrap(),
            viz,
            1.0,
            1.0,
            None,
            notify,
            true,
            Arc::new(std::sync::atomic::AtomicBool::new(true)),
            true,
            false,
        );

        if paused {
            handle.pause();
        }

        (source, handle)
    }

    /// While playing, `consumed_notify` fires once every CONSUMED_NOTIFY_STRIDE
    /// samples (512).  After consuming 1024 samples we expect exactly 2 fires
    /// stored as permits in the `Notify`.
    ///
    /// `Notify` coalesces into a single stored permit, so we verify by
    /// observing that at least one `notified()` future completes immediately
    /// (i.e., a permit was set), which proves the notify fired at least once.
    #[tokio::test]
    async fn streaming_source_fires_consumed_notify_while_playing() {
        // Fill ring with 1024 samples — enough for 2 STRIDE boundaries.
        let (mut source, handle) = make_source_with_data(1024, false);

        // Drain all samples.
        for _ in 0..1024 {
            let _ = source.next();
        }

        // At least one notify permit must be stored (fired at sample 512 or 1023).
        let fired = tokio::time::timeout(
            tokio::time::Duration::from_millis(1),
            handle.consumed_notify().notified(),
        )
        .await;
        assert!(
            fired.is_ok(),
            "consumed_notify should have fired after consuming ≥512 real samples"
        );
    }

    /// While paused, `StreamingSource::next()` returns silence without consuming
    /// from the ring buffer.  `consumed_notify` must NOT fire — the decode loop
    /// relies on this silence to sleep cheaply for the full 500 ms timeout
    /// instead of busy-waking every 5 ms.
    #[tokio::test]
    async fn streaming_source_does_not_fire_consumed_notify_while_paused() {
        // Ring has samples but the source is paused — next() returns silence.
        let (mut source, handle) = make_source_with_data(2048, true);

        // Pull many samples — all should be silence (paused), none consumed.
        for _ in 0..2048 {
            let s = source
                .next()
                .expect("paused source should return Some(0.0)");
            assert_eq!(s, 0.0, "paused source must emit silence");
        }

        // The notify must NOT have fired — no permit stored.
        let fired = tokio::time::timeout(
            tokio::time::Duration::from_millis(1),
            handle.consumed_notify().notified(),
        )
        .await;
        assert!(
            fired.is_err(),
            "consumed_notify must not fire while the stream is paused"
        );
    }

    /// Unpause wakes the waiting side: pause → consume silence (no fire) →
    /// resume → consume real samples → notify fires.
    #[tokio::test]
    async fn streaming_source_fires_consumed_notify_after_unpause() {
        let (mut source, handle) = make_source_with_data(1024, true);

        // Drain while paused — no fires.
        for _ in 0..512 {
            let _ = source.next();
        }
        // No permit yet.
        let still_silent = tokio::time::timeout(
            tokio::time::Duration::from_millis(1),
            handle.consumed_notify().notified(),
        )
        .await;
        assert!(still_silent.is_err(), "no fire expected while paused");

        // Resume and drain 512 real samples — exactly one STRIDE → one fire.
        handle.resume();
        for _ in 0..512 {
            let _ = source.next();
        }

        let woke = tokio::time::timeout(
            tokio::time::Duration::from_millis(10),
            handle.consumed_notify().notified(),
        )
        .await;
        assert!(
            woke.is_ok(),
            "consumed_notify must fire after resuming and consuming ≥512 samples"
        );
    }

    /// The write-retry timeout (500 ms) bounds the decode loop's exit latency
    /// when its generation is superseded while it is waiting on the notify.
    ///
    /// We simulate this by verifying that a `timeout(500ms, notified())` on an
    /// Arc<Notify> that nobody fires resolves within 600 ms.
    #[tokio::test]
    async fn write_retry_timeout_bounds_supersede_exit_latency() {
        use std::sync::Arc;

        use tokio::sync::Notify;

        let notify = Arc::new(Notify::new());
        let start = std::time::Instant::now();

        // Nobody fires the notify — should time out at 500 ms.
        let _ =
            tokio::time::timeout(tokio::time::Duration::from_millis(500), notify.notified()).await;

        let elapsed = start.elapsed();
        assert!(
            elapsed >= tokio::time::Duration::from_millis(490),
            "timeout should have elapsed ~500 ms, got {elapsed:?}"
        );
        assert!(
            elapsed < tokio::time::Duration::from_millis(600),
            "timeout should not overshoot by more than 100 ms, got {elapsed:?}"
        );
    }
    // =========================================================================
    // F2 — request_shutdown tests
    // =========================================================================

    /// `request_shutdown` must complete (not hang) well under any reasonable
    /// wall-clock budget. A fresh engine has no running decode loop and no
    /// render thread, so this should be near-instant; we allow 1 s to give
    /// CI headroom on slow machines.
    ///
    /// Requires a tokio runtime because `CustomAudioEngine::new()` captures
    /// `tokio::runtime::Handle::current()` inside `AudioRenderer::new()`.
    #[tokio::test]
    async fn request_shutdown_completes_within_timeout() {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        let mut engine = CustomAudioEngine::new();
        engine.request_shutdown();
        assert!(
            std::time::Instant::now() < deadline,
            "request_shutdown took longer than 1 s"
        );
    }

    /// Superseding the decode-loop generation is the primary signal that the
    /// decode loop should exit. After `request_shutdown`, the generation
    /// counter must be strictly greater than the initial value of 0.
    ///
    /// Requires a tokio runtime — see `request_shutdown_completes_within_timeout`.
    #[tokio::test]
    async fn request_shutdown_supersedes_decode_loop() {
        let mut engine = CustomAudioEngine::new();
        let gen_before = engine.decode_loop.current();
        engine.request_shutdown();
        let gen_after = engine.decode_loop.current();
        assert!(
            gen_after > gen_before,
            "generation must advance after request_shutdown (before={gen_before}, after={gen_after})"
        );
    }

    /// Calling `request_shutdown` twice must not panic. The generation counter
    /// is monotonically increasing, the render-thread join is idempotent (join
    /// on a completed thread returns immediately, and `take()` on a consumed
    /// `Option<JoinHandle>` returns `None`), and `renderer.stop()` is
    /// idempotent.
    ///
    /// Requires a tokio runtime — see `request_shutdown_completes_within_timeout`.
    #[tokio::test]
    async fn request_shutdown_is_idempotent() {
        let mut engine = CustomAudioEngine::new();
        engine.request_shutdown();
        // Must not panic:
        engine.request_shutdown();
    }

    // =========================================================================
    // Group M Lane 1 — module-level constants + LiveStringSlot newtype
    // =========================================================================

    /// `compute_watermarks` is time-based: the cushion (high) holds a constant
    /// ~`CUSHION_MS` of audio at any sample rate and the release (low) is
    /// `high / BACKPRESSURE_RELEASE_DIVISOR`. A 96k stream therefore gets ~2.18x
    /// the SAMPLES of a 44.1k stream for the same DURATION — the property whose
    /// absence (a fixed 96_000-sample cushion = ~0.5s at 96k) drove the issue-9
    /// hi-res rebuffer deadlock. `frame_rate == 0` must never backpressure.
    #[test]
    fn compute_watermarks_hold_constant_time_across_rates() {
        let (high_44, low_44) = compute_watermarks(44_100 * 2, 0);
        assert_eq!(high_44, (88_200u64 * CUSHION_MS / 1000) as usize);
        assert_eq!(low_44, high_44 / BACKPRESSURE_RELEASE_DIVISOR as usize);

        let (high_96, _) = compute_watermarks(96_000 * 2, 0);
        assert_eq!(high_96, (192_000u64 * CUSHION_MS / 1000) as usize);
        // Same ~1.1s of audio → proportionally more samples at the higher rate.
        assert!(high_96 > high_44);

        // Unknown format → a non-triggering high so the loop never backpressures.
        assert_eq!(compute_watermarks(0, 0), (usize::MAX, 0));
    }

    // -----------------------------------------------------------------------
    // backpressure_step state machine (shared by primary + crossfade loops)
    // -----------------------------------------------------------------------

    /// 44.1k stereo frame rate for the backpressure tests.
    const BP_FRAME_RATE: u32 = 88_200;

    #[test]
    fn backpressure_enters_at_high_watermark() {
        let (high, _) = compute_watermarks(BP_FRAME_RATE, 0);
        let mut active = false;
        let action = backpressure_step("TEST", high, BP_FRAME_RATE, 0, false, &mut active);
        assert_eq!(
            action,
            BackpressureAction::Sleep(std::time::Duration::from_millis(100))
        );
        assert!(active, "latch must engage at the high watermark");
    }

    #[test]
    fn backpressure_holds_between_low_and_high_while_active() {
        let (high, low) = compute_watermarks(BP_FRAME_RATE, 0);
        let mut active = true;
        let mid = (low + high) / 2;
        let action = backpressure_step("TEST", mid, BP_FRAME_RATE, 0, false, &mut active);
        assert_eq!(
            action,
            BackpressureAction::Sleep(std::time::Duration::from_millis(50))
        );
        assert!(active, "latch must stay engaged while draining toward low");
    }

    #[test]
    fn backpressure_releases_at_low_watermark() {
        let (_, low) = compute_watermarks(BP_FRAME_RATE, 0);
        let mut active = true;
        let action = backpressure_step("TEST", low, BP_FRAME_RATE, 0, false, &mut active);
        assert_eq!(action, BackpressureAction::Proceed);
        assert!(!active, "latch must release at the low watermark");
    }

    #[test]
    fn backpressure_never_engages_for_infinite_streams() {
        let mut active = false;
        let action = backpressure_step("TEST", usize::MAX, BP_FRAME_RATE, 0, true, &mut active);
        assert_eq!(
            action,
            BackpressureAction::Proceed,
            "radio must proceed at ANY buffer count"
        );
        assert!(!active, "radio must never enter backpressure");
    }

    #[test]
    fn backpressure_never_engages_before_frame_rate_is_known() {
        let mut active = false;
        // frame_rate 0 → compute_watermarks high == usize::MAX (non-triggering).
        let action = backpressure_step("TEST", usize::MAX - 1, 0, 0, false, &mut active);
        assert_eq!(action, BackpressureAction::Proceed);
        assert!(
            !active,
            "frame_rate 0 yields a non-triggering high watermark"
        );
    }

    /// Pins the gate invariant that REPLACES the old `phantom_unreachable` test:
    /// when a crossfade is armed OR active the inline gapless swap stands down, so
    /// the renderer's position-based trigger owns the transition. This is what
    /// lets BASE_HIGH grow the decoded cushion past the crossfade lead without
    /// re-arming the phantom-crossfade (dead-air) bug. Both predicates matter —
    /// render_tick flips Armed->Active synchronously while the engine clears the
    /// prepared slot asynchronously.
    #[test]
    fn should_attempt_gapless_swap_defers_when_crossfade_armed_or_active() {
        assert!(
            !should_attempt_gapless_swap(true, false),
            "armed crossfade ⇒ defer to the renderer trigger"
        );
        assert!(
            !should_attempt_gapless_swap(false, true),
            "active crossfade ⇒ defer (covers the Armed->Active window)"
        );
        assert!(
            should_attempt_gapless_swap(false, false),
            "no crossfade ⇒ the inline gapless swap proceeds"
        );
    }

    /// `LiveStringSlot::set` overwrites prior values and accepts `None` to
    /// clear. Preserves the historical write semantics of the blocking
    /// `RwLock::write()` path used by decoder-init.
    #[test]
    fn live_string_slot_set_get_roundtrip() {
        let slot = LiveStringSlot::new();
        slot.set(Some("foo".to_string()));
        assert_eq!(slot.get(), Some("foo".to_string()));
        slot.set(Some("bar".to_string()));
        assert_eq!(slot.get(), Some("bar".to_string()));
        slot.set(None);
        assert_eq!(slot.get(), None);
    }

    /// `LiveStringSlot::reset` is the B11 hot-path-safe equivalent of
    /// `set(None)` — must clear a previously-set value.
    #[test]
    fn live_string_slot_reset_clears() {
        let slot = LiveStringSlot::new();
        slot.set(Some("x".to_string()));
        assert_eq!(slot.get(), Some("x".to_string()));
        slot.reset();
        assert_eq!(slot.get(), None);
    }

    /// `LiveStringSlot::clone_arc` must hand out a clone that shares state
    /// with the slot — the IcyMetadataReader callback writes through the
    /// cloned `Arc` on its own thread, and the slot's `get()` must observe
    /// the write.
    #[test]
    fn live_string_slot_clone_arc_shares_state() {
        let slot = LiveStringSlot::new();
        let shared = slot.clone_arc();
        // Simulate the IcyMetadataReader-callback pattern: lock the cloned
        // Arc directly and store a value.
        {
            let mut guard = shared.write().expect("write lock");
            *guard = Some("from_callback".to_string());
        }
        assert_eq!(slot.get(), Some("from_callback".to_string()));
    }
}
