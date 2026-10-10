# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- Text fields now undo and redo with Ctrl+Z and Ctrl+Y.
- The raw rules JSON editor now deletes a whole word with Ctrl+Backspace or Ctrl+Delete.
- Bars and Lines have a new Reflection Below Cover setting, on by default.

### Changed

- Beside the artwork column, the player bar now spans only the list.
- The artwork column beside the list now runs down to the window's bottom edge.
- With the Metadata Strip on Top Bar or Top Bar Under, the strip now spans only the list beside the artwork column.
- Toasts now sit over the list instead of across the artwork column.
- The artwork's resize handle is now invisible, marked only by the resize cursor at the artwork's edge beside the list.
- Mouse-wheel scrolling in Settings, the Get Info dialog and the rules editor now glides instead of jumping.
- Hotkey badges in Settings are now at least 96 pixels wide.
- With the Metadata Strip on Top Bar or Top Bar Under, the top nav bar now spans only the list beside the artwork column.
- With the Metadata Strip on Top Bar or Top Bar Under, the artwork column beside the list now reaches the window's top edge.
- With Reflection Below Cover on, the Reflection's water now hangs below the cover over the UI, where there is room for it.
- With Reflection Below Cover on, Bars and Lines now stand on the cover's bottom edge and fill Visualizer Height when their water fits below it.

### Fixed

- Typing in a text field no longer briefly maxes out a CPU core after each keystroke.
- Resizing the window no longer crashes the app while the rules JSON editor's cursor is scrolled out of view.
- The Horizon's receding waves and bars now crest above the visualizer instead of being cut off along a straight line at its top.
- Typing in the Queue's search while a playlist plays no longer knocks focus out of the field after the first letter.

### Removed

## v0.23.0 — 2026-10-09

### Added

- The new Tunnel setting for Scope spirals about the last second of the spectrum down into the cover behind the ring.
- In Scope's Tunnel, kicks light up rings that then fall away down it.
- With Echo on, Scope's Tunnel stays sharp while the ring's echo swirls over it.

### Changed

- The queue's "Playing From" playlist now sits inside the toolbar instead of on its own bar above it.
- The "Playing From" cover now lines up with the start of the queue's rows.
- The "Playing From" cover now measures 16 pixels in the slim toolbar strip and 32 in the full toolbar.
- The "Playing From" playlist's save and edit buttons are now regular toolbar buttons.
- The "Playing From" playlist's save and edit buttons now sit right after its name.
- The "Playing From" playlist's save and edit buttons now hide with the toolbar.
- The "Playing From" playlist name no longer jumps sideways when the cover finishes loading.
- With the toolbar hidden, the Count strip now leads with the playing playlist's cover and name.
- With Collapsed Appearance set to Hairline or Hidden, the "Playing From" playlist now hides with the toolbar.
- The "Playing from playlist" label now appears only as the cover's tooltip.
- A smart playlist's sparkles mark now follows its name in the full toolbar.
- The "Playing From" playlist's details now open after a brief hover on its cover or name.
- The "Playing From" playlist's details now open below the toolbar.
- In narrow windows, the full toolbar shrinks the "Playing From" name first, then hides the song count, then shows only the cover.
- Scope now shows the Tunnel by default.
- Scope's Echo now defaults to off.
- Scope's Fill setting has no effect while the Tunnel is on.

### Fixed

- The Lines Horizon now keeps its receding waves as dim contour lines where the music is quiet, instead of leaving gaps.
- The Bars Horizon now keeps a short stub of every receding bar where the music is quiet, instead of leaving gaps.
- The "Playing From" cover now shows a playlist's uploaded image instead of its album collage.
- After a restart, the "Playing From" playlist's details now refresh from the server without opening the Playlists view.
- With Navigation Layout set to Side, toasts no longer cover the bottom of the sidebar's tabs.
- The "Playing From" playlist's details no longer stay open after switching views or entering Theater Mode.

## Older releases

- **v0.22.x** (2026-10-04 → 2026-10-07, v0.22.0–v0.22.2): [CHANGELOG-0.22.md](./changelog-archive/CHANGELOG-0.22.md)
- **v0.21.x** (2026-09-25 → 2026-10-02, v0.21.0–v0.21.2): [CHANGELOG-0.21.md](./changelog-archive/CHANGELOG-0.21.md)
- **v0.20.x** (2026-09-23, v0.20.0): [CHANGELOG-0.20.md](./changelog-archive/CHANGELOG-0.20.md)
- **v0.19.x** (2026-09-20 → 2026-09-21, v0.19.0–v0.19.1): [CHANGELOG-0.19.md](./changelog-archive/CHANGELOG-0.19.md)
- **v0.18.x** (2026-07-19 → 2026-07-25, v0.18.0–v0.18.4): [CHANGELOG-0.18.md](./changelog-archive/CHANGELOG-0.18.md)
- **v0.17.x** (2026-07-18, v0.17.0): [CHANGELOG-0.17.md](./changelog-archive/CHANGELOG-0.17.md)
- **v0.16.x** (2026-07-15, v0.16.0): [CHANGELOG-0.16.md](./changelog-archive/CHANGELOG-0.16.md)
- **v0.15.x** (2026-07-09 → 2026-07-11, v0.15.0–v0.15.1): [CHANGELOG-0.15.md](./changelog-archive/CHANGELOG-0.15.md)
- **v0.14.x** (2026-07-04 → 2026-07-06, v0.14.0–v0.14.2): [CHANGELOG-0.14.md](./changelog-archive/CHANGELOG-0.14.md)
- **v0.13.x** (2026-07-03, v0.13.0): [CHANGELOG-0.13.md](./changelog-archive/CHANGELOG-0.13.md)
- **v0.12.x** (2026-06-28 → 2026-07-02, v0.12.0–v0.12.2): [CHANGELOG-0.12.md](./changelog-archive/CHANGELOG-0.12.md)
- **v0.11.x** (2026-06-22 → 2026-06-25, v0.11.0–v0.11.3): [CHANGELOG-0.11.md](./changelog-archive/CHANGELOG-0.11.md)
- **v0.10.x** (2026-06-19 → 2026-06-21, v0.10.0–v0.10.1): [CHANGELOG-0.10.md](./changelog-archive/CHANGELOG-0.10.md)
- **v0.9.x** (2026-06-15 → 2026-06-18, v0.9.0–v0.9.4): [CHANGELOG-0.9.md](./changelog-archive/CHANGELOG-0.9.md)
- **v0.8.x** (2026-06-14, v0.8.0): [CHANGELOG-0.8.md](./changelog-archive/CHANGELOG-0.8.md)
- **v0.7.x** (2026-06-07 → 2026-06-10, v0.7.0–v0.7.2): [CHANGELOG-0.7.md](./changelog-archive/CHANGELOG-0.7.md)
- **v0.6.x** (2026-05-25 → 2026-06-06, v0.6.0–v0.6.10): [CHANGELOG-0.6.md](./changelog-archive/CHANGELOG-0.6.md)
- **v0.5.x** (2026-05-21 → 2026-05-24, v0.5.0–v0.5.3): [CHANGELOG-0.5.md](./changelog-archive/CHANGELOG-0.5.md)
- **v0.4.x** (2026-05-16 → 2026-05-19, v0.4.0–v0.4.2): [CHANGELOG-0.4.md](./changelog-archive/CHANGELOG-0.4.md)
- **v0.3.x** (2026-04-27 → 2026-05-14, v0.3.1–v0.3.17): [CHANGELOG-0.3.md](./changelog-archive/CHANGELOG-0.3.md)
