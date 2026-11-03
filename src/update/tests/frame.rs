//! The per-frame subscription's gate (`Nokkvi::frames_wanted`): the
//! display-rate redraw loop (`iced::window::frames()`) runs only while
//! something on screen animates, so a paused app idles on the 10 Hz position
//! tick instead of redrawing at the monitor's rate (14% of a core at 240 Hz).

use std::time::{Duration, Instant};

use nokkvi_data::types::{
    lyrics::LrcDocument,
    player_settings::{LyricsMode, TheaterControls, VisualizationMode},
};

use crate::{
    Screen, View,
    state::{ChromeMotion, OutgoingLyrics},
    test_helpers::*,
};

/// A stopped app on the Queue with every animated feature off.
fn idle_app() -> crate::Nokkvi {
    let mut app = test_app();
    app.playback.playing = false;
    app.playback.paused = false;
    app.settings.visualization_mode = VisualizationMode::Off;
    app.theater.active = false;
    app.screen = Screen::Home;
    app.current_view = View::Queue;
    app
}

#[test]
fn an_idle_app_wants_no_frames() {
    assert!(
        !idle_app().frames_wanted(),
        "nothing animates: no display-rate loop"
    );
}

#[test]
fn a_running_transport_wants_frames_and_a_paused_one_does_not() {
    let mut app = idle_app();
    app.playback.playing = true;
    assert!(
        app.frames_wanted(),
        "the glow, the lyric clock and the boat step while playing"
    );
    app.playback.paused = true;
    assert!(!app.frames_wanted(), "paused: every per-frame step holds");
}

#[test]
fn the_visualizer_theater_and_harbour_want_frames_while_paused() {
    let mut app = idle_app();
    app.playback.playing = true;
    app.playback.paused = true;

    app.settings.visualization_mode = VisualizationMode::Bars;
    assert!(
        app.frames_wanted(),
        "the shaders animate on their own clock"
    );
    app.settings.visualization_mode = VisualizationMode::Off;

    app.theater.active = true;
    assert!(
        app.frames_wanted(),
        "theater's chrome is mid-slide right after it was raised"
    );
    app.theater.active = false;

    app.current_view = View::Harbour;
    assert!(
        app.frames_wanted(),
        "the Harbour sea keeps breathing while paused"
    );
    app.harbour.search_query = "x".into();
    assert!(!app.frames_wanted(), "a Harbour search hides the scene");
}

#[test]
fn a_lyrics_glide_or_dissolve_in_flight_wants_frames_until_it_settles() {
    let mut app = idle_app();
    app.playback.playing = true;
    app.playback.paused = true;
    app.settings.lyrics_mode = LyricsMode::On;
    app.lyrics.matched_song_id = Some("s1".into());
    app.lyrics.doc.synced = true;
    app.lyrics.anim_duration_ms = 500;

    app.lyrics.anim_start = Some(Instant::now());
    assert!(app.frames_wanted(), "a glide eases toward its line");
    app.lyrics.anim_start = Some(Instant::now() - Duration::from_millis(600));
    assert!(!app.frames_wanted(), "a settled glide needs nothing");
    app.lyrics.anim_start = Some(Instant::now());
    app.lyrics.anim_duration_ms = 0;
    assert!(!app.frames_wanted(), "a snap has no motion");
    app.lyrics.anim_duration_ms = 500;
    app.settings.lyrics_mode = LyricsMode::Off;
    assert!(!app.frames_wanted(), "with lyrics off nothing is drawn");

    app.settings.lyrics_mode = LyricsMode::Auto;
    app.lyrics.anim_start = None;
    app.lyrics.outgoing = Some(OutgoingLyrics {
        doc: LrcDocument::default(),
        synced: true,
        center: 0.0,
        started: Instant::now(),
        duration_ms: 400,
    });
    assert!(app.frames_wanted(), "a dissolving sheet cross-blends");
    app.lyrics.outgoing = None;
    assert!(!app.frames_wanted());
}

#[test]
fn theater_wants_frames_only_while_its_chrome_slides_or_is_due_to_flip() {
    let settled = Instant::now() - Duration::from_secs(5);
    let mut app = idle_app();
    app.playback.playing = true;
    app.playback.paused = true;
    app.theater.active = true;
    app.settings.theater_controls = TheaterControls::AutoHide;
    app.theater.window_focused = true;
    app.theater.last_activity = None;

    app.theater.chrome = ChromeMotion::Hidden {
        since: settled,
        from: 0.0,
    };
    assert!(!app.frames_wanted(), "hidden at rest, nothing due: idle");

    app.theater.chrome = ChromeMotion::Shown {
        since: settled,
        from: 1.0,
    };
    assert!(
        app.frames_wanted(),
        "shown with no recent activity: the hide is due, the tick must run"
    );

    app.theater.last_activity = Some(Instant::now());
    assert!(
        !app.frames_wanted(),
        "shown and in use: at rest until the hide delay runs out"
    );

    app.theater.chrome = ChromeMotion::Hidden {
        since: settled,
        from: 0.0,
    };
    assert!(
        app.frames_wanted(),
        "hidden but just touched: the show is due"
    );

    app.theater.chrome = ChromeMotion::Hidden {
        since: Instant::now(),
        from: 0.5,
    };
    app.theater.last_activity = None;
    assert!(app.frames_wanted(), "a slide in flight");

    app.settings.theater_controls = TheaterControls::AlwaysShown;
    app.theater.chrome = ChromeMotion::Shown {
        since: settled,
        from: 0.0,
    };
    assert!(
        !app.frames_wanted(),
        "a bar that is always shown never moves"
    );
}
