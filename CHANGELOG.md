# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- **The Trawl scene's seabed comes alive.** A small school of fish drifts through the mid-water (with a moonlit rim so it reads at night), kelp beds sway along the floor — clusters on the flanks plus a few shorter loners the anchor drags past — and the trawl aerates the bed: a stream of bubbles rises from the dragged anchor, the bigger ones as rings, with slow seeps rising from the kelp roots. Rock mounds, a resting starfish, and a sunken slatted cargo crate settled at a tilt on the right bed dress the bottom, moonlit at night. Cold starlight and seafoam by night, ink by day; the lantern stays the scene's only warm light.
- **Moonbeams in the Trawl night, and rare visitors below.** Three whisper-quiet shafts of starlight fan from the moon down through the night water — you notice the water feels lit rather than seeing rays — and once in a while a long serpent glides through the deep beneath the longship: three tail-beats, then gone.
- **The night sky's rarest event: a black hole.** Every once in a long while an invisible gravity well opens in the night sky — nothing is drawn for it, because a black hole isn't really visible. You know it's there because the stars near it start to fall: accelerating spirals winding tighter, each star shrinking and dimming to nothing as it crosses the event horizon (the light can't escape), the survivors orbiting the darkness — until the well lets go and spits everything back out, stars re-lighting as they sail past their homes and settle back into place. Distant stars never stir; the moon just watches; shooting stars sit those cycles out.
- **The Trawl scene's day finally gets its light.** The waterline's catch-light and traveling shimmer now gild in sun gold by day (they previously drew in starlight, invisible on a light sea), sparse glitter flashes ride the water under the sun, and some cycles a tiny distant sail crosses the far swell — day's answer to the night's shooting star.
- **The moon dreams.** The Trawl scene's moon (and its day sun) now rests as a plain disc. Once when the app opens, and rarely after that, it slips into a short ritual: four verses in carved staves drift through the sky, and with each verse a mark of a face appears on the disc — the grin first — until the face is whole; then it lets go again, the grin lingering last, and the plain moon sails on until the next dream. What the staves say is left to whoever cares to read them. The black hole, the shooting star, and the wandering notes sit those cycles out.

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
