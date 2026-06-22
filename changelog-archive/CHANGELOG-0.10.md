# Changelog — v0.10.x archive

Releases v0.10.0–v0.10.1, covering 2026-06-19 → 2026-06-21. The current changelog (v0.11.0 onward) lives in [CHANGELOG.md](../CHANGELOG.md).

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
