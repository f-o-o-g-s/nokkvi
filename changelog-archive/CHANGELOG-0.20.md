# Changelog — v0.20.x archive

Release v0.20.0, covering 2026-09-23. The current changelog (v0.21.0 onward) lives in [CHANGELOG.md](../CHANGELOG.md).

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
