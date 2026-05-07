//! `HotkeyConfig` — the persisted action → key-combo binding map.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{HotkeyAction, KeyCode, KeyCombo};

/// The full set of hotkey bindings, mapping actions to key combinations.
/// Serialized into redb via `SettingsManager`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyConfig {
    /// Action → KeyCombo mapping. Missing entries fall back to defaults.
    pub(super) bindings: HashMap<HotkeyAction, KeyCombo>,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        let bindings = HotkeyAction::ALL
            .iter()
            .chain(HotkeyAction::RESERVED.iter())
            .map(|action| (*action, action.default_binding()))
            .collect();
        Self { bindings }
    }
}

impl HotkeyConfig {
    /// Get the key combo for a given action (falls back to default if not customized).
    pub fn get_binding(&self, action: &HotkeyAction) -> KeyCombo {
        self.bindings
            .get(action)
            .cloned()
            .unwrap_or_else(|| action.default_binding())
    }

    /// Set or update the binding for an action.
    pub fn set_binding(&mut self, action: HotkeyAction, combo: KeyCombo) {
        self.bindings.insert(action, combo);
    }

    /// Reset a single action to its default binding.
    pub fn reset_binding(&mut self, action: &HotkeyAction) {
        self.bindings.insert(*action, action.default_binding());
    }

    /// Reset all bindings to defaults.
    pub fn reset_all(&mut self) {
        *self = Self::default();
    }

    /// Look up which action a key+modifiers combination is bound to.
    /// Returns `None` if no action matches.
    ///
    /// Searches both explicitly persisted bindings **and** default bindings
    /// for any actions not yet in the user's config (e.g. newly added actions).
    pub fn lookup(
        &self,
        key: &KeyCode,
        shift: bool,
        ctrl: bool,
        alt: bool,
    ) -> Option<HotkeyAction> {
        let combo = KeyCombo {
            key: key.clone(),
            shift,
            ctrl,
            alt,
        };
        // 1. Check explicitly persisted bindings first
        if let Some(found) = self
            .bindings
            .iter()
            .find(|(_, bound_combo)| **bound_combo == combo)
            .map(|(action, _)| *action)
        {
            return Some(found);
        }
        // 2. Fall back to default bindings for any actions not in the map
        //    (e.g. newly added actions that the user hasn't configured yet)
        for action in HotkeyAction::ALL
            .iter()
            .chain(HotkeyAction::RESERVED.iter())
        {
            if !self.bindings.contains_key(action) && action.default_binding() == combo {
                return Some(*action);
            }
        }
        None
    }

    /// Check if a key combo conflicts with an existing binding (excluding a given action).
    /// Returns the conflicting action, if any.
    pub fn find_conflict(&self, combo: &KeyCombo, exclude: &HotkeyAction) -> Option<HotkeyAction> {
        self.bindings
            .iter()
            .find(|(action, bound_combo)| *action != exclude && *bound_combo == combo)
            .map(|(action, _)| *action)
    }

    /// Get all bindings as an iterator.
    pub fn iter(&self) -> impl Iterator<Item = (&HotkeyAction, &KeyCombo)> {
        self.bindings.iter()
    }

    /// Get a reference to the inner bindings map.
    pub fn bindings(&self) -> &HashMap<HotkeyAction, KeyCombo> {
        &self.bindings
    }

    /// Serialize bindings for TOML output.
    /// If `verbose` is false, only non-default bindings are written.
    ///
    /// Returns a `BTreeMap<String, String>` of `action_toml_key → combo_display`.
    /// Using BTreeMap for deterministic key ordering in the TOML file.
    pub fn to_toml_map(&self, verbose: bool) -> std::collections::BTreeMap<String, String> {
        let mut map = std::collections::BTreeMap::new();
        for (action, combo) in &self.bindings {
            if verbose || *combo != action.default_binding() {
                map.insert(action.to_toml_key().to_string(), combo.to_string());
            }
        }
        map
    }

    /// Deserialize from a TOML map of `action_key → combo_string`.
    ///
    /// Starts with defaults, then overrides any entries found in the map.
    /// Unknown action keys or unparseable combos are warned and skipped.
    pub fn from_toml_map(map: &std::collections::BTreeMap<String, String>) -> Self {
        let mut config = Self::default();
        for (action_key, combo_str) in map {
            if let Some(action) = HotkeyAction::from_toml_key(action_key) {
                match combo_str.parse::<KeyCombo>() {
                    Ok(combo) => {
                        config.set_binding(action, combo);
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to parse hotkey combo '{}' for {}: {}",
                            combo_str,
                            action_key,
                            e
                        );
                    }
                }
            } else {
                tracing::warn!("Unknown hotkey action in config.toml: {}", action_key);
            }
        }
        config
    }
}
