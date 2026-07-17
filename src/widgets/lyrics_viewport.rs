//! Over-cover synced-lyrics viewport: a non-interactive `advanced::Widget`
//! that draws the lyric column centered on the active line, plus the scrim
//! layer that keeps it legible over arbitrary album art.
//!
//! C1 = the static look (owner sign-off gate): per-doc uniform slot heights,
//! active line in `accent_bright()`, neighbors on an alpha falloff, a vertical
//! scrim gradient. Motion (eased scroll off the frame clock) lands in C2 —
//! `ease_out_expo` is defined here now so the math is test-pinned early.
//!
//! Event-transparency is load-bearing: `mouse_interaction` returns the default
//! and `update` never captures, so the artwork right-click context menu (and
//! left-click/scroll) pass through to the panel beneath.

use iced::{
    Color, Element, Length, Pixels, Rectangle, Size, Theme, Vector,
    advanced::{
        Renderer as _, layout, renderer,
        text::{
            self as advanced_text, Paragraph as ParagraphTrait, Renderer as TextRenderer, Text,
            paragraph::Plain,
        },
        widget::{self, Widget},
    },
    mouse,
    widget::text::Wrapping,
};
use nokkvi_data::types::lyrics::LrcLine;

use crate::theme;

/// Base lyric font size (logical px).
const LYRIC_FONT_SIZE: f32 = 16.0;
/// Fixed line height (absolute, so the slot math is exact).
const LINE_HEIGHT: f32 = 22.0;
/// Vertical padding inside each line slot.
const SLOT_PAD: f32 = 6.0;
/// Horizontal inset of the text column from the panel edges.
const H_PAD: f32 = 18.0;
/// Max wrapped rows a slot reserves; longer lines clip (honest trade — the
/// corpus p95 line is 54 chars, well inside 2 rows at typical panel widths).
const MAX_SLOT_ROWS: usize = 3;

/// Alpha falloff by distance from the active line (C1 static styling; C2 eases
/// between these as the column glides).
const ACTIVE_ALPHA: f32 = 1.0;
const NEAR_ALPHA: f32 = 0.72;
const MID_ALPHA: f32 = 0.5;
const FAR_ALPHA: f32 = 0.32;

/// Scrim strength: stronger at the panel edges (feathering neighbors away),
/// lighter at the center band where the active line sits.
const SCRIM_EDGE_ALPHA: f32 = 0.78;
const SCRIM_CENTER_ALPHA: f32 = 0.52;

/// Glyph halo: iced text has no shadow primitive, so every lyric fill is
/// preceded by offset re-fills of the SAME cached paragraph in `bg0_hard` —
/// each glyph carries its own contrast edge, making legibility independent of
/// the art behind it (the worst case is white lyric ink crossing white type
/// printed ON the cover, which no panel scrim can separate). Dual ring: a 1px
/// 8-direction rim (k=3 stem-edge compound `1-(1-a)³` ≈ 0.91 at full alpha)
/// plus a 2px 4-direction fringe whose low alpha turns "stroked text" into
/// "soft shadow". Tuning knob: if the halo ever reads as a comic outline,
/// lower `HALO_INNER_ALPHA` first (0.55 → 0.45) — never the geometry.
const HALO_INNER_ALPHA: f32 = 0.55;
const HALO_OUTER_ALPHA: f32 = 0.18;
/// Epsilon cull for deep-faded falloff / late-dissolve fills.
const HALO_MIN_FILL_ALPHA: f32 = 0.008;
const HALO_INNER_OFFSETS: [(f32, f32); 8] = [
    (-1.0, 0.0),
    (1.0, 0.0),
    (0.0, -1.0),
    (0.0, 1.0),
    (-1.0, -1.0),
    (1.0, -1.0),
    (-1.0, 1.0),
    (1.0, 1.0),
];
const HALO_OUTER_OFFSETS: [(f32, f32); 4] = [(-2.0, 0.0), (2.0, 0.0), (0.0, -2.0), (0.0, 2.0)];

/// Exponential ease-out — the glide curve driven by the per-frame boat tick.
pub(crate) fn ease_out_expo(t: f32) -> f32 {
    if t >= 1.0 {
        1.0
    } else if t <= 0.0 {
        0.0
    } else {
        1.0 - 2f32.powf(-10.0 * t)
    }
}

/// The column's live center position in slot-index space (e.g. `2.4` = 40 %
/// of the way from line 2 to line 3), published by the per-frame boat tick and
/// read in `draw()`. A process-global atomic — deliberately, NOT an
/// `Instant::now()` self-animation inside `draw()`: publishing off the tick is
/// what makes the motion assertable as observable state in `test_app` (the
/// `NOW_PLAYING_PHASE` precedent in `slot_list.rs`). Do not "simplify" away.
static LYRICS_CENTER_POS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Publish the eased center (called once per frame by the boat tick).
pub(crate) fn set_lyrics_center(pos: f32) {
    LYRICS_CENTER_POS.store(pos.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// Read the live eased center.
pub(crate) fn lyrics_center_pos() -> f32 {
    f32::from_bits(LYRICS_CENTER_POS.load(std::sync::atomic::Ordering::Relaxed))
}

/// Compute the eased center for a glide (pure — the boat tick feeds it the
/// state fields + `now`, then publishes the result).
pub(crate) fn eased_center(
    from: f32,
    to: f32,
    anim_start: Option<std::time::Instant>,
    duration_ms: u32,
    now: std::time::Instant,
) -> f32 {
    match anim_start {
        Some(start) if duration_ms > 0 => {
            let t =
                now.saturating_duration_since(start).as_secs_f32() * 1000.0 / duration_ms as f32;
            from + (to - from) * ease_out_expo(t)
        }
        _ => to,
    }
}

/// Borrowed per-render view data for the lyrics layer. `Copy` is load-bearing:
/// the artwork panel builds inside `responsive` `Fn` closures, which consume
/// the value on every call (the same reason `OverCoverBoat` is `Copy`).
#[derive(Clone, Copy)]
pub(crate) struct LyricsPanelData<'a> {
    pub lines: &'a [LrcLine],
    pub active_index: Option<usize>,
    /// Shown centered when `lines` is empty (no match for this track).
    pub empty_message: Option<&'static str>,
    /// The previous track's sheet dissolving out across a crossfaded
    /// transition (the incoming sheet fades in against it).
    pub dissolve: Option<DissolveView<'a>>,
}

/// Borrowed view of the dissolving outgoing sheet.
#[derive(Clone, Copy)]
pub(crate) struct DissolveView<'a> {
    pub lines: &'a [LrcLine],
    /// The column center frozen at the transition.
    pub center: f32,
    /// `0.0..1.0` — outgoing alpha is `1 - progress`, incoming is `progress`.
    pub progress: f32,
}

// ---------------------------------------------------------------------------
// Widget
// ---------------------------------------------------------------------------

/// Per-instance shaping cache, stored in the widget tree.
#[derive(Default)]
struct State {
    paragraphs: Vec<Plain<<iced::Renderer as TextRenderer>::Paragraph>>,
    /// `(content identity, shaped width)` the cache was built for.
    cache_key: (u64, u32),
    /// Uniform slot height for this document (max wrapped rows, clamped).
    slot_height: f32,
    /// Second cache for the dissolving outgoing sheet (empty when idle).
    out_paragraphs: Vec<Plain<<iced::Renderer as TextRenderer>::Paragraph>>,
    out_cache_key: (u64, u32),
    out_slot_height: f32,
}

/// Shape a doc's lines at `text_width`, returning the paragraphs plus the
/// per-document uniform slot height (max wrapped rows, clamped — constant
/// within the doc so the scroll math stays exact: index × slot_height).
fn shape_lines(
    lines: &[LrcLine],
    text_width: f32,
    scale_factor: Option<f32>,
) -> (Vec<Plain<<iced::Renderer as TextRenderer>::Paragraph>>, f32) {
    let mut paragraphs = Vec::with_capacity(lines.len());
    let mut max_rows = 1usize;
    for line in lines {
        let text = Text {
            content: line.text.as_str(),
            bounds: Size::new(text_width, f32::INFINITY),
            size: Pixels(LYRIC_FONT_SIZE),
            line_height: advanced_text::LineHeight::Absolute(Pixels(LINE_HEIGHT)),
            font: theme::ui_font(),
            align_x: advanced_text::Alignment::Center,
            align_y: iced::alignment::Vertical::Top,
            shaping: advanced_text::Shaping::Advanced,
            wrapping: Wrapping::Word,
            ellipsis: advanced_text::Ellipsis::None,
            hint_factor: scale_factor,
        };
        let mut paragraph = Plain::default();
        paragraph.update(text);
        let rows = (paragraph.min_bounds().height / LINE_HEIGHT)
            .round()
            .max(1.0) as usize;
        max_rows = max_rows.max(rows.min(MAX_SLOT_ROWS));
        paragraphs.push(paragraph);
    }
    (paragraphs, (max_rows as f32) * LINE_HEIGHT + 2.0 * SLOT_PAD)
}

/// Fill a paragraph with its glyph halo: the dual offset-ring in `bg0_hard`
/// under the main fill (drawn LAST — the only load-bearing ordering; the halo
/// fills share one color so they composite commutatively).
///
/// Halo weight is `line_alpha^1.5`: the k-fill compound `1-(1-a)^k` is concave
/// in `a`, so LINEAR scaling would fade the rim slower than its own text and
/// leave faded far lines wearing disproportionately heavy rims. The 1.5-power
/// keeps the rim tracking just under its text at every falloff step — far
/// lines keep a faint (correctly subordinate) halo, and the dissolving sheet's
/// halos decay slightly AHEAD of their text (no dark ghost plates). The halo
/// stays `bg0_hard` regardless of the main fill's accent lerp: that is the
/// surface `accent_bright` is theme-tuned against, in every theme, both modes.
fn fill_haloed_paragraph(
    renderer: &mut iced::Renderer,
    paragraph: &<iced::Renderer as TextRenderer>::Paragraph,
    pos: iced::Point,
    color: Color,
    clip: Rectangle,
) {
    let weight = color.a * color.a.sqrt();
    let halo = theme::bg0_hard();
    let a_inner = HALO_INNER_ALPHA * weight;
    if a_inner >= HALO_MIN_FILL_ALPHA {
        for (dx, dy) in HALO_INNER_OFFSETS {
            renderer.fill_paragraph(
                paragraph,
                pos + Vector::new(dx, dy),
                Color { a: a_inner, ..halo },
                clip,
            );
        }
    }
    let a_outer = HALO_OUTER_ALPHA * weight;
    if a_outer >= HALO_MIN_FILL_ALPHA {
        for (dx, dy) in HALO_OUTER_OFFSETS {
            renderer.fill_paragraph(
                paragraph,
                pos + Vector::new(dx, dy),
                Color { a: a_outer, ..halo },
                clip,
            );
        }
    }
    renderer.fill_paragraph(paragraph, pos, color, clip);
}

/// Draw one lyric column (paragraphs at a uniform slot height) centered on
/// `center_pos`, with the continuous alpha falloff scaled by `alpha_factor`
/// (the dissolve cross-blend: outgoing fades out as incoming fades in).
#[allow(clippy::too_many_arguments)]
fn draw_column(
    renderer: &mut iced::Renderer,
    paragraphs: &[Plain<<iced::Renderer as TextRenderer>::Paragraph>],
    bounds: Rectangle,
    slot_h: f32,
    center_pos: f32,
    alpha_factor: f32,
    active_index: Option<usize>,
    accent: Color,
    base: Color,
) {
    let center_y = bounds.y + bounds.height / 2.0;
    for (i, paragraph) in paragraphs.iter().enumerate() {
        let offset_slots = i as f32 - center_pos;
        let slot_top = center_y - slot_h / 2.0 + offset_slots * slot_h;

        // Cull slots fully outside the panel.
        if slot_top + slot_h < bounds.y || slot_top > bounds.y + bounds.height {
            continue;
        }

        // Continuous alpha falloff by distance from the eased center, so
        // brightness glides with the column instead of stepping.
        let distance = offset_slots.abs();
        let falloff = if distance <= 1.0 {
            NEAR_ALPHA + (1.0 - distance) * (ACTIVE_ALPHA - NEAR_ALPHA)
        } else if distance <= 2.0 {
            MID_ALPHA + (2.0 - distance) * (NEAR_ALPHA - MID_ALPHA)
        } else if distance <= 3.0 {
            FAR_ALPHA + (3.0 - distance) * (MID_ALPHA - FAR_ALPHA)
        } else {
            FAR_ALPHA
        };
        let alpha = falloff * alpha_factor.clamp(0.0, 1.0);
        // The active line's accent fades in as the center arrives.
        let color = if active_index == Some(i) {
            let arrive = (1.0 - distance).clamp(0.0, 1.0);
            Color {
                r: base.r + (accent.r - base.r) * arrive,
                g: base.g + (accent.g - base.g) * arrive,
                b: base.b + (accent.b - base.b) * arrive,
                a: alpha,
            }
        } else {
            Color { a: alpha, ..base }
        };

        // Clip each paragraph to its slot so a >MAX_SLOT_ROWS outlier can't
        // bleed into the neighbor's slot.
        let slot_bounds = Rectangle {
            x: bounds.x,
            y: slot_top,
            width: bounds.width,
            height: slot_h,
        };
        let Some(visible) = slot_bounds.intersection(&bounds) else {
            continue;
        };
        let pos = iced::Point::new(bounds.x + H_PAD, slot_top + SLOT_PAD);
        renderer.with_translation(Vector::new(0.0, 0.0), |renderer| {
            fill_haloed_paragraph(renderer, paragraph.raw(), pos, color, visible);
        });
    }
}

/// FNV-1a over the line texts — cheap doc-identity for the shaping cache.
fn doc_hash(lines: &[LrcLine]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for line in lines {
        for b in line.text.bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h ^= 0x2e;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

pub(crate) struct LyricViewport<'a> {
    data: LyricsPanelData<'a>,
}

impl<'a> LyricViewport<'a> {
    pub(crate) fn new(data: LyricsPanelData<'a>) -> Self {
        Self { data }
    }
}

impl<M: 'static> Widget<M, Theme, iced::Renderer> for LyricViewport<'_> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Fill,
        }
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let bounds = limits.max();
        let state = tree.state.downcast_mut::<State>();

        // (Re-)shape the paragraphs when the doc or the panel width changes.
        let text_width = (bounds.width - 2.0 * H_PAD).max(1.0);
        let scale = renderer.scale_factor();
        let key = (doc_hash(self.data.lines), text_width as u32);
        if state.cache_key != key {
            state.cache_key = key;
            let (paragraphs, slot_height) = shape_lines(self.data.lines, text_width, scale);
            state.paragraphs = paragraphs;
            state.slot_height = slot_height;
        }

        // Dissolving outgoing sheet (second cache; cleared when idle so a long
        // doc doesn't linger in memory after its fade).
        if let Some(dissolve) = &self.data.dissolve {
            let out_key = (doc_hash(dissolve.lines), text_width as u32);
            if state.out_cache_key != out_key {
                state.out_cache_key = out_key;
                let (paragraphs, slot_height) = shape_lines(dissolve.lines, text_width, scale);
                state.out_paragraphs = paragraphs;
                state.out_slot_height = slot_height;
            }
        } else if !state.out_paragraphs.is_empty() {
            state.out_paragraphs = Vec::new();
            state.out_cache_key = (0, 0);
        }

        layout::Node::new(bounds)
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: layout::Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();

        // Empty state — but NOT during a cold-path dissolve, where the
        // previous sheet must keep fading below while the resolve runs.
        if self.data.lines.is_empty() && self.data.dissolve.is_none() {
            // Empty state: the faded no-match message (nothing else to draw).
            if let Some(message) = self.data.empty_message {
                let text = Text {
                    content: message,
                    bounds: Size::new((bounds.width - 2.0 * H_PAD).max(1.0), f32::INFINITY),
                    size: Pixels(LYRIC_FONT_SIZE - 2.0),
                    line_height: advanced_text::LineHeight::Absolute(Pixels(LINE_HEIGHT)),
                    font: theme::ui_font(),
                    align_x: advanced_text::Alignment::Center,
                    align_y: iced::alignment::Vertical::Top,
                    shaping: advanced_text::Shaping::Advanced,
                    wrapping: Wrapping::Word,
                    ellipsis: advanced_text::Ellipsis::None,
                    hint_factor: renderer.scale_factor(),
                };
                let paragraph = <iced::Renderer as TextRenderer>::Paragraph::with_text(text);
                let text_h = paragraph.min_bounds().height;
                let pos =
                    iced::Point::new(bounds.x + H_PAD, bounds.y + (bounds.height - text_h) / 2.0);
                let color = Color {
                    a: 0.45,
                    ..theme::fg0()
                };
                renderer.with_layer(bounds, |renderer| {
                    fill_haloed_paragraph(renderer, &paragraph, pos, color, bounds);
                });
            }
            return;
        }

        let slot_h = state.slot_height.max(LINE_HEIGHT);
        // The column's center in slot-index space: the live eased position
        // while a glide is in flight (published by the boat tick), clamped to
        // the doc so a stale value from a previous doc can't fling the column.
        // Pre-roll (no active line) parks on line 0, dimmed.
        let max_idx = (self.data.lines.len().saturating_sub(1)) as f32;
        let center_pos = if self.data.active_index.is_some() {
            lyrics_center_pos().clamp(0.0, max_idx)
        } else {
            0.0
        };

        // Contrast-assured against the glyph halo's `bg0_hard` surface — the
        // raw `accent_bright` is under-floor on some light themes.
        let accent = theme::lyrics_accent();
        let base = theme::fg0();
        // Cross-blend: while the previous sheet dissolves out, the incoming
        // one fades in against it (crossfade-coupled transition).
        let incoming_factor = self.data.dissolve.map_or(1.0, |d| d.progress);

        renderer.with_layer(bounds, |renderer| {
            if let Some(dissolve) = self.data.dissolve {
                draw_column(
                    renderer,
                    &state.out_paragraphs,
                    bounds,
                    state.out_slot_height.max(LINE_HEIGHT),
                    dissolve.center,
                    1.0 - dissolve.progress,
                    None,
                    accent,
                    base,
                );
            }
            draw_column(
                renderer,
                &state.paragraphs,
                bounds,
                slot_h,
                center_pos,
                incoming_factor,
                self.data.active_index,
                accent,
                base,
            );
        });
    }

    fn mouse_interaction(
        &self,
        _tree: &widget::Tree,
        _layout: layout::Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        // Event-transparent: never claim the cursor — the artwork context menu
        // and clicks live on the panel beneath this layer.
        mouse::Interaction::default()
    }
}

impl<'a, M: 'static> From<LyricViewport<'a>> for Element<'a, M> {
    fn from(viewport: LyricViewport<'a>) -> Self {
        Element::new(viewport)
    }
}

// ---------------------------------------------------------------------------
// Layer composition
// ---------------------------------------------------------------------------

/// The lyrics scrim: a vertical gradient in `bg0_hard` that dims the art for
/// legibility (real backdrop blur doesn't exist in iced; the alpha ramp
/// substitutes). Split from the text layer so the panel builder can stack the
/// over-cover visualizer BETWEEN scrim and text — art dimmed, visualizer at
/// full strength, haloed lyrics on top.
pub(crate) fn lyrics_scrim<'a, Message: 'a>(width: f32, height: f32) -> Element<'a, Message> {
    use iced::widget::container;

    container(iced::widget::Space::new())
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .style(|_theme| {
            let bg = theme::bg0_hard();
            let edge = Color {
                a: SCRIM_EDGE_ALPHA,
                ..bg
            };
            let center = Color {
                a: SCRIM_CENTER_ALPHA,
                ..bg
            };
            container::Style {
                background: Some(
                    iced::Gradient::Linear(
                        iced::gradient::Linear::new(iced::Radians(0.0))
                            .add_stop(0.0, edge)
                            .add_stop(0.35, center)
                            .add_stop(0.65, center)
                            .add_stop(1.0, edge),
                    )
                    .into(),
                ),
                ..Default::default()
            }
        })
        .into()
}

/// The lyric text layer (the event-transparent viewport alone — the scrim is
/// [`lyrics_scrim`], stacked separately beneath the visualizer). Sized by the
/// caller to the visible art rect.
pub(crate) fn lyrics_text_layer<'a, Message: 'a + 'static>(
    data: LyricsPanelData<'a>,
    width: f32,
    height: f32,
) -> Element<'a, Message> {
    use iced::widget::container;

    container(Element::<Message>::from(LyricViewport::new(data)))
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ease_out_expo_endpoints_and_monotonic() {
        assert_eq!(ease_out_expo(0.0), 0.0);
        assert_eq!(ease_out_expo(1.0), 1.0);
        assert_eq!(ease_out_expo(1.5), 1.0);
        let mut prev = 0.0;
        for step in 1..=20 {
            let t = step as f32 / 20.0;
            let v = ease_out_expo(t);
            assert!(v >= prev, "ease must be monotonic");
            prev = v;
        }
        // Characteristic ease-out shape: fast start.
        assert!(ease_out_expo(0.25) > 0.7);
    }

    /// Pin the halo's compound-rim math so future knob turns are conscious.
    /// The k=3 stem-edge compound is `1-(1-HALO_INNER_ALPHA·w)³` where
    /// `w = a^1.5` is the halo weight for a line at text alpha `a`.
    #[test]
    fn halo_rim_compound_is_strong_yet_subordinate() {
        let rim = |a: f32| {
            let w = a * a.sqrt();
            1.0 - (1.0 - HALO_INNER_ALPHA * w).powi(3)
        };
        // Worst-case guarantee: the active line's rim must be near-opaque —
        // that is what separates white ink from white type printed on the art.
        assert!(
            rim(ACTIVE_ALPHA) >= 0.9,
            "active rim {:.3} lost the white-on-white guarantee",
            rim(ACTIVE_ALPHA)
        );
        // Subordination: at every falloff step the rim stays BELOW its own
        // text alpha, so faded lines never wear heavier rims than ink (mud).
        for a in [ACTIVE_ALPHA, NEAR_ALPHA, MID_ALPHA, FAR_ALPHA] {
            assert!(
                rim(a) < a,
                "rim {:.3} outweighs its text at alpha {a}",
                rim(a)
            );
        }
    }

    #[test]
    fn doc_hash_distinguishes_lines() {
        let a = [LrcLine {
            time_ms: 0,
            text: "hello".into(),
            words: vec![],
        }];
        let b = [LrcLine {
            time_ms: 0,
            text: "world".into(),
            words: vec![],
        }];
        assert_ne!(doc_hash(&a), doc_hash(&b));
        assert_eq!(doc_hash(&a), doc_hash(&a));
    }
}
