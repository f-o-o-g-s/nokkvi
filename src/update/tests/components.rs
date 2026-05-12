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
