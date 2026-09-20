//! Tests for hotkey config types.

use std::collections::HashMap;

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
fn ctrl_enter_maps_to_shuffle_play_without_shadowing_enter_or_shift_enter() {
    let config = HotkeyConfig::default();
    // Ctrl+Enter → ShufflePlay (new)
    assert_eq!(
        config.lookup(&KeyCode::Enter, false, true, false),
        Some(HotkeyAction::ShufflePlay)
    );
    // Plain Enter → Activate (unchanged)
    assert_eq!(
        config.lookup(&KeyCode::Enter, false, false, false),
        Some(HotkeyAction::Activate)
    );
    // Shift+Enter → ExpandCenter (unchanged)
    assert_eq!(
        config.lookup(&KeyCode::Enter, true, false, false),
        Some(HotkeyAction::ExpandCenter)
    );
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
    let conflict = config.find_conflict(&KeyCombo::key(KeyCode::Space), &HotkeyAction::ToggleStar);
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
            .unwrap_or_else(|e| panic!("Failed to parse '{displayed}': {e}"));
        assert_eq!(
            original, parsed,
            "Roundtrip failed: display='{displayed}', original={original:?}, parsed={parsed:?}"
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
    let conflict = config.find_conflict(&KeyCombo::key(KeyCode::Space), &HotkeyAction::ToggleStar);
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

// ====================================================================
// Shared-combo resolution — deterministic across launches
// ====================================================================

/// How many freshly built configs a determinism assertion walks. Each
/// `HashMap` gets its own iteration order, so a resolution rule that leaned on
/// map order would pass once and fail on a later launch.
const SHARED_COMBO_TRIALS: usize = 64;

#[test]
fn shared_combo_prefers_the_action_the_user_moved() {
    // ToggleStar (Shift+L by default) is declared BEFORE AddToQueue in
    // HotkeyAction::ALL, so declaration order alone would pick ToggleStar.
    // AddToQueue sitting on Shift+L is the user's explicit choice and wins.
    for _ in 0..SHARED_COMBO_TRIALS {
        let mut config = HotkeyConfig::default();
        config.set_binding(
            HotkeyAction::AddToQueue,
            KeyCombo::shift(KeyCode::Char('l')),
        );
        assert_eq!(
            config.lookup(&KeyCode::Char('l'), true, false, false),
            Some(HotkeyAction::AddToQueue),
            "a user-chosen binding must beat another action still sitting on its default"
        );
    }
}

#[test]
fn shared_combo_between_two_moved_actions_falls_back_to_declaration_order() {
    // Neither binding is a default, so the tie breaks on HotkeyAction::ALL
    // order: ToggleStar is declared before AddToQueue.
    for _ in 0..SHARED_COMBO_TRIALS {
        let mut config = HotkeyConfig::default();
        config.set_binding(HotkeyAction::ToggleStar, KeyCombo::key(KeyCode::F5));
        config.set_binding(HotkeyAction::AddToQueue, KeyCombo::key(KeyCode::F5));
        assert_eq!(
            config.lookup(&KeyCode::F5, false, false, false),
            Some(HotkeyAction::ToggleStar),
            "two moved actions on one combo resolve in declaration order"
        );
    }
}

#[test]
fn reserved_action_wins_a_shared_combo() {
    // A configurable action parked on Escape must not shadow the reserved one.
    for _ in 0..SHARED_COMBO_TRIALS {
        let mut config = HotkeyConfig::default();
        config.set_binding(HotkeyAction::ToggleStar, KeyCombo::key(KeyCode::Escape));
        assert_eq!(
            config.lookup(&KeyCode::Escape, false, false, false),
            Some(HotkeyAction::Escape),
            "reserved actions resolve ahead of every configurable one"
        );
    }
}

#[test]
fn find_conflict_names_the_action_lookup_would_fire() {
    for _ in 0..SHARED_COMBO_TRIALS {
        let mut config = HotkeyConfig::default();
        // AddToQueue takes ToggleStar's default combo.
        config.set_binding(
            HotkeyAction::AddToQueue,
            KeyCombo::shift(KeyCode::Char('l')),
        );
        let combo = KeyCombo::shift(KeyCode::Char('l'));
        let winner = config.lookup(&combo.key, combo.shift, combo.ctrl, combo.alt);
        assert_eq!(winner, Some(HotkeyAction::AddToQueue));
        // Rebinding a third action onto that combo must report the same winner.
        assert_eq!(
            config.find_conflict(&combo, &HotkeyAction::TogglePlay),
            winner,
            "find_conflict must name the action lookup() would fire"
        );
    }
}

#[test]
fn find_conflict_sees_an_action_still_on_its_default() {
    // Nothing was customized: the conflict is ToggleStar's own default.
    let config = HotkeyConfig::default();
    assert_eq!(
        config.find_conflict(
            &KeyCombo::shift(KeyCode::Char('l')),
            &HotkeyAction::TogglePlay
        ),
        Some(HotkeyAction::ToggleStar)
    );
}

// ====================================================================
// Seek keys, the moved sort defaults, and the retired-default rule
// ====================================================================

/// Build a `[hotkeys]` map from `(toml_key, combo)` pairs.
fn toml_map(pairs: &[(&str, &str)]) -> std::collections::BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

/// Assert the shipped layout: bare arrows seek, Shift+arrows cycle the sort.
fn assert_new_arrow_layout(config: &HotkeyConfig, context: &str) {
    assert_eq!(
        config.lookup(&KeyCode::ArrowLeft, false, false, false),
        Some(HotkeyAction::SeekBackward),
        "{context}: bare Left must seek backward"
    );
    assert_eq!(
        config.lookup(&KeyCode::ArrowRight, false, false, false),
        Some(HotkeyAction::SeekForward),
        "{context}: bare Right must seek forward"
    );
    assert_eq!(
        config.lookup(&KeyCode::ArrowLeft, true, false, false),
        Some(HotkeyAction::PrevSortMode),
        "{context}: Shift+Left must cycle the sort mode backward"
    );
    assert_eq!(
        config.lookup(&KeyCode::ArrowRight, true, false, false),
        Some(HotkeyAction::NextSortMode),
        "{context}: Shift+Right must cycle the sort mode forward"
    );
}

#[test]
fn default_bindings_put_seek_on_the_bare_arrows() {
    assert_new_arrow_layout(&HotkeyConfig::default(), "defaults");
}

#[test]
fn a_verbose_file_from_before_the_move_normalizes_to_the_new_layout() {
    // What `verbose_config = "on"` wrote before the seek actions existed: the
    // sort cycle explicitly on the bare arrows, and no seek lines at all.
    let mut map = HotkeyConfig::default().to_toml_map(true);
    map.insert("prev_sort_mode".to_string(), "Left".to_string());
    map.insert("next_sort_mode".to_string(), "Right".to_string());
    map.remove("seek_backward");
    map.remove("seek_forward");

    let (config, changed) = HotkeyConfig::from_toml_map_reporting(&map);
    assert_new_arrow_layout(&config, "a verbose file from before the move");
    assert!(
        changed,
        "the file still claims the old defaults, so it must be rewritten once"
    );
}

#[test]
fn a_sparse_file_naming_only_the_old_sort_defaults_normalizes_the_same_way() {
    let map = toml_map(&[("prev_sort_mode", "Left"), ("next_sort_mode", "Right")]);
    let (config, changed) = HotkeyConfig::from_toml_map_reporting(&map);
    assert_new_arrow_layout(&config, "a sparse file naming the old defaults");
    assert!(changed);
}

#[test]
fn an_empty_hotkeys_table_needs_no_rewrite() {
    let (config, changed) = HotkeyConfig::from_toml_map_reporting(&toml_map(&[]));
    assert_new_arrow_layout(&config, "an empty table");
    assert!(
        !changed,
        "nothing claimed a retired default, so nothing may be rewritten"
    );
}

#[test]
fn a_deliberate_swap_back_onto_the_bare_arrows_survives_a_restart() {
    // What the capture UI writes when the user takes bare Left for Previous
    // Sort Mode: the steal SWAPS, so seek lands on the sort cycle's old combo.
    // Nothing collides, so the retired-default rule must leave this alone.
    let map = toml_map(&[
        ("prev_sort_mode", "Left"),
        ("next_sort_mode", "Right"),
        ("seek_backward", "Shift + Left"),
        ("seek_forward", "Shift + Right"),
    ]);
    let (config, changed) = HotkeyConfig::from_toml_map_reporting(&map);
    assert_eq!(
        config.lookup(&KeyCode::ArrowLeft, false, false, false),
        Some(HotkeyAction::PrevSortMode),
        "a user who put the sort cycle back on bare Left keeps it"
    );
    assert_eq!(
        config.lookup(&KeyCode::ArrowLeft, true, false, false),
        Some(HotkeyAction::SeekBackward)
    );
    assert!(!changed, "a config that already agrees needs no rewrite");
}

#[test]
fn both_actions_explicitly_on_bare_left_resolve_to_seek() {
    // A genuine conflict — the user named both. The retired-default rule
    // breaks the tie toward the action whose default has NOT moved.
    let map = toml_map(&[("prev_sort_mode", "Left"), ("seek_backward", "Left")]);
    let (config, changed) = HotkeyConfig::from_toml_map_reporting(&map);
    assert_eq!(
        config.lookup(&KeyCode::ArrowLeft, false, false, false),
        Some(HotkeyAction::SeekBackward)
    );
    assert_eq!(
        config.get_binding(&HotkeyAction::PrevSortMode),
        KeyCombo::shift(KeyCode::ArrowLeft),
        "the action still sitting on its RETIRED default is the one that yields"
    );
    assert!(changed);
}

#[test]
fn a_redb_config_missing_the_seek_actions_normalizes_the_same_way() {
    // The shape a pre-upgrade redb blob deserializes into: the seek variants
    // did not exist, so their keys are simply absent from the map.
    let mut config = HotkeyConfig::default();
    config.bindings.remove(&HotkeyAction::SeekBackward);
    config.bindings.remove(&HotkeyAction::SeekForward);
    config.set_binding(
        HotkeyAction::PrevSortMode,
        KeyCombo::key(KeyCode::ArrowLeft),
    );
    config.set_binding(
        HotkeyAction::NextSortMode,
        KeyCombo::key(KeyCode::ArrowRight),
    );

    assert!(config.normalize(), "the stale layout must report a change");
    assert_new_arrow_layout(&config, "a pre-upgrade redb config");
}

#[test]
fn a_user_binding_that_merely_equals_a_retired_default_is_left_alone() {
    // Nothing else claims Left, so there is no conflict to resolve — the rule
    // must not "correct" a binding the user is happily using.
    let mut config = HotkeyConfig::default();
    config.set_binding(
        HotkeyAction::PrevSortMode,
        KeyCombo::key(KeyCode::ArrowLeft),
    );
    config.set_binding(HotkeyAction::SeekBackward, KeyCombo::key(KeyCode::F7));

    assert!(!config.normalize());
    assert_eq!(
        config.get_binding(&HotkeyAction::PrevSortMode),
        KeyCombo::key(KeyCode::ArrowLeft)
    );
}

#[test]
fn a_retired_default_stands_pat_when_its_new_default_is_taken() {
    // A 0.18.x `verbose_config = "on"` file that also moved Move Track Up onto
    // Shift+Left. Migrating Previous Sort Mode there would hand Shift+Left to
    // Move Track Up (a user binding beats one on its default) and leave
    // Previous Sort Mode with no working key at all.
    let mut config = HotkeyConfig::default();
    config.set_binding(
        HotkeyAction::MoveTrackUp,
        KeyCombo::shift(KeyCode::ArrowLeft),
    );
    config.set_binding(
        HotkeyAction::PrevSortMode,
        KeyCombo::key(KeyCode::ArrowLeft),
    );

    let changed = config.normalize();

    assert_eq!(
        config.get_binding(&HotkeyAction::PrevSortMode),
        KeyCombo::key(KeyCode::ArrowLeft),
        "it must stay where it still works rather than move onto a taken combo"
    );
    assert_eq!(
        config.lookup(&KeyCode::ArrowLeft, false, false, false),
        Some(HotkeyAction::PrevSortMode),
        "the pre-upgrade layout keeps working; the NEWLY-shipped action is the one shadowed"
    );
    assert_eq!(
        config.lookup(&KeyCode::ArrowLeft, true, false, false),
        Some(HotkeyAction::MoveTrackUp),
        "the user's own binding is untouched"
    );
    assert!(
        !changed,
        "nothing moved, so nothing may be rewritten to the user's config.toml"
    );
}

// ============================================================================
// Shared combos: shadowed_by + plan_capture
// ============================================================================
//
// 0.18.x moved a shipped default (bare Left/Right became Seek Backward /
// Seek Forward, the sort cycle went to Shift+Left/Right). `normalize` carries
// forward the user who is still sitting on the retired combo — it does nothing
// for the other direction, a newly-shipped default landing on a key the user
// already claimed. Those users end up with two actions on one combo: the old
// layout keeps working, the new rows show a key they never get, and only a
// `warn!` in the log says so.

/// Build a config from TOML overrides, exactly as a `config.toml` load does
/// (so `normalize` runs and the fixtures describe real files).
fn config_with(overrides: &[(&str, &str)]) -> HotkeyConfig {
    let map: std::collections::BTreeMap<String, String> = overrides
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    HotkeyConfig::from_toml_map(&map)
}

/// Doubles as "no shipped default is shared": if this fails, two actions
/// collide out of the box.
#[test]
fn shipped_defaults_shadow_nothing() {
    let config = HotkeyConfig::default();
    for action in HotkeyAction::ALL.iter().chain(HotkeyAction::RESERVED) {
        assert_eq!(
            config.shadowed_by(action),
            None,
            "{action:?} is shadowed on a default config — a shipped default collides",
        );
    }
}

/// Case A: a 0.18.x verbose dump with the old sort binding, plus the user's own
/// action already on the new default. `normalize` cannot move Previous Sort
/// Mode to Shift+Left (taken), so it stands pat on Left and shadows Seek
/// Backward. Nothing can be written: the other row has nowhere to go.
#[test]
fn case_a_retired_default_that_could_not_move_blocks_capture() {
    let config = config_with(&[
        ("prev_sort_mode", "Left"),
        ("move_track_up", "Shift + Left"),
    ]);

    assert_eq!(
        config.get_binding(&HotkeyAction::PrevSortMode),
        KeyCombo::key(KeyCode::ArrowLeft),
        "precondition: normalize left the retired binding in place",
    );
    assert_eq!(
        config.shadowed_by(&HotkeyAction::SeekBackward),
        Some(HotkeyAction::PrevSortMode),
        "Seek Backward shows Left in Settings but never fires",
    );
    assert_eq!(
        config.plan_capture(
            &HotkeyAction::SeekBackward,
            &KeyCombo::key(KeyCode::ArrowLeft)
        ),
        CapturePlan::Blocked {
            by: HotkeyAction::PrevSortMode
        },
        "a swap here would write Previous Sort Mode back onto Left — a no-op with a lying badge",
    );
}

/// Case B: the user's own action sits on a bare arrow, so `resolve` hands it
/// the key (a binding that differs from its default beats one on its default).
/// Its own default is free, so capture can send it home.
#[test]
fn case_b_user_action_on_a_bare_arrow_evicts_to_its_default() {
    let config = config_with(&[("move_track_up", "Left")]);

    assert_eq!(
        config.shadowed_by(&HotkeyAction::SeekBackward),
        Some(HotkeyAction::MoveTrackUp),
    );
    assert_eq!(
        config.plan_capture(
            &HotkeyAction::SeekBackward,
            &KeyCombo::key(KeyCode::ArrowLeft)
        ),
        CapturePlan::Evict {
            from: HotkeyAction::MoveTrackUp,
            to: KeyCombo::shift(KeyCode::ArrowUp),
        },
        "Move Track Up goes back to its own default and Seek Backward keeps Left",
    );
}

/// Three actions on one combo: no single move fixes it, so nothing is written.
#[test]
fn three_actions_on_one_combo_block_capture() {
    let config = config_with(&[("move_track_up", "Left"), ("move_track_down", "Left")]);

    let plan = config.plan_capture(
        &HotkeyAction::SeekBackward,
        &KeyCombo::key(KeyCode::ArrowLeft),
    );
    assert!(
        matches!(plan, CapturePlan::Blocked { .. }),
        "expected Blocked with more than one other claimant, got {plan:?}",
    );
}

/// The other action's own default IS the contested combo (it never moved; the
/// capturing action came to it). Sending it "home" would change nothing.
#[test]
fn other_action_already_on_its_default_blocks_capture() {
    // Seek Backward's default IS Left. Park Previous Sort Mode on Left too,
    // then capture Left onto Previous Sort Mode: evicting Seek Backward would
    // send it to Left, the very combo in dispute.
    let config = config_with(&[
        ("prev_sort_mode", "Left"),
        ("move_track_up", "Shift + Left"),
    ]);

    assert_eq!(
        config.plan_capture(
            &HotkeyAction::PrevSortMode,
            &KeyCombo::key(KeyCode::ArrowLeft)
        ),
        CapturePlan::Blocked {
            by: HotkeyAction::SeekBackward
        },
    );
}

/// A reserved action is never moved.
#[test]
fn a_reserved_claimant_blocks_capture() {
    let config = config_with(&[("toggle_play", "Escape")]);

    assert_eq!(
        config.plan_capture(&HotkeyAction::TogglePlay, &KeyCombo::key(KeyCode::Escape)),
        CapturePlan::Blocked {
            by: HotkeyAction::Escape
        },
        "Escape is reserved — capture must never rebind it",
    );
}

/// A free combo still writes straight through.
#[test]
fn plan_capture_on_a_free_combo_writes() {
    let config = HotkeyConfig::default();
    assert_eq!(
        config.plan_capture(&HotkeyAction::TogglePlay, &KeyCombo::ctrl(KeyCode::F12)),
        CapturePlan::Write,
    );
}

/// Two differently-bound actions still swap, exactly as before.
#[test]
fn plan_capture_swaps_two_differently_bound_actions() {
    let config = HotkeyConfig::default();
    let target = config.get_binding(&HotkeyAction::SeekBackward);
    let own = config.get_binding(&HotkeyAction::TogglePlay);

    assert_eq!(
        config.plan_capture(&HotkeyAction::TogglePlay, &target),
        CapturePlan::Swap {
            with: HotkeyAction::SeekBackward,
            old_combo: own,
        },
    );
}

/// A reserved action can win a combo through `resolve`'s RESERVED-first pass,
/// so the swap branch has to refuse it too — not just the already-shared one.
/// Reachable once `escape` / `reset_to_default` has been hand-edited off its
/// key in `config.toml` (a verbose dump writes both rows).
#[test]
fn a_swap_never_moves_a_reserved_action() {
    let config = config_with(&[("escape", "F5")]);

    assert_eq!(
        config.plan_capture(&HotkeyAction::TogglePlay, &KeyCombo::key(KeyCode::F12)),
        CapturePlan::Write,
        "precondition: an unrelated free combo still writes",
    );
    assert_eq!(
        config.plan_capture(&HotkeyAction::TogglePlay, &KeyCombo::key(KeyCode::F5)),
        CapturePlan::Blocked {
            by: HotkeyAction::Escape
        },
        "swapping would move Escape onto Space, and Escape has no Settings row to undo it from",
    );
}

/// A swap moves exactly one action off the combo, so it only works when there
/// is exactly one other claimant. With two, the captured action lands on a key
/// a third action still wins — the very failure this decision exists to stop.
#[test]
fn a_swap_is_blocked_when_more_than_one_other_action_claims_the_combo() {
    let config = config_with(&[("move_track_up", "Left"), ("move_track_down", "Left")]);

    let plan = config.plan_capture(
        &HotkeyAction::RefreshView,
        &KeyCombo::key(KeyCode::ArrowLeft),
    );
    assert!(
        matches!(plan, CapturePlan::Blocked { .. }),
        "a swap would leave Refresh View on a key Move Track Down still wins, got {plan:?}",
    );
}
