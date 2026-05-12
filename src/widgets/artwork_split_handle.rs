//! Drag handle for resizing the artwork panel.
//!
//! Single widget parameterized over [`Axis`] — `Horizontal` for the
//! right-hand artwork column (handle is a vertical strip dragged left/right)
//! and `Vertical` for the Always-Vertical stack (handle is a horizontal bar
//! dragged up/down). Both axes share the same drag bookkeeping, the same
//! [`DragEvent`] enum, and the same Change/Commit cadence:
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
    Color, Element, Length, Rectangle, Size,
    advanced::{
        Shell,
        layout::{self, Layout, Limits},
        renderer,
        widget::{Tree, Widget, tree},
    },
    event::Event,
    mouse,
};

/// Default visual thickness of the drag affordance (px). Applies to the
/// handle's *constrained* axis: width for [`Axis::Horizontal`], height for
/// [`Axis::Vertical`].
pub(crate) const HANDLE_THICKNESS: f32 = 6.0;

/// Drag orientation — which side of the artwork the handle sits on and which
/// cursor axis drives the drag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Axis {
    /// Handle is a vertical strip to the *left* of the artwork column;
    /// cursor x drives the drag. Cursor right → artwork shrinks → pct
    /// decreases, so `pct_sign() == -1.0`.
    Horizontal,
    /// Handle is a horizontal bar *below* the artwork; cursor y drives the
    /// drag. Cursor down → artwork grows → pct increases, so
    /// `pct_sign() == +1.0`.
    Vertical,
}

impl Axis {
    /// Sign applied to `dpct` when integrating cursor motion into the
    /// committed pct. Encapsulates the asymmetry between the two handle
    /// positions so callers don't have to remember which side gets the flip.
    fn pct_sign(self) -> f32 {
        match self {
            // Handle is *left of* the artwork: pushing the cursor right
            // (positive dx) shrinks the artwork (negative dpct).
            Self::Horizontal => -1.0,
            // Handle is *below* the artwork: pushing the cursor down
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

    /// Size in iced layout terms — `Length::Fill` along the long axis,
    /// `Length::Fixed(thickness)` across the constrained axis.
    fn size(self, thickness: f32) -> Size<Length> {
        match self {
            Self::Horizontal => Size {
                width: Length::Fixed(thickness),
                height: Length::Fill,
            },
            Self::Vertical => Size {
                width: Length::Fill,
                height: Length::Fixed(thickness),
            },
        }
    }

    /// Lay out the node — span the long axis fully from `limits`, fix the
    /// short axis to `thickness`.
    fn layout_size(self, limits_max: Size, thickness: f32) -> Size {
        match self {
            Self::Horizontal => Size::new(thickness, limits_max.height),
            Self::Vertical => Size::new(limits_max.width, thickness),
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
    /// Visual thickness in pixels (along the constrained axis).
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
        self.axis.size(self.thickness)
    }

    fn layout(&mut self, _tree: &mut Tree, _renderer: &Renderer, limits: &Limits) -> layout::Node {
        layout::Node::new(self.axis.layout_size(limits.max(), self.thickness))
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<HandleState>();
        let bounds = layout.bounds();

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(p) = cursor.position_over(bounds) {
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
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<HandleState>();
        let bounds = layout.bounds();
        let hovered = cursor.position_over(bounds).is_some();

        match (state, hovered) {
            (HandleState::Dragging { .. }, _) | (_, true) => self.axis.cursor_icon(),
            _ => mouse::Interaction::Idle,
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _defaults: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<HandleState>();
        let bounds = layout.bounds();

        // Active state — brighten while dragged or hovered.
        let dragging = matches!(state, HandleState::Dragging { .. });
        let hovered = cursor.position_over(bounds).is_some();
        let active = dragging || hovered;

        let bg: Color = if active {
            crate::theme::bg3()
        } else {
            crate::theme::bg1()
        };

        renderer.fill_quad(
            renderer::Quad {
                bounds,
                ..Default::default()
            },
            iced::Background::Color(bg),
        );
    }
}

impl<'a, Message: 'a, Theme, Renderer> From<ArtworkSplitHandle<'a, Message>>
    for Element<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn from(handle: ArtworkSplitHandle<'a, Message>) -> Self {
        Self::new(handle)
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
    .into()
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
    .into()
}
