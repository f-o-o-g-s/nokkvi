# Changelog — v0.11.x archive

Releases v0.11.0–v0.11.3, covering 2026-06-22 → 2026-06-25. The current changelog (v0.12.0 onward) lives in [CHANGELOG.md](../CHANGELOG.md).

## v0.11.3 — 2026-06-25

### Fixed

- Dragging to reorder the queue now lands the row where you drop it, instead of one slot off or somewhere unexpected.
- Dropping a dragged queue row into the empty space below the last track now appends it to the end instead of snapping back.
- With the auto-hide toolbar collapsed, "Go to Album" now lands the target row at the top of the list instead of one row too low.

## v0.11.2 — 2026-06-24

### Changed

- The Queue sort dropdown now shows **Unsorted** until you apply a sort, instead of a stale mode the queue isn't actually in.

### Fixed

- The over-cover visualizer (Bars, Lines, and Scope) now freezes in place when paused instead of disappearing, like the bottom-band placement already did.

## v0.11.1 — 2026-06-23

### Added

- New **Shuffle Play**: right-click any album, artist, genre, playlist, song, or multi-selection — or press **Ctrl+Enter** — to replace the queue with those tracks in a fresh random order, leaving the player-bar shuffle mode untouched.
- New **Shuffle Play on Enter** setting (Settings → General → Behavior): when on, pressing Enter or clicking a collection plays it in a fresh random order instead of list order.

### Changed

- Selected library rows now show an accent border ring instead of a solid fill highlight, matching the theme picker's selection style.
- New installs now default the visualizer to 40% of the window height, up from 25%.

### Fixed

- Playing a filtered Songs view no longer appends tracks from outside the filter as the rest of the queue loads in the background.

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
