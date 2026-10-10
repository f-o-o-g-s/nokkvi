//! Drag handle for resizing the artwork panel.
//!
//! Single widget parameterized over [`Axis`] — `Horizontal` for the
//! right-hand artwork column (dragged left/right by its left edge) and
//! `Vertical` for the Always-Vertical stack (dragged up/down by its bottom
//! edge). The handle is invisible and takes no room: it is a `Stack` layer
//! over the artwork panel that grabs only a `HANDLE_THICKNESS` strip along
//! the edge facing the list, and the resize cursor is its only affordance.
//! Both axes share the same drag bookkeeping, the same [`DragEvent`] enum,
//! and the same Change/Commit cadence:
//!
//! - `on_change(pct)` fires on every `CursorMoved` during a drag (live
//!   preview — update the theme atomic, do not persist yet).
//! - `on_commit(pct)` fires once on `ButtonReleased` (persist to TOML).
//!
//! Convenience constructors [`artwork_split_handle_horizontal_element`] and
//! [`artwork_split_handle_vertical_element`] read the appropriate theme
//! atomic (`artwork_column_width_pct` vs `artwork_vertical_height_pct`) so
//! the drag is anchored to the displayed extent, not a stale snapshot.
//!
//! There is no click-vs-drag threshold — the handle has no click action, so
//! every press-drag-release is treated as a resize gesture.
//!
//! State lives in the widget tree (`HandleState`) so the handle widget can be
//! recreated freely on every render without losing the in-flight drag.

use iced::{
    Element, Length, Rectangle, Size, Widget as _,
    advanced::{
        Shell,
        layout::{Layout, Limits},
        renderer,
        widget::{Tree, Widget, tree},
    },
    event::Event,
    mouse,
};

/// Thickness (px) of the invisible grab strip along the artwork's edge: its
/// width over the column's left edge ([`Axis::Horizontal`]), its height over
/// the stacked artwork's bottom edge ([`Axis::Vertical`]).
const HANDLE_THICKNESS: f32 = 6.0;

/// Drag orientation — which side of the artwork the handle sits on and which
/// cursor axis drives the drag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Axis {
    /// Grab strip along the artwork column's *left* edge; cursor x drives
    /// the drag. Cursor right → artwork shrinks → pct decreases, so
    /// `pct_sign() == -1.0`.
    Horizontal,
    /// Grab strip along the stacked artwork's *bottom* edge; cursor y drives
    /// the drag. Cursor down → artwork grows → pct increases, so
    /// `pct_sign() == +1.0`.
    Vertical,
}

impl Axis {
    /// Sign applied to `dpct` when integrating cursor motion into the
    /// committed pct. Encapsulates the asymmetry between the two handle
    /// positions so callers don't have to remember which side gets the flip.
    fn pct_sign(self) -> f32 {
        match self {
            // Strip on the artwork's *left* edge: pushing the cursor right
            // (positive dx) shrinks the artwork (negative dpct).
            Self::Horizontal => -1.0,
            // Strip on the artwork's *bottom* edge: pushing the cursor down
            // (positive dy) grows the artwork (positive dpct).
            Self::Vertical => 1.0,
        }
    }

    /// Default minimum pct floor for each axis. Horizontal can shrink to a
    /// sliver (0.05); vertical needs a taller floor (0.10) since artwork at
    /// 5% of window height is unreadably small.
    fn default_min_pct(self) -> f32 {
        match self {
            Self::Horizontal => 0.05,
            Self::Vertical => 0.10,
        }
    }

    /// Iced [`mouse::Interaction`] cursor shown when hovered or dragging.
    fn cursor_icon(self) -> mouse::Interaction {
        match self {
            Self::Horizontal => mouse::Interaction::ResizingHorizontally,
            Self::Vertical => mouse::Interaction::ResizingVertically,
        }
    }

    /// The grab strip inside the handle's `bounds` (the whole artwork
    /// panel): the `thickness`-px band along the edge that faces the list.
    fn grip(self, bounds: Rectangle, thickness: f32) -> Rectangle {
        match self {
            Self::Horizontal => Rectangle {
                width: thickness.min(bounds.width),
                ..bounds
            },
            Self::Vertical => {
                let height = thickness.min(bounds.height);
                Rectangle {
                    y: bounds.y + bounds.height - height,
                    height,
                    ..bounds
                }
            }
        }
    }
}

/// Single user-facing event the handle emits as the drag progresses.
///
/// `Change` fires on every cursor movement during the drag (live preview).
/// `Commit` fires once on release (persist to TOML).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DragEvent {
    Change(f32),
    Commit(f32),
}

/// Internal drag state stored in the widget tree.
#[derive(Debug, Clone, Copy, Default)]
enum HandleState {
    #[default]
    Idle,
    Dragging {
        /// Cursor position on the drag axis when the gesture started
        /// (x for horizontal, y for vertical).
        start_cursor: f32,
        /// Extent fraction when the gesture started.
        start_pct: f32,
    },
}

/// Visual + behavioral configuration for the handle.
pub(crate) struct ArtworkSplitHandle<'a, Message> {
    axis: Axis,
    /// Window extent along the drag axis (width for horizontal, height for
    /// vertical) — used to convert px deltas into pct deltas.
    window_extent: f32,
    /// Current extent fraction (used as the gesture anchor).
    current_pct: f32,
    /// Pct emitted on every CursorMoved during a drag (live preview).
    on_change: Box<dyn Fn(f32) -> Message + 'a>,
    /// Pct emitted on ButtonReleased after a drag (commit + persist).
    on_commit: Box<dyn Fn(f32) -> Message + 'a>,
    /// Inclusive lower bound for the published pct.
    min_pct: f32,
    /// Inclusive upper bound for the published pct.
    max_pct: f32,
    /// Grab-strip thickness in pixels (across the edge it lies on).
    thickness: f32,
}

impl<'a, Message> ArtworkSplitHandle<'a, Message> {
    pub(crate) fn new(
        axis: Axis,
        window_extent: f32,
        current_pct: f32,
        on_change: impl Fn(f32) -> Message + 'a,
        on_commit: impl Fn(f32) -> Message + 'a,
    ) -> Self {
        Self {
            axis,
            window_extent,
            current_pct,
            on_change: Box::new(on_change),
            on_commit: Box::new(on_commit),
            min_pct: axis.default_min_pct(),
            max_pct: 0.80,
            thickness: HANDLE_THICKNESS,
        }
    }

    /// Compute the pct that corresponds to the cursor's current position on
    /// the drag axis. The `Axis::pct_sign()` factor encodes whether cursor
    /// motion grows or shrinks the artwork.
    fn pct_from_cursor(&self, cursor: f32, start_cursor: f32, start_pct: f32) -> f32 {
        if self.window_extent <= 0.0 {
            return start_pct;
        }
        let delta = cursor - start_cursor;
        let dpct = (delta / self.window_extent) * self.axis.pct_sign();
        (start_pct + dpct).clamp(self.min_pct, self.max_pct)
    }
}

impl<Message> iced::advanced::widget::Meta for ArtworkSplitHandle<'_, Message> {}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for ArtworkSplitHandle<'_, Message>
where
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<HandleState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(HandleState::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Fill,
        }
    }

    /// Covers the whole layer (the artwork panel) so the grab strip can sit
    /// on whichever edge [`Axis::grip`] picks.
    fn layout(&mut self, tree: &mut Tree, _renderer: &Renderer, limits: &Limits) {
        tree.size = limits.bounds();
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<HandleState>();
        let grip = self.axis.grip(layout.bounds(), self.thickness);

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(p) = cursor.position_over(grip) {
                    let start_cursor = match self.axis {
                        Axis::Horizontal => p.x,
                        Axis::Vertical => p.y,
                    };
                    *state = HandleState::Dragging {
                        start_cursor,
                        start_pct: self.current_pct,
                    };
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let HandleState::Dragging {
                    start_cursor,
                    start_pct,
                } = *state
                    && let Some(p) = cursor.position()
                {
                    let cursor_axis = match self.axis {
                        Axis::Horizontal => p.x,
                        Axis::Vertical => p.y,
                    };
                    let new_pct = self.pct_from_cursor(cursor_axis, start_cursor, start_pct);
                    shell.publish((self.on_change)(new_pct));
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if let HandleState::Dragging {
                    start_cursor,
                    start_pct,
                } = *state
                {
                    let final_pct = cursor.position().map_or(start_pct, |p| {
                        let cursor_axis = match self.axis {
                            Axis::Horizontal => p.x,
                            Axis::Vertical => p.y,
                        };
                        self.pct_from_cursor(cursor_axis, start_cursor, start_pct)
                    });
                    *state = HandleState::Idle;
                    shell.publish((self.on_commit)(final_pct));
                    shell.capture_event();
                }
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<HandleState>();
        let hovered = cursor
            .position_over(self.axis.grip(layout.bounds(), self.thickness))
            .is_some();

        // `None` (not `Idle`) off the grab strip: the handle is a `Stack`
        // layer over the artwork panel, and a `Stack` reports its topmost
        // non-`None` layer (and hides the cursor from the layers under a
        // non-`None` one), so `Idle` would mask the panel's own cursor and
        // hover (the Theater corner button).
        match (state, hovered) {
            (HandleState::Dragging { .. }, _) | (_, true) => self.axis.cursor_icon(),
            _ => mouse::Interaction::None,
        }
    }

    /// Draws nothing: the resize cursor is the handle's only affordance.
    fn draw(
        &self,
        _tree: &Tree,
        _renderer: &mut Renderer,
        _theme: &Theme,
        _defaults: &renderer::Style,
        _layout: Layout,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
    }
}

/// Convenience constructor for the right-hand horizontal artwork column.
/// Reads `theme::artwork_column_width_pct()` so the drag is anchored to the
/// displayed width fraction.
pub(crate) fn artwork_split_handle_horizontal_element<'a, M, F>(
    window_width: f32,
    on_drag: F,
) -> Element<'a, M>
where
    M: 'a,
    F: Fn(DragEvent) -> M + Clone + 'a,
{
    let on_change = on_drag.clone();
    let on_commit = on_drag;
    ArtworkSplitHandle::new(
        Axis::Horizontal,
        window_width,
        crate::theme::artwork_column_width_pct(),
        move |pct| on_change(DragEvent::Change(pct)),
        move |pct| on_commit(DragEvent::Commit(pct)),
    )
    .boxed()
}

/// Convenience constructor for the Always-Vertical artwork stack. Reads
/// `theme::artwork_vertical_height_pct()` so the drag is anchored to the
/// displayed height fraction.
pub(crate) fn artwork_split_handle_vertical_element<'a, M, F>(
    window_height: f32,
    on_drag: F,
) -> Element<'a, M>
where
    M: 'a,
    F: Fn(DragEvent) -> M + Clone + 'a,
{
    let on_change = on_drag.clone();
    let on_commit = on_drag;
    ArtworkSplitHandle::new(
        Axis::Vertical,
        window_height,
        crate::theme::artwork_vertical_height_pct(),
        move |pct| on_change(DragEvent::Change(pct)),
        move |pct| on_commit(DragEvent::Commit(pct)),
    )
    .boxed()
}

#[cfg(test)]
mod tests {
    use iced::{Point, advanced::Layout};

    use super::*;

    /// An artwork panel laid out away from the origin, so a grip computed
    /// against the window instead of the panel shows up.
    const PANEL: Rectangle = Rectangle {
        x: 100.0,
        y: 50.0,
        width: 400.0,
        height: 300.0,
    };

    fn handle(axis: Axis) -> ArtworkSplitHandle<'static, ()> {
        ArtworkSplitHandle::new(axis, 1000.0, 0.4, |_| (), |_| ())
    }

    /// The cursor the handle reports with the pointer at `at` over [`PANEL`].
    fn interaction_at(axis: Axis, at: Point) -> mouse::Interaction {
        let handle = handle(axis);
        let tree = Tree::new::<(), (), ()>(&handle);
        let layout = Layout::new(PANEL.size()).move_to(PANEL.position());
        Widget::<(), (), ()>::mouse_interaction(
            &handle,
            &tree,
            layout,
            mouse::Cursor::Available(at),
            &PANEL,
            &(),
        )
    }

    #[test]
    fn horizontal_grip_is_the_panels_left_edge() {
        assert_eq!(
            Axis::Horizontal.grip(PANEL, HANDLE_THICKNESS),
            Rectangle {
                width: HANDLE_THICKNESS,
                ..PANEL
            }
        );
    }

    #[test]
    fn vertical_grip_is_the_panels_bottom_edge() {
        assert_eq!(
            Axis::Vertical.grip(PANEL, HANDLE_THICKNESS),
            Rectangle {
                y: PANEL.y + PANEL.height - HANDLE_THICKNESS,
                height: HANDLE_THICKNESS,
                ..PANEL
            }
        );
    }

    #[test]
    fn grip_stays_inside_a_panel_thinner_than_it() {
        let sliver = Rectangle {
            width: 2.0,
            height: 2.0,
            ..PANEL
        };
        assert_eq!(Axis::Horizontal.grip(sliver, HANDLE_THICKNESS), sliver);
        assert_eq!(Axis::Vertical.grip(sliver, HANDLE_THICKNESS), sliver);
    }

    /// Off the grip the handle must report `None`: it is the top `Stack`
    /// layer over the whole panel, and anything else would mask the panel's
    /// own cursor and hide the pointer from the panel's hover.
    #[test]
    fn handle_shows_the_resize_cursor_on_its_edge_only() {
        let center = PANEL.center();
        let left_edge = Point::new(PANEL.x + 1.0, center.y);
        let bottom_edge = Point::new(center.x, PANEL.y + PANEL.height - 1.0);

        assert_eq!(
            interaction_at(Axis::Horizontal, left_edge),
            mouse::Interaction::ResizingHorizontally
        );
        assert_eq!(
            interaction_at(Axis::Horizontal, center),
            mouse::Interaction::None
        );
        assert_eq!(
            interaction_at(Axis::Horizontal, bottom_edge),
            mouse::Interaction::None
        );

        assert_eq!(
            interaction_at(Axis::Vertical, bottom_edge),
            mouse::Interaction::ResizingVertically
        );
        assert_eq!(
            interaction_at(Axis::Vertical, center),
            mouse::Interaction::None
        );
        assert_eq!(
            interaction_at(Axis::Vertical, left_edge),
            mouse::Interaction::None
        );
    }
}
