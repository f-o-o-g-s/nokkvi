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
        .slot_list_message(SlotListPageMessage::ActivateCenter(false));
    assert!(matches!(
        msg,
        Message::Songs(views::SongsMessage::SlotList(
            SlotListPageMessage::ActivateCenter(false)
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
        .slot_list_message(SlotListPageMessage::ActivateCenter(false));
    assert!(matches!(
        msg,
        Message::Queue(views::QueueMessage::SlotList(
            SlotListPageMessage::ActivateCenter(false)
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
// Search clears a stale selection (TODO #6)
// ============================================================================
//
// A click leaves `selected_indices = {k}`. A search re-bases every index on
// the filtered list, and a non-empty selection turns off the center-slot ring,
// so the stale `k` left the filtered rows with no highlight at all.

fn radio_station(id: &str, name: &str) -> nokkvi_data::types::radio_station::RadioStation {
    nokkvi_data::types::radio_station::RadioStation {
        id: id.into(),
        name: name.into(),
        stream_url: format!("http://example.invalid/{id}"),
        home_page_url: None,
        cover_art: None,
    }
}

#[test]
fn radios_search_clears_selection_left_by_a_click() {
    use crate::views::RadiosMessage;
    let mut app = test_app();
    app.current_view = View::Radios;
    app.library.radio_stations = vec![
        radio_station("r1", "BBC Radio"),
        radio_station("r2", "FIP"),
        radio_station("r3", "KEXP"),
        radio_station("r4", "SomaFM"),
    ];

    let _ = app.handle_radios(RadiosMessage::SlotList(SlotListPageMessage::SetOffset(
        2,
        iced::keyboard::Modifiers::empty(),
    )));
    assert!(
        app.radios_page
            .common
            .slot_list
            .selected_indices
            .contains(&2)
    );

    let _ = app.handle_radios(RadiosMessage::SlotList(
        SlotListPageMessage::SearchQueryChanged("soma".to_string()),
    ));

    let sl = &app.radios_page.common.slot_list;
    assert!(
        sl.selected_indices.is_empty(),
        "stale click selection suppresses the ring on the one result"
    );
    assert_eq!(sl.anchor_index, None);
    assert_eq!(sl.selected_offset, None);
}

#[test]
fn albums_search_clears_selection_left_by_a_click() {
    use crate::views::AlbumsMessage;
    let mut app = test_app();
    app.current_view = View::Albums;
    let albums = (0..6)
        .map(|i| make_album(&format!("a{i}"), &format!("Album {i}"), "Artist"))
        .collect();
    app.library.albums.set_first_page(albums, 6);

    let _ = app.handle_albums(AlbumsMessage::SlotList(SlotListPageMessage::SetOffset(
        3,
        iced::keyboard::Modifiers::empty(),
    )));
    assert!(
        app.albums_page
            .common
            .slot_list
            .selected_indices
            .contains(&3)
    );

    // The reload the search dispatches is a Task (not run here); the
    // selection must already be gone before it lands.
    let _ = app.handle_albums(AlbumsMessage::SearchQueryChanged("album".to_string()));

    let sl = &app.albums_page.common.slot_list;
    assert!(sl.selected_indices.is_empty());
    assert_eq!(sl.anchor_index, None);
    assert_eq!(sl.selected_offset, None);
}

// ============================================================================
// Stale selection: a reorder / buffer replace the click outlived
// ============================================================================
//
// Same class as the search case above: `selected_indices` and `selected_offset`
// are ABSOLUTE positions, so any path that reorders or replaces the rows must
// drop all three fields. `set_offset` alone clears only `selected_offset`, and
// `clear_multi_selection` alone keeps it — neither is enough on its own.

#[test]
fn radios_sort_order_flip_clears_selection_left_by_a_click() {
    use crate::views::RadiosMessage;
    let mut app = test_app();
    app.current_view = View::Radios;
    app.library.radio_stations = vec![
        radio_station("r1", "BBC Radio"),
        radio_station("r2", "FIP"),
        radio_station("r3", "KEXP"),
        radio_station("r4", "SomaFM"),
    ];

    let _ = app.handle_radios(RadiosMessage::SlotList(SlotListPageMessage::SetOffset(
        2,
        iced::keyboard::Modifiers::empty(),
    )));
    assert!(
        app.radios_page
            .common
            .slot_list
            .selected_indices
            .contains(&2),
        "precondition: the click left index 2 selected"
    );

    // The real toolbar toggle: flips `sort_ascending`, then `sort_radio_stations`
    // reverses the list in place.
    let _ = app.handle_radios(RadiosMessage::SlotList(
        SlotListPageMessage::ToggleSortOrder,
    ));

    let sl = &app.radios_page.common.slot_list;
    assert!(
        sl.selected_indices.is_empty(),
        "index 2 names a different station after the reverse — the ring would be \
         on the wrong row, or missing"
    );
    assert_eq!(sl.anchor_index, None);
    assert_eq!(sl.selected_offset, None);
}

#[test]
fn albums_foreground_reload_clears_the_click_marker() {
    use crate::views::AlbumsMessage;
    let mut app = test_app();
    app.current_view = View::Albums;
    seed_albums(&mut app, albums_indexed(40));

    let _ = app.handle_albums(AlbumsMessage::SlotList(SlotListPageMessage::SetOffset(
        30,
        iced::keyboard::Modifiers::empty(),
    )));
    assert_eq!(
        app.albums_page.common.slot_list.selected_offset,
        Some(30),
        "precondition: the click left a focus marker at 30"
    );

    // A foreground load — what a sort-mode change dispatches. The viewport goes
    // back to 0, so a kept marker at 30 would put the ring off-screen, and
    // `get_effective_center_index` prefers the marker, so Enter / Shift+Q would
    // act on row 30 instead of the centered row.
    let _ = app.handle_albums_loaded(Ok(albums_indexed(40)), 40, false, None);

    let sl = &app.albums_page.common.slot_list;
    assert_eq!(sl.viewport_offset, 0, "a foreground load starts at the top");
    assert_eq!(
        sl.selected_offset, None,
        "the marker names an absolute index into a replaced buffer"
    );
    assert!(sl.selected_indices.is_empty());
    assert_eq!(sl.anchor_index, None);
}

/// The clear above runs inside `apply_viewport_on_load`, which
/// `handle_loaded_with` calls BEFORE `try_resolve_pending_expand` — so a
/// find-and-expand chain still gets its top-pin. Locks that ordering.
#[test]
fn albums_foreground_reload_still_pins_a_pending_find_and_expand() {
    let mut app = test_app();
    app.current_view = View::Albums;
    arm_pending_album(&mut app, "a12");

    let _ = app.handle_albums_loaded(Ok(albums_indexed(40)), 40, false, None);

    let sl = &app.albums_page.common.slot_list;
    assert_eq!(
        sl.selected_offset,
        Some(12),
        "the pending find-and-expand pin lands after the load's selection clear"
    );
    assert!(
        sl.selected_offset_pinned,
        "and it lands as a top-pin, not a click marker"
    );
}
