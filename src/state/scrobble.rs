//! Scrobble state with anti-seek-fraud accounting.

use nokkvi_data::services::radio_scrobble::ScrobbleTargets;

/// Scrobbling state with anti-seek-fraud protection
#[derive(Debug, Clone, Default)]
pub struct ScrobbleState {
    /// Actual seconds listened (not playback position) - prevents seek-fraud
    pub listening_time: f32,
    /// Last known position for calculating listening time deltas
    pub last_position: f32,
    /// Whether the current song's scrobble has been CONFIRMED submitted by the
    /// server (prevents double-scrobble). Latched only on a successful HTTP
    /// response, never on submission intent.
    pub submitted: bool,
    /// Whether a scrobble submission is currently in flight (a GET has been
    /// dispatched but no result has landed yet). Gates re-dispatch so ticks do
    /// not spam duplicate submissions, and is cleared on every result (Ok or
    /// Err) and on song change so a dropped task can never wedge scrobbling.
    pub submission_in_flight: bool,
    /// Timer ID for debounced "now playing" notification
    pub now_playing_timer_id: u64,
    /// Current song ID for scrobble tracking
    pub current_song_id: Option<String>,
    /// Duration (seconds) of the song currently being tracked. Captured at
    /// `reset_for_new_song` so the song-change scrobble fallback evaluates the
    /// FINISHED song against its own duration rather than the volatile shared
    /// `PlaybackState::duration`, which has already been overwritten with the
    /// successor's duration by the time the fallback runs.
    pub current_song_duration: u32,
}

impl ScrobbleState {
    /// Reset for a new song.
    ///
    /// `duration` is the new song's length in seconds; it is stored on
    /// `current_song_duration` so the next song-change fallback can judge this
    /// song's listening time against its own duration.
    pub fn reset_for_new_song(&mut self, song_id: Option<String>, position: f32, duration: u32) {
        self.current_song_id = song_id;
        self.listening_time = 0.0;
        self.last_position = position;
        self.submitted = false;
        self.submission_in_flight = false;
        self.current_song_duration = duration;
    }

    /// Check if scrobble conditions are met for the given track duration.
    ///
    /// Mirrors Navidrome's canonical play-tracker rule: a play counts once the
    /// listener has heard at least `min(duration * threshold_percent, 4 minutes)`.
    /// The absolute 4-minute arm lets long-form content (DJ mixes, podcasts,
    /// audiobooks) reach eligibility where a percentage-only rule never would.
    ///
    /// Returns `false` while a submission is already confirmed (`submitted`) or
    /// in flight (`submission_in_flight`) — those latches gate exactly one
    /// submission per song — and for zero-duration tracks.
    pub fn should_scrobble(&self, track_duration: u32, threshold_percent: f32) -> bool {
        if self.submitted || self.submission_in_flight || track_duration == 0 {
            return false;
        }
        self.listening_time >= ABSOLUTE_SCROBBLE_SECS
            || self.listening_time >= (track_duration as f32 * threshold_percent)
    }
}

/// Absolute listening-time arm for scrobble eligibility, in seconds. Matches
/// Navidrome's `4 * 60 * 1000` ms cap and the Last.fm 4-minute convention.
const ABSOLUTE_SCROBBLE_SECS: f32 = 240.0;

/// Upper clamp on a single tick's accrued listen seconds. Absorbs a resume
/// after a long pause and non-monotonic wall-clock steps (NTP) so neither
/// inflates the window. Mirrors the queue path's forward-only 0–10 s clamp in
/// `track_listening_time`.
const MAX_RADIO_TICK_DELTA: i64 = 10;
/// Per-target failed-submit cap before giving up on the current track. With the
/// cooldown between attempts this spans minutes — long enough to ride out a
/// brief service outage, bounded enough that a permanently-rejected scrobble
/// (e.g. a Last.fm ignore-list artist) stops, and the next track resets it.
const MAX_RADIO_SUBMIT_ATTEMPTS: u32 = 5;
/// Wall-clock seconds a failed target waits before it may retry. Spaces retries
/// so a sub-second blip doesn't burn the attempt cap, and lets a recovered
/// service get the listen on the next pass (re-arm on recovery).
const RADIO_RETRY_COOLDOWN_SECS: i64 = 20;

/// Per-target scrobble-submission progress for the current track.
#[derive(Debug, Clone, Default)]
struct TargetProgress {
    /// Confirmed accepted, gave up after the cap, or unconfigured — never
    /// attempt this target again for the current track.
    done: bool,
    /// Failed attempts so far (caps the retry loop).
    attempts: u32,
    /// Wall-clock secs until which this target is in retry cooldown.
    retry_after: i64,
}

impl TargetProgress {
    /// Whether this target should be attempted now: not done and out of cooldown.
    fn ready(&self, now: i64) -> bool {
        !self.done && now >= self.retry_after
    }

    /// Fold in one dispatch outcome. `None` = not attempted (unconfigured / not
    /// requested) → done so it isn't chased; `Some(true)` = accepted → done;
    /// `Some(false)` = failed → cooldown, or give up at the cap.
    fn apply(&mut self, outcome: Option<bool>, now: i64) {
        match outcome {
            None | Some(true) => self.done = true,
            Some(false) => {
                self.attempts += 1;
                if self.attempts >= MAX_RADIO_SUBMIT_ATTEMPTS {
                    self.done = true;
                } else {
                    self.retry_after = now + RADIO_RETRY_COOLDOWN_SECS;
                }
            }
        }
    }
}

/// Pure timing + dedup state machine for scrobbling internet radio.
///
/// Radio differs from queue playback in two ways that make [`ScrobbleState`]
/// unusable for it: streams report **no duration** (so the
/// `min(duration * pct, 4 min)` rule degenerates), and there is no song id, only
/// the ICY `StreamTitle` artist+title. This state instead **accrues listen
/// seconds only while playing** (clamped wall-clock deltas — so pauses and clock
/// steps don't inflate the window) and keys dedup on the cleaned `(artist,
/// title)`. Each scrobble target (ListenBrainz / Last.fm) is latched
/// **independently** on confirmed success, with a per-target cooldown re-arm so
/// a transient failure doesn't permanently drop a listen and a target that
/// already succeeded is never re-submitted on another target's retry.
///
/// The caller injects `now` (unix seconds), keeping it fully unit-testable.
#[derive(Debug, Clone, Default)]
pub struct RadioScrobbleState {
    current: Option<RadioCurrent>,
    /// Last raw ICY `(artist, title)` seen — lets the tick handler skip
    /// rebuilding the cleaned `ScrobbleTrack` on the ~10 Hz ticks where the
    /// stream title hasn't changed (the vast majority).
    last_raw: Option<(Option<String>, Option<String>)>,
}

#[derive(Debug, Clone)]
struct RadioCurrent {
    artist: String,
    title: String,
    /// Unix seconds the track was first observed — the scrobble timestamp.
    started_at: i64,
    /// Unix seconds of the last tick, for clamped wall-clock delta accrual.
    last_tick: i64,
    /// Accrued listen seconds — advances ONLY while actually playing.
    elapsed: i64,
    /// A submit dispatch is awaiting its (per-target) outcome.
    in_flight: bool,
    /// ListenBrainz submission progress.
    lb: TargetProgress,
    /// Last.fm submission progress.
    lf: TargetProgress,
    /// Unix seconds the now-playing was last sent, for the keep-alive refresh.
    last_now_playing: i64,
}

impl RadioCurrent {
    /// Both targets have resolved (accepted, given up, or unconfigured) — no
    /// further dispatch is needed for this track.
    fn fully_done(&self) -> bool {
        self.lb.done && self.lf.done
    }
}

/// What the caller should do in response to a [`RadioScrobbleState`] transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RadioScrobbleAction {
    /// Nothing to do this step.
    None,
    /// A new track started — send a now-playing update for it.
    NowPlaying { artist: String, title: String },
    /// Submit a scrobble to `targets`, timestamped at `started_at` (unix seconds,
    /// when the track started). `targets` is narrowed to the targets that still
    /// need submission (not yet succeeded and out of cooldown). The caller
    /// reports each target's outcome back via [`RadioScrobbleState::mark_outcome`].
    Scrobble {
        artist: String,
        title: String,
        started_at: i64,
        targets: ScrobbleTargets,
    },
}

impl RadioScrobbleState {
    /// Observe the current ICY metadata (`artist`/`title` already cleaned and
    /// validated non-empty by the caller). `now` is unix seconds.
    ///
    /// Returns [`RadioScrobbleAction::NowPlaying`] when this is a NEW track
    /// (different `(artist, title)`), starting a fresh listen window; returns
    /// [`RadioScrobbleAction::None`] when it's the same track still playing (so
    /// its accrued time and submit latches are preserved — the dedup guarantee).
    pub fn observe(&mut self, artist: &str, title: &str, now: i64) -> RadioScrobbleAction {
        if let Some(cur) = &self.current
            && cur.artist == artist
            && cur.title == title
        {
            return RadioScrobbleAction::None;
        }
        self.current = Some(RadioCurrent {
            artist: artist.to_string(),
            title: title.to_string(),
            started_at: now,
            last_tick: now,
            elapsed: 0,
            in_flight: false,
            lb: TargetProgress::default(),
            lf: TargetProgress::default(),
            last_now_playing: now,
        });
        RadioScrobbleAction::NowPlaying {
            artist: artist.to_string(),
            title: title.to_string(),
        }
    }

    /// Advance the listen timer. `now` is unix seconds; `playing` is whether
    /// playback is actually active (playing and not paused).
    ///
    /// Accrues the clamped wall-clock delta into `elapsed` only while playing
    /// (so paused time and clock jumps don't count), always advancing
    /// `last_tick` so a resume doesn't credit the paused gap. Returns
    /// [`RadioScrobbleAction::Scrobble`] once `elapsed` reaches `threshold_secs`
    /// and at least one target is ready (not done, out of cooldown) — narrowing
    /// `targets` to exactly those — then latches `in_flight` until
    /// [`Self::mark_outcome`].
    pub fn tick(&mut self, now: i64, playing: bool, threshold_secs: i64) -> RadioScrobbleAction {
        let Some(cur) = &mut self.current else {
            return RadioScrobbleAction::None;
        };
        let delta = (now - cur.last_tick).clamp(0, MAX_RADIO_TICK_DELTA);
        cur.last_tick = now;
        if !playing || cur.in_flight || cur.fully_done() {
            return RadioScrobbleAction::None;
        }
        cur.elapsed += delta;
        if cur.elapsed < threshold_secs {
            return RadioScrobbleAction::None;
        }
        let targets = ScrobbleTargets {
            listenbrainz: cur.lb.ready(now),
            lastfm: cur.lf.ready(now),
        };
        if !targets.any() {
            return RadioScrobbleAction::None;
        }
        cur.in_flight = true;
        RadioScrobbleAction::Scrobble {
            artist: cur.artist.clone(),
            title: cur.title.clone(),
            started_at: cur.started_at,
            targets,
        }
    }

    /// Report the per-target outcome of a dispatched scrobble for `(artist,
    /// title)`. `lb`/`lf` are `Some(true)` on accept, `Some(false)` on a
    /// retryable failure, `None` when that target wasn't attempted (unconfigured
    /// / already done). Each accepted or unattempted target latches done; a
    /// failed target enters a cooldown and re-arms (so a recovered service still
    /// scrobbles) up to [`MAX_RADIO_SUBMIT_ATTEMPTS`]. A result for a track
    /// that's no longer current is ignored.
    pub fn mark_outcome(
        &mut self,
        artist: &str,
        title: &str,
        now: i64,
        lb: Option<bool>,
        lf: Option<bool>,
    ) {
        if let Some(cur) = &mut self.current
            && cur.artist == artist
            && cur.title == title
            && cur.in_flight
        {
            cur.in_flight = false;
            cur.lb.apply(lb, now);
            cur.lf.apply(lf, now);
        }
    }

    /// Whether a now-playing keep-alive is due: returns the current `(artist,
    /// title)` (and resets the timer) when playing and `interval_secs` have
    /// elapsed since the last now-playing send, else `None`. Keeps the service's
    /// now-playing indicator alive on long single-title segments.
    pub fn now_playing_refresh_due(
        &mut self,
        now: i64,
        playing: bool,
        interval_secs: i64,
    ) -> Option<(String, String)> {
        let cur = self.current.as_mut()?;
        if playing && now - cur.last_now_playing >= interval_secs {
            cur.last_now_playing = now;
            return Some((cur.artist.clone(), cur.title.clone()));
        }
        None
    }

    /// Cheap unchanged-check on the raw ICY `(artist, title)`: returns true (and
    /// caches the new value) when it differs from the previous call. Lets the
    /// tick handler skip rebuilding the cleaned `ScrobbleTrack` while the stream
    /// title is unchanged.
    pub fn raw_icy_changed(&mut self, artist: Option<&str>, title: Option<&str>) -> bool {
        let same = matches!(&self.last_raw, Some((a, t))
            if a.as_deref() == artist && t.as_deref() == title);
        if !same {
            self.last_raw = Some((artist.map(str::to_string), title.map(str::to_string)));
        }
        !same
    }

    /// Clear all tracking — call when radio playback stops or the app leaves
    /// radio mode, so the next station starts fresh.
    pub fn clear(&mut self) {
        self.current = None;
        self.last_raw = None;
    }

    /// The current track's `(artist, title)`, or `None` when nothing is
    /// tracked. Test-only accessor used by handler tests to assert the radio
    /// scrobble state without exposing the private `current`.
    #[cfg(test)]
    pub(crate) fn current_key(&self) -> Option<(&str, &str)> {
        self.current
            .as_ref()
            .map(|c| (c.artist.as_str(), c.title.as_str()))
    }
}

#[cfg(test)]
mod radio_tests {
    use super::*;

    const T: i64 = 60; // test threshold (seconds)
    const P: bool = true; // playing

    /// Accrue `secs` of real play time in ≤`MAX_RADIO_TICK_DELTA` steps (ticks
    /// are ~1 s apart in production; the clamp only guards pauses / clock jumps),
    /// starting at wall-clock `start`. Returns `(end_time, action)` where action
    /// is the Scrobble at the crossing step if one occurred, else the last step.
    fn play(s: &mut RadioScrobbleState, start: i64, secs: i64) -> (i64, RadioScrobbleAction) {
        let mut t = start;
        let end = start + secs;
        let mut last = RadioScrobbleAction::None;
        while t < end {
            t = (t + MAX_RADIO_TICK_DELTA).min(end);
            let a = s.tick(t, P, T);
            if matches!(a, RadioScrobbleAction::Scrobble { .. }) {
                return (t, a);
            }
            last = a;
        }
        (t, last)
    }

    #[test]
    fn observe_new_track_emits_now_playing_and_tracks_it() {
        let mut s = RadioScrobbleState::default();
        let action = s.observe("Daft Punk", "Around the World", 1_000);
        assert_eq!(
            action,
            RadioScrobbleAction::NowPlaying {
                artist: "Daft Punk".into(),
                title: "Around the World".into(),
            }
        );
        assert_eq!(s.current_key(), Some(("Daft Punk", "Around the World")));
    }

    #[test]
    fn observe_same_track_is_noop_and_preserves_timer() {
        let mut s = RadioScrobbleState::default();
        s.observe("A", "B", 1_000);
        let (t, a) = play(&mut s, 1_000, 30); // 30s accrued
        assert_eq!(a, RadioScrobbleAction::None);
        // Re-observing the same title must NOT reset the window or re-notify.
        assert_eq!(s.observe("A", "B", t), RadioScrobbleAction::None);
        // +30 more accrued → crosses the 60s threshold; timestamped at start.
        let (_, a) = play(&mut s, t, 30);
        assert!(
            matches!(a, RadioScrobbleAction::Scrobble { started_at, .. } if started_at == 1_000)
        );
    }

    #[test]
    fn tick_scrobbles_once_then_latches_until_result() {
        let mut s = RadioScrobbleState::default();
        s.observe("A", "B", 1_000);
        let (t, a) = play(&mut s, 1_000, 50);
        assert_eq!(a, RadioScrobbleAction::None); // 50s — not yet
        let (t, a) = play(&mut s, t, 10); // reaches 60s
        assert!(matches!(
            a,
            RadioScrobbleAction::Scrobble { started_at, targets, .. }
                if started_at == 1_000 && targets.listenbrainz && targets.lastfm
        ));
        // In-flight: no re-dispatch until the outcome lands.
        assert_eq!(s.tick(t + 1, P, T), RadioScrobbleAction::None);
        // Both targets accept → done — still no re-dispatch.
        s.mark_outcome("A", "B", t, Some(true), Some(true));
        assert_eq!(s.tick(t + 100, P, T), RadioScrobbleAction::None);
    }

    #[test]
    fn retry_only_redispatches_the_failed_target() {
        let mut s = RadioScrobbleState::default();
        s.observe("A", "B", 1_000);
        let (t, a) = play(&mut s, 1_000, 60);
        assert!(matches!(a, RadioScrobbleAction::Scrobble { targets, .. }
            if targets.listenbrainz && targets.lastfm));
        // ListenBrainz accepts, Last.fm fails.
        s.mark_outcome("A", "B", t, Some(true), Some(false));
        // After the cooldown, the retry targets ONLY Last.fm — the succeeded
        // ListenBrainz target is never re-submitted (review #1).
        let a = s.tick(t + RADIO_RETRY_COOLDOWN_SECS, P, T);
        assert!(
            matches!(a, RadioScrobbleAction::Scrobble { targets, .. }
                if !targets.listenbrainz && targets.lastfm),
            "retry must narrow to the failed target only, got {a:?}"
        );
    }

    #[test]
    fn failed_target_waits_for_cooldown_then_rearms() {
        let mut s = RadioScrobbleState::default();
        s.observe("A", "B", 1_000);
        let (t, _) = play(&mut s, 1_000, 60);
        // ListenBrainz fails; Last.fm unconfigured (None → done).
        s.mark_outcome("A", "B", t, Some(false), None);
        // Still inside the cooldown → no immediate hammering.
        assert_eq!(s.tick(t + 1, P, T), RadioScrobbleAction::None);
        // Cooldown elapsed → re-arm (recovery path, review #4).
        assert!(matches!(
            s.tick(t + RADIO_RETRY_COOLDOWN_SECS, P, T),
            RadioScrobbleAction::Scrobble { targets, .. } if targets.listenbrainz
        ));
    }

    #[test]
    fn failed_target_gives_up_after_attempt_cap() {
        let mut s = RadioScrobbleState::default();
        s.observe("A", "B", 1_000);
        let (mut t, a) = play(&mut s, 1_000, 60); // dispatch 1
        assert!(matches!(a, RadioScrobbleAction::Scrobble { .. }));
        // Burn the cap: fail, wait the cooldown, re-dispatch, repeat.
        for attempt in 1..MAX_RADIO_SUBMIT_ATTEMPTS {
            s.mark_outcome("A", "B", t, Some(false), Some(false));
            t += RADIO_RETRY_COOLDOWN_SECS;
            assert!(
                matches!(s.tick(t, P, T), RadioScrobbleAction::Scrobble { .. }),
                "attempt {} should re-dispatch after the cooldown",
                attempt + 1
            );
        }
        // Final failure reaches the cap → give up, no further dispatch.
        s.mark_outcome("A", "B", t, Some(false), Some(false));
        assert_eq!(
            s.tick(t + RADIO_RETRY_COOLDOWN_SECS + 100, P, T),
            RadioScrobbleAction::None
        );
    }

    #[test]
    fn paused_time_does_not_count_toward_threshold() {
        let mut s = RadioScrobbleState::default();
        s.observe("A", "B", 1_000);
        assert_eq!(s.tick(1_010, P, T), RadioScrobbleAction::None); // 10s played
        // Paused for 10 minutes (playing=false) — must NOT accrue.
        assert_eq!(s.tick(1_610, false, T), RadioScrobbleAction::None);
        // Resume: only ~10s more of real play; still under threshold (no
        // instant scrobble from the pause gap).
        assert_eq!(s.tick(1_620, P, T), RadioScrobbleAction::None);
    }

    #[test]
    fn clock_step_is_clamped_not_instant_scrobble() {
        let mut s = RadioScrobbleState::default();
        s.observe("A", "B", 1_000);
        // A forward NTP step of 1000s in one tick is clamped to 10s.
        assert_eq!(s.tick(2_000, P, T), RadioScrobbleAction::None);
        // A backward step yields a negative delta, clamped to 0 (no panic/scrobble).
        assert_eq!(s.tick(1_500, P, T), RadioScrobbleAction::None);
    }

    #[test]
    fn track_change_resets_window_so_short_tracks_do_not_scrobble() {
        let mut s = RadioScrobbleState::default();
        s.observe("A", "B", 1_000);
        let (t, a) = play(&mut s, 1_000, 40); // 40s, under threshold
        assert_eq!(a, RadioScrobbleAction::None);
        // New track before the old one qualified — old never scrobbles.
        assert_eq!(
            s.observe("C", "D", t),
            RadioScrobbleAction::NowPlaying {
                artist: "C".into(),
                title: "D".into(),
            }
        );
        let (t, a) = play(&mut s, t, 40); // new track only 40s in
        assert_eq!(a, RadioScrobbleAction::None);
        let (_, a) = play(&mut s, t, 20); // new track now ≥ 60s
        assert!(
            matches!(a, RadioScrobbleAction::Scrobble { started_at, .. } if started_at == 1_040)
        );
    }

    #[test]
    fn mark_outcome_for_stale_track_is_ignored() {
        let mut s = RadioScrobbleState::default();
        s.observe("A", "B", 1_000);
        let (t, _) = play(&mut s, 1_000, 60); // dispatch for A (in-flight)
        s.observe("C", "D", t); // track changed before A's result
        // A late result for the OLD track must not touch the new one.
        s.mark_outcome("A", "B", t, Some(true), Some(true));
        let (_, a) = play(&mut s, t, 60);
        assert!(matches!(a, RadioScrobbleAction::Scrobble { started_at, .. } if started_at == t));
    }

    #[test]
    fn now_playing_refresh_due_only_after_interval_while_playing() {
        let mut s = RadioScrobbleState::default();
        s.observe("A", "B", 1_000); // sets last_now_playing = 1_000
        assert!(s.now_playing_refresh_due(1_020, P, 30).is_none()); // 20s < 30s
        assert!(s.now_playing_refresh_due(1_031, false, 30).is_none()); // not playing
        assert_eq!(
            s.now_playing_refresh_due(1_031, P, 30), // 31s ≥ 30s → due
            Some(("A".to_string(), "B".to_string()))
        );
        assert!(s.now_playing_refresh_due(1_040, P, 30).is_none()); // timer reset
    }

    #[test]
    fn raw_icy_changed_dedups_unchanged_titles() {
        let mut s = RadioScrobbleState::default();
        assert!(
            s.raw_icy_changed(Some("A"), Some("B")),
            "first sighting is a change"
        );
        assert!(
            !s.raw_icy_changed(Some("A"), Some("B")),
            "same raw ICY is no change"
        );
        assert!(
            s.raw_icy_changed(Some("A"), Some("C")),
            "new title is a change"
        );
        s.clear();
        assert!(
            s.raw_icy_changed(Some("A"), Some("C")),
            "clear() resets the cache"
        );
    }

    #[test]
    fn tick_without_current_track_is_noop() {
        let mut s = RadioScrobbleState::default();
        assert_eq!(s.tick(1_100, P, T), RadioScrobbleAction::None);
    }

    #[test]
    fn clear_drops_tracking() {
        let mut s = RadioScrobbleState::default();
        s.observe("A", "B", 1_000);
        s.clear();
        assert_eq!(s.current_key(), None);
        assert_eq!(s.tick(1_100, P, T), RadioScrobbleAction::None);
    }
}
