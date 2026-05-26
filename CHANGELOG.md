# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

### Changed

### Fixed

### Removed

## v0.6.0 — 2026-05-25

### Added

- New "Chrome Border" theme entry (TOML key `border`) controls the 1 px separator color used across nav, slot lists, settings, and modals; auto-derives from the theme's hard background when left empty.
- New "Mini Player" track-info display mode — artwork thumbnail with stacked title / artist / album to the left of the transport, transports stacked above the scrub bar, visible down to 540 px windows.
- New "Top Bar Under" metadata-strip mode places the strip on its own row directly below the top nav (top-nav layouts only).
- Nav-bar labels render in uppercase on both the top and side nav.
- Top-bar tab labels now scale with window width and stretch into empty center space when no other widgets are present.
- Side-nav tabs stretch across the full column instead of hugging the icon.
- Settings sidebar is now persistent — six categories live in a left rail with the search input pinned above them, and the right pane scrolls a variable-height list of rows with inline help text per label.
- Settings detail pane auto-scrolls the focused row into view as keyboard navigation walks the list, with a themed scrollbar matching the design.
- Below 1400 px of content width, the Settings sidebar collapses to a horizontal scrollable chip strip above the detail pane (with the search bar pinned in a thin strip above).
- New `SettingsCategoryNext` / `SettingsCategoryPrev` hotkeys (default `Shift+Tab` / `Shift+Backspace`) step between Settings categories from anywhere in the Settings view, including while the search input has focus.

### Changed

- **Flat redesign across all chrome.** Every surface (nav, transport, slot rows, modals, settings widgets) now uses a 1 px sided-border vocabulary in flat mode and a coherent pill / radius scale in rounded mode.
- Hamburger menu and library-filter trigger now sit on the LEFT of both top-nav and side-nav layouts (previously top-nav had them on the right).
- Player bar: 40×40 borderless transport buttons, 38 / 40 px mode toggles with a 1 px chrome-border outline, a single 8×44 vertical-bar volume meter per channel (music + SFX render side-by-side as two bars), and a 6 px thin progress bar with a 14 px handle. Base height 72 px in both modes.
- Status strip below the player bar bumped to 24 px on a dedicated background slightly darker than the main hard background, so it reads as its own band.
- Side-nav restyled with a narrow 32 px (flat) / 40 px (rounded) column; rounded mode keeps the pill-card tab visuals inside 4 px outer gutters.
- View header: flat sided-border row (50 px) in flat mode, pill segmented capsule (44 px) with inset search in rounded mode.
- Slot rows now touch (zero gap) with a bottom-only 1 px chrome-border separator; rounded mode wraps the whole list in an outer rounded shell.
- Modal chrome unified across all five overlay modals (About, Info, EQ, text-input, default-playlist picker): hard background, 1 px accent outline, large rounded corners.
- Hover overlay tint is now theme-aware (light tint on dark themes, dark tint on light themes) for cross-theme legibility.
- Settings widgets (Bool / Enum / ToggleSet / Hotkey / HexColor / Number) restyled to the design's chip vocabulary; settings rows get a 3 px accent left-stripe cursor treatment.
- Settings rows no longer surface a per-row "Default: X" column — defaults are reached through the Theme tab's Restore sentinels and the Del-while-editing hotkey, freeing the value column to stretch wider.

### Fixed

- Pressing Escape while a text-input dialog is open inside Settings cancels the dialog instead of closing the Settings view.
- Overlay modals (About, Info, EQ, text-input, default-playlist picker) reset to closed on logout, so a stale modal from the previous server is never briefly visible after re-login.
- The About modal's "Copy All" preserves the Captain and Shipwrights attribution rows (previously dropped, with User/Navidrome ordering swapped).
- SFX volume slider responds to mouse-wheel scrolling, and the music slider's wheel handler no longer reuses a stale base volume across rapid notches.
- A failed large-artwork fetch no longer leaves the in-flight marker stuck, which previously suppressed all subsequent artwork fetches for the same surface.
- Similar and Top Songs view-header labels ("similar to: …" / "top songs: …") no longer clip to a single character before the ellipsis at typical window widths.

### Removed

- 3D bevel rendering across all chrome: transport buttons, mode toggles, nav tabs, hamburger, library-filter trigger, slot rows, and settings widgets all switched to flat (1 px border) or rounded (pill / soft radius) vocabulary.
- Scrolling metadata overlay on the progress bar — replaced by the new "Mini Player" display mode. Existing TOML configs with `track_info_display = "progress_track"` migrate to the new mode automatically.
- Per-cover dominant-color extraction — the text overlay on the large-artwork column now pins to the theme's hard background instead of blending in a tint sampled from the current album cover.

## v0.5.3 — 2026-05-24

### Added

- In Top nav layout, the artwork panel now reaches up over the nav bar, except when the metadata strip itself occupies the top bar.

### Changed

- The label naming the centered slot-list item is now an opaque bar pinned to the bottom of the artwork panel, replacing the centered floating pill.
- The queue's read-only playlist context bar no longer shows a small icon next to the playlist title.
- Overlay menus (hamburger, player-modes kebab, context menus, library-selector popover) now cast a drop shadow.
- Toggles in the player-modes kebab now render with the same filled checkbox glyph as the library-selector popover.

### Fixed

- Playlists side-nav tab now uses a music-note sheet icon to avoid the 'iii' visual collision with rotated tab text.

## v0.5.2 — 2026-05-23

### Added

- MPRIS album-art cache now self-cleans — orphan files from prior crashes are swept on launch, and clean exits clear the cache on shutdown.

### Fixed

- Logging out and back in to Navidrome no longer leaks one OS thread per cycle, so long-running sessions stay flat in thread count.
- MPRIS album art shown by desktop shells now refreshes on every track change instead of pinning to the first track's cover for the whole session.

## v0.5.1 — 2026-05-22

### Added

- Multi-library filter — a new nav-bar popover (top-nav and side-nav
  layouts) lets users scope every browse view (Albums, Artists, Songs,
  Genres) to a subset of Navidrome libraries. Empty selection is treated
  as "all". The trigger is hidden when only one library exists, so
  single-library servers see no UI change. Selection persists across
  restarts (redb), and libraries deleted on the server are pruned from
  the active set at next launch. Playlists are intentionally not
  filtered — Navidrome's `/api/playlist` endpoint ignores `library_id`
  and the server's per-user library access already filters playlists.

### Changed

- Hamburger menu moved from the far-right of the top nav to the left edge, next to the library-filter trigger in both top-nav and side-nav layouts.

### Fixed

- MPRIS `LoopStatus` requests from clients like `playerctl` now set the requested mode directly instead of cycling, so `playerctl loop Track` from Playlist state no longer lands on None.
- MPRIS cover art is published on D-Bus as a local file URL instead of an authenticated Navidrome link, no longer exposing the Subsonic credential triple to other same-user processes.
- Switching Navidrome servers no longer shows the prior server's covers for overlapping album IDs, retries SSE against the old host, or emits the old server's cover via MPRIS until the next track change.
- Radios and Similar views now render the right number of rows after a window resize, matching every other slot-list view.
- Library-changed SSE events with non-ASCII metadata (artist names with diacritics, Japanese titles, …) are no longer dropped when a multi-byte character spans an HTTP chunk boundary.

## v0.5.0 — 2026-05-21

### Added

- New `nokkvi <verb>` CLI for scripting and WM hotkeys — 16 verbs covering transport, volume, queue, view-switching, and `love`/`rate` on the currently-playing track.

### Fixed

- Lock, heart, and star outline icons in slot-list rows now darken in lockstep with the row's text when the row is selected (centered, multi-selected, or currently playing) — previously they kept their muted light tint against the light selected-row fill and were hard to read. Most visible on private playlists in the Playlists view, but the heart and star outlines had the same issue under multi-selection (ctrl-click) on every slot-list view.
- Menu text no longer renders blank on systems whose sans-serif font has no Medium weight (e.g. Sway + Intel iGPU; reported by hollisticated-horse).

## Older releases

- **v0.4.x** (2026-05-16 → 2026-05-19, v0.4.0–v0.4.2): [CHANGELOG-0.4.md](./changelog-archive/CHANGELOG-0.4.md)
- **v0.3.x** (2026-04-27 → 2026-05-14, v0.3.1–v0.3.17): [CHANGELOG-0.3.md](./changelog-archive/CHANGELOG-0.3.md)
