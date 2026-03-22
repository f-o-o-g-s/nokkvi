//! Base slot list layout component
//!
//! Provides standard page shell layout: ViewHeader + Slot List + Optional Artwork Column
//! Matches QML's BaseSlotListView architecture

use iced::{
    Alignment, Color, Element, Length,
    widget::{column, container, row},
};

use crate::theme;

/// Wrap an artwork panel element with a 1px `bg3` stripe on its left edge,
/// visually separating it from the slot list column.
fn with_left_stripe<'a, Message: 'a>(artwork: Element<'a, Message>) -> Element<'a, Message> {
    let stripe = container(iced::widget::Space::new())
        .width(Length::Fixed(2.0))
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(theme::bg1().into()),
            ..Default::default()
        });
    row![stripe, artwork].spacing(0).height(Length::Fill).into()
}

/// Artwork column styling - functions for dynamic theme support
#[inline]
pub(crate) fn artwork_outer_bg() -> Color {
    theme::bg0_soft()
}

/// Minimum slot list width before artwork column hides
pub(crate) const MIN_SLOT_LIST_WIDTH: f32 = 800.0;

/// Maximum artwork panel size as percentage of window width (for width-based calculation)
pub(crate) const ARTWORK_MAX_WIDTH_PERCENT: f32 = 0.40;

/// Maximum artwork panel size as percentage of window width (for square windows)
pub(crate) const ARTWORK_SQUARE_WINDOW_PERCENT: f32 = 0.60;

/// Maximum artwork panel size in pixels
pub(crate) const ARTWORK_MAX_SIZE: f32 = 1000.0;

/// Configuration for base slot list layout
#[derive(Debug, Clone)]
pub(crate) struct BaseSlotListLayoutConfig {
    pub window_width: f32,
    pub window_height: f32,
    pub show_artwork_column: bool,
}

/// Determine if artwork column should be shown based on window size
pub(crate) fn should_show_artwork(config: &BaseSlotListLayoutConfig) -> bool {
    if !config.show_artwork_column {
        return false;
    }

    // Match QML BaseSlotListView.qml formula (lines 453-456)
    let width_based_size = (config.window_width * ARTWORK_MAX_WIDTH_PERCENT).min(ARTWORK_MAX_SIZE);
    let height_based_size = config.window_height;
    let is_square_window = height_based_size >= width_based_size;

    let square_size = if is_square_window {
        // For square/portrait windows, use up to 60% of width
        height_based_size.min(config.window_width * ARTWORK_SQUARE_WINDOW_PERCENT)
    } else {
        // For landscape windows, use the smaller of width-based or height
        width_based_size.min(height_based_size)
    };

    let remaining_slot_list_width = config.window_width - square_size;

    remaining_slot_list_width >= MIN_SLOT_LIST_WIDTH
}

/// Create an empty placeholder artwork element that preserves the widget tree structure.
/// Used by base_slot_list_empty_state so the root widget type (row vs column) stays consistent
/// when transitioning between results and no-results states.
pub(crate) fn base_slot_list_empty_artwork<'a, Message: 'a>(
    config: &BaseSlotListLayoutConfig,
) -> Option<Element<'a, Message>> {
    if !should_show_artwork(config) {
        return None;
    }

    // Build a placeholder artwork panel that occupies the same space as the real artwork
    // but shows nothing — just the background color
    Some(
        iced::widget::responsive(move |size| {
            let square_size = size.width.min(size.height).max(0.0);

            iced::widget::container(iced::widget::text(""))
                .width(Length::Fixed(square_size))
                .height(Length::Fixed(square_size))
                .style(|_theme| iced::widget::container::Style {
                    background: Some(artwork_outer_bg().into()),
                    ..Default::default()
                })
                .into()
        })
        .width(Length::Shrink)
        .into(),
    )
}

/// Create a single-image artwork panel (used by albums, songs, queue, artists)
///
/// Wraps an optional image handle in a responsive outer/inner container that
/// enforces a square aspect ratio. When `artwork_handle` is `None`, shows empty space.
///
/// The `responsive()` closure captures `&'a Handle` and returns `Element<'a>` —
/// both share the same lifetime, so the borrow is valid.
pub(crate) fn single_artwork_panel<'a, Message: 'a>(
    artwork_handle: Option<&'a iced::widget::image::Handle>,
) -> Element<'a, Message> {
    iced::widget::responsive(move |size| {
        use iced::widget::{container, image, text};

        let square_size = size.width.min(size.height).max(0.0);

        let content: Element<'_, Message> = if let Some(handle) = artwork_handle {
            image(handle.clone())
                .content_fit(iced::ContentFit::Cover)
                .width(Length::Fixed(square_size))
                .height(Length::Fixed(square_size))
                .into()
        } else {
            container(text(""))
                .width(Length::Fixed(square_size))
                .height(Length::Fixed(square_size))
                .style(|_theme| container::Style {
                    background: Some(artwork_outer_bg().into()),
                    ..Default::default()
                })
                .into()
        };

        container(content)
            .width(Length::Fixed(square_size))
            .height(Length::Fixed(square_size))
            .style(|_theme| container::Style {
                background: Some(artwork_outer_bg().into()),
                ..Default::default()
            })
            .into()
    })
    .width(Length::Shrink)
    .into()
}

/// Create a single-image artwork panel with an optional right-click context menu.
///
/// When `on_refresh` is `Some`, wraps the panel in a context menu with "Refresh Artwork".
/// When `None`, this is identical to [`single_artwork_panel`].
pub(crate) fn single_artwork_panel_with_menu<'a, Message: Clone + 'a>(
    artwork_handle: Option<&'a iced::widget::image::Handle>,
    on_refresh: Option<Message>,
) -> Element<'a, Message> {
    let panel = single_artwork_panel(artwork_handle);

    if let Some(refresh_msg) = on_refresh {
        // Wrap in context menu with a single "Refresh Artwork" entry
        use crate::widgets::context_menu::{context_menu, menu_button};

        context_menu(panel, vec![()], move |_entry, _length| {
            menu_button(
                Some("assets/icons/refresh-cw.svg"),
                "Refresh Artwork",
                refresh_msg.clone(),
            )
        })
        .into()
    } else {
        panel
    }
}

/// Create a 3×3 collage artwork panel (used by genres, playlists)
///
/// Shows a grid of album covers when `collage_handles` has 2+ entries.
/// If `collage_handles` is empty, shows an empty placeholder.
/// Images are repeated (modulo) to fill all 9 cells.
///
/// The `responsive()` closure captures `&'a [Handle]` and returns `Element<'a>` —
/// both share the same lifetime, so the borrow is valid.
pub(crate) fn collage_artwork_panel<'a, Message: 'a>(
    collage_handles: &'a [iced::widget::image::Handle],
) -> Element<'a, Message> {
    iced::widget::responsive(move |size| {
        use iced::widget::{column as col, container, image, row as irow, text};

        let square_size = size.width.min(size.height).max(0.0);

        let content: Element<'_, Message> = if collage_handles.is_empty() {
            container(text(""))
                .width(Length::Fixed(square_size))
                .height(Length::Fixed(square_size))
                .style(|_theme| container::Style {
                    background: Some(artwork_outer_bg().into()),
                    ..Default::default()
                })
                .into()
        } else {
            let num = collage_handles.len();
            let cell_size = square_size / 3.0;

            let mut rows_vec: Vec<Element<'_, Message>> = Vec::new();
            for row_idx in 0..3 {
                let mut cells: Vec<Element<'_, Message>> = Vec::new();
                for col_idx in 0..3 {
                    let h = &collage_handles[(row_idx * 3 + col_idx) % num];
                    cells.push(
                        container(
                            image(h.clone())
                                .content_fit(iced::ContentFit::Cover)
                                .width(Length::Fixed(cell_size))
                                .height(Length::Fixed(cell_size)),
                        )
                        .width(Length::Fixed(cell_size))
                        .height(Length::Fixed(cell_size))
                        .into(),
                    );
                }
                rows_vec.push(
                    irow(cells)
                        .spacing(0.0)
                        .height(Length::Fixed(cell_size))
                        .into(),
                );
            }

            col(rows_vec)
                .spacing(0.0)
                .width(Length::Fixed(square_size))
                .height(Length::Fixed(square_size))
                .into()
        };

        container(content)
            .width(Length::Fixed(square_size))
            .height(Length::Fixed(square_size))
            .style(|_theme| container::Style {
                background: Some(artwork_outer_bg().into()),
                ..Default::default()
            })
            .into()
    })
    .width(Length::Shrink)
    .into()
}

/// Create standard base slot list layout with header, slot list content, and optional artwork
pub(crate) fn base_slot_list_layout<'a, Message: 'a>(
    config: &BaseSlotListLayoutConfig,
    header: Element<'a, Message>,
    slot_list_content: Element<'a, Message>,
    artwork_content: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let show_artwork = should_show_artwork(config);

    if show_artwork && let Some(artwork) = artwork_content {
        // Two-column layout: slot list + artwork (stripe on artwork's left edge)
        row![
            column![header, slot_list_content]
                .width(Length::Fill)
                .height(Length::Fill)
                .spacing(0),
            with_left_stripe(artwork)
        ]
        .align_y(Alignment::Start)
        .spacing(0)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    } else {
        // Single column: just slot list
        column![header, slot_list_content]
            .width(Length::Fill)
            .height(Length::Fill)
            .spacing(0)
            .into()
    }
}
