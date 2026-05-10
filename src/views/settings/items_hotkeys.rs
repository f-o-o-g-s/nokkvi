//! Hotkeys tab setting entries and key/action mapping helpers

use nokkvi_data::types::hotkey_config::{HotkeyAction, HotkeyConfig};

use super::items::{SettingItem, SettingValue, SettingsEntry};

/// Reverse-lookup a settings key string (e.g. "hotkey.toggle_play") to its `HotkeyAction`.
pub(crate) fn key_to_hotkey_action(key: &str) -> Option<HotkeyAction> {
    for action in HotkeyAction::ALL {
        if action.settings_key() == key {
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
                    action.settings_key(),
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
