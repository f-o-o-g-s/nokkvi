---
trigger: glob
globs: data/src/audio/**
---

# Audio Engine

## Architecture

```
CustomAudioEngine
├── AudioDecoder (Symphonia) → HTTP streaming via RangeHttpReader (256KB chunks, 16-chunk LRU, next-chunk prefetch)
├── AudioRenderer (ring buffers) → Visualizer callback from StreamingSource
│   └── RodioOutput (shared rodio Mixer) → creates ActiveStream per track
│       └── StreamingSource (rodio::Source) → EqProcessor → lock-free ring buffer → pipewire callback
├── CrossfadePhase state machine: Idle → Active → OutgoingFinished
└── EqState (eq.rs) → shared atomic gains passed to each StreamingSource
```

## Audio Output (native PipeWire)

One native PipeWire stream via a shared `rodio::Mixer`:
- `SfxEngine` owns the app-wide `ActiveSink` (`NativePipewire` or `Cpal` fallback)
- `NativePipeWireSink` runs a dedicated `pw_nokkvi_out` thread with its own PipeWire mainloop
- Each track gets an `ActiveStream`: ring buffer (480K samples ≈ 5s at 48kHz stereo) + `StreamingSource` added to mixer
- `StreamingSource` implements `rodio::Source` (pull model) — pipewire callback pulls f32 samples

## Critical Rules

- **Never hold the engine lock during decoder operations** — create fresh decoders on track change
- **`source_generation` (AtomicU64)**: engine increments on `set_source()`; renderer snapshots and discards stale callbacks
- **Mode toggle reset**: `reset_next_track()` clears prepared decoder + disarms crossfade on shuffle/repeat/consume toggle
- Decoupled render thread: 20ms intervals (50Hz), handles crossfade tick + completion detection

## Volume

Dual-path: PipeWire native (preferred) or software fallback.

- **PipeWire-native** (`pw_volume_active = true`): software at 1.0, SfxEngine sends cubic volume (`v³`) to PipeWire via IPC
- **Software fallback**: exponential amplitude curve, per-sample exponential smoothing (~5ms) to avoid crackle
- Crossfade: equal-power cos²/sin² curves. When `pw_volume_active`, fade uses only the coefficient; PipeWire applies user volume on top.

## Equalizer

- 10-band graphic EQ: shared `Arc<[AtomicU32; 10]>` gains, `EqProcessor` per-stream biquad filter bank
- Bands: 31Hz–16kHz, ±15 dB. Selecting a preset auto-enables the EQ.

## Crossfade

Two concurrent `ActiveStream`s. Guards: both songs ≥ 10s, duration clamped to `min(xfade, shorter/2)`. Position-based trigger must be **synchronous** (set `crossfade_active = true`) then signal engine async. Canceling ops call `cancel_crossfade()`.

## Visualizer Sync

`StreamingSource::next()` → pre-volume samples → viz_buffer → FFT thread (60 FPS) → display buffers (Mutex) → Shader GPU. `is_dirty()` gates redraws — GPU idle when paused.