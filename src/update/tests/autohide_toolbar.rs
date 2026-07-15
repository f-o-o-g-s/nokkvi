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
fn center_on_playing_does_not_reveal_toolbar() {
    // Shift+C scrolls the list to centre the playing track; it changes nothing
    // in the toolbar itself (unlike sort cycle / focus search, which alter
    // toolbar content), so it must NOT surface the auto-hide toolbar. Leaving
    // it collapsed also avoids stranding the 2.5s reveal window when the user
    // immediately focuses another OS window mid-reveal.
    //
    // Seed the playing song INTO the loaded buffer so the handler reaches the
    // centering branch — not the no-song early return — i.e. the test actually
    // exercises the path where a reveal could be re-introduced.
    let mut app = songs_app();
    seed_songs(&mut app, songs_indexed(20));
    app.scrobble.current_song_id = Some("s5".to_string());

    let _ = app.handle_center_on_playing();

    assert!(app.songs_page.common.toolbar_reveal_until.is_none());
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
fn window_unfocus_clears_transient_reveal_locks_but_keeps_search() {
    // On Wayland an unfocused surface stops receiving frame callbacks, so a
    // mid-reveal toolbar (2.5s hotkey timer, hover, or open dropdown) would
    // strand expanded — the timer expires in wall-clock time with no repaint to
    // collapse it. Losing OS focus must drop those transient locks on every
    // page. A live search query legitimately keeps its own toolbar revealed.
    let mut app = songs_app();
    app.songs_page.common.reveal_toolbar();
    app.songs_page.common.set_toolbar_hovered(true);
    app.albums_page.common.set_toolbar_dropdown_open(true);
    app.artists_page.common.search_query = "boards of canada".to_string();

    let _ = app.update(crate::app_message::Message::WindowUnfocused);

    // Transient locks cleared everywhere → those toolbars collapse.
    assert!(app.songs_page.common.toolbar_reveal_until.is_none());
    assert!(!app.songs_page.common.toolbar_hovered);
    assert!(!app.albums_page.common.toolbar_dropdown_open);
    assert!(!app.songs_page.common.toolbar_revealed(true));
    assert!(!app.albums_page.common.toolbar_revealed(true));
    // Search-driven reveal is preserved (reset_reveal_locks leaves search state).
    assert_eq!(app.artists_page.common.search_query, "boards of canada");
    assert!(app.artists_page.common.toolbar_revealed(true));
}

#[test]
fn unfocused_window_collapses_hovered_toolbar() {
    // The core robustness guarantee: even if `toolbar_hovered` is (or stays)
    // true — its `on_exit` never fired on the unfocused Wayland surface, or the
    // mouse_area re-published `on_enter` over a parked cursor — an unfocused
    // window must collapse the transient reveal at render time, independent of
    // whether the reveal-lock got cleared.
    let mut app = songs_app();
    app.songs_page.common.set_toolbar_hovered(true);
    assert!(
        app.songs_page.common.toolbar_revealed(true),
        "focused + hovered → revealed"
    );

    app.songs_page.common.set_window_focused(false);
    assert!(
        !app.songs_page.common.toolbar_revealed(true),
        "unfocused → the transient hover reveal collapses"
    );

    // A live search query still reveals even while unfocused (not focus-gated).
    app.songs_page.common.search_query = "aphex".to_string();
    assert!(
        app.songs_page.common.toolbar_revealed(true),
        "search-driven reveal is not focus-gated"
    );
}

#[test]
fn unfocused_window_collapses_focused_but_empty_search() {
    // Repro from the field report: pressing `/` focuses the (empty) search box,
    // then the user switches to another OS window. A focused-but-EMPTY search
    // input must NOT pin the toolbar open while unfocused — only a non-empty
    // filter survives focus loss (so a live filter is never hidden).
    let mut app = songs_app();
    app.songs_page.common.search_input_focused = true;
    assert!(app.songs_page.common.search_query.is_empty());
    assert!(
        app.songs_page.common.toolbar_revealed(true),
        "focused empty search while FOCUSED → revealed"
    );

    app.songs_page.common.set_window_focused(false);
    assert!(
        !app.songs_page.common.toolbar_revealed(true),
        "focused empty search while UNFOCUSED → collapses"
    );

    // A non-empty filter, however, stays revealed even while unfocused.
    app.songs_page.common.search_query = "boards".to_string();
    assert!(
        app.songs_page.common.toolbar_revealed(true),
        "non-empty filter is never hidden, even unfocused"
    );
}

#[test]
fn unfocused_window_collapses_toolbar_with_open_columns_dropdown() {
    // The columns-cog dropdown must not strand the toolbar expanded behind
    // another window. While focused, an open dropdown keeps the toolbar open
    // (not collapsed); losing focus collapses it regardless.
    let mut app = songs_app();
    assert!(
        !app.songs_page.common.toolbar_collapsed(true, true),
        "focused + open columns dropdown → toolbar stays open"
    );
    app.songs_page.common.set_window_focused(false);
    assert!(
        app.songs_page.common.toolbar_collapsed(true, true),
        "unfocused → an open columns dropdown can't strand the toolbar"
    );
}

#[test]
fn window_unfocus_closes_header_dropdowns_only() {
    // Header-anchored dropdowns (columns cog + the queue server-sync menu) render
    // from stored trigger bounds independent of the header, so they must close on
    // unfocus or they strand visible over another app (and with autohide on,
    // re-fire open on refocus once the reveal-lock remounts the trigger). Other
    // menus (context menu, library selector, hamburger) are not toolbar-related
    // and must stay open — closing them on a transient focus blip (amplified by
    // focus-follows-mouse) would be a regression.
    let mut app = songs_app();

    app.open_menu = Some(crate::app_message::OpenMenu::CheckboxDropdown {
        view: View::Songs,
        trigger_bounds: iced::Rectangle::default(),
    });
    let _ = app.update(crate::app_message::Message::WindowUnfocused);
    assert!(
        app.open_menu.is_none(),
        "columns-cog dropdown closes on unfocus"
    );

    // The queue server-sync action menu shares the same header-anchored,
    // stored-bounds chassis and must close on unfocus too.
    app.open_menu = Some(crate::app_message::OpenMenu::QueueSync {
        trigger_bounds: iced::Rectangle::default(),
    });
    let _ = app.update(crate::app_message::Message::WindowUnfocused);
    assert!(
        app.open_menu.is_none(),
        "queue server-sync menu closes on unfocus"
    );

    app.open_menu = Some(crate::app_message::OpenMenu::Hamburger);
    let _ = app.update(crate::app_message::Message::WindowUnfocused);
    assert!(
        matches!(app.open_menu, Some(crate::app_message::OpenMenu::Hamburger)),
        "non-toolbar menus stay open across a focus change"
    );
}

#[test]
fn window_unfocus_drops_search_focus_so_refocus_doesnt_reveal() {
    // Field report: pressing `/` (focuses the empty search box) then leaving and
    // re-entering the window re-expanded the header with the cursor nowhere near
    // it. Under auto-hide the search box unmounts when the toolbar collapses, so
    // the lingering `search_input_focused` re-revealed on refocus. Losing focus
    // must drop it — but only when the box actually unmounts (see the two
    // keeps-focus tests below).
    let _guard = crate::theme::THEME_MODE_LOCK.lock();
    crate::theme::set_autohide_toolbar(true);

    let mut app = songs_app();
    app.songs_page.common.search_input_focused = true;
    assert!(app.songs_page.common.search_query.is_empty());

    let _ = app.update(crate::app_message::Message::WindowUnfocused);
    crate::theme::set_autohide_toolbar(false); // restore before asserts

    assert!(
        !app.songs_page.common.search_input_focused,
        "empty focused search box unmounts under auto-hide → focus dropped"
    );

    let _ = app.update(crate::app_message::Message::WindowFocused);
    assert!(
        !app.songs_page.common.toolbar_revealed(true),
        "refocus must not re-reveal the header from stale search focus"
    );
}

#[test]
fn window_unfocus_keeps_search_focus_with_active_filter() {
    // A non-empty filter keeps the search box mounted and really iced-focused,
    // so clearing search_input_focused would desync from iced and break Tab-out
    // / Escape (both read this flag). The flag must be left set.
    let _guard = crate::theme::THEME_MODE_LOCK.lock();
    crate::theme::set_autohide_toolbar(true);

    let mut app = songs_app();
    app.songs_page.common.search_query = "aphex".to_string();
    app.songs_page.common.search_input_focused = true;

    let _ = app.update(crate::app_message::Message::WindowUnfocused);
    crate::theme::set_autohide_toolbar(false);

    assert!(
        app.songs_page.common.search_input_focused,
        "active filter keeps a mounted, focused search box — flag must survive"
    );
}

#[test]
fn window_unfocus_keeps_search_focus_when_autohide_off() {
    // Auto-hide off → the toolbar (and search box) is always mounted, so the
    // flag tracks iced's real focus; clearing it would break Tab/Escape.
    let _guard = crate::theme::THEME_MODE_LOCK.lock();
    crate::theme::set_autohide_toolbar(false);

    let mut app = songs_app();
    app.songs_page.common.search_input_focused = true;

    let _ = app.update(crate::app_message::Message::WindowUnfocused);

    assert!(
        app.songs_page.common.search_input_focused,
        "auto-hide off keeps the box mounted — search focus is left intact"
    );
}

#[test]
fn window_focus_round_trip_restores_transient_reveal() {
    let mut app = songs_app();

    let _ = app.update(crate::app_message::Message::WindowUnfocused);
    assert!(!app.songs_page.common.window_focused);

    // Refocus re-enables transient reveals but does NOT auto-reveal (hover was
    // cleared on unfocus); a genuine cursor move must re-fire the hover.
    let _ = app.update(crate::app_message::Message::WindowFocused);
    assert!(app.songs_page.common.window_focused);
    assert!(
        !app.songs_page.common.toolbar_revealed(true),
        "no auto-reveal on refocus"
    );

    app.songs_page.common.set_toolbar_hovered(true);
    assert!(
        app.songs_page.common.toolbar_revealed(true),
        "a real hover after refocus reveals again"
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
