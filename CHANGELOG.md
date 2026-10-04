# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- The new Accent From Album Art setting in the Theme tab takes the accent color from the playing cover, adjusted to stay readable.
- With Accent From Album Art on, rating stars and love hearts take the accent color too.
- The new Visualizer From Album Art setting gives the bars, lines, scope and nokkvi's MilkDrop presets a gradient from the playing cover.
- Black-and-white covers turn the cover-driven accent and visualizer grey instead of keeping the theme's color.

### Changed

- A prompt that opens while another window is up now appears on top of it instead of hiding behind it.
- The `nav-up`, `nav-down` and `enter` commands now do nothing while the EQ, About or Get Info window or a dialog is open, like the keys.
- With Fade on Skip set to Boundary Fade, Next and Previous now cut straight out of a song the server is still transcoding.
- Searching in the default playlist picker now keeps the highlighted playlist highlighted while it still matches, instead of jumping to the top.
- On dark themes, Harbour's Trawl scene now glows with a streaked aurora, rays of light hanging into the water, drifting plankton and a lit seabed.
- On dark themes, the Trawl scene's stars and moon now glow softly, with the aurora drifting in front of them.
- The Trawl scene's sea now sways in gentle, low folds instead of rolling hills.
- On dark themes, the Trawl scene's longship is moonlit, keeping its colors, with a lantern at the masthead.
- On dark themes, the Trawl scene's fish, rope and anchor now catch the aurora's light, and the kelp glows faintly.
- The Trawl scene's rocks, starfish and kelp now sit in the sand with soft shadows, lit by the sea floor's own light.
- The Trawl scene's sunken crate is now a half-buried viking shield painted like the longship's shields.
- On dark themes, the Trawl scene's stars flare on the beat.
- The Trawl scene's fish now waggle their tails as they swim.
- The Trawl scene's gulls now flap their wings in bursts between glides.
- The Trawl scene's bubbles now look like glass, and on dark themes its sky notes glow softly.
- On light themes, the Trawl scene is now sunlit: a soft sky with high cloud, a glowing sun, sun shafts and caustics on the sand.
- On dark themes, the Trawl scene's aurora now follows the music, reaching with the spectrum and surging on each kick, even with the visualizer off.

### Fixed

- Hovering an active tab or mode toggle is now visible on dark themes with a light accent.
- The multi-item drag count badge now stays readable on every theme's accent.
- Check marks in dialogs now stay readable on every theme's accent.
- Selected text in the EQ preset name field no longer hides behind an opaque highlight.
- Logging in no longer wipes the rest of config.toml when the file has a typo or can't be read.
- When config.toml has a typo or can't be read, logging in now warns that the login wasn't saved to it.
- Choosing a theme no longer wipes config.toml when the file can't be read.
- Saving radio scrobbling credentials no longer wipes config.toml when the file has a typo.
- Numeric settings hand-edited in config.toml, such as seek step, crossfade length or ReplayGain pre-amp, now stay within their allowed range.
- Saving config.toml with a typo no longer resets the visualizer to its defaults.
- Dragging an EQ slider now moves the handle all the way to the end of its track.
- Saving config.toml with an invalid visualizer value, such as text where a number belongs, no longer resets the visualizer to its defaults.
- Saving config.toml with a typo now shows a warning naming the line of the error.
- When a section of config.toml holds an invalid value, a warning now names the section that kept its previous values.
- Playing several selected items, Shuffle Play, or Replace Queue With All Found while a radio station plays now hands the player back to the queue.
- Replacing the queue from Similar or Top Songs results now clears the previous playlist's header.
- Show in Folder in an album's Get Info no longer opens a different, expanded album's folder.
- Find Similar from the Songs view now titles the results "Similar to: …" like every other view.
- The Playlists create menu now closes when the window loses focus, like the other header menus.
- `nokkvi --help` now lists `harbour` among the `switch-view` targets.
- When the volume can't be saved, a warning now says so instead of the change silently not surviving a restart.
- Playing something else while a long Songs list is still loading into the queue no longer appends the rest of that list to the new queue.
- Pressing Enter in the split view's library pane now only adds to the queue, leaving a playing radio station and the playlist header alone.
- Cancelling a roulette spin while a radio station plays now keeps the app in radio mode.
- When playing a selection fails while a radio station plays, the app now stays in radio mode instead of switching to the queue.
- Playing a row from the smart-playlist rules preview no longer leaves the previous playlist's "Playing From" header on the queue.
- Enter in a full library view now plays instead of only adding to the queue when the split view was left open on the Queue.
- With the split view left open behind another view, Enter and the list keys now act on the view on screen, not a hidden tab.
- A radio station that drops the connection now reconnects instead of staying silent.
- Auto-advancing into a track with a different sample rate now plays it at its own ReplayGain level instead of the previous track's.
- In ReplayGain Track mode, auto-advancing into a track with a different gain now plays it at its own level instead of the previous track's.
- On the non-PipeWire audio fallback, where bit-perfect Strict has no effect, clicking a track now crossfades with Fade on Skip like Next does.
- ReplayGain normalization now applies to songs played from albums, artists, genres and the Songs view.
- In ReplayGain Album mode, auto-advancing into another album now plays it at that album's level instead of the previous album's.
- In ReplayGain Track mode, auto-advancing now also honors clipping prevention and album-gain fallback for the next track.
- With ReplayGain on, tracks with different gains now follow each other without a gap.
- Closing the EQ with Escape no longer brings back an unfinished preset name prompt the next time it opens.
- Escape now plays its sound when it closes the EQ, About or Get Info window, like the other windows.
- Pressing the EQ key or Shift+I again now closes the EQ or Get Info window it opened.
- Escape now closes an open window before cancelling a roulette spin running behind it.
- Enter now reaches an open window instead of stopping a roulette spin running behind it.
- A list's scrollbar now fades out even when a window opens before it hides.
- A duplicate-songs check that finishes while another window is open now shows a warning toast instead of opening its dialog.
- Top Songs on a track inside an expanded album now opens the artist's top songs instead of doing nothing.
- When the queue can't be saved to disk, a song removed from it no longer still plays next.
- When the queue can't be saved to disk, the queue view and the shuffle, repeat and consume buttons now match what playback does.
- With bit-perfect Strict on and Fade on Skip set to Crossfade, Next and Previous now start the next track sooner.
- Enter or Ctrl+Enter with several rows selected in Artists, Genres or Playlists now plays the selection instead of the row under the cursor.
- In the split view's library pane, playing several selected rows now adds them to the queue instead of replacing it.
- After clicking a song or album, Enter now follows the Enter Behavior setting instead of always replacing the queue with it.
- With an album or artist expanded, Enter on an album or artist row below it now plays that row instead of a different one.
- Below an expanded playlist, the artwork panel now loads the collage of the playlist in focus instead of another playlist's.
- A Find Similar still loading at logout can no longer replace the results of one started after logging back in.
- After logging back in, the playing playlist's header no longer falls back to a single cover when its first albums left the queue.
- Rows selected before logging out are no longer still selected after logging back in, where Enter could play different items.
- When a lyrics lookup fails, for example offline, the song no longer shows no lyrics for the rest of the session.
- During radio, clicking Shuffle, Repeat, Consume or Lyrics in the player bar's kebab menu now does nothing, like their buttons.
- During radio, the kebab's Shuffle, Repeat, Consume and Lyrics entries are now greyed out like their buttons.
- During radio, the kebab's Shuffle, Repeat, Consume and Lyrics checkboxes now stay unticked, as their buttons stay unlit.
- During radio, the kebab's dot no longer lights up because Shuffle, Repeat, Consume or Lyrics is on.
- The bottom row of the font, theme, default playlist and MilkDrop preset pickers is no longer squashed shorter than the rows above it.

### Removed

- Genre rows no longer offer Get Info, which did nothing.
- Artist rows no longer offer Show in File Manager, which did nothing.

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

## Older releases

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
