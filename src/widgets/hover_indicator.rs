//! Shared hover-aware indicator canvas program.
//!
//! A generic canvas program that draws a filled rectangle when:
//!   - `indicator_color` is set (active state — always visible), OR
//!   - The cursor is within an optionally expanded detection area and
//!     `hover_indicator_color` is set.
//!
//! Used by both the horizontal nav bar (underline indicator) and the
//! vertical side nav bar (right-edge indicator / rotated label indicator).

use iced::{Color, Point, Rectangle, widget::canvas};

/// Directional hover area expansion (pixels).
///
/// Each field expands the cursor detection rectangle *beyond* the canvas bounds
/// in the corresponding direction, allowing hover effects to trigger even when
/// the cursor is over adjacent elements (e.g., the button above an underline).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct HoverExpand {
    pub up: f32,
    pub down: f32,
    pub left: f32,
    pub right: f32,
}

impl HoverExpand {
    pub(crate) const fn left(value: f32) -> Self {
        Self {
            up: 0.0,
            down: 0.0,
            left: value,
            right: 0.0,
        }
    }

    pub(crate) const fn up(value: f32) -> Self {
        Self {
            up: value,
            down: 0.0,
            left: 0.0,
            right: 0.0,
        }
    }
}

/// Canvas program for hover-aware indicator bars.
///
/// Draws a solid filled rectangle when active or hovered.
/// The hover detection area can be expanded in any direction to cover
/// adjacent elements that don't directly contain the indicator.
pub(crate) struct HoverIndicator {
    /// Active indicator color (always shown when `Some`)
    pub indicator_color: Option<Color>,
    /// Hover indicator color (shown on mouse-over when not active)
    pub hover_indicator_color: Option<Color>,
    /// Expand the cursor detection area beyond the canvas bounds
    pub expand: HoverExpand,
}

impl<Message> canvas::Program<Message> for HoverIndicator {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let show_indicator = self.indicator_color.or_else(|| {
            let hover_area = Rectangle {
                x: bounds.x - self.expand.left,
                y: bounds.y - self.expand.up,
                width: bounds.width + self.expand.left + self.expand.right,
                height: bounds.height + self.expand.up + self.expand.down,
            };
            if cursor.position().is_some_and(|p| hover_area.contains(p)) {
                self.hover_indicator_color
            } else {
                None
            }
        });

        let mut frame = canvas::Frame::new(renderer, bounds.size());
        if let Some(accent) = show_indicator {
            frame.fill_rectangle(Point::ORIGIN, bounds.size(), canvas::Fill::from(accent));
        }
        vec![frame.into_geometry()]
    }
}
