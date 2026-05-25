use std::borrow::Cow;

use iced::{
    Alignment, Element, Length,
    font::{Font, Weight},
    widget::{container, mouse_area, pick_list, row, text},
};
// Re-export SortMode from data crate (canonical definition)
pub(crate) use nokkvi_data::types::sort_mode::SortMode;

use super::hover_overlay::HoverOverlay;
use crate::theme;

/// Wrapper used internally by `view_header` to splice a "Roulette" entry
/// into the sort dropdown without polluting any per-view sort enum. The
/// `current_view` is always `Mode(_)`; `Roulette` only appears in the
/// option list and is intercepted by the `pick_list` select handler so it
/// never becomes the persisted sort.
#[derive(Debug, Clone, PartialEq)]
enum SortPickerEntry<V> {
    Mode(V),
    Roulette,
}

impl<V: std::fmt::Display> std::fmt::Display for SortPickerEntry<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mode(v) => v.fmt(f),
            // Plain text — `pick_list` items render through `text(...)`
            // which has no inline SVG support; a dice icon would need a
            // separate header button rather than dropdown decoration.
            Self::Roulette => f.write_str("Roulette"),
        }
    }
}

/// One optional toolbar button, rendered left-to-right in push order.
///
/// Callers push only the buttons they want — unused variants are invisible.
/// Adding a new button type requires extending this enum and the render
/// loop in [`view_header`]; views that don't want the new button are
/// completely untouched.
pub(crate) enum HeaderButton<'a, Message> {
    SortToggle(Message),
    Refresh(Message),
    CenterOnPlaying(Message),
    /// `(tooltip_label, on_press)` — the plus-icon "add" button.
    Add(&'static str, Message),
    /// Arbitrary element rendered between the built-in buttons and the
    /// search field (e.g. the columns-cog dropdown).
    Trailing(Element<'a, Message>),
}

/// Configuration for [`view_header`].
///
/// `on_roulette` is NOT a button — it injects a "Roulette" entry into the
/// sort picker dropdown and is intercepted before reaching the standard
/// `on_view_selected` path. It must stay a separate field.
pub(crate) struct ViewHeaderConfig<'a, V, Message> {
    pub current_view: V,
    pub view_options: &'a [V],
    pub sort_ascending: bool,
    pub search_query: &'a str,
    pub filtered_count: usize,
    pub total_count: usize,
    pub item_type: &'a str,
    /// Unique ID for this view's search input (must be `'static`).
    pub search_input_id: &'static str,
    pub on_view_selected: Box<dyn Fn(V) -> Message + 'a>,
    pub show_search: bool,
    pub on_search_change: Box<dyn Fn(String) -> Message + 'a>,
    /// Toolbar buttons, rendered in push order.
    pub buttons: Vec<HeaderButton<'a, Message>>,
    /// When `Some`, appends a "Roulette" action entry to the sort dropdown
    /// that emits this message on selection. The dropdown's persisted sort
    /// is left untouched.
    pub on_roulette: Option<Message>,
}

/// Flat-mode header height — `bg0_hard()` strip with sided-border cells
/// (50 px matches the design's `.nk-controls` row).
const FLAT_HEADER_HEIGHT: f32 = 50.0;

/// Rounded-mode header height — pill capsule inside a 12 px outer margin,
/// 4 px inner padding (44 px tall capsule + 8 px vertical margins = 52 px
/// total chrome, matching the design's `--r-pill` capsule treatment).
const ROUNDED_CAPSULE_HEIGHT: f32 = 44.0;

/// Outer horizontal/vertical margin around the rounded-mode pill capsule.
const ROUNDED_OUTER_MARGIN_X: f32 = 16.0;
const ROUNDED_OUTER_MARGIN_Y: f32 = 12.0;

/// Inner padding inside the rounded-mode pill capsule.
const ROUNDED_INNER_PADDING: f32 = 4.0;

/// Pixel-perfect cell width for header icon buttons in flat mode. Mirrors
/// `.nk-ctrl-btn { width: 44px }` from the design CSS — narrower than
/// `ICON_BUTTON_SIZE` (40 px) gets, because the divider hairlines on
/// either side of the cell already separate it from its neighbors.
const FLAT_ICON_CELL_WIDTH: f32 = 44.0;

/// Rounded-mode header icon-button cell width (`.nk-ctrl-btn` rounded
/// override). Slightly narrower because the pill chrome around the row
/// adds visual breathing room.
const ROUNDED_ICON_CELL_WIDTH: f32 = 36.0;

/// Min-width of the sort-dropdown cell in both modes. Matches
/// `.nk-ctrl-sort { min-width: 130px }`.
const SORT_CELL_MIN_WIDTH: f32 = 130.0;

/// ViewHeader component - horizontal bar with view selector, sort, search, and count
/// Generic over sort mode V to support different view enums (Albums, Queue, etc.)
pub(crate) fn view_header<
    'a,
    Message: 'a + Clone,
    V: 'a + std::fmt::Display + Clone + PartialEq,
>(
    config: ViewHeaderConfig<'a, V, Message>,
) -> Element<'a, Message> {
    let ViewHeaderConfig {
        current_view,
        view_options,
        sort_ascending,
        search_query,
        filtered_count,
        total_count,
        item_type,
        search_input_id,
        on_view_selected,
        show_search,
        on_search_change,
        buttons,
        on_roulette,
    } = config;

    let is_rounded = theme::is_rounded_mode();
    let cell_height = if is_rounded {
        ROUNDED_CAPSULE_HEIGHT - 2.0 * ROUNDED_INNER_PADDING
    } else {
        FLAT_HEADER_HEIGHT
    };

    let view_selector: Element<'a, Message> = if view_options.is_empty() {
        // Static label cell — rendered when the view supplies no sort
        // options (e.g. Settings, Login).
        container(
            text(current_view.to_string())
                .size(12.0)
                .font(Font {
                    weight: Weight::Medium,
                    ..theme::ui_font()
                })
                .color(theme::fg0())
                .wrapping(iced::widget::text::Wrapping::None)
                .ellipsis(iced::widget::text::Ellipsis::End),
        )
        .padding([0, 14])
        .max_width(300.0)
        .align_y(Alignment::Center)
        .height(Length::Fixed(cell_height))
        .into()
    } else {
        // Wrap V into SortPickerEntry so we can splice a trailing
        // "Roulette" action without polluting the per-view sort enum.
        let mut entries: Vec<SortPickerEntry<V>> = view_options
            .iter()
            .cloned()
            .map(SortPickerEntry::Mode)
            .collect();
        if on_roulette.is_some() {
            entries.push(SortPickerEntry::Roulette);
        }

        let on_roulette_owned = on_roulette;
        let select_handler = move |entry: SortPickerEntry<V>| match entry {
            SortPickerEntry::Mode(v) => on_view_selected(v),
            SortPickerEntry::Roulette => on_roulette_owned
                .clone()
                .expect("Roulette entry only present when on_roulette is Some"),
        };

        // The pick_list itself is transparent — the surrounding cell
        // wrapper supplies the divider hairline (flat) or capsule
        // (rounded). Hover/open accent shows on the dropdown's own
        // border so the affordance stays discoverable.
        container(
            pick_list(
                Some(SortPickerEntry::Mode(current_view)),
                Cow::<'a, [SortPickerEntry<V>]>::Owned(entries),
                |entry: &SortPickerEntry<V>| entry.to_string(),
            )
            .on_select(select_handler)
            .width(Length::Shrink)
            .text_size(12.0)
            .font(Font {
                weight: Weight::Medium,
                ..theme::ui_font()
            })
            .padding([8, 12])
            .style(move |_theme, status| pick_list::Style {
                text_color: theme::fg0(),
                placeholder_color: theme::fg4(),
                handle_color: theme::fg4(),
                background: iced::Color::TRANSPARENT.into(),
                border: iced::Border {
                    color: match status {
                        pick_list::Status::Active | pick_list::Status::Disabled => {
                            iced::Color::TRANSPARENT
                        }
                        pick_list::Status::Hovered | pick_list::Status::Opened { .. } => {
                            theme::accent_bright()
                        }
                    },
                    width: 1.0,
                    radius: theme::ui_radius_pill(),
                },
            })
            .menu_style(move |_theme| iced::widget::overlay::menu::Style {
                text_color: theme::fg0(),
                background: theme::bg1().into(),
                border: iced::Border {
                    color: theme::border(),
                    width: 1.0,
                    radius: theme::ui_radius_sm(),
                },
                selected_text_color: theme::bg0_hard(),
                selected_background: theme::accent_bright().into(),
                shadow: iced::Shadow::default(),
            }),
        )
        .height(Length::Fixed(cell_height))
        .align_y(Alignment::Center)
        .padding([0, 6])
        .into()
    };

    // Render each requested toolbar button into a cell. Render order is
    // controlled by the caller's push order — keep call sites consistent
    // with the legacy positional ordering: SortToggle, Refresh,
    // CenterOnPlaying, Add, Trailing.
    let mut button_cells: Vec<Element<'a, Message>> = Vec::with_capacity(buttons.len());
    for btn in buttons {
        match btn {
            HeaderButton::SortToggle(sort_msg) => {
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
                button_cells.push(header_icon_cell(
                    sort_icon_path,
                    tooltip_text,
                    sort_msg,
                    cell_height,
                ));
            }
            HeaderButton::Refresh(refresh_msg) => {
                button_cells.push(header_icon_cell(
                    "assets/icons/refresh-cw.svg",
                    "Refresh Data",
                    refresh_msg,
                    cell_height,
                ));
            }
            HeaderButton::CenterOnPlaying(center_msg) => {
                button_cells.push(header_icon_cell(
                    "assets/icons/locate.svg",
                    "Center on Playing",
                    center_msg,
                    cell_height,
                ));
            }
            HeaderButton::Add(tooltip, add_msg) => {
                button_cells.push(header_icon_cell(
                    "assets/icons/plus.svg",
                    tooltip,
                    add_msg,
                    cell_height,
                ));
            }
            HeaderButton::Trailing(element) => {
                // External elements (columns dropdown, shuffle button) come
                // pre-styled by their owners — wrap in a height-locked
                // container so they line up with the row's cell rhythm.
                button_cells.push(
                    container(element)
                        .height(Length::Fixed(cell_height))
                        .align_y(Alignment::Center)
                        .into(),
                );
            }
        }
    }

    let search_field: Option<Element<'a, Message>> = if show_search {
        let bar = crate::widgets::search_bar::search_bar(
            search_query,
            "Search...",
            search_input_id,
            on_search_change,
            None,
        );
        Some(
            container(bar)
                .width(Length::Fill)
                .height(Length::Fixed(cell_height))
                .align_y(Alignment::Center)
                .padding(if is_rounded {
                    iced::Padding {
                        top: 0.0,
                        right: 4.0,
                        bottom: 0.0,
                        left: 4.0,
                    }
                } else {
                    iced::Padding {
                        top: 0.0,
                        right: 8.0,
                        bottom: 0.0,
                        left: 8.0,
                    }
                })
                .into(),
        )
    } else {
        None
    };

    let count_text = if filtered_count > 0 && filtered_count < total_count {
        format!("{filtered_count} of {total_count} {item_type}")
    } else {
        format!("{total_count} {item_type}")
    };

    let count_cell: Element<'a, Message> = container(
        text(count_text)
            .size(12.0)
            .font(Font {
                weight: Weight::Medium,
                ..theme::ui_font()
            })
            .color(theme::fg2())
            .width(Length::Shrink),
    )
    .padding([0, 14])
    .height(Length::Fixed(cell_height))
    .align_y(Alignment::Center)
    .into();

    // Wrap the sort-dropdown cell with a sided-border divider (flat) or
    // a no-op pad (rounded). Pinning to a fixed `SORT_CELL_MIN_WIDTH`
    // matches the design's `.nk-ctrl-sort { min-width: 130px }` and
    // keeps the rest of the row aligned across views with different
    // sort-mode labels.
    let view_selector_cell: Element<'a, Message> = wrap_header_cell(
        container(view_selector)
            .width(Length::Fixed(SORT_CELL_MIN_WIDTH))
            .into(),
        is_rounded,
        true,
    );

    // Build the row of cells. Flat mode has no inter-cell spacing (cells'
    // sided borders touch). Rounded mode uses a 2 px gap between cells
    // inside the pill capsule.
    let mut header_row = row![].align_y(Alignment::Center).spacing(if is_rounded {
        2.0
    } else {
        0.0
    });

    header_row = header_row.push(view_selector_cell);
    for cell in button_cells {
        header_row = header_row.push(wrap_header_cell(cell, is_rounded, true));
    }
    if let Some(search_element) = search_field {
        header_row = header_row.push(wrap_header_cell(search_element, is_rounded, true));
    } else {
        // No search bar — push a flex spacer so the count cell still ends
        // up flush-right. The spacer is wrapped to provide the divider
        // before the count in flat mode.
        header_row = header_row.push(wrap_header_cell(
            iced::widget::Space::new()
                .width(Length::Fill)
                .height(Length::Fixed(cell_height))
                .into(),
            is_rounded,
            true,
        ));
    }
    // Count cell is the row terminator — no trailing divider after it.
    header_row = header_row.push(wrap_header_cell(count_cell, is_rounded, false));

    if is_rounded {
        // Rounded mode: wrap the row in a pill capsule inside a 12 px outer
        // margin. The capsule itself has 1 px theme::border() outline,
        // bg0_hard() fill, and pill corners.
        let capsule = container(header_row.width(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fixed(ROUNDED_CAPSULE_HEIGHT))
            .padding(ROUNDED_INNER_PADDING)
            .style(|_| container::Style {
                background: Some(theme::bg0_hard().into()),
                border: iced::Border {
                    color: theme::border(),
                    width: 1.0,
                    radius: theme::ui_radius_pill(),
                },
                ..Default::default()
            });
        container(capsule)
            .width(Length::Fill)
            .padding(iced::Padding {
                top: ROUNDED_OUTER_MARGIN_Y,
                right: ROUNDED_OUTER_MARGIN_X,
                bottom: ROUNDED_OUTER_MARGIN_Y,
                left: ROUNDED_OUTER_MARGIN_X,
            })
            .into()
    } else {
        // Flat mode: 50 px tall bg0_hard() strip with a bottom 1 px
        // theme::border() separator. The separator is drawn by using
        // the container's border-bottom via uniform Border + matching bg.
        container(header_row.width(Length::Fill).height(Length::Fixed(cell_height)))
            .width(Length::Fill)
            .height(Length::Fixed(FLAT_HEADER_HEIGHT))
            .style(|_| container::Style {
                background: Some(theme::bg0_hard().into()),
                border: iced::Border {
                    color: theme::border(),
                    width: 1.0,
                    radius: iced::border::Radius::default(),
                },
                ..Default::default()
            })
            .into()
    }
}

/// Wrap a header cell with the redesign's sided-border divider treatment.
///
/// Flat mode: appends a 1 px right border (`theme::border()`) so adjacent
/// cells form a sided-divider rhythm matching the design's
/// `border-right: 1px solid #1a2024` cells. Setting `trailing_divider` to
/// `false` suppresses the right border on the row's final cell.
///
/// Rounded mode: no per-cell border at all — the surrounding pill capsule
/// owns the chrome.
fn wrap_header_cell<'a, Message: 'a>(
    inner: Element<'a, Message>,
    is_rounded: bool,
    trailing_divider: bool,
) -> Element<'a, Message> {
    if is_rounded {
        return inner;
    }
    if !trailing_divider {
        return inner;
    }
    // Use a right-side 1 px sibling stripe rather than the container's
    // `border` field — iced's Border draws all 4 sides with uniform width.
    // A sibling stripe gives us a clean right-only divider without
    // affecting the cell's top/bottom/left edges.
    row![
        container(inner).height(Length::Fill),
        container(iced::widget::Space::new())
            .width(Length::Fixed(1.0))
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(theme::border().into()),
                ..Default::default()
            }),
    ]
    .align_y(Alignment::Center)
    .height(Length::Fill)
    .into()
}

/// Reusable header icon button — mode-aware cell width + transparent
/// background. Hover overlay handles the press feedback; the surrounding
/// `wrap_header_cell` supplies the divider chrome.
fn header_icon_cell<'a, Message: Clone + 'a>(
    icon_path: &str,
    tooltip_text: &str,
    on_press: Message,
    cell_height: f32,
) -> Element<'a, Message> {
    use iced::widget::{svg, tooltip};

    let is_rounded = theme::is_rounded_mode();
    let cell_width = if is_rounded {
        ROUNDED_ICON_CELL_WIDTH
    } else {
        FLAT_ICON_CELL_WIDTH
    };

    let icon_svg = crate::embedded_svg::svg_widget(icon_path)
        .width(Length::Fixed(18.0))
        .height(Length::Fixed(18.0))
        .style(|_theme, _status| svg::Style {
            color: Some(theme::fg0()),
        });

    tooltip(
        mouse_area(
            HoverOverlay::new(
                container(icon_svg)
                    .width(Length::Fixed(cell_width))
                    .height(Length::Fixed(cell_height))
                    .center(Length::Fill),
            )
            .border_radius(theme::ui_radius_pill()),
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
    fn header_icon_cell_produces_element() {
        // Characterization test: the extracted helper compiles and produces a valid Element.
        let _el: Element<'_, String> = header_icon_cell(
            "assets/icons/locate.svg",
            "Center on Playing",
            "test_press".to_string(),
            44.0,
        );
    }

    #[test]
    fn wrap_header_cell_no_divider_returns_inner_in_flat() {
        // Trailing cell (count) in flat mode must NOT add a right border.
        let inner: Element<'_, String> = iced::widget::text("count").into();
        let _ = wrap_header_cell(inner, false, false);
    }

    #[test]
    fn wrap_header_cell_with_divider_in_flat_wraps_in_row() {
        let inner: Element<'_, String> = iced::widget::text("cell").into();
        let _ = wrap_header_cell(inner, false, true);
    }

    #[test]
    fn wrap_header_cell_passes_through_in_rounded() {
        let inner: Element<'_, String> = iced::widget::text("cell").into();
        // Rounded mode never adds dividers — pass-through.
        let _ = wrap_header_cell(inner, true, true);
    }
}
