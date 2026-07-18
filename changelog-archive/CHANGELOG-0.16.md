# Changelog — v0.16.x archive

Release v0.16.0, covering 2026-07-15. The current changelog (v0.17.0 onward) lives in [CHANGELOG.md](../CHANGELOG.md).

## v0.16.0 — 2026-07-15

### Added

- **Queue sync with your Navidrome server.** A new sync button in the Queue header opens a menu to push the current queue to the server, or pull it back on any device (or a fresh session), including the playing track and its exact position. Each row names what it replaces ("Replaces the queue saved on the server" / "Replaces your local queue") so the direction is unmistakable before you commit. Pulling cues the restored queue without starting playback: if you were listening, it resumes mid-song right where the other device left off, and if you were paused, pressing Play picks up from the saved spot. Duplicate tracks survive the round trip (the sync rides the OpenSubsonic indexBasedQueue extension, so the playing slot is a position, not a track id), and huge queues are fine since saves go as POST form bodies. The button appears once the server advertises the extension (Navidrome 0.58.5 and newer); `nokkvi queue-push` and `nokkvi queue-pull` do the same from the command line.
- Trawl's rating filter gains "Unrated only", for mixes built purely from songs you haven't rated yet.

### Changed

- The default start view is now Harbour: new installs open on the home shelves, and users still on the old default move there too.

### Fixed

- Hovering the Queue's "Playing From" banner for a description-less playlist no longer shows a blank gap; the stats row now sits flush under the title.
