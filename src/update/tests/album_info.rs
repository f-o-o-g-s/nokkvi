//! The Albums view asks the server for the centred album's info
//! (`getAlbumInfo2`) once that album has stayed centred, once per album, and
//! keeps each answer under its own id. Get Info shows what was fetched.

use nokkvi_data::types::{album::AlbumInfo, error::NokkviError, info_modal::InfoModalItem};

use crate::{
    Nokkvi, View,
    app_message::{AlbumsLoaderMessage, Message, NavigationMessage},
    test_helpers::*,
    views::AlbumsMessage,
    widgets::{SlotListPageMessage, info_modal::InfoModalMessage},
};

/// An app on the Albums view, its panel showing text (Text Overlay On
/// Artwork, on by default in the app; a bare test app has every setting
/// zeroed), with `count` albums.
fn app_with_albums(count: usize) -> Nokkvi {
    let mut app = test_app();
    app.current_view = View::Albums;
    app.settings.albums_artwork_overlay = true;
    seed_albums(&mut app, albums_indexed(count));
    app
}

/// The same app with the Queue in front and the Albums list not loaded yet.
fn app_on_the_queue() -> Nokkvi {
    let mut app = app_with_albums(0);
    app.current_view = View::Queue;
    app
}

/// The Albums list lands, as at login or on a reload: the load's centre hook
/// runs. Returns the album it centred.
fn load_albums(app: &mut Nokkvi, count: usize) -> String {
    let _ = app.handle_albums_loaded(Ok(albums_indexed(count)), count, false, None, None);
    app.centred_album_id().expect("the load centred an album")
}

fn slot(app: &mut Nokkvi, msg: SlotListPageMessage) {
    let _ = app.update(Message::Albums(AlbumsMessage::SlotList(msg)));
}

/// Move the centre one row, returning the album now centred.
fn navigate(app: &mut Nokkvi, msg: SlotListPageMessage) -> String {
    slot(app, msg);
    app.centred_album_id().expect("an album is centred")
}

/// The settle timer for `id` fires.
fn settle(app: &mut Nokkvi, id: &str) {
    let _ = app.update(Message::AlbumsLoader(AlbumsLoaderMessage::InfoSettled(
        id.to_string(),
    )));
}

fn land(app: &mut Nokkvi, generation: u64, id: &str, result: Result<AlbumInfo, String>) {
    let _ = app.update(Message::AlbumsLoader(AlbumsLoaderMessage::InfoLoaded {
        generation,
        album_id: id.to_string(),
        result,
    }));
}

fn notes(text: &str) -> AlbumInfo {
    AlbumInfo {
        notes: Some(text.to_string()),
        external_url: Some("https://www.last.fm/music/x/y".to_string()),
    }
}

fn cached_notes<'a>(app: &'a Nokkvi, id: &str) -> Option<&'a str> {
    app.album_info
        .cache
        .snapshot
        .get(id)
        .and_then(|info| info.notes.as_deref())
}

/// The rows the open Get Info modal shows, by label.
fn info_row(app: &Nokkvi, label: &str) -> Option<String> {
    app.info_modal
        .cached_properties
        .iter()
        .find(|(l, _)| l == label)
        .map(|(_, value)| value.clone())
}

fn open_get_info(app: &mut Nokkvi, id: &str) {
    let album = make_album(id, "An Album", "Artist");
    let _ = app.update(Message::InfoModal(InfoModalMessage::Open(Box::new(
        InfoModalItem::from_album_view_data(&album, None),
    ))));
}

#[test]
fn an_album_that_stays_centred_is_asked_about_once() {
    let mut app = app_with_albums(20);
    let generation = app.album_info.session_generation();

    let first = navigate(&mut app, SlotListPageMessage::NavigateDown);
    assert!(
        !app.album_info.is_pending(&first),
        "nothing is asked before the centre settles"
    );
    settle(&mut app, &first);
    assert!(app.album_info.is_pending(&first), "asked once settled");

    land(&mut app, generation, &first, Ok(notes("first notes")));
    assert!(!app.album_info.is_pending(&first));
    assert_eq!(cached_notes(&app, &first), Some("first notes"));

    // Away and back: the cached album is not asked about again.
    navigate(&mut app, SlotListPageMessage::NavigateDown);
    let back = navigate(&mut app, SlotListPageMessage::NavigateUp);
    assert_eq!(back, first);
    settle(&mut app, &first);
    assert!(
        !app.album_info.is_pending(&first),
        "a cached album is not asked about again"
    );
}

/// A held arrow key scrolls past rows: only the album it stops on is asked
/// about, since a first ask makes the server run its agents.
#[test]
fn scrolling_past_albums_asks_only_about_the_one_it_stops_on() {
    let mut app = app_with_albums(20);
    let passed: Vec<String> = (0..4)
        .map(|_| navigate(&mut app, SlotListPageMessage::NavigateDown))
        .collect();
    let stopped_on = passed.last().expect("moved").clone();
    for id in &passed {
        settle(&mut app, id);
    }
    for id in &passed[..passed.len() - 1] {
        assert!(!app.album_info.is_pending(id), "{id} was scrolled past");
    }
    assert!(app.album_info.is_pending(&stopped_on));
}

/// A click on a row off the viewport centre highlights it in place (stable
/// viewport); the panel shows that album, so that is the one asked about.
#[test]
fn a_clicked_row_off_centre_is_the_album_asked_about() {
    let mut app = app_with_albums(20);
    slot(&mut app, SlotListPageMessage::NavigateDown);
    let viewport_centre = app
        .albums_page
        .common
        .get_center_item_index(app.library.albums.len())
        .expect("a viewport centre");
    let clicked = viewport_centre + 2;
    slot(
        &mut app,
        SlotListPageMessage::SetOffset(clicked, iced::keyboard::Modifiers::default()),
    );
    let id = format!("a{clicked}");
    assert_eq!(app.centred_album_id().as_deref(), Some(id.as_str()));
    settle(&mut app, &id);
    assert!(app.album_info.is_pending(&id));
}

/// With an album expanded, a centred track row shows that album's panel, so
/// a step onto its tracks asks about the album, not about whichever album
/// sits at the same position in the plain list.
#[test]
fn a_track_row_of_an_expanded_album_asks_about_that_album() {
    let mut app = app_with_albums(20);
    expand_albums_with(
        &mut app,
        "a0",
        (0..6)
            .map(|i| make_song(&format!("s{i}"), &format!("Song {i}"), "Artist"))
            .collect(),
    );
    for _ in 0..3 {
        slot(&mut app, SlotListPageMessage::NavigateDown);
    }
    assert_eq!(app.centred_album_id().as_deref(), Some("a0"));
    settle(&mut app, "a0");
    assert!(app.album_info.is_pending("a0"));
}

/// A result that lands after the user moved on is cached under the album it
/// was asked for, never shown for the album centred now.
#[test]
fn a_result_for_an_album_no_longer_centred_stays_with_its_own_album() {
    let mut app = app_with_albums(20);
    let generation = app.album_info.session_generation();
    let first = navigate(&mut app, SlotListPageMessage::NavigateDown);
    settle(&mut app, &first);
    let now = navigate(&mut app, SlotListPageMessage::NavigateDown);
    settle(&mut app, &now);
    assert_ne!(first, now);

    land(&mut app, generation, &first, Ok(notes("first notes")));
    assert!(app.album_info.cache.snapshot.contains_key(&first));
    assert!(
        !app.album_info.cache.snapshot.contains_key(&now),
        "the late result is not filed under the centred album"
    );
    assert!(app.album_info.is_pending(&now), "its own ask is still out");
}

#[test]
fn a_result_from_before_logout_is_dropped() {
    let _lock = super::sse_slot_guard();
    let mut app = app_with_albums(20);
    let before = app.album_info.session_generation();
    let first = navigate(&mut app, SlotListPageMessage::NavigateDown);
    settle(&mut app, &first);
    assert!(app.album_info.is_pending(&first));

    let _ = app.reset_session_state();
    assert!(!app.album_info.is_pending(&first), "logout forgets the ask");

    land(&mut app, before, &first, Ok(notes("old server")));
    assert!(
        app.album_info.cache.is_empty(),
        "a pre-logout answer must not reach the next session"
    );
}

/// Logout also forgets what was fetched: the next server's albums can share
/// an id with the old one's.
#[test]
fn logout_forgets_the_fetched_notes() {
    let _lock = super::sse_slot_guard();
    let mut app = app_with_albums(20);
    let generation = app.album_info.session_generation();
    let first = navigate(&mut app, SlotListPageMessage::NavigateDown);
    settle(&mut app, &first);
    land(&mut app, generation, &first, Ok(notes("old server")));
    assert_eq!(cached_notes(&app, &first), Some("old server"));

    let _ = app.reset_session_state();
    assert!(app.album_info.cache.is_empty());
}

#[test]
fn a_failed_fetch_is_asked_again_on_the_next_centring() {
    let mut app = app_with_albums(20);
    let generation = app.album_info.session_generation();
    let first = navigate(&mut app, SlotListPageMessage::NavigateDown);
    settle(&mut app, &first);
    land(&mut app, generation, &first, Err("HTTP 500".to_string()));
    assert!(!app.album_info.is_pending(&first));
    assert!(app.album_info.cache.is_empty(), "a failure is not cached");

    navigate(&mut app, SlotListPageMessage::NavigateDown);
    let back = navigate(&mut app, SlotListPageMessage::NavigateUp);
    assert_eq!(back, first);
    settle(&mut app, &first);
    assert!(app.album_info.is_pending(&first), "asked again");
}

/// An album the agents know nothing about is remembered as such, so it is
/// not asked about on every return to it.
#[test]
fn an_album_without_notes_is_remembered_and_not_asked_again() {
    let mut app = app_with_albums(20);
    let generation = app.album_info.session_generation();
    let first = navigate(&mut app, SlotListPageMessage::NavigateDown);
    settle(&mut app, &first);
    land(&mut app, generation, &first, Ok(AlbumInfo::default()));

    navigate(&mut app, SlotListPageMessage::NavigateDown);
    navigate(&mut app, SlotListPageMessage::NavigateUp);
    settle(&mut app, &first);
    assert!(!app.album_info.is_pending(&first));
}

/// A "N songs" click or a link that finds and expands an album skips the
/// scroll-driven trigger, so the expand path starts the settle too.
#[test]
fn expanding_an_album_asks_for_its_info() {
    let mut app = app_with_albums(2);
    let _ = app.handle_albums(AlbumsMessage::FocusAndExpand(1));
    settle(&mut app, "a1");
    assert!(app.album_info.is_pending("a1"));
}

/// A rejected session routes to the login screen, never to a quiet "no notes".
#[test]
fn a_rejected_session_routes_to_login() {
    let msg = Nokkvi::album_info_result_message(
        0,
        "a1".to_string(),
        Err(NokkviError::Unauthorized.into()),
    );
    assert!(matches!(msg, Message::SessionExpired), "got: {msg:?}");

    let msg = Nokkvi::album_info_result_message(0, "a1".to_string(), Err(anyhow::anyhow!("boom")));
    assert!(
        matches!(
            msg,
            Message::AlbumsLoader(AlbumsLoaderMessage::InfoLoaded { result: Err(_), .. })
        ),
        "got: {msg:?}"
    );
}

// ---------------------------------------------------------------------------
// Only an album on screen is asked about
// ---------------------------------------------------------------------------

/// The Albums list loads at login behind the start view and reloads in the
/// background under any view. An ask makes the server run its agents and
/// tell every client, so an album nobody is looking at is not asked about:
/// its load starts no timer, so none can fire once the list is on screen.
#[test]
fn an_album_centred_by_a_load_behind_another_view_is_not_asked_about() {
    let mut app = app_on_the_queue();
    let centred = load_albums(&mut app, 20);
    app.current_view = View::Albums;
    settle(&mut app, &centred);
    assert!(!app.album_info.is_pending(&centred));
}

/// Opening the Albums view shows its centred album's panel, so that album is
/// asked about without a row step.
#[test]
fn opening_the_albums_view_asks_about_its_centred_album() {
    let mut app = app_on_the_queue();
    let centred = load_albums(&mut app, 20);

    let _ = app.update(Message::Navigation(NavigationMessage::SwitchView(
        View::Albums,
    )));
    assert_eq!(app.current_view, View::Albums);
    settle(&mut app, &centred);
    assert!(app.album_info.is_pending(&centred));
}

/// So does switching the split view's browsing pane to its Albums tab: the
/// list is loaded already, so nothing else centres it.
#[test]
fn switching_the_browsing_pane_to_albums_asks_about_its_centred_album() {
    let mut app = app_on_the_queue();
    let centred = load_albums(&mut app, 20);
    let mut panel = crate::views::BrowsingPanel::new();
    panel.active_view = crate::views::BrowsingView::Songs;
    app.browsing_panel = Some(panel);
    assert!(app.split_view_active());

    let _ = app.update(Message::BrowsingPanel(
        crate::views::BrowsingPanelMessage::SwitchView(crate::views::BrowsingView::Albums),
    ));
    settle(&mut app, &centred);
    assert!(app.album_info.is_pending(&centred));
}

/// A roulette spin sweeps past dozens of albums. A reload landing mid-spin
/// centres whichever is under the wheel, and a timer started just before the
/// spin fires during it: neither asks, or each answer's broadcast would
/// reload the list and ask about the next album under the wheel.
#[test]
fn a_spin_asks_about_no_album() {
    let mut app = app_with_albums(20);
    let before_spin = navigate(&mut app, SlotListPageMessage::NavigateDown);
    let _ = app.handle_roulette_message(crate::app_message::RouletteMessage::Start(View::Albums));
    assert!(app.roulette.is_some());

    settle(&mut app, &before_spin);
    assert!(
        !app.album_info.is_pending(&before_spin),
        "a timer from before the spin asks nothing"
    );

    let under_the_wheel = load_albums(&mut app, 20);
    settle(&mut app, &under_the_wheel);
    assert!(
        !app.album_info.is_pending(&under_the_wheel),
        "a reload mid-spin asks nothing"
    );
}

/// A find-and-expand chain pages through the list to its target with row 0
/// centred. Asking about row 0 in passing would make the server broadcast a
/// reload that throws away the pages the chain has loaded.
#[test]
fn a_find_and_expand_chain_asks_about_no_album_in_passing() {
    let mut app = app_with_albums(0);
    arm_pending_album(&mut app, "a400");
    // The first page of 500: the target is further down, so the chain goes on.
    let _ = app.handle_albums_loaded(Ok(albums_indexed(20)), 500, false, None, None);
    let row_zero = app.centred_album_id().expect("the load centred an album");
    assert!(app.pending_expand.target.is_some(), "the chain is running");
    settle(&mut app, &row_zero);
    assert!(!app.album_info.is_pending(&row_zero));
}

/// Theater Mode hides the list (the current view stays Albums behind it).
#[test]
fn theater_mode_asks_about_no_album() {
    let mut app = app_with_albums(0);
    app.theater.active = true;
    let centred = load_albums(&mut app, 20);
    settle(&mut app, &centred);
    assert!(!app.album_info.is_pending(&centred));
}

/// With Text Overlay On Artwork off for Albums the panel has no text, so
/// there is nowhere for the notes to show and nothing is asked.
#[test]
fn albums_without_the_text_overlay_ask_about_no_album() {
    let mut app = app_with_albums(20);
    app.settings.albums_artwork_overlay = false;
    let centred = navigate(&mut app, SlotListPageMessage::NavigateDown);
    settle(&mut app, &centred);
    assert!(!app.album_info.is_pending(&centred));
}

/// A find-and-expand chain that ends (here: called off) leaves the list on
/// whatever album it centred; that album is asked about then.
#[test]
fn the_album_centred_when_a_find_chain_ends_is_asked_about() {
    let mut app = app_with_albums(0);
    arm_pending_album(&mut app, "a400");
    let _ = app.handle_albums_loaded(Ok(albums_indexed(20)), 500, false, None, None);
    let centred = app.centred_album_id().expect("the load centred an album");

    // A search edit supersedes the chain.
    let _ = app.update(Message::Albums(AlbumsMessage::SearchQueryChanged(
        "a".to_string(),
    )));
    assert!(app.pending_expand.target.is_none(), "the chain is over");
    settle(&mut app, &centred);
    assert!(app.album_info.is_pending(&centred));
}

/// The same load under the Albums view asks about the album it centred.
#[test]
fn a_load_under_the_albums_view_asks_about_the_album_it_centred() {
    let mut app = app_with_albums(0);
    let centred = load_albums(&mut app, 20);
    settle(&mut app, &centred);
    assert!(app.album_info.is_pending(&centred));
}

/// The split view's Albums tab is on screen beside the Queue: a step there
/// asks, though the current view is the Queue.
#[test]
fn a_step_in_the_browsing_panes_albums_tab_asks() {
    let mut app = app_with_albums(20);
    app.current_view = View::Queue;
    let mut panel = crate::views::BrowsingPanel::new();
    panel.active_view = crate::views::BrowsingView::Albums;
    app.browsing_panel = Some(panel);
    assert!(app.split_view_active());

    let centred = navigate(&mut app, SlotListPageMessage::NavigateDown);
    settle(&mut app, &centred);
    assert!(app.album_info.is_pending(&centred));
}

/// An album centred just before the user left Albums is not asked about
/// once they are gone; coming back centres it again.
#[test]
fn leaving_albums_before_the_centre_settles_asks_nothing() {
    let mut app = app_with_albums(20);
    let centred = navigate(&mut app, SlotListPageMessage::NavigateDown);
    app.current_view = View::Queue;
    settle(&mut app, &centred);
    assert!(!app.album_info.is_pending(&centred));
}

// ---------------------------------------------------------------------------
// Get Info
// ---------------------------------------------------------------------------

/// Get Info is built from the album row, which Navidrome fills only on a
/// later reload: until then the notes fetched this session stand in.
#[test]
fn get_info_shows_the_notes_fetched_this_session() {
    let mut app = app_with_albums(20);
    let generation = app.album_info.session_generation();
    let first = navigate(&mut app, SlotListPageMessage::NavigateDown);
    settle(&mut app, &first);
    land(&mut app, generation, &first, Ok(notes("fetched notes")));

    open_get_info(&mut app, &first);
    assert_eq!(info_row(&app, "Notes").as_deref(), Some("fetched notes"));
    assert_eq!(
        info_row(&app, "External URL").as_deref(),
        Some("https://www.last.fm/music/x/y")
    );

    // An album nothing was fetched for has neither row.
    open_get_info(&mut app, "a15");
    assert_eq!(info_row(&app, "Notes"), None);
    assert_eq!(info_row(&app, "External URL"), None);
}

/// Shift+I pressed right after landing on an album opens Get Info before the
/// answer is back; the rows appear when it lands.
#[test]
fn an_answer_landing_under_an_open_get_info_fills_it() {
    let mut app = app_with_albums(20);
    let generation = app.album_info.session_generation();
    let first = navigate(&mut app, SlotListPageMessage::NavigateDown);
    settle(&mut app, &first);
    open_get_info(&mut app, &first);
    assert_eq!(info_row(&app, "Notes"), None);

    land(&mut app, generation, &first, Ok(notes("landed notes")));
    assert_eq!(info_row(&app, "Notes").as_deref(), Some("landed notes"));
    assert!(app.info_modal.visible);
}

/// An answer for another album leaves the open modal as it is, the rows the
/// user expanded included.
#[test]
fn an_answer_for_another_album_leaves_the_open_get_info_alone() {
    let mut app = app_with_albums(20);
    let generation = app.album_info.session_generation();
    let first = navigate(&mut app, SlotListPageMessage::NavigateDown);
    settle(&mut app, &first);
    open_get_info(&mut app, "a15");
    app.info_modal.expanded_rows.insert(1);

    land(&mut app, generation, &first, Ok(notes("landed notes")));
    assert_eq!(info_row(&app, "Notes"), None);
    assert!(app.info_modal.expanded_rows.contains(&1));
}

/// So does an answer for the open album that adds nothing: most albums have
/// no notes, and the modal already shows whatever the row had. Its rows are
/// not compared as text: "Last Played: 5 minutes ago" reads differently a
/// minute later with nothing new to show (here a row is aged by hand).
#[test]
fn an_answer_that_adds_nothing_leaves_the_open_get_info_alone() {
    let mut app = app_with_albums(20);
    let generation = app.album_info.session_generation();
    let first = navigate(&mut app, SlotListPageMessage::NavigateDown);
    settle(&mut app, &first);
    open_get_info(&mut app, &first);
    app.info_modal.expanded_rows.insert(1);
    app.info_modal.cached_properties[0].1 = "as it read a minute ago".to_string();

    land(&mut app, generation, &first, Ok(AlbumInfo::default()));
    assert!(app.info_modal.expanded_rows.contains(&1));
}
