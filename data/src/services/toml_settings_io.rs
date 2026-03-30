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

#[cfg(test)]
mod tests {

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

        assert_eq!(format!("{}", binding), "F1");
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
        assert_eq!(settings.verbose_config, true);
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

        let mut settings = TomlSettings::default();
        settings.verbose_config = true;

        // Serialize → parse → verify flag survives
        let toml_str = toml::to_string_pretty(&settings).unwrap();
        let roundtrip: TomlSettings = toml::from_str(&toml_str).unwrap();
        assert!(roundtrip.verbose_config);
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
pub fn write_toml_settings(settings: &TomlSettings) -> Result<()> {
    write_section("settings", &toml::Value::try_from(settings)?)
}

/// Write the `[hotkeys]` section to config.toml, preserving all other content.
/// If `verbose` is true, all bindings are written; otherwise, only non-default bindings.
pub fn write_toml_hotkeys(hotkeys: &HotkeyConfig, verbose: bool) -> Result<()> {
    let map = hotkeys.to_toml_map(verbose);
    write_section("hotkeys", &toml::Value::try_from(map)?)
}

/// Write the `[views]` section to config.toml, preserving all other content.
pub fn write_toml_views(views: &TomlViewPreferences) -> Result<()> {
    write_section("views", &toml::Value::try_from(views)?)
}

/// Replace a single top-level section in config.toml using `toml_edit`.
///
/// Uses the same atomic write (temp file → rename) pattern as `config_writer.rs`.
/// Preserves comments, formatting, and ordering in all other sections.
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

    // Write atomically: temp file → rename
    let temp_path = config_path.with_extension("toml.tmp");
    std::fs::write(&temp_path, doc.to_string())
        .with_context(|| format!("Failed to write temp file: {}", temp_path.display()))?;
    crate::utils::paths::suppress_config_reload(|| std::fs::rename(&temp_path, &config_path))
        .with_context(|| format!("Failed to rename temp file to: {}", config_path.display()))?;

    Ok(())
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

    insert_section(&mut doc, "settings", &toml::Value::try_from(settings)?)?;
    insert_section(
        &mut doc,
        "hotkeys",
        &toml::Value::try_from(hotkeys.to_toml_map(verbose))?,
    )?;
    insert_section(&mut doc, "views", &toml::Value::try_from(views)?)?;

    debug!(" [TOML I/O] Wrote [settings], [hotkeys], [views] to config.toml");

    let temp_path = config_path.with_extension("toml.tmp");
    std::fs::write(&temp_path, doc.to_string())
        .with_context(|| format!("Failed to write temp file: {}", temp_path.display()))?;
    crate::utils::paths::suppress_config_reload(|| std::fs::rename(&temp_path, &config_path))
        .with_context(|| format!("Failed to rename temp file to: {}", config_path.display()))?;

    Ok(())
}
