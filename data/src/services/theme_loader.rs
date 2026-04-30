//! Theme loader — seeding, discovery, and I/O for theme files.
//!
//! Built-in themes are compiled into the binary via `include_str!` and seeded
//! to `~/.config/nokkvi/themes/` on first run. All runtime I/O reads/writes
//! from the user's themes directory.

use std::{collections::HashMap, path::PathBuf};

use anyhow::{Context, Result};
use tracing::{debug, info, warn};

use crate::{types::theme_file::ThemeFile, utils::paths::get_themes_dir};

// ============================================================================
// Built-in theme registry (compiled into the binary)
// ============================================================================

/// A built-in theme: (filename stem, display name, TOML content).
struct BuiltinTheme {
    stem: &'static str,
    content: &'static str,
}

/// All built-in themes. Order matters — first is the default.
const BUILTIN_THEMES: &[BuiltinTheme] = &[
    BuiltinTheme {
        stem: "everforest",
        content: include_str!("../../../themes/everforest.toml"),
    },
    BuiltinTheme {
        stem: "adwaita",
        content: include_str!("../../../themes/adwaita.toml"),
    },
    BuiltinTheme {
        stem: "bio_luminal_swamplab",
        content: include_str!("../../../themes/bio_luminal_swamplab.toml"),
    },
    BuiltinTheme {
        stem: "catppuccin_frappe",
        content: include_str!("../../../themes/catppuccin_frappe.toml"),
    },
    BuiltinTheme {
        stem: "catppuccin_macchiato",
        content: include_str!("../../../themes/catppuccin_macchiato.toml"),
    },
    BuiltinTheme {
        stem: "catppuccin",
        content: include_str!("../../../themes/catppuccin.toml"),
    },
    BuiltinTheme {
        stem: "cryo",
        content: include_str!("../../../themes/cryo.toml"),
    },
    BuiltinTheme {
        stem: "dracula",
        content: include_str!("../../../themes/dracula.toml"),
    },
    BuiltinTheme {
        stem: "ember",
        content: include_str!("../../../themes/ember.toml"),
    },
    BuiltinTheme {
        stem: "ferra",
        content: include_str!("../../../themes/ferra.toml"),
    },
    BuiltinTheme {
        stem: "gruvbox",
        content: include_str!("../../../themes/gruvbox.toml"),
    },
    BuiltinTheme {
        stem: "iced",
        content: include_str!("../../../themes/iced.toml"),
    },
    BuiltinTheme {
        stem: "kanagawa_dragon",
        content: include_str!("../../../themes/kanagawa_dragon.toml"),
    },
    BuiltinTheme {
        stem: "kanagawa",
        content: include_str!("../../../themes/kanagawa.toml"),
    },
    BuiltinTheme {
        stem: "moonfly",
        content: include_str!("../../../themes/moonfly.toml"),
    },
    BuiltinTheme {
        stem: "nightfly",
        content: include_str!("../../../themes/nightfly.toml"),
    },
    BuiltinTheme {
        stem: "nord",
        content: include_str!("../../../themes/nord.toml"),
    },
    BuiltinTheme {
        stem: "oxocarbon",
        content: include_str!("../../../themes/oxocarbon.toml"),
    },
    BuiltinTheme {
        stem: "solarized",
        content: include_str!("../../../themes/solarized.toml"),
    },
    BuiltinTheme {
        stem: "tokyo_night_storm",
        content: include_str!("../../../themes/tokyo_night_storm.toml"),
    },
    BuiltinTheme {
        stem: "tokyo_night",
        content: include_str!("../../../themes/tokyo_night.toml"),
    },
];

/// Lazy map from stem → TOML content for O(1) lookup.
/// Built once (on first access) via `LazyLock` to avoid reconstructing the
/// `HashMap` every time `load_builtin_theme()` or `discover_themes()` is called.
fn builtin_registry() -> &'static HashMap<&'static str, &'static str> {
    use std::sync::LazyLock;
    static REGISTRY: LazyLock<HashMap<&'static str, &'static str>> =
        LazyLock::new(|| BUILTIN_THEMES.iter().map(|t| (t.stem, t.content)).collect());
    &REGISTRY
}

// ============================================================================
// Public info type
// ============================================================================

/// Metadata about a discovered theme.
#[derive(Debug, Clone)]
pub struct ThemeInfo {
    /// Filename stem (e.g., "everforest")
    pub stem: String,
    /// Human-readable display name from the theme file
    pub display_name: String,
    /// Full path on disk
    pub path: PathBuf,
    /// Whether this theme has a built-in counterpart
    pub is_builtin: bool,
}

// ============================================================================
// Seeding
// ============================================================================

/// Seed any missing built-in themes to the user's themes directory.
///
/// Only writes files that don't already exist — never overwrites user edits.
/// Called once at startup.
pub fn seed_builtin_themes() -> Result<()> {
    let themes_dir = get_themes_dir()?;

    for builtin in BUILTIN_THEMES {
        let path = themes_dir.join(format!("{}.toml", builtin.stem));
        if !path.exists() {
            std::fs::write(&path, builtin.content).with_context(|| {
                format!(
                    "Failed to seed theme '{}' to {}",
                    builtin.stem,
                    path.display()
                )
            })?;
            info!(theme = builtin.stem, "Seeded built-in theme");
        }
    }

    Ok(())
}

// ============================================================================
// Discovery
// ============================================================================

/// Scan the themes directory and return metadata for every `.toml` file found.
///
/// Themes are sorted alphabetically by display name.
pub fn discover_themes() -> Result<Vec<ThemeInfo>> {
    let themes_dir = get_themes_dir()?;
    let registry = builtin_registry();
    let mut themes = Vec::new();

    let entries = std::fs::read_dir(&themes_dir)
        .with_context(|| format!("Failed to read themes directory: {}", themes_dir.display()))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }

        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };

        // Read display name from the file (fast: only parse the `name` field)
        let display_name = match std::fs::read_to_string(&path) {
            Ok(content) => match ThemeFile::load(&content) {
                Ok(tf) => tf.name,
                Err(e) => {
                    warn!(theme = %stem, error = %e, "Skipping malformed theme file");
                    continue;
                }
            },
            Err(e) => {
                warn!(theme = %stem, error = %e, "Failed to read theme file");
                continue;
            }
        };

        themes.push(ThemeInfo {
            is_builtin: registry.contains_key(stem.as_str()),
            stem,
            display_name,
            path,
        });
    }

    themes.sort_by(|a, b| {
        a.display_name
            .to_lowercase()
            .cmp(&b.display_name.to_lowercase())
    });
    Ok(themes)
}

// ============================================================================
// Load / Save
// ============================================================================

/// Load a theme by stem name from `~/.config/nokkvi/themes/{name}.toml`.
///
/// Falls back to the Adwaita default if the file is missing or corrupt.
pub fn load_theme(name: &str) -> ThemeFile {
    match try_load_theme(name) {
        Ok(theme) => theme,
        Err(e) => {
            warn!(
                theme = name,
                error = %e,
                "Failed to load theme, falling back to Adwaita default"
            );
            ThemeFile::default()
        }
    }
}

/// Try to load a theme, returning an error on failure.
fn try_load_theme(name: &str) -> Result<ThemeFile> {
    let themes_dir = get_themes_dir()?;
    let path = themes_dir.join(format!("{name}.toml"));

    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Theme file not found: {}", path.display()))?;

    ThemeFile::load(&content)
        .with_context(|| format!("Failed to parse theme file: {}", path.display()))
}

/// Load the original default values for a built-in theme by stem name.
/// Returns `None` if the theme is not built-in or fails to parse.
pub fn load_builtin_theme(name: &str) -> Option<ThemeFile> {
    builtin_registry()
        .get(name)
        .and_then(|content| ThemeFile::load(content).ok())
}

/// Save a theme to `~/.config/nokkvi/themes/{name}.toml`.
pub fn save_theme(name: &str, theme: &ThemeFile) -> Result<()> {
    let themes_dir = get_themes_dir()?;
    let path = themes_dir.join(format!("{name}.toml"));

    let content = theme.save().context("Failed to serialize theme")?;

    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write theme file: {}", path.display()))?;

    debug!(theme = name, "Saved theme file");
    Ok(())
}

/// Restore a built-in theme by overwriting the user's copy with the original.
///
/// Returns `Err` if the theme is not a built-in.
pub fn restore_builtin(name: &str) -> Result<()> {
    let registry = builtin_registry();
    let content = registry
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("'{name}' is not a built-in theme"))?;

    let themes_dir = get_themes_dir()?;
    let path = themes_dir.join(format!("{name}.toml"));

    std::fs::write(&path, content)
        .with_context(|| format!("Failed to restore theme: {}", path.display()))?;

    info!(theme = name, "Restored built-in theme to defaults");
    Ok(())
}

// ============================================================================
// Config.toml helpers
// ============================================================================

/// Default theme name when none is configured.
pub const DEFAULT_THEME: &str = "everforest";

/// Read the `theme = "..."` key from config.toml.
///
/// Returns [`DEFAULT_THEME`] if the key is missing or the file can't be read.
pub fn read_theme_name_from_config() -> String {
    match try_read_theme_name() {
        Ok(name) => name,
        Err(e) => {
            debug!(error = %e, "Could not read theme name from config, using default");
            DEFAULT_THEME.to_string()
        }
    }
}

fn try_read_theme_name() -> Result<String> {
    let config_path = crate::utils::paths::get_config_path()?;
    let content = std::fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read config.toml: {}", config_path.display()))?;

    extract_theme_name_from_toml(&content)
}

fn extract_theme_name_from_toml(content: &str) -> Result<String> {
    // Quick parse: just extract the top-level `theme` key
    let doc: toml::Table =
        toml::from_str(content).context("Failed to parse config.toml for theme key")?;

    match doc.get("theme").and_then(|v| v.as_str()) {
        Some(name) if !name.is_empty() => Ok(name.to_string()),
        _ => Ok(DEFAULT_THEME.to_string()),
    }
}

/// Write the `theme = "..."` key to config.toml using toml_edit (preserves structure).
pub fn write_theme_name_to_config(name: &str) -> Result<()> {
    let config_path = crate::utils::paths::get_config_path()?;

    let content = std::fs::read_to_string(&config_path).unwrap_or_default();

    let mut doc: toml_edit::DocumentMut = content
        .parse()
        .context("Failed to parse config.toml for editing")?;

    doc["theme"] = toml_edit::value(name);

    crate::utils::paths::suppress_config_reload(|| std::fs::write(&config_path, doc.to_string()))
        .with_context(|| "Failed to write theme name to config.toml")?;

    info!(theme = name, "Updated theme name in config.toml");
    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    /// Override themes dir for test isolation.
    fn seed_to_temp() -> (TempDir, Vec<ThemeInfo>) {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();

        // Write built-in themes to temp dir
        for builtin in BUILTIN_THEMES {
            let path = dir.join(format!("{}.toml", builtin.stem));
            fs::write(&path, builtin.content).unwrap();
        }

        // Discover from temp dir
        let mut themes = Vec::new();
        let registry = builtin_registry();
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            let stem = path.file_stem().unwrap().to_str().unwrap().to_string();
            let content = fs::read_to_string(&path).unwrap();
            let tf = ThemeFile::load(&content).unwrap();
            themes.push(ThemeInfo {
                is_builtin: registry.contains_key(stem.as_str()),
                stem,
                display_name: tf.name,
                path,
            });
        }
        themes.sort_by(|a, b| {
            a.display_name
                .to_lowercase()
                .cmp(&b.display_name.to_lowercase())
        });

        (tmp, themes)
    }

    #[test]
    fn test_all_builtin_themes_parse() {
        let registry = builtin_registry();
        assert_eq!(registry.len(), 21, "Expected 21 built-in themes");

        for builtin in BUILTIN_THEMES {
            let theme = ThemeFile::load(builtin.content).unwrap_or_else(|e| {
                panic!("Failed to parse built-in theme '{}': {}", builtin.stem, e);
            });
            assert!(
                !theme.name.is_empty(),
                "Theme '{}' has empty display name",
                builtin.stem
            );
            assert!(
                !theme.dark.background.hard.is_empty(),
                "Theme '{}' missing dark.background.hard",
                builtin.stem
            );
            assert!(
                !theme.light.background.hard.is_empty(),
                "Theme '{}' missing light.background.hard",
                builtin.stem
            );
        }
    }

    #[test]
    fn test_seed_and_discover() {
        let (_tmp, themes) = seed_to_temp();
        assert_eq!(themes.len(), 21);
        assert!(themes.iter().all(|t| t.is_builtin));

        // Check a specific one
        let everforest = themes.iter().find(|t| t.stem == "everforest").unwrap();
        assert_eq!(everforest.display_name, "Everforest");
    }

    #[test]
    fn test_round_trip_all_themes() {
        for builtin in BUILTIN_THEMES {
            let theme = ThemeFile::load(builtin.content).unwrap();
            let serialized = theme.save().unwrap();
            let reparsed = ThemeFile::load(&serialized).unwrap_or_else(|e| {
                panic!(
                    "Round-trip failed for '{}': {}\n\nSerialized:\n{}",
                    builtin.stem, e, serialized
                );
            });
            assert_eq!(
                theme.name, reparsed.name,
                "Name mismatch for {}",
                builtin.stem
            );
            assert_eq!(
                theme.dark.background.hard, reparsed.dark.background.hard,
                "dark.background.hard mismatch for {}",
                builtin.stem
            );
            assert_eq!(
                theme.dark.visualizer.bar_gradient_colors.len(),
                reparsed.dark.visualizer.bar_gradient_colors.len(),
                "dark.visualizer.bar_gradient_colors length mismatch for {}",
                builtin.stem
            );
        }
    }

    #[test]
    fn test_restore_builtin_content() {
        let registry = builtin_registry();
        assert!(registry.contains_key("gruvbox"));
        assert!(registry.contains_key("everforest"));
        assert!(!registry.contains_key("nonexistent"));
    }

    #[test]
    fn test_default_theme_name() {
        assert_eq!(DEFAULT_THEME, "adwaita");
    }

    #[test]
    fn test_read_theme_config_missing_key() {
        // Simulate a config.toml where the theme key doesn't exist at all
        let content = "[visualizer]\nbars = 10\n";
        let name = extract_theme_name_from_toml(content).expect("Should parse cleanly");
        assert_eq!(
            name, DEFAULT_THEME,
            "Missing key should fallback to default theme"
        );
    }

    #[test]
    fn test_read_theme_config_malformed() {
        // Simulate a totally malformed config.toml
        let content = "this is garbage [[[\0";
        let result = extract_theme_name_from_toml(content);
        assert!(
            result.is_err(),
            "Garbage should fail to parse, triggering the outer fallback"
        );
    }

    #[test]
    fn test_corrupted_theme_fallback() {
        // Simulate a corrupted theme file (e.g. users editing manually and saving a syntax error)
        let garbage = "name = \"MyTheme\"\n[dark\nbackground = { hard = \"#fff\" }";
        let result = ThemeFile::load(garbage);
        assert!(result.is_err(), "Corrupted theme TOML should return Err()");
        // Verify that the fallback mechanism would trigger Adwaita (which it does in load_theme)
        let default_theme = ThemeFile::default();
        assert_eq!(default_theme.name, "Adwaita");
    }
}
