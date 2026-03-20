use std::time::Instant;

/// Slot list view component for displaying items in a 9-slot circular navigation interface.
/// This is the signature UI pattern from the original QML/Slint implementations.
///
/// Slot list view state and configuration
#[derive(Debug, Clone)]
pub struct SlotListView {
    /// Current viewport offset (which item is centered at slot 4)
    pub viewport_offset: usize,
    /// When set, overrides which item gets "center" styling without moving the viewport.
    /// Used by click-to-focus to highlight a clicked item in-place.
    /// Cleared automatically by keyboard navigation (`move_up`/`move_down`).
    pub selected_offset: Option<usize>,
    /// Current slot count (set during render by `build_slot_list_slots`).
    /// Used by `slot_to_item_index` to translate drag slot indices to item indices.
    pub slot_count: usize,
    /// Timestamp of the last scroll event (for transient scrollbar fade animation).
    /// `None` means the scrollbar should be fully hidden.
    pub last_scrolled: Option<Instant>,
    /// Generation counter for scroll events (guards timer deduplication, like volume_change_id).
    pub scroll_generation_id: u64,
    /// Timestamp of the last "flash center" trigger.
    /// When set, the center slot's `HoverOverlay` shows the press animation.
    /// Auto-expires after the overlay's animation duration.
    pub flash_center_at: Option<Instant>,
}

impl SlotListView {
    pub fn new() -> Self {
        Self {
            viewport_offset: 0,
            selected_offset: None,
            slot_count: 9,
            last_scrolled: None,
            scroll_generation_id: 0,
            flash_center_at: None,
        }
    }

    /// Trigger a timed press animation on the center slot.
    /// Called by any action that "activates" the center item (play, Enter, MPRIS, player bar).
    pub fn flash_center(&mut self) {
        self.flash_center_at = Some(Instant::now());
    }

    /// Calculate which item index should be displayed in a given slot
    /// Uses the provided center_slot for dynamic slot count support.
    /// Returns None for out-of-range indices (no wrapping).
    pub fn get_slot_item_index_with_center(
        &self,
        slot_index: usize,
        total_items: usize,
        center_slot: usize,
    ) -> Option<usize> {
        if total_items == 0 {
            return None;
        }

        // Calculate the offset from center
        let offset_from_center = slot_index as i32 - center_slot as i32;
        let target_index = self.viewport_offset as i32 + offset_from_center;

        // Clamp: return None for out-of-range indices instead of wrapping
        if target_index < 0 || target_index >= total_items as i32 {
            return None;
        }

        Some(target_index as usize)
    }

    /// Translate a drag-column slot index to an absolute item index.
    ///
    /// Replicates the `effective_center` computation from `build_slot_list_slots`
    /// so the drag handler uses exactly the same slot→item mapping as rendering.
    pub fn slot_to_item_index(&self, slot_index: usize, total_items: usize) -> Option<usize> {
        let center_slot = self.slot_count / 2;
        let effective_center = if total_items < self.slot_count {
            let block_start = (self.slot_count.saturating_sub(total_items)) / 2;
            (block_start + self.viewport_offset).min(self.slot_count.saturating_sub(1))
        } else {
            let items_at_and_after = total_items.saturating_sub(self.viewport_offset);
            let end_push = self.slot_count.saturating_sub(items_at_and_after);
            center_slot.min(self.viewport_offset).max(end_push)
        };
        self.get_slot_item_index_with_center(slot_index, total_items, effective_center)
    }

    /// Like `slot_to_item_index` but allows the result to equal `total_items`.
    ///
    /// Drop targets use insert-before semantics, so `total_items` is a valid
    /// target meaning "after the last item". The regular `slot_to_item_index`
    /// rejects this because it's out of range for *rendering*, but it's valid
    /// for *dropping*.
    pub fn slot_to_item_index_for_drop(
        &self,
        slot_index: usize,
        total_items: usize,
    ) -> Option<usize> {
        let center_slot = self.slot_count / 2;
        let effective_center = if total_items < self.slot_count {
            let block_start = (self.slot_count.saturating_sub(total_items)) / 2;
            (block_start + self.viewport_offset).min(self.slot_count.saturating_sub(1))
        } else {
            let items_at_and_after = total_items.saturating_sub(self.viewport_offset);
            let end_push = self.slot_count.saturating_sub(items_at_and_after);
            center_slot.min(self.viewport_offset).max(end_push)
        };

        if total_items == 0 {
            return None;
        }

        let offset_from_center = slot_index as i32 - effective_center as i32;
        let target_index = self.viewport_offset as i32 + offset_from_center;

        // Allow target_index == total_items (insert-after-last), but reject
        // negative and anything beyond total_items.
        if target_index < 0 || target_index > total_items as i32 {
            return None;
        }

        Some(target_index as usize)
    }

    /// Calculate which item index should be displayed in a given slot
    /// Slot 4 is the center (for 9-slot layout), slots 0-3 are above, slots 5-8 are below
    pub fn get_slot_item_index(&self, slot_index: usize, total_items: usize) -> Option<usize> {
        self.get_slot_item_index_with_center(slot_index, total_items, 4)
    }

    /// Calculate opacity for a slot based on distance from a dynamic center
    pub fn calculate_slot_opacity_with_center(slot_index: usize, center_slot: usize) -> f32 {
        let distance = (slot_index as i32 - center_slot as i32).abs();
        let opacity = 1.0 - (distance as f32 * 0.2);
        opacity.max(0.2)
    }

    /// Calculate opacity for a slot based on distance from center (assumes center at slot 4)
    pub fn calculate_slot_opacity(slot_index: usize) -> f32 {
        Self::calculate_slot_opacity_with_center(slot_index, 4)
    }

    /// Move viewport up (decrease offset, clamped at 0).
    /// If `selected_offset` is set (click-to-focus), snaps the viewport there
    /// first so scrolling continues from the clicked item rather than the old
    /// viewport position.
    pub fn move_up(&mut self, total_items: usize) {
        if total_items == 0 {
            return;
        }
        if let Some(sel) = self.selected_offset.take() {
            self.viewport_offset = sel;
        }
        self.viewport_offset = self.viewport_offset.saturating_sub(1);
    }

    /// Move viewport down (increase offset, clamped at total_items - 1).
    /// If `selected_offset` is set (click-to-focus), snaps the viewport there
    /// first so scrolling continues from the clicked item rather than the old
    /// viewport position.
    pub fn move_down(&mut self, total_items: usize) {
        if total_items == 0 {
            return;
        }
        if let Some(sel) = self.selected_offset.take() {
            self.viewport_offset = sel;
        }
        self.viewport_offset = (self.viewport_offset + 1).min(total_items - 1);
    }

    /// Set viewport to specific offset (clears selected_offset)
    pub fn set_offset(&mut self, offset: usize, total_items: usize) {
        if total_items > 0 && offset < total_items {
            self.viewport_offset = offset;
            self.selected_offset = None;
        }
    }

    /// Set the selected item without moving the viewport.
    /// The selected item will receive "center" styling in the slot list render.
    pub fn set_selected(&mut self, offset: usize, total_items: usize) {
        if total_items > 0 && offset < total_items {
            self.selected_offset = Some(offset);
        }
    }

    /// Record a scroll event for transient scrollbar animation.
    /// Sets `last_scrolled` to now and increments `scroll_generation_id`.
    pub fn record_scroll(&mut self) {
        self.last_scrolled = Some(Instant::now());
        self.scroll_generation_id = self.scroll_generation_id.wrapping_add(1);
    }

    /// Compute the current scrollbar opacity based on elapsed time since last scroll.
    ///
    /// Returns 0.0 if no scroll has occurred. Otherwise:
    /// - 0.0–1.0s elapsed: full opacity (1.0)
    /// - 1.0–1.5s elapsed: linear fade from 1.0 → 0.0
    /// - >1.5s elapsed: fully hidden (0.0)
    pub fn scrollbar_opacity(&self) -> f32 {
        let Some(last) = self.last_scrolled else {
            return 0.0;
        };
        let elapsed = last.elapsed().as_secs_f32();
        const HOLD_DURATION: f32 = 1.0;
        const FADE_DURATION: f32 = 0.5;
        if elapsed < HOLD_DURATION {
            1.0
        } else {
            let fade_progress = (elapsed - HOLD_DURATION) / FADE_DURATION;
            (1.0 - fade_progress).clamp(0.0, 1.0)
        }
    }

    /// Get the center item index using the current dynamic slot count.
    pub fn get_center_item_index(&self, total_items: usize) -> Option<usize> {
        let center_slot = self.slot_count / 2;
        self.get_slot_item_index_with_center(center_slot, total_items, center_slot)
    }

    /// Get the effective center item index — returns `selected_offset` if set,
    /// otherwise falls back to the viewport center.
    pub fn get_effective_center_index(&self, total_items: usize) -> Option<usize> {
        self.selected_offset
            .filter(|&s| s < total_items)
            .or_else(|| self.get_center_item_index(total_items))
    }

    /// Minimum extra items beyond the visible viewport to prefetch.
    /// Ensures responsive scrolling even on small windows.
    const MIN_PREFETCH_BUFFER: i32 = 3;

    /// Get indices to prefetch around the current viewport.
    ///
    /// Derives the radius from `self.slot_count` so the prefetch window always
    /// covers every visible slot plus a small buffer for smooth scrolling.
    ///
    /// Uses the full `slot_count` (not half) because at list edges
    /// (viewport_offset near 0 or total_items), all visible slots land on
    /// one side of center, requiring radius >= slot_count - 1.
    ///
    /// This is the canonical definition of "what's near the viewport" —
    /// callers should not duplicate this logic.
    pub fn prefetch_indices(&self, total_items: usize) -> impl Iterator<Item = usize> {
        let radius = (self.slot_count as i32) + Self::MIN_PREFETCH_BUFFER;
        self.indices_to_prefetch(total_items, radius)
    }

    /// Get indices to prefetch around the current viewport with custom radius.
    ///
    /// Returns indices within [-radius, +radius] of the viewport center,
    /// clamped to valid range (no wrapping).
    ///
    /// # Arguments
    /// * `total_items` - Total number of items in the list
    /// * `radius` - Number of items on each side of center to include
    pub fn indices_to_prefetch(
        &self,
        total_items: usize,
        radius: i32,
    ) -> impl Iterator<Item = usize> {
        let center = self.viewport_offset as i32;
        let max = total_items as i32;

        (-radius..=radius).filter_map(move |offset| {
            let idx = center + offset;
            if max == 0 || idx < 0 || idx >= max {
                None
            } else {
                Some(idx as usize)
            }
        })
    }
}

impl Default for SlotListView {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clamped_indexing() {
        let sl = SlotListView::new();

        // Test with 5 items, offset at 0 (9-slot layout, center=4)
        assert_eq!(sl.get_slot_item_index(0, 5), None); // slot 0 = offset-4 = -4 -> out of range
        assert_eq!(sl.get_slot_item_index(4, 5), Some(0)); // center slot = current offset
        assert_eq!(sl.get_slot_item_index(8, 5), Some(4)); // slot 8 = offset+4 = 4 -> valid
        assert_eq!(sl.get_slot_item_index(3, 5), None); // slot 3 = offset-1 = -1 -> out of range
        assert_eq!(sl.get_slot_item_index(5, 5), Some(1)); // slot 5 = offset+1 = 1 -> valid
    }

    #[test]
    fn test_opacity_gradient() {
        // Test with default 9-slot layout (center at 4)
        assert_eq!(SlotListView::calculate_slot_opacity(4), 1.0); // center
        assert_eq!(SlotListView::calculate_slot_opacity(3), 0.8); // one away
        assert_eq!(SlotListView::calculate_slot_opacity(5), 0.8); // one away
        assert_eq!(SlotListView::calculate_slot_opacity(0), 0.2); // four away (clamped)
    }

    #[test]
    fn test_opacity_gradient_dynamic_center() {
        // Test with 5-slot layout (center at 2)
        assert_eq!(SlotListView::calculate_slot_opacity_with_center(2, 2), 1.0); // center
        assert_eq!(SlotListView::calculate_slot_opacity_with_center(1, 2), 0.8); // one away
        assert_eq!(SlotListView::calculate_slot_opacity_with_center(3, 2), 0.8); // one away
        assert_eq!(SlotListView::calculate_slot_opacity_with_center(0, 2), 0.6); // two away

        // Test with 3-slot layout (center at 1)
        assert_eq!(SlotListView::calculate_slot_opacity_with_center(1, 1), 1.0); // center
        assert_eq!(SlotListView::calculate_slot_opacity_with_center(0, 1), 0.8); // one away
        assert_eq!(SlotListView::calculate_slot_opacity_with_center(2, 1), 0.8);
        // one away
    }

    #[test]
    fn test_navigation() {
        let mut sl = SlotListView::new();

        sl.move_down(10);
        assert_eq!(sl.viewport_offset, 1);

        sl.move_up(10);
        assert_eq!(sl.viewport_offset, 0);

        sl.move_up(10); // Should clamp at 0 (no wrapping)
        assert_eq!(sl.viewport_offset, 0);

        // Test clamping at the end
        sl.set_offset(9, 10);
        sl.move_down(10); // Should clamp at 9 (last item)
        assert_eq!(sl.viewport_offset, 9);
    }

    #[test]
    fn test_selected_offset_set_and_clear() {
        let mut sl = SlotListView::new();
        assert_eq!(sl.selected_offset, None);

        // set_selected sets selected_offset without touching viewport_offset
        sl.set_offset(5, 20);
        sl.set_selected(8, 20);
        assert_eq!(sl.viewport_offset, 5);
        assert_eq!(sl.selected_offset, Some(8));

        // move_up snaps viewport to selected (8), then decrements → 7
        sl.move_up(20);
        assert_eq!(sl.selected_offset, None);
        assert_eq!(sl.viewport_offset, 7);

        // Re-set and verify move_down snaps then increments → 11
        sl.set_selected(10, 20);
        assert_eq!(sl.selected_offset, Some(10));
        sl.move_down(20);
        assert_eq!(sl.selected_offset, None);
        assert_eq!(sl.viewport_offset, 11);

        // Re-set and verify set_offset clears it
        sl.set_selected(10, 20);
        sl.set_offset(2, 20);
        assert_eq!(sl.selected_offset, None);
        assert_eq!(sl.viewport_offset, 2);
    }

    #[test]
    fn test_effective_center_index() {
        let mut sl = SlotListView::new();

        // Without selected_offset, returns viewport center
        sl.set_offset(5, 20);
        assert_eq!(sl.get_effective_center_index(20), Some(5));

        // With selected_offset, returns it instead
        sl.set_selected(12, 20);
        assert_eq!(sl.get_effective_center_index(20), Some(12));

        // viewport_offset is still 5
        assert_eq!(sl.get_center_item_index(20), Some(5));

        // Out-of-range selected_offset falls back to center
        sl.selected_offset = Some(25); // beyond total_items
        assert_eq!(sl.get_effective_center_index(20), Some(5));
    }

    #[test]
    fn test_set_selected_bounds() {
        let mut sl = SlotListView::new();

        // Should not set selected_offset beyond total_items
        sl.set_selected(10, 5);
        assert_eq!(sl.selected_offset, None);

        // Should not set on empty list
        sl.set_selected(0, 0);
        assert_eq!(sl.selected_offset, None);

        // Valid set
        sl.set_selected(4, 5);
        assert_eq!(sl.selected_offset, Some(4));
    }

    #[test]
    fn test_prefetch_covers_all_visible_slots() {
        // For any slot_count, prefetch_indices must cover everything
        // the viewport can show (slot_count/2 in each direction from center).
        // Test mid-list, start-of-list, and end-of-list (the original bug was at edges).
        let total_items = 500;
        for slot_count in [1, 3, 5, 9, 11, 15, 21, 29] {
            for viewport_offset in [0, total_items / 2, total_items - 1] {
                let mut sl = SlotListView::new();
                sl.slot_count = slot_count;
                sl.viewport_offset = viewport_offset;

                let indices: Vec<usize> = sl.prefetch_indices(total_items).collect();
                let half = slot_count / 2;
                // Every slot visible from center must be in the prefetch set
                for offset in 0..=half {
                    let expected = viewport_offset + offset;
                    if expected < total_items {
                        assert!(
                            indices.contains(&expected),
                            "slot_count={slot_count}, vp={viewport_offset}: missing index \
                             {expected} (offset +{offset})"
                        );
                    }
                    if let Some(expected_neg) = viewport_offset.checked_sub(offset) {
                        assert!(
                            indices.contains(&expected_neg),
                            "slot_count={slot_count}, vp={viewport_offset}: missing index \
                             {expected_neg} (offset -{offset})"
                        );
                    }
                }
            }
        }
    }
}
