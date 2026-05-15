//! Unified Volume Slider Widget with 3D styling and drag-to-adjust
//!
//! A single parameterized widget for both main volume and SFX volume controls.
//! This is a custom Iced widget that provides:
//! - 3D styled track with inset borders
//! - 3D styled handle with grip lines
//! - Click-to-set and drag-to-adjust functionality
//! - Themeable colors via `SliderVariant` (Music = aqua, SFX = yellow)

use iced::{
    Color, Element, Event, Length, Rectangle, Shadow, Size, Theme, Vector,
    advanced::{
        Shell,
        layout::{self, Layout},
        renderer,
        widget::{self, Widget},
    },
    mouse, touch,
};

// Volume slider dimensions - height matches player bar button size
const SLIDER_WIDTH: f32 = 14.0;
const SLIDER_HEIGHT: f32 = 44.0;
/// Horizontal slider length (1.25× the vertical height for more drag range)
const SLIDER_HORIZONTAL_LENGTH: f32 = 55.0;
/// Handle size in pixels (used for both width in horizontal and height in vertical)
const HANDLE_SIZE: f32 = 12.0;

/// Minimum volume change to trigger an update during dragging.
/// This prevents flooding the audio system with redundant volume commands.
const VOLUME_THROTTLE_THRESHOLD: f32 = 0.02; // 2% change required

/// Visual theme variant for the volume slider
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SliderVariant {
    /// Aqua/bright accent for main music volume
    #[default]
    Music,
    /// Yellow/darker accent for sound effects volume
    Sfx,
}

impl SliderVariant {
    /// Get the accent color for this variant
    fn accent_color(&self) -> Color {
        match self {
            SliderVariant::Music => crate::theme::accent_bright(),
            SliderVariant::Sfx => crate::theme::accent(),
        }
    }

    /// Get the 3D border colors for the handle (top-left, bottom-right)
    fn handle_borders(&self) -> (Color, Color) {
        match self {
            SliderVariant::Music => crate::theme::border_3d_accent_raised(),
            SliderVariant::Sfx => crate::theme::border_3d_accent_darker_raised(),
        }
    }
}

/// State for volume slider interaction
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct State {
    is_dragging: bool,
    drag_volume: f32, // Visual drag position (0.0-1.0) - only used during dragging
    last_published_volume: f32, // Last volume value actually sent to audio system
}

/// Custom volume slider with 3D styling (vertical or horizontal)
pub struct VolumeSlider<'a, Message> {
    volume: f32, // 0.0-1.0
    on_change: Box<dyn Fn(f32) -> Message + 'a>,
    on_release: Option<Box<dyn Fn(f32) -> Message + 'a>>,
    on_scroll: Option<Box<dyn Fn(f32) -> Message + 'a>>,
    width: f32,
    height: f32,
    variant: SliderVariant,
    horizontal: bool,
}

impl<'a, Message> VolumeSlider<'a, Message> {
    pub fn new<F>(volume: f32, on_change: F) -> Self
    where
        F: 'a + Fn(f32) -> Message,
    {
        Self {
            volume: volume.clamp(0.0, 1.0),
            on_change: Box::new(on_change),
            on_release: None,
            on_scroll: None,
            width: SLIDER_WIDTH,
            height: SLIDER_HEIGHT,
            variant: SliderVariant::default(),
            horizontal: false,
        }
    }

    /// Set the visual theme variant (Music or SFX)
    pub fn variant(mut self, variant: SliderVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Set the drag-release callback. The release message is emitted with the
    /// final drag value when the user lifts the mouse button (or finger) after
    /// a drag, in addition to the regular `on_change` stream. Use this when
    /// the consumer needs to distinguish "still dragging" from "drag finished"
    /// — e.g. to force-persist the final value past a throttle on `on_change`.
    pub fn on_release<F>(mut self, on_release: F) -> Self
    where
        F: 'a + Fn(f32) -> Message,
    {
        self.on_release = Some(Box::new(on_release));
        self
    }

    /// Set the wheel-scroll callback. The argument passed to the callback is
    /// the **delta** (not the new absolute volume), so the consumer can add it
    /// to fresh app state at message-handling time. This avoids the staleness
    /// trap where computing `self.volume + delta` in the widget uses a snapshot
    /// from the most recent render — two wheel events arriving between renders
    /// otherwise see the same base and silently overwrite each other.
    ///
    /// When unset, wheel events fall back to the absolute computation via
    /// `on_release` (or `on_change` if `on_release` is also unset).
    pub fn on_scroll<F>(mut self, on_scroll: F) -> Self
    where
        F: 'a + Fn(f32) -> Message,
    {
        self.on_scroll = Some(Box::new(on_scroll));
        self
    }

    /// Set horizontal orientation (swaps width/height, uses longer track)
    pub fn horizontal(mut self, horizontal: bool) -> Self {
        if horizontal {
            self.horizontal = true;
            self.width = SLIDER_HORIZONTAL_LENGTH;
            self.height = SLIDER_WIDTH;
        }
        self
    }

    /// Override the cross-axis thickness (height when horizontal, width when vertical).
    /// Used to size stacked horizontal sliders so their combined height matches button height.
    pub fn thickness(mut self, size: f32) -> Self {
        if self.horizontal {
            self.height = size;
        } else {
            self.width = size;
        }
        self
    }

    /// Calculate volume from cursor position.
    /// Vertical: top = 1.0, bottom = 0.0
    /// Horizontal: left = 0.0, right = 1.0
    fn locate(&self, cursor_pos: f32, bounds: Rectangle) -> f32 {
        if self.horizontal {
            let effective_width = bounds.width - HANDLE_SIZE;
            if effective_width <= 0.0 {
                return self.volume;
            }
            let relative_x = cursor_pos - bounds.x - HANDLE_SIZE / 2.0;
            (relative_x / effective_width).clamp(0.0, 1.0)
        } else {
            let effective_height = bounds.height - HANDLE_SIZE;
            if effective_height <= 0.0 {
                return self.volume;
            }
            let relative_y = cursor_pos - bounds.y - HANDLE_SIZE / 2.0;
            // Invert: top=0 becomes volume=1.0, bottom=effective_height becomes volume=0.0
            1.0 - (relative_y / effective_height).clamp(0.0, 1.0)
        }
    }
}

impl<Message: Clone> Widget<Message, Theme, iced::Renderer> for VolumeSlider<'_, Message> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fixed(self.width),
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

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                if let Some(cursor_position) = cursor.position_over(bounds) {
                    let coord = if self.horizontal {
                        cursor_position.x
                    } else {
                        cursor_position.y
                    };
                    let new_volume = self.locate(coord, bounds);
                    state.is_dragging = true;
                    state.drag_volume = new_volume;
                    state.last_published_volume = new_volume;
                    // Publish immediately on click for real-time feedback
                    shell.publish((self.on_change)(new_volume));
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerLifted { .. } | touch::Event::FingerLost { .. })
                if state.is_dragging =>
            {
                // Publish final value (if it drifted from the last 2%-threshold
                // value) so consumers without an on_release callback still see
                // the trailing edge.
                if (state.drag_volume - state.last_published_volume).abs() > 0.001 {
                    shell.publish((self.on_change)(state.drag_volume));
                    state.last_published_volume = state.drag_volume;
                }
                // Emit the dedicated release signal — consumers use this to
                // bypass any throttle on `on_change` and guarantee the final
                // value reaches disk.
                if let Some(ref on_release) = self.on_release {
                    shell.publish(on_release(state.drag_volume));
                }
                state.is_dragging = false;
            }
            Event::Mouse(mouse::Event::CursorMoved { .. })
            | Event::Touch(touch::Event::FingerMoved { .. }) => {
                if state.is_dragging
                    && let Some(pos) = cursor.position()
                {
                    let coord = if self.horizontal { pos.x } else { pos.y };
                    // Update visual position always for smooth feedback
                    let new_volume = self.locate(coord, bounds);
                    state.drag_volume = new_volume;

                    // THROTTLE: Only publish to audio system if change is significant (>= 2%)
                    // This prevents flooding the audio pipeline during fast dragging
                    let delta = (new_volume - state.last_published_volume).abs();
                    if delta >= VOLUME_THROTTLE_THRESHOLD {
                        state.last_published_volume = new_volume;
                        shell.publish((self.on_change)(new_volume));
                    }

                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if cursor.is_over(bounds) => {
                // Calculate scroll delta (positive = up = increase volume)
                let scroll_delta = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => y * 0.01, // 1% per line
                    mouse::ScrollDelta::Pixels { y, .. } => y * 0.001, // Finer control for pixels
                };
                // Prefer the delta-based on_scroll callback when wired — it lets
                // the parent add to fresh app state at handler time. Computing
                // `self.volume + delta` here would use the constructor-captured
                // value, which two rapid wheel events between renders would both
                // see as the same stale base.
                if let Some(ref on_scroll) = self.on_scroll {
                    shell.publish(on_scroll(scroll_delta));
                } else {
                    // Fallback for consumers that haven't wired on_scroll: each
                    // wheel notch is a discrete gesture, so still prefer
                    // on_release (force-persist) over on_change (throttled).
                    let new_volume = (self.volume + scroll_delta).clamp(0.0, 1.0);
                    if let Some(ref on_release) = self.on_release {
                        shell.publish(on_release(new_volume));
                    } else {
                        shell.publish((self.on_change)(new_volume));
                    }
                }
                shell.capture_event();
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
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let border_width = 1.0;

        // Calculate handle position - use drag_volume when dragging for smooth visual feedback
        let volume = if state.is_dragging {
            state.drag_volume
        } else {
            self.volume
        };

        // Bundle all theme colors into a struct to avoid threading 8+ params
        let colors = DrawColors {
            bg1: crate::theme::bg1(),
            track_3d: crate::theme::border_3d_inset(),
            accent: self.variant.accent_color(),
            handle_3d: self.variant.handle_borders(),
            radius: crate::theme::ui_border_radius(),
            is_rounded: crate::theme::is_rounded_mode(),
            shadow: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
        };

        self.draw_oriented(renderer, bounds, volume, border_width, &colors);
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
}

// =============================================================================
// Draw helpers (extracted to keep Widget::draw short)
// =============================================================================

/// Bundles all theme-derived colors and style flags for drawing.
/// Avoids threading 8+ individual color parameters through the draw call chain.
struct DrawColors {
    bg1: Color,
    track_3d: (Color, Color),
    accent: Color,
    handle_3d: (Color, Color),
    radius: iced::border::Radius,
    is_rounded: bool,
    shadow: Color,
}

impl<Message> VolumeSlider<'_, Message> {
    /// Unified draw for both orientations.
    ///
    /// Vertical: handle moves up/down, grip lines are horizontal.
    /// Horizontal: handle moves left/right, grip lines are vertical.
    fn draw_oriented(
        &self,
        renderer: &mut iced::Renderer,
        bounds: Rectangle,
        volume: f32,
        border_width: f32,
        c: &DrawColors,
    ) {
        // Grip reuses handle 3D colors (track/handle colors read from `c` by sub-helpers)
        let (grip_tl, grip_br) = c.handle_3d;

        // 1. Track background
        self.draw_track(renderer, bounds, border_width, c);

        // 2. Handle position + bounds (only difference between orientations)
        let handle_bounds = if self.horizontal {
            let effective = bounds.width - HANDLE_SIZE;
            Rectangle {
                x: bounds.x + volume * effective,
                y: bounds.y,
                width: HANDLE_SIZE,
                height: bounds.height,
            }
        } else {
            let effective = bounds.height - HANDLE_SIZE;
            Rectangle {
                x: bounds.x,
                y: bounds.y + (1.0 - volume) * effective,
                width: bounds.width,
                height: HANDLE_SIZE,
            }
        };
        self.draw_handle(renderer, handle_bounds, border_width, c);

        // 3. Grip — dimensions swap between orientations
        //    Vertical handle gets a wide, short grip; horizontal gets a narrow, tall grip.
        let (gw, gh, gx, gy) = if self.horizontal {
            let gw = 4.0;
            let gh = if c.is_rounded {
                8.0
            } else {
                bounds.height - 6.0
            };
            let gx = handle_bounds.x + (HANDLE_SIZE - gw) / 2.0;
            let gy = bounds.y + (bounds.height - gh) / 2.0;
            (gw, gh, gx, gy)
        } else {
            let gh = 4.0;
            let gw = if c.is_rounded {
                8.0
            } else {
                bounds.width - 6.0
            };
            let gx = bounds.x + (bounds.width - gw) / 2.0;
            let gy = handle_bounds.y + (HANDLE_SIZE - gh) / 2.0;
            (gw, gh, gx, gy)
        };

        if c.is_rounded {
            self.draw_grip_rounded(renderer, gx, gy, gw, gh, c.radius, grip_tl);
        } else {
            self.draw_grip_square(renderer, gx, gy, gw, gh, c.accent, grip_tl, grip_br);
        }
    }

    // ── Shared sub-draw helpers ─────────────────────────────────────────

    fn draw_track(
        &self,
        renderer: &mut iced::Renderer,
        bounds: Rectangle,
        bw: f32,
        c: &DrawColors,
    ) {
        use iced::advanced::Renderer;
        let (tl, br) = c.track_3d;
        if c.is_rounded {
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border: iced::Border {
                        color: tl,
                        width: bw,
                        radius: c.radius,
                    },
                    ..Default::default()
                },
                c.bg1,
            );
        } else {
            // Background fill
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: bounds.x + bw,
                        y: bounds.y + bw,
                        width: bounds.width - bw * 2.0,
                        height: bounds.height - bw * 2.0,
                    },
                    ..Default::default()
                },
                c.bg1,
            );
            self.draw_3d_border(renderer, bounds, bw, tl, br);
        }
    }

    fn draw_handle(&self, renderer: &mut iced::Renderer, hb: Rectangle, bw: f32, c: &DrawColors) {
        use iced::advanced::Renderer;
        let (tl, br) = c.handle_3d;
        if c.is_rounded {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: hb,
                    border: iced::Border {
                        color: tl,
                        width: bw,
                        radius: c.radius,
                    },
                    shadow: Shadow {
                        color: c.shadow,
                        offset: Vector::new(0.0, 1.0),
                        blur_radius: 3.0,
                    },
                    ..Default::default()
                },
                c.accent,
            );
        } else {
            // Shadow
            renderer.fill_quad(
                renderer::Quad {
                    bounds: hb,
                    shadow: Shadow {
                        color: c.shadow,
                        offset: Vector::new(0.0, 1.0),
                        blur_radius: 3.0,
                    },
                    ..Default::default()
                },
                Color::TRANSPARENT,
            );
            // Fill
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: hb.x + bw,
                        y: hb.y + bw,
                        width: hb.width - bw * 2.0,
                        height: hb.height - bw * 2.0,
                    },
                    ..Default::default()
                },
                c.accent,
            );
            self.draw_3d_border(renderer, hb, bw, tl, br);
        }
    }

    /// Draw a 3D raised/inset border (4 edges: top+left = tl color, bottom+right = br color)
    fn draw_3d_border(
        &self,
        renderer: &mut iced::Renderer,
        r: Rectangle,
        bw: f32,
        tl: Color,
        br: Color,
    ) {
        use iced::advanced::Renderer;
        // Top
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x: r.x,
                    y: r.y,
                    width: r.width,
                    height: bw,
                },
                ..Default::default()
            },
            tl,
        );
        // Left
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x: r.x,
                    y: r.y,
                    width: bw,
                    height: r.height,
                },
                ..Default::default()
            },
            tl,
        );
        // Bottom
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x: r.x,
                    y: r.y + r.height - bw,
                    width: r.width,
                    height: bw,
                },
                ..Default::default()
            },
            br,
        );
        // Right
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x: r.x + r.width - bw,
                    y: r.y,
                    width: bw,
                    height: r.height,
                },
                ..Default::default()
            },
            br,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_grip_rounded(
        &self,
        renderer: &mut iced::Renderer,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: iced::border::Radius,
        color: Color,
    ) {
        use iced::advanced::Renderer;
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x,
                    y,
                    width: w,
                    height: h,
                },
                border: iced::Border {
                    radius,
                    ..Default::default()
                },
                ..Default::default()
            },
            color,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_grip_square(
        &self,
        renderer: &mut iced::Renderer,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        accent: Color,
        tl: Color,
        br: Color,
    ) {
        use iced::advanced::Renderer;
        // Center fill
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x: x + 1.0,
                    y: y + 1.0,
                    width: w - 2.0,
                    height: h - 2.0,
                },
                ..Default::default()
            },
            accent,
        );
        self.draw_3d_border(
            renderer,
            Rectangle {
                x,
                y,
                width: w,
                height: h,
            },
            1.0,
            tl,
            br,
        );
    }
}

impl<'a, Message: Clone + 'a> From<VolumeSlider<'a, Message>> for Element<'a, Message> {
    fn from(volume_slider: VolumeSlider<'a, Message>) -> Self {
        Element::new(volume_slider)
    }
}

/// Helper function to create a volume slider (Music variant by default)
pub(crate) fn volume_slider<'a, Message: Clone + 'a>(
    volume: f32,
    on_change: impl Fn(f32) -> Message + 'a,
) -> VolumeSlider<'a, Message> {
    VolumeSlider::new(volume, on_change)
}
