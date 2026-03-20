//! Shared 3D bevel drawing helpers for custom widgets.
//!
//! Consolidates the 5-quad flat-mode bevel pattern (background + 4 directional
//! border quads) and the rounded-mode fallback (single quad with corner radius)
//! used by `ThreeDButton`, `ThreeDIconButton`, and `HamburgerMenu`.

use iced::{Color, Rectangle, advanced::renderer};

use crate::theme;

/// Draw a 3D beveled button background.
///
/// **Flat mode:** 5-quad bevel — inset background + top/left highlight + bottom/right shadow.
/// **Rounded mode:** single quad with uniform border color + corner radius.
///
/// Callers determine `top_left_color` / `bottom_right_color` based on pressed/active
/// state (swap them for a "pushed in" look vs. raised).
pub(crate) fn draw_3d_bevel(
    renderer: &mut iced::Renderer,
    bounds: Rectangle,
    border_width: f32,
    bg_color: Color,
    top_left_color: Color,
    bottom_right_color: Color,
) {
    use iced::advanced::Renderer;

    let radius = theme::ui_border_radius();

    if theme::is_rounded_mode() {
        // Rounded mode: single quad with uniform border
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: iced::Border {
                    color: top_left_color,
                    width: border_width,
                    radius,
                },
                ..Default::default()
            },
            bg_color,
        );
    } else {
        // Flat mode: 5-quad 3D bevel
        // Main background (inset by border_width)
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x: bounds.x + border_width,
                    y: bounds.y + border_width,
                    width: bounds.width - border_width * 2.0,
                    height: bounds.height - border_width * 2.0,
                },
                ..Default::default()
            },
            bg_color,
        );

        // Top border
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x: bounds.x,
                    y: bounds.y,
                    width: bounds.width,
                    height: border_width,
                },
                ..Default::default()
            },
            top_left_color,
        );

        // Left border
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x: bounds.x,
                    y: bounds.y,
                    width: border_width,
                    height: bounds.height,
                },
                ..Default::default()
            },
            top_left_color,
        );

        // Bottom border
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x: bounds.x,
                    y: bounds.y + bounds.height - border_width,
                    width: bounds.width,
                    height: border_width,
                },
                ..Default::default()
            },
            bottom_right_color,
        );

        // Right border
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    x: bounds.x + bounds.width - border_width,
                    y: bounds.y,
                    width: border_width,
                    height: bounds.height,
                },
                ..Default::default()
            },
            bottom_right_color,
        );
    }
}
