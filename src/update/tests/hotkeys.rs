//! Tests for starred-status and rating hotkey update handlers.

use crate::test_helpers::*;

// ============================================================================
// Starred Status Handlers (hotkeys.rs)
// ============================================================================

#[test]
fn song_starred_status_updated_in_queue() {
    let mut app = test_app();
    let mut song = make_queue_song("s1", "Song 1", "Artist", "Album");
    song.starred = false;
    app.library.queue_songs = vec![song, make_queue_song("s2", "Song 2", "Artist", "Album")];

    let _ = app.handle_song_starred_status_updated("s1".to_string(), true);
    assert!(app.library.queue_songs[0].starred);
    assert!(!app.library.queue_songs[1].starred); // other song unaffected
}

#[test]
fn song_starred_status_no_match_is_noop() {
    let mut app = test_app();
    app.library.queue_songs = vec![make_queue_song("s1", "Song 1", "Artist", "Album")];

    let _ = app.handle_song_starred_status_updated("nonexistent".to_string(), true);
    assert!(!app.library.queue_songs[0].starred); // unchanged
}

#[test]
fn album_starred_status_updated() {
    let mut app = test_app();
    app.library.albums.set_from_vec(vec![
        make_album("a1", "Album 1", "Artist"),
        make_album("a2", "Album 2", "Artist"),
    ]);

    let _ = app.handle_album_starred_status_updated("a2".to_string(), true);
    assert!(!app.library.albums[0].is_starred);
    assert!(app.library.albums[1].is_starred);
}

#[test]
fn artist_starred_status_updated() {
    let mut app = test_app();
    app.library.artists.set_from_vec(vec![
        make_artist("ar1", "Artist 1"),
        make_artist("ar2", "Artist 2"),
    ]);

    let _ = app.handle_artist_starred_status_updated("ar1".to_string(), true);
    assert!(app.library.artists[0].is_starred);
    assert!(!app.library.artists[1].is_starred);
}

#[test]
fn song_starred_from_songs_view_updates_both_lists() {
    let mut app = test_app();
    // Same song ID in both songs list and queue
    app.library
        .songs
        .set_from_vec(vec![make_song("s1", "Shared Song", "Artist")]);
    let mut queue_song = make_queue_song("s1", "Shared Song", "Artist", "Album");
    queue_song.starred = false;
    app.library.queue_songs = vec![queue_song];

    let _ = app.handle_song_starred_status_updated("s1".to_string(), true);
    assert!(
        app.library.songs[0].is_starred,
        "songs list should be updated"
    );
    assert!(
        app.library.queue_songs[0].starred,
        "queue should also be updated"
    );
}

#[test]
fn song_starred_from_songs_view_only_in_songs() {
    let mut app = test_app();
    app.library
        .songs
        .set_from_vec(vec![make_song("s1", "Only In Songs", "Artist")]);
    app.library.queue_songs = vec![]; // not in queue

    let _ = app.handle_song_starred_status_updated("s1".to_string(), true);
    assert!(app.library.songs[0].is_starred);
}

// ============================================================================
// Cross-View Rating Propagation (hotkeys.rs)
// ============================================================================

#[test]
fn song_rating_updated_propagates_to_playlist_expansion_children() {
    let mut app = test_app();
    // Set up a song in the playlist expansion children
    let mut track = make_song("s1", "Playlist Track", "Artist");
    track.rating = None;
    app.playlists_page.expansion.children = vec![track];

    let _ = app.handle_song_rating_updated("s1".to_string(), 4);
    assert_eq!(app.playlists_page.expansion.children[0].rating, Some(4));
}

#[test]
fn song_rating_updated_propagates_to_albums_expansion_children() {
    let mut app = test_app();
    let mut track = make_song("s1", "Album Track", "Artist");
    track.rating = None;
    app.albums_page.expansion.children = vec![track];

    let _ = app.handle_song_rating_updated("s1".to_string(), 3);
    assert_eq!(app.albums_page.expansion.children[0].rating, Some(3));
}

#[test]
fn album_rating_updated_propagates_to_artists_expansion_children() {
    let mut app = test_app();
    let mut album = make_album("a1", "Expanded Album", "Artist");
    album.rating = None;
    app.artists_page.expansion.children = vec![album];

    let _ = app.handle_album_rating_updated("a1".to_string(), 5);
    assert_eq!(app.artists_page.expansion.children[0].rating, Some(5));
}

#[test]
fn album_rating_updated_propagates_to_genres_expansion_children() {
    let mut app = test_app();
    let mut album = make_album("a1", "Genre Album", "Artist");
    album.rating = None;
    app.genres_page.expansion.children = vec![album];

    let _ = app.handle_album_rating_updated("a1".to_string(), 2);
    assert_eq!(app.genres_page.expansion.children[0].rating, Some(2));
}

#[test]
fn song_rating_zero_clears_rating_everywhere() {
    let mut app = test_app();
    // Song in multiple locations with existing rating
    let mut song = make_song("s1", "Rated Song", "Artist");
    song.rating = Some(3);
    app.library.songs.set_from_vec(vec![song.clone()]);
    let mut queue_song = make_queue_song("s1", "Rated Song", "Artist", "Album");
    queue_song.rating = Some(3);
    app.library.queue_songs = vec![queue_song];
    let mut track = make_song("s1", "Rated Song", "Artist");
    track.rating = Some(3);
    app.playlists_page.expansion.children = vec![track];

    let _ = app.handle_song_rating_updated("s1".to_string(), 0);
    assert_eq!(app.library.songs[0].rating, None);
    assert_eq!(app.library.queue_songs[0].rating, None);
    assert_eq!(app.playlists_page.expansion.children[0].rating, None);
}
