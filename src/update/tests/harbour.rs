//! Tests for the Harbour home view's update handlers.
//!
//! Harbour is now a slot-list `ViewPage`: a single flattened, heterogeneous
//! row list (sections + items + hints) built by `build_harbour_rows`, driven
//! through the shared `SlotList(SlotListPageMessage)` carrier. All assertions
//! are on observable `Nokkvi` / `HarbourPage` state — `app_service` is `None`
//! under `test_app()`, so `shell_task` / play helpers yield no async work.

use nokkvi_data::backend::playlists::PlaylistUIViewData;

use crate::{
    View,
    app_message::{HarbourLoaderMessage, HarbourShelvesData},
    test_helpers::*,
    views::{
        HarbourMessage,
        harbour::{HarbourRow, HarbourSection, HarbourSectionId, build_harbour_rows},
    },
    widgets::SlotListPageMessage,
};

fn search_results_with_genre() -> Box<nokkvi_data::types::library_search::LibrarySearchResults> {
    use nokkvi_data::types::{genre::Genre, library_search::LibrarySearchResults};
    Box::new(LibrarySearchResults {
        genres: vec![Genre {
            id: "g1".into(),
            name: "Ambient".into(),
            album_count: 0,
            song_count: 0,
        }],
        ..Default::default()
    })
}

/// Minimal `PlaylistUIViewData` for shelf tests (no `make_playlist` helper
/// exists — playlists aren't a slot-list library fixture).
fn harbour_playlist(id: &str, name: &str) -> PlaylistUIViewData {
    PlaylistUIViewData {
        id: id.to_string(),
        name: name.to_string(),
        comment: String::new(),
        duration: 0.0,
        song_count: 0,
        owner_name: String::new(),
        public: false,
        updated_at: String::new(),
        artwork_album_ids: Vec::new(),
        uploaded_image: None,
        searchable_lower: name.to_lowercase(),
    }
}

/// Minimal played `Song` for Recently Played shelf tests. `play_date` is set so
/// the subtitle exercises the "Played <relative>" formatting; clear it in a test
/// that needs the no-date path. Mirrors the Similar view's `test_song` fixture
/// (the data crate's `Song::test_default` is `#[cfg(test)]`-gated to that crate).
fn make_recent_song(
    id: &str,
    title: &str,
    artist: &str,
    album_id: &str,
) -> nokkvi_data::types::song::Song {
    nokkvi_data::types::song::Song {
        id: id.to_string(),
        title: title.to_string(),
        artist: artist.to_string(),
        artist_id: None,
        album: "Album".to_string(),
        album_id: Some(album_id.to_string()),
        cover_art: None,
        duration: 180,
        track: None,
        disc: None,
        year: None,
        genre: None,
        path: String::new(),
        size: 0,
        bitrate: None,
        starred: false,
        play_count: None,
        bpm: None,
        channels: None,
        comment: None,
        rating: None,
        album_artist: None,
        suffix: None,
        sample_rate: None,
        created_at: None,
        play_date: Some("2020-01-01T00:00:00Z".to_string()),
        compilation: None,
        bit_depth: None,
        updated_at: None,
        replay_gain: None,
        tags: None,
        participants: None,
        original_position: None,
    }
}

fn shelves_with_albums() -> Box<HarbourShelvesData> {
    Box::new(HarbourShelvesData {
        recently_played: vec![make_recent_song("s1", "Recent Track", "Artist", "al1")],
        recently_added: vec![make_album("a1", "Added", "Artist")],
        most_played_songs: Vec::new(),
        most_played_albums: Vec::new(),
        most_played_artists: Vec::new(),
        most_played_genres: Vec::new(),
        playlists: vec![harbour_playlist("p1", "Mix")],
        genres: vec![make_genre("g1", "Ambient")],
    })
}

/// Seed a radio playback so a play handler's `guard_play_action` has something
/// observable to transition.
fn seed_radio_playback(app: &mut crate::Nokkvi) {
    use crate::state::{ActivePlayback, RadioPlaybackState};
    app.active_playback = ActivePlayback::Radio(RadioPlaybackState {
        station: nokkvi_data::types::radio_station::RadioStation {
            id: "r1".into(),
            name: "Test".into(),
            stream_url: "http://example.invalid/stream".into(),
            home_page_url: None,
            cover_art: None,
        },
        icy_artist: None,
        icy_title: None,
        icy_url: None,
    });
}

#[test]
fn switch_view_to_harbour_sets_current_view() {
    let mut app = test_app();
    assert_eq!(app.current_view, View::Queue); // default

    let _ = app.handle_switch_view(View::Harbour);
    assert_eq!(app.current_view, View::Harbour);
}

#[test]
fn load_harbour_arms_shelf_loading_and_bumps_generation() {
    let mut app = test_app();
    assert!(!app.harbour.shelves_loading);
    let gen_before = app.harbour.shelves_generation;

    let _ = app.handle_load_harbour();

    assert!(
        app.harbour.shelves_loading,
        "LoadHarbour must set the loading flag before dispatching the fetch"
    );
    assert_eq!(
        app.harbour.shelves_generation,
        gen_before.wrapping_add(1),
        "each load bumps the stale-drop generation"
    );
}

#[test]
fn search_changed_captures_query() {
    let mut app = test_app();
    assert!(app.harbour.search_query.is_empty());

    let _ = app.handle_harbour(HarbourMessage::SearchChanged("night".into()));
    assert_eq!(app.harbour.search_query, "night");

    // Clearing the query empties it again.
    let _ = app.handle_harbour(HarbourMessage::SearchChanged(String::new()));
    assert!(app.harbour.search_query.is_empty());
}

#[test]
fn harbour_is_start_view_eligible_and_round_trips() {
    assert_eq!(View::Harbour.start_view_option(), Some("Harbour"));
    assert_eq!(View::from_start_view_name("Harbour"), Some(View::Harbour));
}

#[test]
fn harbour_is_now_a_view_page() {
    // Harbour is a proper slot-list `ViewPage` now — the explicit page lookups
    // resolve to its page (previously they returned `None`).
    let app = test_app();
    assert!(app.view_page(View::Harbour).is_some());
    let mut app = app;
    assert!(app.view_page_mut(View::Harbour).is_some());
}

#[test]
fn switching_off_harbour_does_not_wedge_current_view() {
    // Round-trip through Harbour and back — guards against a missing switch-view
    // arm silently trapping the user.
    let mut app = test_app();
    let _ = app.handle_switch_view(View::Harbour);
    assert_eq!(app.current_view, View::Harbour);
    let _ = app.handle_switch_view(View::Albums);
    assert_eq!(app.current_view, View::Albums);
}

// --- Shelf pipeline ---

#[test]
fn shelves_loaded_populates_all_shelves_and_clears_loading() {
    let mut app = test_app();
    app.harbour.shelves_loading = true;
    let generation = app.harbour.shelves_generation;

    let _ = app.handle_harbour_loader(HarbourLoaderMessage::ShelvesLoaded {
        generation,
        result: Ok(shelves_with_albums()),
    });

    assert!(!app.harbour.shelves_loading, "loading flag cleared");
    assert_eq!(app.harbour.recently_played.len(), 1);
    assert_eq!(app.harbour.recently_added.len(), 1);
    assert_eq!(app.harbour.playlists.len(), 1);
    assert_eq!(app.harbour.genres.len(), 1);
    assert!(!app.harbour.shelves_empty());
}

#[test]
fn shelves_loaded_with_stale_generation_is_dropped() {
    let mut app = test_app();
    // Simulate a newer load having bumped the generation after this result's
    // fetch was dispatched.
    app.harbour.shelves_generation = 5;
    app.harbour.shelves_loading = true;

    let _ = app.handle_harbour_loader(HarbourLoaderMessage::ShelvesLoaded {
        generation: 4, // stale
        result: Ok(shelves_with_albums()),
    });

    assert!(
        app.harbour.shelves_empty(),
        "stale result must not populate"
    );
    assert!(
        app.harbour.shelves_loading,
        "stale result must not clear the loading flag of the newer load"
    );
}

#[test]
fn shelves_load_error_clears_loading_and_toasts() {
    let mut app = test_app();
    app.harbour.shelves_loading = true;
    let generation = app.harbour.shelves_generation;
    assert!(app.toast.toasts.is_empty());

    let _ = app.handle_harbour_loader(HarbourLoaderMessage::ShelvesLoaded {
        generation,
        result: Err("boom".to_string()),
    });

    assert!(!app.harbour.shelves_loading, "loading cleared on error");
    assert!(app.harbour.shelves_empty(), "no shelves on error");
    assert!(!app.toast.toasts.is_empty(), "error surfaces a toast");
}

#[test]
fn playlist_quad_ids_loaded_sets_artwork_album_ids() {
    let mut app = test_app();
    app.harbour.playlists = vec![harbour_playlist("p1", "Mix")];
    let generation = app.harbour.shelves_generation;

    let _ = app.handle_harbour_loader(HarbourLoaderMessage::PlaylistQuadIdsLoaded {
        generation,
        results: vec![("p1".to_string(), vec!["al1".to_string(), "al2".to_string()])],
    });

    assert_eq!(
        app.harbour.playlists[0].artwork_album_ids,
        vec!["al1".to_string(), "al2".to_string()]
    );
}

#[test]
fn playlist_quad_ids_loaded_stale_generation_dropped() {
    let mut app = test_app();
    app.harbour.playlists = vec![harbour_playlist("p1", "Mix")];
    app.harbour.shelves_generation = 9;

    let _ = app.handle_harbour_loader(HarbourLoaderMessage::PlaylistQuadIdsLoaded {
        generation: 8, // stale
        results: vec![("p1".to_string(), vec!["al1".to_string()])],
    });

    assert!(
        app.harbour.playlists[0].artwork_album_ids.is_empty(),
        "stale quad ids must not be applied"
    );
}

#[test]
fn genre_quad_ids_loaded_sets_ids_on_both_shelves_sharing_the_genre() {
    let mut app = test_app();
    // The same genre id appears on BOTH the Random and Most Played Genres
    // shelves. The loader chains `genres` with `most_played_genres` and applies
    // the ids to every match — not just the first shelf — so both resolve their
    // quad tiles (the dual-shelf behavior the single-list playlist path lacks).
    app.harbour.genres = vec![make_genre("Rock", "Rock")];
    app.harbour.most_played_genres = vec![make_genre("Rock", "Rock")];
    let generation = app.harbour.shelves_generation;

    let _ = app.handle_harbour_loader(HarbourLoaderMessage::GenreQuadIdsLoaded {
        generation,
        results: vec![(
            "Rock".to_string(),
            vec!["al1".to_string(), "al2".to_string()],
        )],
    });

    assert_eq!(
        app.harbour.genres[0].artwork_album_ids,
        vec!["al1".to_string(), "al2".to_string()],
        "Random Genres shelf gets the quad ids"
    );
    assert_eq!(
        app.harbour.most_played_genres[0].artwork_album_ids,
        vec!["al1".to_string(), "al2".to_string()],
        "the Most Played Genres shelf sharing the id also gets them"
    );
}

#[test]
fn genre_quad_ids_loaded_stale_generation_dropped() {
    let mut app = test_app();
    app.harbour.genres = vec![make_genre("Rock", "Rock")];
    app.harbour.shelves_generation = 9;

    let _ = app.handle_harbour_loader(HarbourLoaderMessage::GenreQuadIdsLoaded {
        generation: 8, // stale
        results: vec![("Rock".to_string(), vec!["al1".to_string()])],
    });

    assert!(
        app.harbour.genres[0].artwork_album_ids.is_empty(),
        "stale genre quad ids must not be applied"
    );
}

// --- Row model (build_harbour_rows) ---

#[test]
fn shelves_mode_builds_sections_in_order_album_shelves_expanded() {
    let mut app = test_app();
    app.harbour.recently_played = vec![make_recent_song("s3", "Recent", "Artist", "al3")];
    app.harbour.recently_added = vec![make_album("a1", "Added", "Artist")];
    app.harbour.playlists = vec![harbour_playlist("p1", "Mix")];

    let rows = build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate);

    // Section rows for all four shelves, top to bottom.
    let sections: Vec<(HarbourSectionId, bool)> = rows
        .iter()
        .filter_map(|r| match r {
            HarbourRow::Section { id, expanded, .. } => Some((*id, *expanded)),
            _ => None,
        })
        .collect();
    assert_eq!(
        sections,
        vec![
            (HarbourSectionId::RecentlyPlayed, false),
            (HarbourSectionId::RecentlyAdded, false),
            (HarbourSectionId::Playlists, false),
            (HarbourSectionId::Genres, false),
        ],
        "every section is collapsed by default"
    );

    // All sections collapsed, so no item rows are injected even with data seeded.
    let item_titles: Vec<&str> = rows
        .iter()
        .filter_map(|r| match r {
            HarbourRow::Item { title, .. } => Some(title.as_str()),
            _ => None,
        })
        .collect();
    assert!(item_titles.is_empty(), "collapsed sections inject no items");
}

#[test]
fn shelves_mode_caps_each_section_at_hot_picks() {
    use crate::views::harbour::HOT_PICKS_PER_SECTION;
    let mut app = test_app();
    // More songs than the cap allows — the view must clamp to HOT_PICKS.
    app.harbour.recently_played = (0..HOT_PICKS_PER_SECTION + 3)
        .map(|i| {
            make_recent_song(
                &format!("s{i}"),
                &format!("Track {i}"),
                "Artist",
                &format!("al{i}"),
            )
        })
        .collect();
    // Sections start collapsed — expand the one under test so its items render.
    app.harbour_page
        .collapsed
        .remove(&HarbourSectionId::RecentlyPlayed);

    let rows = build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate);
    let item_count = rows
        .iter()
        .filter(|r| matches!(r, HarbourRow::Item { .. }))
        .count();
    assert_eq!(
        item_count, HOT_PICKS_PER_SECTION,
        "an expanded section renders at most HOT_PICKS_PER_SECTION items"
    );
}

#[test]
fn toggling_collapsed_flips_a_sections_expanded() {
    let mut app = test_app();
    app.harbour.playlists = vec![harbour_playlist("p1", "Mix")];

    // Playlists starts collapsed — removing it from the set expands it.
    app.harbour_page
        .collapsed
        .remove(&HarbourSectionId::Playlists);
    let rows = build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate);
    let playlists_expanded = rows.iter().any(|r| {
        matches!(
            r,
            HarbourRow::Section {
                id: HarbourSectionId::Playlists,
                expanded: true,
                ..
            }
        )
    });
    assert!(
        playlists_expanded,
        "removing from collapsed set expands the shelf"
    );
}

#[test]
fn search_query_builds_search_sections_expanded_with_see_all() {
    let mut app = test_app();
    app.harbour.search_query = "amb".into();
    app.harbour.search_results = Some(*search_results_with_genre());

    let rows = build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate);

    // A single non-empty group (genres) → one Section (expanded, See-all) + item.
    assert!(matches!(
        rows.first(),
        Some(HarbourRow::Section {
            id: HarbourSectionId::SearchGenres,
            expanded: true,
            see_all: Some(HarbourSection::Genres),
            ..
        })
    ));
    assert!(matches!(
        rows.get(1),
        Some(HarbourRow::Item { title, .. }) if title == "Ambient"
    ));
}

#[test]
fn short_query_builds_a_single_hint_row() {
    let mut app = test_app();
    app.harbour.search_query = "a".into();

    let rows = build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate);
    assert_eq!(rows.len(), 1);
    assert!(matches!(rows.first(), Some(HarbourRow::Hint(_))));
}

// --- Item subtitle enrichment (build_harbour_rows) ---

/// The subtitle of the single item under an expanded shelf section.
fn item_subtitle(app: &crate::Nokkvi, id: HarbourSectionId) -> String {
    let rows = build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate);
    // Find the section, then the first following Item row.
    let sec = rows
        .iter()
        .position(|r| matches!(r, HarbourRow::Section { id: sid, .. } if *sid == id))
        .expect("section present");
    match rows.get(sec + 1) {
        Some(HarbourRow::Item { subtitle, .. }) => subtitle.clone(),
        other => panic!("expected an item row after the section, got {other:?}"),
    }
}

#[test]
fn recently_played_subtitle_is_artist_when_no_play_date() {
    let mut app = test_app();
    // No play_date → the song subtitle collapses to just the artist.
    let mut s = make_recent_song("s1", "Kiara", "Bonobo", "al1");
    s.play_date = None;
    app.harbour.recently_played = vec![s];
    app.harbour_page
        .collapsed
        .remove(&HarbourSectionId::RecentlyPlayed);

    assert_eq!(
        item_subtitle(&app, HarbourSectionId::RecentlyPlayed),
        "Bonobo"
    );
}

#[test]
fn recently_played_subtitle_prefixes_played_when_dated() {
    let mut app = test_app();
    // make_recent_song sets play_date → subtitle is "Artist • Played <relative>".
    app.harbour.recently_played = vec![make_recent_song("s1", "Kiara", "Bonobo", "al1")];
    app.harbour_page
        .collapsed
        .remove(&HarbourSectionId::RecentlyPlayed);

    let sub = item_subtitle(&app, HarbourSectionId::RecentlyPlayed);
    assert!(
        sub.starts_with("Bonobo • Played ") && sub.ends_with("ago"),
        "got: {sub}"
    );
}

#[test]
fn recently_added_subtitle_joins_artist_and_year() {
    let mut app = test_app();
    // created_at None (deterministic) leaves artist + year.
    let mut a = make_album("a1", "Album", "Aphex Twin");
    a.year = Some(2023);
    app.harbour.recently_added = vec![a];
    app.harbour_page
        .collapsed
        .remove(&HarbourSectionId::RecentlyAdded);

    assert_eq!(
        item_subtitle(&app, HarbourSectionId::RecentlyAdded),
        "Aphex Twin • 2023"
    );
}

#[test]
fn recently_added_subtitle_prefixes_added_when_dated() {
    let mut app = test_app();
    let mut a = make_album("a1", "Album", "Aphex Twin");
    a.created_at = Some("2020-01-01T00:00:00Z".to_string());
    app.harbour.recently_added = vec![a];
    app.harbour_page
        .collapsed
        .remove(&HarbourSectionId::RecentlyAdded);

    let sub = item_subtitle(&app, HarbourSectionId::RecentlyAdded);
    assert!(
        sub.starts_with("Aphex Twin • Added ") && sub.ends_with("ago"),
        "got: {sub}"
    );
}

#[test]
fn playlist_subtitle_shows_song_count_and_duration() {
    let mut app = test_app();
    let mut p = harbour_playlist("p1", "Mix");
    p.song_count = 312;
    p.duration = 18720.0; // 5h 12m
    app.harbour.playlists = vec![p];
    app.harbour_page
        .collapsed
        .remove(&HarbourSectionId::Playlists);

    assert_eq!(
        item_subtitle(&app, HarbourSectionId::Playlists),
        "312 songs • 5h 12m"
    );
}

#[test]
fn genre_subtitle_shows_album_and_song_counts() {
    let mut app = test_app();
    // make_genre defaults: album_count 3, song_count 30.
    app.harbour.genres = vec![make_genre("g1", "Ambient")];
    app.harbour_page.collapsed.remove(&HarbourSectionId::Genres);

    assert_eq!(
        item_subtitle(&app, HarbourSectionId::Genres),
        "3 albums • 30 songs"
    );
}

// --- Section toggling via message + activate-center ---

#[test]
fn toggle_section_flips_page_collapsed_set() {
    let mut app = test_app();
    // RecentlyAdded starts collapsed (in the set) → toggling expands it.
    assert!(
        app.harbour_page
            .collapsed
            .contains(&HarbourSectionId::RecentlyAdded)
    );
    let _ = app.handle_harbour(HarbourMessage::ToggleSection(
        HarbourSectionId::RecentlyAdded,
    ));
    assert!(
        !app.harbour_page
            .collapsed
            .contains(&HarbourSectionId::RecentlyAdded)
    );
    // Toggling again re-collapses it.
    let _ = app.handle_harbour(HarbourMessage::ToggleSection(
        HarbourSectionId::RecentlyAdded,
    ));
    assert!(
        app.harbour_page
            .collapsed
            .contains(&HarbourSectionId::RecentlyAdded)
    );
}

#[test]
fn activate_center_on_section_row_toggles_it() {
    let mut app = test_app();
    app.harbour.recently_played = vec![make_recent_song("s1", "A", "Artist", "al1")];

    let total =
        build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate).len();
    // Row 0 is the RecentlyPlayed section header — focus it.
    app.harbour_page.common.slot_list.set_selected(1, total);

    let _ = app.handle_harbour(HarbourMessage::SlotList(
        SlotListPageMessage::ActivateCenter(false),
    ));

    assert!(
        !app.harbour_page
            .collapsed
            .contains(&HarbourSectionId::RecentlyPlayed),
        "activating a centered collapsed section row expands it"
    );
}

#[test]
fn activate_center_on_item_row_transitions_radio_to_queue() {
    let mut app = test_app();
    app.harbour.recently_played = vec![make_recent_song("s1", "A", "Artist", "al1")];
    seed_radio_playback(&mut app);
    // Expand RecentlyPlayed so its item row exists at row 1.
    app.harbour_page
        .collapsed
        .remove(&HarbourSectionId::RecentlyPlayed);

    let total =
        build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate).len();
    // Row 1 is the album item under the expanded RecentlyPlayed section.
    app.harbour_page.common.slot_list.set_selected(2, total);

    let _ = app.handle_harbour(HarbourMessage::SlotList(
        SlotListPageMessage::ActivateCenter(false),
    ));

    assert!(
        matches!(app.active_playback, crate::state::ActivePlayback::Queue),
        "activating a centered item row plays it and runs the radio-to-queue guard"
    );
}

#[test]
fn activate_center_on_genre_item_transitions_radio_to_queue() {
    let mut app = test_app();
    // A genre item plays via PlayTarget::GenreRandom, a separate play arm that
    // must run the same guard_play_action radio->queue transition as the
    // batch-item arm (an easy copy/paste divergence otherwise).
    let mut g = make_genre("Rock", "Rock");
    g.artwork_album_ids = vec!["al1".into()];
    app.harbour.genres = vec![g];
    seed_radio_playback(&mut app);
    app.harbour_page.collapsed.remove(&HarbourSectionId::Genres);

    let rows = build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate);
    let idx = rows
        .iter()
        .position(|r| {
            matches!(
                r,
                HarbourRow::Item {
                    play: crate::views::harbour::PlayTarget::GenreRandom(_),
                    ..
                }
            )
        })
        .expect("a genre item row (GenreRandom)");
    let total = rows.len();
    app.harbour_page.common.slot_list.set_selected(idx, total);

    let _ = app.handle_harbour(HarbourMessage::SlotList(
        SlotListPageMessage::ActivateCenter(false),
    ));

    assert!(
        matches!(app.active_playback, crate::state::ActivePlayback::Queue),
        "activating a centered genre item runs the radio-to-queue guard"
    );
}

// --- ExpandCenter (Shift+Enter) ---

#[test]
fn expand_center_on_collapsed_section_expands_it() {
    let mut app = test_app();
    app.harbour.playlists = vec![harbour_playlist("p1", "Mix")];
    // Sections start expanded now, so collapse Playlists first to exercise the
    // expand path.
    app.harbour_page
        .collapsed
        .insert(HarbourSectionId::Playlists);

    // Locate the collapsed Playlists section header row and center it.
    let rows = build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate);
    let total = rows.len();
    let idx = rows
        .iter()
        .position(|r| {
            matches!(
                r,
                HarbourRow::Section {
                    id: HarbourSectionId::Playlists,
                    ..
                }
            )
        })
        .expect("Playlists section is present");
    app.harbour_page.common.slot_list.set_selected(idx, total);

    assert!(
        app.harbour_page
            .collapsed
            .contains(&HarbourSectionId::Playlists),
        "Playlists starts collapsed"
    );
    let _ = app.handle_harbour(HarbourMessage::ExpandCenter);
    assert!(
        !app.harbour_page
            .collapsed
            .contains(&HarbourSectionId::Playlists),
        "Shift+Enter on a centered collapsed section expands it"
    );
}

#[test]
fn expand_center_on_item_row_is_a_noop() {
    let mut app = test_app();
    app.harbour.recently_played = vec![make_recent_song("s1", "A", "Artist", "al1")];
    // Expand RecentlyPlayed so its item row exists at row 1.
    app.harbour_page
        .collapsed
        .remove(&HarbourSectionId::RecentlyPlayed);

    // Row 1 is the album item under the expanded RecentlyPlayed section.
    let total =
        build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate).len();
    app.harbour_page.common.slot_list.set_selected(2, total);

    let collapsed_before = app.harbour_page.collapsed.clone();
    let _ = app.handle_harbour(HarbourMessage::ExpandCenter);
    assert_eq!(
        app.harbour_page.collapsed, collapsed_before,
        "Shift+Enter centered on an item toggles no section"
    );
}

// --- Whole-library search lifecycle ---

#[test]
fn search_below_threshold_clears_results_without_loading() {
    let mut app = test_app();
    app.harbour.search_results = Some(*search_results_with_genre());
    let gen_before = app.harbour.search_generation;

    // One char is below the 2-char network threshold.
    let _ = app.handle_harbour(HarbourMessage::SearchChanged("a".into()));

    assert_eq!(app.harbour.search_query, "a");
    assert!(app.harbour.search_results.is_none(), "results cleared");
    assert!(!app.harbour.search_loading, "no load below threshold");
    assert_eq!(
        app.harbour.search_generation,
        gen_before.wrapping_add(1),
        "generation still bumps so a late in-flight result is dropped"
    );
}

#[test]
fn search_at_threshold_sets_loading_and_bumps_generation() {
    let mut app = test_app();
    let gen_before = app.harbour.search_generation;

    let _ = app.handle_harbour(HarbourMessage::SearchChanged("ni".into()));

    assert!(app.harbour.search_loading, "≥2 chars arms the fan-out");
    assert_eq!(app.harbour.search_generation, gen_before.wrapping_add(1));
}

#[test]
fn search_loaded_populates_results_and_clears_loading() {
    let mut app = test_app();
    // Mirror the state after a keystroke fired the fan-out.
    let _ = app.handle_harbour(HarbourMessage::SearchChanged("night".into()));
    let generation = app.harbour.search_generation;

    let _ = app.handle_harbour_loader(HarbourLoaderMessage::SearchLoaded {
        generation,
        result: Ok(search_results_with_genre()),
    });

    assert!(!app.harbour.search_loading);
    assert_eq!(
        app.harbour.search_results.as_ref().map(|r| r.genres.len()),
        Some(1)
    );
}

#[test]
fn search_loaded_stale_generation_is_dropped() {
    let mut app = test_app();
    app.harbour.search_generation = 7;
    app.harbour.search_loading = true;

    let _ = app.handle_harbour_loader(HarbourLoaderMessage::SearchLoaded {
        generation: 6, // stale
        result: Ok(search_results_with_genre()),
    });

    assert!(app.harbour.search_results.is_none(), "stale search dropped");
    assert!(
        app.harbour.search_loading,
        "newer search's loading untouched"
    );
}

#[test]
fn search_error_toasts_clears_loading_and_drops_stale_results() {
    let mut app = test_app();
    let _ = app.handle_harbour(HarbourMessage::SearchChanged("night".into()));
    // Results from the PREVIOUS query are still on screen when the new
    // query's fan-out fails — they must not keep rendering as if they
    // matched the new query.
    app.harbour.search_results = Some(*search_results_with_genre());
    let generation = app.harbour.search_generation;
    assert!(app.toast.toasts.is_empty());

    let _ = app.handle_harbour_loader(HarbourLoaderMessage::SearchLoaded {
        generation,
        result: Err("boom".into()),
    });

    assert!(!app.harbour.search_loading);
    assert!(
        app.harbour.search_results.is_none(),
        "a failed search drops the previous query's stale results"
    );
    assert!(
        !app.toast.toasts.is_empty(),
        "search failure surfaces a toast"
    );
}

#[test]
fn clearing_query_drops_results() {
    let mut app = test_app();
    app.harbour.search_results = Some(*search_results_with_genre());

    let _ = app.handle_harbour(HarbourMessage::SearchChanged(String::new()));

    assert!(app.harbour.search_query.is_empty());
    assert!(
        app.harbour.search_results.is_none(),
        "an empty query restores the shelves (results cleared)"
    );
}

#[test]
fn see_all_albums_routes_to_albums_with_query() {
    let mut app = test_app();
    app.harbour.search_query = "night".into();

    let _ = app.handle_harbour(HarbourMessage::SeeAll(HarbourSection::Albums));

    assert_eq!(app.current_view, View::Albums);
    assert_eq!(app.albums_page.common.search_query, "night");
    assert!(app.albums_page.common.active_filter.is_none());
}

#[test]
fn see_all_playlists_routes_to_playlists_with_query() {
    // Playlists is the target `NavigateAndFilter` never supported — pin it.
    let mut app = test_app();
    app.harbour.search_query = "late".into();

    let _ = app.handle_harbour(HarbourMessage::SeeAll(HarbourSection::Playlists));

    assert_eq!(app.current_view, View::Playlists);
    assert_eq!(app.playlists_page.common.search_query, "late");
    assert!(app.playlists_page.common.active_filter.is_none());
}

#[test]
fn invalidate_shelves_clears_data_and_bumps_generation() {
    let mut app = test_app();
    app.harbour.recently_added = vec![make_album("a1", "A", "Artist")];
    app.harbour.recently_played = vec![make_recent_song("s2", "R", "Artist", "al2")];
    app.harbour.search_query = "night".into();
    app.harbour.search_results = Some(*search_results_with_genre());
    let gen_before = app.harbour.shelves_generation;
    let search_gen_before = app.harbour.search_generation;

    app.harbour.invalidate_shelves();

    assert!(app.harbour.shelves_empty());
    assert_eq!(
        app.harbour.shelves_generation,
        gen_before.wrapping_add(1),
        "invalidation bumps the generation so in-flight loads drop"
    );
    assert!(
        app.harbour.search_results.is_none(),
        "scope-stale search results are dropped (the query is kept for re-fire)"
    );
    assert_eq!(app.harbour.search_query, "night", "the query survives");
    assert_eq!(
        app.harbour.search_generation,
        search_gen_before.wrapping_add(1),
        "an in-flight old-scope search fan-out is generation-dropped"
    );
}

// ============================================================================
// Stationary-center re-warms: the row list / data changes under an unmoved
// center (shelf load, quad-id arrival, section toggle, view entry) — the
// centered row's large art must warm without any NavigateUp/Down/SetOffset.
// ============================================================================

#[test]
fn shelves_loaded_warms_the_centered_collection_header() {
    let mut app = test_app();
    // Pre-load rows are the four always-rendered shelf headers (RecentlyPlayed,
    // RecentlyAdded, Playlists, Genres), all collapsed. Center the Random
    // Playlists header the way a user who scrolled before the fetch landed
    // would (row 0 is centered by default — same class of stationary center).
    let total =
        build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate).len();
    app.harbour_page.common.slot_list.set_selected(3, total);
    let generation = app.harbour.shelves_generation;

    let mut data = shelves_with_albums();
    data.playlists[0].artwork_album_ids = vec!["al1".into(), "al2".into()];
    let _ = app.handle_harbour_loader(HarbourLoaderMessage::ShelvesLoaded {
        generation,
        result: Ok(data),
    });

    assert!(
        app.artwork.playlist.pending.contains("p1"),
        "a landed shelf load must warm the already-centered header's preview \
         collage — no navigation event fires on first load"
    );
}

#[test]
fn playlist_quad_ids_loaded_warms_the_centered_collection() {
    let mut app = test_app();
    app.harbour.playlists = vec![harbour_playlist("p1", "Mix")];
    // Center the Playlists header while its album ids are still unresolved —
    // the ShelvesLoaded-time warm no-ops on empty ids, so the quad-id arrival
    // is the first moment the collage CAN warm.
    let total =
        build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate).len();
    app.harbour_page.common.slot_list.set_selected(3, total);
    let generation = app.harbour.shelves_generation;

    let _ = app.handle_harbour_loader(HarbourLoaderMessage::PlaylistQuadIdsLoaded {
        generation,
        results: vec![("p1".to_string(), vec!["al1".to_string()])],
    });

    assert!(
        app.artwork.playlist.pending.contains("p1"),
        "freshly-resolved album ids must warm the centered collection's collage"
    );
}

#[test]
fn genre_quad_ids_loaded_warms_the_centered_collection() {
    let mut app = test_app();
    app.harbour.genres = vec![make_genre("Rock", "Rock")];
    // Rows: RecentlyPlayed(0) RecentlyAdded(1) Playlists(2) Genres(3).
    let total =
        build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate).len();
    app.harbour_page.common.slot_list.set_selected(4, total);
    let generation = app.harbour.shelves_generation;

    let _ = app.handle_harbour_loader(HarbourLoaderMessage::GenreQuadIdsLoaded {
        generation,
        results: vec![("Rock".to_string(), vec!["al1".to_string()])],
    });

    assert!(
        app.artwork.genre.pending.contains("Rock"),
        "freshly-resolved genre album ids must warm the centered header's collage"
    );
}

#[test]
fn toggle_section_warms_the_row_newly_centered() {
    let mut app = test_app();
    app.harbour.recently_played = vec![make_recent_song("s1", "A", "Artist", "al1")];
    let mut p = harbour_playlist("p1", "Mix");
    p.artwork_album_ids = vec!["al1".into()];
    app.harbour.playlists = vec![p];
    // All collapsed: RP(0) RA(1) PL(2) GE(3) — center the GENRES header.
    let total =
        build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate).len();
    app.harbour_page.common.slot_list.set_selected(4, total);

    // Expanding Recently Played inserts its song row ABOVE the center, so the
    // Playlists header shifts into the centered index — a different row now
    // sits under the panel with no navigation event.
    let _ = app.handle_harbour(HarbourMessage::ToggleSection(
        HarbourSectionId::RecentlyPlayed,
    ));

    assert!(
        app.artwork.playlist.pending.contains("p1"),
        "toggling a section must re-warm whatever row now occupies the center"
    );
}

#[test]
fn activating_a_centered_header_warms_its_preview() {
    let mut app = test_app();
    let mut p = harbour_playlist("p1", "Mix");
    p.artwork_album_ids = vec!["al1".into()];
    app.harbour.playlists = vec![p];
    let total =
        build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate).len();
    app.harbour_page.common.slot_list.set_selected(3, total); // Playlists header

    // Enter on a centered header toggles it — the header stays centered and
    // its preview must warm through the same stationary-center path.
    let _ = app.handle_harbour(HarbourMessage::SlotList(
        SlotListPageMessage::ActivateCenter(false),
    ));

    assert!(
        app.artwork.playlist.pending.contains("p1"),
        "Enter-toggling a centered header must warm its preview collage"
    );
}

#[test]
fn entering_harbour_refires_an_active_search_with_no_results() {
    let mut app = test_app();
    // A library-scope change from another view invalidated the old results
    // (invalidate_shelves keeps the query, drops the results).
    app.harbour.search_query = "night".into();
    app.harbour.search_results = None;
    app.harbour.recently_played = vec![make_recent_song("s1", "A", "Artist", "al1")];
    let gen_before = app.harbour.search_generation;

    let _ = app.handle_switch_view(View::Harbour);

    assert!(
        app.harbour.search_loading,
        "entering Harbour with an orphaned query re-fires the search"
    );
    assert_eq!(app.harbour.search_generation, gen_before.wrapping_add(1));
}

#[test]
fn search_changed_mirrors_common_query_and_resets_viewport() {
    let mut app = test_app();
    // Deep-scroll expanded shelves, then type: the query must mirror into the
    // shared common state (the Escape handler and browsing-panel guard read
    // common.search_query) and the viewport must reset to the top like every
    // other view's search path.
    app.harbour.recently_played = (0..4)
        .map(|i| make_recent_song(&format!("s{i}"), "T", "A", "al"))
        .collect();
    app.harbour_page
        .collapsed
        .remove(&HarbourSectionId::RecentlyPlayed);
    let total =
        build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate).len();
    app.harbour_page
        .common
        .slot_list
        .set_selected(total - 1, total);

    let _ = app.handle_harbour(HarbourMessage::SearchChanged("ni".into()));

    assert_eq!(
        app.harbour_page.common.search_query, "ni",
        "the live query mirrors into the shared slot-list state"
    );
    assert_eq!(
        app.harbour_page.common.slot_list.viewport_offset, 0,
        "a search transition resets the viewport to the top"
    );
    assert!(
        app.harbour_page.common.slot_list.selected_offset.is_none(),
        "a search transition clears the click-to-focus marker"
    );
}

#[test]
fn reset_session_state_preserves_harbour_generations() {
    let mut app = test_app();
    app.harbour.shelves_generation = 5;
    app.harbour.search_generation = 7;
    app.harbour.recently_played = vec![make_recent_song("s1", "A", "Artist", "al1")];

    let _ = app.reset_session_state();

    assert!(app.harbour.shelves_empty(), "logout drops all shelf data");
    assert_eq!(
        app.harbour.shelves_generation, 6,
        "the stale-drop generation carries forward bumped — zeroing it would \
         let a pre-logout in-flight fetch match a fresh post-login load"
    );
    assert_eq!(app.harbour.search_generation, 8);
}

// ============================================================================
// On-center collage warming (300px large-column mosaic)
// ============================================================================

/// Center the first row matching `pred` and warm its artwork, returning nothing
/// — assertions read `app.artwork.{playlist,genre}.pending` afterwards.
fn warm_center_matching(app: &mut crate::Nokkvi, pred: impl Fn(&HarbourRow) -> bool) {
    let rows = build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate);
    let idx = rows
        .iter()
        .position(pred)
        .expect("a row matching the predicate");
    let center = rows.get(idx);
    let _ = app.warm_harbour_center_art(center);
}

#[test]
fn centering_playlist_item_warms_its_collage() {
    let mut app = test_app();
    let mut p = harbour_playlist("p1", "Mix");
    p.artwork_album_ids = vec!["al1".into(), "al2".into(), "al3".into()];
    app.harbour.playlists = vec![p];
    app.harbour_page
        .collapsed
        .remove(&HarbourSectionId::Playlists);

    warm_center_matching(&mut app, |r| matches!(r, HarbourRow::Item { .. }));

    assert!(
        app.artwork.playlist.pending.contains("p1"),
        "centering a playlist item marks its 300px collage pending"
    );
}

#[test]
fn centering_genre_item_warms_its_collage() {
    let mut app = test_app();
    // Production genres have id == name (the LibraryFilter::GenreId convention
    // play_harbour_genre relies on), and a genre item plays via
    // GenreRandom(name), so its collage keys on that name.
    let mut g = make_genre("Rock", "Rock");
    g.artwork_album_ids = vec!["al1".into(), "al2".into()];
    app.harbour.genres = vec![g];
    app.harbour_page.collapsed.remove(&HarbourSectionId::Genres);

    warm_center_matching(&mut app, |r| matches!(r, HarbourRow::Item { .. }));

    assert!(
        app.artwork.genre.pending.contains("Rock"),
        "centering a genre item marks its 300px collage pending"
    );
}

#[test]
fn centering_playlists_section_header_warms_first_picks_collage() {
    let mut app = test_app();
    let mut p1 = harbour_playlist("p1", "Mix");
    p1.artwork_album_ids = vec!["al1".into(), "al2".into()];
    app.harbour.playlists = vec![p1, harbour_playlist("p2", "Other")];
    // Playlists starts collapsed — its header is the centered row.

    warm_center_matching(&mut app, |r| {
        matches!(
            r,
            HarbourRow::Section {
                id: HarbourSectionId::Playlists,
                ..
            }
        )
    });

    assert!(
        app.artwork.playlist.pending.contains("p1"),
        "centering the Playlists header warms its first pick's collage (the one the pill names)"
    );
}

#[test]
fn centering_album_item_warms_no_collage() {
    let mut app = test_app();
    app.harbour.recently_added = vec![make_album("a1", "Album", "Artist")];
    app.harbour_page
        .collapsed
        .remove(&HarbourSectionId::RecentlyAdded);

    warm_center_matching(&mut app, |r| matches!(r, HarbourRow::Item { .. }));

    assert!(
        app.artwork.playlist.pending.is_empty() && app.artwork.genre.pending.is_empty(),
        "an album item warms only its single large cover — no collage pipeline"
    );
}

#[test]
fn centering_collection_without_album_ids_warms_no_collage() {
    let mut app = test_app();
    // A playlist whose album ids have not resolved yet: nothing to tile, so the
    // collage warm must not mark it pending (it would fetch zero tiles).
    app.harbour.playlists = vec![harbour_playlist("p1", "Mix")];
    app.harbour_page
        .collapsed
        .remove(&HarbourSectionId::Playlists);

    warm_center_matching(&mut app, |r| matches!(r, HarbourRow::Item { .. }));

    assert!(
        !app.artwork.playlist.pending.contains("p1"),
        "a collection with no resolved album ids is not marked pending"
    );
}

// ============================================================================
// Whole-library search artwork warming
// ============================================================================

#[test]
fn search_warm_album_ids_covers_albums_and_songs_only() {
    use nokkvi_data::types::{genre::Genre, library_search::LibrarySearchResults};

    use crate::update::harbour::search_warm_album_ids;

    let mut song_no_album = make_recent_song("s2", "No Album", "Artist", "unused");
    song_no_album.album_id = None;

    let results = LibrarySearchResults {
        // Songs contribute their album_id; one without an album_id is skipped.
        songs: vec![
            make_recent_song("s1", "Track", "Artist", "song_al"),
            song_no_album,
        ],
        // Genres/playlists have no resolved album ids → contribute nothing.
        genres: vec![Genre {
            id: "g1".into(),
            name: "Ambient".into(),
            album_count: 0,
            song_count: 0,
        }],
        ..Default::default()
    };

    let ids = search_warm_album_ids(&results);
    assert_eq!(
        ids,
        vec!["song_al".to_string()],
        "only song rows with an album_id contribute a cover to warm; artists/genres/playlists do not"
    );
}

// ============================================================================
// Full-parity search thumbnails (artists / genres / playlists)
// ============================================================================

/// Minimal raw `Artist` for search-result tests (no Default derive on the type).
fn search_artist(id: &str, name: &str) -> nokkvi_data::types::artist::Artist {
    nokkvi_data::types::artist::Artist {
        id: id.to_string(),
        name: name.to_string(),
        album_count: None,
        song_count: None,
        starred: None,
        starred_at: None,
        large_image_url: None,
        medium_image_url: None,
        small_image_url: None,
        play_count: None,
        play_date: None,
        size: None,
        mbz_artist_id: None,
        biography: None,
        similar_artists: None,
        external_url: None,
        external_info_updated_at: None,
        rating: None,
    }
}

#[test]
fn artist_search_row_resolves_its_image_from_the_artist_id() {
    use nokkvi_data::types::library_search::LibrarySearchResults;

    let mut app = test_app();
    app.harbour.search_query = "aphex".into();
    app.harbour.search_results = Some(LibrarySearchResults {
        artists: vec![search_artist("ar1", "Aphex Twin")],
        ..Default::default()
    });

    let rows = build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate);
    // The artist Item row (subtitle "Artist") must key its thumbnail on the
    // artist id — artist images live in album_art keyed by artist id.
    let art = rows
        .iter()
        .find_map(|r| match r {
            HarbourRow::Item {
                subtitle,
                art_album_id,
                ..
            } if subtitle == "Artist" => Some(art_album_id.clone()),
            _ => None,
        })
        .expect("an artist search row");
    assert_eq!(art, Some("ar1".to_string()));
}

#[test]
fn genre_search_row_reads_resolved_album_ids_from_the_side_map() {
    let mut app = test_app();
    app.harbour.search_query = "amb".into();
    app.harbour.search_results = Some(*search_results_with_genre()); // genre "Ambient"
    app.harbour
        .search_genre_album_ids
        .insert("Ambient".into(), vec!["al1".into(), "al2".into()]);

    let rows = build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate);
    let ids = rows
        .iter()
        .find_map(|r| match r {
            HarbourRow::Item { art_album_ids, .. } if !art_album_ids.is_empty() => {
                Some(art_album_ids.clone())
            }
            _ => None,
        })
        .expect("a genre search row with resolved quad ids");
    assert_eq!(ids, vec!["al1".to_string(), "al2".to_string()]);
}

#[test]
fn search_collage_ids_loaded_fills_the_target_side_map() {
    use crate::app_message::CollageTarget;

    let mut app = test_app();
    let generation = app.harbour.search_generation;

    let _ = app.handle_harbour_loader(HarbourLoaderMessage::SearchCollageIdsLoaded {
        generation,
        target: CollageTarget::Genre,
        results: vec![("Rock".into(), vec!["al1".into(), "al2".into()])],
    });
    assert_eq!(
        app.harbour.search_genre_album_ids.get("Rock"),
        Some(&vec!["al1".to_string(), "al2".to_string()]),
    );
    // Wrong target's map stays empty.
    assert!(app.harbour.search_playlist_album_ids.is_empty());
}

#[test]
fn search_collage_ids_loaded_still_caches_under_stale_generation() {
    use crate::app_message::CollageTarget;

    let mut app = test_app();
    app.harbour.search_generation = 5;

    // A stale result (older keystroke) still populates the side-map: album ids
    // are query-independent, so caching them dedups the fan-out across
    // keystrokes. Only the warm/re-render is gated on the current generation.
    let _ = app.handle_harbour_loader(HarbourLoaderMessage::SearchCollageIdsLoaded {
        generation: 4, // stale — a newer keystroke has moved on
        target: CollageTarget::Playlist,
        results: vec![("p1".into(), vec!["al1".into()])],
    });
    assert_eq!(
        app.harbour.search_playlist_album_ids.get("p1"),
        Some(&vec!["al1".to_string()]),
        "resolved album ids are cached regardless of generation to dedup re-fan-out"
    );
}

#[test]
fn invalidate_shelves_clears_search_collage_side_maps() {
    let mut app = test_app();
    app.harbour
        .search_genre_album_ids
        .insert("Rock".into(), vec!["al1".into()]);
    app.harbour
        .search_playlist_album_ids
        .insert("p1".into(), vec!["al2".into()]);

    app.harbour.invalidate_shelves();

    assert!(app.harbour.search_genre_album_ids.is_empty());
    assert!(app.harbour.search_playlist_album_ids.is_empty());
}

#[test]
fn centering_artist_search_row_warms_the_artist_large_image() {
    use nokkvi_data::types::library_search::LibrarySearchResults;

    let mut app = test_app();
    app.harbour.search_query = "aphex".into();
    app.harbour.search_results = Some(LibrarySearchResults {
        artists: vec![search_artist("ar1", "Aphex Twin")],
        ..Default::default()
    });

    // Centering an artist row warms its large image via the artist endpoint
    // (observable in-flight marker), NOT an album LoadLarge that would 404.
    warm_center_matching(
        &mut app,
        |r| matches!(r, HarbourRow::Item { subtitle, .. } if subtitle == "Artist"),
    );

    assert_eq!(
        app.artwork.loading_large_artwork.as_deref(),
        Some("ar1"),
        "an artist row warms its large image via handle_load_artist_large_artwork"
    );
}

// ============================================================================
// Most Played shelves
// ============================================================================

fn played_song(id: &str, genre: &str, plays: u32) -> nokkvi_data::types::song::Song {
    let mut s = make_recent_song(id, "Track", "Artist", "al1");
    s.genre = Some(genre.to_string());
    s.play_count = Some(plays);
    s
}

#[test]
fn tally_genres_by_play_ranks_by_summed_plays() {
    use crate::update::harbour::tally_genres_by_play;

    // Ambient: 100 plays (1 track). Techno: 50+30 = 80 (2 tracks). Jazz: 5 (1).
    let songs = vec![
        played_song("1", "Techno", 50),
        played_song("2", "Techno", 30),
        played_song("3", "Ambient", 100),
        played_song("4", "Jazz", 5),
    ];
    let genres = tally_genres_by_play(&songs);

    let names: Vec<&str> = genres.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["Ambient", "Techno", "Jazz"],
        "ranked by summed plays"
    );
    let techno = genres.iter().find(|g| g.name == "Techno").unwrap();
    assert_eq!(techno.song_count, 2, "song_count carries the track share");
}

#[test]
fn tally_genres_skips_songs_without_a_genre_and_caps_at_hot_picks() {
    use crate::{update::harbour::tally_genres_by_play, views::harbour::HOT_PICKS_PER_SECTION};

    // A genreless song with the highest play count of all: if it were counted it
    // would rank #1 with an empty genre name. It must contribute nothing.
    let mut genreless = played_song("x", "unused", 9999);
    genreless.genre = None;
    let mut songs = vec![genreless];
    // Comfortably more distinct genres than the cap so truncation is exercised
    // (a plain literal, not an enum discriminant repurposed as a count).
    for i in 0..12 {
        songs.push(played_song(
            &format!("g{i}"),
            &format!("Genre {i}"),
            (i as u32) + 1,
        ));
    }

    let genres = tally_genres_by_play(&songs);

    assert!(
        genres.len() <= HOT_PICKS_PER_SECTION,
        "tally is capped at HOT_PICKS_PER_SECTION"
    );
    assert!(
        !genres.is_empty(),
        "the genre'd songs still produce a tally"
    );
    assert!(
        genres.iter().all(|g| !g.name.is_empty()),
        "the high-play genreless song contributes no empty-name genre"
    );
}

#[test]
fn most_played_tracks_shelf_shows_play_count_subtitle() {
    let mut app = test_app();
    app.harbour.most_played_songs = vec![played_song("s1", "Techno", 42)];
    app.harbour_page
        .collapsed
        .remove(&HarbourSectionId::MostPlayedTracks);

    let rows = build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate);
    assert!(
        rows.iter().any(|r| matches!(
            r,
            HarbourRow::Section {
                id: HarbourSectionId::MostPlayedTracks,
                ..
            }
        )),
        "the Most Played Tracks header renders when populated"
    );
    let sub = rows
        .iter()
        .find_map(|r| match r {
            HarbourRow::Item { subtitle, .. } => Some(subtitle.clone()),
            _ => None,
        })
        .expect("a track item");
    assert!(
        sub.contains("42 plays"),
        "subtitle shows play count, not a recency date (got: {sub})"
    );
}

#[test]
fn most_played_shelf_hidden_when_top_item_has_zero_plays() {
    let mut app = test_app();
    // A fresh/low-play library: the top "most played" track has no plays.
    app.harbour.most_played_songs = vec![played_song("s1", "Techno", 0)];

    let rows = build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate);
    assert!(
        !rows.iter().any(|r| matches!(
            r,
            HarbourRow::Section {
                id: HarbourSectionId::MostPlayedTracks,
                ..
            }
        )),
        "a zero-play Most Played shelf is hidden so it never shows arbitrary rows"
    );
}

#[test]
fn most_played_artist_row_keys_thumbnail_on_artist_id() {
    let mut a = search_artist("ar1", "Aphex Twin");
    a.play_count = Some(99);
    let mut app = test_app();
    app.harbour.most_played_artists = vec![a];
    app.harbour_page
        .collapsed
        .remove(&HarbourSectionId::MostPlayedArtists);

    let rows = build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate);
    let art = rows
        .iter()
        .find_map(|r| match r {
            HarbourRow::Item {
                art_album_id,
                subtitle,
                ..
            } if subtitle == "99 plays" => Some(art_album_id.clone()),
            _ => None,
        })
        .expect("an artist row with a play-count subtitle");
    assert_eq!(
        art,
        Some("ar1".to_string()),
        "artist rows key their thumbnail on the artist id"
    );
}

#[test]
fn shelves_loaded_populates_most_played_shelves() {
    let mut app = test_app();
    let generation = app.harbour.shelves_generation;
    let mut data = shelves_with_albums();
    data.most_played_songs = vec![played_song("s1", "Techno", 42)];
    data.most_played_albums = vec![make_album("a2", "Top Album", "Artist")];
    data.most_played_artists = vec![search_artist("ar1", "Top Artist")];
    data.most_played_genres = vec![make_genre("Techno", "Techno")];

    let _ = app.handle_harbour_loader(HarbourLoaderMessage::ShelvesLoaded {
        generation,
        result: Ok(data),
    });

    assert_eq!(app.harbour.most_played_songs.len(), 1);
    assert_eq!(app.harbour.most_played_albums.len(), 1);
    assert_eq!(app.harbour.most_played_artists.len(), 1);
    assert_eq!(app.harbour.most_played_genres.len(), 1);
}

#[test]
fn most_played_genres_hidden_when_tally_is_empty() {
    let mut app = test_app();
    // Top tracks have plays, but the genre tally produced nothing (e.g. the top
    // tracks carry no genre tags) — the genre shelf must not render a "(0)"
    // header just because tracks have plays.
    app.harbour.most_played_songs = vec![played_song("s1", "Techno", 42)];
    app.harbour.most_played_genres = Vec::new();

    let rows = build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate);
    assert!(
        !rows.iter().any(|r| matches!(
            r,
            HarbourRow::Section {
                id: HarbourSectionId::MostPlayedGenres,
                ..
            }
        )),
        "Most Played Genres hides when the tally is empty, even with played tracks"
    );

    // With a tallied genre present, it renders.
    app.harbour.most_played_genres = vec![make_genre("Techno", "Techno")];
    let rows = build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate);
    assert!(
        rows.iter().any(|r| matches!(
            r,
            HarbourRow::Section {
                id: HarbourSectionId::MostPlayedGenres,
                ..
            }
        )),
        "Most Played Genres renders once the tally has a genre"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Trawl row — the mix-builder door (first slot in shelves mode)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn trawl_row_is_first_in_shelves_mode() {
    let app = test_app();
    let rows = build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate);
    assert!(
        matches!(rows.first(), Some(HarbourRow::Trawl { .. })),
        "the Trawl door leads the shelves index"
    );
}

#[test]
fn trawl_row_absent_in_search_mode() {
    let mut app = test_app();
    app.harbour.search_query = "night".into();
    app.harbour.search_results =
        Some(nokkvi_data::types::library_search::LibrarySearchResults::default());
    let rows = build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate);
    assert!(
        !rows.iter().any(|r| matches!(r, HarbourRow::Trawl { .. })),
        "search mode shows results only"
    );
}

#[test]
fn trawl_row_carries_crate_count_and_blend() {
    let mut app = test_app();
    app.trawl_crate
        .add(nokkvi_data::types::trawl::TrawlSeed::new(
            nokkvi_data::types::batch::BatchItem::Album("al1".into()),
            "A",
            "Artist",
        ));
    app.trawl_crate.blend = nokkvi_data::types::trawl::TrawlBlend::Weighted;
    let rows = build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate);
    assert!(
        matches!(
            rows.first(),
            Some(HarbourRow::Trawl {
                seeds: 1,
                blend: nokkvi_data::types::trawl::TrawlBlend::Weighted,
            })
        ),
        "row mirrors the live crate"
    );
}

#[test]
fn trawl_subtitle_copy() {
    use nokkvi_data::types::trawl::TrawlBlend;

    use crate::views::harbour::trawl_row_subtitle;
    assert_eq!(
        trawl_row_subtitle(0, TrawlBlend::Interleave),
        "Build a mix from anything in the library"
    );
    assert_eq!(
        trawl_row_subtitle(1, TrawlBlend::Interleave),
        "1 seed • Interleave"
    );
    assert_eq!(
        trawl_row_subtitle(3, TrawlBlend::ShuffleAll),
        "3 seeds • Shuffle all"
    );
}

#[test]
fn activate_center_on_trawl_row_opens_the_modal() {
    let mut app = test_app();
    let total =
        build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate).len();
    app.harbour_page.common.slot_list.set_selected(0, total);
    assert!(app.trawl_modal.is_none());

    let _ = app.handle_harbour(HarbourMessage::SlotList(
        SlotListPageMessage::ActivateCenter(false),
    ));

    assert!(
        app.trawl_modal.is_some(),
        "activating the Trawl row opens the modal"
    );
    assert!(
        app.harbour_page.collapsed.len() == 8,
        "no section collapse state was touched"
    );
}

#[test]
fn add_center_to_queue_on_trawl_row_is_noop() {
    let mut app = test_app();
    let total =
        build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate).len();
    app.harbour_page.common.slot_list.set_selected(0, total);

    let _ = app.handle_harbour(HarbourMessage::SlotList(
        SlotListPageMessage::AddCenterToQueue,
    ));

    assert!(app.toast.toasts.is_empty(), "nothing to enqueue, no toast");
    assert!(app.trawl_modal.is_none(), "and no modal either");
}

#[test]
fn expand_center_on_trawl_row_is_noop() {
    let mut app = test_app();
    let total =
        build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate).len();
    app.harbour_page.common.slot_list.set_selected(0, total);
    let collapsed_before = app.harbour_page.collapsed.clone();

    let _ = app.handle_harbour(HarbourMessage::ExpandCenter);

    assert_eq!(
        app.harbour_page.collapsed, collapsed_before,
        "Shift+Enter on the Trawl row toggles nothing"
    );
}

// ============================================================================
// Section-header teasers + glyphs (first-pick teaser rows)
// ============================================================================

/// The section header's teaser fields, cloned out of a fresh row build:
/// `(subtitle, art_album_id, art_album_ids, custom_playlist_id)`. `None` =
/// empty shelf (the header renders its glyph + "Nothing here yet").
#[allow(clippy::type_complexity)]
fn header_teaser(
    app: &crate::Nokkvi,
    id: HarbourSectionId,
) -> Option<(String, Option<String>, Vec<String>, Option<String>)> {
    let rows = build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate);
    rows.iter()
        .find_map(|r| match r {
            HarbourRow::Section {
                id: sid, teaser, ..
            } if *sid == id => Some(teaser.as_ref().map(|t| {
                (
                    t.subtitle.clone(),
                    t.art_album_id.clone(),
                    t.art_album_ids.clone(),
                    t.custom_playlist_id.clone(),
                )
            })),
            _ => None,
        })
        .expect("section header present")
}

/// Minimal raw `Album` for search-result fixtures (no `Default` impl; serde
/// fills every `Option` field — mirrors split_view.rs's `similar_song`).
fn search_album(id: &str, name: &str, artist: &str) -> nokkvi_data::types::album::Album {
    serde_json::from_value(serde_json::json!({ "id": id, "name": name, "artist": artist }))
        .expect("minimal Album JSON should deserialize")
}

/// Minimal raw `Playlist` for search-result fixtures.
fn search_playlist(
    id: &str,
    name: &str,
    song_count: u32,
) -> nokkvi_data::types::playlist::Playlist {
    serde_json::from_value(serde_json::json!({ "id": id, "name": name, "songCount": song_count }))
        .expect("minimal Playlist JSON should deserialize")
}

#[test]
fn section_icon_maps_every_section_to_a_shipped_glyph() {
    use crate::views::harbour::section_icon;

    assert_eq!(
        section_icon(HarbourSectionId::RecentlyPlayed),
        "assets/icons/clock.svg"
    );
    assert_eq!(
        section_icon(HarbourSectionId::RecentlyAdded),
        "assets/icons/calendar.svg"
    );
    // Tracks/Songs share the Songs vocabulary glyph (music-2, per nav_bar).
    assert_eq!(
        section_icon(HarbourSectionId::MostPlayedTracks),
        "assets/icons/music-2.svg"
    );
    assert_eq!(
        section_icon(HarbourSectionId::SearchSongs),
        "assets/icons/music-2.svg"
    );
    assert_eq!(
        section_icon(HarbourSectionId::MostPlayedAlbums),
        "assets/icons/disc-3.svg"
    );
    assert_eq!(
        section_icon(HarbourSectionId::SearchAlbums),
        "assets/icons/disc-3.svg"
    );
    assert_eq!(
        section_icon(HarbourSectionId::MostPlayedArtists),
        "assets/icons/mic.svg"
    );
    assert_eq!(
        section_icon(HarbourSectionId::SearchArtists),
        "assets/icons/mic.svg"
    );
    assert_eq!(
        section_icon(HarbourSectionId::MostPlayedGenres),
        "assets/icons/tags.svg"
    );
    assert_eq!(
        section_icon(HarbourSectionId::Genres),
        "assets/icons/tags.svg"
    );
    assert_eq!(
        section_icon(HarbourSectionId::SearchGenres),
        "assets/icons/tags.svg"
    );
    assert_eq!(
        section_icon(HarbourSectionId::Playlists),
        "assets/icons/list-music.svg"
    );
    assert_eq!(
        section_icon(HarbourSectionId::SearchPlaylists),
        "assets/icons/list-music.svg"
    );
}

#[test]
fn every_section_header_carries_its_section_icon_glyph() {
    use crate::views::harbour::section_icon;

    let mut app = test_app();
    app.harbour.recently_played = vec![make_recent_song("s1", "A", "Artist", "al1")];
    app.harbour.playlists = vec![harbour_playlist("p1", "Mix")];

    let rows = build_harbour_rows(&app.harbour, &app.harbour_page.collapsed, &app.trawl_crate);
    for row in &rows {
        if let HarbourRow::Section { id, glyph, .. } = row {
            assert_eq!(
                *glyph,
                section_icon(*id),
                "the header glyph routes through section_icon ({id:?})"
            );
        }
    }
}

#[test]
fn recently_played_header_teaser_names_the_first_pick() {
    use nokkvi_data::utils::formatters::format_relative_time;

    let mut app = test_app();
    app.harbour.recently_played = vec![make_recent_song("s1", "Karma Police", "Radiohead", "al1")];

    let (subtitle, art_id, art_ids, custom) =
        header_teaser(&app, HarbourSectionId::RecentlyPlayed).expect("populated shelf teases");
    assert_eq!(
        subtitle,
        format!(
            "Karma Police • Radiohead • Played {}",
            format_relative_time("2020-01-01T00:00:00Z")
        ),
        "the teaser reuses the item rows' relative-time formatting"
    );
    assert!(subtitle.ends_with("ago"), "got: {subtitle}");
    assert_eq!(art_id, Some("al1".to_string()));
    assert_eq!(art_ids, vec!["al1".to_string()]);
    assert_eq!(custom, None);
}

#[test]
fn recently_played_teaser_drops_a_missing_play_date() {
    let mut app = test_app();
    let mut s = make_recent_song("s1", "Karma Police", "Radiohead", "al1");
    s.play_date = None;
    app.harbour.recently_played = vec![s];

    let (subtitle, ..) =
        header_teaser(&app, HarbourSectionId::RecentlyPlayed).expect("teaser present");
    assert_eq!(
        subtitle, "Karma Police • Radiohead",
        "a missing fact drops — no placeholder"
    );
}

#[test]
fn recently_added_header_teaser_drops_the_year() {
    use nokkvi_data::utils::formatters::format_relative_time;

    let mut app = test_app();
    let mut a = make_album("al9", "In Rainbows", "Radiohead");
    a.year = Some(2007);
    a.created_at = Some("2020-01-01T00:00:00Z".to_string());
    app.harbour.recently_added = vec![a];

    let (subtitle, art_id, ..) =
        header_teaser(&app, HarbourSectionId::RecentlyAdded).expect("teaser present");
    assert_eq!(
        subtitle,
        format!(
            "In Rainbows • Radiohead • Added {}",
            format_relative_time("2020-01-01T00:00:00Z")
        )
    );
    assert!(
        !subtitle.contains("2007"),
        "the year is dropped from the teaser — the pick's title occupies the line (got: {subtitle})"
    );
    assert_eq!(art_id, Some("al9".to_string()));
}

#[test]
fn most_played_tracks_header_teaser_shows_plays_not_recency() {
    let mut app = test_app();
    // make_recent_song sets play_date — the Most Played teaser must ignore it.
    let mut s = make_recent_song("s1", "Weird Fishes", "Radiohead", "al1");
    s.play_count = Some(87);
    app.harbour.most_played_songs = vec![s];

    let (subtitle, ..) =
        header_teaser(&app, HarbourSectionId::MostPlayedTracks).expect("teaser present");
    assert_eq!(subtitle, "Weird Fishes • Radiohead • 87 plays");
}

#[test]
fn most_played_albums_header_teaser_shows_plays() {
    let mut app = test_app();
    let mut a = make_album("al2", "OK Computer", "Radiohead");
    a.play_count = Some(214);
    app.harbour.most_played_albums = vec![a];

    let (subtitle, art_id, ..) =
        header_teaser(&app, HarbourSectionId::MostPlayedAlbums).expect("teaser present");
    assert_eq!(subtitle, "OK Computer • Radiohead • 214 plays");
    assert_eq!(art_id, Some("al2".to_string()));
}

#[test]
fn most_played_artists_header_teaser_keys_art_on_the_artist_id() {
    let mut app = test_app();
    let mut ar = search_artist("ar1", "Radiohead");
    ar.play_count = Some(542);
    app.harbour.most_played_artists = vec![ar];

    let (subtitle, art_id, art_ids, ..) =
        header_teaser(&app, HarbourSectionId::MostPlayedArtists).expect("teaser present");
    assert_eq!(subtitle, "Radiohead • 542 plays");
    assert_eq!(
        art_id,
        Some("ar1".to_string()),
        "artist minis live in album_art keyed by artist id"
    );
    assert!(art_ids.is_empty(), "artists have no quad");
}

#[test]
fn most_played_genres_header_teaser_shows_the_top_track_share() {
    let mut app = test_app();
    // Gate: the genre shelf rides the tracks pool's has-plays gate.
    app.harbour.most_played_songs = vec![played_song("s1", "Art Rock", 42)];
    let mut g = make_genre("Art Rock", "Art Rock");
    g.song_count = 9;
    app.harbour.most_played_genres = vec![g];

    let (subtitle, ..) =
        header_teaser(&app, HarbourSectionId::MostPlayedGenres).expect("teaser present");
    assert_eq!(subtitle, "Art Rock • 9 of your top tracks");
}

#[test]
fn playlists_header_teaser_shows_the_picks_own_counts_and_custom_art_key() {
    let mut app = test_app();
    let mut p = harbour_playlist("p1", "Morning Mix");
    p.song_count = 32;
    p.duration = 8100.0; // 2h 15m
    p.artwork_album_ids = vec!["al1".to_string(), "al2".to_string()];
    app.harbour.playlists = vec![p, harbour_playlist("p2", "Other")];

    let (subtitle, art_id, art_ids, custom) =
        header_teaser(&app, HarbourSectionId::Playlists).expect("teaser present");
    assert_eq!(
        subtitle, "Morning Mix • 32 songs • 2h 15m",
        "bare pick name + the first pick's OWN counts, no 'Featuring' prefix"
    );
    assert_eq!(
        custom,
        Some("p1".to_string()),
        "custom cover key = first pick"
    );
    assert_eq!(art_id, Some("al1".to_string()));
    assert_eq!(art_ids, vec!["al1".to_string(), "al2".to_string()]);
}

#[test]
fn genres_header_teaser_shows_library_counts_and_quad_ids() {
    let mut app = test_app();
    let mut g = make_genre("Shoegaze", "Shoegaze");
    g.album_count = 12;
    g.song_count = 148;
    g.artwork_album_ids = vec!["al1".to_string(), "al2".to_string()];
    app.harbour.genres = vec![g];

    let (subtitle, art_id, art_ids, ..) =
        header_teaser(&app, HarbourSectionId::Genres).expect("teaser present");
    assert_eq!(subtitle, "Shoegaze • 12 albums • 148 songs");
    assert_eq!(art_id, Some("al1".to_string()));
    assert_eq!(art_ids, vec!["al1".to_string(), "al2".to_string()]);
}

#[test]
fn empty_shelf_headers_have_no_teaser() {
    let app = test_app();
    assert!(
        header_teaser(&app, HarbourSectionId::RecentlyPlayed).is_none(),
        "an empty shelf never fakes a pick"
    );
    assert!(header_teaser(&app, HarbourSectionId::Playlists).is_none());
}

#[test]
fn search_artists_header_teaser_is_the_bare_name() {
    use nokkvi_data::types::library_search::LibrarySearchResults;

    let mut app = test_app();
    app.harbour.search_query = "aphex".into();
    app.harbour.search_results = Some(LibrarySearchResults {
        artists: vec![search_artist("ar1", "Aphex Twin")],
        ..Default::default()
    });

    let (subtitle, art_id, ..) =
        header_teaser(&app, HarbourSectionId::SearchArtists).expect("teaser present");
    assert_eq!(
        subtitle, "Aphex Twin",
        "the redundant 'Artist' fact of the item rows is dropped"
    );
    assert_eq!(art_id, Some("ar1".to_string()));
}

#[test]
fn search_albums_header_teaser_joins_name_and_artist() {
    use nokkvi_data::types::library_search::LibrarySearchResults;

    let mut app = test_app();
    app.harbour.search_query = "amnesiac".into();
    app.harbour.search_results = Some(LibrarySearchResults {
        albums: vec![search_album("al4", "Amnesiac", "Radiohead")],
        ..Default::default()
    });

    let (subtitle, art_id, ..) =
        header_teaser(&app, HarbourSectionId::SearchAlbums).expect("teaser present");
    assert_eq!(subtitle, "Amnesiac • Radiohead");
    assert_eq!(art_id, Some("al4".to_string()));
}

#[test]
fn search_songs_header_teaser_joins_title_and_artist() {
    use nokkvi_data::types::library_search::LibrarySearchResults;

    let mut app = test_app();
    app.harbour.search_query = "night".into();
    app.harbour.search_results = Some(LibrarySearchResults {
        songs: vec![make_recent_song("s1", "Nightingale", "Fever Ray", "al7")],
        ..Default::default()
    });

    let (subtitle, art_id, ..) =
        header_teaser(&app, HarbourSectionId::SearchSongs).expect("teaser present");
    assert_eq!(subtitle, "Nightingale • Fever Ray");
    assert_eq!(art_id, Some("al7".to_string()));
}

#[test]
fn search_genres_header_teaser_reads_quad_ids_from_the_side_map() {
    use nokkvi_data::types::{genre::Genre, library_search::LibrarySearchResults};

    let mut app = test_app();
    app.harbour.search_query = "amb".into();
    app.harbour.search_results = Some(LibrarySearchResults {
        genres: vec![Genre {
            id: "g1".into(),
            name: "Ambient".into(),
            album_count: 12,
            song_count: 0,
        }],
        ..Default::default()
    });
    app.harbour
        .search_genre_album_ids
        .insert("Ambient".into(), vec!["al1".into(), "al2".into()]);

    let (subtitle, art_id, art_ids, ..) =
        header_teaser(&app, HarbourSectionId::SearchGenres).expect("teaser present");
    assert_eq!(subtitle, "Ambient • 12 albums");
    assert_eq!(art_id, Some("al1".to_string()));
    assert_eq!(art_ids, vec!["al1".to_string(), "al2".to_string()]);
}

#[test]
fn search_playlists_header_teaser_carries_the_custom_art_key() {
    use nokkvi_data::types::library_search::LibrarySearchResults;

    let mut app = test_app();
    app.harbour.search_query = "late".into();
    app.harbour.search_results = Some(LibrarySearchResults {
        playlists: vec![search_playlist("p1", "Late Nights", 14)],
        ..Default::default()
    });

    let (subtitle, _art_id, _art_ids, custom) =
        header_teaser(&app, HarbourSectionId::SearchPlaylists).expect("teaser present");
    assert_eq!(subtitle, "Late Nights • 14 songs");
    assert_eq!(custom, Some("p1".to_string()));
}

// --- Header subtitle: collapsed teaser ↔ expanded count swap ---

fn probe_teaser() -> crate::views::harbour::SectionTeaser {
    crate::views::harbour::SectionTeaser {
        subtitle: "Karma Police • Radiohead".to_string(),
        art_album_id: None,
        art_album_ids: Vec::new(),
        custom_playlist_id: None,
    }
}

#[test]
fn header_subtitle_swaps_between_teaser_and_pick_count() {
    use crate::views::harbour::section_header_subtitle;

    let teaser = probe_teaser();
    assert_eq!(
        section_header_subtitle(Some(&teaser), false, 4, false),
        "Karma Police • Radiohead",
        "a collapsed header shows the concrete first-pick line"
    );
    assert_eq!(
        section_header_subtitle(Some(&teaser), true, 4, false),
        "4 picks",
        "an expanded header swaps to the pick count"
    );
    assert_eq!(
        section_header_subtitle(Some(&teaser), true, 1, false),
        "1 pick",
        "singular pick"
    );
}

#[test]
fn search_header_subtitle_counts_matches_with_an_honest_cap() {
    use crate::views::harbour::{SEARCH_PREVIEW_LIMIT, section_header_subtitle};

    let teaser = probe_teaser();
    assert_eq!(
        section_header_subtitle(Some(&teaser), true, 1, true),
        "1 match",
        "singular match"
    );
    assert_eq!(
        section_header_subtitle(Some(&teaser), true, SEARCH_PREVIEW_LIMIT - 1, true),
        format!("{} matches", SEARCH_PREVIEW_LIMIT - 1),
        "below the preview cap the fan-out returned its true total"
    );
    assert_eq!(
        section_header_subtitle(Some(&teaser), true, SEARCH_PREVIEW_LIMIT, true),
        format!("{SEARCH_PREVIEW_LIMIT}+ matches"),
        "at the cap the count is only an honest floor"
    );
}

#[test]
fn empty_header_subtitle_reads_nothing_here_in_both_states() {
    use crate::views::harbour::section_header_subtitle;

    assert_eq!(
        section_header_subtitle(None, false, 0, false),
        "Nothing here yet"
    );
    assert_eq!(
        section_header_subtitle(None, true, 0, false),
        "Nothing here yet"
    );
}
