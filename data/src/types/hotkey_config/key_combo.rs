//! `KeyCombo` — pairs a `KeyCode` with modifier flags.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use super::KeyCode;

/// The word the logo modifier goes by in config text: Cmd on macOS, Super
/// (the Windows key) elsewhere. [`KeyCombo::from_str`] reads every spelling on
/// every OS, so a config file moves between machines.
#[cfg(target_os = "macos")]
const LOGO_NAME: &str = "Cmd";
#[cfg(not(target_os = "macos"))]
const LOGO_NAME: &str = "Super";

/// The word Alt goes by in config text. macOS calls the key Option, and
/// either spelling parses everywhere.
#[cfg(target_os = "macos")]
const ALT_NAME: &str = "Option";
#[cfg(not(target_os = "macos"))]
const ALT_NAME: &str = "Alt";

/// A key combination: a key code plus modifier flags.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeyCombo {
    pub key: KeyCode,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub alt: bool,
    /// The logo modifier: Cmd (⌘) on macOS, Super / the Windows key elsewhere.
    #[serde(default)]
    pub logo: bool,
}

impl KeyCombo {
    /// Create a simple key combo with no modifiers.
    pub const fn key(key: KeyCode) -> Self {
        Self {
            key,
            shift: false,
            ctrl: false,
            alt: false,
            logo: false,
        }
    }

    /// Create a Shift+key combo.
    pub const fn shift(key: KeyCode) -> Self {
        Self {
            shift: true,
            ..Self::key(key)
        }
    }

    /// Create a Ctrl+key combo.
    pub const fn ctrl(key: KeyCode) -> Self {
        Self {
            ctrl: true,
            ..Self::key(key)
        }
    }

    /// Create a Cmd+key (macOS) / Super+key combo.
    pub const fn logo(key: KeyCode) -> Self {
        Self {
            logo: true,
            ..Self::key(key)
        }
    }

    /// The platform's primary shortcut modifier + key: Cmd on macOS, Ctrl
    /// elsewhere (what iced's `Modifiers::command()` reads). Defaults that are
    /// "a shortcut" rather than a bare key use it, so a Mac gets ⌘E where
    /// Linux gets Ctrl+E.
    pub const fn primary(key: KeyCode) -> Self {
        if cfg!(target_os = "macos") {
            Self::logo(key)
        } else {
            Self::ctrl(key)
        }
    }

    /// Whether the OS (or every text field) owns this combo, so it can never
    /// be a hotkey: binding it would either never fire or fire on top of the
    /// system action (⌘C copying in a search field AND running the hotkey).
    ///
    /// On macOS these are the Cmd chords every app shares: Quit, Close, Hide,
    /// Minimize, the edit commands (Copy / Paste / Cut / Select All / Undo,
    /// ⇧⌘Z Redo), the app switcher (⌘Tab), window cycling (⌘\`) and
    /// Spotlight (⌘Space). Elsewhere nothing is reserved here: a desktop that
    /// grabs a Super chord simply never delivers it.
    pub fn is_os_reserved(&self) -> bool {
        if !cfg!(target_os = "macos") || !self.logo || self.ctrl || self.alt {
            return false;
        }
        match &self.key {
            KeyCode::Char(c) => {
                matches!(c, 'q' | 'w' | 'h' | 'm' | 'c' | 'v' | 'x' | 'a' | 'z' | '`')
            }
            KeyCode::Tab | KeyCode::Space => true,
            _ => false,
        }
    }

    /// The combo as config text (`"Ctrl + Shift + L"`, `"Cmd + E"` on macOS,
    /// `"Super + E"` elsewhere): the `[hotkeys]` wire format that
    /// [`Self::from_str`] reads back. Also what [`fmt::Display`] prints.
    pub fn to_config_string(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        if self.alt {
            parts.push(ALT_NAME.to_string());
        }
        if self.logo {
            parts.push(LOGO_NAME.to_string());
        }
        parts.push(self.key.to_string());
        parts.join(" + ")
    }

    /// Human-readable label for the UI.
    ///
    /// On macOS, modifiers render as Apple's glyphs in Apple's order
    /// (⌃ ⌥ ⇧ ⌘), the way menus show shortcuts: `"⇧⌘L"`, `"⌥Space"`. On
    /// other platforms it is the config text (`"Ctrl + Shift + L"`).
    pub fn display(&self) -> String {
        if cfg!(target_os = "macos") {
            self.mac_glyph_label()
        } else {
            self.to_config_string()
        }
    }

    /// The macOS menu-style label behind [`Self::display`], built on every OS
    /// so its test runs everywhere.
    fn mac_glyph_label(&self) -> String {
        let mut out = String::new();
        if self.ctrl {
            out.push('⌃');
        }
        if self.alt {
            out.push('⌥');
        }
        if self.shift {
            out.push('⇧');
        }
        if self.logo {
            out.push('⌘');
        }
        out.push_str(&self.key.to_string());
        out
    }
}

impl fmt::Display for KeyCombo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_config_string())
    }
}

impl FromStr for KeyCombo {
    type Err = String;

    /// Parse a human-readable key combo string back to a `KeyCombo`.
    ///
    /// Accepts the format produced by [`KeyCombo::to_config_string`]:
    /// `"Shift + L"`, `"Ctrl + E"`, `"Space"`, `"Cmd + Shift + ↑"`, etc.
    /// Modifier names are case-insensitive and every platform's spelling
    /// parses everywhere (`Cmd` / `Command` / `Super` / `Win` / `Meta` / `⌘`,
    /// `Alt` / `Option` / `⌥`, `Ctrl` / `Control` / `⌃`, `Shift` / `⇧`), and
    /// so does the macOS UI label ([`KeyCombo::display`], `"⇧⌘L"`).
    fn from_str(s: &str) -> Result<Self, String> {
        let parts: Vec<&str> = s.split('+').map(str::trim).collect();
        let Some((key_str, modifiers)) = parts.split_last() else {
            return Err("empty key combo".to_string());
        };

        // The macOS UI label packs glyph modifiers onto the key (`"⇧⌘L"`), so
        // a label copied out of Settings parses too.
        let mut glyphs = String::new();
        let key_str = key_str.trim_start_matches(|c| {
            let glyph = matches!(c, '⌃' | '⌥' | '⇧' | '⌘');
            if glyph {
                glyphs.push(c);
            }
            glyph
        });

        let mut combo = KeyCombo::key(KeyCode::from_name(key_str)?);
        for glyph in glyphs.chars() {
            match glyph {
                '⌃' => combo.ctrl = true,
                '⌥' => combo.alt = true,
                '⇧' => combo.shift = true,
                _ => combo.logo = true,
            }
        }
        for &part in modifiers {
            match part.to_lowercase().as_str() {
                "shift" | "⇧" => combo.shift = true,
                "ctrl" | "control" | "⌃" => combo.ctrl = true,
                "alt" | "option" | "opt" | "⌥" => combo.alt = true,
                "cmd" | "command" | "super" | "win" | "windows" | "meta" | "logo" | "⌘" => {
                    combo.logo = true;
                }
                _ => return Err(format!("unknown modifier: {part}")),
            }
        }
        Ok(combo)
    }
}
