//! Seekable progress bar, drawn in one of five looks ([`SeekbarStyle`]).
//!
//! - **Classic**: a 6 px track (`theme::bg2()`) with an `accent_fill()` fill and
//!   a 14 px square (flat) / pill (rounded) handle with a 1 px `bg0_hard()`
//!   border. Dragging the handle seeks; a track click jumps.
//! - **Line**: a 4 px track that thickens to 6 px and shows a 12 px handle at
//!   the playhead while the cursor is over it.
//! - **Capsule**: a full-height block with the times drawn inside, their ink
//!   switching where the fill passes under them.
//! - **Cells**: a row of discrete cells, the playhead's cell lifted toward the
//!   foreground.
//! - **Waveform**: the track's envelope ([`Waveform`]); draws as Line until the
//!   envelope arrives, then fades it in.
//!
//! Every handle-less style seeks from anywhere on the track (press, then drag)
//! and previews the hovered time: a tooltip, plus a marker (Capsule) or a
//! highlighted range between the playhead and the cursor (Cells, Waveform).
//! Times passed through [`ProgressBar::time_labels`] sit inside a Capsule and
//! beside the track for the other styles; beside the track their widths are
//! measured in layout (the elapsed time against a widest-digits template), so
//! the track's edges stay put while the elapsed time ticks.
//!
//! Based on Iced's slider widget event handling pattern. The handle draws in
//! its own layer and the seek tooltip through `overlay()`, so both sit above
//! neighboring widgets.

use std::{
    sync::{Arc, Weak},
    time::Instant,
};

use iced::{
    Color, Event, Length, Pixels, Point, Rectangle, Shadow, Size, Theme, Vector,
    advanced::{
        Shell,
        layout::{self, Layout},
        renderer,
        text::{self, Paragraph as _, Renderer as TextRenderer, Shaping, Text},
        widget::{self, Widget},
    },
    alignment, mouse, touch,
};
use nokkvi_data::types::{player_settings::SeekbarStyle, waveform::Waveform};

/// State for progress bar interaction
#[derive(Debug, Clone, Default)]
pub(crate) struct State {
    is_dragging: bool,
    drag_progress: f32,
    last_position: f32,
    last_update: Option<Instant>,
    /// The cursor's position over the track as a `0..=1` fraction, for the
    /// handle-less styles' hover preview. `None` while the cursor is elsewhere.
    hover: Option<f32>,
    /// Measured widths of the times drawn beside the track.
    caps: CapWidths,
    /// The waveform that last started fading in, and when. Held as a `Weak`
    /// so the allocation (and its address) can't be reused by the next song's
    /// envelope while the comparison still refers to it.
    waveform_since: Option<(Weak<Waveform>, Instant)>,
}

/// Widths of the left / right times beside the track, keyed by the templates
/// and the font they were measured with, so layout only re-shapes text when
/// one of those changes (the font picker changes the font live).
#[derive(Debug, Clone, Default)]
struct CapWidths {
    key: (String, String),
    font: Option<iced::Font>,
    left: f32,
    right: f32,
}

/// One end-cap label. `full` is the entire string (time + codec / bitrate),
/// drawn at the dimmer metadata ink; `time` is the elapsed / duration portion
/// re-drawn at full ink over the same outer-edge alignment anchor (left cap →
/// left edge, right cap → right edge), so only the codec / bitrate reads
/// dimmer than the time. When the cap carries no metadata, `time == full`.
#[derive(Debug, Clone)]
pub(crate) struct CapLabel {
    pub full: String,
    pub time: String,
}

impl CapLabel {
    /// Cap with no metadata — `time == full`, renders fully opaque.
    pub(crate) fn time_only(time: impl Into<String>) -> Self {
        let time = time.into();
        Self {
            full: time.clone(),
            time,
        }
    }

    /// Cap whose `time` stays opaque while the rest of `full` (codec / bitrate)
    /// renders dimmer.
    pub(crate) fn new(full: impl Into<String>, time: impl Into<String>) -> Self {
        Self {
            full: full.into(),
            time: time.into(),
        }
    }

    /// Whether there's a dimmer codec / bitrate segment to underpaint (i.e. the
    /// full string differs from the bare time).
    fn has_meta(&self) -> bool {
        self.full != self.time
    }
}

/// Seekable progress bar; see the module docs for the five looks.
pub(crate) struct ProgressBar<'a, Message> {
    position: f32,
    duration: f32,
    is_playing: bool,
    on_seek: Box<dyn Fn(f32) -> Message + 'a>,
    width: Length,
    height: f32,
    hide_handle: bool,
    interactive: bool,
    style: SeekbarStyle,
    /// `(left, right)` end-cap labels: inside a Capsule, beside the track for
    /// the other styles. See [`CapLabel`].
    time_labels: Option<(CapLabel, CapLabel)>,
    /// The track's envelope for the Waveform style; `None` draws Line.
    waveform: Option<Arc<Waveform>>,
}

/// Classic: track thickness within the widget bounds. The bounds may be
/// taller (to accommodate the handle hit target); the track is centered.
const TRACK_THICKNESS: f32 = 6.0;
/// Classic: handle size — 14 px square in flat mode, pill in rounded mode.
const HANDLE_SIZE: f32 = 14.0;
/// Line: track thickness at rest, and under the cursor / while dragging.
const LINE_THICKNESS: f32 = 4.0;
const LINE_THICKNESS_ACTIVE: f32 = 6.0;
/// Line: the handle shown at the playhead under the cursor.
const LINE_HANDLE_SIZE: f32 = 12.0;
/// Cells: how many cells span the track (fewer when they'd be thinner than
/// `MIN_CELL_WIDTH`), the gap between them, and their height.
const CELL_COUNT: usize = 64;
const CELL_GAP: f32 = 2.0;
const MIN_CELL_WIDTH: f32 = 4.0;
const CELL_HEIGHT: f32 = 12.0;
/// Cells (and waveform bars): corner radius in rounded mode. The `xs` scale
/// step would round a 12 px cell into a dot.
const CELL_RADIUS_ROUNDED: f32 = 2.0;
/// Waveform: bar pitch (bar + gap) the bar count is derived from, the gap,
/// and the inset above and below the tallest bar.
const WAVE_PITCH: f32 = 6.0;
const WAVE_GAP: f32 = 1.0;
const WAVE_INSET: f32 = 2.0;
/// How long a newly arrived waveform takes to fade in over the Line.
const WAVE_FADE_SECS: f32 = 0.2;
/// End caps: padding from the widget edge, gap to the track, text size.
pub(crate) const CAP_PAD: f32 = 10.0;
const CAP_GAP: f32 = 10.0;
const CAP_TEXT_SIZE: f32 = 11.0;
/// Codec / bitrate opacity relative to the time — reads as secondary.
const META_ALPHA: f32 = 0.6;
/// Capsule: width of the hover marker.
const HOVER_MARKER_WIDTH: f32 = 2.0;
/// How far the playhead cell, and the hovered played range, lift toward the
/// foreground from `accent_fill()`.
const PLAYHEAD_LIFT: f32 = 0.45;
const HOVER_LIFT: f32 = 0.25;
/// How far an unplayed cell under the hover range lifts toward the foreground
/// from `bg2()` — `bg3()` sits too close to `bg2()` on the muted palettes to
/// read as a highlight.
const UNPLAYED_HOVER_LIFT: f32 = 0.18;

impl<'a, Message> ProgressBar<'a, Message> {
    pub(crate) fn new<F>(position: f32, duration: f32, on_seek: F) -> Self
    where
        F: 'a + Fn(f32) -> Message,
    {
        Self {
            position: position.max(0.0),
            duration: duration.max(1.0), // Avoid division by zero
            is_playing: false,
            on_seek: Box::new(on_seek),
            width: Length::Fill,
            height: 24.0,
            hide_handle: false,
            interactive: true,
            style: SeekbarStyle::Classic,
            time_labels: None,
            waveform: None,
        }
    }

    /// Which look to draw. See the module docs.
    pub(crate) fn style(mut self, style: SeekbarStyle) -> Self {
        self.style = style;
        self
    }

    /// The end-cap labels: drawn inside a Capsule (color-aware against the
    /// fill and track) and beside the track for the other styles. See
    /// [`CapLabel`].
    pub(crate) fn time_labels(mut self, left: CapLabel, right: CapLabel) -> Self {
        self.time_labels = Some((left, right));
        self
    }

    /// The track's envelope for the Waveform style. `None` draws the Line
    /// look until it arrives.
    pub(crate) fn waveform(mut self, waveform: Option<Arc<Waveform>>) -> Self {
        self.waveform = waveform;
        self
    }

    #[expect(
        clippy::wrong_self_convention,
        reason = "a builder setter named for its flag, not an is_ accessor"
    )]
    pub(crate) fn is_playing(mut self, is_playing: bool) -> Self {
        self.is_playing = is_playing;
        self
    }

    pub(crate) fn hide_handle(mut self, hide: bool) -> Self {
        self.hide_handle = hide;
        self
    }

    /// Whether click/drag seeking (and the hover preview) is enabled. Defaults
    /// to `true`. Set `false` for non-seekable streams (radio). Decoupled from
    /// [`Self::hide_handle`] so a handle-less bar can still be clickable.
    pub(crate) fn interactive(mut self, interactive: bool) -> Self {
        self.interactive = interactive;
        self
    }

    pub(crate) fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub(crate) fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    /// The look actually drawn: a Waveform without its envelope draws Line.
    fn drawn_style(&self) -> SeekbarStyle {
        match self.style {
            SeekbarStyle::Waveform if self.waveform.is_none() => SeekbarStyle::Line,
            style => style,
        }
    }

    /// Whether the end caps sit beside the track (every style but Capsule).
    fn has_outside_caps(&self) -> bool {
        self.time_labels.is_some() && !self.style.times_inside()
    }

    /// The seekable track inside the widget bounds: the whole bounds, or the
    /// span between the end caps when they sit beside it.
    fn track(&self, caps: &CapWidths, bounds: Rectangle) -> Rectangle {
        if !self.has_outside_caps() {
            return bounds;
        }
        let left = bounds.x + CAP_PAD + caps.left + CAP_GAP;
        let right = bounds.x + bounds.width - CAP_PAD - caps.right - CAP_GAP;
        Rectangle {
            x: left,
            width: (right - left).max(0.0),
            ..bounds
        }
    }

    /// The `0..=1` position under `cursor_x` on `track`. Classic reserves
    /// `HANDLE_SIZE` so the handle center tracks the cursor; the handle-less
    /// styles map the full width.
    fn fraction_at(&self, cursor_x: f32, track: Rectangle) -> f32 {
        let (effective_width, offset) = if self.style == SeekbarStyle::Classic {
            (track.width - HANDLE_SIZE, HANDLE_SIZE / 2.0)
        } else {
            (track.width, 0.0)
        };
        if effective_width <= 0.0 {
            return 0.0;
        }
        ((cursor_x - track.x - offset) / effective_width).clamp(0.0, 1.0)
    }

    /// Calculate the seek position (seconds) under `cursor_x` on `track`.
    fn locate(&self, cursor_x: f32, track: Rectangle) -> f32 {
        self.fraction_at(cursor_x, track) * self.duration
    }

    /// Measure the end caps beside the track, re-shaping only when their
    /// templates change. Both caps carry numbers that move during playback:
    /// the left one its elapsed time (measured against a widest-digits
    /// template), the right one the live bitrate (every digit run padded to
    /// [`MIN_TEMPLATE_DIGITS`] zeros). So the widths, and the track's edges,
    /// hold while those numbers change.
    fn measure_caps(&self, caps: &mut CapWidths) {
        let Some((left, right)) = &self.time_labels else {
            return;
        };
        let template = widest_time_template(&left.time, &right.time);
        let left_key = match left.full.strip_prefix(left.time.as_str()) {
            Some(meta) => format!("{template}{meta}"),
            None => left.full.clone(),
        };
        let right_key = match right.full.strip_suffix(right.time.as_str()) {
            Some(meta) => format!(
                "{}{}",
                padded_digit_template(meta),
                widest_time_template(&right.time, "")
            ),
            None => padded_digit_template(&right.full),
        };
        let key = (left_key, right_key);
        let font = crate::theme::ui_font();
        if caps.key == key && caps.font == Some(font) {
            return;
        }
        caps.left = cap_text_width(&key.0);
        caps.right = cap_text_width(&key.1);
        caps.key = key;
        caps.font = Some(font);
    }
}

/// A time string for the widest elapsed time the left cap can show: the
/// longer of `elapsed` and `duration`, every digit swapped for `0` (the
/// widest digit in most UI fonts).
fn widest_time_template(elapsed: &str, duration: &str) -> String {
    let longer = if elapsed.chars().count() >= duration.chars().count() {
        elapsed
    } else {
        duration
    };
    longer
        .chars()
        .map(|c| if c.is_ascii_digit() { '0' } else { c })
        .collect()
}

/// Digits a number in the right cap's template reserves at least: a live
/// bitrate under 10 000 kbps never changes the cap's width.
const MIN_TEMPLATE_DIGITS: usize = 4;

/// `text` with every run of digits swapped for at least
/// [`MIN_TEMPLATE_DIGITS`] zeros (more when the run is longer).
fn padded_digit_template(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + MIN_TEMPLATE_DIGITS);
    let mut run = 0;
    let flush = |out: &mut String, run: &mut usize| {
        if *run > 0 {
            out.extend(std::iter::repeat_n('0', (*run).max(MIN_TEMPLATE_DIGITS)));
            *run = 0;
        }
    };
    for c in text.chars() {
        if c.is_ascii_digit() {
            run += 1;
        } else {
            flush(&mut out, &mut run);
            out.push(c);
        }
    }
    flush(&mut out, &mut run);
    out
}

/// The end-cap text spec, shared by measuring and drawing so both shape the
/// same glyphs.
fn cap_text<C>(content: C, bounds: Size, align_x: text::Alignment) -> Text<C> {
    Text {
        content,
        bounds,
        size: Pixels(CAP_TEXT_SIZE),
        line_height: crate::theme::UI_LINE_HEIGHT,
        font: crate::theme::ui_font(),
        align_x,
        align_y: alignment::Vertical::Center,
        shaping: Shaping::Basic,
        wrapping: text::Wrapping::None,
        ellipsis: text::Ellipsis::default(),
        hint_factor: Some(1.0),
    }
}

/// Laid-out width of an end-cap string.
fn cap_text_width(content: &str) -> f32 {
    let paragraph = <iced::Renderer as TextRenderer>::Paragraph::with_text(cap_text(
        content,
        Size::INFINITE,
        text::Alignment::Left,
    ));
    paragraph.min_bounds().width.ceil()
}

/// What a cell shows, from its index, the playhead's cell and the hovered
/// cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CellRole {
    Playhead,
    Played,
    /// Played, between the hovered cell and the playhead.
    PlayedHovered,
    Unplayed,
    /// Unplayed, between the playhead and the hovered cell.
    UnplayedHovered,
}

fn cell_role(index: usize, playhead: usize, hovered: Option<usize>) -> CellRole {
    use std::cmp::Ordering::{Equal, Greater, Less};
    match index.cmp(&playhead) {
        Equal => CellRole::Playhead,
        Less if hovered.is_some_and(|h| h <= index) => CellRole::PlayedHovered,
        Less => CellRole::Played,
        Greater if hovered.is_some_and(|h| h >= index) => CellRole::UnplayedHovered,
        Greater => CellRole::Unplayed,
    }
}

/// Whether a waveform bar centered at `x` (a `0..=1` fraction) falls between
/// the playhead and the hovered point.
fn in_hover_range(x: f32, progress: f32, hover: Option<f32>) -> bool {
    hover.is_some_and(|h| (x > progress && x <= h) || (x < progress && x >= h))
}

/// How many cells fit `width`: [`CELL_COUNT`], or fewer when a cell would be
/// thinner than [`MIN_CELL_WIDTH`].
fn cell_count(width: f32) -> usize {
    let fit = ((width + CELL_GAP) / (MIN_CELL_WIDTH + CELL_GAP)).floor();
    (fit.max(1.0) as usize).min(CELL_COUNT)
}

/// How many waveform bars fit `width` at [`WAVE_PITCH`].
fn wave_bar_count(width: f32) -> usize {
    ((width + WAVE_GAP) / WAVE_PITCH).floor().clamp(1.0, 2048.0) as usize
}

/// Corner radius for cells and waveform bars: square in flat mode, a small
/// [`CELL_RADIUS_ROUNDED`] in rounded mode.
fn cell_radius() -> iced::border::Radius {
    if crate::theme::is_rounded_for_player() {
        CELL_RADIUS_ROUNDED.into()
    } else {
        0.0.into()
    }
}

fn with_alpha(color: Color, alpha: f32) -> Color {
    Color {
        a: color.a * alpha,
        ..color
    }
}

impl<Message> iced::advanced::widget::Meta for ProgressBar<'_, Message> {}

impl<Message: Clone> Widget<Message, Theme, iced::Renderer> for ProgressBar<'_, Message> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: Length::Fixed(self.height),
        }
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) {
        if self.has_outside_caps() {
            let state = tree.state.downcast_mut::<State>();
            self.measure_caps(&mut state.caps);
        }
        tree.size = layout::atomic(limits, self.width, self.height);
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();
        let track = self.track(&state.caps, bounds);

        // Track position changes for interpolation
        // When position changes (from playback update), record it
        if (self.position - state.last_position).abs() > 0.01 {
            state.last_position = self.position;
            state.last_update = Some(Instant::now());
        }

        // If playing, request continuous redraws for smooth interpolation
        if self.is_playing && !state.is_dragging {
            shell.request_redraw();
        }

        // A newly arrived waveform fades in; keep redrawing until it has.
        if self.drawn_style() == SeekbarStyle::Waveform
            && let Some(waveform) = &self.waveform
        {
            match &state.waveform_since {
                Some((seen, since)) if Weak::ptr_eq(seen, &Arc::downgrade(waveform)) => {
                    if since.elapsed().as_secs_f32() < WAVE_FADE_SECS {
                        shell.request_redraw();
                    }
                }
                _ => {
                    state.waveform_since = Some((Arc::downgrade(waveform), Instant::now()));
                    shell.request_redraw();
                }
            }
        }

        // Non-seekable streams (radio) disable all interaction. Note this is
        // decoupled from `hide_handle`: a handle-less bar stays seekable.
        if !self.interactive {
            // A drag in progress when the bar turned non-seekable (the queue
            // moved on to a live transcode mid-drag) ends here, unpublished:
            // otherwise the release goes unseen and the next release anywhere
            // seeks the next song to a stale point.
            let had_hover = state.hover.take().is_some();
            let was_dragging = std::mem::take(&mut state.is_dragging);
            if had_hover || was_dragging {
                shell.request_redraw();
            }
            return;
        }

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                if let Some(cursor_position) = cursor.position_over(track) {
                    let current_progress = (self.position / self.duration).clamp(0.0, 1.0);

                    // Classic: did the press land on the handle itself? The
                    // handle-less styles always take a track press.
                    let clicked_on_handle = self.style == SeekbarStyle::Classic && {
                        let handle_x = track.x + current_progress * (track.width - HANDLE_SIZE);
                        cursor_position.x >= handle_x && cursor_position.x <= handle_x + HANDLE_SIZE
                    };

                    if clicked_on_handle {
                        // Start dragging from current position
                        state.is_dragging = true;
                        state.drag_progress = current_progress;
                        shell.capture_event();
                        shell.request_redraw();
                    } else {
                        // Clicked on track - immediately seek to clicked position
                        let seek_pos = self.locate(cursor_position.x, track);
                        state.drag_progress = (seek_pos / self.duration).clamp(0.0, 1.0);
                        // The handle-less styles have nothing to grab, so a
                        // track press also begins a drag — click-and-drag
                        // scrub from anywhere along the bar.
                        if self.style != SeekbarStyle::Classic {
                            state.is_dragging = true;
                        }
                        shell.publish((self.on_seek)(seek_pos));
                        shell.capture_event();
                        shell.request_redraw();
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerLifted { .. } | touch::Event::FingerLost { .. })
                if state.is_dragging =>
            {
                // C++ pattern: ONLY seek to final position when user releases after drag
                // This prevents seek spam during dragging
                let seek_pos = state.drag_progress * self.duration;
                shell.publish((self.on_seek)(seek_pos));
                state.is_dragging = false;
            }
            Event::Mouse(mouse::Event::CursorMoved { .. })
            | Event::Touch(touch::Event::FingerMoved { .. }) => {
                if state.is_dragging
                    && let Some(Point { x, .. }) = cursor.position()
                {
                    // C++ pattern: Update visual position ONLY, don't seek during drag
                    // This ensures smooth handle movement without audio interruption
                    state.drag_progress = self.fraction_at(x, track);
                    shell.capture_event();
                    // Request redraw to show updated handle position
                    shell.request_redraw();
                }
                // The handle-less styles preview the hovered time. Redraw only
                // when the hovered point changes.
                if self.style != SeekbarStyle::Classic {
                    let hover = cursor
                        .position_over(track)
                        .map(|p| self.fraction_at(p.x, track));
                    if hover != state.hover {
                        state.hover = hover;
                        shell.request_redraw();
                    }
                }
            }
            Event::Mouse(mouse::Event::CursorLeft) if state.hover.take().is_some() => {
                shell.request_redraw();
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let track = self.track(&state.caps, bounds);

        // Playhead position with smooth interpolation during playback
        let progress = if state.is_dragging {
            // During drag, use the visual drag position (not the actual playback position)
            state.drag_progress
        } else if self.is_playing
            && let Some(last_update) = state.last_update
            && self.duration > 0.0
        {
            // Interpolate position based on elapsed time since last update
            let elapsed = last_update.elapsed().as_secs_f32();
            let interpolated_pos = (state.last_position + elapsed).min(self.duration);
            (interpolated_pos / self.duration).clamp(0.0, 1.0)
        } else if self.duration > 0.0 {
            // Not playing or no timing info - use actual position
            (self.position / self.duration).clamp(0.0, 1.0)
        } else {
            0.0
        };
        // While dragging, the fill already follows the cursor; the preview is
        // for hovering only.
        let hover = if state.is_dragging || !self.interactive {
            None
        } else {
            state.hover
        };

        match self.drawn_style() {
            SeekbarStyle::Classic => self.draw_classic(renderer, track, progress),
            SeekbarStyle::Line => {
                let active = state.is_dragging || hover.is_some();
                self.draw_line(renderer, track, progress, active, 1.0);
            }
            SeekbarStyle::Capsule => self.draw_capsule(renderer, track, progress, hover),
            SeekbarStyle::Cells => draw_cells(renderer, track, progress, hover),
            SeekbarStyle::Waveform => {
                let fade = state.waveform_since.as_ref().map_or(1.0, |(_, since)| {
                    (since.elapsed().as_secs_f32() / WAVE_FADE_SECS).min(1.0)
                });
                if fade < 1.0 {
                    self.draw_line(renderer, track, progress, false, 1.0 - fade);
                }
                if let Some(waveform) = &self.waveform {
                    draw_waveform(renderer, track, progress, hover, waveform, fade);
                }
            }
        }

        if self.has_outside_caps()
            && let Some((left, right)) = &self.time_labels
        {
            draw_outside_caps(renderer, bounds, left, right);
        }

        // Tooltip is drawn via overlay() for proper z-ordering
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if !self.interactive {
            return mouse::Interaction::default();
        }

        let state = tree.state.downcast_ref::<State>();

        if state.is_dragging {
            mouse::Interaction::Grabbing
        } else if cursor.is_over(self.track(&state.caps, layout.bounds())) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut widget::Tree,
        layout: Layout,
        _renderer: &iced::Renderer,
        _viewport: &Rectangle,
        translation: Vector,
        _window: Size,
    ) -> Vec<iced::advanced::overlay::Element<'b, Message, Theme, iced::Renderer>> {
        let state = tree.state.downcast_ref::<State>();
        // The dragged time while dragging; the hovered time for the
        // handle-less styles.
        let shown = if state.is_dragging {
            Some(state.drag_progress)
        } else if self.interactive && self.style != SeekbarStyle::Classic {
            state.hover
        } else {
            None
        };
        let Some(fraction) = shown else {
            return Vec::new();
        };

        let bounds = layout.bounds();
        let track = self.track(&state.caps, bounds);
        let center_x = if self.style == SeekbarStyle::Classic {
            track.x + fraction * (track.width - HANDLE_SIZE) + HANDLE_SIZE / 2.0
        } else {
            track.x + fraction * track.width
        };

        vec![iced::advanced::overlay::Element::new(Box::new(
            TooltipOverlay::new(
                center_x + translation.x,
                bounds.y + translation.y,
                fraction,
                self.duration,
            ),
        ))]
    }
}

impl<Message> ProgressBar<'_, Message> {
    /// Classic: the 6 px track, its fill, and the 14 px handle.
    fn draw_classic(&self, renderer: &mut iced::Renderer, track: Rectangle, progress: f32) {
        use iced::advanced::Renderer;

        let effective_width = track.width - HANDLE_SIZE;
        let handle_x = track.x + progress * effective_width;
        let track_y = track.y + (track.height - TRACK_THICKNESS) / 2.0;
        // `ui_radius_pill_player()` returns `0.0.into()` in flat mode and the
        // pill radius in rounded mode — no separate ladder needed.
        let radius = crate::theme::ui_radius_pill_player();

        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x: track.x,
                    y: track_y,
                    width: track.width,
                    height: TRACK_THICKNESS,
                },
                border: iced::Border {
                    radius,
                    ..Default::default()
                },
                ..Default::default()
            },
            crate::theme::bg2(),
        );

        // The fill runs to the handle center.
        let fill_width = (handle_x - track.x + HANDLE_SIZE / 2.0).clamp(0.0, track.width);
        if fill_width > 0.0 {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: track.x,
                        y: track_y,
                        width: fill_width,
                        height: TRACK_THICKNESS,
                    },
                    border: iced::Border {
                        radius,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                crate::theme::accent_fill(),
            );
        }

        if !self.hide_handle {
            draw_handle(
                renderer,
                track,
                Rectangle {
                    x: handle_x,
                    y: track.y + (track.height - HANDLE_SIZE) / 2.0,
                    width: HANDLE_SIZE,
                    height: HANDLE_SIZE,
                },
            );
        }
    }

    /// Line: the thin track and its fill, thickened with the handle shown at
    /// the playhead while `active` (hovered or dragging). `alpha` fades the
    /// whole look out under an arriving waveform.
    fn draw_line(
        &self,
        renderer: &mut iced::Renderer,
        track: Rectangle,
        progress: f32,
        active: bool,
        alpha: f32,
    ) {
        use iced::advanced::Renderer;

        let thickness = if active {
            LINE_THICKNESS_ACTIVE
        } else {
            LINE_THICKNESS
        };
        let y = track.y + (track.height - thickness) / 2.0;
        let radius = crate::theme::ui_radius_pill_player();
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x: track.x,
                    y,
                    width: track.width,
                    height: thickness,
                },
                border: iced::Border {
                    radius,
                    ..Default::default()
                },
                ..Default::default()
            },
            with_alpha(crate::theme::bg2(), alpha),
        );
        let fill_width = (progress * track.width).clamp(0.0, track.width);
        if fill_width > 0.0 {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: track.x,
                        y,
                        width: fill_width,
                        height: thickness,
                    },
                    border: iced::Border {
                        radius,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                with_alpha(crate::theme::accent_fill(), alpha),
            );
        }
        if active && !self.hide_handle && self.interactive && track.width > LINE_HANDLE_SIZE {
            let x = (track.x + fill_width - LINE_HANDLE_SIZE / 2.0)
                .clamp(track.x, track.x + track.width - LINE_HANDLE_SIZE);
            draw_handle(
                renderer,
                track,
                Rectangle {
                    x,
                    y: track.y + (track.height - LINE_HANDLE_SIZE) / 2.0,
                    width: LINE_HANDLE_SIZE,
                    height: LINE_HANDLE_SIZE,
                },
            );
        }
    }

    /// Capsule: the full-height block, its fill, the hover marker, and the
    /// times inside.
    fn draw_capsule(
        &self,
        renderer: &mut iced::Renderer,
        track: Rectangle,
        progress: f32,
        hover: Option<f32>,
    ) {
        use iced::advanced::Renderer;

        // Always square, so the block reads as one connected element even in
        // rounded mode. The darker `bg0` track sets it apart from the bar.
        renderer.fill_quad(
            renderer::Quad {
                bounds: track,
                ..Default::default()
            },
            crate::theme::bg0(),
        );
        let fill_width = (progress * track.width).clamp(0.0, track.width);
        if fill_width > 0.0 {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        width: fill_width,
                        ..track
                    },
                    ..Default::default()
                },
                crate::theme::accent_fill(),
            );
        }

        // Hover marker: ink that contrasts with whichever side it lands on.
        // Skipped on a block too narrow to hold it (the clamp below needs it).
        if let Some(h) = hover
            && track.width >= HOVER_MARKER_WIDTH
        {
            let x = track.x + h * track.width;
            let ink = if x < track.x + fill_width {
                crate::theme::on_accent_fill()
            } else {
                crate::theme::fg0()
            };
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: (x - HOVER_MARKER_WIDTH / 2.0)
                            .clamp(track.x, track.x + track.width - HOVER_MARKER_WIDTH),
                        width: HOVER_MARKER_WIDTH,
                        ..track
                    },
                    ..Default::default()
                },
                ink,
            );
        }

        // Color-aware overlaid end-cap labels. Each cap is painted twice —
        // clipped to the fill region (dark text over the bright fill) and the
        // track region (light text over the dark track) — so it stays legible
        // and flips color exactly at the fill edge. The codec / bitrate is
        // underpainted at a dimmer opacity (the full string), then the bare
        // time is repainted opaque over the same outer-edge anchor so only the
        // time pops.
        let Some((left, right)) = &self.time_labels else {
            return;
        };
        let on_fill = crate::theme::on_accent_fill();
        let on_track = crate::theme::fg1();
        let fill_clip = Rectangle {
            width: fill_width,
            ..track
        };
        let track_clip = Rectangle {
            x: track.x + fill_width,
            width: (track.width - fill_width).max(0.0),
            ..track
        };
        let cy = track.center_y();
        let left_pos = Point::new(track.x + CAP_PAD, cy);
        let right_pos = Point::new(track.x + track.width - CAP_PAD, cy);

        let mut paint =
            |content: &str, align_x: text::Alignment, pos: Point, fill_c: Color, track_c: Color| {
                for (clip, color) in [(fill_clip, fill_c), (track_clip, track_c)] {
                    renderer.fill_text(
                        cap_text(content.to_owned(), track.size(), align_x),
                        pos,
                        color,
                        clip,
                    );
                }
            };
        for (cap, align_x, pos) in [
            (left, text::Alignment::Left, left_pos),
            (right, text::Alignment::Right, right_pos),
        ] {
            if cap.has_meta() {
                paint(
                    &cap.full,
                    align_x,
                    pos,
                    with_alpha(on_fill, META_ALPHA),
                    with_alpha(on_track, META_ALPHA),
                );
            }
            paint(&cap.time, align_x, pos, on_fill, on_track);
        }
    }
}

/// The square (flat) / pill (rounded) handle with its 1 px border, on its own
/// layer so it draws above neighboring quads.
fn draw_handle(renderer: &mut iced::Renderer, clip: Rectangle, handle: Rectangle) {
    use iced::advanced::Renderer;

    renderer.with_layer(clip, |renderer| {
        renderer.fill_quad(
            renderer::Quad {
                bounds: handle,
                border: iced::Border {
                    color: crate::theme::bg0_hard(),
                    width: 1.0,
                    radius: crate::theme::ui_radius_pill_player(),
                },
                ..Default::default()
            },
            crate::theme::accent_fill(),
        );
    });
}

/// Cells: played cells in `accent_fill()`, the playhead's cell lifted toward
/// the foreground, the hovered range washed.
fn draw_cells(renderer: &mut iced::Renderer, track: Rectangle, progress: f32, hover: Option<f32>) {
    use iced::advanced::Renderer;

    let n = cell_count(track.width);
    let cell_width = (track.width - CELL_GAP * (n - 1) as f32) / n as f32;
    if cell_width <= 0.0 {
        return;
    }
    let height = CELL_HEIGHT.min(track.height - 4.0).max(2.0);
    let y = track.y + (track.height - height) / 2.0;
    let index_at = |fraction: f32| ((fraction * n as f32) as usize).min(n - 1);
    let playhead = index_at(progress);
    let hovered = hover.map(index_at);

    let played = crate::theme::accent_fill();
    let colors = |role: CellRole| match role {
        CellRole::Playhead => crate::theme::toward_foreground(played, PLAYHEAD_LIFT),
        CellRole::Played => played,
        CellRole::PlayedHovered => crate::theme::toward_foreground(played, HOVER_LIFT),
        CellRole::Unplayed => crate::theme::bg2(),
        CellRole::UnplayedHovered => {
            crate::theme::toward_foreground(crate::theme::bg2(), UNPLAYED_HOVER_LIFT)
        }
    };
    let radius = cell_radius();
    for i in 0..n {
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x: track.x + i as f32 * (cell_width + CELL_GAP),
                    y,
                    width: cell_width,
                    height,
                },
                border: iced::Border {
                    radius,
                    ..Default::default()
                },
                ..Default::default()
            },
            colors(cell_role(i, playhead, hovered)),
        );
    }
}

/// Waveform: one bar per [`WAVE_PITCH`], played bars in `accent_fill()` (the
/// bar under the playhead split at it), the hovered range lifted, all at
/// `alpha` while fading in.
fn draw_waveform(
    renderer: &mut iced::Renderer,
    track: Rectangle,
    progress: f32,
    hover: Option<f32>,
    waveform: &Waveform,
    alpha: f32,
) {
    use iced::advanced::Renderer;

    let n = wave_bar_count(track.width);
    let bar_width = (track.width - WAVE_GAP * (n - 1) as f32) / n as f32;
    if bar_width <= 0.0 {
        return;
    }
    let available = (track.height - 2.0 * WAVE_INSET).max(2.0);
    let center_y = track.center_y();
    let fill_x = track.x + progress * track.width;
    let radius = cell_radius();

    let played = crate::theme::accent_fill();
    let played_hovered = crate::theme::toward_foreground(played, HOVER_LIFT);
    let unplayed = crate::theme::fg4();
    let unplayed_hovered = crate::theme::fg2();

    let mut quad = |x: f32, width: f32, height: f32, color: Color| {
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x,
                    y: center_y - height / 2.0,
                    width,
                    height,
                },
                border: iced::Border {
                    radius,
                    ..Default::default()
                },
                ..Default::default()
            },
            with_alpha(color, alpha),
        );
    };
    for (i, column) in waveform.columns(n).into_iter().enumerate() {
        let x = track.x + i as f32 * (bar_width + WAVE_GAP);
        let height = (column * available).max(2.0);
        let center = (i as f32 + 0.5) / n as f32;
        let hovered = in_hover_range(center, progress, hover);
        let (played_c, unplayed_c) = if hovered {
            (played_hovered, unplayed_hovered)
        } else {
            (played, unplayed)
        };
        if fill_x >= x + bar_width {
            quad(x, bar_width, height, played_c);
        } else if fill_x <= x {
            quad(x, bar_width, height, unplayed_c);
        } else {
            // The playhead's bar: played to the fill edge, unplayed after.
            quad(x, fill_x - x, height, played_c);
            quad(fill_x, x + bar_width - fill_x, height, unplayed_c);
        }
    }
}

/// The times beside the track: the elapsed time at the left edge with its
/// codec trailing, the bitrate leading the duration at the right edge. The
/// codec / bitrate underpaint dimmer, then the bare time repaints at full ink.
fn draw_outside_caps(
    renderer: &mut iced::Renderer,
    bounds: Rectangle,
    left: &CapLabel,
    right: &CapLabel,
) {
    let time_ink = crate::theme::legible_strip_text(crate::theme::fg1(), crate::theme::bg0_hard());
    let meta_ink = crate::theme::legible_strip_text(crate::theme::fg3(), crate::theme::bg0_hard());
    let cy = bounds.center_y();
    for (cap, align_x, x) in [
        (left, text::Alignment::Left, bounds.x + CAP_PAD),
        (
            right,
            text::Alignment::Right,
            bounds.x + bounds.width - CAP_PAD,
        ),
    ] {
        let pos = Point::new(x, cy);
        if cap.has_meta() {
            renderer.fill_text(
                cap_text(cap.full.clone(), bounds.size(), align_x),
                pos,
                meta_ink,
                bounds,
            );
        }
        renderer.fill_text(
            cap_text(cap.time.clone(), bounds.size(), align_x),
            pos,
            time_ink,
            bounds,
        );
    }
}

/// Overlay for the seek time tooltip — renders on top of all other widgets.
///
/// Flat design: 1 px `theme::border()` outline on `theme::bg0_hard()` background;
/// no 3D bevel. A small downward arrow points at the seek point.
struct TooltipOverlay {
    /// The tooltip body plus the arrow under it, in window coordinates.
    bounds: Rectangle,
    drag_progress: f32,
    duration: f32,
}

// Tooltip dimensions
const TOOLTIP_HEIGHT: f32 = 20.0;
const TOOLTIP_WIDTH: f32 = 44.0;
const TOOLTIP_ARROW_SIZE: f32 = 6.0;
const TOOLTIP_GAP: f32 = 2.0;

impl TooltipOverlay {
    /// A tooltip centered on `center_x` (window coordinates), floating just
    /// above the bar's top edge `bounds_y`.
    fn new(center_x: f32, bounds_y: f32, drag_progress: f32, duration: f32) -> Self {
        let tooltip_x = center_x - TOOLTIP_WIDTH / 2.0;
        let tooltip_y = bounds_y - TOOLTIP_HEIGHT - TOOLTIP_ARROW_SIZE - TOOLTIP_GAP;

        Self {
            bounds: Rectangle::new(
                Point::new(tooltip_x, tooltip_y),
                Size::new(TOOLTIP_WIDTH, TOOLTIP_HEIGHT + TOOLTIP_ARROW_SIZE),
            ),
            drag_progress,
            duration,
        }
    }
}

impl<Message> iced::advanced::overlay::Overlay<Message, Theme, iced::Renderer> for TooltipOverlay {
    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        _cursor: mouse::Cursor,
    ) {
        use iced::advanced::Renderer;

        let bounds = self.bounds;
        let tooltip_height = TOOLTIP_HEIGHT;
        let tooltip_width = TOOLTIP_WIDTH;
        let tooltip_arrow_size = TOOLTIP_ARROW_SIZE;

        let tooltip_x = bounds.x;
        let tooltip_y = bounds.y;

        // Calculate the time being seeked to
        let seek_time = self.drag_progress * self.duration;
        let seek_minutes = (seek_time / 60.0) as u32;
        let seek_seconds = (seek_time % 60.0) as u32;
        let time_text = format!("{seek_minutes}:{seek_seconds:02}");

        use crate::theme;
        let tooltip_bg = theme::bg0_hard();
        let tooltip_border = theme::border();
        let tooltip_text_color = theme::fg1();
        // `ui_radius_sm()` returns `0.0.into()` in flat mode.
        let radius = crate::theme::ui_radius_sm_player();

        // Arrow pointing down toward the seek point. Drawn as a small filled
        // square tucked below the tooltip body — keeps the visual link without
        // needing a triangle primitive.
        let arrow_x = tooltip_x + tooltip_width / 2.0 - tooltip_arrow_size / 2.0;
        let arrow_y = tooltip_y + tooltip_height;

        // The tooltip draws in its own layer, above the widgets under it.
        renderer.with_layer(bounds, |renderer| {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: arrow_x,
                        y: arrow_y,
                        width: tooltip_arrow_size,
                        height: tooltip_arrow_size / 2.0,
                    },
                    ..Default::default()
                },
                tooltip_bg,
            );

            // Tooltip body — flat fill with 1 px border.
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: tooltip_x,
                        y: tooltip_y,
                        width: tooltip_width,
                        height: tooltip_height,
                    },
                    border: iced::Border {
                        color: tooltip_border,
                        width: 1.0,
                        radius,
                    },
                    shadow: Shadow::default(),
                    ..Default::default()
                },
                tooltip_bg,
            );

            // Draw the time text centered in tooltip
            let tooltip_bounds = Rectangle {
                x: tooltip_x,
                y: tooltip_y,
                width: tooltip_width,
                height: tooltip_height,
            };

            renderer.fill_text(
                Text {
                    content: time_text,
                    bounds: tooltip_bounds.size(),
                    size: Pixels(12.0),
                    line_height: crate::theme::UI_LINE_HEIGHT,
                    font: crate::theme::ui_font(),
                    align_x: text::Alignment::Center,
                    align_y: alignment::Vertical::Center,
                    shaping: Shaping::Basic,
                    wrapping: text::Wrapping::default(),
                    ellipsis: text::Ellipsis::default(),
                    hint_factor: Some(1.0),
                },
                tooltip_bounds.center(),
                tooltip_text_color,
                Rectangle::with_size(Size::INFINITE),
            );
        });
    }
}

/// Helper function to create a progress bar
pub(crate) fn progress_bar<'a, Message: Clone + 'a>(
    position: f32,
    duration: f32,
    on_seek: impl Fn(f32) -> Message + 'a,
) -> ProgressBar<'a, Message> {
    ProgressBar::new(position, duration, on_seek)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(duration: f32, style: SeekbarStyle) -> ProgressBar<'static, ()> {
        ProgressBar::new(0.0, duration, |_| ()).style(style)
    }

    fn bounds(width: f32) -> Rectangle {
        Rectangle {
            x: 0.0,
            y: 0.0,
            width,
            height: 20.0,
        }
    }

    /// The handle-less styles seek across the FULL width — the edges map to
    /// the track ends with no handle inset, so click/drag-to-seek lands
    /// accurately.
    #[test]
    fn handleless_locate_maps_full_width_edges_to_ends() {
        for style in [
            SeekbarStyle::Line,
            SeekbarStyle::Capsule,
            SeekbarStyle::Cells,
            SeekbarStyle::Waveform,
        ] {
            let b = bar(100.0, style);
            assert_eq!(b.locate(0.0, bounds(100.0)), 0.0, "{style:?}");
            assert_eq!(b.locate(50.0, bounds(100.0)), 50.0, "{style:?}");
            assert_eq!(b.locate(100.0, bounds(100.0)), 100.0, "{style:?}");
        }
    }

    /// Classic reserves `HANDLE_SIZE`; the cursor at the handle's leftmost
    /// center (x = HANDLE_SIZE/2) maps to the start. Guards against a future
    /// refactor leaking the handle offset into the handle-less styles.
    #[test]
    fn classic_locate_insets_by_half_handle() {
        let b = bar(100.0, SeekbarStyle::Classic);
        assert_eq!(b.locate(HANDLE_SIZE / 2.0, bounds(100.0)), 0.0);
    }

    /// With the times beside the track, seeking maps the span between them,
    /// not the whole widget.
    #[test]
    fn outside_caps_narrow_the_seekable_track() {
        let b = bar(100.0, SeekbarStyle::Line)
            .time_labels(CapLabel::time_only("0:00"), CapLabel::time_only("1:40"));
        let caps = CapWidths {
            key: Default::default(),
            font: None,
            left: 30.0,
            right: 30.0,
        };
        let track = b.track(&caps, bounds(200.0));
        assert_eq!(track.x, CAP_PAD + 30.0 + CAP_GAP);
        assert_eq!(track.width, 200.0 - 2.0 * (CAP_PAD + 30.0 + CAP_GAP));
        assert_eq!(b.locate(track.x, track), 0.0);
        assert_eq!(b.locate(track.x + track.width, track), 100.0);
        // A Capsule draws its times inside, so its track is the whole bar.
        let capsule = bar(100.0, SeekbarStyle::Capsule)
            .time_labels(CapLabel::time_only("0:00"), CapLabel::time_only("1:40"));
        assert_eq!(capsule.track(&caps, bounds(200.0)), bounds(200.0));
    }

    #[test]
    fn waveform_without_an_envelope_draws_line() {
        assert_eq!(
            bar(100.0, SeekbarStyle::Waveform).drawn_style(),
            SeekbarStyle::Line
        );
        let wf = Arc::new(Waveform::new(vec![1], vec![1]).expect("waveform"));
        assert_eq!(
            bar(100.0, SeekbarStyle::Waveform)
                .waveform(Some(wf))
                .drawn_style(),
            SeekbarStyle::Waveform
        );
    }

    /// The left cap is measured against the widest time it can show, so its
    /// width (and the track's left edge) holds while the elapsed time ticks.
    #[test]
    fn widest_time_template_covers_the_duration() {
        assert_eq!(widest_time_template("0:44", "15:39"), "00:00");
        assert_eq!(widest_time_template("9:59", "4:12"), "0:00");
        // Radio: the elapsed time outgrows the placeholder duration.
        assert_eq!(widest_time_template("125:03", "--:--"), "000:00");
    }

    /// The right cap's live bitrate moves every second; its template holds
    /// the width for any bitrate under 10 000 kbps.
    #[test]
    fn padded_digit_template_holds_the_bitrate_width() {
        assert_eq!(padded_digit_template("957kbps · "), "0000kbps · ");
        assert_eq!(padded_digit_template("1411kbps · "), "0000kbps · ");
        assert_eq!(padded_digit_template("12345kbps · "), "00000kbps · ");
        assert_eq!(padded_digit_template("no digits"), "no digits");
    }

    #[test]
    fn cell_roles_follow_the_playhead_and_hover() {
        use CellRole::*;
        let roles =
            |hovered| -> Vec<CellRole> { (0..6).map(|i| cell_role(i, 2, hovered)).collect() };
        assert_eq!(
            roles(None),
            [Played, Played, Playhead, Unplayed, Unplayed, Unplayed]
        );
        // Hovering ahead washes the unplayed cells up to the cursor.
        assert_eq!(
            roles(Some(4)),
            [
                Played,
                Played,
                Playhead,
                UnplayedHovered,
                UnplayedHovered,
                Unplayed
            ]
        );
        // Hovering behind lifts the played cells back to the cursor.
        assert_eq!(
            roles(Some(1)),
            [
                Played,
                PlayedHovered,
                Playhead,
                Unplayed,
                Unplayed,
                Unplayed
            ]
        );
    }

    #[test]
    fn hover_range_spans_playhead_to_cursor_either_way() {
        assert!(in_hover_range(0.6, 0.5, Some(0.7)));
        assert!(!in_hover_range(0.8, 0.5, Some(0.7)));
        assert!(in_hover_range(0.4, 0.5, Some(0.3)));
        assert!(!in_hover_range(0.2, 0.5, Some(0.3)));
        assert!(!in_hover_range(0.6, 0.5, None));
    }

    #[test]
    fn narrow_tracks_draw_fewer_wider_cells() {
        assert_eq!(cell_count(2000.0), CELL_COUNT);
        let n = cell_count(100.0);
        assert!(n < CELL_COUNT);
        let width = (100.0 - CELL_GAP * (n - 1) as f32) / n as f32;
        assert!(width >= MIN_CELL_WIDTH);
        assert_eq!(cell_count(0.0), 1);
    }

    #[test]
    fn cap_label_has_meta_distinguishes_time_only() {
        assert!(!CapLabel::time_only("3:40").has_meta());
        assert!(CapLabel::new("3:40 · FLAC 44.1kHz", "3:40").has_meta());
    }
}
