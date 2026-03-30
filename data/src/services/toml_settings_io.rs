//! TOML settings I/O — read/write [settings], [hotkeys], and [views] sections
//!
//! All functions operate on the shared `config.toml` file using `toml_edit`
//! for surgical section replacement that preserves comments and formatting
//! in other sections.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use tracing::debug;

use crate::{
    types::{
        hotkey_config::HotkeyConfig,
        toml_settings::TomlSettings,
        toml_views::TomlViewPreferences,
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

    let content =
        std::fs::read_to_string(&config_path).context("Failed to read config.toml")?;
    let doc: toml::Value = content.parse().context("Failed to parse config.toml")?;

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

    let content =
        std::fs::read_to_string(&config_path).context("Failed to read config.toml")?;
    let doc: toml::Value = content.parse().context("Failed to parse config.toml")?;

    match doc.get("hotkeys") {
        Some(section) => {
            // The [hotkeys] section is a flat map of action_key = "combo_string"
            let map: BTreeMap<String, String> = section
                .clone()
                .try_into()
                .context("Failed to deserialize [hotkeys] as string map")?;
            Ok(Some(HotkeyConfig::from_toml_map(&map)))
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

    let content =
        std::fs::read_to_string(&config_path).context("Failed to read config.toml")?;
    let doc: toml::Value = content.parse().context("Failed to parse config.toml")?;

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
/// Only non-default bindings are written.
pub fn write_toml_hotkeys(hotkeys: &HotkeyConfig) -> Result<()> {
    let map = hotkeys.to_toml_map();
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

/// Write all three sections at once (used during migration).
/// Single read-modify-write cycle to avoid multiple file I/Os.
pub fn write_all_toml_sections(
    settings: &TomlSettings,
    hotkeys: &HotkeyConfig,
    views: &TomlViewPreferences,
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
        &toml::Value::try_from(hotkeys.to_toml_map())?,
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
