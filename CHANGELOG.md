# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

### Changed

### Fixed

### Removed

## v0.26.1 — 2026-10-15

### Added

- A new General setting, Expandable Harbour Shelves, brings back Harbour shelves that open to browse their picks.

### Changed

- Escape in Settings now steps from the rows back to the categories before it leaves Settings.
- Pressing a Harbour shelf now plays it instead of opening it.
- The Recently Added, Most Played Album and Most Played Artist shelves each play their top pick.
- The Most Played Genre shelf plays a random mix of songs from your top genre.
- The Recently Played and Most Played Tracks shelves play up to 100 of their songs.
- The Recently Played and Most Played Tracks shelves name how many songs a press plays.

### Fixed

- Arrow keys, Enter and Escape alone now reach every Settings category.
- Harbour's Recently Played shelf no longer lists songs you have never played.

## v0.26.0 — 2026-10-14

### Added

- The Lyrics setting gains Auto, which shows lyrics over the Queue cover and in Theater Mode only for songs that have them.
- A new Playback setting, Lyrics: Alignment, sets lyric lines against the left edge, centered, or against the right edge.
- While covers load on Navidrome 0.64 or newer, album and artist rows show a blurred preview of each cover instead of a flat square.
- Song rows show the same blurred preview when nokkvi has already loaded the song's album, such as in the Albums view.
- The big cover panel shows the blurred preview, instead of staying blank, until a thumbnail or the full cover arrives.

### Changed

- Opening the playlist editor now plays the view-change sound, like other view switches.
- Find Similar from another view now plays the view-change sound.
- A song the server transcodes on the fly now opens with two requests to the server instead of four.
- A song the server serves as a file now opens with one request fewer to the server.
- Lyrics are now centered on the Queue cover and in Theater Mode instead of starting at the left edge.
- Scrolling fast through a list now downloads one full-size cover for the big panel at a time, not one for every row you pass.

### Fixed

- With Similar results focused, the rating keys and the love key now act on the centered Similar song, not the queue's.
- With Similar results focused, `nokkvi selection` now reports the centered Similar song instead of the queue's.
- `nokkvi selection` now reports `similar` as its view when Similar results have focus.
- With Similar results focused, `nokkvi add-to-queue` now names the Similar song it added, not the queue's.
- `nokkvi add-to-queue` now names an added playlist, genre or Harbour row instead of replying null.
- Get Info on a focused but empty Similar tab no longer opens the centered queue song's details.
- The `add-to-queue` and `remove-from-queue` commands now do nothing under the EQ, About or Get Info window, a picker or a dialog.
- Loving a song now rates it five stars only after the server accepts the love.
- Dropping songs into the playlist editor on an expired session now returns you to the login screen instead of doing nothing.
- Opening the playlist editor now stops a roulette spin running on the Playlists view.
- Clicking the Editing tab while Settings is open now opens the playlist editor.
- A scrobble the server rejects no longer bumps the song's play count in nokkvi.
- A scrobble or now-playing update rejected by an expired login now returns you to the login screen.
- A view switch made from Settings (a nav tab, a view hotkey, `nokkvi switch-view`) now opens that view, not the one you came from.
- Find Similar now stops a roulette spin running on the view it leaves.
- Find Similar now cancels a pending jump to a clicked album, artist or genre on the view it leaves.
- With Enter set to Append & Play and shuffle on, Enter while stopped or paused now plays every appended song instead of skipping some.
- A radio station started just as a gapless song change lands now shows what it is playing.
- A radio station started just as a gapless song change lands now scrobbles its songs when radio scrobbling is on.
- Enter after a search now plays the empty-list sound only when the list really shows nothing.
- Adding a Trawl mix to the queue now shows the new songs in an open Queue view right away, without switching views.
- Re-evaluating the smart playlist preview no longer re-requests covers still on their way or ones the server has no art for.
- Under the Trawl window, `nokkvi add-to-queue` now replies `Trawl mix` instead of naming the row behind the window.
- `nokkvi selection` now names a centered playlist or genre row instead of replying null.
- Adding or dropping a whole playlist or album into the queue now toasts how many songs it added, not "1 items".
- A love or rating the server refuses now shows an error toast instead of leaving only the "Starred" or "Rated" toast.
- Turning lyrics off and back on from Settings or `config.toml` no longer brings back the previous song's lyrics.
- A song without lyrics now shows the "No lyrics for this track" note over the cover instead of a blank overlay.
- Focusing a text box from the keyboard (`/`, or Tab on the login screen) now puts the cursor after its text instead of before it.
- With Cover Blur on, the cover no longer blinks blank for a frame when lyrics show or hide.
- An artist with no picture on the server but an outside poster now shows that poster as its thumbnail, as the big panel already did.
- Far down a long Albums, Artists or Songs list, the view now stays put when a scrobble or an artist refresh reloads the list.

## Older releases

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
