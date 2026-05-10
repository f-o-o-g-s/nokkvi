//! Marquee text widget — scrolls overflowing text horizontally.
//!
//! If the text fits within its container, it renders normally (no animation).
//! If it overflows, the text scrolls continuously in a ring-buffer loop:
//! the text is rendered twice with a gap, so as one copy scrolls off the left
//! the next copy seamlessly appears from the right.

use std::time::Instant;

use iced::{
    Color, Element, Length, Pixels, Rectangle, Size, Theme, Vector,
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
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<State>();

        // Follow Iced's native text layout pattern: layout::sized() resolves
        // Length::Fill/Shrink against limits, then we build the paragraph with
        // the resolved bounds and return min_bounds().
        layout::sized(limits, Length::Fill, Length::Shrink, |limits| {
            let bounds = limits.max();

            let text = Text {
                content: self.content.as_str(),
                bounds,
                size: self.size,
                line_height: advanced_text::LineHeight::default(),
                font: self.font,
                align_x: advanced_text::Alignment::Left,
                align_y: iced::alignment::Vertical::Center,
                shaping: Shaping::Advanced,
                wrapping: Wrapping::None,
                ellipsis: advanced_text::Ellipsis::None,
                hint_factor: renderer.scale_factor(),
            };
            state.constrained.update(text);

            // Measure full unconstrained width for scroll animation
            let unconstrained_text = Text {
                bounds: Size::new(f32::INFINITY, f32::INFINITY),
                ..text
            };
            let full_para =
                <iced::Renderer as TextRenderer>::Paragraph::with_text(unconstrained_text);
            state.full_width = full_para.min_bounds().width;

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
            }

            // Reset the cycle on a fits→overflows transition so the animation
            // restarts from offset 0.  When the transition is caused purely by a
            // resize (content unchanged), skip the initial pause so scrolling
            // begins immediately — the user is resizing the window, not looking
            // at a new track title.
            if now_overflowing && !state.was_overflowing {
                state.cycle_start = Instant::now();
                if !content_changed {
                    // Pure resize-to-overflow: no new title to read from the start.
                    state.skip_initial_pause = true;
                }
            }

            state.was_overflowing = now_overflowing;

            state.constrained.min_bounds()
        })
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
            // Ring-buffer loop: render text twice with a gap
            let elapsed = state.cycle_start.elapsed().as_secs_f32();

            // One full loop cycle = scrolling through (content_width + gap) pixels
            let cycle_px = content_width + LOOP_GAP;

            let offset = if !state.skip_initial_pause && elapsed < INITIAL_PAUSE_SECS {
                // Hold at start so user can read the beginning of a new track title.
                // Skipped for resize-driven overflow or breakpoint-driven field drops
                // while already scrolling (skip_initial_pause = true in those cases).
                0.0
            } else {
                let scroll_elapsed = if state.skip_initial_pause {
                    elapsed
                } else {
                    elapsed - INITIAL_PAUSE_SECS
                };
                // Continuous modulo for seamless looping
                (scroll_elapsed * SCROLL_PX_PER_SEC) % cycle_px
            };

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
        layout: layout::Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _shell: &mut iced::advanced::Shell<'_, M>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let overflow = state.full_width - layout.bounds().width;

        if overflow > 0.0 {
            _shell.request_redraw();
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &widget::Tree,
        _layout: layout::Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        mouse::Interaction::default()
    }
}

impl<'a, M: 'static> From<MarqueeText> for Element<'a, M> {
    fn from(marquee: MarqueeText) -> Self {
        Element::new(marquee)
    }
}

/// Helper constructor for a marquee text widget.
pub(crate) fn marquee_text(content: impl Into<String>) -> MarqueeText {
    MarqueeText::new(content)
}
