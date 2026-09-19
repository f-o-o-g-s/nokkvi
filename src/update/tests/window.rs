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

// ============================================================================
// Queue: stored slot_count == rendered slot_count
//
// The queue's within-list drag maps a picked/dropped slot to a row through the
// STORED `slot_count` that `resync_slot_counts` writes, while the rows on
// screen come from the count `QueuePage::view` renders. Each test below turns
// on one bar the queue stacks above its list (or one input that changes the
// render's budget) and asserts, across a sweep of window heights, that the
// resync stores exactly the rendered count. Each also asserts its setup
// invariant: some height in the sweep where that term changes the count, so
// the test cannot pass with the term missing from the resync.
// ============================================================================

mod queue_resync_parity {
    use nokkvi_data::types::player_settings::ArtworkColumnMode;

    use crate::{
        Nokkvi,
        test_helpers::test_app,
        theme,
        views::queue::view::{QueueChromeInputs, queue_effective_chrome},
        widgets::slot_list::SlotListConfig,
    };

    /// A description long enough to wrap onto several lines of the
    /// hover-expanded detail block at any pane width.
    const LONG_COMMENT: &str = "Late-night drives, rainy windows, and the kind of slow \
        records that sound better at low volume. Built up over a few winters from \
        whatever kept coming back on repeat, then trimmed until nothing felt out of \
        place.";

    fn lock_and_reset() -> parking_lot::MutexGuard<'static, ()> {
        let guard = crate::theme::THEME_MODE_LOCK.lock();
        reset_atomics();
        guard
    }

    fn reset_atomics() {
        theme::set_artwork_column_mode(ArtworkColumnMode::Auto);
        theme::set_artwork_auto_max_pct(0.40);
        theme::set_autohide_toolbar(false);
    }

    fn with_banner(app: &mut Nokkvi, comment: &str) {
        app.active_playlist_info = Some(crate::state::ActivePlaylistContext::minimal(
            "pl-1".into(),
            "Mix".into(),
            comment.into(),
        ));
    }

    /// The slot count `with_dynamic_slots` budgets for these chrome inputs.
    fn count(inputs: &QueueChromeInputs<'_>) -> usize {
        SlotListConfig::with_dynamic_slots(inputs.window_height, queue_effective_chrome(inputs))
            .slot_count
    }

    /// The slot count the queue renders for the app's current state: the
    /// `with_dynamic_slots` input `QueuePage::view` reads off its view data.
    fn rendered(app: &Nokkvi) -> usize {
        count(&app.build_queue_view_data(false).chrome)
    }

    /// Heights in `heights` where the render's inputs with the term under test
    /// removed (`without_term`) budget a different count than the render does.
    fn term_matters(
        app: &mut Nokkvi,
        heights: impl IntoIterator<Item = u32>,
        without_term: impl Fn(&mut QueueChromeInputs<'_>),
    ) -> Vec<u32> {
        heights
            .into_iter()
            .filter(|&h| {
                app.window.height = h as f32;
                let inputs = app.build_queue_view_data(false).chrome;
                let mut without = inputs;
                without_term(&mut without);
                count(&without) != count(&inputs)
            })
            .collect()
    }

    /// At every height, resync and compare the stored count to the rendered
    /// one. Panics listing the first mismatches as (height, stored, rendered).
    fn assert_parity(app: &mut Nokkvi, heights: impl IntoIterator<Item = u32>) {
        let mismatches: Vec<(u32, usize, usize)> = heights
            .into_iter()
            .filter_map(|h| {
                app.window.height = h as f32;
                let expected = rendered(app);
                app.resync_slot_counts();
                let stored = app.queue_page.common.slot_list.slot_count;
                (stored != expected).then_some((h, stored, expected))
            })
            .collect();
        assert!(
            mismatches.is_empty(),
            "stored queue slot_count != rendered at {} heights; first (height, stored, \
             rendered): {:?}",
            mismatches.len(),
            &mismatches[..mismatches.len().min(6)],
        );
    }

    fn sweep() -> impl Iterator<Item = u32> + Clone {
        (400..=2000).step_by(7)
    }

    /// A: the "Playing From" banner (46 px) and its 1 px separator.
    #[test]
    fn resync_counts_the_playing_from_banner() {
        let _g = lock_and_reset();
        let mut app = test_app();
        app.window.width = 1400.0;
        with_banner(&mut app, "");

        let differs = term_matters(&mut app, sweep(), |i| i.playlist_comment = None);
        assert!(
            !differs.is_empty(),
            "setup invariant: the banner must change the count at some height"
        );
        assert_parity(&mut app, sweep());
        reset_atomics();
    }

    /// B: the banner's hover-expanded detail block, sized to a multi-line
    /// comment.
    #[test]
    fn resync_counts_the_hover_expanded_banner_detail() {
        let _g = lock_and_reset();
        let mut app = test_app();
        app.window.width = 1400.0;
        with_banner(&mut app, LONG_COMMENT);
        app.queue_page.playlist_strip_expanded = true;

        let differs = term_matters(&mut app, sweep(), |i| i.strip_expanded = false);
        assert!(
            !differs.is_empty(),
            "setup invariant: the detail block must change the count at some height"
        );
        assert_parity(&mut app, sweep());
        reset_atomics();
    }

    /// C: the multi-select column's select-all bar, no banner.
    #[test]
    fn resync_counts_the_select_all_bar() {
        let _g = lock_and_reset();
        let mut app = test_app();
        app.window.width = 1400.0;
        app.queue_page.column_visibility.select = true;

        let differs = term_matters(&mut app, sweep(), |i| i.select_visible = false);
        assert!(
            !differs.is_empty(),
            "setup invariant: the select-all bar must change the count at some height"
        );
        assert_parity(&mut app, sweep());
        reset_atomics();
    }

    /// D: the select-all bar under the banner.
    #[test]
    fn resync_counts_the_select_all_bar_under_the_banner() {
        let _g = lock_and_reset();
        let mut app = test_app();
        app.window.width = 1400.0;
        with_banner(&mut app, "");
        app.queue_page.column_visibility.select = true;

        let differs = term_matters(&mut app, sweep(), |i| {
            i.select_visible = false;
            i.playlist_comment = None;
        });
        assert!(
            !differs.is_empty(),
            "setup invariant: the banner + select-all bar must change the count at some height"
        );
        assert_parity(&mut app, sweep());
        reset_atomics();
    }

    /// E: split view. The queue renders in the 55% pane, where Auto artwork
    /// falls back to a portrait column stacked above the list while the full
    /// width (1000 px, too narrow for the horizontal column and too wide for
    /// the portrait fallback) shows none.
    #[test]
    fn resync_sizes_the_queue_at_the_split_view_pane_width() {
        use crate::widgets::base_slot_list_layout::{
            BaseSlotListLayoutConfig, resolve_artwork_layout,
        };

        let _g = lock_and_reset();
        let mut app = test_app();
        app.window.width = 1000.0;
        app.browsing_panel = Some(crate::views::BrowsingPanel::new());
        let full = app.content_pane_width();
        let pane = app.queue_pane_width();
        assert!(
            pane < full,
            "setup invariant: the queue renders in the split pane"
        );

        let orientation = |width: f32, h: u32| {
            resolve_artwork_layout(&BaseSlotListLayoutConfig {
                window_width: width,
                window_height: h as f32,
                show_artwork_column: true,
                slot_list_chrome: 0.0,
                elevated: false,
            })
            .map(|layout| layout.orientation)
        };
        assert!(
            sweep().any(|h| orientation(pane, h) != orientation(full, h)),
            "setup invariant: the pane and the full width must resolve to different \
             artwork orientations at some height"
        );
        let differs = term_matters(&mut app, sweep(), |i| i.pane_width = full);
        assert!(
            !differs.is_empty(),
            "setup invariant: the pane width must change the count at some height"
        );
        assert_parity(&mut app, sweep());
        reset_atomics();
    }

    /// F: the banner's 1 px top hairline under a flush Auto portrait artwork
    /// column (a narrow, tall single-view window).
    #[test]
    fn resync_counts_the_banner_top_hairline() {
        let _g = lock_and_reset();
        let mut app = test_app();
        app.window.width = 530.0;
        with_banner(&mut app, "");
        let heights = || 400..=2000;

        // The hairline is 1 px, so compare the render's chrome with and
        // without that pixel rather than through an input toggle. The
        // collapsed banner alone is 46 px + its 1 px separator; anything more
        // is the hairline.
        let hairline_matters = heights().any(|h| {
            use crate::views::queue::view::{PLAYLIST_STRIP_COMPACT_H, queue_chrome_height};
            app.window.height = h as f32;
            let inputs = app.build_queue_view_data(false).chrome;
            let banner_h = queue_chrome_height(&inputs)
                - queue_chrome_height(&QueueChromeInputs {
                    playlist_comment: None,
                    ..inputs
                });
            let hairline_shows = banner_h > PLAYLIST_STRIP_COMPACT_H + 1.0;
            let chrome = queue_effective_chrome(&inputs);
            let at = |c: f32| SlotListConfig::with_dynamic_slots(h as f32, c).slot_count;
            hairline_shows && at(chrome) != at(chrome - 1.0)
        });
        assert!(
            hairline_matters,
            "setup invariant: the top hairline must show and change the count at some height"
        );
        assert_parity(&mut app, heights());
        reset_atomics();
    }

    /// An open header dropdown (columns cog or server sync) holds the
    /// auto-hide toolbar expanded in the render, so the resync must size the
    /// queue with the expanded header too.
    #[test]
    fn resync_keeps_the_header_expanded_while_a_queue_menu_is_open() {
        let _g = lock_and_reset();
        theme::set_autohide_toolbar(true);
        let mut app = test_app();
        app.window.width = 1400.0;
        app.queue_page.common.set_window_focused(true);
        app.open_menu = Some(crate::app_message::OpenMenu::CheckboxDropdown {
            view: crate::View::Queue,
            trigger_bounds: iced::Rectangle::default(),
        });
        assert!(
            !app.build_queue_view_data(false).chrome.toolbar_collapsed,
            "setup invariant: the open columns menu holds the header expanded"
        );

        let differs = term_matters(&mut app, sweep(), |i| i.toolbar_collapsed = true);
        assert!(
            !differs.is_empty(),
            "setup invariant: collapsing the header must change the count at some height"
        );
        assert_parity(&mut app, sweep());
        reset_atomics();
    }
    // ------------------------------------------------------------------
    // The stored count follows every message. `handle_queue` resyncs before
    // its arm runs, and several inputs change outside it (the browsing
    // panel, the banner's playlist context, window focus), so the root
    // `update` resyncs after each message too.
    // ------------------------------------------------------------------

    /// Dispatch `message` through the root `update`, then assert the stored
    /// queue count equals the rendered one.
    fn assert_parity_after(app: &mut Nokkvi, message: crate::app_message::Message, what: &str) {
        let _ = app.update(message);
        let stored = app.queue_page.common.slot_list.slot_count;
        let expected = rendered(app);
        assert_eq!(
            stored, expected,
            "after {what}: stored queue slot_count {stored} != rendered {expected}"
        );
    }

    #[test]
    fn strip_hover_resyncs_the_queue_count() {
        use crate::{app_message::Message, views::QueueMessage};

        let _g = lock_and_reset();
        let mut app = test_app();
        app.window.width = 1400.0;
        with_banner(&mut app, LONG_COMMENT);
        app.queue_page.playlist_strip_expanded = true;
        let differs = term_matters(&mut app, sweep(), |i| i.strip_expanded = false);
        let height = *differs
            .first()
            .expect("setup invariant: a height where the detail block changes the count");
        app.window.height = height as f32;
        app.queue_page.playlist_strip_expanded = false;
        app.resync_slot_counts();

        assert_parity_after(
            &mut app,
            Message::Queue(QueueMessage::PlaylistStripHoverEnter),
            "the banner expands",
        );
        assert_parity_after(
            &mut app,
            Message::Queue(QueueMessage::PlaylistStripHoverExit),
            "the banner collapses",
        );
        reset_atomics();
    }

    #[test]
    fn select_column_toggle_resyncs_the_queue_count() {
        use crate::{
            app_message::Message,
            views::{QueueMessage, queue::QueueColumn},
        };

        let _g = lock_and_reset();
        let mut app = test_app();
        app.window.width = 1400.0;
        app.queue_page.column_visibility.select = true;
        let differs = term_matters(&mut app, sweep(), |i| i.select_visible = false);
        let height = *differs
            .first()
            .expect("setup invariant: a height where the select-all bar changes the count");
        app.window.height = height as f32;
        app.queue_page.column_visibility.select = false;
        app.resync_slot_counts();

        assert_parity_after(
            &mut app,
            Message::Queue(QueueMessage::ToggleColumnVisible(QueueColumn::Select)),
            "the select column turns on",
        );
        reset_atomics();
    }

    #[test]
    fn browsing_panel_toggle_resyncs_the_queue_count() {
        use crate::app_message::{Message, SplitViewMessage};

        let _g = lock_and_reset();
        let mut app = test_app();
        app.current_view = crate::View::Queue;
        app.window.width = 1000.0;
        // Same shape as the split-view parity test: the pane's portrait
        // artwork changes the count at some height.
        app.browsing_panel = Some(crate::views::BrowsingPanel::new());
        let full = app.content_pane_width();
        let differs = term_matters(&mut app, sweep(), |i| i.pane_width = full);
        let height = *differs
            .first()
            .expect("setup invariant: a height where the pane width changes the count");
        app.window.height = height as f32;
        app.browsing_panel = None;
        app.resync_slot_counts();

        assert_parity_after(
            &mut app,
            Message::SplitView(SplitViewMessage::ToggleBrowsingPanel),
            "Ctrl+E opens the browsing panel",
        );
        assert!(app.browsing_panel.is_some(), "the toggle opened the panel");
        assert_parity_after(
            &mut app,
            Message::SplitView(SplitViewMessage::ToggleBrowsingPanel),
            "Ctrl+E closes the browsing panel",
        );
        reset_atomics();
    }
}
