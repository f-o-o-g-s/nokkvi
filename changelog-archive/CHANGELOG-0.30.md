# Changelog — v0.30.x archive

Releases v0.30.0–v0.30.1, covering 2026-11-12 → 2026-11-13. The current changelog (v0.31.0 onward) lives in [CHANGELOG.md](../CHANGELOG.md).

## v0.30.1 — 2026-11-13

### Changed

- The About modal now credits Fable 5.2 instead of Fable 5.1 among the shipwrights.
- Opening the Playlists view no longer holds hundreds of megabytes of memory on a library with very large playlists.
- Full-size covers kept in memory for stepping back are now limited to about 64 MB.
- Holding a navigation key now skips the full-size covers that could not arrive before their row scrolls past.
- nokkvi now hands freed memory back to the system every 30 seconds.

## v0.30.0 — 2026-11-12

### Added

- An expanded artist's albums are grouped under release-type headers: Albums, EPs, Singles, Live, Compilations and more, with Appears On last.
- An artist with only plain albums keeps the flat album list.
- Enter on a release-type header plays that whole section in list order, or adds it from the split view's library pane.
- Shift+Enter on a release-type header, or a click on its chevron, folds or unfolds the section.
- Folded sections stay folded for every artist until nokkvi quits.
- A release-type header's right-click menu shuffles its section, or adds it to the queue, a playlist or a mix.
- A selected release-type header stands for all of its albums in batch actions.
- Album rows under a release-type header carry small tags for what the header leaves out, like Live on an EP.
- Album rows under a release-type header also tag the album version, like Deluxe Edition, and mark explicit albums with an E.
- Albums under Appears On name their own artist.
- Settings → General has Group Artist Albums by Release Type, on by default.
- With Group Artist Albums by Release Type off, an expanded artist's albums show as one list, newest first.
- Get Info on an album shows its version, like Deluxe Edition, and its record label when the tags carry them.
- Playlist rows have a clickable heart and star rating on Navidrome 0.64 or newer, like album rows.
- The star and rating keys work on a focused playlist row.
- The Playlists view sorts by Favorited and by Rating.
- The Playlists columns menu has Stars and Love toggles; Stars is off by default and shows under a Rating sort.
- The playlist artwork panel shows Favorited and the rating.
- The Albums artwork panel shows the centred album's notes under its stats when Navidrome has any.
- A Read more link under an album's notes opens the album's page, usually on Last.fm.
- Get Info on an album lists its notes and external link when Navidrome has them.

### Fixed

- `nokkvi love` and `nokkvi rate` now refuse during radio playback instead of starring or rating the last queue song.
- With crossfade off, seeking to a song's last second while paused no longer starts the next song playing.
- Clicking a song just as the playing one changes no longer leaves the queue one song past the click.
- Stop pressed just as a song changes gaplessly no longer starts the next song.
- Dragging a row to the queue from under an expanded album, artist or genre now shows that row on the drag label.
- A slow album or track load no longer re-opens a row you already closed, or replaces the row you opened after it.
- Switching between Albums and the Queue no longer downloads the same album thumbnails again each time.
- The Playlists list no longer jumps back to the top when a playlist changes on the server.
- The Albums artwork panel no longer repeats the year, like "2003 (2003)", for an album with no original date.
- The Albums artwork panel no longer reads "0 (2021)" for an album tagged with only a year.
- Launching nokkvi no longer asks the server, ListenBrainz and Deezer about the first artist in the Artists list before that view is opened.
- A roulette spin in Artists no longer looks up the artists it passes when the list reloads mid-spin.
- The Albums, Artists, Songs and Playlists lists no longer blink to "Loading..." when the server reports a change, like a scrobble.
- Scrolling to the end of the loaded rows no longer blanks the list while the next page loads.
- The big cover no longer goes blank for an instant between the thumbnail and the full-size picture when you land on an album or artist.
