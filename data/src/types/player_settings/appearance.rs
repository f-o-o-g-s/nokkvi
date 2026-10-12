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

define_labeled_enum! {
    /// Which icon family the UI renders its glyphs from.
    ///
    /// `Phosphor` is the default — the Phosphor family (Regular weight, with
    /// the transport + rating glyphs forced to the Fill weight so the play
    /// button and rating stars still read solid). `Lucide` is the alternate:
    /// the thin stroked outlines on a 24px grid that the app originally
    /// shipped. The boat hull and rope are bespoke art (unaffected by this
    /// knob), but the doodad's anchor follows the set — filled Phosphor or
    /// stroked Lucide.
    ///
    /// Serializes to snake_case strings in `config.toml` and the redb-backed
    /// `PersistedPlayerSettings`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum IconSet {
        /// The original thin-outline alternate. The icons that ship under
        /// `assets/icons/`.
        Lucide { label: "Lucide", wire: "lucide" },
        /// Default. Phosphor glyphs (`assets/icons-phosphor/`), Regular weight +
        /// forced Fill for the filled transport/rating glyphs.
        #[default]
        Phosphor { label: "Phosphor", wire: "phosphor" },
    }
}

define_labeled_enum! {
    /// How the player bar draws its seek bar. One value is kept for the
    /// MiniPlayer strip mode and one for every other mode, so each bar starts
    /// at the look it shipped with ([`Self::default`] for the normal bar,
    /// [`Self::MINI_PLAYER_DEFAULT`] for the mini player).
    ///
    /// Serializes to snake_case strings in `config.toml` and the redb-backed
    /// `PersistedPlayerSettings`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum SeekbarStyle {
        /// Default for the normal bar. A 6 px track with a draggable 14 px
        /// handle; the times sit outside the bar.
        #[default]
        Classic { label: "Classic", wire: "classic" },
        /// A thin handle-less track that thickens and shows its handle under
        /// the cursor; the times sit outside the bar.
        Line { label: "Line", wire: "line" },
        /// A solid full-height block with the times drawn inside it, switching
        /// ink where the fill passes under them. The mini player's default.
        Capsule { label: "Capsule", wire: "capsule" },
        /// A row of discrete cells, like a level meter.
        Cells { label: "Cells", wire: "cells" },
        /// The track's peak envelope. Draws as [`Self::Line`] until the
        /// envelope has been worked out, and always for radio.
        Waveform { label: "Waveform", wire: "waveform" },
    }
}

impl SeekbarStyle {
    /// The mini player's starting style: the capsule it has always drawn.
    pub const MINI_PLAYER_DEFAULT: Self = Self::Capsule;

    /// Whether the elapsed / duration times are drawn inside the bar rather
    /// than beside it.
    pub fn times_inside(self) -> bool {
        matches!(self, Self::Capsule)
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

    #[test]
    fn icon_set_label_roundtrip_and_default() {
        for set in [IconSet::Lucide, IconSet::Phosphor] {
            assert_eq!(IconSet::from_label(set.as_label()), set);
        }
        // Phosphor is the shipped default; the ui_meta default literal and the
        // settings projection both depend on this.
        assert_eq!(IconSet::default(), IconSet::Phosphor);
        assert_eq!(IconSet::Phosphor.as_label(), "Phosphor");
    }

    #[test]
    fn seekbar_style_round_trips_and_keeps_both_shipped_looks() {
        for style in [
            SeekbarStyle::Classic,
            SeekbarStyle::Line,
            SeekbarStyle::Capsule,
            SeekbarStyle::Cells,
            SeekbarStyle::Waveform,
        ] {
            assert_eq!(SeekbarStyle::from_label(style.as_label()), style);
            let json = serde_json::to_string(&style).unwrap();
            assert_eq!(json.trim_matches('"'), style.to_string());
        }
        // The normal bar keeps its handle and the mini player its capsule, so
        // neither bar changes for an existing setup.
        assert_eq!(SeekbarStyle::default(), SeekbarStyle::Classic);
        assert_eq!(SeekbarStyle::MINI_PLAYER_DEFAULT, SeekbarStyle::Capsule);
        assert!(SeekbarStyle::Capsule.times_inside());
        assert!(!SeekbarStyle::Waveform.times_inside());
    }

    #[test]
    fn icon_set_serializes_to_snake_case_wire() {
        assert_eq!(
            serde_json::to_string(&IconSet::Lucide).unwrap(),
            "\"lucide\""
        );
        assert_eq!(
            serde_json::to_string(&IconSet::Phosphor).unwrap(),
            "\"phosphor\""
        );
    }
}
