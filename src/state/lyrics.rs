//! Lyrics UI state: the resolved document, the active-line cursor, the store
//! index handle, and the stale-load guard. Scroll-animation fields and the
//! word-level dissolve land later (C2 / E1) with their readers.

use std::sync::Arc;

use nokkvi_data::types::lyrics::{LrcDocument, LrcLine, LyricsIndex};

/// The previous track's lyric sheet, dissolving out across a transition while
/// the incoming sheet fades in — the UI-timed companion to the audio
/// crossfade. Parked by `park_outgoing` at the song-change edge; expired by
/// the playback tick once `duration_ms` has elapsed.
#[derive(Debug)]
pub struct OutgoingLyrics {
    pub doc: LrcDocument,
    /// The column center (slot-index space) frozen at the transition, so the
    /// outgoing sheet fades in place instead of re-centering.
    pub center: f32,
    pub started: std::time::Instant,
    pub duration_ms: u32,
}

impl OutgoingLyrics {
    /// Dissolve progress `0.0..=1.0` (1.0 = fully faded out).
    pub fn progress(&self, now: std::time::Instant) -> f32 {
        if self.duration_ms == 0 {
            return 1.0;
        }
        (now.saturating_duration_since(self.started).as_secs_f32() * 1000.0
            / self.duration_ms as f32)
            .min(1.0)
    }
}

/// Per-session lyrics state, embedded on `Nokkvi`.
#[derive(Debug, Default)]
pub struct LyricsState {
    /// Live mirror of `general.lyrics_enabled` (flipped synchronously by the
    /// toggle in Stage D; seeded from the persisted setting at login).
    pub enabled: bool,
    /// The store index, built once at boot. `None` until the build lands (or if
    /// the store dir is unavailable) — the store channel is simply skipped then.
    pub index: Option<Arc<LyricsIndex>>,
    /// The resolved document for the current track. Empty (and `synced == false`)
    /// when nothing matched — the render treats that as the empty state.
    pub doc: LrcDocument,
    /// Prefetched doc for the next track (populated by a later gapless-prefetch
    /// hook; consumed by `promote_next`). `None` for now.
    pub pending_next: Option<(String, LrcDocument)>,
    /// Identity of the track `doc` belongs to — half of the stale-load guard.
    pub matched_song_id: Option<String>,
    /// Index of the active line, or `None` before the first timestamp (pre-roll).
    pub active_index: Option<usize>,
    /// Bumped on every clear / promote / (future) seek. An async resolve result
    /// is applied only if the epoch still matches — the other half of the guard.
    pub load_epoch: u64,
    /// Last authoritative playback position (ms) the active line was computed at.
    pub position_ms: u32,
    /// Glide origin in slot-index space (the eased center at retarget time).
    pub scroll_from: f32,
    /// Glide target: the active line's index as a float.
    pub scroll_to: f32,
    /// When the current glide began; `None` = settled (publish `scroll_to`).
    pub anim_start: Option<std::time::Instant>,
    /// Per-retarget glide duration; `0` = snap (seek-sized jump / first line).
    pub anim_duration_ms: u32,
    /// The previous track's sheet dissolving across a crossfaded transition.
    /// `None` when no dissolve is in flight (incl. crossfade disabled).
    pub outgoing: Option<OutgoingLyrics>,
}

impl LyricsState {
    /// Drop the current document (and any stale prefetch), reset the cursor, and
    /// bump the epoch so an in-flight resolve for the old track is rejected.
    /// Leaves `enabled` untouched.
    pub fn clear(&mut self) {
        self.doc = LrcDocument::default();
        self.pending_next = None;
        self.matched_song_id = None;
        self.active_index = None;
        self.load_epoch = self.load_epoch.wrapping_add(1);
    }

    /// Park the current sheet as the dissolving outgoing layer (crossfade-
    /// coupled transitions). Call BEFORE `promote_next`/`clear` at the
    /// song-change edge; a no-op when there is nothing worth fading.
    pub fn park_outgoing(&mut self, center: f32, duration_ms: u32) {
        if self.doc.synced && !self.doc.lines.is_empty() && duration_ms > 0 {
            self.outgoing = Some(OutgoingLyrics {
                doc: std::mem::take(&mut self.doc),
                center,
                started: std::time::Instant::now(),
                duration_ms,
            });
        }
    }

    /// Drop a finished dissolve (driven from the playback tick).
    pub fn expire_outgoing(&mut self, now: std::time::Instant) {
        if self
            .outgoing
            .as_ref()
            .is_some_and(|o| o.progress(now) >= 1.0)
        {
            self.outgoing = None;
        }
    }

    /// Begin (or snap) a glide of the column center toward `new_active`.
    /// `current_pos` is the live eased center (read from the published atomic)
    /// so a retarget mid-glide continues from where the column visually is.
    /// `duration_ms == 0` snaps: the very next frame lands on the target.
    pub fn retarget_scroll(&mut self, new_active: usize, current_pos: f32, duration_ms: u32) {
        self.scroll_to = new_active as f32;
        if duration_ms == 0 {
            self.scroll_from = self.scroll_to;
            self.anim_start = None;
            self.anim_duration_ms = 0;
        } else {
            self.scroll_from = current_pos;
            self.anim_start = Some(std::time::Instant::now());
            self.anim_duration_ms = duration_ms;
        }
    }

    /// If a prefetched doc for `new_id` is ready, promote it into `doc`
    /// synchronously (no async round-trip) and return `true`; else `false` (the
    /// caller then falls back to clear + async resolve).
    pub fn promote_next(&mut self, new_id: &str) -> bool {
        match self.pending_next.take() {
            Some((id, doc)) if id == new_id => {
                self.doc = doc;
                self.matched_song_id = Some(id);
                self.active_index = None;
                self.load_epoch = self.load_epoch.wrapping_add(1);
                true
            }
            other => {
                // Not a match (or nothing prefetched): put it back untouched
                // only if it was for a different id we might still promote later.
                self.pending_next = other.filter(|(id, _)| id != new_id);
                false
            }
        }
    }
}

/// Index of the last line whose timestamp is `<= position_ms`, or `None` before
/// the first timestamp (pre-roll — no line is active yet). O(log n); relies on
/// `parse()` having sorted the lines by time.
pub(crate) fn active_line_at(lines: &[LrcLine], position_ms: u32) -> Option<usize> {
    let reached = lines.partition_point(|l| l.time_ms <= position_ms);
    (reached > 0).then(|| reached - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(time_ms: u32) -> LrcLine {
        LrcLine {
            time_ms,
            text: String::new(),
            words: vec![],
        }
    }

    #[test]
    fn active_line_pre_roll_is_none() {
        let lines = [line(5_000), line(10_000)];
        assert_eq!(active_line_at(&lines, 0), None);
        assert_eq!(active_line_at(&lines, 4_999), None);
        assert_eq!(active_line_at(&lines, 5_000), Some(0));
        assert_eq!(active_line_at(&lines, 9_999), Some(0));
        assert_eq!(active_line_at(&lines, 10_000), Some(1));
    }

    #[test]
    fn clear_bumps_epoch_and_resets() {
        let mut state = LyricsState {
            active_index: Some(3),
            ..Default::default()
        };
        state.matched_song_id = Some("s1".into());
        let before = state.load_epoch;
        state.clear();
        assert_eq!(state.active_index, None);
        assert_eq!(state.matched_song_id, None);
        assert_eq!(state.load_epoch, before.wrapping_add(1));
    }

    #[test]
    fn promote_next_swaps_matching_pending() {
        let mut state = LyricsState::default();
        let doc = LrcDocument {
            lines: vec![line(0)],
            synced: true,
        };
        state.pending_next = Some(("s2".into(), doc));
        assert!(state.promote_next("s2"));
        assert_eq!(state.matched_song_id.as_deref(), Some("s2"));
        assert_eq!(state.doc.lines.len(), 1);
        assert!(state.pending_next.is_none());
    }

    #[test]
    fn promote_next_false_on_mismatch() {
        let mut state = LyricsState {
            pending_next: Some(("other".into(), LrcDocument::default())),
            ..Default::default()
        };
        assert!(!state.promote_next("s2"));
        assert!(state.doc.lines.is_empty());
    }
}
