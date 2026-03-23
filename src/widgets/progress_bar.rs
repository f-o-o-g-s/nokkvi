//! Custom Progress Bar Widget with 3D styling and drag-to-seek
//!
//! This is a custom Iced widget that provides:
//! - 3D styled track with inset borders
//! - 3D styled handle with grip lines  
//! - Handle-only drag-to-seek (user must grab the handle to seek)
//!
//! Based on Iced's slider widget event handling pattern.

use iced::{
    Color, Element, Event, Length, Point, Rectangle, Shadow, Size, Theme, Vector,
    advanced::{
        Shell,
        layout::{self, Layout},
        renderer,
        text::{Paragraph as _, Renderer as TextRenderer, Shaping, Text, paragraph::Plain},
        widget::{self, Widget},
    },
    alignment, mouse, touch,
    widget::text::Wrapping,
};

/// A single text segment with its own color for the progress bar overlay.
#[derive(Clone, Debug)]
pub struct OverlaySegment {
    pub text: String,
    pub color: Color,
}

/// Per-segment paragraph + measured width, stored in widget state.
#[derive(Debug, Clone)]
struct SegmentState {
    paragraph: Plain<<iced::Renderer as TextRenderer>::Paragraph>,
    width: f32,
    color: Color,
}

impl Default for SegmentState {
    fn default() -> Self {
        Self {
            paragraph: Plain::default(),
            width: 0.0,
            color: Color::TRANSPARENT,
        }
    }
}

/// State for progress bar interaction
#[derive(Debug, Clone)]
pub(crate) struct State {
    is_dragging: bool,
    drag_progress: f32,
    last_position: f32,
    last_update: Option<std::time::Instant>,
    // Overlay segment animation
    overlay_segments: Vec<SegmentState>,
    overlay_full_width: f32,
    overlay_cycle_start: std::time::Instant,
}

impl Default for State {
    fn default() -> Self {
        Self {
            is_dragging: false,
            drag_progress: 0.0,
            last_position: 0.0,
            last_update: None,
            overlay_segments: Vec::new(),
            overlay_full_width: 0.0,
            overlay_cycle_start: std::time::Instant::now(),
        }
    }
}

/// Custom progress bar with 3D styling
pub struct ProgressBar<'a, Message> {
    position: f32,
    duration: f32,
    is_playing: bool,
    on_seek: Box<dyn Fn(f32) -> Message + 'a>,
    width: Length,
    height: f32,
    overlay_segments: Vec<OverlaySegment>,
}

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
            overlay_segments: Vec::new(),
        }
    }

    #[allow(clippy::wrong_self_convention)] // Builder pattern setter, not an accessor
    pub fn is_playing(mut self, is_playing: bool) -> Self {
        self.is_playing = is_playing;
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

    pub fn overlay_segments(mut self, segments: Vec<OverlaySegment>) -> Self {
        self.overlay_segments = segments;
        self
    }

    /// Calculate seek position from cursor X coordinate
    fn locate(&self, cursor_x: f32, bounds: Rectangle) -> f32 {
        let handle_width = 32.0;
        let effective_width = bounds.width - handle_width;

        if effective_width <= 0.0 {
            return 0.0;
        }

        // Calculate position relative to track (accounting for handle width)
        let relative_x = cursor_x - bounds.x - handle_width / 2.0;
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
        tree: &mut widget::Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let node = layout::atomic(limits, self.width, self.height);

        // Build overlay segment paragraphs if configured
        if !self.overlay_segments.is_empty() {
            let state = tree.state.downcast_mut::<State>();

            let font = iced::Font {
                weight: iced::font::Weight::Normal,
                ..crate::theme::ui_font()
            };
            let hint_factor = {
                use iced::advanced::Renderer as _;
                renderer.scale_factor()
            };

            // Resize state vec to match segment count
            state
                .overlay_segments
                .resize_with(self.overlay_segments.len(), SegmentState::default);

            let mut total_width: f32 = 0.0;
            for (i, seg) in self.overlay_segments.iter().enumerate() {
                // Measure unconstrained width per segment
                let unconstrained = Text {
                    content: seg.text.as_str(),
                    bounds: Size::new(f32::INFINITY, f32::INFINITY),
                    size: iced::Pixels(8.0),
                    line_height: iced::advanced::text::LineHeight::default(),
                    font,
                    align_x: alignment::Horizontal::Left.into(),
                    align_y: alignment::Vertical::Center,
                    shaping: Shaping::Advanced,
                    wrapping: Wrapping::None,
                    ellipsis: iced::advanced::text::Ellipsis::None,
                    hint_factor,
                };
                let para = <iced::Renderer as TextRenderer>::Paragraph::with_text(unconstrained);
                let seg_width = para.min_bounds().width;

                // Store the constrained paragraph (clipped to track width for rendering)
                let text_area_width = node.size().width * 0.99;
                let constrained = Text {
                    bounds: Size::new(text_area_width, self.height),
                    ..unconstrained
                };
                state.overlay_segments[i].paragraph.update(constrained);
                state.overlay_segments[i].width = seg_width;
                state.overlay_segments[i].color = seg.color;
                total_width += seg_width;
            }

            // Only reset scroll animation when text width changes significantly
            if (total_width - state.overlay_full_width).abs() > 5.0 {
                state.overlay_cycle_start = std::time::Instant::now();
            }
            state.overlay_full_width = total_width;
        }

        node
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

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                if let Some(cursor_position) = cursor.position_over(bounds) {
                    // Calculate current handle position
                    let handle_width = 32.0;
                    let current_progress = if self.duration > 0.0 {
                        (self.position / self.duration).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let effective_width = bounds.width - handle_width;
                    let handle_x = bounds.x + current_progress * effective_width;

                    // Check if click is within the handle bounds
                    let handle_bounds = Rectangle {
                        x: handle_x,
                        y: bounds.y,
                        width: handle_width,
                        height: bounds.height,
                    };

                    // Check if user clicked on the handle itself
                    let clicked_on_handle = cursor_position.x >= handle_bounds.x
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
                        shell.publish((self.on_seek)(seek_pos));
                        shell.capture_event();
                        shell.request_redraw();
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerLifted { .. } | touch::Event::FingerLost { .. }) => {
                if state.is_dragging {
                    // C++ pattern: ONLY seek to final position when user releases after drag
                    // This prevents seek spam during dragging
                    let seek_pos = state.drag_progress * self.duration;
                    shell.publish((self.on_seek)(seek_pos));
                    state.is_dragging = false;
                }
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
        let handle_width = 32.0;
        let border_width = 1.0;

        // Calculate handle position with smooth interpolation during playback
        let progress = if state.is_dragging {
            // During drag, use the visual drag position (not the actual playback position)
            state.drag_progress
        } else if self.is_playing && state.last_update.is_some() && self.duration > 0.0 {
            // Interpolate position based on elapsed time since last update
            let elapsed = state.last_update.unwrap().elapsed().as_secs_f32();
            let interpolated_pos = (state.last_position + elapsed).min(self.duration);
            (interpolated_pos / self.duration).clamp(0.0, 1.0)
        } else if self.duration > 0.0 {
            // Not playing or no timing info - use actual position
            (self.position / self.duration).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let effective_width = bounds.width - handle_width;
        let handle_x = bounds.x + progress * effective_width;

        // Colors from theme (supports light/dark mode)
        let bg1 = crate::theme::bg1();
        // Track uses inset 3D effect (dark on top/left for "carved in" look)
        let (track_top_left, track_bottom_right) = crate::theme::border_3d_inset();
        let accent = crate::theme::accent_bright();
        // Handle uses raised accent 3D effect
        let (accent_top_left, accent_bottom_right) = crate::theme::border_3d_accent_raised();
        // Grip uses raised accent effect (same as handle)
        let (grip_top_left, grip_bottom_right) = crate::theme::border_3d_accent_raised();
        let grip_mid = crate::theme::accent();

        let radius = crate::theme::ui_border_radius();
        let is_rounded = crate::theme::is_rounded_mode();

        // Track background + borders
        if is_rounded {
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border: iced::Border {
                        color: track_top_left,
                        width: border_width,
                        radius,
                    },
                    ..Default::default()
                },
                bg1,
            );
        } else {
            // Track background (main BG2 area)
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: bounds.x + border_width,
                        y: bounds.y + border_width,
                        width: bounds.width - border_width * 2.0,
                        height: bounds.height - border_width * 2.0,
                    },
                    ..Default::default()
                },
                bg1,
            );

            // Track top border (dark - inset effect)
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: bounds.x,
                        y: bounds.y,
                        width: bounds.width,
                        height: border_width,
                    },
                    ..Default::default()
                },
                track_top_left,
            );

            // Track left border (dark - inset effect)
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: bounds.x,
                        y: bounds.y,
                        width: border_width,
                        height: bounds.height,
                    },
                    ..Default::default()
                },
                track_top_left,
            );

            // Track bottom border (light - inset effect)
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: bounds.x,
                        y: bounds.y + bounds.height - border_width,
                        width: bounds.width,
                        height: border_width,
                    },
                    ..Default::default()
                },
                track_bottom_right,
            );

            // Track right border (light - inset effect)
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: bounds.x + bounds.width - border_width,
                        y: bounds.y,
                        width: border_width,
                        height: bounds.height,
                    },
                    ..Default::default()
                },
                track_bottom_right,
            );
        }

        // Overlay segments: scrolling colored metadata centered in the progress bar track
        if !state.overlay_segments.is_empty() {
            let text_area_width = bounds.width * 0.99;
            let text_x = bounds.x + (bounds.width - text_area_width) / 2.0;
            // Use the first segment's paragraph height for vertical centering
            let text_height = state.overlay_segments[0].paragraph.min_bounds().height;
            let vert_y = bounds.y + (bounds.height - text_height) / 2.0;
            let content_width = state.overlay_full_width;
            let clip = Rectangle {
                x: text_x,
                y: bounds.y,
                width: text_area_width,
                height: bounds.height,
            };

            const SCROLL_PX_PER_SEC: f32 = 30.0;
            const LOOP_GAP: f32 = 80.0;
            const INITIAL_PAUSE_SECS: f32 = 2.0;

            let overflow = (content_width - text_area_width).max(0.0);

            // Helper: render all segments at a given base X offset
            let render_segments = |renderer: &mut iced::Renderer, base_x: f32| {
                let mut x_cursor = base_x;
                for seg in &state.overlay_segments {
                    renderer.fill_paragraph(
                        seg.paragraph.raw(),
                        Point::new(x_cursor, vert_y),
                        seg.color,
                        clip,
                    );
                    x_cursor += seg.width;
                }
            };

            if overflow <= 0.0 {
                // Text fits — center horizontally
                let cx = text_x + (text_area_width - content_width) / 2.0;
                render_segments(renderer, cx);
            } else {
                // Scrolling ring-buffer animation
                let elapsed = state.overlay_cycle_start.elapsed().as_secs_f32();
                let cycle_px = content_width + LOOP_GAP;
                let offset = if elapsed < INITIAL_PAUSE_SECS {
                    0.0
                } else {
                    ((elapsed - INITIAL_PAUSE_SECS) * SCROLL_PX_PER_SEC) % cycle_px
                };

                render_segments(renderer, text_x - offset);
                render_segments(renderer, text_x - offset + cycle_px);
            }
        }
        // Handle + grip in a separate layer so it renders ON TOP of overlay text.
        // (Iced's wgpu renderer renders quads before text within the same layer,
        //  so a new layer is needed to ensure the handle appears above the text.)
        renderer.with_layer(bounds, |renderer| {
            // Handle background + borders
            let handle_bounds = Rectangle {
                x: handle_x,
                y: bounds.y,
                width: handle_width,
                height: bounds.height,
            };
            let shadow_color = Color::from_rgba(0.0, 0.0, 0.0, 0.7);

            if is_rounded {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: handle_bounds,
                        border: iced::Border {
                            color: accent_top_left,
                            width: border_width,
                            radius,
                        },
                        shadow: Shadow {
                            color: shadow_color,
                            offset: Vector::new(0.0, 2.5),
                            blur_radius: 3.0,
                        },
                        ..Default::default()
                    },
                    accent,
                );
            } else {
                // Handle background with integrated shadow
                // IMPORTANT: Shadow must be on a quad with a real fill color, not TRANSPARENT.
                // Iced's WGSL shader blends shadow with quad_color, and using TRANSPARENT
                // causes edge artifacts (white pixels) in light mode.
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: handle_x + border_width,
                            y: bounds.y + border_width,
                            width: handle_width - border_width * 2.0,
                            height: bounds.height - border_width * 2.0,
                        },
                        shadow: Shadow {
                            color: shadow_color,
                            offset: Vector::new(0.0, 2.5),
                            blur_radius: 3.0,
                        },
                        ..Default::default()
                    },
                    accent,
                );

                // Handle top border (dark - 3D raised effect)
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: handle_x,
                            y: bounds.y,
                            width: handle_width,
                            height: border_width,
                        },
                        ..Default::default()
                    },
                    accent_top_left,
                );

                // Handle left border (dark - 3D raised effect)
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: handle_x,
                            y: bounds.y,
                            width: border_width,
                            height: bounds.height,
                        },
                        ..Default::default()
                    },
                    accent_top_left,
                );

                // Handle bottom border - use base accent (not lightened) to avoid white line
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: handle_x,
                            y: bounds.y + bounds.height - border_width,
                            width: handle_width,
                            height: border_width,
                        },
                        ..Default::default()
                    },
                    accent,
                );

                // Handle right border (shadow - 3D raised effect)
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: handle_x + handle_width - border_width,
                            y: bounds.y,
                            width: border_width,
                            height: bounds.height,
                        },
                        ..Default::default()
                    },
                    accent_bottom_right,
                );
            }

            // Grip groove (centered in handle)
            if is_rounded {
                // Rounded mode: mini version of the handle shape (rounded rect with border)
                let grip_width = 16.0;
                let grip_height = 6.0;
                let grip_x = handle_x + (handle_width - grip_width) / 2.0;
                let grip_y = bounds.y + (bounds.height - grip_height) / 2.0;

                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: grip_x,
                            y: grip_y,
                            width: grip_width,
                            height: grip_height,
                        },
                        border: iced::Border {
                            radius,
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    grip_top_left,
                );
            } else {
                // Non-rounded mode: 3D raised rectangle grip
                let grip_padding = 8.0;
                let grip_width = handle_width - grip_padding * 2.0;
                let grip_height = 8.0;
                let grip_x = handle_x + grip_padding;
                let grip_y = bounds.y + (bounds.height - grip_height) / 2.0;

                // Grip center fill
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: grip_x + 1.0,
                            y: grip_y + 1.0,
                            width: grip_width - 2.0,
                            height: grip_height - 2.0,
                        },
                        ..Default::default()
                    },
                    grip_mid,
                );

                // Grip top border (light - raised effect)
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: grip_x,
                            y: grip_y,
                            width: grip_width,
                            height: 1.0,
                        },
                        ..Default::default()
                    },
                    grip_top_left,
                );

                // Grip left border (light - raised effect)
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: grip_x,
                            y: grip_y,
                            width: 1.0,
                            height: grip_height,
                        },
                        ..Default::default()
                    },
                    grip_top_left,
                );

                // Grip bottom border (dark - raised effect)
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: grip_x,
                            y: grip_y + grip_height - 1.0,
                            width: grip_width,
                            height: 1.0,
                        },
                        ..Default::default()
                    },
                    grip_bottom_right,
                );

                // Grip right border (dark - raised effect)
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: grip_x + grip_width - 1.0,
                            y: grip_y,
                            width: 1.0,
                            height: grip_height,
                        },
                        ..Default::default()
                    },
                    grip_bottom_right,
                );
            }
        }); // end handle layer

        // Tooltip is now drawn via overlay() for proper z-ordering
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
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
            let handle_width = 32.0;
            let effective_width = bounds.width - handle_width;
            let handle_x = bounds.x + state.drag_progress * effective_width;

            Some(iced::advanced::overlay::Element::new(Box::new(
                TooltipOverlay {
                    handle_x: handle_x + translation.x,
                    handle_width,
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

/// Overlay for the seek time tooltip - renders on top of all other widgets
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
        let border_width = 2.0;

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

        // Tooltip colors using Gruvbox theme
        use crate::theme;
        let tooltip_bg = theme::bg0_hard();
        let tooltip_border_dark = theme::bg0();
        let tooltip_border_light = theme::bg3();
        let tooltip_text_color = theme::fg1();

        // Draw small arrow/pointer below tooltip (pointing down to handle)
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

        // Draw tooltip background with 3D borders
        let radius = crate::theme::ui_border_radius();
        let is_rounded = crate::theme::is_rounded_mode();

        if is_rounded {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: tooltip_x,
                        y: tooltip_y,
                        width: tooltip_width,
                        height: tooltip_height,
                    },
                    border: iced::Border {
                        color: tooltip_border_dark,
                        width: border_width,
                        radius,
                    },
                    ..Default::default()
                },
                tooltip_bg,
            );
        } else {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: tooltip_x + border_width,
                        y: tooltip_y + border_width,
                        width: tooltip_width - border_width * 2.0,
                        height: tooltip_height - border_width * 2.0,
                    },
                    ..Default::default()
                },
                tooltip_bg,
            );

            // Tooltip top border (dark)
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: tooltip_x,
                        y: tooltip_y,
                        width: tooltip_width,
                        height: border_width,
                    },
                    ..Default::default()
                },
                tooltip_border_dark,
            );

            // Tooltip left border (dark)
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: tooltip_x,
                        y: tooltip_y,
                        width: border_width,
                        height: tooltip_height,
                    },
                    ..Default::default()
                },
                tooltip_border_dark,
            );

            // Tooltip bottom border (light)
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: tooltip_x,
                        y: tooltip_y + tooltip_height - border_width,
                        width: tooltip_width,
                        height: border_width,
                    },
                    ..Default::default()
                },
                tooltip_border_light,
            );

            // Tooltip right border (light)
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: tooltip_x + tooltip_width - border_width,
                        y: tooltip_y,
                        width: border_width,
                        height: tooltip_height,
                    },
                    ..Default::default()
                },
                tooltip_border_light,
            );
        }

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
