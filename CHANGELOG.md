# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

### Changed

- Replaced the app logo with a new Viking longship mark across the About modal, login page, visualizer boat, README, and desktop/tray icons. The in-app logo and boat recolor with the active theme and stay consistent in both light and dark mode; the OS and tray icons use the Svalbard default palette.

### Fixed

- Nokkvi now registers as a single player in Navidrome instead of appearing as two separate ones.
- The Radio view now gives the currently-playing station the same breathing glow as the now-playing track elsewhere, instead of leaving it unmarked.

### Removed

## v0.6.8 — 2026-06-03

### Added

- Optional "Rewind on Previous" setting (off by default): the Previous button restarts the current track instead of skipping back once it's played past 5 seconds.

### Changed

- Nokkvi's default theme is now **Svalbard**, a custom cold-petrol Nordic palette created for the project. Everforest is still available as a built-in theme; it is simply no longer the default. Configs that already pin a theme are unaffected, and new installs start on Svalbard.
- The application icon (desktop launcher and system tray) was redesigned for Svalbard. The longship-and-visualizer mark now sits on a transparent ground with a dark outer stroke, replacing the squircle backdrop and the earlier Everforest-coloured version.

### Fixed

- Removing the currently-playing track while shuffle is on no longer skips a track or stops playback early with songs still left to play.
- If a playing track was deleted from the server between sessions, restoring the queue on the next launch now resumes at the neighbouring track instead of jumping to the end and stopping after one song.
- With shuffle on, pressing Previous after relaunching the app now steps back through the shuffled play order instead of the on-screen row order, so it no longer jumps to an unrelated track.
- Pressing Previous now returns to the exact track that played before, even when two copies of the same song sit next to each other in the queue.

## v0.6.7 — 2026-06-01

### Changed

- The show/hide columns menu now uses the same boxed checkbox style as the player bar menu and library filter, instead of plain checkmarks.

### Fixed

- The empty (unchecked) checkboxes in the list-view Select column now have a soft, readable outline in every theme, instead of a near-black frame that blended into the row and was hard to see.
- In rounded mode, list views no longer show a stray lighter patch tucked into their top-left or bottom-left corner.
- Star ratings now shrink as a group on narrow columns instead of scaling only the last star, so 5 stars no longer read as 4.
- The now-playing row's shimmer animation no longer floods the log with repeated gradient-range warnings.

## v0.6.6 — 2026-05-31

### Added

- The currently playing track now has a gentle breathing glow: a soft inner light pulses along its top and bottom edges and a shimmer periodically sweeps across it, both in your theme's accent color so the effect fits every theme.

### Changed

- The divider between the list and the artwork panel is now a thin line matching the tab bar's underline, instead of a thicker, lighter band.
- The now-playing row now uses the same fill color as the selection highlight, instead of a separate, louder green that clashed with the palette; the breathing glow is what marks the playing track.

### Removed

- The bright border ring around the now-playing row is gone; its breathing glow and shimmer set it apart instead. Regular selection highlights keep their ring.

## v0.6.5 — 2026-05-31

### Added

### Changed

### Fixed

- The player bar's playback and mode-toggle buttons now show the theme accent highlight on hover instead of a flat grey one.
- Pausing during a crossfade now resumes the fade where it left off, instead of jumping ahead, hard-cutting to the next track, or freezing the queue.
- A crossfade into a track that fails to load now recovers and moves on, instead of fading into silence.
- Toggling shuffle, repeat, or consume during a crossfade now cancels the fade cleanly instead of switching to the wrong track.
- The visualizer now reflects the source audio regardless of your equalizer settings, instead of being shaped by EQ.
- The equalizer now applies to a track that was already playing when you turn EQ on, instead of staying flat until the next track.
- The saved queue is now reconciled on load, so a restored queue no longer points at the wrong current track or silently drops songs.
- A corrupted saved queue now recovers to an empty queue on startup instead of bouncing you to the login screen.
- Repeat-one no longer turns itself off after a relaunch when you manually skipped a track during the session.
- With repeat-all on, pressing Previous on the first track now wraps to the last track instead of doing nothing.
- Consume mode now plays and removes the final track and stops cleanly, and no longer loops forever when repeat-one is also on.
- Skipping manually under consume now removes the track you skipped, even if the queue changed while the skip was in flight.
- Reordering the queue while shuffle is on no longer re-randomizes the rest of the queue or changes what plays next.
- Live updates are more robust: the connection backs off between reconnect attempts and no longer keeps reconnecting to a previous server after logout.
- A server-pushed library refresh no longer interrupts an in-progress playlist edit or drag by resetting scroll and selection.
- After a background library refresh, a multi-selection is now cleared so actions like Add to Queue or Play can't target the wrong songs.
- Hand-editing a config or theme file now reliably reloads, even right after the app saved a different setting.
- Dragging a song into the queue while a search filter is active now works instead of being silently cancelled.
- In split view, arrow-key navigation, Get Info, and roulette now act on the focused browser tab instead of the wrong pane.
- Typing a capital letter in a search or text field no longer triggers letter shortcuts like Clear Queue or star.
- Keyboard shortcuts no longer act on the view behind an open modal (EQ, Info, About, playlist picker); only Escape passes through.
- Scrobbling now covers long-form content at the 4-minute mark and no longer drops a short track played right before a long one.
- Scrobbles are no longer submitted more than once, and a failed scrobble now retries instead of being silently lost.
- Seeking forward within a track no longer counts the skipped span as listening time toward a scrobble.
- Quickly toggling shuffle, repeat, or consume no longer briefly snaps back to the old state before applying.
- During radio playback, the shuffle, repeat, and consume buttons now appear dimmed and no longer quietly change your saved library queue.
- If playback fails to start, the player no longer shows "playing"; it surfaces an error instead.
- Skipping between radio stations after the current one disappears no longer jumps to an unrelated station.
- Logging out now finishes pending writes before clearing your session, so a stale queue can't reappear on the next launch.
- An album cover changed on the server now refreshes in the Albums, Artists, and Genres views instead of showing the old art until restart.
- Playing a playlist from a chosen track now follows the same edit-mode and radio rules as other play actions.
- Editing a playlist's comment no longer rewrites its name or reverts a public/private change made on the server.
- The playlist editor no longer overwrites the whole playlist before it finishes loading, so adding songs to a still-loading playlist is safe.
- Saving a playlist with no track changes no longer rewrites the entire track list.
- If a playlist changed on the server while you had it open in the editor, saving now warns you to reload instead of overwriting those changes.

### Removed

## v0.6.4 — 2026-05-29

### Changed

- Hover and press highlights now follow the theme's accent color, making them clearly visible in light mode instead of nearly invisible.
- Selected and now-playing row highlights are now derived from each theme's accent colors, keeping every element on the highlighted row legible on every theme.
- The now-playing row now stays visibly distinct from the keyboard-selected row, fixing the muddy grey selection on Everforest light and the unreadable now-playing row on Kanagawa Dragon dark.
- The status strip background in light mode now tints toward the theme's text color instead of darkening toward black, so it stays on-palette instead of dingy; dark mode is unchanged.

### Fixed

- Tabbing through long settings tabs (Hotkeys, Theme, Visualizer) now keeps the selected row centered in view instead of letting it scroll out of sight.
- The queue's "Playing From" banner no longer stays stuck expanded after you enter or leave the playlist editor; it collapses again when not hovered.
- Removing the current song from a stopped queue (after a restart or pressing Stop) no longer kicks playback into starting on its own; the queue stays stopped until you press play.
- Status strip metadata text (title, artist, album) is now guaranteed readable against the strip background in both light and dark mode, fixing the hard-to-read strip text on Kanagawa Dragon dark.

### Removed

- The per-theme `now_playing` and `selected` accent swatches were removed from the theme editor, since those highlights are now derived automatically; existing theme files still load unchanged (the fields are kept for compatibility).

## v0.6.3 — 2026-05-28

### Added

- New "Player Only" rounded-corners option keeps the playback bar soft while the rest of the UI stays flat.
- A sticky pill mini-index above the settings detail pane lets you jump to each sub-section; hidden on tabs with only one section.

### Changed

- Appearance toggles (Theme Mode, Rounded Corners, Opacity Gradient) now sit at the top of the Theme settings tab instead of below the theme picker.
- Settings sidebar and detail pane now show a 1 px hairline between them in the wide layout.
- Settings sub-section headers are larger, bolder, and show an item count (e.g. APPLICATION (8)) so sections read as architecture instead of metadata.
- A fading accent rule now flags each settings sub-section header, extending from the label and dissipating into the pane background.
- Each settings tab's first section now uses concrete domain headings (Library, Navigation, Transitions, Frame, Mode) instead of generic Application/Layout/Appearance/General.
- Track Info Display moved into the Metadata Strip section next to the other strip controls.
- Visualizer's Waves, Waves Intensity, and Monstercat smoothing knobs moved from General into the Bars section.
- The queue's "Playing From" bar is now a banner with cover art and a hover-expand block showing the playlist's comment, song count, duration, and visibility.
- Editing a playlist now opens a dedicated editor view with an "Editing" nav pill, so you can leave and return mid-edit.

### Fixed

- About modal's Ko-fi heart now uses the theme's love color (usually red) instead of the accent color, matching hearts elsewhere in the app.
- Editing a playlist no longer replaces your play queue; your music keeps playing while you reorder, remove, or add songs.

## v0.6.2 — 2026-05-27

### Added

- Numeric settings sliders are now click-and-drag, with fixed-width value badges so rows line up.

### Changed

- First-launch defaults retuned: crossfade enabled (7s), rounded mode, compact slot rows, player-bar track-info strip, merged metadata with slash separator, opacity gradient off, library-refresh toasts suppressed, SFX off, and a flat LED-bars visualizer (lines mode drops to 8 points).
- Narrow-mode Settings category strip scales its chips to fit the window width, with label sizes shrinking at the same breakpoints as the top nav instead of clipping off the right edge.

## v0.6.1 — 2026-05-26

### Changed

- Clicking a setting row no longer plays the activation sound effect (value-change feedback is unchanged).

### Fixed

- Metadata-strip text (title, artist, codec, bitrate) is now legible on light themes.
- Toggling light mode in Settings no longer reverts to dark when any other setting is changed.
- Left-clicking a settings row no longer scrolls the detail pane when stable viewport is enabled (Tab and Backspace still scroll).

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

## Older releases

- **v0.5.x** (2026-05-21 → 2026-05-24, v0.5.0–v0.5.3): [CHANGELOG-0.5.md](./changelog-archive/CHANGELOG-0.5.md)
- **v0.4.x** (2026-05-16 → 2026-05-19, v0.4.0–v0.4.2): [CHANGELOG-0.4.md](./changelog-archive/CHANGELOG-0.4.md)
- **v0.3.x** (2026-04-27 → 2026-05-14, v0.3.1–v0.3.17): [CHANGELOG-0.3.md](./changelog-archive/CHANGELOG-0.3.md)
