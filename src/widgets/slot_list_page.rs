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
    pub active_filter: Option<nokkvi_data::types::filter::LibraryFilter>,
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
            active_filter: None,
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
    /// Handles Shift and Ctrl modifiers to manage the multi-selection set.
    pub fn handle_slot_click(
        &mut self,
        offset: usize,
        total_items: usize,
        modifiers: iced::keyboard::Modifiers,
    ) {
        if offset >= total_items {
            return;
        }

        if modifiers.control() {
            // Toggle selection for clicked item
            if self.slot_list.selected_indices.contains(&offset) {
                self.slot_list.selected_indices.remove(&offset);
                if self.slot_list.anchor_index == Some(offset) {
                    self.slot_list.anchor_index = None;
                }
                // Clear focus cursor when the selection set empties, otherwise
                // the deselected slot keeps its center-highlight.
                if self.slot_list.selected_indices.is_empty() {
                    self.slot_list.selected_offset = None;
                }
            } else {
                self.slot_list.selected_indices.insert(offset);
                self.slot_list.anchor_index = Some(offset);
                self.slot_list.selected_offset = Some(offset);
            }
        } else if modifiers.shift() {
            // Range selection
            if let Some(anchor) = self.slot_list.anchor_index {
                let start = anchor.min(offset);
                let end = anchor.max(offset);

                // Clear existing selection except anchor, then add range
                self.slot_list.selected_indices.clear();
                for i in start..=end {
                    self.slot_list.selected_indices.insert(i);
                }
            } else {
                // No anchor yet, behave like a normal click
                self.slot_list.selected_indices.clear();
                self.slot_list.selected_indices.insert(offset);
                self.slot_list.anchor_index = Some(offset);
            }
            self.slot_list.selected_offset = Some(offset);
        } else {
            // Normal click: clear multi-selection, select only this
            self.clear_multi_selection();
            self.slot_list.selected_indices.insert(offset);
            self.slot_list.anchor_index = Some(offset);
            self.slot_list.selected_offset = Some(offset);
        }
    }

    /// Clear current multi-selection and return true if anything was cleared.
    pub fn clear_multi_selection(&mut self) -> bool {
        let has_selection =
            !self.slot_list.selected_indices.is_empty() || self.slot_list.anchor_index.is_some();
        self.slot_list.selected_indices.clear();
        self.slot_list.anchor_index = None;
        has_selection
    }

    /// Evaluate a context menu click. If the clicked index is not in the selection,
    /// the selection is cleared and the clicked index becomes the solely selected item.
    /// Returns the target indices intended for batch operations.
    pub fn evaluate_context_menu(&mut self, clicked_index: usize) -> Vec<usize> {
        if self.slot_list.selected_indices.contains(&clicked_index) {
            self.slot_list.selected_indices.iter().copied().collect()
        } else {
            self.clear_multi_selection();
            self.slot_list.selected_indices.insert(clicked_index);
            self.slot_list.anchor_index = Some(clicked_index);
            vec![clicked_index]
        }
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
        self.active_filter = None;
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

    /// Resolve target indices for queue/batch operations (Shift+Q, etc.).
    ///
    /// If multi-selection is active, returns all selected indices (sorted).
    /// Otherwise, falls back to the effective center index.
    /// Returns an empty vec if there are no items.
    pub fn get_queue_target_indices(&self, total_items: usize) -> Vec<usize> {
        if !self.slot_list.selected_indices.is_empty() {
            self.slot_list.selected_indices.iter().copied().collect()
        } else if let Some(center_idx) = self.get_center_item_index(total_items) {
            vec![center_idx]
        } else {
            vec![]
        }
    }

    /// Resolve target indices for context menu batch operations.
    ///
    /// Evaluates the context menu click (preserving selection if the clicked item
    /// is within it, otherwise resetting to just the clicked item), then clears
    /// multi-selection state. Returns the resolved target indices.
    pub fn get_batch_target_indices(&mut self, clicked_idx: usize) -> Vec<usize> {
        let indices = self.evaluate_context_menu(clicked_idx);
        self.clear_multi_selection();
        indices
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

    // ══════════════════════════════════════════════════════════════════════
    //  Multi-Selection State Machine
    // ══════════════════════════════════════════════════════════════════════

    fn no_modifiers() -> iced::keyboard::Modifiers {
        iced::keyboard::Modifiers::empty()
    }

    fn ctrl() -> iced::keyboard::Modifiers {
        iced::keyboard::Modifiers::CTRL
    }

    fn shift() -> iced::keyboard::Modifiers {
        iced::keyboard::Modifiers::SHIFT
    }

    #[test]
    fn ctrl_click_adds_to_selection() {
        let mut state = SlotListPageState::default();
        state.handle_slot_click(3, 10, ctrl());

        assert!(state.slot_list.selected_indices.contains(&3));
        assert_eq!(state.slot_list.anchor_index, Some(3));
        assert_eq!(state.slot_list.selected_offset, Some(3));
    }

    #[test]
    fn ctrl_click_toggles_off() {
        let mut state = SlotListPageState::default();
        state.handle_slot_click(3, 10, ctrl()); // select
        state.handle_slot_click(3, 10, ctrl()); // deselect

        assert!(state.slot_list.selected_indices.is_empty());
        assert_eq!(state.slot_list.anchor_index, None);
    }

    #[test]
    fn ctrl_deselect_last_clears_selected_offset() {
        // Regression: bug ebd98d1 — selected_offset was not cleared when
        // the last item was ctrl-deselected, leaving a stale center highlight.
        let mut state = SlotListPageState::default();
        state.handle_slot_click(5, 10, ctrl()); // select
        state.handle_slot_click(5, 10, ctrl()); // deselect last

        assert!(state.slot_list.selected_indices.is_empty());
        assert_eq!(
            state.slot_list.selected_offset, None,
            "selected_offset must be None when selection set empties"
        );
    }

    #[test]
    fn ctrl_click_accumulates_non_contiguous() {
        let mut state = SlotListPageState::default();
        state.handle_slot_click(1, 10, ctrl());
        state.handle_slot_click(3, 10, ctrl());
        state.handle_slot_click(5, 10, ctrl());

        assert_eq!(state.slot_list.selected_indices.len(), 3);
        assert!(state.slot_list.selected_indices.contains(&1));
        assert!(state.slot_list.selected_indices.contains(&3));
        assert!(state.slot_list.selected_indices.contains(&5));
    }

    #[test]
    fn shift_click_selects_range_from_anchor() {
        let mut state = SlotListPageState::default();
        // Normal click sets anchor at 2
        state.handle_slot_click(2, 10, no_modifiers());
        // Shift+click extends range to 5
        state.handle_slot_click(5, 10, shift());

        assert_eq!(state.slot_list.selected_indices.len(), 4);
        for i in 2..=5 {
            assert!(
                state.slot_list.selected_indices.contains(&i),
                "index {i} should be in selection"
            );
        }
    }

    #[test]
    fn shift_click_without_anchor_behaves_as_normal() {
        let mut state = SlotListPageState::default();
        state.handle_slot_click(4, 10, shift());

        assert_eq!(state.slot_list.selected_indices.len(), 1);
        assert!(state.slot_list.selected_indices.contains(&4));
        assert_eq!(state.slot_list.anchor_index, Some(4));
    }

    #[test]
    fn normal_click_clears_multi_selection() {
        let mut state = SlotListPageState::default();
        // Build up a multi-selection
        state.handle_slot_click(1, 10, ctrl());
        state.handle_slot_click(3, 10, ctrl());
        state.handle_slot_click(5, 10, ctrl());
        assert_eq!(state.slot_list.selected_indices.len(), 3);

        // Normal click should clear and select only the clicked item
        state.handle_slot_click(2, 10, no_modifiers());
        assert_eq!(state.slot_list.selected_indices.len(), 1);
        assert!(state.slot_list.selected_indices.contains(&2));
        assert_eq!(state.slot_list.anchor_index, Some(2));
    }

    #[test]
    fn context_menu_outside_selection_resets_to_clicked() {
        let mut state = SlotListPageState::default();
        state.handle_slot_click(1, 10, ctrl());
        state.handle_slot_click(3, 10, ctrl());
        state.handle_slot_click(5, 10, ctrl());

        // Right-click on 7 (not in selection) → resets to just 7
        let targets = state.evaluate_context_menu(7);
        assert_eq!(targets, vec![7]);
        assert_eq!(state.slot_list.selected_indices.len(), 1);
        assert!(state.slot_list.selected_indices.contains(&7));
    }

    #[test]
    fn context_menu_inside_selection_returns_all() {
        let mut state = SlotListPageState::default();
        state.handle_slot_click(1, 10, ctrl());
        state.handle_slot_click(3, 10, ctrl());
        state.handle_slot_click(5, 10, ctrl());

        // Right-click on 3 (in selection) → returns all selected
        let mut targets = state.evaluate_context_menu(3);
        targets.sort();
        assert_eq!(targets, vec![1, 3, 5]);
        // Selection should be unchanged
        assert_eq!(state.slot_list.selected_indices.len(), 3);
    }

    #[test]
    fn out_of_bounds_click_is_noop() {
        let mut state = SlotListPageState::default();
        state.handle_slot_click(15, 10, no_modifiers()); // >= total_items

        assert!(state.slot_list.selected_indices.is_empty());
        assert_eq!(state.slot_list.selected_offset, None);
    }

    // ══════════════════════════════════════════════════════════════════════
    //  get_queue_target_indices
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn queue_target_uses_selection_when_present() {
        let mut state = SlotListPageState::default();
        state.handle_slot_click(2, 10, ctrl());
        state.handle_slot_click(5, 10, ctrl());
        state.handle_slot_click(8, 10, ctrl());

        let targets = state.get_queue_target_indices(10);
        assert_eq!(targets, vec![2, 5, 8]);
    }

    #[test]
    fn queue_target_falls_back_to_center() {
        let mut state = SlotListPageState::default();
        state.slot_list.set_offset(5, 20);

        let targets = state.get_queue_target_indices(20);
        assert_eq!(targets, vec![5]);
    }

    #[test]
    fn queue_target_returns_empty_when_no_items() {
        let state = SlotListPageState::default();
        let targets = state.get_queue_target_indices(0);
        assert!(targets.is_empty());
    }

    #[test]
    fn queue_target_uses_selected_offset_when_no_multi() {
        let mut state = SlotListPageState::default();
        state.slot_list.set_offset(3, 20);
        state.slot_list.set_selected(7, 20);

        let targets = state.get_queue_target_indices(20);
        // selected_offset=7 is used as effective center
        assert_eq!(targets, vec![7]);
    }

    // ══════════════════════════════════════════════════════════════════════
    //  get_batch_target_indices
    // ══════════════════════════════════════════════════════════════════════

    #[test]
    fn batch_target_within_selection_returns_all_and_clears() {
        let mut state = SlotListPageState::default();
        state.handle_slot_click(1, 10, ctrl());
        state.handle_slot_click(3, 10, ctrl());
        state.handle_slot_click(5, 10, ctrl());

        let mut targets = state.get_batch_target_indices(3);
        targets.sort();
        assert_eq!(targets, vec![1, 3, 5]);
        // Multi-selection should be cleared
        assert!(state.slot_list.selected_indices.is_empty());
        assert_eq!(state.slot_list.anchor_index, None);
    }

    #[test]
    fn batch_target_outside_selection_returns_clicked_and_clears() {
        let mut state = SlotListPageState::default();
        state.handle_slot_click(1, 10, ctrl());
        state.handle_slot_click(3, 10, ctrl());
        state.handle_slot_click(5, 10, ctrl());

        // Right-click on 7 (not in selection)
        let targets = state.get_batch_target_indices(7);
        assert_eq!(targets, vec![7]);
        // Multi-selection should be cleared
        assert!(state.slot_list.selected_indices.is_empty());
    }
}
