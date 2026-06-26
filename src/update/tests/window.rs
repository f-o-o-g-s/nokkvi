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

/// Regression: when the auto-hide toolbar is enabled and collapsed, the queue
/// renders the SHORTER collapsed header and packs more slots. `resync_slot_counts`
/// must size the stored count with that collapsed footprint, not the hardcoded
/// expanded one — otherwise consumers that read `slot_count` without revealing
/// the toolbar (find-and-expand row landing; previously drag-reorder) desync
/// from the live render and land a row off.
#[test]
fn resync_slot_counts_uses_collapsed_header_under_active_autohide() {
    use crate::widgets::{
        base_slot_list_layout::{BaseSlotListLayoutConfig, vertical_artwork_chrome},
        slot_list::{SlotListConfig, chrome_height_with_header},
    };

    // Serialize against every other test that reads/writes the process-global
    // UI_MODE atomics or asserts slot-count/chrome math — this test flips
    // `set_autohide_toolbar`, so it must hold the same lock they do.
    let _guard = crate::theme::THEME_MODE_LOCK.lock();

    let mut app = test_app();
    app.window.width = 1400.0; // landscape → no vertical artwork chrome

    // Mirror resync's own slot-count math so the assertion is exact.
    let vertical = |h: f32| {
        vertical_artwork_chrome(&BaseSlotListLayoutConfig {
            window_width: 1400.0,
            window_height: h,
            show_artwork_column: true,
            slot_list_chrome: chrome_height_with_header(false),
            elevated: false,
        })
    };
    let sc = |h: f32, collapsed: bool| {
        SlotListConfig::with_dynamic_slots(h, chrome_height_with_header(collapsed) + vertical(h))
            .slot_count
    };

    // Pick a height where the collapse delta actually changes the count, so a
    // pass means the collapsed path was taken (not a no-op coincidence).
    let height = (300..=1200)
        .map(|h| h as f32)
        .find(|&h| sc(h, true) > sc(h, false))
        .expect("a window height where the collapsed header packs more slots must exist");
    app.window.height = height;
    let expected_collapsed = sc(height, true);
    let expanded = sc(height, false);

    crate::theme::set_autohide_toolbar(true);
    // A freshly-built page isn't hovered / searching, so it's collapsed.
    assert!(app.queue_page.common.toolbar_collapsed(true, false));

    app.resync_slot_counts();
    let got = app.queue_page.common.slot_list.slot_count;

    // Restore the global before asserting so a failure can't leak into siblings.
    crate::theme::set_autohide_toolbar(false);

    assert_eq!(
        got, expected_collapsed,
        "resync must size pages with the collapsed header when auto-hide is active \
         (got {got}, collapsed {expected_collapsed}, expanded {expanded})"
    );
}
