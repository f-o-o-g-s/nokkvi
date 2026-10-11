//! Over-cover lyrics viewport: an `advanced::Widget` that draws the lyric
//! column centered on the live center position, plus the scrim layer that
//! keeps it legible over arbitrary album art.
//!
//! Two kinds of sheet share it. A SYNCED sheet centers on its active line,
//! which glows in `lyrics_accent()` and glides between lines. A PLAIN sheet
//! (untimed lyrics from the server) has no line known to be current, so
//! nothing is accented: a flat `PLAIN_BAND_LINES` band carries the eye, and
//! the column drifts with playback progress (`drift_center`).
//!
//! C1 = the static look (owner sign-off gate): per-doc uniform slot heights,
//! active line in `accent_bright()`, neighbors on an alpha falloff, a vertical
//! scrim gradient. Motion (eased scroll off the frame clock) lands in C2 —
//! `ease_out_expo` is defined here now so the math is test-pinned early.
//!
//! Event-transparency is load-bearing, with ONE exception: `mouse_interaction`
//! always returns the default, and `update` captures nothing but a
//! `WheelScrolled` over a non-empty PLAIN sheet with a callback wired. Every
//! other event — the right-click that opens the artwork context menu, every
//! click, every wheel over a synced sheet or the empty state — passes straight
//! through to the panel beneath.

use iced::{
    Color, Element, Event, Length, Pixels, Rectangle, Size, Theme, Vector, Widget as _,
    advanced::{
        Renderer as _, Shell, layout, renderer,
        text::{
            self as advanced_text, Paragraph as ParagraphTrait, Renderer as TextRenderer, Text,
            paragraph::Plain,
        },
        widget::{self, Widget},
    },
    mouse,
    widget::text::Wrapping,
};
use nokkvi_data::types::{
    lyrics::{LrcLine, WordCursor, WordSpan},
    player_settings::LyricsWordHighlight,
};

use crate::theme;

/// Base lyric font size (logical px).
const LYRIC_FONT_SIZE: f32 = 16.0;
/// Fixed line height (absolute, so the slot math is exact).
const LINE_HEIGHT: f32 = 22.0;
/// Vertical padding inside each line slot.
const SLOT_PAD: f32 = 6.0;
/// Horizontal inset of the text column from the panel edges.
const H_PAD: f32 = 18.0;
/// The panel side (shorter of width and height) at which the sheet starts
/// growing: about a Queue cover on a 1080p window. Below it the text keeps the
/// base size; above it the Queue cover and Theater Mode both scale up, so one
/// panel size shows one text size wherever it is.
const LYRICS_REFERENCE_SIDE: f32 = 540.0;
/// The largest panel-fit scale (a 1440p Queue cover, a 4K theater window).
const LYRICS_MAX_SCALE: f32 = 2.5;
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

/// How long a word takes to light up whole in the `Word` highlight, in
/// milliseconds from its start.
const WORD_POP_MS: f32 = 150.0;
/// The sweep front's soft edge: extra accent fills, each one logical pixel
/// further past the front and fainter, so the front reads as a bloom rather
/// than a hard cut through a glyph. `(offset px, alpha factor)`.
const SWEEP_EDGE_STEPS: [(f32, f32); 3] = [(1.0, 0.6), (2.0, 0.35), (3.0, 0.15)];

/// Exponential ease-out — the glide curve driven by the per-frame tick.
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
/// of the way from line 2 to line 3), published by the per-frame tick and
/// read in `draw()`. A process-global atomic — deliberately, NOT an
/// `Instant::now()` self-animation inside `draw()`: publishing off the tick is
/// what makes the motion assertable as observable state in `test_app` (the
/// `NOW_PLAYING_PHASE` precedent in `slot_list.rs`). Do not "simplify" away.
static LYRICS_CENTER_POS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Publish the eased center (called once per frame by the frame tick).
pub(crate) fn set_lyrics_center(pos: f32) {
    LYRICS_CENTER_POS.store(pos.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// Read the live eased center.
pub(crate) fn lyrics_center_pos() -> f32 {
    f32::from_bits(LYRICS_CENTER_POS.load(std::sync::atomic::Ordering::Relaxed))
}

/// Lines one wheel notch moves a plain sheet. Tuning knob: raise for coarser
/// paging, lower for a finer crawl.
pub(crate) const WHEEL_LINES_PER_NOTCH: f32 = 2.0;

/// Convert one wheel event into a LINE delta, positive = forward through the
/// sheet. Wheel DOWN (negative `y`) moves the sheet forward, matching every
/// other scroll surface. A pixel delta (touchpads, high-resolution wheels)
/// converts through the sheet's own slot height so a given finger travel moves
/// the same visual distance whatever the line height.
fn wheel_lines(delta: mouse::ScrollDelta, slot_height: f32) -> f32 {
    match delta {
        mouse::ScrollDelta::Lines { y, .. } => -y * WHEEL_LINES_PER_NOTCH,
        mouse::ScrollDelta::Pixels { y, .. } => {
            if slot_height > 0.0 {
                -y / slot_height
            } else {
                0.0
            }
        }
    }
}

/// How far either side of the center a PLAIN sheet stays at full brightness,
/// in slots. A plain sheet has no current line to accent, so a flat band
/// carries the eye instead of a point. Tuning knob: widen for a calmer sheet,
/// narrow to focus.
pub(crate) const PLAIN_BAND_LINES: f32 = 2.0;

/// The column center for a PLAIN sheet (pure): playback progress walks it from
/// the first line to the last, `offset` carries the user's wheel scrolling, and
/// the result is clamped to the sheet. No easing — a seek and a wheel notch
/// both jump, because the center is a pure function of the last tick.
///
/// A zero or unknown duration parks the sheet at `offset` rather than dividing
/// by zero; a sheet with one line (or none) has nowhere to drift.
pub(crate) fn drift_center(
    position_ms: u32,
    duration_ms: u32,
    line_count: usize,
    offset: f32,
) -> f32 {
    let max = line_count.saturating_sub(1) as f32;
    if max <= 0.0 {
        return 0.0;
    }
    (raw_drift(position_ms, duration_ms, line_count) + offset).clamp(0.0, max)
}

/// Where playback ALONE would put the center, unclamped. Separate from
/// [`drift_center`] so [`drift_offset_for`] can invert exactly the same term.
fn raw_drift(position_ms: u32, duration_ms: u32, line_count: usize) -> f32 {
    let max = line_count.saturating_sub(1) as f32;
    if max <= 0.0 || duration_ms == 0 {
        return 0.0;
    }
    position_ms as f32 / duration_ms as f32 * max
}

/// The offset that puts the column at `wanted` — the exact inverse of
/// [`drift_center`], so a wheel notch lands where it asked to the pixel.
///
/// It subtracts the UNCLAMPED drift deliberately. The playback tick reports
/// duration in whole seconds while the position is exact, so `progress`
/// exceeds 1.0 through a track's final second on any track whose length isn't
/// a round number; subtracting a clamped term there would leave the sheet
/// short of the notch by `(progress - 1) * (n - 1)` slots.
pub(crate) fn drift_offset_for(
    position_ms: u32,
    duration_ms: u32,
    line_count: usize,
    wanted: f32,
) -> f32 {
    wanted - raw_drift(position_ms, duration_ms, line_count)
}

/// Compute the eased center for a glide (pure — the frame tick feeds it the
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
    /// `false` for a plain (untimed) sheet: nothing is accented and the column
    /// center comes from the drift rather than a line cursor.
    pub synced: bool,
    pub active_index: Option<usize>,
    /// Shown centered when `lines` is empty (no match for this track).
    pub empty_message: Option<&'static str>,
    /// The previous track's sheet dissolving out across a crossfaded
    /// transition (the incoming sheet fades in against it).
    pub dissolve: Option<DissolveView<'a>>,
    /// The lyric clock (`LyricsState::clock_ms`): the playhead the word-level
    /// highlight reads, run on per frame from the last position tick.
    pub clock_ms: u32,
    /// How the active line lights its words when it carries word timing.
    pub word_highlight: LyricsWordHighlight,
}

/// The lyric text scale for a panel of `width` × `height`: 1.0 up to a
/// Queue-cover-sized panel, then growing with the panel's shorter side,
/// capped.
pub(crate) fn panel_fit_scale(width: f32, height: f32) -> f32 {
    (width.min(height) / LYRICS_REFERENCE_SIDE).clamp(1.0, LYRICS_MAX_SCALE)
}

/// Borrowed view of the dissolving outgoing sheet.
#[derive(Clone, Copy)]
pub(crate) struct DissolveView<'a> {
    pub lines: &'a [LrcLine],
    /// The parked sheet's own kind — it keeps its own falloff while it fades,
    /// whatever kind of sheet replaced it.
    pub synced: bool,
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
    /// `(content identity, shaped width, text scale × 100)` the cache was
    /// built for.
    cache_key: (u64, u32, u32),
    /// Uniform slot height for this document (max wrapped rows, clamped).
    slot_height: f32,
    /// Second cache for the dissolving outgoing sheet (empty when idle).
    out_paragraphs: Vec<Plain<<iced::Renderer as TextRenderer>::Paragraph>>,
    out_cache_key: (u64, u32, u32),
    out_slot_height: f32,
    /// The active line's measured words for the word-level highlight; `None`
    /// while the highlight is off or the active line has no word timing.
    active_words: Option<ActiveWordLayout>,
}

/// One word's footprint on one wrapped row of its line, in logical px from the
/// paragraph's origin. A word wrapped mid-way has one box per row it touches.
#[derive(Debug, Clone, Copy, PartialEq)]
struct WordBox {
    row: usize,
    x0: f32,
    x1: f32,
    /// The word runs right to left: its sweep fills from `x1` down.
    rtl: bool,
}

/// A line's words as measured in its shaped paragraph.
#[derive(Debug, Default)]
struct MeasuredWords {
    /// Per word, its boxes in row order. Empty when the line's words could
    /// not be placed in its text: the line is then lit whole, as without
    /// word timing.
    words: Vec<Vec<WordBox>>,
    /// Per wrapped row, `(top, height)` in logical px from the paragraph's
    /// origin.
    rows: Vec<(f32, f32)>,
}

/// The measured words of the ACTIVE line, rebuilt when the active line or the
/// shaping cache changes.
#[derive(Debug)]
struct ActiveWordLayout {
    line: usize,
    /// The shaping cache key the boxes were measured against.
    key: (u64, u32, u32),
    /// [`words_hash`] of the line's word timing the boxes were cut for.
    words_key: u64,
    measured: MeasuredWords,
}

/// FNV-1a over a line's word starts, ends and texts — the word-timing half of
/// the active line's measuring key (`doc_hash` covers the text alone).
fn words_hash(line: &LrcLine) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |bytes: &[u8]| {
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for word in &line.words {
        mix(&word.start_ms.to_le_bytes());
        mix(&word.end_ms.unwrap_or(u32::MAX).to_le_bytes());
        mix(word.text.as_bytes());
        mix(&[0x1f]);
    }
    h
}

/// One accent fill over the active line, in logical px from the paragraph's
/// origin: the sung part of one row, or one step of the sweep front's edge.
#[derive(Debug, Clone, Copy, PartialEq)]
struct AccentFill {
    row: usize,
    x0: f32,
    x1: f32,
    /// Scales the accent's alpha (1.0 for sung words).
    alpha: f32,
}

/// The active line's word-level highlight for one frame.
struct WordHighlightFrame<'a> {
    line: usize,
    rows: &'a [(f32, f32)],
    fills: &'a [AccentFill],
}

/// Byte ranges of `words` inside `text`, found in order: each search starts
/// where the previous word ended, so a repeated word lands on its own
/// occurrence. `None` when a word is missing from the text, which means the
/// timing and the text disagree and the line is better lit whole.
fn word_byte_ranges(text: &str, words: &[WordSpan]) -> Option<Vec<std::ops::Range<usize>>> {
    let mut ranges = Vec::with_capacity(words.len());
    let mut cursor = 0usize;
    for word in words {
        let needle = word.text.trim();
        if needle.is_empty() {
            return None;
        }
        let at = cursor + text.get(cursor..)?.find(needle)?;
        ranges.push(at..at + needle.len());
        cursor = at + needle.len();
    }
    Some(ranges)
}

/// Where each of the line's words sits in its shaped paragraph: one box per
/// wrapped row the word touches, read straight off the cosmic-text layout
/// runs (so the wrapping, the centring and the bidi are the real ones) and
/// scaled back from the buffer's hinted units to logical px. `None` when the
/// words can't be placed in the text or a word shaped to no glyph.
fn measure_words(
    paragraph: &<iced::Renderer as TextRenderer>::Paragraph,
    text: &str,
    words: &[WordSpan],
) -> Option<MeasuredWords> {
    let ranges = word_byte_ranges(text, words)?;
    let buffer = paragraph.buffer();
    // The buffer is shaped in units scaled by the paragraph's hint factor
    // (`None` = unhinted, 1.0); undo it to get logical px.
    let hint = paragraph.hint_factor().unwrap_or(1.0);
    let scale = if hint.is_finite() && hint > 0.0 {
        1.0 / hint
    } else {
        1.0
    };
    let mut boxes: Vec<Vec<WordBox>> = vec![Vec::new(); ranges.len()];
    let mut rows = Vec::new();
    for (row, run) in buffer.layout_runs().enumerate() {
        // A lyric line is one buffer line; glyph byte offsets are relative to
        // it. A second one would restart them, so give up rather than misplace.
        if run.line_i != 0 {
            return None;
        }
        rows.push((run.line_top * scale, run.line_height * scale));
        for glyph in run.glyphs {
            let Some(word) = ranges.iter().position(|range| range.contains(&glyph.start)) else {
                continue;
            };
            let (x0, x1) = (glyph.x * scale, (glyph.x + glyph.w) * scale);
            match boxes.get_mut(word).and_then(|b| b.last_mut()) {
                Some(last) if last.row == row => {
                    last.x0 = last.x0.min(x0);
                    last.x1 = last.x1.max(x1);
                }
                _ => {
                    if let Some(word_boxes) = boxes.get_mut(word) {
                        word_boxes.push(WordBox {
                            row,
                            x0,
                            x1,
                            rtl: glyph.level.is_rtl(),
                        });
                    }
                }
            }
        }
    }
    if boxes.iter().any(Vec::is_empty) {
        return None;
    }
    Some(MeasuredWords { words: boxes, rows })
}

/// The accent fills that light the sung part of the active line (pure).
///
/// `Sweep`: on every row, the union of the sung words and the current word up
/// to the sweep front (an RTL word fills from its right), at full strength,
/// then the front's soft edge ([`SWEEP_EDGE_STEPS`]). `Word`: the sung words
/// at full strength, the current word's boxes whole at the pop ramp
/// (`elapsed_ms` / [`WORD_POP_MS`]). `Off`: nothing — the caller lights the
/// line whole. `elapsed_ms` is how long the current word has been sung;
/// `edge_px` is the width of one edge step (the text scale, so the bloom
/// grows with the panel).
fn accent_fills(
    layout: &MeasuredWords,
    cursor: WordCursor,
    mode: LyricsWordHighlight,
    elapsed_ms: f32,
    edge_px: f32,
) -> Vec<AccentFill> {
    let mut fills = Vec::new();
    if mode == LyricsWordHighlight::Off {
        return fills;
    }
    // The union of fully lit boxes per row.
    let mut by_row: Vec<Option<(f32, f32)>> = vec![None; layout.rows.len()];
    let extend = |by_row: &mut Vec<Option<(f32, f32)>>, row: usize, x0: f32, x1: f32| {
        if let Some(slot) = by_row.get_mut(row) {
            let (lo, hi) = slot.get_or_insert((x0, x1));
            *lo = lo.min(x0);
            *hi = hi.max(x1);
        }
    };
    for word in layout.words.iter().take(cursor.sung) {
        for b in word {
            extend(&mut by_row, b.row, b.x0, b.x1);
        }
    }

    let current = cursor
        .current
        .and_then(|(index, fraction)| layout.words.get(index).map(|boxes| (boxes, fraction)));
    let mut edge: Option<(usize, f32, bool)> = None;
    let mut pops: Vec<AccentFill> = Vec::new();
    match (mode, current) {
        (LyricsWordHighlight::Sweep, Some((boxes, fraction))) => {
            // Spread the word's progress across its boxes by width, so a word
            // wrapped over two rows sweeps the first row out, then the second.
            let total: f32 = boxes.iter().map(|b| (b.x1 - b.x0).max(0.0)).sum();
            let mut filled = fraction.clamp(0.0, 1.0) * total;
            for b in boxes {
                let width = (b.x1 - b.x0).max(0.0);
                let take = filled.min(width);
                filled -= take;
                if take > 0.0 {
                    let (x0, x1) = if b.rtl {
                        (b.x1 - take, b.x1)
                    } else {
                        (b.x0, b.x0 + take)
                    };
                    extend(&mut by_row, b.row, x0, x1);
                }
                if take < width {
                    let front = if b.rtl { b.x1 - take } else { b.x0 + take };
                    edge = Some((b.row, front, b.rtl));
                    break;
                }
            }
        }
        (LyricsWordHighlight::Word, Some((boxes, _))) => {
            let alpha = (elapsed_ms / WORD_POP_MS).clamp(0.0, 1.0);
            pops.extend(boxes.iter().map(|b| AccentFill {
                row: b.row,
                x0: b.x0,
                x1: b.x1,
                alpha,
            }));
        }
        (LyricsWordHighlight::Off | LyricsWordHighlight::Sweep | LyricsWordHighlight::Word, _) => {}
    }

    fills.extend(by_row.iter().enumerate().filter_map(|(row, span)| {
        span.map(|(x0, x1)| AccentFill {
            row,
            x0,
            x1,
            alpha: 1.0,
        })
    }));
    fills.extend(pops);
    if let Some((row, front, rtl)) = edge {
        let step = edge_px.max(0.0);
        fills.extend(SWEEP_EDGE_STEPS.iter().map(|&(offset, alpha)| {
            let (near, far) = ((offset - 1.0) * step, offset * step);
            let (x0, x1) = if rtl {
                (front - far, front - near)
            } else {
                (front + near, front + far)
            };
            AccentFill { row, x0, x1, alpha }
        }));
    }
    fills
}

/// Shape a doc's lines at `text_width`, returning the paragraphs plus the
/// per-document uniform slot height (max wrapped rows, clamped — constant
/// within the doc so the scroll math stays exact: index × slot_height).
fn shape_lines(
    lines: &[LrcLine],
    text_width: f32,
    scale_factor: Option<f32>,
    text_scale: f32,
) -> (Vec<Plain<<iced::Renderer as TextRenderer>::Paragraph>>, f32) {
    let line_height = LINE_HEIGHT * text_scale;
    let mut paragraphs = Vec::with_capacity(lines.len());
    let mut max_rows = 1usize;
    for line in lines {
        let text = Text {
            content: line.text.as_str(),
            bounds: Size::new(text_width, f32::INFINITY),
            size: Pixels(LYRIC_FONT_SIZE * text_scale),
            line_height: advanced_text::LineHeight::Absolute(Pixels(line_height)),
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
        let rows = (paragraph.min_bounds().height / line_height)
            .round()
            .max(1.0) as usize;
        max_rows = max_rows.max(rows.min(MAX_SLOT_ROWS));
        paragraphs.push(paragraph);
    }
    (
        paragraphs,
        (max_rows as f32) * line_height + 2.0 * SLOT_PAD * text_scale,
    )
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
    text_scale: f32,
) {
    let weight = color.a * color.a.sqrt();
    let halo = theme::bg0_hard();
    let a_inner = HALO_INNER_ALPHA * weight;
    if a_inner >= HALO_MIN_FILL_ALPHA {
        for (dx, dy) in HALO_INNER_OFFSETS {
            renderer.fill_paragraph(
                paragraph,
                pos + Vector::new(dx, dy) * text_scale,
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
                pos + Vector::new(dx, dy) * text_scale,
                Color { a: a_outer, ..halo },
                clip,
            );
        }
    }
    renderer.fill_paragraph(paragraph, pos, color, clip);
}

/// Continuous alpha falloff by distance (in slots) from the column center, so
/// brightness glides with the column instead of stepping.
fn falloff_at(distance: f32) -> f32 {
    if distance <= 1.0 {
        NEAR_ALPHA + (1.0 - distance) * (ACTIVE_ALPHA - NEAR_ALPHA)
    } else if distance <= 2.0 {
        MID_ALPHA + (2.0 - distance) * (NEAR_ALPHA - MID_ALPHA)
    } else if distance <= 3.0 {
        FAR_ALPHA + (3.0 - distance) * (MID_ALPHA - FAR_ALPHA)
    } else {
        FAR_ALPHA
    }
}

/// Draw one lyric column (paragraphs at a uniform slot height) centered on
/// `center_pos`, with the continuous alpha falloff scaled by `alpha_factor`
/// (the dissolve cross-blend: outgoing fades out as incoming fades in).
///
/// `synced` picks the falloff shape: a synced sheet peaks on its active line,
/// a plain one holds a flat full-brightness band `PLAIN_BAND_LINES` wide either
/// side of the center before the same curve takes over — no line of an untimed
/// sheet is current, so none may look it.
#[expect(
    clippy::too_many_arguments,
    reason = "the per-frame draw helper; its one caller passes that frame's layout and colors"
)]
fn draw_column(
    renderer: &mut iced::Renderer,
    paragraphs: &[Plain<<iced::Renderer as TextRenderer>::Paragraph>],
    bounds: Rectangle,
    slot_h: f32,
    center_pos: f32,
    alpha_factor: f32,
    active_index: Option<usize>,
    synced: bool,
    accent: Color,
    base: Color,
    text_scale: f32,
    words: Option<&WordHighlightFrame<'_>>,
) {
    let center_y = bounds.y + bounds.height / 2.0;
    for (i, paragraph) in paragraphs.iter().enumerate() {
        let offset_slots = i as f32 - center_pos;
        let slot_top = center_y - slot_h / 2.0 + offset_slots * slot_h;

        // Cull slots fully outside the panel.
        if slot_top + slot_h < bounds.y || slot_top > bounds.y + bounds.height {
            continue;
        }

        let distance = offset_slots.abs();
        let falloff = if synced {
            falloff_at(distance)
        } else {
            falloff_at((distance - PLAIN_BAND_LINES).max(0.0))
        };
        let alpha = falloff * alpha_factor.clamp(0.0, 1.0);
        // The active line's accent fades in as the center arrives.
        let arrive = (1.0 - distance).clamp(0.0, 1.0);
        // With word timing the active line keeps its base ink and the accent
        // is painted over its sung words below, word by word.
        let word_frame = words.filter(|frame| active_index == Some(i) && frame.line == i);
        let color = if active_index == Some(i) && word_frame.is_none() {
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
        let pos = iced::Point::new(
            bounds.x + H_PAD * text_scale,
            slot_top + SLOT_PAD * text_scale,
        );
        renderer.with_translation(Vector::new(0.0, 0.0), |renderer| {
            fill_haloed_paragraph(renderer, paragraph.raw(), pos, color, visible, text_scale);
            // The sung words: the same paragraph again in accent, clipped to
            // each fill's rectangle (the text pipeline trims glyphs to the
            // clip per pixel, so a sweep front cuts cleanly through a glyph).
            let Some(frame) = word_frame else {
                return;
            };
            for fill in frame.fills {
                let Some(&(top, height)) = frame.rows.get(fill.row) else {
                    continue;
                };
                let rect = Rectangle {
                    x: pos.x + fill.x0,
                    y: pos.y + top,
                    width: (fill.x1 - fill.x0).max(0.0),
                    height,
                };
                let Some(clip) = rect.intersection(&visible) else {
                    continue;
                };
                let a = alpha * arrive * fill.alpha;
                if a >= HALO_MIN_FILL_ALPHA {
                    renderer.fill_paragraph(paragraph.raw(), pos, Color { a, ..accent }, clip);
                }
            }
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

pub(crate) struct LyricViewport<'a, M> {
    data: LyricsPanelData<'a>,
    /// Wheel callback for a plain sheet. A plain `fn` pointer, not a boxed
    /// closure and not a field of `LyricsPanelData` — that struct is built on
    /// `Nokkvi` with no message type in scope and must stay `Copy` (the
    /// artwork panel rebuilds it inside `responsive` `Fn` closures).
    on_wheel: Option<fn(f32) -> M>,
}

impl<'a, M> LyricViewport<'a, M> {
    pub(crate) fn new(data: LyricsPanelData<'a>, on_wheel: Option<fn(f32) -> M>) -> Self {
        Self { data, on_wheel }
    }

    /// The active line the word-level highlight applies to this frame: the
    /// setting is on, a line is active, and it carries word timing.
    fn worded_active_line(&self) -> Option<usize> {
        if self.data.word_highlight == LyricsWordHighlight::Off {
            return None;
        }
        self.data.active_index.filter(|&i| {
            self.data
                .lines
                .get(i)
                .is_some_and(|line| !line.words.is_empty())
        })
    }

    /// The accent fills over the active line for this frame, read from the
    /// lyric clock against the line's measured words. `None` when the
    /// highlight doesn't apply or the words could not be placed in the text.
    fn word_fills(&self, state: &State, text_scale: f32) -> Option<(usize, Vec<AccentFill>)> {
        let index = self.worded_active_line()?;
        let layout = state
            .active_words
            .as_ref()
            .filter(|layout| layout.line == index && !layout.measured.words.is_empty())?;
        let line = self.data.lines.get(index)?;
        // The next line's stamp caps the last word; a plain sheet has none.
        let line_end =
            nokkvi_data::types::lyrics::line_end_ms(self.data.lines, self.data.synced, index);
        let cursor = line.word_cursor(line_end, self.data.clock_ms);
        let elapsed_ms = cursor
            .current
            .and_then(|(word, _)| line.word_window(word, line_end))
            .map_or(0.0, |(start, _)| {
                self.data.clock_ms.saturating_sub(start) as f32
            });
        Some((
            index,
            accent_fills(
                &layout.measured,
                cursor,
                self.data.word_highlight,
                elapsed_ms,
                text_scale,
            ),
        ))
    }
}

impl<M> iced::advanced::widget::Meta for LyricViewport<'_, M> {}

impl<M: 'static> Widget<M, Theme, iced::Renderer> for LyricViewport<'_, M> {
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
    ) {
        let bounds = limits.bounds();
        let state = tree.state.downcast_mut::<State>();

        // (Re-)shape the paragraphs when the doc or the panel width changes.
        let text_scale = panel_fit_scale(bounds.width, bounds.height);
        let text_width = (bounds.width - 2.0 * H_PAD * text_scale).max(1.0);
        let scale = renderer.hint_factor();
        let scale_key = (text_scale * 100.0).round() as u32;
        let key = (doc_hash(self.data.lines), text_width as u32, scale_key);
        if state.cache_key != key {
            state.cache_key = key;
            let (paragraphs, slot_height) =
                shape_lines(self.data.lines, text_width, scale, text_scale);
            state.paragraphs = paragraphs;
            state.slot_height = slot_height;
        }

        // Measure the active line's words for the word-level highlight, again
        // whenever the active line or the shaping changes. A line whose words
        // can't be placed in its text measures empty, and the draw lights it
        // whole.
        match self.worded_active_line() {
            Some(i) => {
                // The shaping key hashes text only; two sheets with the same
                // text but different word splits need their own boxes.
                let words_key = self.data.lines.get(i).map_or(0, words_hash);
                if state.active_words.as_ref().is_none_or(|layout| {
                    (layout.line, layout.key, layout.words_key) != (i, key, words_key)
                }) {
                    let measured = state
                        .paragraphs
                        .get(i)
                        .zip(self.data.lines.get(i))
                        .and_then(|(paragraph, line)| {
                            measure_words(paragraph.raw(), &line.text, &line.words)
                        })
                        .unwrap_or_default();
                    state.active_words = Some(ActiveWordLayout {
                        line: i,
                        key,
                        words_key,
                        measured,
                    });
                }
            }
            None => state.active_words = None,
        }

        // Dissolving outgoing sheet (second cache; cleared when idle so a long
        // doc doesn't linger in memory after its fade).
        if let Some(dissolve) = &self.data.dissolve {
            let out_key = (doc_hash(dissolve.lines), text_width as u32, scale_key);
            if state.out_cache_key != out_key {
                state.out_cache_key = out_key;
                let (paragraphs, slot_height) =
                    shape_lines(dissolve.lines, text_width, scale, text_scale);
                state.out_paragraphs = paragraphs;
                state.out_slot_height = slot_height;
            }
        } else if !state.out_paragraphs.is_empty() {
            state.out_paragraphs = Vec::new();
            state.out_cache_key = (0, 0, 0);
        }

        tree.size = bounds;
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: layout::Layout,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let text_scale = panel_fit_scale(bounds.width, bounds.height);
        let h_pad = H_PAD * text_scale;
        let line_height = LINE_HEIGHT * text_scale;

        // Empty state — but NOT during a cold-path dissolve, where the
        // previous sheet must keep fading below while the resolve runs.
        if self.data.lines.is_empty() && self.data.dissolve.is_none() {
            // Empty state: the faded no-match message (nothing else to draw).
            if let Some(message) = self.data.empty_message {
                let text = Text {
                    content: message,
                    bounds: Size::new((bounds.width - 2.0 * h_pad).max(1.0), f32::INFINITY),
                    size: Pixels((LYRIC_FONT_SIZE - 2.0) * text_scale),
                    line_height: advanced_text::LineHeight::Absolute(Pixels(line_height)),
                    font: theme::ui_font(),
                    align_x: advanced_text::Alignment::Center,
                    align_y: iced::alignment::Vertical::Top,
                    shaping: advanced_text::Shaping::Advanced,
                    wrapping: Wrapping::Word,
                    ellipsis: advanced_text::Ellipsis::None,
                    hint_factor: renderer.hint_factor(),
                };
                let paragraph = <iced::Renderer as TextRenderer>::Paragraph::with_text(text);
                let text_h = paragraph.min_bounds().height;
                let pos =
                    iced::Point::new(bounds.x + h_pad, bounds.y + (bounds.height - text_h) / 2.0);
                let color = Color {
                    a: 0.45,
                    ..theme::fg0()
                };
                renderer.with_layer(bounds, |renderer| {
                    fill_haloed_paragraph(renderer, &paragraph, pos, color, bounds, text_scale);
                });
            }
            return;
        }

        let slot_h = state.slot_height.max(line_height);
        // The column's center in slot-index space, published by the frame tick
        // and clamped to THIS doc so a stale value from a previous one (a long
        // plain sheet before a short synced one) can't fling the column.
        //
        // A synced sheet parks on line 0 through pre-roll, dimmed — its center
        // is only meaningful once a line is active. A plain sheet has no active
        // line ever, so it always reads the published drift.
        let max_idx = (self.data.lines.len().saturating_sub(1)) as f32;
        let center_pos = if !self.data.synced || self.data.active_index.is_some() {
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

        // The word-level highlight over the active line, from the lyric clock.
        let word_fills = self.word_fills(state, text_scale);
        let word_frame = word_fills.as_ref().and_then(|(line, fills)| {
            state
                .active_words
                .as_ref()
                .map(|layout| WordHighlightFrame {
                    line: *line,
                    rows: &layout.measured.rows,
                    fills,
                })
        });

        renderer.with_layer(bounds, |renderer| {
            if let Some(dissolve) = self.data.dissolve {
                // Clamp the frozen center to the OUTGOING doc, exactly as the
                // live column is clamped above. It was snapshotted from the
                // process-global atomic at the transition, and a synced sheet
                // parked during the next track's pre-roll can carry the
                // PREVIOUS track's line index — far past a short sheet's end,
                // where every line culls and the dissolve shows nothing.
                let out_max = (dissolve.lines.len().saturating_sub(1)) as f32;
                draw_column(
                    renderer,
                    &state.out_paragraphs,
                    bounds,
                    state.out_slot_height.max(line_height),
                    dissolve.center.clamp(0.0, out_max),
                    1.0 - dissolve.progress,
                    None,
                    dissolve.synced,
                    accent,
                    base,
                    text_scale,
                    None,
                );
            }
            draw_column(
                renderer,
                &state.paragraphs,
                bounds,
                slot_h,
                center_pos,
                incoming_factor,
                // `None` for a plain sheet, which never accents a line
                // (`LrcDocument::active_line_at` names no line of it).
                self.data.active_index,
                self.data.synced,
                accent,
                base,
                text_scale,
                word_frame.as_ref(),
            );
        });
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: layout::Layout,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        shell: &mut Shell<'_, M>,
        _viewport: &Rectangle,
    ) {
        // The ONE event this layer claims. Everything else falls through
        // untouched so the artwork right-click menu and clicks keep reaching
        // the panel beneath — see the module header.
        let Event::Mouse(mouse::Event::WheelScrolled { delta }) = event else {
            return;
        };
        let (Some(on_wheel), false, false) =
            (self.on_wheel, self.data.synced, self.data.lines.is_empty())
        else {
            return;
        };
        if !cursor.is_over(layout.bounds()) {
            return;
        }

        // Publish the DELTA, never an absolute: two notches arriving between
        // renders would both read the same constructor-captured base and one
        // would be silently lost (the rule `volume_slider` spells out).
        let slot_h = tree
            .state
            .downcast_ref::<State>()
            .slot_height
            .max(LINE_HEIGHT);
        let lines = wheel_lines(*delta, slot_h);
        // A purely horizontal scroll (shift-wheel, a sideways swipe) converts
        // to zero lines and moves nothing, so it is left for whatever else
        // might want it rather than swallowed.
        if lines != 0.0 {
            shell.publish(on_wheel(lines));
            shell.capture_event();
            shell.request_redraw();
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &widget::Tree,
        _layout: layout::Layout,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        // Never claim the cursor, even over a scrollable plain sheet — the
        // artwork context menu and clicks live on the panel beneath, and a
        // changed cursor would advertise an interaction this layer doesn't own.
        mouse::Interaction::default()
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
        .boxed()
}

/// The lyric text layer (the event-transparent viewport alone — the scrim is
/// [`lyrics_scrim`], stacked separately beneath the visualizer). Sized by the
/// caller to the visible art rect.
pub(crate) fn lyrics_text_layer<'a, Message: 'a + 'static>(
    data: LyricsPanelData<'a>,
    on_wheel: Option<fn(f32) -> Message>,
    width: f32,
    height: f32,
) -> Element<'a, Message> {
    use iced::widget::container;

    container(Element::<Message>::from(
        LyricViewport::new(data, on_wheel).boxed(),
    ))
    .width(Length::Fixed(width))
    .height(Length::Fixed(height))
    .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_fit_scale_grows_with_the_shorter_side_and_caps() {
        assert_eq!(
            panel_fit_scale(400.0, 400.0),
            1.0,
            "a small cover keeps the base size"
        );
        assert_eq!(
            panel_fit_scale(3000.0, 300.0),
            1.0,
            "the shorter side rules"
        );
        let fhd = panel_fit_scale(1920.0, 1080.0);
        assert!(
            fhd > 1.5 && fhd < 2.5,
            "a 1080p window roughly doubles: {fhd}"
        );
        assert!(panel_fit_scale(1920.0, 1200.0) > fhd, "monotone");
        assert_eq!(panel_fit_scale(8000.0, 8000.0), LYRICS_MAX_SCALE, "capped");
    }

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
    fn drift_center_walks_the_sheet_with_playback() {
        // 11 lines → indices 0..=10, a 200 s track.
        let at = |pos_ms| drift_center(pos_ms, 200_000, 11, 0.0);
        assert_eq!(at(0), 0.0, "starts on the first line");
        assert_eq!(at(100_000), 5.0, "halfway through sits on the middle line");
        assert_eq!(at(200_000), 10.0, "ends on the last line");
    }

    #[test]
    fn drift_center_clamps_at_both_ends() {
        // A huge offset either way can never push the column off the sheet —
        // the clamp is applied to the RESULT, so no hidden overshoot survives.
        assert_eq!(drift_center(0, 200_000, 11, -500.0), 0.0);
        assert_eq!(drift_center(200_000, 200_000, 11, 500.0), 10.0);
        assert_eq!(drift_center(100_000, 200_000, 11, 500.0), 10.0);
        assert_eq!(drift_center(100_000, 200_000, 11, -500.0), 0.0);
        // A position past the end (a rounding overshoot at the last tick)
        // clamps too rather than running off.
        assert_eq!(drift_center(400_000, 200_000, 11, 0.0), 10.0);
    }

    #[test]
    fn drift_center_offset_moves_the_column() {
        // The wheel adds to the drift; the drift carries on from there.
        assert_eq!(drift_center(0, 200_000, 11, 2.0), 2.0);
        assert_eq!(drift_center(100_000, 200_000, 11, 2.0), 7.0);
        assert_eq!(drift_center(100_000, 200_000, 11, -2.0), 3.0);
    }

    #[test]
    fn drift_center_survives_a_zero_duration_and_tiny_sheets() {
        // Duration 0 = unknown (the tick reports whole seconds, and a stream
        // or a just-started track can report none): park at the offset rather
        // than divide by zero.
        assert_eq!(drift_center(50_000, 0, 11, 0.0), 0.0);
        assert_eq!(drift_center(50_000, 0, 11, 3.0), 3.0);
        assert_eq!(drift_center(50_000, 0, 11, 99.0), 10.0, "still clamped");
        // One line, and none at all: nowhere to drift, no panic.
        assert_eq!(drift_center(50_000, 200_000, 1, 4.0), 0.0);
        assert_eq!(drift_center(50_000, 200_000, 0, 4.0), 0.0);
    }

    #[test]
    fn drift_offset_for_inverts_drift_center_exactly() {
        // Round-trip: the offset this returns must put the column exactly
        // where it was asked, at any position in the track.
        for position_ms in [0, 1, 55_555, 199_999, 200_000] {
            for wanted in [0.0, 3.5, 10.0] {
                let offset = drift_offset_for(position_ms, 200_000, 11, wanted);
                let landed = drift_center(position_ms, 200_000, 11, offset);
                assert!(
                    (landed - wanted).abs() < 1e-4,
                    "asked {wanted} at {position_ms} ms, landed {landed}"
                );
            }
        }

        // The tick truncates duration to whole seconds while the position is
        // exact, so `progress` exceeds 1.0 in a track's final second. The
        // inverse must still be exact there — a clamped one would fall short.
        let past_end = drift_offset_for(200_900, 200_000, 101, 98.0);
        let landed = drift_center(200_900, 200_000, 101, past_end);
        assert!(
            (landed - 98.0).abs() < 1e-3,
            "overshooting position must still land the notch, got {landed}"
        );

        // A zero duration and a one-line sheet have no drift term to invert.
        assert_eq!(drift_offset_for(50_000, 0, 11, 4.0), 4.0);
        assert_eq!(drift_offset_for(50_000, 200_000, 1, 4.0), 4.0);
    }

    #[test]
    fn wheel_lines_converts_notches_and_pixels() {
        use iced::mouse::ScrollDelta;
        // Wheel DOWN (negative y) moves the sheet FORWARD.
        assert_eq!(
            wheel_lines(ScrollDelta::Lines { x: 0.0, y: -1.0 }, 34.0),
            WHEEL_LINES_PER_NOTCH
        );
        assert_eq!(
            wheel_lines(ScrollDelta::Lines { x: 0.0, y: 1.0 }, 34.0),
            -WHEEL_LINES_PER_NOTCH
        );
        // A pixel delta converts through the sheet's own slot height.
        assert_eq!(
            wheel_lines(ScrollDelta::Pixels { x: 0.0, y: -34.0 }, 34.0),
            1.0
        );
        assert_eq!(
            wheel_lines(ScrollDelta::Pixels { x: 0.0, y: -17.0 }, 34.0),
            0.5
        );
        // A zero slot height (never shaped yet) must not divide by zero.
        assert_eq!(
            wheel_lines(ScrollDelta::Pixels { x: 0.0, y: -34.0 }, 0.0),
            0.0
        );
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

    // --- word-level highlight geometry ---

    fn spans(words: &[&str]) -> Vec<WordSpan> {
        words
            .iter()
            .enumerate()
            .map(|(i, text)| WordSpan {
                start_ms: i as u32 * 500,
                end_ms: None,
                text: (*text).to_string(),
            })
            .collect()
    }

    #[test]
    fn word_byte_ranges_follow_the_text_in_order() {
        let ranges = word_byte_ranges("Hello world", &spans(&["Hello", "world"])).expect("placed");
        assert_eq!(ranges, vec![0..5, 6..11]);
        // A repeated word lands on its own occurrence, not the first again.
        let ranges = word_byte_ranges("la la land", &spans(&["la", "la", "land"])).expect("placed");
        assert_eq!(ranges, vec![0..2, 3..5, 6..10]);
        // A server cue keeps its trailing space; the match trims it.
        let ranges = word_byte_ranges("Hello world", &spans(&["Hello ", "world"])).expect("placed");
        assert_eq!(ranges, vec![0..5, 6..11]);
        // Timing that names a word the text lacks is a disagreement: no layout.
        assert_eq!(
            word_byte_ranges("Hello world", &spans(&["Hello", "there"])),
            None
        );
        assert_eq!(
            word_byte_ranges("Hello world", &spans(&["Hello", ""])),
            None
        );
    }

    /// Two words on one row: `w0` at 10..40, `w1` at 50..90.
    fn two_word_layout() -> MeasuredWords {
        MeasuredWords {
            words: vec![
                vec![WordBox {
                    row: 0,
                    x0: 10.0,
                    x1: 40.0,
                    rtl: false,
                }],
                vec![WordBox {
                    row: 0,
                    x0: 50.0,
                    x1: 90.0,
                    rtl: false,
                }],
            ],
            rows: vec![(0.0, 22.0)],
        }
    }

    fn fill(row: usize, x0: f32, x1: f32, alpha: f32) -> AccentFill {
        AccentFill { row, x0, x1, alpha }
    }

    #[test]
    fn sweep_floods_the_row_up_to_the_front_and_blooms_past_it() {
        let layout = two_word_layout();
        let at = |sung, current| {
            accent_fills(
                &layout,
                WordCursor { sung, current },
                LyricsWordHighlight::Sweep,
                0.0,
                1.0,
            )
        };
        // Nothing sung yet: nothing lit (the line sits in base ink).
        assert!(at(0, None).is_empty());
        // Half through the first word: 10..25 lit, then the three edge steps.
        assert_eq!(
            at(0, Some((0, 0.5))),
            vec![
                fill(0, 10.0, 25.0, 1.0),
                fill(0, 25.0, 26.0, 0.6),
                fill(0, 26.0, 27.0, 0.35),
                fill(0, 27.0, 28.0, 0.15),
            ]
        );
        // A quarter into the second: one union from 10 to 60 (the gap between
        // the words is the space, nothing to paint).
        assert_eq!(at(1, Some((1, 0.25)))[0], fill(0, 10.0, 60.0, 1.0));
        // The word just starting: no fill, but the bloom sits at its start.
        assert_eq!(at(1, Some((1, 0.0)))[0], fill(0, 10.0, 40.0, 1.0));
        assert_eq!(at(1, Some((1, 0.0)))[1], fill(0, 50.0, 51.0, 0.6));
        // Everything sung: the whole row, no front.
        assert_eq!(at(2, None), vec![fill(0, 10.0, 90.0, 1.0)]);
    }

    #[test]
    fn word_mode_pops_the_current_word_whole() {
        let layout = two_word_layout();
        let fills = accent_fills(
            &layout,
            WordCursor {
                sung: 1,
                current: Some((1, 0.9)),
            },
            LyricsWordHighlight::Word,
            75.0,
            1.0,
        );
        // The sung word full, the current one whole at half the pop ramp —
        // its fraction through the word plays no part.
        assert_eq!(
            fills,
            vec![fill(0, 10.0, 40.0, 1.0), fill(0, 50.0, 90.0, 0.5)]
        );
        // Past the ramp it is full.
        let fills = accent_fills(
            &layout,
            WordCursor {
                sung: 1,
                current: Some((1, 0.9)),
            },
            LyricsWordHighlight::Word,
            WORD_POP_MS * 3.0,
            1.0,
        );
        assert_eq!(fills[1].alpha, 1.0);
        // Off paints nothing whatever the cursor says.
        assert!(
            accent_fills(
                &layout,
                WordCursor {
                    sung: 2,
                    current: None,
                },
                LyricsWordHighlight::Off,
                0.0,
                1.0,
            )
            .is_empty()
        );
    }

    #[test]
    fn sweep_runs_an_rtl_word_from_its_right_and_a_wrapped_word_row_by_row() {
        let mut layout = two_word_layout();
        layout.words[0][0].rtl = true;
        let fills = accent_fills(
            &layout,
            WordCursor {
                sung: 0,
                current: Some((0, 0.5)),
            },
            LyricsWordHighlight::Sweep,
            0.0,
            1.0,
        );
        // Right half lit (25..40), the bloom steps leftwards from the front.
        assert_eq!(fills[0], fill(0, 25.0, 40.0, 1.0));
        assert_eq!(fills[1], fill(0, 24.0, 25.0, 0.6));
        assert_eq!(fills[3], fill(0, 22.0, 23.0, 0.15));

        // A word wrapped over two rows (40 px on row 0, 20 px on row 1) at
        // three quarters: row 0 full, row 1 a quarter of its 20.
        let layout = MeasuredWords {
            words: vec![vec![
                WordBox {
                    row: 0,
                    x0: 60.0,
                    x1: 100.0,
                    rtl: false,
                },
                WordBox {
                    row: 1,
                    x0: 10.0,
                    x1: 30.0,
                    rtl: false,
                },
            ]],
            rows: vec![(0.0, 22.0), (22.0, 22.0)],
        };
        let fills = accent_fills(
            &layout,
            WordCursor {
                sung: 0,
                current: Some((0, 0.75)),
            },
            LyricsWordHighlight::Sweep,
            0.0,
            1.0,
        );
        assert_eq!(fills[0], fill(0, 60.0, 100.0, 1.0));
        assert_eq!(fills[1], fill(1, 10.0, 15.0, 1.0));
        assert_eq!(fills[2], fill(1, 15.0, 16.0, 0.6));

        // A sheet scaled to twice the size gets edge steps twice as wide.
        let fills = accent_fills(
            &layout,
            WordCursor {
                sung: 0,
                current: Some((0, 0.75)),
            },
            LyricsWordHighlight::Sweep,
            0.0,
            2.0,
        );
        assert_eq!(fills[2], fill(1, 15.0, 17.0, 0.6));
        assert_eq!(fills[4], fill(1, 19.0, 21.0, 0.15));
    }
}
