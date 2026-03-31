//! Theme tab setting entries
//!
//! Builds the settings UI for the Theme tab using the active `ThemeFile`.
//! Color keys use theme-file-relative paths (e.g. `dark.background.hard`)
//! and are written to the active theme file via `config_writer::update_theme_value`.

use nokkvi_data::types::theme_file::ThemeFile;

use super::items::{SettingItem, SettingsEntry};

/// Build settings entries for the Theme tab from the active theme file.
/// Shows the active palette (dark or light) colors based on current mode.
///
/// Keys are theme-file-relative (e.g. `dark.background.hard`), not config.toml
/// paths. The settings handler routes these through `update_theme_value`.
/// Presets display discovered themes from `~/.config/nokkvi/themes/`.
pub(crate) fn build_theme_items(
    theme: &ThemeFile,
    active_stem: &str,
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
    let palette_prefix = if is_light { "light" } else { "dark" };
    let palette_label = if is_light { "Light" } else { "Dark" };
    let palette = if is_light { &theme.light } else { &theme.dark };
    let defaults =
        nokkvi_data::services::theme_loader::load_builtin_theme(active_stem).unwrap_or_default();
    let default_palette = if is_light {
        &defaults.light
    } else {
        &defaults.dark
    };

    // ── Theme Picker ─────────────────────────────────────────────────
    e.push(SettingsEntry::Header {
        label: "Select Theme",
        icon: PR,
    });

    // Restore Defaults (only for built-in themes)
    e.push(SettingItem::text(
        meta!(
            "__restore_theme",
            "⟲ Restore Defaults",
            "Select Theme",
            "Restore this theme to its original built-in colors"
        ),
        "Press Enter",
        "Press Enter",
    ));

    // List all discovered themes
    let themes = presets::all_themes();
    for (i, info) in themes.iter().enumerate() {
        let key = format!("__preset_{i}");
        let suffix = if info.stem == active_stem {
            " ● active"
        } else {
            ""
        };
        let label = format!("{}{suffix}", info.display_name);
        let sub = if info.is_builtin {
            "Built-in"
        } else {
            "Custom"
        };
        e.push(SettingItem::text(
            meta!(key, &label, "Select Theme"),
            sub,
            "",
        ));
    }

    // ── Font ─────────────────────────────────────────────────────────
    e.push(SettingsEntry::Header {
        label: "Font",
        icon: F,
    });
    let font_display = if theme.font_family.is_empty() {
        "(system default)"
    } else {
        &theme.font_family
    };
    e.push(SettingItem::text(
        meta!(
            "font_family",
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
