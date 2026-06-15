//! Reusable widget style helpers — containers, separators, modal chrome,
//! settings inputs, the iced Theme bridge, and toast level colors.

use iced::Color;
// ============================================================================
// Container Style Helpers
// ============================================================================
// These functions can be used directly with `.style(theme::container_bg0_hard)`
// instead of writing inline closures like `.style(|_theme| container::Style { ... })`
use iced::{
    Theme,
    widget::{container, text_input},
};

use super::{
    accent_bright, bg0_hard, bg1, bg2, border, danger, fg0, fg1, fg4, selection_color, success,
    ui_border_radius, ui_radius_lg, ui_radius_xs, warning,
};

/// Container with BG0_HARD background (darkest)
pub(crate) fn container_bg0_hard(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(bg0_hard().into()),
        ..Default::default()
    }
}

/// Themed tooltip container style — `bg0_hard` fill, `theme::border()`
/// hairline, and the design's smallest corner radius. Migrated onto the
/// shared chrome tokens so tooltip corners pick up the active theme's
/// per-palette border color and the global flat-vs-rounded toggle.
pub(crate) fn container_tooltip(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(bg0_hard().into()),
        border: iced::Border {
            color: border(),
            width: 1.0,
            radius: ui_radius_xs(),
        },
        text_color: Some(fg1()),
        ..Default::default()
    }
}

/// Full-width horizontal separator line.
///
/// Renders as a `border()`-colored container with the given pixel height.
/// Replaces the inline `container(space()).width(Fill).height(Fixed(h)).style(bg1)`
/// pattern that was duplicated across `player_bar.rs`, `track_info_strip.rs`,
/// and `app_view.rs`. The redesign aligned every 1 px chrome rule onto the
/// shared `theme::border()` token, so this helper now reads the same
/// hairline color as the modal/menu/nav-bar separator family.
pub(crate) fn horizontal_separator<'a, M: 'a>(height: f32) -> iced::Element<'a, M> {
    use iced::{
        Length,
        widget::{container, space},
    };
    container(space())
        .width(Length::Fill)
        .height(Length::Fixed(height))
        .style(move |_| container::Style {
            background: Some(border().into()),
            ..Default::default()
        })
        .into()
}

/// Fixed-height vertical separator line (1px wide, `border()` colored).
///
/// Used inside info strip rows to delineate fields. Shares the same
/// `theme::border()` hairline color as `horizontal_separator` and the
/// rest of the chrome separator family.
pub(crate) fn vertical_separator<'a, M: 'a>(height: f32) -> iced::Element<'a, M> {
    use iced::{
        Length,
        widget::{container, space},
    };
    container(space())
        .width(Length::Fixed(1.0))
        .height(Length::Fixed(height))
        .style(move |_| container::Style {
            background: Some(border().into()),
            ..Default::default()
        })
        .into()
}

// ----------------------------------------------------------------------------
// Modal separators
// ----------------------------------------------------------------------------
// Both helpers consolidate the eight near-identical separator lambdas that
// previously lived in `about_modal`, `info_modal`, `eq_modal`, `nav_bar`
// (twice), and `side_nav_bar`. After the flat redesign they share the same
// `border()` token — the design CSS uses the same `#1a2024` for modal-head,
// modal-actions, row separators, popover head, and pop-row borders.

/// 1-px horizontal separator between rows inside a modal.
///
/// Replaces the inline `row_separator` lambdas in `about_modal::info_row`
/// and `info_modal`'s property table.
pub(crate) fn modal_row_separator<'a, M: 'a>() -> iced::Element<'a, M> {
    use iced::{
        Length,
        widget::{container, space},
    };
    container(space::horizontal())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(|_| container::Style {
            background: Some(border().into()),
            ..Default::default()
        })
        .into()
}

/// 1-px horizontal separator under a modal's header.
///
/// Replaces the inline `separator_line` lambdas in `about_modal`, `info_modal`,
/// and `eq_modal`.
pub(crate) fn modal_header_separator<'a, M: 'a>() -> iced::Element<'a, M> {
    use iced::{
        Length,
        widget::{container, space},
    };
    container(space::horizontal())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(|_| container::Style {
            background: Some(border().into()),
            ..Default::default()
        })
        .into()
}

// `NavSeparatorAxis` + `nav_separator` were the canonical "2-px nav-bar
// separator" recipe — both axes, optional `force_visible` to defeat the
// rounded-mode hide. L2 (nav-chrome) replaced them with a 1-px
// `theme::border()`-colored rule local to each nav bar, so the helpers had
// no callers in the redesign. Removed during the cleanup; recover from
// `git show` if a future surface wants the old thick visual.

// ----------------------------------------------------------------------------
// Modal scaffolding
// ----------------------------------------------------------------------------

/// Wrap a modal dialog box in the canonical backdrop + opaque scaffold.
///
/// Produces the `mouse_area(opaque(container(...).style(backdrop)))` Element
/// that all four overlay modals (`about`, `info`, `eq`, `text_input_dialog`)
/// previously open-coded. The backdrop is a semi-transparent `bg0_hard` wash
/// (alpha = `backdrop_alpha`, conventionally `0.6`); clicking it emits
/// `on_backdrop_press` (Close / Cancel depending on caller); `opaque()`
/// blocks pointer events from reaching widgets behind the modal.
///
/// Restraint: only the backdrop layer is consolidated here. The dialog box
/// itself (border color, max_height, fixed width, etc.) stays at each call
/// site because those genuinely diverge between modals.
pub(crate) fn modal_scaffold<'a, M: Clone + 'a>(
    dialog_box: iced::Element<'a, M>,
    on_backdrop_press: M,
    backdrop_alpha: f32,
) -> iced::Element<'a, M> {
    use iced::{
        Alignment, Length,
        widget::{container, mouse_area, opaque},
    };
    let backdrop = mouse_area(
        container(opaque(dialog_box))
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .style(move |_| {
                let mut bg = bg0_hard();
                bg.a = backdrop_alpha;
                container::Style {
                    background: Some(bg.into()),
                    ..Default::default()
                }
            }),
    )
    .on_press(on_backdrop_press);
    opaque(backdrop)
}

/// Conventional backdrop alpha used by every overlay modal.
pub(crate) const MODAL_BACKDROP_ALPHA: f32 = 0.6;

/// Shared `container::Style` for overlay modal panels — flat `bg0_hard()`
/// fill, 1 px `accent_bright()` outline, `ui_radius_lg()` corners.
///
/// Five overlay modals (`about`, `info`, `eq`, `text_input_dialog`,
/// `default_playlist_picker`) open-coded this exact block. Routing them
/// through one function means a future per-theme tweak to the modal frame
/// (e.g. swapping the outline onto `border()` for the chrome-quiet variant)
/// only touches this body — and the radius / fill / border are all
/// guaranteed to stay in lockstep across the modal family.
pub(crate) fn modal_frame_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(bg0_hard().into()),
        border: iced::Border {
            color: accent_bright(),
            width: 1.0,
            radius: ui_radius_lg(),
        },
        ..Default::default()
    }
}

// ----------------------------------------------------------------------------
// Transparent button style
// ----------------------------------------------------------------------------

/// Borderless button style: no background when idle, `bg1` on hover, no
/// outline, `ui_border_radius()` corners. Hoisted from
/// `default_playlist_picker::transparent_button_style` so future callers can
/// find it without re-inventing.
pub(crate) fn transparent_button_style(
    _theme: &Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    use iced::widget::button;
    button::Style {
        background: match status {
            button::Status::Hovered => Some(bg1().into()),
            _ => None,
        },
        text_color: fg0(),
        border: iced::Border {
            radius: ui_border_radius(),
            ..Default::default()
        },
        ..Default::default()
    }
}

// `theme::search_input_style` was the legacy 2 px-bordered Gruvbox view-header
// style. The L3 flat redesign moved view-header callers to
// `search_bar::flat_search_input_style` and the L5 settings UI runs through
// `settings_search_input_style` below; the original helper had no remaining
// callers and was removed during the cleanup. Recover from `git show` if a
// future caller wants the old visual.

/// Specialized search style for settings panels so it doesn't blend into bg0_soft.
pub(crate) fn settings_search_input_style(
    _theme: &Theme,
    status: text_input::Status,
) -> text_input::Style {
    text_input::Style {
        background: (bg0_hard()).into(),
        border: iced::Border {
            color: if matches!(status, text_input::Status::Focused { .. }) {
                accent_bright()
            } else {
                bg2()
            },
            width: 1.0,
            radius: ui_border_radius(),
        },
        icon: fg4(),
        placeholder: fg4(),
        value: fg0(),
        selection: selection_color(),
    }
}

/// Themed scrollbar style for the settings detail pane: `bg2` rail, `fg4`
/// scroller resting, `accent_bright` scroller on hover/drag. Matches the
/// info-modal scrollable's chrome so all in-settings scrollable surfaces
/// read consistently against the flat-redesign palette.
pub(crate) fn settings_scrollable_style(
    _theme: &Theme,
    status: iced::widget::scrollable::Status,
) -> iced::widget::scrollable::Style {
    use iced::widget::{container, scrollable};

    let rail = scrollable::Rail {
        background: Some(bg2().into()),
        border: iced::Border {
            radius: ui_border_radius(),
            ..Default::default()
        },
        scroller: scrollable::Scroller {
            background: fg4().into(),
            border: iced::Border {
                radius: ui_border_radius(),
                ..Default::default()
            },
        },
    };
    let hot_rail = scrollable::Rail {
        scroller: scrollable::Scroller {
            background: accent_bright().into(),
            ..rail.scroller
        },
        ..rail
    };
    let auto_scroll = scrollable::AutoScroll {
        background: iced::Color::TRANSPARENT.into(),
        border: iced::Border::default(),
        shadow: iced::Shadow::default(),
        icon: iced::Color::TRANSPARENT,
    };

    match status {
        scrollable::Status::Active { .. } => scrollable::Style {
            container: container::Style::default(),
            vertical_rail: rail,
            horizontal_rail: rail,
            gap: None,
            auto_scroll,
        },
        scrollable::Status::Hovered {
            is_vertical_scrollbar_hovered,
            is_horizontal_scrollbar_hovered,
            ..
        } => scrollable::Style {
            container: container::Style::default(),
            vertical_rail: if is_vertical_scrollbar_hovered {
                hot_rail
            } else {
                rail
            },
            horizontal_rail: if is_horizontal_scrollbar_hovered {
                hot_rail
            } else {
                rail
            },
            gap: None,
            auto_scroll,
        },
        scrollable::Status::Dragged {
            is_vertical_scrollbar_dragged,
            is_horizontal_scrollbar_dragged,
            ..
        } => scrollable::Style {
            container: container::Style::default(),
            vertical_rail: if is_vertical_scrollbar_dragged {
                hot_rail
            } else {
                rail
            },
            horizontal_rail: if is_horizontal_scrollbar_dragged {
                hot_rail
            } else {
                rail
            },
            gap: None,
            auto_scroll,
        },
    }
}

// ============================================================================
// Iced Theme Integration
// ============================================================================

/// Build a custom `iced::Theme` from the current live Gruvbox colors.
///
/// This maps the Gruvbox palette into an `iced::Palette` so that widgets
/// relying on the default Iced catalog styles (e.g. the scrollbar inside
/// `combo_box` menus) pick up Gruvbox colors instead of the built-in defaults.
///
/// Since all other widgets in the app use closure-based `.style()` that ignore
/// the `&Theme` parameter, this only affects widgets that fall through to the
/// Iced catalog default — notably the combo_box dropdown scrollbar.
pub(crate) fn iced_theme() -> Theme {
    use iced::theme::palette::Seed;

    let palette = Seed {
        background: bg0_hard(),
        text: fg0(),
        primary: accent_bright(),
        success: success(),
        warning: warning(),
        danger: danger(),
    };

    Theme::custom("Nokkvi", palette)
}

// ============================================================================
// Toast Level Colors
// ============================================================================

/// Map toast notification level to a theme-appropriate text color.
/// Uses the `base` (non-bright) color variants because:
/// - In dark themes, `base` colors are still vivid and readable
/// - In light themes, `bright` colors wash out against light backgrounds
/// - Theme authors set `base` variants to be readable against their chosen bg colors
pub(crate) fn toast_level_color(level: nokkvi_data::types::toast::ToastLevel) -> Color {
    use nokkvi_data::types::toast::ToastLevel;
    match level {
        ToastLevel::Info => fg1(),
        ToastLevel::Success => success(),
        ToastLevel::Warning => warning(),
        ToastLevel::Error => danger(),
    }
}
