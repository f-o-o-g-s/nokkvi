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

/// Alpha for the hover tint overlay (subtle on top of any background).
const HOVER_ALPHA: f32 = 0.10;

/// Alpha for the press tint overlay (stronger, tactile).
const PRESS_ALPHA: f32 = 0.20;

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
    /// `true` when the wrapped surface is already filled with `accent_bright()`
    /// (active nav tab, active player mode toggle). Such surfaces use a
    /// contrasting neutral hover pigment instead of the accent wash, which
    /// over an accent fill would be a near-no-op.
    on_accent_surface: bool,
    /// When `false`, the hover/press color wash is suppressed (the press
    /// scale-down still fires). Used by per-theme swatch rows, where the
    /// active-theme accent wash would clash with each row's own palette.
    wash_enabled: bool,
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
            on_accent_surface: false,
            wash_enabled: true,
        }
    }

    /// Suppress the hover/press color wash (the press scale-down still fires).
    /// For surfaces painted in a foreign palette where the active-theme wash
    /// muddies the colors — e.g. the theme-picker swatch rows.
    pub(crate) fn wash_enabled(mut self, yes: bool) -> Self {
        self.wash_enabled = yes;
        self
    }

    /// Set the border radius of the hover overlay quad.
    pub(crate) fn border_radius(mut self, radius: iced::border::Radius) -> Self {
        self.border_radius = radius;
        self
    }

    /// Mark the wrapped surface as already `accent_bright()`-filled when `yes`
    /// is `true` (pass the surface's own active flag). Such surfaces deposit a
    /// contrasting neutral pigment (`theme::hover_tint_on_accent()`) instead of
    /// the accent wash, which over an accent fill would barely register.
    pub(crate) fn on_accent_surface(mut self, yes: bool) -> Self {
        self.on_accent_surface = yes;
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

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
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
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if cursor.is_over(layout.bounds()) =>
            {
                state.is_mouse_pressed = true;
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if state.is_mouse_pressed =>
            {
                state.is_mouse_pressed = false;
                shell.request_redraw();
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

        // Draw overlay on top: stronger for press, subtle for hover.
        // The pigment is the theme accent wash (`hover_tint`) so hover reads
        // as the same family as the playlist-header wash across all themes —
        // except over an already-`accent_bright()`-filled surface, where a
        // contrasting neutral pigment (`hover_tint_on_accent`) is used so
        // accent-over-accent doesn't vanish. The overlay's own alpha makes the
        // live composite equal `lerp(surface, pigment, alpha)`.
        if (is_hovered || is_pressed) && self.wash_enabled {
            let base = if self.on_accent_surface {
                crate::theme::hover_tint_on_accent()
            } else {
                crate::theme::hover_tint()
            };
            let alpha = if is_pressed { PRESS_ALPHA } else { HOVER_ALPHA };
            let color = Color { a: alpha, ..base };

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
