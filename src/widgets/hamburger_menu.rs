//! Hamburger Menu Widget
//!
//! A custom widget that renders a hamburger (☰) icon button in the nav bar.
//! When clicked, it opens a dropdown menu overlay with settings toggles.
//!
//! Controlled by the parent: `is_open` and `on_open_change` are passed in, so
//! a single root-level `Option<OpenMenu>` enforces mutual exclusion with the
//! other overlay menus (player-bar kebab, checkbox dropdowns, context menus).

use iced::{
    Element, Event, Length, Point, Radians, Rectangle, Size, Theme, Vector,
    advanced::{
        Shell,
        layout::{self, Layout},
        overlay, renderer,
        svg::{Handle, Svg as SvgData},
        widget::{self, Widget},
    },
    mouse, touch,
};

use crate::{
    theme,
    widgets::{
        menu_constants::{
            MENU_HAMBURGER_WIDTH as MENU_WIDTH, MENU_ITEM_HEIGHT, MENU_PADDING, MENU_SHADOW,
            MENU_TEXT_SIZE, inflate_for_shadow, visible_menu_bounds,
        },
        menu_dismiss,
    },
};

// ============================================================================
// Menu Item Definitions
// ============================================================================

/// Actions that the menu can emit
#[derive(Debug, Clone, Copy)]
pub enum MenuAction {
    ToggleLightMode,
    OpenSettings,
    About,
    Quit,
}

// ============================================================================
// HamburgerMenu Widget
// ============================================================================

/// Custom hamburger menu widget with overlay dropdown.
///
/// Open/closed is owned by the parent (controlled component): the parent
/// passes `is_open` derived from `Nokkvi.open_menu`, and receives open/close
/// requests through `on_open_change(bool)`.
pub struct HamburgerMenu<Message> {
    icon_handle: Handle,
    /// Called with the selected menu action
    on_action: Box<dyn Fn(MenuAction) -> Message>,
    /// Emitted with `true` to request open, `false` to request close.
    on_open_change: Box<dyn Fn(bool) -> Message>,
    /// Whether the dropdown should currently render. Mirrors the parent's
    /// `Nokkvi.open_menu == Some(OpenMenu::Hamburger)`.
    is_open: bool,
    /// Current light mode state (for label text)
    is_light_mode: bool,
    /// Icon button chassis width
    button_width: f32,
    /// Icon button chassis height
    button_height: f32,
    /// Icon size within the button
    icon_size: f32,
    /// When true, use 3D player bar button styling
    player_bar_style: bool,
}

impl<Message: Clone> HamburgerMenu<Message> {
    pub fn new(
        on_action: impl Fn(MenuAction) -> Message + 'static,
        on_open_change: impl Fn(bool) -> Message + 'static,
        is_open: bool,
        is_light_mode: bool,
    ) -> Self {
        let svg_content = crate::embedded_svg::get_svg("assets/icons/menu.svg");
        let icon_handle = Handle::from_memory(svg_content.as_bytes());

        Self {
            icon_handle,
            on_action: Box::new(on_action),
            on_open_change: Box::new(on_open_change),
            is_open,
            is_light_mode,
            button_width: 28.0,
            button_height: 28.0,
            icon_size: 18.0,
            player_bar_style: false,
        }
    }

    /// Override the chassis dimensions (default 28 × 28). Nav-bar use cases
    /// size to match the adjacent nav-tab cell so hamburger, library
    /// trigger, and tabs share the same row/column band.
    pub fn chassis(mut self, width: f32, height: f32) -> Self {
        self.button_width = width;
        self.button_height = height;
        self
    }

    /// Use player-bar button chassis (44 × 44 button, 20 px icon, `ui_radius_sm()`
    /// in rounded mode). Same flat chrome as the nav-bar use; only the
    /// size and corner radius differ.
    pub fn player_bar_style(mut self) -> Self {
        self.player_bar_style = true;
        self.button_width = 44.0;
        self.button_height = 44.0;
        self.icon_size = 20.0;
        self
    }
}

impl<Message: Clone + 'static> Widget<Message, Theme, iced::Renderer> for HamburgerMenu<Message> {
    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fixed(self.button_width),
            height: Length::Fixed(self.button_height),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut widget::Tree,
        _renderer: &iced::Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(Size::new(self.button_width, self.button_height))
    }

    fn update(
        &mut self,
        _tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. })
                if cursor.is_over(bounds) =>
            {
                shell.publish((self.on_open_change)(!self.is_open));
                shell.capture_event();
                shell.request_redraw();
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        _tree: &widget::Tree,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use iced::advanced::{Renderer, svg::Renderer as SvgRenderer};

        let bounds = layout.bounds();

        // Open = `accent_bright()` filled chrome with `bg0_hard()` icon
        // (matches the active-nav-tab visual); idle = `bg0_hard()` chrome
        // with `fg0()` icon. Hover comes from `HoverOverlay` at the call
        // site so this widget only renders open-vs-idle.
        let bg_color = if self.is_open {
            theme::accent_bright()
        } else {
            theme::bg0_hard()
        };
        let icon_color = if self.is_open {
            theme::bg0_hard()
        } else {
            theme::fg0()
        };

        // Corner radius: pill in nav-bar use (matches `.nk-nav-btn`
        // pill), `ui_radius_sm()` in the player-bar use so the larger
        // chrome doesn't look like a stretched circle. Both modes
        // resolve to 0 in flat mode.
        let radius = if self.player_bar_style {
            theme::ui_radius_sm()
        } else {
            theme::ui_radius_pill()
        };

        // Background quad — flat in both modes (no 3D bevel).
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: iced::Border {
                    radius,
                    ..Default::default()
                },
                ..Default::default()
            },
            bg_color,
        );

        // Draw centered SVG icon (shared across nav-bar and player-bar use)
        let icon_x = bounds.center_x() - self.icon_size / 2.0;
        let icon_y = bounds.center_y() - self.icon_size / 2.0;
        let icon_bounds = Rectangle {
            x: icon_x,
            y: icon_y,
            width: self.icon_size,
            height: self.icon_size,
        };

        renderer.draw_svg(
            SvgData {
                handle: self.icon_handle.clone(),
                color: Some(icon_color),
                rotation: Radians(0.0),
                opacity: 1.0,
            },
            icon_bounds,
            icon_bounds,
        );
    }

    fn mouse_interaction(
        &self,
        _tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }

    fn overlay<'b>(
        &'b mut self,
        _tree: &'b mut widget::Tree,
        layout: Layout<'_>,
        _renderer: &iced::Renderer,
        _viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
        if !self.is_open {
            return None;
        }

        let bounds = layout.bounds();
        // Position the menu below the icon, right-aligned
        let position = Point::new(
            bounds.x + bounds.width + translation.x,
            bounds.y + bounds.height + translation.y,
        );

        Some(overlay::Element::new(Box::new(MenuOverlay {
            position,
            on_action: &self.on_action,
            on_open_change: &self.on_open_change,
            is_light_mode: self.is_light_mode,
        })))
    }
}

impl<'a, Message: Clone + 'a + 'static> From<HamburgerMenu<Message>> for Element<'a, Message> {
    fn from(menu: HamburgerMenu<Message>) -> Self {
        Element::new(menu)
    }
}

// ============================================================================
// Menu Overlay
// ============================================================================

/// Menu overlay that appears below the hamburger icon
struct MenuOverlay<'a, Message> {
    position: Point,
    on_action: &'a dyn Fn(MenuAction) -> Message,
    on_open_change: &'a dyn Fn(bool) -> Message,
    is_light_mode: bool,
}

// Hamburger menu rendering constants — geometry comes from the shared
// `menu_constants` module (see imports at top); only the inner text padding
// is hamburger-specific.
const MENU_TEXT_PADDING_LEFT: f32 = 10.0;

/// Click-dispatch order for hamburger menu items. Index is the visual
/// position; `SEPARATOR_INDEX` is the position before which the divider
/// is drawn. Reordering this slice is the only way to reorder the menu.
const MENU_ITEMS: &[MenuAction] = &[
    MenuAction::ToggleLightMode,
    MenuAction::OpenSettings,
    MenuAction::About,
    MenuAction::Quit,
];

/// Number of menu items
const MENU_ITEM_COUNT: usize = MENU_ITEMS.len();
const SEPARATOR_INDEX: usize = 3; // Separator drawn before this item

const _: () = assert!(SEPARATOR_INDEX < MENU_ITEM_COUNT);
const _: () = assert!(matches!(MENU_ITEMS[MENU_ITEM_COUNT - 1], MenuAction::Quit));

impl<Message: Clone> overlay::Overlay<Message, Theme, iced::Renderer> for MenuOverlay<'_, Message> {
    fn layout(&mut self, _renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        // Extra 1px for separator line before Quit
        let menu_height = MENU_ITEM_HEIGHT * MENU_ITEM_COUNT as f32 + MENU_PADDING * 2.0 + 1.0;

        // Right-align: position.x is the right edge of the icon button
        let mut x = self.position.x - MENU_WIDTH;
        let y = self.position.y;

        // Clamp to viewport
        if x < 0.0 {
            x = 0.0;
        }
        if x + MENU_WIDTH > bounds.width {
            x = bounds.width - MENU_WIDTH;
        }

        let clamped_y = if y + menu_height > bounds.height {
            (bounds.height - menu_height).max(0.0)
        } else {
            y
        };

        inflate_for_shadow(Size::new(MENU_WIDTH, menu_height), Point::new(x, clamped_y))
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
    ) {
        let bounds = visible_menu_bounds(layout.bounds());

        // Escape / outside-press dismissal — see `widgets::menu_dismiss` for
        // the capture semantics (outside presses deliberately stay
        // uncaptured). A press with no cursor position is a no-op here.
        if menu_dismiss::handle_dismiss(
            event,
            shell,
            || {
                menu_dismiss::press_began(event)
                    && cursor.position().is_some_and(|p| !bounds.contains(p))
            },
            || (self.on_open_change)(false),
        ) {
            return;
        }

        match event {
            // Determine which menu item was clicked. Item-clicks fire on
            // left/touch press; ignore right/middle inside the menu so they
            // don't act like selections.
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                let Some(cursor_pos) = cursor.position() else {
                    return;
                };
                // Defensive: outside presses already returned above.
                if !bounds.contains(cursor_pos) {
                    return;
                }
                // Account for 1px separator before SEPARATOR_INDEX
                let mut relative_y = cursor_pos.y - bounds.y - MENU_PADDING;
                if relative_y < 0.0 {
                    return;
                }
                // Items after separator are shifted down by 1px
                let sep_y = MENU_ITEM_HEIGHT * SEPARATOR_INDEX as f32;
                if relative_y >= sep_y {
                    relative_y -= 1.0; // subtract separator height
                }
                let item_index = (relative_y / MENU_ITEM_HEIGHT) as usize;

                let action = MENU_ITEMS.get(item_index).copied();

                if let Some(action) = action {
                    shell.publish((self.on_action)(action));
                    shell.publish((self.on_open_change)(false));
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        _defaults: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        use iced::{
            advanced::{
                Renderer,
                text::{Renderer as TextRenderer, Text},
            },
            alignment,
        };

        let bounds = visible_menu_bounds(layout.bounds());
        let _ = theme; // We use our own theme functions

        // Menu chrome: `bg1()` fill with a 1 px `theme::border()` outline
        // and a `ui_radius_md()` corner in rounded mode (flat = 0). The
        // accent-bright outline of the old design read as "selected"; the
        // new flat language reserves accent for active-state surfaces
        // (tabs, buttons), not panel borders.
        // Shared menu-panel chrome — see `widgets::menu_chrome`.
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: super::menu_chrome::border(),
                shadow: MENU_SHADOW,
                ..Default::default()
            },
            super::menu_chrome::fill(),
        );

        // Menu items
        let items: [(&str, bool); MENU_ITEM_COUNT] = [
            (
                if self.is_light_mode {
                    "Dark Mode"
                } else {
                    "Light Mode"
                },
                true,
            ),
            ("Settings", true),
            ("About", true),
            ("Quit", true),
        ];
        debug_assert_eq!(
            items.len(),
            MENU_ITEM_COUNT,
            "labels array out of sync with MENU_ITEMS"
        );

        let cursor_pos = cursor.position();

        // Track extra offset for separator line
        let mut separator_offset = 0.0;

        for (i, (label, enabled)) in items.iter().enumerate() {
            // Draw separator line before Quit item — 1 px `theme::border()`
            // rule matching the panel outline color so the row band reads
            // as a continuation of the chrome.
            if i == SEPARATOR_INDEX {
                let sep_y =
                    bounds.y + MENU_PADDING + MENU_ITEM_HEIGHT * i as f32 + separator_offset;
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle {
                            x: bounds.x + 4.0,
                            y: sep_y,
                            width: bounds.width - 8.0,
                            height: 1.0,
                        },
                        ..Default::default()
                    },
                    theme::border(),
                );
                separator_offset += 1.0;
            }

            let item_y = bounds.y + MENU_PADDING + MENU_ITEM_HEIGHT * i as f32 + separator_offset;
            let inset = 1.0 + MENU_PADDING; // border + padding
            let item_bounds = Rectangle {
                x: bounds.x + inset,
                y: item_y,
                width: bounds.width - inset * 2.0,
                height: MENU_ITEM_HEIGHT,
            };

            // Hover highlight
            let is_hovered = cursor_pos.is_some_and(|p| item_bounds.contains(p));

            // Hover highlight — `bg2()` fill with `ui_radius_xs()` corners
            // so the highlight nests neatly inside a `ui_radius_md()`
            // panel without sharing the larger outer curve.
            if is_hovered && *enabled {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: item_bounds,
                        border: iced::Border {
                            radius: theme::ui_radius_xs(),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    theme::bg2(),
                );
            }

            // Text color
            let text_color = if !enabled {
                theme::fg4() // grayed out for placeholder
            } else if is_hovered {
                theme::fg0()
            } else {
                theme::fg1()
            };

            renderer.fill_text(
                Text {
                    content: label.to_string(),
                    bounds: Size::new(item_bounds.width, item_bounds.height),
                    size: MENU_TEXT_SIZE.into(),
                    line_height: iced::advanced::text::LineHeight::default(),
                    font: theme::weighted_ui_font(iced::font::Weight::Medium),
                    align_x: alignment::Horizontal::Left.into(),
                    align_y: alignment::Vertical::Center,
                    shaping: iced::advanced::text::Shaping::default(),
                    wrapping: iced::advanced::text::Wrapping::None,
                    ellipsis: iced::advanced::text::Ellipsis::default(),
                    hint_factor: Some(1.0),
                },
                Point::new(
                    item_bounds.x + MENU_TEXT_PADDING_LEFT,
                    item_bounds.center_y(),
                ),
                text_color,
                item_bounds,
            );
        }
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(visible_menu_bounds(layout.bounds())) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}
