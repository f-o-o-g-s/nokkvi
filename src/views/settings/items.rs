//! Settings item definitions — maps config structs to slot-list-navigable items
//!
//! Each `SettingItem` represents a single configurable value with its current value,
//! default, valid range/options, display label, and category grouping.
//! Builder functions produce `Vec<SettingItem>` per settings tab from live config state.

// `SettingValue`, `SettingMeta`, `SettingItem`, and `SettingsEntry` live in the
// data crate so the macro-emitted `build_<tab>_tab_settings_items` helpers
// (also in the data crate, see `define_settings!`) can return a
// `Vec<SettingsEntry>` directly. Re-exported here so existing
// `crate::views::settings::items::*` import paths keep resolving.
pub(crate) use nokkvi_data::types::{
    setting_item::{SettingItem, SettingMeta, SettingsEntry},
    setting_value::SettingValue,
};

/// Wrapper around the macro-emitted `Vec<SettingsEntry>` returned by
/// `build_<tab>_tab_settings_items`, with a single-use `take(key)` accessor
/// that drains rows in display order.
///
/// The hand-written `items_<tab>.rs` builders need to interleave the
/// `define_settings!`-emitted rows with section headers, conditional rows, and
/// ToggleSet rows. They previously reached for an inline closure to look up
/// each row by key and remove it from the underlying `Vec`. This helper
/// centralizes that pattern so the same panic-on-miss semantics
/// (`"missing macro row for {key}"`) are guaranteed across all three sites
/// (`items_general.rs`, `items_interface.rs`, `items_playback.rs`).
pub(crate) struct MacroRows {
    rows: Vec<SettingsEntry>,
}

impl MacroRows {
    /// Wrap a freshly-built `Vec<SettingsEntry>` (the output of any
    /// `build_<tab>_tab_settings_items` helper).
    pub(crate) fn new(rows: Vec<SettingsEntry>) -> Self {
        Self { rows }
    }

    /// Remove and return the row whose `SettingItem.key` matches `key`.
    ///
    /// Panics with `"missing macro row for {key}"` if no row matches —
    /// matching the original inline closure's behavior. This is the correct
    /// abort semantic for builder code: a missing macro row means the
    /// `define_settings!` table and the items_<tab>.rs display order drifted,
    /// which would otherwise show up as a silently missing row in the UI.
    pub(crate) fn take(&mut self, key: &str) -> SettingsEntry {
        let pos = self
            .rows
            .iter()
            .position(|e| matches!(e, SettingsEntry::Item(it) if it.key.as_ref() == key))
            .unwrap_or_else(|| panic!("missing macro row for {key}"));
        self.rows.remove(pos)
    }
}

// ============================================================================
// Tab Builder Re-exports
// ============================================================================

// Re-export `GeneralSettingsData` so the inline `#[cfg(test)] mod tests`
// below can refer to it by short name (the General tab's tests are written at
// this module's level rather than inside a tab-specific scope). The
// Interface/Playback equivalents are imported directly by the tests that
// need them via `super::super::items_<tab>::...`. Production code reads each
// struct from `nokkvi_data::types::settings_data` directly.
#[cfg(test)]
pub(crate) use super::items_general::GeneralSettingsData;
pub(crate) use super::{
    items_general::build_general_items,
    items_hotkeys::{build_hotkeys_items, key_to_hotkey_action},
    items_interface::build_interface_items,
    items_playback::build_playback_items,
    items_theme::build_theme_items,
    items_visualizer::build_visualizer_items,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::visualizer_config::{VisualizerConfig, keys as vkeys};

    /// Count real items (excluding headers) in a settings entry list
    fn count_items(entries: &[SettingsEntry]) -> usize {
        entries
            .iter()
            .filter(|e| matches!(e, SettingsEntry::Item(_)))
            .count()
    }

    /// Count headers in a settings entry list
    fn count_headers(entries: &[SettingsEntry]) -> usize {
        entries
            .iter()
            .filter(|e| matches!(e, SettingsEntry::Header { .. }))
            .count()
    }

    /// Extract all TOML key paths from settings entries
    fn extract_keys(entries: &[SettingsEntry]) -> Vec<&str> {
        entries
            .iter()
            .filter_map(|e| match e {
                SettingsEntry::Item(item) => Some(item.key.as_ref()),
                SettingsEntry::Header { .. } => None,
            })
            .collect()
    }

    /// Helper: build a 1-row `Vec<SettingsEntry>` containing only a Bool item
    /// with the supplied key, for `MacroRows` test setup.
    fn bool_item(key: &'static str) -> SettingsEntry {
        SettingItem::bool_val(SettingMeta::new(key, "label", "Test"), true, true)
    }

    #[test]
    fn macro_rows_take_returns_matching_entry() {
        let rows = vec![
            bool_item("a"),
            SettingsEntry::Header {
                label: "header",
                icon: "assets/icons/monitor.svg",
            },
            bool_item("b"),
        ];
        let mut macro_rows = MacroRows::new(rows);
        let taken = macro_rows.take("a");
        match taken {
            SettingsEntry::Item(item) => assert_eq!(item.key.as_ref(), "a"),
            SettingsEntry::Header { .. } => panic!("expected Item, got Header"),
        }
        // Underlying rows shrink by 1; "b" is still reachable.
        assert_eq!(macro_rows.rows.len(), 2);
        let still_there = macro_rows.take("b");
        assert!(matches!(still_there, SettingsEntry::Item(it) if it.key.as_ref() == "b"));
    }

    #[test]
    #[should_panic(expected = "missing macro row for nope")]
    fn macro_rows_take_missing_key_panics() {
        let mut macro_rows = MacroRows::new(Vec::new());
        let _ = macro_rows.take("nope");
    }

    #[test]
    #[should_panic(expected = "missing macro row for a")]
    fn macro_rows_take_then_take_same_key_panics() {
        let mut macro_rows = MacroRows::new(vec![bool_item("a")]);
        let _ = macro_rows.take("a");
        let _ = macro_rows.take("a");
    }

    #[test]
    fn visualizer_items_structure() {
        let config = VisualizerConfig::default();
        let theme = nokkvi_data::types::theme_file::ThemeFile::default();
        let entries = build_visualizer_items(&config, &theme, "everforest");

        // Verify section headers
        assert_eq!(
            count_headers(&entries),
            5,
            "Expected 5 sections: General, Bars, Bar Colors (Dark), Bar Colors (Light), Lines"
        );

        // Verify total item count (catches drift when settings are added/removed)
        let item_count = count_items(&entries);
        assert_eq!(
            item_count, 46,
            "Expected 46 visualizer settings items (update this if adding settings)"
        );

        // Spot-check the border_opacity setting (now per-theme under dark/light)
        let keys = extract_keys(&entries);
        assert!(
            keys.contains(&"dark.visualizer.border_opacity"),
            "Missing dark border_opacity key"
        );

        // Verify key paths start with "visualizer." (skip __ sentinel keys)
        let keys = extract_keys(&entries);
        for key in &keys {
            if super::super::sentinel::SentinelKind::from_key(key).is_some() {
                continue;
            }
            assert!(
                key.starts_with("visualizer.")
                    || key.starts_with("dark.")
                    || key.starts_with("light."),
                "All visualizer keys should start with 'visualizer.', 'dark.', or 'light.', got: {key}"
            );
        }
    }

    #[test]
    fn visualizer_items_key_paths() {
        let config = VisualizerConfig::default();
        let theme = nokkvi_data::types::theme_file::ThemeFile::default();
        let entries = build_visualizer_items(&config, &theme, "everforest");
        let keys = extract_keys(&entries);

        // Spot-check critical key paths that config_writer depends on
        assert!(
            keys.contains(&vkeys::NOISE_REDUCTION),
            "Missing noise_reduction key"
        );
        assert!(
            keys.contains(&vkeys::BARS_BAR_SPACING),
            "Missing bar_spacing key"
        );
        assert!(
            keys.contains(&vkeys::BARS_GRADIENT_MODE),
            "Missing gradient_mode key"
        );
        assert!(
            keys.contains(&vkeys::BARS_PEAK_MODE),
            "Missing peak_mode key"
        );
        assert!(
            keys.contains(&vkeys::AUTO_SENSITIVITY),
            "Missing auto_sensitivity key"
        );
        assert!(
            keys.contains(&vkeys::BARS_BAR_DEPTH_3D),
            "Missing bar_depth_3d key"
        );
        assert!(
            keys.contains(&"dark.visualizer.bar_gradient_colors"),
            "Missing dark bar_gradient_colors key"
        );
        assert!(
            keys.contains(&"light.visualizer.bar_gradient_colors"),
            "Missing light bar_gradient_colors key"
        );
    }

    #[test]
    fn visualizer_items_value_types() {
        let config = VisualizerConfig::default();
        let theme = nokkvi_data::types::theme_file::ThemeFile::default();
        let entries = build_visualizer_items(&config, &theme, "everforest");

        for entry in &entries {
            if let SettingsEntry::Item(item) = entry {
                let k = item.key.as_ref();
                if super::super::sentinel::SentinelKind::from_key(k).is_some() {
                    // Sentinel keys (restore buttons, etc.) — skip
                } else if k == vkeys::NOISE_REDUCTION || k == vkeys::MONSTERCAT {
                    // Float settings
                    assert!(
                        matches!(item.value, SettingValue::Float { .. }),
                        "Expected Float for {}, got {:?}",
                        item.key,
                        item.value
                    );
                } else if k == vkeys::WAVES
                    || k == vkeys::BARS_LED_BARS
                    || k == vkeys::AUTO_SENSITIVITY
                {
                    // Bool settings
                    assert!(
                        matches!(item.value, SettingValue::Bool(_)),
                        "Expected Bool for {}, got {:?}",
                        item.key,
                        item.value
                    );
                } else if k == vkeys::BARS_GRADIENT_MODE
                    || k == vkeys::BARS_GRADIENT_ORIENTATION
                    || k == vkeys::BARS_PEAK_GRADIENT_MODE
                    || k == vkeys::BARS_PEAK_MODE
                {
                    // Enum settings
                    assert!(
                        matches!(item.value, SettingValue::Enum { .. }),
                        "Expected Enum for {}, got {:?}",
                        item.key,
                        item.value
                    );
                } else if k.contains("gradient_colors") {
                    // Color arrays
                    assert!(
                        matches!(item.value, SettingValue::ColorArray(_)),
                        "Expected ColorArray for {}, got {:?}",
                        item.key,
                        item.value
                    );
                }
                // Other items are fine
            }
        }
    }

    #[test]
    fn visualizer_items_defaults_match_config() {
        let config = VisualizerConfig::default();
        let theme = nokkvi_data::types::theme_file::ThemeFile::default();
        let entries = build_visualizer_items(&config, &theme, "everforest");

        // When built from defaults, value should equal default for every item
        // (skip __ sentinel keys — they're action buttons, not config values)
        for entry in &entries {
            if let SettingsEntry::Item(item) = entry {
                if super::super::sentinel::SentinelKind::from_key(&item.key).is_some() {
                    continue;
                }
                let val_display = item.value.display();
                let default_display = item.default.display();
                assert_eq!(
                    val_display, default_display,
                    "Setting '{}' value ({}) != default ({}) when built from VisualizerConfig::default()",
                    item.key, val_display, default_display
                );
            }
        }
    }

    #[test]
    fn theme_items_structure() {
        let theme = nokkvi_data::types::theme_file::ThemeFile::default();
        let entries = build_theme_items(&theme, "everforest", false, true, false);

        // Verify section headers
        assert_eq!(
            count_headers(&entries),
            6,
            "Expected 6 theme sections: Select Theme, Appearance, Background Colors, Foreground Colors, Accent Colors, Semantic Colors"
        );

        // Verify we have a reasonable number of items
        let item_count = count_items(&entries);
        assert!(
            item_count >= 23,
            "Expected at least 23 theme items (7 bg + 6 fg + 6 accent + 8 semantic + themes/rounded), got {item_count}"
        );
    }

    #[test]
    fn general_items_structure() {
        // Structure-only test — header/item counts are independent of the
        // string values, so the Default sentinels are fine here. Default
        // boolean fields are all `false`; that's enough since the General tab
        // has no conditional rows.
        let data = GeneralSettingsData::default();
        let entries = build_general_items(&data);

        assert_eq!(
            count_headers(&entries),
            4,
            "Expected 4 sections: Application, Mouse Behavior, System Tray, Account"
        );
        assert_eq!(count_items(&entries), 15, "Expected 15 items");
    }

    #[test]
    fn interface_items_structure() {
        use super::super::items_interface::{InterfaceSettingsData, build_interface_items};
        // `artwork_column_mode = "test-default"` falls through to
        // `ArtworkColumnMode::Auto` (not stretched), so the conditional
        // stretch-fit knob is omitted — matching the 16-item baseline.
        let data = InterfaceSettingsData::default();
        let entries = build_interface_items(&data);

        assert_eq!(
            count_headers(&entries),
            5,
            "Expected 5 sections: Layout, Views, Font, Metadata Strip, Artwork Column"
        );
        assert_eq!(
            count_items(&entries),
            16,
            "Expected 16 items (... + show_labels, field_separator, artwork_column_mode, auto_max_pct, vertical_height_pct); stretched mode adds the fit knob"
        );
    }

    #[test]
    fn interface_items_artwork_column_stretched_adds_fit_knob() {
        use super::super::items_interface::{InterfaceSettingsData, build_interface_items};
        let data = InterfaceSettingsData {
            artwork_column_mode: "Always (Stretched)".into(),
            ..Default::default()
        };
        let entries = build_interface_items(&data);
        assert_eq!(count_items(&entries), 17);
    }

    #[test]
    fn playback_items_structure_off_mode() {
        use super::super::items_playback::{PlaybackSettingsData, build_playback_items};
        // `volume_normalization = "test-default"` matches neither "AGC" nor
        // any ReplayGain label, so the conditional knobs are omitted —
        // matching the 8-item Off baseline.
        let data = PlaybackSettingsData::default();
        let entries = build_playback_items(&data);

        assert_eq!(
            count_headers(&entries),
            3,
            "Expected 3 sections: Playback, Scrobbling, Playlists"
        );
        // Off mode hides AGC level + RG knobs.
        assert_eq!(
            count_items(&entries),
            8,
            "Off mode: crossfade_enabled, crossfade_duration, volume_normalization, scrobbling_enabled, scrobble_threshold, quick_add_to_playlist, default_playlist_name, queue_show_default_playlist"
        );
    }

    #[test]
    fn playback_items_structure_agc_mode_shows_target_level() {
        use super::super::items_playback::{PlaybackSettingsData, build_playback_items};
        let data = PlaybackSettingsData {
            volume_normalization: "AGC".into(),
            ..Default::default()
        };
        let entries = build_playback_items(&data);
        // AGC mode adds the target-level dropdown.
        assert_eq!(count_items(&entries), 9);
    }

    #[test]
    fn playback_items_structure_replay_gain_mode_shows_rg_knobs() {
        use super::super::items_playback::{PlaybackSettingsData, build_playback_items};
        let data = PlaybackSettingsData {
            volume_normalization: "ReplayGain (Track)".into(),
            ..Default::default()
        };
        let entries = build_playback_items(&data);
        // RG modes add 4 knobs: preamp, fallback_db, fallback_to_agc, prevent_clipping.
        assert_eq!(count_items(&entries), 12);
    }

    /// Find the first `SettingItem` matching `key` and assert it carries the
    /// "Enter ↵" dialog/picker flag (`needs_enter_hint == true`). Used by the
    /// regression tests below for tier-0 defect #0.3.
    fn assert_entry_needs_enter_hint(entries: &[SettingsEntry], key: &str) {
        let found = entries
            .iter()
            .find_map(|e| match e {
                SettingsEntry::Item(it) if it.key.as_ref() == key => Some(it),
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing settings entry for key {key}"));
        assert!(
            found.needs_enter_hint,
            "Expected SettingItem '{key}' to have needs_enter_hint=true (dialog/picker row should show \"Enter ↵\" affordance)"
        );
    }

    /// Regression test for tier-0 defect #0.3 — the Font Family row in the
    /// Interface tab opens a picker on Enter, so the renderer must show the
    /// "Enter ↵" hint when it is centered. Prior to the fix, the renderer
    /// gated the hint on a dead-letter key string (`theme.font.family`)
    /// instead of the live key (`font_family`), silently dropping the
    /// affordance.
    #[test]
    fn interface_items_font_family_has_enter_hint() {
        use super::super::items_interface::{InterfaceSettingsData, build_interface_items};
        let data = InterfaceSettingsData::default();
        let entries = build_interface_items(&data);
        assert_entry_needs_enter_hint(&entries, "font_family");
    }

    /// Regression test for tier-0 defect #0.3 — the Default Playlist row in
    /// the Playback tab opens the default-playlist picker on Enter.
    /// Previously the renderer's hint gate omitted this key entirely.
    #[test]
    fn playback_items_default_playlist_name_has_enter_hint() {
        use super::super::items_playback::{PlaybackSettingsData, build_playback_items};
        let data = PlaybackSettingsData::default();
        let entries = build_playback_items(&data);
        assert_entry_needs_enter_hint(&entries, "general.default_playlist_name");
    }

    /// Regression test for tier-0 defect #0.3 — the Local Music Path row in
    /// the General tab opens a free-text input dialog on Enter. This row
    /// was the only one of the three that *did* render the hint pre-fix
    /// (the renderer's string match included it); the test guards against
    /// future regressions when the structural flag replaces the match.
    #[test]
    fn general_items_local_music_path_has_enter_hint() {
        let data = GeneralSettingsData::default();
        let entries = build_general_items(&data);
        assert_entry_needs_enter_hint(&entries, "general.local_music_path");
    }

    #[test]
    fn setting_value_increment_decrement_roundtrip() {
        // Float
        let float_val = SettingValue::Float {
            val: 0.5,
            min: 0.0,
            max: 1.0,
            step: 0.1,
            unit: "",
        };
        let incremented = float_val.increment().unwrap();
        if let SettingValue::Float { val, .. } = incremented {
            assert!((val - 0.6).abs() < 1e-10, "Float increment failed");
        }

        // Int
        let int_val = SettingValue::Int {
            val: 5,
            min: 0,
            max: 10,
            step: 1,
            unit: "",
        };
        let decremented = int_val.decrement().unwrap();
        if let SettingValue::Int { val, .. } = decremented {
            assert_eq!(val, 4, "Int decrement failed");
        }

        // Bool
        let bool_val = SettingValue::Bool(false);
        let toggled = bool_val.increment().unwrap();
        assert!(matches!(toggled, SettingValue::Bool(true)));

        // Enum
        let enum_val = SettingValue::Enum {
            val: "a".to_string(),
            options: vec!["a", "b", "c"],
        };
        let next = enum_val.increment().unwrap();
        if let SettingValue::Enum { val, .. } = next {
            assert_eq!(val, "b");
        }

        // Enum wraps around
        let enum_last = SettingValue::Enum {
            val: "c".to_string(),
            options: vec!["a", "b", "c"],
        };
        let wrapped = enum_last.increment().unwrap();
        if let SettingValue::Enum { val, .. } = wrapped {
            assert_eq!(val, "a", "Enum should wrap around");
        }
    }

    #[test]
    fn setting_value_boundary_clamp() {
        // Float at max stays at max
        let at_max = SettingValue::Float {
            val: 1.0,
            min: 0.0,
            max: 1.0,
            step: 0.1,
            unit: "",
        };
        if let SettingValue::Float { val, .. } = at_max.increment().unwrap() {
            assert!((val - 1.0).abs() < 1e-10, "Should clamp at max");
        }

        // Int at min stays at min
        let at_min = SettingValue::Int {
            val: 0,
            min: 0,
            max: 10,
            step: 1,
            unit: "",
        };
        if let SettingValue::Int { val, .. } = at_min.decrement().unwrap() {
            assert_eq!(val, 0, "Should clamp at min");
        }
    }

    #[test]
    fn non_editable_values_return_none() {
        let hex = SettingValue::HexColor("#ff0000".to_string());
        assert!(hex.increment().is_none());
        assert!(hex.decrement().is_none());
        assert!(!hex.is_editable());

        let text = SettingValue::Text("hello".to_string());
        assert!(text.increment().is_none());
        assert!(!text.is_editable());

        let colors = SettingValue::ColorArray(vec!["#ff0000".to_string()]);
        assert!(colors.increment().is_none());
        assert!(!colors.is_editable());
    }

    #[test]
    fn settings_descriptions_fit_in_footer() {
        let general = crate::views::settings::items_general::GeneralSettingsData::default();
        let interface = crate::views::settings::items_interface::InterfaceSettingsData::default();
        let playback = crate::views::settings::items_playback::PlaybackSettingsData::default();
        let hotkeys = nokkvi_data::types::hotkey_config::HotkeyConfig::default();
        let theme = nokkvi_data::types::theme_file::ThemeFile::default();
        let visualizer = crate::visualizer_config::VisualizerConfig::default();

        let mut all_entries = Vec::new();
        all_entries.extend(crate::views::settings::items_general::build_general_items(
            &general,
        ));
        all_entries
            .extend(crate::views::settings::items_interface::build_interface_items(&interface));
        all_entries.extend(crate::views::settings::items_playback::build_playback_items(&playback));
        all_entries.extend(crate::views::settings::items_hotkeys::build_hotkeys_items(
            &hotkeys,
        ));
        all_entries.extend(crate::views::settings::items_theme::build_theme_items(
            &theme,
            "everforest",
            false,
            true,
            false,
        ));
        all_entries.extend(
            crate::views::settings::items_visualizer::build_visualizer_items(
                &visualizer,
                &theme,
                "everforest",
            ),
        );

        for entry in all_entries {
            if let SettingsEntry::Item(item) = entry
                && let Some(subtitle) = item.subtitle
            {
                let newlines = subtitle.chars().filter(|c| *c == '\n').count();
                assert!(
                    newlines <= 4,
                    "Description for '{}' has {} newlines, which exceeds the max of 4. This will overflow the footer.\nDescription:\n{}",
                    item.label,
                    newlines,
                    subtitle
                );
            }
        }
    }
}
