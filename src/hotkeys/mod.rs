//! Hotkey Handling Module
//!
//! Centralizes keyboard shortcut handling for the application.
//! Split into global hotkeys and view-specific hotkeys.

mod global;

pub(crate) use global::{action_to_message, iced_key_to_keycode, resolve_action};
