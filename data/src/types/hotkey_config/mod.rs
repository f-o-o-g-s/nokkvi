//! Hotkey configuration types — maps logical actions to physical key combinations
//!
//! `HotkeyAction` enumerates every bindable action in the application.
//! `KeyCombo` pairs a `KeyCode` with modifier flags (shift, ctrl, alt).
//! `HotkeyConfig` stores the full binding map, supports lookup by key event,
//! conflict detection, and serde for persistence in redb.

mod action;
mod config;
mod key_code;
mod key_combo;

pub use action::HotkeyAction;
pub use config::HotkeyConfig;
pub use key_code::KeyCode;
pub use key_combo::KeyCombo;

#[cfg(test)]
mod tests;
