# Changelog

## [Unreleased]

### Features
- **Verbose Configuration** — Introduced a verbose configuration mode that saves even default settings to `config.toml`, guaranteeing total consistency between UI controls and the persistence file.
- **Configurable Settings Migration** — All user configuration preferences (Playback, Hotkeys, General, Theme, Visualizer) are now entirely hosted in the hot-reloadable `config.toml` file, eliminating reliance on local `redb` state for settings.
- **Hardware Volume Integration** — Real-time volume synchronization with PipeWire stream channel volumes (`SPA_PROP_channelVolumes`).

### Improvements
- **Standardized Hotkey Formats** — Upgraded from legacy non-standard Unicode artifacts (e.g., arrows) to formal ASCII identifiers (e.g., `RightArrow`, `UpArrow`) for resilient TOML parity.

---

## v0.1.1 — 2026-03-29

### Fixes
- **Modal icon visibility** — resolved a critical rendering bug where SVG icons were invisible in overlay modals (`EQ` and `About`) by adopting the view-header rendering pattern.
- **EQ toggle toast** — fixed a bug where enabling/disabling the equalizer from the modal produced an empty toast notification.

### Improvements
- **EQ visual language** — overhauled the 10-band equalizer modal with themed colors (accent-based instead of status-indicator green/yellow) and a refined **Save Preset** UX.
- **Preset optimization** — updated all 10-band EQ built-in defaults to better align with modern audio standards.
- **Nautical branding** — updated the **About** modal with nautical-themed contributor roles and the project tagline: *"A sturdy hull for the endless stream."*

---

## v0.1.0 — 2026-03-29

### Features
- **Graphic Equalizer** — implemented a 10-band graphic equalizer with custom preset management and redb persistence.

### Fixes
- **PipeWire hardware sync** — synchronized PipeWire stream properties via mainloop IPC to prevent tokio runtime panics and ensure metadata is correctly pushed.

### Improvements
- **PipeWire backend cleanup** — internal refactoring of the native pipewire implementation for better reliability.

### Internal
- Removed accidental cpal submodule pointer.

---

## v0.0.10 — 2026-03-28

### Fixes
- **PipeWire hardware volume control** — restored native PipeWire stream volume control by explicitly setting the `application.name` and `media.role` stream properties, ensuring the desktop shell and external tools can identify and control Nokkvi's audio node.

## v0.0.9 — 2026-03-28

### Features
- **Native PipeWire integration** — the audio engine now explicitly selects the native PipeWire host when available, falling back to ALSA, preserving stream metadata (`application.name`, `media.role`).
- **About modal** — added a new modal accessible from the hamburger menu that displays application version, system information, and a new custom boat icon.

### Fixes
- **About modal icon** — corrected the logo in the new About modal so the correct dimensions and source are used without upscaling artifacts.

### Improvements
- **Theme preset updates** — aligned the bundled layout presets by removing hardcoded font overrides and updated the Everforest preset to match the active configuration.

### Docs
- **Open Source guidelines** — added comprehensive preparation documentation and third-party license notices in preparation for public release.

### Internal
- Removed old logo iteration files from the root directory.

---

## v0.0.8 — 2026-03-25

### Features
- **Keyboard navigation for ToggleSet fields** — visible fields in settings can
  now be toggled via configurable EditUp/EditDown hotkeys.
- **Fall-fade peak mode** — new visualizer peak mode where peaks fall back down
  with a fading trail.

### Fixes
- **Scrollbar drag triggering cross-pane drag** — interacting with the library
  browser scrollbar no longer incorrectly initiates a cross-pane drag event, and
  artwork loading now works correctly when the browsing panel is active.
- **Playlist context bar layout overflow** — long playlist comments are now
  constrained and clipped, preventing layout overflow that pushed UI elements
  off-screen and triggered rendering failures.
- **Stale progress track overlay segments** — disabling metadata fields in the
  progress track overlay now correctly removes the corresponding segments.

### Improvements
- **Codebase quality refactor** — eliminated production `.unwrap()` calls,
  deduplicated message handlers, and consolidated playlist messages for improved
  robustness and maintainability.

### Docs
- **Settings metadata strip note** — added a note clarifying that the click
  action setting has no effect when the metadata strip is in Progress Track mode.

### Internal
- Agent rules and workflows synced with current codebase.

---

## v0.0.7 — 2026-03-23

### Features
- **Lines visualizer settings** — gradient mode, fill-under-curve opacity,
  mirror mode, and line style (smooth/angular) are now configurable in
  Settings → Visualizer → Lines.
- **Lines gradient mode** — five modes for line coloring: breathing (time-based
  palette cycling), static (single color), position (horizontal rainbow),
  height (amplitude-based), and gradient (wave-style position + amplitude
  blend where peaks shift further along the palette).
- **Progress track metadata overlay** — new "Progress Track" display mode
  shows title, artist, and album directly on the progress bar with per-field
  accent colors.
- **Per-field colors in progress track** — title, artist, and album text in
  the progress track overlay uses distinct colors for visual clarity.

### Fixes
- **Play button cold-start** — the play button now plays the currently
  selected track instead of always defaulting to the first track in the queue,
  matching enter-key behavior.
- **Keyboard navigation in top-packing slot lists** — restored keyboard
  navigation that broke when lists had fewer items than the visible slot count.
- **Settings footer description cutoff** — increased the description footer
  height and removed redundant header text so all gradient mode descriptions
  are fully visible.
- **Visible fields wired to progress track** — the metadata strip's "Visible
  Fields" toggles now also control which fields appear in progress track mode.
- **Metadata display mode switching** — switching the track info display mode
  (e.g. from Progress Track to Top Bar) now forces an immediate re-render,
  fixing stale layout where the previous mode's UI stayed visible.

### Improvements
- **Unified hover feedback** — refactored the hamburger menu button to use the
  shared `HoverOverlay` widget, making hover behavior consistent across all
  interactive elements.

### Internal
- Agent rules and workflows synced with current codebase.

---

## v0.0.6 — 2026-03-22

### Fixes
- **Play button cold-start** — pressing Play on an empty player after adding tracks
  to the queue now correctly starts playback and populates the metadata strip;
  previously the transport would stay in the paused state with no track showing.
- **Slot list top-packing on scroll** — when the queue or any slot list has fewer
  items than the visible slot count, scrolling no longer shifts items away from
  the top of the list.
- **Slot list top-packing initial render** — lists shorter than the viewport are
  now packed to the top on first render instead of being vertically centred.

### Improvements
- **Artwork panel visual cleanup** — internal padding and borders are removed from
  the artwork panel in the queue, genres, and playlists views; artwork now fills
  its square space edge-to-edge with no gaps.
- **Separator alignment** — the column separator between the artwork panel and the
  slot list is now drawn on the left edge of the artwork column, giving a cleaner
  visual split consistent with the header divider.

### Internal
- Removed unused `bg4` theme colour.
- Agent rules and workflows synced with current codebase.

---

## v0.0.5 — 2026-03-22

### Features
- **Playlist comment editing** — the split-view edit mode now includes a comment
  field alongside the playlist name, so you can view and edit the playlist
  description directly in the queue panel.
- **Persist playlist context across restarts** — the currently-playing playlist
  name, ID, and comment are saved to disk and restored on next launch, keeping
  the queue header accurate after a restart.
- **Configurable metadata strip** — a new Interface settings tab lets you choose
  where the track info strip appears (player bar, top bar, or both), and
  configure click behaviour (navigate to queue, album, or artist).
- **Right-click context menu on metadata strip** — right-clicking the strip now
  shows "Go to Queue", "Go to Album", "Go to Artist", "Copy Track Info",
  "Toggle Star", and "Show in Folder" actions.
- **Progressive metadata collapsing in nav bar** — track info in the side/top
  nav collapses gracefully as the window narrows (title → title+artist →
  title+artist+album → full metadata).
- **HoverOverlay on player bar buttons and side nav** — player bar transport
  buttons and the side nav hover indicator now use the `HoverOverlay` widget for
  consistent press-darkening and flash micro-animations.
- **Show in File Manager for albums and artists** — album and artist context
  menus now expose a "Show in File Manager" entry in addition to individual
  songs.

### Fixes
- **Artwork: full-res collage for single-album genres/playlists** — genres and
  playlists that contain only one album now correctly display the high-resolution
  1000px artwork in the collage view instead of a thumbnail.
- **Artwork: genre/playlist collage 3×3 tile layout restored** — a regression
  that collapsed the 3×3 collage to a single tile was fixed.
- **Artwork: large artwork URL in queue fallback** — the queue's fallback artwork
  path now correctly builds a full-size URL instead of reusing the thumbnail URL.
- **Artwork: 80px thumbnails for queue song rows** — each song row in the queue
  now requests the 80px thumbnail instead of the 1000px image, reducing bandwidth
  and memory usage.
- **Queue playlist edit bar layout** — the name and comment inputs are stacked
  vertically instead of overflowing horizontally.
- **HoverOverlay press-scale fix** — replaced the `HoverOverlay`-wrapped
  `Button` pattern with `mouse_area` + `HoverOverlay`-wrapped `Container` to
  correctly apply the press-darkening scale effect.
- **Hamburger menu Escape key** — pressing Escape now closes the hamburger
  context menu, consistent with all other context menus in the app.
- **Nav tab hover indicator cursor detection** — expanded the hit area for hover
  indicator detection in side nav icon-only mode so it triggers reliably.
- **Visualizer border width stale layout** — the visualizer widget now re-reads
  `border_width` from the live config on each `width()` call, fixing layout
  drift after hot-reload.
- **Gapless prep cleared on mode change** — stale gapless pre-buffer is now
  discarded when shuffle, repeat, or consume mode is toggled mid-playback,
  preventing the wrong track from being preloaded.
- **install.sh Exec= absolute path** — the desktop entry `Exec=` line is patched
  to the absolute binary path so launchers that don't search `$PATH` work
  correctly.
- **install.sh desktop database refresh** — `install.sh` now runs
  `update-desktop-database` after copying the desktop entry, ensuring the
  launcher discovers the app immediately.

### Improvements
- **Playlist header redesign** — the queue panel header for playing playlists
  now features accent stripes and separators for a cleaner visual hierarchy.
  Borders refined to 1px, stripe color adjusted to neutral.

### Internal
- Agent rules and workflows synced with current codebase.
- Added `/commit` workflow for conventional commits.

---

## v0.0.4 — 2026-03-19

### Fixes
- **install.sh permissions in zip** — `package.sh` now uses `cp -p` to preserve
  the execute bit on `install.sh`, so users no longer need to `chmod +x` after
  extracting.

---

## v0.0.3 — 2026-03-19

### Fixes
- **install.sh copies binary** — `install.sh` now copies `target/release/nokkvi`
  to `~/.local/bin/`, so the desktop entry's `Exec=nokkvi` works without manual
  `$PATH` setup. Exits with a helpful message if the binary hasn't been built yet.

---

## v0.0.2 — 2026-03-19

### Features
- **Click track metadata to navigate** — clicking the title, artist, or album
  text in the track info strip (top bar, player bar, or side nav) navigates to
  the queue view. Codec and bitrate fields remain non-clickable.

### Fixes
- **Network playback stuttering** — audio no longer cuts in and out when the
  Navidrome server is on a different machine. Root cause was ring buffer
  starvation: the cpal audio callback consumed samples faster than the decoder
  could supply them over the network, producing silence on underruns.
  - Ring buffer increased from 2s to 5s (192K → 480K samples), giving more
    runway to absorb network latency spikes.
  - HTTP connection pooling re-enabled — previously every 256KB chunk fetch
    opened a new TCP connection, paying TLS handshake and TCP slow start on
    each request.
  - HTTP chunk cache doubled (8 → 16 chunks, ~4MB) to reduce re-fetches.
  - Sequential prefetch added — the HTTP reader now speculatively fetches the
    next chunk after each read, keeping the cache ahead of the decoder.
  - Pre-buffering increased: 5 → 15 chunks at playback start, 3 → 10 after
    seek, ensuring the ring buffer is well-filled before audio begins.

### Internal
- Agent rules and workflows synced with current codebase.

---

## v0.0.1 — 2026-03-19

Initial release.
