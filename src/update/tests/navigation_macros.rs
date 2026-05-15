//! Macro infrastructure for the album/artist/genre find-chain test mirror.
//!
//! Background: navigation.rs used to host three near-identical mirrors of the
//! find-and-expand state machine (`pending_expand` → `try_resolve` → top-pin →
//! re-pin), one per expandable entity. ~1500 LOC of mostly-identical bodies
//! drifted independently and AI editors had to keep three copies in sync.
//!
//! Pattern: `for_each_expandable_entity!($scenarios)` invokes the scenarios
//! macro three times with entity tokens. The scenarios macro dispatches by
//! `kernel_set` to either `find_chain_scenarios_full` (album/artist — paginated)
//! or `find_chain_scenarios_single_shot` (genre — no pagination, no stale-id
//! timeout test, top-pin assertion folded into _finds_loaded_).
//!
//! Each kernel macro emits a `mod $name { use super::*; #[test] fn ... }` block
//! so test names stay searchable: `cargo test album::navigate_and_expand_…`
//! and per-scenario failure isolation is preserved (every kernel becomes a
//! real `#[test]` after expansion).
//!
//! Genre quirks kept bespoke in `navigation.rs`:
//!  - `try_resolve_pending_expand_genre_matches_by_name_not_internal_id`
//!  - `try_resolve_pending_expand_genre_clears_when_idle_and_missing`
//!
//! See `~/nokkvi-audit-results/dry-tests.md` §4 for the original mirror table.

// ============================================================================
// Entity binding
// ============================================================================
//
// The single place where Album / Artist / Genre tokens live. Each invocation
// of `for_each_expandable_entity!($mac)` calls `$mac` once per entity with its
// full token bag. `$mac` is the dispatcher (`find_chain_scenarios!`), which in
// turn picks the right kernel set.

macro_rules! for_each_expandable_entity {
    ($mac:ident) => {
        $mac!(
            album,
            kernel_set:              full,
            indexed_factory:         albums_indexed,
            seed:                    seed_albums,
            arm_pending:             arm_pending_album,
            page_field:              albums_page,
            library_field:           albums,
            pending_var:             crate::state::PendingExpand::Album,
            pending_field:           album_id,
            pending_factory:         pending_album,
            pin_var:                 crate::state::PendingTopPin::Album,
            view_const:              crate::View::Albums,
            children_loaded_msg:     crate::views::AlbumsMessage::TracksLoaded,
            children_loaded_arg:     vec![make_song("s1", "Song", "Artist")],
            expansion_child:         vec![make_song("s1", "Song", "Artist")],
            handle_view_fn:          handle_albums,
            try_resolve_fn:          try_resolve_pending_expand_album,
            handle_navigate_fn:      handle_navigate_and_expand_album,
            handle_browser_fn:       handle_browser_pane_navigate_and_expand_album,
            target_in_3:             "a1",
            target_idx_in_3:         1,
            expected_pin_in_3:       "a1",
            target_long_id:          "a320",
            target_long_idx:         320,
            long_total:              1343,
        );
        $mac!(
            artist,
            kernel_set:              full,
            indexed_factory:         artists_indexed,
            seed:                    seed_artists,
            arm_pending:             arm_pending_artist,
            page_field:              artists_page,
            library_field:           artists,
            pending_var:             crate::state::PendingExpand::Artist,
            pending_field:           artist_id,
            pending_factory:         pending_artist,
            pin_var:                 crate::state::PendingTopPin::Artist,
            view_const:              crate::View::Artists,
            children_loaded_msg:     crate::views::ArtistsMessage::AlbumsLoaded,
            children_loaded_arg:     vec![make_album("a1", "Album One", "Artist Two")],
            expansion_child:         vec![make_album("a1", "Album", "Artist")],
            handle_view_fn:          handle_artists,
            try_resolve_fn:          try_resolve_pending_expand_artist,
            handle_navigate_fn:      handle_navigate_and_expand_artist,
            handle_browser_fn:       handle_browser_pane_navigate_and_expand_artist,
            target_in_3:             "ar1",
            target_idx_in_3:         1,
            expected_pin_in_3:       "ar1",
            target_long_id:          "ar320",
            target_long_idx:         320,
            long_total:              1000,
        );
        $mac!(
            genre,
            kernel_set:              single_shot,
            indexed_factory:         genres_indexed,
            seed:                    seed_genres,
            arm_pending:             arm_pending_genre,
            page_field:              genres_page,
            library_field:           genres,
            pending_var:             crate::state::PendingExpand::Genre,
            pending_field:           genre_id,
            pending_factory:         pending_genre,
            pin_var:                 crate::state::PendingTopPin::Genre,
            view_const:              crate::View::Genres,
            children_loaded_msg:     crate::views::GenresMessage::AlbumsLoaded,
            children_loaded_arg:     vec![make_album("a1", "Album One", "Artist")],
            expansion_child:         vec![make_album("a1", "Album", "Artist")],
            handle_view_fn:          handle_genres,
            try_resolve_fn:          try_resolve_pending_expand_genre,
            handle_navigate_fn:      handle_navigate_and_expand_genre,
            handle_browser_fn:       handle_browser_pane_navigate_and_expand_genre,
            target_in_3:             "Genre 1",
            target_idx_in_3:         1,
            expected_pin_in_3:       "uuid-1",
            target_long_id:          "Genre 50",
            target_long_idx:         50,
            long_total:              200,
        );
    };
}

// ============================================================================
// Dispatcher
// ============================================================================

macro_rules! find_chain_scenarios {
    ($name:ident, kernel_set: full, $($rest:tt)*) => {
        find_chain_scenarios_full!($name, $($rest)*);
    };
    ($name:ident, kernel_set: single_shot, $($rest:tt)*) => {
        find_chain_scenarios_single_shot!($name, $($rest)*);
    };
}

// ============================================================================
// Full kernel set — album + artist (paginated, has stale-id timeout test)
// ============================================================================

macro_rules! find_chain_scenarios_full {
    (
        $name:ident,
        indexed_factory:         $indexed:ident,
        seed:                    $seed:ident,
        arm_pending:             $arm_pending:ident,
        page_field:              $page:ident,
        library_field:           $lib:ident,
        pending_var:             $pending_var:path,
        pending_field:           $pfield:ident,
        pending_factory:         $pending_factory:ident,
        pin_var:                 $pin_var:path,
        view_const:              $view:path,
        children_loaded_msg:     $children_msg:path,
        children_loaded_arg:     $children_arg:expr,
        expansion_child:         $expansion_child:expr,
        handle_view_fn:          $handle_view:ident,
        try_resolve_fn:          $resolve:ident,
        handle_navigate_fn:      $navigate:ident,
        handle_browser_fn:       $browser:ident,
        target_in_3:             $target_in_3:expr,
        target_idx_in_3:         $idx_in_3:expr,
        expected_pin_in_3:       $expected_pin_in_3:expr,
        target_long_id:          $target_long_id:expr,
        target_long_idx:         $target_long_idx:expr,
        long_total:              $long_total:expr,
    ) => {
        mod $name {
            use super::*;

            #[test]
            fn navigate_and_expand_clears_search_filter_and_sets_target() {
                let mut app = test_app();
                app.current_view = View::Songs;
                app.$page.common.active_filter =
                    Some(nokkvi_data::types::filter::LibraryFilter::AlbumId {
                        id: "old".to_string(),
                        title: "Old".to_string(),
                    });
                app.$page.common.search_query = "old".to_string();
                app.$page.common.search_input_focused = true;

                let _ = app.$navigate("a1".to_string());

                assert_eq!(app.current_view, $view);
                assert!(app.$page.common.active_filter.is_none());
                assert!(app.$page.common.search_query.is_empty());
                assert!(!app.$page.common.search_input_focused);
                match &app.pending_expand {
                    Some($pending_var {
                        $pfield,
                        for_browsing_pane,
                    }) => {
                        assert_eq!($pfield, "a1");
                        assert!(!*for_browsing_pane);
                    }
                    other => panic!("expected top-pane pending target a1, got {other:?}"),
                }
            }

            #[test]
            fn navigate_and_expand_collapses_existing_expansion() {
                let mut app = test_app();
                app.current_view = View::Songs;
                app.$page.expansion.expanded_id = Some("other".to_string());
                app.$page.expansion.children = $expansion_child;

                let _ = app.$navigate("a1".to_string());

                assert!(app.$page.expansion.expanded_id.is_none());
                assert!(app.$page.expansion.children.is_empty());
            }

            #[test]
            fn browser_pane_navigate_and_expand_sets_browsing_flag() {
                let mut app = test_app();

                let _ = app.$browser("a1".to_string());

                match &app.pending_expand {
                    Some($pending_var {
                        for_browsing_pane, ..
                    }) => {
                        assert!(*for_browsing_pane, "for_browsing_pane should be true");
                    }
                    other => panic!("expected browser-pane pending target, got {other:?}"),
                }
            }

            #[test]
            fn pending_target_cleared_on_switch_view_away() {
                let mut app = test_app();
                $arm_pending(&mut app, "a1");

                let _ = app.handle_switch_view(View::Songs);

                assert!(app.pending_expand.is_none());
            }

            #[test]
            fn pending_target_persists_on_switch_view_to_self() {
                let mut app = test_app();
                $arm_pending(&mut app, "a1");

                let _ = app.handle_switch_view($view);

                assert!(
                    app.pending_expand.is_some(),
                    "switching to the entity's own view should not cancel the in-flight find chain",
                );
            }

            #[test]
            fn pending_target_cleared_on_navigate_and_filter() {
                let mut app = test_app();
                $arm_pending(&mut app, "a1");

                let _ = app.handle_navigate_and_filter(
                    View::Songs,
                    nokkvi_data::types::filter::LibraryFilter::ArtistId {
                        id: "ar1".to_string(),
                        name: "Artist".to_string(),
                    },
                );

                assert!(app.pending_expand.is_none());
            }

            #[test]
            fn try_resolve_finds_loaded_and_takes_target() {
                let mut app = test_app();
                $arm_pending(&mut app, $target_in_3);
                $seed(&mut app, $indexed(3));

                let task = app.$resolve();

                assert!(task.is_some(), "found target should produce a task");
                assert!(
                    app.pending_expand.is_none(),
                    "target should be taken once dispatched",
                );
                // For a 3-item library with default slot_count=9 (center_slot=4),
                // viewport_offset = idx + 4 clamps to total - 1 = 2.
                assert_eq!(app.$page.common.slot_list.viewport_offset, 2);
                assert_eq!(app.$page.common.slot_list.selected_offset, Some($idx_in_3),);
            }

            #[test]
            fn try_resolve_places_target_at_top_slot() {
                // Long-library case: target should land at slot 0, not the center.
                // viewport_offset is the index of the item shown at the center
                // slot (slot_count/2), so to put the target at slot 0 we set
                // viewport_offset = target_idx + center_slot.
                let mut app = test_app();
                $arm_pending(&mut app, $target_long_id);
                $seed(&mut app, $indexed($long_total));

                let task = app.$resolve();
                assert!(task.is_some(), "found target should dispatch a task");

                let center_slot = app.$page.common.slot_list.slot_count / 2;
                assert_eq!(
                    app.$page.common.slot_list.viewport_offset,
                    $target_long_idx + center_slot,
                    "target must land at slot 0 (top), not at the center slot",
                );
                assert_eq!(
                    app.$page.common.slot_list.selected_offset,
                    Some($target_long_idx),
                );
            }

            #[test]
            fn try_resolve_clears_when_fully_loaded_and_missing() {
                let mut app = test_app();
                $arm_pending(&mut app, "missing");
                // set_from_vec sets total_count = items.len(), so fully_loaded() is true.
                $seed(&mut app, $indexed(1));

                let task = app.$resolve();

                assert!(task.is_some(), "fully-loaded miss should produce a task");
                assert!(
                    app.pending_expand.is_none(),
                    "target should be cleared when known-not-in-library",
                );
            }

            #[test]
            fn try_resolve_returns_none_when_loading() {
                let mut app = test_app();
                $arm_pending(&mut app, $target_in_3);
                app.library.$lib.set_first_page($indexed(1), 100);
                app.library.$lib.set_loading(true);

                let task = app.$resolve();

                assert!(task.is_none(), "should wait while a page is in flight");
                assert!(
                    app.pending_expand.is_some(),
                    "target preserved while loading",
                );
            }

            #[test]
            fn try_resolve_kicks_next_page_when_idle_and_more_remain() {
                let mut app = test_app();
                $arm_pending(&mut app, "missing-target");
                // 1 loaded of 100 known total, idle → should request next page.
                app.library.$lib.set_first_page($indexed(1), 100);

                let task = app.$resolve();

                assert!(task.is_some(), "should dispatch next-page load");
                assert!(
                    app.pending_expand.is_some(),
                    "target preserved while still hunting",
                );
            }

            #[test]
            fn try_resolve_bypasses_scroll_edge_gate_when_paging() {
                // Bug regression: `handle_X_load_page` has a defensive gate
                // that bails when the user isn't near the loaded edge. The
                // find chain leaves viewport_offset at 0 while paging through
                // the full library, so without bypass the chain stalls after
                // the first page lands. `set_loading(true)` is the proxy:
                // load_internal calls it BEFORE shell_task but only AFTER the
                // gate. If the gate bails, is_loading() stays false.
                let mut app = test_app();
                $arm_pending(&mut app, "missing");
                // 200 loaded of 1000 total. With page_size=500 the threshold
                // is 100, so viewport=0 + 100 < loaded=200 — the unfortified
                // gate would bail.
                app.library.$lib.set_first_page($indexed(200), 1000);
                assert!(!app.library.$lib.is_loading(), "precondition: idle");

                let task = app.$resolve();
                assert!(task.is_some(), "should produce a load task");
                assert!(
                    app.library.$lib.is_loading(),
                    "next-page fetch must actually start — the scroll-edge \
                     gate must be bypassed during find-and-expand",
                );
            }

            #[test]
            fn pending_timeout_does_not_toast_when_target_already_resolved() {
                let mut app = test_app();
                assert!(app.pending_expand.is_none());

                let _ = app.handle_pending_expand_timeout($pending_factory("a1"));

                assert!(
                    app.toast.toasts.is_empty(),
                    "no toast when target already gone",
                );
            }

            #[test]
            fn pending_timeout_does_not_toast_for_stale_id() {
                let mut app = test_app();
                $arm_pending(&mut app, "newer");

                let _ = app.handle_pending_expand_timeout($pending_factory("older"));

                assert!(
                    app.toast.toasts.is_empty(),
                    "stale timeout (different id) should not toast",
                );
            }

            #[test]
            fn pending_timeout_toasts_when_target_still_in_flight() {
                let mut app = test_app();
                $arm_pending(&mut app, "a1");

                let _ = app.handle_pending_expand_timeout($pending_factory("a1"));

                assert_eq!(app.toast.toasts.len(), 1);
            }

            #[test]
            fn try_resolve_sets_top_pin_when_target_found() {
                let mut app = test_app();
                $arm_pending(&mut app, $target_in_3);
                $seed(&mut app, $indexed(3));

                let _ = app.$resolve();

                match app.pending_top_pin.as_ref() {
                    Some($pin_var(id)) => assert_eq!(id, $expected_pin_in_3),
                    other => panic!("expected pending_top_pin set to target, got {other:?}"),
                }
            }

            #[test]
            fn children_loaded_re_pins_selected_offset() {
                let mut app = test_app();
                $seed(&mut app, $indexed(3));
                app.$page
                    .common
                    .slot_list
                    .set_selected($idx_in_3, app.library.$lib.len());
                app.pending_top_pin = Some($pin_var($expected_pin_in_3.to_string()));

                let _ =
                    app.$handle_view($children_msg($expected_pin_in_3.to_string(), $children_arg));

                assert_eq!(
                    app.$page.common.slot_list.selected_offset,
                    Some($idx_in_3),
                    "highlight must follow the target after expansion completes",
                );
                assert!(
                    app.pending_top_pin.is_none(),
                    "pin should be consumed once applied",
                );
            }
        }
    };
}

// ============================================================================
// Single-shot kernel set — genre (not paginated; folds top-pin into _finds_loaded_)
// ============================================================================
//
// Differences from `_full`:
//  - No `try_resolve_kicks_next_page_when_idle_and_more_remain` (genre is single-shot).
//  - No `try_resolve_bypasses_scroll_edge_gate_when_paging` (no pagination).
//  - No `pending_timeout_does_not_toast_for_stale_id` (only one stale variant
//    in the original genre block; the prose `clears_when_idle_and_missing`
//    test stays bespoke for the single-shot quirk).
//  - No standalone `try_resolve_sets_top_pin_when_target_found` — the
//    `try_resolve_finds_loaded_and_takes_target` body asserts the pin inline.
//  - `try_resolve_returns_none_when_loading` doesn't pre-seed a page (a single
//    in-flight load is enough to make the resolver wait).

macro_rules! find_chain_scenarios_single_shot {
    (
        $name:ident,
        indexed_factory:         $indexed:ident,
        seed:                    $seed:ident,
        arm_pending:             $arm_pending:ident,
        page_field:              $page:ident,
        library_field:           $lib:ident,
        pending_var:             $pending_var:path,
        pending_field:           $pfield:ident,
        pending_factory:         $pending_factory:ident,
        pin_var:                 $pin_var:path,
        view_const:              $view:path,
        children_loaded_msg:     $children_msg:path,
        children_loaded_arg:     $children_arg:expr,
        expansion_child:         $expansion_child:expr,
        handle_view_fn:          $handle_view:ident,
        try_resolve_fn:          $resolve:ident,
        handle_navigate_fn:      $navigate:ident,
        handle_browser_fn:       $browser:ident,
        target_in_3:             $target_in_3:expr,
        target_idx_in_3:         $idx_in_3:expr,
        expected_pin_in_3:       $expected_pin_in_3:expr,
        target_long_id:          $target_long_id:expr,
        target_long_idx:         $target_long_idx:expr,
        long_total:              $long_total:expr,
    ) => {
        mod $name {
            use super::*;

            #[test]
            fn navigate_and_expand_clears_search_filter_and_sets_target() {
                let mut app = test_app();
                app.current_view = View::Songs;
                app.$page.common.active_filter =
                    Some(nokkvi_data::types::filter::LibraryFilter::AlbumId {
                        id: "old".to_string(),
                        title: "Old".to_string(),
                    });
                app.$page.common.search_query = "old".to_string();
                app.$page.common.search_input_focused = true;

                let _ = app.$navigate("Rock".to_string());

                assert_eq!(app.current_view, $view);
                assert!(app.$page.common.active_filter.is_none());
                assert!(app.$page.common.search_query.is_empty());
                assert!(!app.$page.common.search_input_focused);
                match &app.pending_expand {
                    Some($pending_var {
                        $pfield,
                        for_browsing_pane,
                    }) => {
                        assert_eq!($pfield, "Rock");
                        assert!(!*for_browsing_pane);
                    }
                    other => panic!("expected top-pane pending target Rock, got {other:?}"),
                }
            }

            #[test]
            fn navigate_and_expand_collapses_existing_expansion() {
                let mut app = test_app();
                app.$page.expansion.expanded_id = Some("other".to_string());
                app.$page.expansion.children = $expansion_child;

                let _ = app.$navigate("Rock".to_string());

                assert!(app.$page.expansion.expanded_id.is_none());
                assert!(app.$page.expansion.children.is_empty());
            }

            #[test]
            fn browser_pane_navigate_and_expand_sets_browsing_flag() {
                let mut app = test_app();

                let _ = app.$browser("Rock".to_string());

                match &app.pending_expand {
                    Some($pending_var {
                        for_browsing_pane, ..
                    }) => {
                        assert!(*for_browsing_pane, "for_browsing_pane should be true");
                    }
                    other => panic!("expected browser-pane pending target, got {other:?}"),
                }
            }

            #[test]
            fn pending_target_cleared_on_switch_view_away() {
                let mut app = test_app();
                $arm_pending(&mut app, "Rock");

                let _ = app.handle_switch_view(View::Songs);

                assert!(app.pending_expand.is_none());
            }

            #[test]
            fn pending_target_persists_on_switch_view_to_self() {
                let mut app = test_app();
                $arm_pending(&mut app, "Rock");

                let _ = app.handle_switch_view($view);

                assert!(
                    app.pending_expand.is_some(),
                    "switching to the entity's own view should not cancel the in-flight find chain",
                );
            }

            #[test]
            fn pending_target_cleared_on_navigate_and_filter() {
                let mut app = test_app();
                $arm_pending(&mut app, "Rock");

                let _ = app.handle_navigate_and_filter(
                    View::Songs,
                    nokkvi_data::types::filter::LibraryFilter::ArtistId {
                        id: "ar1".to_string(),
                        name: "Artist".to_string(),
                    },
                );

                assert!(app.pending_expand.is_none());
            }

            #[test]
            fn try_resolve_finds_loaded_and_takes_target() {
                let mut app = test_app();
                $arm_pending(&mut app, $target_in_3);
                $seed(&mut app, $indexed(3));

                let task = app.$resolve();

                assert!(task.is_some(), "found target should produce a task");
                assert!(
                    app.pending_expand.is_none(),
                    "target should be taken once dispatched",
                );
                assert_eq!(app.$page.common.slot_list.viewport_offset, 2);
                assert_eq!(app.$page.common.slot_list.selected_offset, Some($idx_in_3),);
                // Top-pin assertion folded in (genre's _sets_top_pin_ test was
                // never separated from _finds_loaded_).
                match app.pending_top_pin.as_ref() {
                    Some($pin_var(id)) => assert_eq!(id, $expected_pin_in_3),
                    other => panic!("expected pending_top_pin set to target, got {other:?}"),
                }
            }

            #[test]
            fn try_resolve_places_target_at_top_slot() {
                let mut app = test_app();
                $arm_pending(&mut app, $target_long_id);
                $seed(&mut app, $indexed($long_total));

                let task = app.$resolve();
                assert!(task.is_some());

                let center_slot = app.$page.common.slot_list.slot_count / 2;
                assert_eq!(
                    app.$page.common.slot_list.viewport_offset,
                    $target_long_idx + center_slot,
                    "target must land at slot 0 (top), not the center",
                );
                assert_eq!(
                    app.$page.common.slot_list.selected_offset,
                    Some($target_long_idx),
                );
            }

            #[test]
            fn try_resolve_returns_none_when_loading() {
                let mut app = test_app();
                $arm_pending(&mut app, "Rock");
                app.library.$lib.set_loading(true);

                let task = app.$resolve();

                assert!(task.is_none(), "should wait while load is in flight");
                assert!(app.pending_expand.is_some());
            }

            #[test]
            fn pending_timeout_does_not_toast_when_target_already_resolved() {
                let mut app = test_app();

                let _ = app.handle_pending_expand_timeout($pending_factory("Rock"));

                assert!(app.toast.toasts.is_empty());
            }

            #[test]
            fn pending_timeout_toasts_when_target_still_in_flight() {
                let mut app = test_app();
                $arm_pending(&mut app, "Rock");

                let _ = app.handle_pending_expand_timeout($pending_factory("Rock"));

                assert_eq!(app.toast.toasts.len(), 1);
            }

            #[test]
            fn children_loaded_re_pins_selected_offset() {
                let mut app = test_app();
                $seed(&mut app, $indexed(3));
                app.$page
                    .common
                    .slot_list
                    .set_selected($idx_in_3, app.library.$lib.len());
                app.pending_top_pin = Some($pin_var($expected_pin_in_3.to_string()));

                let _ =
                    app.$handle_view($children_msg($expected_pin_in_3.to_string(), $children_arg));

                assert_eq!(app.$page.common.slot_list.selected_offset, Some($idx_in_3),);
                assert!(app.pending_top_pin.is_none());
            }
        }
    };
}
