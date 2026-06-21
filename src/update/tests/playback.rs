//! Tests for playback transport, modes, volume, and crossfade update handlers.

use crate::{app_message::PlaybackStateUpdate, test_helpers::*};

// ============================================================================
// Mode Flag Handlers (playback.rs)
// ============================================================================

#[test]
fn random_toggled_sets_flag() {
    let mut app = test_app();
    assert!(!app.modes.random);

    let _ = app.handle_random_toggled(true);
    assert!(app.modes.random);

    let _ = app.handle_random_toggled(false);
    assert!(!app.modes.random);
}

#[test]
fn repeat_toggled_sets_both_flags() {
    let mut app = test_app();
    assert!(!app.modes.repeat);
    assert!(!app.modes.repeat_queue);

    let _ = app.handle_repeat_toggled(true, false);
    assert!(app.modes.repeat);
    assert!(!app.modes.repeat_queue);

    let _ = app.handle_repeat_toggled(true, true);
    assert!(app.modes.repeat);
    assert!(app.modes.repeat_queue);

    let _ = app.handle_repeat_toggled(false, false);
    assert!(!app.modes.repeat);
    assert!(!app.modes.repeat_queue);
}

#[test]
fn consume_toggled_sets_flag() {
    let mut app = test_app();
    assert!(!app.modes.consume);

    let _ = app.handle_consume_toggled(true);
    assert!(app.modes.consume);

    let _ = app.handle_consume_toggled(false);
    assert!(!app.modes.consume);
}

// ============================================================================
// Playback State Machine (playback.rs)
// ============================================================================

fn make_playback_update() -> PlaybackStateUpdate {
    PlaybackStateUpdate {
        position: 42,
        duration: 200,
        playing: true,
        paused: false,
        title: "Test Song".to_string(),
        artist: "Test Artist".to_string(),
        album: "Test Album".to_string(),
        art_url: None,
        random: true,
        repeat: false,
        repeat_queue: false,
        consume: false,
        current_index: Some(0),
        current_entry_id: Some(0),
        song_id: Some("song_1".to_string()),
        format_suffix: "flac".to_string(),
        sample_rate: 44100,
        current_stream_bit_perfect: false,
        bitrate: 1411,
        live_icy_metadata: None,
        bpm: None,
    }
}

#[test]
fn playback_state_updated_maps_fields() {
    let mut app = test_app();
    let update = make_playback_update();

    let _ = app.handle_playback_state_updated(update);

    assert_eq!(app.playback.position, 42);
    assert_eq!(app.playback.duration, 200);
    assert!(app.playback.playing);
    assert!(!app.playback.paused);
    assert_eq!(app.playback.title, "Test Song");
    assert_eq!(app.playback.artist, "Test Artist");
    assert_eq!(app.playback.album, "Test Album");
    assert_eq!(app.playback.format_suffix, "flac");
    assert_eq!(app.playback.sample_rate, 44100);
    assert!(app.modes.random);
    assert!(!app.modes.repeat);
}

#[test]
fn playback_state_updated_detects_song_change() {
    let mut app = test_app();
    // Simulate first song playing
    app.scrobble.current_song_id = Some("old_song".to_string());
    app.scrobble.listening_time = 10.0;

    let update = make_playback_update(); // song_id = "song_1" (different)
    let _ = app.handle_playback_state_updated(update);

    // Scrobble state should be reset for new song
    assert_eq!(app.scrobble.current_song_id.as_deref(), Some("song_1"));
    assert_eq!(app.scrobble.listening_time, 0.0);
    assert!(!app.scrobble.submitted);
}

// ----------------------------------------------------------------------------
// Now-playing artwork warm seam (now_playing_artwork_to_warm)
//
// The MiniPlayer thumbnail (and the queue view's now-playing artwork tier) read
// the current track's album art from the large_artwork / album_art LRUs, which
// are warmed only by the slot-list VIEWPORT. When the playing track is filtered
// out of / scrolled away from the view, neither LRU holds its album_id and the
// thumbnail goes gray. `now_playing_artwork_to_warm()` names the album a
// song-change-driven warm must fetch to close that gap, independent of the
// viewport. These tests pin the predicate; the async fetch it gates needs
// `app_service`, so it is out of unit scope (asserted by reading the handler).
// ----------------------------------------------------------------------------

/// Build a throwaway image handle for seeding an artwork LRU. Contents are
/// irrelevant — the seam only checks album_id key membership.
fn blank_artwork_handle() -> iced::widget::image::Handle {
    iced::widget::image::Handle::from_bytes(vec![0u8; 64])
}

#[test]
fn now_playing_warm_needed_when_album_cold_in_both_lrus() {
    // The gray-box repro: playing track's album is in NEITHER LRU.
    let mut app = test_app();
    app.library.queue_songs = vec![make_queue_song("s1", "T", "A", "Alb")];
    app.scrobble.current_song_id = Some("s1".to_string());

    assert_eq!(
        app.now_playing_artwork_to_warm(),
        Some("album_s1".to_string()),
        "a now-playing album cached in neither LRU must be flagged for a warm"
    );
}

#[test]
fn now_playing_warm_skipped_when_large_artwork_cached() {
    let mut app = test_app();
    app.library.queue_songs = vec![make_queue_song("s1", "T", "A", "Alb")];
    app.scrobble.current_song_id = Some("s1".to_string());
    app.artwork
        .large_artwork
        .put("album_s1".to_string(), blank_artwork_handle());

    assert!(
        app.now_playing_artwork_to_warm().is_none(),
        "already-cached large art means no redundant warm"
    );
}

#[test]
fn now_playing_warm_skipped_when_only_mini_cached() {
    // CRITICAL discriminator: the mini-player paints the 80px album_art
    // fallback, so an album with only the mini cached is NOT a gray box. A
    // naive large-only gate would wrongly re-fetch here.
    let mut app = test_app();
    app.library.queue_songs = vec![make_queue_song("s1", "T", "A", "Alb")];
    app.scrobble.current_song_id = Some("s1".to_string());
    app.artwork
        .album_art
        .put("album_s1".to_string(), blank_artwork_handle());

    assert!(
        app.now_playing_artwork_to_warm().is_none(),
        "the 80px mini fallback already paints — warming again is wasted work"
    );
}

#[test]
fn now_playing_warm_none_for_radio_playback() {
    use crate::state::{ActivePlayback, RadioPlaybackState};

    let mut app = test_app();
    app.library.queue_songs = vec![make_queue_song("s1", "T", "A", "Alb")];
    app.scrobble.current_song_id = Some("s1".to_string());
    app.active_playback = ActivePlayback::Radio(RadioPlaybackState {
        station: nokkvi_data::types::radio_station::RadioStation {
            id: "r1".into(),
            name: "Test".into(),
            stream_url: "http://example.invalid/stream".into(),
            home_page_url: None,
        },
        icy_artist: None,
        icy_title: None,
        icy_url: None,
    });

    assert!(
        app.now_playing_artwork_to_warm().is_none(),
        "radio streams have no album-keyed art — never warm"
    );
}

#[test]
fn now_playing_warm_none_without_current_song() {
    let app = test_app();
    assert!(
        app.scrobble.current_song_id.is_none(),
        "precondition: no current song"
    );
    assert!(app.now_playing_artwork_to_warm().is_none());
}

#[test]
fn now_playing_warm_none_when_song_absent_from_queue() {
    // Guards against warming (or panicking) when current_song_id is not yet
    // projected into queue_songs.
    let mut app = test_app();
    app.library.queue_songs = vec![make_queue_song("s1", "T", "A", "Alb")];
    app.scrobble.current_song_id = Some("missing".to_string());

    assert!(app.now_playing_artwork_to_warm().is_none());
}

#[test]
fn song_change_resolves_new_cold_album_for_warm() {
    // Integration smoke: the warm seam composes with real post-song-change
    // handler state. After handle_playback_state_updated lands a new song,
    // current_song_id is the LANDED track (reset_for_new_song) and — with that
    // track's album cold in both LRUs — now_playing_artwork_to_warm() names that
    // album, the value the warm block dispatches on. (The async fetch needs
    // app_service, absent in test_app(), so the LRU stays cold; the per-predicate
    // resolution rules are pinned by the now_playing_warm_* unit tests above.)
    let mut app = test_app();
    app.library.queue_songs = vec![make_queue_song("song_1", "T", "A", "Alb")];
    app.scrobble.current_song_id = Some("old_song".to_string());

    let _ = app.handle_playback_state_updated(make_playback_update()); // song_id = "song_1"

    assert_eq!(app.scrobble.current_song_id.as_deref(), Some("song_1"));
    assert_eq!(
        app.now_playing_artwork_to_warm(),
        Some("album_song_1".to_string()),
        "after the song change the seam must name the NEW cold album"
    );
}

#[test]
fn focus_mirror_refreshes_on_same_index_entry_id_swap() {
    let mut app = test_app();
    // Prior queue recorded at index 0 with a now-stale entry_id (e.g. the
    // restored queue's reseeded id). A fresh queue swap (PlayGenre at index 0
    // when the prior queue was also at index 0) keeps the index unchanged but
    // allocates a new entry_id at that row.
    app.last_queue_current_index = Some(0);
    app.last_queue_current_entry_id = Some(99);
    app.scrobble.current_song_id = Some("song_1".to_string());

    let mut update = make_playback_update();
    update.current_index = Some(0);
    update.current_entry_id = Some(7);
    update.song_id = Some("song_1".to_string());

    let _ = app.handle_playback_state_updated(update);

    // The entry_id mirror that gates the now-playing breathing glow must track
    // the fresh row, not the stale prior value — otherwise the queue view's
    // `is_current` (entry_id AND song_id) fails and the now-playing row never
    // arms its glow + sheen until a real index change.
    assert_eq!(app.last_queue_current_entry_id, Some(7));
}

#[test]
fn playback_state_updated_same_song_no_reset() {
    let mut app = test_app();
    app.scrobble.current_song_id = Some("song_1".to_string());
    app.scrobble.listening_time = 50.0;
    app.scrobble.last_position = 50.0;

    let mut update = make_playback_update();
    update.position = 55;
    update.song_id = Some("song_1".to_string()); // same song
    let _ = app.handle_playback_state_updated(update);

    // Listening time should accumulate, not reset
    assert!(app.scrobble.listening_time > 50.0);
}

#[test]
fn playback_state_tracks_listening_time_forward() {
    let mut app = test_app();
    app.scrobble.current_song_id = Some("song_1".to_string());
    app.scrobble.last_position = 10.0;
    app.scrobble.listening_time = 0.0;

    let mut update = make_playback_update();
    update.position = 15; // 5 second forward delta
    update.song_id = Some("song_1".to_string());
    let _ = app.handle_playback_state_updated(update);

    assert!((app.scrobble.listening_time - 5.0).abs() < 0.1);
    assert_eq!(app.scrobble.last_position, 15.0);
}

#[test]
fn playback_state_ignores_seek_for_listening_time() {
    let mut app = test_app();
    app.scrobble.current_song_id = Some("song_1".to_string());
    app.scrobble.last_position = 10.0;
    app.scrobble.listening_time = 5.0;

    // Big jump = seek, should not count
    let mut update = make_playback_update();
    update.position = 150; // 140 second jump
    update.song_id = Some("song_1".to_string());
    let _ = app.handle_playback_state_updated(update);

    // Listening time should NOT have increased by 140
    assert!(app.scrobble.listening_time < 10.0);
    // Position should still be updated for next delta
    assert_eq!(app.scrobble.last_position, 150.0);
}

// Volume Handlers (playback.rs) — toast-on-change unification
// ============================================================================

#[test]
fn volume_changed_sets_state_and_pushes_toast() {
    let mut app = test_app();
    assert!(app.toast.toasts.is_empty());

    let _ = app.handle_volume_changed(0.42);

    assert!((app.playback.volume - 0.42).abs() < f32::EPSILON);
    let last = app
        .toast
        .toasts
        .back()
        .expect("a volume toast should have been pushed");
    assert_eq!(last.message, "Volume: 42%");
    assert!(last.right_aligned, "volume toast is right-aligned");
}

#[test]
fn sfx_volume_changed_sets_state_and_pushes_toast() {
    let mut app = test_app();
    assert!(app.toast.toasts.is_empty());

    let _ = app.handle_sfx_volume_changed(0.7);

    assert!((app.sfx.volume - 0.7).abs() < f32::EPSILON);
    let last = app
        .toast
        .toasts
        .back()
        .expect("an sfx volume toast should have been pushed");
    assert_eq!(last.message, "SFX Volume: 70%");
    assert!(last.right_aligned, "sfx volume toast is right-aligned");
}

#[test]
fn volume_committed_sets_state_and_pushes_toast() {
    let mut app = test_app();
    assert!(app.toast.toasts.is_empty());

    let _ = app.handle_volume_committed(0.42);

    assert!((app.playback.volume - 0.42).abs() < f32::EPSILON);
    let last = app
        .toast
        .toasts
        .back()
        .expect("a volume toast should have been pushed");
    assert_eq!(last.message, "Volume: 42%");
    assert!(last.right_aligned, "volume toast is right-aligned");
}

#[test]
fn volume_committed_advances_throttle_inside_blocked_window() {
    // Pin the bug fix: VolumeCommitted must always advance the persist throttle
    // (and dispatch the persist task) even when VolumeChanged would be throttled.
    // Otherwise drag-release values within 500ms of the click-open value
    // never reach disk and are lost on next launch.
    let mut app = test_app();

    // First change opens the throttle window — persists.
    let _ = app.handle_volume_changed(0.30);
    let t1 = app
        .playback
        .volume_persist_throttle
        .expect("throttle should be set after first VolumeChanged");

    // Subsequent VolumeChanged within 500ms is blocked — throttle stays put.
    let _ = app.handle_volume_changed(0.50);
    let t1b = app
        .playback
        .volume_persist_throttle
        .expect("throttle still set");
    assert_eq!(
        t1, t1b,
        "VolumeChanged inside the 500ms window does NOT advance the throttle"
    );

    // VolumeCommitted MUST force-advance the throttle (force-persist semantics).
    let _ = app.handle_volume_committed(0.70);
    let t2 = app
        .playback
        .volume_persist_throttle
        .expect("throttle still set");
    assert!(
        t2 > t1,
        "VolumeCommitted advances throttle even inside the blocked window — \
         this is the slider-drag persistence fix"
    );

    // Final in-memory volume reflects the released value (not the blocked
    // intermediate change).
    assert!((app.playback.volume - 0.70).abs() < f32::EPSILON);
}

#[test]
fn volume_committed_sets_throttle_when_previously_unset() {
    // Even on the first event in a session (throttle = None), VolumeCommitted
    // sets the throttle so subsequent rapid VolumeChanged events get the
    // expected cooldown.
    let mut app = test_app();
    assert!(app.playback.volume_persist_throttle.is_none());

    let _ = app.handle_volume_committed(0.55);

    assert!(
        app.playback.volume_persist_throttle.is_some(),
        "VolumeCommitted seeds the throttle from the unset state"
    );
}

#[test]
fn sfx_volume_changed_clamps_above_one() {
    let mut app = test_app();
    let _ = app.handle_sfx_volume_changed(1.5);
    assert!((app.sfx.volume - 1.0).abs() < f32::EPSILON);
    assert_eq!(
        app.toast.toasts.back().map(|t| t.message.as_str()),
        Some("SFX Volume: 100%")
    );
}

// ============================================================================
// Crossfade Toggle (playback.rs)
// ============================================================================

#[test]
fn crossfade_toggle_flips_state() {
    let mut app = test_app();
    assert!(
        !app.engine.crossfade_enabled,
        "crossfade should default to false"
    );

    let _ = app.handle_toggle_crossfade();
    assert!(
        app.engine.crossfade_enabled,
        "first toggle should enable crossfade"
    );

    let _ = app.handle_toggle_crossfade();
    assert!(
        !app.engine.crossfade_enabled,
        "second toggle should disable crossfade"
    );
}

#[test]
fn crossfade_toggle_from_enabled() {
    let mut app = test_app();
    app.engine.crossfade_enabled = true;

    let _ = app.handle_toggle_crossfade();
    assert!(
        !app.engine.crossfade_enabled,
        "toggle from enabled should disable"
    );
}

#[test]
fn enabling_crossfade_clears_bit_perfect() {
    use nokkvi_data::types::player_settings::BitPerfectMode;
    let mut app = test_app();
    // Mutually exclusive modes: enabling crossfade forces bit-perfect Off.
    app.engine.bit_perfect_mode = BitPerfectMode::Relaxed;
    app.engine.crossfade_enabled = false;

    let _ = app.handle_toggle_crossfade();
    assert!(
        app.engine.crossfade_enabled,
        "toggle should enable crossfade"
    );
    assert_eq!(
        app.engine.bit_perfect_mode,
        BitPerfectMode::Off,
        "enabling crossfade must turn bit-perfect off (exclusive modes)"
    );

    // Disabling crossfade leaves bit-perfect alone (stays Off here).
    let _ = app.handle_toggle_crossfade();
    assert!(
        !app.engine.crossfade_enabled,
        "toggle should disable crossfade"
    );
    assert_eq!(app.engine.bit_perfect_mode, BitPerfectMode::Off);
}

#[test]
fn cycling_bit_perfect_walks_off_strict_relaxed_off() {
    use nokkvi_data::types::player_settings::BitPerfectMode;
    let mut app = test_app();
    app.engine.bit_perfect_mode = BitPerfectMode::Off;

    let _ = app.handle_toggle_bit_perfect();
    assert_eq!(app.engine.bit_perfect_mode, BitPerfectMode::Strict);

    let _ = app.handle_toggle_bit_perfect();
    assert_eq!(app.engine.bit_perfect_mode, BitPerfectMode::Relaxed);

    let _ = app.handle_toggle_bit_perfect();
    assert_eq!(app.engine.bit_perfect_mode, BitPerfectMode::Off);
}

#[test]
fn cycling_bit_perfect_to_non_off_clears_crossfade() {
    use nokkvi_data::types::player_settings::BitPerfectMode;
    let mut app = test_app();
    // Crossfade on; cycling bit-perfect to a non-Off mode must turn it off
    // (exclusive modes). Relaxed then runs its own same-rate crossfade.
    app.engine.crossfade_enabled = true;
    app.engine.bit_perfect_mode = BitPerfectMode::Off;

    // Off -> Strict clears crossfade.
    let _ = app.handle_toggle_bit_perfect();
    assert_eq!(app.engine.bit_perfect_mode, BitPerfectMode::Strict);
    assert!(
        !app.engine.crossfade_enabled,
        "switching to a non-Off bit-perfect mode must turn crossfade off"
    );

    // Strict -> Relaxed: crossfade stays off (already cleared).
    let _ = app.handle_toggle_bit_perfect();
    assert_eq!(app.engine.bit_perfect_mode, BitPerfectMode::Relaxed);
    assert!(!app.engine.crossfade_enabled);

    // Relaxed -> Off: bit-perfect off; crossfade is NOT auto-re-enabled.
    let _ = app.handle_toggle_bit_perfect();
    assert_eq!(
        app.engine.bit_perfect_mode,
        BitPerfectMode::Off,
        "three cycles return to Off"
    );
    assert!(
        !app.engine.crossfade_enabled,
        "cycling back to Off must not auto-re-enable crossfade"
    );
}

#[test]
fn device_rate_probe_applies_when_engaged_and_rate_matches() {
    use crate::state::BitPerfectStatus;
    let mut app = test_app();
    app.playback.bit_perfect_engaged = true;
    app.playback.sample_rate = 96_000;
    app.playback.bit_perfect_probe_generation = 3;
    app.playback.bit_perfect_status = BitPerfectStatus::Off;

    // Probe (current generation) came back with the device clocked at the track
    // rate → Verified, and any holder name is cleared (not meaningful when ok).
    let _ = app.handle_bit_perfect_device_rate_probed(3, 96_000, Some(96_000), Some("Zen".into()));
    assert_eq!(app.playback.bit_perfect_status, BitPerfectStatus::Verified);
    assert_eq!(app.playback.bit_perfect_holder, None);

    // A different device rate → Resampled, carrying the real device rate AND the
    // app holding it (for the inline `· Zen`).
    let _ = app.handle_bit_perfect_device_rate_probed(3, 96_000, Some(48_000), Some("Zen".into()));
    assert_eq!(
        app.playback.bit_perfect_status,
        BitPerfectStatus::Resampled {
            device_rate: 48_000
        }
    );
    assert_eq!(app.playback.bit_perfect_holder.as_deref(), Some("Zen"));
}

#[test]
fn device_rate_probe_ignored_when_not_engaged() {
    use crate::state::BitPerfectStatus;
    let mut app = test_app();
    // Mode no longer engaged (e.g. toggled off since the probe was dispatched).
    app.playback.bit_perfect_engaged = false;
    app.playback.sample_rate = 96_000;
    app.playback.bit_perfect_status = BitPerfectStatus::Off;

    let _ = app.handle_bit_perfect_device_rate_probed(0, 96_000, Some(96_000), None);
    assert_eq!(
        app.playback.bit_perfect_status,
        BitPerfectStatus::Off,
        "a probe result must not resurrect the badge once the mode is off"
    );
}

#[test]
fn device_rate_probe_ignored_when_track_rate_changed() {
    use crate::state::BitPerfectStatus;
    let mut app = test_app();
    app.playback.bit_perfect_engaged = true;
    // The track changed (now 44.1k) after a 96k probe was dispatched.
    app.playback.sample_rate = 44_100;
    app.playback.bit_perfect_status = BitPerfectStatus::Unknown;

    let _ = app.handle_bit_perfect_device_rate_probed(0, 96_000, Some(96_000), None);
    assert_eq!(
        app.playback.bit_perfect_status,
        BitPerfectStatus::Unknown,
        "a stale probe (track rate changed) must not relabel the current track"
    );
}

#[test]
fn unreadable_probe_holds_at_unknown_for_one_grace_probe_then_settles_unverifiable() {
    use crate::state::BitPerfectStatus;
    let mut app = test_app();
    app.playback.bit_perfect_engaged = true;
    app.playback.sample_rate = 96_000;
    app.playback.bit_perfect_probe_generation = 5;
    // Fresh after a transition: hidden, grace streak reset.
    app.playback.bit_perfect_status = BitPerfectStatus::Unknown;
    app.playback.bit_perfect_unverifiable_streak = 0;

    // First probe right after the switch reads nothing (PipeWire is still
    // opening the hardware PCM). Hold at the hidden Unknown — no false flash.
    let _ = app.handle_bit_perfect_device_rate_probed(5, 96_000, None, None);
    assert_eq!(
        app.playback.bit_perfect_status,
        BitPerfectStatus::Unknown,
        "the first unreadable probe after a transition must not flash UNVERIFIED"
    );

    // Still unreadable on the next probe → settle to Unverifiable (genuine
    // Bluetooth / idle).
    let _ = app.handle_bit_perfect_device_rate_probed(5, 96_000, None, None);
    assert_eq!(
        app.playback.bit_perfect_status,
        BitPerfectStatus::Unverifiable,
        "a persistently unreadable sink settles to UNVERIFIED past the grace"
    );
}

#[test]
fn a_readable_probe_resets_the_unverifiable_grace_streak() {
    use crate::state::BitPerfectStatus;
    let mut app = test_app();
    app.playback.bit_perfect_engaged = true;
    app.playback.sample_rate = 96_000;
    app.playback.bit_perfect_probe_generation = 7;
    app.playback.bit_perfect_status = BitPerfectStatus::Unknown;

    // One unreadable probe builds the streak partway.
    let _ = app.handle_bit_perfect_device_rate_probed(7, 96_000, None, None);
    assert_eq!(app.playback.bit_perfect_unverifiable_streak, 1);

    // A good reading verifies AND zeroes the streak.
    let _ = app.handle_bit_perfect_device_rate_probed(7, 96_000, Some(96_000), None);
    assert_eq!(app.playback.bit_perfect_status, BitPerfectStatus::Verified);
    assert_eq!(app.playback.bit_perfect_unverifiable_streak, 0);

    // So a later single unreadable blip falls back to the hidden Unknown for the
    // grace window — it does NOT flip straight to UNVERIFIED.
    let _ = app.handle_bit_perfect_device_rate_probed(7, 96_000, None, None);
    assert_eq!(
        app.playback.bit_perfect_status,
        BitPerfectStatus::Unknown,
        "a single blip after verifying must re-earn the grace, not jump to UNVERIFIED"
    );
}

#[test]
fn settings_load_keeps_crossfade_when_bit_perfect_off() {
    use nokkvi_data::types::player_settings::{BitPerfectMode, LivePlayerSettings};
    let mut app = test_app();
    // The shipped default: crossfade on, bit-perfect off. Loading it leaves both
    // exactly as stored — no reconcile.
    let _ = app.handle_player_settings_loaded(LivePlayerSettings {
        crossfade_enabled: true,
        bit_perfect: BitPerfectMode::Off,
        ..Default::default()
    });
    assert!(
        app.engine.crossfade_enabled,
        "a crossfade-only config must survive a settings load untouched"
    );
    assert_eq!(app.engine.bit_perfect_mode, BitPerfectMode::Off);
}

#[test]
fn settings_load_reconciles_both_on_bit_perfect_wins() {
    use nokkvi_data::types::player_settings::{BitPerfectMode, LivePlayerSettings};
    let mut app = test_app();
    // Crossfade and bit-perfect are mutually exclusive. A hand-edited / pre-
    // migration config with both on reconciles crossfade OFF on load (bit-perfect
    // wins, the non-default opt-in).
    let _ = app.handle_player_settings_loaded(LivePlayerSettings {
        crossfade_enabled: true,
        bit_perfect: BitPerfectMode::Relaxed,
        ..Default::default()
    });
    assert!(
        !app.engine.crossfade_enabled,
        "a both-on config must reconcile crossfade off"
    );
    assert_eq!(
        app.engine.bit_perfect_mode,
        BitPerfectMode::Relaxed,
        "bit-perfect wins the reconciliation"
    );
}

#[test]
fn settings_load_keeps_crossfade_when_bit_perfect_off_after_migration() {
    use nokkvi_data::types::player_settings::{BitPerfectMode, LivePlayerSettings};
    let mut app = test_app();
    // The common case: crossfade on, bit-perfect Off → both survive untouched.
    let _ = app.handle_player_settings_loaded(LivePlayerSettings {
        crossfade_enabled: true,
        bit_perfect: BitPerfectMode::Off,
        ..Default::default()
    });
    assert!(app.engine.crossfade_enabled);
    assert_eq!(app.engine.bit_perfect_mode, BitPerfectMode::Off);
}

#[test]
fn stale_out_of_order_probe_does_not_clobber_a_fresher_result() {
    use crate::state::BitPerfectStatus;
    let mut app = test_app();
    app.playback.bit_perfect_engaged = true;
    app.playback.sample_rate = 96_000;
    // The latest dispatched probe is generation 5; a fresher probe already set
    // Verified.
    app.playback.bit_perfect_probe_generation = 5;
    app.playback.bit_perfect_status = BitPerfectStatus::Verified;

    // An OLDER probe (generation 4, caught the device mid-reclock → 48k) lands
    // late. It must be ignored, not overwrite the fresher Verified.
    let _ = app.handle_bit_perfect_device_rate_probed(4, 96_000, Some(48_000), None);
    assert_eq!(
        app.playback.bit_perfect_status,
        BitPerfectStatus::Verified,
        "an out-of-order stale probe must not clobber the latest result"
    );
}

// ============================================================================
// Settings Sub-List Escape: Search & Escape Behaviour
// ============================================================================
//
// The old description-footer tests assumed a 2-level drill-down +
// settings-panel footer. The persistent-sidebar redesign retires both:
// `description_text` continues to live on for one transitional cycle while
// `view.rs` still renders the old footer, but it's no longer the source of
// truth for any UX. These tests focus on the surviving behaviours: search
// is ignored from inside a sub-list, and Escape on an active search clears
// the search without exiting settings.

#[test]
fn settings_escape_active_search_clears_search() {
    use crate::views::settings::{SettingsAction, SettingsMessage};

    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    // 1. Type a search query — search becomes active.
    let _ = page.update(SettingsMessage::SearchChanged("scrobbl".to_string()), &data);
    assert!(page.search_active, "search should be active after typing");
    assert_eq!(page.search_query, "scrobbl");

    // 2. Escape clears the active search without exiting settings.
    let action = page.update(SettingsMessage::Escape, &data);
    assert!(
        matches!(action, SettingsAction::None),
        "Escape with active search should clear search, not exit"
    );
    assert!(!page.search_active, "search should be deactivated");
    assert!(
        page.search_query.is_empty(),
        "search query should be cleared"
    );
}

#[test]
fn settings_search_from_sub_list_is_noop() {
    use crate::views::settings::{SettingsMessage, SettingsTab};

    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    // 1. Land on Visualizer with its entries cached, then open the
    //    color sub-list by activating the first ColorArray item.
    page.active_category = SettingsTab::Visualizer;
    page.refresh_entries(&data);
    let color_idx = page
        .cached_entries
        .iter()
        .position(|e| {
            matches!(e, crate::views::settings::items::SettingsEntry::Item(item)
                if matches!(item.value, crate::views::settings::items::SettingValue::ColorArray(_)))
        })
        .expect("Visualizer should have a ColorArray entry");
    let total = page.cached_entries.len();
    page.slot_list.set_offset(color_idx, total);
    let _ = page.update(SettingsMessage::EditActivate, &data);
    assert!(page.sub_list.is_some(), "should be in sub-list");

    // 2. Capture current cache size.
    let entries_before = page.cached_entries.len();

    // 3. SearchChanged routes through the sub-list handler while a
    //    sub-list is open — must NOT mutate the parent search query or
    //    rebuild the cached entries.
    let _ = page.update(SettingsMessage::SearchChanged("test".to_string()), &data);

    assert!(page.sub_list.is_some(), "sub-list should remain open");
    assert_eq!(
        page.cached_entries.len(),
        entries_before,
        "entries should not change during sub-list search"
    );
    assert!(
        page.search_query.is_empty(),
        "search_query should not be modified while in sub-list"
    );
}

/// Find the cached-entry index of the row with `key`, set the slot-list center
/// to it, and return that index. Mirrors the focus-then-activate flow.
#[cfg(test)]
fn focus_settings_key(page: &mut crate::views::SettingsPage, key: &str) {
    let idx = page
        .cached_entries
        .iter()
        .position(|e| {
            matches!(e, crate::views::settings::items::SettingsEntry::Item(item)
                if item.key.as_ref() == key)
        })
        .unwrap_or_else(|| panic!("settings entries should contain key {key}"));
    let total = page.cached_entries.len();
    page.slot_list.set_offset(idx, total);
}

#[test]
fn settings_edit_activate_default_playlist_returns_picker_action() {
    use crate::views::settings::{SettingsAction, SettingsMessage, SettingsTab};

    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    page.active_category = SettingsTab::Playback;
    page.refresh_entries(&data);
    focus_settings_key(&mut page, "general.default_playlist_name");

    let action = page.update(SettingsMessage::EditActivate, &data);
    assert!(
        matches!(action, SettingsAction::OpenDefaultPlaylistPicker),
        "Enter on default-playlist row should open the picker, got {action:?}"
    );
}

#[test]
fn settings_edit_activate_local_music_path_returns_text_input() {
    use crate::views::settings::{SettingsAction, SettingsMessage, SettingsTab};

    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    page.active_category = SettingsTab::General;
    page.refresh_entries(&data);
    focus_settings_key(&mut page, "general.local_music_path");

    let action = page.update(SettingsMessage::EditActivate, &data);
    assert!(
        matches!(action, SettingsAction::OpenTextInput { ref key, .. }
            if key == "general.local_music_path"),
        "Enter on local-music-path row should open the text input dialog, got {action:?}"
    );
}

#[test]
fn settings_edit_activate_font_family_opens_font_sub_list() {
    use crate::views::settings::{SettingsAction, SettingsMessage, SettingsTab};

    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    page.active_category = SettingsTab::Interface;
    page.refresh_entries(&data);
    focus_settings_key(&mut page, "font_family");

    let action = page.update(SettingsMessage::EditActivate, &data);
    assert!(
        matches!(action, SettingsAction::None),
        "Font picker activation returns None and mutates state, got {action:?}"
    );
    assert!(
        page.font_sub_list.is_some(),
        "font picker sub-list should open after activating the font_family row"
    );
}

/// Three synthetic theme rows for the theme-picker handler tests. Built
/// in-memory (no disk read) so the apply/search/escape behavior is
/// deterministic regardless of whether the test env has seeded theme files.
fn make_theme_rows() -> Vec<crate::views::settings::ThemeRow> {
    use iced::Color;

    use crate::views::settings::{ThemePreviewColors, ThemeRow};

    let mk = |stem: &str, name: &str, active: bool| ThemeRow {
        stem: stem.to_string(),
        display_name: name.to_string(),
        is_builtin: true,
        is_active: active,
        preview: ThemePreviewColors {
            bg: Color::BLACK,
            fg: Color::WHITE,
            accent: Color::from_rgb(0.2, 0.6, 0.8),
        },
    };

    vec![
        mk("alpha", "Alpha", false),
        mk("bravo", "Bravo", true),
        mk("charlie", "Charlie", false),
    ]
}

/// Open the theme picker on `page` with the given rows, centered on `center_idx`.
fn open_theme_picker(
    page: &mut crate::views::SettingsPage,
    rows: Vec<crate::views::settings::ThemeRow>,
    center_idx: usize,
) {
    use crate::{views::settings::ThemeSubListState, widgets::SlotListView};

    let total = rows.len();
    let mut slot_list = SlotListView::new();
    slot_list.set_offset(center_idx, total);
    page.theme_sub_list = Some(ThemeSubListState {
        filtered_rows: rows.clone(),
        all_rows: rows,
        search_query: String::new(),
        slot_list,
        parent_offset: 0,
    });
}

#[test]
fn settings_edit_activate_browse_themes_opens_theme_sub_list() {
    use crate::views::settings::{SettingsAction, SettingsMessage, SettingsTab};

    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    page.active_category = SettingsTab::Theme;
    page.refresh_entries(&data);
    focus_settings_key(&mut page, "__theme_picker");

    let action = page.update(SettingsMessage::EditActivate, &data);
    assert!(
        matches!(action, SettingsAction::None),
        "Theme picker activation returns None and mutates state, got {action:?}"
    );
    assert!(
        page.theme_sub_list.is_some(),
        "theme picker sub-list should open after activating the Browse Themes row"
    );
}

#[test]
fn theme_sub_list_activate_returns_applypreset_for_centered_row() {
    use crate::views::settings::{SettingsAction, SettingsMessage};

    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    // Center on "Bravo" (index 1) — NOT the first row, so a filtered/positional
    // bug would surface the wrong theme.
    open_theme_picker(&mut page, make_theme_rows(), 1);

    let action = page.update(SettingsMessage::EditActivate, &data);
    assert!(
        matches!(
            action,
            SettingsAction::ApplyPreset { ref stem, ref display_name }
                if stem == "bravo" && display_name == "Bravo"
        ),
        "centered row activate should apply that row's theme by stem, got {action:?}"
    );
    assert!(
        page.theme_sub_list.is_none(),
        "activating a theme should close the picker"
    );
}

#[test]
fn theme_sub_list_search_filters_rows() {
    use crate::views::settings::SettingsMessage;

    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    open_theme_picker(&mut page, make_theme_rows(), 0);

    let _ = page.update(
        SettingsMessage::SubListSearchChanged("char".to_string()),
        &data,
    );

    let tsw = page
        .theme_sub_list
        .as_ref()
        .expect("theme picker stays open while searching");
    assert_eq!(tsw.search_query, "char");
    assert_eq!(
        tsw.filtered_rows.len(),
        1,
        "only Charlie matches the query 'char'"
    );
    assert_eq!(tsw.filtered_rows[0].stem, "charlie");
}

#[test]
fn theme_sub_list_escape_closes_picker() {
    use crate::views::settings::{SettingsAction, SettingsMessage};

    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    open_theme_picker(&mut page, make_theme_rows(), 0);

    let action = page.update(SettingsMessage::Escape, &data);
    assert!(matches!(action, SettingsAction::None));
    assert!(
        page.theme_sub_list.is_none(),
        "Escape should close the theme picker"
    );
}

#[test]
fn theme_sub_list_nav_moves_cursor() {
    use crate::views::settings::SettingsMessage;

    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    open_theme_picker(&mut page, make_theme_rows(), 0);

    let offset = |p: &crate::views::SettingsPage| {
        p.theme_sub_list
            .as_ref()
            .expect("picker open")
            .slot_list
            .viewport_offset
    };

    let _ = page.update(SettingsMessage::SlotListDown, &data);
    assert_eq!(offset(&page), 1, "Down moves the picker cursor down one");
    let _ = page.update(SettingsMessage::SlotListDown, &data);
    assert_eq!(offset(&page), 2);
    let _ = page.update(SettingsMessage::SlotListUp, &data);
    assert_eq!(offset(&page), 1, "Up moves it back");
}

#[test]
fn theme_sub_list_click_recenters_then_activates() {
    use crate::views::settings::{SettingsAction, SettingsMessage};

    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    // Start centered on index 0; a non-center click recenters (no apply yet).
    open_theme_picker(&mut page, make_theme_rows(), 0);

    let click = page.update(SettingsMessage::SlotListClickItem(2), &data);
    assert!(
        matches!(click, SettingsAction::None),
        "clicking a non-center row recenters without applying, got {click:?}"
    );
    assert!(
        page.theme_sub_list.is_some(),
        "the picker stays open after a recenter click"
    );

    // A second activate on the now-centered row applies that theme.
    let action = page.update(SettingsMessage::EditActivate, &data);
    assert!(
        matches!(
            action,
            SettingsAction::ApplyPreset { ref stem, .. } if stem == "charlie"
        ),
        "activate after recenter should apply the clicked theme, got {action:?}"
    );
}

#[test]
fn font_sub_list_search_filters_after_rename() {
    use crate::{
        views::settings::{FontSubListState, SettingsMessage},
        widgets::SlotListView,
    };

    // Guards the shared `SubListSearchChanged` rename: the FONT picker's search
    // must still filter after the variant was renamed from `FontSearchChanged`.
    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    let fonts = vec![
        "Iced Default (SansSerif)".to_string(),
        "DejaVu Sans Mono".to_string(),
        "Arial".to_string(),
    ];
    page.font_sub_list = Some(FontSubListState {
        filtered_fonts: fonts.clone(),
        all_fonts: fonts,
        search_query: String::new(),
        slot_list: SlotListView::new(),
        parent_offset: 0,
    });

    let _ = page.update(
        SettingsMessage::SubListSearchChanged("mono".to_string()),
        &data,
    );

    let fsw = page
        .font_sub_list
        .as_ref()
        .expect("font picker stays open while searching");
    assert_eq!(fsw.search_query, "mono");
    assert_eq!(
        fsw.filtered_fonts,
        vec!["DejaVu Sans Mono".to_string()],
        "case-insensitive substring filter keeps only the mono font"
    );
}

#[test]
fn settings_nav_routes_to_theme_picker_not_background() {
    use crate::views::settings::SettingsMessage;

    // Regression: the nav fast path in `handle_settings` must skip when the
    // theme picker is open, or Up/Down scrolls the dimmed background list
    // instead of moving the picker cursor.
    let mut app = test_app();
    let _ = app.handle_switch_view(crate::View::Settings);
    assert!(
        !app.settings_page.cached_entries.is_empty(),
        "entering Settings populates the entry cache (so the fast-path guard's \
         non-empty check is satisfied and only the picker check can gate it)"
    );

    open_theme_picker(&mut app.settings_page, make_theme_rows(), 0);
    let bg_offset_before = app.settings_page.slot_list.viewport_offset;

    let _ = app.handle_settings(SettingsMessage::SlotListDown);

    let tsw = app
        .settings_page
        .theme_sub_list
        .as_ref()
        .expect("picker stays open");
    assert_eq!(
        tsw.slot_list.viewport_offset, 1,
        "nav must move the theme picker cursor"
    );
    assert_eq!(
        app.settings_page.slot_list.viewport_offset, bg_offset_before,
        "the dimmed background settings list must NOT scroll while the picker is open"
    );
}

// ============================================================================
// I22 — song-change fallback uses the finished song's own duration
// ============================================================================

#[test]
fn song_change_scrobbles_short_prev_before_long_next() {
    let mut app = test_app();
    // Previous song "A": 30s long, listened 27s (>= 90%), not yet submitted.
    app.scrobble.current_song_id = Some("A".to_string());
    app.scrobble.current_song_duration = 30;
    app.scrobble.listening_time = 27.0;
    app.scrobble.submitted = false;
    app.scrobble.submission_in_flight = false;
    app.settings.scrobble_threshold = 0.9;
    app.playback.duration = 30;

    // Capture the eligibility decision for A against ITS OWN duration BEFORE
    // the handler overwrites playback.duration with B's 360s. This is the
    // observable contract: A (27/30s) was eligible.
    let a_eligible = app
        .scrobble
        .should_scrobble(app.scrobble.current_song_duration, 0.9);
    assert!(a_eligible, "short previous song A should be eligible");

    let mut update = make_playback_update();
    update.song_id = Some("B".to_string());
    update.duration = 360;
    let _ = app.handle_playback_state_updated(update);

    // The fallback ran and reset for B (current_song_id advanced). Had the
    // fallback used the clobbered playback.duration (360s), A would have been
    // judged against 360*0.9 and dropped; current_song_duration pins it to 30.
    assert_eq!(app.scrobble.current_song_id.as_deref(), Some("B"));
    assert_eq!(app.scrobble.current_song_duration, 360);
}

// ============================================================================
// N13 — small forward seeks credit no listening time
// ============================================================================

#[test]
fn small_forward_seek_does_not_credit_listening_time() {
    let mut app = test_app();
    app.scrobble.current_song_id = Some("song_1".to_string());
    app.scrobble.last_position = 10.0;
    app.scrobble.listening_time = 5.0;

    // 8s forward seek — UNDER the old 10s magnitude window, so the heuristic
    // alone would have credited it.
    let _ = app.handle_seek(18.0);
    assert_eq!(
        app.scrobble.last_position, 18.0,
        "handle_seek must advance last_position to the seek target"
    );

    // Post-seek tick at the new position.
    let mut update = make_playback_update();
    update.position = 18;
    update.song_id = Some("song_1".to_string());
    let _ = app.handle_playback_state_updated(update);

    assert!(
        (app.scrobble.listening_time - 5.0).abs() < 0.1,
        "an 8s forward seek must credit no listening time (stays ~5.0)"
    );
    assert_eq!(app.scrobble.last_position, 18.0);
}

// ============================================================================
// I16 — stale ticks do not revert optimistic mode toggles
// ============================================================================

#[test]
fn stale_tick_does_not_revert_optimistic_random() {
    let mut app = test_app();
    let before = app.modes.random;
    let _ = app.handle_toggle_random();
    assert_eq!(app.modes.random, !before, "optimistic flip");

    // A tick whose snapshot predates the commit carries the OLD value.
    let mut update = make_playback_update();
    update.random = before;
    let _ = app.handle_playback_state_updated(update);
    assert_eq!(
        app.modes.random, !before,
        "stale in-flight tick must not revert the optimistic toggle"
    );

    // Commit lands → gate clears → modes hold.
    let _ = app.handle_random_toggled(!before);
    assert_eq!(app.modes.random, !before);
    assert_eq!(app.pending_mode_commits, 0, "gate released after commit");
}

#[test]
fn stale_tick_does_not_revert_optimistic_consume() {
    let mut app = test_app();
    let before = app.modes.consume;
    let _ = app.handle_toggle_consume();
    assert_eq!(app.modes.consume, !before);

    let mut update = make_playback_update();
    update.consume = before;
    let _ = app.handle_playback_state_updated(update);
    assert_eq!(
        app.modes.consume, !before,
        "stale tick must not revert optimistic consume"
    );

    let _ = app.handle_consume_toggled(!before);
    assert_eq!(app.modes.consume, !before);
}

#[test]
fn stale_tick_does_not_revert_optimistic_repeat() {
    let mut app = test_app();
    // off -> repeat one
    let _ = app.handle_toggle_repeat();
    assert!(app.modes.repeat);
    assert!(!app.modes.repeat_queue);

    // Stale tick reporting repeat off.
    let mut update = make_playback_update();
    update.repeat = false;
    update.repeat_queue = false;
    let _ = app.handle_playback_state_updated(update);
    assert!(
        app.modes.repeat,
        "stale tick must not revert optimistic repeat"
    );

    let _ = app.handle_repeat_toggled(true, false);
    assert!(app.modes.repeat);
    assert_eq!(app.pending_mode_commits, 0);
}

// ============================================================================
// I26 — mode toggles are no-ops during radio playback
// ============================================================================

fn radio_test_app() -> crate::Nokkvi {
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

#[test]
fn radio_active_mode_toggles_are_noops() {
    let mut app = radio_test_app();
    let random = app.modes.random;
    let repeat = app.modes.repeat;
    let repeat_queue = app.modes.repeat_queue;
    let consume = app.modes.consume;

    let _ = app.handle_toggle_random();
    let _ = app.handle_toggle_repeat();
    let _ = app.handle_toggle_consume();

    assert_eq!(app.modes.random, random, "shuffle unchanged during radio");
    assert_eq!(app.modes.repeat, repeat, "repeat unchanged during radio");
    assert_eq!(app.modes.repeat_queue, repeat_queue);
    assert_eq!(app.modes.consume, consume, "consume unchanged during radio");
    // Radio no-ops must NOT leak a pending commit count.
    assert_eq!(
        app.pending_mode_commits, 0,
        "radio no-op must not bump the mode-commit gate"
    );
}

#[test]
fn non_radio_mode_toggles_still_flip() {
    let mut app = test_app();
    let random = app.modes.random;
    let consume = app.modes.consume;

    let _ = app.handle_toggle_random();
    let _ = app.handle_toggle_repeat();
    let _ = app.handle_toggle_consume();

    assert_eq!(app.modes.random, !random, "shuffle flips off radio");
    assert!(app.modes.repeat, "repeat advances off radio");
    assert_eq!(app.modes.consume, !consume, "consume flips off radio");
}

// ============================================================================
// I15 — cold-start toggle_play does not optimistically assert Playing
// ============================================================================

#[test]
fn toggle_play_does_not_show_playing_when_cold_start_cannot_start() {
    let mut app = test_app();
    // Cold start: nothing loaded, but a queue exists so we pass the empty guard.
    app.playback.playing = false;
    app.playback.paused = false;
    app.library.queue_songs = vec![make_queue_song("q1", "T", "A", "Al")];

    let _ = app.handle_toggle_play();

    assert!(
        !app.playback.playing,
        "cold-start toggle must not optimistically assert Playing on a no-op path"
    );
}

#[test]
fn toggle_play_resume_of_paused_track_flips_playing() {
    let mut app = test_app();
    // Paused, loaded track: resume keeps instant optimistic feedback.
    app.playback.playing = true;
    app.playback.paused = true;

    let _ = app.handle_toggle_play();

    assert!(
        app.playback.playing,
        "resuming a paused track keeps the optimistic Playing flip"
    );
    assert!(!app.playback.paused);
}

// ============================================================================
// Previous: rewind-on-previous (opt-in restart vs step-back), Tier A #1
// ============================================================================
//
// `settings.rewind_on_previous` (off by default, mirrors fooyin) gates whether
// Previous restarts the current track once it has played past
// `PREV_RESTART_THRESHOLD_SECS`. The observable hook is `scrobble.last_position`:
// a restart routes through `handle_seek(0.0)`, which advances `last_position`
// to 0.0 synchronously before returning the task; the step-back path calls
// `shell.previous()` and never touches it. So `last_position == 0.0` means
// "restarted" and an untouched sentinel means "stepped back". (`app_service`
// is None in tests, so the async `shell_task` is a no-op — only the
// synchronous decision is observable, which is exactly what we pin.)

#[test]
fn prev_track_restarts_current_when_enabled_and_past_threshold() {
    let mut app = test_app();
    app.settings.rewind_on_previous = true;
    // A track that has played well past the restart threshold.
    app.playback.position = 8;
    // Sentinel distinct from the restart target (0.0).
    app.scrobble.last_position = 99.0;

    let _ = app.handle_prev_track();

    assert_eq!(
        app.scrobble.last_position, 0.0,
        "with the setting on, Previous past the threshold restarts the current track (seek to 0)"
    );
}

#[test]
fn prev_track_setting_off_always_steps_back() {
    let mut app = test_app();
    // Default is OFF (fooyin default): even past the threshold, Previous steps
    // back and must never restart-seek.
    assert!(
        !app.settings.rewind_on_previous,
        "rewind_on_previous must default to off"
    );
    app.playback.position = 8;
    app.scrobble.last_position = 99.0;

    let _ = app.handle_prev_track();

    assert_eq!(
        app.scrobble.last_position, 99.0,
        "with the setting off, Previous steps back regardless of position (no restart-seek)"
    );
}

#[test]
fn prev_track_steps_back_when_within_threshold() {
    let mut app = test_app();
    app.settings.rewind_on_previous = true;
    // Within the first few seconds, Previous steps to the previous track even
    // when the setting is on.
    app.playback.position = 1;
    app.scrobble.last_position = 99.0;

    let _ = app.handle_prev_track();

    assert_eq!(
        app.scrobble.last_position, 99.0,
        "Previous within the threshold steps back and must NOT seek (last_position untouched)"
    );
}

#[test]
fn prev_track_radio_ignores_restart_when_enabled() {
    let mut app = radio_test_app();
    app.settings.rewind_on_previous = true;
    // Even with the setting on and past the threshold, radio Previous cycles
    // stations — never restart-seeks.
    app.playback.position = 8;
    app.scrobble.last_position = 99.0;

    let _ = app.handle_prev_track();

    assert_eq!(
        app.scrobble.last_position, 99.0,
        "radio Previous must cycle stations, not restart-seek, regardless of position"
    );
}
