//! `HotkeyConfig` — the persisted action → key-combo binding map.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{HotkeyAction, KeyCode, KeyCombo};

/// Defaults that have MOVED since a release, paired with the combo they used
/// to hold.
///
/// Moving a default is otherwise a silent trap: a user whose config names the
/// old default (every `verbose_config = "on"` file does) would end up with two
/// actions on one combo, and only one of them would ever fire. The rule in
/// [`HotkeyConfig::normalize`] carries those files forward — but ONLY when the
/// retired binding actually collides.
///
/// A hand-written `prev_sort_mode = "Left"` is byte-identical to what a
/// verbose dump wrote, so the rule cannot tell them apart and normalizes both.
/// To put the old action back on that key for good, the NEW owner has to move
/// off it as well — which is exactly what the capture UI's steal does, and why
/// a swap made there survives a restart.
///
/// Add a row here in the same commit that changes a `default:` in
/// `define_hotkey_actions!`.
const RETIRED_DEFAULTS: &[(HotkeyAction, KeyCombo)] = &[
    // 0.18.x → bare Left/Right became Seek Backward / Seek Forward.
    (
        HotkeyAction::PrevSortMode,
        KeyCombo::key(KeyCode::ArrowLeft),
    ),
    (
        HotkeyAction::NextSortMode,
        KeyCombo::key(KeyCode::ArrowRight),
    ),
];

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
    /// Two actions CAN share one combo — a user rebinding onto an occupied
    /// key, or a new default landing on a combo a user already claimed. The
    /// winner is picked by [`Self::resolve`], never by map order, so the same
    /// key fires the same action on every launch.
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
        self.resolve(&combo, None)
    }

    /// Decide which action owns `combo`, in a fixed order that does not depend
    /// on `HashMap` iteration:
    ///
    /// 1. a **reserved** action (Escape / Delete are never user-configurable,
    ///    so a configurable action parked on one must not shadow it);
    /// 2. a configurable action whose binding **differs from its default** —
    ///    the user put it there on purpose, so it beats an action merely
    ///    sitting where it was shipped;
    /// 3. otherwise the earliest match in [`HotkeyAction::ALL`] (declaration
    ///    order).
    ///
    /// `exclude` skips one action, which is what makes [`Self::find_conflict`]
    /// share this ordering instead of running its own scan. Reads
    /// [`Self::get_binding`], so an action missing from the map is compared by
    /// its default — the old two-pass persisted-then-defaults walk folds in.
    fn resolve(&self, combo: &KeyCombo, exclude: Option<&HotkeyAction>) -> Option<HotkeyAction> {
        for action in HotkeyAction::RESERVED {
            if exclude == Some(action) {
                continue;
            }
            if self.get_binding(action) == *combo {
                return Some(*action);
            }
        }
        let mut on_default: Option<HotkeyAction> = None;
        for action in HotkeyAction::ALL {
            if exclude == Some(action) {
                continue;
            }
            let bound = self.get_binding(action);
            if bound != *combo {
                continue;
            }
            if bound != action.default_binding() {
                return Some(*action);
            }
            if on_default.is_none() {
                on_default = Some(*action);
            }
        }
        on_default
    }

    /// Check if a key combo conflicts with an existing binding (excluding a given action).
    /// Returns the conflicting action, if any.
    ///
    /// Shares [`Self::resolve`] with [`Self::lookup`], so the action named here
    /// as the conflict is exactly the one the key would actually fire.
    pub fn find_conflict(&self, combo: &KeyCombo, exclude: &HotkeyAction) -> Option<HotkeyAction> {
        self.resolve(combo, Some(exclude))
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
        Self::from_toml_map_reporting(map).0
    }

    /// [`Self::from_toml_map`], plus whether [`Self::normalize`] had to change
    /// anything.
    ///
    /// `true` means the file on disk still describes a layout this version no
    /// longer ships — startup rewrites `[hotkeys]` once so the file stops
    /// claiming something untrue. The hot-reload path deliberately discards
    /// the flag: writing from there would re-trigger the config watcher.
    pub fn from_toml_map_reporting(
        map: &std::collections::BTreeMap<String, String>,
    ) -> (Self, bool) {
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
        let changed = config.normalize();
        (config, changed)
    }

    /// Bring a loaded config up to date with this version's defaults. Returns
    /// whether anything changed.
    ///
    /// Two steps:
    ///
    /// 1. fill in any action the config does not mention — a new action added
    ///    since the config was written, which a pre-upgrade redb blob simply
    ///    has no key for;
    /// 2. for each entry in [`RETIRED_DEFAULTS`] still sitting on its retired
    ///    combo, hand that combo over to whoever else now claims it and move
    ///    the retired action to its CURRENT default.
    ///
    /// Step 2 is conditional on the collision on purpose. A user who moved the
    /// new owner elsewhere — exactly what the capture UI's swap does when they
    /// take the key back — has no conflict, so their choice stands.
    pub fn normalize(&mut self) -> bool {
        let mut changed = false;
        for action in HotkeyAction::ALL
            .iter()
            .chain(HotkeyAction::RESERVED.iter())
        {
            if !self.bindings.contains_key(action) {
                self.bindings.insert(*action, action.default_binding());
                changed = true;
            }
        }
        for (action, retired) in RETIRED_DEFAULTS {
            if self.get_binding(action) != *retired {
                continue;
            }
            if !self.is_claimed_by_another(retired, action) {
                continue;
            }
            let current = action.default_binding();
            // Never move an action onto a combo someone ELSE already holds:
            // `resolve` would hand that combo to the other action (a user's
            // binding beats one sitting on its default), leaving this action
            // with no working key at all. Standing pat instead keeps the
            // PRE-UPGRADE layout working — the retired binding differs from
            // the current default, so `resolve` reads it as the user's choice
            // and it keeps the contested key, shadowing the newly-shipped
            // action rather than an action they have been using for releases.
            if self.is_claimed_by_another(&current, action) {
                tracing::warn!(
                    "Hotkey '{}' moved to another action in this version, but '{}' cannot follow \
                     it to '{}' — that combo is taken. Leaving it on '{}'; rebind in \
                     Settings > Hotkeys to get the new layout.",
                    retired,
                    action.display_name(),
                    current,
                    retired
                );
                continue;
            }
            tracing::info!(
                "Hotkey '{}' moved to another action in this version: '{}' follows its new \
                 default '{}'",
                retired,
                action.display_name(),
                current
            );
            self.bindings.insert(*action, current);
            changed = true;
        }
        // Runs here rather than at the TOML reader so the redb-only load path
        // (no `[hotkeys]` table) gets the same diagnostic — that path is the
        // one most likely to CREATE a collision, since a pre-upgrade blob has
        // no key for an action added since it was written.
        self.warn_on_shared_combos();
        changed
    }

    /// Whether some action other than `owner` is bound to `combo`.
    fn is_claimed_by_another(&self, combo: &KeyCombo, owner: &HotkeyAction) -> bool {
        HotkeyAction::ALL
            .iter()
            .chain(HotkeyAction::RESERVED.iter())
            .any(|other| other != owner && self.get_binding(other) == *combo)
    }

    /// Log one `warn!` per key combo that two or more actions now share,
    /// naming every claimant and the one [`Self::resolve`] will fire.
    ///
    /// Called after the overrides land, so it reports the file the user
    /// actually has. Runs on hot-reload too — the file log is the right place
    /// for a "your config says two things" notice, and there is no user-facing
    /// surface for an unbound action yet.
    fn warn_on_shared_combos(&self) {
        let mut claims: HashMap<KeyCombo, Vec<HotkeyAction>> = HashMap::new();
        for action in HotkeyAction::ALL
            .iter()
            .chain(HotkeyAction::RESERVED.iter())
        {
            claims
                .entry(self.get_binding(action))
                .or_default()
                .push(*action);
        }
        for (combo, actions) in claims {
            if actions.len() < 2 {
                continue;
            }
            let names: Vec<&str> = actions.iter().map(|a| a.display_name()).collect();
            let winner = self
                .resolve(&combo, None)
                .map_or("nothing", |a| a.display_name());
            tracing::warn!(
                "Hotkey '{}' is claimed by {} actions ({}) — '{}' wins; rebind the others in \
                 Settings > Hotkeys",
                combo,
                actions.len(),
                names.join(", "),
                winner
            );
        }
    }
}
