//! A wrapper widget that renders a semi-transparent overlay on mouse hover/press.
//!
//! Delegates all sizing, layout, and events to the wrapped child.
//! In `draw()`, renders the child first, then fills a dark quad on top
//! when the cursor is over the widget bounds. Press events are tracked
//! passively (never captured) so the inner button's click handling is unaffected.
//!
//! The center slot can receive a `flash_at` timestamp from external triggers
//! (Enter key, MPRIS, player bar) to show a timed press animation.

use iced::{
    Background, Color, Element, Event, Length, Rectangle, Size, Transformation, Vector,
    advanced::{
        Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{Operation, Tree, tree},
    },
    time, window,
};

/// Overlay color on hover: subtle semi-transparent black
const HOVER_COLOR: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.08,
};

/// Overlay color on press: stronger semi-transparent black for tactile feedback
const PRESS_COLOR: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.16,
};

/// Scale factor on press: 98% gives a subtle "push in" feel.
const PRESS_SCALE: f32 = 0.98;

/// Duration of the flash animation triggered by external sources (Enter, MPRIS, etc.).
pub(crate) const FLASH_DURATION: time::Duration = time::Duration::from_millis(120);

/// Tracks mouse press state for visual feedback.
#[derive(Debug, Clone, Copy, Default)]
struct State {
    /// Mouse button is currently held down over this widget.
    is_mouse_pressed: bool,
}

/// A widget that draws a semi-transparent dark overlay on hover/press.
///
/// Wraps any child element, passing through all layout/event/overlay behavior unchanged.
/// The overlay is purely visual — no messages are emitted. Mouse press state is tracked
/// passively (events are never captured) so the inner button's click handling is unaffected.
///
/// External triggers (Enter key, MPRIS, player bar) pass a `flash_at` timestamp which
/// drives a timed press animation on the center slot.
pub(crate) struct HoverOverlay<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    border_radius: iced::border::Radius,
    /// External flash timestamp from `SlotListView::flash_center_at`.
    /// When within `FLASH_DURATION`, the widget shows the press animation.
    flash_at: Option<time::Instant>,
}

impl<'a, Message, Theme, Renderer> HoverOverlay<'a, Message, Theme, Renderer>
where
    Renderer: iced::advanced::Renderer,
{
    /// Wrap `content` with a hover overlay.
    pub(crate) fn new(content: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        Self {
            content: content.into(),
            border_radius: crate::theme::ui_border_radius(),
            flash_at: None,
        }
    }

    /// Set the border radius of the hover overlay quad.
    pub(crate) fn border_radius(mut self, radius: iced::border::Radius) -> Self {
        self.border_radius = radius;
        self
    }

    /// Set the external flash timestamp for timed press animation.
    /// Passed from `SlotListView::flash_center_at` for the center slot.
    pub(crate) fn flash_at(mut self, flash_at: Option<time::Instant>) -> Self {
        self.flash_at = flash_at;
        self
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for HoverOverlay<'_, Message, Theme, Renderer>
where
    Renderer: iced::advanced::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        // Delegate to child first — the inner button captures the event for click handling
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            shell,
            viewport,
        );

        let state = tree.state.downcast_mut::<State>();

        match event {
            // Mouse press/release tracking (passive — never captures).
            // Request redraws so the scale-down effect is painted immediately.
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if cursor.is_over(layout.bounds()) {
                    state.is_mouse_pressed = true;
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.is_mouse_pressed {
                    state.is_mouse_pressed = false;
                    shell.request_redraw();
                }
            }

            // During a flash animation, request redraws to keep it alive
            Event::Window(window::Event::RedrawRequested(_)) => {
                if let Some(flash_at) = self.flash_at
                    && time::Instant::now().duration_since(flash_at) < FLASH_DURATION
                {
                    shell.request_redraw();
                }
            }

            _ => {}
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let state = tree.state.downcast_ref::<State>();
        let is_hovered = cursor.is_over(bounds);

        // Check external flash animation
        let is_flashing = self
            .flash_at
            .is_some_and(|t| time::Instant::now().duration_since(t) < FLASH_DURATION);

        let is_pressed = (is_hovered && state.is_mouse_pressed) || is_flashing;

        // When pressed, scale the child content down around the slot center
        if is_pressed {
            let cx = bounds.x + bounds.width / 2.0;
            let cy = bounds.y + bounds.height / 2.0;

            let transformation = Transformation::translate(cx, cy)
                * Transformation::scale(PRESS_SCALE)
                * Transformation::translate(-cx, -cy);

            renderer.with_layer(bounds, |renderer| {
                renderer.with_transformation(transformation, |renderer| {
                    self.content.as_widget().draw(
                        &tree.children[0],
                        renderer,
                        theme,
                        style,
                        layout,
                        cursor,
                        viewport,
                    );
                });
            });
        } else {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                layout,
                cursor,
                viewport,
            );
        }

        // Draw overlay on top: stronger for press, subtle for hover
        if is_hovered || is_pressed {
            let color = if is_pressed { PRESS_COLOR } else { HOVER_COLOR };

            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border: iced::Border {
                        radius: self.border_radius,
                        ..Default::default()
                    },
                    shadow: iced::Shadow::default(),
                    snap: true,
                },
                Background::Color(color),
            );
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message, Theme, Renderer> From<HoverOverlay<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: iced::advanced::Renderer + 'a,
{
    fn from(overlay: HoverOverlay<'a, Message, Theme, Renderer>) -> Self {
        Self::new(overlay)
    }
}
