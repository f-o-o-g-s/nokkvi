# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

### Changed

### Fixed

- Surfing boat now moves on ambient and soundtrack tracks (sustained pads, drones, slow swells) that produce loud but slowly-changing spectra — these previously made the boat coast to a stop because the cruise signal listened only to spectral change.
- Surfing boat's top speed now scales with the music's energy stack instead of pinning every percussive track at the same ceiling — energetic tracks (brick-walled, blast-beat, heavy-onset material) now read visibly faster than steady punchy tracks, and the baseline cruise speed is lifted across the board.

### Removed

## v0.3.12 — 2026-05-04

### Added

- Drop-anchor doodad on the surfing boat — every 45–120 s the boat anchors for 10–15 s, dropping a lucide-anchor icon to the bottom of the visualizer with a curved theme-colored rope back up to the boat. The rope sways with local wave amplitude, the anchor stays planted on the floor while the boat bobs above it, and tacks/wind shifts are paused for the duration.

### Changed

- Surfing boat is now propelled entirely by the music — silence brings the boat to a stop, and tagged BPM + onset energy + a slow-window energy envelope together drive both the cruise speed and the velocity floor. Different songs now produce visibly different boat motion instead of every track looking the same.
- Surfing boat now eases into and out of direction changes instead of snapping on a dime — sail thrust drops to zero at the moment of a tack and ramps back to full over four seconds, so the boat decelerates through zero and accelerates smoothly onto its new heading.
- Surfing boat slope force now only resists motion — going up a wave still slows the boat, but going down a wave no longer accelerates it. A sailboat doesn't surf the way a board does.

### Fixed

- Right-edge spacing on the Queue and Songs row text (duration or play count) when the Love column is toggled off — the trailing text was previously flush against the row's right edge instead of carrying the same padding it has when Love is on.

## v0.3.11 — 2026-05-03

### Added

- Genre column toggle on Queue and Songs views — stacks under the album when both are visible, takes over the album slot at album-size font when album is hidden, and auto-shows when the list is sorted by Genre.
- Multi-select column UI-wide — opt in per view (Albums, Artists, Genres, Playlists, Queue, Songs, Similar) under each view's columns-cog dropdown to add a row-level checkbox plus a tri-state select-all header that mirrors ctrl/shift+click selections.

### Changed

- Surfing boat now sails continuously through the screen edges instead of bouncing back: when it reaches one side it keeps its momentum and emerges from the opposite side, drawn split across the seam during the crossing so the wrap looks seamless.
- Surfing boat now wanders both directions evenly instead of drifting consistently toward bass: the soft pull-toward-center spring and the captain's bias toward the louder half of the spectrum are gone, since on a torus they conspired to favor whichever wrap direction the music's spectrum happened to lean.
- Surfing boat now tilts to match the local wave slope (spring-damped so it eases into the lean instead of snapping to spectrum jitter, capped at ~17°) and horizontally mirrors itself based on travel direction so the sail catches wind from behind whichever way the boat is sailing. Tilt is baked into the SVG path data each frame (rotation applied in vector space and then rasterized fresh, rather than rotating an upright bitmap in the GPU shader) so the rotated boat stays sharp even at small sprite sizes; the resulting handle is cached per quantized angle to keep resvg cost bounded.
- Surfing boat now carries a thin outline that uses the same `border_color` / `border_opacity` as the lines-mode wave outline, so it reads as part of the same theme. The outline tracks the active theme automatically and follows whichever opacity the theme defines (so it matches the wave's behavior in light mode where the border is intentionally hidden).
- Surfing boat outline is now half as thick (~0.5 px instead of ~1 px) — the previous stroke read as too heavy on the small sprite and competed with the fill instead of just tracing it. Wave-line outline thickness is unchanged.
- Clicking an album, artist, or genre name link in any list now navigates to that item's view and expands it inline at the top, instead of leaving you on a one-row filtered list with the contents hidden behind a follow-up Shift+Enter.
- Surfing boat now gets a brief off-screen stretch past each screen edge with a quiet eject impulse — when it leaves frame, slope-tracking pauses and a firm push eases it through the seam, so music with loud bins near a spectrum edge can no longer keep dragging it back to the edge it just tried to leave.

### Fixed

- Thumbnails in large genre and artist expansion rows no longer leave a stray slot or two permanently blank — failed cover-art fetches now retry up to three times instead of caching the empty result.
- Clicking an artist name link in the queue or songs view now loads the large artist image and dominant color in the artwork column on arrival — previously it stayed blank until you scrolled to a different artist and back. Same fix for the genre 3×3 collage column when clicking a genre name link.
- Surfing boat no longer gets pinned at the wrap seam or dwells near either screen edge, and the captain's rowing charges now ramp in and out smoothly (half-sine envelope) instead of feeling like motor thrust kicking on and off.
- Columns-cog dropdown in the library browsing panel now opens — previously it was wired closed and never showed its menu.
- Surfing boat no longer clips its corners off when tilting to extreme angles.
- Multi-select checkbox toggles in the library browsing panel now add or remove only the clicked row — previously, clicking an already-checked checkbox kept it checked while every other selected row was wiped.
- Drop indicator during cross-pane drag-and-drop now aligns with the queue rows when the queue's Select column is enabled, instead of riding 24 px above where it should have been.
- First mouse-wheel scroll after clicking a name link to inline-expand a target in another view no longer jerks the highlighted target row from the top of the list down to the middle — the highlight now stays on the row and rides the scroll naturally until it leaves the viewport.

### Removed

- Third-tier inline expansion in Artists and Genres views (album → tracks). Both views are now 2 levels deep like the others; the "X songs" link on a child album row and Shift+Enter on a centered child album now jump to the Albums view and expand the album there.

## v0.3.10 — 2026-05-02

### Changed

- Surfing boat now makes periodic rowing charges toward the louder side of the waveform instead of camping on the calm side, and sits slightly into the wave line instead of hovering above it.

### Fixed

- Surfing boat no longer gets stuck against the far left or right of the window — a soft wall bumper pushes it back inward — and the boat is now clipped to the visualizer area so it doesn't draw on top of the player bar.
- Surfing boat now picks up theme changes immediately — switching presets, toggling light/dark, or editing colors no longer leaves it painting the previous palette until restart.
- Surfing boat now freezes when audio is paused — previously it kept drifting against the held waveform and ended up off the line with no way to resync on resume.
- Surfing boat now stays aligned with the wave line during play and sinks to the bottom during silence — previously the visualizer's frozen baseline at the end of a track could leave it parked well above the visible wave with no way back down.
- Remove from queue (right-click or Ctrl+D) and Play next now consistently target the song you clicked — previously, after sorting the queue or removing other songs in the same session, the action could hit a different row or silently do nothing.
- Multi-selection in the queue now clears on background queue refreshes (consume mode advancing, navigation reload) and on sort changes — previously the selection kept its row positions across the reorder and could target the wrong songs on the next bulk action.
- Removing the currently-playing song from the queue now stops that track and rolls forward to the next song (or stops if the queue empties) — previously the audio kept streaming the deleted song while the strip advertised a different one as "now playing".

## v0.3.9 — 2026-05-01

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
