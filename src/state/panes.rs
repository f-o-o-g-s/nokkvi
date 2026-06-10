//! Pane focus and cross-pane drag state for the split-view browsing panel.

/// Which pane has keyboard focus during playlist edit mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaneFocus {
    #[default]
    Queue,
    Browser,
}

/// Active cross-pane drag state (tracked at app level since DragColumn
/// can't span across separate widget trees).
///
/// No payload is needed — on drop we dispatch the active browsing view's
/// `AddCenterToQueue` message, which resolves the item internally.
#[derive(Debug, Clone)]
pub struct CrossPaneDragState {
    /// Where the drag started — used to draw offset from origin.
    pub origin: iced::Point,
    /// Current cursor position — updated via mouse event subscription.
    pub cursor: iced::Point,
    /// Snapshotted center item index at drag activation time.
    /// This is read from the browsing view's effective center when the drag
    /// threshold is exceeded, so the preview is decoupled from subsequent
    /// state changes (e.g., `selected_offset` being cleared by scrolling).
    pub center_index: Option<usize>,
    /// Number of items in this drag. 1 = single item, >1 = batch from multi-selection.
    /// When >1, `handle_cross_pane_drag_released` skips `set_selected()` and lets
    /// `AddCenterToQueue` read the existing `selected_indices` on the slot list.
    pub selection_count: usize,
}

/// App-level cross-pane drag UI cluster: the active drag plus the press
/// tracking that arms it and the drop position it leaves behind. Groups
/// the six formerly-loose `Nokkvi` fields so the press → threshold →
/// release/cancel state machine mutates one substruct.
#[derive(Debug, Clone)]
pub struct CrossPaneDragUi {
    /// Active cross-pane drag from browsing panel to queue (None when idle).
    pub active: Option<CrossPaneDragState>,
    /// Last known cursor position (tracked via event subscription when panel
    /// is open).
    pub last_cursor_position: iced::Point,
    /// Press origin for cross-pane drag threshold detection (cleared on
    /// release).
    pub press_origin: Option<iced::Point>,
    /// Center item index snapshotted at press time (before drag activation).
    /// Captured right after the button's SlotListSetOffset processes (widget
    /// messages run before subscription messages in Iced), so selected_offset
    /// is guaranteed accurate. Immune to auto-follow or viewport mutations.
    pub pressed_item: Option<usize>,
    /// Snapshotted selection count at press time. 1 = single item, >1 = batch.
    pub selection_count: usize,
    /// Pending queue insertion position for cross-pane drag drop.
    /// Set by `handle_cross_pane_drag_released` before dispatching the
    /// `AddCenterToQueue` message; consumed by the update handler to
    /// insert at position instead of appending.
    pub pending_queue_insert_position: Option<usize>,
}

/// Manual impl (not derived): `selection_count` must idle at 1 — a derived
/// `Default` would silently zero it and break batch-drag resolution.
impl Default for CrossPaneDragUi {
    fn default() -> Self {
        Self {
            active: None,
            last_cursor_position: iced::Point::ORIGIN,
            press_origin: None,
            pressed_item: None,
            selection_count: 1,
            pending_queue_insert_position: None,
        }
    }
}

impl CrossPaneDragUi {
    /// Reset the press-tracking trio (origin, pressed item, selection count)
    /// back to idle. Intentionally leaves `active` and
    /// `pending_queue_insert_position` untouched — release clears press state
    /// before consuming the drag, and cancel must not drop a pending insert
    /// position that a dispatched `AddCenterToQueue` is about to consume.
    pub fn clear_press_tracking(&mut self) {
        self.press_origin = None;
        self.pressed_item = None;
        self.selection_count = 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drift guard: the manual `Default` must keep `selection_count` at 1
    /// (a derive would zero it and break batch-drag logic).
    #[test]
    fn cross_pane_drag_ui_default_selection_count_is_one() {
        assert_eq!(CrossPaneDragUi::default().selection_count, 1);
    }

    /// `clear_press_tracking` resets only the press trio — `active` and
    /// `pending_queue_insert_position` survive (cancel relies on this).
    #[test]
    fn clear_press_tracking_leaves_active_and_insert_position_untouched() {
        let mut ui = CrossPaneDragUi {
            active: Some(CrossPaneDragState {
                origin: iced::Point::ORIGIN,
                cursor: iced::Point::ORIGIN,
                center_index: Some(3),
                selection_count: 2,
            }),
            last_cursor_position: iced::Point::new(10.0, 20.0),
            press_origin: Some(iced::Point::new(1.0, 2.0)),
            pressed_item: Some(7),
            selection_count: 4,
            pending_queue_insert_position: Some(5),
        };

        ui.clear_press_tracking();

        assert!(ui.press_origin.is_none());
        assert!(ui.pressed_item.is_none());
        assert_eq!(ui.selection_count, 1);
        assert!(ui.active.is_some(), "active drag must survive");
        assert_eq!(
            ui.pending_queue_insert_position,
            Some(5),
            "pending insert position must survive"
        );
    }
}
