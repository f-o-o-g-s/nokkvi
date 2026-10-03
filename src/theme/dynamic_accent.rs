//! Dynamic accent — an accent color taken from the playing cover and fitted
//! to the active palette.
//!
//! The cover supplies a hue and a chroma ([`AccentSeed`], from
//! [`seed_from_rgba`]); [`fit_accent`] chooses the lightness. Every widget
//! reads the accent through `theme::accent()` / `accent_bright()` and pairs it
//! with a background tier — as accent ink on chrome, or as an accent fill
//! under `bg0_hard()` ink — so the fit holds the accent tokens to a contrast
//! floor against those tiers once, here, and each call site inherits it.

use iced::{Color, color::Oklch};

use super::{LEGIBLE_TEXT_CONTRAST, SELECTION_RING_MIN_CONTRAST, contrast_ratio, legible_text_on};
use crate::theme_config::{AccentRoles, ResolvedDualTheme, ResolvedTheme};

// ============================================================================
// Seed
// ============================================================================

/// The color a cover contributes: an Oklch hue and chroma, plus the lightness
/// it had in the artwork (the fit starts there and moves only as far as the
/// palette demands).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AccentSeed {
    /// Oklch lightness in the artwork, 0..=1.
    pub lightness: f32,
    /// Oklch chroma.
    pub chroma: f32,
    /// Oklch hue, in radians.
    pub hue: f32,
}

impl AccentSeed {
    #[cfg(test)]
    pub(crate) fn from_color(color: Color) -> Self {
        let Oklch { l, c, h, .. } = color.into_oklch();
        Self {
            lightness: l,
            chroma: c,
            hue: h,
        }
    }
}

// ============================================================================
// Fit
// ============================================================================

/// Headroom over each WCAG floor, so 8-bit quantization at render time cannot
/// drop a fitted color back under it.
const FIT_HEADROOM: f32 = 0.1;
/// Oklch lightness step of the walk away from the background.
const LIGHTNESS_STEP: f32 = 0.005;
/// Oklch lightness between a calm accent and its louder counterpart.
const BRIGHT_STEP: f32 = 0.07;
/// Lightest a seed may start on a dark palette / darkest on a light one. A
/// near-white or near-black cover color has no room left for chroma, so it is
/// pulled into the band where its hue still reads. The walk may leave the band
/// when the palette's floors demand it.
const BAND_LIGHTEST: f32 = 0.92;
const BAND_DARKEST: f32 = 0.28;

/// Every accent token, fitted to one palette. Two shades of the cover: the
/// TEXT shade (`accent`, `accent_bright`) also reads as small text on the
/// chrome, so it sits further from the background; the FILL shade
/// (`accent_fill*`) only has to stand out as a surface, so it stays closer to
/// the cover's own color, and `on_accent_fill` is the ink that reads on it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct FittedAccent {
    pub accent: Color,
    pub accent_bright: Color,
    pub accent_border_light: Color,
    pub accent_fill: Color,
    pub accent_fill_calm: Color,
    pub on_accent_fill: Color,
}

/// Whether `color` clears the TEXT floors of `palette`: body-text contrast
/// against the tiers that carry accent text (`bg2` included — depth-2 slot
/// rows put accent link text on it), and the UI-component floor against
/// `bg3`, which carries only accent outlines.
pub(super) fn clears_accent_floors(color: Color, palette: &ResolvedTheme) -> bool {
    let text = [
        palette.bg0_hard,
        palette.bg0,
        palette.bg0_soft,
        palette.bg1,
        palette.bg2,
    ];
    let raised = [palette.bg3];
    text.iter()
        .all(|bg| contrast_ratio(color, *bg) >= LEGIBLE_TEXT_CONTRAST + FIT_HEADROOM)
        && raised
            .iter()
            .all(|bg| contrast_ratio(color, *bg) >= SELECTION_RING_MIN_CONTRAST + FIT_HEADROOM)
}

/// Lowest contrast a fill keeps against `bg2`. A fill there is the progress
/// bar's elapsed part over its `bg2` track, or a row among depth-2 rows that
/// also carries its own ring, so it needs to stay separate, not to read as a
/// lone control. Holding it to the full 3:1 is what pushed a red cover to
/// salmon on dark themes, `bg2` being the lightest tier.
const FILL_ON_BG2_CONTRAST: f32 = 2.0;

/// Whether `color` clears the FILL floor of `palette`: the WCAG non-text
/// (UI-component) contrast against the tiers filled controls sit on (the
/// nav and player bars, settings rows, depth-0/1 slot rows), so an active
/// tab, a toggle or the now-playing row is always seen as one, and
/// [`FILL_ON_BG2_CONTRAST`] against `bg2`. Text on the fill is not this
/// floor's job — `on_accent_fill` is.
pub(super) fn clears_fill_floor(color: Color, palette: &ResolvedTheme) -> bool {
    [palette.bg0_hard, palette.bg0, palette.bg0_soft, palette.bg1]
        .iter()
        .all(|bg| contrast_ratio(color, *bg) >= SELECTION_RING_MIN_CONTRAST + FIT_HEADROOM)
        && contrast_ratio(color, palette.bg2) >= FILL_ON_BG2_CONTRAST + FIT_HEADROOM
}

/// Whether an Oklch triple survives the trip through sRGB unclipped.
fn in_gamut(l: f32, c: f32, h: f32) -> bool {
    const TOLERANCE: f32 = 0.002;
    let back = Color::from_oklch(Oklch { l, c, h, a: 1.0 }).into_oklch();
    (back.l - l).abs() <= TOLERANCE && (back.c - c).abs() <= TOLERANCE
}

/// The most chroma sRGB holds for hue `h` at lightness `l` (none at pure
/// black or white).
pub(super) fn max_chroma(l: f32, h: f32) -> f32 {
    let (mut lo, mut hi) = (0.0_f32, 0.5_f32);
    for _ in 0..14 {
        let mid = f32::midpoint(lo, hi);
        if in_gamut(l, mid, h) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

/// How saturated the cover's color is, as a share of the most chroma its hue
/// can hold at its own lightness: 1.0 for a color at the edge of sRGB.
///
/// The fit keeps this share, not the absolute chroma, when it moves the
/// lightness. A deep blood red is as saturated as a dark red can be; lifted
/// to a lightness that reads on a dark theme it should stay as saturated as a
/// red can be there, where sRGB holds far more chroma. Keeping the absolute
/// chroma instead turned it a washed-out coral.
pub(super) fn saturation(seed: AccentSeed) -> f32 {
    let room = max_chroma(seed.lightness.clamp(0.0, 1.0), seed.hue);
    if room <= f32::EPSILON {
        1.0
    } else {
        (seed.chroma.max(0.0) / room).min(1.0)
    }
}

/// The seed's hue at lightness `l`, at the seed's [`saturation`].
fn seed_color_at(seed: AccentSeed, saturation: f32, l: f32) -> Color {
    let l = l.clamp(0.0, 1.0);
    Color::from_oklch(Oklch {
        l,
        c: saturation * max_chroma(l, seed.hue),
        h: seed.hue,
        a: 1.0,
    })
}

/// The seed's hue walked away from the background, from lightness `from`,
/// to the nearest lightness where `clears` holds. Ends on pure black or white
/// (whichever lies away from the background) when nothing in between does.
///
/// Bisects rather than stepping: it runs on the UI thread for both palettes
/// at every accent change and theme reload. Contrast against the background
/// grows along the walk, so the boundary is found in a few probes; the result
/// always satisfies `clears` (the bisection only ever keeps a passing end).
pub(super) fn walk(
    color_at: impl Fn(f32) -> Color,
    away: f32,
    from: f32,
    clears: impl Fn(Color) -> bool,
) -> (f32, Color) {
    /// Lightness resolution of the bisection: 2^-12 of the remaining range.
    const PROBES: usize = 12;
    let from = from.clamp(0.0, 1.0);
    let start = color_at(from);
    if clears(start) {
        return (from, start);
    }
    let end_l = if away > 0.0 { 1.0 } else { 0.0 };
    let end = color_at(end_l);
    if !clears(end) {
        return (end_l, end);
    }
    let (mut failing, mut passing) = (from, end_l);
    for _ in 0..PROBES {
        let mid = f32::midpoint(failing, passing);
        if clears(color_at(mid)) {
            passing = mid;
        } else {
            failing = mid;
        }
    }
    (passing, color_at(passing))
}

/// Lowest contrast `ink` reaches against any of `fills`.
fn min_contrast(ink: Color, fills: &[Color]) -> f32 {
    fills
        .iter()
        .map(|fill| contrast_ratio(ink, *fill))
        .fold(f32::INFINITY, f32::min)
}

/// The ink for text on both fills: the palette's own chrome background or
/// text color when one reads (it keeps the theme's tint), else pure black or
/// white, whichever reads better.
fn fill_ink(palette: &ResolvedTheme, fills: &[Color]) -> Color {
    let floor = LEGIBLE_TEXT_CONTRAST + FIT_HEADROOM / 2.0;
    [palette.bg0_hard, palette.fg0]
        .into_iter()
        .find(|ink| min_contrast(*ink, fills) >= floor)
        .unwrap_or_else(|| {
            if min_contrast(Color::BLACK, fills) >= min_contrast(Color::WHITE, fills) {
                Color::BLACK
            } else {
                Color::WHITE
            }
        })
}

/// Fit `seed` to one palette, keeping the cover's hue and [`saturation`]
/// throughout.
///
/// TEXT shade: `accent` takes the lightness nearest the cover's own that
/// clears [`clears_accent_floors`]; `accent_bright` sits one [`BRIGHT_STEP`]
/// further from the background, so "bright" always means "more contrast".
///
/// FILL shade: `accent_fill` takes the lightness nearest the cover's own that
/// clears the looser [`clears_fill_floor`] — usually the cover's color itself
/// — and `accent_fill_calm` sits one step nearer the background (or the loud
/// fill one step further, when the floor leaves no room below it). The ink
/// is chosen to read on both; when no single ink does (the two fills
/// straddle mid-grey), the calm fill moves toward the loud one until one does.
///
/// When no lightness clears a floor (a palette whose tiers straddle
/// mid-grey), the walk ends on pure black or white — whichever
/// [`legible_text_on`] picks for `bg0_hard`, which is at least 4.58:1 there.
pub(super) fn fit_accent(seed: AccentSeed, palette: &ResolvedTheme) -> FittedAccent {
    // Away from the background: lighter on a dark palette, darker on a light one.
    let away = if legible_text_on(palette.bg0_hard) == Color::WHITE {
        1.0
    } else {
        -1.0
    };
    let start = if away > 0.0 {
        seed.lightness.min(BAND_LIGHTEST - BRIGHT_STEP)
    } else {
        seed.lightness.max(BAND_DARKEST + BRIGHT_STEP)
    };

    let saturation = saturation(seed);
    let color_at = |l: f32| seed_color_at(seed, saturation, l);

    let text = |c: Color| clears_accent_floors(c, palette);
    let (accent_l, accent) = walk(color_at, away, start, text);
    let (_, accent_bright) = walk(color_at, away, accent_l + away * BRIGHT_STEP, text);

    let fill = |c: Color| clears_fill_floor(c, palette);
    let (cover_l, _) = walk(color_at, away, start, fill);
    let (mut calm_l, mut calm) = walk(color_at, away, cover_l - away * BRIGHT_STEP, fill);
    let loud_from = if away > 0.0 {
        cover_l.max(calm_l + BRIGHT_STEP)
    } else {
        cover_l.min(calm_l - BRIGHT_STEP)
    };
    let (loud_l, loud) = walk(color_at, away, loud_from, fill);
    let mut ink = fill_ink(palette, &[calm, loud]);
    while min_contrast(ink, &[calm, loud]) < LEGIBLE_TEXT_CONTRAST && calm_l != loud_l {
        calm_l = if away > 0.0 {
            (calm_l + LIGHTNESS_STEP).min(loud_l)
        } else {
            (calm_l - LIGHTNESS_STEP).max(loud_l)
        };
        calm = color_at(calm_l);
        ink = fill_ink(palette, &[calm, loud]);
    }

    FittedAccent {
        accent,
        accent_bright,
        accent_border_light: accent_bright,
        accent_fill: loud,
        accent_fill_calm: calm,
        on_accent_fill: ink,
    }
}

/// Replace the accent tokens of both modes with `seed`, each fitted to its own
/// backgrounds, so a light/dark toggle needs no refit. Stars and hearts take
/// the text shade, so they match the accent on the chrome rows instead of
/// clashing with it (on the loud-fill rows they take the row's ink; see
/// `slot_list`).
pub(super) fn apply(seed: AccentSeed, theme: &mut ResolvedDualTheme) {
    for palette in [&mut theme.dark, &mut theme.light] {
        let fitted = fit_accent(seed, palette);
        palette.accent = fitted.accent;
        palette.accent_bright = fitted.accent_bright;
        palette.accent_border_light = fitted.accent_border_light;
        palette.roles = Some(AccentRoles {
            accent_fill: fitted.accent_fill,
            accent_fill_calm: fitted.accent_fill_calm,
            on_accent_fill: fitted.on_accent_fill,
            rating: fitted.accent_bright,
            love: fitted.accent_bright,
        });
    }
}

// ============================================================================
// Extraction
// ============================================================================

/// Upper bound on the pixels read from one cover; larger images are strided.
const MAX_SAMPLES: u32 = 4096;
/// Hue histogram resolution (10° per bin).
const HUE_BINS: usize = 36;
/// A pixel below this chroma is grey and carries no hue.
const MIN_PIXEL_CHROMA: f32 = 0.04;
/// A pixel outside this lightness range is too close to black or white for its
/// hue to be seen.
const MIN_PIXEL_LIGHTNESS: f32 = 0.2;
const MAX_PIXEL_LIGHTNESS: f32 = 0.96;
/// Share of the sampled pixels that must carry a hue at all; below it the
/// cover is black-and-white and yields a neutral palette.
const MIN_CHROMATIC_SHARE: f32 = 0.03;
/// Mean chroma the cover's strongest hue needs to count as a color. Below it
/// the "color" is the tint of a monochrome print — a sepia or cool-toned
/// scan sits around 0.025–0.045, a genuinely brown cover at 0.06 — and the
/// cover yields a neutral palette.
const ACHROMATIC_CHROMA: f32 = 0.055;
/// Share of the sampled pixels a hue needs before it can win, so a sticker or
/// a barcode cannot color the whole app.
const MIN_HUE_SHARE: f32 = 0.015;
/// Most hues taken from one cover (the visualizer gradient's anchors).
const MAX_COVER_HUES: usize = 4;
/// Closest two picked hues may sit, in bins (40°): nearer than that they are
/// one color, and the second would only muddy the gradient.
const MIN_HUE_SEPARATION_BINS: usize = 4;
/// Score a secondary hue needs relative to the primary, so a few stray pixels
/// of a third color do not become a gradient stop.
const MIN_SECONDARY_SCORE: f32 = 0.12;

/// The colors a cover contributes: its prominent vivid hues, the most
/// prominent first — or, for a black-and-white cover, one neutral grey (zero
/// chroma), so a monochrome album gets a monochrome accent and visualizer.
/// Never empty. The accent takes the first; the visualizer gradient runs
/// through all of them.
#[derive(Debug, Clone, PartialEq)]
pub struct CoverPalette {
    colors: Vec<AccentSeed>,
    /// The cover's average Oklch lightness: what a visualizer drawn over the
    /// artwork sits on (see `cover_visualizer`).
    backdrop_lightness: f32,
}

impl CoverPalette {
    /// A monochrome cover's palette: one zero-chroma seed, which also sits
    /// at the print's average lightness.
    fn neutral(seed: AccentSeed) -> Self {
        Self {
            colors: vec![seed],
            backdrop_lightness: seed.lightness,
        }
    }

    /// A palette of one color, over an artwork of that color's lightness.
    #[cfg(test)]
    pub(crate) fn single(seed: AccentSeed) -> Self {
        Self {
            colors: vec![seed],
            backdrop_lightness: seed.lightness,
        }
    }

    /// A palette from `colors`, most prominent first, over an artwork of
    /// average lightness `backdrop_lightness`; `None` when empty.
    #[cfg(test)]
    pub(crate) fn from_colors(colors: Vec<AccentSeed>, backdrop_lightness: f32) -> Option<Self> {
        (!colors.is_empty()).then_some(Self {
            colors,
            backdrop_lightness,
        })
    }

    /// The cover's average lightness (Oklch).
    pub(crate) fn backdrop_lightness(&self) -> f32 {
        self.backdrop_lightness
    }

    /// The most prominent hue: the accent's seed.
    pub(crate) fn primary(&self) -> AccentSeed {
        self.colors[0]
    }

    /// Every hue, most prominent first.
    pub(crate) fn colors(&self) -> &[AccentSeed] {
        &self.colors
    }
}

#[derive(Clone, Copy, Default)]
struct HueBin {
    count: u32,
    /// Σ chroma² — vivid pixels outvote muted ones.
    score: f32,
    sin: f32,
    cos: f32,
    chroma: f32,
    lightness: f32,
}

/// Pick the palette of an RGBA8 image: up to [`MAX_COVER_HUES`] prominent
/// vivid hues, the strongest first, or one neutral grey for a black-and-white
/// image (see [`ACHROMATIC_CHROMA`]). `None` only for an empty or transparent
/// image.
pub(crate) fn palette_from_rgba(width: u32, height: u32, rgba: &[u8]) -> Option<CoverPalette> {
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || rgba.len() < w * h * 4 {
        return None;
    }
    let stride = (((w * h) as f32 / MAX_SAMPLES as f32).sqrt().ceil() as usize).max(1);

    let mut bins = [HueBin::default(); HUE_BINS];
    let mut sampled = 0u32;
    let mut chromatic = 0u32;
    let mut lightness_sum = 0.0_f32;
    for y in (0..h).step_by(stride) {
        for x in (0..w).step_by(stride) {
            let Some(&[r, g, b, a]) = rgba.get((y * w + x) * 4..).and_then(|p| p.first_chunk())
            else {
                continue;
            };
            if a < 128 {
                continue;
            }
            sampled += 1;
            let Oklch { l, c, h: hue, .. } = Color::from_rgb8(r, g, b).into_oklch();
            lightness_sum += l;
            if c < MIN_PIXEL_CHROMA || !(MIN_PIXEL_LIGHTNESS..=MAX_PIXEL_LIGHTNESS).contains(&l) {
                continue;
            }
            chromatic += 1;
            let turn = (hue + std::f32::consts::PI) / std::f32::consts::TAU;
            let bin = &mut bins[((turn * HUE_BINS as f32) as usize).min(HUE_BINS - 1)];
            let weight = c * c;
            bin.count += 1;
            bin.score += weight;
            bin.sin += weight * hue.sin();
            bin.cos += weight * hue.cos();
            bin.chroma += weight * c;
            bin.lightness += weight * l;
        }
    }
    if sampled == 0 {
        return None;
    }
    // A monochrome cover: a grey at the print's own average lightness.
    let neutral = || {
        CoverPalette::neutral(AccentSeed {
            lightness: lightness_sum / sampled as f32,
            chroma: 0.0,
            hue: 0.0,
        })
    };
    if (chromatic as f32) < sampled as f32 * MIN_CHROMATIC_SHARE {
        return Some(neutral());
    }

    // A hue's support is its bin plus both neighbors: a color sitting on a bin
    // edge must not lose to a narrower one that happens to be centered.
    let around = |i: usize| {
        [
            bins[(i + HUE_BINS - 1) % HUE_BINS],
            bins[i],
            bins[(i + 1) % HUE_BINS],
        ]
    };
    let score = |i: usize| {
        let [prev, mid, next] = around(i);
        0.5 * prev.score + mid.score + 0.5 * next.score
    };
    let min_count = sampled as f32 * MIN_HUE_SHARE;
    let mut ranked: Vec<usize> = (0..HUE_BINS)
        .filter(|&i| around(i).iter().map(|b| b.count).sum::<u32>() as f32 >= min_count)
        .collect();
    ranked.sort_by(|&a, &b| score(b).total_cmp(&score(a)));

    // Each candidate hue's mean color over its support. A barely tinted
    // candidate (paper, a sepia wash) is not a color at all: it is dropped
    // BEFORE the primary is chosen, so it can neither become a grey gradient
    // stop nor out-score a vivid hue that clears the population floor.
    let seed_of = |i: usize| {
        let (mut score, mut sin, mut cos, mut chroma, mut lightness) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for bin in around(i) {
            score += bin.score;
            sin += bin.sin;
            cos += bin.cos;
            chroma += bin.chroma;
            lightness += bin.lightness;
        }
        (score > 0.0).then(|| AccentSeed {
            lightness: lightness / score,
            chroma: chroma / score,
            hue: sin.atan2(cos),
        })
    };
    let colorful: Vec<(usize, AccentSeed)> = ranked
        .into_iter()
        .filter_map(|i| seed_of(i).map(|seed| (i, seed)))
        .filter(|(_, seed)| seed.chroma >= ACHROMATIC_CHROMA)
        .collect();

    let bin_distance = |a: usize, b: usize| {
        let d = a.abs_diff(b);
        d.min(HUE_BINS - d)
    };
    let mut picked: Vec<(usize, AccentSeed)> = Vec::with_capacity(MAX_COVER_HUES);
    for (i, seed) in colorful {
        if picked.len() == MAX_COVER_HUES {
            break;
        }
        let strong_enough = picked
            .first()
            .is_none_or(|&(first, _)| score(i) >= score(first) * MIN_SECONDARY_SCORE);
        let distinct = picked
            .iter()
            .all(|&(p, _)| bin_distance(i, p) >= MIN_HUE_SEPARATION_BINS);
        if strong_enough && distinct {
            picked.push((i, seed));
        }
    }
    if picked.is_empty() {
        return Some(neutral());
    }
    Some(CoverPalette {
        colors: picked.into_iter().map(|(_, seed)| seed).collect(),
        backdrop_lightness: lightness_sum / sampled as f32,
    })
}

/// [`palette_from_rgba`] for an encoded cover (PNG / JPEG / …). Blocking; run
/// it off the UI thread.
pub(crate) fn palette_from_encoded(bytes: &[u8]) -> Option<CoverPalette> {
    /// Decode target: a cover's prominent hues survive any downscale.
    const THUMB_SIDE: u32 = 64;
    let thumb = image::load_from_memory(bytes)
        .ok()?
        .thumbnail(THUMB_SIDE, THUMB_SIDE)
        .to_rgba8();
    palette_from_rgba(thumb.width(), thumb.height(), thumb.as_raw())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{relative_luminance, resolve_highlight_fills, tests::all_builtin_palettes};

    fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color::from_rgb8(r, g, b)
    }

    /// The primary hue of an image, as the accent sees it.
    fn seed_from_rgba(width: u32, height: u32, rgba: &[u8]) -> Option<AccentSeed> {
        palette_from_rgba(width, height, rgba).map(|p| p.primary())
    }

    fn seed_from_encoded(bytes: &[u8]) -> Option<AccentSeed> {
        palette_from_encoded(bytes).map(|p| p.primary())
    }

    /// Seeds that cover the hue wheel at several chromas and lightnesses, plus
    /// the hard cases: the extremes, greys, a neon, a deep navy.
    fn seed_sweep() -> Vec<AccentSeed> {
        let mut seeds = Vec::new();
        for hue_step in 0..24 {
            let hue = (hue_step as f32 / 24.0) * std::f32::consts::TAU - std::f32::consts::PI;
            for chroma in [0.02, 0.09, 0.18, 0.32] {
                for lightness in [0.05, 0.3, 0.55, 0.8, 0.98] {
                    seeds.push(AccentSeed {
                        lightness,
                        chroma,
                        hue,
                    });
                }
            }
        }
        for color in [
            Color::BLACK,
            Color::WHITE,
            rgb(0x80, 0x80, 0x80),
            rgb(0xff, 0xff, 0x00),
            rgb(0x00, 0xff, 0x00),
            rgb(0x00, 0x00, 0xff),
            rgb(0xff, 0x00, 0xff),
            rgb(0x0a, 0x12, 0x30),
        ] {
            seeds.push(AccentSeed::from_color(color));
        }
        seeds
    }

    /// The feature's promise: whatever the cover, both accent tokens clear
    /// every accent floor on every shipped theme in both modes.
    #[test]
    fn fitted_accent_clears_every_floor_on_every_theme() {
        for (name, mode, palette) in all_builtin_palettes() {
            // A seed that IS the palette's own background is the worst cover.
            let mut seeds = seed_sweep();
            seeds.push(AccentSeed::from_color(palette.bg0_hard));
            seeds.push(AccentSeed::from_color(palette.bg2));
            for seed in seeds {
                let fitted = fit_accent(seed, &palette);
                for (token, color) in [
                    ("accent", fitted.accent),
                    ("accent_bright", fitted.accent_bright),
                    ("accent_border_light", fitted.accent_border_light),
                ] {
                    assert!(
                        clears_accent_floors(color, &palette),
                        "{name}/{mode} {token} {color:?} from {seed:?} misses a floor \
                         (bg0_hard {:.2}:1, bg1 {:.2}:1, bg3 {:.2}:1)",
                        contrast_ratio(color, palette.bg0_hard),
                        contrast_ratio(color, palette.bg1),
                        contrast_ratio(color, palette.bg3),
                    );
                }
            }
        }
    }

    /// `bg0_hard` ink on an accent fill (settings pills, active nav tabs, mode
    /// toggles, key-caps) is the same pair as accent text on `bg0_hard`; pin
    /// it by name so the reason the fit exists stays on the page.
    #[test]
    fn chrome_ink_reads_on_fitted_accent_fills() {
        for (name, mode, palette) in all_builtin_palettes() {
            for seed in seed_sweep() {
                let fitted = fit_accent(seed, &palette);
                for fill in [fitted.accent, fitted.accent_bright] {
                    let cr = contrast_ratio(palette.bg0_hard, fill);
                    assert!(
                        cr >= LEGIBLE_TEXT_CONTRAST,
                        "{name}/{mode} bg0_hard ink on fill {fill:?} only {cr:.2}:1"
                    );
                }
            }
        }
    }

    /// "Bright" means more contrast against the chrome in both modes, so the
    /// derived highlight fills keep their loud/ambient hierarchy.
    #[test]
    fn accent_bright_is_never_closer_to_the_background() {
        for (name, mode, palette) in all_builtin_palettes() {
            for seed in seed_sweep() {
                let fitted = fit_accent(seed, &palette);
                assert!(
                    contrast_ratio(fitted.accent_bright, palette.bg0_hard) + 1e-3
                        >= contrast_ratio(fitted.accent, palette.bg0_hard),
                    "{name}/{mode} accent_bright recedes behind accent for {seed:?}"
                );
            }
        }
    }

    /// The now-playing and expanded-parent fills derive from the fitted FILL
    /// pair and must stay tellable apart, as the static themes' are.
    #[test]
    fn fitted_highlight_fills_stay_distinct() {
        for (name, mode, palette) in all_builtin_palettes() {
            for seed in seed_sweep() {
                let fitted = fit_accent(seed, &palette);
                let (play, sel) = resolve_highlight_fills(
                    fitted.accent_fill_calm,
                    fitted.accent_fill,
                    palette.bg0_hard,
                );
                let cr = contrast_ratio(play, sel);
                assert!(
                    cr >= crate::theme::FILL_DISTINCT_CONTRAST - 1e-3,
                    "{name}/{mode} fills only {cr:.2}:1 apart for {seed:?}"
                );
            }
        }
    }

    /// Both fill shades stand out from every tier a filled control sits on,
    /// on every shipped theme, whatever the cover.
    #[test]
    fn fills_clear_the_fill_floor_on_every_theme() {
        for (name, mode, palette) in all_builtin_palettes() {
            let mut seeds = seed_sweep();
            seeds.push(AccentSeed::from_color(palette.bg0_hard));
            for seed in seeds {
                let fitted = fit_accent(seed, &palette);
                for (token, fill) in [
                    ("accent_fill", fitted.accent_fill),
                    ("accent_fill_calm", fitted.accent_fill_calm),
                ] {
                    assert!(
                        clears_fill_floor(fill, &palette),
                        "{name}/{mode} {token} {fill:?} from {seed:?} blends into the chrome \
                         (bg0_hard {:.2}:1, bg2 {:.2}:1)",
                        contrast_ratio(fill, palette.bg0_hard),
                        contrast_ratio(fill, palette.bg2),
                    );
                }
            }
        }
    }

    /// The point of the split: text and icons on an accent fill (tab labels,
    /// toggle glyphs, pill labels, the progress time) read on BOTH fills.
    #[test]
    fn fill_ink_reads_on_both_fills_on_every_theme() {
        for (name, mode, palette) in all_builtin_palettes() {
            let mut seeds = seed_sweep();
            seeds.push(AccentSeed::from_color(palette.bg0_hard));
            for seed in seeds {
                let f = fit_accent(seed, &palette);
                for (token, fill) in [
                    ("accent_fill", f.accent_fill),
                    ("accent_fill_calm", f.accent_fill_calm),
                ] {
                    let cr = contrast_ratio(f.on_accent_fill, fill);
                    assert!(
                        cr >= LEGIBLE_TEXT_CONTRAST,
                        "{name}/{mode} ink {:?} on {token} {fill:?} only {cr:.2}:1 for {seed:?}",
                        f.on_accent_fill
                    );
                }
            }
        }
    }

    /// The fill shade is the better match: it never moves further from the
    /// cover's own lightness than the text shade does (beyond the one step
    /// the calm fill may push it), and it keeps the cover's hue.
    #[test]
    fn the_fill_shade_stays_closer_to_the_cover() {
        for (name, mode, palette) in all_builtin_palettes() {
            let dark_bg = legible_text_on(palette.bg0_hard) == Color::WHITE;
            for seed in seed_sweep() {
                let f = fit_accent(seed, &palette);
                let (fill, text) = (f.accent_fill.into_oklch(), f.accent.into_oklch());
                // The lightness both fits aim at: the cover's own, pulled
                // into the band where a hue still reads.
                let aim = if dark_bg {
                    seed.lightness.min(BAND_LIGHTEST - BRIGHT_STEP)
                } else {
                    seed.lightness.max(BAND_DARKEST + BRIGHT_STEP)
                };
                assert!(
                    (fill.l - aim).abs() <= (text.l - aim).abs() + BRIGHT_STEP + 0.01,
                    "{name}/{mode} fill drifted further than text for {seed:?}"
                );
                if fill.c > 0.05 && seed.chroma > 0.05 {
                    let d = (fill.h - seed.hue).rem_euclid(std::f32::consts::TAU);
                    let drift = d.min(std::f32::consts::TAU - d);
                    assert!(drift < 0.15, "{name}/{mode} fill hue drifted {drift:.3}");
                }
            }
        }
    }

    /// The case the owner flagged: a red cover's accent went salmon on dark
    /// themes because it had to read as text. As a fill it stays red.
    #[test]
    fn a_red_cover_fills_red_on_dark_themes() {
        let red = AccentSeed::from_color(rgb(0xd0, 0x28, 0x30));
        for (name, mode, palette) in all_builtin_palettes() {
            if legible_text_on(palette.bg0_hard) != Color::WHITE {
                continue;
            }
            let f = fit_accent(red, &palette);
            let (fill, text) = (f.accent_fill.into_oklch(), f.accent.into_oklch());
            // Salmon is red lifted toward white: the fill must stay below
            // the text shade's lightness, nearer the cover's own. How far
            // below depends on the palette's lightest tier; on the default
            // theme it is a clear step.
            let gap = if name == "svalbard" { 0.05 } else { 0.01 };
            assert!(
                fill.l + gap <= text.l,
                "{name}/{mode}: fill lightness {:.3} is as pale as text {:.3}",
                fill.l,
                text.l
            );
        }
    }

    /// A deep, fully saturated red (a blood-red metal cover) lifted to a
    /// lightness that reads on a dark theme stays a saturated red, not coral:
    /// the fit keeps the cover's saturation, not its absolute chroma.
    #[test]
    fn a_deep_red_stays_saturated_when_lifted() {
        let blood = AccentSeed::from_color(rgb(0x8a, 0x0c, 0x10));
        for (name, mode, palette) in all_builtin_palettes() {
            if legible_text_on(palette.bg0_hard) != Color::WHITE {
                continue;
            }
            let fill = fit_accent(blood, &palette).accent_fill.into_oklch();
            let room = max_chroma(fill.l, fill.h);
            assert!(
                fill.c >= room * 0.9,
                "{name}/{mode}: fill chroma {:.3} of a possible {room:.3} at L {:.2}",
                fill.c,
                fill.l
            );
        }
    }

    /// A muted cover stays muted: the share of possible chroma is kept, so a
    /// dusty color does not turn neon when lifted.
    #[test]
    fn a_muted_cover_stays_muted_when_lifted() {
        let dusty = AccentSeed {
            lightness: 0.35,
            chroma: 0.04,
            hue: 0.5,
        };
        let share = saturation(dusty);
        assert!(
            share < 0.5,
            "fixture: dusty is under half saturated ({share:.2})"
        );
        for (name, mode, palette) in all_builtin_palettes() {
            let fill = fit_accent(dusty, &palette).accent_fill.into_oklch();
            let room = max_chroma(fill.l, fill.h);
            if room > 0.02 {
                assert!(
                    fill.c <= room * (share + 0.05),
                    "{name}/{mode}: dusty seed went vivid ({:.3} of {room:.3})",
                    fill.c
                );
            }
        }
    }

    /// A background the fit cannot serve with any color (every tier mid-grey)
    /// still gets a legible accent: the walk ends on black or white.
    #[test]
    fn mid_grey_palette_falls_back_to_the_legible_extreme() {
        for grey in [0.30_f32, 0.42, 0.46, 0.5, 0.6] {
            let bg = Color::from_rgb(grey, grey, grey);
            let palette = ResolvedTheme {
                bg0_hard: bg,
                bg0: bg,
                bg0_soft: bg,
                bg1: bg,
                bg2: bg,
                bg3: bg,
                ..ResolvedTheme::default()
            };
            for seed in seed_sweep() {
                let fitted = fit_accent(seed, &palette);
                for color in [fitted.accent, fitted.accent_bright] {
                    let cr = contrast_ratio(color, bg);
                    assert!(
                        cr >= LEGIBLE_TEXT_CONTRAST,
                        "grey {grey} accent {color:?} only {cr:.2}:1"
                    );
                }
                for fill in [fitted.accent_fill, fitted.accent_fill_calm] {
                    let cr = contrast_ratio(fitted.on_accent_fill, fill);
                    assert!(
                        cr >= LEGIBLE_TEXT_CONTRAST,
                        "grey {grey} fill ink only {cr:.2}:1 on {fill:?}"
                    );
                }
            }
        }
    }

    /// A cover color that already reads on the palette keeps its hue and stays
    /// a color: the fit must not bleach what it does not need to move.
    #[test]
    fn a_vivid_seed_keeps_its_hue_and_chroma() {
        let orange = rgb(0xe8, 0x7a, 0x1c);
        let seed = AccentSeed::from_color(orange);
        for (name, mode, palette) in all_builtin_palettes() {
            let fitted = fit_accent(seed, &palette);
            let out = fitted.accent.into_oklch();
            let hue_drift = (out.h - seed.hue).abs();
            assert!(
                hue_drift < 0.12,
                "{name}/{mode} hue drifted {hue_drift:.3} rad"
            );
            assert!(
                out.c > 0.06,
                "{name}/{mode} accent lost its color (chroma {:.3})",
                out.c
            );
        }
    }

    /// A dark background lifts the accent, a light one deepens it. The
    /// direction follows the palette's actual background, not its mode name:
    /// Dracula ships a dark palette in its light slot.
    #[test]
    fn fit_moves_away_from_the_background() {
        let seed = AccentSeed::from_color(rgb(0x3a, 0x6f, 0xd8));
        for (name, mode, palette) in all_builtin_palettes() {
            let fitted = fit_accent(seed, &palette);
            let bg = relative_luminance(palette.bg0_hard);
            let accent = relative_luminance(fitted.accent);
            if legible_text_on(palette.bg0_hard) == Color::WHITE {
                assert!(
                    accent > bg,
                    "{name}/{mode} accent is not lighter than its bg"
                );
            } else {
                assert!(
                    accent < bg,
                    "{name}/{mode} accent is not darker than its bg"
                );
            }
        }
    }

    #[test]
    fn apply_overrides_only_the_accent_tokens() {
        let base = ResolvedDualTheme::default();
        let mut themed = base.clone();
        apply(AccentSeed::from_color(rgb(0xd0, 0x30, 0x60)), &mut themed);
        for (before, after) in [(&base.dark, &themed.dark), (&base.light, &themed.light)] {
            assert_ne!(before.accent, after.accent);
            assert_ne!(before.accent_bright, after.accent_bright);
            assert_eq!(before.roles, None, "a static theme derives its roles");
            let roles = after.roles.expect("the overlay sets the roles");
            assert_eq!(roles.rating, after.accent_bright, "stars follow the accent");
            assert_eq!(roles.love, after.accent_bright, "hearts follow the accent");
            assert_eq!(before.bg0_hard, after.bg0_hard);
            assert_eq!(before.fg0, after.fg0);
            assert_eq!(before.danger, after.danger);
            assert_eq!(before.success, after.success);
            assert_eq!(before.warning, after.warning);
            assert_eq!(before.star_bright, after.star_bright);
            assert_eq!(before.border, after.border);
        }
    }

    // ---- extraction --------------------------------------------------------

    /// An image of `blocks`: each `(color, pixels)` run laid out in order.
    fn image_of(blocks: &[([u8; 3], usize)]) -> (u32, u32, Vec<u8>) {
        let mut rgba = Vec::new();
        for ([r, g, b], n) in blocks {
            for _ in 0..*n {
                rgba.extend_from_slice(&[*r, *g, *b, 255]);
            }
        }
        let side = 32;
        assert_eq!(
            rgba.len(),
            side * side * 4,
            "fixture must fill a 32x32 image"
        );
        (side as u32, side as u32, rgba)
    }

    fn hue_of(r: u8, g: u8, b: u8) -> f32 {
        rgb(r, g, b).into_oklch().h
    }

    fn hue_distance(a: f32, b: f32) -> f32 {
        let d = (a - b).rem_euclid(std::f32::consts::TAU);
        d.min(std::f32::consts::TAU - d)
    }

    #[test]
    fn a_flat_cover_yields_its_own_hue() {
        let (w, h, px) = image_of(&[([0x20, 0x70, 0xd0], 1024)]);
        let seed = seed_from_rgba(w, h, &px).expect("a blue cover has a hue");
        assert!(hue_distance(seed.hue, hue_of(0x20, 0x70, 0xd0)) < 0.02);
    }

    /// The palette is a single zero-chroma grey: a monochrome album gets a
    /// monochrome accent, not the theme's color.
    fn assert_neutral(palette: Option<CoverPalette>, what: &str) {
        let palette = palette.unwrap_or_else(|| panic!("{what}: a neutral palette, not None"));
        assert_eq!(palette.colors().len(), 1, "{what}: {:?}", palette.colors());
        assert_eq!(
            palette.primary().chroma,
            0.0,
            "{what}: {:?}",
            palette.primary()
        );
    }

    #[test]
    fn black_and_white_covers_yield_a_neutral_palette() {
        for grey in [0x00, 0x30, 0x80, 0xc8, 0xff] {
            let (w, h, px) = image_of(&[([grey, grey, grey], 1024)]);
            assert_neutral(palette_from_rgba(w, h, &px), &format!("grey {grey:#x}"));
        }
        // A photo-like mix of greys.
        let (w, h, px) = image_of(&[
            ([0x10, 0x10, 0x10], 400),
            ([0x90, 0x90, 0x90], 400),
            ([0xf0, 0xf0, 0xf0], 224),
        ]);
        assert_neutral(palette_from_rgba(w, h, &px), "a mix of greys");
    }

    /// A sepia or cool-toned print is a monochrome cover with a tint, not a
    /// colored one; it must not come out brown or blue.
    #[test]
    fn a_tinted_monochrome_print_is_neutral() {
        let (w, h, px) = image_of(&[
            ([0x5a, 0x4c, 0x38], 340),
            ([0x8a, 0x78, 0x5c], 340),
            ([0xc8, 0xb8, 0x9a], 344),
        ]);
        assert_neutral(palette_from_rgba(w, h, &px), "sepia");
        let (w, h, px) = image_of(&[([0x7a, 0x84, 0x92], 1024)]);
        assert_neutral(palette_from_rgba(w, h, &px), "cool tint");
    }

    /// The neutral grey sits at the print's own average lightness.
    #[test]
    fn a_neutral_palette_keeps_the_prints_lightness() {
        let (w, h, px) = image_of(&[([0x20, 0x20, 0x20], 1024)]);
        let dark = palette_from_rgba(w, h, &px).expect("palette").primary();
        let (w, h, px) = image_of(&[([0xe0, 0xe0, 0xe0], 1024)]);
        let light = palette_from_rgba(w, h, &px).expect("palette").primary();
        assert!(dark.lightness < light.lightness);
    }

    /// A vivid minority beats a muted majority: the accent should be the color
    /// a person would name, not the average.
    #[test]
    fn a_vivid_region_beats_a_larger_muted_one() {
        let (w, h, px) = image_of(&[([0x80, 0x60, 0x40], 724), ([0xe0, 0x20, 0x30], 300)]);
        let seed = seed_from_rgba(w, h, &px).expect("has color");
        assert!(
            hue_distance(seed.hue, hue_of(0xe0, 0x20, 0x30)) < 0.1,
            "expected the red, got hue {}",
            seed.hue
        );
    }

    /// A speck (under the hue share floor) cannot color the app.
    #[test]
    fn a_speck_of_color_does_not_win() {
        let (w, h, px) = image_of(&[([0x20, 0x70, 0xd0], 1016), ([0xff, 0x00, 0x00], 8)]);
        let seed = seed_from_rgba(w, h, &px).expect("has color");
        assert!(hue_distance(seed.hue, hue_of(0x20, 0x70, 0xd0)) < 0.05);
    }

    /// One red speck on an otherwise grey cover is not a colored cover.
    #[test]
    fn a_speck_on_a_grey_cover_is_neutral() {
        let (w, h, px) = image_of(&[([0x80, 0x80, 0x80], 1016), ([0xff, 0x00, 0x00], 8)]);
        assert_neutral(palette_from_rgba(w, h, &px), "speck");
    }

    #[test]
    fn transparent_pixels_are_ignored() {
        let mut px = Vec::new();
        for _ in 0..512 {
            px.extend_from_slice(&[0xff, 0x00, 0x00, 0]);
        }
        for _ in 0..512 {
            px.extend_from_slice(&[0x20, 0xb0, 0x40, 255]);
        }
        let seed = seed_from_rgba(32, 32, &px).expect("the opaque half has a hue");
        assert!(hue_distance(seed.hue, hue_of(0x20, 0xb0, 0x40)) < 0.02);
    }

    /// A genuinely brown cover is a color, and keeps its own (muted) chroma:
    /// nothing boosts it.
    #[test]
    fn a_muted_cover_keeps_its_own_chroma() {
        let (w, h, px) = image_of(&[([0x80, 0x60, 0x40], 1024)]);
        let seed = seed_from_rgba(w, h, &px).expect("brown is a hue");
        assert!(seed.chroma >= ACHROMATIC_CHROMA, "brown counts as a color");
        assert!(
            (seed.chroma - 0.062).abs() < 0.01,
            "chroma {:.3}",
            seed.chroma
        );
    }

    #[test]
    fn malformed_buffers_yield_no_seed() {
        assert_eq!(seed_from_rgba(0, 0, &[]), None);
        assert_eq!(seed_from_rgba(4, 4, &[0; 8]), None);
        assert_eq!(seed_from_encoded(b"not an image"), None);
    }

    /// A large image is strided, not read whole, and still finds the hue.
    #[test]
    fn a_large_cover_is_sampled() {
        let side = 1000usize;
        let mut px = Vec::with_capacity(side * side * 4);
        for _ in 0..side * side {
            px.extend_from_slice(&[0xd0, 0x80, 0x10, 255]);
        }
        let seed = seed_from_rgba(side as u32, side as u32, &px).expect("has a hue");
        assert!(hue_distance(seed.hue, hue_of(0xd0, 0x80, 0x10)) < 0.02);
    }

    /// A two-color cover yields both hues, the stronger first.
    #[test]
    fn a_two_color_cover_yields_both_hues() {
        let (w, h, px) = image_of(&[([0xe0, 0x20, 0x30], 600), ([0x20, 0x70, 0xd0], 424)]);
        let palette = palette_from_rgba(w, h, &px).expect("has color");
        let hues: Vec<f32> = palette.colors().iter().map(|c| c.hue).collect();
        assert_eq!(hues.len(), 2, "{hues:?}");
        assert!(hue_distance(hues[0], hue_of(0xe0, 0x20, 0x30)) < 0.1);
        assert!(hue_distance(hues[1], hue_of(0x20, 0x70, 0xd0)) < 0.1);
    }

    /// Two shades of one color are one hue, not a two-stop gradient.
    #[test]
    fn neighbouring_hues_merge() {
        let (w, h, px) = image_of(&[([0xe0, 0x20, 0x30], 512), ([0xe0, 0x40, 0x20], 512)]);
        let palette = palette_from_rgba(w, h, &px).expect("has color");
        assert_eq!(palette.colors().len(), 1, "{:?}", palette.colors());
    }

    /// At most four hues, however many the cover has.
    #[test]
    fn a_rainbow_cover_caps_at_four_hues() {
        let stripes = [
            [0xe0, 0x20, 0x30],
            [0xe8, 0x90, 0x10],
            [0xd0, 0xd0, 0x20],
            [0x30, 0xb0, 0x40],
            [0x20, 0x90, 0xd0],
            [0x70, 0x30, 0xd0],
            [0xd0, 0x30, 0xb0],
            [0x20, 0xc0, 0xb0],
        ];
        let (w, h, px) = image_of(&stripes.map(|c| (c, 128)));
        let palette = palette_from_rgba(w, h, &px).expect("has color");
        assert_eq!(palette.colors().len(), MAX_COVER_HUES);
    }

    /// A faint second color (a few percent of a vivid first) is not a stop.
    #[test]
    fn a_faint_second_hue_is_dropped() {
        let (w, h, px) = image_of(&[([0xe0, 0x20, 0x30], 1000), ([0x6b, 0x70, 0x90], 24)]);
        let palette = palette_from_rgba(w, h, &px).expect("has color");
        assert_eq!(palette.colors().len(), 1, "{:?}", palette.colors());
    }

    /// A vivid red logo on beige paper: the paper is a tint, not a second
    /// hue, so it never becomes a grey gradient stop.
    #[test]
    fn a_tinted_paper_is_not_a_second_hue() {
        let (w, h, px) = image_of(&[([0xc8, 0xb8, 0x9a], 820), ([0xe0, 0x20, 0x30], 204)]);
        let palette = palette_from_rgba(w, h, &px).expect("has color");
        assert_eq!(palette.colors().len(), 1, "{:?}", palette.colors());
        assert!(hue_distance(palette.primary().hue, hue_of(0xe0, 0x20, 0x30)) < 0.1);
    }

    /// Sepia paper over most of the cover plus a small vivid red block: the
    /// red is the color a person would name, so it wins over the tint.
    #[test]
    fn a_vivid_block_on_a_sepia_cover_is_its_color() {
        let (w, h, px) = image_of(&[([0x8a, 0x78, 0x5c], 1000), ([0xe0, 0x20, 0x30], 24)]);
        let palette = palette_from_rgba(w, h, &px).expect("has color");
        assert!(palette.primary().chroma > 0.1, "{:?}", palette.primary());
        assert!(hue_distance(palette.primary().hue, hue_of(0xe0, 0x20, 0x30)) < 0.1);
    }

    #[test]
    fn an_encoded_cover_decodes_to_its_hue() {
        let img = image::RgbaImage::from_pixel(120, 90, image::Rgba([0x30, 0xa0, 0x60, 255]));
        let mut png = std::io::Cursor::new(Vec::new());
        img.write_to(&mut png, image::ImageFormat::Png)
            .expect("encode");
        let seed = seed_from_encoded(png.get_ref()).expect("has a hue");
        assert!(hue_distance(seed.hue, hue_of(0x30, 0xa0, 0x60)) < 0.02);
    }
}
