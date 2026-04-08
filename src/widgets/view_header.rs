use iced::{
    Alignment, Element, Length,
    font::{Font, Weight},
    widget::{container, mouse_area, pick_list, row, text},
};
// Re-export SortMode from data crate (canonical definition)
pub(crate) use nokkvi_data::types::sort_mode::SortMode;

use super::hover_overlay::HoverOverlay;
use crate::theme;

/// ViewHeader component - horizontal bar with view selector, sort, search, and count
/// Generic over sort mode V to support different view enums (Albums, Queue, etc.)
#[allow(clippy::too_many_arguments)] // Reusable component with naturally many configuration params
pub(crate) fn view_header<
    'a,
    Message: 'a + Clone,
    V: 'a + std::fmt::Display + Clone + PartialEq,
>(
    current_view: V,
    view_options: &'a [V],
    sort_ascending: bool,
    search_query: &str,
    filtered_count: usize,
    total_count: usize,
    item_type: &str,
    search_input_id: &'static str, // Unique ID for this view's search input (must be 'static)
    on_view_selected: impl Fn(V) -> Message + 'a,
    on_sort_toggle: Option<Message>,
    on_shuffle: Option<Message>,           // Optional shuffle button
    on_refresh: Option<Message>,           // Optional refresh button
    on_center_on_playing: Option<Message>, // Optional center button
    show_search: bool,
    on_search_change: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    let view_selector = if view_options.is_empty() {
        // Render a static pill matching the pick_list styling
        Some(
            container(
                text(current_view.to_string())
                    .size(12.0)
                    .font(Font {
                        weight: Weight::Medium,
                        ..theme::ui_font()
                    })
                    .wrapping(iced::widget::text::Wrapping::None)
                    .ellipsis(iced::widget::text::Ellipsis::End),
            )
            .padding([0, 12]) // Horizontal padding
            .max_width(300.0)
            .align_y(Alignment::Center)
            .style(move |_theme| container::Style {
                text_color: Some(theme::fg0()),
                background: Some(theme::bg0_soft().into()),
                border: iced::Border {
                    color: iced::Color::TRANSPARENT,
                    width: 2.0,
                    radius: theme::ui_border_radius(),
                },
                ..Default::default()
            })
            .height(Length::Fixed(40.0)),
        )
    } else {
        Some(
            container(
                pick_list(Some(current_view), view_options, |v: &V| v.to_string())
                    .on_select(on_view_selected)
                    .width(Length::Fixed(200.0))
                    .text_size(12.0) // Match QML font size
                    .font(Font {
                        weight: Weight::Medium,
                        ..theme::ui_font()
                    })
                    .padding([10, 8]) // Increased vertical padding to fill 40px height
                    .style(move |_theme, status| pick_list::Style {
                        text_color: theme::fg0(),
                        placeholder_color: theme::fg4(),
                        handle_color: theme::fg4(),
                        background: (theme::bg0_soft()).into(),
                        border: iced::Border {
                            color: match status {
                                pick_list::Status::Active | pick_list::Status::Disabled => {
                                    iced::Color::TRANSPARENT
                                }
                                pick_list::Status::Hovered => theme::accent_bright(),
                                pick_list::Status::Opened { .. } => theme::accent_bright(),
                            },
                            width: 2.0,
                            radius: theme::ui_border_radius(),
                        },
                    })
                    .menu_style(move |_theme| {
                        // Style for the dropdown menu overlay
                        iced::widget::overlay::menu::Style {
                            text_color: theme::fg0(),
                            background: (theme::bg1()).into(),
                            border: iced::Border {
                                color: theme::accent_bright(),
                                width: 2.0,
                                radius: theme::ui_border_radius(),
                            },
                            selected_text_color: theme::bg0_hard(),
                            selected_background: (theme::accent_bright()).into(),
                            shadow: iced::Shadow::default(),
                        }
                    }),
            )
            .height(Length::Fixed(40.0)),
        ) // Match button and search field height
    };

    let sort_button = on_sort_toggle.map(|sort_msg| {
        let sort_icon_path = if sort_ascending {
            "assets/icons/arrow-up.svg"
        } else {
            "assets/icons/arrow-down.svg"
        };
        let tooltip_text = if sort_ascending {
            "Sort: Ascending"
        } else {
            "Sort: Descending"
        };
        header_icon_button(sort_icon_path, tooltip_text, sort_msg)
    });

    let refresh_button = on_refresh.map(|refresh_msg| {
        header_icon_button("assets/icons/refresh-cw.svg", "Refresh Data", refresh_msg)
    });

    // Optional shuffle button (only rendered if on_shuffle is provided)
    let shuffle_button = on_shuffle.map(|shuffle_msg| {
        header_icon_button("assets/icons/shuffle.svg", "Shuffle All", shuffle_msg)
    });

    // Optional center on playing button
    let center_button = on_center_on_playing.map(|center_msg| {
        header_icon_button("assets/icons/locate.svg", "Center on Playing", center_msg)
    });

    let search_field = if show_search {
        Some(crate::widgets::search_bar::search_bar(
            search_query,
            "Search...",
            search_input_id,
            on_search_change,
            None,
        ))
    } else {
        None
    };

    let count_text = if filtered_count > 0 && filtered_count < total_count {
        format!("{filtered_count} of {total_count} {item_type}")
    } else {
        format!("{total_count} {item_type}")
    };

    let count_display = text(count_text)
        .size(12.0) // Match other text sizes
        .font(Font {
            weight: Weight::Medium,
            ..theme::ui_font()
        })
        .color(theme::fg2())
        .width(Length::Shrink); // Take only needed space, not Fill

    // Build the row with conditionally included buttons
    let mut header_row = row![];
    if let Some(selector) = view_selector {
        header_row = header_row.push(selector);
    }
    if let Some(sort_btn) = sort_button {
        header_row = header_row.push(sort_btn);
    }
    if let Some(refresh_btn) = refresh_button {
        header_row = header_row.push(refresh_btn);
    }
    if let Some(shuffle_btn) = shuffle_button {
        header_row = header_row.push(shuffle_btn);
    }
    if let Some(center_btn) = center_button {
        header_row = header_row.push(center_btn);
    }
    if let Some(search_element) = search_field {
        header_row = header_row.push(search_element);
    } else {
        // Push empty space to force the count display to the right edge
        header_row = header_row.push(iced::widget::Space::new().width(Length::Fill));
    }
    header_row = header_row.push(count_display);

    container(
        header_row
            .spacing(8) // Reduced from 12 to avoid double-padding with element borders
            .padding(8)
            .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fixed(48.0))
    .style(theme::container_bg0_hard)
    .into()
}

/// Reusable header icon button with tooltip, hover overlay, and consistent styling.
///
/// Wraps an SVG icon in a 40×40 container with bg0_soft background, adds hover
/// overlay for interactive feedback, and positions a tooltip above the button.
fn header_icon_button<'a, Message: Clone + 'a>(
    icon_path: &str,
    tooltip_text: &str,
    on_press: Message,
) -> Element<'a, Message> {
    use iced::widget::{svg, tooltip};

    let icon_svg = crate::embedded_svg::svg_widget(icon_path)
        .width(Length::Fixed(20.0))
        .height(Length::Fixed(20.0))
        .style(|_theme, _status| svg::Style {
            color: Some(theme::fg0()),
        });

    tooltip(
        mouse_area(
            HoverOverlay::new(
                container(icon_svg)
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
                    .center(Length::Fixed(40.0)),
            )
            .border_radius(theme::ui_border_radius()),
        )
        .on_press(on_press)
        .interaction(iced::mouse::Interaction::Pointer),
        container(
            text(tooltip_text.to_string())
                .size(11.0)
                .font(theme::ui_font()),
        )
        .padding(4),
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
    fn header_icon_button_produces_element() {
        // Characterization test: the extracted helper compiles and produces a valid Element.
        let _el: Element<'_, String> = header_icon_button(
            "assets/icons/shuffle.svg",
            "Shuffle All",
            "test_press".to_string(),
        );
    }
}
