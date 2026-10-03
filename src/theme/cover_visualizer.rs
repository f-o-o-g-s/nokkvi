//! Visualizer colors from the playing cover: the bar gradient and peak colors
//! that Bars, Lines and Scope draw with, and the colors nokkvi's themed
//! MilkDrop presets are compiled with.
//!
//! The gradient runs through every hue the cover contributes
//! ([`CoverPalette`]), darkest-in-the-artwork first, at a lightness ramp the
//! theme's background can carry: rising on a dark palette, deepening on a
//! light one. Bars, Lines and Scope draw over the artwork, where a gradient in
//! the cover's own colors would vanish into it, so the whole ramp (and the
//! peaks beyond its top) is fitted inside the lightness range that stands out
//! both from the panel and from the cover's average color, keeping its spread
//! (see [`layout`]). Each stop keeps its hue's saturation (see
//! `dynamic_accent::saturation`) and is checked again with its real color.
//! When no lightness can do both, the panel wins (it is the one guarantee).
//! The bar border and its opacities stay the theme's.

use iced::{Color, color::Oklch};
use nokkvi_data::types::theme_file::VisualizerColors;

use super::{
    contrast_ratio,
    dynamic_accent::{AccentSeed, CoverPalette, max_chroma, saturation, walk},
    legible_text_on,
};
use crate::theme_config::ResolvedTheme;

/// Bar gradient stops, bottom to top (the shipped themes' count).
const BAR_STOPS: usize = 6;
/// Peak gradient stops: the six palette slots `bars.wgsl` cycles and indexes
/// by height, filled as a closed loop so Cycle never snaps.
const PEAK_STOPS: usize = 6;
/// Bar gradient lightness, bottom to top, on a dark palette and a light one.
const DARK_BAR_LIGHTNESS: (f32, f32) = (0.50, 0.86);
const LIGHT_BAR_LIGHTNESS: (f32, f32) = (0.62, 0.38);
/// Peak lightness: beyond the top of the bar gradient, so a peak cap stands
/// out from the bar under it.
const DARK_PEAK_LIGHTNESS: f32 = 0.93;
const LIGHT_PEAK_LIGHTNESS: f32 = 0.30;
/// Lowest contrast a bar stop keeps against the background: bars are shapes,
/// not text, but must never sink into the panel.
const BAR_MIN_CONTRAST: f32 = 2.0;
/// Lowest contrast a peak keeps: a 2 px cap needs more than a whole bar.
const PEAK_MIN_CONTRAST: f32 = 3.0;
/// Lowest contrast a stop keeps against the cover's average color, so it
/// still shows when drawn over the artwork.
const COVER_MIN_CONTRAST: f32 = 2.0;
/// Lightness step of the search for a stop that clears both the panel and
/// the cover, and of the grid [`layout`] scans.
const PLACE_STEP: f32 = 0.02;
/// Narrowest the bar ramp is squeezed to before it stops shrinking.
const MIN_RAMP_SPREAD: f32 = 0.12;
/// Lightness between the top of the bar ramp and the peaks, so a peak cap
/// stands out from the bar under it.
const PEAK_GAP: f32 = 0.07;
/// MilkDrop accent / highlight lightness on the dark panel its presets draw.
const MILKDROP_ACCENT_LIGHTNESS: f32 = 0.72;
const MILKDROP_HIGHLIGHT_LIGHTNESS: f32 = 0.86;

/// One anchor of the gradient: a cover hue with its saturation share.
#[derive(Debug, Clone, Copy)]
struct Anchor {
    hue: f32,
    saturation: f32,
}

impl Anchor {
    fn of(seed: AccentSeed) -> Self {
        Self {
            hue: seed.hue,
            saturation: saturation(seed),
        }
    }

    fn at(self, l: f32) -> Color {
        let l = l.clamp(0.0, 1.0);
        Color::from_oklch(Oklch {
            l,
            c: self.saturation * max_chroma(l, self.hue),
            h: self.hue,
            a: 1.0,
        })
    }

    /// The anchor `t` of the way from `self` to `other`, hue on the short arc.
    fn mix(self, other: Self, t: f32) -> Self {
        let tau = std::f32::consts::TAU;
        let mut delta = (other.hue - self.hue).rem_euclid(tau);
        if delta > std::f32::consts::PI {
            delta -= tau;
        }
        Self {
            hue: self.hue + delta * t,
            saturation: self.saturation + (other.saturation - self.saturation) * t,
        }
    }
}

/// The anchor at position `t` (0..=1) along `anchors`.
fn along(anchors: &[Anchor], t: f32) -> Anchor {
    if anchors.len() == 1 {
        return anchors[0];
    }
    let span = (anchors.len() - 1) as f32;
    let pos = (t.clamp(0.0, 1.0) * span).min(span);
    let k = (pos.floor() as usize).min(anchors.len() - 2);
    anchors[k].mix(anchors[k + 1], pos - k as f32)
}

fn hex(c: Color) -> String {
    let [r, g, b, _] = c.into_rgba8();
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// `anchor` near lightness `l`, walked away from the background until it
/// clears `floor` against both chrome backgrounds the visualizer sits on.
fn visible(anchor: Anchor, l: f32, floor: f32, palette: &ResolvedTheme) -> Color {
    let away = if legible_text_on(palette.bg0_hard) == Color::WHITE {
        1.0
    } else {
        -1.0
    };
    walk(|l| anchor.at(l), away, l, |c| on_panel(c, floor, palette)).1
}

fn on_panel(c: Color, floor: f32, palette: &ResolvedTheme) -> bool {
    contrast_ratio(c, palette.bg0_hard) >= floor && contrast_ratio(c, palette.bg0) >= floor
}

/// `anchor` at the lightness nearest `l` (searched outward on the
/// [`PLACE_STEP`] grid) where `clears` holds.
fn nearest(anchor: Anchor, l: f32, clears: impl Fn(Color) -> bool) -> Option<Color> {
    let steps = (1.0 / PLACE_STEP).ceil() as usize;
    (0..=steps)
        .flat_map(|k| {
            let offset = k as f32 * PLACE_STEP;
            [l + offset, l - offset]
        })
        .filter(|probe| (0.0..=1.0).contains(probe))
        .map(|probe| anchor.at(probe))
        .find(|c| clears(*c))
}

/// `anchor` at the lightness nearest `l` that clears `floor` on the panel AND
/// [`COVER_MIN_CONTRAST`] against `backdrop` (the cover's average color);
/// [`visible`] when no lightness does both.
fn placed(anchor: Anchor, l: f32, floor: f32, palette: &ResolvedTheme, backdrop: Color) -> Color {
    nearest(anchor, l, |c| {
        on_panel(c, floor, palette) && contrast_ratio(c, backdrop) >= COVER_MIN_CONTRAST
    })
    .unwrap_or_else(|| visible(anchor, l, floor, palette))
}

/// A peak: [`placed`] at the peak floor when it fits beside the cover, else
/// at the bar floor beside the cover (a cramped cover leaves room for only
/// one), so the cap stays at the far end of the ramp instead of falling back
/// onto the bar top.
fn placed_peak(anchor: Anchor, l: f32, palette: &ResolvedTheme, backdrop: Color) -> Color {
    let beside_cover = |floor: f32| {
        move |c: Color| {
            on_panel(c, floor, palette) && contrast_ratio(c, backdrop) >= COVER_MIN_CONTRAST
        }
    };
    nearest(anchor, l, beside_cover(PEAK_MIN_CONTRAST))
        .or_else(|| nearest(anchor, l, beside_cover(BAR_MIN_CONTRAST)))
        .unwrap_or_else(|| visible(anchor, l, PEAK_MIN_CONTRAST, palette))
}

fn grey(l: f32) -> Color {
    Color::from_oklch(Oklch {
        l: l.clamp(0.0, 1.0),
        c: 0.0,
        h: 0.0,
        a: 1.0,
    })
}

/// Where the bar ramp and the peaks sit on one palette: `(bottom, top, peak)`
/// lightness. The natural ramp when it fits; otherwise the longest stretch of
/// it (up to its natural span, at least [`MIN_RAMP_SPREAD`] where the room
/// allows) that clears the panel and the cover, with the peaks at its far
/// end. Measured on greys: lightness drives contrast, and every stop is
/// checked again with its real color.
fn layout(dark_bg: bool, palette: &ResolvedTheme, backdrop: Option<Color>) -> (f32, f32, f32) {
    let natural = if dark_bg {
        (
            DARK_BAR_LIGHTNESS.0,
            DARK_BAR_LIGHTNESS.1,
            DARK_PEAK_LIGHTNESS,
        )
    } else {
        (
            LIGHT_BAR_LIGHTNESS.0,
            LIGHT_BAR_LIGHTNESS.1,
            LIGHT_PEAK_LIGHTNESS,
        )
    };
    let Some(backdrop) = backdrop else {
        return natural;
    };
    // Work in "distance from the background" so both modes run upward.
    let flip = |l: f32| if dark_bg { l } else { 1.0 - l };
    let (b0, t0, p0) = (flip(natural.0), flip(natural.1), flip(natural.2));
    let ok = |x: f32| {
        let c = grey(flip(x));
        on_panel(c, BAR_MIN_CONTRAST, palette) && contrast_ratio(c, backdrop) >= COVER_MIN_CONTRAST
    };
    // Runs of clearing lightness on the grid.
    let steps = (1.0 / PLACE_STEP).round() as usize;
    let mut runs: Vec<(f32, f32)> = Vec::new();
    let mut open: Option<f32> = None;
    for k in 0..=steps {
        let x = k as f32 * PLACE_STEP;
        match (ok(x), open) {
            (true, None) => open = Some(x),
            (false, Some(start)) => {
                runs.push((start, x - PLACE_STEP));
                open = None;
            }
            _ => {}
        }
    }
    if let Some(start) = open {
        runs.push((start, 1.0));
    }
    let overlap = |&(a, b): &(f32, f32)| (b.min(p0) - a.max(b0)).max(0.0);
    let Some(&(a, b)) = runs.iter().max_by(|x, y| {
        overlap(x)
            .total_cmp(&overlap(y))
            .then((x.1 - x.0).total_cmp(&(y.1 - y.0)))
    }) else {
        return natural;
    };
    if a <= b0 && b >= p0 {
        return natural;
    }
    let span = (p0 - b0).min(b - a).max(MIN_RAMP_SPREAD.min(b - a));
    let bottom = b0.clamp(a, (b - span).max(a));
    let peak = (bottom + span).min(b);
    let top = (peak - PEAK_GAP.min(span * 0.45)).min(t0.max(bottom));
    (flip(bottom), flip(top), flip(peak))
}

/// The bar ramp's stops for `cover` on one palette: `BAR_STOPS` colors from
/// bottom to top, darkest-in-the-artwork hue first. With a `backdrop`, every
/// stop also stands out from the cover; without one, only from the panel.
fn bar_stops(
    cover: &CoverPalette,
    palette: &ResolvedTheme,
    backdrop: Option<Color>,
) -> (Vec<Color>, f32) {
    let dark_bg = legible_text_on(palette.bg0_hard) == Color::WHITE;
    let (bottom, top, peak) = layout(dark_bg, palette, backdrop);
    let mut by_lightness: Vec<(f32, Anchor)> = cover
        .colors()
        .iter()
        .map(|seed| (seed.lightness, Anchor::of(*seed)))
        .collect();
    by_lightness.sort_by(|a, b| a.0.total_cmp(&b.0));
    let anchors: Vec<Anchor> = by_lightness.into_iter().map(|(_, a)| a).collect();
    let stops = (0..BAR_STOPS)
        .map(|i| {
            let t = i as f32 / (BAR_STOPS - 1) as f32;
            let l = bottom + (top - bottom) * t;
            let anchor = along(&anchors, t);
            match backdrop {
                Some(backdrop) => placed(anchor, l, BAR_MIN_CONTRAST, palette, backdrop),
                None => visible(anchor, l, BAR_MIN_CONTRAST, palette),
            }
        })
        .collect();
    (stops, peak)
}

/// The visualizer colors for `cover` on one palette. `base` is the theme's
/// own visualizer colors for that mode, whose border and opacities carry over.
pub(super) fn visualizer_colors(
    cover: &CoverPalette,
    palette: &ResolvedTheme,
    base: &VisualizerColors,
) -> VisualizerColors {
    let backdrop = grey(cover.backdrop_lightness());
    let (bars, peak_l) = bar_stops(cover, palette, Some(backdrop));

    // Peaks lead with the cover's most vivid hue and loop back to it, so the
    // Cycle mode's wrap from the last slot to the first is not a jump.
    let mut peak_anchors: Vec<Anchor> = cover.colors().iter().map(|s| Anchor::of(*s)).collect();
    peak_anchors.sort_by(|a, b| b.saturation.total_cmp(&a.saturation));
    let mut peak_loop = peak_anchors.clone();
    peak_loop.push(peak_anchors[0]);
    let peak_gradient_colors = (0..PEAK_STOPS)
        .map(|i| {
            let anchor = along(&peak_loop, i as f32 / PEAK_STOPS as f32);
            hex(placed_peak(anchor, peak_l, palette, backdrop))
        })
        .collect();

    VisualizerColors {
        border_color: base.border_color.clone(),
        border_opacity: base.border_opacity,
        led_border_opacity: base.led_border_opacity,
        bar_gradient_colors: bars.into_iter().map(hex).collect(),
        peak_gradient_colors,
    }
}

/// The colors nokkvi's themed MilkDrop presets take from the cover. They are
/// light-on-dark, so these come from the theme's DARK palette in either mode.
/// A preset draws its own canvas rather than over the artwork, so the ramp
/// only has to stand out from the panel.
#[derive(Debug, Clone)]
pub(crate) struct CoverMilkdrop {
    /// `NOKKVI_ACCENT`.
    pub accent: Color,
    /// `NOKKVI_HIGHLIGHT`.
    pub highlight: Color,
    /// `NOKKVI_RAMP0..5`, dark to light.
    pub ramp: Vec<Color>,
}

/// [`CoverMilkdrop`] for `cover`, on the theme's dark `palette`.
pub(super) fn milkdrop_colors(cover: &CoverPalette, palette: &ResolvedTheme) -> CoverMilkdrop {
    let primary = Anchor::of(cover.primary());
    CoverMilkdrop {
        accent: visible(
            primary,
            MILKDROP_ACCENT_LIGHTNESS,
            PEAK_MIN_CONTRAST,
            palette,
        ),
        highlight: visible(
            primary,
            MILKDROP_HIGHLIGHT_LIGHTNESS,
            PEAK_MIN_CONTRAST,
            palette,
        ),
        ramp: bar_stops(cover, palette, None).0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::tests::{all_builtin_palettes, builtin_visualizer as dual_visualizer};

    fn seed(r: u8, g: u8, b: u8) -> AccentSeed {
        let Oklch { l, c, h, .. } = Color::from_rgb8(r, g, b).into_oklch();
        AccentSeed {
            lightness: l,
            chroma: c,
            hue: h,
        }
    }

    fn covers() -> Vec<CoverPalette> {
        [
            vec![seed(0xd0, 0x28, 0x30)],
            vec![seed(0xf2, 0xd0, 0x20)],
            vec![seed(0x8a, 0x3c, 0xd0), seed(0x3c, 0xa8, 0x48)],
            vec![
                seed(0xe8, 0x7a, 0x1c),
                seed(0x20, 0x40, 0x90),
                seed(0xd0, 0x28, 0x30),
            ],
            vec![seed(0x10, 0x18, 0x40)],
        ]
        .into_iter()
        .filter_map(|colors| CoverPalette::from_colors(colors, 0.45))
        .collect()
    }

    fn parse(hex: &str) -> Color {
        crate::theme_config::parse_hex_color(hex).expect("generated hex parses")
    }

    fn hue_distance(a: f32, b: f32) -> f32 {
        let d = (a - b).rem_euclid(std::f32::consts::TAU);
        d.min(std::f32::consts::TAU - d)
    }

    /// Every generated stop parses, the counts match what the shaders pad
    /// from, and every bar and peak stays visible on the panel, on every
    /// shipped theme in both modes.
    #[test]
    fn cover_gradients_stay_visible_on_every_theme() {
        for (name, mode, palette) in all_builtin_palettes() {
            let base = dual_visualizer(&name, mode);
            for cover in covers() {
                let viz = visualizer_colors(&cover, &palette, &base);
                assert_eq!(viz.bar_gradient_colors.len(), BAR_STOPS);
                assert_eq!(viz.peak_gradient_colors.len(), PEAK_STOPS);
                for (kind, list, floor) in [
                    ("bar", &viz.bar_gradient_colors, BAR_MIN_CONTRAST),
                    ("peak", &viz.peak_gradient_colors, PEAK_MIN_CONTRAST),
                ] {
                    for stop in list {
                        let c = parse(stop);
                        let cr = contrast_ratio(c, palette.bg0_hard);
                        assert!(
                            cr >= floor - 0.05,
                            "{name}/{mode} {kind} {stop} only {cr:.2}:1 on bg0_hard"
                        );
                    }
                }
            }
        }
    }

    /// The border and its opacities are the theme's; only the gradients move.
    #[test]
    fn the_theme_keeps_its_border() {
        let (_, _, palette) = all_builtin_palettes().remove(0);
        let base = VisualizerColors::default();
        let viz = visualizer_colors(&covers()[0], &palette, &base);
        assert_eq!(viz.border_color, base.border_color);
        assert_eq!(viz.border_opacity, base.border_opacity);
        assert_eq!(viz.led_border_opacity, base.led_border_opacity);
    }

    /// A two-hue cover's gradient starts at one hue and ends at the other.
    #[test]
    fn a_two_hue_cover_runs_from_one_hue_to_the_other() {
        let purple = seed(0x5a, 0x20, 0x90);
        let green = seed(0x60, 0xd0, 0x70);
        let cover = CoverPalette::from_colors(vec![green, purple], 0.3).expect("non-empty");
        for (name, mode, palette) in all_builtin_palettes() {
            let viz = visualizer_colors(&cover, &palette, &VisualizerColors::default());
            let first = parse(&viz.bar_gradient_colors[0]).into_oklch();
            let last = parse(&viz.bar_gradient_colors[BAR_STOPS - 1]).into_oklch();
            // Purple is darker in the artwork, so it anchors the bottom.
            assert!(
                hue_distance(first.h, purple.hue) < 0.2,
                "{name}/{mode} bottom hue {:.2}",
                first.h
            );
            assert!(
                hue_distance(last.h, green.hue) < 0.2,
                "{name}/{mode} top hue {:.2}",
                last.h
            );
        }
    }

    /// A single-hue cover keeps that hue through the whole gradient.
    #[test]
    fn a_single_hue_cover_keeps_its_hue() {
        let red = seed(0xd0, 0x28, 0x30);
        let cover = CoverPalette::single(red);
        for (name, mode, palette) in all_builtin_palettes() {
            let viz = visualizer_colors(&cover, &palette, &VisualizerColors::default());
            for stop in viz
                .bar_gradient_colors
                .iter()
                .chain(&viz.peak_gradient_colors)
            {
                let c = parse(stop).into_oklch();
                if c.c > 0.04 {
                    assert!(
                        hue_distance(c.h, red.hue) < 0.15,
                        "{name}/{mode} stop {stop} drifted off red"
                    );
                }
            }
        }
    }

    /// The case the owner's screenshots caught: a green gradient drawn over a
    /// green cover vanished. Every stop now stands out from the cover's
    /// average color as well as from the panel, wherever both can be met.
    #[test]
    fn stops_stand_out_from_the_cover_they_draw_over() {
        let green = seed(0x30, 0xa0, 0x30);
        for backdrop_l in [0.3_f32, 0.5, 0.62, 0.8] {
            let cover = CoverPalette::from_colors(vec![green], backdrop_l).expect("non-empty");
            let backdrop = Color::from_oklch(Oklch {
                l: backdrop_l,
                c: 0.0,
                h: 0.0,
                a: 1.0,
            });
            for (name, mode, palette) in all_builtin_palettes() {
                let viz = visualizer_colors(&cover, &palette, &VisualizerColors::default());
                // Only where a lightness can clear both does the cover floor
                // apply; the panel floor is pinned on every case above.
                let feasible = (0..=50).any(|k| {
                    let c = Anchor::of(green).at(k as f32 / 50.0);
                    on_panel(c, BAR_MIN_CONTRAST, &palette)
                        && contrast_ratio(c, backdrop) >= COVER_MIN_CONTRAST
                });
                if !feasible {
                    continue;
                }
                for stop in &viz.bar_gradient_colors {
                    let cr = contrast_ratio(parse(stop), backdrop);
                    assert!(
                        cr >= COVER_MIN_CONTRAST - 0.05,
                        "{name}/{mode} backdrop L {backdrop_l}: {stop} only {cr:.2}:1 on the cover"
                    );
                }
            }
        }
    }

    /// The review's case: a near-white cover on a dark theme (and a dark one
    /// on a light theme) left the ramp's top flat and the peaks the color of
    /// the bar tops. The ramp keeps a spread and the peaks sit beyond it.
    #[test]
    fn a_tight_cover_keeps_the_ramp_spread_and_peaks_apart() {
        let green = seed(0x30, 0xa0, 0x30);
        for (backdrop_l, want_dark_bg) in [(0.85_f32, true), (0.2, false)] {
            let cover = CoverPalette::from_colors(vec![green], backdrop_l).expect("non-empty");
            for (name, mode, palette) in all_builtin_palettes() {
                let dark_bg = legible_text_on(palette.bg0_hard) == Color::WHITE;
                if dark_bg != want_dark_bg {
                    continue;
                }
                let viz = visualizer_colors(&cover, &palette, &VisualizerColors::default());
                let l = |hex: &str| parse(hex).into_oklch().l;
                let first = l(&viz.bar_gradient_colors[0]);
                let last = l(&viz.bar_gradient_colors[BAR_STOPS - 1]);
                // The window between the panel and a near-white cover can be
                // as narrow as ~0.16 (Everforest dark); the ramp keeps a
                // visible spread and leaves the rest to the peaks.
                assert!(
                    (last - first).abs() >= 0.06,
                    "{name}/{mode} backdrop {backdrop_l}: ramp collapsed ({first:.2}..{last:.2})"
                );
                let peak = l(&viz.peak_gradient_colors[0]);
                assert!(
                    (peak - last).abs() >= 0.03,
                    "{name}/{mode} backdrop {backdrop_l}: peak {peak:.2} on the bar top {last:.2}"
                );
            }
        }
    }

    /// Six peak slots, as `bars.wgsl` reads them, looping back to the first
    /// hue so Cycle never snaps.
    #[test]
    fn peaks_fill_six_slots_and_loop() {
        let a = seed(0xd0, 0x28, 0x30);
        let b = seed(0x20, 0x90, 0xd0);
        let cover = CoverPalette::from_colors(vec![a, b], 0.3).expect("non-empty");
        let (_, _, palette) = all_builtin_palettes().remove(0);
        let viz = visualizer_colors(&cover, &palette, &VisualizerColors::default());
        assert_eq!(viz.peak_gradient_colors.len(), 6);
        let hue = |i: usize| parse(&viz.peak_gradient_colors[i]).into_oklch().h;
        // Most vivid first, the other hue mid-loop, and back toward the first.
        assert!(
            hue_distance(hue(3), hue(0)) > 0.5,
            "mid-loop is the second hue"
        );
        assert!(
            hue_distance(hue(5), hue(0)) < hue_distance(hue(3), hue(0)),
            "the loop returns toward the first hue"
        );
    }

    /// MilkDrop's accent and highlight read on the dark panel the presets draw.
    #[test]
    fn milkdrop_colors_read_on_the_dark_panel() {
        for (name, mode, palette) in all_builtin_palettes() {
            if mode != "dark" {
                continue;
            }
            for cover in covers() {
                let md = milkdrop_colors(&cover, &palette);
                for (role, c) in [("accent", md.accent), ("highlight", md.highlight)] {
                    let cr = contrast_ratio(c, palette.bg0_hard);
                    assert!(
                        cr >= PEAK_MIN_CONTRAST - 0.05,
                        "{name} {role} only {cr:.2}:1"
                    );
                }
            }
        }
    }
}
