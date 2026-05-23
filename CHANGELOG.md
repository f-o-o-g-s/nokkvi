# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

### Changed

### Fixed

- Logging out and back in to Navidrome no longer leaks one OS thread per cycle — the visualizer's background FFT worker is now joined when the prior session's widget is released, so long-running sessions with repeated re-logins stop accumulating orphaned `visualizer-fft` threads.

### Removed

## v0.5.1 — 2026-05-22

### Added

- Multi-library filter — a new nav-bar popover (top-nav and side-nav
  layouts) lets users scope every browse view (Albums, Artists, Songs,
  Genres) to a subset of Navidrome libraries. Empty selection is treated
  as "all". The trigger is hidden when only one library exists, so
  single-library servers see no UI change. Selection persists across
  restarts (redb), and libraries deleted on the server are pruned from
  the active set at next launch. Playlists are intentionally not
  filtered — Navidrome's `/api/playlist` endpoint ignores `library_id`
  and the server's per-user library access already filters playlists.

### Changed

- Hamburger menu moved from the far-right of the top nav to the left edge, next to the library-filter trigger in both top-nav and side-nav layouts.

### Fixed

- MPRIS `LoopStatus` requests from clients like `playerctl` now set the requested mode directly instead of cycling, so `playerctl loop Track` from Playlist state no longer lands on None.
- MPRIS cover art is published on D-Bus as a local file URL instead of an authenticated Navidrome link, no longer exposing the Subsonic credential triple to other same-user processes.
- Switching Navidrome servers no longer shows the prior server's covers for overlapping album IDs, retries SSE against the old host, or emits the old server's cover via MPRIS until the next track change.
- Radios and Similar views now render the right number of rows after a window resize, matching every other slot-list view.
- Library-changed SSE events with non-ASCII metadata (artist names with diacritics, Japanese titles, …) are no longer dropped when a multi-byte character spans an HTTP chunk boundary.

## v0.5.0 — 2026-05-21

### Added

- New `nokkvi <verb>` CLI for scripting and WM hotkeys — 16 verbs covering transport, volume, queue, view-switching, and `love`/`rate` on the currently-playing track.

### Fixed

- Lock, heart, and star outline icons in slot-list rows now darken in lockstep with the row's text when the row is selected (centered, multi-selected, or currently playing) — previously they kept their muted light tint against the light selected-row fill and were hard to read. Most visible on private playlists in the Playlists view, but the heart and star outlines had the same issue under multi-selection (ctrl-click) on every slot-list view.
- Menu text no longer renders blank on systems whose sans-serif font has no Medium weight (e.g. Sway + Intel iGPU; reported by hollisticated-horse).

## Older releases

- **v0.4.x** (2026-05-16 → 2026-05-19, v0.4.0–v0.4.2): [CHANGELOG-0.4.md](./changelog-archive/CHANGELOG-0.4.md)
- **v0.3.x** (2026-04-27 → 2026-05-14, v0.3.1–v0.3.17): [CHANGELOG-0.3.md](./changelog-archive/CHANGELOG-0.3.md)
