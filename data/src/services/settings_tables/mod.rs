//! Per-tab settings tables backing the [`define_settings!`][crate::define_settings]
//! macro.
//!
//! Each submodule invokes the macro once with the entries it owns. The macro
//! emits, per tab:
//!
//! - `TAB_<TAB>_SETTINGS: &[SettingDef]` — declarative table.
//! - `tab_<tab>_contains(key) -> bool` — quick presence check (used by the
//!   strangler-fig caller before locking the manager mutex).
//! - `dispatch_<tab>_tab_setting(key, value, &mut SettingsManager)` — sync
//!   persistence dispatcher.
//! - `apply_toml_<tab>_tab(ts, p)` — TOML→internal `PlayerSettings` copy step.
//! - `dump_<tab>_tab_player_settings(src, out)` — internal→UI-facing
//!   `PlayerSettings` copy step (drives `Message::PlayerSettingsLoaded`).
//!
//! See [`crate::types::setting_def`] for the macro and supporting types and
//! [`crate::types::settings_side_effect`] for the typed side-effect enum the
//! `on_dispatch:` hook returns.

pub mod general;
pub mod interface;
pub mod playback;

#[cfg(test)]
mod lock_watchpoint_test;

pub use general::{
    TAB_GENERAL_SETTINGS, apply_toml_general_tab, dispatch_general_tab_setting,
    dump_general_tab_player_settings, tab_general_contains,
};
pub use interface::{
    TAB_INTERFACE_SETTINGS, apply_toml_interface_tab, dispatch_interface_tab_setting,
    dump_interface_tab_player_settings, tab_interface_contains,
};
pub use playback::{
    TAB_PLAYBACK_SETTINGS, apply_toml_playback_tab, dispatch_playback_tab_setting,
    dump_playback_tab_player_settings, tab_playback_contains,
};

pub use crate::types::settings_side_effect::SettingsSideEffect;

/// Returns true if any per-tab dispatcher claims `key`. The strangler-fig
/// caller in `update/settings.rs` uses this as a sync pre-check before
/// locking the manager mutex inside an async task.
pub fn any_tab_contains(key: &str) -> bool {
    tab_general_contains(key) || tab_interface_contains(key) || tab_playback_contains(key)
}
