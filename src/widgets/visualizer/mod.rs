//! Audio Visualizer Component
//!
//! Modular audio visualizer supporting multiple visualization modes.

mod pipeline;
pub(crate) mod shader;
mod state;

use iced::{Color, Element, Length};
pub(crate) use shader::{ShaderParams, ShaderVisualizer};
pub(crate) use state::{SharedVisualizerConfig, VisualizerState};

/// Visualization mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualizationMode {
    Bars,
    Lines,
}

/// Default bar width in pixels (fallback when dynamic calculation fails)
const BAR_WIDTH: f32 = 4.0;

/// Minimum bar count for display (if fewer bars fit, return 0 to skip rendering)
const MIN_BAR_COUNT: usize = 4;

/// Calculate dynamic bar width based on window width
///
/// This creates a responsive visualizer that adapts to window size:
/// - Small windows (400px) → thin bars (bar_width_min) for dense, detailed look
/// - Large windows (2560px) → thicker bars (bar_width_max) for bold, chunky look
///
/// The formula uses linear interpolation with defined breakpoints.
fn calculate_dynamic_bar_width(window_width: f32, min_bar_width: f32, max_bar_width: f32) -> f32 {
    // Breakpoints for scaling
    const MIN_WINDOW_WIDTH: f32 = 400.0; // Small window threshold
    const MAX_WINDOW_WIDTH: f32 = 2560.0; // Large window threshold (4K width)

    // Clamp window width to our range
    let clamped_width = window_width.clamp(MIN_WINDOW_WIDTH, MAX_WINDOW_WIDTH);

    // Linear interpolation: map window width to bar width
    let t = (clamped_width - MIN_WINDOW_WIDTH) / (MAX_WINDOW_WIDTH - MIN_WINDOW_WIDTH);
    let bar_width = min_bar_width + t * (max_bar_width - min_bar_width);

    // Round to nearest integer pixel (bar widths should be whole pixels)
    bar_width.round()
}

/// Calculate optimal bar count and bar width for a given canvas width.
///
/// Direct O(1) calculation — no iteration or culling needed:
/// 1. Compute how many bars fit at the desired width (from dynamic scaling)
/// 2. Clamp to min(max_bars)
/// 3. Use floor(width) so bars never exceed the canvas
/// 4. Distribute any remainder evenly to both edges for centering
///
/// Note: FFT limits are handled separately in state.rs during engine init.
/// If visual bar count exceeds FFT bins, state.rs interpolates the FFT output.
///
/// Returns (bar_count, bar_width, edge_spacing)
fn calculate_bar_layout(
    canvas_width: f32,
    desired_bar_width: f32,
    bar_spacing: f32,
    border_width: f32,
    max_bars: usize,
) -> (usize, f32, f32) {
    if canvas_width <= 0.0 {
        return (0, 0.0, bar_spacing);
    }

    let base_edge = bar_spacing;
    let gap_between_borders = if border_width > 0.0 {
        border_width
    } else {
        0.0
    };
    let spacing_per_bar = bar_spacing + gap_between_borders;

    // How many bars fit at the desired width?
    // Total = N * bar_width + (N-1) * spacing + 2 * edge
    //       = N * (bar_width + spacing) - spacing + 2 * edge
    // Solving for N:
    //   N = (canvas_width - 2*edge + spacing) / (bar_width + spacing)
    let bar_width = desired_bar_width.max(1.0);
    let natural_count = ((canvas_width - 2.0 * base_edge + spacing_per_bar)
        / (bar_width + spacing_per_bar))
        .floor() as usize;

    // Clamp to [MIN_BAR_COUNT, max_bars]
    let bar_count = natural_count.clamp(MIN_BAR_COUNT, max_bars);
    let n = bar_count as f32;

    // Recalculate the actual bar width for this count (may differ from desired if clamped)
    let total_spacing = if bar_count > 1 {
        (bar_count - 1) as f32 * spacing_per_bar
    } else {
        0.0
    };
    let available = canvas_width - total_spacing - 2.0 * base_edge;

    // When there is no spacing at all (bar_spacing=0 AND border_width=0), use the exact
    // fractional bar width so bars tile perfectly across the full canvas with no gaps.
    // Otherwise, use floor() to guarantee bars never exceed canvas width, distributing
    // the fractional remainder evenly to both edges for centering.
    let no_spacing = bar_spacing < 0.001 && border_width < 0.001;
    let actual_bar_width = if no_spacing {
        (available / n).max(1.0)
    } else {
        (available / n).floor().max(1.0)
    };

    let remainder = available - n * actual_bar_width;
    let edge_spacing = if no_spacing {
        0.0
    } else {
        base_edge + remainder / 2.0
    };

    (bar_count, actual_bar_width, edge_spacing)
}

/// Audio visualizer widget
#[derive(Clone)]
pub struct Visualizer {
    state: VisualizerState,
    config: SharedVisualizerConfig, // Shared config for hot-reload
    mode: VisualizationMode,
    bar_count: usize,
    point_count: usize,
    window_height: f32, // For scaling line thickness
    window_width: f32,  // For dynamic bar count calculation
    // Bars mode config
    bar_width: f32,
    bar_spacing: f32,
    edge_spacing: f32, // Edge spacing for centering bars
    border_width: f32,
    peak_enabled: bool,
    peak_alpha: f32,
    peak_color: Color,
    // Lines mode config
    line_thickness: f32,
    // Dynamic bars
    dynamic_bars: bool,
    max_bars: usize, // Maximum bar count to try when calculating
}

impl Visualizer {
    /// Create a new visualizer with bars mode
    pub fn new(bar_count: usize, config: SharedVisualizerConfig) -> Self {
        let state = VisualizerState::new(bar_count, config.clone());

        // Read initial settings from config
        let cfg = config.read();
        let (bar_spacing, border_width, point_count, line_thickness, max_bars) = (
            cfg.bars.bar_spacing,
            cfg.bars.border_width,
            cfg.lines.point_count,
            cfg.lines.line_thickness,
            cfg.bars.max_bars,
        );
        drop(cfg);

        Self {
            state,
            config,
            mode: VisualizationMode::Bars,
            bar_count: 192,       // Default for bars mode
            point_count,          // From config
            window_height: 800.0, // Default window height
            window_width: 1200.0, // Default window width
            // Bars mode config
            bar_width: BAR_WIDTH, // Fixed minimum bar width
            bar_spacing,
            edge_spacing: bar_spacing, // Edge spacing = bar spacing
            max_bars,                  // From config (default 256)
            border_width,              // From config
            peak_enabled: true,
            peak_alpha: 1.0,
            peak_color: crate::theme::yellow_bright(),
            // Lines mode config
            line_thickness, // From config
            // Dynamic bars enabled
            dynamic_bars: true,
        }
    }

    /// Set window height for scaling
    pub fn window_height(mut self, height: f32) -> Self {
        self.window_height = height;
        self
    }

    /// Set window width and recalculate bar count/width if dynamic bars enabled
    /// Bar width is dynamically calculated based on window size for optimal aesthetics
    /// Bar spacing is still read from config for hot-reload support
    pub fn width(mut self, width: f32) -> Self {
        self.window_width = width;

        if self.dynamic_bars && self.mode == VisualizationMode::Bars {
            // Read bar width min/max and spacing from shared config for hot-reload support
            let cfg = self.config.read();
            let (
                bar_width_min,
                bar_width_max,
                config_bar_spacing,
                config_max_bars,
                config_border_width,
            ) = (
                cfg.bars.bar_width_min,
                cfg.bars.bar_width_max,
                cfg.bars.bar_spacing,
                cfg.bars.max_bars,
                cfg.bars.border_width,
            );
            drop(cfg);

            // Calculate dynamic bar width based on window size and config limits
            let dynamic_bar_width =
                calculate_dynamic_bar_width(width, bar_width_min, bar_width_max);

            // Update local fields from config
            self.bar_spacing = config_bar_spacing;
            self.max_bars = config_max_bars;
            self.border_width = config_border_width;

            // Calculate optimal bar count, width, and edge spacing to fill the screen
            let (new_bar_count, calculated_bar_width, edge_spacing) = calculate_bar_layout(
                width,
                dynamic_bar_width, // dynamically calculated based on window size
                config_bar_spacing,
                self.border_width,
                self.max_bars,
            );

            // Compare against target bar count (includes pending resize)
            // This prevents redundant resize calls while debouncing is active
            let target = self.state.target_bar_count();
            if new_bar_count != target && new_bar_count > 0 {
                // Resize the state - this queues a debounced engine reinitialization
                self.state.resize(new_bar_count);
            }

            // Update bar count, width, and edge spacing for rendering
            self.bar_count = new_bar_count;
            self.bar_width = calculated_bar_width;
            self.edge_spacing = edge_spacing;
        }

        self
    }

    /// Set visualization mode and resize the spectrum engine for the new mode's count
    /// Also reads lines config from shared config for hot-reload
    ///
    /// NOTE: For Bars mode, we do NOT resize here - width() handles that since
    /// bar count is dynamically calculated based on window size.
    pub fn mode(mut self, mode: VisualizationMode) -> Self {
        self.mode = mode;
        self.state.set_lines_mode(mode == VisualizationMode::Lines);

        // Only Lines mode resizes here - Bars mode lets width() handle it
        if mode == VisualizationMode::Lines {
            let cfg = self.config.read();
            let (config_point_count, config_line_thickness) =
                (cfg.lines.point_count, cfg.lines.line_thickness);
            drop(cfg);

            tracing::trace!(
                "📊 Lines mode: read config point_count={}, thickness={}",
                config_point_count,
                config_line_thickness
            );
            self.point_count = config_point_count;
            self.line_thickness = config_line_thickness;

            // Get current and pending counts from state
            let current_count = self.state.bar_count();
            let pending_count = self.state.target_bar_count();

            // Only resize if target differs from both current AND pending
            if self.point_count != current_count
                && self.point_count != pending_count
                && self.point_count > 0
            {
                tracing::debug!(
                    "📊 Resizing visualizer from {} to {} for Lines mode",
                    current_count,
                    self.point_count
                );
                self.state.resize(self.point_count);
            }
        }

        self
    }

    /// Set bar width (bars mode)
    pub fn bar_width(mut self, width: f32) -> Self {
        self.bar_width = width;
        self
    }

    /// Set bar spacing (bars mode)
    pub fn bar_spacing(mut self, spacing: f32) -> Self {
        self.bar_spacing = spacing;
        self
    }

    /// Set maximum bar count to try when calculating dynamic bars
    pub fn max_bars(mut self, count: usize) -> Self {
        self.max_bars = count;
        self
    }

    /// Set line thickness (lines mode)
    pub fn line_thickness(mut self, thickness: f32) -> Self {
        self.line_thickness = thickness;
        self
    }

    /// Set border width (bars mode)
    pub fn border_width(mut self, width: f32) -> Self {
        self.border_width = width;
        self
    }

    /// Enable or disable peak bars (bars mode)
    pub fn peak_enabled(mut self, enabled: bool) -> Self {
        self.peak_enabled = enabled;
        self
    }

    /// Decay peaks (call periodically for animation)
    pub fn decay_peaks(&self, delta_time: std::time::Duration) {
        self.state.decay_peaks(delta_time);
    }

    /// Get audio callback for connecting to audio engine.
    /// Returns a closure accepting `&[f32]` samples directly from the streaming source.
    pub fn audio_callback(&self) -> impl Fn(&[f32], u32) + Send + Sync + use<> {
        self.state.audio_callback()
    }

    /// Reset the visualizer state for a new track
    /// This reinitializes the spectrum engine to reset autosensitivity calibration, preventing
    /// the 2-4 second pause when manually switching to a track with different loudness.
    pub fn reset(&self) {
        self.state.reset();
    }

    /// Apply config changes (hot-reload support)
    /// Reinitializes the spectrum engine with updated parameters from config
    pub fn apply_config(&self) {
        self.state.apply_config();
    }

    /// Get callback for clearing sample buffer on track changes
    ///
    /// Clears the raw sample buffer used by the visualizer.
    /// When tracks change, we only clear the sample buffer to prevent the visualizer from
    /// processing stale audio from the previous track.
    pub fn clear_buffer_callback(&self) -> impl Fn() + Send + Sync + 'static {
        let state = self.state.clone();
        move || {
            state.clear_sample_buffer();
        }
    }

    /// Convert to widget element based on mode
    /// Uses GPU shader widget for hardware-accelerated rendering
    pub fn view<'a, Message: 'a>(&self) -> Element<'a, Message> {
        use iced::widget::shader;

        // Read behavior config from shared config (hot-reload from config.toml)
        // Colors now come from the theme system (not config.toml)
        let cfg = self.config.read();
        let colors: crate::visualizer_config::ThemeBarColors =
            crate::theme::get_visualizer_colors().into();
        let (
            border_width,
            led_bars,
            led_segment_height,
            led_border_opacity,
            border_opacity,
            gradient_colors,
            peak_gradient_colors,
            gradient_mode,
            gradient_orientation,
            peak_gradient_mode,
            peak_mode,
            peak_hold_time,
            peak_fade_time,
            border_color,
            bar_depth_3d,
            peak_height_ratio,
        ) = (
            cfg.bars.border_width,
            cfg.bars.led_bars,
            cfg.bars.led_segment_height,
            colors.led_border_opacity,
            colors.border_opacity,
            colors.get_bar_gradient_colors(),
            colors.get_peak_gradient_colors(),
            cfg.bars.get_gradient_mode_value(), // 0=static, 2=wave, 3=shimmer, 4=energy
            cfg.bars.get_gradient_orientation_value(), // 0=vertical, 1=horizontal
            cfg.bars.get_peak_gradient_mode_value(), // 0=static, 1=cycle, 2=height, 3=match
            cfg.bars.get_peak_mode_value(),     // 0=none, 1=fade, 2=fall, 3=fall_accel
            cfg.bars.peak_hold_time as f32 / 1000.0, // Convert ms to seconds
            cfg.bars.peak_fade_time as f32 / 1000.0, // Convert ms to seconds
            colors.get_border_color(),
            cfg.bars.bar_depth_3d,
            cfg.bars.peak_height_ratio as f32 / 100.0, // Convert percentage to ratio
        );
        let global_opacity = cfg.opacity;
        let (lines_outline_thickness, lines_outline_opacity, lines_animation_speed) = (
            cfg.lines.outline_thickness,
            cfg.lines.outline_opacity,
            cfg.lines.animation_speed,
        );
        let lines_gradient_mode = cfg.lines.get_gradient_mode_value();
        let lines_fill_opacity = cfg.lines.fill_opacity;
        let lines_mirror = cfg.lines.mirror;
        let lines_style = cfg.lines.get_style_value();
        drop(cfg);

        // Create shader-based visualizer (GPU accelerated)
        let params = ShaderParams {
            gradient_colors,
            peak_gradient_colors,
            border_color,
            border_width,
            peak_enabled: self.peak_enabled,
            peak_thickness: peak_height_ratio,
            peak_alpha: self.peak_alpha,
            peak_color: self.peak_color,
            line_thickness: self.line_thickness,
            bar_width: self.bar_width,
            bar_spacing: self.bar_spacing,
            edge_spacing: self.edge_spacing,
            led_bars,
            led_segment_height,
            led_border_opacity,
            border_opacity,
            gradient_mode,
            gradient_orientation,
            peak_gradient_mode,
            peak_mode,
            peak_hold_time,
            peak_fade_time,
            bar_depth_3d,
            global_opacity,
            lines_outline_thickness,
            lines_outline_opacity,
            lines_animation_speed,
            lines_gradient_mode,
            lines_fill_opacity,
            lines_mirror,
            lines_glow_intensity: 0.0,
            lines_style,
        };

        let shader_viz = ShaderVisualizer::new(self.state.clone(), self.mode, params);

        shader(shader_viz)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

/// Helper function to create a visualizer
pub(crate) fn visualizer(bar_count: usize, config: SharedVisualizerConfig) -> Visualizer {
    Visualizer::new(bar_count, config)
}
