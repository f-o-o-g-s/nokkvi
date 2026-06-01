//! Shared geometry + color recipe for the menu-style checkbox glyph.
//!
//! The glyph is a filled `accent_bright()` rounded square with a centered
//! `check.svg` in `bg0()` when checked, and an outlined 1.5 px `fg2()` rounded
//! square (transparent fill) when unchecked. Three menu sites render it:
//!
//! - [`element`] — composed-`Element` adapter used by `checkbox_dropdown.rs`
//!   for both the single-column show/hide-columns rows (`dropdown_item`) and
//!   the two-column library-filter rows (`dropdown_item_two_column`).
//! - [`draw`] — imperative adapter used by `player_modes_menu.rs`, whose
//!   `MenuOverlay::draw` hand-paints every row via `renderer.fill_quad` +
//!   `renderer.draw_svg` rather than composing an `Element` tree.
//!
//! Mirrors the `menu_constants.rs` / `menu_chrome.rs` split: shared geometry
//! consts + thin adapters, two render shapes, ONE source of truth so the
//! `Element` path and the `fill_quad` path cannot drift. The `const _: () =
//! assert!(…)` blocks pin the glyph's internal coherence; the cross-site
//! anti-drift guarantee is structural — both call sites read these consts and
//! no parallel literals survive at either site.
//!
//! Out of scope for this module: the `slot_list.rs` multi-select family (size
//! 18, theme-aware `ui_radius_xs()` corners, row-selection semantics) and the
//! `text_input_dialog.rs` native `iced::widget::checkbox` (font-codepoint
//! check) — both are deliberately distinct visual families.

use iced::{
    Border, Color, Element, Length, Point, Radians, Rectangle,
    advanced::{
        Renderer, renderer,
        svg::{Handle, Renderer as SvgRenderer, Svg as SvgData},
    },
    widget::{Space, container},
};

use crate::theme;

/// Edge length of the rounded square (px). Was `CHECKBOX_GLYPH_SIZE`
/// (`checkbox_dropdown.rs`) and `MENU_CHECKBOX_SIZE` (`player_modes_menu.rs`).
/// Deliberately 2 px larger than `menu_constants::MENU_ICON_SIZE` (14.0) so
/// the box reads heavier than a bare check glyph.
pub(crate) const GLYPH_SIZE: f32 = 16.0;

/// Corner radius (px). FIXED, deliberately NOT theme-aware. Both legacy sites
/// hardcoded 3.0 and neither gated on `ROUNDED_MODE`; `theme::ui_radius_xs()`
/// is 4.0 rounded / 0.0 flat (not 3.0), so swapping to it would change the
/// look in both modes and erase the boundary with the `slot_list` multi-select
/// family. Keep fixed.
pub(crate) const CORNER_RADIUS: f32 = 3.0;

/// Centered `check.svg` edge length (px). Was the inline `GLYPH_SIZE - 4.0`
/// and `MENU_CHECKBOX_INNER_CHECK_SIZE`. Named here so the `-4.0` gap stops
/// being a magic literal at the call sites.
pub(crate) const INNER_CHECK_SIZE: f32 = 12.0;

/// Unchecked outline width (px). Was the inline `1.5` and
/// `MENU_CHECKBOX_BORDER_WIDTH`.
pub(crate) const BORDER_WIDTH: f32 = 1.5;

/// Single owner of the check-icon path string. Keeping it a `&str` literal
/// here means the embedded-SVG registration test
/// (`all_svg_paths_in_source_are_registered`) still sees the literal so the
/// icon stays registered. Both adapters source it: [`element`] feeds it to
/// `svg_widget`; `player_modes_menu` builds its cached `Handle` from
/// `get_svg(CHECK_SVG_PATH)`.
pub(crate) const CHECK_SVG_PATH: &str = "assets/icons/check.svg";

/// Resolved per-state colors + outline width. Internal — both adapters route
/// through it so the checked/unchecked recipe lives at exactly one site.
#[derive(Clone, Copy)]
struct Palette {
    /// Box fill: `accent_bright()` checked, transparent unchecked.
    fill: Color,
    /// Box outline: `accent_bright()` checked (0 px), `fg2()` unchecked.
    border: Color,
    /// Outline width: 0 checked, [`BORDER_WIDTH`] unchecked.
    border_width: f32,
    /// Centered check tint: `bg0()` checked, transparent unchecked (unused
    /// when unchecked — no check is drawn).
    check_tint: Color,
}

fn palette(checked: bool) -> Palette {
    if checked {
        Palette {
            fill: theme::accent_bright(),
            border: theme::accent_bright(),
            border_width: 0.0,
            check_tint: theme::bg0(),
        }
    } else {
        Palette {
            fill: Color::TRANSPARENT,
            border: theme::fg2(),
            border_width: BORDER_WIDTH,
            check_tint: Color::TRANSPARENT,
        }
    }
}

/// Composed-`Element` adapter for the dropdown rows.
///
/// The outer widget is a `container` in BOTH branches, so the row's leading
/// cell keeps a stable widget type across checked/unchecked re-renders (the
/// render-stability rule that protects sibling `text_input` focus). The
/// `Message` bound is intentionally just `'a` — the glyph is a leaf that emits
/// nothing, so a `Clone` bound would be a gratuitous over-constraint.
pub(crate) fn element<'a, Message: 'a>(checked: bool) -> Element<'a, Message> {
    let p = palette(checked);

    let inner: Element<'a, Message> = if checked {
        crate::embedded_svg::svg_widget(CHECK_SVG_PATH)
            .width(Length::Fixed(INNER_CHECK_SIZE))
            .height(Length::Fixed(INNER_CHECK_SIZE))
            .style(move |_theme, _status| iced::widget::svg::Style {
                color: Some(p.check_tint),
            })
            .into()
    } else {
        Space::new().into()
    };

    // `.center(GLYPH_SIZE)` centers the 12 px check inside the 16 px box for
    // the checked branch and is a harmless no-op for the empty Space — keep it
    // or the check top-left-aligns (~2 px shift).
    container(inner)
        .width(Length::Fixed(GLYPH_SIZE))
        .height(Length::Fixed(GLYPH_SIZE))
        .center(Length::Fixed(GLYPH_SIZE))
        .style(move |_| container::Style {
            // Explicit None (not Some(TRANSPARENT)) when unchecked so this
            // byte-matches the legacy two-column glyph's `background: None`.
            background: if checked { Some(p.fill.into()) } else { None },
            border: Border {
                radius: CORNER_RADIUS.into(),
                width: p.border_width,
                color: p.border,
            },
            ..Default::default()
        })
        .into()
}

/// Imperative adapter for hand-drawn overlays (`player_modes_menu`).
///
/// Takes the glyph's top-left `origin`, NOT a pre-built `Rectangle`: the box
/// edge length ([`GLYPH_SIZE`]) lives only inside this module, so the caller
/// cannot pass a mismatched size that would off-center the inner check. The
/// caller owns the cached `check_handle` and passes `&Handle`; we clone it into
/// `SvgData`, preserving the per-frame handle-reuse discipline.
pub(crate) fn draw(
    renderer: &mut iced::Renderer,
    origin: Point,
    check_handle: &Handle,
    checked: bool,
) {
    let p = palette(checked);
    let bounds = Rectangle {
        x: origin.x,
        y: origin.y,
        width: GLYPH_SIZE,
        height: GLYPH_SIZE,
    };

    renderer.fill_quad(
        renderer::Quad {
            bounds,
            border: Border {
                radius: CORNER_RADIUS.into(),
                width: p.border_width,
                color: p.border,
            },
            ..Default::default()
        },
        p.fill,
    );

    if checked {
        let inset = (GLYPH_SIZE - INNER_CHECK_SIZE) / 2.0;
        let inner_bounds = Rectangle {
            x: bounds.x + inset,
            y: bounds.y + inset,
            width: INNER_CHECK_SIZE,
            height: INNER_CHECK_SIZE,
        };
        renderer.draw_svg(
            SvgData {
                handle: check_handle.clone(),
                color: Some(p.check_tint),
                rotation: Radians(0.0),
                opacity: 1.0,
            },
            inner_bounds,
            inner_bounds,
        );
    }
}

// Compile-time drift guards — `menu_constants.rs` idiom. (Runtime
// `assert!(<const>)` is forbidden by clippy `assertions_on_constants = "deny"`,
// so these live in `const { … }` form.)
const _: () = assert!(
    INNER_CHECK_SIZE < GLYPH_SIZE,
    "INNER_CHECK_SIZE must fit inside the GLYPH_SIZE box"
);
const _: () = assert!(
    INNER_CHECK_SIZE == GLYPH_SIZE - 4.0,
    "INNER_CHECK_SIZE pins the historical GLYPH_SIZE-4 inner-check gap; rederive both call sites if you change this"
);
const _: () = assert!(BORDER_WIDTH > 0.0, "unchecked outline must be visible");
const _: () = assert!(
    CORNER_RADIUS > 0.0 && CORNER_RADIUS < GLYPH_SIZE / 2.0,
    "stays a rounded square, never a pill (so it reads as a checkbox)"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn element_builds_checked() {
        // Exercises the checked container + svg branch and Into<Element>
        // plumbing with a unit Message (proves the looser `Message: 'a`
        // bound resolves without a Clone bound).
        let _e: Element<'_, ()> = element::<()>(true);
    }

    #[test]
    fn element_builds_unchecked() {
        let _e: Element<'_, ()> = element::<()>(false);
    }

    #[test]
    fn palette_checked_fills_accent_and_tints_bg0() {
        let p = palette(true);
        assert_eq!(p.border_width, 0.0);
        assert_eq!(p.fill, theme::accent_bright());
        assert_eq!(p.border, theme::accent_bright());
        assert_eq!(p.check_tint, theme::bg0());
    }

    #[test]
    fn palette_unchecked_is_transparent_with_fg2_border() {
        let p = palette(false);
        assert_eq!(p.fill, Color::TRANSPARENT);
        assert_eq!(p.border, theme::fg2());
        assert_eq!(p.border_width, BORDER_WIDTH);
    }

    #[test]
    fn shared_consts_match_legacy_literals() {
        // Documents the migration: these were the literals open-coded at the
        // two legacy sites before centralization. assert_eq! on named f32
        // consts is allowed (clippy `assertions_on_constants` only bans
        // runtime `assert!(<bare-const>)`; the geometry-relationship guards
        // above use the sanctioned `const _: () = assert!(…)` form).
        assert_eq!(GLYPH_SIZE, 16.0);
        assert_eq!(CORNER_RADIUS, 3.0);
        assert_eq!(INNER_CHECK_SIZE, 12.0);
        assert_eq!(BORDER_WIDTH, 1.5);
        assert_eq!(INNER_CHECK_SIZE, GLYPH_SIZE - 4.0);
    }
}
