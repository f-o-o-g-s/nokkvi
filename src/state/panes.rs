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
