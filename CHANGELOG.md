# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- Smart playlists are now recognized: a sparkles badge marks them in the Playlists view.
- Expanded playlist tracks gained a right-click menu: play, queue/mix/playlist adds, Get Info, Remove from Playlist.
- Shift+Up/Down reorders tracks inside the playlist editor; changes stay staged until Save.
- Create-playlist dialogs warn when the name already exists, without blocking.
- Get Info now works inside the playlist editor.
- Smart playlists can now be created and edited in-app: a rules editor with live validation.
- The Playlists header's + becomes a create menu (regular or smart) on capable servers.
- Edit Rules… on owned smart rows; the queue banner pencil routes smart playlists there too.
- New hotkeys: Shift+N opens a new smart playlist; `e` edits the centered playlist.
- Rules editing includes seeded presets, a raw-JSON mode, and an honest evaluated-at freshness stamp.
- Rule previews are server-evaluated through a private draft, fresh on every press, before anything saves.
- Enter on a preview row plays it — tweak rules and hear matches in one loop.
- The rules preview gained a columns cog: show rating, love, plays, genre, and duration alongside each match.
- Stray preview drafts clean themselves up at login; other clients see them clearly labeled meanwhile.
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

### Removed

## v0.17.0 — 2026-07-18

### Added

- Synced lyrics now display over the Queue cover art, following the playing track line by line.
- A Lyrics toggle joins the player-bar modes (hotkey `L`), with a matching Settings row.
- Lyrics resolve from a local store, then your server's OpenSubsonic lyrics, then LRCLIB.
- Fetched lyrics cache into `~/.local/share/nokkvi/lyrics/` and resolve offline afterwards.
- An online-fetch setting gates the LRCLIB channel for privacy.
- The lyric column glides between lines with eased motion, snapping on seeks.
- With crossfade on, the old track's lyrics dissolve out as the next track's fade in.
- The over-cover visualizer keeps playing underneath the lyrics instead of yielding to them.
- A Lyrics: Cover Blur setting frosts the cover behind the lyrics, Off through Heavy.

- A small school of fish now drifts through the mid-water of the Trawl scene, above the seabed and below the waterline.
- Kelp beds now sway along the Trawl seabed, taller clusters on each flank and shorter loners toward the middle.
- A stream of bubbles now rises from the dragged anchor, aerating the bed as the trawl works across the floor.
- Rock mounds and a resting starfish now settle on the Trawl scene's seabed floor.
- A slatted cargo crate now lies tilted on the Trawl scene's right seabed.
- The Trawl scene's night water now feels lit by the moon.
- Once in a while a long inked serpent glides across the Trawl scene's deep water, then vanishes.
- The Trawl scene's daytime waterline catch-light and shimmer now gild in sun gold, recolored from the starlight they drew at night.
- Sparse glitter now flashes on the Trawl scene's daytime water beneath the sun.
- Some cycles a tiny hazed sail now crosses the far swell behind the longship by day, bobbing with the distant water.
- Rarely the Trawl night sky opens an invisible black hole, and nearby stars spiral in, shrink to nothing, then get spat back into place.
- The Trawl scene's moon now rests as a bare disc, faceless between dreams.
- Once at every launch, and rarely after, the Trawl scene's sky slips into a short ritual that calls a face onto its disc.
- The face builds mark by mark as each carved verse arrives, the grin appearing first.
- When the dream ends the marks leave in reverse, the strap first and the grin lingering last before the disc is bare again.
- The moon dream's four carved verses ship untranslated, left to whoever cares to read them.

## Older releases

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
