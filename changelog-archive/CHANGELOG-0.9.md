# Changelog — v0.9.x archive

Releases v0.9.0–v0.9.4, covering 2026-06-15 → 2026-06-18. The current changelog (v0.10.0 onward) lives in [CHANGELOG.md](../CHANGELOG.md).

## v0.9.4 — 2026-06-18

### Added

- A new Scrollbar setting (Settings → Interface → Slot List) shows the slot-list scrollbar Always (default), On hover, or Hidden.
- A new Rating Change Notification setting (Settings → Playback) shows a desktop notification with the new star count when you rate by hotkey or CLI.

### Fixed

- The slot list no longer shows a small gray corner artifact at its top-right and bottom-right edges when Rounded Corners mode is on.

## v0.9.3 — 2026-06-17

### Fixed

- The CRT / Film visualizer effect no longer distorts the image at the window edges and now stays pixel-sharp and flush to them.

## v0.9.2 — 2026-06-15

### Added

- The login screen now uses a responsive two-panel layout on wide windows that folds into a single centered card on narrow ones.
- The login form now scrolls when the window is too short to fit it.
- Login errors now name the problem (wrong username or password, server unreachable, or not a Navidrome server) and highlight the field at fault.
- The login screen focuses the first empty field automatically, and Enter submits from any field.
- Nokkvi now fills in the address scheme and trims trailing slashes on the server URL you type.
- A subtle warning appears when an unencrypted http:// address points at a server outside your local network.

### Fixed

- Pressing Tab on the login screen no longer moves focus the wrong way.
- Resizing the window no longer cuts off the login form or the Login button.
- Sharing nokkvi.log for a bug report no longer leaks a reusable Navidrome credential from logged Subsonic request URLs.

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
