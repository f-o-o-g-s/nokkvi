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
