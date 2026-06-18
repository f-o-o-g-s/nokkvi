//! Scroll indicator overlay for slot list views
//!
//! Custom widget that overlays a proportionally-sized scrollbar handle on the
//! right edge of the slot list. Behavior is governed by the user's
//! [`ScrollbarVisibility`] setting:
//! - `OnHover`: a transient handle that fades in on hover/scroll and floats over
//!   a Stack — the slot list keeps its full width, no shifting.
//! - `Always` (default): a permanent track + handle that never fades. The caller
//!   reserves a gutter column (right padding) so the bar never floats over content.
//! - `Hidden`: nothing is drawn and no gutter is reserved; mouse-wheel scrolling
//!   still works via the surrounding wheel handler (`wrap_with_scroll`, applied
//!   around this widget's result by the slot-list view builders).

use iced::{
    Alignment, Color, Element, Event, Length, Padding, Point, Rectangle, Size, Theme,
    advanced::{
        Shell,
        layout::{self, Layout},
        renderer,
        widget::{self, Widget},
    },
    mouse, touch,
    widget::{container, stack},
};
use nokkvi_data::types::player_settings::ScrollbarVisibility;

use crate::{theme, widgets::slot_list_view::SlotListView};

/// Base track width in pixels (at reference row height)
const BASE_TRACK_WIDTH: f32 = 16.0;

/// Minimum handle height in pixels (so it stays grabbable)
const MIN_HANDLE_HEIGHT: f32 = 40.0;

/// Maximum handle height as a fraction of track height (prevents giant
/// handles on short lists that barely exceed the viewport)
const MAX_HANDLE_RATIO: f32 = 0.4;

/// Reference row height for scaling — at this height the scrollbar
/// uses the base dimensions. Smaller rows → thinner scrollbar.
const REFERENCE_ROW_HEIGHT: f32 = 80.0;

/// Padding from the right edge of the widget bounds
const RIGHT_PADDING: f32 = 2.0;

/// Widget state for drag interaction
#[derive(Debug, Clone, Copy, Default)]
struct ScrollbarState {
    is_dragging: bool,
    /// Fraction (0.0–1.0) being dragged to — updated live during drag
    drag_fraction: f32,
}

/// Transparent overlay scrollbar widget.
///
/// Fills the entire parent area but only draws a narrow track + handle
/// at the right edge. Interaction (click/drag) is limited to the track area.
struct ScrollbarOverlay<'a, Message> {
    /// Current scroll fraction (0.0 = top, 1.0 = bottom)
    fraction: f32,
    /// Ratio of visible items to total items (determines handle size)
    viewport_ratio: f32,
    /// Visual opacity from scroll-fade timer (0.0–1.0). Ignored in `always` mode.
    opacity: f32,
    /// Scaled track width based on row height
    track_width: f32,
    /// "Always" mode: draw a permanent track behind the handle and force full
    /// opacity (no hover/scroll fade). The caller reserves a matching gutter.
    always: bool,
    /// Whether the list actually overflows the viewport. In `always` mode with
    /// no overflow the track is drawn but the handle is omitted.
    has_overflow: bool,
    /// Callback mapping a fraction (0.0–1.0) to a Message
    on_seek: Box<dyn Fn(f32) -> Message + 'a>,
}

impl<'a, Message> ScrollbarOverlay<'a, Message> {
    fn new(
        fraction: f32,
        viewport_ratio: f32,
        opacity: f32,
        track_width: f32,
        always: bool,
        has_overflow: bool,
        on_seek: impl Fn(f32) -> Message + 'a,
    ) -> Self {
        Self {
            fraction: fraction.clamp(0.0, 1.0),
            viewport_ratio: viewport_ratio.clamp(0.0, 1.0),
            opacity,
            track_width,
            always,
            has_overflow,
            on_seek: Box::new(on_seek),
        }
    }

    /// The track rectangle at the right edge of the widget
    fn track_bounds(&self, bounds: Rectangle) -> Rectangle {
        Rectangle {
            x: bounds.x + bounds.width - self.track_width - RIGHT_PADDING,
            y: bounds.y,
            width: self.track_width,
            height: bounds.height,
        }
    }

    /// Calculate the handle height as a proportion of the track
    fn handle_height(&self, track_height: f32) -> f32 {
        let max_height = track_height * MAX_HANDLE_RATIO;
        (track_height * self.viewport_ratio)
            .min(max_height)
            .max(MIN_HANDLE_HEIGHT)
    }

    /// Calculate the handle Y position within the track
    fn handle_y(&self, track: Rectangle, fraction: f32) -> f32 {
        let handle_h = self.handle_height(track.height);
        let available = track.height - handle_h;
        track.y + fraction * available
    }

    /// Convert a cursor Y position to a scroll fraction
    fn y_to_fraction(&self, cursor_y: f32, track: Rectangle) -> f32 {
        let handle_h = self.handle_height(track.height);
        let available = track.height - handle_h;

        if available <= 0.0 {
            return 0.0;
        }

        let relative_y = cursor_y - track.y - handle_h / 2.0;
        (relative_y / available).clamp(0.0, 1.0)
    }
}

impl<Message: Clone> Widget<Message, Theme, iced::Renderer> for ScrollbarOverlay<'_, Message> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<ScrollbarState>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(ScrollbarState::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Fill,
        }
    }

    fn layout(
        &mut self,
        _tree: &mut widget::Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let size = limits.resolve(Length::Fill, Length::Fill, Size::ZERO);
        layout::Node::new(size)
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
        // Nothing to scroll → the track is purely decorative (only reachable in
        // "Always" mode on a non-overflowing list). Skip drag handling so it
        // isn't clickable. Transient mode always has overflow here.
        if !self.has_overflow {
            return;
        }

        let state = tree.state.downcast_mut::<ScrollbarState>();
        let bounds = layout.bounds();
        let track = self.track_bounds(bounds);

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                // Only start drag if click is on the track area
                if let Some(cursor_position) = cursor.position_over(track) {
                    let new_fraction = self.y_to_fraction(cursor_position.y, track);
                    state.is_dragging = true;
                    state.drag_fraction = new_fraction;
                    shell.publish((self.on_seek)(new_fraction));
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerLifted { .. } | touch::Event::FingerLost { .. })
                if state.is_dragging =>
            {
                shell.publish((self.on_seek)(state.drag_fraction));
                state.is_dragging = false;
            }
            Event::Mouse(mouse::Event::CursorMoved { .. })
            | Event::Touch(touch::Event::FingerMoved { .. }) => {
                if state.is_dragging
                    && let Some(Point { y, .. }) = cursor.position()
                {
                    let new_fraction = self.y_to_fraction(y, track);
                    state.drag_fraction = new_fraction;
                    shell.publish((self.on_seek)(new_fraction));
                    shell.capture_event();
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
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer;

        let state = tree.state.downcast_ref::<ScrollbarState>();
        let bounds = layout.bounds();
        let track = self.track_bounds(bounds);
        let is_hovered_track = cursor.is_over(track) || state.is_dragging;

        // Hover detection: cursor anywhere in the overlay bounds (= slot list area)
        let is_mouse_in_area = cursor.is_over(bounds);

        // Effective opacity. In "Always" mode the bar is permanent → full
        // opacity, no scroll-fade / hover gating. Otherwise it's the max of the
        // scroll-fade opacity and a hover-presence floor (transient style).
        let effective_opacity = if self.always {
            1.0
        } else {
            let hover_opacity = if is_hovered_track {
                0.6
            } else if is_mouse_in_area {
                0.3
            } else {
                0.0
            };
            self.opacity.max(hover_opacity)
        };

        if effective_opacity <= 0.0 {
            return;
        }

        // "Always" mode draws a permanent track/rail behind the handle so the
        // reserved gutter reads as a real scrollbar column. Transient mode keeps
        // the modern handle-only look. `bg3` is a faintly-raised neutral that
        // stays subordinate to the accent handle.
        if self.always {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: track,
                    border: iced::Border {
                        radius: theme::ui_radius_pill(),
                        width: 0.0,
                        color: Color::TRANSPARENT,
                    },
                    ..Default::default()
                },
                Color {
                    a: 0.5,
                    ..theme::bg3()
                },
            );
        }

        // No handle when the list fits the viewport. This is only reachable in
        // "Always" mode (transient mode early-returns upstream when it fits), so
        // the empty track stands in as a "nothing to scroll" affordance.
        if !self.has_overflow {
            return;
        }

        // Use drag fraction during drag for smooth visual feedback
        let fraction = if state.is_dragging {
            state.drag_fraction
        } else {
            self.fraction
        };

        let handle_h = self.handle_height(track.height);
        let handle_y = self.handle_y(track, fraction);

        // --- Handle ---
        let handle_bounds = Rectangle {
            x: track.x + 2.0,
            y: handle_y,
            width: track.width - 4.0,
            height: handle_h,
        };

        // Always use accent-family colors so the handle stays visible over
        // selected (accent_bright) and now-playing (accent) slot backgrounds.
        // Hover/drag: deeper accent for extra contrast; idle: bright accent.
        let handle_base = if is_hovered_track {
            theme::accent()
        } else {
            theme::accent_bright()
        };

        let handle_color = Color {
            a: effective_opacity * if is_hovered_track { 0.9 } else { 0.7 },
            ..handle_base
        };

        // Rounded mode → fully-pill handle; flat mode → square.
        let handle_radius = theme::ui_radius_pill();

        // Use the darkest theme color for a crisp border that pops against any bg
        let border_color = Color {
            a: effective_opacity * 0.8,
            ..theme::bg0_hard()
        };

        renderer.fill_quad(
            renderer::Quad {
                bounds: handle_bounds,
                border: iced::Border {
                    radius: handle_radius,
                    width: 1.0,
                    color: border_color,
                },
                ..Default::default()
            },
            handle_color,
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
        // Decorative track (Always mode, nothing to scroll) → normal cursor.
        if !self.has_overflow {
            return mouse::Interaction::default();
        }

        let state = tree.state.downcast_ref::<ScrollbarState>();
        let track = self.track_bounds(layout.bounds());

        if state.is_dragging {
            mouse::Interaction::Grabbing
        } else if cursor.is_over(track) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
}

impl<'a, Message: Clone + 'a> From<ScrollbarOverlay<'a, Message>> for Element<'a, Message> {
    fn from(widget: ScrollbarOverlay<'a, Message>) -> Self {
        Element::new(widget)
    }
}

/// Wrap a slot list element with a scroll indicator, honoring the user's
/// [`ScrollbarVisibility`] setting.
///
/// Call this on the raw content and wrap the RESULT in the wheel `mouse_area`
/// (`wrap_with_scroll`), not the other way around — the `Always` gutter narrows
/// the content here, so the wheel handler must stay full-width on the outside
/// or it would lose wheel events over the reserved gutter.
///
/// - `OnHover`: a transient overlay handle on the right edge that
///   fades in on hover/scroll. The list keeps its full width (Stack, no shift),
///   and nothing is drawn when the list fits the viewport.
/// - `Always` (default): a permanent track + handle. A gutter the width of the track is
///   reserved on the right (content narrows) so the bar never floats over rows;
///   the track stays drawn even when the list fits (the handle is omitted then).
/// - `Hidden`: returns `inner` untouched — no bar, no gutter. Mouse-wheel
///   scrolling still works via the surrounding wheel handler.
///
/// The handle height is proportional to the visible viewport
/// (`slot_count / total_items`), and styling uses the app's theme colors.
///
/// # Arguments
/// * `inner` – The slot list Element to wrap
/// * `sl` – SlotListView state for computing position and opacity
/// * `total_items` – Total number of items in the filtered list
/// * `row_height` – Current row height for scaling the track width
/// * `on_seek` – Callback mapping a fractional position (0.0 = top, 1.0 = bottom) to a Message
pub(crate) fn wrap_with_scroll_indicator<'a, Message: Clone + 'a>(
    inner: Element<'a, Message>,
    sl: &SlotListView,
    total_items: usize,
    row_height: f32,
    on_seek: impl Fn(f32) -> Message + 'a,
) -> Element<'a, Message> {
    let mode = theme::scrollbar_visibility();

    // Hidden: never draw a bar and reserve no gutter (full width retained).
    if matches!(mode, ScrollbarVisibility::Hidden) {
        return inner;
    }

    // A truly empty list has nothing to show or reserve a column for, in any
    // mode — fall back to plain content (full width, no track).
    if total_items == 0 {
        return inner;
    }

    let always = matches!(mode, ScrollbarVisibility::Always);
    // `total_items > 0` is guaranteed by the early-return above, so a bare
    // `>` against the unsigned `slot_count` is sufficient.
    let has_overflow = total_items > sl.slot_count;

    // Transient mode draws nothing when the list fits the viewport. Always mode
    // keeps the reserved gutter + track even when it fits, so the column doesn't
    // appear/disappear as the list crosses the overflow threshold.
    if !always && !has_overflow {
        return inner;
    }

    let opacity = if always { 1.0 } else { sl.scrollbar_opacity() };

    let current_fraction = if has_overflow && total_items > 1 {
        sl.viewport_offset as f32 / (total_items - 1) as f32
    } else {
        0.0
    };

    // Visible/total ratio drives the handle height; 1.0 when the list fits so
    // the handle math stays well-defined (the handle isn't drawn in that case).
    let viewport_ratio = if has_overflow {
        sl.slot_count as f32 / total_items as f32
    } else {
        1.0
    };

    // Scale scrollbar dimensions relative to row height
    let scale = (row_height / REFERENCE_ROW_HEIGHT).clamp(0.5, 2.0);
    let track_width = (BASE_TRACK_WIDTH * scale).round().max(6.0);

    let scrollbar: Element<'a, Message> = ScrollbarOverlay::new(
        current_fraction,
        viewport_ratio,
        opacity,
        track_width,
        always,
        has_overflow,
        on_seek,
    )
    .into();

    let overlay = container(scrollbar)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::End);

    if always {
        // Reserve a gutter so the permanent track never floats over content.
        // The track sits at `[W - track_width - RIGHT_PADDING, W - RIGHT_PADDING]`;
        // pad the content's right edge to the track's left edge (+2px breathing
        // room) so rows end just before the bar.
        let gutter = track_width + RIGHT_PADDING + 2.0;
        let reserved = container(inner).padding(Padding {
            top: 0.0,
            right: gutter,
            bottom: 0.0,
            left: 0.0,
        });
        stack![reserved, overlay]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        // Transient: float over content (no reserved width).
        stack![inner, overlay]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
