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

/// Wrap an artwork panel element with a 1 px `border()` hairline on its left
/// edge, visually separating it from the slot list column in Horizontal mode.
fn with_left_stripe<'a, Message: 'a>(artwork: Element<'a, Message>) -> Element<'a, Message> {
    let stripe = container(iced::widget::Space::new())
        .width(Length::Fixed(HORIZONTAL_ARTWORK_STRIPE))
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(theme::border().into()),
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

/// Thickness of the `border()` hairline on the left edge of the artwork column
/// in Horizontal orientation. Vertical orientation runs flush to whatever
/// chrome sits above it, with no extra inset.
pub(crate) const HORIZONTAL_ARTWORK_STRIPE: f32 = 1.0;

/// Configuration for base slot list layout
#[derive(Debug, Clone)]
pub(crate) struct BaseSlotListLayoutConfig {
    /// Content-pane width — the horizontal extent the view's widgets
    /// actually get to fill. In Top / None nav layouts this equals the
    /// window width; in Side nav it's already had the sidebar footprint
    /// (33 px flat / 41 px rounded) subtracted upstream
    /// (`Nokkvi::content_pane_width` in `app_view.rs`).
    /// Split-view callers multiply the pane width by their split fraction
    /// before passing it in. The resolver, `always_column_width`, and the
    /// drag handle all treat this as the available horizontal budget — if
    /// a caller passes the raw window width, the Auto-mode vertical
    /// fallback over-sizes the artwork square and the panel letterboxes
    /// inside the pane.
    pub window_width: f32,
    /// Content-pane height — the vertical extent the view's widgets get.
    /// Top nav steals height (subtracted upstream); Side / None do not.
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
    /// Whether artwork-elevation is in effect for this frame. Set by
    /// `home_view` (the only caller that resolves elevation) and threaded
    /// through every `*ViewData` so each view forwards it into the config it
    /// builds. `horizontal_layout` reads this to push the slot-list column
    /// down by `theme::nav_bar_height()` so the overlaid nav bar lands on
    /// an empty band. Always `false` in side-nav / none-nav layouts and in
    /// the internal probe configs that `Nokkvi::elevated_artwork_extent`
    /// uses before elevation is decided.
    pub elevated: bool,
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
            // windows: mirror the formula on the other axis. The artwork
            // runs edge-to-edge (matching the new slot-row geometry), so
            // the available width is just `window_width`. Only triggers
            // when the height-based square is at least that wide —
            // otherwise the panel would show `bg0_soft` letterbox bars,
            // which looks awkward; hide instead.
            if config.window_height > config.window_width {
                let inset_width = config.window_width.max(0.0);
                let v_square_uncapped = (config.window_height * auto_max_pct).min(ARTWORK_MAX_SIZE);

                if v_square_uncapped >= inset_width {
                    let v_square = inset_width.min(ARTWORK_MAX_SIZE);

                    if config.window_height - v_square >= MIN_SLOT_LIST_HEIGHT {
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
    let upper = ARTWORK_MAX_SIZE.min((window_height - MIN_SLOT_LIST_HEIGHT).max(1.0));
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
/// thickness is added on top of `extent`.
pub(crate) fn vertical_artwork_chrome(config: &BaseSlotListLayoutConfig) -> f32 {
    // Spell every arm out so a future `ArtworkOrientation` variant forces a
    // compile error here — matches the workspace's
    // `match_wildcard_for_single_variants = "deny"` discipline.
    match resolve_artwork_layout(config) {
        None
        | Some(ArtworkLayout {
            orientation: ArtworkOrientation::Horizontal,
            ..
        }) => 0.0,
        Some(
            layout @ ArtworkLayout {
                orientation: ArtworkOrientation::Vertical,
                ..
            },
        ) => {
            let handle = if theme::artwork_column_mode().is_vertical() {
                crate::widgets::artwork_split_handle::HANDLE_THICKNESS
            } else {
                0.0
            };
            layout.extent + handle
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
        })
        .width(Length::Shrink)
        .height(Length::Shrink)
        .into(),
    )
}

/// What an artwork panel draws when it has no image handle. Lets the panel keep
/// its mode-aware sizing (and any over-cover visualizer) while customizing the
/// art-less fill, so views don't hand-roll a parallel placeholder panel.
#[derive(Clone, Copy, Default)]
pub(crate) enum ArtworkPlaceholder {
    /// Empty `artwork_outer_bg` square — the default for albums/songs/etc.
    #[default]
    Blank,
    /// Centered radio-tower glyph on the artwork background — Radios stations
    /// with no logo / not-yet-loaded now-playing art.
    RadioTower,
}

impl ArtworkPlaceholder {
    /// The art-less cover content at the given panel size. `Blank` is an empty
    /// styled square; `RadioTower` centers the tower glyph on it.
    fn content<'a, Message: 'a>(self, width: Length, height: Length) -> Element<'a, Message> {
        use iced::widget::{container, text};
        let base = container::<Message, _, _>(match self {
            ArtworkPlaceholder::Blank => Element::from(text("")),
            ArtworkPlaceholder::RadioTower => crate::embedded_svg::svg_widget(
                crate::widgets::track_info_strip::RADIO_TOWER_ICON_PATH,
            )
            .width(Length::Fixed(96.0))
            .height(Length::Fixed(96.0))
            .style(|_, _| iced::widget::svg::Style {
                color: Some(theme::fg2()),
            })
            .into(),
        })
        .width(width)
        .height(height)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .style(|_theme| container::Style {
            background: Some(artwork_outer_bg().into()),
            ..Default::default()
        });
        base.into()
    }
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
    single_artwork_panel_inner(artwork_handle, None, None, ArtworkPlaceholder::Blank)
}

/// Surfing-boat overlay for the over-cover Lines visualizer. Carries a borrow of
/// the live [`BoatState`](crate::widgets::boat::BoatState) plus the visual params
/// the boat needs (global visualizer `opacity` + the Lines `mirror` flag).
/// `Some` only when Lines is drawn over the cover and the boat is visible; the
/// physics ticks in `update::boat` regardless of placement, so the position is
/// already live. The boat is inert (emits no messages, event-transparent), so it
/// composes under the artwork context menu exactly like the visualizer ring.
#[derive(Clone, Copy)]
pub(crate) struct OverCoverBoat<'a> {
    pub state: &'a crate::widgets::boat::BoatState,
    pub opacity: f32,
    pub mirror: bool,
}

/// Like [`single_artwork_panel`], but stacks the active visualizer over the
/// cover when `over_art` is `Some`. The tuple carries the cloned [`Visualizer`]
/// plus which widget mode to draw (`Scope` ring, `Bars`, or `Lines`) — the
/// over-cover placement option for Bars/Lines reuses this same slot the Scope
/// ring uses. The visualizer is event-transparent (its `shader::Program::update`
/// only requests redraws), so a wrapping context menu still receives the
/// artwork right-click.
///
/// In the square (Auto / Native) layouts the visualizer fills the square cover.
/// In the stretched (non-square) layouts the cover fills the column edge to
/// edge and the visualizer fills the whole panel too. The Scope ring sizes off
/// `min(w, h)` so it stays a true centered circle regardless of aspect, while
/// the particle dust fades out at the real panel edges instead of being clipped
/// at a sub-square boundary. Bars/Lines are bottom-anchored (they map to the
/// full rectangle); Bars additionally recompute their bar layout against the
/// panel width so they fit the column rather than the window.
///
/// When `boat` is `Some` (Lines over the cover, boat visible) the surfing boat
/// is stacked above the ring, riding the over-cover wave just as it rides the
/// bottom band.
///
/// The `f32` in `over_art` is the Visualizer Height fraction: Bars/Lines occupy
/// that fraction of the cover height, bottom-anchored (so cover art stays visible
/// above), matching the bottom-band height knob. Scope ignores it — its ring is
/// centered and sized by `scope.radius`, filling the panel.
///
/// [`Visualizer`]: crate::widgets::visualizer::Visualizer
fn single_artwork_panel_inner<'a, Message: 'a>(
    artwork_handle: Option<&'a iced::widget::image::Handle>,
    over_art: Option<(
        crate::widgets::visualizer::Visualizer,
        crate::widgets::visualizer::VisualizationMode,
        f32,
    )>,
    boat: Option<OverCoverBoat<'a>>,
    placeholder: ArtworkPlaceholder,
) -> Element<'a, Message> {
    if theme::artwork_column_mode().is_stretched() {
        let fit = match theme::artwork_column_stretch_fit() {
            ArtworkStretchFit::Cover => ContentFit::Cover,
            ArtworkStretchFit::Fill => ContentFit::Fill,
        };
        return iced::widget::responsive(move |size| {
            use iced::widget::{container, image, stack};

            let content: Element<'_, Message> = if let Some(handle) = artwork_handle {
                image(handle.clone())
                    .content_fit(fit)
                    .width(Length::Fixed(size.width))
                    .height(Length::Fixed(size.height))
                    .into()
            } else {
                placeholder.content(Length::Fixed(size.width), Length::Fixed(size.height))
            };

            let cover = container(content)
                .width(Length::Fixed(size.width))
                .height(Length::Fixed(size.height))
                .style(|_theme| container::Style {
                    background: Some(artwork_outer_bg().into()),
                    ..Default::default()
                });

            // Overlay the active visualizer across the FULL (non-square) cover,
            // not a centered square. For Scope the shader sizes the ring off
            // `min(w, h)` in pixel space, so it stays a true circle centered in
            // the panel regardless of aspect; letting the visualizer fill the
            // whole rect means the particle dust fades out at the real panel
            // edges instead of being hard-clipped (scissored) at a sub-square
            // boundary. Bars/Lines instead honor the Visualizer Height setting:
            // they occupy `height_percent` of the cover height, bottom-anchored
            // (cover art shows above) — the same knob the bottom band uses. Bars
            // also recompute their bar layout from the panel width.
            let panel: Element<'_, Message> = if let Some((viz, mode, height_percent)) = &over_art {
                let is_scope = *mode == crate::widgets::visualizer::VisualizationMode::Scope;
                let band_h = if is_scope {
                    size.height
                } else {
                    (size.height * *height_percent).clamp(0.0, size.height)
                };
                let top_pad = (size.height - band_h).max(0.0);

                let mut configured = viz.clone().mode(*mode);
                // Bars recompute their bar layout from the actual panel width
                // (the bottom-band path feeds the window width, which over a
                // narrow column overflows and scissor-clips). Lines/Scope use a
                // fixed point count, so no width hint is needed.
                if *mode == crate::widgets::visualizer::VisualizationMode::Bars {
                    configured = configured.width(size.width);
                }
                let ring = configured.view::<Message>();
                let ring_layer: Element<'_, Message> = if is_scope {
                    container(ring)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .into()
                } else {
                    column![
                        container(iced::widget::Space::new()).height(Length::Fixed(top_pad)),
                        container(ring)
                            .width(Length::Fill)
                            .height(Length::Fixed(band_h)),
                    ]
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
                };
                let mut layers = stack![cover, ring_layer];
                // Surfing boat over the Lines wave, confined to the same bottom
                // band so it rides the rendered waveform. Inert + event-
                // transparent, so it never steals the artwork right-click.
                if let Some(b) = &boat
                    && *mode == crate::widgets::visualizer::VisualizationMode::Lines
                {
                    // Size the boat off min(width, band) so its width can't exceed
                    // a narrow band — keeps the over-cover wrap-margin constant
                    // valid; the band's bottom is the boat's waterline.
                    let boat_el = crate::widgets::boat::boat_overlay::<Message>(
                        b.state,
                        size.width,
                        band_h,
                        size.width.min(band_h),
                        b.opacity,
                        b.mirror,
                        // Lines over-cover boat keeps the drop-anchor doodad.
                        None,
                    );
                    let boat_layer = column![
                        container(iced::widget::Space::new()).height(Length::Fixed(top_pad)),
                        container(boat_el)
                            .width(Length::Fill)
                            .height(Length::Fixed(band_h)),
                    ]
                    .width(Length::Fill)
                    .height(Length::Fill);
                    layers = layers.push(boat_layer);
                }
                layers.into()
            } else {
                cover.into()
            };
            panel
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into();
    }

    // Square (Auto / AlwaysNative) — original behavior, plus the optional ring.
    iced::widget::responsive(move |size| {
        use iced::widget::{container, image, stack};

        let square_size = size.width.min(size.height).max(0.0);

        let content: Element<'_, Message> = if let Some(handle) = artwork_handle {
            image(handle.clone())
                .content_fit(ContentFit::Cover)
                .width(Length::Fixed(square_size))
                .height(Length::Fixed(square_size))
                .into()
        } else {
            placeholder.content(Length::Fixed(square_size), Length::Fixed(square_size))
        };

        let cover = container(content)
            .width(Length::Fixed(square_size))
            .height(Length::Fixed(square_size))
            .style(|_theme| container::Style {
                background: Some(artwork_outer_bg().into()),
                ..Default::default()
            });

        // Overlay the active visualizer over the cover square. Scope fills the
        // square (centered ring). Bars/Lines honor the Visualizer Height setting:
        // they occupy `height_percent` of the square, bottom-anchored (cover art
        // shows above) — the same knob the bottom band uses.
        let panel: Element<'_, Message> = if let Some((viz, mode, height_percent)) = &over_art {
            let is_scope = *mode == crate::widgets::visualizer::VisualizationMode::Scope;
            let band_h = if is_scope {
                square_size
            } else {
                (square_size * *height_percent).clamp(0.0, square_size)
            };
            let top_pad = (square_size - band_h).max(0.0);

            let mut configured = viz.clone().mode(*mode);
            if *mode == crate::widgets::visualizer::VisualizationMode::Bars {
                configured = configured.width(square_size);
            }
            let ring = configured.view::<Message>();
            let ring_layer: Element<'_, Message> = if is_scope {
                container(ring)
                    .width(Length::Fixed(square_size))
                    .height(Length::Fixed(square_size))
                    .into()
            } else {
                column![
                    container(iced::widget::Space::new()).height(Length::Fixed(top_pad)),
                    container(ring)
                        .width(Length::Fixed(square_size))
                        .height(Length::Fixed(band_h)),
                ]
                .width(Length::Fixed(square_size))
                .height(Length::Fixed(square_size))
                .into()
            };
            let mut layers = stack![cover, ring_layer];
            // Surfing boat over the Lines wave, confined to the same bottom band
            // so it rides the rendered waveform. Inert + transparent.
            if let Some(b) = &boat
                && *mode == crate::widgets::visualizer::VisualizationMode::Lines
            {
                // Sprite basis = min(square_size, band): on a short band the boat
                // sizes off the band height, keeping the wrap-margin constant valid.
                let boat_el = crate::widgets::boat::boat_overlay::<Message>(
                    b.state,
                    square_size,
                    band_h,
                    square_size.min(band_h),
                    b.opacity,
                    b.mirror,
                    // Lines over-cover boat keeps the drop-anchor doodad.
                    None,
                );
                let boat_layer = column![
                    container(iced::widget::Space::new()).height(Length::Fixed(top_pad)),
                    container(boat_el)
                        .width(Length::Fixed(square_size))
                        .height(Length::Fixed(band_h)),
                ]
                .width(Length::Fixed(square_size))
                .height(Length::Fixed(square_size));
                layers = layers.push(boat_layer);
            }
            layers.into()
        } else {
            cover.into()
        };
        panel
    })
    .width(Length::Shrink)
    .height(Length::Shrink)
    .into()
}

/// Wrap an artwork panel in its right-click menu when `entries` is non-empty;
/// return the bare panel otherwise (an empty list means "no menu on this
/// panel"). Shared implementation behind every
/// `*_artwork_panel_with_*` helper below — the menu is a controlled overlay
/// driven by the caller's `is_open` / `open_position` / `on_open_change` trio
/// (see `context_menu::artwork_panel_open_state`).
///
/// Each view's entries list is statically non-empty or statically empty per
/// render path, so the conditional wrap never flips the widget-tree shape
/// across renders (root-widget-stability rule); only the entry CONTENT varies
/// (e.g. a gated "Reset Artwork"), which is fine — the menu element is built
/// on open.
fn wrap_with_panel_menu<'a, Message: Clone + 'a>(
    panel: Element<'a, Message>,
    entries: Vec<crate::widgets::context_menu::PanelMenuEntry<Message>>,
    is_open: bool,
    open_position: Option<iced::Point>,
    on_open_change: impl Fn(Option<iced::Point>) -> Message + 'a,
) -> Element<'a, Message> {
    if entries.is_empty() {
        return panel;
    }
    crate::widgets::context_menu::context_menu(
        panel,
        entries,
        |entry, _length| entry.view(),
        is_open,
        open_position,
        on_open_change,
    )
    .into()
}

/// Artwork panel with the active visualizer overlaid on the cover (when
/// `over_art` is `Some`) plus a right-click menu built from `menu_entries`.
/// Used by the Queue now-playing panel for the over-cover visualizer placement
/// (the Scope ring always, and Bars/Lines when their placement is `OverCover`).
/// The tuple carries the cloned visualizer and which widget mode to draw; `boat`
/// adds the surfing-boat overlay on top when Lines rides over the cover.
/// `placeholder` controls the art-less fill (e.g. Radios passes `RadioTower` so
/// the tower glyph shows under the visualizer instead of a blank square).
#[allow(clippy::too_many_arguments)]
pub(crate) fn single_artwork_panel_with_visualizer_and_menu<'a, Message: Clone + 'a>(
    artwork_handle: Option<&'a iced::widget::image::Handle>,
    over_art: Option<(
        crate::widgets::visualizer::Visualizer,
        crate::widgets::visualizer::VisualizationMode,
        f32,
    )>,
    boat: Option<OverCoverBoat<'a>>,
    placeholder: ArtworkPlaceholder,
    menu_entries: Vec<crate::widgets::context_menu::PanelMenuEntry<Message>>,
    is_open: bool,
    open_position: Option<iced::Point>,
    on_open_change: impl Fn(Option<iced::Point>) -> Message + 'a,
) -> Element<'a, Message> {
    let panel = single_artwork_panel_inner(artwork_handle, over_art, boat, placeholder);
    wrap_with_panel_menu(panel, menu_entries, is_open, open_position, on_open_change)
}

/// Create a single-image artwork panel with an optional right-click context menu.
///
/// A non-empty `menu_entries` wraps the panel in a context menu; an empty
/// list is identical to [`single_artwork_panel`].
pub(crate) fn single_artwork_panel_with_menu<'a, Message: Clone + 'a>(
    artwork_handle: Option<&'a iced::widget::image::Handle>,
    menu_entries: Vec<crate::widgets::context_menu::PanelMenuEntry<Message>>,
    is_open: bool,
    open_position: Option<iced::Point>,
    on_open_change: impl Fn(Option<iced::Point>) -> Message + 'a,
) -> Element<'a, Message> {
    let panel = single_artwork_panel(artwork_handle);
    wrap_with_panel_menu(panel, menu_entries, is_open, open_position, on_open_change)
}

/// Wrap an existing artwork panel element with a bottom-anchored, full-width
/// bar overlay.
///
/// Shared implementation backing `single_artwork_panel_with_pill` and
/// `collage_artwork_panel_with_pill`. Builds the styled bar container with a
/// fixed `bg0_hard()` backdrop, sandwiches it between 1 px `theme::border()`
/// sibling rules (matching the view-header separator), and stacks the result
/// on top of `base_panel`. Corners are always flat — rounded mode is
/// intentionally ignored here so the overlay reads as a banded strip rather
/// than a floating pill.
pub(crate) fn wrap_with_pill_overlay<'a, Message: 'a>(
    base_panel: Element<'a, Message>,
    content: Element<'a, Message>,
) -> Element<'a, Message> {
    use iced::widget::{container, stack};

    let overlay = container(banded_pill(content))
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(iced::Alignment::End);

    stack![base_panel, overlay].into()
}

/// The banded pill strip itself — `content` centered on a fixed `bg0_hard()`
/// bar sandwiched between 1 px `theme::border()` rules. Shared by
/// [`wrap_with_pill_overlay`] (which floats it over an artwork panel) and the
/// Harbour Trawl scene (which DOCKS it below the sea in a column, so the
/// seabed lands on the band's top rail instead of hiding behind it). One
/// builder, so the two placements can't drift.
pub(crate) fn banded_pill<'a, Message: 'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    let separator = || {
        container(iced::widget::Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(1.0))
            .style(|_| container::Style {
                background: Some(theme::border().into()),
                ..Default::default()
            })
    };

    let bar = container(content)
        .width(Length::Fill)
        .padding(16)
        .align_x(iced::Alignment::Center)
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(crate::theme::bg0_hard())),
            ..Default::default()
        });

    column![separator(), bar, separator()].into()
}

/// Create a single-image artwork panel with a bottom-anchored, full-width bar overlay
pub(crate) fn single_artwork_panel_with_pill<'a, Message: Clone + 'a>(
    artwork_handle: Option<&'a iced::widget::image::Handle>,
    pill_content: Option<Element<'a, Message>>,
    menu_entries: Vec<crate::widgets::context_menu::PanelMenuEntry<Message>>,
    is_open: bool,
    open_position: Option<iced::Point>,
    on_open_change: impl Fn(Option<iced::Point>) -> Message + 'a,
) -> Element<'a, Message> {
    let base_panel = single_artwork_panel(artwork_handle);

    let panel = if let Some(content) = pill_content {
        wrap_with_pill_overlay(base_panel, content)
    } else {
        base_panel
    };

    wrap_with_panel_menu(panel, menu_entries, is_open, open_position, on_open_change)
}

/// Create a 3x3 collage artwork panel with a bottom-anchored, full-width bar
/// overlay and an optional right-click menu (same `menu_entries` plumbing as
/// its single-panel sibling; an empty list leaves the panel bare).
pub(crate) fn collage_artwork_panel_with_pill<'a, Message: Clone + 'a>(
    collage_handles: &'a [iced::widget::image::Handle],
    pill_content: Option<Element<'a, Message>>,
    menu_entries: Vec<crate::widgets::context_menu::PanelMenuEntry<Message>>,
    is_open: bool,
    open_position: Option<iced::Point>,
    on_open_change: impl Fn(Option<iced::Point>) -> Message + 'a,
) -> Element<'a, Message> {
    let base_panel = collage_artwork_panel(collage_handles);

    let panel = if let Some(content) = pill_content {
        wrap_with_pill_overlay(base_panel, content)
    } else {
        base_panel
    };

    wrap_with_panel_menu(panel, menu_entries, is_open, open_position, on_open_change)
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
    })
    .width(Length::Shrink)
    .height(Length::Shrink)
    .into()
}

/// Create a 2×2 quad artwork grid at a fixed edge size.
///
/// Small-thumbnail sibling of [`collage_artwork_panel`] for playlist/genre
/// slot rows and the queue's "Playing From" strip cover: the same seamless
/// zero-spacing grid with the same modulo wrap when fewer than 4 distinct
/// tiles exist (2 tiles → AB/AB, 3 → ABC/A). Handles are cloned into the
/// tree eagerly (the edge is known, so no `responsive` indirection), which
/// also lets callers pass short-lived borrows of the `album_art` snapshot.
///
/// `opacity` is applied per tile — slot rows forward their non-center fade,
/// the strip passes 1.0. Callers resolve tiles via
/// `services::collage_artwork::resolve_quad_handles` (≥2 tiles); an empty
/// slice degrades to the same blank `artwork_outer_bg` square the collage
/// panel renders.
pub(crate) fn quad_artwork_grid<'a, Message: 'a>(
    tile_handles: &[&iced::widget::image::Handle],
    edge: f32,
    opacity: f32,
) -> Element<'a, Message> {
    use iced::widget::{column as col, container, image, row as irow, text};

    if tile_handles.is_empty() {
        return container(text(""))
            .width(Length::Fixed(edge))
            .height(Length::Fixed(edge))
            .style(|_theme| container::Style {
                background: Some(artwork_outer_bg().into()),
                ..Default::default()
            })
            .into();
    }

    let num = tile_handles.len();
    let cell_size = edge / 2.0;

    let mut rows_vec: Vec<Element<'a, Message>> = Vec::new();
    for row_idx in 0..2 {
        let mut cells: Vec<Element<'a, Message>> = Vec::new();
        for col_idx in 0..2 {
            let h = tile_handles[(row_idx * 2 + col_idx) % num];
            cells.push(
                container(
                    image(h.clone())
                        .content_fit(ContentFit::Cover)
                        .width(Length::Fixed(cell_size))
                        .height(Length::Fixed(cell_size))
                        .opacity(opacity),
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
        .width(Length::Fixed(edge))
        .height(Length::Fixed(edge))
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
        fn(crate::widgets::artwork_split_handle::DragEvent) -> Message,
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
    G: Fn(crate::widgets::artwork_split_handle::DragEvent) -> Message + Clone + 'a,
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
        // Single column: just slot list. Wrap in an outer column so the root
        // widget type matches horizontal_layout / vertical_layout — switching
        // artwork-mode mid-session keeps text_input focus alive. See CLAUDE.md
        // "Render output" gotcha.
        let inner = column![header, slot_list_content]
            .width(Length::Fill)
            .height(Length::Fill)
            .spacing(0);

        column![inner]
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
    let is_always = theme::artwork_column_mode().is_always_horizontal();

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
            crate::widgets::artwork_split_handle::artwork_split_handle_horizontal_element(
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
                    background: Some(theme::border().into()),
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

    // In elevated mode the home view stretches main_content up over the
    // top-nav row and overlays the nav-bar back on top of the slot-list
    // column. Stack a transparent spacer matching the live nav-bar height
    // above the header so the overlaid nav-bar lands on an unoccupied
    // band rather than on top of the view header. The artwork column
    // intentionally has no top padding so it fills the row all the way to
    // the top of the window.
    //
    // Use `theme::nav_bar_height()` (32 flat / 44 rounded), not the
    // legacy `slot_list::NAV_BAR_HEIGHT` const (pinned at 32) — in
    // rounded mode the live nav is 44 px, so a 32 px spacer lets the
    // overlay eat the view-header pill's 12 px top margin and pushes the
    // pill flush against the bottom of the nav bar. Mirrors the
    // non-elevated fix in `app_view.rs::home_view`.
    //
    // `config.elevated` is plumbed by `home_view` through each view's
    // `*ViewData` — the only frame-level signal authoritative enough to
    // gate this branch. Reading a theme-only predicate would also fire in
    // split-view, where home_view does *not* elevate, leaving the slot
    // list with a stranded top gap.
    //
    // The spacer is intentionally a sibling `Space` rather than a wrapping
    // container with `padding(top: …)` — a wrapping container ends up
    // painting the row's residual right-side allocation with an unintended
    // grey rect when the artwork column sits next to it. A plain `Space`
    // sibling has no draw surface at all, so the band stays transparent.
    let spacer_height = if config.elevated {
        crate::theme::nav_bar_height()
    } else {
        0.0
    };
    let slot_list_column: Element<'a, Message> = column![
        iced::widget::Space::new()
            .width(Length::Fill)
            .height(Length::Fixed(spacer_height)),
        header,
        slot_list_content,
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .spacing(0)
    .into();

    // Wrap the inner row in an outer column so every base_slot_list_layout
    // branch (horizontal, vertical, no-artwork) returns the same root widget
    // type — switching artwork-mode mid-session keeps text_input focus alive.
    // See CLAUDE.md "Render output" gotcha.
    let inner = row![slot_list_column, artwork_side]
        .align_y(Alignment::Start)
        .spacing(0)
        .width(Length::Fill)
        .height(Length::Fill);

    column![inner]
        .width(Length::Fill)
        .height(Length::Fill)
        .spacing(0)
        .into()
}

/// Vertical layout — artwork stacked above the slot list, edge-to-edge L/R
/// (matching the slot rows' zero-inset geometry) and flush against whatever
/// chrome sits above it. The view header lives between the artwork and the
/// slot list so it stays adjacent to the list it controls; its own internal
/// top padding provides any gap below the artwork.
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
    G: Fn(crate::widgets::artwork_split_handle::DragEvent) -> Message + Clone + 'a,
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
    let is_always_vertical = theme::artwork_column_mode().is_vertical();
    let handle: Option<Element<'a, Message>> = if is_always_vertical {
        on_drag_vertical.map(|f| {
            crate::widgets::artwork_split_handle::artwork_split_handle_vertical_element(
                config.window_height,
                f,
            )
        })
    } else {
        None
    };
    let handle_height = if handle.is_some() {
        crate::widgets::artwork_split_handle::HANDLE_THICKNESS
    } else {
        0.0
    };

    // Artwork + optional drag handle, both running edge-to-edge horizontally
    // (matches the slot rows' zero-pad geometry) and flush against the chrome
    // above.
    let artwork_side: Element<'a, Message> = if let Some(h) = handle {
        column![artwork_panel, h]
            .width(Length::Fill)
            .spacing(0)
            .into()
    } else {
        artwork_panel.into()
    };

    // Pin the slot-list rect to a Fixed height that matches what
    // `SlotListConfig::with_dynamic_slots` budgeted for. The view passes
    // its slot-list chrome via `config.slot_list_chrome`; subtracting that
    // plus the artwork side (`extent + optional handle`) from `window_height`
    // yields the exact slot-list rect the slot-count math expects. Using
    // `Fill` here lets iced's flex layout drift by a few pixels and produce
    // a partial slot at the bottom — Fixed locks the rect to the slot math.
    let slot_list_height =
        (config.window_height - config.slot_list_chrome - layout.extent - handle_height).max(0.0);

    let slot_list_pinned = container(slot_list_content)
        .width(Length::Fill)
        .height(Length::Fixed(slot_list_height));

    // Wrap the inner column in an outer column so every base_slot_list_layout
    // branch returns the same root widget type — switching artwork-mode
    // mid-session keeps text_input focus alive. See CLAUDE.md "Render output"
    // gotcha.
    let inner = column![artwork_side, header, slot_list_pinned]
        .width(Length::Fill)
        .height(Length::Fill)
        .spacing(0);

    column![inner]
        .width(Length::Fill)
        .height(Length::Fill)
        .spacing(0)
        .into()
}

#[cfg(test)]
mod tests {

    use super::*;

    /// All tests in this module mutate the global theme atomics for artwork
    /// column mode/fit/width_pct. Acquire the crate-wide theme lock at the
    /// start of each test so they serialize against every other test family
    /// that touches the same atomics (`theme::tests`, the settings handler
    /// tests, the slot-count resync tests).
    fn lock_atomics() -> parking_lot::MutexGuard<'static, ()> {
        crate::theme::THEME_MODE_LOCK.lock()
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
            elevated: false,
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
        // Very tall + skinny window: 530 × 1430. The artwork runs edge-to-edge
        // (zero L/R inset), so inset_width = 530. height × 0.40 = 572 ≥ 530 so
        // the artwork fills the window width with no letterbox. extent =
        // min(530, 1000) = 530. Leftover height 1430 - 530 - 10 = 890 ≥ 400.
        let l = resolve_artwork_layout(&cfg(530.0, 1430.0, true)).expect("should show");
        assert_eq!(l.orientation, ArtworkOrientation::Vertical);
        assert!(matches!(l.panel_kind, PanelKind::Square));
        assert!((l.extent - 530.0).abs() < 1e-3);
    }

    #[test]
    fn auto_portrait_hides_when_artwork_would_letterbox() {
        let _g = lock_atomics();
        reset_atomics();
        // 766 × 1370 is portrait but height × 0.40 = 548 < inset width
        // (edge-to-edge L/R, so inset_width = 766), so the height-based
        // square wouldn't fill the window — the panel would show `bg0_soft`
        // bars on the sides. Hide instead of showing the letterboxed panel.
        assert!(resolve_artwork_layout(&cfg(766.0, 1370.0, true)).is_none());
    }

    #[test]
    fn auto_portrait_hides_when_list_too_short() {
        let _g = lock_atomics();
        reset_atomics();
        // 200 × 500: passes the letterbox check (500 × 0.40 = 200 ≥ inset
        // width 200, edge-to-edge L/R) but leftover height 500 - 200 - 10
        // = 290 < MIN_SLOT_LIST_HEIGHT (400) → hide.
        assert!(resolve_artwork_layout(&cfg(200.0, 500.0, true)).is_none());
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
    fn vertical_artwork_chrome_returns_extent_on_tall_skinny() {
        let _g = lock_atomics();
        reset_atomics();
        // Tall-skinny Auto resolves to Vertical with extent = inset width.
        // Edge-to-edge L/R, so inset width = window width = 530. Chrome =
        // extent (no top pad — the artwork sits flush against chrome above).
        let chrome = vertical_artwork_chrome(&cfg(530.0, 1430.0, true));
        assert!((chrome - 530.0).abs() < 1e-3);
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
        // 1080 × 0.40 = 432; upper = min(MAX_SIZE, 1080 - 400) = min(1000, 680) = 680.
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
        // 1370 × 0.40 = 548; upper = min(1000, 1370 - 400) = min(1000, 970) = 970.
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
        // 1000 × 0.80 = 800. Upper = min(1000, 1000 - 400) = 600.
        // Result clamps to 600 so the slot list keeps 400 px.
        let l = resolve_artwork_layout(&cfg(800.0, 1000.0, true)).expect("should show");
        assert!((l.extent - 600.0).abs() < 1e-3);
        reset_atomics();
    }

    #[test]
    fn always_vertical_chrome_includes_handle_height() {
        let _g = lock_atomics();
        reset_atomics();
        theme::set_artwork_column_mode(ArtworkColumnMode::AlwaysVerticalNative);
        theme::set_artwork_vertical_height_pct(0.40);
        // 1500 × 0.40 = 600; chrome = 600 + handle_height.
        let chrome = vertical_artwork_chrome(&cfg(1920.0, 1500.0, true));
        let expected = 600.0 + crate::widgets::artwork_split_handle::HANDLE_THICKNESS;
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
        // (edge-to-edge L/R, so inset_width = 766), so the resolver hides
        // (letterbox guard).
        assert!(resolve_artwork_layout(&cfg(766.0, 1370.0, true)).is_none());
        // Bumping the user pct to 0.70 → height × pct = 959 ≥ inset 766, so
        // the vertical candidate now fills the inset and the panel shows.
        theme::set_artwork_auto_max_pct(0.70);
        let l = resolve_artwork_layout(&cfg(766.0, 1370.0, true)).expect("should show");
        assert_eq!(l.orientation, ArtworkOrientation::Vertical);
        assert!((l.extent - 766.0).abs() < 1e-3);
        reset_atomics();
    }
}
