//! Measured scroll-into-view for variable-height scrollable content.
//!
//! A custom iced widget [`Operation`] that reads the REAL laid-out bounds of a
//! target widget (tagged with an [`Id`]) and of its containing scrollable in a
//! single tree walk, then chains a `scroll_to` that centers the target in the
//! viewport. This replaces estimated per-row pixel heights: the settings detail
//! pane has variable-height rows (wrapped subtitles, value badges, color
//! swatches), so any fixed per-entry height estimate drifts cumulatively and
//! eventually walks the focused row out of view (undershoot on tall rows,
//! overshoot on short ones).
//!
//! The scrollable reports its frame and content bounds first, then traverses
//! its children in the same walk (and `button` / `column` / `container` all
//! forward `operation.traverse`), so both bounds land in one pass. Child
//! layouts are reported pre-translation, so the target's offset within the
//! content is `target.y - content.y` — independent of the current scroll
//! position.

use iced::{
    Rectangle, Task, Vector,
    advanced::widget::{Id, Operation, operate, operation},
    widget::scrollable::AbsoluteOffset,
};

/// Vertical absolute scroll offset that centers `target` within a scrollable
/// whose viewport is `frame` and whose content is `content`.
///
/// All three rectangles come from the same widget-operation layout pass, so
/// `target.y - content.y` is the target's offset from the content top
/// regardless of the current scroll translation. The result is clamped to the
/// scrollable's valid range so a target near the top stays pinned at `0` and
/// one near the bottom pins to the content end rather than over-scrolling.
fn center_offset_y(frame: Rectangle, content: Rectangle, target: Rectangle) -> f32 {
    let target_top = target.y - content.y;
    let centered = target_top + target.height / 2.0 - frame.height / 2.0;
    let max_offset = (content.height - frame.height).max(0.0);
    centered.clamp(0.0, max_offset)
}

/// One-walk operation: capture the scrollable's frame/content bounds and the
/// target widget's bounds, then center the target.
struct CenterInScrollable {
    scrollable_id: Id,
    target_id: Id,
    /// (viewport bounds, content bounds) of the matched scrollable.
    frame: Option<(Rectangle, Rectangle)>,
    /// Bounds of the matched target widget.
    target: Option<Rectangle>,
}

impl<T: 'static> Operation<T> for CenterInScrollable {
    fn traverse(&mut self, recurse: &mut dyn FnMut(&mut dyn Operation<T>)) {
        recurse(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        content_bounds: Rectangle,
        _translation: Vector,
        _state: &mut dyn operation::Scrollable,
    ) {
        if id == Some(&self.scrollable_id) {
            self.frame = Some((bounds, content_bounds));
        }
    }

    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        if id == Some(&self.target_id) {
            self.target = Some(bounds);
        }
    }

    fn finish(&self) -> operation::Outcome<T> {
        let (Some((frame, content)), Some(target)) = (self.frame, self.target) else {
            // Either id absent from the current tree (empty list, target not
            // rendered) — leave the scroll position untouched.
            return operation::Outcome::None;
        };

        let y = center_offset_y(frame, content, target);
        operation::Outcome::Chain(Box::new(operation::scrollable::scroll_to(
            self.scrollable_id.clone(),
            AbsoluteOffset {
                x: None,
                y: Some(y),
            },
        )))
    }
}

/// Build a [`Task`] that centers the widget tagged `target_id` within the
/// scrollable tagged `scrollable_id`, using real laid-out geometry.
///
/// No-op (leaves the scroll position untouched) if either id is absent from the
/// current widget tree. The returned task produces no message.
pub(crate) fn center_in_scrollable<T: Send + 'static>(
    scrollable_id: impl Into<Id>,
    target_id: impl Into<Id>,
) -> Task<T> {
    operate(CenterInScrollable {
        scrollable_id: scrollable_id.into(),
        target_id: target_id.into(),
        frame: None,
        target: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(y: f32, height: f32) -> Rectangle {
        Rectangle {
            x: 0.0,
            y,
            width: 100.0,
            height,
        }
    }

    #[test]
    fn centers_a_mid_list_row_in_the_viewport() {
        // Content starts at y=0, viewport is 600 tall. A 78px row whose top sits
        // 1000px into the content should center: 1000 + 39 - 300 = 739.
        let frame = rect(0.0, 600.0);
        let content = rect(0.0, 3000.0);
        let target = rect(1000.0, 78.0);
        assert_eq!(center_offset_y(frame, content, target), 739.0);
    }

    #[test]
    fn subtracts_content_origin_so_offset_is_scroll_independent() {
        // When the layout pass reports a non-zero content origin, the target's
        // content-space top is target.y - content.y, not target.y. Row top is
        // 1000px into content regardless of where the content rect sits.
        let frame = rect(50.0, 600.0);
        let content = rect(50.0, 3000.0);
        let target = rect(1050.0, 78.0);
        // 1000 + 39 - 300 = 739 — same as the origin-at-zero case.
        assert_eq!(center_offset_y(frame, content, target), 739.0);
    }

    #[test]
    fn clamps_to_zero_for_rows_near_the_top() {
        let frame = rect(0.0, 600.0);
        let content = rect(0.0, 3000.0);
        let target = rect(40.0, 60.0); // centering would want a negative offset
        assert_eq!(center_offset_y(frame, content, target), 0.0);
    }

    #[test]
    fn clamps_to_max_for_rows_near_the_bottom() {
        let frame = rect(0.0, 600.0);
        let content = rect(0.0, 3000.0);
        let target = rect(2960.0, 40.0); // last row
        // max scroll = 3000 - 600 = 2400; centering wants more, so clamp.
        assert_eq!(center_offset_y(frame, content, target), 2400.0);
    }

    #[test]
    fn returns_zero_when_content_fits_in_the_viewport() {
        let frame = rect(0.0, 600.0);
        let content = rect(0.0, 300.0); // shorter than the viewport
        let target = rect(200.0, 60.0);
        assert_eq!(center_offset_y(frame, content, target), 0.0);
    }
}
