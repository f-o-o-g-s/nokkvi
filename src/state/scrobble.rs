//! Scrobble state with anti-seek-fraud accounting.

/// Scrobbling state with anti-seek-fraud protection
#[derive(Debug, Clone, Default)]
pub struct ScrobbleState {
    /// Actual seconds listened (not playback position) - prevents seek-fraud
    pub listening_time: f32,
    /// Last known position for calculating listening time deltas
    pub last_position: f32,
    /// Whether current song has been submitted (prevents double-scrobble)
    pub submitted: bool,
    /// Timer ID for debounced "now playing" notification
    pub now_playing_timer_id: u64,
    /// Current song ID for scrobble tracking
    pub current_song_id: Option<String>,
}

impl ScrobbleState {
    /// Reset for a new song
    pub fn reset_for_new_song(&mut self, song_id: Option<String>, position: f32) {
        self.current_song_id = song_id;
        self.listening_time = 0.0;
        self.last_position = position;
        self.submitted = false;
    }

    /// Check if scrobble conditions are met for the given track duration.
    /// Returns true if accumulated listening time meets the configured percentage
    /// of track duration and the song hasn't been submitted yet.
    pub fn should_scrobble(&self, track_duration: u32, threshold_percent: f32) -> bool {
        if self.submitted || track_duration == 0 {
            return false;
        }
        self.listening_time >= (track_duration as f32 * threshold_percent)
    }
}
