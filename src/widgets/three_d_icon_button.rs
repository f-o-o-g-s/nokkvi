//! Custom 3D Icon Button Widget with Pressed State SVG Color Feedback
//!
//! A specialized button widget for SVG icons that changes the icon color
//! when pressed, matching the QML reference client's ThreeDBorderBackground behavior.

use iced::{
    Color, Element, Event, Length, Radians, Rectangle, Size, Theme, Transformation,
    advanced::{
        Renderer as _, Shell,
        layout::{self, Layout},
        renderer,
        svg::{Handle, Svg as SvgData},
        widget::{self, Widget},
    },
    mouse, touch,
};

use crate::theme;

/// Scale factor on press: shrink to 92% for tactile "push in" on small buttons.
const PRESS_SCALE: f32 = 0.92;

/// State for 3D icon button interaction
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct State {
    is_pressed: bool,
}

/// Custom 3D icon button with pressed state visual feedback for SVGs
pub struct ThreeDIconButton<Message> {
    icon_handle: Handle,
    on_press: Option<Message>,
    width: f32,
    height: f32,
    icon_size: f32,
    bg_color: Color,
    icon_color: Color,
    pressed_icon_color: Color,
    is_active: bool,
}

impl<Message: Clone> ThreeDIconButton<Message> {
    pub fn new(icon_path: &str) -> Self {
        // Get embedded SVG content and convert to Handle
        let svg_content = crate::embedded_svg::get_svg(icon_path);
        let icon_handle = Handle::from_memory(svg_content.as_bytes());

        Self {
            icon_handle,
            on_press: None,
            width: 36.0,
            height: 36.0,
            icon_size: 20.0,
            bg_color: theme::bg2(),
            icon_color: theme::fg1(),
            pressed_icon_color: theme::bg0_hard(),
            is_active: false,
        }
    }

    pub fn on_press(mut self, message: Message) -> Self {
        self.on_press = Some(message);
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    pub fn icon_size(mut self, size: f32) -> Self {
        self.icon_size = size;
        self
    }

    pub fn background(mut self, color: Color) -> Self {
        self.bg_color = color;
        self
    }

    pub fn icon_color(mut self, color: Color) -> Self {
        self.icon_color = color;
        self
    }

    pub fn pressed_icon_color(mut self, color: Color) -> Self {
        self.pressed_icon_color = color;
        self
    }

    pub fn active(mut self, is_active: bool) -> Self {
        self.is_active = is_active;
        self
    }
}

impl<Message: Clone> Widget<Message, Theme, iced::Renderer> for ThreeDIconButton<Message> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fixed(self.width),
            height: Length::Fixed(self.height),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut widget::Tree,
        _renderer: &iced::Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(Size::new(self.width, self.height))
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
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                if self.on_press.is_some() && cursor.is_over(bounds) {
                    // Reset any leftover pressed state from previous interaction
                    state.is_pressed = true;
                    shell.capture_event();
                    shell.request_redraw();
                } else {
                    // Clear pressed state if clicking elsewhere
                    if state.is_pressed {
                        state.is_pressed = false;
                        shell.request_redraw();
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerLifted { .. }) => {
                if state.is_pressed {
                    if cursor.is_over(bounds) {
                        // Publish the click action
                        if let Some(on_press) = &self.on_press {
                            shell.publish(on_press.clone());
                        }
                    }
                    // Always clear pressed state on release to prevent stuck buttons
                    state.is_pressed = false;
                    shell.request_redraw();
                }
            }
            Event::Touch(touch::Event::FingerLost { .. }) => {
                state.is_pressed = false;
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use iced::advanced::svg::Renderer as SvgRenderer;

        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let border_width = 2.0;

        // Determine colors based on state
        // Both active and pressed states use the same "pressed in" appearance
        // Get raised border colors from theme (automatically handles light/dark mode)
        let (raised_top_left, raised_bottom_right) = theme::border_3d_raised();
        let (top_left_color, bottom_right_color) = if self.is_active || state.is_pressed {
            (raised_bottom_right, raised_top_left)
        } else {
            (raised_top_left, raised_bottom_right)
        };

        let bg_color = if state.is_pressed || self.is_active {
            theme::accent_bright()
        } else {
            self.bg_color
        };

        let icon_color = if state.is_pressed || self.is_active {
            self.pressed_icon_color
        } else {
            self.icon_color
        };

        // Helper closure: draw the bevel + icon at current renderer transform
        let draw_content = |renderer: &mut iced::Renderer| {
            // Draw 3D beveled background (shared helper)
            super::three_d_helpers::draw_3d_bevel(
                renderer,
                bounds,
                border_width,
                bg_color,
                top_left_color,
                bottom_right_color,
            );

            // Draw centered SVG icon
            let icon_x = bounds.center_x() - self.icon_size / 2.0;
            let icon_y = bounds.center_y() - self.icon_size / 2.0;
            let icon_bounds = Rectangle {
                x: icon_x,
                y: icon_y,
                width: self.icon_size,
                height: self.icon_size,
            };

            renderer.draw_svg(
                SvgData {
                    handle: self.icon_handle.clone(),
                    color: Some(icon_color),
                    rotation: Radians(0.0),
                    opacity: 1.0,
                },
                icon_bounds,
                icon_bounds,
            );
        };

        // When pressed, scale the entire button down around its center
        if state.is_pressed {
            let cx = bounds.x + bounds.width / 2.0;
            let cy = bounds.y + bounds.height / 2.0;
            let transformation = Transformation::translate(cx, cy)
                * Transformation::scale(PRESS_SCALE)
                * Transformation::translate(-cx, -cy);

            renderer.with_layer(bounds, |renderer| {
                renderer.with_transformation(transformation, |renderer| {
                    draw_content(renderer);
                });
            });
        } else {
            draw_content(renderer);
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if self.on_press.is_some() && cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

impl<'a, Message: Clone + 'a> From<ThreeDIconButton<Message>> for Element<'a, Message> {
    fn from(button: ThreeDIconButton<Message>) -> Self {
        Element::new(button)
    }
}

/// Helper function to create a 3D icon button
pub(crate) fn three_d_icon_button<Message: Clone + 'static>(
    icon_path: &str,
) -> ThreeDIconButton<Message> {
    ThreeDIconButton::new(icon_path)
}
