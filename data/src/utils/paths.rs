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
    collections::{HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use parking_lot::Mutex;

const APP_NAME: &str = "nokkvi";

/// Config filename. Debug builds use a separate file so `cargo run` against a
/// dev build doesn't clobber the user's real `config.toml` (server creds,
/// theme, hotkeys, etc.).
#[cfg(debug_assertions)]
const CONFIG_FILENAME: &str = "config.debug.toml";
#[cfg(not(debug_assertions))]
const CONFIG_FILENAME: &str = "config.toml";

/// Guards the legacy `~/.config/nokkvi/` → `~/.local/state/nokkvi/` migration
/// so it runs at most once per process. The migration is invoked explicitly
/// from `main` after tracing is initialised — earlier callers (notably the
/// tracing init itself, via `get_log_path`) would lose their log output to a
/// not-yet-registered subscriber.
static MIGRATION_DONE: OnceLock<()> = OnceLock::new();

/// How long an internal-write record remains valid for self-write suppression.
/// A config-watcher event that matches a recorded `(path, content-hash)` inside
/// this window is treated as the app's own write and ignored; anything older or
/// non-matching is a genuine external edit and reloads.
const SUPPRESSION_WINDOW: Duration = Duration::from_millis(500);

/// One recorded internal write: the content hash of the bytes that were written
/// and the monotonic instant at which the write landed. `recorded_at` uses
/// `Instant` (not wall-clock) so `elapsed()` is saturating and a backward OS
/// clock step (NTP, suspend/resume, manual change) can never underflow the
/// window comparison.
#[derive(Clone, Copy)]
struct WriteRecord {
    content_hash: u64,
    recorded_at: Instant,
}

/// Per-path registry of the app's own recent config/theme writes, keyed by the
/// normalized file path. Replaces the former single process-global wall-clock
/// timestamp: identity-based suppression (exact path + content hash, within a
/// monotonic recency window) lets a genuine external edit reload even when it
/// lands inside the time shadow of an unrelated internal write, while still
/// ignoring true self-writes that would otherwise trigger a hot-reload
/// feedback loop.
static INTERNAL_WRITES: Mutex<Option<HashMap<PathBuf, WriteRecord>>> = Mutex::new(None);

/// Hash arbitrary bytes with the std `DefaultHasher`. Config/theme files are
/// small, so hashing the full contents is cheap and precisely distinguishes
/// "the exact bytes we wrote" from "a different external edit".
pub fn hash_config_bytes(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

/// Normalize a path for registry keying so the record side (`write_atomic`,
/// raw paths) and the check side (`poll_changes`, which receives canonicalized
/// config paths and raw inotify theme paths) reconcile. `canonicalize` resolves
/// symlinks the same way inotify does; on error (file already renamed away,
/// permission) fall back to the path as-given so a missing canonical form does
/// not silently disable suppression.
fn normalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Record that the app itself just wrote `content` to `path`. Called from
/// `write_atomic` after the temp file is renamed into place, using the exact
/// bytes written (never a re-read) so an external edit landing between write
/// and read cannot be mistaken for the self-write.
pub fn record_internal_write(path: &Path, content: &str) {
    let record = WriteRecord {
        content_hash: hash_config_bytes(content.as_bytes()),
        recorded_at: Instant::now(),
    };
    let mut guard = INTERNAL_WRITES.lock();
    let map = guard.get_or_insert_with(HashMap::new);
    // Opportunistically evict stale entries so the map stays bounded to the
    // handful of config/theme files written in any 500ms window.
    map.retain(|_, rec| rec.recorded_at.elapsed() < SUPPRESSION_WINDOW);
    map.insert(normalize_path(path), record);
}

/// Returns `true` only when the registry holds an entry for `path` whose stored
/// content hash equals `content_hash` AND was recorded within the monotonic
/// suppression window. Any mismatch (unknown path, different content, or a stale
/// record) returns `false` so the change is treated as a genuine external edit
/// and reloads — the fail-toward-showing-the-user's-edit direction.
pub fn was_internal_write(path: &Path, content_hash: u64) -> bool {
    let key = normalize_path(path);
    let mut guard = INTERNAL_WRITES.lock();
    let Some(map) = guard.as_mut() else {
        return false;
    };
    // Evict stale entries opportunistically on the read side too.
    map.retain(|_, rec| rec.recorded_at.elapsed() < SUPPRESSION_WINDOW);
    match map.get(&key) {
        Some(rec) => {
            rec.content_hash == content_hash && rec.recorded_at.elapsed() < SUPPRESSION_WINDOW
        }
        None => false,
    }
}

/// Monotonic counter used to build unique temp-file names for `write_atomic`.
/// A fixed temp suffix (e.g. `"config.toml.tmp"`) would race when two writers
/// land on the same path concurrently — the counter eliminates collisions
/// without forcing a global write lock.
static TEMP_WRITE_ID: AtomicUsize = AtomicUsize::new(0);

/// Serializes any test that observes / mutates the global internal-write
/// registry (`INTERNAL_WRITES`). Lives at module scope (not inside `mod tests`)
/// so behavioral tests in sibling modules (`credentials`, `services::theme_loader`)
/// can take the same lock and avoid racing each other's registry assertions
/// under parallel `cargo test`. `parking_lot::Mutex` is used so a test panic
/// doesn't poison the lock and cascade-fail the group — same precedent as
/// `src/widgets/boat_tests.rs::THEME_MUTATION_LOCK`.
#[cfg(test)]
pub(crate) static INTERNAL_WRITE_TEST_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

/// Atomically write `content` to `path`, suppressing the config-watcher's
/// feedback event. Uses a per-write counter-suffixed temp name to avoid
/// collisions under concurrent writers.
///
/// All production writes to `~/.config/nokkvi/` must route through this
/// helper so the internal-write registry is reliably recorded — bypassing it
/// re-introduces the spurious-reload class the registry exists to close.
pub fn write_atomic(path: &Path, content: &str) -> Result<()> {
    let id = TEMP_WRITE_ID.fetch_add(1, Ordering::Relaxed);
    let temp_name = format!(
        "{}.{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy(),
        id
    );
    let temp_path = path.with_file_name(temp_name);

    std::fs::write(&temp_path, content)
        .with_context(|| format!("Failed to write temp file: {}", temp_path.display()))?;

    std::fs::rename(&temp_path, path)
        .with_context(|| format!("Failed to rename temp file to: {}", path.display()))?;

    // Record AFTER the rename lands, using the exact content written, so the
    // watcher can identity-match its own write and suppress the spurious reload.
    record_internal_write(path, content);

    tracing::debug!(" [ATOMIC WRITE] Atomic write to {}", path.display());
    Ok(())
}

/// Get the configuration directory (`~/.config/nokkvi`).
///
/// Holds user-editable configuration: `config.toml`, `themes/`, `sfx/`.
pub(crate) fn get_app_dir() -> Result<PathBuf> {
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
pub(crate) fn get_state_dir() -> Result<PathBuf> {
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

/// Get the credentials config file path (`~/.config/nokkvi/config.toml`,
/// or `config.debug.toml` in debug builds — see `CONFIG_FILENAME`).
pub fn get_config_path() -> Result<PathBuf> {
    Ok(get_app_dir()?.join(CONFIG_FILENAME))
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

    // ══════════════════════════════════════════════════════════════════
    //  write_atomic — the consolidated config-write helper
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn test_write_atomic_happy_path() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        write_atomic(&path, "key = \"value\"\n").unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "key = \"value\"\n");

        // No leftover *.tmp files in the directory
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s == "tmp")
            })
            .collect();
        assert!(
            leftovers.is_empty(),
            "expected no leftover .tmp files, got: {leftovers:?}"
        );
    }

    #[test]
    fn test_write_atomic_records_internal_write() {
        let _guard = super::INTERNAL_WRITE_TEST_LOCK.lock();

        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");

        let content = "k = 1\n";
        write_atomic(&path, content).unwrap();

        // After the atomic write, the registry must identity-match the exact
        // bytes at the exact (canonicalized) path so the watcher suppresses
        // its own write instead of looping a spurious reload.
        assert!(
            was_internal_write(&path, hash_config_bytes(content.as_bytes())),
            "write_atomic must record the (path, content-hash) so was_internal_write matches"
        );
    }

    // ══════════════════════════════════════════════════════════════════
    //  Internal-write suppression registry (N11 + N12)
    // ══════════════════════════════════════════════════════════════════

    #[test]
    fn was_internal_write_true_only_for_matching_path_and_hash() {
        let _guard = super::INTERNAL_WRITE_TEST_LOCK.lock();

        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let content = "theme = \"everforest\"\n";
        // Create the file so canonicalize() resolves the same on record + check.
        std::fs::write(&path, content).unwrap();

        record_internal_write(&path, content);

        // Exact self-write: matching path + matching content hash.
        assert!(
            was_internal_write(&path, hash_config_bytes(content.as_bytes())),
            "exact (path, content-hash) self-write must be recognized"
        );
    }

    #[test]
    fn external_edit_to_unwritten_path_is_not_suppressed() {
        let _guard = super::INTERNAL_WRITE_TEST_LOCK.lock();

        let dir = tempfile::TempDir::new().unwrap();
        let path_a = dir.path().join("config.toml");
        let path_b = dir.path().join("themes").join("custom.toml");
        std::fs::create_dir_all(path_b.parent().unwrap()).unwrap();
        let content_a = "a = 1\n";
        std::fs::write(&path_a, content_a).unwrap();
        std::fs::write(&path_b, "b = 1\n").unwrap();

        record_internal_write(&path_a, content_a);

        let hash_a = hash_config_bytes(content_a.as_bytes());
        let hash_other = hash_config_bytes(b"a = 2\n");
        let hash_b = hash_config_bytes(b"b = 1\n");

        // A different path is never a self-write of path_a — even inside the
        // 500ms window of path_a's internal write (the N12 lost-update fix).
        assert!(
            !was_internal_write(&path_b, hash_b),
            "an external edit to a DIFFERENT path must not be suppressed"
        );
        // The SAME path with DIFFERENT content (external re-edit of the file we
        // just wrote) must not be suppressed either.
        assert!(
            !was_internal_write(&path_a, hash_other),
            "an external edit to the same path with different content must not be suppressed"
        );
        // Sanity: the exact self-write still matches.
        assert!(
            was_internal_write(&path_a, hash_a),
            "the exact self-write must still be recognized"
        );
    }

    #[test]
    fn suppression_window_uses_saturating_math() {
        let _guard = super::INTERNAL_WRITE_TEST_LOCK.lock();

        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let content = "k = 1\n";
        std::fs::write(&path, content).unwrap();

        // Record an internal write, then immediately check. The registry's
        // recency uses Instant::elapsed (saturating), so no wall-clock skew —
        // forward OR backward — can underflow the comparison and leak the
        // self-write through as a spurious reload. This is the N11 fail-closed
        // guarantee expressed structurally: a fresh record always suppresses.
        record_internal_write(&path, content);
        assert!(
            was_internal_write(&path, hash_config_bytes(content.as_bytes())),
            "a freshly recorded internal write must always be suppressed; \
             Instant::elapsed cannot underflow on a backward OS clock step"
        );
    }

    #[test]
    fn test_write_atomic_no_temp_collision() {
        // 20 threads write concurrently to the same path. The counter-suffixed
        // temp name must prevent collisions; the final file must hold one of
        // the writes' content; no temp files may be left behind.
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");

        let mut handles = vec![];
        for i in 0..20 {
            let p = path.clone();
            handles.push(std::thread::spawn(move || {
                let content = format!("thread_val = {i}\n");
                let _ = write_atomic(&p, &content);
            }));
        }

        for h in handles {
            let _ = h.join();
        }

        let content = std::fs::read_to_string(&path).expect("final file must exist");
        assert!(
            content.starts_with("thread_val = "),
            "file must hold one writer's content, got: {content:?}"
        );

        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s == "tmp")
            })
            .collect();
        assert!(
            leftovers.is_empty(),
            "expected no leftover .tmp files after concurrent writes, got: {leftovers:?}"
        );
    }

    #[test]
    fn test_write_atomic_overwrites_existing() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");

        write_atomic(&path, "first = true\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "first = true\n",
            "first write must land"
        );

        write_atomic(&path, "second = true\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "second = true\n",
            "second write must replace first via rename"
        );
    }
}
