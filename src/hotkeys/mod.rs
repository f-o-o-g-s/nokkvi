//! Hotkey Handling Module
//!
//! Centralizes keyboard shortcut handling for the application.
//! Split into global hotkeys and view-specific hotkeys.

mod global;

pub(crate) use global::{handle_hotkey, iced_key_to_keycode};
