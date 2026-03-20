//! Embedded SVG Icons
//!
//! All SVG icons are embedded at compile time using include_str!
//! This makes the binary portable and eliminates the need for external asset files.

use iced::widget::svg;
use tracing::warn;

/// Get the SVG content for a given icon path
///
/// This function maps the old asset paths to embedded SVG strings.
/// Usage: `embedded_svg("assets/icons/play.svg")` returns the SVG content.
pub(crate) fn get_svg(path: &str) -> &'static str {
    match path {
        "assets/icons/arrow-down.svg" => ARROW_DOWN,
        "assets/icons/arrow-down-to-line.svg" => ARROW_DOWN_TO_LINE,
        "assets/icons/arrow-up.svg" => ARROW_UP,
        "assets/icons/arrow-up-to-line.svg" => ARROW_UP_TO_LINE,
        "assets/icons/audio-lines.svg" => AUDIO_LINES,
        "assets/icons/audio-waveform.svg" => AUDIO_WAVEFORM,
        "assets/icons/chevron-down.svg" => CHEVRON_DOWN,
        "assets/icons/chevron-up.svg" => CHEVRON_UP,
        "assets/icons/check.svg" => CHECK,
        "assets/icons/circle.svg" => CIRCLE,
        "assets/icons/circle-play.svg" => CIRCLE_PLAY,
        "assets/icons/chevron-left.svg" => CHEVRON_LEFT,
        "assets/icons/chevron-right.svg" => CHEVRON_RIGHT,
        "assets/icons/heart-filled.svg" => HEART_FILLED,
        "assets/icons/heart.svg" => HEART,
        "assets/icons/info.svg" => INFO,
        "assets/icons/cookie.svg" => COOKIE,
        "assets/icons/ellipsis-vertical.svg" => ELLIPSIS_VERTICAL,
        "assets/icons/list-end.svg" => LIST_END,
        "assets/icons/list-minus.svg" => LIST_MINUS,
        "assets/icons/list-music.svg" => LIST_MUSIC,
        "assets/icons/list-plus.svg" => LIST_PLUS,
        "assets/icons/list.svg" => LIST,
        "assets/icons/menu.svg" => MENU,
        "assets/icons/mic.svg" => MIC,
        "assets/icons/music.svg" => MUSIC,
        "assets/icons/pause.svg" => PAUSE,
        "assets/icons/pencil.svg" => PENCIL,
        "assets/icons/pencil-line.svg" => PENCIL_LINE,
        "assets/icons/play.svg" => PLAY,
        "assets/icons/repeat-1.svg" => REPEAT_1,
        "assets/icons/repeat-2.svg" => REPEAT_2,
        "assets/icons/save.svg" => SAVE,
        "assets/icons/search.svg" => SEARCH,
        "assets/icons/settings.svg" => SETTINGS,
        "assets/icons/shuffle.svg" => SHUFFLE,
        "assets/icons/skip-back.svg" => SKIP_BACK,
        "assets/icons/skip-forward.svg" => SKIP_FORWARD,
        "assets/icons/star-filled.svg" => STAR_FILLED,
        "assets/icons/star.svg" => STAR,
        "assets/icons/stop.svg" => STOP,
        "assets/icons/trash-2.svg" => TRASH_2,
        "assets/icons/sun-moon.svg" => SUN_MOON,
        "assets/icons/sliders-horizontal.svg" => SLIDERS_HORIZONTAL,
        "assets/icons/palette.svg" => PALETTE,
        "assets/icons/cog.svg" => COG,
        "assets/icons/keyboard.svg" => KEYBOARD,
        "assets/icons/compass.svg" => COMPASS,
        "assets/icons/disc-3.svg" => DISC_3,
        "assets/icons/unfold-vertical.svg" => UNFOLD_VERTICAL,
        "assets/icons/library-big.svg" => LIBRARY_BIG,
        "assets/icons/list-filter.svg" => LIST_FILTER,
        "assets/icons/rotate-ccw.svg" => ROTATE_CCW,
        "assets/icons/globe.svg" => GLOBE,
        "assets/icons/monitor.svg" => MONITOR,
        "assets/icons/radio-tower.svg" => RADIO_TOWER,
        "assets/icons/database.svg" => DATABASE,
        "assets/icons/log-out.svg" => LOG_OUT,
        "assets/icons/hard-drive.svg" => HARD_DRIVE,
        "assets/icons/user-round.svg" => USER_ROUND,
        "assets/icons/refresh-cw.svg" => REFRESH_CW,
        "assets/icons/type.svg" => TYPE,
        "assets/icons/mouse-pointer.svg" => MOUSE_POINTER,
        "assets/icons/x.svg" => X,
        "assets/icons/copy.svg" => COPY,
        "assets/icons/folder-open.svg" => FOLDER_OPEN,
        "assets/icons/panel-right-open.svg" => PANEL_RIGHT_OPEN,
        "assets/icons/panels-top-left.svg" => PANELS_TOP_LEFT,
        "assets/icons/swatch-book.svg" => SWATCH_BOOK,
        "assets/icons/tags.svg" => TAGS,
        _ => {
            warn!("  Unknown SVG path: {}", path);
            PLAY // Fallback to play icon
        }
    }
}

/// Create an SVG widget from an embedded icon path
///
/// This is a drop-in replacement for `svg(Handle::from_path(path))`
/// that uses embedded SVG data instead.
pub(crate) fn svg_widget<'a>(path: &str) -> svg::Svg<'a> {
    let svg_content = get_svg(path);
    let handle = svg::Handle::from_memory(svg_content.as_bytes());
    svg(handle)
}

// Embed all SVG files at compile time
const ARROW_DOWN: &str = include_str!("../assets/icons/arrow-down.svg");
const ARROW_DOWN_TO_LINE: &str = include_str!("../assets/icons/arrow-down-to-line.svg");
const ARROW_UP: &str = include_str!("../assets/icons/arrow-up.svg");
const ARROW_UP_TO_LINE: &str = include_str!("../assets/icons/arrow-up-to-line.svg");
const AUDIO_LINES: &str = include_str!("../assets/icons/audio-lines.svg");
const AUDIO_WAVEFORM: &str = include_str!("../assets/icons/audio-waveform.svg");
const CHEVRON_DOWN: &str = include_str!("../assets/icons/chevron-down.svg");
const CHEVRON_UP: &str = include_str!("../assets/icons/chevron-up.svg");
const CHECK: &str = include_str!("../assets/icons/check.svg");
const CIRCLE: &str = include_str!("../assets/icons/circle.svg");
const CIRCLE_PLAY: &str = include_str!("../assets/icons/circle-play.svg");
const CHEVRON_LEFT: &str = include_str!("../assets/icons/chevron-left.svg");
const CHEVRON_RIGHT: &str = include_str!("../assets/icons/chevron-right.svg");
const HEART_FILLED: &str = include_str!("../assets/icons/heart-filled.svg");
const HEART: &str = include_str!("../assets/icons/heart.svg");
const INFO: &str = include_str!("../assets/icons/info.svg");
const COOKIE: &str = include_str!("../assets/icons/cookie.svg");
const ELLIPSIS_VERTICAL: &str = include_str!("../assets/icons/ellipsis-vertical.svg");
const LIST_END: &str = include_str!("../assets/icons/list-end.svg");
const LIST_MINUS: &str = include_str!("../assets/icons/list-minus.svg");
const LIST_MUSIC: &str = include_str!("../assets/icons/list-music.svg");
const LIST_PLUS: &str = include_str!("../assets/icons/list-plus.svg");
const LIST: &str = include_str!("../assets/icons/list.svg");
const MENU: &str = include_str!("../assets/icons/menu.svg");
const MIC: &str = include_str!("../assets/icons/mic.svg");
const MUSIC: &str = include_str!("../assets/icons/music.svg");
const PAUSE: &str = include_str!("../assets/icons/pause.svg");
const PENCIL: &str = include_str!("../assets/icons/pencil.svg");
const PENCIL_LINE: &str = include_str!("../assets/icons/pencil-line.svg");
const PLAY: &str = include_str!("../assets/icons/play.svg");
const REPEAT_1: &str = include_str!("../assets/icons/repeat-1.svg");
const REPEAT_2: &str = include_str!("../assets/icons/repeat-2.svg");
const SAVE: &str = include_str!("../assets/icons/save.svg");
const SEARCH: &str = include_str!("../assets/icons/search.svg");
const SETTINGS: &str = include_str!("../assets/icons/settings.svg");
const SHUFFLE: &str = include_str!("../assets/icons/shuffle.svg");
const SKIP_BACK: &str = include_str!("../assets/icons/skip-back.svg");
const SKIP_FORWARD: &str = include_str!("../assets/icons/skip-forward.svg");
const STAR_FILLED: &str = include_str!("../assets/icons/star-filled.svg");
const STAR: &str = include_str!("../assets/icons/star.svg");
const STOP: &str = include_str!("../assets/icons/stop.svg");
const TRASH_2: &str = include_str!("../assets/icons/trash-2.svg");
const SUN_MOON: &str = include_str!("../assets/icons/sun-moon.svg");
const SLIDERS_HORIZONTAL: &str = include_str!("../assets/icons/sliders-horizontal.svg");
const PALETTE: &str = include_str!("../assets/icons/palette.svg");
const COG: &str = include_str!("../assets/icons/cog.svg");
const KEYBOARD: &str = include_str!("../assets/icons/keyboard.svg");
const COMPASS: &str = include_str!("../assets/icons/compass.svg");
const DISC_3: &str = include_str!("../assets/icons/disc-3.svg");
const UNFOLD_VERTICAL: &str = include_str!("../assets/icons/unfold-vertical.svg");
const LIBRARY_BIG: &str = include_str!("../assets/icons/library-big.svg");
const LIST_FILTER: &str = include_str!("../assets/icons/list-filter.svg");
const ROTATE_CCW: &str = include_str!("../assets/icons/rotate-ccw.svg");
const GLOBE: &str = include_str!("../assets/icons/globe.svg");
const MONITOR: &str = include_str!("../assets/icons/monitor.svg");
const RADIO_TOWER: &str = include_str!("../assets/icons/radio-tower.svg");
const DATABASE: &str = include_str!("../assets/icons/database.svg");
const LOG_OUT: &str = include_str!("../assets/icons/log-out.svg");
const HARD_DRIVE: &str = include_str!("../assets/icons/hard-drive.svg");
const USER_ROUND: &str = include_str!("../assets/icons/user-round.svg");
const REFRESH_CW: &str = include_str!("../assets/icons/refresh-cw.svg");
const TYPE: &str = include_str!("../assets/icons/type.svg");
const SWATCH_BOOK: &str = include_str!("../assets/icons/swatch-book.svg");
const MOUSE_POINTER: &str = include_str!("../assets/icons/mouse-pointer.svg");
const X: &str = include_str!("../assets/icons/x.svg");
const COPY: &str = include_str!("../assets/icons/copy.svg");
const FOLDER_OPEN: &str = include_str!("../assets/icons/folder-open.svg");
const PANEL_RIGHT_OPEN: &str = include_str!("../assets/icons/panel-right-open.svg");
const PANELS_TOP_LEFT: &str = include_str!("../assets/icons/panels-top-left.svg");
const TAGS: &str = include_str!("../assets/icons/tags.svg");

/// Check whether a given SVG path is registered in the embedded icon table.
/// Returns `false` for paths that would hit the fallback arm.
#[cfg(test)]
fn is_registered(path: &str) -> bool {
    // We can't introspect the match arms, so duplicate the known set.
    // The test below ensures this list stays in sync with `get_svg()`.
    const KNOWN: &[&str] = &[
        "assets/icons/arrow-down.svg",
        "assets/icons/arrow-down-to-line.svg",
        "assets/icons/arrow-up.svg",
        "assets/icons/arrow-up-to-line.svg",
        "assets/icons/audio-lines.svg",
        "assets/icons/audio-waveform.svg",
        "assets/icons/chevron-down.svg",
        "assets/icons/chevron-up.svg",
        "assets/icons/check.svg",
        "assets/icons/circle.svg",
        "assets/icons/circle-play.svg",
        "assets/icons/chevron-left.svg",
        "assets/icons/chevron-right.svg",
        "assets/icons/heart-filled.svg",
        "assets/icons/heart.svg",
        "assets/icons/info.svg",
        "assets/icons/cookie.svg",
        "assets/icons/ellipsis-vertical.svg",
        "assets/icons/list-end.svg",
        "assets/icons/list-minus.svg",
        "assets/icons/list-music.svg",
        "assets/icons/list-plus.svg",
        "assets/icons/list.svg",
        "assets/icons/menu.svg",
        "assets/icons/mic.svg",
        "assets/icons/music.svg",
        "assets/icons/pause.svg",
        "assets/icons/pencil.svg",
        "assets/icons/pencil-line.svg",
        "assets/icons/play.svg",
        "assets/icons/repeat-1.svg",
        "assets/icons/repeat-2.svg",
        "assets/icons/save.svg",
        "assets/icons/search.svg",
        "assets/icons/settings.svg",
        "assets/icons/shuffle.svg",
        "assets/icons/skip-back.svg",
        "assets/icons/skip-forward.svg",
        "assets/icons/star-filled.svg",
        "assets/icons/star.svg",
        "assets/icons/stop.svg",
        "assets/icons/trash-2.svg",
        "assets/icons/sun-moon.svg",
        "assets/icons/sliders-horizontal.svg",
        "assets/icons/palette.svg",
        "assets/icons/cog.svg",
        "assets/icons/keyboard.svg",
        "assets/icons/compass.svg",
        "assets/icons/disc-3.svg",
        "assets/icons/unfold-vertical.svg",
        "assets/icons/library-big.svg",
        "assets/icons/list-filter.svg",
        "assets/icons/rotate-ccw.svg",
        "assets/icons/globe.svg",
        "assets/icons/monitor.svg",
        "assets/icons/radio-tower.svg",
        "assets/icons/database.svg",
        "assets/icons/log-out.svg",
        "assets/icons/hard-drive.svg",
        "assets/icons/user-round.svg",
        "assets/icons/refresh-cw.svg",
        "assets/icons/type.svg",
        "assets/icons/mouse-pointer.svg",
        "assets/icons/x.svg",
        "assets/icons/copy.svg",
        "assets/icons/folder-open.svg",
        "assets/icons/panel-right-open.svg",
        "assets/icons/panels-top-left.svg",
        "assets/icons/swatch-book.svg",
        "assets/icons/tags.svg",
    ];
    KNOWN.contains(&path)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    /// Recursively collect all `.rs` files under a directory, skipping `embedded_svg.rs`.
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
    /// and verify every one is registered in `get_svg()`.
    ///
    /// This prevents the silent play-icon fallback from shipping unnoticed
    /// when a new icon is added to a view but not registered here.
    #[test]
    fn all_svg_paths_in_source_are_registered() {
        let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut rs_files = Vec::new();
        collect_rs_files(&src_dir, &mut rs_files);

        let mut unregistered: BTreeSet<String> = BTreeSet::new();
        for file in &rs_files {
            let contents = std::fs::read_to_string(file).unwrap();
            for path in extract_svg_paths(&contents) {
                if !is_registered(&path) {
                    unregistered.insert(path);
                }
            }
        }

        assert!(
            unregistered.is_empty(),
            "SVG paths used in source but not registered in embedded_svg.rs:\n{}",
            unregistered
                .iter()
                .map(|p| format!("  - {p}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    /// Verify that every path in the KNOWN list also appears in `get_svg()`.
    /// If someone adds a path to KNOWN but forgets the match arm, the
    /// fallback (PLAY) will be returned and this test will catch it.
    #[test]
    fn known_list_matches_get_svg() {
        let known = [
            "assets/icons/arrow-down.svg",
            "assets/icons/arrow-down-to-line.svg",
            "assets/icons/arrow-up.svg",
            "assets/icons/arrow-up-to-line.svg",
            "assets/icons/audio-lines.svg",
            "assets/icons/audio-waveform.svg",
            "assets/icons/chevron-down.svg",
            "assets/icons/chevron-up.svg",
            "assets/icons/check.svg",
            "assets/icons/circle.svg",
            "assets/icons/circle-play.svg",
            "assets/icons/chevron-left.svg",
            "assets/icons/chevron-right.svg",
            "assets/icons/heart-filled.svg",
            "assets/icons/heart.svg",
            "assets/icons/info.svg",
            "assets/icons/cookie.svg",
            "assets/icons/ellipsis-vertical.svg",
            "assets/icons/list-end.svg",
            "assets/icons/list-minus.svg",
            "assets/icons/list-music.svg",
            "assets/icons/list-plus.svg",
            "assets/icons/list.svg",
            "assets/icons/menu.svg",
            "assets/icons/mic.svg",
            "assets/icons/music.svg",
            "assets/icons/pause.svg",
            "assets/icons/pencil.svg",
            "assets/icons/pencil-line.svg",
            "assets/icons/play.svg",
            "assets/icons/repeat-1.svg",
            "assets/icons/repeat-2.svg",
            "assets/icons/save.svg",
            "assets/icons/search.svg",
            "assets/icons/settings.svg",
            "assets/icons/shuffle.svg",
            "assets/icons/skip-back.svg",
            "assets/icons/skip-forward.svg",
            "assets/icons/star-filled.svg",
            "assets/icons/star.svg",
            "assets/icons/stop.svg",
            "assets/icons/trash-2.svg",
            "assets/icons/sun-moon.svg",
            "assets/icons/sliders-horizontal.svg",
            "assets/icons/palette.svg",
            "assets/icons/cog.svg",
            "assets/icons/keyboard.svg",
            "assets/icons/compass.svg",
            "assets/icons/disc-3.svg",
            "assets/icons/unfold-vertical.svg",
            "assets/icons/library-big.svg",
            "assets/icons/list-filter.svg",
            "assets/icons/rotate-ccw.svg",
            "assets/icons/globe.svg",
            "assets/icons/monitor.svg",
            "assets/icons/radio-tower.svg",
            "assets/icons/database.svg",
            "assets/icons/log-out.svg",
            "assets/icons/hard-drive.svg",
            "assets/icons/user-round.svg",
            "assets/icons/refresh-cw.svg",
            "assets/icons/type.svg",
            "assets/icons/mouse-pointer.svg",
            "assets/icons/x.svg",
            "assets/icons/copy.svg",
            "assets/icons/folder-open.svg",
            "assets/icons/panel-right-open.svg",
            "assets/icons/panels-top-left.svg",
            "assets/icons/swatch-book.svg",
            "assets/icons/tags.svg",
        ];

        for path in &known {
            let result = get_svg(path);
            // play.svg is the only path that should legitimately return PLAY
            if *path != "assets/icons/play.svg" {
                assert_ne!(
                    result as *const str, PLAY as *const str,
                    "Path '{path}' is in KNOWN list but get_svg() returned PLAY fallback — \
                     add it to the match table!"
                );
            }
        }
    }
}
