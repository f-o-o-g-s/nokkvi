---
paths:
  - "src/state/{lyrics,artwork}.rs"
  - "src/update/{lyrics,playback,theater,frame,player_bar,dynamic_accent}.rs"
  - "src/update/tests/lyrics.rs"
  - "src/widgets/lyrics_viewport.rs"
  - "src/app_view.rs"
  - "src/views/queue/view.rs"
  - "data/src/types/lyrics.rs"
  - "data/src/services/lyrics_source.rs"
  - "data/src/services/api/lyrics.rs"
---

# Gotchas: Lyrics

- **A plain lyric sheet's stamps are all `time_ms == 0`**: drive it through `drift_center` (playback progress × `n - 1`, plus the wheel's `drift_offset`, clamped), and never search its stamps by time: every zero stamp passes a `partition_point`, which would name the LAST line from the first tick and highlight it for the whole track. `LrcDocument::active_line_at` returns `None` for a plain doc, and the apply path and the tick (the only writers of `active_index`) both go through it, so `active_index` stays `None` for a plain sheet's whole life. Write the wheel offset already clamped, via `drift_offset_for` (the exact inverse of `drift_center`), so nothing banks hidden overshoot and a notch lands where it asked even in the final second, where the tick's whole-second duration puts progress above 1.0.
- **Lyrics overlay identity**: the lyrics + blurred-cover layer must gate on the **transport** (`playing || paused`), never on `current_song_id` / `is_queue()` — both survive `stop()` and are seeded from the restored queue at login, so gating on them scrims a bare cover with no audio. The backdrop must key on the **shown/resolved cover handle**, not the playing track: during the cold-art window the panel falls back to the centered row's cover, so rendering the playing track's lyrics over a *different* album is wrong.
- **The lyric clock is not the position**: the word-level highlight reads `LyricsState::clock_ms`, which the frame tick runs on from the last position tick (`lyric_clock_ms`, `src/state/lyrics.rs`) and which only ever moves forward within a track; a tick more than `LYRIC_CLOCK_SEEK_BACK_MS` (300 ms) behind it is a seek and snaps it back. It is deliberately NOT gated on the seek epoch (a stale pre-seek tick costs one tick, then the next one snaps), the position handler re-runs it right after anchoring so the line it names never draws against the old reading, and `clear()` / `promote_next()` zero it, because the frame tick would otherwise carry the previous track's reading over a new sheet until a tick fell far enough behind. Toggling lyrics off and on from Settings or `config.toml` never calls `clear()` (pre-existing), so a sheet that stayed loaded through an off-period keeps its clock.
- **Navidrome invents cue ends**: its normaliser gives a cue with no `end` one whenever the line has a fallback (the next cue's start; for a line's last cue the line's own `end`, a TTML `<p end>`, else the NEXT line's start), and clears every end of the sheet's last line when it has none; a bare space between two word stamps arrives as a cue of its own. So `LrcDocument::from_structured` strips a last word's end that EQUALS the next line's start (that fill would hold the word lit through the whole gap; a source end of exactly that value is indistinguishable and takes the same pace fallback) and folds a textless cue into the previous word's end. `LrcLine::word_window` then honours every remaining end, cut at the next line's start, so a held note that runs into the next line sweeps until it begins. The local parser folds blank `<mm:ss.xx>` markers the same way, so a store file and the same sidecar served by Navidrome time alike. Navidrome reads sidecars at REQUEST time, so a `.elrc` beside the audio needs no rescan and never shows in the database.
