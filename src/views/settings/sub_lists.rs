//! Sub-list update and rendering logic for settings view
//!
//! Handles the color array sub-list (gradient editing) and font picker state/update.
//! Extracted from mod.rs to reduce file size.

use iced::{
    Alignment, Border, Element, Length, Padding,
    font::{Font, Weight},
    widget::{Space, button, column, container, row, svg, text},
};

use super::{
    BREADCRUMB_HEIGHT, SETTINGS_CHROME_HEIGHT, SettingsAction, SettingsMessage, SettingsPage,
    normalize_hex,
    rendering::{SlotRenderContext, render_color_slot, transparent_button_style},
};
use crate::{
    embedded_svg, theme,
    widgets::{SlotListView, slot_list},
};

// ============================================================================
// Sub-list State Types
// ============================================================================

/// State for a color sub-list (drilling into a ColorArray)
#[derive(Debug, Clone)]
pub(crate) struct SubListState {
    /// TOML key of the parent ColorArray item
    pub key: String,
    /// Label of the parent item (e.g. "Bar Gradient")
    pub label: String,
    /// The color array being edited
    pub colors: Vec<String>,
    /// Sub-slot list navigation state
    pub slot_list: SlotListView,
    /// Index of the color currently being hex-edited (None = browse mode)
    pub editing_color_index: Option<usize>,
    /// Current hex input buffer for inline editing
    pub hex_input: String,
    /// Parent slot list offset to restore on exit
    pub parent_offset: usize,
}

/// State for the font picker sub-list
#[derive(Debug, Clone)]
pub(crate) struct FontSubListState {
    /// All discovered font families (unfiltered)
    pub all_fonts: Vec<String>,
    /// Current search/filter query
    pub search_query: String,
    /// Filtered font list (what's displayed in the slot list)
    pub filtered_fonts: Vec<String>,
    /// Sub-slot list navigation state
    pub slot_list: SlotListView,
    /// Parent slot list offset to restore on exit
    pub parent_offset: usize,
}

impl FontSubListState {
    /// Create a new font sub-list with the given font list
    pub(super) fn new(all_fonts: Vec<String>, parent_offset: usize) -> Self {
        // Prepend the default entry
        let mut fonts_with_default = vec!["Iced Default (SansSerif)".to_string()];
        fonts_with_default.extend(all_fonts.iter().cloned());
        Self {
            filtered_fonts: fonts_with_default.clone(),
            all_fonts: fonts_with_default,
            search_query: String::new(),
            slot_list: SlotListView::new(),
            parent_offset,
        }
    }

    /// Refilter the font list based on the current search query
    pub(super) fn refilter(&mut self) {
        if self.search_query.is_empty() {
            self.filtered_fonts = self.all_fonts.clone();
        } else {
            let query = self.search_query.to_lowercase();
            self.filtered_fonts = self
                .all_fonts
                .iter()
                .filter(|f| f.to_lowercase().contains(&query))
                .cloned()
                .collect();
        }
        // Reset slot list to top when filter changes
        self.slot_list = SlotListView::new();
    }
}

// ============================================================================
// Sub-list Update Logic
// ============================================================================

/// Commit any in-progress hex edit and clear edit state.
/// Returns `WriteColorEntry` if the buffer parsed cleanly, else `None`.
fn commit_pending_hex_edit(sub: &mut SubListState) -> SettingsAction {
    let Some(idx) = sub.editing_color_index else {
        return SettingsAction::None;
    };
    let action = if let Some(normalized) = normalize_hex(&sub.hex_input) {
        if let Some(color) = sub.colors.get_mut(idx) {
            *color = normalized.clone();
        }
        SettingsAction::WriteColorEntry {
            key: crate::config_writer::ConfigKey::theme_array(sub.key.clone()),
            index: idx,
            hex_color: normalized,
        }
    } else {
        SettingsAction::None
    };
    sub.editing_color_index = None;
    sub.hex_input.clear();
    action
}

impl SettingsPage {
    /// Handle messages when in sub-list (color array editing) mode
    pub(super) fn update_sub_list(&mut self, message: SettingsMessage) -> SettingsAction {
        let sub = match self.sub_list.as_mut() {
            Some(s) => s,
            None => return SettingsAction::None,
        };
        let total = sub.colors.len().max(1);

        match message {
            SettingsMessage::SlotListUp => {
                let action = commit_pending_hex_edit(sub);
                sub.slot_list.move_up(total);
                action
            }
            SettingsMessage::SlotListDown => {
                let action = commit_pending_hex_edit(sub);
                sub.slot_list.move_down(total);
                action
            }
            SettingsMessage::SlotListSetOffset(offset, _)
            | SettingsMessage::SlotListClickItem(offset) => {
                let action = commit_pending_hex_edit(sub);
                sub.slot_list.set_offset(offset, total);
                action
            }
            SettingsMessage::EditActivate => {
                // Toggle hex input on center color
                if sub.editing_color_index.is_some() {
                    // Exit hex editing
                    sub.editing_color_index = None;
                    sub.hex_input.clear();
                } else if let Some(center_idx) = sub.slot_list.get_center_item_index(total) {
                    // Enter hex editing for this color
                    if let Some(color) = sub.colors.get(center_idx) {
                        sub.editing_color_index = Some(center_idx);
                        sub.hex_input = color.clone();
                        return SettingsAction::FocusHexInput;
                    }
                }
                SettingsAction::None
            }
            SettingsMessage::HexInputChanged(new_hex) => {
                sub.hex_input = new_hex;
                SettingsAction::None
            }
            SettingsMessage::HexInputSubmit => {
                // Validate and apply the hex input
                if let Some(color_idx) = sub.editing_color_index
                    && let Some(normalized) = normalize_hex(&sub.hex_input)
                {
                    // Update local state
                    if let Some(color) = sub.colors.get_mut(color_idx) {
                        *color = normalized.clone();
                    }
                    sub.editing_color_index = None;
                    sub.hex_input.clear();
                    return SettingsAction::WriteColorEntry {
                        key: crate::config_writer::ConfigKey::theme_array(sub.key.clone()),
                        index: color_idx,
                        hex_color: normalized,
                    };
                }
                SettingsAction::None
            }
            SettingsMessage::Escape => {
                if sub.editing_color_index.is_some() {
                    // Exit hex editing, stay in sub-list
                    sub.editing_color_index = None;
                    sub.hex_input.clear();
                    SettingsAction::None
                } else {
                    // Exit sub-list, return to parent settings slot list
                    let parent_offset = sub.parent_offset;
                    self.sub_list = None;
                    self.restore_parent_offset(parent_offset);
                    SettingsAction::None
                }
            }
            SettingsMessage::EditLeft
            | SettingsMessage::EditRight
            | SettingsMessage::EditSetValue(_)
            | SettingsMessage::EditSetFraction(_)
            | SettingsMessage::ResetToDefault
            | SettingsMessage::HotkeyCaptured(_, _)
            | SettingsMessage::FontSearchChanged(_)
            | SettingsMessage::SearchChanged(_)
            | SettingsMessage::ToggleSearch
            | SettingsMessage::ToggleSetToggle(_)
            | SettingsMessage::SidebarUp
            | SettingsMessage::SidebarDown
            | SettingsMessage::SidebarSetOffset(_, _)
            | SettingsMessage::SidebarClickItem(_) => SettingsAction::None,
        }
    }

    /// Handle messages when in font sub-list (font picker) mode
    pub(super) fn update_font_sub_list(&mut self, message: SettingsMessage) -> SettingsAction {
        let fsw = match self.font_sub_list.as_mut() {
            Some(s) => s,
            None => return SettingsAction::None,
        };
        let total = fsw.filtered_fonts.len().max(1);

        match message {
            SettingsMessage::SlotListUp => {
                fsw.slot_list.move_up(total);
                SettingsAction::None
            }
            SettingsMessage::SlotListDown => {
                fsw.slot_list.move_down(total);
                SettingsAction::None
            }
            SettingsMessage::SlotListSetOffset(offset, _)
            | SettingsMessage::SlotListClickItem(offset) => {
                fsw.slot_list.set_offset(offset, total);
                SettingsAction::None
            }
            SettingsMessage::EditActivate => {
                // Select the center font
                if let Some(center_idx) = fsw.slot_list.get_center_item_index(total)
                    && let Some(font_name) = fsw.filtered_fonts.get(center_idx).cloned()
                {
                    let parent_offset = fsw.parent_offset;
                    self.font_sub_list = None;
                    self.restore_parent_offset(parent_offset);
                    // Convert default entry to empty string
                    let family = if font_name.starts_with("Iced Default") {
                        String::new()
                    } else {
                        font_name
                    };
                    return SettingsAction::WriteFontFamily(family);
                }
                SettingsAction::None
            }
            SettingsMessage::FontSearchChanged(query) => {
                fsw.search_query = query;
                fsw.refilter();
                SettingsAction::None
            }
            SettingsMessage::Escape => {
                let parent_offset = fsw.parent_offset;
                self.font_sub_list = None;
                self.restore_parent_offset(parent_offset);
                SettingsAction::None
            }
            // Not applicable in font sub-list:
            SettingsMessage::EditLeft
            | SettingsMessage::EditRight
            | SettingsMessage::EditSetValue(_)
            | SettingsMessage::EditSetFraction(_)
            | SettingsMessage::ResetToDefault
            | SettingsMessage::HexInputChanged(_)
            | SettingsMessage::HexInputSubmit
            | SettingsMessage::HotkeyCaptured(_, _)
            | SettingsMessage::SearchChanged(_)
            | SettingsMessage::ToggleSearch
            | SettingsMessage::ToggleSetToggle(_)
            | SettingsMessage::SidebarUp
            | SettingsMessage::SidebarDown
            | SettingsMessage::SidebarSetOffset(_, _)
            | SettingsMessage::SidebarClickItem(_) => SettingsAction::None,
        }
    }

    // ========================================================================
    // Sub-list Rendering
    // ========================================================================

    /// Render the color sub-list (gradient color editing)
    pub(super) fn render_sub_list<'a>(
        &'a self,
        sub: &'a SubListState,
        window_height: f32,
        _font: iced::Font,
    ) -> Element<'a, SettingsMessage> {
        if sub.colors.is_empty() {
            return container(text("Empty gradient").size(14).color(theme::fg4()))
                .width(Length::Fill)
                .height(Length::Fill)
                .center(Length::Fill)
                .into();
        }

        let config = slot_list::SlotListConfig::with_dynamic_slots(
            window_height,
            SETTINGS_CHROME_HEIGHT + BREADCRUMB_HEIGHT,
        );
        let colors_owned = sub.colors.clone();
        let label = sub.label.clone();
        let label_for_title = label.clone();
        let editing_color_index = sub.editing_color_index;
        let hex_input = sub.hex_input.clone();
        let total_colors = colors_owned.len();

        let slot_list_content = slot_list::slot_list_view_with_scroll(
            &sub.slot_list,
            &colors_owned,
            &config,
            SettingsMessage::SlotListUp,
            SettingsMessage::SlotListDown,
            super::settings_seek_to(total_colors),
            None,
            move |hex_color, ctx| {
                let is_editing = editing_color_index == Some(ctx.item_index);
                let ctx = SlotRenderContext {
                    item_index: ctx.item_index,
                    is_center: ctx.is_center,
                    opacity: ctx.opacity,
                    row_height: ctx.row_height,
                    scale_factor: ctx.scale_factor,
                    is_capturing: false,
                    conflict_text: None,
                    toggle_cursor: None,
                };
                render_color_slot(
                    &ctx,
                    hex_color,
                    &label,
                    total_colors,
                    is_editing,
                    if is_editing { &hex_input } else { "" },
                )
            },
        );

        // Inline sub-list title bar: shows the gradient's display label
        // plus an X back-button. The sidebar still highlights the parent
        // category (Theme / Visualizer), so this header just disambiguates
        // "you're inside a color editor right now". No path-y breadcrumb
        // needed.
        let title = text(label_for_title)
            .size(13.0)
            .color(theme::fg0())
            .font(Font {
                weight: Weight::Bold,
                ..theme::ui_font()
            });
        let back_btn = button(
            embedded_svg::svg_widget("assets/icons/x.svg")
                .width(Length::Fixed(13.0))
                .height(Length::Fixed(13.0))
                .style(move |_, _| svg::Style {
                    color: Some(theme::fg3()),
                }),
        )
        .on_press(SettingsMessage::Escape)
        .style(transparent_button_style)
        .padding(Padding::new(2.0));

        let title_row = row![title, Space::new().width(Length::Fill), back_btn]
            .align_y(Alignment::Center)
            .padding(Padding::new(0.0).left(18.0).right(14.0));

        let title_sep = container(Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(1.0))
            .style(|_: &iced::Theme| container::Style {
                background: Some(theme::border().into()),
                ..Default::default()
            });

        let title_bar = column![
            container(title_row)
                .width(Length::Fill)
                .height(Length::Fixed(BREADCRUMB_HEIGHT - 1.0))
                .align_y(Alignment::Center),
            title_sep,
        ]
        .width(Length::Fill)
        .height(Length::Fixed(BREADCRUMB_HEIGHT));

        let content = column![title_bar, slot_list_content]
            .width(Length::Fill)
            .height(Length::Fill);

        let _ = Border::default();
        slot_list::slot_list_background_container(content.into())
    }
}
