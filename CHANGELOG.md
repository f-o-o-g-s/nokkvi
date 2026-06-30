# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

### Changed

### Fixed

- The auto-hide view-header toolbar now collapses when nokkvi loses focus, instead of staying stuck expanded behind another window or re-expanding on return.
- Pressing Shift+C to centre the playing track no longer expands the auto-hide toolbar.

### Removed

## v0.12.0 — 2026-06-28

### Added

- Internet radio stations can now scrobble to **ListenBrainz** and **Last.fm**, set up under Settings → Playback → Radio Scrobbling.
- nokkvi reads the Artist - Title from a station's stream title and scrobbles it once it has played past your listen-threshold slider.
- A separate toggle sends a live now-playing update each time the radio track changes, and each service's row shows whether it's connected.
- Radio scrobble keys live in a plaintext `[radio_scrobble]` block in `config.toml`, editable in the GUI or by hand and overridable with `NOKKVI_RADIO_*` environment variables.
- Radio stations now show artwork (an uploaded station logo, or the last now-playing stream image) instead of a generic tower glyph.
- Station artwork appears in the station list, the large artwork panel, and the MiniPlayer, and persists across restarts.
- The over-cover visualizer now animates over a station's artwork while its stream plays.
- Right-clicking a radio station now offers **Refresh Artwork** to clear a stale or wrong thumbnail.

## Older releases

- **v0.11.x** (2026-06-22 → 2026-06-25, v0.11.0–v0.11.3): [CHANGELOG-0.11.md](./changelog-archive/CHANGELOG-0.11.md)
- **v0.10.x** (2026-06-19 → 2026-06-21, v0.10.0–v0.10.1): [CHANGELOG-0.10.md](./changelog-archive/CHANGELOG-0.10.md)
- **v0.9.x** (2026-06-15 → 2026-06-18, v0.9.0–v0.9.4): [CHANGELOG-0.9.md](./changelog-archive/CHANGELOG-0.9.md)
- **v0.8.x** (2026-06-14, v0.8.0): [CHANGELOG-0.8.md](./changelog-archive/CHANGELOG-0.8.md)
- **v0.7.x** (2026-06-07 → 2026-06-10, v0.7.0–v0.7.2): [CHANGELOG-0.7.md](./changelog-archive/CHANGELOG-0.7.md)
- **v0.6.x** (2026-05-25 → 2026-06-06, v0.6.0–v0.6.10): [CHANGELOG-0.6.md](./changelog-archive/CHANGELOG-0.6.md)
- **v0.5.x** (2026-05-21 → 2026-05-24, v0.5.0–v0.5.3): [CHANGELOG-0.5.md](./changelog-archive/CHANGELOG-0.5.md)
- **v0.4.x** (2026-05-16 → 2026-05-19, v0.4.0–v0.4.2): [CHANGELOG-0.4.md](./changelog-archive/CHANGELOG-0.4.md)
- **v0.3.x** (2026-04-27 → 2026-05-14, v0.3.1–v0.3.17): [CHANGELOG-0.3.md](./changelog-archive/CHANGELOG-0.3.md)
