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
//! Two flavors share the same widget chassis (overlay positioning, escape /
//! click-outside, persisted hover state):
//!
//! 1. [`checkbox_dropdown`] — **single-column rows** with `&'static str` labels,
//!    used by view-header column-visibility menus.
//! 2. [`checkbox_dropdown_dynamic`] — **two-column rows** with owned `String`
//!    name + right-aligned dim metadata label, used when the row contents come
//!    from runtime data (e.g. the library-filter popover).
//!
//! ```ignore
//! checkbox_dropdown(
//!     "assets/icons/columns-3-cog.svg",
//!     "Show/hide columns",
//!     vec![
//!         (Col::Stars, "Stars", state.stars),
//!         (Col::Album, "Album", state.album),
//!     ],
//!     Message::ToggleColumn,  // bare tuple-variant constructor
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

use crate::{
    theme,
    widgets::{
        menu_constants::{MENU_ICON_SIZE, MENU_MIN_WIDTH, MENU_TEXT_SIZE},
        sizes::ICON_BUTTON_SIZE,
    },
};

/// 40×40 trigger button chassis — matches the shared `ICON_BUTTON_SIZE`.
const TRIGGER_SIZE: f32 = ICON_BUTTON_SIZE;
const TRIGGER_ICON_SIZE: f32 = 20.0;

/// Max width of the name column in two-column rows (px).
///
/// Derived from `MENU_MIN_WIDTH` (180) minus container padding (8 left +
/// 16 right = 24), check icon (14), inter-element spacing (8 + 8 = 16),
/// and ~30 reserved for the right-aligned metadata label. Names longer
/// than this ellipsize so the right label stays anchored to the menu edge
/// and the row never overflows the popover width.
const MAX_NAME_WIDTH: f32 = 110.0;

/// One row in the dropdown menu. Single-column rows carry a static label
/// (used by view-header column dropdowns); two-column rows carry an owned
/// name + dim right-aligned metadata label (used by runtime-data popovers
/// like the library filter).
///
/// `TwoColumn` is constructed by [`checkbox_dropdown_dynamic`]; the
/// `dead_code` allow is in place until Lane C wires up the library-filter
/// popover at its call site, after which the production reachability check
/// will satisfy itself.
#[allow(dead_code)]
enum DropdownItemData<Key> {
    SingleColumn {
        key: Key,
        label: &'static str,
        checked: bool,
    },
    TwoColumn {
        key: Key,
        name: String,
        right_label: String,
        checked: bool,
    },
}

/// Build a checkbox dropdown anchored to a trigger icon button.
///
/// `is_open` and `on_open_change` make this a controlled component — open
/// state lives on the parent (so a single root-level menu coordinator can
/// enforce mutual exclusion with other overlay menus). When opening, the
/// callback receives the trigger's screen-space bounds so the parent can
/// stash them in `OpenMenu::CheckboxDropdown`. The bounds come back via
/// `trigger_bounds` so the overlay can anchor below the trigger.
pub(crate) fn checkbox_dropdown<'a, Key, Message>(
    trigger_icon: &'static str,
    tooltip: &'static str,
    items: Vec<(Key, &'static str, bool)>,
    on_item_toggle: impl Fn(Key) -> Message + 'a,
    on_open_change: impl Fn(Option<Rectangle>) -> Message + 'a,
    is_open: bool,
    trigger_bounds: Option<Rectangle>,
) -> CheckboxDropdown<'a, Key, Message>
where
    Key: Copy + 'a,
    Message: Clone + 'a,
{
    let items = items
        .into_iter()
        .map(|(key, label, checked)| DropdownItemData::SingleColumn {
            key,
            label,
            checked,
        })
        .collect();

    CheckboxDropdown {
        trigger: trigger_button(trigger_icon, tooltip),
        items,
        on_item_toggle: Box::new(on_item_toggle),
        on_open_change: Box::new(on_open_change),
        is_open,
        trigger_bounds,
        menu: None,
    }
}

/// Runtime-data sibling of [`checkbox_dropdown`] for popovers whose row
/// contents are owned `String`s (e.g. the library-filter popover, whose
/// library names + song counts come from the Navidrome server).
///
/// Each item is `(id, name, right_label, checked)`:
/// - `name` is the primary label (ellipsized at [`MAX_NAME_WIDTH`]).
/// - `right_label` is the dim right-aligned metadata (e.g. a song count).
/// - `checked` drives the leading check-or-empty glyph.
///
/// Shares overlay positioning, escape / click-outside handling, and the
/// same `OpenMenu`-style controlled open state with [`checkbox_dropdown`].
/// `items` is consumed by value (moved into the widget); `on_item_toggle`
/// is invoked once per row press, never on every render.
///
/// `dead_code` allow is in place until Lane C wires this into the
/// library-filter popover; the tests in this module exercise the
/// constructor so it never genuinely rots.
#[allow(dead_code)]
pub(crate) fn checkbox_dropdown_dynamic<'a, Key, Message>(
    trigger_icon: &'static str,
    tooltip: &'static str,
    items: Vec<(Key, String, String, bool)>,
    on_item_toggle: impl Fn(Key) -> Message + 'a,
    on_open_change: impl Fn(Option<Rectangle>) -> Message + 'a,
    is_open: bool,
    trigger_bounds: Option<Rectangle>,
) -> CheckboxDropdown<'a, Key, Message>
where
    Key: Copy + Eq + std::hash::Hash + 'a,
    Message: Clone + 'a,
{
    let items = items
        .into_iter()
        .map(
            |(key, name, right_label, checked)| DropdownItemData::TwoColumn {
                key,
                name,
                right_label,
                checked,
            },
        )
        .collect();

    CheckboxDropdown {
        trigger: trigger_button(trigger_icon, tooltip),
        items,
        on_item_toggle: Box::new(on_item_toggle),
        on_open_change: Box::new(on_open_change),
        is_open,
        trigger_bounds,
        menu: None,
    }
}

/// Overlay-only variant of [`checkbox_dropdown_dynamic`] for popovers
/// whose trigger lives outside this widget (e.g. the library-filter nav-bar
/// trigger, which has its own icon + count + pip chrome that the standard
/// `trigger_button()` cannot represent).
///
/// The widget itself renders a zero-size `Space` as its "trigger" — it
/// takes no layout space and intercepts no clicks. The overlay still
/// anchors to the externally-captured `trigger_bounds`, so the popover
/// appears below the parent's visible trigger button.
///
/// The parent's trigger button is responsible for emitting the
/// open / close `on_open_change` messages on left-click; this widget
/// only handles row clicks, click-outside-to-close, and Escape.
pub(crate) fn library_selector_popover<'a, Message>(
    items: Vec<(i32, String, String, bool)>,
    on_item_toggle: impl Fn(i32) -> Message + 'a,
    on_open_change: impl Fn(Option<Rectangle>) -> Message + 'a,
    is_open: bool,
    trigger_bounds: Option<Rectangle>,
) -> CheckboxDropdown<'a, i32, Message>
where
    Message: Clone + 'a,
{
    let items = items
        .into_iter()
        .map(
            |(key, name, right_label, checked)| DropdownItemData::TwoColumn {
                key,
                name,
                right_label,
                checked,
            },
        )
        .collect();

    CheckboxDropdown {
        trigger: iced::widget::Space::new().into(),
        items,
        on_item_toggle: Box::new(on_item_toggle),
        on_open_change: Box::new(on_open_change),
        is_open,
        trigger_bounds,
        menu: None,
    }
}

/// Drop-in wrapper around [`checkbox_dropdown`] that pre-builds the
/// `OpenMenu::CheckboxDropdown { view, trigger_bounds }` open-change message
/// so each view only supplies its column items, toggle-message constructor,
/// and `SetOpenMenu` handler.
pub(crate) fn view_columns_dropdown<'a, Key, Message>(
    view: crate::View,
    items: Vec<(Key, &'static str, bool)>,
    on_toggle: impl Fn(Key) -> Message + 'a,
    on_set_open_menu: impl Fn(Option<crate::app_message::OpenMenu>) -> Message + 'a,
    is_open: bool,
    trigger_bounds: Option<iced::Rectangle>,
) -> CheckboxDropdown<'a, Key, Message>
where
    Key: Copy + 'a,
    Message: Clone + 'a,
{
    checkbox_dropdown(
        "assets/icons/columns-3-cog.svg",
        "Show/hide columns",
        items,
        on_toggle,
        move |rect| match rect {
            Some(b) => on_set_open_menu(Some(crate::app_message::OpenMenu::CheckboxDropdown {
                view,
                trigger_bounds: b,
            })),
            None => on_set_open_menu(None),
        },
        is_open,
        trigger_bounds,
    )
}

/// Like [`view_columns_dropdown`] for the Similar panel, which uses
/// `OpenMenu::CheckboxDropdownSimilar` because `View` has no `Similar` variant.
pub(crate) fn similar_columns_dropdown<'a, Key, Message>(
    items: Vec<(Key, &'static str, bool)>,
    on_toggle: impl Fn(Key) -> Message + 'a,
    on_set_open_menu: impl Fn(Option<crate::app_message::OpenMenu>) -> Message + 'a,
    is_open: bool,
    trigger_bounds: Option<iced::Rectangle>,
) -> CheckboxDropdown<'a, Key, Message>
where
    Key: Copy + 'a,
    Message: Clone + 'a,
{
    checkbox_dropdown(
        "assets/icons/columns-3-cog.svg",
        "Show/hide columns",
        items,
        on_toggle,
        move |rect| match rect {
            Some(b) => on_set_open_menu(Some(
                crate::app_message::OpenMenu::CheckboxDropdownSimilar { trigger_bounds: b },
            )),
            None => on_set_open_menu(None),
        },
        is_open,
        trigger_bounds,
    )
}

/// Build the trigger element — a 40×40 styled container holding the icon,
/// wrapped in a tooltip that mirrors `view_header::flat_icon_button` chrome
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

/// Render a two-column dropdown item: check-or-empty icon + name (ellipsized
/// at [`MAX_NAME_WIDTH`]) + flexible spacer + dim right-aligned metadata label.
///
/// Mirrors the chrome of [`dropdown_item`] (padding, hover behavior, border
/// radius) so the two row variants look uniform inside a single menu.
fn dropdown_item_two_column<'a, Message: Clone + 'a>(
    name: &str,
    right_label: &str,
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

    let name_text = text(name.to_string())
        .size(MENU_TEXT_SIZE)
        .font(theme::ui_font())
        .color(theme::fg0())
        .width(Length::Fixed(MAX_NAME_WIDTH))
        .wrapping(iced::widget::text::Wrapping::None)
        .ellipsis(iced::widget::text::Ellipsis::End);

    let right_text = text(right_label.to_string())
        .size(MENU_TEXT_SIZE)
        .font(theme::ui_font())
        .color(theme::fg2());

    let row_content = row![
        check_element,
        name_text,
        iced::widget::Space::new().width(Length::Fill),
        right_text,
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

/// Build the menu element that floats below the trigger when open. Dispatches
/// each item to the matching row renderer based on its variant.
fn build_menu_element<'a, Key, Message>(
    items: &[DropdownItemData<Key>],
    on_item_toggle: &dyn Fn(Key) -> Message,
) -> Element<'a, Message>
where
    Key: Copy,
    Message: Clone + 'a,
{
    let rows: Vec<Element<'a, Message>> = items
        .iter()
        .map(|item| match item {
            DropdownItemData::SingleColumn {
                key,
                label,
                checked,
            } => dropdown_item(label, *checked, on_item_toggle(*key)),
            DropdownItemData::TwoColumn {
                key,
                name,
                right_label,
                checked,
            } => dropdown_item_two_column(name, right_label, *checked, on_item_toggle(*key)),
        })
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

pub struct CheckboxDropdown<'a, Key, Message> {
    trigger: Element<'a, Message>,
    items: Vec<DropdownItemData<Key>>,
    on_item_toggle: Box<dyn Fn(Key) -> Message + 'a>,
    /// Emitted with `Some(trigger_bounds)` to request open at those bounds, or
    /// `None` to request close.
    on_open_change: Box<dyn Fn(Option<Rectangle>) -> Message + 'a>,
    /// Whether the dropdown should currently render. Mirrors the parent's
    /// `Nokkvi.open_menu == Some(OpenMenu::CheckboxDropdown { .. })` for this
    /// widget's view.
    is_open: bool,
    /// Trigger bounds captured by the parent at open time (lives in
    /// `OpenMenu::CheckboxDropdown { trigger_bounds }`). The overlay anchors
    /// below this rectangle.
    trigger_bounds: Option<Rectangle>,
    /// Cached menu element, rebuilt each frame the dropdown is open.
    menu: Option<Element<'a, Message>>,
}

/// Tree-state. We still keep `menu_tree` because the overlay's button widgets
/// need their hover/press state to persist across frames; only `Status` is
/// lifted out (now controlled by the parent via `is_open`).
#[derive(Debug)]
struct State {
    menu_tree: widget::Tree,
}

impl State {
    fn new() -> Self {
        Self {
            menu_tree: widget::Tree::empty(),
        }
    }
}

impl<'a, Key, Message> Widget<Message, Theme, iced::Renderer> for CheckboxDropdown<'a, Key, Message>
where
    Key: Copy + 'a,
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
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
            && cursor.position_over(layout.bounds()).is_some()
        {
            let intent = if self.is_open {
                None
            } else {
                Some(layout.bounds())
            };
            shell.publish((self.on_open_change)(intent));
            shell.capture_event();
            shell.request_redraw();
            return;
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
        let our_overlay = if self.is_open {
            build_overlay(
                state,
                &mut self.menu,
                &self.items,
                &*self.on_item_toggle,
                &*self.on_open_change,
                self.trigger_bounds,
                translation,
            )
        } else {
            // Drop any cached menu element + reset the persisted tree so the
            // next open starts fresh.
            self.menu = None;
            state.menu_tree = widget::Tree::empty();
            None
        };

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

impl<'a, Key, Message> From<CheckboxDropdown<'a, Key, Message>> for Element<'a, Message>
where
    Key: Copy + 'a,
    Message: Clone + 'a,
{
    fn from(dropdown: CheckboxDropdown<'a, Key, Message>) -> Self {
        Element::new(dropdown)
    }
}

// ============================================================================
// Overlay Builder
// ============================================================================

fn build_overlay<'a, 'b, Key, Message>(
    state: &'b mut State,
    menu: &'b mut Option<Element<'a, Message>>,
    items: &[DropdownItemData<Key>],
    on_item_toggle: &dyn Fn(Key) -> Message,
    on_open_change: &'b dyn Fn(Option<Rectangle>) -> Message,
    trigger_bounds: Option<Rectangle>,
    translation: Vector,
) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>>
where
    Key: Copy,
    Message: Clone + 'a,
{
    if items.is_empty() {
        return None;
    }
    // Without trigger bounds we can't anchor the overlay; bail out (this is a
    // transient state — the parent will dispatch the bounds in the same frame).
    let trigger_bounds = trigger_bounds?;

    let m = menu.get_or_insert_with(|| build_menu_element(items, on_item_toggle));
    if state.menu_tree.children.is_empty() {
        state.menu_tree = widget::Tree::new(&*m);
    } else {
        state.menu_tree.diff(&*m as &Element<'a, Message>);
    }

    menu.as_mut().map(|m| {
        overlay::Element::new(Box::new(MenuOverlay {
            menu: m,
            state,
            on_open_change,
            trigger_bounds: trigger_bounds + translation,
        }))
    })
}

// ============================================================================
// Menu Overlay
// ============================================================================

struct MenuOverlay<'a, 'b, Message> {
    menu: &'b mut Element<'a, Message>,
    state: &'b mut State,
    on_open_change: &'b dyn Fn(Option<Rectangle>) -> Message,
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
            shell.publish((self.on_open_change)(None));
            shell.capture_event();
            shell.request_redraw();
            return;
        }

        let menu_bounds = layout.bounds();
        let cursor_over_menu = cursor.position_over(menu_bounds).is_some();
        let cursor_over_trigger = cursor.position_over(self.trigger_bounds).is_some();

        // Click outside the menu AND outside the trigger → emit close. The
        // trigger's own Widget::update toggles when clicked, so we leave that
        // case alone. Do NOT capture: if the click is also on a different
        // menu's trigger, iced dispatches overlays before the widget tree, so
        // that trigger's open emit arrives later and wins.
        if matches!(event, Event::Mouse(mouse::Event::ButtonPressed(_)))
            && !cursor_over_menu
            && !cursor_over_trigger
        {
            shell.publish((self.on_open_change)(None));
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

    /// Placeholder message used by the smoke tests below. Mirrors the shape a
    /// real call site (e.g. the library-filter popover) would use: one Toggle
    /// arm for item presses and one OpenChange arm for trigger / outside-click
    /// / Escape events. The inner payloads are never read — these tests only
    /// exercise constructor / `Into<Element>` plumbing.
    #[derive(Debug, Clone)]
    #[allow(dead_code)]
    enum TestMessage {
        Toggle(i32),
        OpenChange(Option<iced::Rectangle>),
    }

    #[test]
    fn checkbox_dropdown_compiles_with_typical_inputs() {
        // Smoke test: the public API accepts the expected shape and produces
        // a valid Element. Uses usize as the key type for compactness.
        let items: Vec<(usize, &'static str, bool)> = vec![
            (0, "Stars", true),
            (1, "Album", false),
            (2, "Duration", true),
        ];
        let _el: Element<'_, String> = checkbox_dropdown(
            "assets/icons/columns-3-cog.svg",
            "Show/hide columns",
            items,
            |key: usize| format!("toggle-{key}"),
            |bounds| match bounds {
                Some(_) => "open".to_string(),
                None => "close".to_string(),
            },
            false,
            None,
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
            Vec::<(usize, &'static str, bool)>::new(),
            |key: usize| format!("toggle-{key}"),
            |_| "noop".to_string(),
            false,
            None,
        )
        .into();
    }

    #[test]
    fn checkbox_dropdown_dynamic_compiles_with_zero_items() {
        // Degenerate-but-valid input: empty popover. Must not panic during
        // construction or conversion to Element; the overlay path will short-
        // circuit at `items.is_empty()` if it ever opens.
        let items: Vec<(i32, String, String, bool)> = Vec::new();
        let _el: Element<'_, TestMessage> = checkbox_dropdown_dynamic(
            "assets/icons/library.svg",
            "Libraries",
            items,
            TestMessage::Toggle,
            TestMessage::OpenChange,
            false,
            None,
        )
        .into();
    }

    #[test]
    fn checkbox_dropdown_dynamic_compiles_with_three_items() {
        // Typical input: three rows with owned name + right-label strings, the
        // shape the library-filter popover (Lane C) will produce. Mixed
        // checked / unchecked covers both row-icon code paths.
        let items: Vec<(i32, String, String, bool)> = vec![
            (1, "Music Library".to_string(), "13,627".to_string(), true),
            (
                2,
                "Longmont Potion Castle".to_string(),
                "412".to_string(),
                false,
            ),
            (3, "Field Recordings".to_string(), "58".to_string(), true),
        ];
        let _el: Element<'_, TestMessage> = checkbox_dropdown_dynamic(
            "assets/icons/library.svg",
            "Libraries",
            items,
            TestMessage::Toggle,
            TestMessage::OpenChange,
            true,
            Some(iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: 40.0,
                height: 40.0,
            }),
        )
        .into();
    }
}
