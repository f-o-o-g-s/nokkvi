# Changelog — v0.29.x archive

Releases v0.29.0, covering 2026-11-10. The current changelog (v0.30.0 onward) lives in [CHANGELOG.md](../CHANGELOG.md).

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
