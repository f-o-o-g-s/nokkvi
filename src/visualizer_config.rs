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

/// Field-level deserializer that falls back to `T::default()` on any error
/// (empty string, unknown variant, malformed value). Preserves the
/// pre-Group-G "silent fallback on unknown" behavior for the visualizer's
/// stringly-typed-then-now-typed-enum fields, so existing user `config.toml`
/// files with empty strings or typos keep parsing instead of rejecting the
/// whole `[visualizer]` section.
fn deserialize_or_default<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: Deserialize<'de> + Default,
    D: serde::Deserializer<'de>,
{
    use serde::de::{IntoDeserializer, value::Error as ValueError};

    let raw = String::deserialize(deserializer).unwrap_or_default();
    let inner: serde::de::value::StringDeserializer<ValueError> = raw.into_deserializer();
    Ok(T::deserialize(inner).unwrap_or_default())
}

/// Bar gradient color mode.
///
/// Discriminants match the integer dispatch in `widgets/visualizer/shaders/bars.wgsl`.
/// `1` is intentionally skipped — `bars.wgsl` has no branch for it and would silently
/// fall through to the static gradient. See the
/// `bars_gradient_mode_never_emits_dead_1u` test below.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum BarsGradientMode {
    /// Height-based gradient (bottom to top).
    Static = 0,
    // value 1 intentionally skipped — dead in bars.wgsl.
    /// Gradient stretching (taller bars show more bottom colors).
    #[default]
    Wave = 2,
    /// Bars cycle through gradient colors with music-driven animation.
    Shimmer = 3,
    /// Gradient shifts based on overall loudness.
    Energy = 4,
    /// Bars alternate between first two gradient colors.
    Alternate = 5,
}

impl BarsGradientMode {
    /// Wire-format string used in `config.toml`. Must match the
    /// `#[serde(rename_all = "snake_case")]` output exactly.
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Self::Static => "static",
            Self::Wave => "wave",
            Self::Shimmer => "shimmer",
            Self::Energy => "energy",
            Self::Alternate => "alternate",
        }
    }
}

/// Gradient orientation — which axis the gradient colors map along.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum BarsGradientOrientation {
    /// Colors map bottom-to-top within each bar.
    #[default]
    Vertical = 0,
    /// Colors map left-to-right across bars (bass → treble rainbow).
    Horizontal = 1,
}

impl BarsGradientOrientation {
    /// Wire-format string used in `config.toml`.
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Self::Vertical => "vertical",
            Self::Horizontal => "horizontal",
        }
    }
}

/// Peak gradient color mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum BarsPeakGradientMode {
    /// First color in `peak_gradient_colors` only.
    Static = 0,
    /// Time-based animation cycling through all peak colors.
    #[default]
    Cycle = 1,
    /// Color based on peak height.
    Height = 2,
    /// Uses same color as bar gradient at that height position.
    Match = 3,
}

impl BarsPeakGradientMode {
    /// Wire-format string used in `config.toml`.
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Self::Static => "static",
            Self::Cycle => "cycle",
            Self::Height => "height",
            Self::Match => "match",
        }
    }
}

/// Peak behavior mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum BarsPeakMode {
    /// Peak bars disabled.
    None = 0,
    /// Hold, then fade out in place.
    Fade = 1,
    /// Hold, then fall at constant speed.
    #[default]
    Fall = 2,
    /// Hold, then fall with gravity acceleration.
    FallAccel = 3,
    /// Hold, then fall at constant speed while fading out.
    FallFade = 4,
}

impl BarsPeakMode {
    /// Wire-format string used in `config.toml`.
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Fade => "fade",
            Self::Fall => "fall",
            Self::FallAccel => "fall_accel",
            Self::FallFade => "fall_fade",
        }
    }
}

/// Lines mode gradient color mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum LinesGradientMode {
    /// Time-based cycling through all gradient colors.
    #[default]
    Breathing = 0,
    /// Uses first gradient color only (no animation).
    Static = 1,
    /// Color based on horizontal position (bass → treble).
    Position = 2,
    /// Color based on amplitude (quiet → loud).
    Height = 3,
    /// Position + amplitude blend (peaks shift palette).
    Gradient = 4,
}

impl LinesGradientMode {
    /// Wire-format string used in `config.toml`.
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Self::Breathing => "breathing",
            Self::Static => "static",
            Self::Position => "position",
            Self::Height => "height",
            Self::Gradient => "gradient",
        }
    }
}

/// Lines mode interpolation style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum LinesStyle {
    /// Catmull-Rom spline (curvy).
    #[default]
    Smooth = 0,
    /// Straight line segments between points.
    Angular = 1,
}

impl LinesStyle {
    /// Wire-format string used in `config.toml`.
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Self::Smooth => "smooth",
            Self::Angular => "angular",
        }
    }
}

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
    /// Default: 10.0
    pub bar_width_min: f32,

    /// Maximum bar width for large windows (used in dynamic scaling).
    /// When window is at 2560px or larger, bars will be this width.
    /// Default: 20.0
    pub bar_width_max: f32,

    /// Spacing between bars in pixels.
    /// Default: 0.0
    pub bar_spacing: f32,

    /// Border width around each bar in pixels.
    /// In LED mode, this also controls the gap between segments.
    /// Default: 2.0
    pub border_width: f32,

    /// Enable LED-style segmented bars (like VU meters).
    /// When enabled, bars are rendered as stacked LED segments with gaps.
    /// Default: false
    pub led_bars: bool,

    /// Height of each LED segment in pixels.
    /// Only used when led_bars is true.
    /// Default: 5.0
    pub led_segment_height: f32,

    /// Bar gradient color mode. See [`BarsGradientMode`] for variants.
    ///
    /// Default: [`BarsGradientMode::Wave`]
    #[serde(deserialize_with = "deserialize_or_default")]
    pub gradient_mode: BarsGradientMode,

    /// Gradient orientation — controls which axis the gradient colors are mapped along.
    /// Works with all gradient modes except `Alternate`.
    ///
    /// Default: [`BarsGradientOrientation::Vertical`]
    #[serde(deserialize_with = "deserialize_or_default")]
    pub gradient_orientation: BarsGradientOrientation,

    /// Peak gradient color mode. See [`BarsPeakGradientMode`] for variants.
    ///
    /// Default: [`BarsPeakGradientMode::Cycle`] (the fallback used when no value
    /// is set; the [`Default`] impl for [`BarsConfig`] overrides this to
    /// [`BarsPeakGradientMode::Static`])
    #[serde(deserialize_with = "deserialize_or_default")]
    pub peak_gradient_mode: BarsPeakGradientMode,

    /// Peak behavior mode (inspired by audioMotion-analyzer). See [`BarsPeakMode`].
    ///
    /// Default: [`BarsPeakMode::FallFade`] (overridden via the [`Default`] impl
    /// for [`BarsConfig`])
    #[serde(deserialize_with = "deserialize_or_default")]
    pub peak_mode: BarsPeakMode,

    /// Time in milliseconds for peaks to hold before falling/fading
    /// Default: 1950
    pub peak_hold_time: u32,

    /// Time in milliseconds for peaks to completely fade out (only for "fade" mode)
    /// Default: 750
    pub peak_fade_time: u32,

    /// Peak bar height as percentage of bar_width (non-LED mode only).
    /// In LED mode, peak height always equals led_segment_height.
    /// Default: 35 (35%), range 10-100
    pub peak_height_ratio: u32,

    /// Peak fall speed (1-20). Controls how fast peaks drop in fall/fall_accel modes.
    /// Scales the base velocity: 1 = very slow, 5 = default, 20 = very fast.
    /// No effect in fade mode (use peak_fade_time instead).
    /// Default: 5
    pub peak_fall_speed: u32,

    /// Isometric 3D depth in pixels.
    /// When > 0, bars are rendered with a top face and right side face for a 3D look.
    /// Default: 1.0
    pub bar_depth_3d: f32,

    /// Maximum number of bars to display.
    /// The dynamic layout algorithm will try to fit up to this many bars in the window.
    /// Default: 512, range 16–2048
    pub max_bars: usize,
}

impl Default for BarsConfig {
    fn default() -> Self {
        Self {
            bar_width_min: 10.0,
            bar_width_max: 20.0,
            bar_spacing: 0.0,
            border_width: 2.0,
            led_bars: false,
            led_segment_height: 5.0,
            gradient_mode: BarsGradientMode::Wave,
            gradient_orientation: BarsGradientOrientation::Vertical,
            peak_gradient_mode: BarsPeakGradientMode::Static,
            peak_mode: BarsPeakMode::FallFade,
            peak_hold_time: 1950,
            peak_fade_time: 750,
            peak_height_ratio: 35,
            peak_fall_speed: 5,
            bar_depth_3d: 1.0,
            max_bars: 512,
        }
    }
}

impl BarsConfig {
    /// Get the gradient mode as u32 for shader (0=static, 2=wave, 3=shimmer, 4=energy, 5=alternate).
    ///
    /// `1u` is intentionally absent from the emitted set — `bars.wgsl` does not branch on it
    /// and would silently fall through to the static gradient. The explicit discriminants on
    /// [`BarsGradientMode`] preserve this non-contiguous {0, 2, 3, 4, 5} encoding; the
    /// `bars_gradient_mode_never_emits_dead_1u` test below pins this so a future agent who
    /// adds a `1`-valued variant fails immediately.
    pub fn get_gradient_mode_value(&self) -> u32 {
        self.gradient_mode as u32
    }

    /// Get the gradient orientation as u32 for shader (0=vertical, 1=horizontal)
    pub fn get_gradient_orientation_value(&self) -> u32 {
        self.gradient_orientation as u32
    }

    /// Get the peak gradient mode as u32 for shader (0=static, 1=cycle, 2=height, 3=match)
    pub fn get_peak_gradient_mode_value(&self) -> u32 {
        self.peak_gradient_mode as u32
    }

    /// Get the peak behavior mode as u32 for shader (0=none, 1=fade, 2=fall, 3=fall_accel, 4=fall_fade)
    pub fn get_peak_mode_value(&self) -> u32 {
        self.peak_mode as u32
    }
}

/// Lines mode specific configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct LinesConfig {
    /// Number of points to render (default: 24)
    pub point_count: usize,
    /// Line thickness as fraction of visualizer height (0.01-0.10, default: 0.01 = 1%)
    pub line_thickness: f32,
    /// Outline thickness in pixels (0.0 = disabled, up to 5.0).
    /// The outline is a darker border drawn behind the main line.
    /// Default: 1.0
    pub outline_thickness: f32,
    /// Outline opacity (0.0 = invisible, 1.0 = fully opaque).
    /// Default: 1.0
    pub outline_opacity: f32,
    /// Color animation cycle speed (0.05 = very slow, 1.0 = very fast).
    /// Controls how quickly the line color cycles through the gradient palette.
    /// Default: 0.1
    pub animation_speed: f32,
    /// Gradient color mode. See [`LinesGradientMode`] for variants.
    ///
    /// Default: [`LinesGradientMode::Breathing`] (the fallback used when no value
    /// is set; the [`Default`] impl for [`LinesConfig`] overrides this to
    /// [`LinesGradientMode::Static`])
    #[serde(deserialize_with = "deserialize_or_default")]
    pub gradient_mode: LinesGradientMode,
    /// Fill opacity under the curve (0.0 = disabled, 1.0 = fully opaque).
    /// Default: 0.5
    pub fill_opacity: f32,
    /// Mirror mode: render waveform symmetrically from center.
    /// Default: false
    pub mirror: bool,
    /// Interpolation style. See [`LinesStyle`] for variants.
    ///
    /// Default: [`LinesStyle::Smooth`]
    #[serde(deserialize_with = "deserialize_or_default")]
    pub style: LinesStyle,
    /// Surfing boat: render a small boat that rides the waveform.
    /// Default: false
    pub boat: bool,
}

impl Default for LinesConfig {
    fn default() -> Self {
        Self {
            point_count: 24,
            line_thickness: 0.01,
            outline_thickness: 1.0,
            outline_opacity: 1.0,
            animation_speed: 0.1,
            gradient_mode: LinesGradientMode::Static,
            fill_opacity: 0.5,
            mirror: false,
            style: LinesStyle::Smooth,
            boat: false,
        }
    }
}

impl LinesConfig {
    /// Get the gradient mode as u32 for shader (0=breathing, 1=static, 2=position, 3=height, 4=gradient)
    pub fn get_gradient_mode_value(&self) -> u32 {
        self.gradient_mode as u32
    }

    /// Get the style as u32 for shader (0=smooth, 1=angular)
    pub fn get_style_value(&self) -> u32 {
        self.style as u32
    }
}

/// Minimum effective monstercat value.
/// Below this, `monstercat * 1.5 < 1.0` so the exponential base inverts the filter
/// (amplifies neighbors instead of attenuating). Values in `(0.0, MIN)` are snapped to 0.0.
pub(crate) const MONSTERCAT_MIN_EFFECTIVE: f64 = 0.7;

/// Typed TOML key constants for all `visualizer.*` config entries.
///
/// Use these instead of raw string literals so that typos become compile errors.
/// The `starts_with("visualizer.")` prefix check in `update/settings.rs` is
/// intentionally left as a string literal — it is structural routing logic,
/// not a specific key name.
pub(crate) mod keys {
    // ── General ─────────────────────────────────────────────────────────
    pub(crate) const NOISE_REDUCTION: &str = "visualizer.noise_reduction";
    pub(crate) const WAVES: &str = "visualizer.waves";
    pub(crate) const WAVES_SMOOTHING: &str = "visualizer.waves_smoothing";
    pub(crate) const MONSTERCAT: &str = "visualizer.monstercat";
    pub(crate) const LOWER_CUTOFF_FREQ: &str = "visualizer.lower_cutoff_freq";
    pub(crate) const HIGHER_CUTOFF_FREQ: &str = "visualizer.higher_cutoff_freq";
    pub(crate) const HEIGHT_PERCENT: &str = "visualizer.height_percent";
    pub(crate) const OPACITY: &str = "visualizer.opacity";
    pub(crate) const AUTO_SENSITIVITY: &str = "visualizer.auto_sensitivity";

    // ── Bars ─────────────────────────────────────────────────────────────
    pub(crate) const BARS_MAX_BARS: &str = "visualizer.bars.max_bars";
    pub(crate) const BARS_BAR_WIDTH_MIN: &str = "visualizer.bars.bar_width_min";
    pub(crate) const BARS_BAR_WIDTH_MAX: &str = "visualizer.bars.bar_width_max";
    pub(crate) const BARS_BAR_SPACING: &str = "visualizer.bars.bar_spacing";
    pub(crate) const BARS_BORDER_WIDTH: &str = "visualizer.bars.border_width";
    pub(crate) const BARS_LED_BARS: &str = "visualizer.bars.led_bars";
    pub(crate) const BARS_LED_SEGMENT_HEIGHT: &str = "visualizer.bars.led_segment_height";
    pub(crate) const BARS_GRADIENT_MODE: &str = "visualizer.bars.gradient_mode";
    pub(crate) const BARS_GRADIENT_ORIENTATION: &str = "visualizer.bars.gradient_orientation";
    pub(crate) const BARS_PEAK_GRADIENT_MODE: &str = "visualizer.bars.peak_gradient_mode";
    pub(crate) const BARS_PEAK_MODE: &str = "visualizer.bars.peak_mode";
    pub(crate) const BARS_PEAK_HOLD_TIME: &str = "visualizer.bars.peak_hold_time";
    pub(crate) const BARS_PEAK_FADE_TIME: &str = "visualizer.bars.peak_fade_time";
    pub(crate) const BARS_PEAK_FALL_SPEED: &str = "visualizer.bars.peak_fall_speed";
    pub(crate) const BARS_PEAK_HEIGHT_RATIO: &str = "visualizer.bars.peak_height_ratio";
    pub(crate) const BARS_BAR_DEPTH_3D: &str = "visualizer.bars.bar_depth_3d";

    // ── Lines ────────────────────────────────────────────────────────────
    pub(crate) const LINES_POINT_COUNT: &str = "visualizer.lines.point_count";
    pub(crate) const LINES_LINE_THICKNESS: &str = "visualizer.lines.line_thickness";
    pub(crate) const LINES_OUTLINE_THICKNESS: &str = "visualizer.lines.outline_thickness";
    pub(crate) const LINES_OUTLINE_OPACITY: &str = "visualizer.lines.outline_opacity";
    pub(crate) const LINES_ANIMATION_SPEED: &str = "visualizer.lines.animation_speed";
    pub(crate) const LINES_GRADIENT_MODE: &str = "visualizer.lines.gradient_mode";
    pub(crate) const LINES_FILL_OPACITY: &str = "visualizer.lines.fill_opacity";
    pub(crate) const LINES_MIRROR: &str = "visualizer.lines.mirror";
    pub(crate) const LINES_STYLE: &str = "visualizer.lines.style";
    pub(crate) const LINES_BOAT: &str = "visualizer.lines.boat";
}

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
    /// Default: 5
    pub waves_smoothing: u32,

    /// Monstercat smoothing (0.0 = disabled, >= MONSTERCAT_MIN_EFFECTIVE = enabled).
    /// Creates exponential falloff spreading effect.
    /// Values below the minimum are snapped to 0.0 (off) during validation,
    /// because the math (`pow(monstercat * 1.5, distance)`) requires a base >= 1.0
    /// to attenuate neighbors — below that threshold it amplifies instead.
    /// Mutually exclusive with waves.
    /// Default: 1.0
    pub monstercat: f64,

    /// Lower cutoff frequency in Hz (bass floor).
    /// Frequencies below this are not visualized.
    /// Default: 20 Hz
    pub lower_cutoff_freq: u32,

    /// Higher cutoff frequency in Hz (treble ceiling).
    /// Frequencies above this are not visualized.
    /// Should not exceed sample_rate / 2 (Nyquist limit).
    /// Default: 10000 Hz
    pub higher_cutoff_freq: u32,

    /// Visualizer height as percentage of window height (0.1-1.0).
    /// Default: 0.25 (25%)
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
            waves_smoothing: 5,
            monstercat: 1.0,
            lower_cutoff_freq: 20,
            higher_cutoff_freq: 10000,
            height_percent: 0.25,
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
        viz_config.lines.gradient_mode.as_wire_str()
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins the `BarsConfig::get_gradient_mode_value` emitted u32 set so a future agent
    /// who adds a `1`-valued variant fails immediately — `bars.wgsl` has no branch for
    /// `1u` and would silently fall through to the static gradient. See Tier 0 #0.10 in
    /// the 2026-05-11 audit roadmap.
    #[test]
    fn bars_gradient_mode_never_emits_dead_1u() {
        // Every defined variant (the only inputs reachable from the TOML config + UI dropdown).
        let variants = [
            BarsGradientMode::Static,
            BarsGradientMode::Wave,
            BarsGradientMode::Shimmer,
            BarsGradientMode::Energy,
            BarsGradientMode::Alternate,
        ];
        for variant in variants {
            let cfg = BarsConfig {
                gradient_mode: variant,
                ..Default::default()
            };
            let value = cfg.get_gradient_mode_value();
            assert_ne!(
                value, 1,
                "gradient_mode variant {variant:?} emits 1u, which is dead in bars.wgsl",
            );
        }

        // The default fallback (used when TOML is missing the field) must also avoid 1u.
        let cfg = BarsConfig::default();
        assert_ne!(
            cfg.get_gradient_mode_value(),
            1,
            "default fallback for gradient_mode emits dead 1u",
        );
    }

    /// Pins the exact emitted set so a renumbering that shifts a mode onto 1u is caught.
    /// This is intentionally redundant with the test above — together they cover both
    /// "no variant maps to 1" and "the full set is what bars.wgsl branches on".
    #[test]
    fn bars_gradient_mode_emits_expected_discriminants() {
        let expected: &[(BarsGradientMode, u32)] = &[
            (BarsGradientMode::Static, 0),
            (BarsGradientMode::Wave, 2),
            (BarsGradientMode::Shimmer, 3),
            (BarsGradientMode::Energy, 4),
            (BarsGradientMode::Alternate, 5),
        ];
        for (variant, want) in expected {
            let cfg = BarsConfig {
                gradient_mode: *variant,
                ..Default::default()
            };
            assert_eq!(
                cfg.get_gradient_mode_value(),
                *want,
                "gradient_mode {variant:?} should emit {want}u",
            );
        }
    }

    /// Round-trip every `BarsConfig` enum variant through TOML to verify the
    /// `#[serde(rename_all = "snake_case")]` wire format is preserved end-to-end
    /// and matches the literal strings stored in `config.toml`.
    #[test]
    fn bars_config_serde_roundtrip_byte_identity() {
        let cases: &[(BarsGradientMode, &str)] = &[
            (BarsGradientMode::Static, "static"),
            (BarsGradientMode::Wave, "wave"),
            (BarsGradientMode::Shimmer, "shimmer"),
            (BarsGradientMode::Energy, "energy"),
            (BarsGradientMode::Alternate, "alternate"),
        ];
        for (variant, expected_wire) in cases {
            assert_eq!(variant.as_wire_str(), *expected_wire);
            let cfg = BarsConfig {
                gradient_mode: *variant,
                ..Default::default()
            };
            let toml_str = toml::to_string(&cfg).expect("serialize BarsConfig");
            assert!(
                toml_str.contains(&format!("gradient_mode = \"{expected_wire}\"")),
                "BarsConfig with gradient_mode={variant:?} should emit \
                 `gradient_mode = \"{expected_wire}\"`, got:\n{toml_str}",
            );
            let parsed: BarsConfig = toml::from_str(&toml_str).expect("deserialize BarsConfig");
            assert_eq!(parsed.gradient_mode, *variant);
        }

        let orient_cases: &[(BarsGradientOrientation, &str)] = &[
            (BarsGradientOrientation::Vertical, "vertical"),
            (BarsGradientOrientation::Horizontal, "horizontal"),
        ];
        for (variant, expected_wire) in orient_cases {
            assert_eq!(variant.as_wire_str(), *expected_wire);
            let cfg = BarsConfig {
                gradient_orientation: *variant,
                ..Default::default()
            };
            let toml_str = toml::to_string(&cfg).expect("serialize BarsConfig");
            assert!(
                toml_str.contains(&format!("gradient_orientation = \"{expected_wire}\"")),
                "BarsConfig with gradient_orientation={variant:?} should emit \
                 `gradient_orientation = \"{expected_wire}\"`, got:\n{toml_str}",
            );
            let parsed: BarsConfig = toml::from_str(&toml_str).expect("deserialize BarsConfig");
            assert_eq!(parsed.gradient_orientation, *variant);
        }

        let peak_grad_cases: &[(BarsPeakGradientMode, &str)] = &[
            (BarsPeakGradientMode::Static, "static"),
            (BarsPeakGradientMode::Cycle, "cycle"),
            (BarsPeakGradientMode::Height, "height"),
            (BarsPeakGradientMode::Match, "match"),
        ];
        for (variant, expected_wire) in peak_grad_cases {
            assert_eq!(variant.as_wire_str(), *expected_wire);
            let cfg = BarsConfig {
                peak_gradient_mode: *variant,
                ..Default::default()
            };
            let toml_str = toml::to_string(&cfg).expect("serialize BarsConfig");
            assert!(
                toml_str.contains(&format!("peak_gradient_mode = \"{expected_wire}\"")),
                "BarsConfig with peak_gradient_mode={variant:?} should emit \
                 `peak_gradient_mode = \"{expected_wire}\"`, got:\n{toml_str}",
            );
            let parsed: BarsConfig = toml::from_str(&toml_str).expect("deserialize BarsConfig");
            assert_eq!(parsed.peak_gradient_mode, *variant);
        }

        let peak_cases: &[(BarsPeakMode, &str)] = &[
            (BarsPeakMode::None, "none"),
            (BarsPeakMode::Fade, "fade"),
            (BarsPeakMode::Fall, "fall"),
            (BarsPeakMode::FallAccel, "fall_accel"),
            (BarsPeakMode::FallFade, "fall_fade"),
        ];
        for (variant, expected_wire) in peak_cases {
            assert_eq!(variant.as_wire_str(), *expected_wire);
            let cfg = BarsConfig {
                peak_mode: *variant,
                ..Default::default()
            };
            let toml_str = toml::to_string(&cfg).expect("serialize BarsConfig");
            assert!(
                toml_str.contains(&format!("peak_mode = \"{expected_wire}\"")),
                "BarsConfig with peak_mode={variant:?} should emit \
                 `peak_mode = \"{expected_wire}\"`, got:\n{toml_str}",
            );
            let parsed: BarsConfig = toml::from_str(&toml_str).expect("deserialize BarsConfig");
            assert_eq!(parsed.peak_mode, *variant);
        }
    }

    /// Round-trip every `LinesConfig` enum variant through TOML.
    #[test]
    fn lines_config_serde_roundtrip_byte_identity() {
        let grad_cases: &[(LinesGradientMode, &str)] = &[
            (LinesGradientMode::Breathing, "breathing"),
            (LinesGradientMode::Static, "static"),
            (LinesGradientMode::Position, "position"),
            (LinesGradientMode::Height, "height"),
            (LinesGradientMode::Gradient, "gradient"),
        ];
        for (variant, expected_wire) in grad_cases {
            assert_eq!(variant.as_wire_str(), *expected_wire);
            let cfg = LinesConfig {
                gradient_mode: *variant,
                ..Default::default()
            };
            let toml_str = toml::to_string(&cfg).expect("serialize LinesConfig");
            assert!(
                toml_str.contains(&format!("gradient_mode = \"{expected_wire}\"")),
                "LinesConfig with gradient_mode={variant:?} should emit \
                 `gradient_mode = \"{expected_wire}\"`, got:\n{toml_str}",
            );
            let parsed: LinesConfig = toml::from_str(&toml_str).expect("deserialize LinesConfig");
            assert_eq!(parsed.gradient_mode, *variant);
        }

        let style_cases: &[(LinesStyle, &str)] = &[
            (LinesStyle::Smooth, "smooth"),
            (LinesStyle::Angular, "angular"),
        ];
        for (variant, expected_wire) in style_cases {
            assert_eq!(variant.as_wire_str(), *expected_wire);
            let cfg = LinesConfig {
                style: *variant,
                ..Default::default()
            };
            let toml_str = toml::to_string(&cfg).expect("serialize LinesConfig");
            assert!(
                toml_str.contains(&format!("style = \"{expected_wire}\"")),
                "LinesConfig with style={variant:?} should emit \
                 `style = \"{expected_wire}\"`, got:\n{toml_str}",
            );
            let parsed: LinesConfig = toml::from_str(&toml_str).expect("deserialize LinesConfig");
            assert_eq!(parsed.style, *variant);
        }
    }

    /// Existing `config.toml` files on disk (pre-Group-G) may have empty
    /// strings or typo'd values for the enum-typed visualizer fields. The
    /// pre-Group-G `String`-typed implementation silently fell back to the
    /// default for unknown values; the post-Group-G typed enums would
    /// otherwise reject the whole `[visualizer]` section with a serde error.
    /// `deserialize_or_default` restores the field-level silent fallback so
    /// existing user configs keep parsing.
    #[test]
    fn bars_config_tolerates_empty_and_typo_strings() {
        let toml_input = r#"
gradient_mode = "shimer"
gradient_orientation = ""
peak_gradient_mode = ""
peak_mode = "unknown_mode"
"#;
        let cfg: BarsConfig = toml::from_str(toml_input).expect(
            "BarsConfig must tolerate empty + typo strings instead of rejecting the whole struct",
        );
        assert_eq!(cfg.gradient_mode, BarsGradientMode::default());
        assert_eq!(cfg.gradient_orientation, BarsGradientOrientation::default());
        assert_eq!(cfg.peak_gradient_mode, BarsPeakGradientMode::default());
        assert_eq!(cfg.peak_mode, BarsPeakMode::default());
    }

    #[test]
    fn lines_config_tolerates_empty_and_typo_strings() {
        let toml_input = r#"
gradient_mode = ""
style = "wibbly"
"#;
        let cfg: LinesConfig = toml::from_str(toml_input).expect(
            "LinesConfig must tolerate empty + typo strings instead of rejecting the whole struct",
        );
        assert_eq!(cfg.gradient_mode, LinesGradientMode::default());
        assert_eq!(cfg.style, LinesStyle::default());
    }

    /// Lock the WGSL dispatch contract — the `#[repr(u32)]` discriminants on
    /// [`BarsGradientMode`] must match the constants `bars.wgsl` branches on
    /// (`gradient_mode == 0u`, `== 2u`, etc.). Value `1` is intentionally
    /// absent (the dead branch).
    #[test]
    fn bars_gradient_mode_discriminants_match_wgsl_dispatch() {
        assert_eq!(BarsGradientMode::Static as u32, 0);
        assert_eq!(BarsGradientMode::Wave as u32, 2);
        assert_eq!(BarsGradientMode::Shimmer as u32, 3);
        assert_eq!(BarsGradientMode::Energy as u32, 4);
        assert_eq!(BarsGradientMode::Alternate as u32, 5);

        // Lock the full {0, 2, 3, 4, 5} set — assert no variant emits 1.
        let all = [
            BarsGradientMode::Static,
            BarsGradientMode::Wave,
            BarsGradientMode::Shimmer,
            BarsGradientMode::Energy,
            BarsGradientMode::Alternate,
        ];
        for v in all {
            assert_ne!(v as u32, 1, "{v:?} emits 1u — dead in bars.wgsl");
        }
    }

    /// Pin the PascalCase→snake_case transform for the most drift-prone variant.
    /// `FallFade` must serialize to `"fall_fade"` (not `"fallfade"` or `"fall-fade"`).
    #[test]
    fn bars_peak_mode_fall_fade_serializes_to_snake_case() {
        // Direct enum serialization via TOML's value wrapper (TOML can't
        // serialize a bare enum at the document root, so wrap it).
        #[derive(Serialize, Deserialize)]
        struct Wrap {
            v: BarsPeakMode,
        }
        let w = Wrap {
            v: BarsPeakMode::FallFade,
        };
        let s = toml::to_string(&w).expect("serialize Wrap");
        assert!(
            s.contains("v = \"fall_fade\""),
            "FallFade should serialize as `\"fall_fade\"`, got:\n{s}",
        );

        // Also pin the round trip.
        let parsed: Wrap = toml::from_str("v = \"fall_fade\"").expect("deserialize Wrap");
        assert_eq!(parsed.v, BarsPeakMode::FallFade);

        // FallAccel — the other PascalCase variant.
        let w2 = Wrap {
            v: BarsPeakMode::FallAccel,
        };
        let s2 = toml::to_string(&w2).expect("serialize Wrap");
        assert!(
            s2.contains("v = \"fall_accel\""),
            "FallAccel should serialize as `\"fall_accel\"`, got:\n{s2}",
        );

        // And the as_wire_str helper.
        assert_eq!(BarsPeakMode::FallFade.as_wire_str(), "fall_fade");
        assert_eq!(BarsPeakMode::FallAccel.as_wire_str(), "fall_accel");
    }
}
