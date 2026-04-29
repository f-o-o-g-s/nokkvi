//! Vertical drag handle for resizing the artwork column.
//!
//! Drop-in widget rendered between the slot list and the artwork column when
//! `ArtworkColumnMode != Auto`. Drag it left/right to resize the artwork
//! column; the new width fraction is published via callbacks:
//!
//! - `on_change(pct)` fires every `CursorMoved` during a drag so the UI can
//!   update its atomic for live preview.
//! - `on_commit(pct)` fires once on `ButtonReleased` so persistence happens
//!   only at the end of the gesture.
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

/// Default visual width of the drag affordance.
pub(crate) const HANDLE_WIDTH: f32 = 6.0;

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
        /// Cursor x-position when the gesture started.
        start_cursor_x: f32,
        /// Column width fraction when the gesture started.
        start_pct: f32,
    },
}

/// Visual + behavioral configuration for the handle.
pub(crate) struct ArtworkSplitHandle<'a, Message> {
    /// Window width — used to convert px deltas into pct deltas.
    window_width: f32,
    /// Current column width fraction (used as the gesture anchor).
    current_pct: f32,
    /// Pct emitted on every CursorMoved during a drag (live preview).
    on_change: Box<dyn Fn(f32) -> Message + 'a>,
    /// Pct emitted on ButtonReleased after a drag (commit + persist).
    on_commit: Box<dyn Fn(f32) -> Message + 'a>,
    /// Inclusive lower bound for the published pct.
    min_pct: f32,
    /// Inclusive upper bound for the published pct.
    max_pct: f32,
    /// Visual width in pixels.
    width: f32,
}

impl<'a, Message> ArtworkSplitHandle<'a, Message> {
    pub(crate) fn new(
        window_width: f32,
        current_pct: f32,
        on_change: impl Fn(f32) -> Message + 'a,
        on_commit: impl Fn(f32) -> Message + 'a,
    ) -> Self {
        Self {
            window_width,
            current_pct,
            on_change: Box::new(on_change),
            on_commit: Box::new(on_commit),
            min_pct: 0.05,
            max_pct: 0.80,
            width: HANDLE_WIDTH,
        }
    }

    /// Compute the pct that corresponds to the cursor's current x given the
    /// drag origin. Dragging left (cursor_x < start) shrinks the artwork
    /// column (pct decreases); dragging right grows it.
    ///
    /// **Convention.** The handle sits to the *left* of the artwork column,
    /// so `column_pct = (window_width - handle_x) / window_width`. Increasing
    /// cursor_x pushes the handle right, which shrinks the artwork side.
    fn pct_from_cursor(&self, cursor_x: f32, start_x: f32, start_pct: f32) -> f32 {
        if self.window_width <= 0.0 {
            return start_pct;
        }
        let dx = cursor_x - start_x;
        let dpct = dx / self.window_width;
        (start_pct - dpct).clamp(self.min_pct, self.max_pct)
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
        Size {
            width: Length::Fixed(self.width),
            height: Length::Fill,
        }
    }

    fn layout(&mut self, _tree: &mut Tree, _renderer: &Renderer, limits: &Limits) -> layout::Node {
        let max = limits.max();
        layout::Node::new(Size::new(self.width, max.height))
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
                    *state = HandleState::Dragging {
                        start_cursor_x: p.x,
                        start_pct: self.current_pct,
                    };
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let HandleState::Dragging {
                    start_cursor_x,
                    start_pct,
                } = *state
                    && let Some(p) = cursor.position()
                {
                    let new_pct = self.pct_from_cursor(p.x, start_cursor_x, start_pct);
                    shell.publish((self.on_change)(new_pct));
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if let HandleState::Dragging {
                    start_cursor_x,
                    start_pct,
                } = *state
                {
                    let final_pct = cursor.position().map_or(start_pct, |p| {
                        self.pct_from_cursor(p.x, start_cursor_x, start_pct)
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
            (HandleState::Dragging { .. }, _) | (_, true) => {
                mouse::Interaction::ResizingHorizontally
            }
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

/// Convenience constructor: build an `Element` for the handle that emits a
/// single per-view `Message` via the given closure. Reads the current width
/// fraction from the theme atomic so the drag is anchored to the displayed
/// width (not a stale snapshot).
pub(crate) fn artwork_split_handle_element<'a, M, F>(
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
        window_width,
        crate::theme::artwork_column_width_pct(),
        move |pct| on_change(DragEvent::Change(pct)),
        move |pct| on_commit(DragEvent::Commit(pct)),
    )
    .into()
}
