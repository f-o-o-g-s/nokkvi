//! TV-remote keys.
//!
//! A remote with a USB dongle (the G20S-style air mice) is a keyboard to the
//! system, but two of its most-used buttons don't arrive as the keys they mean.
//! Its OK sends `KEY_SELECT`, which iced has no name for, and its Back sends
//! the browser Back key. [`remote_key_alias`] runs in the keyboard
//! subscription, before any handler sees the press, so OK and Back act exactly
//! like Enter and Escape everywhere: the modal key gate, the rules editor,
//! Theater Mode, hotkey capture. Plasma Bigscreen, the TV desktop these remotes
//! drive, reads them the same way.
//!
//! The remote's other buttons (Home, Menu, the media and volume keys) keep
//! their own names as bindable `KeyCode`s; a key iced can't name at all is
//! still bindable by its Linux input-event code ([`unnamed_key_code`]).

use iced::keyboard::{
    Key,
    key::{Named, NativeCode, Physical},
};

/// Linux `KEY_OK` (`linux/input-event-codes.h`), a remote's OK on some models.
const KEY_OK: u32 = 352;
/// Linux `KEY_SELECT`, the G20S's OK in arrow mode (HID consumer usage 0x41).
/// xkeyboard-config names it `XF86Select`, which winit doesn't map, so iced
/// reports it as `Key::Unidentified`.
const KEY_SELECT: u32 = 353;

/// The key a press stands for: a remote's OK is Enter and its Back is Escape.
/// Every other key comes back unchanged.
pub(crate) fn remote_key_alias(key: Key, physical: &Physical) -> Key {
    match key {
        Key::Named(Named::Select) => Key::Named(Named::Enter),
        Key::Named(Named::BrowserBack | Named::GoBack) => Key::Named(Named::Escape),
        Key::Unidentified if matches!(linux_key_code(physical), Some(KEY_OK | KEY_SELECT)) => {
            Key::Named(Named::Enter)
        }
        other => other,
    }
}

/// The Linux input-event code of a key iced has no name for, so
/// `KeyCode::Native` can bind it; `None` for every key iced names.
pub(crate) fn unnamed_key_code(key: &Key, physical: &Physical) -> Option<u32> {
    match key {
        Key::Unidentified => linux_key_code(physical),
        Key::Named(_) | Key::Character(_) => None,
    }
}

/// The evdev code behind a physical key iced couldn't classify. winit's Linux
/// backends (Wayland and X11) report it as `Xkb(code)`, already minus XKB's
/// offset of 8, so it is the number `evtest` prints. Code 0 is no key.
fn linux_key_code(physical: &Physical) -> Option<u32> {
    match physical {
        Physical::Unidentified(NativeCode::Xkb(code)) if *code != 0 => Some(*code),
        Physical::Unidentified(
            NativeCode::Xkb(_)
            | NativeCode::Unidentified
            | NativeCode::Android(_)
            | NativeCode::MacOS(_)
            | NativeCode::Windows(_),
        )
        | Physical::Code(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use iced::keyboard::key::Code;

    use super::*;

    fn unidentified(code: u32) -> Physical {
        Physical::Unidentified(NativeCode::Xkb(code))
    }

    #[test]
    fn a_remote_ok_is_enter() {
        // The G20S in arrow mode: no logical key, physical KEY_SELECT.
        assert_eq!(
            remote_key_alias(Key::Unidentified, &unidentified(KEY_SELECT)),
            Key::Named(Named::Enter)
        );
        assert_eq!(
            remote_key_alias(Key::Unidentified, &unidentified(KEY_OK)),
            Key::Named(Named::Enter)
        );
        // A keymap that does name KEY_SELECT.
        assert_eq!(
            remote_key_alias(Key::Named(Named::Select), &Physical::Code(Code::Select)),
            Key::Named(Named::Enter)
        );
    }

    #[test]
    fn a_remote_back_is_escape() {
        assert_eq!(
            remote_key_alias(Key::Named(Named::BrowserBack), &unidentified(158)),
            Key::Named(Named::Escape)
        );
        assert_eq!(
            remote_key_alias(Key::Named(Named::GoBack), &unidentified(158)),
            Key::Named(Named::Escape)
        );
    }

    #[test]
    fn other_keys_pass_through() {
        let home = Key::Named(Named::BrowserHome);
        assert_eq!(remote_key_alias(home.clone(), &unidentified(172)), home);
        let a = Key::Character("a".into());
        assert_eq!(remote_key_alias(a.clone(), &Physical::Code(Code::KeyA)), a);
        // An unnamed key that isn't OK stays unnamed (and bindable by code).
        assert_eq!(
            remote_key_alias(Key::Unidentified, &unidentified(582)),
            Key::Unidentified
        );
    }

    #[test]
    fn only_unnamed_keys_carry_a_code() {
        // KEY_VOICECOMMAND, the G20S voice button.
        assert_eq!(
            unnamed_key_code(&Key::Unidentified, &unidentified(582)),
            Some(582)
        );
        assert_eq!(
            unnamed_key_code(&Key::Named(Named::BrowserHome), &unidentified(172)),
            None
        );
        assert_eq!(
            unnamed_key_code(&Key::Unidentified, &Physical::Code(Code::KeyA)),
            None
        );
        assert_eq!(unnamed_key_code(&Key::Unidentified, &unidentified(0)), None);
    }
}
