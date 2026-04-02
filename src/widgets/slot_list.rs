//! Consolidated Slot List Component
//!
//! Generic 9-slot circular navigation interface that can render any item type.
//! Generic slot list view component for circular navigation
//!
//! Provides reusable slot list rendering with configurable item rendering

use iced::{
    Color, Element, Length, Padding,
    widget::{column, container, mouse_area},
};

use crate::{theme, widgets::SlotListView};

/// Per-slot rendering context passed to render closures.
///
/// Bundles all the common parameters that every slot list row renderer needs,
/// avoiding long argument lists in both the closure and the render functions.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SlotListRowContext {
    pub item_index: usize,
    pub is_center: bool,
    pub is_selected: bool,
    pub opacity: f32,
    /// Window scale factor
    pub scale_factor: f32,
    /// Layout row height
    pub row_height: f32,
    /// Global keyboard modifiers
    pub modifiers: iced::keyboard::Modifiers,
}

/// Styling for slot list slots (backgrounds, borders, text colors)
#[derive(Debug, Clone, Copy)]
pub(crate) struct SlotListSlotStyle {
    pub bg_color: Color,
    pub border_color: Color,
    pub border_width: f32,
    pub border_radius: iced::border::Radius,
    pub text_color: Color,
    pub subtext_color: Color,
}

impl SlotListSlotStyle {
    /// Get slot styling based on state
    pub(crate) fn for_slot(
        is_center: bool,
        is_highlighted: bool,
        is_selected: bool,
        opacity: f32,
    ) -> Self {
        if is_highlighted {
            // Currently playing/selected state (e.g., current song in queue)
            Self {
                bg_color: theme::now_playing_color(),
                border_color: theme::accent_bright(),
                border_width: 2.0,
                border_radius: slot_list_border_radius(),
                text_color: theme::bg0_hard(),
                subtext_color: theme::bg0_hard(),
            }
        } else if is_selected || is_center {
            // Selected item or center slot highlighting
            // decoupled from the now-playing highlight color.
            Self {
                bg_color: theme::selected_color(),
                border_color: if is_center {
                    theme::accent_bright()
                } else {
                    theme::selected_color()
                },
                border_width: 2.0,
                border_radius: slot_list_border_radius(),
                text_color: theme::bg0_hard(),
                subtext_color: theme::bg0_hard(),
            }
        } else {
            // Regular slot with opacity fade (both background and text)
            Self {
                bg_color: Color {
                    a: opacity,
                    ..theme::bg0()
                },
                border_color: Color {
                    a: opacity,
                    ..theme::bg3()
                },
                border_width: 1.0,
                border_radius: slot_list_border_radius(),
                text_color: Color {
                    a: opacity,
                    ..theme::fg0()
                },
                subtext_color: Color {
                    a: opacity,
                    ..theme::fg4()
                },
            }
        }
    }

    /// Convert to an `iced::widget::container::Style` for slot background/border rendering.
    ///
    /// This is the single source of truth for the `SlotListSlotStyle → container::Style`
    /// conversion used across all view files, empty slots, and drag previews.
    pub(crate) fn to_container_style(self) -> container::Style {
        container::Style {
            background: Some(self.bg_color.into()),
            border: iced::Border {
                color: self.border_color,
                width: self.border_width,
                radius: self.border_radius,
            },
            ..Default::default()
        }
    }
}

/// Standard padding for slot list slot content
pub(crate) const SLOT_LIST_SLOT_PADDING: f32 = 8.0;

/// Border radius for slot list slots — reads the current rounded mode setting.
pub(crate) fn slot_list_border_radius() -> iced::border::Radius {
    crate::theme::ui_border_radius()
}

/// Standard vertical spacing between slot list slot elements
pub(crate) const SLOT_LIST_COL_SPACING: f32 = 4.0;

/// Standard width for the index column (supports up to 4 digits)
pub(crate) const SLOT_LIST_INDEX_WIDTH: f32 = 60.0;

/// Minimum row height before we try to reduce slot count (pixels)
const MIN_COMFORTABLE_ROW_HEIGHT: f32 = 55.0;

/// Target row height — reads the user-configurable setting from `theme::slot_row_height()`.
/// Defaults to 70px (calibrated to ~65px at 758px window with 9 slots).
fn target_row_height() -> f32 {
    crate::theme::slot_row_height()
}

/// Maximum slot count to prevent excessive slots on very tall displays
const MAX_SLOT_COUNT: usize = 29;

// =========================================================================
// Layout Constants (single source of truth)
//
// These are used by slot_list.rs itself, cross_pane_drag.rs position
// calculations, and app_view.rs drop-indicator rendering.
// =========================================================================

/// Spacing between slot list slots in the column layout (pixels).
pub(crate) const SLOT_SPACING: f32 = 3.0;

/// Height of the navigation bar at the top of the window (28px content + 4px borders).
pub(crate) const NAV_BAR_HEIGHT: f32 = 32.0;

/// Height of the view header row (sort controls, search, etc.).
pub(crate) const VIEW_HEADER_HEIGHT: f32 = 48.0;

/// Height of the playlist edit / playlist context bar.
pub(crate) const EDIT_BAR_HEIGHT: f32 = 32.0;

/// Height of the browsing panel tab bar.
pub(crate) const TAB_BAR_HEIGHT: f32 = 36.0;

/// Bottom padding for slot_list_background_container — also subtracted in row_height()
/// to keep slot heights in sync with actual available space. Single source of truth.
const SLOT_LIST_CONTAINER_PADDING: f32 = 10.0;

use super::player_bar::player_bar_height;

/// Total height of chrome elements for views with headers.
///
/// In top nav mode: nav_bar(32) + player_bar(56+) + view_header(48)
/// In side nav mode: player_bar(56+) + view_header(48) (no top bar)
///   + TopBar strip (21+1) when TrackInfoDisplay::TopBar is active
pub(crate) fn chrome_height_with_header() -> f32 {
    if crate::theme::is_side_nav() {
        // Side mode: no top nav bar, but TopBar track info strip adds height above content
        let top_bar_strip = if crate::theme::show_top_bar_strip() {
            super::track_info_strip::STRIP_HEIGHT_WITH_SEPARATOR
        } else {
            0.0
        };
        player_bar_height() + VIEW_HEADER_HEIGHT + top_bar_strip
    } else {
        NAV_BAR_HEIGHT + player_bar_height() + VIEW_HEADER_HEIGHT
    }
}

/// Y-coordinate where the queue slot list begins in window space.
///
/// Encapsulates the `is_side_nav` / `show_top_bar_strip` / nav-bar branching
/// that was previously inlined in `app_view.rs` and `cross_pane_drag.rs`.
///
/// `edit_bar_height`: extra height from playlist edit/context bars (typically 0 or 32).
pub(crate) fn queue_slot_list_start_y(edit_bar_height: f32) -> f32 {
    if crate::theme::is_side_nav() {
        let top_strip = if crate::theme::show_top_bar_strip() {
            super::track_info_strip::STRIP_HEIGHT_WITH_SEPARATOR
        } else {
            0.0
        };
        top_strip + VIEW_HEADER_HEIGHT + edit_bar_height
    } else {
        NAV_BAR_HEIGHT + VIEW_HEADER_HEIGHT + edit_bar_height
    }
}

/// Configuration for slot list rendering
#[derive(Debug, Clone)]
pub(crate) struct SlotListConfig {
    pub slot_count: usize,
    pub center_slot: usize,
    pub window_height: f32,
    pub chrome_height: f32, // nav_bar + player_bar + other UI elements
    /// When true, slots with no corresponding item are skipped (not rendered).
    /// Used by settings to avoid showing empty placeholder rows.
    pub cull_empty: bool,
    /// The global keyboard modifiers state (for shift/ctrl clicking)
    pub modifiers: iced::keyboard::Modifiers,
}

impl Default for SlotListConfig {
    fn default() -> Self {
        Self {
            slot_count: 9,
            center_slot: 4,
            window_height: 800.0,
            chrome_height: chrome_height_with_header(),
            cull_empty: false,
            modifiers: iced::keyboard::Modifiers::default(),
        }
    }
}

impl SlotListConfig {
    /// Create a new SlotListConfig with automatic slot count based on available height.
    ///
    /// Computes the slot count to keep row height near `target_row_height()` (~70px).
    /// Tall windows get more slots (11, 13, 15…) instead of comically large rows;
    /// short windows get fewer slots (7, 5, 3, 1) as before.
    /// Slot count is always odd so the center slot works correctly.
    pub(crate) fn with_dynamic_slots(window_height: f32, chrome_height: f32) -> Self {
        let available_height =
            (window_height - chrome_height - SLOT_LIST_CONTAINER_PADDING).max(0.0);

        // Estimate content height with a mid-range spacing guess for initial calc
        let estimated_spacing = 8.0 * SLOT_SPACING; // ~9 slots worth
        let estimated_content = (available_height - estimated_spacing).max(0.0);
        let raw_count = estimated_content / target_row_height();

        // Round to nearest odd number: try both adjacent odds, pick the one
        // whose resulting row height is closest to target_row_height().
        let lower_odd = ((raw_count as usize) | 1).max(1); // nearest odd <= raw (or 1)
        let upper_odd = lower_odd + 2;

        let row_height_for = |count: usize| -> f32 {
            let spacing = count.saturating_sub(1) as f32 * SLOT_SPACING;
            let content = (available_height - spacing).max(0.0);
            content / count.max(1) as f32
        };

        let lower_height = row_height_for(lower_odd);
        let upper_height = row_height_for(upper_odd);

        let best_slot_count = if upper_odd <= MAX_SLOT_COUNT
            && upper_height >= MIN_COMFORTABLE_ROW_HEIGHT
            && (upper_height - target_row_height()).abs()
                <= (lower_height - target_row_height()).abs()
        {
            upper_odd
        } else if lower_height >= MIN_COMFORTABLE_ROW_HEIGHT {
            lower_odd
        } else {
            // Window is very small — fall back to reducing slot count until MIN is met
            let mut count = lower_odd;
            while count > 1 && row_height_for(count) < MIN_COMFORTABLE_ROW_HEIGHT {
                count = count.saturating_sub(2); // step down by 2 to stay odd
            }
            count.max(1)
        };

        let best_slot_count = best_slot_count.min(MAX_SLOT_COUNT);

        Self {
            slot_count: best_slot_count,
            center_slot: best_slot_count / 2, // Works because slot count is always odd
            window_height,
            chrome_height,
            cull_empty: false,
            modifiers: iced::keyboard::Modifiers::default(),
        }
    }

    /// Builder method to set global keyboard modifiers for slot interactions
    pub(crate) fn with_modifiers(mut self, modifiers: iced::keyboard::Modifiers) -> Self {
        self.modifiers = modifiers;
        self
    }

    /// Calculate row height based on window size
    /// All slots have uniform height.
    pub(crate) fn row_height(&self) -> f32 {
        let available_height =
            (self.window_height - self.chrome_height - SLOT_LIST_CONTAINER_PADDING).max(0.0);
        let spacing_height = (self.slot_count.saturating_sub(1)) as f32 * SLOT_SPACING;
        let content_height = (available_height - spacing_height).max(0.0);

        (content_height / self.slot_count.max(1) as f32).max(40.0)
    }
}

/// Render a slot list view with custom item rendering
///
/// # Arguments
/// * `sl` - The SlotListView managing viewport offset
/// * `items` - Slice of items to render
/// * `config` - Slot list configuration
/// * `render_item` - Closure to render each item, receives (item_index, item, slot_index, is_center, opacity, row_height, scale_factor)
///   Note: The closure should clone/copy any data it needs from the item, as the returned Element's lifetime
///   is independent of the item's lifetime.
///
/// # Returns
/// Element containing the slot list view
pub(crate) fn slot_list_view<'a, T, Message: 'a>(
    sl: &SlotListView,
    items: &[T],
    config: &SlotListConfig,
    mut render_item: impl FnMut(&T, SlotListRowContext) -> Element<'a, Message>,
) -> Element<'a, Message> {
    let slots = build_slot_list_slots(sl, items, config, &mut render_item);

    container(
        column(slots)
            .spacing(3)
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// Render a slot list view with scroll support
///
/// Wraps the slot list in a mouse_area that captures scroll events and emits
/// the provided messages for navigation. Scrolling up navigates to previous
/// items, scrolling down navigates to next items.
///
/// # Arguments
/// * `sl` - The SlotListView managing viewport offset
/// * `items` - Slice of items to render
/// * `config` - Slot list configuration
/// * `on_scroll_up` - Message to emit when scrolling up (previous item)
/// * `on_scroll_down` - Message to emit when scrolling down (next item)
/// * `render_item` - Closure to render each item
///
/// # Returns
/// Element containing the scrollable slot list view
pub(crate) fn slot_list_view_with_scroll<'a, T, Message: Clone + 'a>(
    sl: &SlotListView,
    items: &[T],
    config: &SlotListConfig,
    on_scroll_up: Message,
    on_scroll_down: Message,
    on_seek: impl Fn(f32) -> Message + 'a,
    render_item: impl FnMut(&T, SlotListRowContext) -> Element<'a, Message>,
) -> Element<'a, Message> {
    let total_items = items.len();
    let row_height = config.row_height();
    let inner = slot_list_view(sl, items, config, render_item);
    let inner = wrap_with_scroll(inner, on_scroll_up, on_scroll_down);
    crate::widgets::scroll_indicator::wrap_with_scroll_indicator(
        inner,
        sl,
        total_items,
        row_height,
        on_seek,
    )
}

/// Render a slot list view with scroll support AND drag-and-drop reordering.
///
/// Same as `slot_list_view_with_scroll` but the inner column of slots is a `DragColumn`
/// that emits drag events via `on_drag_event`. Slot indices in the `DragEvent` are
/// raw **slot** indices — caller translates to item indices via `viewport_offset`.
#[expect(clippy::too_many_arguments)] // Mirrors slot_list_view_with_scroll (7 args) +1 on_drag_event; struct would require boxing on_seek
pub(crate) fn slot_list_view_with_drag<'a, T, Message: Clone + 'a>(
    sl: &SlotListView,
    items: &[T],
    config: &SlotListConfig,
    on_scroll_up: Message,
    on_scroll_down: Message,
    on_seek: impl Fn(f32) -> Message + 'a,
    on_drag_event: impl Fn(crate::widgets::drag_column::DragEvent) -> Message + 'a,
    mut render_item: impl FnMut(&T, SlotListRowContext) -> Element<'a, Message>,
) -> Element<'a, Message> {
    use crate::widgets::drag_column::DragColumn;

    let total_items = items.len();
    let row_height = config.row_height();
    let slots = build_slot_list_slots(sl, items, config, &mut render_item);

    let inner: Element<'a, Message> = container(
        DragColumn::from_vec(slots)
            .spacing(3)
            .width(Length::Fill)
            .height(Length::Fill)
            .on_drag(on_drag_event),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into();

    let inner = wrap_with_scroll(inner, on_scroll_up, on_scroll_down);
    crate::widgets::scroll_indicator::wrap_with_scroll_indicator(
        inner,
        sl,
        total_items,
        row_height,
        on_seek,
    )
}

/// Build the slot elements for a slot list view.
///
/// Shared by `slot_list_view`, `slot_list_view_with_drag`, etc. to avoid duplicating
/// the effective-center calculation and slot rendering logic.
fn build_slot_list_slots<'a, T, Message: 'a>(
    sl: &SlotListView,
    items: &[T],
    config: &SlotListConfig,
    render_item: &mut impl FnMut(&T, SlotListRowContext) -> Element<'a, Message>,
) -> Vec<Element<'a, Message>> {
    let row_height = config.row_height();
    let total_items = items.len();

    // Dynamic center: adapts based on position in the list.
    // When fewer items than slots exist, center=0 so items pack to the top
    // and empty slots flow naturally below. In this case viewport_offset must
    // be treated as 0 — scrolling is meaningless when all items fit, and
    // using the real viewport_offset would cause items to disappear off the
    // top as the user scrolls down.
    let top_packing = total_items < config.slot_count;
    let effective_center = if top_packing {
        sl.viewport_offset.min(total_items.saturating_sub(1))
    } else {
        let items_at_and_after = total_items.saturating_sub(sl.viewport_offset);
        let end_push = config.slot_count.saturating_sub(items_at_and_after);
        config.center_slot.min(sl.viewport_offset).max(end_push)
    };

    let mut slots = Vec::with_capacity(config.slot_count);
    for slot_index in 0..config.slot_count {
        let opacity = if crate::theme::is_opacity_gradient() {
            SlotListView::calculate_slot_opacity_with_center(slot_index, effective_center)
        } else {
            1.0
        };
        let scale_factor = 1.0;

        let mut is_center_slot = false;

        // In top-packing mode, map slots directly to item indices (slot N → item N),
        // ignoring viewport_offset which is meaningless when all items fit.
        let item_index_opt = if top_packing {
            if slot_index < total_items {
                Some(slot_index)
            } else {
                None
            }
        } else {
            sl.get_slot_item_index_with_center(slot_index, total_items, effective_center)
        };

        let slot_content: Element<'a, Message> = if let Some(item_index) = item_index_opt {
            if let Some(item) = items.get(item_index) {
                let disable_fallback_center = config.modifiers.shift()
                    || config.modifiers.control()
                    || sl.selected_indices.len() > 1;
                let is_center = match sl.selected_offset {
                    Some(sel) => item_index == sel,
                    None => {
                        if disable_fallback_center {
                            false
                        } else {
                            slot_index == effective_center
                        }
                    }
                };
                is_center_slot = is_center;
                let is_selected = sl.selected_indices.contains(&item_index);
                let ctx = SlotListRowContext {
                    item_index,
                    is_center,
                    is_selected,
                    opacity,
                    row_height,
                    scale_factor,
                    modifiers: config.modifiers,
                };
                render_item(item, ctx)
            } else if config.cull_empty {
                continue;
            } else {
                empty_slot(opacity)
            }
        } else if config.cull_empty {
            continue;
        } else {
            empty_slot(opacity)
        };

        let slot_element = container(slot_content)
            .width(Length::Fill)
            .height(Length::Fixed(row_height));

        let flash = if is_center_slot {
            sl.flash_center_at
        } else {
            None
        };

        slots.push(
            crate::widgets::hover_overlay::HoverOverlay::new(slot_element)
                .border_radius(slot_list_border_radius())
                .flash_at(flash)
                .into(),
        );
    }

    slots
}

/// Wrap a slot list view element with scroll event handling.
fn wrap_with_scroll<'a, Message: Clone + 'a>(
    inner: Element<'a, Message>,
    on_scroll_up: Message,
    on_scroll_down: Message,
) -> Element<'a, Message> {
    use iced::mouse::ScrollDelta;

    mouse_area(inner)
        .on_scroll(move |delta| {
            let y = match delta {
                ScrollDelta::Lines { y, .. } => y,
                ScrollDelta::Pixels { y, .. } => y,
            };

            if y > 0.0 {
                on_scroll_up.clone()
            } else {
                on_scroll_down.clone()
            }
        })
        .into()
}

/// Standard slot list text with no line wrapping
///
/// Uses `Wrapping::None` + `Ellipsis::End` so iced's text layout engine
/// handles overflow natively — text is clipped with "…" at the container
/// boundary without any manual width estimation.
pub(crate) fn slot_list_text<'a>(
    content: impl Into<String>,
    size: f32,
    color: Color,
) -> iced::widget::Text<'a> {
    use iced::widget::{
        text,
        text::{Ellipsis, Wrapping},
    };

    text(content.into())
        .size(size)
        .color(color)
        .font(theme::ui_font())
        .wrapping(Wrapping::None)
        .ellipsis(Ellipsis::End)
}

/// Render the index column for a slot list slot
///
/// # Arguments
/// * `index` - The item index (0-based, will be displayed as 1-based)
/// * `font_size` - Scaled font size for the text
/// * `style` - Slot styling to determine text color
/// * `opacity` - Opacity for non-highlighted slots
pub(crate) fn slot_list_index_column<'a, Message: 'a>(
    index: usize,
    font_size: f32,
    style: SlotListSlotStyle,
    opacity: f32,
) -> Element<'a, Message> {
    use iced::Alignment;

    container(slot_list_text(
        format!("{}", index + 1),
        font_size,
        if style.text_color == theme::bg0_hard() {
            theme::bg0_hard()
        } else {
            iced::Color {
                a: opacity * 0.7,
                ..theme::fg4()
            }
        },
    ))
    .width(Length::Fixed(SLOT_LIST_INDEX_WIDTH))
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
    .into()
}

/// Render an artwork column for a slot list slot
///
/// # Arguments
/// * `artwork_handle` - Optional image handle for the artwork
/// * `artwork_size` - Size of the artwork square in pixels
/// * `is_center` - Whether this is the centered slot
/// * `is_highlighted` - Whether this slot is highlighted (e.g., currently playing)
/// * `opacity` - Opacity for non-highlighted slots (0.0-1.0)
pub(crate) fn slot_list_artwork_column<'a, Message: 'a>(
    artwork_handle: Option<&'a iced::widget::image::Handle>,
    artwork_size: f32,
    is_center: bool,
    is_highlighted: bool,
    opacity: f32,
) -> Element<'a, Message> {
    use iced::widget::image;

    let effective_opacity = if is_center || is_highlighted {
        1.0
    } else {
        opacity
    };

    if let Some(handle) = artwork_handle {
        container(
            image(handle.clone())
                .content_fit(iced::ContentFit::Cover)
                .width(Length::Fill)
                .height(Length::Fill)
                .opacity(effective_opacity),
        )
        .width(Length::Fixed(artwork_size))
        .height(Length::Fixed(artwork_size))
        .style(move |_theme| container::Style {
            background: Some(
                Color {
                    a: effective_opacity,
                    ..theme::bg2()
                }
                .into(),
            ),
            ..Default::default()
        })
        .into()
    } else {
        container(iced::widget::text(""))
            .width(Length::Fixed(artwork_size))
            .height(Length::Fixed(artwork_size))
            .style(move |_theme| container::Style {
                background: Some(
                    Color {
                        a: effective_opacity,
                        ..theme::bg2()
                    }
                    .into(),
                ),
                ..Default::default()
            })
            .into()
    }
}

/// Render a text column with title and subtitle for a slot list slot
///
/// Two-line text column for slot list rows (title + subtitle).
///
/// Uses iced's native `Ellipsis::End` for overflow — the text layout engine
/// handles clipping and "…" insertion based on actual glyph measurements
/// and container bounds. No manual width estimation or font-size shrinking.
///
/// # Arguments
/// * `title` - Primary text (e.g., album name, song title)
/// * `subtitle` - Secondary text (e.g., artist name)
/// * `title_size` - Font size for the title
/// * `subtitle_size` - Font size for the subtitle
/// * `style` - Slot styling to determine text colors
/// * `is_bold` - Whether to bold the title
/// * `portion` - FillPortion width allocation
pub(crate) fn slot_list_text_column<'a, Message: 'a>(
    title: String,
    subtitle: String,
    title_size: f32,
    subtitle_size: f32,
    style: SlotListSlotStyle,
    is_bold: bool,
    portion: u16,
) -> Element<'a, Message> {
    use iced::{
        Alignment,
        widget::text::{Ellipsis, Wrapping},
    };

    let title_font = if is_bold {
        iced::Font {
            weight: iced::font::Weight::Bold,
            ..theme::ui_font()
        }
    } else {
        theme::ui_font()
    };

    let title_widget = iced::widget::text(title)
        .size(title_size)
        .color(style.text_color)
        .font(title_font)
        .wrapping(Wrapping::None)
        .ellipsis(Ellipsis::End);

    let subtitle_widget = slot_list_text(subtitle, subtitle_size, style.subtext_color);

    container(column![title_widget, subtitle_widget,].spacing(SLOT_LIST_COL_SPACING))
        .width(Length::FillPortion(portion))
        .height(Length::Fill)
        .clip(true)
        .align_y(Alignment::Center)
        .into()
}

/// Render a metadata column for a slot list slot (single line of text)
///
/// # Arguments
/// * `content` - The metadata text to display
/// * `font_size` - Font size for the text
/// * `style` - Slot styling to determine text color
/// * `portion` - FillPortion width allocation
pub(crate) fn slot_list_metadata_column<'a, Message: 'a>(
    content: String,
    font_size: f32,
    style: SlotListSlotStyle,
    portion: u16,
) -> Element<'a, Message> {
    use iced::Alignment;

    container(slot_list_text(content, font_size, style.subtext_color))
        .width(Length::FillPortion(portion))
        .height(Length::Fill)
        .clip(true)
        .align_y(Alignment::Center)
        .into()
}

/// Layer a filled SVG icon with a semi-transparent outline SVG on top.
///
/// Used by star ratings and favorite icons to ensure the filled icon has a
/// visible contrasting edge regardless of the background color / theme.
fn outlined_svg_icon<'a, M: 'a>(
    filled_path: &str,
    outline_path: &str,
    icon_size: f32,
    fill_color: Color,
    opacity: f32,
) -> Element<'a, M> {
    use iced::widget::svg;

    let outline_color = Color {
        a: 0.6,
        ..theme::bg0_hard()
    };
    let fill_svg: Element<'a, M> = crate::embedded_svg::svg_widget(filled_path)
        .width(Length::Fixed(icon_size))
        .height(Length::Fixed(icon_size))
        .opacity(opacity)
        .style(move |_theme, _status| svg::Style {
            color: Some(fill_color),
        })
        .into();
    let outline_svg: Element<'a, M> = crate::embedded_svg::svg_widget(outline_path)
        .width(Length::Fixed(icon_size))
        .height(Length::Fixed(icon_size))
        .opacity(opacity)
        .style(move |_theme, _status| svg::Style {
            color: Some(outline_color),
        })
        .into();
    iced::widget::stack![fill_svg, outline_svg]
        .width(Length::Fixed(icon_size))
        .height(Length::Fixed(icon_size))
        .into()
}

/// Render a star rating display (1-5 stars) for slot list slots
///
/// Replaces per-star copy-paste with a loop. Uses filled/empty star SVGs
/// with yellow for filled stars and contextual colors for empty stars.
///
/// # Arguments
/// * `rating` - Star count (0-5), clamped internally
/// * `icon_size` - Size of each star icon in pixels
/// * `is_center` - Whether this is the centered slot list slot
/// * `opacity` - Opacity for non-center slots (0.0-1.0)
/// * `portion` - When `Some(n)`, wraps the stars in a `FillPortion(n)` container
///   for use as a standalone slot list column. When `None`, returns the bare star row
///   for embedding inside caller-controlled layouts (e.g. a column).
pub(crate) fn slot_list_star_rating<'a, Message: Clone + 'a>(
    rating: usize,
    icon_size: f32,
    is_center: bool,
    opacity: f32,
    portion: Option<u16>,
    on_click: Option<impl Fn(usize) -> Message + 'a>,
) -> Element<'a, Message> {
    use iced::{
        Alignment,
        widget::{row, svg},
    };

    let svg_opacity = if is_center { 1.0 } else { opacity };
    let filled_color = theme::star_bright();
    let empty_color = if is_center {
        theme::bg0_hard()
    } else {
        theme::fg4()
    };

    let stars = (1..=5).fold(row![].spacing(2), |r, i| {
        let star_element: Element<'a, Message> = if rating >= i {
            outlined_svg_icon(
                "assets/icons/star-filled.svg",
                "assets/icons/star.svg",
                icon_size,
                filled_color,
                svg_opacity,
            )
        } else {
            let color = empty_color;
            crate::embedded_svg::svg_widget("assets/icons/star.svg")
                .width(Length::Fixed(icon_size))
                .height(Length::Fixed(icon_size))
                .opacity(svg_opacity)
                .style(move |_theme, _status| svg::Style { color: Some(color) })
                .into()
        };

        // Wrap each star in a clickable mouse_area when on_click is provided
        let star_element: Element<'a, Message> = if let Some(ref on_click) = on_click {
            mouse_area(star_element)
                .on_press(on_click(i))
                .interaction(iced::mouse::Interaction::Pointer)
                .into()
        } else {
            star_element
        };

        r.push(star_element)
    });

    match portion {
        Some(p) => container(stars)
            .width(Length::FillPortion(p))
            .align_y(Alignment::Center)
            .into(),
        None => stars.into(),
    }
}

/// Render a favorite icon (heart or star) with proper colors for slot list slots
///
/// Handles color logic for starred items, centered slots, highlighted slots, and regular slots.
/// Use this for consistent favorite icon rendering across all slot-list-based views.
///
/// # Arguments
/// * `is_starred` - Whether the item is starred/favorited
/// * `is_center` - Whether this is the centered slot
/// * `is_highlighted` - Whether this slot is highlighted (e.g., currently playing)
/// * `opacity` - Opacity for non-highlighted slots (0.0-1.0)
/// * `icon_size` - Size of the icon in pixels
/// * `icon_type` - "heart" for songs/queue, "star" for albums
/// * `on_click` - Optional message to emit when clicked (toggles starred state)
pub(crate) fn slot_list_favorite_icon<'a, Message: Clone + 'a>(
    is_starred: bool,
    is_center: bool,
    is_highlighted: bool,
    opacity: f32,
    icon_size: f32,
    icon_type: &'a str,
    on_click: Option<Message>,
) -> Element<'a, Message> {
    use iced::widget::svg;

    let (filled_icon, empty_icon) = match icon_type {
        "heart" => ("assets/icons/heart-filled.svg", "assets/icons/heart.svg"),
        "star" => ("assets/icons/star-filled.svg", "assets/icons/star.svg"),
        _ => ("assets/icons/heart-filled.svg", "assets/icons/heart.svg"), // default to heart
    };

    let svg_opacity = if is_center || is_highlighted {
        1.0
    } else {
        opacity
    };

    let svg_element: Element<'a, Message> = if is_starred {
        // Starred: layer filled icon + outline on top for contrast
        let fill_color = match icon_type {
            "heart" => theme::danger_bright(),
            "star" => theme::star_bright(),
            _ => theme::danger_bright(),
        };
        outlined_svg_icon(filled_icon, empty_icon, icon_size, fill_color, svg_opacity)
    } else {
        // Not starred: just the empty outline
        let color = if is_center || is_highlighted {
            theme::bg0_hard()
        } else {
            theme::fg4()
        };
        crate::embedded_svg::svg_widget(empty_icon)
            .width(Length::Fixed(icon_size))
            .height(Length::Fixed(icon_size))
            .opacity(svg_opacity)
            .style(move |_theme, _status| svg::Style { color: Some(color) })
            .into()
    };

    // Wrap in clickable mouse_area when on_click is provided
    if let Some(message) = on_click {
        mouse_area(svg_element)
            .on_press(message)
            .interaction(iced::mouse::Interaction::Pointer)
            .into()
    } else {
        svg_element
    }
}

/// Create an empty slot element with a visible placeholder
///
/// Uses the same border/background styling as a regular slot at minimum opacity
/// so empty slots read as "slots" rather than floating text.
fn empty_slot<'a, Message: 'a>(opacity: f32) -> Element<'a, Message> {
    use iced::Alignment;

    let style = SlotListSlotStyle::for_slot(false, false, false, opacity);

    container(
        iced::widget::text("· · ·")
            .size(14)
            .color(Color {
                a: opacity * 0.4,
                ..theme::fg4()
            })
            .font(theme::ui_font()),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
    .style(move |_theme| style.to_container_style())
    .into()
}

/// Wrap slot list content with standard dark background container
///
/// This prevents lighter background colors from bleeding through transparent slot list slots.
/// Should be used by all slot-list-based views (albums, queue, etc.) for visual consistency.
pub(crate) fn slot_list_background_container<'a, Message: 'a>(
    slot_list_content: Element<'a, Message>,
) -> Element<'a, Message> {
    container(slot_list_content)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(
            Padding::new(0.0)
                .right(10.0)
                .bottom(SLOT_LIST_CONTAINER_PADDING)
                .left(10.0),
        )
        .style(theme::container_bg0_hard)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_row_height() {
        // Test with explicit 9-slot config
        let config = SlotListConfig {
            window_height: 900.0,
            chrome_height: 234.0,
            slot_count: 9,
            center_slot: 0,
            cull_empty: false,
            modifiers: iced::keyboard::Modifiers::default(),
        };

        let row_height = config.row_height();
        // Available = 900 - 234 - 10 (padding) = 656
        // Spacing = 8 * 3 = 24
        // Content = 632
        // Row height = 632 / 9 ≈ 70.22
        assert!((row_height - 70.222).abs() < 0.01);
    }

    #[test]
    fn test_config_min_row_height() {
        let config = SlotListConfig {
            window_height: 100.0, // Very small window
            chrome_height: 200.0, // Chrome larger than window
            slot_count: 9,
            center_slot: 4,
            cull_empty: false,
            modifiers: iced::keyboard::Modifiers::default(),
        };

        let row_height = config.row_height();
        assert_eq!(row_height, 40.0); // Should clamp to minimum
    }

    #[test]
    fn test_dynamic_slot_count_large_window() {
        // Large window: should get more than 9 slots to keep row height near TARGET
        let config = SlotListConfig::with_dynamic_slots(900.0, 134.0);
        // Available = 900 - 134 - 10 = 756
        // raw ≈ 756 / 70 ≈ 10.5 → try 11 and 13
        // 11 slots: spacing=30, content=726, row=66
        // 13 slots: spacing=36, content=720, row=55.4
        // 11 is closer to 70 → 11
        assert_eq!(config.slot_count, 11);
        assert_eq!(config.center_slot, 5);
    }

    #[test]
    fn test_dynamic_slot_count_medium_window() {
        // Medium window should get fewer slots
        let config = SlotListConfig::with_dynamic_slots(450.0, 134.0);
        // Available = 450 - 134 - 10 = 306
        // raw ≈ 306 / 70 ≈ 4.1 → try 3 and 5
        // 3 slots: spacing=6, content=300, row=100 (|100-70|=30)
        // 5 slots: spacing=12, content=294, row=58.8 (|58.8-70|=11.2)
        // 5 is closer → 5
        assert_eq!(config.slot_count, 5);
        assert_eq!(config.center_slot, 2);
    }

    #[test]
    fn test_dynamic_slot_count_small_window() {
        // Very small window should fall back to minimum slots
        let config = SlotListConfig::with_dynamic_slots(250.0, 134.0);
        // Available = 250 - 134 - 10 = 106
        // raw ≈ 106 / 70 ≈ 1.3 → lower_odd=1, upper_odd=3
        // 1 slot: spacing=0, content=106, row=106  (|106-70|=36)
        // 3 slots: spacing=6, content=100, row=33.3 (< MIN 55)
        // upper fails MIN → use lower_odd=1
        assert_eq!(config.slot_count, 1);
        assert_eq!(config.center_slot, 0);
    }

    #[test]
    fn test_dynamic_slot_count_keeps_target_row_height() {
        // At various window heights, row height should stay near target_row_height()
        for window_height in [600.0, 758.0, 900.0, 1080.0, 1200.0, 1440.0, 2160.0] {
            let config = SlotListConfig::with_dynamic_slots(window_height, 134.0);
            let row_height = config.row_height();
            // Row height should always be in a reasonable range
            assert!(
                row_height >= MIN_COMFORTABLE_ROW_HEIGHT,
                "window_height={window_height}: row_height={row_height} < MIN={MIN_COMFORTABLE_ROW_HEIGHT}"
            );
            // Should never exceed 2× TARGET (prevents comically large slots)
            assert!(
                row_height < target_row_height() * 2.0,
                "window_height={window_height}: row_height={row_height} >= 2×TARGET={}",
                target_row_height() * 2.0
            );
        }
    }
}
