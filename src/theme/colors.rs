//! Theme color accessors and color math — palette tokens, blending,
//! contrast helpers, accent-wash, and the derived highlight-fill family.

use iced::Color;

use super::{is_light_mode, read_color};

// ============================================================================
// Background Colors
// ============================================================================

#[inline]
pub(crate) fn bg0_hard() -> Color {
    read_color(|t| t.bg0_hard)
}
#[inline]
pub(crate) fn bg0() -> Color {
    read_color(|t| t.bg0)
}
#[inline]
pub(crate) fn bg0_soft() -> Color {
    read_color(|t| t.bg0_soft)
}
#[inline]
pub(crate) fn bg1() -> Color {
    read_color(|t| t.bg1)
}
#[inline]
pub(crate) fn bg2() -> Color {
    read_color(|t| t.bg2)
}
#[inline]
pub(crate) fn bg3() -> Color {
    read_color(|t| t.bg3)
}

// ============================================================================
// Foreground Colors
// ============================================================================

#[inline]
pub(crate) fn fg4() -> Color {
    read_color(|t| t.fg4)
}
#[inline]
pub(crate) fn fg3() -> Color {
    read_color(|t| t.fg3)
}
#[inline]
pub(crate) fn fg2() -> Color {
    read_color(|t| t.fg2)
}
#[inline]
pub(crate) fn fg1() -> Color {
    read_color(|t| t.fg1)
}
#[inline]
pub(crate) fn fg0() -> Color {
    read_color(|t| t.fg0)
}

// ============================================================================
// Accent Colors
// ============================================================================

#[inline]
pub(crate) fn accent() -> Color {
    read_color(|t| t.accent)
}
#[inline]
pub(crate) fn accent_bright() -> Color {
    read_color(|t| t.accent_bright)
}
#[inline]
pub(crate) fn accent_border_light() -> Color {
    read_color(|t| t.accent_border_light)
}

// The now-playing and selected slot highlights are no longer per-theme stored
// colors. They are derived from the live accent tokens with built-in contrast
// guards — see the "Highlight-fill family" section below
// (`playing_fill` / `selected_fill_resolved` / `legible_text_on`). The
// `accent.now_playing` / `accent.selected` TOML fields are still parsed for
// round-trip compatibility (the `star.base` precedent) but no longer consumed.

/// Semi-transparent accent color for text input selection highlights.
///
/// Iced's `text_input::Style` has no `selected_text_color` field — the text
/// always renders with `style.value` color even during selection. Using an
/// opaque accent background makes text unreadable when theme foreground and
/// accent colors have poor contrast (e.g. peach text on green highlight).
/// A translucent tint lets the underlying background show through, keeping
/// the text readable across all theme combinations.
#[inline]
pub(crate) fn selection_color() -> Color {
    let mut c = accent_bright();
    c.a = 0.35;
    c
}

// ============================================================================
// Semantic Colors
// ============================================================================

#[inline]
pub(crate) fn danger() -> Color {
    read_color(|t| t.danger)
}
#[inline]
pub(crate) fn danger_bright() -> Color {
    read_color(|t| t.danger_bright)
}
#[inline]
pub(crate) fn success() -> Color {
    read_color(|t| t.success)
}
#[inline]
pub(crate) fn warning() -> Color {
    read_color(|t| t.warning)
}
#[inline]
pub(crate) fn warning_bright() -> Color {
    read_color(|t| t.warning_bright)
}
// Base `star()` accessor was retained alongside `star_bright()` for any
// future surface that wanted both ends of the star ratings palette, but
// only `star_bright()` is consumed (slot-list star renders + metadata
// pills). Removed during the cleanup; `palette.star.base` still lives in
// the TOML schema so existing themes round-trip without a migration.
#[inline]
pub(crate) fn star_bright() -> Color {
    read_color(|t| t.star_bright)
}

// ============================================================================
// Chrome Border (1px hairline separators)
// ============================================================================

/// Hairline border color used by chrome dividers (between nav bars, list
/// rows, capsules). Per-theme in TOML, falls back to a darkened
/// `bg0_hard()` when unset. Replaces hard-coded `#1a2024`-style dividers.
#[inline]
pub(crate) fn border() -> Color {
    read_color(|t| t.border)
}

// ============================================================================
// Color Blending Helpers
// ============================================================================
// Used by `darken()` (the border-token fallback in theme_config and the
// `status_strip_bg` derivation), and by `slot_list`'s depth-darkening of
// now-playing rows. The flat redesign removed the 3D bevel chrome, taking
// the old `border_3d_*` / sRGB `lighten` helpers with it. `lighten_oklch()`
// is the perceptual replacement for "a brighter version of the same accent"
// (the now-playing glow lights), brightening without the sRGB-toward-white
// desaturation/hue-drift the old helper suffered.

/// Blend a color toward a target color by the given factor (0.0 = base, 1.0 = target).
#[inline]
pub(super) fn blend_toward(base: Color, target: Color, factor: f32) -> Color {
    Color {
        r: base.r + (target.r - base.r) * factor,
        g: base.g + (target.g - base.g) * factor,
        b: base.b + (target.b - base.b) * factor,
        a: base.a, // Keep original alpha
    }
}

/// Darken a color by blending it toward black.
#[inline]
pub(crate) fn darken(color: Color, amount: f32) -> Color {
    blend_toward(color, Color::BLACK, amount)
}

/// Lighten a color perceptually by lifting its Oklch lightness toward white by
/// `amount` (0.0 = unchanged, 1.0 = `l` driven to 1.0), holding hue and chroma.
///
/// The perceptual counterpart to `darken()` and the replacement for the removed
/// sRGB toward-white `lighten`. Naive sRGB lerp-toward-white desaturates as it
/// brightens (chroma collapses to 0 at white) and drifts hue through the blend,
/// so a saturated accent reads as a washed-out grey tint rather than a brighter
/// version of itself. Lifting Oklch `l` while keeping `c`/`h` yields a genuinely
/// brighter same-hue accent. Alpha is preserved (Oklch round-trips it). On a
/// low-chroma/near-white accent the lift is near a no-op, which is correct.
#[inline]
pub(crate) fn lighten_oklch(color: Color, amount: f32) -> Color {
    let mut oklch = color.into_oklch();
    oklch.l = (oklch.l + (1.0 - oklch.l) * amount).min(1.0);
    Color::from_oklch(oklch)
}

// ============================================================================
// Accent-wash family (single source of truth)
// ============================================================================
//
// The playlist / queue "Playing From" header banner and the `HoverOverlay`
// hover/press feedback are the same recipe — a faint pull of the live theme
// `accent()` over a surface. Both route through `accent_wash` / `hover_tint`
// so a 22nd theme or a new wash site inherits the look for free and cannot
// silently re-fork the recipe (the duplication this consolidated removed).

/// Faint accent-wash factor for the playlist / queue "Playing From" header
/// banner — `bg0_soft()` lerped this far toward `accent()`.
pub(crate) const HEADER_WASH: f32 = 0.07;

/// Blend `base` toward the active theme `accent()` by `factor` (0.0 = base,
/// 1.0 = pure accent). Preserves `base`'s alpha (opaque base → opaque wash).
/// The single home for the accent-wash family.
#[inline]
pub(crate) fn accent_wash(base: Color, factor: f32) -> Color {
    blend_toward(base, accent(), factor)
}

/// Opaque pigment the hover/press overlay deposits over a NEUTRAL surface.
///
/// `HoverOverlay` applies its own hover/press alpha on top, so the live
/// src-over composite equals `lerp(surface, this, alpha)` — the same
/// accent wash as the header, viewed at the overlay's alpha. Light mode
/// pulls toward `accent()`; dark toward the brighter `accent_bright()` so it
/// still reads over dark chrome. Fixes the pre-redesign light-mode no-op
/// (a near-black tint at 10% over a near-`bg0_hard()` surface was invisible).
#[inline]
pub(crate) fn hover_tint() -> Color {
    if is_light_mode() {
        accent()
    } else {
        accent_bright()
    }
}

/// Hover/press pigment for a surface that is ALREADY filled with
/// `accent_bright()` — active nav tabs and active player mode toggles.
///
/// Depositing the accent wash there is a near-no-op (accent over accent), so
/// these surfaces get a CONTRASTING neutral pull instead: `bg0_hard()` in
/// light mode, `fg0()` in dark. Call sites opt in via
/// [`HoverOverlay::on_accent_surface`] with their own active flag.
#[inline]
pub(crate) fn hover_tint_on_accent() -> Color {
    if is_light_mode() { bg0_hard() } else { fg0() }
}

// ============================================================================
// Contrast helpers
// ============================================================================
//
// Several shipped light-mode palettes tune `accent.now_playing` / `selected`
// to muted, low-saturation hues that match the surrounding chrome aesthetic.
// When those colors are reused as *text* (metadata strip title/artist), the
// luminance ends up too close to `bg0_hard()` and the text becomes unreadable
// even though the same accent reads fine as a fill or border. The helpers
// below let a render path nudge such a color back into a legible band
// without disturbing dark-mode behavior or the original theme palette.

/// WCAG 2.1 relative luminance.
#[inline]
pub(super) fn relative_luminance(c: Color) -> f32 {
    let channel = |v: f32| {
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
}

/// WCAG contrast ratio between two colors. Result is in `[1.0, 21.0]`.
#[inline]
pub(super) fn contrast_ratio(a: Color, b: Color) -> f32 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    let (light, dark) = if la >= lb { (la, lb) } else { (lb, la) };
    (light + 0.05) / (dark + 0.05)
}

/// Minimum contrast we aim for when used as small (10 px) UI text — WCAG AA
/// for normal text.
pub(super) const LEGIBLE_TEXT_CONTRAST: f32 = 4.5;

/// Push `color` toward whichever pure extreme (black or white) maximizes its
/// WCAG contrast against `reference`, stopping as soon as the contrast clears
/// `floor` (or at a near-full blend, which is guaranteed to clear ~4.58:1).
///
/// Bidirectional and surface-aware: the target extreme is the one farthest in
/// luminance from `reference` (= [`legible_text_on`] of the reference), so a
/// too-light color over a light surface darkens, and a too-dark color over a
/// dark surface lightens. This single primitive backs legible strip / chrome
/// text in BOTH modes — it replaces the old light-only `darken_until_legible`,
/// which could not lift a too-dark color on a dark theme.
pub(super) fn legible_against(color: Color, reference: Color, floor: f32) -> Color {
    if contrast_ratio(color, reference) >= floor {
        return color;
    }
    let target = legible_text_on(reference);
    let mut adjusted = color;
    let mut amount: f32 = 0.05;
    while amount < 0.99 {
        adjusted = blend_toward(color, target, amount);
        if contrast_ratio(adjusted, reference) >= floor {
            return adjusted;
        }
        amount += 0.05;
    }
    adjusted
}

/// Make `color` legible as small strip / chrome text over the surface it is
/// ACTUALLY painted on (`status_strip_bg()` for the status strip, `bg0_hard()`
/// for the nav bar / mini-player). Surface-aware and bidirectional — fixes
/// both the old dark-mode no-op and the wrong-surface measurement.
#[inline]
pub(crate) fn legible_strip_text(color: Color, surface: Color) -> Color {
    legible_against(color, surface, LEGIBLE_TEXT_CONTRAST)
}

// ============================================================================
// Highlight-fill family (single source of truth)
// ============================================================================
//
// Selected/center and now-playing/expanded slots render as OPAQUE accent fills
// with the row text forced to a guaranteed-legible color. Unlike the
// translucent hover wash (which leaves the row's own text untouched), the
// readability of a fill is governed by its FORCED TEXT, so this family pairs a
// derived fill with `legible_text_on`. Both fills are derived from the live
// accent tokens — the per-theme `now_playing` / `selected` TOML values are no
// longer consumed — so a new theme inherits readable, mutually-distinct
// highlights for free instead of hand-tuning two colors that could (and did,
// e.g. Everforest light / Kanagawa Dragon dark) land unreadable.

/// Minimum WCAG contrast kept between the now-playing fill and the selected
/// fill, so the playing row stays distinguishable from the keyboard-cursor row
/// when both are visible at once.
pub(super) const FILL_DISTINCT_CONTRAST: f32 = 1.5;

/// Pick pure black or white as the forced text/ink for an opaque highlight
/// `fill`, choosing whichever yields more WCAG contrast. Provably ≥ 4.58:1
/// against ANY fill (the black/white contrast curves cross at luminance
/// ≈ 0.179, where both equal 4.58), so the forced text always clears AA — even
/// for a future low-contrast accent.
#[inline]
pub(crate) fn legible_text_on(fill: Color) -> Color {
    if contrast_ratio(fill, Color::BLACK) >= contrast_ratio(fill, Color::WHITE) {
        Color::BLACK
    } else {
        Color::WHITE
    }
}

/// A visible ring for an opaque highlight chip: blends `fill` toward its own
/// forced text color (the guaranteed-contrasting extreme), so the border is
/// always perceptible against the fill regardless of theme. Every current caller
/// (the now-playing / expanded-parent rows and the drag-preview ghost) passes
/// `strength` 1.0 for a max-contrast ring; lower strengths give a subtler ring.
/// In-list selection no longer uses this — it is border-only over the plain row
/// bg, with a contrast-floored accent ring (see [`selection_ring_on`]).
#[inline]
pub(crate) fn highlight_border(fill: Color, strength: f32) -> Color {
    blend_toward(fill, legible_text_on(fill), strength)
}

/// Minimum WCAG non-text / UI-component contrast (3:1) kept between the in-list
/// selection ring and the row background it is drawn on. A border-only selection
/// has no fill swap or text recolor to fall back on, so the ring is the SOLE cue
/// and must clear this floor on every theme — unlike the theme picker swatch list
/// it mirrors, which leans on an always-visible swatch + center-anchor + hint.
pub(super) const SELECTION_RING_MIN_CONTRAST: f32 = 3.0;

/// Resolve the in-list selection ring color for a row whose background is `bg`.
/// Starts from `accent_bright` (so the ring reads as "the theme accent", exactly
/// like the theme picker swatch list) and only nudges it toward `bg`'s
/// contrasting extreme when the raw accent sits too close to `bg` to register as
/// a thin 2 px ring — so a selection is never invisible on a low-contrast theme
/// (e.g. Firmium light's near-bg gold, Kanagawa-Dragon dark's near-bg navy). On
/// the great majority of themes the accent already clears the floor and is
/// returned unchanged. Pure in its inputs so the all-themes gauntlet can sweep
/// it; [`selection_ring_on`] is the live-theme wrapper.
pub(super) fn selection_ring(accent_bright: Color, bg: Color) -> Color {
    legible_against(accent_bright, bg, SELECTION_RING_MIN_CONTRAST)
}

/// Live-theme [`selection_ring`]: the contrast-floored accent ring for a selected
/// slot drawn over background `bg` (the row's per-depth `bg0`/`bg1`/`bg2`).
#[inline]
pub(crate) fn selection_ring_on(bg: Color) -> Color {
    selection_ring(accent_bright(), bg)
}

/// Resolve the `(now_playing, selected)` fill pair from the accent tokens,
/// applying the distinctness separator once so the pair is always mutually
/// consistent. `selected` anchors on the louder `accent_bright`; `playing` is
/// the calmer `accent`, receded toward `bg` if needed to clear
/// [`FILL_DISTINCT_CONTRAST`] WITHOUT crossing selected's luminance (crossing
/// would invert the "cursor is loud, playing is ambient" hierarchy). If
/// receding stalls (accent and accent_bright sit at near-equal luminance),
/// `selected` is pushed toward the extreme farthest from `playing`
/// ([`legible_text_on`] of playing) instead — which always reaches the floor.
pub(super) fn resolve_highlight_fills(
    accent: Color,
    accent_bright: Color,
    bg: Color,
) -> (Color, Color) {
    let sel = accent_bright;
    let mut play = accent;
    if contrast_ratio(play, sel) >= FILL_DISTINCT_CONTRAST {
        return (play, sel);
    }
    let l_sel = relative_luminance(sel);
    let started_below = relative_luminance(play) < l_sel;
    // Step A: recede playing toward the chrome bg, never crossing selected.
    let mut amount: f32 = 0.05;
    let mut best = play;
    while amount < 0.90 {
        let candidate = blend_toward(play, bg, amount);
        let l_c = relative_luminance(candidate);
        let crossed = if started_below {
            l_c >= l_sel
        } else {
            l_c <= l_sel
        };
        if crossed {
            break;
        }
        best = candidate;
        if contrast_ratio(best, sel) >= FILL_DISTINCT_CONTRAST {
            return (best, sel);
        }
        amount += 0.05;
    }
    play = best;
    // Step B: receding stalled — push selected farther from playing in the
    // direction that PRESERVES the hierarchy (lighter stays lighter, darker
    // stays darker), so the separator never inverts "cursor loud / playing
    // ambient". Saturated accent pairs have ample headroom toward their nearer
    // extreme; the distinctness guard test catches any palette that doesn't.
    let target = if relative_luminance(sel) >= relative_luminance(play) {
        Color::WHITE
    } else {
        Color::BLACK
    };
    let mut s = sel;
    let mut sa: f32 = 0.05;
    while sa < 0.99 {
        s = blend_toward(sel, target, sa);
        if contrast_ratio(s, play) >= FILL_DISTINCT_CONTRAST {
            break;
        }
        sa += 0.05;
    }
    (play, s)
}

/// Now-playing / expanded-parent slot fill — derived, distinctness-resolved.
#[inline]
pub(crate) fn playing_fill() -> Color {
    resolve_highlight_fills(accent(), accent_bright(), bg0_hard()).0
}

/// Loud accent fill for the now-playing row and the drag-preview ghost —
/// derived, distinctness-resolved. (Despite the historical name, in-list
/// selection is now border-only and does NOT use this; see
/// [`selection_ring_on`].)
#[inline]
pub(crate) fn selected_fill_resolved() -> Color {
    resolve_highlight_fills(accent(), accent_bright(), bg0_hard()).1
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clearly chromatic mid-blue, so hue/chroma assertions are meaningful.
    fn accent_sample() -> Color {
        Color::from_rgb(0.20, 0.45, 0.85)
    }

    #[test]
    fn lighten_oklch_zero_is_identity() {
        let c = accent_sample();
        let out = lighten_oklch(c, 0.0);
        // Oklch round-trip is not bit-exact; a small epsilon is expected.
        assert!((out.r - c.r).abs() < 1e-3);
        assert!((out.g - c.g).abs() < 1e-3);
        assert!((out.b - c.b).abs() < 1e-3);
    }

    #[test]
    fn lighten_oklch_raises_perceptual_lightness() {
        let c = accent_sample();
        assert!(lighten_oklch(c, 0.5).into_oklch().l > c.into_oklch().l);
    }

    #[test]
    fn lighten_oklch_is_monotonic_in_amount() {
        let c = accent_sample();
        assert!(lighten_oklch(c, 0.7).into_oklch().l > lighten_oklch(c, 0.3).into_oklch().l);
    }

    #[test]
    fn lighten_oklch_preserves_hue_in_gamut() {
        // Hue is held exactly in Oklch; observable drift only comes from sRGB
        // gamut clamping on out-of-gamut lifts. A muted sample lifted modestly
        // stays in gamut, so the held hue is preserved to float precision.
        let c = Color::from_rgb(0.35, 0.45, 0.62);
        let h0 = c.into_oklch().h;
        let h1 = lighten_oklch(c, 0.25).into_oklch().h;
        assert!((h1 - h0).abs() < 1e-2, "hue drifted: {h0} -> {h1}");
    }

    #[test]
    fn lighten_oklch_preserves_alpha() {
        let c = Color {
            a: 0.5,
            ..accent_sample()
        };
        assert!((lighten_oklch(c, 0.4).a - 0.5).abs() < 1e-3);
    }
}
