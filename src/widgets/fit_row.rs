//! `FitRow`: a single-line row whose first child always shows and whose
//! later children show only while they fit whole.
//!
//! A slot-list title is followed by small tags (a release type, an album
//! version). The title must never give way to them: in a narrow pane a tag
//! that no longer fits beside the full title is left out entirely, never
//! squeezed, wrapped or cut in half. A plain `Row` can't do that, because it
//! hands each child whatever width is left and draws it regardless.

use iced::{
    Event, Length, Rectangle, Size, Vector,
    advanced::{
        Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{Operation, Tree},
    },
};

/// A row that lays its first child out at full width (so its own ellipsis
/// still works), then places each later child after it at its natural width
/// while there is room. The first child that doesn't fit, and every one after
/// it, is hidden: not drawn, not hit-tested.
pub(crate) struct FitRow<W> {
    children: Vec<W>,
    spacing: f32,
}

impl<W> FitRow<W> {
    /// A row leading with `lead`, the child that always shows.
    pub(crate) fn new(lead: W) -> Self {
        Self {
            children: vec![lead],
            spacing: 0.0,
        }
    }

    /// Add a child that shows only while it fits whole after the ones before.
    pub(crate) fn push(mut self, child: W) -> Self {
        self.children.push(child);
        self
    }

    /// The gap between neighbours.
    pub(crate) fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }
}

/// The width a later child is measured in: far wider than any row, and
/// finite so no layout arithmetic downstream meets an infinity.
const MEASURE_WIDTH: f32 = 100_000.0;

/// Where each child goes: `Some(x)` for a child that shows, `None` for one
/// that is hidden. `widths[0]` is the lead's laid-out width (already capped
/// at `available`); the rest are natural widths.
fn placements(widths: &[f32], spacing: f32, available: f32) -> Vec<Option<f32>> {
    let mut next_x = 0.0;
    let mut hidden = false;
    widths
        .iter()
        .enumerate()
        .map(|(i, width)| {
            if i == 0 {
                next_x = *width;
                return Some(0.0);
            }
            let x = next_x + spacing;
            // Half a pixel of slack absorbs text-measurement rounding.
            if hidden || x + width > available + 0.5 {
                hidden = true;
                return None;
            }
            next_x = x + width;
            Some(x)
        })
        .collect()
}

/// Whether a child laid out by [`FitRow::layout`] is one that shows: hidden
/// children are given no size.
fn shows(tree: &Tree) -> bool {
    tree.size.width > 0.0
}

impl<W> iced::advanced::widget::Meta for FitRow<W> {}

impl<W, Message, Theme, Renderer> Widget<Message, Theme, Renderer> for FitRow<W>
where
    W: Widget<Message, Theme, Renderer>,
    Renderer: iced::advanced::Renderer,
{
    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(&mut self.children);
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Shrink,
        }
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) {
        let max = limits.max;
        // The lead may use the whole width; the rest are measured unbounded
        // so their natural width is known before deciding whether they fit.
        let lead_limits = layout::Limits::new(Size::ZERO, max);
        let natural_limits = layout::Limits::new(Size::ZERO, Size::new(MEASURE_WIDTH, max.height));
        for (i, (child, tree)) in self.children.iter_mut().zip(&mut tree.children).enumerate() {
            let child_limits = if i == 0 {
                &lead_limits
            } else {
                &natural_limits
            };
            child.layout(tree, renderer, child_limits);
        }

        let widths: Vec<f32> = tree.children.iter().map(|child| child.size.width).collect();
        let places = placements(&widths, self.spacing, max.width);
        let height = tree
            .children
            .iter()
            .zip(&places)
            .filter(|(_, place)| place.is_some())
            .map(|(child, _)| child.size.height)
            .fold(0.0, f32::max);
        let mut content_width = 0.0_f32;
        for (child, place) in tree.children.iter_mut().zip(&places) {
            match place {
                Some(x) => {
                    child.translation = Vector::new(*x, (height - child.size.height) / 2.0);
                    content_width = content_width.max(x + child.size.width);
                }
                None => {
                    child.translation = Vector::ZERO;
                    child.size = Size::ZERO;
                }
            }
        }

        tree.size = limits.resolve(
            Length::Fill,
            Length::Shrink,
            Size::new(content_width, height),
        );
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(None, layout.bounds(), viewport);
        operation.traverse(&mut |operation| {
            self.children
                .iter_mut()
                .zip(layout.iter_mut(&mut tree.children))
                .filter(|(_, (_, tree))| shows(tree))
                .for_each(|(child, (layout, tree))| {
                    child.operate(tree, layout, viewport, renderer, operation);
                });
        });
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
        for (child, (layout, tree)) in self
            .children
            .iter_mut()
            .zip(layout.iter_mut(&mut tree.children))
        {
            if shows(tree) {
                child.update(tree, event, layout, cursor, renderer, shell, viewport);
            }
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.children
            .iter()
            .zip(layout.iter(&tree.children))
            .filter(|(_, (_, tree))| shows(tree))
            .map(|(child, (layout, tree))| {
                child.mouse_interaction(tree, layout, cursor, viewport, renderer)
            })
            .find(|&interaction| interaction != mouse::Interaction::None)
            .unwrap_or_default()
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
        for (child, (layout, tree)) in self.children.iter().zip(layout.iter(&tree.children)) {
            if shows(tree) {
                child.draw(tree, renderer, theme, style, layout, cursor, viewport);
            }
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
        self.children
            .iter_mut()
            .zip(layout.iter_mut(&mut tree.children))
            .filter(|(_, (_, tree))| shows(tree))
            .flat_map(|(child, (layout, tree))| {
                child.overlay(tree, layout, renderer, viewport, translation, window)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::placements;

    #[test]
    fn everything_shows_while_it_fits() {
        assert_eq!(
            placements(&[100.0, 30.0, 20.0], 6.0, 200.0),
            [Some(0.0), Some(106.0), Some(142.0)]
        );
        // Exactly full still fits.
        assert_eq!(
            placements(&[100.0, 30.0], 6.0, 136.0),
            [Some(0.0), Some(106.0)]
        );
    }

    #[test]
    fn a_child_that_does_not_fit_whole_is_hidden_with_all_after_it() {
        // The second tag would end at 162 of 150: it and the small third
        // one (which alone would fit) are both dropped, so tags never
        // reorder or leave a gap.
        assert_eq!(
            placements(&[100.0, 30.0, 20.0, 4.0], 6.0, 150.0),
            [Some(0.0), Some(106.0), None, None]
        );
    }

    #[test]
    fn a_lead_that_fills_the_row_leaves_no_room_for_tags() {
        assert_eq!(placements(&[150.0, 30.0], 6.0, 150.0), [Some(0.0), None]);
        // The lead always shows, even alone.
        assert_eq!(placements(&[150.0], 6.0, 150.0), [Some(0.0)]);
        assert_eq!(placements(&[], 6.0, 150.0), []);
    }
}
