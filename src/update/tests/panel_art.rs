//! The big panel's full-size cover waits while the selection runs past rows:
//! a lone step fetches it at once, a run of steps fetches only the row it
//! stops on, once the settle timer fires (`PanelArtSettle`). Only steps in
//! the list the user is steering wait; a step in any other list loads
//! directly and leaves the gate alone.
//!
//! `loading_large_artwork` is the observable: it names the latest cover a
//! load started for. The gate reads tokio's clock, so these tests run on the
//! paused clock and step it by hand.

use nokkvi_data::{backend::albums::AlbumUIViewData, types::album::Album};

use crate::{
    Nokkvi, View,
    app_message::{ArtworkMessage, Message},
    state::{PANEL_ART_RUN_GAP, PanelArt},
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

fn settle(app: &mut Nokkvi, source: Option<View>, target: PanelArt) {
    let _ = app.update(Message::Artwork(ArtworkMessage::PanelArtSettled {
        source,
        target,
    }));
}

fn loading(app: &Nokkvi) -> Option<&str> {
    app.artwork.loading_large_artwork.as_deref()
}

fn cache_large(app: &mut Nokkvi, id: &str) {
    app.artwork.large_artwork.put(
        id.to_string(),
        iced::widget::image::Handle::from_rgba(1, 1, vec![0, 0, 0, 255]),
    );
}

#[tokio::test(start_paused = true)]
async fn a_lone_step_fetches_the_big_cover_at_once() {
    let mut app = albums_app(20);
    let first = album_step(&mut app);
    assert_eq!(loading(&app), Some(first.as_str()));

    // A press after a pause is lone again.
    tokio::time::advance(PANEL_ART_RUN_GAP).await;
    let later = album_step(&mut app);
    assert_eq!(loading(&app), Some(later.as_str()));
}

/// A held arrow key: the rows the run passes never fetch their full-size
/// cover, the row it stops on does, once its settle timer fires.
#[tokio::test(start_paused = true)]
async fn a_run_of_steps_fetches_only_the_row_it_stops_on() {
    let mut app = albums_app(20);
    let first = album_step(&mut app);
    let passed: Vec<String> = (0..3).map(|_| album_step(&mut app)).collect();
    assert_eq!(loading(&app), Some(first.as_str()), "the run's rows wait");

    let (stopped_on, passed_by) = passed.split_last().expect("moved");
    for id in passed_by {
        settle(&mut app, Some(View::Albums), PanelArt::Album(id.clone()));
    }
    assert_eq!(
        loading(&app),
        Some(first.as_str()),
        "a passed row's timer fetches nothing"
    );

    settle(
        &mut app,
        Some(View::Albums),
        PanelArt::Album(stopped_on.clone()),
    );
    assert_eq!(loading(&app), Some(stopped_on.as_str()));
}

/// A waiting step hands back its settle timer: without it the row a run
/// stops on would never be fetched.
#[tokio::test(start_paused = true)]
async fn a_waiting_step_schedules_its_settle_timer() {
    let mut app = albums_app(4);
    let lone = app.panel_art_step(Some(View::Albums), PanelArt::Album("a1".into()));
    assert_eq!(lone.units(), 0, "a lone step loads (no backend in tests)");
    let waiting = app.panel_art_step(Some(View::Albums), PanelArt::Album("a2".into()));
    assert_eq!(waiting.units(), 1, "the settle timer");
}

/// A shelf or buffer landing behind another view steps its own list: that
/// loads directly and leaves the row the visible run is waiting on alone.
#[tokio::test(start_paused = true)]
async fn a_step_in_a_list_out_of_sight_leaves_the_wait_alone() {
    let mut app = albums_app(20);
    album_step(&mut app);
    let waiting = album_step(&mut app);

    let _ = app.panel_art_step(Some(View::Harbour), PanelArt::Album("h1".into()));
    assert_eq!(loading(&app), Some("h1"), "out of sight: loaded directly");

    settle(
        &mut app,
        Some(View::Albums),
        PanelArt::Album(waiting.clone()),
    );
    assert_eq!(
        loading(&app),
        Some(waiting.as_str()),
        "the run's row still loads"
    );
}

/// A run's last row is fetched only while its list is still the one being
/// steered.
#[tokio::test(start_paused = true)]
async fn a_settle_after_a_view_switch_fetches_nothing() {
    let mut app = albums_app(20);
    let first = album_step(&mut app);
    let waiting = album_step(&mut app);

    app.current_view = View::Songs;
    settle(&mut app, Some(View::Albums), PanelArt::Album(waiting));

    assert_eq!(loading(&app), Some(first.as_str()));
}

/// The Queue steps through its message, cached rows included: a run that
/// passes a cached row leaves nothing waiting from before it.
#[tokio::test(start_paused = true)]
async fn a_queue_run_through_a_cached_row_leaves_nothing_waiting() {
    let mut app = test_app();
    assert_eq!(app.current_view, View::Queue);
    let step = |app: &mut Nokkvi, id: &str| {
        let _ = app.update(Message::Artwork(ArtworkMessage::PanelArtStep {
            source: Some(View::Queue),
            target: PanelArt::Album(id.into()),
        }));
    };
    step(&mut app, "q1");
    step(&mut app, "q2");
    cache_large(&mut app, "q3");
    step(&mut app, "q3");

    settle(&mut app, Some(View::Queue), PanelArt::Album("q2".into()));
    assert_eq!(loading(&app), Some("q1"), "the passed row is not fetched");
}

/// The Artists view's centring waits the same way.
#[tokio::test(start_paused = true)]
async fn artists_hold_the_big_cover_back_too() {
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
    let second = step(&mut app);
    assert_eq!(loading(&app), Some(first.as_str()), "a run's row waits");
    settle(
        &mut app,
        Some(View::Artists),
        PanelArt::Artist(second.clone()),
    );
    assert_eq!(loading(&app), Some(second.as_str()));
}

/// The Songs view's panel shows the centred song's album, and waits too.
#[tokio::test(start_paused = true)]
async fn songs_hold_the_big_cover_back_too() {
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
    let second = step(&mut app);
    assert_eq!(loading(&app), Some(first.as_str()), "a run's row waits");
    settle(&mut app, Some(View::Songs), PanelArt::Album(second.clone()));
    assert_eq!(loading(&app), Some(second.as_str()));
}

/// Logout resets the artwork state, so a settle timer still running from
/// the ended session finds nothing waiting.
#[tokio::test(start_paused = true)]
async fn a_settle_timer_from_an_ended_session_fetches_nothing() {
    let mut app = albums_app(20);
    album_step(&mut app);
    let waiting = album_step(&mut app);

    app.artwork = crate::state::ArtworkState::default();
    settle(&mut app, Some(View::Albums), PanelArt::Album(waiting));

    assert_eq!(loading(&app), None);
}
