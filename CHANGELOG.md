# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

### Changed

- The About modal now credits Claude Opus 5.5 and Fable 5.1 as the shipwrights (previously Opus 5).
- The metadata strip now defaults to Player Bar Above instead of Mini Player.
- A config.toml that doesn't set `track_info_display` now loads the metadata strip on Player Bar Above.

### Fixed

- With Merged Mode off, the metadata strip's fields now fill the space between format and bitrate instead of the middle third.

### Removed

## v0.24.0 — 2026-10-11

### Added

- Text fields now undo and redo with Ctrl+Z and Ctrl+Y.
- The raw rules JSON editor now deletes a whole word with Ctrl+Backspace or Ctrl+Delete.
- Bars and Lines have a new Reflection Below Cover setting, on by default.
- Lyrics with word timing (TTML, enhanced LRC, server cues) now light the current line word by word as it is sung.
- A new Playback setting, Lyrics: Word Highlight, picks Sweep (a karaoke fill), Word (each word lights whole) or Off.
- On Navidrome servers that offer API keys, nokkvi now connects with its own revocable key instead of a token derived from your password.
- nokkvi's API key lives on its own player in Navidrome's Players list, named after your computer.
- The API key's player starts with your current nokkvi player's transcoding, bit-rate and scrobbling settings.
- Logging out revokes nokkvi's API key on the server.
- A new Metadata Strip option, Player Bar Above, shows the strip on top of the player bar's controls.

### Changed

- Beside the artwork column, the player bar now spans only the list.
- The artwork column beside the list now runs down to the window's bottom edge.
- With the Metadata Strip on Top Bar or Top Bar Under, the strip now spans only the list beside the artwork column.
- Toasts now sit over the list instead of across the artwork column.
- The artwork's resize handle is now invisible, marked only by the resize cursor at the artwork's edge beside the list.
- Mouse-wheel scrolling in Settings, the Get Info dialog and the rules editor now glides instead of jumping.
- Clicking a scrollbar's track in Settings, Get Info or the rules editor now pages toward the pointer instead of jumping there.
- Hotkey badges in Settings are now at least 96 pixels wide.
- With the Metadata Strip on Top Bar or Top Bar Under, the top nav bar now spans only the list beside the artwork column.
- With the Metadata Strip on Top Bar or Top Bar Under, the artwork column beside the list now reaches the window's top edge.
- With Reflection Below Cover on, the Reflection's water now hangs below the cover over the UI, where there is room for it.
- With Reflection Below Cover on, Bars and Lines now stand on the cover's bottom edge and fill Visualizer Height when their water fits below it.
- Lyrics over a large Queue cover now grow with the cover, the same size Theater Mode shows at that size.
- With Bars or Lines in the bottom band, the visualizer now spans only the list beside the artwork column.

### Fixed

- Typing in a text field no longer briefly maxes out a CPU core after each keystroke.
- Resizing the window no longer crashes the app while the rules JSON editor's cursor is scrolled out of view.
- The Horizon's receding waves and bars now crest above the visualizer instead of being cut off along a straight line at its top.
- Typing in the Queue's search while a playlist plays no longer knocks focus out of the field after the first letter.
- Playing or queueing a genre, or mixing one in Trawl, now includes songs tagged with a different capitalization of its name.
- On Navidrome 0.64 and newer, importing a .nsp file with an unreadable refresh delay now stops with an explanation before uploading anything.
- Importing a .nsp file with a refresh delay now warns when the server is older than 0.64 and will ignore it.
- Adding, removing or moving songs late in a track no longer turns the next crossfade or gapless join into a hard cut.
- Changing a crossfade or bit-perfect setting late in a track no longer turns the next transition into a hard cut.
- With Fade on Skip set to Crossfade, a song reached by pressing Previous late in a track no longer ends in a hard cut.
- With Fade on Skip set to Crossfade, pressing Next twice quickly on the second-to-last song now plays the last song.
- Seeking at the instant a crossfade finishes no longer jumps back into the previous song.
- Clearing the queue while a radio station plays now leaves the station playing, as removing every song already did.
- With ReplayGain on, a radio station now plays at the untagged-track level instead of the previous song's.
- Top Songs now shows the chosen artist's songs, not those of another artist with the same name, on Navidrome 0.64 and newer.
- A Top Songs request the server fails now shows an error instead of "No similar songs found".
- Starring, rating and scrobbling no longer make every other user's open Navidrome sessions refresh.
- A rejected or expired server token now returns you to the login screen instead of showing an error toast.
- Accounts whose username contains characters like + or & now play songs and load covers.
- A playlist the server fails to send now shows an error when played, expanded or edited, instead of opening as empty.
- A radio station list the server fails to send now shows an error instead of an empty Radios view.
- A failed library list fetch now shows an error instead of clearing your library selection.
- A Find Similar request the server fails now shows an error instead of "No similar songs found".
- Adding the same song to a playlist twice in quick succession now adds it once, then asks about the duplicate.
- In Theater Mode, the `nav-up`, `nav-down`, `enter`, `add-to-queue` and `remove-from-queue` commands now leave Theater Mode instead of acting on the hidden list.
- Switching between a radio station and the queue no longer briefly shows the previous item's cover in desktop media widgets.
- Desktop media widgets no longer show a seek bar while a radio station plays.
- Desktop media widgets now receive a track ID for the playing song or station.
- A media widget's jump to a position in a previous song, or past the song's end, is now ignored instead of moving the playhead.
- Launching nokkvi twice in quick succession now opens one window instead of two.
- A song the server transcodes on the fly now ends and moves to the next song instead of restarting.
- The progress bar now shows no seek handle for a song the server transcodes on the fly.
- Desktop media widgets can no longer seek a song the server transcodes on the fly.
- A song the server transcodes on the fly is no longer labelled bit-perfect.
- A song the server transcodes on the fly now shows the codec the server actually sends.
- A radio station whose server reports a stream length now reconnects when it drops instead of ending like a song.
- The Artists view now shows an artist's biography even when no other app has opened that artist on the server before.
- An artist biography's link now names the site it opens instead of always saying Last.fm.
- Clicking an artist off the centre row now loads that artist's full-size artwork instead of a blurry thumbnail.
- Scrolling an expanded artist's albums no longer loads another artist's artwork in the background.
- Several server rejections at once now return you to the login screen once, with one toast, instead of wiping the form repeatedly.
- A server rejection left over from an earlier login no longer logs out the session that replaced it.
- A genre or playlist list the server fails to send now shows an error and keeps the rows, instead of an empty view.
- A rejected session while loading genres or playlists now returns you to the login screen.
- Playing several items at once, or a Trawl mix, on a rejected session now returns to the login screen instead of saying "No songs found".
- A failed unstar no longer rates the song five stars locally and on the server.
- With "Skip the N already there" checked, a song that reaches the playlist while the add-to-playlist confirm is open is no longer added twice.
- A library list that fails to reload now keeps its header count instead of showing 0 above the rows it kept.
- With Navigation Layout set to Side, bottom-band Bars now fit beside the sidebar instead of losing their rightmost bars off the window's edge.
- Seeking in a variable-bitrate MP3 now resumes exactly where you aimed instead of seconds off.
- After a gapless transition, the new song's position and lyrics no longer run about a second ahead of its audio.
- Pausing with lyrics on now keeps the Queue cover and its lyrics on the playing song.
- Holding an arrow key in a list no longer requests the same full-size cover again while it is still loading.
- Scrolling a list on a slow server no longer requests a thumbnail again while it is still loading.
- The "Playing From" cover's 2×2 collage now loads even when its albums are scrolled out of view.
- The playing song's title now waits at most half a second for its cover before showing in the app and desktop media widgets.
- Logging into a different server no longer shows the previous server's queue.
- Logging back into a server now brings back the queue you left there.
- The Playing From playlist now switches along with the queue when you log into another server.
- After config.toml is edited to name another server or user, nokkvi now shows the login screen instead of resuming the old session there.
- A network error while scrobbling no longer writes your server credentials into nokkvi.log.
- A server URL typed with a capitalized scheme, like HTTPS://, no longer makes every song fail to play.
- nokkvi no longer follows a redirect from your server to another host, which could have handed that host your login.

## Older releases

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
