//! Typed discriminant for the `__*` sentinel keys used by the Settings view.
//!
//! Sentinel keys are stringly-typed flags that mark a `SettingItem` as a
//! special-purpose row (preset, restore-defaults, action button). Toggle-set
//! item keys (e.g. `__toggle_artwork_overlays`) are NOT sentinels — they're
//! regular keys whose values are `SettingValue::ToggleSet` and route through
//! `ToggleSetToggle`, not through this dispatch surface.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SentinelKind {
    Logout,
    RestoreVisualizer,
    RestoreAllHotkeys,
}

impl SentinelKind {
    /// Parse a settings item key into a typed `SentinelKind`.
    /// Returns `None` for any key that isn't a registered sentinel — including
    /// regular toggle-set keys like `__toggle_artwork_overlays`.
    pub(crate) fn from_key(key: &str) -> Option<Self> {
        match key {
            "__action_logout" => Some(Self::Logout),
            "__restore_visualizer" => Some(Self::RestoreVisualizer),
            "__restore_all_hotkeys" => Some(Self::RestoreAllHotkeys),
            _ => None,
        }
    }

    /// Emit the canonical settings-item key string for this sentinel.
    /// Used by item-builder sites so the literal lives in exactly one place.
    pub(crate) fn to_key(&self) -> String {
        match self {
            Self::Logout => "__action_logout".to_string(),
            Self::RestoreVisualizer => "__restore_visualizer".to_string(),
            Self::RestoreAllHotkeys => "__restore_all_hotkeys".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_key_roundtrip() {
        for k in [
            SentinelKind::Logout,
            SentinelKind::RestoreVisualizer,
            SentinelKind::RestoreAllHotkeys,
        ] {
            assert_eq!(SentinelKind::from_key(&k.to_key()), Some(k));
        }
    }

    #[test]
    fn toggle_keys_are_not_sentinels() {
        assert_eq!(SentinelKind::from_key("__toggle_artwork_overlays"), None);
        assert_eq!(SentinelKind::from_key("__toggle_strip_fields"), None);
    }

    #[test]
    fn unknown_double_underscore_is_not_sentinel() {
        assert_eq!(SentinelKind::from_key("__nope"), None);
        assert_eq!(SentinelKind::from_key("__unfocus_all__"), None);
        assert_eq!(SentinelKind::from_key("__progress_label"), None);
    }
}
