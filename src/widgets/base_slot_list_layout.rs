//! Base slot list layout component
//!
//! Provides standard page shell layout: ViewHeader + Slot List + Optional Artwork Column
//! Matches QML's BaseSlotListView architecture

use iced::{
    Alignment, Color, ContentFit, Element, Length,
    widget::{column, container, row},
};
use nokkvi_data::types::player_settings::{ArtworkColumnMode, ArtworkStretchFit};

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

/// Minimum slot list width before artwork column hides (Auto mode only)
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

/// How the image renders inside the artwork column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PanelKind {
    /// Image is rendered square inside the column (centered, letterboxed
    /// vertically when the column is taller than wide). Used for Auto and
    /// AlwaysNative modes, and for collage panels in every mode.
    Square,
    /// Image fills the column non-square via `iced::ContentFit`. Used for
    /// AlwaysStretched mode on single-image panels.
    Stretched { fit: ContentFit },
}

/// Resolved layout for the artwork column.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ArtworkLayout {
    /// Outer column width (the row gives this many pixels to the artwork side).
    pub column_width: f32,
    /// How the panel image renders inside that column.
    pub panel_kind: PanelKind,
}

/// Resolve the artwork-column layout from window size, view config, and the
/// user's display-mode atomic. Returns `None` when the column should not be
/// shown (Never mode, Auto leftover < 800px, or `show_artwork_column = false`).
pub(crate) fn resolve_artwork_layout(config: &BaseSlotListLayoutConfig) -> Option<ArtworkLayout> {
    if !config.show_artwork_column {
        return None;
    }

    match theme::artwork_column_mode() {
        ArtworkColumnMode::Never => None,
        ArtworkColumnMode::Auto => {
            // Match QML BaseSlotListView.qml formula (lines 453-456)
            let width_based_size =
                (config.window_width * ARTWORK_MAX_WIDTH_PERCENT).min(ARTWORK_MAX_SIZE);
            let height_based_size = config.window_height;
            let is_square_window = height_based_size >= width_based_size;

            let square_size = if is_square_window {
                height_based_size.min(config.window_width * ARTWORK_SQUARE_WINDOW_PERCENT)
            } else {
                width_based_size.min(height_based_size)
            };

            let remaining_slot_list_width = config.window_width - square_size;
            if remaining_slot_list_width < MIN_SLOT_LIST_WIDTH {
                return None;
            }

            Some(ArtworkLayout {
                column_width: square_size,
                panel_kind: PanelKind::Square,
            })
        }
        ArtworkColumnMode::AlwaysNative => Some(ArtworkLayout {
            column_width: always_column_width(config.window_width),
            panel_kind: PanelKind::Square,
        }),
        ArtworkColumnMode::AlwaysStretched => {
            let fit = match theme::artwork_column_stretch_fit() {
                ArtworkStretchFit::Cover => ContentFit::Cover,
                ArtworkStretchFit::Fill => ContentFit::Fill,
            };
            Some(ArtworkLayout {
                column_width: always_column_width(config.window_width),
                panel_kind: PanelKind::Stretched { fit },
            })
        }
    }
}

/// Compute the artwork column width for AlwaysNative/AlwaysStretched modes.
/// Reads the user-tuned width fraction from the theme atomic.
fn always_column_width(window_width: f32) -> f32 {
    let pct = theme::artwork_column_width_pct();
    (window_width * pct)
        .max(1.0)
        .min(ARTWORK_MAX_SIZE.min(window_width))
}

/// Create an empty placeholder artwork element that preserves the widget tree structure.
/// Used by base_slot_list_empty_state so the root widget type (row vs column) stays consistent
/// when transitioning between results and no-results states.
pub(crate) fn base_slot_list_empty_artwork<'a, Message: 'a>(
    config: &BaseSlotListLayoutConfig,
) -> Option<Element<'a, Message>> {
    let layout = resolve_artwork_layout(config)?;

    // Stretched: fill the column rect.
    if matches!(layout.panel_kind, PanelKind::Stretched { .. }) {
        return Some(
            iced::widget::responsive(move |size| {
                iced::widget::container(iced::widget::text(""))
                    .width(Length::Fixed(size.width))
                    .height(Length::Fixed(size.height))
                    .style(|_theme| iced::widget::container::Style {
                        background: Some(artwork_outer_bg().into()),
                        ..Default::default()
                    })
                    .into()
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        );
    }

    // Square: shrink to a min(w, h) square — matches the live panel.
    Some(
        iced::widget::responsive(move |size| {
            let s = size.width.min(size.height).max(0.0);
            iced::widget::container(iced::widget::text(""))
                .width(Length::Fixed(s))
                .height(Length::Fixed(s))
                .style(|_theme| iced::widget::container::Style {
                    background: Some(artwork_outer_bg().into()),
                    ..Default::default()
                })
                .into()
        })
        .width(Length::Shrink)
        .height(Length::Shrink)
        .into(),
    )
}

/// Create a single-image artwork panel (used by albums, songs, queue, artists).
///
/// Mode-aware:
/// - `Auto` / `AlwaysNative` → responsive `Length::Shrink` resolving to a
///   `min(w, h)` square (preserves original layout exactly: column shrinks to
///   the square's natural size, no horizontal letterbox gap).
/// - `AlwaysStretched` → responsive `Length::Fill` with image filling the
///   parent rect via the configured `ContentFit`.
pub(crate) fn single_artwork_panel<'a, Message: 'a>(
    artwork_handle: Option<&'a iced::widget::image::Handle>,
) -> Element<'a, Message> {
    if matches!(
        theme::artwork_column_mode(),
        ArtworkColumnMode::AlwaysStretched
    ) {
        let fit = match theme::artwork_column_stretch_fit() {
            ArtworkStretchFit::Cover => ContentFit::Cover,
            ArtworkStretchFit::Fill => ContentFit::Fill,
        };
        return iced::widget::responsive(move |size| {
            use iced::widget::{container, image, text};

            let content: Element<'_, Message> = if let Some(handle) = artwork_handle {
                image(handle.clone())
                    .content_fit(fit)
                    .width(Length::Fixed(size.width))
                    .height(Length::Fixed(size.height))
                    .into()
            } else {
                container(text(""))
                    .width(Length::Fixed(size.width))
                    .height(Length::Fixed(size.height))
                    .style(|_theme| container::Style {
                        background: Some(artwork_outer_bg().into()),
                        ..Default::default()
                    })
                    .into()
            };

            container(content)
                .width(Length::Fixed(size.width))
                .height(Length::Fixed(size.height))
                .style(|_theme| container::Style {
                    background: Some(artwork_outer_bg().into()),
                    ..Default::default()
                })
                .into()
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into();
    }

    // Square (Auto / AlwaysNative) — original behavior.
    iced::widget::responsive(move |size| {
        use iced::widget::{container, image, text};

        let square_size = size.width.min(size.height).max(0.0);

        let content: Element<'_, Message> = if let Some(handle) = artwork_handle {
            image(handle.clone())
                .content_fit(ContentFit::Cover)
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
    .height(Length::Shrink)
    .into()
}

/// Create a single-image artwork panel with an optional right-click context menu.
///
/// When `on_refresh` is `Some`, wraps the panel in a context menu with "Refresh Artwork".
/// When `None`, this is identical to [`single_artwork_panel`].
pub(crate) fn single_artwork_panel_with_menu<'a, Message: Clone + 'a>(
    artwork_handle: Option<&'a iced::widget::image::Handle>,
    on_refresh: Option<Message>,
    is_open: bool,
    open_position: Option<iced::Point>,
    on_open_change: impl Fn(Option<iced::Point>) -> Message + 'a,
) -> Element<'a, Message> {
    let panel = single_artwork_panel(artwork_handle);

    if let Some(refresh_msg) = on_refresh {
        // Wrap in context menu with a single "Refresh Artwork" entry
        use crate::widgets::context_menu::{context_menu, menu_button};

        context_menu(
            panel,
            vec![()],
            move |_entry, _length| {
                menu_button(
                    Some("assets/icons/refresh-cw.svg"),
                    "Refresh Artwork",
                    refresh_msg.clone(),
                )
            },
            is_open,
            open_position,
            on_open_change,
        )
        .into()
    } else {
        panel
    }
}

/// Wrap an existing artwork panel element with a centered pill overlay.
///
/// Shared implementation backing `single_artwork_panel_with_pill` and
/// `collage_artwork_panel_with_pill`. Computes the background color, builds the
/// styled pill container, and stacks it on top of `base_panel`.
fn wrap_with_pill_overlay<'a, Message: 'a>(
    base_panel: Element<'a, Message>,
    content: Element<'a, Message>,
    bg_color: iced::Color,
) -> Element<'a, Message> {
    use iced::widget::{container, stack};

    let pill = container(content)
        .padding(16)
        .style(move |_theme| container::Style {
            background: Some(iced::Background::Color(bg_color)),
            border: iced::Border {
                radius: if crate::theme::is_rounded_mode() {
                    12.0.into()
                } else {
                    0.0.into()
                },
                ..Default::default()
            },
            ..Default::default()
        });

    let overlay = container(pill)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(16) // Padding pushes pill off edge
        .align_x(iced::Alignment::Center)
        .align_y(iced::Alignment::Center);

    stack![base_panel, overlay].into()
}

/// Create a single-image artwork panel with a bottom-anchored, pill-shaped overlay
pub(crate) fn single_artwork_panel_with_pill<'a, Message: Clone + 'a>(
    artwork_handle: Option<&'a iced::widget::image::Handle>,
    pill_content: Option<Element<'a, Message>>,
    dominant_color: Option<iced::Color>,
    on_refresh: Option<Message>,
    is_open: bool,
    open_position: Option<iced::Point>,
    on_open_change: impl Fn(Option<iced::Point>) -> Message + 'a,
) -> Element<'a, Message> {
    let base_panel = single_artwork_panel(artwork_handle);

    let panel = if let Some(content) = pill_content {
        // Determine background color. Use theme background blended with a hint of dominant color.
        let theme_bg = crate::theme::bg0_hard();
        let mut bg_color = dominant_color.unwrap_or(theme_bg);
        bg_color.r = theme_bg.r * 0.85 + bg_color.r * 0.15;
        bg_color.g = theme_bg.g * 0.85 + bg_color.g * 0.15;
        bg_color.b = theme_bg.b * 0.85 + bg_color.b * 0.15;
        bg_color.a = 0.85;

        wrap_with_pill_overlay(base_panel, content, bg_color)
    } else {
        base_panel
    };

    if let Some(refresh_msg) = on_refresh {
        use crate::widgets::context_menu::{context_menu, menu_button};
        context_menu(
            panel,
            vec![()],
            move |_entry, _length| {
                menu_button(
                    Some("assets/icons/refresh-cw.svg"),
                    "Refresh Artwork",
                    refresh_msg.clone(),
                )
            },
            is_open,
            open_position,
            on_open_change,
        )
        .into()
    } else {
        panel
    }
}

/// Create a 3x3 collage artwork panel with a bottom-anchored, pill-shaped overlay
pub(crate) fn collage_artwork_panel_with_pill<'a, Message: Clone + 'a>(
    collage_handles: &'a [iced::widget::image::Handle],
    pill_content: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let base_panel = collage_artwork_panel(collage_handles);

    if let Some(content) = pill_content {
        // Static dark backdrop for collages
        let mut bg_color = crate::theme::bg0_hard();
        bg_color.a = 0.85;

        wrap_with_pill_overlay(base_panel, content, bg_color)
    } else {
        base_panel
    }
}

/// Create a 3×3 collage artwork panel (used by genres, playlists)
///
/// Always renders square — even in `AlwaysStretched` mode the 3×3 grid stays
/// square, centered inside the column, with `bg0_soft` fill on the sides.
/// Per-cell stretch is intentionally not supported (looks bad on real albums).
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
                                .content_fit(ContentFit::Cover)
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
    .height(Length::Shrink)
    .into()
}

/// Create standard base slot list layout with header, slot list content, and optional artwork
pub(crate) fn base_slot_list_layout<'a, Message: 'a>(
    config: &BaseSlotListLayoutConfig,
    header: Element<'a, Message>,
    slot_list_content: Element<'a, Message>,
    artwork_content: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    base_slot_list_layout_with_handle::<
        Message,
        fn(crate::widgets::artwork_split_handle::DragEvent) -> Message,
    >(config, header, slot_list_content, artwork_content, None)
}

/// Variant of [`base_slot_list_layout`] that also renders a drag handle between
/// the slot list and artwork columns. The handle is only drawn in always
/// modes (Native/Stretched); in Auto/Never it is suppressed regardless of the
/// `on_drag` parameter.
pub(crate) fn base_slot_list_layout_with_handle<'a, Message, F>(
    config: &BaseSlotListLayoutConfig,
    header: Element<'a, Message>,
    slot_list_content: Element<'a, Message>,
    artwork_content: Option<Element<'a, Message>>,
    on_drag: Option<F>,
) -> Element<'a, Message>
where
    Message: 'a,
    F: Fn(crate::widgets::artwork_split_handle::DragEvent) -> Message + Clone + 'a,
{
    let layout = resolve_artwork_layout(config);

    if let (Some(layout), Some(artwork)) = (layout, artwork_content) {
        let mode = theme::artwork_column_mode();
        let is_always = matches!(
            mode,
            ArtworkColumnMode::AlwaysNative | ArtworkColumnMode::AlwaysStretched
        );

        // In Auto mode, pass the artwork through directly so the row sizes
        // itself to the panel's natural square (the panel's responsive
        // returns a Fixed-size square via Length::Shrink). In always modes,
        // wrap with Length::Fixed(column_width) so the user-tuned width is
        // authoritative — the panel inside will square or stretch to fit.
        let artwork_side_inner: Element<'a, Message> = if is_always {
            container(artwork)
                .width(Length::Fixed(layout.column_width))
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
                .style(|_| container::Style {
                    background: Some(artwork_outer_bg().into()),
                    ..Default::default()
                })
                .into()
        } else {
            artwork
        };

        // Drag handle only in always modes — suppressed in Auto.
        let handle: Option<Element<'a, Message>> = if is_always {
            on_drag.map(|f| {
                crate::widgets::artwork_split_handle::artwork_split_handle_element(
                    config.window_width,
                    f,
                )
            })
        } else {
            None
        };

        let artwork_side: Element<'a, Message> = if let Some(handle_elem) = handle {
            row![
                handle_elem,
                container(iced::widget::Space::new())
                    .width(Length::Fixed(2.0))
                    .height(Length::Fill)
                    .style(|_| container::Style {
                        background: Some(theme::bg1().into()),
                        ..Default::default()
                    }),
                artwork_side_inner
            ]
            .spacing(0)
            .height(Length::Fill)
            .into()
        } else {
            with_left_stripe(artwork_side_inner)
        };

        row![
            column![header, slot_list_content]
                .width(Length::Fill)
                .height(Length::Fill)
                .spacing(0),
            artwork_side
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

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, MutexGuard};

    use super::*;

    /// All tests in this module mutate the global theme atomics for artwork
    /// column mode/fit/width_pct. Acquire this lock at the start of each test
    /// so they don't race when `cargo test` runs in parallel.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn lock_atomics() -> MutexGuard<'static, ()> {
        // Allow poisoned mutexes — a panicking test shouldn't break siblings.
        TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn cfg(w: f32, h: f32, show: bool) -> BaseSlotListLayoutConfig {
        BaseSlotListLayoutConfig {
            window_width: w,
            window_height: h,
            show_artwork_column: show,
        }
    }

    fn reset_atomics() {
        theme::set_artwork_column_mode(ArtworkColumnMode::Auto);
        theme::set_artwork_column_stretch_fit(ArtworkStretchFit::Cover);
        theme::set_artwork_column_width_pct(0.40);
    }

    #[test]
    fn auto_landscape_window_shows_square() {
        let _g = lock_atomics();
        reset_atomics();
        // Wide landscape: 1920 wide → leftover = 1920 - min(768, 1080) = 1152 ≥ 800
        let l = resolve_artwork_layout(&cfg(1920.0, 1080.0, true)).expect("should show");
        assert!(matches!(l.panel_kind, PanelKind::Square));
        assert!(l.column_width > 0.0);
    }

    #[test]
    fn auto_narrow_window_hides() {
        let _g = lock_atomics();
        reset_atomics();
        // 1100 wide → max width-based = 440, leftover = 660 < 800 → hide
        assert!(resolve_artwork_layout(&cfg(1100.0, 800.0, true)).is_none());
    }

    #[test]
    fn auto_respects_show_artwork_false() {
        let _g = lock_atomics();
        reset_atomics();
        assert!(resolve_artwork_layout(&cfg(1920.0, 1080.0, false)).is_none());
    }

    #[test]
    fn never_mode_hides_even_on_wide_window() {
        let _g = lock_atomics();
        reset_atomics();
        theme::set_artwork_column_mode(ArtworkColumnMode::Never);
        assert!(resolve_artwork_layout(&cfg(1920.0, 1080.0, true)).is_none());
        reset_atomics();
    }

    #[test]
    fn always_native_uses_width_pct_and_square_kind() {
        let _g = lock_atomics();
        reset_atomics();
        theme::set_artwork_column_mode(ArtworkColumnMode::AlwaysNative);
        theme::set_artwork_column_width_pct(0.30);
        let l = resolve_artwork_layout(&cfg(1000.0, 800.0, true)).expect("should show");
        assert!((l.column_width - 300.0).abs() < 1e-3);
        assert!(matches!(l.panel_kind, PanelKind::Square));
        reset_atomics();
    }

    #[test]
    fn always_stretched_uses_configured_fit() {
        let _g = lock_atomics();
        reset_atomics();
        theme::set_artwork_column_mode(ArtworkColumnMode::AlwaysStretched);
        theme::set_artwork_column_stretch_fit(ArtworkStretchFit::Fill);
        let l = resolve_artwork_layout(&cfg(1000.0, 800.0, true)).expect("should show");
        assert!(matches!(
            l.panel_kind,
            PanelKind::Stretched {
                fit: ContentFit::Fill
            }
        ));
        reset_atomics();
    }

    #[test]
    fn always_stretched_does_not_auto_hide() {
        let _g = lock_atomics();
        reset_atomics();
        theme::set_artwork_column_mode(ArtworkColumnMode::AlwaysStretched);
        // Same narrow window that auto-hides — always modes still show.
        assert!(resolve_artwork_layout(&cfg(600.0, 800.0, true)).is_some());
        reset_atomics();
    }

    #[test]
    fn always_column_width_clamped_below_max_size() {
        let _g = lock_atomics();
        reset_atomics();
        theme::set_artwork_column_mode(ArtworkColumnMode::AlwaysNative);
        theme::set_artwork_column_width_pct(0.80);
        // Width 4000 × 0.80 = 3200, clamped to ARTWORK_MAX_SIZE (1000)
        let l = resolve_artwork_layout(&cfg(4000.0, 1080.0, true)).expect("should show");
        assert!((l.column_width - ARTWORK_MAX_SIZE).abs() < 1e-3);
        reset_atomics();
    }

    #[test]
    fn always_column_width_clamped_below_window_width() {
        let _g = lock_atomics();
        reset_atomics();
        theme::set_artwork_column_mode(ArtworkColumnMode::AlwaysNative);
        theme::set_artwork_column_width_pct(0.80);
        // Window 600 × 0.80 = 480, below ARTWORK_MAX_SIZE so stays at 480.
        let l = resolve_artwork_layout(&cfg(600.0, 800.0, true)).expect("should show");
        assert!((l.column_width - 480.0).abs() < 1e-3);
        reset_atomics();
    }

    #[test]
    fn theme_atomic_clamps_width_pct_into_safe_range() {
        let _g = lock_atomics();
        reset_atomics();
        // Out of range — atomic clamps to [0.05, 0.80].
        theme::set_artwork_column_width_pct(0.001);
        assert!((theme::artwork_column_width_pct() - 0.05).abs() < 1e-6);
        theme::set_artwork_column_width_pct(0.99);
        assert!((theme::artwork_column_width_pct() - 0.80).abs() < 1e-6);
        reset_atomics();
    }
}
