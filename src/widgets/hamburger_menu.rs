//! Hamburger Menu Widget
//!
//! A custom widget that renders a hamburger (☰) icon button in the nav bar.
//! When clicked, it opens a dropdown menu overlay with settings toggles.
//! Click-outside-close is handled natively via the iced overlay system.

use iced::{
    Element, Event, Length, Point, Radians, Rectangle, Size, Theme, Vector,
    advanced::{
        Shell,
        layout::{self, Layout},
        overlay, renderer,
        svg::{Handle, Svg as SvgData},
        widget::{self, Widget},
    },
    keyboard, mouse, touch,
};

use crate::theme;

// ============================================================================
// Widget State
// ============================================================================

#[derive(Debug, Clone, Copy, Default)]
struct State {
    is_open: bool,
}

// ============================================================================
// Menu Item Definitions
// ============================================================================

/// Actions that the menu can emit
#[derive(Debug, Clone)]
pub enum MenuAction {
    ToggleLightMode,
    ToggleSoundEffects,
    OpenSettings,
    Quit,
}

// ============================================================================
// HamburgerMenu Widget
// ============================================================================

/// Custom hamburger menu widget with overlay dropdown
pub struct HamburgerMenu<Message> {
    icon_handle: Handle,
    /// Called with the selected menu action
    on_action: Box<dyn Fn(MenuAction) -> Message>,
    /// Current light mode state (for label text)
    is_light_mode: bool,
    /// Current SFX enabled state (for label text)
    sfx_enabled: bool,
    /// Icon button size
    button_size: f32,
    /// Icon size within the button
    icon_size: f32,
    /// When true, use 3D player bar button styling
    player_bar_style: bool,
}

impl<Message: Clone> HamburgerMenu<Message> {
    pub fn new(
        on_action: impl Fn(MenuAction) -> Message + 'static,
        is_light_mode: bool,
        sfx_enabled: bool,
    ) -> Self {
        let svg_content = crate::embedded_svg::get_svg("assets/icons/menu.svg");
        let icon_handle = Handle::from_memory(svg_content.as_bytes());

        Self {
            icon_handle,
            on_action: Box::new(on_action),
            is_light_mode,
            sfx_enabled,
            button_size: 28.0,
            icon_size: 18.0,
            player_bar_style: false,
        }
    }

    /// Use 3D player bar button styling (44x44, bevel borders)
    pub fn player_bar_style(mut self) -> Self {
        self.player_bar_style = true;
        self.button_size = 44.0;
        self.icon_size = 20.0;
        self
    }
}

impl<Message: Clone + 'static> Widget<Message, Theme, iced::Renderer> for HamburgerMenu<Message> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fixed(self.button_size),
            height: Length::Fixed(self.button_size),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut widget::Tree,
        _renderer: &iced::Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(Size::new(self.button_size, self.button_size))
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
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                if cursor.is_over(bounds) {
                    state.is_open = !state.is_open;
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
        use iced::advanced::{Renderer, svg::Renderer as SvgRenderer};

        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let is_hovered = cursor.is_over(bounds);

        let icon_color = if self.player_bar_style {
            // 3D player bar button styling (matches ThreeDIconButton)
            let border_width = 2.0;
            let (raised_top_left, raised_bottom_right) = theme::border_3d_raised();
            let (top_left_color, bottom_right_color) = if state.is_open {
                (raised_bottom_right, raised_top_left)
            } else {
                (raised_top_left, raised_bottom_right)
            };

            let bg_color = if state.is_open {
                theme::accent_bright()
            } else {
                theme::bg1()
            };

            let icon_color = if state.is_open {
                theme::bg0_hard()
            } else {
                theme::fg1()
            };

            // Draw 3D beveled background (shared helper)
            super::three_d_helpers::draw_3d_bevel(
                renderer,
                bounds,
                border_width,
                bg_color,
                top_left_color,
                bottom_right_color,
            );

            icon_color
        } else {
            // Original flat nav bar styling
            let bg_color = if state.is_open {
                theme::accent_bright()
            } else if is_hovered {
                theme::bg2()
            } else {
                theme::bg0_hard()
            };

            let icon_color = if state.is_open {
                theme::bg0()
            } else {
                theme::fg1()
            };

            // Background quad
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border: iced::Border {
                        radius: theme::ui_border_radius(),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                bg_color,
            );

            icon_color
        };

        // Draw centered SVG icon (shared across both styles)
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
        tree: &'b mut widget::Tree,
        layout: Layout<'_>,
        _renderer: &iced::Renderer,
        _viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
        let state = tree.state.downcast_mut::<State>();

        if !state.is_open {
            return None;
        }

        let bounds = layout.bounds();
        // Position the menu below the icon, right-aligned
        let position = Point::new(
            bounds.x + bounds.width + translation.x,
            bounds.y + bounds.height + translation.y,
        );

        Some(overlay::Element::new(Box::new(MenuOverlay {
            state,
            position,
            on_action: &self.on_action,
            is_light_mode: self.is_light_mode,
            sfx_enabled: self.sfx_enabled,
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
    state: &'a mut State,
    position: Point,
    on_action: &'a dyn Fn(MenuAction) -> Message,
    is_light_mode: bool,
    sfx_enabled: bool,
}

/// Constants for menu item rendering
const MENU_WIDTH: f32 = 180.0;
const MENU_ITEM_HEIGHT: f32 = 28.0;
const MENU_PADDING: f32 = 4.0;
const MENU_TEXT_SIZE: f32 = 13.0;
const MENU_TEXT_PADDING_LEFT: f32 = 10.0;

/// Number of menu items (3 settings + separator + quit)
const MENU_ITEM_COUNT: usize = 4;
const SEPARATOR_INDEX: usize = 3; // Separator drawn before this item

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

        layout::Node::new(Size::new(MENU_WIDTH, menu_height)).move_to(Point::new(x, clamped_y))
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
    ) {
        let bounds = layout.bounds();

        match event {
            // Escape key → close
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            }) => {
                self.state.is_open = false;
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                if let Some(cursor_pos) = cursor.position() {
                    if !bounds.contains(cursor_pos) {
                        // Click outside menu -> close
                        self.state.is_open = false;
                        shell.capture_event();
                        shell.request_redraw();
                        return;
                    }

                    // Determine which menu item was clicked
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

                    let action = match item_index {
                        0 => Some(MenuAction::ToggleLightMode),
                        1 => Some(MenuAction::ToggleSoundEffects),
                        2 => Some(MenuAction::OpenSettings),
                        3 => Some(MenuAction::Quit),
                        _ => None,
                    };

                    if let Some(action) = action {
                        shell.publish((self.on_action)(action));
                        self.state.is_open = false;
                        shell.capture_event();
                        shell.request_redraw();
                    }
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

        let bounds = layout.bounds();
        let _ = theme; // We use our own theme functions

        // Menu background
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: iced::Border {
                    width: 1.0,
                    color: theme::accent_bright(),
                    radius: theme::ui_border_radius(),
                },
                ..Default::default()
            },
            theme::bg1(),
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
            (
                if self.sfx_enabled {
                    "UI SFX: On"
                } else {
                    "UI SFX: Off"
                },
                true,
            ),
            ("Settings", true),
            ("Quit", true),
        ];

        let cursor_pos = cursor.position();

        // Track extra offset for separator line
        let mut separator_offset = 0.0;

        for (i, (label, enabled)) in items.iter().enumerate() {
            // Draw separator line before Quit item
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
                    theme::bg3(),
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

            if is_hovered && *enabled {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: item_bounds,
                        border: iced::Border {
                            radius: theme::ui_border_radius(),
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
                    font: iced::font::Font {
                        weight: iced::font::Weight::Medium,
                        ..theme::ui_font()
                    },
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
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}
