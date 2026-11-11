# Changelog — v0.28.x archive

Releases v0.28.0–v0.28.2, covering 2026-10-22 → 2026-11-03. The current changelog (v0.29.0 onward) lives in [CHANGELOG.md](../CHANGELOG.md).

## v0.28.2 — 2026-11-03

### Fixed

- When lrclib.net says it is busy for a moment, nokkvi now asks again instead of giving up.
- Lyrics from lrclib.net are now found when your album is named differently there, such as a deluxe edition.
- When lrclib.net has only untimed lyrics for a song, nokkvi now shows them instead of "No lyrics for this track".
- A paused nokkvi now uses far less CPU on high refresh rate monitors while nothing on screen moves.

## v0.28.1 — 2026-10-29

### Fixed

- Under Track Artwork's Track's Own Cover, a smart playlist's rules preview now shows each song's own cover.
- Returning to the playlist editor after changing Track Artwork now loads its thumbnails, instead of blank squares until you scroll.
- Ctrl+clicking a selected row to deselect it moves the highlight onto a row still selected, so Ctrl+D or a star no longer hits it.
- With rows ticked in the checkbox column, Ctrl+D now removes those rows instead of the unmarked row in the middle of the queue.
- With a queue search active, Play now starts the highlighted song instead of the song at that position in the full queue.
- With Auto-Follow Playing off, the queue's highlighted row now stays on its song when the queue reloads, for example after another client removes rows.
- In split view, `nokkvi remove-from-queue` now names the queue song it removed, not the browsing panel's focused item.
- A password typed while nokkvi checks the saved session at launch is no longer cleared when the server turns that session away.
- When the server's keepalive answers with an error page, nokkvi still checks the saved sign-in before the home screen loads.
- Similar Artists opened from a song or album now asks Deezer for the artist's own name, so "feat." credits no longer come back empty.
- A similar artist sharing a name with a different library artist is no longer linked to that library artist.
- Switching libraries or a server rescan now refreshes which similar artists show as in your library.
- A background library refresh that lands after you scroll no longer jumps the list back to where it was when the refresh started.
- Dragging the scrollbar no longer clears rows ticked in the checkbox column.
- With an album expanded, the Albums panel now loads the full-size cover of the row you are on after a reload or resize.
- Under Track Artwork's Track's Own Cover, a smart playlist's rules editor now reloads its preview covers when you return to it.

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
