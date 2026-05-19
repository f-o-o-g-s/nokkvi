//! Tests for the shared paged-loader dispatch (`Nokkvi::load_paged`).
//!
//! The three paged library views (Albums, Artists, Songs) all funnel through
//! `Nokkvi::load_paged<T: LoaderTarget>` for their initial-load, page-append,
//! and force-load paths. The handler owns the invariant body — page_size +
//! defensive `needs_fetch` gate + `PaginatedFetch` build + debug log +
//! `set_loading(true)` BEFORE dispatch. These tests pin that body's
//! observable contracts so a future refactor can't regress them.
//!
//! Critical invariant (CLAUDE.md gotcha): `set_loading(true)` MUST run
//! before the fetch dispatch. Without this, rapid scroll triggers duplicate
//! fetches — `PagedBuffer::needs_fetch` only returns `None` when `loading`
//! is true, so a second scroll-driven dispatch would pass the gate and
//! enqueue a second in-flight fetch for the same page.
//!
//! Tests run without `app_service` (cf. `test_app()`); `shell_task` returns
//! `Task::none()` in that case but the pre-dispatch state mutations still
//! run synchronously — which is exactly the invariant being pinned.

use crate::test_helpers::*;

// ============================================================================
// Albums paged loader
// ============================================================================

#[test]
fn load_paged_albums_sets_loading_before_dispatch() {
    // The bug-class pin: `set_loading(true)` must flip the loading flag
    // synchronously, BEFORE returning the Task. If a future refactor
    // accidentally moves the flip inside the spawned async closure, rapid
    // scroll will trigger duplicate fetches because `needs_fetch` will see
    // `loading == false` on the second poll.
    let mut app = test_app();
    seed_albums(&mut app, albums_indexed(50));
    // Force a state where `needs_fetch` would return `Some` if checked —
    // viewport near the loaded edge with more on the server.
    app.library.albums.set_first_page(albums_indexed(500), 1000);
    app.albums_page.common.slot_list.viewport_offset = 400;

    assert!(
        !app.library.albums.is_loading(),
        "precondition: buffer not loading"
    );

    let _task = app.handle_albums_load_page(500);

    assert!(
        app.library.albums.is_loading(),
        "set_loading(true) MUST flip the flag synchronously, before the task \
         is spawned — otherwise rapid scroll triggers duplicate fetches"
    );
}

#[test]
fn load_paged_albums_skips_dispatch_when_needs_fetch_returns_none() {
    // Defensive gate: when `offset > 0` and `force == false`, the loader
    // must early-return without flipping `is_loading()` if the buffer is
    // fully loaded (or the viewport is far from the edge). This is the
    // "Phase 5A" gate that catches duplicate dispatches racing past the
    // upstream `needs_fetch` check at the action site.
    let mut app = test_app();
    // Fully-loaded buffer → `needs_fetch` returns None unconditionally.
    seed_albums(&mut app, albums_indexed(50));
    app.albums_page.common.slot_list.viewport_offset = 0;

    let _task = app.handle_albums_load_page(50);

    assert!(
        !app.library.albums.is_loading(),
        "fully-loaded buffer + offset > 0 must not flip set_loading — the \
         defensive gate caught the redundant dispatch"
    );
}

#[test]
fn load_paged_albums_force_bypasses_needs_fetch_gate() {
    // The find-and-expand chain uses `force_load_albums_page` to walk the
    // entire library while the viewport stays at 0; the scroll-edge gate
    // would otherwise short-circuit every page after the first.
    let mut app = test_app();
    seed_albums(&mut app, albums_indexed(50));
    app.albums_page.common.slot_list.viewport_offset = 0;

    let _task = app.force_load_albums_page(50);

    assert!(
        app.library.albums.is_loading(),
        "force_load_albums_page MUST bypass the needs_fetch gate so the \
         find-and-expand chain can page through the whole library"
    );
}

#[test]
fn load_paged_albums_offset_zero_always_dispatches() {
    // Initial loads (offset == 0) always proceed — sort/search changes need
    // a fresh page even when the buffer is fully loaded. The defensive gate
    // explicitly carves this out (`offset > 0 && ...`).
    let mut app = test_app();
    // Fully-loaded buffer that would otherwise block a page-load.
    seed_albums(&mut app, albums_indexed(50));
    app.albums_page.common.slot_list.viewport_offset = 0;

    // `handle_load_albums` is the offset == 0 path.
    let _task = app.handle_load_albums(false, None);

    assert!(
        app.library.albums.is_loading(),
        "offset == 0 (initial load / sort change / search change) MUST \
         always dispatch even when the buffer is fully loaded"
    );
}

// ============================================================================
// Cross-entity invariant pins
// ============================================================================
//
// The same set_loading-before-dispatch invariant binds the Artists and Songs
// loaders. One pin per entity proves the trait dispatch reaches all three.

#[test]
fn load_paged_artists_sets_loading_before_dispatch() {
    let mut app = test_app();
    app.library.artists.set_first_page(
        (0..500)
            .map(|i| make_artist(&format!("ar{i}"), "n"))
            .collect(),
        1000,
    );
    app.artists_page.common.slot_list.viewport_offset = 400;

    let _task = app.handle_artists_load_page(500);

    assert!(
        app.library.artists.is_loading(),
        "Artists loader must flip set_loading(true) before dispatch — the \
         shared load_paged body owns this invariant for every paged entity"
    );
}

#[test]
fn load_paged_songs_sets_loading_before_dispatch() {
    let mut app = test_app();
    app.library.songs.set_first_page(
        (0..500)
            .map(|i| make_song(&format!("s{i}"), "t", "a"))
            .collect(),
        1000,
    );
    app.songs_page.common.slot_list.viewport_offset = 400;

    let _task = app.handle_songs_load_page(500);

    assert!(
        app.library.songs.is_loading(),
        "Songs loader must flip set_loading(true) before dispatch — the \
         shared load_paged body owns this invariant for every paged entity"
    );
}

// ============================================================================
// prefetch_and_maybe_load_next_page (loader_target.rs)
// ============================================================================
//
// The shared tail used by every paged view's LoadLargeArtwork action: prefetch
// the viewport's mini artwork and chain a page-load if scrolling near the
// loaded edge. Tests assert the load-page chain trigger (the user-observable
// scroll-edge behavior); without `app_service` the prefetch helpers
// short-circuit to empty Vecs, so we pin the needs_fetch chain.

#[test]
fn prefetch_and_maybe_load_next_page_chains_load_page_when_near_edge() {
    use crate::update::AlbumsTarget;

    let mut app = test_app();
    // Buffer reports 500 loaded of 1000 total → needs_fetch will fire when
    // viewport is near the loaded edge.
    app.library.albums.set_first_page(albums_indexed(500), 1000);
    app.albums_page.common.slot_list.viewport_offset = 480;

    // Use a sentinel closure so we can observe whether the load_page chain
    // fired. The `load_page` closure receives `&mut Self` + `offset`; the
    // helper invokes it exactly when needs_fetch returns Some.
    let mut chain_offset: Option<usize> = None;
    let _tasks = app.prefetch_and_maybe_load_next_page::<AlbumsTarget>(|_app, offset| {
        chain_offset = Some(offset);
        iced::Task::none()
    });

    assert!(
        chain_offset.is_some(),
        "needs_fetch should have fired the load_page chain when viewport is near loaded edge"
    );
}

#[test]
fn prefetch_and_maybe_load_next_page_skips_load_page_when_fully_loaded() {
    use crate::update::AlbumsTarget;

    let mut app = test_app();
    // Fully-loaded buffer: 50 of 50 total → needs_fetch always returns None.
    seed_albums(&mut app, albums_indexed(50));
    app.albums_page.common.slot_list.viewport_offset = 0;

    let mut chain_fired = false;
    let _tasks = app.prefetch_and_maybe_load_next_page::<AlbumsTarget>(|_app, _offset| {
        chain_fired = true;
        iced::Task::none()
    });

    assert!(
        !chain_fired,
        "fully-loaded buffer must not chain a page-load — needs_fetch returns None"
    );
}

#[test]
fn prefetch_and_maybe_load_next_page_skips_load_page_when_library_empty() {
    use crate::update::SongsTarget;

    let mut app = test_app();
    // Empty library — needs_fetch can't meaningfully apply; the helper's
    // `is_empty()` guard skips the chain entirely.
    assert_eq!(app.library.songs.len(), 0);

    let mut chain_fired = false;
    let _tasks = app.prefetch_and_maybe_load_next_page::<SongsTarget>(|_app, _offset| {
        chain_fired = true;
        iced::Task::none()
    });

    assert!(
        !chain_fired,
        "empty library must not chain a page-load — the is_empty() guard fires first"
    );
}
