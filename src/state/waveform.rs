//! Waveform seek bar bookkeeping: which song's envelope is on screen, the one
//! ready for the song after it, and the single lookup or scan in flight. The
//! envelopes themselves come from the data crate's `WaveformService`; the
//! 100 ms tick drives this state (`update/waveform.rs`).

use std::{collections::HashSet, sync::Arc};

use nokkvi_data::{services::task_manager::CancellationToken, types::waveform::Waveform};

/// Seconds into the playing song before any scan starts: past the stream's
/// own start-up buffering, and past songs skipped straight through.
pub(crate) const WAVEFORM_SCAN_AFTER_SECS: u32 = 3;

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

/// The job in flight: the song it is for (`None` for [`WaveformJob::FindNext`])
/// and, for a scan, the token that stops it.
#[derive(Debug)]
pub struct WaveformInFlight {
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
    pub(crate) fn plan(&self, position_secs: u32) -> Option<WaveformJob> {
        let current = self.current.as_deref()?;
        if self.in_flight.is_some() {
            return None;
        }
        let may_scan = !self.scans_off && position_secs >= WAVEFORM_SCAN_AFTER_SECS;
        if self.shown.is_none() {
            if !self.not_cached.contains(current) {
                return Some(WaveformJob::Lookup(current.to_owned()));
            }
            let gave_up = self.gave_up.contains(current);
            if may_scan && !gave_up {
                return Some(WaveformJob::Scan(current.to_owned()));
            }
            if !gave_up && !self.scans_off {
                // Still waiting to scan the playing song.
                return None;
            }
        }
        if self.next_for.as_deref() != Some(current) {
            return Some(WaveformJob::FindNext);
        }
        let next = self.next.as_deref()?;
        if next == current || self.ready.as_ref().is_some_and(|(id, _)| id == next) {
            return None;
        }
        if !self.not_cached.contains(next) {
            return Some(WaveformJob::Lookup(next.to_owned()));
        }
        (may_scan && !self.gave_up.contains(next)).then(|| WaveformJob::Scan(next.to_owned()))
    }

    /// Record that `job` started.
    pub(crate) fn started(&mut self, job: &WaveformJob, cancel: Option<CancellationToken>) {
        let song_id = match job {
            WaveformJob::Lookup(id) | WaveformJob::Scan(id) => Some(id.clone()),
            WaveformJob::FindNext => None,
        };
        self.in_flight = Some(WaveformInFlight { song_id, cancel });
    }

    /// Clear the job in flight when it is the one for `song_id` (`None` for
    /// [`WaveformJob::FindNext`]). A result from a job that was dropped
    /// leaves the newer job alone.
    pub(crate) fn finished(&mut self, song_id: Option<&str>) {
        if self
            .in_flight
            .as_ref()
            .is_some_and(|job| job.song_id.as_deref() == song_id)
        {
            self.in_flight = None;
        }
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

    #[test]
    fn the_playing_song_is_looked_up_then_scanned_after_the_delay() {
        let mut s = playing("a");
        assert_eq!(s.plan(0), Some(WaveformJob::Lookup("a".into())));
        s.not_cached.insert("a".into());
        assert_eq!(
            s.plan(WAVEFORM_SCAN_AFTER_SECS - 1),
            None,
            "too early to scan"
        );
        assert_eq!(
            s.plan(WAVEFORM_SCAN_AFTER_SECS),
            Some(WaveformJob::Scan("a".into()))
        );
    }

    #[test]
    fn one_job_at_a_time() {
        let mut s = playing("a");
        s.started(&WaveformJob::Lookup("a".into()), None);
        assert_eq!(s.plan(60), None);
        s.finished(Some("a"));
        assert!(s.in_flight.is_none());
    }

    #[test]
    fn once_the_playing_song_shows_the_next_one_is_fetched_ahead() {
        let mut s = playing("a");
        s.place("a", wf());
        assert_eq!(s.plan(0), Some(WaveformJob::FindNext));
        s.next = Some("b".into());
        s.next_for = Some("a".into());
        assert_eq!(s.plan(0), Some(WaveformJob::Lookup("b".into())));
        s.not_cached.insert("b".into());
        assert_eq!(
            s.plan(0),
            None,
            "the next song's scan waits for the delay too"
        );
        assert_eq!(s.plan(10), Some(WaveformJob::Scan("b".into())));
        s.place("b", wf());
        assert_eq!(s.plan(10), None, "nothing left to do");
        // When it starts playing, the ready envelope is on screen at once.
        s.follow(Some("b"));
        assert!(s.shown.is_some());
        assert!(s.ready.is_none());
    }

    #[test]
    fn a_song_that_cannot_be_scanned_still_lets_the_next_one_go() {
        let mut s = playing("a");
        s.not_cached.insert("a".into());
        s.gave_up.insert("a".into());
        assert_eq!(s.plan(10), Some(WaveformJob::FindNext));
        // With scanning off, the next song is still read from the cache.
        let mut off = playing("a");
        off.not_cached.insert("a".into());
        off.scans_off = true;
        off.next = Some("b".into());
        off.next_for = Some("a".into());
        assert_eq!(off.plan(10), Some(WaveformJob::Lookup("b".into())));
        off.not_cached.insert("b".into());
        assert_eq!(off.plan(10), None);
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
        let mut s = playing("a");
        s.next = Some("b".into());
        s.next_for = Some("a".into());
        let cancel = CancellationToken::new();
        s.started(&WaveformJob::Scan("b".into()), Some(cancel.clone()));
        s.follow(Some("b"));
        assert!(!cancel.is_cancelled());
        assert!(s.in_flight.is_some());
    }

    #[test]
    fn a_late_result_from_a_dropped_job_leaves_the_new_job_alone() {
        let mut s = playing("a");
        s.started(&WaveformJob::Lookup("b".into()), None);
        s.finished(Some("a"));
        assert!(s.in_flight.is_some());
        // And an envelope for a song that is neither playing nor next is not kept.
        s.place("z", wf());
        assert!(s.shown.is_none() && s.ready.is_none());
    }

    #[test]
    fn turning_the_style_off_drops_everything_on_screen() {
        let mut s = playing("a");
        s.place("a", wf());
        let cancel = CancellationToken::new();
        s.started(&WaveformJob::FindNext, None);
        s.follow(None);
        assert!(s.shown.is_none() && s.in_flight.is_none() && s.current.is_none());
        assert_eq!(s.plan(60), None);
        // A reset stops a running scan.
        s.started(&WaveformJob::Scan("a".into()), Some(cancel.clone()));
        s.reset();
        assert!(cancel.is_cancelled());
    }
}
