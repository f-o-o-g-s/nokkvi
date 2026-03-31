//! Visualizer configuration with hot-reload support
//!
//! Loads visualizer settings from config.toml and watches for changes.
//! Settings are applied in real-time without restarting the application.

use std::{
    path::PathBuf,
    sync::{Arc, mpsc},
    time::Duration,
};

use anyhow::{Context, Result};
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

/// Bars mode configuration with nested dark/light color settings
/// Maps to [visualizer.bars] in config.toml
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct BarsConfig {
    /// Minimum bar width for small windows (used in dynamic scaling).
    /// When window is at 400px or smaller, bars will be this width.
    /// Default: 2.0 (thin bars for small windows)
    pub bar_width_min: f32,

    /// Maximum bar width for large windows (used in dynamic scaling).
    /// When window is at 2560px or larger, bars will be this width.
    /// Default: 14.0 (thick bars for large windows)
    pub bar_width_max: f32,

    /// Spacing between bars in pixels.
    /// Default: 2.0
    pub bar_spacing: f32,

    /// Border width around each bar in pixels.
    /// In LED mode, this also controls the gap between segments.
    /// Default: 1.0
    pub border_width: f32,

    /// Enable LED-style segmented bars (like VU meters).
    /// When enabled, bars are rendered as stacked LED segments with gaps.
    /// Default: true
    pub led_bars: bool,

    /// Height of each LED segment in pixels.
    /// Only used when led_bars is true.
    /// Default: 2.0
    pub led_segment_height: f32,

    /// Bar gradient color mode:
    /// - "static": Static height-based gradient (bottom to top)
    /// - "wave": Gradient stretching (taller bars show more bottom colors, works great with monstercat)
    /// - "shimmer": Bars cycle through all gradient colors as flat per-bar colors with music-driven animation
    /// - "energy": Energy-scaled gradient offset (gradient shifts dramatically based on overall loudness)
    /// - "alternate": Bars alternate between first two gradient colors with music-driven 2-color oscillation
    ///   Default: "wave"
    pub gradient_mode: String,

    /// Gradient orientation — controls which axis the gradient colors are mapped along:
    /// - "vertical": Colors map bottom-to-top within each bar (default)
    /// - "horizontal": Colors map left-to-right across bars (bass → treble rainbow)
    ///   Works with all gradient modes except alternate (static, wave, shimmer, energy).
    ///   Default: "vertical"
    #[serde(default)]
    pub gradient_orientation: String,

    /// Peak gradient color mode:
    /// - "static": Uses first color in peak_gradient_colors only
    /// - "cycle": Time-based animation cycling through all peak colors
    /// - "height": Color based on peak height (taller peaks show higher gradient colors)
    /// - "match": Uses same color as bar gradient at that height position
    ///   Default: "cycle"
    pub peak_gradient_mode: String,

    /// Peak behavior mode (inspired by audioMotion-analyzer):
    /// - "none": Peak bars disabled
    /// - "fade": Hold, then fade out in place (opacity decreases)
    /// - "fall": Hold, then fall at constant speed
    /// - "fall_accel": Hold, then fall with gravity acceleration
    /// - "fall_fade": Hold, then fall at constant speed while simultaneously fading out
    ///   Default: "fall"
    pub peak_mode: String,

    /// Time in milliseconds for peaks to hold before falling/fading
    /// Default: 500
    pub peak_hold_time: u32,

    /// Time in milliseconds for peaks to completely fade out (only for "fade" mode)
    /// Default: 750
    pub peak_fade_time: u32,

    /// Peak bar height as percentage of bar_width (non-LED mode only).
    /// In LED mode, peak height always equals led_segment_height.
    /// Default: 66 (66%), range 10-100
    pub peak_height_ratio: u32,

    /// Peak fall speed (1-20). Controls how fast peaks drop in fall/fall_accel modes.
    /// Scales the base velocity: 1 = very slow, 5 = default, 20 = very fast.
    /// No effect in fade mode (use peak_fade_time instead).
    /// Default: 5
    pub peak_fall_speed: u32,

    /// Isometric 3D depth in pixels.
    /// When > 0, bars are rendered with a top face and right side face for a 3D look.
    /// Default: 0.0 (flat / disabled)
    pub bar_depth_3d: f32,

    /// Maximum number of bars to display.
    /// The dynamic layout algorithm will try to fit up to this many bars in the window.
    /// Default: 256, range 16–2048
    pub max_bars: usize,
}

impl Default for BarsConfig {
    fn default() -> Self {
        Self {
            bar_width_min: 4.0, // Bar width at small windows
            bar_width_max: 4.0, // Bar width at large windows (uniform)
            bar_spacing: 1.0,
            border_width: 2.0,
            led_bars: false,
            led_segment_height: 2.0,
            gradient_mode: "wave".to_string(),
            gradient_orientation: "vertical".to_string(),
            peak_gradient_mode: "static".to_string(),
            peak_mode: "fade".to_string(), // Default: fade out in place
            peak_hold_time: 1000,          // 1000ms hold before fading
            peak_fade_time: 750,           // 750ms fade duration
            peak_height_ratio: 50,         // 50% of bar_width
            peak_fall_speed: 5,            // Medium speed (1=slow, 10=fast)
            bar_depth_3d: 0.0,             // Flat by default (no 3D effect)
            max_bars: 2048,                // Maximum bars to try fitting
        }
    }
}

impl BarsConfig {
    /// Get the gradient mode as u32 for shader (0=static, 2=wave, 3=shimmer, 4=energy, 5=alternate)
    pub fn get_gradient_mode_value(&self) -> u32 {
        match self.gradient_mode.to_lowercase().as_str() {
            "static" => 0,
            "wave" => 2,
            "shimmer" => 3,
            "energy" => 4,
            "alternate" => 5,
            _ => 3, // Default to shimmer mode
        }
    }

    /// Get the gradient orientation as u32 for shader (0=vertical, 1=horizontal)
    pub fn get_gradient_orientation_value(&self) -> u32 {
        match self.gradient_orientation.to_lowercase().as_str() {
            "horizontal" => 1,
            _ => 0, // Default to vertical
        }
    }

    /// Get the peak gradient mode as u32 for shader (0=static, 1=cycle, 2=height, 3=match)
    pub fn get_peak_gradient_mode_value(&self) -> u32 {
        match self.peak_gradient_mode.to_lowercase().as_str() {
            "static" => 0,
            "cycle" => 1,
            "height" => 2,
            "match" => 3,
            _ => 1, // Default to cycle mode
        }
    }

    /// Get the peak behavior mode as u32 for shader (0=none, 1=fade, 2=fall, 3=fall_accel, 4=fall_fade)
    pub fn get_peak_mode_value(&self) -> u32 {
        match self.peak_mode.to_lowercase().as_str() {
            "none" => 0,
            "fade" => 1,
            "fall" => 2,
            "fall_accel" => 3,
            "fall_fade" => 4,
            _ => 2, // Default to fall mode
        }
    }
}

/// Lines mode specific configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct LinesConfig {
    /// Number of points to render (default: 24)
    pub point_count: usize,
    /// Line thickness as fraction of visualizer height (0.01-0.10, default: 0.05 = 5%)
    pub line_thickness: f32,
    /// Outline thickness in pixels (0.0 = disabled, up to 5.0).
    /// The outline is a darker border drawn behind the main line.
    /// Default: 2.0
    pub outline_thickness: f32,
    /// Outline opacity (0.0 = invisible, 1.0 = fully opaque).
    /// Default: 1.0
    pub outline_opacity: f32,
    /// Color animation cycle speed (0.05 = very slow, 1.0 = very fast).
    /// Controls how quickly the line color cycles through the gradient palette.
    /// Default: 0.25
    pub animation_speed: f32,
    /// Gradient color mode:
    /// - "breathing": Time-based cycling through all gradient colors (default)
    /// - "static": Uses first gradient color only (no animation)
    /// - "position": Color based on horizontal position (bass=left → treble=right)
    /// - "height": Color based on amplitude (quiet=bottom colors, loud=top colors)
    ///   Default: "breathing"
    pub gradient_mode: String,
    /// Fill opacity under the curve (0.0 = disabled, 1.0 = fully opaque).
    /// Default: 0.0
    pub fill_opacity: f32,
    /// Mirror mode: render waveform symmetrically from center.
    /// Default: false
    pub mirror: bool,
    /// Interpolation style:
    /// - "smooth": Catmull-Rom spline (default)
    /// - "angular": Straight line segments between points
    ///   Default: "smooth"
    pub style: String,
}

impl Default for LinesConfig {
    fn default() -> Self {
        Self {
            point_count: 24,
            line_thickness: 0.05,
            outline_thickness: 2.0,
            outline_opacity: 1.0,
            animation_speed: 0.25,
            gradient_mode: "breathing".to_string(),
            fill_opacity: 0.0,
            mirror: false,
            style: "smooth".to_string(),
        }
    }
}

impl LinesConfig {
    /// Get the gradient mode as u32 for shader (0=breathing, 1=static, 2=position, 3=height, 4=gradient)
    pub fn get_gradient_mode_value(&self) -> u32 {
        match self.gradient_mode.to_lowercase().as_str() {
            "breathing" => 0,
            "static" => 1,
            "position" => 2,
            "height" => 3,
            "gradient" => 4,
            _ => 0, // Default to breathing
        }
    }

    /// Get the style as u32 for shader (0=smooth, 1=angular)
    pub fn get_style_value(&self) -> u32 {
        match self.style.to_lowercase().as_str() {
            "smooth" => 0,
            "angular" => 1,
            _ => 0,
        }
    }
}

/// Minimum effective monstercat value.
/// Below this, `monstercat * 1.5 < 1.0` so the exponential base inverts the filter
/// (amplifies neighbors instead of attenuating). Values in `(0.0, MIN)` are snapped to 0.0.
pub(crate) const MONSTERCAT_MIN_EFFECTIVE: f64 = 0.7;

/// Visualizer configuration loaded from config.toml
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct VisualizerConfig {
    /// Auto-sensitivity: dynamically adjusts output to span the full [0, 1] range.
    /// When disabled, raw FFT magnitudes are used (volume-dependent).
    /// Default: true
    #[serde(default = "default_auto_sensitivity")]
    pub auto_sensitivity: bool,

    /// Noise reduction (0.0-1.0, higher = smoother)
    pub noise_reduction: f64,

    /// Waves smoothing (true = enabled, false = disabled).
    /// Smooth Catmull-Rom spline interpolation between bars.
    /// Mutually exclusive with monstercat.
    /// Default: true
    pub waves: bool,

    /// Waves smoothing intensity (2-16).
    /// Controls subsampling step for Catmull-Rom spline control points.
    /// Higher values = smoother (fewer control points, more interpolation).
    /// Default: 4
    pub waves_smoothing: u32,

    /// Monstercat smoothing (0.0 = disabled, >= MONSTERCAT_MIN_EFFECTIVE = enabled).
    /// Creates exponential falloff spreading effect.
    /// Values below the minimum are snapped to 0.0 (off) during validation,
    /// because the math (`pow(monstercat * 1.5, distance)`) requires a base >= 1.0
    /// to attenuate neighbors — below that threshold it amplifies instead.
    /// Mutually exclusive with waves.
    /// Default: 0.0
    pub monstercat: f64,

    /// Lower cutoff frequency in Hz (bass floor).
    /// Frequencies below this are not visualized.
    /// Default: 50 Hz
    pub lower_cutoff_freq: u32,

    /// Higher cutoff frequency in Hz (treble ceiling).
    /// Frequencies above this are not visualized.
    /// Should not exceed sample_rate / 2 (Nyquist limit).
    /// Default: 10000 Hz
    pub higher_cutoff_freq: u32,

    /// Visualizer height as percentage of window height (0.1-1.0).
    /// Default: 0.30 (30%)
    pub height_percent: f32,

    /// Overall visualizer opacity (0.0 = invisible, 1.0 = fully opaque).
    /// Default: 1.0
    pub opacity: f32,

    /// Bars mode specific settings
    /// Use [visualizer.bars] in config.toml
    #[serde(default)]
    pub bars: BarsConfig,

    /// Lines mode specific settings
    /// Use [visualizer.lines] in config.toml
    #[serde(default)]
    pub lines: LinesConfig,
}

fn default_auto_sensitivity() -> bool {
    true
}

impl Default for VisualizerConfig {
    fn default() -> Self {
        Self {
            auto_sensitivity: default_auto_sensitivity(),
            noise_reduction: 0.77,
            waves: false,
            waves_smoothing: 4,
            monstercat: 0.0,
            lower_cutoff_freq: 50,
            higher_cutoff_freq: 10000,
            height_percent: 0.20,
            opacity: 1.0,
            bars: BarsConfig::default(),
            lines: LinesConfig::default(),
        }
    }
}

impl VisualizerConfig {
    /// Validate and clamp values to valid ranges
    pub fn validate(&mut self) {
        self.noise_reduction = self.noise_reduction.clamp(0.0, 1.0);
        // Snap sub-threshold values to off — the filter amplifies instead of
        // attenuating when `monstercat * 1.5 < 1.0`
        if self.monstercat < MONSTERCAT_MIN_EFFECTIVE {
            self.monstercat = 0.0;
        }
        self.waves_smoothing = self.waves_smoothing.clamp(2, 16);

        // Frequency cutoffs: lower must be at least 20Hz, higher must be > lower
        self.lower_cutoff_freq = self.lower_cutoff_freq.max(20);
        self.higher_cutoff_freq = self.higher_cutoff_freq.max(self.lower_cutoff_freq + 100);
        // Cap higher cutoff at 22050 Hz (Nyquist for 44100 sample rate)
        self.higher_cutoff_freq = self.higher_cutoff_freq.min(22050);

        // Validate bars config

        self.bars.bar_width_min = self.bars.bar_width_min.clamp(1.0, 10.0);
        self.bars.bar_width_max = self.bars.bar_width_max.clamp(self.bars.bar_width_min, 20.0);
        self.bars.bar_spacing = self.bars.bar_spacing.max(0.0);
        self.bars.border_width = self.bars.border_width.clamp(0.0, 5.0);
        self.bars.led_segment_height = self.bars.led_segment_height.clamp(2.0, 20.0);
        self.bars.bar_depth_3d = self.bars.bar_depth_3d.clamp(0.0, 20.0);
        self.bars.peak_height_ratio = self.bars.peak_height_ratio.clamp(10, 100);
        self.bars.peak_fall_speed = self.bars.peak_fall_speed.clamp(1, 20);
        self.bars.max_bars = self.bars.max_bars.clamp(16, 2048);

        // Validate lines config
        self.lines.point_count = self.lines.point_count.clamp(8, 512);
        self.lines.line_thickness = self.lines.line_thickness.clamp(0.01, 0.1);
        self.lines.outline_thickness = self.lines.outline_thickness.clamp(0.0, 5.0);
        self.lines.outline_opacity = self.lines.outline_opacity.clamp(0.0, 1.0);
        self.lines.animation_speed = self.lines.animation_speed.clamp(0.05, 1.0);
        self.lines.fill_opacity = self.lines.fill_opacity.clamp(0.0, 1.0);

        // Validate height_percent (10% to 60% — above 60% the visualizer overlaps the player bar)
        self.height_percent = self.height_percent.clamp(0.1, 0.60);

        // Validate opacity (0.0–1.0)
        self.opacity = self.opacity.clamp(0.0, 1.0);
    }
}

/// Full config file structure (includes credentials + visualizer sections)
#[derive(Debug, Deserialize, Serialize, Default)]
struct ConfigFile {
    #[serde(default)]
    visualizer: VisualizerConfig,
    // Other sections are ignored (credentials handled separately)
}

/// Shared config state for thread-safe access
pub(crate) type SharedVisualizerConfig = Arc<RwLock<VisualizerConfig>>;

/// Load visualizer config from config.toml
pub(crate) fn load_visualizer_config() -> Result<VisualizerConfig> {
    let config_path = nokkvi_data::utils::paths::get_config_path()?;

    if !config_path.exists() {
        debug!(" No config.toml found, using default visualizer settings");
        return Ok(VisualizerConfig::default());
    }

    let content = std::fs::read_to_string(&config_path).context("Failed to read config.toml")?;

    let config_file: ConfigFile = toml::from_str(&content).unwrap_or_else(|e| {
        warn!("  Failed to parse visualizer config: {}, using defaults", e);
        ConfigFile::default()
    });

    let mut viz_config = config_file.visualizer;
    viz_config.validate();

    debug!(
        " Loaded visualizer config: noise_reduction={:.2}, waves={}, freq={}-{}Hz",
        viz_config.noise_reduction,
        viz_config.waves,
        viz_config.lower_cutoff_freq,
        viz_config.higher_cutoff_freq
    );
    debug!(
        " Bars: spacing={:.1}, border={:.1}, led_bars={}, segment_height={:.1}",
        viz_config.bars.bar_spacing,
        viz_config.bars.border_width,
        viz_config.bars.led_bars,
        viz_config.bars.led_segment_height
    );
    debug!(
        " Lines: points={}, thickness={:.3}, outline={:.1}, anim_speed={:.2}, gradient={}",
        viz_config.lines.point_count,
        viz_config.lines.line_thickness,
        viz_config.lines.outline_thickness,
        viz_config.lines.animation_speed,
        viz_config.lines.gradient_mode
    );

    Ok(viz_config)
}

/// Create shared config state
pub(crate) fn create_shared_config() -> SharedVisualizerConfig {
    let config = load_visualizer_config().unwrap_or_default();
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

    /// Check for config changes (non-blocking)
    /// Returns Some(new_config) if config file was modified
    pub(crate) fn poll_changes(&self) -> Option<VisualizerConfig> {
        use notify::EventKind;

        // Drain all pending events
        let mut should_reload = false;

        while let Ok(event_result) = self.receiver.try_recv() {
            if let Ok(event) = event_result {
                // Only react to actual file modifications, not access or metadata changes
                let is_modification =
                    matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_));

                if is_modification {
                    for path in &event.paths {
                        // config.toml changed
                        if *path == self.config_path {
                            should_reload = true;
                        }
                        // a .toml file inside themes/ changed
                        if let Some(ref themes_dir) = self.themes_dir
                            && path.starts_with(themes_dir)
                            && path.extension().is_some_and(|e| e == "toml")
                        {
                            should_reload = true;
                        }
                    }
                }
            }
        }

        if should_reload {
            // Check if this reload was triggered by our own internal write
            let last_write = nokkvi_data::utils::paths::LAST_INTERNAL_WRITE
                .load(std::sync::atomic::Ordering::Acquire);
            if let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                && now.as_millis() as u64 - last_write < 500
            {
                debug!(" Ignoring config file change triggered by internal write");
                return None;
            }

            match load_visualizer_config() {
                Ok(config) => {
                    debug!(" Hot-reloaded visualizer config");
                    Some(config)
                }
                Err(e) => {
                    warn!("  Failed to reload config: {}", e);
                    None
                }
            }
        } else {
            None
        }
    }
}

/// Create a subscription stream for Iced that polls config changes
pub(crate) fn config_watcher_subscription() -> impl futures::Stream<Item = Option<VisualizerConfig>>
{
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
