# Changelog

## [Unreleased]

## v0.2.16 — 2026-04-23

### Features
- **Slot Text Links Toggle** — added a user-configurable setting (Interface tab) to enable or disable clickable text links (Artist, Album, Genre) in library and queue views, preventing accidental navigation.
- **Auditory Feedback** — implemented a "Play Sound on Finish" toggle for optional audio cues at the end of tracks.

### Fixes
- **Queue Link Gating** — ensured album and genre links in the queue view correctly respect the "Slot Text Links" configuration.

### Internal
- **Settings Persistence** — wired the slot text links toggle through the `SettingsService` for reliable state management.

## v0.2.15 — 2026-04-12

### Features
- **Crossfade Hotkey** — added a configurable global hotkey (default `F`) for toggling gapless crossfade mode.
- **TDD Parity** — comprehensive test-driven coverage achieved for hotkey dispatch, configuration serialization, and input normalization routines.

### Fixes
- **Settings UI Layout** — fixed string casing resolution to correctly display the "Sort & View" menu grouping in the hotkeys settings panel.

### Improvements
- **Audio Thread Sync** — streamlined synchronous backend transitions in `on_renderer_finished` by refactoring monolithic loops.
- **Audio Async Refactoring** — replaced aggressive async threading contexts in `engine.rs` specifically for crossfade and seeking optimizations with native event loops.

### Internal
- Synced agent rules and workflows with the current codebase.

## v0.2.14 — 2026-04-12

### Features
- **Navigation Architecture** — implemented id-based view filtering and album artist toggle to efficiently handle nested library associations.
- **Browsing Panel Tabs** — cross-view navigation clicks inside the browsing panel are now intelligently routed to internal tabs rather than breaking layout.
- **Custom Tooltips** — parameterized the `view_header` add button to allow contextual tooltips across different views.

### Fixes
- **Navigation Routing** — fixed a bug to correctly enable active filter routing for genres.

### Improvements
- **Cargo Metadata** — optimized package metadata for publishing and integrated funding link placeholder.
- **Formatting** — standardized internal import styling and widget comment layout.

## v0.2.13 — 2026-04-12

### Features
- **Background Library Refresh** — implemented an event-driven background refresh mechanism via Navidrome SSE that keeps the UI seamlessly in sync with server changes, preserving scrolling viewports.
- **Login Button Effects** — added hover and pressed interactive effects to the login button.

## v0.2.12 — 2026-04-11

### Features
- **Internet Radio Playback** — implemented full internet radio streaming support natively using the standard audio engine.
- **Radio Station Management** — implemented full CRUD capabilities for internet radio stations, including creating, editing, and deleting saved stations.
- **Radio Stream Codec Overlay** — extracted and surfaced actual live stream codec format (e.g. MP3, AAC) for radio playback instead of hiding zero values.
- **Radio MPRIS Integration** — implemented MPRIS navigation, auto-focus, and live ICY metadata propagation so the desktop shell natively displays live radio track changes.

### Fixes
- **Application Shutdown Hang** — eliminated a significant player freeze caused by the network logic blocking the Tokio runtime during app exit when an internet radio stream was active by shifting the stream network fetch into pure async tasks.
- **Radio Starvation** — eliminated network buffer accumulation delays to prevent radio stream playback starvation ("blips").
- **Radio Page Playback Viewport** — forced the radio page to respect the `stable_viewport` setting uniformly.
- **MPRIS Radio Guard** — updated MPRIS metadata for radio streams and added missing play guard protections across the codebase.
- **Settings Typography** — standardized modal typography and applied custom font configurations consistently to all modal text inputs.

### Improvements
- **Network Pipeline Refactoring** — hardened radio streaming architecture from audit findings to eliminate `unwrap()` usage and safely gracefully recover from decoder errors.
- **Slot Metrics Consolidation** — consolidated dynamic slot layout metrics into a centralized `SlotListRowMetrics` for greater UI consistency.
- **Pill Overlay Deduplication** — deduplicated metadata pill layouts and consolidated their builder logic throughout the codebase.
- **Player Bar Interactivity** — made the crossfade toggle button persistent in the player bar.

### Internal
- Synced agent rules and workflows with the current codebase architecture.
## v0.2.11 — 2026-04-09

### Fixes
- **Settings State** — persisted theme mode to correct config table so the application no longer reverts to dark mode on restart.

## v0.2.10 — 2026-04-09

### Features
- **Artwork Metadata Overlays** — unified and standardized artwork metadata overlays across the UI.
- **Crossfade Indicator** — added crossfade toggle indicator to player bar.

### Fixes
- **Format Technical Specs** — hid zero values for format technical specifications.
- **Settings State** — cleared stale footer description across exit/re-open cycles.
- **Artwork Text Contrast** — used absolute contrast colors for artwork overlay text.
- **Audio Engine Stability** — replaced production `unwrap()` calls with safe fallbacks.

### Improvements
- **UI Progress Bar Handle** — designed knurled pill ridges for rounded progress bar handle.
- **Project License** — updated project license to GPLv3.

### Docs & Internal
- Enforced max description length to prevent footer overflow and removed preamble from gradient mode descriptions to fit in footer.
- Add `--all-targets` to clippy commands for CI parity.
- Applied nightly rustfmt import guidelines, including new settings tests.

## v0.2.9 — 2026-04-08

### Features
- **Clickable Library Data** — made primary row titles (album, artist, song, genre) and inline counts clickable across all library views for navigation and expansion.
- **Album Overlay** — added a full-bleed album metadata overlay on large artwork with dynamic contrast detection.
- **Context Menus in Expansions** — added full context menu support to expanded child and grandchild slots across library views.
- **Unified Search** — added integrated clear button.
- **UI Shading** — added depth-based shading and indentation to slot list expansion.

### Fixes
- **Artwork Workflows** — fixed missing load actions upon expansion.
- **Hotkeys & Navigation** — prioritized settings escape over browsing panel dismissal and properly cleared stale selections on expansion to fix child highlighting.

### Improvements
- **Code Refactoring** — parameterized navigate and search functionality via shared common view actions to reduce boilerplate.

### Docs & Internal
- **Rule Strictness** — enforced strict clippy checks for CI parity and eradicated remaining architectural negative triggers.
- **Theme Document** — corrected default theme documentation to Adwaita.
## v0.2.8 — 2026-04-06

### Features
- **Similar Songs Enhancements** — finalized similar songs integration, including refined UI headers and batch operations in the context menu.
- **Find Similar/Top Songs Hotkeys** — added global configurable hotkey bindings (default `Shift+S` and `Shift+T`) to quickly find similar or top songs for the currently playing track's artist.

### Fixes
- **Settings Input Escaping** — prevented the escape key from being swallowed by focused text inputs, ensuring it consistently closes settings or modals.
- **Hotkey Suppressions** — safely suppressed global hotkey dispatch when text inputs have captured the key event, avoiding accidental actions while typing.

### Improvements
- **UI Refinements** — replaced the equalizer icon with a clean text label and removed deprecated queue movement and action variants.
- **Scalability** — performed an audit remediation for the UI and API client to improve overall application performance.

### Docs & Internal
- **Documentation Updates** — updated the readme with cache storage information, clarified contributing guidelines, and refreshed third-party licenses.
- **Rule Synchronization** — synced agent rules, incorporated red-green TDD protocols, and fixed clippy warnings.

## v0.2.7 — 2026-04-05

### Features
- **Algorithmic Radio Tabs** — added a "Similar" tab to the library browser that explicitly fetches "Similar Songs" and "Top Songs" via algorithmic backend queries (Navidrome/ListenBrainz). Right-clicking on any artist or song (in the library, queue, or now-playing strip) allows you to endlessly explore mathematically related discographies with full cross-pane drag-and-drop routing support.
- **Header Tooltips & Navigation** — added tooltips to UI headers and implemented a global "Center on Playing" shortcut for precise library navigation.
- **Server Version Awareness** — the About modal now reliably fetches and displays the actual Navidrome backend server version.

### Improvements
- **DRY Refactoring** — aggressively re-architected view layer boilerplate into domain specific helper macros, streamlining UI maintenance across all primary lists.

### CI & Internal
- **Dynamic CI Automation** — implemented robust GitHub Actions workflows and local pre-commit hooks to automatically test PRs, validate builds, and sync Navidrome backend version tracking natively into the README.
- **Node Environment** — sanitized CI logic and patched Node 20 deprecation warnings.
- **Readme Modernization** — upgraded the readme to support active feature showcases using high-resolution animated WebP images natively embedding real-time application renders.

## v0.2.6 — 2026-04-03

### Fixes
- **Theme Sync** — immediate UI synchronization when changing the active theme from settings.

### Improvements
- **Logo Aesthetics** — replaced complex SVG visualizer gradients with clean flat colors in both the application and desktop icons for a more minimalist, easily readable aesthetic.

### Internal
- **TDD Integration** — implemented comprehensive test-driven development suites covering backend state (`PagedBuffer`), data integrity, atomic configuration persistence, theme serialization, and settings invariants.

## v0.2.5 — 2026-04-03

### Features
- **Configurable Library Page Size** — added a new setting in the General tab to configure the number of items fetched per API request for library pagination, allowing you to optimize between loading frequency and memory footprint.
- **Modernized Login View** — refreshed the login screen with a sleek, card-based layout featuring a theme-adaptive Nokkvi logo and tagline.

### Fixes
- **Queue Reshuffling on Repeat** — fixed an issue where the queue would replay the exact same shuffle sequence when wrapping around in both "Shuffle" and "Repeat Playlist" modes. It now correctly generates a new randomized sequence each time the playlist loops.
- **Single-Item Selection State** — resolved a bug where the selection highlight was permanently cleared after using the "Center to currently playing track" (Shift+C) hotkey, ensuring consistent visual feedback for keyboard scrolling.
- **Log Batch Resolution Errors** — the cross-pane drag handler now safely attempts to perform batch actions and cleanly logs errors instead of crashing the UI when an operation attempts to act on stale items.
## v0.2.4 — 2026-04-02

### Features
- **Multi-Selection** — added multi-selection batch action support to the library and queue views. You can now select multiple items using Ctrl/Shift and apply context menu actions or hotkeys (e.g. Play Next, Remove from Queue) to the entire selection at once.
- **Batch Drag-and-Drop** — drag a multi-selection batch to reorder multiple tracks simultaneously natively within the queue, or drag a batch of songs from the library browser into the queue. A `×N` badge provides visual feedback for the batch size during the drag.

### Fixes
- **Library Multi-Selection Reliability** — prevented the cross-pane drag handler from improperly intercepting mouse clicks during multi-selection, ensuring that Ctrl and Shift-clicking consistently preserves selection state instead of reverting to single-item bounds.
- **Multi-Selection UI States** — the now-playing track highlight is suppressed cleanly while actively making multi-selections (holding Ctrl or Shift) to ensure selection limits are clearly readable. Batch operations clear the selection correctly afterward to prevent stale selections on the next action.
- **Multi-Selection Action Guards** — suppressed accidental play actions while building selections via Ctrl/Shift clicks, keeping UI state resilient.
- **Multi-Selection Deselection** — cleared the visual selection focus correctly when removing the last remaining item from a multi-selection batch using Ctrl+Click.

## v0.2.3 — 2026-04-02

### Fixes
- **Repeat track** — resolved an issue where playback would stop instead of continuously looping a single track, and fixed manual track skipping when repeat mode is active.

## v0.2.2 — 2026-04-01

### Features
- **Theme Font Decoupling** — decoupled font configuration from themes and introduced iced framework parity color palettes.

### Fixes
- **Large Artwork Loading** — resolved explicit slot index resolution for large album art to ensure consistent loading from UI interactions.
- **Theme Syncing** — synchronized internal ThemeFile defaults with the generated gruvbox parity theme and corrected the gruvbox visualizer gradient direction to cool-to-warm.

### Improvements
- **Theme Engine** — migrated the core ThemeFile struct defaults to Adwaita, and cleaned up legacy gruvbox color variants across the codebase.

### Internal
- Synced agent rules and workflows with current codebase.

---

## v0.2.1 — 2026-04-01

### Fixes
- **Playback transitions** — correctly re-peeks the next song after a gapless transition if the queue was mutated mid-playback.
- **UI borders** — the hover overlay border radius now correctly defaults to the theme's standard, fixing square highlights in rounded UI mode.
- **Track info display layout** — setting the metadata strip to "Player Bar" now works correctly when using the top navigation layout instead of remaining in the top bar.

---

## v0.2.0 — 2026-03-31

> **⚠️ Breaking Change:** The configuration architecture has been completely overhauled to use a global `config.toml` file and named theme files. It is highly recommended to delete your existing `~/.config/nokkvi/config.toml` file before running this version to prevent parsing errors and ensure clean defaults.

### Features
- **Verbose Configuration** — introduced a verbose configuration mode that saves even default settings to `config.toml`, guaranteeing total consistency between UI controls and the persistence file.
- **Configurable Settings Migration** — all user configuration preferences (Playback, Hotkeys, General, Theme, Visualizer) are now entirely hosted in the hot-reloadable `config.toml` file, eliminating reliance on local `redb` state for settings.
- **Hardware Volume Integration** — real-time volume synchronization with PipeWire stream channel volumes (`SPA_PROP_channelVolumes`).
- **Named Theme System** — migrated to named TOML file themes (`~/.config/nokkvi/themes/`) for robust theme management and live configuration reloading.
- **Queue Album Column** — the queue view now always shows the album column for better context during playback.
- **Settings color editors** — editing color fields intuitively applies the hex code on "Enter" without needing a secondary confirmation click.
- **Visualizer hot-reload** — visualizer settings such as "fill opacity" and "noise reduction" now hot-reload instantly.
- **Theme default restoration** — "Restore Defaults" now correctly pulls values specific to the active named theme instead of global application defaults.
- **Equalizer flat mode** — flat presets now synchronize with the EQ's global toggle state, rather than fighting it.
- **Progress bar rendering** — expanded the render clip bounds so the progress bar thumb handle no longer cuts off at the bottom.
- **Font picker** — selecting a new font now instantly hot-reloads the application without requiring a restart.
- **Float serialization noise** — floating-point values are now clipped to 4 decimal places when serialized to `config.toml`.

### Improvements
- **Standardized Hotkey Formats** — upgraded from legacy non-standard Unicode artifacts (e.g., arrows) to formal ASCII identifiers (e.g., `RightArrow`, `UpArrow`) for resilient TOML parity.
- **Equalizer UX** — removed the redundant 'Flat' preset and implemented auto-enable logic when selecting any non-flat preset.
- **Performance & Linting** — significant codebase auditing to resolve clippy lints, remove dead code, and improve DRY architecture.

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
