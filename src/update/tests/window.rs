//! Tests for window resize / slot-count resync handlers.

use crate::test_helpers::*;

// ============================================================================
// resync_slot_counts (update/window.rs)
// ============================================================================

/// Regression test for NF4: window resize must propagate the recomputed
/// `slot_count` to every page that owns a `common.slot_list`, not just the
/// six "primary" library views. Before the fix, `radios_page` and
/// `similar_page` kept their default `slot_count` of 9, so the artwork
/// prefetch indices under-fetched and large windows showed only ~9 rows of
/// content for those views.
#[test]
fn resync_slot_counts_covers_radios_and_similar_pages() {
    let mut app = test_app();

    // Sanity: every page starts at the SlotListView default of 9.
    assert_eq!(app.albums_page.common.slot_list.slot_count, 9);
    assert_eq!(app.radios_page.common.slot_list.slot_count, 9);
    assert_eq!(app.similar_page.common.slot_list.slot_count, 9);

    // Tall window — picks a slot_count != 9 so the assertion would have
    // caught the original bug (default == bug-state value).
    app.window.width = 1600.0;
    app.window.height = 1600.0;

    app.resync_slot_counts();

    // Albums is the canonical "did the resync run" witness: it's been in the
    // list since the function was introduced.
    let sc = app.albums_page.common.slot_list.slot_count;
    assert_ne!(
        sc, 9,
        "test setup invariant: a 1600px-tall window must resolve to a slot_count != 9, \
         otherwise this test would pass even with the bug present"
    );

    // The fix: radios_page and similar_page must converge to the same sc.
    assert_eq!(
        app.radios_page.common.slot_list.slot_count, sc,
        "radios_page.slot_count was not resynced on window resize"
    );
    assert_eq!(
        app.similar_page.common.slot_list.slot_count, sc,
        "similar_page.slot_count was not resynced on window resize"
    );
}
