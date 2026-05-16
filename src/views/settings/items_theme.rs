//! Theme tab setting entries
//!
//! Builds the settings UI for the Theme tab using the active `ThemeFile`.
//! Color keys use theme-file-relative paths (e.g. `dark.background.hard`)
//! and are written to the active theme file via `config_writer::update_theme_value`.

use nokkvi_data::types::theme_file::ThemeFile;

use super::{
    items::{SettingItem, SettingMeta, SettingsEntry},
    sentinel::SentinelKind,
};

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
    e.push(
        SettingItem::text(
            SettingMeta::new(
                SentinelKind::RestoreTheme.to_key(),
                "⟲ Restore Defaults",
                "Select Theme",
            )
            .with_subtitle("Restore this theme to its original built-in colors"),
            "Press Enter",
            "Press Enter",
        )
        .with_theme_key(),
    );

    // List all discovered themes
    let themes = presets::all_themes();
    for (i, info) in themes.iter().enumerate() {
        let key = SentinelKind::PresetTheme(i as u32).to_key();
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
        e.push(
            SettingItem::text(SettingMeta::new(key, &label, "Select Theme"), sub, "")
                .with_theme_key(),
        );
    }

    // ── Appearance ───────────────────────────────────────────────────
    const A: &str = "assets/icons/monitor.svg";
    e.push(SettingsEntry::Header {
        label: "Appearance",
        icon: A,
    });
    let theme_val = if is_light_mode { "Light" } else { "Dark" };
    e.push(SettingItem::enum_val(
        SettingMeta::new("general.light_mode", "Theme Mode", "Appearance")
            .with_subtitle("Switch between dark and light"),
        theme_val,
        "Dark",
        vec!["Dark", "Light"],
    ));
    e.push(SettingItem::bool_val(
        SettingMeta::new("general.rounded_mode", "Rounded Corners", "Appearance")
            .with_subtitle("Apply rounded borders to UI elements"),
        rounded_mode,
        false,
    ));
    e.push(SettingItem::bool_val(
        SettingMeta::new("general.opacity_gradient", "Opacity Gradient", "Appearance")
            .with_subtitle("Fade non-center slots in list views"),
        opacity_gradient,
        true,
    ));

    // ── Background Colors ────────────────────────────────────────────
    e.push(SettingsEntry::Header {
        label: "Background Colors",
        icon: P,
    });
    e.push(
        SettingItem::text(
            SettingMeta::new(
                SentinelKind::RestoreBg.to_key(),
                "⟲ Restore Defaults",
                "Background Colors",
            ),
            "Press Enter",
            "Press Enter",
        )
        .with_theme_key(),
    );
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
        e.push(
            SettingItem::hex_color(
                SettingMeta::new(
                    key,
                    &format!("BG {field} ({palette_label})"),
                    "Background Colors",
                ),
                value,
                default,
            )
            .with_theme_key(),
        );
    }

    // ── Foreground Colors ────────────────────────────────────────────
    e.push(SettingsEntry::Header {
        label: "Foreground Colors",
        icon: P,
    });
    e.push(
        SettingItem::text(
            SettingMeta::new(
                SentinelKind::RestoreFg.to_key(),
                "⟲ Restore Defaults",
                "Foreground Colors",
            ),
            "Press Enter",
            "Press Enter",
        )
        .with_theme_key(),
    );
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
        e.push(
            SettingItem::hex_color(
                SettingMeta::new(
                    key,
                    &format!("FG {field} ({palette_label})"),
                    "Foreground Colors",
                ),
                value,
                default,
            )
            .with_theme_key(),
        );
    }

    // ── Accent Colors ────────────────────────────────────────────────
    e.push(SettingsEntry::Header {
        label: "Accent Colors",
        icon: P,
    });
    e.push(
        SettingItem::text(
            SettingMeta::new(
                SentinelKind::RestoreAccent.to_key(),
                "⟲ Restore Defaults",
                "Accent Colors",
            ),
            "Press Enter",
            "Press Enter",
        )
        .with_theme_key(),
    );
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
        e.push(
            SettingItem::hex_color(
                SettingMeta::new(
                    key,
                    &format!("Accent {field} ({palette_label})"),
                    "Accent Colors",
                ),
                value,
                default,
            )
            .with_theme_key(),
        );
    }

    // ── Semantic Colors ──────────────────────────────────────────────
    e.push(SettingsEntry::Header {
        label: "Semantic Colors",
        icon: P,
    });
    e.push(
        SettingItem::text(
            SettingMeta::new(
                SentinelKind::RestoreSemantic.to_key(),
                "⟲ Restore Defaults",
                "Semantic Colors",
            ),
            "Press Enter",
            "Press Enter",
        )
        .with_theme_key(),
    );
    for (color_name, section, base, bright, def_base, def_bright) in [
        (
            "Danger",
            "danger",
            &palette.danger.base,
            &palette.danger.bright,
            &default_palette.danger.base,
            &default_palette.danger.bright,
        ),
        (
            "Success",
            "success",
            &palette.success.base,
            &palette.success.bright,
            &default_palette.success.base,
            &default_palette.success.bright,
        ),
        (
            "Warning",
            "warning",
            &palette.warning.base,
            &palette.warning.bright,
            &default_palette.warning.base,
            &default_palette.warning.bright,
        ),
        (
            "Star",
            "star",
            &palette.star.base,
            &palette.star.bright,
            &default_palette.star.base,
            &default_palette.star.bright,
        ),
    ] {
        let key_base = format!("{palette_prefix}.{section}.base");
        let key_bright = format!("{palette_prefix}.{section}.bright");
        e.push(
            SettingItem::hex_color(
                SettingMeta::new(
                    key_base,
                    &format!("{color_name} Base ({palette_label})"),
                    "Semantic Colors",
                ),
                base,
                def_base,
            )
            .with_theme_key(),
        );
        e.push(
            SettingItem::hex_color(
                SettingMeta::new(
                    key_bright,
                    &format!("{color_name} Bright ({palette_label})"),
                    "Semantic Colors",
                ),
                bright,
                def_bright,
            )
            .with_theme_key(),
        );
    }

    e
}
