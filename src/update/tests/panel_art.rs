//! The big panel's full-size cover waits while the selection runs past rows:
//! a lone step fetches it at once, a run of steps fetches only the row it
//! stops on, once the settle timer fires (`PanelArtSettle`).
//!
//! `loading_large_artwork` is the observable: it names the latest cover a
//! load started for.

use nokkvi_data::{backend::albums::AlbumUIViewData, types::album::Album};

use crate::{
    Nokkvi,
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

fn app_with_albums(count: usize) -> Nokkvi {
    let mut app = test_app();
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

fn settle(app: &mut Nokkvi, target: PanelArt) {
    let _ = app.update(Message::Artwork(ArtworkMessage::PanelArtSettled(target)));
}

fn loading(app: &Nokkvi) -> Option<&str> {
    app.artwork.loading_large_artwork.as_deref()
}

#[test]
fn a_lone_step_fetches_the_big_cover_at_once() {
    let mut app = app_with_albums(20);
    let centred = album_step(&mut app);
    assert_eq!(loading(&app), Some(centred.as_str()));
}

/// A held arrow key: the rows the run passes never fetch their full-size
/// cover, the row it stops on does, once its settle timer fires.
#[test]
fn a_run_of_steps_fetches_only_the_row_it_stops_on() {
    let mut app = app_with_albums(20);
    let first = album_step(&mut app);
    let passed: Vec<String> = (0..3).map(|_| album_step(&mut app)).collect();
    assert_eq!(loading(&app), Some(first.as_str()), "the run's rows wait");

    let (stopped_on, passed_by) = passed.split_last().expect("moved");
    for id in passed_by {
        settle(&mut app, PanelArt::Album(id.clone()));
    }
    assert_eq!(
        loading(&app),
        Some(first.as_str()),
        "a passed row's timer fetches nothing"
    );

    settle(&mut app, PanelArt::Album(stopped_on.clone()));
    assert_eq!(loading(&app), Some(stopped_on.as_str()));
}

/// The Artists view's centring waits the same way.
#[test]
fn artists_hold_the_big_cover_back_too() {
    let mut app = test_app();
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
    settle(&mut app, PanelArt::Artist(second.clone()));
    assert_eq!(loading(&app), Some(second.as_str()));
}

/// The Songs view's panel shows the centred song's album, and waits too.
#[test]
fn songs_hold_the_big_cover_back_too() {
    let mut app = test_app();
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
    settle(&mut app, PanelArt::Album(second.clone()));
    assert_eq!(loading(&app), Some(second.as_str()));
}

/// Logout resets the artwork state, so a settle timer still running from
/// the ended session finds nothing waiting.
#[test]
fn a_settle_timer_from_an_ended_session_fetches_nothing() {
    let mut app = app_with_albums(20);
    album_step(&mut app);
    let waiting = album_step(&mut app);

    app.artwork = crate::state::ArtworkState::default();
    settle(&mut app, PanelArt::Album(waiting));

    assert_eq!(loading(&app), None);
}
