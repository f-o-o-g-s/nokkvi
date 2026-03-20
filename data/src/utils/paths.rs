//! Centralized path management for all application storage.
//!
//! All data is stored under `~/.config/nokkvi/`:
//! - `config.toml`: Server URL, username, theme & visualizer settings (user-editable)
//! - `app.redb`: Unified persistence (encrypted password, queue state, settings, hotkeys)
//! - `cache/`: Artwork and other cached data
//!   - `artwork/`: Album artwork cache
//!   - `artist_artwork/`: Artist artwork cache
//! - `sfx/`: User-customizable sound effects (WAV files, seeded from bundled defaults)

use std::path::PathBuf;

use anyhow::{Context, Result};

const APP_NAME: &str = "nokkvi";

/// Get the base application directory (~/.config/nokkvi)
pub fn get_app_dir() -> Result<PathBuf> {
    let base_dirs = directories::BaseDirs::new()
        .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;

    let app_dir = base_dirs.config_dir().join(APP_NAME);

    // Ensure directory exists
    if !app_dir.exists() {
        std::fs::create_dir_all(&app_dir).context(format!(
            "Failed to create app directory: {}",
            app_dir.display()
        ))?;
    }

    Ok(app_dir)
}

/// Get the credentials config file path (~/.config/nokkvi/config.toml)
pub fn get_config_path() -> Result<PathBuf> {
    Ok(get_app_dir()?.join("config.toml"))
}

/// Get the unified app database path (~/.config/nokkvi/app.redb)
/// Both QueueManager and SettingsManager share this single database file
/// via a cloneable `StateStorage` backed by `Arc<Database>`.
pub fn get_app_db_path() -> Result<PathBuf> {
    Ok(get_app_dir()?.join("app.redb"))
}

/// Get the cache directory path (~/.config/nokkvi/cache)
pub fn get_cache_dir() -> Result<PathBuf> {
    let cache_dir = get_app_dir()?.join("cache");

    // Ensure directory exists
    if !cache_dir.exists() {
        std::fs::create_dir_all(&cache_dir).context(format!(
            "Failed to create cache directory: {}",
            cache_dir.display()
        ))?;
    }

    Ok(cache_dir)
}

/// Get a specific cache subdirectory path
/// (~/.config/nokkvi/cache/{name})
pub fn get_cache_subdir(name: &str) -> Result<PathBuf> {
    let subdir = get_cache_dir()?.join(name);

    // Ensure directory exists
    if !subdir.exists() {
        std::fs::create_dir_all(&subdir).context(format!(
            "Failed to create cache subdirectory: {}",
            subdir.display()
        ))?;
    }

    Ok(subdir)
}

/// Get the sound effects directory (~/.config/nokkvi/sfx)
///
/// Users can customize sounds by placing WAV files here.
/// On first run, bundled defaults are seeded into this directory.
pub fn get_sfx_dir() -> Result<PathBuf> {
    let sfx_dir = get_app_dir()?.join("sfx");

    // Ensure directory exists
    if !sfx_dir.exists() {
        std::fs::create_dir_all(&sfx_dir).context(format!(
            "Failed to create sfx directory: {}",
            sfx_dir.display()
        ))?;
    }

    Ok(sfx_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paths_are_under_config() {
        let app_dir = get_app_dir().unwrap();
        assert!(app_dir.to_string_lossy().contains(".config/nokkvi"));

        let config_path = get_config_path().unwrap();
        assert!(config_path.starts_with(&app_dir));

        let db_path = get_app_db_path().unwrap();
        assert!(db_path.starts_with(&app_dir));

        let cache_dir = get_cache_dir().unwrap();
        assert!(cache_dir.starts_with(&app_dir));
    }
}
