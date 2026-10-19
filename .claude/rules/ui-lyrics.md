---
paths:
  - "src/update/{lyrics,frame,theater}.rs"
  - "src/state/lyrics.rs"
  - "src/views/queue/view.rs"
  - "src/widgets/lyrics_viewport.rs"
  - "src/update/tests/lyrics.rs"
  - "data/src/types/lyrics.rs"
---

# Lyrics (UI)

## Lyrics Overlay

A lyrics layer overlays the now-playing Queue cover (handler `update/lyrics.rs`, state `src/state/lyrics.rs`; resolve pipeline via `Message::LyricsLoader`, `LyricsIndexReady` / `LyricsBlurReady` / `LyricsCoverHeld`). The Lyrics setting is a `LyricsMode`: Off, On (the layer for every song), or Auto (the layer only once the playing song's lookup found lyrics; Toggle Lyrics overrides that for the playing song only and never writes the setting). Which gate reads which: see gotchas/lyrics.md "Two lyrics gates". It is an **overlay, not a modal** — it renders only when the cover panel actually shows the now-playing track (the `cover_shows_now_playing` gate in `views/queue/view.rs`), behind a frosted-cover backdrop. The panel locks to the playing track's cover while playback runs, and while paused too whenever the layer is up (`QueueViewData::cover_locks_to_playing`); otherwise it follows the centered row, so a pause that unlocked it would drop the sheet. Identity gates (transport-gate the layer; key it to the shown cover, not the playing track): see gotchas/lyrics.md "Lyrics overlay identity".

**Two kinds of sheet**, told apart by `LrcDocument.synced` and carried to the view as `LyricsPanelData.synced`:
- **Synced** — the active line is accented and the column glides between lines (`eased_center`), snapping on seek-sized jumps (`LYRICS_SNAP_INDEX_DELTA`).
- **Plain** — untimed lyrics from the server (a sidecar file or the lyrics embedded in the track's tags). **No line is ever accented**, and its `active_index` stays `None` for its whole life: every stamp is 0, so `LrcDocument::active_line_at` returns `None` for it rather than name the LAST line (see gotchas/lyrics.md). A flat `PLAIN_BAND_LINES` band carries the eye instead, then the same falloff curve. Its center is `drift_center(position, duration, line_count, drift_offset)` — a pure function, so a seek and a wheel notch both jump.

The per-frame tick (`update/frame.rs`) is the SINGLE publisher of the column center for both kinds; `clear()` / `promote_next()` park the glide so a new sheet's pre-roll can't keep republishing the previous track's line into the next dissolve.

**Word timing** (a TTML / enhanced-LRC sidecar, the server's `cueLine` layer, an enhanced `.lrc` in the store) fills `LrcLine.words`; with the Playback setting Lyrics: Word Highlight (`general.lyrics_word_highlight`: Off / Sweep / Word, default Sweep) not Off, the ACTIVE line of such a sheet keeps the base ink and the sung words are painted over it in accent (`Sweep` floods each word left to right with a soft front; `Word` lights each word whole over 150 ms). The pure input is `LrcLine::word_cursor(line_end, clock_ms)` (data crate): a word ends at its explicit end, cut at the next word's start, else at the next word's start; a LAST word ends at its explicit end, else at the line's own word pace (clamped 250..1500 ms; 800 ms for a one-word line), and never past the next line's stamp. The end Navidrome invents for a last cue is stripped on the way in (gotchas/lyrics.md "Navidrome invents cue ends"). The clock is `LyricsState::clock_ms`, the **lyric clock**: the position tick anchors it (`position_ms` + `Instant`) and the frame tick (`step_lyrics_clock`, before the center publish) runs it on while the transport runs, holds it while paused, never steps it back for a tick that merely lags, and snaps it only for a tick more than 300 ms behind (a seek); `clear()` / `promote_next()` zero it. Lines without word timing, plain sheets, the dissolving sheet and Off render exactly as before. Geometry lives in the viewport: see widgets.md.

**The wheel** is the one event the viewport captures, and only over a non-empty plain sheet with a non-zero converted delta; it publishes a LINE delta (`QueueMessage::LyricsWheel`) that returns from `handle_queue`'s pointer-motion fast-path block. Everything else stays event-transparent — see widgets.md.
