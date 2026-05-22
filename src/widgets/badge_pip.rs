//! Shared badge-pip helper.
//!
//! A badge pip is the small accent-colored dot drawn in the top-right
//! corner of a trigger button to signal "something is on" without
//! opening the menu. The pattern originated in
//! [`super::player_modes_menu`] (the kebab "modes" trigger) and is now
//! shared with [`super::library_filter_trigger`] (the nav-bar library
//! selector) so both surfaces use the same pixel dimensions, the same
//! accent-bright fill, and the same 1 px bg0-hard hairline that keeps
//! the dot legible against accent-on-accent backgrounds.
//!
//! Geometry is pinned at the module level so a future visual tweak
//! lands once. Callers pass the trigger bounds they're decorating and
//! the helper renders the pip in screen-space coordinates.
//!
//! ```text
//! ┌────────────┐
//! │ icon   ●   │  ← BADGE_DIAMETER dot, inset BADGE_INSET from
//! │            │     the trigger's top-right corner.
//! └────────────┘
//! ```
//!
//! Use only when the visible icon already conveys the trigger's
//! identity (i.e. the pip is a *modifier*, not the primary signal).
//! For chrome that needs a count or label, render text alongside the
//! icon and call this helper to overlay the dot afterwards.

use iced::{
    Rectangle,
    advanced::{Renderer, renderer},
};

use crate::theme;

/// Diameter (in pixels) of the badge dot.
///
/// Picked to read at 28 px and 44 px button sizes without crowding the
/// icon centerline. If a future variant needs a different size, take a
/// `diameter: f32` parameter rather than introducing a parallel
/// constant — drift between badge sites is a visual-consistency bug.
pub(crate) const BADGE_DIAMETER: f32 = 8.0;

/// Inset (in pixels) from the trigger's right and top edges to the
/// outer edge of the badge dot. Mirrors the original
/// `player_modes_menu` value so the two surfaces look identical.
pub(crate) const BADGE_INSET: f32 = 5.0;

/// Draw an accent-bright pip in the top-right corner of `trigger_bounds`.
///
/// The pip is a filled circle (`accent_bright()`) with a 1 px hairline
/// border in `bg0_hard()` — the hairline keeps the dot visible against
/// either the idle bg0-hard chrome or the open-state accent-bright
/// chrome.
///
/// Renders nothing if the trigger bounds is degenerate (zero width or
/// height); callers don't need to guard.
pub(crate) fn draw_badge_pip(renderer: &mut iced::Renderer, trigger_bounds: Rectangle) {
    if trigger_bounds.width <= 0.0 || trigger_bounds.height <= 0.0 {
        return;
    }

    let badge_x = trigger_bounds.x + trigger_bounds.width - BADGE_INSET - BADGE_DIAMETER;
    let badge_y = trigger_bounds.y + BADGE_INSET;
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
