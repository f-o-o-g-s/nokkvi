//! `HotkeyAction` — every bindable action and its metadata (display name,
//! description, category, default binding, TOML key).

use serde::{Deserialize, Serialize};

use super::{KeyCode, KeyCombo};

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
    ToggleCrossfade,

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
    Roulette,

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
        HotkeyAction::ToggleCrossfade,
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
        HotkeyAction::Roulette,
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
            HotkeyAction::ToggleCrossfade => "Toggle Crossfade",
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
            HotkeyAction::Roulette => "Roulette",
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
            HotkeyAction::ToggleCrossfade => "Enable or disable gapless crossfading",
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
            HotkeyAction::Roulette => "Spin the wheel and play a random item from the current view",
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
            | HotkeyAction::ToggleEqModal
            | HotkeyAction::ToggleCrossfade => "Playback",

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
            | HotkeyAction::RefreshView
            | HotkeyAction::Roulette => "Sort & View",

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
            HotkeyAction::ToggleCrossfade => KeyCombo::key(KeyCode::Char('f')),
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
            HotkeyAction::Roulette => KeyCombo::ctrl(KeyCode::Char('r')),
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
            HotkeyAction::ToggleCrossfade => "toggle_crossfade",
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
            HotkeyAction::Roulette => "roulette",
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
            "toggle_crossfade" => HotkeyAction::ToggleCrossfade,
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
            "roulette" => HotkeyAction::Roulette,
            "edit_up" => HotkeyAction::EditUp,
            "edit_down" => HotkeyAction::EditDown,
            "escape" => HotkeyAction::Escape,
            "reset_to_default" => HotkeyAction::ResetToDefault,
            _ => return None,
        })
    }
}
