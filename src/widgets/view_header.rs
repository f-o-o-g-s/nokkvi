use std::borrow::Cow;

use iced::{
    Alignment, Element, Length,
    font::Weight,
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
    /// Anchor-icon button opening the Trawl mix builder.
    Trawl(Message),
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
    /// Auto-hide toolbar: when `true`, render only the thin
    /// [`COLLAPSED_HEADER_HEIGHT`] hairline instead of the full toolbar. The
    /// caller flips this from the page's `toolbar_revealed()` state.
    pub collapsed: bool,
    /// Auto-hide toolbar: hover-reveal callbacks. When both are `Some` the
    /// header (hairline or full) is wrapped in a `mouse_area` so entering it
    /// reveals the toolbar and leaving it collapses again. `None` (the
    /// default for non-participating headers, e.g. Similar's static label)
    /// leaves the header always-on with no hover wiring.
    pub on_hover_enter: Option<Message>,
    pub on_hover_exit: Option<Message>,
    /// Auto-hide toolbar: the sort dropdown's open/close hooks. When supplied
    /// (auto-hide on), opening the dropdown keeps the toolbar revealed while
    /// the cursor is in the open menu (off the header); `None` skips the wiring.
    pub on_dropdown_open: Option<Message>,
    pub on_dropdown_close: Option<Message>,
    /// Count strip only: total duration (seconds) of the shown items, appended
    /// to the count as e.g. "12 songs · 47m". `None` (or 0) shows no duration —
    /// used for views whose items carry no duration (Artists, Genres, Radios).
    pub total_duration_secs: Option<u64>,
    /// When `Some`, the sort dropdown shows this grayed placeholder text instead
    /// of `current_view`, and no entry is marked selected. Used by the Queue
    /// view to render "Unsorted" until the user applies a queue sort (the queue
    /// takes its order from whatever populated it, so a remembered mode would
    /// misrepresent the actual order). All other views pass `None`.
    pub sort_placeholder: Option<&'a str>,
}

/// View-header height — `bg0_hard()` strip with sided-border cells
/// (50 px matches the design's `.nk-controls` row). Rendered identically in
/// flat and rounded modes: the surrounding pill chrome looked out of place
/// stacked above the slot-list shell, so the header keeps its flat
/// treatment regardless of the global rounded-mode toggle.
///
/// Exposed to `slot_list::view_header_chrome()` so the slot-count math
/// derives from this single source of truth.
pub(crate) const HEADER_HEIGHT: f32 = 50.0;

/// Height of the 1 px `theme::border()` sibling separator rendered below the
/// header strip. Counted as part of the header's chrome footprint so callers
/// that stack additional bars below (queue edit-mode, playlist-context) can
/// add their own bars onto a chrome total that already accounts for the
/// separator under the view-header itself.
pub(crate) const HEADER_BOTTOM_SEPARATOR: f32 = 1.0;

/// Width of the centered accent "grip" bar drawn on the collapsed auto-hide
/// toolbar when `theme::is_autohide_toolbar_grip()` is set — a "hover here"
/// hint. The collapsed strip's height is user-configurable via
/// `theme::autohide_toolbar_height_px()` (chrome total derived in
/// `slot_list::collapsed_view_header_chrome()`).
const GRIP_WIDTH: f32 = 44.0;
/// Thickness of the grip bar. Stays within the smallest configurable collapsed
/// height (4 px) with margin to spare.
const GRIP_HEIGHT: f32 = 2.0;

/// Invisible top catch-zone height for the "Hidden" collapsed appearance —
/// visually nothing, but still a (thin) mouse hover target so a flick to the
/// top edge reveals the toolbar. Exposed for `slot_list::collapsed_view_header_chrome`.
pub(crate) const HIDDEN_CATCH_HEIGHT: f32 = 3.0;

/// Height of the "Count strip" collapsed appearance — a slim read-only strip
/// echoing the current sort + item count. Exposed for the chrome math.
pub(crate) const COUNT_STRIP_HEIGHT: f32 = 24.0;

/// Pixel-perfect cell width for header icon buttons. Mirrors
/// `.nk-ctrl-btn { width: 44px }` from the design CSS — narrower than
/// `ICON_BUTTON_SIZE` (40 px) gets, because the divider hairlines on
/// either side of the cell already separate it from its neighbors.
const ICON_CELL_WIDTH: f32 = 44.0;

/// Min-width of the sort-dropdown cell. Matches
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
        collapsed,
        on_hover_enter,
        on_hover_exit,
        on_dropdown_open,
        on_dropdown_close,
        total_duration_secs,
        sort_placeholder,
    } = config;

    // Auto-hide collapsed state: render only a thin `bg0_hard()` sliver (plus
    // the usual bottom separator) standing in for the full toolbar, wrapped in
    // the same hover zone so entering it reveals the toolbar. The chrome math
    // reserves only this configured height, so the slot list reclaims the
    // freed space while hidden. An optional centered accent grip bar hints
    // that the strip is interactive.
    if collapsed {
        use nokkvi_data::types::player_settings::CollapsedAppearance;
        let collapsed_el: Element<'a, Message> = match theme::autohide_collapsed_appearance() {
            // Hairline: a thin `bg0_hard` sliver (configurable height) with an
            // optional centered accent grip bar.
            CollapsedAppearance::Hairline => {
                let height = f32::from(theme::autohide_toolbar_height_px());
                let inner: Element<'a, Message> = if theme::is_autohide_toolbar_grip() {
                    container(
                        container(iced::widget::Space::new())
                            .width(Length::Fixed(GRIP_WIDTH))
                            .height(Length::Fixed(GRIP_HEIGHT))
                            .style(|_| container::Style {
                                background: Some(theme::accent_bright().into()),
                                border: iced::Border {
                                    radius: (GRIP_HEIGHT / 2.0).into(),
                                    ..Default::default()
                                },
                                ..Default::default()
                            }),
                    )
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(Alignment::Center)
                    .align_y(Alignment::Center)
                    .into()
                } else {
                    iced::widget::Space::new().into()
                };
                let strip = container(inner)
                    .width(Length::Fill)
                    .height(Length::Fixed(height))
                    .style(|_| container::Style {
                        background: Some(theme::bg0_hard().into()),
                        ..Default::default()
                    });
                iced::widget::column![strip, collapsed_separator()].into()
            }
            // Hidden: visually nothing — but a thin transparent strip still
            // catches a mouse flick to the top edge (hotkeys reveal it too).
            // No separator, so the list reads as reclaiming the whole top.
            CollapsedAppearance::Hidden => iced::widget::Space::new()
                .width(Length::Fill)
                .height(Length::Fixed(HIDDEN_CATCH_HEIGHT))
                .into(),
            // Count strip: a slim read-only strip echoing the current sort +
            // direction (left) and the item count (right).
            CollapsedAppearance::CountStrip => {
                let arrow = if sort_ascending { "↑" } else { "↓" };
                // When the view supplies an unsorted placeholder (queue with no
                // applied sort), echo it plainly — no mode, no direction arrow.
                let label = match sort_placeholder {
                    Some(ph) => ph.to_string(),
                    None => format!("{current_view} {arrow}"),
                };
                let count = count_label(filtered_count, total_count, item_type);
                // Append a total-duration stat ("12 songs · 47m") when the view
                // supplies one (song / album / playlist lists).
                let count = match total_duration_secs {
                    Some(secs) if secs > 0 => format!("{count} · {}", format_total_duration(secs)),
                    _ => count,
                };
                // Dimmed, non-interactive icon hints echoing the toolbar's
                // controls — search + the view's action buttons — in the same
                // left-to-right order. The sort is already shown as the `↓`
                // text and the columns cog is an opaque `Trailing` element, so
                // both are omitted. Hovering the strip reveals the real,
                // interactive toolbar; these are purely a "controls live here"
                // affordance.
                let mut hint_icons: Vec<Element<'a, Message>> = Vec::new();
                for btn in &buttons {
                    let path = match btn {
                        HeaderButton::Refresh(_) => Some("assets/icons/refresh-cw.svg"),
                        HeaderButton::CenterOnPlaying(_) => Some("assets/icons/locate.svg"),
                        HeaderButton::Add(_, _) => Some("assets/icons/plus.svg"),
                        HeaderButton::Trawl(_) => Some("assets/icons/anchor.svg"),
                        HeaderButton::SortToggle(_) | HeaderButton::Trailing(_) => None,
                    };
                    if let Some(p) = path {
                        hint_icons.push(hint_icon(p));
                    }
                }
                if show_search {
                    hint_icons.push(hint_icon("assets/icons/search.svg"));
                }
                let strip_row = row![
                    container(
                        text(label)
                            .size(11.0)
                            .font(theme::ui_font())
                            .color(theme::fg2())
                    )
                    .padding([0, 14])
                    .align_y(Alignment::Center)
                    .height(Length::Fill),
                    row(hint_icons)
                        .spacing(7.0)
                        .align_y(Alignment::Center)
                        .height(Length::Fill),
                    iced::widget::Space::new().width(Length::Fill),
                    container(
                        text(count)
                            .size(11.0)
                            .font(theme::ui_font())
                            .color(theme::fg2())
                    )
                    .padding([0, 14])
                    .align_y(Alignment::Center)
                    .height(Length::Fill),
                ]
                .align_y(Alignment::Center)
                .height(Length::Fill);
                let strip = container(strip_row)
                    .width(Length::Fill)
                    .height(Length::Fixed(COUNT_STRIP_HEIGHT))
                    .style(|_| container::Style {
                        background: Some(theme::bg0_hard().into()),
                        ..Default::default()
                    });
                iced::widget::column![strip, collapsed_separator()].into()
            }
        };
        return maybe_hover_wrap(collapsed_el, on_hover_enter, on_hover_exit);
    }

    // All header cells size to `HEADER_HEIGHT`; the previous `cell_height`
    // local was just a rename for the same value (see audit #M-P2-3).

    let view_selector: Element<'a, Message> = if view_options.is_empty() {
        // Static label cell — rendered when the view supplies no sort
        // options (e.g. Settings, Login).
        container(
            text(current_view.to_string())
                .size(12.0)
                .font(theme::weighted_ui_font(Weight::Medium))
                .color(theme::fg0())
                .wrapping(iced::widget::text::Wrapping::None)
                .ellipsis(iced::widget::text::Ellipsis::End),
        )
        .padding([0, 14])
        .max_width(300.0)
        .align_y(Alignment::Center)
        .height(Length::Fixed(HEADER_HEIGHT))
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
        //
        // When `sort_placeholder` is set (queue with no applied sort), no entry
        // is marked selected and iced renders the placeholder text grayed via
        // `placeholder_color` below.
        let selected = match sort_placeholder {
            Some(_) => None,
            None => Some(SortPickerEntry::Mode(current_view)),
        };
        let mut sort_picker = pick_list(
            selected,
            Cow::<'a, [SortPickerEntry<V>]>::Owned(entries),
            |entry: &SortPickerEntry<V>| entry.to_string(),
        )
        .placeholder(sort_placeholder.unwrap_or_default())
        .on_select(select_handler)
        .width(Length::Shrink)
        .text_size(12.0)
        .font(theme::weighted_ui_font(Weight::Medium))
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
                radius: 0.0.into(),
            },
        })
        .menu_style(move |_theme| iced::widget::overlay::menu::Style {
            text_color: theme::fg0(),
            background: theme::bg1().into(),
            border: iced::Border {
                color: theme::border(),
                width: 1.0,
                radius: 0.0.into(),
            },
            selected_text_color: theme::bg0_hard(),
            selected_background: theme::accent_bright().into(),
            shadow: iced::Shadow::default(),
        });
        // Auto-hide: opening the dropdown keeps the toolbar revealed while the
        // cursor is in the menu (off the header); closing drops the lock.
        if let Some(msg) = on_dropdown_open {
            sort_picker = sort_picker.on_open(msg);
        }
        if let Some(msg) = on_dropdown_close {
            sort_picker = sort_picker.on_close(msg);
        }
        container(sort_picker)
            .height(Length::Fixed(HEADER_HEIGHT))
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
                button_cells.push(header_icon_cell(sort_icon_path, tooltip_text, sort_msg));
            }
            HeaderButton::Refresh(refresh_msg) => {
                button_cells.push(header_icon_cell(
                    "assets/icons/refresh-cw.svg",
                    "Refresh Data",
                    refresh_msg,
                ));
            }
            HeaderButton::CenterOnPlaying(center_msg) => {
                button_cells.push(header_icon_cell(
                    "assets/icons/locate.svg",
                    "Center on Playing",
                    center_msg,
                ));
            }
            HeaderButton::Add(tooltip, add_msg) => {
                button_cells.push(header_icon_cell("assets/icons/plus.svg", tooltip, add_msg));
            }
            HeaderButton::Trawl(msg) => {
                button_cells.push(header_icon_cell(
                    "assets/icons/anchor.svg",
                    "Trawl — build a mix",
                    msg,
                ));
            }
            HeaderButton::Trailing(element) => {
                // External elements (columns dropdown, shuffle button) come
                // pre-styled by their owners — wrap in a height-locked
                // container so they line up with the row's cell rhythm.
                button_cells.push(
                    container(element)
                        .height(Length::Fixed(HEADER_HEIGHT))
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
                .height(Length::Fixed(HEADER_HEIGHT))
                .align_y(Alignment::Center)
                .padding(iced::Padding {
                    top: 0.0,
                    right: 8.0,
                    bottom: 0.0,
                    left: 8.0,
                })
                .into(),
        )
    } else {
        None
    };

    let count_text = count_label(filtered_count, total_count, item_type);

    let count_cell: Element<'a, Message> = container(
        text(count_text)
            .size(12.0)
            .font(theme::weighted_ui_font(Weight::Medium))
            .color(theme::fg2())
            .width(Length::Shrink),
    )
    .padding([0, 14])
    .height(Length::Fixed(HEADER_HEIGHT))
    .align_y(Alignment::Center)
    .into();

    // Wrap the sort-dropdown cell with a sided-border divider. Pinning to
    // a fixed `SORT_CELL_MIN_WIDTH` matches the design's
    // `.nk-ctrl-sort { min-width: 130px }` and keeps the rest of the row
    // aligned across views with different sort-mode labels. The static-label
    // branch (Similar's "similar to: …" / "top songs: …") instead shrinks to
    // content up to the inner container's `max_width(300)` — dynamic labels
    // are longer than the dropdown cells and would otherwise ellipsize to a
    // single letter inside the 130 px fixed cell.
    let view_selector_cell: Element<'a, Message> = wrap_header_cell(
        if view_options.is_empty() {
            view_selector
        } else {
            container(view_selector)
                .width(Length::Fixed(SORT_CELL_MIN_WIDTH))
                .into()
        },
        true,
    );

    // Build the row of cells. No inter-cell spacing — adjacent sided
    // borders touch to form the sided-divider rhythm.
    let mut header_row = row![].align_y(Alignment::Center).spacing(0.0);

    header_row = header_row.push(view_selector_cell);
    for cell in button_cells {
        header_row = header_row.push(wrap_header_cell(cell, true));
    }
    if let Some(search_element) = search_field {
        header_row = header_row.push(wrap_header_cell(search_element, true));
    } else {
        // No search bar — push a flex spacer so the count cell still ends
        // up flush-right. The spacer is wrapped to provide the divider
        // before the count.
        header_row = header_row.push(wrap_header_cell(
            iced::widget::Space::new()
                .width(Length::Fill)
                .height(Length::Fixed(HEADER_HEIGHT))
                .into(),
            true,
        ));
    }
    // Count cell is the row terminator — no trailing divider after it.
    header_row = header_row.push(wrap_header_cell(count_cell, false));

    // A bg0_hard() strip plus a 1 px theme::border() sibling separator
    // below it. Using a sibling line instead of the container's `border`
    // field avoids ringing the header with a 4-sided dark frame (Iced's
    // `Border` width applies uniformly to all sides).
    let full: Element<'a, Message> = iced::widget::column![
        container(
            header_row
                .width(Length::Fill)
                .height(Length::Fixed(HEADER_HEIGHT)),
        )
        .width(Length::Fill)
        .height(Length::Fixed(HEADER_HEIGHT))
        .style(|_| container::Style {
            background: Some(theme::bg0_hard().into()),
            ..Default::default()
        }),
        container(iced::widget::Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(1.0))
            .style(|_| container::Style {
                background: Some(theme::border().into()),
                ..Default::default()
            }),
    ]
    .into();

    // When auto-hide is active (hover callbacks supplied), wrap the revealed
    // toolbar in the same hover zone so leaving it collapses again. Search
    // focus / an active query keep it revealed via `toolbar_revealed()`, so
    // the toolbar won't vanish mid-type even if the cursor wanders off.
    maybe_hover_wrap(full, on_hover_enter, on_hover_exit)
}

/// Wrap `el` in a hover-reporting `mouse_area` when both reveal callbacks are
/// supplied (auto-hide enabled); otherwise return it untouched. `on_enter` /
/// `on_exit` are passive (they never capture clicks), so the wrapped toolbar's
/// dropdown, buttons, and search input keep working.
fn maybe_hover_wrap<'a, Message: 'a + Clone>(
    el: Element<'a, Message>,
    on_enter: Option<Message>,
    on_exit: Option<Message>,
) -> Element<'a, Message> {
    match (on_enter, on_exit) {
        (Some(enter), Some(exit)) => mouse_area(el).on_enter(enter).on_exit(exit).into(),
        _ => el,
    }
}

/// The 1 px `theme::border()` sibling separator drawn beneath a collapsed
/// strip (Hairline / Count strip), matching the full header's bottom rule.
fn collapsed_separator<'a, Message: 'a>() -> Element<'a, Message> {
    container(iced::widget::Space::new())
        .width(Length::Fill)
        .height(Length::Fixed(HEADER_BOTTOM_SEPARATOR))
        .style(|_| container::Style {
            background: Some(theme::border().into()),
            ..Default::default()
        })
        .into()
}

/// Item-count label shared by the full header and the CountStrip collapsed
/// appearance: `"{filtered} of {total} {item_type}"` while a search narrows
/// the list, plain `"{total} {item_type}"` otherwise.
fn count_label(filtered: usize, total: usize, item_type: &str) -> String {
    if filtered > 0 && filtered < total {
        format!("{filtered} of {total} {item_type}")
    } else {
        format!("{total} {item_type}")
    }
}

/// Compact total-duration label for the count strip: `47m`, `4h 53m`, or
/// `3d 4h` for very large sets (whole-library song views).
fn format_total_duration(secs: u64) -> String {
    let mins = secs / 60;
    let (hours, mins) = (mins / 60, mins % 60);
    let (days, hours) = (hours / 24, hours % 24);
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {mins}m")
    } else {
        format!("{mins}m")
    }
}

/// A small, dimmed, non-interactive SVG used as a "this control lives here"
/// hint in the Count-strip collapsed appearance. Sized (13 px) to sit inside
/// the slim strip; tinted `fg4()` so it reads as an affordance, not a button.
fn hint_icon<'a, Message: 'a>(path: &str) -> Element<'a, Message> {
    crate::embedded_svg::svg_widget(path)
        .width(Length::Fixed(13.0))
        .height(Length::Fixed(13.0))
        .style(|_theme, _status| iced::widget::svg::Style {
            color: Some(theme::fg4()),
        })
        .into()
}

/// Wrap a header cell with the redesign's sided-border divider treatment.
///
/// Appends a 1 px right border (`theme::border()`) so adjacent cells form
/// a sided-divider rhythm matching the design's
/// `border-right: 1px solid #1a2024` cells. Setting `trailing_divider` to
/// `false` suppresses the right border on the row's final cell.
fn wrap_header_cell<'a, Message: 'a>(
    inner: Element<'a, Message>,
    trailing_divider: bool,
) -> Element<'a, Message> {
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

/// Reusable header icon button — `ICON_CELL_WIDTH × HEADER_HEIGHT` cell,
/// transparent background, square hover. Hover overlay handles the press
/// feedback; surrounding chrome (`wrap_header_cell` or a sibling stripe)
/// supplies the dividers.
///
/// Exposed `pub(crate)` so peer surfaces that need to drop a header-style
/// icon button alongside `view_header`'s own (e.g. `default_playlist_chip`)
/// share the exact same chassis — sizing, hover radius, tooltip vocabulary
/// — without re-deriving the constants.
pub(crate) fn header_icon_cell<'a, Message: Clone + 'a>(
    icon_path: &str,
    tooltip_text: &str,
    on_press: Message,
) -> Element<'a, Message> {
    use iced::widget::{svg, tooltip};

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
                    .width(Length::Fixed(ICON_CELL_WIDTH))
                    .height(Length::Fixed(HEADER_HEIGHT))
                    .align_x(Alignment::Center)
                    .align_y(Alignment::Center),
            )
            .border_radius(0.0.into()),
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
    fn count_label_formats() {
        // No search active: plain total.
        assert_eq!(count_label(0, 100, "albums"), "100 albums");
        // Search matches everything: still the plain total.
        assert_eq!(count_label(100, 100, "albums"), "100 albums");
        // Search narrows the list: "M of N".
        assert_eq!(count_label(12, 100, "albums"), "12 of 100 albums");
    }

    #[test]
    fn format_total_duration_tiers() {
        assert_eq!(format_total_duration(0), "0m");
        assert_eq!(format_total_duration(47 * 60), "47m");
        assert_eq!(format_total_duration(4 * 3600 + 53 * 60), "4h 53m");
        assert_eq!(format_total_duration(3 * 86400 + 4 * 3600), "3d 4h");
    }

    #[test]
    fn header_icon_cell_produces_element() {
        // Characterization test: the extracted helper compiles and produces a valid Element.
        let _el: Element<'_, String> = header_icon_cell(
            "assets/icons/locate.svg",
            "Center on Playing",
            "test_press".to_string(),
        );
    }

    #[test]
    fn wrap_header_cell_no_divider_returns_inner() {
        // Trailing cell (count) must NOT add a right border.
        let inner: Element<'_, String> = iced::widget::text("count").into();
        let _ = wrap_header_cell(inner, false);
    }

    #[test]
    fn wrap_header_cell_with_divider_wraps_in_row() {
        let inner: Element<'_, String> = iced::widget::text("cell").into();
        let _ = wrap_header_cell(inner, true);
    }
}
