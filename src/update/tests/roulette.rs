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
fn roulette_start_arms_state_when_library_has_items() {
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
    assert!(state.target_idx < 4);
    assert!(
        !state.decel_keyframes.is_empty(),
        "decel keyframes must be pre-rolled"
    );
    assert_eq!(
        state.decel_keyframes.last().map(|k| k.offset),
        Some(state.target_idx),
        "decel sequence must terminate on target"
    );
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
    let mut app = test_app();
    app.library.albums.set_from_vec(vec![
        make_album("a1", "One", "X"),
        make_album("a2", "Two", "X"),
        make_album("a3", "Three", "X"),
        make_album("a4", "Four", "X"),
    ]);

    let _ = app.handle_roulette_message(crate::app_message::RouletteMessage::Start(View::Albums));
    let target_first = app.roulette.as_ref().map(|s| s.target_idx);

    let _ = app.handle_roulette_message(crate::app_message::RouletteMessage::Start(View::Albums));
    let target_second = app.roulette.as_ref().map(|s| s.target_idx);

    assert_eq!(target_first, target_second);
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
    app.active_playlist_info = Some(ActivePlaylistContext {
        id: "pl_42".to_string(),
        name: "Sunday Set".to_string(),
        comment: String::new(),
    });

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
    app.active_playlist_info = Some(ActivePlaylistContext {
        id: "pl_42".to_string(),
        name: "Sunday Set".to_string(),
        comment: String::new(),
    });

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
