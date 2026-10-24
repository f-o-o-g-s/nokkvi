# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

### Changed

### Fixed

- Under Track Artwork's Track's Own Cover, a smart playlist's rules preview now shows each song's own cover.
- Returning to the playlist editor after changing Track Artwork now loads its thumbnails, instead of blank squares until you scroll.
- Ctrl+clicking a selected row to deselect it moves the highlight onto a row still selected, so Ctrl+D or a star no longer hits it.
- With rows ticked in the checkbox column, Ctrl+D now removes those rows instead of the unmarked row in the middle of the queue.
- With a queue search active, Play now starts the highlighted song instead of the song at that position in the full queue.
- The queue's highlighted row now stays on its song when the queue reloads, for example after a consume advance.
- In split view, `nokkvi remove-from-queue` now names the queue song it removed, not the browsing panel's focused item.
- A password typed while nokkvi checks the saved session at launch is no longer cleared when the server turns that session away.
- When the server's keepalive answers with an error page, nokkvi still checks the saved sign-in before the home screen loads.
- Similar Artists opened from a song or album now asks Deezer for the artist's own name, so "feat." credits no longer come back empty.
- A similar artist sharing a name with a different library artist is no longer linked to that library artist.
- Switching libraries or a server rescan now refreshes which similar artists show as in your library.

### Removed

## v0.28.0 — 2026-10-22

### Added

- On Wayland, a TV remote's OK button now plays or opens the focused item, so a G20S-style air mouse can drive the lists.
- A TV remote's Back button now backs out of popups, menus, search boxes and Theater Mode.
- The media keys now play, pause, stop and skip tracks while nokkvi's window has focus, if your desktop doesn't keep them.
- The media keys work even while a popup like the equalizer is open.
- A remote's Home button now opens Harbour, on desktops that don't keep it for themselves.
- Remote and media keys such as Search, Rewind and Volume Up can now be bound to actions, where your desktop passes them through.
- A remote button with no key name can now be bound by its key number, shown as "Key 582".
- A new Focus Navigation hotkey, on the Menu key by default, highlights the view tabs.
- On desktops that pass it through, a remote's Menu button also highlights the view tabs.
- A remote's Back button now highlights the view tabs when there's nothing left to back out of.
- While the tabs are highlighted, the arrows move the highlight along them.
- Enter, or a remote's OK, opens the highlighted tab.
- Escape, a remote's Back, or the arrow toward the list takes the highlight back to the list.

### Changed

- The bare Up and Down arrows now move through lists outside Settings and Trawl, except while typing in a search.
- Holding Enter or Escape now acts once instead of repeating.
- A keyboard's browser Back key now does what a remote's Back button does.

### Fixed

- A saved session the server no longer accepts now goes straight to the login screen at startup, without flashing the home screen first.
- Restarting nokkvi after a password change no longer triggers Navidrome's failed-login block.
- Logging in during Navidrome's short failed-login block now waits it out instead of sending you back to the login screen.
- While it waits, the login form says Navidrome is pausing sign-ins.
- If the block outlasts the wait, the login form says to update the old password in any other app or device still using it.
- Hitting Navidrome's sign-in limit now says "Too many sign-in attempts" instead of showing a 429 status.

## Older releases

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
