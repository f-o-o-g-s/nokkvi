//! The default hotkeys over 30 real keyboard layouts. `testdata/xkb_level0.json`
//! holds what each key types with no modifiers on each layout (what iced
//! reports as `KeyPressed.key`; `testdata/gen_xkb_level0.py` makes it from
//! libxkbcommon), and every key goes through the live rewrite
//! ([`layout_independent_key`]) and the binding lookup
//! ([`key_press_to_keycode`]), the way a press does.
//!
//! Two lists pin the result: the default hotkeys a layout can't reach, and
//! the keys that fire a hotkey belonging to neither the character printed on
//! them nor their US position. A change that moves either one fails here and
//! prints the new list, ready to paste once the change is meant.

use std::collections::{BTreeMap, BTreeSet};

use iced::keyboard::{
    Key,
    key::{Code, Physical},
};
use nokkvi_data::types::hotkey_config::{HotkeyConfig, KeyCode};
use serde::Deserialize;

use super::{LATIN_LAYOUT_LOCK, UsKey, layout_independent_key, us_qwerty};
use crate::hotkeys::{key_press_to_keycode, unnamed_key_code};

const TABLES: &str = include_str!("../testdata/xkb_level0.json");

/// Default hotkeys a layout can't reach without rebinding:
/// - `/` (Focus Search) is a Shift or AltGr character on most European
///   layouts, and their US `/` key types an ASCII character of their own
///   (German `-`), which binds as itself.
/// - `` ` `` (Settings) and `=` (Increase Rating): the US key types other
///   ASCII the layout keeps (Italian `\`), holds a dead key (German `^`,
///   Swiss `^`, US-International `` ` ``), or is Zenkaku/Hankaku (Japanese).
/// - `-` (Decrease Rating) on French: AZERTY types it on the 6 key, which
///   binds as the digit.
const KNOWN_UNREACHABLE: &[(&str, char)] = &[
    ("be", '/'),
    ("ch", '/'),
    ("ch", '='),
    ("cz", '/'),
    ("cz", '`'),
    ("de", '/'),
    ("de", '`'),
    ("es", '/'),
    ("fr", '-'),
    ("fr", '/'),
    ("hu", '/'),
    ("hu", '`'),
    ("it", '/'),
    ("it", '`'),
    ("jp", '='),
    ("jp", '`'),
    ("no", '/'),
    ("no", '`'),
    ("pt", '/'),
    ("pt", '`'),
    ("se", '/'),
    ("tr", '/'),
    ("tr", '='),
    ("tr", '`'),
    ("us(intl)", '`'),
];

/// Keys firing another key's default hotkey: (layout, key, the character the
/// lookup made of it). All of them predate the layout rewrite: the US shift
/// map in `iced_key_to_keycode` turns a symbol the layout types unshifted
/// into a US digit or `=` (French `$` → `4`, Songs; German `#` → `3`,
/// Artists; German `+` → `=`, Increase Rating).
const KNOWN_MISFIRES: &[(&str, &str, char)] = &[
    ("be", "BracketRight", '4'),
    ("ch", "Backslash", '4'),
    ("de", "Backslash", '3'),
    ("de", "BracketRight", '='),
    ("es", "BracketRight", '='),
    ("fr", "Backslash", '8'),
    ("fr", "BracketRight", '4'),
    ("fr", "Slash", '1'),
    ("gb", "Backslash", '3'),
    ("it", "BracketRight", '='),
    ("jp", "BracketLeft", '2'),
    ("jp", "Equal", '6'),
    ("no", "Minus", '='),
    ("pt", "BracketLeft", '='),
    ("se", "Minus", '='),
    ("tr", "Minus", '8'),
];

#[derive(Deserialize)]
struct Tables {
    layouts: BTreeMap<String, BTreeMap<String, Option<String>>>,
}

/// Every physical key the table covers: the US main block plus ISO's extra key.
const CODES: [Code; 48] = [
    Code::Backquote,
    Code::Digit1,
    Code::Digit2,
    Code::Digit3,
    Code::Digit4,
    Code::Digit5,
    Code::Digit6,
    Code::Digit7,
    Code::Digit8,
    Code::Digit9,
    Code::Digit0,
    Code::Minus,
    Code::Equal,
    Code::KeyQ,
    Code::KeyW,
    Code::KeyE,
    Code::KeyR,
    Code::KeyT,
    Code::KeyY,
    Code::KeyU,
    Code::KeyI,
    Code::KeyO,
    Code::KeyP,
    Code::BracketLeft,
    Code::BracketRight,
    Code::Backslash,
    Code::KeyA,
    Code::KeyS,
    Code::KeyD,
    Code::KeyF,
    Code::KeyG,
    Code::KeyH,
    Code::KeyJ,
    Code::KeyK,
    Code::KeyL,
    Code::Semicolon,
    Code::Quote,
    Code::IntlBackslash,
    Code::KeyZ,
    Code::KeyX,
    Code::KeyC,
    Code::KeyV,
    Code::KeyB,
    Code::KeyN,
    Code::KeyM,
    Code::Comma,
    Code::Period,
    Code::Slash,
];

fn code_named(name: &str) -> Code {
    CODES
        .into_iter()
        .find(|code| format!("{code:?}") == name)
        .unwrap_or_else(|| panic!("xkb_level0.json names a key CODES lacks: {name}"))
}

fn us_char(code: Code) -> Option<char> {
    match us_qwerty(code)? {
        UsKey::Letter(c) | UsKey::Digit(c) | UsKey::Punctuation(c) => Some(c),
    }
}

/// The character binding a press of `code` typing `typed` (`None`: a dead
/// key) looks up, if any.
fn bound_char(code: Code, typed: Option<&str>) -> Option<char> {
    let physical = Physical::Code(code);
    let key = typed.map_or(Key::Unidentified, |t| Key::Character(t.into()));
    let key = layout_independent_key(key, &physical);
    match key_press_to_keycode(&key, unnamed_key_code(&key, &physical))? {
        KeyCode::Char(c) => Some(c),
        _ => None,
    }
}

#[test]
fn default_hotkeys_over_30_real_layouts() {
    let _guard = LATIN_LAYOUT_LOCK.lock();
    let tables: Tables = serde_json::from_str(TABLES).expect("xkb_level0.json parses");
    let defaults = HotkeyConfig::default();
    let bound: BTreeSet<char> = defaults
        .iter()
        .filter_map(|(_, combo)| match combo.key {
            KeyCode::Char(c) => Some(c),
            _ => None,
        })
        .collect();

    let mut unreachable = Vec::new();
    let mut misfires = Vec::new();
    for (layout, keys) in &tables.layouts {
        // The layout's own A key first, as a user's first letter would: it
        // sets the Latin flag the punctuation keys read.
        let a = keys.get("KeyA").and_then(Option::as_deref);
        let _ = bound_char(Code::KeyA, a);
        let mut reached = BTreeSet::new();
        for (name, typed) in keys {
            let code = code_named(name);
            let Some(fires) = bound_char(code, typed.as_deref()) else {
                continue;
            };
            reached.insert(fires);
            let printed = typed
                .as_deref()
                .and_then(|t| t.chars().next())
                .and_then(|c| c.to_lowercase().next());
            if bound.contains(&fires) && Some(fires) != us_char(code) && Some(fires) != printed {
                misfires.push((layout.clone(), name.clone(), fires));
            }
        }
        unreachable.extend(bound.difference(&reached).map(|c| (layout.clone(), *c)));
    }

    let known_unreachable: Vec<(String, char)> = KNOWN_UNREACHABLE
        .iter()
        .map(|(layout, c)| ((*layout).to_owned(), *c))
        .collect();
    assert!(
        unreachable == known_unreachable,
        "the default hotkeys a layout can't reach changed. If that is meant, \
         KNOWN_UNREACHABLE becomes:\n{}",
        unreachable
            .iter()
            .map(|(layout, c)| format!("    ({layout:?}, {c:?}),"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let known_misfires: Vec<(String, String, char)> = KNOWN_MISFIRES
        .iter()
        .map(|(layout, name, c)| ((*layout).to_owned(), (*name).to_owned(), *c))
        .collect();
    assert!(
        misfires == known_misfires,
        "the keys that fire another key's default hotkey changed. If that is \
         meant, KNOWN_MISFIRES becomes:\n{}",
        misfires
            .iter()
            .map(|(layout, name, c)| format!("    ({layout:?}, {name:?}, {c:?}),"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
