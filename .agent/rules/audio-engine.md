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
├── CrossfadePhase: Idle → Active → OutgoingFinished
└── EqState (eq.rs) — shared atomic gains passed to each StreamingSource
```

## Audio Output (native PipeWire)

One native PipeWire stream via a shared `rodio::Mixer`:
- `SfxEngine` owns the app-wide `ActiveSink` (`NativePipewire` or `Cpal` fallback)
- `NativePipeWireSink` runs a dedicated `pw_nokkvi_out` thread with its own PipeWire mainloop
- Each track gets an `ActiveStream`: `RING_BUFFER_CAPACITY = 5_000_000` samples (~52 s at 48 kHz stereo, sized for radio jitter) + a `StreamingSource` added to the mixer
- `StreamingSource` implements `rodio::Source` (pull model) — pipewire callback pulls f32 samples

## Critical Rules

- **Track changes**: create fresh decoders **before** locking the engine; release the engine lock during decoder operations.
- **`source_generation` (AtomicU64)**: engine increments on `set_source()`; renderer snapshots and discards stale callbacks.
- **Mode toggle reset**: `reset_next_track()` clears the prepared decoder and disarms crossfade on shuffle / repeat / consume toggle.
- **Track-completion path**: the playback navigator releases its lock across engine I/O — do not re-introduce a held lock around `transition_to_queued()` / `set_source()`.
- Decoupled render thread: 20 ms intervals (50 Hz), handles crossfade tick + completion detection.

## Volume

Dual-path: PipeWire native (preferred) or software fallback.

- **PipeWire-native** (`pw_volume_active = true`): software volume at 1.0; SfxEngine sends cubic (`v³`) to PipeWire over IPC.
- **Software fallback**: exponential amplitude curve, per-sample exponential smoothing (~5 ms) for anti-aliasing.
- Crossfade: equal-power cos²/sin² curves. When `pw_volume_active`, fade uses only the coefficient and PipeWire applies user volume on top.

## Volume Normalization & ReplayGain

`VolumeNormalizationMode`: `Off`, `Agc`, `ReplayGainTrack`, `ReplayGainAlbum`. Settings under General → Application include preamp dB, fallback dB, fallback-to-AGC, prevent-clipping. Renderer reads `volume_normalization_mode` and resolves a gain factor; `RodioOutput` applies it via `source.amplify(gain).limit(LimitSettings::dynamic_content())`. Engine stashes incoming-track ReplayGain via `set_pending_replay_gain()` / `set_pending_crossfade_replay_gain()` so the next stream creation picks up the right factor.

## Equalizer

- 10-band graphic EQ: `EqState.gains: Arc<[AtomicU32; 10]>`, `EqProcessor` per-stream biquad bank
- Bands: 31 Hz–16 kHz (ISO standard center frequencies), ±12 dB clamp. Selecting a preset auto-enables the EQ.
- Headroom: −1 dB applied only when the max boost > 0 dB.

## Crossfade

Two concurrent `ActiveStream`s. Guards: both songs ≥ 10 s, duration clamped to `min(xfade, shorter / 2)`. The position-based trigger must be **synchronous** (set `crossfade_active = true` in the same tick as the position check) before signaling the engine async — otherwise EOF fires first → hard cut. Cancellation calls `cancel_crossfade()`.

## Visualizer Sync

`StreamingSource::next()` → pre-volume samples (S16-scaled, volume-independent) → viz_buffer → FFT thread (60 fps, `try_lock()` only) → display buffers (Mutex) → shader GPU. `is_dirty()` gates redraws — GPU idle when paused.
