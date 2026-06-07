//! Tests for the auto-hide toolbar reveal state + the header hotkeys that
//! surface it. Asserts against observable `SlotListPageState` mutations
//! (`toolbar_hovered`, `toolbar_reveal_until`) and the derived
//! `toolbar_revealed()` predicate — no app_service / async needed.

use std::time::{Duration, Instant};

use crate::{View, test_helpers::*, widgets::SlotListPageMessage};

// ----------------------------------------------------------------------------
// Reveal predicate (SlotListPageState::toolbar_revealed)
// ----------------------------------------------------------------------------

#[test]
fn fresh_toolbar_is_collapsed_when_autohide_enabled() {
    let app = test_app();
    let common = &app.songs_page.common;
    // Autohide on, nothing active → collapsed.
    assert!(!common.toolbar_revealed(true));
    // Autohide off → always expanded regardless of state.
    assert!(common.toolbar_revealed(false));
}

#[test]
fn hover_enter_then_exit_toggles_revealed() {
    let mut app = test_app();

    let _ = app
        .songs_page
        .common
        .handle(SlotListPageMessage::ToolbarHoverEnter, 0);
    assert!(app.songs_page.common.toolbar_hovered);
    assert!(app.songs_page.common.toolbar_revealed(true));

    app.songs_page
        .common
        .handle(SlotListPageMessage::ToolbarHoverExit, 0);
    assert!(!app.songs_page.common.toolbar_hovered);
    assert!(!app.songs_page.common.toolbar_revealed(true));
}

#[test]
fn active_search_query_keeps_toolbar_revealed() {
    let mut app = test_app();
    app.songs_page.common.search_query = "radiohead".to_string();
    // Not hovered, no hotkey window — still revealed because a filter is live.
    assert!(!app.songs_page.common.toolbar_hovered);
    assert!(app.songs_page.common.toolbar_revealed(true));
}

#[test]
fn focused_search_input_keeps_toolbar_revealed() {
    let mut app = test_app();
    app.songs_page.common.search_input_focused = true;
    assert!(app.songs_page.common.toolbar_revealed(true));
}

#[test]
fn reveal_toolbar_opens_a_future_window() {
    let mut app = test_app();
    assert!(app.songs_page.common.toolbar_reveal_until.is_none());

    app.songs_page.common.reveal_toolbar();

    let until = app
        .songs_page
        .common
        .toolbar_reveal_until
        .expect("reveal_toolbar sets the window");
    assert!(
        until > Instant::now(),
        "reveal window must be in the future"
    );
    assert!(app.songs_page.common.toolbar_revealed(true));
}

#[test]
fn expired_reveal_window_collapses_again() {
    let mut app = test_app();
    // A window that already elapsed must not keep the toolbar revealed.
    app.songs_page.common.toolbar_reveal_until = Some(Instant::now() - Duration::from_secs(1));
    assert!(!app.songs_page.common.toolbar_revealed(true));
}

// ----------------------------------------------------------------------------
// Header hotkeys reveal the current view's toolbar
// ----------------------------------------------------------------------------

fn songs_app() -> crate::Nokkvi {
    let mut app = test_app();
    app.current_view = View::Songs;
    app.screen = crate::Screen::Home;
    app
}

#[test]
fn toggle_sort_order_reveals_toolbar() {
    let mut app = songs_app();
    assert!(app.songs_page.common.toolbar_reveal_until.is_none());
    let _ = app.handle_toggle_sort_order();
    assert!(app.songs_page.common.toolbar_reveal_until.is_some());
}

#[test]
fn cycle_sort_mode_reveals_toolbar() {
    let mut app = songs_app();
    let _ = app.handle_cycle_sort_mode(true);
    assert!(app.songs_page.common.toolbar_reveal_until.is_some());
}

#[test]
fn focus_search_reveals_toolbar() {
    let mut app = songs_app();
    let _ = app.handle_focus_search();
    assert!(app.songs_page.common.toolbar_reveal_until.is_some());
}

#[test]
fn center_on_playing_reveals_toolbar() {
    // Reveal fires before the no-song early return, so no playback setup needed.
    let mut app = songs_app();
    let _ = app.handle_center_on_playing();
    assert!(app.songs_page.common.toolbar_reveal_until.is_some());
}

// ----------------------------------------------------------------------------
// Sort dropdown reveal-lock (keeps the toolbar open while the menu is open)
// ----------------------------------------------------------------------------

#[test]
fn open_sort_dropdown_keeps_toolbar_revealed() {
    let mut app = test_app();
    // Opening the dropdown reveals the toolbar even with no hover / search /
    // timer — so moving the cursor into the open menu doesn't collapse it.
    let _ = app
        .songs_page
        .common
        .handle(SlotListPageMessage::ToolbarDropdownToggled(true), 0);
    assert!(app.songs_page.common.toolbar_dropdown_open);
    assert!(app.songs_page.common.toolbar_revealed(true));

    // Closing via click-outside / trigger (on_close) drops the lock.
    let _ = app
        .songs_page
        .common
        .handle(SlotListPageMessage::ToolbarDropdownToggled(false), 0);
    assert!(!app.songs_page.common.toolbar_dropdown_open);
    assert!(!app.songs_page.common.toolbar_revealed(true));
}

#[test]
fn selecting_sort_mode_clears_dropdown_lock() {
    let mut app = test_app();
    let _ = app
        .songs_page
        .common
        .handle(SlotListPageMessage::ToolbarDropdownToggled(true), 0);
    assert!(app.songs_page.common.toolbar_dropdown_open);
    // Selecting an option closes the menu via on_select (NOT on_close), so the
    // sort handler must clear the lock or it would stick revealed forever.
    let _ = app
        .songs_page
        .common
        .handle_sort_mode_selected(crate::widgets::view_header::SortMode::Name);
    assert!(!app.songs_page.common.toolbar_dropdown_open);
}

#[test]
fn closing_browsing_panel_clears_stranded_dropdown_locks() {
    let mut app = test_app();
    app.current_view = View::Queue;
    app.screen = crate::Screen::Home;
    app.browsing_panel = Some(crate::views::BrowsingPanel::new());
    // A panel-hosted page's sort dropdown is open when the panel closes — its
    // pick_list unmounts, so on_close can't fire to drop the lock.
    app.albums_page.common.set_toolbar_dropdown_open(true);

    let _ = app.handle_toggle_browsing_panel();

    assert!(app.browsing_panel.is_none(), "panel should have closed");
    assert!(
        !app.albums_page.common.toolbar_dropdown_open,
        "closing the panel must clear stranded dropdown locks (else toolbar stuck revealed)"
    );
}

// ----------------------------------------------------------------------------
// Stranded reveal-locks are cleared on unmount edges
//
// `toolbar_hovered` / `toolbar_dropdown_open` are only cleared by the header's
// mouse_area `on_exit` / pick_list `on_close`, which can't fire once the header
// has unmounted. A view switch, panel close, or session reset must clear them
// or the toolbar stays stuck revealed on return ("sometimes won't autohide").
// ----------------------------------------------------------------------------

#[test]
fn switching_view_clears_stranded_hover() {
    // Hover reveals the toolbar; a keyboard view-switch unmounts the header
    // mouse_area so on_exit can't fire — the switch itself must clear it.
    let mut app = songs_app();
    app.songs_page.common.set_toolbar_hovered(true);
    assert!(app.songs_page.common.toolbar_revealed(true));

    let _ = app.handle_switch_view(View::Albums);

    assert!(!app.songs_page.common.toolbar_hovered);
    assert!(!app.songs_page.common.toolbar_revealed(true));
}

#[test]
fn switching_view_clears_stranded_dropdown_lock() {
    // A sort dropdown open during a keyboard view-switch unmounts its pick_list
    // (no on_close) — the switch must drop the lock or the toolbar sticks open.
    let mut app = songs_app();
    app.songs_page.common.set_toolbar_dropdown_open(true);

    let _ = app.handle_switch_view(View::Albums);

    assert!(!app.songs_page.common.toolbar_dropdown_open);
    assert!(!app.songs_page.common.toolbar_revealed(true));
}

#[test]
fn switching_view_clears_stranded_reveal_window() {
    // A hotkey-opened reveal window on the outgoing view shouldn't carry over.
    let mut app = songs_app();
    app.songs_page.common.reveal_toolbar();
    assert!(app.songs_page.common.toolbar_reveal_until.is_some());

    let _ = app.handle_switch_view(View::Albums);

    assert!(app.songs_page.common.toolbar_reveal_until.is_none());
}

#[test]
fn closing_browsing_panel_clears_stranded_hover() {
    let mut app = test_app();
    app.current_view = View::Queue;
    app.screen = crate::Screen::Home;
    app.browsing_panel = Some(crate::views::BrowsingPanel::new());
    // A panel-hosted page's header is hovered when the panel closes — its
    // mouse_area unmounts, so on_exit can't fire to clear the hover flag.
    app.albums_page.common.set_toolbar_hovered(true);

    let _ = app.handle_toggle_browsing_panel();

    assert!(app.browsing_panel.is_none(), "panel should have closed");
    assert!(
        !app.albums_page.common.toolbar_hovered,
        "closing the panel must clear stranded hover (else toolbar stuck revealed)"
    );
}

#[test]
fn session_reset_clears_stranded_reveal_locks() {
    // A dropdown/hover lock set at logout/session-expiry must not survive into
    // the next session (the pick_list unmounts on the Login-screen swap).
    let mut app = songs_app();
    app.songs_page.common.set_toolbar_dropdown_open(true);
    app.albums_page.common.set_toolbar_hovered(true);

    let _ = app.reset_session_state();

    assert!(!app.songs_page.common.toolbar_dropdown_open);
    assert!(!app.albums_page.common.toolbar_hovered);
}

// ----------------------------------------------------------------------------
// Collapsed-chrome math (space reclamation)
// ----------------------------------------------------------------------------

#[test]
fn collapsed_chrome_reclaims_space() {
    use crate::widgets::slot_list::{
        chrome_height_with_header, collapsed_view_header_chrome, view_header_chrome,
    };
    // The collapsed footprint is strictly smaller than the full toolbar's.
    assert!(collapsed_view_header_chrome() < view_header_chrome());
    // Collapsing reduces total chrome by exactly the header-chrome delta, so
    // the slot list reclaims that space as extra rows.
    let delta = chrome_height_with_header(false) - chrome_height_with_header(true);
    let expected = view_header_chrome() - collapsed_view_header_chrome();
    assert!((delta - expected).abs() < f32::EPSILON);
}

#[test]
fn reveal_current_toolbar_is_noop_on_settings() {
    let mut app = test_app();
    app.current_view = View::Settings;
    app.screen = crate::Screen::Home;
    // Must not panic and must not touch any slot-list page.
    app.reveal_current_toolbar();
    assert!(app.songs_page.common.toolbar_reveal_until.is_none());
}
