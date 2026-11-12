//! Starring and rating playlists (Navidrome 0.64+): the heart and stars on a
//! playlist row, the star and rating keys, the per-server gate, and the
//! background reload that keeps its place once the write is confirmed.

use nokkvi_data::types::ItemKind;

use crate::{
    View,
    app_message::{HotkeyMessage, Message, PlaylistsLoaderMessage},
    services::navidrome_sse::LibraryChange,
    test_helpers::*,
    views::{PlaylistsAction, PlaylistsMessage},
    widgets::view_header::SortMode,
};

fn playlists(
    range: std::ops::Range<usize>,
) -> Vec<nokkvi_data::backend::playlists::PlaylistUIViewData> {
    range
        .map(|i| make_playlist(&format!("p{i}"), &format!("Playlist {i}")))
        .collect()
}

/// A logged-in app on the Playlists view of a server that stars playlists
/// (`annotations`), holding `count` playlists.
fn playlists_app(count: usize, annotations: bool) -> crate::Nokkvi {
    let mut app = test_app();
    app.screen = crate::Screen::Home;
    app.current_view = View::Playlists;
    app.library.playlists.set_from_vec(playlists(0..count));
    app.library.counts.playlists = count;
    app.playlists_page.annotations = annotations;
    app
}

fn page_action(app: &mut crate::Nokkvi, msg: PlaylistsMessage) -> PlaylistsAction {
    let total = app.library.playlists.len();
    app.playlists_page
        .update(msg, total, &app.library.playlists)
        .1
}

// ============================================================================
// The row's heart and stars
// ============================================================================

#[test]
fn heart_click_on_a_playlist_row_stars_and_unstars_it() {
    let mut app = playlists_app(3, true);
    let action = page_action(&mut app, PlaylistsMessage::ClickToggleStar(1));
    assert!(
        matches!(
            action,
            PlaylistsAction::ToggleStar(ref id, ItemKind::Playlist, true) if id == "p1"
        ),
        "got {action:?}"
    );

    app.library.playlists[1].is_starred = true;
    let action = page_action(&mut app, PlaylistsMessage::ClickToggleStar(1));
    assert!(
        matches!(
            action,
            PlaylistsAction::ToggleStar(ref id, ItemKind::Playlist, false) if id == "p1"
        ),
        "got {action:?}"
    );
}

#[test]
fn star_click_on_a_playlist_row_rates_it_and_the_same_star_steps_down() {
    let mut app = playlists_app(3, true);
    let action = page_action(&mut app, PlaylistsMessage::ClickSetRating(2, 4));
    assert!(
        matches!(
            action,
            PlaylistsAction::SetRating(ref id, ItemKind::Playlist, 4) if id == "p2"
        ),
        "got {action:?}"
    );

    app.library.playlists[2].rating = Some(4);
    let action = page_action(&mut app, PlaylistsMessage::ClickSetRating(2, 4));
    assert!(
        matches!(
            action,
            PlaylistsAction::SetRating(ref id, ItemKind::Playlist, 3) if id == "p2"
        ),
        "the star already set steps the rating down, as on an album row; got {action:?}"
    );
}

/// A server older than 0.64 ignores the write, so a stray click does nothing.
#[test]
fn heart_and_stars_do_nothing_on_a_server_that_does_not_star_playlists() {
    let mut app = playlists_app(3, false);
    let action = page_action(&mut app, PlaylistsMessage::ClickToggleStar(1));
    assert!(matches!(action, PlaylistsAction::None), "got {action:?}");
    let action = page_action(&mut app, PlaylistsMessage::ClickSetRating(1, 5));
    assert!(matches!(action, PlaylistsAction::None), "got {action:?}");
}

/// An expanded playlist's track keeps its own heart.
#[test]
fn heart_click_on_a_track_row_still_stars_the_song() {
    let mut app = playlists_app(2, true);
    expand_playlists_with(&mut app, "p0", vec![make_song("s1", "Track", "Artist")]);
    let action = page_action(&mut app, PlaylistsMessage::ClickToggleStar(1));
    assert!(
        matches!(
            action,
            PlaylistsAction::ToggleStar(ref id, ItemKind::Song, true) if id == "s1"
        ),
        "got {action:?}"
    );
}

// ============================================================================
// Local state: the optimistic update and its revert
// ============================================================================

#[test]
fn a_playlist_star_and_its_revert_flip_the_row() {
    let mut app = playlists_app(3, true);
    let _ = app.update(crate::Nokkvi::starred_flag_message(
        "p1".into(),
        ItemKind::Playlist,
        true,
    ));
    assert!(app.library.playlists[1].is_starred);
    assert!(!app.library.playlists[0].is_starred, "sibling untouched");

    // The server refused it: `star_result_message` sends the flag back.
    let _ = app.update(crate::Nokkvi::starred_flag_message(
        "p1".into(),
        ItemKind::Playlist,
        false,
    ));
    assert!(!app.library.playlists[1].is_starred);
}

#[test]
fn a_playlist_rating_and_its_revert_set_the_row() {
    let mut app = playlists_app(3, true);
    let _ = app.update(crate::Nokkvi::rating_revert_message(
        "p2".into(),
        ItemKind::Playlist,
        4,
    ));
    assert_eq!(app.library.playlists[2].rating, Some(4));

    let _ = app.update(crate::Nokkvi::rating_revert_message(
        "p2".into(),
        ItemKind::Playlist,
        0,
    ));
    assert_eq!(app.library.playlists[2].rating, None, "0 is unrated");
}

/// A playlist id never reaches the song handler: a song that shares the id
/// string keeps its flag.
#[test]
fn a_playlist_star_leaves_songs_alone() {
    let mut app = playlists_app(1, true);
    app.library.queue_songs = vec![make_queue_song("p0", "Song", "Artist", "Album")];
    let _ = app.update(crate::Nokkvi::starred_flag_message(
        "p0".into(),
        ItemKind::Playlist,
        true,
    ));
    assert!(app.library.playlists[0].is_starred);
    assert!(!app.library.queue_songs[0].starred);
}

// ============================================================================
// The star and rating keys
// ============================================================================

#[test]
fn the_star_and_rating_keys_find_the_focused_playlist() {
    let mut app = playlists_app(3, true);
    app.library.playlists[0].is_starred = true;
    app.library.playlists[0].rating = Some(2);

    let info = app
        .get_center_item_info()
        .expect("a playlist row is rateable on 0.64+");
    assert_eq!(info.id, "p0");
    assert_eq!(info.kind, ItemKind::Playlist);
    assert!(info.starred);
    assert_eq!(info.rating, 2);
    assert!(
        app.focused_artist().is_none(),
        "a playlist names no artist for Similar Artists"
    );
}

#[test]
fn the_star_and_rating_keys_find_nothing_on_an_older_server() {
    let app = playlists_app(3, false);
    assert!(app.get_center_item_info().is_none());
    // The row is still named, for Shift+A's reply.
    assert_eq!(
        app.center_item()
            .map(crate::update::hotkeys::CenterItem::into_name)
            .as_deref(),
        Some("Playlist 0")
    );
}

// ============================================================================
// The per-server gate
// ============================================================================

#[test]
fn the_server_version_turns_playlist_favourites_on_and_off() {
    let mut app = playlists_app(3, false);
    let _ = app.update(Message::ServerVersionFetched(Some("0.64.2 (abc)".into())));
    assert!(app.playlists_page.annotations);
    assert!(
        app.playlists_page
            .sort_modes()
            .contains(&SortMode::Favorited)
    );
    assert!(app.playlists_page.sort_modes().contains(&SortMode::Rating));

    let _ = app.update(Message::ServerVersionFetched(Some("0.63.2".into())));
    assert!(!app.playlists_page.annotations);
    assert!(
        !app.playlists_page
            .sort_modes()
            .contains(&SortMode::Favorited)
    );
    assert!(!app.playlists_page.sort_modes().contains(&SortMode::Rating));
}

/// A Favorited or Rating sort kept from a 0.64 server can't be served by an
/// older one: the page goes back to Name and reloads.
#[test]
fn an_older_server_takes_a_stranded_sort_back_to_name() {
    for stranded in [SortMode::Favorited, SortMode::Rating] {
        let mut app = playlists_app(3, true);
        app.playlists_page.common.current_sort_mode = stranded;
        app.playlists_page.common.sort_ascending = false;

        let _ = app.update(Message::ServerVersionFetched(Some("0.63.2".into())));
        assert_eq!(
            app.playlists_page.common.current_sort_mode,
            SortMode::Name,
            "{stranded:?}"
        );
        assert!(app.playlists_page.common.sort_ascending);
        assert!(
            app.library.playlists.is_loading(),
            "the list loaded under {stranded:?} is reloaded"
        );
    }

    // Every other sort stays, with no reload.
    let mut app = playlists_app(3, true);
    app.playlists_page.common.current_sort_mode = SortMode::UpdatedAt;
    let _ = app.update(Message::ServerVersionFetched(Some("0.63.2".into())));
    assert_eq!(
        app.playlists_page.common.current_sort_mode,
        SortMode::UpdatedAt
    );
    assert!(!app.library.playlists.is_loading());
}

/// A 0.64 server keeps the sort the user chose.
#[test]
fn a_064_server_keeps_a_favorited_sort() {
    let mut app = playlists_app(3, false);
    app.playlists_page.common.current_sort_mode = SortMode::Favorited;
    let _ = app.update(Message::ServerVersionFetched(Some("0.64.0".into())));
    assert_eq!(
        app.playlists_page.common.current_sort_mode,
        SortMode::Favorited
    );
    assert!(!app.library.playlists.is_loading());
}

#[test]
fn logout_forgets_the_last_servers_answer() {
    let mut app = playlists_app(3, true);
    let _ = app.reset_session_state();
    assert!(!app.playlists_page.annotations);
}

#[test]
fn the_heart_and_stars_columns_draw_only_where_the_server_stars_playlists() {
    use crate::views::playlists::view::{playlists_love_visible, playlists_stars_visible};

    assert!(playlists_love_visible(true, true));
    assert!(!playlists_love_visible(false, true));
    assert!(!playlists_love_visible(true, false));

    assert!(playlists_stars_visible(SortMode::Name, true, true));
    assert!(!playlists_stars_visible(SortMode::Name, false, true));
    assert!(
        playlists_stars_visible(SortMode::Rating, false, true),
        "a Rating sort shows what it sorts by"
    );
    assert!(!playlists_stars_visible(SortMode::Rating, true, false));
}

// ============================================================================
// The reload behind a confirmed star keeps its place
// ============================================================================

fn reload_lands(
    app: &mut crate::Nokkvi,
    rows: Vec<nokkvi_data::backend::playlists::PlaylistUIViewData>,
    background: bool,
) {
    let total = rows.len();
    let _ = app.update(Message::PlaylistsLoader(PlaylistsLoaderMessage::Loaded(
        Ok(rows),
        total,
        None,
        background,
    )));
}

#[test]
fn a_background_reload_keeps_the_viewport_and_the_highlight_on_their_rows() {
    let mut app = playlists_app(40, true);
    app.playlists_page.common.slot_list.slot_count = 9;
    app.playlists_page.common.slot_list.viewport_offset = 20;
    app.playlists_page.common.slot_list.set_selected(22, 40);

    // The reload comes back in another order (a Favorited sort after a
    // star): p30 leads, everything else follows.
    let mut rows = playlists(0..40);
    let moved = rows.remove(30);
    rows.insert(0, moved);
    reload_lands(&mut app, rows, true);

    let list = &app.playlists_page.common.slot_list;
    assert_eq!(
        app.library.playlists[list.viewport_offset].id, "p20",
        "the viewport stays on its row"
    );
    assert_eq!(
        list.selected_offset
            .map(|i| app.library.playlists[i].id.as_str()),
        Some("p22"),
        "the highlight stays on its row"
    );
}

/// A load the user asked for (sort, search, refresh) still starts at the top.
#[test]
fn a_foreground_load_starts_at_the_top() {
    let mut app = playlists_app(40, true);
    app.playlists_page.common.slot_list.viewport_offset = 20;
    reload_lands(&mut app, playlists(0..40), false);
    assert_eq!(app.playlists_page.common.slot_list.viewport_offset, 0);
}

/// A row gone from the list (deleted elsewhere) leaves the viewport where it
/// was, clamped to the rows left.
#[test]
fn a_background_reload_clamps_when_the_viewport_row_is_gone() {
    let mut app = playlists_app(40, true);
    app.playlists_page.common.slot_list.viewport_offset = 39;
    reload_lands(&mut app, playlists(0..10), true);
    assert_eq!(app.playlists_page.common.slot_list.viewport_offset, 9);
}

/// The viewport sits on a track of an expanded playlist: it comes back to
/// that track, not to a playlist at the same flattened index.
#[test]
fn a_background_reload_keeps_a_viewport_inside_an_expansion() {
    let mut app = playlists_app(10, true);
    app.playlists_page.expansion.expanded_id = Some("p2".into());
    app.playlists_page.expansion.parent_offset = 2;
    app.playlists_page.expansion.children = (0..5)
        .map(|i| make_song(&format!("s{i}"), &format!("Track {i}"), "Artist"))
        .collect();
    // Flattened: p0 p1 p2 s0 s1 s2 s3 s4 p3 … ; row 5 is s2.
    app.playlists_page.common.slot_list.viewport_offset = 5;

    // p9 moves to the front, pushing everything down one.
    let mut rows = playlists(0..10);
    let moved = rows.remove(9);
    rows.insert(0, moved);
    reload_lands(&mut app, rows, true);

    assert_eq!(
        app.playlists_page.common.slot_list.viewport_offset, 6,
        "p9 p0 p1 p2 s0 s1 [s2]"
    );
}

/// nokkvi's own confirmed star or rating reloads the list only under the
/// two sorts it can reorder. Under Updated At a reload reshuffles the rows
/// (the server stamps smart playlists as it reads them), which took the row
/// just rated out from under the pointer.
#[test]
fn a_confirmed_playlist_star_or_rating_reloads_only_under_favorited_and_rating() {
    let confirmed = [
        crate::Nokkvi::star_result_message(Ok(()), "p1".into(), ItemKind::Playlist, true),
        crate::Nokkvi::star_result_message(Ok(()), "p1".into(), ItemKind::Playlist, false),
        crate::Nokkvi::rating_result_message(Ok(()), "p1".into(), ItemKind::Playlist, 0),
    ];
    for msg in &confirmed {
        assert!(
            matches!(
                msg,
                Message::Hotkey(HotkeyMessage::PlaylistAnnotated(id)) if id == "p1"
            ),
            "got {msg:?}"
        );
    }

    for (sort, reloads) in [
        (SortMode::Name, false),
        (SortMode::UpdatedAt, false),
        (SortMode::SongCount, false),
        (SortMode::Duration, false),
        (SortMode::Random, false),
        (SortMode::Favorited, true),
        (SortMode::Rating, true),
    ] {
        let mut app = playlists_app(5, true);
        app.playlists_page.common.current_sort_mode = sort;
        let _ = app.update(confirmed[0].clone());
        assert_eq!(app.library.playlists.is_loading(), reloads, "{sort:?}");
    }
}

/// Another client's playlist change still reloads the list, in the
/// background, except under Random.
#[test]
fn a_server_playlist_event_reloads_the_list_except_under_random() {
    let event = || {
        Message::LibraryChanged(LibraryChange {
            playlist_ids: vec!["p1".into()],
            ..Default::default()
        })
    };
    let mut app = playlists_app(5, true);
    app.playlists_page.common.current_sort_mode = SortMode::UpdatedAt;
    let _ = app.update(event());
    assert!(app.library.playlists.is_loading());

    let mut app = playlists_app(5, true);
    app.playlists_page.common.current_sort_mode = SortMode::Random;
    let _ = app.update(event());
    assert!(!app.library.playlists.is_loading());
}

/// The messages the optimistic update and the revert ride on.
#[test]
fn playlist_flags_route_to_the_playlist_handlers() {
    let msg = crate::Nokkvi::starred_flag_message("pl1".into(), ItemKind::Playlist, true);
    assert!(
        matches!(
            msg,
            Message::Hotkey(HotkeyMessage::PlaylistStarredStatusUpdated(ref id, true))
                if id == "pl1"
        ),
        "got {msg:?}"
    );
    let msg = crate::Nokkvi::rating_revert_message("pl1".into(), ItemKind::Playlist, 2);
    assert!(
        matches!(
            msg,
            Message::Hotkey(HotkeyMessage::PlaylistRatingUpdated(ref id, 2)) if id == "pl1"
        ),
        "got {msg:?}"
    );
}
