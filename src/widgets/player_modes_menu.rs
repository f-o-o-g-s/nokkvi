//! Player Modes Menu Widget
//!
//! Kebab-style (vertical-ellipsis) dropdown anchored in the bottom player bar.
//! Holds the mode toggles that get culled out of the inline button row at
//! narrow widths. Distinct iconography (kebab vs hamburger) keeps it readable
//! even when the app's main hamburger menu is also rendered in the player bar
//! (Side / None nav layouts).
//!
//! Differences from [`super::hamburger_menu::HamburgerMenu`]:
//! - Generic over `Message`, with each row carrying its own action — no fixed
//!   `MenuAction` enum. Caller passes a `Vec<ModeMenuRow<Message>>`.
//! - Rows render a leading 14×14 check-or-empty icon (mirroring
//!   `widgets::checkbox_dropdown::dropdown_item`) so toggle state is visible
//!   on a single glance.
//! - When the trigger is closed, an accent dot is drawn in the icon's
//!   top-right when any item in `rows` is active. This is the at-a-glance
//!   "something is on" affordance for hidden mode state.
//! - 3D bevel trigger chrome (matching the rest of the player bar buttons).

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
// Constants
// ============================================================================

const TRIGGER_BUTTON_SIZE: f32 = 44.0;
const TRIGGER_ICON_SIZE: f32 = 20.0;
const TRIGGER_BORDER_WIDTH: f32 = 2.0;

/// Diameter of the badge dot drawn in the trigger's top-right corner when any
/// menu item is active and the menu is closed.
const BADGE_DIAMETER: f32 = 8.0;
/// Inset from the trigger's right and top edges to the badge dot's outer edge.
const BADGE_INSET: f32 = 5.0;

const MENU_WIDTH: f32 = 220.0;
const MENU_ITEM_HEIGHT: f32 = 28.0;
const MENU_PADDING: f32 = 4.0;
const MENU_TEXT_SIZE: f32 = 13.0;
const MENU_CHECK_ICON_SIZE: f32 = 14.0;
const MENU_ROW_INSET: f32 = 6.0;
const MENU_CHECK_GAP: f32 = 8.0;

/// Total vertical space taken by a separator row (1px line + padding above/below).
const SEPARATOR_HEIGHT: f32 = 1.0;
const SEPARATOR_VPAD: f32 = 4.0;
const SEPARATOR_ROW_HEIGHT: f32 = SEPARATOR_HEIGHT + SEPARATOR_VPAD * 2.0;

// ============================================================================
// Public API
// ============================================================================

/// One row in the menu — either a togglable item or a divider.
#[derive(Debug, Clone)]
pub enum ModeMenuRow<Message> {
    Item(ModeMenuItem<Message>),
    Separator,
}

/// A togglable menu row. `label` typically embeds the current state
/// (e.g. `"Shuffle: On"`) so the user can read mode status without relying
/// solely on the check icon.
#[derive(Debug, Clone)]
pub struct ModeMenuItem<Message> {
    pub label: String,
    pub is_active: bool,
    pub on_action: Message,
}

/// Convenience builder for an item row.
pub(crate) fn mode_menu_item<Message>(
    label: impl Into<String>,
    is_active: bool,
    on_action: Message,
) -> ModeMenuRow<Message> {
    ModeMenuRow::Item(ModeMenuItem {
        label: label.into(),
        is_active,
        on_action,
    })
}

/// Convenience builder for a separator row.
pub(crate) fn mode_menu_separator<Message>() -> ModeMenuRow<Message> {
    ModeMenuRow::Separator
}

// ============================================================================
// Widget
// ============================================================================

pub struct PlayerModesMenu<Message> {
    icon_handle: Handle,
    check_handle: Handle,
    rows: Vec<ModeMenuRow<Message>>,
    /// Whether the dropdown should currently render. Mirrors the parent's
    /// `Nokkvi.open_menu == Some(OpenMenu::PlayerModes)`.
    is_open: bool,
    /// Emitted with `true` to request open, `false` to request close.
    on_open_change: Box<dyn Fn(bool) -> Message>,
}

impl<Message: Clone + 'static> PlayerModesMenu<Message> {
    pub fn new(
        rows: Vec<ModeMenuRow<Message>>,
        on_open_change: impl Fn(bool) -> Message + 'static,
        is_open: bool,
    ) -> Self {
        let icon_svg = crate::embedded_svg::get_svg("assets/icons/ellipsis-vertical.svg");
        let check_svg = crate::embedded_svg::get_svg("assets/icons/check.svg");
        Self {
            icon_handle: Handle::from_memory(icon_svg.as_bytes()),
            check_handle: Handle::from_memory(check_svg.as_bytes()),
            rows,
            is_open,
            on_open_change: Box::new(on_open_change),
        }
    }

    fn any_active(&self) -> bool {
        self.rows.iter().any(|row| match row {
            ModeMenuRow::Item(item) => item.is_active,
            ModeMenuRow::Separator => false,
        })
    }

    fn menu_inner_height(&self) -> f32 {
        let mut h = 0.0;
        for row in &self.rows {
            h += match row {
                ModeMenuRow::Item(_) => MENU_ITEM_HEIGHT,
                ModeMenuRow::Separator => SEPARATOR_ROW_HEIGHT,
            };
        }
        h
    }
}

impl<Message: Clone + 'static> Widget<Message, Theme, iced::Renderer> for PlayerModesMenu<Message> {
    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fixed(TRIGGER_BUTTON_SIZE),
            height: Length::Fixed(TRIGGER_BUTTON_SIZE),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut widget::Tree,
        _renderer: &iced::Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(Size::new(TRIGGER_BUTTON_SIZE, TRIGGER_BUTTON_SIZE))
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

        // 3D bevel chrome — pressed appearance when open, raised when closed.
        let (raised_top_left, raised_bottom_right) = theme::border_3d_raised();
        let (top_left_color, bottom_right_color) = if self.is_open {
            (raised_bottom_right, raised_top_left)
        } else {
            (raised_top_left, raised_bottom_right)
        };
        let bg_color = if self.is_open {
            theme::accent_bright()
        } else {
            theme::bg1()
        };
        let icon_color = if self.is_open {
            theme::bg0_hard()
        } else {
            theme::fg1()
        };

        super::three_d_helpers::draw_3d_bevel(
            renderer,
            bounds,
            TRIGGER_BORDER_WIDTH,
            bg_color,
            top_left_color,
            bottom_right_color,
        );

        // Centered kebab icon.
        let icon_x = bounds.center_x() - TRIGGER_ICON_SIZE / 2.0;
        let icon_y = bounds.center_y() - TRIGGER_ICON_SIZE / 2.0;
        let icon_bounds = Rectangle {
            x: icon_x,
            y: icon_y,
            width: TRIGGER_ICON_SIZE,
            height: TRIGGER_ICON_SIZE,
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

        // Active-state badge dot — only when closed (open state shows the
        // checkmarks directly so the badge would be redundant).
        if !self.is_open && self.any_active() {
            let badge_x = bounds.x + bounds.width - BADGE_INSET - BADGE_DIAMETER;
            let badge_y = bounds.y + BADGE_INSET;
            let badge_bounds = Rectangle {
                x: badge_x,
                y: badge_y,
                width: BADGE_DIAMETER,
                height: BADGE_DIAMETER,
            };
            renderer.fill_quad(
                renderer::Quad {
                    bounds: badge_bounds,
                    border: iced::Border {
                        radius: (BADGE_DIAMETER / 2.0).into(),
                        width: 1.0,
                        color: theme::bg0_hard(),
                    },
                    ..Default::default()
                },
                theme::accent_bright(),
            );
        }
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
        // Bottom-right anchor of the trigger button (icon sits in player bar);
        // overlay positions itself ABOVE the bar since there's no room below.
        let anchor = Point::new(
            bounds.x + bounds.width + translation.x,
            bounds.y + translation.y,
        );

        Some(overlay::Element::new(Box::new(MenuOverlay {
            on_open_change: &self.on_open_change,
            check_handle: &self.check_handle,
            rows: &self.rows,
            menu_inner_height: self.menu_inner_height(),
            anchor,
        })))
    }
}

impl<'a, Message: Clone + 'a + 'static> From<PlayerModesMenu<Message>> for Element<'a, Message> {
    fn from(menu: PlayerModesMenu<Message>) -> Self {
        Element::new(menu)
    }
}

// ============================================================================
// Menu Overlay
// ============================================================================

struct MenuOverlay<'a, Message> {
    on_open_change: &'a dyn Fn(bool) -> Message,
    check_handle: &'a Handle,
    rows: &'a [ModeMenuRow<Message>],
    menu_inner_height: f32,
    /// Trigger top-right corner in screen coordinates. Overlay anchors its
    /// bottom-right to this point so it floats above the player bar with the
    /// kebab icon serving as the visual hinge.
    anchor: Point,
}

impl<Message: Clone> MenuOverlay<'_, Message> {
    /// Y offset (relative to overlay bounds top) where each row begins.
    fn row_offsets(&self) -> Vec<f32> {
        let mut offsets = Vec::with_capacity(self.rows.len());
        let mut y = MENU_PADDING;
        for row in self.rows {
            offsets.push(y);
            y += match row {
                ModeMenuRow::Item(_) => MENU_ITEM_HEIGHT,
                ModeMenuRow::Separator => SEPARATOR_ROW_HEIGHT,
            };
        }
        offsets
    }
}

impl<Message: Clone> overlay::Overlay<Message, Theme, iced::Renderer> for MenuOverlay<'_, Message> {
    fn layout(&mut self, _renderer: &iced::Renderer, viewport: Size) -> layout::Node {
        let menu_height = self.menu_inner_height + MENU_PADDING * 2.0;

        // Right-align: anchor.x is the trigger's right edge.
        let mut x = self.anchor.x - MENU_WIDTH;
        // Float above the player bar — anchor.y is the trigger's top edge,
        // so subtract the menu height (plus a 4px gap).
        let mut y = self.anchor.y - menu_height - 4.0;

        // Clamp to viewport with a small inset.
        let padding = 4.0;
        if x < padding {
            x = padding;
        }
        if x + MENU_WIDTH > viewport.width - padding {
            x = (viewport.width - padding - MENU_WIDTH).max(padding);
        }
        if y < padding {
            // Fall back to anchoring below the trigger if the window is so
            // short that opening upward clips off the top.
            y = (self.anchor.y + TRIGGER_BUTTON_SIZE + 4.0)
                .min(viewport.height - menu_height - padding)
                .max(padding);
        }

        layout::Node::new(Size::new(MENU_WIDTH, menu_height)).move_to(Point::new(x, y))
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
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(keyboard::key::Named::Escape),
                ..
            }) => {
                shell.publish((self.on_open_change)(false));
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::ButtonPressed(_))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                let Some(cursor_pos) = cursor.position() else {
                    return;
                };

                if !bounds.contains(cursor_pos) {
                    // Click outside menu → emit close. Do NOT capture so the
                    // click can also reach a different menu's trigger; iced
                    // dispatches overlays before the widget tree, so the
                    // trigger's open emit arrives later and wins.
                    shell.publish((self.on_open_change)(false));
                    shell.request_redraw();
                    return;
                }

                // Item-clicks fire on left/touch press only.
                if !matches!(
                    event,
                    Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                        | Event::Touch(touch::Event::FingerPressed { .. })
                ) {
                    return;
                }

                // Map cursor y to a row index, accounting for variable-height
                // separator rows.
                let offsets = self.row_offsets();
                let local_y = cursor_pos.y - bounds.y;
                let mut hit: Option<usize> = None;
                for (i, &row_y) in offsets.iter().enumerate() {
                    let row_h = match self.rows[i] {
                        ModeMenuRow::Item(_) => MENU_ITEM_HEIGHT,
                        ModeMenuRow::Separator => SEPARATOR_ROW_HEIGHT,
                    };
                    if local_y >= row_y && local_y < row_y + row_h {
                        hit = Some(i);
                        break;
                    }
                }

                if let Some(idx) = hit
                    && let ModeMenuRow::Item(item) = &self.rows[idx]
                {
                    shell.publish(item.on_action.clone());
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
        _theme: &Theme,
        _defaults: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        use iced::{
            advanced::{
                Renderer,
                svg::Renderer as SvgRenderer,
                text::{Renderer as TextRenderer, Text},
            },
            alignment,
        };

        let bounds = layout.bounds();

        // Menu background (matches HamburgerMenu chrome).
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

        let cursor_pos = cursor.position();
        let offsets = self.row_offsets();

        for (i, row) in self.rows.iter().enumerate() {
            let row_y = bounds.y + offsets[i];
            match row {
                ModeMenuRow::Separator => {
                    let sep_y = row_y + SEPARATOR_VPAD;
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle {
                                x: bounds.x + MENU_ROW_INSET,
                                y: sep_y,
                                width: bounds.width - MENU_ROW_INSET * 2.0,
                                height: SEPARATOR_HEIGHT,
                            },
                            ..Default::default()
                        },
                        theme::bg3(),
                    );
                }
                ModeMenuRow::Item(item) => {
                    let inset = MENU_ROW_INSET;
                    let item_bounds = Rectangle {
                        x: bounds.x + inset,
                        y: row_y,
                        width: bounds.width - inset * 2.0,
                        height: MENU_ITEM_HEIGHT,
                    };

                    let is_hovered = cursor_pos.is_some_and(|p| item_bounds.contains(p));
                    if is_hovered {
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

                    // Leading check icon (or invisible spacer if inactive) so
                    // labels stay aligned across the column.
                    let check_x = item_bounds.x + 6.0;
                    let check_y = item_bounds.y + (MENU_ITEM_HEIGHT - MENU_CHECK_ICON_SIZE) / 2.0;
                    let check_bounds = Rectangle {
                        x: check_x,
                        y: check_y,
                        width: MENU_CHECK_ICON_SIZE,
                        height: MENU_CHECK_ICON_SIZE,
                    };
                    if item.is_active {
                        renderer.draw_svg(
                            SvgData {
                                handle: self.check_handle.clone(),
                                color: Some(theme::fg0()),
                                rotation: Radians(0.0),
                                opacity: 1.0,
                            },
                            check_bounds,
                            check_bounds,
                        );
                    }

                    let text_x = check_bounds.x + MENU_CHECK_ICON_SIZE + MENU_CHECK_GAP;
                    let text_color = if is_hovered {
                        theme::fg0()
                    } else {
                        theme::fg1()
                    };
                    let text_bounds_size = Size::new(
                        item_bounds.x + item_bounds.width - text_x,
                        item_bounds.height,
                    );
                    renderer.fill_text(
                        Text {
                            content: item.label.clone(),
                            bounds: text_bounds_size,
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
                        Point::new(text_x, item_bounds.center_y()),
                        text_color,
                        item_bounds,
                    );
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    type TestMessage = String;

    fn make_item(label: &str, active: bool) -> ModeMenuRow<TestMessage> {
        mode_menu_item(label, active, label.to_string())
    }

    fn make_menu(rows: Vec<ModeMenuRow<TestMessage>>) -> PlayerModesMenu<TestMessage> {
        PlayerModesMenu::new(rows, |_| String::new(), false)
    }

    #[test]
    fn any_active_returns_false_for_all_inactive() {
        let menu = make_menu(vec![make_item("a", false), make_item("b", false)]);
        assert!(!menu.any_active());
    }

    #[test]
    fn any_active_returns_true_when_at_least_one_is_active() {
        let menu = make_menu(vec![
            make_item("a", false),
            make_item("b", true),
            make_item("c", false),
        ]);
        assert!(menu.any_active());
    }

    #[test]
    fn any_active_ignores_separators() {
        let menu = make_menu(vec![
            make_item("a", false),
            mode_menu_separator(),
            make_item("b", false),
        ]);
        assert!(!menu.any_active());
    }

    #[test]
    fn menu_inner_height_sums_rows_correctly() {
        let menu = make_menu(vec![
            make_item("a", false),
            make_item("b", true),
            mode_menu_separator(),
            make_item("c", false),
        ]);
        let expected = MENU_ITEM_HEIGHT * 3.0 + SEPARATOR_ROW_HEIGHT;
        assert_eq!(menu.menu_inner_height(), expected);
    }

    #[test]
    fn empty_rows_produce_zero_inner_height() {
        let menu = make_menu(Vec::new());
        assert_eq!(menu.menu_inner_height(), 0.0);
        assert!(!menu.any_active());
    }
}
