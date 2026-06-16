# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- The login screen now uses a responsive two-panel layout on wide windows that folds into a single centered card on narrow ones.
- The login form scrolls when the window is too short, so the Login button always stays reachable.
- Login errors now name the problem (wrong username or password, server unreachable, or not a Navidrome server) and highlight the field at fault.
- The login screen focuses the first empty field automatically, and Enter submits from any field.
- Nokkvi now fills in the address scheme and trims trailing slashes on the server URL you type.
- A subtle warning appears when an unencrypted http:// address points at a server outside your local network.

### Changed

### Fixed

- Pressing Tab on the login screen no longer moves focus the wrong way.
- Resizing the window no longer cuts off the login form or the Login button.
- The log file no longer records the salt and token from Subsonic request URLs (streaming, cover art, and the server-version probe), so sharing nokkvi.log (for a bug report) can no longer leak a reusable Navidrome credential.

### Removed

## v0.9.1 — 2026-06-15

### Added

- The player-bar Bit-Perfect button now cycles three modes: Off, Strict, and Relaxed.
- Relaxed Bit-Perfect runs its own crossfade between back-to-back same-rate tracks, hard-cutting the rest.

### Changed

- Switching Bit-Perfect to Strict or Relaxed turns Crossfade off, and turning Crossfade on returns Bit-Perfect to Off.
- In Relaxed Bit-Perfect mode, only the few-second blend between tracks is not bit-perfect.

### Fixed

- Album art now shows in your desktop media controls and on the lock screen for tracks on multi-disc albums, instead of appearing blank.
- A brief network glitch while loading album art no longer leaves the thumbnail permanently blank.
- Album covers the server can't resolve now show a placeholder instead of being re-requested on every scroll and playback tick.

## v0.9.0 — 2026-06-15

### Added

- A new Bit-Perfect playback mode sends audio to your DAC untouched, with no EQ, software volume, or limiter.
- Bit-Perfect mode switches the output device to each track's native sample rate.
- A now-playing badge reads the real device clock and reports whether playback is bit-perfect, resampled, or unverified.

### Changed

- Hi-res tracks (24-bit and float) now play back at their real bit depth instead of being narrowed to 16-bit.
- Turning on Crossfade now turns off Bit-Perfect, and vice versa.
- The crossfade settings now explain that turning crossfade off plays tracks gapless, and that very short tracks always play gapless.

### Fixed

- Turning the visualizer off now releases the CPU it was using during playback.
- The crossfade duration slider no longer offers values above 12 seconds that silently reset to 12 on the next launch.
- The Mini Player's thumbnail now shows artwork when you skip to a track not visible in the queue, instead of a gray box.

## Older releases

- **v0.8.x** (2026-06-14, v0.8.0): [CHANGELOG-0.8.md](./changelog-archive/CHANGELOG-0.8.md)
- **v0.7.x** (2026-06-07 → 2026-06-10, v0.7.0–v0.7.2): [CHANGELOG-0.7.md](./changelog-archive/CHANGELOG-0.7.md)
- **v0.6.x** (2026-05-25 → 2026-06-06, v0.6.0–v0.6.10): [CHANGELOG-0.6.md](./changelog-archive/CHANGELOG-0.6.md)
- **v0.5.x** (2026-05-21 → 2026-05-24, v0.5.0–v0.5.3): [CHANGELOG-0.5.md](./changelog-archive/CHANGELOG-0.5.md)
- **v0.4.x** (2026-05-16 → 2026-05-19, v0.4.0–v0.4.2): [CHANGELOG-0.4.md](./changelog-archive/CHANGELOG-0.4.md)
- **v0.3.x** (2026-04-27 → 2026-05-14, v0.3.1–v0.3.17): [CHANGELOG-0.3.md](./changelog-archive/CHANGELOG-0.3.md)
