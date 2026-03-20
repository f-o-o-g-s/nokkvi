//! Theme tab setting entries

use super::items::{SettingItem, SettingsEntry};
use crate::theme_config::DualThemeConfig;

/// Build settings entries for the Theme tab from dual theme config.
/// Shows the active palette (dark or light) colors based on current mode.
///
/// Each hex color entry has a real TOML key (e.g. `theme.dark.background.hard`)
/// and a real default from `DualThemeConfig::default()`, making them editable
/// and persisted. Each color subgroup includes a "⟲ Restore Defaults" entry.
/// Presets are listed inline (no sub-list).
pub(crate) fn build_theme_items(
    config: &DualThemeConfig,
    rounded_mode: bool,
    opacity_gradient: bool,
    is_light_mode: bool,
) -> Vec<SettingsEntry> {
    use super::presets;

    const P: &str = "assets/icons/palette.svg";
    const PR: &str = "assets/icons/swatch-book.svg";
    const F: &str = "assets/icons/type.svg";
    let mut e = Vec::new();

    let is_light = crate::theme::is_light_mode();
    let palette_prefix = if is_light {
        "theme.light"
    } else {
        "theme.dark"
    };
    let palette_label = if is_light { "Light" } else { "Dark" };
    let palette = if is_light {
        &config.light
    } else {
        &config.dark
    };
    let defaults = DualThemeConfig::default();
    let default_palette = if is_light {
        &defaults.light
    } else {
        &defaults.dark
    };

    // ── Presets (inline — no sub-list) ──────────────────────────────
    e.push(SettingsEntry::Header {
        label: "Presets",
        icon: PR,
    });
    e.push(SettingItem::text(
        meta!(
            "__restore_theme",
            "⟲ Restore Defaults",
            "Presets",
            "Restore all theme colors to the default palette. Overrides any color customizations."
        ),
        "Press Enter",
        "Press Enter",
    ));
    for (i, preset) in presets::all_presets().iter().enumerate() {
        let key = format!("__preset_{i}");
        e.push(SettingItem::text(
            meta!(key, preset.name, "Presets"),
            preset.description,
            "",
        ));
    }

    // ── Font ─────────────────────────────────────────────────────────
    e.push(SettingsEntry::Header {
        label: "Font",
        icon: F,
    });
    let font_display = if config.font.family.is_empty() {
        "(system default)"
    } else {
        &config.font.family
    };
    e.push(SettingItem::text(
        meta!(
            "theme.font.family",
            "Font Family",
            "Font",
            "Enter to browse installed fonts"
        ),
        font_display,
        "(system default)",
    ));

    // ── Appearance ───────────────────────────────────────────────────
    const A: &str = "assets/icons/monitor.svg";
    e.push(SettingsEntry::Header {
        label: "Appearance",
        icon: A,
    });
    let theme_val = if is_light_mode { "Light" } else { "Dark" };
    e.push(SettingItem::enum_val(
        meta!(
            "general.light_mode",
            "Theme Mode",
            "Appearance",
            "Switch between dark and light"
        ),
        theme_val,
        "Dark",
        vec!["Dark", "Light"],
    ));
    e.push(SettingItem::bool_val(
        meta!(
            "general.rounded_mode",
            "Rounded Corners",
            "Appearance",
            "Apply rounded borders to UI elements"
        ),
        rounded_mode,
        false,
    ));
    e.push(SettingItem::bool_val(
        meta!(
            "general.opacity_gradient",
            "Opacity Gradient",
            "Appearance",
            "Fade non-center slots in list views"
        ),
        opacity_gradient,
        true,
    ));

    // ── Background Colors ────────────────────────────────────────────
    e.push(SettingsEntry::Header {
        label: "Background Colors",
        icon: P,
    });
    e.push(SettingItem::text(
        meta!("__restore_bg", "⟲ Restore Defaults", "Background Colors"),
        "Press Enter",
        "Press Enter",
    ));
    for (field, value, default) in [
        (
            "hard",
            &palette.background.hard,
            &default_palette.background.hard,
        ),
        (
            "default",
            &palette.background.default,
            &default_palette.background.default,
        ),
        (
            "soft",
            &palette.background.soft,
            &default_palette.background.soft,
        ),
        (
            "level1",
            &palette.background.level1,
            &default_palette.background.level1,
        ),
        (
            "level2",
            &palette.background.level2,
            &default_palette.background.level2,
        ),
        (
            "level3",
            &palette.background.level3,
            &default_palette.background.level3,
        ),
        (
            "level4",
            &palette.background.level4,
            &default_palette.background.level4,
        ),
    ] {
        let key = format!("{palette_prefix}.background.{field}");
        e.push(SettingItem::hex_color(
            meta!(
                key,
                &format!("BG {field} ({palette_label})"),
                "Background Colors"
            ),
            value,
            default,
        ));
    }

    // ── Foreground Colors ────────────────────────────────────────────
    e.push(SettingsEntry::Header {
        label: "Foreground Colors",
        icon: P,
    });
    e.push(SettingItem::text(
        meta!("__restore_fg", "⟲ Restore Defaults", "Foreground Colors"),
        "Press Enter",
        "Press Enter",
    ));
    for (field, value, default) in [
        (
            "bright",
            &palette.foreground.bright,
            &default_palette.foreground.bright,
        ),
        (
            "level1",
            &palette.foreground.level1,
            &default_palette.foreground.level1,
        ),
        (
            "level2",
            &palette.foreground.level2,
            &default_palette.foreground.level2,
        ),
        (
            "level3",
            &palette.foreground.level3,
            &default_palette.foreground.level3,
        ),
        (
            "level4",
            &palette.foreground.level4,
            &default_palette.foreground.level4,
        ),
        (
            "gray",
            &palette.foreground.gray,
            &default_palette.foreground.gray,
        ),
    ] {
        let key = format!("{palette_prefix}.foreground.{field}");
        e.push(SettingItem::hex_color(
            meta!(
                key,
                &format!("FG {field} ({palette_label})"),
                "Foreground Colors"
            ),
            value,
            default,
        ));
    }

    // ── Accent Colors ────────────────────────────────────────────────
    e.push(SettingsEntry::Header {
        label: "Accent Colors",
        icon: P,
    });
    e.push(SettingItem::text(
        meta!("__restore_accent", "⟲ Restore Defaults", "Accent Colors"),
        "Press Enter",
        "Press Enter",
    ));
    for (field, value, default) in [
        (
            "primary",
            &palette.accent.primary,
            &default_palette.accent.primary,
        ),
        (
            "bright",
            &palette.accent.bright,
            &default_palette.accent.bright,
        ),
        (
            "border_dark",
            &palette.accent.border_dark,
            &default_palette.accent.border_dark,
        ),
        (
            "border_light",
            &palette.accent.border_light,
            &default_palette.accent.border_light,
        ),
        (
            "now_playing",
            &palette.accent.now_playing,
            &default_palette.accent.now_playing,
        ),
        (
            "selected",
            &palette.accent.selected,
            &default_palette.accent.selected,
        ),
    ] {
        let key = format!("{palette_prefix}.accent.{field}");
        e.push(SettingItem::hex_color(
            meta!(
                key,
                &format!("Accent {field} ({palette_label})"),
                "Accent Colors"
            ),
            value,
            default,
        ));
    }

    // ── Named Colors ─────────────────────────────────────────────────
    e.push(SettingsEntry::Header {
        label: "Named Colors",
        icon: P,
    });
    e.push(SettingItem::text(
        meta!("__restore_named", "⟲ Restore Defaults", "Named Colors"),
        "Press Enter",
        "Press Enter",
    ));
    for (color_name, section, normal, bright, def_normal, def_bright) in [
        (
            "Red",
            "red",
            &palette.red.normal,
            &palette.red.bright,
            &default_palette.red.normal,
            &default_palette.red.bright,
        ),
        (
            "Green",
            "green",
            &palette.green.normal,
            &palette.green.bright,
            &default_palette.green.normal,
            &default_palette.green.bright,
        ),
        (
            "Yellow",
            "yellow",
            &palette.yellow.normal,
            &palette.yellow.bright,
            &default_palette.yellow.normal,
            &default_palette.yellow.bright,
        ),
        (
            "Purple",
            "purple",
            &palette.purple.normal,
            &palette.purple.bright,
            &default_palette.purple.normal,
            &default_palette.purple.bright,
        ),
        (
            "Aqua",
            "aqua",
            &palette.aqua.normal,
            &palette.aqua.bright,
            &default_palette.aqua.normal,
            &default_palette.aqua.bright,
        ),
        (
            "Orange",
            "orange",
            &palette.orange.normal,
            &palette.orange.bright,
            &default_palette.orange.normal,
            &default_palette.orange.bright,
        ),
    ] {
        let key_normal = format!("{palette_prefix}.{section}.normal");
        let key_bright = format!("{palette_prefix}.{section}.bright");
        e.push(SettingItem::hex_color(
            meta!(
                key_normal,
                &format!("{color_name} Normal ({palette_label})"),
                "Named Colors"
            ),
            normal,
            def_normal,
        ));
        e.push(SettingItem::hex_color(
            meta!(
                key_bright,
                &format!("{color_name} Bright ({palette_label})"),
                "Named Colors"
            ),
            bright,
            def_bright,
        ));
    }

    e
}
