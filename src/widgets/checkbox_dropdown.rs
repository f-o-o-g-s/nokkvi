//! Checkbox Dropdown Widget
//!
//! Reusable trigger-button + overlay-panel combo for multi-toggle UIs (e.g.
//! per-column visibility in a view header). Internally manages its open/closed
//! state, mirroring [`crate::widgets::context_menu`] but with:
//!
//! - **Left-click trigger** (vs right-click).
//! - **Anchored below the trigger** (vs at cursor position).
//! - **Stays open after item click** (vs closes), so the user can flip several
//!   toggles in one open.
//! - Items render with a check-or-empty icon + label.
//!
//! ```ignore
//! checkbox_dropdown(
//!     "assets/icons/columns-3-cog.svg",
//!     "Show/hide columns",
//!     vec![
//!         ("Stars".into(), state.stars),
//!         ("Album".into(), state.album),
//!     ],
//!     |idx| Message::ToggleColumn(idx),
//! )
//! ```

use iced::{
    Element, Event, Length, Point, Rectangle, Size, Theme, Vector,
    advanced::{
        Layout, Shell, Widget, layout, overlay, renderer,
        widget::{self, tree},
    },
    keyboard, mouse,
    widget::{column, container, mouse_area, row, svg, text, tooltip},
};

use crate::theme;

/// Build a checkbox dropdown anchored to a trigger icon button.
pub(crate) fn checkbox_dropdown<'a, Message: Clone + 'a>(
    trigger_icon: &'static str,
    tooltip: &'static str,
    items: Vec<(String, bool)>,
    on_item_toggle: impl Fn(usize) -> Message + 'a,
) -> CheckboxDropdown<'a, Message> {
    CheckboxDropdown {
        trigger: trigger_button(trigger_icon, tooltip),
        items,
        on_item_toggle: Box::new(on_item_toggle),
        menu: None,
    }
}

const TRIGGER_SIZE: f32 = 40.0;
const TRIGGER_ICON_SIZE: f32 = 20.0;
const MENU_MIN_WIDTH: f32 = 180.0;
const MENU_ICON_SIZE: f32 = 14.0;
const MENU_TEXT_SIZE: f32 = 13.0;

/// Build the trigger element — a 40×40 styled container holding the icon,
/// wrapped in a tooltip that mirrors `view_header::header_icon_button` chrome
/// (without on_press, since the `CheckboxDropdown` Widget intercepts the
/// left-click itself).
fn trigger_button<'a, Message: 'a>(
    icon_path: &'static str,
    tooltip_text: &'static str,
) -> Element<'a, Message> {
    let icon = crate::embedded_svg::svg_widget(icon_path)
        .width(Length::Fixed(TRIGGER_ICON_SIZE))
        .height(Length::Fixed(TRIGGER_ICON_SIZE))
        .style(|_theme, _status| svg::Style {
            color: Some(theme::fg0()),
        });

    let trigger = container(icon)
        .width(Length::Fixed(TRIGGER_SIZE))
        .height(Length::Fixed(TRIGGER_SIZE))
        .style(|_theme| container::Style {
            background: Some(theme::bg0_soft().into()),
            border: iced::Border {
                radius: theme::ui_border_radius(),
                ..Default::default()
            },
            ..Default::default()
        })
        .center(Length::Fixed(TRIGGER_SIZE));

    tooltip(
        trigger,
        container(text(tooltip_text).size(11.0).font(theme::ui_font())).padding(4),
        tooltip::Position::Top,
    )
    .gap(4)
    .style(theme::container_tooltip)
    .into()
}

/// Render a single dropdown item: check-or-empty icon + label.
fn dropdown_item<'a, Message: Clone + 'a>(
    label: &str,
    checked: bool,
    on_press: Message,
) -> Element<'a, Message> {
    let check_icon_path = if checked {
        "assets/icons/check.svg"
    } else {
        // Render an empty 14×14 spacer so labels stay aligned.
        ""
    };

    let check_element: Element<'a, Message> = if check_icon_path.is_empty() {
        iced::widget::Space::new()
            .width(Length::Fixed(MENU_ICON_SIZE))
            .height(Length::Fixed(MENU_ICON_SIZE))
            .into()
    } else {
        crate::embedded_svg::svg_widget(check_icon_path)
            .width(Length::Fixed(MENU_ICON_SIZE))
            .height(Length::Fixed(MENU_ICON_SIZE))
            .style(|_theme, _status| svg::Style {
                color: Some(theme::fg0()),
            })
            .into()
    };

    let row_content = row![
        check_element,
        text(label.to_string())
            .size(MENU_TEXT_SIZE)
            .font(theme::ui_font())
            .color(theme::fg0()),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    mouse_area(
        container(row_content)
            .width(Length::Fill)
            .padding(iced::Padding {
                left: 8.0,
                right: 16.0,
                top: 4.0,
                bottom: 4.0,
            })
            .style(|_theme| container::Style {
                background: None,
                border: iced::Border {
                    radius: theme::ui_border_radius(),
                    ..Default::default()
                },
                ..Default::default()
            }),
    )
    .on_press(on_press)
    .interaction(iced::mouse::Interaction::Pointer)
    .into()
}

/// Build the menu element that floats below the trigger when open.
fn build_menu_element<'a, Message: Clone + 'a>(
    items: &[(String, bool)],
    on_item_toggle: &dyn Fn(usize) -> Message,
) -> Element<'a, Message> {
    let rows: Vec<Element<'a, Message>> = items
        .iter()
        .enumerate()
        .map(|(idx, (label, checked))| dropdown_item(label, *checked, on_item_toggle(idx)))
        .collect();

    container(column(rows).spacing(0))
        .width(Length::Fixed(MENU_MIN_WIDTH))
        .padding(4)
        .style(|_theme| container::Style {
            background: Some(theme::bg1().into()),
            border: iced::Border {
                width: 1.0,
                color: theme::accent_bright(),
                radius: theme::ui_border_radius(),
            },
            ..Default::default()
        })
        .into()
}

// ============================================================================
// Widget
// ============================================================================

pub struct CheckboxDropdown<'a, Message> {
    trigger: Element<'a, Message>,
    items: Vec<(String, bool)>,
    on_item_toggle: Box<dyn Fn(usize) -> Message + 'a>,
    /// Cached menu element, rebuilt each frame the dropdown is open.
    menu: Option<Element<'a, Message>>,
}

#[derive(Debug)]
struct State {
    status: Status,
    menu_tree: widget::Tree,
}

impl State {
    fn new() -> Self {
        Self {
            status: Status::Closed,
            menu_tree: widget::Tree::empty(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Status {
    Closed,
    /// Open with the trigger's screen-space bounds captured at click time, so
    /// the overlay can anchor below it.
    Open {
        trigger_bounds: Rectangle,
    },
}

impl<'a, Message> Widget<Message, Theme, iced::Renderer> for CheckboxDropdown<'a, Message>
where
    Message: Clone + 'a,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::new())
    }

    fn children(&self) -> Vec<widget::Tree> {
        vec![widget::Tree::new(&self.trigger)]
    }

    fn diff(&self, tree: &mut widget::Tree) {
        tree.diff_children(std::slice::from_ref(&self.trigger));
    }

    fn size(&self) -> Size<Length> {
        self.trigger.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.trigger
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.trigger.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        // Intercept left-click on the trigger bounds to toggle open/closed.
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event {
            let state = tree.state.downcast_mut::<State>();
            if cursor.position_over(layout.bounds()).is_some() {
                state.status = match state.status {
                    Status::Closed => Status::Open {
                        trigger_bounds: layout.bounds(),
                    },
                    Status::Open { .. } => Status::Closed,
                };
                shell.capture_event();
                shell.request_redraw();
                return;
            }
        }

        // Forward all other events to the trigger child (e.g. for cursor
        // interaction tracking — the trigger is a plain container, so nothing
        // load-bearing happens here, but pass through for completeness).
        self.trigger.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if cursor.position_over(layout.bounds()).is_some() {
            return mouse::Interaction::Pointer;
        }
        self.trigger.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut widget::Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
        let trigger_state = tree.children.first_mut()?;
        let trigger_overlay = self.trigger.as_widget_mut().overlay(
            trigger_state,
            layout,
            renderer,
            viewport,
            translation,
        );

        let state = tree.state.downcast_mut::<State>();
        let our_overlay = build_overlay(
            state,
            &mut self.menu,
            &self.items,
            &*self.on_item_toggle,
            translation,
        );

        if trigger_overlay.is_none() && our_overlay.is_none() {
            None
        } else {
            Some(
                overlay::Group::with_children(
                    trigger_overlay.into_iter().chain(our_overlay).collect(),
                )
                .overlay(),
            )
        }
    }
}

impl<'a, Message: Clone + 'a> From<CheckboxDropdown<'a, Message>> for Element<'a, Message> {
    fn from(dropdown: CheckboxDropdown<'a, Message>) -> Self {
        Element::new(dropdown)
    }
}

// ============================================================================
// Overlay Builder
// ============================================================================

fn build_overlay<'a, 'b, Message>(
    state: &'b mut State,
    menu: &'b mut Option<Element<'a, Message>>,
    items: &[(String, bool)],
    on_item_toggle: &dyn Fn(usize) -> Message,
    translation: Vector,
) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>>
where
    Message: Clone + 'a,
{
    if items.is_empty() {
        return None;
    }

    match state.status {
        Status::Open { .. } => {
            let m = menu.get_or_insert_with(|| build_menu_element(items, on_item_toggle));
            if state.menu_tree.children.is_empty() {
                state.menu_tree = widget::Tree::new(&*m);
            } else {
                state.menu_tree.diff(&*m as &Element<'a, Message>);
            }
        }
        Status::Closed => {
            *menu = None;
            state.menu_tree = widget::Tree::empty();
            return None;
        }
    }

    if let Status::Open { trigger_bounds } = state.status {
        menu.as_mut().map(|m| {
            overlay::Element::new(Box::new(MenuOverlay {
                menu: m,
                state,
                trigger_bounds: trigger_bounds + translation,
            }))
        })
    } else {
        None
    }
}

// ============================================================================
// Menu Overlay
// ============================================================================

struct MenuOverlay<'a, 'b, Message> {
    menu: &'b mut Element<'a, Message>,
    state: &'b mut State,
    trigger_bounds: Rectangle,
}

impl<Message> overlay::Overlay<Message, Theme, iced::Renderer> for MenuOverlay<'_, '_, Message> {
    fn layout(&mut self, renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        let limits = layout::Limits::new(Size::ZERO, bounds)
            .width(Length::Shrink)
            .height(Length::Shrink);

        let node = self
            .menu
            .as_widget_mut()
            .layout(&mut self.state.menu_tree, renderer, &limits);

        // Anchor below the trigger, right-aligned to its right edge so the
        // menu doesn't visually pull away from the icon.
        let menu_size = node.size();
        let mut x = self.trigger_bounds.x + self.trigger_bounds.width - menu_size.width;
        let mut y = self.trigger_bounds.y + self.trigger_bounds.height + 4.0;

        // Clamp inside the viewport (with a small inset).
        let padding = 5.0;
        let max_x = bounds.width - padding - menu_size.width;
        let max_y = bounds.height - padding - menu_size.height;
        if x < padding {
            x = padding;
        } else if x > max_x {
            x = max_x.max(padding);
        }
        if y < padding {
            y = padding;
        } else if y > max_y {
            // Fall back to anchoring above the trigger if there's no room
            // below.
            y = (self.trigger_bounds.y - menu_size.height - 4.0).max(padding);
        }

        node.move_to(Point::new(x, y))
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
    ) {
        // Escape → close.
        if matches!(
            event,
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            })
        ) {
            self.state.status = Status::Closed;
            shell.capture_event();
            shell.request_redraw();
            return;
        }

        let menu_bounds = layout.bounds();
        let cursor_over_menu = cursor.position_over(menu_bounds).is_some();
        let cursor_over_trigger = cursor.position_over(self.trigger_bounds).is_some();

        // Click outside the menu AND outside the trigger → close. (Click on
        // the trigger is handled by the underlying Widget::update, which
        // toggles back to Closed.)
        if matches!(event, Event::Mouse(mouse::Event::ButtonPressed(_)))
            && !cursor_over_menu
            && !cursor_over_trigger
        {
            self.state.status = Status::Closed;
            shell.capture_event();
            shell.request_redraw();
            return;
        }

        // Forward to menu content so item mouse_areas fire on_press.
        // Stays open on item click — the user can flip several toggles in
        // one open.
        self.menu.as_widget_mut().update(
            &mut self.state.menu_tree,
            event,
            layout,
            cursor,
            renderer,
            shell,
            &menu_bounds,
        );
    }

    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        self.menu.as_widget().draw(
            &self.state.menu_tree,
            renderer,
            theme,
            style,
            layout,
            cursor,
            &layout.bounds(),
        );
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.menu.as_widget().mouse_interaction(
            &self.state.menu_tree,
            layout,
            cursor,
            &layout.bounds(),
            renderer,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkbox_dropdown_compiles_with_typical_inputs() {
        // Smoke test: the public API accepts the expected shape and produces
        // a valid Element.
        let items = vec![
            ("Stars".to_string(), true),
            ("Album".to_string(), false),
            ("Duration".to_string(), true),
        ];
        let _el: Element<'_, String> = checkbox_dropdown(
            "assets/icons/columns-3-cog.svg",
            "Show/hide columns",
            items,
            |idx| format!("toggle-{idx}"),
        )
        .into();
    }

    #[test]
    fn empty_items_still_produces_element() {
        // Edge case: empty items vector is a valid input (overlay just won't
        // render anything when opened).
        let _el: Element<'_, String> = checkbox_dropdown(
            "assets/icons/columns-3-cog.svg",
            "Show/hide columns",
            Vec::new(),
            |idx| format!("toggle-{idx}"),
        )
        .into();
    }
}
