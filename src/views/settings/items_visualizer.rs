//! Visualizer tab setting entries

use nokkvi_data::{
    services::settings_tables::visualizer::build_visualizer_tab_settings_items,
    types::{
        settings_data::VisualizerSettingsData,
        theme_file::{ThemeFile, VisualizerColors},
    },
};

use super::{
    items::{MacroRows, SettingItem, SettingMeta, SettingsEntry},
    sentinel::SentinelKind,
};
use crate::visualizer_config::{VisualizerConfig, keys};

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
/// The 64 scalar/enum rows come from `define_settings!` via
/// `build_visualizer_tab_settings_items` (the Visualizer table in
/// `data/src/services/settings_tables/visualizer.rs`) — one `ui_meta` cluster
/// per dispatchable key, consumed here through the `MacroRows` take-in-
/// display-order pattern. Section headers, the Restore-Defaults sentinel row,
/// and the theme-routed Bar Colors (Dark/Light) sections stay hand-written
/// (colors are surgical-write rows with no dispatch arm).
///
/// `theme` provides the current visualizer colors (from the active theme file).
/// Accepts it as a parameter rather than loading from disk, keeping this function
/// pure and testable.
pub(crate) fn build_visualizer_items(
    config: &VisualizerConfig,
    theme: &ThemeFile,
    active_stem: &str,
) -> Vec<SettingsEntry> {
    let dt =
        nokkvi_data::services::theme_loader::load_builtin_theme(active_stem).unwrap_or_default();
    const F: &str = "assets/icons/layout-grid.svg";
    const S: &str = "assets/icons/sliders-horizontal.svg";
    const B: &str = "assets/icons/audio-lines.svg";
    const P: &str = "assets/icons/palette.svg";
    const L: &str = "assets/icons/audio-waveform.svg";
    const SC: &str = "assets/icons/radar.svg";

    let data = VisualizerSettingsData::from(config);
    let mut m = MacroRows::new(build_visualizer_tab_settings_items(&data));

    let mut e = Vec::with_capacity(80);

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
    e.push(m.take(keys::HEIGHT_PERCENT));
    e.push(m.take(keys::OPACITY));
    e.push(m.take(keys::BLOOM));
    e.push(m.take(keys::BLOOM_INTENSITY));
    e.push(m.take(keys::BEAT_REACTIVITY));
    e.push(m.take(keys::CRT));

    // --- Signal section (FFT/DSP, affects both Bars and Lines modes) ---
    e.push(SettingsEntry::Header {
        label: "Signal",
        icon: S,
    });
    e.push(m.take(keys::NOISE_REDUCTION));
    e.push(m.take(keys::LOWER_CUTOFF_FREQ));
    e.push(m.take(keys::HIGHER_CUTOFF_FREQ));
    e.push(m.take(keys::AUTO_SENSITIVITY));

    // --- Bars section ---
    e.push(SettingsEntry::Header {
        label: "Bars",
        icon: B,
    });
    e.push(m.take(keys::BARS_PLACEMENT));
    e.push(m.take(keys::WAVES));
    e.push(m.take(keys::WAVES_SMOOTHING));
    e.push(m.take(keys::MONSTERCAT));
    e.push(m.take(keys::BARS_MAX_BARS));
    e.push(m.take(keys::BARS_BAR_WIDTH_MIN));
    e.push(m.take(keys::BARS_BAR_WIDTH_MAX));
    e.push(m.take(keys::BARS_BAR_SPACING));
    e.push(m.take(keys::BARS_BORDER_WIDTH));
    e.push(m.take(keys::BARS_LED_BARS));
    e.push(m.take(keys::BARS_LED_SEGMENT_HEIGHT));
    e.push(m.take(keys::BARS_GRADIENT_MODE));
    e.push(m.take(keys::BARS_GRADIENT_ORIENTATION));
    e.push(m.take(keys::BARS_PEAK_GRADIENT_MODE));
    e.push(m.take(keys::BARS_PEAK_MODE));
    e.push(m.take(keys::BARS_PEAK_HOLD_TIME));
    e.push(m.take(keys::BARS_PEAK_FADE_TIME));
    e.push(m.take(keys::BARS_PEAK_FALL_SPEED));
    e.push(m.take(keys::BARS_PEAK_HEIGHT_RATIO));
    e.push(m.take(keys::BARS_BAR_DEPTH_3D));
    e.push(m.take(keys::BARS_FLASH_INTENSITY));
    e.push(m.take(keys::BARS_TRAILS));
    e.push(m.take(keys::BARS_ECHO));

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
    e.push(m.take(keys::LINES_PLACEMENT));
    e.push(m.take(keys::LINES_POINT_COUNT));
    e.push(m.take(keys::LINES_LINE_THICKNESS));
    e.push(m.take(keys::LINES_OUTLINE_THICKNESS));
    e.push(m.take(keys::LINES_OUTLINE_OPACITY));
    e.push(m.take(keys::LINES_ANIMATION_SPEED));
    e.push(m.take(keys::LINES_GRADIENT_MODE));
    e.push(m.take(keys::LINES_FILL_OPACITY));
    e.push(m.take(keys::LINES_GLOW_INTENSITY));
    e.push(m.take(keys::LINES_MIRROR));
    e.push(m.take(keys::LINES_STYLE));
    e.push(m.take(keys::LINES_BOAT));
    e.push(m.take(keys::LINES_TRAILS));
    e.push(m.take(keys::LINES_ECHO));

    // --- Scope section (circular oscilloscope) ---
    e.push(SettingsEntry::Header {
        label: "Scope",
        icon: SC,
    });
    e.push(m.take(keys::SCOPE_RADIUS));
    e.push(m.take(keys::SCOPE_SENSITIVITY));
    e.push(m.take(keys::SCOPE_POINT_COUNT));
    e.push(m.take(keys::SCOPE_LINE_THICKNESS));
    e.push(m.take(keys::SCOPE_FILL_OPACITY));
    e.push(m.take(keys::SCOPE_GLOW_INTENSITY));
    e.push(m.take(keys::SCOPE_OUTLINE_THICKNESS));
    e.push(m.take(keys::SCOPE_OUTLINE_OPACITY));
    e.push(m.take(keys::SCOPE_GRADIENT_MODE));
    e.push(m.take(keys::SCOPE_ANIMATION_SPEED));
    e.push(m.take(keys::SCOPE_STYLE));
    e.push(m.take(keys::SCOPE_PARTICLES));
    e.push(m.take(keys::SCOPE_PARTICLE_COUNT));
    e.push(m.take(keys::SCOPE_PARTICLE_SPEED));
    e.push(m.take(keys::SCOPE_BEAM));
    e.push(m.take(keys::SCOPE_TRAILS));
    e.push(m.take(keys::SCOPE_ECHO));

    m.finish();
    e
}
