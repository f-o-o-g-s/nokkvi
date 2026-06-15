# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

### Changed

- The crossfade settings now explain that turning crossfade off plays tracks gapless, and that very short tracks always play gapless.

### Fixed

- Turning the visualizer off now releases the CPU it was using during playback; previously its background audio analysis kept running unseen.
- The crossfade duration slider no longer offers values above 12 seconds that silently reset to 12 on the next launch.
- The Mini Player's thumbnail now shows artwork when you skip to a track that isn't currently visible in the queue, instead of a gray box.

### Removed

## v0.8.0 — 2026-06-14

### Added

- The Lines visualizer can now glow like neon, with a halo that brightens with the music and flares on each beat, plus an adjustable Glow Intensity setting.
- The Bars visualizer now blooms toward the peak color when bars hit a beat, with an adjustable Peak Flash setting.
- The visualizer now has a bloom glow: bright bars, peak flashes, and the neon line bleed a soft halo that surges on bass drops, with a Bloom toggle and intensity setting.
- A Beat Reactivity setting controls how hard the glow, bars, and bloom pump on the beat and bass (0 = static, loudness-only).
- A Motion Trails setting makes the bars and lines leave a fading comet-trail after-image (off by default).
- An Echo setting adds Milkdrop-style feedback: the visualizer spirals and tunnels into itself, swirling with the bass and beat (off by default).
- A CRT / Film setting adds a retro post-process — chromatic aberration, scanlines, vignette, grain, and a beat zoom-punch (off by default).

### Changed

- The metadata strip now defaults to Mini Player instead of the Player Bar.
- The sort and search toolbar now auto-hides by default, collapsing to a count strip.
- The visualizer's bars now default to a solid fill instead of segmented LED bars.
- The visualizer's surfing boat (Lines mode) is now enabled by default.
- Nokkvi now builds against upstream iced (iced-rs/iced) instead of a personal fork, now that the image-resize crash fix it carried landed upstream.

### Fixed

- The now-playing row's breathing glow and sheen now appear on the first track of a freshly played queue, not only after skipping ahead.
- Certain MP3s no longer display a wildly wrong duration and bitrate (such as 30:24 for a 4:44 track).
- Seeking in those affected MP3s now lands at the chosen position instead of far earlier in the track.
- Seeking near the end of a track no longer makes the crossfaded-in next song get skipped moments after it starts.

### Removed

- The Bars visualizer's shimmer, energy, and alternate gradient modes, which the new glow, bloom, and beat effects make redundant (existing configs fall back to wave).

## Older releases

- **v0.7.x** (2026-06-07 → 2026-06-10, v0.7.0–v0.7.2): [CHANGELOG-0.7.md](./changelog-archive/CHANGELOG-0.7.md)
- **v0.6.x** (2026-05-25 → 2026-06-06, v0.6.0–v0.6.10): [CHANGELOG-0.6.md](./changelog-archive/CHANGELOG-0.6.md)
- **v0.5.x** (2026-05-21 → 2026-05-24, v0.5.0–v0.5.3): [CHANGELOG-0.5.md](./changelog-archive/CHANGELOG-0.5.md)
- **v0.4.x** (2026-05-16 → 2026-05-19, v0.4.0–v0.4.2): [CHANGELOG-0.4.md](./changelog-archive/CHANGELOG-0.4.md)
- **v0.3.x** (2026-04-27 → 2026-05-14, v0.3.1–v0.3.17): [CHANGELOG-0.3.md](./changelog-archive/CHANGELOG-0.3.md)
