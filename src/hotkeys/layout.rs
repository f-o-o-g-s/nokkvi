//! Keyboard-layout independence for character hotkeys.
//!
//! iced reports a key press as the character the ACTIVE layout produces
//! (without modifiers). Bindings are stored as US-QWERTY characters (`s`,
//! `/`, `` ` ``), so under a Russian, Greek or Hebrew layout the `s` key
//! arrives as `ы` and nothing fires. [`layout_independent_key`] runs in the
//! keyboard subscription, before any handler sees the press (beside
//! [`super::remote_key_alias`]), and rewrites the character from the key's
//! physical position when the layout's own character can't be a binding:
//!
//! - **Letter keys** (`KeyA`..`KeyZ`): the layout's character when it is
//!   ASCII, so AZERTY / Dvorak / QWERTZ users keep the letters printed on
//!   their keys; otherwise (Cyrillic and other non-Latin scripts) the US
//!   letter at that position.
//! - **The digit row** (`Digit0`..`Digit9`): always the digit. AZERTY's
//!   unshifted row types `&é"'(`, Czech types `+ěščř`, and the number-key
//!   bindings (view switching) must still work there.
//! - **Punctuation keys** (`` ` - = [ ] \ ; ' , . / ``): the layout's
//!   character when it is ASCII and the layout is Latin, else the US
//!   character at that position. A non-Latin layout puts ASCII punctuation
//!   on other keys (Russian types `.` on the US `/` key), so taking its
//!   ASCII literally would fire the wrong binding — and leave `/` (Focus
//!   Search) unreachable.
//!
//! "The layout is Latin" is learned from the letter keys: the last letter
//! press decides it ([`LATIN_LAYOUT`]). It starts Latin, and after a layout
//! switch the first letter typed (in a search field or as a hotkey) brings it
//! up to date.
//!
//! Keys with no US-QWERTY character (named keys, the numpad, ISO's extra
//! key) and presses without a physical code pass through unchanged.

use std::sync::atomic::{AtomicBool, Ordering};

use iced::keyboard::{
    Key,
    key::{Code, Physical},
};

/// Whether the active layout types Latin letters, as of the last letter-key
/// press. Written only from the keyboard subscription.
static LATIN_LAYOUT: AtomicBool = AtomicBool::new(true);

/// The key a press stands for in binding terms: see the module docs.
/// Updates [`LATIN_LAYOUT`] on a letter-key press.
pub(crate) fn layout_independent_key(key: Key, physical: &Physical) -> Key {
    let latin = LATIN_LAYOUT.load(Ordering::Relaxed);
    let (key, latin_now) = normalize(key, physical, latin);
    if latin_now != latin {
        LATIN_LAYOUT.store(latin_now, Ordering::Relaxed);
    }
    key
}

/// Which family of keys a physical code belongs to, and the character it
/// carries on US QWERTY.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UsKey {
    Letter(char),
    Digit(char),
    Punctuation(char),
}

fn us_qwerty(code: Code) -> Option<UsKey> {
    use UsKey::{Digit, Letter, Punctuation};
    Some(match code {
        Code::KeyA => Letter('a'),
        Code::KeyB => Letter('b'),
        Code::KeyC => Letter('c'),
        Code::KeyD => Letter('d'),
        Code::KeyE => Letter('e'),
        Code::KeyF => Letter('f'),
        Code::KeyG => Letter('g'),
        Code::KeyH => Letter('h'),
        Code::KeyI => Letter('i'),
        Code::KeyJ => Letter('j'),
        Code::KeyK => Letter('k'),
        Code::KeyL => Letter('l'),
        Code::KeyM => Letter('m'),
        Code::KeyN => Letter('n'),
        Code::KeyO => Letter('o'),
        Code::KeyP => Letter('p'),
        Code::KeyQ => Letter('q'),
        Code::KeyR => Letter('r'),
        Code::KeyS => Letter('s'),
        Code::KeyT => Letter('t'),
        Code::KeyU => Letter('u'),
        Code::KeyV => Letter('v'),
        Code::KeyW => Letter('w'),
        Code::KeyX => Letter('x'),
        Code::KeyY => Letter('y'),
        Code::KeyZ => Letter('z'),
        Code::Digit0 => Digit('0'),
        Code::Digit1 => Digit('1'),
        Code::Digit2 => Digit('2'),
        Code::Digit3 => Digit('3'),
        Code::Digit4 => Digit('4'),
        Code::Digit5 => Digit('5'),
        Code::Digit6 => Digit('6'),
        Code::Digit7 => Digit('7'),
        Code::Digit8 => Digit('8'),
        Code::Digit9 => Digit('9'),
        Code::Backquote => Punctuation('`'),
        Code::Minus => Punctuation('-'),
        Code::Equal => Punctuation('='),
        Code::BracketLeft => Punctuation('['),
        Code::BracketRight => Punctuation(']'),
        Code::Backslash => Punctuation('\\'),
        Code::Semicolon => Punctuation(';'),
        Code::Quote => Punctuation('\''),
        Code::Comma => Punctuation(','),
        Code::Period => Punctuation('.'),
        Code::Slash => Punctuation('/'),
        _ => return None,
    })
}

/// The pure rule behind [`layout_independent_key`]: the key to bind against,
/// and whether the layout reads as Latin afterwards.
fn normalize(key: Key, physical: &Physical, latin: bool) -> (Key, bool) {
    let Key::Character(typed) = &key else {
        return (key, latin);
    };
    let Some(us) = (match physical {
        Physical::Code(code) => us_qwerty(*code),
        Physical::Unidentified(_) => None,
    }) else {
        return (key, latin);
    };
    // One ASCII character is something a binding can name; anything else (a
    // Cyrillic letter, a dead key's composed text, an empty string) is not.
    let mut chars = typed.chars();
    let ascii = matches!((chars.next(), chars.next()), (Some(c), None) if c.is_ascii());
    let us_key = |c: char| Key::Character(c.to_string().into());
    match us {
        UsKey::Letter(c) => {
            if ascii {
                (key, true)
            } else {
                (us_key(c), false)
            }
        }
        UsKey::Digit(c) => (us_key(c), latin),
        UsKey::Punctuation(c) => {
            if ascii && latin {
                (key, latin)
            } else {
                (us_key(c), latin)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use iced::keyboard::key::{NativeCode, Physical};

    use super::*;

    fn press(typed: &str, code: Code, latin: bool) -> (Key, bool) {
        normalize(Key::Character(typed.into()), &Physical::Code(code), latin)
    }

    fn ch(c: &str) -> Key {
        Key::Character(c.into())
    }

    #[test]
    fn a_cyrillic_letter_binds_as_the_us_letter_and_marks_the_layout() {
        // Russian ЙЦУКЕН: the S key types "ы", the L key "д".
        assert_eq!(press("ы", Code::KeyS, true), (ch("s"), false));
        assert_eq!(press("д", Code::KeyL, false), (ch("l"), false));
    }

    #[test]
    fn a_latin_letter_keeps_the_layouts_character() {
        // AZERTY's A key sits where US has Q; Dvorak's J key types "h".
        assert_eq!(press("a", Code::KeyQ, true), (ch("a"), true));
        assert_eq!(press("h", Code::KeyJ, true), (ch("h"), true));
        // Typing a Latin letter after a Cyrillic stretch flips back.
        assert_eq!(press("s", Code::KeyS, false), (ch("s"), true));
        // AZERTY's M key types ",": still a Latin layout, kept as typed.
        assert_eq!(press(",", Code::KeyM, true), (ch(","), true));
    }

    #[test]
    fn the_digit_row_always_binds_as_digits() {
        // AZERTY unshifted: & é " ' (
        assert_eq!(press("&", Code::Digit1, true), (ch("1"), true));
        assert_eq!(press("é", Code::Digit2, true), (ch("2"), true));
        // Russian types the digits themselves.
        assert_eq!(press("4", Code::Digit4, false), (ch("4"), false));
    }

    #[test]
    fn punctuation_follows_the_physical_key_on_a_non_latin_layout() {
        // Russian: the US "/" key types ".", the US "." key types "ю", the US
        // "`" key types "ё".
        assert_eq!(press(".", Code::Slash, false), (ch("/"), false));
        assert_eq!(press("ю", Code::Period, false), (ch("."), false));
        assert_eq!(press("ё", Code::Backquote, false), (ch("`"), false));
        // Before any letter told us otherwise, a non-ASCII character still
        // maps by position.
        assert_eq!(press("ж", Code::Semicolon, true), (ch(";"), true));
    }

    #[test]
    fn punctuation_keeps_the_layouts_ascii_on_a_latin_layout() {
        // German QWERTZ types "-" on the US "/" key.
        assert_eq!(press("-", Code::Slash, true), (ch("-"), true));
        assert_eq!(press("/", Code::Slash, true), (ch("/"), true));
    }

    #[test]
    fn keys_without_a_us_character_pass_through() {
        use iced::keyboard::key::Named;
        let enter = Key::Named(Named::Enter);
        assert_eq!(
            normalize(enter.clone(), &Physical::Code(Code::Enter), false),
            (enter, false)
        );
        assert_eq!(press("1", Code::Numpad1, true), (ch("1"), true));
        assert_eq!(
            normalize(
                ch("ы"),
                &Physical::Unidentified(NativeCode::Unidentified),
                true
            ),
            (ch("ы"), true)
        );
    }
}
