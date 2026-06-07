//! Custom Progress Bar Widget with flat styling and drag-to-seek
//!
//! Flat-design progress bar:
//! - 6 px thin track (`theme::bg2()` fill) with `accent_bright()` progress fill
//! - 14 px square (flat) / pill (rounded) handle with 1 px `bg0_hard()` border
//! - Click-on-track jumps to position, drag-handle seeks
//! - Seek tooltip drawn via `overlay()` for proper z-ordering
//!
//! Based on Iced's slider widget event handling pattern.

use iced::{
    Element, Event, Length, Point, Rectangle, Shadow, Size, Theme, Vector,
    advanced::{
        Shell,
        layout::{self, Layout},
        renderer,
        widget::{self, Widget},
    },
    mouse, touch,
};

/// State for progress bar interaction
#[derive(Debug, Clone, Default)]
pub(crate) struct State {
    is_dragging: bool,
    drag_progress: f32,
    last_position: f32,
    last_update: Option<std::time::Instant>,
}

/// One end-cap label for the filled capsule scrub. `full` is the entire string
/// (time + codec / bitrate), drawn at the dimmer metadata opacity; `time` is the
/// elapsed / duration portion re-drawn at full opacity over the same outer-edge
/// alignment anchor (left cap → left edge, right cap → right edge), so only the
/// codec / bitrate reads dimmer than the time. When the cap carries no metadata,
/// `time == full` and it renders fully opaque.
#[derive(Debug, Clone)]
pub struct CapLabel {
    pub full: String,
    pub time: String,
}

impl CapLabel {
    /// Cap with no metadata — `time == full`, renders fully opaque.
    pub fn time_only(time: impl Into<String>) -> Self {
        let time = time.into();
        Self {
            full: time.clone(),
            time,
        }
    }

    /// Cap whose `time` stays opaque while the rest of `full` (codec / bitrate)
    /// renders dimmer.
    pub fn new(full: impl Into<String>, time: impl Into<String>) -> Self {
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

/// Custom progress bar with flat styling
pub struct ProgressBar<'a, Message> {
    position: f32,
    duration: f32,
    is_playing: bool,
    on_seek: Box<dyn Fn(f32) -> Message + 'a>,
    width: Length,
    height: f32,
    hide_handle: bool,
    interactive: bool,
    filled: bool,
    /// `(left, right)` end-cap labels drawn overlaid on the FILLED track. Each
    /// cap is rendered with the fill / track regions clipped so the text stays
    /// legible (dark over the bright fill, light over the dark track), splitting
    /// color at the fill edge; the codec / bitrate portion is underpainted at a
    /// dimmer opacity so the time pops. Capsule scrub only. See [`CapLabel`].
    time_labels: Option<(CapLabel, CapLabel)>,
}

/// Visual thickness of the progress track within the widget bounds.
/// The widget bounds may be taller (to accommodate the handle hit target);
/// the track itself is centered vertically within those bounds.
const TRACK_THICKNESS: f32 = 6.0;
/// Handle size — 14 px square in flat mode, pill in rounded mode.
const HANDLE_SIZE: f32 = 14.0;

impl<'a, Message> ProgressBar<'a, Message> {
    pub fn new<F>(position: f32, duration: f32, on_seek: F) -> Self
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
            filled: false,
            time_labels: None,
        }
    }

    /// Draw the track + progress fill at the FULL widget height (a solid bar)
    /// instead of the default thin 6 px centered track. Used by the MiniPlayer
    /// capsule scrub, where the progress reads as one continuous filled block.
    pub fn filled(mut self, filled: bool) -> Self {
        self.filled = filled;
        self
    }

    /// Overlay the left / right end-cap labels on the filled track with
    /// color-aware (fill-vs-track) coloring and a dimmer codec / bitrate
    /// segment. Only drawn in [`Self::filled`] mode. See [`CapLabel`].
    pub fn time_labels(mut self, left: CapLabel, right: CapLabel) -> Self {
        self.time_labels = Some((left, right));
        self
    }

    #[allow(clippy::wrong_self_convention)] // Builder pattern setter, not an accessor
    pub fn is_playing(mut self, is_playing: bool) -> Self {
        self.is_playing = is_playing;
        self
    }

    pub fn hide_handle(mut self, hide: bool) -> Self {
        self.hide_handle = hide;
        self
    }

    /// Whether click/drag seeking is enabled. Defaults to `true`. Set `false`
    /// for non-seekable streams (radio). Decoupled from [`Self::hide_handle`]
    /// so a handle-less overlay scrub can still be clickable to seek.
    pub fn interactive(mut self, interactive: bool) -> Self {
        self.interactive = interactive;
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    /// Calculate seek position from cursor X coordinate. Filled mode maps the
    /// FULL width (no handle inset, since it has no handle); the thin track
    /// reserves `HANDLE_SIZE` so the handle center tracks the cursor.
    fn locate(&self, cursor_x: f32, bounds: Rectangle) -> f32 {
        let (effective_width, offset) = if self.filled {
            (bounds.width, 0.0)
        } else {
            (bounds.width - HANDLE_SIZE, HANDLE_SIZE / 2.0)
        };

        if effective_width <= 0.0 {
            return 0.0;
        }

        let relative_x = cursor_x - bounds.x - offset;
        let percentage = (relative_x / effective_width).clamp(0.0, 1.0);
        percentage * self.duration
    }
}

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
        _tree: &mut widget::Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, self.width, self.height)
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();

        // Track position changes for interpolation
        // When position changes (from playback update), record it
        if (self.position - state.last_position).abs() > 0.01 {
            state.last_position = self.position;
            state.last_update = Some(std::time::Instant::now());
        }

        // If playing, request continuous redraws for smooth interpolation
        if self.is_playing && !state.is_dragging {
            shell.request_redraw();
        }

        // Non-seekable streams (radio) disable all interaction. Note this is
        // decoupled from `hide_handle`: an overlay scrub hides the handle but
        // stays seekable.
        if !self.interactive {
            return;
        }

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                if let Some(cursor_position) = cursor.position_over(bounds) {
                    // Calculate current handle position
                    let current_progress = if self.duration > 0.0 {
                        (self.position / self.duration).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let effective_width = bounds.width - HANDLE_SIZE;
                    let handle_x = bounds.x + current_progress * effective_width;

                    // Check if click is within the handle bounds
                    let handle_bounds = Rectangle {
                        x: handle_x,
                        y: bounds.y,
                        width: HANDLE_SIZE,
                        height: bounds.height,
                    };

                    // Check if user clicked on the handle itself. Filled mode
                    // has no handle, so it's always a track-click.
                    let clicked_on_handle = !self.filled
                        && cursor_position.x >= handle_bounds.x
                        && cursor_position.x <= handle_bounds.x + handle_bounds.width
                        && cursor_position.y >= handle_bounds.y
                        && cursor_position.y <= handle_bounds.y + handle_bounds.height;

                    if clicked_on_handle {
                        // Start dragging from current position
                        state.is_dragging = true;
                        state.drag_progress = current_progress;
                        shell.capture_event();
                        shell.request_redraw();
                    } else {
                        // Clicked on track - immediately seek to clicked position
                        let seek_pos = self.locate(cursor_position.x, bounds);
                        let new_progress = (seek_pos / self.duration).clamp(0.0, 1.0);

                        // Update visual position and seek immediately
                        state.drag_progress = new_progress;
                        // Filled mode has no grabbable handle, so a track press
                        // also begins a drag — enabling click-and-drag scrub
                        // from anywhere along the bar.
                        if self.filled {
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
                    let seek_pos = self.locate(x, bounds);
                    state.drag_progress = (seek_pos / self.duration).clamp(0.0, 1.0);
                    shell.capture_event();
                    // Request redraw to show updated handle position
                    shell.request_redraw();
                }
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
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer;

        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();

        // Calculate handle position with smooth interpolation during playback
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
        // Filled mode has no handle, so its fill spans the full width; the thin
        // track reserves HANDLE_SIZE so the handle center tracks the fill edge.
        let effective_width = if self.filled {
            bounds.width
        } else {
            bounds.width - HANDLE_SIZE
        };
        let handle_x = bounds.x + progress * effective_width;

        // Default: a 6px thin track centered vertically. Filled mode: a solid
        // full-height bar (capsule scrub) — the track spans the whole widget.
        // `ui_radius_pill()` returns `0.0.into()` in flat mode and the pill
        // radius in rounded mode — no separate ladder needed.
        let (track_y, track_h) = if self.filled {
            (bounds.y, bounds.height)
        } else {
            (
                bounds.y + (bounds.height - TRACK_THICKNESS) / 2.0,
                TRACK_THICKNESS,
            )
        };
        // Filled (capsule) mode is always square so the track butts flush
        // against its time end-caps as one connected element — even in rounded
        // mode. The thin track follows the theme pill radius.
        let track_radius = if self.filled {
            iced::border::Radius::from(0.0)
        } else {
            crate::theme::ui_radius_pill_player()
        };
        // Filled mode uses the darker `bg0` track (capsule); thin uses `bg2`.
        let track_bg = if self.filled {
            crate::theme::bg0()
        } else {
            crate::theme::bg2()
        };

        // Track background.
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x: bounds.x,
                    y: track_y,
                    width: bounds.width,
                    height: track_h,
                },
                border: iced::Border {
                    radius: track_radius,
                    ..Default::default()
                },
                ..Default::default()
            },
            track_bg,
        );

        // Progress fill — accent_bright. Filled mode fills edge-to-edge to the
        // progress point; the thin track fills to the handle center.
        let fill_width = if self.filled {
            (progress * bounds.width).clamp(0.0, bounds.width)
        } else {
            (handle_x - bounds.x + HANDLE_SIZE / 2.0).clamp(0.0, bounds.width)
        };
        if fill_width > 0.0 {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: bounds.x,
                        y: track_y,
                        width: fill_width,
                        height: track_h,
                    },
                    border: iced::Border {
                        radius: track_radius,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                crate::theme::accent_bright(),
            );
        }

        // Filled (capsule) mode is structurally handle-less — it reads like a
        // level meter — so the handle never draws there regardless of the flag.
        if !self.hide_handle && !self.filled {
            // Handle on a separate layer so it draws above any neighboring quads.
            let handle_clip = bounds;
            renderer.with_layer(handle_clip, |renderer| {
                let handle_y = bounds.y + (bounds.height - HANDLE_SIZE) / 2.0;
                let handle_bounds = Rectangle {
                    x: handle_x,
                    y: handle_y,
                    width: HANDLE_SIZE,
                    height: HANDLE_SIZE,
                };
                // `ui_radius_pill()` returns `0.0.into()` in flat mode.
                let handle_radius = crate::theme::ui_radius_pill_player();
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: handle_bounds,
                        border: iced::Border {
                            color: crate::theme::bg0_hard(),
                            width: 1.0,
                            radius: handle_radius,
                        },
                        ..Default::default()
                    },
                    crate::theme::accent_bright(),
                );
            });
        }

        // Color-aware overlaid end-cap labels (filled / capsule mode). Each cap
        // is painted twice — clipped to the fill region (dark text over the
        // bright fill) and the track region (light text over the dark track) —
        // so it stays legible and flips color exactly at the fill edge. The
        // codec / bitrate is underpainted at a dimmer opacity (the full string),
        // then the bare time is repainted opaque over the same outer-edge anchor
        // so only the time pops.
        if self.filled
            && let Some((left, right)) = &self.time_labels
        {
            use iced::{
                Pixels,
                advanced::text::{self, Renderer as TextRenderer, Shaping, Text},
                alignment,
            };

            const PAD: f32 = 10.0;
            /// Codec / bitrate opacity relative to the time — reads as secondary.
            const META_ALPHA: f32 = 0.6;
            let on_fill = crate::theme::bg0_hard();
            let on_track = crate::theme::fg1();
            let dim = |c: iced::Color| iced::Color {
                a: c.a * META_ALPHA,
                ..c
            };
            let font = crate::theme::ui_font();
            let fill_clip = Rectangle {
                x: bounds.x,
                y: bounds.y,
                width: fill_width,
                height: bounds.height,
            };
            let track_clip = Rectangle {
                x: bounds.x + fill_width,
                y: bounds.y,
                width: (bounds.width - fill_width).max(0.0),
                height: bounds.height,
            };
            let make = |content: String, align_x: text::Alignment| Text {
                content,
                bounds: bounds.size(),
                size: Pixels(11.0),
                line_height: text::LineHeight::default(),
                font,
                align_x,
                align_y: alignment::Vertical::Center,
                shaping: Shaping::Basic,
                wrapping: text::Wrapping::None,
                ellipsis: text::Ellipsis::default(),
                hint_factor: Some(1.0),
            };
            let cy = bounds.center_y();
            let left_pos = Point::new(bounds.x + PAD, cy);
            let right_pos = Point::new(bounds.x + bounds.width - PAD, cy);

            // Paint a string in both the fill and track regions (color split at
            // the fill edge).
            let mut paint = |content: &str,
                             align_x: text::Alignment,
                             pos: Point,
                             fill_c: iced::Color,
                             track_c: iced::Color| {
                renderer.fill_text(make(content.to_string(), align_x), pos, fill_c, fill_clip);
                renderer.fill_text(make(content.to_string(), align_x), pos, track_c, track_clip);
            };

            // LEFT cap — time anchored at the left edge, codec / kHz trailing
            // toward center (underpainted dimmer, then time repainted opaque).
            if left.has_meta() {
                paint(
                    &left.full,
                    text::Alignment::Left,
                    left_pos,
                    dim(on_fill),
                    dim(on_track),
                );
            }
            paint(
                &left.time,
                text::Alignment::Left,
                left_pos,
                on_fill,
                on_track,
            );

            // RIGHT cap — time anchored at the right edge, kbps leading from
            // center (underpainted dimmer, then time repainted opaque).
            if right.has_meta() {
                paint(
                    &right.full,
                    text::Alignment::Right,
                    right_pos,
                    dim(on_fill),
                    dim(on_track),
                );
            }
            paint(
                &right.time,
                text::Alignment::Right,
                right_pos,
                on_fill,
                on_track,
            );
        }

        // Tooltip is drawn via overlay() for proper z-ordering
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
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
        } else if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut widget::Tree,
        layout: Layout<'_>,
        _renderer: &iced::Renderer,
        _viewport: &Rectangle,
        translation: Vector,
    ) -> Option<iced::advanced::overlay::Element<'b, Message, Theme, iced::Renderer>> {
        let state = tree.state.downcast_ref::<State>();

        if state.is_dragging {
            let bounds = layout.bounds();
            let effective_width = bounds.width - HANDLE_SIZE;
            let handle_x = bounds.x + state.drag_progress * effective_width;

            Some(iced::advanced::overlay::Element::new(Box::new(
                TooltipOverlay {
                    handle_x: handle_x + translation.x,
                    handle_width: HANDLE_SIZE,
                    bounds_y: bounds.y + translation.y,
                    drag_progress: state.drag_progress,
                    duration: self.duration,
                },
            )))
        } else {
            None
        }
    }
}

impl<'a, Message: Clone + 'a> From<ProgressBar<'a, Message>> for Element<'a, Message> {
    fn from(progress_bar: ProgressBar<'a, Message>) -> Self {
        Element::new(progress_bar)
    }
}

/// Overlay for the seek time tooltip — renders on top of all other widgets.
///
/// Flat design: 1 px `theme::border()` outline on `theme::bg0_hard()` background;
/// no 3D bevel. A small downward arrow points at the handle.
struct TooltipOverlay {
    handle_x: f32,
    handle_width: f32,
    bounds_y: f32,
    drag_progress: f32,
    duration: f32,
}

impl<Message> iced::advanced::overlay::Overlay<Message, Theme, iced::Renderer> for TooltipOverlay {
    fn layout(&mut self, _renderer: &iced::Renderer, _bounds: Size) -> layout::Node {
        // Tooltip dimensions
        let tooltip_height = 20.0;
        let tooltip_width = 44.0;
        let tooltip_arrow_size = 6.0;
        let tooltip_gap = 2.0;

        // Position tooltip above the handle
        let tooltip_x = self.handle_x + (self.handle_width - tooltip_width) / 2.0;
        let tooltip_y = self.bounds_y - tooltip_height - tooltip_arrow_size - tooltip_gap;

        layout::Node::new(Size::new(
            tooltip_width,
            tooltip_height + tooltip_arrow_size,
        ))
        .move_to(Point::new(tooltip_x, tooltip_y))
    }

    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
    ) {
        use iced::{
            Pixels,
            advanced::{
                Renderer,
                text::{self, Renderer as TextRenderer, Shaping, Text},
            },
            alignment,
        };

        let bounds = layout.bounds();

        // Tooltip dimensions
        let tooltip_height = 20.0;
        let tooltip_width = 44.0;
        let tooltip_arrow_size = 6.0;

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

        // Arrow pointing down toward the handle. Drawn as a small filled square
        // tucked below the tooltip body — keeps the visual link without needing
        // a triangle primitive.
        let arrow_x = tooltip_x + tooltip_width / 2.0 - tooltip_arrow_size / 2.0;
        let arrow_y = tooltip_y + tooltip_height;

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
        let font = crate::theme::ui_font();
        let text_size = Pixels(12.0);

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
                size: text_size,
                line_height: text::LineHeight::default(),
                font,
                align_x: text::Alignment::Center,
                align_y: alignment::Vertical::Center,
                shaping: Shaping::Basic,
                wrapping: text::Wrapping::default(),
                ellipsis: iced::advanced::text::Ellipsis::default(),
                hint_factor: Some(1.0),
            },
            tooltip_bounds.center(),
            tooltip_text_color,
            Rectangle::with_size(Size::INFINITE),
        );
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

    fn bar(duration: f32, filled: bool) -> ProgressBar<'static, ()> {
        ProgressBar::new(0.0, duration, |_| ()).filled(filled)
    }

    /// Filled (capsule) mode seeks across the FULL width — the edges map to the
    /// track ends with no handle inset, so click/drag-to-seek lands accurately.
    #[test]
    fn filled_locate_maps_full_width_edges_to_ends() {
        let b = bar(100.0, true);
        let bounds = Rectangle {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: CAPSULE_DEFAULT_TEST_HEIGHT,
        };
        assert_eq!(b.locate(0.0, bounds), 0.0);
        assert_eq!(b.locate(50.0, bounds), 50.0);
        assert_eq!(b.locate(100.0, bounds), 100.0);
    }

    /// Thin mode reserves `HANDLE_SIZE`; the cursor at the handle's leftmost
    /// center (x = HANDLE_SIZE/2) maps to the start. Guards against a future
    /// refactor leaking the handle offset into filled mode.
    #[test]
    fn thin_locate_insets_by_half_handle() {
        let b = bar(100.0, false);
        let bounds = Rectangle {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: CAPSULE_DEFAULT_TEST_HEIGHT,
        };
        assert_eq!(b.locate(HANDLE_SIZE / 2.0, bounds), 0.0);
    }

    #[test]
    fn cap_label_has_meta_distinguishes_time_only() {
        assert!(!CapLabel::time_only("3:40").has_meta());
        assert!(CapLabel::new("3:40 · FLAC 44.1kHz", "3:40").has_meta());
    }

    const CAPSULE_DEFAULT_TEST_HEIGHT: f32 = 20.0;
}
