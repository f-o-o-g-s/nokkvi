//! Graphic Equalizer Slider Widget
//!
//! A custom vertical slider for the 10-band EQ. Similar to VolumeSlider but
//! specifically designed for a symmetric [-15.0, +15.0] dB range, with a center
//! detent line at 0 dB.

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

const SLIDER_WIDTH: f32 = 20.0;
const SLIDER_HEIGHT: f32 = 180.0;
const HANDLE_SIZE: f32 = 14.0;
const MAX_DB: f32 = 15.0;
const MIN_DB: f32 = -15.0;
const RANGE_DB: f32 = MAX_DB - MIN_DB;

/// State for eq slider interaction
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct State {
    is_dragging: bool,
    drag_gain: f32, // Visual drag position (-15.0 to +15.0)
    last_published_gain: f32,
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
        let effective_height = bounds.height - HANDLE_SIZE;
        if effective_height <= 0.0 {
            return self.gain;
        }
        let relative_y = cursor_pos - bounds.y - HANDLE_SIZE / 2.0;
        let pct = (relative_y / effective_height).clamp(0.0, 1.0);
        let val = MAX_DB - (pct * RANGE_DB);

        // Snap to exactly 0.0 if within +/- 0.5 dB
        if val.abs() < 0.5 { 0.0 } else { val }
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
                    state.is_dragging = true;
                    state.drag_gain = new_gain;
                    state.last_published_gain = new_gain;
                    shell.publish((self.on_change)(new_gain));
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerLifted { .. } | touch::Event::FingerLost { .. })
                if state.is_dragging => {
                    if (state.drag_gain - state.last_published_gain).abs() > 0.01 {
                        shell.publish((self.on_change)(state.drag_gain));
                        state.last_published_gain = state.drag_gain;
                    }
                    state.is_dragging = false;
                }
            Event::Mouse(mouse::Event::CursorMoved { .. })
            | Event::Touch(touch::Event::FingerMoved { .. }) => {
                if state.is_dragging
                    && let Some(pos) = cursor.position()
                {
                    let new_gain = self.locate(pos.y, bounds);
                    state.drag_gain = new_gain;

                    let delta = (new_gain - state.last_published_gain).abs();
                    if delta >= 0.1 {
                        // Only publish visible changes (e.g. 0.1 dB steps)
                        state.last_published_gain = new_gain;
                        shell.publish((self.on_change)(new_gain));
                    }
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta })
                if cursor.is_over(bounds) => {
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

        let gain = if state.is_dragging {
            state.drag_gain
        } else {
            self.gain
        };

        let is_rounded = crate::theme::is_rounded_mode();
        let radius = crate::theme::ui_border_radius();
        let bg1 = crate::theme::bg1();
        let (tl, br) = crate::theme::border_3d_inset();

        // Use accent for any non-zero gain, muted fg3 for flat — avoids the
        // green/yellow split that reads as success/warning status indicators.
        let accent = if gain.abs() < 0.1 {
            crate::theme::fg3()
        } else {
            crate::theme::accent_bright()
        };

        let (handle_tl, handle_br) = if gain.abs() < 0.1 {
            (crate::theme::fg3(), crate::theme::bg0_hard())
        } else {
            crate::theme::border_3d_accent_raised()
        };

        // 1. Draw track background
        if is_rounded {
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border: iced::Border {
                        color: tl,
                        width: border_width,
                        radius,
                    },
                    ..Default::default()
                },
                bg1,
            );
        } else {
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
            self.draw_3d_border(renderer, bounds, border_width, tl, br);
        }

        // Draw 0 dB center line
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

        // 2. Draw Handle
        let effective_height = bounds.height - HANDLE_SIZE;
        let pct = ((MAX_DB - gain) / RANGE_DB).clamp(0.0, 1.0);
        let hb = Rectangle {
            x: bounds.x,
            y: bounds.y + pct * effective_height,
            width: bounds.width,
            height: HANDLE_SIZE,
        };

        if is_rounded {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: hb,
                    border: iced::Border {
                        color: handle_tl,
                        width: border_width,
                        radius,
                    },
                    shadow: Shadow {
                        color: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
                        offset: Vector::new(0.0, 1.0),
                        blur_radius: 3.0,
                    },
                    ..Default::default()
                },
                accent,
            );
        } else {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: hb,
                    shadow: Shadow {
                        color: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
                        offset: Vector::new(0.0, 1.0),
                        blur_radius: 3.0,
                    },
                    ..Default::default()
                },
                Color::TRANSPARENT,
            );
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: hb.x + border_width,
                        y: hb.y + border_width,
                        width: hb.width - border_width * 2.0,
                        height: hb.height - border_width * 2.0,
                    },
                    ..Default::default()
                },
                accent,
            );
            self.draw_3d_border(renderer, hb, border_width, handle_tl, handle_br);
        }

        // Grip lines
        let gh = 4.0;
        let gw = if is_rounded { 8.0 } else { bounds.width - 6.0 };
        let gx = bounds.x + (bounds.width - gw) / 2.0;
        let gy = hb.y + (HANDLE_SIZE - gh) / 2.0;

        if is_rounded {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: gx,
                        y: gy,
                        width: gw,
                        height: gh,
                    },
                    border: iced::Border {
                        radius,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                handle_tl,
            );
        } else {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: gx + 1.0,
                        y: gy + 1.0,
                        width: gw - 2.0,
                        height: gh - 2.0,
                    },
                    ..Default::default()
                },
                accent,
            );
            self.draw_3d_border(
                renderer,
                Rectangle {
                    x: gx,
                    y: gy,
                    width: gw,
                    height: gh,
                },
                1.0,
                handle_tl,
                handle_br,
            );
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

// Draw 3D border helper
impl<Message> EqSlider<'_, Message> {
    fn draw_3d_border(
        &self,
        renderer: &mut iced::Renderer,
        r: Rectangle,
        bw: f32,
        tl: Color,
        br: Color,
    ) {
        use iced::advanced::Renderer;
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
