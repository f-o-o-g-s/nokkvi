//! Visualizer tab setting entries

use nokkvi_data::types::theme_file::{ThemeFile, VisualizerColors};

use super::items::{SettingItem, SettingsEntry};
use crate::visualizer_config::VisualizerConfig;

/// Push color entries for one mode (dark or light) into the settings list.
///
/// Deduplicates the identical dark/light color sections that previously
/// differed only in key prefix and section label.
fn push_visualizer_color_entries(
    e: &mut Vec<SettingsEntry>,
    prefix: &str,
    label: &'static str,
    icon: &'static str,
    colors: &VisualizerColors,
    defaults: &VisualizerColors,
) {
    e.push(SettingsEntry::Header { label, icon });
    e.push(SettingItem::hex_color(
        meta!(
            format!("{prefix}.visualizer.border_color"),
            "Border Color",
            label,
            "Color of bar borders and LED gaps"
        ),
        &colors.border_color,
        &defaults.border_color,
    ));
    e.push(SettingItem::float(
        meta!(
            format!("{prefix}.visualizer.border_opacity"),
            "Border Opacity",
            label,
            "Transparency of bar outlines in non-LED mode"
        ),
        colors.border_opacity as f64,
        defaults.border_opacity as f64,
        0.0,
        1.0,
        0.1,
        "",
    ));
    e.push(SettingItem::float(
        meta!(
            format!("{prefix}.visualizer.led_border_opacity"),
            "LED Border Opacity",
            label,
            "Opacity of gaps between LED segments"
        ),
        colors.led_border_opacity as f64,
        defaults.led_border_opacity as f64,
        0.0,
        1.0,
        0.1,
        "",
    ));
    e.push(SettingItem::color_array(
        meta!(
            format!("{prefix}.visualizer.bar_gradient_colors"),
            "Bar Gradient",
            label,
            "6 colors from low to high frequency"
        ),
        colors.bar_gradient_colors.clone(),
        defaults.bar_gradient_colors.clone(),
    ));
    e.push(SettingItem::color_array(
        meta!(
            format!("{prefix}.visualizer.peak_gradient_colors"),
            "Peak Gradient",
            label,
            "6 colors cycling for peak indicators"
        ),
        colors.peak_gradient_colors.clone(),
        defaults.peak_gradient_colors.clone(),
    ));
}

/// Build settings entries for the Visualizer tab from live config.
///
/// `theme` provides the current visualizer colors (from the active theme file).
/// Accepts it as a parameter rather than loading from disk, keeping this function
/// pure and testable.
#[allow(clippy::vec_init_then_push)]
pub(crate) fn build_visualizer_items(
    config: &VisualizerConfig,
    theme: &ThemeFile,
    active_stem: &str,
) -> Vec<SettingsEntry> {
    let d = VisualizerConfig::default();
    let dt =
        nokkvi_data::services::theme_loader::load_builtin_theme(active_stem).unwrap_or_default();
    const S: &str = "assets/icons/sliders-horizontal.svg";
    const B: &str = "assets/icons/audio-lines.svg";
    const P: &str = "assets/icons/palette.svg";
    const L: &str = "assets/icons/audio-waveform.svg";

    let mut e = Vec::with_capacity(40);

    // --- General section ---
    e.push(SettingsEntry::Header {
        label: "General",
        icon: S,
    });
    e.push(SettingItem::text(
        meta!(
            "__restore_visualizer",
            "⟲ Restore Defaults",
            "General",
            "Restore all visualizer settings to defaults. Preserves your color palette."
        ),
        "Press Enter",
        "Press Enter",
    ));
    e.push(SettingItem::float(
        meta!(
            "visualizer.noise_reduction",
            "Noise Reduction",
            "General",
            "0.0 = raw FFT, 1.0 = fully smoothed"
        ),
        config.noise_reduction,
        d.noise_reduction,
        0.0,
        1.0,
        0.01,
        "",
    ));
    e.push(SettingItem::bool_val(
        meta!(
            "visualizer.waves",
            "Waves Smoothing",
            "General",
            "Bars mode only — Catmull-Rom spline smoothing creates smooth rolling hills. Mutually exclusive with Monstercat"
        ),
        config.waves,
        d.waves,
    ));
    e.push(SettingItem::int(
        meta!(
            "visualizer.waves_smoothing",
            "Waves Intensity",
            "General",
            "Bars mode only — control point spacing for waves spline. Higher = smoother (fewer control points)"
        ),
        config.waves_smoothing as i64,
        d.waves_smoothing as i64,
        2,
        16,
        1,
        "",
    ));
    e.push(SettingItem::float(
        meta!(
            "visualizer.monstercat",
            "Monstercat Smoothing",
            "General",
            "Bars mode only — sharp triangular peaks with exponential falloff. Higher = wider spread. Mutually exclusive with Waves"
        ),
        config.monstercat,
        d.monstercat,
        0.0,
        10.0,
        0.1,
        "",
    ));
    e.push(SettingItem::int(
        meta!(
            "visualizer.lower_cutoff_freq",
            "Lower Cutoff Freq",
            "General",
            "Frequencies below this are hidden"
        ),
        config.lower_cutoff_freq as i64,
        d.lower_cutoff_freq as i64,
        20,
        1000,
        10,
        " Hz",
    ));
    e.push(SettingItem::int(
        meta!(
            "visualizer.higher_cutoff_freq",
            "Upper Cutoff Freq",
            "General",
            "Frequencies above this are hidden"
        ),
        config.higher_cutoff_freq as i64,
        d.higher_cutoff_freq as i64,
        1000,
        22050,
        100,
        " Hz",
    ));
    e.push(SettingItem::float(
        meta!(
            "visualizer.height_percent",
            "Visualizer Height",
            "General",
            "% of window height, 10–60%"
        ),
        config.height_percent as f64,
        d.height_percent as f64,
        0.1,
        0.60,
        0.05,
        "%",
    ));
    e.push(SettingItem::float(
        meta!(
            "visualizer.opacity",
            "Visualizer Opacity",
            "General",
            "0.0 = invisible, 1.0 = fully opaque"
        ),
        config.opacity as f64,
        d.opacity as f64,
        0.0,
        1.0,
        0.05,
        "",
    ));
    e.push(SettingItem::bool_val(
        meta!(
            "visualizer.auto_sensitivity",
            "Auto Sensitivity",
            "General",
            "Scales output to always fill full height"
        ),
        config.auto_sensitivity,
        d.auto_sensitivity,
    ));

    // --- Bars section ---
    e.push(SettingsEntry::Header {
        label: "Bars",
        icon: B,
    });
    e.push(SettingItem::int(
        meta!(
            "visualizer.bars.max_bars",
            "Max Bar Count",
            "Bars",
            "Maximum number of bars to fit in the window"
        ),
        config.bars.max_bars as i64,
        d.bars.max_bars as i64,
        16,
        2048,
        8,
        "",
    ));
    e.push(SettingItem::int(
        meta!(
            "visualizer.bars.bar_width_min",
            "Bar Width Min",
            "Bars",
            "Bar width at smallest window size"
        ),
        config.bars.bar_width_min as i64,
        d.bars.bar_width_min as i64,
        1,
        10,
        1,
        " px",
    ));
    e.push(SettingItem::int(
        meta!(
            "visualizer.bars.bar_width_max",
            "Bar Width Max",
            "Bars",
            "Bar width at largest window size"
        ),
        config.bars.bar_width_max as i64,
        d.bars.bar_width_max as i64,
        2,
        20,
        1,
        " px",
    ));
    e.push(SettingItem::int(
        meta!(
            "visualizer.bars.bar_spacing",
            "Bar Spacing",
            "Bars",
            "Gap between bars in pixels"
        ),
        config.bars.bar_spacing as i64,
        d.bars.bar_spacing as i64,
        0,
        10,
        1,
        " px",
    ));
    e.push(SettingItem::int(
        meta!(
            "visualizer.bars.border_width",
            "Border Width",
            "Bars",
            "Outline around each bar; also sets LED gap size"
        ),
        config.bars.border_width as i64,
        d.bars.border_width as i64,
        0,
        5,
        1,
        " px",
    ));

    e.push(SettingItem::bool_val(
        meta!(
            "visualizer.bars.led_bars",
            "LED Mode",
            "Bars",
            "Render bars as stacked LED segments like a VU meter"
        ),
        config.bars.led_bars,
        d.bars.led_bars,
    ));
    e.push(SettingItem::int(
        meta!(
            "visualizer.bars.led_segment_height",
            "LED Segment Height",
            "Bars",
            "Height of each LED segment in pixels"
        ),
        config.bars.led_segment_height as i64,
        d.bars.led_segment_height as i64,
        2,
        20,
        1,
        " px",
    ));

    e.push(SettingItem::enum_val(
        meta!(
            "visualizer.bars.gradient_mode",
            "Gradient Mode",
            "Bars",
            "static: height-based gradient (bottom to top)\nwave: gradient stretching (taller bars show more bottom colors)\nshimmer: bars cycle through all gradient colors as flat per-bar colors\nenergy: gradient shifts based on overall loudness\nalternate: bars alternate between first two gradient colors"
        ),
        &config.bars.gradient_mode,
        &d.bars.gradient_mode,
        vec!["static", "wave", "shimmer", "energy", "alternate"],
    ));
    e.push(SettingItem::enum_val(
        meta!(
            "visualizer.bars.gradient_orientation",
            "Gradient Orientation",
            "Bars",
            "Axis the gradient colors are mapped along (ignored by alternate mode)\nvertical: colors map bottom-to-top within each bar\nhorizontal: colors map left-to-right across bars (bass to treble)"
        ),
        &config.bars.gradient_orientation,
        &d.bars.gradient_orientation,
        vec!["vertical", "horizontal"],
    ));
    e.push(SettingItem::enum_val(
        meta!(
            "visualizer.bars.peak_gradient_mode",
            "Peak Gradient Mode",
            "Bars",
            "Color mode for peak indicators\nstatic: uses first color in peak gradient only\ncycle: time-based animation cycling through all peak colors\nheight: color based on peak height position\nmatch: uses same color as bar gradient at that height"
        ),
        &config.bars.peak_gradient_mode,
        &d.bars.peak_gradient_mode,
        vec!["static", "cycle", "height", "match"],
    ));
    e.push(SettingItem::enum_val(
        meta!(
            "visualizer.bars.peak_mode",
            "Peak Mode",
            "Bars",
            "none: peak bars disabled\nfade: hold, then fade out in place (opacity decreases)\nfall: hold, then fall at constant speed\nfall_accel: hold, then fall with gravity acceleration\nfall_fade: hold, then fall at constant speed while fading out"
        ),
        &config.bars.peak_mode,
        &d.bars.peak_mode,
        vec!["none", "fade", "fall", "fall_accel", "fall_fade"],
    ));
    e.push(SettingItem::int(
        meta!(
            "visualizer.bars.peak_hold_time",
            "Peak Hold Time",
            "Bars",
            "How long peaks stay before falling/fading"
        ),
        config.bars.peak_hold_time as i64,
        d.bars.peak_hold_time as i64,
        0,
        5000,
        50,
        " ms",
    ));
    e.push(SettingItem::int(
        meta!(
            "visualizer.bars.peak_fade_time",
            "Peak Fade Time",
            "Bars",
            "Duration of fade-out in 'fade' mode"
        ),
        config.bars.peak_fade_time as i64,
        d.bars.peak_fade_time as i64,
        0,
        5000,
        50,
        " ms",
    ));
    e.push(SettingItem::int(
        meta!(
            "visualizer.bars.peak_fall_speed",
            "Peak Fall Speed",
            "Bars",
            "How fast peaks drop in fall/fall_accel modes. 1 = slow, 20 = fast. No effect in fade mode"
        ),
        config.bars.peak_fall_speed as i64,
        d.bars.peak_fall_speed as i64,
        1,
        20,
        1,
        "",
    ));
    e.push(SettingItem::int(
        meta!(
            "visualizer.bars.peak_height_ratio",
            "Peak Height",
            "Bars",
            "Peak bar size as % of bar width (ignored in LED mode — peaks are one segment tall)"
        ),
        config.bars.peak_height_ratio as i64,
        d.bars.peak_height_ratio as i64,
        10,
        100,
        5,
        "%",
    ));
    e.push(SettingItem::int(
        meta!(
            "visualizer.bars.bar_depth_3d",
            "Isometric Depth",
            "Bars",
            "3D top and side face depth in pixels, 0 = flat"
        ),
        config.bars.bar_depth_3d as i64,
        d.bars.bar_depth_3d as i64,
        0,
        20,
        1,
        " px",
    ));

    // --- Bar Colors (Dark / Light) ---
    // These keys are theme-file-relative — they write to the active theme file,
    // not config.toml. The handler routes them via update_theme_value().
    push_visualizer_color_entries(
        &mut e,
        "dark",
        "Bar Colors (Dark)",
        P,
        &theme.dark.visualizer,
        &dt.dark.visualizer,
    );
    push_visualizer_color_entries(
        &mut e,
        "light",
        "Bar Colors (Light)",
        P,
        &theme.light.visualizer,
        &dt.light.visualizer,
    );

    // --- Lines section ---
    e.push(SettingsEntry::Header {
        label: "Lines",
        icon: L,
    });
    e.push(SettingItem::int(
        meta!(
            "visualizer.lines.point_count",
            "Point Count",
            "Lines",
            "8–512, more = finer detail"
        ),
        config.lines.point_count as i64,
        d.lines.point_count as i64,
        8,
        512,
        8,
        "",
    ));
    e.push(SettingItem::float(
        meta!(
            "visualizer.lines.line_thickness",
            "Line Thickness",
            "Lines",
            "% of visualizer height, 1–10%"
        ),
        config.lines.line_thickness as f64,
        d.lines.line_thickness as f64,
        0.01,
        0.10,
        0.01,
        "%",
    ));
    e.push(SettingItem::float(
        meta!(
            "visualizer.lines.outline_thickness",
            "Outline Thickness",
            "Lines",
            "Border behind the line in pixels, 0 = disabled"
        ),
        config.lines.outline_thickness as f64,
        d.lines.outline_thickness as f64,
        0.0,
        5.0,
        0.5,
        " px",
    ));
    e.push(SettingItem::float(
        meta!(
            "visualizer.lines.outline_opacity",
            "Outline Opacity",
            "Lines",
            "0.0 = invisible, 1.0 = fully opaque"
        ),
        config.lines.outline_opacity as f64,
        d.lines.outline_opacity as f64,
        0.0,
        1.0,
        0.1,
        "",
    ));
    e.push(SettingItem::float(
        meta!(
            "visualizer.lines.animation_speed",
            "Animation Speed",
            "Lines",
            "Color cycling speed. Lower = slower, higher = faster"
        ),
        config.lines.animation_speed as f64,
        d.lines.animation_speed as f64,
        0.05,
        1.0,
        0.05,
        "",
    ));
    e.push(SettingItem::enum_val(
        meta!(
            "visualizer.lines.gradient_mode",
            "Gradient Mode",
            "Lines",
            "breathing: time-based cycling through gradient palette\nstatic: uses first gradient color only\nposition: color by horizontal position (bass → treble rainbow)\nheight: color by amplitude (quiet → loud)\ngradient: position + amplitude blend (peaks shift palette)"
        ),
        &config.lines.gradient_mode,
        &d.lines.gradient_mode,
        vec!["breathing", "static", "position", "height", "gradient"],
    ));
    e.push(SettingItem::float(
        meta!(
            "visualizer.lines.fill_opacity",
            "Fill Opacity",
            "Lines",
            "Fills under the curve with a gradient. 0 = disabled"
        ),
        config.lines.fill_opacity as f64,
        d.lines.fill_opacity as f64,
        0.0,
        1.0,
        0.05,
        "",
    ));
    e.push(SettingItem::bool_val(
        meta!(
            "visualizer.lines.mirror",
            "Mirror",
            "Lines",
            "Symmetric oscilloscope — line extends from center"
        ),
        config.lines.mirror,
        d.lines.mirror,
    ));
    e.push(SettingItem::enum_val(
        meta!(
            "visualizer.lines.style",
            "Line Style",
            "Lines",
            "Interpolation between data points\nsmooth: Catmull-Rom spline (curvy)\nangular: straight line segments"
        ),
        &config.lines.style,
        &d.lines.style,
        vec!["smooth", "angular"],
    ));
    e.push(SettingItem::bool_val(
        meta!(
            "visualizer.lines.boat",
            "Surfing boat",
            "Lines",
            "Show a small boat that rides the waveform"
        ),
        config.lines.boat,
        d.lines.boat,
    ));

    e
}
