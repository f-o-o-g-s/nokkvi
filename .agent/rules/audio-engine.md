---
trigger: glob
globs: data/src/audio/**
---

# Audio Engine

## Architecture

```
CustomAudioEngine
├── AudioDecoder (Symphonia)
│   ├── Standard: HTTP via RangeHttpReader (256KB chunks, MAX_CACHED_CHUNKS=16, prefetch on seek + background sliding-window read-ahead)
│   └── Radio:    HTTP via AsyncNetworkBuffer (tokio async → bounded mpsc → sync Read) with generation-gated auto-reconnect. The EOF reconnect loop is the free async `radio_reconnect_loop()` → `RadioReconnectOutcome::{Reconnected, Superseded, GaveUp}` control token (the caller owns the `continue 'decode_loop` / `break` flow); the jitter prebuffer is the free `radio_jitter_prebuffer_step()`
├── AudioRenderer (ring buffers) → visualizer callback from StreamingSource
│   └── RodioOutput (shared rodio Mixer) → ActiveStream per track
│       └── StreamingSource (rodio::Source) → EqProcessor → lock-free ring buffer → pipewire callback
├── CrossfadeCoordinator (engine, `engine.rs`) — owns the engine-side crossfade cluster: the `CrossfadePhase` + the `enabled` / `duration_ms` config + the bit-perfect mirror. `crossfade_eligible()` (shared coarse arm gate) and `is_crossfade_live(&renderer)` (engine-not-Idle OR renderer-Active reconciliation for `reset_next_track`) live on it. Does NOT own the renderer `CrossfadeState` nor the `crossfade_duration_shared` atomic (a decode-loop channel)
│   └── CrossfadePhase enum: Idle → Active { decoder, incoming_source } → OutgoingFinished { decoder, incoming_source }
├── CrossfadeState enum (renderer): Idle / Armed / Active — per-phase data lives inside the variant; `mem::replace` swaps phases atomically
├── DecodeLoopChannels (engine, `engine.rs`) — bundles the lock-free atomics the primary decode loop reads/writes (`source_generation`, `decoder_eof`, `stream_is_infinite`, `crossfade_duration_shared`, `live_bitrate`); `clone_for_decode_loop()` hands the spawned task exactly those Arcs by identity-preserving clone (an `Arc::ptr_eq` test guards the same-Arc invariant). `live_sample_rate` is intentionally excluded — engine-only, never cloned into the loop
├── GaplessSlot — bundles `decoder` + `source` + `prepared` under one `tokio::Mutex` so the decode loop, async path, and `cancel_crossfade` always lock together (audit IG-13). The inline EOF swap is extracted into the free async `try_gapless_swap()` → `GaplessSwapOutcome::{Swapped, NotPrepared, FormatMismatch, CrossfadeActive, DecoderMissing}`; `bump_for_gapless` is its SOLE decode-loop bump
├── DecodeLoopHandle / SourceGeneration (`generation.rs`) — typed atomic-counter wrappers; `bump_for_user_action` / `bump_for_gapless` / `accept_internal_swap` make every "stop the loop" / "invalidate callback" call site self-documenting
├── LiveStringSlot (`engine.rs`) — `Arc<RwLock<Option<String>>>` newtype for the engine's hot-path live strings (`live_icy_metadata`, `live_codec_name`). Writes go through `reset()` (non-blocking `try_write`) / `set()` (blocking) — the type encodes the reset-side discipline (B11 hot-path) so it can't drift into a blocking `write()`
└── EqState (eq.rs) — shared atomic gains passed to each StreamingSource
```

## Audio Output (native PipeWire)

One native PipeWire stream via a shared `rodio::Mixer`:
- `SfxEngine` owns the app-wide `ActiveSink` (`NativePipewire` or `Cpal` fallback)
- `NativePipeWireSink` runs a dedicated `pw_nokkvi_out` thread with its own PipeWire mainloop
- Each track gets an `ActiveStream`: `RING_BUFFER_CAPACITY = 5_000_000` samples (~52 s at 48 kHz stereo, sized for radio jitter) + a `StreamingSource` added to the mixer
- `StreamingSource` implements `rodio::Source` (pull model) — pipewire callback pulls f32 samples

## Critical Rules

- **Codec registry**: every Symphonia decoder/lookup MUST go through `audio::symphonia_registry::codecs()`, never `symphonia::default::get_codecs()`. The shared registry adds `symphonia-adapter-libopus` on top of the Symphonia defaults; the upstream default registry has no Opus decoder (pdeljanov/Symphonia#8 open since 2020), so any direct call to `get_codecs()` re-breaks `.opus` playback (see GH#3). The `audio::symphonia_registry::probe_and_make_decoder(mss, hint, enable_gapless)` helper owns the shared probe+decoder build; `enable_gapless` is load-bearing — `true` only for the primary init in `AudioDecoder::open_input`, `false` for the `ResetRequired` reprobe and `SfxEngine::decode_wav_stream`.
- **Track changes**: create fresh decoders **before** locking the engine; release the engine lock during decoder operations. Use `engine.load_track_with_rg(url, rg, expected_duration_ms)` — the atomic pair that stashes ReplayGain on the renderer and then calls `set_source(url, expected_duration_ms)`, replacing the historical `set_pending_replay_gain` + `load_track` / `set_source` pairing in `PlaybackController`. `expected_duration_ms` carries the server's track length into the decoder for the probed-duration cross-check (see "Probed Duration").
- **`SourceGeneration`**: typed atomic counter; `bump_for_user_action()` on every user-driven source change. The renderer snapshots `current()` before releasing the engine lock and discards stale completion callbacks.
- **Next-track reset**: `reset_next_track()` clears the prepared decoder and disarms crossfade. Every queue mutator (mode toggles, move/insert/remove/sort, set_queue, add_songs, reposition_to_index) returns `NextTrackResetEffect` — a `#[must_use]` token that the caller dispatches via `apply_to(&engine)` (engine mutex) or `apply_locked(&mut engine)` (engine lock already held). The token makes the reset a compile-time obligation, so a new reorder path can't reintroduce the shuffle + crossfade UI-vs-engine desync.
- **Track-completion path**: the playback navigator releases its lock across engine I/O — do not re-introduce a held lock around `PeekedQueue::transition()` / `set_source()`.
- Decoupled render thread: 20 ms intervals (50 Hz), handles crossfade tick + completion detection.

## Network Resilience (issue #9)

Layered subsystems keep finite HTTP streams playing on shallow networks (shipped v0.6.9; hi-res deadlock fixed v0.7.1):

- **Background read-ahead** (`range_http_reader.rs`): `prefetch_loop` keeps a sliding window of `PREFETCH_WINDOW_CHUNKS = 5` chunks ahead of the read cursor, topping up when the lead drops to `PREFETCH_LOW_WATERMARK_CHUNKS` (= 2). Spawned lazily by `maybe_spawn_prefetch()` on the first forward-sequential read (current chunk == previous + 1), so the FLAC init-probe's binary-search seeks never spawn a task chasing a bouncing cursor. A const assert (`1 + PREFETCH_WINDOW_CHUNKS + EXPECTED_BEHIND_CHUNKS <= MAX_CACHED_CHUNKS`) guarantees the LRU can never evict the live window — keep it satisfied when resizing any of the three. The synchronous seek-time prefetch is a separate, unchanged path.
- **Decode cushion** (`engine.rs`): the decode loop buffers `CUSHION_MS = 1100` ms ahead (TIME-based, scaled by frame rate, grown for long crossfades); backpressure releases at `CUSHION_MS / BACKPRESSURE_RELEASE_DIVISOR` (= 3).
- **Pause-and-rebuffer** (`renderer.rs`): pure state machine `rebuffer_action()` → `RebufferAction::{Enter, Hold, Exit, None}` driven from `render_tick`. Watermarks are TIME-based — enter below `REBUFFER_ENTER_MS = 200`, resume at `REBUFFER_RESUME_MS = 800` — converted to samples per-format via `frame_rate`. Fixed-SAMPLE budgets were the v0.6.9 hi-res deadlock: at 96k/192k they shrank in time until rebuffer entry sat above the decode backpressure-release point, freezing the ring. Compile-time interlocks encode the ordering (`REBUFFER_ENTER_MS * BACKPRESSURE_RELEASE_DIVISOR < CUSHION_MS`, `REBUFFER_RESUME_MS < CUSHION_MS`, `REBUFFER_ENTER_MS < REBUFFER_RESUME_MS`) — keep all three satisfied when changing any of these constants. Finite (seekable) streams only: never radio, never mid-crossfade. `MAX_REBUFFER_TICKS = 500` is the dead-socket safety valve.
- **Latch discipline**: `reset_rebuffer_latch()` zeroes `rebuffering` / `rebuffer_primed` / `rebuffer_ticks` on every fresh-ring transition — start, stop, seek, and `finalize_crossfade` (carrying a stale `rebuffer_primed` across a sample-rate change is the one path that re-creates the deadlock; test `finalize_crossfade_resets_rebuffer_latch`). `pause()` deliberately clears only `rebuffering`. The latch primes once the ring first reaches the resume target, so a cold track start cannot false-pause at 0:00.

## Volume

Dual-path: PipeWire native (preferred) or software fallback.

- **PipeWire-native** (`pw_volume_active = true`): software volume at 1.0; SfxEngine sends cubic (`v³`) to PipeWire over IPC.
- **Software fallback**: exponential amplitude curve, per-sample exponential smoothing (~5 ms) for anti-aliasing.
- Crossfade: equal-power cos²/sin² curves. When `pw_volume_active`, fade uses only the coefficient and PipeWire applies user volume on top.

## Volume Normalization & ReplayGain

`VolumeNormalizationMode`: `Off`, `Agc`, `ReplayGainTrack`, `ReplayGainAlbum`. Settings live in the Settings view's Playback tab, "Volume Normalization" section (`src/views/settings/items_playback.rs`): mode dropdown, AGC target level (AGC only), and — in RG modes — preamp dB, fallback dB, fallback-to-AGC, prevent-clipping. Renderer reads `volume_normalization_mode` and resolves a gain factor; `RodioOutput` applies it via `source.amplify(gain).limit(LimitSettings::dynamic_content())`. Primary loads stash incoming-track ReplayGain via `load_track_with_rg(url, rg, expected_duration_ms)`; the crossfade decoder uses `set_pending_crossfade_replay_gain()` before its stream is built — both paths land the right factor at stream creation.

## Bit-Perfect

`BitPerfectMode` (`types/player_settings/playback.rs`): tri-state `Off` / `Strict` / `Relaxed` (legacy bool records deserialize `true → Strict`, `false → Off`). The engine mirrors the setting in `crossfade.bit_perfect_mode` (the `CrossfadeCoordinator` field that `crossfade_eligible` reads), kept in sync by `set_bit_perfect`, which also delegates to `renderer.set_bit_perfect`.

- **Active condition**: `bit_perfect_active() = mode.builds_bit_perfect() && pw_volume_active` — both `Strict` and `Relaxed` build bit-perfect; it requires PipeWire-native volume so no software gain touches the samples.
- **DSP bypass** (`StreamingSource::next`): bit-perfect skips EQ entirely and, at unity, returns the raw decoded sample untouched (no software volume, not even the unity-curve multiply) so a settled body stays bit-identical. During a Relaxed crossfade the `volume` atomic carries only the equal-power fade coefficient, applied directly (already shaped by the tick — not re-curved through `perceptual_volume`).
- **Honest badge**: `current_stream_bit_perfect` is captured at stream-build time, not read live, so a mid-track toggle can't claim BIT-PERFECT for a stream still on the DSP path.
- **Crossfade gate**: `crossfade_blocked(current, incoming)` returns `Strict` → always blocked, `Relaxed` → blocked only on sample-rate / channel-count mismatch (the DAC can't re-clock mid-blend), `Off` → never. Both crossfade triggers gate on this one method with the same `(current, incoming)` pair.

## Probed Duration

`sanitize_probed_duration(probed_ms, expected_ms)` (`decoder.rs`) cross-checks Symphonia's probed length against the server's track length (threaded in as `expected_duration_ms`). When the probe wildly overshoots the server metadata (the Symphonia#516 Xing/CRC bug reports ~30 min for a ~4 min MP3), the server value wins. `compute_seek_scale(probed_ms, sanitized_ms)` then derives `seek_scale` (1.0 unless the probe was overridden) so seeks land at the right wall-clock position against the corrected timeline.

## Equalizer

- 10-band graphic EQ: `EqState.gains: Arc<[AtomicU32; EQ_BAND_COUNT]>` (`eq.rs::EQ_BAND_COUNT = 10` is the single source of truth for every gain-array width, including the `eq_gains` fields on the settings/types side), `EqProcessor` per-stream biquad bank
- Bands: 31 Hz–16 kHz (ISO standard center frequencies), ±12 dB clamp. Selecting a preset auto-enables the EQ.
- Headroom: −1 dB applied only when the max boost > 0 dB.

## Crossfade

Two concurrent `ActiveStream`s. Guards: both songs ≥ 10 s, duration clamped to `min(xfade, shorter / 2)`. The position-based trigger in `render_tick` must be **synchronous** — `mem::replace` swaps `crossfade_state` from `Armed` to `Active` in the same tick as the position check, then signals the engine async. Without the synchronous state flip, EOF fires first → hard cut. Cancellation splits by state: `cancel_crossfade()` clears `Active` (the in-flight stream); `disarm_crossfade()` clears `Armed` (metadata only). Pair them when both must clear (engine-level `cancel_crossfade`, renderer `stop`). `renderer.seek` calls only `cancel_crossfade()` so a pending gapless `Armed` state survives the seek and the position-based trigger can still fire.

Eligibility is the shared `CrossfadeCoordinator::crossfade_eligible()` predicate (`enabled || bit_perfect_mode == Relaxed` — the Relaxed gate lets same-format tracks blend under bit-perfect; see "Bit-Perfect") that all three arm/trigger sites (`store_prepared_decoder`, `rearm_crossfade_if_prepared`, `try_start_crossfade_transition`) call; the `crossfade_blocked()` gate then vetoes ineligible format pairs. **Finalize before EOF**: `try_finalize_crossfade(is_eof, renderer_fade_active)` finalizes the `Active` phase as soon as the renderer reports its fade is no longer active, even when the outgoing decoder has **not** hit EOF — past the fade the outgoing is at zero volume, so its remaining tail is inaudible and is discarded. Requiring `is_eof` for the `Active` phase left a torn state for seconds on coarse VBR seeks (renderer promoted the incoming, engine still `Active`), then promoted an already-EOF decoder so the next track was instantly skipped. The decode loop's `decoder_eof` signal is generation-gated so a loop superseded mid-`read_buffer` (which can block for seconds) can't store EOF for a no-longer-current track.

## Visualizer Sync

`StreamingSource::next()` → pre-volume samples (S16-scaled, volume-independent) → viz_buffer → FFT thread (60 fps, `try_lock()` only) → display buffers (Mutex) → shader GPU. `is_dirty()` gates redraws — GPU idle when paused. Toggling the visualizer **off** also releases CPU: a `viz_enabled` atomic (threaded renderer → `RodioOutput` → `StreamingSource`, ANDed with `feeds_visualizer`) skips the per-sample tap, and a synchronous `feed_active` flag early-returns the 60 Hz FFT worker — both driven from the cycle-visualization handler and synced at login so a persisted "off" never spins the worker. Only the active primary feeds the shared callback: `StreamHandle.feeds_visualizer: Arc<AtomicBool>` is set `true` at construction for primary streams (init / seek-recreate) and `false` for the crossfade incoming stream. `tick_crossfade` hands the feed off to the incoming at the equal-power midpoint (`progress >= 0.5`) so the viz never freezes when the outgoing's ring drains during the crossfade tail; `finalize_crossfade` then reaffirms `true` on the newly-promoted primary. Without these gates two concurrent streams at different rates would each tag batches with their own rate, flipping the visualizer's stored-rate atomic every batch and thrashing the spectrum engine into constant reinit (blank bars during cross-rate crossfade windows); without the midpoint handoff the visualizer freezes once the outgoing decoder EOFs and its ring drains, until finalize.
