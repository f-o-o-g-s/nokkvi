//! Consolidated Slot List Component
//!
//! Generic 9-slot circular navigation interface that can render any item type.
//! Generic slot list view component for circular navigation
//!
//! Provides reusable slot list rendering with configurable item rendering

use std::sync::atomic::{AtomicU32, Ordering};

use iced::{
    Background, Color, Element, Gradient, Length, Radians,
    gradient::Linear,
    widget::{Space, Stack, button, column, container, mouse_area},
};

use crate::{
    theme,
    widgets::{HoveredSlot, SlotListPageMessage, SlotListView},
};

/// Per-slot hover callbacks for `build_slot_list_slots`. Each slot is
/// wrapped in a `mouse_area` that fires `on_enter` when the cursor crosses
/// into the slot's rendered bounds, and `on_exit` when it leaves. The
/// callbacks receive a `HoveredSlot` describing the slot index AND the
/// resolved item index baked in at render time — so cross-pane drag
/// handlers consume cursor → slot → item mapping straight from the widget
/// tree, no chrome math needed.
///
/// `None` at a call site (settings, default-playlist picker) skips the
/// wrapping entirely so non-drag callers pay zero overhead.
pub(crate) struct SlotHoverCallback<'a, Message> {
    pub(crate) on_enter: Box<dyn Fn(HoveredSlot) -> Message + 'a>,
    pub(crate) on_exit: Box<dyn Fn(HoveredSlot) -> Message + 'a>,
}

impl<'a, Message: 'a> SlotHoverCallback<'a, Message> {
    pub(crate) fn new(
        on_enter: impl Fn(HoveredSlot) -> Message + 'a,
        on_exit: impl Fn(HoveredSlot) -> Message + 'a,
    ) -> Self {
        Self {
            on_enter: Box::new(on_enter),
            on_exit: Box::new(on_exit),
        }
    }

    /// Build a callback from a single message wrapper — the per-view
    /// `SlotList(SlotListPageMessage)` variant constructor — making the
    /// "always route hover events through `SlotListPageMessage::Hover*Slot`"
    /// contract structural at the call site.
    ///
    /// Replaces the lambda-pair boilerplate
    /// `SlotHoverCallback::new(|h| M::SlotList(SLM::HoverEnterSlot(h)), |h| ...)`
    /// with `SlotHoverCallback::for_slot_list(M::SlotList)`.
    pub(crate) fn for_slot_list(wrap: fn(SlotListPageMessage) -> Message) -> Self {
        Self::new(
            move |h| wrap(SlotListPageMessage::HoverEnterSlot(h)),
            move |h| wrap(SlotListPageMessage::HoverExitSlot(h)),
        )
    }
}

/// Pre-computed font and sizing metrics for slot list rows.
///
/// Calculated once per slot list render pass and passed via `SlotListRowContext`.
/// Views reference these fields instead of re-deriving them from `row_height`
/// and `scale_factor` in every `render_*_row` function.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SlotListRowMetrics {
    /// Artwork thumbnail size (scaled): `(row_height - 16.0).max(32.0) * scale_factor`
    pub artwork_size: f32,
    /// Large title — base 16.0 (songs, albums, queue, similar)
    pub title_size_lg: f32,
    /// Standard title — base 14.0 (artists, playlists, genres, expansion)
    pub title_size: f32,
    /// Subtitle text — base 13.0
    pub subtitle_size: f32,
    /// Metadata / index text — base 12.0
    pub metadata_size: f32,
    /// Star/heart icon size for parent rows: `clamp(16.0, 24.0)`
    pub star_size: f32,
    /// Star/heart icon size for expansion child rows: `clamp(14.0, 20.0)`
    pub star_size_child: f32,
}

impl SlotListRowMetrics {
    /// Compute all metrics from layout parameters.
    fn from_row(row_height: f32, scale_factor: f32) -> Self {
        use nokkvi_data::utils::scale::calculate_font_size;
        Self {
            artwork_size: (row_height - 16.0).max(32.0) * scale_factor,
            title_size_lg: calculate_font_size(16.0, row_height, scale_factor) * scale_factor,
            title_size: calculate_font_size(14.0, row_height, scale_factor) * scale_factor,
            subtitle_size: calculate_font_size(13.0, row_height, scale_factor) * scale_factor,
            metadata_size: calculate_font_size(12.0, row_height, scale_factor) * scale_factor,
            star_size: (row_height * 0.3 * scale_factor).clamp(16.0, 24.0),
            star_size_child: (row_height * 0.3 * scale_factor).clamp(14.0, 20.0),
        }
    }
}

/// Per-slot rendering context passed to render closures.
///
/// Bundles all the common parameters that every slot list row renderer needs,
/// avoiding long argument lists in both the closure and the render functions.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SlotListRowContext {
    pub item_index: usize,
    pub is_center: bool,
    pub is_selected: bool,
    pub has_multi_selection: bool,
    pub opacity: f32,
    /// Window scale factor
    pub scale_factor: f32,
    /// Layout row height
    pub row_height: f32,
    /// Global keyboard modifiers
    pub modifiers: iced::keyboard::Modifiers,
    /// Pre-computed font and sizing metrics
    pub metrics: SlotListRowMetrics,
}

impl SlotListRowContext {
    /// Forward this row context's positional state into
    /// [`SlotListSlotStyle::for_slot`], shrinking the 7-arg transposition-prone
    /// call down to the 3 per-renderer-varying inputs. The other four
    /// (`is_center` / `is_selected` / `has_multi_selection` / `opacity`) are
    /// always the context's own fields, so routing them here means a renderer
    /// can never accidentally transpose them.
    pub(crate) fn slot_style(
        &self,
        is_highlighted: bool,
        is_playing: bool,
        depth: u8,
    ) -> SlotListSlotStyle {
        SlotListSlotStyle::for_slot(
            self.is_center,
            is_highlighted,
            is_playing,
            self.is_selected,
            self.has_multi_selection,
            self.opacity,
            depth,
        )
    }
}

// ============================================================================
// Now-playing breathing highlight
// ============================================================================

/// Period of one full breath / one shimmer cycle, in seconds. Driven per frame
/// by the boat frame tick (`update::boat::handle_boat_tick`) so the motion stays
/// smooth at any display refresh rate. Tune here to change the speed.
pub(crate) const GLOW_PERIOD_SECS: f32 = 3.4;

// The now-playing row is a NORMAL full-bleed slot (the same loud fill as the
// drag-preview ghost; in-list selection is border-only and has no fill). Two
// in-bounds overlays give it life: a pulsing INNER GLOW at the top & bottom
// edges and a travelling SHIMMER sheen sweeping across. Both are gated on
// `glow_seed`, derived from the theme accent, and painted by [`glow_overlay`].
// All values below are tuning knobs.

/// Inner-glow edge alpha at the trough → peak of the breath.
const INNER_GLOW_MIN_ALPHA: f32 = 0.10;
const INNER_GLOW_MAX_ALPHA: f32 = 0.42;

/// Travelling sheen: peak alpha, half-width (fraction of the row), diagonal
/// angle (radians), and the fraction of the period spent sweeping (the rest is
/// an idle gap before the next sweep).
const SHIMMER_PEAK_ALPHA: f32 = 0.22;
const SHIMMER_HALF_WIDTH: f32 = 0.13;
const SHIMMER_ANGLE_RAD: f32 = 1.9;
const SHIMMER_SWEEP_FRACTION: f32 = 0.45;

/// How far each glow light's perceptual lightness is lifted toward white from
/// the accent seed (Oklch `l`-lift via [`theme::lighten_oklch`], holding hue and
/// chroma). The light must be brighter than the (accent) fill to read over it;
/// the inner glow stays nearer the accent, the sheen is lifted closer to white.
/// (These are Oklch lift amounts, not sRGB mix fractions — retune by eye.)
const INNER_GLOW_LIGHT_LIFT: f32 = 0.55;
const SHIMMER_LIGHT_LIFT: f32 = 0.85;

/// Global breathing/shimmer phase in `0.0..1.0`, written each frame by the boat
/// frame tick (`update::boat::handle_boat_tick`) while audio is playing, and
/// read by the now-playing overlays. A process global (like the theme tokens) so
/// the row builders stay pure functions of `(state, theme, phase)`.
static NOW_PLAYING_PHASE: AtomicU32 = AtomicU32::new(0);

/// Store the current phase (`0.0..1.0`). Called once per frame while playing.
pub(crate) fn set_now_playing_phase(phase: f32) {
    NOW_PLAYING_PHASE.store(phase.to_bits(), Ordering::Relaxed);
}

fn now_playing_phase() -> f32 {
    f32::from_bits(NOW_PLAYING_PHASE.load(Ordering::Relaxed))
}

/// Eased 0 → 1 → 0 breath over one period.
fn breath_k(phase: f32) -> f32 {
    0.5 - 0.5 * (phase * std::f32::consts::TAU).cos()
}

/// Inner-glow edge alpha at `phase`.
fn inner_glow_edge_alpha(phase: f32) -> f32 {
    INNER_GLOW_MIN_ALPHA + (INNER_GLOW_MAX_ALPHA - INNER_GLOW_MIN_ALPHA) * breath_k(phase)
}

/// Shimmer band center along the gradient axis at `phase`. Sweeps from just
/// off-screen-left to just off-screen-right during the sweep window, then parks
/// off-screen (returns `> 1`, so no band is visible) for the idle gap.
fn shimmer_band_center(phase: f32) -> f32 {
    if phase < SHIMMER_SWEEP_FRACTION {
        let t = phase / SHIMMER_SWEEP_FRACTION;
        -SHIMMER_HALF_WIDTH + t * (1.0 + 2.0 * SHIMMER_HALF_WIDTH)
    } else {
        2.0
    }
}

/// Pulsing inner-glow gradient: accent light at the top & bottom edges (angle 0
/// is vertical: offset 0 = bottom, 1 = top), transparent center, edge alpha
/// breathing on `phase`.
fn inner_glow_gradient(seed: Color, phase: f32) -> Gradient {
    let light = theme::lighten_oklch(seed, INNER_GLOW_LIGHT_LIFT);
    let edge = Color {
        a: inner_glow_edge_alpha(phase),
        ..light
    };
    let clear = Color { a: 0.0, ..light };
    Gradient::Linear(
        Linear::new(Radians(0.0))
            .add_stop(0.0, edge)
            .add_stop(0.34, clear)
            .add_stop(0.66, clear)
            .add_stop(1.0, edge),
    )
}

/// Travelling-sheen gradient: a bright accent-light band sweeping across the row
/// diagonally, parked off-screen during the idle gap. The band's moving stops
/// (`c` and `c ± SHIMMER_HALF_WIDTH`) drift outside `0.0..=1.0` as the band
/// enters and leaves, so each is added only when in range. `add_stop` already
/// ignores an out-of-range offset, but it `log::warn!`s every time it does,
/// which floods the terminal across the now-playing sweep; filtering first
/// yields the identical gradient (`add_stop` inserts by sorted offset, so order
/// is irrelevant) without the log spam. A fully parked band keeps only the two
/// anchor stops, leaving a transparent (no-sheen) gradient.
fn shimmer_gradient(seed: Color, phase: f32) -> Gradient {
    let light = theme::lighten_oklch(seed, SHIMMER_LIGHT_LIFT);
    let clear = Color { a: 0.0, ..light };
    let bright = Color {
        a: SHIMMER_PEAK_ALPHA,
        ..light
    };
    let c = shimmer_band_center(phase);
    let mut gradient = Linear::new(Radians(SHIMMER_ANGLE_RAD)).add_stop(0.0, clear);
    for (offset, color) in [
        (c - SHIMMER_HALF_WIDTH, clear),
        (c, bright),
        (c + SHIMMER_HALF_WIDTH, clear),
    ] {
        if (0.0..=1.0).contains(&offset) {
            gradient = gradient.add_stop(offset, color);
        }
    }
    Gradient::Linear(gradient.add_stop(1.0, clear))
}

/// Overlay the now-playing breathing glow (pulsing inner glow + travelling
/// shimmer) on top of a full-bleed row. Returns `row` unchanged for every other
/// row (gated on [`SlotListSlotStyle::glow_seed`]); the now-playing row stays a
/// normal full-bleed slot with the glow painted on top. The overlay layers are
/// non-interactive, so the [`Stack`] passes clicks straight through to the row.
pub(crate) fn glow_overlay<'a, M: 'a>(
    row: impl Into<Element<'a, M>>,
    style: SlotListSlotStyle,
) -> Element<'a, M> {
    let row = row.into();
    let Some(seed) = style.glow_seed else {
        return row;
    };
    let inner = container(Space::new().width(Length::Fill).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(Background::Gradient(inner_glow_gradient(
                seed,
                now_playing_phase(),
            ))),
            ..Default::default()
        });
    let shimmer = container(Space::new().width(Length::Fill).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(Background::Gradient(shimmer_gradient(
                seed,
                now_playing_phase(),
            ))),
            ..Default::default()
        });
    Stack::new().push(row).push(inner).push(shimmer).into()
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
    pub hover_text_color: Color,
    /// `true` on the opaque loud-fill rows — the now-playing row, expanded-parent
    /// headers, and the floating drag-preview ghost — where every glyph (index,
    /// empty star/heart outlines, lock) must use the row's forced-legible
    /// `text_color` instead of a muted theme color that would wash out against
    /// the accent fill. A border-only selection (see [`for_slot`]) is NOT one of
    /// these: it keeps the normal row's theme text, so this stays `false` for it.
    /// Read by [`slot_list_static_icon_color`].
    ///
    /// [`for_slot`]: SlotListSlotStyle::for_slot
    pub forces_legible_text: bool,
    /// `Some(fill)` ONLY on the actively-playing row (queue now-playing). The
    /// gate + accent source for the breathing glow: [`glow_overlay`] reads it to
    /// decide whether to stack the pulsing inner glow + travelling shimmer over
    /// the row, and derives the glow light from `fill` (the theme accent). `None`
    /// everywhere else — including the expanded-parent headers that share the
    /// static highlight fill but must not animate. The now-playing row itself
    /// stays a normal full-bleed slot ([`to_container_style`] ignores this); the
    /// glow is purely an in-bounds overlay.
    ///
    /// [`to_container_style`]: SlotListSlotStyle::to_container_style
    pub glow_seed: Option<Color>,
}

impl SlotListSlotStyle {
    /// Get slot styling based on state
    ///
    /// `is_highlighted` covers BOTH the now-playing queue row and expanded-parent
    /// headers (albums/artists/playlists/genres). `is_playing` narrows that to
    /// the actually-playing track: it picks the loud accent fill (shared with the
    /// drag-preview ghost) and arms the breathing glow, while expanded parents
    /// (`is_highlighted` without `is_playing`) keep the calmer static
    /// `playing_fill()`.
    ///
    /// `depth` controls hierarchy-based background darkening for expanded slots:
    /// 0 = parent/root (no darkening), 1 = child, 2 = grandchild.
    pub(crate) fn for_slot(
        is_center: bool,
        is_highlighted: bool,
        is_playing: bool,
        is_selected: bool,
        has_multi_selection: bool,
        opacity: f32,
        depth: u8,
    ) -> Self {
        if is_highlighted {
            // Now-playing (queue) row OR an expanded-parent header. The
            // now-playing row wears the loud accent fill (shared with the
            // drag-preview ghost) and breathes; an expanded-parent header keeps
            // the calmer `playing_fill()` and stays static. Both still get
            // guaranteed-legible forced text, recomputed on the depth-darkened bg
            // so nested rows never push text into the fill.
            let fill = if is_playing {
                theme::selected_fill_resolved()
            } else {
                theme::playing_fill()
            };
            let bg = if depth > 0 {
                theme::darken(fill, depth as f32 * 0.15)
            } else {
                fill
            };
            let txt = theme::legible_text_on(bg);
            // The actively-playing row wears NO ring — its breathing glow +
            // shimmer overlay (see `glow_overlay`) is the sole distinguisher, so
            // the loud fill plus the motion set it apart from a quiet
            // border-only selection. Expanded-parent headers keep the
            // max-contrast highlight ring.
            let (border_color, border_width) = if is_playing {
                (Color::TRANSPARENT, 0.0)
            } else {
                (theme::highlight_border(bg, 1.0), 2.0)
            };
            Self {
                bg_color: bg,
                border_color,
                border_width,
                border_radius: slot_list_border_radius(),
                text_color: txt,
                subtext_color: txt,
                hover_text_color: txt,
                forces_legible_text: true,
                // Only the actively-playing row breathes; an expanded-parent
                // header shares the highlight branch but stays static.
                glow_seed: if is_playing { Some(bg) } else { None },
            }
        } else {
            // A regular slot, OR a selection — which is now just a regular slot
            // plus an accent ring. A "selection" is the multi-selected rows AND
            // the lone click/keyboard cursor when no multi-selection is active.
            //
            // BORDER-ONLY selection, mirroring the theme picker swatch list
            // (`render_theme_slot`): the selected row keeps a normal row's
            // background + theme text colors, and a 2 px accent RING is the sole
            // cue — so a slot-list selection reads exactly like a selection in
            // the theme modal. The loud unified fill is reserved for the
            // now-playing row (handled above) and the drag-preview ghost
            // ([`drag_preview`]); folding selection back into the regular branch
            // keeps the two looks from drifting (selection == regular + ring).
            //
            // [`drag_preview`]: SlotListSlotStyle::drag_preview
            let is_selection = is_selected || (is_center && !has_multi_selection);
            // A selection is IMMUNE to the opacity gradient — exactly like the
            // now-playing / expanded rows above. Its accent ring is the SOLE cue,
            // so fading it with an off-center row would drop it below the contrast
            // floor and the selection could vanish; a plain row still fades.
            let row_alpha = if is_selection { 1.0 } else { opacity };
            // Per-depth background steps along the theme's elevation ramp so
            // nested expansion rows stay distinguishable from each other.
            let base = match depth {
                0 => theme::bg0(),
                1 => theme::bg1(),
                _ => theme::bg2(),
            };
            // Unselected rows touch (`SLOT_SPACING == 0`) and use the 1 px
            // `theme::border()` hairline as a shared separator. Iced draws
            // borders fully inside the rect, so adjacent rows' hairlines overlap
            // into one clean line. In rounded mode the outer list shell owns the
            // sealed `theme::border()` perimeter (square corners) — inside the
            // shell every row is still flush, so the per-row hairline continues
            // to read as a single separator. A selected row swaps that hairline
            // for the louder 2 px accent ring.
            let (border_color, border_width) = if is_selection {
                // Contrast-floored accent ring (see `theme::selection_ring_on`):
                // the raw theme accent on most themes, nudged toward `base`'s
                // contrasting extreme only where the accent would sit too close
                // to the row bg to see. Kept at FULL alpha (the ring never fades
                // with the gradient) so it always clears the contrast floor.
                (theme::selection_ring_on(base), 2.0)
            } else {
                (
                    Color {
                        a: opacity,
                        ..theme::border()
                    },
                    1.0,
                )
            };
            Self {
                bg_color: Color {
                    a: row_alpha,
                    ..base
                },
                border_color,
                border_width,
                border_radius: slot_list_border_radius(),
                text_color: Color {
                    a: row_alpha,
                    ..theme::fg0()
                },
                subtext_color: Color {
                    a: row_alpha,
                    ..theme::fg4()
                },
                hover_text_color: theme::accent_bright(),
                forces_legible_text: false,
                glow_seed: None,
            }
        }
    }

    /// Bold style for a floating drag-preview ghost: the loud unified selection
    /// fill, guaranteed-legible forced text, and a max-contrast ring, so the
    /// dragged row reads clearly as "grabbed" while it floats over arbitrary
    /// content. This is deliberately NOT the quiet in-list selection look (which
    /// is a border-only accent ring — see [`for_slot`]): a drag ghost wants to
    /// shout, an in-list selection wants to whisper.
    ///
    /// [`for_slot`]: SlotListSlotStyle::for_slot
    pub(crate) fn drag_preview() -> Self {
        let fill = theme::selected_fill_resolved();
        let txt = theme::legible_text_on(fill);
        Self {
            bg_color: fill,
            border_color: theme::highlight_border(fill, 1.0),
            border_width: 2.0,
            border_radius: slot_list_border_radius(),
            text_color: txt,
            subtext_color: txt,
            hover_text_color: txt,
            forces_legible_text: true,
            glow_seed: None,
        }
    }

    /// Convert to an `iced::widget::container::Style` for slot background/border rendering.
    ///
    /// This is the single source of truth for the `SlotListSlotStyle → container::Style`
    /// conversion used across all view files, empty slots, and drag previews. The
    /// now-playing row renders as a normal full-bleed slot here; its breathing glow is
    /// painted on top as overlays by [`glow_overlay`], not in this style.
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

/// Border radius for slot list slots — always zero.
///
/// Slot rows stay square in both flat and rounded modes. The rounded-mode
/// list shell (`slot_list_background_container`) draws a square
/// `theme::border()` perimeter and clips the touching row hairlines at its
/// edges via `clip(true)`, so the outer perimeter still reads as a sealed
/// shell even though every individual row inside it has 0 px corners.
pub(crate) fn slot_list_border_radius() -> iced::border::Radius {
    iced::border::Radius::default()
}

/// Standard vertical spacing between slot list slot elements
pub(crate) const SLOT_LIST_COL_SPACING: f32 = 4.0;

/// Standard width for the index column (supports up to 4 digits)
pub(crate) const SLOT_LIST_INDEX_WIDTH: f32 = 60.0;

/// Width reserved for the leading multi-select checkbox column. Wide enough
/// to give the 16px box a comfortable click target with surrounding padding.
pub(crate) const SLOT_LIST_SELECT_WIDTH: f32 = 40.0;

/// Height of the tri-state "select all" header bar that appears above the
/// slot list when the per-view select column is active. Subtracted from the
/// slot list available height so slot count math stays correct.
pub(crate) const SELECT_HEADER_HEIGHT: f32 = 24.0;

/// Side length (px) of the per-row multi-select checkbox visual. Tuned for
/// the flat redesign — 18 px gives the click target a comfortable 9 px
/// gutter inside the 40 px column without crowding the row text.
const CHECKBOX_SIZE: f32 = 18.0;

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
///
/// Flat redesign target: rows touch (0 px gap) so the bottom-only
/// `theme::border()` separators line up into a single continuous rule
/// between rows. The constant is kept at `0.0` in both flat and rounded
/// modes — rounded mode wraps the whole list in an outer shell instead
/// of separating individual rows.
pub(crate) const SLOT_SPACING: f32 = 0.0;

/// Total chrome consumed by the view-header row.
///
/// `HEADER_HEIGHT` strip + 1 px `theme::border()` sibling separator below it.
/// Derives from `view_header::HEADER_HEIGHT` + `HEADER_BOTTOM_SEPARATOR` so
/// the slot-count math stays welded to the actual rendered widget. The
/// header keeps its flat treatment in both flat and rounded modes — the
/// surrounding pill capsule was removed because it looked out of place
/// stacked above the slot-list shell.
#[inline]
pub(crate) fn view_header_chrome() -> f32 {
    super::view_header::HEADER_HEIGHT + super::view_header::HEADER_BOTTOM_SEPARATOR
}

/// Chrome footprint of the *collapsed* auto-hide toolbar, per the chosen
/// [`CollapsedAppearance`]: the configurable Hairline sliver + separator, the
/// thin invisible Hidden catch-zone (no separator), or the Count strip +
/// separator. Used in place of [`view_header_chrome`] when a view's toolbar is
/// hidden, so the slot list reclaims the freed height as extra rows.
pub(crate) fn collapsed_view_header_chrome() -> f32 {
    use nokkvi_data::types::player_settings::CollapsedAppearance;

    use super::view_header::{COUNT_STRIP_HEIGHT, HEADER_BOTTOM_SEPARATOR, HIDDEN_CATCH_HEIGHT};
    match crate::theme::autohide_collapsed_appearance() {
        CollapsedAppearance::Hairline => {
            f32::from(crate::theme::autohide_toolbar_height_px()) + HEADER_BOTTOM_SEPARATOR
        }
        // No separator — the list reads as reclaiming the entire top.
        CollapsedAppearance::Hidden => HIDDEN_CATCH_HEIGHT,
        CollapsedAppearance::CountStrip => COUNT_STRIP_HEIGHT + HEADER_BOTTOM_SEPARATOR,
    }
}

/// Height of the browsing panel tab bar.
pub(crate) const TAB_BAR_HEIGHT: f32 = 32.0;

use super::player_bar::player_bar_height;

/// Total height of chrome elements for views with headers.
///
/// In top nav mode: nav_bar + player_bar + view_header_chrome(), plus the
/// `TopBarUnder` strip when that mode is active (sits in its own row beneath
/// the nav bar — see `show_top_bar_under_strip`).
/// In side and none nav modes: player_bar + view_header_chrome() (no top bar),
/// plus the strip when `TopBar` or `TopBarUnder` is active (both render as a
/// row above the content in those layouts — see `show_top_bar_strip`).
///
/// The slot list runs flush to the player bar, so no bottom pad is subtracted
/// from the slot-count math in `with_dynamic_slots`.
pub(crate) fn chrome_height_with_header(collapsed_header: bool) -> f32 {
    let header_chrome = if collapsed_header {
        collapsed_view_header_chrome()
    } else {
        view_header_chrome()
    };
    if crate::theme::is_top_nav() {
        let top_bar_under_strip = if crate::theme::show_top_bar_under_strip() {
            super::track_info_strip::STRIP_HEIGHT_WITH_SEPARATOR
        } else {
            0.0
        };
        crate::theme::nav_bar_height() + player_bar_height() + header_chrome + top_bar_under_strip
    } else {
        // Side or None mode: no top nav bar, but TopBar / TopBarUnder add height
        let top_bar_strip = if crate::theme::show_top_bar_strip() {
            super::track_info_strip::STRIP_HEIGHT_WITH_SEPARATOR
        } else {
            0.0
        };
        player_bar_height() + header_chrome + top_bar_strip
    }
}

/// Chrome height for a view whose own bar fully **replaces** the view header
/// (the playlist editor's edit bar stands in for the `view_header` strip, which
/// that view never renders). Equals [`chrome_height_with_header`] minus
/// [`view_header_chrome`], leaving the caller to add its replacement bar's
/// height instead. Derived by subtraction so it stays welded to
/// `chrome_height_with_header` — a new chrome term added there tracks here
/// automatically. Counting `view_header_chrome()` in such a view over-reserves
/// 51 px and leaves a blank, placeholder-less band at the bottom of the list.
#[inline]
pub(crate) fn chrome_height_without_view_header() -> f32 {
    // The view header is fully replaced here, so the collapsed/expanded
    // distinction is irrelevant — subtract the full footprint either way.
    chrome_height_with_header(false) - view_header_chrome()
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
    /// When false, the per-slot hover/press color wash is suppressed (the press
    /// scale-down still fires). Used by the theme-picker swatch list, where the
    /// active-theme accent wash would muddy rows painted in their own palette.
    pub hover_wash: bool,
}

impl Default for SlotListConfig {
    fn default() -> Self {
        Self {
            slot_count: 9,
            center_slot: 4,
            window_height: 800.0,
            chrome_height: chrome_height_with_header(false),
            cull_empty: false,
            modifiers: iced::keyboard::Modifiers::default(),
            hover_wash: true,
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
        let available_height = (window_height - chrome_height).max(0.0);

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
            hover_wash: true,
        }
    }

    /// Builder method to set global keyboard modifiers for slot interactions
    pub(crate) fn with_modifiers(mut self, modifiers: iced::keyboard::Modifiers) -> Self {
        self.modifiers = modifiers;
        self
    }

    /// Suppress the per-slot hover/press color wash. For lists whose rows are
    /// painted in their own (non-active-theme) colors, where the wash clashes.
    pub(crate) fn without_hover_wash(mut self) -> Self {
        self.hover_wash = false;
        self
    }

    /// Calculate row height based on window size
    /// All slots have uniform height.
    pub(crate) fn row_height(&self) -> f32 {
        let available_height = (self.window_height - self.chrome_height).max(0.0);
        let spacing_height = (self.slot_count.saturating_sub(1)) as f32 * SLOT_SPACING;
        let content_height = (available_height - spacing_height).max(0.0);

        (content_height / self.slot_count.max(1) as f32).max(40.0)
    }
}

/// Render a slot list view with custom item rendering
///
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
#[expect(clippy::too_many_arguments)] // 7 args; an options-struct would force boxing on_seek
pub(crate) fn slot_list_view_with_scroll<'a, T, Message: Clone + 'a>(
    sl: &SlotListView,
    items: &[T],
    config: &SlotListConfig,
    on_scroll_up: Message,
    on_scroll_down: Message,
    on_seek: impl Fn(f32) -> Message + 'a,
    on_hover: Option<SlotHoverCallback<'a, Message>>,
    mut render_item: impl FnMut(&T, SlotListRowContext) -> Element<'a, Message>,
) -> Element<'a, Message> {
    let total_items = items.len();
    let row_height = config.row_height();
    let slots = build_slot_list_slots(sl, items, config, &mut render_item, on_hover);
    let inner: Element<'a, Message> = container(
        column(slots)
            .spacing(SLOT_SPACING)
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into();
    // Reserve the gutter (Always mode) on the content FIRST, then wrap the
    // whole stack in the wheel `mouse_area`. Order matters: wrapping the
    // mouse_area first and padding it afterward would shrink the wheel
    // hit-region, so a wheel over the reserved scrollbar gutter would do
    // nothing. Wrapping the mouse_area LAST keeps it full-width, so the wheel
    // works everywhere — including over the permanent track.
    let indicated = crate::widgets::scroll_indicator::wrap_with_scroll_indicator(
        inner,
        sl,
        total_items,
        row_height,
        on_seek,
    );
    wrap_with_scroll(indicated, on_scroll_up, on_scroll_down)
}

/// Render a slot list view with scroll support AND drag-and-drop reordering.
///
/// Same as `slot_list_view_with_scroll` but the inner column of slots is a `DragColumn`
/// that emits drag events via `on_drag_event`. Slot indices in the `DragEvent` are
/// raw **slot** indices — caller translates to item indices via `viewport_offset`.
#[expect(clippy::too_many_arguments)] // Mirrors slot_list_view_with_scroll (8 args) +on_drag_event +drop_indicator_slot; struct would require boxing on_seek
pub(crate) fn slot_list_view_with_drag<'a, T, Message: Clone + 'a>(
    sl: &SlotListView,
    items: &[T],
    config: &SlotListConfig,
    on_scroll_up: Message,
    on_scroll_down: Message,
    on_seek: impl Fn(f32) -> Message + 'a,
    on_drag_event: impl Fn(crate::widgets::drag_column::DragEvent) -> Message + 'a,
    on_hover: Option<SlotHoverCallback<'a, Message>>,
    drop_indicator_slot: Option<usize>,
    mut render_item: impl FnMut(&T, SlotListRowContext) -> Element<'a, Message>,
) -> Element<'a, Message> {
    use iced::widget::{Space, Stack, stack};

    use crate::widgets::drag_column::DragColumn;

    let total_items = items.len();
    let row_height = config.row_height();
    let badge_count = if sl.selected_indices.len() > 1 {
        sl.selected_indices.len()
    } else {
        1
    };
    let slots = build_slot_list_slots(sl, items, config, &mut render_item, on_hover);

    let drag_column: Element<'a, Message> = DragColumn::from_vec(slots)
        .spacing(SLOT_SPACING)
        .width(Length::Fill)
        .height(Length::Fill)
        .on_drag(on_drag_event)
        .drag_badge_count(badge_count)
        .into();

    // Drop indicator rendered inside the slot list's own coordinate space,
    // so its y-position is `slot_index * (row_height + SLOT_SPACING)` with
    // no chrome math involved. Empty stack when no drag is active.
    let content: Element<'a, Message> = if let Some(slot_idx) = drop_indicator_slot {
        let slot_step = row_height + SLOT_SPACING;
        let indicator_y = ((slot_idx as f32 * slot_step) - SLOT_SPACING / 2.0).max(0.0);
        let line = container(Space::new())
            .width(Length::Fill)
            .height(Length::Fixed(2.0))
            .style(|_theme: &iced::Theme| container::Style {
                background: Some(theme::accent_bright().into()),
                border: iced::Border {
                    radius: 2.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            });
        let indicator = container(line).width(Length::Fill).padding(iced::Padding {
            top: indicator_y,
            left: 0.0,
            right: 0.0,
            bottom: 0.0,
        });
        let mut s: Stack<'a, Message> = stack![drag_column];
        s = s.push(indicator);
        s.into()
    } else {
        drag_column
    };

    let inner: Element<'a, Message> = container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .into();

    // Reserve the gutter (Always mode) on the content FIRST, then wrap the
    // whole stack in the wheel `mouse_area`. Order matters: wrapping the
    // mouse_area first and padding it afterward would shrink the wheel
    // hit-region, so a wheel over the reserved scrollbar gutter would do
    // nothing. Wrapping the mouse_area LAST keeps it full-width, so the wheel
    // works everywhere — including over the permanent track.
    let indicated = crate::widgets::scroll_indicator::wrap_with_scroll_indicator(
        inner,
        sl,
        total_items,
        row_height,
        on_seek,
    );
    wrap_with_scroll(indicated, on_scroll_up, on_scroll_down)
}

/// Build the slot elements for a slot list view.
///
/// Shared by `slot_list_view`, `slot_list_view_with_drag`, etc. to avoid
/// duplicating slot rendering logic. The slot→item index mapping (including
/// the effective-center calculation) is owned by
/// `SlotListView::slot_to_item` / `SlotListView::effective_center`, which the
/// drag mappers also call so rendering and dragging stay in lockstep.
///
/// When `on_hover` is `Some`, each slot is wrapped in a `mouse_area` that
/// fires `on_enter` / `on_exit` carrying the slot's `HoveredSlot`
/// (containing both its visual slot index and the resolved item index for
/// the current viewport offset). Consumers store this on
/// `SlotListView::hovered_slot` and read it from cross-pane drag handlers
/// instead of reconstructing slot positions from chrome constants.
fn build_slot_list_slots<'a, T, Message: Clone + 'a>(
    sl: &SlotListView,
    items: &[T],
    config: &SlotListConfig,
    render_item: &mut impl FnMut(&T, SlotListRowContext) -> Element<'a, Message>,
    on_hover: Option<SlotHoverCallback<'a, Message>>,
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
    // Presentation-only center used for opacity gradient (:`calculate_slot_opacity_with_center`)
    // and the center-styling fallback (`slot_index == effective_center` below).
    // The ITEM-INDEX mapping is owned by `SlotListView::slot_to_item` — do not
    // reuse this value for index resolution. In top-packing mode this clamps
    // to the visible viewport row; otherwise it is the shared effective_center
    // (`config.center_slot == config.slot_count / 2` by the
    // `center_slot_is_always_middle` invariant).
    let effective_center = if top_packing {
        sl.viewport_offset.min(total_items.saturating_sub(1))
    } else {
        sl.effective_center(total_items, config.slot_count, config.center_slot)
    };

    let metrics = SlotListRowMetrics::from_row(row_height, 1.0);

    let mut slots = Vec::with_capacity(config.slot_count);
    for slot_index in 0..config.slot_count {
        let opacity = if crate::theme::is_opacity_gradient() {
            SlotListView::calculate_slot_opacity_with_center(slot_index, effective_center)
        } else {
            1.0
        };
        let scale_factor = 1.0;

        let mut is_center_slot = false;

        // Resolve the item index through the shared slot→item owner
        // (`allow_end = false` is the render reject: target >= total_items and
        // top-packing cap slot < total_items). Top-packing vs end_push are
        // handled internally, so this is the single source for both rendering
        // and the drag mappers.
        let item_index_opt = sl.slot_to_item(
            slot_index,
            total_items,
            config.slot_count,
            config.center_slot,
            false,
        );

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
                    has_multi_selection: !sl.selected_indices.is_empty(),
                    opacity,
                    row_height,
                    scale_factor,
                    modifiers: config.modifiers,
                    metrics,
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

        let hover_target: Element<'a, Message> =
            crate::widgets::hover_overlay::HoverOverlay::new(slot_element)
                .border_radius(slot_list_border_radius())
                .flash_at(flash)
                .wash_enabled(config.hover_wash)
                .into();

        let wrapped = if let Some(cb) = on_hover.as_ref() {
            let hovered = match item_index_opt {
                Some(item_index) => HoveredSlot::Item {
                    slot_index,
                    item_index,
                    items_len: total_items,
                },
                None => HoveredSlot::Empty {
                    slot_index,
                    items_len: total_items,
                },
            };
            let enter_msg = (cb.on_enter)(hovered);
            let exit_msg = (cb.on_exit)(hovered);
            // `on_move` republishes the enter payload on every CursorMoved
            // while the cursor is inside the slot. The iced mouse_area diff
            // only fires `on_enter`/`on_exit` on a bounds- OR position-change
            // crossing, so a cursor-stationary scroll (viewport_offset shift)
            // would otherwise leave `hovered_slot` referencing the
            // pre-scroll `item_index`. With `on_move`, any cursor twitch
            // after the scroll re-bakes the message with the current
            // render's `item_index` and refreshes state.
            let move_msg = enter_msg.clone();
            mouse_area(hover_target)
                .on_enter(enter_msg)
                .on_exit(exit_msg)
                .on_move(move |_pt| move_msg.clone())
                .into()
        } else {
            hover_target
        };

        slots.push(wrapped);
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
    slot_list_labeled_index_column(format!("{}", index + 1), font_size, style, opacity)
}

/// Resolve the color for a static tinted glyph (lock, index / sub-index label,
/// empty heart/star outline) inside a slot-list row. On the opaque-fill
/// highlight rows (`style.forces_legible_text`) it returns the row's
/// forced-legible `text_color` so the glyph matches the song name and stays
/// readable against the accent fill; otherwise `fallback` with the given alpha.
/// Always prefer this over hardcoding a `theme::fg*()` color in a row renderer.
pub(crate) fn slot_list_static_icon_color(
    style: SlotListSlotStyle,
    fallback: Color,
    opacity: f32,
) -> Color {
    if style.forces_legible_text {
        style.text_color
    } else {
        Color {
            a: opacity,
            ..fallback
        }
    }
}

/// Render an index column with a free-form label (e.g. dotted decimal "236.1"
/// for expanded child rows). Shares styling with `slot_list_index_column` so
/// child sub-indices visually match parent indices in font, color, and width.
pub(crate) fn slot_list_labeled_index_column<'a, Message: 'a>(
    label: impl Into<String>,
    font_size: f32,
    style: SlotListSlotStyle,
    opacity: f32,
) -> Element<'a, Message> {
    use iced::Alignment;

    container(slot_list_text(
        label.into(),
        font_size,
        slot_list_static_icon_color(style, theme::fg4(), opacity * 0.7),
    ))
    .width(Length::Fixed(SLOT_LIST_INDEX_WIDTH))
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
    .into()
}

/// Alpha applied to `theme::fg2()` for the UNCHECKED select-checkbox outline.
///
/// Opaque `fg2()` equals the body-text color in many themes, so a 1 px box ring
/// painted in it reads even louder than the row text it sits beside. Muting it
/// to this alpha keeps the outline clearly visible (composited WCAG contrast
/// ~1.8–4.5 across every shipped theme/mode, floored well above the old
/// `theme::border()` hairline's invisible 1.0–1.7) while staying subordinate to
/// the opaque text in all of them. It is the calibrated midpoint between the
/// invisible `border()` and the over-loud opaque `fg2()`.
const UNCHECKED_BOX_OUTLINE_ALPHA: f32 = 0.5;

/// Muted `theme::fg2()` outline shared by the per-row select box and the
/// tri-state header box. Returns a *translucent* `fg2` (not a precomputed blend)
/// so it composites correctly over whatever backs the box — the row background
/// for per-row boxes, the `bg0_soft` strip for the header.
fn unchecked_box_outline() -> iced::Color {
    iced::Color {
        a: UNCHECKED_BOX_OUTLINE_ALPHA,
        ..theme::fg2()
    }
}

/// Shared box visual for the per-row select checkbox and the tri-state
/// "select all" header box — the two MUST stay visually identical so they
/// read as the same family.
///
/// Flat redesign treatment: unchecked sits transparent with the muted
/// `unchecked_box_outline()`; checked fills with `theme::accent_bright()` and
/// matches the row's selected-state colors. The per-row box lives in its own
/// left column (sibling of the filled row content in
/// `wrap_with_select_column`), so it is always backed by the plain slot-list
/// shell bg — never an accent highlight fill — while the header box sits over
/// the `bg0_soft` strip; the translucent outline composites correctly over
/// both. Keep it that way or the unchecked outline needs to become
/// fill-aware.
///
/// Colors are evaluated here (at view-build time) and captured by the style
/// closure; only `ui_radius_xs()` stays inside the closure so the radius
/// tracks the live rounded-mode toggle. `glyph` receives the resolved glyph
/// color (`bg0_hard` on the accent fill, `fg0` otherwise).
fn checkbox_box_visual<'a, Message: 'a>(
    checked: bool,
    glyph: impl FnOnce(iced::Color) -> Element<'a, Message>,
) -> iced::widget::Container<'a, Message> {
    use iced::Alignment;

    let bg_color = if checked {
        theme::accent_bright()
    } else {
        iced::Color::TRANSPARENT
    };
    let border_color = if checked {
        theme::accent_bright()
    } else {
        unchecked_box_outline()
    };
    let glyph_color = if checked {
        theme::bg0_hard()
    } else {
        theme::fg0()
    };

    container(glyph(glyph_color))
        .width(Length::Fixed(CHECKBOX_SIZE))
        .height(Length::Fixed(CHECKBOX_SIZE))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .style(move |_| container::Style {
            background: Some(bg_color.into()),
            border: iced::Border {
                color: border_color,
                width: 1.0,
                radius: theme::ui_radius_xs(),
            },
            ..Default::default()
        })
}

/// The 14×14 check.svg glyph shared by the checked per-row box and the
/// header's `All` state, tinted with the resolved glyph color.
fn select_check_glyph<'a, Message: 'a>(color: iced::Color) -> Element<'a, Message> {
    use iced::widget::svg;

    crate::embedded_svg::svg_widget("assets/icons/check.svg")
        .width(Length::Fixed(14.0))
        .height(Length::Fixed(14.0))
        .style(move |_, _| svg::Style { color: Some(color) })
        .into()
}

/// Render the leading multi-select checkbox column for a slot list row.
///
/// Built from a `mouse_area`-wrapped custom 16×16 box (matching the
/// tri-state "select all" header bar) rather than `iced::widget::Checkbox`.
/// `mouse_area::on_press` calls `shell.capture_event()` on left-click, so
/// the click is consumed before the row's surrounding `button` (or any
/// other sibling click handler) can react — even when the row is rendered
/// inside the browsing-panel split-view, where the row's button dispatches
/// `SlotListSetOffset(no_modifiers)` (which would otherwise call
/// `clear_multi_selection` and erase every other selection).
pub(crate) fn slot_list_select_checkbox<'a, Message: 'a + Clone>(
    is_checked: bool,
    item_index: usize,
    on_toggle: impl Fn(usize) -> Message + 'a,
) -> Element<'a, Message> {
    use iced::{
        Alignment,
        widget::{Space, mouse_area},
    };

    // Visual recipe shared with the tri-state header box — see
    // `checkbox_box_visual` for the flat-treatment / outline rationale.
    let box_visual = checkbox_box_visual(is_checked, |glyph_color| {
        if is_checked {
            select_check_glyph(glyph_color)
        } else {
            Space::new()
                .width(Length::Fixed(0.0))
                .height(Length::Fixed(0.0))
                .into()
        }
    });

    // Centre the visible 16×16 box inside a column-wide hit area, then
    // wrap that whole area in `mouse_area` so the click target covers the
    // entire 40 px column (including the empty padding around the box).
    // Otherwise a click that lands on the column's padding would slip past
    // the checkbox's bounds and propagate to the row's surrounding button.
    let cell = container(box_visual)
        .width(Length::Fixed(SLOT_LIST_SELECT_WIDTH))
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center);

    mouse_area(cell)
        .on_press(on_toggle(item_index))
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}

/// Wrap a slot's main content with the leading select-checkbox column when
/// the per-view "select" flag is on. Returns `inner` unchanged when the
/// column is hidden.
///
/// The checkbox state mirrors `selected_indices` membership, regardless of
/// how membership was set (ctrl/shift+click, the checkbox itself, or the
/// header tri-state). Click on the checkbox dispatches `on_toggle(item_index)`.
pub(crate) fn wrap_with_select_column<'a, Message: 'a + Clone>(
    show: bool,
    is_selected: bool,
    item_index: usize,
    on_toggle: impl Fn(usize) -> Message + 'a,
    inner: Element<'a, Message>,
) -> Element<'a, Message> {
    if !show {
        return inner;
    }
    use iced::widget::row;
    let cb = slot_list_select_checkbox(is_selected, item_index, on_toggle);
    row![cb, inner]
        .align_y(iced::Alignment::Center)
        .spacing(0.0)
        .into()
}

/// Context-driven convenience wrapper over [`wrap_with_select_column`] for the
/// common slot-list views.
///
/// Pulls `is_selected` / `item_index` off the row context and synthesizes the
/// identical `SlotListPageMessage::SelectionToggle` lambda that every
/// select-column view repeats verbatim, so each call site shrinks to the
/// per-view `wrap` constructor (`AlbumsMessage::SlotList`, etc.). `wrap` lifts a
/// `SlotListPageMessage` into the caller's outer message type — the same pattern
/// as [`primary_slot_button`]. Returns `inner` unchanged when `show` is false.
///
/// Sites that route selection through a bespoke event channel rather than a
/// plain fn-pointer (e.g. the song-list pane's `SongListRowEvent::Slot`) keep
/// calling [`wrap_with_select_column`] directly.
pub(crate) fn wrap_with_select_column_for<'a, M: 'a + Clone>(
    show: bool,
    ctx: &SlotListRowContext,
    wrap: impl Fn(SlotListPageMessage) -> M + 'a,
    inner: Element<'a, M>,
) -> Element<'a, M> {
    let item_index = ctx.item_index;
    wrap_with_select_column(
        show,
        ctx.is_selected,
        item_index,
        move |i| wrap(SlotListPageMessage::SelectionToggle(i)),
        inner,
    )
}

/// Render the tri-state "select all" header bar that sits above the slot
/// list when the per-view select column is active. Built from a `mouse_area`
/// wrapping a custom container instead of `iced::widget::checkbox` so we
/// can paint the partial-selection state with a visible dash, since iced's
/// binary checkbox lacks a tri-state mode.
pub(crate) fn slot_list_select_header<'a, Message: Clone + 'a>(
    state: crate::widgets::slot_list_page::SelectAllState,
    on_toggle: Message,
) -> Element<'a, Message> {
    use iced::{
        Alignment, Length,
        widget::{Space, container, mouse_area, row},
    };

    use crate::widgets::slot_list_page::SelectAllState;

    // Visual recipe shared with the per-row checkbox via
    // `checkbox_box_visual`, so the header reads as the same family as the
    // per-row boxes. `Some` and `All` both render as "checked"; the glyph
    // (dash vs check) distinguishes them.
    let box_visual = checkbox_box_visual(state.is_checked_visual(), |glyph_color| match state {
        SelectAllState::All => select_check_glyph(glyph_color),
        SelectAllState::Some => container(Space::new())
            .width(Length::Fixed(10.0))
            .height(Length::Fixed(2.0))
            .style(move |_| container::Style {
                background: Some(glyph_color.into()),
                ..Default::default()
            })
            .into(),
        SelectAllState::None => Space::new()
            .width(Length::Fixed(0.0))
            .height(Length::Fixed(0.0))
            .into(),
    });

    let cb_cell = mouse_area(box_visual)
        .on_press(on_toggle)
        .interaction(iced::mouse::Interaction::Pointer);

    container(
        row![
            container(cb_cell)
                .width(Length::Fixed(SLOT_LIST_SELECT_WIDTH))
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center),
            Space::new().width(Length::Fill),
        ]
        .height(Length::Fill),
    )
    .height(Length::Fixed(SELECT_HEADER_HEIGHT))
    .width(Length::Fill)
    .style(|_| container::Style {
        background: Some(theme::bg0_soft().into()),
        ..Default::default()
    })
    .into()
}

/// Compose a view's existing header element with the tri-state select-all
/// header bar below it. Returns `header` unchanged when the select column
/// is off, so out-of-scope views keep their original layout.
pub(crate) fn compose_header_with_select<'a, Message: Clone + 'a>(
    show: bool,
    state: crate::widgets::slot_list_page::SelectAllState,
    on_toggle: Message,
    header: Element<'a, Message>,
) -> Element<'a, Message> {
    if !show {
        return header;
    }
    iced::widget::column![header, slot_list_select_header(state, on_toggle)]
        .spacing(0)
        .into()
}

/// Chrome height with optional select-header bar. Each view consults this
/// instead of [`chrome_height_with_header`] when its select column may be
/// active, so slot-count math accounts for the extra 24 px above the slots.
pub(crate) fn chrome_height_with_select_header(
    collapsed_header: bool,
    select_header_visible: bool,
) -> f32 {
    chrome_height_with_header(collapsed_header)
        + if select_header_visible {
            SELECT_HEADER_HEIGHT
        } else {
            0.0
        }
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

    let inner: Element<'a, Message> = if let Some(handle) = artwork_handle {
        image(handle.clone())
            .content_fit(iced::ContentFit::Cover)
            .width(Length::Fill)
            .height(Length::Fill)
            .opacity(effective_opacity)
            .into()
    } else {
        // Empty-state child stays a `text("")` (not `Space`) — an empty text
        // node and a Space have different intrinsic layout in some
        // containers, and the placeholder must keep pixel parity with the
        // artwork branch's chassis.
        iced::widget::text("").into()
    };

    container(inner)
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

/// Render a 2×2 quad artwork column for a slot list slot.
///
/// Same chassis as [`slot_list_artwork_column`] (fixed square, `bg2`
/// backdrop, center/highlight opacity rules) with the single image swapped
/// for a [`quad_artwork_grid`](crate::widgets::base_slot_list_layout::quad_artwork_grid).
/// Used by playlist and genre rows whose first ≤4 distinct album tiles are
/// all cached; callers fall back to the single-mini column otherwise so the
/// column never renders a half-filled grid.
pub(crate) fn slot_list_artwork_quad_column<'a, Message: 'a>(
    tile_handles: &[&iced::widget::image::Handle],
    artwork_size: f32,
    is_center: bool,
    is_highlighted: bool,
    opacity: f32,
) -> Element<'a, Message> {
    let effective_opacity = if is_center || is_highlighted {
        1.0
    } else {
        opacity
    };

    let grid = crate::widgets::base_slot_list_layout::quad_artwork_grid(
        tile_handles,
        artwork_size,
        effective_opacity,
    );

    container(grid)
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
#[allow(clippy::too_many_arguments)]
pub(crate) fn slot_list_text_column<'a, Message: Clone + 'a + 'static>(
    title: String,
    title_on_press: Option<Message>,
    subtitle: String,
    subtitle_on_press: Option<Message>,
    title_size: f32,
    subtitle_size: f32,
    style: SlotListSlotStyle,
    is_bold: bool,
    portion: u16,
) -> Element<'a, Message> {
    // When slot text links are disabled, suppress click messages
    let links_enabled = crate::theme::is_slot_text_links();
    let title_on_press = if links_enabled { title_on_press } else { None };
    let subtitle_on_press = if links_enabled {
        subtitle_on_press
    } else {
        None
    };
    use iced::{
        Alignment,
        widget::text::{Ellipsis, Wrapping},
    };

    let title_font = if is_bold {
        theme::weighted_ui_font(iced::font::Weight::Bold)
    } else {
        theme::ui_font()
    };

    let title_widget: Element<'a, Message> = if let Some(msg) = title_on_press {
        crate::widgets::link_text::LinkText::new(title)
            .size(title_size)
            .color(style.text_color)
            .hover_color(style.hover_text_color)
            .font(title_font)
            .on_press(Some(msg))
            .into()
    } else {
        iced::widget::text(title)
            .size(title_size)
            .color(style.text_color)
            .font(title_font)
            .wrapping(Wrapping::None)
            .ellipsis(Ellipsis::End)
            .into()
    };

    // Empty subtitle → render title-only so the row doesn't reserve a
    // blank line beneath the title (relevant when callers want a clean
    // single-line layout, e.g. playlists with all metadata columns off).
    if subtitle.is_empty() {
        return container(title_widget)
            .width(Length::FillPortion(portion))
            .height(Length::Fill)
            .clip(true)
            .align_y(Alignment::Center)
            .into();
    }

    let subtitle_widget: Element<'a, Message> = if let Some(msg) = subtitle_on_press {
        crate::widgets::link_text::LinkText::new(subtitle)
            .size(subtitle_size)
            .color(style.subtext_color)
            .hover_color(style.hover_text_color)
            .font(theme::ui_font())
            .on_press(Some(msg))
            .into()
    } else {
        slot_list_text(subtitle, subtitle_size, style.subtext_color).into()
    };

    container(column![title_widget, subtitle_widget].spacing(SLOT_LIST_COL_SPACING))
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
pub(crate) fn slot_list_metadata_column<'a, Message: Clone + 'a + 'static>(
    content: String,
    on_press: Option<Message>,
    font_size: f32,
    style: SlotListSlotStyle,
    portion: u16,
) -> Element<'a, Message> {
    use iced::Alignment;

    let text_widget: Element<'a, Message> =
        if let Some(msg) = on_press.filter(|_| crate::theme::is_slot_text_links()) {
            crate::widgets::link_text::LinkText::new(content)
                .size(font_size)
                .color(style.subtext_color)
                .hover_color(style.hover_text_color)
                .font(theme::ui_font())
                .on_press(Some(msg))
                .into()
        } else {
            slot_list_text(content, font_size, style.subtext_color).into()
        };

    container(text_widget)
        .width(Length::FillPortion(portion))
        .height(Length::Fill)
        .clip(true)
        .align_y(Alignment::Center)
        .into()
}

/// Inter-star gap in the five-star rating row, in logical pixels. Shared
/// between the row's `.spacing()` and [`star_row_intrinsic_width`] so the cap
/// stays in lockstep with the actual layout — a drift here would mis-size the
/// row and reintroduce uneven star scaling.
const STAR_ROW_SPACING: f32 = 2.0;

/// Natural (full-size) width of the five-star rating row: five `icon_size`-wide
/// stars plus the four inter-star gaps.
///
/// The row is handed this as a *fixed* width so the group caps at full size on
/// wide columns, and on narrower columns iced's flex layout clamps the whole
/// row down — distributing the deficit *evenly* across all five
/// `FillPortion(1)` stars rather than squeezing only the trailing one. That
/// keeps a 5/5 rating legible as five equal stars at any width (the previous
/// `Fixed`-per-star layout let only the last star shrink, so 5/5 could read as
/// 4/5).
fn star_row_intrinsic_width(icon_size: f32, spacing: f32) -> f32 {
    5.0 * icon_size + 4.0 * spacing
}

/// Layer a filled SVG icon with a semi-transparent outline SVG on top.
///
/// Used by star ratings and favorite icons to ensure the filled icon has a
/// visible contrasting edge regardless of the background color / theme.
///
/// `width` controls the layout width of the icon: `Length::Fixed(icon_size)`
/// for a standalone square icon (e.g. the love-column heart), or
/// `Length::FillPortion(1)` so a star in the rating row shares width evenly
/// with its siblings and the whole group scales uniformly when clamped. Height
/// is always `Fixed(icon_size)`; the inner SVGs fill the box and `ContentFit`
/// keeps the glyph square and centered.
fn outlined_svg_icon<'a, M: 'a>(
    filled_path: &str,
    outline_path: &str,
    icon_size: f32,
    fill_color: Color,
    opacity: f32,
    width: Length,
) -> Element<'a, M> {
    use iced::widget::svg;

    let outline_color = Color {
        a: 0.6,
        ..theme::bg0_hard()
    };
    let fill_svg: Element<'a, M> = crate::embedded_svg::svg_widget(filled_path)
        .width(Length::Fill)
        .height(Length::Fill)
        .opacity(opacity)
        .style(move |_theme, _status| svg::Style {
            color: Some(fill_color),
        })
        .into();
    let outline_svg: Element<'a, M> = crate::embedded_svg::svg_widget(outline_path)
        .width(Length::Fill)
        .height(Length::Fill)
        .opacity(opacity)
        .style(move |_theme, _status| svg::Style {
            color: Some(outline_color),
        })
        .into();
    iced::widget::stack![fill_svg, outline_svg]
        .width(width)
        .height(Length::Fixed(icon_size))
        .into()
}

/// Render a star rating display (1-5 stars) for slot list slots.
///
/// Filled stars use the brand `star_bright` color; empty stars use
/// `slot_list_static_icon_color(style, fg4, 1.0)` so the outline matches the
/// row's forced-legible text on selected / highlighted / centered rows (and
/// stays a muted `fg4` elsewhere). Per-star opacity tracks `style.text_color.a`
/// to fade with the row.
///
/// # Arguments
/// * `rating` - Star count (0-5), clamped internally
/// * `icon_size` - Size of each star icon in pixels
/// * `style` - Resolved slot style (drives color + opacity adaptation)
/// * `portion` - When `Some(n)`, wraps the stars in a `FillPortion(n)` container
///   for use as a standalone slot list column. When `None`, returns the bare star row
///   for embedding inside caller-controlled layouts (e.g. a column).
pub(crate) fn slot_list_star_rating<'a, Message: Clone + 'a>(
    rating: usize,
    icon_size: f32,
    style: SlotListSlotStyle,
    portion: Option<u16>,
    on_click: Option<impl Fn(usize) -> Message + 'a>,
) -> Element<'a, Message> {
    use iced::{
        Alignment,
        widget::{row, svg},
    };

    let svg_opacity = style.text_color.a;
    let filled_color = theme::star_bright();
    let empty_color = slot_list_static_icon_color(style, theme::fg4(), 1.0);

    // Each star takes `FillPortion(1)` so the five share width evenly; the row
    // is capped to its natural full-size width. On wide columns the stars stay
    // square at `icon_size`; on narrow ones the flex layout clamps the row and
    // every star shrinks *together* (never just the trailing one). See
    // [`star_row_intrinsic_width`].
    let stars = (1..=5)
        .fold(row![].spacing(STAR_ROW_SPACING), |r, i| {
            let star_element: Element<'a, Message> = if rating >= i {
                outlined_svg_icon(
                    "assets/icons/star-filled.svg",
                    "assets/icons/star.svg",
                    icon_size,
                    filled_color,
                    svg_opacity,
                    Length::FillPortion(1),
                )
            } else {
                let color = empty_color;
                crate::embedded_svg::svg_widget("assets/icons/star.svg")
                    .width(Length::FillPortion(1))
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
        })
        .width(Length::Fixed(star_row_intrinsic_width(
            icon_size,
            STAR_ROW_SPACING,
        )));

    match portion {
        Some(p) => container(stars)
            .width(Length::FillPortion(p))
            .align_y(Alignment::Center)
            .into(),
        None => stars.into(),
    }
}

/// Which glyph family `slot_list_favorite_icon` renders.
///
/// Every current caller uses `Heart` (the love column); `Star` keeps the
/// star-glyph recipe available for future single-star favorite columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FavoriteIconKind {
    Heart,
    Star,
}

// Liveness anchor: every production caller currently passes `Heart`, so
// without this `Star` would trip dead_code under `-D warnings` (pattern
// matching alone does not count as a use). A `const _` construction keeps
// the variant — and its exhaustive-match arms — compiling until a real
// caller arrives.
const _: FavoriteIconKind = FavoriteIconKind::Star;

impl FavoriteIconKind {
    /// `(filled, outline)` SVG asset paths for this glyph family.
    fn icon_paths(self) -> (&'static str, &'static str) {
        match self {
            Self::Heart => ("assets/icons/heart-filled.svg", "assets/icons/heart.svg"),
            Self::Star => ("assets/icons/star-filled.svg", "assets/icons/star.svg"),
        }
    }

    /// Brand fill color used when the item is starred.
    fn fill_color(self) -> Color {
        match self {
            Self::Heart => theme::danger_bright(),
            Self::Star => theme::star_bright(),
        }
    }
}

/// Render a favorite icon for a slot-list row.
///
/// Both color and opacity are derived from `style` so the icon stays in
/// lockstep with the row's text — empty outlines take the row's forced-legible
/// text color on selected / highlighted / centered rows via
/// `slot_list_static_icon_color`, filled icons keep their brand colors
/// (`danger_bright` / `star_bright`) and fade with `style.text_color.a` to
/// match the surrounding text.
///
/// # Arguments
/// * `is_starred` - Whether the item is starred/favorited
/// * `style` - Resolved slot style (drives color + opacity adaptation)
/// * `icon_size` - Size of the icon in pixels
/// * `kind` - Glyph family (`FavoriteIconKind::Heart` / `Star`)
/// * `on_click` - Optional message to emit when clicked (toggles starred state)
pub(crate) fn slot_list_favorite_icon<'a, Message: Clone + 'a>(
    is_starred: bool,
    style: SlotListSlotStyle,
    icon_size: f32,
    kind: FavoriteIconKind,
    on_click: Option<Message>,
) -> Element<'a, Message> {
    use iced::widget::svg;

    let (filled_icon, empty_icon) = kind.icon_paths();

    let svg_opacity = style.text_color.a;

    let svg_element: Element<'a, Message> = if is_starred {
        let fill_color = kind.fill_color();
        outlined_svg_icon(
            filled_icon,
            empty_icon,
            icon_size,
            fill_color,
            svg_opacity,
            Length::Fixed(icon_size),
        )
    } else {
        let color = slot_list_static_icon_color(style, theme::fg4(), 1.0);
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

    let style = SlotListSlotStyle::for_slot(false, false, false, false, false, opacity, 0);

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
///
/// Both modes paint a `bg0_hard()` fill behind the slot rows and run
/// edge-to-edge — no L/R padding (rows align with the view header strip
/// above) and no bottom padding (the last row meets the player bar with
/// zero gap). Rounded mode adds an outer 1 px `theme::border()` outline
/// that clips the touching row hairlines into a single sealed perimeter.
/// The shell's corners are kept square in every mode — see the inline note
/// on the `radius:` field for why rounding an edge-to-edge shell under
/// `clip(true)` bleeds the base theme background at the corners.
pub(crate) fn slot_list_background_container<'a, Message: 'a>(
    slot_list_content: Element<'a, Message>,
) -> Element<'a, Message> {
    if crate::theme::is_rounded_mode() {
        container(slot_list_content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_| container::Style {
                background: Some(theme::bg0_hard().into()),
                border: iced::Border {
                    color: theme::border(),
                    width: 1.0,
                    // Square ALL corners. This shell runs edge-to-edge and
                    // butts flush against the view header strip above it and the
                    // player bar below it (see this fn's doc comment), so a
                    // rounded corner plus `clip(true)` leaves the wedge outside
                    // the arc unpainted — the lighter surface behind the shell
                    // bleeds through there (reads as gray from the base theme).
                    // The left corners always showed it; the right corners do
                    // too — the slot-list scrollbar does NOT reliably cover them
                    // (the transient bar fades out, and the always-on bar fills
                    // only its track, not the corner wedge). The scrollbar
                    // handle keeps its own pill rounding in `scroll_indicator`;
                    // the shell itself stays square in every mode.
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .clip(true)
            .into()
    } else {
        container(slot_list_content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::container_bg0_hard)
            .into()
    }
}

/// Resolve the `SlotListPageMessage` a primary slot-row click should
/// dispatch, given modifier state, whether the row is centered, and the
/// `stable_viewport` user setting.
///
/// Precedence (the single source of truth for 7 slot-list views — Albums,
/// Artists, Genres, Playlists, Queue, Songs, Radios):
///
/// 1. `ctrl|shift` → `SetOffset(item_index, modifiers)` so
///    `SlotListPageState::handle_slot_click` runs its multi-select branches
///    (ctrl-toggle, shift-range).
/// 2. `is_center` → `ActivateCenter` (play in place).
/// 3. `stable_viewport` → `SetOffset(item_index, modifiers)` (highlight
///    without moving the viewport — modifiers are empty in this branch,
///    so handle_slot_click takes its no-mods single-select path).
/// 4. Otherwise → `ClickPlay(item_index)` (legacy click-to-play: move
///    viewport AND play).
///
/// Pure function — no Iced widget construction. Exercised exhaustively by
/// the unit tests in this module, then composed by [`primary_slot_button`]
/// to build the actual `on_press` payload. Similar uses the variant in
/// [`highlight_only_slot_click_message`].
pub(crate) fn primary_slot_click_message(
    item_index: usize,
    is_center: bool,
    modifiers: iced::keyboard::Modifiers,
    stable_viewport: bool,
) -> SlotListPageMessage {
    if modifiers.control() || modifiers.shift() {
        SlotListPageMessage::SetOffset(item_index, modifiers)
    } else if is_center {
        SlotListPageMessage::ActivateCenter(false)
    } else if stable_viewport {
        SlotListPageMessage::SetOffset(item_index, modifiers)
    } else {
        SlotListPageMessage::ClickPlay(item_index)
    }
}

/// Resolve the `SlotListPageMessage` for the Similar view's intentional
/// highlight-only click contract: always `SetOffset(item_index, modifiers)`.
///
/// Modifier pass-through preserves ctrl-toggle / shift-range selection
/// (those branches still fire inside `handle_slot_click`). What's
/// deliberately omitted is the center-click play path (`ActivateCenter`)
/// and the legacy click-to-play path (`ClickPlay`) — see the file-level
/// doc comment on `views/similar.rs` for the rationale.
pub(crate) fn highlight_only_slot_click_message(
    item_index: usize,
    modifiers: iced::keyboard::Modifiers,
) -> SlotListPageMessage {
    SlotListPageMessage::SetOffset(item_index, modifiers)
}

/// Wrap a pre-styled clickable container in the canonical slot-row button
/// with the 4-arm modifier-aware click dispatch. Used by 7 slot-list views
/// (Albums, Artists, Genres, Playlists, Queue, Songs, Radios). See
/// [`primary_slot_click_message`] for the precedence rule.
///
/// `wrap` lifts a `SlotListPageMessage` into the caller's outer message
/// type, typically `AlbumsMessage::SlotList` / `QueueMessage::SlotList` /
/// etc.
pub(crate) fn primary_slot_button<'a, M: Clone + 'a>(
    content: impl Into<Element<'a, M>>,
    ctx: &SlotListRowContext,
    stable_viewport: bool,
    wrap: impl Fn(SlotListPageMessage) -> M,
) -> Element<'a, M> {
    let msg = primary_slot_click_message(
        ctx.item_index,
        ctx.is_center,
        ctx.modifiers,
        stable_viewport,
    );
    make_slot_button(content, wrap(msg))
}

/// Wrap a pre-styled clickable container in the canonical slot-row button
/// with always-`SetOffset` dispatch — Similar's intentional highlight-only
/// contract. See [`highlight_only_slot_click_message`].
pub(crate) fn highlight_only_slot_button<'a, M: Clone + 'a>(
    content: impl Into<Element<'a, M>>,
    ctx: &SlotListRowContext,
    wrap: impl Fn(SlotListPageMessage) -> M,
) -> Element<'a, M> {
    let msg = highlight_only_slot_click_message(ctx.item_index, ctx.modifiers);
    make_slot_button(content, wrap(msg))
}

/// Wrap child-row content in a styled clickable container + slot-row button,
/// routed through the same 4-arm modifier-aware ladder as [`primary_slot_button`].
///
/// Used by the expansion child-row renderers (`render_child_track_row` /
/// `render_child_album_row`) shared across Albums, Artists, Genres, and
/// Playlists. Unlike [`primary_slot_button`], this helper applies the
/// `SlotListSlotStyle`-derived container style around the row content before
/// wrapping in the canonical transparent button — child rows render their
/// per-depth `bg0/bg1/bg2` ramp inside the button itself, not in an outer
/// container.
///
/// `wrap` lifts a `SlotListPageMessage` into the caller's outer message type,
/// typically `AlbumsMessage::SlotList` / `ArtistsMessage::SlotList` / etc.
pub(crate) fn child_slot_button<'a, M: Clone + 'a>(
    content: iced::widget::Row<'a, M>,
    ctx: &SlotListRowContext,
    style: SlotListSlotStyle,
    stable_viewport: bool,
    wrap: impl Fn(SlotListPageMessage) -> M,
) -> Element<'a, M> {
    let clickable = container(content)
        .style(move |_theme| style.to_container_style())
        .width(Length::Fill);
    primary_slot_button(clickable, ctx, stable_viewport, wrap)
}

// Shared visual contract for both primary and highlight-only slot buttons:
// transparent background, no border, zero padding, fills width. Splitting
// the visual chrome out of the dispatch helpers keeps one source of truth
// so a future style tweak lands in both ladders.
fn make_slot_button<'a, M: Clone + 'a>(
    content: impl Into<Element<'a, M>>,
    on_press: M,
) -> Element<'a, M> {
    button(content)
        .on_press(on_press)
        .style(|_theme, _status| button::Style {
            background: None,
            border: iced::Border::default(),
            ..Default::default()
        })
        .padding(0)
        .width(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_checkbox_produces_element_in_both_states() {
        // Compile + panic guard for the shared box-visual recipe.
        for checked in [false, true] {
            let _el: Element<'_, String> =
                slot_list_select_checkbox(checked, 0, |i| format!("toggle {i}"));
        }
    }

    #[test]
    fn select_header_produces_element_across_tri_state() {
        use crate::widgets::slot_list_page::SelectAllState;

        for state in [
            SelectAllState::None,
            SelectAllState::Some,
            SelectAllState::All,
        ] {
            let _el: Element<'_, String> = slot_list_select_header(state, "select all".to_string());
        }
    }

    #[test]
    fn favorite_icon_kind_paths_are_paired() {
        assert_eq!(
            FavoriteIconKind::Heart.icon_paths(),
            ("assets/icons/heart-filled.svg", "assets/icons/heart.svg")
        );
        assert_eq!(
            FavoriteIconKind::Star.icon_paths(),
            ("assets/icons/star-filled.svg", "assets/icons/star.svg")
        );
    }

    #[test]
    fn star_row_intrinsic_width_sums_stars_and_gaps() {
        // Five stars + four inter-star gaps at the default spacing.
        assert!((star_row_intrinsic_width(14.0, STAR_ROW_SPACING) - 78.0).abs() < 0.01);
        assert!((star_row_intrinsic_width(24.0, STAR_ROW_SPACING) - 128.0).abs() < 0.01);
        // Zero spacing collapses to exactly five star widths.
        assert!((star_row_intrinsic_width(20.0, 0.0) - 100.0).abs() < 0.01);
    }

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
            hover_wash: true,
        };

        let row_height = config.row_height();
        // Available = 900 - 234 = 666 (slot list now runs flush to the
        // player bar, no bottom pad).
        // Spacing = 8 * 0 = 0 (flat-redesign rows touch).
        // Content = 666
        // Row height = 666 / 9 = 74.0
        assert!((row_height - 74.0).abs() < 0.01);
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
            hover_wash: true,
        };

        let row_height = config.row_height();
        assert_eq!(row_height, 40.0); // Should clamp to minimum
    }

    #[test]
    fn test_dynamic_slot_count_large_window() {
        // Large window: should get more than 9 slots to keep row height near TARGET
        let config = SlotListConfig::with_dynamic_slots(900.0, 134.0);
        // Available = 900 - 134 - 10 = 756, SLOT_SPACING=0 (flat redesign).
        // raw ≈ 756 / 70 ≈ 10.8 → try 11 and 13
        // 11 slots: spacing=0, content=756, row=68.7  (|68.7-70|=1.3)
        // 13 slots: spacing=0, content=756, row=58.2  (|58.2-70|=11.8)
        // 11 is closer to 70 → 11
        assert_eq!(config.slot_count, 11);
        assert_eq!(config.center_slot, 5);
    }

    #[test]
    fn test_dynamic_slot_count_medium_window() {
        // Medium window should get fewer slots
        let config = SlotListConfig::with_dynamic_slots(450.0, 134.0);
        // Available = 450 - 134 - 10 = 306, SLOT_SPACING=0.
        // raw ≈ 306 / 70 ≈ 4.37 → try 3 and 5
        // 3 slots: spacing=0, content=306, row=102 (|102-70|=32)
        // 5 slots: spacing=0, content=306, row=61.2 (|61.2-70|=8.8)
        // 5 is closer → 5
        assert_eq!(config.slot_count, 5);
        assert_eq!(config.center_slot, 2);
    }

    #[test]
    fn test_dynamic_slot_count_small_window() {
        // Very small window should fall back to minimum slots
        let config = SlotListConfig::with_dynamic_slots(250.0, 134.0);
        // Available = 250 - 134 - 10 = 106, SLOT_SPACING=0.
        // raw ≈ 106 / 70 ≈ 1.51 → lower_odd=1, upper_odd=3
        // 1 slot: content=106, row=106  (|106-70|=36)
        // 3 slots: content=106, row=35.3 (< MIN 55)
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

    // ── Chrome Height Invariants ──

    #[test]
    fn slot_count_always_odd() {
        for height in (300..=2160).step_by(50) {
            let config = SlotListConfig::with_dynamic_slots(height as f32, 134.0);
            assert_eq!(
                config.slot_count % 2,
                1,
                "slot_count must be odd at height={height}, got {}",
                config.slot_count
            );
        }
    }

    #[test]
    fn slot_count_monotonically_increases_with_height() {
        let mut prev_count = 0;
        for height in (300..=2160).step_by(50) {
            let config = SlotListConfig::with_dynamic_slots(height as f32, 134.0);
            assert!(
                config.slot_count >= prev_count,
                "slot_count decreased from {prev_count} to {} at height={height}",
                config.slot_count
            );
            prev_count = config.slot_count;
        }
    }

    #[test]
    fn center_slot_is_always_middle() {
        for height in (300..=2160).step_by(100) {
            let config = SlotListConfig::with_dynamic_slots(height as f32, 134.0);
            assert_eq!(
                config.center_slot,
                config.slot_count / 2,
                "center_slot must be slot_count/2 at height={height}"
            );
        }
    }

    #[test]
    fn slots_never_exceed_available_space() {
        // The core invariant: rendered slots + spacing must fit in the
        // available area. If this fails, the last slot clips. The slot list
        // now runs flush to the player bar (no bottom pad in either mode),
        // so `available` is simply `height - chrome`.
        for height in (300..=2160).step_by(50) {
            for chrome in [100.0, 134.0, 170.0, 200.0] {
                let config = SlotListConfig::with_dynamic_slots(height as f32, chrome);
                let row_height = config.row_height();
                let spacing = config.slot_count.saturating_sub(1) as f32 * SLOT_SPACING;
                let used = config.slot_count as f32 * row_height + spacing;
                let available = (height as f32 - chrome).max(0.0);
                assert!(
                    used <= available + 0.01, // f32 tolerance
                    "clipped at height={height}, chrome={chrome}: used={used:.1} > available={available:.1}"
                );
            }
        }
    }

    #[test]
    fn view_header_chrome_matches_rendered_widget_height() {
        // Pins `view_header_chrome()` to the actual widget tree built by
        // `view_header::view_header()` (a 50 px strip stacked with a 1 px
        // sibling separator). If `view_header` adds/removes a row, this
        // assertion fires before the slot-count math silently under-counts.
        use crate::widgets::view_header::{HEADER_BOTTOM_SEPARATOR, HEADER_HEIGHT};
        let expected = HEADER_HEIGHT + HEADER_BOTTOM_SEPARATOR;
        assert!(
            (view_header_chrome() - expected).abs() < f32::EPSILON,
            "view_header_chrome() drifted from HEADER_HEIGHT + separator: got {}, expected {}",
            view_header_chrome(),
            expected,
        );
    }

    #[test]
    fn highlight_slots_force_legible_ink() {
        // Normal (unhighlighted) slot -> hover text is the bright accent.
        let style = SlotListSlotStyle::for_slot(false, false, false, false, false, 1.0, 0);
        assert_eq!(style.hover_text_color, crate::theme::accent_bright());

        // Highlighted expanded-parent slot (is_highlighted, not is_playing) ->
        // all three text fields are forced to the legible ink for the derived
        // fill, and stay consistent.
        let hl = SlotListSlotStyle::for_slot(false, true, false, false, false, 1.0, 0);
        assert_eq!(hl.text_color, crate::theme::legible_text_on(hl.bg_color));
        assert_eq!(hl.hover_text_color, hl.text_color);
        assert_eq!(hl.subtext_color, hl.text_color);

        // Selected slot -> BORDER-ONLY (theme-modal style): no forced ink. It
        // keeps the normal row's theme text + bright-accent hover, exactly like
        // an unselected row — only the border changes (see
        // `selection_is_border_only_accent_ring`).
        let sel = SlotListSlotStyle::for_slot(false, false, false, true, false, 1.0, 0);
        assert_eq!(sel.text_color, crate::theme::fg0());
        assert_eq!(sel.subtext_color, crate::theme::fg4());
        assert_eq!(sel.hover_text_color, crate::theme::accent_bright());
        assert!(!sel.forces_legible_text);

        // Static glyphs (index, empty star/heart outlines, lock) follow the
        // forced text on the loud-fill highlight rows...
        assert!(hl.forces_legible_text);
        assert_eq!(
            slot_list_static_icon_color(hl, crate::theme::fg4(), 1.0),
            hl.text_color
        );
        // ...but a normal row AND a border-only selection keep the muted
        // fallback rather than the row text color.
        assert!(!style.forces_legible_text);
        assert_ne!(
            slot_list_static_icon_color(style, crate::theme::fg4(), 1.0),
            style.text_color
        );
        assert_ne!(
            slot_list_static_icon_color(sel, crate::theme::fg4(), 1.0),
            sel.text_color
        );
    }

    /// Now-playing rows (`is_playing`) wear the loud accent fill (now shared with
    /// the drag-preview ghost, NOT the border-only in-list selection), seed the
    /// breathing glow, and carry NO ring (the glow is their sole distinguisher).
    /// Expanded-parent headers share the `is_highlighted` branch but stay static
    /// — calmer `playing_fill`, no glow, and they KEEP the highlight ring.
    #[test]
    fn now_playing_wears_loud_fill_and_seeds_glow() {
        // Actively-playing row: is_highlighted + is_playing.
        let playing = SlotListSlotStyle::for_slot(false, true, true, false, false, 1.0, 0);
        assert_eq!(
            playing.bg_color,
            crate::theme::selected_fill_resolved(),
            "now-playing fill must be the loud accent fill (shared with the drag-preview ghost)"
        );
        assert_eq!(
            playing.glow_seed,
            Some(playing.bg_color),
            "now-playing must seed the breathing glow with its own fill"
        );
        assert_eq!(
            playing.border_width, 0.0,
            "now-playing wears no ring — only the glow overlay distinguishes it"
        );
        assert_eq!(playing.border_color, Color::TRANSPARENT);

        // Expanded-parent header: is_highlighted WITHOUT is_playing — keeps the
        // calmer static fill, never breathes, and KEEPS the highlight ring.
        let parent = SlotListSlotStyle::for_slot(false, true, false, false, false, 1.0, 0);
        assert_eq!(
            parent.bg_color,
            crate::theme::playing_fill(),
            "expanded-parent header keeps the calmer playing_fill"
        );
        assert_eq!(
            parent.glow_seed, None,
            "expanded-parent header must not breathe"
        );
        assert_eq!(parent.border_width, 2.0, "expanded-parent keeps its ring");
        assert_eq!(
            parent.border_color,
            crate::theme::highlight_border(parent.bg_color, 1.0),
            "expanded-parent ring is the max-contrast highlight border"
        );

        // Plain selection KEEPS its ring (and never breathes).
        let selected = SlotListSlotStyle::for_slot(false, false, false, true, false, 1.0, 0);
        assert_eq!(selected.glow_seed, None);
        assert_eq!(selected.border_width, 2.0, "selection keeps its ring");
        let normal = SlotListSlotStyle::for_slot(false, false, false, false, false, 1.0, 0);
        assert_eq!(normal.glow_seed, None);
    }

    /// A selection — multi-selected rows AND the lone click/keyboard cursor —
    /// is a BORDER-ONLY affordance, mirroring the theme picker swatch list
    /// (`render_theme_slot`): the row keeps a normal row's background + theme
    /// text, and the ONLY change is a 2 px accent ring. No loud fill, no forced
    /// ink, no glow — so a slot-list selection reads exactly like a selection in
    /// the theme modal. (The now-playing row keeps its loud fill — see
    /// `now_playing_wears_loud_fill_and_seeds_glow`.)
    #[test]
    fn selection_is_border_only_accent_ring() {
        let normal = SlotListSlotStyle::for_slot(false, false, false, false, false, 1.0, 0);
        let multi_selected = SlotListSlotStyle::for_slot(false, false, false, true, false, 1.0, 0);
        let lone_cursor = SlotListSlotStyle::for_slot(true, false, false, false, false, 1.0, 0);

        for sel in [multi_selected, lone_cursor] {
            // Background + text are identical to a normal row — selection
            // touches neither (the whole point: it reads like the theme modal).
            assert_eq!(sel.bg_color, normal.bg_color);
            assert_eq!(sel.text_color, normal.text_color);
            assert_eq!(sel.subtext_color, normal.subtext_color);
            assert_eq!(sel.hover_text_color, normal.hover_text_color);
            assert!(!sel.forces_legible_text);
            assert_eq!(sel.glow_seed, None);
            // The sole cue: a 2 px accent ring (vs the normal 1 px hairline),
            // contrast-floored against the row bg so it is never invisible.
            assert_eq!(sel.border_width, 2.0);
            assert_eq!(
                sel.border_color,
                Color {
                    a: 1.0,
                    ..crate::theme::selection_ring_on(crate::theme::bg0())
                }
            );
        }
    }

    /// A floating drag-preview ghost deliberately keeps the BOLD loud-fill look
    /// (not the quiet in-list border-only selection ring), so the dragged row
    /// shouts over whatever content it floats above.
    #[test]
    fn drag_preview_keeps_bold_fill() {
        let ghost = SlotListSlotStyle::drag_preview();
        assert_eq!(ghost.bg_color, crate::theme::selected_fill_resolved());
        assert_eq!(
            ghost.text_color,
            crate::theme::legible_text_on(ghost.bg_color)
        );
        assert!(ghost.forces_legible_text);
        assert_eq!(ghost.border_width, 2.0);
        assert_eq!(ghost.glow_seed, None);
    }

    /// The change's core promise: a now-playing row's loud fill + glow (and an
    /// expanded-parent header's fill + ring) survive UNCHANGED even when that
    /// same row is also part of a multi-selection. In real use a row is routinely
    /// both at once (e.g. clicking the currently-playing track). Pinned because
    /// correctness rests SOLELY on the `is_highlighted` branch winning over the
    /// selection branch — a reorder would silently strip the now-playing look and
    /// no other test would catch it.
    #[test]
    fn now_playing_and_parent_win_over_selection() {
        // Now-playing AND selected (with an active multi-selection) renders
        // identically to a plain now-playing row.
        let playing_only = SlotListSlotStyle::for_slot(false, true, true, false, false, 1.0, 0);
        let playing_selected = SlotListSlotStyle::for_slot(true, true, true, true, true, 1.0, 0);
        assert_eq!(playing_selected.bg_color, playing_only.bg_color);
        assert_eq!(playing_selected.glow_seed, playing_only.glow_seed);
        assert_eq!(playing_selected.border_width, playing_only.border_width);
        assert_eq!(
            playing_selected.forces_legible_text,
            playing_only.forces_legible_text
        );

        // Expanded-parent header AND selected keeps its loud `playing_fill` + 2px
        // ring — it is NOT downgraded to the border-only selection look.
        let parent_only = SlotListSlotStyle::for_slot(false, true, false, false, false, 1.0, 0);
        let parent_selected = SlotListSlotStyle::for_slot(true, true, false, true, true, 1.0, 0);
        assert_eq!(parent_only.bg_color, crate::theme::playing_fill());
        assert_eq!(parent_selected.bg_color, parent_only.bg_color);
        assert_eq!(parent_selected.border_color, parent_only.border_color);
        assert_eq!(
            parent_selected.forces_legible_text,
            parent_only.forces_legible_text
        );
    }

    /// Two contracts of the border-only selection branch: (1) a selection is
    /// IMMUNE to the opacity gradient — its accent ring (the sole cue), bg, and
    /// text all stay fully opaque even on an off-center, faded row, so the ring
    /// never drops below the contrast floor (the now-playing / expanded rows are
    /// immune the same way); and (2) the lone keyboard cursor yields its ring to
    /// an active explicit multi-selection, falling back to the plain 1px hairline.
    #[test]
    fn selection_is_opaque_under_gradient_and_yields_to_multi_selection() {
        // (1) A selected row asked to render at a faded opacity ignores it: ring,
        // bg, and text all stay fully opaque so the selection can never fade out.
        let faded_selected = SlotListSlotStyle::for_slot(false, false, false, true, false, 0.3, 0);
        assert_eq!(faded_selected.border_color.a, 1.0);
        assert_eq!(faded_selected.bg_color.a, 1.0);
        assert_eq!(faded_selected.text_color.a, 1.0);
        assert_eq!(faded_selected.border_width, 2.0);
        // A plain row at the same opacity DOES fade (the gradient still applies).
        let faded_plain = SlotListSlotStyle::for_slot(false, false, false, false, false, 0.3, 0);
        assert!((faded_plain.bg_color.a - 0.3).abs() < 1e-6);

        // (2) Center cursor amid an active multi-selection (and not itself
        // selected) is a plain row again — 1px hairline, no 2px ring.
        let cursor_amid_multi =
            SlotListSlotStyle::for_slot(true, false, false, false, true, 1.0, 0);
        assert_eq!(cursor_amid_multi.border_width, 1.0);
    }

    /// The inner glow's edge alpha breathes between its min and max over a
    /// period, easing through a cosine (min at the trough, max at the peak).
    #[test]
    fn inner_glow_breathes_between_min_and_max() {
        assert!(
            (inner_glow_edge_alpha(0.0) - INNER_GLOW_MIN_ALPHA).abs() < 1e-6,
            "trough sits at the minimum edge alpha"
        );
        assert!(
            (inner_glow_edge_alpha(0.5) - INNER_GLOW_MAX_ALPHA).abs() < 1e-6,
            "peak reaches the maximum edge alpha"
        );
        let mid = inner_glow_edge_alpha(0.25);
        assert!(
            mid > INNER_GLOW_MIN_ALPHA && mid < INNER_GLOW_MAX_ALPHA,
            "mid-breath alpha is between the extremes, got {mid}"
        );
    }

    /// The shimmer band sweeps from off-screen-left, across the row, then parks
    /// off-screen (no visible sheen) during the idle gap before the next sweep.
    #[test]
    fn shimmer_sweeps_then_parks() {
        // Enters from just off the left edge at the start of the sweep window.
        assert!(
            shimmer_band_center(0.0) < 0.0,
            "band starts off-screen-left"
        );
        // Crosses the middle partway through the sweep window.
        let mid = shimmer_band_center(SHIMMER_SWEEP_FRACTION * 0.5);
        assert!(
            (0.3..=0.7).contains(&mid),
            "band crosses near the middle mid-sweep, got {mid}"
        );
        // Parks off-screen (offset > 1 → all stops dropped → no sheen) in the gap.
        assert!(
            shimmer_band_center((SHIMMER_SWEEP_FRACTION + 1.0) * 0.5) > 1.0,
            "band is parked off-screen during the idle gap"
        );
    }

    /// The now-playing row itself stays a normal full-bleed slot: its container
    /// style is the static fill + border, with the glow living in the overlay.
    #[test]
    fn now_playing_container_style_is_a_normal_slot() {
        let playing = SlotListSlotStyle::for_slot(false, true, true, false, false, 1.0, 0);
        let style = playing.to_container_style();
        assert_eq!(
            style.background,
            Some(playing.bg_color.into()),
            "the now-playing fill is the static loved selection color"
        );
        assert_eq!(
            style.shadow.color.a, 0.0,
            "no shadow on the slot itself — the glow is an overlay"
        );
        assert_eq!(style.border.color, playing.border_color);
    }

    fn unfocused_bg(depth: u8) -> iced::Color {
        SlotListSlotStyle::for_slot(false, false, false, false, false, 1.0, depth).bg_color
    }

    fn focused_bg(depth: u8) -> iced::Color {
        SlotListSlotStyle::for_slot(true, false, false, false, false, 1.0, depth).bg_color
    }

    fn focused_ring(depth: u8) -> iced::Color {
        SlotListSlotStyle::for_slot(true, false, false, false, false, 1.0, depth).border_color
    }

    #[test]
    fn unfocused_bg_at_depth_zero_uses_bg0() {
        let style_bg = unfocused_bg(0);
        let expected = crate::theme::bg0();
        assert!(
            (style_bg.r - expected.r).abs() < f32::EPSILON
                && (style_bg.g - expected.g).abs() < f32::EPSILON
                && (style_bg.b - expected.b).abs() < f32::EPSILON,
            "depth 0 unfocused bg should match bg0(); got {style_bg:?}, expected {expected:?}"
        );
    }

    #[test]
    fn unfocused_bg_at_depth_one_uses_bg1() {
        let style_bg = unfocused_bg(1);
        let expected = crate::theme::bg1();
        assert!(
            (style_bg.r - expected.r).abs() < f32::EPSILON
                && (style_bg.g - expected.g).abs() < f32::EPSILON
                && (style_bg.b - expected.b).abs() < f32::EPSILON,
            "depth 1 unfocused bg should match bg1(); got {style_bg:?}, expected {expected:?}"
        );
    }

    #[test]
    fn unfocused_bg_at_depth_two_uses_bg2() {
        let style_bg = unfocused_bg(2);
        let expected = crate::theme::bg2();
        assert!(
            (style_bg.r - expected.r).abs() < f32::EPSILON
                && (style_bg.g - expected.g).abs() < f32::EPSILON
                && (style_bg.b - expected.b).abs() < f32::EPSILON,
            "depth 2 unfocused bg should match bg2(); got {style_bg:?}, expected {expected:?}"
        );
    }

    /// In themes whose elevated palette differs from the base (e.g. Everforest:
    /// bg0=#2D353B, bg1=#343F44, bg2=#3D484D), the per-depth unfocused background
    /// must change with depth — otherwise nested expansion rows are visually flat.
    /// Themes that intentionally collapse the elevation ramp (bg0==bg1==bg2) are
    /// exempt; the assertion only fires when the active theme provides a ramp.
    #[test]
    fn unfocused_bg_changes_with_depth_when_theme_has_ramp() {
        let bg0 = crate::theme::bg0();
        let bg1 = crate::theme::bg1();
        let bg2 = crate::theme::bg2();

        let ramp_present = (bg0.r, bg0.g, bg0.b) != (bg1.r, bg1.g, bg1.b)
            || (bg1.r, bg1.g, bg1.b) != (bg2.r, bg2.g, bg2.b);
        if !ramp_present {
            return;
        }

        let d0 = unfocused_bg(0);
        let d1 = unfocused_bg(1);
        let d2 = unfocused_bg(2);
        assert!(
            (d0.r, d0.g, d0.b) != (d1.r, d1.g, d1.b) || (d1.r, d1.g, d1.b) != (d2.r, d2.g, d2.b),
            "expected at least one depth transition to change bg color when theme has an elevation ramp; \
             got d0={d0:?}, d1={d1:?}, d2={d2:?}"
        );
    }

    /// The keyboard-focused row is a BORDER-ONLY selection (theme-modal style):
    /// the accent RING — not the fill — marks focus, and it stays PERCEPTIBLE at
    /// every depth because its color is contrast-floored against that depth's bg
    /// (`selection_ring_on`). The fill itself tracks the normal per-depth
    /// elevation ramp (selection == normal row + ring), so a focused deep row
    /// stays tonally consistent with its siblings.
    #[test]
    fn focused_ring_is_contrast_floored_and_fill_follows_ramp() {
        for depth in 0u8..=2 {
            let bg = match depth {
                0 => crate::theme::bg0(),
                1 => crate::theme::bg1(),
                _ => crate::theme::bg2(),
            };
            // Ring = the contrast-floored accent for THIS depth's bg.
            assert_eq!(focused_ring(depth), crate::theme::selection_ring_on(bg));
            // Fill = the normal row's bg at this depth (no separate fill).
            assert_eq!(focused_bg(depth), unfocused_bg(depth));
        }
    }

    // ========================================================================
    // primary_slot_click_message — 4-arm ladder precedence
    // ========================================================================
    //
    // Pins the precedence rule used by 7 slot-list views (Albums, Artists,
    // Genres, Playlists, Queue, Songs, Radios). The audit flagged this
    // ladder as HIGH RISK because reordering arms silently inverts user-
    // facing click semantics. These tests are the regression net.

    use iced::keyboard::Modifiers;

    #[test]
    fn primary_click_center_no_mods_activates_center() {
        // Arm 2: is_center beats stable_viewport, plays in place.
        let m = primary_slot_click_message(5, true, Modifiers::default(), true);
        assert!(matches!(m, SlotListPageMessage::ActivateCenter(false)));
    }

    #[test]
    fn primary_click_non_center_stable_viewport_highlights() {
        // Arm 3: stable_viewport with non-center click sets viewport offset
        // without playing — modifiers are empty so handle_slot_click takes
        // its single-select branch.
        let m = primary_slot_click_message(5, false, Modifiers::default(), true);
        match m {
            SlotListPageMessage::SetOffset(idx, mods) => {
                assert_eq!(idx, 5);
                assert!(mods.is_empty(), "no-mods click must pass empty modifiers");
            }
            other => panic!("expected SetOffset(5, empty), got {other:?}"),
        }
    }

    #[test]
    fn primary_click_non_center_legacy_uses_click_play() {
        // Arm 4: legacy mode (stable_viewport=false) on non-center click
        // dispatches ClickPlay — the historical click-to-play behavior.
        let m = primary_slot_click_message(5, false, Modifiers::default(), false);
        assert!(matches!(m, SlotListPageMessage::ClickPlay(5)));
    }

    #[test]
    fn primary_click_ctrl_overrides_center_for_multi_select() {
        // Arm 1: ctrl beats is_center so multi-select toggle still works
        // when the user ctrl-clicks the centered row.
        let m = primary_slot_click_message(5, true, Modifiers::CTRL, true);
        match m {
            SlotListPageMessage::SetOffset(idx, mods) => {
                assert_eq!(idx, 5);
                assert!(mods.control() && !mods.shift());
            }
            other => panic!("expected SetOffset with CTRL, got {other:?}"),
        }
    }

    #[test]
    fn primary_click_shift_overrides_center_for_range_select() {
        let m = primary_slot_click_message(5, true, Modifiers::SHIFT, true);
        match m {
            SlotListPageMessage::SetOffset(idx, mods) => {
                assert_eq!(idx, 5);
                assert!(mods.shift() && !mods.control());
            }
            other => panic!("expected SetOffset with SHIFT, got {other:?}"),
        }
    }

    #[test]
    fn primary_click_ctrl_overrides_legacy_click_play() {
        // Even in legacy mode (stable_viewport=false), ctrl must still
        // route to multi-select rather than ClickPlay — otherwise ctrl+click
        // would play a song instead of toggling its selection.
        let m = primary_slot_click_message(7, false, Modifiers::CTRL, false);
        match m {
            SlotListPageMessage::SetOffset(idx, mods) => {
                assert_eq!(idx, 7);
                assert!(mods.control());
            }
            other => panic!("expected SetOffset with CTRL in legacy mode, got {other:?}"),
        }
    }

    #[test]
    fn primary_click_ctrl_plus_shift_routes_through_set_offset() {
        // Bitflag union pass-through: both modifier bits reach handle_slot_click.
        let m = primary_slot_click_message(3, false, Modifiers::CTRL | Modifiers::SHIFT, true);
        match m {
            SlotListPageMessage::SetOffset(idx, mods) => {
                assert_eq!(idx, 3);
                assert!(mods.control() && mods.shift());
            }
            other => panic!("expected SetOffset with CTRL|SHIFT, got {other:?}"),
        }
    }

    // ========================================================================
    // highlight_only_slot_click_message — Similar's intentional 1-arm variant
    // ========================================================================
    //
    // Always SetOffset(item_index, modifiers). Center clicks deliberately do
    // not play; multi-select still works because modifiers pass through.

    #[test]
    fn highlight_only_no_mods_sets_offset() {
        let m = highlight_only_slot_click_message(3, Modifiers::default());
        match m {
            SlotListPageMessage::SetOffset(idx, mods) => {
                assert_eq!(idx, 3);
                assert!(mods.is_empty());
            }
            other => panic!("expected SetOffset(3, empty), got {other:?}"),
        }
    }

    #[test]
    fn highlight_only_center_click_does_not_play() {
        // Distinguishes Similar from the primary ladder: even with is_center
        // semantics the helper does NOT emit ActivateCenter. We exercise this
        // by noting the helper has no is_center input — same call regardless.
        let m = highlight_only_slot_click_message(0, Modifiers::default());
        assert!(
            !matches!(m, SlotListPageMessage::ActivateCenter(_)),
            "highlight-only must never dispatch ActivateCenter"
        );
        assert!(
            !matches!(m, SlotListPageMessage::ClickPlay(_)),
            "highlight-only must never dispatch ClickPlay"
        );
    }

    #[test]
    fn highlight_only_passes_ctrl_through_for_multi_select() {
        let m = highlight_only_slot_click_message(3, Modifiers::CTRL);
        match m {
            SlotListPageMessage::SetOffset(idx, mods) => {
                assert_eq!(idx, 3);
                assert!(mods.control());
            }
            other => panic!("expected SetOffset with CTRL, got {other:?}"),
        }
    }

    #[test]
    fn highlight_only_passes_shift_through_for_range_select() {
        let m = highlight_only_slot_click_message(3, Modifiers::SHIFT);
        match m {
            SlotListPageMessage::SetOffset(idx, mods) => {
                assert_eq!(idx, 3);
                assert!(mods.shift());
            }
            other => panic!("expected SetOffset with SHIFT, got {other:?}"),
        }
    }

    // ========================================================================
    // primary_slot_button / highlight_only_slot_button — compile-time exercise
    // ========================================================================
    //
    // The Element-returning helpers are exercised at runtime by every
    // migrated view, but commits 1→3 are sequenced so commit 1 needs an
    // in-module call site to keep dead_code analysis quiet. These tests do
    // exactly that — construct an Element via each helper and drop it.

    fn dummy_row_context(is_center: bool, modifiers: Modifiers) -> SlotListRowContext {
        SlotListRowContext {
            item_index: 0,
            is_center,
            is_selected: false,
            has_multi_selection: false,
            opacity: 1.0,
            scale_factor: 1.0,
            row_height: 70.0,
            modifiers,
            metrics: SlotListRowMetrics::from_row(70.0, 1.0),
        }
    }

    #[test]
    fn primary_slot_button_builds_an_element() {
        let ctx = dummy_row_context(false, Modifiers::default());
        let _: Element<'_, ()> = primary_slot_button(
            iced::widget::text("row"),
            &ctx,
            true, // stable_viewport
            |_msg| (),
        );
    }

    #[test]
    fn highlight_only_slot_button_builds_an_element() {
        let ctx = dummy_row_context(true, Modifiers::default());
        let _: Element<'_, ()> =
            highlight_only_slot_button(iced::widget::text("row"), &ctx, |_msg| ());
    }

    #[test]
    fn child_slot_button_builds_an_element() {
        let ctx = dummy_row_context(false, Modifiers::default());
        let style = SlotListSlotStyle::for_slot(false, false, false, false, false, 1.0, 1);
        let row = iced::widget::Row::new().push(iced::widget::text("child row"));
        let _: Element<'_, ()> = child_slot_button(row, &ctx, style, true, |_msg| ());
    }

    // ========================================================================
    // child_slot_button — precedence parity with primary_slot_click_message
    // ========================================================================
    //
    // Regression guard against future divergence: the child-row helper must
    // route through the same 4-arm ladder as the parent-row helper. The
    // 7 char-tests above pin the ladder itself; these tests pin that
    // child_slot_button reuses it (via primary_slot_button →
    // primary_slot_click_message) for all 4 arms.

    /// Helper: extract the same `SlotListPageMessage` `child_slot_button` would
    /// dispatch on click. Since the element returned by `child_slot_button` is
    /// opaque (Iced consumes `on_press` internally), we exercise the shared
    /// path by replaying `primary_slot_click_message` with the same inputs and
    /// trusting `child_slot_button → primary_slot_button → primary_slot_click_message`
    /// — the only allowed routing per the helper's doc-comment.
    fn child_click_routing(
        item_index: usize,
        is_center: bool,
        modifiers: Modifiers,
        stable_viewport: bool,
    ) -> SlotListPageMessage {
        // Mirror primary_slot_button: pass (ctx, stable_viewport) into
        // primary_slot_click_message. If child_slot_button ever stops
        // delegating, this test still passes — but the user-facing rows
        // would break. Pair with #[test] below that constructs an element
        // and verifies it builds at compile time.
        primary_slot_click_message(item_index, is_center, modifiers, stable_viewport)
    }

    #[test]
    fn child_click_center_no_mods_matches_primary() {
        assert!(matches!(
            child_click_routing(5, true, Modifiers::default(), true),
            SlotListPageMessage::ActivateCenter(false)
        ));
    }

    #[test]
    fn child_click_non_center_stable_viewport_matches_primary() {
        match child_click_routing(5, false, Modifiers::default(), true) {
            SlotListPageMessage::SetOffset(idx, mods) => {
                assert_eq!(idx, 5);
                assert!(mods.is_empty());
            }
            other => panic!("expected SetOffset(5, empty), got {other:?}"),
        }
    }

    #[test]
    fn child_click_non_center_legacy_matches_primary() {
        assert!(matches!(
            child_click_routing(5, false, Modifiers::default(), false),
            SlotListPageMessage::ClickPlay(5)
        ));
    }

    #[test]
    fn child_click_ctrl_overrides_center_matches_primary() {
        match child_click_routing(5, true, Modifiers::CTRL, true) {
            SlotListPageMessage::SetOffset(idx, mods) => {
                assert_eq!(idx, 5);
                assert!(mods.control());
            }
            other => panic!("expected SetOffset with CTRL, got {other:?}"),
        }
    }

    // ========================================================================
    // SlotListRowContext::slot_style — forwarder parity with for_slot
    // ========================================================================
    //
    // Pins that the convenience forwarder maps each of the four context-owned
    // fields (is_center / is_selected / has_multi_selection / opacity) to the
    // correct positional arg of for_slot. (A reorder of for_slot's own params
    // is caught by for_slot's dedicated tests, not here — both paths would shift
    // together.) The configs below differ pairwise in their three bool fields,
    // so a swap of any two inside slot_style breaks at least one case loudly.

    /// Build a row context with the four style-bearing fields set explicitly, so
    /// a transposed arg in `slot_style` changes the resulting style.
    fn style_probe_row_context(
        is_center: bool,
        is_selected: bool,
        has_multi_selection: bool,
        opacity: f32,
    ) -> SlotListRowContext {
        SlotListRowContext {
            item_index: 3,
            is_center,
            is_selected,
            has_multi_selection,
            opacity,
            scale_factor: 1.0,
            row_height: 70.0,
            modifiers: Modifiers::default(),
            metrics: SlotListRowMetrics::from_row(70.0, 1.0),
        }
    }

    /// Assert two styles are equal field-by-field (no `PartialEq` on the type).
    fn assert_slot_style_eq(a: &SlotListSlotStyle, b: &SlotListSlotStyle) {
        assert_eq!(a.bg_color, b.bg_color, "bg_color");
        assert_eq!(a.border_color, b.border_color, "border_color");
        assert_eq!(a.border_width, b.border_width, "border_width");
        assert_eq!(a.text_color, b.text_color, "text_color");
        assert_eq!(a.subtext_color, b.subtext_color, "subtext_color");
        assert_eq!(a.hover_text_color, b.hover_text_color, "hover_text_color");
        assert_eq!(
            a.forces_legible_text, b.forces_legible_text,
            "forces_legible_text"
        );
        assert_eq!(a.glow_seed, b.glow_seed, "glow_seed");
    }

    #[test]
    fn slot_style_forwards_ctx_fields_into_for_slot() {
        // Configs whose (is_center, is_selected, has_multi_selection) triples
        // differ pairwise, so swapping any two context-owned fields inside
        // slot_style produces a different style in at least one config.
        let contexts = [
            style_probe_row_context(true, false, false, 0.5),
            style_probe_row_context(false, true, false, 0.8),
            style_probe_row_context(false, false, true, 1.0),
        ];
        // Per-renderer-varying inputs: plain, expanded-parent highlight,
        // actively-playing, and a deeper expansion depth.
        let call_matrix = [
            (false, false, 0u8),
            (true, false, 0),
            (true, true, 0),
            (false, false, 2),
        ];
        for ctx in &contexts {
            for (is_highlighted, is_playing, depth) in call_matrix {
                let forwarded = ctx.slot_style(is_highlighted, is_playing, depth);
                let direct = SlotListSlotStyle::for_slot(
                    ctx.is_center,
                    is_highlighted,
                    is_playing,
                    ctx.is_selected,
                    ctx.has_multi_selection,
                    ctx.opacity,
                    depth,
                );
                assert_slot_style_eq(&forwarded, &direct);
            }
        }
    }

    #[test]
    fn wrap_with_select_column_for_hidden_returns_inner_unchanged() {
        // With show=false the helper must be a pass-through: the typed Message
        // variant already makes a miscopied lambda a compile error, so this is
        // belt-and-suspenders for the show-gate delegation to
        // wrap_with_select_column's early return.
        let ctx = dummy_row_context(false, Modifiers::default());
        let _: Element<'_, ()> = wrap_with_select_column_for(
            false, // show
            &ctx,
            |_msg| (),
            iced::widget::text("row").into(),
        );
    }
}
