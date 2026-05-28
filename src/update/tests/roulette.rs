//! Tests for roulette update handlers.

use crate::{View, test_helpers::*};

// Roulette Handler (roulette.rs)
// ============================================================================

#[test]
fn roulette_start_with_no_items_is_noop() {
    // Empty Songs library → no spin, no state.
    let mut app = test_app();
    let _ = app.handle_roulette_message(crate::app_message::RouletteMessage::Start(View::Songs));
    assert!(app.roulette.is_none());
}

#[test]
fn roulette_start_arms_indefinite_cruise() {
    // Start kicks off the cruise phase only — target and decel keyframes
    // are rolled later by Stop. State.decel should be None and the snapshot
    // fields (view, total_items, original_offset, cruise rate) should be
    // populated.
    let mut app = test_app();
    app.library.albums.set_from_vec(vec![
        make_album("a1", "First", "Artist"),
        make_album("a2", "Second", "Artist"),
        make_album("a3", "Third", "Artist"),
        make_album("a4", "Fourth", "Artist"),
    ]);
    app.albums_page.common.slot_list.viewport_offset = 1;

    let _ = app.handle_roulette_message(crate::app_message::RouletteMessage::Start(View::Albums));

    let state = app
        .roulette
        .as_ref()
        .expect("roulette should be armed for a non-trivial album list");
    assert_eq!(state.view, View::Albums);
    assert_eq!(state.total_items, 4);
    assert_eq!(state.original_offset, 1);
    assert!(
        state.decel.is_none(),
        "decel must stay None until the user presses Enter to stop"
    );
    assert!(
        state.cruise_pos_per_sec > 0,
        "cruise rate must be a positive positions-per-second figure"
    );
}

#[test]
fn roulette_stop_without_state_is_noop() {
    let mut app = test_app();
    let _ = app.handle_roulette_message(crate::app_message::RouletteMessage::Stop);
    assert!(app.roulette.is_none());
}

#[test]
fn roulette_stop_arms_decel_with_target_and_keyframes() {
    // Stop commits the spin: rolls a random target, builds the decel walk
    // anchored at `now`, and transitions state.decel from None to Some.
    let mut app = test_app();
    app.library.albums.set_from_vec(vec![
        make_album("a1", "First", "Artist"),
        make_album("a2", "Second", "Artist"),
        make_album("a3", "Third", "Artist"),
        make_album("a4", "Fourth", "Artist"),
    ]);

    let _ = app.handle_roulette_message(crate::app_message::RouletteMessage::Start(View::Albums));
    assert!(
        app.roulette.as_ref().unwrap().decel.is_none(),
        "freshly-started spin must be in cruise (decel = None)"
    );

    let _ = app.handle_roulette_message(crate::app_message::RouletteMessage::Stop);

    let state = app.roulette.as_ref().expect("spin must still be armed");
    let arm = state.decel.as_ref().expect("Stop must arm the decel walk");
    assert!(arm.target_idx < 4, "rolled target must be in range");
    assert!(
        !arm.decel_keyframes.is_empty(),
        "decel keyframes must be pre-rolled on Stop"
    );
    assert_eq!(
        arm.decel_keyframes.last().map(|k| k.offset),
        Some(arm.target_idx),
        "decel sequence must terminate on target"
    );
}

#[test]
fn decel_per_click_advance_stays_small_on_huge_library() {
    // Regression: the original "1 revolution + walk-to-target" decel
    // gave ~800-position-per-click jumps on a 13 k-song library because
    // natural_steps scaled with total_items. The new walk is bounded
    // ([NATURAL_KEYFRAME_COUNT, 3×NATURAL_KEYFRAME_COUNT]) so per-click
    // advance stays at 1–3 positions regardless of library size. With
    // the wheel ratcheting that gently, the user actually sees the
    // deceleration instead of seeing teleportation between clicks.
    let mut app = test_app();
    let albums: Vec<_> = (0..15_000)
        .map(|i| make_album(&format!("a{i}"), &format!("Album {i}"), "Artist"))
        .collect();
    app.library.albums.set_from_vec(albums);

    let _ = app.handle_roulette_message(crate::app_message::RouletteMessage::Start(View::Albums));
    let _ = app.handle_roulette_message(crate::app_message::RouletteMessage::Stop);

    let arm = app
        .roulette
        .as_ref()
        .unwrap()
        .decel
        .as_ref()
        .expect("Stop must arm decel");

    // Inspect each adjacent-keyframe delta — going through total_items
    // because the walk may wrap. Reasonable visual ceiling: ≤ 10 positions
    // per click (well within the ~3 that the current bounds produce, with
    // slack for the pattern tail's overshoot/false-settle hops).
    let total_items = app.roulette.as_ref().unwrap().total_items as i64;
    for w in arm.decel_keyframes.windows(2) {
        let a = w[0].offset as i64;
        let b = w[1].offset as i64;
        let forward = (b - a).rem_euclid(total_items);
        let backward = (a - b).rem_euclid(total_items);
        let step = forward.min(backward);
        assert!(
            step <= 10,
            "decel per-click jump must stay readable on huge libraries — got {step} positions \
             between offset {a} and {b}"
        );
    }
}

#[test]
fn roulette_stop_during_decel_is_noop() {
    // A second Stop press while the decel walk is already underway must
    // not re-roll the target or rebuild the keyframes — the spin is
    // committed.
    let mut app = test_app();
    app.library.albums.set_from_vec(vec![
        make_album("a1", "First", "Artist"),
        make_album("a2", "Second", "Artist"),
        make_album("a3", "Third", "Artist"),
        make_album("a4", "Fourth", "Artist"),
    ]);

    let _ = app.handle_roulette_message(crate::app_message::RouletteMessage::Start(View::Albums));
    let _ = app.handle_roulette_message(crate::app_message::RouletteMessage::Stop);
    let first_target = app
        .roulette
        .as_ref()
        .unwrap()
        .decel
        .as_ref()
        .unwrap()
        .target_idx;
    let first_stop_time = app
        .roulette
        .as_ref()
        .unwrap()
        .decel
        .as_ref()
        .unwrap()
        .stop_time;

    // Stagger to make sure a second Stop would land in a different
    // nanosecond bucket and roll a different target.
    std::thread::sleep(std::time::Duration::from_millis(2));
    let _ = app.handle_roulette_message(crate::app_message::RouletteMessage::Stop);

    let arm = app.roulette.as_ref().unwrap().decel.as_ref().unwrap();
    assert_eq!(arm.target_idx, first_target);
    assert_eq!(arm.stop_time, first_stop_time);
}

#[test]
fn roulette_cancel_clears_state_and_restores_offset() {
    let mut app = test_app();
    app.library.albums.set_from_vec(vec![
        make_album("a1", "One", "X"),
        make_album("a2", "Two", "X"),
        make_album("a3", "Three", "X"),
        make_album("a4", "Four", "X"),
    ]);
    app.albums_page.common.slot_list.viewport_offset = 2;

    let _ = app.handle_roulette_message(crate::app_message::RouletteMessage::Start(View::Albums));
    assert!(app.roulette.is_some());

    // Pretend the spin advanced the viewport mid-flight.
    app.albums_page.common.slot_list.viewport_offset = 17 % 4;

    let _ = app.handle_roulette_message(crate::app_message::RouletteMessage::Cancel);

    assert!(app.roulette.is_none(), "cancel must clear state");
    assert_eq!(
        app.albums_page.common.slot_list.viewport_offset, 2,
        "cancel must restore the original viewport offset"
    );
}

#[test]
fn roulette_start_is_reentrant_safe() {
    // A second Start while a spin is already armed must be a no-op so the
    // user can't double-click their way into a weird mid-spin re-roll.
    // start_time is the witness — a fresh init would bump it past the
    // first call.
    let mut app = test_app();
    app.library.albums.set_from_vec(vec![
        make_album("a1", "One", "X"),
        make_album("a2", "Two", "X"),
        make_album("a3", "Three", "X"),
        make_album("a4", "Four", "X"),
    ]);

    let _ = app.handle_roulette_message(crate::app_message::RouletteMessage::Start(View::Albums));
    let first_start = app.roulette.as_ref().map(|s| s.start_time);

    // Stagger so a re-init would land on a later Instant.
    std::thread::sleep(std::time::Duration::from_millis(2));
    let _ = app.handle_roulette_message(crate::app_message::RouletteMessage::Start(View::Albums));
    let second_start = app.roulette.as_ref().map(|s| s.start_time);

    assert_eq!(first_start, second_start);
}

#[test]
fn roulette_start_on_queue_preserves_active_playlist_info() {
    // Regression: queue-view roulette is an in-queue play — it picks a song
    // already in the queue and advances the playback pointer, so the loaded
    // playlist header must survive. handle_roulette_start used to clear
    // active_playlist_info unconditionally; the settle path's per-view
    // dispatch now handles its own context entry.
    use crate::state::ActivePlaylistContext;

    let mut app = test_app();
    app.library.queue_songs = vec![
        make_queue_song("s1", "One", "Artist", "Album"),
        make_queue_song("s2", "Two", "Artist", "Album"),
        make_queue_song("s3", "Three", "Artist", "Album"),
        make_queue_song("s4", "Four", "Artist", "Album"),
    ];
    app.active_playlist_info = Some(ActivePlaylistContext::minimal(
        "pl_42".to_string(),
        "Sunday Set".to_string(),
        String::new(),
    ));

    let _ = app.handle_roulette_message(crate::app_message::RouletteMessage::Start(View::Queue));

    assert!(
        app.active_playlist_info.is_some(),
        "queue-view roulette must preserve the loaded-playlist header"
    );
}

#[test]
fn roulette_start_on_albums_clears_via_settle_path() {
    // The roulette start itself no longer touches active_playlist_info —
    // each settle dispatch handles its own context entry. Queue-replacing
    // views (Albums here) clear the header when the settle's play action
    // runs, not when the spin begins.
    use crate::state::ActivePlaylistContext;

    let mut app = test_app();
    app.library.albums.set_from_vec(vec![
        make_album("a1", "First", "Artist"),
        make_album("a2", "Second", "Artist"),
        make_album("a3", "Third", "Artist"),
        make_album("a4", "Fourth", "Artist"),
    ]);
    app.active_playlist_info = Some(ActivePlaylistContext::minimal(
        "pl_42".to_string(),
        "Sunday Set".to_string(),
        String::new(),
    ));

    let _ = app.handle_roulette_message(crate::app_message::RouletteMessage::Start(View::Albums));

    assert!(
        app.active_playlist_info.is_some(),
        "roulette start arms the spin; the header is cleared later by the \
         settle dispatch's play handler, not by handle_roulette_start"
    );
}

#[test]
fn roulette_tick_without_state_is_noop() {
    let mut app = test_app();
    let _ = app.handle_roulette_message(crate::app_message::RouletteMessage::Tick(
        std::time::Instant::now(),
    ));
    assert!(app.roulette.is_none());
}

#[test]
fn click_navigate_and_expand_album_keeps_center_only_off_for_top_pin_layout() {
    // Regression guard: a click-driven NavigateAndExpand chain must NOT leak
    // center-only mode (which would suppress FocusAndExpand and put the row
    // at the center slot instead of slot 0). This ensures the two chains
    // share state cleanly.
    let mut app = test_app();
    app.pending_expand_center_only = true; // simulate a stale flag

    let _ = app.handle_navigate_and_expand_album("a1".to_string());

    assert!(
        !app.pending_expand_center_only,
        "starting a click-driven find chain must reset center_only — \
         otherwise the click would get the Shift+C layout"
    );
}
