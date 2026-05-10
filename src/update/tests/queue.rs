//! Tests for queue sort, removal, and multi-select update handlers.

use crate::test_helpers::*;

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
        crate::views::AlbumsMessage::SlotList(
            crate::widgets::SlotListPageMessage::SelectionToggle(2),
        ),
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
        crate::views::AlbumsMessage::SlotList(crate::widgets::SlotListPageMessage::SelectAllToggle),
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
