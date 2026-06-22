//! Verbose-config mode — controls how `config.toml` is written.

use serde::{Deserialize, Deserializer, Serialize};

use crate::define_labeled_enum;

define_labeled_enum! {
    /// How `config.toml` is (re)written when settings change.
    ///
    /// - `On` writes EVERY setting, including unchanged defaults — a fully
    ///   documented template.
    /// - `Off` writes only non-default keys (sparse) and adds a `# description`
    ///   comment above each `[visualizer]` key the GUI writes.
    /// - `Clean` writes only non-default keys (sparse) but adds NO comments —
    ///   for users who want a tidy file they annotate themselves.
    ///
    /// `On` and `Off` preserve the historical two-state behavior; legacy
    /// `true`/`false` bool values from the pre-enum era load via
    /// [`deserialize_verbose_config_with_bool_compat`] (`true` → `On`,
    /// `false` → `Off`). The default is `Off`.
    ///
    /// Serializes to snake_case strings in `config.toml` and the redb-backed
    /// `PersistedPlayerSettings`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum VerboseConfig {
        /// Write every setting, including unchanged defaults.
        On { label: "On", wire: "on" },
        /// Sparse — only non-default keys, with descriptive comments.
        #[default]
        Off { label: "Off", wire: "off" },
        /// Sparse — only non-default keys, no comments.
        Clean { label: "Clean", wire: "clean" },
    }
}

impl VerboseConfig {
    /// Whether the TOML writers should emit every key, including unchanged
    /// defaults (`On`), instead of pruning down to the non-default set
    /// (`Off`/`Clean`). This is the bool the section writers in
    /// `toml_settings_io` consume.
    pub fn writes_all_defaults(self) -> bool {
        matches!(self, Self::On)
    }

    /// Whether the surgical single-key writer should attach a `# description`
    /// comment above newly written `[visualizer]` keys. True for `On`/`Off`;
    /// false for `Clean`, which keeps the file comment-free.
    pub fn writes_comments(self) -> bool {
        !matches!(self, Self::Clean)
    }
}

/// Field-level shim used by `#[serde(deserialize_with = ...)]` on the
/// `verbose_config` fields of [`PersistedPlayerSettings`] and [`TomlSettings`].
///
/// Accepts the new enum wire format (`"on"` / `"off"` / `"clean"`) and legacy
/// bool values from pre-enum configs (`true` → `On`, `false` → `Off`) in the
/// same field, so upgrading does not stomp users' existing preference. Mirrors
/// [`deserialize_rounded_mode_with_bool_compat`].
///
/// [`PersistedPlayerSettings`]: crate::types::settings::PersistedPlayerSettings
/// [`TomlSettings`]: crate::types::toml_settings::TomlSettings
/// [`deserialize_rounded_mode_with_bool_compat`]: super::deserialize_rounded_mode_with_bool_compat
pub fn deserialize_verbose_config_with_bool_compat<'de, D>(
    deserializer: D,
) -> Result<VerboseConfig, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Repr {
        Bool(bool),
        Mode(VerboseConfig),
    }
    match Repr::deserialize(deserializer)? {
        Repr::Bool(true) => Ok(VerboseConfig::On),
        Repr::Bool(false) => Ok(VerboseConfig::Off),
        Repr::Mode(mode) => Ok(mode),
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[derive(Deserialize)]
    struct Wrapper {
        #[serde(deserialize_with = "deserialize_verbose_config_with_bool_compat")]
        verbose_config: VerboseConfig,
    }

    #[test]
    fn legacy_bool_true_loads_as_on() {
        let w: Wrapper = serde_json::from_str(r#"{"verbose_config": true}"#).unwrap();
        assert_eq!(w.verbose_config, VerboseConfig::On);
    }

    #[test]
    fn legacy_bool_false_loads_as_off() {
        let w: Wrapper = serde_json::from_str(r#"{"verbose_config": false}"#).unwrap();
        assert_eq!(w.verbose_config, VerboseConfig::Off);
    }

    #[test]
    fn new_wire_strings_load_as_corresponding_variants() {
        for (wire, expected) in [
            ("on", VerboseConfig::On),
            ("off", VerboseConfig::Off),
            ("clean", VerboseConfig::Clean),
        ] {
            let json = format!(r#"{{"verbose_config": "{wire}"}}"#);
            let w: Wrapper = serde_json::from_str(&json).unwrap();
            assert_eq!(w.verbose_config, expected, "wire={wire}");
        }
    }

    #[test]
    fn serialized_wire_form_matches_label_roundtrip() {
        for mode in [VerboseConfig::On, VerboseConfig::Off, VerboseConfig::Clean] {
            let wire = mode.to_string();
            assert_eq!(VerboseConfig::from_label(mode.as_label()), mode);
            let json = serde_json::to_string(&mode).unwrap();
            assert_eq!(json.trim_matches('"'), wire);
        }
    }

    #[test]
    fn default_variant_is_off() {
        assert_eq!(VerboseConfig::default(), VerboseConfig::Off);
    }

    #[test]
    fn from_label_unknown_falls_back_to_default() {
        assert_eq!(VerboseConfig::from_label("Nonsense"), VerboseConfig::Off);
    }

    #[test]
    fn writes_all_defaults_only_for_on() {
        assert!(VerboseConfig::On.writes_all_defaults());
        assert!(!VerboseConfig::Off.writes_all_defaults());
        assert!(!VerboseConfig::Clean.writes_all_defaults());
    }

    #[test]
    fn writes_comments_for_all_but_clean() {
        assert!(VerboseConfig::On.writes_comments());
        assert!(VerboseConfig::Off.writes_comments());
        assert!(!VerboseConfig::Clean.writes_comments());
    }
}
