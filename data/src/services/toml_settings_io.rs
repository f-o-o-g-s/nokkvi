//! TOML settings I/O — read/write [settings], [hotkeys], and [views] sections
//!
//! All functions operate on the shared `config.toml` file using `toml_edit`
//! for surgical section replacement that preserves comments and formatting
//! in other sections.

use anyhow::{Context, Result};
use tracing::debug;

use crate::{
    types::{
        hotkey_config::HotkeyConfig, toml_settings::TomlSettings, toml_views::TomlViewPreferences,
    },
    utils::paths::get_config_path,
};

// =============================================================================
// Readers
// =============================================================================

/// Read the `[settings]` section from config.toml.
/// Returns `None` if the file or section doesn't exist.
pub fn read_toml_settings() -> Result<Option<TomlSettings>> {
    let config_path = get_config_path().context("Failed to get config path")?;
    if !config_path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&config_path).context("Failed to read config.toml")?;
    let doc: toml::Value = toml::from_str(&content).context("Failed to parse config.toml")?;

    match doc.get("settings") {
        Some(section) => {
            let settings: TomlSettings = section
                .clone()
                .try_into()
                .context("Failed to deserialize [settings] section")?;
            Ok(Some(settings))
        }
        None => Ok(None),
    }
}

/// Read the `[hotkeys]` section from config.toml.
/// Returns `None` if the file or section doesn't exist.
pub fn read_toml_hotkeys() -> Result<Option<HotkeyConfig>> {
    let config_path = get_config_path().context("Failed to get config path")?;
    if !config_path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&config_path).context("Failed to read config.toml")?;
    let doc: toml::Value = toml::from_str(&content).context("Failed to parse config.toml")?;

    match doc.get("hotkeys") {
        Some(section) => {
            let map: std::collections::BTreeMap<String, String> = section
                .clone()
                .try_into()
                .context("Failed to deserialize [hotkeys] section")?;
            Ok(Some(
                crate::types::hotkey_config::HotkeyConfig::from_toml_map(&map),
            ))
        }
        None => Ok(None),
    }
}

/// Read the `[views]` section from config.toml.
/// Returns `None` if the file or section doesn't exist.
pub fn read_toml_views() -> Result<Option<TomlViewPreferences>> {
    let config_path = get_config_path().context("Failed to get config path")?;
    if !config_path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&config_path).context("Failed to read config.toml")?;
    let doc: toml::Value = toml::from_str(&content).context("Failed to parse config.toml")?;

    match doc.get("views") {
        Some(section) => {
            let views: TomlViewPreferences = section
                .clone()
                .try_into()
                .context("Failed to deserialize [views] section")?;
            Ok(Some(views))
        }
        None => Ok(None),
    }
}

// =============================================================================
// Writers
// =============================================================================

/// Write the `[settings]` section to config.toml, preserving all other content.
///
/// When `verbose` is false, only non-default keys are written (plus the
/// `verbose_config` anchor); when true, every field is written.
pub fn write_toml_settings(settings: &TomlSettings, verbose: bool) -> Result<()> {
    write_section("settings", &settings_value(settings, verbose)?)
}

/// Write the `[hotkeys]` section to config.toml, preserving all other content.
/// If `verbose` is true, all bindings are written; otherwise, only non-default bindings.
pub fn write_toml_hotkeys(hotkeys: &HotkeyConfig, verbose: bool) -> Result<()> {
    let map = hotkeys.to_toml_map(verbose);
    write_section("hotkeys", &toml::Value::try_from(map)?)
}

/// Write the `[views]` section to config.toml, preserving all other content.
///
/// When `verbose` is false, only non-default sort keys are written; when true,
/// every field is written.
pub fn write_toml_views(views: &TomlViewPreferences, verbose: bool) -> Result<()> {
    write_section("views", &views_value(views, verbose)?)
}

/// Serialize `[settings]` to a TOML value, pruning default-valued keys when
/// `verbose` is false. `verbose_config` is always retained as a section anchor:
/// a fully-default `[settings]` would otherwise strip to nothing, drop the
/// section, and re-trigger the full-dump migration (the `has_toml` gate in
/// `SettingsManager::new`) on next launch.
fn settings_value(settings: &TomlSettings, verbose: bool) -> Result<toml::Value> {
    let full = toml::Value::try_from(settings)?;
    if verbose {
        return Ok(full);
    }
    let defaults = toml::Value::try_from(TomlSettings::default())?;
    Ok(prune_default_keys(full, &defaults, &["verbose_config"]))
}

/// Serialize `[views]` to a TOML value, pruning default-valued keys when
/// `verbose` is false. No anchor is needed — the migration gate keys on
/// `[settings]`, so an all-default (empty) `[views]` reads back as the default
/// without re-triggering anything.
fn views_value(views: &TomlViewPreferences, verbose: bool) -> Result<toml::Value> {
    let full = toml::Value::try_from(views)?;
    if verbose {
        return Ok(full);
    }
    let defaults = toml::Value::try_from(TomlViewPreferences::default())?;
    Ok(prune_default_keys(full, &defaults, &[]))
}

/// Remove every top-level key whose value equals the same key in `defaults`,
/// except keys listed in `keep`. `full` and `defaults` must be produced by the
/// same serializer so float rounding etc. compares equal.
///
/// Round-trip safety (an omitted key reads back as its struct default) is
/// pinned by the `empty_table_deserializes_to_struct_default` guards on
/// `TomlSettings` / `TomlViewPreferences`.
fn prune_default_keys(full: toml::Value, defaults: &toml::Value, keep: &[&str]) -> toml::Value {
    let toml::Value::Table(mut table) = full else {
        return full;
    };
    if let Some(defaults) = defaults.as_table() {
        let mut to_remove: Vec<String> = Vec::new();
        for (key, value) in &table {
            if keep.contains(&key.as_str()) {
                continue;
            }
            if defaults.get(key.as_str()) == Some(value) {
                to_remove.push(key.clone());
            }
        }
        for key in to_remove {
            table.remove(key.as_str());
        }
    }
    toml::Value::Table(table)
}

/// Replace a single top-level section in config.toml using `toml_edit`.
///
/// Routes through the shared `write_atomic` helper for the temp + rename and
/// watcher-suppress contract. Preserves comments, formatting, and ordering
/// in all other sections.
fn write_section(section_name: &str, value: &toml::Value) -> Result<()> {
    use toml_edit::{DocumentMut, Item};

    let config_path = get_config_path().context("Failed to get config path")?;

    let content = if config_path.exists() {
        std::fs::read_to_string(&config_path).context("Failed to read config.toml")?
    } else {
        String::new()
    };

    let mut doc: DocumentMut = content
        .parse::<DocumentMut>()
        .context("Failed to parse config.toml as TOML")?;

    // Serialize the value to a TOML string, then re-parse as a toml_edit document
    // so we get properly formatted table entries.
    let section_toml = toml::to_string_pretty(value)
        .with_context(|| format!("Failed to serialize [{section_name}]"))?;

    // Parse the serialized section as a toml_edit table
    let section_doc: DocumentMut = section_toml
        .parse::<DocumentMut>()
        .with_context(|| format!("Failed to re-parse [{section_name}] as toml_edit"))?;

    // Replace the section in the main document
    let section_table = section_doc.as_table().clone();
    doc.insert(section_name, Item::Table(section_table));

    debug!(" [TOML I/O] Updated [{section_name}] in config.toml");

    crate::utils::paths::write_atomic(&config_path, &doc.to_string())
}

pub fn write_all_toml_sections(
    settings: &TomlSettings,
    hotkeys: &HotkeyConfig,
    views: &TomlViewPreferences,
    verbose: bool,
) -> Result<()> {
    use toml_edit::{DocumentMut, Item};

    let config_path = get_config_path().context("Failed to get config path")?;

    let content = if config_path.exists() {
        std::fs::read_to_string(&config_path).context("Failed to read config.toml")?
    } else {
        String::new()
    };

    let mut doc: DocumentMut = content
        .parse::<DocumentMut>()
        .context("Failed to parse config.toml as TOML")?;

    // Helper: serialize → re-parse → insert
    let insert_section = |doc: &mut DocumentMut, name: &str, value: &toml::Value| -> Result<()> {
        let toml_str = toml::to_string_pretty(value)
            .with_context(|| format!("Failed to serialize [{name}]"))?;
        let section_doc: DocumentMut = toml_str
            .parse::<DocumentMut>()
            .with_context(|| format!("Failed to re-parse [{name}]"))?;
        doc.insert(name, Item::Table(section_doc.as_table().clone()));
        Ok(())
    };

    insert_section(&mut doc, "settings", &settings_value(settings, verbose)?)?;
    insert_section(
        &mut doc,
        "hotkeys",
        &toml::Value::try_from(hotkeys.to_toml_map(verbose))?,
    )?;
    insert_section(&mut doc, "views", &views_value(views, verbose)?)?;

    debug!(" [TOML I/O] Wrote [settings], [hotkeys], [views] to config.toml");

    crate::utils::paths::write_atomic(&config_path, &doc.to_string())
}

#[cfg(test)]
mod tests {

    // -- Sparse-config (verbose-off) pruning --------------------------------
    //
    // These exercise the pure `settings_value` / `views_value` helpers (no I/O)
    // that the `verbose` flag routes through. The companion round-trip exactness
    // guard lives in `toml_settings::tests::empty_table_deserializes_to_struct_default`.

    #[test]
    fn sparse_default_settings_keeps_only_verbose_config_anchor() {
        use crate::types::toml_settings::TomlSettings;
        let v = super::settings_value(&TomlSettings::default(), false).expect("settings_value");
        let tbl = v.as_table().expect("table");
        assert_eq!(
            tbl.len(),
            1,
            "all-default [settings] must prune to just the verbose_config anchor, got: {tbl:?}"
        );
        assert!(
            tbl.contains_key("verbose_config"),
            "anchor must be retained"
        );
    }

    #[test]
    fn verbose_settings_keeps_every_key() {
        use crate::types::toml_settings::TomlSettings;
        let full = super::settings_value(&TomlSettings::default(), true).expect("verbose");
        assert!(
            full.as_table().expect("table").len() > 50,
            "verbose must keep the full settings table (no pruning)"
        );
    }

    #[test]
    fn sparse_settings_roundtrip_preserves_nondefault_and_restores_default() {
        use crate::types::{player_settings::StripSeparator, toml_settings::TomlSettings};
        let s = TomlSettings {
            crossfade_duration_secs: 10,          // default 7
            light_mode: true,                     // default false
            queue_show_album: false,              // default true
            strip_separator: StripSeparator::Dot, // default Slash
            ..TomlSettings::default()
        };
        let sparse = super::settings_value(&s, false).expect("sparse");
        let tbl = sparse.as_table().expect("table");
        assert!(
            tbl.contains_key("crossfade_duration_secs"),
            "non-default kept"
        );
        assert!(tbl.contains_key("strip_separator"), "non-default kept");
        assert!(tbl.contains_key("verbose_config"), "anchor kept");
        assert!(!tbl.contains_key("auto_follow_playing"), "default pruned");
        assert!(!tbl.contains_key("scrobble_threshold"), "default pruned");

        // The sparse table must deserialize back to exactly `s` (non-defaults
        // preserved, omitted keys restored to their struct defaults).
        let back: TomlSettings = sparse.try_into().expect("deserialize sparse");
        assert_eq!(
            toml::to_string_pretty(&back).unwrap(),
            toml::to_string_pretty(&s).unwrap(),
            "sparse round-trip must preserve non-defaults and restore defaults exactly"
        );
    }

    #[test]
    fn sparse_views_default_strips_empty_and_nondefault_roundtrips() {
        use crate::types::toml_views::TomlViewPreferences;
        // All-default views prune to an empty table (no anchor — the migration
        // gate keys on [settings], so an empty [views] reads back as default).
        let empty = super::views_value(&TomlViewPreferences::default(), false).expect("views");
        assert!(
            empty.as_table().expect("table").is_empty(),
            "all-default [views] must prune to nothing"
        );

        // A non-default sort survives and round-trips.
        let v = TomlViewPreferences {
            queue_sort: "rating".to_string(), // default "album"
            queue_ascending: false,           // default true
            ..TomlViewPreferences::default()
        };
        let sparse = super::views_value(&v, false).expect("views");
        let tbl = sparse.as_table().expect("table");
        assert!(tbl.contains_key("queue_sort"));
        assert!(tbl.contains_key("queue_ascending"));
        assert!(!tbl.contains_key("albums_sort"), "default sort pruned");
        let back: TomlViewPreferences = sparse.try_into().expect("deserialize");
        assert_eq!(
            toml::to_string_pretty(&back).unwrap(),
            toml::to_string_pretty(&v).unwrap(),
        );
    }

    #[test]
    fn test_hotkeys_parsing() {
        let toml_str = r#"[hotkeys]
switch_to_settings = "F1"
"#;
        let doc: toml::Value = toml::from_str(toml_str).unwrap();
        let section = doc.get("hotkeys").unwrap();
        let map: std::collections::BTreeMap<String, String> = section.clone().try_into().unwrap();
        assert_eq!(map.get("switch_to_settings").unwrap(), "F1");

        let config = crate::types::hotkey_config::HotkeyConfig::from_toml_map(&map);
        let action = crate::types::hotkey_config::HotkeyAction::SwitchToSettings;
        let binding = config.get_binding(&action);

        assert_eq!(format!("{binding}"), "F1");
    }

    #[test]
    fn test_settings_parsing() {
        let toml_str = r#"[settings]
auto_follow_playing = true
crossfade_duration_secs = 12
crossfade_enabled = true
custom_eq_presets = []
enter_behavior = "play_all"
eq_enabled = true
eq_gains = [
    4.0,
    3.5,
    1.5,
    0.0,
    -1.5,
    -0.5,
    0.5,
    2.0,
    3.5,
    4.0,
]
horizontal_volume = false
light_mode = false
local_music_path = "/music/Library"
mini_player_show_modes = true
mini_player_show_volume = true
nav_display_mode = "text_only"
nav_layout = "top"
normalization_level = "normal"
opacity_gradient = false
quick_add_to_playlist = false
rounded_mode = true
scrobble_threshold = 0.8999999761581421
scrobbling_enabled = true
sfx_volume = 0.3758544921875
slot_row_height = "compact"
sound_effects_enabled = false
stable_viewport = true
start_view = "Queue"
strip_click_action = "go_to_queue"
strip_show_album = true
strip_show_artist = true
strip_show_format_info = true
strip_show_title = true
track_info_display = "top_bar"
verbose_config = true
visualization_mode = "lines"
volume_normalization = true
"#;
        let doc: toml::Value = toml::from_str(toml_str).unwrap();
        let section = doc.get("settings").unwrap();
        let settings: crate::types::toml_settings::TomlSettings =
            section.clone().try_into().unwrap();
        assert!(settings.verbose_config);
    }

    #[test]
    fn test_hotkeys_roundtrip_verbose() {
        use std::str::FromStr;

        use crate::types::hotkey_config::{HotkeyAction, HotkeyConfig, KeyCombo};

        // Custom binding: SwitchToSettings → F1
        let mut config = HotkeyConfig::default();
        config.set_binding(
            HotkeyAction::SwitchToSettings,
            KeyCombo::from_str("F1").unwrap(),
        );

        // Verbose mode: all bindings serialized
        let map = config.to_toml_map(true);
        assert!(map.len() > 1, "verbose mode should serialize all bindings");
        assert_eq!(map.get("switch_to_settings").unwrap(), "F1");

        // Round-trip: deserialize back and verify
        let roundtrip = HotkeyConfig::from_toml_map(&map);
        assert_eq!(
            format!("{}", roundtrip.get_binding(&HotkeyAction::SwitchToSettings)),
            "F1"
        );

        // Sparse mode: only non-default bindings
        let sparse = config.to_toml_map(false);
        assert_eq!(
            sparse.get("switch_to_settings").unwrap(),
            "F1",
            "non-default binding should always appear"
        );
        assert!(
            sparse.len() < map.len(),
            "sparse mode should have fewer entries than verbose"
        );
    }

    #[test]
    fn test_settings_roundtrip_verbose_config_flag() {
        use crate::types::toml_settings::TomlSettings;

        let settings = TomlSettings {
            verbose_config: true,
            ..Default::default()
        };

        // Serialize → parse → verify flag survives
        let toml_str = toml::to_string_pretty(&settings).unwrap();
        let roundtrip: TomlSettings = toml::from_str(&toml_str).unwrap();
        assert!(roundtrip.verbose_config);
    }
}
