# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- The Lines visualizer can now glow like neon, with a halo that brightens with the music and flares on each beat, plus an adjustable Glow Intensity setting.
- The Bars visualizer now blooms toward the peak color when bars hit a beat, with an adjustable Peak Flash setting.
- The visualizer now has a bloom glow: bright bars, peak flashes, and the neon line bleed a soft halo that flares on each beat, with a Bloom toggle and intensity setting.

### Changed

- The metadata strip now defaults to Mini Player instead of the Player Bar.
- The sort and search toolbar now auto-hides by default, collapsing to a count strip.
- The visualizer's bars now default to a solid fill instead of segmented LED bars.
- The visualizer's surfing boat (Lines mode) is now enabled by default.

### Fixed

- The now-playing row's breathing glow and sheen now appear on the first track of a freshly played queue, not only after skipping ahead.
- Certain MP3s no longer display a wildly wrong duration and bitrate (such as 30:24 for a 4:44 track).
- Seeking in those affected MP3s now lands at the chosen position instead of far earlier in the track.
- Seeking near the end of a track no longer makes the crossfaded-in next song get skipped moments after it starts.

### Removed

## v0.7.2 — 2026-06-10

### Added

- Turning off Verbose Config now trims config.toml down to only your non-default settings and view sorts.
- Playlist and genre rows, plus the queue's "Playing From" strip, now show a 2x2 collage of up to four album covers.

### Changed

- Playing an empty album, artist, genre, or playlist now reports which kind of item was empty instead of a generic message.
- The Settings view rebuilds its entries only when a setting changes, instead of re-reading config files every frame.

### Fixed

- Restoring the metadata strip's Field Separator to its default now gives Slash, not a middle dot.
- Album and song lists no longer draw the date, year, duration, or genre sort column larger than the rest of the row.

### Removed

- The Theme tab no longer offers the background "level4" and foreground "gray" colors, which never affected any rendered surface.

## v0.7.1 — 2026-06-07

### Added

- Settings search maps synonyms, so terms like "loudness", "systray", or "dark mode" jump to the matching setting.
- Hotkey settings are searchable by their key binding: type "ctrl" or "space" to find the shortcuts bound to them.

### Changed

- Settings search is now fuzzy and typo-tolerant, ranks results by relevance, and highlights matched characters.

### Fixed

- High-sample-rate tracks (96 kHz and up) no longer stutter or repeatedly pause mid-playback.

## v0.7.0 — 2026-06-07

### Added

- New `nokkvi status` command prints the current playback state, track, volume, and shuffle/repeat/consume modes as JSON.
- New `nav-up`, `nav-down`, `enter`, and `selection` CLI commands move the slot-list selection, activate the centered item, and read it.
- New `nokkvi add-to-queue` command enqueues the focused list item (the Shift+A hotkey) and reports what it added.
- New `nokkvi remove-from-queue` command removes the centered queue song (the Ctrl+D hotkey) and reports what it removed.
- MiniPlayer mode gains a "Visible Controls" setting to hide its volume slider and mode menu independently.
- New "Auto-hide Toolbar" setting collapses the sort and search bar to give the list more room, revealing it on hover or a sort/search shortcut.
- The auto-hide toolbar's "Collapsed appearance" shows a thin hairline, nothing, or a slim strip with the current sort, item count, and duration.

### Changed

- The `nokkvi` command-line verbs now print a JSON result instead of nothing, so toggles like `nokkvi consume` confirm their new state in the shell.
- `nokkvi rate` and `nokkvi love` now show an in-window toast confirming the change, like the in-app rating and star hotkeys.
- MiniPlayer mode is redesigned with a full-width progress bar across the top, showing elapsed and total time alongside the track's format and bitrate.
- On wider windows, MiniPlayer mode now centers the transport controls, with metadata on the left and volume and mode controls on the right.
- The player bar now uses a 3-button transport set (previous / play-pause / next) in every layout.

### Fixed

- Pressing Previous while Shuffle and Consume are both on is now blocked with an explanatory toast, instead of stranding an unplayed track.

### Removed

- The dedicated Stop button is gone from the player bar, but Stop remains available via media keys and the `nokkvi stop` command.

## Older releases

- **v0.6.x** (2026-05-25 → 2026-06-06, v0.6.0–v0.6.10): [CHANGELOG-0.6.md](./changelog-archive/CHANGELOG-0.6.md)
- **v0.5.x** (2026-05-21 → 2026-05-24, v0.5.0–v0.5.3): [CHANGELOG-0.5.md](./changelog-archive/CHANGELOG-0.5.md)
- **v0.4.x** (2026-05-16 → 2026-05-19, v0.4.0–v0.4.2): [CHANGELOG-0.4.md](./changelog-archive/CHANGELOG-0.4.md)
- **v0.3.x** (2026-04-27 → 2026-05-14, v0.3.1–v0.3.17): [CHANGELOG-0.3.md](./changelog-archive/CHANGELOG-0.3.md)
