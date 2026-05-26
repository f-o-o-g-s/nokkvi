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
