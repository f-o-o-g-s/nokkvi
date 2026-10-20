//! Hotkey Handling Module
//!
//! Centralizes keyboard shortcut handling for the application.
//! Split into global hotkeys and view-specific hotkeys.

mod global;
mod remote;

pub(crate) use global::{action_to_message, key_press_to_keycode, resolve_action};
pub(crate) use remote::{remote_key_alias, unnamed_key_code};
