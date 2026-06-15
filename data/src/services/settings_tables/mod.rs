//! Per-tab settings tables backing the [`define_settings!`][crate::define_settings]
//! macro.
//!
//! Each submodule invokes the macro once with the entries it owns. The macro
//! emits, per tab:
//!
//! - `TAB_<TAB>_SETTINGS: &[SettingDef]` — declarative table.
//! - `tab_<tab>_contains(key) -> bool` — quick presence check for a tab's keys.
//! - `dispatch_<tab>_tab_setting(key, value, &mut SettingsManager)` — sync
//!   persistence dispatcher.
//! - `apply_toml_<tab>_tab(ts, p)` — TOML→`PersistedPlayerSettings` copy step.
//! - `dump_<tab>_tab_player_settings(src, out)` — `PersistedPlayerSettings`→
//!   UI-facing `LivePlayerSettings` copy step (drives
//!   `Message::PlayerSettingsLoaded`).
//! - `write_<tab>_tab_toml(ps, ts)` — UI-facing `LivePlayerSettings`→TOML
//!   copy step (inverse of `apply_toml_<tab>_tab`; called from
//!   `TomlSettings::from_player_settings`).
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
    dump_general_tab_player_settings, write_general_tab_toml,
};
pub use interface::{
    TAB_INTERFACE_SETTINGS, apply_toml_interface_tab, dispatch_interface_tab_setting,
    dump_interface_tab_player_settings, write_interface_tab_toml,
};
pub use playback::{
    TAB_PLAYBACK_SETTINGS, apply_toml_playback_tab, dispatch_playback_tab_setting,
    dump_playback_tab_player_settings, write_playback_tab_toml,
};

pub use crate::types::settings_side_effect::SettingsSideEffect;
