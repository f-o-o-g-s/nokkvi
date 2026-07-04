# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- **Crossfade Curve** picker (Settings → Playback → Transitions): **Equal Power** (the new default) holds loudness steady through the blend, **Constant Gain** (the previous curve) dips about 3 dB in the middle for a softer same-album feel, and **Linear** is a plain straight-line fade.
- **Minimum Track Length to Crossfade** slider (0–60 s): the old hardcoded 10-second floor is now configurable, so 0 blends everything including interludes and 30 only blends full-length songs.
- **Keep Gapless Albums Seamless** (off by default): skips the blend when the next track continues the same album, so authored segues stay tight. Crossfade still applies between different albums, on shuffle, and on compilations.
- A new **Fading** section under Settings → Playback:
  - **Smooth Track Starts** (on by default): ramps up the first ~20 ms of each track to remove the click when a skip or seek lands mid-waveform; off restores an instant, honest onset.
  - **Fade on Pause / Resume** and **Fade on Stop** (off by default, 20–500 ms): soft gain ramps instead of instant cuts, so pausing, resuming, and stopping no longer click.
  - **Fade Radio Switches** (off by default): a short fade when starting a radio station or returning to the queue; the fade-in waits for the stream's first real audio instead of popping at full gain after the prebuffer.
  - **Fade on Skip** (Off / Boundary Fade / Crossfade, default Off) with a 1–4 s duration: manual Next/Previous can fade out or blend into the next track instead of hard-cutting.
  - **Skip Silence Between Tracks** (off by default): trims silent lead-ins from tracks prepared in advance and starts the blend early over a silent outro. Bit-perfect streams never trim.
  - **Gap / Overlap Trim** (−2 to +2 s): hold a moment of silence between tracks, or start blends early.
  - **Snap Crossfade to Musical Bars** (off by default): rounds the blend length to whole bars of the outgoing track's BPM tag so beats line up through the blend; ignored when a track has no BPM.

### Changed

- The default crossfade curve is now true **Equal Power**, removing the ~3 dB loudness dip in the middle of blends between different songs; the previous curve remains selectable as Constant Gain.
- Fresh streams (play, seek, skip) now start with the ~20 ms de-click ramp from Smooth Track Starts; bit-perfect streams keep their instant onset.

### Fixed

- Crossfades on the default path (Crossfade on, Bit-Perfect off) no longer collapse to near-silence mid-blend: the fade coefficient was re-curved through the perceptual volume taper, putting every fade midpoint at roughly −24 dB. Fades now apply linearly on their own channel, independent of user volume.
- A crossfade whose incoming network stream pushes a few KB and then stalls now recovers (the outgoing track is restored and the stall skipped past) instead of promoting a stream that plays its residue and hangs; a stalled fade also signals recovery once instead of retrying every 20 ms.
- Cancelling a crossfade after its midpoint no longer leaves the visualizer spectrum frozen until the next track change.

### Removed

## v0.13.0 — 2026-07-03

### Added

- Right-click a playlist or radio station and choose **Set Custom Artwork…** to upload a cover image to Navidrome, shown across all clients.
- **Reset Artwork** removes the uploaded image and restores the automatic cover: an album collage for playlists, stream art or the tower glyph for stations.

### Fixed

- Reloading the Radios list (the refresh button or R hotkey) no longer silently drops the name sort when the station count is unchanged.

## Older releases

- **v0.12.x** (2026-06-28 → 2026-07-02, v0.12.0–v0.12.2): [CHANGELOG-0.12.md](./changelog-archive/CHANGELOG-0.12.md)
- **v0.11.x** (2026-06-22 → 2026-06-25, v0.11.0–v0.11.3): [CHANGELOG-0.11.md](./changelog-archive/CHANGELOG-0.11.md)
- **v0.10.x** (2026-06-19 → 2026-06-21, v0.10.0–v0.10.1): [CHANGELOG-0.10.md](./changelog-archive/CHANGELOG-0.10.md)
- **v0.9.x** (2026-06-15 → 2026-06-18, v0.9.0–v0.9.4): [CHANGELOG-0.9.md](./changelog-archive/CHANGELOG-0.9.md)
- **v0.8.x** (2026-06-14, v0.8.0): [CHANGELOG-0.8.md](./changelog-archive/CHANGELOG-0.8.md)
- **v0.7.x** (2026-06-07 → 2026-06-10, v0.7.0–v0.7.2): [CHANGELOG-0.7.md](./changelog-archive/CHANGELOG-0.7.md)
- **v0.6.x** (2026-05-25 → 2026-06-06, v0.6.0–v0.6.10): [CHANGELOG-0.6.md](./changelog-archive/CHANGELOG-0.6.md)
- **v0.5.x** (2026-05-21 → 2026-05-24, v0.5.0–v0.5.3): [CHANGELOG-0.5.md](./changelog-archive/CHANGELOG-0.5.md)
- **v0.4.x** (2026-05-16 → 2026-05-19, v0.4.0–v0.4.2): [CHANGELOG-0.4.md](./changelog-archive/CHANGELOG-0.4.md)
- **v0.3.x** (2026-04-27 → 2026-05-14, v0.3.1–v0.3.17): [CHANGELOG-0.3.md](./changelog-archive/CHANGELOG-0.3.md)
