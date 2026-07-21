# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

### Changed

- New installs default the Verbose Config setting to Clean, so config.toml stays free of the inline comments the previous Off default injected.
- The Artists view now bolds the centered row's artist name, matching the other library views.

### Fixed

- Playback that stops because the next track fails to load now reports the error instead of stopping silently.
- An expired session during a library refresh now returns you to the login screen instead of a generic error toast.
- A server error sent in place of audio now reports the track as unavailable instead of a format failure.
- Artist names in the Artists view now stop being clickable when Slot Text Links is off.

### Removed

## v0.18.1 — 2026-07-19

### Changed

- The queue's playlist banner shows a smart-playlist indicator in place of the save button for smart playlists.

### Fixed

- Typing a capital letter in the smart-playlist editor no longer fires a global hotkey instead of inserting it.

## v0.18.0 — 2026-07-19

### Added

- Smart playlists are now recognized: a sparkles badge marks them in the Playlists view.
- Expanded playlist tracks gained a right-click menu: play, queue/mix/playlist adds, Get Info, Remove from Playlist.
- Shift+Up/Down reorders tracks inside the playlist editor.
- Edits in the playlist editor stay staged until Save.
- Create-playlist dialogs warn when the name already exists, without blocking.
- Get Info now works inside the playlist editor.
- Smart playlists can now be created and edited in-app: a rules editor with live validation.
- The Playlists header's + becomes a create menu (regular or smart) on capable servers.
- Edit Rules… appears on smart playlists you own.
- The queue banner's edit pencil opens a smart playlist's rules too.
- Shift+N opens a new smart playlist.
- `e` edits the centered playlist.
- Rules editing includes seeded presets, a raw-JSON mode, and an honest evaluated-at freshness stamp.
- Rule previews are server-evaluated through a private draft, fresh on every press, before anything saves.
- Enter on a preview row plays it — tweak rules and hear matches in one loop.
- The rules preview gained a columns cog: show stars, love, plays, genre, and duration alongside each match.
- Stray preview drafts clean themselves up at login.
- Until then, other clients see stray drafts clearly labeled.
- .nsp smart-playlist files import from the Playlists create menu, with update-or-create-new choice on name collision.
- The rules editor's empty state can load a .nsp file straight into the open session.
- The Trawl mix builder gained Save as Playlist (Shift+P): the resolved mix becomes an ordinary playlist.
- Rule values are picked, not typed: genres and ratings choose from lists, dates from a calendar.
- The playlist editor's edit bar shows the cover and sets custom art or resets it.

### Changed

- Add-to-playlist pickers, quick-add, queue overwrite, and the default-playlist choice now skip smart playlists, whose tracks the server keeps read-only.
- Creating a regular playlist drops into the editor to name it and add tracks, replacing the naming dialog.
- Deleting or renaming a file-backed playlist now explains scan resurrection and rules re-sync honestly.

### Fixed

- Renaming a smart playlist on Navidrome 0.61 no longer wipes its rules.

## Older releases

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
