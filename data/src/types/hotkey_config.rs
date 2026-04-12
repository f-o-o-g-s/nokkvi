//! Hotkey configuration types — maps logical actions to physical key combinations
//!
//! `HotkeyAction` enumerates every bindable action in the application.
//! `KeyCombo` pairs a `KeyCode` with modifier flags (shift, ctrl, alt).
//! `HotkeyConfig` stores the full binding map, supports lookup by key event,
//! conflict detection, and serde for persistence in redb.

use std::{collections::HashMap, fmt, str::FromStr};

use serde::{Deserialize, Serialize};

// ============================================================================
// KeyCode — physical key identifier
// ============================================================================

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
            _ => Err(format!("unknown key: {s}")),
        }
    }
}

// ============================================================================
// KeyCombo — key + modifiers
// ============================================================================

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

// ============================================================================
// HotkeyAction — every bindable action
// ============================================================================

/// Every action that can be bound to a hotkey.
/// Variants are organized by functional category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HotkeyAction {
    // --- Navigation ---
    SwitchToQueue,
    SwitchToAlbums,
    SwitchToArtists,
    SwitchToSongs,
    SwitchToGenres,
    SwitchToPlaylists,
    SwitchToRadios,
    SwitchToSettings,

    // --- Playback ---
    TogglePlay,
    ToggleRandom,
    ToggleRepeat,
    ToggleConsume,
    ToggleSoundEffects,
    CycleVisualization,
    ToggleEqModal,

    // --- Slot list navigation ---
    SlotListUp,
    SlotListDown,
    Activate,
    ExpandCenter,

    // --- Browse actions ---
    ToggleBrowsingPanel,
    CenterOnPlaying,
    ToggleStar,
    AddToQueue,
    RemoveFromQueue,
    ClearQueue,
    FocusSearch,
    IncreaseRating,
    DecreaseRating,
    GetInfo,
    FindSimilar,
    FindTopSongs,

    // --- Queue reorder ---
    MoveTrackUp,
    MoveTrackDown,

    // --- Queue actions ---
    SaveQueueAsPlaylist,

    // --- Sort & view ---
    PrevSortMode,
    NextSortMode,
    ToggleSortOrder,
    RefreshView,

    // --- Settings edit (vertical) ---
    EditUp,
    EditDown,

    // --- Global ---
    Escape,
    ResetToDefault,
}

impl HotkeyAction {
    /// All variants in display order.
    pub const ALL: &'static [HotkeyAction] = &[
        // Navigation
        HotkeyAction::SwitchToQueue,
        HotkeyAction::SwitchToAlbums,
        HotkeyAction::SwitchToArtists,
        HotkeyAction::SwitchToSongs,
        HotkeyAction::SwitchToGenres,
        HotkeyAction::SwitchToPlaylists,
        HotkeyAction::SwitchToRadios,
        HotkeyAction::SwitchToSettings,
        // Playback
        HotkeyAction::TogglePlay,
        HotkeyAction::ToggleRandom,
        HotkeyAction::ToggleRepeat,
        HotkeyAction::ToggleConsume,
        HotkeyAction::ToggleSoundEffects,
        HotkeyAction::CycleVisualization,
        HotkeyAction::ToggleEqModal,
        // Slot List
        HotkeyAction::SlotListUp,
        HotkeyAction::SlotListDown,
        HotkeyAction::Activate,
        HotkeyAction::ExpandCenter,
        // Browse
        HotkeyAction::ToggleBrowsingPanel,
        HotkeyAction::CenterOnPlaying,
        HotkeyAction::ToggleStar,
        HotkeyAction::AddToQueue,
        HotkeyAction::RemoveFromQueue,
        HotkeyAction::ClearQueue,
        HotkeyAction::FocusSearch,
        HotkeyAction::IncreaseRating,
        HotkeyAction::DecreaseRating,
        HotkeyAction::GetInfo,
        HotkeyAction::FindSimilar,
        HotkeyAction::FindTopSongs,
        // Queue reorder
        HotkeyAction::MoveTrackUp,
        HotkeyAction::MoveTrackDown,
        // Queue actions
        HotkeyAction::SaveQueueAsPlaylist,
        // Sort & view
        HotkeyAction::PrevSortMode,
        HotkeyAction::NextSortMode,
        HotkeyAction::ToggleSortOrder,
        HotkeyAction::RefreshView,
        // Settings edit
        HotkeyAction::EditUp,
        HotkeyAction::EditDown,
        // Escape and Delete excluded — reserved, not configurable
    ];

    /// Reserved actions that are NOT user-configurable but still need bindings
    /// for `lookup()` to find them. These are excluded from `ALL` so they don't
    /// appear in the settings hotkey editor, but they must be present in the
    /// `HotkeyConfig::bindings` map.
    pub const RESERVED: &'static [HotkeyAction] =
        &[HotkeyAction::Escape, HotkeyAction::ResetToDefault];

    /// Human-readable name for display in settings UI.
    pub fn display_name(&self) -> &'static str {
        match self {
            HotkeyAction::SwitchToQueue => "Queue",
            HotkeyAction::SwitchToAlbums => "Albums",
            HotkeyAction::SwitchToArtists => "Artists",
            HotkeyAction::SwitchToSongs => "Songs",
            HotkeyAction::SwitchToGenres => "Genres",
            HotkeyAction::SwitchToPlaylists => "Playlists",
            HotkeyAction::SwitchToRadios => "Radios",
            HotkeyAction::SwitchToSettings => "Settings",
            HotkeyAction::TogglePlay => "Play / Pause",
            HotkeyAction::ToggleRandom => "Toggle Random",
            HotkeyAction::ToggleRepeat => "Toggle Repeat",
            HotkeyAction::ToggleConsume => "Toggle Consume",
            HotkeyAction::ToggleSoundEffects => "Toggle SFX",
            HotkeyAction::CycleVisualization => "Cycle Visualizer",
            HotkeyAction::ToggleEqModal => "Toggle Equalizer",
            HotkeyAction::SlotListUp => "Slot List Up",
            HotkeyAction::SlotListDown => "Slot List Down",
            HotkeyAction::Activate => "Activate / Enter",
            HotkeyAction::ExpandCenter => "Expand / Collapse",
            HotkeyAction::ToggleBrowsingPanel => "Library Browser",
            HotkeyAction::CenterOnPlaying => "Center on Playing",
            HotkeyAction::ToggleStar => "Toggle Love",
            HotkeyAction::AddToQueue => "Add to Queue",
            HotkeyAction::RemoveFromQueue => "Remove from Queue",
            HotkeyAction::ClearQueue => "Clear Queue",
            HotkeyAction::FocusSearch => "Search",
            HotkeyAction::IncreaseRating => "Increase Rating",
            HotkeyAction::DecreaseRating => "Decrease Rating",
            HotkeyAction::GetInfo => "Get Info",
            HotkeyAction::FindSimilar => "Find Similar",
            HotkeyAction::FindTopSongs => "Top Songs",
            HotkeyAction::MoveTrackUp => "Move Track Up",
            HotkeyAction::MoveTrackDown => "Move Track Down",
            HotkeyAction::SaveQueueAsPlaylist => "Save Queue as Playlist",
            HotkeyAction::PrevSortMode => "Previous Sort Mode",
            HotkeyAction::NextSortMode => "Next Sort Mode",
            HotkeyAction::ToggleSortOrder => "Sort Asc / Desc",
            HotkeyAction::RefreshView => "Refresh View",
            HotkeyAction::EditUp => "Edit Value Up",
            HotkeyAction::EditDown => "Edit Value Down",
            HotkeyAction::Escape => "Escape / Back",
            HotkeyAction::ResetToDefault => "Reset to Default",
        }
    }

    /// Brief description of what this action does (shown as subtext in settings).
    pub fn description(&self) -> &'static str {
        match self {
            HotkeyAction::SwitchToQueue => "Switch to the queue view",
            HotkeyAction::SwitchToAlbums => "Switch to the albums view",
            HotkeyAction::SwitchToArtists => "Switch to the artists view",
            HotkeyAction::SwitchToSongs => "Switch to the songs view",
            HotkeyAction::SwitchToGenres => "Switch to the genres view",
            HotkeyAction::SwitchToPlaylists => "Switch to the playlists view",
            HotkeyAction::SwitchToRadios => "Switch to the internet radios view",
            HotkeyAction::SwitchToSettings => "Open the settings panel",
            HotkeyAction::TogglePlay => "Play or pause the current track",
            HotkeyAction::ToggleRandom => "Toggle random/shuffle mode",
            HotkeyAction::ToggleRepeat => "Cycle repeat mode (off → one → queue)",
            HotkeyAction::ToggleConsume => "Toggle consume mode (remove after play)",
            HotkeyAction::ToggleSoundEffects => "Enable or disable sound effects",
            HotkeyAction::CycleVisualization => "Cycle visualizer (off → bars → lines)",
            HotkeyAction::ToggleEqModal => "Open or close the 10-band graphic equalizer",
            HotkeyAction::SlotListUp => "Navigate up in the slot list",
            HotkeyAction::SlotListDown => "Navigate down in the slot list",
            HotkeyAction::Activate => "Activate the focused item",
            HotkeyAction::ExpandCenter => {
                "Expand/collapse item. Works in albums/artists/playlists/genres only."
            }
            HotkeyAction::ToggleBrowsingPanel => "Toggle library browser beside queue",
            HotkeyAction::CenterOnPlaying => "Scroll to the currently playing track",
            HotkeyAction::ToggleStar => "Love/unlove · Navidrome star API",
            HotkeyAction::AddToQueue => "Add focused album/song to the queue",
            HotkeyAction::RemoveFromQueue => "Remove focused item from the queue",
            HotkeyAction::ClearQueue => "Clear the entire queue",
            HotkeyAction::FocusSearch => "Focus the search input field",
            HotkeyAction::IncreaseRating => "Increase rating by one star",
            HotkeyAction::DecreaseRating => "Decrease rating by one star",
            HotkeyAction::GetInfo => "Show info for the focused item",
            HotkeyAction::FindSimilar => "Find similar songs for the playing track",
            HotkeyAction::FindTopSongs => "Show top songs for the playing track's artist",
            HotkeyAction::MoveTrackUp => "Move centered track up in queue",
            HotkeyAction::MoveTrackDown => "Move centered track down in queue",
            HotkeyAction::SaveQueueAsPlaylist => "Open save-as-playlist dialog for the queue",
            HotkeyAction::PrevSortMode => "Cycle sort mode backward",
            HotkeyAction::NextSortMode => "Cycle sort mode forward",
            HotkeyAction::ToggleSortOrder => "Toggle ascending/descending sort",
            HotkeyAction::RefreshView => "Reload current view data from the server",
            HotkeyAction::EditUp => "Toggle setting on · enable field",
            HotkeyAction::EditDown => "Toggle setting off · disable field",
            HotkeyAction::Escape => "Close overlay, clear search, or go back",
            HotkeyAction::ResetToDefault => "Reset focused setting to its default value",
        }
    }

    /// Category label for grouping in the settings slot list.
    pub fn category(&self) -> &'static str {
        match self {
            HotkeyAction::SwitchToQueue
            | HotkeyAction::SwitchToAlbums
            | HotkeyAction::SwitchToArtists
            | HotkeyAction::SwitchToSongs
            | HotkeyAction::SwitchToGenres
            | HotkeyAction::SwitchToPlaylists
            | HotkeyAction::SwitchToRadios
            | HotkeyAction::SwitchToSettings => "Views",

            HotkeyAction::TogglePlay
            | HotkeyAction::ToggleRandom
            | HotkeyAction::ToggleRepeat
            | HotkeyAction::ToggleConsume
            | HotkeyAction::ToggleSoundEffects
            | HotkeyAction::CycleVisualization
            | HotkeyAction::ToggleEqModal => "Playback",

            HotkeyAction::SlotListUp
            | HotkeyAction::SlotListDown
            | HotkeyAction::Activate
            | HotkeyAction::ExpandCenter
            | HotkeyAction::FocusSearch
            | HotkeyAction::CenterOnPlaying
            | HotkeyAction::ToggleBrowsingPanel => "Navigation",

            HotkeyAction::ToggleStar
            | HotkeyAction::IncreaseRating
            | HotkeyAction::DecreaseRating
            | HotkeyAction::GetInfo
            | HotkeyAction::FindSimilar
            | HotkeyAction::FindTopSongs
            | HotkeyAction::AddToQueue
            | HotkeyAction::RemoveFromQueue
            | HotkeyAction::ClearQueue
            | HotkeyAction::MoveTrackUp
            | HotkeyAction::MoveTrackDown
            | HotkeyAction::SaveQueueAsPlaylist => "Item Actions",

            HotkeyAction::PrevSortMode
            | HotkeyAction::NextSortMode
            | HotkeyAction::ToggleSortOrder
            | HotkeyAction::RefreshView => "Sort & View",

            HotkeyAction::EditUp | HotkeyAction::EditDown => "Settings Edit",

            HotkeyAction::Escape | HotkeyAction::ResetToDefault => "Global",
        }
    }

    /// Default key binding for this action (matches current hardcoded bindings).
    pub fn default_binding(&self) -> KeyCombo {
        match self {
            // Navigation
            HotkeyAction::SwitchToQueue => KeyCombo::key(KeyCode::Char('1')),
            HotkeyAction::SwitchToAlbums => KeyCombo::key(KeyCode::Char('2')),
            HotkeyAction::SwitchToArtists => KeyCombo::key(KeyCode::Char('3')),
            HotkeyAction::SwitchToSongs => KeyCombo::key(KeyCode::Char('4')),
            HotkeyAction::SwitchToGenres => KeyCombo::key(KeyCode::Char('5')),
            HotkeyAction::SwitchToPlaylists => KeyCombo::key(KeyCode::Char('6')),
            HotkeyAction::SwitchToRadios => KeyCombo::key(KeyCode::Char('7')),
            HotkeyAction::SwitchToSettings => KeyCombo::key(KeyCode::Char('`')),
            // Playback
            HotkeyAction::TogglePlay => KeyCombo::key(KeyCode::Space),
            HotkeyAction::ToggleRandom => KeyCombo::key(KeyCode::Char('x')),
            HotkeyAction::ToggleRepeat => KeyCombo::key(KeyCode::Char('z')),
            HotkeyAction::ToggleConsume => KeyCombo::key(KeyCode::Char('c')),
            HotkeyAction::ToggleSoundEffects => KeyCombo::key(KeyCode::Char('s')),
            HotkeyAction::CycleVisualization => KeyCombo::key(KeyCode::Char('v')),
            HotkeyAction::ToggleEqModal => KeyCombo::key(KeyCode::Char('q')),
            // Slot list navigation
            HotkeyAction::SlotListUp => KeyCombo::key(KeyCode::Backspace),
            HotkeyAction::SlotListDown => KeyCombo::key(KeyCode::Tab),
            HotkeyAction::Activate => KeyCombo::key(KeyCode::Enter),
            HotkeyAction::ExpandCenter => KeyCombo::shift(KeyCode::Enter),
            // Browse actions
            HotkeyAction::ToggleBrowsingPanel => KeyCombo::ctrl(KeyCode::Char('e')),
            HotkeyAction::CenterOnPlaying => KeyCombo::shift(KeyCode::Char('c')),
            HotkeyAction::ToggleStar => KeyCombo::shift(KeyCode::Char('l')),
            HotkeyAction::AddToQueue => KeyCombo::shift(KeyCode::Char('a')),
            HotkeyAction::RemoveFromQueue => KeyCombo::ctrl(KeyCode::Char('d')),
            HotkeyAction::ClearQueue => KeyCombo::shift(KeyCode::Char('d')),
            HotkeyAction::FocusSearch => KeyCombo::key(KeyCode::Char('/')),
            HotkeyAction::IncreaseRating => KeyCombo::key(KeyCode::Char('=')),
            HotkeyAction::DecreaseRating => KeyCombo::key(KeyCode::Char('-')),
            HotkeyAction::GetInfo => KeyCombo::shift(KeyCode::Char('i')),
            HotkeyAction::FindSimilar => KeyCombo::shift(KeyCode::Char('s')),
            HotkeyAction::FindTopSongs => KeyCombo::shift(KeyCode::Char('t')),
            // Queue reorder
            HotkeyAction::MoveTrackUp => KeyCombo::shift(KeyCode::ArrowUp),
            HotkeyAction::MoveTrackDown => KeyCombo::shift(KeyCode::ArrowDown),
            // Queue actions
            HotkeyAction::SaveQueueAsPlaylist => KeyCombo::ctrl(KeyCode::Char('s')),
            // Sort & view
            HotkeyAction::PrevSortMode => KeyCombo::key(KeyCode::ArrowLeft),
            HotkeyAction::NextSortMode => KeyCombo::key(KeyCode::ArrowRight),
            HotkeyAction::ToggleSortOrder => KeyCombo::key(KeyCode::PageUp),
            HotkeyAction::RefreshView => KeyCombo::key(KeyCode::Char('r')),
            // Settings edit
            HotkeyAction::EditUp => KeyCombo::key(KeyCode::ArrowUp),
            HotkeyAction::EditDown => KeyCombo::key(KeyCode::ArrowDown),
            // Global
            HotkeyAction::Escape => KeyCombo::key(KeyCode::Escape),
            HotkeyAction::ResetToDefault => KeyCombo::key(KeyCode::Delete),
        }
    }

    /// Convert to a snake_case TOML key string.
    pub fn to_toml_key(self) -> &'static str {
        match self {
            HotkeyAction::SwitchToQueue => "switch_to_queue",
            HotkeyAction::SwitchToAlbums => "switch_to_albums",
            HotkeyAction::SwitchToArtists => "switch_to_artists",
            HotkeyAction::SwitchToSongs => "switch_to_songs",
            HotkeyAction::SwitchToGenres => "switch_to_genres",
            HotkeyAction::SwitchToPlaylists => "switch_to_playlists",
            HotkeyAction::SwitchToRadios => "switch_to_radios",
            HotkeyAction::SwitchToSettings => "switch_to_settings",
            HotkeyAction::TogglePlay => "toggle_play",
            HotkeyAction::ToggleRandom => "toggle_random",
            HotkeyAction::ToggleRepeat => "toggle_repeat",
            HotkeyAction::ToggleConsume => "toggle_consume",
            HotkeyAction::ToggleSoundEffects => "toggle_sound_effects",
            HotkeyAction::CycleVisualization => "cycle_visualization",
            HotkeyAction::ToggleEqModal => "toggle_eq_modal",
            HotkeyAction::SlotListUp => "slot_list_up",
            HotkeyAction::SlotListDown => "slot_list_down",
            HotkeyAction::Activate => "activate",
            HotkeyAction::ExpandCenter => "expand_center",
            HotkeyAction::ToggleBrowsingPanel => "toggle_browsing_panel",
            HotkeyAction::CenterOnPlaying => "center_on_playing",
            HotkeyAction::ToggleStar => "toggle_star",
            HotkeyAction::AddToQueue => "add_to_queue",
            HotkeyAction::RemoveFromQueue => "remove_from_queue",
            HotkeyAction::ClearQueue => "clear_queue",
            HotkeyAction::FocusSearch => "focus_search",
            HotkeyAction::IncreaseRating => "increase_rating",
            HotkeyAction::DecreaseRating => "decrease_rating",
            HotkeyAction::GetInfo => "get_info",
            HotkeyAction::FindSimilar => "find_similar",
            HotkeyAction::FindTopSongs => "find_top_songs",
            HotkeyAction::MoveTrackUp => "move_track_up",
            HotkeyAction::MoveTrackDown => "move_track_down",
            HotkeyAction::SaveQueueAsPlaylist => "save_queue_as_playlist",
            HotkeyAction::PrevSortMode => "prev_sort_mode",
            HotkeyAction::NextSortMode => "next_sort_mode",
            HotkeyAction::ToggleSortOrder => "toggle_sort_order",
            HotkeyAction::RefreshView => "refresh_view",
            HotkeyAction::EditUp => "edit_up",
            HotkeyAction::EditDown => "edit_down",
            HotkeyAction::Escape => "escape",
            HotkeyAction::ResetToDefault => "reset_to_default",
        }
    }

    /// Parse from a snake_case TOML key string. Returns None for unknown keys.
    pub fn from_toml_key(s: &str) -> Option<HotkeyAction> {
        Some(match s {
            "switch_to_queue" => HotkeyAction::SwitchToQueue,
            "switch_to_albums" => HotkeyAction::SwitchToAlbums,
            "switch_to_artists" => HotkeyAction::SwitchToArtists,
            "switch_to_songs" => HotkeyAction::SwitchToSongs,
            "switch_to_genres" => HotkeyAction::SwitchToGenres,
            "switch_to_playlists" => HotkeyAction::SwitchToPlaylists,
            "switch_to_radios" => HotkeyAction::SwitchToRadios,
            "switch_to_settings" => HotkeyAction::SwitchToSettings,
            "toggle_play" => HotkeyAction::TogglePlay,
            "toggle_random" => HotkeyAction::ToggleRandom,
            "toggle_repeat" => HotkeyAction::ToggleRepeat,
            "toggle_consume" => HotkeyAction::ToggleConsume,
            "toggle_sound_effects" => HotkeyAction::ToggleSoundEffects,
            "cycle_visualization" => HotkeyAction::CycleVisualization,
            "toggle_eq_modal" => HotkeyAction::ToggleEqModal,
            "slot_list_up" => HotkeyAction::SlotListUp,
            "slot_list_down" => HotkeyAction::SlotListDown,
            "activate" => HotkeyAction::Activate,
            "expand_center" => HotkeyAction::ExpandCenter,
            "toggle_browsing_panel" => HotkeyAction::ToggleBrowsingPanel,
            "center_on_playing" => HotkeyAction::CenterOnPlaying,
            "toggle_star" => HotkeyAction::ToggleStar,
            "add_to_queue" => HotkeyAction::AddToQueue,
            "remove_from_queue" => HotkeyAction::RemoveFromQueue,
            "clear_queue" => HotkeyAction::ClearQueue,
            "focus_search" => HotkeyAction::FocusSearch,
            "increase_rating" => HotkeyAction::IncreaseRating,
            "decrease_rating" => HotkeyAction::DecreaseRating,
            "get_info" => HotkeyAction::GetInfo,
            "find_similar" => HotkeyAction::FindSimilar,
            "find_top_songs" => HotkeyAction::FindTopSongs,
            "move_track_up" => HotkeyAction::MoveTrackUp,
            "move_track_down" => HotkeyAction::MoveTrackDown,
            "save_queue_as_playlist" => HotkeyAction::SaveQueueAsPlaylist,
            "prev_sort_mode" => HotkeyAction::PrevSortMode,
            "next_sort_mode" => HotkeyAction::NextSortMode,
            "toggle_sort_order" => HotkeyAction::ToggleSortOrder,
            "refresh_view" => HotkeyAction::RefreshView,
            "edit_up" => HotkeyAction::EditUp,
            "edit_down" => HotkeyAction::EditDown,
            "escape" => HotkeyAction::Escape,
            "reset_to_default" => HotkeyAction::ResetToDefault,
            _ => return None,
        })
    }
}

// ============================================================================
// HotkeyConfig — the persisted binding map
// ============================================================================

/// The full set of hotkey bindings, mapping actions to key combinations.
/// Serialized into redb via `SettingsManager`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyConfig {
    /// Action → KeyCombo mapping. Missing entries fall back to defaults.
    bindings: HashMap<HotkeyAction, KeyCombo>,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        let bindings = HotkeyAction::ALL
            .iter()
            .chain(HotkeyAction::RESERVED.iter())
            .map(|action| (*action, action.default_binding()))
            .collect();
        Self { bindings }
    }
}

impl HotkeyConfig {
    /// Get the key combo for a given action (falls back to default if not customized).
    pub fn get_binding(&self, action: &HotkeyAction) -> KeyCombo {
        self.bindings
            .get(action)
            .cloned()
            .unwrap_or_else(|| action.default_binding())
    }

    /// Set or update the binding for an action.
    pub fn set_binding(&mut self, action: HotkeyAction, combo: KeyCombo) {
        self.bindings.insert(action, combo);
    }

    /// Reset a single action to its default binding.
    pub fn reset_binding(&mut self, action: &HotkeyAction) {
        self.bindings.insert(*action, action.default_binding());
    }

    /// Reset all bindings to defaults.
    pub fn reset_all(&mut self) {
        *self = Self::default();
    }

    /// Look up which action a key+modifiers combination is bound to.
    /// Returns `None` if no action matches.
    ///
    /// Searches both explicitly persisted bindings **and** default bindings
    /// for any actions not yet in the user's config (e.g. newly added actions).
    pub fn lookup(
        &self,
        key: &KeyCode,
        shift: bool,
        ctrl: bool,
        alt: bool,
    ) -> Option<HotkeyAction> {
        let combo = KeyCombo {
            key: key.clone(),
            shift,
            ctrl,
            alt,
        };
        // 1. Check explicitly persisted bindings first
        if let Some(found) = self
            .bindings
            .iter()
            .find(|(_, bound_combo)| **bound_combo == combo)
            .map(|(action, _)| *action)
        {
            return Some(found);
        }
        // 2. Fall back to default bindings for any actions not in the map
        //    (e.g. newly added actions that the user hasn't configured yet)
        for action in HotkeyAction::ALL
            .iter()
            .chain(HotkeyAction::RESERVED.iter())
        {
            if !self.bindings.contains_key(action) && action.default_binding() == combo {
                return Some(*action);
            }
        }
        None
    }

    /// Check if a key combo conflicts with an existing binding (excluding a given action).
    /// Returns the conflicting action, if any.
    pub fn find_conflict(&self, combo: &KeyCombo, exclude: &HotkeyAction) -> Option<HotkeyAction> {
        self.bindings
            .iter()
            .find(|(action, bound_combo)| *action != exclude && *bound_combo == combo)
            .map(|(action, _)| *action)
    }

    /// Get all bindings as an iterator.
    pub fn iter(&self) -> impl Iterator<Item = (&HotkeyAction, &KeyCombo)> {
        self.bindings.iter()
    }

    /// Get a reference to the inner bindings map.
    pub fn bindings(&self) -> &HashMap<HotkeyAction, KeyCombo> {
        &self.bindings
    }

    /// Serialize bindings for TOML output.
    /// If `verbose` is false, only non-default bindings are written.
    ///
    /// Returns a `BTreeMap<String, String>` of `action_toml_key → combo_display`.
    /// Using BTreeMap for deterministic key ordering in the TOML file.
    pub fn to_toml_map(&self, verbose: bool) -> std::collections::BTreeMap<String, String> {
        let mut map = std::collections::BTreeMap::new();
        for (action, combo) in &self.bindings {
            if verbose || *combo != action.default_binding() {
                map.insert(action.to_toml_key().to_string(), combo.to_string());
            }
        }
        map
    }

    /// Deserialize from a TOML map of `action_key → combo_string`.
    ///
    /// Starts with defaults, then overrides any entries found in the map.
    /// Unknown action keys or unparseable combos are warned and skipped.
    pub fn from_toml_map(map: &std::collections::BTreeMap<String, String>) -> Self {
        let mut config = Self::default();
        for (action_key, combo_str) in map {
            if let Some(action) = HotkeyAction::from_toml_key(action_key) {
                match combo_str.parse::<KeyCombo>() {
                    Ok(combo) => {
                        config.set_binding(action, combo);
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to parse hotkey combo '{}' for {}: {}",
                            combo_str,
                            action_key,
                            e
                        );
                    }
                }
            } else {
                tracing::warn!("Unknown hotkey action in config.toml: {}", action_key);
            }
        }
        config
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_bindings_complete() {
        let config = HotkeyConfig::default();
        for action in HotkeyAction::ALL {
            assert!(
                config.bindings.contains_key(action),
                "Missing default binding for {action:?}"
            );
        }
        for action in HotkeyAction::RESERVED {
            assert!(
                config.bindings.contains_key(action),
                "Missing default binding for reserved action {action:?}"
            );
        }
        let expected = HotkeyAction::ALL.len() + HotkeyAction::RESERVED.len();
        assert_eq!(
            config.bindings.len(),
            expected,
            "Binding count should match ALL + RESERVED count"
        );
    }

    #[test]
    fn no_duplicate_default_bindings() {
        let config = HotkeyConfig::default();
        let mut seen: HashMap<&KeyCombo, HotkeyAction> = HashMap::new();
        for (action, combo) in &config.bindings {
            if let Some(existing) = seen.get(combo) {
                // ToggleSortOrder uses PageUp — check that PageDown isn't duplicated
                // (it's actually the same binding in our model; if we need PageDown
                // as a separate trigger, we'd add a ToggleSortOrderAlt action)
                panic!("Duplicate binding {combo:?}: both {existing:?} and {action:?}");
            }
            seen.insert(combo, *action);
        }
    }

    #[test]
    fn keycombo_display() {
        assert_eq!(KeyCombo::key(KeyCode::Space).display(), "Space");
        assert_eq!(KeyCombo::shift(KeyCode::Char('l')).display(), "Shift + L");
        assert_eq!(KeyCombo::ctrl(KeyCode::Char('d')).display(), "Ctrl + D");
        assert_eq!(
            KeyCombo {
                key: KeyCode::Char('a'),
                shift: true,
                ctrl: true,
                alt: false
            }
            .display(),
            "Ctrl + Shift + A"
        );
    }

    #[test]
    fn keycombo_serde_roundtrip() {
        let combo = KeyCombo::shift(KeyCode::Char('l'));
        let json = serde_json::to_string(&combo).unwrap();
        let deserialized: KeyCombo = serde_json::from_str(&json).unwrap();
        assert_eq!(combo, deserialized);
    }

    #[test]
    fn config_serde_roundtrip() {
        let config = HotkeyConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: HotkeyConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.bindings.len(), deserialized.bindings.len());
        for action in HotkeyAction::ALL {
            assert_eq!(
                config.get_binding(action),
                deserialized.get_binding(action),
                "Mismatch after roundtrip for {action:?}"
            );
        }
    }

    #[test]
    fn lookup_matches_default() {
        let config = HotkeyConfig::default();
        // Shift+L → ToggleStar
        assert_eq!(
            config.lookup(&KeyCode::Char('l'), true, false, false),
            Some(HotkeyAction::ToggleStar)
        );
        // Space → TogglePlay
        assert_eq!(
            config.lookup(&KeyCode::Space, false, false, false),
            Some(HotkeyAction::TogglePlay)
        );
        // Escape → Escape (reserved action, now in bindings)
        assert_eq!(
            config.lookup(&KeyCode::Escape, false, false, false),
            Some(HotkeyAction::Escape)
        );
        // Delete → ResetToDefault (reserved action)
        assert_eq!(
            config.lookup(&KeyCode::Delete, false, false, false),
            Some(HotkeyAction::ResetToDefault)
        );
        // Unbound key
        assert_eq!(config.lookup(&KeyCode::F12, false, false, false), None);
    }

    #[test]
    fn set_and_lookup_custom_binding() {
        let mut config = HotkeyConfig::default();
        // Rebind ToggleStar from Shift+L to Shift+K
        config.set_binding(
            HotkeyAction::ToggleStar,
            KeyCombo::shift(KeyCode::Char('k')),
        );
        assert_eq!(
            config.lookup(&KeyCode::Char('k'), true, false, false),
            Some(HotkeyAction::ToggleStar)
        );
        // Old binding should no longer match ToggleStar
        assert_ne!(
            config.lookup(&KeyCode::Char('l'), true, false, false),
            Some(HotkeyAction::ToggleStar)
        );
    }

    #[test]
    fn conflict_detection() {
        let config = HotkeyConfig::default();
        // Space is bound to TogglePlay — trying to bind it to ToggleStar should conflict
        let conflict =
            config.find_conflict(&KeyCombo::key(KeyCode::Space), &HotkeyAction::ToggleStar);
        assert_eq!(conflict, Some(HotkeyAction::TogglePlay));

        // Shift+L is bound to ToggleStar — no conflict when checking for ToggleStar itself
        let no_conflict = config.find_conflict(
            &KeyCombo::shift(KeyCode::Char('l')),
            &HotkeyAction::ToggleStar,
        );
        assert_eq!(no_conflict, None);
    }

    #[test]
    fn reset_single_binding() {
        let mut config = HotkeyConfig::default();
        let original = config.get_binding(&HotkeyAction::ToggleStar);
        config.set_binding(HotkeyAction::ToggleStar, KeyCombo::key(KeyCode::F5));
        assert_ne!(config.get_binding(&HotkeyAction::ToggleStar), original);
        config.reset_binding(&HotkeyAction::ToggleStar);
        assert_eq!(config.get_binding(&HotkeyAction::ToggleStar), original);
    }

    #[test]
    fn reset_all_bindings() {
        let mut config = HotkeyConfig::default();
        config.set_binding(HotkeyAction::ToggleStar, KeyCombo::key(KeyCode::F5));
        config.set_binding(HotkeyAction::TogglePlay, KeyCombo::key(KeyCode::F6));
        config.reset_all();
        let default = HotkeyConfig::default();
        for action in HotkeyAction::ALL {
            assert_eq!(
                config.get_binding(action),
                default.get_binding(action),
                "Reset failed for {action:?}"
            );
        }
    }

    #[test]
    fn all_actions_have_category() {
        for action in HotkeyAction::ALL {
            let cat = action.category();
            assert!(!cat.is_empty(), "Action {action:?} has empty category");
        }
    }

    #[test]
    fn all_actions_have_display_name() {
        for action in HotkeyAction::ALL {
            let name = action.display_name();
            assert!(!name.is_empty(), "Action {action:?} has empty display_name");
        }
    }

    // ====================================================================
    // KeyCode::from_name — parsing edge cases
    // ====================================================================

    #[test]
    fn keycode_from_name_single_char_lowercased() {
        // Single uppercase char → stored as lowercase Char variant
        assert_eq!(KeyCode::from_name("A"), Ok(KeyCode::Char('a')));
        assert_eq!(KeyCode::from_name("Z"), Ok(KeyCode::Char('z')));
        // Single lowercase char → stays lowercase
        assert_eq!(KeyCode::from_name("m"), Ok(KeyCode::Char('m')));
    }

    #[test]
    fn keycode_from_name_special_chars() {
        // Punctuation characters recognized as Char variants
        assert_eq!(KeyCode::from_name("/"), Ok(KeyCode::Char('/')));
        assert_eq!(KeyCode::from_name("-"), Ok(KeyCode::Char('-')));
        assert_eq!(KeyCode::from_name("="), Ok(KeyCode::Char('=')));
        assert_eq!(KeyCode::from_name("`"), Ok(KeyCode::Char('`')));
    }

    #[test]
    fn keycode_from_name_named_keys_case_insensitive() {
        // Named keys are case-insensitive
        assert_eq!(KeyCode::from_name("SPACE"), Ok(KeyCode::Space));
        assert_eq!(KeyCode::from_name("space"), Ok(KeyCode::Space));
        assert_eq!(KeyCode::from_name("Space"), Ok(KeyCode::Space));
        assert_eq!(KeyCode::from_name("ESCAPE"), Ok(KeyCode::Escape));
        assert_eq!(KeyCode::from_name("esc"), Ok(KeyCode::Escape));
        assert_eq!(KeyCode::from_name("ESC"), Ok(KeyCode::Escape));
    }

    #[test]
    fn keycode_from_name_arrow_aliases() {
        // Arrow keys via unicode symbols
        assert_eq!(KeyCode::from_name("↑"), Ok(KeyCode::ArrowUp));
        assert_eq!(KeyCode::from_name("↓"), Ok(KeyCode::ArrowDown));
        assert_eq!(KeyCode::from_name("←"), Ok(KeyCode::ArrowLeft));
        assert_eq!(KeyCode::from_name("→"), Ok(KeyCode::ArrowRight));
        // Arrow keys via text names
        assert_eq!(KeyCode::from_name("up"), Ok(KeyCode::ArrowUp));
        assert_eq!(KeyCode::from_name("ArrowUp"), Ok(KeyCode::ArrowUp));
    }

    #[test]
    fn keycode_from_name_rejects_unknown() {
        assert!(KeyCode::from_name("Hyper").is_err());
        assert!(KeyCode::from_name("SuperKey").is_err());
        assert!(KeyCode::from_name("").is_err()); // empty string
    }

    #[test]
    fn keycode_from_name_page_keys_with_space() {
        // "Page Up" with space (matches Display output)
        assert_eq!(KeyCode::from_name("Page Up"), Ok(KeyCode::PageUp));
        assert_eq!(KeyCode::from_name("page down"), Ok(KeyCode::PageDown));
        // Also without space
        assert_eq!(KeyCode::from_name("pageup"), Ok(KeyCode::PageUp));
        assert_eq!(KeyCode::from_name("pagedown"), Ok(KeyCode::PageDown));
    }

    #[test]
    fn keycode_from_name_delete_insert_aliases() {
        assert_eq!(KeyCode::from_name("del"), Ok(KeyCode::Delete));
        assert_eq!(KeyCode::from_name("Delete"), Ok(KeyCode::Delete));
        assert_eq!(KeyCode::from_name("ins"), Ok(KeyCode::Insert));
        assert_eq!(KeyCode::from_name("Insert"), Ok(KeyCode::Insert));
    }

    #[test]
    fn keycode_from_name_f_keys() {
        assert_eq!(KeyCode::from_name("F1"), Ok(KeyCode::F1));
        assert_eq!(KeyCode::from_name("f12"), Ok(KeyCode::F12));
        assert_eq!(KeyCode::from_name("F6"), Ok(KeyCode::F6));
    }

    #[test]
    fn keycode_from_name_whitespace_trimmed() {
        assert_eq!(KeyCode::from_name("  a  "), Ok(KeyCode::Char('a')));
        assert_eq!(KeyCode::from_name(" Space "), Ok(KeyCode::Space));
    }

    // ====================================================================
    // KeyCombo::from_str — parsing edge cases
    // ====================================================================

    #[test]
    fn keycombo_parse_simple_key() {
        let combo: KeyCombo = "Space".parse().unwrap();
        assert_eq!(combo, KeyCombo::key(KeyCode::Space));
    }

    #[test]
    fn keycombo_parse_shift_modifier() {
        let combo: KeyCombo = "Shift + L".parse().unwrap();
        assert_eq!(combo, KeyCombo::shift(KeyCode::Char('l')));
    }

    #[test]
    fn keycombo_parse_ctrl_modifier() {
        let combo: KeyCombo = "Ctrl + D".parse().unwrap();
        assert_eq!(combo, KeyCombo::ctrl(KeyCode::Char('d')));
    }

    #[test]
    fn keycombo_parse_multi_modifier() {
        let combo: KeyCombo = "Ctrl + Shift + A".parse().unwrap();
        assert_eq!(
            combo,
            KeyCombo {
                key: KeyCode::Char('a'),
                shift: true,
                ctrl: true,
                alt: false,
            }
        );
    }

    #[test]
    fn keycombo_parse_alt_modifier() {
        let combo: KeyCombo = "Alt + F4".parse().unwrap();
        assert_eq!(
            combo,
            KeyCombo {
                key: KeyCode::F4,
                shift: false,
                ctrl: false,
                alt: true,
            }
        );
    }

    #[test]
    fn keycombo_parse_control_alias() {
        // "Control" should be accepted as an alias for "Ctrl"
        let combo: KeyCombo = "Control + E".parse().unwrap();
        assert_eq!(combo, KeyCombo::ctrl(KeyCode::Char('e')));
    }

    #[test]
    fn keycombo_parse_arrow_key_named() {
        let combo: KeyCombo = "Shift + Up".parse().unwrap();
        assert_eq!(combo, KeyCombo::shift(KeyCode::ArrowUp));
    }

    #[test]
    fn keycombo_parse_rejects_empty() {
        let result = "".parse::<KeyCombo>();
        assert!(result.is_err());
    }

    #[test]
    fn keycombo_parse_rejects_unknown_modifier() {
        let result = "Super + A".parse::<KeyCombo>();
        assert!(result.is_err());
    }

    #[test]
    fn keycombo_display_roundtrip() {
        // Every KeyCombo should survive display → parse roundtrip
        let combos = vec![
            KeyCombo::key(KeyCode::Space),
            KeyCombo::shift(KeyCode::Char('l')),
            KeyCombo::ctrl(KeyCode::Char('d')),
            KeyCombo::shift(KeyCode::ArrowUp),
            KeyCombo::key(KeyCode::F5),
            KeyCombo::key(KeyCode::Char('/')),
            KeyCombo::key(KeyCode::Char('-')),
            KeyCombo::key(KeyCode::Char('`')),
            KeyCombo {
                key: KeyCode::Char('a'),
                shift: true,
                ctrl: true,
                alt: false,
            },
        ];

        for original in combos {
            let displayed = original.display();
            let parsed: KeyCombo = displayed
                .parse()
                .unwrap_or_else(|e| panic!("Failed to parse '{}': {}", displayed, e));
            assert_eq!(
                original, parsed,
                "Roundtrip failed: display='{}', original={:?}, parsed={:?}",
                displayed, original, parsed
            );
        }
    }

    // ====================================================================
    // TOML roundtrip — custom binding preservation
    // ====================================================================

    #[test]
    fn toml_roundtrip_preserves_custom_bindings() {
        let mut config = HotkeyConfig::default();
        // Customize a few bindings
        config.set_binding(HotkeyAction::ToggleStar, KeyCombo::key(KeyCode::F5));
        config.set_binding(HotkeyAction::AddToQueue, KeyCombo::ctrl(KeyCode::Char('q')));

        // Export to TOML map (verbose=true includes everything)
        let toml_map = config.to_toml_map(true);

        // Re-import
        let restored = HotkeyConfig::from_toml_map(&toml_map);

        // Verify custom bindings survived
        assert_eq!(
            restored.get_binding(&HotkeyAction::ToggleStar),
            KeyCombo::key(KeyCode::F5),
        );
        assert_eq!(
            restored.get_binding(&HotkeyAction::AddToQueue),
            KeyCombo::ctrl(KeyCode::Char('q')),
        );

        // Verify unmodified bindings are still default
        assert_eq!(
            restored.get_binding(&HotkeyAction::TogglePlay),
            HotkeyAction::TogglePlay.default_binding(),
        );
    }

    #[test]
    fn toml_roundtrip_non_verbose_only_custom() {
        let mut config = HotkeyConfig::default();
        config.set_binding(HotkeyAction::ToggleStar, KeyCombo::key(KeyCode::F5));

        // Non-verbose only exports changed bindings
        let toml_map = config.to_toml_map(false);
        assert!(
            toml_map.contains_key("toggle_star"),
            "Custom binding should be exported"
        );
        // Default bindings should NOT be present in non-verbose mode
        assert!(
            !toml_map.contains_key("toggle_play"),
            "Default binding should NOT be exported in non-verbose mode"
        );

        // Re-import should restore the custom binding + defaults for everything else
        let restored = HotkeyConfig::from_toml_map(&toml_map);
        assert_eq!(
            restored.get_binding(&HotkeyAction::ToggleStar),
            KeyCombo::key(KeyCode::F5),
        );
        assert_eq!(
            restored.get_binding(&HotkeyAction::TogglePlay),
            HotkeyAction::TogglePlay.default_binding(),
        );
    }

    #[test]
    fn toml_roundtrip_unknown_action_skipped() {
        // Simulate a config file with a key that doesn't exist in our enum
        let mut map = std::collections::BTreeMap::new();
        map.insert("nonexistent_action".to_string(), "Ctrl + Z".to_string());
        map.insert("toggle_play".to_string(), "F1".to_string());

        let config = HotkeyConfig::from_toml_map(&map);
        // toggle_play should be overridden
        assert_eq!(
            config.get_binding(&HotkeyAction::TogglePlay),
            KeyCombo::key(KeyCode::F1),
        );
        // Everything else should be default (unknown key silently skipped)
        assert_eq!(
            config.get_binding(&HotkeyAction::ToggleStar),
            HotkeyAction::ToggleStar.default_binding(),
        );
    }

    #[test]
    fn toml_roundtrip_unparseable_combo_skipped() {
        // Simulate a config file with a valid action but garbage combo string
        let mut map = std::collections::BTreeMap::new();
        map.insert("toggle_play".to_string(), "???!!!".to_string());

        let config = HotkeyConfig::from_toml_map(&map);
        // Should fall back to default since the combo couldn't be parsed
        assert_eq!(
            config.get_binding(&HotkeyAction::TogglePlay),
            HotkeyAction::TogglePlay.default_binding(),
        );
    }

    // ====================================================================
    // lookup() — fallback for actions missing from user config
    // ====================================================================

    #[test]
    fn lookup_falls_back_to_default_for_missing_actions() {
        // Simulate a config that's missing an action (e.g. newly added after user saved config)
        let mut config = HotkeyConfig::default();
        // Remove an action from the binding map to simulate a stale config
        config.bindings.remove(&HotkeyAction::FindTopSongs);

        // lookup should still find it via the default fallback path
        let default_combo = HotkeyAction::FindTopSongs.default_binding();
        let result = config.lookup(
            &default_combo.key,
            default_combo.shift,
            default_combo.ctrl,
            default_combo.alt,
        );
        assert_eq!(
            result,
            Some(HotkeyAction::FindTopSongs),
            "lookup() should fall back to default binding for actions missing from the map"
        );
    }

    #[test]
    fn lookup_custom_binding_shadows_default() {
        let mut config = HotkeyConfig::default();
        // Rebind ToggleStar from Shift+L to F5
        config.set_binding(HotkeyAction::ToggleStar, KeyCombo::key(KeyCode::F5));

        // F5 should now resolve to ToggleStar
        assert_eq!(
            config.lookup(&KeyCode::F5, false, false, false),
            Some(HotkeyAction::ToggleStar),
        );
        // Shift+L should NOT resolve to ToggleStar anymore
        // (it was removed from ToggleStar; if no other action claims it, returns None)
        assert_ne!(
            config.lookup(&KeyCode::Char('l'), true, false, false),
            Some(HotkeyAction::ToggleStar),
        );
    }

    // ====================================================================
    // Conflict detection with custom bindings
    // ====================================================================

    #[test]
    fn conflict_detection_custom_binding() {
        let mut config = HotkeyConfig::default();
        // Rebind AddToQueue to Space (which is already TogglePlay)
        config.set_binding(HotkeyAction::AddToQueue, KeyCombo::key(KeyCode::Space));

        // Now check: does Space conflict for ToggleStar? Yes — it's bound to AddToQueue
        let conflict =
            config.find_conflict(&KeyCombo::key(KeyCode::Space), &HotkeyAction::ToggleStar);
        // Could be TogglePlay (default) or AddToQueue (custom) depending on iteration order,
        // but it should NOT be None — there IS a conflict
        assert!(
            conflict.is_some(),
            "Space should conflict with an existing binding"
        );
    }

    #[test]
    fn no_conflict_with_self() {
        let config = HotkeyConfig::default();
        // Querying the current binding of TogglePlay should not conflict with TogglePlay itself
        let combo = config.get_binding(&HotkeyAction::TogglePlay);
        assert_eq!(
            config.find_conflict(&combo, &HotkeyAction::TogglePlay),
            None,
            "An action's own binding should not register as a conflict"
        );
    }

    // ====================================================================
    // TOML key roundtrip (to_toml_key / from_toml_key)
    // ====================================================================

    #[test]
    fn toml_key_roundtrip_all_actions() {
        // Every action should survive to_toml_key → from_toml_key
        for action in HotkeyAction::ALL
            .iter()
            .chain(HotkeyAction::RESERVED.iter())
        {
            let key = action.to_toml_key();
            let parsed = HotkeyAction::from_toml_key(key);
            assert_eq!(
                parsed,
                Some(*action),
                "TOML key roundtrip failed for {action:?} (key: {key})"
            );
        }
    }

    #[test]
    fn from_toml_key_returns_none_for_unknown() {
        assert_eq!(HotkeyAction::from_toml_key("doesnt_exist"), None);
        assert_eq!(HotkeyAction::from_toml_key(""), None);
    }

    // ====================================================================
    // Default binding integrity
    // ====================================================================

    #[test]
    fn all_default_bindings_are_findable_via_lookup() {
        let config = HotkeyConfig::default();
        for action in HotkeyAction::ALL
            .iter()
            .chain(HotkeyAction::RESERVED.iter())
        {
            let combo = action.default_binding();
            let found = config.lookup(&combo.key, combo.shift, combo.ctrl, combo.alt);
            assert_eq!(
                found,
                Some(*action),
                "Default binding for {action:?} ({combo}) not found via lookup()"
            );
        }
    }

    #[test]
    fn reserved_actions_not_in_all() {
        // Reserved actions (Escape, ResetToDefault) must NOT appear in ALL
        // (they're excluded from the settings hotkey editor)
        for reserved in HotkeyAction::RESERVED {
            assert!(
                !HotkeyAction::ALL.contains(reserved),
                "Reserved action {reserved:?} must not appear in HotkeyAction::ALL"
            );
        }
    }
}
