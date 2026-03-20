use iced::{
    Alignment, Element, Length,
    font::{Font, Weight},
    widget::{button, container, pick_list, row, svg, text, text_input},
};
// Re-export SortMode from data crate (canonical definition)
pub(crate) use nokkvi_data::types::sort_mode::SortMode;

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
    on_sort_toggle: Message,
    on_shuffle: Option<Message>, // Optional shuffle button
    on_search_change: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    let view_selector = container(
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
    .height(Length::Fixed(40.0)); // Match button and search field height

    // Use Lucide SVG icons matching QML/Slint implementation
    let sort_icon_path = if sort_ascending {
        "assets/icons/arrow-up.svg"
    } else {
        "assets/icons/arrow-down.svg"
    };

    let sort_svg = crate::embedded_svg::svg_widget(sort_icon_path)
        .width(Length::Fixed(20.0))
        .height(Length::Fixed(20.0))
        .style(|_theme, _status| svg::Style {
            color: Some(theme::fg0()),
        });

    let sort_button = button(sort_svg)
        .width(Length::Fixed(40.0))
        .height(Length::Fixed(40.0))
        .style(|_theme, status| button::Style {
            background: Some((theme::bg0_soft()).into()),
            text_color: theme::fg0(),
            border: iced::Border {
                color: if matches!(status, button::Status::Hovered) {
                    theme::accent_bright()
                } else {
                    iced::Color::TRANSPARENT
                },
                width: 2.0,
                radius: theme::ui_border_radius(),
            },
            ..button::Style::default()
        })
        .on_press(on_sort_toggle);

    // Optional shuffle button (only rendered if on_shuffle is provided)
    let shuffle_button = on_shuffle.map(|shuffle_msg| {
        let shuffle_svg = crate::embedded_svg::svg_widget("assets/icons/shuffle.svg")
            .width(Length::Fixed(20.0))
            .height(Length::Fixed(20.0))
            .style(|_theme, _status| svg::Style {
                color: Some(theme::fg0()),
            });

        button(shuffle_svg)
            .width(Length::Fixed(40.0))
            .height(Length::Fixed(40.0))
            .style(|_theme, status| button::Style {
                background: Some((theme::bg0_soft()).into()),
                text_color: theme::fg0(),
                border: iced::Border {
                    color: if matches!(status, button::Status::Hovered) {
                        theme::accent_bright()
                    } else {
                        iced::Color::TRANSPARENT
                    },
                    width: 2.0,
                    radius: theme::ui_border_radius(),
                },
                ..button::Style::default()
            })
            .on_press(shuffle_msg)
    });

    let search_field = container(
        text_input("Search...", search_query)
            .id(search_input_id) // Use unique ID per view to prevent focus transfer
            .on_input(on_search_change)
            .width(Length::Fill)
            .padding(8)
            .size(12.0) // Match QML font size
            .font(Font {
                weight: Weight::Medium,
                ..theme::ui_font()
            })
            .style(theme::search_input_style),
    )
    .height(Length::Fixed(40.0)) // Match button and pick_list height
    .width(Length::Fill);

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

    // Build the row with conditionally included shuffle button
    let mut header_row = row![view_selector, sort_button];
    if let Some(shuffle_btn) = shuffle_button {
        header_row = header_row.push(shuffle_btn);
    }
    header_row = header_row.push(search_field).push(count_display);

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
