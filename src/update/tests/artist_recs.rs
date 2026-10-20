//! The Artists view asks for the centred artist's similar artists once that
//! artist has stayed centred, once per artist: ListenBrainz with the
//! artist's MusicBrainz id from its row or, failing that, from the
//! `getArtistInfo2` answer, and Deezer (when turned on) with the artist's
//! name. Each list is kept under its own artist id.

use nokkvi_data::types::{
    artist::ArtistInfo,
    artist_recs::{ArtistRec, LibraryMatch, RecSource},
    error::NokkviError,
};

use crate::{
    Nokkvi,
    app_message::{ArtistsLoaderMessage, Message},
    test_helpers::*,
    views::{ArtistsAction, ArtistsMessage},
    widgets::SlotListPageMessage,
};

/// An app with `count` artists, each tagged with a MusicBrainz id when
/// `tagged`.
fn app_with_artists(count: usize, tagged: bool) -> Nokkvi {
    let mut app = test_app();
    app.library.artists.set_from_vec(
        (0..count)
            .map(|i| {
                let mut artist = make_artist(&format!("a{i}"), &format!("Artist {i}"));
                if tagged {
                    artist.mbz_artist_id = Some(format!("mbid-{i}"));
                }
                artist
            })
            .collect(),
    );
    app
}

/// Move the centre one row, returning the artist now centred.
fn navigate(app: &mut Nokkvi, msg: SlotListPageMessage) -> String {
    let _ = app.update(Message::Artists(ArtistsMessage::SlotList(msg)));
    app.centred_artist_id().expect("an artist is centred")
}

/// The settle timer for `id` fires.
fn settle(app: &mut Nokkvi, id: &str) {
    let _ = app.update(Message::ArtistsLoader(ArtistsLoaderMessage::InfoSettled(
        id.to_string(),
    )));
}

fn land_info(app: &mut Nokkvi, generation: u64, id: &str, result: Result<ArtistInfo, String>) {
    let _ = app.update(Message::ArtistsLoader(ArtistsLoaderMessage::InfoLoaded {
        generation,
        artist_id: id.to_string(),
        result,
    }));
}

fn land_recs(app: &mut Nokkvi, generation: u64, id: &str, result: Result<Vec<ArtistRec>, String>) {
    let _ = app.update(Message::ArtistsLoader(ArtistsLoaderMessage::RecsLoaded {
        generation,
        artist_id: id.to_string(),
        result,
    }));
}

pub(super) fn rec(name: &str) -> ArtistRec {
    ArtistRec {
        source: RecSource::ListenBrainz {
            mbid: format!("mbid-{name}"),
        },
        name: name.to_string(),
        comment: None,
        library: LibraryMatch::Missing,
    }
}

/// Turn the two similar-artists sources on or off.
fn sources(app: &mut Nokkvi, listenbrainz: bool, deezer: bool) {
    app.settings.similar_artists_online = listenbrainz;
    app.settings.similar_artists_deezer = deezer;
}

fn cached_names(app: &Nokkvi, id: &str) -> Option<Vec<String>> {
    app.artist_recs
        .cache
        .snapshot
        .get(id)
        .map(|recs| recs.iter().map(|r| r.name.clone()).collect())
}

/// The row carries the MusicBrainz id, so the similar artists are asked for
/// at the same moment as the info, once, and kept under the artist.
#[test]
fn a_tagged_artist_that_stays_centred_is_asked_about_once() {
    let mut app = app_with_artists(20, true);
    let generation = app.artist_recs.session_generation();

    let first = navigate(&mut app, SlotListPageMessage::NavigateDown);
    assert!(
        !app.artist_recs.is_pending(&first),
        "nothing is asked before the centre settles"
    );
    settle(&mut app, &first);
    assert!(app.artist_recs.is_pending(&first), "asked once settled");
    assert!(
        app.artist_info.is_pending(&first),
        "the info is asked alongside"
    );

    land_recs(&mut app, generation, &first, Ok(vec![rec("Muse")]));
    assert!(!app.artist_recs.is_pending(&first));
    assert_eq!(cached_names(&app, &first), Some(vec!["Muse".to_string()]));

    // Away and back: the cached artist is not asked about again.
    navigate(&mut app, SlotListPageMessage::NavigateDown);
    let back = navigate(&mut app, SlotListPageMessage::NavigateUp);
    assert_eq!(back, first);
    settle(&mut app, &first);
    assert!(!app.artist_recs.is_pending(&first));
}

/// Without a MusicBrainz id on the row, the ask waits for the info fetch and
/// goes out with the id that answer supplies.
#[test]
fn an_untagged_artist_waits_for_the_info_and_asks_with_its_id() {
    let mut app = app_with_artists(20, false);
    let info_generation = app.artist_info.session_generation();
    let first = navigate(&mut app, SlotListPageMessage::NavigateDown);
    settle(&mut app, &first);
    assert!(!app.artist_recs.is_pending(&first), "no id to ask with yet");
    assert!(app.artist_recs.is_awaiting(&first));
    assert!(app.artist_info.is_pending(&first));

    land_info(
        &mut app,
        info_generation,
        &first,
        Ok(ArtistInfo {
            musicbrainz_id: Some("mbid-from-info".to_string()),
            ..ArtistInfo::default()
        }),
    );
    assert!(!app.artist_recs.is_awaiting(&first));
    assert!(
        app.artist_recs.is_pending(&first),
        "asked with the info's id"
    );
}

/// When neither the row nor the info has an id, the artist settles on an
/// empty list and is not asked about again this session.
#[test]
fn an_artist_with_no_id_anywhere_settles_on_no_list() {
    let mut app = app_with_artists(20, false);
    let info_generation = app.artist_info.session_generation();
    let first = navigate(&mut app, SlotListPageMessage::NavigateDown);
    settle(&mut app, &first);
    land_info(&mut app, info_generation, &first, Ok(ArtistInfo::default()));
    assert!(!app.artist_recs.is_awaiting(&first));
    assert!(!app.artist_recs.is_pending(&first));
    assert_eq!(cached_names(&app, &first), Some(Vec::new()));

    navigate(&mut app, SlotListPageMessage::NavigateDown);
    let back = navigate(&mut app, SlotListPageMessage::NavigateUp);
    assert_eq!(back, first);
    settle(&mut app, &first);
    assert!(!app.artist_recs.is_pending(&first), "not asked again");
}

/// A failed info fetch ends the wait; the next centring asks for both again.
#[test]
fn a_failed_info_fetch_ends_the_wait_until_the_next_centring() {
    let mut app = app_with_artists(20, false);
    let info_generation = app.artist_info.session_generation();
    let first = navigate(&mut app, SlotListPageMessage::NavigateDown);
    settle(&mut app, &first);
    land_info(
        &mut app,
        info_generation,
        &first,
        Err("HTTP 500".to_string()),
    );
    assert!(!app.artist_recs.is_awaiting(&first));
    assert!(
        !app.artist_recs.is_cached(&first),
        "a failure is not remembered"
    );

    navigate(&mut app, SlotListPageMessage::NavigateDown);
    let back = navigate(&mut app, SlotListPageMessage::NavigateUp);
    assert_eq!(back, first);
    settle(&mut app, &first);
    assert!(
        app.artist_recs.is_awaiting(&first),
        "waiting on the retried info"
    );
    assert!(app.artist_info.is_pending(&first));
}

/// With the setting off nothing is asked of ListenBrainz and nothing waits,
/// while the info is still asked for.
#[test]
fn with_the_setting_off_nothing_is_asked() {
    let mut app = app_with_artists(20, true);
    app.settings.similar_artists_online = false;
    let first = navigate(&mut app, SlotListPageMessage::NavigateDown);
    settle(&mut app, &first);
    assert!(!app.artist_recs.is_pending(&first));
    assert!(!app.artist_recs.is_awaiting(&first));
    assert!(app.artist_recs.cache.is_empty());
    assert!(app.artist_info.is_pending(&first), "the info is unaffected");
}

#[test]
fn a_failed_fetch_is_asked_again_on_the_next_centring() {
    let mut app = app_with_artists(20, true);
    let generation = app.artist_recs.session_generation();
    let first = navigate(&mut app, SlotListPageMessage::NavigateDown);
    settle(&mut app, &first);
    land_recs(&mut app, generation, &first, Err("HTTP 503".to_string()));
    assert!(!app.artist_recs.is_pending(&first));
    assert!(app.artist_recs.cache.is_empty(), "a failure is not cached");

    navigate(&mut app, SlotListPageMessage::NavigateDown);
    let back = navigate(&mut app, SlotListPageMessage::NavigateUp);
    assert_eq!(back, first);
    settle(&mut app, &first);
    assert!(app.artist_recs.is_pending(&first), "asked again");
}

/// A list that lands after the user moved on is kept under the artist it
/// was asked for, never shown for the artist centred now.
#[test]
fn a_late_list_stays_with_its_own_artist() {
    let mut app = app_with_artists(20, true);
    let generation = app.artist_recs.session_generation();
    let first = navigate(&mut app, SlotListPageMessage::NavigateDown);
    settle(&mut app, &first);
    let now = navigate(&mut app, SlotListPageMessage::NavigateDown);
    settle(&mut app, &now);
    assert_ne!(first, now);

    land_recs(&mut app, generation, &first, Ok(vec![rec("Muse")]));
    assert_eq!(cached_names(&app, &first), Some(vec!["Muse".to_string()]));
    assert_eq!(cached_names(&app, &now), None);
    assert!(app.artist_recs.is_pending(&now), "its own ask is still out");
}

#[test]
fn a_list_from_before_logout_is_dropped() {
    let _lock = super::SSE_SLOT_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut app = app_with_artists(20, true);
    let before = app.artist_recs.session_generation();
    let first = navigate(&mut app, SlotListPageMessage::NavigateDown);
    settle(&mut app, &first);
    assert!(app.artist_recs.is_pending(&first));

    let _ = app.reset_session_state();
    assert!(
        !app.artist_recs.is_pending(&first),
        "logout forgets the ask"
    );

    land_recs(&mut app, before, &first, Ok(vec![rec("Old Server Band")]));
    assert!(
        app.artist_recs.cache.is_empty(),
        "a pre-logout list must not reach the next session"
    );
}

/// A rejected session (a library lookup refused) routes to the login
/// screen, never to a quiet "no similar artists".
#[test]
fn a_rejected_session_routes_to_login() {
    let msg = Nokkvi::artist_recs_result_message(
        0,
        "a1".to_string(),
        Err(NokkviError::Unauthorized.into()),
    );
    assert!(matches!(msg, Message::SessionExpired), "got: {msg:?}");

    let msg = Nokkvi::artist_recs_result_message(0, "a1".to_string(), Err(anyhow::anyhow!("boom")));
    assert!(
        matches!(
            msg,
            Message::ArtistsLoader(ArtistsLoaderMessage::RecsLoaded { result: Err(_), .. })
        ),
        "got: {msg:?}"
    );
}

/// A chip for an artist the library has bubbles the find-and-expand action
/// the root routes to the right pane.
#[test]
fn a_similar_artist_chip_bubbles_find_and_expand() {
    let mut app = test_app();
    let (_, action) = app.artists_page.update(
        ArtistsMessage::NavigateAndExpandArtist("ar9".to_string()),
        0,
        &[],
    );
    assert!(matches!(
        action,
        ArtistsAction::NavigateAndExpandArtist(ref id) if id == "ar9"
    ));
}

/// Turning the setting off hides the lists already fetched this session, as
/// its subtitle promises ("Off shows no list"); turning it back on shows them.
#[test]
fn the_setting_off_hides_lists_already_fetched() {
    let mut app = app_with_artists(3, true);
    app.artist_recs.settle_with("a0", vec![rec("Muse")]);
    assert_eq!(
        app.shown_artist_recs().len(),
        1,
        "on: the cached list shows"
    );
    app.settings.similar_artists_online = false;
    assert!(app.shown_artist_recs().is_empty(), "off: no list shows");
    app.settings.similar_artists_online = true;
    assert_eq!(app.shown_artist_recs().len(), 1, "back on: it shows again");
}

/// Deezer is asked by name, so with ListenBrainz off an artist with no
/// MusicBrainz id is asked about at once, with no wait for the info.
#[test]
fn with_only_deezer_on_an_untagged_artist_is_asked_by_name() {
    let mut app = app_with_artists(20, false);
    sources(&mut app, false, true);
    let first = navigate(&mut app, SlotListPageMessage::NavigateDown);
    settle(&mut app, &first);
    assert!(app.artist_recs.is_pending(&first), "asked by name");
    assert!(!app.artist_recs.is_awaiting(&first), "no id to wait for");
}

/// With both on, an untagged artist still waits for the info, so
/// ListenBrainz can join with the id it supplies; an answer with no id, or
/// a failed one, still lets Deezer be asked by name instead of settling on
/// no list.
#[test]
fn with_both_on_an_untagged_artist_waits_then_asks_deezer_without_an_id() {
    for info in [Ok(ArtistInfo::default()), Err("HTTP 500".to_string())] {
        let mut app = app_with_artists(20, false);
        sources(&mut app, true, true);
        let info_generation = app.artist_info.session_generation();
        let first = navigate(&mut app, SlotListPageMessage::NavigateDown);
        settle(&mut app, &first);
        assert!(app.artist_recs.is_awaiting(&first), "waits for the id");

        land_info(&mut app, info_generation, &first, info);
        assert!(!app.artist_recs.is_awaiting(&first));
        assert!(
            app.artist_recs.is_pending(&first),
            "Deezer is asked by name"
        );
    }
}

/// The lists show while either source is on.
#[test]
fn the_lists_show_with_only_deezer_on() {
    let mut app = app_with_artists(3, true);
    app.artist_recs.settle_with("a0", vec![rec("Muse")]);
    sources(&mut app, false, true);
    assert_eq!(app.shown_artist_recs().len(), 1);
    sources(&mut app, false, false);
    assert!(app.shown_artist_recs().is_empty());
}

/// Turning a source on or off changes what a list is made of: the lists
/// fetched before are dropped and the centred artist is asked again, so the
/// panel shows the new mix without waiting for the next centring. A reload
/// that changes neither source (a config hot reload) keeps them.
#[test]
fn a_change_of_sources_drops_the_lists_and_asks_again() {
    let _guard = crate::theme::THEME_MODE_LOCK.lock();
    let mut app = app_with_artists(20, true);
    let centred = navigate(&mut app, SlotListPageMessage::NavigateDown);
    app.artist_recs.settle_with(&centred, vec![rec("Muse")]);
    app.artist_recs.settle_with("a9", vec![rec("Blur")]);

    let same = app.settings.clone();
    let _ = app.handle_player_settings_loaded(same);
    assert_eq!(
        cached_names(&app, "a9"),
        Some(vec!["Blur".to_string()]),
        "same sources: the lists stay"
    );

    let mut deezer_on = app.settings.clone();
    deezer_on.similar_artists_deezer = true;
    let _ = app.handle_player_settings_loaded(deezer_on);
    assert_eq!(cached_names(&app, "a9"), None, "the old mix is dropped");
    assert!(
        app.artist_recs.is_pending(&centred),
        "the centred artist is asked again"
    );
}
