# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- Theater Mode (F11) fills the window with the playing track's cover, visualizer and lyrics, hiding the library, toolbar and nav.
- Escape leaves Theater Mode; keys aimed at the hidden list leave it too, and view keys leave and then switch.
- Right-clicking the Theater Mode cover offers Exit Theater Mode and Refresh Artwork.
- In Theater Mode the player bar slides away after 2.5 idle seconds and slides back on any mouse or key activity.
- In Theater Mode the mouse cursor hides after 2.5 idle seconds, like in a video player.
- In Theater Mode the lyrics grow with the window, up to two and a half times their Queue size.
- A Cover Art setting can swap the now-playing cover for a plain backdrop, everywhere or only in Theater Mode.
- With the cover hidden in Theater Mode, the visualizer spans the whole window instead of a centered square.
- Hovering the Queue cover reveals an expand icon that enters Theater Mode; its right-click menu gains Enter Theater Mode.
- In Theater Mode an exit icon rides above the player bar.
- A Theater Controls setting keeps Theater Mode's player bar auto-hiding, always shown, or always hidden.
- A Theater Fills the Screen setting also makes the window fullscreen in Theater Mode and restores it on leaving.
- `nokkvi theater` toggles Theater Mode from the command line.
- `nokkvi status` now reports whether Theater Mode is on.
- `nokkvi preset next` (or previous, lock, unlock, favorite, unfavorite, hide) controls MilkDrop presets from the command line.
- `nokkvi status` now reports the visualizer mode and the MilkDrop preset on screen.
- A MilkDrop visualizer mode plays MilkDrop presets in place of the Queue cover and fills Theater Mode.
- The visualizer button and `v` now cycle Off, Bars, Lines, Scope and MilkDrop.
- MilkDrop switches to another preset every 30 seconds and on each new track, showing the preset's name.
- In MilkDrop mode, `n` jumps to another preset and `p` returns to the previous one.
- In MilkDrop mode, Shift+M locks the current preset until pressed again.
- Right-clicking the MilkDrop panel offers Next, Previous, Lock, Favorite and Never Show This Preset.
- Never Show This Preset hides a preset for good and moves on at once.
- Presets dropped into `~/.config/nokkvi/milkdrop/` join the rotation; the refresh key picks up new ones.
- A MilkDrop settings section sets the preset interval, whether tracks change presets, favorites only, render quality and name toasts.
- Seven nokkvi MilkDrop presets draw in your theme's colours; five of them use the playing album's cover.
- nokkvi's MilkDrop presets recolour on the spot when you change theme, and keep the theme's dark colours in light mode.
- The MilkDrop Presets setting gains `nokkvi`, which plays only nokkvi's own presets.

### Changed

- Svalbard's visualizer peaks now step from dark to light teal instead of alternating two colors.
- The Harbour moon and stars now glow in the lightest peak color, whatever order a theme lists its peaks in.

### Fixed

### Removed

## v0.20.0 — 2026-09-23

### Added

- `nokkvi show` reopens the window after it was closed to the tray.
- `nokkvi show` on an already open window asks the desktop to flag it for attention.
- Launching nokkvi again while it runs now reopens a window closed to the tray instead of refusing.
- Launching nokkvi again while its window is open now flags that window instead of exiting with an error.
- MPRIS Raise now reopens a window closed to the tray.
- MPRIS Raise on an open window asks the desktop to flag it for attention.
- Bars gain four animated Gradient Modes (Drift, Swell, Pulse, Ripple) that keep the bar colors moving.

### Changed

- In Bars LED mode, the gap between LEDs now follows Bar Spacing, matching the gap between bars.
- In Bars LED mode, each LED now gets its own outline, like each bar does.

### Fixed

- A `nokkvi` command sent to a frozen instance now fails after five seconds instead of hanging.
- Turning off Show Tray Icon now removes the tray icon instead of leaving a dead one until restart.

## Older releases

- **v0.19.x** (2026-09-20 → 2026-09-21, v0.19.0–v0.19.1): [CHANGELOG-0.19.md](./changelog-archive/CHANGELOG-0.19.md)
- **v0.18.x** (2026-07-19 → 2026-07-25, v0.18.0–v0.18.4): [CHANGELOG-0.18.md](./changelog-archive/CHANGELOG-0.18.md)
- **v0.17.x** (2026-07-18, v0.17.0): [CHANGELOG-0.17.md](./changelog-archive/CHANGELOG-0.17.md)
- **v0.16.x** (2026-07-15, v0.16.0): [CHANGELOG-0.16.md](./changelog-archive/CHANGELOG-0.16.md)
- **v0.15.x** (2026-07-09 → 2026-07-11, v0.15.0–v0.15.1): [CHANGELOG-0.15.md](./changelog-archive/CHANGELOG-0.15.md)
- **v0.14.x** (2026-07-04 → 2026-07-06, v0.14.0–v0.14.2): [CHANGELOG-0.14.md](./changelog-archive/CHANGELOG-0.14.md)
- **v0.13.x** (2026-07-03, v0.13.0): [CHANGELOG-0.13.md](./changelog-archive/CHANGELOG-0.13.md)
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
