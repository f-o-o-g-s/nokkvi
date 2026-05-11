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

/// Wrap an artwork panel element with a 2 px `bg1` stripe on its left edge,
/// visually separating it from the slot list column in Horizontal mode.
fn with_left_stripe<'a, Message: 'a>(artwork: Element<'a, Message>) -> Element<'a, Message> {
    let stripe = container(iced::widget::Space::new())
        .width(Length::Fixed(HORIZONTAL_ARTWORK_STRIPE))
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

/// Minimum slot list height before the vertical Auto-mode fallback hides the
/// stacked artwork. Mirrors `MIN_SLOT_LIST_WIDTH` on the vertical axis but
/// uses a smaller floor — the player bar + view header already eat ~150 px of
/// chrome, so requiring 400 px below the artwork keeps room for a handful of
/// slot rows at the comfortable target row height.
pub(crate) const MIN_SLOT_LIST_HEIGHT: f32 = 400.0;

/// Maximum artwork panel size as percentage of window width (for square windows)
pub(crate) const ARTWORK_SQUARE_WINDOW_PERCENT: f32 = 0.60;

/// Maximum artwork panel size in pixels
pub(crate) const ARTWORK_MAX_SIZE: f32 = 1000.0;

/// Thickness of the `bg1` divider on the left edge of the artwork column in
/// Horizontal orientation. Vertical orientation uses padding-based separation
/// instead (see `VERTICAL_ARTWORK_BOTTOM_PAD`).
pub(crate) const HORIZONTAL_ARTWORK_STRIPE: f32 = 2.0;

/// Left/right inset for the Vertical-orientation artwork — matches the 10 px
/// horizontal padding `slot_list_background_container` applies to the slot
/// list, so the artwork's edges line up vertically with the slot rows.
pub(crate) const VERTICAL_ARTWORK_SIDE_PAD: f32 = 10.0;

/// Bottom inset for the Vertical-orientation artwork — matches
/// `SLOT_LIST_CONTAINER_PADDING` (10 px) so the gap below the artwork mirrors
/// the gap above the player bar. Top inset is 0 because the view header
/// already provides vertical breathing room.
pub(crate) const VERTICAL_ARTWORK_BOTTOM_PAD: f32 = 10.0;

/// Configuration for base slot list layout
#[derive(Debug, Clone)]
pub(crate) struct BaseSlotListLayoutConfig {
    pub window_width: f32,
    pub window_height: f32,
    pub show_artwork_column: bool,
    /// Slot-list chrome the view passes to `SlotListConfig::with_dynamic_slots`
    /// (i.e. `chrome_height_with_select_header(select_visible)`). The vertical
    /// orientation uses this to pin the slot-list rect to a Fixed height that
    /// exactly matches the slot-count math — without it, iced's flex layout
    /// can give the slot list a few pixels more than `with_dynamic_slots`
    /// expected, producing a partial slot at the bottom. Horizontal /
    /// Always-mode layouts ignore this field.
    pub slot_list_chrome: f32,
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

/// Where the artwork panel sits relative to the slot list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArtworkOrientation {
    /// Artwork in a column to the right of the slot list (default). Used by
    /// every mode except the Auto-mode portrait fallback.
    Horizontal,
    /// Artwork stacked above the slot list. Auto mode only — triggered when
    /// the horizontal candidate would leave < `MIN_SLOT_LIST_WIDTH` for the
    /// list and the window is taller than it is wide.
    Vertical,
}

/// Resolved layout for the artwork panel.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ArtworkLayout {
    /// Size of the artwork panel in the constraining direction:
    /// column width in `Horizontal`, row height in `Vertical`.
    pub extent: f32,
    /// How the panel image renders inside its allotted rect.
    pub panel_kind: PanelKind,
    /// Side of the slot list the artwork occupies.
    pub orientation: ArtworkOrientation,
}

/// Resolve the artwork-panel layout from window size, view config, and the
/// user's display-mode atomic. Returns `None` when the panel should not be
/// shown (Never mode, Auto with neither horizontal nor vertical fit, or
/// `show_artwork_column = false`).
pub(crate) fn resolve_artwork_layout(config: &BaseSlotListLayoutConfig) -> Option<ArtworkLayout> {
    if !config.show_artwork_column {
        return None;
    }

    match theme::artwork_column_mode() {
        ArtworkColumnMode::Never => None,
        ArtworkColumnMode::Auto => {
            // Horizontal candidate — original QML BaseSlotListView formula,
            // with the max-percent factor lifted to a user-tunable atomic.
            let auto_max_pct = theme::artwork_auto_max_pct();
            let width_based_size = (config.window_width * auto_max_pct).min(ARTWORK_MAX_SIZE);
            let height_based_size = config.window_height;
            let is_square_window = height_based_size >= width_based_size;

            let h_square = if is_square_window {
                height_based_size.min(config.window_width * ARTWORK_SQUARE_WINDOW_PERCENT)
            } else {
                width_based_size.min(height_based_size)
            };

            if config.window_width - h_square >= MIN_SLOT_LIST_WIDTH {
                return Some(ArtworkLayout {
                    extent: h_square,
                    panel_kind: PanelKind::Square,
                    orientation: ArtworkOrientation::Horizontal,
                });
            }

            // Horizontal doesn't fit. Try the vertical fallback on portrait
            // windows: mirror the formula on the other axis. The artwork is
            // inset by `VERTICAL_ARTWORK_SIDE_PAD` on each side to line up
            // with the slot rows, so the available width for the square is
            // `window_width - 2 * pad`. Only triggers when the height-based
            // square is at least that wide — otherwise the panel would show
            // `bg0_soft` letterbox bars inside the inset, which looks
            // awkward; hide instead.
            if config.window_height > config.window_width {
                let inset_width = (config.window_width - 2.0 * VERTICAL_ARTWORK_SIDE_PAD).max(0.0);
                let v_square_uncapped = (config.window_height * auto_max_pct).min(ARTWORK_MAX_SIZE);

                if v_square_uncapped >= inset_width {
                    let v_square = inset_width.min(ARTWORK_MAX_SIZE);

                    if config.window_height - v_square - VERTICAL_ARTWORK_BOTTOM_PAD
                        >= MIN_SLOT_LIST_HEIGHT
                    {
                        return Some(ArtworkLayout {
                            extent: v_square,
                            panel_kind: PanelKind::Square,
                            orientation: ArtworkOrientation::Vertical,
                        });
                    }
                }
            }

            None
        }
        ArtworkColumnMode::AlwaysNative => Some(ArtworkLayout {
            extent: always_column_width(config.window_width),
            panel_kind: PanelKind::Square,
            orientation: ArtworkOrientation::Horizontal,
        }),
        ArtworkColumnMode::AlwaysStretched => {
            let fit = match theme::artwork_column_stretch_fit() {
                ArtworkStretchFit::Cover => ContentFit::Cover,
                ArtworkStretchFit::Fill => ContentFit::Fill,
            };
            Some(ArtworkLayout {
                extent: always_column_width(config.window_width),
                panel_kind: PanelKind::Stretched { fit },
                orientation: ArtworkOrientation::Horizontal,
            })
        }
        ArtworkColumnMode::AlwaysVerticalNative => Some(ArtworkLayout {
            extent: always_vertical_extent(config.window_height),
            panel_kind: PanelKind::Square,
            orientation: ArtworkOrientation::Vertical,
        }),
        ArtworkColumnMode::AlwaysVerticalStretched => {
            let fit = match theme::artwork_column_stretch_fit() {
                ArtworkStretchFit::Cover => ContentFit::Cover,
                ArtworkStretchFit::Fill => ContentFit::Fill,
            };
            Some(ArtworkLayout {
                extent: always_vertical_extent(config.window_height),
                panel_kind: PanelKind::Stretched { fit },
                orientation: ArtworkOrientation::Vertical,
            })
        }
    }
}

/// Resolve the artwork-row extent for the Always-Vertical* modes from the
/// window height and the user's `artwork_vertical_height_pct` atomic.
///
/// Clamps into `[1.0, min(ARTWORK_MAX_SIZE, window_height - bottom_pad - MIN_SLOT_LIST_HEIGHT)]`
/// so the slot list always retains at least `MIN_SLOT_LIST_HEIGHT` pixels
/// beneath the artwork — the user opted into vertical, but the slot list
/// can never disappear entirely. Letterboxing of the panel itself (when
/// `extent > inset_width` or `< inset_width`) is intentional in always
/// vertical modes; the user gave up the no-letterbox guarantee in
/// exchange for a fixed-height artwork.
fn always_vertical_extent(window_height: f32) -> f32 {
    let raw = window_height * theme::artwork_vertical_height_pct();
    let upper = ARTWORK_MAX_SIZE
        .min((window_height - VERTICAL_ARTWORK_BOTTOM_PAD - MIN_SLOT_LIST_HEIGHT).max(1.0));
    raw.clamp(1.0, upper)
}

/// Extra slot-list chrome consumed when the artwork is vertically stacked
/// above the list. Slot-list row math (`SlotListConfig::with_dynamic_slots`)
/// works in absolute pixels — without this adjustment, slot rows would render
/// too tall and overflow behind the vertical artwork. Returns 0 in every
/// other configuration (Horizontal, hidden, `show_artwork_column = false`).
///
/// In `AlwaysVerticalNative` / `AlwaysVerticalStretched` modes the vertical
/// drag handle sits between the artwork and the slot list, so its 6 px
/// thickness is added on top of `extent + bottom_pad`.
pub(crate) fn vertical_artwork_chrome(config: &BaseSlotListLayoutConfig) -> f32 {
    match resolve_artwork_layout(config) {
        Some(layout) if layout.orientation == ArtworkOrientation::Vertical => {
            let handle = if matches!(
                theme::artwork_column_mode(),
                ArtworkColumnMode::AlwaysVerticalNative
                    | ArtworkColumnMode::AlwaysVerticalStretched
            ) {
                crate::widgets::artwork_split_handle_vertical::HANDLE_HEIGHT
            } else {
                0.0
            };
            layout.extent + VERTICAL_ARTWORK_BOTTOM_PAD + handle
        }
        _ => 0.0,
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
/// - `Auto` / `AlwaysNative` / `AlwaysVerticalNative` → responsive
///   `Length::Shrink` resolving to a `min(w, h)` square (preserves the
///   original layout exactly: column shrinks to the square's natural size,
///   no horizontal letterbox gap).
/// - `AlwaysStretched` / `AlwaysVerticalStretched` → responsive `Length::Fill`
///   with image filling the parent rect via the configured `ContentFit`.
pub(crate) fn single_artwork_panel<'a, Message: 'a>(
    artwork_handle: Option<&'a iced::widget::image::Handle>,
) -> Element<'a, Message> {
    if matches!(
        theme::artwork_column_mode(),
        ArtworkColumnMode::AlwaysStretched | ArtworkColumnMode::AlwaysVerticalStretched
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
        fn(crate::widgets::artwork_split_handle_vertical::DragEvent) -> Message,
    >(
        config,
        header,
        slot_list_content,
        artwork_content,
        None,
        None,
    )
}

/// Variant of [`base_slot_list_layout`] that also renders drag handles
/// alongside the artwork. The horizontal handle (`on_drag`) draws in the
/// `AlwaysNative` / `AlwaysStretched` modes; the vertical handle
/// (`on_drag_vertical`) draws in the `AlwaysVerticalNative` /
/// `AlwaysVerticalStretched` modes. Auto modes — including the
/// portrait-fallback vertical layout — suppress both handles since the
/// resolver picks the extent itself.
pub(crate) fn base_slot_list_layout_with_handle<'a, Message, F, G>(
    config: &BaseSlotListLayoutConfig,
    header: Element<'a, Message>,
    slot_list_content: Element<'a, Message>,
    artwork_content: Option<Element<'a, Message>>,
    on_drag: Option<F>,
    on_drag_vertical: Option<G>,
) -> Element<'a, Message>
where
    Message: 'a,
    F: Fn(crate::widgets::artwork_split_handle::DragEvent) -> Message + Clone + 'a,
    G: Fn(crate::widgets::artwork_split_handle_vertical::DragEvent) -> Message + Clone + 'a,
{
    let layout = resolve_artwork_layout(config);

    if let (Some(layout), Some(artwork)) = (layout, artwork_content) {
        match layout.orientation {
            ArtworkOrientation::Horizontal => {
                // Always-Vertical* modes never resolve to Horizontal; the
                // vertical drag callback is intentionally dropped.
                let _ = on_drag_vertical;
                horizontal_layout(config, layout, header, slot_list_content, artwork, on_drag)
            }
            ArtworkOrientation::Vertical => {
                // Auto-mode portrait fallback never opts into the horizontal
                // drag handle; it's a property of Always-Horizontal modes.
                let _ = on_drag;
                vertical_layout(
                    config,
                    layout,
                    header,
                    slot_list_content,
                    artwork,
                    on_drag_vertical,
                )
            }
        }
    } else {
        // Single column: just slot list
        column![header, slot_list_content]
            .width(Length::Fill)
            .height(Length::Fill)
            .spacing(0)
            .into()
    }
}

/// Original horizontal layout — artwork in a right-hand column, drag handle
/// available in Always modes.
fn horizontal_layout<'a, Message, F>(
    config: &BaseSlotListLayoutConfig,
    layout: ArtworkLayout,
    header: Element<'a, Message>,
    slot_list_content: Element<'a, Message>,
    artwork: Element<'a, Message>,
    on_drag: Option<F>,
) -> Element<'a, Message>
where
    Message: 'a,
    F: Fn(crate::widgets::artwork_split_handle::DragEvent) -> Message + Clone + 'a,
{
    let mode = theme::artwork_column_mode();
    let is_always = matches!(
        mode,
        ArtworkColumnMode::AlwaysNative | ArtworkColumnMode::AlwaysStretched
    );

    // In Auto mode, pass the artwork through directly so the row sizes
    // itself to the panel's natural square (the panel's responsive
    // returns a Fixed-size square via Length::Shrink). In always modes,
    // wrap with Length::Fixed(extent) so the user-tuned width is
    // authoritative — the panel inside will square or stretch to fit.
    let artwork_side_inner: Element<'a, Message> = if is_always {
        container(artwork)
            .width(Length::Fixed(layout.extent))
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
                .width(Length::Fixed(HORIZONTAL_ARTWORK_STRIPE))
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
}

/// Vertical layout — artwork stacked above the slot list, inset by
/// `VERTICAL_ARTWORK_SIDE_PAD` left and right so its edges line up with the
/// slot rows, and `VERTICAL_ARTWORK_BOTTOM_PAD` below to create a clean gap
/// before the list starts. Top padding stays 0 because the view header above
/// already provides vertical breathing room.
///
/// Used by both the Auto-mode portrait fallback (no drag handle) and the
/// `AlwaysVerticalNative` / `AlwaysVerticalStretched` modes (with a
/// horizontal-bar drag handle below the artwork).
fn vertical_layout<'a, Message, G>(
    config: &BaseSlotListLayoutConfig,
    layout: ArtworkLayout,
    header: Element<'a, Message>,
    slot_list_content: Element<'a, Message>,
    artwork: Element<'a, Message>,
    on_drag_vertical: Option<G>,
) -> Element<'a, Message>
where
    Message: 'a,
    G: Fn(crate::widgets::artwork_split_handle_vertical::DragEvent) -> Message + Clone + 'a,
{
    // Inner panel for the artwork. In `Square` panel kinds the responsive
    // widget inside resolves to a `min(w, h)` square; in `Stretched` it
    // fills the rect via ContentFit. The vertical Auto-mode resolver
    // guarantees `extent == inset_width` so Square fills without
    // letterboxing; Always-Vertical* modes accept letterboxing as the
    // user-opted-in tradeoff for a fixed-height artwork.
    let artwork_panel = container(artwork)
        .width(Length::Fill)
        .height(Length::Fixed(layout.extent))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .style(|_| container::Style {
            background: Some(artwork_outer_bg().into()),
            ..Default::default()
        });

    // Drag handle only in Always-Vertical* modes — Auto's vertical fallback
    // sizes the artwork itself and suppresses the handle.
    let mode = theme::artwork_column_mode();
    let is_always_vertical = matches!(
        mode,
        ArtworkColumnMode::AlwaysVerticalNative | ArtworkColumnMode::AlwaysVerticalStretched
    );
    let handle: Option<Element<'a, Message>> = if is_always_vertical {
        on_drag_vertical.map(|f| {
            crate::widgets::artwork_split_handle_vertical::artwork_split_handle_vertical_element(
                config.window_height,
                f,
            )
        })
    } else {
        None
    };
    let handle_height = if handle.is_some() {
        crate::widgets::artwork_split_handle_vertical::HANDLE_HEIGHT
    } else {
        0.0
    };

    // Artwork + optional drag handle inside the same inset wrapper so the
    // handle aligns with the slot rows' L/R padding.
    let inset_inner: Element<'a, Message> = if let Some(h) = handle {
        column![artwork_panel, h]
            .width(Length::Fill)
            .spacing(0)
            .into()
    } else {
        artwork_panel.into()
    };

    // Outer wrapper that paints the slot-list `bg0_hard` background in the
    // left/right/bottom inset, so the artwork sits inside a margin that
    // visually matches the slot rows' inset.
    let artwork_side = container(inset_inner)
        .width(Length::Fill)
        .padding(iced::Padding {
            top: 0.0,
            right: VERTICAL_ARTWORK_SIDE_PAD,
            bottom: VERTICAL_ARTWORK_BOTTOM_PAD,
            left: VERTICAL_ARTWORK_SIDE_PAD,
        })
        .style(theme::container_bg0_hard);

    // Pin the slot-list rect to a Fixed height that matches what
    // `SlotListConfig::with_dynamic_slots` budgeted for. The view passes
    // its slot-list chrome via `config.slot_list_chrome`; subtracting that
    // plus the artwork side (`extent + bottom_pad + optional handle`) from
    // `window_height` yields the exact slot-list rect the slot-count math
    // expects. Using `Fill` here lets iced's flex layout drift by a few
    // pixels and produce a partial slot at the bottom — Fixed locks the
    // rect to the slot math.
    let slot_list_height = (config.window_height
        - config.slot_list_chrome
        - layout.extent
        - VERTICAL_ARTWORK_BOTTOM_PAD
        - handle_height)
        .max(0.0);

    let slot_list_pinned = container(slot_list_content)
        .width(Length::Fill)
        .height(Length::Fixed(slot_list_height));

    column![header, artwork_side, slot_list_pinned]
        .width(Length::Fill)
        .height(Length::Fill)
        .spacing(0)
        .into()
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
            // Tests for `resolve_artwork_layout` / `vertical_artwork_chrome`
            // don't care about the slot-list rect — they exercise the
            // artwork-resolution math only.
            slot_list_chrome: 0.0,
        }
    }

    fn reset_atomics() {
        theme::set_artwork_column_mode(ArtworkColumnMode::Auto);
        theme::set_artwork_column_stretch_fit(ArtworkStretchFit::Cover);
        theme::set_artwork_column_width_pct(0.40);
        theme::set_artwork_auto_max_pct(0.40);
        theme::set_artwork_vertical_height_pct(0.40);
    }

    #[test]
    fn auto_landscape_window_shows_square() {
        let _g = lock_atomics();
        reset_atomics();
        // Wide landscape: 1920 wide → leftover = 1920 - min(768, 1080) = 1152 ≥ 800
        let l = resolve_artwork_layout(&cfg(1920.0, 1080.0, true)).expect("should show");
        assert!(matches!(l.panel_kind, PanelKind::Square));
        assert_eq!(l.orientation, ArtworkOrientation::Horizontal);
        assert!(l.extent > 0.0);
    }

    #[test]
    fn auto_narrow_landscape_window_hides() {
        let _g = lock_atomics();
        reset_atomics();
        // 1100 × 800 → leftover width too small AND not portrait → hide
        assert!(resolve_artwork_layout(&cfg(1100.0, 800.0, true)).is_none());
    }

    #[test]
    fn auto_tall_skinny_window_returns_vertical_inset_to_slot_list_padding() {
        let _g = lock_atomics();
        reset_atomics();
        // Very tall + skinny window: 530 × 1430. Inset width = 530 - 20 = 510.
        // height × 0.40 = 572 ≥ 510 so the artwork fills the inset width with
        // no letterbox. extent = min(510, 1000) = 510. Leftover height
        // 1430 - 510 - 10 = 910 ≥ 400.
        let l = resolve_artwork_layout(&cfg(530.0, 1430.0, true)).expect("should show");
        assert_eq!(l.orientation, ArtworkOrientation::Vertical);
        assert!(matches!(l.panel_kind, PanelKind::Square));
        assert!((l.extent - 510.0).abs() < 1e-3);
    }

    #[test]
    fn auto_portrait_hides_when_artwork_would_letterbox() {
        let _g = lock_atomics();
        reset_atomics();
        // 766 × 1370 is portrait but height × 0.40 = 548 < inset width
        // (766 - 20 = 746), so the height-based square wouldn't fill the
        // inset — the panel would show `bg0_soft` bars on the sides inside
        // the inset. Hide instead of showing the letterboxed panel.
        assert!(resolve_artwork_layout(&cfg(766.0, 1370.0, true)).is_none());
    }

    #[test]
    fn auto_portrait_hides_when_list_too_short() {
        let _g = lock_atomics();
        reset_atomics();
        // 220 × 500: passes the letterbox check (500 × 0.40 = 200 ≥ inset
        // 220 - 20 = 200) but leftover height 500 - 200 - 10 = 290 <
        // MIN_SLOT_LIST_HEIGHT (400) → hide.
        assert!(resolve_artwork_layout(&cfg(220.0, 500.0, true)).is_none());
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
        assert!((l.extent - 300.0).abs() < 1e-3);
        assert!(matches!(l.panel_kind, PanelKind::Square));
        assert_eq!(l.orientation, ArtworkOrientation::Horizontal);
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
        assert_eq!(l.orientation, ArtworkOrientation::Horizontal);
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
    fn always_modes_stay_horizontal_on_portrait() {
        let _g = lock_atomics();
        reset_atomics();
        // Same portrait dims as the Auto vertical test — Always modes ignore
        // the orientation fallback and keep the right-hand column.
        theme::set_artwork_column_mode(ArtworkColumnMode::AlwaysNative);
        let l = resolve_artwork_layout(&cfg(766.0, 1370.0, true)).expect("should show");
        assert_eq!(l.orientation, ArtworkOrientation::Horizontal);
        theme::set_artwork_column_mode(ArtworkColumnMode::AlwaysStretched);
        let l = resolve_artwork_layout(&cfg(766.0, 1370.0, true)).expect("should show");
        assert_eq!(l.orientation, ArtworkOrientation::Horizontal);
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
        assert!((l.extent - ARTWORK_MAX_SIZE).abs() < 1e-3);
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
        assert!((l.extent - 480.0).abs() < 1e-3);
        reset_atomics();
    }

    #[test]
    fn vertical_artwork_chrome_returns_zero_on_landscape() {
        let _g = lock_atomics();
        reset_atomics();
        // Landscape Auto resolves to Horizontal — no extra slot-list chrome.
        assert_eq!(vertical_artwork_chrome(&cfg(1920.0, 1080.0, true)), 0.0);
    }

    #[test]
    fn vertical_artwork_chrome_returns_extent_plus_bottom_pad_on_tall_skinny() {
        let _g = lock_atomics();
        reset_atomics();
        // Tall-skinny Auto resolves to Vertical with extent = inset width.
        // Chrome = extent + VERTICAL_ARTWORK_BOTTOM_PAD.
        let chrome = vertical_artwork_chrome(&cfg(530.0, 1430.0, true));
        assert!((chrome - (510.0 + VERTICAL_ARTWORK_BOTTOM_PAD)).abs() < 1e-3);
    }

    #[test]
    fn vertical_artwork_chrome_returns_zero_when_portrait_would_letterbox() {
        let _g = lock_atomics();
        reset_atomics();
        // 766 × 1370 is portrait but not tall-skinny enough to fill the
        // window width — the helper must return 0 so the slot list doesn't
        // budget chrome for an artwork that isn't going to render.
        assert_eq!(vertical_artwork_chrome(&cfg(766.0, 1370.0, true)), 0.0);
    }

    #[test]
    fn vertical_artwork_chrome_zero_when_hidden() {
        let _g = lock_atomics();
        reset_atomics();
        // show_artwork_column = false: no chrome regardless of dims.
        assert_eq!(vertical_artwork_chrome(&cfg(530.0, 1430.0, false)), 0.0);
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

    #[test]
    fn theme_atomic_clamps_auto_max_pct_into_safe_range() {
        let _g = lock_atomics();
        reset_atomics();
        // Out of range — atomic clamps to [0.30, 0.70].
        theme::set_artwork_auto_max_pct(0.001);
        assert!((theme::artwork_auto_max_pct() - 0.30).abs() < 1e-6);
        theme::set_artwork_auto_max_pct(0.99);
        assert!((theme::artwork_auto_max_pct() - 0.70).abs() < 1e-6);
        reset_atomics();
    }

    #[test]
    fn always_vertical_native_returns_vertical_square_on_landscape() {
        let _g = lock_atomics();
        reset_atomics();
        theme::set_artwork_column_mode(ArtworkColumnMode::AlwaysVerticalNative);
        let l = resolve_artwork_layout(&cfg(1920.0, 1080.0, true)).expect("should show");
        assert_eq!(l.orientation, ArtworkOrientation::Vertical);
        assert!(matches!(l.panel_kind, PanelKind::Square));
        // 1080 × 0.40 = 432; upper = min(MAX_SIZE, 1080 - 10 - 400) = min(1000, 670) = 670.
        assert!((l.extent - 432.0).abs() < 1e-3);
        reset_atomics();
    }

    #[test]
    fn always_vertical_native_stays_vertical_on_portrait() {
        let _g = lock_atomics();
        reset_atomics();
        // Same portrait dims that Auto-mode would reject for letterbox —
        // Always-Vertical* accepts letterboxing as the opt-in tradeoff.
        theme::set_artwork_column_mode(ArtworkColumnMode::AlwaysVerticalNative);
        let l = resolve_artwork_layout(&cfg(766.0, 1370.0, true)).expect("should show");
        assert_eq!(l.orientation, ArtworkOrientation::Vertical);
        // 1370 × 0.40 = 548; upper = min(1000, 1370 - 10 - 400) = min(1000, 960) = 960.
        assert!((l.extent - 548.0).abs() < 1e-3);
        reset_atomics();
    }

    #[test]
    fn always_vertical_stretched_returns_vertical_stretched_kind() {
        let _g = lock_atomics();
        reset_atomics();
        theme::set_artwork_column_mode(ArtworkColumnMode::AlwaysVerticalStretched);
        theme::set_artwork_column_stretch_fit(ArtworkStretchFit::Fill);
        let l = resolve_artwork_layout(&cfg(1920.0, 1080.0, true)).expect("should show");
        assert_eq!(l.orientation, ArtworkOrientation::Vertical);
        assert!(matches!(
            l.panel_kind,
            PanelKind::Stretched {
                fit: ContentFit::Fill
            }
        ));
        reset_atomics();
    }

    #[test]
    fn always_vertical_extent_tracks_user_pct() {
        let _g = lock_atomics();
        reset_atomics();
        theme::set_artwork_column_mode(ArtworkColumnMode::AlwaysVerticalNative);
        theme::set_artwork_vertical_height_pct(0.40);
        let l = resolve_artwork_layout(&cfg(1920.0, 1500.0, true)).expect("should show");
        assert!((l.extent - 600.0).abs() < 1e-3); // 1500 × 0.40
        theme::set_artwork_vertical_height_pct(0.60);
        let l = resolve_artwork_layout(&cfg(1920.0, 1500.0, true)).expect("should show");
        assert!((l.extent - 900.0).abs() < 1e-3); // 1500 × 0.60
        reset_atomics();
    }

    #[test]
    fn always_vertical_extent_clamped_to_preserve_slot_list_floor() {
        let _g = lock_atomics();
        reset_atomics();
        theme::set_artwork_column_mode(ArtworkColumnMode::AlwaysVerticalNative);
        theme::set_artwork_vertical_height_pct(0.80);
        // 1000 × 0.80 = 800. Upper = min(1000, 1000 - 10 - 400) = 590.
        // Result clamps to 590 so the slot list keeps 400 px.
        let l = resolve_artwork_layout(&cfg(800.0, 1000.0, true)).expect("should show");
        assert!((l.extent - 590.0).abs() < 1e-3);
        reset_atomics();
    }

    #[test]
    fn always_vertical_chrome_includes_handle_height() {
        let _g = lock_atomics();
        reset_atomics();
        theme::set_artwork_column_mode(ArtworkColumnMode::AlwaysVerticalNative);
        theme::set_artwork_vertical_height_pct(0.40);
        // 1500 × 0.40 = 600; chrome = 600 + bottom_pad + handle_height.
        let chrome = vertical_artwork_chrome(&cfg(1920.0, 1500.0, true));
        let expected = 600.0
            + VERTICAL_ARTWORK_BOTTOM_PAD
            + crate::widgets::artwork_split_handle_vertical::HANDLE_HEIGHT;
        assert!((chrome - expected).abs() < 1e-3);
        reset_atomics();
    }

    #[test]
    fn theme_atomic_clamps_vertical_height_pct_into_safe_range() {
        let _g = lock_atomics();
        reset_atomics();
        theme::set_artwork_vertical_height_pct(0.001);
        assert!((theme::artwork_vertical_height_pct() - 0.10).abs() < 1e-6);
        theme::set_artwork_vertical_height_pct(0.99);
        assert!((theme::artwork_vertical_height_pct() - 0.80).abs() < 1e-6);
        reset_atomics();
    }

    #[test]
    fn auto_vertical_fallback_threshold_tracks_user_max_pct() {
        let _g = lock_atomics();
        reset_atomics();
        // 766 × 1370 at default 0.40 → height × pct = 548 < inset width
        // (766 - 20 = 746), so the resolver hides (letterbox guard).
        assert!(resolve_artwork_layout(&cfg(766.0, 1370.0, true)).is_none());
        // Bumping the user pct to 0.70 → height × pct = 959 ≥ inset 746, so
        // the vertical candidate now fills the inset and the panel shows.
        theme::set_artwork_auto_max_pct(0.70);
        let l = resolve_artwork_layout(&cfg(766.0, 1370.0, true)).expect("should show");
        assert_eq!(l.orientation, ArtworkOrientation::Vertical);
        assert!((l.extent - 746.0).abs() < 1e-3);
        reset_atomics();
    }
}
