//! Rating-reminder notification handlers + suppression logic.
//!
//! The reminder is driven from two trigger sites (the scrobble-confirmed arm
//! in `scrobbling.rs` and the percentage-played check in the playback tick),
//! both of which funnel through [`Nokkvi::maybe_fire_rating_reminder`]. The
//! suppression predicate [`Nokkvi::should_send_rating_reminder`] is pure so it
//! can be unit-tested against observable state.

use iced::Task;
use tracing::debug;

use crate::{Message, Nokkvi, services::notifications::NotificationEvent};

/// Tracks shorter than this (seconds) never trigger a rating reminder — very
/// short interludes/intros rarely warrant a prompt and firing on them is noise.
const RATING_REMINDER_MIN_TRACK_SECS: u32 = 30;

impl Nokkvi {
    /// Handle an event from the rating-reminder notification service.
    pub(crate) fn handle_notification(&mut self, event: NotificationEvent) -> Task<Message> {
        match event {
            NotificationEvent::Connected(connection) => {
                debug!(" [NOTIFY] Rating-reminder service connected");
                self.notification_connection = Some(connection);
                Task::none()
            }
        }
    }

    /// Whether a rating reminder should fire for `song_id` right now.
    ///
    /// Pure predicate (no side effects) so it is unit-testable. Suppresses when
    /// the feature is off, during radio playback, for a track already reminded
    /// this session, for an already-rated or loved track, for an unknown song,
    /// and for very short tracks.
    pub(crate) fn should_send_rating_reminder(&self, song_id: &str) -> bool {
        if !self.settings.rating_reminder_enabled {
            return false;
        }
        // Radio has no rateable song; only remind during queue playback.
        if !self.active_playback.is_queue() {
            return false;
        }
        // Once per track per session — also covers repeat-one loops, which
        // clear the scrobble `submitted` latch each lap.
        if self.last_reminded_song_id.as_deref() == Some(song_id) {
            return false;
        }
        // Look the song up once; an unknown id has nothing to rate.
        let Some(song) = self.library.queue_songs.iter().find(|s| s.id == song_id) else {
            return false;
        };
        // Already curated — rated (loving force-sets rating to 5) or loved.
        if song.rating.unwrap_or(0) > 0 || song.starred {
            return false;
        }
        // Very short tracks are noise.
        if song.duration_seconds < RATING_REMINDER_MIN_TRACK_SECS {
            return false;
        }
        true
    }

    /// Fire a rating reminder for `song_id` when
    /// [`Self::should_send_rating_reminder`] allows it. Latches
    /// `last_reminded_song_id` (the once-per-track guard) *before* delivery so a
    /// dropped notification cannot cause a re-fire, then pushes the reminder to
    /// the notification service if it is connected.
    pub(crate) fn maybe_fire_rating_reminder(&mut self, song_id: &str) {
        if !self.should_send_rating_reminder(song_id) {
            return;
        }
        self.last_reminded_song_id = Some(song_id.to_string());

        let Some((title, artist)) = self
            .library
            .queue_songs
            .iter()
            .find(|s| s.id == song_id)
            .map(|s| (s.title.clone(), s.artist.clone()))
        else {
            return;
        };
        if let Some(connection) = &self.notification_connection {
            connection.show_rating_reminder(title, artist);
        }
    }
}
