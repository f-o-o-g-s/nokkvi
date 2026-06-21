//! Settings view rendering — persistent two-pane layout.
//!
//! Layout: a 340 px categories sidebar on the left + a scrollable detail
//! pane on the right, both flush against the chrome (no outer padding,
//! no panel border, no footer). Search lives in the sidebar header.
//! Sub-lists (color array editor) replace the detail pane content
//! in-place; the font picker still overlays as a modal.
//!
//! All methods are pure `&self` view functions producing
//! `Element<SettingsMessage>`.

use iced::{
    Alignment, Border, Color, Element, Length, Padding,
    font::{Font, Weight},
    widget::{Space, button, column, container, mouse_area, row, stack, svg, text},
};
use nokkvi_data::utils::fuzzy;

use super::{
    BREADCRUMB_HEIGHT, FONT_SEARCH_BAR_HEIGHT, SETTINGS_SEARCH_INPUT_ID, SettingsMessage,
    SettingsPage, SettingsTab,
    items::SettingsEntry,
    rendering::{render_detail_header, render_detail_row, transparent_button_style},
};
use crate::{embedded_svg, theme, widgets::slot_list};

/// Sidebar width (px). Matches the design spec.
pub(super) const SIDEBAR_WIDTH: f32 = 340.0;

/// Height of the sidebar header (Settings title + search input).
const SIDEBAR_HEADER_HEIGHT: f32 = 60.0;

/// Height of the sidebar footer (version + Esc pill).
const SIDEBAR_FOOTER_HEIGHT: f32 = 44.0;

/// Below this window width the layout collapses to the narrow variant —
/// the 340 px sidebar swaps for a horizontal scrollable chip strip
/// above the detail pane. Tuned from a running-app review: at ~1320 px
/// the wide sidebar still rendered but the detail pane felt
/// claustrophobic. 1400 keeps the wide layout for ≥ 1080p monitors and
/// flips narrow on 1366-px laptops and smaller.
const NARROW_BREAKPOINT_WIDTH: f32 = 1400.0;

/// Vertical height of the narrow-variant chip strip (matches the
/// design's `NarrowTabStrip` 12 px / 16 px padding around 32 px pills,
/// plus a 1 px bottom border).
const NARROW_STRIP_HEIGHT: f32 = 56.0;

impl SettingsPage {
    /// Render the settings view — persistent two-pane layout.
    ///
    /// Left: 340 px categories sidebar (always visible).
    /// Right: detail pane for the active category, OR an in-place sub-list
    ///        (color array editor) when one is open.
    /// Font picker still overlays as a modal stack on top of everything.
    ///
    /// Takes only the two window dimensions — the entry list comes from
    /// `cached_entries`, which the update path rebuilds on every mutation
    /// (tab switch, search, config writes, hot-reloads, view entry) via
    /// `refresh_entries` / `Nokkvi::refresh_settings_entries_if_dirty`.
    pub(crate) fn view(
        &self,
        window_width: f32,
        window_height: f32,
    ) -> Element<'_, SettingsMessage> {
        let font = theme::ui_font();
        let is_narrow = window_width < NARROW_BREAKPOINT_WIDTH;

        // Detail-pane entries render straight from the update-maintained
        // cache. An empty cache with an active search is the legitimate
        // "No settings match" empty state. Sub-lists bypass this entirely
        // (they render their own slot list).
        let entries: &[SettingsEntry] = &self.cached_entries;

        let right_pane = if let Some(sub) = &self.sub_list {
            self.render_sub_list(sub, window_height, font)
        } else {
            self.render_detail_pane(entries, window_height)
        };

        // Wide: 340 px sidebar + detail in a horizontal row.
        // Narrow: horizontal category strip + search header above the
        //         detail pane, both stacked vertically.
        let base: Element<'_, SettingsMessage> = if is_narrow {
            let strip = self.render_narrow_strip(window_width);
            let search_bar = self.render_narrow_search_header();
            column![search_bar, strip, right_pane]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            let pane_separator = container(Space::new())
                .width(Length::Fixed(1.0))
                .height(Length::Fill)
                .style(|_: &iced::Theme| container::Style {
                    background: Some(theme::border().into()),
                    ..Default::default()
                });
            row![self.render_sidebar(), pane_separator, right_pane]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        };

        // Theme / font picker overlays on top of either layout. Only one
        // sub-list is ever open at a time (theme takes precedence).
        if let Some(tsw) = &self.theme_sub_list {
            let modal = self.render_theme_modal(tsw, window_height);
            stack![base, modal]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else if let Some(fsw) = &self.font_sub_list {
            let modal = self.render_font_modal(fsw, window_height);
            stack![base, modal]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            base
        }
    }

    // ========================================================================
    // Sidebar (left pane)
    // ========================================================================

    /// Render the 340 px categories sidebar: title + search input header,
    /// scrollable slot list of categories, version + Esc footer.
    fn render_sidebar(&self) -> Element<'_, SettingsMessage> {
        let header = self.render_sidebar_header();
        let body = self.render_sidebar_body();
        let footer = self.render_sidebar_footer();

        let content = column![header, body, footer]
            .width(Length::Fill)
            .height(Length::Fill);

        container(content)
            .width(Length::Fixed(SIDEBAR_WIDTH))
            .height(Length::Fill)
            .style(|_: &iced::Theme| container::Style {
                background: Some(theme::bg0_hard().into()),
                border: Border {
                    color: theme::border(),
                    width: 0.0,
                    radius: iced::border::Radius::default(),
                },
                ..Default::default()
            })
            .into()
    }

    /// Sidebar header: "Settings" title + relocated search input. Search
    /// always renders so its `text_input` id survives across renders.
    fn render_sidebar_header(&self) -> Element<'_, SettingsMessage> {
        let title = text("Settings").size(15.0).color(theme::fg0()).font(Font {
            weight: Weight::Bold,
            ..theme::ui_font()
        });

        let search_input = crate::widgets::search_bar::search_bar(
            &self.search_query,
            "Search settings…",
            SETTINGS_SEARCH_INPUT_ID,
            SettingsMessage::SearchChanged,
            Some(theme::settings_search_input_style),
        );

        let header_row = row![title, Space::new().width(Length::Fixed(12.0)), search_input]
            .align_y(Alignment::Center)
            .width(Length::Fill);

        let sep = container(Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(1.0))
            .style(|_: &iced::Theme| container::Style {
                background: Some(theme::border().into()),
                ..Default::default()
            });

        column![
            container(header_row)
                .width(Length::Fill)
                .height(Length::Fixed(SIDEBAR_HEADER_HEIGHT - 1.0))
                .align_y(Alignment::Center)
                .padding(Padding::new(0.0).left(18.0).right(18.0)),
            sep,
        ]
        .width(Length::Fill)
        .height(Length::Fixed(SIDEBAR_HEADER_HEIGHT))
        .into()
    }

    /// Sidebar body: a fixed `Column` of six compact category rows. Slot
    /// list infrastructure isn't useful here — six rows always fit in any
    /// realistic window, so we skip the dynamic-slot-count machinery and
    /// drive the active highlight straight off `sidebar_slot_list
    /// .viewport_offset`. Keyboard nav still routes through
    /// `SidebarUp`/`SidebarDown` (handled by the hotkey dispatcher,
    /// independent of the rendering path).
    fn render_sidebar_body(&self) -> Element<'_, SettingsMessage> {
        let active_index = self.sidebar_slot_list.viewport_offset;
        let mut col = column![].width(Length::Fill);
        for (idx, tab) in SettingsTab::ALL.iter().enumerate() {
            col = col.push(render_sidebar_row(*tab, idx, idx == active_index));
        }
        container(col)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Sidebar footer: version label on the left, decorative "Esc" pill on
    /// the right (matches the design's `.nk-sidebar-foot`).
    fn render_sidebar_footer(&self) -> Element<'_, SettingsMessage> {
        let sep = container(Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(1.0))
            .style(|_: &iced::Theme| container::Style {
                background: Some(theme::border().into()),
                ..Default::default()
            });

        let version = text(concat!("v", env!("CARGO_PKG_VERSION")))
            .size(10.0)
            .color(theme::fg3())
            .font(theme::ui_font());

        let esc_pill = button(
            row![
                embedded_svg::svg_widget("assets/icons/log-out.svg")
                    .width(Length::Fixed(11.0))
                    .height(Length::Fixed(11.0))
                    .style(move |_, _| svg::Style {
                        color: Some(theme::fg2()),
                    }),
                text("Esc").size(10.0).color(theme::fg0()).font(Font {
                    weight: Weight::Medium,
                    ..theme::ui_font()
                }),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .on_press(SettingsMessage::Escape)
        .style(|_theme: &iced::Theme, status: button::Status| {
            let is_hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: Some(
                    if is_hovered {
                        theme::bg1()
                    } else {
                        theme::bg0()
                    }
                    .into(),
                ),
                border: Border {
                    color: theme::border(),
                    width: 1.0,
                    radius: theme::ui_radius_pill(),
                },
                text_color: theme::fg0(),
                ..Default::default()
            }
        })
        .padding(Padding::new(4.0).left(10.0).right(10.0));

        let footer_row = row![version, Space::new().width(Length::Fill), esc_pill]
            .align_y(Alignment::Center)
            .width(Length::Fill);

        column![
            sep,
            container(footer_row)
                .width(Length::Fill)
                .height(Length::Fixed(SIDEBAR_FOOTER_HEIGHT - 1.0))
                .align_y(Alignment::Center)
                .padding(Padding::new(0.0).left(18.0).right(14.0)),
        ]
        .width(Length::Fill)
        .height(Length::Fixed(SIDEBAR_FOOTER_HEIGHT))
        .into()
    }

    // ========================================================================
    // Narrow variant (horizontal chip strip + search header)
    // ========================================================================

    /// Narrow-variant search header: just the relocated search input on
    /// `bg0_hard` with a 1 px bottom border. The "Settings" title is
    /// implicit (the Settings nav-tab is already highlighted in the chrome).
    fn render_narrow_search_header(&self) -> Element<'_, SettingsMessage> {
        let search_input = crate::widgets::search_bar::search_bar(
            &self.search_query,
            "Search settings…",
            SETTINGS_SEARCH_INPUT_ID,
            SettingsMessage::SearchChanged,
            Some(theme::settings_search_input_style),
        );

        let body = container(search_input)
            .width(Length::Fill)
            .padding(
                Padding::new(0.0)
                    .top(8.0)
                    .bottom(8.0)
                    .left(16.0)
                    .right(16.0),
            )
            .style(|_: &iced::Theme| container::Style {
                background: Some(theme::bg0_hard().into()),
                ..Default::default()
            });

        let sep = container(Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(1.0))
            .style(|_: &iced::Theme| container::Style {
                background: Some(theme::border().into()),
                ..Default::default()
            });

        column![body, sep].width(Length::Fill).into()
    }

    /// Narrow-variant category strip: horizontal row of pill chips, one
    /// per category, each taking an equal `FillPortion(1)` share of the
    /// strip width so the row spans without scrolling. Active chip gets
    /// the accent border + `accent_soft` fill + accent text; inactive
    /// chips use the `bg_border_2` outline. Label font scales via
    /// `tab_text_size` on the same curve as the top nav, so the strip
    /// stays readable as the window narrows.
    fn render_narrow_strip(&self, window_width: f32) -> Element<'_, SettingsMessage> {
        let active_index = self.sidebar_slot_list.viewport_offset;
        let text_size = crate::widgets::nav_bar::tab_text_size(window_width);
        let mut chip_row = row![Space::new().width(Length::Fixed(16.0))]
            .spacing(6)
            .width(Length::Fill)
            .align_y(Alignment::Center);
        for (idx, tab) in SettingsTab::ALL.iter().enumerate() {
            chip_row = chip_row.push(render_narrow_chip(
                *tab,
                idx,
                idx == active_index,
                text_size,
            ));
        }
        chip_row = chip_row.push(Space::new().width(Length::Fixed(16.0)));

        let body = container(chip_row)
            .width(Length::Fill)
            .height(Length::Fixed(NARROW_STRIP_HEIGHT - 1.0))
            .align_y(Alignment::Center)
            .style(|_: &iced::Theme| container::Style {
                background: Some(theme::bg0_hard().into()),
                ..Default::default()
            });

        let sep = container(Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(1.0))
            .style(|_: &iced::Theme| container::Style {
                background: Some(theme::border().into()),
                ..Default::default()
            });

        column![body, sep]
            .width(Length::Fill)
            .height(Length::Fixed(NARROW_STRIP_HEIGHT))
            .into()
    }

    // ========================================================================
    // Detail pane (right pane)
    // ========================================================================

    /// Render the detail pane — a `scrollable` column of variable-height
    /// rows for the active category's entries. Each row grows to fit its
    /// label + inline help text + control + "Default: X" label. Headers
    /// scroll with the content (v1 sticky fallback per the plan).
    ///
    /// The focused row is `slot_list.viewport_offset` (the existing slot
    /// state, repurposed as a row-index pointer now that the visual is
    /// no longer slot-list-paginated). Tab / Backspace continue to drive
    /// `SettingsMessage::SlotListUp`/`SlotListDown`; click on a row
    /// dispatches `SlotListClickItem(idx)`. Mouse-wheel scrolls the pane
    /// natively via `iced::scrollable`.
    fn render_detail_pane<'a>(
        &'a self,
        entries: &[SettingsEntry],
        _window_height: f32,
    ) -> Element<'a, SettingsMessage> {
        if entries.is_empty() {
            let empty_msg = if self.search_query.is_empty() {
                "No settings available"
            } else {
                "No settings match the search query"
            };
            return container(
                text(empty_msg)
                    .size(14)
                    .font(theme::ui_font())
                    .color(theme::fg4()),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center(Length::Fill)
            .style(|_: &iced::Theme| container::Style {
                background: Some(theme::bg0().into()),
                ..Default::default()
            })
            .into();
        }

        let focused_index = self.slot_list.viewport_offset;
        let editing_index = self.editing_index;
        let is_capturing = self.capturing_hotkey.is_some();
        let conflict_text_owned = self.conflict_label.as_ref().map(|(s, _)| s.clone());
        let hex_input_owned = self.hex_input.clone();
        let toggle_cursor = self.toggle_cursor;

        let sections = compute_section_index(entries);
        let mut section_cursor = 0;

        // Lowercase the active query once; per-row highlight ranges are derived
        // from it with the same fuzzy matcher the search ranking uses. Trimmed to
        // match the ranking path so a stray leading/trailing space neither breaks
        // matching nor highlights a literal space.
        let query_trimmed = self.search_query.trim();
        let query_lower = (!query_trimmed.is_empty()).then(|| query_trimmed.to_lowercase());

        let mut col = column![].width(Length::Fill);
        for (idx, entry) in entries.iter().enumerate() {
            let row_element: Element<'a, SettingsMessage> = match entry {
                SettingsEntry::Header { label, icon } => {
                    let count = sections.get(section_cursor).map_or(0, |s| s.count);
                    section_cursor += 1;
                    render_detail_header(label, icon, count)
                }
                SettingsEntry::Item(item) => {
                    let is_focused = idx == focused_index;
                    let is_editing = editing_index == Some(idx);
                    let conflict_text = if is_focused && is_capturing {
                        conflict_text_owned.as_deref()
                    } else {
                        None
                    };
                    let match_spans = query_lower.as_deref().and_then(|q| {
                        fuzzy::fuzzy_match(&item.label, q)
                            .filter(|m| fuzzy::is_strong(m, q))
                            .map(|m| m.ranges)
                    });
                    render_detail_row(
                        item,
                        idx,
                        is_focused,
                        is_editing,
                        is_capturing && is_focused,
                        if is_editing {
                            hex_input_owned.as_str()
                        } else {
                            ""
                        },
                        toggle_cursor,
                        conflict_text,
                        match_spans.as_deref(),
                    )
                }
            };
            col = col.push(row_element);
        }

        let scrollable_body = iced::widget::scrollable(col)
            .id(iced::widget::Id::new(super::DETAIL_SCROLLABLE_ID))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::settings_scrollable_style);

        // Stack the sticky mini-index above the scrollable. Single-section
        // tabs (Hotkeys) drop the strip — a one-pill index reads as noise.
        let pane_body: Element<'a, SettingsMessage> = if sections.len() > 1 {
            column![render_section_pill_strip(&sections), scrollable_body]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            scrollable_body.into()
        };

        container(pane_body)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_: &iced::Theme| container::Style {
                background: Some(theme::bg0().into()),
                ..Default::default()
            })
            .into()
    }

    /// Render the font picker as a modal overlay (dimmed backdrop + centered
    /// panel) so the user keeps visual context of the settings panel. All
    /// chrome is shared with the theme picker via [`render_picker_modal`]; only
    /// the slot-list body (each font name drawn in its own typeface) differs.
    fn render_font_modal<'a>(
        &'a self,
        fsw: &'a super::FontSubListState,
        window_height: f32,
    ) -> Element<'a, SettingsMessage> {
        use super::rendering::{SlotRenderContext, render_font_slot};

        render_picker_modal(
            "Font Family",
            "Type to filter fonts...",
            super::FONT_SEARCH_INPUT_ID,
            &fsw.search_query,
            window_height,
            |modal_height, modal_chrome| {
                if fsw.filtered_fonts.is_empty() {
                    return picker_empty_state("No fonts match the search query");
                }
                let config =
                    slot_list::SlotListConfig::with_dynamic_slots(modal_height, modal_chrome);
                slot_list::slot_list_view_with_scroll(
                    &fsw.slot_list,
                    &fsw.filtered_fonts,
                    &config,
                    SettingsMessage::SlotListUp,
                    SettingsMessage::SlotListDown,
                    super::settings_seek_to(fsw.filtered_fonts.len()),
                    None,
                    |font_name, ctx| {
                        let ctx = SlotRenderContext {
                            item_index: ctx.item_index,
                            is_center: ctx.is_center,
                            opacity: 1.0,
                            row_height: ctx.row_height,
                            scale_factor: ctx.scale_factor,
                            is_capturing: false,
                            conflict_text: None,
                            toggle_cursor: None,
                        };
                        render_font_slot(&ctx, font_name)
                    },
                )
            },
        )
    }

    /// Theme picker modal — a sibling of [`Self::render_font_modal`] sharing the
    /// same chrome via [`render_picker_modal`]. The difference is the row
    /// renderer: each row is painted in its OWN theme's palette (see
    /// [`render_theme_slot`]) and the hover wash is disabled so those colors
    /// stay true, so scrolling the list IS a live preview.
    fn render_theme_modal<'a>(
        &'a self,
        tsw: &'a super::ThemeSubListState,
        window_height: f32,
    ) -> Element<'a, SettingsMessage> {
        use super::rendering::{SlotRenderContext, render_theme_slot};

        render_picker_modal(
            "Themes",
            "Type to filter themes...",
            super::THEME_SEARCH_INPUT_ID,
            &tsw.search_query,
            window_height,
            |modal_height, modal_chrome| {
                if tsw.filtered_rows.is_empty() {
                    // Distinguish "search matched nothing" from "discovery
                    // returned zero themes" (only when the themes dir fails).
                    let msg = if tsw.search_query.is_empty() {
                        "No themes found"
                    } else {
                        "No themes match the search query"
                    };
                    return picker_empty_state(msg);
                }
                // No hover wash: the active-theme accent wash would muddy rows
                // painted in their own palette (selection shows via the per-row
                // accent ring in `render_theme_slot`).
                let config =
                    slot_list::SlotListConfig::with_dynamic_slots(modal_height, modal_chrome)
                        .without_hover_wash();
                slot_list::slot_list_view_with_scroll(
                    &tsw.slot_list,
                    &tsw.filtered_rows,
                    &config,
                    SettingsMessage::SlotListUp,
                    SettingsMessage::SlotListDown,
                    super::settings_seek_to(tsw.filtered_rows.len()),
                    None,
                    |row, ctx| {
                        let ctx = SlotRenderContext {
                            item_index: ctx.item_index,
                            is_center: ctx.is_center,
                            opacity: 1.0,
                            row_height: ctx.row_height,
                            scale_factor: ctx.scale_factor,
                            is_capturing: false,
                            conflict_text: None,
                            toggle_cursor: None,
                        };
                        render_theme_slot(&ctx, row)
                    },
                )
            },
        )
    }
}

/// Centered dim-text body for a picker modal's empty state.
fn picker_empty_state<'a>(msg: &'static str) -> Element<'a, SettingsMessage> {
    container(text(msg).size(14).color(theme::fg4()))
        .width(Length::Fill)
        .height(Length::Fill)
        .center(Length::Fill)
        .into()
}

/// Shared chrome for the settings picker modals (font + theme): a dimmed
/// backdrop (Escape on press, wheel → slot Up/Down) behind a centered panel
/// with a title bar (X back-button), a search bar, and the caller's body.
///
/// The two pickers differ only in their title/placeholder/search-input-id and
/// their slot-list body, so this keeps the scaffolding in one place. `build_body`
/// receives the computed `(modal_height, modal_chrome)` so its slot-list config
/// matches the panel height; it borrows the picker state directly (no per-frame
/// clone of the row list or the search query).
fn render_picker_modal<'a>(
    title: &'static str,
    placeholder: &'static str,
    search_input_id: &'static str,
    search_query: &'a str,
    window_height: f32,
    build_body: impl FnOnce(f32, f32) -> Element<'a, SettingsMessage>,
) -> Element<'a, SettingsMessage> {
    // The modal floats over the settings panel at 70% of window height, minus
    // chrome (breadcrumb-height title bar + search bar).
    let modal_height = window_height * 0.70;
    let modal_chrome = BREADCRUMB_HEIGHT + FONT_SEARCH_BAR_HEIGHT;
    let main_area = build_body(modal_height, modal_chrome);

    // ── Title bar (X back-button on the right) ──
    let title_bar = {
        let dim_color = theme::fg4();
        let label_size = 13.0;
        let back_btn = button(
            embedded_svg::svg_widget("assets/icons/x.svg")
                .width(Length::Fixed(label_size))
                .height(Length::Fixed(label_size))
                .style(move |_theme, _status| svg::Style {
                    color: Some(dim_color),
                }),
        )
        .on_press(SettingsMessage::Escape)
        .style(transparent_button_style)
        .padding(Padding::new(2.0));

        let content = row![
            Space::new().width(Length::Fixed(12.0)),
            text(title)
                .size(label_size)
                .font(Font {
                    weight: Weight::Bold,
                    ..theme::ui_font()
                })
                .color(theme::fg0()),
            Space::new().width(Length::Fill),
            back_btn,
            Space::new().width(Length::Fixed(12.0)),
        ]
        .align_y(Alignment::Center)
        .height(Length::Fixed(BREADCRUMB_HEIGHT));

        container(content).width(Length::Fill)
    };

    // ── Search bar ──
    let search_bar = {
        let input = crate::widgets::search_bar::search_bar(
            search_query,
            placeholder,
            search_input_id,
            SettingsMessage::SubListSearchChanged,
            Some(crate::theme::settings_search_input_style),
        );
        container(input)
            .width(Length::Fill)
            .height(Length::Fixed(FONT_SEARCH_BAR_HEIGHT))
            .padding(Padding::new(4.0).left(12.0).right(12.0))
    };

    // ── Panel (bg0_hard fill + accent border; matches the sibling picker, not
    //    the shared `modal_frame_style` used by the other dialogs) ──
    let modal_bg = theme::bg0_hard();
    let modal_border = theme::accent();
    let modal_radius = theme::ui_border_radius();
    let modal_panel = container(
        column![title_bar, search_bar, main_area]
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .width(Length::FillPortion(5))
    .height(Length::Fixed(modal_height))
    .clip(true)
    .padding(Padding::new(4.0))
    .style(move |_: &iced::Theme| container::Style {
        background: Some(modal_bg.into()),
        border: Border {
            color: modal_border,
            width: 1.5,
            radius: modal_radius,
        },
        ..Default::default()
    });

    // ── Centered panel + dimmed backdrop (press → Escape, wheel → Up/Down) ──
    let modal_row = row![
        Space::new().width(Length::FillPortion(1)),
        modal_panel,
        Space::new().width(Length::FillPortion(1)),
    ]
    .width(Length::Fill)
    .align_y(Alignment::Center);

    let backdrop_color = Color {
        a: 0.55,
        ..Color::BLACK
    };

    mouse_area(
        container(modal_row)
            .width(Length::Fill)
            .height(Length::Fill)
            .center(Length::Fill)
            .style(move |_: &iced::Theme| container::Style {
                background: Some(backdrop_color.into()),
                ..Default::default()
            }),
    )
    .on_press(SettingsMessage::Escape)
    .on_scroll(|delta| {
        let y = match delta {
            iced::mouse::ScrollDelta::Lines { y, .. } => y,
            iced::mouse::ScrollDelta::Pixels { y, .. } => y,
        };
        if y > 0.0 {
            SettingsMessage::SlotListUp
        } else {
            SettingsMessage::SlotListDown
        }
    })
    .into()
}

/// Narrow-variant category chip: pill-shaped, icon + name only (no
/// blurb), `accent` border + `accent_soft` fill on the active chip,
/// `bg2` outline + `bg0` fill on inactive. Each chip claims a
/// `FillPortion(1)` slice of the strip so all six chips span the
/// available width together; the label scales via `text_size` from the
/// shared `tab_text_size` curve so labels stay inside their share as
/// the window narrows. Click emits `SidebarClickItem(idx)` — same
/// handler as the wide-sidebar row.
fn render_narrow_chip<'a>(
    tab: SettingsTab,
    idx: usize,
    is_active: bool,
    text_size: f32,
) -> Element<'a, SettingsMessage> {
    let border_color = if is_active {
        theme::accent_bright()
    } else {
        theme::bg2()
    };
    let bg_color = if is_active {
        // Mirrors `--nk-acc-soft` (rgba(184, 212, 154, 0.16)) — a soft
        // accent fill at ~16% alpha; nokkvi doesn't expose this exact
        // token so we build it inline.
        Color {
            a: 0.16,
            ..theme::accent_bright()
        }
    } else {
        theme::bg0()
    };
    let text_color = if is_active {
        theme::accent_bright()
    } else {
        theme::fg0()
    };

    let icon = embedded_svg::svg_widget(tab.icon_path())
        .width(Length::Fixed(13.0))
        .height(Length::Fixed(13.0))
        .style(move |_, _| svg::Style {
            color: Some(text_color),
        });
    let label = text(tab.label())
        .size(text_size)
        .color(text_color)
        .font(Font {
            weight: if is_active {
                Weight::Bold
            } else {
                Weight::Medium
            },
            ..theme::ui_font()
        })
        .wrapping(iced::widget::text::Wrapping::None);

    // Inner container claims the chip's full content area and centers
    // the icon+label row inside — without it the row anchors at the
    // padded left edge of the `FillPortion(1)` button, leaving the
    // right side of each chip blank.
    let content = container(row![icon, label].spacing(8).align_y(Alignment::Center))
        .width(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center);

    button(content)
        .on_press(SettingsMessage::SidebarClickItem(idx))
        .width(Length::FillPortion(1))
        .padding(
            Padding::new(0.0)
                .top(8.0)
                .bottom(8.0)
                .left(12.0)
                .right(12.0),
        )
        .style(move |_theme: &iced::Theme, status: button::Status| {
            let hovered = matches!(status, button::Status::Hovered);
            let bg = if is_active {
                bg_color
            } else if hovered {
                theme::bg1()
            } else {
                bg_color
            };
            button::Style {
                background: Some(bg.into()),
                border: Border {
                    color: border_color,
                    width: 1.0,
                    radius: theme::ui_radius_pill(),
                },
                text_color,
                ..Default::default()
            }
        })
        .into()
}

/// Single sidebar row at the design's compact proportions: 38 px icon
/// chip + 14 px name / 10 px blurb, 14 px / 16 px / 18 px padding,
/// 1 px hairline bottom separator, 2 px accent left stripe + `bg1` fill
/// on the active row. Click emits `SidebarClickItem(idx)`.
fn render_sidebar_row<'a>(
    tab: SettingsTab,
    idx: usize,
    is_active: bool,
) -> Element<'a, SettingsMessage> {
    let chip_bg = if is_active {
        theme::bg1()
    } else {
        theme::bg0()
    };
    let chip_icon_color = if is_active {
        theme::accent_bright()
    } else {
        theme::fg2()
    };
    let name_color = if is_active {
        theme::accent_bright()
    } else {
        theme::fg0()
    };

    let icon_chip = container(
        embedded_svg::svg_widget(tab.icon_path())
            .width(Length::Fixed(18.0))
            .height(Length::Fixed(18.0))
            .style(move |_, _| svg::Style {
                color: Some(chip_icon_color),
            }),
    )
    .width(Length::Fixed(38.0))
    .height(Length::Fixed(38.0))
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
    .style(move |_: &iced::Theme| container::Style {
        background: Some(chip_bg.into()),
        border: Border {
            color: theme::border(),
            width: 1.0,
            radius: theme::ui_radius_sm(),
        },
        ..Default::default()
    });

    let name = text(tab.label()).size(14.0).color(name_color).font(Font {
        weight: if is_active {
            Weight::Bold
        } else {
            Weight::Medium
        },
        ..theme::ui_font()
    });
    let blurb = text(tab.description())
        .size(10.0)
        .color(theme::fg3())
        .font(theme::ui_font());

    let text_col = column![name, blurb].spacing(2).width(Length::Fill);

    let row_content = row![icon_chip, Space::new().width(Length::Fixed(12.0)), text_col]
        .spacing(0)
        .align_y(Alignment::Center)
        .width(Length::Fill);

    // 2 px accent left stripe when active; transparent otherwise so the
    // row body keeps its horizontal position across the active toggle.
    let stripe_color = if is_active {
        theme::accent_bright()
    } else {
        Color::TRANSPARENT
    };
    let stripe = container(Space::new())
        .width(Length::Fixed(2.0))
        .height(Length::Fill)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(stripe_color.into()),
            ..Default::default()
        });

    let stripe_row = row![
        stripe,
        container(row_content).width(Length::Fill).padding(
            Padding::new(0.0)
                .top(14.0)
                .bottom(14.0)
                .left(16.0)
                .right(18.0)
        ),
    ]
    .align_y(Alignment::Center);

    let row_bg = if is_active {
        theme::bg1()
    } else {
        Color::TRANSPARENT
    };
    let body = container(stripe_row)
        .width(Length::Fill)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(row_bg.into()),
            ..Default::default()
        });

    let row_with_sep = column![
        body,
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(1.0))
            .style(|_: &iced::Theme| container::Style {
                background: Some(theme::border().into()),
                ..Default::default()
            }),
    ]
    .width(Length::Fill);

    button(row_with_sep)
        .style(transparent_button_style)
        .padding(0)
        .width(Length::Fill)
        .on_press(SettingsMessage::SidebarClickItem(idx))
        .into()
}

// ============================================================================
// Mini-index pill strip (sticky above detail-pane scrollable)
// ============================================================================

/// Height (px) of the sticky mini-index strip above the detail pane.
/// Matches the narrow-strip footprint so the two share visual rhythm.
const MINI_INDEX_HEIGHT: f32 = 44.0;

/// A single sub-section descriptor used by the mini-index pill strip and
/// by the in-flow header count.
struct SectionInfo {
    label: &'static str,
    icon: &'static str,
    count: usize,
    header_idx: usize,
}

/// Walk the flat entry list once, emit one `SectionInfo` per `Header`
/// entry with the item count of the slice between this header and the
/// next (or end). Iteration order matches the render order, so the
/// caller can advance a parallel cursor through it during the row loop.
fn compute_section_index(entries: &[SettingsEntry]) -> Vec<SectionInfo> {
    let mut sections = Vec::new();
    let mut i = 0;
    while i < entries.len() {
        if let SettingsEntry::Header { label, icon } = entries[i] {
            let mut count = 0;
            let mut j = i + 1;
            while j < entries.len() && !matches!(entries[j], SettingsEntry::Header { .. }) {
                count += 1;
                j += 1;
            }
            sections.push(SectionInfo {
                label,
                icon,
                count,
                header_idx: i,
            });
            i = j;
        } else {
            i += 1;
        }
    }
    sections
}

/// Render the horizontal pill mini-index that sits above the detail
/// scrollable. Each pill is a clickable label that emits
/// `JumpToSection(header_idx)`; the handler scrolls the pane so the
/// matching header lands at the top. Pills are wrapped in a horizontal
/// scrollable so tabs with many sections (Theme has 12) stay reachable
/// at narrower widths.
fn render_section_pill_strip<'a>(sections: &[SectionInfo]) -> Element<'a, SettingsMessage> {
    let mut chip_row = row![Space::new().width(Length::Fixed(16.0))]
        .spacing(6)
        .align_y(Alignment::Center);
    for section in sections {
        chip_row = chip_row.push(render_section_pill(
            section.label,
            section.icon,
            section.header_idx,
        ));
    }
    chip_row = chip_row.push(Space::new().width(Length::Fixed(16.0)));

    let scrollable_row = iced::widget::scrollable(chip_row)
        .direction(iced::widget::scrollable::Direction::Horizontal(
            iced::widget::scrollable::Scrollbar::new()
                .width(0)
                .scroller_width(0),
        ))
        .width(Length::Fill);

    let body = container(scrollable_row)
        .width(Length::Fill)
        .height(Length::Fixed(MINI_INDEX_HEIGHT - 1.0))
        .align_y(Alignment::Center)
        .style(|_: &iced::Theme| container::Style {
            background: Some(theme::bg0_hard().into()),
            ..Default::default()
        });

    let sep = container(Space::new())
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(|_: &iced::Theme| container::Style {
            background: Some(theme::border().into()),
            ..Default::default()
        });

    column![body, sep]
        .width(Length::Fill)
        .height(Length::Fixed(MINI_INDEX_HEIGHT))
        .into()
}

/// A single mini-index pill: icon + uppercase label, click dispatches
/// `JumpToSection(header_idx)`.
fn render_section_pill<'a>(
    label: &'static str,
    icon_path: &'static str,
    header_idx: usize,
) -> Element<'a, SettingsMessage> {
    let icon = embedded_svg::svg_widget(icon_path)
        .width(Length::Fixed(12.0))
        .height(Length::Fixed(12.0))
        .style(|_, _| svg::Style {
            color: Some(theme::fg2()),
        });
    let label_widget = text(label.to_uppercase())
        .size(11.0)
        .color(theme::fg1())
        .font(Font {
            weight: Weight::Bold,
            ..theme::ui_font()
        })
        .wrapping(iced::widget::text::Wrapping::None);

    let content = row![icon, label_widget]
        .spacing(6)
        .align_y(Alignment::Center);

    button(content)
        .on_press(SettingsMessage::JumpToSection(header_idx))
        .padding(
            Padding::new(0.0)
                .top(6.0)
                .bottom(6.0)
                .left(12.0)
                .right(12.0),
        )
        .style(|_theme: &iced::Theme, status: button::Status| {
            let hovered = matches!(status, button::Status::Hovered);
            let bg = if hovered { theme::bg1() } else { theme::bg0() };
            button::Style {
                background: Some(bg.into()),
                border: Border {
                    color: theme::bg2(),
                    width: 1.0,
                    radius: theme::ui_radius_pill(),
                },
                text_color: theme::fg1(),
                ..Default::default()
            }
        })
        .into()
}
