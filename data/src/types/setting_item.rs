//! Settings item definitions consumed by the slot-list-based settings UI.
//!
//! These types live in the data crate so the macro-emitted
//! `build_<tab>_tab_settings_items` helpers (also in the data crate, see
//! `define_settings!`) can return a `Vec<SettingsEntry>` directly. The UI
//! crate re-exports them via `src/views/settings/items.rs` so existing
//! `crate::views::settings::items::{SettingItem, SettingMeta, SettingsEntry}`
//! import paths continue to resolve.
//!
//! All fields are pure data — `Cow`, `String`, `&'static str`, plus
//! `SettingValue` (also iced-free). No iced types reach this module.

use std::borrow::Cow;

use crate::types::setting_value::SettingValue;

/// Common metadata shared by all setting items (key, label, category, icon).
/// Extracted to reduce argument count in builder methods.
///
/// Prefer the [`SettingMeta::new`] + [`SettingMeta::with_subtitle`] builder over
/// direct struct literals. `define_settings!` still constructs these inline in
/// its macro expansion (see `define_settings_build_item_arm!`).
#[derive(Debug, Clone)]
pub struct SettingMeta<'a> {
    pub key: Cow<'static, str>,
    pub label: &'a str,
    pub category: &'static str,
    /// Optional subtitle override (displayed instead of `category` in the UI).
    ///
    /// `Cow<'static, str>` accepts both `&'static str` literals (zero-cost
    /// `Cow::Borrowed`) and owned `String` values (e.g. the per-hotkey-action
    /// subtitle built via `format!`). This replaces the previous `Box::leak`
    /// dance the hotkeys builder used to satisfy the old `&'static str` bound.
    pub subtitle: Option<Cow<'static, str>>,
}

impl<'a> SettingMeta<'a> {
    /// Build a `SettingMeta` without a subtitle.
    ///
    /// Accepts both `&'static str` and `String` keys via
    /// `Into<Cow<'static, str>>` — `format!`-generated keys (e.g. theme color
    /// rows in `items_theme.rs`) work without an extra conversion.
    pub fn new<K: Into<Cow<'static, str>>>(key: K, label: &'a str, category: &'static str) -> Self {
        Self {
            key: key.into(),
            label,
            category,
            subtitle: None,
        }
    }

    /// Attach a subtitle (description text shown in the footer / search index).
    ///
    /// Accepts both `&'static str` and `String` via `Into<Cow<'static, str>>`.
    /// Owned strings (e.g. the per-hotkey-action subtitle) avoid the previous
    /// `Box::leak` allocation.
    #[must_use]
    pub fn with_subtitle<S: Into<Cow<'static, str>>>(mut self, subtitle: S) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }
}

/// A single navigable setting in the slot list.
#[derive(Debug, Clone)]
pub struct SettingItem {
    /// TOML dotted key path (e.g. "visualizer.bars.border_width").
    pub key: Cow<'static, str>,
    /// Human-readable label.
    pub label: String,
    /// Section/category header for grouping.
    pub category: &'static str,
    /// Current value.
    pub value: SettingValue,
    /// Default value (for reset-to-default).
    pub default: SettingValue,
    /// Optional inline SVG icon rendered next to the label.
    pub label_icon: Option<&'static str>,
    /// Optional subtitle override (displayed instead of `category` in the UI).
    pub subtitle: Option<Cow<'static, str>>,
    /// True when the key targets the active theme file (`dark.*` / `light.*`).
    /// False means the key targets config.toml.
    pub is_theme_key: bool,
    /// True when activating this row opens a dialog/picker/sub-list and the
    /// renderer should display an "Enter ↵" affordance. Always-interactive
    /// value types (Hotkey / HexColor / ColorArray) get the hint
    /// unconditionally; this flag is for `Text` rows whose activation behavior
    /// can't be inferred from the value type alone (font picker, local music
    /// path text input, default playlist picker).
    pub needs_enter_hint: bool,
}

impl SettingItem {
    /// Create a SettingItem from metadata and value/default pair.
    pub fn from_meta(m: SettingMeta, value: SettingValue, default: SettingValue) -> SettingsEntry {
        SettingsEntry::Item(SettingItem {
            key: m.key,
            label: m.label.to_string(),
            category: m.category,
            value,
            default,
            label_icon: None,
            subtitle: m.subtitle,
            is_theme_key: false,
            needs_enter_hint: false,
        })
    }

    /// Build a float setting entry.
    pub fn float(
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

    /// Build an integer setting entry.
    pub fn int(
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

    /// Build a boolean setting entry.
    pub fn bool_val(m: SettingMeta, val: bool, default: bool) -> SettingsEntry {
        Self::from_meta(m, SettingValue::Bool(val), SettingValue::Bool(default))
    }

    /// Build an enum setting entry.
    pub fn enum_val(
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

    /// Build a hex color setting entry.
    pub fn hex_color(m: SettingMeta, val: &str, default: &str) -> SettingsEntry {
        Self::from_meta(
            m,
            SettingValue::HexColor(val.to_string()),
            SettingValue::HexColor(default.to_string()),
        )
    }

    /// Build a color array setting entry.
    pub fn color_array(m: SettingMeta, val: Vec<String>, default: Vec<String>) -> SettingsEntry {
        Self::from_meta(
            m,
            SettingValue::ColorArray(val),
            SettingValue::ColorArray(default),
        )
    }

    /// Build a read-only text setting entry.
    pub fn text(m: SettingMeta, val: &str, default: &str) -> SettingsEntry {
        Self::from_meta(
            m,
            SettingValue::Text(val.to_string()),
            SettingValue::Text(default.to_string()),
        )
    }

    /// Build a read-only text setting entry with an inline label icon.
    ///
    /// Equivalent to [`Self::text`] followed by setting `label_icon`, but without
    /// the verbose `let mut entry` / `if let` workaround at every call site.
    pub fn text_with_icon(
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
    pub fn toggle_set(m: SettingMeta, items: Vec<(String, String, bool)>) -> SettingsEntry {
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

/// A slot-list-renderable entry — either a real setting or a section header.
#[derive(Debug, Clone)]
pub enum SettingsEntry {
    /// Category separator rendered as non-interactive slot.
    Header {
        label: &'static str,
        icon: &'static str,
    },
    /// A real configurable setting.
    Item(SettingItem),
}

impl SettingsEntry {
    /// Whether this entry is a section header (non-interactive separator).
    pub fn is_header(&self) -> bool {
        matches!(self, SettingsEntry::Header { .. })
    }

    /// Mark the item as targeting the active theme file.
    /// No-op on headers (they are never written to any config file).
    pub fn with_theme_key(mut self) -> Self {
        if let SettingsEntry::Item(ref mut item) = self {
            item.is_theme_key = true;
        }
        self
    }

    /// Mark the item as opening a dialog/picker on Enter, so the renderer
    /// shows the "Enter ↵" affordance. No-op on headers.
    ///
    /// Always-interactive value types (Hotkey / HexColor / ColorArray) get
    /// the hint unconditionally from the renderer — call this only for
    /// `Text` rows whose activation behavior isn't inferable from the value
    /// type alone (font picker, local-music-path text input, default
    /// playlist picker).
    pub fn with_enter_hint(mut self) -> Self {
        if let SettingsEntry::Item(ref mut item) = self {
            item.needs_enter_hint = true;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setting_meta_new_defaults_subtitle_to_none() {
        let meta = SettingMeta::new("general.start_view", "Start View", "Application");
        assert!(meta.subtitle.is_none());
        assert_eq!(meta.key, Cow::Borrowed("general.start_view"));
        assert_eq!(meta.label, "Start View");
        assert_eq!(meta.category, "Application");
    }

    #[test]
    fn setting_meta_with_subtitle_borrowed() {
        let meta =
            SettingMeta::new("general.start_view", "Start View", "Application").with_subtitle("s");
        let subtitle = meta.subtitle.expect("subtitle should be Some");
        assert!(
            matches!(&subtitle, Cow::Borrowed(s) if *s == "s"),
            "expected Cow::Borrowed(\"s\"), got {subtitle:?}"
        );
    }

    #[test]
    fn setting_meta_with_subtitle_owned() {
        let meta = SettingMeta::new("hotkey.toggle_play", "Toggle Play", "Playback")
            .with_subtitle(format!("dynamic {}", 42));
        let subtitle = meta.subtitle.as_ref().expect("subtitle should be Some");
        assert!(
            matches!(subtitle, Cow::Owned(_)),
            "expected Cow::Owned, got Cow::Borrowed"
        );
        assert_eq!(subtitle.as_ref(), "dynamic 42");
    }
}
