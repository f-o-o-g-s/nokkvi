//! A background reload (`handle_library_changed`) keeps the user's place in a
//! paged view: it fetches every row down to the viewport, so the row the
//! viewport was on comes back and the view stays on it, and a page asked for
//! before the reload can't land on top of the reloaded rows.

use nokkvi_data::{backend::artists::ArtistUIViewData, types::paged_buffer::ReplaceTicket};

use crate::{
    View,
    app_message::{ArtistsLoaderMessage, Message},
    services::navidrome_sse::LibraryChange,
    test_helpers::*,
    update::{ArtistsTarget, Reload},
};

fn artists(range: std::ops::Range<usize>) -> Vec<ArtistUIViewData> {
    range
        .map(|i| make_artist(&format!("r{i}"), &format!("Artist {i}")))
        .collect()
}

/// A logged-in app on the Artists view: `loaded` of `total` artists in the
/// buffer, the viewport's top row at `top`.
fn artists_app(loaded: usize, total: usize, top: usize) -> crate::Nokkvi {
    let mut app = test_app();
    app.screen = crate::Screen::Home;
    app.current_view = View::Artists;
    app.library
        .artists
        .set_first_page(artists(0..loaded), total);
    app.artists_page.common.slot_list.viewport_offset = top;
    app
}

fn reload_lands(app: &mut crate::Nokkvi, rows: Vec<ArtistUIViewData>, anchor_id: Option<String>) {
    let total_count = rows.len();
    let _ = app.update(Message::ArtistsLoader(ArtistsLoaderMessage::Loaded {
        result: Ok(rows),
        total_count,
        background: true,
        anchor_id,
        replace: None,
    }));
}

/// A list-replacing load's answer lands, carrying its place in line.
fn load_lands(
    app: &mut crate::Nokkvi,
    result: Result<Vec<ArtistUIViewData>, String>,
    background: bool,
    replace: ReplaceTicket,
) {
    let total_count = result.as_ref().map_or(0, Vec::len);
    let _ = app.update(Message::ArtistsLoader(ArtistsLoaderMessage::Loaded {
        result,
        total_count,
        background,
        anchor_id: background.then(|| "r905".to_string()),
        replace: Some(replace),
    }));
}

/// A background reload (now up to 10,000 rows) landing after a newer,
/// one-page search: the search's rows stand, or the user would see every
/// artist under a filled search box and the view would jump to the old
/// anchor.
#[test]
fn a_reload_landing_after_a_newer_search_is_dropped() {
    let mut app = artists_app(917, 917, 905);
    let reload = app.library.artists.begin_replace();
    let search = app.library.artists.begin_replace();

    load_lands(&mut app, Ok(artists(0..3)), false, search);
    load_lands(&mut app, Ok(artists(0..917)), true, reload);

    assert_eq!(
        app.library.artists.len(),
        3,
        "the newer search's rows stand"
    );
    assert_eq!(app.artists_page.common.slot_list.viewport_offset, 0);
}

/// The same stale reload failing is dropped quietly: no toast, no end to
/// a find chain, for rows nobody waits for.
#[test]
fn a_failed_stale_reload_toasts_nothing() {
    let mut app = artists_app(917, 917, 905);
    let reload = app.library.artists.begin_replace();
    let search = app.library.artists.begin_replace();
    load_lands(&mut app, Ok(artists(0..3)), false, search);

    load_lands(&mut app, Err("timed out".into()), true, reload);

    assert!(app.toast.toasts.is_empty());
    assert_eq!(app.library.artists.len(), 3);
}

/// The newest load always lands, whichever order the answers arrive in.
#[test]
fn the_newest_load_lands_after_an_older_one() {
    let mut app = artists_app(917, 917, 905);
    let reload = app.library.artists.begin_replace();
    let search = app.library.artists.begin_replace();

    load_lands(&mut app, Ok(artists(0..917)), true, reload);
    assert_eq!(
        app.library.artists.len(),
        917,
        "nothing newer has landed yet"
    );
    load_lands(&mut app, Ok(artists(0..3)), false, search);

    assert_eq!(app.library.artists.len(), 3);
}

/// The report: centred near the end of a 917-artist list, each artist
/// refresh (the server announces one per agent fill, about every 5 s while
/// browsing) reloaded only the first 500 rows, so the row the viewport was
/// on was gone and the view jumped to the top.
#[test]
fn a_reload_fetches_every_row_down_to_the_viewport() {
    let deep = artists_app(917, 917, 905);
    assert_eq!(
        deep.background_reload::<ArtistsTarget>(),
        Some(Reload {
            rows: 1500,
            anchor_id: Some("r905".into()),
        }),
        "three 500-row pages reach past the viewport and its look-ahead"
    );

    let top = artists_app(917, 917, 0);
    assert_eq!(
        top.background_reload::<ArtistsTarget>()
            .map(|reload| reload.rows),
        Some(500),
        "at the top, one page as before"
    );
}

/// Once those rows land the view is on the same row, here one further down
/// after an artist was added above it.
#[test]
fn the_reload_lands_on_the_row_the_viewport_was_on() {
    let mut app = artists_app(917, 917, 905);
    let reload = app.background_reload::<ArtistsTarget>().expect("a reload");
    let mut rows = artists(0..917);
    rows.insert(0, make_artist("new", "Aaa"));
    reload_lands(&mut app, rows, reload.anchor_id);
    assert_eq!(app.artists_page.common.slot_list.viewport_offset, 906);
}

/// A row gone from the reloaded list leaves the view where it was rather
/// than at the top, as Albums already did.
#[test]
fn a_missing_anchor_keeps_the_view_near_its_row() {
    let mut app = artists_app(917, 917, 905);
    let rows = artists(0..917)
        .into_iter()
        .filter(|artist| artist.id != "r905")
        .collect();
    reload_lands(&mut app, rows, Some("r905".into()));
    assert_eq!(app.artists_page.common.slot_list.viewport_offset, 905);
}

/// Scrolled past `MAX_RELOAD_ROWS`, a view keeps its rows: refetching every
/// row above it on each event would cost seconds.
#[test]
fn a_view_scrolled_past_the_reload_limit_keeps_its_rows() {
    let mut app = test_app();
    app.screen = crate::Screen::Home;
    app.library.songs.set_first_page(
        (0..12_000)
            .map(|i| make_song(&format!("s{i}"), "Song", "Artist"))
            .collect(),
        17_000,
    );
    app.songs_page.common.slot_list.viewport_offset = 11_000;

    let _ = app.handle_library_changed(LibraryChange {
        song_ids: vec!["s1".into()],
        ..Default::default()
    });

    assert!(!app.library.songs.is_loading(), "no reload starts");
    assert_eq!(app.library.songs.len(), 12_000);
    assert_eq!(app.songs_page.common.slot_list.viewport_offset, 11_000);
}

fn page_lands(
    app: &mut crate::Nokkvi,
    slot: nokkvi_data::types::paged_buffer::PageSlot,
    result: Result<Vec<ArtistUIViewData>, String>,
) {
    let _ = app.update(Message::ArtistsLoader(ArtistsLoaderMessage::PageLoaded {
        slot,
        result,
        total_count: 917,
    }));
}

/// A page asked for before a reload, landing after it: the reloaded rows
/// stand, nothing repeated (the Shift+C find chain's page against an artist
/// refresh).
#[test]
fn a_page_asked_for_before_a_reload_is_dropped() {
    let mut app = artists_app(500, 917, 400);
    let slot = app.library.artists.page_slot(500);
    reload_lands(&mut app, artists(0..917), Some("r400".into()));

    page_lands(&mut app, slot, Ok(artists(500..917)));

    assert_eq!(app.library.artists.len(), 917);
}

/// The same page failing is dropped quietly too: no toast for rows nobody
/// waits for.
#[test]
fn a_failed_page_from_a_replaced_list_toasts_nothing() {
    let mut app = artists_app(500, 917, 400);
    let slot = app.library.artists.page_slot(500);
    reload_lands(&mut app, artists(0..917), Some("r400".into()));

    page_lands(&mut app, slot, Err("timed out".into()));

    assert!(app.toast.toasts.is_empty());
}

/// A page that still follows the loaded rows lands as before.
#[test]
fn a_page_lands_right_after_the_rows_it_follows() {
    let mut app = artists_app(500, 917, 400);
    let slot = app.library.artists.page_slot(500);

    page_lands(&mut app, slot, Ok(artists(500..917)));

    assert_eq!(app.library.artists.len(), 917);
    assert_eq!(app.library.artists[500].id, "r500");
}
