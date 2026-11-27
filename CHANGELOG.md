# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- The equalizer has a Parametric view beside the ten sliders: up to 32 bands of peak, shelf and pass filters.
- Parametric bands are points on a response curve that can be dragged, with the wheel setting a band's width.
- Every parametric band value can be typed, with Tab and Shift+Tab moving between the fields.
- The Parametric view imports and exports Equalizer APO and AutoEq ParametricEQ.txt profiles.
- Parametric band lists can be saved as named profiles and switched from the equalizer's header.
- The Parametric view has an AutoEq search that finds your headphones and loads their profile.
- The AutoEq headphone list is downloaded from GitHub only when you open that search.

### Changed

- A paused or stopped visualizer now holds still once its bars have fallen, so an idle nokkvi uses almost no CPU.
- The Harbour sea pauses while nothing plays and the window is unfocused or untouched for a minute, and resumes where it stopped.
- While paused, long now-playing titles rest at their start instead of scrolling.
- Hovering a resting now-playing title scrolls it.
- Playing music now uses a fraction of the CPU it did.
- The Harbour scene animates at 60 fps on high-refresh displays, which cuts its CPU use while music plays.
- A boosting equalizer curve now lowers the overall level by its largest boost, so presets like Bass Boost play about 5 dB quieter.
- Changing the equalizer while music plays now fades to the new curve over 20 ms instead of switching at once.

### Fixed

- A count of one reads singular, like 1 song, 1 album or 1 play, instead of 1 songs.
- Harbour's Most Played genres read 1 of your top tracks instead of 1 of your top track.
- Leaving the Harbour view with the visualizer off now stops the background audio analysis it had started.
- Equalizer boosts no longer distort loud music.
- An equalizer setting that fails to save now says Failed to save once, not twice.
- Dragging an equalizer slider no longer shows an empty notification box above the player bar.
- Text typed into a search box after clearing it with its X button keeps the app's font.

### Removed

## v0.32.1 — 2026-11-23

### Changed

- With Rounded Corners on, list rows are rounded cards with a small gap between them, in the Queue, library views, Harbour and the playlist editor.

## v0.32.0 — 2026-11-22

### Added

- Hotkeys can use Super: press it while rebinding in Settings → Hotkeys, or write `Super + J` in config.toml.
- Settings → Playback → Discord Status shows what you're listening to on your Discord profile, in the Discord app or Vesktop / Equibop.
- The Discord status shows the album cover when your Navidrome is reachable from the internet, else nokkvi's icon.
- With a server reachable from the internet, the cover's link shows your server's address to anyone who can see your Discord status.
- Discord App ID, in the same section, shows the status under your own Discord application's name and icon instead of nokkvi's.

### Changed

- On French AZERTY, the 6 key switches to Playlists instead of lowering the rating.
- On French AZERTY, numpad minus still lowers the rating.
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
- Opening a link or folder no longer leaves a defunct `xdg-open` process behind until nokkvi quits.

## Older releases

- **v0.31.x** (2026-11-18, v0.31.0): [CHANGELOG-0.31.md](./changelog-archive/CHANGELOG-0.31.md)
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
