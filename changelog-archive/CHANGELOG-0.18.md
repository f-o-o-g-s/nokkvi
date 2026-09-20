# Changelog — v0.18.x archive

Releases v0.18.0–v0.18.4, covering 2026-07-19 → 2026-07-25. The current changelog (v0.19.0 onward) lives in [CHANGELOG.md](../CHANGELOG.md).

## v0.18.4 — 2026-07-25

### Fixed

- Browsing Albums or Songs sorted Random no longer repeats or skips rows after a visit to Harbour.
- The Random Genre row now works on servers that do not report genre song counts.
- The Random Playlist row now shows its icon instead of a blank square while artwork loads.
- The Random Artist row now shows the same large artwork the Most Played Artists shelf does.
- Add to Queue now works on Harbour's five Random rows.
- Add to Queue now works on Harbour's genre rows.
- Holding the refresh key on Harbour no longer starts overlapping reloads.
- The Random Genre row no longer lists more songs than one press plays.
- The Random Genre row's details no longer read "1 songs".
- The Random Playlist row's details no longer read "1 songs".
- Random rows with no pick drawn now play the dismiss sound, not the confirm sound.
- Harbour's no-pick message no longer blames an empty library when a draw simply failed.
- Playing a random genre now surfaces the server's error instead of reporting no songs to play.
- The Random Artist row now keeps its pick when one request fails mid-draw.
- Various Artists albums in random and similar-song queues no longer join gaplessly as one album.
- Changing the font in Settings now updates the Trawl modal, login form, and empty-state labels too.

## v0.18.3 — 2026-07-25

### Added

- Harbour gains a Random block: one row each for a random album, artist, playlist, genre, and 100-song mix.
- Each Random row shows the pick it drew, with artwork and details.
- Pressing a Random row plays the pick it shows.
- Refreshing Harbour re-rolls every Random row's pick.
- Harbour's header gains the standard refresh button.

### Changed

- Harbour's Random rows follow the nav bar's order: Album, Artist, Songs, Genre, Playlist.
- The queue's album and genre column now uses the same text size as the Songs view.
- Artist lines on expansion child rows now use the standard subtitle size.
- The Songs plays column is now right-aligned.

### Fixed

- Shift+Enter on a Harbour item row now collapses its section.
- Labels that ignored the Settings font at launch now use it.
- Harbour section headers now use the same title-subtitle spacing as the rows below.
- Harbour section header subtitles now use the same color as every other subtitle.
- A centered Genres row now bolds its name.

### Removed

- The four-pick Random Playlists and Random Genres shelves.

## v0.18.2 — 2026-07-22

### Added

- Settings search now reaches every row by synonym, including all 53 hotkey rows, radio scrobbling, and the Scope visualizer controls.
- Hotkey rows now answer searches for "shortcut", "keybind", and "keyboard".
- The theme picker now answers color searches like "palette", "accent", and "border".
- The smart-playlist editor gains an Add group row, so mixing All and Any logic no longer requires the raw JSON editor.

### Changed

- New installs default the Verbose Config setting to Clean, so config.toml stays free of the inline comments the previous Off default injected.
- The Artists view now bolds the centered row's artist name, matching the other library views.
- Settings search now matches words in any order, so "radio scrobble" finds the Scrobble Radio row.
- New installs now default the Auto-mode artwork size to its largest setting, so album art is bigger out of the box.

### Fixed

- Playback that stops because the next track fails to load now reports the error instead of stopping silently.
- An expired session during a library refresh now returns you to the login screen instead of a generic error toast.
- A server error sent in place of audio now reports the track as unavailable instead of a format failure.
- Artist names in the Artists view now stop being clickable when Slot Text Links is off.
- Settings search no longer lists unrelated rows for a term nokkvi has no setting for.
- A search fragment matching mid-word in a tab name, like "lay" in Playback, no longer pulls in that whole tab.
- Ten settings rows now show help text that was previously invisible.

## v0.18.1 — 2026-07-19

### Changed

- The queue's playlist banner shows a smart-playlist indicator in place of the save button for smart playlists.

### Fixed

- Typing a capital letter in the smart-playlist editor no longer fires a global hotkey instead of inserting it.

## v0.18.0 — 2026-07-19

### Added

- Smart playlists are now recognized: a sparkles badge marks them in the Playlists view.
- Expanded playlist tracks gained a right-click menu: play, queue/mix/playlist adds, Get Info, Remove from Playlist.
- Shift+Up/Down reorders tracks inside the playlist editor.
- Edits in the playlist editor stay staged until Save.
- Create-playlist dialogs warn when the name already exists, without blocking.
- Get Info now works inside the playlist editor.
- Smart playlists can now be created and edited in-app: a rules editor with live validation.
- The Playlists header's + becomes a create menu (regular or smart) on capable servers.
- Edit Rules… appears on smart playlists you own.
- The queue banner's edit pencil opens a smart playlist's rules too.
- Shift+N opens a new smart playlist.
- `e` edits the centered playlist.
- Rules editing includes seeded presets, a raw-JSON mode, and an honest evaluated-at freshness stamp.
- Rule previews are server-evaluated through a private draft, fresh on every press, before anything saves.
- Enter on a preview row plays it — tweak rules and hear matches in one loop.
- The rules preview gained a columns cog: show stars, love, plays, genre, and duration alongside each match.
- Stray preview drafts clean themselves up at login.
- Until then, other clients see stray drafts clearly labeled.
- .nsp smart-playlist files import from the Playlists create menu, with update-or-create-new choice on name collision.
- The rules editor's empty state can load a .nsp file straight into the open session.
- The Trawl mix builder gained Save as Playlist (Shift+P): the resolved mix becomes an ordinary playlist.
- Rule values are picked, not typed: genres and ratings choose from lists, dates from a calendar.
- The playlist editor's edit bar shows the cover and sets custom art or resets it.

### Changed

- Add-to-playlist pickers, quick-add, queue overwrite, and the default-playlist choice now skip smart playlists, whose tracks the server keeps read-only.
- Creating a regular playlist drops into the editor to name it and add tracks, replacing the naming dialog.
- Deleting or renaming a file-backed playlist now explains scan resurrection and rules re-sync honestly.

### Fixed

- Renaming a smart playlist on Navidrome 0.61 no longer wipes its rules.
