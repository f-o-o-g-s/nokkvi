# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

### Changed

### Fixed

### Removed

## v0.9.0 — 2026-06-15

### Added

- A new Bit-Perfect playback mode sends audio to your DAC untouched, with no EQ, software volume, or limiter.
- Bit-Perfect mode switches the output device to each track's native sample rate.
- A now-playing badge reads the real device clock and reports whether playback is bit-perfect, resampled, or unverified.

### Changed

- Hi-res tracks (24-bit and float) now play back at their real bit depth instead of being narrowed to 16-bit.
- Turning on Crossfade now turns off Bit-Perfect, and vice versa.
- The crossfade settings now explain that turning crossfade off plays tracks gapless, and that very short tracks always play gapless.

### Fixed

- Turning the visualizer off now releases the CPU it was using during playback.
- The crossfade duration slider no longer offers values above 12 seconds that silently reset to 12 on the next launch.
- The Mini Player's thumbnail now shows artwork when you skip to a track not visible in the queue, instead of a gray box.

## Older releases

- **v0.8.x** (2026-06-14, v0.8.0): [CHANGELOG-0.8.md](./changelog-archive/CHANGELOG-0.8.md)
- **v0.7.x** (2026-06-07 → 2026-06-10, v0.7.0–v0.7.2): [CHANGELOG-0.7.md](./changelog-archive/CHANGELOG-0.7.md)
- **v0.6.x** (2026-05-25 → 2026-06-06, v0.6.0–v0.6.10): [CHANGELOG-0.6.md](./changelog-archive/CHANGELOG-0.6.md)
- **v0.5.x** (2026-05-21 → 2026-05-24, v0.5.0–v0.5.3): [CHANGELOG-0.5.md](./changelog-archive/CHANGELOG-0.5.md)
- **v0.4.x** (2026-05-16 → 2026-05-19, v0.4.0–v0.4.2): [CHANGELOG-0.4.md](./changelog-archive/CHANGELOG-0.4.md)
- **v0.3.x** (2026-04-27 → 2026-05-14, v0.3.1–v0.3.17): [CHANGELOG-0.3.md](./changelog-archive/CHANGELOG-0.3.md)
