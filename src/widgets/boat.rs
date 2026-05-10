//! Sailing-boat overlay for the lines-mode visualizer.
//!
//! Pure CPU helpers + a tiny stateful struct that the root TEA owns. The
//! widget does not touch the WGSL pipeline or the FFT thread — it only reads
//! the bar buffer (`VisualizerState::get_bars()`) the shader already consumes,
//! resamples it via the same Catmull-Rom basis (`catmull_rom_1d`) the lines
//! shader uses, and rides on top of the rendered waveform.
//!
//! The model borrows the "always-forward, never-stalled" trick from Tiny
//! Wings (`reference-tiny-wings/Hero.mm`'s `minVelocityX`): a constant sail
//! thrust along `facing` plus a hard forward-velocity floor that no wave
//! shape can overcome. Wave heights still affect the boat — they just do so
//! through tilt and buoyancy rather than by blocking horizontal travel.
//!
//! - **Sail thrust** — `facing · MAX_SAIL_THRUST · total_intensity`.
//!   Music intensity (cruise + beat + onset, all in `[0, 1]`) is the
//!   sole driver of forward motion: silence produces zero thrust and
//!   the boat coasts to a stop. There is no constant baseline; the
//!   boat is propelled entirely by what's playing.
//! - **Forward-velocity floor** — after every integration, `x_velocity` is
//!   reasserted to at least `MIN_SAILING_VELOCITY` in the facing direction.
//!   This single clamp is what guarantees the boat clears every wave: no
//!   slope, damping, or numerical extreme can drop forward speed below the
//!   floor.
//! - **Slope force** — the local wave gradient at the boat's x position
//!   pushes it downhill (positive slope → push left, negative → push right),
//!   capped at `MAX_SLOPE_FORCE`. Sized below `MAX_SAIL_THRUST` so an uphill
//!   slows the boat noticeably without ever reversing it. Gated to zero in
//!   low-amplitude regions so the boat doesn't drift on calm water.
//! - **Velocity damping** — friction on `x_velocity` gives the "floating"
//!   feel; the boat lags fast wave changes instead of snapping to them.
//! - **Tack events** — every `[TACK_INTERVAL_MIN_SECS,
//!   TACK_INTERVAL_MAX_SECS]` seconds the wind shifts: `facing` flips and
//!   the boat sails the other way. The countdown only ticks down in the
//!   visible area so a tack can't fire while the boat is mid-margin
//!   (where it would briefly disagree with the latched eject direction).
//! - **Y dynamics** — `y_ratio` follows the sampled wave height through a
//!   spring-damper rather than tracking it exactly, so the boat bobs with
//!   buoyancy rather than gluing to the curve. This is the half of the
//!   wave interaction that actually carries the boat (vertically), even
//!   while sail thrust handles horizontal travel.
//! - **Tilt** — a spring-damper toward `-slope · TILT_GAIN`, capped at
//!   `MAX_TILT`. Independent of facing because the SVG mirrors when
//!   facing flips, so "uphill on the right = bow-up" works for both sides.
//! - **Toroidal X wrap with off-screen margin** — `x_ratio` lives in
//!   `[-x_wrap_margin, 1 + x_wrap_margin)` and wraps via `rem_euclid` over
//!   that extended span; `x_velocity` is preserved across the seam. The
//!   margin is sized in the handler from the live boat sprite width
//!   (`BOAT_WRAP_MARGIN_BOAT_WIDTHS · boat_w / area_width`) so the boat
//!   fully exits the visible area before wrapping — the renderer draws a
//!   single copy at `target_x` and lets the outer clip trim the off-screen
//!   portion, so the boat is never visible in two places at once.
//! - **Margin deadspace** — while in the margin, slope force is muted
//!   so toroidal "across the seam" gradients can't drag the boat back
//!   into the edge it just left. No special force is applied; sail
//!   thrust + the velocity floor carry the boat through the margin at
//!   terminal velocity in its facing direction. (An earlier revision
//!   used a constant `EJECT_FORCE` here as a stall-prevention; with the
//!   floor in place it became redundant and was removed.)
//!
//! `BoatState.tilt_handles` caches themed boat SVGs lazily on first use,
//! keyed by quantized `(tilt, facing)`. `Handle::from_memory` re-hashes
//! input bytes per call (see `reference-iced/core/src/svg.rs:89`), so
//! per-frame construction would churn GPU cache keys — the same class of
//! bug as the `image::Handle::from_path` gotcha called out in `CLAUDE.md`.

#[path = "boat_physics.rs"]
mod boat_physics;
pub(crate) use boat_physics::{
    ANCHOR_HEIGHT_MULTIPLE_OF_BOAT, BOAT_SINK_FRACTION, BOAT_WRAP_MARGIN_BOAT_WIDTHS, BoatState,
    MusicSignals, boat_pixel_size, effective_bars, rope_stroke_for, step,
};

use iced::{
    Color, Element, Event, Length, Point, Rectangle, Size, Vector,
    advanced::{
        Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{Operation, Tree},
    },
    widget::{Stack, Svg, canvas, container, svg},
};

/// Position a child element at an arbitrary `(x, y)` (including negative
/// coordinates) inside a parent without shrinking the child.
///
/// `iced::widget::Pin` does almost the right thing — it accepts negative
/// coordinates and respects the parent clip — but it computes the child's
/// available layout space as `parent_max - position`, which silently
/// squashes a `Length::Fixed`-sized child as `position` approaches the
/// parent's far edge (`Length::Fixed(40)` with available `20` clamps to
/// `20` via `Limits::width()` at `core/src/layout/limits.rs:57`). For the
/// boat that produces a visible "shrinking ship" artifact at the wrap
/// seam. `OverflowPin` instead passes the parent's full limits through to
/// the child, then translates the laid-out node — the child keeps its
/// natural size and any portion that falls outside the parent is trimmed
/// by the ancestor clip in `draw()`.
struct OverflowPin<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer>
where
    Renderer: iced::advanced::Renderer,
{
    content: Element<'a, Message, Theme, Renderer>,
    position: Point,
}

impl<'a, Message, Theme, Renderer> OverflowPin<'a, Message, Theme, Renderer>
where
    Renderer: iced::advanced::Renderer,
{
    fn new(content: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        Self {
            content: content.into(),
            position: Point::ORIGIN,
        }
    }

    fn position(mut self, position: Point) -> Self {
        self.position = position;
        self
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for OverflowPin<'_, Message, Theme, Renderer>
where
    Renderer: iced::advanced::Renderer,
{
    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> iced::advanced::widget::tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }

    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Fill,
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let node = self
            .content
            .as_widget_mut()
            .layout(tree, renderer, limits)
            .move_to(self.position);

        let size = limits.resolve(Length::Fill, Length::Fill, node.size());
        layout::Node::with_children(size, vec![node])
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content.as_widget_mut().operate(
            tree,
            layout
                .children()
                .next()
                .expect("OverflowPin always lays out exactly one child"),
            renderer,
            operation,
        );
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            tree,
            event,
            layout
                .children()
                .next()
                .expect("OverflowPin always lays out exactly one child"),
            cursor,
            renderer,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            tree,
            layout
                .children()
                .next()
                .expect("OverflowPin always lays out exactly one child"),
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        if let Some(clipped_viewport) = bounds.intersection(viewport) {
            self.content.as_widget().draw(
                tree,
                renderer,
                theme,
                style,
                layout
                    .children()
                    .next()
                    .expect("OverflowPin always lays out exactly one child"),
                cursor,
                &clipped_viewport,
            );
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            tree,
            layout
                .children()
                .next()
                .expect("OverflowPin always lays out exactly one child"),
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message, Theme, Renderer> From<OverflowPin<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: iced::advanced::Renderer + 'a,
{
    fn from(p: OverflowPin<'a, Message, Theme, Renderer>) -> Self {
        Element::new(p)
    }
}

/// Build the boat overlay element. Returned as a plain `Element` (not
/// `Option<Element>`) so the visibility branch lives at the call site —
/// `Stack::push` expects `impl Into<Element>`.
///
/// `area_width` / `area_height` are the pixel dimensions of the visualizer
/// area the boat rides over. They size the outer clipping container and let
/// us compute the boat's pixel position from `(x_ratio, y_ratio)`.
///
/// Layout: a fixed-size `container.clip(true)` framing the visualizer area,
/// containing a single `OverflowPin`-positioned boat sprite at
/// `(target_x, target_y)`. `target_x` may extend past either edge by up to
/// `BOAT_WRAP_MARGIN_BOAT_WIDTHS · boat_w` — the physics in `step()` wrap
/// only after the boat has fully cleared the visible area, so a second
/// "ghost" copy at `target_x ± area_width` is unnecessary (and would put
/// the boat in two places at once during the off-screen drift). The outer
/// clip handles the fade-out as the sprite slides off; the next visible
/// frame on the opposite edge picks it up after the wrap fires.
///
/// Tilt and facing are read straight from `state`. The boat picks its
/// cached SVG handle via `cached_handle_for(tilt, facing)`, which returns
/// a handle whose path data has the rotation (and optional horizontal
/// mirror) *baked into the SVG itself*. resvg then rasterizes the
/// already-rotated paths at the boat's display resolution — much cleaner
/// than letting iced rasterize an upright sprite and then rotate the
/// bitmap in the wgpu shader, which aliases visibly at small sprite
/// sizes. The tilt is quantized to `TILT_QUANT_DEG`-degree steps so the
/// underlying cache stays bounded.
///
/// `OverflowPin` (defined just above) is used instead of `iced::widget::pin`
/// because the stock `Pin` shrinks `Length::Fixed`-sized content as the
/// position approaches the parent's far edge (silently squashing the
/// boat). `OverflowPin` lets the boat keep its real size and trims the
/// off-screen portion via the ancestor clip in its `draw()` path.
///
/// Why not `Float`: `iced::widget::Float` renders translated content via an
/// overlay layer (`reference-iced/widget/src/float.rs:204-244`) that calls
/// `renderer.with_layer(self.viewport, ...)` with the full window viewport,
/// so a parent `container.clip(true)` is silently ignored and the boat
/// would draw over neighbouring overlays (the player bar).
pub(crate) fn boat_overlay<'a, M: 'a>(
    state: &BoatState,
    area_width: f32,
    area_height: f32,
) -> Element<'a, M> {
    // The handler is responsible for calling `cache_handle_for(tilt,
    // facing)` on the first visible tick, so by the time we render the
    // matching handle is cached. The fallback rebuilds inline if a render
    // somehow precedes the tick OR if the theme just changed and the next
    // BoatTick hasn't refreshed the cache yet — in either case we ship a
    // fresh-rotation, fresh-color frame rather than a stale one.
    let handle = state
        .cached_handle_for(state.tilt, state.facing)
        .unwrap_or_else(|| {
            let bytes =
                crate::embedded_svg::themed_boat_svg(state.tilt, state.facing < 0.0).into_bytes();
            svg::Handle::from_memory(bytes)
        });
    let (boat_w, boat_h) = boat_pixel_size(area_height);

    // The boat SVG carries a padded viewBox so a `MAX_TILT` rotation
    // doesn't clip the rotated bounding box's corners. Scale the iced
    // container by the matching factor so the boat *content* still
    // renders at `boat_w × boat_h` pixels — `pad_x`/`pad_y` is the
    // half-padding the rotated corners can occupy on each side.
    let pad_factor = 1.0 + 2.0 * crate::embedded_svg::BOAT_VIEWBOX_PAD_FRACTION;
    let container_w = boat_w * pad_factor;
    let container_h = boat_h * pad_factor;
    let pad_x = (container_w - boat_w) * 0.5;
    let pad_y = (container_h - boat_h) * 0.5;

    // Pixel offsets within the visualizer area. The waterline is
    // `(1 - y_ratio) * area_height` from the top (visualizer draws upward
    // from the bottom). `BOAT_SINK_FRACTION` of the boat's height sits below
    // the waterline; the rest sits above. `OverflowPin` accepts negative
    // `target_x` directly, so we don't clamp it here — the outer clip
    // handles the off-screen portion. `target_y` keeps its `.max(0.0)`
    // because Y has no wrap (wave height is bounded), so the overlap-above
    // case really is just "nudge against the top edge".
    //
    // `target_x` / `target_y` describe where the boat *content* lands.
    // The container is shifted left/up by the half-padding so the content
    // remains at those coordinates regardless of the surrounding margin.
    let cx = state.x_ratio * area_width;
    let target_x = cx - boat_w * 0.5;
    let line_y = area_height * (1.0 - state.y_ratio);
    let target_y = (line_y - boat_h + boat_h * BOAT_SINK_FRACTION).max(0.0);

    let pin_at = |x: f32| {
        OverflowPin::new(
            container(
                Svg::new(handle.clone())
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .width(Length::Fixed(container_w))
            .height(Length::Fixed(container_h)),
        )
        .position(Point::new(x - pad_x, target_y - pad_y))
    };

    // Single sprite. The wrap zone in `step()` is sized so the boat fully
    // exits the visible area before reappearing on the opposite side, so
    // there is never a frame where two copies would be on screen at once
    // — outer clip handles the off-screen portion of the sprite as it
    // slides through the hidden stretch.
    let mut overlay = Stack::new().push(pin_at(target_x));

    // Anchor sprite + rope canvas: only rendered while anchored. The
    // anchor sprite sits at the bottom of the visualizer area, pinned
    // to the x where the boat dropped the anchor — it does NOT move
    // with the boat, so wave-driven Y-bobbing leaves the anchor
    // planted on the floor while the boat rides above it. The rope is
    // a curved canvas path drawn from the boat's bottom-center to the
    // top of the anchor's ring; its bend is driven by `anchor_sway`,
    // which the physics still oscillates from local wave amplitude.
    if state.anchor_remaining_secs > 0.0 {
        let anchor_handle = state.cached_anchor_handle().unwrap_or_else(|| {
            let bytes = crate::embedded_svg::themed_anchor_svg().into_bytes();
            svg::Handle::from_memory(bytes)
        });

        // Anchor sprite sized as a fraction of the boat — small enough
        // that it reads as a doodad rather than a second focal point.
        let anchor_total_h = boat_h * ANCHOR_HEIGHT_MULTIPLE_OF_BOAT;
        let anchor_total_w = anchor_total_h; // lucide anchor's viewBox is square
        let anchor_left_x = state.anchor_drop_x * area_width - anchor_total_w * 0.5;
        let anchor_top_y = area_height - anchor_total_h;

        overlay = overlay.push(
            OverflowPin::new(
                container(
                    Svg::new(anchor_handle)
                        .width(Length::Fill)
                        .height(Length::Fill),
                )
                .width(Length::Fixed(anchor_total_w))
                .height(Length::Fixed(anchor_total_h)),
            )
            .position(Point::new(anchor_left_x, anchor_top_y)),
        );

        // Rope canvas: draws a single quadratic Bezier from the boat's
        // bottom-center to the top of the anchor's ring. The control
        // point sits at the rope's midpoint, offset perpendicular to
        // the rope axis by `anchor_sway · rope_length` so the bend
        // amplitude scales with the rope's current length (longer rope
        // = bigger swing arc, which reads more naturally than a fixed
        // pixel offset on a stretched line).
        let viz_colors = crate::theme::get_visualizer_colors();
        let rope_color =
            parse_hex_color(&viz_colors.border_color).unwrap_or(Color::from_rgb(0.5, 0.5, 0.5));
        let rope_alpha = viz_colors.border_opacity;

        let boat_bottom_x = cx;
        let boat_bottom_y = target_y + boat_h - boat_h * BOAT_SINK_FRACTION;
        let anchor_ring_x = state.anchor_drop_x * area_width;
        let anchor_ring_y =
            anchor_top_y + anchor_total_h * crate::embedded_svg::anchor_svg_ring_top_fraction();

        let rope = RopeCanvas {
            start: Point::new(boat_bottom_x, boat_bottom_y),
            end: Point::new(anchor_ring_x, anchor_ring_y),
            sway: state.anchor_sway,
            stroke_color: Color {
                a: rope_alpha,
                ..rope_color
            },
            stroke_width: rope_stroke_for(boat_h),
        };
        overlay = overlay.push(
            canvas::Canvas::new(rope)
                .width(Length::Fixed(area_width))
                .height(Length::Fixed(area_height)),
        );
    }

    container(overlay)
        .width(Length::Fixed(area_width))
        .height(Length::Fixed(area_height))
        .clip(true)
        .into()
}

/// Parse a `#rrggbb` hex color string into an `iced::Color`. Returns
/// `None` if the string isn't a 7-character hex form. The visualizer
/// theme's `border_color` is always emitted in this form by
/// `embedded_svg::color_to_hex`, so this is the inverse — used by the
/// rope canvas to translate a string-formatted theme color back into
/// an iced color for `Stroke`.
fn parse_hex_color(s: &str) -> Option<Color> {
    let s = s.strip_prefix('#')?;
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some(Color::from_rgb(
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
    ))
}

/// Canvas program for the anchor rope. Draws a single quadratic Bezier
/// from `start` to `end`, with the control point at the midpoint
/// offset perpendicular to the rope axis by `sway · rope_length`. The
/// physics in `step()` drives `sway` from local wave amplitude, so a
/// loud spectrum produces a visibly bowing rope while calm music
/// leaves it nearly straight.
struct RopeCanvas {
    start: Point,
    end: Point,
    sway: f32,
    stroke_color: Color,
    stroke_width: f32,
}

impl<Message> canvas::Program<Message> for RopeCanvas {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        // Rope axis: from start to end. Length and unit-perpendicular
        // give us the bend offset direction. Perpendicular is rotate
        // axis 90° CCW: `(dx, dy) → (-dy, dx)`. Sign of `sway` picks
        // which side the bow hangs on — positive sway = bow to the
        // right (canvas x+), matching the boat's convention.
        let dx = self.end.x - self.start.x;
        let dy = self.end.y - self.start.y;
        let length = (dx * dx + dy * dy).sqrt();
        if length <= 0.0 {
            return Vec::new();
        }
        let perp_x = -dy / length;
        let perp_y = dx / length;
        let bend_offset = self.sway * length;
        let mid_x = (self.start.x + self.end.x) * 0.5 + perp_x * bend_offset;
        let mid_y = (self.start.y + self.end.y) * 0.5 + perp_y * bend_offset;

        let path = canvas::Path::new(|builder| {
            builder.move_to(self.start);
            builder.quadratic_curve_to(Point::new(mid_x, mid_y), self.end);
        });

        frame.stroke(
            &path,
            canvas::Stroke::default()
                .with_color(self.stroke_color)
                .with_width(self.stroke_width)
                .with_line_cap(canvas::LineCap::Round),
        );

        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
#[path = "boat_tests.rs"]
mod tests;
