use iced::{
    Alignment, Element, Length,
    font::Weight,
    widget::{container, mouse_area, stack, text_input},
};

use crate::theme;

/// Default text-input style used by the view-header search bar (flat
/// redesign). Flat mode: 1 px `theme::border()` outline, `theme::bg0_soft()`
/// fill. Rounded mode: pill outline (`theme::ui_radius_pill()`) with
/// `theme::bg0()` fill so the input reads as an inset capsule against the
/// view header's pill chrome.
///
/// Kept inside `search_bar.rs` so the flat-redesign defaults stay
/// co-located with the call site; callers wanting a different look pass
/// `Some(style_fn)` via the `style` parameter.
pub(crate) fn flat_search_input_style(
    _theme: &iced::Theme,
    status: text_input::Status,
) -> text_input::Style {
    let bg = if theme::is_rounded_mode() {
        theme::bg0()
    } else {
        theme::bg0_soft()
    };
    text_input::Style {
        background: bg.into(),
        border: iced::Border {
            color: if matches!(status, text_input::Status::Focused { .. }) {
                theme::accent_bright()
            } else {
                theme::border()
            },
            width: 1.0,
            radius: theme::ui_radius_pill(),
        },
        icon: theme::fg4(),
        placeholder: theme::fg4(),
        value: theme::fg0(),
        selection: theme::selection_color(),
    }
}

/// A reusable search bar widget that visually integrates a magnifying glass icon
/// on the left and a conditional clear button on the right inside the input bounds,
/// preserving native focus borders and styles.
pub(crate) fn search_bar<'a, Message: Clone + 'a>(
    query: &str,
    placeholder: &str,
    input_id: &'static str,
    on_change: impl Fn(String) -> Message + 'a,
    style: Option<
        fn(&iced::Theme, iced::widget::text_input::Status) -> iced::widget::text_input::Style,
    >,
) -> Element<'a, Message> {
    // Generate the "clear" message proactively since the closure will be moved
    let clear_msg = on_change(String::new());

    // We manually increase L and R padding to give the overlay icons breathing room.
    // Standard padding is 8px. 32px leaves 24px for the icon + spacing.
    let input = text_input(placeholder, query)
        .id(input_id)
        .on_input(on_change)
        .width(Length::Fill)
        .padding(iced::Padding {
            top: 8.0,
            right: 32.0,
            bottom: 8.0,
            left: 32.0,
        })
        .size(12.0)
        .font(theme::weighted_ui_font(Weight::Medium))
        .style(style.unwrap_or(flat_search_input_style));

    // Left magnifying glass icon
    let search_icon = crate::embedded_svg::svg_widget("assets/icons/search.svg")
        .width(Length::Fixed(16.0))
        .height(Length::Fixed(16.0))
        .style(|_, _| iced::widget::svg::Style {
            color: Some(theme::fg4()),
        });

    let search_icon_container = container(search_icon)
        .height(Length::Fill)
        .padding([0, 8]) // 8px from left edge
        .align_y(Alignment::Center)
        .align_x(Alignment::Start);

    // If query is present, show clear button, else just search icon
    if query.is_empty() {
        container(stack![
            input,
            container(search_icon_container)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Start)
        ])
        .width(Length::Fill)
        .into()
    } else {
        // Right clear icon button
        let clear_icon = crate::embedded_svg::svg_widget("assets/icons/x.svg")
            .width(Length::Fixed(16.0))
            .height(Length::Fixed(16.0))
            .style(|_, _| iced::widget::svg::Style {
                color: Some(theme::fg4()),
            });

        // The interactive button area
        let clear_button = mouse_area(clear_icon)
            .on_press(clear_msg)
            .interaction(iced::mouse::Interaction::Pointer);

        let clear_container = container(clear_button)
            .height(Length::Fill)
            .padding([0, 8]) // 8px from right edge
            .align_y(Alignment::Center)
            .align_x(Alignment::End);

        container(stack![
            input,
            container(search_icon_container)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Start),
            container(clear_container)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::End)
        ])
        .width(Length::Fill)
        .into()
    }
}
