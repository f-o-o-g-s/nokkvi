//! Centralized path management for all application storage.
//!
//! Per-user files split between two XDG basedir roots:
//!
//! - `$XDG_CONFIG_HOME/nokkvi/` (`~/.config/nokkvi/`):
//!   - `config.toml`: server URL, username, theme & visualizer settings (user-editable)
//!   - `themes/`: color-only theme TOML files (built-ins seeded on first run)
//!   - `sfx/`: user-customizable sound effects (WAV files, seeded from bundled defaults)
//!
//! - `$XDG_STATE_HOME/nokkvi/` (`~/.local/state/nokkvi/`):
//!   - `app.redb`: unified persistence (session tokens, queue state, settings, hotkeys)
//!   - `nokkvi.log`: file log (truncated on every launch)
//!
//! Artwork is fetched on demand from the Navidrome server — there is no client-side
//! cache directory.

use std::{
    path::PathBuf,
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};

const APP_NAME: &str = "nokkvi";

/// Guards the legacy `~/.config/nokkvi/` → `~/.local/state/nokkvi/` migration
/// so it runs at most once per process. The migration is invoked explicitly
/// from `main` after tracing is initialised — earlier callers (notably the
/// tracing init itself, via `get_log_path`) would lose their log output to a
/// not-yet-registered subscriber.
static MIGRATION_DONE: OnceLock<()> = OnceLock::new();

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

/// Get the configuration directory (`~/.config/nokkvi`).
///
/// Holds user-editable configuration: `config.toml`, `themes/`, `sfx/`.
pub fn get_app_dir() -> Result<PathBuf> {
    let base_dirs = directories::BaseDirs::new()
        .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;

    let app_dir = base_dirs.config_dir().join(APP_NAME);

    if !app_dir.exists() {
        std::fs::create_dir_all(&app_dir).context(format!(
            "Failed to create app directory: {}",
            app_dir.display()
        ))?;
    }

    Ok(app_dir)
}

/// Get the state directory (`~/.local/state/nokkvi`).
///
/// Holds runtime state and logs that aren't user-editable configuration:
/// `app.redb`, `nokkvi.log`. Per the XDG Base Directory Specification,
/// `$XDG_STATE_HOME` is the right home for this kind of persisted-but-not-
/// portable data.
pub fn get_state_dir() -> Result<PathBuf> {
    let base_dirs = directories::BaseDirs::new()
        .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;

    let state_dir = base_dirs
        .state_dir()
        .context("$XDG_STATE_HOME unavailable on this platform")?
        .join(APP_NAME);

    if !state_dir.exists() {
        std::fs::create_dir_all(&state_dir).context(format!(
            "Failed to create state directory: {}",
            state_dir.display()
        ))?;
    }

    Ok(state_dir)
}

/// Get the credentials config file path (`~/.config/nokkvi/config.toml`).
pub fn get_config_path() -> Result<PathBuf> {
    Ok(get_app_dir()?.join("config.toml"))
}

/// Get the unified app database path (`~/.local/state/nokkvi/app.redb`).
///
/// Both QueueManager and SettingsManager share this single database file
/// via a cloneable `StateStorage` backed by `Arc<Database>`.
pub fn get_app_db_path() -> Result<PathBuf> {
    Ok(get_state_dir()?.join("app.redb"))
}

/// Get the file log path (`~/.local/state/nokkvi/nokkvi.log`).
///
/// Truncated on every launch. Captures full debug context for bug reports
/// regardless of how the app was launched (terminal, .desktop file, hotkey).
pub fn get_log_path() -> Result<PathBuf> {
    Ok(get_state_dir()?.join("nokkvi.log"))
}

/// Get the sound effects directory (`~/.config/nokkvi/sfx`).
///
/// Users can customize sounds by placing WAV files here.
/// On first run, bundled defaults are seeded into this directory.
pub fn get_sfx_dir() -> Result<PathBuf> {
    let sfx_dir = get_app_dir()?.join("sfx");

    if !sfx_dir.exists() {
        std::fs::create_dir_all(&sfx_dir).context(format!(
            "Failed to create sfx directory: {}",
            sfx_dir.display()
        ))?;
    }

    Ok(sfx_dir)
}

/// Get the themes directory (`~/.config/nokkvi/themes`).
///
/// Contains color-only theme TOML files. Built-in themes are seeded
/// here on first run; users can edit in-place or add custom themes.
pub fn get_themes_dir() -> Result<PathBuf> {
    let themes_dir = get_app_dir()?.join("themes");

    if !themes_dir.exists() {
        std::fs::create_dir_all(&themes_dir).context(format!(
            "Failed to create themes directory: {}",
            themes_dir.display()
        ))?;
    }

    Ok(themes_dir)
}

/// One-time migration from the legacy "everything under ~/.config/nokkvi/"
/// layout to the XDG-compliant split layout. OnceLock-guarded internally,
/// so repeat calls in the same process are cheap no-ops.
///
/// Call this from `main` after tracing is initialised so the migration's log
/// output is visible. (Calling it during tracing init would lose the output
/// because the subscriber isn't registered yet.)
///
/// Behavior:
/// - If `~/.config/nokkvi/app.redb` exists and the new location does not,
///   move it via `rename` (cross-filesystem fallback to copy + remove).
/// - If both locations have an `app.redb`, prefer the new one and warn —
///   the old file is left in place so the user can inspect / delete it.
/// - If `~/.config/nokkvi/nokkvi.log` exists, delete it. The log is
///   truncated on every launch, so a stale orphan in the config dir
///   has no value.
pub fn migrate_to_state_dir() {
    if MIGRATION_DONE.set(()).is_err() {
        return;
    }

    let Ok(config_dir) = get_app_dir() else {
        return;
    };
    let Ok(state_dir) = get_state_dir() else {
        return;
    };

    let old_db = config_dir.join("app.redb");
    let new_db = state_dir.join("app.redb");
    if old_db.exists() {
        if new_db.exists() {
            tracing::warn!(
                target: "nokkvi::migration",
                " [migration] app.redb exists in both {} and {} — using the new location, leaving the old file in place for manual cleanup",
                old_db.display(),
                new_db.display()
            );
        } else {
            match std::fs::rename(&old_db, &new_db) {
                Ok(()) => tracing::info!(
                    target: "nokkvi::migration",
                    " [migration] moved app.redb {} → {}",
                    old_db.display(),
                    new_db.display()
                ),
                Err(e) => {
                    tracing::debug!(
                        " [migration] rename failed ({e}), falling back to copy + remove"
                    );
                    if let Err(e) = std::fs::copy(&old_db, &new_db) {
                        tracing::warn!(
                            target: "nokkvi::migration",
                            " [migration] failed to copy {} → {}: {e}",
                            old_db.display(),
                            new_db.display()
                        );
                    } else if let Err(e) = std::fs::remove_file(&old_db) {
                        tracing::warn!(
                            target: "nokkvi::migration",
                            " [migration] copied app.redb but failed to remove old {}: {e}",
                            old_db.display()
                        );
                    } else {
                        tracing::info!(
                            target: "nokkvi::migration",
                            " [migration] moved app.redb {} → {} (copy fallback)",
                            old_db.display(),
                            new_db.display()
                        );
                    }
                }
            }
        }
    }

    let old_log = config_dir.join("nokkvi.log");
    if old_log.exists() {
        match std::fs::remove_file(&old_log) {
            Ok(()) => tracing::info!(
                target: "nokkvi::migration",
                " [migration] removed orphaned log file {}",
                old_log.display()
            ),
            Err(e) => tracing::debug!(
                " [migration] failed to remove orphaned log {}: {e}",
                old_log.display()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_paths_under_config_dir() {
        let app_dir = get_app_dir().unwrap();
        assert!(app_dir.to_string_lossy().contains(".config/nokkvi"));

        let config_path = get_config_path().unwrap();
        assert!(config_path.starts_with(&app_dir));
    }

    #[test]
    fn test_state_paths_under_state_dir() {
        let state_dir = get_state_dir().unwrap();
        assert!(state_dir.to_string_lossy().contains(".local/state/nokkvi"));

        let db_path = get_app_db_path().unwrap();
        assert!(db_path.starts_with(&state_dir));

        let log_path = get_log_path().unwrap();
        assert!(log_path.starts_with(&state_dir));
    }
}
