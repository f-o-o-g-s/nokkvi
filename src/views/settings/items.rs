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
    setting_item::{ActivateKind, SettingItem, SettingMeta, SettingsEntry},
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

    /// Assert that every macro-emitted row was consumed, naming any leftovers.
    ///
    /// Call as the last statement before returning `items` from each
    /// `build_<tab>_items` builder. A leftover row means a `take()` was
    /// forgotten — without this guard the row would silently vanish from the
    /// UI. For rows that should render conditionally, the convention is
    /// take-unconditionally-then-push-conditionally (unused rows just drop),
    /// so consumption stays data-independent and the per-tab structure tests
    /// exercise every key on every branch.
    ///
    /// Panics in debug builds (so tests fail naming the key); release builds
    /// log a warning and render without the leftover rows — mirroring the
    /// panic-in-debug / warn-in-release precedent used elsewhere.
    pub(crate) fn finish(self) {
        let leftovers: Vec<&str> = self
            .rows
            .iter()
            .filter_map(|e| match e {
                SettingsEntry::Item(it) => Some(it.key.as_ref()),
                SettingsEntry::Header { .. } => None,
            })
            .collect();
        if leftovers.is_empty() {
            return;
        }
        if cfg!(debug_assertions) {
            panic!(
                "unconsumed macro rows (take() them, or take unconditionally and push \
                 conditionally): {leftovers:?}"
            );
        }
        tracing::warn!(
            ?leftovers,
            "unconsumed macro rows — rendering the settings tab without them"
        );
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
    use super::{
        super::{
            sentinel::SentinelKind,
            test_support::{
                assert_section_keys, extract_keys, header_labels, palette_prefix_from,
                section_slice,
            },
        },
        *,
    };
    use crate::visualizer_config::{VisualizerConfig, keys as vkeys};

    /// Count real items (excluding headers) in a settings entry list
    fn count_items(entries: &[SettingsEntry]) -> usize {
        entries
            .iter()
            .filter(|e| matches!(e, SettingsEntry::Item(_)))
            .count()
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
    #[should_panic(expected = "unconsumed macro rows")]
    fn macro_rows_finish_panics_on_leftover_keys() {
        let mut macro_rows = MacroRows::new(vec![bool_item("a"), bool_item("forgotten")]);
        let _ = macro_rows.take("a");
        macro_rows.finish();
    }

    #[test]
    fn macro_rows_finish_is_quiet_when_fully_consumed() {
        let mut macro_rows = MacroRows::new(vec![bool_item("a"), bool_item("b")]);
        let _ = macro_rows.take("a");
        let _ = macro_rows.take("b");
        macro_rows.finish();
    }

    #[test]
    fn visualizer_items_structure() {
        let config = VisualizerConfig::default();
        let theme = nokkvi_data::types::theme_file::ThemeFile::default();
        let entries = build_visualizer_items(&config, &theme, "everforest");

        assert_eq!(
            header_labels(&entries),
            vec![
                "Frame",
                "Signal",
                "Bars",
                "Bar Colors (Dark)",
                "Bar Colors (Light)",
                "Lines"
            ],
            "Visualizer tab section headers diverge",
        );

        let restore = SentinelKind::RestoreVisualizer.to_key();
        assert_section_keys(
            &entries,
            "Frame",
            &[
                restore.as_str(),
                vkeys::HEIGHT_PERCENT,
                vkeys::OPACITY,
                vkeys::BLOOM,
                vkeys::BLOOM_INTENSITY,
                vkeys::BEAT_REACTIVITY,
                vkeys::TRAILS,
                vkeys::ECHO,
                vkeys::CRT,
            ],
        );
        assert_section_keys(
            &entries,
            "Signal",
            &[
                vkeys::NOISE_REDUCTION,
                vkeys::LOWER_CUTOFF_FREQ,
                vkeys::HIGHER_CUTOFF_FREQ,
                vkeys::AUTO_SENSITIVITY,
            ],
        );
        assert_section_keys(
            &entries,
            "Bars",
            &[
                vkeys::WAVES,
                vkeys::WAVES_SMOOTHING,
                vkeys::MONSTERCAT,
                vkeys::BARS_MAX_BARS,
                vkeys::BARS_BAR_WIDTH_MIN,
                vkeys::BARS_BAR_WIDTH_MAX,
                vkeys::BARS_BAR_SPACING,
                vkeys::BARS_BORDER_WIDTH,
                vkeys::BARS_LED_BARS,
                vkeys::BARS_LED_SEGMENT_HEIGHT,
                vkeys::BARS_GRADIENT_MODE,
                vkeys::BARS_GRADIENT_ORIENTATION,
                vkeys::BARS_PEAK_GRADIENT_MODE,
                vkeys::BARS_PEAK_MODE,
                vkeys::BARS_PEAK_HOLD_TIME,
                vkeys::BARS_PEAK_FADE_TIME,
                vkeys::BARS_PEAK_FALL_SPEED,
                vkeys::BARS_PEAK_HEIGHT_RATIO,
                vkeys::BARS_BAR_DEPTH_3D,
                vkeys::BARS_FLASH_INTENSITY,
            ],
        );
        assert_section_keys(
            &entries,
            "Bar Colors (Dark)",
            &[
                "dark.visualizer.border_color",
                "dark.visualizer.border_opacity",
                "dark.visualizer.led_border_opacity",
                "dark.visualizer.bar_gradient_colors",
                "dark.visualizer.peak_gradient_colors",
            ],
        );
        assert_section_keys(
            &entries,
            "Bar Colors (Light)",
            &[
                "light.visualizer.border_color",
                "light.visualizer.border_opacity",
                "light.visualizer.led_border_opacity",
                "light.visualizer.bar_gradient_colors",
                "light.visualizer.peak_gradient_colors",
            ],
        );
        assert_section_keys(
            &entries,
            "Lines",
            &[
                vkeys::LINES_POINT_COUNT,
                vkeys::LINES_LINE_THICKNESS,
                vkeys::LINES_OUTLINE_THICKNESS,
                vkeys::LINES_OUTLINE_OPACITY,
                vkeys::LINES_ANIMATION_SPEED,
                vkeys::LINES_GRADIENT_MODE,
                vkeys::LINES_FILL_OPACITY,
                vkeys::LINES_GLOW_INTENSITY,
                vkeys::LINES_MIRROR,
                vkeys::LINES_STYLE,
                vkeys::LINES_BOAT,
            ],
        );

        // Single coarse backstop: the per-section pins above sum to
        // 9 + 4 + 20 + 5 + 5 + 11 = 54. Catches an item landing OUTSIDE the
        // pinned sections (which the section asserts cannot see).
        assert_eq!(count_items(&entries), 54);
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
        let entries = build_theme_items(
            &theme,
            "everforest",
            nokkvi_data::types::player_settings::RoundedMode::Off,
            true,
            false,
        );

        // Chrome Border is its own section so the 1 px hairline gets a
        // Restore Defaults sentinel like every other color group.
        assert_eq!(
            header_labels(&entries),
            vec![
                "Mode",
                "Display",
                "Select Theme",
                "Background Colors",
                "Foreground Colors",
                "Accent Colors",
                "Semantic Colors",
                "Chrome Border"
            ],
            "Theme tab section headers diverge",
        );

        assert_section_keys(&entries, "Mode", &["general.light_mode"]);
        assert_section_keys(
            &entries,
            "Display",
            &["general.rounded_mode", "general.opacity_gradient"],
        );

        // Chrome Border: restore sentinel + the single prefix-dependent
        // border row. `build_theme_items` reads the global light-mode atomic,
        // so derive the prefix from the entries instead of hardcoding "dark".
        let prefix = palette_prefix_from(&entries);
        let restore_border = SentinelKind::RestoreBorder.to_key();
        let border_key = format!("{prefix}.border");
        assert_section_keys(
            &entries,
            "Chrome Border",
            &[restore_border.as_str(), border_key.as_str()],
        );

        // Select Theme stays count-flexible: presets::all_themes() reads the
        // user themes dir, so the preset row count is machine-dependent. Pin
        // the restore sentinel first; every following row must be a
        // __preset_N sentinel.
        let select_keys = extract_keys(&section_slice(&entries, "Select Theme")[1..]);
        let restore_theme = SentinelKind::RestoreTheme.to_key();
        assert_eq!(
            select_keys.first().copied(),
            Some(restore_theme.as_str()),
            "Select Theme must lead with the restore sentinel",
        );
        for key in &select_keys[1..] {
            assert!(
                matches!(
                    SentinelKind::from_key(key),
                    Some(SentinelKind::PresetTheme(_))
                ),
                "Select Theme row '{key}' should be a __preset_N sentinel",
            );
        }

        // The four color sections are exactly pinned in items_theme.rs's own
        // tests — keep one coarse backstop here instead of duplicating them.
        let item_count = count_items(&entries);
        assert!(
            item_count >= 24,
            "Expected at least 24 theme items (6 bg + 6 fg + 5 accent + 8 semantic + 1 border + themes/rounded), got {item_count}",
        );
    }

    #[test]
    fn general_items_structure() {
        // Structure-only test — keys are independent of the string values,
        // so the Default sentinels are fine here. Default boolean fields are
        // all `false`; that's enough since the General tab has no
        // conditional rows.
        let data = GeneralSettingsData::default();
        let entries = build_general_items(&data);

        assert_eq!(
            header_labels(&entries),
            vec![
                "Library",
                "Display",
                "Behavior",
                "Window & Tray",
                "Advanced",
                "Account"
            ],
            "General tab section headers diverge",
        );

        let logout = SentinelKind::Logout.to_key();
        assert_eq!(
            extract_keys(&entries),
            vec![
                "general.library_page_size",
                "general.artwork_resolution",
                "general.show_album_artists_only",
                "general.start_view",
                "general.suppress_library_refresh_toasts",
                "general.enter_behavior",
                "general.stable_viewport",
                "general.auto_follow_playing",
                "general.show_tray_icon",
                "general.close_to_tray",
                "general.local_music_path",
                "general.verbose_config",
                "general.server_url",
                "general.username",
                logout.as_str(),
            ],
            "General tab item keys diverge (order matters)",
        );
    }

    #[test]
    fn interface_items_structure() {
        use super::super::items_interface::{InterfaceSettingsData, build_interface_items};
        // Defaults fall through every conditional gate: `artwork_column_mode
        // = "test-default"` resolves to `ArtworkColumnMode::Auto` (not
        // stretched), `track_info_display` to a non-MiniPlayer mode, and
        // `autohide_toolbar` is off — so the stretch-fit knob, the Visible
        // Controls ToggleSet, and the auto-hide sub-controls are all absent
        // from this baseline. Their gates are pinned by the dedicated tests
        // below and in items_interface.rs.
        let data = InterfaceSettingsData::default();
        let entries = build_interface_items(&data);

        assert_eq!(
            header_labels(&entries),
            vec![
                "Navigation",
                "Slot List",
                "Player Bar",
                "Font",
                "Metadata Strip",
                "Artwork Overlays",
                "Artwork Column"
            ],
            "Interface tab section headers diverge",
        );

        assert_eq!(
            extract_keys(&entries),
            vec![
                "general.nav_layout",
                "general.nav_display_mode",
                "general.slot_row_height",
                "general.slot_text_links",
                "general.scrollbar_visibility",
                "general.autohide_toolbar",
                "general.horizontal_volume",
                "font_family",
                "general.track_info_display",
                "__toggle_strip_fields",
                "general.strip_merged_mode",
                "general.strip_show_labels",
                "general.strip_separator",
                "general.strip_click_action",
                "__toggle_artwork_overlays",
                "general.artwork_column_mode",
                "general.artwork_auto_max_pct",
                "general.artwork_vertical_height_pct",
            ],
            "Interface tab baseline item keys diverge (order matters)",
        );
    }

    #[test]
    fn interface_items_artwork_column_stretched_adds_fit_knob() {
        use super::super::items_interface::{InterfaceSettingsData, build_interface_items};

        // Absent at baseline (Auto mode)…
        let baseline = build_interface_items(&InterfaceSettingsData::default());
        assert!(
            !extract_keys(&baseline).contains(&"general.artwork_column_stretch_fit"),
            "stretch-fit knob must be absent outside stretched modes",
        );

        // …and appended last in stretched mode.
        let data = InterfaceSettingsData {
            artwork_column_mode: "Always (Stretched)".into(),
            ..Default::default()
        };
        let stretched = build_interface_items(&data);
        assert_eq!(
            extract_keys(&stretched).last().copied(),
            Some("general.artwork_column_stretch_fit"),
            "stretched mode appends the fit knob as the final row",
        );
    }

    #[test]
    fn playback_items_structure_off_mode() {
        use super::super::items_playback::{PlaybackSettingsData, build_playback_items};
        // `volume_normalization = "test-default"` matches neither "AGC" nor
        // any ReplayGain label, so the conditional knobs are omitted; the
        // rating reminder is disabled by default so only its enable toggle
        // shows (timing rows are gated off).
        let data = PlaybackSettingsData::default();
        let entries = build_playback_items(&data);

        assert_eq!(
            header_labels(&entries),
            vec![
                "Transitions",
                "Volume Normalization",
                "Scrobbling",
                "Rating Reminder",
                "Playlists"
            ],
            "Playback tab section headers diverge",
        );

        assert_eq!(
            extract_keys(&entries),
            vec![
                "general.crossfade_enabled",
                "general.crossfade_duration",
                "general.bit_perfect",
                "general.rewind_on_previous",
                "general.volume_normalization",
                "general.scrobbling_enabled",
                "general.scrobble_threshold",
                "general.rating_reminder_enabled",
                "general.rating_change_notification_enabled",
                "general.quick_add_to_playlist",
                "general.default_playlist_name",
                "general.queue_show_default_playlist",
            ],
            "Playback tab Off-mode item keys diverge (order matters)",
        );
    }

    #[test]
    fn playback_items_structure_rating_reminder_enabled_shows_timing_rows() {
        use super::super::items_playback::{PlaybackSettingsData, build_playback_items};
        // Enabled + percentage trigger surfaces both the timing dropdown and
        // the percentage knob.
        let percentage = PlaybackSettingsData {
            rating_reminder_enabled: true,
            rating_reminder_trigger: "Percentage Played".into(),
            ..Default::default()
        };
        assert_section_keys(
            &build_playback_items(&percentage),
            "Rating Reminder",
            &[
                "general.rating_reminder_enabled",
                "general.rating_change_notification_enabled",
                "general.rating_reminder_trigger",
                "general.rating_reminder_percent",
            ],
        );

        // On-scrobble trigger hides the percentage knob.
        let scrobble = PlaybackSettingsData {
            rating_reminder_enabled: true,
            rating_reminder_trigger: "On Scrobble".into(),
            ..Default::default()
        };
        assert_section_keys(
            &build_playback_items(&scrobble),
            "Rating Reminder",
            &[
                "general.rating_reminder_enabled",
                "general.rating_change_notification_enabled",
                "general.rating_reminder_trigger",
            ],
        );
    }

    #[test]
    fn playback_items_structure_agc_mode_shows_target_level() {
        use super::super::items_playback::{PlaybackSettingsData, build_playback_items};
        // AGC mode adds the target-level dropdown beneath the mode picker.
        let data = PlaybackSettingsData {
            volume_normalization: "AGC".into(),
            ..Default::default()
        };
        assert_section_keys(
            &build_playback_items(&data),
            "Volume Normalization",
            &[
                "general.volume_normalization",
                "general.normalization_level",
            ],
        );
    }

    #[test]
    fn playback_items_structure_replay_gain_mode_shows_rg_knobs() {
        use super::super::items_playback::{PlaybackSettingsData, build_playback_items};
        // RG modes add the four ReplayGain knobs beneath the mode picker.
        let data = PlaybackSettingsData {
            volume_normalization: "ReplayGain (Track)".into(),
            ..Default::default()
        };
        assert_section_keys(
            &build_playback_items(&data),
            "Volume Normalization",
            &[
                "general.volume_normalization",
                "general.replay_gain_preamp_db",
                "general.replay_gain_fallback_db",
                "general.replay_gain_fallback_to_agc",
                "general.replay_gain_prevent_clipping",
            ],
        );
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

    /// Find the first `SettingItem` matching `key` and return its
    /// `on_activate` intent.
    fn entry_on_activate(entries: &[SettingsEntry], key: &str) -> Option<ActivateKind> {
        entries
            .iter()
            .find_map(|e| match e {
                SettingsEntry::Item(it) if it.key.as_ref() == key => Some(it),
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing settings entry for key {key}"))
            .on_activate
    }

    /// The three dialog/picker rows carry a structural `on_activate` intent so
    /// `EditActivate` dispatches on the enum instead of string-matching the key.
    /// Guards against a key rename silently dropping the action while leaving the
    /// (flag-driven) Enter hint intact.
    #[test]
    fn setting_items_carry_on_activate_intent() {
        use super::super::{
            items_interface::{InterfaceSettingsData, build_interface_items},
            items_playback::{PlaybackSettingsData, build_playback_items},
        };

        let interface = build_interface_items(&InterfaceSettingsData::default());
        assert_eq!(
            entry_on_activate(&interface, "font_family"),
            Some(ActivateKind::FontPicker),
            "font_family row should carry FontPicker activation intent"
        );

        let general = build_general_items(&GeneralSettingsData::default());
        assert_eq!(
            entry_on_activate(&general, "general.local_music_path"),
            Some(ActivateKind::TextInputDialog),
            "local_music_path row should carry TextInputDialog activation intent"
        );

        let playback = build_playback_items(&PlaybackSettingsData::default());
        assert_eq!(
            entry_on_activate(&playback, "general.default_playlist_name"),
            Some(ActivateKind::PlaylistPicker),
            "default_playlist_name row should carry PlaylistPicker activation intent"
        );
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
    fn set_fraction_snaps_to_step_and_clamps_for_float() {
        let base = SettingValue::Float {
            val: 0.0,
            min: 0.0,
            max: 1.0,
            step: 0.05,
            unit: "",
        };

        // 0.0 / 0.5 / 1.0 hit min, midpoint, max exactly.
        for (frac, want) in [(0.0_f32, 0.0_f64), (0.5, 0.5), (1.0, 1.0)] {
            let SettingValue::Float { val, .. } = base.set_fraction(frac).unwrap() else {
                panic!("set_fraction returned non-Float variant");
            };
            assert!(
                (val - want).abs() < 1e-9,
                "frac {frac} -> {val} (want {want})"
            );
        }

        // A fraction that lands between steps snaps to the nearest 0.05 multiple.
        let SettingValue::Float { val, .. } = base.set_fraction(0.43).unwrap() else {
            panic!();
        };
        assert!(
            (val - 0.45).abs() < 1e-9,
            "expected snap to 0.45, got {val}"
        );

        // Out-of-range fractions clamp.
        let SettingValue::Float { val, .. } = base.set_fraction(-0.5).unwrap() else {
            panic!();
        };
        assert!((val - 0.0).abs() < 1e-9);
        let SettingValue::Float { val, .. } = base.set_fraction(1.5).unwrap() else {
            panic!();
        };
        assert!((val - 1.0).abs() < 1e-9);
    }

    #[test]
    fn set_fraction_snaps_to_step_and_clamps_for_int() {
        let base = SettingValue::Int {
            val: 0,
            min: 1000,
            max: 22050,
            step: 100,
            unit: " Hz",
        };

        // Endpoints map to min / max exactly.
        let SettingValue::Int { val, .. } = base.set_fraction(0.0).unwrap() else {
            panic!();
        };
        assert_eq!(val, 1000);
        let SettingValue::Int { val, .. } = base.set_fraction(1.0).unwrap() else {
            panic!();
        };
        assert_eq!(val, 22050);

        // Midpoint snaps to nearest step boundary above min.
        let SettingValue::Int { val, .. } = base.set_fraction(0.5).unwrap() else {
            panic!();
        };
        assert_eq!(val % 100, 0, "expected snap to 100 Hz step, got {val}");

        // Non-incrementable variants reject the call.
        assert!(
            SettingValue::Bool(true).set_fraction(0.5).is_none(),
            "Bool should not accept set_fraction"
        );
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
            nokkvi_data::types::player_settings::RoundedMode::Off,
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
