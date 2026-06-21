//! Theme tab setting entries
//!
//! Builds the settings UI for the Theme tab using the active `ThemeFile`.
//! Color keys use theme-file-relative paths (e.g. `dark.background.hard`)
//! and are written to the active theme file via `config_writer::update_theme_value`.

use nokkvi_data::types::{player_settings::RoundedMode, theme_file::ThemeFile};

use super::{
    items::{ActivateKind, SettingItem, SettingMeta, SettingsEntry},
    sentinel::SentinelKind,
};

/// Push a "flat" color section (Background / Foreground / Accent) into the
/// entries list.
///
/// Expands to: a section header, a restore-defaults sentinel row, and one
/// `hex_color` row per `$field`. Each listed field is bound at compile time
/// to a struct field of `$palette.$section` — a stale entry (field renamed
/// or removed from the palette struct) is a compile error. The reverse is
/// NOT enforced: adding a new palette field without listing it here is
/// silent, so fan new fields out to this list explicitly.
///
/// `$icon` is the section icon, `$header_label` the header text (also reused
/// as the row category), `$sentinel` the typed `SentinelKind` for the
/// restore row, and `$label_prefix` the per-row label prefix
/// (e.g. "BG" → "BG hard (Dark)").
macro_rules! push_color_section {
    (
        $e:expr,
        $prefix:expr,
        $palette_label:expr,
        $palette:expr,
        $default:expr,
        $section:ident,
        $icon:expr,
        $header_label:expr,
        $sentinel:expr,
        $label_prefix:expr,
        [$($field:ident),+ $(,)?]
    ) => {{
        $e.push(SettingsEntry::Header {
            label: $header_label,
            icon: $icon,
        });
        $e.push(
            SettingItem::text(
                SettingMeta::new(
                    $sentinel.to_key(),
                    "⟲ Restore Defaults",
                    $header_label,
                ),
                "Press Enter",
                "Press Enter",
            )
            .with_theme_key(),
        );
        $(
            {
                let key = format!(
                    "{}.{}.{}",
                    $prefix,
                    stringify!($section),
                    stringify!($field),
                );
                let label_text = format!(
                    "{} {} ({})",
                    $label_prefix,
                    stringify!($field),
                    $palette_label,
                );
                $e.push(
                    SettingItem::hex_color(
                        SettingMeta::new(key, &label_text, $header_label),
                        &$palette.$section.$field,
                        &$default.$section.$field,
                    )
                    .with_theme_key(),
                );
            }
        )+
    }};
}

/// Push the Semantic Colors section.
///
/// Each `(emotion_ident, "Display Name")` pair expands to two `hex_color`
/// rows (`.base` and `.bright`). The emotion ident must match a
/// `SemanticColorConfig` field on `$palette`. Adding a new emotion to
/// `ThemePalette` without picking it up here is a compile error.
macro_rules! push_semantic_color_section {
    (
        $e:expr,
        $prefix:expr,
        $palette_label:expr,
        $palette:expr,
        $default:expr,
        $icon:expr,
        $header_label:expr,
        $sentinel:expr,
        [$(($emotion:ident, $name:literal)),+ $(,)?]
    ) => {{
        $e.push(SettingsEntry::Header {
            label: $header_label,
            icon: $icon,
        });
        $e.push(
            SettingItem::text(
                SettingMeta::new(
                    $sentinel.to_key(),
                    "⟲ Restore Defaults",
                    $header_label,
                ),
                "Press Enter",
                "Press Enter",
            )
            .with_theme_key(),
        );
        $(
            {
                let key_base = format!(
                    "{}.{}.base",
                    $prefix,
                    stringify!($emotion),
                );
                let key_bright = format!(
                    "{}.{}.bright",
                    $prefix,
                    stringify!($emotion),
                );
                let label_base = format!("{} Base ({})", $name, $palette_label);
                let label_bright = format!("{} Bright ({})", $name, $palette_label);
                $e.push(
                    SettingItem::hex_color(
                        SettingMeta::new(key_base, &label_base, $header_label),
                        &$palette.$emotion.base,
                        &$default.$emotion.base,
                    )
                    .with_theme_key(),
                );
                $e.push(
                    SettingItem::hex_color(
                        SettingMeta::new(key_bright, &label_bright, $header_label),
                        &$palette.$emotion.bright,
                        &$default.$emotion.bright,
                    )
                    .with_theme_key(),
                );
            }
        )+
    }};
}

/// Build settings entries for the Theme tab from the active theme file.
/// Shows the active palette (dark or light) colors based on current mode.
///
/// Keys are theme-file-relative (e.g. `dark.background.hard`), not config.toml
/// paths. The settings handler routes these through `update_theme_value`.
/// Presets display discovered themes from `~/.config/nokkvi/themes/`.
pub(crate) fn build_theme_items(
    theme: &ThemeFile,
    active_stem: &str,
    rounded_mode: RoundedMode,
    opacity_gradient: bool,
    is_light_mode: bool,
) -> Vec<SettingsEntry> {
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

    // ── Mode + Display ───────────────────────────────────────────────
    // Placed above the theme picker so the small set of high-traffic
    // toggles isn't buried under the (potentially long) theme list.
    // Mode hosts the palette-mode switch; Display groups the chrome-shape
    // and slot-list fade knobs that don't belong with palette selection.
    const MODE_ICON: &str = "assets/icons/monitor.svg";
    const DISPLAY_ICON: &str = "assets/icons/layout-grid.svg";
    e.push(SettingsEntry::Header {
        label: "Mode",
        icon: MODE_ICON,
    });
    let theme_val = if is_light_mode { "Light" } else { "Dark" };
    e.push(SettingItem::enum_val(
        SettingMeta::new("general.light_mode", "Theme Mode", "Mode")
            .with_subtitle("Switch between dark and light"),
        theme_val,
        "Dark",
        vec!["Dark", "Light"],
    ));

    e.push(SettingsEntry::Header {
        label: "Display",
        icon: DISPLAY_ICON,
    });
    e.push(SettingItem::enum_val(
        SettingMeta::new("general.rounded_mode", "Rounded Corners", "Display")
            .with_subtitle("Apply rounded borders to UI elements"),
        rounded_mode.as_label(),
        RoundedMode::default().as_label(),
        vec![
            RoundedMode::Off.as_label(),
            RoundedMode::On.as_label(),
            RoundedMode::PlayerOnly.as_label(),
        ],
    ));
    e.push(SettingItem::bool_val(
        SettingMeta::new("general.opacity_gradient", "Opacity Gradient", "Display")
            .with_subtitle("Fade non-center slots in list views"),
        opacity_gradient,
        true,
    ));

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

    // Theme selection now lives in a modal swatch picker (each row painted in
    // its own palette) rather than a long inline list. This single opener row
    // launches it; the current theme name is shown as the row value.
    e.push(
        SettingItem::text(
            SettingMeta::new("__theme_picker", "Browse Themes…", "Select Theme")
                .with_subtitle("Preview and pick a theme"),
            &theme.name,
            &theme.name,
        )
        .with_enter_hint()
        .with_activate(ActivateKind::ThemePicker),
    );

    // ── Background Colors ────────────────────────────────────────────
    push_color_section!(
        e,
        palette_prefix,
        palette_label,
        palette,
        default_palette,
        background,
        P,
        "Background Colors",
        SentinelKind::RestoreBg,
        "BG",
        [hard, default, soft, level1, level2, level3]
    );

    // ── Foreground Colors ────────────────────────────────────────────
    push_color_section!(
        e,
        palette_prefix,
        palette_label,
        palette,
        default_palette,
        foreground,
        P,
        "Foreground Colors",
        SentinelKind::RestoreFg,
        "FG",
        [bright, level1, level2, level3, level4]
    );

    // ── Accent Colors ────────────────────────────────────────────────
    push_color_section!(
        e,
        palette_prefix,
        palette_label,
        palette,
        default_palette,
        accent,
        P,
        "Accent Colors",
        SentinelKind::RestoreAccent,
        "Accent",
        // `now_playing` / `selected` are intentionally absent: the slot
        // highlights are now derived from these accent tokens with contrast
        // guards (see `theme::playing_fill` / `selected_fill_resolved`), so
        // hand-tuning them did nothing. The TOML fields still round-trip.
        [primary, bright, border_light]
    );

    // ── Semantic Colors ──────────────────────────────────────────────
    push_semantic_color_section!(
        e,
        palette_prefix,
        palette_label,
        palette,
        default_palette,
        P,
        "Semantic Colors",
        SentinelKind::RestoreSemantic,
        [
            (danger, "Danger"),
            (success, "Success"),
            (warning, "Warning"),
            (star, "Star"),
        ]
    );

    // ── Chrome Border ────────────────────────────────────────────────
    // Single-field section: the 1 px `theme::border()` hairline used between
    // nav bars, list rows, capsules, etc. Authored per-theme; when left
    // empty the runtime derives a value from `bg0_hard` (see
    // `ResolvedTheme::from_theme_palette`).
    e.push(SettingsEntry::Header {
        label: "Chrome Border",
        icon: P,
    });
    e.push(
        SettingItem::text(
            SettingMeta::new(
                SentinelKind::RestoreBorder.to_key(),
                "⟲ Restore Defaults",
                "Chrome Border",
            ),
            "Press Enter",
            "Press Enter",
        )
        .with_theme_key(),
    );
    {
        let key = format!("{palette_prefix}.border");
        let label = format!("Chrome border ({palette_label})");
        e.push(
            SettingItem::hex_color(
                SettingMeta::new(key, &label, "Chrome Border"),
                &palette.border,
                &default_palette.border,
            )
            .with_theme_key(),
        );
    }

    e
}

#[cfg(test)]
mod tests {
    use nokkvi_data::types::theme_file::ThemeFile;

    use super::{
        super::test_support::{extract_keys, palette_prefix_from, section_slice},
        *,
    };

    #[test]
    fn push_color_section_emits_header_restore_and_field_rows() {
        let theme = ThemeFile::default();
        let entries = build_theme_items(&theme, "everforest", RoundedMode::Off, true, false);
        let prefix = palette_prefix_from(&entries);
        let section = section_slice(&entries, "Background Colors");

        // Header
        match &section[0] {
            SettingsEntry::Header { label, .. } => assert_eq!(*label, "Background Colors"),
            SettingsEntry::Item(_) => panic!("expected header at index 0"),
        }
        // Restore sentinel row
        match &section[1] {
            SettingsEntry::Item(it) => {
                assert_eq!(it.key.as_ref(), SentinelKind::RestoreBg.to_key().as_str());
                assert!(it.is_theme_key);
            }
            SettingsEntry::Header { .. } => panic!("expected sentinel row at index 1"),
        }
        // 6 hex-color field rows follow, in the macro-declared order
        let expected: Vec<String> = [
            "background.hard",
            "background.default",
            "background.soft",
            "background.level1",
            "background.level2",
            "background.level3",
        ]
        .iter()
        .map(|suffix| format!("{prefix}.{suffix}"))
        .collect();
        let expected_refs: Vec<&str> = expected.iter().map(String::as_str).collect();
        let keys = extract_keys(&section[2..]);
        assert_eq!(keys, expected_refs);
        assert_eq!(section.len(), 1 + 1 + expected.len());
    }

    #[test]
    fn push_color_section_foreground_and_accent_rows() {
        let theme = ThemeFile::default();
        let entries = build_theme_items(&theme, "everforest", RoundedMode::Off, true, false);
        let prefix = palette_prefix_from(&entries);

        let fg = section_slice(&entries, "Foreground Colors");
        // `foreground.level4` IS consumed (resolved as fg4) and must stay in
        // the editor — only the dead `foreground.gray` row was removed.
        let expected_fg: Vec<String> = [
            "foreground.bright",
            "foreground.level1",
            "foreground.level2",
            "foreground.level3",
            "foreground.level4",
        ]
        .iter()
        .map(|s| format!("{prefix}.{s}"))
        .collect();
        let expected_fg_refs: Vec<&str> = expected_fg.iter().map(String::as_str).collect();
        let fg_keys = extract_keys(&fg[2..]);
        assert_eq!(fg_keys, expected_fg_refs);
        assert!(
            fg_keys.contains(&format!("{prefix}.foreground.level4").as_str()),
            "the consumed foreground.level4 row must survive dead-field removal"
        );

        let accent = section_slice(&entries, "Accent Colors");
        let expected_accent: Vec<String> =
            ["accent.primary", "accent.bright", "accent.border_light"]
                .iter()
                .map(|s| format!("{prefix}.{s}"))
                .collect();
        let expected_accent_refs: Vec<&str> = expected_accent.iter().map(String::as_str).collect();
        assert_eq!(extract_keys(&accent[2..]), expected_accent_refs);
    }

    #[test]
    fn push_semantic_color_section_expands_each_emotion_to_two_rows() {
        let theme = ThemeFile::default();
        let entries = build_theme_items(&theme, "everforest", RoundedMode::Off, true, false);
        let prefix = palette_prefix_from(&entries);
        let section = section_slice(&entries, "Semantic Colors");

        // Header + restore sentinel + 8 emotion rows (4 emotions × 2 rows)
        assert_eq!(section.len(), 2 + 8);

        match &section[1] {
            SettingsEntry::Item(it) => {
                assert_eq!(
                    it.key.as_ref(),
                    SentinelKind::RestoreSemantic.to_key().as_str()
                );
            }
            SettingsEntry::Header { .. } => panic!("expected sentinel row at index 1"),
        }

        let expected: Vec<String> = [
            "danger.base",
            "danger.bright",
            "success.base",
            "success.bright",
            "warning.base",
            "warning.bright",
            "star.base",
            "star.bright",
        ]
        .iter()
        .map(|s| format!("{prefix}.{s}"))
        .collect();
        let expected_refs: Vec<&str> = expected.iter().map(String::as_str).collect();
        assert_eq!(extract_keys(&section[2..]), expected_refs);
    }
}
