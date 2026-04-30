//! Settings item definitions — maps config structs to slot-list-navigable items
//!
//! Each `SettingItem` represents a single configurable value with its current value,
//! default, valid range/options, display label, and category grouping.
//! Builder functions produce `Vec<SettingItem>` per settings tab from live config state.

use std::borrow::Cow;

/// Determine the number of meaningful decimal places in a step value.
/// e.g. 0.1 → 1, 0.01 → 2, 0.005 → 3
fn decimal_places(step: f64) -> usize {
    let s = format!("{step}");
    s.find('.')
        .map_or(0, |dot| s[dot + 1..].trim_end_matches('0').len().max(1))
}

// ============================================================================
// Setting Value Types
// ============================================================================

/// A typed setting value with metadata for rendering and editing
#[derive(Debug, Clone)]
pub(crate) enum SettingValue {
    /// Floating-point with range and step
    Float {
        val: f64,
        min: f64,
        max: f64,
        step: f64,
        unit: &'static str,
    },
    /// Integer with range and step
    Int {
        val: i64,
        min: i64,
        max: i64,
        step: i64,
        unit: &'static str,
    },
    /// Boolean toggle
    Bool(bool),
    /// Enum-like string with a fixed set of options
    Enum {
        val: String,
        options: Vec<&'static str>,
    },
    /// Hex color string (e.g. "#458588")
    HexColor(String),
    /// Array of hex color strings (gradient) — opens sub-list in Phase 4
    ColorArray(Vec<String>),
    /// Read-only text (for display only, e.g. server URL)
    Text(String),
    /// Hotkey binding display (key combo string, read-only in Phase 3)
    Hotkey(String),
    /// Multi-select toggle badges — each badge independently toggleable.
    /// Vec of (display_label, setting_key, enabled).
    ToggleSet(Vec<(String, String, bool)>),
}

impl SettingValue {
    /// Human-readable display of the current value
    pub(crate) fn display(&self) -> String {
        match self {
            SettingValue::Float {
                val, unit, step, ..
            } => {
                if *unit == "%" {
                    format!("{:.0}{}", val * 100.0, unit)
                } else {
                    // Derive precision from step (e.g. step=0.005 → 3 decimals)
                    let precision = decimal_places(*step);
                    format!(
                        "{:.prec$}{}",
                        val,
                        if unit.is_empty() { "" } else { unit },
                        prec = precision
                    )
                }
            }
            SettingValue::Int { val, unit, .. } => {
                format!("{}{}", val, if unit.is_empty() { "" } else { unit })
            }
            SettingValue::Bool(v) => {
                if *v {
                    "On".to_string()
                } else {
                    "Off".to_string()
                }
            }
            SettingValue::Enum { val, .. } => val.clone(),
            SettingValue::HexColor(hex) => hex.clone(),
            SettingValue::ColorArray(colors) => format!("{} colors", colors.len()),
            SettingValue::Text(t) => t.clone(),
            SettingValue::Hotkey(combo) => combo.clone(),
            SettingValue::ToggleSet(items) => {
                let enabled: Vec<_> = items
                    .iter()
                    .filter(|(_, _, on)| *on)
                    .map(|(label, _, _)| label.as_str())
                    .collect();
                if enabled.is_empty() {
                    "None".to_string()
                } else {
                    enabled.join(", ")
                }
            }
        }
    }

    /// Increment the value (Right arrow in edit mode).
    /// Returns a new SettingValue with the incremented value, or None if not editable.
    pub(crate) fn increment(&self) -> Option<SettingValue> {
        match self {
            SettingValue::Float {
                val,
                min,
                max,
                step,
                unit,
            } => {
                let new_val = (val + step).min(*max);
                Some(SettingValue::Float {
                    val: new_val,
                    min: *min,
                    max: *max,
                    step: *step,
                    unit,
                })
            }
            SettingValue::Int {
                val,
                min,
                max,
                step,
                unit,
            } => {
                let new_val = (val + step).min(*max);
                Some(SettingValue::Int {
                    val: new_val,
                    min: *min,
                    max: *max,
                    step: *step,
                    unit,
                })
            }
            SettingValue::Bool(v) => Some(SettingValue::Bool(!v)),
            SettingValue::Enum { val, options } => {
                if options.is_empty() {
                    return None;
                }
                let current_idx = options.iter().position(|o| o == val).unwrap_or(0);
                let next_idx = (current_idx + 1) % options.len();
                Some(SettingValue::Enum {
                    val: options[next_idx].to_string(),
                    options: options.clone(),
                })
            }
            // HexColor, ColorArray, Text — not incrementable via arrow keys
            _ => None,
        }
    }

    /// Decrement the value (Left arrow in edit mode).
    /// Returns a new SettingValue with the decremented value, or None if not editable.
    pub(crate) fn decrement(&self) -> Option<SettingValue> {
        match self {
            SettingValue::Float {
                val,
                min,
                max,
                step,
                unit,
            } => {
                let new_val = (val - step).max(*min);
                Some(SettingValue::Float {
                    val: new_val,
                    min: *min,
                    max: *max,
                    step: *step,
                    unit,
                })
            }
            SettingValue::Int {
                val,
                min,
                max,
                step,
                unit,
            } => {
                let new_val = (val - step).max(*min);
                Some(SettingValue::Int {
                    val: new_val,
                    min: *min,
                    max: *max,
                    step: *step,
                    unit,
                })
            }
            SettingValue::Bool(v) => Some(SettingValue::Bool(!v)),
            SettingValue::Enum { val, options } => {
                if options.is_empty() {
                    return None;
                }
                let current_idx = options.iter().position(|o| o == val).unwrap_or(0);
                let prev_idx = if current_idx == 0 {
                    options.len() - 1
                } else {
                    current_idx - 1
                };
                Some(SettingValue::Enum {
                    val: options[prev_idx].to_string(),
                    options: options.clone(),
                })
            }
            _ => None,
        }
    }

    /// Whether this value type supports inline increment/decrement editing
    pub(crate) fn is_editable(&self) -> bool {
        matches!(
            self,
            SettingValue::Float { .. }
                | SettingValue::Int { .. }
                | SettingValue::Bool(_)
                | SettingValue::Enum { .. }
        )
    }

    /// Whether this value type is a numeric step value (Int/Float) that benefits
    /// from showing chevron arrow hints. Bool and Enum use SM-style clickable
    /// option buttons instead, so arrows would be redundant.
    pub(crate) fn is_incrementable(&self) -> bool {
        matches!(self, SettingValue::Float { .. } | SettingValue::Int { .. })
    }

    /// Parse a new value from a string representation.
    /// Used by `EditSetValue` for direct badge clicks (bool On/Off, enum option selection).
    pub(crate) fn parse_from_str(&self, s: &str) -> Option<SettingValue> {
        match self {
            SettingValue::Bool(_) => match s {
                "true" | "On" => Some(SettingValue::Bool(true)),
                "false" | "Off" => Some(SettingValue::Bool(false)),
                _ => None,
            },
            SettingValue::Enum { options, .. } => {
                if options.contains(&s) {
                    Some(SettingValue::Enum {
                        val: s.to_string(),
                        options: options.clone(),
                    })
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

// ============================================================================
// Setting Item
// ============================================================================

/// Common metadata shared by all setting items (key, label, category, icon).
/// Extracted to reduce argument count in builder methods.
#[derive(Debug, Clone)]
pub(crate) struct SettingMeta<'a> {
    pub key: Cow<'static, str>,
    pub label: &'a str,
    pub category: &'static str,
    /// Optional subtitle override (displayed instead of `category` in the UI)
    pub subtitle: Option<&'static str>,
}

/// Shorthand for constructing `SettingMeta` inline.
/// Accepts both `&'static str` and `String` keys via `Into<Cow<'static, str>>`.
macro_rules! meta {
    ($key:expr, $label:expr, $cat:expr) => {
        $crate::views::settings::items::SettingMeta {
            key: std::borrow::Cow::from($key),
            label: $label,
            category: $cat,
            subtitle: None,
        }
    };
    ($key:expr, $label:expr, $cat:expr, $sub:expr) => {
        $crate::views::settings::items::SettingMeta {
            key: std::borrow::Cow::from($key),
            label: $label,
            category: $cat,
            subtitle: Some($sub),
        }
    };
}

/// A single navigable setting in the slot list
#[derive(Debug, Clone)]
pub(crate) struct SettingItem {
    /// TOML dotted key path (e.g. "visualizer.bars.border_width")
    pub key: Cow<'static, str>,
    /// Human-readable label
    pub label: String,
    /// Section/category header for grouping
    pub category: &'static str,
    /// Current value
    pub value: SettingValue,
    /// Default value (for reset-to-default)
    pub default: SettingValue,
    /// Optional inline SVG icon rendered next to the label
    pub label_icon: Option<&'static str>,
    /// Optional subtitle override (displayed instead of `category` in the UI)
    pub subtitle: Option<&'static str>,
}

impl SettingItem {
    /// Create a SettingItem from metadata and value/default pair
    pub(crate) fn from_meta(
        m: SettingMeta,
        value: SettingValue,
        default: SettingValue,
    ) -> SettingsEntry {
        SettingsEntry::Item(SettingItem {
            key: m.key,
            label: m.label.to_string(),
            category: m.category,
            value,
            default,
            label_icon: None,
            subtitle: m.subtitle,
        })
    }

    /// Build a float setting entry
    pub(crate) fn float(
        m: SettingMeta,
        val: f64,
        default: f64,
        min: f64,
        max: f64,
        step: f64,
        unit: &'static str,
    ) -> SettingsEntry {
        Self::from_meta(
            m,
            SettingValue::Float {
                val,
                min,
                max,
                step,
                unit,
            },
            SettingValue::Float {
                val: default,
                min,
                max,
                step,
                unit,
            },
        )
    }

    /// Build an integer setting entry
    pub(crate) fn int(
        m: SettingMeta,
        val: i64,
        default: i64,
        min: i64,
        max: i64,
        step: i64,
        unit: &'static str,
    ) -> SettingsEntry {
        Self::from_meta(
            m,
            SettingValue::Int {
                val,
                min,
                max,
                step,
                unit,
            },
            SettingValue::Int {
                val: default,
                min,
                max,
                step,
                unit,
            },
        )
    }

    /// Build a boolean setting entry
    pub(crate) fn bool_val(m: SettingMeta, val: bool, default: bool) -> SettingsEntry {
        Self::from_meta(m, SettingValue::Bool(val), SettingValue::Bool(default))
    }

    /// Build an enum setting entry
    pub(crate) fn enum_val(
        m: SettingMeta,
        val: &str,
        default: &str,
        options: Vec<&'static str>,
    ) -> SettingsEntry {
        Self::from_meta(
            m,
            SettingValue::Enum {
                val: val.to_string(),
                options: options.clone(),
            },
            SettingValue::Enum {
                val: default.to_string(),
                options,
            },
        )
    }

    /// Build a hex color setting entry
    pub(crate) fn hex_color(m: SettingMeta, val: &str, default: &str) -> SettingsEntry {
        Self::from_meta(
            m,
            SettingValue::HexColor(val.to_string()),
            SettingValue::HexColor(default.to_string()),
        )
    }

    /// Build a color array setting entry
    pub(crate) fn color_array(
        m: SettingMeta,
        val: Vec<String>,
        default: Vec<String>,
    ) -> SettingsEntry {
        Self::from_meta(
            m,
            SettingValue::ColorArray(val),
            SettingValue::ColorArray(default),
        )
    }

    /// Build a read-only text setting entry
    pub(crate) fn text(m: SettingMeta, val: &str, default: &str) -> SettingsEntry {
        Self::from_meta(
            m,
            SettingValue::Text(val.to_string()),
            SettingValue::Text(default.to_string()),
        )
    }

    /// Build a read-only text setting entry with an inline label icon.
    ///
    /// Equivalent to [`text()`] followed by setting `label_icon`, but without
    /// the verbose `let mut entry` / `if let` workaround at every call site.
    pub(crate) fn text_with_icon(
        m: SettingMeta,
        val: &str,
        default: &str,
        icon: &'static str,
    ) -> SettingsEntry {
        let mut entry = Self::text(m, val, default);
        if let SettingsEntry::Item(ref mut item) = entry {
            item.label_icon = Some(icon);
        }
        entry
    }

    /// Build a toggle-set setting entry (multi-select badges).
    /// Each item is (display_label, setting_key, enabled).
    /// All enabled by default.
    pub(crate) fn toggle_set(m: SettingMeta, items: Vec<(String, String, bool)>) -> SettingsEntry {
        let defaults: Vec<(String, String, bool)> = items
            .iter()
            .map(|(l, k, _)| (l.clone(), k.clone(), true))
            .collect();
        Self::from_meta(
            m,
            SettingValue::ToggleSet(items),
            SettingValue::ToggleSet(defaults),
        )
    }
}

/// A slot-list-renderable entry — either a real setting or a section header
#[derive(Debug, Clone)]
pub(crate) enum SettingsEntry {
    /// Category separator rendered as non-interactive slot
    Header {
        label: &'static str,
        icon: &'static str,
    },
    /// A real configurable setting
    Item(SettingItem),
}

impl SettingsEntry {
    /// Whether this entry is a section header (non-interactive separator).
    pub(crate) fn is_header(&self) -> bool {
        matches!(self, SettingsEntry::Header { .. })
    }
}

// ============================================================================
// Tab Builder Re-exports
// ============================================================================

// The `meta!` macro above is used by these sibling modules via `#[macro_use]`
// on the `mod items;` declaration in `settings/mod.rs`.

pub(crate) use super::{
    items_general::{GeneralSettingsData, build_general_items},
    items_hotkeys::{
        build_hotkeys_items, is_action_key, is_preset_key, is_restore_key, key_to_hotkey_action,
        preset_key_index,
    },
    items_interface::{InterfaceSettingsData, build_interface_items},
    items_playback::{PlaybackSettingsData, build_playback_items},
    items_theme::build_theme_items,
    items_visualizer::build_visualizer_items,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::visualizer_config::VisualizerConfig;

    /// Count real items (excluding headers) in a settings entry list
    fn count_items(entries: &[SettingsEntry]) -> usize {
        entries
            .iter()
            .filter(|e| matches!(e, SettingsEntry::Item(_)))
            .count()
    }

    /// Count headers in a settings entry list
    fn count_headers(entries: &[SettingsEntry]) -> usize {
        entries
            .iter()
            .filter(|e| matches!(e, SettingsEntry::Header { .. }))
            .count()
    }

    /// Extract all TOML key paths from settings entries
    fn extract_keys(entries: &[SettingsEntry]) -> Vec<&str> {
        entries
            .iter()
            .filter_map(|e| match e {
                SettingsEntry::Item(item) => Some(item.key.as_ref()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn visualizer_items_structure() {
        let config = VisualizerConfig::default();
        let theme = nokkvi_data::types::theme_file::ThemeFile::default();
        let entries = build_visualizer_items(&config, &theme, "adwaita");

        // Verify section headers
        assert_eq!(
            count_headers(&entries),
            5,
            "Expected 5 sections: General, Bars, Bar Colors (Dark), Bar Colors (Light), Lines"
        );

        // Verify total item count (catches drift when settings are added/removed)
        let item_count = count_items(&entries);
        assert_eq!(
            item_count, 45,
            "Expected 45 visualizer settings items (update this if adding settings)"
        );

        // Spot-check the border_opacity setting (now per-theme under dark/light)
        let keys = extract_keys(&entries);
        assert!(
            keys.contains(&"dark.visualizer.border_opacity"),
            "Missing dark border_opacity key"
        );

        // Verify key paths start with "visualizer." (skip __ sentinel keys)
        let keys = extract_keys(&entries);
        for key in &keys {
            if key.starts_with("__") {
                continue;
            }
            assert!(
                key.starts_with("visualizer.")
                    || key.starts_with("dark.")
                    || key.starts_with("light."),
                "All visualizer keys should start with 'visualizer.', 'dark.', or 'light.', got: {key}"
            );
        }
    }

    #[test]
    fn visualizer_items_key_paths() {
        let config = VisualizerConfig::default();
        let theme = nokkvi_data::types::theme_file::ThemeFile::default();
        let entries = build_visualizer_items(&config, &theme, "adwaita");
        let keys = extract_keys(&entries);

        // Spot-check critical key paths that config_writer depends on
        assert!(
            keys.contains(&"visualizer.noise_reduction"),
            "Missing noise_reduction key"
        );
        assert!(
            keys.contains(&"visualizer.bars.bar_spacing"),
            "Missing bar_spacing key"
        );
        assert!(
            keys.contains(&"visualizer.bars.gradient_mode"),
            "Missing gradient_mode key"
        );
        assert!(
            keys.contains(&"visualizer.bars.peak_mode"),
            "Missing peak_mode key"
        );
        assert!(
            keys.contains(&"visualizer.auto_sensitivity"),
            "Missing auto_sensitivity key"
        );
        assert!(
            keys.contains(&"visualizer.bars.bar_depth_3d"),
            "Missing bar_depth_3d key"
        );
        assert!(
            keys.contains(&"dark.visualizer.bar_gradient_colors"),
            "Missing dark bar_gradient_colors key"
        );
        assert!(
            keys.contains(&"light.visualizer.bar_gradient_colors"),
            "Missing light bar_gradient_colors key"
        );
    }

    #[test]
    fn visualizer_items_value_types() {
        let config = VisualizerConfig::default();
        let theme = nokkvi_data::types::theme_file::ThemeFile::default();
        let entries = build_visualizer_items(&config, &theme, "adwaita");

        for entry in &entries {
            if let SettingsEntry::Item(item) = entry {
                match item.key.as_ref() {
                    // Float settings
                    "visualizer.noise_reduction" | "visualizer.monstercat" => {
                        assert!(
                            matches!(item.value, SettingValue::Float { .. }),
                            "Expected Float for {}, got {:?}",
                            item.key,
                            item.value
                        );
                    }
                    // Bool settings
                    "visualizer.waves"
                    | "visualizer.bars.led_bars"
                    | "visualizer.auto_sensitivity" => {
                        assert!(
                            matches!(item.value, SettingValue::Bool(_)),
                            "Expected Bool for {}, got {:?}",
                            item.key,
                            item.value
                        );
                    }
                    // Enum settings
                    "visualizer.bars.gradient_mode"
                    | "visualizer.bars.gradient_orientation"
                    | "visualizer.bars.peak_gradient_mode"
                    | "visualizer.bars.peak_mode" => {
                        assert!(
                            matches!(item.value, SettingValue::Enum { .. }),
                            "Expected Enum for {}, got {:?}",
                            item.key,
                            item.value
                        );
                    }
                    // Sentinel keys (restore buttons, etc.)
                    k if k.starts_with("__") => {}
                    // Color arrays
                    k if k.contains("gradient_colors") => {
                        assert!(
                            matches!(item.value, SettingValue::ColorArray(_)),
                            "Expected ColorArray for {}, got {:?}",
                            item.key,
                            item.value
                        );
                    }
                    _ => {} // Other items are fine
                }
            }
        }
    }

    #[test]
    fn visualizer_items_defaults_match_config() {
        let config = VisualizerConfig::default();
        let theme = nokkvi_data::types::theme_file::ThemeFile::default();
        let entries = build_visualizer_items(&config, &theme, "adwaita");

        // When built from defaults, value should equal default for every item
        // (skip __ sentinel keys — they're action buttons, not config values)
        for entry in &entries {
            if let SettingsEntry::Item(item) = entry {
                if item.key.starts_with("__") {
                    continue;
                }
                let val_display = item.value.display();
                let default_display = item.default.display();
                assert_eq!(
                    val_display, default_display,
                    "Setting '{}' value ({}) != default ({}) when built from VisualizerConfig::default()",
                    item.key, val_display, default_display
                );
            }
        }
    }

    #[test]
    fn theme_items_structure() {
        let theme = nokkvi_data::types::theme_file::ThemeFile::default();
        let entries = build_theme_items(&theme, "adwaita", false, true, false);

        // Verify section headers
        assert_eq!(
            count_headers(&entries),
            6,
            "Expected 6 theme sections: Select Theme, Appearance, Background Colors, Foreground Colors, Accent Colors, Semantic Colors"
        );

        // Verify we have a reasonable number of items
        let item_count = count_items(&entries);
        assert!(
            item_count >= 23,
            "Expected at least 23 theme items (7 bg + 6 fg + 6 accent + 8 semantic + themes/rounded), got {item_count}"
        );
    }

    #[test]
    fn general_items_structure() {
        let data = GeneralSettingsData {
            server_url: "http://localhost:4533",
            username: "admin",
            start_view: "Queue",
            stable_viewport: true,
            auto_follow_playing: true,
            enter_behavior: "Play All",
            local_music_path: "",
            verbose_config: false,
            library_page_size: "Default (500)",
            artwork_resolution: "Default (1000px)",
            show_album_artists_only: true,
            suppress_library_refresh_toasts: false,
            show_tray_icon: false,
            close_to_tray: false,
        };
        let entries = build_general_items(&data);

        assert_eq!(
            count_headers(&entries),
            4,
            "Expected 4 sections: Application, Mouse Behavior, System Tray, Account"
        );
        assert_eq!(count_items(&entries), 15, "Expected 15 items");
    }

    #[test]
    fn interface_items_structure() {
        use super::super::items_interface::{InterfaceSettingsData, build_interface_items};
        let data = InterfaceSettingsData {
            nav_layout: "Top",
            nav_display_mode: "Text Only",
            track_info_display: "Off",
            slot_row_height: "Default",
            horizontal_volume: false,
            slot_text_links: true,
            font_family: "",
            strip_show_title: true,
            strip_show_artist: true,
            strip_show_album: true,
            strip_show_format_info: true,
            strip_merged_mode: false,
            strip_click_action: "Go to Queue",
            albums_artwork_overlay: true,
            artists_artwork_overlay: true,
            songs_artwork_overlay: true,
            playlists_artwork_overlay: true,
            artwork_column_mode: "Auto",
            artwork_column_stretch_fit: "Cover",
        };
        let entries = build_interface_items(&data);

        assert_eq!(
            count_headers(&entries),
            5,
            "Expected 5 sections: Layout, Views, Font, Metadata Strip, Artwork Column"
        );
        assert_eq!(
            count_items(&entries),
            12,
            "Expected 12 items (... + artwork_column_mode); stretched mode adds the fit knob"
        );
    }

    #[test]
    fn interface_items_artwork_column_stretched_adds_fit_knob() {
        use super::super::items_interface::{InterfaceSettingsData, build_interface_items};
        let data = InterfaceSettingsData {
            nav_layout: "Top",
            nav_display_mode: "Text Only",
            track_info_display: "Off",
            slot_row_height: "Default",
            horizontal_volume: false,
            slot_text_links: true,
            font_family: "",
            strip_show_title: true,
            strip_show_artist: true,
            strip_show_album: true,
            strip_show_format_info: true,
            strip_merged_mode: false,
            strip_click_action: "Go to Queue",
            albums_artwork_overlay: true,
            artists_artwork_overlay: true,
            songs_artwork_overlay: true,
            playlists_artwork_overlay: true,
            artwork_column_mode: "Always (Stretched)",
            artwork_column_stretch_fit: "Cover",
        };
        let entries = build_interface_items(&data);
        assert_eq!(count_items(&entries), 13);
    }

    #[test]
    fn playback_items_structure_off_mode() {
        use super::super::items_playback::{PlaybackSettingsData, build_playback_items};
        let data = PlaybackSettingsData {
            crossfade_enabled: false,
            crossfade_duration_secs: 5,
            volume_normalization: "Off",
            normalization_level: "Normal",
            replay_gain_preamp_db: 0,
            replay_gain_fallback_db: 0,
            replay_gain_fallback_to_agc: false,
            replay_gain_prevent_clipping: true,
            scrobbling_enabled: true,
            scrobble_threshold: 0.50,
            quick_add_to_playlist: false,
            default_playlist_name: "",
            queue_show_default_playlist: false,
        };
        let entries = build_playback_items(&data);

        assert_eq!(
            count_headers(&entries),
            3,
            "Expected 3 sections: Playback, Scrobbling, Playlists"
        );
        // Off mode hides AGC level + RG knobs.
        assert_eq!(
            count_items(&entries),
            8,
            "Off mode: crossfade_enabled, crossfade_duration, volume_normalization, scrobbling_enabled, scrobble_threshold, quick_add_to_playlist, default_playlist_name, queue_show_default_playlist"
        );
    }

    #[test]
    fn playback_items_structure_agc_mode_shows_target_level() {
        use super::super::items_playback::{PlaybackSettingsData, build_playback_items};
        let data = PlaybackSettingsData {
            crossfade_enabled: false,
            crossfade_duration_secs: 5,
            volume_normalization: "AGC",
            normalization_level: "Normal",
            replay_gain_preamp_db: 0,
            replay_gain_fallback_db: 0,
            replay_gain_fallback_to_agc: false,
            replay_gain_prevent_clipping: true,
            scrobbling_enabled: true,
            scrobble_threshold: 0.50,
            quick_add_to_playlist: false,
            default_playlist_name: "",
            queue_show_default_playlist: false,
        };
        let entries = build_playback_items(&data);
        // AGC mode adds the target-level dropdown.
        assert_eq!(count_items(&entries), 9);
    }

    #[test]
    fn playback_items_structure_replay_gain_mode_shows_rg_knobs() {
        use super::super::items_playback::{PlaybackSettingsData, build_playback_items};
        let data = PlaybackSettingsData {
            crossfade_enabled: false,
            crossfade_duration_secs: 5,
            volume_normalization: "ReplayGain (Track)",
            normalization_level: "Normal",
            replay_gain_preamp_db: 0,
            replay_gain_fallback_db: 0,
            replay_gain_fallback_to_agc: false,
            replay_gain_prevent_clipping: true,
            scrobbling_enabled: true,
            scrobble_threshold: 0.50,
            quick_add_to_playlist: false,
            default_playlist_name: "",
            queue_show_default_playlist: false,
        };
        let entries = build_playback_items(&data);
        // RG modes add 4 knobs: preamp, fallback_db, fallback_to_agc, prevent_clipping.
        assert_eq!(count_items(&entries), 12);
    }

    #[test]
    fn setting_value_increment_decrement_roundtrip() {
        // Float
        let float_val = SettingValue::Float {
            val: 0.5,
            min: 0.0,
            max: 1.0,
            step: 0.1,
            unit: "",
        };
        let incremented = float_val.increment().unwrap();
        if let SettingValue::Float { val, .. } = incremented {
            assert!((val - 0.6).abs() < 1e-10, "Float increment failed");
        }

        // Int
        let int_val = SettingValue::Int {
            val: 5,
            min: 0,
            max: 10,
            step: 1,
            unit: "",
        };
        let decremented = int_val.decrement().unwrap();
        if let SettingValue::Int { val, .. } = decremented {
            assert_eq!(val, 4, "Int decrement failed");
        }

        // Bool
        let bool_val = SettingValue::Bool(false);
        let toggled = bool_val.increment().unwrap();
        assert!(matches!(toggled, SettingValue::Bool(true)));

        // Enum
        let enum_val = SettingValue::Enum {
            val: "a".to_string(),
            options: vec!["a", "b", "c"],
        };
        let next = enum_val.increment().unwrap();
        if let SettingValue::Enum { val, .. } = next {
            assert_eq!(val, "b");
        }

        // Enum wraps around
        let enum_last = SettingValue::Enum {
            val: "c".to_string(),
            options: vec!["a", "b", "c"],
        };
        let wrapped = enum_last.increment().unwrap();
        if let SettingValue::Enum { val, .. } = wrapped {
            assert_eq!(val, "a", "Enum should wrap around");
        }
    }

    #[test]
    fn setting_value_boundary_clamp() {
        // Float at max stays at max
        let at_max = SettingValue::Float {
            val: 1.0,
            min: 0.0,
            max: 1.0,
            step: 0.1,
            unit: "",
        };
        if let SettingValue::Float { val, .. } = at_max.increment().unwrap() {
            assert!((val - 1.0).abs() < 1e-10, "Should clamp at max");
        }

        // Int at min stays at min
        let at_min = SettingValue::Int {
            val: 0,
            min: 0,
            max: 10,
            step: 1,
            unit: "",
        };
        if let SettingValue::Int { val, .. } = at_min.decrement().unwrap() {
            assert_eq!(val, 0, "Should clamp at min");
        }
    }

    #[test]
    fn non_editable_values_return_none() {
        let hex = SettingValue::HexColor("#ff0000".to_string());
        assert!(hex.increment().is_none());
        assert!(hex.decrement().is_none());
        assert!(!hex.is_editable());

        let text = SettingValue::Text("hello".to_string());
        assert!(text.increment().is_none());
        assert!(!text.is_editable());

        let colors = SettingValue::ColorArray(vec!["#ff0000".to_string()]);
        assert!(colors.increment().is_none());
        assert!(!colors.is_editable());
    }

    #[test]
    fn settings_descriptions_fit_in_footer() {
        let general = crate::views::settings::items_general::GeneralSettingsData {
            server_url: "http://localhost:4533",
            username: "admin",
            start_view: "Queue",
            stable_viewport: true,
            auto_follow_playing: true,
            enter_behavior: "Play All",
            local_music_path: "",
            verbose_config: false,
            library_page_size: "Default (500)",
            artwork_resolution: "Default (1000px)",
            show_album_artists_only: true,
            suppress_library_refresh_toasts: false,
            show_tray_icon: false,
            close_to_tray: false,
        };
        let interface = crate::views::settings::items_interface::InterfaceSettingsData {
            nav_layout: "Top",
            nav_display_mode: "Text Only",
            track_info_display: "Off",
            slot_row_height: "Default",
            horizontal_volume: false,
            slot_text_links: true,
            font_family: "",
            strip_show_title: true,
            strip_show_artist: true,
            strip_show_album: true,
            strip_show_format_info: true,
            strip_merged_mode: false,
            strip_click_action: "Go to Queue",
            albums_artwork_overlay: true,
            artists_artwork_overlay: true,
            songs_artwork_overlay: true,
            playlists_artwork_overlay: true,
            artwork_column_mode: "Auto",
            artwork_column_stretch_fit: "Cover",
        };
        let playback = crate::views::settings::items_playback::PlaybackSettingsData {
            crossfade_enabled: false,
            crossfade_duration_secs: 5,
            volume_normalization: "Off",
            normalization_level: "Normal",
            replay_gain_preamp_db: 0,
            replay_gain_fallback_db: 0,
            replay_gain_fallback_to_agc: false,
            replay_gain_prevent_clipping: true,
            scrobbling_enabled: true,
            scrobble_threshold: 0.50,
            quick_add_to_playlist: false,
            default_playlist_name: "",
            queue_show_default_playlist: false,
        };
        let hotkeys = nokkvi_data::types::hotkey_config::HotkeyConfig::default();
        let theme = nokkvi_data::types::theme_file::ThemeFile::default();
        let visualizer = crate::visualizer_config::VisualizerConfig::default();

        let mut all_entries = Vec::new();
        all_entries.extend(crate::views::settings::items_general::build_general_items(
            &general,
        ));
        all_entries
            .extend(crate::views::settings::items_interface::build_interface_items(&interface));
        all_entries.extend(crate::views::settings::items_playback::build_playback_items(&playback));
        all_entries.extend(crate::views::settings::items_hotkeys::build_hotkeys_items(
            &hotkeys,
        ));
        all_entries.extend(crate::views::settings::items_theme::build_theme_items(
            &theme, "adwaita", false, true, false,
        ));
        all_entries.extend(
            crate::views::settings::items_visualizer::build_visualizer_items(
                &visualizer,
                &theme,
                "adwaita",
            ),
        );

        for entry in all_entries {
            if let SettingsEntry::Item(item) = entry
                && let Some(subtitle) = item.subtitle
            {
                let newlines = subtitle.chars().filter(|c| *c == '\n').count();
                assert!(
                    newlines <= 4,
                    "Description for '{}' has {} newlines, which exceeds the max of 4. This will overflow the footer.\nDescription:\n{}",
                    item.label,
                    newlines,
                    subtitle
                );
            }
        }
    }
}
