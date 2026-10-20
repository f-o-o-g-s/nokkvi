//! `KeyCode` — a framework-agnostic key identifier with serde support.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A key identifier that can be serialized/deserialized.
/// Wraps iced key variants in a framework-agnostic enum.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KeyCode {
    /// A printable character key (lowercase, e.g. 'a', '1', '/')
    Char(char),
    // Named keys
    Space,
    Enter,
    Escape,
    Backspace,
    Tab,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    PageUp,
    PageDown,
    Home,
    End,
    Delete,
    Insert,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    // Remote-control and media keys: what a TV remote's USB dongle and a
    // keyboard's media row send. A remote's OK and Back are not here: they
    // arrive as Enter and Escape (`src/hotkeys/remote.rs`).
    Menu,
    HomePage,
    Search,
    Favorites,
    Forward,
    Refresh,
    MediaPlayPause,
    MediaPause,
    MediaStop,
    MediaNext,
    MediaPrevious,
    MediaRewind,
    MediaFastForward,
    VolumeUp,
    VolumeDown,
    VolumeMute,
    /// A key with no name, by its Linux input-event code (the `KEY_*` number
    /// `evtest` prints), so a remote's odd button (its voice key is
    /// `KEY_VOICECOMMAND`, 582) can still be bound.
    Native(u32),
}

impl fmt::Display for KeyCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyCode::Char(c) => {
                // Display printable chars in uppercase for readability
                match c {
                    '`' => write!(f, "`"),
                    '/' => write!(f, "/"),
                    '-' => write!(f, "-"),
                    '=' => write!(f, "="),
                    _ => write!(f, "{}", c.to_uppercase()),
                }
            }
            KeyCode::Space => write!(f, "Space"),
            KeyCode::Enter => write!(f, "Enter"),
            KeyCode::Escape => write!(f, "Escape"),
            KeyCode::Backspace => write!(f, "Backspace"),
            KeyCode::Tab => write!(f, "Tab"),
            KeyCode::ArrowUp => write!(f, "Up"),
            KeyCode::ArrowDown => write!(f, "Down"),
            KeyCode::ArrowLeft => write!(f, "Left"),
            KeyCode::ArrowRight => write!(f, "Right"),
            KeyCode::PageUp => write!(f, "Page Up"),
            KeyCode::PageDown => write!(f, "Page Down"),
            KeyCode::Home => write!(f, "Home"),
            KeyCode::End => write!(f, "End"),
            KeyCode::Delete => write!(f, "Delete"),
            KeyCode::Insert => write!(f, "Insert"),
            KeyCode::F1 => write!(f, "F1"),
            KeyCode::F2 => write!(f, "F2"),
            KeyCode::F3 => write!(f, "F3"),
            KeyCode::F4 => write!(f, "F4"),
            KeyCode::F5 => write!(f, "F5"),
            KeyCode::F6 => write!(f, "F6"),
            KeyCode::F7 => write!(f, "F7"),
            KeyCode::F8 => write!(f, "F8"),
            KeyCode::F9 => write!(f, "F9"),
            KeyCode::F10 => write!(f, "F10"),
            KeyCode::F11 => write!(f, "F11"),
            KeyCode::F12 => write!(f, "F12"),
            KeyCode::Menu => write!(f, "Menu"),
            KeyCode::HomePage => write!(f, "Home Page"),
            KeyCode::Search => write!(f, "Search"),
            KeyCode::Favorites => write!(f, "Favorites"),
            KeyCode::Forward => write!(f, "Forward"),
            KeyCode::Refresh => write!(f, "Refresh"),
            KeyCode::MediaPlayPause => write!(f, "Play/Pause"),
            KeyCode::MediaPause => write!(f, "Media Pause"),
            KeyCode::MediaStop => write!(f, "Media Stop"),
            KeyCode::MediaNext => write!(f, "Next Track"),
            KeyCode::MediaPrevious => write!(f, "Previous Track"),
            KeyCode::MediaRewind => write!(f, "Rewind"),
            KeyCode::MediaFastForward => write!(f, "Fast Forward"),
            KeyCode::VolumeUp => write!(f, "Volume Up"),
            KeyCode::VolumeDown => write!(f, "Volume Down"),
            KeyCode::VolumeMute => write!(f, "Mute"),
            KeyCode::Native(code) => write!(f, "Key {code}"),
        }
    }
}

impl KeyCode {
    /// Parse a key name string (from Display output) back to a KeyCode.
    ///
    /// Handles uppercase/lowercase chars, arrow symbols, and named keys.
    /// Returns `Err` for unrecognized key names.
    pub fn from_name(s: &str) -> Result<Self, String> {
        let s = s.trim();
        // Single ASCII character → Char variant (lowercase)
        if s.chars().count() == 1 && s.is_ascii() {
            let c = s.chars().next().unwrap_or('?');
            return Ok(KeyCode::Char(c.to_lowercase().next().unwrap_or(c)));
        }
        // Named keys (case-insensitive)
        match s.to_lowercase().as_str() {
            "space" => Ok(KeyCode::Space),
            "enter" | "return" => Ok(KeyCode::Enter),
            "escape" | "esc" => Ok(KeyCode::Escape),
            "backspace" => Ok(KeyCode::Backspace),
            "tab" => Ok(KeyCode::Tab),
            "↑" | "arrowup" | "up" => Ok(KeyCode::ArrowUp),
            "↓" | "arrowdown" | "down" => Ok(KeyCode::ArrowDown),
            "←" | "arrowleft" | "left" => Ok(KeyCode::ArrowLeft),
            "→" | "arrowright" | "right" => Ok(KeyCode::ArrowRight),
            "page up" | "pageup" => Ok(KeyCode::PageUp),
            "page down" | "pagedown" => Ok(KeyCode::PageDown),
            "home" => Ok(KeyCode::Home),
            "end" => Ok(KeyCode::End),
            "delete" | "del" => Ok(KeyCode::Delete),
            "insert" | "ins" => Ok(KeyCode::Insert),
            "f1" => Ok(KeyCode::F1),
            "f2" => Ok(KeyCode::F2),
            "f3" => Ok(KeyCode::F3),
            "f4" => Ok(KeyCode::F4),
            "f5" => Ok(KeyCode::F5),
            "f6" => Ok(KeyCode::F6),
            "f7" => Ok(KeyCode::F7),
            "f8" => Ok(KeyCode::F8),
            "f9" => Ok(KeyCode::F9),
            "f10" => Ok(KeyCode::F10),
            "f11" => Ok(KeyCode::F11),
            "f12" => Ok(KeyCode::F12),
            "menu" | "context menu" => Ok(KeyCode::Menu),
            "home page" | "homepage" => Ok(KeyCode::HomePage),
            "search" => Ok(KeyCode::Search),
            "favorites" | "favourites" => Ok(KeyCode::Favorites),
            "forward" => Ok(KeyCode::Forward),
            "refresh" => Ok(KeyCode::Refresh),
            "play/pause" | "play" => Ok(KeyCode::MediaPlayPause),
            "media pause" => Ok(KeyCode::MediaPause),
            "media stop" => Ok(KeyCode::MediaStop),
            "next track" => Ok(KeyCode::MediaNext),
            "previous track" | "prev track" => Ok(KeyCode::MediaPrevious),
            "rewind" => Ok(KeyCode::MediaRewind),
            "fast forward" => Ok(KeyCode::MediaFastForward),
            "volume up" => Ok(KeyCode::VolumeUp),
            "volume down" => Ok(KeyCode::VolumeDown),
            "mute" | "volume mute" => Ok(KeyCode::VolumeMute),
            other => other
                .strip_prefix("key ")
                .and_then(|code| code.trim().parse().ok())
                .map(KeyCode::Native)
                .ok_or_else(|| format!("unknown key: {s}")),
        }
    }
}
