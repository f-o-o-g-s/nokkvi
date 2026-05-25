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
    items::{SettingItem, SettingValue, SettingsEntry},
};
use crate::{
    embedded_svg, theme,
    widgets::{
        pill_segmented_button::{PillOption, PillRowParams, PillVariant, pill_segmented_button},
        slot_list,
    },
};

// ============================================================================
// Shared Helpers
// ============================================================================

/// Whether a `SettingItem` should render the "Enter ↵" affordance when it is
/// the centered (selected) row.
///
/// Two activation patterns trigger the hint:
/// - **Always-interactive value types**: Hotkey / HexColor / ColorArray rows
///   always require Enter to activate edit / capture mode.
/// - **Opt-in dialog rows**: any item whose builder called
///   [`SettingsEntry::with_enter_hint`] — used by `Text` rows that open a
///   picker or text input dialog (font picker, local-music-path text input,
///   default playlist picker). Reading the flag here avoids stale
///   string-match drift between key strings declared in `items_*.rs` and a
///   hardcoded match arm in the renderer.
pub(crate) fn item_needs_enter_hint(item: &SettingItem) -> bool {
    matches!(
        item.value,
        SettingValue::Hotkey(_) | SettingValue::HexColor(_) | SettingValue::ColorArray(_)
    ) || item.needs_enter_hint
}

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

/// Flat value badge — mono uppercase label inside a 1 px theme::border()
/// outlined chip with theme::bg0() fill and theme::ui_radius_sm() corners in
/// rounded mode. Used for Text rows and the numeric value display inside
/// `render_numeric_row` (which then layers arrow buttons + mini-slider
/// around it).
fn render_badge<'a>(
    display_text: String,
    font_size: f32,
    is_center: bool,
    opacity: f32,
) -> Element<'a, SettingsMessage> {
    let eff_opacity = if is_center { 1.0 } else { opacity };
    let text_color = scale_alpha_local(theme::fg0(), eff_opacity);
    let badge_bg = scale_alpha_local(theme::bg0(), eff_opacity);
    let badge_border = scale_alpha_local(theme::border(), eff_opacity);
    let badge_size = font_size * 0.95;

    container(
        slot_list::slot_list_text(display_text, badge_size, text_color).font(Font {
            weight: Weight::Medium,
            ..theme::ui_font()
        }),
    )
    .padding(Padding::new(4.0).left(10.0).right(10.0))
    .style(move |_theme| container::Style {
        background: Some(badge_bg.into()),
        border: Border {
            color: badge_border,
            width: 1.0,
            radius: theme::ui_radius_sm(),
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

/// Render a section header slot.
///
/// - **Level 1** (category picker): hero row — title rendered with
///   [`theme::title_font()`] italic at 24 px equivalent + small-caps mono
///   description below + 56×56 icon chip on the left. Clickable. The cursor
///   gets a 3 px [`theme::accent_bright()`] left stripe + [`theme::bg1()`] fill.
/// - **Level 2** (section separator within a category): small-caps mono label,
///   theme::fg3() text, with a 1 px theme::border() bottom separator. Inert.
fn render_header_slot<'a>(
    ctx: &SlotRenderContext<'_>,
    label: &'static str,
    icon_path: &'static str,
    is_collapsed: bool,
) -> Element<'a, SettingsMessage> {
    if ctx.is_level1 {
        render_l1_category_row(ctx, label, icon_path)
    } else {
        render_l2_section_header(ctx, label, icon_path, is_collapsed)
    }
}

/// L2 section header — small-caps mono label, dim fg3 color, 1 px border()
/// bottom separator (matches the design's `.nk-set-section-head`).
fn render_l2_section_header<'a>(
    ctx: &SlotRenderContext<'_>,
    label: &'static str,
    icon_path: &'static str,
    is_collapsed: bool,
) -> Element<'a, SettingsMessage> {
    let font_size =
        nokkvi_data::utils::scale::calculate_font_size(11.0, ctx.row_height, ctx.scale_factor)
            * ctx.scale_factor;
    let icon_size = (font_size * 1.25).clamp(10.0, 16.0);

    // Section heads always render dim (no center accent) so they read as
    // structural dividers, not focused items. Opacity tracks the slot-list
    // fade for off-center rows.
    let text_color = scale_alpha_local(theme::fg3(), ctx.opacity);
    let icon_color = scale_alpha_local(theme::fg2(), ctx.opacity);

    let section_icon = embedded_svg::svg_widget(icon_path)
        .width(Length::Fixed(icon_size))
        .height(Length::Fixed(icon_size))
        .style(move |_, _| svg::Style {
            color: Some(icon_color),
        });

    let chevron_path = if is_collapsed {
        "assets/icons/chevron-right.svg"
    } else {
        "assets/icons/chevron-down.svg"
    };
    let chevron = embedded_svg::svg_widget(chevron_path)
        .width(Length::Fixed(icon_size))
        .height(Length::Fixed(icon_size))
        .style(move |_, _| svg::Style {
            color: Some(text_color),
        });

    let label_widget = text(label.to_uppercase())
        .size(font_size)
        .font(Font {
            weight: Weight::Medium,
            ..theme::ui_font()
        })
        .color(text_color)
        .wrapping(Wrapping::None);

    let content = row![
        Space::new().width(Length::Fixed(20.0)),
        chevron,
        section_icon,
        label_widget,
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let header_body = container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .clip(true)
        .align_y(Alignment::Center)
        .padding(Padding::new(0.0).top(8.0).left(8.0));

    // Bottom border() separator pinning the small-caps text to the panel
    // surface.
    row_with_bottom_separator(header_body.into(), false)
}

/// L1 category landing row — hero treatment matching `.nk-cat-row` in the
/// flat CSS. 56×56 icon chip on the left, title in [`theme::title_font()`]
/// italic at scaled 24 px equivalent, small-caps mono description below.
/// Cursor row gets the [`theme::bg1()`] fill + 3 px accent left stripe.
fn render_l1_category_row<'a>(
    ctx: &SlotRenderContext<'_>,
    label: &'static str,
    icon_path: &'static str,
) -> Element<'a, SettingsMessage> {
    let title_size =
        nokkvi_data::utils::scale::calculate_font_size(20.0, ctx.row_height, ctx.scale_factor)
            * ctx.scale_factor;
    let desc_size =
        nokkvi_data::utils::scale::calculate_font_size(11.0, ctx.row_height, ctx.scale_factor)
            * ctx.scale_factor;

    let title_color = if ctx.is_center {
        theme::fg0()
    } else {
        scale_alpha_local(theme::fg0(), ctx.opacity * 0.85)
    };
    let desc_color = scale_alpha_local(theme::fg2(), ctx.opacity * 0.85);

    // Tile-style icon chip: 56×56, theme::bg0_hard() body, 1px theme::border()
    // outline, accent icon. Sized down at narrow row heights so the chip
    // doesn't dominate.
    let chip_size = (title_size * 2.4).clamp(40.0, 56.0);
    let icon_inner_size = (chip_size * 0.5).clamp(20.0, 28.0);
    let icon_color = scale_alpha_local(theme::accent_bright(), ctx.opacity);
    let chip_bg = scale_alpha_local(theme::bg0_hard(), ctx.opacity);
    let chip_border = scale_alpha_local(theme::border(), ctx.opacity);

    let icon_chip = container(
        embedded_svg::svg_widget(icon_path)
            .width(Length::Fixed(icon_inner_size))
            .height(Length::Fixed(icon_inner_size))
            .style(move |_, _| svg::Style {
                color: Some(icon_color),
            }),
    )
    .width(Length::Fixed(chip_size))
    .height(Length::Fixed(chip_size))
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
    .style(move |_: &iced::Theme| container::Style {
        background: Some(chip_bg.into()),
        border: Border {
            color: chip_border,
            width: 1.0,
            radius: theme::ui_radius_md(),
        },
        ..Default::default()
    });

    let title = text(label)
        .size(title_size)
        .font(Font {
            weight: Weight::Bold,
            ..theme::title_font()
        })
        .color(title_color)
        .wrapping(Wrapping::None);

    let description = category_description(label);
    let desc_widget = text(description)
        .size(desc_size)
        .font(theme::ui_font())
        .color(desc_color)
        .wrapping(Wrapping::None);

    let text_col = column![title, desc_widget].spacing(4).width(Length::Fill);

    let content = row![
        Space::new().width(Length::Fixed(16.0)),
        icon_chip,
        Space::new().width(Length::Fixed(16.0)),
        container(text_col)
            .width(Length::Fill)
            .height(Length::Fill)
            .clip(true)
            .align_y(Alignment::Center),
    ]
    .spacing(0)
    .align_y(Alignment::Center)
    .height(Length::Fill);

    // Cursor row: light bg fill + 3 px accent left stripe.
    let body = with_cursor_stripe(content.into(), ctx.is_center);

    button(body)
        .style(transparent_button_style)
        .padding(0)
        .width(Length::Fill)
        .on_press(if ctx.is_center {
            SettingsMessage::EditActivate
        } else {
            SettingsMessage::SlotListClickItem(ctx.item_index)
        })
        .into()
}

/// Per-category description shown below the L1 hero title. Kept compact so
/// the L1 hero rows don't blow past the slot height at small window sizes.
fn category_description(label: &str) -> &'static str {
    match label {
        "General" => "Account, mouse behavior, tray, library defaults",
        "Interface" => "Layout, fonts, artwork column, metadata strip",
        "Playback" => "Crossfade, scrobbling, playlists, gapless behavior",
        "Hotkeys" => "Rebind keyboard shortcuts and resolve conflicts",
        "Theme" => "Switch themes and tune color tokens",
        "Visualizer" => "Bars, lines, peak modes, palette tuning",
        _ => "Configure this section",
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
    let is_action_item = super::sentinel::SentinelKind::from_key(key_ref).is_some();

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

        // Determine if this item needs an "Enter ↵" hint. See
        // [`item_needs_enter_hint`] for the rule.
        let show_hint =
            item_needs_enter_hint(item) && ctx.is_center && !is_editing && !ctx.is_capturing;

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

    // Cursor stripe + bg fill for the center row, then the design's
    // theme::border() bottom separator pinning rows to the panel surface.
    let row_body = with_cursor_stripe(content, ctx.is_center);
    let with_separator = row_with_bottom_separator(row_body, ctx.is_center);

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

// ============================================================================
// Row chrome helpers (cursor stripe + bottom separator)
// ============================================================================

/// Wrap a row's content in the cursor stripe + bg fill chrome. The cursor
/// row gets a 3 px [`theme::accent_bright()`] left stripe + [`theme::bg1()`]
/// fill matching the design's `.nk-set-row.cursor::before` + `.nk-set-row.cursor`
/// styling. Non-cursor rows render with transparent bg + transparent stripe.
fn with_cursor_stripe<'a>(
    content: Element<'a, SettingsMessage>,
    is_center: bool,
) -> Element<'a, SettingsMessage> {
    let row_bg = if is_center {
        theme::bg1()
    } else {
        Color::TRANSPARENT
    };
    let stripe_color = if is_center {
        theme::accent_bright()
    } else {
        Color::TRANSPARENT
    };

    let stripe = container(Space::new())
        .width(Length::Fixed(3.0))
        .height(Length::Fill)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(stripe_color.into()),
            ..Default::default()
        });

    let row_body = row![stripe, container(content).width(Length::Fill)]
        .height(Length::Fill)
        .width(Length::Fill);

    container(row_body)
        .width(Length::Fill)
        .height(Length::Fill)
        .clip(true)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(row_bg.into()),
            ..Default::default()
        })
        .into()
}

/// Pin a 1 px [`theme::border()`] separator under a row. `is_center` controls
/// whether the separator dims slightly to avoid clashing with the cursor
/// stripe (design keeps it crisp regardless, so this just routes color).
fn row_with_bottom_separator<'a>(
    content: Element<'a, SettingsMessage>,
    _is_center: bool,
) -> Element<'a, SettingsMessage> {
    let sep_color = theme::border();

    column![
        container(content).width(Length::Fill).height(Length::Fill),
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
        SettingValue::Bool(v) => render_bool_pills(*v, font_size, is_center, opacity),

        SettingValue::HexColor(hex) => {
            if is_editing {
                // Inline text input for hex editing in the main slot list. The
                // editor still uses its own (square) preview swatch; design
                // parity for the editor surface is handled in `render_hex_editor`.
                let swatch_size = (font_size * 1.2).clamp(12.0, 20.0);
                render_hex_editor(hex_input, font_size, swatch_size)
            } else {
                render_hex_value_chip(hex, font_size, is_center, opacity, style.subtext_color)
            }
        }

        SettingValue::ColorArray(colors) => {
            render_color_array_swatches(colors, font_size, is_center, opacity, style.subtext_color)
        }

        SettingValue::Enum { val, options } => {
            render_enum_pills(options, val, font_size, is_center, opacity)
        }

        SettingValue::ToggleSet(items) => render_toggle_set_pills(
            items,
            font_size,
            is_center,
            opacity,
            if is_center { ctx.toggle_cursor } else { None },
        ),

        SettingValue::Hotkey(combo) => {
            return render_hotkey_badge(combo, font_size, ctx);
        }

        _ => {
            // Float, Int, Text — badge with bg0_hard background
            render_badge(value.display(), font_size, is_center, opacity)
        }
    };

    // Show chevron arrows + mini-slider for numeric values only.
    // Bool / Enum / ToggleSet use chip widgets instead.
    if value.is_incrementable() {
        render_numeric_row(value, value_widget, font_size, is_center, opacity)
    } else {
        value_widget
    }
}

/// Compose the numeric row chrome around a pre-rendered value badge:
/// `[ ‹ ] [ value ] [ slider track ] [ › ]` matching the design's `.nk-w-num`
/// layout. Slider is purely visual (non-draggable) so wheel + arrows remain
/// the sole input paths — a 4 px draggable track inside a slot row would
/// fight the slot-list scroll listeners.
fn render_numeric_row<'a>(
    value: &SettingValue,
    value_badge: Element<'a, SettingsMessage>,
    font_size: f32,
    is_center: bool,
    opacity: f32,
) -> Element<'a, SettingsMessage> {
    let eff_opacity = if is_center { 1.0 } else { opacity };

    let left_arrow = arrow_button(
        "assets/icons/chevron-left.svg",
        font_size,
        is_center,
        eff_opacity,
        SettingsMessage::EditLeft,
    );
    let right_arrow = arrow_button(
        "assets/icons/chevron-right.svg",
        font_size,
        is_center,
        eff_opacity,
        SettingsMessage::EditRight,
    );

    // Optional mini-slider track between value + right arrow.
    let track: Option<Element<'a, SettingsMessage>> =
        numeric_normalized_fraction(value).map(|frac| numeric_mini_track(frac, eff_opacity));

    let mut layout = row![
        left_arrow,
        Space::new().width(Length::Fixed(8.0)),
        value_badge,
    ]
    .align_y(Alignment::Center);
    if let Some(track_el) = track {
        layout = layout
            .push(Space::new().width(Length::Fixed(10.0)))
            .push(track_el);
    }
    layout = layout
        .push(Space::new().width(Length::Fixed(8.0)))
        .push(right_arrow);

    layout.into()
}

/// 22×22 flat arrow button — 1 px [`theme::border()`] outline, [`theme::bg0()`]
/// fill, [`theme::fg2()`] chevron. Clickable only on the center row (same rule
/// as the legacy chevrons — `EditLeft` / `EditRight` act on the center item).
fn arrow_button<'a>(
    icon_path: &'static str,
    font_size: f32,
    is_center: bool,
    eff_opacity: f32,
    on_press: SettingsMessage,
) -> Element<'a, SettingsMessage> {
    let arrow_icon_size = (font_size * 0.85).clamp(10.0, 16.0);
    let icon_color = scale_alpha_local(theme::fg2(), eff_opacity);
    let border = scale_alpha_local(theme::border(), eff_opacity);
    let fill = scale_alpha_local(theme::bg0(), eff_opacity);

    let icon = embedded_svg::svg_widget(icon_path)
        .width(Length::Fixed(arrow_icon_size))
        .height(Length::Fixed(arrow_icon_size))
        .style(move |_, _| svg::Style {
            color: Some(icon_color),
        });

    let body = container(icon)
        .width(Length::Fixed(22.0))
        .height(Length::Fixed(22.0))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(fill.into()),
            border: Border {
                color: border,
                width: 1.0,
                radius: theme::ui_radius_pill(),
            },
            ..Default::default()
        });

    let mut btn = button(body).style(transparent_button_style).padding(0);
    if is_center {
        btn = btn.on_press(on_press);
    }
    btn.into()
}

/// 120×4 mini-slider track + 10 px handle showing the value's position within
/// its `min..max` range. Read-only by design — the surrounding arrow buttons
/// (and wheel events handled at the slot list level) drive the value.
fn numeric_mini_track<'a>(fraction: f32, eff_opacity: f32) -> Element<'a, SettingsMessage> {
    const TRACK_WIDTH: f32 = 120.0;
    const TRACK_HEIGHT: f32 = 4.0;
    const HANDLE_SIZE: f32 = 10.0;

    let track_bg = scale_alpha_local(theme::bg0(), eff_opacity);
    let track_border = scale_alpha_local(theme::border(), eff_opacity);
    let fill_color = scale_alpha_local(theme::accent_bright(), eff_opacity);
    let handle_color = fill_color;

    let frac = fraction.clamp(0.0, 1.0);
    let fill_width = (TRACK_WIDTH * frac).max(0.0);
    // Center the handle on the fill edge — subtract half its width.
    let handle_offset = (TRACK_WIDTH * frac - HANDLE_SIZE / 2.0).max(0.0);
    let right_spacer = (TRACK_WIDTH - handle_offset - HANDLE_SIZE).max(0.0);

    // Track + colored fill (two stacked rectangles in a row sized to the
    // fraction). The handle sits on top in its own row aligned via Space-padded
    // anchors so we don't need an absolute-position overlay.
    let fill = container(Space::new())
        .width(Length::Fixed(fill_width))
        .height(Length::Fixed(TRACK_HEIGHT))
        .style(move |_: &iced::Theme| container::Style {
            background: Some(fill_color.into()),
            border: Border {
                radius: theme::ui_radius_pill(),
                ..Default::default()
            },
            ..Default::default()
        });
    let track_body = container(fill)
        .width(Length::Fixed(TRACK_WIDTH))
        .height(Length::Fixed(TRACK_HEIGHT))
        .align_y(Alignment::Center)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(track_bg.into()),
            border: Border {
                color: track_border,
                width: 1.0,
                radius: theme::ui_radius_pill(),
            },
            ..Default::default()
        });

    let handle = container(Space::new())
        .width(Length::Fixed(HANDLE_SIZE))
        .height(Length::Fixed(HANDLE_SIZE))
        .style(move |_: &iced::Theme| container::Style {
            background: Some(handle_color.into()),
            border: Border {
                radius: theme::ui_radius_pill(),
                ..Default::default()
            },
            ..Default::default()
        });

    // Stack track + handle. The handle row uses Space padding instead of an
    // overlay so positioning happens entirely through layout and respects the
    // ambient alpha scale.
    let handle_row = row![
        Space::new().width(Length::Fixed(handle_offset)),
        handle,
        Space::new().width(Length::Fixed(right_spacer)),
    ]
    .align_y(Alignment::Center);

    iced::widget::stack![
        container(track_body)
            .width(Length::Fixed(TRACK_WIDTH))
            .height(Length::Fixed(HANDLE_SIZE))
            .align_y(Alignment::Center),
        container(handle_row)
            .width(Length::Fixed(TRACK_WIDTH))
            .height(Length::Fixed(HANDLE_SIZE))
            .align_y(Alignment::Center),
    ]
    .width(Length::Fixed(TRACK_WIDTH))
    .height(Length::Fixed(HANDLE_SIZE))
    .into()
}

/// Compute the value's normalized 0..1 fraction within its `min..max` range,
/// or `None` if the range is degenerate (max == min) or the variant isn't
/// numeric. Used by the mini-slider track to position its handle.
fn numeric_normalized_fraction(value: &SettingValue) -> Option<f32> {
    match value {
        SettingValue::Float { val, min, max, .. } => {
            if (max - min).abs() < f64::EPSILON {
                return None;
            }
            Some(((val - min) / (max - min)) as f32)
        }
        SettingValue::Int { val, min, max, .. } => {
            if *max == *min {
                return None;
            }
            Some(((val - min) as f32) / ((max - min) as f32))
        }
        _ => None,
    }
}

// ============================================================================
// Pill-Segmented Widget Adapters (Bool / Enum / ToggleSet)
// ============================================================================
//
// These thin wrappers translate the legacy `SettingValue` shape into the
// shared `pill_segmented_button` widget. They produce 1px-bordered chips in
// flat mode and pill-rounded chips in rounded mode, with selected chips
// filling in `theme::accent_bright()`. Non-center rows render the chips
// non-interactively (the parent slot list row handles up/down/click
// navigation).

/// Render a Bool setting as a two-chip On/Off group.
fn render_bool_pills<'a>(
    val: bool,
    font_size: f32,
    is_center: bool,
    opacity: f32,
) -> Element<'a, SettingsMessage> {
    let options = [
        PillOption {
            display: "On".to_string(),
            key: "On".to_string(),
            on: val,
        },
        PillOption {
            display: "Off".to_string(),
            key: "Off".to_string(),
            on: !val,
        },
    ];
    pill_segmented_button(
        &options,
        PillVariant::Single,
        PillRowParams {
            font_size: chip_label_size(font_size),
            is_center,
            opacity,
        },
        SettingsMessage::EditSetValue,
    )
}

/// Render an Enum setting as a single-select chip group, one chip per option.
fn render_enum_pills<'a>(
    options: &[&'a str],
    selected: &str,
    font_size: f32,
    is_center: bool,
    opacity: f32,
) -> Element<'a, SettingsMessage> {
    let chip_options: Vec<PillOption> = options
        .iter()
        .map(|&option| PillOption {
            display: option.to_string(),
            key: option.to_string(),
            on: option == selected,
        })
        .collect();
    pill_segmented_button(
        &chip_options,
        PillVariant::Single,
        PillRowParams {
            font_size: chip_label_size(font_size),
            is_center,
            opacity,
        },
        SettingsMessage::EditSetValue,
    )
}

/// Render a ToggleSet as a multi-select chip group. The cursored chip (set by
/// keyboard arrow navigation within the toggle set) gets the accent outline
/// even when it isn't on, signaling which chip Enter will toggle.
fn render_toggle_set_pills<'a>(
    items: &[(String, String, bool)],
    font_size: f32,
    is_center: bool,
    opacity: f32,
    cursor_index: Option<usize>,
) -> Element<'a, SettingsMessage> {
    let chip_options: Vec<PillOption> = items
        .iter()
        .map(|(label, key, enabled)| PillOption {
            display: label.clone(),
            key: key.clone(),
            on: *enabled,
        })
        .collect();
    pill_segmented_button(
        &chip_options,
        PillVariant::Multi { cursor_index },
        PillRowParams {
            font_size: chip_label_size(font_size),
            is_center,
            opacity,
        },
        SettingsMessage::ToggleSetToggle,
    )
}

/// Chip label is rendered at ~80 % of the row's value font size so chips don't
/// dominate the row visually. Mirrors the CSS designs' `11 px` chip label vs
/// `13 px` row label ratio.
#[inline]
fn chip_label_size(font_size: f32) -> f32 {
    font_size * 0.80
}

// ============================================================================
// HexColor + ColorArray rendering
// ============================================================================
//
// Per the design (`.nk-w-hex`): mono hex on the left, then a 28×24 swatch on
// the right with a 1 px `theme::border()` outline in flat mode and
// `theme::ui_radius_xs()` corners in rounded mode. The CSS layout is `gap:
// 10px; min-width: 76px; text-align: right` on the hex label; we keep the
// 76 px min so the swatch column lines up across stacked color rows.

const HEX_VALUE_MIN_WIDTH: f32 = 76.0;
const HEX_SWATCH_WIDTH: f32 = 28.0;
const HEX_SWATCH_HEIGHT: f32 = 24.0;

/// Static (non-editing) hex value badge — uppercase mono hex + swatch chip.
fn render_hex_value_chip<'a>(
    hex: &str,
    font_size: f32,
    is_center: bool,
    opacity: f32,
    hex_label_color: Color,
) -> Element<'a, SettingsMessage> {
    let parsed_color = crate::theme_config::parse_hex_color(hex).unwrap_or_else(theme::fg4);
    let eff_opacity = if is_center { 1.0 } else { opacity };
    let fill = scale_alpha_local(parsed_color, eff_opacity);
    let border = scale_alpha_local(theme::border(), eff_opacity);

    let hex_label = container(slot_list::slot_list_text(
        hex.to_uppercase(),
        font_size * 0.95,
        hex_label_color,
    ))
    .width(Length::Fixed(HEX_VALUE_MIN_WIDTH))
    .align_x(Alignment::End);

    let swatch = container(Space::new())
        .width(Length::Fixed(HEX_SWATCH_WIDTH))
        .height(Length::Fixed(HEX_SWATCH_HEIGHT))
        .style(move |_: &iced::Theme| container::Style {
            background: Some(fill.into()),
            border: Border {
                color: border,
                width: 1.0,
                radius: theme::ui_radius_xs(),
            },
            ..Default::default()
        });

    row![hex_label, swatch]
        .spacing(10)
        .align_y(Alignment::Center)
        .into()
}

/// Small swatch strip for ColorArray rows — N tiny `theme::border()`-outlined
/// `theme::ui_radius_xs()` swatches followed by the count label. Capped at 8
/// previews to keep the row width bounded.
fn render_color_array_swatches<'a>(
    colors: &[String],
    font_size: f32,
    is_center: bool,
    opacity: f32,
    count_label_color: Color,
) -> Element<'a, SettingsMessage> {
    let eff_opacity = if is_center { 1.0 } else { opacity };
    let swatch_size = (font_size * 0.95).clamp(10.0, 16.0);
    let border = scale_alpha_local(theme::border(), eff_opacity);

    let mut r = row![].spacing(2).align_y(Alignment::Center);
    for hex in colors.iter().take(8) {
        let parsed = crate::theme_config::parse_hex_color(hex).unwrap_or_else(theme::fg4);
        let fill = scale_alpha_local(parsed, eff_opacity);
        r = r.push(
            container(Space::new())
                .width(Length::Fixed(swatch_size))
                .height(Length::Fixed(swatch_size))
                .style(move |_: &iced::Theme| container::Style {
                    background: Some(fill.into()),
                    border: Border {
                        color: border,
                        width: 1.0,
                        radius: theme::ui_radius_xs(),
                    },
                    ..Default::default()
                }),
        );
    }
    r = r.push(Space::new().width(Length::Fixed(8.0)));
    r = r.push(slot_list::slot_list_text(
        format!("{}", colors.len()),
        font_size * 0.85,
        count_label_color,
    ));
    r.into()
}

// ============================================================================
// Hotkey Badge States
// ============================================================================
//
// Per the design (nokkvi-settings.css `.nk-w-key`):
// - Idle:     `accent_bright()` fill + `bg0_hard()` text, 96 px wide key-cap.
// - Capture:  transparent fill + `warning_bright()` border + `warning_bright()`
//             text, with the "Esc cancel · Del reset" hint inline. (Pulse
//             animation noted in the design is omitted — iced has no
//             keyframes; would require a per-frame Tick subscription that
//             isn't worth wiring just for visual flourish.)
// - Conflict: transparent fill + `danger()` border + `danger()` text, showing
//             the conflict label that the capture handler emitted (which
//             names the colliding action).
//
// The design also lists a "disabled" state (`bg2()` border, `fg3()` text).
// That state has no data-level producer today — `HotkeyConfig::get_binding()`
// always returns a `KeyCombo`, never `None` — so it's intentionally not
// rendered. Add it here when an "unbound" representation lands in the data
// layer.

/// Render the hotkey value badge in its current state.
fn render_hotkey_badge<'a>(
    combo: &str,
    font_size: f32,
    ctx: &SlotRenderContext<'_>,
) -> Element<'a, SettingsMessage> {
    let opacity = if ctx.is_center { 1.0 } else { ctx.opacity };

    // Capture mode (center-row only by construction in view.rs) — either a
    // "press a key" prompt or a conflict warning, both rendered as inverted
    // badges with the design's gold/red palette.
    if ctx.is_capturing && ctx.is_center {
        if let Some(conflict) = ctx.conflict_text {
            return hotkey_capture_badge(
                conflict,
                None,
                font_size,
                theme::danger(),
                theme::danger(),
            );
        }
        return hotkey_capture_badge(
            "Press a key...",
            Some("Esc cancel · Del reset"),
            font_size,
            theme::warning_bright(),
            theme::warning_bright(),
        );
    }

    // Idle / non-center: green key-cap badge.
    hotkey_idle_badge(combo, font_size, opacity)
}

/// Idle key-cap badge — full accent fill, dark text, 96 px wide minimum
/// matching `nk-w-key` in the flat CSS.
fn hotkey_idle_badge<'a>(
    combo: &str,
    font_size: f32,
    opacity: f32,
) -> Element<'a, SettingsMessage> {
    let badge_size = font_size * 0.92;
    let bg = scale_alpha_local(theme::accent_bright(), opacity);
    let border = bg;
    let text_color = scale_alpha_local(theme::bg0_hard(), opacity);

    container(
        slot_list::slot_list_text(combo.to_string(), badge_size, text_color).font(Font {
            weight: Weight::Medium,
            ..theme::ui_font()
        }),
    )
    .width(Length::Fixed(96.0))
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
    .padding(Padding::new(5.0).left(14.0).right(14.0))
    .style(move |_: &iced::Theme| container::Style {
        background: Some(bg.into()),
        border: Border {
            color: border,
            width: 1.0,
            radius: theme::ui_radius_pill(),
        },
        ..Default::default()
    })
    .into()
}

/// Capture / conflict badge — transparent fill, colored border + text, with
/// an optional inline hint suffix. Used for both the "Press a key..." prompt
/// (gold) and the conflict warning (red).
fn hotkey_capture_badge<'a>(
    label: &str,
    hint: Option<&str>,
    font_size: f32,
    border_color: Color,
    text_color: Color,
) -> Element<'a, SettingsMessage> {
    let badge_size = font_size * 0.92;
    let hint_size = font_size * 0.72;
    let hint_color = Color {
        a: 0.7,
        ..text_color
    };

    let mut body = row![
        slot_list::slot_list_text(label.to_string(), badge_size, text_color).font(Font {
            weight: Weight::Medium,
            ..theme::ui_font()
        }),
    ]
    .align_y(Alignment::Center);

    if let Some(h) = hint {
        body = body
            .push(Space::new().width(Length::Fixed(8.0)))
            .push(slot_list::slot_list_text(
                h.to_string(),
                hint_size,
                hint_color,
            ));
    }

    container(body)
        .padding(Padding::new(5.0).left(14.0).right(14.0))
        .style(move |_: &iced::Theme| container::Style {
            background: Some(Color::TRANSPARENT.into()),
            border: Border {
                color: border_color,
                width: 1.0,
                radius: theme::ui_radius_pill(),
            },
            ..Default::default()
        })
        .into()
}

/// Local copy of the pill widget's alpha scaler (kept private to avoid
/// promoting a trivial helper to a shared API).
#[inline]
fn scale_alpha_local(c: Color, factor: f32) -> Color {
    Color {
        a: c.a * factor,
        ..c
    }
}

// ============================================================================
// Color Sub-List Slot Rendering
// ============================================================================

/// Render a single slot in the color sub-list (gradient editing). Uses the
/// same flat row chrome as the main settings slot list — theme::bg1() cursor
/// fill + 3 px theme::accent_bright() left stripe + theme::border() bottom
/// separator. The swatch picks up theme::border() outline and
/// theme::ui_radius_xs() corners in rounded mode for parity with the inline
/// HexColor row.
pub(crate) fn render_color_slot<'a>(
    ctx: &SlotRenderContext<'_>,
    hex_color: &str,
    parent_label: &str,
    total_colors: usize,
    is_editing: bool,
    hex_input: &str,
) -> Element<'a, SettingsMessage> {
    let label_size =
        nokkvi_data::utils::scale::calculate_font_size(14.0, ctx.row_height, ctx.scale_factor)
            * ctx.scale_factor;
    let value_size =
        nokkvi_data::utils::scale::calculate_font_size(13.0, ctx.row_height, ctx.scale_factor)
            * ctx.scale_factor;
    let position_size =
        nokkvi_data::utils::scale::calculate_font_size(10.0, ctx.row_height, ctx.scale_factor)
            * ctx.scale_factor;

    let label_color = if ctx.is_center {
        theme::accent_bright()
    } else {
        scale_alpha_local(theme::fg0(), ctx.opacity)
    };
    let subtext_color = scale_alpha_local(theme::fg3(), ctx.opacity * 0.7);

    let eff_opacity = if ctx.is_center { 1.0 } else { ctx.opacity };

    // Color swatch — larger than the inline mini swatches (matches the design
    // intent of giving the gradient editor a more prominent preview chip).
    let parsed_color = crate::theme_config::parse_hex_color(hex_color).unwrap_or_else(theme::fg4);
    let swatch_size = (label_size * 2.0).clamp(20.0, 36.0);
    let swatch_fill = scale_alpha_local(parsed_color, eff_opacity);
    let swatch_border = scale_alpha_local(theme::border(), eff_opacity);

    let swatch = container(Space::new())
        .width(Length::Fixed(swatch_size))
        .height(Length::Fixed(swatch_size))
        .style(move |_: &iced::Theme| container::Style {
            background: Some(swatch_fill.into()),
            border: Border {
                color: swatch_border,
                width: 1.0,
                radius: theme::ui_radius_xs(),
            },
            ..Default::default()
        });

    // Label column (color position + parent label)
    let position_label = format!("Color {} of {}", ctx.item_index + 1, total_colors);
    let label_col = container(
        column![
            slot_list::slot_list_text(position_label, label_size, label_color).font(Font {
                weight: Weight::Bold,
                ..theme::ui_font()
            }),
            slot_list::slot_list_text(parent_label.to_string(), position_size, subtext_color),
        ]
        .spacing(2),
    )
    .height(Length::Fill)
    .clip(true)
    .align_y(Alignment::Center);

    // Value column — hex text input when editing, otherwise mono hex label.
    let value_display: Element<'a, SettingsMessage> = if is_editing {
        render_hex_editor(hex_input, value_size, 16.0)
    } else {
        slot_list::slot_list_text(hex_color.to_uppercase(), value_size, subtext_color).into()
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

    let body = with_cursor_stripe(content.into(), ctx.is_center);
    let with_separator = row_with_bottom_separator(body, ctx.is_center);

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

/// Render a single slot in the font picker sub-list. Same flat row chrome as
/// the main slot list (cursor stripe + bottom border separator) so the font
/// modal feels like an extension of the settings panel rather than a separate
/// surface.
pub(crate) fn render_font_slot<'a>(
    ctx: &SlotRenderContext<'_>,
    font_name: &str,
) -> Element<'a, SettingsMessage> {
    let label_size =
        nokkvi_data::utils::scale::calculate_font_size(14.0, ctx.row_height, ctx.scale_factor)
            * ctx.scale_factor;
    let hint_size =
        nokkvi_data::utils::scale::calculate_font_size(10.0, ctx.row_height, ctx.scale_factor)
            * ctx.scale_factor;

    let label_color = if ctx.is_center {
        theme::accent_bright()
    } else {
        scale_alpha_local(theme::fg0(), ctx.opacity)
    };
    let subtext_color = scale_alpha_local(theme::fg3(), ctx.opacity * 0.7);

    let is_default = font_name.starts_with("Iced Default");

    // Font name rendered in its own typeface for preview.
    let preview = if is_default {
        Font::DEFAULT
    } else {
        preview_font(font_name)
    };
    let name_widget = slot_list::slot_list_text(font_name.to_string(), label_size, label_color)
        .font(Font {
            weight: Weight::Bold,
            ..preview
        });

    let hint_text = if ctx.is_center { "Enter ↵" } else { "" };
    let hint_widget = slot_list::slot_list_text(hint_text, hint_size, subtext_color);

    let subtitle = if is_default {
        "No custom font — uses iced::Font::DEFAULT"
    } else {
        ""
    };
    let subtitle_widget = slot_list::slot_list_text(subtitle, hint_size, subtext_color);

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

    let body = with_cursor_stripe(content.into(), ctx.is_center);
    let with_separator = row_with_bottom_separator(body, ctx.is_center);

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

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;

    /// Construct a bare `SettingItem` for predicate tests, with all defaults
    /// (`needs_enter_hint = false`, `is_theme_key = false`, no subtitle, no
    /// icon). Callers tweak whichever fields the test asserts against.
    fn bare_item(key: &'static str, value: SettingValue) -> SettingItem {
        let default = value.clone();
        SettingItem {
            key: Cow::Borrowed(key),
            label: key.to_string(),
            category: "Test",
            value,
            default,
            label_icon: None,
            subtitle: None,
            is_theme_key: false,
            needs_enter_hint: false,
        }
    }

    #[test]
    fn enter_hint_unconditional_for_hotkey_hexcolor_colorarray() {
        let hotkey = bare_item("k.hotkey", SettingValue::Hotkey("Ctrl+P".to_string()));
        let hex = bare_item("k.hex", SettingValue::HexColor("#abcdef".to_string()));
        let arr = bare_item(
            "k.arr",
            SettingValue::ColorArray(vec!["#000000".to_string()]),
        );
        assert!(item_needs_enter_hint(&hotkey));
        assert!(item_needs_enter_hint(&hex));
        assert!(item_needs_enter_hint(&arr));
    }

    #[test]
    fn enter_hint_off_by_default_for_plain_text_and_scalars() {
        let bool_item = bare_item("k.bool", SettingValue::Bool(true));
        let text_item = bare_item("k.text", SettingValue::Text("hi".to_string()));
        let float_item = bare_item(
            "k.float",
            SettingValue::Float {
                val: 0.0,
                min: 0.0,
                max: 1.0,
                step: 0.1,
                unit: "",
            },
        );
        assert!(!item_needs_enter_hint(&bool_item));
        assert!(!item_needs_enter_hint(&text_item));
        assert!(!item_needs_enter_hint(&float_item));
    }

    /// Regression guard for tier-0 defect #0.3 — a `Text` row marked with
    /// `needs_enter_hint = true` (via `SettingsEntry::with_enter_hint`) must
    /// trigger the affordance regardless of its key. Replaces the previous
    /// `matches!(key, "theme.font.family" | ...)` string match which silently
    /// dropped `font_family` and `general.default_playlist_name`.
    #[test]
    fn enter_hint_opts_in_via_needs_enter_hint_flag() {
        let mut text_item = bare_item("font_family", SettingValue::Text("Sans".to_string()));
        assert!(
            !item_needs_enter_hint(&text_item),
            "default Text item must not show hint"
        );
        text_item.needs_enter_hint = true;
        assert!(
            item_needs_enter_hint(&text_item),
            "Text item opted in via flag must show hint"
        );
    }
}
