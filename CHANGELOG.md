# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- Column visibility toggles for Index and Thumbnail on Queue, Albums, Songs, and Artists views.
- Album thumbnails in nested album expansion rows (Artists→Album and Genres→Album), and a full column-visibility menu on Genres view (Index, Thumbnail, Album count, Song count).
- Center-on-playing button in the Radios view header (previously only available via keyboard shortcut).
- Full column-visibility menu on Playlists view (Index, Thumbnail, Song count, Duration, Updated at).
- Public/private playlist support. New playlists default to public; toggle visibility in the create dialog or via a lock/unlock button in the split-view edit bar. Private playlists show a lock glyph in the list view, with hover tooltips on every control.
- Create-new-playlist button in the Playlists view header — opens a name + public dialog and drops you into split-view edit mode for the new empty playlist.
- Optional surfing-boat overlay for the lines visualizer — a small boat drifts across the waveform and rides each wave's slope. Toggle under Settings → Visualizer → Lines.

### Changed

- Hiding the Song count or Duration column in Playlists now hides those values entirely — they no longer fall back to a subtitle line under the playlist name.
- Save Queue / Add to Playlist modal: pencil icon prefix on the playlist-name input aligns it with the combo-box, and the Public checkbox indents under the input.
- Public row in the playlist info modal now renders ✓ in green and ✗ in red/orange (theme-aware) instead of plain glyphs.

### Fixed

- Partial dark themes that omit `[dark.success]` / `[dark.warning]` / `[dark.star]` now inherit the correct dark-mode greens/yellows instead of silently falling back to light-mode hexes.
- Merged metadata strip no longer renders orphan `title:` / `artist:` / `album:` labels when the field is empty but its show-label toggle is on.
- Keyboard-focus contrast restored in nested expansion rows — focused vs. unfocused stays clearly distinct at every depth across all built-in themes.
- Playlist context header no longer points at the wrong playlist after exiting edit mode on a playlist different from the one currently playing.

### Removed

## v0.3.8 — 2026-04-30

### Added

- Settings → toggle the `title:` / `artist:` / `album:` prefixes on the metadata strip and pick the field separator (·, •, |, —, /, │).

### Changed

- First-run default theme is now Everforest, matching the docs site styling.
- Retuned Everforest visualizer gradients: bars ramp green → tan → orange → red and peaks soften to cream/yellow; light mode picks up the same colors plus a visible dark border for readability on the cream background.
- Visualizer first-run defaults now match the shipped reference config — fresh installs get the intended look without editing `config.toml`.
- Metadata strip text bumped one point for readability.

### Fixed

- ProgressTrack metadata mode now honors the show-labels and field-separator settings instead of silently ignoring them.
- Top-bar merged marquee centers properly and the scroll lane spans the full center section on narrow windows; narrowing the window mid-track restarts the scroll cleanly instead of resuming mid-stride.
- Merged marquee scroll lane now stretches all the way between the codec/bitrate bookends — no more visible gaps inside each edge.

## v0.3.7 — 2026-04-29

### Added

- Default-playlist chip in the view header: always visible in the Playlists view; opt-in in the Queue view via a new `queue_show_default_playlist` setting (default off).
- Searchable picker overlay for choosing the default playlist, with thumbnail, song count, and total duration on each row, plus a "Clear default" entry that survives filtering. Also reachable from Settings → Playback → Playlists → Default Playlist.

### Fixed

- Merged player-bar metadata strip stays centered on narrow windows; codec/bitrate edge text no longer clips into the marquee.
- Dropped the redundant pair of separators flanking the merged metadata strip — the codec/bitrate sections already provide them.

## v0.3.6 — 2026-04-29

### Added

- `nokkvi --version` and `nokkvi --help`

### Changed

- Per-user data layout now follows XDG: `app.redb` and `nokkvi.log` move from `~/.config/nokkvi/` to `~/.local/state/nokkvi/` (one-time migration on first launch). `config.toml`, `themes/`, and `sfx/` stay in `~/.config/nokkvi/`.
- Debug builds now read and write `config.debug.toml` so a debug binary can run alongside a release install without overwriting each other's settings.
- Minimum supported Rust version is now 1.87 (only relevant when building from source).

### Fixed

- Auto-login no longer floods stderr with pre-login `shell_task` warnings; auth lifecycle (resume / success / failure) is now visible at default verbosity.
- Artwork panel is now properly centered in the always-mode artwork column.

## v0.3.5 — 2026-04-28

### Fixed

- Close-to-tray now actually hides the window on Wayland (Hyprland, KDE, GNOME, sway)

## v0.3.4 — 2026-04-28

### Added

- System tray integration with optional close-to-tray
- Artwork column display modes (Auto / Native / Stretched / Never) with draggable column width
- Per-mode hysteresis on the player bar; folded modes move into a kebab menu, transports collapse to prev/play/next at narrow widths

### Fixed

- Only one overlay menu (hamburger, kebab, view-header dropdowns, right-click) can be open at a time

## v0.3.3 — 2026-04-27

### Fixed

- Crash and missing artwork at certain window sizes caused by an iced sub-pixel image bug
- Cargo `license` field updated to current SPDX `GPL-3.0-only`

### Changed

- Added a 512px PNG launcher icon as a fallback for desktops that mis-render the SVG

## v0.3.2 — 2026-04-27

### Fixed

- About modal no longer shows "Commit: unknown" when built outside a git context

## v0.3.1 — 2026-04-27

### Added

- ReplayGain track/album volume normalization with pre-amp, untagged-track fallback, and clipping prevention; replaces the boolean AGC toggle with Off / RG Track / RG Album / AGC

### Changed

- Quieter default terminal logging (WARN+); full debug still goes to `~/.config/nokkvi/nokkvi.log`
- Tarball releases on GitHub; `install.sh` auto-detects tarball vs source build

### Fixed

- Workspace `license` field corrected so `nokkvi` and `nokkvi-data` crates report GPL-3.0
