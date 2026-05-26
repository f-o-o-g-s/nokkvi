//! Shared `modal_icon_button` helper.
//!
//! Both `about_modal` and `info_modal` independently defined the exact same
//! `mouse_area(HoverOverlay(container(svg).center()))` chassis to render
//! their close / copy / folder-open header icons. The two definitions were
//! byte-equivalent apart from the Message type. Consolidate them into a
//! single generic helper so a future tweak to the modal-icon chrome
//! (radius, hover behavior, glyph color) lands at one site.
//!
//! `eq_modal` builds a similar shape inside a closure but with a different
//! fill (a styled `bg0_hard` body, configurable icon color and size). That
//! variant stays where it is — it's a different visual recipe and the
//! tighter local closure keeps the modal's own customizations close to
//! their call sites.

use iced::{
    Element, Length,
    widget::{container, mouse_area, svg},
};

use crate::{
    embedded_svg, theme,
    widgets::{hover_overlay::HoverOverlay, sizes::MODAL_ICON_BUTTON_SIZE},
};

/// Borderless modal-header icon button. Renders an `icon_size` SVG centered
/// inside a `MODAL_ICON_BUTTON_SIZE` square, wrapped in a `HoverOverlay`
/// for hover/press feedback and a `mouse_area` for the click target.
///
/// Generic over the parent's `Message` type so close / copy / folder /
/// save buttons in different modals can pass distinct variants.
pub(crate) fn modal_icon_button<'a, Message>(
    icon_path: &'static str,
    icon_size: f32,
    on_press: Message,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    mouse_area(
        HoverOverlay::new(
            container(
                embedded_svg::svg_widget(icon_path)
                    .width(Length::Fixed(icon_size))
                    .height(Length::Fixed(icon_size))
                    .style(|_theme, _status| svg::Style {
                        color: Some(theme::fg3()),
                    }),
            )
            .width(Length::Fixed(MODAL_ICON_BUTTON_SIZE))
            .height(Length::Fixed(MODAL_ICON_BUTTON_SIZE))
            .style(|_theme| container::Style {
                background: None,
                border: iced::Border::default(),
                ..Default::default()
            })
            .center(Length::Fixed(MODAL_ICON_BUTTON_SIZE)),
        )
        .border_radius(theme::ui_border_radius()),
    )
    .on_press(on_press)
    .interaction(iced::mouse::Interaction::Pointer)
    .into()
}
