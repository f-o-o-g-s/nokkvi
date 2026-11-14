//! Marquee text widget — scrolls overflowing text horizontally.
//!
//! If the text fits within its container, it renders normally (no animation).
//! If it overflows, the text scrolls continuously in a ring-buffer loop:
//! the text is rendered twice with a gap, so as one copy scrolls off the left
//! the next copy seamlessly appears from the right.
//!
//! It scrolls only while it is wanted: the transport runs
//! ([`set_marquee_motion`], set by the root view each render) or the cursor
//! rests on it. Otherwise it finishes the loop in flight, rests at the start
//! of the text and asks for no frames, so a paused app idles.

use std::{
    hash::{Hash, Hasher},
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use iced::{
    Color, Length, Pixels, Rectangle, Size, Theme, Vector,
    advanced::{
        Renderer, layout, renderer,
        text::{
            self as advanced_text, Paragraph as ParagraphTrait, Renderer as TextRenderer, Shaping,
            Text,
        },
        widget::{self, Widget},
    },
    alignment::Horizontal,
    font::{Font, Weight},
    mouse,
    widget::text::Wrapping,
};

// ---------------------------------------------------------------------------
// Animation constants
// ---------------------------------------------------------------------------

/// Initial pause before scrolling begins (seconds).
const INITIAL_PAUSE_SECS: f32 = 2.0;

/// Scroll speed in logical pixels per second.
const SCROLL_PX_PER_SEC: f32 = 30.0;

/// Gap between the two copies of the text (pixels).
const LOOP_GAP: f32 = 60.0;

/// Redraw pacing while scrolling: half a pixel at [`SCROLL_PX_PER_SEC`],
/// so a high-refresh display doesn't redraw the window for sub-pixel steps.
const MARQUEE_FRAME: std::time::Duration =
    std::time::Duration::from_millis((500.0 / SCROLL_PX_PER_SEC) as u64);

/// Whether overflowing marquees scroll on their own: the transport runs.
static MOTION: AtomicBool = AtomicBool::new(false);

/// Let overflowing marquees scroll on their own (`true`) or only under the
/// cursor (`false`). The root view sets it from the transport each render.
pub(crate) fn set_marquee_motion(on: bool) {
    MOTION.store(on, Ordering::Relaxed);
}

fn marquee_motion() -> bool {
    MOTION.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Per-instance animation state, stored in the widget tree.
#[derive(Debug, Clone)]
struct State {
    /// Paragraph sized to container bounds (used for layout + constrained rendering).
    constrained:
        iced::advanced::text::paragraph::Plain<<iced::Renderer as TextRenderer>::Paragraph>,
    /// Full intrinsic text width (unconstrained measurement for scroll animation).
    full_width: f32,
    /// What `full_width` was measured from (content, size, font, hint
    /// factor). Layout runs every frame while anything animates, and shaping
    /// the unconstrained line each time was a third of the UI thread; the
    /// measure reruns only when this changes.
    measure_key: u64,
    /// Scrolling now: wanted (transport or hover), or finishing the loop in
    /// flight before it rests at the start.
    animating: bool,
    /// The scroll offset `draw` uses, advanced by `update` on each event.
    offset: f32,
    /// When the current animation cycle started (or content last changed).
    cycle_start: Instant,
    /// Hash of the content string — used to detect track changes.
    content_hash: u64,
    /// Whether the previous layout pass had overflowing text. Used to reset
    /// `cycle_start` cleanly on a fits→overflows transition (e.g. user
    /// narrows the window after the lane was wide enough to fit), so the
    /// animation restarts from offset 0 with the initial pause instead of
    /// resuming mid-scroll from a stale elapsed time.
    was_overflowing: bool,
    /// When true, the initial pause is skipped so scrolling starts immediately.
    /// Set on resize-driven overflow (content unchanged) and on breakpoint-driven
    /// field drops that happen while the marquee is already scrolling — in both
    /// cases there is no new text for the user to read from the start, so the
    /// 2-second hold is unwanted.
    skip_initial_pause: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            constrained: Default::default(),
            full_width: 0.0,
            measure_key: 0,
            animating: false,
            offset: 0.0,
            cycle_start: Instant::now(),
            content_hash: 0,
            was_overflowing: false,
            skip_initial_pause: false,
        }
    }
}

/// Simple FNV-1a hash for content change detection.
fn hash_content(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Key of everything the unconstrained measure depends on.
fn measure_key(content: &str, size: Pixels, font: Font, hint_factor: Option<f32>) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    size.0.to_bits().hash(&mut hasher);
    font.hash(&mut hasher);
    hint_factor.map(f32::to_bits).hash(&mut hasher);
    hasher.finish()
}

/// Scroll offset at `now` for a line `content_width` wide, from the state's
/// cycle start and initial-pause choice.
fn scroll_offset(state: &State, content_width: f32, now: Instant) -> f32 {
    let elapsed = now
        .saturating_duration_since(state.cycle_start)
        .as_secs_f32();
    // One full loop cycle = scrolling through (content_width + gap) pixels
    let cycle_px = content_width + LOOP_GAP;
    if !state.skip_initial_pause && elapsed < INITIAL_PAUSE_SECS {
        // Hold at start so user can read the beginning of a new track title.
        // Skipped for resize-driven overflow, breakpoint-driven field drops
        // while already scrolling, and hover starts.
        return 0.0;
    }
    let scroll_elapsed = if state.skip_initial_pause {
        elapsed
    } else {
        elapsed - INITIAL_PAUSE_SECS
    };
    // Continuous modulo for seamless looping
    (scroll_elapsed * SCROLL_PX_PER_SEC) % cycle_px
}

// ---------------------------------------------------------------------------
// Widget
// ---------------------------------------------------------------------------

/// A text widget that scrolls horizontally when its content overflows.
pub(crate) struct MarqueeText {
    content: String,
    size: Pixels,
    color: Color,
    font: Font,
    align_x: Horizontal,
}

impl MarqueeText {
    pub(crate) fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            size: Pixels(9.0),
            color: Color::WHITE,
            font: Font {
                weight: Weight::Normal,
                ..crate::theme::ui_font()
            },
            align_x: Horizontal::Left,
        }
    }

    pub(crate) fn size(mut self, size: f32) -> Self {
        self.size = Pixels(size);
        self
    }

    pub(crate) fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    pub(crate) fn font(mut self, font: Font) -> Self {
        self.font = font;
        self
    }

    /// Horizontal alignment used when the text fits inside its bounds.
    /// When the text overflows, scrolling always starts from the left edge
    /// regardless of this setting.
    pub(crate) fn align_x(mut self, align_x: Horizontal) -> Self {
        self.align_x = align_x;
        self
    }
}

impl iced::advanced::widget::Meta for MarqueeText {}

impl<M: 'static> Widget<M, Theme, iced::Renderer> for MarqueeText {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Shrink,
        }
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) {
        let state = tree.state.downcast_mut::<State>();

        // Follow Iced's native text layout pattern: layout::sized() resolves
        // Length::Fill/Shrink against limits, then we build the paragraph with
        // the resolved bounds and return min_bounds().
        tree.size = layout::sized(limits, Length::Fill, Length::Shrink, |limits| {
            let bounds = limits.bounds();

            let text = Text {
                content: self.content.as_str(),
                bounds,
                size: self.size,
                line_height: crate::theme::UI_LINE_HEIGHT,
                font: self.font,
                align_x: advanced_text::Alignment::Left,
                align_y: iced::alignment::Vertical::Center,
                shaping: Shaping::Advanced,
                wrapping: Wrapping::None,
                ellipsis: advanced_text::Ellipsis::None,
                hint_factor: renderer.hint_factor(),
            };
            state.constrained.update(text);

            // Measure full unconstrained width for scroll animation, only
            // when the text or its look changed.
            let key = measure_key(&self.content, self.size, self.font, text.hint_factor);
            if key != state.measure_key {
                state.measure_key = key;
                let unconstrained_text = Text {
                    bounds: Size::new(f32::INFINITY, f32::INFINITY),
                    ..text
                };
                let full_para =
                    <iced::Renderer as TextRenderer>::Paragraph::with_text(unconstrained_text);
                state.full_width = full_para.min_bounds().width;
            }

            let now_overflowing = state.full_width > bounds.width;

            // Detect content changes (e.g. track change) → reset animation.
            // When the marquee was already overflowing (visible scrolling) and the
            // content changes due to a breakpoint-driven field drop (album/artist
            // hidden as the window narrows), the new string is shorter but the
            // user hasn't navigated to a new track — skip the initial pause so
            // the marquee continues without a 2-second freeze.
            let new_hash = hash_content(&self.content);
            let content_changed = new_hash != state.content_hash;
            if content_changed {
                state.content_hash = new_hash;
                state.cycle_start = Instant::now();
                state.skip_initial_pause = state.was_overflowing;
                state.offset = 0.0;
            }

            // Reset the cycle on a fits→overflows transition so the animation
            // restarts from offset 0.  When the transition is caused purely by a
            // resize (content unchanged), skip the initial pause so scrolling
            // begins immediately — the user is resizing the window, not looking
            // at a new track title.
            if now_overflowing && !state.was_overflowing {
                state.cycle_start = Instant::now();
                state.offset = 0.0;
                if !content_changed {
                    // Pure resize-to-overflow: no new title to read from the start.
                    state.skip_initial_pause = true;
                }
            }

            state.was_overflowing = now_overflowing;

            state.constrained.min_bounds()
        });
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
        use iced::advanced::Renderer;

        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let content_width = state.full_width;
        let container_width = bounds.width;

        // How much the text overflows (0 or positive)
        let overflow = (content_width - container_width).max(0.0);

        if overflow <= 0.0 {
            // Text fits — render normally, no animation. Honor align_x by
            // shifting the draw position within the bounds; the paragraph
            // itself stays Left-aligned so scrolling stays correct above.
            let slack = (container_width - content_width).max(0.0);
            let offset_x = match self.align_x {
                Horizontal::Left => 0.0,
                Horizontal::Center => slack / 2.0,
                Horizontal::Right => slack,
            };
            let pos = bounds.position() + Vector::new(offset_x, 0.0);
            renderer.with_layer(bounds, |renderer| {
                renderer.fill_paragraph(
                    state.constrained.raw(),
                    pos,
                    self.color,
                    Rectangle::with_size(Size::INFINITE),
                );
            });
        } else {
            // Ring-buffer loop: render text twice with a gap. The offset is
            // `update`'s (0 at rest).
            let cycle_px = content_width + LOOP_GAP;
            let offset = state.offset;

            let pos = bounds.position();

            renderer.with_layer(bounds, |renderer| {
                // First copy: scrolls left
                renderer.with_translation(Vector::new(-offset, 0.0), |renderer| {
                    renderer.fill_paragraph(
                        state.constrained.raw(),
                        pos,
                        self.color,
                        Rectangle::with_size(Size::INFINITE),
                    );
                });

                // Second copy: follows after (content_width + gap)
                renderer.with_translation(Vector::new(-offset + cycle_px, 0.0), |renderer| {
                    renderer.fill_paragraph(
                        state.constrained.raw(),
                        pos,
                        self.color,
                        Rectangle::with_size(Size::INFINITE),
                    );
                });
            });
        }
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        _event: &iced::Event,
        layout: layout::Layout,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        shell: &mut iced::advanced::Shell<'_, M>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();
        let overflowing = state.full_width > bounds.width;
        let motion = marquee_motion();
        let wanted = overflowing && (motion || cursor.is_over(bounds));
        let now = Instant::now();

        if wanted && !state.animating {
            // A start from rest: playback resuming holds at the start for the
            // reading pause; a hover starts scrolling at once.
            state.animating = true;
            state.cycle_start = now;
            state.skip_initial_pause = !motion;
            state.offset = 0.0;
        }
        if !overflowing {
            state.animating = false;
            state.offset = 0.0;
            return;
        }
        if !state.animating {
            return;
        }

        let next = scroll_offset(state, state.full_width, now);
        // Unwanted: run the loop in flight out to the start (the offset
        // wraps), then rest there without asking for frames.
        if !wanted && (next == 0.0 || next < state.offset) {
            state.animating = false;
            state.offset = 0.0;
            shell.request_redraw();
            return;
        }
        state.offset = next;
        shell.request_redraw_at(now + MARQUEE_FRAME);
    }

    fn mouse_interaction(
        &self,
        _tree: &widget::Tree,
        _layout: layout::Layout,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        mouse::Interaction::default()
    }
}

/// Helper constructor for a marquee text widget.
pub(crate) fn marquee_text(content: impl Into<String>) -> MarqueeText {
    MarqueeText::new(content)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn state_started(ago: Duration, skip_initial_pause: bool) -> State {
        State {
            cycle_start: Instant::now() - ago,
            skip_initial_pause,
            ..State::default()
        }
    }

    #[test]
    fn a_new_cycle_holds_at_the_start_for_the_reading_pause() {
        let state = state_started(Duration::from_secs(1), false);
        assert_eq!(scroll_offset(&state, 300.0, Instant::now()), 0.0);
    }

    #[test]
    fn a_hover_start_scrolls_at_once() {
        let state = state_started(Duration::from_secs(1), true);
        let offset = scroll_offset(&state, 300.0, Instant::now());
        assert!(
            offset > 0.0,
            "skipping the pause scrolls from the first second"
        );
    }

    #[test]
    fn the_offset_wraps_after_one_loop() {
        let width = 100.0;
        let loop_secs = (width + LOOP_GAP) / SCROLL_PX_PER_SEC;
        let state = state_started(Duration::from_secs_f32(loop_secs + 0.5), true);
        let offset = scroll_offset(&state, width, Instant::now());
        assert!(offset < width + LOOP_GAP);
        assert!((offset - 0.5 * SCROLL_PX_PER_SEC).abs() < 1.0);
    }

    #[test]
    fn the_measure_key_follows_text_size_and_font() {
        let font = Font::default();
        let base = measure_key("Title", Pixels(10.0), font, Some(1.0));
        assert_eq!(base, measure_key("Title", Pixels(10.0), font, Some(1.0)));
        assert_ne!(base, measure_key("Title 2", Pixels(10.0), font, Some(1.0)));
        assert_ne!(base, measure_key("Title", Pixels(11.0), font, Some(1.0)));
        assert_ne!(base, measure_key("Title", Pixels(10.0), font, Some(2.0)));
        let bold = Font {
            weight: Weight::Bold,
            ..font
        };
        assert_ne!(base, measure_key("Title", Pixels(10.0), bold, Some(1.0)));
    }

    #[test]
    fn motion_follows_the_switch() {
        set_marquee_motion(true);
        assert!(marquee_motion());
        set_marquee_motion(false);
        assert!(!marquee_motion());
    }
}
