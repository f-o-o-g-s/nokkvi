# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- `nokkvi --version` and `nokkvi --help`

### Changed

- Per-user data layout now follows XDG: `app.redb` and `nokkvi.log` move from `~/.config/nokkvi/` to `~/.local/state/nokkvi/` (one-time migration on first launch). `config.toml`, `themes/`, and `sfx/` stay in `~/.config/nokkvi/`.

### Fixed

- Auto-login no longer floods stderr with pre-login `shell_task` warnings; auth lifecycle (resume / success / failure) is now visible at default verbosity.

## v0.3.5 — 2026-04-28

### Fixed

- Close-to-tray now actually hides the window on Wayland (Hyprland, KDE, GNOME, sway)

## v0.3.4 — 2026-04-28

### Added

- System tray integration with optional close-to-tray
- Artwork column display modes (Auto / Native / Stretched / Never) with draggable column width
- Per-mode hysteresis on the player bar; folded modes move into a kebab menu, transports collapse to prev/play/next at narrow widths

### Fixed

- Only one overlay menu (hamburger, kebab, view-header dropdowns, right-click) can be open at a time

## v0.3.3 — 2026-04-27

### Fixed

- Crash and missing artwork at certain window sizes caused by an iced sub-pixel image bug
- Cargo `license` field updated to current SPDX `GPL-3.0-only`

### Changed

- Added a 512px PNG launcher icon as a fallback for desktops that mis-render the SVG

## v0.3.2 — 2026-04-27

### Fixed

- About modal no longer shows "Commit: unknown" when built outside a git context

## v0.3.1 — 2026-04-27

### Added

- ReplayGain track/album volume normalization with pre-amp, untagged-track fallback, and clipping prevention; replaces the boolean AGC toggle with Off / RG Track / RG Album / AGC

### Changed

- Quieter default terminal logging (WARN+); full debug still goes to `~/.config/nokkvi/nokkvi.log`
- Tarball releases on GitHub; `install.sh` auto-detects tarball vs source build

### Fixed

- Workspace `license` field corrected so `nokkvi` and `nokkvi-data` crates report GPL-3.0
