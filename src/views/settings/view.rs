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

use super::{
    BREADCRUMB_HEIGHT, FONT_SEARCH_BAR_HEIGHT, SETTINGS_SEARCH_INPUT_ID, SettingsMessage,
    SettingsPage, SettingsTab, SettingsViewData,
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

impl SettingsPage {
    /// Render the settings view — persistent two-pane layout.
    ///
    /// Left: 340 px categories sidebar (always visible).
    /// Right: detail pane for the active category, OR an in-place sub-list
    ///        (color array editor) when one is open.
    /// Font picker still overlays as a modal stack on top of everything.
    pub(crate) fn view(&self, data: SettingsViewData) -> Element<'_, SettingsMessage> {
        let font = theme::ui_font();
        let window_height = data.window_height;

        // Detail-pane entries: either the optimistically-edited cache or a
        // fresh rebuild from live config so hot-reloads land. Sub-lists
        // bypass this entirely (they render their own slot list).
        let built_entries;
        let entries: &[SettingsEntry] = if (self.editing_index.is_some()
            || self.sub_list.is_some()
            || self.font_sub_list.is_some()
            || !self.search_query.is_empty())
            && !self.cached_entries.is_empty()
        {
            &self.cached_entries
        } else {
            built_entries = if self.search_query.is_empty() {
                Self::build_category_sections(self.active_category, &data)
            } else {
                Self::search_all_entries(&data, &self.search_query)
            };
            &built_entries
        };

        let right_pane = if let Some(sub) = &self.sub_list {
            self.render_sub_list(sub, window_height, font)
        } else {
            self.render_detail_pane(entries, window_height)
        };

        let base_row: Element<'_, SettingsMessage> = row![self.render_sidebar(), right_pane]
            .width(Length::Fill)
            .height(Length::Fill)
            .into();

        // Font picker overlays on top of the two-pane layout.
        if let Some(fsw) = &self.font_sub_list {
            let modal = self.render_font_modal(fsw, window_height, font);
            stack![base_row, modal]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            base_row
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

        let mut col = column![].width(Length::Fill);
        for (idx, entry) in entries.iter().enumerate() {
            let row_element: Element<'a, SettingsMessage> = match entry {
                SettingsEntry::Header { label, icon } => render_detail_header(label, icon),
                SettingsEntry::Item(item) => {
                    let is_focused = idx == focused_index;
                    let is_editing = editing_index == Some(idx);
                    let conflict_text = if is_focused && is_capturing {
                        conflict_text_owned.as_deref()
                    } else {
                        None
                    };
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
                    )
                }
            };
            col = col.push(row_element);
        }

        let scrollable_body = iced::widget::scrollable(col)
            .width(Length::Fill)
            .height(Length::Fill);

        container(scrollable_body)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_: &iced::Theme| container::Style {
                background: Some(theme::bg0().into()),
                ..Default::default()
            })
            .into()
    }

    /// Render the font picker as a modal overlay — dimmed backdrop + centered panel.
    ///
    /// Replaces the old full-view-replacement approach so the user keeps
    /// visual context of the underlying settings panel.
    fn render_font_modal<'a>(
        &'a self,
        fsw: &'a super::FontSubListState,
        window_height: f32,
        _font: Font,
    ) -> Element<'a, SettingsMessage> {
        use super::{
            FONT_SEARCH_INPUT_ID,
            rendering::{SlotRenderContext, render_font_slot},
        };

        let search_query = fsw.search_query.clone();

        // ── Modal dimensions ──
        // The modal floats on top of the settings panel.
        // Use 70% of window height for the modal, minus chrome (breadcrumb + search bar).
        let modal_height = window_height * 0.70;
        let modal_chrome = BREADCRUMB_HEIGHT + FONT_SEARCH_BAR_HEIGHT;

        // ── Title bar (replaces breadcrumb) ──
        let title_bar = {
            let dim_color = theme::fg4();
            let active_color = theme::fg0();
            let label_size = 13.0;

            let back_icon_size = label_size;
            let back_btn = button(
                embedded_svg::svg_widget("assets/icons/x.svg")
                    .width(Length::Fixed(back_icon_size))
                    .height(Length::Fixed(back_icon_size))
                    .style(move |_theme, _status| svg::Style {
                        color: Some(dim_color),
                    }),
            )
            .on_press(SettingsMessage::Escape)
            .style(transparent_button_style)
            .padding(Padding::new(2.0));

            let content = row![
                Space::new().width(Length::Fixed(12.0)),
                text("Font Family")
                    .size(label_size)
                    .font(Font {
                        weight: Weight::Bold,
                        ..theme::ui_font()
                    })
                    .color(active_color),
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
                &search_query,
                "Type to filter fonts...",
                FONT_SEARCH_INPUT_ID,
                SettingsMessage::FontSearchChanged,
                Some(crate::theme::settings_search_input_style),
            );
            container(input)
                .width(Length::Fill)
                .height(Length::Fixed(FONT_SEARCH_BAR_HEIGHT))
                .padding(Padding::new(4.0).left(12.0).right(12.0))
        };

        // ── Font slot list or empty state ──
        let main_area: Element<'a, SettingsMessage> = if fsw.filtered_fonts.is_empty() {
            container(
                text("No fonts match the search query")
                    .size(14)
                    .color(theme::fg4()),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center(Length::Fill)
            .into()
        } else {
            let config = slot_list::SlotListConfig::with_dynamic_slots(modal_height, modal_chrome);
            let fonts_owned = fsw.filtered_fonts.clone();

            slot_list::slot_list_view_with_scroll(
                &fsw.slot_list,
                &fonts_owned,
                &config,
                SettingsMessage::SlotListUp,
                SettingsMessage::SlotListDown,
                super::settings_seek_to(fonts_owned.len()),
                None,
                move |font_name, ctx| {
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
        };

        // ── Modal panel ──
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

        // ── Centered modal with spacers ──
        let modal_row = row![
            Space::new().width(Length::FillPortion(1)),
            modal_panel,
            Space::new().width(Length::FillPortion(1)),
        ]
        .width(Length::Fill)
        .align_y(Alignment::Center);

        // ── Semi-transparent backdrop (click sends Escape to dismiss) ──
        let backdrop_color = Color {
            a: 0.55,
            ..Color::BLACK
        };

        let backdrop = mouse_area(
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
        });

        backdrop.into()
    }
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
