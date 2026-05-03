//! Slot rendering functions for the settings slot list.
//!
//! All rendering of individual slot list slots is extracted here:
//! - Main settings slots (headers, items)
//! - Color sub-list slots (gradient editing)

use iced::{
    Alignment, Border, Color, Element, Length, Padding,
    font::{Font, Weight},
    widget::{Space, button, column, container, row, svg, text, text::Wrapping, text_input},
};

use super::{
    SettingsMessage,
    items::{self, SettingItem, SettingValue, SettingsEntry},
};
use crate::{embedded_svg, theme, widgets::slot_list};

// ============================================================================
// Shared Helpers
// ============================================================================

/// Transparent button style — no background, no border. Used for clickable
/// slots that should look like plain content, not raised buttons.
pub(crate) fn transparent_button_style(
    _theme: &iced::Theme,
    _status: button::Status,
) -> button::Style {
    button::Style {
        background: None,
        border: Border::default(),
        ..Default::default()
    }
}

/// Render a badge-style value container with bg0_hard background.
/// Used for Hotkey combos, Float/Int/Text values — anything that should
/// appear in a pill-shaped container.
fn render_badge<'a>(
    display_text: String,
    font_size: f32,
    is_center: bool,
    opacity: f32,
) -> Element<'a, SettingsMessage> {
    let text_color = if is_center {
        theme::fg0()
    } else {
        Color {
            a: opacity,
            ..theme::fg0()
        }
    };
    let badge_bg = if is_center {
        theme::bg0_hard()
    } else {
        Color {
            a: opacity * 0.3,
            ..theme::bg0_hard()
        }
    };
    let badge_border = if is_center {
        theme::fg4()
    } else {
        Color {
            a: opacity * 0.4,
            ..theme::fg4()
        }
    };
    let badge_size = font_size * 0.95;

    container(slot_list::slot_list_text(
        display_text,
        badge_size,
        text_color,
    ))
    .padding(Padding::new(2.0).left(8.0).right(8.0))
    .style(move |_theme| container::Style {
        background: Some(badge_bg.into()),
        border: Border {
            color: badge_border,
            width: 1.0,
            radius: theme::ui_border_radius(),
        },
        ..Default::default()
    })
    .into()
}

/// Render an inline hex color editor (text input + preview swatch).
/// Shared between the main slot list (HexColor items) and the color sub-list.
fn render_hex_editor<'a>(
    hex_input: &str,
    font_size: f32,
    swatch_size: f32,
) -> Element<'a, SettingsMessage> {
    let input = text_input("e.g. #458588", hex_input)
        .id(super::HEX_EDITOR_INPUT_ID)
        .on_input(SettingsMessage::HexInputChanged)
        .on_submit(SettingsMessage::HexInputSubmit)
        .size(font_size)
        .width(Length::Fill)
        .font(Font {
            weight: Weight::Medium,
            ..theme::ui_font()
        });

    let preview_color = crate::theme_config::parse_hex_color(hex_input).unwrap_or_else(theme::fg4);
    let preview_swatch = container(Space::new())
        .width(Length::Fixed(swatch_size))
        .height(Length::Fixed(swatch_size))
        .style(move |_theme| container::Style {
            background: Some(preview_color.into()),
            border: Border {
                color: Color {
                    a: 0.5,
                    ..theme::fg4()
                },
                width: 1.0,
                radius: theme::ui_border_radius(),
            },
            ..Default::default()
        });

    row![preview_swatch, input]
        .spacing(6)
        .align_y(Alignment::Center)
        .into()
}

// ============================================================================
// Slot Rendering
// ============================================================================

/// Shared context for all slot rendering functions
pub(crate) struct SlotRenderContext<'a> {
    pub item_index: usize,
    pub is_center: bool,
    pub opacity: f32,
    pub row_height: f32,
    pub scale_factor: f32,
    /// Whether this slot's hotkey is in capture mode (center item only)
    pub is_capturing: bool,
    /// Conflict warning text to display instead of the combo badge
    pub conflict_text: Option<&'a str>,
    /// Whether we're rendering at Level 1 (category picker) — centers headers
    pub is_level1: bool,
    /// Index of the keyboard-cursored badge within a ToggleSet (center row only)
    pub toggle_cursor: Option<usize>,
}

/// Render a single settings slot list slot (either header or item)
pub(crate) fn render_settings_slot<'a>(
    ctx: &SlotRenderContext<'_>,
    entry: &SettingsEntry,
    is_editing: bool,
    is_collapsed: bool,
    hex_input: &str,
) -> Element<'a, SettingsMessage> {
    match entry {
        SettingsEntry::Header { label, icon } => render_header_slot(ctx, label, icon, is_collapsed),
        SettingsEntry::Item(item) => render_item_slot(ctx, item, is_editing, hex_input),
    }
}

/// Render a section header slot — flat on the panel, text-color-only highlighting.
///
/// - **Center slot**: Bright text (accent_bright) to indicate focus
/// - **Non-center slots**: Dimmed text (opacity-scaled)
///
/// No per-row background or border — the panel provides the visual container.
fn render_header_slot<'a>(
    ctx: &SlotRenderContext<'_>,
    label: &'static str,
    icon_path: &'static str,
    is_collapsed: bool,
) -> Element<'a, SettingsMessage> {
    let font_size =
        nokkvi_data::utils::scale::calculate_font_size(16.0, ctx.row_height, ctx.scale_factor)
            * ctx.scale_factor;
    let icon_size = (font_size * 1.2).clamp(10.0, 20.0);
    let chevron_size = (font_size * 1.0).clamp(8.0, 16.0);

    // Text-color-only highlighting: bright for center, dimmed for others
    let text_color = if ctx.is_center {
        theme::accent_bright()
    } else {
        Color {
            a: ctx.opacity,
            ..theme::fg2()
        }
    };

    let section_icon = embedded_svg::svg_widget(icon_path)
        .width(Length::Fixed(icon_size))
        .height(Length::Fixed(icon_size))
        .style(move |_theme, _status| svg::Style {
            color: Some(text_color),
        });

    // Collapse/expand chevron indicator
    let chevron_path = if is_collapsed {
        "assets/icons/chevron-right.svg"
    } else {
        "assets/icons/chevron-down.svg"
    };
    let chevron = embedded_svg::svg_widget(chevron_path)
        .width(Length::Fixed(chevron_size))
        .height(Length::Fixed(chevron_size))
        .style(move |_theme, _status| svg::Style {
            color: Some(text_color),
        });

    // Layout varies by level:
    // - Level 1: centered text, no chevron (category picker items)
    // - Level 2: left-aligned with chevron (section separators)
    let content: Element<'a, SettingsMessage> = if ctx.is_level1 {
        row![
            section_icon,
            text(label)
                .size(font_size)
                .font(Font {
                    weight: Weight::Bold,
                    ..theme::ui_font()
                })
                .color(text_color)
                .wrapping(Wrapping::None),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
    } else {
        row![
            Space::new().width(Length::Fixed(8.0)),
            chevron,
            section_icon,
            text(label)
                .size(font_size)
                .font(Font {
                    weight: Weight::Bold,
                    ..theme::ui_font()
                })
                .color(text_color)
                .wrapping(Wrapping::None),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into()
    };

    // Level 2 headers get a darker background + lighter border to stand out from the panel
    let (header_bg, header_border) = if ctx.is_level1 {
        (Color::TRANSPARENT, Color::TRANSPARENT)
    } else if ctx.is_center {
        (theme::bg0_hard(), theme::bg2())
    } else {
        (
            Color {
                a: ctx.opacity,
                ..theme::bg0_hard()
            },
            Color {
                a: ctx.opacity * 0.5,
                ..theme::bg2()
            },
        )
    };
    let align_x = if ctx.is_level1 {
        Alignment::Center
    } else {
        Alignment::Start
    };
    let styled = container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .clip(true)
        .align_y(Alignment::Center)
        .align_x(align_x)
        .padding(Padding::new(4.0).left(8.0))
        .style(move |_: &iced::Theme| container::Style {
            background: Some(header_bg.into()),
            border: Border {
                color: header_border,
                width: 1.0,
                radius: theme::ui_border_radius(),
            },
            ..Default::default()
        });

    // Level 2 headers get a bottom separator line (visual only, not a slot list entry)
    let with_separator: Element<'a, SettingsMessage> = if !ctx.is_level1 {
        let sep_color = theme::bg2();
        column![
            container(styled).width(Length::Fill).height(Length::Fill),
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fixed(1.0))
                .style(move |_: &iced::Theme| container::Style {
                    background: Some(sep_color.into()),
                    ..Default::default()
                }),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    } else {
        styled.into()
    };

    // At Level 1, headers are interactive drill-down targets (clickable).
    // At Level 2, headers are non-interactive section separators (inert).
    let header_btn = button(with_separator)
        .style(transparent_button_style)
        .padding(0)
        .width(Length::Fill);

    if ctx.is_level1 {
        header_btn
            .on_press(if ctx.is_center {
                SettingsMessage::EditActivate
            } else {
                SettingsMessage::SlotListClickItem(ctx.item_index)
            })
            .into()
    } else {
        // Level 2: no on_press — headers are non-interactive
        header_btn.into()
    }
}

/// Render a setting item slot — flat on the panel, text-color-only highlighting.
///
/// - **Center slot**: Bright label (accent_bright), saturated values
/// - **Non-center slots**: Dimmed label + muted values (opacity-scaled)
/// - **Edit mode**: Subtle accent underline bar at bottom
///
/// No per-row background or border — the panel provides the visual container.
fn render_item_slot<'a>(
    ctx: &SlotRenderContext<'_>,
    item: &SettingItem,
    is_editing: bool,
    hex_input: &str,
) -> Element<'a, SettingsMessage> {
    let label_size =
        nokkvi_data::utils::scale::calculate_font_size(14.0, ctx.row_height, ctx.scale_factor)
            * ctx.scale_factor;
    let value_size =
        nokkvi_data::utils::scale::calculate_font_size(13.0, ctx.row_height, ctx.scale_factor)
            * ctx.scale_factor;
    let category_size =
        nokkvi_data::utils::scale::calculate_font_size(10.0, ctx.row_height, ctx.scale_factor)
            * ctx.scale_factor;

    // Mockup-style: accent label on selected row, bold when centered, row gets highlight bg
    let label_color = if ctx.is_center {
        theme::accent_bright()
    } else {
        Color {
            a: ctx.opacity,
            ..theme::fg0()
        }
    };
    let label_weight = if ctx.is_center {
        Weight::Bold
    } else {
        Weight::Normal
    };
    let subtext_color = Color {
        a: ctx.opacity * 0.5,
        ..theme::fg3()
    };

    // Build a lightweight style struct for render_value_display compatibility
    let style = slot_list::SlotListSlotStyle {
        text_color: label_color,
        subtext_color,
        bg_color: Color::TRANSPARENT,
        border_color: Color::TRANSPARENT,
        border_width: 0.0,
        border_radius: 0.0.into(),
        hover_text_color: crate::theme::accent_bright(),
    };

    // ── Special layout for preset, restore, and action items ──────────
    let key_ref = item.key.as_ref();
    let is_action_item = items::is_preset_key(key_ref)
        || items::is_restore_key(key_ref)
        || items::is_action_key(key_ref);

    let content: Element<'a, SettingsMessage> = if is_action_item {
        let description = item.value.display();

        // Manual text column (no slot_list_text_column dependency on SlotListSlotStyle)
        let title_text = text(item.label.clone())
            .size(label_size)
            .font(Font {
                weight: label_weight,
                ..theme::ui_font()
            })
            .color(label_color)
            .wrapping(Wrapping::None);
        let desc_text = text(description)
            .size(category_size)
            .font(theme::ui_font())
            .color(subtext_color)
            .wrapping(Wrapping::None);
        let text_col = container(column![title_text, desc_text].spacing(2))
            .width(Length::FillPortion(35))
            .height(Length::Fill)
            .clip(true)
            .align_y(Alignment::Center);

        // Right-side hint
        let hint_text = if ctx.is_center { "Enter ↵" } else { "" };
        let hint_col = container(
            text(hint_text)
                .size(category_size)
                .font(theme::ui_font())
                .color(subtext_color),
        )
        .width(Length::FillPortion(65))
        .height(Length::Fill)
        .clip(true)
        .align_y(Alignment::Center)
        .align_x(Alignment::End)
        .padding(Padding::new(0.0).right(12.0));

        row![Space::new().width(Length::Fixed(28.0)), text_col, hint_col,]
            .spacing(8)
            .align_y(Alignment::Center)
            .height(Length::Fill)
            .into()
    } else {
        // ── Standard item layout ─────────────────────────────────────
        let label_text = text(item.label.clone())
            .size(label_size)
            .font(Font {
                weight: label_weight,
                ..theme::ui_font()
            })
            .color(label_color)
            .wrapping(Wrapping::None);

        // Build label row: text + optional inline icon
        let label_row: Element<'a, SettingsMessage> = if let Some(icon_path) = item.label_icon {
            let icon_size = (label_size * 1.0).clamp(10.0, 18.0);
            let icon_color = label_color;
            let inline_icon = embedded_svg::svg_widget(icon_path)
                .width(Length::Fixed(icon_size))
                .height(Length::Fixed(icon_size))
                .style(move |_theme, _status| svg::Style {
                    color: Some(icon_color),
                });
            row![label_text, inline_icon]
                .spacing(5)
                .align_y(Alignment::Center)
                .into()
        } else {
            label_text.into()
        };

        let label_col = container(label_row)
            .width(Length::FillPortion(35))
            .height(Length::Fill)
            .clip(true)
            .align_y(Alignment::Center);

        // Determine if this item needs an "Enter ↵" hint:
        // Hotkey, HexColor, ColorArray always need Enter to activate.
        // Specific Text items (font picker, local music path) also need Enter.
        let needs_enter_hint = matches!(
            item.value,
            SettingValue::Hotkey(_) | SettingValue::HexColor(_) | SettingValue::ColorArray(_)
        ) || (matches!(item.value, SettingValue::Text(_))
            && matches!(key_ref, "theme.font.family" | "general.local_music_path"));
        let show_hint = needs_enter_hint && ctx.is_center && !is_editing && !ctx.is_capturing;

        // Value column (right 65%)
        let value_display =
            render_value_display(&item.value, value_size, &style, ctx, is_editing, hex_input);

        let value_content: Element<'a, SettingsMessage> = if show_hint {
            let hint = text("Enter ↵")
                .size(category_size)
                .font(theme::ui_font())
                .color(subtext_color);
            row![value_display, Space::new().width(Length::Fill), hint]
                .align_y(Alignment::Center)
                .into()
        } else {
            value_display
        };

        let value_col = container(value_content)
            .width(Length::FillPortion(65))
            .height(Length::Fill)
            .clip(true)
            .align_y(Alignment::Center)
            .align_x(Alignment::Start)
            .padding(Padding::new(0.0).right(12.0));

        row![
            Space::new().width(Length::Fixed(28.0)),
            label_col,
            value_col,
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .height(Length::Fill)
        .into()
    };

    // Subtle row highlight for center (selected) row
    let row_bg = if ctx.is_center {
        Color {
            a: 0.4,
            ..theme::bg1()
        }
    } else {
        Color::TRANSPARENT
    };
    let styled = container(content)
        .width(Length::Fill)
        .clip(true)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(row_bg.into()),
            ..Default::default()
        });

    // Bottom separator line — visual only, not a slot list entry
    let sep_color = theme::bg2();
    let with_separator = column![
        container(styled).width(Length::Fill).height(Length::Fill),
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(1.0))
            .style(move |_: &iced::Theme| container::Style {
                background: Some(sep_color.into()),
                ..Default::default()
            }),
    ]
    .width(Length::Fill)
    .height(Length::Fill);

    // Make clickable — center click enters edit mode, other slots navigate
    button(with_separator)
        .on_press(if ctx.is_center {
            SettingsMessage::EditActivate
        } else {
            SettingsMessage::SlotListClickItem(ctx.item_index)
        })
        .style(transparent_button_style)
        .padding(0)
        .width(Length::Fill)
        .into()
}

/// Render the value display based on SettingValue type
fn render_value_display<'a>(
    value: &SettingValue,
    font_size: f32,
    style: &slot_list::SlotListSlotStyle,
    ctx: &SlotRenderContext<'_>,
    is_editing: bool,
    hex_input: &str,
) -> Element<'a, SettingsMessage> {
    let is_center = ctx.is_center;
    let opacity = ctx.opacity;

    let value_widget: Element<'a, SettingsMessage> = match value {
        SettingValue::Bool(v) => {
            // SM-style: show all options as plain text, selected gets underline
            render_sm_options(
                &["On", "Off"],
                if *v { "On" } else { "Off" },
                font_size,
                is_center,
                opacity,
            )
        }

        SettingValue::HexColor(hex) => {
            let parsed_color = crate::theme_config::parse_hex_color(hex).unwrap_or_else(theme::fg4);
            let eff_opacity = if is_center { 1.0 } else { opacity };
            let swatch_size = (font_size * 1.2).clamp(12.0, 20.0);

            if is_editing {
                // Inline text input for hex editing in the main slot list
                let swatch_size = (font_size * 1.2).clamp(12.0, 20.0);
                render_hex_editor(hex_input, font_size, swatch_size)
            } else {
                row![
                    // Color swatch
                    container(Space::new())
                        .width(Length::Fixed(swatch_size))
                        .height(Length::Fixed(swatch_size))
                        .style(move |_theme| container::Style {
                            background: Some(
                                Color {
                                    a: eff_opacity,
                                    ..parsed_color
                                }
                                .into()
                            ),
                            border: Border {
                                color: Color {
                                    a: eff_opacity * 0.5,
                                    ..theme::fg4()
                                },
                                width: 1.0,
                                radius: theme::ui_border_radius(),
                            },
                            ..Default::default()
                        }),
                    slot_list::slot_list_text(hex.clone(), font_size * 0.9, style.subtext_color),
                ]
                .spacing(6)
                .align_y(Alignment::Center)
                .into()
            }
        }

        SettingValue::ColorArray(colors) => {
            let eff_opacity = if is_center { 1.0 } else { opacity };
            let swatch_size = (font_size * 0.9).clamp(8.0, 14.0);

            // Show mini color swatches for each color in the gradient
            let mut r = row![].spacing(2).align_y(Alignment::Center);
            for hex in colors.iter().take(8) {
                let parsed = crate::theme_config::parse_hex_color(hex).unwrap_or_else(theme::fg4);
                r = r.push(
                    container(Space::new())
                        .width(Length::Fixed(swatch_size))
                        .height(Length::Fixed(swatch_size))
                        .style(move |_theme| container::Style {
                            background: Some(
                                Color {
                                    a: eff_opacity,
                                    ..parsed
                                }
                                .into(),
                            ),
                            border: Border {
                                color: Color {
                                    a: eff_opacity * 0.3,
                                    ..theme::fg4()
                                },
                                width: 0.5,
                                radius: theme::ui_border_radius(),
                            },
                            ..Default::default()
                        }),
                );
            }
            // Append count label
            let count = container(slot_list::slot_list_text(
                format!("{}", colors.len()),
                font_size * 0.8,
                style.subtext_color,
            ));
            r = r.push(count);
            r.into()
        }

        SettingValue::Enum { val, options } => {
            // SM-style: show all options as plain text, selected gets underline
            render_sm_options(options, val, font_size, is_center, opacity)
        }

        SettingValue::ToggleSet(items) => {
            // Multi-select: each badge independently toggleable
            render_toggle_set(
                items,
                font_size,
                is_center,
                opacity,
                if is_center { ctx.toggle_cursor } else { None },
            )
        }

        SettingValue::Hotkey(combo) => {
            // Capture mode: show "Press a key..." or conflict warning
            if ctx.is_capturing && ctx.is_center {
                if let Some(conflict) = ctx.conflict_text {
                    // Conflict warning: inverted red badge (red bg, dark text)
                    let text_color = theme::bg0_hard();
                    let badge_bg = theme::danger_bright();
                    let badge_border = theme::danger();
                    let badge_size = font_size * 0.9;
                    return container(slot_list::slot_list_text(conflict, badge_size, text_color))
                        .padding(Padding::new(2.0).left(8.0).right(8.0))
                        .style(move |_theme| container::Style {
                            background: Some(badge_bg.into()),
                            border: Border {
                                color: badge_border,
                                width: 1.0,
                                radius: theme::ui_border_radius(),
                            },
                            ..Default::default()
                        })
                        .into();
                }
                // Capture mode prompt: inverted yellow badge (yellow bg, dark text)
                let text_color = theme::bg0_hard();
                let hint_color = Color {
                    a: 0.7,
                    ..theme::bg0_hard()
                };
                let badge_bg = theme::warning();
                let badge_border = theme::warning_bright();
                let badge_size = font_size * 0.9;
                let hint_size = font_size * 0.7;
                return container(
                    row![
                        slot_list::slot_list_text("Press a key...", badge_size, text_color),
                        Space::new().width(Length::Fixed(8.0)),
                        slot_list::slot_list_text("Esc cancel · Del reset", hint_size, hint_color),
                    ]
                    .align_y(Alignment::Center),
                )
                .padding(Padding::new(2.0).left(8.0).right(8.0))
                .style(move |_theme| container::Style {
                    background: Some(badge_bg.into()),
                    border: Border {
                        color: badge_border,
                        width: 1.0,
                        radius: theme::ui_border_radius(),
                    },
                    ..Default::default()
                })
                .into();
            }

            // Normal display: key combo in a badge-style container
            // Use fg0 for text (always contrasts with bg) and bg0_hard for badge bg (maximum separation)
            render_badge(combo.clone(), font_size, is_center, opacity)
        }

        _ => {
            // Float, Int, Text — badge with bg0_hard background
            render_badge(value.display(), font_size, is_center, opacity)
        }
    };

    // Show chevron arrows for numeric values only — Bool/Enum use SM-style clickable options instead
    if value.is_incrementable() {
        let arrow_icon_size = (font_size * 0.85).clamp(10.0, 18.0);
        // Arrow color: bright accent when centered (interactive hint), dimmed otherwise
        let arrow_color = if is_center {
            theme::accent_bright()
        } else {
            Color {
                a: opacity * 0.4,
                ..theme::fg4()
            }
        };

        // Pressed background: subtle accent pill behind the chevron
        let pressed_bg = Color {
            a: 0.2,
            ..theme::accent_bright()
        };
        let arrow_btn_style = move |_theme: &iced::Theme, status: button::Status| {
            let bg = if matches!(status, button::Status::Pressed) {
                Some(pressed_bg.into())
            } else {
                None
            };
            button::Style {
                background: bg,
                border: Border {
                    radius: 99.0.into(),
                    ..Border::default()
                },
                ..Default::default()
            }
        };

        let mut left_arrow = button(
            embedded_svg::svg_widget("assets/icons/chevron-left.svg")
                .width(Length::Fixed(arrow_icon_size))
                .height(Length::Fixed(arrow_icon_size))
                .style(move |_theme, _status| svg::Style {
                    color: Some(arrow_color),
                }),
        )
        .style(arrow_btn_style)
        .padding(0);

        let mut right_arrow = button(
            embedded_svg::svg_widget("assets/icons/chevron-right.svg")
                .width(Length::Fixed(arrow_icon_size))
                .height(Length::Fixed(arrow_icon_size))
                .style(move |_theme, _status| svg::Style {
                    color: Some(arrow_color),
                }),
        )
        .style(arrow_btn_style)
        .padding(0);

        // Only make arrows interactive on the center (selected) row.
        // EditLeft/EditRight always act on the center item, so firing
        // them from a non-center row would modify the wrong setting.
        if is_center {
            left_arrow = left_arrow.on_press(SettingsMessage::EditLeft);
            right_arrow = right_arrow.on_press(SettingsMessage::EditRight);
        }

        row![
            left_arrow,
            Space::new().width(Length::Fixed(4.0)),
            value_widget,
            Space::new().width(Length::Fixed(4.0)),
            right_arrow,
        ]
        .align_y(Alignment::Center)
        .into()
    } else {
        value_widget
    }
}

// ============================================================================
// StepMania-Style Value Display
// ============================================================================

/// Render all possible values as plain text with underline cursor on the selected value.
///
/// Three visual states:
/// - **Not selected, any row**: dimmed text, no underline
/// - **Selected, non-center row**: medium text + subtle underline bar
/// - **Selected, center row**: bright accent text + accent underline bar
///
/// Each option is a clickable button sending `EditSetValue`.
fn render_sm_options<'a>(
    options: &[&'a str],
    selected: &str,
    font_size: f32,
    is_center: bool,
    opacity: f32,
) -> Element<'a, SettingsMessage> {
    let opt_size = font_size * 0.75;
    let underline_height = 2.0;

    let mut r = row![].spacing(22).align_y(Alignment::Center);

    for &option in options {
        let is_selected = option == selected;

        // Mockup style: ALL option text same color, only underlines get accent
        let text_color = if is_center {
            Color {
                a: if is_selected { 1.0 } else { 0.5 },
                ..theme::fg0()
            }
        } else {
            Color {
                a: opacity * if is_selected { 0.8 } else { 0.35 },
                ..theme::fg0()
            }
        };

        // Underline color (only for selected)
        let underline_color = if is_selected {
            if is_center {
                theme::accent_bright()
            } else {
                Color {
                    a: opacity * 0.6,
                    ..theme::accent_bright()
                }
            }
        } else {
            Color::TRANSPARENT
        };

        let font_weight = if is_selected {
            Weight::Bold
        } else {
            Weight::Normal
        };

        let label = text(option)
            .size(opt_size)
            .font(Font {
                weight: font_weight,
                ..theme::ui_font()
            })
            .color(text_color)
            .wrapping(Wrapping::None);

        // Underline hugs the text width: Shrink-width column makes Fill
        // expand only to the text's natural width, not the full container.
        let underline = container(Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(underline_height))
            .style(move |_: &iced::Theme| container::Style {
                background: Some(underline_color.into()),
                ..Default::default()
            });

        let option_col = column![label, underline]
            .spacing(1)
            .width(Length::Shrink)
            .align_x(Alignment::Center);

        let option_str = option.to_string();
        let mut option_btn = button(option_col)
            .style(transparent_button_style)
            .padding(Padding::new(2.0).left(4.0).right(4.0));

        // Only make option buttons interactive on the center (selected) row.
        // EditSetValue always acts on the center item, so firing it from a
        // non-center row would modify the wrong setting.
        if is_center {
            option_btn = option_btn.on_press(SettingsMessage::EditSetValue(option_str));
        }

        r = r.push(option_btn);
    }

    container(r).width(Length::Fill).clip(true).into()
}

/// Render a multi-select toggle set as independently clickable badges.
///
/// Mirrors `render_sm_options` visually but each badge toggles independently.
/// - **Enabled**: bold text + accent underline
/// - **Disabled**: dimmed text, no underline
fn render_toggle_set<'a>(
    items: &[(String, String, bool)],
    font_size: f32,
    is_center: bool,
    opacity: f32,
    cursor_index: Option<usize>,
) -> Element<'a, SettingsMessage> {
    let opt_size = font_size * 0.75;
    let underline_height = 2.0;

    let mut r = row![].spacing(22).align_y(Alignment::Center);

    for (i, (label, key, enabled)) in items.iter().enumerate() {
        let is_on = *enabled;
        let is_cursored = cursor_index == Some(i);

        let text_color = if is_cursored {
            // Cursored badge: accent color regardless of on/off
            theme::accent_bright()
        } else if is_center {
            Color {
                a: if is_on { 1.0 } else { 0.5 },
                ..theme::fg0()
            }
        } else {
            Color {
                a: opacity * if is_on { 0.8 } else { 0.35 },
                ..theme::fg0()
            }
        };

        let underline_color = if is_on {
            if is_center {
                theme::accent_bright()
            } else {
                Color {
                    a: opacity * 0.6,
                    ..theme::accent_bright()
                }
            }
        } else {
            Color::TRANSPARENT
        };

        let font_weight = if is_on { Weight::Bold } else { Weight::Normal };

        let label_widget = text(label.clone())
            .size(opt_size)
            .font(Font {
                weight: font_weight,
                ..theme::ui_font()
            })
            .color(text_color)
            .wrapping(Wrapping::None);

        let underline = container(Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(underline_height))
            .style(move |_: &iced::Theme| container::Style {
                background: Some(underline_color.into()),
                ..Default::default()
            });

        let option_col = column![label_widget, underline]
            .spacing(1)
            .width(Length::Shrink)
            .align_x(Alignment::Center);

        let key_owned = key.clone();
        let mut option_btn = button(option_col)
            .style(transparent_button_style)
            .padding(Padding::new(2.0).left(4.0).right(4.0));

        if is_center {
            option_btn = option_btn.on_press(SettingsMessage::ToggleSetToggle(key_owned));
        }

        r = r.push(option_btn);
    }

    container(r).width(Length::Fill).clip(true).into()
}

// ============================================================================
// Color Sub-List Slot Rendering
// ============================================================================

/// Render a single slot in the color sub-list (gradient editing)
pub(crate) fn render_color_slot<'a>(
    ctx: &SlotRenderContext<'_>,
    hex_color: &str,
    parent_label: &str,
    total_colors: usize,
    is_editing: bool,
    hex_input: &str,
) -> Element<'a, SettingsMessage> {
    let style =
        slot_list::SlotListSlotStyle::for_slot(ctx.is_center, false, false, false, ctx.opacity, 0);
    let label_size =
        nokkvi_data::utils::scale::calculate_font_size(14.0, ctx.row_height, ctx.scale_factor)
            * ctx.scale_factor;
    let value_size =
        nokkvi_data::utils::scale::calculate_font_size(13.0, ctx.row_height, ctx.scale_factor)
            * ctx.scale_factor;
    let position_size =
        nokkvi_data::utils::scale::calculate_font_size(10.0, ctx.row_height, ctx.scale_factor)
            * ctx.scale_factor;

    let eff_opacity = if ctx.is_center { 1.0 } else { ctx.opacity };

    // Color swatch (larger than the mini swatches in the main slot list)
    let parsed_color = crate::theme_config::parse_hex_color(hex_color).unwrap_or_else(theme::fg4);
    let swatch_size = (label_size * 2.0).clamp(20.0, 36.0);

    let swatch = container(Space::new())
        .width(Length::Fixed(swatch_size))
        .height(Length::Fixed(swatch_size))
        .style(move |_theme| container::Style {
            background: Some(
                Color {
                    a: eff_opacity,
                    ..parsed_color
                }
                .into(),
            ),
            border: Border {
                color: Color {
                    a: eff_opacity * 0.6,
                    ..theme::fg4()
                },
                width: 1.0,
                radius: theme::ui_border_radius(),
            },
            ..Default::default()
        });

    // Label column (color position + parent label)
    let position_label = format!("Color {} of {}", ctx.item_index + 1, total_colors);
    let label_col = container(
        column![
            slot_list::slot_list_text(position_label, label_size, style.text_color).font(Font {
                weight: Weight::Bold,
                ..theme::ui_font()
            }),
            slot_list::slot_list_text(parent_label.to_string(), position_size, style.subtext_color),
        ]
        .spacing(2),
    )
    .height(Length::Fill)
    .clip(true)
    .align_y(Alignment::Center);

    // Value column — hex text input when editing, otherwise hex text display
    let value_display: Element<'a, SettingsMessage> = if is_editing {
        render_hex_editor(hex_input, value_size, 16.0)
    } else {
        slot_list::slot_list_text(hex_color.to_owned(), value_size, style.subtext_color).into()
    };

    let value_col = container(value_display)
        .height(Length::Fill)
        .clip(true)
        .align_y(Alignment::Center)
        .align_x(Alignment::End)
        .padding(Padding::new(0.0).right(12.0));

    let content = row![
        Space::new().width(Length::Fixed(12.0)),
        swatch,
        Space::new().width(Length::Fixed(12.0)),
        label_col,
        Space::new().width(Length::Fill),
        value_col,
    ]
    .spacing(0)
    .align_y(Alignment::Center)
    .height(Length::Fill);

    // Edit mode: accent border
    let (border_color, border_width) = if is_editing {
        (theme::accent_bright(), 2.0)
    } else {
        (style.border_color, style.border_width)
    };

    let styled = container(content)
        .style(move |_theme| container::Style {
            background: Some(style.bg_color.into()),
            border: Border {
                color: border_color,
                width: border_width,
                radius: style.border_radius,
            },
            ..Default::default()
        })
        .width(Length::Fill);

    // Make clickable — center click enters edit mode, other slots navigate
    button(styled)
        .on_press(if ctx.is_center {
            SettingsMessage::EditActivate
        } else {
            SettingsMessage::SlotListClickItem(ctx.item_index)
        })
        .style(transparent_button_style)
        .padding(0)
        .width(Length::Fill)
        .into()
}

// ============================================================================
// Font Sub-List Slot Rendering
// ============================================================================

/// Cache of Font objects for preview rendering.
/// `Family::name()` handles interning internally; we cache the `Font` to
/// avoid re-locking the global `FxHashSet` on every frame.
fn preview_font(name: &str) -> Font {
    use std::{collections::HashMap, sync::LazyLock};

    use parking_lot::Mutex;

    static CACHE: LazyLock<Mutex<HashMap<String, Font>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));

    let cache = CACHE.lock();
    if let Some(&font) = cache.get(name) {
        return font;
    }
    drop(cache);

    let font = Font::with_family(iced::font::Family::name(name));
    CACHE.lock().insert(name.to_string(), font);
    font
}

/// Render a single slot in the font picker sub-list
pub(crate) fn render_font_slot<'a>(
    ctx: &SlotRenderContext<'_>,
    font_name: &str,
) -> Element<'a, SettingsMessage> {
    let style =
        slot_list::SlotListSlotStyle::for_slot(ctx.is_center, false, false, false, ctx.opacity, 0);
    let label_size =
        nokkvi_data::utils::scale::calculate_font_size(14.0, ctx.row_height, ctx.scale_factor)
            * ctx.scale_factor;
    let hint_size =
        nokkvi_data::utils::scale::calculate_font_size(10.0, ctx.row_height, ctx.scale_factor)
            * ctx.scale_factor;

    let is_default = font_name.starts_with("Iced Default");

    // Font name rendered in its own typeface for preview
    let preview = if is_default {
        Font::DEFAULT
    } else {
        preview_font(font_name)
    };
    let name_widget =
        slot_list::slot_list_text(font_name.to_string(), label_size, style.text_color).font(Font {
            weight: Weight::Bold,
            ..preview
        });

    // Right-side hint for center item
    let hint_text = if ctx.is_center { "Enter ↵" } else { "" };
    let hint_widget = slot_list::slot_list_text(hint_text, hint_size, style.subtext_color);

    // If it's the default entry, show a small subtitle
    let subtitle = if is_default {
        "No custom font — uses iced::Font::DEFAULT"
    } else {
        ""
    };
    let subtitle_widget = slot_list::slot_list_text(subtitle, hint_size, style.subtext_color);

    let label_col = container(column![name_widget, subtitle_widget].spacing(2))
        .width(Length::FillPortion(70))
        .height(Length::Fill)
        .clip(true)
        .align_y(Alignment::Center);

    let hint_col = container(hint_widget)
        .width(Length::FillPortion(30))
        .height(Length::Fill)
        .clip(true)
        .align_y(Alignment::Center)
        .align_x(Alignment::End)
        .padding(Padding::new(0.0).right(12.0));

    let content = row![Space::new().width(Length::Fixed(12.0)), label_col, hint_col,]
        .spacing(8)
        .align_y(Alignment::Center)
        .height(Length::Fill);

    let styled = container(content)
        .style(move |_theme| style.to_container_style())
        .width(Length::Fill);

    button(styled)
        .on_press(if ctx.is_center {
            SettingsMessage::EditActivate
        } else {
            SettingsMessage::SlotListClickItem(ctx.item_index)
        })
        .style(transparent_button_style)
        .padding(0)
        .width(Length::Fill)
        .into()
}
