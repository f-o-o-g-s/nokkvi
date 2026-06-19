//! Visualizer tab setting entries

use nokkvi_data::types::theme_file::{ThemeFile, VisualizerColors};

use super::{
    items::{SettingItem, SettingMeta, SettingsEntry},
    sentinel::SentinelKind,
};
use crate::visualizer_config::{
    BarsGradientMode, BarsGradientOrientation, BarsPeakGradientMode, BarsPeakMode,
    LinesGradientMode, LinesStyle, VisualizerConfig, VisualizerPlacement, keys,
};

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
    e.push(
        SettingItem::hex_color(
            SettingMeta::new(
                format!("{prefix}.visualizer.border_color"),
                "Border Color",
                label,
            )
            .with_subtitle("Color of bar borders and LED gaps"),
            &colors.border_color,
            &defaults.border_color,
        )
        .with_theme_key(),
    );
    e.push(
        SettingItem::float(
            SettingMeta::new(
                format!("{prefix}.visualizer.border_opacity"),
                "Border Opacity",
                label,
            )
            .with_subtitle("Transparency of bar outlines in non-LED mode"),
            colors.border_opacity as f64,
            defaults.border_opacity as f64,
            0.0,
            1.0,
            0.1,
            "",
        )
        .with_theme_key(),
    );
    e.push(
        SettingItem::float(
            SettingMeta::new(
                format!("{prefix}.visualizer.led_border_opacity"),
                "LED Border Opacity",
                label,
            )
            .with_subtitle("Opacity of gaps between LED segments"),
            colors.led_border_opacity as f64,
            defaults.led_border_opacity as f64,
            0.0,
            1.0,
            0.1,
            "",
        )
        .with_theme_key(),
    );
    e.push(
        SettingItem::color_array(
            SettingMeta::new(
                format!("{prefix}.visualizer.bar_gradient_colors"),
                "Bar Gradient",
                label,
            )
            .with_subtitle("6 colors from low to high frequency"),
            colors.bar_gradient_colors.clone(),
            defaults.bar_gradient_colors.clone(),
        )
        .with_theme_key(),
    );
    e.push(
        SettingItem::color_array(
            SettingMeta::new(
                format!("{prefix}.visualizer.peak_gradient_colors"),
                "Peak Gradient",
                label,
            )
            .with_subtitle("6 colors cycling for peak indicators"),
            colors.peak_gradient_colors.clone(),
            defaults.peak_gradient_colors.clone(),
        )
        .with_theme_key(),
    );
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
    const F: &str = "assets/icons/layout-grid.svg";
    const S: &str = "assets/icons/sliders-horizontal.svg";
    const B: &str = "assets/icons/audio-lines.svg";
    const P: &str = "assets/icons/palette.svg";
    const L: &str = "assets/icons/audio-waveform.svg";
    const SC: &str = "assets/icons/radar.svg";

    let mut e = Vec::with_capacity(75);

    // --- Frame section (how the visualizer occupies the window) ---
    e.push(SettingsEntry::Header {
        label: "Frame",
        icon: F,
    });
    e.push(SettingItem::text(
        SettingMeta::new(
            SentinelKind::RestoreVisualizer.to_key(),
            "⟲ Restore Defaults",
            "Frame",
        )
        .with_subtitle(
            "Restore all visualizer settings to defaults. Preserves your color palette.",
        ),
        "Press Enter",
        "Press Enter",
    ));
    e.push(SettingItem::float(
        SettingMeta::new(keys::HEIGHT_PERCENT, "Visualizer Height", "Frame").with_subtitle(
            "Bars/Lines height: % of the bottom band's window height, or of the cover when placed over it. 10–60%",
        ),
        config.height_percent as f64,
        d.height_percent as f64,
        0.1,
        0.60,
        0.05,
        "%",
    ));
    e.push(SettingItem::float(
        SettingMeta::new(keys::OPACITY, "Visualizer Opacity", "Frame")
            .with_subtitle("0.0 = invisible, 1.0 = fully opaque"),
        config.opacity as f64,
        d.opacity as f64,
        0.0,
        1.0,
        0.05,
        "",
    ));
    e.push(SettingItem::bool_val(
        SettingMeta::new(keys::BLOOM, "Bloom Glow", "Frame")
            .with_subtitle("Soft glow halo around bright bars, peak flashes, and the line"),
        config.bloom,
        d.bloom,
    ));
    e.push(SettingItem::float(
        SettingMeta::new(keys::BLOOM_INTENSITY, "Bloom Intensity", "Frame")
            .with_subtitle("Strength of the bloom glow. 0 = off"),
        config.bloom_intensity as f64,
        d.bloom_intensity as f64,
        0.0,
        1.0,
        0.05,
        "",
    ));
    e.push(SettingItem::float(
        SettingMeta::new(keys::BEAT_REACTIVITY, "Beat Reactivity", "Frame")
            .with_subtitle(
                "Pulses the bloom, glow, and bars with the beat + bass drops.\n1 = punches on every kick, 0 = steady (tracks loudness only)",
            ),
        config.beat_reactivity as f64,
        d.beat_reactivity as f64,
        0.0,
        1.0,
        0.05,
        "",
    ));
    e.push(SettingItem::float(
        SettingMeta::new(keys::CRT, "CRT / Film", "Frame").with_subtitle(
            "Retro post-process: chromatic aberration, scanlines, vignette, grain, beat zoom. 0 = off",
        ),
        config.crt as f64,
        d.crt as f64,
        0.0,
        1.0,
        0.05,
        "",
    ));

    // --- Signal section (FFT/DSP, affects both Bars and Lines modes) ---
    e.push(SettingsEntry::Header {
        label: "Signal",
        icon: S,
    });
    e.push(SettingItem::float(
        SettingMeta::new(keys::NOISE_REDUCTION, "Noise Reduction", "Signal")
            .with_subtitle("0.0 = raw FFT, 1.0 = fully smoothed"),
        config.noise_reduction,
        d.noise_reduction,
        0.0,
        1.0,
        0.01,
        "",
    ));
    e.push(SettingItem::int(
        SettingMeta::new(keys::LOWER_CUTOFF_FREQ, "Lower Cutoff Freq", "Signal")
            .with_subtitle("Frequencies below this are hidden"),
        config.lower_cutoff_freq as i64,
        d.lower_cutoff_freq as i64,
        20,
        1000,
        10,
        " Hz",
    ));
    e.push(SettingItem::int(
        SettingMeta::new(keys::HIGHER_CUTOFF_FREQ, "Upper Cutoff Freq", "Signal")
            .with_subtitle("Frequencies above this are hidden"),
        config.higher_cutoff_freq as i64,
        d.higher_cutoff_freq as i64,
        1000,
        22050,
        100,
        " Hz",
    ));
    e.push(SettingItem::bool_val(
        SettingMeta::new(keys::AUTO_SENSITIVITY, "Auto Sensitivity", "Signal")
            .with_subtitle("Scales output to always fill full height"),
        config.auto_sensitivity,
        d.auto_sensitivity,
    ));

    // --- Bars section ---
    e.push(SettingsEntry::Header {
        label: "Bars",
        icon: B,
    });
    e.push(SettingItem::enum_val(
        SettingMeta::new(keys::BARS_PLACEMENT, "Placement", "Bars").with_subtitle(
            "Where the Bars visualizer is drawn\nbottom_band: a band above the player bar (every view)\nover_cover: on the now-playing cover art (Queue view, while playing)",
        ),
        config.bars.placement.as_wire_str(),
        d.bars.placement.as_wire_str(),
        VisualizerPlacement::all_wire_strs(),
    ));
    e.push(SettingItem::bool_val(
        SettingMeta::new(keys::WAVES, "Waves Smoothing", "Bars")
            .with_subtitle(
                "Catmull-Rom spline smoothing creates smooth rolling hills. Mutually exclusive with Monstercat",
            ),
        config.waves,
        d.waves,
    ));
    e.push(SettingItem::int(
        SettingMeta::new(keys::WAVES_SMOOTHING, "Waves Intensity", "Bars").with_subtitle(
            "Control point spacing for waves spline. Higher = smoother (fewer control points)",
        ),
        config.waves_smoothing as i64,
        d.waves_smoothing as i64,
        2,
        16,
        1,
        "",
    ));
    e.push(SettingItem::float(
        SettingMeta::new(keys::MONSTERCAT, "Monstercat Smoothing", "Bars")
            .with_subtitle(
                "Sharp triangular peaks with exponential falloff. Higher = wider spread. Mutually exclusive with Waves",
            ),
        config.monstercat,
        d.monstercat,
        0.0,
        10.0,
        0.1,
        "",
    ));
    e.push(SettingItem::int(
        SettingMeta::new(keys::BARS_MAX_BARS, "Max Bar Count", "Bars")
            .with_subtitle("Maximum number of bars to fit in the window"),
        config.bars.max_bars as i64,
        d.bars.max_bars as i64,
        16,
        2048,
        8,
        "",
    ));
    e.push(SettingItem::int(
        SettingMeta::new(keys::BARS_BAR_WIDTH_MIN, "Bar Width Min", "Bars")
            .with_subtitle("Bar width at smallest window size"),
        config.bars.bar_width_min as i64,
        d.bars.bar_width_min as i64,
        1,
        10,
        1,
        " px",
    ));
    e.push(SettingItem::int(
        SettingMeta::new(keys::BARS_BAR_WIDTH_MAX, "Bar Width Max", "Bars")
            .with_subtitle("Bar width at largest window size"),
        config.bars.bar_width_max as i64,
        d.bars.bar_width_max as i64,
        2,
        20,
        1,
        " px",
    ));
    e.push(SettingItem::int(
        SettingMeta::new(keys::BARS_BAR_SPACING, "Bar Spacing", "Bars")
            .with_subtitle("Gap between bars in pixels"),
        config.bars.bar_spacing as i64,
        d.bars.bar_spacing as i64,
        0,
        10,
        1,
        " px",
    ));
    e.push(SettingItem::int(
        SettingMeta::new(keys::BARS_BORDER_WIDTH, "Border Width", "Bars")
            .with_subtitle("Outline around each bar; also sets LED gap size"),
        config.bars.border_width as i64,
        d.bars.border_width as i64,
        0,
        5,
        1,
        " px",
    ));

    e.push(SettingItem::bool_val(
        SettingMeta::new(keys::BARS_LED_BARS, "LED Mode", "Bars")
            .with_subtitle("Render bars as stacked LED segments like a VU meter"),
        config.bars.led_bars,
        d.bars.led_bars,
    ));
    e.push(SettingItem::int(
        SettingMeta::new(keys::BARS_LED_SEGMENT_HEIGHT, "LED Segment Height", "Bars")
            .with_subtitle("Height of each LED segment in pixels"),
        config.bars.led_segment_height as i64,
        d.bars.led_segment_height as i64,
        2,
        20,
        1,
        " px",
    ));

    e.push(SettingItem::enum_val(
        SettingMeta::new(keys::BARS_GRADIENT_MODE, "Gradient Mode", "Bars")
            .with_subtitle(
                "static: height-based gradient (bottom to top)\nwave: gradient stretching (taller bars show more bottom colors)",
            ),
        config.bars.gradient_mode.as_wire_str(),
        d.bars.gradient_mode.as_wire_str(),
        BarsGradientMode::all_wire_strs(),
    ));
    e.push(SettingItem::enum_val(
        SettingMeta::new(
            keys::BARS_GRADIENT_ORIENTATION,
            "Gradient Orientation",
            "Bars",
        )
        .with_subtitle(
            "Axis the gradient colors are mapped along\nvertical: colors map bottom-to-top within each bar\nhorizontal: colors map left-to-right across bars (bass to treble)",
        ),
        config.bars.gradient_orientation.as_wire_str(),
        d.bars.gradient_orientation.as_wire_str(),
        BarsGradientOrientation::all_wire_strs(),
    ));
    e.push(SettingItem::enum_val(
        SettingMeta::new(
            keys::BARS_PEAK_GRADIENT_MODE,
            "Peak Gradient Mode",
            "Bars",
        )
        .with_subtitle(
            "Color mode for peak indicators\nstatic: uses first color in peak gradient only\ncycle: time-based animation cycling through all peak colors\nheight: color based on peak height position\nmatch: uses same color as bar gradient at that height",
        ),
        config.bars.peak_gradient_mode.as_wire_str(),
        d.bars.peak_gradient_mode.as_wire_str(),
        BarsPeakGradientMode::all_wire_strs(),
    ));
    e.push(SettingItem::enum_val(
        SettingMeta::new(keys::BARS_PEAK_MODE, "Peak Mode", "Bars")
            .with_subtitle(
                "none: peak bars disabled\nfade: hold, then fade out in place (opacity decreases)\nfall: hold, then fall at constant speed\nfall_accel: hold, then fall with gravity acceleration\nfall_fade: hold, then fall at constant speed while fading out",
            ),
        config.bars.peak_mode.as_wire_str(),
        d.bars.peak_mode.as_wire_str(),
        BarsPeakMode::all_wire_strs(),
    ));
    e.push(SettingItem::int(
        SettingMeta::new(keys::BARS_PEAK_HOLD_TIME, "Peak Hold Time", "Bars")
            .with_subtitle("How long peaks stay before falling/fading"),
        config.bars.peak_hold_time as i64,
        d.bars.peak_hold_time as i64,
        0,
        5000,
        50,
        " ms",
    ));
    e.push(SettingItem::int(
        SettingMeta::new(keys::BARS_PEAK_FADE_TIME, "Peak Fade Time", "Bars")
            .with_subtitle("Duration of fade-out in 'fade' mode"),
        config.bars.peak_fade_time as i64,
        d.bars.peak_fade_time as i64,
        0,
        5000,
        50,
        " ms",
    ));
    e.push(SettingItem::int(
        SettingMeta::new(keys::BARS_PEAK_FALL_SPEED, "Peak Fall Speed", "Bars")
            .with_subtitle(
                "How fast peaks drop in fall/fall_accel modes. 1 = slow, 20 = fast. No effect in fade mode",
            ),
        config.bars.peak_fall_speed as i64,
        d.bars.peak_fall_speed as i64,
        1,
        20,
        1,
        "",
    ));
    e.push(SettingItem::int(
        SettingMeta::new(keys::BARS_PEAK_HEIGHT_RATIO, "Peak Height", "Bars").with_subtitle(
            "Peak bar size as % of bar width (ignored in LED mode — peaks are one segment tall)",
        ),
        config.bars.peak_height_ratio as i64,
        d.bars.peak_height_ratio as i64,
        10,
        100,
        5,
        "%",
    ));
    e.push(SettingItem::int(
        SettingMeta::new(keys::BARS_BAR_DEPTH_3D, "Isometric Depth", "Bars")
            .with_subtitle("3D top and side face depth in pixels, 0 = flat"),
        config.bars.bar_depth_3d as i64,
        d.bars.bar_depth_3d as i64,
        0,
        20,
        1,
        " px",
    ));
    e.push(SettingItem::float(
        SettingMeta::new(keys::BARS_FLASH_INTENSITY, "Peak Flash", "Bars")
            .with_subtitle("Bars bloom toward the peak color on a beat. 0 = disabled"),
        config.bars.flash_intensity as f64,
        d.bars.flash_intensity as f64,
        0.0,
        1.0,
        0.05,
        "",
    ));
    e.push(SettingItem::float(
        SettingMeta::new(keys::BARS_TRAILS, "Motion Trails", "Bars")
            .with_subtitle("Bars leave a fading after-image. 0 = off, 1 = long comet trails"),
        config.bars.trails as f64,
        d.bars.trails as f64,
        0.0,
        1.0,
        0.05,
        "",
    ));
    e.push(SettingItem::float(
        SettingMeta::new(keys::BARS_ECHO, "Echo", "Bars").with_subtitle(
            "Milkdrop feedback — the bars spiral and tunnel into themselves with the beat. 0 = off",
        ),
        config.bars.echo as f64,
        d.bars.echo as f64,
        0.0,
        1.0,
        0.05,
        "",
    ));

    // --- Bar Colors (Dark / Light) ---
    // These keys are theme-file-relative — they write to the active theme file,
    // not config.toml. The handler routes them via update_theme_value().
    for (prefix, label, colors, defaults) in [
        (
            "dark",
            "Bar Colors (Dark)",
            &theme.dark.visualizer,
            &dt.dark.visualizer,
        ),
        (
            "light",
            "Bar Colors (Light)",
            &theme.light.visualizer,
            &dt.light.visualizer,
        ),
    ] {
        push_visualizer_color_entries(&mut e, prefix, label, P, colors, defaults);
    }

    // --- Lines section ---
    e.push(SettingsEntry::Header {
        label: "Lines",
        icon: L,
    });
    e.push(SettingItem::enum_val(
        SettingMeta::new(keys::LINES_PLACEMENT, "Placement", "Lines").with_subtitle(
            "Where the Lines visualizer is drawn\nbottom_band: a band above the player bar (every view)\nover_cover: on the now-playing cover art (Queue view, while playing)",
        ),
        config.lines.placement.as_wire_str(),
        d.lines.placement.as_wire_str(),
        VisualizerPlacement::all_wire_strs(),
    ));
    e.push(SettingItem::int(
        SettingMeta::new(keys::LINES_POINT_COUNT, "Point Count", "Lines")
            .with_subtitle("8–512, more = finer detail"),
        config.lines.point_count as i64,
        d.lines.point_count as i64,
        8,
        512,
        8,
        "",
    ));
    e.push(SettingItem::float(
        SettingMeta::new(keys::LINES_LINE_THICKNESS, "Line Thickness", "Lines")
            .with_subtitle("% of visualizer height, 1–10%"),
        config.lines.line_thickness as f64,
        d.lines.line_thickness as f64,
        0.01,
        0.10,
        0.01,
        "%",
    ));
    e.push(SettingItem::float(
        SettingMeta::new(keys::LINES_OUTLINE_THICKNESS, "Outline Thickness", "Lines")
            .with_subtitle("Border behind the line in pixels, 0 = disabled"),
        config.lines.outline_thickness as f64,
        d.lines.outline_thickness as f64,
        0.0,
        5.0,
        0.5,
        " px",
    ));
    e.push(SettingItem::float(
        SettingMeta::new(keys::LINES_OUTLINE_OPACITY, "Outline Opacity", "Lines")
            .with_subtitle("0.0 = invisible, 1.0 = fully opaque"),
        config.lines.outline_opacity as f64,
        d.lines.outline_opacity as f64,
        0.0,
        1.0,
        0.1,
        "",
    ));
    e.push(SettingItem::float(
        SettingMeta::new(keys::LINES_ANIMATION_SPEED, "Animation Speed", "Lines")
            .with_subtitle("Color cycling speed. Lower = slower, higher = faster"),
        config.lines.animation_speed as f64,
        d.lines.animation_speed as f64,
        0.05,
        1.0,
        0.05,
        "",
    ));
    e.push(SettingItem::enum_val(
        SettingMeta::new(keys::LINES_GRADIENT_MODE, "Gradient Mode", "Lines")
            .with_subtitle(
                "breathing: time-based cycling through gradient palette\nstatic: uses first gradient color only\nposition: color by horizontal position (bass → treble rainbow)\nheight: color by amplitude (quiet → loud)\ngradient: position + amplitude blend (peaks shift palette)",
            ),
        config.lines.gradient_mode.as_wire_str(),
        d.lines.gradient_mode.as_wire_str(),
        LinesGradientMode::all_wire_strs(),
    ));
    e.push(SettingItem::float(
        SettingMeta::new(keys::LINES_FILL_OPACITY, "Fill Opacity", "Lines")
            .with_subtitle("Fills under the curve with a gradient. 0 = disabled"),
        config.lines.fill_opacity as f64,
        d.lines.fill_opacity as f64,
        0.0,
        1.0,
        0.05,
        "",
    ));
    e.push(SettingItem::float(
        SettingMeta::new(keys::LINES_GLOW_INTENSITY, "Glow Intensity", "Lines")
            .with_subtitle("Neon halo around the line. 0 = disabled, brightens with loudness"),
        config.lines.glow_intensity as f64,
        d.lines.glow_intensity as f64,
        0.0,
        1.0,
        0.05,
        "",
    ));
    e.push(SettingItem::bool_val(
        SettingMeta::new(keys::LINES_MIRROR, "Mirror", "Lines")
            .with_subtitle("Symmetric oscilloscope — line extends from center"),
        config.lines.mirror,
        d.lines.mirror,
    ));
    e.push(SettingItem::enum_val(
        SettingMeta::new(keys::LINES_STYLE, "Line Style", "Lines")
            .with_subtitle(
                "Interpolation between data points\nsmooth: Catmull-Rom spline (curvy)\nangular: straight line segments",
            ),
        config.lines.style.as_wire_str(),
        d.lines.style.as_wire_str(),
        LinesStyle::all_wire_strs(),
    ));
    e.push(SettingItem::bool_val(
        SettingMeta::new(keys::LINES_BOAT, "Surfing boat", "Lines")
            .with_subtitle("Show a small boat that rides the waveform"),
        config.lines.boat,
        d.lines.boat,
    ));
    e.push(SettingItem::float(
        SettingMeta::new(keys::LINES_TRAILS, "Motion Trails", "Lines")
            .with_subtitle("The line leaves a fading after-image. 0 = off, 1 = long comet trails"),
        config.lines.trails as f64,
        d.lines.trails as f64,
        0.0,
        1.0,
        0.05,
        "",
    ));
    e.push(SettingItem::float(
        SettingMeta::new(keys::LINES_ECHO, "Echo", "Lines").with_subtitle(
            "Milkdrop feedback — the line spirals and tunnels into itself with the beat. 0 = off",
        ),
        config.lines.echo as f64,
        d.lines.echo as f64,
        0.0,
        1.0,
        0.05,
        "",
    ));

    // --- Scope section (circular oscilloscope) ---
    e.push(SettingsEntry::Header {
        label: "Scope",
        icon: SC,
    });
    e.push(SettingItem::float(
        SettingMeta::new(keys::SCOPE_RADIUS, "Ring Size", "Scope").with_subtitle(
            "Mean ring radius over the cover. 0.1 = small inner ring, 0.95 = nearly fills the panel",
        ),
        config.scope.radius as f64,
        d.scope.radius as f64,
        0.1,
        0.95,
        0.05,
        "",
    ));
    e.push(SettingItem::float(
        SettingMeta::new(keys::SCOPE_SENSITIVITY, "Sensitivity", "Scope").with_subtitle(
            "How hard loud audio swings the ring in and out. 0.5 = subtle, 5 = wild",
        ),
        config.scope.sensitivity as f64,
        d.scope.sensitivity as f64,
        0.5,
        5.0,
        0.1,
        "×",
    ));
    e.push(SettingItem::int(
        SettingMeta::new(keys::SCOPE_POINT_COUNT, "Point Count", "Scope")
            .with_subtitle("Points around the ring. 16 = chunky, 512 = finely detailed waveform"),
        config.scope.point_count as i64,
        d.scope.point_count as i64,
        16,
        512,
        16,
        "",
    ));
    e.push(SettingItem::float(
        SettingMeta::new(keys::SCOPE_LINE_THICKNESS, "Line Thickness", "Scope")
            .with_subtitle("Ring stroke as % of panel size, 0.5–10%"),
        config.scope.line_thickness as f64,
        d.scope.line_thickness as f64,
        0.005,
        0.1,
        0.005,
        "%",
    ));
    e.push(SettingItem::float(
        SettingMeta::new(keys::SCOPE_FILL_OPACITY, "Fill Opacity", "Scope").with_subtitle(
            "Radial gradient fill from the ring toward the center. 0 = outline only, 1 = solid rim",
        ),
        config.scope.fill_opacity as f64,
        d.scope.fill_opacity as f64,
        0.0,
        1.0,
        0.05,
        "",
    ));
    e.push(SettingItem::float(
        SettingMeta::new(keys::SCOPE_GLOW_INTENSITY, "Glow Intensity", "Scope")
            .with_subtitle("Neon halo around the ring. 0 = disabled, brightens with loudness"),
        config.scope.glow_intensity as f64,
        d.scope.glow_intensity as f64,
        0.0,
        1.0,
        0.05,
        "",
    ));
    e.push(SettingItem::float(
        SettingMeta::new(keys::SCOPE_OUTLINE_THICKNESS, "Outline Thickness", "Scope")
            .with_subtitle("Darker border behind the ring in pixels, 0 = disabled"),
        config.scope.outline_thickness as f64,
        d.scope.outline_thickness as f64,
        0.0,
        5.0,
        0.5,
        " px",
    ));
    e.push(SettingItem::float(
        SettingMeta::new(keys::SCOPE_OUTLINE_OPACITY, "Outline Opacity", "Scope")
            .with_subtitle("0.0 = invisible, 1.0 = fully opaque"),
        config.scope.outline_opacity as f64,
        d.scope.outline_opacity as f64,
        0.0,
        1.0,
        0.1,
        "",
    ));
    e.push(SettingItem::enum_val(
        SettingMeta::new(keys::SCOPE_GRADIENT_MODE, "Gradient Mode", "Scope").with_subtitle(
            "breathing: time-based cycling through gradient palette\nstatic: uses first gradient color only\nposition: color by angle around the ring\nheight: color by amplitude (quiet → loud)\ngradient: angle + amplitude blend (peaks shift palette)",
        ),
        config.scope.gradient_mode.as_wire_str(),
        d.scope.gradient_mode.as_wire_str(),
        LinesGradientMode::all_wire_strs(),
    ));
    e.push(SettingItem::float(
        SettingMeta::new(keys::SCOPE_ANIMATION_SPEED, "Animation Speed", "Scope")
            .with_subtitle("Color cycling speed for the breathing gradient. Lower = slower"),
        config.scope.animation_speed as f64,
        d.scope.animation_speed as f64,
        0.05,
        1.0,
        0.05,
        "",
    ));
    e.push(SettingItem::enum_val(
        SettingMeta::new(keys::SCOPE_STYLE, "Line Style", "Scope").with_subtitle(
            "Interpolation around the ring\nsmooth: Catmull-Rom spline (curvy)\nangular: straight segments",
        ),
        config.scope.style.as_wire_str(),
        d.scope.style.as_wire_str(),
        LinesStyle::all_wire_strs(),
    ));
    e.push(SettingItem::bool_val(
        SettingMeta::new(keys::SCOPE_PARTICLES, "Particles", "Scope")
            .with_subtitle("Glowing particles drifting out from the ring (NCS-style)"),
        config.scope.particles,
        d.scope.particles,
    ));
    e.push(SettingItem::int(
        SettingMeta::new(keys::SCOPE_PARTICLE_COUNT, "Particle Count", "Scope")
            .with_subtitle("How many particles fill the field. 0 = none, 2048 = dense"),
        config.scope.particle_count as i64,
        d.scope.particle_count as i64,
        0,
        2048,
        64,
        "",
    ));
    e.push(SettingItem::float(
        SettingMeta::new(keys::SCOPE_PARTICLE_SPEED, "Particle Speed", "Scope")
            .with_subtitle("How fast particles fly out. 0.1 = lazy drift, 4 = energetic"),
        config.scope.particle_speed as f64,
        d.scope.particle_speed as f64,
        0.1,
        4.0,
        0.1,
        "×",
    ));
    e.push(SettingItem::bool_val(
        SettingMeta::new(keys::SCOPE_BEAM, "Beam Glow", "Scope").with_subtitle(
            "Additive luminous beam (woscope-style) — the ring glows brighter over the cover. Pair with Glow",
        ),
        config.scope.beam,
        d.scope.beam,
    ));
    e.push(SettingItem::float(
        SettingMeta::new(keys::SCOPE_TRAILS, "Motion Trails", "Scope")
            .with_subtitle("The ring leaves a fading after-image. 0 = off, 1 = long comet trails"),
        config.scope.trails as f64,
        d.scope.trails as f64,
        0.0,
        1.0,
        0.05,
        "",
    ));
    e.push(SettingItem::float(
        SettingMeta::new(keys::SCOPE_ECHO, "Echo", "Scope").with_subtitle(
            "Milkdrop feedback — the ring spirals and tunnels inward with the beat. 0 = off",
        ),
        config.scope.echo as f64,
        d.scope.echo as f64,
        0.0,
        1.0,
        0.05,
        "",
    ));

    e
}
