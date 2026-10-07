# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- The new Reflection setting for Bars and Lines stands the visualizer on a waterline above rippling dark water that mirrors it.
- With Reflection on, beats send ripples running sideways across the water.
- With Reflection on, the Lines surfing boat rides the line above the water.
- The new Horizon setting for Bars sends the last second and a half of bars receding behind the live ones toward a misty horizon.
- For Lines, the Horizon setting sends the last second and a half of the line receding behind it as misty waves.
- Horizon rows nearest the viewer run off both edges of the visualizer, while farther rows narrow toward the middle.
- In LED mode, the Horizon's receding bars are cut into LED segments too.

### Changed

- Living Ink now drops rings and amorphous blobs on the music's hits, and its ink swells and merges while the music is busy.

### Fixed

### Removed

## v0.22.1 — 2026-10-04

### Fixed

- Covers in muted greens, blues, plums and reds, like a dark green cloth binding, now color the accent instead of turning grey.
- Faintly tinted covers, such as sepia photos or cream paper, now give a matching faint accent instead of grey.
- A black cover whose only color is a thin rainbow now takes the rainbow's colors.
- A large muted area, like a dark olive background, now joins the visualizer gradient beside the cover's vivid accent color.
- On dark themes, bars over a bright cover now use deep cover colors where those stay visible, instead of near-white pastels.

## v0.22.0 — 2026-10-04

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
- On dark themes, Harbour's Trawl scene sky now holds a streaked, slowly folding aurora curtain.
- On dark themes, the Trawl scene's aurora drifts in front of the moon and stars.
- On dark themes, the Trawl scene's stars and moon now glow softly.
- On dark themes, streaked rays of light now hang down into the Trawl scene's water.
- On dark themes, glowing plankton now drifts through the Trawl scene's water.
- On dark themes, a moving net of light now plays across the Trawl scene's seabed.
- On dark themes, the Trawl scene's aurora rays now rise and fall with the music.
- On dark themes, each kick now sends a surge of light sweeping across the Trawl scene's aurora.
- On dark themes, the Trawl scene's stars now flare on the beat.
- The Trawl scene's aurora and stars react to the music even with the visualizer off.
- On dark themes, the Trawl scene's longship is now moonlit, keeping its own colors.
- On dark themes, a lantern now hangs at the masthead of the Trawl scene's longship.
- On dark themes, the edges of the Trawl scene's fish now catch the aurora's light.
- On dark themes, the Trawl scene's kelp now carries softly blinking, glowing beads.
- On dark themes, the Trawl scene's anchor now looks like moonlit metal.
- On dark themes, the Trawl scene's anchor rope now shows a faint lit sheen.
- On dark themes, the Trawl scene's sky notes now glow softly.
- On light themes, the Trawl scene's sky now warms to a soft haze at the horizon.
- On light themes, high cloud now drifts across the Trawl scene's sky.
- On light themes, the Trawl scene's sun now glows in a soft bloom.
- On light themes, sun shafts now slant down into the Trawl scene's water.
- On light themes, ripples of sunlight now play across the Trawl scene's sand.
- The Trawl scene's sea now sways in gentle, low folds instead of rolling hills.
- The Trawl scene's rocks, starfish and kelp now cast soft shadows on the sand.
- The Trawl scene's rocks, kelp and sunken shield now sink into drifted sand at their base.
- The Trawl scene's sunken crate is now a half-buried viking shield painted like the longship's shields.
- The Trawl scene's bubbles now look like glass.
- The Trawl scene's fish now waggle their tails as they swim.
- The Trawl scene's gulls now flap their wings in bursts between glides.

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
- Playing something else while a long Songs list is still loading into the queue no longer appends the rest of it.
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

## Older releases

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
