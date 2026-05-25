//! Unified Volume Slider Widget with flat styling and drag-to-adjust
//!
//! A single parameterized widget for both main volume and SFX volume controls.
//! Flat-design rendering:
//! - Vertical: stereo 2-channel bars (each 8×44, 2 px gap = 18 px total).
//!   1 px `theme::border()` outline, `theme::bg0()` track, accent-bright fill
//!   from the bottom up to the current volume level. Channels are decorative
//!   (both render the same value) — the widget remains a single-volume control.
//! - Horizontal: 6 px thin track with a 14 px handle (matches progress bar).
//! - Variant determines accent color (Music = bright accent, SFX = base accent).

use iced::{
    Color, Element, Event, Length, Rectangle, Size, Theme,
    advanced::{
        Shell,
        layout::{self, Layout},
        renderer,
        widget::{self, Widget},
    },
    mouse, touch,
};

// ───────────────────────── dimensions ─────────────────────────
// Vertical (stereo bars): 8 px channel + 2 px gap + 8 px channel = 18 px wide,
// 44 px tall (matches mode-button height).
const VERTICAL_WIDTH: f32 = 18.0;
const VERTICAL_HEIGHT: f32 = 44.0;
const STEREO_CHANNEL_WIDTH: f32 = 8.0;
const STEREO_CHANNEL_GAP: f32 = 2.0;

// Horizontal: 6 px thin track, 55 px long (1.25× vertical for drag range),
// 14 px handle (matches progress bar).
const HORIZONTAL_LENGTH: f32 = 55.0;
const HORIZONTAL_TRACK_THICKNESS: f32 = 6.0;
const HORIZONTAL_HANDLE_SIZE: f32 = 14.0;

/// Minimum volume change to trigger an update during dragging.
/// This prevents flooding the audio system with redundant volume commands.
const VOLUME_THROTTLE_THRESHOLD: f32 = 0.02; // 2% change required

/// Visual theme variant for the volume slider
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SliderVariant {
    /// Bright accent for main music volume
    #[default]
    Music,
    /// Darker accent for sound effects volume
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
}

/// State for volume slider interaction
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct State {
    is_dragging: bool,
    drag_volume: f32, // Visual drag position (0.0-1.0) - only used during dragging
    last_published_volume: f32, // Last volume value actually sent to audio system
}

/// Custom volume slider with flat styling (vertical stereo bars or horizontal track)
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
            width: VERTICAL_WIDTH,
            height: VERTICAL_HEIGHT,
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
            self.width = HORIZONTAL_LENGTH;
            self.height = VERTICAL_WIDTH;
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
            let effective_width = bounds.width - HORIZONTAL_HANDLE_SIZE;
            if effective_width <= 0.0 {
                return self.volume;
            }
            let relative_x = cursor_pos - bounds.x - HORIZONTAL_HANDLE_SIZE / 2.0;
            (relative_x / effective_width).clamp(0.0, 1.0)
        } else {
            // Vertical: full bar height maps 0..1 (no handle inset — the fill
            // is a level meter, not a draggable handle).
            if bounds.height <= 0.0 {
                return self.volume;
            }
            let relative_y = cursor_pos - bounds.y;
            // Invert: top=0 becomes volume=1.0, bottom=height becomes volume=0.0
            1.0 - (relative_y / bounds.height).clamp(0.0, 1.0)
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

        // Calculate level - use drag_volume when dragging for smooth visual feedback
        let volume = if state.is_dragging {
            state.drag_volume
        } else {
            self.volume
        };

        let accent = self.variant.accent_color();
        let border = crate::theme::border();
        let track_bg = crate::theme::bg0();

        if self.horizontal {
            self.draw_horizontal(renderer, bounds, volume, accent, border, track_bg);
        } else {
            self.draw_stereo_vertical(renderer, bounds, volume, accent, border, track_bg);
        }
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
// Draw helpers (flat redesign)
// =============================================================================

impl<Message> VolumeSlider<'_, Message> {
    /// Vertical stereo meter: two narrow channel bars side-by-side, each with
    /// a 1 px border on the bg0 track and an accent fill rising from the bottom
    /// to the current volume level. In rounded mode the channels become pills.
    fn draw_stereo_vertical(
        &self,
        renderer: &mut iced::Renderer,
        bounds: Rectangle,
        volume: f32,
        accent: Color,
        border: Color,
        track_bg: Color,
    ) {
        use iced::advanced::Renderer;

        let radius = if crate::theme::is_rounded_mode() {
            crate::theme::ui_radius_pill()
        } else {
            iced::border::Radius::from(0.0)
        };

        // Both channels render identical levels — the stereo split is purely
        // cosmetic per the design spec; the slider remains a single-volume
        // control.
        // Compute the total stereo cluster width once and center it inside the
        // widget bounds so external `thickness()` overrides don't squash the
        // bars off-center.
        let cluster_width = STEREO_CHANNEL_WIDTH * 2.0 + STEREO_CHANNEL_GAP;
        let cluster_x = bounds.x + (bounds.width - cluster_width) / 2.0;

        for ch in 0..2 {
            let ch_x = cluster_x + (STEREO_CHANNEL_WIDTH + STEREO_CHANNEL_GAP) * ch as f32;
            let ch_bounds = Rectangle {
                x: ch_x,
                y: bounds.y,
                width: STEREO_CHANNEL_WIDTH,
                height: bounds.height,
            };

            // Track (outlined, bg0 fill).
            renderer.fill_quad(
                renderer::Quad {
                    bounds: ch_bounds,
                    border: iced::Border {
                        color: border,
                        width: 1.0,
                        radius,
                    },
                    ..Default::default()
                },
                track_bg,
            );

            // Fill from bottom up to the current level. Inset by 1 px on each
            // side so the fill sits inside the border.
            let inner_height = (bounds.height - 2.0).max(0.0);
            let fill_height = (inner_height * volume).clamp(0.0, inner_height);
            if fill_height > 0.0 {
                let fill_y = ch_bounds.y + 1.0 + (inner_height - fill_height);
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: ch_bounds.x + 1.0,
                            y: fill_y,
                            width: STEREO_CHANNEL_WIDTH - 2.0,
                            height: fill_height,
                        },
                        border: iced::Border {
                            radius,
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    accent,
                );
            }
        }
    }

    /// Horizontal track + handle, matching the progress bar's flat style.
    fn draw_horizontal(
        &self,
        renderer: &mut iced::Renderer,
        bounds: Rectangle,
        volume: f32,
        accent: Color,
        border: Color,
        track_bg: Color,
    ) {
        use iced::advanced::Renderer;

        let radius = if crate::theme::is_rounded_mode() {
            crate::theme::ui_radius_pill()
        } else {
            iced::border::Radius::from(0.0)
        };

        // Track centered vertically within widget bounds.
        let track_y = bounds.y + (bounds.height - HORIZONTAL_TRACK_THICKNESS) / 2.0;
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x: bounds.x,
                    y: track_y,
                    width: bounds.width,
                    height: HORIZONTAL_TRACK_THICKNESS,
                },
                border: iced::Border {
                    radius,
                    ..Default::default()
                },
                ..Default::default()
            },
            track_bg,
        );

        // Handle (square in flat / pill in rounded).
        let effective = bounds.width - HORIZONTAL_HANDLE_SIZE;
        let handle_x = bounds.x + volume * effective.max(0.0);
        let handle_y = bounds.y + (bounds.height - HORIZONTAL_HANDLE_SIZE) / 2.0;
        let handle_bounds = Rectangle {
            x: handle_x,
            y: handle_y,
            width: HORIZONTAL_HANDLE_SIZE,
            height: HORIZONTAL_HANDLE_SIZE,
        };

        // Fill from left edge up to the handle's center.
        let fill_width =
            (handle_x - bounds.x + HORIZONTAL_HANDLE_SIZE / 2.0).clamp(0.0, bounds.width);
        if fill_width > 0.0 {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: bounds.x,
                        y: track_y,
                        width: fill_width,
                        height: HORIZONTAL_TRACK_THICKNESS,
                    },
                    border: iced::Border {
                        radius,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                accent,
            );
        }

        renderer.fill_quad(
            renderer::Quad {
                bounds: handle_bounds,
                border: iced::Border {
                    color: border,
                    width: 1.0,
                    radius,
                },
                ..Default::default()
            },
            accent,
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
