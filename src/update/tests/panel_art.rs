//! The big panel keeps one full-size cover on its way at a time
//! (`PanelArtGate`): a step with nothing on its way fetches at once, a step
//! while a cover is on its way waits as the newest row, and the landing of
//! that cover sends the newest waiting row. Only steps in the list the user
//! is steering reach the gate; a step in any other list loads directly and
//! leaves it alone.
//!
//! `loading_large_artwork` is the observable: it names the latest cover a
//! load started for. Tests have no backend, so no loader claims a fetch;
//! `panel_gate.sent` stands in for one, except in
//! `a_loader_claim_marks_the_cover_on_its_way`.

use nokkvi_data::{backend::albums::AlbumUIViewData, types::album::Album};

use crate::{
    Nokkvi, View,
    app_message::{ArtworkMessage, Message},
    state::PanelArt,
    test_helpers::*,
    views::{AlbumsMessage, ArtistsMessage, SongsMessage},
    widgets::SlotListPageMessage,
};

fn album(id: &str) -> AlbumUIViewData {
    let v = serde_json::json!({ "id": id, "name": id, "updatedAt": "T1" });
    let album: Album = serde_json::from_value(v).expect("album json");
    AlbumUIViewData::from_album(&album, "http://srv", "u=x&s=y&t=z")
}

/// An app on the Albums view, with `count` albums.
fn albums_app(count: usize) -> Nokkvi {
    let mut app = test_app();
    app.current_view = View::Albums;
    app.library
        .albums
        .set_from_vec((0..count).map(|i| album(&format!("a{i}"))).collect());
    app
}

/// Move the Albums centre one row down, returning the album now centred.
fn album_step(app: &mut Nokkvi) -> String {
    let _ = app.update(Message::Albums(AlbumsMessage::SlotList(
        SlotListPageMessage::NavigateDown,
    )));
    let len = app.library.albums.len();
    let idx = app
        .albums_page
        .common
        .get_center_item_index(len)
        .expect("an album is centred");
    app.library.albums[idx].id.clone()
}

/// The cover for `id` lands (here: a failed fetch, which also releases).
fn land(app: &mut Nokkvi, id: &str) {
    let _ = app.update(Message::Artwork(ArtworkMessage::LargeLoaded(
        id.to_string(),
        None,
    )));
}

fn loading(app: &Nokkvi) -> Option<&str> {
    app.artwork.loading_large_artwork.as_deref()
}

/// The pointer scrolls the browsing pane's Albums tab while keyboard focus
/// stays on the Queue: those steps steer the big panel too, so a cover on
/// its way holds them like a step in the main view (each wheel notch used
/// to send its own full-size request).
#[test]
fn a_browsing_tab_step_waits_for_the_cover_on_its_way() {
    let mut app = albums_app(20);
    app.current_view = View::Queue;
    let mut panel = crate::views::BrowsingPanel::new();
    panel.active_view = crate::views::BrowsingView::Albums;
    app.browsing_panel = Some(panel);
    assert!(app.split_view_active());

    let first = album_step(&mut app);
    app.artwork.panel_gate.sent(PanelArt::Album(first.clone()));
    let second = album_step(&mut app);
    assert_eq!(loading(&app), Some(first.as_str()), "the tab's step waits");

    land(&mut app, &first);
    assert_eq!(loading(&app), Some(second.as_str()));
}

#[test]
fn a_step_with_nothing_on_its_way_fetches_at_once() {
    let mut app = albums_app(20);
    let first = album_step(&mut app);
    assert_eq!(loading(&app), Some(first.as_str()));
    let second = album_step(&mut app);
    assert_eq!(
        loading(&app),
        Some(second.as_str()),
        "nothing was sent, so nothing waits"
    );
}

/// A held arrow key while a cover is on its way: the rows passed never fetch
/// their full-size cover; the newest goes out when that cover lands.
#[test]
fn steps_wait_for_the_cover_on_its_way_and_the_newest_follows_it() {
    let mut app = albums_app(20);
    let first = album_step(&mut app);
    app.artwork.panel_gate.sent(PanelArt::Album(first.clone()));

    let passed: Vec<String> = (0..3).map(|_| album_step(&mut app)).collect();
    assert_eq!(loading(&app), Some(first.as_str()), "the rows wait");

    land(&mut app, &first);
    let newest = passed.last().expect("moved");
    assert_eq!(loading(&app), Some(newest.as_str()));
}

/// A waiting step hands back its overdue timer: without it a stuck cover
/// would hold the row the scroll stopped on until the request timed out.
#[test]
fn a_waiting_step_schedules_its_overdue_timer() {
    let mut app = albums_app(4);
    let now = app.panel_art_step(Some(View::Albums), PanelArt::Album("a1".into()));
    assert_eq!(now.units(), 0, "a step that loads (no backend in tests)");
    app.artwork.panel_gate.sent(PanelArt::Album("a1".into()));
    let waiting = app.panel_art_step(Some(View::Albums), PanelArt::Album("a2".into()));
    assert_eq!(waiting.units(), 1, "the overdue timer");
}

#[test]
fn an_overdue_cover_lets_the_newest_row_through() {
    let mut app = albums_app(20);
    let first = album_step(&mut app);
    app.artwork.panel_gate.sent(PanelArt::Album(first.clone()));
    album_step(&mut app);
    let newest = album_step(&mut app);

    let _ = app.update(Message::Artwork(ArtworkMessage::PanelArtOverdue {
        source: Some(View::Albums),
        target: PanelArt::Album(newest.clone()),
    }));
    assert_eq!(loading(&app), Some(newest.as_str()));

    land(&mut app, &first);
    assert_eq!(
        loading(&app),
        Some(newest.as_str()),
        "the slow cover's landing sends nothing"
    );
}

/// A shelf or buffer landing behind another view steps its own list: that
/// loads directly and leaves the row the visible scroll is waiting on alone.
#[test]
fn a_step_in_a_list_out_of_sight_leaves_the_wait_alone() {
    let mut app = albums_app(20);
    let first = album_step(&mut app);
    app.artwork.panel_gate.sent(PanelArt::Album(first.clone()));
    let waiting = album_step(&mut app);

    let _ = app.panel_art_step(Some(View::Harbour), PanelArt::Album("h1".into()));
    assert_eq!(loading(&app), Some("h1"), "out of sight: loaded directly");

    land(&mut app, &first);
    assert_eq!(
        loading(&app),
        Some(waiting.as_str()),
        "the waiting row still goes"
    );
}

/// A waiting row is fetched only while its list is still the one steered.
#[test]
fn a_landing_after_a_view_switch_fetches_nothing() {
    let mut app = albums_app(20);
    let first = album_step(&mut app);
    app.artwork.panel_gate.sent(PanelArt::Album(first.clone()));
    let waiting = album_step(&mut app);

    app.current_view = View::Songs;
    land(&mut app, &first);

    assert_ne!(loading(&app), Some(waiting.as_str()));
}

/// The Queue steps through its message, cached rows included: a cached row
/// shows at once and leaves nothing waiting from before it.
#[test]
fn a_queue_step_onto_a_cached_row_leaves_nothing_waiting() {
    let mut app = test_app();
    assert_eq!(app.current_view, View::Queue);
    let step = |app: &mut Nokkvi, id: &str| {
        let _ = app.update(Message::Artwork(ArtworkMessage::PanelArtStep {
            source: Some(View::Queue),
            target: PanelArt::Album(id.into()),
        }));
    };
    step(&mut app, "q1");
    app.artwork.panel_gate.sent(PanelArt::Album("q1".into()));
    step(&mut app, "q2");
    app.artwork.large_artwork.put(
        "q3".to_string(),
        iced::widget::image::Handle::from_rgba(1, 1, vec![0, 0, 0, 255]),
    );
    step(&mut app, "q3");

    land(&mut app, "q1");
    assert_ne!(loading(&app), Some("q2"), "the passed row is not fetched");
}

/// The Artists view's centring goes through the gate too.
#[test]
fn artists_wait_for_the_cover_on_its_way_too() {
    let mut app = test_app();
    app.current_view = View::Artists;
    app.library.artists.set_from_vec(
        (0..20)
            .map(|i| make_artist(&format!("r{i}"), &format!("Artist {i}")))
            .collect(),
    );
    let step = |app: &mut Nokkvi| {
        let _ = app.update(Message::Artists(ArtistsMessage::SlotList(
            SlotListPageMessage::NavigateDown,
        )));
        app.centred_artist_id().expect("an artist is centred")
    };
    let first = step(&mut app);
    assert_eq!(loading(&app), Some(first.as_str()));
    app.artwork.panel_gate.sent(PanelArt::Artist(first.clone()));
    let second = step(&mut app);
    assert_eq!(loading(&app), Some(first.as_str()), "the row waits");

    let _ = app.update(Message::Artwork(ArtworkMessage::LargeArtistLoaded(
        first, None,
    )));
    assert_eq!(loading(&app), Some(second.as_str()));
}

/// The Songs view's panel shows the centred song's album, and waits too.
#[test]
fn songs_wait_for_the_cover_on_its_way_too() {
    let mut app = test_app();
    app.current_view = View::Songs;
    app.library.songs.set_from_vec(
        (0..20)
            .map(|i| {
                let mut song = make_song(&format!("s{i}"), "Song", "Artist");
                song.album_id = Some(format!("al{i}"));
                song
            })
            .collect(),
    );
    let step = |app: &mut Nokkvi| {
        let _ = app.update(Message::Songs(SongsMessage::SlotList(
            SlotListPageMessage::NavigateDown,
        )));
        let len = app.library.songs.len();
        let idx = app
            .songs_page
            .common
            .get_center_item_index(len)
            .expect("a song is centred");
        app.library.songs[idx].album_id.clone().expect("an album")
    };
    let first = step(&mut app);
    assert_eq!(loading(&app), Some(first.as_str()));
    app.artwork.panel_gate.sent(PanelArt::Album(first.clone()));
    let second = step(&mut app);
    assert_eq!(loading(&app), Some(first.as_str()), "the row waits");

    land(&mut app, &first);
    assert_eq!(loading(&app), Some(second.as_str()));
}

/// Logout resets the artwork state, so a cover landing from the ended
/// session finds nothing waiting.
#[test]
fn a_landing_from_an_ended_session_fetches_nothing() {
    let mut app = albums_app(20);
    let first = album_step(&mut app);
    app.artwork.panel_gate.sent(PanelArt::Album(first.clone()));
    album_step(&mut app);

    app.artwork = crate::state::ArtworkState::default();
    land(&mut app, &first);

    assert_eq!(loading(&app), None);
}

/// The gate learns a fetch went out from the loader's claim in
/// `large_artwork_pending`, which a fetch already on its way also holds.
#[test]
fn a_loader_claim_marks_the_cover_on_its_way() {
    let mut app = albums_app(8);
    // a3's full-size fetch is already out (its row version is "T1").
    app.artwork
        .large_artwork_pending
        .claim("a3".into(), Some("T1".into()));

    let _ = app.panel_art_step(Some(View::Albums), PanelArt::Album("a3".into()));
    let waiting = app.panel_art_step(Some(View::Albums), PanelArt::Album("a4".into()));

    assert_eq!(waiting.units(), 1, "a4 waits for a3");
    assert_eq!(loading(&app), Some("a3"));
}
