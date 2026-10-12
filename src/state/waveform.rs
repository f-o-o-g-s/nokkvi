//! Waveform seek bar bookkeeping: which song's envelope is on screen, the one
//! ready for the song after it, and the single lookup or scan in flight. The
//! envelopes themselves come from the data crate's `WaveformService`; the
//! 100 ms tick drives this state (`update/waveform.rs`).

use std::{
    collections::HashSet,
    sync::Arc,
    time::{Duration, Instant},
};

use nokkvi_data::{services::task_manager::CancellationToken, types::waveform::Waveform};

/// Seconds into the playing song before any scan starts: past the stream's
/// own start-up buffering, and past songs skipped straight through.
pub(crate) const WAVEFORM_SCAN_AFTER_SECS: u32 = 3;

/// How long the answer to "which song plays next" holds before it is asked
/// again: a Play Next, a reorder or a shuffle change moves it mid-song.
pub(crate) const WAVEFORM_NEXT_REFRESH: Duration = Duration::from_secs(15);

/// The one piece of work the tick may start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WaveformJob {
    /// Read the song's envelope from the disk cache.
    Lookup(String),
    /// Work the song's envelope out from its stream.
    Scan(String),
    /// Ask which song follows the playing one in play order.
    FindNext,
}

/// The job in flight: its number (results carry it, so a late result from a
/// dropped job never clears a newer one), the song it is for (`None` for
/// [`WaveformJob::FindNext`]) and, for a scan, the token that stops it.
#[derive(Debug)]
pub struct WaveformInFlight {
    pub job: u64,
    pub song_id: Option<String>,
    pub cancel: Option<CancellationToken>,
}

/// See the module docs.
#[derive(Debug, Default)]
pub struct WaveformState {
    /// The playing song the state follows; `None` while the Waveform style is
    /// off, on radio, or with nothing playing.
    pub current: Option<String>,
    /// The envelope on screen, for `current`.
    pub shown: Option<Arc<Waveform>>,
    /// The song after `current` in play order, and the `current` it was
    /// worked out for.
    pub next: Option<String>,
    pub next_for: Option<String>,
    /// When `next` was worked out ([`WAVEFORM_NEXT_REFRESH`]).
    pub next_found_at: Option<Instant>,
    /// An envelope fetched ahead for `next`, moved on screen when it plays.
    pub ready: Option<(String, Arc<Waveform>)>,
    pub in_flight: Option<WaveformInFlight>,
    /// Songs the cache had no envelope for this session.
    pub not_cached: HashSet<String>,
    /// Songs whose scan ended unusable (cut short, an error) during this play
    /// of `current`; forgotten on the next song, so the next play retries.
    pub gave_up: HashSet<String>,
    /// The session stopped scanning (a scan ran too slowly for the link).
    pub scans_off: bool,
    /// The last job number handed out.
    pub jobs: u64,
}

impl WaveformState {
    /// Follow the playing song (`None` = nothing to show). On a change the
    /// envelope ready for the new song moves on screen, a job for any other
    /// song is dropped (a scan is cancelled), and this play's give-ups are
    /// forgotten.
    pub(crate) fn follow(&mut self, playing: Option<&str>) {
        if self.current.as_deref() == playing {
            return;
        }
        self.current = playing.map(str::to_owned);
        self.shown = self
            .ready
            .take()
            .filter(|(id, _)| Some(id.as_str()) == playing)
            .map(|(_, waveform)| waveform);
        self.next = None;
        self.next_for = None;
        self.next_found_at = None;
        self.gave_up.clear();
        if self
            .in_flight
            .as_ref()
            .is_some_and(|job| job.song_id.is_none() || job.song_id.as_deref() != playing)
        {
            self.drop_in_flight();
        }
    }

    fn drop_in_flight(&mut self) {
        if let Some(cancel) = self.in_flight.take().and_then(|job| job.cancel) {
            cancel.cancel();
        }
    }

    /// The next job, or `None` while one is in flight or nothing is left to
    /// do. The playing song comes first: cache, then (after
    /// [`WAVEFORM_SCAN_AFTER_SECS`]) a scan. Once it is settled — shown, given
    /// up, or uncached with scanning off — the song after it gets the same.
    pub(crate) fn plan(&self, position_secs: u32, now: Instant) -> Option<WaveformJob> {
        let current = self.current.as_deref()?;
        if self.in_flight.is_some() {
            return None;
        }
        let may_scan = !self.scans_off && position_secs >= WAVEFORM_SCAN_AFTER_SECS;
        // A song given up on this play (scan cut short or failed, or it left
        // the queue) counts as settled: neither looked up nor scanned again.
        if self.shown.is_none() && !self.gave_up.contains(current) {
            if !self.not_cached.contains(current) {
                return Some(WaveformJob::Lookup(current.to_owned()));
            }
            if may_scan {
                return Some(WaveformJob::Scan(current.to_owned()));
            }
            if !self.scans_off {
                // Still waiting to scan the playing song.
                return None;
            }
        }
        let next_stale = self
            .next_found_at
            .is_none_or(|at| now.saturating_duration_since(at) >= WAVEFORM_NEXT_REFRESH);
        if self.next_for.as_deref() != Some(current) || next_stale {
            return Some(WaveformJob::FindNext);
        }
        let next = self.next.as_deref()?;
        if next == current
            || self.gave_up.contains(next)
            || self.ready.as_ref().is_some_and(|(id, _)| id == next)
        {
            return None;
        }
        if !self.not_cached.contains(next) {
            return Some(WaveformJob::Lookup(next.to_owned()));
        }
        may_scan.then(|| WaveformJob::Scan(next.to_owned()))
    }

    /// Record that `job` started; returns the number its result must carry.
    pub(crate) fn started(&mut self, job: &WaveformJob, cancel: Option<CancellationToken>) -> u64 {
        let song_id = match job {
            WaveformJob::Lookup(id) | WaveformJob::Scan(id) => Some(id.clone()),
            WaveformJob::FindNext => None,
        };
        self.jobs += 1;
        self.in_flight = Some(WaveformInFlight {
            job: self.jobs,
            song_id,
            cancel,
        });
        self.jobs
    }

    /// Clear the job in flight when it is job number `job`. A result from a
    /// job that was dropped (a track change cancelled it) leaves a newer job
    /// alone, even one for the same song.
    pub(crate) fn finished(&mut self, job: u64) {
        if self.in_flight.as_ref().is_some_and(|f| f.job == job) {
            self.in_flight = None;
        }
    }

    /// Record which song plays after `for_current`, when that is still the
    /// playing song.
    pub(crate) fn next_found(&mut self, for_current: String, next: Option<String>, now: Instant) {
        if self.current.as_deref() == Some(for_current.as_str()) {
            self.next_for = Some(for_current);
            self.next = next;
            self.next_found_at = Some(now);
        }
    }

    /// `song_id` left the queue: skip it for this play, and ask again which
    /// song plays next if it was that one.
    pub(crate) fn song_gone(&mut self, song_id: String) {
        if self.next.as_deref() == Some(song_id.as_str()) {
            self.next_for = None;
        }
        self.gave_up.insert(song_id);
    }

    /// File an envelope that arrived: on screen for the playing song, ready
    /// for the next one, dropped for any other (the disk cache keeps it).
    pub(crate) fn place(&mut self, song_id: &str, waveform: Arc<Waveform>) {
        if self.current.as_deref() == Some(song_id) {
            self.shown = Some(waveform);
        } else if self.next.as_deref() == Some(song_id) {
            self.ready = Some((song_id.to_owned(), waveform));
        }
    }

    /// Back to nothing, stopping a scan in flight (logout, session expiry).
    pub(crate) fn reset(&mut self) {
        self.drop_in_flight();
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wf() -> Arc<Waveform> {
        Arc::new(Waveform::new(vec![1], vec![1]).expect("waveform"))
    }

    fn playing(id: &str) -> WaveformState {
        let mut s = WaveformState::default();
        s.follow(Some(id));
        s
    }

    /// `s` with "which song is next" answered just now.
    fn with_next(mut s: WaveformState, next: &str, now: Instant) -> WaveformState {
        let current = s.current.clone().expect("playing");
        s.next_found(current, Some(next.to_owned()), now);
        s
    }

    #[test]
    fn the_playing_song_is_looked_up_then_scanned_after_the_delay() {
        let now = Instant::now();
        let mut s = playing("a");
        assert_eq!(s.plan(0, now), Some(WaveformJob::Lookup("a".into())));
        s.not_cached.insert("a".into());
        assert_eq!(
            s.plan(WAVEFORM_SCAN_AFTER_SECS - 1, now),
            None,
            "too early to scan"
        );
        assert_eq!(
            s.plan(WAVEFORM_SCAN_AFTER_SECS, now),
            Some(WaveformJob::Scan("a".into()))
        );
    }

    #[test]
    fn one_job_at_a_time() {
        let now = Instant::now();
        let mut s = playing("a");
        let job = s.started(&WaveformJob::Lookup("a".into()), None);
        assert_eq!(s.plan(60, now), None);
        s.finished(job);
        assert!(s.in_flight.is_none());
    }

    #[test]
    fn once_the_playing_song_shows_the_next_one_is_fetched_ahead() {
        let now = Instant::now();
        let mut s = playing("a");
        s.place("a", wf());
        assert_eq!(s.plan(0, now), Some(WaveformJob::FindNext));
        let mut s = with_next(s, "b", now);
        assert_eq!(s.plan(0, now), Some(WaveformJob::Lookup("b".into())));
        s.not_cached.insert("b".into());
        assert_eq!(
            s.plan(0, now),
            None,
            "the next song's scan waits for the delay too"
        );
        assert_eq!(s.plan(10, now), Some(WaveformJob::Scan("b".into())));
        s.place("b", wf());
        assert_eq!(s.plan(10, now), None, "nothing left to do");
        // When it starts playing, the ready envelope is on screen at once.
        s.follow(Some("b"));
        assert!(s.shown.is_some());
        assert!(s.ready.is_none());
    }

    /// A Play Next, a reorder or a shuffle change moves the next song
    /// mid-song, so the answer is asked again after a while.
    #[test]
    fn the_next_song_is_asked_again_after_the_refresh_interval() {
        let now = Instant::now();
        let mut s = playing("a");
        s.place("a", wf());
        let s = with_next(s, "b", now);
        let mut s = s;
        s.place("b", wf());
        assert_eq!(s.plan(10, now), None);
        assert_eq!(
            s.plan(10, now + WAVEFORM_NEXT_REFRESH),
            Some(WaveformJob::FindNext)
        );
    }

    #[test]
    fn a_song_that_cannot_be_scanned_still_lets_the_next_one_go() {
        let now = Instant::now();
        let mut s = playing("a");
        s.not_cached.insert("a".into());
        s.gave_up.insert("a".into());
        assert_eq!(s.plan(10, now), Some(WaveformJob::FindNext));
        // With scanning off, the next song is still read from the cache.
        let mut off = playing("a");
        off.not_cached.insert("a".into());
        off.scans_off = true;
        let mut off = with_next(off, "b", now);
        assert_eq!(off.plan(10, now), Some(WaveformJob::Lookup("b".into())));
        off.not_cached.insert("b".into());
        assert_eq!(off.plan(10, now), None);
    }

    /// A song that left the queue is skipped for this play instead of being
    /// planned again every tick, and the next song is asked again.
    #[test]
    fn a_song_that_left_the_queue_is_not_retried_every_tick() {
        let now = Instant::now();
        let mut s = playing("a");
        s.place("a", wf());
        let mut s = with_next(s, "b", now);
        s.not_cached.insert("b".into());
        assert_eq!(s.plan(10, now), Some(WaveformJob::Scan("b".into())));
        s.song_gone("b".into());
        assert_eq!(s.plan(10, now), Some(WaveformJob::FindNext));
        // Even if it is still reported next, it is not scanned again this play.
        let s = with_next(s, "b", now);
        assert_eq!(s.plan(10, now), None);
        // The playing song leaving the queue settles it too: no lookup, no
        // scan, straight on to the next song.
        let mut gone = playing("x");
        gone.song_gone("x".into());
        assert_eq!(gone.plan(10, now), Some(WaveformJob::FindNext));
    }

    #[test]
    fn a_track_change_cancels_a_scan_for_another_song_and_forgets_give_ups() {
        let mut s = playing("a");
        let cancel = CancellationToken::new();
        s.started(&WaveformJob::Scan("a".into()), Some(cancel.clone()));
        s.gave_up.insert("x".into());
        s.follow(Some("b"));
        assert!(cancel.is_cancelled());
        assert!(s.in_flight.is_none());
        assert!(s.gave_up.is_empty(), "the next play retries");
        assert!(s.shown.is_none());
    }

    #[test]
    fn a_scan_for_the_song_that_starts_playing_keeps_running() {
        let now = Instant::now();
        let mut s = with_next(playing("a"), "b", now);
        let cancel = CancellationToken::new();
        s.started(&WaveformJob::Scan("b".into()), Some(cancel.clone()));
        s.follow(Some("b"));
        assert!(!cancel.is_cancelled());
        assert!(s.in_flight.is_some());
    }

    /// A cancelled scan of A can still be finishing when A plays again and a
    /// new scan of A starts; its late result must not clear the new job.
    #[test]
    fn a_late_result_from_a_dropped_job_leaves_the_new_job_alone() {
        let mut s = playing("a");
        let old = s.started(
            &WaveformJob::Scan("a".into()),
            Some(CancellationToken::new()),
        );
        s.follow(Some("b"));
        s.follow(Some("a"));
        let new = s.started(
            &WaveformJob::Scan("a".into()),
            Some(CancellationToken::new()),
        );
        s.finished(old);
        assert!(s.in_flight.as_ref().is_some_and(|f| f.job == new));
        s.finished(new);
        assert!(s.in_flight.is_none());
        // And an envelope for a song that is neither playing nor next is not kept.
        s.place("z", wf());
        assert!(s.shown.is_none() && s.ready.is_none());
    }

    #[test]
    fn turning_the_style_off_drops_everything_on_screen() {
        let now = Instant::now();
        let mut s = playing("a");
        s.place("a", wf());
        let cancel = CancellationToken::new();
        s.started(&WaveformJob::FindNext, None);
        s.follow(None);
        assert!(s.shown.is_none() && s.in_flight.is_none() && s.current.is_none());
        assert_eq!(s.plan(60, now), None);
        // A reset stops a running scan.
        s.started(&WaveformJob::Scan("a".into()), Some(cancel.clone()));
        s.reset();
        assert!(cancel.is_cancelled());
    }
}
