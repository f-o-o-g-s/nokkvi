//! `OverflowPin`: place a child at an arbitrary `(x, y)` without shrinking
//! it. Shared by the surfing boat (which slides past the area's edges) and
//! Theater Mode's transient chrome (which slides below the window's bottom
//! edge).

use iced::{
    Element, Event, Length, Point, Rectangle, Size, Vector,
    advanced::{
        Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{Operation, Tree},
    },
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
/// natural size and any portion that falls outside the pin's own bounds is
/// trimmed by the REAL clip layer its `draw()` pushes (`with_layer`). The
/// layer matters: `container(..).clip(true)` only narrows the `viewport`
/// culling hint, which `Svg::draw` ignores — sprite quads are scissored
/// exclusively by render layers, so without one the boat kept painting
/// past the area's left edge (see `draw()` below).
pub(crate) struct OverflowPin<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer>
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
    pub(crate) fn new(content: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        Self {
            content: content.into(),
            position: Point::ORIGIN,
        }
    }

    pub(crate) fn position(mut self, position: Point) -> Self {
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

    fn diff(&mut self, tree: &mut Tree) {
        self.content.as_widget_mut().diff(tree);
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
            // A REAL clip layer, not just a narrowed viewport hint. In this
            // iced version `container(..).clip(true)` only shrinks the
            // `viewport` argument passed down — a CULLING HINT that `Svg`'s
            // draw ignores outright (`_viewport`), while the actual GPU
            // scissor comes exclusively from render layers
            // (`renderer.start_layer` → `layers.push_clip` → per-layer
            // `set_scissor_rect`, wgpu/src/lib.rs:452). Without this layer
            // the sprite quads draw wherever their bounds land: a boat
            // exiting the area's LEFT edge kept painting over the sidebar
            // (Lines bottom band) / the slot list (Harbour panel) for the
            // entire off-screen wrap transit. The right edge only ever
            // LOOKED correct because it usually coincides with the window
            // edge, whose base scissor clips for free. Scoping the layer to
            // `bounds ∩ viewport` scissors the pinned sprite to the
            // visualizer/scene area on every edge.
            renderer.with_layer(clipped_viewport, |renderer| {
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
            });
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
