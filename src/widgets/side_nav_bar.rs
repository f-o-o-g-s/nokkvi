//! Side Navigation Bar — vertical nav tabs for Side layout mode
//!
//! Renders view tab buttons in a vertical column on the left edge of the app.
//! Each button uses a canvas widget with rotated text (-90°, reading bottom-to-top).
//! Styling matches the horizontal nav_bar exactly:
//!   - Rounded mode: accent text for active, accent right-edge indicator, fg1 on hover
//!   - Flat mode: filled accent_bright background for active, bg2 on hover
//!
//! Emits the same `NavBarMessage` variants as the horizontal nav bar.

use std::f32::consts::FRAC_PI_2;

use iced::{
    Background, Border, Color, Element, Length, Point, Rectangle, Vector,
    font::{Font, Weight},
    widget::{Space, button, canvas, column, container, row},
};
use nokkvi_data::types::player_settings::NavDisplayMode;

use super::{
    hover_indicator::{HoverExpand, HoverIndicator},
    nav_bar::{NAV_TABS, NavBarMessage, NavView, colored_icon, flat_tab_style},
};
use crate::theme;

/// Width of the side nav bar (px)
pub(crate) const SIDE_NAV_WIDTH: f32 = 28.0;

/// Height allocated for each tab button (enough for rotated text)
const TAB_HEIGHT: f32 = 72.0;

/// Height for icon-only tab buttons (smaller, no text rotation needed)
const ICON_TAB_HEIGHT: f32 = 36.0;

/// Height for text+icon tab buttons (icon above rotated text)
const TEXT_ICON_TAB_HEIGHT: f32 = 88.0;

/// Height of the icon slot within text+icon tabs
const ICON_SLOT_HEIGHT: f32 = 22.0;

/// Icon size in the side nav bar
const ICON_SIZE: f32 = 14.0;

/// Width of the active-tab indicator bar (right edge, rounded mode)
const INDICATOR_WIDTH: f32 = 2.5;

/// Data passed to the side nav bar for rendering
pub(crate) struct SideNavBarData {
    pub current_view: NavView,
    pub settings_open: bool,
}

/// Canvas program that draws rotated text with optional active/hover indicator.
struct RotatedLabel {
    label: &'static str,
    color: Color,
    /// If set, draw a right-edge accent indicator bar (active state)
    indicator_color: Option<Color>,
    /// If set, draw indicator on hover when no active indicator is shown
    hover_indicator_color: Option<Color>,
}

impl<Message> canvas::Program<Message> for RotatedLabel {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        // Draw right-edge indicator: active state always, hover state on mouse-over
        let show_indicator = self.indicator_color.or_else(|| {
            if cursor.is_over(bounds) {
                self.hover_indicator_color
            } else {
                None
            }
        });
        if let Some(accent) = show_indicator {
            frame.fill_rectangle(
                Point::new(bounds.width - INDICATOR_WIDTH, 0.0),
                iced::Size::new(INDICATOR_WIDTH, bounds.height),
                canvas::Fill::from(accent),
            );
        }

        // Translate to center, rotate -90°, draw text (reads bottom-to-top)
        let center = frame.center();
        frame.translate(Vector::new(center.x, center.y));
        frame.rotate(-FRAC_PI_2);

        frame.fill_text(canvas::Text {
            content: self.label.to_string(),
            position: Point::new(0.0, 0.0),
            color: self.color,
            size: iced::Pixels(12.0),
            font: Font {
                weight: Weight::Bold,
                ..theme::ui_font()
            },
            align_x: iced::alignment::Horizontal::Center.into(),
            align_y: iced::alignment::Vertical::Center,
            ..Default::default()
        });

        vec![frame.into_geometry()]
    }
}

/// Build side nav tab content based on display mode.
///
/// Returns `(content_element, tab_height)` — shared between `nav_tab` and the settings indicator
/// to avoid duplicating the display mode layout logic.
fn side_nav_tab_content(
    label: &'static str,
    icon_path: &'static str,
    display_mode: NavDisplayMode,
    text_color: Color,
    indicator_color: Option<Color>,
    hover_indicator_color: Option<Color>,
) -> (Element<'static, NavBarMessage>, f32) {
    match display_mode {
        NavDisplayMode::TextOnly => {
            let content = canvas(RotatedLabel {
                label,
                color: text_color,
                indicator_color,
                hover_indicator_color,
            })
            .width(Length::Fixed(SIDE_NAV_WIDTH))
            .height(Length::Fixed(TAB_HEIGHT))
            .into();
            (content, TAB_HEIGHT)
        }
        NavDisplayMode::IconsOnly => {
            let icon_container = container(colored_icon(icon_path, ICON_SIZE, text_color))
                .width(Length::Fill)
                .height(Length::Fixed(ICON_TAB_HEIGHT))
                .align_x(iced::Alignment::Center)
                .align_y(iced::Alignment::Center);

            // Right-edge indicator bar — shared canvas-based hover indicator
            let indicator = canvas(HoverIndicator {
                indicator_color,
                hover_indicator_color,
                expand: HoverExpand::left(SIDE_NAV_WIDTH - INDICATOR_WIDTH),
            })
            .width(Length::Fixed(INDICATOR_WIDTH))
            .height(Length::Fill);

            let content = row![icon_container, indicator]
                .width(Length::Fixed(SIDE_NAV_WIDTH))
                .height(Length::Fixed(ICON_TAB_HEIGHT))
                .into();
            (content, ICON_TAB_HEIGHT)
        }
        NavDisplayMode::TextAndIcons => {
            let icon_widget = container(colored_icon(icon_path, ICON_SIZE, text_color))
                .width(Length::Fixed(SIDE_NAV_WIDTH))
                .height(Length::Fixed(ICON_SLOT_HEIGHT))
                .align_x(iced::Alignment::Center)
                .align_y(iced::Alignment::End);

            let label_canvas = canvas(RotatedLabel {
                label,
                color: text_color,
                indicator_color,
                hover_indicator_color,
            })
            .width(Length::Fixed(SIDE_NAV_WIDTH))
            .height(Length::Fixed(TEXT_ICON_TAB_HEIGHT - ICON_SLOT_HEIGHT));

            let content = column![icon_widget, label_canvas]
                .spacing(0)
                .width(Length::Fixed(SIDE_NAV_WIDTH))
                .height(Length::Fixed(TEXT_ICON_TAB_HEIGHT))
                .into();
            (content, TEXT_ICON_TAB_HEIGHT)
        }
    }
}

/// Build the vertical side navigation bar
pub(crate) fn side_nav_bar(data: SideNavBarData) -> Element<'static, NavBarMessage> {
    let settings_open = data.settings_open;
    let current = data.current_view;
    let is_rounded = theme::is_rounded_mode();

    let active_accent = theme::active_accent();

    let nav_tab = |label: &'static str,
                   icon_path: &'static str,
                   view: NavView|
     -> Element<'_, NavBarMessage> {
        let is_active = !settings_open && current == view;
        let display_mode = theme::nav_display_mode();

        // Button style — flat mode uses shared flat_tab_style, rounded uses plain bg
        let tab_style = move |_theme: &iced::Theme, status: button::Status| {
            if is_rounded {
                button::Style {
                    background: Some(Background::Color(theme::bg0_hard())),
                    text_color: theme::fg2(),
                    ..button::Style::default()
                }
            } else {
                flat_tab_style(is_active)(_theme, status)
            }
        };

        // Determine text color based on active state
        let text_color = if is_rounded {
            if is_active {
                active_accent
            } else {
                theme::fg2()
            }
        } else if is_active {
            theme::bg0()
        } else {
            theme::fg2()
        };

        // Indicator drawn inside canvas for active rounded tabs
        let indicator_color = if is_rounded && is_active {
            Some(active_accent)
        } else {
            None
        };

        // Hover indicator: show accent bar on hover in rounded mode
        let hover_indicator_color = if is_rounded && !is_active {
            Some(active_accent)
        } else {
            None
        };

        let (content, tab_height) = side_nav_tab_content(
            label,
            icon_path,
            display_mode,
            text_color,
            indicator_color,
            hover_indicator_color,
        );

        button(content)
            .on_press(NavBarMessage::SwitchView(view))
            .padding(0)
            .width(Length::Fixed(SIDE_NAV_WIDTH))
            .height(Length::Fixed(tab_height))
            .style(tab_style)
            .into()
    };

    // Separator line between tabs (horizontal line in vertical layout)
    let separator = || -> Element<'_, NavBarMessage> {
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(2.0))
            .style(move |_| container::Style {
                background: if is_rounded {
                    None
                } else {
                    Some(theme::bg1().into())
                },
                ..Default::default()
            })
            .into()
    };

    // Settings indicator when settings are open (non-interactive)
    let settings_indicator: Option<Element<'_, NavBarMessage>> = if settings_open {
        let display_mode = theme::nav_display_mode();
        let text_color = if is_rounded {
            active_accent
        } else {
            theme::bg0()
        };
        let bg = if is_rounded {
            theme::bg0_hard()
        } else {
            theme::accent_bright()
        };
        let indicator_color = if is_rounded {
            Some(active_accent)
        } else {
            None
        };

        let (settings_content, tab_height) = side_nav_tab_content(
            "Settings",
            "assets/icons/settings.svg",
            display_mode,
            text_color,
            indicator_color,
            None, // No hover indicator — settings tab is always active
        );

        Some(
            container(settings_content)
                .width(Length::Fixed(SIDE_NAV_WIDTH))
                .height(Length::Fixed(tab_height))
                .style(move |_: &iced::Theme| container::Style {
                    background: Some(Background::Color(bg)),
                    border: Border {
                        radius: theme::ui_border_radius(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .into(),
        )
    } else {
        None
    };

    // Build vertical column of tabs from shared NAV_TABS
    let mut tabs = column![].spacing(0).width(Length::Fixed(SIDE_NAV_WIDTH));
    for &(label, icon_path, view) in NAV_TABS {
        tabs = tabs.push(nav_tab(label, icon_path, view)).push(separator());
    }

    if let Some(indicator) = settings_indicator {
        tabs = tabs.push(indicator).push(separator());
    }

    // Fill remaining space
    tabs = tabs.push(Space::new().height(Length::Fill));

    // Right edge separator (vertical line)
    let right_edge: Element<'_, NavBarMessage> = container(Space::new())
        .width(Length::Fixed(2.0))
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(theme::bg1().into()),
            ..Default::default()
        })
        .into();

    container(
        row![
            container(tabs)
                .width(Length::Fixed(SIDE_NAV_WIDTH))
                .height(Length::Fill)
                .style(theme::container_bg0_hard),
            right_edge,
        ]
        .spacing(0)
        .height(Length::Fill),
    )
    .width(Length::Fixed(SIDE_NAV_WIDTH + 2.0))
    .height(Length::Fill)
    .into()
}
