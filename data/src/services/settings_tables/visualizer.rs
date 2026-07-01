//! Visualizer-tab settings table.
//!
//! Owns every scalar/enum `visualizer.*` key (see the `keys` module in
//! `types/visualizer_config.rs`). Unlike the other tabs, setters mutate the
//! manager's IN-MEMORY `visualizer` field (config.toml `[visualizer]` is the
//! sole persistent store — never redb, so there is no `save()`), and the
//! `toml_apply`/`read`/`write` closures are no-ops: `LivePlayerSettings`
//! receives the config wholesale in `get_player_settings`, and the
//! `[visualizer]` section is read wholesale in startup phase 1 / reload while
//! writes stay surgical per-key in the UI's `handle_settings_write_config`
//! (preserving the color sub-tables the struct does not model).
//!
//! Enum entries dispatch via `from_wire_str` — visualizer dropdowns key on
//! WIRE strings (`"fall_fade"`), intentionally unlike the `from_label` tabs,
//! so the items builder and dispatch agree on the same literal.
//!
//! On the repeated `|_ts, _p| {}` no-op closures (review #9, WON'T-FIX):
//! `define_settings!` REQUIRES toml_apply/read/write on every entry — that
//! mandatory-ness is the compile-time guarantee that a General/Interface/
//! Playback entry can never silently omit a wire copy (the bug class this
//! whole refactor retires). Making them optional to save these ~3 lines per
//! entry would reopen that hole for the three real tabs. The emitted
//! apply/dump/write functions for THIS tab are intentionally-empty and not
//! re-exported; the orchestrators must never call them.
//!
//! Value-type note: several `f32` config fields (`bar_width_*`, `bar_spacing`,
//! `border_width`, `led_segment_height`, `bar_depth_3d`) surface as INT rows
//! (whole pixels) — their entries take `Int` and cast `v as f32`, matching
//! what the GUI emits. `every_visualizer_macro_row_dispatches` pins the
//! row-value ↔ dispatch-arm agreement for every entry.
//!
//! `ui_meta.default` expressions read `VisualizerConfig::default()` directly,
//! so the row defaults can never drift from the config defaults.

use crate::{
    define_settings,
    types::{
        settings_data::VisualizerSettingsData,
        visualizer_config::{
            BarsGradientMode, BarsGradientOrientation, BarsPeakGradientMode, BarsPeakMode,
            LinesGradientMode, LinesStyle, VisualizerPlacement,
        },
    },
};

define_settings! {
    tab: crate::types::setting_def::Tab::Visualizer,
    data_type: VisualizerSettingsData,
    mgr_type: crate::services::settings::SettingsManager,
    items_fn: build_visualizer_tab_settings_items,
    settings_const: TAB_VISUALIZER_SETTINGS,
    contains_fn: tab_visualizer_contains,
    dispatch_fn: dispatch_visualizer_tab_setting,
    apply_fn: apply_toml_visualizer_tab,
    dump_fn: dump_visualizer_tab_player_settings,
    write_fn: write_visualizer_tab_toml,
    settings: [
        // -- Frame ---------------------------------------------------------------
        VisHeightPercent {
            key: "visualizer.height_percent",
            value_type: Float,
            setter: |mgr, v: f64| mgr.with_visualizer(|vz| vz.height_percent = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Visualizer Height",
                category: "Frame",
                subtitle: Some("Bars/Lines height: % of the bottom band's window height, or of the cover when placed over it. 10–60%"),
                default: f64::from(crate::types::visualizer_config::VisualizerConfig::default().height_percent),
                min: 0.1,
                max: 0.60,
                step: 0.05, unit: "%",
                read_field: |d| d.height_percent,
            },
        },
        VisOpacity {
            key: "visualizer.opacity",
            value_type: Float,
            setter: |mgr, v: f64| mgr.with_visualizer(|vz| vz.opacity = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Visualizer Opacity",
                category: "Frame",
                subtitle: Some("0.0 = invisible, 1.0 = fully opaque"),
                default: f64::from(crate::types::visualizer_config::VisualizerConfig::default().opacity),
                min: 0.0,
                max: 1.0,
                step: 0.05, unit: "",
                read_field: |d| d.opacity,
            },
        },
        VisBloom {
            key: "visualizer.bloom",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.with_visualizer(|vz| vz.bloom = v),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Bloom Glow",
                category: "Frame",
                subtitle: Some("Soft glow halo around bright bars, peak flashes, and the line"),
                default: crate::types::visualizer_config::VisualizerConfig::default().bloom,
                read_field: |d| d.bloom,
            },
        },
        VisBloomIntensity {
            key: "visualizer.bloom_intensity",
            value_type: Float,
            setter: |mgr, v: f64| mgr.with_visualizer(|vz| vz.bloom_intensity = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Bloom Intensity",
                category: "Frame",
                subtitle: Some("Strength of the bloom glow. 0 = off"),
                default: f64::from(crate::types::visualizer_config::VisualizerConfig::default().bloom_intensity),
                min: 0.0,
                max: 1.0,
                step: 0.05, unit: "",
                read_field: |d| d.bloom_intensity,
            },
        },
        VisBeatReactivity {
            key: "visualizer.beat_reactivity",
            value_type: Float,
            setter: |mgr, v: f64| mgr.with_visualizer(|vz| vz.beat_reactivity = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Beat Reactivity",
                category: "Frame",
                subtitle: Some("Pulses the bloom, glow, and bars with the beat + bass drops.\n1 = punches on every kick, 0 = steady (tracks loudness only)"),
                default: f64::from(crate::types::visualizer_config::VisualizerConfig::default().beat_reactivity),
                min: 0.0,
                max: 1.0,
                step: 0.05, unit: "",
                read_field: |d| d.beat_reactivity,
            },
        },
        VisCrt {
            key: "visualizer.crt",
            value_type: Float,
            setter: |mgr, v: f64| mgr.with_visualizer(|vz| vz.crt = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "CRT / Film",
                category: "Frame",
                subtitle: Some("Retro post-process: chromatic aberration, scanlines, vignette, grain, beat zoom. 0 = off"),
                default: f64::from(crate::types::visualizer_config::VisualizerConfig::default().crt),
                min: 0.0,
                max: 1.0,
                step: 0.05, unit: "",
                read_field: |d| d.crt,
            },
        },
        // -- Signal --------------------------------------------------------------
        VisNoiseReduction {
            key: "visualizer.noise_reduction",
            value_type: Float,
            setter: |mgr, v: f64| mgr.with_visualizer(|vz| vz.noise_reduction = v),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Noise Reduction",
                category: "Signal",
                subtitle: Some("0.0 = raw FFT, 1.0 = fully smoothed"),
                default: crate::types::visualizer_config::VisualizerConfig::default().noise_reduction,
                min: 0.0,
                max: 1.0,
                step: 0.01, unit: "",
                read_field: |d| d.noise_reduction,
            },
        },
        VisLowerCutoffFreq {
            key: "visualizer.lower_cutoff_freq",
            value_type: Int,
            setter: |mgr, v: i64| mgr.with_visualizer(|vz| vz.lower_cutoff_freq = v as u32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Lower Cutoff Freq",
                category: "Signal",
                subtitle: Some("Frequencies below this are hidden"),
                default: crate::types::visualizer_config::VisualizerConfig::default().lower_cutoff_freq as i64,
                min: 20,
                max: 1000,
                step: 10, unit: " Hz",
                read_field: |d| d.lower_cutoff_freq,
            },
        },
        VisHigherCutoffFreq {
            key: "visualizer.higher_cutoff_freq",
            value_type: Int,
            setter: |mgr, v: i64| mgr.with_visualizer(|vz| vz.higher_cutoff_freq = v as u32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Upper Cutoff Freq",
                category: "Signal",
                subtitle: Some("Frequencies above this are hidden"),
                default: crate::types::visualizer_config::VisualizerConfig::default().higher_cutoff_freq as i64,
                min: 1000,
                max: 22050,
                step: 100, unit: " Hz",
                read_field: |d| d.higher_cutoff_freq,
            },
        },
        VisAutoSensitivity {
            key: "visualizer.auto_sensitivity",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.with_visualizer(|vz| vz.auto_sensitivity = v),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Auto Sensitivity",
                category: "Signal",
                subtitle: Some("Scales output to always fill full height"),
                default: crate::types::visualizer_config::VisualizerConfig::default().auto_sensitivity,
                read_field: |d| d.auto_sensitivity,
            },
        },
        // -- Bars ----------------------------------------------------------------
        BarsPlacement {
            key: "visualizer.bars.placement",
            value_type: Enum,
            setter: |mgr, v: String| mgr.with_visualizer(|vz| vz.bars.placement = VisualizerPlacement::from_wire_str(&v)),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Placement",
                category: "Bars",
                subtitle: Some("Where the Bars visualizer is drawn\nbottom_band: a band above the player bar (every view)\nover_cover: on the now-playing cover art (Queue view, while playing)"),
                default: crate::types::visualizer_config::VisualizerConfig::default().bars.placement.as_wire_str(),
                options: VisualizerPlacement::all_wire_strs(),
                read_field: |d| d.bars_placement.as_ref(),
            },
        },
        // Mutually exclusive with monstercat: enabling waves zeroes it.
        // The UI write handler mirrors this with a dual config.toml write
        // (config.toml wins on reload — both keys must persist).
        VisWaves {
            key: "visualizer.waves",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.with_visualizer(|vz| {
                vz.waves = v;
                if v {
                    vz.monstercat = 0.0;
                }
            }),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Waves Smoothing",
                category: "Bars",
                subtitle: Some("Catmull-Rom spline smoothing creates smooth rolling hills. Mutually exclusive with Monstercat"),
                default: crate::types::visualizer_config::VisualizerConfig::default().waves,
                read_field: |d| d.waves,
            },
        },
        VisWavesSmoothing {
            key: "visualizer.waves_smoothing",
            value_type: Int,
            setter: |mgr, v: i64| mgr.with_visualizer(|vz| vz.waves_smoothing = v as u32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Waves Intensity",
                category: "Bars",
                subtitle: Some("Control point spacing for waves spline. Higher = smoother (fewer control points)"),
                default: crate::types::visualizer_config::VisualizerConfig::default().waves_smoothing as i64,
                min: 2,
                max: 16,
                step: 1, unit: "",
                read_field: |d| d.waves_smoothing,
            },
        },
        // Mutually exclusive with waves: an EFFECTIVE monstercat (>= the
        // minimum; sub-threshold values snap to 0.0 in validate) disables
        // waves. The UI write handler mirrors this with a dual
        // config.toml write.
        VisMonstercat {
            key: "visualizer.monstercat",
            value_type: Float,
            setter: |mgr, v: f64| mgr.with_visualizer(|vz| {
                vz.monstercat = v;
                if v >= crate::types::visualizer_config::MONSTERCAT_MIN_EFFECTIVE {
                    vz.waves = false;
                }
            }),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Monstercat Smoothing",
                category: "Bars",
                subtitle: Some("Sharp triangular peaks with exponential falloff. Higher = wider spread. Mutually exclusive with Waves"),
                default: crate::types::visualizer_config::VisualizerConfig::default().monstercat,
                min: 0.0,
                max: 10.0,
                step: 0.1, unit: "",
                read_field: |d| d.monstercat,
            },
        },
        BarsMaxBars {
            key: "visualizer.bars.max_bars",
            value_type: Int,
            setter: |mgr, v: i64| mgr.with_visualizer(|vz| vz.bars.max_bars = v as usize),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Max Bar Count",
                category: "Bars",
                subtitle: Some("Maximum number of bars to fit in the window"),
                default: crate::types::visualizer_config::VisualizerConfig::default().bars.max_bars as i64,
                min: 16,
                max: 2048,
                step: 8, unit: "",
                read_field: |d| d.bars_max_bars,
            },
        },
        BarsBarWidthMin {
            key: "visualizer.bars.bar_width_min",
            value_type: Int,
            setter: |mgr, v: i64| mgr.with_visualizer(|vz| vz.bars.bar_width_min = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Bar Width Min",
                category: "Bars",
                subtitle: Some("Bar width at smallest window size"),
                default: crate::types::visualizer_config::VisualizerConfig::default().bars.bar_width_min as i64,
                min: 1,
                max: 10,
                step: 1, unit: " px",
                read_field: |d| d.bars_bar_width_min,
            },
        },
        BarsBarWidthMax {
            key: "visualizer.bars.bar_width_max",
            value_type: Int,
            setter: |mgr, v: i64| mgr.with_visualizer(|vz| vz.bars.bar_width_max = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Bar Width Max",
                category: "Bars",
                subtitle: Some("Bar width at largest window size"),
                default: crate::types::visualizer_config::VisualizerConfig::default().bars.bar_width_max as i64,
                min: 2,
                max: 20,
                step: 1, unit: " px",
                read_field: |d| d.bars_bar_width_max,
            },
        },
        BarsBarSpacing {
            key: "visualizer.bars.bar_spacing",
            value_type: Int,
            setter: |mgr, v: i64| mgr.with_visualizer(|vz| vz.bars.bar_spacing = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Bar Spacing",
                category: "Bars",
                subtitle: Some("Gap between bars in pixels"),
                default: crate::types::visualizer_config::VisualizerConfig::default().bars.bar_spacing as i64,
                min: 0,
                max: 10,
                step: 1, unit: " px",
                read_field: |d| d.bars_bar_spacing,
            },
        },
        BarsBorderWidth {
            key: "visualizer.bars.border_width",
            value_type: Int,
            setter: |mgr, v: i64| mgr.with_visualizer(|vz| vz.bars.border_width = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Border Width",
                category: "Bars",
                subtitle: Some("Outline around each bar; also sets LED gap size"),
                default: crate::types::visualizer_config::VisualizerConfig::default().bars.border_width as i64,
                min: 0,
                max: 5,
                step: 1, unit: " px",
                read_field: |d| d.bars_border_width,
            },
        },
        BarsLedBars {
            key: "visualizer.bars.led_bars",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.with_visualizer(|vz| vz.bars.led_bars = v),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "LED Mode",
                category: "Bars",
                subtitle: Some("Render bars as stacked LED segments like a VU meter"),
                default: crate::types::visualizer_config::VisualizerConfig::default().bars.led_bars,
                read_field: |d| d.bars_led_bars,
            },
        },
        BarsLedSegmentHeight {
            key: "visualizer.bars.led_segment_height",
            value_type: Int,
            setter: |mgr, v: i64| mgr.with_visualizer(|vz| vz.bars.led_segment_height = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "LED Segment Height",
                category: "Bars",
                subtitle: Some("Height of each LED segment in pixels"),
                default: crate::types::visualizer_config::VisualizerConfig::default().bars.led_segment_height as i64,
                min: 2,
                max: 20,
                step: 1, unit: " px",
                read_field: |d| d.bars_led_segment_height,
            },
        },
        BarsGradientMode {
            key: "visualizer.bars.gradient_mode",
            value_type: Enum,
            setter: |mgr, v: String| mgr.with_visualizer(|vz| vz.bars.gradient_mode = BarsGradientMode::from_wire_str(&v)),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Gradient Mode",
                category: "Bars",
                subtitle: Some("static: height-based gradient (bottom to top)\nwave: gradient stretching (taller bars show more bottom colors)"),
                default: crate::types::visualizer_config::VisualizerConfig::default().bars.gradient_mode.as_wire_str(),
                options: BarsGradientMode::all_wire_strs(),
                read_field: |d| d.bars_gradient_mode.as_ref(),
            },
        },
        BarsGradientOrientation {
            key: "visualizer.bars.gradient_orientation",
            value_type: Enum,
            setter: |mgr, v: String| mgr.with_visualizer(|vz| vz.bars.gradient_orientation = BarsGradientOrientation::from_wire_str(&v)),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Gradient Orientation",
                category: "Bars",
                subtitle: Some("Axis the gradient colors are mapped along\nvertical: colors map bottom-to-top within each bar\nhorizontal: colors map left-to-right across bars (bass to treble)"),
                default: crate::types::visualizer_config::VisualizerConfig::default().bars.gradient_orientation.as_wire_str(),
                options: BarsGradientOrientation::all_wire_strs(),
                read_field: |d| d.bars_gradient_orientation.as_ref(),
            },
        },
        BarsPeakGradientMode {
            key: "visualizer.bars.peak_gradient_mode",
            value_type: Enum,
            setter: |mgr, v: String| mgr.with_visualizer(|vz| vz.bars.peak_gradient_mode = BarsPeakGradientMode::from_wire_str(&v)),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Peak Gradient Mode",
                category: "Bars",
                subtitle: Some("Color mode for peak indicators\nstatic: uses first color in peak gradient only\ncycle: time-based animation cycling through all peak colors\nheight: color based on peak height position\nmatch: uses same color as bar gradient at that height"),
                default: crate::types::visualizer_config::VisualizerConfig::default().bars.peak_gradient_mode.as_wire_str(),
                options: BarsPeakGradientMode::all_wire_strs(),
                read_field: |d| d.bars_peak_gradient_mode.as_ref(),
            },
        },
        BarsPeakMode {
            key: "visualizer.bars.peak_mode",
            value_type: Enum,
            setter: |mgr, v: String| mgr.with_visualizer(|vz| vz.bars.peak_mode = BarsPeakMode::from_wire_str(&v)),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Peak Mode",
                category: "Bars",
                subtitle: Some("none: peak bars disabled\nfade: hold, then fade out in place (opacity decreases)\nfall: hold, then fall at constant speed\nfall_accel: hold, then fall with gravity acceleration\nfall_fade: hold, then fall at constant speed while fading out"),
                default: crate::types::visualizer_config::VisualizerConfig::default().bars.peak_mode.as_wire_str(),
                options: BarsPeakMode::all_wire_strs(),
                read_field: |d| d.bars_peak_mode.as_ref(),
            },
        },
        BarsPeakHoldTime {
            key: "visualizer.bars.peak_hold_time",
            value_type: Int,
            setter: |mgr, v: i64| mgr.with_visualizer(|vz| vz.bars.peak_hold_time = v as u32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Peak Hold Time",
                category: "Bars",
                subtitle: Some("How long peaks stay before falling/fading"),
                default: crate::types::visualizer_config::VisualizerConfig::default().bars.peak_hold_time as i64,
                min: 0,
                max: 5000,
                step: 50, unit: " ms",
                read_field: |d| d.bars_peak_hold_time,
            },
        },
        BarsPeakFadeTime {
            key: "visualizer.bars.peak_fade_time",
            value_type: Int,
            setter: |mgr, v: i64| mgr.with_visualizer(|vz| vz.bars.peak_fade_time = v as u32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Peak Fade Time",
                category: "Bars",
                subtitle: Some("Duration of fade-out in 'fade' mode"),
                default: crate::types::visualizer_config::VisualizerConfig::default().bars.peak_fade_time as i64,
                min: 0,
                max: 5000,
                step: 50, unit: " ms",
                read_field: |d| d.bars_peak_fade_time,
            },
        },
        BarsPeakFallSpeed {
            key: "visualizer.bars.peak_fall_speed",
            value_type: Int,
            setter: |mgr, v: i64| mgr.with_visualizer(|vz| vz.bars.peak_fall_speed = v as u32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Peak Fall Speed",
                category: "Bars",
                subtitle: Some("How fast peaks drop in fall/fall_accel modes. 1 = slow, 20 = fast. No effect in fade mode"),
                default: crate::types::visualizer_config::VisualizerConfig::default().bars.peak_fall_speed as i64,
                min: 1,
                max: 20,
                step: 1, unit: "",
                read_field: |d| d.bars_peak_fall_speed,
            },
        },
        BarsPeakHeightRatio {
            key: "visualizer.bars.peak_height_ratio",
            value_type: Int,
            setter: |mgr, v: i64| mgr.with_visualizer(|vz| vz.bars.peak_height_ratio = v as u32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Peak Height",
                category: "Bars",
                subtitle: Some("Peak bar size as % of bar width (ignored in LED mode — peaks are one segment tall)"),
                default: crate::types::visualizer_config::VisualizerConfig::default().bars.peak_height_ratio as i64,
                min: 10,
                max: 100,
                step: 5, unit: "%",
                read_field: |d| d.bars_peak_height_ratio,
            },
        },
        BarsBarDepth3d {
            key: "visualizer.bars.bar_depth_3d",
            value_type: Int,
            setter: |mgr, v: i64| mgr.with_visualizer(|vz| vz.bars.bar_depth_3d = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Isometric Depth",
                category: "Bars",
                subtitle: Some("3D top and side face depth in pixels, 0 = flat"),
                default: crate::types::visualizer_config::VisualizerConfig::default().bars.bar_depth_3d as i64,
                min: 0,
                max: 20,
                step: 1, unit: " px",
                read_field: |d| d.bars_bar_depth_3d,
            },
        },
        BarsFlashIntensity {
            key: "visualizer.bars.flash_intensity",
            value_type: Float,
            setter: |mgr, v: f64| mgr.with_visualizer(|vz| vz.bars.flash_intensity = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Peak Flash",
                category: "Bars",
                subtitle: Some("Bars bloom toward the peak color on a beat. 0 = disabled"),
                default: f64::from(crate::types::visualizer_config::VisualizerConfig::default().bars.flash_intensity),
                min: 0.0,
                max: 1.0,
                step: 0.05, unit: "",
                read_field: |d| d.bars_flash_intensity,
            },
        },
        BarsTrails {
            key: "visualizer.bars.trails",
            value_type: Float,
            setter: |mgr, v: f64| mgr.with_visualizer(|vz| vz.bars.trails = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Motion Trails",
                category: "Bars",
                subtitle: Some("Bars leave a fading after-image. 0 = off, 1 = long comet trails"),
                default: f64::from(crate::types::visualizer_config::VisualizerConfig::default().bars.trails),
                min: 0.0,
                max: 1.0,
                step: 0.05, unit: "",
                read_field: |d| d.bars_trails,
            },
        },
        BarsEcho {
            key: "visualizer.bars.echo",
            value_type: Float,
            setter: |mgr, v: f64| mgr.with_visualizer(|vz| vz.bars.echo = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Echo",
                category: "Bars",
                subtitle: Some("Milkdrop feedback — the bars spiral and tunnel into themselves with the beat. 0 = off"),
                default: f64::from(crate::types::visualizer_config::VisualizerConfig::default().bars.echo),
                min: 0.0,
                max: 1.0,
                step: 0.05, unit: "",
                read_field: |d| d.bars_echo,
            },
        },
        // -- Lines ---------------------------------------------------------------
        LinesPlacement {
            key: "visualizer.lines.placement",
            value_type: Enum,
            setter: |mgr, v: String| mgr.with_visualizer(|vz| vz.lines.placement = VisualizerPlacement::from_wire_str(&v)),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Placement",
                category: "Lines",
                subtitle: Some("Where the Lines visualizer is drawn\nbottom_band: a band above the player bar (every view)\nover_cover: on the now-playing cover art (Queue view, while playing)"),
                default: crate::types::visualizer_config::VisualizerConfig::default().lines.placement.as_wire_str(),
                options: VisualizerPlacement::all_wire_strs(),
                read_field: |d| d.lines_placement.as_ref(),
            },
        },
        LinesPointCount {
            key: "visualizer.lines.point_count",
            value_type: Int,
            setter: |mgr, v: i64| mgr.with_visualizer(|vz| vz.lines.point_count = v as usize),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Point Count",
                category: "Lines",
                subtitle: Some("8–512, more = finer detail"),
                default: crate::types::visualizer_config::VisualizerConfig::default().lines.point_count as i64,
                min: 8,
                max: 512,
                step: 8, unit: "",
                read_field: |d| d.lines_point_count,
            },
        },
        LinesLineThickness {
            key: "visualizer.lines.line_thickness",
            value_type: Float,
            setter: |mgr, v: f64| mgr.with_visualizer(|vz| vz.lines.line_thickness = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Line Thickness",
                category: "Lines",
                subtitle: Some("% of visualizer height, 1–10%"),
                default: f64::from(crate::types::visualizer_config::VisualizerConfig::default().lines.line_thickness),
                min: 0.01,
                max: 0.10,
                step: 0.01, unit: "%",
                read_field: |d| d.lines_line_thickness,
            },
        },
        LinesOutlineThickness {
            key: "visualizer.lines.outline_thickness",
            value_type: Float,
            setter: |mgr, v: f64| mgr.with_visualizer(|vz| vz.lines.outline_thickness = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Outline Thickness",
                category: "Lines",
                subtitle: Some("Border behind the line in pixels, 0 = disabled"),
                default: f64::from(crate::types::visualizer_config::VisualizerConfig::default().lines.outline_thickness),
                min: 0.0,
                max: 5.0,
                step: 0.5, unit: " px",
                read_field: |d| d.lines_outline_thickness,
            },
        },
        LinesOutlineOpacity {
            key: "visualizer.lines.outline_opacity",
            value_type: Float,
            setter: |mgr, v: f64| mgr.with_visualizer(|vz| vz.lines.outline_opacity = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Outline Opacity",
                category: "Lines",
                subtitle: Some("0.0 = invisible, 1.0 = fully opaque"),
                default: f64::from(crate::types::visualizer_config::VisualizerConfig::default().lines.outline_opacity),
                min: 0.0,
                max: 1.0,
                step: 0.1, unit: "",
                read_field: |d| d.lines_outline_opacity,
            },
        },
        LinesAnimationSpeed {
            key: "visualizer.lines.animation_speed",
            value_type: Float,
            setter: |mgr, v: f64| mgr.with_visualizer(|vz| vz.lines.animation_speed = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Animation Speed",
                category: "Lines",
                subtitle: Some("Color cycling speed. Lower = slower, higher = faster"),
                default: f64::from(crate::types::visualizer_config::VisualizerConfig::default().lines.animation_speed),
                min: 0.05,
                max: 1.0,
                step: 0.05, unit: "",
                read_field: |d| d.lines_animation_speed,
            },
        },
        LinesGradientMode {
            key: "visualizer.lines.gradient_mode",
            value_type: Enum,
            setter: |mgr, v: String| mgr.with_visualizer(|vz| vz.lines.gradient_mode = LinesGradientMode::from_wire_str(&v)),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Gradient Mode",
                category: "Lines",
                subtitle: Some("breathing: time-based cycling through gradient palette\nstatic: uses first gradient color only\nposition: color by horizontal position (bass → treble rainbow)\nheight: color by amplitude (quiet → loud)\ngradient: position + amplitude blend (peaks shift palette)"),
                default: crate::types::visualizer_config::VisualizerConfig::default().lines.gradient_mode.as_wire_str(),
                options: LinesGradientMode::all_wire_strs(),
                read_field: |d| d.lines_gradient_mode.as_ref(),
            },
        },
        LinesFillOpacity {
            key: "visualizer.lines.fill_opacity",
            value_type: Float,
            setter: |mgr, v: f64| mgr.with_visualizer(|vz| vz.lines.fill_opacity = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Fill Opacity",
                category: "Lines",
                subtitle: Some("Fills under the curve with a gradient. 0 = disabled"),
                default: f64::from(crate::types::visualizer_config::VisualizerConfig::default().lines.fill_opacity),
                min: 0.0,
                max: 1.0,
                step: 0.05, unit: "",
                read_field: |d| d.lines_fill_opacity,
            },
        },
        LinesGlowIntensity {
            key: "visualizer.lines.glow_intensity",
            value_type: Float,
            setter: |mgr, v: f64| mgr.with_visualizer(|vz| vz.lines.glow_intensity = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Glow Intensity",
                category: "Lines",
                subtitle: Some("Neon halo around the line. 0 = disabled, brightens with loudness"),
                default: f64::from(crate::types::visualizer_config::VisualizerConfig::default().lines.glow_intensity),
                min: 0.0,
                max: 1.0,
                step: 0.05, unit: "",
                read_field: |d| d.lines_glow_intensity,
            },
        },
        LinesMirror {
            key: "visualizer.lines.mirror",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.with_visualizer(|vz| vz.lines.mirror = v),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Mirror",
                category: "Lines",
                subtitle: Some("Symmetric oscilloscope — line extends from center"),
                default: crate::types::visualizer_config::VisualizerConfig::default().lines.mirror,
                read_field: |d| d.lines_mirror,
            },
        },
        LinesStyle {
            key: "visualizer.lines.style",
            value_type: Enum,
            setter: |mgr, v: String| mgr.with_visualizer(|vz| vz.lines.style = LinesStyle::from_wire_str(&v)),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Line Style",
                category: "Lines",
                subtitle: Some("Interpolation between data points\nsmooth: Catmull-Rom spline (curvy)\nangular: straight line segments"),
                default: crate::types::visualizer_config::VisualizerConfig::default().lines.style.as_wire_str(),
                options: LinesStyle::all_wire_strs(),
                read_field: |d| d.lines_style.as_ref(),
            },
        },
        LinesBoat {
            key: "visualizer.lines.boat",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.with_visualizer(|vz| vz.lines.boat = v),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Surfing boat",
                category: "Lines",
                subtitle: Some("Show a small boat that rides the waveform"),
                default: crate::types::visualizer_config::VisualizerConfig::default().lines.boat,
                read_field: |d| d.lines_boat,
            },
        },
        LinesTrails {
            key: "visualizer.lines.trails",
            value_type: Float,
            setter: |mgr, v: f64| mgr.with_visualizer(|vz| vz.lines.trails = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Motion Trails",
                category: "Lines",
                subtitle: Some("The line leaves a fading after-image. 0 = off, 1 = long comet trails"),
                default: f64::from(crate::types::visualizer_config::VisualizerConfig::default().lines.trails),
                min: 0.0,
                max: 1.0,
                step: 0.05, unit: "",
                read_field: |d| d.lines_trails,
            },
        },
        LinesEcho {
            key: "visualizer.lines.echo",
            value_type: Float,
            setter: |mgr, v: f64| mgr.with_visualizer(|vz| vz.lines.echo = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Echo",
                category: "Lines",
                subtitle: Some("Milkdrop feedback — the line spirals and tunnels into itself with the beat. 0 = off"),
                default: f64::from(crate::types::visualizer_config::VisualizerConfig::default().lines.echo),
                min: 0.0,
                max: 1.0,
                step: 0.05, unit: "",
                read_field: |d| d.lines_echo,
            },
        },
        // -- Scope ---------------------------------------------------------------
        ScopeRadius {
            key: "visualizer.scope.radius",
            value_type: Float,
            setter: |mgr, v: f64| mgr.with_visualizer(|vz| vz.scope.radius = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Ring Size",
                category: "Scope",
                subtitle: Some("Mean ring radius over the cover. 0.1 = small inner ring, 0.95 = nearly fills the panel"),
                default: f64::from(crate::types::visualizer_config::VisualizerConfig::default().scope.radius),
                min: 0.1,
                max: 0.95,
                step: 0.05, unit: "",
                read_field: |d| d.scope_radius,
            },
        },
        ScopeSensitivity {
            key: "visualizer.scope.sensitivity",
            value_type: Float,
            setter: |mgr, v: f64| mgr.with_visualizer(|vz| vz.scope.sensitivity = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Sensitivity",
                category: "Scope",
                subtitle: Some("How hard loud audio swings the ring in and out. 0.5 = subtle, 5 = wild"),
                default: f64::from(crate::types::visualizer_config::VisualizerConfig::default().scope.sensitivity),
                min: 0.5,
                max: 5.0,
                step: 0.1, unit: "×",
                read_field: |d| d.scope_sensitivity,
            },
        },
        ScopePointCount {
            key: "visualizer.scope.point_count",
            value_type: Int,
            setter: |mgr, v: i64| mgr.with_visualizer(|vz| vz.scope.point_count = v as usize),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Point Count",
                category: "Scope",
                subtitle: Some("Points around the ring. 16 = chunky, 512 = finely detailed waveform"),
                default: crate::types::visualizer_config::VisualizerConfig::default().scope.point_count as i64,
                min: 16,
                max: 512,
                step: 16, unit: "",
                read_field: |d| d.scope_point_count,
            },
        },
        ScopeLineThickness {
            key: "visualizer.scope.line_thickness",
            value_type: Float,
            setter: |mgr, v: f64| mgr.with_visualizer(|vz| vz.scope.line_thickness = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Line Thickness",
                category: "Scope",
                subtitle: Some("Ring stroke as % of panel size, 0.5–10%"),
                default: f64::from(crate::types::visualizer_config::VisualizerConfig::default().scope.line_thickness),
                min: 0.005,
                max: 0.1,
                step: 0.005, unit: "%",
                read_field: |d| d.scope_line_thickness,
            },
        },
        ScopeFillOpacity {
            key: "visualizer.scope.fill_opacity",
            value_type: Float,
            setter: |mgr, v: f64| mgr.with_visualizer(|vz| vz.scope.fill_opacity = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Fill Opacity",
                category: "Scope",
                subtitle: Some("Radial gradient fill from the ring toward the center. 0 = outline only, 1 = solid rim"),
                default: f64::from(crate::types::visualizer_config::VisualizerConfig::default().scope.fill_opacity),
                min: 0.0,
                max: 1.0,
                step: 0.05, unit: "",
                read_field: |d| d.scope_fill_opacity,
            },
        },
        ScopeGlowIntensity {
            key: "visualizer.scope.glow_intensity",
            value_type: Float,
            setter: |mgr, v: f64| mgr.with_visualizer(|vz| vz.scope.glow_intensity = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Glow Intensity",
                category: "Scope",
                subtitle: Some("Neon halo around the ring. 0 = disabled, brightens with loudness"),
                default: f64::from(crate::types::visualizer_config::VisualizerConfig::default().scope.glow_intensity),
                min: 0.0,
                max: 1.0,
                step: 0.05, unit: "",
                read_field: |d| d.scope_glow_intensity,
            },
        },
        ScopeOutlineThickness {
            key: "visualizer.scope.outline_thickness",
            value_type: Float,
            setter: |mgr, v: f64| mgr.with_visualizer(|vz| vz.scope.outline_thickness = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Outline Thickness",
                category: "Scope",
                subtitle: Some("Darker border behind the ring in pixels, 0 = disabled"),
                default: f64::from(crate::types::visualizer_config::VisualizerConfig::default().scope.outline_thickness),
                min: 0.0,
                max: 5.0,
                step: 0.5, unit: " px",
                read_field: |d| d.scope_outline_thickness,
            },
        },
        ScopeOutlineOpacity {
            key: "visualizer.scope.outline_opacity",
            value_type: Float,
            setter: |mgr, v: f64| mgr.with_visualizer(|vz| vz.scope.outline_opacity = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Outline Opacity",
                category: "Scope",
                subtitle: Some("0.0 = invisible, 1.0 = fully opaque"),
                default: f64::from(crate::types::visualizer_config::VisualizerConfig::default().scope.outline_opacity),
                min: 0.0,
                max: 1.0,
                step: 0.1, unit: "",
                read_field: |d| d.scope_outline_opacity,
            },
        },
        ScopeGradientMode {
            key: "visualizer.scope.gradient_mode",
            value_type: Enum,
            setter: |mgr, v: String| mgr.with_visualizer(|vz| vz.scope.gradient_mode = LinesGradientMode::from_wire_str(&v)),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Gradient Mode",
                category: "Scope",
                subtitle: Some("breathing: time-based cycling through gradient palette\nstatic: uses first gradient color only\nposition: color by angle around the ring\nheight: color by amplitude (quiet → loud)\ngradient: angle + amplitude blend (peaks shift palette)"),
                default: crate::types::visualizer_config::VisualizerConfig::default().scope.gradient_mode.as_wire_str(),
                options: LinesGradientMode::all_wire_strs(),
                read_field: |d| d.scope_gradient_mode.as_ref(),
            },
        },
        ScopeAnimationSpeed {
            key: "visualizer.scope.animation_speed",
            value_type: Float,
            setter: |mgr, v: f64| mgr.with_visualizer(|vz| vz.scope.animation_speed = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Animation Speed",
                category: "Scope",
                subtitle: Some("Color cycling speed for the breathing gradient. Lower = slower"),
                default: f64::from(crate::types::visualizer_config::VisualizerConfig::default().scope.animation_speed),
                min: 0.05,
                max: 1.0,
                step: 0.05, unit: "",
                read_field: |d| d.scope_animation_speed,
            },
        },
        ScopeStyle {
            key: "visualizer.scope.style",
            value_type: Enum,
            setter: |mgr, v: String| mgr.with_visualizer(|vz| vz.scope.style = LinesStyle::from_wire_str(&v)),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Line Style",
                category: "Scope",
                subtitle: Some("Interpolation around the ring\nsmooth: Catmull-Rom spline (curvy)\nangular: straight segments"),
                default: crate::types::visualizer_config::VisualizerConfig::default().scope.style.as_wire_str(),
                options: LinesStyle::all_wire_strs(),
                read_field: |d| d.scope_style.as_ref(),
            },
        },
        ScopeParticles {
            key: "visualizer.scope.particles",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.with_visualizer(|vz| vz.scope.particles = v),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Particles",
                category: "Scope",
                subtitle: Some("Glowing particles drifting out from the ring (NCS-style)"),
                default: crate::types::visualizer_config::VisualizerConfig::default().scope.particles,
                read_field: |d| d.scope_particles,
            },
        },
        ScopeParticleCount {
            key: "visualizer.scope.particle_count",
            value_type: Int,
            setter: |mgr, v: i64| mgr.with_visualizer(|vz| vz.scope.particle_count = v as usize),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Particle Count",
                category: "Scope",
                subtitle: Some("How many particles fill the field. 0 = none, 2048 = dense"),
                default: crate::types::visualizer_config::VisualizerConfig::default().scope.particle_count as i64,
                min: 0,
                max: 2048,
                step: 64, unit: "",
                read_field: |d| d.scope_particle_count,
            },
        },
        ScopeParticleSpeed {
            key: "visualizer.scope.particle_speed",
            value_type: Float,
            setter: |mgr, v: f64| mgr.with_visualizer(|vz| vz.scope.particle_speed = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Particle Speed",
                category: "Scope",
                subtitle: Some("How fast particles fly out. 0.1 = lazy drift, 4 = energetic"),
                default: f64::from(crate::types::visualizer_config::VisualizerConfig::default().scope.particle_speed),
                min: 0.1,
                max: 4.0,
                step: 0.1, unit: "×",
                read_field: |d| d.scope_particle_speed,
            },
        },
        ScopeBeam {
            key: "visualizer.scope.beam",
            value_type: Bool,
            setter: |mgr, v: bool| mgr.with_visualizer(|vz| vz.scope.beam = v),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Beam Glow",
                category: "Scope",
                subtitle: Some("Additive luminous beam (woscope-style) — the ring glows brighter over the cover. Pair with Glow"),
                default: crate::types::visualizer_config::VisualizerConfig::default().scope.beam,
                read_field: |d| d.scope_beam,
            },
        },
        ScopeTrails {
            key: "visualizer.scope.trails",
            value_type: Float,
            setter: |mgr, v: f64| mgr.with_visualizer(|vz| vz.scope.trails = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Motion Trails",
                category: "Scope",
                subtitle: Some("The ring leaves a fading after-image. 0 = off, 1 = long comet trails"),
                default: f64::from(crate::types::visualizer_config::VisualizerConfig::default().scope.trails),
                min: 0.0,
                max: 1.0,
                step: 0.05, unit: "",
                read_field: |d| d.scope_trails,
            },
        },
        ScopeEcho {
            key: "visualizer.scope.echo",
            value_type: Float,
            setter: |mgr, v: f64| mgr.with_visualizer(|vz| vz.scope.echo = v as f32),
            toml_apply: |_ts, _p| {},
            read: |_src, _out| {},
            write: |_ps, _ts| {},
            ui_meta: {
                label: "Echo",
                category: "Scope",
                subtitle: Some("Milkdrop feedback — the ring spirals and tunnels inward with the beat. 0 = off"),
                default: f64::from(crate::types::visualizer_config::VisualizerConfig::default().scope.echo),
                min: 0.0,
                max: 1.0,
                step: 0.05, unit: "",
                read_field: |d| d.scope_echo,
            },
        },
    ]
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::{
        services::{settings::SettingsManager, state_storage::StateStorage},
        types::{setting_value::SettingValue, visualizer_config::*},
    };

    fn make_mgr() -> (SettingsManager, TempDir) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("test_settings.redb");
        let storage = StateStorage::new(path).expect("StateStorage::new");
        (SettingsManager::for_test(storage), tmp)
    }

    fn int(val: i64) -> SettingValue {
        SettingValue::Int {
            val,
            min: 0,
            max: 10_000,
            step: 1,
            unit: "",
        }
    }

    fn float(val: f64) -> SettingValue {
        SettingValue::Float {
            val,
            min: 0.0,
            max: 10.0,
            step: 0.01,
            unit: "",
        }
    }

    fn enum_val(val: &str) -> SettingValue {
        SettingValue::Enum {
            val: val.to_string(),
            options: vec![],
        }
    }

    /// Bool dispatch mutates the in-memory `mgr.visualizer` field.
    #[test]
    fn dispatch_visualizer_bool_persists() {
        let (mut mgr, _tmp) = make_mgr();
        assert!(!mgr.visualizer().bars.led_bars);
        let res = dispatch_visualizer_tab_setting(
            "visualizer.bars.led_bars",
            SettingValue::Bool(true),
            &mut mgr,
        );
        assert!(matches!(res, Some(Ok(_))), "dispatch must claim + succeed");
        assert!(mgr.visualizer().bars.led_bars);
    }

    /// Int dispatch mutates the in-memory field (in-range value preserved
    /// through the setter-side validate()).
    #[test]
    fn dispatch_visualizer_int_persists() {
        let (mut mgr, _tmp) = make_mgr();
        let res =
            dispatch_visualizer_tab_setting("visualizer.scope.point_count", int(128), &mut mgr);
        assert!(matches!(res, Some(Ok(_))));
        assert_eq!(mgr.visualizer().scope.point_count, 128);

        // Out-of-range values are clamped by validate() — the same clamp the
        // legacy reload path applied on read-back.
        let res =
            dispatch_visualizer_tab_setting("visualizer.scope.point_count", int(9000), &mut mgr);
        assert!(matches!(res, Some(Ok(_))));
        assert_eq!(mgr.visualizer().scope.point_count, 512);
    }

    /// Float dispatch mutates the in-memory field.
    #[test]
    fn dispatch_visualizer_float_persists() {
        let (mut mgr, _tmp) = make_mgr();
        let res =
            dispatch_visualizer_tab_setting("visualizer.noise_reduction", float(0.42), &mut mgr);
        assert!(matches!(res, Some(Ok(_))));
        assert_eq!(mgr.visualizer().noise_reduction, 0.42);
    }

    /// Enum dispatch parses the WIRE string via `from_wire_str` and persists;
    /// every dropdown wire string round-trips to the field and back.
    #[test]
    fn dispatch_visualizer_enum_persists() {
        let (mut mgr, _tmp) = make_mgr();
        let res = dispatch_visualizer_tab_setting(
            "visualizer.bars.gradient_mode",
            enum_val("static"),
            &mut mgr,
        );
        assert!(matches!(res, Some(Ok(_))));
        assert_eq!(
            mgr.visualizer().bars.gradient_mode,
            BarsGradientMode::Static
        );

        // Every wire string a dropdown can emit round-trips exactly.
        for wire in BarsPeakMode::all_wire_strs() {
            let res = dispatch_visualizer_tab_setting(
                "visualizer.bars.peak_mode",
                enum_val(wire),
                &mut mgr,
            );
            assert!(matches!(res, Some(Ok(_))));
            assert_eq!(mgr.visualizer().bars.peak_mode.as_wire_str(), wire);
        }
    }

    /// Mutual exclusivity (data-side): enabling monstercat (>= the effective
    /// minimum) auto-disables waves in the in-memory config.
    #[test]
    fn monstercat_enable_disables_waves() {
        let (mut mgr, _tmp) = make_mgr();
        let res =
            dispatch_visualizer_tab_setting("visualizer.waves", SettingValue::Bool(true), &mut mgr);
        assert!(matches!(res, Some(Ok(_))));
        assert!(mgr.visualizer().waves);

        let res = dispatch_visualizer_tab_setting("visualizer.monstercat", float(1.0), &mut mgr);
        assert!(matches!(res, Some(Ok(_))));
        assert_eq!(mgr.visualizer().monstercat, 1.0);
        assert!(
            !mgr.visualizer().waves,
            "enabling monstercat must auto-disable waves"
        );

        // Sub-threshold monstercat snaps to 0.0 (validate) and must NOT
        // touch waves.
        let _ =
            dispatch_visualizer_tab_setting("visualizer.waves", SettingValue::Bool(true), &mut mgr);
        let res = dispatch_visualizer_tab_setting("visualizer.monstercat", float(0.5), &mut mgr);
        assert!(matches!(res, Some(Ok(_))));
        assert_eq!(
            mgr.visualizer().monstercat,
            0.0,
            "sub-threshold monstercat snaps to off"
        );
        assert!(
            mgr.visualizer().waves,
            "sub-threshold monstercat must not disable waves"
        );
    }

    /// Mutual exclusivity (data-side): enabling waves zeroes monstercat.
    #[test]
    fn waves_enable_zeroes_monstercat() {
        let (mut mgr, _tmp) = make_mgr();
        // Default monstercat is 1.0 (on).
        assert_eq!(mgr.visualizer().monstercat, 1.0);

        let res =
            dispatch_visualizer_tab_setting("visualizer.waves", SettingValue::Bool(true), &mut mgr);
        assert!(matches!(res, Some(Ok(_))));
        assert!(mgr.visualizer().waves);
        assert_eq!(
            mgr.visualizer().monstercat,
            0.0,
            "enabling waves must zero monstercat"
        );

        // Disabling waves leaves monstercat alone.
        let _ = dispatch_visualizer_tab_setting("visualizer.monstercat", float(1.0), &mut mgr);
        let res = dispatch_visualizer_tab_setting(
            "visualizer.waves",
            SettingValue::Bool(false),
            &mut mgr,
        );
        assert!(matches!(res, Some(Ok(_))));
        assert_eq!(
            mgr.visualizer().monstercat,
            1.0,
            "disabling waves must not touch monstercat"
        );
    }

    /// A wrong SettingValue variant is a typed error, not a silent ignore.
    #[test]
    fn visualizer_dispatch_type_mismatch_errors() {
        let (mut mgr, _tmp) = make_mgr();
        let res = dispatch_visualizer_tab_setting("visualizer.bars.led_bars", float(1.0), &mut mgr);
        assert!(
            matches!(res, Some(Err(_))),
            "Bool key fed a Float must be Some(Err), got a silent pass"
        );
    }

    /// Every macro row's CURRENT value (as built by the items builder from a
    /// default config) dispatches cleanly — pinning the row-value-variant ↔
    /// dispatch-arm agreement per key. This is the guard that catches an
    /// f32-backed config field surfaced as an Int row but declared Float in
    /// the table (a silent write-then-no-live-update bug).
    #[test]
    fn every_visualizer_macro_row_dispatches() {
        let (mut mgr, _tmp) = make_mgr();
        let data = crate::types::settings_data::VisualizerSettingsData::from(
            &crate::types::visualizer_config::VisualizerConfig::default(),
        );
        let rows = build_visualizer_tab_settings_items(&data);
        assert_eq!(
            rows.len(),
            keys::ALL_KEYS.len(),
            "every visualizer entry must carry ui_meta (one UI row per key)"
        );
        for row in rows {
            let crate::types::setting_item::SettingsEntry::Item(item) = row else {
                panic!("macro rows must be items");
            };
            let res = dispatch_visualizer_tab_setting(&item.key, item.value.clone(), &mut mgr);
            assert!(
                matches!(res, Some(Ok(_))),
                "row {} with value {:?} must dispatch cleanly (row/table type drift)",
                item.key,
                item.value
            );
        }
    }

    /// Every `keys::` constant has exactly one macro entry: the table length
    /// matches `keys::ALL_KEYS`, every key is claimed by the containment
    /// helper, and every table key appears in the registry (bidirectional).
    #[test]
    fn every_visualizer_key_has_a_macro_entry() {
        assert_eq!(
            TAB_VISUALIZER_SETTINGS.len(),
            keys::ALL_KEYS.len(),
            "table entries must match the keys registry 1:1"
        );
        for key in keys::ALL_KEYS {
            assert!(
                tab_visualizer_contains(key),
                "keys::ALL_KEYS entry {key} has no macro entry"
            );
        }
        for def in TAB_VISUALIZER_SETTINGS {
            assert!(
                keys::ALL_KEYS.contains(&def.key),
                "table key {} missing from keys::ALL_KEYS",
                def.key
            );
        }
    }
}
