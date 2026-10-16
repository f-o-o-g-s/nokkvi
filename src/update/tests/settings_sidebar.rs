//! Tests for the settings persistent sidebar — category selection state,
//! detail-pane reset on category change, and search isolation.
//!
//! These cover the Phase 2a additions in `views/settings/mod.rs`:
//! `active_category`, `sidebar_slot_list`, and the four `Sidebar*`
//! `SettingsMessage` variants. The sidebar lives alongside the legacy
//! drill-down state in this phase; tests target the new fields directly.

use crate::{
    test_helpers::*,
    views::settings::{SettingsMessage, SettingsTab},
};

/// `SettingsTab::ALL` order is the ground truth for sidebar row indices.
/// Pinning it explicitly so a reorder there is caught here before it
/// silently shuffles every sidebar nav assertion below.
#[test]
fn sidebar_index_order_matches_settings_tab_all() {
    let tabs: Vec<SettingsTab> = SettingsTab::ALL.to_vec();
    assert_eq!(
        tabs,
        vec![
            SettingsTab::General,
            SettingsTab::Interface,
            SettingsTab::Playback,
            SettingsTab::Hotkeys,
            SettingsTab::Theme,
            SettingsTab::Visualizer,
        ],
    );
}

#[test]
fn new_settings_page_defaults_to_general_active() {
    let page = crate::views::SettingsPage::new();
    assert_eq!(page.active_category, SettingsTab::General);
    assert_eq!(page.sidebar_slot_list.viewport_offset, 0);
}

#[test]
fn sidebar_down_advances_active_category() {
    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();
    let _ = page.update(SettingsMessage::SidebarDown, &data);
    assert_eq!(page.active_category, SettingsTab::Interface);
    assert_eq!(page.sidebar_slot_list.viewport_offset, 1);
}

#[test]
fn sidebar_up_clamps_at_zero() {
    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();
    // Already at index 0; SidebarUp should be a no-op (saturating_sub).
    let _ = page.update(SettingsMessage::SidebarUp, &data);
    assert_eq!(page.active_category, SettingsTab::General);
    assert_eq!(page.sidebar_slot_list.viewport_offset, 0);
}

#[test]
fn sidebar_down_clamps_at_last_index() {
    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();
    // Walk to the last tab (Visualizer = index 5).
    for _ in 0..SettingsTab::ALL.len() - 1 {
        let _ = page.update(SettingsMessage::SidebarDown, &data);
    }
    assert_eq!(page.active_category, SettingsTab::Visualizer);
    assert_eq!(page.sidebar_slot_list.viewport_offset, 5);

    // One more SidebarDown is clamped.
    let _ = page.update(SettingsMessage::SidebarDown, &data);
    assert_eq!(page.active_category, SettingsTab::Visualizer);
    assert_eq!(page.sidebar_slot_list.viewport_offset, 5);
}

#[test]
fn sidebar_click_sets_active_category_directly() {
    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();
    // Click index 4 (Theme).
    let _ = page.update(SettingsMessage::SidebarClickItem(4), &data);
    assert_eq!(page.active_category, SettingsTab::Theme);
    assert_eq!(page.sidebar_slot_list.viewport_offset, 4);
}

#[test]
fn sidebar_set_offset_sets_active_category() {
    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();
    let _ = page.update(
        SettingsMessage::SidebarSetOffset(2, iced::keyboard::Modifiers::default()),
        &data,
    );
    assert_eq!(page.active_category, SettingsTab::Playback);
    assert_eq!(page.sidebar_slot_list.viewport_offset, 2);
}

#[test]
fn sidebar_motion_resets_detail_slot_list() {
    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();
    // Force the detail slot list to a non-zero offset to prove the reset.
    page.slot_list.viewport_offset = 7;
    let _ = page.update(SettingsMessage::SidebarDown, &data);
    assert_eq!(
        page.slot_list.viewport_offset, 0,
        "category change should reset the detail pane focus to slot 0",
    );
}

#[test]
fn sidebar_motion_to_same_category_does_not_reset_detail() {
    // Click the already-active category. apply_sidebar_index early-returns
    // when the new tab equals the current one, so the detail slot list
    // must survive untouched.
    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();
    page.slot_list.viewport_offset = 11;
    let _ = page.update(SettingsMessage::SidebarClickItem(0), &data);
    assert_eq!(page.active_category, SettingsTab::General);
    assert_eq!(
        page.slot_list.viewport_offset, 11,
        "clicking the active category must not blow away detail-pane focus",
    );
}

#[test]
fn sidebar_motion_clears_edit_state() {
    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();
    page.editing_index = Some(3);
    page.toggle_cursor = Some(2);
    page.hex_input = "#abcdef".to_string();

    let _ = page.update(SettingsMessage::SidebarDown, &data);

    assert!(page.editing_index.is_none());
    assert!(page.toggle_cursor.is_none());
    assert!(page.hex_input.is_empty());
}

#[test]
fn sidebar_motion_preserves_search_query() {
    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();
    page.search_query = "crossfade".to_string();
    page.search_active = true;

    let _ = page.update(SettingsMessage::SidebarDown, &data);

    assert_eq!(
        page.search_query, "crossfade",
        "sidebar nav must not touch the cross-tab search query",
    );
    assert!(page.search_active);
}

#[test]
fn sidebar_set_offset_clamps_oob_index() {
    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();
    // SettingsTab::ALL has 6 entries; index 99 is out of range.
    // SlotListView::set_offset rejects out-of-range offsets silently;
    // apply_sidebar_index then reads viewport_offset which stays 0.
    let _ = page.update(
        SettingsMessage::SidebarSetOffset(99, iced::keyboard::Modifiers::default()),
        &data,
    );
    assert_eq!(page.active_category, SettingsTab::General);
    assert_eq!(page.sidebar_slot_list.viewport_offset, 0);
}

// ============================================================================
// Keyboard focus between the sidebar and the rows (d-pad reachability)
// ============================================================================
//
// The arrows, Enter and Escape alone must reach every category: a TV remote
// has nothing else. Before the persistent sidebar, Settings was a drill-down
// and Escape walked back to the category picker; these pin that walk on the
// two-pane layout.

use crate::views::settings::{SettingsAction, SettingsFocus};

#[test]
fn settings_opens_with_the_keyboard_on_the_rows() {
    let page = crate::views::SettingsPage::new();
    assert_eq!(page.focus, SettingsFocus::Detail);
}

#[test]
fn escape_from_the_rows_moves_to_the_sidebar_instead_of_leaving() {
    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();

    let action = page.update(SettingsMessage::Escape, &data);

    assert!(
        matches!(action, SettingsAction::None),
        "Escape on the rows must not leave Settings, got: {action:?}"
    );
    assert_eq!(page.focus, SettingsFocus::Sidebar);
}

#[test]
fn escape_from_the_sidebar_leaves_and_reopens_on_the_rows() {
    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();
    page.focus = SettingsFocus::Sidebar;

    let action = page.update(SettingsMessage::Escape, &data);

    assert!(
        matches!(action, SettingsAction::ExitSettings),
        "Escape on the sidebar must leave Settings, got: {action:?}"
    );
    assert_eq!(
        page.focus,
        SettingsFocus::Detail,
        "the next visit opens on the rows again"
    );
}

#[test]
fn up_and_down_on_the_sidebar_change_category() {
    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();
    page.focus = SettingsFocus::Sidebar;

    let _ = page.update(SettingsMessage::SlotListDown, &data);
    assert_eq!(page.active_category, SettingsTab::Interface);
    let _ = page.update(SettingsMessage::SlotListDown, &data);
    assert_eq!(page.active_category, SettingsTab::Playback);
    let _ = page.update(SettingsMessage::SlotListUp, &data);
    assert_eq!(page.active_category, SettingsTab::Interface);

    assert_eq!(page.focus, SettingsFocus::Sidebar);
    assert_eq!(
        page.slot_list.viewport_offset, 0,
        "the rows' cursor stays put while the sidebar moves"
    );
}

#[test]
fn enter_or_right_on_the_sidebar_walks_into_the_rows() {
    let data = make_settings_view_data();
    for message in [SettingsMessage::EditActivate, SettingsMessage::EditRight] {
        let mut page = crate::views::SettingsPage::new();
        page.focus = SettingsFocus::Sidebar;
        let _ = page.update(SettingsMessage::SlotListDown, &data);

        let action = page.update(message.clone(), &data);

        assert!(matches!(action, SettingsAction::None), "{message:?}");
        assert_eq!(page.focus, SettingsFocus::Detail, "{message:?}");
        assert_eq!(page.active_category, SettingsTab::Interface, "{message:?}");
        assert!(
            page.editing_index.is_none(),
            "{message:?} only moves focus, it starts no edit"
        );
    }
}

#[test]
fn left_and_delete_on_the_sidebar_touch_no_row() {
    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();
    page.refresh_entries(&data);
    // Park the rows' cursor on a row Left would change, so a stray edit shows.
    let editable = page
        .cached_entries
        .iter()
        .position(|entry| {
            matches!(entry, crate::views::settings::items::SettingsEntry::Item(item)
                if item.value.is_editable())
        })
        .expect("General has an editable row");
    page.slot_list.viewport_offset = editable;
    page.focus = SettingsFocus::Sidebar;
    let entries_before = format!("{:?}", page.cached_entries);

    let left = page.update(SettingsMessage::EditLeft, &data);
    let reset = page.update(SettingsMessage::ResetToDefault, &data);

    assert!(matches!(left, SettingsAction::None), "got: {left:?}");
    assert!(matches!(reset, SettingsAction::None), "got: {reset:?}");
    assert!(page.editing_index.is_none());
    assert!(page.toggle_cursor.is_none());
    assert_eq!(format!("{:?}", page.cached_entries), entries_before);
    assert_eq!(page.focus, SettingsFocus::Sidebar);
}

#[test]
fn a_click_or_a_search_takes_the_keyboard_back_to_the_rows() {
    let data = make_settings_view_data();
    for message in [
        SettingsMessage::SlotListClickItem(1),
        SettingsMessage::SearchChanged("crossfade".to_string()),
        SettingsMessage::SidebarClickItem(2),
    ] {
        let mut page = crate::views::SettingsPage::new();
        page.refresh_entries(&data);
        page.focus = SettingsFocus::Sidebar;

        let _ = page.update(message.clone(), &data);

        assert_eq!(page.focus, SettingsFocus::Detail, "{message:?}");
    }
}

#[test]
fn shift_tab_category_steps_keep_the_focus_where_it_is() {
    let data = make_settings_view_data();
    for focus in [SettingsFocus::Sidebar, SettingsFocus::Detail] {
        let mut page = crate::views::SettingsPage::new();
        page.focus = focus;

        let _ = page.update(SettingsMessage::SidebarDown, &data);

        assert_eq!(page.active_category, SettingsTab::Interface);
        assert_eq!(page.focus, focus);
    }
}

#[test]
fn the_esc_pill_leaves_in_one_click_from_any_depth() {
    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();
    page.refresh_entries(&data);
    page.editing_index = Some(1);
    page.toggle_cursor = Some(0);
    page.capturing_hotkey = Some(nokkvi_data::types::hotkey_config::HotkeyAction::TogglePlay);

    let action = page.update(SettingsMessage::Leave, &data);

    assert!(
        matches!(action, SettingsAction::ExitSettings),
        "the pill must leave Settings, got: {action:?}"
    );
    assert!(page.editing_index.is_none());
    assert!(page.toggle_cursor.is_none());
    assert!(page.capturing_hotkey.is_none());
    assert_eq!(page.focus, SettingsFocus::Detail);
}

#[test]
fn the_esc_pill_also_closes_an_open_color_editor() {
    let mut page = crate::views::SettingsPage::new();
    let data = make_settings_view_data();
    page.sub_list = Some(crate::views::settings::SubListState {
        key: "visualizer.bars.dark_colors".to_string(),
        label: "Bar Colors".to_string(),
        colors: vec!["#000000".to_string()],
        slot_list: crate::widgets::SlotListView::new(),
        editing_color_index: None,
        hex_input: String::new(),
        parent_offset: 0,
    });

    let action = page.update(SettingsMessage::Leave, &data);

    assert!(
        matches!(action, SettingsAction::ExitSettings),
        "got: {action:?}"
    );
    assert!(page.sub_list.is_none());
}

#[test]
fn no_row_looks_focused_while_the_sidebar_holds_the_keyboard() {
    let mut page = crate::views::SettingsPage::new();
    page.slot_list.viewport_offset = 3;
    assert_eq!(page.focused_detail_row(), Some(3));

    page.focus = SettingsFocus::Sidebar;
    assert_eq!(page.focused_detail_row(), None);
}

/// App-level: the root fast path that moves the rows' cursor without building
/// `SettingsViewData` must step the sidebar instead when it holds the keyboard.
#[test]
fn arrow_keys_reach_the_categories_through_the_root_handler() {
    let mut app = test_app();
    app.screen = crate::Screen::Home;
    app.current_view = crate::View::Settings;
    app.refresh_settings_entries_if_dirty();
    assert!(
        !app.settings_page.cached_entries.is_empty(),
        "the fast path needs cached rows"
    );

    let _ = app.handle_settings(SettingsMessage::Escape);
    assert_eq!(app.settings_page.focus, SettingsFocus::Sidebar);

    let _ = app.handle_settings(SettingsMessage::SlotListDown);
    assert_eq!(app.settings_page.active_category, SettingsTab::Interface);
    assert_eq!(app.settings_page.slot_list.viewport_offset, 0);

    let _ = app.handle_settings(SettingsMessage::EditActivate);
    assert_eq!(app.settings_page.focus, SettingsFocus::Detail);
    let _ = app.handle_settings(SettingsMessage::SlotListDown);
    assert_eq!(
        app.settings_page.active_category,
        SettingsTab::Interface,
        "back on the rows, Down moves the rows again"
    );
}

#[test]
fn a_section_pill_click_takes_the_keyboard_back_to_the_rows() {
    let mut app = test_app();
    app.screen = crate::Screen::Home;
    app.current_view = crate::View::Settings;
    app.refresh_settings_entries_if_dirty();
    app.settings_page.focus = SettingsFocus::Sidebar;

    let _ = app.handle_settings(SettingsMessage::JumpToSection(0));

    assert_eq!(app.settings_page.focus, SettingsFocus::Detail);
}
