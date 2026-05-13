//! Tests for slot-list navigation and page-load error update handlers.

use crate::{
    View,
    app_message::Message,
    test_helpers::*,
    views::{self, ViewPage},
    widgets::SlotListPageMessage,
};

// SlotListDown Unfocuses Search (slot_list.rs)
// ============================================================================

#[test]
fn slot_list_down_unfocuses_search_when_focused() {
    let mut app = test_app();
    app.current_view = View::Albums;
    app.albums_page.common.search_input_focused = true;

    let _ = app.handle_slot_list_navigate_down();

    assert!(
        !app.albums_page.common.search_input_focused,
        "search should be unfocused after SlotListDown"
    );
}

#[test]
fn slot_list_down_navigates_when_search_not_focused() {
    let mut app = test_app();
    app.current_view = View::Albums;
    app.albums_page.common.search_input_focused = false;

    // Should NOT unfocus (already unfocused) — returns a Task dispatching SlotListNavigateDown
    let _ = app.handle_slot_list_navigate_down();
    assert!(
        !app.albums_page.common.search_input_focused,
        "search should remain unfocused"
    );
}

#[test]
fn slot_list_down_preserves_settings_search_query() {
    let mut app = test_app();
    app.current_view = View::Settings;
    app.settings_page.search_active = true;
    app.settings_page.search_query = "Scrobbl".to_string();

    let _ = app.handle_slot_list_navigate_down();

    assert!(
        !app.settings_page.search_active,
        "search bar should be dismissed"
    );
    assert_eq!(
        app.settings_page.search_query, "Scrobbl",
        "search query should be preserved so filtered results remain navigable"
    );
}

// ============================================================================
// Loading State Recovery (Layer 1 — stuck Loading... bug fix)
// ============================================================================

#[test]
fn albums_loaded_error_clears_loading() {
    let mut app = test_app();
    app.library.albums.set_loading(true);
    assert!(app.library.albums.is_loading());

    let _ = app.handle_albums_loaded(Err("network error".to_string()), 0, false, None);
    assert!(
        !app.library.albums.is_loading(),
        "loading flag should be cleared on error"
    );
}

#[test]
fn artists_loaded_error_clears_loading() {
    let mut app = test_app();
    app.library.artists.set_loading(true);
    assert!(app.library.artists.is_loading());

    let _ = app.handle_artists_loaded(Err("network error".to_string()), 0, false, None);
    assert!(
        !app.library.artists.is_loading(),
        "loading flag should be cleared on error"
    );
}

#[test]
fn songs_loaded_error_clears_loading() {
    let mut app = test_app();
    app.library.songs.set_loading(true);
    assert!(app.library.songs.is_loading());

    let _ = app.handle_songs_loaded(Err("network error".to_string()), 0, false, None);
    assert!(
        !app.library.songs.is_loading(),
        "loading flag should be cleared on error"
    );
}

#[test]
fn genres_loaded_error_clears_loading() {
    let mut app = test_app();
    app.library.genres.set_loading(true);
    assert!(app.library.genres.is_loading());

    let _ = app.handle_genres_loaded(Err("network error".to_string()), 0);
    assert!(
        !app.library.genres.is_loading(),
        "loading flag should be cleared on error"
    );
}

#[test]
fn playlists_loaded_error_clears_loading() {
    let mut app = test_app();
    app.library.playlists.set_loading(true);
    assert!(app.library.playlists.is_loading());

    let _ = app.handle_playlists_loaded(Err("network error".to_string()), 0);
    assert!(
        !app.library.playlists.is_loading(),
        "loading flag should be cleared on error"
    );
}

// ============================================================================
// ViewPage::slot_list_message — per-view wrapping pins
// ============================================================================
//
// Each test asserts the trait impl wraps `SlotListPageMessage` in the correct
// outer `Message::<View>(<View>Message::SlotList(...))` variant. The wraps
// are one-line impls per view; the tests guard against typo regressions
// (e.g. an impl accidentally wrapping in the wrong outer variant) and pin
// the compile-time-enforced "added new view, must implement slot_list_message"
// contract.

#[test]
fn albums_slot_list_message_wraps_in_albums_variant() {
    let app = test_app();
    let msg = app
        .albums_page
        .slot_list_message(SlotListPageMessage::NavigateUp);
    assert!(matches!(
        msg,
        Message::Albums(views::AlbumsMessage::SlotList(
            SlotListPageMessage::NavigateUp
        ))
    ));
}

#[test]
fn artists_slot_list_message_wraps_in_artists_variant() {
    let app = test_app();
    let msg = app
        .artists_page
        .slot_list_message(SlotListPageMessage::NavigateDown);
    assert!(matches!(
        msg,
        Message::Artists(views::ArtistsMessage::SlotList(
            SlotListPageMessage::NavigateDown
        ))
    ));
}

#[test]
fn songs_slot_list_message_wraps_in_songs_variant() {
    let app = test_app();
    let msg = app
        .songs_page
        .slot_list_message(SlotListPageMessage::ActivateCenter);
    assert!(matches!(
        msg,
        Message::Songs(views::SongsMessage::SlotList(
            SlotListPageMessage::ActivateCenter
        ))
    ));
}

#[test]
fn genres_slot_list_message_wraps_in_genres_variant() {
    let app = test_app();
    let msg = app
        .genres_page
        .slot_list_message(SlotListPageMessage::NavigateUp);
    assert!(matches!(
        msg,
        Message::Genres(views::GenresMessage::SlotList(
            SlotListPageMessage::NavigateUp
        ))
    ));
}

#[test]
fn playlists_slot_list_message_wraps_in_playlists_variant() {
    let app = test_app();
    let msg = app
        .playlists_page
        .slot_list_message(SlotListPageMessage::NavigateDown);
    assert!(matches!(
        msg,
        Message::Playlists(views::PlaylistsMessage::SlotList(
            SlotListPageMessage::NavigateDown
        ))
    ));
}

#[test]
fn queue_slot_list_message_wraps_in_queue_variant() {
    let app = test_app();
    let msg = app
        .queue_page
        .slot_list_message(SlotListPageMessage::ActivateCenter);
    assert!(matches!(
        msg,
        Message::Queue(views::QueueMessage::SlotList(
            SlotListPageMessage::ActivateCenter
        ))
    ));
}

#[test]
fn radios_slot_list_message_wraps_in_radios_variant() {
    let app = test_app();
    let msg = app
        .radios_page
        .slot_list_message(SlotListPageMessage::NavigateUp);
    assert!(matches!(
        msg,
        Message::Radios(views::RadiosMessage::SlotList(
            SlotListPageMessage::NavigateUp
        ))
    ));
}

#[test]
fn similar_slot_list_message_wraps_in_similar_variant() {
    let app = test_app();
    let msg = app
        .similar_page
        .slot_list_message(SlotListPageMessage::NavigateDown);
    assert!(matches!(
        msg,
        Message::Similar(views::SimilarMessage::SlotList(
            SlotListPageMessage::NavigateDown
        ))
    ));
}

// ============================================================================
