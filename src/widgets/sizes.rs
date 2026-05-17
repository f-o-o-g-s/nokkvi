//! Shared widget-geometry constants.
//!
//! Consolidates icon-button / toolbar-button literals that recur across
//! multiple widget files. Restraint: only the obviously-duplicated sizes
//! live here. One-off literals stay in their owning widget; premature
//! consolidation is an anti-pattern per CLAUDE.md ("Reuse existing
//! patterns. Check the codebase before building something new.").
//!
//! Module-level UPPER_SNAKE matches the longstanding flat-literal widget
//! constant pattern (`NAV_BAR_HEIGHT`, `MAX_BARS`, `TOOLBAR_HEIGHT`, etc.).

/// Standard square icon-button footprint (px) used in view headers, the
/// default-playlist chip, player-bar transports, the checkbox dropdown
/// trigger, etc. Picked once to keep the visual chrome consistent.
pub(crate) const ICON_BUTTON_SIZE: f32 = 40.0;

/// Player-bar mode-toggle / kebab button footprint (px). Slightly larger
/// than `ICON_BUTTON_SIZE` to give the kebab menu a recognizable visual
/// anchor distinct from the other player-bar controls.
pub(crate) const TOOLBAR_BUTTON_SIZE: f32 = 44.0;

/// Modal header icon-button footprint (px), used by close/copy/save buttons
/// in `about_modal`, `info_modal`, and `eq_modal`. Smaller than
/// `ICON_BUTTON_SIZE` because modal header chrome runs tighter than the
/// view-header chrome.
pub(crate) const MODAL_ICON_BUTTON_SIZE: f32 = 28.0;

/// Small modal icon glyph size (px) — used for the "copy", "folder-open",
/// chevron, etc. SVGs inside modal headers.
pub(crate) const MODAL_ICON_SIZE_SMALL: f32 = 14.0;

/// Large modal icon glyph size (px) — used for the close (X) SVG in modal
/// headers, which is rendered slightly larger so it remains the most
/// prominent affordance even when adjacent to copy/save/folder buttons.
pub(crate) const MODAL_ICON_SIZE_LARGE: f32 = 16.0;

// Compile-time invariants — these are constants, so they belong in
// `const { ... }` rather than runtime assertions (clippy enforces this via
// `assertions_on_constants = "deny"`).

/// Toolbar buttons (e.g. the kebab) deliberately sit above the standard
/// icon button in the visual hierarchy.
const _: () = assert!(
    TOOLBAR_BUTTON_SIZE > ICON_BUTTON_SIZE,
    "TOOLBAR_BUTTON_SIZE must remain larger than ICON_BUTTON_SIZE",
);

/// Modal header chrome runs tighter than view-header chrome.
const _: () = assert!(
    MODAL_ICON_BUTTON_SIZE < ICON_BUTTON_SIZE,
    "MODAL_ICON_BUTTON_SIZE must remain smaller than ICON_BUTTON_SIZE",
);

/// Small modal icon glyph must stay smaller than the dominant close-glyph.
const _: () = assert!(
    MODAL_ICON_SIZE_SMALL < MODAL_ICON_SIZE_LARGE,
    "MODAL_ICON_SIZE_SMALL must remain smaller than MODAL_ICON_SIZE_LARGE",
);

#[cfg(test)]
mod tests {
    use super::*;

    /// `ICON_BUTTON_SIZE` MUST match the literal 40 px used by `view_header`'s
    /// `flat_icon_button` chrome (the most visible call site). If a future
    /// refactor wants to change either, this test surfaces the drift.
    #[test]
    fn icon_button_size_matches_view_header_chrome() {
        assert_eq!(
            ICON_BUTTON_SIZE, 40.0,
            "ICON_BUTTON_SIZE pinned to the historic 40px header chrome",
        );
    }

    /// Modal icon glyphs come in two sizes — the small one is for secondary
    /// affordances (copy, folder-open), the large one for the dominant
    /// dismiss glyph (X). The pair must remain distinct.
    #[test]
    fn modal_icon_sizes_are_paired() {
        assert_eq!(
            MODAL_ICON_SIZE_SMALL, 14.0,
            "small modal icon stays at 14 px"
        );
        assert_eq!(
            MODAL_ICON_SIZE_LARGE, 16.0,
            "large modal icon stays at 16 px"
        );
    }
}
