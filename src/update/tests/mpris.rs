//! Tests for MPRIS event handlers.
//!
//! Pure-function coverage for the `LoopStatus` → `RepeatMode` mapping that
//! powers `MprisEvent::SetLoopStatus`. The bug fixed in this module
//! (NF1) was that the SetLoopStatus arm previously dispatched
//! `PlaybackMessage::ToggleRepeat`, which calls `cycle_repeat()` and
//! cycles `None → Track → Playlist → None`. From state `Track`,
//! `playerctl loop Track` would advance to `Playlist`; from state
//! `Playlist`, `playerctl loop Playlist` would advance to `None`; and
//! from state `None`, both `Track` and `Playlist` collapsed to `Track`.
//!
//! The fix routes through a direct setter, so the mapping is exhaustive
//! and idempotent.
//!
//! Those tests assert the pure mapping fn — no `Nokkvi` state needed. The
//! `Raise` tests at the bottom drive `handle_mpris` against `test_app()` and
//! assert on the window state `show_window` owns.

use mpris_server::LoopStatus;
use nokkvi_data::types::queue::RepeatMode;

use crate::{
    services::mpris::MprisEvent, test_helpers::test_app, update::mpris::loop_status_to_repeat_mode,
};

#[test]
fn loop_status_none_maps_to_repeat_none() {
    assert_eq!(
        loop_status_to_repeat_mode(LoopStatus::None),
        RepeatMode::None
    );
}

#[test]
fn loop_status_track_maps_to_repeat_track() {
    assert_eq!(
        loop_status_to_repeat_mode(LoopStatus::Track),
        RepeatMode::Track
    );
}

#[test]
fn loop_status_playlist_maps_to_repeat_playlist() {
    assert_eq!(
        loop_status_to_repeat_mode(LoopStatus::Playlist),
        RepeatMode::Playlist
    );
}

// ============================================================================
// Raise — routed to `Nokkvi::show_window`, same as the `show` IPC verb
// ============================================================================

#[test]
fn raise_reopens_a_tray_hidden_window() {
    let mut app = test_app();
    app.tray_window_hidden = true;
    app.main_window_id = None;

    let _ = app.handle_mpris(MprisEvent::Raise);

    assert!(
        !app.tray_window_hidden,
        "Raise must reopen a window closed to the tray"
    );
    assert_eq!(
        app.main_window_id, None,
        "the new id arrives via WindowOpened"
    );
}

#[test]
fn raise_with_an_open_window_leaves_it_alone() {
    let mut app = test_app();
    let id = iced::window::Id::unique();
    app.main_window_id = Some(id);

    let _ = app.handle_mpris(MprisEvent::Raise);

    assert_eq!(app.main_window_id, Some(id), "the open window is kept");
    assert!(!app.tray_window_hidden);
}

// ============================================================================
// Conformance: mpris:trackid, CanSeek and SetPosition
// ============================================================================

use mpris_server::TrackId;

use crate::{
    app_message::PlaybackStateUpdate,
    services::mpris::{MprisItem, PublishedTrack, track_id_path},
    test_helpers::seed_radio_playback,
    update::tests::playback::make_playback_update,
};

#[test]
fn track_id_names_the_item_under_its_own_prefix() {
    assert_eq!(
        track_id_path(MprisItem::Song("abc123")),
        "/org/nokkvi/track/abc123"
    );
    assert_eq!(
        track_id_path(MprisItem::Station("r1")),
        "/org/nokkvi/station/r1"
    );
}

#[test]
fn track_id_is_no_track_when_nothing_is_loaded() {
    let no_track = TrackId::NO_TRACK.as_str().to_string();
    assert_eq!(track_id_path(MprisItem::Nothing), no_track);
    assert_eq!(track_id_path(MprisItem::Song("")), no_track);
    assert_eq!(track_id_path(MprisItem::Station("")), no_track);
}

#[test]
fn track_id_escapes_ids_into_a_valid_object_path() {
    // UUID-style and odd ids: every byte outside [A-Za-z0-9] is escaped.
    for id in ["0b1f-4e2a", "a_b", "a/b", "ünï", "x.y z"] {
        let path = track_id_path(MprisItem::Song(id));
        assert!(
            TrackId::try_from(path.as_str()).is_ok(),
            "{id:?} → {path:?} must be a valid object path"
        );
        assert_ne!(path, TrackId::NO_TRACK.as_str(), "{id:?}");
    }
    assert_eq!(
        track_id_path(MprisItem::Song("0b1f-4e2a")),
        "/org/nokkvi/track/0b1f_2d4e2a"
    );
}

#[test]
fn track_id_escaping_never_merges_two_ids() {
    // A lossy `-` → `_` swap would hand both the same path, and a stale
    // SetPosition for one would seek the other.
    let pairs = [("a-b", "a_b"), ("a_b", "a_5fb"), ("a/b", "a_2fb")];
    for (x, y) in pairs {
        assert_ne!(
            track_id_path(MprisItem::Song(x)),
            track_id_path(MprisItem::Song(y)),
            "{x:?} vs {y:?}"
        );
    }
}

#[test]
fn only_a_song_of_known_length_can_seek() {
    assert!(PublishedTrack::for_item(MprisItem::Song("s"), 200_000_000, true).can_seek);
    assert!(!PublishedTrack::for_item(MprisItem::Song("s"), 0, true).can_seek);
    assert!(!PublishedTrack::for_item(MprisItem::Station("r1"), 0, true).can_seek);
    assert!(!PublishedTrack::for_item(MprisItem::Nothing, 0, true).can_seek);
    assert!(!PublishedTrack::default().can_seek);
    assert!(
        !PublishedTrack::for_item(MprisItem::Song("s"), 200_000_000, false).can_seek,
        "not loaded"
    );
}

fn song_track() -> PublishedTrack {
    PublishedTrack::for_item(MprisItem::Song("song_1"), 200_000_000, true)
}

#[test]
fn set_position_targets_the_current_track_inside_its_length() {
    let track = song_track();
    let id = track.track_id.clone();
    assert_eq!(track.set_position_target(&id, 30_000_000), Some(30.0));
    assert_eq!(track.set_position_target(&id, 0), Some(0.0));
    assert_eq!(track.set_position_target(&id, 200_000_000), Some(200.0));
}

#[test]
fn set_position_ignores_what_the_spec_says_to_ignore() {
    let track = song_track();
    let id = track.track_id.clone();
    let stale = track_id_path(MprisItem::Song("song_0"));
    assert_eq!(
        track.set_position_target(&stale, 30_000_000),
        None,
        "stale id"
    );
    assert_eq!(track.set_position_target(&id, -1), None, "negative");
    assert_eq!(
        track.set_position_target(&id, 200_000_001),
        None,
        "past the end"
    );
    let station = PublishedTrack::for_item(MprisItem::Station("r1"), 0, true);
    let station_id = station.track_id.clone();
    assert_eq!(
        station.set_position_target(&station_id, 0),
        None,
        "CanSeek false"
    );
}

/// `handle_seek` writes `scrobble.last_position` before anything else, so it
/// is the observable trace of a dispatched seek.
const UNTOUCHED: f32 = -7.0;

#[test]
fn a_stale_or_out_of_range_set_position_does_not_seek() {
    let mut app = test_app();
    app.mpris_track = song_track();
    app.scrobble.last_position = UNTOUCHED;

    let _ = app.handle_mpris(MprisEvent::SetPosition {
        track_id: track_id_path(MprisItem::Song("song_0")),
        position_us: 30_000_000,
    });
    let _ = app.handle_mpris(MprisEvent::SetPosition {
        track_id: app.mpris_track.track_id.clone(),
        position_us: 500_000_000,
    });

    assert_eq!(app.scrobble.last_position, UNTOUCHED);
}

#[test]
fn a_current_set_position_seeks() {
    let mut app = test_app();
    app.mpris_track = song_track();
    app.scrobble.last_position = UNTOUCHED;

    let _ = app.handle_mpris(MprisEvent::SetPosition {
        track_id: app.mpris_track.track_id.clone(),
        position_us: 30_000_000,
    });

    assert_eq!(app.scrobble.last_position, 30.0);
}

#[test]
fn a_queue_tick_publishes_a_seekable_song() {
    let mut app = test_app();
    let _ = app.handle_playback_state_updated(make_playback_update());
    assert_eq!(app.mpris_track, song_track());
}

#[test]
fn a_station_tick_publishes_an_unseekable_station() {
    let mut app = test_app();
    seed_radio_playback(&mut app);
    let _ = app.handle_playback_state_updated(PlaybackStateUpdate {
        song_id: Some("r1".to_string()),
        station_id: Some("r1".to_string()),
        duration: 0,
        ..make_playback_update()
    });
    assert_eq!(
        app.mpris_track,
        PublishedTrack::for_item(MprisItem::Station("r1"), 0, true)
    );
    assert!(!app.mpris_track.can_seek);
}

#[test]
fn a_stale_tick_leaves_the_published_track_alone() {
    // Built for station r1, landing after a switch back to the queue: the
    // record keeps describing the song it last published.
    let mut app = test_app();
    app.mpris_track = song_track();
    let _ = app.handle_playback_state_updated(PlaybackStateUpdate {
        song_id: Some("r1".to_string()),
        station_id: Some("r1".to_string()),
        ..make_playback_update()
    });
    assert_eq!(app.mpris_track, song_track());
}

// ============================================================================
// What the tick's push sends (a channel-backed connection)
// ============================================================================

use crate::services::mpris::{MprisCommand, MprisConnection};

fn connected_app() -> (crate::Nokkvi, std::sync::mpsc::Receiver<MprisCommand>) {
    let mut app = test_app();
    let (conn, rx) = MprisConnection::test_pair();
    app.mpris_connection = Some(conn);
    (app, rx)
}

fn sent(rx: &std::sync::mpsc::Receiver<MprisCommand>) -> Vec<MprisCommand> {
    rx.try_iter().collect()
}

#[test]
fn a_stale_tick_sends_no_metadata() {
    // Built for station r1, landing after a switch back to the queue: its art
    // and title describe the station, so nothing of them may reach D-Bus.
    let (mut app, rx) = connected_app();
    let _ = app.handle_playback_state_updated(PlaybackStateUpdate {
        song_id: Some("r1".to_string()),
        station_id: Some("r1".to_string()),
        ..make_playback_update()
    });
    let commands = sent(&rx);
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c, MprisCommand::SetMetadata { .. })),
        "{commands:?}"
    );
    assert!(
        commands
            .iter()
            .any(|c| matches!(c, MprisCommand::SetPlaybackStatus(_))),
        "status still goes out: {commands:?}"
    );
}

#[test]
fn every_current_tick_sends_the_track_id_and_can_seek() {
    // CanSeek goes out with every current tick (mpris-server drops an
    // unchanged value), so a reconnect or a failed emit heals on the next one.
    let (mut app, rx) = connected_app();
    for _ in 0..2 {
        let _ = app.handle_playback_state_updated(make_playback_update());
        let commands = sent(&rx);
        assert!(
            commands.iter().any(|c| matches!(
                c,
                MprisCommand::SetMetadata { track_id, .. }
                    if *track_id == track_id_path(MprisItem::Song("song_1"))
            )),
            "{commands:?}"
        );
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, MprisCommand::SetCanSeek(true))),
            "{commands:?}"
        );
    }
}

#[test]
fn a_stopped_song_cannot_seek() {
    // A restored queue at login names its current song, but the engine has
    // nothing loaded until play.
    let mut app = test_app();
    let _ = app.handle_playback_state_updated(PlaybackStateUpdate {
        playing: false,
        paused: false,
        ..make_playback_update()
    });
    assert_eq!(
        app.mpris_track.track_id,
        track_id_path(MprisItem::Song("song_1"))
    );
    assert!(!app.mpris_track.can_seek);
}
