# Changelog — v0.26.x archive

Releases v0.26.0–v0.26.1, covering 2026-10-14 → 2026-10-15. The current changelog (v0.27.0 onward) lives in [CHANGELOG.md](../CHANGELOG.md).

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
