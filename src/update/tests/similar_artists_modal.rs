//! The Similar Artists modal: opened from a queue row's menu for that row's
//! artist, filled from the same ListenBrainz list the Artists panel uses
//! once it lands, and acting on a row the way the panel's chips do.

use nokkvi_data::types::{
    artist::ArtistInfo,
    artist_recs::{ArtistRec, LibraryArtist, LibraryMatch},
};

use super::artist_recs::rec;
use crate::{
    Nokkvi,
    app_message::{ArtistsLoaderMessage, Message},
    test_helpers::*,
    views::{QueueMessage, queue::QueueContextEntry},
    widgets::similar_artists_modal::{SimilarArtistsModalMessage, SimilarArtistsStatus},
};

/// An app whose queue holds one song by `artist_<id>`, on the Queue view.
fn app_with_queue() -> Nokkvi {
    let mut app = test_app();
    app.current_view = crate::View::Queue;
    app.library.queue_songs = vec![make_queue_song("s1", "Song", "DJ Smokey", "Album")];
    app
}

const ARTIST_ID: &str = "artist_s1";

fn open_from_row(app: &mut Nokkvi) {
    let _ = app.update(Message::Queue(QueueMessage::ContextMenuAction(
        0,
        QueueContextEntry::SimilarArtists,
    )));
}

fn land_info(app: &mut Nokkvi, result: Result<ArtistInfo, String>) {
    let generation = app.artist_info.session_generation();
    let _ = app.update(Message::ArtistsLoader(ArtistsLoaderMessage::InfoLoaded {
        generation,
        artist_id: ARTIST_ID.to_string(),
        result,
    }));
}

fn land_recs(app: &mut Nokkvi, result: Result<Vec<ArtistRec>, String>) {
    let generation = app.artist_recs.session_generation();
    let _ = app.update(Message::ArtistsLoader(ArtistsLoaderMessage::RecsLoaded {
        generation,
        artist_id: ARTIST_ID.to_string(),
        result,
    }));
}

fn info_with_mbid() -> ArtistInfo {
    ArtistInfo {
        musicbrainz_id: Some("mbid-dj-smokey".to_string()),
        ..ArtistInfo::default()
    }
}

fn found(name: &str, album_artist: bool) -> ArtistRec {
    ArtistRec {
        library: LibraryMatch::Found(LibraryArtist {
            id: format!("lib-{name}"),
            album_artist,
        }),
        ..rec(name)
    }
}

fn modal(app: &Nokkvi) -> &crate::widgets::similar_artists_modal::SimilarArtistsModalState {
    app.similar_artists_modal
        .as_ref()
        .expect("the modal is open")
}

fn press(app: &mut Nokkvi, msg: SimilarArtistsModalMessage) {
    let _ = app.update(Message::SimilarArtistsModal(msg));
}

/// The row's menu opens the modal for the row's artist and asks for that
/// artist's info (the queue row carries no MusicBrainz id) and, once that
/// lands with one, the list.
#[test]
fn opening_from_a_queue_row_asks_about_that_artist() {
    let mut app = app_with_queue();
    open_from_row(&mut app);
    assert_eq!(modal(&app).artist_id, ARTIST_ID);
    assert_eq!(modal(&app).artist_name, "DJ Smokey");
    assert_eq!(modal(&app).status, SimilarArtistsStatus::Loading);
    assert!(app.artist_info.is_pending(ARTIST_ID));
    assert!(app.artist_recs.is_awaiting(ARTIST_ID));

    land_info(&mut app, Ok(info_with_mbid()));
    assert!(
        app.artist_recs.is_pending(ARTIST_ID),
        "asked with the info's id"
    );
    assert_eq!(modal(&app).status, SimilarArtistsStatus::Loading);
}

#[test]
fn the_list_fills_the_open_modal_when_it_lands() {
    let mut app = app_with_queue();
    open_from_row(&mut app);
    land_info(&mut app, Ok(info_with_mbid()));
    land_recs(
        &mut app,
        Ok(vec![found("Travis Scott", false), rec("Lil Peep")]),
    );
    let state = modal(&app);
    assert_eq!(state.status, SimilarArtistsStatus::Ready);
    let names: Vec<&str> = state
        .list
        .filtered
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(names, ["Travis Scott", "Lil Peep"]);
}

/// A list the Artists panel already fetched opens the modal ready, with no
/// new ask.
#[test]
fn a_cached_list_opens_ready() {
    let mut app = app_with_queue();
    app.artist_recs
        .settle_with(ARTIST_ID, vec![rec("Lil Peep")]);
    open_from_row(&mut app);
    assert_eq!(modal(&app).status, SimilarArtistsStatus::Ready);
    assert_eq!(modal(&app).list.all.len(), 1);
    assert!(!app.artist_info.is_pending(ARTIST_ID));
    assert!(!app.artist_recs.is_pending(ARTIST_ID));
}

#[test]
fn a_failed_ask_reads_as_failed_and_a_missing_id_as_no_id() {
    let mut app = app_with_queue();
    open_from_row(&mut app);
    land_info(&mut app, Ok(info_with_mbid()));
    land_recs(&mut app, Err("HTTP 503".to_string()));
    assert_eq!(modal(&app).status, SimilarArtistsStatus::Failed);

    let mut app = app_with_queue();
    open_from_row(&mut app);
    land_info(&mut app, Ok(ArtistInfo::default()));
    assert_eq!(modal(&app).status, SimilarArtistsStatus::NoMbid);

    let mut app = app_with_queue();
    open_from_row(&mut app);
    land_info(&mut app, Err("HTTP 500".to_string()));
    assert_eq!(modal(&app).status, SimilarArtistsStatus::Failed);
}

#[test]
fn with_the_setting_off_the_modal_says_so_and_asks_nothing() {
    let mut app = app_with_queue();
    app.settings.similar_artists_online = false;
    open_from_row(&mut app);
    assert_eq!(modal(&app).status, SimilarArtistsStatus::Off);
    assert!(!app.artist_info.is_pending(ARTIST_ID));
    assert!(!app.artist_recs.is_awaiting(ARTIST_ID));
}

/// Enter on an artist the library has closes the modal (the navigation
/// follows as a task); on one it lacks, the modal stays for the next row.
#[test]
fn enter_closes_the_modal_only_for_a_library_artist() {
    for album_artist in [true, false] {
        let mut app = app_with_queue();
        app.artist_recs.settle_with(
            ARTIST_ID,
            vec![found("Travis Scott", album_artist), rec("Lil Peep")],
        );
        open_from_row(&mut app);
        press(&mut app, SimilarArtistsModalMessage::ActivateCenter);
        assert!(
            app.similar_artists_modal.is_none(),
            "a library artist (album artist: {album_artist}) closes the modal"
        );
    }

    let mut app = app_with_queue();
    app.artist_recs
        .settle_with(ARTIST_ID, vec![rec("Lil Peep")]);
    open_from_row(&mut app);
    press(&mut app, SimilarArtistsModalMessage::ClickItem(0));
    assert!(
        app.similar_artists_modal.is_some(),
        "a missing artist keeps it open"
    );
}

#[test]
fn the_search_filters_by_name_and_note() {
    let mut app = app_with_queue();
    let mut noted = rec("Muse");
    noted.comment = Some("UK rock band".to_string());
    app.artist_recs
        .settle_with(ARTIST_ID, vec![noted, rec("Lil Peep")]);
    open_from_row(&mut app);
    press(
        &mut app,
        SimilarArtistsModalMessage::SearchChanged("rock".to_string()),
    );
    let names: Vec<&str> = modal(&app)
        .list
        .filtered
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(names, ["Muse"]);
    press(
        &mut app,
        SimilarArtistsModalMessage::SearchChanged("peep".to_string()),
    );
    let names: Vec<&str> = modal(&app)
        .list
        .filtered
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(names, ["Lil Peep"]);
}

/// A list landing for another artist leaves the modal alone.
#[test]
fn another_artists_list_does_not_fill_the_modal() {
    let mut app = app_with_queue();
    open_from_row(&mut app);
    let generation = app.artist_recs.session_generation();
    let _ = app.update(Message::ArtistsLoader(ArtistsLoaderMessage::RecsLoaded {
        generation,
        artist_id: "someone-else".to_string(),
        result: Ok(vec![rec("Lil Peep")]),
    }));
    assert_eq!(modal(&app).status, SimilarArtistsStatus::Loading);
    assert!(modal(&app).list.all.is_empty());
}
