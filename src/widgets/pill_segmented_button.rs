//! Pill-segmented option chip group used by Settings Bool / Enum / ToggleSet widgets.
//!
//! Renders a horizontal row of option chips with the design's flat chip vocabulary:
//! - **Flat mode**: 1 px [`theme::border()`] outline, `theme::bg0()` fill,
//!   `theme::fg2()` label text, mono 11 px UPPERCASE with 0.1 em-equivalent
//!   letter-spacing approximation.
//! - **Rounded mode**: same look + [`theme::ui_radius_pill()`] corners.
//! - **Selected (single)**: full `theme::accent_bright()` fill, `theme::bg0_hard()`
//!   label. ToggleSet variants light up multiple chips simultaneously.
//! - **Cursored (ToggleSet keyboard cursor)**: same fill as idle but the border
//!   switches to `theme::accent_bright()` and the label switches to
//!   `theme::accent_bright()` — signals which chip Enter will toggle.
//! - **Non-center row** (slot-list rows that aren't the keyboard cursor row):
//!   chips render but stay non-interactive; alpha is scaled by `opacity` so the
//!   row matches the SM-style fade applied to other slot widgets.
//!
//! The renderer is generic over the message type so the settings widget
//! callbacks (`EditSetValue(String)` for single-select, `ToggleSetToggle(String)`
//! for multi-select) plug in directly without an intermediate Message enum.
//!
//! See `views/settings/rendering.rs` for the call sites and
//! `IMPLEMENTATION_PLAN.md` §5 (L5) for design rationale.

use iced::{
    Alignment, Border, Color, Element, Length, Padding,
    font::Weight,
    widget::{button, container, row, text, text::Wrapping},
};

use crate::theme;

/// One option entry in a pill-segmented group.
///
/// `display` is the human-readable chip label (rendered uppercase via the
/// runtime helper since iced's text widget has no CSS-equivalent
/// `text-transform: uppercase`). `key` is the value sent back to the message
/// builder on click — for Enum chips this is the option string identical to
/// `display`; for ToggleSet chips this is the toggle key string distinct from
/// the label.
#[derive(Debug, Clone)]
pub(crate) struct PillOption {
    pub display: String,
    pub key: String,
    pub on: bool,
}

/// Render mode for the chip group — controls multi-vs-single selection visual
/// and whether the cursor highlight applies.
#[derive(Debug, Clone, Copy)]
pub(crate) enum PillVariant {
    /// Single-select (Bool, Enum) — exactly one chip is `on`; cursor unused.
    Single,
    /// Multi-select (ToggleSet) — zero or more chips can be `on`; the cursored
    /// chip gets the accent border + label even when `on == false`.
    Multi { cursor_index: Option<usize> },
}

/// Visual + interaction parameters shared across one chip group render.
pub(crate) struct PillRowParams {
    /// Base chip text size in logical px (pre-`scale_factor`).
    pub font_size: f32,
    /// Whether this row is the keyboard-cursored slot-list row. Non-center
    /// chips render as fully visible but are non-interactive — the callback
    /// passed via `on_click` is only attached when `is_center == true`.
    pub is_center: bool,
    /// Opacity scale applied to chip colors for off-cursor rows (matches the
    /// SM-style fade applied to row label text).
    pub opacity: f32,
}

/// Build a pill-segmented option group.
///
/// `on_click` is invoked with the clicked option's `key` — typically wrapped
/// in `SettingsMessage::EditSetValue` (Bool/Enum) or
/// `SettingsMessage::ToggleSetToggle` (ToggleSet) at the call site.
pub(crate) fn pill_segmented_button<'a, Message, F>(
    options: &[PillOption],
    variant: PillVariant,
    params: PillRowParams,
    on_click: F,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
    F: Fn(String) -> Message + Copy + 'a,
{
    // Chip group: tight gap between chips matches the design's
    // adjacent-cell look in flat mode + rounded-mode 4 px pill gap.
    let chip_gap: f32 = if theme::is_rounded_mode() { 4.0 } else { 0.0 };
    let mut chip_row = row![].spacing(chip_gap).align_y(Alignment::Center);

    let cursor_index = match variant {
        PillVariant::Multi { cursor_index } => cursor_index,
        PillVariant::Single => None,
    };

    for (i, option) in options.iter().enumerate() {
        let chip = build_chip(option, i, cursor_index, &params, on_click);
        chip_row = chip_row.push(chip);
    }

    container(chip_row)
        .height(Length::Shrink)
        .align_y(Alignment::Center)
        .into()
}

/// Single chip — sized for ~11 px label text, 5 px vertical / 14 px horizontal
/// padding (flat) or 6 px / 14–16 px (rounded). The label is uppercased at
/// render time; mono is implied by `theme::ui_font()` when the user's title
/// font is left at default (the design assumes JetBrains Mono).
fn build_chip<'a, Message, F>(
    option: &PillOption,
    index: usize,
    cursor_index: Option<usize>,
    params: &PillRowParams,
    on_click: F,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
    F: Fn(String) -> Message + Copy + 'a,
{
    let is_on = option.on;
    let is_cursored = cursor_index == Some(index);
    let effective_opacity = if params.is_center {
        1.0
    } else {
        params.opacity
    };

    // Color resolution — selected wins over cursored on label color (cursored
    // border still signals which chip Enter targets, but the green fill is the
    // primary "this is on" affordance).
    let (bg_color, border_color, label_color) = if is_on {
        // Selected: full accent fill, dark text for contrast.
        (
            theme::accent_bright(),
            theme::accent_bright(),
            theme::bg0_hard(),
        )
    } else if is_cursored {
        // Cursored but off: transparent body with accent outline + label.
        (theme::bg0(), theme::accent_bright(), theme::accent_bright())
    } else {
        // Idle: subtle outlined chip.
        (theme::bg0(), theme::border(), theme::fg2())
    };

    // Apply the slot-list opacity fade to non-center rows so the chip's visual
    // weight matches the label/value text around it.
    let bg_color = scale_alpha(bg_color, effective_opacity);
    let border_color = scale_alpha(border_color, effective_opacity);
    let label_color = scale_alpha(label_color, effective_opacity);

    // Chip label — uppercased manually since iced has no text-transform.
    // Mono comes from `ui_font()`; weight bumps to Medium when on so the chip
    // pops a bit visually without changing layout width significantly.
    let label_weight = if is_on {
        Weight::Medium
    } else {
        Weight::Normal
    };
    let label = text(option.display.to_uppercase())
        .size(params.font_size)
        .font(theme::weighted_ui_font(label_weight))
        .color(label_color)
        .wrapping(Wrapping::None);

    let chip_padding = if theme::is_rounded_mode() {
        Padding::new(6.0).left(14.0).right(14.0)
    } else {
        Padding::new(5.0).left(12.0).right(12.0)
    };

    // Interactive only on the center row; non-center rows render the same chip
    // shape but ignore clicks (the surrounding slot list row handles
    // up/down/select navigation when a non-center chip's row is clicked).
    if params.is_center {
        // Center row: paint the chip via the wrapping button's `Style` so
        // `button::Status::Hovered` / `::Pressed` can adjust the fill +
        // border in lockstep. The label is the only child; the body's
        // bg/border come from the button. Active variant (`is_on`) keeps
        // dimming its own bright fill on press so the user sees feedback;
        // idle variant brightens its bg toward `bg1` on hover so the chip
        // reads as targetable.
        let chip_body = container(label)
            .padding(chip_padding)
            .align_y(Alignment::Center)
            .align_x(Alignment::Center);
        let radius = theme::ui_radius_pill();
        let click_key = option.key.clone();
        button(chip_body)
            .on_press(on_click(click_key))
            .padding(0)
            .style(move |_theme: &iced::Theme, status| {
                let (fill, outline) = match status {
                    button::Status::Hovered => {
                        if is_on {
                            (
                                scale_alpha(theme::accent(), effective_opacity),
                                border_color,
                            )
                        } else {
                            (
                                scale_alpha(theme::bg1(), effective_opacity),
                                theme::accent(),
                            )
                        }
                    }
                    button::Status::Pressed => {
                        let press_bg = if is_on { theme::accent() } else { theme::bg2() };
                        (
                            scale_alpha(press_bg, effective_opacity),
                            scale_alpha(theme::accent_bright(), effective_opacity),
                        )
                    }
                    button::Status::Active | button::Status::Disabled => (bg_color, border_color),
                };
                button::Style {
                    background: Some(fill.into()),
                    border: Border {
                        color: outline,
                        width: 1.0,
                        radius,
                    },
                    ..Default::default()
                }
            })
            .into()
    } else {
        // Non-center rows: static chip painted by a styled container —
        // no hover/press feedback (clicks bubble to the slot-list row).
        container(label)
            .padding(chip_padding)
            .align_y(Alignment::Center)
            .align_x(Alignment::Center)
            .style(move |_: &iced::Theme| container::Style {
                background: Some(bg_color.into()),
                border: Border {
                    color: border_color,
                    width: 1.0,
                    radius: theme::ui_radius_pill(),
                },
                ..Default::default()
            })
            .into()
    }
}

/// Scale a color's alpha by the given factor.
#[inline]
fn scale_alpha(c: Color, factor: f32) -> Color {
    Color {
        a: c.a * factor,
        ..c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_alpha_preserves_rgb_zero_factor() {
        let c = Color::from_rgba(0.25, 0.5, 0.75, 0.8);
        let scaled = scale_alpha(c, 0.5);
        assert_eq!(scaled.r, 0.25);
        assert_eq!(scaled.g, 0.5);
        assert_eq!(scaled.b, 0.75);
        assert_eq!(scaled.a, 0.4);
    }

    #[test]
    fn scale_alpha_identity_for_factor_one() {
        let c = Color::from_rgba(0.1, 0.2, 0.3, 0.6);
        let scaled = scale_alpha(c, 1.0);
        assert_eq!(scaled.r, c.r);
        assert_eq!(scaled.a, c.a);
    }

    #[test]
    fn pill_option_construction_compiles() {
        let opt = PillOption {
            display: "On".to_string(),
            key: "On".to_string(),
            on: true,
        };
        assert_eq!(opt.display, "On");
        assert!(opt.on);
    }

    #[test]
    fn pill_variant_single_has_no_cursor() {
        // PillVariant::Single is uninhabited cursor-wise — the impl ignores
        // cursor_index entirely. This is a structural guard so a future
        // refactor doesn't sneak a cursor field into the single-select case
        // without also handling its precedence vs `on`.
        match PillVariant::Single {
            PillVariant::Single => {}
            PillVariant::Multi { .. } => panic!("Single variant should not match Multi"),
        }
    }
}
