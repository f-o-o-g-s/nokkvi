//! Tests for queue sort, removal, and multi-select update handlers.

use crate::test_helpers::*;

// ============================================================================
// "Playing From" strip quad cover (app_view.rs)
// ============================================================================

fn blank_handle() -> iced::widget::image::Handle {
    iced::widget::image::Handle::from_bytes(Vec::<u8>::new())
}

/// Activate a playlist context with a queue whose rows span the given albums
/// (one song per album, `make_queue_song` keys album ids as `album_<song
/// id>`). The strip quad snapshot is NOT taken — tests arrange the queue
/// first, then call `snapshot_strip_quad_ids()` to freeze.
fn app_with_active_playlist_queue(song_ids: &[&str]) -> crate::Nokkvi {
    let mut app = test_app();
    app.active_playlist_info = Some(crate::state::ActivePlaylistContext::minimal(
        "pl-1".to_string(),
        "Mix".to_string(),
        String::new(),
    ));
    app.library.queue_songs = song_ids
        .iter()
        .map(|id| make_queue_song(id, "Title", "Artist", "Album"))
        .collect();
    app
}

#[test]
fn strip_quad_none_without_active_playlist() {
    let mut app = app_with_active_playlist_queue(&["s1", "s2", "s3", "s4"]);
    app.snapshot_strip_quad_ids();
    for s in &["s1", "s2", "s3", "s4"] {
        app.artwork
            .album_art
            .put(format!("album_{s}"), blank_handle());
    }
    app.active_playlist_info = None;

    assert!(app.active_playlist_strip_quad().is_none());
}

#[test]
fn strip_quad_resolves_first_four_distinct_queue_albums() {
    let mut app = app_with_active_playlist_queue(&["s1", "s2", "s3", "s4", "s5"]);
    // A repeated album in the leading rows must not consume a quad slot.
    app.library.queue_songs[1].album_id = "album_s1".to_string();
    app.snapshot_strip_quad_ids();
    for s in &["s1", "s3", "s4", "s5"] {
        app.artwork
            .album_art
            .put(format!("album_{s}"), blank_handle());
    }
    // The quad reads the snapshot, not the rendered list — an active search
    // that matches nothing must not blank the strip cover.
    app.queue_page.common.search_query = "no-match".to_string();

    let tiles = app
        .active_playlist_strip_quad()
        .expect("4 distinct cached albums resolve");
    assert_eq!(tiles.len(), 4);
}

#[test]
fn strip_quad_none_when_queue_spans_one_album() {
    let mut app = app_with_active_playlist_queue(&["s1", "s2", "s3"]);
    for song in app.library.queue_songs.iter_mut() {
        song.album_id = "album_s1".to_string();
    }
    app.snapshot_strip_quad_ids();
    app.artwork
        .album_art
        .put("album_s1".to_string(), blank_handle());

    assert!(app.active_playlist_strip_quad().is_none());
}

#[test]
fn strip_quad_none_while_any_tile_cold() {
    let mut app = app_with_active_playlist_queue(&["s1", "s2"]);
    app.snapshot_strip_quad_ids();
    app.artwork
        .album_art
        .put("album_s1".to_string(), blank_handle());

    assert!(app.active_playlist_strip_quad().is_none());
}

/// Queue mutations after the freeze (consume advance, sort, play-next
/// insertions) must not morph the strip quad's identity.
#[test]
fn strip_quad_identity_frozen_across_queue_mutations() {
    let mut app = app_with_active_playlist_queue(&["s1", "s2", "s3", "s4"]);
    app.snapshot_strip_quad_ids();
    let frozen = app.strip_quad_album_ids.clone();
    for s in &["s1", "s2", "s3", "s4"] {
        app.artwork
            .album_art
            .put(format!("album_{s}"), blank_handle());
    }

    // Simulate a consume-mode reload with a foreign play-next insertion: the
    // queue head now leads with different albums.
    app.library.queue_songs = ["s9", "s8", "s3", "s4"]
        .iter()
        .map(|id| make_queue_song(id, "Title", "Artist", "Album"))
        .collect();

    assert_eq!(app.strip_quad_album_ids, frozen);
    let tiles = app
        .active_playlist_strip_quad()
        .expect("frozen ids keep resolving after queue mutation");
    assert_eq!(tiles.len(), 4);
}

/// `handle_queue_loaded` freezes the snapshot only while it is empty — the
/// first queue for a context wins; later reloads leave it alone.
#[test]
fn queue_loaded_freezes_strip_quad_ids_once() {
    let mut app = test_app();
    app.active_playlist_info = Some(crate::state::ActivePlaylistContext::minimal(
        "pl-1".to_string(),
        "Mix".to_string(),
        String::new(),
    ));

    let first: Vec<_> = ["s1", "s2"]
        .iter()
        .map(|id| make_queue_song(id, "Title", "Artist", "Album"))
        .collect();
    let _ = app.handle_queue_loaded(Ok(first));
    let frozen = app.strip_quad_album_ids.clone();
    assert_eq!(frozen, vec!["album_s1".to_string(), "album_s2".to_string()]);

    let second: Vec<_> = ["s7", "s8"]
        .iter()
        .map(|id| make_queue_song(id, "Title", "Artist", "Album"))
        .collect();
    let _ = app.handle_queue_loaded(Ok(second));
    assert_eq!(app.strip_quad_album_ids, frozen);
}

/// Clearing the playlist context drops the frozen quad identity with it.
#[test]
fn clear_active_playlist_drops_strip_quad_ids() {
    let mut app = app_with_active_playlist_queue(&["s1", "s2"]);
    app.snapshot_strip_quad_ids();
    assert!(!app.strip_quad_album_ids.is_empty());

    app.clear_active_playlist();

    assert!(app.strip_quad_album_ids.is_empty());
    assert!(app.active_playlist_strip_quad().is_none());
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
// Fix shape: `QueueAction::RemoveFromQueue` carries `Vec<u64>` of per-row
// `entry_id`s — distinct even when two rows share a song_id, so right-click
// removal targets one row instead of every duplicate. The optimistic local
// removal echoes the same `entry_id` set; `handle_queue_loaded` /
// `apply_queue_sort` clear `selected_indices`.

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

/// Regression: `QueueAction::FocusOnSong` must find the playing row by
/// per-row `entry_id`, drift-immune across the optimistic-mutation window
/// where `track_number` would still carry stale stamps from the
/// pre-mutation projection. The legacy handler did
/// `position(|s| s.track_number == queue_index + 1)`, which silently picked
/// the wrong row when filtered_queue carried out-of-date stamps.
#[test]
fn focus_on_song_finds_row_by_entry_id_through_stale_track_number() {
    use crate::views::QueueMessage;

    let mut app = test_app();
    // Simulate a post-reorder UI: track_numbers are stale (still labelled
    // {1, 2, 3} from the pre-reorder projection, but the rows now sit in
    // a different physical order). entry_ids carry the true row identity.
    let mut song_a = make_queue_song("a", "A", "Artist", "Album");
    song_a.track_number = 3; // stale — UI shows "A" but the pre-reorder
    //                          backend slot for A was index 2 (track_number=3)
    song_a.entry_id = 100;
    let mut song_b = make_queue_song("b", "B", "Artist", "Album");
    song_b.track_number = 1;
    song_b.entry_id = 101;
    let mut song_c = make_queue_song("c", "C", "Artist", "Album");
    song_c.track_number = 2;
    song_c.entry_id = 102;
    app.library.queue_songs = vec![song_a, song_b, song_c];

    // Backend reports "play the row identified by entry_id 101" — that's B,
    // physically at filtered index 1. (The pre-fix handler would have done
    // queue_index + 1 = N+1 and searched track_number == N+1, which would
    // have picked a different row entirely.)
    let _ = app.handle_queue(QueueMessage::FocusCurrentPlaying(101, false));

    assert_eq!(
        app.queue_page.common.slot_list.viewport_offset, 1,
        "FocusOnSong must scroll to entry_id 101's current filtered index, not derive it from stale track_number"
    );
}

/// Regression: two queue rows with the same `song_id` (e.g. "Speak to Me" by
/// Pink Floyd queued twice). Right-clicking one row → Remove must take only
/// that row, never the duplicate sibling. The legacy `Vec<String>` payload
/// keyed by song_id removed both at the optimistic-UI step.
#[test]
fn remove_from_queue_with_duplicate_song_id_removes_only_clicked_row() {
    use crate::views::{QueueMessage, queue::QueueContextEntry};

    let mut app = test_app();
    // Two rows of the same song. `make_queue_song` hands out distinct
    // entry_ids per call, so the duplicate rows are individually addressable.
    let row_first = make_queue_song("dup", "Speak to Me", "Pink Floyd", "Dark Side");
    let row_second = make_queue_song("dup", "Speak to Me", "Pink Floyd", "Dark Side");
    let first_entry_id = row_first.entry_id;
    let second_entry_id = row_second.entry_id;
    assert_ne!(first_entry_id, second_entry_id);
    app.library.queue_songs = vec![row_first, row_second];

    // Right-click filtered row 0 → Remove. Only that row must disappear.
    let _ = app.handle_queue(QueueMessage::ContextMenuAction(
        0,
        QueueContextEntry::RemoveFromQueue,
    ));

    assert_eq!(
        app.library.queue_songs.len(),
        1,
        "removing one of two duplicate rows must leave the other intact"
    );
    assert_eq!(
        app.library.queue_songs[0].entry_id, second_entry_id,
        "the surviving row should be the one the user did not click"
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
    expand_albums_with(
        &mut app,
        "a1",
        vec![
            make_song("t1", "Track 1", "Artist"),
            make_song("t2", "Track 2", "Artist"),
        ],
    );

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

    expand_albums_with(
        &mut app,
        "a1",
        vec![
            make_song("t1", "Track 1", "Artist"),
            make_song("t2", "Track 2", "Artist"),
            make_song("t3", "Track 3", "Artist"),
        ],
    );

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
            uploaded_image: None,
            searchable_lower: "playlist 1".to_string(),
        }];
    app.library
        .playlists
        .append_page(playlists.clone(), playlists.len());

    // Expand playlist p1. Flattened: [p1=0, t1=1, t2=2].
    expand_playlists_with(
        &mut app,
        "p1",
        vec![
            make_song("t1", "Track 1", "Artist"),
            make_song("t2", "Track 2", "Artist"),
        ],
    );

    let (_, _action) = app.playlists_page.update(
        crate::views::PlaylistsMessage::SlotList(
            crate::widgets::SlotListPageMessage::SelectionToggle(2),
        ),
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
    expand_artists_with(
        &mut app,
        "ar1",
        vec![
            make_album("a1", "Album 1", "Artist 1"),
            make_album("a2", "Album 2", "Artist 1"),
        ],
    );

    let (_, _action) = app.artists_page.update(
        crate::views::ArtistsMessage::SlotList(
            crate::widgets::SlotListPageMessage::SelectionToggle(2),
        ),
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
    expand_genres_with(
        &mut app,
        "g1",
        vec![
            make_album("a1", "Album 1", "Artist"),
            make_album("a2", "Album 2", "Artist"),
        ],
    );

    let (_, _action) = app.genres_page.update(
        crate::views::GenresMessage::SlotList(
            crate::widgets::SlotListPageMessage::SelectionToggle(2),
        ),
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
// Hover must not re-run artwork prefetch (b58bee3 flicker regression)
// ============================================================================

/// Regression guard for the queue mini-thumbnail hover flicker introduced by
/// b58bee3. The slot list republishes `HoverEnterSlot` on every `CursorMoved`
/// (`slot_list.rs` `on_move`), so a hover that merely tracks the cursor used to
/// fall through to the `prefetch_album_artwork_tasks` tail. Because the
/// version-aware dedup is keyed by `album_id` but fed the per-song `updated_at`,
/// a same-album queue re-fetched and re-`put` the `album_art` handle every
/// frame, and iced's `Handle::from_bytes` mints a fresh `Id::unique()` texture
/// for identical bytes → continuous flicker while the cursor moved. The hover
/// fast path must short-circuit before the prefetch dispatch while still
/// keeping `hovered_slot` current for cross-pane drag.
///
/// Discriminator: with a real shell and an uncached queue the pre-fix
/// fall-through batches prefetch + `LoadLarge` tasks (`Task::units() >= 1`); the
/// fast path returns `Task::none()` (`units() == 0`). `units()` cannot
/// discriminate without a shell because the prefetch tail short-circuits when
/// `app_service` is `None`.
#[tokio::test]
async fn queue_slot_hover_does_not_dispatch_artwork_prefetch() {
    use super::library::test_app_with_shell;
    use crate::{
        app_message::Message,
        views::QueueMessage,
        widgets::{HoveredSlot, SlotListPageMessage},
    };

    let (mut app, db_path) = test_app_with_shell().await;

    // Seed a non-empty queue + viewport so the (pre-fix) prefetch tail has
    // uncached slots to dispatch fetches for.
    app.library.queue_songs = vec![
        make_queue_song("s1", "Track 1", "Artist", "Album"),
        make_queue_song("s2", "Track 2", "Artist", "Album"),
    ];
    app.queue_page.common.slot_list.slot_count = 8;
    app.queue_page.common.slot_list.viewport_offset = 0;

    let hovered = HoveredSlot::Item {
        slot_index: 0,
        item_index: 0,
        items_len: 2,
    };
    let task = app.update(Message::Queue(QueueMessage::SlotList(
        SlotListPageMessage::HoverEnterSlot(hovered),
    )));

    assert_eq!(
        task.units(),
        0,
        "hovering a queue slot must not spawn artwork prefetch / large-artwork tasks",
    );
    assert_eq!(
        app.queue_page.common.slot_list.hovered_slot,
        Some(hovered),
        "the hover fast path must still record hovered_slot for cross-pane drag",
    );

    let _ = std::fs::remove_file(db_path);
}

// ============================================================================
// build_queue_view_data helper (app_view.rs)
//
// The split-pane and single-view branches share one builder; these pin the
// two parameters that diverge between them and the non-parametrized field
// wiring, so a future field re-order/mis-wire in the single helper is caught.
// ============================================================================

#[test]
fn build_queue_view_data_wires_window_width_and_elevated() {
    let app = test_app();

    let vd = app.build_queue_view_data(640.0, true);
    assert_eq!(vd.window_width, 640.0);
    assert!(vd.elevated);

    let vd2 = app.build_queue_view_data(320.0, false);
    assert_eq!(vd2.window_width, 320.0);
    assert!(!vd2.elevated);
}

#[test]
fn build_queue_view_data_matches_settings_and_counts() {
    let mut app = test_app();
    // Distinct, non-default values so a stable_viewport <-> queue_show_default_playlist
    // cross-wire (or a wrong count source) FAILS. Under plain defaults these fields
    // can both be false, which would let a field swap pass silently.
    app.settings.stable_viewport = true;
    app.settings.queue_show_default_playlist = false;
    app.library.queue_loading_target = Some(7);

    let vd = app.build_queue_view_data(100.0, false);
    assert!(
        vd.stable_viewport,
        "stable_viewport must wire from settings.stable_viewport"
    );
    assert!(
        !vd.show_default_playlist_chip,
        "show_default_playlist_chip must wire from settings.queue_show_default_playlist"
    );
    assert_eq!(
        vd.total_queue_count, 7,
        "total_queue_count must use queue_loading_target"
    );
}

// ============================================================================
// "Unsorted" queue state — the dropdown shows a grayed "Unsorted" placeholder
// until the user applies a queue sort, and reverts honestly thereafter.
// `queue_sorted` is promoted ONLY by `apply_queue_sort`; `handle_queue_loaded`
// demotes it (never promotes) so a queue that merely coincides with a mode is
// never shown as if the user applied it.
// ============================================================================

#[test]
fn queue_unsorted_by_default() {
    let app = test_app();
    assert!(
        !app.queue_page.queue_sorted,
        "a fresh queue shows 'Unsorted' until the user applies a sort"
    );
}

#[test]
fn apply_queue_sort_marks_sorted() {
    use nokkvi_data::types::queue_sort_mode::QueueSortMode;

    let mut app = test_app();
    app.library.queue_songs = vec![
        make_queue_song("a", "Charlie", "Artist", "Album"),
        make_queue_song("b", "Alpha", "Artist", "Album"),
    ];
    assert!(!app.queue_page.queue_sorted, "precondition: unsorted");

    let _ = app.apply_queue_sort(QueueSortMode::Title, true);

    assert!(
        app.queue_page.queue_sorted,
        "applying a queue sort is the sole promoter of the sorted state"
    );
}

#[test]
fn random_shuffle_marks_unsorted() {
    let mut app = test_app();
    app.queue_page.queue_sorted = true;

    // Synchronous mutation runs before the (no-op without app_service) shell task.
    let _ = app.dispatch_random_queue_shuffle();

    assert!(
        !app.queue_page.queue_sorted,
        "a random shuffle has no verifiable sort — the dropdown shows 'Unsorted'"
    );
}

#[test]
fn queue_loaded_preserves_sorted_when_order_still_matches() {
    use nokkvi_data::types::queue_sort_mode::QueueSortMode;

    let mut app = test_app();
    app.queue_page.queue_sort_mode = QueueSortMode::Title;
    app.queue_page.common.sort_ascending = true;
    app.queue_page.queue_sorted = true; // a sort was applied

    // The backend reactive echo arrives in the same (Title-ascending) order.
    let _ = app.handle_queue_loaded(Ok(vec![
        make_queue_song("a", "Apple", "Artist", "Album"),
        make_queue_song("b", "Zebra", "Artist", "Album"),
    ]));

    assert!(
        app.queue_page.queue_sorted,
        "a reload whose order still matches the applied sort keeps the label"
    );
}

#[test]
fn queue_loaded_reverts_to_unsorted_on_external_order() {
    use nokkvi_data::types::queue_sort_mode::QueueSortMode;

    let mut app = test_app();
    app.queue_page.queue_sort_mode = QueueSortMode::Title;
    app.queue_page.common.sort_ascending = true;
    app.queue_page.queue_sorted = true; // a sort was applied

    // An external repopulation (e.g. played an album) lands out of Title order.
    let _ = app.handle_queue_loaded(Ok(vec![
        make_queue_song("a", "Zebra", "Artist", "Album"),
        make_queue_song("b", "Apple", "Artist", "Album"),
    ]));

    assert!(
        !app.queue_page.queue_sorted,
        "a reload whose order no longer matches the applied sort reverts to 'Unsorted'"
    );
}

#[test]
fn queue_loaded_never_promotes_unsorted_even_if_order_coincides() {
    use nokkvi_data::types::queue_sort_mode::QueueSortMode;

    let mut app = test_app();
    app.queue_page.queue_sort_mode = QueueSortMode::Title;
    app.queue_page.common.sort_ascending = true;
    app.queue_page.queue_sorted = false; // unsorted (e.g. just played an album)

    // The incoming order happens to be Title-ascending, but the user never
    // applied a sort — it must stay unsorted (demote-only invariant).
    let _ = app.handle_queue_loaded(Ok(vec![
        make_queue_song("a", "Apple", "Artist", "Album"),
        make_queue_song("b", "Zebra", "Artist", "Album"),
    ]));

    assert!(
        !app.queue_page.queue_sorted,
        "a coincidentally-ordered queue is never auto-promoted — only apply_queue_sort promotes"
    );
}

#[test]
fn queue_is_sorted_trivial_for_short_queues() {
    use nokkvi_data::types::queue_sort_mode::QueueSortMode;

    let mut app = test_app();
    assert!(
        app.queue_is_sorted(QueueSortMode::Title, true),
        "an empty queue is trivially sorted"
    );
    app.library.queue_songs = vec![make_queue_song("a", "Solo", "Artist", "Album")];
    assert!(
        app.queue_is_sorted(QueueSortMode::Title, true),
        "a single-item queue is trivially sorted"
    );
}

#[test]
fn queue_is_sorted_false_for_random() {
    use nokkvi_data::types::queue_sort_mode::QueueSortMode;

    let mut app = test_app();
    app.library.queue_songs = vec![
        make_queue_song("a", "A", "Artist", "Album"),
        make_queue_song("b", "B", "Artist", "Album"),
    ];
    assert!(
        !app.queue_is_sorted(QueueSortMode::Random, true),
        "a shuffled order has no verifiable sort"
    );
}

#[test]
fn queue_is_sorted_detects_misordered_queue() {
    use nokkvi_data::types::queue_sort_mode::QueueSortMode;

    let mut app = test_app();
    app.library.queue_songs = vec![
        make_queue_song("a", "Zebra", "Artist", "Album"),
        make_queue_song("b", "Apple", "Artist", "Album"),
    ];
    assert!(
        !app.queue_is_sorted(QueueSortMode::Title, true),
        "Zebra before Apple is not Title-ascending"
    );
    assert!(
        app.queue_is_sorted(QueueSortMode::Title, false),
        "Zebra before Apple IS Title-descending"
    );
}

#[test]
fn revalidate_queue_sorted_demotes_on_broken_order_keeps_on_intact() {
    use nokkvi_data::types::queue_sort_mode::QueueSortMode;

    let mut app = test_app();
    app.queue_page.queue_sort_mode = QueueSortMode::Title;
    app.queue_page.common.sort_ascending = true;
    app.queue_page.queue_sorted = true;
    app.library.queue_songs = vec![
        make_queue_song("a", "Apple", "Artist", "Album"),
        make_queue_song("b", "Mango", "Artist", "Album"),
        make_queue_song("c", "Zebra", "Artist", "Album"),
    ];

    // Order still matches Title-ascending → stays sorted (e.g. after a removal,
    // which preserves relative order).
    app.revalidate_queue_sorted();
    assert!(
        app.queue_page.queue_sorted,
        "an intact sorted order keeps the sort label"
    );

    // Simulate a drag reorder that breaks Title order → demotes to unsorted.
    app.library.queue_songs.swap(0, 2);
    app.revalidate_queue_sorted();
    assert!(
        !app.queue_page.queue_sorted,
        "a drag reorder that breaks the applied order reverts to 'Unsorted'"
    );
}

/// Regression (review finding): the Shift+↑/↓ hotkey reorder (`handle_move_track`)
/// is a same-length in-place reorder like drag MoveItem, so it must also revert
/// a sorted queue to "Unsorted" when it breaks the applied order.
#[test]
fn move_track_hotkey_reverts_sorted_queue_to_unsorted() {
    use nokkvi_data::types::queue_sort_mode::QueueSortMode;

    let mut app = test_app();
    app.current_view = crate::View::Queue;
    app.queue_page.queue_sort_mode = QueueSortMode::Title;
    app.queue_page.common.sort_ascending = true;
    app.queue_page.queue_sorted = true;
    app.library.queue_songs = vec![
        make_queue_song("a", "Apple", "Artist", "Album"),
        make_queue_song("b", "Bravo", "Artist", "Album"),
        make_queue_song("c", "Charlie", "Artist", "Album"),
    ];

    // Fresh viewport (offset 0) centers on row 0; moving it down breaks Title order.
    let _ = app.handle_move_track(false);

    assert!(
        !app.queue_page.queue_sorted,
        "a Shift+Down hotkey reorder of a sorted queue reverts to 'Unsorted'"
    );
}

/// Regression (review finding): after an external same-length reorder reload,
/// the `sort_queue_songs` short-circuit cache (`last_sort_signature`) must be
/// invalidated so re-applying the same mode actually re-sorts — not render the
/// stale order under the sort label.
#[test]
fn reapplying_sort_after_same_length_reload_actually_resorts() {
    use nokkvi_data::types::queue_sort_mode::QueueSortMode;

    let mut app = test_app();
    app.queue_page.common.sort_ascending = true;
    // `sort_queue_songs` (the local sort inside `apply_queue_sort`) reads
    // `queue_sort_mode`; the real flow sets it via `SortModeSelected` first.
    app.queue_page.queue_sort_mode = QueueSortMode::Title;

    // 1. Apply a Title sort to a scrambled queue.
    app.library.queue_songs = vec![
        make_queue_song("c", "Charlie", "Artist", "Album"),
        make_queue_song("a", "Apple", "Artist", "Album"),
        make_queue_song("b", "Bravo", "Artist", "Album"),
    ];
    let _ = app.apply_queue_sort(QueueSortMode::Title, true);
    assert_eq!(
        app.library
            .queue_songs
            .iter()
            .map(|s| s.title.as_str())
            .collect::<Vec<_>>(),
        vec!["Apple", "Bravo", "Charlie"],
        "precondition: sorted by Title"
    );

    // 2. External reload replaces with the SAME length in a different order
    //    (e.g. a play action / SSE). This demotes to unsorted and must clear
    //    the cached signature.
    let _ = app.handle_queue_loaded(Ok(vec![
        make_queue_song("c", "Charlie", "Artist", "Album"),
        make_queue_song("b", "Bravo", "Artist", "Album"),
        make_queue_song("a", "Apple", "Artist", "Album"),
    ]));
    assert!(
        !app.queue_page.queue_sorted,
        "external reorder reverts to unsorted"
    );
    assert!(
        app.queue_page.last_sort_signature.is_none(),
        "the sort short-circuit cache must be invalidated on out-of-band reorder"
    );

    // 3. Re-apply the same Title sort — it must actually re-sort, not no-op.
    let _ = app.apply_queue_sort(QueueSortMode::Title, true);
    assert_eq!(
        app.library
            .queue_songs
            .iter()
            .map(|s| s.title.as_str())
            .collect::<Vec<_>>(),
        vec!["Apple", "Bravo", "Charlie"],
        "re-applying the same mode after a same-length reload must re-sort the queue"
    );
    assert!(
        app.queue_page.queue_sorted,
        "re-applied sort is shown as sorted"
    );
}

#[test]
fn revalidate_queue_sorted_never_promotes() {
    use nokkvi_data::types::queue_sort_mode::QueueSortMode;

    let mut app = test_app();
    app.queue_page.queue_sort_mode = QueueSortMode::Title;
    app.queue_page.common.sort_ascending = true;
    app.queue_page.queue_sorted = false; // unsorted
    // Perfectly Title-ascending order, but the user never applied a sort.
    app.library.queue_songs = vec![
        make_queue_song("a", "Apple", "Artist", "Album"),
        make_queue_song("b", "Zebra", "Artist", "Album"),
    ];

    app.revalidate_queue_sorted();
    assert!(
        !app.queue_page.queue_sorted,
        "revalidate is demote-only — it never promotes a coincidentally-ordered queue"
    );
}

/// INTERLOCK: `queue_is_sorted` must agree with `sort_queue_songs` for every
/// deterministic mode and direction — after a real sort, the verifier reports
/// the queue as sorted. Guards the two parallel per-mode matches against drift.
#[test]
fn queue_is_sorted_matches_sort_queue_songs() {
    use nokkvi_data::types::queue_sort_mode::QueueSortMode;

    // A queue with every sortable field varied and deliberately scrambled.
    let varied = || {
        let mut songs = vec![
            make_queue_song("a", "Charlie", "Zoe", "Mango"),
            make_queue_song("b", "alpha", "anna", "apple"),
            make_queue_song("c", "Bravo", "Mike", "Banana"),
        ];
        songs[0].duration_seconds = 300;
        songs[0].rating = Some(2);
        songs[0].play_count = Some(50);
        songs[0].genre = "Rock".to_string();
        songs[1].duration_seconds = 120;
        songs[1].rating = Some(5);
        songs[1].play_count = Some(10);
        songs[1].genre = "Ambient".to_string();
        songs[2].duration_seconds = 200;
        songs[2].rating = None;
        songs[2].play_count = Some(99);
        songs[2].genre = "Jazz".to_string();
        songs
    };

    let deterministic = [
        QueueSortMode::Title,
        QueueSortMode::Artist,
        QueueSortMode::Album,
        QueueSortMode::Genre,
        QueueSortMode::Duration,
        QueueSortMode::Rating,
        QueueSortMode::MostPlayed,
    ];

    for mode in deterministic {
        for ascending in [true, false] {
            let mut app = test_app();
            app.library.queue_songs = varied();
            app.queue_page.queue_sort_mode = mode;
            app.queue_page.common.sort_ascending = ascending;
            app.queue_page.last_sort_signature = None; // force the sort to run

            app.sort_queue_songs();

            assert!(
                app.queue_is_sorted(mode, ascending),
                "after sort_queue_songs by {mode:?} (ascending={ascending}), \
                 queue_is_sorted must agree"
            );
        }
    }
}

// ============================================================================
// Drag-reorder: source identity is captured by entry_id at PICK time
//
// Regression for the queue reorder-mislanding bug. `DragColumn` freezes the
// pick SLOT index for the whole gesture; re-resolving it to an item index at
// DROP time against the live `viewport_offset` moves the WRONG row whenever the
// viewport (or buffer) shifted between pick and drop. The common trigger is
// playback auto-follow re-centering the queue on a track change, but a mid-drag
// wheel scroll or queue reload does it too. The fix snapshots the source
// row(s)' per-row `entry_id` at pick time; the destination stays live (it
// follows the cursor on release).
// ============================================================================

/// Build a queue of `n` rows (`s0..s{n-1}`) longer than the slot window so the
/// non-top-packing slot→item mapping (which depends on `viewport_offset`) is
/// exercised.
fn app_with_numbered_queue(n: usize) -> crate::Nokkvi {
    let mut app = test_app();
    app.library.queue_songs = (0..n)
        .map(|i| make_queue_song(&format!("s{i}"), &format!("T{i}"), "Artist", "Album"))
        .collect();
    // Pin a deterministic 9-slot window (center 4) so the math below is fixed.
    app.queue_page.common.slot_list.slot_count = 9;
    app
}

#[test]
fn drag_reorder_moves_pick_time_row_after_midwait_viewport_shift() {
    use crate::{
        views::{QueueAction, QueueMessage},
        widgets::drag_column::DragEvent,
    };

    let mut app = app_with_numbered_queue(20);
    app.queue_page.common.slot_list.set_offset(5, 20);
    let songs = app.library.queue_songs.clone();

    // At offset 5 (effective_center 4): slot s → item s+1, so slot 3 → item 4.
    let grabbed = songs[4].entry_id;
    let _ = app.queue_page.update(
        QueueMessage::DragReorder(DragEvent::Picked { index: 3 }),
        &songs,
    );

    // Mid-drag the playing track advances → auto-follow re-centers the viewport.
    app.queue_page.common.slot_list.set_offset(11, 20);

    // Release over slot 7. The frozen pick slot 3 now maps to item 10 — the bug.
    let (_t, action) = app.queue_page.update(
        QueueMessage::DragReorder(DragEvent::Dropped {
            index: 3,
            target_index: 7,
        }),
        &songs,
    );

    match action {
        QueueAction::MoveItem {
            source_entry_id,
            to,
        } => {
            assert_eq!(
                source_entry_id, grabbed,
                "the moved row must be the one picked (entry_id snapshotted at pick), \
                 not whatever the frozen pick slot maps to after the viewport shifted"
            );
            // At offset 11: slot 7 → item 14. Destination follows the live cursor.
            assert_eq!(to, 14, "destination tracks the live cursor at drop time");
        }
        other => panic!("expected MoveItem, got {other:?}"),
    }
}

#[test]
fn drag_reorder_batch_captured_at_pick_survives_selection_clear() {
    use crate::{
        views::{QueueAction, QueueMessage},
        widgets::drag_column::DragEvent,
    };

    let mut app = app_with_numbered_queue(20);
    app.queue_page.common.slot_list.set_offset(5, 20);
    for i in [3usize, 4, 5] {
        app.queue_page.common.slot_list.selected_indices.insert(i);
    }
    let songs = app.library.queue_songs.clone();
    let want: std::collections::HashSet<u64> =
        [songs[3].entry_id, songs[4].entry_id, songs[5].entry_id]
            .into_iter()
            .collect();

    // Pick a selected member: slot 3 → item 4 ∈ {3,4,5} → batch drag.
    let _ = app.queue_page.update(
        QueueMessage::DragReorder(DragEvent::Picked { index: 3 }),
        &songs,
    );

    // A consume-mode reload mid-drag clears the multi-selection and re-centers.
    app.queue_page.common.slot_list.selected_indices.clear();
    app.queue_page.common.slot_list.set_offset(11, 20);

    let (_t, action) = app.queue_page.update(
        QueueMessage::DragReorder(DragEvent::Dropped {
            index: 3,
            target_index: 7,
        }),
        &songs,
    );

    match action {
        QueueAction::MoveBatch { entry_ids, target } => {
            let got: std::collections::HashSet<u64> = entry_ids.into_iter().collect();
            assert_eq!(
                got, want,
                "the whole pick-time selection must move as a batch even after the \
                 selection set was cleared by a mid-drag reload"
            );
            assert_eq!(target, 14);
        }
        other => panic!("expected MoveBatch, got {other:?}"),
    }
}

#[test]
fn drag_reorder_drop_past_last_row_appends_to_end() {
    use crate::{
        views::{QueueAction, QueueMessage},
        widgets::drag_column::DragEvent,
    };

    // Short queue (top-packing): 3 rows in a 9-slot window.
    let mut app = app_with_numbered_queue(3);
    app.queue_page.common.slot_list.set_offset(0, 3);
    let songs = app.library.queue_songs.clone();
    let grabbed = songs[0].entry_id;

    let _ = app.queue_page.update(
        QueueMessage::DragReorder(DragEvent::Picked { index: 0 }),
        &songs,
    );

    // Cursor released below all rows → `compute_target_index` reports
    // `children.len()` (== slot_count, 9), which maps past the last item.
    let (_t, action) = app.queue_page.update(
        QueueMessage::DragReorder(DragEvent::Dropped {
            index: 0,
            target_index: 9,
        }),
        &songs,
    );

    match action {
        QueueAction::MoveItem {
            source_entry_id,
            to,
        } => {
            assert_eq!(source_entry_id, grabbed);
            assert_eq!(
                to, 3,
                "a drop past the last row appends at end (to == len), not a silent no-op"
            );
        }
        other => panic!("expected MoveItem appending to end, got {other:?}"),
    }
}

/// The destination must be resolved against the LIVE viewport at drop time,
/// including the boundary `effective_center` clamp. When a mid-drag viewport
/// shift moves the list into its end region, `effective_center` clamps away
/// from `slot_count/2`; `slot_to_item_index_for_drop` accounts for that, where
/// a frozen-at-pick mapping would misland by the clamp delta.
#[test]
fn drag_reorder_destination_uses_live_boundary_effective_center() {
    use crate::{
        views::{QueueAction, QueueMessage},
        widgets::drag_column::DragEvent,
    };

    // 14 rows, 9 slots (center 4).
    let mut app = app_with_numbered_queue(14);
    // Pick in the mid region (offset 4 → effective_center 4): slot 4 → item 4.
    app.queue_page.common.slot_list.set_offset(4, 14);
    let songs = app.library.queue_songs.clone();
    let grabbed = songs[4].entry_id;

    let _ = app.queue_page.update(
        QueueMessage::DragReorder(DragEvent::Picked { index: 4 }),
        &songs,
    );

    // Mid-drag auto-follow re-centers near the END: offset 10 → items_after=4,
    // end_push=5, so effective_center clamps to 5 (not 4).
    app.queue_page.common.slot_list.set_offset(10, 14);

    // Drop at slot 3. Live mapping: 10 + (3 - 5) = item 8. A frozen-center
    // formula would have given 10 + (3 - 4) ... = 9 — off by the clamp delta.
    let (_t, action) = app.queue_page.update(
        QueueMessage::DragReorder(DragEvent::Dropped {
            index: 4,
            target_index: 3,
        }),
        &songs,
    );

    match action {
        QueueAction::MoveItem {
            source_entry_id,
            to,
        } => {
            assert_eq!(source_entry_id, grabbed, "source stays the pick-time row");
            assert_eq!(
                to, 8,
                "destination must use the live boundary effective_center (item 8), not 9"
            );
        }
        other => panic!("expected MoveItem, got {other:?}"),
    }
}
