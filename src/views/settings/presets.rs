//! Preset theme definitions — embedded at compile time
//!
//! Each preset contains a complete config.toml (theme + visualizer settings).
//! Applying a preset overwrites the user's config.toml atomically, and
//! the hot-reload watcher picks up the change.

use anyhow::{Context, Result};
use tracing::debug;

// ============================================================================
// Preset Definitions
// ============================================================================

/// A preset theme with embedded TOML content
#[derive(Debug, Clone)]
pub(crate) struct ThemePreset {
    /// Display name shown in the slot list
    pub name: &'static str,
    /// Short description shown as subtitle
    pub description: &'static str,
    /// Full config.toml content, embedded at compile time
    pub toml_content: &'static str,
}

/// All available preset themes, ordered for display
pub(crate) fn all_presets() -> Vec<ThemePreset> {
    vec![
        ThemePreset {
            name: "Gruvbox Blue",
            description: "Gruvbox dark hard with blue accents",
            toml_content: include_str!("../../../example_themes/gruvbox_dark_hard_blue.toml"),
        },
        ThemePreset {
            name: "Gruvbox Red",
            description: "Gruvbox dark hard with red accents",
            toml_content: include_str!("../../../example_themes/gruvbox_dark_hard_red.toml"),
        },
        ThemePreset {
            name: "Catppuccin",
            description: "Mocha (dark) / Latte (light) — pastel palette",
            toml_content: include_str!("../../../example_themes/config_catppuccin.toml"),
        },
        ThemePreset {
            name: "Dracula",
            description: "Classic dark theme with purple accents",
            toml_content: include_str!("../../../example_themes/config_dracula.toml"),
        },
        ThemePreset {
            name: "Everforest",
            description: "Warm green tones inspired by nature",
            toml_content: include_str!("../../../example_themes/config_everforest.toml"),
        },
        ThemePreset {
            name: "Kanagawa",
            description: "Inspired by Katsushika Hokusai — deep blues",
            toml_content: include_str!("../../../example_themes/config_kanagawa.toml"),
        },
        ThemePreset {
            name: "Nord",
            description: "Arctic, north-bluish clean palette",
            toml_content: include_str!("../../../example_themes/config_nord.toml"),
        },
        ThemePreset {
            name: "Bio-Luminal Swamplab",
            description: "Bioluminescent swamp aesthetic",
            toml_content: include_str!("../../../example_themes/config_bio_luminal_swamplab.toml"),
        },
        ThemePreset {
            name: "Cryo",
            description: "Frozen, icy cool tones",
            toml_content: include_str!("../../../example_themes/cryo.toml"),
        },
        ThemePreset {
            name: "Ember",
            description: "Warm glowing ember tones",
            toml_content: include_str!("../../../example_themes/ember.toml"),
        },
    ]
}

// ============================================================================
// Preset Application
// ============================================================================

/// Apply a preset by atomically writing its TOML content to config.toml.
///
/// Preserves the user's `server_url` and `username` from the existing config.
/// Password is stored in redb, so it is unaffected by preset application.
pub(crate) fn apply_preset(preset: &ThemePreset) -> Result<()> {
    let config_path =
        nokkvi_data::utils::paths::get_config_path().context("Failed to get config path")?;

    // Read existing credentials before overwriting
    let existing_content = if config_path.exists() {
        std::fs::read_to_string(&config_path).ok()
    } else {
        None
    };

    // Parse existing config for user-editable credentials
    let (server_url, username) = if let Some(ref content) = existing_content {
        let doc: toml_edit::DocumentMut = content.parse().unwrap_or_default();
        let server_url = doc
            .get("server_url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let username = doc
            .get("username")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        (server_url, username)
    } else {
        (String::new(), String::new())
    };

    // Parse the preset TOML and inject existing credentials
    let mut doc: toml_edit::DocumentMut = preset
        .toml_content
        .parse()
        .context("Failed to parse preset TOML")?;

    // Overwrite credentials with existing values (don't wipe user's login)
    if !server_url.is_empty() {
        doc["server_url"] = toml_edit::value(server_url);
    }
    if !username.is_empty() {
        doc["username"] = toml_edit::value(username);
    }

    // Write atomically via temp file + rename
    let temp_path = config_path.with_extension("toml.tmp");
    std::fs::write(&temp_path, doc.to_string())
        .with_context(|| format!("Failed to write temp file: {}", temp_path.display()))?;
    std::fs::rename(&temp_path, &config_path)
        .with_context(|| format!("Failed to rename temp file to: {}", config_path.display()))?;

    debug!(" [PRESETS] Applied preset: {}", preset.name);
    Ok(())
}
