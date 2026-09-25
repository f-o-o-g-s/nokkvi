//! MilkDrop mode handlers: picking presets, the off-thread load pipeline
//! (parse + shader translation, then the renderer build on iced's device),
//! releasing the renderer, and the 100 ms tick that drives it all.
//!
//! Every load bumps `generation` (mirrored to `shared.current_generation`);
//! anything that finishes for an older generation is dropped. A release bumps
//! it too, so a build landing after the mode was left is dropped silently.

use std::{
    sync::{Arc, atomic::Ordering},
    time::Instant,
};

use iced::Task;
use nokkvi_data::types::player_settings::VisualizationMode;
use tracing::{debug, info, warn};

use crate::{
    Nokkvi, Screen, View,
    app_message::{Message, MilkdropMessage},
    state::{MILKDROP_HISTORY_CAP, MILKDROP_MAX_CONSECUTIVE_FAILURES},
    widgets::visualizer::milkdrop::{
        BuiltPreset, CompiledPreset, GpuHandles, build_renderer, compile_preset,
    },
};

/// Whether MilkDrop should animate: MilkDrop mode, playing and not paused, on
/// the Home screen, with a panel that shows it on screen. Only the Queue and
/// Radios artwork panels and Theater Mode render the over-cover slot, so on any
/// other view nothing advances, switches or builds; the renderer is kept (not
/// released) so returning shows the frozen frame at once.
pub(crate) fn milkdrop_running(
    mode: VisualizationMode,
    playing: bool,
    paused: bool,
    screen: Screen,
    panel_visible: bool,
) -> bool {
    mode == VisualizationMode::Milkdrop
        && playing
        && !paused
        && screen == Screen::Home
        && panel_visible
}

/// Why a new preset is being picked (logged).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdvanceReason {
    /// Nothing on screen: entering the mode, play after stop, a new device.
    Enter,
    /// The previous build failed.
    Failure,
}

impl Nokkvi {
    fn milkdrop_mode_active(&self) -> bool {
        self.engine.visualization_mode == VisualizationMode::Milkdrop
    }

    fn milkdrop_panel_visible(&self) -> bool {
        self.theater.active || matches!(self.current_view, View::Queue | View::Radios)
    }

    fn milkdrop_set_generation(&mut self, generation: u64) {
        self.milkdrop.generation = generation;
        self.milkdrop
            .shared
            .current_generation
            .store(generation, Ordering::Release);
    }

    pub(crate) fn handle_milkdrop(&mut self, msg: MilkdropMessage) -> Task<Message> {
        match msg {
            MilkdropMessage::Compiled { generation, result } => {
                self.handle_milkdrop_compiled(generation, result)
            }
            MilkdropMessage::Built { generation, result } => {
                self.handle_milkdrop_built(generation, result)
            }
        }
    }

    /// Mode edge, computed from the previous and new mode so a config hot
    /// reload that re-delivers the same mode fires nothing. Entering only flips
    /// the analyzer on (the tick loads the first preset within 100 ms);
    /// leaving also releases the renderer.
    pub(crate) fn milkdrop_mode_edge(&mut self, prev: VisualizationMode, new: VisualizationMode) {
        let was = prev == VisualizationMode::Milkdrop;
        let is = new == VisualizationMode::Milkdrop;
        if was == is {
            return;
        }
        if let Some(viz) = &self.visualizer {
            viz.set_milkdrop_mode(is);
        }
        if was {
            self.milkdrop_release();
        }
    }

    /// Drop the renderer (the pipeline enforces the raised watermark in
    /// `trim()` on the next frame, mounted or not) and forget the current preset so the next play
    /// or re-entry loads again. Called on leaving the mode, on stop and on
    /// logout.
    pub(crate) fn milkdrop_release(&mut self) {
        self.milkdrop.shared.running.store(false, Ordering::Release);
        self.milkdrop.current = None;
        self.milkdrop.build_in_flight = None;
        self.milkdrop.awaiting_gpu = None;
        self.milkdrop.next_switch_at = None;
        self.milkdrop.consecutive_failures = 0;
        let generation = self.milkdrop.generation + 1;
        self.milkdrop_set_generation(generation);
        // Everything loaded before this point is released.
        self.milkdrop
            .shared
            .released_below
            .store(generation, Ordering::Release);
    }

    /// Build the preset library at login: the bundled pack, the user's
    /// `~/.config/nokkvi/milkdrop/*.json` and the curation file. The only
    /// place the real paths are read, so tests never touch them.
    pub(crate) fn milkdrop_build_library(&mut self) {
        use nokkvi_data::{
            services::milkdrop_presets::{Curation, PresetLibrary},
            utils::paths,
        };
        match (
            paths::get_milkdrop_dir(),
            paths::get_milkdrop_curation_path(),
        ) {
            (Ok(dir), Ok(curation)) => {
                self.milkdrop.user_dir = dir;
                self.milkdrop.curation_path = curation;
            }
            (Err(e), _) | (_, Err(e)) => warn!("milkdrop: no user preset directory: {e}"),
        }
        let curation = if self.milkdrop.curation_path.as_os_str().is_empty() {
            Curation::default()
        } else {
            Curation::load(&self.milkdrop.curation_path)
        };
        self.milkdrop.library = PresetLibrary::new(
            crate::widgets::visualizer::milkdrop::BUNDLED_MILKDROP_PRESETS,
            &self.milkdrop.user_dir,
            curation,
        );
        debug!(
            presets = self.milkdrop.library.len(),
            "milkdrop: preset library built"
        );
    }

    /// Start loading `name`: parse + translate its shaders on a blocking
    /// thread, then (`Compiled`) build its renderer.
    pub(crate) fn milkdrop_load(&mut self, name: String) -> Task<Message> {
        let Some(source) = self.milkdrop.library.source(&name).cloned() else {
            warn!(preset = %name, "milkdrop: preset vanished from the library");
            return Task::none();
        };
        let generation = self.milkdrop.generation + 1;
        self.milkdrop_set_generation(generation);
        self.milkdrop.build_in_flight = Some(generation);
        self.milkdrop.awaiting_gpu = None;
        self.milkdrop.current = Some(name.clone());

        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    let started = Instant::now();
                    let text = source.read().map_err(|e| format!("{name}: {e}"))?;
                    let preset = compile_preset(name, &text)?;
                    debug!(
                        preset = %preset.name,
                        ms = started.elapsed().as_millis(),
                        "milkdrop: preset compiled"
                    );
                    Ok(Arc::new(preset))
                })
                .await
                .map_err(|e| format!("preset compile task failed: {e}"))
                .and_then(|result| result)
            },
            move |result| Message::Milkdrop(MilkdropMessage::Compiled { generation, result }),
        )
    }

    fn handle_milkdrop_compiled(
        &mut self,
        generation: u64,
        result: Result<Arc<CompiledPreset>, String>,
    ) -> Task<Message> {
        if self.milkdrop.build_in_flight != Some(generation) {
            debug!(generation, "milkdrop: dropped a stale compiled preset");
            return Task::none();
        }
        match result {
            Err(e) => self.milkdrop_failed(&e),
            Ok(preset) => match self.milkdrop.shared.gpu_handles() {
                Some(gpu) => self.milkdrop_build_task(gpu, generation, preset),
                None => {
                    // `prepare` has not run yet; the tick drains this once it has.
                    self.milkdrop.awaiting_gpu = Some((generation, preset));
                    Task::none()
                }
            },
        }
    }

    /// Build the renderer on a blocking thread and hand it to `prepare` through
    /// `shared.built` (refused there if a newer load superseded it).
    fn milkdrop_build_task(
        &self,
        gpu: GpuHandles,
        generation: u64,
        preset: Arc<CompiledPreset>,
    ) -> Task<Message> {
        let shared = self.milkdrop.shared.clone();
        let size = *shared.render_size.lock();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    let started = Instant::now();
                    let renderer = build_renderer(&gpu, size, &preset)
                        .map_err(|e| format!("{}: {e}", preset.name))?;
                    debug!(
                        preset = %preset.name,
                        ms = started.elapsed().as_millis(),
                        ?size,
                        "milkdrop: renderer built"
                    );
                    shared.offer_built(BuiltPreset {
                        generation,
                        epoch: gpu.epoch,
                        name: preset.name.clone(),
                        renderer,
                    });
                    Ok(())
                })
                .await
                .map_err(|e| format!("renderer build task failed: {e}"))
                .and_then(|result| result)
            },
            move |result| Message::Milkdrop(MilkdropMessage::Built { generation, result }),
        )
    }

    fn handle_milkdrop_built(
        &mut self,
        generation: u64,
        result: Result<(), String>,
    ) -> Task<Message> {
        // A build finishing after the mode was left, or for a superseded load,
        // is dropped without a toast.
        if self.milkdrop.build_in_flight != Some(generation) || !self.milkdrop_mode_active() {
            debug!(generation, "milkdrop: dropped a stale build result");
            return Task::none();
        }
        match result {
            Err(e) => self.milkdrop_failed(&e),
            Ok(()) => {
                self.milkdrop.build_in_flight = None;
                self.milkdrop.consecutive_failures = 0;
                self.milkdrop.empty_warned = false;
                if let Some(name) = self.milkdrop.current.clone() {
                    info!(preset = %name, "milkdrop: preset on screen");
                    self.toast_info(name);
                }
                Task::none()
            }
        }
    }

    fn milkdrop_failed(&mut self, error: &str) -> Task<Message> {
        warn!(
            preset = self.milkdrop.current.as_deref().unwrap_or_default(),
            "milkdrop: preset failed to load: {error}"
        );
        self.milkdrop.build_in_flight = None;
        self.milkdrop.awaiting_gpu = None;
        self.milkdrop.consecutive_failures = self.milkdrop.consecutive_failures.saturating_add(1);
        self.milkdrop_advance(AdvanceReason::Failure)
    }

    /// Pick the next preset and load it, unless too many builds failed in a
    /// row or nothing is eligible (each warns once).
    pub(crate) fn milkdrop_advance(&mut self, reason: AdvanceReason) -> Task<Message> {
        if self.milkdrop.consecutive_failures >= MILKDROP_MAX_CONSECUTIVE_FAILURES {
            if self.milkdrop.consecutive_failures == MILKDROP_MAX_CONSECUTIVE_FAILURES {
                self.toast_warn("MilkDrop: no preset could be built; check nokkvi.log");
            }
            self.milkdrop.current = None;
            self.milkdrop.next_switch_at = None;
            return Task::none();
        }
        let current = self.milkdrop.current.clone();
        let Some(next) = self.milkdrop.library.next_random(current.as_deref()) else {
            if !self.milkdrop.empty_warned {
                self.milkdrop.empty_warned = true;
                self.toast_warn("MilkDrop: no presets to show (all hidden?)");
            }
            self.milkdrop.current = None;
            self.milkdrop.next_switch_at = None;
            return Task::none();
        };
        debug!(?reason, preset = %next, "milkdrop: next preset");
        if let Some(previous) = current
            && reason != AdvanceReason::Failure
        {
            self.milkdrop.history.push(previous);
            if self.milkdrop.history.len() > MILKDROP_HISTORY_CAP {
                self.milkdrop.history.remove(0);
            }
        }
        self.milkdrop_load(next)
    }

    /// Runs every 100 ms: publishes the running flag, notices a lost slot or a
    /// replaced GPU device, feeds a waiting compiled preset to the builder,
    /// and loads a preset whenever one should be on screen and none is.
    pub(crate) fn milkdrop_tick(&mut self) -> Task<Message> {
        let active = self.milkdrop_mode_active();
        if let Some(viz) = &self.visualizer {
            // Level-set every tick: a login after the settings loaded (or the
            // reverse) still lands on the right analyzer mode.
            viz.set_milkdrop_mode(active);
        }
        if !active {
            self.milkdrop.shared.running.store(false, Ordering::Release);
            return Task::none();
        }

        let running = milkdrop_running(
            self.engine.visualization_mode,
            self.playback.playing,
            self.playback.paused,
            self.screen,
            self.milkdrop_panel_visible(),
        );
        self.milkdrop
            .shared
            .running
            .store(running, Ordering::Release);

        // `prepare` dropped the renderer after a GPU error: load another.
        if self.milkdrop.shared.slot_lost.swap(false, Ordering::AcqRel) {
            self.milkdrop.current = None;
            self.milkdrop.build_in_flight = None;
        }

        // A new device (tray hide + show): everything built on the old one is
        // gone (`prepare` dropped it), so reload.
        if let Some(epoch) = self.milkdrop.shared.gpu_epoch()
            && epoch != self.milkdrop.gpu_epoch_seen
        {
            if self.milkdrop.gpu_epoch_seen != 0 {
                debug!(epoch, "milkdrop: GPU device replaced; reloading");
                self.milkdrop.current = None;
                self.milkdrop.build_in_flight = None;
                self.milkdrop.awaiting_gpu = None;
            }
            self.milkdrop.gpu_epoch_seen = epoch;
        }

        let mut tasks = Vec::new();
        if let Some((generation, preset)) = self.milkdrop.awaiting_gpu.take() {
            match self.milkdrop.shared.gpu_handles() {
                Some(gpu) if self.milkdrop.build_in_flight == Some(generation) => {
                    tasks.push(self.milkdrop_build_task(gpu, generation, preset));
                }
                Some(_) => {}
                None => self.milkdrop.awaiting_gpu = Some((generation, preset)),
            }
        }

        if !running {
            self.milkdrop.next_switch_at = None;
        } else if self.milkdrop.current.is_none()
            && self.milkdrop.build_in_flight.is_none()
            && self.milkdrop.consecutive_failures < MILKDROP_MAX_CONSECUTIVE_FAILURES
        {
            tasks.push(self.milkdrop_advance(AdvanceReason::Enter));
        }
        Task::batch(tasks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_needs_every_condition() {
        let m = VisualizationMode::Milkdrop;
        assert!(milkdrop_running(m, true, false, Screen::Home, true));
        assert!(!milkdrop_running(
            VisualizationMode::Scope,
            true,
            false,
            Screen::Home,
            true
        ));
        assert!(
            !milkdrop_running(m, false, false, Screen::Home, true),
            "stopped"
        );
        assert!(
            !milkdrop_running(m, true, true, Screen::Home, true),
            "paused"
        );
        assert!(
            !milkdrop_running(m, true, false, Screen::Login, true),
            "login"
        );
        assert!(
            !milkdrop_running(m, true, false, Screen::Home, false),
            "off the panel"
        );
    }
}
