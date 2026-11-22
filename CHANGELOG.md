# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- Hotkeys can use Super: press it while rebinding in Settings → Hotkeys, or write `Super + J` in config.toml.
- Settings → Playback → Discord Status shows what you're listening to on your Discord profile, in the Discord app or Vesktop / Equibop.
- The Discord status shows the album cover when your Navidrome is reachable from the internet, else nokkvi's icon.

### Changed

- On French AZERTY, the 6 key switches to Playlists instead of lowering the rating; numpad minus still lowers it.
- Hotkeys you rebound on AZERTY, Czech or a non-Latin layout may sit on a different key: check them in Settings → Hotkeys.

### Fixed

- nokkvi builds on Rust 1.95, the oldest version it supports.
- With Accent From Album Art on, a black, white or grey cover keeps the active lyric line on the theme's accent in every shipped theme.
- With the visualizer off, seeking while paused moves the lyrics to the new line instead of waiting until playback resumes.
- When its config and state folders are the same folder, nokkvi keeps its log file instead of deleting it at every launch.
- A Super chord no hotkey uses no longer fires the plain key's hotkey: Super+X used to toggle Random.
- Letter hotkeys work on non-Latin keyboard layouts such as Russian, Greek, Hebrew, Arabic or Thai: each key fires its US position's hotkey.
- On French AZERTY, Czech and other layouts whose number row types symbols, the number keys switch views.
- With Bit-Perfect Output on, a song at a different sample rate plays at your volume instead of full volume.
- When a radio station sends an empty track title, desktop media controls show the station's name instead of a blank title.
- While a radio station plays, desktop media controls and `nokkvi status` no longer report the length of the queue's current song.

### Removed

## v0.31.0 — 2026-11-18

### Added

- Harbour's new Instant Mix row keeps the playing song and replaces the rest of the queue with songs like it.
- With nothing playing, Instant Mix plays songs like up to five songs Harbour picks from your recent, most played or random tracks.
- Shift+A on the Instant Mix row adds the mix to the queue and skips songs already in it.
- Instant Mix leaves out tracks under a minute and songs rated one star.
- Autoplay, a new playback mode, adds 10 songs like your last few when at most one song is left after the playing one.
- Autoplay toggles from its infinity button in the player bar, the modes menu, the `a` key, `nokkvi autoplay` or Settings → Playback.
- Songs Autoplay added show a small infinity sign before their artist in the Queue.
- Songs you add to the queue go ahead of the Autoplay songs that have not played yet, while shuffle is off.
- Turning Autoplay off removes the songs it added that have not played yet.
- When it finds no similar, same-genre or same-artist songs, Autoplay adds random songs from your library instead.
- Autoplay waits while Repeat is on, and its button's tooltip says so.
- If the queue runs out before a batch arrives, Autoplay plays the batch's first song when it lands.

### Changed

- Most of the GPU memory the Bars, Lines or Scope visualizer takes in fullscreen Theater Mode is now freed once it is off screen.
- The Albums view now uses about half the CPU while a song plays.
- The Artists, Genres and Playlists views now use less CPU while a song plays.
- The night Trawl scene's anchor is now outlined and shaded like the boat instead of a flat shape.

## Older releases

- **v0.30.x** (2026-11-12 → 2026-11-13, v0.30.0–v0.30.1): [CHANGELOG-0.30.md](./changelog-archive/CHANGELOG-0.30.md)
- **v0.29.x** (2026-11-10, v0.29.0): [CHANGELOG-0.29.md](./changelog-archive/CHANGELOG-0.29.md)
- **v0.28.x** (2026-10-22 → 2026-11-03, v0.28.0–v0.28.2): [CHANGELOG-0.28.md](./changelog-archive/CHANGELOG-0.28.md)
- **v0.27.x** (2026-10-17 → 2026-10-20, v0.27.0–v0.27.2): [CHANGELOG-0.27.md](./changelog-archive/CHANGELOG-0.27.md)
- **v0.26.x** (2026-10-14 → 2026-10-15, v0.26.0–v0.26.1): [CHANGELOG-0.26.md](./changelog-archive/CHANGELOG-0.26.md)
- **v0.25.x** (2026-10-12, v0.25.0): [CHANGELOG-0.25.md](./changelog-archive/CHANGELOG-0.25.md)
- **v0.24.x** (2026-10-11 → 2026-10-12, v0.24.0–v0.24.1): [CHANGELOG-0.24.md](./changelog-archive/CHANGELOG-0.24.md)
- **v0.23.x** (2026-10-09, v0.23.0): [CHANGELOG-0.23.md](./changelog-archive/CHANGELOG-0.23.md)
- **v0.22.x** (2026-10-04 → 2026-10-07, v0.22.0–v0.22.2): [CHANGELOG-0.22.md](./changelog-archive/CHANGELOG-0.22.md)
- **v0.21.x** (2026-09-25 → 2026-10-02, v0.21.0–v0.21.2): [CHANGELOG-0.21.md](./changelog-archive/CHANGELOG-0.21.md)
- **v0.20.x** (2026-09-23, v0.20.0): [CHANGELOG-0.20.md](./changelog-archive/CHANGELOG-0.20.md)
- **v0.19.x** (2026-09-20 → 2026-09-21, v0.19.0–v0.19.1): [CHANGELOG-0.19.md](./changelog-archive/CHANGELOG-0.19.md)
- **v0.18.x** (2026-07-19 → 2026-07-25, v0.18.0–v0.18.4): [CHANGELOG-0.18.md](./changelog-archive/CHANGELOG-0.18.md)
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
