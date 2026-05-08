//! Interface-tab settings table.
//!
//! Empty in the foundation slice — per-tab follow-up commits will migrate
//! the `general.nav_*`, `general.strip_*`, `general.artwork_column_*`,
//! `general.slot_*`, and `general.horizontal_volume` keys here.

use crate::define_settings;

define_settings! {
    tab: crate::types::setting_def::Tab::Interface,
    settings_const: TAB_INTERFACE_SETTINGS,
    contains_fn: tab_interface_contains,
    dispatch_fn: dispatch_interface_tab_setting,
    apply_fn: apply_toml_interface_tab,
    settings: []
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_interface_is_empty() {
        assert!(TAB_INTERFACE_SETTINGS.is_empty());
        assert!(!tab_interface_contains("general.stable_viewport"));
    }
}
