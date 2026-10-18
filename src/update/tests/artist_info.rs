//! The Artists view asks the server for the centred artist's info
//! (`getArtistInfo2`) once that artist has stayed centred, once per artist,
//! and keeps each answer under its own id.

use nokkvi_data::types::{artist::ArtistInfo, error::NokkviError};

use crate::{
    Nokkvi,
    app_message::{ArtistsLoaderMessage, Message},
    test_helpers::*,
    views::ArtistsMessage,
    widgets::SlotListPageMessage,
};

fn app_with_artists(count: usize) -> Nokkvi {
    let mut app = test_app();
    app.library.artists.set_from_vec(
        (0..count)
            .map(|i| make_artist(&format!("a{i}"), &format!("Artist {i}")))
            .collect(),
    );
    app
}

fn slot(app: &mut Nokkvi, msg: SlotListPageMessage) {
    let _ = app.update(Message::Artists(ArtistsMessage::SlotList(msg)));
}

/// Move the centre one row, returning the artist now centred.
fn navigate(app: &mut Nokkvi, msg: SlotListPageMessage) -> String {
    slot(app, msg);
    app.centred_artist_id().expect("an artist is centred")
}

/// The settle timer for `id` fires.
fn settle(app: &mut Nokkvi, id: &str) {
    let _ = app.update(Message::ArtistsLoader(ArtistsLoaderMessage::InfoSettled(
        id.to_string(),
    )));
}

fn land(app: &mut Nokkvi, generation: u64, id: &str, result: Result<ArtistInfo, String>) {
    let _ = app.update(Message::ArtistsLoader(ArtistsLoaderMessage::InfoLoaded {
        generation,
        artist_id: id.to_string(),
        result,
    }));
}

fn bio(text: &str) -> ArtistInfo {
    ArtistInfo {
        biography: Some(text.to_string()),
        external_url: Some("https://www.last.fm/music/x".to_string()),
        ..ArtistInfo::default()
    }
}

#[test]
fn an_artist_that_stays_centred_is_asked_about_once() {
    let mut app = app_with_artists(20);
    let generation = app.artist_info.session_generation();

    let first = navigate(&mut app, SlotListPageMessage::NavigateDown);
    assert!(
        !app.artist_info.is_pending(&first),
        "nothing is asked before the centre settles"
    );
    settle(&mut app, &first);
    assert!(app.artist_info.is_pending(&first), "asked once settled");

    land(&mut app, generation, &first, Ok(bio("first bio")));
    assert!(!app.artist_info.is_pending(&first));
    assert_eq!(
        app.artist_info
            .cache
            .snapshot
            .get(&first)
            .and_then(|i| i.biography.as_deref()),
        Some("first bio")
    );

    // Away and back: the cached artist is not asked about again.
    navigate(&mut app, SlotListPageMessage::NavigateDown);
    let back = navigate(&mut app, SlotListPageMessage::NavigateUp);
    assert_eq!(back, first);
    settle(&mut app, &first);
    assert!(
        !app.artist_info.is_pending(&first),
        "a cached artist is not asked about again"
    );
}

/// A held arrow key scrolls past rows: only the artist it stops on is asked
/// about, since a first ask makes the server run its agents.
#[test]
fn scrolling_past_artists_asks_only_about_the_one_it_stops_on() {
    let mut app = app_with_artists(20);
    let passed: Vec<String> = (0..4)
        .map(|_| navigate(&mut app, SlotListPageMessage::NavigateDown))
        .collect();
    let stopped_on = passed.last().expect("moved").clone();
    for id in &passed {
        settle(&mut app, id);
    }
    for id in &passed[..passed.len() - 1] {
        assert!(!app.artist_info.is_pending(id), "{id} was scrolled past");
    }
    assert!(app.artist_info.is_pending(&stopped_on));
}

/// A click on a row off the viewport centre highlights it in place (stable
/// viewport); the panel shows that artist, so that is the one asked about.
#[test]
fn a_clicked_row_off_centre_is_the_artist_asked_about() {
    let mut app = app_with_artists(20);
    slot(&mut app, SlotListPageMessage::NavigateDown);
    let viewport_centre = app
        .artists_page
        .common
        .get_center_item_index(app.library.artists.len())
        .expect("a viewport centre");
    let clicked = viewport_centre + 2;
    slot(
        &mut app,
        SlotListPageMessage::SetOffset(clicked, iced::keyboard::Modifiers::default()),
    );
    let id = format!("a{clicked}");
    assert_eq!(app.centred_artist_id().as_deref(), Some(id.as_str()));
    settle(&mut app, &id);
    assert!(app.artist_info.is_pending(&id));
}

/// With an artist expanded, a centred album row belongs to that artist, not
/// to whichever artist sits at the same position in the plain list.
#[test]
fn an_album_row_of_an_expanded_artist_resolves_to_that_artist() {
    let mut app = app_with_artists(20);
    expand_artists_with(
        &mut app,
        "a0",
        (0..6)
            .map(|i| make_album(&format!("al{i}"), &format!("Album {i}"), "Artist 0"))
            .collect(),
    );
    for _ in 0..3 {
        slot(&mut app, SlotListPageMessage::NavigateDown);
    }
    assert_eq!(app.centred_artist_id().as_deref(), Some("a0"));
}

/// A result that lands after the user moved on is cached under the artist it
/// was asked for, never shown for the artist centred now.
#[test]
fn a_result_for_an_artist_no_longer_centred_stays_with_its_own_artist() {
    let mut app = app_with_artists(20);
    let generation = app.artist_info.session_generation();
    let first = navigate(&mut app, SlotListPageMessage::NavigateDown);
    settle(&mut app, &first);
    let now = navigate(&mut app, SlotListPageMessage::NavigateDown);
    settle(&mut app, &now);
    assert_ne!(first, now);

    land(&mut app, generation, &first, Ok(bio("first bio")));
    assert!(app.artist_info.cache.snapshot.contains_key(&first));
    assert!(
        !app.artist_info.cache.snapshot.contains_key(&now),
        "the late result is not filed under the centred artist"
    );
    assert!(app.artist_info.is_pending(&now), "its own ask is still out");
}

#[test]
fn a_result_from_before_logout_is_dropped() {
    let _lock = super::SSE_SLOT_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut app = app_with_artists(20);
    let before = app.artist_info.session_generation();
    let first = navigate(&mut app, SlotListPageMessage::NavigateDown);
    settle(&mut app, &first);
    assert!(app.artist_info.is_pending(&first));

    let _ = app.reset_session_state();
    assert!(
        !app.artist_info.is_pending(&first),
        "logout forgets the ask"
    );

    land(&mut app, before, &first, Ok(bio("old server")));
    assert!(
        app.artist_info.cache.is_empty(),
        "a pre-logout answer must not reach the next session"
    );
}

#[test]
fn a_failed_fetch_is_asked_again_on_the_next_centring() {
    let mut app = app_with_artists(20);
    let generation = app.artist_info.session_generation();
    let first = navigate(&mut app, SlotListPageMessage::NavigateDown);
    settle(&mut app, &first);
    land(&mut app, generation, &first, Err("HTTP 500".to_string()));
    assert!(!app.artist_info.is_pending(&first));
    assert!(app.artist_info.cache.is_empty(), "a failure is not cached");

    navigate(&mut app, SlotListPageMessage::NavigateDown);
    let back = navigate(&mut app, SlotListPageMessage::NavigateUp);
    assert_eq!(back, first);
    settle(&mut app, &first);
    assert!(app.artist_info.is_pending(&first), "asked again");
}

/// A link click that finds and expands an artist skips the scroll-driven
/// trigger, so the expand path starts the settle too.
#[test]
fn expanding_an_artist_asks_for_its_info() {
    let mut app = app_with_artists(2);
    let _ = app.handle_artists(ArtistsMessage::FocusAndExpand(1));
    settle(&mut app, "a1");
    assert!(app.artist_info.is_pending("a1"));
}

/// A rejected session routes to the login screen, never to a quiet "no bio".
#[test]
fn a_rejected_session_routes_to_login() {
    let msg = Nokkvi::artist_info_result_message(
        0,
        "a1".to_string(),
        Err(NokkviError::Unauthorized.into()),
    );
    assert!(matches!(msg, Message::SessionExpired), "got: {msg:?}");

    let msg = Nokkvi::artist_info_result_message(0, "a1".to_string(), Err(anyhow::anyhow!("boom")));
    assert!(
        matches!(
            msg,
            Message::ArtistsLoader(ArtistsLoaderMessage::InfoLoaded { result: Err(_), .. })
        ),
        "got: {msg:?}"
    );
}

// ---------------------------------------------------------------------------
// The highlight across a background reload
// ---------------------------------------------------------------------------

/// The reload the server's artist refresh triggers, with the same rows: the
/// anchor is the row at the viewport, as `background_reload` sends it.
fn refresh_reload(app: &mut Nokkvi) {
    let artists: Vec<_> = app.library.artists.iter().cloned().collect();
    let total = artists.len();
    let anchor = app
        .library
        .artists
        .get(app.artists_page.common.slot_list.viewport_offset)
        .map(|a| a.id.clone());
    let _ = app.handle_artists_loaded(Ok(artists), total, true, anchor, None);
}

/// A jump to an artist (a link or a similar-artist chip) pins it to the top
/// slot, expanded; its first info fetch makes the server announce an artist
/// change, and the reload that follows dropped the pin, so the panel moved to
/// the row in the middle (2026-10-17: Vaperror to vecher 1998, Mare Cognitum
/// to Markus Guentner).
#[test]
fn a_jumped_to_artist_keeps_the_highlight_through_the_refresh_reload() {
    let mut app = app_with_artists(20);
    expand_artists_with(
        &mut app,
        "a5",
        vec![
            make_album("al1", "One", "Artist 5"),
            make_album("al2", "Two", "Artist 5"),
        ],
    );
    let total = app
        .artists_page
        .expansion
        .flattened_len(&app.library.artists);
    let list = &mut app.artists_page.common.slot_list;
    list.set_offset(9, total);
    list.pin_selected(5, total);
    assert_eq!(app.centred_artist_id().as_deref(), Some("a5"));

    refresh_reload(&mut app);
    let list = &app.artists_page.common.slot_list;
    assert_eq!(list.selected_offset, Some(5), "the pin stays on the artist");
    assert!(list.selected_offset_pinned, "still a pin");
    assert_eq!(
        app.centred_artist_id().as_deref(),
        Some("a5"),
        "the panel stays"
    );
}

/// A highlighted album row under the expanded artist stays highlighted too.
#[test]
fn a_highlighted_album_under_the_expanded_artist_survives_the_reload() {
    let mut app = app_with_artists(20);
    expand_artists_with(
        &mut app,
        "a5",
        vec![
            make_album("al1", "One", "Artist 5"),
            make_album("al2", "Two", "Artist 5"),
        ],
    );
    let total = app
        .artists_page
        .expansion
        .flattened_len(&app.library.artists);
    let list = &mut app.artists_page.common.slot_list;
    list.set_offset(7, total);
    list.set_selected(7, total); // "Two", the second album under a5

    refresh_reload(&mut app);
    assert_eq!(app.artists_page.common.slot_list.selected_offset, Some(7));
}

/// A highlighted row the reload no longer holds is dropped, as before.
#[test]
fn a_highlighted_artist_gone_from_the_reload_is_dropped() {
    let mut app = app_with_artists(20);
    let list = &mut app.artists_page.common.slot_list;
    list.set_offset(6, 20);
    list.set_selected(5, 20);
    let without_a5: Vec<_> = app
        .library
        .artists
        .iter()
        .filter(|a| a.id != "a5")
        .cloned()
        .collect();
    let _ = app.handle_artists_loaded(Ok(without_a5), 19, true, Some("a6".into()), None);
    assert_eq!(app.artists_page.common.slot_list.selected_offset, None);
}

/// A highlighted row the reload pushes off screen (rows arrived between it
/// and the viewport's anchor) is dropped: an unseen highlight would steer
/// Enter and the keys.
#[test]
fn a_highlight_pushed_off_screen_by_the_reload_is_dropped() {
    let mut app = app_with_artists(30);
    let list = &mut app.artists_page.common.slot_list;
    list.set_offset(15, 30);
    list.set_selected(17, 30);
    let mut rows: Vec<_> = app.library.artists.iter().cloned().collect();
    let arrivals = (0..10).map(|i| make_artist(&format!("n{i}"), &format!("New {i}")));
    rows.splice(16..16, arrivals);
    let _ = app.handle_artists_loaded(Ok(rows), 40, true, Some("a15".into()), None);
    assert_eq!(app.artists_page.common.slot_list.viewport_offset, 15);
    assert_eq!(app.artists_page.common.slot_list.selected_offset, None);
}
