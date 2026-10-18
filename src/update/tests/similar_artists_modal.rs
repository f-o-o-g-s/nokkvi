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

// ---------------------------------------------------------------------------
// The Similar Artists key (Shift+R) and the library row menus
// ---------------------------------------------------------------------------

fn press_similar_artists_key(app: &mut Nokkvi) {
    let _ = app.update(Message::Hotkey(
        crate::app_message::HotkeyMessage::SimilarArtists,
    ));
}

fn opened_for(app: &Nokkvi) -> Option<(String, String)> {
    app.similar_artists_modal
        .as_ref()
        .map(|state| (state.artist_id.clone(), state.artist_name.clone()))
}

/// A song row: the key asks about the song's artist.
#[test]
fn the_key_opens_the_focused_songs_artist() {
    let mut app = test_app();
    app.current_view = crate::View::Songs;
    let mut song = make_song("s1", "Song", "Muse");
    song.artist_id = Some("ar-muse".to_string());
    app.library.songs.set_from_vec(vec![song]);
    press_similar_artists_key(&mut app);
    assert_eq!(
        opened_for(&app),
        Some(("ar-muse".to_string(), "Muse".to_string()))
    );
}

/// An artist row is the artist; an album row asks about its album artist.
#[test]
fn the_key_on_an_artist_or_album_row_asks_about_that_artist() {
    let mut app = test_app();
    app.current_view = crate::View::Artists;
    app.library
        .artists
        .set_from_vec(vec![make_artist("ar1", "Burial")]);
    press_similar_artists_key(&mut app);
    assert_eq!(
        opened_for(&app),
        Some(("ar1".to_string(), "Burial".to_string()))
    );

    let mut app = test_app();
    app.current_view = crate::View::Albums;
    app.library
        .albums
        .set_from_vec(vec![make_album("al1", "Untrue", "Burial")]);
    press_similar_artists_key(&mut app);
    assert_eq!(
        opened_for(&app),
        Some(("artist_al1".to_string(), "Burial".to_string())),
        "make_album's artist id"
    );
}

/// The key acts on the highlighted queue row (a click under Stable
/// Viewport), like the other keys.
#[test]
fn the_key_follows_the_highlighted_queue_row() {
    use crate::widgets::SlotListPageMessage;
    let mut app = test_app();
    app.current_view = crate::View::Queue;
    app.library.queue_songs = (0..20)
        .map(|i| make_queue_song(&format!("s{i}"), "Song", &format!("Artist {i}"), "Album"))
        .collect();
    let _ = app.update(Message::Queue(QueueMessage::SlotList(
        SlotListPageMessage::NavigateDown,
    )));
    let centre = app
        .queue_page
        .common
        .get_center_item_index(20)
        .expect("a centre");
    let clicked = centre + 2;
    let _ = app.update(Message::Queue(QueueMessage::SlotList(
        SlotListPageMessage::SetOffset(clicked, iced::keyboard::Modifiers::default()),
    )));
    press_similar_artists_key(&mut app);
    assert_eq!(
        opened_for(&app).map(|(_, name)| name),
        Some(format!("Artist {clicked}"))
    );
}

/// Pressed again with the modal on top, the key closes it, and the key gate
/// admits it there (as Get Info's own key).
#[test]
fn the_key_again_closes_the_modal() {
    use nokkvi_data::types::hotkey_config::HotkeyAction;

    use crate::update::modals::ActiveModal;

    assert!(ActiveModal::SimilarArtists.takes_action(HotkeyAction::SimilarArtists));
    let mut app = app_with_queue();
    press_similar_artists_key(&mut app);
    assert!(app.similar_artists_modal.is_some());
    press_similar_artists_key(&mut app);
    assert!(app.similar_artists_modal.is_none());
}

/// A row that names no artist (a playlist) toasts and opens nothing; so
/// does a song the server sent without an artist id.
#[test]
fn a_row_without_an_artist_toasts() {
    use crate::update::similar_artists_modal::NO_ARTIST_TOAST;

    let mut app = test_app();
    app.current_view = crate::View::Playlists;
    app.library
        .playlists
        .set_from_vec(vec![make_playlist("p1", "Road Trip")]);
    press_similar_artists_key(&mut app);
    assert!(app.similar_artists_modal.is_none());
    assert_eq!(
        app.toast.toasts.back().map(|t| t.message.as_str()),
        Some(NO_ARTIST_TOAST)
    );

    let mut app = test_app();
    app.current_view = crate::View::Songs;
    app.library
        .songs
        .set_from_vec(vec![make_song("s1", "Song", "Someone")]);
    press_similar_artists_key(&mut app);
    assert!(app.similar_artists_modal.is_none(), "no artist id, no ask");
    assert_eq!(
        app.toast.toasts.back().map(|t| t.message.as_str()),
        Some(NO_ARTIST_TOAST)
    );
}

/// A Songs row's menu entry hands the row's artist on; the modal's Open
/// message, which the common route sends, opens it for that artist.
#[test]
fn a_songs_row_menu_entry_opens_the_rows_artist() {
    use crate::{
        views::{ArtistRef, SongsAction, SongsMessage},
        widgets::{
            context_menu::LibraryContextEntry, similar_artists_modal::SimilarArtistsModalMessage,
        },
    };

    let mut app = test_app();
    let mut song = make_song("s1", "Song", "Muse");
    song.artist_id = Some("ar-muse".to_string());
    let songs = vec![song];
    let (_, action) = app.songs_page.update(
        SongsMessage::ContextMenuAction(0, LibraryContextEntry::SimilarArtists),
        &songs,
    );
    let SongsAction::SimilarArtists(artist) = action else {
        panic!("the entry hands the artist on, got {action:?}");
    };
    assert_eq!(artist, ArtistRef::new("ar-muse", "Muse"));

    let _ = app.update(Message::SimilarArtistsModal(
        SimilarArtistsModalMessage::Open(artist),
    ));
    assert_eq!(
        opened_for(&app),
        Some(("ar-muse".to_string(), "Muse".to_string()))
    );
}

/// Harbour: an artist item row asks about that artist.
#[test]
fn the_key_on_a_harbour_artist_row_asks_about_that_artist() {
    use nokkvi_data::types::batch::BatchItem;

    use crate::views::harbour::{HarbourRow, HarbourSectionId, PlayTarget};

    let mut app = test_app();
    app.current_view = crate::View::Harbour;
    app.settings.harbour_expandable_shelves = true;
    // A Most Played shelf hides when its top item has no plays.
    let mut artist = super::harbour::search_artist("ar9", "Actress");
    artist.play_count = Some(5);
    app.harbour.most_played_artists = vec![artist];
    app.harbour_page
        .collapsed
        .remove(&HarbourSectionId::MostPlayedArtists);
    let rows = app.harbour_rows();
    let item = rows
        .iter()
        .position(|r| {
            matches!(
                r,
                HarbourRow::Item {
                    play: PlayTarget::Item(BatchItem::Artist(_)),
                    ..
                }
            )
        })
        .expect("the artist item row");
    app.harbour_page
        .common
        .slot_list
        .set_offset(item, rows.len());
    press_similar_artists_key(&mut app);
    assert_eq!(
        opened_for(&app),
        Some(("ar9".to_string(), "Actress".to_string()))
    );
}
