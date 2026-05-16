---
trigger: glob
globs: data/src/audio/**
---

# Audio Engine

## Architecture

```
CustomAudioEngine
├── AudioDecoder (Symphonia)
│   ├── Standard: HTTP via RangeHttpReader (256KB chunks, MAX_CACHED_CHUNKS=16, prefetch on seek)
│   └── Radio:    HTTP via AsyncNetworkBuffer (tokio async → bounded mpsc → sync Read) with generation-gated auto-reconnect
├── AudioRenderer (ring buffers) → visualizer callback from StreamingSource
│   └── RodioOutput (shared rodio Mixer) → ActiveStream per track
│       └── StreamingSource (rodio::Source) → EqProcessor → lock-free ring buffer → pipewire callback
├── CrossfadePhase enum (engine): Idle → Active { decoder, incoming_source } → OutgoingFinished { decoder, incoming_source }
├── CrossfadeState enum (renderer): Idle / Armed / Active — per-phase data lives inside the variant; `mem::replace` swaps phases atomically
├── GaplessSlot — bundles `decoder` + `source` + `prepared` under one `tokio::Mutex` so the decode loop, async path, and `cancel_crossfade` always lock together (audit IG-13)
├── DecodeLoopHandle / SourceGeneration (`generation.rs`) — typed atomic-counter wrappers; `bump_for_user_action` / `bump_for_gapless` / `accept_internal_swap` make every "stop the loop" / "invalidate callback" call site self-documenting
└── EqState (eq.rs) — shared atomic gains passed to each StreamingSource
```

## Audio Output (native PipeWire)

One native PipeWire stream via a shared `rodio::Mixer`:
- `SfxEngine` owns the app-wide `ActiveSink` (`NativePipewire` or `Cpal` fallback)
- `NativePipeWireSink` runs a dedicated `pw_nokkvi_out` thread with its own PipeWire mainloop
- Each track gets an `ActiveStream`: `RING_BUFFER_CAPACITY = 5_000_000` samples (~52 s at 48 kHz stereo, sized for radio jitter) + a `StreamingSource` added to the mixer
- `StreamingSource` implements `rodio::Source` (pull model) — pipewire callback pulls f32 samples

## Critical Rules

- **Codec registry**: every Symphonia decoder/lookup MUST go through `audio::symphonia_registry::codecs()`, never `symphonia::default::get_codecs()`. The shared registry adds `symphonia-adapter-libopus` on top of the Symphonia defaults; the upstream default registry has no Opus decoder (pdeljanov/Symphonia#8 open since 2020), so any direct call to `get_codecs()` re-breaks `.opus` playback (see GH#3).
- **Track changes**: create fresh decoders **before** locking the engine; release the engine lock during decoder operations. Use `engine.load_track_with_rg(url, rg)` — the atomic pair that stashes ReplayGain on the renderer and then calls `set_source(url)`, replacing the historical `set_pending_replay_gain` + `load_track` / `set_source` pairing in `PlaybackController`.
- **`SourceGeneration`**: typed atomic counter; `bump_for_user_action()` on every user-driven source change. The renderer snapshots `current()` before releasing the engine lock and discards stale completion callbacks.
- **Mode toggle reset**: `reset_next_track()` clears the prepared decoder and disarms crossfade on shuffle / repeat / consume toggle. Mode toggles return `ModeToggleEffect` (currently a no-op type) so the controller chains the reset uniformly.
- **Track-completion path**: the playback navigator releases its lock across engine I/O — do not re-introduce a held lock around `transition_to_queued()` / `set_source()`.
- Decoupled render thread: 20 ms intervals (50 Hz), handles crossfade tick + completion detection.

## Volume

Dual-path: PipeWire native (preferred) or software fallback.

- **PipeWire-native** (`pw_volume_active = true`): software volume at 1.0; SfxEngine sends cubic (`v³`) to PipeWire over IPC.
- **Software fallback**: exponential amplitude curve, per-sample exponential smoothing (~5 ms) for anti-aliasing.
- Crossfade: equal-power cos²/sin² curves. When `pw_volume_active`, fade uses only the coefficient and PipeWire applies user volume on top.

## Volume Normalization & ReplayGain

`VolumeNormalizationMode`: `Off`, `Agc`, `ReplayGainTrack`, `ReplayGainAlbum`. Settings under General → Application include preamp dB, fallback dB, fallback-to-AGC, prevent-clipping. Renderer reads `volume_normalization_mode` and resolves a gain factor; `RodioOutput` applies it via `source.amplify(gain).limit(LimitSettings::dynamic_content())`. Primary loads stash incoming-track ReplayGain via `load_track_with_rg(url, rg)`; the crossfade decoder uses `set_pending_crossfade_replay_gain()` before its stream is built — both paths land the right factor at stream creation.

## Equalizer

- 10-band graphic EQ: `EqState.gains: Arc<[AtomicU32; 10]>`, `EqProcessor` per-stream biquad bank
- Bands: 31 Hz–16 kHz (ISO standard center frequencies), ±12 dB clamp. Selecting a preset auto-enables the EQ.
- Headroom: −1 dB applied only when the max boost > 0 dB.

## Crossfade

Two concurrent `ActiveStream`s. Guards: both songs ≥ 10 s, duration clamped to `min(xfade, shorter / 2)`. The position-based trigger must be **synchronous** (set `crossfade_active = true` in the same tick as the position check) before signaling the engine async — otherwise EOF fires first → hard cut. Cancellation splits by state: `cancel_crossfade()` clears `Active` (the in-flight stream); `disarm_crossfade()` clears `Armed` (metadata only). Pair them when both must clear (engine-level `cancel_crossfade`, renderer `stop`). `renderer.seek` calls only `cancel_crossfade()` so a pending gapless `Armed` state survives the seek and the position-based trigger can still fire.

## Visualizer Sync

`StreamingSource::next()` → pre-volume samples (S16-scaled, volume-independent) → viz_buffer → FFT thread (60 fps, `try_lock()` only) → display buffers (Mutex) → shader GPU. `is_dirty()` gates redraws — GPU idle when paused. Only the active primary feeds the shared callback: `StreamHandle.feeds_visualizer: Arc<AtomicBool>` is set `true` at construction for primary streams (init / seek-recreate) and `false` for the crossfade incoming stream. `tick_crossfade` hands the feed off to the incoming at the equal-power midpoint (`progress >= 0.5`) so the viz never freezes when the outgoing's ring drains during the crossfade tail; `finalize_crossfade` then reaffirms `true` on the newly-promoted primary. Without these gates two concurrent streams at different rates would each tag batches with their own rate, flipping the visualizer's stored-rate atomic every batch and thrashing the spectrum engine into constant reinit (blank bars during cross-rate crossfade windows); without the midpoint handoff the visualizer freezes once the outgoing decoder EOFs and its ring drains, until finalize.
