# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

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

### Changed

### Fixed

### Removed

## v0.16.0 — 2026-07-15

### Added

- **Queue sync with your Navidrome server.** A new sync button in the Queue header opens a menu to push the current queue to the server, or pull it back on any device (or a fresh session), including the playing track and its exact position. Each row names what it replaces ("Replaces the queue saved on the server" / "Replaces your local queue") so the direction is unmistakable before you commit. Pulling cues the restored queue without starting playback: if you were listening, it resumes mid-song right where the other device left off, and if you were paused, pressing Play picks up from the saved spot. Duplicate tracks survive the round trip (the sync rides the OpenSubsonic indexBasedQueue extension, so the playing slot is a position, not a track id), and huge queues are fine since saves go as POST form bodies. The button appears once the server advertises the extension (Navidrome 0.58.5 and newer); `nokkvi queue-push` and `nokkvi queue-pull` do the same from the command line.
- Trawl's rating filter gains "Unrated only", for mixes built purely from songs you haven't rated yet.

### Changed

- The default start view is now Harbour: new installs open on the home shelves, and users still on the old default move there too.

### Fixed

- Hovering the Queue's "Playing From" banner for a description-less playlist no longer shows a blank gap; the stats row now sits flush under the title.

## Older releases

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
