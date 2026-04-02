//! Custom 3D Button Widget with Pressed State Feedback
//!
//! A button widget that provides tactile visual feedback when pressed,
//! matching the QML reference client's ThreeDBorderBackground behavior.
//!
//! Features:
//! - 3D beveled borders that invert on press
//! - Active state toggle for selected buttons
//! - Supports arbitrary content (text, icons, etc.)

use iced::{
    Color, Element, Event, Length, Padding, Rectangle, Size, Theme,
    advanced::{
        Shell,
        layout::{self, Layout},
        renderer,
        widget::{self, Widget},
    },
    mouse, touch,
};

use crate::theme;

/// State for 3D button interaction
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct State {
    is_pressed: bool,
}

/// Custom 3D button with pressed state visual feedback
pub struct ThreeDButton<'a, Message> {
    content: Element<'a, Message, Theme, iced::Renderer>,
    on_press: Option<Message>,
    width: Length,
    height: f32,
    bg_color: Color,
    content_color: Color,
    pressed_content_color: Color,
    is_active: bool,
}

impl<'a, Message: Clone> ThreeDButton<'a, Message> {
    pub fn new(content: impl Into<Element<'a, Message, Theme, iced::Renderer>>) -> Self {
        Self {
            content: content.into(),
            on_press: None,
            width: Length::Fill,
            height: 36.0,
            bg_color: theme::bg2(),
            content_color: theme::fg1(),
            pressed_content_color: theme::bg0_hard(),
            is_active: false,
        }
    }

    pub fn on_press(mut self, message: Message) -> Self {
        self.on_press = Some(message);
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    pub fn background(mut self, color: Color) -> Self {
        self.bg_color = color;
        self
    }

    pub fn active(mut self, is_active: bool) -> Self {
        self.is_active = is_active;
        self
    }

    pub fn content_color(mut self, color: Color) -> Self {
        self.content_color = color;
        self
    }

    pub fn pressed_content_color(mut self, color: Color) -> Self {
        self.pressed_content_color = color;
        self
    }
}

impl<Message: Clone> Widget<Message, Theme, iced::Renderer> for ThreeDButton<'_, Message> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }

    fn children(&self) -> Vec<widget::Tree> {
        vec![widget::Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut widget::Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: Length::Fixed(self.height),
        }
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        // Border width for padding calculation
        let border_width = 2.0;
        let padding = Padding::new(border_width);

        // Layout the content with padding for borders
        layout::padded(
            limits,
            self.width,
            Length::Fixed(self.height),
            padding,
            |limits| {
                self.content
                    .as_widget_mut()
                    .layout(&mut tree.children[0], renderer, limits)
            },
        )
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
            | Event::Touch(touch::Event::FingerLifted { .. })
                if state.is_pressed => {
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
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let border_width = 2.0;

        // Determine border colors based on state
        // Both active and pressed states use the same "pressed in" appearance
        // Get raised border colors from theme (automatically handles light/dark mode)
        let (raised_top_left, raised_bottom_right) = theme::border_3d_raised();
        let (top_left_color, bottom_right_color) = if self.is_active || state.is_pressed {
            // Pressed: invert for "pushed in" look
            (raised_bottom_right, raised_top_left)
        } else {
            // Normal: raised 3D
            (raised_top_left, raised_bottom_right)
        };

        // Main background
        let bg_color = if state.is_pressed || self.is_active {
            theme::accent_bright()
        } else {
            self.bg_color
        };

        // Helper closure: draw the bevel + content at current renderer transform
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

            // Draw content - get the content layout from children
            if let Some(content_layout) = layout.children().next() {
                let content_style = renderer::Style {
                    text_color: if state.is_pressed || self.is_active {
                        self.pressed_content_color
                    } else {
                        self.content_color
                    },
                };

                self.content.as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    &content_style,
                    content_layout,
                    cursor,
                    viewport,
                );
            }
        };

        draw_content(renderer);
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

impl<'a, Message: Clone + 'a> From<ThreeDButton<'a, Message>> for Element<'a, Message> {
    fn from(button: ThreeDButton<'a, Message>) -> Self {
        Element::new(button)
    }
}

/// Helper function to create a 3D button
pub(crate) fn three_d_button<'a, Message: Clone + 'a>(
    content: impl Into<Element<'a, Message, Theme, iced::Renderer>>,
) -> ThreeDButton<'a, Message> {
    ThreeDButton::new(content)
}
