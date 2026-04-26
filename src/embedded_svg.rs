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
