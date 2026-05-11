//! Horizontal-bar drag handle for resizing the Always-Vertical artwork
//! panel.
//!
//! Sibling of `artwork_split_handle.rs` — same `DragEvent` enum, same
//! Change/Commit cadence — but oriented horizontally so it sits between the
//! stacked artwork and the slot list and is dragged up/down. The published
//! pct is the artwork's height as a fraction of window height.
//!
//! - `on_change(pct)` fires every `CursorMoved` during a drag (live preview).
//! - `on_commit(pct)` fires once on `ButtonReleased` (persist to TOML).
//!
//! There is no click-vs-drag threshold — every press-drag-release is treated
//! as a resize gesture, matching the horizontal handle's behavior.

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

pub(crate) use super::artwork_split_handle::DragEvent;

/// Visual thickness of the horizontal drag bar.
pub(crate) const HANDLE_HEIGHT: f32 = 6.0;

#[derive(Debug, Clone, Copy, Default)]
enum HandleState {
    #[default]
    Idle,
    Dragging {
        /// Cursor y-position when the gesture started.
        start_cursor_y: f32,
        /// Height fraction when the gesture started.
        start_pct: f32,
    },
}

/// Visual + behavioral configuration for the vertical-stack handle.
pub(crate) struct ArtworkSplitHandleVertical<'a, Message> {
    /// Window height — used to convert px deltas into pct deltas.
    window_height: f32,
    /// Current height fraction (used as the gesture anchor).
    current_pct: f32,
    /// Pct emitted on every CursorMoved during a drag (live preview).
    on_change: Box<dyn Fn(f32) -> Message + 'a>,
    /// Pct emitted on ButtonReleased after a drag (commit + persist).
    on_commit: Box<dyn Fn(f32) -> Message + 'a>,
    /// Inclusive lower bound for the published pct.
    min_pct: f32,
    /// Inclusive upper bound for the published pct.
    max_pct: f32,
    /// Visual height in pixels.
    height: f32,
}

impl<'a, Message> ArtworkSplitHandleVertical<'a, Message> {
    pub(crate) fn new(
        window_height: f32,
        current_pct: f32,
        on_change: impl Fn(f32) -> Message + 'a,
        on_commit: impl Fn(f32) -> Message + 'a,
    ) -> Self {
        Self {
            window_height,
            current_pct,
            on_change: Box::new(on_change),
            on_commit: Box::new(on_commit),
            min_pct: 0.10,
            max_pct: 0.80,
            height: HANDLE_HEIGHT,
        }
    }

    /// Compute the pct that corresponds to the cursor's current y given the
    /// drag origin. The handle sits *below* the artwork — dragging down
    /// (cursor_y > start) grows the artwork (pct increases), up shrinks.
    fn pct_from_cursor(&self, cursor_y: f32, start_y: f32, start_pct: f32) -> f32 {
        if self.window_height <= 0.0 {
            return start_pct;
        }
        let dy = cursor_y - start_y;
        let dpct = dy / self.window_height;
        (start_pct + dpct).clamp(self.min_pct, self.max_pct)
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for ArtworkSplitHandleVertical<'_, Message>
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
            height: Length::Fixed(self.height),
        }
    }

    fn layout(&mut self, _tree: &mut Tree, _renderer: &Renderer, limits: &Limits) -> layout::Node {
        let max = limits.max();
        layout::Node::new(Size::new(max.width, self.height))
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
                        start_cursor_y: p.y,
                        start_pct: self.current_pct,
                    };
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let HandleState::Dragging {
                    start_cursor_y,
                    start_pct,
                } = *state
                    && let Some(p) = cursor.position()
                {
                    let new_pct = self.pct_from_cursor(p.y, start_cursor_y, start_pct);
                    shell.publish((self.on_change)(new_pct));
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if let HandleState::Dragging {
                    start_cursor_y,
                    start_pct,
                } = *state
                {
                    let final_pct = cursor.position().map_or(start_pct, |p| {
                        self.pct_from_cursor(p.y, start_cursor_y, start_pct)
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
            (HandleState::Dragging { .. }, _) | (_, true) => mouse::Interaction::ResizingVertically,
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

impl<'a, Message: 'a, Theme, Renderer> From<ArtworkSplitHandleVertical<'a, Message>>
    for Element<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn from(handle: ArtworkSplitHandleVertical<'a, Message>) -> Self {
        Self::new(handle)
    }
}

/// Convenience constructor: build an `Element` for the vertical handle that
/// emits a single per-view `Message` via the given closure. Reads the current
/// height fraction from the theme atomic so the drag is anchored to the
/// displayed extent (not a stale snapshot).
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
    ArtworkSplitHandleVertical::new(
        window_height,
        crate::theme::artwork_vertical_height_pct(),
        move |pct| on_change(DragEvent::Change(pct)),
        move |pct| on_commit(DragEvent::Commit(pct)),
    )
    .into()
}
