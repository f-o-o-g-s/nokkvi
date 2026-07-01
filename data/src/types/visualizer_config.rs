//! Visualizer configuration — pure data-crate types (no UI-framework imports).
//!
//! The behavior config for the audio visualizer, persisted in the
//! `[visualizer]` section of `config.toml` ONLY (never redb). Moved here from
//! the UI crate (M3) so the `define_settings!` Visualizer table can dispatch
//! against it; the UI-coupled residue (`ThemeBarColors`, disk load,
//! `ConfigWatcher`, `SharedVisualizerConfig`) stays in
//! `src/visualizer_config.rs`, which re-exports everything here.
//!
//! The 7 mode enums are [`wire_enum!`][crate::wire_enum] invocations: explicit
//! per-variant wire literals tied to serde renames, explicit `#[repr(u32)]`
//! discriminants consumed by the WGSL shaders (note `BarsGradientMode`'s
//! intentionally dead `1`), and a tolerant `from_wire_str` fallback matching
//! `deserialize_or_default`.

use serde::{Deserialize, Serialize};

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

crate::wire_enum! {
    /// Bar gradient color mode.
    ///
    /// Discriminants match the integer dispatch in
    /// `widgets/visualizer/shaders/bars.wgsl`. `1` is intentionally skipped —
    /// `bars.wgsl` has no branch for it and would silently fall through to
    /// the static gradient. See the `bars_gradient_mode_never_emits_dead_1u`
    /// test below. (Modes 3 Shimmer / 4 Energy / 5 Alternate were removed:
    /// the glow / bloom / beat-reactive effects supersede them; existing
    /// configs naming them fall back to Wave via `deserialize_or_default`.)
    #[repr(u32)]
    pub enum BarsGradientMode {
        /// Height-based gradient (bottom to top).
        Static = 0 => "static",
        // value 1 intentionally skipped — dead in bars.wgsl.
        /// Gradient stretching (taller bars show more bottom colors).
        #[default]
        Wave = 2 => "wave",
    }
}

crate::wire_enum! {
    /// Gradient orientation — which axis the gradient colors map along.
    #[repr(u32)]
    pub enum BarsGradientOrientation {
        /// Colors map bottom-to-top within each bar.
        #[default]
        Vertical = 0 => "vertical",
        /// Colors map left-to-right across bars (bass → treble rainbow).
        Horizontal = 1 => "horizontal",
    }
}

crate::wire_enum! {
    /// Peak gradient color mode.
    #[repr(u32)]
    pub enum BarsPeakGradientMode {
        /// First color in `peak_gradient_colors` only.
        Static = 0 => "static",
        /// Time-based animation cycling through all peak colors.
        #[default]
        Cycle = 1 => "cycle",
        /// Color based on peak height.
        Height = 2 => "height",
        /// Uses same color as bar gradient at that height position.
        Match = 3 => "match",
    }
}

crate::wire_enum! {
    /// Peak behavior mode.
    #[repr(u32)]
    pub enum BarsPeakMode {
        /// Peak bars disabled.
        None = 0 => "none",
        /// Hold, then fade out in place.
        Fade = 1 => "fade",
        /// Hold, then fall at constant speed.
        #[default]
        Fall = 2 => "fall",
        /// Hold, then fall with gravity acceleration.
        FallAccel = 3 => "fall_accel",
        /// Hold, then fall at constant speed while fading out.
        FallFade = 4 => "fall_fade",
    }
}

crate::wire_enum! {
    /// Lines mode gradient color mode.
    #[repr(u32)]
    pub enum LinesGradientMode {
        /// Time-based cycling through all gradient colors.
        #[default]
        Breathing = 0 => "breathing",
        /// Uses first gradient color only (no animation).
        Static = 1 => "static",
        /// Color based on horizontal position (bass → treble).
        Position = 2 => "position",
        /// Color based on amplitude (quiet → loud).
        Height = 3 => "height",
        /// Position + amplitude blend (peaks shift palette).
        Gradient = 4 => "gradient",
    }
}

crate::wire_enum! {
    /// Lines mode interpolation style.
    #[repr(u32)]
    pub enum LinesStyle {
        /// Catmull-Rom spline (curvy).
        #[default]
        Smooth = 0 => "smooth",
        /// Straight line segments between points.
        Angular = 1 => "angular",
    }
}

crate::wire_enum! {
    /// Where a spectrum visualizer mode (Bars / Lines) is drawn on screen.
    ///
    /// `OverCover` (the default) draws the visualizer over the now-playing
    /// cover art in the Queue view — the same slot the Scope ring uses — and
    /// only while audio is playing; it's the more striking first impression
    /// (the app opens to the Queue). `BottomBand` is the classic placement: a
    /// band across the bottom of the window, above the player bar, visible on
    /// every view. Scope is always drawn over the cover and has no placement
    /// of its own.
    #[repr(u32)]
    pub enum VisualizerPlacement {
        /// A band across the bottom of the window, above the player bar.
        BottomBand = 0 => "bottom_band",
        /// Over the now-playing cover art (Queue view, while playing).
        /// Default — the integrated cover look greets a new user on the
        /// default Queue view.
        #[default]
        OverCover = 1 => "over_cover",
    }
}

/// Bars mode configuration.
/// Maps to `[visualizer.bars]` in config.toml.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
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
    /// Default: 2.0
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
    /// Works with all gradient modes.
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
    /// Default: 0.0
    pub bar_depth_3d: f32,

    /// Peak-flash bloom strength for Bars mode (0.0 = disabled, 1.0 = max).
    /// Bars bloom toward the peak color when they hit a transient/beat, using
    /// the per-bar flash envelope already computed by `update_flash_effect`.
    /// Default: 0.6
    pub flash_intensity: f32,

    /// Maximum number of bars to display.
    /// The dynamic layout algorithm will try to fit up to this many bars in the window.
    /// Default: 512, range 16–2048
    pub max_bars: usize,

    /// Motion trails: bars leave a fading after-image (0.0 = off,
    /// 1.0 = long comet trails). Maps to a per-frame persistence/decay.
    /// Per-mode (was a single global knob).
    /// Default: 0.0 (off — it noticeably changes the visualizer's character)
    pub trails: f32,

    /// Echo (Milkdrop-style zoom/rotate feedback): the bars spiral and tunnel
    /// into themselves, swirling with the bass/beat (0.0 = off, 1.0 = strong
    /// persistence). A psychedelic feedback layer; takes over the display when on.
    /// Default: 0.0 (off — strong character change)
    pub echo: f32,

    /// Where the Bars visualizer is drawn. See [`VisualizerPlacement`].
    /// Default: [`VisualizerPlacement::OverCover`] (over the now-playing cover art)
    #[serde(deserialize_with = "deserialize_or_default")]
    pub placement: VisualizerPlacement,
}

impl Default for BarsConfig {
    fn default() -> Self {
        Self {
            bar_width_min: 10.0,
            bar_width_max: 20.0,
            bar_spacing: 2.0,
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
            bar_depth_3d: 0.0,
            flash_intensity: 0.6,
            max_bars: 512,
            trails: 0.0,
            echo: 0.0,
            placement: VisualizerPlacement::OverCover,
        }
    }
}

impl BarsConfig {
    /// Get the gradient mode as u32 for shader (0=static, 2=wave).
    ///
    /// `1u` is intentionally absent from the emitted set — `bars.wgsl` does not branch on it
    /// and would silently fall through to the static gradient. The explicit discriminants on
    /// [`BarsGradientMode`] preserve this non-contiguous {0, 2} encoding; the
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
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct LinesConfig {
    /// Number of points to render (default: 8)
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
    /// Neon glow halo around the main line (0.0 = disabled, 1.0 = max).
    /// An exponential emissive falloff beyond the stroke that brightens with
    /// loudness. Rendered in `lines.wgsl` (the dark outline pass never glows).
    /// Default: 0.5
    pub glow_intensity: f32,
    /// Mirror mode: render waveform symmetrically from center.
    /// Default: false
    pub mirror: bool,
    /// Interpolation style. See [`LinesStyle`] for variants.
    ///
    /// Default: [`LinesStyle::Smooth`]
    #[serde(deserialize_with = "deserialize_or_default")]
    pub style: LinesStyle,
    /// Surfing boat: render a small boat that rides the waveform.
    /// Default: true
    pub boat: bool,

    /// Motion trails: the line leaves a fading after-image (0.0 = off,
    /// 1.0 = long comet trails). Maps to a per-frame persistence/decay.
    /// Per-mode (was a single global knob).
    /// Default: 0.0 (off — it noticeably changes the visualizer's character)
    pub trails: f32,

    /// Echo (Milkdrop-style zoom/rotate feedback): the line spirals and tunnels
    /// into itself, swirling with the bass/beat (0.0 = off, 1.0 = strong
    /// persistence). A psychedelic feedback layer; takes over the display when on.
    /// Default: 0.0 (off — strong character change)
    pub echo: f32,

    /// Where the Lines visualizer is drawn. See [`VisualizerPlacement`].
    /// Default: [`VisualizerPlacement::OverCover`] (over the now-playing cover art)
    #[serde(deserialize_with = "deserialize_or_default")]
    pub placement: VisualizerPlacement,
}

impl Default for LinesConfig {
    fn default() -> Self {
        Self {
            point_count: 8,
            line_thickness: 0.01,
            outline_thickness: 1.0,
            outline_opacity: 1.0,
            animation_speed: 0.1,
            gradient_mode: LinesGradientMode::Static,
            fill_opacity: 0.5,
            glow_intensity: 0.5,
            mirror: false,
            style: LinesStyle::Smooth,
            boat: true,
            trails: 0.0,
            echo: 0.0,
            placement: VisualizerPlacement::OverCover,
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

/// Scope (circular oscilloscope) mode specific configuration.
///
/// Mirrors the Lines appearance knobs (so the time-domain ring can be styled
/// independently of Lines mode) plus two geometry params unique to the ring:
/// `radius` (how big the ring sits over the cover) and `sensitivity` (how hard
/// the waveform swings). Reuses [`LinesGradientMode`] / [`LinesStyle`].
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ScopeConfig {
    /// Number of points around the ring (more = finer waveform detail).
    /// Default: 16 (a chunky, smooth-blobby ring)
    pub point_count: usize,
    /// Mean ring radius as a fraction of the available space inside the cover
    /// (0.1 = tiny inner ring, 0.95 = nearly fills the panel). Default: 0.7
    pub radius: f32,
    /// Waveform swing / gain — how far loud audio pushes the ring in and out
    /// (0.5 = subtle, 5.0 = wild). Default: 1.5
    pub sensitivity: f32,
    /// Line thickness as a fraction of the panel size (0.005-0.1). Default: 0.005
    pub line_thickness: f32,
    /// Radial gradient fill from the ring toward the center (0.0 = no fill,
    /// 1.0 = opaque rim). Default: 0.75
    pub fill_opacity: f32,
    /// Neon glow halo around the ring (0.0 = disabled, 1.0 = max). Default: 0.75
    pub glow_intensity: f32,
    /// Outline thickness in pixels behind the ring (0.0 = disabled). Default: 0.0
    pub outline_thickness: f32,
    /// Outline opacity (0.0 = invisible, 1.0 = fully opaque). Default: 0.0
    pub outline_opacity: f32,
    /// Gradient color mode. See [`LinesGradientMode`]. Default: Height
    #[serde(deserialize_with = "deserialize_or_default")]
    pub gradient_mode: LinesGradientMode,
    /// Color animation cycle speed for the breathing gradient (0.05-1.0).
    /// Default: 0.1
    pub animation_speed: f32,
    /// Interpolation style around the ring. See [`LinesStyle`]. Default: Smooth
    #[serde(deserialize_with = "deserialize_or_default")]
    pub style: LinesStyle,
    /// Glowing particle field drifting out from the ring (the NCS / Wav2Bar
    /// look). Default: true
    pub particles: bool,
    /// Number of particles in the field (0 disables). Default: 512
    pub particle_count: usize,
    /// Particle speed multiplier — scales both the launch kick AND the ongoing
    /// curl-swirl drift, so the whole field moves faster/slower (0.1 = lazy
    /// drift, 4.0 = energetic). Default: 1.0
    pub particle_speed: f32,
    /// Luminous-beam look: render the ring with additive blending so the glow
    /// accumulates into a bright neon beam over the cover (woscope-style).
    /// Default: true
    pub beam: bool,

    /// Motion trails: the ring leaves a fading after-image (0.0 = off,
    /// 1.0 = long comet trails). Maps to a per-frame persistence/decay.
    /// Per-mode (was a single global knob).
    /// Default: 0.0 (off — it noticeably changes the visualizer's character)
    pub trails: f32,

    /// Echo (Milkdrop-style zoom/rotate feedback): the ring spirals and tunnels
    /// inward, swirling with the bass/beat (0.0 = off, 1.0 = strong persistence).
    /// A psychedelic feedback layer; takes over the display when on.
    /// Default: 0.25 (a subtle feedback swirl)
    pub echo: f32,
}

impl Default for ScopeConfig {
    fn default() -> Self {
        Self {
            point_count: 16,
            radius: 0.7,
            sensitivity: 1.5,
            line_thickness: 0.01,
            fill_opacity: 0.5,
            glow_intensity: 0.35,
            outline_thickness: 0.0,
            outline_opacity: 0.0,
            gradient_mode: LinesGradientMode::Height,
            animation_speed: 0.1,
            style: LinesStyle::Smooth,
            particles: true,
            particle_count: 192,
            particle_speed: 0.5,
            beam: true,
            trails: 0.0,
            echo: 1.0,
        }
    }
}

impl ScopeConfig {
    /// Gradient mode as u32 for the shader (matches `LinesGradientMode`).
    pub fn get_gradient_mode_value(&self) -> u32 {
        self.gradient_mode as u32
    }

    /// Interpolation style as u32 for the shader (0=smooth, 1=angular).
    pub fn get_style_value(&self) -> u32 {
        self.style as u32
    }
}

/// Minimum effective monstercat value.
/// Below this, `monstercat * 1.5 < 1.0` so the exponential base inverts the filter
/// (amplifies neighbors instead of attenuating). Values in `(0.0, MIN)` are snapped to 0.0.
pub const MONSTERCAT_MIN_EFFECTIVE: f64 = 0.7;

/// Typed TOML key constants for all `visualizer.*` config entries.
///
/// Use these instead of raw string literals so that typos become compile errors.
/// The `starts_with("visualizer.")` prefix check in `update/settings.rs` is
/// intentionally left as a string literal — it is structural routing logic,
/// not a specific key name.
pub mod keys {
    // ── General ─────────────────────────────────────────────────────────
    pub const NOISE_REDUCTION: &str = "visualizer.noise_reduction";
    pub const WAVES: &str = "visualizer.waves";
    pub const WAVES_SMOOTHING: &str = "visualizer.waves_smoothing";
    pub const MONSTERCAT: &str = "visualizer.monstercat";
    pub const LOWER_CUTOFF_FREQ: &str = "visualizer.lower_cutoff_freq";
    pub const HIGHER_CUTOFF_FREQ: &str = "visualizer.higher_cutoff_freq";
    pub const HEIGHT_PERCENT: &str = "visualizer.height_percent";
    pub const OPACITY: &str = "visualizer.opacity";
    pub const AUTO_SENSITIVITY: &str = "visualizer.auto_sensitivity";
    pub const BLOOM: &str = "visualizer.bloom";
    pub const BLOOM_INTENSITY: &str = "visualizer.bloom_intensity";
    pub const BEAT_REACTIVITY: &str = "visualizer.beat_reactivity";
    pub const CRT: &str = "visualizer.crt";

    // ── Bars ─────────────────────────────────────────────────────────────
    pub const BARS_MAX_BARS: &str = "visualizer.bars.max_bars";
    pub const BARS_BAR_WIDTH_MIN: &str = "visualizer.bars.bar_width_min";
    pub const BARS_BAR_WIDTH_MAX: &str = "visualizer.bars.bar_width_max";
    pub const BARS_BAR_SPACING: &str = "visualizer.bars.bar_spacing";
    pub const BARS_BORDER_WIDTH: &str = "visualizer.bars.border_width";
    pub const BARS_LED_BARS: &str = "visualizer.bars.led_bars";
    pub const BARS_LED_SEGMENT_HEIGHT: &str = "visualizer.bars.led_segment_height";
    pub const BARS_GRADIENT_MODE: &str = "visualizer.bars.gradient_mode";
    pub const BARS_GRADIENT_ORIENTATION: &str = "visualizer.bars.gradient_orientation";
    pub const BARS_PEAK_GRADIENT_MODE: &str = "visualizer.bars.peak_gradient_mode";
    pub const BARS_PEAK_MODE: &str = "visualizer.bars.peak_mode";
    pub const BARS_PEAK_HOLD_TIME: &str = "visualizer.bars.peak_hold_time";
    pub const BARS_PEAK_FADE_TIME: &str = "visualizer.bars.peak_fade_time";
    pub const BARS_PEAK_FALL_SPEED: &str = "visualizer.bars.peak_fall_speed";
    pub const BARS_PEAK_HEIGHT_RATIO: &str = "visualizer.bars.peak_height_ratio";
    pub const BARS_BAR_DEPTH_3D: &str = "visualizer.bars.bar_depth_3d";
    pub const BARS_FLASH_INTENSITY: &str = "visualizer.bars.flash_intensity";
    pub const BARS_TRAILS: &str = "visualizer.bars.trails";
    pub const BARS_ECHO: &str = "visualizer.bars.echo";
    pub const BARS_PLACEMENT: &str = "visualizer.bars.placement";

    // ── Lines ────────────────────────────────────────────────────────────
    pub const LINES_POINT_COUNT: &str = "visualizer.lines.point_count";
    pub const LINES_LINE_THICKNESS: &str = "visualizer.lines.line_thickness";
    pub const LINES_OUTLINE_THICKNESS: &str = "visualizer.lines.outline_thickness";
    pub const LINES_OUTLINE_OPACITY: &str = "visualizer.lines.outline_opacity";
    pub const LINES_ANIMATION_SPEED: &str = "visualizer.lines.animation_speed";
    pub const LINES_GRADIENT_MODE: &str = "visualizer.lines.gradient_mode";
    pub const LINES_FILL_OPACITY: &str = "visualizer.lines.fill_opacity";
    pub const LINES_GLOW_INTENSITY: &str = "visualizer.lines.glow_intensity";
    pub const LINES_MIRROR: &str = "visualizer.lines.mirror";
    pub const LINES_STYLE: &str = "visualizer.lines.style";
    pub const LINES_BOAT: &str = "visualizer.lines.boat";
    pub const LINES_TRAILS: &str = "visualizer.lines.trails";
    pub const LINES_ECHO: &str = "visualizer.lines.echo";
    pub const LINES_PLACEMENT: &str = "visualizer.lines.placement";

    // ── Scope (circular oscilloscope) ────────────────────────────────────
    pub const SCOPE_POINT_COUNT: &str = "visualizer.scope.point_count";
    pub const SCOPE_RADIUS: &str = "visualizer.scope.radius";
    pub const SCOPE_SENSITIVITY: &str = "visualizer.scope.sensitivity";
    pub const SCOPE_LINE_THICKNESS: &str = "visualizer.scope.line_thickness";
    pub const SCOPE_FILL_OPACITY: &str = "visualizer.scope.fill_opacity";
    pub const SCOPE_GLOW_INTENSITY: &str = "visualizer.scope.glow_intensity";
    pub const SCOPE_OUTLINE_THICKNESS: &str = "visualizer.scope.outline_thickness";
    pub const SCOPE_OUTLINE_OPACITY: &str = "visualizer.scope.outline_opacity";
    pub const SCOPE_GRADIENT_MODE: &str = "visualizer.scope.gradient_mode";
    pub const SCOPE_ANIMATION_SPEED: &str = "visualizer.scope.animation_speed";
    pub const SCOPE_STYLE: &str = "visualizer.scope.style";
    pub const SCOPE_PARTICLES: &str = "visualizer.scope.particles";
    pub const SCOPE_PARTICLE_COUNT: &str = "visualizer.scope.particle_count";
    pub const SCOPE_PARTICLE_SPEED: &str = "visualizer.scope.particle_speed";
    pub const SCOPE_BEAM: &str = "visualizer.scope.beam";
    pub const SCOPE_TRAILS: &str = "visualizer.scope.trails";
    pub const SCOPE_ECHO: &str = "visualizer.scope.echo";

    /// Every `visualizer.*` key exactly once — the exhaustiveness registry the
    /// `every_visualizer_key_has_a_macro_entry` test pins the dispatch table
    /// against (bidirectionally). Add new key consts here AND to the
    /// Visualizer settings table.
    pub const ALL_KEYS: &[&str] = &[
        NOISE_REDUCTION,
        WAVES,
        WAVES_SMOOTHING,
        MONSTERCAT,
        LOWER_CUTOFF_FREQ,
        HIGHER_CUTOFF_FREQ,
        HEIGHT_PERCENT,
        OPACITY,
        AUTO_SENSITIVITY,
        BLOOM,
        BLOOM_INTENSITY,
        BEAT_REACTIVITY,
        CRT,
        BARS_MAX_BARS,
        BARS_BAR_WIDTH_MIN,
        BARS_BAR_WIDTH_MAX,
        BARS_BAR_SPACING,
        BARS_BORDER_WIDTH,
        BARS_LED_BARS,
        BARS_LED_SEGMENT_HEIGHT,
        BARS_GRADIENT_MODE,
        BARS_GRADIENT_ORIENTATION,
        BARS_PEAK_GRADIENT_MODE,
        BARS_PEAK_MODE,
        BARS_PEAK_HOLD_TIME,
        BARS_PEAK_FADE_TIME,
        BARS_PEAK_FALL_SPEED,
        BARS_PEAK_HEIGHT_RATIO,
        BARS_BAR_DEPTH_3D,
        BARS_FLASH_INTENSITY,
        BARS_TRAILS,
        BARS_ECHO,
        BARS_PLACEMENT,
        LINES_POINT_COUNT,
        LINES_LINE_THICKNESS,
        LINES_OUTLINE_THICKNESS,
        LINES_OUTLINE_OPACITY,
        LINES_ANIMATION_SPEED,
        LINES_GRADIENT_MODE,
        LINES_FILL_OPACITY,
        LINES_GLOW_INTENSITY,
        LINES_MIRROR,
        LINES_STYLE,
        LINES_BOAT,
        LINES_TRAILS,
        LINES_ECHO,
        LINES_PLACEMENT,
        SCOPE_POINT_COUNT,
        SCOPE_RADIUS,
        SCOPE_SENSITIVITY,
        SCOPE_LINE_THICKNESS,
        SCOPE_FILL_OPACITY,
        SCOPE_GLOW_INTENSITY,
        SCOPE_OUTLINE_THICKNESS,
        SCOPE_OUTLINE_OPACITY,
        SCOPE_GRADIENT_MODE,
        SCOPE_ANIMATION_SPEED,
        SCOPE_STYLE,
        SCOPE_PARTICLES,
        SCOPE_PARTICLE_COUNT,
        SCOPE_PARTICLE_SPEED,
        SCOPE_BEAM,
        SCOPE_TRAILS,
        SCOPE_ECHO,
    ];
}

/// Visualizer configuration loaded from config.toml
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
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
    /// Default: 0.40 (40%)
    pub height_percent: f32,

    /// Overall visualizer opacity (0.0 = invisible, 1.0 = fully opaque).
    /// Default: 1.0
    pub opacity: f32,

    /// Bloom glow post-processing: bright bars / peak flashes / the neon line
    /// core bleed a soft additive halo. Applies to every mode.
    /// Default: true
    pub bloom: bool,

    /// Bloom glow strength (0.0 = off, 1.0 = max additive glow).
    /// Default: 0.6
    pub bloom_intensity: f32,

    /// Beat reactivity: how strongly effects pump on the beat / bass drops
    /// (0.0 = static, loudness-only; 1.0 = full punch). Scales the bloom
    /// surge, the neon glow flare, and the bar brightness lift together.
    /// Default: 1.0
    pub beat_reactivity: f32,

    /// CRT / film composite: a retro post-process (chromatic aberration,
    /// scanlines, vignette, grain, beat zoom-punch), one master amount
    /// (0.0 = off, 1.0 = full). Opt-in.
    /// Default: 0.0
    pub crt: f32,

    /// Bars mode specific settings
    /// Use [visualizer.bars] in config.toml
    #[serde(default)]
    pub bars: BarsConfig,

    /// Lines mode specific settings
    /// Use [visualizer.lines] in config.toml
    #[serde(default)]
    pub lines: LinesConfig,

    /// Scope (circular oscilloscope) mode specific settings
    /// Use [visualizer.scope] in config.toml
    #[serde(default)]
    pub scope: ScopeConfig,
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
            height_percent: 0.40,
            opacity: 1.0,
            bloom: true,
            bloom_intensity: 0.6,
            beat_reactivity: 1.0,
            crt: 0.0,
            bars: BarsConfig::default(),
            lines: LinesConfig::default(),
            scope: ScopeConfig::default(),
        }
    }
}

/// NaN-safe clamp: a hand-written `nan` in `[visualizer]` snaps to `lo`
/// instead of propagating. NaN must never survive `validate()` — it defeats
/// the PartialEq change gate on the shared-config apply (NaN != NaN → FFT
/// re-init on every settings interaction) and panics `clamp` when the NaN
/// field is itself used as a bound (`bar_width_max.clamp(bar_width_min, ..)`).
fn finite_clamp32(v: f32, lo: f32, hi: f32) -> f32 {
    if v.is_nan() { lo } else { v.clamp(lo, hi) }
}

/// See [`finite_clamp32`].
fn finite_clamp64(v: f64, lo: f64, hi: f64) -> f64 {
    if v.is_nan() { lo } else { v.clamp(lo, hi) }
}

impl VisualizerConfig {
    /// Validate and clamp values to valid ranges
    pub fn validate(&mut self) {
        self.noise_reduction = finite_clamp64(self.noise_reduction, 0.0, 1.0);
        // Snap sub-threshold values to off — the filter amplifies instead of
        // attenuating when `monstercat * 1.5 < 1.0`. NaN also snaps to off
        // (it must never survive validate — see `finite_clamp32`).
        if self.monstercat.is_nan() || self.monstercat < MONSTERCAT_MIN_EFFECTIVE {
            self.monstercat = 0.0;
        }
        self.waves_smoothing = self.waves_smoothing.clamp(2, 16);

        // Frequency cutoffs: lower must be at least 20Hz, higher must be > lower
        self.lower_cutoff_freq = self.lower_cutoff_freq.max(20);
        self.higher_cutoff_freq = self.higher_cutoff_freq.max(self.lower_cutoff_freq + 100);
        // Cap higher cutoff at 22050 Hz (Nyquist for 44100 sample rate)
        self.higher_cutoff_freq = self.higher_cutoff_freq.min(22050);

        // Validate bars config

        self.bars.bar_width_min = finite_clamp32(self.bars.bar_width_min, 1.0, 10.0);
        self.bars.bar_width_max =
            finite_clamp32(self.bars.bar_width_max, self.bars.bar_width_min, 20.0);
        // NaN-safe as-is: f32::max ignores a NaN operand (returns 0.0).
        self.bars.bar_spacing = self.bars.bar_spacing.max(0.0);
        self.bars.border_width = finite_clamp32(self.bars.border_width, 0.0, 5.0);
        self.bars.led_segment_height = finite_clamp32(self.bars.led_segment_height, 2.0, 20.0);
        self.bars.bar_depth_3d = finite_clamp32(self.bars.bar_depth_3d, 0.0, 20.0);
        self.bars.flash_intensity = finite_clamp32(self.bars.flash_intensity, 0.0, 1.0);
        self.bars.peak_height_ratio = self.bars.peak_height_ratio.clamp(10, 100);
        self.bars.peak_fall_speed = self.bars.peak_fall_speed.clamp(1, 20);
        self.bars.max_bars = self.bars.max_bars.clamp(16, 2048);
        self.bars.trails = finite_clamp32(self.bars.trails, 0.0, 1.0);
        self.bars.echo = finite_clamp32(self.bars.echo, 0.0, 1.0);

        // Validate lines config
        self.lines.point_count = self.lines.point_count.clamp(8, 512);
        self.lines.line_thickness = finite_clamp32(self.lines.line_thickness, 0.01, 0.1);
        self.lines.outline_thickness = finite_clamp32(self.lines.outline_thickness, 0.0, 5.0);
        self.lines.outline_opacity = finite_clamp32(self.lines.outline_opacity, 0.0, 1.0);
        self.lines.animation_speed = finite_clamp32(self.lines.animation_speed, 0.05, 1.0);
        self.lines.fill_opacity = finite_clamp32(self.lines.fill_opacity, 0.0, 1.0);
        self.lines.glow_intensity = finite_clamp32(self.lines.glow_intensity, 0.0, 1.0);
        self.lines.trails = finite_clamp32(self.lines.trails, 0.0, 1.0);
        self.lines.echo = finite_clamp32(self.lines.echo, 0.0, 1.0);

        // Validate scope config.
        self.scope.point_count = self.scope.point_count.clamp(16, 512);
        self.scope.radius = finite_clamp32(self.scope.radius, 0.1, 0.95);
        self.scope.sensitivity = finite_clamp32(self.scope.sensitivity, 0.5, 5.0);
        self.scope.line_thickness = finite_clamp32(self.scope.line_thickness, 0.005, 0.1);
        self.scope.fill_opacity = finite_clamp32(self.scope.fill_opacity, 0.0, 1.0);
        self.scope.glow_intensity = finite_clamp32(self.scope.glow_intensity, 0.0, 1.0);
        self.scope.outline_thickness = finite_clamp32(self.scope.outline_thickness, 0.0, 5.0);
        self.scope.outline_opacity = finite_clamp32(self.scope.outline_opacity, 0.0, 1.0);
        self.scope.animation_speed = finite_clamp32(self.scope.animation_speed, 0.05, 1.0);
        self.scope.particle_count = self.scope.particle_count.min(2048);
        self.scope.particle_speed = finite_clamp32(self.scope.particle_speed, 0.1, 4.0);
        self.scope.trails = finite_clamp32(self.scope.trails, 0.0, 1.0);
        self.scope.echo = finite_clamp32(self.scope.echo, 0.0, 1.0);

        // Validate height_percent (10% to 60% — above 60% the visualizer overlaps the player bar)
        self.height_percent = finite_clamp32(self.height_percent, 0.1, 0.60);

        // Validate opacity (0.0–1.0)
        self.opacity = finite_clamp32(self.opacity, 0.0, 1.0);

        // Validate bloom intensity (0.0–1.0)
        self.bloom_intensity = finite_clamp32(self.bloom_intensity, 0.0, 1.0);

        // Validate beat reactivity (0.0–1.0)
        self.beat_reactivity = finite_clamp32(self.beat_reactivity, 0.0, 1.0);

        self.crt = finite_clamp32(self.crt, 0.0, 1.0);
    }
}

/// Full config file structure (only the `[visualizer]` section is modeled;
/// other sections are ignored — credentials/settings are handled separately).
/// Public so the UI crate's disk loader can parse through it until the
/// unified reload path fully owns the read.
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct ConfigFile {
    #[serde(default)]
    pub visualizer: VisualizerConfig,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// M3-S1 pin: the pure visualizer types live in the DATA crate
    /// and are constructible from here.
    #[test]
    fn visualizer_config_is_in_data_crate() {
        let cfg = crate::types::visualizer_config::VisualizerConfig::default();
        assert!(cfg.auto_sensitivity);
        assert_eq!(cfg.bars.gradient_mode, BarsGradientMode::Wave);
    }

    /// `validate()` clamps the Scope ring point count into `[16, 512]`. The old
    /// even-only constraint (which existed solely so the mirror seam closed into
    /// a palindrome) is gone with the seam tuning, so in-range odd values are now
    /// preserved as-is.
    #[test]
    fn validate_clamps_scope_point_count() {
        // In-range odd values survive untouched (no even-rounding any more).
        for v in [17usize, 33, 129, 511] {
            let mut cfg = VisualizerConfig {
                scope: ScopeConfig {
                    point_count: v,
                    ..Default::default()
                },
                ..Default::default()
            };
            cfg.validate();
            assert_eq!(cfg.scope.point_count, v, "in-range value {v} was altered");
        }

        // Out-of-range values clamp to the bounds.
        let mut low = VisualizerConfig {
            scope: ScopeConfig {
                point_count: 3,
                ..Default::default()
            },
            ..Default::default()
        };
        low.validate();
        assert_eq!(low.scope.point_count, 16);

        let mut high = VisualizerConfig {
            scope: ScopeConfig {
                point_count: 9000,
                ..Default::default()
            },
            ..Default::default()
        };
        high.validate();
        assert_eq!(high.scope.point_count, 512);
    }

    /// Pins the `BarsConfig::get_gradient_mode_value` emitted u32 set so a future agent
    /// who adds a `1`-valued variant fails immediately — `bars.wgsl` has no branch for
    /// `1u` and would silently fall through to the static gradient. See Tier 0 #0.10 in
    /// the 2026-05-11 audit roadmap.
    #[test]
    fn bars_gradient_mode_never_emits_dead_1u() {
        // Every defined variant (the only inputs reachable from the TOML config + UI dropdown).
        let variants = [BarsGradientMode::Static, BarsGradientMode::Wave];
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
        let expected: &[(BarsGradientMode, u32)] =
            &[(BarsGradientMode::Static, 0), (BarsGradientMode::Wave, 2)];
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
    /// per-variant `#[serde(rename = ...)]` wire format is preserved
    /// end-to-end and matches the literal strings stored in `config.toml`.
    #[test]
    fn bars_config_serde_roundtrip_byte_identity() {
        let cases: &[(BarsGradientMode, &str)] = &[
            (BarsGradientMode::Static, "static"),
            (BarsGradientMode::Wave, "wave"),
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

        let placement_cases: &[(VisualizerPlacement, &str)] = &[
            (VisualizerPlacement::BottomBand, "bottom_band"),
            (VisualizerPlacement::OverCover, "over_cover"),
        ];
        for (variant, expected_wire) in placement_cases {
            assert_eq!(variant.as_wire_str(), *expected_wire);
            let cfg = BarsConfig {
                placement: *variant,
                ..Default::default()
            };
            let toml_str = toml::to_string(&cfg).expect("serialize BarsConfig");
            assert!(
                toml_str.contains(&format!("placement = \"{expected_wire}\"")),
                "BarsConfig with placement={variant:?} should emit \
                 `placement = \"{expected_wire}\"`, got:\n{toml_str}",
            );
            let parsed: BarsConfig = toml::from_str(&toml_str).expect("deserialize BarsConfig");
            assert_eq!(parsed.placement, *variant);
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

        let placement_cases: &[(VisualizerPlacement, &str)] = &[
            (VisualizerPlacement::BottomBand, "bottom_band"),
            (VisualizerPlacement::OverCover, "over_cover"),
        ];
        for (variant, expected_wire) in placement_cases {
            assert_eq!(variant.as_wire_str(), *expected_wire);
            let cfg = LinesConfig {
                placement: *variant,
                ..Default::default()
            };
            let toml_str = toml::to_string(&cfg).expect("serialize LinesConfig");
            assert!(
                toml_str.contains(&format!("placement = \"{expected_wire}\"")),
                "LinesConfig with placement={variant:?} should emit \
                 `placement = \"{expected_wire}\"`, got:\n{toml_str}",
            );
            let parsed: LinesConfig = toml::from_str(&toml_str).expect("deserialize LinesConfig");
            assert_eq!(parsed.placement, *variant);
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
placement = "somewhere_else"
"#;
        let cfg: BarsConfig = toml::from_str(toml_input).expect(
            "BarsConfig must tolerate empty + typo strings instead of rejecting the whole struct",
        );
        assert_eq!(cfg.gradient_mode, BarsGradientMode::default());
        assert_eq!(cfg.gradient_orientation, BarsGradientOrientation::default());
        assert_eq!(cfg.peak_gradient_mode, BarsPeakGradientMode::default());
        assert_eq!(cfg.peak_mode, BarsPeakMode::default());
        assert_eq!(cfg.placement, VisualizerPlacement::default());
    }

    #[test]
    fn lines_config_tolerates_empty_and_typo_strings() {
        let toml_input = r#"
gradient_mode = ""
style = "wibbly"
placement = "nowhere"
"#;
        let cfg: LinesConfig = toml::from_str(toml_input).expect(
            "LinesConfig must tolerate empty + typo strings instead of rejecting the whole struct",
        );
        assert_eq!(cfg.gradient_mode, LinesGradientMode::default());
        assert_eq!(cfg.style, LinesStyle::default());
        assert_eq!(cfg.placement, VisualizerPlacement::default());
    }

    /// Pin the owner-chosen default placement: Bars/Lines draw over the cover by
    /// default (the app opens to the Queue, so it's the striking first
    /// impression). The typo-tolerance tests above only compare against
    /// `default()`, so they wouldn't catch a flip of the `#[default]` back to
    /// `BottomBand` — this asserts the concrete value.
    #[test]
    fn default_placement_is_over_cover() {
        assert_eq!(
            VisualizerPlacement::default(),
            VisualizerPlacement::OverCover
        );
        assert_eq!(
            BarsConfig::default().placement,
            VisualizerPlacement::OverCover
        );
        assert_eq!(
            LinesConfig::default().placement,
            VisualizerPlacement::OverCover
        );
    }

    /// Lock the WGSL dispatch contract — the `#[repr(u32)]` discriminants on
    /// [`BarsGradientMode`] must match the constants `bars.wgsl` branches on
    /// (`gradient_mode == 0u`, `== 2u`, etc.). Value `1` is intentionally
    /// absent (the dead branch).
    #[test]
    fn bars_gradient_mode_discriminants_match_wgsl_dispatch() {
        assert_eq!(BarsGradientMode::Static as u32, 0);
        assert_eq!(BarsGradientMode::Wave as u32, 2);

        // Lock the full {0, 2} set — assert no variant emits 1 (dead in bars.wgsl).
        let all = [BarsGradientMode::Static, BarsGradientMode::Wave];
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

    /// Every `ALL` const carries every variant exactly once, in declaration
    /// order, with pairwise-distinct wire strings. The no-wildcard exhaustive
    /// match per enum means adding a variant breaks this test at compile time,
    /// forcing a review of the `ALL` slice (and thus the settings dropdown).
    #[test]
    fn all_consts_are_exhaustive_and_ordered() {
        for v in BarsGradientMode::ALL {
            match v {
                BarsGradientMode::Static | BarsGradientMode::Wave => {}
            }
        }
        assert_eq!(BarsGradientMode::ALL.len(), 2);

        for v in BarsGradientOrientation::ALL {
            match v {
                BarsGradientOrientation::Vertical | BarsGradientOrientation::Horizontal => {}
            }
        }
        assert_eq!(BarsGradientOrientation::ALL.len(), 2);

        for v in BarsPeakGradientMode::ALL {
            match v {
                BarsPeakGradientMode::Static
                | BarsPeakGradientMode::Cycle
                | BarsPeakGradientMode::Height
                | BarsPeakGradientMode::Match => {}
            }
        }
        assert_eq!(BarsPeakGradientMode::ALL.len(), 4);

        for v in BarsPeakMode::ALL {
            match v {
                BarsPeakMode::None
                | BarsPeakMode::Fade
                | BarsPeakMode::Fall
                | BarsPeakMode::FallAccel
                | BarsPeakMode::FallFade => {}
            }
        }
        assert_eq!(BarsPeakMode::ALL.len(), 5);

        for v in LinesGradientMode::ALL {
            match v {
                LinesGradientMode::Breathing
                | LinesGradientMode::Static
                | LinesGradientMode::Position
                | LinesGradientMode::Height
                | LinesGradientMode::Gradient => {}
            }
        }
        assert_eq!(LinesGradientMode::ALL.len(), 5);

        for v in LinesStyle::ALL {
            match v {
                LinesStyle::Smooth | LinesStyle::Angular => {}
            }
        }
        assert_eq!(LinesStyle::ALL.len(), 2);

        for v in VisualizerPlacement::ALL {
            match v {
                VisualizerPlacement::BottomBand | VisualizerPlacement::OverCover => {}
            }
        }
        assert_eq!(VisualizerPlacement::ALL.len(), 2);

        // Wire strings must be pairwise distinct per enum — duplicates would
        // make the dropdown's selected-value matching ambiguous.
        fn assert_distinct(name: &str, strs: &[&'static str]) {
            for (i, a) in strs.iter().enumerate() {
                for b in &strs[i + 1..] {
                    assert_ne!(a, b, "{name} has duplicate wire string {a:?}");
                }
            }
        }
        assert_distinct("BarsGradientMode", &BarsGradientMode::all_wire_strs());
        assert_distinct(
            "BarsGradientOrientation",
            &BarsGradientOrientation::all_wire_strs(),
        );
        assert_distinct(
            "BarsPeakGradientMode",
            &BarsPeakGradientMode::all_wire_strs(),
        );
        assert_distinct("BarsPeakMode", &BarsPeakMode::all_wire_strs());
        assert_distinct("LinesGradientMode", &LinesGradientMode::all_wire_strs());
        assert_distinct("LinesStyle", &LinesStyle::all_wire_strs());
        assert_distinct("VisualizerPlacement", &VisualizerPlacement::all_wire_strs());
    }

    /// Pins each `all_wire_strs()` output to the exact literal vec the
    /// settings dropdowns used before deriving from `ALL` — locks both the
    /// display order and the snake_case wire contract.
    #[test]
    fn all_wire_strs_pin_dropdown_display_order() {
        assert_eq!(BarsGradientMode::all_wire_strs(), vec!["static", "wave"]);
        assert_eq!(
            BarsGradientOrientation::all_wire_strs(),
            vec!["vertical", "horizontal"],
        );
        assert_eq!(
            BarsPeakGradientMode::all_wire_strs(),
            vec!["static", "cycle", "height", "match"],
        );
        assert_eq!(
            BarsPeakMode::all_wire_strs(),
            vec!["none", "fade", "fall", "fall_accel", "fall_fade"],
        );
        assert_eq!(
            LinesGradientMode::all_wire_strs(),
            vec!["breathing", "static", "position", "height", "gradient"],
        );
        assert_eq!(LinesStyle::all_wire_strs(), vec!["smooth", "angular"]);
        assert_eq!(
            VisualizerPlacement::all_wire_strs(),
            vec!["bottom_band", "over_cover"],
        );
    }

    /// A hand-written `nan` in `[visualizer]` must not survive `validate()`:
    /// NaN != NaN would make the change-gated shared-config apply
    /// (`snapshot() != settings.visualizer`) permanently true, re-initializing
    /// the FFT engine on every settings interaction. After validate, every
    /// float is finite and the config equals its own clone.
    #[test]
    fn validate_snaps_nan_floats_to_finite_values() {
        let cf: ConfigFile = toml::from_str(
            "[visualizer]\nnoise_reduction = nan\nopacity = nan\nmonstercat = nan\nheight_percent = nan\n\n[visualizer.bars]\nbar_width_min = nan\ntrails = nan\n\n[visualizer.lines]\nline_thickness = nan\n\n[visualizer.scope]\nradius = nan\necho = nan\n",
        )
        .expect("nan floats parse");
        let mut v = cf.visualizer;
        v.validate();

        assert!(v.noise_reduction.is_finite());
        assert!(v.opacity.is_finite());
        assert_eq!(v.monstercat, 0.0, "NaN monstercat snaps to off");
        assert!(v.height_percent.is_finite());
        assert!(v.bars.bar_width_min.is_finite());
        assert!(v.bars.trails.is_finite());
        assert!(v.lines.line_thickness.is_finite());
        assert!(v.scope.radius.is_finite());
        assert!(v.scope.echo.is_finite());

        // The load-bearing property: a validated config equals its own clone
        // (PartialEq is the change gate), so NaN can never wedge the gate.
        assert_eq!(v, v.clone(), "validated config must be PartialEq-reflexive");
    }

    /// M3-S3: a non-default `VisualizerConfig` round-trips through TOML with
    /// byte identity on re-serialization (the `[visualizer]` wire contract).
    #[test]
    fn visualizer_toml_section_roundtrips_byte_identity() {
        let mut cfg = VisualizerConfig::default();
        cfg.noise_reduction = 0.42;
        cfg.waves = true;
        cfg.monstercat = 0.0;
        cfg.bars.led_bars = true;
        cfg.bars.gradient_mode = BarsGradientMode::Static;
        cfg.lines.point_count = 256;
        cfg.scope.echo = 0.25;

        let first = toml::to_string(&cfg).expect("serialize VisualizerConfig");
        let parsed: VisualizerConfig = toml::from_str(&first).expect("parse VisualizerConfig");
        let second = toml::to_string(&parsed).expect("re-serialize VisualizerConfig");
        assert_eq!(first, second, "TOML round-trip must be byte-identical");
    }

    /// M3-S3: a config.toml with NO `[visualizer]` section parses to the
    /// default config (the `#[serde(default)]` on `ConfigFile.visualizer`),
    /// and unknown sub-tables (the color sub-tables the struct does not
    /// model) are ignored rather than rejected.
    #[test]
    fn visualizer_missing_section_fills_from_default() {
        let cf: ConfigFile =
            toml::from_str("[settings]\nstart_view = \"Queue\"\n").expect("parse ConfigFile");
        assert_eq!(
            toml::to_string(&cf.visualizer).expect("serialize"),
            toml::to_string(&VisualizerConfig::default()).expect("serialize default"),
            "missing [visualizer] must fill from Default"
        );

        // Color sub-tables (unmodeled) are ignored, not fatal.
        let with_colors: ConfigFile = toml::from_str(
            "[visualizer]\nnoise_reduction = 0.5\n\n[visualizer.bars.dark]\nborder_color = \"#1d2021\"\n",
        )
        .expect("parse ConfigFile with color sub-tables");
        assert_eq!(with_colors.visualizer.noise_reduction, 0.5);
    }

    /// M3-S8g: a sanitized REAL-shaped `[visualizer]` snapshot — color
    /// sub-tables the struct does not model, legacy top-level `trails`/`echo`
    /// scalars from the pre-per-mode era, and one intentionally typo'd enum
    /// value — parses without error, lands the expected values, and the typo
    /// falls back to the correct default (`deserialize_or_default` tolerance
    /// intact). Re-serialization round-trips byte-identically.
    #[test]
    fn current_user_visualizer_toml_snapshot_parses_and_reserializes() {
        let snapshot = r##"
[visualizer]
auto_sensitivity = true
noise_reduction = 0.77
waves = false
monstercat = 1.0
lower_cutoff_freq = 20
higher_cutoff_freq = 10000
height_percent = 0.4
opacity = 1.0
bloom = true
bloom_intensity = 0.6
beat_reactivity = 1.0
crt = 0.0
trails = 0.35
echo = 0.5

[visualizer.bars]
max_bars = 512
bar_width_min = 10
bar_width_max = 20
bar_spacing = 2
gradient_mode = "shimmer"
peak_mode = "fall_fade"

[visualizer.bars.dark]
border_color = "#1d2021"
bar_gradient_colors = ["#458588", "#83a598", "#689d6a"]
peak_gradient_colors = ["#fe8019", "#fabd2f"]

[visualizer.bars.light]
border_color = "#fbf1c7"

[visualizer.lines]
point_count = 256
style = "smooth"

[visualizer.scope]
point_count = 16
sensitivity = 1.5
line_thickness = 0.005
fill_opacity = 0.75
glow_intensity = 0.75
beam = true
particle_count = 512
echo = 0.25
"##;
        let cf: ConfigFile = toml::from_str(snapshot)
            .expect("real-shaped [visualizer] snapshot must parse without error");
        let v = cf.visualizer;

        // Modeled scalars land.
        assert_eq!(v.noise_reduction, 0.77);
        assert_eq!(v.bars.max_bars, 512);
        assert_eq!(
            v.bars.bar_width_min, 10.0,
            "integer TOML fills the f32 field"
        );
        assert_eq!(v.lines.point_count, 256);
        assert_eq!(v.scope.particle_count, 512);
        assert_eq!(v.scope.echo, 0.25);

        // The typo'd enum ("shimmer" was removed in b92d311) falls back to the
        // field default instead of rejecting the section.
        assert_eq!(
            v.bars.gradient_mode,
            BarsGradientMode::Wave,
            "unknown gradient_mode must fall back to the default"
        );
        assert_eq!(v.bars.peak_mode, BarsPeakMode::FallFade);

        // Unmodeled color sub-tables + legacy top-level trails/echo are
        // ignored, not fatal (they stay on disk — writes are surgical).

        // Byte-identical re-serialization of the modeled remainder.
        let first = toml::to_string(&v).expect("serialize");
        let parsed: VisualizerConfig = toml::from_str(&first).expect("re-parse");
        assert_eq!(first, toml::to_string(&parsed).expect("re-serialize"));
    }

    /// M3-S5: the `wire_enum!`-generated `from_wire_str` round-trips every
    /// variant of every visualizer enum through its own wire string, and
    /// unknown input falls back to `Default` (mirroring the
    /// `deserialize_or_default` serde tolerance).
    #[test]
    fn visualizer_enum_from_wire_str_roundtrips_every_variant() {
        for v in BarsGradientMode::ALL {
            assert_eq!(BarsGradientMode::from_wire_str(v.as_wire_str()), *v);
        }
        for v in BarsGradientOrientation::ALL {
            assert_eq!(BarsGradientOrientation::from_wire_str(v.as_wire_str()), *v);
        }
        for v in BarsPeakGradientMode::ALL {
            assert_eq!(BarsPeakGradientMode::from_wire_str(v.as_wire_str()), *v);
        }
        for v in BarsPeakMode::ALL {
            assert_eq!(BarsPeakMode::from_wire_str(v.as_wire_str()), *v);
        }
        for v in LinesGradientMode::ALL {
            assert_eq!(LinesGradientMode::from_wire_str(v.as_wire_str()), *v);
        }
        for v in LinesStyle::ALL {
            assert_eq!(LinesStyle::from_wire_str(v.as_wire_str()), *v);
        }
        for v in VisualizerPlacement::ALL {
            assert_eq!(VisualizerPlacement::from_wire_str(v.as_wire_str()), *v);
        }

        assert_eq!(
            BarsGradientMode::from_wire_str("garbage"),
            BarsGradientMode::default()
        );
        assert_eq!(
            LinesGradientMode::from_wire_str(""),
            LinesGradientMode::default()
        );
        assert_eq!(
            VisualizerPlacement::from_wire_str("nowhere"),
            VisualizerPlacement::default()
        );
    }

    /// M3-S5: the serde wire string equals `as_wire_str` for every variant of
    /// every visualizer enum — they cannot drift because `wire_enum!` ties
    /// both to the same explicit literal, and this pins that construction.
    #[test]
    fn visualizer_enum_wire_str_matches_serde_rename() {
        fn check<T: Serialize + Copy>(all: &[T], as_wire: impl Fn(&T) -> &'static str) {
            for v in all {
                let json = serde_json::to_string(v).expect("serialize enum variant");
                assert_eq!(json.trim_matches('"'), as_wire(v));
            }
        }
        check(BarsGradientMode::ALL, |v| v.as_wire_str());
        check(BarsGradientOrientation::ALL, |v| v.as_wire_str());
        check(BarsPeakGradientMode::ALL, |v| v.as_wire_str());
        check(BarsPeakMode::ALL, |v| v.as_wire_str());
        check(LinesGradientMode::ALL, |v| v.as_wire_str());
        check(LinesStyle::ALL, |v| v.as_wire_str());
        check(VisualizerPlacement::ALL, |v| v.as_wire_str());
    }
}
