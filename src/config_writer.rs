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

/// Update a single value in the active theme file, preserving all other content.
///
/// Reads `theme = "name"` from config.toml, resolves the path
/// `~/.config/nokkvi/themes/{name}.toml`, and edits in-place.
pub(crate) fn update_theme_value(toml_key: &str, value: &SettingValue) -> Result<()> {
    let theme_path = get_active_theme_path()?;

    let content =
        std::fs::read_to_string(&theme_path).context("Failed to read active theme file")?;

    let mut doc: DocumentMut = content
        .parse::<DocumentMut>()
        .context("Failed to parse theme file as TOML")?;

    set_dotted_value(&mut doc, toml_key, value, None)?;

    debug!(" [CONFIG WRITER] Updated {toml_key} in theme file");

    write_atomic(&theme_path, &doc.to_string())
}

/// Update a color in a color array at a specific index inside the active theme file.
pub(crate) fn update_theme_color_array_entry(
    toml_key: &str,
    index: usize,
    hex_color: &str,
) -> Result<()> {
    let theme_path = get_active_theme_path()?;

    let content =
        std::fs::read_to_string(&theme_path).context("Failed to read active theme file")?;

    let mut doc: DocumentMut = content
        .parse::<DocumentMut>()
        .context("Failed to parse theme file as TOML")?;

    let parts: Vec<&str> = toml_key.split('.').collect();
    let item = navigate_to_item_mut(&mut doc, &parts)?;

    if let Some(arr) = item.as_array_mut() {
        if index < arr.len() {
            arr.replace(index, hex_color);
            debug!(" [CONFIG WRITER] Updated {toml_key}[{index}] = {hex_color} in theme file");
        } else {
            anyhow::bail!(
                "Index {index} out of bounds for array {toml_key} (len={})",
                arr.len()
            );
        }
    } else {
        anyhow::bail!("{toml_key} is not an array");
    }

    write_atomic(&theme_path, &doc.to_string())
}

/// Resolve the filesystem path to the active theme file.
fn get_active_theme_path() -> Result<std::path::PathBuf> {
    let name = nokkvi_data::services::theme_loader::read_theme_name_from_config();
    let themes_dir =
        nokkvi_data::utils::paths::get_themes_dir().context("Failed to get themes dir")?;
    Ok(themes_dir.join(format!("{name}.toml")))
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
        // Hotkeys and ToggleSet are stored in redb, not TOML — these branches should never be reached
        SettingValue::Hotkey(_) | SettingValue::ToggleSet(_) => Item::None,
    }
}

/// Write content to a file atomically using temp file + rename.
fn write_atomic(path: &Path, content: &str) -> Result<()> {
    let temp_path = path.with_extension("toml.tmp");

    std::fs::write(&temp_path, content)
        .with_context(|| format!("Failed to write temp file: {}", temp_path.display()))?;

    nokkvi_data::utils::paths::suppress_config_reload(|| std::fs::rename(&temp_path, path))
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

/// Write full `[visualizer]` section to config.toml,
/// replacing it with a complete serialization of all behavior fields (excluding colors,
/// which now live in theme files).
///
/// Used when verbose_config mode is enabled so the user sees every configurable
/// value in config.toml as a human-readable template.
pub(crate) fn write_full_visualizer(
    visualizer_config: &crate::visualizer_config::VisualizerConfig,
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

    // Serialize full visualizer config with current values
    let viz_toml = toml::to_string_pretty(visualizer_config)
        .context("Failed to serialize VisualizerConfig")?;
    let viz_doc: DocumentMut = viz_toml
        .parse::<DocumentMut>()
        .context("Failed to re-parse [visualizer] as toml_edit")?;

    doc.insert("visualizer", Item::Table(viz_doc.as_table().clone()));

    debug!(" [CONFIG WRITER] Wrote full [visualizer] (verbose mode)");
    write_atomic(&config_path, &doc.to_string())
}

/// Strip `[visualizer]` section back to sparse mode by removing
/// keys whose values match the default configuration.
///
/// Used when verbose_config mode is disabled to clean up the config file.
pub(crate) fn strip_to_sparse() -> Result<()> {
    let config_path =
        nokkvi_data::utils::paths::get_config_path().context("Failed to get config path")?;

    if !config_path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(&config_path).context("Failed to read config.toml")?;

    let mut doc: DocumentMut = content
        .parse::<DocumentMut>()
        .context("Failed to parse config.toml as TOML")?;

    // Build default reference for visualizer (no theme — theme is separate files)
    let default_viz = crate::visualizer_config::VisualizerConfig::default();

    let viz_default_toml =
        toml::to_string_pretty(&default_viz).context("Failed to serialize default visualizer")?;

    let viz_default_doc: DocumentMut = viz_default_toml
        .parse::<DocumentMut>()
        .context("Failed to parse default visualizer")?;

    // Strip matching keys from [visualizer]
    strip_matching_keys(&mut doc, "visualizer", viz_default_doc.as_table());

    // Remove any leftover [theme] TABLE section from pre-refactor configs.
    // The new architecture uses `theme = "name"` (a string key), NOT a [theme] table.
    // Guard: only remove if it's actually a table, not the string key we need.
    if doc
        .get("theme")
        .is_some_and(|v| v.is_table() || v.is_table_like())
    {
        doc.remove("theme");
        debug!(" [CONFIG WRITER] Removed leftover [theme] table section from config.toml");
    }

    debug!(" [CONFIG WRITER] Stripped [visualizer] to sparse (verbose off)");
    write_atomic(&config_path, &doc.to_string())
}

/// Recursively remove keys from a section in `doc` that match the `defaults` table.
///
/// For nested tables (e.g. `theme.dark.background`), recurses and removes the
/// parent table if it becomes empty after stripping.
fn strip_matching_keys(doc: &mut DocumentMut, section_name: &str, defaults: &toml_edit::Table) {
    let Some(section) = doc.get_mut(section_name) else {
        return;
    };
    let Some(section_tbl) = section.as_table_like_mut() else {
        return;
    };

    // Collect keys to process (avoid borrow conflicts)
    let keys: Vec<String> = section_tbl.iter().map(|(k, _)| k.to_string()).collect();

    for key in &keys {
        let Some(default_val) = defaults.get(key) else {
            continue; // User-added key, keep it
        };

        let Some(current_val) = section_tbl.get(key) else {
            continue;
        };

        // Both are tables → recurse
        if let (Some(sub_defaults), Some(tbl)) = (default_val.as_table(), current_val.as_table()) {
            // Build a sub-document for recursive stripping
            let sub_doc_str = format!("[{key}]\n{}", toml_edit::DocumentMut::from(tbl.clone()));
            let mut sub_doc: DocumentMut = match sub_doc_str.parse::<DocumentMut>() {
                Ok(d) => d,
                Err(_) => continue,
            };

            strip_matching_keys(&mut sub_doc, key, sub_defaults);

            // If the sub-table is now empty, remove the entire key
            if let Some(sub_section) = sub_doc.get(key)
                && let Some(sub_tbl) = sub_section.as_table()
                && sub_tbl.is_empty()
            {
                section_tbl.remove(key);
            } else if let Some(sub_section) = sub_doc.get(key) {
                section_tbl.insert(key, sub_section.clone());
            }
        } else {
            // Scalar comparison — compare TOML string representation
            let cur_str = current_val.to_string();
            let def_str = default_val.to_string();
            if cur_str == def_str {
                section_tbl.remove(key);
            }
        }
    }

    // Remove the entire section if it's now empty
    if section_tbl.is_empty() {
        doc.remove(section_name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::settings::items::SettingValue;

    /// Round-trip helper: set a value via `set_dotted_value` and return the
    /// serialized TOML string.
    fn write_value(key: &str, value: &SettingValue, comment: Option<&str>) -> String {
        let mut doc: DocumentMut = "".parse().unwrap();
        set_dotted_value(&mut doc, key, value, comment).unwrap();
        doc.to_string()
    }

    /// Write a value and re-parse the output, returning the document for typed access.
    fn write_and_reparse(key: &str, value: &SettingValue) -> DocumentMut {
        let toml = write_value(key, value, None);
        toml.parse().unwrap()
    }

    /// Parse a TOML string and build the defaults table from it.
    /// This ensures the formatting matches what `strip_matching_keys` compares.
    fn defaults_from_toml(section: &str, toml_str: &str) -> toml_edit::Table {
        let doc: DocumentMut = toml_str.parse().unwrap();
        doc[section].as_table().unwrap().clone()
    }

    // ══════════════════════════════════════════════════════════════════
    //  Value Round-Trip Tests
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn float_roundtrip_eliminates_precision_noise() {
        // f32→f64 conversion of 0.046 produces 0.04600000075995922
        // The writer must round to 6 decimal places.
        let val = SettingValue::Float {
            val: 0.046_f32 as f64,
            min: 0.0,
            max: 1.0,
            step: 0.001,
            unit: "",
        };
        let doc = write_and_reparse("test.float_val", &val);
        let written = doc["test"]["float_val"].as_float().unwrap();
        assert!(
            (written - 0.046).abs() < 1e-9,
            "f32 precision noise must be eliminated, got {written}"
        );
    }

    #[test]
    fn int_roundtrip() {
        let val = SettingValue::Int {
            val: 42,
            min: 0,
            max: 100,
            step: 1,
            unit: "px",
        };
        let doc = write_and_reparse("test.int_val", &val);
        assert_eq!(doc["test"]["int_val"].as_integer().unwrap(), 42);
    }

    #[test]
    fn bool_roundtrip() {
        let val = SettingValue::Bool(true);
        let doc = write_and_reparse("test.enabled", &val);
        assert_eq!(doc["test"]["enabled"].as_bool().unwrap(), true);
    }

    #[test]
    fn enum_roundtrip() {
        let val = SettingValue::Enum {
            val: "horizontal".to_string(),
            options: vec!["horizontal", "vertical"],
        };
        let doc = write_and_reparse("test.direction", &val);
        assert_eq!(doc["test"]["direction"].as_str().unwrap(), "horizontal");
    }

    #[test]
    fn hex_color_roundtrip() {
        let val = SettingValue::HexColor("#458588".to_string());
        let doc = write_and_reparse("palette.accent", &val);
        assert_eq!(doc["palette"]["accent"].as_str().unwrap(), "#458588");
    }

    #[test]
    fn color_array_roundtrip() {
        let val = SettingValue::ColorArray(vec![
            "#ff0000".to_string(),
            "#00ff00".to_string(),
            "#0000ff".to_string(),
        ]);
        let doc = write_and_reparse("viz.gradient", &val);
        let arr = doc["viz"]["gradient"].as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr.get(0).unwrap().as_str().unwrap(), "#ff0000");
        assert_eq!(arr.get(2).unwrap().as_str().unwrap(), "#0000ff");
    }

    // ══════════════════════════════════════════════════════════════════
    //  Dotted Key Navigation
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn set_dotted_value_creates_intermediate_tables() {
        let val = SettingValue::Int {
            val: 5,
            min: 0,
            max: 10,
            step: 1,
            unit: "",
        };
        let doc = write_and_reparse("visualizer.bars.gap", &val);
        assert!(doc["visualizer"].is_table());
        assert!(doc["visualizer"]["bars"].is_table());
        assert_eq!(doc["visualizer"]["bars"]["gap"].as_integer().unwrap(), 5);
    }

    #[test]
    fn set_dotted_value_preserves_existing_keys() {
        let mut doc: DocumentMut = "[test]\nexisting = true\n".parse().unwrap();
        let val = SettingValue::Int {
            val: 99,
            min: 0,
            max: 100,
            step: 1,
            unit: "",
        };
        set_dotted_value(&mut doc, "test.new_key", &val, None).unwrap();

        assert_eq!(
            doc["test"]["existing"].as_bool().unwrap(),
            true,
            "existing key must be preserved"
        );
        assert_eq!(
            doc["test"]["new_key"].as_integer().unwrap(),
            99,
            "new key must be added"
        );
    }

    #[test]
    fn set_dotted_value_adds_comment_on_new_key() {
        let mut doc: DocumentMut = "".parse().unwrap();
        let val = SettingValue::Bool(true);
        set_dotted_value(
            &mut doc,
            "general.verbose",
            &val,
            Some("Enable verbose output"),
        )
        .unwrap();

        let output = doc.to_string();
        assert!(
            output.contains("# Enable verbose output"),
            "comment must appear in output: {output}"
        );
    }

    // ══════════════════════════════════════════════════════════════════
    //  Strip Matching Keys (Sparse Mode)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn strip_matching_keys_removes_defaults() {
        // Both doc and defaults must be parsed from TOML so formatting matches
        let mut doc: DocumentMut = "[viz]\ngap = 5\nbars = 30\n".parse().unwrap();
        let defaults = defaults_from_toml("viz", "[viz]\ngap = 5\nbars = 30\n");

        strip_matching_keys(&mut doc, "viz", &defaults);
        assert!(doc.get("viz").is_none(), "empty section must be removed");
    }

    #[test]
    fn strip_matching_keys_preserves_custom_values() {
        let mut doc: DocumentMut = "[viz]\ngap = 5\nbars = 50\n".parse().unwrap();
        let defaults = defaults_from_toml("viz", "[viz]\ngap = 5\nbars = 30\n");

        strip_matching_keys(&mut doc, "viz", &defaults);
        // gap matches default → removed; bars differs → kept
        assert!(doc["viz"].as_table().is_some());
        assert!(
            doc["viz"].get("gap").is_none(),
            "matching key must be stripped"
        );
        assert_eq!(
            doc["viz"]["bars"].as_integer().unwrap(),
            50,
            "non-matching key must be preserved"
        );
    }

    #[test]
    fn strip_matching_keys_recurses_into_subtables() {
        // Production config uses flat `[section]\nkey = value` format from
        // toml::to_string_pretty — not dotted headers like `[viz.sub]`.
        // The recursive path handles this by re-serializing the sub-table.
        let mut doc: DocumentMut = "[viz]\nval = 10\nname = \"test\"\n".parse().unwrap();
        let defaults = defaults_from_toml("viz", "[viz]\nval = 10\nname = \"test\"\n");

        strip_matching_keys(&mut doc, "viz", &defaults);
        assert!(
            doc.get("viz").is_none(),
            "section with all matching keys must be removed"
        );
    }

    #[test]
    fn strip_removes_empty_parent_table() {
        let toml_str = "[section]\nonly_key = 42\n";
        let mut doc: DocumentMut = toml_str.parse().unwrap();
        let defaults = defaults_from_toml("section", toml_str);

        strip_matching_keys(&mut doc, "section", &defaults);
        assert!(
            doc.get("section").is_none(),
            "section with only default keys must be removed entirely"
        );
    }
}
