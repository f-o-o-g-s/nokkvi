//! Default-playlist chip — pin button in the view header.
//!
//! Always renders as a 40×40 icon button. Hovering reveals a tooltip with
//! "Default Playlist: <name>" (or "Default Playlist: (none) — click to set"
//! when no default is set). Click opens the picker overlay.
//!
//! Used by the Playlists view (always) and the Queue view (gated by the
//! `queue_show_default_playlist` setting).

use iced::{
    Element, Length,
    widget::{container, mouse_area, svg, text, tooltip},
};

use crate::{embedded_svg::svg_widget, theme, widgets::hover_overlay::HoverOverlay};

/// Render the default-playlist chip as a pin icon button.
///
/// `default_playlist_name` empty → "(none)" placeholder in the tooltip + dimmed icon.
/// `on_press` fires when the chip is clicked (open the picker).
pub(crate) fn default_playlist_chip<'a, Message: Clone + 'a>(
    default_playlist_name: &str,
    on_press: Message,
) -> Element<'a, Message> {
    let has_default = !default_playlist_name.is_empty();
    let icon_color = if has_default {
        theme::fg0()
    } else {
        theme::fg3()
    };

    let pin_icon = svg_widget("assets/icons/pin.svg")
        .width(Length::Fixed(20.0))
        .height(Length::Fixed(20.0))
        .style(move |_theme, _status| svg::Style {
            color: Some(icon_color),
        });

    let body = container(pin_icon)
        .width(Length::Fixed(40.0))
        .height(Length::Fixed(40.0))
        .style(|_theme| container::Style {
            background: Some(theme::bg0_soft().into()),
            border: iced::Border {
                radius: theme::ui_border_radius(),
                ..Default::default()
            },
            ..Default::default()
        })
        .center(Length::Fixed(40.0));

    let tooltip_label = if has_default {
        format!("Default Playlist: {default_playlist_name}")
    } else {
        "Default Playlist: (none) — click to set".to_string()
    };

    tooltip(
        mouse_area(HoverOverlay::new(body).border_radius(theme::ui_border_radius()))
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
