---
name: macos-audio
description: macOS audio output for nokkvi — CoreAudio via cpal/rodio replacing the native PipeWire sink, sample-rate handling, bit-perfect mode on macOS, the SFX sink, and how to test playback on a Mac. Use when touching data/src/audio/ sink code (sfx_engine.rs open_preferred_sink / ActiveSink, pipewire_output.rs, renderer.rs sink rebuild, graph_probe.rs), bit-perfect/RESAMPLED badge logic, or when audio misbehaves only on macOS. Load macos-port first for the general rules.
---

# macOS audio

Read `.claude/rules/audio-engine.md` (auto-injects on `data/src/audio/**`) first — every engine invariant there (lock discipline, `SourceGeneration`, synchronous crossfade trigger, FFT `try_lock()`) is platform-independent and still holds. Only the **sink** layer below the shared `rodio::Mixer` is platform-specific.

## How output is chosen today

`data/src/audio/sfx_engine.rs::open_preferred_sink(node_name, rate, request_native_rate)`:

- **Linux**: builds a `rodio::mixer` at `rate` and hands its source to `NativePipeWireSink` (`pipewire_output.rs`, compiled only on Linux). On failure → cpal fallback.
- **Everything else (macOS)**: `DeviceSinkBuilder::open_default_sink()` → cpal's CoreAudio host, default output device. `ActiveSink::Cpal`.

What the cpal arm means for the rest of the app on macOS:

| `ActiveSink` method | cpal arm | Effect on macOS |
|---|---|---|
| `rate()` | `MUSIC_SINK_DEFAULT_RATE` (48 000, `renderer.rs`) | Reports 48 kHz whatever the device runs at. Verify what cpal actually opens (the CoreAudio default device is often 44.1 kHz) and what the mixer resamples, before trusting any rate shown in the UI. |
| `has_native_volume()` | `false` | Bit-perfect needs native volume, so Strict/Relaxed can't engage on cpal. |
| `command_forwarder()` | `None` | No per-stream title/volume on the OS side (PipeWire node props have no CoreAudio equivalent). |
| `log_on_drop()` | forwarded to cpal | — |

The sink is rebuilt per rate in `renderer.rs` (around `open_preferred_sink("Nokkvi", rate, request_native)`): the old sink is dropped before the new one opens. That ordering matters on CoreAudio too — two open streams on one device can both briefly play.

## Bit-perfect on macOS

The Linux design (`BitPerfectMode` in `data/src/types/settings.rs`; badge logic in `src/state/playback.rs`, `src/update/playback.rs`; `graph_probe.rs`) has two halves:

1. **Ask** the server to switch the device clock: PipeWire `node.rate`.
2. **Verify** what really happened: `/proc/asound/*/pcm*/sub*/hw_params` for the device rate (`active_alsa_playback_rates`), plus a PipeWire graph walk naming the other app holding the sink (`probe_sink_holder`, stubbed to `None` off Linux).

On macOS both halves return nothing today, so the badge would sit on UNVERIFIED. Until there is a native implementation, hide bit-perfect (or show it as unavailable) on macOS rather than showing a verdict built from empty probes.

The macOS equivalent, when the owner wants it:

- Ask: set `kAudioDevicePropertyNominalSampleRate` on the output device (AudioObjectSetPropertyData) to the track rate, and open the stream at that rate. Optionally `kAudioDevicePropertyHogMode` for exclusive access (Relaxed vs Strict maps naturally: hog only in Strict).
- Verify: read `kAudioDevicePropertyNominalSampleRate` back; a mismatch → RESAMPLED. There is no cheap "who holds the device" query; the HOLDER name has no analogue — leave it `None`.
- Restore the device's original nominal rate when bit-perfect turns off or the app quits (CoreAudio keeps the change system-wide, unlike PipeWire's per-graph rate).
- This needs CoreAudio bindings. cpal already pulls `objc2-core-audio` / `objc2-core-audio-types` / `objc2-core-foundation` 0.3.x (`cargo tree -i objc2-core-audio`); depending on the same crates and versions directly adds no new code to the build, but it is still a new direct dependency → ask the owner first. Keep it in `data/` behind `#[cfg(target_os = "macos")]`, as a new `ActiveSink` variant or a rate helper beside the cpal arm, mirroring how `NativePipewire` is gated.

## Rules for sink code

- Every new `ActiveSink` variant gets a `#[cfg(target_os = "macos")]` on the variant **and** on each match arm; never add a `_ =>`.
- The `expect(unused_variables, …)` attributes on `open_preferred_sink`'s parameters are fulfilled only while macOS ignores them. Once a macOS arm reads `rate` / `request_native_rate`, remove the matching `cfg_attr` or the build fails (that is the point of `expect`).
- CoreAudio property calls can block (device wake, hog negotiation). Keep them off the UI thread and outside the engine lock, like decoder init in transition paths (CLAUDE.md "Track changes and the engine lock").
- Device changes (headphones plugged, AirPods connecting) switch the system default under a running cpal stream. Check that a default-device change doesn't kill output silently; cpal surfaces it as a stream error callback — log at `warn!` and rebuild the sink.
- Don't change the Linux PipeWire path while porting. If shared code must change, describe the Linux effect in the handoff (it can't be tested from the Mac; see `macos-port` → Verifying).

## Testing on the Mac

- `cargo test -p nokkvi-data` runs the engine/crossfade/EQ tests that don't need a device; they must pass unchanged on macOS.
- Manual playback check for the owner after `cargo build --release`: gapless album, crossfade on skip, seek in a FLAC, a radio station, EQ on/off, volume, and a default-device switch mid-track. Also the SFX (UI sounds) — they share `open_preferred_sink`.
- Logs: the file log is in the state dir (`~/Library/Application Support/nokkvi/nokkvi.log` on macOS); look for the `🔊 Audio output:` line to see which sink opened.
