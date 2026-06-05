//! Tests for the rating-reminder suppression predicate and fire latch.

use crate::test_helpers::*;

/// Build an app primed for a reminder: feature on, one unrated, unstarred,
/// long-enough song in the queue, queue playback (the default). `make_queue_song`
/// defaults rating to `None`, starred to `false`, and duration to 180s.
fn reminder_app(song_id: &str) -> crate::Nokkvi {
    let mut app = test_app();
    app.settings.rating_reminder_enabled = true;
    app.library.queue_songs = vec![make_queue_song(song_id, "Title", "Artist", "Album")];
    app
}

#[test]
fn fires_for_unrated_queue_song() {
    let app = reminder_app("s1");
    assert!(app.should_send_rating_reminder("s1"));
}

#[test]
fn suppressed_when_disabled() {
    let mut app = reminder_app("s1");
    app.settings.rating_reminder_enabled = false;
    assert!(!app.should_send_rating_reminder("s1"));
}

#[test]
fn suppressed_when_already_rated() {
    let mut app = reminder_app("s1");
    app.library.queue_songs[0].rating = Some(3);
    assert!(!app.should_send_rating_reminder("s1"));
}

#[test]
fn suppressed_when_loved() {
    let mut app = reminder_app("s1");
    app.library.queue_songs[0].starred = true;
    assert!(!app.should_send_rating_reminder("s1"));
}

#[test]
fn suppressed_during_radio() {
    let mut app = reminder_app("s1");
    let station = nokkvi_data::types::radio_station::RadioStation {
        id: "r".into(),
        name: "n".into(),
        stream_url: "u".into(),
        home_page_url: None,
    };
    app.active_playback = crate::state::ActivePlayback::Radio(crate::state::RadioPlaybackState {
        station,
        icy_artist: None,
        icy_title: None,
        icy_url: None,
    });
    assert!(!app.should_send_rating_reminder("s1"));
}

#[test]
fn suppressed_when_already_reminded_this_song() {
    let mut app = reminder_app("s1");
    app.last_reminded_song_id = Some("s1".to_string());
    assert!(!app.should_send_rating_reminder("s1"));
}

#[test]
fn fires_again_for_a_different_song() {
    let mut app = reminder_app("s1");
    app.library
        .queue_songs
        .push(make_queue_song("s2", "T2", "A2", "Al2"));
    app.last_reminded_song_id = Some("s1".to_string());
    // s1 suppressed (already reminded), s2 still eligible.
    assert!(!app.should_send_rating_reminder("s1"));
    assert!(app.should_send_rating_reminder("s2"));
}

#[test]
fn suppressed_for_short_track() {
    let mut app = reminder_app("s1");
    app.library.queue_songs[0].duration_seconds = 20; // below the 30s floor
    assert!(!app.should_send_rating_reminder("s1"));
}

#[test]
fn suppressed_for_unknown_song() {
    let app = reminder_app("s1");
    assert!(!app.should_send_rating_reminder("nope"));
}

#[test]
fn maybe_fire_latches_last_reminded_on_happy_path() {
    let mut app = reminder_app("s1");
    assert_eq!(app.last_reminded_song_id, None);
    app.maybe_fire_rating_reminder("s1");
    assert_eq!(app.last_reminded_song_id.as_deref(), Some("s1"));
}

#[test]
fn maybe_fire_does_not_latch_when_suppressed() {
    let mut app = reminder_app("s1");
    app.settings.rating_reminder_enabled = false;
    app.maybe_fire_rating_reminder("s1");
    assert_eq!(app.last_reminded_song_id, None);
}

// --- Trigger wiring: the scrobble-confirmed arm --------------------------------

#[test]
fn scrobble_trigger_fires_reminder_on_confirmed_scrobble() {
    let mut app = reminder_app("s1");
    // Default trigger is On Scrobble.
    let _ = app.handle_scrobble_submission_result(Ok("s1".to_string()));
    assert_eq!(app.last_reminded_song_id.as_deref(), Some("s1"));
}

#[test]
fn scrobble_trigger_silent_in_percentage_mode() {
    use nokkvi_data::types::player_settings::RatingReminderTrigger;
    let mut app = reminder_app("s1");
    app.settings.rating_reminder_trigger = RatingReminderTrigger::PercentagePlayed;
    let _ = app.handle_scrobble_submission_result(Ok("s1".to_string()));
    assert_eq!(
        app.last_reminded_song_id, None,
        "scrobble confirmation must not remind while in percentage mode"
    );
}
