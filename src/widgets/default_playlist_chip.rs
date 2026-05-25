//! Default-playlist chip — pin button in the view header.
//!
//! Renders as a 44×50 transparent icon cell matching the surrounding
//! `view_header` icon buttons (sort/refresh/center/add) and the columns-cog
//! trigger. Hovering reveals a tooltip with "Default Playlist: <name>" (or
//! "Default Playlist: (none) — click to set" when no default is set). Click
//! opens the picker overlay. Icon color is always `fg0()` to stay flush with
//! the row of peer header buttons; the no-default state is conveyed through
//! the tooltip text rather than icon dimming.
//!
//! Used by the Playlists view (always) and the Queue view (gated by the
//! `queue_show_default_playlist` setting).

use iced::{
    Alignment, Element, Length,
    widget::{container, mouse_area, svg, text, tooltip},
};

use crate::{embedded_svg::svg_widget, theme, widgets::hover_overlay::HoverOverlay};

/// Render the default-playlist chip as a pin icon button.
///
/// `default_playlist_name` empty → "(none)" placeholder in the tooltip.
/// `on_press` fires when the chip is clicked (open the picker).
pub(crate) fn default_playlist_chip<'a, Message: Clone + 'a>(
    default_playlist_name: &str,
    on_press: Message,
) -> Element<'a, Message> {
    let has_default = !default_playlist_name.is_empty();

    let pin_icon = svg_widget("assets/icons/pin.svg")
        .width(Length::Fixed(18.0))
        .height(Length::Fixed(18.0))
        .style(|_theme, _status| svg::Style {
            color: Some(theme::fg0()),
        });

    // Mirror `view_header::header_icon_cell`: transparent 44×50 cell, no
    // border, square hover. The header's bg0_hard() strip and sided
    // dividers supply the chrome around this cell.
    let chassis = container(pin_icon)
        .width(Length::Fixed(44.0))
        .height(Length::Fixed(50.0))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center);

    let tooltip_label = if has_default {
        format!("Default Playlist: {default_playlist_name}")
    } else {
        "Default Playlist: (none) — click to set".to_string()
    };

    tooltip(
        mouse_area(HoverOverlay::new(chassis).border_radius(0.0.into()))
            .on_press(on_press)
            .interaction(iced::mouse::Interaction::Pointer),
        container(text(tooltip_label).size(11.0).font(theme::ui_font())).padding(4),
        tooltip::Position::Top,
    )
    .gap(4)
    .style(theme::container_tooltip)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chip_renders_with_default_set() {
        let _: Element<'_, String> = default_playlist_chip("Workout", "click".to_string());
    }

    #[test]
    fn chip_renders_when_no_default() {
        let _: Element<'_, String> = default_playlist_chip("", "click".to_string());
    }
}
