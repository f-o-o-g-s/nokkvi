//! `KeyCombo` — pairs a `KeyCode` with modifier flags.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use super::KeyCode;

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
}

impl KeyCombo {
    /// Create a simple key combo with no modifiers.
    pub const fn key(key: KeyCode) -> Self {
        Self {
            key,
            shift: false,
            ctrl: false,
            alt: false,
        }
    }

    /// Create a Shift+key combo.
    pub const fn shift(key: KeyCode) -> Self {
        Self {
            key,
            shift: true,
            ctrl: false,
            alt: false,
        }
    }

    /// Create a Ctrl+key combo.
    pub const fn ctrl(key: KeyCode) -> Self {
        Self {
            key,
            shift: false,
            ctrl: true,
            alt: false,
        }
    }

    /// Human-readable display (e.g. "Shift + L", "Ctrl + D", "Space")
    pub fn display(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        parts.push(self.key.to_string());
        parts.join(" + ")
    }
}

impl fmt::Display for KeyCombo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display())
    }
}

impl FromStr for KeyCombo {
    type Err = String;

    /// Parse a human-readable key combo string back to a `KeyCombo`.
    ///
    /// Accepts the format produced by `KeyCombo::display()`: `"Shift + L"`,
    /// `"Ctrl + E"`, `"Space"`, `"Shift + ↑"`, etc.
    fn from_str(s: &str) -> Result<Self, String> {
        let parts: Vec<&str> = s.split('+').map(str::trim).collect();
        if parts.is_empty() {
            return Err("empty key combo".to_string());
        }

        let mut shift = false;
        let mut ctrl = false;
        let mut alt = false;

        // All parts except the last are modifiers
        for &part in &parts[..parts.len() - 1] {
            match part.to_lowercase().as_str() {
                "shift" => shift = true,
                "ctrl" | "control" => ctrl = true,
                "alt" => alt = true,
                _ => return Err(format!("unknown modifier: {part}")),
            }
        }

        let key_str = parts.last().ok_or_else(|| "no key specified".to_string())?;
        let key = KeyCode::from_name(key_str)?;

        Ok(KeyCombo {
            key,
            shift,
            ctrl,
            alt,
        })
    }
}
