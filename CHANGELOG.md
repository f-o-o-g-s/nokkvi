# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- Multi-library filter — a new nav-bar popover (top-nav layout) lets users
  scope every browse view (Albums, Artists, Songs, Genres) to a subset of
  Navidrome libraries. Empty selection is treated as "all". The trigger is
  hidden when only one library exists, so single-library servers see no UI
  change. Selection persists across restarts (redb), and libraries deleted
  on the server are pruned from the active set at next launch. Playlists
  are intentionally not filtered — Navidrome's `/api/playlist` endpoint
  ignores `library_id` and the server's per-user library access already
  filters playlists.

### Changed

### Fixed

### Removed

## v0.5.0 — 2026-05-21

### Added

- New `nokkvi <verb>` CLI for scripting and WM hotkeys — 16 verbs covering transport, volume, queue, view-switching, and `love`/`rate` on the currently-playing track.

### Fixed

- Lock, heart, and star outline icons in slot-list rows now darken in lockstep with the row's text when the row is selected (centered, multi-selected, or currently playing) — previously they kept their muted light tint against the light selected-row fill and were hard to read. Most visible on private playlists in the Playlists view, but the heart and star outlines had the same issue under multi-selection (ctrl-click) on every slot-list view.
- Menu text no longer renders blank on systems whose sans-serif font has no Medium weight (e.g. Sway + Intel iGPU; reported by hollisticated-horse).

## Older releases

- **v0.4.x** (2026-05-16 → 2026-05-19, v0.4.0–v0.4.2): [CHANGELOG-0.4.md](./CHANGELOG-0.4.md)
- **v0.3.x** (2026-04-27 → 2026-05-14, v0.3.1–v0.3.17): [CHANGELOG-0.3.md](./CHANGELOG-0.3.md)
