//! Settings view rendering — panel with drill-down navigation.
//!
//! All methods are pure `&self` view functions producing `Element<SettingsMessage>`.

use iced::{
    Alignment, Border, Color, Element, Length, Padding,
    font::{Font, Weight},
    widget::{Space, button, column, container, mouse_area, row, stack, svg, text},
};

use super::{
    BREADCRUMB_HEIGHT, FONT_SEARCH_BAR_HEIGHT, NavLevel, SETTINGS_CHROME_HEIGHT,
    SETTINGS_SEARCH_INPUT_ID, SettingsMessage, SettingsPage, SettingsViewData,
    items::SettingsEntry,
    rendering::{SlotRenderContext, render_settings_slot, transparent_button_style},
};
use crate::{embedded_svg, theme, widgets::slot_list};

/// Height of the description area at the bottom of the panel
const DESCRIPTION_HEIGHT: f32 = 72.0;

/// SM-style panel container: bg0_soft interior + bg1 border.
/// Replaces `slot_list_background_container` for the settings-only panel look.
fn settings_panel_container(content: Element<'_, SettingsMessage>) -> Element<'_, SettingsMessage> {
    let bg = theme::bg0_soft();
    let border_color = theme::bg1();
    let radius = theme::ui_border_radius();
    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(bg.into()),
            border: Border {
                color: border_color,
                width: 1.0,
                radius,
            },
            ..Default::default()
        })
        .into()
}

impl SettingsPage {
    /// Render the settings view — centered panel layout
    pub(crate) fn view(&self, data: SettingsViewData) -> Element<'_, SettingsMessage> {
        let font = theme::ui_font();
        let window_height = data.window_height;

        // When editing, use cached entries (modified optimistically in update());
        // otherwise rebuild from live config so hot-reload changes are reflected.
        let built_entries;
        let entries: &[SettingsEntry] = if (self.editing_index.is_some()
            || self.sub_list.is_some()
            || self.font_sub_list.is_some()
            || !self.search_query.is_empty())
            && !self.cached_entries.is_empty()
        {
            &self.cached_entries
        } else {
            built_entries = match self.current_level() {
                NavLevel::CategoryPicker => Self::build_category_picker_entries(),
                NavLevel::Category(tab) => Self::build_category_sections(*tab, &data),
            };
            &built_entries
        };

        // Base content layer — color sub-list or main settings slot list
        // (font sub-list is now a modal overlay, not a replacement)
        let base_content = if let Some(sub) = &self.sub_list {
            self.render_sub_list(sub, window_height, font)
        } else {
            self.render_slot_list(entries, &data, window_height, font)
        };

        // Panel at ~75% width, centered with spacers
        let panel = container(base_content)
            .width(Length::Fill)
            .height(Length::Fill)
            .clip(true)
            .style(theme::container_bg0_hard)
            .padding(Padding::new(0.0).top(10.0).bottom(10.0));

        // 75% center panel via FillPortion: 1 | 6 | 1 = 75% center
        let left_spacer = container(Space::new())
            .width(Length::FillPortion(1))
            .height(Length::Fill)
            .style(theme::container_bg0_hard);
        let right_spacer = container(Space::new())
            .width(Length::FillPortion(1))
            .height(Length::Fill)
            .style(theme::container_bg0_hard);

        let base_row: Element<'_, SettingsMessage> = row![
            left_spacer,
            panel.width(Length::FillPortion(6)),
            right_spacer
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into();

        // If font sub-list is active, overlay it as a modal
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

    /// Render the main settings slot list with items
    fn render_slot_list<'a>(
        &'a self,
        entries: &[SettingsEntry],
        _data: &SettingsViewData,
        window_height: f32,
        _font: Font,
    ) -> Element<'a, SettingsMessage> {
        let has_search = !self.search_query.is_empty();

        if entries.is_empty() {
            let empty_msg = if has_search {
                "No settings match the search query"
            } else {
                "No settings available"
            };
            let top_bar = self.breadcrumb_header();
            let top_section = column![top_bar].width(Length::Fill);
            let empty_content = container(
                text(empty_msg)
                    .size(14)
                    .font(theme::ui_font())
                    .color(theme::fg4()),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center(Length::Fill);
            let content = column![top_section, empty_content]
                .width(Length::Fill)
                .height(Length::Fill);
            return settings_panel_container(content.into());
        }

        let mut config = slot_list::SlotListConfig::with_dynamic_slots(
            window_height,
            SETTINGS_CHROME_HEIGHT + BREADCRUMB_HEIGHT + DESCRIPTION_HEIGHT,
        );
        config.cull_empty = true;

        let entries_owned: Vec<SettingsEntry> = entries.to_vec();
        let is_level1 = matches!(self.current_level(), NavLevel::CategoryPicker);
        let editing_index = self.editing_index;
        let is_capturing = self.capturing_hotkey.is_some();
        let conflict_text_owned = self.conflict_label.as_ref().map(|(s, _)| s.clone());
        let hex_input_owned = self.hex_input.clone();
        let toggle_cursor = self.toggle_cursor;

        let slot_list_content = slot_list::slot_list_view_with_scroll(
            &self.slot_list,
            &entries_owned,
            &config,
            SettingsMessage::SlotListUp,
            SettingsMessage::SlotListDown,
            super::settings_seek_to(entries_owned.len()),
            move |entry, ctx| {
                let is_editing = editing_index == Some(ctx.item_index);
                // All headers are always expanded (no drill-down at Level 2)
                let is_collapsed = false;
                let ctx = SlotRenderContext {
                    item_index: ctx.item_index,
                    is_center: ctx.is_center,
                    opacity: 1.0,
                    row_height: ctx.row_height,
                    scale_factor: ctx.scale_factor,
                    is_capturing: is_capturing && ctx.is_center,
                    conflict_text: if is_capturing && ctx.is_center {
                        conflict_text_owned.as_deref()
                    } else {
                        None
                    },
                    is_level1,
                    toggle_cursor: if ctx.is_center { toggle_cursor } else { None },
                };
                let hi = if is_editing { &hex_input_owned } else { "" };
                render_settings_slot(&ctx, entry, is_editing, is_collapsed, hi)
            },
        );

        // Top bar: breadcrumb with inline search when active
        let breadcrumb = self.breadcrumb_header();
        let top_section = column![breadcrumb].width(Length::Fill);

        // Description area at bottom
        let description = self.description_area();

        let content = column![top_section, slot_list_content, description]
            .width(Length::Fill)
            .height(Length::Fill);

        settings_panel_container(content.into())
    }

    /// Render the breadcrumb bar based on the current nav stack.
    ///
    /// Level 1: "Settings"
    /// Level 2: "‹ General"
    /// Sub-list: "‹ General  ›  Sub-item"
    pub(super) fn breadcrumb_header(&self) -> Element<'_, SettingsMessage> {
        let font = theme::ui_font();
        let label_size = 13.0;
        let separator_size = 12.0;

        let dim_color = theme::fg4();
        let active_color = theme::fg0();

        // Build segments from nav stack
        let mut segments: Vec<&str> = Vec::new();
        let can_go_back = self.nav_stack.len() > 1 || self.sub_list.is_some();

        match self.current_level() {
            NavLevel::CategoryPicker => {
                segments.push("Settings");
            }
            NavLevel::Category(tab) => {
                segments.push("Settings");
                segments.push(tab.label());
            }
        }

        // Append sub-list label if in sub-list mode
        if let Some(sub) = &self.sub_list {
            segments.push(&sub.label);
        }

        let mut content = row![Space::new().width(Length::Fixed(12.0))];

        // Back arrow if we can navigate back
        if can_go_back {
            content = content.push(text("‹  ").size(separator_size + 2.0).color(dim_color));
        }

        let last_idx = segments.len().saturating_sub(1);
        for (i, segment) in segments.iter().enumerate() {
            let is_last = i == last_idx;

            if i > 0 {
                content = content.push(text("  ›  ").size(separator_size).color(dim_color));
            }

            if is_last {
                content = content.push(
                    text(*segment)
                        .size(label_size)
                        .font(Font {
                            weight: Weight::Bold,
                            ..font
                        })
                        .color(active_color),
                );
            } else {
                content = content.push(text(*segment).size(label_size).font(font).color(dim_color));
            }
        }

        // Always-visible inline search
        content = content.push(Space::new().width(Length::Fixed(16.0)));
        let search_bar = crate::widgets::search_bar::search_bar(
            &self.search_query,
            "Search settings…",
            SETTINGS_SEARCH_INPUT_ID,
            SettingsMessage::SearchChanged,
            Some(crate::theme::settings_search_input_style),
        );
        content = content.push(search_bar);

        content = content.push(Space::new().width(Length::Fixed(12.0)));

        let content = content
            .align_y(Alignment::Center)
            .height(Length::Fixed(38.0));

        // Clickable breadcrumb when we can go back
        if can_go_back {
            button(content)
                .on_press(SettingsMessage::Escape)
                .style(transparent_button_style)
                .padding(0)
                .width(Length::Fill)
                .into()
        } else {
            container(content).width(Length::Fill).into()
        }
    }

    /// Description area at the bottom of the panel — shows the focused item's description
    /// with an SM-style exit button on the right side.
    fn description_area(&self) -> Element<'_, SettingsMessage> {
        let desc = if self.description_text.is_empty() {
            " " // Maintain height even when empty
        } else {
            &self.description_text
        };

        let sep = container(Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(1.0))
            .style(|_: &iced::Theme| container::Style {
                background: Some(
                    iced::Color {
                        a: 0.25,
                        ..theme::accent()
                    }
                    .into(),
                ),
                ..Default::default()
            });

        let desc_text = text(desc)
            .size(11.0)
            .color(theme::accent_bright())
            .font(Font {
                weight: Weight::Medium,
                ..theme::ui_font()
            });

        // SM-style exit button — always visible, sends Escape
        let exit_icon_size = 12.0;
        let exit_label_size = 10.0;
        let exit_btn = button(
            row![
                embedded_svg::svg_widget("assets/icons/log-out.svg")
                    .width(Length::Fixed(exit_icon_size))
                    .height(Length::Fixed(exit_icon_size))
                    .style(move |_theme, _status| svg::Style {
                        color: Some(theme::fg3()),
                    }),
                text("Exit (Esc)")
                    .size(exit_label_size)
                    .color(theme::fg3())
                    .font(Font {
                        weight: Weight::Medium,
                        ..theme::ui_font()
                    }),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        )
        .on_press(SettingsMessage::Escape)
        .style(|_theme: &iced::Theme, status: button::Status| {
            let is_hovered = matches!(status, button::Status::Hovered);
            let bg_alpha = if is_hovered { 0.35 } else { 0.20 };
            let border_alpha = if is_hovered { 0.6 } else { 0.35 };
            button::Style {
                background: Some(
                    Color {
                        a: bg_alpha,
                        ..theme::accent()
                    }
                    .into(),
                ),
                border: Border {
                    color: Color {
                        a: border_alpha,
                        ..theme::accent()
                    },
                    width: 1.0,
                    radius: theme::ui_border_radius(),
                },
                text_color: if is_hovered {
                    theme::fg2()
                } else {
                    theme::fg3()
                },
                ..Default::default()
            }
        })
        .padding(Padding::new(4.0).left(8.0).right(8.0));

        let desc_row = row![
            container(desc_text)
                .width(Length::Fill)
                .height(Length::Fill)
                .clip(true)
                .align_y(Alignment::Center)
                .padding(Padding::new(0.0).left(12.0)),
            exit_btn,
            Space::new().width(Length::Fixed(8.0)),
        ]
        .align_y(Alignment::Center)
        .height(Length::Fill);

        let desc_container = container(desc_row)
            .width(Length::Fill)
            .height(Length::Fixed(DESCRIPTION_HEIGHT - 1.0))
            .padding(Padding::new(0.0))
            .align_y(Alignment::Center)
            .style(|_: &iced::Theme| container::Style {
                background: Some(
                    iced::Color {
                        a: 0.15,
                        ..theme::accent()
                    }
                    .into(),
                ),
                border: Border {
                    radius: {
                        let r = theme::ui_border_radius();
                        iced::border::Radius {
                            top_left: 0.0,
                            top_right: 0.0,
                            bottom_left: r.top_left,
                            bottom_right: r.top_right,
                        }
                    },
                    ..Default::default()
                },
                ..Default::default()
            });

        column![sep, desc_container]
            .width(Length::Fill)
            .height(Length::Fixed(DESCRIPTION_HEIGHT))
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
                move |font_name, ctx| {
                    let ctx = SlotRenderContext {
                        item_index: ctx.item_index,
                        is_center: ctx.is_center,
                        opacity: 1.0,
                        row_height: ctx.row_height,
                        scale_factor: ctx.scale_factor,
                        is_capturing: false,
                        conflict_text: None,
                        is_level1: false,
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
