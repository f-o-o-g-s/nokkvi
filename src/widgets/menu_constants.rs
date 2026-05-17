//! Shared sizing constants for menu-style widgets.
//!
//! Consolidates the `MENU_*` literals that were previously duplicated across
//! `context_menu.rs`, `checkbox_dropdown.rs`, `hamburger_menu.rs`, and
//! `player_modes_menu.rs`. Hamburger and player-modes menus deliberately use
//! distinct widths (180 vs 220 px) because the latter carries longer rows like
//! "Visualizer: On" — they are kept as separate named constants here so a
//! future agent does not collapse them by mistake.
//!
//! Module-level UPPER_SNAKE matches the longstanding flat-literal widget
//! constant pattern (`NAV_BAR_HEIGHT`, `MAX_BARS`, `TOOLBAR_HEIGHT`, etc.).
//! These are intentionally not part of `theme.rs` because they are widget
//! geometry, not theme palette.

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
