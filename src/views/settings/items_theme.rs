//! Theme tab setting entries
//!
//! Builds the settings UI for the Theme tab. Theme COLORS are edited directly
//! in each theme's TOML file at `~/.config/nokkvi/themes/`, not in the GUI — the
//! tab exposes only the dark/light mode switch, the display knobs, and the theme
//! picker. Creating a custom theme is the same file-based flow (drop a `.toml`
//! into that directory).

use nokkvi_data::types::{player_settings::RoundedMode, theme_file::ThemeFile};

use super::items::{ActivateKind, SettingItem, SettingMeta, SettingsEntry};

/// Build settings entries for the Theme tab: the dark/light mode switch, the
/// chrome-shape + slot-fade display knobs, and the theme-picker opener. Per-color
/// editing was removed — colors are authored in the theme TOML file.
pub(crate) fn build_theme_items(
    theme: &ThemeFile,
    rounded_mode: RoundedMode,
    opacity_gradient: bool,
    is_light_mode: bool,
) -> Vec<SettingsEntry> {
    const MODE_ICON: &str = "assets/icons/monitor.svg";
    const DISPLAY_ICON: &str = "assets/icons/layout-grid.svg";
    const PR: &str = "assets/icons/swatch-book.svg";

    let mut e = Vec::new();

    // ── Mode ─────────────────────────────────────────────────────────
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

    // ── Display ──────────────────────────────────────────────────────
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

    // ── Select Theme ─────────────────────────────────────────────────
    // Colors live in each theme's TOML file; the GUI only switches between
    // themes. The opener launches the modal swatch picker (its value shows the
    // current theme name).
    e.push(SettingsEntry::Header {
        label: "Select Theme",
        icon: PR,
    });
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

    e
}
