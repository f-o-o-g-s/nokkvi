---
trigger: glob
globs: data/src/audio/**
---

# Audio Engine

## Architecture

```
CustomAudioEngine
├── AudioDecoder (Symphonia) → HTTP streaming via RangeHttpReader
│   └── RangeHttpReader (range_http_reader.rs) → sparse chunk-cached HTTP Range requests
├── AudioRenderer (ring buffers) → Visualizer callback from StreamingSource
│   └── RodioOutput (shared rodio Mixer) → creates ActiveStream per track
│       └── StreamingSource (rodio::Source) → EqProcessor → lock-free ring buffer → pipewire callback
├── CrossfadePhase state machine: Idle → Active → OutgoingFinished
└── EqState (eq.rs) → shared atomic gains passed to each StreamingSource
```

## Audio Output (native PipeWire)

All audio flows through **one native PipeWire stream** via a shared `rodio::Mixer`:
- `SfxEngine` owns the app-wide `ActiveSink` (either `NativePipewire` or `Cpal` fallback)
- `ActiveSink::NativePipewire(NativePipeWireSink)` runs a dedicated `pw_nokkvi_out` thread with its own PipeWire mainloop
- Music renderer receives a `Mixer` clone via `set_shared_mixer()`
- Each track gets an `ActiveStream`: ring buffer (480K samples ≈ 5s at 48kHz stereo) + `StreamingSource` added to mixer
- `StreamingSource` implements `rodio::Source` (pull model) — pipewire callback pulls f32 samples
- PipeWire stream properties: `MEDIA_ROLE = "Music"`, `MEDIA_NAME = "Nokkvi"`

## Deadlock Prevention (Critical)

1. **Fresh decoder creation**: On track change, create a new decoder — never reuse
2. **Atomic signaling**: `pending_clear` atomic flag clears visualizer buffer without blocking
3. **Decoupled initialization**: Gapless prep happens outside engine lock
4. **Mode toggle reset**: `reset_next_track()` clears prepared decoder, shared source, and disarms crossfade trigger — called on shuffle/repeat/consume toggle to prevent stale gapless transitions

**Never hold the engine lock during decoder operations.**

## Source Generation Counter

`source_generation` (`AtomicU64`) prevents stale track-completion callbacks:
- Engine increments on every `set_source()` (new track load)
- Renderer snapshots before releasing the lock
- If generation changed during callback, the callback is discarded
- Fixes consume+shuffle replaying the just-consumed track

## Decoupled Render Thread

Audio control logic runs on a dedicated `std::thread` at 20ms intervals (50Hz):
- Crossfade tick + completion detection
- Started via `engine.start_render_thread()`, stopped via `engine.stop_render_thread()`
- Actual audio rendering is done by pipewire callback (pulls from ring buffers)

## Volume Control

Dual-path volume: PipeWire native (preferred) or software fallback.

**PipeWire-native path** (when `pw_volume_active = true`):
```
UI → engine.set_volume() → renderer stores volume (software at 1.0)
                         → sfx_engine.set_output_volume(v³) → PipeWire channelVolumes via IPC
```
- Renderer keeps software volume at 1.0 (unity) during normal playback
- SfxEngine sends cubic volume (`v³`) to PipeWire — system mixer shows `cbrt(v³) = v` matching the slider
- Crossfade ramps use only the fade coefficient; PipeWire applies user volume uniformly on top

**Software fallback** (when `pw_volume_active = false`):
```
UI → engine.set_volume() → renderer.set_volume() → StreamHandle AtomicU32
```
- **Perceptual volume curve**: linear 0.0–1.0 input → exponential amplitude
- Volume applied per-sample with exponential smoothing (~5ms time constant) to avoid crossfade crackle
- Position tracked via atomic sample counter in `StreamHandle`

## Equalizer

- 10-band graphic equalizer via `data/src/audio/eq.rs`
- `EqState`: shared atomic gains (`Arc<[AtomicU32; 10]>`) — updated from UI, read by StreamingSource
- `EqProcessor`: per-stream biquad filter bank (10 bands × 2 channels), processes inline with `StreamingSource::next()`
- `EqPreset`: built-in presets (selecting one auto-enables the EQ), `CustomEqPreset`: user-saved name + gains
- Bands: 31Hz, 62Hz, 125Hz, 250Hz, 500Hz, 1kHz, 2kHz, 4kHz, 8kHz, 16kHz
- Range: ±15 dB per band — 0 dB = bypass

## Volume Normalization (AGC)

Optional per-track automatic gain control via rodio's AGC:
- `volume_normalization` (bool) + `normalization_level` (`NormalizationLevel` enum: Quiet/Normal/Loud) in `PlayerSettings`
- AGC applied as a `rodio::source::AutomaticGainControl` wrapper on the `StreamingSource`
- Target levels: Quiet=0.6, Normal=1.0, Loud=1.4
- Works with crossfade (each stream has independent AGC)
- Settings exposed in Settings → Playback

## Visualizer Sync

```
StreamingSource::next() → viz_buffer accumulator → callback(pre_volume_samples, rate)
    → VisualizerState.audio_callback() → sample_buffer
    → FFT thread (60 FPS, "visualizer-fft" thread) → display buffers (Mutex)
    → Shader.prepare() → GPU
```

- **Pre-volume samples**: visualizer receives raw samples **before** volume multiplication, scaled to S16 range. FFT input is volume-independent.
- Samples batched (~2048 samples) via shared `SharedVisualizerCallback` slot
- **Use `lock()` not `try_lock()`** for display buffers — guarantee valid data every frame
- Shader widget self-drives redraws via `Action::request_redraw()`
- Spectrum engine reinitialized when sample rate changes
- Resize debouncing: 100ms for bar count changes
- `live_bitrate` and `live_sample_rate` are `AtomicU32` — updated per-packet by decoder, read by UI

## Crossfade

Two concurrent `ActiveStream` instances on the shared mixer:

1. **Arming**: gapless prep completes + crossfade enabled → `arm_crossfade(duration_ms, incoming_format, track_duration_ms, incoming_duration_ms)`
2. **Guards** (inspired by MPD's `CanCrossFadeSong`):
   - Both songs must be ≥ `MIN_CROSSFADE_TRACK_MS` (10s) — shorter songs fall back to gapless
   - Effective duration clamped to `min(xfade, shorter_track / 2)` so outgoing track always has real audio for at least half the fade
3. **Position-based triggering**: `render_tick()` checks `position >= track_duration - crossfade_duration`. Triggered **synchronously** (sets `crossfade_active = true`) then signals engine async
4. **Blending**: `tick_crossfade()` ramps volumes using equal-power cos²/sin² curves. When `pw_volume_active`, fade uses only the coefficient (× 1.0); PipeWire applies user volume on top.
5. **Finalization**: crossfade stream promoted to primary, old primary stopped, decoders swapped

Settings: `crossfade_enabled` (bool) and `crossfade_duration_secs` (u32) in `PlayerSettings`, exposed in Settings → Playback.

Canceling operations (seek, skip, stop) cancel active crossfades via `cancel_crossfade()`.
Mode toggles (shuffle/repeat/consume) call `reset_next_track()` to clear any armed crossfade prep.

## Sound Effects Engine (`sfx_engine.rs`)

- Owns the app-wide `ActiveSink` (shared mixer) — `NativePipewire` or `Cpal` fallback
- Polyphonic voice mixing, pre-loaded WAV samples at 48kHz (bundled + user-customizable from config dir)
- Uses rodio `SamplesBuffer` + mixer for SFX playback
- Volume synced to PipeWire via `set_output_volume()` (cubic curve for perceptual match)
- `has_native_volume()` tells the renderer whether to use PipeWire or software volume

## Sparse Chunk Cache (Decoder)

HTTP Range requests with 256KB chunks, 16-chunk LRU cache (~4MB). Connection pooling enabled (TCP/TLS reuse between chunk fetches). Next-chunk prefetch after sequential reads reduces decoder stalls. Enables random-access seeking.