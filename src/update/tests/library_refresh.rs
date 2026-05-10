//! Tests for library refresh, viewport clamp, and seek-driven artwork load update handlers.

use crate::{View, test_helpers::*};

// Library Refresh Toast Suppression (library_refresh.rs)
// ============================================================================

#[test]
fn library_refreshed_emits_toast_by_default() {
    let mut app = test_app();
    assert!(!app.suppress_library_refresh_toasts);
    assert!(app.toast.toasts.is_empty());

    let _ = app.handle_library_changed(Vec::new(), true);

    assert_eq!(
        app.toast.toasts.len(),
        1,
        "Wildcard refresh should emit one info toast by default"
    );
    let toast = &app.toast.toasts[0];
    assert!(
        toast.message.contains("Library refreshed"),
        "Expected 'Library refreshed' message, got: {}",
        toast.message
    );
    assert_eq!(toast.level, nokkvi_data::types::toast::ToastLevel::Info);
}

#[test]
fn library_refreshed_suppresses_toast_when_flag_set() {
    let mut app = test_app();
    app.suppress_library_refresh_toasts = true;
    assert!(app.toast.toasts.is_empty());

    let _ = app.handle_library_changed(Vec::new(), true);

    assert!(
        app.toast.toasts.is_empty(),
        "No toast should be pushed when suppress_library_refresh_toasts is true"
    );
}

// ============================================================================
// Albums library-refresh: viewport reconciliation (PROMPT 16)
// ============================================================================
//
// Repro: idle on Albums view (sort=RecentlyAdded), SSE refresh fires, slots
// render blank — borders and backgrounds remain but text/artwork are gone.
//
// Root cause: `handle_library_changed` snapshots `viewport_offset` and the
// album at that offset as an anchor, then `handle_albums_loaded` only updates
// `viewport_offset` when the anchor is *found* in the new list. If the new
// list is shorter than the old offset (server pruned, reordered, or the
// anchor album was removed), `viewport_offset` is left pointing past the end
// of the buffer. `get_slot_item_index_with_center` then returns `None` for
// every slot, and `build_slot_list_slots` falls back to `empty_slot()` —
// which is exactly the "border/background remains, text blank" symptom.
//
// Tests target observable state: `viewport_offset` against new buffer length,
// `selected_indices` purge, and the `library.counts.albums` / buffer-length
// agreement after the load completes.

#[test]
fn albums_loaded_clamps_viewport_when_anchor_missing() {
    // Old buffer had 50 albums, viewport was deep at 40. SSE-driven refresh
    // returns 15 albums and the anchor ID isn't present. viewport_offset must
    // land within the new buffer to keep slots rendering real items.
    let mut app = test_app();
    app.current_view = View::Albums;
    let old_albums: Vec<_> = (0..50)
        .map(|i| make_album(&format!("a{i}"), &format!("Album {i}"), "Artist"))
        .collect();
    app.library.albums.set_from_vec(old_albums);
    app.albums_page.common.slot_list.viewport_offset = 40;

    let new_albums: Vec<_> = (0..15)
        .map(|i| make_album(&format!("b{i}"), &format!("New {i}"), "Artist"))
        .collect();
    let _ = app.handle_albums_loaded(Ok(new_albums), 15, true, Some("a40".to_string()));

    assert_eq!(app.library.albums.len(), 15);
    assert!(
        app.albums_page.common.slot_list.viewport_offset < app.library.albums.len(),
        "viewport_offset {} must stay within new buffer length {}",
        app.albums_page.common.slot_list.viewport_offset,
        app.library.albums.len()
    );
}

#[test]
fn albums_loaded_clamps_viewport_when_anchor_id_none() {
    // Background reload with no anchor (pre-existing buffer empty at snapshot
    // time, then offset advanced somehow). viewport_offset still must land
    // within the new buffer.
    let mut app = test_app();
    app.current_view = View::Albums;
    let old_albums: Vec<_> = (0..30)
        .map(|i| make_album(&format!("a{i}"), &format!("Album {i}"), "Artist"))
        .collect();
    app.library.albums.set_from_vec(old_albums);
    app.albums_page.common.slot_list.viewport_offset = 25;

    let new_albums: Vec<_> = (0..10)
        .map(|i| make_album(&format!("b{i}"), &format!("New {i}"), "Artist"))
        .collect();
    let _ = app.handle_albums_loaded(Ok(new_albums), 10, true, None);

    assert!(
        app.albums_page.common.slot_list.viewport_offset < app.library.albums.len(),
        "viewport_offset {} must stay within new buffer length {}",
        app.albums_page.common.slot_list.viewport_offset,
        app.library.albums.len()
    );
}

#[test]
fn albums_loaded_anchor_match_takes_precedence_over_clamp() {
    // When the anchor IS found, anchor wins — viewport jumps to the new
    // index, even though that index is also within bounds. Locks the existing
    // anchor-restore behavior so the clamp fix can't quietly override it.
    let mut app = test_app();
    app.current_view = View::Albums;
    let old_albums: Vec<_> = (0..50)
        .map(|i| make_album(&format!("a{i}"), &format!("Album {i}"), "Artist"))
        .collect();
    app.library.albums.set_from_vec(old_albums);
    app.albums_page.common.slot_list.viewport_offset = 40;

    // New list: a40 sits at index 5 (5 newer albums prepended, recently-added
    // sort behavior).
    let mut new_albums: Vec<_> = (0..5)
        .map(|i| make_album(&format!("new{i}"), &format!("Newest {i}"), "Artist"))
        .collect();
    new_albums.push(make_album("a40", "Album 40", "Artist"));
    new_albums
        .extend((41..50).map(|i| make_album(&format!("a{i}"), &format!("Album {i}"), "Artist")));

    let _ = app.handle_albums_loaded(Ok(new_albums), 15, true, Some("a40".to_string()));

    assert_eq!(
        app.albums_page.common.slot_list.viewport_offset, 5,
        "anchor lookup should jump viewport to the anchor's new index"
    );
}

#[test]
fn albums_loaded_purges_stale_selected_indices() {
    // Selected indices that point past the new buffer must be removed —
    // otherwise the slot list highlight + batch-action paths see phantom
    // selections against items that no longer exist.
    let mut app = test_app();
    app.current_view = View::Albums;
    let old_albums: Vec<_> = (0..50)
        .map(|i| make_album(&format!("a{i}"), &format!("Album {i}"), "Artist"))
        .collect();
    app.library.albums.set_from_vec(old_albums);
    app.albums_page.common.slot_list.viewport_offset = 40;
    app.albums_page
        .common
        .slot_list
        .selected_indices
        .extend([3, 35, 40, 45]);

    let new_albums: Vec<_> = (0..10)
        .map(|i| make_album(&format!("b{i}"), &format!("New {i}"), "Artist"))
        .collect();
    let _ = app.handle_albums_loaded(Ok(new_albums), 10, true, None);

    let stale: Vec<_> = app
        .albums_page
        .common
        .slot_list
        .selected_indices
        .iter()
        .copied()
        .filter(|&i| i >= app.library.albums.len())
        .collect();
    assert!(
        stale.is_empty(),
        "selected_indices should not retain entries past new buffer length, got stale: {stale:?}"
    );
}

#[test]
fn albums_loaded_total_count_matches_buffer_length() {
    // Header total count and buffer length come from the same load — they
    // must agree after `handle_albums_loaded` returns. Locks the assignment
    // ordering so a future reorder doesn't introduce a transient mismatch.
    let mut app = test_app();
    app.current_view = View::Albums;
    let old_albums: Vec<_> = (0..50)
        .map(|i| make_album(&format!("a{i}"), &format!("Album {i}"), "Artist"))
        .collect();
    app.library.albums.set_from_vec(old_albums);
    app.library.counts.albums = 50;

    let new_albums: Vec<_> = (0..30)
        .map(|i| make_album(&format!("b{i}"), &format!("New {i}"), "Artist"))
        .collect();
    let _ = app.handle_albums_loaded(Ok(new_albums), 30, true, None);

    assert_eq!(app.library.albums.len(), 30);
    assert_eq!(app.library.counts.albums, 30);
}

#[test]
fn albums_loaded_background_preserves_viewport_when_safe() {
    // Background refresh with anchor found at the same index must NOT reset
    // viewport to 0 (foreground refresh resets, background preserves).
    // Regression guard around the existing `if !background` branch in
    // `handle_albums_loaded`.
    let mut app = test_app();
    app.current_view = View::Albums;
    let albums: Vec<_> = (0..50)
        .map(|i| make_album(&format!("a{i}"), &format!("Album {i}"), "Artist"))
        .collect();
    app.library.albums.set_from_vec(albums.clone());
    app.albums_page.common.slot_list.viewport_offset = 12;

    let _ = app.handle_albums_loaded(Ok(albums), 50, true, Some("a12".to_string()));

    assert_eq!(
        app.albums_page.common.slot_list.viewport_offset, 12,
        "background refresh with stable anchor should preserve viewport_offset"
    );
}

// ============================================================================
// Scrollbar Seek → Large Artwork Loading (regression: missing large artwork
// after rapid scroll in albums view)
// ============================================================================
//
// User-reported bug: after scrolling rapidly via the scrollbar in the Albums
// view and then stopping, the large artwork column sometimes stays blank. The
// mini thumbnails for visible slots load fine, and stepping one slot up/down
// with the keyboard then back fixes it. Eventually an SSE-driven artwork
// refresh races in and populates the missing artwork ("Updated artwork for 1
// album" toast).
//
// Architecture intent: SlotListScrollSeek is a hot-path event that should NOT
// trigger a fetch. After 150ms idle, `seek_settled_timer` fires
// `SlotListMessage::SeekSettled`, which synthesises a `SlotListSetOffset` for
// the target view; that path is supposed to dispatch LoadLargeArtwork for the
// centred album.
//
// The `LoadLargeArtwork` action is also dispatched by the existing nav paths,
// so its handler is what eventually puts the album into `loading_large_artwork`
// and kicks off the fetch — which means a synchronous side-effect on
// `loading_large_artwork` is the right TDD signal that the chain is wired
// correctly without depending on the async iced runtime.

#[test]
fn albums_scroll_seek_does_not_load_artwork_immediately() {
    // Hot-path scroll events should only update viewport state. Artwork loading
    // is deferred to the seek_settled debounce timer.
    let mut app = test_app();
    app.current_view = View::Albums;
    let albums: Vec<_> = (0..50)
        .map(|i| make_album(&format!("a{i}"), &format!("Album {i}"), "Artist"))
        .collect();
    app.library.albums.set_from_vec(albums);

    let _ = app.handle_albums(crate::views::AlbumsMessage::SlotList(
        crate::widgets::SlotListPageMessage::ScrollSeek(25),
    ));

    assert_eq!(app.albums_page.common.slot_list.viewport_offset, 25);
    assert_eq!(
        app.artwork.loading_large_artwork, None,
        "scroll seek alone should not start a fetch"
    );
}

#[test]
fn albums_scroll_seek_bumps_scroll_generation_id() {
    // The seek_settled timer is gated by scroll_generation_id; each scroll
    // event must bump it so stale timers from earlier seeks are skipped.
    let mut app = test_app();
    app.current_view = View::Albums;
    let albums: Vec<_> = (0..20)
        .map(|i| make_album(&format!("a{i}"), &format!("Album {i}"), "Artist"))
        .collect();
    app.library.albums.set_from_vec(albums);

    let initial = app.albums_page.common.slot_list.scroll_generation_id;
    let _ = app.handle_albums(crate::views::AlbumsMessage::SlotList(
        crate::widgets::SlotListPageMessage::ScrollSeek(5),
    ));

    assert!(
        app.albums_page.common.slot_list.scroll_generation_id > initial,
        "scroll seek should bump scroll_generation_id"
    );
}

#[test]
fn albums_seek_settled_dispatches_load_large_artwork_for_centered_album() {
    // The bug: this is the chain that fails to fire the artwork load after a
    // rapid-scroll-then-stop in the Albums view. Synthesising the SeekSettled
    // message reproduces what the 150ms debounce timer does in production.
    use crate::app_message::SlotListMessage;

    let mut app = test_app();
    app.current_view = View::Albums;
    let albums: Vec<_> = (0..50)
        .map(|i| make_album(&format!("a{i}"), &format!("Album {i}"), "Artist"))
        .collect();
    app.library.albums.set_from_vec(albums);

    let _ = app.handle_albums(crate::views::AlbumsMessage::SlotList(
        crate::widgets::SlotListPageMessage::ScrollSeek(25),
    ));
    let gen_id = app.albums_page.common.slot_list.scroll_generation_id;

    let _ = app.handle_slot_list_message(SlotListMessage::SeekSettled(View::Albums, gen_id));

    assert_eq!(
        app.artwork.loading_large_artwork.as_deref(),
        Some("a25"),
        "seek_settled should trigger LoadLargeArtwork for the centered album"
    );
}

#[test]
fn songs_seek_settled_dispatches_load_large_artwork_for_centered_song_album() {
    // Songs view is reported to never have the bug — keep it green as a
    // regression sentinel so the fix in albums doesn't break the working path.
    use crate::app_message::SlotListMessage;

    let mut app = test_app();
    app.current_view = View::Songs;
    app.library.songs.set_from_vec(
        (0..50)
            .map(|i| make_song(&format!("s{i}"), &format!("Song {i}"), "Artist"))
            .collect(),
    );

    let _ = app.handle_songs(crate::views::SongsMessage::SlotList(
        crate::widgets::SlotListPageMessage::ScrollSeek(25),
    ));
    let gen_id = app.songs_page.common.slot_list.scroll_generation_id;

    let _ = app.handle_slot_list_message(SlotListMessage::SeekSettled(View::Songs, gen_id));

    // make_song defaults album_id to "album_{id}", so song s25 → album_s25.
    assert_eq!(
        app.artwork.loading_large_artwork.as_deref(),
        Some("album_s25"),
        "seek_settled should trigger LoadLargeArtwork for the centered song's album"
    );
}

#[test]
fn albums_seek_settled_skipped_when_generation_id_is_stale() {
    // Sanity check: if a newer scroll has bumped gen_id, a stale timer's gen_id
    // is rejected and no artwork load happens. Verifies the guard keeps working
    // alongside the fix.
    use crate::app_message::SlotListMessage;

    let mut app = test_app();
    app.current_view = View::Albums;
    let albums: Vec<_> = (0..20)
        .map(|i| make_album(&format!("a{i}"), &format!("Album {i}"), "Artist"))
        .collect();
    app.library.albums.set_from_vec(albums);

    let _ = app.handle_albums(crate::views::AlbumsMessage::SlotList(
        crate::widgets::SlotListPageMessage::ScrollSeek(5),
    ));
    let stale_gen = app.albums_page.common.slot_list.scroll_generation_id;
    // Subsequent scroll bumps gen_id, leaving stale_gen behind.
    let _ = app.handle_albums(crate::views::AlbumsMessage::SlotList(
        crate::widgets::SlotListPageMessage::ScrollSeek(10),
    ));

    let _ = app.handle_slot_list_message(SlotListMessage::SeekSettled(View::Albums, stale_gen));

    assert_eq!(
        app.artwork.loading_large_artwork, None,
        "stale-generation seek_settled timer must not start a fetch"
    );
}

// ============================================================================
// Artists rating-sort carve-out (Phase 3B regression net)
//
// The Subsonic API does not expose a "by rating" sort, so the artists handler
// sorts client-side after each page load. Phase 3B's `load_paginated`
// consolidation must preserve this behaviour — these tests pin the contract
// of `Nokkvi::artists_rating_sort`.
// ============================================================================

#[test]
fn artists_rating_sort_some_before_none_then_desc_by_value() {
    use nokkvi_data::backend::artists::ArtistUIViewData;

    use crate::Nokkvi;

    fn make(id: &str, rating: Option<u32>) -> ArtistUIViewData {
        ArtistUIViewData {
            id: id.into(),
            name: format!("Artist {id}"),
            album_count: 0,
            song_count: 0,
            is_starred: false,
            image_url: None,
            artwork_url: None,
            rating,
            play_count: None,
            play_date: None,
            size: None,
            mbz_artist_id: None,
            biography: None,
            external_url: None,
            searchable_lower: String::new(),
        }
    }

    let mut artists = vec![
        make("none1", None),
        make("low", Some(1)),
        make("none2", None),
        make("high", Some(5)),
        make("mid", Some(3)),
    ];
    Nokkvi::artists_rating_sort(&mut artists);

    let order: Vec<&str> = artists.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(
        order,
        vec!["high", "mid", "low", "none1", "none2"],
        "rated artists come first sorted desc by value; unrated artists tail in original order"
    );
}

#[test]
fn artists_rating_sort_empty_is_noop() {
    use nokkvi_data::backend::artists::ArtistUIViewData;

    use crate::Nokkvi;

    let mut artists: Vec<ArtistUIViewData> = vec![];
    Nokkvi::artists_rating_sort(&mut artists);
    assert!(artists.is_empty());
}

// ============================================================================
// System Tray (services/tray.rs + update/tray.rs)
// ============================================================================

#[test]
fn tray_settings_default_off() {
    let app = test_app();
    assert!(!app.show_tray_icon);
    assert!(!app.close_to_tray);
    assert!(!app.tray_window_hidden);
    assert!(app.tray_connection.is_none());
    assert!(app.main_window_id.is_none());
}

#[test]
fn window_opened_replaces_main_window_id() {
    let mut app = test_app();
    let id1 = iced::window::Id::unique();
    let id2 = iced::window::Id::unique();

    let _ = app.handle_window_opened(id1);
    assert_eq!(app.main_window_id, Some(id1));
    assert!(!app.tray_window_hidden);

    // Daemon mode: close-to-tray destroys the surface; tray Activate opens
    // a fresh window with a different id. handle_window_opened must adopt
    // the new id (the old one was destroyed) and mark the app as visible.
    app.tray_window_hidden = true;
    let _ = app.handle_window_opened(id2);
    assert_eq!(app.main_window_id, Some(id2));
    assert!(!app.tray_window_hidden);
}

#[test]
fn window_close_requested_with_close_to_tray_off_does_not_hide() {
    let mut app = test_app();
    app.show_tray_icon = true;
    app.close_to_tray = false;
    let id = iced::window::Id::unique();

    let _ = app.handle_window_close_requested(id);

    assert!(
        !app.tray_window_hidden,
        "close_to_tray off → window must not be marked hidden (X should quit)"
    );
}

#[test]
fn window_close_requested_with_close_to_tray_on_destroys_window() {
    let mut app = test_app();
    app.show_tray_icon = true;
    app.close_to_tray = true;
    app.main_window_id = Some(iced::window::Id::unique());
    let id = iced::window::Id::unique();

    let _ = app.handle_window_close_requested(id);

    assert!(app.tray_window_hidden);
    // The window is being destroyed — its id is no longer addressable.
    // Cleared so the next tray Activate goes through the "open" branch.
    assert_eq!(app.main_window_id, None);
}

#[test]
fn tray_activate_toggles_window_hidden_flag() {
    use crate::services::tray::TrayEvent;

    let mut app = test_app();
    app.main_window_id = Some(iced::window::Id::unique());
    assert!(!app.tray_window_hidden);

    // First Activate: visible → hidden. Closes the window, clears the id.
    let _ = app.handle_tray(TrayEvent::Activate);
    assert!(
        app.tray_window_hidden,
        "first Activate hides (closes window)"
    );
    assert_eq!(
        app.main_window_id, None,
        "closed window's id is not re-usable"
    );

    // Second Activate: hidden → visible. Dispatches window::open; the new
    // id arrives via WindowOpened later. We flip the flag synchronously
    // so a third rapid Activate reads the right intent.
    let _ = app.handle_tray(TrayEvent::Activate);
    assert!(
        !app.tray_window_hidden,
        "second Activate shows (opens new window)"
    );
}

#[test]
fn tray_activate_without_window_id_is_noop() {
    use crate::services::tray::TrayEvent;

    let mut app = test_app();
    assert!(app.main_window_id.is_none());

    let _ = app.handle_tray(TrayEvent::Activate);
    assert!(
        !app.tray_window_hidden,
        "Activate before window id captured leaves state unchanged"
    );
}

// ============================================================================
