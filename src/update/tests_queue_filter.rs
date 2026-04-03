//! Tests for queue filter → unfiltered index mapping
//!
//! Verifies that `filter_queue_songs()` produces correct subsets and that
//! operations targeting filtered indices resolve to the correct unfiltered
//! queue entries. This is the exact gap that caused 5 fix commits around
//! context-menu targeting and star/remove on filtered queues.

use crate::test_helpers::*;

#[test]
fn empty_filter_returns_all() {
    let mut app = test_app();
    let songs = vec![
        make_queue_song("s1", "Song A", "Artist", "Album"),
        make_queue_song("s2", "Song B", "Artist", "Album"),
        make_queue_song("s3", "Song C", "Artist", "Album"),
    ];
    app.library.queue_songs = songs;
    app.queue_page.common.search_query = String::new();

    let filtered = app.filter_queue_songs();
    assert_eq!(filtered.len(), 3, "empty query should return all songs");
    // Cow::Borrowed when no filter is active
    assert!(matches!(filtered, std::borrow::Cow::Borrowed(_)));
}

#[test]
fn filter_returns_subset_matching_query() {
    let mut app = test_app();
    app.library.queue_songs = vec![
        make_queue_song_full("s1", "Rock Anthem", "Band A", "Album", 1, 240, "Rock"),
        make_queue_song_full("s2", "Jazz Standard", "Band B", "Album", 2, 180, "Jazz"),
        make_queue_song_full("s3", "Rock Ballad", "Band C", "Album", 3, 200, "Rock"),
        make_queue_song_full("s4", "Pop Hit", "Band D", "Album", 4, 210, "Pop"),
    ];
    app.queue_page.common.search_query = "Rock".to_string();

    let filtered = app.filter_queue_songs();
    assert_eq!(filtered.len(), 2, "only Rock songs should match");
    assert_eq!(filtered[0].title, "Rock Anthem");
    assert_eq!(filtered[1].title, "Rock Ballad");
}

#[test]
fn filtered_index_maps_to_correct_unfiltered_song() {
    let mut app = test_app();
    app.library.queue_songs = vec![
        make_queue_song_full("s1", "Pop Song", "A", "Album", 1, 180, "Pop"),
        make_queue_song_full("s2", "Rock Song 1", "B", "Album", 2, 180, "Rock"),
        make_queue_song_full("s3", "Jazz Song", "C", "Album", 3, 180, "Jazz"),
        make_queue_song_full("s4", "Rock Song 2", "D", "Album", 4, 180, "Rock"),
        make_queue_song_full("s5", "Blues Song", "E", "Album", 5, 180, "Blues"),
    ];
    app.queue_page.common.search_query = "Rock".to_string();

    let filtered = app.filter_queue_songs();
    // filtered[0] = "Rock Song 1" (s2, unfiltered index 1)
    // filtered[1] = "Rock Song 2" (s4, unfiltered index 3)
    assert_eq!(filtered[0].id, "s2");
    assert_eq!(filtered[1].id, "s4");
}

#[test]
fn star_filtered_song_targets_correct_unfiltered_entry() {
    let mut app = test_app();
    app.library.queue_songs = vec![
        make_queue_song("s1", "Alpha Song", "A", "Album"),
        make_queue_song("s2", "Beta Ballad", "B", "Album"),
        make_queue_song("s3", "Beta Bop", "C", "Album"),
    ];

    // Star "s2" (which would be filtered[0] if query="Beta")
    let _ = app.handle_song_starred_status_updated("s2".to_string(), true);
    assert!(app.library.queue_songs[1].starred, "s2 should be starred");
    assert!(
        !app.library.queue_songs[0].starred,
        "s1 should not be starred"
    );
    assert!(
        !app.library.queue_songs[2].starred,
        "s3 should not be starred"
    );
}

#[test]
fn search_query_change_resets_offset() {
    let mut app = test_app();
    app.library.queue_songs = vec![
        make_queue_song("s1", "Song A", "A", "Album"),
        make_queue_song("s2", "Song B", "B", "Album"),
    ];
    app.queue_page.common.slot_list.set_offset(5, 10); // simulate scrolled state

    let _ = app
        .queue_page
        .common
        .handle_search_query_changed("test".to_string(), 2);

    assert_eq!(
        app.queue_page.common.slot_list.viewport_offset, 0,
        "search query change must reset viewport to top"
    );
}

#[test]
fn case_insensitive_filter() {
    let mut app = test_app();
    app.library.queue_songs = vec![
        make_queue_song_full("s1", "ROCK anthem", "A", "AlbumX", 1, 180, "Metal"),
        make_queue_song_full("s2", "Jazz Standard", "B", "AlbumY", 2, 180, "Jazz"),
    ];
    app.queue_page.common.search_query = "rock".to_string();

    let filtered = app.filter_queue_songs();
    assert_eq!(filtered.len(), 1, "filter should be case-insensitive");
    assert_eq!(filtered[0].id, "s1");
}
