# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

### Changed

### Fixed

### Removed

## v0.10.1 — 2026-06-21

### Added

- New built-in **Firmium** theme: warm gold on near-black with a terminal/monospace mood (ported from the Firmium client), plus an original neutral-grey light companion.

### Changed

- The Scope particle field now stays crisp during echo instead of smearing into the feedback trail.
- Retuned the default Scope look: stronger echo, fewer and slower particles, a thinner ring, and softer glow and fill.

### Fixed

- The Lines and Scope visualizers no longer show triangular spikes or glow artifacts at sharp peaks and bends.
- With Shuffle on, clicking a track to play while stopped or paused now reshuffles the queue behind it instead of stopping after one song.
- Dragging the progress bar while paused now seeks without resuming playback.
- The Scope Particle Speed slider now changes the dust's whole-field motion across its full range, not just the initial launch.

## v0.10.0 — 2026-06-19

### Added

- A new Scope visualizer mode draws the audio waveform as a circular oscilloscope ring over the now-playing cover art, cycled after Lines.
- Scope mode adds a glowing particle field, a luminous beam glow, and a radial fill, tunable in a new Settings → Visualizer → Scope section.
- Bars and Lines can now be positioned per mode — over the now-playing cover art or in the band above the player bar.

### Changed

- Bars and Lines now draw over the now-playing cover art by default instead of a band above the player bar.
- Motion Trails and Echo are now tuned per visualizer mode instead of a single global pair, resetting existing global values to off.

### Fixed

- The Queue's Playing-From-Playlist banner now has a divider separating it from the cover art directly above it in a portrait window.

## Older releases

- **v0.9.x** (2026-06-15 → 2026-06-18, v0.9.0–v0.9.4): [CHANGELOG-0.9.md](./changelog-archive/CHANGELOG-0.9.md)
- **v0.8.x** (2026-06-14, v0.8.0): [CHANGELOG-0.8.md](./changelog-archive/CHANGELOG-0.8.md)
- **v0.7.x** (2026-06-07 → 2026-06-10, v0.7.0–v0.7.2): [CHANGELOG-0.7.md](./changelog-archive/CHANGELOG-0.7.md)
- **v0.6.x** (2026-05-25 → 2026-06-06, v0.6.0–v0.6.10): [CHANGELOG-0.6.md](./changelog-archive/CHANGELOG-0.6.md)
- **v0.5.x** (2026-05-21 → 2026-05-24, v0.5.0–v0.5.3): [CHANGELOG-0.5.md](./changelog-archive/CHANGELOG-0.5.md)
- **v0.4.x** (2026-05-16 → 2026-05-19, v0.4.0–v0.4.2): [CHANGELOG-0.4.md](./changelog-archive/CHANGELOG-0.4.md)
- **v0.3.x** (2026-04-27 → 2026-05-14, v0.3.1–v0.3.17): [CHANGELOG-0.3.md](./changelog-archive/CHANGELOG-0.3.md)
