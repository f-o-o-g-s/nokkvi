//! Hotkey configuration types — maps logical actions to physical key combinations
//!
//! `HotkeyAction` enumerates every bindable action in the application.
//! `KeyCombo` pairs a `KeyCode` with modifier flags (shift, ctrl, alt).
//! `HotkeyConfig` stores the full binding map, supports lookup by key event,
//! conflict detection, and serde for persistence in redb.

use std::{collections::HashMap, fmt};

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
            KeyCode::ArrowUp => write!(f, "↑"),
            KeyCode::ArrowDown => write!(f, "↓"),
            KeyCode::ArrowLeft => write!(f, "←"),
            KeyCode::ArrowRight => write!(f, "→"),
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
    SwitchToSettings,

    // --- Playback ---
    TogglePlay,
    ToggleRandom,
    ToggleRepeat,
    ToggleConsume,
    ToggleSoundEffects,
    CycleVisualization,

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

    // --- Queue reorder ---
    MoveTrackUp,
    MoveTrackDown,

    // --- Queue actions ---
    SaveQueueAsPlaylist,

    // --- Sort & view ---
    PrevSortMode,
    NextSortMode,
    ToggleSortOrder,

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
        HotkeyAction::SwitchToSettings,
        // Playback
        HotkeyAction::TogglePlay,
        HotkeyAction::ToggleRandom,
        HotkeyAction::ToggleRepeat,
        HotkeyAction::ToggleConsume,
        HotkeyAction::ToggleSoundEffects,
        HotkeyAction::CycleVisualization,
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
        // Queue reorder
        HotkeyAction::MoveTrackUp,
        HotkeyAction::MoveTrackDown,
        // Queue actions
        HotkeyAction::SaveQueueAsPlaylist,
        // Sort & view
        HotkeyAction::PrevSortMode,
        HotkeyAction::NextSortMode,
        HotkeyAction::ToggleSortOrder,
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
            HotkeyAction::SwitchToSettings => "Settings",
            HotkeyAction::TogglePlay => "Play / Pause",
            HotkeyAction::ToggleRandom => "Toggle Random",
            HotkeyAction::ToggleRepeat => "Toggle Repeat",
            HotkeyAction::ToggleConsume => "Toggle Consume",
            HotkeyAction::ToggleSoundEffects => "Toggle SFX",
            HotkeyAction::CycleVisualization => "Cycle Visualizer",
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
            HotkeyAction::MoveTrackUp => "Move Track Up",
            HotkeyAction::MoveTrackDown => "Move Track Down",
            HotkeyAction::SaveQueueAsPlaylist => "Save Queue as Playlist",
            HotkeyAction::PrevSortMode => "Previous Sort Mode",
            HotkeyAction::NextSortMode => "Next Sort Mode",
            HotkeyAction::ToggleSortOrder => "Sort Asc / Desc",
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
            HotkeyAction::SwitchToSettings => "Open the settings panel",
            HotkeyAction::TogglePlay => "Play or pause the current track",
            HotkeyAction::ToggleRandom => "Toggle random/shuffle mode",
            HotkeyAction::ToggleRepeat => "Cycle repeat mode (off → one → queue)",
            HotkeyAction::ToggleConsume => "Toggle consume mode (remove after play)",
            HotkeyAction::ToggleSoundEffects => "Enable or disable sound effects",
            HotkeyAction::CycleVisualization => "Cycle visualizer (off → bars → lines)",
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
            HotkeyAction::MoveTrackUp => "Move centered track up in queue",
            HotkeyAction::MoveTrackDown => "Move centered track down in queue",
            HotkeyAction::SaveQueueAsPlaylist => "Open save-as-playlist dialog for the queue",
            HotkeyAction::PrevSortMode => "Cycle sort mode backward",
            HotkeyAction::NextSortMode => "Cycle sort mode forward",
            HotkeyAction::ToggleSortOrder => "Toggle ascending/descending sort",
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
            | HotkeyAction::SwitchToSettings => "Views",

            HotkeyAction::TogglePlay
            | HotkeyAction::ToggleRandom
            | HotkeyAction::ToggleRepeat
            | HotkeyAction::ToggleConsume
            | HotkeyAction::ToggleSoundEffects
            | HotkeyAction::CycleVisualization => "Playback",

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
            | HotkeyAction::AddToQueue
            | HotkeyAction::RemoveFromQueue
            | HotkeyAction::ClearQueue
            | HotkeyAction::MoveTrackUp
            | HotkeyAction::MoveTrackDown
            | HotkeyAction::SaveQueueAsPlaylist => "Item Actions",

            HotkeyAction::PrevSortMode
            | HotkeyAction::NextSortMode
            | HotkeyAction::ToggleSortOrder => "Sort",

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
            HotkeyAction::SwitchToSettings => KeyCombo::key(KeyCode::Char('`')),
            // Playback
            HotkeyAction::TogglePlay => KeyCombo::key(KeyCode::Space),
            HotkeyAction::ToggleRandom => KeyCombo::key(KeyCode::Char('x')),
            HotkeyAction::ToggleRepeat => KeyCombo::key(KeyCode::Char('z')),
            HotkeyAction::ToggleConsume => KeyCombo::key(KeyCode::Char('c')),
            HotkeyAction::ToggleSoundEffects => KeyCombo::key(KeyCode::Char('s')),
            HotkeyAction::CycleVisualization => KeyCombo::key(KeyCode::Char('v')),
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
            // Queue reorder
            HotkeyAction::MoveTrackUp => KeyCombo::shift(KeyCode::ArrowUp),
            HotkeyAction::MoveTrackDown => KeyCombo::shift(KeyCode::ArrowDown),
            // Queue actions
            HotkeyAction::SaveQueueAsPlaylist => KeyCombo::ctrl(KeyCode::Char('s')),
            // Sort & view
            HotkeyAction::PrevSortMode => KeyCombo::key(KeyCode::ArrowLeft),
            HotkeyAction::NextSortMode => KeyCombo::key(KeyCode::ArrowRight),
            HotkeyAction::ToggleSortOrder => KeyCombo::key(KeyCode::PageUp),
            // Global
            HotkeyAction::Escape => KeyCombo::key(KeyCode::Escape),
            HotkeyAction::ResetToDefault => KeyCombo::key(KeyCode::Delete),
        }
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
                "Missing default binding for {:?}",
                action
            );
        }
        for action in HotkeyAction::RESERVED {
            assert!(
                config.bindings.contains_key(action),
                "Missing default binding for reserved action {:?}",
                action
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
                panic!(
                    "Duplicate binding {:?}: both {:?} and {:?}",
                    combo, existing, action
                );
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
                "Mismatch after roundtrip for {:?}",
                action
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
                "Reset failed for {:?}",
                action
            );
        }
    }

    #[test]
    fn all_actions_have_category() {
        for action in HotkeyAction::ALL {
            let cat = action.category();
            assert!(!cat.is_empty(), "Action {:?} has empty category", action);
        }
    }

    #[test]
    fn all_actions_have_display_name() {
        for action in HotkeyAction::ALL {
            let name = action.display_name();
            assert!(
                !name.is_empty(),
                "Action {:?} has empty display_name",
                action
            );
        }
    }
}
