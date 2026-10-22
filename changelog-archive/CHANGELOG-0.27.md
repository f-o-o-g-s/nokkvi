# Changelog — v0.27.x archive

Releases v0.27.0–v0.27.2, covering 2026-10-17 → 2026-10-20. The current changelog (v0.28.0 onward) lives in [CHANGELOG.md](../CHANGELOG.md).

## v0.27.2 — 2026-10-20

### Added

- Similar artists now include Deezer's related artists alongside ListenBrainz's.
- Artists with no MusicBrainz id can now get similar artists from Deezer.
- Among similar artists you have, and among those you don't, ones both ListenBrainz and Deezer list come first.
- Similar artists you don't have that only Deezer lists open their Deezer page.
- A new General setting, Similar Artists: Deezer, turns Deezer off so nothing is sent to Deezer.

### Changed

- Similar artists you don't have now come first in the Artists panel and the Similar Artists popup.
- Turning off Similar Artists: ListenBrainz now drops only ListenBrainz's picks, not the whole similar artists list.
- Hovering a similar artist in the Artists panel now also says whether ListenBrainz, Deezer or both listed them.
- Each Similar Artists popup row now says whether ListenBrainz, Deezer or both listed that artist.
- Changing a Similar Artists setting now refreshes the similar artists already on screen.

## v0.27.1 — 2026-10-19

### Added

- Settings > General > Track Artwork can show each song's own embedded cover instead of its album's cover.

### Fixed

- With Accent From Album Art on, the current lyric line now stands out on black, white and grey covers.

## v0.27.0 — 2026-10-17

### Added

- The Artists panel lists similar artists from ListenBrainz under the artist's biography.
- In that list, artists you have show as filled tags and the rest as outlined tags.
- Clicking a similar artist you have jumps to that artist in Artists.
- Clicking a similar artist you only have on other artists' albums opens their songs in Songs.
- Clicking a similar artist you don't have opens their page on ListenBrainz.
- Hovering a similar artist in the Artists panel says whether you have them, with MusicBrainz's note when there is one.
- A queue row's right-click menu gains Similar Artists, a searchable popup of the same list for that track's artist.
- Album, song and artist rows across the library get the same Similar Artists menu entry.
- Shift+R opens Similar Artists for the highlighted row's artist.
- Pressing Shift+R again closes the Similar Artists popup.
- A new General setting, Similar Artists: ListenBrainz, switches the feature off so nothing is sent to ListenBrainz.

### Fixed

- After a session ends, `nokkvi status` reports stopped instead of a frozen playing track.
- After a session ends, MPRIS reports stopped instead of a frozen playing track.
- After a session ends, the tray reports stopped instead of a frozen playing track.
- Live library updates keep arriving after two days of uptime.
- Logging in no longer leaves a missing-parameter warning in Navidrome's log.
- A rejected server credential no longer marks every cover as missing artwork for the rest of the session.
- Requests to a server that has gone away fail after eight seconds instead of about two minutes.
- A search or sort typed while a background reload is out no longer gets replaced by the reload's unfiltered rows.
- Scrolling a browsing tab with the mouse now fetches one full-size cover at a time, like the main view.
- Turning Lyrics or Cover Blur off releases the frosted cover from GPU memory.
- The library picker now follows libraries an admin adds, removes or grants, without a rescan or re-login.
- The clear-queue help line no longer claims it stops a playing radio station.
- The library selection now belongs to the server and user it was made on, like the queue.
- A queue-replacing play that fails while a station streams hands radio mode back from every view, not only from Albums.
- The love and rating keys act on the highlighted row, not the row at the list's center.
- Get Info (Shift+I) shows the highlighted row's details, not those of the row at the list's center.
- Ctrl+D removes the highlighted queue row, not the row at the list's center.
- Shift+arrow moves the highlighted queue row, not the row at the list's center.
- `nokkvi selection` and the `nokkvi add-to-queue` reply name the highlighted row.
- Play with nothing loaded starts the highlighted queue row, as Enter does.
- The Songs artwork panel shows the highlighted song's cover, not the centered one's.
- The Similar tab's artwork panel shows the highlighted song's cover, not the centered one's.
- After a jump to an artist, the highlight and artwork panel stay on that artist instead of sliding to the middle row.
