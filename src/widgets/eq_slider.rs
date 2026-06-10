//! Graphic Equalizer Slider Widget
//!
//! A custom vertical slider for the 10-band EQ. Similar to VolumeSlider but
//! specifically designed for a symmetric [-15.0, +15.0] dB range, with a center
//! detent line at 0 dB.

use iced::{
    Element, Event, Length, Rectangle, Size, Theme,
    advanced::{
        Shell,
        layout::{self, Layout},
        renderer,
        widget::{self, Widget},
    },
    mouse, touch,
};

use crate::widgets::slider_drag::{self, Axis, SliderDragState};

const SLIDER_WIDTH: f32 = 20.0;
const SLIDER_HEIGHT: f32 = 180.0;
const HANDLE_SIZE: f32 = 14.0;
const MAX_DB: f32 = 15.0;
const MIN_DB: f32 = -15.0;
const RANGE_DB: f32 = MAX_DB - MIN_DB;

/// State for eq slider interaction
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct State {
    drag: SliderDragState,
}

pub struct EqSlider<'a, Message> {
    gain: f32,
    on_change: Box<dyn Fn(f32) -> Message + 'a>,
    width: f32,
    height: f32,
}

impl<'a, Message> EqSlider<'a, Message> {
    pub fn new<F>(gain: f32, on_change: F) -> Self
    where
        F: 'a + Fn(f32) -> Message,
    {
        Self {
            gain: gain.clamp(MIN_DB, MAX_DB),
            on_change: Box::new(on_change),
            width: SLIDER_WIDTH,
            height: SLIDER_HEIGHT,
        }
    }

    fn locate(&self, cursor_pos: f32, bounds: Rectangle) -> f32 {
        slider_drag::project_fraction(cursor_pos, bounds, HANDLE_SIZE, Axis::Vertical).map_or(
            self.gain,
            |pct| {
                let val = MAX_DB - (pct * RANGE_DB);
                // Snap to exactly 0.0 if within +/- 0.5 dB
                if val.abs() < 0.5 { 0.0 } else { val }
            },
        )
    }
}

impl<Message: Clone> Widget<Message, Theme, iced::Renderer> for EqSlider<'_, Message> {
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
                    let new_gain = self.locate(cursor_position.y, bounds);
                    shell.publish((self.on_change)(state.drag.press(new_gain)));
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerLifted { .. } | touch::Event::FingerLost { .. })
                if state.drag.is_dragging() =>
            {
                // Trailing publish when the visual value drifted past the
                // last published one. NO capture/redraw on release — only
                // the settings slider does that.
                if let Some(trailing) = state.drag.release(0.01) {
                    shell.publish((self.on_change)(trailing));
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. })
            | Event::Touch(touch::Event::FingerMoved { .. }) => {
                if state.drag.is_dragging()
                    && let Some(pos) = cursor.position()
                {
                    let new_gain = self.locate(pos.y, bounds);
                    // Only publish visible changes (e.g. 0.1 dB steps); the
                    // visual drag position still updates on every move.
                    if let Some(value) = state.drag.drag(new_gain, 0.1) {
                        shell.publish((self.on_change)(value));
                    }
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if cursor.is_over(bounds) => {
                let scroll_delta = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => y * 1.0, // 1 dB per line
                    mouse::ScrollDelta::Pixels { y, .. } => y * 0.1, // 0.1 dB per pixel
                };
                let raw_gain = (self.gain + scroll_delta).clamp(MIN_DB, MAX_DB);
                let new_gain = if raw_gain.abs() < 0.25 { 0.0 } else { raw_gain };
                shell.publish((self.on_change)(new_gain));
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
        use iced::advanced::Renderer;

        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let border_width = 1.0;

        let gain = state.drag.display_value(self.gain);

        // Flat redesign: 1 px border() track, flat accent fill on the
        // handle. Handle picks up `pill` shape in rounded mode so the EQ
        // band visually pairs with the volume / progress sliders.
        let track_radius = crate::theme::ui_radius_pill();
        let handle_radius = crate::theme::ui_radius_pill();

        // Use accent for any non-zero gain, muted fg3 at the detent so the
        // user sees that 0 dB is the neutral position without it suddenly
        // changing color category.
        let accent = if gain.abs() < 0.1 {
            crate::theme::fg3()
        } else {
            crate::theme::accent_bright()
        };

        // 1. Draw track background — single flat fill + 1 px border().
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: iced::Border {
                    color: crate::theme::border(),
                    width: border_width,
                    radius: track_radius,
                },
                ..Default::default()
            },
            crate::theme::bg1(),
        );

        // Draw 0 dB center line (kept — visual detent that helps users
        // align bands to neutral).
        let center_y = bounds.y + bounds.height / 2.0;
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x: bounds.x + 2.0,
                    y: center_y - 1.0,
                    width: bounds.width - 4.0,
                    height: 2.0,
                },
                ..Default::default()
            },
            crate::theme::fg4(),
        );

        // 2. Draw Handle — flat accent fill + 1 px border() outline. The
        // 3D grip lines from the legacy bevel renderer are removed; the
        // handle reads as a single coloured pill against the track.
        let effective_height = bounds.height - HANDLE_SIZE;
        let pct = ((MAX_DB - gain) / RANGE_DB).clamp(0.0, 1.0);
        let hb = Rectangle {
            x: bounds.x,
            y: bounds.y + pct * effective_height,
            width: bounds.width,
            height: HANDLE_SIZE,
        };

        renderer.fill_quad(
            renderer::Quad {
                bounds: hb,
                border: iced::Border {
                    color: crate::theme::border(),
                    width: border_width,
                    radius: handle_radius,
                },
                ..Default::default()
            },
            accent,
        );
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
        slider_drag::grab_interaction(state.drag.is_dragging(), cursor.is_over(layout.bounds()))
    }
}

impl<'a, Message: Clone + 'a> From<EqSlider<'a, Message>> for Element<'a, Message> {
    fn from(slider: EqSlider<'a, Message>) -> Self {
        Element::new(slider)
    }
}

pub(crate) fn eq_slider<'a, Message: Clone + 'a>(
    gain: f32,
    on_change: impl Fn(f32) -> Message + 'a,
) -> EqSlider<'a, Message> {
    EqSlider::new(gain, on_change)
}
