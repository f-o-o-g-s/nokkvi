//! Centralized path management for all application storage.
//!
//! All data is stored under `~/.config/nokkvi/`:
//! - `config.toml`: Server URL, username, theme & visualizer settings (user-editable)
//! - `app.redb`: Unified persistence (session tokens, queue state, settings, hotkeys)
//! - `themes/`: Color-only theme TOML files (built-ins seeded on first run)
//! - `sfx/`: User-customizable sound effects (WAV files, seeded from bundled defaults)
//!
//! Artwork is fetched on demand from the Navidrome server — there is no client-side
//! cache directory.

use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};

const APP_NAME: &str = "nokkvi";

/// Timestamp (ms since epoch) of the last config.toml write initiated by the app itself.
/// Used to prevent hot-reload feedback loops when the UI updates a setting.
pub static LAST_INTERNAL_WRITE: AtomicU64 = AtomicU64::new(0);

/// Wrapper to execute a config write and record its timestamp, suppressing
/// the hot-reload file watcher for the next 500ms.
pub fn suppress_config_reload<T>(f: impl FnOnce() -> T) -> T {
    let result = f();
    if let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) {
        LAST_INTERNAL_WRITE.store(now.as_millis() as u64, Ordering::Release);
    }
    result
}

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

/// Get the themes directory (~/.config/nokkvi/themes)
///
/// Contains color-only theme TOML files. Built-in themes are seeded
/// here on first run; users can edit in-place or add custom themes.
pub fn get_themes_dir() -> Result<PathBuf> {
    let themes_dir = get_app_dir()?.join("themes");

    // Ensure directory exists
    if !themes_dir.exists() {
        std::fs::create_dir_all(&themes_dir).context(format!(
            "Failed to create themes directory: {}",
            themes_dir.display()
        ))?;
    }

    Ok(themes_dir)
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
    }
}
