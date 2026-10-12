# Changelog — v0.23.x archive

Release v0.23.0, covering 2026-10-09. The current changelog (v0.24.0 onward) lives in [CHANGELOG.md](../CHANGELOG.md).

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
