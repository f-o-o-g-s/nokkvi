//! Sub-list update and rendering logic for settings view
//!
//! Handles the color array sub-list (gradient editing) and font picker state/update.
//! Extracted from mod.rs to reduce file size.

use iced::{
    Alignment, Border, Color, Element, Length, Padding,
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

/// Resolved preview colors for a single theme row — the theme's OWN palette,
/// not the active theme's. Computed once when the picker opens (see
/// [`ThemeSubListState::new`]) and never in the render path, because
/// [`theme_loader::load_theme`](nokkvi_data::services::theme_loader::load_theme)
/// does disk IO + a TOML parse per theme.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ThemePreviewColors {
    /// Row background — the theme's `background.default` (`bg0`).
    pub bg: Color,
    /// Row text — the theme's `foreground.bright` (`fg0`), authored legible on `bg`.
    pub fg: Color,
    /// Accent swatch + cursor stripe — the theme's `accent.bright`.
    pub accent: Color,
}

/// One row in the theme picker: identity plus the pre-resolved colors used to
/// paint the row in that theme's own palette. Bundling colors WITH the row
/// (rather than a parallel `stem -> colors` map) keeps the filtered list from
/// ever drifting out of sync with its preview data.
#[derive(Debug, Clone)]
pub(crate) struct ThemeRow {
    /// Theme filename stem (the apply key).
    pub stem: String,
    /// Human-readable theme name.
    pub display_name: String,
    /// Whether the theme has a built-in counterpart.
    pub is_builtin: bool,
    /// Whether this is the currently-applied theme.
    pub is_active: bool,
    /// Pre-resolved preview colors for this theme.
    pub preview: ThemePreviewColors,
}

/// State for the theme picker sub-list — mirrors [`FontSubListState`], but each
/// row previews its own palette (see [`ThemeRow`]). Selection reuses the
/// existing apply path via [`SettingsAction::ApplyPreset`].
#[derive(Debug, Clone)]
pub(crate) struct ThemeSubListState {
    /// All discovered theme rows (unfiltered), colors pre-resolved at open.
    pub all_rows: Vec<ThemeRow>,
    /// Current search/filter query.
    pub search_query: String,
    /// Filtered rows — what the slot list displays.
    pub filtered_rows: Vec<ThemeRow>,
    /// Sub-slot list navigation state.
    pub slot_list: SlotListView,
    /// Parent detail-pane offset to restore on exit.
    pub parent_offset: usize,
}

impl ThemeSubListState {
    /// Build the picker state, resolving every discovered theme's preview
    /// colors ONCE here (disk read + TOML parse), so the render path only ever
    /// reads the cached [`ThemePreviewColors`]. Previews use the app's current
    /// mode (dark/light) — what selecting the theme would actually produce.
    pub(super) fn new(parent_offset: usize) -> Self {
        use crate::theme_config::ResolvedTheme;

        let active = super::presets::active_theme_stem();
        let light = crate::theme::is_light_mode();

        // Discover + parse each theme file ONCE here (never in the render path),
        // resolving its preview colors for the app's current mode.
        let rows: Vec<ThemeRow> = super::presets::all_theme_files()
            .into_iter()
            .map(|(info, file)| {
                let palette = if light { &file.light } else { &file.dark };
                let resolved = ResolvedTheme::from_theme_palette(palette);
                ThemeRow {
                    is_active: info.stem == active,
                    stem: info.stem,
                    display_name: info.display_name,
                    is_builtin: info.is_builtin,
                    preview: ThemePreviewColors {
                        bg: resolved.bg0,
                        fg: resolved.fg0,
                        accent: resolved.accent_bright,
                    },
                }
            })
            .collect();

        // Open centered on the active theme so the user's current selection is
        // the initial cursor (each row already carries `is_active`).
        let mut slot_list = SlotListView::new();
        if let Some(active_idx) = rows.iter().position(|r| r.is_active) {
            slot_list.set_offset(active_idx, rows.len());
        }

        Self {
            filtered_rows: rows.clone(),
            all_rows: rows,
            search_query: String::new(),
            slot_list,
            parent_offset,
        }
    }

    /// Refilter rows by case-insensitive substring on the display name,
    /// resetting the slot list to the top (mirrors [`FontSubListState::refilter`]).
    pub(super) fn refilter(&mut self) {
        if self.search_query.is_empty() {
            self.filtered_rows = self.all_rows.clone();
        } else {
            let query = self.search_query.to_lowercase();
            self.filtered_rows = self
                .all_rows
                .iter()
                .filter(|r| r.display_name.to_lowercase().contains(&query))
                .cloned()
                .collect();
        }
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
            | SettingsMessage::SubListSearchChanged(_)
            | SettingsMessage::SearchChanged(_)
            | SettingsMessage::ToggleSearch
            | SettingsMessage::ToggleSetToggle(_)
            | SettingsMessage::SidebarUp
            | SettingsMessage::SidebarDown
            | SettingsMessage::SidebarSetOffset(_, _)
            | SettingsMessage::SidebarClickItem(_)
            | SettingsMessage::JumpToSection(_) => SettingsAction::None,
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
            SettingsMessage::SubListSearchChanged(query) => {
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
            | SettingsMessage::SidebarClickItem(_)
            | SettingsMessage::JumpToSection(_) => SettingsAction::None,
        }
    }

    /// Handle messages when in theme sub-list (theme picker) mode. Mirrors
    /// [`Self::update_font_sub_list`]; selection reuses the existing
    /// [`SettingsAction::ApplyPreset`] apply path, keyed by the row's stem (no
    /// positional index to drift through the search filter).
    pub(super) fn update_theme_sub_list(&mut self, message: SettingsMessage) -> SettingsAction {
        let tsw = match self.theme_sub_list.as_mut() {
            Some(s) => s,
            None => return SettingsAction::None,
        };
        let total = tsw.filtered_rows.len().max(1);

        match message {
            SettingsMessage::SlotListUp => {
                tsw.slot_list.move_up(total);
                SettingsAction::None
            }
            SettingsMessage::SlotListDown => {
                tsw.slot_list.move_down(total);
                SettingsAction::None
            }
            SettingsMessage::SlotListSetOffset(offset, _)
            | SettingsMessage::SlotListClickItem(offset) => {
                tsw.slot_list.set_offset(offset, total);
                SettingsAction::None
            }
            SettingsMessage::EditActivate => {
                // Apply the centered theme by stem — read straight from the
                // filtered row, so search filtering can't misroute the choice.
                if let Some(center_idx) = tsw.slot_list.get_center_item_index(total)
                    && let Some(row) = tsw.filtered_rows.get(center_idx)
                {
                    let stem = row.stem.clone();
                    let display_name = row.display_name.clone();
                    let parent_offset = tsw.parent_offset;
                    self.theme_sub_list = None;
                    self.restore_parent_offset(parent_offset);
                    return SettingsAction::ApplyPreset { stem, display_name };
                }
                SettingsAction::None
            }
            SettingsMessage::SubListSearchChanged(query) => {
                tsw.search_query = query;
                tsw.refilter();
                SettingsAction::None
            }
            SettingsMessage::Escape => {
                let parent_offset = tsw.parent_offset;
                self.theme_sub_list = None;
                self.restore_parent_offset(parent_offset);
                SettingsAction::None
            }
            // Not applicable in theme sub-list:
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
            | SettingsMessage::SidebarClickItem(_)
            | SettingsMessage::JumpToSection(_) => SettingsAction::None,
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
