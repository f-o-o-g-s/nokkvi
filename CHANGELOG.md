# Changelog

## v0.5.2 — 2026-03-15

### New Features
- **Horizontal volume controls** — new layout option for volume sliders in the
  player bar. When enabled, main and SFX volume sliders stack horizontally with
  height matching adjacent buttons when both are visible. Configurable in
  Settings → General → Application.

### Fixes
- **Crossfade replay with shuffle+consume** — crossfade no longer replays the
  same track when shuffle and consume modes are both enabled.
- **Current track desync on gapless/crossfade** — `current_index` now syncs to
  the actually-playing track during gapless and crossfade transitions, preventing
  UI from showing a different track than what's audible.
- **Visualizer volume independence** — visualizer FFT input now receives
  pre-volume samples, restoring volume-independent spectrum display.

### Internal
- Agent rules and workflows synced with current codebase.
- Added `/sync-rules` and changelog generation workflows.

---

## v0.5.1 — 2026-03-14

### New Features
- **Settings tab reorganization** — split the General tab into General
  (Application, Mouse Behavior, Account, Cache) and Playback (Playback,
  Scrobbling, Playlists) for better organization and scalability.
- **Quick-add to playlist** — new "Set as Default Playlist" context menu on
  playlists. When a default is set and "Quick Add to Playlist" is enabled in
  Settings → Playback → Playlists, Add to Playlist actions skip the picker
  dialog and add directly with a toast confirmation.
- **Self-documenting config.toml** — settings written via the UI now
  automatically inject descriptive TOML comments from the setting's subtitle.

### Fixes
- **Play Next with search filter** — context menu "Play Next" now uses the
  correct raw queue index instead of the filtered list index.
- **TOML comment injection** — comments are now added to existing config keys,
  not just newly created ones.

---

## v0.5.0 — 2026-03-14

Major audio engine overhaul: migrated to rodio, replaced vendored C dependencies
with pure Rust, and improved audio quality.

### New Features
- **Rodio audio engine** — replaced the cpal-based audio pipeline with rodio,
  providing a cleaner abstraction over audio output and source composition.
- **Pure-Rust visualizer (RustFFT)** — replaced the vendored cava C library and
  FFTW3 FFT with a pure-Rust `SpectrumEngine` built on RustFFT. Eliminates the
  `clang`, `bindgen`, and `fftw` build dependencies entirely.
- **Peak limiter** — `rodio::source::Limit` with `dynamic_content()` preset
  prevents audio clipping on loud tracks.
- **Perceptual volume curve** — `amplify_normalized()` replaces linear volume
  scaling for a more natural-sounding volume control.


### Fixes
- **Stale audio on track change** — eliminated race condition where audio from
  the previous track could leak into the new track.
- **Crossfade audio cutouts** — fixed backpressure bug in the crossfade decode
  loop that caused buffer underruns during transitions.
- **Crossfade trigger race condition** — crossfade no longer misfires after
  seeking or during shuffle transitions.
- **Progressive queue flash animation** — suppressed spurious slot list flash
  animations during progressive queue page loading.
- **Graceful SFX degradation** — SFX engine now handles missing audio devices
  gracefully instead of panicking.
- **Zero-allocation visualizer callback** — `audio_callback` now accepts
  `&[f32]` and converts inline, eliminating heap allocations on the audio thread.

### Polish
- **Post-migration audit cleanup** — DRY improvements and code quality fixes
  identified during the rodio migration audit.
- **Dependency upgrades** — thiserror 2, bincode-next 2.1, toml 1.0,
  toml_edit 0.25, mpris-server 0.9, rand 0.10, and all compatible deps updated.
- **Reduced system dependencies** — build no longer requires `clang`, `fftw`,
  or `pkg-config` (only `libpipewire` and `fontconfig` remain).

---

## v0.4.2 — 2026-03-13

Bugfix release.

### Bug Fixes
- **Fixed waves smoothing amplitude inflation** — the waves filter was using
  max-based windowed subsampling for spline control points, which inflated bar
  heights like a multiplier. Now uses direct point sampling to preserve the
  original amplitude envelope.
- **Decoupled smoothing from lines mode** — waves and monstercat smoothing
  filters now only apply in bars mode. Lines mode does its own GPU-side
  Catmull-Rom smoothing, so CPU-side filters were causing unwanted amplitude
  changes.
- **Updated settings descriptions** — Waves Smoothing, Waves Intensity, and
  Monstercat Smoothing descriptions now clarify they are bars-mode-only.

### Polish
- **Fixed clippy lint in vendored cava-sys** — replaced `vec![]` with array
  literal to satisfy new `useless_vec` lint in Rust 1.93.

---

## v0.4.1 — 2026-03-13

Bugfix release.

### Bug Fixes
- **Fixed audio continuing after logout** — logging out now properly stops
  the audio engine (PipeWire streams, decode loop, render thread). Previously,
  audio kept playing in the background after logout, and logging back in
  created a second overlapping audio pipeline.

## v0.4.0 — 2026-03-13

Changes since v0.3.7 (last packaged release).

### New Features
- **Dual-stream crossfade** — smooth track-to-track crossfade using two
  persistent PipeWire streams with dynamic backpressure timing. Pre-created
  streams eliminate first-transition audio pops.
- **Settings GUI revamp** — centered drill-down panel with breadcrumb
  navigation, inline search, Enter hints, enhanced subtitles, confirmation
  dialogs for visualizer/hotkey resets, and font modal overlay.
- **File-based logging** — application logs now written to
  `~/.config/nokkvi/nokkvi.log` for debugging.
- **Slot list hover overlay** — subtle hover highlight on all slot list rows
  with press darkening and scale-down micro-animation on activation.
- **Center slot flash animation** — visual flash feedback on center slot
  when activated via Enter, play/next/prev buttons, or MPRIS.
- **Top bar track info strip** — full-width marquee text widget showing
  codec, sample rate, bitrate, title, artist, album above the content area.
- **Configurable peak fall speed** — new visualizer setting to control
  the speed at which peak bars descend in fall/fall_accel modes.
- **Catmull-Rom spline interpolation** — replaces the old waves filter with
  smooth spline-based bar interpolation; also available as monstercat
  post-smoothing.
- **Play Next shuffle warning** — toast notification when using Play Next
  while shuffle mode is active.
- **Restore previous view on Settings close** — returning from Settings now
  navigates back to the view you were on instead of defaulting to Queue.
- **Confirmation dialogs** for visualizer and hotkey "Restore Defaults".
- **MPRIS volume toast** — volume changes via MPRIS now show the same toast
  as scroll-to-adjust.
- **Configurable opacity gradient toggle** — disable the slot list edge fade
  in Settings.

### Fixes
- **Crossfade sample rate mismatch** — crossfade stream reuse no longer
  ignores sample rate changes between tracks (e.g. 44.1 kHz → 48 kHz).
- **Crossfade shuffle/seek misfire** — crossfade no longer triggers
  incorrectly after seeking or during shuffle transitions.
- **Duplicate track queue bugs** — clicking a specific duplicate now plays
  that instance; consume mode removes only the played copy; UI scrolls to
  the correct duplicate.
- **Consume mode race conditions** — fixed UI/audio desync in
  consume+shuffle and stale queue display during gapless transitions.
- **Gapless + consume race** — preparation flag now resets correctly on song
  change, preventing duplicate decode loops.
- **Queue remove (Ctrl+D) desync** — removing a track no longer desyncs
  `current_index` from the playing track.
- **Settings search navigation** — filtered results preserved on
  SlotListDown; search unfocuses on Tab like regular views.
- **Settings click activation** — arrow/option clicks on non-center rows no
  longer modify the wrong setting; scrollbar drag no longer activates edit
  mode.
- **Boolean settings** now respond to mouse clicks.
- **FFTW thread-safety crash** — all FFTW plan creation moved to the FFT
  thread to prevent concurrent access.
- **Visualizer shimmer flash** — uses the gradient palette instead of
  hard-coded white.
- **Monstercat smoothing** — clamped to effective range (0.0 or 0.7+) to
  prevent artifacts.
- **Waves filter scaling** — corrected bar height scaling; mutual
  exclusivity synced in the GUI.
- **MPRIS Position/Seeked** — Position property and Seeked signal now
  update correctly.
- **Progressive queue count** — stale page count display prevented.
- **Views stuck in Loading** — loading state no longer gets stuck after
  certain navigation patterns.
- **Pool load resilience** — corrupted song pool data no longer prevents
  login.
- **Marquee scroll gap** — FillPortion layout for proportional space
  distribution.
- **Dynamic artwork prefetch** — covers all views and window sizes.
- **Nav bar separators** — trailing-only in rounded top-nav mode; metadata
  separators shown in rounded mode; stopped-state layout fixed.
- **Settings footer** — only rounds bottom corners in rounded mode.
- **Narrow scrollbar** — slot list scrollbar narrowed to avoid overlapping
  the loved/star column.

### Polish
- **SongPool refactor** — song data split from queue ordering; persistence
  migrated to bincode for performance; centralized ID→index lookups.
- **Settings handler split** — `handle_settings()` refactored from a 529-line
  monolith into focused sub-handlers (~120-line dispatcher).
- **Service initialization** — replaced `Mutex<Option<T>>` with `OnceCell`
  for lazy-init; eliminates pre-login race conditions.
- **DRY metadata strip** — marquee scrolling shared between side and top nav
  modes via generic `MarqueeText` widget.
- **Crossfade code quality audit** — cleanup of crossfade-era code.
- **Audio cleanup** — removed `AudioHealthMonitor` subsystem; removed dead
  async from engine methods; defensive duration casts.
- **Settings cleanup** — removed legacy `SettingsTab1-4` hotkey actions;
  removed GUI revamp design docs; code quality improvements.
- **Theme cleanup** — removed unused color accessors and struct fields.
- **18 new tests** — cross-view sync, scrobble, sort stability, and toasts.
- **Settings inline search** always visible with accent styling.
- **Settings row separators** and enhanced ExpandCenter description.
- **Settings exit button** restyled with accent background and hover effect.

---

## v0.3.7 — 2026-03-08

### New Features
- **Dynamic slot sizing** — slot list rows now scale dynamically with window
  height using a target row height algorithm. Taller windows show more rows
  instead of comically large slots; short windows gracefully reduce.
- **Configurable slot row height** — new "Slot Row Height" slider in
  Settings → General → Application (40–120px, step 5, default 70). Controls
  the target height per row in all slot lists. Smaller values show more rows.

---

## v0.3.6 — 2026-03-08

### New Features
- **Side navigation layout** — optional vertical sidebar with view tabs on the left
  (Settings → General → Nav Layout). Supports three display modes: Text Only,
  Text + Icons, Icons Only. Hamburger menu in player bar provides access to
  settings, light/dark toggle, SFX toggle, and quit in side nav mode.
- **Nav bar icons** — SVG icons for all navigation tabs (queue, albums, artists,
  songs, genres, playlists). Configurable in Settings → General → Nav Display Mode.
- **Track info strip** — now-playing metadata strip with three placement modes:
  Off, Player Bar (inline), Top Bar (full-width above content). Shows codec,
  sample rate, bitrate, title, artist, and album with color-coded fields.
  Configurable in Settings → General → Track Info Display.
- **Visualizer opacity** — new `visualizer.opacity` setting in config.toml
  (0.0–1.0) for transparent visualizer overlays.
- **Scroll-to-adjust volume** — scroll anywhere on the player bar to change
  volume. Shows a right-aligned toast with current percentage.
- **Now-playing accent color** — optional `accent.now_playing` color in
  config.toml for the currently-playing slot highlight (falls back to primary).
- **Selected slot accent color** — optional `accent.selected` color in
  config.toml for the center/selected slot (falls back to bright).
- **Star/heart icon outlines** — filled star and heart icons now have a dark
  outline layer for readability across all themes.
- **Spacebar play on cold start** — pressing play after restarting the app
  with a persisted queue now resumes from the last-loaded track instead of
  doing nothing.
- **Empty queue toast** — pressing play with an empty queue shows a
  "Queue is empty" notification.
- **Package build tracking** — `package.sh` now embeds the git commit hash
  in the zip filename and includes a `BUILD_INFO` file for traceability.

### Fixes
- **Window resize crash** — clamped artwork panel dimensions to ≥ 0, preventing
  a panic when resizing the window very small.
- **Text input selection readability** — semi-transparent accent highlight
  (35% alpha) replaces opaque selection background, keeping text readable
  across all themes (e.g. Everforest peach-on-green).
- **TopBar chrome height** — track info strip height is now accounted for in
  layout calculations, preventing slot list overflow.
- **Scrollbar system lockup** — scrollbar drag on large queues (12k+ items)
  no longer freezes the system. Fast-path seek handler avoids O(n) clones per event.
- **Scrollbar viewport tracking** — viewport now updates correctly during
  handle drag operations.
- **Scrollbar proportional sizing** — handle scales proportionally with content,
  capped at 40% of track height. Always-visible track in slot list area.
- **Scrollbar track width** — scales dynamically with window size and row height.
- **Cross-pane drag indicator** — width and position corrected for side nav layout.
- **Iced upgrade** — updated to b655cb6e with native text ellipsis support.
- **Visualizer redraws** — gated on dirty flag to reduce compositor contention
  and GPU load.
- **Empty slot list placeholders** — now have border/background instead of raw text.
- **Volume toast alignment** — text is right-aligned to match volume slider position.

### Polish
- **Native text ellipsis** — hand-rolled truncation (~130 lines of char-width
  heuristics) replaced with Iced's `Ellipsis::End` for accurate rendering.
- **Slot list rows** dynamically fill the viewport height (no empty trailing slots).
- **Scrollbar overlay style** — modern transient design with accent-colored handle,
  dark border, and 1px border.
- **Rounded corners** setting moved from General to Theme tab.
- **DRY refactors** — shared slot styling, chrome height constants, toast helpers,
  theme separators, player bar decoupled from track metadata, outlined_svg_icon
  helper, 3D bevel helper, active_accent() helper.
- **`update/mod.rs`** split into focused handler modules; `settings/mod.rs`
  modularized into entries.rs, items_general.rs, items_theme.rs, items_visualizer.rs,
  items_hotkeys.rs.
- **Artwork panel borrows** — `&Handle` references instead of cloning for
  better performance.
- **Three clippy suppressions** addressed (removed or upgraded to `expect`).
- **Legacy code removed** — concave corner widget, `check_completion()` dead method,
  encrypted_password cleanup routine, 'flash' gradient mode alias, 'backwards-compatible'
  misleading doc comments.
- **Cavacore** updated to latest upstream; added upstream tracking file and update
  check script.

---

## v0.3.5 — 2026-03-04

Changes since v0.3.2 (last packaged release).

### New Features
- **Rounded mode** — new toggle in Settings → General. Applies rounded borders to
  buttons, artwork, and UI elements throughout. In rounded mode the nav tabs switch
  to an underline indicator style (active tab highlighted with an accent underline)
  and slider handles gain a rounded grip.
- **Concave (gothic) corner ornaments** — in rounded mode, decorative concave
  corners appear at the slot list / artwork column junction for a polished look.
- **Get Info hotkey (Shift+I)** — opens a rich info modal for the selected track,
  album, or artist. The modal shows all metadata fields, biography, tags, and lets
  you select and copy any value. Press Escape to close.
- **Three-tier expansion for Artists and Genres** — expand an artist or genre to
  browse its albums, then expand an album to browse its tracks, all inline.
- **Open in file manager** — context menu and info modal "Show in Folder" option.
  Requires the Local Music Path to be configured in Settings → Application.
- **Playlist save button** — a 💾 button appears in the playlist context bar
  (shown when a playlist is playing). Clicking it opens the save-as dialog to
  overwrite or fork the playlist.

### Fixes
- Volume slider: percentage tooltip shown during drag prevents the overlay
  from disappearing and resetting the drag mid-gesture.
- Scrobble threshold is now a pure percentage (0–100) — no longer misinterpreted
  as a fraction.
- Context menu: now closes on Escape key.
- Info modal: strips HTML tags from biography text; scrollbar themed correctly.
- Visualizer bars fill the full window width when bar spacing is set to zero.
- Peak bars no longer fall below their parent bar.
- Nav bar rounded mode: consistent 2px separator, correct light/dark accent colour.
- Clearing the queue with Shift+D now correctly hides the playlist context bar.
- Rounded mode corner-clipping artifacts fixed on several full-width header strips
  (queue edit bar, playlist bar, library browser tab bar).

### Polish
- Edit mode bar in the queue view: save/discard are now icon-only buttons
  (matching the playlist bar style); confusing yellow/green dirty indicator removed.
- Nav tab readability improved in light + rounded mode.
- Settings page: icon and subtitle fields audited and corrected throughout.
- Various internal refactors and file splits for maintainability.
