//! Tests for cross-view star/rating propagation
//!
//! Verifies that `handle_song_starred_status_updated()` and
//! `handle_song_rating_updated()` correctly propagate state changes
//! across all 6 parallel data lists. Historically this gap caused
//! 10 fix commits where starring/rating only updated some views.

use crate::test_helpers::*;

// ══════════════════════════════════════════════════════════════════════
//  Song Star Propagation
// ══════════════════════════════════════════════════════════════════════

#[test]
fn star_propagates_to_queue_songs() {
    let mut app = test_app();
    app.library.queue_songs = vec![
        make_queue_song("s1", "Song A", "Artist", "Album"),
        make_queue_song("s2", "Song B", "Artist", "Album"),
    ];

    let _ = app.handle_song_starred_status_updated("s1".to_string(), true);
    assert!(app.library.queue_songs[0].starred);
    assert!(!app.library.queue_songs[1].starred);
}

#[test]
fn star_propagates_to_songs_list() {
    let mut app = test_app();
    app.library
        .songs
        .set_from_vec(vec![make_song("s1", "Song A", "Artist")]);

    let _ = app.handle_song_starred_status_updated("s1".to_string(), true);
    assert!(app.library.songs[0].is_starred);
}

#[test]
fn star_propagates_to_similar_songs() {
    let mut app = test_app();
    let song = make_song("s1", "Song A", "Artist").into();
    app.similar_songs = Some(crate::state::SimilarSongsState {
        songs: vec![song],
        label: "Similar".to_string(),
        loading: false,
    });

    let _ = app.handle_song_starred_status_updated("s1".to_string(), true);
    assert!(app.similar_songs.unwrap().songs[0].starred);
}

#[test]
fn star_propagates_to_album_expansion_children() {
    let mut app = test_app();
    // Simulate expanded album with track children
    let mut track = make_song("s1", "Track 1", "Artist");
    track.is_starred = false;
    app.albums_page.expansion.children = vec![track];
    app.albums_page.expansion.expanded_id = Some("album-1".to_string());

    let _ = app.handle_song_starred_status_updated("s1".to_string(), true);
    assert!(
        app.albums_page.expansion.children[0].is_starred,
        "star must propagate to album expansion children"
    );
}

#[test]
fn star_propagates_to_playlist_expansion_children() {
    let mut app = test_app();
    let mut track = make_song("s1", "Track 1", "Artist");
    track.is_starred = false;
    app.playlists_page.expansion.children = vec![track];
    app.playlists_page.expansion.expanded_id = Some("pl-1".to_string());

    let _ = app.handle_song_starred_status_updated("s1".to_string(), true);
    assert!(
        app.playlists_page.expansion.children[0].is_starred,
        "star must propagate to playlist expansion children"
    );
}

#[test]
fn star_propagates_to_artist_sub_expansion() {
    let mut app = test_app();
    let mut track = make_song("s1", "Track 1", "Artist");
    track.is_starred = false;
    app.artists_page.sub_expansion.children = vec![track];
    app.artists_page.sub_expansion.expanded_id = Some("album-1".to_string());

    let _ = app.handle_song_starred_status_updated("s1".to_string(), true);
    assert!(
        app.artists_page.sub_expansion.children[0].is_starred,
        "star must propagate to artist sub-expansion children"
    );
}

#[test]
fn star_propagates_to_genre_sub_expansion() {
    let mut app = test_app();
    let mut track = make_song("s1", "Track 1", "Artist");
    track.is_starred = false;
    app.genres_page.sub_expansion.children = vec![track];
    app.genres_page.sub_expansion.expanded_id = Some("album-1".to_string());

    let _ = app.handle_song_starred_status_updated("s1".to_string(), true);
    assert!(
        app.genres_page.sub_expansion.children[0].is_starred,
        "star must propagate to genre sub-expansion children"
    );
}

#[test]
fn star_song_not_in_any_list_is_noop() {
    let mut app = test_app();
    app.library.queue_songs = vec![make_queue_song("s1", "Song A", "Artist", "Album")];
    app.library
        .songs
        .set_from_vec(vec![make_song("s1", "Song A", "Artist")]);

    // Star a non-existent song — should not panic or modify anything
    let _ = app.handle_song_starred_status_updated("nonexistent".to_string(), true);
    assert!(!app.library.queue_songs[0].starred);
    assert!(!app.library.songs[0].is_starred);
}

// ══════════════════════════════════════════════════════════════════════
//  Song Rating Propagation
// ══════════════════════════════════════════════════════════════════════

#[test]
fn rating_propagates_to_all_song_views() {
    let mut app = test_app();
    app.library.queue_songs = vec![make_queue_song("s1", "Song A", "Artist", "Album")];
    app.library
        .songs
        .set_from_vec(vec![make_song("s1", "Song A", "Artist")]);

    // Set up expansion children
    app.albums_page.expansion.children = vec![make_song("s1", "Song A", "Artist")];
    app.playlists_page.expansion.children = vec![make_song("s1", "Song A", "Artist")];
    app.artists_page.sub_expansion.children = vec![make_song("s1", "Song A", "Artist")];
    app.genres_page.sub_expansion.children = vec![make_song("s1", "Song A", "Artist")];

    let _ = app.handle_song_rating_updated("s1".to_string(), 4);

    // Check all 6 views
    assert_eq!(app.library.queue_songs[0].rating, Some(4), "queue");
    assert_eq!(app.library.songs[0].rating, Some(4), "songs");
    assert_eq!(
        app.albums_page.expansion.children[0].rating,
        Some(4),
        "album expansion"
    );
    assert_eq!(
        app.playlists_page.expansion.children[0].rating,
        Some(4),
        "playlist expansion"
    );
    assert_eq!(
        app.artists_page.sub_expansion.children[0].rating,
        Some(4),
        "artist sub-expansion"
    );
    assert_eq!(
        app.genres_page.sub_expansion.children[0].rating,
        Some(4),
        "genre sub-expansion"
    );
}

#[test]
fn rating_propagates_to_similar_songs() {
    let mut app = test_app();
    let song = make_song("s1", "Song A", "Artist").into();
    app.similar_songs = Some(crate::state::SimilarSongsState {
        songs: vec![song],
        label: "Similar".to_string(),
        loading: false,
    });

    let _ = app.handle_song_rating_updated("s1".to_string(), 4);

    assert_eq!(app.similar_songs.unwrap().songs[0].rating, Some(4));
}

#[test]
fn rating_zero_stored_as_none() {
    let mut app = test_app();
    let mut song = make_song("s1", "Song A", "Artist");
    song.rating = Some(3);
    app.library.songs.set_from_vec(vec![song]);

    let _ = app.handle_song_rating_updated("s1".to_string(), 0);
    assert_eq!(
        app.library.songs[0].rating, None,
        "rating=0 should be stored as None"
    );
}

// ══════════════════════════════════════════════════════════════════════
//  Album Star/Rating Propagation
// ══════════════════════════════════════════════════════════════════════

#[test]
fn album_star_propagates_to_albums_list() {
    let mut app = test_app();
    app.library
        .albums
        .set_from_vec(vec![make_album("a1", "My Album", "Artist")]);

    let _ = app.handle_album_starred_status_updated("a1".to_string(), true);
    assert!(app.library.albums[0].is_starred);
}

#[test]
fn album_star_propagates_to_artist_expansion() {
    let mut app = test_app();
    app.artists_page.expansion.children = vec![make_album("a1", "My Album", "Artist")];
    app.artists_page.expansion.expanded_id = Some("artist-1".to_string());

    let _ = app.handle_album_starred_status_updated("a1".to_string(), true);
    assert!(
        app.artists_page.expansion.children[0].is_starred,
        "album star must propagate to artist expansion"
    );
}

#[test]
fn album_star_propagates_to_genre_expansion() {
    let mut app = test_app();
    app.genres_page.expansion.children = vec![make_album("a1", "My Album", "Artist")];
    app.genres_page.expansion.expanded_id = Some("genre-1".to_string());

    let _ = app.handle_album_starred_status_updated("a1".to_string(), true);
    assert!(
        app.genres_page.expansion.children[0].is_starred,
        "album star must propagate to genre expansion"
    );
}

#[test]
fn album_rating_propagates_to_all_views() {
    let mut app = test_app();
    app.library
        .albums
        .set_from_vec(vec![make_album("a1", "My Album", "Artist")]);
    app.artists_page.expansion.children = vec![make_album("a1", "My Album", "Artist")];
    app.genres_page.expansion.children = vec![make_album("a1", "My Album", "Artist")];

    let _ = app.handle_album_rating_updated("a1".to_string(), 5);

    assert_eq!(app.library.albums[0].rating, Some(5), "albums list");
    assert_eq!(
        app.artists_page.expansion.children[0].rating,
        Some(5),
        "artist expansion"
    );
    assert_eq!(
        app.genres_page.expansion.children[0].rating,
        Some(5),
        "genre expansion"
    );
}

// ══════════════════════════════════════════════════════════════════════
//  Artist Star/Rating Propagation
// ══════════════════════════════════════════════════════════════════════

#[test]
fn artist_star_propagates_to_artists_list() {
    let mut app = test_app();
    app.library
        .artists
        .set_from_vec(vec![make_artist("ar1", "My Artist")]);

    let _ = app.handle_artist_starred_status_updated("ar1".to_string(), true);
    assert!(app.library.artists[0].is_starred);
}

#[test]
fn artist_rating_propagates_to_artists_list() {
    let mut app = test_app();
    app.library
        .artists
        .set_from_vec(vec![make_artist("ar1", "My Artist")]);

    let _ = app.handle_artist_rating_updated("ar1".to_string(), 3);
    assert_eq!(app.library.artists[0].rating, Some(3));
}

// ══════════════════════════════════════════════════════════════════════
//  Artists Background Reload: viewport/selection preservation
// ══════════════════════════════════════════════════════════════════════

#[test]
fn background_artists_reload_anchor_not_found_resets_to_zero() {
    let mut app = test_app();

    // Initial list: 5 artists, centered on "ar3" (viewport_offset = 2)
    app.library.artists.set_from_vec(vec![
        make_artist("ar1", "Alpha"),
        make_artist("ar2", "Beta"),
        make_artist("ar3", "Gamma"),
        make_artist("ar4", "Delta"),
        make_artist("ar5", "Epsilon"),
    ]);
    app.artists_page.common.slot_list.viewport_offset = 2;

    // Background reload returns a new random first page that does NOT contain "ar3"
    let new_page = vec![
        make_artist("ar6", "Zeta"),
        make_artist("ar7", "Eta"),
        make_artist("ar8", "Theta"),
    ];
    let _ = app.handle_artists_loaded(Ok(new_page), 8, true, Some("ar3".to_string()));

    assert_eq!(
        app.artists_page.common.slot_list.viewport_offset, 0,
        "anchor not found in new random page must reset viewport to 0, not leave it stale"
    );
}

#[test]
fn background_artists_reload_clears_selected_offset() {
    let mut app = test_app();

    app.library.artists.set_from_vec(vec![
        make_artist("ar1", "Alpha"),
        make_artist("ar2", "Beta"),
        make_artist("ar3", "Gamma"),
    ]);
    app.artists_page.common.slot_list.viewport_offset = 1;
    app.artists_page.common.slot_list.selected_offset = Some(1); // user had clicked "ar2"

    // Background reload: ar2 is now at a different index in the new order
    let new_page = vec![
        make_artist("ar3", "Gamma"),
        make_artist("ar1", "Alpha"),
        make_artist("ar2", "Beta"),
    ];
    let _ = app.handle_artists_loaded(Ok(new_page), 3, true, Some("ar2".to_string()));

    assert_eq!(
        app.artists_page.common.slot_list.selected_offset, None,
        "background reload must clear selected_offset — stale index maps to wrong artist after re-ordering"
    );
}

#[test]
fn artist_rating_update_does_not_move_viewport() {
    let mut app = test_app();

    app.library.artists.set_from_vec(vec![
        make_artist("ar1", "Alpha"),
        make_artist("ar2", "Beta"),
        make_artist("ar3", "Gamma"),
    ]);
    app.artists_page.common.slot_list.viewport_offset = 1; // centered on "ar2"

    let _ = app.handle_artist_rating_updated("ar2".to_string(), 4);

    assert_eq!(
        app.artists_page.common.slot_list.viewport_offset, 1,
        "in-place rating patch must not move the viewport"
    );
    assert_eq!(
        app.library.artists[1].id, "ar2",
        "rating update must not reorder the list"
    );
    assert_eq!(
        app.library.artists[1].rating,
        Some(4),
        "rating must be applied in-place"
    );
}

// ══════════════════════════════════════════════════════════════════════
//  Songs Background Reload: viewport/selection preservation
// ══════════════════════════════════════════════════════════════════════

#[test]
fn background_songs_reload_anchor_not_found_resets_to_zero() {
    let mut app = test_app();

    app.library.songs.set_from_vec(vec![
        make_song("s1", "Alpha", "Artist"),
        make_song("s2", "Beta", "Artist"),
        make_song("s3", "Gamma", "Artist"),
        make_song("s4", "Delta", "Artist"),
        make_song("s5", "Epsilon", "Artist"),
    ]);
    app.songs_page.common.slot_list.viewport_offset = 2;

    // Background reload returns a new random first page that does NOT contain "s3"
    let new_page = vec![
        make_song("s6", "Zeta", "Artist"),
        make_song("s7", "Eta", "Artist"),
        make_song("s8", "Theta", "Artist"),
    ];
    let _ = app.handle_songs_loaded(Ok(new_page), 8, true, Some("s3".to_string()));

    assert_eq!(
        app.songs_page.common.slot_list.viewport_offset, 0,
        "anchor not found in new random page must reset viewport to 0, not leave it stale"
    );
}

#[test]
fn background_songs_reload_clears_selected_offset() {
    let mut app = test_app();

    app.library.songs.set_from_vec(vec![
        make_song("s1", "Alpha", "Artist"),
        make_song("s2", "Beta", "Artist"),
        make_song("s3", "Gamma", "Artist"),
    ]);
    app.songs_page.common.slot_list.viewport_offset = 1;
    app.songs_page.common.slot_list.selected_offset = Some(1); // user had clicked "s2"

    // Background reload: s2 is now at a different index in the new order
    let new_page = vec![
        make_song("s3", "Gamma", "Artist"),
        make_song("s1", "Alpha", "Artist"),
        make_song("s2", "Beta", "Artist"),
    ];
    let _ = app.handle_songs_loaded(Ok(new_page), 3, true, Some("s2".to_string()));

    assert_eq!(
        app.songs_page.common.slot_list.selected_offset, None,
        "background reload must clear selected_offset — stale index maps to wrong song after re-ordering"
    );
}

#[test]
fn song_rating_update_does_not_move_songs_viewport() {
    let mut app = test_app();

    app.library.songs.set_from_vec(vec![
        make_song("s1", "Alpha", "Artist"),
        make_song("s2", "Beta", "Artist"),
        make_song("s3", "Gamma", "Artist"),
    ]);
    app.songs_page.common.slot_list.viewport_offset = 1; // centered on "s2"

    let _ = app.handle_song_rating_updated("s2".to_string(), 4);

    assert_eq!(
        app.songs_page.common.slot_list.viewport_offset, 1,
        "in-place rating patch must not move the viewport"
    );
    assert_eq!(
        app.library.songs[1].id, "s2",
        "rating update must not reorder the list"
    );
    assert_eq!(
        app.library.songs[1].rating,
        Some(4),
        "rating must be applied in-place"
    );
}

// ══════════════════════════════════════════════════════════════════════
//  Albums Background Reload: viewport/selection preservation
// ══════════════════════════════════════════════════════════════════════

#[test]
fn background_albums_reload_clears_selected_offset() {
    let mut app = test_app();

    app.library.albums.set_from_vec(vec![
        make_album("a1", "Alpha", "Artist"),
        make_album("a2", "Beta", "Artist"),
        make_album("a3", "Gamma", "Artist"),
    ]);
    app.albums_page.common.slot_list.viewport_offset = 1;
    app.albums_page.common.slot_list.selected_offset = Some(1); // user had clicked "a2"

    // Background reload: a2 is now at a different index in the new order
    let new_page = vec![
        make_album("a3", "Gamma", "Artist"),
        make_album("a1", "Alpha", "Artist"),
        make_album("a2", "Beta", "Artist"),
    ];
    let _ = app.handle_albums_loaded(Ok(new_page), 3, true, Some("a2".to_string()));

    assert_eq!(
        app.albums_page.common.slot_list.selected_offset, None,
        "background reload must clear selected_offset — stale index maps to wrong album after re-ordering"
    );
}

#[test]
fn album_rating_update_does_not_move_albums_viewport() {
    let mut app = test_app();

    app.library.albums.set_from_vec(vec![
        make_album("a1", "Alpha", "Artist"),
        make_album("a2", "Beta", "Artist"),
        make_album("a3", "Gamma", "Artist"),
    ]);
    app.albums_page.common.slot_list.viewport_offset = 1; // centered on "a2"

    let _ = app.handle_album_rating_updated("a2".to_string(), 4);

    assert_eq!(
        app.albums_page.common.slot_list.viewport_offset, 1,
        "in-place rating patch must not move the viewport"
    );
    assert_eq!(
        app.library.albums[1].id, "a2",
        "rating update must not reorder the list"
    );
    assert_eq!(
        app.library.albums[1].rating,
        Some(4),
        "rating must be applied in-place"
    );
}
