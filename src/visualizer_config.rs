//! Visualizer configuration — UI-crate residue with hot-reload support.
//!
//! The pure config types (`VisualizerConfig`, `BarsConfig`, `LinesConfig`,
//! `ScopeConfig`, the 7 `wire_enum!` mode enums, `validate()`, the `keys`
//! module, `MONSTERCAT_MIN_EFFECTIVE`) live in the iced-free data crate at
//! `nokkvi_data::types::visualizer_config` (M3) and are re-exported below so
//! every existing `crate::visualizer_config::X` path keeps resolving.
//!
//! What stays here is the iced-/runtime-coupled residue: `ThemeBarColors`
//! (iced::Color), the disk loader, the `SharedVisualizerConfig` lock wrapper,
//! and the `ConfigWatcher` that hot-reloads config.toml + theme file changes.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    time::Duration,
};

use anyhow::Result;
pub(crate) use nokkvi_data::types::visualizer_config::*;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

/// Theme-specific bar color configuration (colors only)
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct ThemeBarColors {
    /// Border color for bars as a hex string.
    /// Example: "#1d2021" (Gruvbox BG0_HARD dark)
    /// Default: "#1d2021"
    pub border_color: String,

    /// Border opacity in LED mode (0.0 = transparent/hidden, 1.0 = fully opaque).
    /// Only applies when led_bars is true.
    /// Default: 1.0 (dark), 0.0 (light)
    #[serde(default = "default_border_opacity")]
    pub led_border_opacity: f32,

    /// Border opacity for regular (non-LED) bars (0.0 = transparent/hidden, 1.0 = fully opaque).
    /// Only applies when led_bars is false.
    /// Default: 1.0 (dark), 0.0 (light)
    #[serde(default = "default_border_opacity")]
    pub border_opacity: f32,

    /// Gradient colors for the bars (bottom to top), 6 hex color strings.
    /// Example: ["#458588", "#83a598", "#689d6a", "#8ec07c", "#8ec07c", "#8ec07c"]
    /// Default: Blue to aqua gradient
    pub bar_gradient_colors: Vec<String>,

    /// Gradient colors for peak breathing animation, 6 hex color strings.
    /// These colors cycle over time for the breathing effect.
    /// Example: ["#fe8019", "#fabd2f", "#fb4934", "#fe8019", "#fabd2f", "#fb4934"]
    /// Default: Warm colors (orange, yellow, red)
    pub peak_gradient_colors: Vec<String>,
}

/// Default border opacity for dark mode (used by serde)
fn default_border_opacity() -> f32 {
    1.0
}

impl Default for ThemeBarColors {
    fn default() -> Self {
        Self::from(nokkvi_data::types::theme_file::VisualizerColors::default())
    }
}

impl From<nokkvi_data::types::theme_file::VisualizerColors> for ThemeBarColors {
    fn from(v: nokkvi_data::types::theme_file::VisualizerColors) -> Self {
        Self {
            border_color: v.border_color,
            led_border_opacity: v.led_border_opacity,
            border_opacity: v.border_opacity,
            bar_gradient_colors: v.bar_gradient_colors,
            peak_gradient_colors: v.peak_gradient_colors,
        }
    }
}

impl ThemeBarColors {
    /// Parse a hex color string via the canonical implementation in theme_config
    fn parse_hex_color(hex: &str) -> Option<iced::Color> {
        crate::theme_config::parse_hex_color(hex)
    }

    /// Get bar gradient colors as iced::Color (padded to 8 colors for shader)
    pub(crate) fn get_bar_gradient_colors(&self) -> Vec<iced::Color> {
        let mut colors: Vec<iced::Color> = self
            .bar_gradient_colors
            .iter()
            .filter_map(|hex| Self::parse_hex_color(hex))
            .collect();

        // Pad to exactly 8 colors (shader requirement)
        while colors.len() < 8 {
            colors.push(
                colors
                    .last()
                    .copied()
                    .unwrap_or(iced::Color::from_rgb(0.27, 0.52, 0.53)),
            ); // fallback blue
        }
        colors.truncate(8);
        colors
    }

    /// Get peak gradient colors as iced::Color (padded to 8 colors for shader)
    pub(crate) fn get_peak_gradient_colors(&self) -> Vec<iced::Color> {
        let mut colors: Vec<iced::Color> = self
            .peak_gradient_colors
            .iter()
            .filter_map(|hex| Self::parse_hex_color(hex))
            .collect();

        // Pad to exactly 8 colors (shader requirement)
        while colors.len() < 8 {
            colors.push(
                colors
                    .last()
                    .copied()
                    .unwrap_or(iced::Color::from_rgb(0.98, 0.50, 0.10)),
            ); // fallback orange
        }
        colors.truncate(8);
        colors
    }

    /// Get border color as iced::Color
    pub(crate) fn get_border_color(&self) -> iced::Color {
        Self::parse_hex_color(&self.border_color).unwrap_or(iced::Color::from_rgb(0.11, 0.13, 0.13))
    }
}

/// Shared config state for thread-safe access
pub(crate) type SharedVisualizerConfig = Arc<RwLock<VisualizerConfig>>;

/// Tiny extension trait that fronts the two patterns every call site of
/// `SharedVisualizerConfig` was open-coding: full-swap on hot-reload /
/// settings dispatch (`apply`) and read-clone for view-data assembly
/// (`snapshot`).
///
/// Both methods are intentionally one-liners that hold the read/write
/// lock for the absolute minimum window — the snapshot pump that feeds
/// shader parameters depends on writers never holding the lock across
/// any closure or async point.
pub(crate) trait SharedVisualizerConfigExt {
    /// Replace the inner config under a single write-lock acquisition.
    fn apply(&self, new: VisualizerConfig);
    /// Clone the current config out from under a single read-lock acquisition.
    fn snapshot(&self) -> VisualizerConfig;
}

impl SharedVisualizerConfigExt for SharedVisualizerConfig {
    fn apply(&self, new: VisualizerConfig) {
        *self.write() = new;
    }

    fn snapshot(&self) -> VisualizerConfig {
        self.read().clone()
    }
}

/// Create shared config state, seeded from the config.toml `[visualizer]`
/// section (validated by the data-crate reader; missing file/section or a
/// parse error falls back to defaults). After the first
/// `PlayerSettingsLoaded`, the SettingsManager's in-memory copy is the
/// authority and keeps this in lockstep via the unified settings path.
pub(crate) fn create_shared_config() -> SharedVisualizerConfig {
    let config = nokkvi_data::services::toml_settings_io::read_toml_visualizer()
        .unwrap_or_else(|e| {
            warn!("  Failed to read [visualizer] from config.toml: {e}");
            None
        })
        .unwrap_or_default();
    Arc::new(RwLock::new(config))
}

/// File watcher for hot-reloading config.toml AND theme file changes
pub(crate) struct ConfigWatcher {
    receiver: mpsc::Receiver<Result<Event, notify::Error>>,
    _watcher: RecommendedWatcher,
    config_path: PathBuf,
    /// Themes directory — changes here also trigger ThemeConfigReloaded
    themes_dir: Option<PathBuf>,
}

impl ConfigWatcher {
    /// Create a new config watcher that monitors both config.toml and themes/
    pub(crate) fn new() -> Result<Self> {
        let config_path = nokkvi_data::utils::paths::get_config_path()?;
        // Canonicalize the path so it matches what inotify reports
        // (inotify resolves symlinks, so we need the real path for comparison)
        let config_path = config_path.canonicalize().unwrap_or(config_path);
        let (tx, rx) = mpsc::channel();

        // Create watcher with debounce
        let mut watcher = notify::recommended_watcher(move |res| {
            let _ = tx.send(res);
        })?;

        // Watch the config directory (not the file directly, for atomic saves)
        if let Some(parent) = config_path.parent() {
            watcher.watch(parent, RecursiveMode::NonRecursive)?;
        }

        // Also watch the themes directory for hot-reload on theme file edits
        let themes_dir = nokkvi_data::utils::paths::get_themes_dir()
            .ok()
            .and_then(|dir| {
                if dir.exists() {
                    watcher
                        .watch(&dir, RecursiveMode::NonRecursive)
                        .map(|()| {
                            debug!(" Watching themes dir: {}", dir.display());
                            dir
                        })
                        .ok()
                } else {
                    None
                }
            });

        Ok(Self {
            receiver: rx,
            _watcher: watcher,
            config_path,
            themes_dir,
        })
    }

    /// Whether a config-watcher event for `path` is one we care about
    /// hot-reloading: the watched `config.toml`, or a `.toml` file inside the
    /// themes directory.
    fn is_relevant_path(&self, path: &Path) -> bool {
        if *path == self.config_path {
            return true;
        }
        if let Some(ref themes_dir) = self.themes_dir {
            return path.starts_with(themes_dir) && path.extension().is_some_and(|e| e == "toml");
        }
        false
    }

    /// Check for config changes (non-blocking).
    /// Returns `Some(())` when a relevant file was externally modified — the
    /// caller fans out to the unified reload messages; nothing is loaded
    /// here (the SettingsManager re-reads every section itself).
    pub(crate) fn poll_changes(&self) -> Option<()> {
        use notify::EventKind;

        // Drain all pending events into the SET of changed-and-relevant paths.
        // Per-path identity (rather than a single coarse should_reload bool)
        // lets `decide_reload` distinguish a genuine external edit from the
        // app's own write even when they land in the same 100ms poll.
        let mut changed: Vec<PathBuf> = Vec::new();

        while let Ok(event_result) = self.receiver.try_recv() {
            if let Ok(event) = event_result {
                // Only react to actual file modifications, not access or metadata changes
                let is_modification =
                    matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_));

                if is_modification {
                    for path in &event.paths {
                        if self.is_relevant_path(path) && !changed.contains(path) {
                            changed.push(path.clone());
                        }
                    }
                }
            }
        }

        if changed.is_empty() {
            return None;
        }

        if !decide_reload(&changed) {
            debug!(" Ignoring config file change(s) triggered by internal write");
            return None;
        }

        debug!(" External config change detected — triggering unified reload");
        Some(())
    }
}

/// Pure suppression decision over the set of changed-and-relevant paths.
///
/// Returns `true` (reload) unless EVERY changed path is an exact self-write —
/// i.e. its CURRENT on-disk bytes hash to a `(path, content-hash)` the app
/// recorded inside the monotonic suppression window. A single external edit
/// (different path, or different content at the same path) forces a reload,
/// closing the lost-update bug where a genuine user edit landing in the time
/// shadow of an unrelated internal write was silently dropped.
///
/// Reading a changed file back to hash it can fail (deleted / locked mid-event);
/// on `Err` the path is treated as NOT a self-write so the reload runs — the
/// safe direction for a lost-update bug is to surface the user's edit.
fn decide_reload(changed: &[PathBuf]) -> bool {
    use nokkvi_data::utils::paths::{hash_config_bytes, was_internal_write};

    changed.iter().any(|path| match std::fs::read(path) {
        Ok(bytes) => !was_internal_write(path, hash_config_bytes(&bytes)),
        Err(_) => true,
    })
}

/// Create a subscription stream for Iced that polls config changes
pub(crate) fn config_watcher_subscription() -> impl futures::Stream<Item = Option<()>> {
    use std::time::Instant;

    use futures::stream;

    struct WatcherState {
        watcher: Option<ConfigWatcher>,
        last_check: Instant,
    }

    let initial_state = WatcherState {
        watcher: ConfigWatcher::new().ok(),
        last_check: Instant::now(),
    };

    stream::unfold(initial_state, |mut state| async move {
        // Check every 100ms for faster shutdown response (was 500ms)
        tokio::time::sleep(Duration::from_millis(100)).await;

        let result = if let Some(ref watcher) = state.watcher {
            watcher.poll_changes()
        } else {
            None
        };

        state.last_check = Instant::now();
        Some((result, state))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `apply` writes the new config under the write lock and `snapshot` reads
    /// a fresh clone under the read lock. Round-trips a non-default config to
    /// confirm the helper pair is a wire-equivalent replacement for the
    /// previous `*shared.write() = new` / `shared.read().clone()` inline
    /// patterns at the 4 call sites.
    #[test]
    fn shared_visualizer_config_apply_snapshot_roundtrip() {
        let shared: SharedVisualizerConfig = Arc::new(RwLock::new(VisualizerConfig::default()));

        let mut custom = VisualizerConfig::default();
        custom.noise_reduction = 0.42;
        custom.waves = !custom.waves;
        custom.waves_smoothing = 7;
        custom.bars.bar_spacing = 7.5;
        custom.lines.point_count = 256;
        let expected_waves = custom.waves;

        shared.apply(custom);

        let read_back = shared.snapshot();
        assert_eq!(read_back.noise_reduction, 0.42);
        assert_eq!(read_back.waves, expected_waves);
        assert_eq!(read_back.waves_smoothing, 7);
        assert_eq!(read_back.bars.bar_spacing, 7.5);
        assert_eq!(read_back.lines.point_count, 256);

        // `snapshot` returns an owned clone, so mutating it must not leak
        // back into the shared state — the write lock is only acquired
        // explicitly via `apply`. A second `snapshot()` therefore yields
        // the same field values that the first one observed.
        let second_snapshot = shared.snapshot();
        assert_eq!(second_snapshot.noise_reduction, 0.42);
        assert_eq!(second_snapshot.bars.bar_spacing, 7.5);
    }

    // ══════════════════════════════════════════════════════════════════
    //  Config-watcher suppression (N11 + N12)
    // ══════════════════════════════════════════════════════════════════

    /// Serializes the two `decide_reload` tests below: both record into the
    /// process-global internal-write registry in `nokkvi_data`, so running them
    /// concurrently in the UI test binary could let one's record satisfy the
    /// other's `was_internal_write` check. `parking_lot::Mutex` avoids poison
    /// cascades on a test panic.
    static SUPPRESSION_TEST_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

    /// Per-test temp dir under `$TMPDIR` (no `tempfile` dep — not in this
    /// crate's `[dev-dependencies]`). Same precedent as
    /// `services::mpris_art_writer::tests::ScratchDir`: a fresh
    /// `nokkvi-visualizer-config-test-<pid>-<counter>/` directory with a Drop
    /// guard that removes it recursively on scope exit.
    struct ScratchDir {
        path: PathBuf,
    }

    impl ScratchDir {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static SEQ: AtomicU64 = AtomicU64::new(0);
            let seq = SEQ.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "nokkvi-visualizer-config-test-{}-{}",
                std::process::id(),
                seq
            ));
            std::fs::create_dir_all(&path).expect("create scratch dir");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    /// An external edit to config.toml that lands inside the 500ms time shadow
    /// of an UNRELATED internal write must still reload (N12 lost-update fix).
    /// The old single-global-timestamp suppression dropped it; the per-path,
    /// per-content-hash registry recognizes the changed bytes as NOT a recorded
    /// self-write and `decide_reload` returns true.
    #[test]
    fn poll_changes_reloads_external_edit_inside_internal_write_window() {
        let _guard = SUPPRESSION_TEST_LOCK.lock();
        use nokkvi_data::utils::paths::{record_internal_write, write_atomic};

        let dir = ScratchDir::new();
        // An unrelated internal write opens the suppression window for path_a.
        let path_a = dir.path().join("themes").join("seed.toml");
        std::fs::create_dir_all(path_a.parent().unwrap()).unwrap();
        write_atomic(&path_a, "name = \"Seed\"\n").unwrap();

        // The user externally edits config.toml within that window. The bytes
        // on disk are NOT what the app wrote (the app never wrote this path),
        // so it must NOT be suppressed.
        let config = dir.path().join("config.toml");
        std::fs::write(&config, "theme = \"user-edit\"\n").unwrap();

        assert!(
            super::decide_reload(std::slice::from_ref(&config)),
            "external edit to config.toml inside an unrelated internal-write window must reload"
        );

        // And a true self-write to config.toml IS suppressed: record the exact
        // content the app would have written, then the watcher sees those same
        // bytes on disk.
        let self_written = "theme = \"app-written\"\n";
        std::fs::write(&config, self_written).unwrap();
        record_internal_write(&config, self_written);
        assert!(
            !super::decide_reload(std::slice::from_ref(&config)),
            "a true self-write (recorded path + matching on-disk content) must be suppressed"
        );

        // A subsequent EXTERNAL re-edit of that same file (different bytes than
        // the recorded self-write) must reload again.
        std::fs::write(&config, "theme = \"user-edit-2\"\n").unwrap();
        assert!(
            super::decide_reload(std::slice::from_ref(&config)),
            "external re-edit of the self-written file (different content) must reload"
        );
    }

    /// `decide_reload` over a mix of paths reloads when ANY changed path is not
    /// a self-write, even if another changed path in the same batch is.
    #[test]
    fn decide_reload_reloads_if_any_changed_path_is_external() {
        let _guard = SUPPRESSION_TEST_LOCK.lock();
        use nokkvi_data::utils::paths::record_internal_write;

        let dir = ScratchDir::new();
        let self_path = dir.path().join("config.toml");
        let self_content = "a = 1\n";
        std::fs::write(&self_path, self_content).unwrap();
        record_internal_write(&self_path, self_content);

        let external = dir.path().join("themes").join("custom.toml");
        std::fs::create_dir_all(external.parent().unwrap()).unwrap();
        std::fs::write(&external, "name = \"Custom\"\n").unwrap();

        assert!(
            super::decide_reload(&[self_path, external]),
            "a batch containing one external edit must reload despite a self-write also present"
        );
    }
}
