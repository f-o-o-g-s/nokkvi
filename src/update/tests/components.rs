//! Tests for common view-action component update handlers.

use crate::{View, state::ActivePlaylistContext, test_helpers::*};

fn make_playlist_ctx() -> ActivePlaylistContext {
    ActivePlaylistContext {
        id: "pl_42".to_string(),
        name: "Sunday Set".to_string(),
        comment: "weekend rotation".to_string(),
    }
}

// View Action Handlers (components.rs)
// ============================================================================

#[test]
fn handle_common_view_action_refresh_returns_task() {
    let app = test_app();

    let persist_fn = |_s, _m, _a| async { Ok(()) };

    let task = app.handle_common_view_action(
        crate::views::CommonViewAction::RefreshViewData,
        crate::app_message::Message::LoadAlbums,
        "albums",
        crate::widgets::view_header::SortMode::Name,
        true,
        persist_fn,
    );

    assert!(task.is_some(), "RefreshViewData should return a task");
}

#[test]
fn handle_common_view_action_navigate_and_search_returns_task() {
    let app = test_app();
    let persist_fn = |_s, _m, _a| async { Ok(()) };

    let task = app.handle_common_view_action(
        crate::views::CommonViewAction::NavigateAndFilter(
            View::Artists,
            nokkvi_data::types::filter::LibraryFilter::ArtistId {
                id: "Beatles".to_string(),
                name: "Beatles".to_string(),
            },
        ),
        crate::app_message::Message::LoadAlbums,
        "albums",
        crate::widgets::view_header::SortMode::Name,
        true,
        persist_fn,
    );

    assert!(
        task.is_some(),
        "NavigateAndFilter should be handled by common action handler"
    );
}

// ============================================================================
// guard_play_action / enter_new_playback_context
// ============================================================================
//
// Decomposed from a single helper after the 2026-05-12 regression where
// `QueueAction::PlaySong` cleared the loaded-playlist header. The guard now
// only handles universal checks (edit-mode block, radio→queue transition);
// `enter_new_playback_context` carries the cleanup that must NOT run for
// in-queue plays.

#[test]
fn guard_play_action_preserves_active_playlist_info() {
    // Regression: clicking play on a song in the queue must keep the loaded
    // playlist header visible. The guard alone — which is all `PlaySong`
    // calls — must not touch `active_playlist_info`.
    let mut app = test_app();
    app.active_playlist_info = Some(make_playlist_ctx());

    let blocked = app.guard_play_action();

    assert!(blocked.is_none(), "guard should let the play proceed");
    assert!(
        app.active_playlist_info.is_some(),
        "guard alone must preserve the loaded-playlist header — \
         clearing belongs in enter_new_playback_context"
    );
}

#[test]
fn enter_new_playback_context_clears_active_playlist_info() {
    // Queue-replacing plays (album / artist / playlist / song / batch /
    // roulette) call this helper after the guard to drop the previous
    // playlist context.
    let mut app = test_app();
    app.active_playlist_info = Some(make_playlist_ctx());
    app.library.queue_loading_target = Some(5);

    app.enter_new_playback_context();

    assert!(
        app.active_playlist_info.is_none(),
        "new-context entry must clear the playlist header"
    );
    assert!(
        app.library.queue_loading_target.is_none(),
        "new-context entry must cancel the in-progress queue load target"
    );
}

#[test]
fn guard_play_action_blocks_during_playlist_edit() {
    let mut app = test_app();
    app.active_playlist_info = Some(make_playlist_ctx());
    app.playlist_edit = Some(nokkvi_data::types::playlist_edit::PlaylistEditState::new(
        "pl_42".into(),
        "Sunday Set".into(),
        String::new(),
        false,
        Vec::new(),
    ));

    let blocked = app.guard_play_action();

    assert!(blocked.is_some(), "edit-mode plays must be blocked");
    assert!(
        app.active_playlist_info.is_some(),
        "the blocked guard must not mutate playlist context either"
    );
}

#[test]
fn guard_play_action_transitions_radio_to_queue() {
    use crate::state::{ActivePlayback, RadioPlaybackState};

    let mut app = test_app();
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

    let blocked = app.guard_play_action();

    assert!(blocked.is_none(), "guard should let the play proceed");
    assert!(
        app.active_playback.is_queue(),
        "active radio must transition back to queue mode"
    );
}

// ============================================================================
// redirect_play_to_queue_in_browsing_panel (components.rs)
// ============================================================================

#[test]
fn redirect_play_returns_none_when_browsing_panel_closed() {
    // Single-pane play actions must NOT be redirected — the caller proceeds
    // to its normal "replace queue" play flow.
    let mut app = test_app();
    assert!(app.browsing_panel.is_none());

    let mut add_fired = false;
    let mut insert_fired = false;
    let task = app.redirect_play_to_queue_in_browsing_panel(
        |_app| {
            add_fired = true;
            iced::Task::none()
        },
        |_app, _pos| {
            insert_fired = true;
            iced::Task::none()
        },
    );

    assert!(task.is_none(), "no redirect without browsing panel");
    assert!(!add_fired, "add closure must not fire when closed");
    assert!(!insert_fired, "insert closure must not fire when closed");
}

#[test]
fn redirect_play_invokes_add_when_no_pending_insert() {
    // Browsing panel open, no drag-drop position → append branch.
    let mut app = test_app();
    app.browsing_panel = Some(crate::views::BrowsingPanel::new());
    app.pending_queue_insert_position = None;

    let mut add_fired = false;
    let mut insert_fired = false;
    let task = app.redirect_play_to_queue_in_browsing_panel(
        |_app| {
            add_fired = true;
            iced::Task::none()
        },
        |_app, _pos| {
            insert_fired = true;
            iced::Task::none()
        },
    );

    assert!(task.is_some(), "browsing-panel redirect must return Some");
    assert!(
        add_fired,
        "add closure must run when no insert position pending"
    );
    assert!(!insert_fired, "insert closure must NOT run");
}

// ============================================================================
// find_current_rating (components.rs)
// ============================================================================

#[test]
fn find_current_rating_returns_zero_on_miss() {
    // Optimistic rating updates need the prior value to revert on API
    // failure. When the id isn't in the slice, the helper must return 0
    // (the prior contract before the lookup was lifted from 4 inline sites).
    use crate::Nokkvi;

    let mut song = make_song("s1", "Song 1", "Artist");
    song.rating = Some(4);
    let items = vec![song];

    let rating = Nokkvi::find_current_rating(&items, "missing-id", |s| s.id.as_str(), |s| s.rating);

    assert_eq!(
        rating, 0,
        "miss must default to 0 — the inline `.unwrap_or(0)` contract"
    );
}

#[test]
fn find_current_rating_returns_rating_on_hit() {
    use crate::Nokkvi;

    let mut song = make_song("s1", "Song 1", "Artist");
    song.rating = Some(4);
    let items = vec![song, make_song("s2", "Song 2", "Artist")];

    let rating = Nokkvi::find_current_rating(&items, "s1", |s| s.id.as_str(), |s| s.rating);

    assert_eq!(rating, 4, "hit must return the item's rating");
}

#[test]
fn find_current_rating_unrated_item_returns_zero() {
    // Item is found but its rating is None → still 0 (the `.and_then` →
    // `.unwrap_or(0)` chain in the original).
    use crate::Nokkvi;

    let song = make_song("s1", "Song 1", "Artist"); // rating defaults to None
    let items = vec![song];

    let rating = Nokkvi::find_current_rating(&items, "s1", |s| s.id.as_str(), |s| s.rating);

    assert_eq!(rating, 0, "unrated item must yield 0 — same as miss");
}

// ============================================================================
// play_batch_task (components.rs)
// ============================================================================

#[test]
fn play_batch_task_clears_active_playlist() {
    // play_batch_task replaces the queue → the loaded-playlist header must
    // be cleared so it doesn't outlive the playlist it was named after.
    let mut app = test_app();
    app.active_playlist_info = Some(make_playlist_ctx());

    let payload = nokkvi_data::types::batch::BatchPayload::new().with_item(
        nokkvi_data::types::batch::BatchItem::Album("a1".to_string()),
    );
    let _task = app.play_batch_task(payload);

    assert!(
        app.active_playlist_info.is_none(),
        "play_batch_task must clear active_playlist_info — the queue is being replaced"
    );
}

#[test]
fn redirect_play_invokes_insert_and_consumes_position() {
    // Browsing panel open with a drag-drop position → insert branch, AND
    // the position is consumed via `take()` so the next play sees None.
    let mut app = test_app();
    app.browsing_panel = Some(crate::views::BrowsingPanel::new());
    app.pending_queue_insert_position = Some(3);

    let mut add_fired = false;
    let mut received_pos: Option<usize> = None;
    let task = app.redirect_play_to_queue_in_browsing_panel(
        |_app| {
            add_fired = true;
            iced::Task::none()
        },
        |_app, pos| {
            received_pos = Some(pos);
            iced::Task::none()
        },
    );

    assert!(task.is_some(), "browsing-panel redirect must return Some");
    assert!(!add_fired, "add closure must NOT run");
    assert_eq!(
        received_pos,
        Some(3),
        "insert closure receives the position"
    );
    assert!(
        app.pending_queue_insert_position.is_none(),
        "pending_queue_insert_position must be consumed via take()"
    );
}
