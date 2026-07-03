# Changelog — v0.12.x archive

Releases v0.12.0–v0.12.2, covering 2026-06-28 → 2026-07-02. The current changelog (v0.13.0 onward) lives in [CHANGELOG.md](../CHANGELOG.md).

## v0.12.2 — 2026-07-02

### Changed

- Clickable artist and album name links in list rows are now off by default (re-enable under Settings → Interface → Slot Text Links).

### Fixed

- "Play Next" now refreshes the queue immediately, so the moved song appears in its new spot and stays clickable instead of erroring.
- Toggling Crossfade while the next track is already prepared no longer causes a silent fade or a stuck "stalled — recovering" loop.

## v0.12.1 — 2026-06-30

### Fixed

- The auto-hide view-header toolbar now collapses when nokkvi loses focus, instead of staying stuck expanded behind another window or re-expanding on return.
- Pressing Shift+C to centre the playing track no longer expands the auto-hide toolbar.

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
