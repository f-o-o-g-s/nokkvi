//! Hotkeys tab setting entries and key/action mapping helpers

use nokkvi_data::types::hotkey_config::{HotkeyAction, HotkeyConfig};

use super::items::{SettingItem, SettingValue, SettingsEntry};

/// Reverse-lookup a settings key string (e.g. "hotkey.toggle_play") to its `HotkeyAction`.
pub(crate) fn key_to_hotkey_action(key: &str) -> Option<HotkeyAction> {
    for action in HotkeyAction::ALL {
        if hotkey_action_to_key(action) == key {
            return Some(*action);
        }
    }
    None
}

/// Check whether a key starts with `__restore_` (restore-defaults sentinel).
pub(crate) fn is_restore_key(key: &str) -> bool {
    key.starts_with("__restore_")
}

/// Check whether a key starts with `__preset_` (inline preset sentinel).
pub(crate) fn is_preset_key(key: &str) -> bool {
    key.starts_with("__preset_")
}

/// Check whether a key starts with `__action_` (action button sentinel).
pub(crate) fn is_action_key(key: &str) -> bool {
    key.starts_with("__action_")
}

/// Extract the preset index from a `__preset_N` key.
pub(crate) fn preset_key_index(key: &str) -> Option<usize> {
    key.strip_prefix("__preset_").and_then(|s| s.parse().ok())
}

/// Map a HotkeyAction to its settings key string.
fn hotkey_action_to_key(action: &HotkeyAction) -> &'static str {
    match action {
        HotkeyAction::SwitchToQueue => "hotkey.switch_to_queue",
        HotkeyAction::SwitchToAlbums => "hotkey.switch_to_albums",
        HotkeyAction::SwitchToArtists => "hotkey.switch_to_artists",
        HotkeyAction::SwitchToSongs => "hotkey.switch_to_songs",
        HotkeyAction::SwitchToGenres => "hotkey.switch_to_genres",
        HotkeyAction::SwitchToPlaylists => "hotkey.switch_to_playlists",
        HotkeyAction::SwitchToRadios => "hotkey.switch_to_radios",
        HotkeyAction::SwitchToSettings => "hotkey.switch_to_settings",
        HotkeyAction::TogglePlay => "hotkey.toggle_play",
        HotkeyAction::ToggleRandom => "hotkey.toggle_random",
        HotkeyAction::ToggleRepeat => "hotkey.toggle_repeat",
        HotkeyAction::ToggleConsume => "hotkey.toggle_consume",
        HotkeyAction::ToggleSoundEffects => "hotkey.toggle_sfx",
        HotkeyAction::CycleVisualization => "hotkey.cycle_vis",
        HotkeyAction::SlotListUp => "hotkey.slot_list_up",
        HotkeyAction::SlotListDown => "hotkey.slot_list_down",
        HotkeyAction::Activate => "hotkey.activate",
        HotkeyAction::ExpandCenter => "hotkey.expand_center",
        HotkeyAction::ToggleBrowsingPanel => "hotkey.toggle_browsing_panel",
        HotkeyAction::CenterOnPlaying => "hotkey.center_playing",
        HotkeyAction::ToggleStar => "hotkey.toggle_star",
        HotkeyAction::AddToQueue => "hotkey.add_to_queue",
        HotkeyAction::RemoveFromQueue => "hotkey.remove_from_queue",
        HotkeyAction::ClearQueue => "hotkey.clear_queue",
        HotkeyAction::FocusSearch => "hotkey.focus_search",
        HotkeyAction::IncreaseRating => "hotkey.increase_rating",
        HotkeyAction::DecreaseRating => "hotkey.decrease_rating",
        HotkeyAction::GetInfo => "hotkey.get_info",
        HotkeyAction::FindSimilar => "hotkey.find_similar",
        HotkeyAction::FindTopSongs => "hotkey.find_top_songs",
        HotkeyAction::MoveTrackUp => "hotkey.move_track_up",
        HotkeyAction::MoveTrackDown => "hotkey.move_track_down",
        HotkeyAction::SaveQueueAsPlaylist => "hotkey.save_queue_as_playlist",
        HotkeyAction::PrevSortMode => "hotkey.cycle_view_left",
        HotkeyAction::NextSortMode => "hotkey.cycle_view_right",
        HotkeyAction::ToggleSortOrder => "hotkey.toggle_sort_order",
        HotkeyAction::RefreshView => "hotkey.refresh_view",
        HotkeyAction::Roulette => "hotkey.roulette",
        HotkeyAction::EditUp => "hotkey.edit_up",
        HotkeyAction::EditDown => "hotkey.edit_down",
        HotkeyAction::Escape => "hotkey.escape",
        HotkeyAction::ResetToDefault => "hotkey.reset_to_default",
        HotkeyAction::ToggleEqModal => "hotkey.toggle_eq_modal",
        HotkeyAction::ToggleCrossfade => "hotkey.toggle_crossfade",
    }
}

/// Build settings entries for the Hotkeys tab from live hotkey config.
///
/// Groups actions by category (Views, Settings, Playback, Navigation, Item Actions, Sort)
/// and displays each action's bound key combo.
pub(crate) fn build_hotkeys_items(config: &HotkeyConfig) -> Vec<SettingsEntry> {
    // Per-category icons
    const NAV: &str = "assets/icons/compass.svg";
    const PLAY: &str = "assets/icons/disc-3.svg";
    const NAVIGATION: &str = "assets/icons/unfold-vertical.svg";
    const ITEM: &str = "assets/icons/library-big.svg";
    const SORT: &str = "assets/icons/list-filter.svg";
    const EDIT: &str = "assets/icons/settings.svg";

    /// Map category label to its icon path
    fn cat_icon(cat: &str) -> &'static str {
        match cat {
            "Views" => NAV,
            "Playback" => PLAY,
            "Navigation" => NAVIGATION,
            "Item Actions" => ITEM,
            "Sort & View" => SORT,
            "Settings Edit" => EDIT,
            _ => NAV,
        }
    }

    // Category order (must match ALL_SECTION_LABELS)
    let categories = [
        "Views",
        "Playback",
        "Navigation",
        "Item Actions",
        "Sort & View",
        "Settings Edit",
    ];

    let mut entries = Vec::new();
    let mut restore_pushed = false;

    for &cat in &categories {
        let icon = cat_icon(cat);
        entries.push(SettingsEntry::Header { label: cat, icon });

        // Place restore defaults as the first item (under the first header)
        if !restore_pushed {
            restore_pushed = true;
            entries.push(SettingItem::text(
                meta!(
                    "__restore_all_hotkeys",
                    "⟲ Restore Defaults",
                    cat,
                    "Restore all hotkey bindings to their defaults. Does not affect other settings."
                ),
                "Press Enter",
                "Press Enter",
            ));
        }

        for action in HotkeyAction::ALL {
            if action.category() != cat {
                continue;
            }

            let combo_display = config.get_binding(action).display();
            let default_display = action.default_binding().display();

            let subtitle = format!(
                "Enter to rebind · Esc cancel · Del reset — {}",
                action.description()
            );
            let mut entry = SettingItem::from_meta(
                meta!(
                    hotkey_action_to_key(action),
                    action.display_name(),
                    cat,
                    // Leak the dynamic subtitle string so the &'static str requirement is met.
                    // Bounded by HotkeyAction::ALL count (~35 actions), each leaked once.
                    &*Box::leak(subtitle.into_boxed_str())
                ),
                SettingValue::Hotkey(combo_display),
                SettingValue::Hotkey(default_display),
            );

            // Set inline label icons for star/rating actions
            if let SettingsEntry::Item(ref mut item) = entry {
                match action {
                    HotkeyAction::ToggleStar => {
                        item.label_icon = Some("assets/icons/heart.svg");
                    }
                    HotkeyAction::IncreaseRating | HotkeyAction::DecreaseRating => {
                        item.label_icon = Some("assets/icons/star.svg");
                    }
                    _ => {}
                }
            }

            entries.push(entry);
        }
    }

    entries
}
