//! A custom text widget that acts like a link on hover and click.
//!
//! Renders text exactly to its inner bounds (tight hitbox).
//! Applies an underline and uses the accent color when hovered.
//! Emits an arbitrary message when clicked. Captures click events
//! to prevent them from bubbling to the parent row.

use iced::{
    Background, Color, Element, Event, Length, Pixels, Rectangle, Size, Theme,
    advanced::{
        Layout, Renderer as AdvancedRenderer, Shell, Widget, layout, mouse, renderer,
        text::{self as advanced_text, Renderer as TextRenderer, Shaping, Text},
        widget::{self, Tree},
    },
    font::Font,
    widget::text::Wrapping,
};

#[derive(Debug, Clone, Default)]
struct State {
    constrained:
        iced::advanced::text::paragraph::Plain<<iced::Renderer as TextRenderer>::Paragraph>,
    is_hovered: bool,
}

pub struct LinkText<M> {
    content: String,
    size: Pixels,
    font: Font,
    on_press: Option<M>,
    color: Option<Color>,
    hover_color: Option<Color>,
}

impl<M> LinkText<M> {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            size: Pixels(14.0),
            font: crate::theme::ui_font(),
            on_press: None,
            color: None,
            hover_color: None,
        }
    }

    pub fn size(mut self, size: impl Into<Pixels>) -> Self {
        self.size = size.into();
        self
    }

    pub fn font(mut self, font: Font) -> Self {
        self.font = font;
        self
    }

    pub fn color(mut self, color: impl Into<Color>) -> Self {
        self.color = Some(color.into());
        self
    }

    pub fn hover_color(mut self, color: impl Into<Color>) -> Self {
        self.hover_color = Some(color.into());
        self
    }

    pub fn on_press(mut self, msg: Option<M>) -> Self {
        self.on_press = msg;
        self
    }
}

impl<M: Clone + 'static> Widget<M, Theme, iced::Renderer> for LinkText<M> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Shrink,
            height: Length::Shrink,
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<State>();

        layout::sized(limits, Length::Shrink, Length::Shrink, |limits| {
            let bounds = limits.max();

            let text = Text {
                content: self.content.as_str(),
                bounds,
                size: self.size,
                line_height: advanced_text::LineHeight::default(),
                font: self.font,
                align_x: advanced_text::Alignment::Left,
                align_y: iced::alignment::Vertical::Center,
                shaping: Shaping::Advanced,
                wrapping: Wrapping::None,
                ellipsis: advanced_text::Ellipsis::End,
                hint_factor: AdvancedRenderer::scale_factor(renderer),
            };
            state.constrained.update(text);

            state.constrained.min_bounds()
        })
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        shell: &mut Shell<'_, M>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let is_over = cursor.is_over(layout.bounds());

        let should_be_hovered = is_over && self.on_press.is_some();
        if state.is_hovered != should_be_hovered {
            state.is_hovered = should_be_hovered;
            shell.request_redraw();
        }

        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
            && is_over
            && let Some(msg) = &self.on_press
        {
            shell.publish(msg.clone());
            shell.capture_event(); // Capture to prevent row selection!
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) && self.on_press.is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();

        let color = if state.is_hovered {
            self.hover_color.unwrap_or_else(crate::theme::accent_bright)
        } else {
            self.color.unwrap_or_else(crate::theme::fg0)
        };

        // Draw text
        renderer.fill_paragraph(
            state.constrained.raw(),
            bounds.position(),
            color,
            Rectangle::with_size(Size::INFINITE),
        );

        // Draw underline if hovered
        if state.is_hovered {
            let underline_y = bounds.y + bounds.height;
            AdvancedRenderer::fill_quad(
                renderer,
                renderer::Quad {
                    bounds: Rectangle {
                        x: bounds.x,
                        y: underline_y - 1.0, // Draw 1px thick underline fitting within bounds
                        width: bounds.width,
                        height: 1.0,
                    },
                    border: iced::Border::default(),
                    shadow: iced::Shadow::default(),
                    snap: true,
                },
                Background::Color(color),
            );
        }
    }
}

impl<'a, M: Clone + 'static> From<LinkText<M>> for Element<'a, M> {
    fn from(link: LinkText<M>) -> Self {
        Element::new(link)
    }
}
