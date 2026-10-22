# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- On Wayland, a TV remote's OK button now plays or opens the focused item, so a G20S-style air mouse can drive the lists.
- A TV remote's Back button now backs out of popups, menus, searches and Theater Mode.
- The media keys now play, pause, stop and skip tracks when your desktop passes them to nokkvi.
- The media keys work even while a popup like the equalizer is open.
- A remote's Home button now opens Harbour, on desktops that don't keep it for themselves.
- Remote and media keys such as Search, Rewind and Volume Up can now be bound to actions.
- A remote button with no key name can now be bound by its key number, shown as "Key 582".
- A remote's Back button now highlights the view tabs when there's nothing left to back out of.
- On desktops that pass it through, a remote's Menu button also highlights the view tabs.
- While the tabs are highlighted, the arrows move the highlight along them.
- Enter opens the highlighted tab.
- Escape, or the arrow toward the list, takes the highlight back down to the list.

### Changed

- The bare Up and Down arrows now move through lists outside Settings, except while typing in a search.
- Holding Enter or Escape now acts once instead of repeating.

### Fixed

- A saved session the server no longer accepts now opens the login screen at startup, instead of the home screen.
- Restarting nokkvi after a password change no longer triggers Navidrome's failed-login block.
- Logging in during Navidrome's short failed-login block now waits it out instead of sending you back to the login screen.
- While it waits, the login form says Navidrome is pausing sign-ins.
- Hitting Navidrome's sign-in limit now says "Too many sign-in attempts" instead of showing a 429 status.

### Removed

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

## Older releases

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
