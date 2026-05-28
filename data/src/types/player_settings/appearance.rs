//! Appearance / theming settings — global UI shape controls.

use serde::{Deserialize, Deserializer, Serialize};

use crate::define_labeled_enum;

define_labeled_enum! {
    /// Rounded corners mode — whether UI elements render with rounded borders.
    ///
    /// `On` rounds every UI surface (nav bar, cards, modals, transport). `Off`
    /// flattens everything. `PlayerOnly` keeps the bottom playback chrome
    /// (player bar, progress bar, volume slider, transport buttons, and the
    /// bottom track-info strip) rounded while the rest of the UI stays flat.
    ///
    /// Serializes to snake_case strings in `config.toml` and the redb-backed
    /// `PersistedPlayerSettings`. Legacy `true`/`false` bool values from the
    /// pre-enum two-state era load via
    /// [`deserialize_rounded_mode_with_bool_compat`]
    /// (`true` → `On`, `false` → `Off`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum RoundedMode {
        Off { label: "Off", wire: "off" },
        #[default]
        On { label: "On", wire: "on" },
        PlayerOnly { label: "Player Only", wire: "player_only" },
    }
}

/// Field-level shim used by `#[serde(deserialize_with = ...)]` on the
/// `rounded_mode` fields of [`PersistedPlayerSettings`] and [`TomlSettings`].
///
/// Accepts the new enum wire format (`"off"` / `"on"` / `"player_only"`) and
/// legacy bool values from pre-enum configs (`true` → `On`, `false` → `Off`)
/// in the same field, so upgrading does not stomp users' existing preference.
///
/// [`PersistedPlayerSettings`]: crate::types::settings::PersistedPlayerSettings
/// [`TomlSettings`]: crate::types::toml_settings::TomlSettings
pub fn deserialize_rounded_mode_with_bool_compat<'de, D>(
    deserializer: D,
) -> Result<RoundedMode, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Repr {
        Bool(bool),
        Mode(RoundedMode),
    }
    match Repr::deserialize(deserializer)? {
        Repr::Bool(true) => Ok(RoundedMode::On),
        Repr::Bool(false) => Ok(RoundedMode::Off),
        Repr::Mode(mode) => Ok(mode),
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[derive(Deserialize)]
    struct Wrapper {
        #[serde(deserialize_with = "deserialize_rounded_mode_with_bool_compat")]
        rounded_mode: RoundedMode,
    }

    #[test]
    fn legacy_bool_true_loads_as_on() {
        let w: Wrapper = serde_json::from_str(r#"{"rounded_mode": true}"#).unwrap();
        assert_eq!(w.rounded_mode, RoundedMode::On);
    }

    #[test]
    fn legacy_bool_false_loads_as_off() {
        let w: Wrapper = serde_json::from_str(r#"{"rounded_mode": false}"#).unwrap();
        assert_eq!(w.rounded_mode, RoundedMode::Off);
    }

    #[test]
    fn new_wire_strings_load_as_corresponding_variants() {
        for (wire, expected) in [
            ("off", RoundedMode::Off),
            ("on", RoundedMode::On),
            ("player_only", RoundedMode::PlayerOnly),
        ] {
            let json = format!(r#"{{"rounded_mode": "{wire}"}}"#);
            let w: Wrapper = serde_json::from_str(&json).unwrap();
            assert_eq!(w.rounded_mode, expected, "wire={wire}");
        }
    }

    #[test]
    fn serialized_wire_form_matches_label_roundtrip() {
        for mode in [RoundedMode::Off, RoundedMode::On, RoundedMode::PlayerOnly] {
            let wire = mode.to_string();
            assert_eq!(RoundedMode::from_label(mode.as_label()), mode);
            let json = serde_json::to_string(&mode).unwrap();
            assert_eq!(json.trim_matches('"'), wire);
        }
    }

    #[test]
    fn default_variant_is_on() {
        assert_eq!(RoundedMode::default(), RoundedMode::On);
    }

    #[test]
    fn from_label_unknown_falls_back_to_default() {
        assert_eq!(RoundedMode::from_label("Nonsense"), RoundedMode::On);
    }
}
