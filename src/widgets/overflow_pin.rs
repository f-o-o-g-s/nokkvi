//! `OverflowPin`: place a child at an arbitrary `(x, y)` without shrinking
//! it. Shared by the surfing boat (which slides past the area's edges) and
//! Theater Mode's transient chrome (which slides below the window's bottom
//! edge).

use iced::{
    Event, Length, Point, Rectangle, Size, Vector,
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
pub(crate) struct OverflowPin<W> {
    content: W,
    position: Point,
}

impl<W> OverflowPin<W> {
    pub(crate) fn new(content: W) -> Self {
        Self {
            content,
            position: Point::ORIGIN,
        }
    }

    pub(crate) fn position(mut self, position: Point) -> Self {
        self.position = position;
        self
    }
}

impl<W> iced::advanced::widget::Meta for OverflowPin<W> {}

/// The pinned child's layout and state: the pin always holds exactly one.
fn pinned(layout: Layout, tree: &Tree) -> (Layout, &Tree) {
    layout
        .iter(&tree.children)
        .next()
        .expect("OverflowPin always lays out exactly one child")
}

/// [`pinned`], mutably.
fn pinned_mut(layout: Layout, tree: &mut Tree) -> (Layout, &mut Tree) {
    layout
        .iter_mut(&mut tree.children)
        .next()
        .expect("OverflowPin always lays out exactly one child")
}

impl<W, Message, Theme, Renderer> Widget<Message, Theme, Renderer> for OverflowPin<W>
where
    W: Widget<Message, Theme, Renderer>,
    Renderer: iced::advanced::Renderer,
{
    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Fill,
        }
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) {
        // The child gets our full limits (never `limits - position`, which is
        // what squashes a fixed-size child near the far edge), then moves.
        self.content.layout(&mut tree.children[0], renderer, limits);
        tree.children[0].translation = Vector::new(self.position.x, self.position.y);

        tree.size = limits.resolve(Length::Fill, Length::Fill, tree.children[0].size);
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        // The child is clipped to our bounds when drawn, so operations see the
        // same clipped viewport (as iced's clipping `container` does).
        let clipped_viewport = layout.bounds().intersection(viewport).unwrap_or_default();
        let (layout, tree) = pinned_mut(layout, tree);

        self.content
            .operate(tree, layout, &clipped_viewport, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let (layout, tree) = pinned_mut(layout, tree);

        self.content
            .update(tree, event, layout, cursor, renderer, shell, viewport);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let (layout, tree) = pinned(layout, tree);

        self.content
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout,
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
            let (layout, tree) = pinned(layout, tree);

            renderer.with_layer(clipped_viewport, |renderer| {
                self.content.draw(
                    tree,
                    renderer,
                    theme,
                    style,
                    layout,
                    cursor,
                    &clipped_viewport,
                );
            });
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
        window: Size,
    ) -> Vec<overlay::Element<'b, Message, Theme, Renderer>> {
        let (layout, tree) = pinned_mut(layout, tree);

        self.content
            .overlay(tree, layout, renderer, viewport, translation, window)
    }
}
