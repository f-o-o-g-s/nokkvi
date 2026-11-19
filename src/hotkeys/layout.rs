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
//! - **Letter keys** (`KeyA`..`KeyZ`): the layout's character when it is an
//!   ASCII letter, so AZERTY / Dvorak / QWERTZ users keep the letters printed
//!   on their keys; ASCII punctuation on a letter key is kept on a Latin layout
//!   (AZERTY's `M` key types `,`) and becomes the US letter on a non-Latin
//!   layout (Greek `Q` types `;`, Hebrew `Q` types `/` and `W` types `'`,
//!   Bulgarian `Q` types `,`); non-ASCII characters (Cyrillic, Greek, Turkish
//!   `ı`, etc.) become the US letter at that position.
//! - **The digit row** (`Digit0`..`Digit9`): the digit, whatever character
//!   the layout types there. AZERTY's unshifted row types `&é"'(`, Czech types
//!   `+ěščř`, and the number-key bindings (view switching) must still work
//!   there.
//! - **Punctuation keys** (`` ` - = [ ] \ ; ' , . / ``): the layout's
//!   character when it is ASCII and the layout is Latin (Dvorak's `s` on the
//!   US `;` key, German `-` on the US `/` key), else the US character at that
//!   position. A non-Latin layout puts ASCII punctuation on other keys
//!   (Russian types `.` on the US `/` key), so taking its ASCII literally would
//!   fire the wrong binding and leave `/` (Focus Search) unreachable.
//!
//! **Dead keys stay unbound** (iced reports them as `Key::Unidentified`, and
//! they pass through unchanged on every position). A dead key types nothing by
//! itself, so a focused text field doesn't claim it: bound, German `´` (on the
//! US `=` key) would raise a rating in the middle of typing "Café", and the
//! accent would land on the next letter typed. They bind nothing on the digit
//! row either (Vietnamese (France) has dead keys on 2, 4 and 7).
//!
//! "The layout is Latin" ([`LATIN_LAYOUT`]) is learned from every letter typed
//! on any of these keys, before the rule above runs: a Latin-script letter
//! (ASCII, Turkish `ı`, German `ö` on the US `;` key, Spanish `º`) marks it
//! Latin, any other letter (Cyrillic, Greek, Hebrew, Arabic, Thai) non-Latin,
//! and punctuation leaves it as it was. It starts Latin. Right after a layout
//! switch, ASCII punctuation reads the old way until the first letter: from
//! Russian to German, `-` on the US `/` key fires Focus Search until then. A
//! Settings capture made in that window stores that reading.
//!
//! Keys with no US-QWERTY character (named keys, the numpad, ISO's extra
//! key) and presses without a physical code pass through unchanged.

use std::sync::atomic::{AtomicBool, Ordering};

use iced::keyboard::{
    Key,
    key::{Code, Physical},
};
use tracing::debug;

/// Whether the active layout types Latin letters, as of the last letter
/// typed. Written only from the keyboard subscription.
static LATIN_LAYOUT: AtomicBool = AtomicBool::new(true);

/// Crate-wide serialization guard for tests that call
/// [`layout_independent_key`] (and therefore read or mutate [`LATIN_LAYOUT`]).
/// Every such test holds this lock so parallel tests cannot race on the
/// process-global flag. Pure [`normalize`] tests do not need it.
#[cfg(test)]
pub(crate) static LATIN_LAYOUT_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

/// The key a press stands for in binding terms: see the module docs.
/// Updates [`LATIN_LAYOUT`] when the press types a letter.
pub(crate) fn layout_independent_key(key: Key, physical: &Physical) -> Key {
    let latin = LATIN_LAYOUT.load(Ordering::Relaxed);
    let (key, latin_now) = normalize(key, physical, latin);
    if latin_now != latin {
        LATIN_LAYOUT.store(latin_now, Ordering::Relaxed);
        // Logged on a flip only: whether punctuation binds as typed or by
        // position hangs on it ("`/` stopped working after a layout switch").
        debug!(
            " [HOTKEY] Keyboard layout now reads as {}",
            if latin_now { "Latin" } else { "non-Latin" }
        );
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

/// Whether a letter is Latin script: everything below U+0370 (ASCII, the
/// Latin-1 letters such as `ö` and `º`, Latin Extended-A/B such as Turkish
/// `ı`, and the IPA letters Ghanaian layouts type) plus Latin Extended
/// Additional (Vietnamese). The cutoff is iced's `Key::to_latin`.
fn is_latin_script(letter: char) -> bool {
    letter < '\u{0370}' || ('\u{1E00}'..='\u{1EFF}').contains(&letter)
}

/// The pure rule behind [`layout_independent_key`]: the key to bind against,
/// and whether the layout reads as Latin afterwards.
fn normalize(key: Key, physical: &Physical, latin: bool) -> (Key, bool) {
    let Some(us) = (match physical {
        Physical::Code(code) => us_qwerty(*code),
        Physical::Unidentified(_) => None,
    }) else {
        return (key, latin);
    };
    // A dead key (`Key::Unidentified`) or a named key keeps its key: see the
    // module docs for why a dead key never binds.
    let Key::Character(typed) = &key else {
        return (key, latin);
    };
    let mut chars = typed.chars();
    let single = match (chars.next(), chars.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    };
    // A letter says which script the layout types, wherever it sits; a
    // multi-character string, punctuation or a symbol leaves that as it was.
    let latin = match single {
        Some(c) if c.is_alphabetic() => is_latin_script(c),
        Some(_) | None => latin,
    };
    let us_key = |c: char| Key::Character(c.to_string().into());
    // One ASCII character on a Latin layout is what the key is labelled;
    // anything else (a Cyrillic letter, ASCII on a non-Latin layout, an empty
    // string) binds by position.
    let as_typed = latin && single.is_some_and(|c| c.is_ascii());
    let key = match us {
        UsKey::Letter(_) | UsKey::Punctuation(_) if as_typed => key,
        // The digit row is always the digit (AZERTY's `&` is 1),
        UsKey::Digit(c)
        // and every other key not kept as typed binds by its US position.
        | UsKey::Letter(c)
        | UsKey::Punctuation(c) => us_key(c),
    };
    (key, latin)
}

#[cfg(test)]
mod xkb_tests;

#[cfg(test)]
mod tests {
    use iced::keyboard::{Modifiers, key::NativeCode};
    use nokkvi_data::types::hotkey_config::HotkeyConfig;

    use super::*;
    use crate::hotkeys::{resolve_action, unnamed_key_code};

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
    fn punctuation_on_a_letter_key_binds_by_position_on_a_non_latin_layout() {
        // Greek Q types ";"; Hebrew Q types "/" and W types "'".
        assert_eq!(press(";", Code::KeyQ, false), (ch("q"), false));
        assert_eq!(press("/", Code::KeyQ, false), (ch("q"), false));
        assert_eq!(press("'", Code::KeyW, false), (ch("w"), false));
        // Greek letter flips the flag to non-Latin.
        assert_eq!(press("σ", Code::KeyS, true), (ch("s"), false));
    }

    #[test]
    fn non_ascii_latin_letters_keep_the_layout_latin() {
        // Turkish dotless "ı" on KeyI maps to "i" and marks the layout Latin.
        assert_eq!(press("ı", Code::KeyI, true), (ch("i"), true));
        assert_eq!(press("ı", Code::KeyI, false), (ch("i"), true));
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
    }

    #[test]
    fn punctuation_keeps_the_layouts_ascii_on_a_latin_layout() {
        // German QWERTZ types "-" on the US "/" key.
        assert_eq!(press("-", Code::Slash, true), (ch("-"), true));
        assert_eq!(press("/", Code::Slash, true), (ch("/"), true));
    }

    #[test]
    fn a_letter_on_a_punctuation_key_keeps_its_letter_and_marks_the_layout_latin() {
        // Dvorak: the US ";" key types "s" and the US "/" key "z". They stay
        // letters even right after a Cyrillic stretch, and say Latin.
        assert_eq!(press("s", Code::Semicolon, true), (ch("s"), true));
        assert_eq!(press("z", Code::Slash, false), (ch("z"), true));
        // German "ö" on the US ";" key binds as ";" and says Latin; Russian
        // "ж" on the same key says non-Latin.
        assert_eq!(press("ö", Code::Semicolon, false), (ch(";"), true));
        assert_eq!(press("ж", Code::Semicolon, true), (ch(";"), false));
        // Spanish "º" sits below U+00C0 and is Latin all the same.
        assert_eq!(press("º", Code::Backquote, false), (ch("`"), true));
        // AZERTY's digit row says Latin too ("é" on the 2 key).
        assert_eq!(press("é", Code::Digit2, false), (ch("2"), true));
    }

    #[test]
    fn dead_keys_stay_unbound() {
        // A dead key types nothing by itself, so a focused text field doesn't
        // claim it. Bound, German "´" (on the US "=" key) would raise the
        // selected row's rating in the middle of typing "Café".
        for code in [Code::Equal, Code::Backquote, Code::Digit2, Code::KeyQ] {
            let physical = Physical::Code(code);
            assert_eq!(
                normalize(Key::Unidentified, &physical, true),
                (Key::Unidentified, true)
            );
            assert_eq!(
                resolve_action(
                    &Key::Unidentified,
                    unnamed_key_code(&Key::Unidentified, &physical),
                    Modifiers::empty(),
                    &HotkeyConfig::default()
                ),
                None
            );
        }
    }

    #[test]
    fn odd_text_on_a_letter_key_binds_by_position() {
        assert_eq!(press("", Code::KeyA, false), (ch("a"), false));
        assert_eq!(press("ab", Code::KeyA, true), (ch("a"), true));
    }

    #[test]
    fn right_after_a_layout_switch_ascii_reads_the_old_way_until_a_letter() {
        // A known limit, pinned so a change to it is deliberate. From a Latin
        // layout to Hebrew, the Q key's "/" stays "/" (Focus Search)...
        assert_eq!(press("/", Code::KeyQ, true), (ch("/"), true));
        // ...and from Russian to German, "-" on the US "/" key reads as "/".
        assert_eq!(press("-", Code::Slash, false), (ch("/"), false));
    }

    #[test]
    fn any_letter_brings_the_flag_up_to_date_after_a_layout_switch() {
        let _guard = LATIN_LAYOUT_LOCK.lock();
        let live =
            |typed: &str, code: Code| layout_independent_key(ch(typed), &Physical::Code(code));
        // Russian, then Dvorak: the "s" on the US ";" key is a letter, so the
        // "z" on the US "/" key after it stays "z" instead of Focus Search.
        assert_eq!(live("ы", Code::KeyS), ch("s"));
        assert_eq!(live("s", Code::Semicolon), ch("s"));
        assert_eq!(live("z", Code::Slash), ch("z"));
        // Russian, then German: "ö" says Latin, so "-" on the US "/" key is "-".
        assert_eq!(live("ы", Code::KeyS), ch("s"));
        assert_eq!(live("ö", Code::Semicolon), ch(";"));
        assert_eq!(live("-", Code::Slash), ch("-"));
        // Back to Russian: "ж" on the US ";" key says non-Latin, so "." on
        // the US "/" key is "/" again.
        assert_eq!(live("ж", Code::Semicolon), ch(";"));
        assert_eq!(live(".", Code::Slash), ch("/"));
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
        // ISO's extra key (Greek types "«" there).
        assert_eq!(press("«", Code::IntlBackslash, false), (ch("«"), false));
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
