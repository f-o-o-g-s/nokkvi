//! Scrobble state with anti-seek-fraud accounting.

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
