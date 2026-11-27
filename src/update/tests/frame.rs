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
    state::{
        AMBIENT_IDLE_AFTER, AMBIENT_INPUT_REARM, ChromeMotion, OutgoingLyrics, TICK_IDLE,
        TICK_LIVELY, TICK_LIVELY_HOLD, VISUALIZER_SETTLE,
    },
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
fn plain_playback_wants_no_frames_and_a_lyric_sheet_does() {
    // `frames_wanted` compares the lyrics target with the published center,
    // a process-global the lyrics motion tests write: publish the sheet's
    // resting line so only the clock term is under test.
    let _guard = super::lyrics::LYRICS_MOTION_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::widgets::lyrics_viewport::set_lyrics_center(0.0);
    let mut app = idle_app();
    app.lyrics.doc.synced = true;
    app.playback.playing = true;
    assert!(
        !app.frames_wanted(),
        "the seek bar, marquee and glow pace their own redraws"
    );

    app.settings.lyrics_mode = LyricsMode::On;
    app.lyrics.matched_song_id = Some("s1".into());
    assert!(
        app.frames_wanted(),
        "the word highlight reads the frame clock"
    );

    app.playback.paused = true;
    assert!(!app.frames_wanted(), "paused: the lyric clock holds");
}

#[test]
fn a_lyric_sheet_wants_frames_only_where_it_is_drawn() {
    // `frames_wanted` compares the lyrics target with the published center,
    // a process-global the lyrics motion tests write.
    let _guard = super::lyrics::LYRICS_MOTION_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::widgets::lyrics_viewport::set_lyrics_center(0.0);
    let mut app = idle_app();
    app.playback.playing = true;
    app.settings.lyrics_mode = LyricsMode::On;
    app.lyrics.matched_song_id = Some("s1".into());
    app.lyrics.doc.synced = true;
    assert!(app.frames_wanted(), "on the Queue cover the clock runs");

    app.current_view = View::Albums;
    assert!(
        !app.frames_wanted(),
        "off the Queue and out of Theater Mode nothing draws the sheet"
    );

    app.playback.paused = true;
    app.lyrics.anim_duration_ms = 500;
    app.lyrics.anim_start = Some(Instant::now());
    assert!(
        !app.frames_wanted(),
        "a glide off screen lands on the next frame back"
    );
    app.lyrics.scroll_to = 3.0;
    assert!(
        !app.frames_wanted(),
        "a snap off screen waits for the surface"
    );
    app.current_view = View::Queue;
    assert!(app.frames_wanted(), "back on the Queue it lands");
}

#[test]
fn a_hidden_lyrics_layer_wants_no_clock_frames() {
    let _guard = super::lyrics::LYRICS_MOTION_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::widgets::lyrics_viewport::set_lyrics_center(0.0);
    let mut app = idle_app();
    app.playback.playing = true;
    // Auto with no verdict for the playing song: the layer stays hidden.
    app.settings.lyrics_mode = LyricsMode::Auto;
    app.scrobble.current_song_id = Some("s1".into());
    app.lyrics.matched_song_id = Some("s1".into());
    app.lyrics.doc.synced = true;
    assert!(!app.lyrics_layer_wanted());
    assert!(
        !app.frames_wanted(),
        "a sheet nobody sees needs no frame clock"
    );
}

#[test]
fn a_paused_visualizer_wants_frames_only_while_its_bars_settle() {
    let mut app = idle_app();
    app.playback.playing = true;
    app.playback.paused = true;
    app.settings.visualization_mode = VisualizationMode::Bars;

    app.idle.transport_seen = Some(Instant::now());
    assert!(app.frames_wanted(), "just paused: the bars and peaks fall");

    app.idle.transport_seen = Some(Instant::now() - VISUALIZER_SETTLE - Duration::from_secs(1));
    assert!(!app.frames_wanted(), "settled: the picture holds still");

    app.idle.transport_seen = None;
    assert!(!app.frames_wanted(), "never played: nothing to settle");

    app.playback.paused = false;
    assert!(app.frames_wanted(), "playing: the visualizer runs");
}

#[test]
fn the_tick_stamps_the_transport_only_while_it_runs() {
    let mut app = idle_app();
    let now = Instant::now();
    app.stamp_transport(now);
    assert!(app.idle.transport_seen.is_none(), "stopped: no stamp");
    app.playback.playing = true;
    app.stamp_transport(now);
    assert_eq!(app.idle.transport_seen, Some(now));
    app.playback.paused = true;
    app.stamp_transport(now + Duration::from_secs(1));
    assert_eq!(app.idle.transport_seen, Some(now), "paused: the stamp ages");
}

#[test]
fn theater_wants_frames_right_after_it_was_raised() {
    let mut app = idle_app();
    app.theater.active = true;
    assert!(
        app.frames_wanted(),
        "theater's chrome is mid-slide right after it was raised"
    );
}

#[test]
fn the_harbour_sea_breathes_while_paused_only_for_a_present_user() {
    let mut app = idle_app();
    app.playback.playing = true;
    app.playback.paused = true;
    app.current_view = View::Harbour;
    app.theater.window_focused = true;

    app.idle.last_input = Some(Instant::now());
    assert!(
        app.frames_wanted(),
        "focused and just touched: the sea breathes"
    );

    app.idle.last_input = Some(Instant::now() - AMBIENT_IDLE_AFTER - Duration::from_secs(1));
    assert!(
        !app.frames_wanted(),
        "a minute without input: the sea holds"
    );

    app.idle.last_input = Some(Instant::now());
    app.theater.window_focused = false;
    assert!(!app.frames_wanted(), "unfocused: the sea holds");

    app.playback.paused = false;
    assert!(app.frames_wanted(), "playing: the scene follows the music");

    app.playback.paused = true;
    app.theater.window_focused = true;
    app.harbour.search_query = "x".into();
    assert!(!app.frames_wanted(), "a Harbour search hides the scene");
}

#[test]
fn the_ambient_listener_runs_only_when_input_could_wake_the_scene() {
    let mut app = idle_app();
    app.current_view = View::Harbour;
    app.idle.last_input = Some(Instant::now() - AMBIENT_INPUT_REARM - Duration::from_secs(1));
    assert!(app.ambient_input_wanted(), "stale input on a still scene");

    app.idle.last_input = Some(Instant::now());
    assert!(
        !app.ambient_input_wanted(),
        "fresh input: a mouse move raises no message"
    );

    app.idle.last_input = None;
    app.playback.playing = true;
    assert!(
        !app.ambient_input_wanted(),
        "playing: the scene runs anyway"
    );

    app.playback.playing = false;
    app.current_view = View::Queue;
    assert!(!app.ambient_input_wanted(), "no scene on screen");
}

#[test]
fn user_activity_and_refocus_stamp_the_user_as_present() {
    let mut app = idle_app();
    app.idle.last_input = None;
    let _ = app.update(crate::app_message::Message::UserActivity);
    assert!(app.idle.input_within(Instant::now(), AMBIENT_INPUT_REARM));

    app.idle.last_input = None;
    let _ = app.update(crate::app_message::Message::WindowFocused);
    assert!(app.idle.input_within(Instant::now(), AMBIENT_INPUT_REARM));
}

#[test]
fn a_lyrics_glide_or_dissolve_in_flight_wants_frames_until_it_settles() {
    // `frames_wanted` compares the target with the published center, a
    // process-global the lyrics motion tests write.
    let _guard = super::lyrics::LYRICS_MOTION_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::widgets::lyrics_viewport::set_lyrics_center(0.0);
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
    assert!(
        !app.frames_wanted(),
        "a snap already published has no motion"
    );
    app.lyrics.scroll_to = 3.0;
    assert!(
        app.frames_wanted(),
        "a snap still to publish needs its frame"
    );
    crate::widgets::lyrics_viewport::set_lyrics_center(3.0);
    assert!(!app.frames_wanted(), "published: nothing left to move");
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

#[test]
fn an_idle_app_ticks_slowly_and_a_lively_one_at_ten_hertz() {
    let mut app = idle_app();
    app.idle.last_event = None;
    assert_eq!(app.tick_interval(), TICK_IDLE, "nothing happens: slow poll");

    app.idle.last_event = Some(Instant::now());
    assert_eq!(app.tick_interval(), TICK_LIVELY, "a message just arrived");
    app.idle.last_event = Some(Instant::now() - TICK_LIVELY_HOLD - Duration::from_secs(1));
    assert_eq!(app.tick_interval(), TICK_IDLE);

    app.idle.transport_seen = Some(Instant::now());
    assert_eq!(
        app.tick_interval(),
        TICK_LIVELY,
        "just stopped: holds play out"
    );
    app.idle.transport_seen = None;

    app.playback.playing = true;
    assert_eq!(app.tick_interval(), TICK_LIVELY, "playing: smooth position");
}

#[test]
fn only_the_idle_poll_leaves_the_tick_slow() {
    use crate::app_message::{Message, PlaybackMessage};

    let mut app = idle_app();
    app.idle.last_event = None;
    let _ = app.update(Message::Playback(PlaybackMessage::Tick));
    let _ = app.update(Message::FrameTick(Instant::now()));
    let _ = app.update(Message::SessionScoped {
        generation: 0,
        message: Box::new(Message::Playback(PlaybackMessage::Tick)),
    });
    assert!(app.idle.last_event.is_none(), "polling is not activity");

    let _ = app.update(Message::NoOp);
    assert!(app.idle.last_event.is_some(), "any other message is");
}

#[test]
fn the_harbour_scene_alone_runs_on_the_ambient_timer() {
    use crate::update::FrameDemand;

    let mut app = idle_app();
    app.playback.playing = true;
    app.current_view = View::Harbour;
    assert_eq!(
        app.frame_demand(),
        FrameDemand::Ambient,
        "only the sea moves"
    );

    app.settings.visualization_mode = VisualizationMode::Bars;
    assert_eq!(
        app.frame_demand(),
        FrameDemand::Display,
        "a visualizer tracks the music at the display's rate"
    );

    app.settings.visualization_mode = VisualizationMode::Off;
    app.current_view = View::Queue;
    assert_eq!(app.frame_demand(), FrameDemand::None);
}
