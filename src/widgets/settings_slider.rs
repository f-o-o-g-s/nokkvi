//! Draggable settings slider — flat 6 px track + 14 px square (flat) /
//! pill (rounded) handle. Visual conventions match
//! [`progress_bar.rs`](super::progress_bar) so the seek bar and the settings
//! sliders read as the same widget family.
//!
//! Interaction (only active when `enabled`):
//! - Click on track → emits the fraction under the cursor.
//! - Click + drag → emits a fresh fraction on every cursor move.
//! - Pointer release ends the drag.
//!
//! The widget emits a fraction in `[0.0, 1.0]`; the settings update handler
//! maps it to the centered item's value via [`SettingValue::set_fraction`].
//! Clicks outside the visible bounds are *not* captured, so the surrounding
//! row-button still receives them and can focus the row.
//!
//! [`SettingValue::set_fraction`]: nokkvi_data::types::setting_value::SettingValue::set_fraction

use iced::{
    Color, Element, Event, Length, Point, Rectangle, Size, Theme,
    advanced::{
        Shell,
        layout::{self, Layout},
        renderer,
        widget::{self, Widget},
    },
    mouse, touch,
};

/// Visible track thickness — matches `progress_bar::TRACK_THICKNESS`.
const TRACK_THICKNESS: f32 = 6.0;
/// Handle edge length — matches `progress_bar::HANDLE_SIZE`. Square in flat
/// mode, pill in rounded mode (controlled by `theme::ui_radius_pill()`).
const HANDLE_SIZE: f32 = 14.0;

#[derive(Debug, Clone, Default)]
pub(crate) struct State {
    is_dragging: bool,
}

/// Draggable settings slider. Emits a fraction in `[0.0, 1.0]`.
pub struct SettingsSlider<'a, Message> {
    fraction: f32,
    on_change: Box<dyn Fn(f32) -> Message + 'a>,
    width: Length,
    height: f32,
    enabled: bool,
    opacity: f32,
}

impl<'a, Message> SettingsSlider<'a, Message> {
    /// Build a slider showing `fraction` (clamped to `[0.0, 1.0]`). `on_change`
    /// receives a fresh fraction on click + every drag tick.
    pub fn new<F>(fraction: f32, on_change: F) -> Self
    where
        F: 'a + Fn(f32) -> Message,
    {
        Self {
            fraction: fraction.clamp(0.0, 1.0),
            on_change: Box::new(on_change),
            width: Length::Fill,
            height: 22.0,
            enabled: true,
            opacity: 1.0,
        }
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// When `false`, the slider renders the same visuals but ignores pointer
    /// input — clicks bubble through to whatever sits behind it. Used to keep
    /// non-focused rows in a settings detail pane visually consistent while
    /// letting a click on the row focus it via the surrounding row-button.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Multiply every drawn color's alpha by this value. Used by the settings
    /// detail pane to dim non-focused rows.
    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity.clamp(0.0, 1.0);
        self
    }

    /// Project a screen X to a fraction within the widget's drag track.
    fn fraction_at(&self, cursor_x: f32, bounds: Rectangle) -> f32 {
        let effective_width = bounds.width - HANDLE_SIZE;
        if effective_width <= 0.0 {
            return 0.0;
        }
        let relative_x = cursor_x - bounds.x - HANDLE_SIZE / 2.0;
        (relative_x / effective_width).clamp(0.0, 1.0)
    }
}

impl<Message: Clone> Widget<Message, Theme, iced::Renderer> for SettingsSlider<'_, Message> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: Length::Fixed(self.height),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut widget::Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, self.width, Length::Fixed(self.height))
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        if !self.enabled {
            return;
        }

        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                if let Some(cursor_position) = cursor.position_over(bounds) {
                    state.is_dragging = true;
                    let frac = self.fraction_at(cursor_position.x, bounds);
                    shell.publish((self.on_change)(frac));
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerLifted { .. } | touch::Event::FingerLost { .. })
                if state.is_dragging =>
            {
                state.is_dragging = false;
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::CursorMoved { .. })
            | Event::Touch(touch::Event::FingerMoved { .. }) => {
                if state.is_dragging
                    && let Some(Point { x, .. }) = cursor.position()
                {
                    let frac = self.fraction_at(x, bounds);
                    shell.publish((self.on_change)(frac));
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        _tree: &widget::Tree,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer;

        let bounds = layout.bounds();
        let progress = self.fraction.clamp(0.0, 1.0);

        let effective_width = (bounds.width - HANDLE_SIZE).max(0.0);
        let handle_x = bounds.x + progress * effective_width;
        let track_y = bounds.y + (bounds.height - TRACK_THICKNESS) / 2.0;
        let radius = crate::theme::ui_radius_pill();

        let track_color = apply_alpha(crate::theme::bg2(), self.opacity);
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x: bounds.x,
                    y: track_y,
                    width: bounds.width,
                    height: TRACK_THICKNESS,
                },
                border: iced::Border {
                    radius,
                    ..Default::default()
                },
                ..Default::default()
            },
            track_color,
        );

        let fill_color = apply_alpha(crate::theme::accent_bright(), self.opacity);
        let fill_width = (handle_x - bounds.x + HANDLE_SIZE / 2.0).clamp(0.0, bounds.width);
        if fill_width > 0.0 {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: bounds.x,
                        y: track_y,
                        width: fill_width,
                        height: TRACK_THICKNESS,
                    },
                    border: iced::Border {
                        radius,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                fill_color,
            );
        }

        // Handle on its own layer so a 1 px border on bg0_hard reads cleanly
        // against the green fill underneath.
        let handle_clip = bounds;
        let handle_border_color = apply_alpha(crate::theme::bg0_hard(), self.opacity);
        renderer.with_layer(handle_clip, |renderer| {
            let handle_y = bounds.y + (bounds.height - HANDLE_SIZE) / 2.0;
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: handle_x,
                        y: handle_y,
                        width: HANDLE_SIZE,
                        height: HANDLE_SIZE,
                    },
                    border: iced::Border {
                        color: handle_border_color,
                        width: 1.0,
                        radius,
                    },
                    ..Default::default()
                },
                fill_color,
            );
        });
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if !self.enabled {
            return mouse::Interaction::default();
        }
        let state = tree.state.downcast_ref::<State>();
        if state.is_dragging {
            mouse::Interaction::Grabbing
        } else if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
}

impl<'a, Message: Clone + 'a> From<SettingsSlider<'a, Message>> for Element<'a, Message> {
    fn from(slider: SettingsSlider<'a, Message>) -> Self {
        Element::new(slider)
    }
}

fn apply_alpha(color: Color, opacity: f32) -> Color {
    Color {
        a: color.a * opacity,
        ..color
    }
}
