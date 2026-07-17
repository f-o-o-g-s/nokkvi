//! Embedded SVG Icons
//!
//! All SVG icons are embedded at compile time. The lookup table is generated
//! by `build.rs` from the contents of `assets/icons/`, so adding/removing an
//! icon is a one-step change (drop or remove the file in the directory).
//!
//! See `build.rs::generate_embedded_svg_table` for the generator.

use iced::{Color, widget::svg};
use nokkvi_data::types::player_settings::IconSet;
use tracing::warn;

mod generated {
    include!(concat!(env!("OUT_DIR"), "/embedded_svg_generated.rs"));
}

/// Lucide-stem → Phosphor file mapping for the alternate icon set.
///
/// Keys are the Lucide icon stems every view references (the file name under
/// `assets/icons/` without the `.svg`); values are the full path of the
/// hand-picked Phosphor equivalent under `assets/icons-phosphor/`. Almost all
/// use the Phosphor **Regular** weight (Light read too thin next to Lucide's
/// 2px stroke); the filled transport + rating glyphs (`play`, `pause`,
/// `skip-back`, `skip-forward`, `heart-filled`, `star-filled`) deliberately
/// resolve to the **Fill** weight so the play button and rating stars still
/// read solid.
///
/// Generated from `local/phosphor-investigation/iconmap.py` and pinned by the
/// `icon_name_map_*` tests below: every key must be a real Lucide icon on disk
/// and every value must be an embedded Phosphor file. The list is kept sorted
/// by key so a `binary_search_by_key` lookup stays valid.
const NAME_MAP: &[(&str, &str)] = &[
    ("anchor", "assets/icons-phosphor/anchor-regular.svg"),
    ("arrow-down", "assets/icons-phosphor/arrow-down-regular.svg"),
    (
        "arrow-down-to-line",
        "assets/icons-phosphor/arrow-line-down-regular.svg",
    ),
    (
        "arrow-down-up",
        "assets/icons-phosphor/arrows-down-up-regular.svg",
    ),
    ("arrow-up", "assets/icons-phosphor/arrow-up-regular.svg"),
    (
        "arrow-up-to-line",
        "assets/icons-phosphor/arrow-line-up-regular.svg",
    ),
    ("audio-lines", "assets/icons-phosphor/waveform-regular.svg"),
    (
        "audio-waveform",
        "assets/icons-phosphor/wave-sine-regular.svg",
    ),
    ("binary", "assets/icons-phosphor/binary-regular.svg"),
    ("blend", "assets/icons-phosphor/intersect-regular.svg"),
    ("calendar", "assets/icons-phosphor/calendar-regular.svg"),
    ("check", "assets/icons-phosphor/check-regular.svg"),
    (
        "chevron-down",
        "assets/icons-phosphor/caret-down-regular.svg",
    ),
    (
        "chevron-left",
        "assets/icons-phosphor/caret-left-regular.svg",
    ),
    (
        "chevron-right",
        "assets/icons-phosphor/caret-right-regular.svg",
    ),
    ("chevron-up", "assets/icons-phosphor/caret-up-regular.svg"),
    ("circle", "assets/icons-phosphor/circle-regular.svg"),
    (
        "circle-play",
        "assets/icons-phosphor/play-circle-regular.svg",
    ),
    ("clock", "assets/icons-phosphor/clock-regular.svg"),
    ("cog", "assets/icons-phosphor/gear-six-regular.svg"),
    (
        "columns-3-cog",
        "assets/icons-phosphor/sliders-horizontal-regular.svg",
    ),
    ("combine", "assets/icons-phosphor/arrows-merge-regular.svg"),
    ("compass", "assets/icons-phosphor/compass-regular.svg"),
    ("cookie", "assets/icons-phosphor/cookie-regular.svg"),
    ("copy", "assets/icons-phosphor/copy-regular.svg"),
    ("database", "assets/icons-phosphor/database-regular.svg"),
    ("disc-3", "assets/icons-phosphor/disc-regular.svg"),
    (
        "ellipsis-vertical",
        "assets/icons-phosphor/dots-three-vertical-regular.svg",
    ),
    ("file-music", "assets/icons-phosphor/file-audio-regular.svg"),
    (
        "folder-open",
        "assets/icons-phosphor/folder-open-regular.svg",
    ),
    ("globe", "assets/icons-phosphor/globe-regular.svg"),
    ("hard-drive", "assets/icons-phosphor/hard-drive-regular.svg"),
    ("heart", "assets/icons-phosphor/heart-regular.svg"),
    ("heart-filled", "assets/icons-phosphor/heart-fill.svg"),
    ("info", "assets/icons-phosphor/info-regular.svg"),
    ("keyboard", "assets/icons-phosphor/keyboard-regular.svg"),
    ("layout-grid", "assets/icons-phosphor/grid-four-regular.svg"),
    ("library", "assets/icons-phosphor/books-regular.svg"),
    ("library-big", "assets/icons-phosphor/books-regular.svg"),
    ("list", "assets/icons-phosphor/list-regular.svg"),
    ("list-end", "assets/icons-phosphor/queue-regular.svg"),
    ("list-filter", "assets/icons-phosphor/funnel-regular.svg"),
    (
        "list-minus",
        "assets/icons-phosphor/stack-minus-regular.svg",
    ),
    ("list-music", "assets/icons-phosphor/playlist-regular.svg"),
    ("list-plus", "assets/icons-phosphor/list-plus-regular.svg"),
    (
        "list-tree",
        "assets/icons-phosphor/tree-structure-regular.svg",
    ),
    // Lucide `list-video` (list rows + play arrow) mirrors Phosphor's queue
    // glyph, so it shares the same Phosphor target as `music-4`/`list-end`.
    ("list-video", "assets/icons-phosphor/queue-regular.svg"),
    ("locate", "assets/icons-phosphor/crosshair-regular.svg"),
    ("lock", "assets/icons-phosphor/lock-regular.svg"),
    ("lock-open", "assets/icons-phosphor/lock-open-regular.svg"),
    ("log-out", "assets/icons-phosphor/sign-out-regular.svg"),
    ("menu", "assets/icons-phosphor/list-regular.svg"),
    ("mic", "assets/icons-phosphor/microphone-regular.svg"),
    ("monitor", "assets/icons-phosphor/monitor-regular.svg"),
    ("mouse-pointer", "assets/icons-phosphor/cursor-regular.svg"),
    ("music", "assets/icons-phosphor/music-note-regular.svg"),
    ("music-2", "assets/icons-phosphor/music-note-regular.svg"),
    ("music-4", "assets/icons-phosphor/queue-regular.svg"),
    ("palette", "assets/icons-phosphor/palette-regular.svg"),
    (
        "panel-right-open",
        "assets/icons-phosphor/sidebar-regular.svg",
    ),
    (
        "panels-top-left",
        "assets/icons-phosphor/layout-regular.svg",
    ),
    ("pause", "assets/icons-phosphor/pause-fill.svg"),
    ("pencil", "assets/icons-phosphor/pencil-simple-regular.svg"),
    (
        "pencil-line",
        "assets/icons-phosphor/pencil-simple-line-regular.svg",
    ),
    ("pin", "assets/icons-phosphor/push-pin-regular.svg"),
    ("play", "assets/icons-phosphor/play-fill.svg"),
    ("plus", "assets/icons-phosphor/plus-regular.svg"),
    ("radar", "assets/icons-phosphor/target-regular.svg"),
    (
        "radio-tower",
        "assets/icons-phosphor/cell-tower-regular.svg",
    ),
    (
        "refresh-cw",
        "assets/icons-phosphor/arrows-clockwise-regular.svg",
    ),
    ("repeat-1", "assets/icons-phosphor/repeat-once-regular.svg"),
    ("repeat-2", "assets/icons-phosphor/repeat-regular.svg"),
    (
        "rotate-ccw",
        "assets/icons-phosphor/arrow-counter-clockwise-regular.svg",
    ),
    ("save", "assets/icons-phosphor/floppy-disk-regular.svg"),
    (
        "search",
        "assets/icons-phosphor/magnifying-glass-regular.svg",
    ),
    ("settings", "assets/icons-phosphor/gear-regular.svg"),
    ("shuffle", "assets/icons-phosphor/shuffle-regular.svg"),
    ("skip-back", "assets/icons-phosphor/skip-back-fill.svg"),
    (
        "skip-forward",
        "assets/icons-phosphor/skip-forward-fill.svg",
    ),
    (
        "sliders-horizontal",
        "assets/icons-phosphor/faders-horizontal-regular.svg",
    ),
    (
        "sliders-vertical",
        "assets/icons-phosphor/faders-regular.svg",
    ),
    ("sparkles", "assets/icons-phosphor/sparkle-regular.svg"),
    ("star", "assets/icons-phosphor/star-regular.svg"),
    ("star-filled", "assets/icons-phosphor/star-fill.svg"),
    ("sun-moon", "assets/icons-phosphor/circle-half-regular.svg"),
    ("swatch-book", "assets/icons-phosphor/swatches-regular.svg"),
    ("tags", "assets/icons-phosphor/tag-regular.svg"),
    ("trash-2", "assets/icons-phosphor/trash-regular.svg"),
    ("type", "assets/icons-phosphor/text-aa-regular.svg"),
    (
        "unfold-vertical",
        "assets/icons-phosphor/arrows-out-line-vertical-regular.svg",
    ),
    ("user-round", "assets/icons-phosphor/user-regular.svg"),
    ("x", "assets/icons-phosphor/x-regular.svg"),
];

/// Resolve a Lucide icon path to its Phosphor equivalent path, if one exists.
/// Takes the full `assets/icons/<stem>.svg` path and returns the mapped
/// `assets/icons-phosphor/<file>.svg` path. `None` for paths outside the
/// Lucide namespace or stems without a mapping.
fn phosphor_path(lucide_path: &str) -> Option<&'static str> {
    let stem = lucide_path
        .strip_prefix("assets/icons/")?
        .strip_suffix(".svg")?;
    NAME_MAP
        .binary_search_by_key(&stem, |(k, _)| k)
        .ok()
        .map(|i| NAME_MAP[i].1)
}

/// Get the SVG content for a given icon path.
///
/// When the active icon set is Phosphor (the default), a Lucide path is first
/// remapped to its Phosphor equivalent ([`phosphor_path`]); a mapped-but-missing
/// Phosphor file falls through to the Lucide content (graceful, not the play.svg
/// fallback). Selecting the Lucide set skips the remap (one atomic load) and
/// uses the direct lookup.
///
/// Returns `play.svg` as the fallback when the path is unregistered. The
/// fallback path is the silent failure mode that the test
/// `all_svg_paths_in_source_are_registered` exists to catch.
pub(crate) fn get_svg(path: &str) -> &'static str {
    if crate::theme::icon_set() == IconSet::Phosphor
        && let Some(ph) = phosphor_path(path)
        && let Some(content) = generated::lookup(ph)
    {
        return content;
    }
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

/// The owner's circular smiley avatar — the harbour trawl scene's moon.
/// Normalized to the LOGO master's token scheme: its two semantic roles
/// (pale face fill, dark line work) carry the SAME Svalbard-default
/// sentinels as the ship — [`LOGO_TOKEN_BODY`] and [`LOGO_TOKEN_OUTLINE`]
/// — so the untouched asset renders correctly as the default theme and no
/// new color literal exists anywhere in the pipeline.
const MOON_FACE_SVG: &str = include_str!("../assets/moon_face.svg");

/// Return the moon-face avatar themed for the harbour scene. Follows the
/// boat OVERLAY's convention (not the standalone logo's): the scene's
/// doodads all key on the mode-stable dark visualizer palette — the face
/// fill takes the PEAK color (the scene's starlight, so the moon shares the
/// stars' light) and the line work takes the border ink, exactly the color
/// `themed_boat_svg` strokes the hull with. Handle caching lives on
/// `BoatState` beside the boat and anchor handles, sharing their
/// theme-generation invalidation.
pub(crate) fn themed_moon_face_svg() -> String {
    let viz = crate::theme::get_visualizer_colors_dark();
    let fill = viz
        .peak_gradient_colors
        .first()
        .cloned()
        .unwrap_or_else(|| LOGO_TOKEN_BODY.to_string());
    MOON_FACE_SVG
        .replace(LOGO_TOKEN_BODY, &fill)
        .replace(LOGO_TOKEN_OUTLINE, &viz.border_color)
}

/// The moon face's four fadeable marks, in choreography order. Each id
/// names a `<g>` wrapper in the master asset whose `opacity="1"` the
/// veil rewrite retargets — the master must carry each anchor EXACTLY
/// once (test-pinned), and the rewrite replaces the full `id + opacity`
/// attribute pair so it can never touch any other opacity in the
/// document. The face disc itself is never veiled.
pub(crate) const MOON_VEIL_IDS: [&str; 4] = ["veil-smile", "veil-eye", "veil-patch", "veil-strap"];

/// Veil alpha quantization: each mark's opacity is keyed in 1/32 steps
/// (further scaled by the scene's 0.60 moon alpha at composite time, so
/// one step lands well under a JND). Quantizing is what keeps the
/// per-key handle cache on `BoatState` bounded — an unquantized alpha
/// would mint a new SVG document (and a full usvg parse + raster)
/// every frame.
pub(crate) const MOON_VEIL_STEPS: u8 = 32;

/// Every mark fully present — the dream's mid-ritual peak, when the
/// face is whole. `themed_moon_face_veiled` returns the untouched
/// themed master for this key (byte-identical to
/// `themed_moon_face_svg`, test-pinned). Production code never names
/// this key — the quantizer produces it as a value during the hold —
/// so it exists as shared test vocabulary.
#[cfg(test)]
pub(crate) const MOON_VEIL_OPAQUE: [u8; 4] = [MOON_VEIL_STEPS; 4];

/// The RESTING veil — the bare disc. The moon (and the day sun; same
/// asset) sails faceless between dreams: every mark's group carries
/// zero opacity, leaving only the disc and its rim. This is the key
/// every ordinary frame renders, and the one `BoatState`'s plain moon
/// handle bakes.
pub(crate) const MOON_VEIL_BARE: [u8; 4] = [0; 4];

/// Return the themed moon face with per-mark opacity applied — the
/// harbour scene's moon-dream rewrite. `veil` carries one quantized
/// alpha per [`MOON_VEIL_IDS`] entry; marks at full opacity keep their
/// authored `opacity="1"` anchor untouched. Group opacity is isolated
/// in SVG (the group composites offscreen, once), so a half-faded mark
/// emerges from the face disc rather than double-exposing against it —
/// the reason this rewrite exists instead of stacked per-mark Svg
/// layers, whose compositing provably lightens the settled ink.
pub(crate) fn themed_moon_face_veiled(veil: [u8; 4]) -> String {
    let mut out = themed_moon_face_svg();
    for (id, q) in MOON_VEIL_IDS.iter().zip(veil) {
        if q >= MOON_VEIL_STEPS {
            continue;
        }
        let alpha = f32::from(q) / f32::from(MOON_VEIL_STEPS);
        out = out.replace(
            &format!("id=\"{id}\" opacity=\"1\""),
            &format!("id=\"{id}\" opacity=\"{alpha:.4}\""),
        );
    }
    out
}

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
/// body (sail + hull) → `logo_body()`, shields (the three blocks) →
/// `logo_shields()`, wood (mast + yard) → `logo_wood()`.
///
/// Those accessors read each theme's **dark** palette regardless of the active
/// light/dark mode, so the mark is mode-stable: it still recolors when you
/// switch themes, but the light/dark toggle no longer flips it. Tracking light
/// mode used to invert the body to dark ink, turning the longship into an
/// unreadable blob on a light background.
///
/// The near-black group outline (`LOGO_TOKEN_OUTLINE`) is deliberately left
/// fixed so the mark keeps its shape definition on any background. The boat
/// overlay overrides that stroke separately in `themed_boat_svg()`.
pub(crate) fn themed_logo_svg() -> String {
    use crate::theme;

    LOGO_SVG
        .replace(LOGO_TOKEN_BODY, &color_to_hex(theme::logo_body()))
        .replace(LOGO_TOKEN_SHIELDS, &color_to_hex(theme::logo_shields()))
        .replace(LOGO_TOKEN_WOOD, &color_to_hex(theme::logo_wood()))
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
/// - The master's authored `#111817` group stroke is recolored to the **dark**
///   visualizer `border_color`/`border_opacity` (mode-stable, so the outline
///   stays solid in light mode instead of fading with the wave), its width
///   rescaled to `w · BOAT_GROUP_STROKE_FRACTION`, and
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
    let viz = crate::theme::get_visualizer_colors_dark();
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

/// Return the themed anchor body as standalone SVG, no rope. Follows the
/// active icon set: the Lucide stroked anchor by default-art, or the Phosphor
/// filled anchor when the Phosphor set is active.
///
/// The rope is rendered separately as a curved canvas path in
/// `widgets/boat.rs` so it can sway naturally with the wave action; this
/// helper provides only the static anchor sprite that hangs at the
/// rope's bottom end.
///
/// Either way the anchor uses the active visualizer theme's `border_color` /
/// `border_opacity`, same source as the lines-mode wave outline and the boat
/// outline, so every part of the doodad (boat, rope, anchor) shares one
/// palette — the Lucide anchor *strokes* with it (open path), the Phosphor
/// anchor *fills* with it (solid glyph). Cache invalidation lives on
/// `BoatState`, keyed against `theme_generation()`; a light/dark flip, palette
/// swap, or icon-set change ([`set_icon_set`](crate::theme::set_icon_set) bumps
/// the generation) rebuilds the handle on the next render. The boat renders the
/// handle with no color override, so the baked color is what shows.
pub(crate) fn themed_anchor_svg() -> String {
    let viz = crate::theme::get_visualizer_colors_dark();
    let stroke = viz.border_color;
    let opacity = viz.border_opacity;

    if crate::theme::icon_set() == IconSet::Phosphor {
        // The Phosphor anchor is a filled glyph with no open stroke to recolor,
        // so theme it by replacing its `currentColor` fill with the visualizer
        // border color + opacity. `get_svg` remaps the Lucide path to the
        // Phosphor anchor file (we are already in the Phosphor branch).
        return get_svg("assets/icons/anchor.svg").replace(
            "fill=\"currentColor\"",
            &format!("fill=\"{stroke}\" fill-opacity=\"{opacity}\""),
        );
    }

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

/// Vertical fraction of the anchor sprite where the ring (rope attachment
/// point) sits — follows the active icon set, since the two anchors put their
/// eye at different heights in different-sized viewBoxes. The rope canvas hooks
/// its bottom endpoint to this fraction of the sprite's display height so it
/// reaches the top of the ring regardless of how the renderer sizes the sprite.
///
/// - Lucide: the ring is a circle at viewBox y=4 radius 2 in a 24-unit box, so
///   its top is at y=2 → `2/24`.
/// - Phosphor: the eye is a radius-16 ring centered at (128, 56) in the
///   256-unit box, so its top edge is at y=40 → `40/256`.
pub(crate) fn anchor_svg_ring_top_fraction() -> f32 {
    if crate::theme::icon_set() == IconSet::Phosphor {
        40.0 / 256.0
    } else {
        2.0 / 24.0
    }
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

    /// Read every `.svg` stem-path under one `assets/<dir>/` namespace as
    /// full relative paths (e.g. `assets/icons/play.svg`).
    fn on_disk_svgs(rel_dir: &str) -> Vec<String> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel_dir);
        std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("read {rel_dir}: {e}"))
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                if path.extension().is_some_and(|e| e == "svg") {
                    let name = path.file_name()?.to_str()?.to_owned();
                    Some(format!("{rel_dir}/{name}"))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Verify the generator's KNOWN_PATHS list matches the on-disk contents of
    /// BOTH icon namespaces (`assets/icons/` + `assets/icons-phosphor/`). Cheap
    /// re-confirmation that build-time codegen observed every file the test
    /// sees at runtime.
    #[test]
    fn generated_paths_match_assets_dir() {
        let mut on_disk: Vec<String> = on_disk_svgs("assets/icons");
        on_disk.extend(on_disk_svgs("assets/icons-phosphor"));
        on_disk.sort();

        let mut known: Vec<String> = generated::KNOWN_PATHS
            .iter()
            .map(|s| s.to_string())
            .collect();
        known.sort();

        assert_eq!(
            on_disk, known,
            "generated KNOWN_PATHS drifted from assets/icons{{,-phosphor}}/ contents — rebuild"
        );
    }

    /// The Phosphor `NAME_MAP` must be sorted by Lucide stem so the
    /// `binary_search_by_key` in `phosphor_path` is valid.
    #[test]
    fn icon_name_map_is_sorted_by_key() {
        assert!(
            NAME_MAP.windows(2).all(|w| w[0].0 < w[1].0),
            "NAME_MAP must be strictly sorted by lucide stem (binary_search relies on it)"
        );
    }

    /// Every Lucide icon on disk must have a Phosphor mapping, so selecting the
    /// Phosphor set never silently leaves a glyph un-remapped. Catches a new
    /// `assets/icons/*.svg` landing without a corresponding `NAME_MAP` row.
    #[test]
    fn icon_name_map_covers_every_lucide_icon() {
        let mapped: BTreeSet<&str> = NAME_MAP.iter().map(|(k, _)| *k).collect();
        let mut missing: Vec<String> = Vec::new();
        for full in on_disk_svgs("assets/icons") {
            let stem = full
                .strip_prefix("assets/icons/")
                .and_then(|f| f.strip_suffix(".svg"))
                .unwrap();
            if !mapped.contains(stem) {
                missing.push(stem.to_string());
            }
        }
        assert!(
            missing.is_empty(),
            "Lucide icons with no Phosphor mapping (add a NAME_MAP row):\n{}",
            missing
                .iter()
                .map(|p| format!("  - {p}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    /// Every Phosphor target in `NAME_MAP` must be an embedded file (resolve to
    /// real content via `lookup`, not the fallback). Catches a typo'd Phosphor
    /// filename or a file that was never copied into `assets/icons-phosphor/`.
    #[test]
    fn icon_name_map_targets_all_ship() {
        let mut missing: Vec<&str> = Vec::new();
        for (_, ph) in NAME_MAP {
            if generated::lookup(ph).is_none() {
                missing.push(ph);
            }
        }
        assert!(
            missing.is_empty(),
            "NAME_MAP targets not present in assets/icons-phosphor/:\n{}",
            missing
                .iter()
                .map(|p| format!("  - {p}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    /// The filled transport + rating glyphs must resolve to the Phosphor FILL
    /// weight, so the play button and rating stars stay solid (an outline
    /// play/star would read as a regression). The rest stay on Regular.
    #[test]
    fn icon_name_map_forces_fill_weight_for_filled_glyphs() {
        for stem in [
            "play",
            "pause",
            "skip-back",
            "skip-forward",
            "heart-filled",
            "star-filled",
        ] {
            let ph = phosphor_path(&format!("assets/icons/{stem}.svg"))
                .unwrap_or_else(|| panic!("{stem} must map to a Phosphor file"));
            assert!(
                ph.ends_with("-fill.svg"),
                "{stem} must use the Phosphor Fill weight, got {ph}"
            );
        }
    }

    /// `phosphor_path` resolves a Lucide path to its mapped Phosphor path and
    /// declines paths outside the Lucide namespace.
    #[test]
    fn phosphor_path_resolves_and_rejects() {
        assert_eq!(
            phosphor_path("assets/icons/chevron-down.svg"),
            Some("assets/icons-phosphor/caret-down-regular.svg"),
        );
        // Already a Phosphor path → not remapped again.
        assert_eq!(
            phosphor_path("assets/icons-phosphor/caret-down-regular.svg"),
            None
        );
        // Unknown Lucide stem → no mapping.
        assert_eq!(phosphor_path("assets/icons/does-not-exist.svg"), None);
    }

    /// The composed `get_svg()` remap is what every rendered icon actually goes
    /// through, yet the tests above only assert the pure pieces (NAME_MAP +
    /// `phosphor_path`). Pin the branch end-to-end: under Phosphor a Lucide path
    /// returns the mapped Phosphor bytes; under Lucide the same path returns the
    /// Lucide bytes. Guards against a silent revert-to-Lucide or an inverted
    /// condition that fmt, clippy, and every other test would pass (the sibling
    /// `every_known_path_resolves_to_unique_content` only checks `!= FALLBACK`,
    /// which a wholesale revert would survive). Holds `THEME_MODE_LOCK` (the
    /// process-global theme-atomic serializer) and restores the icon set so no
    /// sibling test observes a leaked value.
    #[test]
    fn get_svg_honors_active_icon_set() {
        let _guard = crate::theme::THEME_MODE_LOCK.lock();
        let original = crate::theme::icon_set();

        let lucide_play = generated::lookup("assets/icons/play.svg").unwrap();
        let phosphor_play = generated::lookup("assets/icons-phosphor/play-fill.svg").unwrap();
        assert_ne!(
            lucide_play, phosphor_play,
            "fixture sanity: the Lucide and Phosphor play glyphs must differ"
        );

        crate::theme::set_icon_set(IconSet::Phosphor);
        assert_eq!(
            get_svg("assets/icons/play.svg"),
            phosphor_play,
            "Phosphor set must remap play -> play-fill"
        );

        crate::theme::set_icon_set(IconSet::Lucide);
        assert_eq!(
            get_svg("assets/icons/play.svg"),
            lucide_play,
            "Lucide set must return the Lucide play bytes (no remap)"
        );

        crate::theme::set_icon_set(original);
    }

    /// The boat must STRUCTURALLY transform the master's group stroke: recolor
    /// it to the **dark** visualizer border, inject a stroke-opacity, scale the
    /// group stroke-width down, strip every per-path rigging width, and KEEP
    /// paint-order (so the fill hides the compound-path seams). We assert the
    /// structure rather than color identity because under the default Svalbard
    /// theme `border_color` equals the master's outline `#111817` in both modes
    /// — a color-equality check would pass on a dead recolor. Holds
    /// `THEME_MODE_LOCK` so no concurrent test flips the mode between the `viz`
    /// read and the boat build.
    #[test]
    fn themed_boat_svg_recolors_and_scales_group_stroke() {
        let _guard = crate::theme::THEME_MODE_LOCK.lock();
        let viz = crate::theme::get_visualizer_colors_dark();
        let logo = themed_logo_svg();
        let boat = themed_boat_svg(0.0, false, false);

        // The static logo carries no stroke-opacity; the boat injects exactly
        // one (on the group) from the dark visualizer border opacity.
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

    /// The logo is mode-stable: the light/dark toggle must NOT change it (the
    /// fix for light mode inverting the body to dark ink). It still maps each
    /// role to the theme's dark-palette accessor at its path count, keeps the
    /// fixed outline, and leaves no uppercase hex. Serialized via
    /// `THEME_MODE_LOCK` (pokes the light-mode atomic).
    #[test]
    fn themed_logo_svg_is_mode_stable_and_maps_roles() {
        use crate::theme;
        let _guard = theme::THEME_MODE_LOCK.lock();
        let was_light = theme::is_light_mode();

        theme::set_light_mode(false);
        let dark = themed_logo_svg();
        theme::set_light_mode(true);
        let light = themed_logo_svg();
        theme::set_light_mode(was_light); // restore before any assertion fires

        // The light/dark toggle must not change the logo.
        assert_eq!(
            dark, light,
            "logo must be identical in light and dark mode (it reads the dark \
             palette regardless of the active mode)"
        );

        // Each role maps to its mode-stable accessor at its path count. The
        // accessors ignore mode, so reading them here is mode-independent.
        let body = color_to_hex(theme::logo_body());
        let shields = color_to_hex(theme::logo_shields());
        let wood = color_to_hex(theme::logo_wood());
        assert_eq!(
            dark.matches(&body).count(),
            2,
            "body (sail+hull) → logo_body ×2"
        );
        assert_eq!(
            dark.matches(&shields).count(),
            3,
            "shields (3 blocks) → logo_shields ×3"
        );
        assert_eq!(
            dark.matches(&wood).count(),
            2,
            "wood (mast+yard) → logo_wood ×2"
        );
        assert_eq!(
            dark.matches(LOGO_TOKEN_OUTLINE).count(),
            1,
            "outline must stay the fixed near-black"
        );
        assert!(!has_uppercase_hex(&dark), "no uppercase hex may survive");
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

    /// The Lucide anchor SVG must carry the **dark** visualizer border color
    /// and opacity as its STROKE. Same accessor as `themed_boat_svg`, so the
    /// boat and its anchor stay mode-stable together. Pins the Lucide set
    /// (Phosphor is the default and fills instead of strokes).
    #[test]
    fn themed_anchor_svg_lucide_uses_theme_stroke() {
        let _guard = crate::theme::THEME_MODE_LOCK.lock();
        let orig = crate::theme::icon_set();
        crate::theme::set_icon_set(IconSet::Lucide);
        let viz = crate::theme::get_visualizer_colors_dark();
        let out = themed_anchor_svg();
        crate::theme::set_icon_set(orig);
        assert!(
            out.contains(&format!("stroke=\"{}\"", viz.border_color)),
            "anchor stroke must come from the dark visualizer border_color \
             (expected {}, got: {out})",
            viz.border_color
        );
        assert!(
            out.contains(&format!("stroke-opacity=\"{}\"", viz.border_opacity)),
            "anchor must inherit the dark visualizer border opacity"
        );
    }

    /// The moon-face asset must ship carrying the shared LOGO tokens (the
    /// Svalbard-default sentinels), with no stray literal from the avatar
    /// template's gruvbox era — and the themed output must land on the dark
    /// visualizer palette like every other scene doodad.
    #[test]
    fn themed_moon_face_svg_uses_shared_logo_tokens() {
        let _guard = crate::theme::THEME_MODE_LOCK.lock();
        assert!(
            MOON_FACE_SVG.contains(LOGO_TOKEN_BODY) && MOON_FACE_SVG.contains(LOGO_TOKEN_OUTLINE),
            "the asset must carry the shared logo sentinels"
        );
        assert!(
            !MOON_FACE_SVG.contains("#458588") && !MOON_FACE_SVG.contains("#1d2021"),
            "no gruvbox-era literal may survive in the normalized asset"
        );
        let viz = crate::theme::get_visualizer_colors_dark();
        let out = themed_moon_face_svg();
        assert!(
            out.contains(&viz.border_color),
            "line work must use the dark visualizer border color"
        );
    }

    /// Every veil anchor must appear in the master exactly once — a
    /// re-save that normalizes `opacity="1"` into a style string (Inkscape
    /// does this) would turn the veil rewrite into a silent no-op, leaving
    /// the moon permanently complete and the dream invisible.
    #[test]
    fn moon_face_master_carries_every_veil_anchor() {
        for id in MOON_VEIL_IDS {
            let anchor = format!("id=\"{id}\" opacity=\"1\"");
            assert_eq!(
                MOON_FACE_SVG.matches(&anchor).count(),
                1,
                "the master must carry `{anchor}` exactly once — hand-edit \
                 the XML, don't re-save through Inkscape"
            );
        }
    }

    /// The fully-opaque veil is the untouched themed master, byte for
    /// byte — the guarantee that the dream's mid-ritual peak shows
    /// exactly the authored face, nothing resampled.
    #[test]
    fn veiled_moon_at_full_opacity_is_the_untouched_master() {
        let _guard = crate::theme::THEME_MODE_LOCK.lock();
        assert_eq!(
            themed_moon_face_veiled(MOON_VEIL_OPAQUE),
            themed_moon_face_svg()
        );
    }

    /// The resting (bare) veil hides every mark — the disc the moon
    /// wears between dreams carries no trace of the face.
    #[test]
    fn veiled_moon_bare_hides_every_mark() {
        let _guard = crate::theme::THEME_MODE_LOCK.lock();
        let out = themed_moon_face_veiled(MOON_VEIL_BARE);
        for id in MOON_VEIL_IDS {
            assert!(
                out.contains(&format!("id=\"{id}\" opacity=\"0.0000\"")),
                "{id} must be fully hidden in the resting document"
            );
        }
    }

    /// Each mark's opacity rewrites independently; a full-opacity mark
    /// keeps its authored anchor untouched.
    #[test]
    fn veiled_moon_rewrites_each_mark_alone() {
        let _guard = crate::theme::THEME_MODE_LOCK.lock();
        let out = themed_moon_face_veiled([0, 16, MOON_VEIL_STEPS, 8]);
        assert!(out.contains(r#"id="veil-smile" opacity="0.0000""#));
        assert!(out.contains(r#"id="veil-eye" opacity="0.5000""#));
        assert!(out.contains(r#"id="veil-patch" opacity="1""#));
        assert!(out.contains(r#"id="veil-strap" opacity="0.2500""#));
    }

    /// The Lucide anchor SVG must include the four lucide-anchor sub-paths
    /// (vertical shaft, curved hook, cross-bar, ring). Proves the lucide
    /// anchor body was inlined fully and not truncated.
    #[test]
    fn themed_anchor_svg_lucide_includes_all_paths() {
        let _guard = crate::theme::THEME_MODE_LOCK.lock();
        let orig = crate::theme::icon_set();
        crate::theme::set_icon_set(IconSet::Lucide);
        let out = themed_anchor_svg();
        crate::theme::set_icon_set(orig);
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

    /// When the Phosphor set is active, the anchor is the Phosphor FILLED glyph
    /// themed via its `fill` (not a stroke), and carries none of the Lucide
    /// inline path data. The rope still has an eye to hook (the `M112,56` ring
    /// arc from the phosphor anchor).
    #[test]
    fn themed_anchor_svg_phosphor_fills_with_theme_color() {
        let _guard = crate::theme::THEME_MODE_LOCK.lock();
        let orig = crate::theme::icon_set();
        crate::theme::set_icon_set(IconSet::Phosphor);
        let viz = crate::theme::get_visualizer_colors_dark();
        let out = themed_anchor_svg();
        crate::theme::set_icon_set(orig);
        assert!(
            out.contains(&format!(
                "fill=\"{}\" fill-opacity=\"{}\"",
                viz.border_color, viz.border_opacity
            )),
            "phosphor anchor must fill with the dark visualizer border color + opacity (got: {out})"
        );
        assert!(
            !out.contains("fill=\"currentColor\""),
            "the currentColor fill must be replaced by the themed fill"
        );
        assert!(
            !out.contains("M12 6v16"),
            "phosphor anchor must NOT carry the Lucide inline path data"
        );
        assert!(
            out.contains("M112,56"),
            "phosphor anchor body (incl. its eye ring) must be present"
        );
    }

    /// The rope hooks the top of the anchor's ring via
    /// `anchor_svg_ring_top_fraction()`, which follows the active set: the
    /// Lucide ring top is y=2 in a 24-box (2/24); the Phosphor eye top is y=40
    /// in a 256-box (40/256). Neither is the ring center or the viewBox top.
    #[test]
    fn anchor_ring_top_fraction_is_icon_set_aware() {
        let _guard = crate::theme::THEME_MODE_LOCK.lock();
        let orig = crate::theme::icon_set();

        crate::theme::set_icon_set(IconSet::Lucide);
        let lucide_f = anchor_svg_ring_top_fraction();
        crate::theme::set_icon_set(IconSet::Phosphor);
        let phosphor_f = anchor_svg_ring_top_fraction();
        crate::theme::set_icon_set(orig);

        assert!(
            (lucide_f - 2.0 / 24.0).abs() < 1e-6,
            "Lucide ring-top fraction must be 2/24, got {lucide_f}"
        );
        assert!(
            (phosphor_f - 40.0 / 256.0).abs() < 1e-6,
            "Phosphor ring-top fraction must be 40/256, got {phosphor_f}"
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
