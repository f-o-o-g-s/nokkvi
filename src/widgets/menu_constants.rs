//! Shared sizing constants and shadow-halo helpers for menu-style widgets.
//!
//! Consolidates the `MENU_*` literals that were previously duplicated across
//! `context_menu.rs`, `checkbox_dropdown.rs`, `hamburger_menu.rs`, and
//! `player_modes_menu.rs`. Hamburger and player-modes menus deliberately use
//! distinct widths (180 vs 220 px) because the latter carries longer rows like
//! "Visualizer: On" — they are kept as separate named constants here so a
//! future agent does not collapse them by mistake.
//!
//! Also owns the [`MENU_SHADOW`] drop-shadow + the inflate/recover helpers
//! (`inflate_for_shadow`, `inflate_for_shadow_around_child`,
//! `visible_menu_bounds`, `visible_menu_layout`) that every menu overlay
//! uses to dodge Iced's per-overlay scissor — see those items for the
//! full mechanism.
//!
//! Module-level UPPER_SNAKE matches the longstanding flat-literal widget
//! constant pattern (`NAV_BAR_HEIGHT`, `MAX_BARS`, `TOOLBAR_HEIGHT`, etc.).
//! These are intentionally not part of `theme.rs` because they are widget
//! geometry, not theme palette.

use iced::{
    Point, Rectangle, Size,
    advanced::{Layout, layout},
};

/// Canonical minimum width for context-menu and checkbox-dropdown menus (px).
///
/// `hamburger_menu` and `player_modes_menu` use distinct values because they
/// are anchor-positioned (right-aligned in the nav bar / player bar) and need
/// known fixed widths, not minimums. See `MENU_HAMBURGER_WIDTH` and
/// `MENU_PLAYER_MODES_WIDTH` for those.
pub(crate) const MENU_MIN_WIDTH: f32 = 180.0;

/// Fixed width of the right-anchored hamburger menu dropdown (px).
///
/// Sized to fit "Switch to Light Mode" / "Switch to Dark Mode" plus the
/// 10-px left padding without truncation, matching the visual chrome the
/// human owner has already approved across builds.
pub(crate) const MENU_HAMBURGER_WIDTH: f32 = 180.0;

/// Fixed width of the player-modes (kebab) menu dropdown (px).
///
/// Wider than `MENU_HAMBURGER_WIDTH` because rows here embed live state
/// (e.g. "Crossfade: 8s", "Repeat: One") that runs longer than the
/// hamburger menu's static labels. The divergence is intentional —
/// do not collapse the two constants into one.
pub(crate) const MENU_PLAYER_MODES_WIDTH: f32 = 220.0;

/// Leading icon size inside menu rows (px). Matches the check / chevron / kebab
/// glyphs across `context_menu`, `checkbox_dropdown`, and `player_modes_menu`.
pub(crate) const MENU_ICON_SIZE: f32 = 14.0;

/// Body text size inside menu rows (px).
pub(crate) const MENU_TEXT_SIZE: f32 = 13.0;

/// Vertical height allocated to a single menu item (px).
pub(crate) const MENU_ITEM_HEIGHT: f32 = 28.0;

/// Vertical padding applied above and below the menu's outermost border (px).
pub(crate) const MENU_PADDING: f32 = 4.0;

/// Drop-shadow elevation applied to every dropdown / kebab / right-click /
/// popover menu surface — `context_menu`, `checkbox_dropdown` (incl. the
/// library-selector popover), `hamburger_menu`, and `player_modes_menu`.
///
/// Style is inspired by the scrub-handle shadow at `progress_bar.rs:563`
/// (downward-only offset, semi-transparent black, soft blur), scaled up
/// for a panel-sized surface rather than a 14 px round slider handle.
/// Theme-agnostic black α reads correctly on both light and dark `bg1`
/// menu backgrounds, so the constant is `const`-evaluated rather than
/// theme-derived.
pub(crate) const MENU_SHADOW: iced::Shadow = iced::Shadow {
    color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.5),
    offset: iced::Vector::new(0.0, 4.0),
    blur_radius: 6.0,
};

/// Halo padding (px) added to every side of a menu overlay's `layout::Node`
/// so the [`MENU_SHADOW`] tail isn't scissored by the per-overlay
/// `with_layer(layout.bounds(), …)` clip Iced wraps around every
/// `overlay::Overlay::draw` in `core/src/overlay/nested.rs`.
///
/// Each `MenuOverlay` reports `layout::Node` bounds inflated by this padding,
/// then derives the visible menu rectangle by shrinking the bounds back —
/// hit testing, background quads, items, and forwarded child layouts all use
/// the visible rectangle, while only the outer scissor sees the inflated
/// bounds. Mirrors the `shadow_overflow = 6.0` trick used for the scrub-handle
/// shadow at `progress_bar.rs:537`.
///
/// Sized as `ceil(blur_radius + |offset.y|)` so the worst-case axis (the
/// downward offset + blur) fits cleanly; the same value is applied uniformly
/// to all sides for code simplicity — the over-inflated top / sides cost
/// only invisible scissor area, not extra drawing.
///
/// Kept module-private — call sites use [`inflate_for_shadow`] /
/// [`inflate_for_shadow_around_child`] (producers) and
/// [`visible_menu_bounds`] / [`visible_menu_layout`] (recoverers) instead
/// of touching the raw constant.
const MENU_SHADOW_PADDING: f32 = 10.0;

// Compile-time invariants — these are pure constants so they live in
// `const { ... }` rather than runtime assertions (clippy enforces this via
// `assertions_on_constants = "deny"`).

/// Player-modes menu is intentionally wider than the hamburger menu because
/// its rows embed live state ("Crossfade: 8s", etc.) that overflows the
/// hamburger width. Pinning this with a const_assert prevents a future
/// agent from collapsing the two width constants.
const _: () = assert!(
    MENU_PLAYER_MODES_WIDTH > MENU_HAMBURGER_WIDTH,
    "MENU_PLAYER_MODES_WIDTH must stay larger than MENU_HAMBURGER_WIDTH",
);

/// `MENU_SHADOW_PADDING` must accommodate the shadow's full extent (offset
/// magnitude + blur radius) on the worst-case axis, otherwise the per-overlay
/// scissor will clip the shadow halo and the elevation effect disappears.
/// Pinned here so a future agent tuning the shadow values gets a compile-time
/// nudge to update the padding to match.
const _: () = assert!(
    MENU_SHADOW_PADDING >= MENU_SHADOW.blur_radius + MENU_SHADOW.offset.y,
    "MENU_SHADOW_PADDING must cover MENU_SHADOW.blur_radius + offset.y",
);

/// `MENU_SHADOW_PADDING`'s simple uniform-shrink derivation assumes the
/// shadow displaces along the +Y axis only. A non-zero horizontal offset
/// or a negative vertical offset would flip the worst-case axis and
/// require rederiving the padding (currently `blur_radius + offset.y`).
const _: () = assert!(
    MENU_SHADOW.offset.x == 0.0,
    "MENU_SHADOW offset is vertical-only by convention",
);
const _: () = assert!(
    MENU_SHADOW.offset.y >= 0.0,
    "MENU_SHADOW must displace downward — MENU_SHADOW_PADDING math assumes a non-negative offset.y",
);

/// Inflate a leaf menu overlay's `layout::Node` so the [`MENU_SHADOW`] halo
/// isn't scissored. Used by the manual-draw overlays (`hamburger_menu`,
/// `player_modes_menu`), which have no inner child widget to wrap.
///
/// `visible_size` and `position` describe the menu the way the surrounding
/// math computes it (clamped, anchored, etc.); this helper expands by
/// `MENU_SHADOW_PADDING` on every side and shifts the origin so the visible
/// rect lands exactly where the caller asked. Recover the visible rect on
/// the read side via [`visible_menu_bounds`].
pub(crate) fn inflate_for_shadow(visible_size: Size, position: Point) -> layout::Node {
    let pad = MENU_SHADOW_PADDING;
    let inflated_size = Size::new(
        visible_size.width + 2.0 * pad,
        visible_size.height + 2.0 * pad,
    );
    layout::Node::new(inflated_size).move_to(Point::new(position.x - pad, position.y - pad))
}

/// Inflate a child-forwarding menu overlay's `layout::Node` around an
/// existing inner `menu_node`. Used by `context_menu` and `checkbox_dropdown`,
/// which host real child `Element`s that need their own coordinate space.
///
/// The returned node has exactly one child positioned at `(pad, pad)` inside
/// inflated bounds shifted to land the visible rect at `position`. Recover
/// the child layout on the read side via [`visible_menu_layout`].
pub(crate) fn inflate_for_shadow_around_child(
    menu_node: layout::Node,
    position: Point,
) -> layout::Node {
    let pad = MENU_SHADOW_PADDING;
    let menu_size = menu_node.size();
    let inflated_size = Size::new(menu_size.width + 2.0 * pad, menu_size.height + 2.0 * pad);
    let positioned_child = menu_node.move_to(Point::new(pad, pad));
    layout::Node::with_children(inflated_size, vec![positioned_child])
        .move_to(Point::new(position.x - pad, position.y - pad))
}

/// Recover the visible menu rectangle from an inflated overlay layout
/// rectangle by shrinking by `MENU_SHADOW_PADDING` on every side. Use in
/// the leaf overlays (`hamburger_menu`, `player_modes_menu`) wherever
/// non-shadow rendering or hit-testing needs the visible bounds — the
/// inflated rect is for the scissor only.
pub(crate) fn visible_menu_bounds(inflated: Rectangle) -> Rectangle {
    Rectangle {
        x: inflated.x + MENU_SHADOW_PADDING,
        y: inflated.y + MENU_SHADOW_PADDING,
        width: inflated.width - 2.0 * MENU_SHADOW_PADDING,
        height: inflated.height - 2.0 * MENU_SHADOW_PADDING,
    }
}

/// Recover the inner visible `Layout` from an inflated child-forwarding
/// overlay layout. Mirror of [`visible_menu_bounds`] for the overlays whose
/// inflated node was built via [`inflate_for_shadow_around_child`].
///
/// Used by `context_menu` and `checkbox_dropdown` in `update` / `draw` /
/// `mouse_interaction` to extract the layout to forward to the hosted child
/// `Element`. The `expect` is unreachable in any non-corrupt iced version —
/// `inflate_for_shadow_around_child` always wraps exactly one child.
pub(crate) fn visible_menu_layout<'a>(inflated: Layout<'a>) -> Layout<'a> {
    inflated
        .children()
        .next()
        .expect("inflated layout always has exactly one menu child")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hamburger and player-modes menus must keep distinct widths so a
    /// future agent doesn't accidentally collapse them — the player-modes
    /// menu embeds live state labels that overflow the hamburger width.
    #[test]
    fn hamburger_and_player_modes_widths_diverge_intentionally() {
        assert_ne!(
            MENU_HAMBURGER_WIDTH, MENU_PLAYER_MODES_WIDTH,
            "menu widths must stay distinct — see module docs"
        );
    }

    /// `MENU_MIN_WIDTH` matches `MENU_HAMBURGER_WIDTH` today; that's not a
    /// hard requirement, but it's the historical convention — pinning so a
    /// future drift surfaces in CI rather than at visual inspection.
    #[test]
    fn min_width_aligns_with_hamburger_width() {
        assert_eq!(
            MENU_MIN_WIDTH, MENU_HAMBURGER_WIDTH,
            "context-menu / checkbox-dropdown min-width historically matches the hamburger width",
        );
    }
}
