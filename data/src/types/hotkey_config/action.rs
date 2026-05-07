//! `HotkeyAction` — every bindable action.
//!
//! All per-variant metadata (display name, description, category, default
//! binding, TOML storage key) is declared once via the `define_hotkey_actions!`
//! macro. Adding a hotkey is a single-site edit.

use serde::{Deserialize, Serialize};

use super::{KeyCode, KeyCombo};

/// Generates [`HotkeyAction`] and its metadata methods from a single declarative
/// table. The macro emits:
///
/// - the `pub enum HotkeyAction { … }` definition
/// - `HotkeyAction::ALL` (configurable variants in declaration order)
/// - `HotkeyAction::RESERVED` (reserved variants — fixed bindings, hidden from
///   the settings editor)
/// - `display_name()`, `description()`, `category()`, `default_binding()`,
///   `to_toml_key()`, `from_toml_key()` — each delegated to per-variant data
///
/// Reserved variants always report `"Global"` as their category. Compiler
/// exhaustiveness on every match arm catches missing variants at build time;
/// `from_toml_key` returns `None` for unknown strings.
macro_rules! define_hotkey_actions {
    (
        configurable: [
            $($variant:ident {
                display: $display:literal,
                description: $description:literal,
                category: $category:literal,
                toml_key: $toml_key:literal,
                default: $default:expr,
            }),* $(,)?
        ],
        reserved: [
            $($reserved:ident {
                display: $r_display:literal,
                description: $r_description:literal,
                toml_key: $r_toml_key:literal,
                default: $r_default:expr,
            }),* $(,)?
        ]
        $(,)?
    ) => {
        /// Every action that can be bound to a hotkey.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum HotkeyAction {
            $($variant,)*
            $($reserved,)*
        }

        impl HotkeyAction {
            /// All user-configurable variants in display order.
            pub const ALL: &'static [HotkeyAction] = &[$(HotkeyAction::$variant,)*];

            /// Reserved actions that are NOT user-configurable but still need
            /// bindings for `lookup()` to find them. These are excluded from
            /// `ALL` so they don't appear in the settings hotkey editor, but
            /// they must be present in the `HotkeyConfig::bindings` map.
            pub const RESERVED: &'static [HotkeyAction] = &[$(HotkeyAction::$reserved,)*];

            /// Human-readable name for display in settings UI.
            pub fn display_name(&self) -> &'static str {
                match self {
                    $(HotkeyAction::$variant => $display,)*
                    $(HotkeyAction::$reserved => $r_display,)*
                }
            }

            /// Brief description of what this action does (shown as subtext in settings).
            pub fn description(&self) -> &'static str {
                match self {
                    $(HotkeyAction::$variant => $description,)*
                    $(HotkeyAction::$reserved => $r_description,)*
                }
            }

            /// Category label for grouping in the settings slot list.
            /// Reserved actions always report `"Global"`.
            pub fn category(&self) -> &'static str {
                match self {
                    $(HotkeyAction::$variant => $category,)*
                    $(HotkeyAction::$reserved => "Global",)*
                }
            }

            /// Default key binding for this action.
            pub fn default_binding(&self) -> KeyCombo {
                match self {
                    $(HotkeyAction::$variant => $default,)*
                    $(HotkeyAction::$reserved => $r_default,)*
                }
            }

            /// Convert to a snake_case TOML key string.
            pub fn to_toml_key(self) -> &'static str {
                match self {
                    $(HotkeyAction::$variant => $toml_key,)*
                    $(HotkeyAction::$reserved => $r_toml_key,)*
                }
            }

            /// Parse from a snake_case TOML key string. Returns None for unknown keys.
            pub fn from_toml_key(s: &str) -> Option<HotkeyAction> {
                Some(match s {
                    $($toml_key => HotkeyAction::$variant,)*
                    $($r_toml_key => HotkeyAction::$reserved,)*
                    _ => return None,
                })
            }
        }
    };
}

define_hotkey_actions! {
    configurable: [
        // --- Views ---
        SwitchToQueue {
            display: "Queue",
            description: "Switch to the queue view",
            category: "Views",
            toml_key: "switch_to_queue",
            default: KeyCombo::key(KeyCode::Char('1')),
        },
        SwitchToAlbums {
            display: "Albums",
            description: "Switch to the albums view",
            category: "Views",
            toml_key: "switch_to_albums",
            default: KeyCombo::key(KeyCode::Char('2')),
        },
        SwitchToArtists {
            display: "Artists",
            description: "Switch to the artists view",
            category: "Views",
            toml_key: "switch_to_artists",
            default: KeyCombo::key(KeyCode::Char('3')),
        },
        SwitchToSongs {
            display: "Songs",
            description: "Switch to the songs view",
            category: "Views",
            toml_key: "switch_to_songs",
            default: KeyCombo::key(KeyCode::Char('4')),
        },
        SwitchToGenres {
            display: "Genres",
            description: "Switch to the genres view",
            category: "Views",
            toml_key: "switch_to_genres",
            default: KeyCombo::key(KeyCode::Char('5')),
        },
        SwitchToPlaylists {
            display: "Playlists",
            description: "Switch to the playlists view",
            category: "Views",
            toml_key: "switch_to_playlists",
            default: KeyCombo::key(KeyCode::Char('6')),
        },
        SwitchToRadios {
            display: "Radios",
            description: "Switch to the internet radios view",
            category: "Views",
            toml_key: "switch_to_radios",
            default: KeyCombo::key(KeyCode::Char('7')),
        },
        SwitchToSettings {
            display: "Settings",
            description: "Open the settings panel",
            category: "Views",
            toml_key: "switch_to_settings",
            default: KeyCombo::key(KeyCode::Char('`')),
        },

        // --- Playback ---
        TogglePlay {
            display: "Play / Pause",
            description: "Play or pause the current track",
            category: "Playback",
            toml_key: "toggle_play",
            default: KeyCombo::key(KeyCode::Space),
        },
        ToggleRandom {
            display: "Toggle Random",
            description: "Toggle random/shuffle mode",
            category: "Playback",
            toml_key: "toggle_random",
            default: KeyCombo::key(KeyCode::Char('x')),
        },
        ToggleRepeat {
            display: "Toggle Repeat",
            description: "Cycle repeat mode (off → one → queue)",
            category: "Playback",
            toml_key: "toggle_repeat",
            default: KeyCombo::key(KeyCode::Char('z')),
        },
        ToggleConsume {
            display: "Toggle Consume",
            description: "Toggle consume mode (remove after play)",
            category: "Playback",
            toml_key: "toggle_consume",
            default: KeyCombo::key(KeyCode::Char('c')),
        },
        ToggleSoundEffects {
            display: "Toggle SFX",
            description: "Enable or disable sound effects",
            category: "Playback",
            toml_key: "toggle_sound_effects",
            default: KeyCombo::key(KeyCode::Char('s')),
        },
        CycleVisualization {
            display: "Cycle Visualizer",
            description: "Cycle visualizer (off → bars → lines)",
            category: "Playback",
            toml_key: "cycle_visualization",
            default: KeyCombo::key(KeyCode::Char('v')),
        },
        ToggleEqModal {
            display: "Toggle Equalizer",
            description: "Open or close the 10-band graphic equalizer",
            category: "Playback",
            toml_key: "toggle_eq_modal",
            default: KeyCombo::key(KeyCode::Char('q')),
        },
        ToggleCrossfade {
            display: "Toggle Crossfade",
            description: "Enable or disable gapless crossfading",
            category: "Playback",
            toml_key: "toggle_crossfade",
            default: KeyCombo::key(KeyCode::Char('f')),
        },

        // --- Slot list navigation (category: "Navigation") ---
        SlotListUp {
            display: "Slot List Up",
            description: "Navigate up in the slot list",
            category: "Navigation",
            toml_key: "slot_list_up",
            default: KeyCombo::key(KeyCode::Backspace),
        },
        SlotListDown {
            display: "Slot List Down",
            description: "Navigate down in the slot list",
            category: "Navigation",
            toml_key: "slot_list_down",
            default: KeyCombo::key(KeyCode::Tab),
        },
        Activate {
            display: "Activate / Enter",
            description: "Activate the focused item",
            category: "Navigation",
            toml_key: "activate",
            default: KeyCombo::key(KeyCode::Enter),
        },
        ExpandCenter {
            display: "Expand / Collapse",
            description: "Expand/collapse item. Works in albums/artists/playlists/genres only.",
            category: "Navigation",
            toml_key: "expand_center",
            default: KeyCombo::shift(KeyCode::Enter),
        },

        // --- Browse / panel actions (category: "Navigation") ---
        ToggleBrowsingPanel {
            display: "Library Browser",
            description: "Toggle library browser beside queue",
            category: "Navigation",
            toml_key: "toggle_browsing_panel",
            default: KeyCombo::ctrl(KeyCode::Char('e')),
        },
        CenterOnPlaying {
            display: "Center on Playing",
            description: "Scroll to the currently playing track",
            category: "Navigation",
            toml_key: "center_on_playing",
            default: KeyCombo::shift(KeyCode::Char('c')),
        },
        FocusSearch {
            display: "Search",
            description: "Focus the search input field",
            category: "Navigation",
            toml_key: "focus_search",
            default: KeyCombo::key(KeyCode::Char('/')),
        },

        // --- Item actions ---
        ToggleStar {
            display: "Toggle Love",
            description: "Love/unlove · Navidrome star API",
            category: "Item Actions",
            toml_key: "toggle_star",
            default: KeyCombo::shift(KeyCode::Char('l')),
        },
        AddToQueue {
            display: "Add to Queue",
            description: "Add focused album/song to the queue",
            category: "Item Actions",
            toml_key: "add_to_queue",
            default: KeyCombo::shift(KeyCode::Char('a')),
        },
        RemoveFromQueue {
            display: "Remove from Queue",
            description: "Remove focused item from the queue",
            category: "Item Actions",
            toml_key: "remove_from_queue",
            default: KeyCombo::ctrl(KeyCode::Char('d')),
        },
        ClearQueue {
            display: "Clear Queue",
            description: "Clear the entire queue",
            category: "Item Actions",
            toml_key: "clear_queue",
            default: KeyCombo::shift(KeyCode::Char('d')),
        },
        IncreaseRating {
            display: "Increase Rating",
            description: "Increase rating by one star",
            category: "Item Actions",
            toml_key: "increase_rating",
            default: KeyCombo::key(KeyCode::Char('=')),
        },
        DecreaseRating {
            display: "Decrease Rating",
            description: "Decrease rating by one star",
            category: "Item Actions",
            toml_key: "decrease_rating",
            default: KeyCombo::key(KeyCode::Char('-')),
        },
        GetInfo {
            display: "Get Info",
            description: "Show info for the focused item",
            category: "Item Actions",
            toml_key: "get_info",
            default: KeyCombo::shift(KeyCode::Char('i')),
        },
        FindSimilar {
            display: "Find Similar",
            description: "Find similar songs for the playing track",
            category: "Item Actions",
            toml_key: "find_similar",
            default: KeyCombo::shift(KeyCode::Char('s')),
        },
        FindTopSongs {
            display: "Top Songs",
            description: "Show top songs for the playing track's artist",
            category: "Item Actions",
            toml_key: "find_top_songs",
            default: KeyCombo::shift(KeyCode::Char('t')),
        },
        MoveTrackUp {
            display: "Move Track Up",
            description: "Move centered track up in queue",
            category: "Item Actions",
            toml_key: "move_track_up",
            default: KeyCombo::shift(KeyCode::ArrowUp),
        },
        MoveTrackDown {
            display: "Move Track Down",
            description: "Move centered track down in queue",
            category: "Item Actions",
            toml_key: "move_track_down",
            default: KeyCombo::shift(KeyCode::ArrowDown),
        },
        SaveQueueAsPlaylist {
            display: "Save Queue as Playlist",
            description: "Open save-as-playlist dialog for the queue",
            category: "Item Actions",
            toml_key: "save_queue_as_playlist",
            default: KeyCombo::ctrl(KeyCode::Char('s')),
        },

        // --- Sort & view ---
        PrevSortMode {
            display: "Previous Sort Mode",
            description: "Cycle sort mode backward",
            category: "Sort & View",
            toml_key: "prev_sort_mode",
            default: KeyCombo::key(KeyCode::ArrowLeft),
        },
        NextSortMode {
            display: "Next Sort Mode",
            description: "Cycle sort mode forward",
            category: "Sort & View",
            toml_key: "next_sort_mode",
            default: KeyCombo::key(KeyCode::ArrowRight),
        },
        ToggleSortOrder {
            display: "Sort Asc / Desc",
            description: "Toggle ascending/descending sort",
            category: "Sort & View",
            toml_key: "toggle_sort_order",
            default: KeyCombo::key(KeyCode::PageUp),
        },
        RefreshView {
            display: "Refresh View",
            description: "Reload current view data from the server",
            category: "Sort & View",
            toml_key: "refresh_view",
            default: KeyCombo::key(KeyCode::Char('r')),
        },
        Roulette {
            display: "Roulette",
            description: "Spin the wheel and play a random item from the current view",
            category: "Sort & View",
            toml_key: "roulette",
            default: KeyCombo::ctrl(KeyCode::Char('r')),
        },

        // --- Settings edit ---
        EditUp {
            display: "Edit Value Up",
            description: "Toggle setting on · enable field",
            category: "Settings Edit",
            toml_key: "edit_up",
            default: KeyCombo::key(KeyCode::ArrowUp),
        },
        EditDown {
            display: "Edit Value Down",
            description: "Toggle setting off · disable field",
            category: "Settings Edit",
            toml_key: "edit_down",
            default: KeyCombo::key(KeyCode::ArrowDown),
        },
    ],
    reserved: [
        Escape {
            display: "Escape / Back",
            description: "Close overlay, clear search, or go back",
            toml_key: "escape",
            default: KeyCombo::key(KeyCode::Escape),
        },
        ResetToDefault {
            display: "Reset to Default",
            description: "Reset focused setting to its default value",
            toml_key: "reset_to_default",
            default: KeyCombo::key(KeyCode::Delete),
        },
    ]
}
