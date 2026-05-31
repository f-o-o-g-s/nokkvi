//! Tests for scrobble submission latching, retry, and now-playing heartbeat.

use crate::{app_message::PlaybackStateUpdate, test_helpers::*};

fn threshold_crossing_update(song_id: &str, pos: u32, dur: u32) -> PlaybackStateUpdate {
    PlaybackStateUpdate {
        position: pos,
        duration: dur,
        playing: true,
        paused: false,
        title: "Song".to_string(),
        artist: "Artist".to_string(),
        album: "Album".to_string(),
        art_url: None,
        random: false,
        repeat: false,
        repeat_queue: false,
        consume: false,
        current_index: Some(0),
        current_entry_id: Some(0),
        song_id: Some(song_id.to_string()),
        format_suffix: "flac".to_string(),
        sample_rate: 44100,
        bitrate: 1411,
        live_icy_metadata: None,
        bpm: None,
    }
}

fn radio_app() -> crate::Nokkvi {
    let mut app = test_app();
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
    app
}

// ============================================================================
// I23 — submission latches on confirmed success, not on intent
// ============================================================================

#[test]
fn submit_dispatch_does_not_latch_submitted_on_intent() {
    let mut app = test_app();
    // Same song already tracked; nearly all of it listened (200s track, 50%
    // default threshold crossed). The tick pushes a Submit.
    app.scrobble.current_song_id = Some("s".to_string());
    app.scrobble.current_song_duration = 200;
    app.scrobble.last_position = 100.0;
    app.scrobble.listening_time = 105.0;
    app.scrobble.submitted = false;
    app.scrobble.submission_in_flight = false;
    app.settings.scrobble_threshold = 0.5;

    let _ = app.handle_playback_state_updated(threshold_crossing_update("s", 101, 200));

    // Intent must NOT latch `submitted`; only a confirmed result may. The
    // in-flight guard is raised so further ticks do not spam duplicate GETs.
    assert!(
        !app.scrobble.submitted,
        "submitted must not latch on submission intent"
    );
    assert!(
        app.scrobble.submission_in_flight,
        "in-flight guard must be raised while a submission is pending"
    );
}

#[test]
fn failed_submission_leaves_retryable() {
    let mut app = test_app();
    app.scrobble.submitted = false;
    app.scrobble.submission_in_flight = true;

    let _ = app.handle_scrobble_submission_result(Err("boom".to_string()));

    assert!(
        !app.scrobble.submitted,
        "a failed submission must not latch submitted"
    );
    assert!(
        !app.scrobble.submission_in_flight,
        "a failed submission must clear the in-flight guard so the next tick retries"
    );
}

#[test]
fn successful_submission_latches_submitted() {
    let mut app = test_app();
    app.scrobble.submitted = false;
    app.scrobble.submission_in_flight = true;

    let _ = app.handle_scrobble_submission_result(Ok("s".to_string()));

    assert!(
        app.scrobble.submitted,
        "a confirmed submission must latch submitted"
    );
    assert!(
        !app.scrobble.submission_in_flight,
        "a confirmed submission must clear the in-flight guard"
    );
}

// ============================================================================
// I24 — now-playing heartbeat (resume re-emit + periodic refresh)
// ============================================================================

#[test]
fn resume_rearms_now_playing() {
    let mut app = test_app();
    // Establish a tracked song and a known timer generation.
    app.scrobble.current_song_id = Some("s".to_string());
    app.scrobble.now_playing_timer_id = 5;
    // Simulate a paused, loaded track (resume case).
    app.playback.playing = true;
    app.playback.paused = true;

    let g1 = app.scrobble.now_playing_timer_id;
    let _ = app.handle_toggle_play();

    assert!(
        app.scrobble.now_playing_timer_id > g1,
        "resuming a paused track must re-arm the now-playing timer"
    );
}

#[test]
fn now_playing_refresh_rearms_when_live() {
    let mut app = test_app();
    app.settings.scrobbling_enabled = true;
    app.scrobble.now_playing_timer_id = 7;
    app.playback.playing = true;
    app.playback.paused = false;
    // active_playback defaults to Queue.

    let _ = app.handle_scrobble_now_playing_refresh(7, "s".to_string());
    // A live refresh re-arms — the timer generation advances.
    assert!(
        app.scrobble.now_playing_timer_id > 7,
        "a live heartbeat must re-arm (bump the timer generation)"
    );
}

#[test]
fn now_playing_refresh_noop_when_stale() {
    let mut app = test_app();
    app.settings.scrobbling_enabled = true;
    app.scrobble.now_playing_timer_id = 7;
    app.playback.playing = true;
    app.playback.paused = false;

    // Stale generation (a song change already bumped the id).
    let _ = app.handle_scrobble_now_playing_refresh(6, "s".to_string());
    assert_eq!(
        app.scrobble.now_playing_timer_id, 7,
        "a stale heartbeat must be a no-op (no re-arm)"
    );
}

#[test]
fn now_playing_refresh_noop_when_paused() {
    let mut app = test_app();
    app.settings.scrobbling_enabled = true;
    app.scrobble.now_playing_timer_id = 7;
    app.playback.playing = true;
    app.playback.paused = true;

    let _ = app.handle_scrobble_now_playing_refresh(7, "s".to_string());
    assert_eq!(
        app.scrobble.now_playing_timer_id, 7,
        "a paused heartbeat must be a no-op (no re-arm)"
    );
}

#[test]
fn now_playing_refresh_noop_when_radio() {
    let mut app = radio_app();
    app.settings.scrobbling_enabled = true;
    app.scrobble.now_playing_timer_id = 7;
    app.playback.playing = true;
    app.playback.paused = false;

    let _ = app.handle_scrobble_now_playing_refresh(7, "s".to_string());
    assert_eq!(
        app.scrobble.now_playing_timer_id, 7,
        "radio playback must not heartbeat now-playing (no re-arm)"
    );
}
