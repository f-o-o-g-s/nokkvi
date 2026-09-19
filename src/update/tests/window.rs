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

    // The fix: radios_page and similar_page are resynced too, each to the
    // count its own view renders (Similar lives in the browsing pane, so its
    // size can differ from Albums').
    use crate::app_view::LibraryPage;
    for page in [LibraryPage::Radios, LibraryPage::Similar] {
        assert_eq!(
            app.library_page_common(page).slot_list.slot_count,
            app.library_page_chrome(page).slot_count(),
            "{page:?}.slot_count was not resynced on window resize"
        );
    }
    assert_ne!(
        app.similar_page.common.slot_list.slot_count, 9,
        "similar_page kept the SlotListView default"
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

    pub(super) fn lock_and_reset() -> parking_lot::MutexGuard<'static, ()> {
        let guard = crate::theme::THEME_MODE_LOCK.lock();
        reset_atomics();
        guard
    }

    pub(super) fn reset_atomics() {
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

    pub(super) fn sweep() -> impl Iterator<Item = u32> + Clone {
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

// ============================================================================
// Library pages: stored slot_count == rendered slot_count
//
// Every pooled page other than the queue renders a view header, an optional
// select-all bar, and (in Auto artwork) a portrait column sized to its pane.
// Each test turns on one term the old resync ignored and asserts, across a
// sweep of window heights, that the resync stores the count the page renders,
// with a setup invariant proving the term changes the count at some height.
// ============================================================================

mod library_resync_parity {
    use super::queue_resync_parity::{lock_and_reset, reset_atomics, sweep};
    use crate::{
        Nokkvi, app_message::OpenMenu, app_view::LibraryPage, test_helpers::test_app, theme,
        widgets::slot_list::SlotListChrome,
    };

    fn stored(app: &Nokkvi, page: LibraryPage) -> usize {
        app.library_page_common(page).slot_list.slot_count
    }

    /// The slot count the page renders: its view data carries this chrome.
    fn rendered(app: &Nokkvi, page: LibraryPage) -> usize {
        app.library_page_chrome(page).slot_count()
    }

    fn set_select(app: &mut Nokkvi, page: LibraryPage) {
        match page {
            LibraryPage::Albums => app.albums_page.column_visibility.select = true,
            LibraryPage::Artists => app.artists_page.column_visibility.select = true,
            LibraryPage::Genres => app.genres_page.column_visibility.select = true,
            LibraryPage::Playlists => app.playlists_page.column_visibility.select = true,
            LibraryPage::Songs => app.songs_page.column_visibility.select = true,
            LibraryPage::Similar => app.similar_page.column_visibility.select = true,
            LibraryPage::Radios | LibraryPage::Harbour => {
                panic!("{page:?} has no select column")
            }
        }
    }

    /// Heights where the page's chrome with the term under test removed
    /// (`without_term`) budgets a different count than the render does.
    fn term_matters(
        app: &mut Nokkvi,
        page: LibraryPage,
        without_term: impl Fn(&mut SlotListChrome),
    ) -> Vec<u32> {
        sweep()
            .filter(|&h| {
                app.window.height = h as f32;
                let chrome = app.library_page_chrome(page);
                let mut without = chrome;
                without_term(&mut without);
                without.slot_count() != chrome.slot_count()
            })
            .collect()
    }

    /// A window height where a page's stored count differs from the rendered
    /// one, as (height, stored, rendered).
    type Mismatch = (u32, usize, usize);

    /// At every height, resync and compare the page's stored count to the
    /// rendered one.
    fn mismatches(app: &mut Nokkvi, page: LibraryPage) -> Vec<Mismatch> {
        sweep()
            .filter_map(|h| {
                app.window.height = h as f32;
                let expected = rendered(app, page);
                app.resync_slot_counts();
                let got = stored(app, page);
                (got != expected).then_some((h, got, expected))
            })
            .collect()
    }

    fn assert_no_mismatches(failures: &[(LibraryPage, Vec<Mismatch>)]) {
        let failing: Vec<_> = failures
            .iter()
            .filter(|(_, m)| !m.is_empty())
            .map(|(page, m)| (page, m.len(), m.first().copied()))
            .collect();
        assert!(
            failing.is_empty(),
            "pages whose stored slot_count != rendered, as (page, mismatched heights, \
             first (height, stored, rendered)): {failing:?}"
        );
    }

    #[test]
    fn resync_counts_each_page_select_all_bar() {
        let _g = lock_and_reset();
        let mut failures = Vec::new();
        for page in [
            LibraryPage::Albums,
            LibraryPage::Artists,
            LibraryPage::Genres,
            LibraryPage::Playlists,
            LibraryPage::Songs,
            LibraryPage::Similar,
        ] {
            let mut app = test_app();
            app.window.width = 1400.0;
            set_select(&mut app, page);
            assert!(
                !term_matters(&mut app, page, |c| c.select_visible = false).is_empty(),
                "setup invariant: {page:?}'s select-all bar must change the count"
            );
            failures.push((page, mismatches(&mut app, page)));
        }
        assert_no_mismatches(&failures);
        reset_atomics();
    }

    /// Similar's and Harbour's views always render the expanded header, so
    /// the resync must not hand them the collapsed count under auto-hide.
    #[test]
    fn resync_keeps_similar_and_harbour_headers_expanded_under_autohide() {
        let _g = lock_and_reset();
        theme::set_autohide_toolbar(true);
        let mut failures = Vec::new();
        for page in [LibraryPage::Similar, LibraryPage::Harbour] {
            let mut app = test_app();
            app.window.width = 1400.0;
            assert!(
                app.library_page_common(page).toolbar_collapsed(true, false),
                "setup invariant: an idle {page:?} page reads as collapsed under auto-hide"
            );
            assert!(
                !app.library_page_chrome(page).toolbar_collapsed,
                "{page:?}'s view always renders the expanded header"
            );
            assert!(
                !term_matters(&mut app, page, |c| c.toolbar_collapsed = true).is_empty(),
                "setup invariant: collapsing {page:?}'s header must change the count"
            );
            failures.push((page, mismatches(&mut app, page)));
        }
        assert_no_mismatches(&failures);
        reset_atomics();
    }

    /// A page shown in the browsing pane renders at the pane's width and at
    /// the window height less the tab bar; the page left in the main view
    /// keeps the full size.
    #[test]
    fn resync_sizes_browsing_pane_pages_at_the_pane() {
        use crate::{
            app_view::BROWSER_PANE_FRACTION,
            views::{BrowsingPanel, BrowsingView},
            widgets::slot_list::TAB_BAR_HEIGHT,
        };

        let _g = lock_and_reset();
        let mut failures = Vec::new();
        for (page, tab) in [
            (LibraryPage::Albums, BrowsingView::Albums),
            (LibraryPage::Artists, BrowsingView::Artists),
            (LibraryPage::Genres, BrowsingView::Genres),
            (LibraryPage::Songs, BrowsingView::Songs),
            (LibraryPage::Similar, BrowsingView::Similar),
        ] {
            let mut app = test_app();
            app.current_view = crate::View::Queue;
            app.window.width = 1000.0;
            app.window.height = 1200.0;
            app.browsing_panel = Some(BrowsingPanel { active_view: tab });
            let full = app.content_pane_width();
            let chrome = app.library_page_chrome(page);
            assert_eq!(
                (chrome.pane_width, chrome.pane_height),
                (full * BROWSER_PANE_FRACTION, 1200.0 - TAB_BAR_HEIGHT),
                "{page:?} renders at the browsing pane's size"
            );
            if page != LibraryPage::Albums {
                assert_eq!(
                    app.library_page_chrome(LibraryPage::Albums).pane_width,
                    full,
                    "Albums, not shown in the pane, keeps the main view's width"
                );
            }
            let pane_matters = term_matters(&mut app, page, |c| {
                c.pane_width = full;
                c.pane_height += TAB_BAR_HEIGHT;
            });
            assert!(
                !pane_matters.is_empty(),
                "setup invariant: the pane size must change {page:?}'s count"
            );
            failures.push((page, mismatches(&mut app, page)));
        }
        assert_no_mismatches(&failures);
        reset_atomics();
    }

    /// An open header menu holds the auto-hide header expanded in the render:
    /// each page's columns cog, and the Playlists create menu.
    #[test]
    fn resync_keeps_the_header_expanded_while_a_page_menu_is_open() {
        let _g = lock_and_reset();
        theme::set_autohide_toolbar(true);
        let columns = |view| OpenMenu::CheckboxDropdown {
            view,
            trigger_bounds: iced::Rectangle::default(),
        };
        let mut failures = Vec::new();
        for (page, menu) in [
            (LibraryPage::Albums, columns(crate::View::Albums)),
            (LibraryPage::Artists, columns(crate::View::Artists)),
            (LibraryPage::Genres, columns(crate::View::Genres)),
            (LibraryPage::Songs, columns(crate::View::Songs)),
            (LibraryPage::Playlists, columns(crate::View::Playlists)),
            (
                LibraryPage::Playlists,
                OpenMenu::PlaylistsCreate {
                    trigger_bounds: iced::Rectangle::default(),
                },
            ),
        ] {
            let mut app = test_app();
            app.window.width = 1400.0;
            app.library_page_common_mut(page).set_window_focused(true);
            app.open_menu = Some(menu);
            assert!(
                !app.library_page_chrome(page).toolbar_collapsed,
                "setup invariant: the open menu holds {page:?}'s header expanded"
            );
            assert!(
                !term_matters(&mut app, page, |c| c.toolbar_collapsed = true).is_empty(),
                "setup invariant: collapsing {page:?}'s header must change the count"
            );
            failures.push((page, mismatches(&mut app, page)));
        }
        assert_no_mismatches(&failures);
        reset_atomics();
    }

    /// Toggling a page's select column goes through the root `update`, which
    /// resyncs after the page flips the flag.
    #[test]
    fn select_column_toggle_resyncs_the_albums_count() {
        use crate::{
            app_message::Message,
            views::{AlbumsMessage, albums::AlbumsColumn},
        };

        let _g = lock_and_reset();
        let mut app = test_app();
        app.window.width = 1400.0;
        set_select(&mut app, LibraryPage::Albums);
        let height = *term_matters(&mut app, LibraryPage::Albums, |c| c.select_visible = false)
            .first()
            .expect("setup invariant: a height where the select-all bar changes the count");
        app.window.height = height as f32;
        app.albums_page.column_visibility.select = false;
        app.resync_slot_counts();

        let _ = app.update(Message::Albums(AlbumsMessage::ToggleColumnVisible(
            AlbumsColumn::Select,
        )));

        assert!(
            app.albums_page.column_visibility.select,
            "the toggle turned it on"
        );
        assert_eq!(
            stored(&app, LibraryPage::Albums),
            rendered(&app, LibraryPage::Albums)
        );
        reset_atomics();
    }

    /// Find-and-expand pins the found album to the TOP rendered slot through
    /// the stored count (`idx + slot_count / 2`). With the Albums select
    /// column on, a stored count that left out the bar pinned the album one
    /// row above the viewport, out of sight.
    #[test]
    fn find_and_expand_lands_the_album_on_the_top_slot_with_the_select_column_on() {
        use crate::{
            test_helpers::{albums_indexed, arm_pending_album, seed_albums},
            widgets::slot_list::SlotListConfig,
        };

        let _g = lock_and_reset();
        let mut app = test_app();
        app.window.width = 1400.0;
        set_select(&mut app, LibraryPage::Albums);
        let height = *term_matters(&mut app, LibraryPage::Albums, |c| c.select_visible = false)
            .last()
            .expect("setup invariant: a height where the select-all bar changes the count");
        app.window.height = height as f32;
        app.resync_slot_counts();

        arm_pending_album(&mut app, "a320");
        seed_albums(&mut app, albums_indexed(1343));
        assert!(app.try_resolve_pending_expand_album().is_some());

        let chrome = app.library_page_chrome(LibraryPage::Albums);
        let cfg = SlotListConfig::with_dynamic_slots(chrome.pane_height, chrome.effective());
        let top = app.albums_page.common.slot_list.slot_to_item(
            0,
            1343,
            cfg.slot_count,
            cfg.center_slot,
            false,
        );
        assert_eq!(
            top,
            Some(320),
            "the found album must render on the top slot ({} rendered slots, window {height} px)",
            cfg.slot_count
        );
        reset_atomics();
    }

    /// `resync_slot_counts` walks `LibraryPage::ALL` plus the queue; together
    /// they must name exactly the pages `all_slot_list_commons_mut` pools, so
    /// a page added to one list and not the other fails here.
    #[test]
    fn library_pages_and_the_queue_cover_every_pooled_page() {
        use std::collections::HashSet;

        let mut app = test_app();
        let mut sized: HashSet<*const crate::widgets::SlotListPageState> = LibraryPage::ALL
            .iter()
            .map(|&page| std::ptr::from_ref(app.library_page_common(page)))
            .collect();
        sized.insert(std::ptr::from_ref(&app.queue_page.common));
        let pooled: HashSet<*const crate::widgets::SlotListPageState> = app
            .all_slot_list_commons_mut()
            .into_iter()
            .map(|common| std::ptr::from_ref(&*common))
            .collect();
        assert_eq!(
            sized.len(),
            LibraryPage::ALL.len() + 1,
            "no page listed twice"
        );
        assert_eq!(sized, pooled);
    }
}
