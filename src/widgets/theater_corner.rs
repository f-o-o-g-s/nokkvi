//! Theater Mode's corner icon: the expand icon revealed on hover over the
//! Queue's now-playing cover, and the exit icon riding above the theater bar.
//! A translucent dark chip behind the glyph keeps it legible over any cover.

use iced::{
    Color, Element, Length,
    widget::{container, mouse_area, svg},
};

use crate::{embedded_svg, theme, widgets::hover_overlay::HoverOverlay};

/// Side of the square hit target.
pub(crate) const CORNER_BUTTON_SIZE: f32 = 36.0;
/// Glyph size inside the button.
const CORNER_ICON_SIZE: f32 = 18.0;
/// Inset from the panel's (or window's) bottom-right edges.
pub(crate) const CORNER_INSET: f32 = 12.0;
/// Chip opacity: enough to separate the glyph from a bright cover without
/// hiding the art beneath.
const CHIP_ALPHA: f32 = 0.6;

/// The corner icon button. The canonical `mouse_area(HoverOverlay(container))`
/// chassis, so hover and press feedback match the modal icon buttons.
pub(crate) fn corner_button<'a, Message: Clone + 'a>(
    icon_path: &'static str,
    on_press: Message,
) -> Element<'a, Message> {
    let radius = theme::ui_radius_sm();
    mouse_area(
        HoverOverlay::new(
            container(
                embedded_svg::svg_widget(icon_path)
                    .width(Length::Fixed(CORNER_ICON_SIZE))
                    .height(Length::Fixed(CORNER_ICON_SIZE))
                    .style(|_theme, _status| svg::Style {
                        color: Some(theme::fg0()),
                    }),
            )
            .style(move |_theme| container::Style {
                background: Some(
                    Color {
                        a: CHIP_ALPHA,
                        ..theme::bg0_hard()
                    }
                    .into(),
                ),
                border: iced::Border {
                    radius,
                    ..Default::default()
                },
                ..Default::default()
            })
            .center(Length::Fixed(CORNER_BUTTON_SIZE)),
        )
        .border_radius(radius),
    )
    .on_press(on_press)
    .interaction(iced::mouse::Interaction::Pointer)
    .into()
}

/// Lay `button` in the bottom-right corner of a `Fill` layer, `CORNER_INSET`
/// from the edges. Only the button is interactive; the rest of the layer is
/// empty and event-transparent.
pub(crate) fn bottom_right<'a, Message: 'a>(
    button: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    container(button)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Right)
        .align_y(iced::alignment::Vertical::Bottom)
        .padding(CORNER_INSET)
        .into()
}
