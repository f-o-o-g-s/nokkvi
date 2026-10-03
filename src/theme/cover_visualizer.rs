//! Visualizer colors from the playing cover: the bar gradient and peak colors
//! that Bars, Lines and Scope draw with, and the colors nokkvi's themed
//! MilkDrop presets are compiled with.
//!
//! The gradient runs through every hue the cover contributes
//! ([`CoverPalette`]), darkest-in-the-artwork first, at a lightness ramp the
//! theme's background can carry: rising on a dark palette, deepening on a
//! light one. Each stop keeps its hue's saturation (see
//! `dynamic_accent::saturation`) and is walked away from the background until
//! it stays visible there. The bar border and its opacities stay the theme's.

use iced::{Color, color::Oklch};
use nokkvi_data::types::theme_file::VisualizerColors;

use super::{
    contrast_ratio,
    dynamic_accent::{AccentSeed, CoverPalette, max_chroma, saturation, walk},
    legible_text_on,
};
use crate::theme_config::ResolvedTheme;

/// Bar gradient stops, bottom to top.
const BAR_STOPS: usize = 6;
/// Peak gradient stops.
const PEAK_STOPS: usize = 3;
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
    let clears = |c: Color| {
        contrast_ratio(c, palette.bg0_hard) >= floor && contrast_ratio(c, palette.bg0) >= floor
    };
    walk(|l| anchor.at(l), away, l, clears).1
}

/// The visualizer colors for `cover` on one palette. `base` is the theme's
/// own visualizer colors for that mode, whose border and opacities carry over.
pub(super) fn visualizer_colors(
    cover: &CoverPalette,
    palette: &ResolvedTheme,
    base: &VisualizerColors,
) -> VisualizerColors {
    let dark_bg = legible_text_on(palette.bg0_hard) == Color::WHITE;
    let (bottom, top) = if dark_bg {
        DARK_BAR_LIGHTNESS
    } else {
        LIGHT_BAR_LIGHTNESS
    };
    let peak_l = if dark_bg {
        DARK_PEAK_LIGHTNESS
    } else {
        LIGHT_PEAK_LIGHTNESS
    };

    // Bars run darkest-in-the-artwork first, so the gradient reads like the
    // cover's own shading; peaks lead with its most vivid hue.
    let mut by_lightness: Vec<(f32, Anchor)> = cover
        .colors()
        .iter()
        .map(|seed| (seed.lightness, Anchor::of(*seed)))
        .collect();
    by_lightness.sort_by(|a, b| a.0.total_cmp(&b.0));
    let bar_anchors: Vec<Anchor> = by_lightness.into_iter().map(|(_, a)| a).collect();
    let mut peak_anchors: Vec<Anchor> = cover.colors().iter().map(|s| Anchor::of(*s)).collect();
    peak_anchors.sort_by(|a, b| b.saturation.total_cmp(&a.saturation));

    let bar_gradient_colors = (0..BAR_STOPS)
        .map(|i| {
            let t = i as f32 / (BAR_STOPS - 1) as f32;
            let l = bottom + (top - bottom) * t;
            hex(visible(
                along(&bar_anchors, t),
                l,
                BAR_MIN_CONTRAST,
                palette,
            ))
        })
        .collect();
    let peak_gradient_colors = (0..PEAK_STOPS)
        .map(|i| {
            let anchor = peak_anchors[i % peak_anchors.len()];
            hex(visible(anchor, peak_l, PEAK_MIN_CONTRAST, palette))
        })
        .collect();

    VisualizerColors {
        border_color: base.border_color.clone(),
        border_opacity: base.border_opacity,
        led_border_opacity: base.led_border_opacity,
        bar_gradient_colors,
        peak_gradient_colors,
    }
}

/// The colors nokkvi's themed MilkDrop presets take from the cover. They are
/// light-on-dark, so these come from the theme's DARK palette in either mode.
#[derive(Debug, Clone)]
pub(crate) struct CoverMilkdrop {
    /// `NOKKVI_ACCENT`.
    pub accent: Color,
    /// `NOKKVI_HIGHLIGHT`.
    pub highlight: Color,
    /// The dark-palette visualizer colors, whose bar gradient is the ramp.
    pub bars: VisualizerColors,
}

/// [`CoverMilkdrop`] for `cover`, on the theme's dark `palette` with its dark
/// visualizer colors `base`.
pub(super) fn milkdrop_colors(
    cover: &CoverPalette,
    palette: &ResolvedTheme,
    base: &VisualizerColors,
) -> CoverMilkdrop {
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
        bars: visualizer_colors(cover, palette, base),
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
        .filter_map(CoverPalette::from_colors)
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
        let cover = CoverPalette::from_colors(vec![green, purple]).expect("non-empty");
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

    /// MilkDrop's accent and highlight read on the dark panel the presets draw.
    #[test]
    fn milkdrop_colors_read_on_the_dark_panel() {
        for (name, mode, palette) in all_builtin_palettes() {
            if mode != "dark" {
                continue;
            }
            for cover in covers() {
                let md = milkdrop_colors(&cover, &palette, &VisualizerColors::default());
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
