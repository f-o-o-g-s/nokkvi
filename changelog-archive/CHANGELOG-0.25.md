# Changelog — v0.25.x archive

Release v0.25.0, covering 2026-10-12. The current changelog (v0.26.0 onward) lives in [CHANGELOG.md](../CHANGELOG.md).

## v0.25.0 — 2026-10-12

### Added

- A new Player Bar setting, Seekbar Style, draws the seek bar as Classic, Line, Capsule, Cells or Waveform.
- The mini player keeps its own Seekbar Style, starting on the Capsule it already used.
- Hovering the Line, Capsule, Cells or Waveform seek bar shows the time under the cursor.
- The mini player's Visible Controls can now hide the cover art.
- The Waveform seek bar works out each song's shape once in the background and keeps it, so replays draw it instantly.
- The Waveform seek bar works out the next song's shape ahead of time, so it is usually ready when that song starts.
- On a link too slow to scan without crowding playback, the Waveform seek bar stops scanning new songs for the session.

### Fixed

- Releasing a seek bar drag after the song changed to one that can't seek no longer seeks a later song.
- Saving a different Last.fm API key now disconnects the old Last.fm login instead of still showing it as connected.
- When Last.fm rejects the saved login during radio scrobbling, nokkvi now disconnects it and warns you to reconnect.
- The Scrobble Radio setting's description now mentions Last.fm, which it also scrobbles to.
