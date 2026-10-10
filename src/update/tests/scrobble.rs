//! Tests for scrobble submission latching, retry, and now-playing heartbeat.

use crate::{app_message::PlaybackStateUpdate, test_helpers::*};

fn threshold_crossing_update(song_id: &str, pos: u32, dur: u32) -> PlaybackStateUpdate {
    PlaybackStateUpdate {
        position: pos,
        position_ms: pos.saturating_mul(1000),
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
        current_stream_bit_perfect: false,
        bitrate: 1411,
        seekable: true,
        live_transcode: false,
        live_icy_metadata: None,
        bpm: None,
        seek_epoch: 0,
        next_track_epoch: 0,
        station_id: None,
    }
}

fn radio_app() -> crate::Nokkvi {
    let mut app = test_app();
    let station = nokkvi_data::types::radio_station::RadioStation {
        id: "r".into(),
        name: "n".into(),
        stream_url: "u".into(),
        home_page_url: None,
        cover_art: None,
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

    let _ = app.handle_scrobble_submission_result(Ok(app.scrobbled_song("s".into())));

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

// ============================================================================
// Radio scrobble tick handler — gating (code-review #12: handler TDD gap)
// ============================================================================

/// A radio app with enabled radio scrobbling and an ICY-tracked track.
fn radio_app_playing(artist: &str, title: &str) -> crate::Nokkvi {
    let mut app = radio_app();
    app.settings.radio_scrobbling_enabled = true;
    app.settings.radio_now_playing_enabled = true;
    // A real threshold so a single tick doesn't instantly scrobble (which would
    // mask the now-playing-dispatch assertions; the test default is 0).
    app.settings.radio_scrobble_threshold_secs = 60;
    if let crate::state::ActivePlayback::Radio(r) = &mut app.active_playback {
        r.icy_artist = Some(artist.to_string());
        r.icy_title = Some(title.to_string());
    }
    app
}

#[test]
fn radio_tick_disabled_clears_tracking_and_dispatches_nothing() {
    let mut app = radio_app_playing("A", "B");
    app.radio_scrobble.observe("A", "B", 1); // pre-existing tracking
    app.settings.radio_scrobbling_enabled = false;

    let mut tasks = Vec::new();
    app.handle_radio_scrobble_tick(true, false, &mut tasks);

    assert_eq!(
        app.radio_scrobble.current_key(),
        None,
        "disabling radio scrobbling must clear tracking"
    );
    assert!(tasks.is_empty(), "disabled → no dispatch");
}

#[test]
fn radio_tick_clears_tracking_on_queue_playback() {
    let mut app = radio_app_playing("A", "B");
    app.radio_scrobble.observe("A", "B", 1);
    app.active_playback = crate::state::ActivePlayback::Queue;

    let mut tasks = Vec::new();
    app.handle_radio_scrobble_tick(true, false, &mut tasks);

    assert_eq!(
        app.radio_scrobble.current_key(),
        None,
        "leaving radio must clear tracking"
    );
    assert!(tasks.is_empty(), "queue playback → no radio dispatch");
}

#[test]
fn radio_tick_tracks_and_now_plays_on_change_when_active() {
    let mut app = radio_app_playing("Artist", "Song");

    let mut tasks = Vec::new();
    app.handle_radio_scrobble_tick(true, false, &mut tasks);

    assert_eq!(
        app.radio_scrobble.current_key(),
        Some(("Artist", "Song")),
        "an active radio tick must track the ICY track"
    );
    assert!(
        !tasks.is_empty(),
        "a genuine track change while playing must dispatch now-playing"
    );
}

#[test]
fn radio_tick_paused_tracks_but_does_not_now_play() {
    let mut app = radio_app_playing("Artist", "Song");

    let mut tasks = Vec::new();
    app.handle_radio_scrobble_tick(false, true, &mut tasks); // paused

    assert_eq!(
        app.radio_scrobble.current_key(),
        Some(("Artist", "Song")),
        "observe still tracks the track while paused"
    );
    assert!(
        tasks.is_empty(),
        "paused → no now-playing dispatch (timer accrues nothing)"
    );
}

#[test]
fn radio_tick_now_playing_toggle_suppresses_dispatch() {
    let mut app = radio_app_playing("Artist", "Song");
    app.settings.radio_now_playing_enabled = false;

    let mut tasks = Vec::new();
    app.handle_radio_scrobble_tick(true, false, &mut tasks);

    assert!(
        tasks.is_empty(),
        "now-playing toggle off → no dispatch even on a track change"
    );
}

#[test]
fn radio_tick_title_only_icy_is_skipped() {
    let mut app = radio_app_playing("Artist", "Song");
    // Station emits a separator-less title (no artist) → not scrobbleable.
    if let crate::state::ActivePlayback::Radio(r) = &mut app.active_playback {
        r.icy_artist = None;
        r.icy_title = Some("Just A Station ID".to_string());
    }

    let mut tasks = Vec::new();
    app.handle_radio_scrobble_tick(true, false, &mut tasks);

    assert!(tasks.is_empty(), "title-only ICY → nothing dispatched");
}

// ============================================================================
// #30 — the client-id header stops the scrobble echo, so the UI refreshes
// the lists the echo used to refresh
// ============================================================================

/// The submission reads the album and artist ids off the loaded row (queue
/// first, then the Songs view), so a consume that drops the row before the
/// server answers can't lose them.
#[test]
fn scrobbled_song_reads_ids_from_the_loaded_row() {
    use crate::app_message::ScrobbledSong;

    let mut app = test_app();
    app.library.queue_songs = vec![make_queue_song("q1", "One", "Artist", "Album")];
    let mut song = make_song("s1", "Two", "Artist");
    song.album_id = Some("al-s1".into());
    song.artist_id = Some("ar-s1".into());
    seed_songs(&mut app, vec![song]);

    assert_eq!(
        app.scrobbled_song("q1".into()),
        ScrobbledSong {
            song_id: "q1".into(),
            album_id: Some(app.library.queue_songs[0].album_id.clone()),
            artist_id: Some("artist_q1".into()),
        }
    );
    assert_eq!(
        app.scrobbled_song("s1".into()),
        ScrobbledSong {
            song_id: "s1".into(),
            album_id: Some("al-s1".into()),
            artist_id: Some("ar-s1".into()),
        }
    );
    assert_eq!(
        app.scrobbled_song("gone".into()),
        ScrobbledSong {
            song_id: "gone".into(),
            album_id: None,
            artist_id: None,
        }
    );
}

/// A confirmed scrobble reloads the Albums, Artists and Songs lists (play
/// counts, last played, Most Played / Recently Played order), as Navidrome's
/// `refreshResource` echo did before nokkvi tagged its requests.
#[test]
fn confirmed_scrobble_refreshes_the_lists_it_changed() {
    let mut app = test_app();
    seed_albums(&mut app, vec![make_album("a0", "Album 0", "Artist")]);
    seed_artists(&mut app, vec![make_artist("ar0", "Artist 0")]);
    seed_songs(&mut app, vec![make_song("s0", "Song 0", "Artist")]);
    seed_genres(&mut app, vec![make_genre("g0", "Genre 0")]);
    app.library.queue_songs = vec![make_queue_song("q1", "One", "Artist", "Album")];
    app.scrobble.submission_in_flight = true;

    let scrobbled = app.scrobbled_song("q1".into());
    let _ = app.handle_scrobble_submission_result(Ok(scrobbled));

    assert!(app.library.albums.is_loading(), "album play count changed");
    assert!(
        app.library.artists.is_loading(),
        "artist play count changed"
    );
    assert!(app.library.songs.is_loading(), "song play count changed");
    assert!(
        !app.library.genres.is_loading(),
        "a scrobble names no genre"
    );
}

/// A failed submission changed nothing on the server, so nothing reloads.
#[test]
fn failed_scrobble_refreshes_nothing() {
    let mut app = test_app();
    seed_albums(&mut app, vec![make_album("a0", "Album 0", "Artist")]);
    seed_songs(&mut app, vec![make_song("s0", "Song 0", "Artist")]);
    app.scrobble.submission_in_flight = true;

    let _ = app.handle_scrobble_submission_result(Err("HTTP 500".into()));

    assert!(!app.library.albums.is_loading());
    assert!(!app.library.songs.is_loading());
}
