//! Tests for pane-aware hotkey routing in the split-view browsing panel.
//!
//! Covers the bugs where the slot-list nav handlers, Get Info, and roulette
//! resolved their target off the raw `self.current_view` (the host view)
//! instead of the focused browser tab. With the browsing panel open and
//! `pane_focus == Browser`, the focused list is the panel's active tab.

use nokkvi_data::types::song::Song;

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

/// Build a minimal Song for the Similar-pane tests (no `Default` impl).
fn similar_song(id: &str, title: &str) -> Song {
    serde_json::from_value(serde_json::json!({ "id": id, "title": title }))
        .expect("minimal Song JSON should deserialize")
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

// ============================================================================
// I11 — Get Info (Shift+I) resolves the focused browser tab
// ============================================================================

#[test]
fn get_info_on_focused_albums_browser_tab_opens_modal() {
    // current_view pinned to PlaylistEditor (real edit-mode host), browser
    // focused on Albums with an album centered. Pre-fix: falls through to the
    // PlaylistEditor catch-all and shows a "not available" toast.
    let mut app = test_app();
    app.current_view = View::PlaylistEditor;
    open_browser_pane(&mut app, BrowsingView::Albums);
    seed_albums(&mut app, vec![make_album("a1", "Album One", "Artist")]);
    // viewport_offset 0 + center slot resolves get_center_item_index → Some(0).

    let _ = app.handle_get_info();

    assert!(
        app.info_modal.visible,
        "Get Info on a focused Albums browser tab should open the info modal"
    );
}

#[test]
fn get_info_on_focused_songs_browser_tab_opens_modal() {
    let mut app = test_app();
    app.current_view = View::PlaylistEditor;
    open_browser_pane(&mut app, BrowsingView::Songs);
    seed_songs(&mut app, vec![make_song("s1", "Song One", "Artist")]);

    let _ = app.handle_get_info();

    assert!(
        app.info_modal.visible,
        "Get Info on a focused Songs browser tab should open the info modal"
    );
}

#[test]
fn get_info_on_focused_artists_browser_tab_opens_modal() {
    let mut app = test_app();
    app.current_view = View::PlaylistEditor;
    open_browser_pane(&mut app, BrowsingView::Artists);
    seed_artists(&mut app, vec![make_artist("ar1", "Artist One")]);

    let _ = app.handle_get_info();

    assert!(
        app.info_modal.visible,
        "Get Info on a focused Artists browser tab should open the info modal"
    );
}

#[test]
fn get_info_similar_browser_tab_still_opens() {
    // Regression guard: the dedicated Similar branch must keep working.
    let mut app = test_app();
    app.current_view = View::PlaylistEditor;
    open_browser_pane(&mut app, BrowsingView::Similar);
    app.similar_songs = Some(crate::state::SimilarSongsState {
        songs: vec![similar_song("s1", "Similar One")],
        label: "Similar to: Test".to_string(),
        loading: false,
    });

    let _ = app.handle_get_info();

    assert!(
        app.info_modal.visible,
        "Get Info on the Similar browser tab should still open the info modal"
    );
}
