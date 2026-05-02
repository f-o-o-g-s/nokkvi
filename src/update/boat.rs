//! Surfing-boat overlay handler — see `widgets/boat.rs` for the physics
//! model and pure helpers.
//!
//! Per-frame: derive visibility, then run one physics step that integrates
//! the boat's position from the live bar buffer. Bails cheaply when not in
//! lines mode so the always-on `iced::window::frames()` subscription isn't
//! expensive.

use std::time::Instant;

use iced::Task;
use nokkvi_data::types::player_settings::VisualizationMode;

use crate::{Nokkvi, app_message::Message, widgets::boat};

/// Handle a per-frame boat tick. Visibility is derived; physics step runs
/// against the live bar buffer. When hidden, position/velocity/phase are
/// preserved so the boat resumes mid-stroke when re-shown.
pub(crate) fn handle_boat_tick(app: &mut Nokkvi, now: Instant) -> Task<Message> {
    // Read mode + config snapshot once per tick. The "visualizer enabled"
    // check is `engine.visualization_mode != VisualizationMode::Off` — that's
    // what gates the shader element in `app_view.rs:319`. There is no
    // separate `cfg.enabled` flag, so the `Lines` discriminator covers both
    // "visualizer on" and "lines mode" in a single check.
    let in_lines_mode = app.engine.visualization_mode == VisualizationMode::Lines;
    let cfg = app.visualizer_config.read();
    let cfg_boat_on = cfg.lines.boat;
    let angular = cfg.lines.style.eq_ignore_ascii_case("angular");
    drop(cfg);

    let visible = in_lines_mode && cfg_boat_on;

    if !visible {
        // Drop the dt baseline so the next visible frame doesn't see a stale gap.
        app.boat.visible = false;
        app.boat.last_tick = None;
        return Task::none();
    }

    let dt = match app.boat.last_tick {
        Some(prev) => now.saturating_duration_since(prev),
        None => std::time::Duration::ZERO,
    };
    app.boat.last_tick = Some(now);

    // Sample the live waveform. When the visualizer hasn't been mounted yet
    // (pre-login, or visualizer disabled mid-flight) `app.visualizer` is
    // None — feed an empty slice so y_ratio settles toward 0.
    let bars = if let Some(viz) = &app.visualizer {
        viz.current_bars()
    } else {
        Vec::new()
    };
    boat::step(&mut app.boat, dt, &bars, angular);
    app.boat.visible = true;
    // Pre-build (and cache) the SVG handle so the immutable `view()` render
    // path can clone it cheaply. Lazy by design — first visible tick only.
    let _ = app.boat.ensure_handle();

    Task::none()
}
