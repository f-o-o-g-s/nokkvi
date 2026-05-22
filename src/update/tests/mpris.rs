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
//! These tests assert the pure mapping fn — no `Nokkvi` state needed.

use mpris_server::LoopStatus;
use nokkvi_data::types::queue::RepeatMode;

use crate::update::mpris::loop_status_to_repeat_mode;

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
