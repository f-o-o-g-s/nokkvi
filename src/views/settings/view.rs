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
    BREADCRUMB_HEIGHT, FONT_SEARCH_BAR_HEIGHT, SETTINGS_CHROME_HEIGHT, SETTINGS_SEARCH_INPUT_ID,
    SettingsMessage, SettingsPage, SettingsTab, SettingsViewData,
    items::SettingsEntry,
    rendering::{SlotRenderContext, render_settings_slot, transparent_button_style},
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

    /// Sidebar body: slot list of the six categories. Reuses the existing
    /// L1-hero renderer via `is_level1: true`; the active row is the slot
    /// list's center, which `apply_sidebar_index` keeps synced to
    /// `active_category`. Click routes through `SidebarClickItem`.
    fn render_sidebar_body(&self) -> Element<'_, SettingsMessage> {
        let entries: Vec<SettingsEntry> = SettingsTab::ALL
            .iter()
            .map(|tab| SettingsEntry::Header {
                label: tab.label(),
                icon: tab.icon_path(),
            })
            .collect();

        // Use the dynamic slot config so the sidebar still scrolls if the
        // window is shorter than 6 rows × row_height.
        let mut config = slot_list::SlotListConfig::with_dynamic_slots(
            // Carve out chrome: top-bar/player-bar (96) + sidebar header
            // (60) + sidebar footer (44).
            // The slot list runs in the remaining space.
            f32::MAX,
            SETTINGS_CHROME_HEIGHT + SIDEBAR_HEADER_HEIGHT + SIDEBAR_FOOTER_HEIGHT,
        );
        config.cull_empty = true;

        let entries_owned = entries.clone();

        slot_list::slot_list_view_with_scroll(
            &self.sidebar_slot_list,
            &entries_owned,
            &config,
            SettingsMessage::SidebarUp,
            SettingsMessage::SidebarDown,
            // Scrollbar seek: clamp into the sidebar slot range.
            {
                let total = entries_owned.len();
                move |f: f32| {
                    SettingsMessage::SidebarSetOffset(
                        (f * total as f32) as usize,
                        iced::keyboard::Modifiers::default(),
                    )
                }
            },
            None,
            move |entry, ctx| {
                let ctx = SlotRenderContext {
                    item_index: ctx.item_index,
                    is_center: ctx.is_center,
                    opacity: ctx.opacity,
                    row_height: ctx.row_height,
                    scale_factor: ctx.scale_factor,
                    is_capturing: false,
                    conflict_text: None,
                    is_level1: true,
                    toggle_cursor: None,
                };
                render_sidebar_slot(&ctx, entry)
            },
        )
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

    /// Render the detail pane — slot list of the active category's entries.
    ///
    /// Phase 3 placeholder: uses the existing uniform-height slot list
    /// renderer with `is_level1: false`. Phase 4 swaps in the
    /// variable-height detail row layout from the design.
    fn render_detail_pane<'a>(
        &'a self,
        entries: &[SettingsEntry],
        window_height: f32,
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

        let mut config =
            slot_list::SlotListConfig::with_dynamic_slots(window_height, SETTINGS_CHROME_HEIGHT);
        config.cull_empty = true;

        let entries_owned: Vec<SettingsEntry> = entries.to_vec();
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
            None,
            move |entry, ctx| {
                let is_editing = editing_index == Some(ctx.item_index);
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
                    is_level1: false,
                    toggle_cursor: if ctx.is_center { toggle_cursor } else { None },
                };
                let hi = if is_editing { &hex_input_owned } else { "" };
                render_settings_slot(&ctx, entry, is_editing, hi)
            },
        );

        container(slot_list_content)
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

/// Render a single sidebar row — visual is the L1 category hero (icon
/// chip + name + blurb + 2 px accent left stripe on active), but clicks
/// dispatch sidebar messages rather than the detail-pane drill-down
/// messages the L1 hero hardcodes. Headers only (sidebar entries are all
/// `SettingsEntry::Header`); `Item` rows are unreachable here.
fn render_sidebar_slot<'a>(
    ctx: &SlotRenderContext<'_>,
    entry: &SettingsEntry,
) -> Element<'a, SettingsMessage> {
    let (label, icon_path) = match entry {
        SettingsEntry::Header { label, icon } => (*label, *icon),
        SettingsEntry::Item(_) => return container(text("")).width(Length::Fill).into(),
    };

    let title_size =
        nokkvi_data::utils::scale::calculate_font_size(20.0, ctx.row_height, ctx.scale_factor)
            * ctx.scale_factor;
    let desc_size =
        nokkvi_data::utils::scale::calculate_font_size(11.0, ctx.row_height, ctx.scale_factor)
            * ctx.scale_factor;

    let title_color = if ctx.is_center {
        theme::fg0()
    } else {
        sidebar_scale_alpha(theme::fg0(), ctx.opacity * 0.85)
    };
    let desc_color = sidebar_scale_alpha(theme::fg2(), ctx.opacity * 0.85);

    let chip_size = (title_size * 2.4).clamp(40.0, 56.0);
    let icon_inner_size = (chip_size * 0.5).clamp(20.0, 28.0);
    let icon_color = sidebar_scale_alpha(theme::accent_bright(), ctx.opacity);
    let chip_bg = sidebar_scale_alpha(theme::bg0_hard(), ctx.opacity);
    let chip_border = sidebar_scale_alpha(theme::border(), ctx.opacity);

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
            ..theme::ui_font()
        })
        .color(title_color);
    let description = sidebar_category_description(label);
    let desc_widget = text(description)
        .size(desc_size)
        .font(theme::ui_font())
        .color(desc_color);

    let text_col = column![title, desc_widget].spacing(4).width(Length::Fill);

    let content = row![
        Space::new().width(Length::Fixed(14.0)),
        icon_chip,
        Space::new().width(Length::Fixed(12.0)),
        container(text_col)
            .width(Length::Fill)
            .height(Length::Fill)
            .clip(true)
            .align_y(Alignment::Center),
    ]
    .spacing(0)
    .align_y(Alignment::Center)
    .height(Length::Fill);

    let body = sidebar_cursor_stripe(content.into(), ctx.is_center);

    button(body)
        .style(transparent_button_style)
        .padding(0)
        .width(Length::Fill)
        .on_press(SettingsMessage::SidebarClickItem(ctx.item_index))
        .into()
}

/// Wrap a sidebar row body in the active-state chrome: bg1 fill + 2 px
/// accent left stripe when this row matches the active category. Mirrors
/// the design's `.nk-cat-row.active` block.
fn sidebar_cursor_stripe<'a>(
    body: Element<'a, SettingsMessage>,
    is_active: bool,
) -> Element<'a, SettingsMessage> {
    let bg = if is_active {
        theme::bg1()
    } else {
        theme::bg0_hard()
    };
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
    let row_with_stripe = row![stripe, body]
        .align_y(Alignment::Center)
        .height(Length::Fill);
    container(row_with_stripe)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_: &iced::Theme| container::Style {
            background: Some(bg.into()),
            ..Default::default()
        })
        .into()
}

/// Per-category description shown below the sidebar row title. Reads
/// directly from `SettingsTab::description()` so a single source of
/// truth feeds both the sidebar blurb and any future ALL-iteration
/// surface (e.g. Hotkeys page descriptions).
fn sidebar_category_description(label: &str) -> &'static str {
    SettingsTab::ALL
        .iter()
        .find(|t| t.label() == label)
        .map_or("Configure this section", |t| t.description())
}

/// Multiply a color's alpha by `factor`. Local helper to avoid pulling
/// in the private `scale_alpha_local` from `rendering.rs`.
fn sidebar_scale_alpha(color: Color, factor: f32) -> Color {
    Color {
        a: (color.a * factor).clamp(0.0, 1.0),
        ..color
    }
}
