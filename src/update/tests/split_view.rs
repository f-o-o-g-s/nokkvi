//! Tests for pane-aware hotkey routing in the split-view browsing panel.
//!
//! Covers the bugs where the slot-list nav handlers, Get Info, and roulette
//! resolved their target off the raw `self.current_view` (the host view)
//! instead of the focused browser tab. With the browsing panel open and
//! `pane_focus == Browser`, the focused list is the panel's active tab.

use crate::{
    View,
    state::PaneFocus,
    test_helpers::*,
    views::{BrowsingPanel, BrowsingView},
};

/// Open the browsing panel on the given tab with browser focus.
fn open_browser_pane(app: &mut crate::Nokkvi, tab: BrowsingView) {
    app.browsing_panel = Some(BrowsingPanel { active_view: tab });
    app.pane_focus = PaneFocus::Browser;
}

// ============================================================================
// current_target_view() — the shared pane-aware resolver (I10/I11/I12)
// ============================================================================

#[test]
fn current_target_view_returns_browser_tab_under_browser_focus() {
    let mut app = test_app();
    // current_view pinned to the host (playlist edit mode), browser focused on Albums.
    app.current_view = View::PlaylistEditor;
    open_browser_pane(&mut app, BrowsingView::Albums);

    assert_eq!(app.current_target_view(), Some(View::Albums));
}

#[test]
fn current_target_view_returns_current_view_under_queue_focus() {
    let mut app = test_app();
    app.current_view = View::Queue;
    app.browsing_panel = Some(BrowsingPanel {
        active_view: BrowsingView::Albums,
    });
    // Queue focus → the keyboard steers the host (Queue), not the browser tab.
    app.pane_focus = PaneFocus::Queue;

    assert_eq!(app.current_target_view(), Some(View::Queue));
}

#[test]
fn current_target_view_returns_none_for_similar_tab() {
    let mut app = test_app();
    app.current_view = View::PlaylistEditor;
    open_browser_pane(&mut app, BrowsingView::Similar);

    // Similar has no `View` variant — callers needing a concrete View treat
    // None as "Similar focused".
    assert_eq!(app.current_target_view(), None);
}

#[test]
fn current_target_view_no_panel_is_current_view() {
    let mut app = test_app();
    app.current_view = View::Songs;
    assert!(app.browsing_panel.is_none());

    // Regression guard: single-pane behavior is byte-identical.
    assert_eq!(app.current_target_view(), Some(View::Songs));
}

// ============================================================================
// I10 — slot-list nav routes to the focused browser tab, not the host pane
// ============================================================================

#[test]
fn navigate_down_routes_to_focused_browser_tab_not_queue() {
    // With the browser pane focused on Albums, SlotListDown's search-unfocus
    // side effect must land on the focused Albums tab — not the Queue host.
    let mut app = test_app();
    app.current_view = View::Queue;
    open_browser_pane(&mut app, BrowsingView::Albums);

    app.albums_page.common.search_input_focused = true;
    app.queue_page.common.search_input_focused = true;

    let _ = app.handle_slot_list_navigate_down();

    assert!(
        !app.albums_page.common.search_input_focused,
        "focused Albums tab should get unfocused — proves pane-aware nav target"
    );
    assert!(
        app.queue_page.common.search_input_focused,
        "the unfocused (Queue) host pane must be untouched"
    );
}

#[test]
fn navigate_down_routes_to_queue_when_queue_focused() {
    // Inverse control: with Queue focused, the Queue page unfocuses and the
    // (unfocused) Albums tab is left alone.
    let mut app = test_app();
    app.current_view = View::Queue;
    app.browsing_panel = Some(BrowsingPanel {
        active_view: BrowsingView::Albums,
    });
    app.pane_focus = PaneFocus::Queue;

    app.albums_page.common.search_input_focused = true;
    app.queue_page.common.search_input_focused = true;

    let _ = app.handle_slot_list_navigate_down();

    assert!(
        !app.queue_page.common.search_input_focused,
        "focused Queue host should get unfocused"
    );
    assert!(
        app.albums_page.common.search_input_focused,
        "the unfocused Albums browser tab must be untouched"
    );
}
