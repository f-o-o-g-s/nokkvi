# Audio-engine type-level invariants — fanout plan (IG-3 engine + IG-6 + IG-7 + IG-8 + IG-10 + IG-11 + IG-13 + B11 + Drift #13)

Closes the audio-engine half of `.agent/audit-progress.md` §6 (the queue half landed in 2026-05-08 via `queue-typestate-igs.md`). Picks up the next high-leverage cluster: the audio engine's load-bearing invariants live almost entirely in CLAUDE.md prose + author discipline today; this plan moves them into the type system.

Last verified baseline: **2026-05-08, `main @ HEAD = 6fa04dd`**.

Source reports leaned on: `~/nokkvi-audit-results/_SYNTHESIS.md` §6 (IG ranking) + §4 (Drift #13), `~/nokkvi-audit-results/monoliths-data.md` (engine.rs / renderer.rs invariant gaps), `~/nokkvi-audit-results/backend-boundary.md` §4 (audio lock-call-site inventory) + §6 (IG-1..IG-12).

---

## 1. Goal & rubric

The audio engine is the second-most agent-bug-prone surface in nokkvi after the queue. The remaining audio-engine invariant gaps are doc-only:

| ID | Doc-only contract | Today's enforcement | Failure mode |
|---|---|---|---|
| **IG-3** (engine side) | Mode toggles must call `engine.reset_next_track()` | `PlaybackController` calls it at lines 416, 445, 465 | A new mode hotkey forgets the reset; gapless prep transitions to the wrong song |
| **IG-6** | Each spawned decode loop captures `decode_generation` and exits when it changes | 6 hand-written `decode_generation.fetch_add(1)` sites in `engine.rs` (501, 915, 964, 1369, 1532, 1944) | New code path forgets the bump; old loop races the new one for the same decoder |
| **IG-7** | `source_generation` increments on user actions (set_source, gapless swap), NOT on crossfade finalize | 2 hand-written `source_generation.fetch_add(1)` sites + a comment at line 1415 | Reorganized crossfade-finalize block silently increments; in-flight `on_renderer_finished` callback is invalidated incorrectly |
| **IG-8** | `CrossfadePhase` transitions are `Idle → Active → OutgoingFinished → Idle` | 3-variant `Copy` enum with 5 mutation sites (engine.rs 1338, 1352, 1422, 1838, plus reads at 1746-1748) | Future code writes `OutgoingFinished` straight from `Idle`; data that should be tied to the phase (`crossfade_decoder`, `crossfade_incoming_source`) is parallel state |
| **IG-10** | `pub` atomics on `AudioRenderer` are written only via `set_engine_reference` | 3 `pub` fields (`engine`, `source_generation`, `decoder_eof`) directly assignable | Anyone with `&mut AudioRenderer` rotates the back-reference; future caller bypasses the engine's setter and the renderer ends up pointing at a stale engine |
| **IG-11** | `set_pending_replay_gain` → `set_source`/`load_track` → `play()` is one atomic 3-step | 5 hand-paired sites in `playback_controller.rs` (227-229, 284-286, 630-631, 688-689, 767-768) | Future caller forgets the RG stash; track plays at the previous track's gain |
| **IG-13** | Three tokio mutexes (`next_source_shared`, `next_decoder`, `next_track_prepared`) acquired in different orders across awaits | No deadlock today (each guard drops between awaits); enforced by reading every site | Future "hold across await" diff deadlocks under contention |
| **Drift #13** | Renderer crossfade armed/active = 8 fields + 2 booleans for one state machine | `disarm`/`cancel`/`finalize` each reset 4-6 fields in lockstep | Adding a new armed field that one of the resetters misses leaves stale state into the next track |
| **B11** | `live_icy_metadata.try_write()` vs `live_codec_name.write()` asymmetry in `set_source` | 3-line discrepancy with no recorded intent | Either both should silently skip on contention or both should block; nothing tests which; the live string view diverges from the codec name on contention |

Rubric (in order): (1) prevent the bug class outright; (2) keep the public API ergonomic; (3) UI crate touch surface = 0 (this is `data/`-only work); (4) tests pass unchanged after each lane unless a lane explicitly migrates a test for ergonomics.

---

## 2. Architecture

Four parallel lanes touching three files in `data/`: `audio/engine.rs`, `audio/renderer.rs`, `backend/playback_controller.rs`. No new dependencies. No public surface change visible to the UI crate (the UI calls `app_service.audio_engine()` and the `PlaybackController` delegations stay shape-stable; only internal types and `pub` visibility narrow).

### 2.1 Lane A — atomic-sharing API (IG-6 + IG-7 + IG-10 + B11)

**File**: `audio/engine.rs` + `audio/renderer.rs` + new `audio/generation.rs` (or inline in `engine.rs`).

```rust
// generation.rs (new) — typed wrappers around the engine ↔ renderer atomics

#[derive(Clone, Debug)]
pub struct DecodeLoopHandle {
    counter: Arc<AtomicU64>,
}

impl DecodeLoopHandle {
    pub fn new() -> Self { Self { counter: Arc::new(AtomicU64::new(0)) } }

    /// Invalidate every spawned decode loop currently observing the previous
    /// generation. Returns the new generation so the caller can spawn a fresh
    /// loop captured to that value. The single mutator — every "stop the
    /// decode loop" path goes through this method.
    pub fn supersede(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// Lock-free generation read for the spawned decode loops.
    pub fn current(&self) -> u64 { self.counter.load(Ordering::Acquire) }
}

#[derive(Clone, Debug)]
pub struct SourceGeneration {
    counter: Arc<AtomicU64>,
}

impl SourceGeneration {
    pub fn new() -> Self { Self { counter: Arc::new(AtomicU64::new(0)) } }

    /// User-driven source change (manual skip, seek, set_source).
    /// Renderer's stale-callback guard observes this and discards completions.
    pub fn bump_for_user_action(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::Release) + 1
    }

    /// Internal gapless transition (decode loop swapped decoder inline).
    /// We DO bump because the source URL changed — the renderer's callback
    /// snapshot must invalidate. (Today line 794 does this; named so the
    /// intent is explicit.)
    pub fn bump_for_gapless(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::Release) + 1
    }

    /// Crossfade-finalize path: source URL changed but the renderer
    /// already swapped streams synchronously and re-armed via finalize.
    /// No-op so the comment at line 1415 ("Don't increment source_generation
    /// here") is encoded in the type rather than the prose.
    pub fn accept_internal_swap(&self) {
        // intentional no-op — keeps intent reviewable at every site
    }

    /// Lock-free read for the renderer's stale-callback guard.
    pub fn current(&self) -> u64 { self.counter.load(Ordering::Acquire) }
}

// renderer.rs — the three pub atomics seal behind one setter.

pub struct AudioRenderer {
    // ...
    engine: Weak<tokio::sync::Mutex<super::engine::CustomAudioEngine>>,  // pub → private
    source_generation: SourceGeneration,                                   // pub Arc<AtomicU64> → private newtype
    decoder_eof: Arc<AtomicBool>,                                          // pub → private
    // ...
}

impl AudioRenderer {
    /// Sealed setter — replaces the historical `pub` field assignment in
    /// `engine.set_engine_reference`. The only path to install the engine
    /// back-link, the source-generation handle, and the EOF flag.
    pub fn set_engine_link(
        &mut self,
        engine: Weak<tokio::sync::Mutex<super::engine::CustomAudioEngine>>,
        source_generation: SourceGeneration,
        decoder_eof: Arc<AtomicBool>,
    ) {
        self.engine = engine;
        self.source_generation = source_generation;
        self.decoder_eof = decoder_eof;
    }

    /// Renderer's stale-callback guard reads via the typed snapshot.
    pub(crate) fn source_generation_snapshot(&self) -> u64 {
        self.source_generation.current()
    }

    /// Decoder-EOF flag (still shared with engine; clone the Arc only here).
    pub(crate) fn decoder_eof_arc(&self) -> Arc<AtomicBool> {
        self.decoder_eof.clone()
    }
}
```

**B11**: pick `try_write()` consistently for both `live_icy_metadata` and `live_codec_name` in `set_source` (engine.rs:235-240). Rationale: ICY-metadata and codec-name setters run during decoder probe, which is on the engine async path; if they contend with a UI reader (the player bar polls `live_icy_metadata()` / `live_codec()`), blocking the set_source path would stall track changes. `try_write` with the implicit "leave stale data; next set_source overwrites" is the documented intent — the codebase already accepts this on line 235 today. Lane A makes both 235 and 238 use `try_write` and adds a one-line comment explaining.

(Optionally bundle into a `LiveStreamMetadata` mini-struct if Lane A's implementer feels it's a clean extraction. Not required.)

### 2.2 Lane B — Crossfade typestate (IG-8 + Drift #13)

**File**: `audio/engine.rs` + `audio/renderer.rs`.

#### 2.2.1 Engine side — `CrossfadePhase` enum-with-data

```rust
// engine.rs

pub enum CrossfadePhase {
    /// Normal single-track playback.
    Idle,
    /// Two decoders active, blending audio in renderer.
    /// `decoder` is the incoming track's decoder shared with the
    /// crossfade decode loop; setting it to `None` (via the inner Mutex)
    /// signals the spawned loop to exit.
    Active {
        decoder: Arc<tokio::sync::Mutex<Option<AudioDecoder>>>,
        incoming_source: String,
    },
    /// Outgoing decoder finished, incoming still draining.
    /// Same data shape as `Active`; transition is one-way (set on EOF
    /// in `on_decoder_finished`, finalized in `try_finalize_crossfade`).
    OutgoingFinished {
        decoder: Arc<tokio::sync::Mutex<Option<AudioDecoder>>>,
        incoming_source: String,
    },
}
```

This **moves** `crossfade_decoder` and `crossfade_incoming_source` (today free fields on `CustomAudioEngine`) **into** the variants. Mutation sites go from "set field + flip enum + flip enum" to one `mem::replace` per transition:

```rust
// start_crossfade (engine.rs:1290) — Idle → Active
let CrossfadePhase::Idle = self.crossfade_phase else {
    debug!("🔀 [CROSSFADE] Already active, skipping");
    return false;
};
// ... build incoming_source / decoder Arc ...
self.crossfade_phase = CrossfadePhase::Active { decoder, incoming_source };

// on_decoder_finished (engine.rs:1834) — Active → OutgoingFinished
if let CrossfadePhase::Active { decoder, incoming_source } =
    std::mem::replace(&mut self.crossfade_phase, CrossfadePhase::Idle) {
    self.crossfade_phase = CrossfadePhase::OutgoingFinished { decoder, incoming_source };
}

// finalize_crossfade_engine (engine.rs:1365) — Active|OutgoingFinished → Idle
let (decoder, incoming_source) = match std::mem::replace(&mut self.crossfade_phase, CrossfadePhase::Idle) {
    CrossfadePhase::Idle => return,
    CrossfadePhase::Active { decoder, incoming_source }
    | CrossfadePhase::OutgoingFinished { decoder, incoming_source } => (decoder, incoming_source),
};
// ... promote decoder to primary, set self.source = incoming_source, etc.

// cancel_crossfade (engine.rs:1347) — Active|OutgoingFinished → Idle
match std::mem::replace(&mut self.crossfade_phase, CrossfadePhase::Idle) {
    CrossfadePhase::Idle => return,
    CrossfadePhase::Active { decoder, .. }
    | CrossfadePhase::OutgoingFinished { decoder, .. } => {
        *decoder.lock().await = None; // signal the spawned crossfade decode loop to exit
    }
}
```

Read sites (`crossfade_phase()` accessor at line 1284, the read-only checks at 1291, 1746-1748, 1771, 1825, 1834, 1845) become explicit matches or a `pub fn is_active(&self) -> bool` helper on `CrossfadePhase`. The `Copy` derive goes away (moved data is non-Copy); accessors must clone or match.

The accessor at line 1284 (`pub fn crossfade_phase(&self) -> CrossfadePhase`) currently returns `Copy` and is used by tests. Replace with:

```rust
pub fn crossfade_is_active_or_finished(&self) -> bool {
    matches!(self.crossfade_phase, CrossfadePhase::Active { .. } | CrossfadePhase::OutgoingFinished { .. })
}
pub fn crossfade_is_idle(&self) -> bool {
    matches!(self.crossfade_phase, CrossfadePhase::Idle)
}
```

(Or a single `is_active(&self) -> bool` if tests only need that.)

#### 2.2.2 Renderer side — `CrossfadeState` enum collapses 8 fields

```rust
// renderer.rs

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

pub struct AudioRenderer {
    // ... keep:
    crossfade_state: CrossfadeState,
    /// Transient post-finalize value; consumed once by `take_crossfade_elapsed_ms`.
    /// Does NOT belong inside `CrossfadeState` because it survives one tick
    /// after the state returns to Idle (engine reads it after render_tick's finalize).
    crossfade_finalized_elapsed_ms: u64,
    // ... drop:
    //   crossfade_active, crossfade_duration_ms, crossfade_start_time,
    //   crossfade_incoming_format, crossfade_armed, crossfade_armed_duration_ms,
    //   crossfade_armed_incoming_format, crossfade_armed_track_duration_ms,
    //   crossfade_stream  (moves into CrossfadeState::Active)
}
```

The 30+ read/write sites collapse to match arms or `let CrossfadeState::Active { .. } = …` patterns. Public accessors stay shape-compatible:

```rust
pub fn is_crossfade_active(&self) -> bool { matches!(self.crossfade_state, CrossfadeState::Active { .. }) }
pub fn is_crossfade_armed(&self)  -> bool { matches!(self.crossfade_state, CrossfadeState::Armed  { .. }) }
pub fn crossfade_armed_duration_ms(&self) -> u64 {
    if let CrossfadeState::Armed { duration_ms, .. } = &self.crossfade_state { *duration_ms } else { 0 }
}
pub fn crossfade_armed_incoming_format(&self) -> &AudioFormat {
    if let CrossfadeState::Armed { incoming_format, .. } = &self.crossfade_state { incoming_format }
    else { &INVALID_FORMAT }  // sentinel (declare a const) — caller's existing code handles invalid format
}
```

`render_tick` (renderer.rs:889-977) drives all transitions; rewrite the body's `if self.crossfade_active && self.tick_crossfade()` chain as a single match on `&mut self.crossfade_state`.

`disarm_crossfade` becomes `self.crossfade_state = CrossfadeState::Idle;` only when the previous state was Armed. `cancel_crossfade` likewise. `finalize_crossfade` consumes the Active variant via `mem::replace`.

The `arm_crossfade` method (line ~640) takes the same args today; under the typestate it sets `CrossfadeState::Armed { duration_ms, incoming_format, track_duration_ms }`. The early-return guards (already-armed, already-active) become explicit match arms.

**Why this is the heaviest lane**: ~30-40 site updates spread across renderer.rs's render-tick + arm/disarm/start/cancel/tick/finalize methods, plus engine.rs's `start_crossfade` / `cancel_crossfade` / `finalize_crossfade_engine` / `try_finalize_crossfade` / `try_start_crossfade_transition` / `on_decoder_finished`. The blast radius is wholly internal to `data/audio/`; the engine's public API surface to `PlaybackController` (which only calls `crossfade_phase()` and `set_crossfade_enabled()` / `set_crossfade_duration()`) stays compatible.

### 2.3 Lane C — Controller invariants (IG-11 + IG-3 engine-side)

**Files**: `audio/engine.rs` (API additions only) + `backend/playback_controller.rs` + `services/queue/mod.rs` (signature change on the 3 mode-toggle methods).

#### 2.3.1 `engine.load_track_with_rg(url, rg)` — IG-11

```rust
// engine.rs (new method on CustomAudioEngine)

/// Atomic three-step: stash ReplayGain → set source → caller's `play()`.
/// Replaces the hand-paired `set_pending_replay_gain` + `set_source` /
/// `load_track` pattern. The caller still calls `play()` afterward, but
/// the RG-stash + set-source pair is now uncuttable.
pub async fn load_track_with_rg(
    &mut self,
    url: &str,
    rg: Option<crate::types::song::ReplayGain>,
) {
    self.set_pending_replay_gain_internal(rg);
    self.set_source(url.to_string()).await;
}
```

Mark `set_pending_replay_gain` and `set_source` as `pub(crate)` (rename existing `set_pending_replay_gain` body to `set_pending_replay_gain_internal`). The standalone setters stay reachable for the gapless-prep path (`store_prepared_decoder` at line 1163-1165 calls `set_pending_crossfade_replay_gain` separately) but the *primary-stream* RG-stash+load pair is gated behind `load_track_with_rg`.

#### 2.3.2 `ModeToggleEffect` — IG-3 engine-side

```rust
// services/queue/mod.rs (or a new types/mode_toggle.rs)

#[must_use = "ModeToggleEffect must be applied via `effect.apply_to(&engine).await` to reset gapless state — forgetting silently corrupts next-track prep"]
pub struct ModeToggleEffect {
    _seal: (),
}

impl ModeToggleEffect {
    pub(crate) fn new() -> Self { Self { _seal: () } }

    pub async fn apply_to(self, engine: &tokio::sync::Mutex<crate::audio::CustomAudioEngine>) {
        let mut guard = engine.lock().await;
        guard.reset_next_track().await;
    }
}

// QueueManager (signature changes — return ModeToggleEffect)
impl QueueManager {
    pub fn toggle_shuffle(&mut self) -> Result<ModeToggleEffect> { ... }
    pub fn set_repeat(&mut self, mode: RepeatMode) -> Result<ModeToggleEffect> { ... }
    pub fn toggle_consume(&mut self) -> Result<ModeToggleEffect> { ... }
}
```

`PlaybackController::toggle_random` / `cycle_repeat` / `toggle_consume` (lines 407, 425, 456) consume the effect:

```rust
pub async fn toggle_random(&self) -> Result<bool> {
    // ... existing setup ...
    let effect = queue_manager.toggle_shuffle()?;
    drop(queue_manager);  // release queue lock before engine lock
    effect.apply_to(&self.audio_engine).await;
    Ok(new_random_state)
}
```

`#[must_use]` plus CI's `-D warnings` means a future caller who calls `queue_manager.toggle_shuffle()?;` and discards the effect compile-fails. The drop branch (`let _ = qm.toggle_shuffle()?;`) is explicit and reviewable.

### 2.4 Lane D — Gapless mutex bundle (IG-13)

**File**: `audio/engine.rs` only.

Today the gapless prep state is split across three independent `tokio::sync::Mutex`es: `next_decoder` (Option<AudioDecoder>), `next_track_prepared` (bool), `next_source_shared` (String). Each is taken individually across awaits in the decode loop, in `prepare_next_track`, in `store_prepared_decoder`, in `consume_gapless_transition`, in `start_crossfade`, in `load_prepared_track`, in `reset_next_track` — at least 30 lock sites. The audit (backend-boundary.md §4) flags the lock order as enforced only by reading every site.

Bundle into one `GaplessSlot`:

```rust
// engine.rs (or audio/gapless.rs — implementer's call)

pub(crate) struct GaplessSlot {
    /// Decoder for the prepared next track. `None` when nothing is staged.
    pub decoder: Option<AudioDecoder>,
    /// Source URL of the prepared track. Empty when not staged.
    pub source: String,
    /// True when `decoder` is non-None and the renderer can use it for
    /// gapless transition. Kept as an explicit field rather than derived
    /// from `decoder.is_some()` because the decode loop sets it to false
    /// AFTER taking the decoder (so the next loop iteration knows the
    /// slot is mid-swap).
    pub prepared: bool,
}

impl GaplessSlot {
    pub fn new() -> Self { Self { decoder: None, source: String::new(), prepared: false } }
    pub fn is_prepared(&self) -> bool { self.prepared && self.decoder.is_some() }
    pub fn clear(&mut self) {
        self.decoder = None;
        self.source.clear();
        self.prepared = false;
    }
}

pub struct CustomAudioEngine {
    // ... drop:
    //   next_decoder: Arc<tokio::sync::Mutex<Option<AudioDecoder>>>,
    //   next_track_prepared: Arc<tokio::sync::Mutex<bool>>,
    //   next_source_shared: Arc<tokio::sync::Mutex<String>>,
    //   next_source: String,                          // engine-local, not shared
    //   next_format: AudioFormat,                     // engine-local, not shared
    // ... add:
    gapless: Arc<tokio::sync::Mutex<GaplessSlot>>,
    next_format: AudioFormat,  // STAYS — read on engine async path only, not from decode loop
    // ... `next_source` field becomes the slot's `.source`
}
```

**`next_format` decision**: today `next_format` is read at engine-only sites (`prepare_next_track` line 1108, `load_prepared_track` line 1547, `start_crossfade` line 1314 reads from `next_decoder.lock().await.format()` directly so doesn't need `next_format`). It is NOT shared with the decode loop. Keep as a plain field, do not move into `GaplessSlot`.

**Lock order**: with one mutex, every site that previously took N of {`next_decoder`, `next_track_prepared`, `next_source_shared`} now takes exactly one `gapless.lock().await`. Lock order question is gone.

**Site count**: roughly 30 → roughly 12 (one `gapless.lock().await` per call site that previously took 1-3 of the old mutexes).

**Awareness with Lane B**: Lane B introduces `CrossfadePhase::Active { decoder: Arc<tokio::sync::Mutex<Option<AudioDecoder>>>, incoming_source: String }`. That `decoder` Arc is a SEPARATE Mutex from `gapless` — it's the crossfade decode loop's working slot, not the gapless prep slot. **The two stay separate because**:

1. They have different lifecycles: gapless slot is reset on `reset_next_track`; crossfade slot is consumed by `finalize_crossfade_engine`.
2. The crossfade decode loop and the primary decode loop run concurrently; bundling them into one mutex would serialize their writes against each other unnecessarily.
3. Lane B's `CrossfadePhase::Active` already encodes the crossfade decoder ownership in the type system; Lane D handles the orthogonal gapless state.

### 2.5 Lane interactions

| Lane pair | Shared file | Conflict surface | Mitigation |
|---|---|---|---|
| A ↔ B | `engine.rs` constructor + struct fields | A swaps `decode_generation`/`source_generation` types; B moves `crossfade_decoder`/`crossfade_incoming_source` into `CrossfadePhase` variants. Disjoint fields. | ~10-line rebase in `CustomAudioEngine::new()`. |
| A ↔ B | `renderer.rs` constructor + struct fields | A removes 3 `pub` fields (`engine`, `source_generation`, `decoder_eof`); B replaces 8 crossfade fields with `crossfade_state`. Disjoint fields. | ~10-line rebase in `AudioRenderer::new()`. |
| A ↔ D | `engine.rs` constructor | A swaps atomic types; D removes 3 mutex fields, adds 1. Disjoint fields. | ~10-line rebase in `CustomAudioEngine::new()`. |
| A ↔ D | `engine.rs` decode loop (line 760-810 gapless inline-swap) | A bumps `source_generation` (line 794); D rewrites the 4 mutex locks at 762, 764, 786, 790-791 to one `gapless.lock().await`. Same function body. | The line A touches and the lines D touches are not adjacent (line 794 vs 762/786/790). Mid-function rebase is mechanical. |
| B ↔ D | `engine.rs` constructor | B inits `CrossfadePhase::Idle`; D inits `Arc::new(Mutex::new(GaplessSlot::new()))`. Disjoint fields. | ~5-line rebase. |
| C ↔ A/B/D | `engine.rs` adds `load_track_with_rg` method | C adds a new public method. A/B/D don't touch RG / load surface. | Zero conflict. |
| C ↔ A/B/D | `services/queue/mod.rs` mode-toggle signatures | C changes 3 mode-toggle return types. A/B/D don't touch `services/queue/`. | Zero conflict. |
| C ↔ A/B/D | `playback_controller.rs` | C edits 8 sites (5 RG-stash + 3 mode-toggle). A/B/D don't touch `playback_controller.rs`. | Zero conflict. |

**Recommended merge order**: **D → A → B → C**.

Rationale:
- **D first**: bundling the gapless mutexes simplifies the field landscape that A and B then rebase against.
- **A second**: typed atomic wrappers settle before B's renderer constructor lands (B's constructor inits `CrossfadeState::Idle` AND no longer touches the 3 sealed atomics — it just calls `set_engine_link` if needed).
- **B third**: heaviest lane; rebases off A's renderer changes and D's gapless changes (B's body in `start_crossfade` reads `next_track_prepared` today; under D it reads `self.gapless.lock().await.is_prepared()`).
- **C last**: fully independent; can land first or last.

C, A, and D can run in any order if the implementer is comfortable rebasing B against whatever has merged. **Acceptable alternate**: C first (it's independent and demonstrates the controller-side win quickly), then D → A → B.

---

## 3. Per-lane scope (sites verified at baseline `6fa04dd`)

### Lane A — files & sites

`data/src/audio/engine.rs`:
- L106 — drop `decode_generation: Arc<AtomicU64>`; add `decode_loop: DecodeLoopHandle`.
- L132 — drop `source_generation: Arc<AtomicU64>`; add `source_generation: SourceGeneration`.
- L187, L195 — constructor inits use `DecodeLoopHandle::new()` / `SourceGeneration::new()`.
- L235-240 — B11: change `live_codec_name.write()` to `live_codec_name.try_write()` (or hoist both into a `LiveStreamMetadata::set_codec(None)` setter); add a one-line comment recording intent.
- L251 — `self.source_generation.fetch_add(1, …)` → `self.source_generation.bump_for_user_action();`.
- L491 — `self.source_generation.clone()` (decode loop captures by clone) → `self.source_generation.clone()` (struct is `Clone`; same shape).
- L501 — `let my_gen = self.decode_generation.fetch_add(1, …) + 1;` → `let my_gen = self.decode_loop.supersede();`.
- L502 — `let decode_gen = self.decode_generation.clone();` → `let decode_gen = self.decode_loop.clone();`.
- L794 — `source_generation.fetch_add(1, …)` → `source_generation.bump_for_gapless();`.
- L915, L964, L1369, L1532, L1944 — five `self.decode_generation.fetch_add(1, …)` sites → `self.decode_loop.supersede();`.
- L1415 — replace the comment-only "don't increment source_generation" with `self.source_generation.accept_internal_swap();` so the no-op is typed.
- L1618 — `pub fn source_generation(&self) -> u64 { self.source_generation.load(…) }` → `… { self.source_generation.current() }`.
- L1677-1682 — `set_engine_reference`: replace direct `pub` field assignment with `renderer.set_engine_link(engine, self.source_generation.clone(), self.decoder_eof.clone());`.

`data/src/audio/renderer.rs`:
- L58, L61, L63 — drop `pub` modifier; change `source_generation: Arc<AtomicU64>` to `source_generation: SourceGeneration`. (Default in constructor: `SourceGeneration::new()` — the engine overwrites via `set_engine_link` immediately after construction.)
- L210 — constructor init `source_generation: SourceGeneration::new()`.
- L985 — `let generation = self.source_generation.load(Ordering::Acquire);` → `let generation = self.source_generation.current();`.
- L994 — `if engine.source_generation() == src_gen` (engine accessor unchanged shape, returns u64) — leave as-is.
- New method on `AudioRenderer`: `pub fn set_engine_link(&mut self, engine: Weak<...>, source_generation: SourceGeneration, decoder_eof: Arc<AtomicBool>)`.

New file `data/src/audio/generation.rs` (or inline at top of `engine.rs` — implementer's call). Declared via `mod generation;` in `data/src/audio/mod.rs`. Re-export `pub(crate) use generation::{DecodeLoopHandle, SourceGeneration};` if accessed across modules.

### Lane B — files & sites

`data/src/audio/engine.rs`:
- L29-36 — `CrossfadePhase` becomes enum-with-data per §2.2.1. Drop `Copy` derive (cannot derive on a variant containing `Arc<Mutex<...>>`); keep `Debug`. Manual `Clone` via `Arc::clone`-on-decoder if needed (but read-only access prefers borrow).
- L145 — field `crossfade_phase: CrossfadePhase` (no shape change).
- L151 — drop `crossfade_decoder: Arc<tokio::sync::Mutex<Option<AudioDecoder>>>` (moves into variants).
- L153 — drop `crossfade_incoming_source: String` (moves into variants).
- L198, L201, L202 — constructor now inits only `crossfade_phase: CrossfadePhase::Idle`.
- L1284 — `pub fn crossfade_phase(&self) -> CrossfadePhase` accessor: replace with `pub fn crossfade_is_active_or_finished(&self) -> bool` and/or `pub fn crossfade_is_idle(&self) -> bool` per §2.2.1. Find the call sites and migrate.
- L1290-1344 — `start_crossfade` body uses `mem::replace` + match; constructs `CrossfadePhase::Active { decoder, incoming_source }`.
- L1347-1360 — `cancel_crossfade` body uses `mem::replace` to extract decoder Arc; sets `*decoder.lock().await = None;`.
- L1365-1428 — `finalize_crossfade_engine` body uses `mem::replace` to extract decoder + incoming_source.
- L1746-1748, L1771, L1825, L1834, L1838, L1845 — 6 read sites become `matches!(self.crossfade_phase, CrossfadePhase::…)` or destructure-let.
- L1432-1517 (`start_crossfade_decode_loop`) — clones the decoder Arc out of the `CrossfadePhase::Active` variant when spawning. The spawned task captures the clone.

`data/src/audio/renderer.rs`:
- L66-76 — drop the 8 crossfade fields; add `crossfade_state: CrossfadeState`. Keep `crossfade_finalized_elapsed_ms` as a separate field.
- L212-220 — constructor init `crossfade_state: CrossfadeState::Idle`.
- L468, L525, L780, L819, L850, L867-878, L903-904, L914, L929-963 — every read of `crossfade_active`/`crossfade_armed`/`crossfade_armed_*` becomes a match or `matches!`.
- L640-672 (`arm_crossfade`) — body sets `self.crossfade_state = CrossfadeState::Armed { duration_ms, incoming_format, track_duration_ms }` after early-return guards.
- L675-680 (`disarm_crossfade`) — body becomes `if matches!(self.crossfade_state, CrossfadeState::Armed { .. }) { self.crossfade_state = CrossfadeState::Idle; }`.
- L684-717 (`start_crossfade`) — body match-extracts the incoming format; constructs `CrossfadeState::Active { stream, started_at: Instant::now(), duration_ms, incoming_format }`.
- L720-746 (`cancel_crossfade`) — body uses `mem::replace` to drop the Active stream; resets to Idle; clears `pending_crossfade_replay_gain`.
- L779-814 (`tick_crossfade`) — body matches Active variant; reads `started_at`, `duration_ms`; returns the `progress >= 1.0` bool unchanged.
- L818-859 (`finalize_crossfade`) — body uses `mem::replace` to extract Active variant; promotes its `stream` to primary; sets `crossfade_finalized_elapsed_ms`; sets state to Idle. Replaces the inline `disarm_crossfade()` call with the implicit Idle transition.
- L889-977 (`render_tick`) — replace the `if self.crossfade_active …`/`if self.crossfade_armed …` chains with one match on `&mut self.crossfade_state` covering Idle, Armed, Active. The Armed-trigger path stays at the top; the Active-tick path stays in the middle; the no-crossfade path falls through to track-completion check.
- `crossfade_armed_duration_ms`, `crossfade_armed_incoming_format`, `is_crossfade_active`, `is_crossfade_armed` — accessor bodies become destructure-or-default. **Public signature unchanged** — engine consumers see the same accessor surface.

### Lane C — files & sites

`data/src/audio/engine.rs`:
- New method: `pub async fn load_track_with_rg(&mut self, url: &str, rg: Option<crate::types::song::ReplayGain>)` per §2.3.1. Body: `self.set_pending_replay_gain_internal(rg); self.set_source(url.to_string()).await;`.
- Rename or split: the current `pub fn set_pending_replay_gain` (line 1264) becomes `pub(crate) fn set_pending_replay_gain_internal` (or stay as `pub` if the gapless-prep path needs it externally — verify by grepping callers). The `set_pending_crossfade_replay_gain` (line 1270) stays public — it's used by `prepare_next_track` and `store_prepared_decoder` for the *next-track* slot, distinct from the primary-stream RG-stash.

`data/src/services/queue/mod.rs`:
- L231 (`pub fn toggle_shuffle(&mut self) -> Result<()>`) → `Result<ModeToggleEffect>`.
- L354 (`pub fn set_repeat(&mut self, mode: RepeatMode) -> Result<()>`) → `Result<ModeToggleEffect>`.
- L360 (`pub fn toggle_consume(&mut self) -> Result<bool>`) → `Result<(bool, ModeToggleEffect)>` (returns the new consume state alongside the effect).

(Lane C's implementer should verify exact return-type shapes against post-queue-typestate-igs HEAD — `mod.rs` line numbers may have shifted.)

`data/src/types/mode_toggle.rs` (or inline in `services/queue/mod.rs`) — new `ModeToggleEffect` per §2.3.2. Re-export `pub use mode_toggle::ModeToggleEffect;` if cross-module.

`data/src/backend/playback_controller.rs`:
- L227-229: `audio.set_pending_replay_gain(rg); audio.load_track(&stream_url).await;` → `audio.load_track_with_rg(&stream_url, rg).await;`.
- L284-286: same swap.
- L630-631: `engine.set_pending_replay_gain(song.replay_gain.clone()); engine.set_source(stream_url).await;` → `engine.load_track_with_rg(&stream_url, song.replay_gain.clone()).await;`.
- L688-689: same swap.
- L767-768: same swap.
- L407-417 (`toggle_random`): `let new_state = queue_manager.toggle_shuffle()?; drop(queue_manager); engine.reset_next_track().await;` → consume `ModeToggleEffect`. Adjust the order of operations to match: queue mutation → drop queue lock → `effect.apply_to(&self.audio_engine).await` → return `new_state`.
- L425-446 (`cycle_repeat`): same shape; `set_repeat` returns `ModeToggleEffect`.
- L456-466 (`toggle_consume`): same shape; `toggle_consume` returns `(bool, ModeToggleEffect)`.

`data/src/services/playback.rs`:
- The audit notes `services/playback.rs::play_song_direct` and `execute_transition` may also pair `set_pending_replay_gain` + `set_source`. Verify with grep and migrate to `load_track_with_rg` if found.

### Lane D — files & sites

`data/src/audio/engine.rs`:
- L88, L109, L159 — drop `next_decoder`, `next_track_prepared`, `next_source_shared`. Add `gapless: Arc<tokio::sync::Mutex<GaplessSlot>>`.
- L181, L188, L204 — constructor initializers consolidate to one `Arc::new(tokio::sync::Mutex::new(GaplessSlot::new()))`.
- L345 — `*self.next_track_prepared.lock().await = false;` → `self.gapless.lock().await.prepared = false;` (or `clear()` if the full reset is intended).
- L487-488, L491-492 — decode loop captures: `let next_decoder = self.next_decoder.clone(); let next_source_shared = self.next_source_shared.clone(); let next_track_prepared = self.next_track_prepared.clone();` → `let gapless = self.gapless.clone();`.
- L760-810 (gapless inline-swap in decode loop) — three sequential locks become one. Capture the slot's fields into locals before doing the renderer/decoder lock swap.
- L1087-1130 (`prepare_next_track`) — consolidate `*self.next_decoder.lock().await = ...; *self.next_source_shared.lock().await = ...; *self.next_track_prepared.lock().await = true;` (lines 1109, 1111, 1112) into one `let mut slot = self.gapless.lock().await; slot.decoder = Some(next_decoder); slot.source = url.to_string(); slot.prepared = true;`.
- L1139-1183 (`store_prepared_decoder`) — same consolidation.
- L1188-1207 (`consume_gapless_transition`) — `*self.next_source_shared.lock().await = String::new();` (line 1203) becomes part of a single slot reset; the gapless info struct stays separate (it's a different field).
- L1296-1313 (`start_crossfade` engine-side) — `let has_prepared = *self.next_track_prepared.lock().await;` then `let next_decoder_opt = self.next_decoder.lock().await.take();` consolidate into one slot lock that takes both.
- L1521-1548 (`load_prepared_track`) — same consolidation.
- L1626-1632 (`reset_next_track`) — `*self.next_decoder.lock().await = None; *self.next_track_prepared.lock().await = false; *self.next_source_shared.lock().await = String::new();` (lines 1627-1630) become `self.gapless.lock().await.clear();`.
- L1685-1687 (`is_next_track_prepared`) — `*self.next_track_prepared.lock().await` → `self.gapless.lock().await.is_prepared()`.

The `next_source` engine-local field (line 95) and `next_format` (line 92) stay — both are accessed only from the engine async path, never from the decode loop.

`gapless_transition_info` (line 157, separate Mutex<Option<GaplessTransitionInfo>>) stays separate. It's a distinct one-shot signal from decode loop → engine async; bundling it into `GaplessSlot` would muddy the lifecycle (the slot is reset on `reset_next_track`; the transition info is consumed once by `consume_gapless_transition`).

---

## 4. Verification (every lane)

After each commit slice on the lane:

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

All four must pass before pushing the slice. Per-lane TDD is light because the changes are structural (Lane A bumps generation in named methods; Lane B reshapes state without changing observable behavior; Lane C rewires call sites; Lane D collapses lock sites). Lane B and Lane D each warrant one new test:

- **Lane B**: assert `CrossfadePhase` cannot transition `Idle → OutgoingFinished` directly. The test constructs the engine, calls `on_decoder_finished` from Idle, and verifies the phase is still Idle (not OutgoingFinished). Today this is implicit; under the typestate the variant data simply isn't available so the path is unreachable.
- **Lane D**: regression test for the lock-order invariant: start a gapless prep, then call `cancel_crossfade` concurrently (via `tokio::join!`) and assert no deadlock + final state is consistent. (Gated by `#[tokio::test]`.)

Lane A and Lane C are pure refactors; existing test coverage suffices.

---

## 5. What each lane does NOT do

- **No UI crate edits.** The UI calls `app_service.audio_engine()`/`app_service.playback()` and the `PlaybackController`'s public methods stay shape-stable. (Lane C's `ModeToggleEffect` is consumed inside `PlaybackController`; the UI sees the same `toggle_random()` signature on `AppService`.)
- **No new dependencies.** All four lanes use what's already in the workspace.
- **No reformatting outside touched files.**
- **No drive-by docstring rewrites unrelated to the typestate.** Each lane updates the docstrings of methods it changes; no others.
- **No engine submodule extraction** (the §2 monoliths-data report's "extract crossfade controller submodule" recommendation is out of scope; that's a separate refactor).
- **No queue-manager IG changes.** `set_current_index` rename (IG-9) and `get_song_mut` guard (mentioned in backend-boundary.md §3) are separate audit items.
- **No TaskManager IG-12.** Different module, different concern; lives in its own future plan.
- **No B7 (visualizer.waves ↔ monstercat mutual exclusion).** Settings-layer bug, not audio.
- **Lane A** does NOT touch the gapless mutex layout (Lane D's job).
- **Lane B** does NOT touch gapless prep, decode generation, or RG stash. Crossfade-only.
- **Lane C** does NOT touch crossfade or atomics. Controller-only.
- **Lane D** does NOT touch decode_generation, source_generation, crossfade_phase, or RG stash. Gapless-only.
- **Lane B and Lane D each keep their decoder Arc separate** — `CrossfadePhase::Active.decoder` (Lane B) and `GaplessSlot.decoder` (Lane D) are distinct lifecycles, not bundled.

---

## Fanout Prompts

### lane-a-atomics

worktree: ~/nokkvi-audio-igs-a
branch: refactor/audio-igs-decode
effort: max
permission-mode: bypassPermissions

````
Task: implement Lane A of the audio-engine type-level invariants plan — typed wrappers for `decode_generation` (IG-6), `source_generation` (IG-7), seal `pub` atomics on `AudioRenderer` (IG-10), and align the live-metadata lock methods (B11).

Plan doc: /home/foogs/nokkvi/.agent/plans/audio-engine-typestate-igs.md (sections 2.1, 3 "Lane A").

Working directory: ~/nokkvi-audio-igs-a (this worktree). Branch: refactor/audio-igs-decode. The worktree is already created — do NOT run `git worktree add`.

## What to do

### 1. Verify baseline

- `git log -1 --oneline` shows `6fa04dd` or a descendant on `main`.
- `grep -n 'decode_generation\.' data/src/audio/engine.rs | wc -l` should return 7-8 lines (1 field declaration, 1 constructor init, 6 fetch_add sites, plus whatever clones).
- `grep -n 'source_generation\.' data/src/audio/engine.rs data/src/audio/renderer.rs` should enumerate ~6-8 sites.
- `grep -n 'pub engine\|pub source_generation\|pub decoder_eof' data/src/audio/renderer.rs` should return the 3 pub-field declarations at lines 58, 61, 63.
- `grep -n 'live_icy_metadata\.try_write\|live_codec_name\.write' data/src/audio/engine.rs:235-240` should show the asymmetric pair.

If any of these grep counts is off by >2, STOP and ask — line numbers may have drifted.

### 2. Add the typed atomic wrappers

Create `data/src/audio/generation.rs`:

```rust
use std::sync::{Arc, atomic::{AtomicU64, Ordering}};

/// Generation counter for the decode loop. Each spawned loop captures
/// `current()` at spawn time and exits when the value moves. `supersede()`
/// is the single mutator — every "stop the decode loop" path goes through it.
#[derive(Clone, Debug, Default)]
pub struct DecodeLoopHandle {
    counter: Arc<AtomicU64>,
}

impl DecodeLoopHandle {
    pub fn new() -> Self { Self::default() }

    /// Invalidate every spawned decode loop currently observing the previous
    /// generation. Returns the new generation.
    pub fn supersede(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// Lock-free generation read for the spawned decode loops.
    pub fn current(&self) -> u64 { self.counter.load(Ordering::Acquire) }
}

/// Source generation counter. Shared with the renderer so completion
/// callbacks can detect staleness without taking the engine lock.
#[derive(Clone, Debug, Default)]
pub struct SourceGeneration {
    counter: Arc<AtomicU64>,
}

impl SourceGeneration {
    pub fn new() -> Self { Self::default() }

    /// User-driven source change (manual skip, seek, set_source).
    pub fn bump_for_user_action(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::Release) + 1
    }

    /// Decode-loop gapless inline-swap — source URL changed.
    pub fn bump_for_gapless(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::Release) + 1
    }

    /// Crossfade-finalize path: intentional no-op so the existing
    /// "don't increment here" comment becomes a typed call.
    pub fn accept_internal_swap(&self) {}

    pub fn current(&self) -> u64 { self.counter.load(Ordering::Acquire) }
}
```

Declare in `data/src/audio/mod.rs`: `mod generation;` and re-export the types: `pub(crate) use generation::{DecodeLoopHandle, SourceGeneration};`.

### 3. Migrate `engine.rs`

**Field declarations** (lines 106, 132):
- `decode_generation: Arc<AtomicU64>` → `decode_loop: DecodeLoopHandle`.
- `source_generation: Arc<AtomicU64>` → `source_generation: SourceGeneration`.

**Constructor** (lines 187, 195): use `DecodeLoopHandle::new()` / `SourceGeneration::new()` (or `::default()`).

**Decode loop spawn** (lines 491-492, 501-502): clone the typed handles. The captured-into-task locals stay named `source_generation` / `decode_gen` for grep continuity:
- `let source_generation = self.source_generation.clone();`
- `let decode_gen = self.decode_loop.clone();`
- `let my_gen = self.decode_loop.supersede();`

**Sites that bump (in order)**:
- L251 (`set_source`): `self.source_generation.bump_for_user_action();`
- L794 (gapless inline-swap, inside the decode loop body): `source_generation.bump_for_gapless();`
- L915 (`stop`), L964 (`seek`), L1369 (`finalize_crossfade_engine`), L1532 (`load_prepared_track`), L1944 (`Drop`): all `self.decode_loop.supersede();`.
- L1415 — replace the comment-only "don't increment source_generation here" with: `self.source_generation.accept_internal_swap();` and update the comment to "intentional no-op (was: 'Don't increment source_generation here')".

**Source-generation accessor** (L1618):
```rust
pub fn source_generation(&self) -> u64 { self.source_generation.current() }
```

**`set_engine_reference`** (L1677-1682):
```rust
pub fn set_engine_reference(&mut self, engine: Weak<tokio::sync::Mutex<CustomAudioEngine>>) {
    let mut renderer = self.renderer.lock();
    renderer.set_engine_link(engine, self.source_generation.clone(), self.decoder_eof.clone());
}
```

**B11 — live-metadata lock alignment** (L235-240): change `self.live_codec_name.write()` to `self.live_codec_name.try_write()` so both setters are non-blocking and silently skip on contention. Add a one-line comment recording the intent (the same "stale data is acceptable here" rationale that has implicitly governed `live_icy_metadata.try_write()` since it landed). Verify the other `live_codec_name.write()` sites at lines 356, 1199 — those are NOT in `set_source`; leave them as `write()` (they run under the engine lock at decoder-init / gapless-transition time, where blocking is fine and contention is impossible).

### 4. Migrate `renderer.rs`

**Field declarations** (L58, L61, L63): drop the `pub` modifier from all three; change `source_generation: Arc<AtomicU64>` to `source_generation: SourceGeneration`.

**Constructor** (L210): `source_generation: SourceGeneration::new()`. (The default is fine — `engine.set_engine_reference` overwrites it via `set_engine_link` immediately after `AudioRenderer::new()` is called inside `CustomAudioEngine::new()`.)

**New method** on `AudioRenderer`:
```rust
/// Sealed setter — the only path to install the engine back-link, the
/// source-generation handle, and the EOF flag. Replaces the historical
/// pub-field assignment in `engine.set_engine_reference`.
pub fn set_engine_link(
    &mut self,
    engine: std::sync::Weak<tokio::sync::Mutex<super::engine::CustomAudioEngine>>,
    source_generation: SourceGeneration,
    decoder_eof: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    self.engine = engine;
    self.source_generation = source_generation;
    self.decoder_eof = decoder_eof;
}
```

**Stale-callback guard** (L985):
```rust
let generation = self.source_generation.current();
```

L994 (`if engine.source_generation() == src_gen`) is unchanged — the engine accessor still returns `u64`.

### 5. Verify

After every change in the order above:

```
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

The clippy pass will likely flag the new `Default` derive as `derivable_impls` if you also wrote a manual `new()` — pick one (prefer the manual `new()` for grep continuity with the other audio types). Don't `#[allow]`; resolve the lint.

### 6. Commit slices

Slice cadence (commit each verified slice without pausing — feature branch in a worktree, batch-commit policy applies):

1. `refactor(audio): introduce DecodeLoopHandle / SourceGeneration newtypes` — new `audio/generation.rs` + module declaration. May leave engine.rs / renderer.rs uncompiled for the moment; if so, slice 1 and 2 must land in one commit.
2. `refactor(audio): migrate engine.rs decode/source generation sites to typed handles` — all 8 fetch_add sites + accessor + set_engine_reference.
3. `refactor(audio): seal AudioRenderer pub atomics behind set_engine_link` — three pub fields → private + new setter + stale-callback guard read.
4. `fix(audio): align live_codec_name lock with live_icy_metadata in set_source` — B11 fix; one-line comment.

Each slice: all four checks pass. Skip the `Co-Authored-By` trailer per global instructions.

### 7. Update audit tracker

After the final commit, append commit refs to `.agent/audit-progress.md` §6 rows IG-6 / IG-7 / IG-10, and §5 row B11. Do NOT mark the audio-engine-igs §7 row as fully done — Lanes B/C/D close the rest.

## What NOT to touch

- Anything related to `crossfade_phase` / crossfade fields (Lane B's territory).
- `next_decoder` / `next_track_prepared` / `next_source_shared` (Lane D's territory).
- `set_pending_replay_gain` / `load_track` / mode toggles (Lane C's territory).
- The UI crate.
- `.agent/rules/` files. (The audit-progress tracker is the only doc you append to.)
- Other audit items.

## If blocked

- If a clippy `must_use` warning fires on `supersede()` / `bump_for_*()` returns: keep the `must_use` (it's actually correct — the returned generation is informational; mark the methods that the caller can ignore the return as `#[allow(clippy::let_underscore_must_use)]` ONLY if the caller intentionally drops it).
- If `cargo test` regresses on a renderer test that pokes the `pub` fields directly: investigate before changing the test. The fields are now private; the test must use the new setter or be reshaped.
- If renderer tests construct `AudioRenderer::new()` and then immediately read `source_generation`: that's fine — the default `SourceGeneration::new()` returns `current() == 0`.
- If you find a 4th `decode_generation.fetch_add` site I missed: stop, list it, ask before continuing. The plan says 6.

## Reporting

End with: commit refs + subjects, the `decode_generation` / `source_generation` site count delta (should drop from 6 + 2 raw `fetch_add` calls to 0), the new `audio/generation.rs` line count, and any test that needed adjustment (one sentence each).
````

### lane-b-crossfade

worktree: ~/nokkvi-audio-igs-b
branch: refactor/audio-igs-xfade
effort: max
permission-mode: bypassPermissions

````
Task: implement Lane B of the audio-engine type-level invariants plan — collapse the crossfade state machine on both sides of the engine/renderer boundary into enum-with-data variants (IG-8 + Drift #13). This is the heaviest lane.

Plan doc: /home/foogs/nokkvi/.agent/plans/audio-engine-typestate-igs.md (section 2.2 for the design, section 3 "Lane B" for the file inventory).

Working directory: ~/nokkvi-audio-igs-b (this worktree). Branch: refactor/audio-igs-xfade. The worktree is already created — do NOT run `git worktree add`.

## What to do

### 1. Verify baseline

- `git log -1 --oneline` shows `6fa04dd` or a descendant on `main`.
- `grep -n 'crossfade_phase' data/src/audio/engine.rs | wc -l` should return at least 11 lines (field, init, accessor, mutations at 1338/1352/1422/1838, reads at 1746-1748/1771/1825/1834/1845).
- `grep -n 'crossfade_active\|crossfade_armed\|crossfade_armed_' data/src/audio/renderer.rs | wc -l` should return ≥35 lines.
- `grep -n 'is_crossfade_active\|is_crossfade_armed\|crossfade_armed_duration_ms\|crossfade_armed_incoming_format' data/src/audio/engine.rs` should enumerate ~5-8 cross-boundary calls.

If counts are off, STOP and reconcile.

### 2. Engine side — `CrossfadePhase` enum-with-data

Replace the existing `pub enum CrossfadePhase { Idle, Active, OutgoingFinished }` (engine.rs:29-36) with:

```rust
/// Crossfade transition phase.
///
/// Variants carry the crossfade decoder + incoming source URL — these
/// fields used to live as parallel `Arc<Mutex<Option<AudioDecoder>>>` /
/// `String` on `CustomAudioEngine`, where every transition reset them
/// in lockstep with the phase flag. Now the data lives WITH the phase
/// so transitions are one `mem::replace` and impossible states are
/// unrepresentable (e.g. `Idle` carries no decoder; `Active` has it).
pub enum CrossfadePhase {
    Idle,
    Active {
        decoder: std::sync::Arc<tokio::sync::Mutex<Option<crate::audio::AudioDecoder>>>,
        incoming_source: String,
    },
    OutgoingFinished {
        decoder: std::sync::Arc<tokio::sync::Mutex<Option<crate::audio::AudioDecoder>>>,
        incoming_source: String,
    },
}

impl CrossfadePhase {
    pub fn is_idle(&self) -> bool {
        matches!(self, CrossfadePhase::Idle)
    }
    pub fn is_active_or_finished(&self) -> bool {
        matches!(self, CrossfadePhase::Active { .. } | CrossfadePhase::OutgoingFinished { .. })
    }
}
```

(Drop the `Copy`/`Eq` derives — variants contain non-Copy data. Keep `Debug` if it's currently derived; you may need to derive `Debug` manually since the inner `Mutex` may not be `Debug`.)

**Drop the engine struct fields** at engine.rs:151 (`crossfade_decoder: Arc<tokio::sync::Mutex<Option<AudioDecoder>>>`) and engine.rs:153 (`crossfade_incoming_source: String`). Also drop their constructor inits at engine.rs:201-202.

**Migrate the mutation sites**:

- L1290-1344 `start_crossfade`:
  ```rust
  if !self.crossfade_phase.is_idle() {
      debug!("🔀 [CROSSFADE] Already active, skipping");
      return false;
  }
  // ... existing prepared-decoder check + take ...
  let next_decoder = ...;          // existing logic
  let incoming_format = next_decoder.format().clone();
  let duration_ms = self.crossfade_duration_ms;
  let incoming_source = self.next_source.clone();
  self.next_source.clear();

  // Create the Arc<Mutex<Option<AudioDecoder>>> once here; the spawned
  // crossfade decode loop captures a clone of this Arc.
  let decoder_arc = std::sync::Arc::new(tokio::sync::Mutex::new(Some(next_decoder)));

  {
      let mut renderer = self.renderer.lock();
      if !renderer.is_crossfade_active() {
          renderer.start_crossfade(duration_ms, &incoming_format);
      }
  }

  self.crossfade_phase = CrossfadePhase::Active {
      decoder: decoder_arc.clone(),
      incoming_source,
  };

  // start_crossfade_decode_loop now takes the Arc rather than reading
  // self.crossfade_decoder.
  self.start_crossfade_decode_loop(decoder_arc);
  ```

- L1347-1360 `cancel_crossfade`:
  ```rust
  let phase = std::mem::replace(&mut self.crossfade_phase, CrossfadePhase::Idle);
  match phase {
      CrossfadePhase::Idle => return,
      CrossfadePhase::Active { decoder, .. }
      | CrossfadePhase::OutgoingFinished { decoder, .. } => {
          *decoder.lock().await = None; // signal the spawned loop to exit
      }
  }
  debug!("🔀 [CROSSFADE] Cancelling");
  let mut renderer = self.renderer.lock();
  renderer.cancel_crossfade();
  renderer.disarm_crossfade();
  ```

- L1365-1428 `finalize_crossfade_engine`:
  ```rust
  let phase = std::mem::replace(&mut self.crossfade_phase, CrossfadePhase::Idle);
  let (decoder_arc, incoming_source) = match phase {
      CrossfadePhase::Idle => return, // nothing to finalize
      CrossfadePhase::Active { decoder, incoming_source }
      | CrossfadePhase::OutgoingFinished { decoder, incoming_source } => (decoder, incoming_source),
  };
  // ... bump decode generation, take the decoder out of the Arc, swap into
  //     self.decoder, set self.source = incoming_source, etc.
  ```

- L1834-1845 `on_decoder_finished` Active → OutgoingFinished:
  ```rust
  let phase = std::mem::replace(&mut self.crossfade_phase, CrossfadePhase::Idle);
  if let CrossfadePhase::Active { decoder, incoming_source } = phase {
      self.crossfade_phase = CrossfadePhase::OutgoingFinished { decoder, incoming_source };
  } else {
      // Was Idle or already OutgoingFinished — restore.
      self.crossfade_phase = phase;
  }
  ```

**Migrate the read sites**:

- L1284 — replace the public `pub fn crossfade_phase(&self) -> CrossfadePhase` accessor (which used to return Copy) with two boolean accessors:
  ```rust
  pub fn crossfade_is_idle(&self) -> bool { self.crossfade_phase.is_idle() }
  pub fn crossfade_is_active_or_finished(&self) -> bool { self.crossfade_phase.is_active_or_finished() }
  ```
  Then grep for callers of `.crossfade_phase()` (across the workspace, including the UI crate) and update each one to the appropriate predicate. If the UI exposes the phase to a debug overlay, the audit allows a one-method addition `pub fn crossfade_phase_for_debug(&self) -> &'static str` returning `"idle"|"active"|"outgoing_finished"`.

- L1746-1748 `try_finalize_crossfade`:
  ```rust
  let should_finalize = match (&self.crossfade_phase, is_eof) {
      (CrossfadePhase::OutgoingFinished { .. }, _) => true,
      (CrossfadePhase::Active { .. }, true) => true,
      _ => false,
  };
  if should_finalize {
      debug!(
          "🔀 [RENDERER FINISHED] Outgoing queue drained during crossfade (eof={}) — finalizing",
          is_eof
      );
      self.finalize_crossfade_engine().await;
  }
  should_finalize
  ```

- L1771 `try_start_crossfade_transition`: replace `self.crossfade_phase != CrossfadePhase::Idle` with `!self.crossfade_phase.is_idle()`.

- L1825 (debug log): the format-debug used `{:?}` on Copy enum; under the new shape, derive `Debug` manually or call a small `phase_label()` helper.

- L1834 `on_decoder_finished` (`if self.crossfade_phase == CrossfadePhase::Active`): use the destructure-let pattern shown in the mutation section above.

### 3. `start_crossfade_decode_loop` signature change

The function used to read `self.crossfade_decoder.clone()`. Under the typestate, the decoder Arc lives inside the variant and was captured at `start_crossfade` time. Pass it as a parameter:

```rust
fn start_crossfade_decode_loop(
    &mut self,
    decoder_arc: std::sync::Arc<tokio::sync::Mutex<Option<crate::audio::AudioDecoder>>>,
) {
    let renderer = self.renderer.clone();
    let crossfade_duration_shared = self.crossfade_duration_shared.clone();
    let decoder = decoder_arc; // shadows the field-clone of the past

    tokio::spawn(async move {
        // ... existing body, unchanged — `decoder.lock().await` works the same ...
    });
}
```

### 4. Renderer side — `CrossfadeState` enum collapses 8 fields

Add at the top of `data/src/audio/renderer.rs` (after the `use` block but before `pub struct AudioRenderer`):

```rust
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
```

(Where `ActiveStream` is whatever type the existing `crossfade_stream: Option<ActiveStream>` field uses — verify the import.)

**Drop fields** at renderer.rs:66-76:
- `crossfade_active`, `crossfade_duration_ms`, `crossfade_start_time`, `crossfade_incoming_format`,
- `crossfade_armed`, `crossfade_armed_duration_ms`, `crossfade_armed_incoming_format`, `crossfade_armed_track_duration_ms`,
- `crossfade_stream` (moves into `CrossfadeState::Active.stream`).

**Keep**: `crossfade_finalized_elapsed_ms` — this is a transient post-finalize value that survives one tick after the state returns to Idle (engine reads it via `take_crossfade_elapsed_ms` after render_tick's finalize); it does NOT belong inside the enum.

**Add field**: `crossfade_state: CrossfadeState`.

**Constructor** (L210-220): replace 8 inits with one `crossfade_state: CrossfadeState::Idle`. Also drop the `crossfade_stream` init if it's currently `None` (which it is).

### 5. Renderer methods

Migrate every method body in renderer.rs that previously read or wrote one of the 8 fields:

- **`is_crossfade_active(&self) -> bool`** (L765): `matches!(self.crossfade_state, CrossfadeState::Active { .. })`.
- **`is_crossfade_armed(&self) -> bool`** (L867): `matches!(self.crossfade_state, CrossfadeState::Armed { .. })`.
- **`crossfade_armed_duration_ms(&self) -> u64`** (L872): destructure-or-zero.
- **`crossfade_armed_incoming_format(&self) -> &AudioFormat`** (L877): destructure-or-`&AudioFormat::INVALID` (declare a const sentinel — `AudioFormat::invalid()` constructs a value; if it's not `const fn`, lazily store a static `OnceLock<AudioFormat>` or change the signature to `Option<&AudioFormat>` if grep finds zero callers that depend on the borrowed-not-Option shape).
- **`arm_crossfade`** (L640): the early-return guards (already-active, zero-duration, etc.) stay; on success, `self.crossfade_state = CrossfadeState::Armed { duration_ms: effective, incoming_format: incoming_format.clone(), track_duration_ms };`.
- **`disarm_crossfade`** (L675): `if matches!(self.crossfade_state, CrossfadeState::Armed { .. }) { self.crossfade_state = CrossfadeState::Idle; }`.
- **`start_crossfade`** (L684): the early returns stay; on success, build the `cf_stream`, then `self.crossfade_state = CrossfadeState::Active { stream: cf_stream, started_at: Instant::now(), duration_ms, incoming_format: incoming_format.clone() };`. Drop the historical `self.crossfade_stream = Some(cf_stream);` and the 4 separate `self.crossfade_*` assignments.
- **`cancel_crossfade`** (L720):
  ```rust
  let was_active = matches!(self.crossfade_state, CrossfadeState::Active { .. });
  let prior = std::mem::replace(&mut self.crossfade_state, CrossfadeState::Idle);
  if let CrossfadeState::Active { stream, .. } = prior {
      stream.silence_and_stop();
  }
  if was_active {
      debug!("🔀 [RENDERER] Crossfade CANCELLED");
  }
  // Drop staged RG since the incoming stream is being thrown away.
  self.pending_crossfade_replay_gain = None;
  if !self.paused
      && let Some(ref stream) = self.primary_stream {
      stream.set_volume(self.stream_volume());
  }
  ```
- **`tick_crossfade(&mut self) -> bool`** (L779):
  ```rust
  let CrossfadeState::Active { started_at, duration_ms, .. } = &self.crossfade_state else {
      return false;
  };
  let elapsed_ms = started_at.elapsed().as_millis() as u64;
  let duration_ms = *duration_ms;
  // ... existing fade-curve math, primary/crossfade volume updates ...
  // (the crossfade_stream.set_volume call needs to access self.crossfade_state's stream; do it
  //  by re-borrowing the variant after the math, or by computing fade values first then matching once.)
  ```
  Restructure the body so the variant fields are borrowed only while needed and the volume sets run after the math.
- **`finalize_crossfade(&mut self) -> u64`** (L818):
  ```rust
  let CrossfadeState::Active { stream, started_at, duration_ms, incoming_format } =
      std::mem::replace(&mut self.crossfade_state, CrossfadeState::Idle)
  else {
      return 0;
  };
  let elapsed_ms = started_at.elapsed().as_millis() as u64;
  if let Some(old_primary) = self.primary_stream.take() {
      old_primary.silence_and_stop();
  }
  self.primary_stream = Some(stream);
  if let Some(ref stream) = self.primary_stream {
      stream.set_volume(self.stream_volume());
  }
  self.format = incoming_format;
  self.current_replay_gain = self.pending_crossfade_replay_gain.take();
  self.crossfade_finalized_elapsed_ms = elapsed_ms;
  // disarm_crossfade is no longer called explicitly — the mem::replace
  // above set state to Idle, which encompasses both Active→Idle and the
  // historical "armed-after-finalize race" case (couldn't actually happen,
  // but the symmetric reset is preserved by the assignment).
  let _ = duration_ms; // legacy log parameter — drop or keep in debug log
  debug!("🔀 [RENDERER] Crossfade FINALIZED: elapsed={}ms", elapsed_ms);
  elapsed_ms
  ```
- **`render_tick`** (L889): rewrite the `if self.crossfade_active && tick…` / `if self.crossfade_armed …` chain as a single match-or-if-let on `&self.crossfade_state`. Trigger sequence stays identical: Armed-trigger before Active-tick before track-completion check.
- **`write_crossfade_samples`** (L749): `if let CrossfadeState::Active { stream, .. } = &mut self.crossfade_state { stream.write_samples(samples) } else { 0 }`.
- **`crossfade_available_space`** (L757), **`crossfade_buffer_count`** (L770): same destructure-or-zero pattern.
- The diagnostics dump in `render_tick` (L893-907) reads `self.crossfade_active` / `self.crossfade_armed` — replace with the predicate methods.

### 6. New test for IG-8

In `data/src/audio/engine.rs` (or a new `engine/tests.rs`), add an `#[tokio::test]` that asserts `Idle → OutgoingFinished` direct transition is impossible:

```rust
#[tokio::test]
async fn crossfade_idle_cannot_transition_directly_to_outgoing_finished() {
    let mut engine = CustomAudioEngine::new();
    // Engine starts in Idle; calling on_decoder_finished should NOT move
    // the phase out of Idle (no Active state to transition from).
    let _ = engine.on_decoder_finished().await;
    assert!(engine.crossfade_is_idle(), "phase must remain Idle when no crossfade is active");
    assert!(!engine.crossfade_is_active_or_finished());
}
```

(Adjust signature if `on_decoder_finished` is `async` and returns nothing; adjust the engine accessor names to whatever you ended up using.)

### 7. Verify

After every slice:

```
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

Lane B is the heaviest — expect 4-6 commit slices (see step 8). Per-slice the engine + renderer must compile together; an interim where one file uses the new types and the other still uses the old fields will not compile.

### 8. Commit slices

1. `refactor(audio): introduce CrossfadeState enum on AudioRenderer` — drop 8 fields, add `crossfade_state`, migrate accessor methods (`is_crossfade_active`, `is_crossfade_armed`, `crossfade_armed_*`).
2. `refactor(audio): migrate renderer crossfade transitions to CrossfadeState match arms` — `arm`/`disarm`/`start`/`cancel`/`tick`/`finalize`/`write_crossfade_samples` bodies.
3. `refactor(audio): migrate render_tick to CrossfadeState dispatch` — the render_tick rewrite.
4. `refactor(audio): introduce CrossfadePhase enum-with-data on CustomAudioEngine` — drop the parallel decoder/source fields, add Active/OutgoingFinished variants.
5. `refactor(audio): migrate engine crossfade transitions and reads to CrossfadePhase variants` — start/cancel/finalize/on_decoder_finished + accessor replacement + start_crossfade_decode_loop signature.
6. `test(audio): assert Idle cannot transition directly to OutgoingFinished` — the new test from step 6.

Each slice: cargo + clippy + fmt all pass. Skip `Co-Authored-By`.

### 9. Update audit tracker

After the final commit, append commit refs to `.agent/audit-progress.md` §6 row IG-8 and §4 row 13. Mark IG-8 as ✅ done if Lane B is the only blocker; otherwise note partial.

## What NOT to touch

- Decode-generation / source-generation / B11 (Lane A's territory).
- Gapless mutex bundle (Lane D's territory).
- RG-stash + load-and-play tail / mode-toggle effects (Lane C's territory).
- The UI crate. (If `crossfade_phase()` accessor calls leak into `src/`, route them through the new boolean predicates in this lane — but only those exact call-site changes; no UI logic edits.)
- `.agent/rules/` files.

## If blocked

- If `crossfade_phase()` is called from the UI crate and the call sites don't map cleanly to the two new predicates: stop and ask. The plan assumed only debug-overlay-shaped reads.
- If `AudioFormat::invalid()` is not `const fn` and you can't return `&'static AudioFormat`: change `crossfade_armed_incoming_format(&self)` to return `Option<&AudioFormat>` and update the engine caller (likely just `start_crossfade` which already has the format from the prepared decoder).
- If an engine test depends on `crossfade_phase: Copy`: it's not Copy any more. Update the test to call the predicates instead.
- If clippy flags `pedantic::if_not_else` style issues in the rewritten render_tick: apply the suggested refactor; the path through is hot enough that legibility matters.
- If the spawned crossfade decode loop captures a stale Arc clone after Lane B's restructure: verify that `start_crossfade_decode_loop(decoder_arc)` receives the SAME `Arc` that's stored in `CrossfadePhase::Active.decoder`. The spawned loop and the variant must share the same Mutex — that's the signaling channel.

## Reporting

End with: commit refs + subjects, the renderer field count delta (8 → 1), the engine field count delta (3 → 1, accounting for the `Copy` enum to enum-with-data + drop of the 2 parallel fields), and any cross-boundary read site that needed a non-trivial reshape.
````

### lane-c-controller

worktree: ~/nokkvi-audio-igs-c
branch: refactor/audio-igs-controller
effort: max
permission-mode: bypassPermissions

````
Task: implement Lane C of the audio-engine type-level invariants plan — `engine.load_track_with_rg(url, rg)` atomic three-step (IG-11) + `ModeToggleEffect` `#[must_use]` token from QueueManager toggles (IG-3 engine-side).

Plan doc: /home/foogs/nokkvi/.agent/plans/audio-engine-typestate-igs.md (sections 2.3, 3 "Lane C").

Working directory: ~/nokkvi-audio-igs-c (this worktree). Branch: refactor/audio-igs-controller. The worktree is already created — do NOT run `git worktree add`.

## What to do

### 1. Verify baseline

- `git log -1 --oneline` shows `6fa04dd` or a descendant on `main`.
- `grep -nE 'set_pending_replay_gain.*\n.*(set_source|load_track)' data/src/backend/playback_controller.rs` should fail (multi-line); use:
  `grep -n 'set_pending_replay_gain\|set_source\|load_track' data/src/backend/playback_controller.rs` — should enumerate the 5 paired-RG sites at 227-229, 284-286, 630-631, 688-689, 767-768.
- `grep -n 'toggle_shuffle\|set_repeat\|toggle_consume' data/src/services/queue/mod.rs` — confirm the three method declarations exist post-queue-typestate-igs landing.
- `grep -n 'reset_next_track' data/src/backend/playback_controller.rs` — should return 3 hits at lines 416, 445, 465 inside the three mode-toggle methods.

If counts differ, STOP and reconcile (queue-typestate-igs may have shifted line numbers; rebase the plan's expectations against actual HEAD).

### 2. IG-11 — `engine.load_track_with_rg`

In `data/src/audio/engine.rs` add a new public method (location: near `load_track` at line 1079, or after the existing `set_pending_replay_gain` setter):

```rust
/// Atomic three-step: stash ReplayGain → set source. The caller still
/// invokes `play()` afterward, but the RG-stash + source-update pair
/// is uncuttable. Replaces the historical `set_pending_replay_gain` +
/// `load_track` / `set_source` pairing in PlaybackController.
pub async fn load_track_with_rg(
    &mut self,
    url: &str,
    rg: Option<crate::types::song::ReplayGain>,
) {
    self.renderer.lock().set_pending_replay_gain(rg);
    self.set_source(url.to_string()).await;
}
```

(Body inlines `set_pending_replay_gain`'s renderer call so we don't take the renderer lock through an extra method hop. Verify the inline matches the existing pub fn at engine.rs:1264.)

**No deletion** of the existing `set_pending_replay_gain` / `set_source` / `load_track` public methods — the gapless-prep path (`store_prepared_decoder`) still uses `set_pending_crossfade_replay_gain` for the *next-track* slot, distinct from this primary-stream RG-stash. The new method is purely additive.

### 3. Migrate `playback_controller.rs` RG-stash sites

Each of the 5 sites is shaped:
```rust
audio.set_pending_replay_gain(rg);
audio.load_track(&stream_url).await;     // OR audio.set_source(stream_url).await
```

Replace each with:
```rust
audio.load_track_with_rg(&stream_url, rg).await;
```

(Where `audio` is the engine guard variable; some sites use `engine` instead — match the local name.)

Sites:
- L227-229 (in `play()`): rg/load_track pair. New: `audio.load_track_with_rg(&stream_url, rg).await;`.
- L284-286 (in cold-start fallback inside `play()`): same shape.
- L630-631 (in `play_songs_from_index`): `engine.set_pending_replay_gain(...); engine.set_source(...).await;` → `engine.load_track_with_rg(&stream_url, song.replay_gain.clone()).await;`.
- L688-689 (in `play_song_from_queue`): same shape.
- L767-769 (in `apply_removal_aftermath`): same shape.

After the swap, grep `data/src/backend/playback_controller.rs` for `set_pending_replay_gain` — should return zero hits (or only inside comments). If a hit remains in actual code, STOP and reconcile — the plan said five.

Also grep `data/src/services/playback.rs` for the same pairing. The audit (DRY #17) said one inline `format!` + a navigator-internal `build_stream_url` may also pair RG-stash + load. Migrate any pair found.

### 4. IG-3 engine-side — `ModeToggleEffect`

Create `data/src/types/mode_toggle.rs`:

```rust
//! `ModeToggleEffect`: typed `#[must_use]` token that compels callers
//! of `QueueManager::toggle_shuffle` / `set_repeat` / `toggle_consume`
//! to dispatch `engine.reset_next_track()` after the queue mutation.
//!
//! Today the `PlaybackController` calls `reset_next_track()` after each
//! mode-toggle queue mutation by hand. The token makes that pairing a
//! compile-time obligation: a caller who drops the effect compiles only
//! with an explicit `let _ = …;` (the `#[must_use]` warning is escalated
//! to error by the workspace's `-D warnings`).

use std::sync::Weak;

use anyhow::Result;
use tokio::sync::Mutex;

#[must_use = "ModeToggleEffect must be applied via `effect.apply_to(&engine).await` to reset gapless prep — forgetting silently corrupts next-track state"]
pub struct ModeToggleEffect {
    _seal: (),
}

impl ModeToggleEffect {
    pub(crate) fn new() -> Self { Self { _seal: () } }

    /// Consume the effect by resetting the engine's prepared next-track
    /// state. The only path to actually perform the reset.
    pub async fn apply_to(self, engine: &Mutex<crate::audio::CustomAudioEngine>) {
        engine.lock().await.reset_next_track().await;
    }
}
```

Declare in `data/src/types/mod.rs`: `pub mod mode_toggle;` and re-export `pub use mode_toggle::ModeToggleEffect;`.

### 5. Migrate `services/queue/mod.rs` mode-toggle signatures

For each of `toggle_shuffle`, `set_repeat`, `toggle_consume`:

- Change return type: `Result<()>` → `Result<ModeToggleEffect>`. (For `toggle_consume` which today returns `Result<bool>`, change to `Result<(bool, ModeToggleEffect)>` so the consume-state-changed signal is preserved.)
- At the end of the method body (after the `commit_save_*()` call from the queue-typestate-igs Lane C), construct the effect:
  ```rust
  // existing tx.commit_save_order() returns Result<()>; bind it then return the effect.
  tx.commit_save_order()?;
  Ok(ModeToggleEffect::new())
  ```

(For `toggle_consume`, return `Ok((new_consume_state, ModeToggleEffect::new()))`.)

The exact line numbers post-queue-typestate-igs may differ — find the methods by name, not by number.

**Caller fan-out**: grep `toggle_shuffle\|set_repeat\|toggle_consume` across the workspace for callers OUTSIDE `playback_controller.rs`:
- Hit 1: `data/src/backend/queue.rs` may delegate through. Update if so.
- Hit 2: any test in `services/queue/mod.rs` tests. Tests can call `let _ = qm.toggle_shuffle().unwrap();` — the explicit `let _` opts out of the `must_use` (acceptable in tests).
- Hit 3: any UI handler? Should be none — the UI dispatches via `app_service.toggle_random()` which hits `PlaybackController::toggle_random()`. Verify with grep.

If the audit is wrong and the UI has a direct `queue_service.queue_manager().lock().await.toggle_shuffle()` call: STOP and reconcile. The plan assumed PlaybackController is the only consumer.

### 6. Migrate `playback_controller.rs` mode-toggle methods

`toggle_random` (L407):
```rust
pub async fn toggle_random(&self) -> Result<bool> {
    let queue_manager_arc = self.queue_service.queue_manager();
    let mut queue_manager = queue_manager_arc.lock().await;
    let effect = queue_manager.toggle_shuffle()?;
    let new_state = queue_manager.get_queue().shuffle;  // re-read post-mutation
    drop(queue_manager);
    effect.apply_to(&self.audio_engine).await;
    Ok(new_state)
}
```

(Adjust `new_state` extraction to match the existing method's shape — likely it captures the bool BEFORE toggling. Check the existing body.)

`cycle_repeat` (L425):
```rust
pub async fn cycle_repeat(&self) -> Result<(bool, bool)> {
    let queue_manager_arc = self.queue_service.queue_manager();
    let mut queue_manager = queue_manager_arc.lock().await;
    let effect = queue_manager.set_repeat(next_mode)?;
    // ... existing logic that derives the (bool, bool) tuple ...
    drop(queue_manager);
    effect.apply_to(&self.audio_engine).await;
    Ok((tracking, queue_one))
}
```

(Match the actual return tuple shape.)

`toggle_consume` (L456):
```rust
pub async fn toggle_consume(&self) -> Result<bool> {
    let queue_manager_arc = self.queue_service.queue_manager();
    let mut queue_manager = queue_manager_arc.lock().await;
    let (new_state, effect) = queue_manager.toggle_consume()?;
    drop(queue_manager);
    effect.apply_to(&self.audio_engine).await;
    Ok(new_state)
}
```

After all three migrations, grep `data/src/backend/playback_controller.rs` for `engine.reset_next_track()` — should return zero hits (it's now called only from the effect's `apply_to`). If a hit remains in any of the three methods, STOP — the migration missed a path.

### 7. Verify

```
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

The clippy pass enforces `-D warnings`, which means the `#[must_use]` warning escalates to an error. If you've left a single `let _ = qm.toggle_shuffle()?;` somewhere, the build fails with a clear pointer.

### 8. Commit slices

1. `feat(audio): add engine.load_track_with_rg atomic three-step` — engine.rs new method.
2. `refactor(playback): migrate playback_controller RG-stash sites to load_track_with_rg` — 5 sites in `playback_controller.rs` + any in `services/playback.rs`.
3. `feat(queue): introduce ModeToggleEffect must_use token for mode toggles` — new `types/mode_toggle.rs` + module export.
4. `refactor(queue): return ModeToggleEffect from toggle_shuffle / set_repeat / toggle_consume` — three signature changes + body return adjustments + test let-underscore opts.
5. `refactor(playback): consume ModeToggleEffect in PlaybackController mode toggles` — three method bodies replace `engine.reset_next_track()` with `effect.apply_to(&self.audio_engine).await`.

Each slice: cargo + clippy + fmt all pass. Skip `Co-Authored-By`.

### 9. Update audit tracker

After the final commit, append commit refs to `.agent/audit-progress.md` §6 rows IG-3 (engine-side ✅), IG-11 (✅). Mark `IG-3` as ✅ done iff this lane closes both queue-side AND engine-side (queue-side closed in `4e2c960`; this lane closes engine-side, so flip the row from 🟡 → ✅).

## What NOT to touch

- Anything related to `decode_generation` / `source_generation` / atomic-sharing surface (Lane A's territory).
- `crossfade_phase` / crossfade fields (Lane B's territory).
- `next_decoder` / `next_track_prepared` / `next_source_shared` / gapless mutex layout (Lane D's territory).
- `set_pending_crossfade_replay_gain` (it stays public — distinct from primary-stream RG-stash).
- The UI crate. (PlaybackController's public methods stay shape-stable; AppService delegations unchanged.)
- `.agent/rules/` files.
- Any other audit item.

## If blocked

- If `set_pending_replay_gain` has an external caller outside `playback_controller.rs` and `services/playback.rs`: STOP. The plan assumed only the controller consumes it.
- If a test in `services/queue/mod.rs` calls `qm.toggle_shuffle()?` and chains assertions on the return, expecting `Result<()>`: convert to `let effect = qm.toggle_shuffle()?; effect_dropped_safely_in_test(effect);` where the test imports a local helper that consumes the effect with a no-op (since tests don't have an engine).
- If clippy fires `must_use` errors that you can't escape from a test: introduce a helper fn `pub(crate) fn drop_effect(_: ModeToggleEffect) {}` in `mode_toggle.rs` for test-only use. Mark it `#[cfg(test)]` if possible.
- If `cycle_repeat`'s tuple-return shape doesn't fit `set_repeat -> Result<ModeToggleEffect>`: keep cycle_repeat's PlaybackController-side derivation logic, just route the effect through after.

## Reporting

End with: commit refs + subjects, the `set_pending_replay_gain` site count delta in `playback_controller.rs` (should drop from 5 to 0), the `engine.reset_next_track()` site count delta in `playback_controller.rs` (3 → 0), and the new `mode_toggle.rs` line count.
````

### lane-d-gapless

worktree: ~/nokkvi-audio-igs-d
branch: refactor/audio-igs-gapless
effort: max
permission-mode: bypassPermissions

````
Task: implement Lane D of the audio-engine type-level invariants plan — bundle the three gapless-prep tokio mutexes (`next_decoder`, `next_track_prepared`, `next_source_shared`) into one `Arc<tokio::sync::Mutex<GaplessSlot>>` (IG-13).

Plan doc: /home/foogs/nokkvi/.agent/plans/audio-engine-typestate-igs.md (sections 2.4, 3 "Lane D").

Working directory: ~/nokkvi-audio-igs-d (this worktree). Branch: refactor/audio-igs-gapless. The worktree is already created — do NOT run `git worktree add`.

## What to do

### 1. Verify baseline

- `git log -1 --oneline` shows `6fa04dd` or a descendant on `main`.
- `grep -n 'next_decoder\|next_track_prepared\|next_source_shared' data/src/audio/engine.rs | wc -l` should return ≥30 (3 field declarations, 3 constructor inits, ~25 lock sites scattered across decode loop / prepare_next_track / store_prepared_decoder / consume_gapless_transition / start_crossfade / load_prepared_track / reset_next_track / is_next_track_prepared).
- `grep -rn 'next_decoder\|next_track_prepared\|next_source_shared' data/src/ --include='*.rs' | grep -v 'engine.rs'` should return zero hits — these mutexes are engine-internal.

If hits exist outside `engine.rs`, STOP and reconcile.

### 2. Define `GaplessSlot`

Either inline at the top of `data/src/audio/engine.rs` (after `PlaybackState` / `CrossfadePhase`) OR in a new file `data/src/audio/gapless.rs` declared `mod gapless;` + `pub(crate) use gapless::GaplessSlot;`. Prefer the new file if `engine.rs` would grow past ~2000 lines after the refactor; inline otherwise. Implementer's call.

```rust
/// Bundled gapless-prep state for the next track. Replaces the three
/// independent tokio mutexes (`next_decoder`, `next_track_prepared`,
/// `next_source_shared`) that the audit (`backend-boundary.md` §4 IG-7
/// → renamed IG-13 here) flagged as enforced only by reading every site.
///
/// Lock order: this struct lives behind one `Arc<tokio::sync::Mutex<…>>`,
/// so all three fields are acquired together. The decode loop, the
/// engine async path, and `cancel_crossfade` all take the same mutex
/// in the same order — the order question disappears.
pub(crate) struct GaplessSlot {
    /// Decoder for the prepared next track. `None` when nothing is staged.
    pub decoder: Option<crate::audio::AudioDecoder>,
    /// Source URL of the prepared track. Empty when not staged.
    pub source: String,
    /// True when the slot is fully prepared and the renderer can use it
    /// for gapless transition. Distinct from `decoder.is_some()` because
    /// the decode loop sets `prepared = false` AFTER `take`-ing the
    /// decoder (so the next loop iteration knows the slot is mid-swap).
    pub prepared: bool,
}

impl GaplessSlot {
    pub fn new() -> Self {
        Self { decoder: None, source: String::new(), prepared: false }
    }

    pub fn is_prepared(&self) -> bool {
        self.prepared && self.decoder.is_some()
    }

    pub fn clear(&mut self) {
        self.decoder = None;
        self.source.clear();
        self.prepared = false;
    }
}
```

### 3. Replace the three engine fields

In `CustomAudioEngine` (engine.rs:78-166):

- L88 — drop `next_decoder: Arc<tokio::sync::Mutex<Option<AudioDecoder>>>`.
- L109 — drop `next_track_prepared: Arc<tokio::sync::Mutex<bool>>`.
- L159 — drop `next_source_shared: Arc<tokio::sync::Mutex<String>>`.

Add: `gapless: Arc<tokio::sync::Mutex<GaplessSlot>>`.

**Keep**: `next_source: String` (engine-local field, line 95) and `next_format: AudioFormat` (line 92). These are accessed only from the engine async path (not from the decode loop), so they don't need to be inside the bundled mutex.

Constructor (engine.rs:181, 188, 204): replace 3 `Arc::new(tokio::sync::Mutex::new(...))` inits with one:
```rust
gapless: Arc::new(tokio::sync::Mutex::new(GaplessSlot::new())),
```

### 4. Migrate every lock site

Pattern: a site that previously did:
```rust
let prepared = *self.next_track_prepared.lock().await;
let decoder_opt = self.next_decoder.lock().await.take();
let source = self.next_source_shared.lock().await.clone();
```
becomes:
```rust
let mut slot = self.gapless.lock().await;
let prepared = slot.is_prepared();
let decoder_opt = slot.decoder.take();
let source = slot.source.clone();
// drop slot at end of scope, OR explicitly `drop(slot);` if a renderer/decoder lock follows
```

Each migration site:

**`stop` (L345)** — `*self.next_track_prepared.lock().await = false;`:
```rust
self.gapless.lock().await.prepared = false;
```
(The decoder field stays; only the prepared flag flips. This matches the existing semantics — `stop` doesn't drop the prepared decoder, it just ungates it.)

**`start_decoding_loop` (L487-492)** — captures three Arc clones:
```rust
let next_decoder = self.next_decoder.clone();
let next_track_prepared = self.next_track_prepared.clone();
let next_source_shared = self.next_source_shared.clone();
```
becomes:
```rust
let gapless = self.gapless.clone();
```

The captured `gapless` is used inside the spawned decode loop body.

**Decode loop gapless inline-swap (L760-810)** — the three sequential locks (`next_track_prepared.lock().await`, `next_decoder.lock().await`, `next_source_shared.lock().await`) consolidate into ONE lock:
```rust
let did_gapless = {
    let mut slot = gapless.lock().await;
    if slot.is_prepared() {
        // Hold the slot lock through the format check + take so the
        // `prepared` flag and the decoder ownership transition atomically.
        if let Some(next_dec) = slot.decoder.take() {
            let next_fmt = next_dec.format().clone();
            // ... format match, RG check ...
            if formats_match && rg_allows_swap {
                let next_duration = next_dec.duration();
                let next_source_url = std::mem::take(&mut slot.source);
                let next_codec = next_dec.live_codec();
                slot.prepared = false;
                drop(slot);  // release before locking decoder + renderer

                // Rest of the gapless swap (Lane A's source_generation.bump_for_gapless()
                // call lives here — keep it after the slot lock drops).
                *decoder.lock().await = next_dec;
                source_generation.fetch_add(1, Ordering::Release);  // Lane A renames this
                // ... existing renderer.lock() block, gapless_info update, callback ...
                true
            } else {
                // Format mismatch — put the decoder back to keep the slot prepared.
                slot.decoder = Some(next_dec);
                drop(slot);
                false
            }
        } else {
            // Slot said prepared=true but decoder was None — race; clear.
            slot.prepared = false;
            drop(slot);
            false
        }
    } else {
        drop(slot);
        false
    }
};
```

**`prepare_next_track` (L1087-1130)** — the three writes on lines 1109, 1111, 1112:
```rust
let mut slot = self.gapless.lock().await;
slot.decoder = Some(next_decoder);
slot.source = url.to_string();
slot.prepared = true;
drop(slot);
```

(The renderer-lock blocks for `set_pending_crossfade_replay_gain` and `arm_crossfade` come AFTER — see existing code structure; just replace the 3 mutex writes with one slot write.)

`self.next_source = url.to_string();` (line 1110, the engine-local field) STAYS — it's a separate field, not inside the slot.

**`store_prepared_decoder` (L1139-1183)** — same shape:
```rust
let mut slot = self.gapless.lock().await;
slot.decoder = Some(decoder);
slot.source = url.clone();
slot.prepared = true;
drop(slot);
self.next_source = url;
self.next_format = ...;
// ... existing renderer block ...
```

The reset-on-mismatch case (line 1151) — `if self.next_source != url { self.reset_next_track().await; }` — runs BEFORE acquiring the slot lock. `reset_next_track` itself migrates separately (see below) and takes the slot lock internally; no nesting issue.

**`consume_gapless_transition` (L1188-1207)** — the `*self.next_source_shared.lock().await = String::new();` (line 1203):
```rust
self.gapless.lock().await.source.clear();
```

The other field updates (line 1195 sets `self.source`, etc.) are engine-local and stay outside the slot.

**`start_crossfade` engine-side (L1297-1313)**:
```rust
let next_decoder = {
    let mut slot = self.gapless.lock().await;
    if !slot.is_prepared() {
        drop(slot);
        debug!("🔀 [CROSSFADE] No prepared decoder, cannot start");
        return false;
    }
    let dec = slot.decoder.take();
    slot.prepared = false;
    dec
};
let next_decoder = match next_decoder {
    Some(d) => d,
    None => {
        debug!("🔀 [CROSSFADE] Prepared flag set but no decoder, skipping");
        return false;
    }
};
// ... existing body ...
```

**`load_prepared_track` (L1521-1548)**:
```rust
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
// ... rest of body, line 1531 onward ...
```

The `*self.next_track_prepared.lock().await = false;` at L1548 is now redundant (the slot already cleared `prepared`); remove it.

**`reset_next_track` (L1626-1632)**:
```rust
self.gapless.lock().await.clear();
self.next_format = AudioFormat::invalid();
self.renderer.lock().disarm_crossfade();
```

(The renderer disarm at line 1632 stays; it's an orthogonal call.)

**`is_next_track_prepared` (L1685-1687)**:
```rust
pub async fn is_next_track_prepared(&self) -> bool {
    self.gapless.lock().await.is_prepared()
}
```

### 5. New regression test for IG-13

Add to `data/src/audio/engine.rs` (or `engine/tests.rs`):

```rust
#[tokio::test]
async fn gapless_slot_prepare_then_cancel_does_not_deadlock() {
    use std::sync::Arc;
    let engine = Arc::new(tokio::sync::Mutex::new(CustomAudioEngine::new()));

    // Prep a "next track" via store_prepared_decoder — but use a stub URL
    // that initialization will reject (or skip the actual decoder init via
    // a test-only helper). The intent is to exercise the lock path, not
    // the decoder I/O.
    //
    // If init is unavoidable, gate this test behind a feature flag or
    // mock the AudioDecoder via a trait. (Don't add a test-only feature
    // flag for this if it exists in the codebase; otherwise document that
    // the test asserts only that no deadlock occurs.)
    let prep = {
        let engine = engine.clone();
        tokio::spawn(async move {
            // Call reset_next_track which acquires gapless lock + renderer lock.
            engine.lock().await.reset_next_track().await;
        })
    };
    let cancel = {
        let engine = engine.clone();
        tokio::spawn(async move {
            engine.lock().await.cancel_crossfade().await;
        })
    };

    // If the lock order between gapless/renderer/decoder were inconsistent
    // before this lane, joining these would deadlock. Under one bundled
    // mutex, both methods serialize on the engine lock first, then on
    // their internal locks in a fixed order.
    let _ = tokio::join!(prep, cancel);
}
```

(Adapt to whatever the existing test harness in `engine.rs` looks like. If the test setup is too involved for this lane's scope, drop the test and note in the lane summary that the regression test is deferred — the structural change itself is the safety improvement.)

### 6. Verify

After every slice:

```
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

### 7. Commit slices

1. `refactor(audio): introduce GaplessSlot bundling next-track prep state` — new struct + module declaration. May leave engine.rs uncompiled briefly; if so, slice 1 and 2 must land in one commit.
2. `refactor(audio): migrate gapless lock sites in engine.rs to GaplessSlot mutex` — replaces all 25-30 lock sites.
3. (optional) `test(audio): regression test for gapless lock-order invariant` — only if the test harness allows it without a deeper mock. Otherwise skip.

Each slice: cargo + clippy + fmt all pass. Skip `Co-Authored-By`.

### 8. Update audit tracker

After the final commit, append commit refs to `.agent/audit-progress.md` §6 row IG-13. Mark IG-13 ✅ done.

## What NOT to touch

- `decode_generation` / `source_generation` / atomic-sharing surface (Lane A's territory).
- `crossfade_phase` / `crossfade_decoder` (Lane B's territory). The crossfade decoder Arc is a SEPARATE Mutex from `gapless` — the two state machines have different lifecycles. Do NOT merge them.
- `set_pending_replay_gain` / `load_track` / mode toggles (Lane C's territory).
- The UI crate.
- `.agent/rules/` files.
- `gapless_transition_info` (engine.rs:157) — distinct one-shot signal from decode loop → engine async, NOT bundled into `GaplessSlot`. Leave as-is.
- `next_source` / `next_format` engine-local fields — NOT inside the slot (read only on engine async path).

## If blocked

- If a lock site holds the gapless lock across an `await` that previously dropped the individual lock first: the new bundled lock has wider scope, which may serialize more work than before. Verify the renderer lock and decoder lock (which have their own mutex types) are NOT taken while holding the slot lock unless that nesting was already implicit. If a deadlock potential emerges, restructure the site to drop(slot) before the next lock.
- If `prepare_next_track` and `store_prepared_decoder` both call `set_pending_crossfade_replay_gain` (on the renderer) AFTER setting the slot fields: that's fine — the slot lock drops before the renderer lock acquires. Same as today's pattern.
- If a test in `engine.rs` directly pokes `next_track_prepared.lock().await`: update it to `engine.is_next_track_prepared().await` or the equivalent slot read.
- If you find a gapless-related lock site I missed (>3 misses): STOP and list them — the plan said ~25-30; an outlier may indicate a hidden caller in another file (the grep at step 1 should have caught it).

## Reporting

End with: commit refs + subjects, the gapless mutex site count delta in `engine.rs` (~25-30 → ~12, with the 3 fields collapsed to 1), and the LOC delta of the engine struct (3 fields removed, 1 added).
````
