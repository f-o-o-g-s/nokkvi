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
    Picked { index: usize },
    Dropped { index: usize, target_index: usize },
    Canceled { index: usize },
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
    /// When > 1, a count badge is drawn on top of the dragged item.
    drag_badge_count: usize,
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
            drag_badge_count: 1,
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
        let child_size = child.as_widget().size_hint();

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

    /// Set the drag badge count. When > 1, a "×N" badge is overlaid on the dragged item.
    pub fn drag_badge_count(mut self, count: usize) -> Self {
        self.drag_badge_count = count;
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

    fn children(&self) -> Vec<Tree> {
        self.children.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.children);
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
        let action = tree.state.downcast_ref::<DragState>();

        if let DragState::Dragging {
            index,
            last_cursor,
            origin,
            ..
        } = action
        {
            let child_count = self.children.len();
            let target_index = self
                .compute_target_index(*last_cursor, layout, *index)
                .min(child_count);

            let Some(drag_layout) = layout.children().nth(*index) else {
                return;
            };
            let drag_bounds = drag_layout.bounds();
            let drag_height = drag_bounds.height + self.spacing;

            for i in 0..child_count {
                let child = &self.children[i];
                let state = &tree.children[i];
                let Some(child_layout) = layout.children().nth(i) else {
                    continue;
                };

                if i == *index {
                    // Draw dragged item at cursor Y offset
                    let translation_y = last_cursor.y - origin.y;
                    renderer.with_translation(
                        Vector {
                            x: 0.0,
                            y: translation_y,
                        },
                        |renderer| {
                            renderer.with_layer(child_layout.bounds(), |renderer| {
                                child.as_widget().draw(
                                    state,
                                    renderer,
                                    theme,
                                    defaults,
                                    child_layout,
                                    cursor,
                                    viewport,
                                );

                                // Badge overlay for multi-selection drags
                                if self.drag_badge_count > 1 {
                                    let badge_text = format!("\u{00d7}{}", self.drag_badge_count);
                                    let badge_bounds = child_layout.bounds();

                                    let font = crate::theme::ui_font();
                                    let badge_font_size: Pixels = 13.0.into();

                                    // Fixed pill width based on digit count
                                    let digit_count = if self.drag_badge_count >= 100 {
                                        3
                                    } else if self.drag_badge_count >= 10 {
                                        2
                                    } else {
                                        1
                                    };
                                    let pill_h = 18.0_f32;
                                    let pill_w = 16.0 + digit_count as f32 * 8.0; // × + digits
                                    let pill_x = badge_bounds.x + badge_bounds.width - pill_w - 8.0;
                                    let pill_y = badge_bounds.y + 4.0;
                                    let pill_rect = Rectangle {
                                        x: pill_x,
                                        y: pill_y,
                                        width: pill_w,
                                        height: pill_h,
                                    };

                                    // Draw pill background
                                    renderer.fill_quad(
                                        renderer::Quad {
                                            bounds: pill_rect,
                                            border: iced::Border {
                                                radius: crate::theme::ui_border_radius(),
                                                ..Default::default()
                                            },
                                            ..Default::default()
                                        },
                                        iced::Background::Color(crate::theme::accent()),
                                    );

                                    // Draw badge text centered in pill
                                    renderer.fill_text(
                                        iced::advanced::text::Text {
                                            content: badge_text,
                                            bounds: Size::new(pill_w, pill_h),
                                            size: badge_font_size,
                                            font,
                                            align_x: alignment::Horizontal::Center.into(),
                                            align_y: alignment::Vertical::Center,
                                            line_height: iced::advanced::text::LineHeight::default(
                                            ),
                                            shaping: iced::advanced::text::Shaping::Advanced,
                                            wrapping: iced::advanced::text::Wrapping::None,
                                            ellipsis: iced::advanced::text::Ellipsis::default(),
                                            hint_factor: Some(1.0),
                                        },
                                        Point::new(pill_rect.center_x(), pill_rect.center_y()),
                                        crate::theme::fg0(),
                                        pill_rect,
                                    );
                                }
                            });
                        },
                    );
                } else {
                    // Shift non-dragged items to make room
                    let offset: i32 = match target_index.cmp(index) {
                        std::cmp::Ordering::Less if i >= target_index && i < *index => 1,
                        std::cmp::Ordering::Greater if i > *index && i < target_index => -1,
                        _ => 0,
                    };

                    let translation = Vector::new(0.0, offset as f32 * drag_height);
                    renderer.with_translation(translation, |renderer| {
                        child.as_widget().draw(
                            state,
                            renderer,
                            theme,
                            defaults,
                            child_layout,
                            cursor,
                            viewport,
                        );
                    });
                }
            }
        } else {
            // Normal draw — no drag active
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
