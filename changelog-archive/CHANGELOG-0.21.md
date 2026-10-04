# Changelog — v0.21.x archive

Releases v0.21.0–v0.21.2, covering 2026-09-25 → 2026-10-02. The current changelog (v0.22.0 onward) lives in [CHANGELOG.md](../CHANGELOG.md).

## v0.21.2 — 2026-10-02

### Added

- The Fractal Voyage MilkDrop preset journeys through four fractals in turn: a Julia set, the Burning Ship, the Mandelbrot set and the dragon curve.
- The Fjord MilkDrop preset flies low down a winding Norwegian fjord between cliffs that rise straight out of the water.
- Press m, or choose Choose Preset… from the panel menu, to browse every MilkDrop preset in a searchable list.
- The MilkDrop preset list plays each preset live as you scroll, and Enter locks the one on screen.
- Closing the MilkDrop preset list without choosing returns to the preset you started from.
- In the MilkDrop preset list you can favorite or hide any preset, bring hidden ones back, or show only favorites.
- The Starfield Nebula MilkDrop preset flies through liquid stars and a living nebula coloured by your theme.
- The Pirate Signal MilkDrop preset shows nokkvi's pirate smiley as a broadcast on an LCD, over living sand and oil paint.

### Changed

- Julia Lace now morphs between different Julia sets and lies in a dark, glossy liquid.
- Living Ink is rebuilt as a 3D tank: drops of ink shoot into dark water from every direction and linger as drifting, curling clouds.

### Removed

- The Chladni, Cover Kaleido and Cover Orb MilkDrop presets are gone.

## v0.21.1 — 2026-09-26

### Added

- The Chladni MilkDrop preset pours theme-coloured sand onto a vibrating plate, redrawing its figure as the music retunes it.
- The Julia Lace MilkDrop preset dives endlessly into a Julia set's spiral vortex, the camera swinging between steep dives and skimming glides.
- The Coral City MilkDrop preset floats through an endless fractal coral reef that sways with the music and leaves light trails.
- The Coral Dive MilkDrop preset dives endlessly into the coral reef, which repeats itself exactly every 26-fold zoom.
- The Infinity MilkDrop preset flies down a spiralling corridor of neon-lit polygon frames standing in a dark, rippling liquid that reflects them.
- Infinity flies faster as the music gets louder and lunges forward on kicks.
- Each beat sends a wave of light down Infinity's neon inlays.
- Switching MilkDrop presets now dissolves the old preset into the new one, in a pattern that changes each time.
- A Preset Crossfade setting sets how long MilkDrop's dissolve lasts, up to 10 seconds.
- A Preset Crossfade of 0 cuts straight to the next preset.

## v0.21.0 — 2026-09-25

### Added

- Theater Mode (F11) fills the window with the playing track's cover, visualizer and lyrics, hiding the library, toolbar and nav.
- Escape leaves Theater Mode; keys aimed at the hidden list leave it too, and view keys leave and then switch.
- Right-clicking the Theater Mode cover offers Exit Theater Mode and Refresh Artwork.
- In Theater Mode the player bar slides away after 2.5 idle seconds and slides back on any mouse or key activity.
- In Theater Mode the mouse cursor hides after 2.5 idle seconds, like in a video player.
- In Theater Mode the lyrics grow with the window, up to two and a half times their Queue size.
- A Cover Art setting can swap the now-playing cover for a plain backdrop, everywhere or only in Theater Mode.
- With the cover hidden in Theater Mode, the visualizer spans the whole window instead of a centered square.
- Hovering the Queue cover reveals an expand icon that enters Theater Mode; its right-click menu gains Enter Theater Mode.
- In Theater Mode an exit icon rides above the player bar.
- A Theater Controls setting keeps Theater Mode's player bar auto-hiding, always shown, or always hidden.
- A Theater Fills the Screen setting also makes the window fullscreen in Theater Mode and restores it on leaving.
- `nokkvi theater` toggles Theater Mode from the command line.
- `nokkvi status` now reports whether Theater Mode is on.
- `nokkvi preset next` (or previous, lock, unlock, favorite, unfavorite, hide) controls MilkDrop presets from the command line.
- `nokkvi status` now reports the visualizer mode and the MilkDrop preset on screen.
- A MilkDrop visualizer mode plays MilkDrop presets in place of the Queue or Radios cover and fills Theater Mode.
- The visualizer button and `v` now cycle Off, Bars, Lines, Scope and MilkDrop.
- MilkDrop switches to another preset every 30 seconds and on each new track, showing the preset's name.
- In MilkDrop mode, `n` jumps to another preset and `p` returns to the previous one.
- In MilkDrop mode, Shift+M locks the current preset until pressed again.
- Right-clicking the MilkDrop panel offers Next, Previous, Lock, Favorite and Never Show This Preset.
- Never Show This Preset hides a preset for good and moves on at once.
- Presets dropped into `~/.config/nokkvi/milkdrop/` join the rotation; the refresh key picks up new ones.
- A MilkDrop settings section sets the preset interval, whether tracks change presets, favorites only, render quality and name toasts.
- Fourteen nokkvi MilkDrop presets draw in your theme's colours; eight of them use the playing album's cover.
- Two nokkvi presets remake the classic "dedicated to the sherwin maxawow" in your theme's colours or your cover's.
- Two nokkvi presets remake Flexi's "black holes", whose bouncing holes swallow the picture, in your theme's colours or your cover's.
- nokkvi's MilkDrop presets recolour on the spot when you change theme, and keep the theme's dark colours in light mode.
- The MilkDrop Presets setting gains `nokkvi`, which plays only nokkvi's own presets.
- Each MilkDrop preset starts from the previous preset's picture, as in MilkDrop, instead of from black.

### Changed

- Svalbard's visualizer peaks now step from dark to light teal instead of alternating two colors.
- The Harbour moon and stars now glow in the lightest peak color, whatever order a theme lists its peaks in.
