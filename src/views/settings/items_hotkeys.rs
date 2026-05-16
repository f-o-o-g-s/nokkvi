//! Hotkeys tab setting entries and key/action mapping helpers

use nokkvi_data::types::hotkey_config::{HotkeyAction, HotkeyConfig};

use super::{
    items::{SettingItem, SettingMeta, SettingValue, SettingsEntry},
    sentinel::SentinelKind,
};

/// Reverse-lookup a settings key string (e.g. "hotkey.toggle_play") to its `HotkeyAction`.
pub(crate) fn key_to_hotkey_action(key: &str) -> Option<HotkeyAction> {
    for action in HotkeyAction::ALL {
        if action.settings_key() == key {
            return Some(*action);
        }
    }
    None
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
                SettingMeta::new(SentinelKind::RestoreAllHotkeys.to_key(), "⟲ Restore Defaults", cat)
                    .with_subtitle(
                        "Restore all hotkey bindings to their defaults. Does not affect other settings.",
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
                SettingMeta::new(action.settings_key(), action.display_name(), cat)
                    .with_subtitle(subtitle),
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
