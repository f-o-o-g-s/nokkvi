//! Embedded SVG Icons
//!
//! All SVG icons are embedded at compile time. The lookup table is generated
//! by `build.rs` from the contents of `assets/icons/`, so adding/removing an
//! icon is a one-step change (drop or remove the file in the directory).
//!
//! See `build.rs::generate_embedded_svg_table` for the generator.

use iced::{Color, widget::svg};
use tracing::warn;

mod generated {
    include!(concat!(env!("OUT_DIR"), "/embedded_svg_generated.rs"));
}

/// Get the SVG content for a given icon path.
///
/// Returns `play.svg` as the fallback when the path is unregistered. The
/// fallback path is the silent failure mode that the test
/// `all_svg_paths_in_source_are_registered` exists to catch.
pub(crate) fn get_svg(path: &str) -> &'static str {
    if let Some(content) = generated::lookup(path) {
        return content;
    }
    warn!("  Unknown SVG path: {}", path);
    generated::FALLBACK
}

/// Create an SVG widget from an embedded icon path.
///
/// Drop-in replacement for `svg(Handle::from_path(path))` that uses embedded
/// SVG bytes instead of touching the filesystem.
pub(crate) fn svg_widget<'a>(path: &str) -> svg::Svg<'a> {
    let svg_content = get_svg(path);
    let handle = svg::Handle::from_memory(svg_content.as_bytes());
    svg(handle)
}

// ============================================================================
// Themed Logo SVG
// ============================================================================

/// The Nokkvi logo SVG template. Contains hardcoded hex colors that
/// `themed_logo_svg()` rewrites at runtime to match the active theme.
const LOGO_SVG: &str = include_str!("../assets/nokkvi_logo.svg");

/// Convert an `iced::Color` to a `#rrggbb` hex string for SVG fill replacement.
fn color_to_hex(c: Color) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        (c.r * 255.0) as u8,
        (c.g * 255.0) as u8,
        (c.b * 255.0) as u8,
    )
}

/// Return the Nokkvi logo SVG with fills remapped to the active theme.
///
/// Performs string replacement on the compile-time SVG template:
/// - Swaps the hardcoded `#ebdbb2` hex color for the current theme's `fg1()`.
/// - Swaps the hardcoded `#458588` hex color for the current theme's `accent()`.
pub(crate) fn themed_logo_svg() -> String {
    use crate::theme;

    LOGO_SVG
        .replace("#ebdbb2", &color_to_hex(theme::fg1()))
        .replace("#458588", &color_to_hex(theme::accent()))
}

/// Stroke width baked into the boat SVG, in viewBox units. The logo's
/// viewBox is 80 wide; with the boat displayed at ~27 px tall, 1 viewBox
/// unit ≈ 0.34 display pixels, so a stroke-width of 1.5 lands at ~0.5
/// display pixels — half the wave line's `LinesConfig::outline_thickness`
/// default of `1.0` px, which read as too heavy on the small boat sprite.
/// SVG strokes are centered on the path, so the visible stroke extends
/// ~0.25 px on each side of the boat's edge.
const BOAT_STROKE_WIDTH_SVG_UNITS: f32 = 1.5;

/// Return the themed logo SVG with a tilt rotation, optional horizontal
/// mirror, and a theme-matched stroke baked into its path data — used by
/// the boat overlay.
///
/// The logo's viewBox is `60 89.5 80 80` (x range `[60, 140]`, y range
/// `[89.5, 169.5]`, center `(100, 129.5)`). All paths get wrapped in a
/// single `<g transform="...">` whose composition is rotate-then-mirror
/// (rightmost SVG transform applies first to coordinates):
///
/// - `scale(-1, 1)` then `translate(200, 0)` reflects every point `x` to
///   `200 - x`, mapping viewBox edges `60 ↔ 140` and leaving the center
///   fixed — that's the horizontal flip used when sailing leftward so
///   the sail catches wind from behind regardless of travel direction.
/// - `rotate(deg, 100, 129.5)` then rotates the (possibly mirrored)
///   geometry around the viewBox center by `deg` degrees. SVG rotate is
///   clockwise for positive degrees in screen coords (Y down), matching
///   iced's rotation convention — so the tilt sign computed in
///   `widgets/boat.rs::step()` (negative for "bow up uphill rightward")
///   carries through unchanged after `f32::to_degrees()`.
///
/// The whole point of baking the rotation into the SVG (rather than
/// passing it to `iced::widget::Svg::rotation()`) is to stay in vector
/// land for as long as possible: resvg rasterizes the *already-rotated*
/// paths at the boat's display size, instead of rasterizing the upright
/// boat first and then rotating the bitmap. The latter aliases visibly
/// at small sprite sizes; the former produces a clean fresh rasterization
/// at every quantized angle.
///
/// The stroke uses the active visualizer theme's `border_color` and
/// `border_opacity` (same source as the lines-mode wave outline, so
/// it's thematically consistent), with `stroke-linejoin="round"` so the
/// joins between the boat's compound subpaths don't spike. Theme changes
/// are picked up automatically via the cache invalidation in
/// `BoatState::clear_if_theme_changed` — both the fill and stroke colors
/// are read from `crate::theme`, which bumps `theme_generation()` on any
/// reload or light/dark flip.
pub(crate) fn themed_boat_svg(angle_radians: f32, mirrored: bool) -> String {
    let viz_colors = crate::theme::get_visualizer_colors();
    let stroke_attrs = format!(
        "stroke=\"{stroke}\" stroke-width=\"{width}\" stroke-opacity=\"{opacity}\" stroke-linejoin=\"round\" ",
        stroke = viz_colors.border_color,
        width = BOAT_STROKE_WIDTH_SVG_UNITS,
        opacity = viz_colors.border_opacity,
    );
    let mut body = themed_logo_svg().replace("<path fill=", &format!("<path {stroke_attrs}fill="));

    let degrees = angle_radians.to_degrees();
    let nonzero_rotation = degrees.abs() > 1e-4;
    if nonzero_rotation || mirrored {
        let mut transform = String::new();
        if nonzero_rotation {
            transform.push_str(&format!("rotate({degrees} 100 129.5)"));
        }
        if mirrored {
            if !transform.is_empty() {
                transform.push(' ');
            }
            transform.push_str("translate(200 0) scale(-1 1)");
        }
        body = body
            .replacen(
                "xmlns=\"http://www.w3.org/2000/svg\">",
                &format!("xmlns=\"http://www.w3.org/2000/svg\"><g transform=\"{transform}\">"),
                1,
            )
            .replacen("</svg>", "</g></svg>", 1);
    }
    body
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    /// Recursively collect all `.rs` files under a directory, skipping
    /// `embedded_svg.rs` (which contains the generated KNOWN_PATHS itself).
    fn collect_rs_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    collect_rs_files(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs")
                    && !path.ends_with("embedded_svg.rs")
                {
                    out.push(path);
                }
            }
        }
    }

    /// Extract all `"assets/icons/....svg"` path literals from source text.
    fn extract_svg_paths(source: &str) -> Vec<String> {
        let needle = "\"assets/icons/";
        let mut paths = Vec::new();
        let mut start = 0;
        while let Some(pos) = source[start..].find(needle) {
            let abs = start + pos + 1; // skip opening quote
            if let Some(end) = source[abs..].find('"') {
                let path = &source[abs..abs + end];
                if path.ends_with(".svg") {
                    paths.push(path.to_string());
                }
            }
            start = abs + 1;
        }
        paths
    }

    /// Scan all `.rs` source files for `"assets/icons/*.svg"` string literals
    /// and verify every one is registered in the generated lookup table.
    ///
    /// Catches the silent play-icon fallback shipping unnoticed when a view
    /// references an icon that does not exist on disk under `assets/icons/`.
    #[test]
    fn all_svg_paths_in_source_are_registered() {
        let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut rs_files = Vec::new();
        collect_rs_files(&src_dir, &mut rs_files);

        let known: BTreeSet<&str> = generated::KNOWN_PATHS.iter().copied().collect();
        let mut unregistered: BTreeSet<String> = BTreeSet::new();
        for file in &rs_files {
            let contents = std::fs::read_to_string(file).unwrap();
            for path in extract_svg_paths(&contents) {
                if !known.contains(path.as_str()) {
                    unregistered.insert(path);
                }
            }
        }

        assert!(
            unregistered.is_empty(),
            "SVG paths used in source but not present in assets/icons/:\n{}",
            unregistered
                .iter()
                .map(|p| format!("  - {p}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    /// Verify the generator's KNOWN_PATHS list matches the on-disk contents
    /// of `assets/icons/`. Cheap re-confirmation that build-time codegen
    /// observed every file the test sees at runtime.
    #[test]
    fn generated_paths_match_assets_dir() {
        let icons_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("icons");
        let mut on_disk: Vec<String> = std::fs::read_dir(&icons_dir)
            .expect("read assets/icons")
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                if path.extension().is_some_and(|e| e == "svg") {
                    let name = path.file_name()?.to_str()?.to_owned();
                    Some(format!("assets/icons/{name}"))
                } else {
                    None
                }
            })
            .collect();
        on_disk.sort();

        let mut known: Vec<String> = generated::KNOWN_PATHS
            .iter()
            .map(|s| s.to_string())
            .collect();
        known.sort();

        assert_eq!(
            on_disk, known,
            "generated KNOWN_PATHS drifted from assets/icons/ contents — rebuild"
        );
    }

    /// Every boat SVG variant must carry the active theme's visualizer
    /// border color and opacity as a stroke on each path, alongside the
    /// existing fill — that's how the boat outline reads as part of the
    /// same theme as the lines-mode wave outline. Sourced from
    /// `crate::theme::get_visualizer_colors()`, which is the same
    /// accessor `widgets/visualizer/mod.rs` uses for the wave's border.
    #[test]
    fn themed_boat_svg_bakes_theme_stroke_into_every_path() {
        let viz = crate::theme::get_visualizer_colors();
        let out = themed_boat_svg(0.0, false);
        assert!(
            out.contains(&format!("stroke=\"{}\"", viz.border_color)),
            "stroke color must come from the active theme's border_color \
             (expected stroke=\"{}\", got {out:?})",
            viz.border_color,
        );
        assert!(
            out.contains(&format!("stroke-opacity=\"{}\"", viz.border_opacity)),
            "stroke opacity must come from the active theme's border_opacity \
             (expected stroke-opacity=\"{}\", got {out:?})",
            viz.border_opacity,
        );
        assert!(
            out.contains("stroke-linejoin=\"round\""),
            "stroke must use rounded joins so the boat's compound subpath \
             corners don't spike (got {out:?})"
        );
        // The logo has two `<path fill=...>` elements (visualizer-bars
        // sub-graphic + boat hull). Both must receive the stroke.
        let stroked_paths = out.matches("stroke=\"").count();
        assert_eq!(
            stroked_paths, 2,
            "every <path> must carry a stroke (got {stroked_paths})"
        );
    }

    /// A non-zero rotation must inject a `rotate(...)` SVG transform
    /// around the viewBox center `(100, 129.5)`. We don't pin the exact
    /// degrees string (formatting differs between e.g. `5` and `5.0`) —
    /// we just confirm the keyword and the pivot are present.
    #[test]
    fn themed_boat_svg_bakes_rotation_around_viewbox_center() {
        let out = themed_boat_svg(0.1, false);
        assert!(
            out.contains("<g transform=\""),
            "rotated boat must wrap content in a <g transform=...>"
        );
        assert!(
            out.contains("rotate("),
            "non-zero angle must use rotate(...) transform"
        );
        assert!(
            out.contains("100 129.5"),
            "rotation pivot must be the viewBox center"
        );
        assert!(
            out.ends_with("</g></svg>\n") || out.ends_with("</g></svg>"),
            "wrapping group must close before the </svg>"
        );
    }

    /// `mirrored = true` with zero rotation must produce just the mirror
    /// transform (no rotate). Confirms the path used by leftward-sailing
    /// boats while exactly upright.
    #[test]
    fn themed_boat_svg_mirror_only_omits_rotate() {
        let out = themed_boat_svg(0.0, true);
        assert!(
            out.contains("translate(200 0) scale(-1 1)"),
            "mirrored boat must apply the horizontal-flip transform"
        );
        assert!(
            !out.contains("rotate("),
            "zero rotation must not emit rotate(...)"
        );
    }

    /// Sanity check: every registered path resolves to non-fallback content
    /// (except `play.svg`, which IS the fallback).
    #[test]
    fn every_known_path_resolves_to_unique_content() {
        for &path in generated::KNOWN_PATHS {
            let result = get_svg(path);
            if path != "assets/icons/play.svg" {
                assert_ne!(
                    result as *const str,
                    generated::FALLBACK as *const str,
                    "Path '{path}' resolved to FALLBACK — generator bug"
                );
            }
        }
    }
}
