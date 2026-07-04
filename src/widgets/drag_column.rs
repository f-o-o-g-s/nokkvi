// Drag-and-drop column widget for queue reordering.
//
// Adapted from flowsurface's column_drag.rs (credits: github.com/airstrike/dragking).
// Key differences from flowsurface:
// - Whole-row drag (no handle) with 5px movement threshold for click-vs-drag
// - Below threshold, press+release passes through to children (stars, hearts)
// - During active drag, events are NOT forwarded to children

use iced::{
    Element, Length, Padding, Pixels, Point, Rectangle, Size, Theme, Vector,
    advanced::{
        Shell,
        layout::{self, Layout},
        overlay, renderer,
        widget::{Operation, Tree, Widget, tree},
    },
    alignment::{self, Alignment},
    event::Event,
    mouse,
};
use tracing::debug;

/// Minimum vertical movement (px) before a press becomes a drag.
const DRAG_THRESHOLD: f32 = 5.0;

/// Vertical band (px) at the top/bottom of the drag column within which the
/// cursor arms edge auto-scroll during a within-list drag.
const DRAG_EDGE_ZONE_PX: f32 = 48.0;

/// Which vertical edge band (if any) the drag cursor currently occupies. Emitted
/// on the [`DragEvent::Dragged`] channel so the app can drive tick-based edge
/// auto-scroll during a within-list drag. `None` when the cursor is centred.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EdgeZone {
    #[default]
    None,
    Top,
    Bottom,
}

impl EdgeZone {
    /// Classify a cursor Y (window space) against a widget's vertical bounds:
    /// within `edge_px` of the top (or above it) → `Top`, within `edge_px` of
    /// the bottom (or below it) → `Bottom`, otherwise `None`. When the bounds
    /// are shorter than `2 * edge_px` the zones overlap and `Top` wins; such
    /// short lists top-pack and auto-scroll is a no-op there anyway.
    pub(crate) fn from_cursor(
        cursor_y: f32,
        bounds_top: f32,
        bounds_height: f32,
        edge_px: f32,
    ) -> Self {
        if cursor_y <= bounds_top + edge_px {
            EdgeZone::Top
        } else if cursor_y >= bounds_top + bounds_height - edge_px {
            EdgeZone::Bottom
        } else {
            EdgeZone::None
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum DragState {
    Idle,
    Picking {
        index: usize,
        origin: Point,
    },
    Dragging {
        index: usize,
        origin: Point,
        last_cursor: Point,
    },
}

#[derive(Debug, Clone)]
pub enum DragEvent {
    Picked {
        index: usize,
    },
    /// Fired on every cursor move during an active drag. Carries the RAW
    /// (unclamped) cursor so the app-level floating ghost tracks the pointer and
    /// edge detection can see past the list edge; the live `edge` band drives
    /// tick auto-scroll; `target_slot` (from `compute_target_index`) drives the
    /// drop-indicator line. DragColumn never handles the wheel event at all, so
    /// wheel-scroll-during-drag keeps working (the outer scroll `mouse_area`
    /// still sees it).
    Dragged {
        cursor: Point,
        edge: EdgeZone,
        target_slot: usize,
    },
    Dropped {
        index: usize,
        target_index: usize,
    },
}

#[allow(missing_debug_implementations)]
pub struct DragColumn<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    spacing: f32,
    padding: Padding,
    width: Length,
    height: Length,
    max_width: f32,
    align: Alignment,
    clip: bool,
    children: Vec<Element<'a, Message, Theme, Renderer>>,
    on_drag: Option<Box<dyn Fn(DragEvent) -> Message + 'a>>,
}

impl<'a, Message, Theme, Renderer> DragColumn<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    pub fn new() -> Self {
        Self::from_vec(Vec::new())
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self::from_vec(Vec::with_capacity(capacity))
    }

    pub fn with_children(
        children: impl IntoIterator<Item = Element<'a, Message, Theme, Renderer>>,
    ) -> Self {
        let iterator = children.into_iter();
        Self::with_capacity(iterator.size_hint().0).extend(iterator)
    }

    pub fn from_vec(children: Vec<Element<'a, Message, Theme, Renderer>>) -> Self {
        Self {
            spacing: 0.0,
            padding: Padding::ZERO,
            width: Length::Shrink,
            height: Length::Shrink,
            max_width: f32::INFINITY,
            align: Alignment::Start,
            clip: false,
            children,
            on_drag: None,
        }
    }

    pub fn spacing(mut self, amount: impl Into<Pixels>) -> Self {
        self.spacing = amount.into().0;
        self
    }

    pub fn padding<P: Into<Padding>>(mut self, padding: P) -> Self {
        self.padding = padding.into();
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    pub fn max_width(mut self, max_width: impl Into<Pixels>) -> Self {
        self.max_width = max_width.into().0;
        self
    }

    pub fn align_x(mut self, align: impl Into<alignment::Horizontal>) -> Self {
        self.align = Alignment::from(align.into());
        self
    }

    pub fn clip(mut self, clip: bool) -> Self {
        self.clip = clip;
        self
    }

    pub fn push(mut self, child: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        let child = child.into();
        let child_size = child.as_widget().size();

        self.width = self.width.enclose(child_size.width);
        self.height = self.height.enclose(child_size.height);

        self.children.push(child);
        self
    }

    pub fn extend(
        self,
        children: impl IntoIterator<Item = Element<'a, Message, Theme, Renderer>>,
    ) -> Self {
        children.into_iter().fold(self, Self::push)
    }

    /// Set the callback for drag events. When `None`, the widget acts as a normal column.
    pub fn on_drag(mut self, on_drag: impl Fn(DragEvent) -> Message + 'a) -> Self {
        self.on_drag = Some(Box::new(on_drag));
        self
    }

    fn compute_target_index(
        &self,
        cursor_position: Point,
        layout: Layout<'_>,
        dragged_index: usize,
    ) -> usize {
        let cursor_y = cursor_position.y;
        let bounds = layout.bounds();

        if cursor_y <= bounds.y {
            return 0;
        }
        if cursor_y >= bounds.y + bounds.height {
            return self.children.len();
        }

        for (i, child_layout) in layout.children().enumerate() {
            let child_bounds = child_layout.bounds();
            let y = child_bounds.y;
            let height = child_bounds.height;
            let middle = y + height / 2.0;

            if cursor_y >= y && cursor_y <= y + height {
                if i == dragged_index {
                    continue;
                }
                if cursor_y < middle {
                    return i;
                }
                return i + 1;
            } else if cursor_y < y {
                return i;
            }
        }

        self.children.len()
    }
}

impl<Message, Renderer> Default for DragColumn<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<'a, Message, Theme, Renderer: renderer::Renderer>
    FromIterator<Element<'a, Message, Theme, Renderer>>
    for DragColumn<'a, Message, Theme, Renderer>
{
    fn from_iter<T: IntoIterator<Item = Element<'a, Message, Theme, Renderer>>>(iter: T) -> Self {
        Self::with_children(iter)
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for DragColumn<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer + iced::advanced::text::Renderer<Font = iced::Font>,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<DragState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(DragState::Idle)
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(&mut self.children);
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let limits = limits.max_width(self.max_width);

        layout::flex::resolve(
            layout::flex::Axis::Vertical,
            renderer,
            &limits,
            self.width,
            self.height,
            self.padding,
            self.spacing,
            self.align,
            &mut self.children,
            &mut tree.children,
        )
    }

    fn operate(
        &mut self,
        _tree: &mut Tree,
        layout: Layout<'_>,
        _renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(None, layout.bounds());
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
        // Only handle drag logic when on_drag is set
        if self.on_drag.is_some() {
            let action = tree.state.downcast_mut::<DragState>();

            match event {
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                    let bounds = layout.bounds();
                    if let Some(cursor_position) = cursor.position_over(bounds) {
                        // Find which child was clicked
                        for (index, child_layout) in layout.children().enumerate() {
                            if child_layout.bounds().contains(cursor_position) {
                                *action = DragState::Picking {
                                    index,
                                    origin: cursor_position,
                                };
                                break;
                            }
                        }
                    }
                }
                Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                    match *action {
                        DragState::Picking { index, origin } => {
                            if let Some(cursor_position) = cursor.position() {
                                let dy = (cursor_position.y - origin.y).abs();
                                if dy >= DRAG_THRESHOLD {
                                    // Threshold crossed — transition to Dragging
                                    *action = DragState::Dragging {
                                        index,
                                        origin,
                                        last_cursor: cursor_position,
                                    };
                                    if let Some(on_drag) = &self.on_drag {
                                        shell.publish(on_drag(DragEvent::Picked { index }));
                                    }
                                }
                            }
                        }
                        DragState::Dragging { origin, index, .. } => {
                            if let Some(cursor_position) = cursor.position() {
                                let bounds = layout.bounds();
                                let clamped_y =
                                    cursor_position.y.clamp(bounds.y, bounds.y + bounds.height);
                                let clamped_cursor = Point {
                                    x: cursor_position.x,
                                    y: clamped_y,
                                };

                                *action = DragState::Dragging {
                                    last_cursor: clamped_cursor,
                                    origin,
                                    index,
                                };

                                // Surface the RAW cursor (so the app-level ghost
                                // follows the pointer and edge detection sees
                                // past-edge), the edge band, and the live
                                // drop-target slot. A plain publish on a
                                // CursorMoved: DragColumn never matches the wheel
                                // event, so wheel-scroll-during-drag still reaches
                                // the outer scroll `mouse_area`.
                                if let Some(on_drag) = &self.on_drag {
                                    let edge = EdgeZone::from_cursor(
                                        cursor_position.y,
                                        bounds.y,
                                        bounds.height,
                                        DRAG_EDGE_ZONE_PX,
                                    );
                                    let target_slot =
                                        self.compute_target_index(clamped_cursor, layout, index);
                                    shell.publish(on_drag(DragEvent::Dragged {
                                        cursor: cursor_position,
                                        edge,
                                        target_slot,
                                    }));
                                }

                                shell.request_redraw();
                            }
                        }
                        DragState::Idle => {}
                    }
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    match *action {
                        DragState::Dragging {
                            index, last_cursor, ..
                        } => {
                            let target_index =
                                self.compute_target_index(last_cursor, layout, index);
                            debug!(
                                "🖱️ [DRAG] Dropped child {} → target {}",
                                index, target_index
                            );
                            if let Some(on_drag) = &self.on_drag {
                                shell.publish(on_drag(DragEvent::Dropped {
                                    index,
                                    target_index,
                                }));
                            }
                            *action = DragState::Idle;
                        }
                        DragState::Picking { .. } => {
                            // Released without exceeding threshold — treat as a click.
                            *action = DragState::Idle;
                        }
                        DragState::Idle => {}
                    }
                }
                _ => {}
            }

            // When actively dragging, do NOT forward events to children.
            // This prevents accidental star/heart clicks mid-drag.
            if matches!(
                tree.state.downcast_ref::<DragState>(),
                DragState::Dragging { .. }
            ) {
                return;
            }
        }

        // Forward events to children (normal column behavior)
        self.children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
            .for_each(|((child, tree), layout)| {
                child
                    .as_widget_mut()
                    .update(tree, event, layout, cursor, renderer, shell, viewport);
            });
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        if self.on_drag.is_some() {
            let action = tree.state.downcast_ref::<DragState>();
            match action {
                DragState::Dragging { .. } | DragState::Picking { .. } => {
                    return mouse::Interaction::Grabbing;
                }
                DragState::Idle => {}
            }
        }

        self.children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((child, state), layout)| {
                child
                    .as_widget()
                    .mouse_interaction(state, layout, cursor, viewport, renderer)
            })
            .max()
            .unwrap_or_default()
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        defaults: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        // Every row draws normally in every state — including during a drag.
        //
        // The slot list is a virtualized data-offset window: lifting the grabbed
        // child by its FROZEN slot position (as this widget used to) redraws
        // whatever item currently occupies that slot, so the ghost's content
        // cycled as the list scrolled under the drag. Instead the grabbed row is
        // represented by an app-level floating ghost rendered from data BY
        // IDENTITY (`Nokkvi::render_within_list_drag_slot`), and the destination
        // by the drop-indicator line (`slot_list_view_with_drag`'s
        // `drop_indicator_slot`, fed by the live `DragEvent::Dragged.target_slot`).
        // So the widget never translates a child, and nothing cycles.
        for ((child, state), layout) in self
            .children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
        {
            child
                .as_widget()
                .draw(state, renderer, theme, defaults, layout, cursor, viewport);
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
        overlay::from_children(
            &mut self.children,
            tree,
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message, Theme, Renderer> From<DragColumn<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + iced::advanced::text::Renderer<Font = iced::Font> + 'a,
{
    fn from(column: DragColumn<'a, Message, Theme, Renderer>) -> Self {
        Self::new(column)
    }
}

#[cfg(test)]
mod tests {
    use super::EdgeZone;

    // bounds: top = 100, height = 400 → bottom = 500, edge band = 48px.
    const TOP: f32 = 100.0;
    const HEIGHT: f32 = 400.0;
    const EDGE: f32 = 48.0;

    #[test]
    fn edge_zone_middle_is_none() {
        assert_eq!(
            EdgeZone::from_cursor(300.0, TOP, HEIGHT, EDGE),
            EdgeZone::None
        );
    }

    #[test]
    fn edge_zone_just_inside_top() {
        // 148 is exactly top+edge → Top (inclusive).
        assert_eq!(
            EdgeZone::from_cursor(148.0, TOP, HEIGHT, EDGE),
            EdgeZone::Top
        );
        assert_eq!(
            EdgeZone::from_cursor(120.0, TOP, HEIGHT, EDGE),
            EdgeZone::Top
        );
    }

    #[test]
    fn edge_zone_just_inside_bottom() {
        // 452 is exactly bottom-edge → Bottom (inclusive).
        assert_eq!(
            EdgeZone::from_cursor(452.0, TOP, HEIGHT, EDGE),
            EdgeZone::Bottom
        );
        assert_eq!(
            EdgeZone::from_cursor(480.0, TOP, HEIGHT, EDGE),
            EdgeZone::Bottom
        );
    }

    #[test]
    fn edge_zone_past_edges_clamp_to_bands() {
        // Above the top / below the bottom still classify as Top / Bottom.
        assert_eq!(
            EdgeZone::from_cursor(90.0, TOP, HEIGHT, EDGE),
            EdgeZone::Top
        );
        assert_eq!(
            EdgeZone::from_cursor(520.0, TOP, HEIGHT, EDGE),
            EdgeZone::Bottom
        );
    }

    #[test]
    fn edge_zone_default_is_none() {
        assert_eq!(EdgeZone::default(), EdgeZone::None);
    }
}
