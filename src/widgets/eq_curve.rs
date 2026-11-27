//! Equalizer response curve: the band list's combined magnitude response on
//! a log-frequency / dB plot, with one draggable, numbered point per band.
//!
//! An iced `canvas::Program` built by [`eq_curve`]. The math lives in pure
//! functions (axes, range, hit-testing, drag values, Q steps, labels), so
//! the tests pin it without a renderer.
//!
//! - **Axes**: 20 Hz..20 kHz on a log scale; gain symmetric around 0 dB, at
//!   least ±12 and growing in 6 dB steps up to ±30 to fit the curve and every
//!   point ([`range_for_peak`] / [`content_peak_db`]). A pass filter's roll-off
//!   does not widen it: a low cut leaves the plot through its bottom edge
//!   instead of squashing every other band into a ±30 scale. A drag holds the
//!   range it started with, so the scale never shrinks under the cursor.
//! - **Curve**: `Response::new(bands, 48 kHz).db(f)` once per pixel, stroked
//!   in the accent and clipped (geometrically, so every renderer cuts it the
//!   same way) where it leaves the plot; a translucent accent fill runs
//!   between it and the 0 dB line.
//! - **Points**: every band, switched-off ones muted, numbered 1.. so they map
//!   to the rows of a band table. A pass filter sits on the 0 dB line.
//! - **Input**: press a point to select it and start a drag (`on_select`,
//!   then `on_move` once the cursor passes [`DRAG_SLOP`], and `on_release`
//!   when a drag that moved the band ends); the wheel over a point, or during
//!   a drag, steps its Q (`on_q`); a double-click on empty space adds a band
//!   there (`on_add`). Every event it acts on is captured. With no callbacks
//!   it is a read-only display and captures nothing.
//! - **Caching**: three `canvas::Cache` layers (panel + grid, curve, points),
//!   each cleared when a hash of what it draws changes (bands, range,
//!   selection, hover, dimming, theme generation, font, corner radius). The
//!   content range is memoized per band list, since `draw` runs on every
//!   window frame while anything else on screen animates.

use std::{
    cell::Cell,
    hash::{DefaultHasher, Hash, Hasher},
    ops::Deref,
    sync::Arc,
};

use iced::{
    Color, Element, Length, Pixels, Point, Rectangle, Size, Vector, Widget as _,
    advanced::mouse,
    alignment::{Horizontal, Vertical},
    font::Weight,
    widget::canvas::{self, Action, Event, Frame, Geometry, LineCap, LineJoin, Path, Stroke},
};
use nokkvi_data::audio::eq::{
    EQ_BAND_GAIN_DB_MAX, EQ_DEFAULT_Q, EQ_MAX_BANDS, EQ_Q_MAX, EQ_Q_MIN, EqBand, EqBandKind,
    Response,
};

use crate::theme;

// =============================================================================
// Constants
// =============================================================================

/// Sample rate the curve is drawn at. The engine designs each stream at its
/// own rate; only the top octave differs.
const DISPLAY_RATE: f64 = 48_000.0;
/// The visible frequency span (the engine's band limits reach a little past
/// it on both sides; such a band sits on the edge).
const VIEW_FREQ_MIN: f64 = 20.0;
const VIEW_FREQ_MAX: f64 = 20_000.0;
/// Gain range: at least ±12 dB, in 6 dB steps, at most the band limit.
const RANGE_MIN_DB: f64 = 12.0;
const RANGE_STEP_DB: f64 = 6.0;
const RANGE_MAX_DB: f64 = EQ_BAND_GAIN_DB_MAX;
/// One horizontal grid line per this many dB.
const GRID_STEP_DB: f64 = 6.0;
/// Vertical grid lines, each labelled.
const FREQ_TICKS: [f64; 10] = [
    20.0, 50.0, 100.0, 200.0, 500.0, 1_000.0, 2_000.0, 5_000.0, 10_000.0, 20_000.0,
];
/// Density of the frequency scan that sizes the range.
const SCAN_STEPS_PER_OCTAVE: f64 = 24.0;

/// A drag (or a double-click) this close to 0 dB lands on 0.
const ZERO_SNAP_DB: f64 = 0.25;
/// Q multiplier per wheel notch: 2^(1/6), so six notches double or halve it.
const Q_WHEEL_FACTOR: f64 = 1.122_462_048_309_373;
/// Touchpad pixels per wheel notch (the EQ slider's 1 dB per line /
/// 0.1 dB per pixel ratio).
const PIXELS_PER_NOTCH: f32 = 10.0;
/// The cursor travels this far from the press before a drag moves the band,
/// so a click that only selects never nudges an imported value.
const DRAG_SLOP: f32 = 3.0;

/// Height when the caller sets none.
const DEFAULT_HEIGHT: f32 = 220.0;
/// Plot insets inside the panel: dB labels on the left, frequency labels
/// below, room for the selected point's ring on the top and right.
const PAD_LEFT: f32 = 32.0;
const PAD_RIGHT: f32 = 16.0;
const PAD_TOP: f32 = 16.0;
const PAD_BOTTOM: f32 = 22.0;
/// Axis label text size, and its gap from the plot edge.
const LABEL_SIZE: f32 = 10.0;
const LABEL_GAP: f32 = 6.0;
/// Closest two dB labels may sit, in pixels; denser lines go unlabelled.
const MIN_LABEL_SPACING: f32 = 14.0;
/// Curve stroke width.
const CURVE_WIDTH: f32 = 2.0;
/// Curve samples per pixel of plot width (inverse).
const CURVE_SAMPLE_PX: f32 = 1.0;
/// Band point radii: resting, selected, the selected ring, the hover halo.
const POINT_RADIUS: f32 = 9.0;
const SELECTED_RADIUS: f32 = 11.0;
const RING_RADIUS: f32 = 14.0;
const RING_WIDTH: f32 = 1.5;
const HALO_RADIUS: f32 = 14.0;
/// A press this close to a point's centre takes it.
const HIT_RADIUS: f32 = 14.0;
/// Band number text size.
const POINT_LABEL_SIZE: f32 = 10.0;

/// Alphas of the translucent theme-derived colors.
const GRID_ALPHA: f32 = 0.3;
const ZERO_LINE_ALPHA: f32 = 0.85;
const FILL_ALPHA: f32 = 0.18;
const HALO_ALPHA: f32 = 0.3;

// The selected point's ring fits inside the panel at the top and right edges.
const _: () = assert!(
    PAD_TOP >= RING_RADIUS + RING_WIDTH && PAD_RIGHT >= RING_RADIUS + RING_WIDTH,
    "the selected ring must fit between the plot and the panel edge",
);

// =============================================================================
// Builder
// =============================================================================

type OnSelect<'a, Message> = Box<dyn Fn(usize) -> Message + 'a>;
type OnMove<'a, Message> = Box<dyn Fn(usize, f64, f64) -> Message + 'a>;
type OnQ<'a, Message> = Box<dyn Fn(usize, f64) -> Message + 'a>;
type OnAdd<'a, Message> = Box<dyn Fn(f64, f64) -> Message + 'a>;
type OnRelease<'a, Message> = Box<dyn Fn(usize) -> Message + 'a>;

/// The band list a curve draws: borrowed, or the equalizer's own shared list
/// (`EqState::parametric_bands`), which a view holds no borrow of.
pub(crate) enum CurveBands<'a> {
    Borrowed(&'a [EqBand]),
    Shared(Arc<Vec<EqBand>>),
}

impl Deref for CurveBands<'_> {
    type Target = [EqBand];

    fn deref(&self) -> &[EqBand] {
        match self {
            Self::Borrowed(bands) => bands,
            Self::Shared(bands) => bands,
        }
    }
}

impl<'a> From<&'a [EqBand]> for CurveBands<'a> {
    fn from(bands: &'a [EqBand]) -> Self {
        Self::Borrowed(bands)
    }
}

impl<'a, const N: usize> From<&'a [EqBand; N]> for CurveBands<'a> {
    fn from(bands: &'a [EqBand; N]) -> Self {
        Self::Borrowed(bands)
    }
}

impl From<Arc<Vec<EqBand>>> for CurveBands<'_> {
    fn from(bands: Arc<Vec<EqBand>>) -> Self {
        Self::Shared(bands)
    }
}

/// The response curve with its band points. Build with [`eq_curve`].
pub(crate) struct EqCurve<'a, Message> {
    bands: CurveBands<'a>,
    selected: Option<usize>,
    dimmed: bool,
    width: Length,
    height: f32,
    on_select: Option<OnSelect<'a, Message>>,
    on_move: Option<OnMove<'a, Message>>,
    on_q: Option<OnQ<'a, Message>>,
    on_add: Option<OnAdd<'a, Message>>,
    on_release: Option<OnRelease<'a, Message>>,
}

/// The response curve of `bands`, read-only until callbacks are set.
pub(crate) fn eq_curve<'a, Message: Clone + 'a>(
    bands: impl Into<CurveBands<'a>>,
) -> EqCurve<'a, Message> {
    EqCurve {
        bands: bands.into(),
        selected: None,
        dimmed: false,
        width: Length::Fill,
        height: DEFAULT_HEIGHT,
        on_select: None,
        on_move: None,
        on_q: None,
        on_add: None,
        on_release: None,
    }
}

impl<'a, Message: Clone + 'a> EqCurve<'a, Message> {
    /// The band whose point is highlighted (an index past the list is ignored).
    pub(crate) fn selected(mut self, band: Option<usize>) -> Self {
        self.selected = band;
        self
    }

    /// Muted colors for the curve and points: the equalizer is switched off.
    pub(crate) fn dimmed(mut self, dimmed: bool) -> Self {
        self.dimmed = dimmed;
        self
    }

    pub(crate) fn width(mut self, w: impl Into<Length>) -> Self {
        self.width = w.into();
        self
    }

    pub(crate) fn height(mut self, h: f32) -> Self {
        self.height = h;
        self
    }

    /// A point was pressed: `(band)`.
    pub(crate) fn on_select(mut self, f: impl Fn(usize) -> Message + 'a) -> Self {
        self.on_select = Some(Box::new(f));
        self
    }

    /// A point was dragged: `(band, freq Hz, gain dB)`, rounded and clamped
    /// (a pass filter's gain stays 0).
    pub(crate) fn on_move(mut self, f: impl Fn(usize, f64, f64) -> Message + 'a) -> Self {
        self.on_move = Some(Box::new(f));
        self
    }

    /// The wheel turned over a point, or during its drag: `(band, new Q)`.
    pub(crate) fn on_q(mut self, f: impl Fn(usize, f64) -> Message + 'a) -> Self {
        self.on_q = Some(Box::new(f));
        self
    }

    /// Double-click on empty space: `(freq Hz, gain dB)` there.
    pub(crate) fn on_add(mut self, f: impl Fn(f64, f64) -> Message + 'a) -> Self {
        self.on_add = Some(Box::new(f));
        self
    }

    /// A drag that moved its band ended (the button came up): `(band)`. A
    /// press that only selected publishes nothing here, so the caller can
    /// persist once per drag instead of on every `on_move`.
    pub(crate) fn on_release(mut self, f: impl Fn(usize) -> Message + 'a) -> Self {
        self.on_release = Some(Box::new(f));
        self
    }
}

impl<'a, Message: Clone + 'a> From<EqCurve<'a, Message>> for Element<'a, Message> {
    fn from(curve: EqCurve<'a, Message>) -> Self {
        let (width, height) = (curve.width, curve.height);
        canvas::Canvas::new(curve)
            .width(width)
            .height(Length::Fixed(height))
            .boxed()
    }
}

impl<Message> EqCurve<'_, Message> {
    fn selected_index(&self) -> Option<usize> {
        self.selected.filter(|&i| i < self.bands.len())
    }

    fn point_centers(&self, plot: Rectangle, range: f64) -> impl Iterator<Item = Point> + '_ {
        self.bands
            .iter()
            .map(move |band| point_center(band, plot, range))
    }

    /// Whether pointing at a point does anything.
    fn points_respond(&self) -> bool {
        self.on_select.is_some() || self.on_move.is_some() || self.on_q.is_some()
    }

    /// The point under the cursor, when points respond to it.
    fn hover_at(&self, at: Option<Point>, plot: Rectangle, range: f64) -> Option<usize> {
        if !self.points_respond() {
            return None;
        }
        hit_test(self.point_centers(plot, range), at?, self.selected_index())
    }

    /// The gain range on screen: the content's, held at least at the drag's
    /// start range while a drag runs.
    fn display_range(&self, state: &State, bands_key: u64) -> f64 {
        let content = state.content_range(&self.bands, bands_key);
        state.drag.map_or(content, |drag| content.max(drag.range))
    }

    fn press(
        &self,
        state: &mut State,
        at: Point,
        plot: Rectangle,
        range: f64,
    ) -> Option<Action<Message>> {
        if self.on_select.is_some() || self.on_move.is_some() {
            let hit = hit_test(self.point_centers(plot, range), at, self.selected_index());
            if let Some((band, data)) = hit.and_then(|i| self.bands.get(i).map(|b| (i, b))) {
                state.last_click = None;
                if self.on_move.is_some() {
                    let center = point_center(data, plot, range);
                    state.drag = Some(Drag::new(band, at, center, range));
                }
                let action = self
                    .on_select
                    .as_ref()
                    .map_or_else(Action::request_redraw, |f| Action::publish(f(band)));
                return Some(action.and_capture());
            }
        }
        let on_add = self.on_add.as_ref()?;
        if hit_test(self.point_centers(plot, range), at, None).is_some() {
            // On a point that takes no press: not empty space.
            return None;
        }
        let click = mouse::Click::new(at, mouse::Button::Left, state.last_click);
        if click.kind() == mouse::click::Kind::Double {
            // Forget it, so a third click starts over instead of adding again.
            state.last_click = None;
            let (freq, gain) = value_at(at, EqBandKind::Peak, range, plot);
            return Some(Action::publish(on_add(freq, gain)).and_capture());
        }
        state.last_click = Some(click);
        Some(Action::capture())
    }

    fn cursor_moved(
        &self,
        state: &mut State,
        cursor: mouse::Cursor,
        bounds: Rectangle,
        plot: Rectangle,
        range: f64,
    ) -> Option<Action<Message>> {
        if let Some(drag) = state.drag.as_mut() {
            let Some(band) = self.bands.get(drag.band) else {
                // The band went away under the drag.
                state.drag = None;
                return Some(Action::request_redraw());
            };
            // Outside the canvas too: the drag follows the cursor anywhere
            // in the window, clamped to the plot's edges.
            let Some(position) = cursor.position() else {
                return Some(Action::capture());
            };
            let at = position - Vector::new(bounds.x, bounds.y);
            let action = match (drag.moved_to(at, band.kind, range, plot), &self.on_move) {
                (Some((freq, gain)), Some(on_move)) => {
                    Action::publish(on_move(drag.band, freq, gain))
                }
                (Some(_) | None, _) => Action::capture(),
            };
            return Some(action.and_capture());
        }
        // Hover moves between overlapping hit zones keep the same cursor
        // icon, so the canvas would not redraw the halo by itself.
        let hovered = self.hover_at(cursor.position_in(bounds), plot, range);
        if hovered == state.hovered {
            return None;
        }
        state.hovered = hovered;
        Some(Action::request_redraw())
    }

    fn wheel(
        &self,
        state: &mut State,
        delta: mouse::ScrollDelta,
        at: Option<Point>,
        plot: Rectangle,
        range: f64,
    ) -> Option<Action<Message>> {
        let on_q = self.on_q.as_ref()?;
        let band = match state.drag {
            Some(drag) => drag.band,
            None => hit_test(self.point_centers(plot, range), at?, self.selected_index())?,
        };
        let q = self.bands.get(band)?.q;
        let notches = state.wheel.take(band, delta);
        let next = q_step(q, notches);
        if notches == 0 || next.to_bits() == q.to_bits() {
            // Still the point's wheel: nothing behind it scrolls.
            return Some(Action::capture());
        }
        Some(Action::publish(on_q(band, next)).and_capture())
    }
}

// =============================================================================
// canvas::Program
// =============================================================================

impl<Message: Clone> canvas::Program<Message> for EqCurve<'_, Message> {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        let Event::Mouse(event) = event else {
            return None;
        };
        let plot = plot_rect(bounds.size());
        let range = self.display_range(state, bands_key(&self.bands));
        match *event {
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                // A drag whose release never arrived ends at the next press,
                // wherever it lands.
                let stale_drag = state.drag.take().is_some();
                match cursor.position_in(bounds) {
                    Some(at) => self.press(state, at, plot, range),
                    None => stale_drag.then(Action::request_redraw),
                }
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                state.drag.take().map(|drag| {
                    match (&self.on_release, drag.last) {
                        // It moved the band: one message for the whole drag.
                        (Some(on_release), Some(_)) => Action::publish(on_release(drag.band)),
                        (Some(_), None) | (None, _) => Action::request_redraw(),
                    }
                    .and_capture()
                })
            }
            mouse::Event::CursorMoved { .. } | mouse::Event::CursorLeft => {
                self.cursor_moved(state, cursor, bounds, plot, range)
            }
            mouse::Event::WheelScrolled { delta } => {
                self.wheel(state, delta, cursor.position_in(bounds), plot, range)
            }
            // Other buttons, and the cursor entering the window.
            mouse::Event::ButtonPressed(_)
            | mouse::Event::ButtonReleased(_)
            | mouse::Event::CursorEntered => None,
        }
    }

    fn draw(
        &self,
        state: &State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let size = bounds.size();
        let plot = plot_rect(size);
        let bands = bands_key(&self.bands);
        let range = self.display_range(state, bands);
        let style = style_key(range, self.dimmed);
        let selected = self.selected_index();
        let hovered = match state.drag {
            Some(drag) => Some(drag.band),
            None => self.hover_at(cursor.position_in(bounds), plot, range),
        };
        let dimmed = self.dimmed;

        let grid = state.grid.draw(renderer, size, style, |frame| {
            draw_grid(frame, plot, range, &Palette::current(dimmed));
        });
        let curve = state
            .curve
            .draw(renderer, size, key_of(&(style, bands)), |frame| {
                draw_curve(frame, &self.bands, plot, range, &Palette::current(dimmed));
            });
        let points_key = key_of(&(style, bands, selected, hovered));
        let points = state.points.draw(renderer, size, points_key, |frame| {
            draw_points(
                frame,
                &self.bands,
                plot,
                range,
                selected,
                hovered,
                &Palette::current(dimmed),
            );
        });
        vec![grid, curve, points]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.drag.is_some() {
            return mouse::Interaction::Grabbing;
        }
        let plot = plot_rect(bounds.size());
        let range = self.display_range(state, bands_key(&self.bands));
        if self
            .hover_at(cursor.position_in(bounds), plot, range)
            .is_none()
        {
            mouse::Interaction::default()
        } else if self.on_move.is_some() {
            mouse::Interaction::Grab
        } else if self.on_select.is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

// =============================================================================
// Widget state
// =============================================================================

/// The canvas state kept in the widget tree.
#[derive(Default)]
pub(crate) struct State {
    drag: Option<Drag>,
    /// The point under the cursor at the last move (redraw trigger only;
    /// `draw` reads the live cursor).
    hovered: Option<usize>,
    /// The last press on empty space, for double-click detection.
    last_click: Option<mouse::Click>,
    wheel: WheelAccum,
    /// `(bands_key, content range)` of the last band list measured.
    range_memo: Cell<Option<(u64, f64)>>,
    grid: Layer,
    curve: Layer,
    points: Layer,
}

impl State {
    fn content_range(&self, bands: &[EqBand], key: u64) -> f64 {
        if let Some((memo_key, range)) = self.range_memo.get()
            && memo_key == key
        {
            return range;
        }
        let range = range_for_peak(content_peak_db(bands));
        self.range_memo.set(Some((key, range)));
        range
    }
}

/// A canvas cache cleared whenever the key of what it draws changes. The
/// cache alone only redraws on a size change.
#[derive(Default)]
struct Layer {
    cache: canvas::Cache,
    key: Cell<Option<u64>>,
}

impl Layer {
    fn draw(
        &self,
        renderer: &iced::Renderer,
        size: Size,
        key: u64,
        content: impl FnOnce(&mut Frame),
    ) -> Geometry {
        if self.key.replace(Some(key)) != Some(key) {
            self.cache.clear();
        }
        self.cache.draw(renderer, size, content)
    }
}

/// A point being dragged.
#[derive(Debug, Clone, Copy)]
struct Drag {
    band: usize,
    /// Cursor at the press, canvas-relative.
    press: Point,
    /// Point centre minus cursor at the press: the point keeps that offset
    /// from the cursor, so it does not jump to it.
    grab: Vector,
    /// The gain range at the press, held for the drag.
    range: f64,
    /// Whether the cursor has left the slop circle.
    moving: bool,
    /// The last `(freq, gain)` published.
    last: Option<(f64, f64)>,
}

impl Drag {
    fn new(band: usize, press: Point, center: Point, range: f64) -> Self {
        Self {
            band,
            press,
            grab: center - press,
            range,
            moving: false,
            last: None,
        }
    }

    /// The cursor moved to `cursor` (canvas-relative): the `(freq, gain)` to
    /// publish, if the drag is past the slop and the value changed.
    fn moved_to(
        &mut self,
        cursor: Point,
        kind: EqBandKind,
        range: f64,
        plot: Rectangle,
    ) -> Option<(f64, f64)> {
        if !self.moving {
            if cursor.distance(self.press) < DRAG_SLOP {
                return None;
            }
            self.moving = true;
        }
        let value = value_at(cursor + self.grab, kind, range, plot);
        if self.last == Some(value) {
            return None;
        }
        self.last = Some(value);
        Some(value)
    }
}

/// Wheel travel not yet spent on a whole notch (hi-res wheels and touchpads
/// send fractions), for one band.
#[derive(Debug, Default, Clone, Copy)]
struct WheelAccum {
    band: Option<usize>,
    notches: f32,
}

impl WheelAccum {
    /// Add `delta` for `band` and take the whole notches out.
    fn take(&mut self, band: usize, delta: mouse::ScrollDelta) -> i32 {
        if self.band != Some(band) {
            *self = Self {
                band: Some(band),
                notches: 0.0,
            };
        }
        self.notches += match delta {
            mouse::ScrollDelta::Lines { y, .. } => y,
            mouse::ScrollDelta::Pixels { y, .. } => y / PIXELS_PER_NOTCH,
        };
        let whole = self.notches.trunc();
        self.notches -= whole;
        whole as i32
    }
}

// =============================================================================
// Geometry and values (pure)
// =============================================================================

/// The plot area inside a panel of `size`, canvas-relative.
fn plot_rect(size: Size) -> Rectangle {
    Rectangle {
        x: PAD_LEFT,
        y: PAD_TOP,
        width: (size.width - PAD_LEFT - PAD_RIGHT).max(1.0),
        height: (size.height - PAD_TOP - PAD_BOTTOM).max(1.0),
    }
}

/// x of `freq` on the log axis, held to the visible span.
fn freq_to_x(freq: f64, plot: Rectangle) -> f32 {
    // `max` also turns a NaN into the left edge.
    let fraction = ((freq.max(VIEW_FREQ_MIN) / VIEW_FREQ_MIN).ln()
        / (VIEW_FREQ_MAX / VIEW_FREQ_MIN).ln())
    .min(1.0);
    plot.x + plot.width * fraction as f32
}

/// Frequency at `x`, held to the visible span.
fn x_to_freq(x: f32, plot: Rectangle) -> f64 {
    let fraction = f64::from(((x - plot.x) / plot.width).clamp(0.0, 1.0));
    VIEW_FREQ_MIN * (VIEW_FREQ_MAX / VIEW_FREQ_MIN).powf(fraction)
}

/// y of `db` on a ±`range` axis (not clamped).
fn gain_to_y(db: f64, range: f64, plot: Rectangle) -> f32 {
    plot.y + plot.height * (0.5 - db / (2.0 * range)) as f32
}

/// Gain at `y` on a ±`range` axis (not clamped).
fn y_to_gain(y: f32, range: f64, plot: Rectangle) -> f64 {
    (0.5 - f64::from((y - plot.y) / plot.height)) * 2.0 * range
}

/// The smallest range that holds `peak_db` (the largest excursion either
/// way): ±12, then 6 dB steps, at most ±30.
fn range_for_peak(peak_db: f64) -> f64 {
    // The epsilon keeps a curve that tops out at exactly a step (a +12 dB
    // slider) in that step despite float noise; `max` drops a NaN.
    let over = (peak_db - RANGE_MIN_DB - 1e-6).max(0.0);
    (RANGE_MIN_DB + (over / RANGE_STEP_DB).ceil() * RANGE_STEP_DB).min(RANGE_MAX_DB)
}

/// The largest excursion from 0 dB the plot must show: every point's gain,
/// the curve's highest value, and its deepest cut from the gain filters
/// alone (a pass filter's stop band falls without bound, so it may leave
/// the plot; its resonant peak counts through the highest value).
fn content_peak_db(bands: &[EqBand]) -> f64 {
    let points = bands
        .iter()
        .map(|band| point_gain(band).abs())
        .fold(0.0, f64::max);
    let full = Response::new(bands, DISPLAY_RATE);
    if full.is_flat() {
        return points;
    }
    // The gain filters alone, in a fixed array: no allocation.
    let mut shaping = [EqBand::new(EqBandKind::Peak, 1_000.0, 0.0, EQ_DEFAULT_Q); EQ_MAX_BANDS];
    let mut len = 0;
    for band in bands
        .iter()
        .filter(|band| band.kind.has_gain() && !band.is_flat())
        .take(EQ_MAX_BANDS)
    {
        shaping[len] = *band;
        len += 1;
    }
    let gains = Response::new(&shaping[..len], DISPLAY_RATE);
    scan_freqs(bands).fold(points, |peak, freq| {
        peak.max(full.db(freq)).max(-gains.db(freq))
    })
}

/// A log grid over the visible span plus each band's own frequency (a
/// narrow peak can fall between grid points).
fn scan_freqs(bands: &[EqBand]) -> impl Iterator<Item = f64> + '_ {
    let ratio = VIEW_FREQ_MAX / VIEW_FREQ_MIN;
    let steps = (ratio.log2() * SCAN_STEPS_PER_OCTAVE).ceil() as u32;
    (0..=steps)
        .map(move |i| VIEW_FREQ_MIN * ratio.powf(f64::from(i) / f64::from(steps)))
        .chain(
            bands
                .iter()
                .map(|band| band.clamped().freq.clamp(VIEW_FREQ_MIN, VIEW_FREQ_MAX)),
        )
}

/// The gain a band's point shows: its own (within the band limit), or 0 for
/// a pass filter.
fn point_gain(band: &EqBand) -> f64 {
    if band.kind.has_gain() {
        band.clamped().gain
    } else {
        0.0
    }
}

/// Where a band's point sits, inside the plot.
fn point_center(band: &EqBand, plot: Rectangle, range: f64) -> Point {
    Point::new(
        freq_to_x(band.clamped().freq, plot),
        gain_to_y(point_gain(band).clamp(-range, range), range, plot),
    )
}

/// The point a press or hover at `cursor` takes: inside the selected point's
/// disc that one, else the nearest within [`HIT_RADIUS`]. A tie goes to the
/// selected point, then to the later band (drawn on top).
fn hit_test(
    points: impl Iterator<Item = Point>,
    cursor: Point,
    selected: Option<usize>,
) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (i, point) in points.enumerate() {
        let distance = point.distance(cursor);
        if Some(i) == selected && distance <= SELECTED_RADIUS {
            return Some(i);
        }
        if distance > HIT_RADIUS {
            continue;
        }
        let better = best.is_none_or(|(best_i, best_distance)| {
            distance < best_distance || (distance <= best_distance && Some(best_i) != selected)
        });
        if better {
            best = Some((i, distance));
        }
    }
    best.map(|(i, _)| i)
}

/// The `(freq, gain)` at point centre `center`: frequency within the visible
/// span, rounded to 1 Hz under 1 kHz and 10 Hz above; gain within the range
/// and the band limit, snapped to 0 near it, rounded to 0.1 dB; 0 for a pass
/// filter.
fn value_at(center: Point, kind: EqBandKind, range: f64, plot: Rectangle) -> (f64, f64) {
    let freq = round_freq(x_to_freq(center.x, plot));
    let gain = if kind.has_gain() {
        let limit = range.min(EQ_BAND_GAIN_DB_MAX);
        round_gain(y_to_gain(center.y, range, plot).clamp(-limit, limit))
    } else {
        0.0
    };
    (freq, gain)
}

fn round_freq(freq: f64) -> f64 {
    let step = if freq < 1_000.0 { 1.0 } else { 10.0 };
    ((freq / step).round() * step).clamp(VIEW_FREQ_MIN, VIEW_FREQ_MAX)
}

fn round_gain(gain: f64) -> f64 {
    if gain.abs() <= ZERO_SNAP_DB {
        0.0
    } else {
        (gain * 10.0).round() / 10.0
    }
}

/// `q` moved `notches` wheel notches (up is narrower), rounded to 0.01 and
/// held to the engine's Q bounds.
fn q_step(q: f64, notches: i32) -> f64 {
    let q = if q.is_finite() { q } else { EQ_DEFAULT_Q };
    let next = q * Q_WHEEL_FACTOR.powi(notches);
    ((next * 100.0).round() / 100.0).clamp(EQ_Q_MIN, EQ_Q_MAX)
}

/// A frequency axis label: `20`, `500`, `1k`, `1.5k`, `20k`.
fn freq_label(hz: f64) -> String {
    if hz >= 1_000.0 {
        let khz = hz / 1_000.0;
        if (khz - khz.round()).abs() < 1e-9 {
            format!("{khz:.0}k")
        } else {
            format!("{khz:.1}k")
        }
    } else {
        format!("{hz:.0}")
    }
}

/// A gain axis label: `+6`, `0`, `-6`.
fn db_label(db: f64) -> String {
    let db = db.round();
    if db > 0.0 {
        format!("+{db:.0}")
    } else if db < 0.0 {
        format!("{db:.0}")
    } else {
        "0".to_owned()
    }
}

/// dB between two labelled grid lines: every line (6 dB) unless that packs
/// the labels closer than [`MIN_LABEL_SPACING`], then every other, and so on.
fn db_label_step(range: f64, plot_height: f32) -> f64 {
    let px_per_db = f64::from(plot_height) / (2.0 * range);
    let mut step = GRID_STEP_DB;
    while step * px_per_db < f64::from(MIN_LABEL_SPACING) && step < range {
        step += GRID_STEP_DB;
    }
    step
}

/// The part of segment `a`→`b` between `top` and `bottom` (y only: the
/// curve's x never leaves the plot). An end point inside comes back as is,
/// so neighbouring pieces join exactly.
fn clip_segment(a: Point, b: Point, top: f32, bottom: f32) -> Option<(Point, Point)> {
    let dy = b.y - a.y;
    if dy.abs() < f32::EPSILON {
        return (a.y >= top && a.y <= bottom).then_some((a, b));
    }
    let (t_top, t_bottom) = ((top - a.y) / dy, (bottom - a.y) / dy);
    let t0 = t_top.min(t_bottom).max(0.0);
    let t1 = t_top.max(t_bottom).min(1.0);
    if t0 > t1 {
        return None;
    }
    let at = |t: f32| Point::new(a.x + (b.x - a.x) * t, a.y + dy * t);
    let start = if t0 <= 0.0 { a } else { at(t0) };
    let end = if t1 >= 1.0 { b } else { at(t1) };
    Some((start, end))
}

/// Start (top or left edge) of the 1 px pixel line whose centre is nearest
/// `c`, so the grid's hairlines stay crisp instead of smearing over two.
fn pixel_line(c: f32) -> f32 {
    (c - 0.5).round()
}

// =============================================================================
// Cache keys
// =============================================================================

fn key_of(value: &impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// Everything the band list contributes to a drawing (also the EQ modal's
/// preamp memo key).
pub(crate) fn bands_key(bands: &[EqBand]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bands.len().hash(&mut hasher);
    for band in bands {
        band.kind.hash(&mut hasher);
        band.freq.to_bits().hash(&mut hasher);
        band.gain.to_bits().hash(&mut hasher);
        band.q.to_bits().hash(&mut hasher);
        band.enabled.hash(&mut hasher);
    }
    hasher.finish()
}

/// Everything else every layer depends on: the scale, the dimming, the
/// palette (its generation counter), the font and the corner radius.
fn style_key(range: f64, dimmed: bool) -> u64 {
    key_of(&(
        range.to_bits(),
        dimmed,
        theme::theme_generation(),
        theme::ui_font(),
        theme::ui_radius_md().top_left.to_bits(),
    ))
}

// =============================================================================
// Drawing
// =============================================================================

/// The theme colors one drawing uses, read once.
struct Palette {
    panel: Color,
    outline: Color,
    grid: Color,
    zero: Color,
    label: Color,
    curve: Color,
    fill: Color,
    point: Color,
    point_ink: Color,
    off_point: Color,
    off_ink: Color,
    ring: Color,
    halo: Color,
}

impl Palette {
    fn current(dimmed: bool) -> Self {
        let fg4 = theme::fg4();
        // Off: the curve and the live points take the muted foreground.
        let (curve, point, point_ink, ring) = if dimmed {
            (fg4, fg4, theme::bg0_hard(), theme::fg3())
        } else {
            (
                theme::accent_bright(),
                theme::accent_fill(),
                theme::on_accent_fill(),
                theme::accent_bright(),
            )
        };
        let fill = if dimmed { fg4 } else { theme::accent_fill() };
        Self {
            panel: theme::bg1(),
            outline: theme::border(),
            grid: with_alpha(fg4, GRID_ALPHA),
            zero: with_alpha(fg4, ZERO_LINE_ALPHA),
            label: theme::fg3(),
            curve,
            fill: with_alpha(fill, FILL_ALPHA),
            point,
            point_ink,
            off_point: theme::bg2(),
            off_ink: fg4,
            ring,
            halo: with_alpha(point, HALO_ALPHA),
        }
    }
}

fn with_alpha(color: Color, alpha: f32) -> Color {
    Color {
        a: color.a * alpha,
        ..color
    }
}

fn label(frame: &mut Frame, content: String, at: Point, align: Horizontal, color: Color) {
    frame.fill_text(canvas::Text {
        content,
        position: at,
        color,
        size: Pixels(LABEL_SIZE),
        line_height: theme::UI_LINE_HEIGHT,
        font: theme::ui_font(),
        align_x: align.into(),
        align_y: Vertical::Center,
        ..Default::default()
    });
}

/// The panel, the grid and its labels, the 0 dB line.
fn draw_grid(frame: &mut Frame, plot: Rectangle, range: f64, palette: &Palette) {
    let size = frame.size();
    let radius = theme::ui_radius_md();
    frame.fill(
        &Path::rounded_rectangle(Point::ORIGIN, size, radius),
        palette.panel,
    );

    let bottom = plot.y + plot.height;
    for hz in FREQ_TICKS {
        let x = freq_to_x(hz, plot);
        frame.fill_rectangle(
            Point::new(pixel_line(x), plot.y),
            Size::new(1.0, plot.height),
            palette.grid,
        );
        let at = Point::new(x, bottom + LABEL_GAP + LABEL_SIZE / 2.0);
        label(frame, freq_label(hz), at, Horizontal::Center, palette.label);
    }

    let lines = (range / GRID_STEP_DB).round() as i32;
    let label_every = ((db_label_step(range, plot.height) / GRID_STEP_DB).round() as i32).max(1);
    for k in -lines..=lines {
        let db = f64::from(k) * GRID_STEP_DB;
        let y = gain_to_y(db, range, plot);
        let color = if k == 0 { palette.zero } else { palette.grid };
        frame.fill_rectangle(
            Point::new(plot.x, pixel_line(y)),
            Size::new(plot.width, 1.0),
            color,
        );
        if k % label_every == 0 {
            let at = Point::new(plot.x - LABEL_GAP, y);
            label(frame, db_label(db), at, Horizontal::Right, palette.label);
        }
    }

    // Outline last, on top of the grid ends: a 1 px hairline on the pixel
    // grid, like the EQ slider track.
    frame.stroke(
        &Path::rounded_rectangle(
            Point::new(0.5, 0.5),
            Size::new(size.width - 1.0, size.height - 1.0),
            radius,
        ),
        Stroke::default()
            .with_color(palette.outline)
            .with_width(1.0),
    );
}

/// The combined response: a translucent fill to the 0 dB line under an
/// accent stroke, both held to the plot.
fn draw_curve(frame: &mut Frame, bands: &[EqBand], plot: Rectangle, range: f64, palette: &Palette) {
    let response = Response::new(bands, DISPLAY_RATE);
    let steps = (plot.width / CURVE_SAMPLE_PX).ceil().max(2.0) as u32;
    let samples: Vec<Point> = (0..=steps)
        .map(|i| {
            let x = plot.x + plot.width * (i as f32 / steps as f32);
            Point::new(x, gain_to_y(response.db(x_to_freq(x, plot)), range, plot))
        })
        .collect();
    let (top, bottom) = (plot.y, plot.y + plot.height);
    let zero = gain_to_y(0.0, range, plot);

    if let (Some(first), Some(last)) = (samples.first(), samples.last()) {
        let fill = Path::new(|path| {
            path.move_to(Point::new(first.x, zero));
            for sample in &samples {
                path.line_to(Point::new(sample.x, sample.y.clamp(top, bottom)));
            }
            path.line_to(Point::new(last.x, zero));
            path.close();
        });
        frame.fill(&fill, palette.fill);
    }

    let line = Path::new(|path| {
        let mut pen: Option<Point> = None;
        for pair in samples.windows(2) {
            let [a, b] = [pair[0], pair[1]];
            if let Some((start, end)) = clip_segment(a, b, top, bottom) {
                if pen != Some(start) {
                    path.move_to(start);
                }
                path.line_to(end);
                pen = Some(end);
            }
        }
    });
    frame.stroke(
        &line,
        Stroke::default()
            .with_color(palette.curve)
            .with_width(CURVE_WIDTH)
            .with_line_join(LineJoin::Round)
            .with_line_cap(LineCap::Round),
    );
}

/// One numbered disc per band; the selected one last (on top), larger and
/// ringed; the hovered one haloed.
fn draw_points(
    frame: &mut Frame,
    bands: &[EqBand],
    plot: Rectangle,
    range: f64,
    selected: Option<usize>,
    hovered: Option<usize>,
    palette: &Palette,
) {
    let number_font = theme::weighted_ui_font(Weight::Bold);
    let order = (0..bands.len())
        .filter(|&i| Some(i) != selected)
        .chain(selected);
    for i in order {
        let Some(band) = bands.get(i) else {
            continue;
        };
        let center = point_center(band, plot, range);
        let is_selected = Some(i) == selected;
        if is_selected {
            frame.stroke(
                &Path::circle(center, RING_RADIUS),
                Stroke::default()
                    .with_color(palette.ring)
                    .with_width(RING_WIDTH),
            );
        } else if Some(i) == hovered {
            frame.fill(&Path::circle(center, HALO_RADIUS), palette.halo);
        }
        let (fill, outline, ink) = if band.enabled {
            (palette.point, palette.outline, palette.point_ink)
        } else {
            (palette.off_point, palette.off_ink, palette.off_ink)
        };
        let disc = Path::circle(
            center,
            if is_selected {
                SELECTED_RADIUS
            } else {
                POINT_RADIUS
            },
        );
        frame.fill(&disc, fill);
        frame.stroke(&disc, Stroke::default().with_color(outline).with_width(1.0));
        frame.fill_text(canvas::Text {
            content: (i + 1).to_string(),
            position: center,
            color: ink,
            size: Pixels(POINT_LABEL_SIZE),
            line_height: theme::UI_LINE_HEIGHT,
            font: number_font,
            align_x: Horizontal::Center.into(),
            align_y: Vertical::Center,
            ..Default::default()
        });
    }
}

#[cfg(test)]
mod tests {
    use iced::{Point, Size, mouse::ScrollDelta};
    use nokkvi_data::audio::eq::{
        EQ_BAND_GAIN_DB_MAX, EQ_DEFAULT_Q, EQ_Q_MAX, EQ_Q_MIN, EqBand, EqBandKind,
    };

    use super::*;

    fn plot() -> Rectangle {
        plot_rect(Size::new(600.0, 220.0))
    }

    fn close(a: f64, b: f64, tolerance: f64) -> bool {
        (a - b).abs() <= tolerance
    }

    #[test]
    fn plot_sits_inside_the_panel() {
        let plot = plot();
        assert!(plot.x > 0.0 && plot.y > 0.0);
        assert!(plot.x + plot.width < 600.0);
        assert!(plot.y + plot.height < 220.0);
    }

    #[test]
    fn frequency_axis_is_logarithmic_from_20_hz_to_20_khz() {
        let plot = plot();
        assert!((freq_to_x(20.0, plot) - plot.x).abs() < 1e-3);
        assert!((freq_to_x(20_000.0, plot) - (plot.x + plot.width)).abs() < 1e-3);
        // The geometric middle of 20 Hz..20 kHz is sqrt(20 * 20000) = 632 Hz.
        let middle = plot.x + plot.width / 2.0;
        assert!((freq_to_x(632.456, plot) - middle).abs() < 0.01);
        assert!(close(x_to_freq(middle, plot), 632.456, 0.01));
        for hz in [20.0, 31.0, 100.0, 1_000.0, 4_321.0, 20_000.0] {
            let back = x_to_freq(freq_to_x(hz, plot), plot);
            assert!(close(back, hz, hz * 1e-4), "{hz} Hz came back as {back}");
        }
    }

    #[test]
    fn frequency_axis_clamps_to_the_visible_range() {
        let plot = plot();
        assert!(close(x_to_freq(plot.x - 50.0, plot), VIEW_FREQ_MIN, 1e-9));
        assert!(close(
            x_to_freq(plot.x + plot.width + 50.0, plot),
            VIEW_FREQ_MAX,
            1e-6
        ));
        // A band below 20 Hz (the engine takes 10 Hz) sits on the left edge.
        assert!((freq_to_x(10.0, plot) - plot.x).abs() < 1e-3);
        assert!((freq_to_x(22_000.0, plot) - (plot.x + plot.width)).abs() < 1e-3);
    }

    #[test]
    fn gain_axis_is_symmetric_around_zero() {
        let plot = plot();
        assert!((gain_to_y(12.0, 12.0, plot) - plot.y).abs() < 1e-3);
        assert!((gain_to_y(-12.0, 12.0, plot) - (plot.y + plot.height)).abs() < 1e-3);
        assert!((gain_to_y(0.0, 18.0, plot) - (plot.y + plot.height / 2.0)).abs() < 1e-3);
        for range in [12.0, 18.0, 30.0] {
            for db in [-range, -7.5, 0.0, 0.3, 6.0, range] {
                let back = y_to_gain(gain_to_y(db, range, plot), range, plot);
                assert!(
                    close(back, db, 1e-3),
                    "{db} dB at ±{range} came back as {back}"
                );
            }
        }
    }

    #[test]
    fn range_is_twelve_db_at_least_and_grows_in_six_db_steps_to_thirty() {
        assert_eq!(range_for_peak(0.0), 12.0);
        assert_eq!(range_for_peak(11.9), 12.0);
        assert_eq!(range_for_peak(12.0), 12.0);
        assert_eq!(range_for_peak(12.1), 18.0);
        assert_eq!(range_for_peak(18.0), 18.0);
        assert_eq!(range_for_peak(23.0), 24.0);
        assert_eq!(range_for_peak(29.9), 30.0);
        assert_eq!(range_for_peak(45.0), RANGE_MAX_DB);
        assert_eq!(range_for_peak(f64::INFINITY), RANGE_MAX_DB);
        assert_eq!(range_for_peak(f64::NAN), 12.0);
    }

    #[test]
    fn range_fits_the_curve_and_every_point() {
        let range = |bands: &[EqBand]| range_for_peak(content_peak_db(bands));
        assert_eq!(range(&[]), 12.0);
        assert_eq!(
            range(&[EqBand::new(EqBandKind::Peak, 1_000.0, 5.0, 1.0)]),
            12.0
        );
        assert_eq!(
            range(&[EqBand::new(EqBandKind::Peak, 1_000.0, 20.0, 1.0)]),
            24.0
        );
        assert_eq!(
            range(&[EqBand::new(EqBandKind::Peak, 1_000.0, -15.0, 1.0)]),
            18.0
        );
        // Two overlapping peaks add up past either point.
        let stacked = [
            EqBand::new(EqBandKind::Peak, 1_000.0, 10.0, 1.0),
            EqBand::new(EqBandKind::Peak, 1_100.0, 10.0, 1.0),
        ];
        assert_eq!(range(&stacked), 24.0);
        // A switched-off point is still drawn, so it still fits.
        let off = EqBand {
            enabled: false,
            ..EqBand::new(EqBandKind::Peak, 1_000.0, 25.0, 1.0)
        };
        assert_eq!(range(&[off]), 30.0);
    }

    #[test]
    fn a_pass_filter_roll_off_does_not_widen_the_range() {
        let range = |bands: &[EqBand]| range_for_peak(content_peak_db(bands));
        // A low cut falls far below -30 dB at 20 Hz; it leaves the plot
        // instead of squashing every other band into a ±30 scale.
        assert_eq!(
            range(&[EqBand::new(EqBandKind::HighPass, 100.0, 0.0, EQ_DEFAULT_Q)]),
            12.0
        );
        // A resonant corner is real gain, so it does count.
        assert!(range(&[EqBand::new(EqBandKind::HighPass, 100.0, 0.0, 10.0)]) >= 18.0);
    }

    #[test]
    fn pass_filters_sit_on_the_zero_line() {
        let plot = plot();
        let zero = gain_to_y(0.0, 12.0, plot);
        let cut = EqBand::new(EqBandKind::LowPass, 8_000.0, 6.0, EQ_DEFAULT_Q);
        assert!((point_center(&cut, plot, 12.0).y - zero).abs() < 1e-3);
        let peak = EqBand::new(EqBandKind::Peak, 8_000.0, 6.0, 1.0);
        assert!((point_center(&peak, plot, 12.0).y - gain_to_y(6.0, 12.0, plot)).abs() < 1e-3);
        // Dragging a pass filter moves only its frequency.
        let top = Point::new(plot.x + plot.width / 2.0, plot.y);
        let (freq, gain) = value_at(top, EqBandKind::HighPass, 12.0, plot);
        assert_eq!(gain, 0.0);
        assert!(close(freq, 630.0, 3.0));
    }

    #[test]
    fn hit_test_picks_the_nearest_point_within_the_radius() {
        let points = [
            Point::new(100.0, 100.0),
            Point::new(120.0, 100.0),
            Point::new(300.0, 50.0),
        ];
        let hit = |at: Point, selected| hit_test(points.iter().copied(), at, selected);
        assert_eq!(hit(Point::new(103.0, 101.0), None), Some(0));
        assert_eq!(hit(Point::new(116.0, 100.0), None), Some(1));
        assert_eq!(
            hit(Point::new(300.0, 50.0 + HIT_RADIUS - 0.5), None),
            Some(2)
        );
        assert_eq!(hit(Point::new(300.0, 50.0 + HIT_RADIUS + 0.5), None), None);
        assert_eq!(hit(Point::new(200.0, 200.0), None), None);
    }

    #[test]
    fn hit_test_prefers_the_selected_point_where_points_overlap() {
        // Two points on the same spot: the selected one wins the tie; with
        // none selected, the one drawn on top (the later band) wins.
        let stacked = [Point::new(50.0, 50.0), Point::new(50.0, 50.0)];
        let at = Point::new(52.0, 50.0);
        assert_eq!(hit_test(stacked.iter().copied(), at, Some(0)), Some(0));
        assert_eq!(hit_test(stacked.iter().copied(), at, Some(1)), Some(1));
        assert_eq!(hit_test(stacked.iter().copied(), at, None), Some(1));
        // Inside the selected point's disc it wins even when a neighbour's
        // centre is nearer; outside its disc the nearer point wins.
        let near = [Point::new(50.0, 50.0), Point::new(62.0, 50.0)];
        let on_selected_rim = Point::new(50.0 + SELECTED_RADIUS - 1.0, 50.0);
        assert_eq!(
            hit_test(near.iter().copied(), on_selected_rim, Some(0)),
            Some(0)
        );
        assert_eq!(
            hit_test(near.iter().copied(), Point::new(62.0, 50.0), Some(0)),
            Some(1)
        );
    }

    #[test]
    fn drag_values_are_clamped_rounded_and_snapped() {
        let plot = plot();
        let at = |freq: f64, db: f64, range: f64| {
            Point::new(freq_to_x(freq, plot), gain_to_y(db, range, plot))
        };
        // Past the plot: the visible 20 Hz..20 kHz and the visible gain range.
        let far_left_top = Point::new(plot.x - 40.0, plot.y - 40.0);
        assert_eq!(
            value_at(far_left_top, EqBandKind::Peak, 12.0, plot),
            (20.0, 12.0)
        );
        let far_right_bottom = Point::new(plot.x + plot.width + 40.0, plot.y + plot.height + 40.0);
        assert_eq!(
            value_at(far_right_bottom, EqBandKind::Peak, 18.0, plot),
            (20_000.0, -18.0)
        );
        // Never past the band limit, whatever the range.
        assert_eq!(
            value_at(far_left_top, EqBandKind::Peak, 36.0, plot).1,
            EQ_BAND_GAIN_DB_MAX
        );
        // 1 Hz under 1 kHz, 10 Hz above; 0.1 dB.
        let (freq, gain) = value_at(at(123.4, 3.13721, 12.0), EqBandKind::Peak, 12.0, plot);
        assert!(close(freq, 123.0, 0.0) || close(freq, 124.0, 0.0), "{freq}");
        assert!(close(gain, 3.1, 1e-9), "{gain}");
        let (freq, _) = value_at(at(4_321.0, 0.0, 12.0), EqBandKind::Peak, 12.0, plot);
        assert_eq!(freq % 10.0, 0.0, "{freq}");
        assert!(close(freq, 4_320.0, 10.0), "{freq}");
        // Within ±0.25 dB of 0 the gain snaps to 0.
        assert_eq!(
            value_at(at(1_000.0, 0.2, 12.0), EqBandKind::Peak, 12.0, plot).1,
            0.0
        );
        assert_eq!(
            value_at(at(1_000.0, -0.24, 12.0), EqBandKind::Peak, 12.0, plot).1,
            0.0
        );
        assert!(close(
            value_at(at(1_000.0, 0.31, 12.0), EqBandKind::Peak, 12.0, plot).1,
            0.3,
            1e-9
        ));
    }

    #[test]
    fn a_drag_waits_past_the_slop_and_skips_repeats() {
        let plot = plot();
        let band = EqBand::new(EqBandKind::Peak, 1_000.0, 0.0, 1.0);
        let center = point_center(&band, plot, 12.0);
        // Pressed 4 px right of the centre: the grab offset keeps the point
        // under the same spot of the cursor, so it does not jump on press.
        let press = Point::new(center.x + 4.0, center.y);
        let mut drag = Drag::new(0, press, center, 12.0);
        let jitter = Point::new(press.x + DRAG_SLOP - 1.0, press.y);
        assert_eq!(drag.moved_to(jitter, band.kind, 12.0, plot), None);
        let up = Point::new(press.x, gain_to_y(6.0, 12.0, plot));
        let (freq, gain) = drag
            .moved_to(up, band.kind, 12.0, plot)
            .expect("past the slop");
        assert!(close(freq, 1_000.0, 10.0), "{freq}");
        assert!(close(gain, 6.0, 0.1), "{gain}");
        assert_eq!(
            drag.moved_to(up, band.kind, 12.0, plot),
            None,
            "no repeat publish"
        );
        // Back inside the slop after it was passed: the drag stays live.
        assert!(drag.moved_to(press, band.kind, 12.0, plot).is_some());
    }

    #[test]
    fn wheel_steps_q_by_a_sixth_of_an_octave_and_clamps() {
        assert!((Q_WHEEL_FACTOR.powi(6) - 2.0).abs() < 1e-12);
        assert_eq!(q_step(1.0, 1), 1.12);
        assert_eq!(q_step(1.0, 6), 2.0);
        assert_eq!(q_step(1.0, -6), 0.5);
        assert_eq!(q_step(19.0, 3), EQ_Q_MAX);
        assert_eq!(q_step(0.12, -5), EQ_Q_MIN);
        assert_eq!(q_step(EQ_Q_MAX, 1), EQ_Q_MAX);
        assert_eq!(q_step(EQ_Q_MIN, -1), EQ_Q_MIN);
        // The lowest Q still moves up a notch after rounding.
        assert!(q_step(EQ_Q_MIN, 1) > EQ_Q_MIN);
        assert_eq!(q_step(f64::NAN, 0), 0.71);
    }

    #[test]
    fn wheel_notches_accumulate_per_band() {
        let mut wheel = WheelAccum::default();
        assert_eq!(wheel.take(0, ScrollDelta::Lines { x: 0.0, y: 1.0 }), 1);
        assert_eq!(wheel.take(0, ScrollDelta::Lines { x: 0.0, y: -2.0 }), -2);
        // Touchpad pixels add up to a notch.
        assert_eq!(wheel.take(0, ScrollDelta::Pixels { x: 0.0, y: 6.0 }), 0);
        assert_eq!(wheel.take(0, ScrollDelta::Pixels { x: 0.0, y: 6.0 }), 1);
        // A half notch left over does not carry to another band.
        assert_eq!(wheel.take(1, ScrollDelta::Lines { x: 0.0, y: 0.5 }), 0);
        assert_eq!(wheel.take(2, ScrollDelta::Lines { x: 0.0, y: 0.5 }), 0);
        assert_eq!(wheel.take(2, ScrollDelta::Lines { x: 0.0, y: 0.5 }), 1);
    }

    #[test]
    fn labels_read_like_an_eq_axis() {
        assert_eq!(freq_label(20.0), "20");
        assert_eq!(freq_label(500.0), "500");
        assert_eq!(freq_label(1_000.0), "1k");
        assert_eq!(freq_label(1_500.0), "1.5k");
        assert_eq!(freq_label(20_000.0), "20k");
        assert_eq!(db_label(6.0), "+6");
        assert_eq!(db_label(0.0), "0");
        assert_eq!(db_label(-0.0), "0");
        assert_eq!(db_label(-6.0), "-6");
        assert_eq!(db_label(30.0), "+30");
    }

    #[test]
    fn db_labels_thin_out_when_the_lines_crowd() {
        // ±12 over 182 px: a line every 45 px, every line labelled.
        assert_eq!(db_label_step(12.0, 182.0), 6.0);
        // ±30 over 100 px: 6 dB is 10 px, so label every other line.
        assert_eq!(db_label_step(30.0, 100.0), 12.0);
        // Never wider than the range itself.
        assert_eq!(db_label_step(12.0, 4.0), 12.0);
    }

    #[test]
    fn the_stroke_is_cut_where_it_leaves_the_plot() {
        let (top, bottom) = (10.0, 110.0);
        let near = |p: Point, q: Point| p.distance(q) < 1e-4;
        let cut = |a: Point, b: Point| clip_segment(a, b, top, bottom);
        // Wholly inside: the very same end points, so the pieces of the
        // polyline join without a gap.
        let a = Point::new(0.0, 50.0);
        let b = Point::new(10.0, 60.0);
        assert_eq!(cut(a, b), Some((a, b)));
        // Leaves through the bottom halfway along.
        let (start, end) = cut(a, Point::new(10.0, 170.0)).expect("half inside");
        assert_eq!(start, a);
        assert!(near(end, Point::new(5.0, 110.0)), "{end:?}");
        // Wholly below the plot: nothing to draw.
        assert_eq!(cut(Point::new(0.0, 120.0), Point::new(1.0, 200.0)), None);
        // Crosses the whole plot.
        let (start, end) = cut(Point::new(0.0, 0.0), Point::new(12.0, 120.0)).expect("crosses");
        assert!(near(start, Point::new(1.0, 10.0)), "{start:?}");
        assert!(near(end, Point::new(11.0, 110.0)), "{end:?}");
        // Flat and outside.
        assert_eq!(cut(Point::new(0.0, 5.0), Point::new(1.0, 5.0)), None);
    }

    // -- The canvas program, driven with synthetic mouse events -------------

    #[derive(Debug, Clone, PartialEq)]
    enum Msg {
        Select(usize),
        Move(usize, f64, f64),
        Q(usize, f64),
        Add(f64, f64),
        Release(usize),
    }

    /// Off the window origin, so a canvas-relative / absolute mix-up shows.
    const BOUNDS: Rectangle = Rectangle {
        x: 100.0,
        y: 50.0,
        width: 600.0,
        height: 220.0,
    };

    fn editable(bands: &[EqBand]) -> EqCurve<'_, Msg> {
        eq_curve(bands)
            .on_select(Msg::Select)
            .on_move(Msg::Move)
            .on_q(Msg::Q)
            .on_add(Msg::Add)
            .on_release(Msg::Release)
    }

    /// One mouse event with the cursor at canvas-relative `at`: the message
    /// published, and whether the event was captured.
    fn send(
        curve: &EqCurve<'_, Msg>,
        state: &mut State,
        event: mouse::Event,
        at: Point,
    ) -> Option<(Option<Msg>, bool)> {
        let cursor = mouse::Cursor::Available(Point::new(BOUNDS.x + at.x, BOUNDS.y + at.y));
        canvas::Program::update(curve, state, &Event::Mouse(event), BOUNDS, cursor).map(|action| {
            let (message, _, status) = action.into_inner();
            (message, status == iced::event::Status::Captured)
        })
    }

    const PRESS: mouse::Event = mouse::Event::ButtonPressed(mouse::Button::Left);
    const RELEASE: mouse::Event = mouse::Event::ButtonReleased(mouse::Button::Left);
    const MOVE: mouse::Event = mouse::Event::CursorMoved {
        position: Point::ORIGIN,
    };
    const NOTCH_UP: mouse::Event = mouse::Event::WheelScrolled {
        delta: ScrollDelta::Lines { x: 0.0, y: 1.0 },
    };

    fn two_bands() -> [EqBand; 2] {
        [
            EqBand::new(EqBandKind::Peak, 100.0, 3.0, 1.0),
            EqBand::new(EqBandKind::HighShelf, 5_000.0, -4.0, EQ_DEFAULT_Q),
        ]
    }

    fn center_of(bands: &[EqBand], i: usize) -> Point {
        let plot = plot_rect(BOUNDS.size());
        let range = range_for_peak(content_peak_db(bands));
        point_center(&bands[i], plot, range)
    }

    #[test]
    fn a_read_only_curve_captures_nothing() {
        let bands = two_bands();
        let curve = eq_curve::<Msg>(&bands);
        let mut state = State::default();
        let on_point = center_of(&bands, 1);
        assert_eq!(send(&curve, &mut state, PRESS, on_point), None);
        assert_eq!(send(&curve, &mut state, NOTCH_UP, on_point), None);
        assert_eq!(
            send(&curve, &mut state, MOVE, Point::new(300.0, 60.0)),
            None
        );
        assert_eq!(send(&curve, &mut state, RELEASE, on_point), None);
    }

    #[test]
    fn press_selects_drag_moves_and_release_ends_it() {
        let bands = two_bands();
        let curve = editable(&bands);
        let mut state = State::default();
        let on_point = center_of(&bands, 1);
        assert_eq!(
            send(&curve, &mut state, PRESS, on_point),
            Some((Some(Msg::Select(1)), true))
        );
        // Inside the slop: captured, nothing published yet.
        let jitter = Point::new(on_point.x + 1.0, on_point.y);
        assert_eq!(send(&curve, &mut state, MOVE, jitter), Some((None, true)));
        let plot = plot_rect(BOUNDS.size());
        let up = Point::new(on_point.x, gain_to_y(6.0, 12.0, plot));
        let Some((Some(Msg::Move(band, freq, gain)), true)) = send(&curve, &mut state, MOVE, up)
        else {
            panic!("the drag publishes a captured move");
        };
        assert_eq!(band, 1);
        assert!(close(freq, 5_000.0, 10.0), "{freq}");
        assert!(close(gain, 6.0, 0.1), "{gain}");
        // A drag follows the cursor outside the canvas, clamped to the plot.
        let outside = Point::new(-500.0, -500.0);
        assert_eq!(
            send(&curve, &mut state, MOVE, outside),
            Some((Some(Msg::Move(1, 20.0, 12.0)), true))
        );
        // The release of a drag that moved names its band, once.
        assert_eq!(
            send(&curve, &mut state, RELEASE, outside),
            Some((Some(Msg::Release(1)), true))
        );
        // Released: moving again does nothing, and a release passes through.
        assert_eq!(send(&curve, &mut state, MOVE, up), None);
        assert_eq!(send(&curve, &mut state, RELEASE, up), None);
    }

    #[test]
    fn a_press_that_only_selects_publishes_no_release() {
        let bands = two_bands();
        let curve = editable(&bands);
        let mut state = State::default();
        let on_point = center_of(&bands, 0);
        assert_eq!(
            send(&curve, &mut state, PRESS, on_point),
            Some((Some(Msg::Select(0)), true))
        );
        // A jitter inside the slop moves nothing, so the release has nothing
        // to persist: it ends the drag without a message.
        let jitter = Point::new(on_point.x + 1.0, on_point.y + 1.0);
        assert_eq!(send(&curve, &mut state, MOVE, jitter), Some((None, true)));
        assert_eq!(
            send(&curve, &mut state, RELEASE, jitter),
            Some((None, true))
        );
    }

    #[test]
    fn a_drag_without_a_release_callback_ends_silently() {
        let bands = two_bands();
        let curve = eq_curve(&bands[..])
            .on_select(Msg::Select)
            .on_move(Msg::Move);
        let mut state = State::default();
        let on_point = center_of(&bands, 0);
        assert!(send(&curve, &mut state, PRESS, on_point).is_some());
        let plot = plot_rect(BOUNDS.size());
        let up = Point::new(on_point.x, gain_to_y(9.0, 12.0, plot));
        assert!(matches!(
            send(&curve, &mut state, MOVE, up),
            Some((Some(Msg::Move(0, _, _)), true))
        ));
        assert_eq!(send(&curve, &mut state, RELEASE, up), Some((None, true)));
    }

    #[test]
    fn a_shared_band_list_draws_like_a_borrowed_one() {
        let bands = two_bands();
        let shared = Arc::new(bands.to_vec());
        let borrowed = eq_curve::<Msg>(&bands).selected(Some(1));
        let owned = eq_curve::<Msg>(Arc::clone(&shared)).selected(Some(1));
        assert_eq!(bands_key(&borrowed.bands), bands_key(&owned.bands));
        assert_eq!(owned.selected_index(), Some(1));
    }

    #[test]
    fn wheel_over_a_point_steps_its_q_and_leaves_empty_space_alone() {
        let bands = two_bands();
        let curve = editable(&bands);
        let mut state = State::default();
        assert_eq!(
            send(&curve, &mut state, NOTCH_UP, center_of(&bands, 0)),
            Some((Some(Msg::Q(0, 1.12)), true))
        );
        // Empty space: the wheel scrolls whatever sits behind the curve.
        assert_eq!(
            send(&curve, &mut state, NOTCH_UP, Point::new(300.0, 30.0)),
            None
        );
    }

    #[test]
    fn double_click_on_empty_space_adds_a_band_there() {
        let bands = two_bands();
        let curve = editable(&bands);
        let mut state = State::default();
        let plot = plot_rect(BOUNDS.size());
        let empty = Point::new(freq_to_x(1_000.0, plot), gain_to_y(-3.0, 12.0, plot));
        assert_eq!(send(&curve, &mut state, PRESS, empty), Some((None, true)));
        assert_eq!(send(&curve, &mut state, RELEASE, empty), None);
        let Some((Some(Msg::Add(freq, gain)), true)) = send(&curve, &mut state, PRESS, empty)
        else {
            panic!("the second click adds a band");
        };
        assert!(close(freq, 1_000.0, 10.0), "{freq}");
        assert!(close(gain, -3.0, 0.1), "{gain}");
        // A third click starts a new click run instead of adding again.
        assert_eq!(send(&curve, &mut state, PRESS, empty), Some((None, true)));
    }

    #[test]
    fn an_out_of_range_selection_is_ignored() {
        let bands = two_bands();
        assert_eq!(
            eq_curve::<Msg>(&bands).selected(Some(1)).selected_index(),
            Some(1)
        );
        assert_eq!(
            eq_curve::<Msg>(&bands).selected(Some(2)).selected_index(),
            None
        );
    }
}
