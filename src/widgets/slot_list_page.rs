//! Generic Slot List Page Trait
//!
//! Provides common functionality for slot-list-based views (Albums, Artists, Songs, Genres, Playlists).
//! Eliminates ~800 lines of duplicated code by abstracting common patterns.

use crate::widgets::{SlotListView, view_header::SortMode};

/// Common state shared by all slot-list-based views
#[derive(Debug)]
pub struct SlotListPageState {
    pub slot_list: SlotListView,
    pub search_query: String,
    pub current_sort_mode: SortMode,
    pub sort_ascending: bool,
    pub search_input_focused: bool,
}

impl SlotListPageState {
    /// Create a new slot list page state with default values
    pub fn new(default_sort_mode: SortMode, default_sort_ascending: bool) -> Self {
        Self {
            slot_list: SlotListView::new(),
            search_query: String::new(),
            current_sort_mode: default_sort_mode,
            sort_ascending: default_sort_ascending,
            search_input_focused: false,
        }
    }

    /// Create a slot list page state for views that use their own sort enum
    /// (e.g., Queue uses `QueueSortMode`). The `current_sort_mode` field is
    /// set to a sentinel value and should not be read.
    pub fn new_without_sort_mode() -> Self {
        Self::new(SortMode::RecentlyAdded, true)
    }
}

impl Default for SlotListPageState {
    fn default() -> Self {
        Self::new(SortMode::RecentlyAdded, false)
    }
}

///
/// Views should wrap these in their own action enum, e.g.:
/// ```
/// pub enum AlbumsAction {
///     SlotList(SlotListPageAction),
///     // ... view-specific actions
/// }
/// ```
#[derive(Debug, Clone)]
pub enum SlotListPageAction {
    SearchChanged(String),
    SortModeChanged(SortMode),
    SortOrderChanged(bool),
    None,
}

/// Helper functions for common slot list page update logic
impl SlotListPageState {
    /// Handle slot list navigation up
    pub fn handle_navigate_up(&mut self, total_items: usize) {
        self.slot_list.move_up(total_items);
        self.slot_list.record_scroll();
    }

    /// Handle slot list navigation down
    pub fn handle_navigate_down(&mut self, total_items: usize) {
        self.slot_list.move_down(total_items);
        self.slot_list.record_scroll();
    }

    /// Handle slot list offset change (moves viewport, clears selected_offset)
    pub fn handle_set_offset(&mut self, offset: usize, total_items: usize) {
        self.slot_list.set_offset(offset, total_items);
        self.slot_list.record_scroll();
    }

    /// Handle click-to-focus: highlight the item without moving the viewport.
    /// Sets `selected_offset` so the item gets center styling in-place.
    pub fn handle_select_offset(&mut self, offset: usize, total_items: usize) {
        self.slot_list.set_selected(offset, total_items);
        self.slot_list.record_scroll();
    }

    /// Handle sort mode selection
    pub fn handle_sort_mode_selected(&mut self, sort_mode: SortMode) -> SlotListPageAction {
        self.current_sort_mode = sort_mode;
        SlotListPageAction::SortModeChanged(sort_mode)
    }

    /// Handle sort order toggle
    pub fn handle_toggle_sort_order(&mut self) -> SlotListPageAction {
        self.sort_ascending = !self.sort_ascending;
        SlotListPageAction::SortOrderChanged(self.sort_ascending)
    }

    /// Handle search query change
    pub fn handle_search_query_changed(
        &mut self,
        query: String,
        total_items: usize,
    ) -> SlotListPageAction {
        self.search_query = query.clone();
        self.slot_list.set_offset(0, total_items); // Reset to top on search
        SlotListPageAction::SearchChanged(query)
    }

    /// Handle search focus change
    pub fn handle_search_focused(&mut self, focused: bool) {
        self.search_input_focused = focused;
    }

    /// Get the currently centered item index.
    /// If `selected_offset` is set (click-to-focus), returns that instead.
    pub fn get_center_item_index(&self, total_items: usize) -> Option<usize> {
        self.slot_list.get_effective_center_index(total_items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slot_list_page_state_default() {
        let state = SlotListPageState::default();
        assert_eq!(state.search_query, "");
        assert_eq!(state.current_sort_mode, SortMode::RecentlyAdded);
        assert!(!state.sort_ascending);
        assert!(!state.search_input_focused);
    }

    #[test]
    fn test_slot_list_page_state_custom() {
        let state = SlotListPageState::new(SortMode::Random, true);
        assert_eq!(state.current_sort_mode, SortMode::Random);
        assert!(state.sort_ascending);
    }

    #[test]
    fn test_handle_sort_mode_selected() {
        let mut state = SlotListPageState::default();
        let action = state.handle_sort_mode_selected(SortMode::MostPlayed);

        assert_eq!(state.current_sort_mode, SortMode::MostPlayed);
        assert!(matches!(
            action,
            SlotListPageAction::SortModeChanged(SortMode::MostPlayed)
        ));
    }

    #[test]
    fn test_handle_toggle_sort_order() {
        let mut state = SlotListPageState::default();
        assert!(!state.sort_ascending);

        let action = state.handle_toggle_sort_order();
        assert!(state.sort_ascending);
        assert!(matches!(action, SlotListPageAction::SortOrderChanged(true)));

        let action = state.handle_toggle_sort_order();
        assert!(!state.sort_ascending);
        assert!(matches!(
            action,
            SlotListPageAction::SortOrderChanged(false)
        ));
    }

    #[test]
    fn test_handle_search_query_changed() {
        let mut state = SlotListPageState::default();
        state.slot_list.set_offset(10, 100); // Start at offset 10

        let action = state.handle_search_query_changed("test".to_string(), 50);

        assert_eq!(state.search_query, "test");
        assert_eq!(state.slot_list.viewport_offset, 0); // Should reset to top
        assert!(matches!(action, SlotListPageAction::SearchChanged(_)));
    }

    #[test]
    fn test_handle_search_focused() {
        let mut state = SlotListPageState::default();
        assert!(!state.search_input_focused);

        state.handle_search_focused(true);
        assert!(state.search_input_focused);

        state.handle_search_focused(false);
        assert!(!state.search_input_focused);
    }

    #[test]
    fn test_slot_list_navigation() {
        let mut state = SlotListPageState::default();
        let total_items = 20;

        // Start at offset 0
        assert_eq!(state.slot_list.viewport_offset, 0);

        // Navigate down
        state.handle_navigate_down(total_items);
        assert_eq!(state.slot_list.viewport_offset, 1);

        // Navigate up
        state.handle_navigate_up(total_items);
        assert_eq!(state.slot_list.viewport_offset, 0);

        // Set specific offset
        state.handle_set_offset(5, total_items);
        assert_eq!(state.slot_list.viewport_offset, 5);
    }
}
