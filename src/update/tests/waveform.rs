//! Waveform seek bar: the tick's bookkeeping and the result handlers. The
//! scheduling decisions themselves are pinned in `state::waveform`; the look
//! is the owner's to judge in the running app.
//!
//! The seek bar style lives in process-global theme atomics, so the tests that
//! set it take `THEME_MODE_LOCK` and restore what they changed.

use std::sync::Arc;

use nokkvi_data::{
    services::waveform::ScanEnd,
    types::{
        player_settings::{SeekbarStyle, TrackInfoDisplay},
        waveform::Waveform,
    },
};

use crate::{
    Nokkvi,
    app_message::{Message, PlaybackMessage, WaveformMessage},
    test_helpers::*,
    theme::{self, THEME_MODE_LOCK},
};

fn wf() -> Arc<Waveform> {
    Arc::new(Waveform::new(vec![1, 2], vec![1, 2]).expect("waveform"))
}

/// An app playing queue song `id`.
fn playing(id: &str) -> Nokkvi {
    let mut app = test_app();
    app.playback.playing = true;
    app.scrobble.current_song_id = Some(id.to_owned());
    app
}

fn send(app: &mut Nokkvi, message: WaveformMessage) {
    let _ = app.update(Message::Waveform(message));
}

/// Run one tick with the given bar style, restoring the style after.
fn tick_with(app: &mut Nokkvi, style: SeekbarStyle) {
    let saved = (theme::track_info_display(), theme::seekbar_style());
    theme::set_track_info_display(TrackInfoDisplay::PlayerBarAbove);
    theme::set_seekbar_style(style);
    let _ = app.update(Message::Playback(PlaybackMessage::Tick));
    theme::set_track_info_display(saved.0);
    theme::set_seekbar_style(saved.1);
}

#[test]
fn the_tick_follows_the_playing_song_only_in_the_waveform_style() {
    let _guard = THEME_MODE_LOCK.lock();
    let mut app = playing("a");
    tick_with(&mut app, SeekbarStyle::Classic);
    assert_eq!(app.waveform.current, None);
    tick_with(&mut app, SeekbarStyle::Waveform);
    assert_eq!(app.waveform.current.as_deref(), Some("a"));
    // Stopped: nothing to show.
    app.playback.playing = false;
    tick_with(&mut app, SeekbarStyle::Waveform);
    assert_eq!(app.waveform.current, None);
}

#[test]
fn the_mini_player_follows_its_own_style() {
    let _guard = THEME_MODE_LOCK.lock();
    let saved = (
        theme::track_info_display(),
        theme::mini_player_seekbar_style(),
    );
    let mut app = playing("a");
    theme::set_track_info_display(TrackInfoDisplay::MiniPlayer);
    theme::set_mini_player_seekbar_style(SeekbarStyle::Waveform);
    let _ = app.update(Message::Playback(PlaybackMessage::Tick));
    theme::set_track_info_display(saved.0);
    theme::set_mini_player_seekbar_style(saved.1);
    assert_eq!(app.waveform.current.as_deref(), Some("a"));
}

#[test]
fn a_cached_envelope_reaches_the_bar_for_the_playing_song() {
    let mut app = playing("a");
    app.waveform.follow(Some("a"));
    send(
        &mut app,
        WaveformMessage::Looked {
            song_id: "a".into(),
            waveform: Some(wf()),
        },
    );
    assert!(app.waveform_for_bar().is_some());
    // Between a track change and the next tick, the old song's envelope never
    // draws under the new song.
    app.scrobble.current_song_id = Some("b".into());
    assert!(app.waveform_for_bar().is_none());
}

#[test]
fn a_cache_miss_is_remembered_so_the_scan_comes_next() {
    let mut app = playing("a");
    app.waveform.follow(Some("a"));
    send(
        &mut app,
        WaveformMessage::Looked {
            song_id: "a".into(),
            waveform: None,
        },
    );
    assert!(app.waveform.not_cached.contains("a"));
}

#[test]
fn a_scanned_envelope_is_shown_and_no_longer_counts_as_uncached() {
    let mut app = playing("a");
    app.waveform.follow(Some("a"));
    app.waveform.not_cached.insert("a".into());
    send(
        &mut app,
        WaveformMessage::Scanned {
            song_id: "a".into(),
            end: Ok(ScanEnd::Ready(wf())),
            scanning: true,
        },
    );
    assert!(app.waveform.shown.is_some());
    assert!(!app.waveform.not_cached.contains("a"));
}

#[test]
fn a_too_slow_scan_turns_scanning_off_for_the_session() {
    let mut app = playing("a");
    app.waveform.follow(Some("a"));
    send(
        &mut app,
        WaveformMessage::Scanned {
            song_id: "a".into(),
            end: Ok(ScanEnd::TooSlow { speed: 3.0 }),
            scanning: false,
        },
    );
    assert!(app.waveform.scans_off);
    assert!(app.waveform.gave_up.contains("a"));
}

#[test]
fn a_failed_or_short_scan_gives_up_for_this_play_only() {
    let mut app = playing("a");
    app.waveform.follow(Some("a"));
    send(
        &mut app,
        WaveformMessage::Scanned {
            song_id: "a".into(),
            end: Err("connection reset".into()),
            scanning: true,
        },
    );
    assert!(app.waveform.gave_up.contains("a"));
    assert!(!app.waveform.scans_off);
    app.waveform.follow(Some("b"));
    assert!(app.waveform.gave_up.is_empty());
}

#[test]
fn a_next_song_answer_for_a_song_no_longer_playing_is_ignored() {
    let mut app = playing("b");
    app.waveform.follow(Some("b"));
    send(
        &mut app,
        WaveformMessage::NextFound {
            for_current: "a".into(),
            next: Some("c".into()),
        },
    );
    assert_eq!(app.waveform.next, None);
    send(
        &mut app,
        WaveformMessage::NextFound {
            for_current: "b".into(),
            next: Some("c".into()),
        },
    );
    assert_eq!(app.waveform.next.as_deref(), Some("c"));
    assert_eq!(app.waveform.next_for.as_deref(), Some("b"));
}

#[test]
fn logout_drops_the_envelopes() {
    let mut app = playing("a");
    app.waveform.follow(Some("a"));
    app.waveform.place("a", wf());
    app.waveform.scans_off = true;
    let _ = app.reset_session_state();
    assert!(app.waveform.shown.is_none());
    assert!(app.waveform.current.is_none());
    assert!(!app.waveform.scans_off, "the next session scans again");
}
