# Changelog — v0.31.x archive

Releases v0.31.0, covering 2026-11-18. The current changelog (v0.32.0 onward) lives in [CHANGELOG.md](../CHANGELOG.md).

## v0.31.0 — 2026-11-18

### Added

- Harbour's new Instant Mix row keeps the playing song and replaces the rest of the queue with songs like it.
- With nothing playing, Instant Mix plays songs like up to five songs Harbour picks from your recent, most played or random tracks.
- Shift+A on the Instant Mix row adds the mix to the queue and skips songs already in it.
- Instant Mix leaves out tracks under a minute and songs rated one star.
- Autoplay, a new playback mode, adds 10 songs like your last few when at most one song is left after the playing one.
- Autoplay toggles from its infinity button in the player bar, the modes menu, the `a` key, `nokkvi autoplay` or Settings → Playback.
- Songs Autoplay added show a small infinity sign before their artist in the Queue.
- Songs you add to the queue go ahead of the Autoplay songs that have not played yet, while shuffle is off.
- Turning Autoplay off removes the songs it added that have not played yet.
- When it finds no similar, same-genre or same-artist songs, Autoplay adds random songs from your library instead.
- Autoplay waits while Repeat is on, and its button's tooltip says so.
- If the queue runs out before a batch arrives, Autoplay plays the batch's first song when it lands.

### Changed

- Most of the GPU memory the Bars, Lines or Scope visualizer takes in fullscreen Theater Mode is now freed once it is off screen.
- The Albums view now uses about half the CPU while a song plays.
- The Artists, Genres and Playlists views now use less CPU while a song plays.
- The night Trawl scene's anchor is now outlined and shaded like the boat instead of a flat shape.
