//! Config writer — updates individual values in config.toml using toml_edit
//!
//! Uses `toml_edit` to preserve comments, formatting, and ordering when
//! modifying a single key. Writes atomically via temp file + rename.

use std::path::Path;

use anyhow::{Context, Result};
use toml_edit::{DocumentMut, Item, Value};
use tracing::debug;

use crate::views::settings::items::SettingValue;

/// Update a single value in config.toml, preserving all other content.
///
/// # Arguments
/// * `toml_key` — Dotted key path, e.g. "visualizer.bars.border_width"
/// * `value` — The new value to write
/// * `comment` — Optional description comment added above newly created keys
///
/// If the key doesn't exist yet, it will be created (including parent tables).
pub(crate) fn update_config_value(
    toml_key: &str,
    value: &SettingValue,
    comment: Option<&str>,
) -> Result<()> {
    let config_path =
        nokkvi_data::utils::paths::get_config_path().context("Failed to get config path")?;

    // Read existing config (or start fresh if it doesn't exist)
    let content = if config_path.exists() {
        std::fs::read_to_string(&config_path).context("Failed to read config.toml")?
    } else {
        String::new()
    };

    // Parse into editable document
    let mut doc: DocumentMut = content
        .parse::<DocumentMut>()
        .context("Failed to parse config.toml as TOML")?;

    // Navigate to the correct table and set the value
    set_dotted_value(&mut doc, toml_key, value, comment)?;

    debug!(" [CONFIG WRITER] Updated {toml_key} in config.toml");

    // Write atomically: temp file → rename
    write_atomic(&config_path, &doc.to_string())
}

/// Update a color in a color array at a specific index.
///
/// e.g. `update_color_array_entry("visualizer.bars.dark.bar_gradient_colors", 2, "#ff0000")`
pub(crate) fn update_color_array_entry(
    toml_key: &str,
    index: usize,
    hex_color: &str,
) -> Result<()> {
    let config_path =
        nokkvi_data::utils::paths::get_config_path().context("Failed to get config path")?;

    let content = if config_path.exists() {
        std::fs::read_to_string(&config_path).context("Failed to read config.toml")?
    } else {
        String::new()
    };

    let mut doc: DocumentMut = content
        .parse::<DocumentMut>()
        .context("Failed to parse config.toml as TOML")?;

    // Navigate to the array
    let parts: Vec<&str> = toml_key.split('.').collect();
    let item = navigate_to_item_mut(&mut doc, &parts)?;

    if let Some(arr) = item.as_array_mut() {
        if index < arr.len() {
            arr.replace(index, hex_color);
            debug!(" [CONFIG WRITER] Updated {toml_key}[{index}] = {hex_color}");
        } else {
            anyhow::bail!(
                "Index {index} out of bounds for array {toml_key} (len={})",
                arr.len()
            );
        }
    } else {
        anyhow::bail!("{toml_key} is not an array");
    }

    write_atomic(&config_path, &doc.to_string())
}

/// Navigate a dotted key path and set the final value.
/// When `comment` is `Some` and the key doesn't already exist, adds a
/// `# description` comment above the new key.
fn set_dotted_value(
    doc: &mut DocumentMut,
    key: &str,
    value: &SettingValue,
    comment: Option<&str>,
) -> Result<()> {
    let parts: Vec<&str> = key.split('.').collect();
    if parts.is_empty() {
        anyhow::bail!("Empty key path");
    }

    let (table_parts, field_name) = parts.split_at(parts.len() - 1);
    let field_name = field_name[0];

    // Navigate/create intermediate tables
    let mut current: &mut toml_edit::Table = doc.as_table_mut();
    for &part in table_parts {
        // If the key doesn't exist at all, create an implicit table
        if !current.contains_key(part) {
            current.insert(part, Item::Table(toml_edit::Table::new()));
        }
        current = current[part]
            .as_table_mut()
            .with_context(|| format!("{part} is not a table in config.toml"))?;
    }

    // Set the value
    let toml_value = setting_value_to_toml(value);
    current.insert(field_name, toml_value);

    // Add a description comment above the key when one is provided.
    // Uses the key's leaf_decor prefix — this is the whitespace/comment area
    // above the key line, NOT the value's decor (which sits after `=`).
    // Multi-line descriptions are split on '\n' with each line prefixed by '# '.
    if let Some(desc) = comment
        && let Some(mut key) = current.key_mut(field_name)
    {
        let prefix: String = desc.lines().map(|line| format!("# {line}\n")).collect();
        key.leaf_decor_mut().set_prefix(prefix);
    }

    Ok(())
}

/// Navigate to an item at a dotted path, returning a mutable reference.
fn navigate_to_item_mut<'a>(doc: &'a mut DocumentMut, parts: &[&str]) -> Result<&'a mut Item> {
    if parts.is_empty() {
        anyhow::bail!("Empty key path");
    }

    let (table_parts, field_parts) = parts.split_at(parts.len() - 1);
    let field_name = field_parts[0];

    let mut current: &mut toml_edit::Table = doc.as_table_mut();
    for &part in table_parts {
        current = current[part]
            .as_table_mut()
            .with_context(|| format!("{part} is not a table in config.toml"))?;
    }

    current
        .get_mut(field_name)
        .with_context(|| format!("{field_name} not found in config.toml"))
}

/// Convert a SettingValue to a toml_edit::Item for insertion.
fn setting_value_to_toml(value: &SettingValue) -> Item {
    match value {
        SettingValue::Float { val, .. } => {
            // Round to 6 decimal places to eliminate f32→f64 precision noise
            // (e.g. 0.046_f32 as f64 = 0.04600000075995922 → 0.046)
            let rounded = format!("{val:.6}").parse::<f64>().unwrap_or(*val);
            Item::Value(Value::Float(toml_edit::Formatted::new(rounded)))
        }
        SettingValue::Int { val, .. } => {
            Item::Value(Value::Integer(toml_edit::Formatted::new(*val)))
        }
        SettingValue::Bool(v) => Item::Value(Value::Boolean(toml_edit::Formatted::new(*v))),
        SettingValue::Enum { val, .. } => {
            Item::Value(Value::String(toml_edit::Formatted::new(val.clone())))
        }
        SettingValue::HexColor(hex) => {
            Item::Value(Value::String(toml_edit::Formatted::new(hex.clone())))
        }
        SettingValue::ColorArray(colors) => {
            let mut arr = toml_edit::Array::new();
            for color in colors {
                arr.push(color.as_str());
            }
            Item::Value(Value::Array(arr))
        }
        SettingValue::Text(t) => Item::Value(Value::String(toml_edit::Formatted::new(t.clone()))),
        // Hotkeys are stored in redb, not TOML — this branch should never be reached
        SettingValue::Hotkey(_) => Item::None,
    }
}

/// Write content to a file atomically using temp file + rename.
fn write_atomic(path: &Path, content: &str) -> Result<()> {
    let temp_path = path.with_extension("toml.tmp");

    std::fs::write(&temp_path, content)
        .with_context(|| format!("Failed to write temp file: {}", temp_path.display()))?;

    std::fs::rename(&temp_path, path)
        .with_context(|| format!("Failed to rename temp file to: {}", path.display()))?;

    debug!(" [CONFIG WRITER] Atomic write to {}", path.display());
    Ok(())
}

/// Reset all `[visualizer]` settings to defaults while preserving user color
/// palettes (`bars.dark`, `bars.light`, `lines.dark`, `lines.light`).
///
/// Serializes the default `VisualizerConfig`, strips color sub-tables from the
/// defaults, then merges the remaining keys into the user's existing config.
pub(crate) fn reset_visualizer_defaults_preserving_colors() -> Result<()> {
    let default_config = crate::visualizer_config::VisualizerConfig::default();
    let toml_str = toml::to_string_pretty(&default_config)
        .context("Failed to serialize default VisualizerConfig")?;

    let config_path =
        nokkvi_data::utils::paths::get_config_path().context("Failed to get config path")?;

    let content = if config_path.exists() {
        std::fs::read_to_string(&config_path).context("Failed to read config.toml")?
    } else {
        String::new()
    };

    let mut doc: DocumentMut = content
        .parse::<DocumentMut>()
        .context("Failed to parse config.toml as TOML")?;

    // Build a default [visualizer] table, then strip color sub-tables
    // so the user's existing dark/light color palettes are preserved.
    let mut default_doc: DocumentMut = format!("[visualizer]\n{toml_str}")
        .parse()
        .context("Failed to parse default visualizer TOML")?;

    if let Some(viz) = default_doc.get_mut("visualizer") {
        for section in ["bars", "lines"] {
            if let Some(s) = viz.get_mut(section)
                && let Some(tbl) = s.as_table_like_mut()
            {
                tbl.remove("dark");
                tbl.remove("light");
            }
        }
    }

    // Merge default values into existing doc, preserving colors.
    if let Some(default_viz) = default_doc.get("visualizer")
        && let Some(default_tbl) = default_viz.as_table()
    {
        // Ensure [visualizer] exists in the user's config
        if doc.get("visualizer").is_none() {
            doc.insert("visualizer", Item::Table(toml_edit::Table::new()));
        }
        if let Some(user_viz) = doc.get_mut("visualizer")
            && let Some(user_tbl) = user_viz.as_table_like_mut()
        {
            for (key, value) in default_tbl.iter() {
                user_tbl.insert(key, value.clone());
            }
        }
    }

    write_atomic(&config_path, &doc.to_string())
}
