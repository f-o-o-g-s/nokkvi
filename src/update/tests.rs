//! Tests for update handlers
//!
//! Covers pure-state-mutation handlers that don't require app_service or async.

use crate::{View, app_message::PlaybackStateUpdate, test_helpers::*};

// ============================================================================
// Mode Flag Handlers (playback.rs)
// ============================================================================

#[test]
fn random_toggled_sets_flag() {
    let mut app = test_app();
    assert!(!app.modes.random);

    let _ = app.handle_random_toggled(true);
    assert!(app.modes.random);

    let _ = app.handle_random_toggled(false);
    assert!(!app.modes.random);
}

#[test]
fn repeat_toggled_sets_both_flags() {
    let mut app = test_app();
    assert!(!app.modes.repeat);
    assert!(!app.modes.repeat_queue);

    let _ = app.handle_repeat_toggled(true, false);
    assert!(app.modes.repeat);
    assert!(!app.modes.repeat_queue);

    let _ = app.handle_repeat_toggled(true, true);
    assert!(app.modes.repeat);
    assert!(app.modes.repeat_queue);

    let _ = app.handle_repeat_toggled(false, false);
    assert!(!app.modes.repeat);
    assert!(!app.modes.repeat_queue);
}

#[test]
fn consume_toggled_sets_flag() {
    let mut app = test_app();
    assert!(!app.modes.consume);

    let _ = app.handle_consume_toggled(true);
    assert!(app.modes.consume);

    let _ = app.handle_consume_toggled(false);
    assert!(!app.modes.consume);
}

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

// ============================================================================
// Playback State Machine (playback.rs)
// ============================================================================

fn make_playback_update() -> PlaybackStateUpdate {
    PlaybackStateUpdate {
        position: 42,
        duration: 200,
        playing: true,
        paused: false,
        title: "Test Song".to_string(),
        artist: "Test Artist".to_string(),
        album: "Test Album".to_string(),
        art_url: None,
        random: true,
        repeat: false,
        repeat_queue: false,
        consume: false,
        current_index: Some(0),
        song_id: Some("song_1".to_string()),
        format_suffix: "flac".to_string(),
        sample_rate: 44100,
        bitrate: 1411,
        live_icy_metadata: None,
    }
}

#[test]
fn playback_state_updated_maps_fields() {
    let mut app = test_app();
    let update = make_playback_update();

    let _ = app.handle_playback_state_updated(update);

    assert_eq!(app.playback.position, 42);
    assert_eq!(app.playback.duration, 200);
    assert!(app.playback.playing);
    assert!(!app.playback.paused);
    assert_eq!(app.playback.title, "Test Song");
    assert_eq!(app.playback.artist, "Test Artist");
    assert_eq!(app.playback.album, "Test Album");
    assert_eq!(app.playback.format_suffix, "flac");
    assert_eq!(app.playback.sample_rate, 44100);
    assert!(app.modes.random);
    assert!(!app.modes.repeat);
}

#[test]
fn playback_state_updated_detects_song_change() {
    let mut app = test_app();
    // Simulate first song playing
    app.scrobble.current_song_id = Some("old_song".to_string());
    app.scrobble.listening_time = 10.0;

    let update = make_playback_update(); // song_id = "song_1" (different)
    let _ = app.handle_playback_state_updated(update);

    // Scrobble state should be reset for new song
    assert_eq!(app.scrobble.current_song_id.as_deref(), Some("song_1"));
    assert_eq!(app.scrobble.listening_time, 0.0);
    assert!(!app.scrobble.submitted);
}

#[test]
fn playback_state_updated_same_song_no_reset() {
    let mut app = test_app();
    app.scrobble.current_song_id = Some("song_1".to_string());
    app.scrobble.listening_time = 50.0;
    app.scrobble.last_position = 50.0;

    let mut update = make_playback_update();
    update.position = 55;
    update.song_id = Some("song_1".to_string()); // same song
    let _ = app.handle_playback_state_updated(update);

    // Listening time should accumulate, not reset
    assert!(app.scrobble.listening_time > 50.0);
}

#[test]
fn playback_state_tracks_listening_time_forward() {
    let mut app = test_app();
    app.scrobble.current_song_id = Some("song_1".to_string());
    app.scrobble.last_position = 10.0;
    app.scrobble.listening_time = 0.0;

    let mut update = make_playback_update();
    update.position = 15; // 5 second forward delta
    update.song_id = Some("song_1".to_string());
    let _ = app.handle_playback_state_updated(update);

    assert!((app.scrobble.listening_time - 5.0).abs() < 0.1);
    assert_eq!(app.scrobble.last_position, 15.0);
}

#[test]
fn playback_state_ignores_seek_for_listening_time() {
    let mut app = test_app();
    app.scrobble.current_song_id = Some("song_1".to_string());
    app.scrobble.last_position = 10.0;
    app.scrobble.listening_time = 5.0;

    // Big jump = seek, should not count
    let mut update = make_playback_update();
    update.position = 150; // 140 second jump
    update.song_id = Some("song_1".to_string());
    let _ = app.handle_playback_state_updated(update);

    // Listening time should NOT have increased by 140
    assert!(app.scrobble.listening_time < 10.0);
    // Position should still be updated for next delta
    assert_eq!(app.scrobble.last_position, 150.0);
}

// ============================================================================
// Queue Sorting (main.rs)
// ============================================================================

fn make_sorting_queue() -> Vec<nokkvi_data::backend::queue::QueueSongUIViewData> {
    vec![
        make_queue_song_full("s1", "Zebra", "Charlie", "Beta", 3, 240, "Pop"),
        make_queue_song_full("s2", "Alpha", "Alice", "Gamma", 1, 120, "Rock"),
        make_queue_song_full("s3", "Mango", "Bob", "Alpha", 2, 180, "Jazz"),
    ]
}

#[test]
fn sort_queue_by_title_ascending() {
    let mut app = test_app();
    app.library.queue_songs = make_sorting_queue();
    app.queue_page.queue_sort_mode = nokkvi_data::types::queue_sort_mode::QueueSortMode::Title;
    app.queue_page.common.sort_ascending = true;

    app.sort_queue_songs();

    let titles: Vec<&str> = app
        .library
        .queue_songs
        .iter()
        .map(|s| s.title.as_str())
        .collect();
    assert_eq!(titles, vec!["Alpha", "Mango", "Zebra"]);
}

#[test]
fn sort_queue_by_title_descending() {
    let mut app = test_app();
    app.library.queue_songs = make_sorting_queue();
    app.queue_page.queue_sort_mode = nokkvi_data::types::queue_sort_mode::QueueSortMode::Title;
    app.queue_page.common.sort_ascending = false;

    app.sort_queue_songs();

    let titles: Vec<&str> = app
        .library
        .queue_songs
        .iter()
        .map(|s| s.title.as_str())
        .collect();
    assert_eq!(titles, vec!["Zebra", "Mango", "Alpha"]);
}

#[test]
fn sort_queue_by_artist() {
    let mut app = test_app();
    app.library.queue_songs = make_sorting_queue();
    app.queue_page.queue_sort_mode = nokkvi_data::types::queue_sort_mode::QueueSortMode::Artist;
    app.queue_page.common.sort_ascending = true;

    app.sort_queue_songs();

    let artists: Vec<&str> = app
        .library
        .queue_songs
        .iter()
        .map(|s| s.artist.as_str())
        .collect();
    assert_eq!(artists, vec!["Alice", "Bob", "Charlie"]);
}

#[test]
fn sort_queue_by_album() {
    let mut app = test_app();
    app.library.queue_songs = make_sorting_queue();
    app.queue_page.queue_sort_mode = nokkvi_data::types::queue_sort_mode::QueueSortMode::Album;
    app.queue_page.common.sort_ascending = true;

    app.sort_queue_songs();

    let albums: Vec<&str> = app
        .library
        .queue_songs
        .iter()
        .map(|s| s.album.as_str())
        .collect();
    assert_eq!(albums, vec!["Alpha", "Beta", "Gamma"]);
}

#[test]
fn sort_queue_by_duration() {
    let mut app = test_app();
    app.library.queue_songs = make_sorting_queue();
    app.queue_page.queue_sort_mode = nokkvi_data::types::queue_sort_mode::QueueSortMode::Duration;
    app.queue_page.common.sort_ascending = true;

    app.sort_queue_songs();

    let durations: Vec<u32> = app
        .library
        .queue_songs
        .iter()
        .map(|s| s.duration_seconds)
        .collect();
    assert_eq!(durations, vec![120, 180, 240]);
}

#[test]
fn sort_queue_by_genre() {
    let mut app = test_app();
    app.library.queue_songs = make_sorting_queue(); // genres: Pop, Rock, Jazz
    app.queue_page.queue_sort_mode = nokkvi_data::types::queue_sort_mode::QueueSortMode::Genre;
    app.queue_page.common.sort_ascending = true;

    app.sort_queue_songs();

    let genres: Vec<&str> = app
        .library
        .queue_songs
        .iter()
        .map(|s| s.genre.as_str())
        .collect();
    assert_eq!(genres, vec!["Jazz", "Pop", "Rock"]);
}

#[test]
fn sort_queue_by_rating() {
    let mut app = test_app();
    let mut songs = make_sorting_queue();
    // s1: rating 3, s2: no rating, s3: rating 5
    songs[0].rating = Some(3);
    songs[1].rating = None;
    songs[2].rating = Some(5);
    app.library.queue_songs = songs;
    app.queue_page.queue_sort_mode = nokkvi_data::types::queue_sort_mode::QueueSortMode::Rating;
    app.queue_page.common.sort_ascending = true; // ascending = highest first for rating

    app.sort_queue_songs();

    let ratings: Vec<Option<u32>> = app.library.queue_songs.iter().map(|s| s.rating).collect();
    // Rated items first (5, 3), then unrated (None)
    assert_eq!(ratings, vec![Some(5), Some(3), None]);
}

// ============================================================================
// ScrobbleState (state.rs)
// ============================================================================

#[test]
fn scrobble_state_reset_for_new_song() {
    let mut state = crate::state::ScrobbleState {
        listening_time: 120.0,
        last_position: 120.0,
        submitted: true,
        current_song_id: Some("old".to_string()),
        ..Default::default()
    };

    state.reset_for_new_song(Some("new".to_string()), 0.0);

    assert_eq!(state.current_song_id.as_deref(), Some("new"));
    assert_eq!(state.listening_time, 0.0);
    assert_eq!(state.last_position, 0.0);
    assert!(!state.submitted);
}

#[test]
fn scrobble_state_reset_with_nonzero_position() {
    let mut state = crate::state::ScrobbleState::default();

    state.reset_for_new_song(Some("song".to_string()), 5.0);

    assert_eq!(state.last_position, 5.0);
    assert_eq!(state.listening_time, 0.0);
}

// ============================================================================
// View Switching (navigation.rs)
// ============================================================================

#[test]
fn switch_view_updates_current_view() {
    let mut app = test_app();
    assert_eq!(app.current_view, View::Queue); // default

    let _ = app.handle_switch_view(View::Albums);
    assert_eq!(app.current_view, View::Albums);

    let _ = app.handle_switch_view(View::Artists);
    assert_eq!(app.current_view, View::Artists);

    let _ = app.handle_switch_view(View::Songs);
    assert_eq!(app.current_view, View::Songs);

    let _ = app.handle_switch_view(View::Genres);
    assert_eq!(app.current_view, View::Genres);

    let _ = app.handle_switch_view(View::Playlists);
    assert_eq!(app.current_view, View::Playlists);
}

// ============================================================================
// SlotListDown Unfocuses Search (slot_list.rs)
// ============================================================================

#[test]
fn slot_list_down_unfocuses_search_when_focused() {
    let mut app = test_app();
    app.current_view = View::Albums;
    app.albums_page.common.search_input_focused = true;

    let _ = app.handle_slot_list_navigate_down();

    assert!(
        !app.albums_page.common.search_input_focused,
        "search should be unfocused after SlotListDown"
    );
}

#[test]
fn slot_list_down_navigates_when_search_not_focused() {
    let mut app = test_app();
    app.current_view = View::Albums;
    app.albums_page.common.search_input_focused = false;

    // Should NOT unfocus (already unfocused) — returns a Task dispatching SlotListNavigateDown
    let _ = app.handle_slot_list_navigate_down();
    assert!(
        !app.albums_page.common.search_input_focused,
        "search should remain unfocused"
    );
}

#[test]
fn slot_list_down_preserves_settings_search_query() {
    let mut app = test_app();
    app.current_view = View::Settings;
    app.settings_page.search_active = true;
    app.settings_page.search_query = "Scrobbl".to_string();

    let _ = app.handle_slot_list_navigate_down();

    assert!(
        !app.settings_page.search_active,
        "search bar should be dismissed"
    );
    assert_eq!(
        app.settings_page.search_query, "Scrobbl",
        "search query should be preserved so filtered results remain navigable"
    );
}

// ============================================================================
// Loading State Recovery (Layer 1 — stuck Loading... bug fix)
// ============================================================================

#[test]
fn albums_loaded_error_clears_loading() {
    let mut app = test_app();
    app.library.albums.set_loading(true);
    assert!(app.library.albums.is_loading());

    let _ = app.handle_albums_loaded(Err("network error".to_string()), 0, false, None);
    assert!(
        !app.library.albums.is_loading(),
        "loading flag should be cleared on error"
    );
}

#[test]
fn artists_loaded_error_clears_loading() {
    let mut app = test_app();
    app.library.artists.set_loading(true);
    assert!(app.library.artists.is_loading());

    let _ = app.handle_artists_loaded(Err("network error".to_string()), 0, false, None);
    assert!(
        !app.library.artists.is_loading(),
        "loading flag should be cleared on error"
    );
}

#[test]
fn songs_loaded_error_clears_loading() {
    let mut app = test_app();
    app.library.songs.set_loading(true);
    assert!(app.library.songs.is_loading());

    let _ = app.handle_songs_loaded(Err("network error".to_string()), 0, false, None);
    assert!(
        !app.library.songs.is_loading(),
        "loading flag should be cleared on error"
    );
}

#[test]
fn genres_loaded_error_clears_loading() {
    let mut app = test_app();
    app.library.genres.set_loading(true);
    assert!(app.library.genres.is_loading());

    let _ = app.handle_genres_loaded(Err("network error".to_string()), 0);
    assert!(
        !app.library.genres.is_loading(),
        "loading flag should be cleared on error"
    );
}

#[test]
fn playlists_loaded_error_clears_loading() {
    let mut app = test_app();
    app.library.playlists.set_loading(true);
    assert!(app.library.playlists.is_loading());

    let _ = app.handle_playlists_loaded(Err("network error".to_string()), 0);
    assert!(
        !app.library.playlists.is_loading(),
        "loading flag should be cleared on error"
    );
}

// ============================================================================
// Cross-View Star Sync — Sub-Expansion Gaps (star_rating.rs)
// ============================================================================

#[test]
fn song_starred_propagates_to_artists_sub_expansion() {
    let mut app = test_app();
    let mut track = make_song("s1", "Sub Track", "Artist");
    track.is_starred = false;
    app.artists_page.sub_expansion.children = vec![track];

    let _ = app.handle_song_starred_status_updated("s1".to_string(), true);
    assert!(
        app.artists_page.sub_expansion.children[0].is_starred,
        "artists sub-expansion child should be starred"
    );
}

#[test]
fn song_starred_propagates_to_genres_sub_expansion() {
    let mut app = test_app();
    let mut track = make_song("s1", "Sub Track", "Artist");
    track.is_starred = false;
    app.genres_page.sub_expansion.children = vec![track];

    let _ = app.handle_song_starred_status_updated("s1".to_string(), true);
    assert!(
        app.genres_page.sub_expansion.children[0].is_starred,
        "genres sub-expansion child should be starred"
    );
}

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
// Cross-View Rating Propagation — Sub-Expansion Gaps (star_rating.rs)
// ============================================================================

#[test]
fn song_rating_updated_propagates_to_artists_sub_expansion() {
    let mut app = test_app();
    let mut track = make_song("s1", "Sub Track", "Artist");
    track.rating = None;
    app.artists_page.sub_expansion.children = vec![track];

    let _ = app.handle_song_rating_updated("s1".to_string(), 4);
    assert_eq!(
        app.artists_page.sub_expansion.children[0].rating,
        Some(4),
        "artists sub-expansion child rating should be updated"
    );
}

#[test]
fn song_rating_updated_propagates_to_genres_sub_expansion() {
    let mut app = test_app();
    let mut track = make_song("s1", "Sub Track", "Artist");
    track.rating = None;
    app.genres_page.sub_expansion.children = vec![track];

    let _ = app.handle_song_rating_updated("s1".to_string(), 2);
    assert_eq!(
        app.genres_page.sub_expansion.children[0].rating,
        Some(2),
        "genres sub-expansion child rating should be updated"
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
// Progressive Queue Generation Counter (state.rs)
// ============================================================================

#[test]
fn progressive_queue_generation_starts_at_zero() {
    let app = test_app();
    assert_eq!(app.library.progressive_queue_generation, 0);
}

#[test]
fn progressive_queue_generation_increments() {
    let mut app = test_app();
    app.library.progressive_queue_generation += 1;
    assert_eq!(app.library.progressive_queue_generation, 1);
    app.library.progressive_queue_generation += 1;
    assert_eq!(app.library.progressive_queue_generation, 2);
}

// ============================================================================
// ScrobbleState Edge Cases (state.rs)
// ============================================================================

#[test]
fn should_scrobble_returns_true_when_threshold_met() {
    let state = crate::state::ScrobbleState {
        listening_time: 120.0,
        submitted: false,
        current_song_id: Some("song".to_string()),
        ..Default::default()
    };
    // 120s listened, track is 200s, threshold 50% → need 100s → should scrobble
    assert!(state.should_scrobble(200, 0.50));
}

#[test]
fn should_scrobble_returns_false_when_already_submitted() {
    let state = crate::state::ScrobbleState {
        listening_time: 120.0,
        submitted: true,
        current_song_id: Some("song".to_string()),
        ..Default::default()
    };
    assert!(
        !state.should_scrobble(200, 0.50),
        "should not scrobble twice"
    );
}

#[test]
fn should_scrobble_returns_false_for_zero_duration() {
    let state = crate::state::ScrobbleState {
        listening_time: 120.0,
        submitted: false,
        current_song_id: Some("song".to_string()),
        ..Default::default()
    };
    assert!(
        !state.should_scrobble(0, 0.50),
        "zero-duration tracks should never scrobble"
    );
}

// ============================================================================
// Queue Sort Stability (main.rs)
// ============================================================================

#[test]
fn sort_queue_stable_for_equal_values() {
    let mut app = test_app();
    // Three songs with the same artist — stable sort should preserve original order
    app.library.queue_songs = vec![
        make_queue_song_full("s1", "First", "SameArtist", "Album A", 1, 120, "Rock"),
        make_queue_song_full("s2", "Second", "SameArtist", "Album B", 2, 180, "Pop"),
        make_queue_song_full("s3", "Third", "SameArtist", "Album C", 3, 240, "Jazz"),
    ];
    app.queue_page.queue_sort_mode = nokkvi_data::types::queue_sort_mode::QueueSortMode::Artist;
    app.queue_page.common.sort_ascending = true;

    app.sort_queue_songs();

    let ids: Vec<&str> = app
        .library
        .queue_songs
        .iter()
        .map(|s| s.id.as_str())
        .collect();
    assert_eq!(
        ids,
        vec!["s1", "s2", "s3"],
        "stable sort should preserve insertion order for equal artists"
    );
}

#[test]
fn sort_queue_ascending_then_descending_inverts() {
    let mut app = test_app();
    app.library.queue_songs = make_sorting_queue();

    // Sort ascending by title
    app.queue_page.queue_sort_mode = nokkvi_data::types::queue_sort_mode::QueueSortMode::Title;
    app.queue_page.common.sort_ascending = true;
    app.sort_queue_songs();
    let asc: Vec<&str> = app
        .library
        .queue_songs
        .iter()
        .map(|s| s.title.as_str())
        .collect();
    assert_eq!(asc, vec!["Alpha", "Mango", "Zebra"]);

    // Sort descending by same mode
    app.queue_page.common.sort_ascending = false;
    app.sort_queue_songs();
    let desc: Vec<&str> = app
        .library
        .queue_songs
        .iter()
        .map(|s| s.title.as_str())
        .collect();
    assert_eq!(desc, vec!["Zebra", "Mango", "Alpha"]);
}

// ============================================================================
// ToastState Edge Cases (state.rs)
// ============================================================================

#[test]
fn toast_keyed_dedup_replaces_existing() {
    use nokkvi_data::types::toast::{Toast, ToastLevel};
    let mut state = crate::state::ToastState::default();

    // Push a keyed toast
    let mut t1 = Toast::new("Loading 1/10", ToastLevel::Info);
    t1.key = Some("progress".to_string());
    state.push(t1);

    // Push another toast with the same key — should replace, not duplicate
    let mut t2 = Toast::new("Loading 5/10", ToastLevel::Info);
    t2.key = Some("progress".to_string());
    state.push(t2);

    assert_eq!(state.toasts.len(), 1, "keyed toast should deduplicate");
    assert_eq!(state.toasts[0].message, "Loading 5/10");
}

#[test]
fn toast_capacity_evicts_oldest() {
    use nokkvi_data::types::toast::{Toast, ToastLevel};
    let mut state = crate::state::ToastState::default();

    // Fill to capacity (MAX_TOASTS = 10)
    for i in 0..10 {
        state.push(Toast::new(format!("Toast {i}"), ToastLevel::Info));
    }
    assert_eq!(state.toasts.len(), 10);

    // Push one more — oldest should be evicted
    state.push(Toast::new("Overflow", ToastLevel::Info));
    assert_eq!(state.toasts.len(), 10, "should not exceed capacity");
    assert_eq!(
        state.toasts.front().map(|t| t.message.as_str()),
        Some("Toast 1"),
        "oldest toast (Toast 0) should have been evicted"
    );
    assert_eq!(
        state.toasts.back().map(|t| t.message.as_str()),
        Some("Overflow")
    );
}

#[test]
fn toast_dismiss_key_removes_matching() {
    use nokkvi_data::types::toast::{Toast, ToastLevel};
    let mut state = crate::state::ToastState::default();

    let mut t1 = Toast::new("Loading...", ToastLevel::Info);
    t1.key = Some("load".to_string());
    state.push(t1);
    state.push(Toast::new("Unrelated", ToastLevel::Success));

    assert_eq!(state.toasts.len(), 2);

    state.dismiss_key("load");
    assert_eq!(state.toasts.len(), 1);
    assert_eq!(state.toasts[0].message, "Unrelated");
}

// ============================================================================
// View Action Handlers (components.rs)
// ============================================================================

#[test]
fn handle_common_view_action_refresh_returns_task() {
    let app = test_app();

    let persist_fn = |_s, _m, _a| async { Ok(()) };

    let task = app.handle_common_view_action(
        crate::views::CommonViewAction::RefreshViewData,
        crate::app_message::Message::LoadAlbums,
        "albums",
        crate::widgets::view_header::SortMode::Name,
        true,
        persist_fn,
    );

    assert!(task.is_some(), "RefreshViewData should return a task");
}

#[test]
fn handle_common_view_action_navigate_and_search_returns_task() {
    let app = test_app();
    let persist_fn = |_s, _m, _a| async { Ok(()) };

    let task = app.handle_common_view_action(
        crate::views::CommonViewAction::NavigateAndFilter(
            View::Artists,
            nokkvi_data::types::filter::LibraryFilter::ArtistId {
                id: "Beatles".to_string(),
                name: "Beatles".to_string(),
            },
        ),
        crate::app_message::Message::LoadAlbums,
        "albums",
        crate::widgets::view_header::SortMode::Name,
        true,
        persist_fn,
    );

    assert!(
        task.is_some(),
        "NavigateAndFilter should be handled by common action handler"
    );
}

// ============================================================================
// Server Version (mod.rs)
// ============================================================================

#[test]
fn server_version_fetched_updates_state() {
    let mut app = test_app();
    assert_eq!(app.server_version, None);

    let _ = app.update(crate::app_message::Message::ServerVersionFetched(Some(
        "0.61.1".to_string(),
    )));

    assert_eq!(app.server_version.as_deref(), Some("0.61.1"));
}

// ============================================================================
// Settings Escape Priority Chain (views/settings/mod.rs)
// ============================================================================

/// Build a minimal SettingsViewData for testing.
/// Only the structure matters — values are defaults/dummies.
fn make_settings_view_data() -> crate::views::SettingsViewData {
    crate::views::SettingsViewData {
        visualizer_config: crate::visualizer_config::VisualizerConfig::default(),
        theme_file: nokkvi_data::types::theme_file::ThemeFile::default(),
        active_theme_stem: String::new(),
        window_height: 800.0,
        hotkey_config: nokkvi_data::types::hotkey_config::HotkeyConfig::default(),
        server_url: String::new(),
        username: String::new(),
        is_light_mode: false,
        scrobbling_enabled: true,
        scrobble_threshold: 0.50,
        start_view: "Queue".to_string(),
        stable_viewport: true,
        auto_follow_playing: true,
        enter_behavior: "PlayAll",
        local_music_path: String::new(),
        library_page_size: "Default",
        show_album_artists_only: true,
        rounded_mode: false,
        nav_layout: "Top",
        nav_display_mode: "IconsAndLabels",
        track_info_display: "Full",
        slot_row_height: "Default",
        opacity_gradient: true,
        slot_text_links: true,
        crossfade_enabled: false,
        crossfade_duration_secs: 5,
        volume_normalization: false,
        normalization_level: "Standard",
        default_playlist_name: String::new(),
        quick_add_to_playlist: false,
        horizontal_volume: false,
        font_family: String::new(),
        strip_show_title: true,
        strip_show_artist: true,
        strip_show_album: true,
        strip_show_format_info: true,
        strip_click_action: "CenterOnPlaying",
        verbose_config: false,
        artwork_resolution: "Default",
    }
}

#[test]
fn settings_escape_at_root_exits() {
    use crate::views::settings::{NavLevel, SettingsAction, SettingsMessage};
    let mut page = crate::views::SettingsPage::new();
    // Default state: nav_stack = [CategoryPicker], no search, no editing
    assert_eq!(page.nav_stack.len(), 1);
    assert_eq!(*page.current_level(), NavLevel::CategoryPicker);

    let data = make_settings_view_data();
    let action = page.update(SettingsMessage::Escape, &data);
    assert!(
        matches!(action, SettingsAction::ExitSettings),
        "Escape at root should exit settings, got: {action:?}"
    );
}

#[test]
fn settings_escape_with_stale_search_exits() {
    use crate::views::settings::{SettingsAction, SettingsMessage};
    let mut page = crate::views::SettingsPage::new();
    // Simulate: user searched, then SlotListDown cleared search_active but kept query
    page.search_query = "scrobbl".to_string();
    page.search_active = false; // search bar is hidden — query is stale/invisible

    let data = make_settings_view_data();
    let action = page.update(SettingsMessage::Escape, &data);
    assert!(
        matches!(action, SettingsAction::ExitSettings),
        "Escape with stale (inactive) search should exit settings, got: {action:?}"
    );
    // Query should also be cleaned up
    assert!(
        page.search_query.is_empty(),
        "Stale search query should be cleared on exit"
    );
}

#[test]
fn settings_escape_with_active_search_clears_search() {
    use crate::views::settings::{SettingsAction, SettingsMessage};
    let mut page = crate::views::SettingsPage::new();
    page.search_query = "scrobbl".to_string();
    page.search_active = true; // search bar is visible

    let data = make_settings_view_data();
    let action = page.update(SettingsMessage::Escape, &data);
    assert!(
        matches!(action, SettingsAction::None),
        "Escape with active search should clear search (not exit), got: {action:?}"
    );
    assert!(!page.search_active, "search_active should be cleared");
    assert!(page.search_query.is_empty(), "search query should be empty");
}

#[test]
fn settings_escape_pops_nav_stack() {
    use crate::views::settings::{NavLevel, SettingsAction, SettingsMessage, SettingsTab};
    let mut page = crate::views::SettingsPage::new();
    // Drill into General category
    page.push_level(NavLevel::Category(SettingsTab::General));
    assert_eq!(page.nav_stack.len(), 2);

    let data = make_settings_view_data();
    let action = page.update(SettingsMessage::Escape, &data);
    assert!(
        matches!(action, SettingsAction::None),
        "Escape at depth 2 should pop nav stack, got: {action:?}"
    );
    assert_eq!(
        page.nav_stack.len(),
        1,
        "Nav stack should be popped to root"
    );
}

#[test]
fn settings_escape_cancels_hotkey_capture() {
    use crate::views::settings::{SettingsAction, SettingsMessage};
    let mut page = crate::views::SettingsPage::new();
    page.capturing_hotkey = Some(nokkvi_data::types::hotkey_config::HotkeyAction::TogglePlay);

    let data = make_settings_view_data();
    let action = page.update(SettingsMessage::Escape, &data);
    assert!(
        matches!(action, SettingsAction::None),
        "Escape during hotkey capture should cancel capture, got: {action:?}"
    );
    assert!(
        page.capturing_hotkey.is_none(),
        "capturing_hotkey should be cleared"
    );
}

#[test]
fn settings_escape_exits_edit_mode() {
    use crate::views::settings::{SettingsAction, SettingsMessage};
    let mut page = crate::views::SettingsPage::new();
    page.editing_index = Some(0);

    let data = make_settings_view_data();
    let action = page.update(SettingsMessage::Escape, &data);
    assert!(
        matches!(action, SettingsAction::None),
        "Escape during edit mode should exit edit, got: {action:?}"
    );
    assert!(
        page.editing_index.is_none(),
        "editing_index should be cleared"
    );
}

// ============================================================================
// Hotkey Suppression During Text Input (TDD — regression from 2c54792)
// ============================================================================
//
// When a text_input widget has captured a key event (Status::Captured),
// hotkeys should NOT fire — the user is typing in a search field.
//
// Exceptions:
// - Escape should always pass through (close overlays / clear search)
// - Ctrl+key combos should always pass through (Ctrl+S, Ctrl+D, Ctrl+E)

/// Helper: simulate a RawKeyEvent through the full update() dispatch.
fn send_raw_key(
    app: &mut crate::Nokkvi,
    key: iced::keyboard::Key,
    modifiers: iced::keyboard::Modifiers,
    status: iced::event::Status,
) -> iced::Task<crate::Message> {
    app.update(crate::Message::RawKeyEvent(key, modifiers, status))
}

#[test]
fn hotkey_suppressed_when_captured_toggle_random() {
    // 'x' is bound to ToggleRandom. If captured by a text_input, it must NOT toggle.
    let mut app = test_app();
    app.current_view = View::Queue;
    app.screen = crate::Screen::Home;
    assert!(!app.modes.random, "random should start as false");

    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Character("x".into()),
        iced::keyboard::Modifiers::empty(),
        iced::event::Status::Captured,
    );

    assert!(
        !app.modes.random,
        "ToggleRandom ('x') should be suppressed when Status::Captured"
    );
}

#[test]
fn hotkey_suppressed_when_captured_toggle_consume() {
    // 'c' is bound to ToggleConsume. Must be suppressed when captured.
    let mut app = test_app();
    app.current_view = View::Albums;
    app.screen = crate::Screen::Home;
    assert!(!app.modes.consume);

    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Character("c".into()),
        iced::keyboard::Modifiers::empty(),
        iced::event::Status::Captured,
    );

    assert!(
        !app.modes.consume,
        "ToggleConsume ('c') should be suppressed when Status::Captured"
    );
}

#[test]
fn hotkey_fires_when_not_captured_toggle_random() {
    // Same key 'x' with Status::Ignored should work normally.
    let mut app = test_app();
    app.current_view = View::Queue;
    app.screen = crate::Screen::Home;
    assert!(!app.modes.random);

    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Character("x".into()),
        iced::keyboard::Modifiers::empty(),
        iced::event::Status::Ignored,
    );

    assert!(
        app.modes.random,
        "ToggleRandom should fire when Status::Ignored (no widget has focus)"
    );
}

#[test]
fn escape_not_suppressed_when_captured() {
    // Escape should always fire, even when a text_input has captured the event.
    // This was the whole reason we switched to event::listen_with() in 2c54792.
    let mut app = test_app();
    app.current_view = View::Settings;
    app.screen = crate::Screen::Home;
    app.window.eq_modal_open = true;

    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
        iced::keyboard::Modifiers::empty(),
        iced::event::Status::Captured,
    );

    assert!(
        !app.window.eq_modal_open,
        "Escape should close EQ modal even when Status::Captured"
    );
}

#[test]
fn ctrl_combo_not_suppressed_when_captured() {
    // Ctrl+E is bound to ToggleBrowsingPanel. Ctrl+ combos are intentional
    // actions, not typing — they must NOT be suppressed even when captured.
    // Without app_service the handler returns Task::none(), but the fact that
    // it reaches the handler (no panic, no suppression) is what we're testing.
    let mut app = test_app();
    app.current_view = View::Queue;
    app.screen = crate::Screen::Home;
    assert!(app.browsing_panel.is_none());

    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Character("e".into()),
        iced::keyboard::Modifiers::CTRL,
        iced::event::Status::Captured,
    );

    // ToggleBrowsingPanel was dispatched (not suppressed). No panic = success.
    // Contrast with hotkey_suppressed_when_captured_toggle_random which MUST
    // be suppressed under the same Status::Captured condition.
}

// ============================================================================
// FocusSearch routing with browsing panel open (navigation.rs)
// ============================================================================
//
// When current_view == Settings and the browsing panel is open with browser
// focus, FocusSearch (/) must route to the Settings search handler — NOT to
// the browsing panel's active page.

#[test]
fn focus_search_on_settings_ignores_browsing_panel() {
    // Setup: Settings view, browsing panel open with browser focus on Songs tab
    let mut app = test_app();
    app.current_view = View::Settings;
    app.screen = crate::Screen::Home;
    app.browsing_panel = Some(crate::views::BrowsingPanel::new()); // default: Songs tab
    app.pane_focus = crate::state::PaneFocus::Browser;

    // Pre-condition: songs page search is not focused
    assert!(
        !app.songs_page.common.search_input_focused,
        "songs_page search should start unfocused"
    );

    // Act: trigger FocusSearch hotkey
    let _ = app.handle_focus_search();

    // Assert: songs_page must NOT have been touched — we're on Settings
    assert!(
        !app.songs_page.common.search_input_focused,
        "FocusSearch on Settings should NOT focus the browsing panel's search field"
    );
}

#[test]
fn focus_search_on_settings_without_panel_works() {
    // Baseline: Settings view, no browsing panel — should not panic or
    // accidentally set any page's search_input_focused.
    let mut app = test_app();
    app.current_view = View::Settings;
    app.screen = crate::Screen::Home;

    let _ = app.handle_focus_search();

    // No ViewPage search should be focused
    assert!(!app.songs_page.common.search_input_focused);
    assert!(!app.albums_page.common.search_input_focused);
    assert!(!app.queue_page.common.search_input_focused);
}

// ============================================================================
// Playlist Mutation → Queue Header (update/mod.rs)
// ============================================================================

#[test]
fn playlist_created_from_queue_sets_active_playlist_info() {
    let mut app = test_app();
    assert!(app.active_playlist_info.is_none());

    let _ = app.update(crate::app_message::Message::PlaylistMutated(
        crate::app_message::PlaylistMutation::Created(
            "My Queue Playlist".to_string(),
            Some("pl-123".to_string()),
        ),
    ));

    let info = app
        .active_playlist_info
        .as_ref()
        .expect("active_playlist_info should be set after Created with ID");
    assert_eq!(info.id, "pl-123", "playlist ID should match");
    assert_eq!(info.name, "My Queue Playlist", "playlist name should match");
    assert_eq!(
        info.comment, "",
        "comment should be empty for new playlists"
    );
}

#[test]
fn playlist_overwritten_from_queue_sets_active_playlist_info() {
    let mut app = test_app();
    assert!(app.active_playlist_info.is_none());

    let _ = app.update(crate::app_message::Message::PlaylistMutated(
        crate::app_message::PlaylistMutation::Overwritten(
            "Overwritten Playlist".to_string(),
            Some("pl-456".to_string()),
        ),
    ));

    let info = app
        .active_playlist_info
        .as_ref()
        .expect("active_playlist_info should be set after Overwritten with ID");
    assert_eq!(info.id, "pl-456");
    assert_eq!(info.name, "Overwritten Playlist");
}

#[test]
fn playlist_created_without_id_does_not_set_active_playlist_info() {
    let mut app = test_app();
    assert!(app.active_playlist_info.is_none());

    // Created from a non-queue context (e.g. "Add to Playlist" dialog) — no ID
    let _ = app.update(crate::app_message::Message::PlaylistMutated(
        crate::app_message::PlaylistMutation::Created("From Songs View".to_string(), None),
    ));

    assert!(
        app.active_playlist_info.is_none(),
        "active_playlist_info should NOT be set when Created has no playlist ID"
    );
}

#[test]
fn playlist_deleted_does_not_set_active_playlist_info() {
    let mut app = test_app();
    assert!(app.active_playlist_info.is_none());

    let _ = app.update(crate::app_message::Message::PlaylistMutated(
        crate::app_message::PlaylistMutation::Deleted("Deleted Playlist".to_string()),
    ));

    assert!(
        app.active_playlist_info.is_none(),
        "active_playlist_info should NOT be set for Delete mutations"
    );
}

// ============================================================================
// Dominant Color State (albums view overlay)
// ============================================================================

#[test]
fn dominant_color_calculated_updates_global_snapshot() {
    let mut app = test_app();
    assert!(
        app.artwork.album_dominant_colors_snapshot.is_empty(),
        "dominant_color snapshot should start empty"
    );

    // Simulate receiving a calculated dominant color
    let color = iced::Color::from_rgb(0.5, 0.3, 0.2);
    let _ = app.update(crate::app_message::Message::Artwork(
        crate::app_message::ArtworkMessage::DominantColorCalculated("dummy".to_string(), color),
    ));

    assert!(
        app.artwork
            .album_dominant_colors_snapshot
            .contains_key("dummy"),
        "dominant_color snapshot should be set after DominantColorCalculated"
    );
    let stored = *app
        .artwork
        .album_dominant_colors_snapshot
        .get("dummy")
        .unwrap();
    assert!((stored.r - 0.5).abs() < 0.01);
    assert!((stored.g - 0.3).abs() < 0.01);
    assert!((stored.b - 0.2).abs() < 0.01);
}

// ============================================================================
// Navigate and Search Handlers
// ============================================================================

#[test]
fn handle_navigate_and_filter_updates_view_and_defocuses() {
    let mut app = test_app();
    app.current_view = View::Queue; // Start at Queue
    app.artists_page.common.search_input_focused = true;

    let _ = app.handle_navigate_and_filter(
        View::Artists,
        nokkvi_data::types::filter::LibraryFilter::ArtistId {
            id: "The Beatles".to_string(),
            name: "The Beatles".to_string(),
        },
    );

    assert_eq!(app.current_view, View::Artists);
    // search_input_focused is cleared synchronously; the actual query is set
    // asynchronously by the batched SearchQueryChanged task.
    assert!(!app.artists_page.common.search_input_focused);
}

#[test]
fn handle_navigate_and_filter_updates_queue_properly() {
    let mut app = test_app();
    app.current_view = View::Songs; // Start at Songs
    app.queue_page.common.search_input_focused = true;

    let _ = app.handle_navigate_and_filter(
        View::Queue,
        nokkvi_data::types::filter::LibraryFilter::AlbumId {
            id: "Master".to_string(),
            title: "Master".to_string(),
        },
    );

    assert_eq!(app.current_view, View::Queue);
    assert!(!app.queue_page.common.search_input_focused);
}

#[test]
fn queue_page_navigate_and_filter_returns_action() {
    let mut app = test_app();
    let (_, action) = app.queue_page.update(
        crate::views::QueueMessage::NavigateAndFilter(
            View::Albums,
            nokkvi_data::types::filter::LibraryFilter::AlbumId {
                id: "Daft Punk".to_string(),
                title: "Daft Punk".to_string(),
            },
        ),
        &[],
    );
    match action {
        crate::views::QueueAction::NavigateAndFilter(v, f) => {
            assert_eq!(v, View::Albums);
            assert!(matches!(
                f,
                nokkvi_data::types::filter::LibraryFilter::AlbumId { .. }
            ));
        }
        _ => panic!("Expected NavigateAndFilter action"),
    }
}

#[test]
fn songs_page_navigate_and_filter_returns_action() {
    let mut app = test_app();
    let (_, action) = app.songs_page.update(
        crate::views::SongsMessage::NavigateAndFilter(
            View::Artists,
            nokkvi_data::types::filter::LibraryFilter::ArtistId {
                id: "Pink".to_string(),
                name: "Pink".to_string(),
            },
        ),
        &[],
    );
    match action {
        crate::views::SongsAction::NavigateAndFilter(v, f) => {
            assert_eq!(v, View::Artists);
            assert!(matches!(
                f,
                nokkvi_data::types::filter::LibraryFilter::ArtistId { .. }
            ));
        }
        _ => panic!("Expected NavigateAndFilter action"),
    }
}

#[test]
fn albums_page_navigate_and_filter_returns_action() {
    let mut app = test_app();
    let (_, action) = app.albums_page.update(
        crate::views::AlbumsMessage::NavigateAndFilter(
            View::Songs,
            nokkvi_data::types::filter::LibraryFilter::AlbumId {
                id: "Get Lucky".to_string(),
                title: "Get Lucky".to_string(),
            },
        ),
        0,
        &[],
    );
    match action {
        crate::views::AlbumsAction::NavigateAndFilter(v, f) => {
            assert_eq!(v, View::Songs);
            assert!(matches!(
                f,
                nokkvi_data::types::filter::LibraryFilter::AlbumId { .. }
            ));
        }
        _ => panic!("Expected NavigateAndFilter action"),
    }
}

// ============================================================================
// Genres Context Menu — Child/Grandchild GetInfo + ShowInFolder
// ============================================================================

#[test]
fn genres_context_menu_get_info_on_child_album() {
    let mut app = test_app();
    let genres = vec![make_genre("g1", "Rock")];
    app.library.genres.set_from_vec(genres.clone());

    // Expand genre so child album is at index 1
    let album = make_album("a1", "Album 1", "Artist A");
    app.genres_page.expansion.expanded_id = Some("g1".to_string());
    app.genres_page.expansion.parent_offset = 0;
    app.genres_page.expansion.children = vec![album];

    let (_, action) = app.genres_page.update(
        crate::views::GenresMessage::ContextMenuAction(
            1, // child album index
            crate::widgets::context_menu::LibraryContextEntry::GetInfo,
        ),
        genres.len(),
        &genres,
    );
    match action {
        crate::views::GenresAction::ShowInfo(item) => match *item {
            nokkvi_data::types::info_modal::InfoModalItem::Album { ref name, .. } => {
                assert_eq!(name, "Album 1");
            }
            _ => panic!("Expected InfoModalItem::Album"),
        },
        other => panic!("Expected ShowInfo action, got {other:?}"),
    }
}

#[test]
fn genres_context_menu_show_in_folder_on_child_album() {
    let mut app = test_app();
    let genres = vec![make_genre("g1", "Rock")];
    app.library.genres.set_from_vec(genres.clone());

    let album = make_album("a1", "Album 1", "Artist A");
    app.genres_page.expansion.expanded_id = Some("g1".to_string());
    app.genres_page.expansion.parent_offset = 0;
    app.genres_page.expansion.children = vec![album];

    let (_, action) = app.genres_page.update(
        crate::views::GenresMessage::ContextMenuAction(
            1, // child album index
            crate::widgets::context_menu::LibraryContextEntry::ShowInFolder,
        ),
        genres.len(),
        &genres,
    );
    match action {
        crate::views::GenresAction::ShowAlbumInFolder(album_id) => {
            assert_eq!(album_id, "a1");
        }
        other => panic!("Expected ShowAlbumInFolder action, got {other:?}"),
    }
}

#[test]
fn genres_context_menu_get_info_on_grandchild_song() {
    let mut app = test_app();
    let genres = vec![make_genre("g1", "Rock")];
    app.library.genres.set_from_vec(genres.clone());

    // Expand genre + sub-expand album so grandchild song is at index 2
    let album = make_album("a1", "Album 1", "Artist A");
    app.genres_page.expansion.expanded_id = Some("g1".to_string());
    app.genres_page.expansion.parent_offset = 0;
    app.genres_page.expansion.children = vec![album];

    let song = make_song("s1", "Song One", "Artist A");
    app.genres_page.sub_expansion.expanded_id = Some("a1".to_string());
    app.genres_page.sub_expansion.parent_offset = 1;
    app.genres_page.sub_expansion.children = vec![song];

    let (_, action) = app.genres_page.update(
        crate::views::GenresMessage::ContextMenuAction(
            2, // grandchild song index
            crate::widgets::context_menu::LibraryContextEntry::GetInfo,
        ),
        genres.len(),
        &genres,
    );
    match action {
        crate::views::GenresAction::ShowInfo(item) => match *item {
            nokkvi_data::types::info_modal::InfoModalItem::Song { ref title, .. } => {
                assert_eq!(title, "Song One");
            }
            _ => panic!("Expected InfoModalItem::Song"),
        },
        other => panic!("Expected ShowInfo action, got {other:?}"),
    }
}

#[test]
fn genres_context_menu_show_in_folder_on_grandchild_song() {
    let mut app = test_app();
    let genres = vec![make_genre("g1", "Rock")];
    app.library.genres.set_from_vec(genres.clone());

    let album = make_album("a1", "Album 1", "Artist A");
    app.genres_page.expansion.expanded_id = Some("g1".to_string());
    app.genres_page.expansion.parent_offset = 0;
    app.genres_page.expansion.children = vec![album];

    let song = make_song("s1", "Song One", "Artist A");
    app.genres_page.sub_expansion.expanded_id = Some("a1".to_string());
    app.genres_page.sub_expansion.parent_offset = 1;
    app.genres_page.sub_expansion.children = vec![song];

    let (_, action) = app.genres_page.update(
        crate::views::GenresMessage::ContextMenuAction(
            2, // grandchild song index
            crate::widgets::context_menu::LibraryContextEntry::ShowInFolder,
        ),
        genres.len(),
        &genres,
    );
    match action {
        crate::views::GenresAction::ShowSongInFolder(path) => {
            assert_eq!(path, "/music/s1.flac");
        }
        other => panic!("Expected ShowSongInFolder action, got {other:?}"),
    }
}

// ============================================================================
// Focus and Expand Artwork Fetching Bug (albums.rs / etc)
// ============================================================================

#[test]
fn album_focus_and_expand_triggers_large_artwork_load() {
    let mut app = test_app();
    app.current_view = View::Albums;
    app.library
        .albums
        .set_from_vec(vec![make_album("a1", "Album", "Artist")]);

    // Act: Focus and expand the first item
    let _ = app.handle_albums(crate::views::AlbumsMessage::FocusAndExpand(0));

    // Assert: It should schedule loading the large artwork so the background updates
    assert_eq!(
        app.artwork.loading_large_artwork.as_deref(),
        Some("a1"),
        "LoadLargeArtwork should be dispatched so the new dominant color can be fetched"
    );
}

// Only AlbumsPage uses the dominant_color overlay logic which requires LoadLargeArtwork
// to be triggered when expanding via click.

// ============================================================================
// Crossfade Toggle (playback.rs)
// ============================================================================

#[test]
fn crossfade_toggle_flips_state() {
    let mut app = test_app();
    assert!(
        !app.engine.crossfade_enabled,
        "crossfade should default to false"
    );

    let _ = app.handle_toggle_crossfade();
    assert!(
        app.engine.crossfade_enabled,
        "first toggle should enable crossfade"
    );

    let _ = app.handle_toggle_crossfade();
    assert!(
        !app.engine.crossfade_enabled,
        "second toggle should disable crossfade"
    );
}

#[test]
fn crossfade_toggle_from_enabled() {
    let mut app = test_app();
    app.engine.crossfade_enabled = true;

    let _ = app.handle_toggle_crossfade();
    assert!(
        !app.engine.crossfade_enabled,
        "toggle from enabled should disable"
    );
}

// ============================================================================
// Settings Footer: Stale description_text After Sub-List Exit
// ============================================================================
//
// Bug: The description footer retains text from the item the user was on
// before entering a sub-list (color array or font picker). When the user
// escapes back to the main settings list, the footer shows the old description
// instead of the current center item's subtitle.

#[test]
fn settings_description_updates_after_color_sub_list_escape() {
    use crate::views::settings::{NavLevel, SettingsMessage, SettingsTab};

    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    // 1. Drill into Visualizer category (Level 2)
    page.push_level(NavLevel::Category(SettingsTab::Visualizer));
    page.refresh_entries(&data);

    // 2. Navigate to a ColorArray item (peak gradient colors).
    //    Find the index of the peak_gradient_colors entry.
    let peak_gradient_idx = page
        .cached_entries
        .iter()
        .position(|e| {
            matches!(e, crate::views::settings::items::SettingsEntry::Item(item)
                if item.key.contains("peak_gradient_colors"))
        })
        .expect("peak_gradient_colors entry should exist in Visualizer tab");

    // Position the slot list on the peak gradient colors item
    let total = page.cached_entries.len();
    page.slot_list.set_offset(peak_gradient_idx, total);
    page.update_description();

    // Capture the description text for the peak gradient item
    let peak_description = page.description_text.clone();
    assert!(
        !peak_description.is_empty(),
        "peak gradient colors item should have a description"
    );

    // 3. Activate to open the color sub-list
    let _ = page.update(SettingsMessage::EditActivate, &data);
    assert!(
        page.sub_list.is_some(),
        "EditActivate on ColorArray should open sub-list"
    );

    // Set a known-stale value while sub-list is active. In reality, the
    // description_text retains whatever it was before entering — this
    // exaggeration makes the test non-trivially detectible.
    page.description_text = "STALE FROM BEFORE COLOR SUB-LIST".to_string();

    // 4. Escape from the sub-list
    let _ = page.update(SettingsMessage::Escape, &data);
    assert!(page.sub_list.is_none(), "Escape should close the sub-list");

    // 5. description_text must be refreshed after sub-list exit
    assert_ne!(
        page.description_text, "STALE FROM BEFORE COLOR SUB-LIST",
        "description_text should be refreshed after color sub-list exit, \
         but it retained the stale value",
    );
}

#[test]
fn settings_description_updates_after_font_sub_list_escape() {
    use crate::views::settings::{NavLevel, SettingsMessage, SettingsTab};

    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    // 1. Drill into Interface category where font_family lives
    page.push_level(NavLevel::Category(SettingsTab::Interface));
    page.refresh_entries(&data);

    // 2. Navigate to the font_family item
    let font_idx = page
        .cached_entries
        .iter()
        .position(|e| {
            matches!(e, crate::views::settings::items::SettingsEntry::Item(item)
                if item.key.as_ref() == "font_family")
        })
        .expect("font_family entry should exist in Interface tab");

    let total = page.cached_entries.len();
    page.slot_list.set_offset(font_idx, total);
    page.update_description();

    let font_description = page.description_text.clone();
    assert!(
        !font_description.is_empty(),
        "font_family item should have a description"
    );

    // 3. Manually open font sub-list (simulating EditActivate)
    let all_fonts = vec!["Inter".to_string(), "Roboto".to_string()];
    page.font_sub_list = Some(crate::views::settings::FontSubListState {
        all_fonts: all_fonts.clone(),
        filtered_fonts: all_fonts,
        search_query: String::new(),
        slot_list: crate::widgets::SlotListView::new(),
        parent_offset: page.slot_list.viewport_offset,
    });

    // 4. Set a different description to prove staleness
    page.description_text = "STALE DESCRIPTION FROM BEFORE FONT PICKER".to_string();

    // 5. Escape from font sub-list
    let _ = page.update(SettingsMessage::Escape, &data);
    assert!(
        page.font_sub_list.is_none(),
        "Escape should close font sub-list"
    );

    // 6. description_text must be refreshed, not stale
    assert_ne!(
        page.description_text, "STALE DESCRIPTION FROM BEFORE FONT PICKER",
        "description_text should be refreshed after font sub-list exit"
    );
}

#[test]
fn settings_description_fresh_after_sub_list_then_pop_to_level1() {
    use crate::views::settings::{NavLevel, SettingsMessage, SettingsTab};

    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    // 1. Drill into Visualizer
    page.push_level(NavLevel::Category(SettingsTab::Visualizer));
    page.refresh_entries(&data);

    // 2. Navigate to a ColorArray item and open sub-list
    let color_idx = page
        .cached_entries
        .iter()
        .position(|e| {
            matches!(e, crate::views::settings::items::SettingsEntry::Item(item)
                if matches!(item.value, crate::views::settings::items::SettingValue::ColorArray(_)))
        })
        .expect("Should have at least one ColorArray entry in Visualizer tab");

    let total = page.cached_entries.len();
    page.slot_list.set_offset(color_idx, total);
    let _ = page.update(SettingsMessage::EditActivate, &data);
    assert!(page.sub_list.is_some(), "Should open sub-list");

    // 3. Escape sub-list
    let _ = page.update(SettingsMessage::Escape, &data);
    assert!(page.sub_list.is_none(), "Sub-list should be closed");

    // 4. Escape to pop back to Level 1 (CategoryPicker)
    let _ = page.update(SettingsMessage::Escape, &data);
    assert_eq!(
        *page.current_level(),
        NavLevel::CategoryPicker,
        "Should be back at CategoryPicker"
    );

    // 5. description_text should show a Level 1 header description,
    //    NOT the stale visualizer sub-item description.
    let level1_descriptions: Vec<&str> = SettingsTab::ALL.iter().map(|t| t.description()).collect();

    // The description should either be one of the tab descriptions or empty
    // (if somehow the cursor landed on nothing), but NOT a visualizer item subtitle.
    let is_valid_level1_desc = level1_descriptions.contains(&page.description_text.as_str())
        || page.description_text.is_empty();

    assert!(
        is_valid_level1_desc,
        "description_text should be a Level 1 tab description after popping to CategoryPicker, \
         got: '{}'",
        page.description_text,
    );
}

// ============================================================================
// Settings Footer: description_text Around Search Interactions
// ============================================================================

#[test]
fn settings_search_updates_description_from_stale() {
    use crate::views::settings::SettingsMessage;

    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    // 1. At CategoryPicker, set up initial state
    page.refresh_entries(&data);
    page.update_description();

    // 2. Inject a known stale description
    page.description_text = "STALE BEFORE SEARCH".to_string();

    // 3. Search for something that yields items from deeper tabs
    let _ = page.update(SettingsMessage::SearchChanged("noise".to_string()), &data);
    assert!(
        !page.cached_entries.is_empty(),
        "'noise' should match at least 'Noise Reduction' from Visualizer tab"
    );

    // 4. Description should have been refreshed by SearchChanged → refresh_entries
    assert_ne!(
        page.description_text, "STALE BEFORE SEARCH",
        "SearchChanged should refresh description_text"
    );
}

#[test]
fn settings_search_clear_restores_level_description() {
    use crate::views::settings::{NavLevel, SettingsMessage, SettingsTab};

    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    // 1. Drill into Visualizer
    page.push_level(NavLevel::Category(SettingsTab::Visualizer));
    page.refresh_entries(&data);
    page.update_description();

    // Capture description at offset 0 of Visualizer (should be the first non-header item)
    let viz_initial_desc = page.description_text.clone();

    // 2. Search for something
    let _ = page.update(SettingsMessage::SearchChanged("led".to_string()), &data);
    let _search_desc = page.description_text.clone();

    // 3. Clear search by sending empty query
    let _ = page.update(SettingsMessage::SearchChanged(String::new()), &data);

    // 4. Entries should be rebuilt for Visualizer (current level).
    //    Slot list was reset to offset 0, so description should match
    //    offset 0 of visualizer entries (same as step 1).
    assert_eq!(
        page.description_text, viz_initial_desc,
        "after clearing search, description should match the current level's entries at offset 0"
    );
}

#[test]
fn settings_escape_active_search_updates_description() {
    use crate::views::settings::{NavLevel, SettingsMessage, SettingsTab};

    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    // 1. Drill into General, search for something
    page.push_level(NavLevel::Category(SettingsTab::General));
    page.refresh_entries(&data);
    let _ = page.update(SettingsMessage::SearchChanged("scrobbl".to_string()), &data);

    // Inject a stale description to catch missing update_description
    page.description_text = "STALE BEFORE ESCAPE SEARCH".to_string();

    // 2. Escape clears active search
    let _ = page.update(SettingsMessage::Escape, &data);
    assert!(!page.search_active, "search should be deactivated");
    assert!(
        page.search_query.is_empty(),
        "search query should be cleared"
    );

    // 3. Description must be refreshed, not stale
    assert_ne!(
        page.description_text, "STALE BEFORE ESCAPE SEARCH",
        "description_text should be refreshed after Escape clears search"
    );
}

#[test]
fn settings_search_then_sub_list_then_escape_updates_description() {
    use crate::views::settings::SettingsMessage;

    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    // 1. At CategoryPicker, search for "gradient" to find color arrays
    let _ = page.update(
        SettingsMessage::SearchChanged("gradient".to_string()),
        &data,
    );
    assert!(
        !page.cached_entries.is_empty(),
        "'gradient' should match entries"
    );

    // 2. Find a ColorArray entry in the search results
    let color_idx = page.cached_entries.iter().position(|e| {
        matches!(e, crate::views::settings::items::SettingsEntry::Item(item)
                if matches!(item.value, crate::views::settings::items::SettingValue::ColorArray(_)))
    });

    if let Some(idx) = color_idx {
        // Navigate to it
        let total = page.cached_entries.len();
        page.slot_list.set_offset(idx, total);
        page.update_description();

        // 3. Open the sub-list
        let _ = page.update(SettingsMessage::EditActivate, &data);
        assert!(page.sub_list.is_some(), "should open color sub-list");

        // Inject stale description
        page.description_text = "STALE FROM SEARCH SUB-LIST".to_string();

        // 4. Escape from sub-list
        let _ = page.update(SettingsMessage::Escape, &data);
        assert!(page.sub_list.is_none(), "sub-list should close");

        // 5. Description should be refreshed (we're still in search mode)
        assert_ne!(
            page.description_text, "STALE FROM SEARCH SUB-LIST",
            "description should be refreshed after sub-list exit during search"
        );

        // Verify search_query is still intact (sub-list exit shouldn't clear search)
        assert_eq!(
            page.search_query, "gradient",
            "search query should survive sub-list exit"
        );
    }
}

#[test]
fn settings_search_from_sub_list_is_noop() {
    use crate::views::settings::{NavLevel, SettingsMessage, SettingsTab};

    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    // 1. Drill into Visualizer and open a color sub-list
    page.push_level(NavLevel::Category(SettingsTab::Visualizer));
    page.refresh_entries(&data);

    let color_idx = page
        .cached_entries
        .iter()
        .position(|e| {
            matches!(e, crate::views::settings::items::SettingsEntry::Item(item)
                if matches!(item.value, crate::views::settings::items::SettingValue::ColorArray(_)))
        })
        .expect("Visualizer should have a ColorArray entry");

    let total = page.cached_entries.len();
    page.slot_list.set_offset(color_idx, total);
    let _ = page.update(SettingsMessage::EditActivate, &data);
    assert!(page.sub_list.is_some(), "should be in sub-list");

    // 2. Capture current state
    let _desc_before = page.description_text.clone();
    let entries_before = page.cached_entries.len();

    // 3. Attempt to search while in sub-list — should be a no-op
    let _ = page.update(SettingsMessage::SearchChanged("test".to_string()), &data);

    // 4. Sub-list should still be open, entries unchanged
    assert!(page.sub_list.is_some(), "sub-list should remain open");
    assert_eq!(
        page.cached_entries.len(),
        entries_before,
        "entries should not change during sub-list search"
    );
    // search_query should NOT be set (sub-list handler ignores SearchChanged)
    // Actually, the sub-list handler returns None without modifying search_query
    // But wait — does search_query get modified? Let's check:
    assert!(
        page.search_query.is_empty(),
        "search_query should not be modified while in sub-list"
    );
}

#[test]
fn settings_search_header_does_not_use_tab_description() {
    use crate::views::settings::{SettingsMessage, SettingsTab};

    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    // Search for "noise" — this returns a "General" section header (from
    // Visualizer's internal sections) followed by "Noise Reduction".
    // The "General" header shares its name with the top-level General tab.
    let _ = page.update(SettingsMessage::SearchChanged("noise".to_string()), &data);
    assert!(
        !page.cached_entries.is_empty(),
        "'noise' should yield results"
    );

    // At offset 0, the center item should be the "General" section header
    // from the Visualizer tab's search results.
    let total = page.cached_entries.len();
    let center_is_general_header = page
        .slot_list
        .get_center_item_index(total)
        .and_then(|idx| page.cached_entries.get(idx))
        .is_some_and(|e| {
            matches!(e, crate::views::settings::items::SettingsEntry::Header { label, .. } if *label == "General")
        });

    if center_is_general_header {
        // The General *tab* description — this should NOT appear during search
        let general_tab_desc = SettingsTab::General.description();

        assert_ne!(
            page.description_text, general_tab_desc,
            "During search, a section header named 'General' should NOT be \
             mapped to the General tab's description '{general_tab_desc}'. \
             It should show just the section label.",
        );

        assert_eq!(
            page.description_text, "General",
            "During search, section headers should display their label, \
             not a tab description"
        );
    }
}

/// Exact repro from the user bug report:
/// 1. Open settings → search "peak" → Tab to navigate → Escape exits settings
/// 2. Re-open settings → description_text shows stale "Peak Gradient Mode" subtitle
///
/// Root cause: Tab sets `search_active = false` while keeping `search_query = "peak"`,
/// so Escape's `search_active && !search_query.is_empty()` check fails, sending the
/// page straight to ExitSettings without clearing description_text.
#[test]
fn settings_stale_description_after_tab_deactivated_search_then_exit() {
    use crate::views::settings::{SettingsMessage, SettingsTab};

    let mut app = test_app();
    app.current_view = View::Settings;

    // 1. Open settings and search for "peak"
    let _ = app.handle_settings(SettingsMessage::SearchChanged("peak".to_string()));
    assert!(
        !app.settings_page.cached_entries.is_empty(),
        "'peak' should match entries"
    );

    // 2. Tab navigates down — also sets search_active = false
    //    (This is what handle_slot_list_navigate_down does for Settings)
    app.settings_page.search_active = false;
    // search_query stays "peak" — this is the zombie state
    let _ = app.handle_settings(SettingsMessage::SlotListDown);

    // Capture the description, which should show peak gradient subtitle
    let desc_during_zombie = app.settings_page.description_text.clone();
    assert!(
        !desc_during_zombie.is_empty(),
        "description should be set during search results"
    );

    // 3. Escape — with search_active=false, this skips search-clearing
    //    and should exit settings. The description_text survives.
    let _ = app.handle_settings(SettingsMessage::Escape);

    // 4. Simulate re-opening settings
    //    In real app: handle_switch_view(Settings) returns Task::none()
    //    so no handle_settings call happens before the first render.
    //    The stale description_text is displayed.
    app.current_view = View::Settings;

    // If config_dirty is false and cached_entries is non-empty,
    // handle_settings won't auto-refresh on the first message.
    // So description_text must already be correct.
    //
    // The description should NOT be the stale zombie search result text.
    // It should be a valid Level 1 tab description or empty.
    let level1_descriptions: Vec<&str> = SettingsTab::ALL.iter().map(|t| t.description()).collect();

    let is_valid = app.settings_page.description_text.is_empty()
        || level1_descriptions.contains(&app.settings_page.description_text.as_str());

    assert!(
        is_valid,
        "After re-opening settings, description should be a Level 1 tab \
         description or empty, got stale: '{}'",
        app.settings_page.description_text,
    );
}

// ============================================================================
// Light Mode Persistence (mod.rs)
// ============================================================================

#[test]
fn toggle_light_mode_persists_to_settings_key() {
    // Set a mock HOME dir to isolate config file I/O
    let temp = tempfile::tempdir().unwrap();
    unsafe {
        std::env::set_var("HOME", temp.path());
        std::env::set_var("XDG_CONFIG_HOME", temp.path().join(".config"));
    }

    // Initialize test app and ensure light mode is in a known state
    let mut app = test_app();
    crate::theme::set_light_mode(false);

    // Trigger the toggle handler
    let _ = app.update(crate::app_message::Message::ToggleLightMode);

    // Validate the config file was created and contains the correct key
    let actual_config_path = nokkvi_data::utils::paths::get_config_path().unwrap();
    let content = std::fs::read_to_string(&actual_config_path).unwrap_or_default();

    let doc = content
        .parse::<toml_edit::DocumentMut>()
        .expect("valid TOML");

    // The key MUST be written to [settings] light_mode
    assert!(
        doc.get("settings").is_some(),
        "[settings] table missing from config.toml. Current content:\n{content}"
    );
    assert!(
        doc["settings"].get("light_mode").is_some(),
        "light_mode missing from [settings]. Current content:\n{content}"
    );
    assert!(doc["settings"]["light_mode"].as_bool().unwrap());
}

#[test]
fn test_handle_radio_metadata_update() {
    let mut app = test_app();

    // Ensure we start with Queue playback
    assert!(app.active_playback.is_queue());

    // Switch to Radio playback
    let station = nokkvi_data::types::radio_station::RadioStation {
        id: "radio_1".into(),
        name: "Test Radio".into(),
        stream_url: "http://test".into(),
        home_page_url: None,
    };
    app.active_playback = crate::state::ActivePlayback::Radio(crate::state::RadioPlaybackState {
        station,
        icy_artist: None,
        icy_title: None,
        icy_url: None,
    });

    // Update metadata
    let _ = app.handle_radio_metadata_update(
        Some("Test Artist".to_string()),
        Some("Test Song".to_string()),
        None,
    );

    // Verify state mutation
    if let crate::state::ActivePlayback::Radio(state) = &app.active_playback {
        assert_eq!(state.icy_artist.as_deref(), Some("Test Artist"));
        assert_eq!(state.icy_title.as_deref(), Some("Test Song"));
    } else {
        panic!("Should still be in Radio playback state");
    }
}

#[test]
fn radios_play_filtered_station_plays_correct_station() {
    use crate::views::RadiosMessage;
    let mut app = test_app();
    app.current_view = crate::View::Radios;

    let s1 = nokkvi_data::types::radio_station::RadioStation {
        id: "r1".into(),
        name: "BBC Radio".into(),
        stream_url: "url3".into(),
        home_page_url: None,
    };
    let s2 = nokkvi_data::types::radio_station::RadioStation {
        id: "r2".into(),
        name: "SomaFM".into(),
        stream_url: "url1".into(),
        home_page_url: None,
    };

    app.library.radio_stations = vec![s1, s2];

    let _ = app.handle_radios(RadiosMessage::SearchQueryChanged("soma".to_string()));
    let _ = app.handle_radios(RadiosMessage::SlotListClickPlay(0));

    match &app.active_playback {
        crate::state::ActivePlayback::Radio(state) => {
            assert_eq!(
                state.station.name, "SomaFM",
                "Should play the filtered station, not the first station in unfiltered list"
            );
        }
        _ => panic!("Expected Radio playback"),
    }
}
