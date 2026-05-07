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
