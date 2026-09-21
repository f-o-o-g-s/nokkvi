# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

### Changed

- The Bars beat flash now spreads across neighboring bars instead of flickering on a single bar.
- Bars no longer sparkle with random flashes between beats.

### Fixed

### Removed

## v0.19.0 — 2026-09-20

### Added

- Nix users can now build and run nokkvi straight from the repository with its new flake.
- Smart playlist rules and sorting gain five album-level fields (date added, date modified, duration, song count, size) on Navidrome 0.64+.
- The smart-playlist editor now warns when a sort field needs a newer Navidrome than the server runs.
- Smart playlists can set their own refresh delay (`1d`, `12h`, `1w`) in the rules editor on Navidrome 0.64+.
- Album and artist thumbnails show the cover's dominant color while they load (Navidrome 0.64+).
- The playlist editor can remove duplicate songs in one click, from the edit bar or a row's right-click menu.
- The queue's right-click menu can remove duplicate songs without interrupting playback.
- Adding songs a playlist already has now asks whether to skip them.
- Plain (untimed) lyrics from your Navidrome server, including lyrics embedded in a file's tags, now show over the Queue cover.
- Plain lyrics drift with playback so the sheet keeps pace with the song.
- The mouse wheel over the Queue cover scrolls a plain lyrics sheet by hand.
- Seek Backward and Seek Forward hotkeys, on Left and Right by default.
- Holding a seek key keeps the playhead moving until you let go.
- A Seek Step setting, 1 to 60 seconds, in General > Behavior.
- `nokkvi seek +10` and `seek -10` seek relative to the current position.

### Changed

- The About modal now credits Claude Opus 5 and Fable 5.1 as the shipwrights (previously Opus 4.8).
- The sort-mode cycle moved from Left and Right to Shift+Left and Shift+Right.
- `nokkvi --help` now lists `queue-push` and `queue-pull` and shows `volume`'s relative form.
- Keyboard scrolling, wheel scrolling, and clicks in a very large queue now respond faster.
- Dropping thousands of selected rows in a very large queue now lands without a pause.
- Dropping thousands of selected rows in the playlist editor now lands without a pause.
- Removing many queue rows at once no longer stalls playback.
- The add-to-playlist confirmation now says how many songs were added.
- On Navidrome 0.64+, albums and artists with no artwork show a plain square instead of Navidrome's placeholder picture.
- On Navidrome 0.64+, a replaced album, artist, or playlist cover shows the next time its row comes into view.
- On Navidrome 0.64+, the large artwork panel also switches to a replaced cover without a manual refresh.
- Your Navidrome server's lyrics now win over LRCLIB downloads, including ones already cached.

### Fixed

- Expanding a genre in the Genres view now lists its albums on Navidrome 0.64.
- Clicking a genre's song count now opens the Songs view with that genre's songs on Navidrome 0.64.
- Harbour's Random Genre row now shows its album covers on Navidrome 0.64.
- Harbour's Most Played Genres rows now show their album covers on Navidrome 0.64.
- Genres in Harbour search results now show their album covers on Navidrome 0.64.
- Centering a genre in Harbour now shows its large collage on Navidrome 0.64.
- CRC-protected MP3s now play gaplessly between album tracks.
- Some CRC-protected VBR MP3s no longer stop playing before their real end.
- MP3s carrying LAME gapless info no longer lose a few milliseconds of audio at the end.
- Ogg Vorbis and Opus radio stations no longer cut out and reconnect at each song change.
- Radio stations that are slow to send their first audio no longer fail to start on the first try.
- Dragging a row in a very large queue no longer freezes the window.
- Holding a dragged queue row still at the list's edge now loads thumbnails for the rows it scrolls into view.
- Holding a dragged playlist-editor row still at the list's edge now loads thumbnails for the rows it scrolls into view.
- Dragging a queue row now picks up and drops the row under the cursor with the playlist banner, select column, or browsing panel showing.
- Clicking an album, artist, or genre link now lands the found row in view when the target view's select column is on.
- With Fade on Skip set to Crossfade, playing a list larger than one page while a song plays now keeps the track you clicked.
- Editing the queue or toggling shuffle, repeat, or consume during a skip crossfade no longer brings the previous track back.
- Seeking during a skip crossfade now seeks the new track instead of bringing the previous one back.
- Stopping during a skip crossfade, then pressing Play, now starts the new track instead of the previous one.
- With ReplayGain on, seeking a track that arrived by crossfade no longer plays it at an earlier track's level.
- With ReplayGain on, stopping and replaying a track that arrived by crossfade no longer plays it at an earlier track's level.
- Switching bit-perfect mode during a crossfade no longer leaves the badge claiming bit-perfect for a track that isn't.
- Seeking during an automatic crossfade no longer makes the next song start without a crossfade.
- With Fade on Skip set to Crossfade, the progress bar now shows the new track's time as soon as you skip.
- With Fade on Skip set to Crossfade, MPRIS clients now see the new track's position and length as soon as you skip.
- With Fade on Skip set to Crossfade, synced lyrics now follow the new track's time as soon as you skip.
- With Fade on Skip set to Crossfade, a seek sent right after skipping now starts the new track at that point.
- Desktop media widgets no longer keep the last song's cover while a radio station without a logo or stream art plays.
- Radio stations with an uploaded logo now show it in desktop media widgets.
- Desktop media widgets now show the nokkvi icon when a song's cover fails to load.
- Scrolling Genres or Playlists no longer re-downloads thumbnails that are already loaded.
- Searching the queue or radio stations after clicking a row no longer leaves the results without a highlighted row.
- The quick-add confirmation no longer ends with a stray apostrophe.
- A `.lrc` you add to the lyrics folder now beats a cached LRCLIB copy of the same song.
- Lyrics added by a library rescan now show up without restarting nokkvi.
- With crossfade on, skipping during a track's intro now fades the sheet you were reading.
- Two actions sharing one key now resolve the same way on every launch.
- Rapid relative seeks from media keys and `playerctl` add up instead of landing as one.
- A seek no longer credits listening time for the part of a track it skipped over.
- Turning lyrics off mid-crossfade no longer brings the previous track's sheet back when you turn them on.
- Removing the playing queue row while a radio station plays no longer replaces the station with a queue song.
- Removing every queue row while a radio station plays no longer stops the station.
- Flipping the radio sort order after clicking a station no longer leaves the highlight on the wrong station.
- Changing the sort mode in Albums, Artists, or Songs no longer leaves the highlight on an off-screen row.
- Settings > Hotkeys now marks a row whose key another action wins, and names that action.
- Rebinding a row onto the key it already shares now moves the other action to its own default instead of doing nothing.
- A rebind that cannot be settled by moving one action now says so instead of reporting a swap that never happened.
- The smart-playlist refresh-delay hint now says a `0` delay also means the server default.

## Older releases

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
