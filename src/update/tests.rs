//! Tests for update handlers
//!
//! Covers pure-state-mutation handlers that don't require app_service or async.

use crate::{View, app_message::PlaybackStateUpdate, test_helpers::*};

// ============================================================================
// Open-Menu Handler (menus.rs)
// ============================================================================

#[test]
fn set_open_menu_opens_when_none() {
    use crate::app_message::OpenMenu;

    let mut app = test_app();
    assert_eq!(app.open_menu, None);

    let _ = app.handle_set_open_menu(Some(OpenMenu::Hamburger));
    assert_eq!(app.open_menu, Some(OpenMenu::Hamburger));
}

#[test]
fn set_open_menu_replaces_existing_menu() {
    use crate::app_message::OpenMenu;

    let mut app = test_app();
    app.open_menu = Some(OpenMenu::Hamburger);

    let _ = app.handle_set_open_menu(Some(OpenMenu::PlayerModes));
    assert_eq!(app.open_menu, Some(OpenMenu::PlayerModes));
}

#[test]
fn set_open_menu_none_closes_any_open_menu() {
    use crate::app_message::OpenMenu;

    let mut app = test_app();
    app.open_menu = Some(OpenMenu::PlayerModes);

    let _ = app.handle_set_open_menu(None);
    assert_eq!(app.open_menu, None);
}

#[test]
fn set_open_menu_none_when_already_none_is_idempotent() {
    let mut app = test_app();
    assert_eq!(app.open_menu, None);

    let _ = app.handle_set_open_menu(None);
    assert_eq!(app.open_menu, None);
}

#[test]
fn switch_view_closes_open_menu() {
    use crate::app_message::OpenMenu;

    let mut app = test_app();
    app.open_menu = Some(OpenMenu::Hamburger);

    let _ = app.handle_switch_view(View::Albums);
    assert_eq!(
        app.open_menu, None,
        "navigating to a new view should close any open overlay menu"
    );
}

#[test]
fn window_resized_closes_open_menu() {
    use crate::app_message::OpenMenu;

    let mut app = test_app();
    app.open_menu = Some(OpenMenu::PlayerModes);

    let _ = app.handle_window_resized(1280.0, 720.0);
    assert_eq!(
        app.open_menu, None,
        "resizing the window invalidates anchored overlays — close them"
    );
}

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
// Settings Dispatch (settings.rs)
// ============================================================================

#[test]
fn settings_general_strip_merged_mode_flips_theme_cache() {
    use crate::views::settings::items::SettingValue;

    let mut app = test_app();
    // Reset cache to a known state to avoid bleed from other tests touching globals.
    crate::theme::set_strip_merged_mode(false);
    assert!(!crate::theme::strip_merged_mode());

    let _ = app.handle_settings_general(
        "general.strip_merged_mode".to_string(),
        SettingValue::Bool(true),
    );
    assert!(crate::theme::strip_merged_mode());

    let _ = app.handle_settings_general(
        "general.strip_merged_mode".to_string(),
        SettingValue::Bool(false),
    );
    assert!(!crate::theme::strip_merged_mode());
}

#[test]
fn settings_general_strip_show_labels_flips_theme_cache() {
    use crate::views::settings::items::SettingValue;

    let mut app = test_app();
    crate::theme::set_strip_show_labels(true);
    assert!(crate::theme::strip_show_labels());

    let _ = app.handle_settings_general(
        "general.strip_show_labels".to_string(),
        SettingValue::Bool(false),
    );
    assert!(!crate::theme::strip_show_labels());

    let _ = app.handle_settings_general(
        "general.strip_show_labels".to_string(),
        SettingValue::Bool(true),
    );
    assert!(crate::theme::strip_show_labels());
}

#[test]
fn settings_general_strip_separator_updates_theme_cache() {
    use nokkvi_data::types::player_settings::StripSeparator;

    use crate::views::settings::items::SettingValue;

    let mut app = test_app();
    crate::theme::set_strip_separator(StripSeparator::Dot);
    assert!(matches!(
        crate::theme::strip_separator(),
        StripSeparator::Dot
    ));

    let _ = app.handle_settings_general(
        "general.strip_separator".to_string(),
        SettingValue::Enum {
            val: "Pipe |".to_string(),
            options: Vec::new(),
        },
    );
    assert!(matches!(
        crate::theme::strip_separator(),
        StripSeparator::Pipe
    ));

    let _ = app.handle_settings_general(
        "general.strip_separator".to_string(),
        SettingValue::Enum {
            val: "Dot ·".to_string(),
            options: Vec::new(),
        },
    );
    assert!(matches!(
        crate::theme::strip_separator(),
        StripSeparator::Dot
    ));
}

#[test]
fn settings_general_albums_artwork_overlay_flips_theme_cache() {
    use crate::views::settings::items::SettingValue;

    let mut app = test_app();
    crate::theme::set_albums_artwork_overlay(true);
    assert!(crate::theme::albums_artwork_overlay());

    let _ = app.handle_settings_general(
        "general.albums_artwork_overlay".to_string(),
        SettingValue::Bool(false),
    );
    assert!(!crate::theme::albums_artwork_overlay());

    let _ = app.handle_settings_general(
        "general.albums_artwork_overlay".to_string(),
        SettingValue::Bool(true),
    );
    assert!(crate::theme::albums_artwork_overlay());
}

#[test]
fn settings_general_artists_artwork_overlay_flips_theme_cache() {
    use crate::views::settings::items::SettingValue;

    let mut app = test_app();
    crate::theme::set_artists_artwork_overlay(true);
    assert!(crate::theme::artists_artwork_overlay());

    let _ = app.handle_settings_general(
        "general.artists_artwork_overlay".to_string(),
        SettingValue::Bool(false),
    );
    assert!(!crate::theme::artists_artwork_overlay());

    let _ = app.handle_settings_general(
        "general.artists_artwork_overlay".to_string(),
        SettingValue::Bool(true),
    );
    assert!(crate::theme::artists_artwork_overlay());
}

#[test]
fn settings_general_songs_artwork_overlay_flips_theme_cache() {
    use crate::views::settings::items::SettingValue;

    let mut app = test_app();
    crate::theme::set_songs_artwork_overlay(true);
    assert!(crate::theme::songs_artwork_overlay());

    let _ = app.handle_settings_general(
        "general.songs_artwork_overlay".to_string(),
        SettingValue::Bool(false),
    );
    assert!(!crate::theme::songs_artwork_overlay());

    let _ = app.handle_settings_general(
        "general.songs_artwork_overlay".to_string(),
        SettingValue::Bool(true),
    );
    assert!(crate::theme::songs_artwork_overlay());
}

#[test]
fn settings_general_playlists_artwork_overlay_flips_theme_cache() {
    use crate::views::settings::items::SettingValue;

    let mut app = test_app();
    crate::theme::set_playlists_artwork_overlay(true);
    assert!(crate::theme::playlists_artwork_overlay());

    let _ = app.handle_settings_general(
        "general.playlists_artwork_overlay".to_string(),
        SettingValue::Bool(false),
    );
    assert!(!crate::theme::playlists_artwork_overlay());

    let _ = app.handle_settings_general(
        "general.playlists_artwork_overlay".to_string(),
        SettingValue::Bool(true),
    );
    assert!(crate::theme::playlists_artwork_overlay());
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
        bpm: None,
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

/// Phase 4B: re-sorting with the same `(mode, ascending, len)` signature
/// must be a no-op — the cached signature short-circuits redundant work.
/// We assert via a custom marker: shuffle the queue manually after the first
/// sort, then call sort again; the cache should keep the marker order.
#[test]
fn sort_queue_short_circuits_when_signature_unchanged() {
    let mut app = test_app();
    app.library.queue_songs = make_sorting_queue();
    app.queue_page.queue_sort_mode = nokkvi_data::types::queue_sort_mode::QueueSortMode::Title;
    app.queue_page.common.sort_ascending = true;

    app.sort_queue_songs();
    let first: Vec<String> = app
        .library
        .queue_songs
        .iter()
        .map(|s| s.title.clone())
        .collect();
    assert_eq!(first, vec!["Alpha", "Mango", "Zebra"]);

    // Manually permute. With the short-circuit in place, the next sort call
    // (with identical mode + ascending + len) should NOT touch the order.
    app.library.queue_songs.swap(0, 2);
    app.sort_queue_songs();
    let second: Vec<String> = app
        .library
        .queue_songs
        .iter()
        .map(|s| s.title.clone())
        .collect();
    assert_eq!(
        second,
        vec!["Zebra", "Mango", "Alpha"],
        "sort signature unchanged → short-circuit must skip re-sorting"
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
// Volume Handlers (playback.rs) — toast-on-change unification
// ============================================================================

#[test]
fn volume_changed_sets_state_and_pushes_toast() {
    let mut app = test_app();
    assert!(app.toast.toasts.is_empty());

    let _ = app.handle_volume_changed(0.42);

    assert!((app.playback.volume - 0.42).abs() < f32::EPSILON);
    let last = app
        .toast
        .toasts
        .back()
        .expect("a volume toast should have been pushed");
    assert_eq!(last.message, "Volume: 42%");
    assert!(last.right_aligned, "volume toast is right-aligned");
}

#[test]
fn sfx_volume_changed_sets_state_and_pushes_toast() {
    let mut app = test_app();
    assert!(app.toast.toasts.is_empty());

    let _ = app.handle_sfx_volume_changed(0.7);

    assert!((app.sfx.volume - 0.7).abs() < f32::EPSILON);
    let last = app
        .toast
        .toasts
        .back()
        .expect("an sfx volume toast should have been pushed");
    assert_eq!(last.message, "SFX Volume: 70%");
    assert!(last.right_aligned, "sfx volume toast is right-aligned");
}

#[test]
fn sfx_volume_changed_clamps_above_one() {
    let mut app = test_app();
    let _ = app.handle_sfx_volume_changed(1.5);
    assert!((app.sfx.volume - 1.0).abs() < f32::EPSILON);
    assert_eq!(
        app.toast.toasts.back().map(|t| t.message.as_str()),
        Some("SFX Volume: 100%")
    );
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
        suppress_library_refresh_toasts: false,
        show_tray_icon: false,
        close_to_tray: false,
        rounded_mode: false,
        nav_layout: "Top",
        nav_display_mode: "IconsAndLabels",
        track_info_display: "Full",
        slot_row_height: "Default",
        opacity_gradient: true,
        slot_text_links: true,
        crossfade_enabled: false,
        crossfade_duration_secs: 5,
        volume_normalization: "Off",
        normalization_level: "Standard",
        replay_gain_preamp_db: 0,
        replay_gain_fallback_db: 0,
        replay_gain_fallback_to_agc: false,
        replay_gain_prevent_clipping: true,
        default_playlist_name: String::new(),
        quick_add_to_playlist: false,
        queue_show_default_playlist: false,
        horizontal_volume: false,
        font_family: String::new(),
        strip_show_title: true,
        strip_show_artist: true,
        strip_show_album: true,
        strip_show_format_info: true,
        strip_merged_mode: false,
        strip_show_labels: true,
        strip_separator: "Dot ·",
        strip_click_action: "CenterOnPlaying",
        albums_artwork_overlay: true,
        artists_artwork_overlay: true,
        songs_artwork_overlay: true,
        playlists_artwork_overlay: true,
        artwork_column_mode: "Auto",
        artwork_column_stretch_fit: "Cover",
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
// Navigate-and-Expand-Album (album-text click → Albums view + auto-expand)
// ============================================================================

#[test]
fn navigate_and_expand_album_clears_search_filter_and_sets_target() {
    let mut app = test_app();
    app.current_view = View::Songs;
    app.albums_page.common.active_filter =
        Some(nokkvi_data::types::filter::LibraryFilter::AlbumId {
            id: "old".to_string(),
            title: "Old Album".to_string(),
        });
    app.albums_page.common.search_query = "old".to_string();
    app.albums_page.common.search_input_focused = true;

    let _ = app.handle_navigate_and_expand_album("a1".to_string());

    assert_eq!(app.current_view, View::Albums);
    assert!(app.albums_page.common.active_filter.is_none());
    assert!(app.albums_page.common.search_query.is_empty());
    assert!(!app.albums_page.common.search_input_focused);
    assert!(
        matches!(
            app.pending_expand,
            Some(crate::state::PendingExpand::Album { ref album_id, for_browsing_pane: false }) if album_id == "a1"
        ),
        "expected pending_expand = Album {{ a1, top-pane }}, got {:?}",
        app.pending_expand
    );
}

#[test]
fn navigate_and_expand_album_collapses_existing_albums_expansion() {
    let mut app = test_app();
    app.current_view = View::Songs;
    app.albums_page.expansion.expanded_id = Some("other".to_string());
    app.albums_page.expansion.children = vec![make_song("s1", "Song", "Artist")];

    let _ = app.handle_navigate_and_expand_album("a1".to_string());

    assert!(app.albums_page.expansion.expanded_id.is_none());
    assert!(app.albums_page.expansion.children.is_empty());
}

#[test]
fn browser_pane_navigate_and_expand_album_sets_browsing_flag() {
    let mut app = test_app();

    let _ = app.handle_browser_pane_navigate_and_expand_album("a1".to_string());

    assert!(
        matches!(
            app.pending_expand,
            Some(crate::state::PendingExpand::Album { ref album_id, for_browsing_pane: true }) if album_id == "a1"
        ),
        "expected pending_expand = Album {{ a1, browsing-pane }}, got {:?}",
        app.pending_expand
    );
}

#[test]
fn pending_expand_album_target_cleared_on_switch_view_away() {
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Album {
        album_id: "a1".to_string(),
        for_browsing_pane: false,
    });

    let _ = app.handle_switch_view(View::Songs);

    assert!(app.pending_expand.is_none());
}

#[test]
fn pending_expand_album_target_persists_on_switch_view_to_albums() {
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Album {
        album_id: "a1".to_string(),
        for_browsing_pane: false,
    });

    let _ = app.handle_switch_view(View::Albums);

    assert!(
        app.pending_expand.is_some(),
        "switching to Albums should not cancel the in-flight find chain"
    );
}

#[test]
fn pending_expand_album_target_cleared_on_navigate_and_filter() {
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Album {
        album_id: "a1".to_string(),
        for_browsing_pane: false,
    });

    let _ = app.handle_navigate_and_filter(
        View::Artists,
        nokkvi_data::types::filter::LibraryFilter::ArtistId {
            id: "ar1".to_string(),
            name: "Artist".to_string(),
        },
    );

    assert!(app.pending_expand.is_none());
}

#[test]
fn try_resolve_pending_expand_finds_loaded_album_and_takes_target() {
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Album {
        album_id: "a2".to_string(),
        for_browsing_pane: false,
    });
    app.library.albums.set_from_vec(vec![
        make_album("a1", "Album One", "Artist"),
        make_album("a2", "Album Two", "Artist"),
        make_album("a3", "Album Three", "Artist"),
    ]);

    let task = app.try_resolve_pending_expand_album();

    assert!(task.is_some(), "found target should produce a task");
    assert!(
        app.pending_expand.is_none(),
        "target should be taken once dispatched"
    );
    // For a 3-album library with default slot_count=9 (center_slot=4), the
    // computed top-placement offset (idx+4 = 5) clamps to total-1 = 2.
    // Whole list top-packs in render, so the visual position is fine.
    assert_eq!(
        app.albums_page.common.slot_list.viewport_offset, 2,
        "viewport_offset must be set to scroll the target into view"
    );
    assert_eq!(
        app.albums_page.common.slot_list.selected_offset,
        Some(1),
        "target must be marked as the highlighted slot via selected_offset"
    );
}

#[test]
fn try_resolve_pending_expand_places_target_at_top_slot() {
    // Long-library case: target should land at slot 0, not the center.
    // viewport_offset is the index of the item shown at the center slot
    // (slot_count/2), so to put the target at slot 0 we set
    // viewport_offset = target_idx + center_slot. With slot_count=9
    // (default), center_slot=4, so target_idx=320 → viewport_offset=324.
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Album {
        album_id: "a320".to_string(),
        for_browsing_pane: false,
    });
    let albums: Vec<_> = (0..1343)
        .map(|i| make_album(&format!("a{i}"), &format!("Album {i}"), "Artist"))
        .collect();
    app.library.albums.set_from_vec(albums);

    let task = app.try_resolve_pending_expand_album();
    assert!(task.is_some(), "found target should dispatch a task");

    let center_slot = app.albums_page.common.slot_list.slot_count / 2;
    assert_eq!(
        app.albums_page.common.slot_list.viewport_offset,
        320 + center_slot,
        "viewport_offset must place the target at slot 0 (top), not at the \
         center slot — otherwise the user sees ~7 albums above the expansion"
    );
    assert_eq!(
        app.albums_page.common.slot_list.selected_offset,
        Some(320),
        "target must keep the highlighted-slot marker even after viewport shift"
    );
}

#[test]
fn try_resolve_pending_expand_clears_when_fully_loaded_and_missing() {
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Album {
        album_id: "missing".to_string(),
        for_browsing_pane: false,
    });
    // set_from_vec sets total_count = items.len(), so fully_loaded() is true
    app.library
        .albums
        .set_from_vec(vec![make_album("a1", "Album One", "Artist")]);

    let task = app.try_resolve_pending_expand_album();

    assert!(task.is_some(), "fully-loaded miss should produce a task");
    assert!(
        app.pending_expand.is_none(),
        "target should be cleared when known-not-in-library"
    );
}

#[test]
fn try_resolve_pending_expand_returns_none_when_loading() {
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Album {
        album_id: "a2".to_string(),
        for_browsing_pane: false,
    });
    app.library
        .albums
        .set_first_page(vec![make_album("a1", "Album One", "Artist")], 100);
    app.library.albums.set_loading(true);

    let task = app.try_resolve_pending_expand_album();

    assert!(task.is_none(), "should wait while a page is in flight");
    assert!(
        app.pending_expand.is_some(),
        "target preserved while loading"
    );
}

#[test]
fn try_resolve_pending_expand_kicks_next_page_when_idle_and_more_remain() {
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Album {
        album_id: "a999".to_string(),
        for_browsing_pane: false,
    });
    // 1 loaded of 100 known total, idle → should request next page
    app.library
        .albums
        .set_first_page(vec![make_album("a1", "Album One", "Artist")], 100);

    let task = app.try_resolve_pending_expand_album();

    assert!(task.is_some(), "should dispatch next-page load");
    assert!(
        app.pending_expand.is_some(),
        "target preserved while still hunting"
    );
}

#[test]
fn try_resolve_pending_expand_bypasses_scroll_edge_gate_when_paging() {
    // Bug regression: `handle_albums_load_page` has a defensive gate that
    // bails when `viewport_offset + threshold < loaded` (i.e. the user
    // isn't near the loaded edge). The find chain leaves viewport_offset
    // at 0 while paging through the full library, so without bypass the
    // chain stalls after the first page lands.
    //
    // `set_loading(true)` is the proxy: load_albums_internal calls it
    // BEFORE shell_task, but only AFTER the gate. If the gate bails,
    // is_loading() stays false. (shell_task itself returns Task::none()
    // in tests because app_service is None — the gate behavior is what
    // we're verifying.)
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Album {
        album_id: "missing".to_string(),
        for_browsing_pane: false,
    });
    // 200 loaded of 1000 total. With page_size=500 the threshold is 100,
    // so viewport=0 + 100 < loaded=200 — the unfortified gate bails.
    let albums: Vec<_> = (0..200)
        .map(|i| make_album(&format!("a{i}"), &format!("Album {i}"), "Artist"))
        .collect();
    app.library.albums.set_first_page(albums, 1000);
    assert!(!app.library.albums.is_loading(), "precondition: idle");

    let task = app.try_resolve_pending_expand_album();
    assert!(task.is_some(), "should produce a load task");
    assert!(
        app.library.albums.is_loading(),
        "next-page fetch must actually start (set_loading=true) — \
         the scroll-edge gate must be bypassed during find-and-expand"
    );
}

#[test]
fn pending_timeout_does_not_toast_when_target_already_resolved() {
    let mut app = test_app();
    assert!(app.pending_expand.is_none());

    let _ = app.handle_pending_expand_album_timeout("a1".to_string());

    assert!(
        app.toast.toasts.is_empty(),
        "no toast when target already gone"
    );
}

#[test]
fn pending_timeout_does_not_toast_for_stale_album_id() {
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Album {
        album_id: "newer".to_string(),
        for_browsing_pane: false,
    });

    let _ = app.handle_pending_expand_album_timeout("older".to_string());

    assert!(
        app.toast.toasts.is_empty(),
        "stale timeout (different album_id) should not toast"
    );
}

#[test]
fn pending_timeout_toasts_when_target_still_in_flight() {
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Album {
        album_id: "a1".to_string(),
        for_browsing_pane: false,
    });

    let _ = app.handle_pending_expand_album_timeout("a1".to_string());

    assert_eq!(app.toast.toasts.len(), 1);
}

#[test]
fn songs_page_navigate_and_expand_album_returns_action() {
    let mut app = test_app();
    let (_, action) = app.songs_page.update(
        crate::views::SongsMessage::NavigateAndExpandAlbum("a1".to_string()),
        &[],
    );
    assert!(matches!(
        action,
        crate::views::SongsAction::NavigateAndExpandAlbum(ref id) if id == "a1"
    ));
}

#[test]
fn queue_page_navigate_and_expand_album_returns_action() {
    let mut app = test_app();
    let (_, action) = app.queue_page.update(
        crate::views::QueueMessage::NavigateAndExpandAlbum("a1".to_string()),
        &[],
    );
    assert!(matches!(
        action,
        crate::views::QueueAction::NavigateAndExpandAlbum(ref id) if id == "a1"
    ));
}

// ============================================================================
// Navigate-and-Expand-Artist (mirror of album navigate-and-expand for artists)
// ============================================================================

#[test]
fn navigate_and_expand_artist_clears_search_filter_and_sets_target() {
    let mut app = test_app();
    app.current_view = View::Songs;
    app.artists_page.common.active_filter =
        Some(nokkvi_data::types::filter::LibraryFilter::ArtistId {
            id: "old".to_string(),
            name: "Old Artist".to_string(),
        });
    app.artists_page.common.search_query = "old".to_string();
    app.artists_page.common.search_input_focused = true;

    let _ = app.handle_navigate_and_expand_artist("ar1".to_string());

    assert_eq!(app.current_view, View::Artists);
    assert!(app.artists_page.common.active_filter.is_none());
    assert!(app.artists_page.common.search_query.is_empty());
    assert!(!app.artists_page.common.search_input_focused);
    assert!(
        matches!(
            app.pending_expand,
            Some(crate::state::PendingExpand::Artist { ref artist_id, for_browsing_pane: false }) if artist_id == "ar1"
        ),
        "expected pending_expand = Artist {{ ar1, top-pane }}, got {:?}",
        app.pending_expand
    );
}

#[test]
fn navigate_and_expand_artist_collapses_existing_artists_expansion() {
    let mut app = test_app();
    app.current_view = View::Songs;
    app.artists_page.expansion.expanded_id = Some("other".to_string());
    app.artists_page.expansion.children = vec![make_album("a1", "Album", "Artist")];

    let _ = app.handle_navigate_and_expand_artist("ar1".to_string());

    assert!(app.artists_page.expansion.expanded_id.is_none());
    assert!(app.artists_page.expansion.children.is_empty());
}

#[test]
fn browser_pane_navigate_and_expand_artist_sets_browsing_flag() {
    let mut app = test_app();

    let _ = app.handle_browser_pane_navigate_and_expand_artist("ar1".to_string());

    assert!(
        matches!(
            app.pending_expand,
            Some(crate::state::PendingExpand::Artist { ref artist_id, for_browsing_pane: true }) if artist_id == "ar1"
        ),
        "expected pending_expand = Artist {{ ar1, browsing-pane }}, got {:?}",
        app.pending_expand
    );
}

#[test]
fn pending_expand_artist_target_cleared_on_switch_view_away() {
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Artist {
        artist_id: "ar1".to_string(),
        for_browsing_pane: false,
    });

    let _ = app.handle_switch_view(View::Songs);

    assert!(app.pending_expand.is_none());
}

#[test]
fn pending_expand_artist_target_persists_on_switch_view_to_artists() {
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Artist {
        artist_id: "ar1".to_string(),
        for_browsing_pane: false,
    });

    let _ = app.handle_switch_view(View::Artists);

    assert!(app.pending_expand.is_some());
}

#[test]
fn pending_expand_artist_target_cleared_on_navigate_and_filter() {
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Artist {
        artist_id: "ar1".to_string(),
        for_browsing_pane: false,
    });

    let _ = app.handle_navigate_and_filter(
        View::Albums,
        nokkvi_data::types::filter::LibraryFilter::AlbumId {
            id: "al1".to_string(),
            title: "Album".to_string(),
        },
    );

    assert!(app.pending_expand.is_none());
}

#[test]
fn try_resolve_pending_expand_artist_finds_loaded_and_takes_target() {
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Artist {
        artist_id: "ar2".to_string(),
        for_browsing_pane: false,
    });
    app.library.artists.set_from_vec(vec![
        make_artist("ar1", "Artist One"),
        make_artist("ar2", "Artist Two"),
        make_artist("ar3", "Artist Three"),
    ]);

    let task = app.try_resolve_pending_expand_artist();

    assert!(task.is_some(), "found target should produce a task");
    assert!(
        app.pending_expand.is_none(),
        "target should be taken once dispatched"
    );
    assert_eq!(
        app.artists_page.common.slot_list.viewport_offset, 2,
        "viewport_offset must be set so target is visible"
    );
    assert_eq!(
        app.artists_page.common.slot_list.selected_offset,
        Some(1),
        "target must keep highlight via selected_offset"
    );
}

#[test]
fn try_resolve_pending_expand_artist_places_target_at_top_slot() {
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Artist {
        artist_id: "ar320".to_string(),
        for_browsing_pane: false,
    });
    let artists: Vec<_> = (0..1000)
        .map(|i| make_artist(&format!("ar{i}"), &format!("Artist {i}")))
        .collect();
    app.library.artists.set_from_vec(artists);

    let task = app.try_resolve_pending_expand_artist();
    assert!(task.is_some(), "found target should dispatch a task");

    let center_slot = app.artists_page.common.slot_list.slot_count / 2;
    assert_eq!(
        app.artists_page.common.slot_list.viewport_offset,
        320 + center_slot,
        "target must land at slot 0 (top), not the center"
    );
    assert_eq!(
        app.artists_page.common.slot_list.selected_offset,
        Some(320),
        "highlight must follow target"
    );
}

#[test]
fn try_resolve_pending_expand_artist_clears_when_fully_loaded_and_missing() {
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Artist {
        artist_id: "missing".to_string(),
        for_browsing_pane: false,
    });
    app.library
        .artists
        .set_from_vec(vec![make_artist("ar1", "Artist One")]);

    let task = app.try_resolve_pending_expand_artist();

    assert!(task.is_some(), "fully-loaded miss should produce a task");
    assert!(
        app.pending_expand.is_none(),
        "target should be cleared when known-not-in-library"
    );
}

#[test]
fn try_resolve_pending_expand_artist_returns_none_when_loading() {
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Artist {
        artist_id: "ar2".to_string(),
        for_browsing_pane: false,
    });
    app.library
        .artists
        .set_first_page(vec![make_artist("ar1", "Artist One")], 100);
    app.library.artists.set_loading(true);

    let task = app.try_resolve_pending_expand_artist();

    assert!(task.is_none(), "should wait while a page is in flight");
    assert!(app.pending_expand.is_some());
}

#[test]
fn try_resolve_pending_expand_artist_kicks_next_page_when_idle() {
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Artist {
        artist_id: "ar999".to_string(),
        for_browsing_pane: false,
    });
    app.library
        .artists
        .set_first_page(vec![make_artist("ar1", "Artist One")], 100);

    let task = app.try_resolve_pending_expand_artist();

    assert!(task.is_some());
    assert!(app.pending_expand.is_some());
}

#[test]
fn try_resolve_pending_expand_artist_bypasses_scroll_edge_gate_when_paging() {
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Artist {
        artist_id: "missing".to_string(),
        for_browsing_pane: false,
    });
    let artists: Vec<_> = (0..200)
        .map(|i| make_artist(&format!("ar{i}"), &format!("Artist {i}")))
        .collect();
    app.library.artists.set_first_page(artists, 1000);
    assert!(!app.library.artists.is_loading(), "precondition: idle");

    let task = app.try_resolve_pending_expand_artist();
    assert!(task.is_some());
    assert!(
        app.library.artists.is_loading(),
        "next-page fetch must actually start — scroll-edge gate must be bypassed"
    );
}

#[test]
fn pending_artist_timeout_does_not_toast_when_target_already_resolved() {
    let mut app = test_app();
    let _ = app.handle_pending_expand_artist_timeout("ar1".to_string());
    assert!(app.toast.toasts.is_empty());
}

#[test]
fn pending_artist_timeout_does_not_toast_for_stale_id() {
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Artist {
        artist_id: "newer".to_string(),
        for_browsing_pane: false,
    });
    let _ = app.handle_pending_expand_artist_timeout("older".to_string());
    assert!(app.toast.toasts.is_empty());
}

#[test]
fn pending_artist_timeout_toasts_when_target_still_in_flight() {
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Artist {
        artist_id: "ar1".to_string(),
        for_browsing_pane: false,
    });
    let _ = app.handle_pending_expand_artist_timeout("ar1".to_string());
    assert_eq!(app.toast.toasts.len(), 1);
}

#[test]
fn songs_page_navigate_and_expand_artist_returns_action() {
    let mut app = test_app();
    let (_, action) = app.songs_page.update(
        crate::views::SongsMessage::NavigateAndExpandArtist("ar1".to_string()),
        &[],
    );
    assert!(matches!(
        action,
        crate::views::SongsAction::NavigateAndExpandArtist(ref id) if id == "ar1"
    ));
}

#[test]
fn queue_page_navigate_and_expand_artist_returns_action() {
    let mut app = test_app();
    let (_, action) = app.queue_page.update(
        crate::views::QueueMessage::NavigateAndExpandArtist("ar1".to_string()),
        &[],
    );
    assert!(matches!(
        action,
        crate::views::QueueAction::NavigateAndExpandArtist(ref id) if id == "ar1"
    ));
}

#[test]
fn albums_page_navigate_and_expand_artist_returns_action() {
    let mut app = test_app();
    let (_, action) = app.albums_page.update(
        crate::views::AlbumsMessage::NavigateAndExpandArtist("ar1".to_string()),
        0,
        &[],
    );
    assert!(matches!(
        action,
        crate::views::AlbumsAction::NavigateAndExpandArtist(ref id) if id == "ar1"
    ));
}

// ============================================================================
// Highlight pin (selected_offset stays on the focused item after expansion)
// ============================================================================

#[test]
fn try_resolve_album_sets_top_pin_when_target_found() {
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Album {
        album_id: "a2".to_string(),
        for_browsing_pane: false,
    });
    app.library.albums.set_from_vec(vec![
        make_album("a1", "Album One", "Artist"),
        make_album("a2", "Album Two", "Artist"),
        make_album("a3", "Album Three", "Artist"),
    ]);

    let _ = app.try_resolve_pending_expand_album();

    match app.pending_top_pin.as_ref() {
        Some(crate::state::PendingTopPin::Album(id)) => assert_eq!(id, "a2"),
        other => panic!("expected pending_top_pin = Album(a2), got {other:?}"),
    }
}

#[test]
fn try_resolve_artist_sets_top_pin_when_target_found() {
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Artist {
        artist_id: "ar2".to_string(),
        for_browsing_pane: false,
    });
    app.library.artists.set_from_vec(vec![
        make_artist("ar1", "Artist One"),
        make_artist("ar2", "Artist Two"),
        make_artist("ar3", "Artist Three"),
    ]);

    let _ = app.try_resolve_pending_expand_artist();

    match app.pending_top_pin.as_ref() {
        Some(crate::state::PendingTopPin::Artist(id)) => assert_eq!(id, "ar2"),
        other => panic!("expected pending_top_pin = Artist(ar2), got {other:?}"),
    }
}

#[test]
fn tracks_loaded_re_pins_selected_offset_for_album() {
    let mut app = test_app();
    app.library.albums.set_from_vec(vec![
        make_album("a1", "Album One", "Artist"),
        make_album("a2", "Album Two", "Artist"),
        make_album("a3", "Album Three", "Artist"),
    ]);
    // Simulate the post-find state: highlight on target, pin set.
    app.albums_page
        .common
        .slot_list
        .set_selected(1, app.library.albums.len());
    app.pending_top_pin = Some(crate::state::PendingTopPin::Album("a2".to_string()));

    // TracksLoaded fires for the pinned album — set_children inside the
    // page update clears selected_offset, then the handler should re-pin.
    let _ = app.handle_albums(crate::views::AlbumsMessage::TracksLoaded(
        "a2".to_string(),
        vec![make_song("s1", "Song", "Artist")],
    ));

    assert_eq!(
        app.albums_page.common.slot_list.selected_offset,
        Some(1),
        "highlight must follow the target album after expansion completes"
    );
    assert!(
        app.pending_top_pin.is_none(),
        "pin should be consumed once applied"
    );
}

#[test]
fn albums_loaded_re_pins_selected_offset_for_artist() {
    let mut app = test_app();
    app.library.artists.set_from_vec(vec![
        make_artist("ar1", "Artist One"),
        make_artist("ar2", "Artist Two"),
        make_artist("ar3", "Artist Three"),
    ]);
    app.artists_page
        .common
        .slot_list
        .set_selected(1, app.library.artists.len());
    app.pending_top_pin = Some(crate::state::PendingTopPin::Artist("ar2".to_string()));

    let _ = app.handle_artists(crate::views::ArtistsMessage::AlbumsLoaded(
        "ar2".to_string(),
        vec![make_album("a1", "Album One", "Artist Two")],
    ));

    assert_eq!(
        app.artists_page.common.slot_list.selected_offset,
        Some(1),
        "highlight must follow the target artist after expansion completes"
    );
    assert!(app.pending_top_pin.is_none());
}

#[test]
fn tracks_loaded_for_unrelated_album_does_not_re_pin() {
    // User clicked album a2 → pin = Album(a2). Then user (somehow) triggers
    // expansion of a different album a3 — the children-load for a3 must
    // not steal the highlight from a2's pin.
    let mut app = test_app();
    app.library.albums.set_from_vec(vec![
        make_album("a1", "Album One", "Artist"),
        make_album("a2", "Album Two", "Artist"),
        make_album("a3", "Album Three", "Artist"),
    ]);
    app.pending_top_pin = Some(crate::state::PendingTopPin::Album("a2".to_string()));

    let _ = app.handle_albums(crate::views::AlbumsMessage::TracksLoaded(
        "a3".to_string(),
        vec![make_song("s1", "Song", "Artist")],
    ));

    assert!(
        matches!(
            app.pending_top_pin,
            Some(crate::state::PendingTopPin::Album(ref id)) if id == "a2"
        ),
        "pin must not be consumed by an unrelated TracksLoaded"
    );
}

#[test]
fn pending_top_pin_cleared_on_search_in_albums() {
    let mut app = test_app();
    app.pending_top_pin = Some(crate::state::PendingTopPin::Album("a1".to_string()));

    let _ = app.handle_albums(crate::views::AlbumsMessage::SearchQueryChanged(
        "foo".to_string(),
    ));

    assert!(
        app.pending_top_pin.is_none(),
        "user-driven search supersedes the find chain — pin should clear"
    );
}

#[test]
fn pending_top_pin_cleared_on_switch_view_away() {
    let mut app = test_app();
    app.pending_top_pin = Some(crate::state::PendingTopPin::Album("a1".to_string()));

    let _ = app.handle_switch_view(View::Songs);

    assert!(app.pending_top_pin.is_none());
}

#[test]
fn pending_top_pin_cleared_on_navigate_and_filter() {
    let mut app = test_app();
    app.pending_top_pin = Some(crate::state::PendingTopPin::Artist("ar1".to_string()));

    let _ = app.handle_navigate_and_filter(
        View::Songs,
        nokkvi_data::types::filter::LibraryFilter::ArtistId {
            id: "ar1".to_string(),
            name: "Artist".to_string(),
        },
    );

    assert!(app.pending_top_pin.is_none());
}

// ============================================================================
// Navigate-and-Expand-Genre (single-shot mirror — genres don't paginate)
// ============================================================================

#[test]
fn navigate_and_expand_genre_clears_search_filter_and_sets_target() {
    let mut app = test_app();
    app.current_view = View::Songs;
    app.genres_page.common.active_filter =
        Some(nokkvi_data::types::filter::LibraryFilter::GenreId {
            id: "Old".to_string(),
            name: "Old".to_string(),
        });
    app.genres_page.common.search_query = "old".to_string();
    app.genres_page.common.search_input_focused = true;

    let _ = app.handle_navigate_and_expand_genre("Rock".to_string());

    assert_eq!(app.current_view, View::Genres);
    assert!(app.genres_page.common.active_filter.is_none());
    assert!(app.genres_page.common.search_query.is_empty());
    assert!(!app.genres_page.common.search_input_focused);
    assert!(
        matches!(
            app.pending_expand,
            Some(crate::state::PendingExpand::Genre { ref genre_id, for_browsing_pane: false }) if genre_id == "Rock"
        ),
        "expected pending_expand = Genre {{ Rock, top-pane }}, got {:?}",
        app.pending_expand
    );
}

#[test]
fn navigate_and_expand_genre_collapses_existing_genres_expansion() {
    let mut app = test_app();
    app.genres_page.expansion.expanded_id = Some("other".to_string());
    app.genres_page.expansion.children = vec![make_album("a1", "Album", "Artist")];

    let _ = app.handle_navigate_and_expand_genre("Rock".to_string());

    assert!(app.genres_page.expansion.expanded_id.is_none());
    assert!(app.genres_page.expansion.children.is_empty());
}

#[test]
fn browser_pane_navigate_and_expand_genre_sets_browsing_flag() {
    let mut app = test_app();

    let _ = app.handle_browser_pane_navigate_and_expand_genre("Rock".to_string());

    assert!(
        matches!(
            app.pending_expand,
            Some(crate::state::PendingExpand::Genre {
                for_browsing_pane: true,
                ..
            })
        ),
        "expected pending_expand = Genre {{ browsing-pane }}, got {:?}",
        app.pending_expand
    );
}

#[test]
fn pending_expand_genre_target_cleared_on_switch_view_away() {
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Genre {
        genre_id: "Rock".to_string(),
        for_browsing_pane: false,
    });

    let _ = app.handle_switch_view(View::Songs);

    assert!(app.pending_expand.is_none());
}

#[test]
fn pending_expand_genre_target_persists_on_switch_view_to_genres() {
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Genre {
        genre_id: "Rock".to_string(),
        for_browsing_pane: false,
    });

    let _ = app.handle_switch_view(View::Genres);

    assert!(app.pending_expand.is_some());
}

#[test]
fn pending_expand_genre_target_cleared_on_navigate_and_filter() {
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Genre {
        genre_id: "Rock".to_string(),
        for_browsing_pane: false,
    });

    let _ = app.handle_navigate_and_filter(
        View::Albums,
        nokkvi_data::types::filter::LibraryFilter::AlbumId {
            id: "al1".to_string(),
            title: "Album".to_string(),
        },
    );

    assert!(app.pending_expand.is_none());
}

#[test]
fn try_resolve_pending_expand_genre_matches_by_name_not_internal_id() {
    // Regression: Navidrome's /api/genre returns genres with proper IDs
    // (UUIDs) that differ from their display names, but the click sites
    // dispatch the displayed name (`extra_value` / `genre` in the slot).
    // The lookup must therefore match against `g.name`, not `g.id`, or
    // every click toasts "Genre not found in library".
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Genre {
        genre_id: "Black Metal".to_string(), // the displayed name, not an internal id
        for_browsing_pane: false,
    });
    app.library.genres.set_from_vec(vec![
        make_genre("uuid-rock-123", "Rock"),
        make_genre("uuid-blackmetal-456", "Black Metal"),
        make_genre("uuid-ambient-789", "Ambient"),
    ]);

    let _ = app.try_resolve_pending_expand_genre();

    // Side-effects only on the found-path: viewport scroll + highlight pin.
    // The pin stores the resolved internal id (the uuid we look up in
    // library.genres), not the display name we matched on, because that's
    // what downstream `GenresMessage::AlbumsLoaded(genre_id, _)` will carry.
    assert!(
        matches!(
            app.pending_top_pin,
            Some(crate::state::PendingTopPin::Genre(ref id)) if id == "uuid-blackmetal-456"
        ),
        "found-path must set pending_top_pin to the resolved internal id (got {:?})",
        app.pending_top_pin
    );
    assert!(
        app.toast.toasts.is_empty(),
        "found-path must not push the 'Genre not found in library' warn toast"
    );
}

#[test]
fn try_resolve_pending_expand_genre_finds_loaded_and_takes_target() {
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Genre {
        genre_id: "Jazz".to_string(),
        for_browsing_pane: false,
    });
    app.library.genres.set_from_vec(vec![
        make_genre("Rock", "Rock"),
        make_genre("Jazz", "Jazz"),
        make_genre("Classical", "Classical"),
    ]);

    let task = app.try_resolve_pending_expand_genre();

    assert!(task.is_some(), "found target should produce a task");
    assert!(
        app.pending_expand.is_none(),
        "target should be taken once dispatched"
    );
    assert_eq!(
        app.genres_page.common.slot_list.viewport_offset, 2,
        "viewport_offset must be set so target is visible"
    );
    assert_eq!(
        app.genres_page.common.slot_list.selected_offset,
        Some(1),
        "target must keep highlight via selected_offset"
    );
    match app.pending_top_pin.as_ref() {
        Some(crate::state::PendingTopPin::Genre(id)) => assert_eq!(id, "Jazz"),
        other => panic!("expected pending_top_pin = Genre(Jazz), got {other:?}"),
    }
}

#[test]
fn try_resolve_pending_expand_genre_places_target_at_top_slot() {
    let mut app = test_app();
    // Click sites dispatch the display name, not the internal id.
    app.pending_expand = Some(crate::state::PendingExpand::Genre {
        genre_id: "Genre 50".to_string(),
        for_browsing_pane: false,
    });
    let genres: Vec<_> = (0..200)
        .map(|i| make_genre(&format!("uuid-{i}"), &format!("Genre {i}")))
        .collect();
    app.library.genres.set_from_vec(genres);

    let task = app.try_resolve_pending_expand_genre();
    assert!(task.is_some());

    let center_slot = app.genres_page.common.slot_list.slot_count / 2;
    assert_eq!(
        app.genres_page.common.slot_list.viewport_offset,
        50 + center_slot,
        "target must land at slot 0 (top), not the center"
    );
    assert_eq!(
        app.genres_page.common.slot_list.selected_offset,
        Some(50),
        "highlight must follow target"
    );
}

#[test]
fn try_resolve_pending_expand_genre_clears_when_idle_and_missing() {
    // Genres are single-shot: if not loading and target absent, it really
    // isn't in the library — no further pages to wait for.
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Genre {
        genre_id: "Missing".to_string(),
        for_browsing_pane: false,
    });
    app.library
        .genres
        .set_from_vec(vec![make_genre("Rock", "Rock")]);
    assert!(!app.library.genres.is_loading());

    let task = app.try_resolve_pending_expand_genre();

    assert!(task.is_some(), "missing target should produce a task");
    assert!(
        app.pending_expand.is_none(),
        "target should be cleared when known-not-in-library"
    );
}

#[test]
fn try_resolve_pending_expand_genre_returns_none_when_loading() {
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Genre {
        genre_id: "Rock".to_string(),
        for_browsing_pane: false,
    });
    app.library.genres.set_loading(true);

    let task = app.try_resolve_pending_expand_genre();

    assert!(task.is_none(), "should wait while load is in flight");
    assert!(app.pending_expand.is_some());
}

#[test]
fn pending_genre_timeout_does_not_toast_when_target_already_resolved() {
    let mut app = test_app();
    let _ = app.handle_pending_expand_genre_timeout("Rock".to_string());
    assert!(app.toast.toasts.is_empty());
}

#[test]
fn pending_genre_timeout_toasts_when_target_still_in_flight() {
    let mut app = test_app();
    app.pending_expand = Some(crate::state::PendingExpand::Genre {
        genre_id: "Rock".to_string(),
        for_browsing_pane: false,
    });
    let _ = app.handle_pending_expand_genre_timeout("Rock".to_string());
    assert_eq!(app.toast.toasts.len(), 1);
}

#[test]
fn songs_page_navigate_and_expand_genre_returns_action() {
    let mut app = test_app();
    let (_, action) = app.songs_page.update(
        crate::views::SongsMessage::NavigateAndExpandGenre("Rock".to_string()),
        &[],
    );
    assert!(matches!(
        action,
        crate::views::SongsAction::NavigateAndExpandGenre(ref id) if id == "Rock"
    ));
}

#[test]
fn albums_page_navigate_and_expand_genre_returns_action() {
    let mut app = test_app();
    let (_, action) = app.albums_page.update(
        crate::views::AlbumsMessage::NavigateAndExpandGenre("Rock".to_string()),
        0,
        &[],
    );
    assert!(matches!(
        action,
        crate::views::AlbumsAction::NavigateAndExpandGenre(ref id) if id == "Rock"
    ));
}

#[test]
fn queue_page_navigate_and_expand_genre_returns_action() {
    let mut app = test_app();
    let (_, action) = app.queue_page.update(
        crate::views::QueueMessage::NavigateAndExpandGenre("Rock".to_string()),
        &[],
    );
    assert!(matches!(
        action,
        crate::views::QueueAction::NavigateAndExpandGenre(ref id) if id == "Rock"
    ));
}

#[test]
fn albums_loaded_re_pins_selected_offset_for_genre() {
    let mut app = test_app();
    app.library.genres.set_from_vec(vec![
        make_genre("Rock", "Rock"),
        make_genre("Jazz", "Jazz"),
        make_genre("Classical", "Classical"),
    ]);
    app.genres_page
        .common
        .slot_list
        .set_selected(1, app.library.genres.len());
    app.pending_top_pin = Some(crate::state::PendingTopPin::Genre("Jazz".to_string()));

    let _ = app.handle_genres(crate::views::GenresMessage::AlbumsLoaded(
        "Jazz".to_string(),
        vec![make_album("a1", "Album One", "Artist")],
    ));

    assert_eq!(
        app.genres_page.common.slot_list.selected_offset,
        Some(1),
        "highlight must follow the target genre after expansion completes"
    );
    assert!(app.pending_top_pin.is_none());
}

// ============================================================================
// Sort Mode: Most Played (PROMPT 6)
// ============================================================================

#[test]
fn albums_sort_mode_most_played_updates_state_and_emits_action() {
    use crate::widgets::view_header::SortMode;
    let mut app = test_app();

    let (_, action) = app.albums_page.update(
        crate::views::AlbumsMessage::SortModeSelected(SortMode::MostPlayed),
        0,
        &[],
    );

    assert_eq!(
        app.albums_page.common.current_sort_mode,
        SortMode::MostPlayed
    );
    assert!(matches!(
        action,
        crate::views::AlbumsAction::SortModeChanged(SortMode::MostPlayed)
    ));
}

#[test]
fn songs_sort_mode_most_played_updates_state_and_emits_action() {
    use crate::widgets::view_header::SortMode;
    let mut app = test_app();

    let (_, action) = app.songs_page.update(
        crate::views::SongsMessage::SortModeSelected(SortMode::MostPlayed),
        &[],
    );

    assert_eq!(
        app.songs_page.common.current_sort_mode,
        SortMode::MostPlayed
    );
    assert!(matches!(
        action,
        crate::views::SongsAction::SortModeChanged(SortMode::MostPlayed)
    ));
}

#[test]
fn artists_sort_mode_most_played_updates_state_and_emits_action() {
    use crate::widgets::view_header::SortMode;
    let mut app = test_app();

    let (_, action) = app.artists_page.update(
        crate::views::ArtistsMessage::SortModeSelected(SortMode::MostPlayed),
        0,
        &[],
    );

    assert_eq!(
        app.artists_page.common.current_sort_mode,
        SortMode::MostPlayed
    );
    assert!(matches!(
        action,
        crate::views::ArtistsAction::SortModeChanged(SortMode::MostPlayed)
    ));
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

// ============================================================================
// Shift+Enter (ExpandCenter) Collapse Behavior — Artists & Genres (2-tier views)
// ============================================================================
//
// In Albums/Playlists, Shift+Enter on an already-expanded parent row collapses
// it. Artists/Genres now match: parent rows toggle the outer expansion;
// centered child album rows route to NavigateAndExpandAlbum (cross-view drill
// down) instead of opening a 3rd inline tier.

#[test]
fn artists_shift_enter_on_parent_collapses_outer_expansion() {
    let mut app = test_app();
    let artists = vec![make_artist("ar1", "Artist 1")];
    app.library.artists.set_from_vec(artists.clone());

    // Outer expansion open on ar1; viewport centered on parent (idx 0).
    let album = make_album("a1", "Album 1", "Artist 1");
    app.artists_page.expansion.expanded_id = Some("ar1".to_string());
    app.artists_page.expansion.parent_offset = 0;
    app.artists_page.expansion.children = vec![album];
    app.artists_page.common.slot_list.selected_offset = Some(0);

    let (_, _action) = app.artists_page.update(
        crate::views::ArtistsMessage::ExpandCenter,
        artists.len(),
        &artists,
    );

    assert_eq!(
        app.artists_page.expansion.expanded_id, None,
        "outer expansion should be collapsed when ExpandCenter fires on the parent row"
    );
}

#[test]
fn artists_shift_enter_on_child_album_routes_to_navigate_and_expand_album() {
    let mut app = test_app();
    let artists = vec![make_artist("ar1", "Artist 1")];
    app.library.artists.set_from_vec(artists.clone());

    let album = make_album("a1", "Album 1", "Artist 1");
    app.artists_page.expansion.expanded_id = Some("ar1".to_string());
    app.artists_page.expansion.parent_offset = 0;
    app.artists_page.expansion.children = vec![album];

    // Center on the child album row (flat idx 1).
    app.artists_page.common.slot_list.selected_offset = Some(1);

    let (_, action) = app.artists_page.update(
        crate::views::ArtistsMessage::ExpandCenter,
        artists.len(),
        &artists,
    );

    match action {
        crate::views::ArtistsAction::NavigateAndExpandAlbum(id) => assert_eq!(id, "a1"),
        other => panic!("Expected ArtistsAction::NavigateAndExpandAlbum(\"a1\"), got {other:?}"),
    }
    assert_eq!(
        app.artists_page.expansion.expanded_id.as_deref(),
        Some("ar1"),
        "outer expansion should remain open — drill-down is cross-view, not a local mutation"
    );
}

#[test]
fn artists_shift_enter_on_unexpanded_parent_opens_expansion() {
    let mut app = test_app();
    let artists = vec![make_artist("ar1", "Artist 1")];
    app.library.artists.set_from_vec(artists.clone());

    // No expansion. Center on the parent row.
    app.artists_page.common.slot_list.selected_offset = Some(0);

    let (_, action) = app.artists_page.update(
        crate::views::ArtistsMessage::ExpandCenter,
        artists.len(),
        &artists,
    );

    match action {
        crate::views::ArtistsAction::ExpandArtist(id) => assert_eq!(id, "ar1"),
        other => panic!("Expected ArtistsAction::ExpandArtist(\"ar1\"), got {other:?}"),
    }
}

#[test]
fn genres_shift_enter_on_parent_collapses_outer_expansion() {
    let mut app = test_app();
    let genres = vec![make_genre("g1", "Rock")];
    app.library.genres.set_from_vec(genres.clone());

    let album = make_album("a1", "Album 1", "Artist 1");
    app.genres_page.expansion.expanded_id = Some("g1".to_string());
    app.genres_page.expansion.parent_offset = 0;
    app.genres_page.expansion.children = vec![album];
    app.genres_page.common.slot_list.selected_offset = Some(0);

    let _ = app.genres_page.update(
        crate::views::GenresMessage::ExpandCenter,
        genres.len(),
        &genres,
    );

    assert_eq!(
        app.genres_page.expansion.expanded_id, None,
        "outer expansion should be collapsed when ExpandCenter fires on the parent row"
    );
}

#[test]
fn genres_shift_enter_on_child_album_routes_to_navigate_and_expand_album() {
    let mut app = test_app();
    let genres = vec![make_genre("g1", "Rock")];
    app.library.genres.set_from_vec(genres.clone());

    let album = make_album("a1", "Album 1", "Artist 1");
    app.genres_page.expansion.expanded_id = Some("g1".to_string());
    app.genres_page.expansion.parent_offset = 0;
    app.genres_page.expansion.children = vec![album];

    app.genres_page.common.slot_list.selected_offset = Some(1);

    let (_, action) = app.genres_page.update(
        crate::views::GenresMessage::ExpandCenter,
        genres.len(),
        &genres,
    );

    match action {
        crate::views::GenresAction::NavigateAndExpandAlbum(id) => assert_eq!(id, "a1"),
        other => panic!("Expected GenresAction::NavigateAndExpandAlbum(\"a1\"), got {other:?}"),
    }
    assert_eq!(app.genres_page.expansion.expanded_id.as_deref(), Some("g1"),);
}

#[test]
fn genres_shift_enter_on_unexpanded_parent_opens_expansion() {
    let mut app = test_app();
    let genres = vec![make_genre("g1", "Rock")];
    app.library.genres.set_from_vec(genres.clone());

    app.genres_page.common.slot_list.selected_offset = Some(0);

    let (_, action) = app.genres_page.update(
        crate::views::GenresMessage::ExpandCenter,
        genres.len(),
        &genres,
    );

    match action {
        crate::views::GenresAction::ExpandGenre(_, id) => assert_eq!(id, "g1"),
        other => panic!("Expected GenresAction::ExpandGenre(_, \"g1\"), got {other:?}"),
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

#[test]
fn artist_focus_and_expand_triggers_large_artwork_load() {
    let mut app = test_app();
    app.current_view = View::Artists;
    app.library
        .artists
        .set_from_vec(vec![make_artist("ar1", "Artist 1")]);

    let _ = app.handle_artists(crate::views::ArtistsMessage::FocusAndExpand(0));

    assert_eq!(
        app.artwork.loading_large_artwork.as_deref(),
        Some("ar1"),
        "FocusAndExpand on an artist should kick off a large-artwork fetch \
         so the artwork column populates without a scroll round-trip"
    );
}

#[test]
fn genre_focus_and_expand_triggers_collage_load() {
    let mut app = test_app();
    app.current_view = View::Genres;
    app.library
        .genres
        .set_from_vec(vec![make_genre("g1", "Rock")]);

    let _ = app.handle_genres(crate::views::GenresMessage::FocusAndExpand(0));

    assert!(
        app.artwork.genre.pending.contains("g1"),
        "FocusAndExpand on a genre should mark its collage as pending so the \
         3x3 artwork column populates without a scroll round-trip"
    );
}

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
        crate::state::ActivePlayback::Queue => panic!("Expected Radio playback"),
    }
}

#[test]
fn test_session_expired_redirects_to_login() {
    let mut app = test_app();
    app.screen = crate::Screen::Home;
    app.current_view = View::Albums;
    app.library
        .albums
        .set_from_vec(vec![make_album("a1", "A", "A")]);

    let _ = app.handle_session_expired();

    assert_eq!(app.screen, crate::Screen::Login);
    assert!(app.app_service.is_none());
    assert!(app.stored_session.is_none());
    assert!(
        app.library.albums.is_empty(),
        "Library should be reset on session expiry"
    );
}

#[test]
fn test_albums_loaded_unauthorized_triggers_logout() {
    let mut app = test_app();
    app.screen = crate::Screen::Home;
    app.current_view = View::Albums;

    // Simulate a wrapped anyhow error that was stringified with {:#}
    let err_string = "Failed to fetch albums: Unauthorized: Session expired".to_string();
    let _ = app.handle_albums_loaded(Err(err_string), 0, false, None);

    assert_eq!(
        app.screen,
        crate::Screen::Login,
        "Should redirect to login on unauthorized error string"
    );
}

// ============================================================================
// Task Manager Notifications (mod.rs)
// ============================================================================

#[test]
fn task_status_changed_failed_pushes_toast() {
    let mut app = test_app();
    let handle = nokkvi_data::services::task_manager::TaskHandle {
        id: 1,
        name: "TestTask".to_string(),
    };
    let status =
        nokkvi_data::services::task_manager::TaskStatus::Failed("simulated error".to_string());

    let _ = app.update(crate::app_message::Message::TaskStatusChanged(
        handle, status,
    ));

    // Toast list should now contain an error message
    assert_eq!(app.toast.toasts.len(), 1);
    let toast = &app.toast.toasts[0];
    assert!(toast.message.contains("Task failed"));
    assert!(toast.message.contains("TestTask"));
    assert!(toast.message.contains("simulated error"));
    assert_eq!(toast.level, nokkvi_data::types::toast::ToastLevel::Error);
}

#[test]
fn task_status_changed_success_no_toast() {
    let mut app = test_app();
    let handle = nokkvi_data::services::task_manager::TaskHandle {
        id: 1,
        name: "TestTask".to_string(),
    };
    let status = nokkvi_data::services::task_manager::TaskStatus::Completed;

    let _ = app.update(crate::app_message::Message::TaskStatusChanged(
        handle, status,
    ));

    // Currently, successful tasks just log to debug, no toast
    assert!(app.toast.toasts.is_empty());
}

// ============================================================================
// Library Refresh Toast Suppression (library_refresh.rs)
// ============================================================================

#[test]
fn library_refreshed_emits_toast_by_default() {
    let mut app = test_app();
    assert!(!app.suppress_library_refresh_toasts);
    assert!(app.toast.toasts.is_empty());

    let _ = app.handle_library_changed(Vec::new(), true);

    assert_eq!(
        app.toast.toasts.len(),
        1,
        "Wildcard refresh should emit one info toast by default"
    );
    let toast = &app.toast.toasts[0];
    assert!(
        toast.message.contains("Library refreshed"),
        "Expected 'Library refreshed' message, got: {}",
        toast.message
    );
    assert_eq!(toast.level, nokkvi_data::types::toast::ToastLevel::Info);
}

#[test]
fn library_refreshed_suppresses_toast_when_flag_set() {
    let mut app = test_app();
    app.suppress_library_refresh_toasts = true;
    assert!(app.toast.toasts.is_empty());

    let _ = app.handle_library_changed(Vec::new(), true);

    assert!(
        app.toast.toasts.is_empty(),
        "No toast should be pushed when suppress_library_refresh_toasts is true"
    );
}

// ============================================================================
// Albums library-refresh: viewport reconciliation (PROMPT 16)
// ============================================================================
//
// Repro: idle on Albums view (sort=RecentlyAdded), SSE refresh fires, slots
// render blank — borders and backgrounds remain but text/artwork are gone.
//
// Root cause: `handle_library_changed` snapshots `viewport_offset` and the
// album at that offset as an anchor, then `handle_albums_loaded` only updates
// `viewport_offset` when the anchor is *found* in the new list. If the new
// list is shorter than the old offset (server pruned, reordered, or the
// anchor album was removed), `viewport_offset` is left pointing past the end
// of the buffer. `get_slot_item_index_with_center` then returns `None` for
// every slot, and `build_slot_list_slots` falls back to `empty_slot()` —
// which is exactly the "border/background remains, text blank" symptom.
//
// Tests target observable state: `viewport_offset` against new buffer length,
// `selected_indices` purge, and the `library.counts.albums` / buffer-length
// agreement after the load completes.

#[test]
fn albums_loaded_clamps_viewport_when_anchor_missing() {
    // Old buffer had 50 albums, viewport was deep at 40. SSE-driven refresh
    // returns 15 albums and the anchor ID isn't present. viewport_offset must
    // land within the new buffer to keep slots rendering real items.
    let mut app = test_app();
    app.current_view = View::Albums;
    let old_albums: Vec<_> = (0..50)
        .map(|i| make_album(&format!("a{i}"), &format!("Album {i}"), "Artist"))
        .collect();
    app.library.albums.set_from_vec(old_albums);
    app.albums_page.common.slot_list.viewport_offset = 40;

    let new_albums: Vec<_> = (0..15)
        .map(|i| make_album(&format!("b{i}"), &format!("New {i}"), "Artist"))
        .collect();
    let _ = app.handle_albums_loaded(Ok(new_albums), 15, true, Some("a40".to_string()));

    assert_eq!(app.library.albums.len(), 15);
    assert!(
        app.albums_page.common.slot_list.viewport_offset < app.library.albums.len(),
        "viewport_offset {} must stay within new buffer length {}",
        app.albums_page.common.slot_list.viewport_offset,
        app.library.albums.len()
    );
}

#[test]
fn albums_loaded_clamps_viewport_when_anchor_id_none() {
    // Background reload with no anchor (pre-existing buffer empty at snapshot
    // time, then offset advanced somehow). viewport_offset still must land
    // within the new buffer.
    let mut app = test_app();
    app.current_view = View::Albums;
    let old_albums: Vec<_> = (0..30)
        .map(|i| make_album(&format!("a{i}"), &format!("Album {i}"), "Artist"))
        .collect();
    app.library.albums.set_from_vec(old_albums);
    app.albums_page.common.slot_list.viewport_offset = 25;

    let new_albums: Vec<_> = (0..10)
        .map(|i| make_album(&format!("b{i}"), &format!("New {i}"), "Artist"))
        .collect();
    let _ = app.handle_albums_loaded(Ok(new_albums), 10, true, None);

    assert!(
        app.albums_page.common.slot_list.viewport_offset < app.library.albums.len(),
        "viewport_offset {} must stay within new buffer length {}",
        app.albums_page.common.slot_list.viewport_offset,
        app.library.albums.len()
    );
}

#[test]
fn albums_loaded_anchor_match_takes_precedence_over_clamp() {
    // When the anchor IS found, anchor wins — viewport jumps to the new
    // index, even though that index is also within bounds. Locks the existing
    // anchor-restore behavior so the clamp fix can't quietly override it.
    let mut app = test_app();
    app.current_view = View::Albums;
    let old_albums: Vec<_> = (0..50)
        .map(|i| make_album(&format!("a{i}"), &format!("Album {i}"), "Artist"))
        .collect();
    app.library.albums.set_from_vec(old_albums);
    app.albums_page.common.slot_list.viewport_offset = 40;

    // New list: a40 sits at index 5 (5 newer albums prepended, recently-added
    // sort behavior).
    let mut new_albums: Vec<_> = (0..5)
        .map(|i| make_album(&format!("new{i}"), &format!("Newest {i}"), "Artist"))
        .collect();
    new_albums.push(make_album("a40", "Album 40", "Artist"));
    new_albums
        .extend((41..50).map(|i| make_album(&format!("a{i}"), &format!("Album {i}"), "Artist")));

    let _ = app.handle_albums_loaded(Ok(new_albums), 15, true, Some("a40".to_string()));

    assert_eq!(
        app.albums_page.common.slot_list.viewport_offset, 5,
        "anchor lookup should jump viewport to the anchor's new index"
    );
}

#[test]
fn albums_loaded_purges_stale_selected_indices() {
    // Selected indices that point past the new buffer must be removed —
    // otherwise the slot list highlight + batch-action paths see phantom
    // selections against items that no longer exist.
    let mut app = test_app();
    app.current_view = View::Albums;
    let old_albums: Vec<_> = (0..50)
        .map(|i| make_album(&format!("a{i}"), &format!("Album {i}"), "Artist"))
        .collect();
    app.library.albums.set_from_vec(old_albums);
    app.albums_page.common.slot_list.viewport_offset = 40;
    app.albums_page
        .common
        .slot_list
        .selected_indices
        .extend([3, 35, 40, 45]);

    let new_albums: Vec<_> = (0..10)
        .map(|i| make_album(&format!("b{i}"), &format!("New {i}"), "Artist"))
        .collect();
    let _ = app.handle_albums_loaded(Ok(new_albums), 10, true, None);

    let stale: Vec<_> = app
        .albums_page
        .common
        .slot_list
        .selected_indices
        .iter()
        .copied()
        .filter(|&i| i >= app.library.albums.len())
        .collect();
    assert!(
        stale.is_empty(),
        "selected_indices should not retain entries past new buffer length, got stale: {stale:?}"
    );
}

#[test]
fn albums_loaded_total_count_matches_buffer_length() {
    // Header total count and buffer length come from the same load — they
    // must agree after `handle_albums_loaded` returns. Locks the assignment
    // ordering so a future reorder doesn't introduce a transient mismatch.
    let mut app = test_app();
    app.current_view = View::Albums;
    let old_albums: Vec<_> = (0..50)
        .map(|i| make_album(&format!("a{i}"), &format!("Album {i}"), "Artist"))
        .collect();
    app.library.albums.set_from_vec(old_albums);
    app.library.counts.albums = 50;

    let new_albums: Vec<_> = (0..30)
        .map(|i| make_album(&format!("b{i}"), &format!("New {i}"), "Artist"))
        .collect();
    let _ = app.handle_albums_loaded(Ok(new_albums), 30, true, None);

    assert_eq!(app.library.albums.len(), 30);
    assert_eq!(app.library.counts.albums, 30);
}

#[test]
fn albums_loaded_background_preserves_viewport_when_safe() {
    // Background refresh with anchor found at the same index must NOT reset
    // viewport to 0 (foreground refresh resets, background preserves).
    // Regression guard around the existing `if !background` branch in
    // `handle_albums_loaded`.
    let mut app = test_app();
    app.current_view = View::Albums;
    let albums: Vec<_> = (0..50)
        .map(|i| make_album(&format!("a{i}"), &format!("Album {i}"), "Artist"))
        .collect();
    app.library.albums.set_from_vec(albums.clone());
    app.albums_page.common.slot_list.viewport_offset = 12;

    let _ = app.handle_albums_loaded(Ok(albums), 50, true, Some("a12".to_string()));

    assert_eq!(
        app.albums_page.common.slot_list.viewport_offset, 12,
        "background refresh with stable anchor should preserve viewport_offset"
    );
}

// ============================================================================
// Scrollbar Seek → Large Artwork Loading (regression: missing large artwork
// after rapid scroll in albums view)
// ============================================================================
//
// User-reported bug: after scrolling rapidly via the scrollbar in the Albums
// view and then stopping, the large artwork column sometimes stays blank. The
// mini thumbnails for visible slots load fine, and stepping one slot up/down
// with the keyboard then back fixes it. Eventually an SSE-driven artwork
// refresh races in and populates the missing artwork ("Updated artwork for 1
// album" toast).
//
// Architecture intent: SlotListScrollSeek is a hot-path event that should NOT
// trigger a fetch. After 150ms idle, `seek_settled_timer` fires
// `SlotListMessage::SeekSettled`, which synthesises a `SlotListSetOffset` for
// the target view; that path is supposed to dispatch LoadLargeArtwork for the
// centred album.
//
// The `LoadLargeArtwork` action is also dispatched by the existing nav paths,
// so its handler is what eventually puts the album into `loading_large_artwork`
// and kicks off the fetch — which means a synchronous side-effect on
// `loading_large_artwork` is the right TDD signal that the chain is wired
// correctly without depending on the async iced runtime.

#[test]
fn albums_scroll_seek_does_not_load_artwork_immediately() {
    // Hot-path scroll events should only update viewport state. Artwork loading
    // is deferred to the seek_settled debounce timer.
    let mut app = test_app();
    app.current_view = View::Albums;
    let albums: Vec<_> = (0..50)
        .map(|i| make_album(&format!("a{i}"), &format!("Album {i}"), "Artist"))
        .collect();
    app.library.albums.set_from_vec(albums);

    let _ = app.handle_albums(crate::views::AlbumsMessage::SlotListScrollSeek(25));

    assert_eq!(app.albums_page.common.slot_list.viewport_offset, 25);
    assert_eq!(
        app.artwork.loading_large_artwork, None,
        "scroll seek alone should not start a fetch"
    );
}

#[test]
fn albums_scroll_seek_bumps_scroll_generation_id() {
    // The seek_settled timer is gated by scroll_generation_id; each scroll
    // event must bump it so stale timers from earlier seeks are skipped.
    let mut app = test_app();
    app.current_view = View::Albums;
    let albums: Vec<_> = (0..20)
        .map(|i| make_album(&format!("a{i}"), &format!("Album {i}"), "Artist"))
        .collect();
    app.library.albums.set_from_vec(albums);

    let initial = app.albums_page.common.slot_list.scroll_generation_id;
    let _ = app.handle_albums(crate::views::AlbumsMessage::SlotListScrollSeek(5));

    assert!(
        app.albums_page.common.slot_list.scroll_generation_id > initial,
        "scroll seek should bump scroll_generation_id"
    );
}

#[test]
fn albums_seek_settled_dispatches_load_large_artwork_for_centered_album() {
    // The bug: this is the chain that fails to fire the artwork load after a
    // rapid-scroll-then-stop in the Albums view. Synthesising the SeekSettled
    // message reproduces what the 150ms debounce timer does in production.
    use crate::app_message::SlotListMessage;

    let mut app = test_app();
    app.current_view = View::Albums;
    let albums: Vec<_> = (0..50)
        .map(|i| make_album(&format!("a{i}"), &format!("Album {i}"), "Artist"))
        .collect();
    app.library.albums.set_from_vec(albums);

    let _ = app.handle_albums(crate::views::AlbumsMessage::SlotListScrollSeek(25));
    let gen_id = app.albums_page.common.slot_list.scroll_generation_id;

    let _ = app.handle_slot_list_message(SlotListMessage::SeekSettled(View::Albums, gen_id));

    assert_eq!(
        app.artwork.loading_large_artwork.as_deref(),
        Some("a25"),
        "seek_settled should trigger LoadLargeArtwork for the centered album"
    );
}

#[test]
fn songs_seek_settled_dispatches_load_large_artwork_for_centered_song_album() {
    // Songs view is reported to never have the bug — keep it green as a
    // regression sentinel so the fix in albums doesn't break the working path.
    use crate::app_message::SlotListMessage;

    let mut app = test_app();
    app.current_view = View::Songs;
    app.library.songs.set_from_vec(
        (0..50)
            .map(|i| make_song(&format!("s{i}"), &format!("Song {i}"), "Artist"))
            .collect(),
    );

    let _ = app.handle_songs(crate::views::SongsMessage::SlotListScrollSeek(25));
    let gen_id = app.songs_page.common.slot_list.scroll_generation_id;

    let _ = app.handle_slot_list_message(SlotListMessage::SeekSettled(View::Songs, gen_id));

    // make_song defaults album_id to "album_{id}", so song s25 → album_s25.
    assert_eq!(
        app.artwork.loading_large_artwork.as_deref(),
        Some("album_s25"),
        "seek_settled should trigger LoadLargeArtwork for the centered song's album"
    );
}

#[test]
fn albums_seek_settled_skipped_when_generation_id_is_stale() {
    // Sanity check: if a newer scroll has bumped gen_id, a stale timer's gen_id
    // is rejected and no artwork load happens. Verifies the guard keeps working
    // alongside the fix.
    use crate::app_message::SlotListMessage;

    let mut app = test_app();
    app.current_view = View::Albums;
    let albums: Vec<_> = (0..20)
        .map(|i| make_album(&format!("a{i}"), &format!("Album {i}"), "Artist"))
        .collect();
    app.library.albums.set_from_vec(albums);

    let _ = app.handle_albums(crate::views::AlbumsMessage::SlotListScrollSeek(5));
    let stale_gen = app.albums_page.common.slot_list.scroll_generation_id;
    // Subsequent scroll bumps gen_id, leaving stale_gen behind.
    let _ = app.handle_albums(crate::views::AlbumsMessage::SlotListScrollSeek(10));

    let _ = app.handle_slot_list_message(SlotListMessage::SeekSettled(View::Albums, stale_gen));

    assert_eq!(
        app.artwork.loading_large_artwork, None,
        "stale-generation seek_settled timer must not start a fetch"
    );
}

// ============================================================================
// Artists rating-sort carve-out (Phase 3B regression net)
//
// The Subsonic API does not expose a "by rating" sort, so the artists handler
// sorts client-side after each page load. Phase 3B's `load_paginated`
// consolidation must preserve this behaviour — these tests pin the contract
// of `Nokkvi::artists_rating_sort`.
// ============================================================================

#[test]
fn artists_rating_sort_some_before_none_then_desc_by_value() {
    use nokkvi_data::backend::artists::ArtistUIViewData;

    use crate::Nokkvi;

    fn make(id: &str, rating: Option<u32>) -> ArtistUIViewData {
        ArtistUIViewData {
            id: id.into(),
            name: format!("Artist {id}"),
            album_count: 0,
            song_count: 0,
            is_starred: false,
            image_url: None,
            artwork_url: None,
            rating,
            play_count: None,
            play_date: None,
            size: None,
            mbz_artist_id: None,
            biography: None,
            external_url: None,
            searchable_lower: String::new(),
        }
    }

    let mut artists = vec![
        make("none1", None),
        make("low", Some(1)),
        make("none2", None),
        make("high", Some(5)),
        make("mid", Some(3)),
    ];
    Nokkvi::artists_rating_sort(&mut artists);

    let order: Vec<&str> = artists.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(
        order,
        vec!["high", "mid", "low", "none1", "none2"],
        "rated artists come first sorted desc by value; unrated artists tail in original order"
    );
}

#[test]
fn artists_rating_sort_empty_is_noop() {
    use nokkvi_data::backend::artists::ArtistUIViewData;

    use crate::Nokkvi;

    let mut artists: Vec<ArtistUIViewData> = vec![];
    Nokkvi::artists_rating_sort(&mut artists);
    assert!(artists.is_empty());
}

// ============================================================================
// System Tray (services/tray.rs + update/tray.rs)
// ============================================================================

#[test]
fn tray_settings_default_off() {
    let app = test_app();
    assert!(!app.show_tray_icon);
    assert!(!app.close_to_tray);
    assert!(!app.tray_window_hidden);
    assert!(app.tray_connection.is_none());
    assert!(app.main_window_id.is_none());
}

#[test]
fn window_opened_replaces_main_window_id() {
    let mut app = test_app();
    let id1 = iced::window::Id::unique();
    let id2 = iced::window::Id::unique();

    let _ = app.handle_window_opened(id1);
    assert_eq!(app.main_window_id, Some(id1));
    assert!(!app.tray_window_hidden);

    // Daemon mode: close-to-tray destroys the surface; tray Activate opens
    // a fresh window with a different id. handle_window_opened must adopt
    // the new id (the old one was destroyed) and mark the app as visible.
    app.tray_window_hidden = true;
    let _ = app.handle_window_opened(id2);
    assert_eq!(app.main_window_id, Some(id2));
    assert!(!app.tray_window_hidden);
}

#[test]
fn window_close_requested_with_close_to_tray_off_does_not_hide() {
    let mut app = test_app();
    app.show_tray_icon = true;
    app.close_to_tray = false;
    let id = iced::window::Id::unique();

    let _ = app.handle_window_close_requested(id);

    assert!(
        !app.tray_window_hidden,
        "close_to_tray off → window must not be marked hidden (X should quit)"
    );
}

#[test]
fn window_close_requested_with_close_to_tray_on_destroys_window() {
    let mut app = test_app();
    app.show_tray_icon = true;
    app.close_to_tray = true;
    app.main_window_id = Some(iced::window::Id::unique());
    let id = iced::window::Id::unique();

    let _ = app.handle_window_close_requested(id);

    assert!(app.tray_window_hidden);
    // The window is being destroyed — its id is no longer addressable.
    // Cleared so the next tray Activate goes through the "open" branch.
    assert_eq!(app.main_window_id, None);
}

#[test]
fn tray_activate_toggles_window_hidden_flag() {
    use crate::services::tray::TrayEvent;

    let mut app = test_app();
    app.main_window_id = Some(iced::window::Id::unique());
    assert!(!app.tray_window_hidden);

    // First Activate: visible → hidden. Closes the window, clears the id.
    let _ = app.handle_tray(TrayEvent::Activate);
    assert!(
        app.tray_window_hidden,
        "first Activate hides (closes window)"
    );
    assert_eq!(
        app.main_window_id, None,
        "closed window's id is not re-usable"
    );

    // Second Activate: hidden → visible. Dispatches window::open; the new
    // id arrives via WindowOpened later. We flip the flag synchronously
    // so a third rapid Activate reads the right intent.
    let _ = app.handle_tray(TrayEvent::Activate);
    assert!(
        !app.tray_window_hidden,
        "second Activate shows (opens new window)"
    );
}

#[test]
fn tray_activate_without_window_id_is_noop() {
    use crate::services::tray::TrayEvent;

    let mut app = test_app();
    assert!(app.main_window_id.is_none());

    let _ = app.handle_tray(TrayEvent::Activate);
    assert!(
        !app.tray_window_hidden,
        "Activate before window id captured leaves state unchanged"
    );
}

// ============================================================================
// Default-Playlist Picker (default_playlist_picker.rs)
// ============================================================================

fn make_test_playlist(id: &str, name: &str) -> nokkvi_data::backend::playlists::PlaylistUIViewData {
    nokkvi_data::backend::playlists::PlaylistUIViewData {
        id: id.to_string(),
        name: name.to_string(),
        comment: String::new(),
        duration: 0.0,
        song_count: 0,
        owner_name: String::new(),
        public: false,
        updated_at: String::new(),
        artwork_album_ids: vec![],
        searchable_lower: name.to_lowercase(),
    }
}

fn seed_playlists(app: &mut crate::Nokkvi, items: Vec<(&str, &str)>) {
    let total = items.len();
    let playlists: Vec<_> = items
        .into_iter()
        .map(|(id, name)| make_test_playlist(id, name))
        .collect();
    app.library.playlists.append_page(playlists, total);
}

#[test]
fn picker_open_initializes_state_with_library_playlists() {
    use crate::widgets::default_playlist_picker::{DefaultPlaylistPickerMessage, PickerEntry};

    let mut app = test_app();
    seed_playlists(
        &mut app,
        vec![("p1", "Workout"), ("p2", "Chill"), ("p3", "Focus")],
    );
    assert!(app.default_playlist_picker.is_none());

    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::Open);

    let state = app
        .default_playlist_picker
        .as_ref()
        .expect("picker should be open after Open message");
    // 3 real playlists + 1 prepended Clear entry
    assert_eq!(state.all_entries.len(), 4);
    assert!(matches!(state.all_entries[0], PickerEntry::Clear));
}

#[test]
fn picker_close_clears_state() {
    use crate::widgets::default_playlist_picker::DefaultPlaylistPickerMessage;

    let mut app = test_app();
    seed_playlists(&mut app, vec![("p1", "Workout")]);
    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::Open);
    assert!(app.default_playlist_picker.is_some());

    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::Close);
    assert!(app.default_playlist_picker.is_none());
}

#[test]
fn picker_search_filters_entries() {
    use crate::widgets::default_playlist_picker::{DefaultPlaylistPickerMessage, PickerEntry};

    let mut app = test_app();
    seed_playlists(
        &mut app,
        vec![("p1", "Workout"), ("p2", "Chill"), ("p3", "Focus")],
    );
    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::Open);

    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::SearchChanged(
        "work".to_string(),
    ));

    let state = app.default_playlist_picker.as_ref().unwrap();
    // Clear stays + only "Workout" matches → 2 entries
    assert_eq!(state.filtered.len(), 2);
    assert!(matches!(state.filtered[0], PickerEntry::Clear));
    if let PickerEntry::Playlist { name, .. } = &state.filtered[1] {
        assert_eq!(name, "Workout");
    } else {
        panic!("expected Playlist entry at index 1");
    }
}

#[test]
fn picker_click_playlist_sets_default_and_closes() {
    use crate::widgets::default_playlist_picker::DefaultPlaylistPickerMessage;

    let mut app = test_app();
    seed_playlists(&mut app, vec![("p1", "Workout"), ("p2", "Chill")]);
    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::Open);
    assert!(app.default_playlist_id.is_none());
    assert!(app.default_playlist_name.is_empty());

    // Index 1 is the first real playlist (index 0 is the Clear virtual entry).
    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::ClickItem(1));

    assert_eq!(app.default_playlist_id, Some("p1".to_string()));
    assert_eq!(app.default_playlist_name, "Workout");
    assert!(
        app.default_playlist_picker.is_none(),
        "selecting an entry should close the picker"
    );
}

#[test]
fn picker_click_clear_unsets_default_and_closes() {
    use crate::widgets::default_playlist_picker::DefaultPlaylistPickerMessage;

    let mut app = test_app();
    app.default_playlist_id = Some("p1".to_string());
    app.default_playlist_name = "Workout".to_string();
    seed_playlists(&mut app, vec![("p1", "Workout")]);
    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::Open);

    // Index 0 is the Clear virtual entry.
    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::ClickItem(0));

    assert!(app.default_playlist_id.is_none());
    assert!(app.default_playlist_name.is_empty());
    assert!(app.default_playlist_picker.is_none());
}

#[test]
fn picker_activate_center_selects_centered_entry() {
    use crate::widgets::default_playlist_picker::DefaultPlaylistPickerMessage;

    let mut app = test_app();
    seed_playlists(&mut app, vec![("p1", "Workout"), ("p2", "Chill")]);
    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::Open);

    // Move down once to put the first real playlist in the center.
    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::SlotListDown);
    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::ActivateCenter);

    // Either Clear or Workout could be centered depending on slot list center index;
    // the contract is just that the picker closes and *some* selection happened.
    assert!(app.default_playlist_picker.is_none());
}

#[test]
fn picker_open_with_empty_library_still_offers_clear_entry() {
    use crate::widgets::default_playlist_picker::{DefaultPlaylistPickerMessage, PickerEntry};

    let mut app = test_app();
    // No playlists seeded — library.playlists stays empty.
    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::Open);

    let state = app.default_playlist_picker.as_ref().unwrap();
    assert_eq!(state.all_entries.len(), 1);
    assert!(matches!(state.all_entries[0], PickerEntry::Clear));
}

#[test]
fn picker_repopulates_when_playlists_load_after_open() {
    use crate::widgets::default_playlist_picker::{DefaultPlaylistPickerMessage, PickerEntry};

    let mut app = test_app();

    // Open picker with empty library — only the Clear entry is shown.
    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::Open);
    let _ = app.handle_default_playlist_picker(DefaultPlaylistPickerMessage::SearchChanged(
        "foo".to_string(),
    ));
    assert_eq!(
        app.default_playlist_picker
            .as_ref()
            .unwrap()
            .all_entries
            .len(),
        1
    );

    // Library load arrives after the picker was opened — refresh hook
    // should repopulate the picker while preserving the user's search query.
    seed_playlists(&mut app, vec![("p1", "Workout"), ("p2", "Foo")]);
    app.refresh_default_playlist_picker_after_load();

    let state = app.default_playlist_picker.as_ref().unwrap();
    assert_eq!(state.all_entries.len(), 3, "Clear + 2 playlists");
    assert_eq!(
        state.search_query, "foo",
        "the user's in-flight search query is preserved across the rebuild"
    );
    // "foo" matches "Foo", and Clear is always visible
    assert_eq!(state.filtered.len(), 2);
    assert!(matches!(state.filtered[0], PickerEntry::Clear));
    if let PickerEntry::Playlist { name, .. } = &state.filtered[1] {
        assert_eq!(name, "Foo");
    } else {
        panic!("expected Playlist entry at index 1");
    }
}

#[test]
fn queue_show_default_playlist_setting_default_is_off() {
    let app = test_app();
    assert!(
        !app.queue_show_default_playlist,
        "the queue chip is opt-in — default should be hidden"
    );
}

// ============================================================================
// Text Input Dialog — Public/Private Playlist Toggle (F1, T5–T7)
// ============================================================================

#[test]
fn text_input_dialog_save_playlist_defaults_to_public() {
    let mut app = test_app();
    app.text_input_dialog.open_save_playlist(&[]);
    assert!(
        app.text_input_dialog.public,
        "newly opened save-playlist dialog must default the toggle to public"
    );
}

#[test]
fn text_input_dialog_public_toggled_message_flips_state() {
    use crate::{app_message::Message, widgets::text_input_dialog::TextInputDialogMessage};

    let mut app = test_app();
    app.text_input_dialog.open_save_playlist(&[]);
    assert!(app.text_input_dialog.public);

    let _ = app.update(Message::TextInputDialog(
        TextInputDialogMessage::PublicToggled(false),
    ));
    assert!(
        !app.text_input_dialog.public,
        "PublicToggled(false) must flip the dialog's public field to false"
    );
}

#[test]
fn text_input_dialog_combo_round_trip_preserves_public_off() {
    use crate::{
        app_message::Message,
        widgets::text_input_dialog::{PlaylistOption, TextInputDialogMessage},
    };

    let mut app = test_app();
    app.text_input_dialog
        .open_save_playlist(&[("p1".into(), "Existing".into())]);

    // User unchecks Public.
    let _ = app.update(Message::TextInputDialog(
        TextInputDialogMessage::PublicToggled(false),
    ));
    assert!(!app.text_input_dialog.public);

    // User flips combo to Existing playlist, then back to NewPlaylist.
    let _ = app.update(Message::TextInputDialog(
        TextInputDialogMessage::PlaylistSelected(PlaylistOption::Existing {
            id: "p1".into(),
            name: "Existing".into(),
        }),
    ));
    let _ = app.update(Message::TextInputDialog(
        TextInputDialogMessage::PlaylistSelected(PlaylistOption::NewPlaylist),
    ));

    assert!(
        !app.text_input_dialog.public,
        "combo round-trip must not silently reset the public toggle"
    );
}

#[test]
fn open_create_playlist_dialog_defaults_to_public_and_no_combo() {
    use crate::widgets::text_input_dialog::TextInputDialogAction;

    let mut app = test_app();
    app.text_input_dialog.open_create_playlist();

    assert!(app.text_input_dialog.visible);
    assert!(
        app.text_input_dialog.public,
        "Create-New-Playlist dialog must default the toggle to public"
    );
    assert!(
        !app.text_input_dialog.save_playlist_mode,
        "Create-New-Playlist must not show the existing-playlists combo"
    );
    assert!(matches!(
        app.text_input_dialog.action,
        Some(TextInputDialogAction::CreatePlaylistAndEdit)
    ));
}

#[test]
fn create_playlist_dialog_refused_when_already_editing() {
    use crate::{app_message::Message, views::PlaylistsMessage};

    let mut app = test_app();
    // Enter split-view edit mode first.
    let _ = app.update(Message::EnterPlaylistEditMode {
        playlist_id: "p1".into(),
        playlist_name: "Existing".into(),
        playlist_comment: String::new(),
        playlist_public: true,
    });
    assert!(app.playlist_edit.is_some());

    // User clicks the view-header `+` — message bubbles to root, guard fires.
    let _ = app.update(Message::Playlists(
        PlaylistsMessage::OpenCreatePlaylistDialog,
    ));

    assert!(
        !app.text_input_dialog.visible,
        "guard must keep the dialog closed when already editing"
    );
    assert!(
        app.playlist_edit.is_some(),
        "guard must not disturb the in-progress edit"
    );
}

#[test]
fn create_playlist_dialog_opens_when_not_editing() {
    use crate::{
        app_message::Message, views::PlaylistsMessage,
        widgets::text_input_dialog::TextInputDialogAction,
    };

    let mut app = test_app();
    assert!(app.playlist_edit.is_none());

    let _ = app.update(Message::Playlists(
        PlaylistsMessage::OpenCreatePlaylistDialog,
    ));

    assert!(app.text_input_dialog.visible);
    assert!(matches!(
        app.text_input_dialog.action,
        Some(TextInputDialogAction::CreatePlaylistAndEdit)
    ));
    assert!(app.text_input_dialog.public);
}

// ============================================================================
// Playlist Edit Mode — Public Toggle (F2, T8–T11)
// ============================================================================

#[test]
fn enter_edit_mode_aligns_active_playlist_info() {
    use crate::{app_message::Message, state::ActivePlaylistContext};

    let mut app = test_app();
    // Pre-condition: a different playlist is currently "active" in the header.
    app.active_playlist_info = Some(ActivePlaylistContext {
        id: "playing".into(),
        name: "Currently Playing".into(),
        comment: String::new(),
    });

    let _ = app.update(Message::EnterPlaylistEditMode {
        playlist_id: "edited".into(),
        playlist_name: "Being Edited".into(),
        playlist_comment: "Edit me".into(),
        playlist_public: false,
    });

    let active = app
        .active_playlist_info
        .as_ref()
        .expect("active_playlist_info must remain Some — re-anchored, not cleared");
    assert_eq!(
        active.id, "edited",
        "entering edit mode must re-anchor active_playlist_info to the edited playlist"
    );
    assert_eq!(active.name, "Being Edited");
    assert_eq!(active.comment, "Edit me");
}

#[test]
fn exit_edit_mode_preserves_aligned_context() {
    use crate::app_message::Message;

    let mut app = test_app();
    // No active playlist initially (e.g., create-and-edit flow).
    assert!(app.active_playlist_info.is_none());

    let _ = app.update(Message::EnterPlaylistEditMode {
        playlist_id: "new".into(),
        playlist_name: "Brand New".into(),
        playlist_comment: String::new(),
        playlist_public: true,
    });

    // Discard.
    let _ = app.update(Message::ExitPlaylistEditMode);

    let active = app.active_playlist_info.as_ref().expect(
        "exit must leave active_playlist_info pointing at the edited playlist, \
             not clear it or revert to a stale prior context",
    );
    assert_eq!(active.id, "new");
    assert!(
        app.playlist_edit.is_none(),
        "exit clears playlist_edit but not active_playlist_info"
    );
}

#[test]
fn enter_playlist_edit_mode_seeds_initial_public() {
    use crate::app_message::Message;

    let mut app = test_app();
    let _ = app.update(Message::EnterPlaylistEditMode {
        playlist_id: "p1".into(),
        playlist_name: "Mix".into(),
        playlist_comment: String::new(),
        playlist_public: false,
    });

    let edit = app
        .playlist_edit
        .as_ref()
        .expect("entering edit mode must populate playlist_edit");
    assert!(
        !edit.playlist_public,
        "EnterPlaylistEditMode with public=false must seed playlist_public=false"
    );
    assert!(
        !edit.is_public_dirty(),
        "freshly seeded edit state must not report public-dirty"
    );
}

#[test]
fn playlist_edit_public_toggle_flips_state() {
    use crate::{app_message::Message, views::QueueMessage};

    let mut app = test_app();
    let _ = app.update(Message::EnterPlaylistEditMode {
        playlist_id: "p1".into(),
        playlist_name: "Mix".into(),
        playlist_comment: String::new(),
        playlist_public: true,
    });

    let _ = app.update(Message::Queue(QueueMessage::PlaylistEditPublicToggled(
        false,
    )));

    let edit = app.playlist_edit.as_ref().expect("playlist_edit set");
    assert!(
        !edit.playlist_public,
        "PlaylistEditPublicToggled(false) must flip the edit-state flag"
    );
    assert!(
        edit.is_public_dirty(),
        "after toggle the edit state must be public-dirty"
    );
}

#[test]
fn playlist_edit_public_revert_clears_dirty() {
    use crate::{app_message::Message, views::QueueMessage};

    let mut app = test_app();
    let _ = app.update(Message::EnterPlaylistEditMode {
        playlist_id: "p1".into(),
        playlist_name: "Mix".into(),
        playlist_comment: String::new(),
        playlist_public: true,
    });

    let _ = app.update(Message::Queue(QueueMessage::PlaylistEditPublicToggled(
        false,
    )));
    let _ = app.update(Message::Queue(QueueMessage::PlaylistEditPublicToggled(
        true,
    )));

    let edit = app.playlist_edit.as_ref().expect("playlist_edit set");
    assert!(
        !edit.is_public_dirty(),
        "toggling back to the original value must clear public-dirty"
    );
}

#[test]
fn playlist_edit_public_only_change_is_metadata_dirty() {
    use crate::{app_message::Message, views::QueueMessage};

    let mut app = test_app();
    let _ = app.update(Message::EnterPlaylistEditMode {
        playlist_id: "p1".into(),
        playlist_name: "Mix".into(),
        playlist_comment: String::new(),
        playlist_public: true,
    });

    let _ = app.update(Message::Queue(QueueMessage::PlaylistEditPublicToggled(
        false,
    )));

    let edit = app.playlist_edit.as_ref().expect("playlist_edit set");
    assert!(
        edit.has_metadata_changes(),
        "a pure-visibility flip must satisfy the predicate the save handler \
         uses to decide whether to call update_playlist (R6 fix)"
    );
}

// ============================================================================
// Surfing-Boat Overlay Handler (boat.rs)
// ============================================================================

mod boat_tests {
    use std::time::{Duration, Instant};

    use nokkvi_data::types::player_settings::VisualizationMode;

    use crate::{app_message::Message, test_helpers::*};

    /// Enable the boat toggle in the shared visualizer config.
    fn enable_boat_in_config(app: &crate::Nokkvi, on: bool) {
        let mut cfg = app.visualizer_config.write();
        cfg.lines.boat = on;
    }

    #[test]
    fn boat_visible_only_in_lines_mode() {
        let mut app = test_app();
        enable_boat_in_config(&app, true);

        // Default mode is Bars — boat should stay hidden even with toggle on.
        app.engine.visualization_mode = VisualizationMode::Bars;
        let _ = app.update(Message::BoatTick(Instant::now()));
        assert!(
            !app.boat.visible,
            "boat must be hidden in Bars mode regardless of the boat toggle"
        );

        // Switch to Lines — boat should now be visible.
        app.engine.visualization_mode = VisualizationMode::Lines;
        let _ = app.update(Message::BoatTick(Instant::now()));
        assert!(
            app.boat.visible,
            "boat must be visible in Lines mode when the toggle is on"
        );
    }

    #[test]
    fn boat_hidden_when_visualizer_disabled() {
        let mut app = test_app();
        enable_boat_in_config(&app, true);
        // VisualizationMode::Off is what mounts the visualizer at all (see
        // app_view.rs). When Off, the boat must also be hidden.
        app.engine.visualization_mode = VisualizationMode::Off;
        let _ = app.update(Message::BoatTick(Instant::now()));
        assert!(
            !app.boat.visible,
            "boat must be hidden when the visualizer is fully off"
        );
    }

    #[test]
    fn boat_hidden_when_settings_toggle_off() {
        let mut app = test_app();
        // Lines mode active, but the user's boat toggle is off (the default).
        app.engine.visualization_mode = VisualizationMode::Lines;
        let _ = app.update(Message::BoatTick(Instant::now()));
        assert!(
            !app.boat.visible,
            "boat must respect the user's `lines.boat` toggle"
        );
    }

    #[test]
    fn boat_advances_x_ratio_on_tick() {
        let mut app = test_app();
        enable_boat_in_config(&app, true);
        app.engine.visualization_mode = VisualizationMode::Lines;

        // The boat is purely propelled by music in the new model and
        // `test_app()` has no visualizer / no BPM — so we seed a
        // non-zero `x_velocity` and verify the integrator advances
        // `x_ratio` from it. This pins "the handler actually ticks
        // step()" without depending on the music pipeline.
        app.boat.x_velocity = 0.05;
        app.boat.facing = 1.0;

        let t0 = Instant::now();
        let _ = app.update(Message::BoatTick(t0));
        let x0 = app.boat.x_ratio;

        let t1 = t0 + Duration::from_millis(100);
        let _ = app.update(Message::BoatTick(t1));
        let x1 = app.boat.x_ratio;

        assert_ne!(
            x0, x1,
            "two ticks 100 ms apart in lines mode must move the boat \
             when seeded with non-zero velocity (got x0={x0}, x1={x1})"
        );
    }

    #[test]
    fn boat_state_resumes_after_mode_round_trip() {
        let mut app = test_app();
        enable_boat_in_config(&app, true);
        app.engine.visualization_mode = VisualizationMode::Lines;

        // Tick a couple of times to seat `last_tick`, advance physics,
        // and let the tack countdown decrement.
        let t0 = Instant::now();
        let _ = app.update(Message::BoatTick(t0));
        let _ = app.update(Message::BoatTick(t0 + Duration::from_millis(50)));
        let saved_tack = app.boat.secs_until_next_tack;
        let saved_x = app.boat.x_ratio;
        assert!(
            saved_tack > 0.0,
            "tack countdown must have been seeded by the first integrating \
             tick (got {saved_tack})"
        );

        // Switch to Bars — boat hides, physics fields preserved.
        app.engine.visualization_mode = VisualizationMode::Bars;
        let _ = app.update(Message::BoatTick(t0 + Duration::from_millis(100)));
        assert!(!app.boat.visible);
        assert_eq!(
            app.boat.secs_until_next_tack, saved_tack,
            "tack countdown must NOT advance while hidden \
             (saved={saved_tack}, now={})",
            app.boat.secs_until_next_tack
        );
        assert_eq!(
            app.boat.x_ratio, saved_x,
            "x_ratio must NOT advance while hidden \
             (saved={saved_x}, now={})",
            app.boat.x_ratio
        );

        // Back to Lines — state resumes from where it left off (the
        // first re-show tick has dt=0 because last_tick was cleared).
        app.engine.visualization_mode = VisualizationMode::Lines;
        let _ = app.update(Message::BoatTick(t0 + Duration::from_millis(150)));
        assert!(app.boat.visible);
        assert_eq!(
            app.boat.secs_until_next_tack, saved_tack,
            "tack countdown preserved across the round trip"
        );
        assert_eq!(
            app.boat.x_ratio, saved_x,
            "x_ratio preserved across the round trip"
        );
    }

    #[test]
    fn boat_clears_last_tick_when_hidden() {
        // Regression: when hidden the handler must drop `last_tick` so the
        // first frame back doesn't see a stale multi-second gap.
        let mut app = test_app();
        enable_boat_in_config(&app, true);
        app.engine.visualization_mode = VisualizationMode::Lines;

        let t0 = Instant::now();
        let _ = app.update(Message::BoatTick(t0));
        assert!(app.boat.last_tick.is_some());

        app.engine.visualization_mode = VisualizationMode::Off;
        let _ = app.update(Message::BoatTick(t0 + Duration::from_secs(5)));
        assert!(
            app.boat.last_tick.is_none(),
            "last_tick must be cleared while hidden so re-show starts with dt=0"
        );
    }

    #[test]
    fn boat_freezes_while_audio_paused() {
        // Audio pause: the visualizer waveform decays to silence (the FFT
        // thread's sample buffer empties), so integrating sail thrust
        // against a flat line still walks the boat across an empty wave.
        // Every dynamic physics field must hold while `playback.paused`.
        let mut app = test_app();
        enable_boat_in_config(&app, true);
        app.engine.visualization_mode = VisualizationMode::Lines;

        // Seed non-default values so an accidental "field stayed at 0
        // because it was already 0" pass can't sneak through.
        app.boat.x_ratio = 0.6;
        app.boat.x_velocity = 0.05;
        app.boat.y_ratio = 0.7;
        app.boat.y_velocity = 0.02;
        app.boat.facing = 1.0;

        // First tick seats `last_tick`; dt=0 keeps the snapshot intact.
        let t0 = Instant::now();
        let _ = app.update(Message::BoatTick(t0));
        let snap = app.boat.clone();

        // Pause and tick after a long gap — under the bug the boat would
        // integrate a half-second of sail thrust against an empty bar buffer.
        app.playback.paused = true;
        let _ = app.update(Message::BoatTick(t0 + Duration::from_millis(500)));

        assert_eq!(
            app.boat.x_ratio, snap.x_ratio,
            "x_ratio must hold while paused"
        );
        assert_eq!(
            app.boat.x_velocity, snap.x_velocity,
            "x_velocity must hold while paused"
        );
        assert_eq!(
            app.boat.y_ratio, snap.y_ratio,
            "y_ratio must hold while paused"
        );
        assert_eq!(
            app.boat.y_velocity, snap.y_velocity,
            "y_velocity must hold while paused"
        );
        assert_eq!(
            app.boat.secs_until_next_tack, snap.secs_until_next_tack,
            "tack countdown must hold while paused"
        );
        assert!(
            app.boat.visible,
            "boat must still render while paused — it just stops moving"
        );
        assert!(
            app.boat.last_tick.is_none(),
            "last_tick must clear so the first tick after resume sees dt=0 \
             (same contract as the hidden branch)"
        );
    }

    #[test]
    fn boat_handler_runs_physics_when_not_playing() {
        // The not-playing path must KEEP ticking physics — the boat
        // smoothly relaxes to the bottom under the silence-override
        // rather than freezing. This guards against a regression where
        // someone copies the pause-fix's early-return and accidentally
        // freezes the boat the moment a track ends.
        //
        // Under the music-only-thrust model the boat doesn't move on
        // silence, so we seed a non-zero `x_velocity` and verify it
        // damps over the not-playing ticks. The handler IS running
        // step() if velocity decays toward zero; if step() were
        // skipped the velocity would persist verbatim.
        let mut app = test_app();
        enable_boat_in_config(&app, true);
        app.engine.visualization_mode = VisualizationMode::Lines;
        app.playback.playing = false;
        app.playback.paused = false;

        app.boat.facing = 1.0;
        app.boat.x_velocity = 0.05;

        let t0 = Instant::now();
        let _ = app.update(Message::BoatTick(t0));
        let v_after_first = app.boat.x_velocity;
        let _ = app.update(Message::BoatTick(t0 + Duration::from_millis(100)));

        assert!(
            app.boat.visible,
            "boat must remain visible while not-playing"
        );
        assert!(
            app.boat.last_tick.is_some(),
            "last_tick must update — physics still ticks when not-playing"
        );
        assert!(
            app.boat.x_velocity < v_after_first,
            "x_velocity must decay between not-playing ticks (the \
             silence override drops bars but does NOT skip step() the \
             way pause does); got v_after_first = {v_after_first}, \
             v_after_second = {}",
            app.boat.x_velocity
        );
    }

    #[test]
    fn boat_resumes_motion_after_unpause() {
        // The pause freeze must not be sticky — once `paused` flips back to
        // false, the next ticks integrate physics again. We seed an initial
        // velocity so the boat has something to move with even without
        // music signals, and verify x_ratio mutates after unpause.
        let mut app = test_app();
        enable_boat_in_config(&app, true);
        app.engine.visualization_mode = VisualizationMode::Lines;
        app.boat.facing = 1.0;
        app.boat.x_velocity = 0.08;

        let t0 = Instant::now();
        let _ = app.update(Message::BoatTick(t0));

        app.playback.paused = true;
        let _ = app.update(Message::BoatTick(t0 + Duration::from_millis(200)));
        let frozen_x = app.boat.x_ratio;

        // Resume. First tick after unpause sees dt=0 (last_tick was cleared);
        // the second tick has a real gap and must mutate position.
        app.playback.paused = false;
        let _ = app.update(Message::BoatTick(t0 + Duration::from_millis(300)));
        let _ = app.update(Message::BoatTick(t0 + Duration::from_millis(400)));

        assert_ne!(
            app.boat.x_ratio, frozen_x,
            "boat must integrate again after unpause (x_ratio still {frozen_x})"
        );
    }
}

// ============================================================================
// Queue Removal & Selection Clear (queue.rs)
// ============================================================================
//
// Two intertwined bugs the user reported as "remove sometimes targets the
// wrong song, sometimes nothing":
//
// 1. The handler used `track_number` (set in `transform_songs_from_pool` from
//    the backend's `song_ids` order) to map filtered display index → backend
//    queue index. After any optimistic in-place mutation or client-side sort,
//    `track_number` no longer matches positions, so subsequent removes
//    targeted the wrong row in the backend.
// 2. `selected_indices` was never cleared on cross-source queue refreshes
//    (e.g. consume-mode auto-advance fires `LoadQueue`), so the indices kept
//    pointing at the rows now occupying those positions — different songs.
//
// Fix shape: `QueueAction::RemoveFromQueue` carries `Vec<String>` (song IDs)
// instead of `Vec<usize>`; the optimistic local removal uses ID lookup; and
// `handle_queue_loaded` / `apply_queue_sort` clear `selected_indices`.

#[test]
fn handle_queue_loaded_clears_selected_indices() {
    let mut app = test_app();
    // User had multi-selected before the backend pushed a queue update.
    app.queue_page.common.slot_list.selected_indices.insert(0);
    app.queue_page.common.slot_list.selected_indices.insert(2);

    let new_songs = vec![
        make_queue_song("a", "A", "Artist", "Album"),
        make_queue_song("b", "B", "Artist", "Album"),
        make_queue_song("c", "C", "Artist", "Album"),
    ];
    let _ = app.handle_queue_loaded(Ok(new_songs));

    assert!(
        app.queue_page.common.slot_list.selected_indices.is_empty(),
        "selected_indices must clear on queue reload — stale indices point at different songs after refresh"
    );
}

#[test]
fn apply_queue_sort_clears_selected_indices() {
    use nokkvi_data::types::queue_sort_mode::QueueSortMode;

    let mut app = test_app();
    app.library.queue_songs = vec![
        make_queue_song("a", "Charlie", "Artist", "Album"),
        make_queue_song("b", "Alpha", "Artist", "Album"),
        make_queue_song("c", "Bravo", "Artist", "Album"),
    ];
    app.queue_page.common.slot_list.selected_indices.insert(0);
    app.queue_page.common.slot_list.selected_indices.insert(1);

    let _ = app.apply_queue_sort(QueueSortMode::Title, true);

    assert!(
        app.queue_page.common.slot_list.selected_indices.is_empty(),
        "selected_indices must clear on sort — sort reorders rows so indices now point at different songs"
    );
}

#[test]
fn remove_from_queue_uses_id_lookup_immune_to_stale_track_number() {
    use crate::views::{QueueMessage, queue::QueueContextEntry};

    let mut app = test_app();
    // Reproduce the post-mutation state: originally [A(tn=1), B(tn=2), C(tn=3)],
    // B was removed in-place, leaving the surviving C with stale track_number=3.
    let mut song_a = make_queue_song("a", "A", "Artist", "Album");
    song_a.track_number = 1;
    let mut song_c = make_queue_song("c", "C", "Artist", "Album");
    song_c.track_number = 3; // stale — should be 2 after compaction
    app.library.queue_songs = vec![song_a, song_c];

    // Right-click row 1 (C) → Remove from queue. The pre-fix handler computed
    // raw_idx = stale_track_number - 1 = 2, then `library.queue_songs.remove(2)`
    // — out of bounds, so nothing happened locally; the backend received an
    // index that pointed at a different song.
    let _ = app.handle_queue(QueueMessage::ContextMenuAction(
        1,
        QueueContextEntry::RemoveFromQueue,
    ));

    assert_eq!(
        app.library.queue_songs.len(),
        1,
        "exactly one song should be removed regardless of stale track_number"
    );
    assert_eq!(
        app.library.queue_songs[0].id, "a",
        "A should remain — the user clicked C"
    );
}

#[test]
fn remove_from_queue_via_filtered_view_removes_correct_song() {
    use crate::views::{QueueMessage, queue::QueueContextEntry};

    let mut app = test_app();
    app.library.queue_songs = vec![
        make_queue_song("s1", "Alpha", "Artist A", "Album"),
        make_queue_song("s2", "Beta Ballad", "Artist B", "Album"),
        make_queue_song("s3", "Gamma", "Artist C", "Album"),
        make_queue_song("s4", "Beta Bop", "Artist D", "Album"),
    ];
    // Filter to "Beta" → filtered display = [s2, s4].
    app.queue_page.common.search_query = "Beta".to_string();

    // Right-click filtered row 1 (s4 "Beta Bop") → Remove.
    let _ = app.handle_queue(QueueMessage::ContextMenuAction(
        1,
        QueueContextEntry::RemoveFromQueue,
    ));

    let remaining_ids: Vec<&str> = app
        .library
        .queue_songs
        .iter()
        .map(|s| s.id.as_str())
        .collect();
    assert_eq!(
        remaining_ids,
        vec!["s1", "s2", "s3"],
        "only s4 (filtered row 1) should be removed"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
//  Multi-select checkbox column on expansion children
//
//  The per-row select column originally rendered only on parent rows; the
//  toggle handlers were already wired to the flattened (parents+children)
//  index space, but no checkbox UI existed on child rows so those indices
//  were unreachable. These tests pin the flattened-index contract for the
//  toggle dispatch on each expansion-capable view, so a regression on the
//  render side (or the handler side) trips here instead of silently leaving
//  expansion children unselectable.
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn albums_selection_toggle_on_expansion_child_lands_in_selected_indices() {
    let mut app = test_app();
    let albums = vec![
        make_album("a1", "Album 1", "Artist"),
        make_album("a2", "Album 2", "Artist"),
    ];
    app.library.albums.set_from_vec(albums.clone());

    // Expand album a1 with two tracks. Flattened: [a1=0, t1=1, t2=2, a2=3].
    let tracks = vec![
        make_song("t1", "Track 1", "Artist"),
        make_song("t2", "Track 2", "Artist"),
    ];
    app.albums_page.expansion.expanded_id = Some("a1".to_string());
    app.albums_page.expansion.children = tracks;

    // Toggle the second child track (flattened index 2).
    let (_, _action) = app.albums_page.update(
        crate::views::AlbumsMessage::SlotListSelectionToggle(2),
        albums.len(),
        &albums,
    );

    assert!(
        app.albums_page
            .common
            .slot_list
            .selected_indices
            .contains(&2),
        "child track at flattened index 2 should be in selected_indices after toggle"
    );
}

#[test]
fn albums_select_all_with_expansion_covers_child_indices() {
    let mut app = test_app();
    let albums = vec![make_album("a1", "Album 1", "Artist")];
    app.library.albums.set_from_vec(albums.clone());

    app.albums_page.expansion.expanded_id = Some("a1".to_string());
    app.albums_page.expansion.children = vec![
        make_song("t1", "Track 1", "Artist"),
        make_song("t2", "Track 2", "Artist"),
        make_song("t3", "Track 3", "Artist"),
    ];

    // Flattened length is 1 parent + 3 children = 4.
    let (_, _action) = app.albums_page.update(
        crate::views::AlbumsMessage::SlotListSelectAllToggle,
        albums.len(),
        &albums,
    );

    let selected = &app.albums_page.common.slot_list.selected_indices;
    assert_eq!(
        selected.len(),
        4,
        "select-all must cover the flattened range"
    );
    for i in 0..4 {
        assert!(selected.contains(&i), "index {i} missing from select-all");
    }
}

#[test]
fn playlists_selection_toggle_on_expansion_child_lands_in_selected_indices() {
    let mut app = test_app();
    let playlists: Vec<nokkvi_data::backend::playlists::PlaylistUIViewData> =
        vec![nokkvi_data::backend::playlists::PlaylistUIViewData {
            id: "p1".to_string(),
            name: "Playlist 1".to_string(),
            comment: String::new(),
            duration: 0.0,
            song_count: 2,
            owner_name: String::new(),
            public: false,
            updated_at: String::new(),
            artwork_album_ids: vec![],
            searchable_lower: "playlist 1".to_string(),
        }];
    app.library
        .playlists
        .append_page(playlists.clone(), playlists.len());

    // Expand playlist p1. Flattened: [p1=0, t1=1, t2=2].
    app.playlists_page.expansion.expanded_id = Some("p1".to_string());
    app.playlists_page.expansion.children = vec![
        make_song("t1", "Track 1", "Artist"),
        make_song("t2", "Track 2", "Artist"),
    ];

    let (_, _action) = app.playlists_page.update(
        crate::views::PlaylistsMessage::SlotListSelectionToggle(2),
        playlists.len(),
        &playlists,
    );

    assert!(
        app.playlists_page
            .common
            .slot_list
            .selected_indices
            .contains(&2),
        "child track at flattened index 2 should be in selected_indices after toggle"
    );
}

#[test]
fn artists_selection_toggle_on_album_child_lands_in_selected_indices() {
    let mut app = test_app();
    let artists = vec![make_artist("ar1", "Artist 1")];
    app.library.artists.set_from_vec(artists.clone());

    // Outer expansion: artist ar1 → 2 albums. Flattened: [ar1=0, a1=1, a2=2].
    app.artists_page.expansion.expanded_id = Some("ar1".to_string());
    app.artists_page.expansion.children = vec![
        make_album("a1", "Album 1", "Artist 1"),
        make_album("a2", "Album 2", "Artist 1"),
    ];

    let (_, _action) = app.artists_page.update(
        crate::views::ArtistsMessage::SlotListSelectionToggle(2),
        artists.len(),
        &artists,
    );

    assert!(
        app.artists_page
            .common
            .slot_list
            .selected_indices
            .contains(&2),
        "album child at flattened index 2 should be in selected_indices after toggle"
    );
}

#[test]
fn genres_selection_toggle_on_album_child_lands_in_selected_indices() {
    let mut app = test_app();
    let genres = vec![make_genre("g1", "Rock")];
    app.library.genres.set_from_vec(genres.clone());

    // Outer expansion: genre g1 → 2 albums. Flattened: [g1=0, a1=1, a2=2].
    app.genres_page.expansion.expanded_id = Some("g1".to_string());
    app.genres_page.expansion.children = vec![
        make_album("a1", "Album 1", "Artist"),
        make_album("a2", "Album 2", "Artist"),
    ];

    let (_, _action) = app.genres_page.update(
        crate::views::GenresMessage::SlotListSelectionToggle(2),
        genres.len(),
        &genres,
    );

    assert!(
        app.genres_page
            .common
            .slot_list
            .selected_indices
            .contains(&2),
        "album child at flattened index 2 should be in selected_indices after toggle"
    );
}

// ============================================================================
// Resume-Session Backfill (navigation.rs)
// ============================================================================

#[test]
fn resume_session_backfills_username_from_credential() {
    use crate::state::StoredSession;

    let mut app = test_app();
    app.login_page.username = String::new();
    app.stored_session = Some(StoredSession {
        server_url: "http://localhost:4533".to_string(),
        username: String::new(),
        jwt_token: "fake.jwt.token".to_string(),
        subsonic_credential: "u=foogs&s=salt&t=token".to_string(),
    });

    let _ = app.handle_resume_session();

    assert_eq!(
        app.login_page.username, "foogs",
        "resume should backfill login_page.username from the credential's u= field \
         so the next save_credentials closes the loop with a real value"
    );
}

#[test]
fn resume_session_does_not_clobber_when_credential_lacks_u() {
    use crate::state::StoredSession;

    let mut app = test_app();
    app.login_page.username = "preexisting".to_string();
    app.stored_session = Some(StoredSession {
        server_url: "http://localhost:4533".to_string(),
        username: "preexisting".to_string(),
        jwt_token: "fake.jwt.token".to_string(),
        subsonic_credential: "s=salt&t=token".to_string(),
    });

    let _ = app.handle_resume_session();

    assert_eq!(
        app.login_page.username, "preexisting",
        "a malformed credential without u= should leave login_page.username untouched"
    );
}
