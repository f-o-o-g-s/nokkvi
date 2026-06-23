# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

### Changed

- New installs now default the visualizer height to 40% of the window (up from 25%); existing configs keep their value.

### Fixed

### Removed

## v0.11.0 — 2026-06-22

### Added

- Picking a theme now opens a searchable modal where each row is painted in that theme's own colors — a live color preview.
- New Icon Set setting (Interface → Font & Icons) switches the UI between Phosphor and Lucide glyphs.

### Changed

- The Scope visualizer now defaults to a height-based gradient instead of a static (flat) color.
- Verbose Config is now a three-way On/Off/Clean choice, where Clean writes a sparse config.toml with no auto-added comments.
- The UI now uses Phosphor icons by default, with Lucide's thin-outline set available in Settings.
- The Queue and Playlists nav tabs now have dedicated icons (a queue glyph for Queue, a playlist glyph for Playlists).
- The surfing-boat doodad's anchor now follows the icon set: a filled Phosphor anchor, or the stroked Lucide one.

### Fixed

- Single-weight fonts (such as pixel fonts) now display correctly for bold and medium UI text instead of falling back to a generic serif/sans.

### Removed

- The inline per-color theme editors in Settings → Theme — edit a theme's colors directly in the TOML file instead.

## Older releases

- **v0.10.x** (2026-06-19 → 2026-06-21, v0.10.0–v0.10.1): [CHANGELOG-0.10.md](./changelog-archive/CHANGELOG-0.10.md)
- **v0.9.x** (2026-06-15 → 2026-06-18, v0.9.0–v0.9.4): [CHANGELOG-0.9.md](./changelog-archive/CHANGELOG-0.9.md)
- **v0.8.x** (2026-06-14, v0.8.0): [CHANGELOG-0.8.md](./changelog-archive/CHANGELOG-0.8.md)
- **v0.7.x** (2026-06-07 → 2026-06-10, v0.7.0–v0.7.2): [CHANGELOG-0.7.md](./changelog-archive/CHANGELOG-0.7.md)
- **v0.6.x** (2026-05-25 → 2026-06-06, v0.6.0–v0.6.10): [CHANGELOG-0.6.md](./changelog-archive/CHANGELOG-0.6.md)
- **v0.5.x** (2026-05-21 → 2026-05-24, v0.5.0–v0.5.3): [CHANGELOG-0.5.md](./changelog-archive/CHANGELOG-0.5.md)
- **v0.4.x** (2026-05-16 → 2026-05-19, v0.4.0–v0.4.2): [CHANGELOG-0.4.md](./changelog-archive/CHANGELOG-0.4.md)
- **v0.3.x** (2026-04-27 → 2026-05-14, v0.3.1–v0.3.17): [CHANGELOG-0.3.md](./changelog-archive/CHANGELOG-0.3.md)
