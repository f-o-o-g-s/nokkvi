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

/// The Nokkvi longship logo SVG template — the canonical master produced by
/// `scripts/gen-logo-assets.sh` (scoured, lowercased hex, per-path stroke
/// widths kept). It carries one unique lowercase hex per semantic role, which
/// `themed_logo_svg()` rewrites at runtime to the active theme. Because each
/// token equals the Svalbard default value, the untouched master also renders
/// correctly as the default theme.
const LOGO_SVG: &str = include_str!("../assets/nokkvi_logo.svg");

/// Sentinel hex per semantic role — the exact (lowercased) fill/stroke literals
/// the master ships. `str::replace` is case-sensitive, which is why the
/// generator lowercases every hex; `themed_logo_svg_remaps_roles_on_theme_flip`
/// guards that each token is still present and actually gets remapped.
const LOGO_TOKEN_BODY: &str = "#e7eceb"; // sail + hull (off-white)
const LOGO_TOKEN_SHIELDS: &str = "#6d9a94"; // 3 shield blocks (teal)
const LOGO_TOKEN_WOOD: &str = "#cba576"; // mast + yard (tan)
const LOGO_TOKEN_OUTLINE: &str = "#111817"; // group stroke (near-black)

/// The master's `viewBox` as `[min_x, min_y, width, height]`. Every piece of
/// boat geometry (rotation pivot, mirror/flip translations, padding) is derived
/// from this — no center/pad/transform literal is hardcoded.
/// `logo_viewbox_const_matches_master_svg` asserts it tracks the SVG.
const LOGO_VIEWBOX: [f32; 4] = [70.96, 119.96, 881.07, 881.07];

/// Convert an `iced::Color` to a `#rrggbb` hex string for SVG fill replacement.
fn color_to_hex(c: Color) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        (c.r * 255.0) as u8,
        (c.g * 255.0) as u8,
        (c.b * 255.0) as u8,
    )
}

/// Return the Nokkvi logo SVG with its fills remapped to the active theme:
/// body (sail + hull) → `fg0()`, shields (the three blocks) → `accent()`,
/// wood (mast + yard) → `warning()`.
///
/// The near-black group outline (`LOGO_TOKEN_OUTLINE`) is deliberately left
/// fixed so the mark keeps its shape definition on light themes (where a
/// theme-tracked outline could go transparent). The boat overlay overrides
/// that stroke separately in `themed_boat_svg()`.
pub(crate) fn themed_logo_svg() -> String {
    use crate::theme;

    LOGO_SVG
        .replace(LOGO_TOKEN_BODY, &color_to_hex(theme::fg0()))
        .replace(LOGO_TOKEN_SHIELDS, &color_to_hex(theme::accent()))
        .replace(LOGO_TOKEN_WOOD, &color_to_hex(theme::warning()))
}

/// Boat-context group stroke width as a fraction of the viewBox width. The
/// master's authored group stroke is `60` units (~6.8% of the 881-unit
/// viewBox) — far too heavy for the 48–160 px boat sprite. The boat keeps the
/// master's `paint-order:stroke markers fill`, so the stroke paints behind the
/// fill and only its outer half is visible; `0.0375` (~33 units) therefore
/// reads as a ~1.9%-of-viewBox hairline, matching the old boat's visible
/// outline weight and the lines-mode wave outline.
const BOAT_GROUP_STROKE_FRACTION: f32 = 0.0375;

/// Per-side viewBox padding for the boat SVG, expressed as a fraction of the
/// boat sprite's visual size. `widgets/boat.rs::boat_overlay` scales the iced
/// container by `1 + 2 · BOAT_VIEWBOX_PAD_FRACTION` so the boat's *content*
/// keeps its target display size while the padded margin around it stays
/// transparent.
///
/// Sized to fit a `MAX_TILT ≈ 17°` rotation without clipping the rotated
/// bounding box's corners: an `881.07`-unit square rotated by θ around its
/// center spans `881.07·(|cos θ| + |sin θ|)` ≈ `1100` units at 17°, inside the
/// `881.07 + 2·132.16 ≈ 1145.4`-unit padded extent.
pub(crate) const BOAT_VIEWBOX_PAD_FRACTION: f32 = 0.15;

/// Remove every `stroke-width="…"` presentation attribute from an SVG string.
/// The boat uses this to drop the master's per-path rigging weights (mast 24,
/// sail 54.83, blocks 52–59.5, …) so every path inherits one uniform, scaled
/// group stroke instead of a jumble of heavy per-path outlines.
fn strip_stroke_width_attrs(svg: &str) -> String {
    const NEEDLE: &str = " stroke-width=\"";
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(pos) = rest.find(NEEDLE) {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + NEEDLE.len()..];
        match after.find('"') {
            Some(end) => rest = &after[end + 1..],
            None => {
                rest = after; // malformed; stop stripping
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Return the themed logo SVG reshaped for the boat overlay: a uniform
/// theme-matched outline, plus an optional tilt rotation, horizontal mirror,
/// and vertical flip — all baked into the SVG so resvg rasterizes the
/// already-transformed vectors crisply at the small sprite size.
///
/// Geometry is derived entirely from `LOGO_VIEWBOX`
/// (`70.96 119.96 881.07 881.07`, center `(511.495, 560.495)`):
///
/// - The master's authored `#111817` group stroke is recolored to the active
///   visualizer `border_color`/`border_opacity` (same source as the lines-mode
///   wave outline), its width rescaled to `w · BOAT_GROUP_STROKE_FRACTION`, and
///   the per-path rigging widths stripped so the whole boat shares one
///   hairline. `paint-order:stroke markers fill` is KEPT so the stroke paints
///   behind the fill, hiding the interior seams of the compound hull and
///   center-shield subpaths.
/// - `rotate(deg, cx, cy)` tilts around the viewBox center. SVG rotate is
///   clockwise-positive in screen coords (Y down), matching iced — so the tilt
///   sign from `widgets/boat.rs::step()` (negative for "bow up uphill
///   rightward") carries through unchanged after `f32::to_degrees()`.
/// - `translate(2·cx, 0) scale(-1, 1)` mirrors horizontally about the center
///   (leftward sailing, so the sail still catches wind from behind);
///   `translate(0, 2·cy) scale(1, -1)` flips vertically (the inverted boat
///   surfing the lower wave in mirrored line mode).
///
/// The transforms compose vertical-flip → rotate → horizontal-mirror (SVG
/// applies the rightmost to a point's coords first, so the vertical flip is the
/// outermost reflection — what reads as bow-up-on-the-right becomes
/// bow-down-on-the-right, the correct lean for the lower wave). They wrap the
/// master's `<g id="nokkvi">` in an outer `<g transform="…">`. Baking the
/// rotation into vector space (rather than `Svg::rotation()`) lets resvg
/// re-rasterize the already-rotated paths at each quantized angle, avoiding the
/// aliasing a rotated bitmap shows at small sizes. Theme changes are picked up
/// automatically via `BoatState::clear_if_theme_changed`, which keys on
/// `theme_generation()`.
pub(crate) fn themed_boat_svg(angle_radians: f32, mirrored: bool, vertical_flip: bool) -> String {
    let viz = crate::theme::get_visualizer_colors();
    let [min_x, min_y, w, h] = LOGO_VIEWBOX;
    let cx = min_x + w / 2.0;
    let cy = min_y + h / 2.0;

    // (1) Uniform themed outline: strip the per-path rigging widths, then
    //     recolor + rescale the single group stroke. paint-order is KEPT
    //     (stroke behind fill) so the fill hides the interior seams where the
    //     hull's two mirrored halves and the center shield's sub-rects abut —
    //     dropping it draws those seams as lines down the middle of the boat.
    //     Only the outer half of the stroke shows, which is why the fraction is
    //     sized ~2× (see BOAT_GROUP_STROKE_FRACTION). `stroke="#111817"` occurs
    //     only on the group (fills are `fill="…"`), so the recolor can't touch a
    //     path fill.
    let boat_stroke_w = w * BOAT_GROUP_STROKE_FRACTION;
    let mut body = strip_stroke_width_attrs(&themed_logo_svg()).replace(
        &format!("stroke=\"{LOGO_TOKEN_OUTLINE}\""),
        &format!(
            "stroke=\"{}\" stroke-opacity=\"{}\" stroke-width=\"{}\"",
            viz.border_color, viz.border_opacity, boat_stroke_w
        ),
    );

    // (2) Pad the viewBox in place so a `MAX_TILT` rotation can't clip the
    //     rotated bounding box. Found and replaced (not reconstructed) so f32
    //     formatting of the original bounds can never desync the anchor.
    let pad = w.max(h) * BOAT_VIEWBOX_PAD_FRACTION;
    let padded = format!(
        "{} {} {} {}",
        min_x - pad,
        min_y - pad,
        w + 2.0 * pad,
        h + 2.0 * pad
    );
    if let Some(vb) = body.find("viewBox=\"") {
        let start = vb + "viewBox=\"".len();
        if let Some(len) = body[start..].find('"') {
            body.replace_range(start..start + len, &padded);
        }
    }

    // (3) Compose and inject the transform group (outermost = vertical flip)
    //     around the master's `<g id="nokkvi">`.
    let degrees = angle_radians.to_degrees();
    let nonzero_rotation = degrees.abs() > 1e-4;
    if nonzero_rotation || mirrored || vertical_flip {
        let mut transform = String::new();
        if vertical_flip {
            transform.push_str(&format!("translate(0 {}) scale(1 -1)", 2.0 * cy));
        }
        if nonzero_rotation {
            if !transform.is_empty() {
                transform.push(' ');
            }
            transform.push_str(&format!("rotate({degrees} {cx} {cy})"));
        }
        if mirrored {
            if !transform.is_empty() {
                transform.push(' ');
            }
            transform.push_str(&format!("translate({} 0) scale(-1 1)", 2.0 * cx));
        }
        body = body
            .replacen(
                "<g id=\"nokkvi\"",
                &format!("<g transform=\"{transform}\"><g id=\"nokkvi\""),
                1,
            )
            .replacen("</svg>", "</g></svg>", 1);
    }
    body
}

// ============================================================================
// Themed Anchor SVG (drops-anchor doodad on the boat overlay)
// ============================================================================

/// Stroke width baked into the anchor SVG, in viewBox units. The viewBox
/// is 24 units tall; the sprite renders at roughly 1/3 of the boat's
/// height, so 1 viewBox unit ≈ 0.4 display pixels. Stroke-width 1.4
/// lands at ~0.5 px — same visual weight as the boat outline.
const ANCHOR_STROKE_WIDTH_SVG_UNITS: f32 = 1.4;

/// Return the themed lucide-anchor body as standalone SVG, no rope.
///
/// The rope is rendered separately as a curved canvas path in
/// `widgets/boat.rs` so it can sway naturally with the wave action; this
/// helper provides only the static anchor sprite that hangs at the
/// rope's bottom end.
///
/// Stroke uses the active visualizer theme's `border_color` /
/// `border_opacity`, same source as the lines-mode wave outline and the
/// boat outline, so every part of the doodad (boat, rope, anchor) shares
/// one palette. Cache invalidation lives on `BoatState`, keyed against
/// `theme_generation()` so a light/dark flip or palette swap rebuilds
/// the handle on the next render.
pub(crate) fn themed_anchor_svg() -> String {
    let viz = crate::theme::get_visualizer_colors();
    let stroke = viz.border_color;
    let opacity = viz.border_opacity;
    let stroke_w = ANCHOR_STROKE_WIDTH_SVG_UNITS;

    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" \
         fill=\"none\" stroke=\"{stroke}\" stroke-opacity=\"{opacity}\" \
         stroke-width=\"{stroke_w}\" stroke-linecap=\"round\" stroke-linejoin=\"round\">\
         <path d=\"M12 6v16\" />\
         <path d=\"m19 13 2-1a9 9 0 0 1-18 0l2 1\" />\
         <path d=\"M9 11h6\" />\
         <circle cx=\"12\" cy=\"4\" r=\"2\" />\
         </svg>"
    )
}

/// Vertical fraction of the anchor sprite where the ring (rope
/// attachment point) sits. The lucide anchor's ring is a small circle
/// at viewBox y=4 with radius 2, so the top of the ring is at y=2 in
/// the 24-unit viewBox. The rope canvas hooks its bottom endpoint to
/// this fraction of the sprite's display height so the rope visually
/// reaches the top of the ring, regardless of how the renderer sizes
/// the sprite.
pub(crate) fn anchor_svg_ring_top_fraction() -> f32 {
    2.0 / 24.0
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
    /// The boat must STRUCTURALLY transform the master's group stroke: recolor
    /// it to the visualizer border, inject a stroke-opacity, scale the group
    /// stroke-width down, strip every per-path rigging width, and drop
    /// paint-order. We assert the structure rather than color identity because
    /// under the default Svalbard theme `border_color` equals the master's
    /// outline `#111817` in both modes — a color-equality check would pass on a
    /// dead recolor. Holds `TEST_THEME_LOCK` so no concurrent test flips the
    /// mode between the `viz` read and the boat build.
    #[test]
    fn themed_boat_svg_recolors_and_scales_group_stroke() {
        let _guard = crate::theme::TEST_THEME_LOCK.lock();
        let viz = crate::theme::get_visualizer_colors();
        let logo = themed_logo_svg();
        let boat = themed_boat_svg(0.0, false, false);

        // The static logo carries no stroke-opacity; the boat injects exactly
        // one (on the group) from the active visualizer border opacity.
        assert_eq!(
            logo.matches("stroke-opacity=").count(),
            0,
            "static logo must not carry stroke-opacity"
        );
        assert_eq!(
            boat.matches(&format!("stroke-opacity=\"{}\"", viz.border_opacity))
                .count(),
            1,
            "boat must inject exactly one themed stroke-opacity on the group"
        );
        assert!(
            boat.contains(&format!("stroke=\"{}\"", viz.border_color)),
            "group stroke must be recolored to the visualizer border_color \
             (expected stroke=\"{}\", got {boat:?})",
            viz.border_color
        );
        // The authored group width 60 and ALL per-path widths are gone, leaving
        // exactly one scaled group stroke-width that every path inherits.
        let scaled = format!(
            "stroke-width=\"{}\"",
            LOGO_VIEWBOX[2] * BOAT_GROUP_STROKE_FRACTION
        );
        assert!(
            boat.contains(&scaled),
            "group stroke must be scaled to {scaled} (got {boat:?})"
        );
        assert_eq!(
            boat.matches("stroke-width=").count(),
            1,
            "exactly one (scaled group) stroke-width must remain; per-path \
             rigging widths must be stripped"
        );
        assert!(
            !boat.contains("stroke-width=\"60\""),
            "the authored group stroke-width must be gone"
        );
        assert!(
            boat.contains("paint-order:stroke markers fill"),
            "boat must KEEP paint-order so the group stroke paints behind the \
             fill, hiding the interior seams of the compound hull/shield subpaths"
        );
    }

    /// A non-zero rotation must inject a `rotate(...)` SVG transform around the
    /// viewBox center, derived from `LOGO_VIEWBOX` (not a hardcoded literal).
    #[test]
    fn themed_boat_svg_bakes_rotation_around_viewbox_center() {
        let out = themed_boat_svg(0.1, false, false);
        assert!(
            out.contains("<g transform=\""),
            "rotated boat must wrap content in a <g transform=...>"
        );
        assert!(
            out.contains("rotate("),
            "non-zero angle must use rotate(...) transform"
        );
        let [min_x, min_y, w, h] = LOGO_VIEWBOX;
        let pivot = format!("{} {}", min_x + w / 2.0, min_y + h / 2.0);
        assert!(
            out.contains(&pivot),
            "rotation pivot must be the viewBox center ({pivot})"
        );
        assert!(
            out.ends_with("</g></svg>\n") || out.ends_with("</g></svg>"),
            "wrapping group must close before the </svg>"
        );
    }

    /// `mirrored = true` with zero rotation must produce just the mirror
    /// transform (no rotate), with the translate derived from the viewBox.
    #[test]
    fn themed_boat_svg_mirror_only_omits_rotate() {
        let out = themed_boat_svg(0.0, true, false);
        let tx = 2.0 * (LOGO_VIEWBOX[0] + LOGO_VIEWBOX[2] / 2.0);
        assert!(
            out.contains(&format!("translate({tx} 0) scale(-1 1)")),
            "mirrored boat must apply the viewBox-derived horizontal-flip"
        );
        assert!(
            !out.contains("rotate("),
            "zero rotation must not emit rotate(...)"
        );
    }

    /// `vertical_flip = true` with zero rotation and no horizontal mirror must
    /// produce just the vertical-flip transform, derived from the viewBox.
    #[test]
    fn themed_boat_svg_vertical_flip_only_emits_y_reflection() {
        let out = themed_boat_svg(0.0, false, true);
        let ty = 2.0 * (LOGO_VIEWBOX[1] + LOGO_VIEWBOX[3] / 2.0);
        assert!(
            out.contains(&format!("translate(0 {ty}) scale(1 -1)")),
            "vertically flipped boat must apply the y-reflection transform"
        );
        assert!(
            !out.contains("rotate("),
            "zero rotation must not emit rotate(...)"
        );
        assert!(
            !out.contains("scale(-1 1)"),
            "no horizontal mirror requested — must not emit the x-flip"
        );
    }

    /// All three transforms requested together must compose in the
    /// vertical-flip → rotate → horizontal-mirror order (leftmost applied last
    /// to a point's coords). Locks the geometry stack for the inverted + tilted
    /// + leftward-sailing boat.
    #[test]
    fn themed_boat_svg_all_three_transforms_compose_in_order() {
        let out = themed_boat_svg(0.1, true, true);
        let g = out
            .find("<g transform=\"")
            .expect("transform group must exist when any transform is requested");
        let close = out[g..]
            .find('"')
            .and_then(|i| out[g + i + 1..].find('"').map(|j| g + i + 1 + j))
            .expect("transform attribute must close");
        let transform = &out[g + "<g transform=\"".len()..close];
        let ty = 2.0 * (LOGO_VIEWBOX[1] + LOGO_VIEWBOX[3] / 2.0);
        let tx = 2.0 * (LOGO_VIEWBOX[0] + LOGO_VIEWBOX[2] / 2.0);
        let v_idx = transform
            .find(&format!("translate(0 {ty}) scale(1 -1)"))
            .expect("vertical flip must be present");
        let r_idx = transform.find("rotate(").expect("rotation must be present");
        let m_idx = transform
            .find(&format!("translate({tx} 0) scale(-1 1)"))
            .expect("horizontal mirror must be present");
        assert!(
            v_idx < r_idx && r_idx < m_idx,
            "transform order must be vertical-flip, then rotate, then \
             horizontal-mirror (got {transform:?})"
        );
    }

    /// Distinct boolean combinations must produce distinct SVG bytes — that's
    /// what makes the `(tilt, facing<0, inverted)` cache key in
    /// `BoatState::cache_handle_for` actually discriminate the four
    /// orientations, and the primary guard against a boat frozen upright in
    /// release. A shared-output bug would have two cache entries silently
    /// rendering the same boat.
    #[test]
    fn themed_boat_svg_distinct_bytes_per_flip_combination() {
        let upright = themed_boat_svg(0.0, false, false);
        let mirrored = themed_boat_svg(0.0, true, false);
        let inverted = themed_boat_svg(0.0, false, true);
        let both = themed_boat_svg(0.0, true, true);
        assert_ne!(upright, mirrored);
        assert_ne!(upright, inverted);
        assert_ne!(upright, both);
        assert_ne!(mirrored, inverted);
        assert_ne!(mirrored, both);
        assert_ne!(inverted, both);
    }

    /// True only when an SVG string still carries an uppercase hex digit in a
    /// `#rrggbb` literal. The master is all-lowercase (the generator lowercases
    /// it) and `color_to_hex` emits lowercase, so a survivor means a
    /// case-sensitive `.replace()` silently missed half a role.
    fn has_uppercase_hex(svg: &str) -> bool {
        svg.match_indices('#').any(|(i, _)| {
            svg[i + 1..]
                .chars()
                .take(6)
                .any(|c| c.is_ascii_hexdigit() && c.is_ascii_uppercase())
        })
    }

    /// Under the default theme every role hex equals its sentinel, so a content
    /// check can't tell a real remap from a no-op. Flip light/dark — where at
    /// least `fg0` moves off its sentinel — and assert in BOTH modes that each
    /// role maps to its accessor at its path count, the outline stays fixed, the
    /// output tracks the theme, and no uppercase hex survives. Serialized via
    /// `TEST_THEME_LOCK` (pokes the light-mode atomic).
    #[test]
    fn themed_logo_svg_remaps_roles_on_theme_flip() {
        use crate::theme;
        let _guard = theme::TEST_THEME_LOCK.lock();
        let was_light = theme::is_light_mode();

        theme::set_light_mode(false);
        let dark = themed_logo_svg();
        let dark_roles = (
            color_to_hex(theme::fg0()),
            color_to_hex(theme::accent()),
            color_to_hex(theme::warning()),
        );

        theme::set_light_mode(true);
        let light = themed_logo_svg();
        let light_roles = (
            color_to_hex(theme::fg0()),
            color_to_hex(theme::accent()),
            color_to_hex(theme::warning()),
        );

        theme::set_light_mode(was_light); // restore before any assertion fires

        // Output tracks the active theme (catches a total no-op / identity bug).
        assert_ne!(
            dark, light,
            "themed logo must change with the active palette"
        );

        // Each role maps to its accessor at its path count, in BOTH modes.
        for (out, fg0, accent, warning) in [
            (&dark, &dark_roles.0, &dark_roles.1, &dark_roles.2),
            (&light, &light_roles.0, &light_roles.1, &light_roles.2),
        ] {
            assert_eq!(
                out.matches(fg0.as_str()).count(),
                2,
                "body (sail+hull) → fg0 ×2"
            );
            assert_eq!(
                out.matches(accent.as_str()).count(),
                3,
                "shields → accent ×3"
            );
            assert_eq!(
                out.matches(warning.as_str()).count(),
                2,
                "wood → warning ×2"
            );
            assert_eq!(
                out.matches(LOGO_TOKEN_OUTLINE).count(),
                1,
                "outline must stay the fixed near-black"
            );
            assert!(!has_uppercase_hex(out), "no uppercase hex may survive");
        }
    }

    /// `LOGO_VIEWBOX` must track the master's declared viewBox — all boat
    /// geometry derives from it, so a silent drift would mis-size or freeze the
    /// boat.
    #[test]
    fn logo_viewbox_const_matches_master_svg() {
        let vb = LOGO_SVG
            .split("viewBox=\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .expect("master must declare a viewBox");
        let nums: Vec<f32> = vb
            .split_whitespace()
            .filter_map(|n| n.parse().ok())
            .collect();
        assert_eq!(nums.len(), 4, "viewBox must have 4 numbers (got {vb:?})");
        for (got, want) in nums.iter().zip(LOGO_VIEWBOX.iter()) {
            assert!(
                (got - want).abs() < 1e-3,
                "LOGO_VIEWBOX drifted from the master: {nums:?} vs {LOGO_VIEWBOX:?}"
            );
        }
    }

    /// The master must KEEP the artwork's per-path stroke widths (group + 6
    /// paths = 7 occurrences). The boat strips them at runtime; the static /
    /// About / icon uses need them to match the authored rigging. Guards against
    /// a regen that flattens them to a single heavy uniform outline.
    #[test]
    fn logo_master_keeps_per_path_stroke_widths() {
        assert_eq!(
            LOGO_SVG.matches("stroke-width").count(),
            7,
            "master must keep the group + 6 per-path stroke widths"
        );
    }

    /// The themed anchor SVG must carry the active theme's border color
    /// as the stroke on every anchor path. Same accessor as
    /// `themed_boat_svg`, so a theme change picks both up uniformly.
    #[test]
    fn themed_anchor_svg_uses_theme_stroke() {
        let _guard = crate::theme::TEST_THEME_LOCK.lock();
        let viz = crate::theme::get_visualizer_colors();
        let out = themed_anchor_svg();
        assert!(
            out.contains(&format!("stroke=\"{}\"", viz.border_color)),
            "anchor stroke must come from the active theme's border_color \
             (expected {}, got: {out})",
            viz.border_color
        );
        assert!(
            out.contains(&format!("stroke-opacity=\"{}\"", viz.border_opacity)),
            "anchor must inherit the theme's border opacity"
        );
    }

    /// The themed anchor SVG must include the four lucide-anchor sub-paths
    /// (vertical shaft, curved hook, cross-bar, ring). Proves the lucide
    /// anchor body was inlined fully and not truncated.
    #[test]
    fn themed_anchor_svg_includes_all_lucide_paths() {
        let out = themed_anchor_svg();
        assert!(
            out.contains("M12 6v16"),
            "anchor shaft path must be present"
        );
        assert!(
            out.contains("a9 9 0 0 1-18 0"),
            "anchor curved-hook path must be present"
        );
        assert!(out.contains("M9 11h6"), "anchor cross-bar must be present");
        assert!(
            out.contains("<circle"),
            "anchor top ring (circle) must be present"
        );
    }

    /// The renderer hooks the rope's bottom endpoint to the top of the
    /// anchor's ring, derived from `anchor_svg_ring_top_fraction()`. The
    /// fraction must place the rope at the top of the ring (y=2 in the
    /// 24-unit viewBox) — neither at the ring's center nor at the top
    /// of the viewBox.
    #[test]
    fn anchor_svg_ring_top_fraction_lands_at_ring_top() {
        let f = anchor_svg_ring_top_fraction();
        // Within the 24-unit viewBox, y=2 (top of the ring) corresponds
        // to a fraction of 2/24 ≈ 0.0833. y=4 (ring center) would be
        // 0.166; y=0 (top of viewBox) would be 0.0.
        assert!(
            (f - 2.0 / 24.0).abs() < 1e-6,
            "ring-top fraction must be 2/24 (top of the ring circle), \
             got {f}"
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
