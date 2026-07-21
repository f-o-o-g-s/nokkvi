//! Coverage gates for settings search.
//!
//! The `new-feature-checklist` skill has required curated search synonyms for
//! every new settings row since 2026-06-10, but nothing enforced it, so 97 rows
//! shipped without any and four table entries rotted into pointing at rows that
//! no longer exist. These tests are that enforcement.
//!
//! Rows are built from SEVERAL data variants, not just `Default`: a good third
//! of the Playback and Interface rows only render when a parent toggle is on or
//! a parent enum sits in a particular mode. Auditing `Default` alone both
//! undercounts coverage and — worse — reports live entries as dead.

use nokkvi_data::{
    types::{
        hotkey_config::HotkeyConfig,
        settings_data::{GeneralSettingsData, InterfaceSettingsData, PlaybackSettingsData},
        theme_file::ThemeFile,
    },
    utils::setting_keywords,
};

use super::{
    SettingsPage, SettingsTab, SettingsViewData,
    items::{SettingItem, SettingsEntry},
};

/// One `SettingsViewData` with the given tab payloads and stock everything else.
fn view_data(interface: InterfaceSettingsData, playback: PlaybackSettingsData) -> SettingsViewData {
    SettingsViewData {
        general: GeneralSettingsData::default(),
        interface,
        playback,
        visualizer_config: crate::visualizer_config::VisualizerConfig::default(),
        theme_file: ThemeFile::default(),
        active_theme_stem: "svalbard".to_string(),
        hotkey_config: HotkeyConfig::default(),
        is_light_mode: false,
        rounded_mode: nokkvi_data::types::player_settings::RoundedMode::default(),
        opacity_gradient: false,
    }
}

/// Data variants whose union renders every conditional row. AGC and ReplayGain
/// are mutually exclusive, so no single variant can show them all.
fn all_variants() -> Vec<SettingsViewData> {
    let expanded_interface = || InterfaceSettingsData {
        autohide_toolbar: true,
        autohide_collapsed_appearance: "Hairline".into(),
        track_info_display: "Mini Player".into(),
        artwork_column_mode: "Always (Stretched)".into(),
        ..Default::default()
    };
    let fades_on = || PlaybackSettingsData {
        crossfade_enabled: true,
        fade_on_pause: true,
        fade_on_stop: true,
        fade_on_skip: "Boundary Fade".into(),
        rating_reminder_enabled: true,
        rating_reminder_trigger: "Percentage Played".into(),
        ..Default::default()
    };
    vec![
        view_data(
            InterfaceSettingsData::default(),
            PlaybackSettingsData::default(),
        ),
        view_data(
            expanded_interface(),
            PlaybackSettingsData {
                volume_normalization: "AGC".into(),
                ..fades_on()
            },
        ),
        view_data(
            expanded_interface(),
            PlaybackSettingsData {
                volume_normalization: "ReplayGain (Track)".into(),
                ..fades_on()
            },
        ),
    ]
}

/// Every row the settings UI can render, across all variants, deduplicated by
/// key.
fn all_rows() -> Vec<SettingItem> {
    let mut seen = std::collections::BTreeMap::new();
    for data in all_variants() {
        for tab in SettingsTab::ALL {
            for entry in SettingsPage::build_tab_entries(*tab, &data) {
                if let SettingsEntry::Item(item) = entry {
                    seen.entry(item.key.to_string()).or_insert(item);
                }
            }
        }
    }
    seen.into_values().collect()
}

/// Mirror of `setting_keywords::normalize`, which is private.
fn normalize(key: &str) -> &str {
    key.strip_prefix("dark.")
        .or_else(|| key.strip_prefix("light."))
        .unwrap_or(key)
}

#[test]
fn every_settings_row_has_search_keywords() {
    let missing: Vec<String> = all_rows()
        .iter()
        .filter(|item| {
            setting_keywords::all_keywords_for(&item.key)
                .next()
                .is_none()
        })
        .map(|item| format!("{} ({})", item.key, item.label))
        .collect();
    assert!(
        missing.is_empty(),
        "{} settings row(s) have no search synonyms. Add them to \
         `keywords_for` in data/src/utils/setting_keywords.rs (see the \
         new-feature-checklist skill):\n  {}",
        missing.len(),
        missing.join("\n  ")
    );
}

#[test]
fn no_dead_keyword_table_keys() {
    let rows = all_rows();
    let live: std::collections::BTreeSet<&str> = rows.iter().map(|i| normalize(&i.key)).collect();
    // Collected once so the failure names every rotted entry, not just the
    // first — they tend to arrive in batches when a section is restructured.
    let dead: Vec<&str> = setting_keywords::TABLE_KEYS
        .iter()
        .copied()
        .filter(|k| !live.contains(k))
        .collect();
    assert!(
        dead.is_empty(),
        "{} keyword table entr(y/ies) name a key no settings row carries, so \
         their synonyms can never match. Re-point them at the live row's key \
         (a ToggleSet's fields are addressed by the PARENT row's sentinel key) \
         or delete them:\n  {}",
        dead.len(),
        dead.join("\n  ")
    );
}

#[test]
fn categories_are_short_labels_not_sentences() {
    // `SettingMeta::new`'s third positional arg is `category`, not `subtitle`.
    // Two rows passed a full sentence there, which both mis-rendered and fed a
    // 97-char string into the CATEGORY search tier.
    let long: Vec<String> = all_rows()
        .iter()
        .filter(|i| i.category.chars().count() > 30)
        .map(|i| format!("{} -> {:?}", i.key, i.category))
        .collect();
    assert!(
        long.is_empty(),
        "category should be a short section name; use `.with_subtitle(...)` \
         for prose:\n  {}",
        long.join("\n  ")
    );
}

// ── Production search path ──────────────────────────────────────────────────
// The ranking tests in `entries.rs` all call `filter_by_search`, which is
// test-only and always passes `tab_score: None` — so the tab-context tier and
// the cross-tab section merge had no coverage at all.

fn search(query: &str) -> Vec<String> {
    let data = view_data(
        InterfaceSettingsData::default(),
        PlaybackSettingsData::default(),
    );
    SettingsPage::search_all_entries(&data, query)
        .into_iter()
        .filter_map(|e| match e {
            SettingsEntry::Item(i) => Some(i.label.clone()),
            SettingsEntry::Header { .. } => None,
        })
        .collect()
}

#[test]
fn absent_features_return_no_results() {
    // The worst outcome is a confident list of unrelated rows for a feature
    // that does not exist — it reads as "here it is" and buries the honest
    // empty state.
    for q in ["sleep timer", "background color", "crossfeed", "podcast"] {
        assert!(
            search(q).is_empty(),
            "query {q:?} matches nothing in nokkvi and must return empty, got {:?}",
            search(q)
        );
    }
}

#[test]
fn midword_tab_name_does_not_pull_in_the_tab() {
    // A tab-name match seeds a baseline for EVERY row in that tab, so a mid-word
    // hit is the costliest false positive in the scorer: "lay" inside "Playback"
    // used to return 88 rows.
    // The Theme tab has four rows, two of which carry "heme" in their own label
    // ("Theme Mode", "Browse Themes…"). The other two may only appear if the
    // mid-word hit on the tab name "Theme" seeded them.
    let heme = search("heme");
    for own_label in ["Theme Mode", "Browse Themes…"] {
        assert!(heme.iter().any(|l| l == own_label), "got {heme:?}");
    }
    for seeded_only in ["Rounded Corners", "Opacity Gradient"] {
        assert!(
            !heme.iter().any(|l| l == seeded_only),
            "{seeded_only:?} matches nothing itself and must not ride in on the \
             tab name, got {heme:?}"
        );
    }
    // Typing a tab name properly still pulls its rows in.
    assert!(search("hotkeys").len() > 40);
}

#[test]
fn word_order_does_not_hide_a_row() {
    let hits = search("radio scrobble");
    assert!(
        hits.iter().any(|l| l == "Scrobble Radio"),
        "reversed word order should still find the row, got {hits:?}"
    );
}

#[test]
fn synonyms_reach_rows_whose_text_lacks_the_word() {
    for (query, expected) in [
        ("listenbrainz", "Scrobble Radio"),
        ("phosphor", "Icon Set"),
        ("oscilloscope", "Line Thickness"),
        ("palette", "Browse Themes…"),
        ("shortcut", "Play / Pause"),
    ] {
        let hits = search(query);
        assert!(
            hits.iter().any(|l| l == expected),
            "query {query:?} should surface {expected:?}, got {hits:?}"
        );
    }
}

#[test]
fn subtitle_only_vocabulary_still_matches() {
    // The contiguous gate must not cost us words that live ONLY in prose.
    for (query, expected) in [
        ("waybar", "Show Tray Icon"),
        ("hyprland", "Show Tray Icon"),
        ("pipewire", "Bit-Perfect Output"),
    ] {
        let hits = search(query);
        assert!(
            hits.iter().any(|l| l == expected),
            "subtitle-only term {query:?} should still find {expected:?}, got {hits:?}"
        );
    }
}
