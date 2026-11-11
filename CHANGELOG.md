# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- Get Info on an album shows its version, like Deluxe Edition, and its record label when the tags carry them.

### Changed

### Fixed

### Removed

## v0.29.0 — 2026-11-10

### Added

- Settings → Playback has a new Streaming Quality section with Stream Format (Original, Opus or MP3) and Max Bitrate (64 to 320 kbps).
- With Stream Format set to Opus or MP3, lossless songs above Max Bitrate now stream from your server as a smaller copy.
- MP3s and other compressed songs more than a quarter above Max Bitrate also stream as a smaller copy.
- With Opus or MP3 chosen, songs in formats nokkvi can't decode, like APE, WavPack or DSD, now play as a smaller copy.
- With Opus or MP3 chosen, songs with more than two channels always stream as a stereo MP3 copy.
- A Streaming Quality change applies from the next song, leaving the song playing as it is.
- The Waveform seek bar shows the Line style for songs streamed as a smaller copy, unless their waveform was saved earlier.
- Seeking works in a smaller copy on Navidrome 0.51 or newer.
- A seek in a smaller copy that fails leaves the song playing where it was.
- A failed seek in a smaller copy says so, and says when the server is busy or not answering.
- When the connection drops partway through a smaller copy, nokkvi picks the song up where it stopped, on Navidrome 0.51 or newer.
- If picking a dropped smaller copy up again keeps failing, nokkvi says the server stopped sending the song.
- Streaming Quality shows when your server's player settings force a format or a bitrate limit for nokkvi.
- `nokkvi status` now reports the playing stream's codec, bitrate, sample rate and delivery, whether it is the original file, and how it seeks.

### Changed

- A seek refused in a song your server transcodes on its own now says the server is choosing its format.

### Fixed

- After a descending queue sort, songs that share a rating or other sorted value no longer flip order when the track changes.
- After a descending queue sort with shuffle off, Next now plays the song shown below the current one.
- A transcode served from Navidrome's cache is now labelled with the codec actually playing, instead of the file's own format.
- BIT-PERFECT now shows only when the stream is the song's original file.
- During a skip crossfade into a transcoded song, the format label now shows its codec instead of the outgoing song's.
- When the next song fails to load, Next now says why instead of "No next track".
- When the previous song fails to load, Previous now says why instead of failing silently.

## Older releases

- **v0.28.x** (2026-10-22 → 2026-11-03, v0.28.0–v0.28.2): [CHANGELOG-0.28.md](./changelog-archive/CHANGELOG-0.28.md)
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
