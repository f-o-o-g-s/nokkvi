//! Tests for cross-view star and play-count propagation update handlers.

use crate::test_helpers::*;

// ============================================================================
// Optimistic Play-Count Increment (scrobbling.rs / star_rating.rs)
// ============================================================================

#[test]
fn play_count_increment_bumps_queue_song() {
    let mut app = test_app();
    let mut song = make_queue_song("s1", "Song 1", "Artist", "Album");
    song.play_count = Some(5);
    app.library.queue_songs = vec![song, make_queue_song("s2", "Song 2", "Artist", "Album")];

    let _ = app.handle_song_play_count_incremented("s1".to_string());
    assert_eq!(app.library.queue_songs[0].play_count, Some(6));
    assert_eq!(app.library.queue_songs[1].play_count, None); // sibling unaffected
}

#[test]
fn play_count_increment_starts_from_none() {
    let mut app = test_app();
    let mut song = make_queue_song("s1", "Song 1", "Artist", "Album");
    song.play_count = None;
    app.library.queue_songs = vec![song];

    let _ = app.handle_song_play_count_incremented("s1".to_string());
    assert_eq!(app.library.queue_songs[0].play_count, Some(1));
}

#[test]
fn play_count_increment_propagates_to_songs_list() {
    let mut app = test_app();
    let mut song = make_song("s1", "Shared Song", "Artist");
    song.play_count = Some(2);
    app.library.songs.set_from_vec(vec![song]);
    let mut queue_song = make_queue_song("s1", "Shared Song", "Artist", "Album");
    queue_song.play_count = Some(2);
    app.library.queue_songs = vec![queue_song];

    let _ = app.handle_song_play_count_incremented("s1".to_string());
    assert_eq!(app.library.songs[0].play_count, Some(3));
    assert_eq!(app.library.queue_songs[0].play_count, Some(3));
}

#[test]
fn play_count_increment_propagates_to_expansion_children() {
    let mut app = test_app();
    let mut album_track = make_song("s1", "Track", "Artist");
    album_track.play_count = Some(7);
    app.albums_page.expansion.children = vec![album_track];

    let mut playlist_track = make_song("s1", "Track", "Artist");
    playlist_track.play_count = Some(7);
    app.playlists_page.expansion.children = vec![playlist_track];

    let _ = app.handle_song_play_count_incremented("s1".to_string());
    assert_eq!(app.albums_page.expansion.children[0].play_count, Some(8));
    assert_eq!(app.playlists_page.expansion.children[0].play_count, Some(8));
}

#[test]
fn play_count_increment_no_match_is_noop() {
    let mut app = test_app();
    let mut song = make_queue_song("s1", "Song 1", "Artist", "Album");
    song.play_count = Some(4);
    app.library.queue_songs = vec![song];

    let _ = app.handle_song_play_count_incremented("nonexistent".to_string());
    assert_eq!(app.library.queue_songs[0].play_count, Some(4));
}

// Cross-View Star Sync — Album-level expansion (star_rating.rs)
// ============================================================================

#[test]
fn album_starred_propagates_to_artists_expansion() {
    let mut app = test_app();
    let mut album = make_album("a1", "Expanded Album", "Artist");
    album.is_starred = false;
    app.artists_page.expansion.children = vec![album];

    let _ = app.handle_album_starred_status_updated("a1".to_string(), true);
    assert!(
        app.artists_page.expansion.children[0].is_starred,
        "artists expansion album should be starred"
    );
}

#[test]
fn album_starred_propagates_to_genres_expansion() {
    let mut app = test_app();
    let mut album = make_album("a1", "Genre Album", "Artist");
    album.is_starred = false;
    app.genres_page.expansion.children = vec![album];

    let _ = app.handle_album_starred_status_updated("a1".to_string(), true);
    assert!(
        app.genres_page.expansion.children[0].is_starred,
        "genres expansion album should be starred"
    );
}

// ============================================================================
// Starred → Auto-5-Star Side Effect (star_rating.rs)
// ============================================================================

#[test]
fn unstarring_song_does_not_touch_rating() {
    let mut app = test_app();
    let mut song = make_song("s1", "Rated Song", "Artist");
    song.is_starred = true;
    song.rating = Some(3);
    app.library.songs.set_from_vec(vec![song]);
    let mut queue_song = make_queue_song("s1", "Rated Song", "Artist", "Album");
    queue_song.starred = true;
    queue_song.rating = Some(3);
    app.library.queue_songs = vec![queue_song];

    // Unstar should NOT change the existing rating
    let _ = app.handle_song_starred_status_updated("s1".to_string(), false);
    assert!(!app.library.songs[0].is_starred);
    assert_eq!(
        app.library.songs[0].rating,
        Some(3),
        "unstarring should not change rating"
    );
    assert_eq!(
        app.library.queue_songs[0].rating,
        Some(3),
        "unstarring should not change queue rating"
    );
}

// ============================================================================
// Revert-message routing truth table
// ============================================================================

#[test]
fn starred_revert_message_routes_album_to_album_handler() {
    use crate::app_message::{HotkeyMessage, Message};
    use nokkvi_data::types::ItemKind;
    let msg = crate::Nokkvi::starred_revert_message("id1".to_string(), ItemKind::Album, true);
    assert!(
        matches!(
            msg,
            Message::Hotkey(HotkeyMessage::AlbumStarredStatusUpdated(ref id, true))
            if id == "id1"
        ),
        "Album kind must route to AlbumStarredStatusUpdated"
    );
}

#[test]
fn starred_revert_message_routes_artist_to_artist_handler() {
    use crate::app_message::{HotkeyMessage, Message};
    use nokkvi_data::types::ItemKind;
    let msg = crate::Nokkvi::starred_revert_message("id2".to_string(), ItemKind::Artist, false);
    assert!(
        matches!(
            msg,
            Message::Hotkey(HotkeyMessage::ArtistStarredStatusUpdated(ref id, false))
            if id == "id2"
        ),
        "Artist kind must route to ArtistStarredStatusUpdated"
    );
}

#[test]
fn starred_revert_message_routes_song_to_song_handler() {
    use crate::app_message::{HotkeyMessage, Message};
    use nokkvi_data::types::ItemKind;
    let msg = crate::Nokkvi::starred_revert_message("s1".to_string(), ItemKind::Song, true);
    assert!(
        matches!(
            msg,
            Message::Hotkey(HotkeyMessage::SongStarredStatusUpdated(ref id, true))
            if id == "s1"
        ),
        "Song kind must route to SongStarredStatusUpdated"
    );
}

#[test]
fn starred_revert_message_routes_playlist_through_song_handler_for_now() {
    use crate::app_message::{HotkeyMessage, Message};
    use nokkvi_data::types::ItemKind;
    let msg = crate::Nokkvi::starred_revert_message("pl1".to_string(), ItemKind::Playlist, true);
    assert!(
        matches!(
            msg,
            Message::Hotkey(HotkeyMessage::SongStarredStatusUpdated(ref id, true))
            if id == "pl1"
        ),
        "Playlist kind must collapse into SongStarredStatusUpdated until playlist starring ships"
    );
}

#[test]
fn rating_revert_message_routes_album_to_album_handler() {
    use crate::app_message::{HotkeyMessage, Message};
    use nokkvi_data::types::ItemKind;
    let msg = crate::Nokkvi::rating_revert_message("a1".to_string(), ItemKind::Album, 4);
    assert!(
        matches!(
            msg,
            Message::Hotkey(HotkeyMessage::AlbumRatingUpdated(ref id, 4))
            if id == "a1"
        ),
        "Album kind must route to AlbumRatingUpdated"
    );
}

#[test]
fn rating_revert_message_routes_artist_to_artist_handler() {
    use crate::app_message::{HotkeyMessage, Message};
    use nokkvi_data::types::ItemKind;
    let msg = crate::Nokkvi::rating_revert_message("ar1".to_string(), ItemKind::Artist, 3);
    assert!(
        matches!(
            msg,
            Message::Hotkey(HotkeyMessage::ArtistRatingUpdated(ref id, 3))
            if id == "ar1"
        ),
        "Artist kind must route to ArtistRatingUpdated"
    );
}

#[test]
fn rating_revert_message_routes_song_to_song_handler() {
    use crate::app_message::{HotkeyMessage, Message};
    use nokkvi_data::types::ItemKind;
    let msg = crate::Nokkvi::rating_revert_message("s1".to_string(), ItemKind::Song, 5);
    assert!(
        matches!(
            msg,
            Message::Hotkey(HotkeyMessage::SongRatingUpdated(ref id, 5))
            if id == "s1"
        ),
        "Song kind must route to SongRatingUpdated"
    );
}

#[test]
fn rating_revert_message_routes_playlist_through_song_handler_for_now() {
    use crate::app_message::{HotkeyMessage, Message};
    use nokkvi_data::types::ItemKind;
    let msg = crate::Nokkvi::rating_revert_message("pl1".to_string(), ItemKind::Playlist, 2);
    assert!(
        matches!(
            msg,
            Message::Hotkey(HotkeyMessage::SongRatingUpdated(ref id, 2))
            if id == "pl1"
        ),
        "Playlist kind must collapse into SongRatingUpdated until playlist rating ships"
    );
}

// ============================================================================
