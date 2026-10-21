//! TV-remote keys.
//!
//! A remote with a USB dongle (the G20S-style air mice) is a keyboard to the
//! system, but two of its most-used buttons don't arrive as the keys they mean.
//! Its OK sends `KEY_SELECT`, which iced has no name for, and its Back sends
//! the browser Back key. [`remote_key_alias`] runs in the keyboard
//! subscription, before any handler sees the press, so OK acts exactly like
//! Enter everywhere. Back becomes Escape at the top of the raw-key handler
//! instead ([`is_remote_back`]), which keeps knowing it came from a remote: with
//! nothing left to back out of, it goes up to the view tabs. Plasma Bigscreen,
//! the TV desktop these remotes drive, reads OK and Back the same way.
//!
//! A remote's Menu sends either the keyboard Menu key (`KEY_COMPOSE`) or the
//! consumer Menu (`KEY_MENU`, which iced can't name); both become the one Menu.
//!
//! The remote's other buttons (Home, Menu, the media and volume keys) keep
//! their own names as bindable `KeyCode`s; a key iced can't name at all is
//! still bindable by its Linux input-event code ([`unnamed_key_code`]).

use iced::keyboard::{
    Key,
    key::{Named, NativeCode, Physical},
};

/// Linux `KEY_MENU`, a remote's Menu sent as a consumer key (HID 0x40).
/// xkeyboard-config names it `XF86MenuKB`, which winit doesn't map.
const KEY_MENU: u32 = 139;
/// Linux `KEY_OK` (`linux/input-event-codes.h`), a remote's OK on some models.
const KEY_OK: u32 = 352;
/// Linux `KEY_SELECT`, the G20S's OK in arrow mode (HID consumer usage 0x41).
/// xkeyboard-config names it `XF86Select`, which winit doesn't map, so iced
/// reports it as `Key::Unidentified`.
const KEY_SELECT: u32 = 353;

/// The key a press stands for: a remote's OK is Enter and its consumer Menu
/// is the Menu key. Every other key comes back unchanged (Back included: see
/// [`is_remote_back`]).
pub(crate) fn remote_key_alias(key: Key, physical: &Physical) -> Key {
    match key {
        Key::Named(Named::Select) => Key::Named(Named::Enter),
        Key::Unidentified if matches!(linux_key_code(physical), Some(KEY_OK | KEY_SELECT)) => {
            Key::Named(Named::Enter)
        }
        Key::Unidentified if linux_key_code(physical) == Some(KEY_MENU) => {
            Key::Named(Named::ContextMenu)
        }
        other => other,
    }
}

/// A remote's Back (the browser Back key): Escape, plus one remote-only step
/// when nothing is left to back out of (`Nokkvi::handle_back`).
pub(crate) fn is_remote_back(key: &Key) -> bool {
    matches!(key, Key::Named(Named::BrowserBack | Named::GoBack))
}

/// Enter and Escape (a remote's OK and Back among them) act once per press.
/// A remote's press often releases late on a busy frame and repeats, and the
/// repeat would act on whatever the first press opened: OK on a tab opened the
/// view, then its repeat played the row there (2026-10-20 review).
pub(crate) fn acts_once_per_press(key: &Key) -> bool {
    matches!(
        key,
        Key::Named(Named::Enter | Named::Escape | Named::BrowserBack | Named::GoBack)
    )
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
    fn a_remote_back_is_told_apart_from_escape() {
        assert!(is_remote_back(&Key::Named(Named::BrowserBack)));
        assert!(is_remote_back(&Key::Named(Named::GoBack)));
        assert!(!is_remote_back(&Key::Named(Named::Escape)));
        // The subscription leaves it alone; the raw-key handler turns it into
        // Escape.
        let back = Key::Named(Named::BrowserBack);
        assert_eq!(remote_key_alias(back.clone(), &unidentified(158)), back);
    }

    #[test]
    fn a_remote_consumer_menu_is_the_menu_key() {
        assert_eq!(
            remote_key_alias(Key::Unidentified, &unidentified(KEY_MENU)),
            Key::Named(Named::ContextMenu)
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
