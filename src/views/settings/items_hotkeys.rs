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

/// The subtitle for one hotkey row.
///
/// A row whose combo another action wins never fires: Settings showed the key
/// with no marker at all, and the only notice was a `warn!` in the log. When
/// that is the case the line leads with it, because the usual rebind hint is
/// advice the user cannot act on until the collision is settled.
///
/// "Rebind one of the two" names both rows deliberately. On the canonical
/// shadowed row — one sitting on its own newly-shipped default, which is the
/// case `HotkeyConfig::normalize` cannot carry forward — Del resets it to the
/// value it already has and re-capturing the same key is
/// [`CapturePlan::Blocked`], so the only move that works may be on the OTHER
/// row.
///
/// [`CapturePlan::Blocked`]: nokkvi_data::types::hotkey_config::CapturePlan::Blocked
fn hotkey_subtitle(config: &HotkeyConfig, action: &HotkeyAction) -> String {
    match config.shadowed_by(action) {
        Some(winner) => format!(
            "Does nothing right now: {} fires {}. Rebind one of the two.",
            config.get_binding(action).display(),
            winner.display_name()
        ),
        None => format!(
            "Enter to rebind · Esc cancel · Del reset — {}",
            action.description()
        ),
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

    // Display order for the hotkey sections. This list is the ONLY thing that
    // decides which categories render, so a `category:` string in
    // `define_hotkey_actions!` that is missing here silently drops its rows.

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

            let subtitle = hotkey_subtitle(config, action);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 0.18.x layout a user kept: Previous Sort Mode still on Left (its retired
    /// default) because their own binding holds Shift+Left, so `resolve` hands
    /// Left to it and Seek Backward never fires.
    fn shadowed_config() -> HotkeyConfig {
        let map: std::collections::BTreeMap<String, String> = [
            ("prev_sort_mode".to_string(), "Left".to_string()),
            ("move_track_up".to_string(), "Shift + Left".to_string()),
        ]
        .into_iter()
        .collect();
        HotkeyConfig::from_toml_map(&map)
    }

    #[test]
    fn shadowed_row_subtitle_says_the_key_does_nothing() {
        let config = shadowed_config();
        let subtitle = hotkey_subtitle(&config, &HotkeyAction::SeekBackward);
        assert!(
            subtitle.starts_with("Does nothing right now:"),
            "a row that never fires must say so first, got {subtitle:?}",
        );
        assert!(
            subtitle.contains("Previous Sort Mode"),
            "and must name the action that wins the key, got {subtitle:?}",
        );
        // The remedy has to point at BOTH rows. On the canonical shadowed row —
        // one sitting on its own newly-shipped default — Del resets it to the
        // value it already has and re-capturing the same key is `Blocked`, so
        // advice aimed only at this row would be advice that does nothing.
        assert!(
            subtitle.contains("Rebind one of the two"),
            "got {subtitle:?}",
        );
        assert!(
            !subtitle.contains("Del to reset"),
            "Del is a no-op on a row already on its default, got {subtitle:?}",
        );
    }

    #[test]
    fn unshadowed_row_subtitle_keeps_the_rebind_hint() {
        let config = shadowed_config();
        // Previous Sort Mode is the one that WINS Left, so its own row is fine.
        let subtitle = hotkey_subtitle(&config, &HotkeyAction::PrevSortMode);
        assert!(subtitle.starts_with("Enter to rebind"), "got {subtitle:?}");
    }

    #[test]
    fn every_row_of_a_default_config_keeps_the_rebind_hint() {
        let config = HotkeyConfig::default();
        for action in HotkeyAction::ALL {
            assert!(
                hotkey_subtitle(&config, action).starts_with("Enter to rebind"),
                "{action:?} should not be marked shadowed on a default config",
            );
        }
    }
}
