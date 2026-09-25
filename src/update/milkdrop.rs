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
    app_message::{Message, MilkdropControl, MilkdropMessage},
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
    /// The switch interval elapsed.
    Timer,
    /// A new track started.
    TrackChange,
    /// The Next Preset key.
    Manual,
}

impl Nokkvi {
    fn milkdrop_mode_active(&self) -> bool {
        self.engine.visualization_mode == VisualizationMode::Milkdrop
    }

    /// A MilkDrop panel is on screen: the window is open, the view is one that
    /// renders the over-cover slot, and the panel's `prepare` actually ran
    /// lately (the Queue without an artwork column, a narrow window or a
    /// split view draw none).
    fn milkdrop_panel_visible(&self) -> bool {
        self.main_window_id.is_some()
            && (self.theater.active || matches!(self.current_view, View::Queue | View::Radios))
            && self.milkdrop.shared.mounted_recently()
    }

    /// Whether MilkDrop should animate right now (see [`milkdrop_running`]).
    pub(crate) fn milkdrop_is_running(&self) -> bool {
        milkdrop_running(
            self.engine.visualization_mode,
            self.playback.playing,
            self.playback.paused,
            self.screen,
            self.milkdrop_panel_visible(),
        )
    }

    /// The live `[visualizer.milkdrop]` settings (hot-reloaded).
    fn milkdrop_config(&self) -> nokkvi_data::types::visualizer_config::MilkdropConfig {
        self.visualizer_config.read().milkdrop.clone()
    }

    fn milkdrop_interval(&self) -> Option<std::time::Duration> {
        let secs = self.milkdrop_config().preset_interval_secs;
        (secs > 0).then(|| std::time::Duration::from_secs(u64::from(secs)))
    }

    fn milkdrop_switch_on_track_change(&self) -> bool {
        self.milkdrop_config().switch_on_track_change
    }

    /// Push the settings the library and the render side read on their own.
    fn milkdrop_apply_config(&mut self) {
        use nokkvi_data::types::visualizer_config::MilkdropPresetSource;
        let cfg = self.milkdrop_config();
        self.milkdrop
            .library
            .set_favorites_only(cfg.preset_source == MilkdropPresetSource::FavoritesOnly);
        self.milkdrop
            .shared
            .quality_short_side
            .store(cfg.render_quality.short_side_px(), Ordering::Release);
    }

    /// Arm the switch timer from now, unless locked or the interval is 0.
    fn milkdrop_arm_timer(&mut self) {
        self.milkdrop.next_switch_at = if self.milkdrop.locked {
            None
        } else {
            self.milkdrop_interval().map(|d| Instant::now() + d)
        };
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
            MilkdropMessage::Control(control) => match control {
                MilkdropControl::Next => self.handle_milkdrop_next(),
                MilkdropControl::Previous => self.handle_milkdrop_previous(),
                MilkdropControl::ToggleLock => self.handle_milkdrop_toggle_lock(),
                MilkdropControl::ToggleFavorite => self.handle_milkdrop_toggle_favorite(),
                MilkdropControl::Hide => self.handle_milkdrop_hide(),
            },
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
        self.milkdrop.on_screen = None;
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

    /// The main window is closing (hidden to the tray, or quitting): release
    /// the renderer and let go of the window's GPU device; the next window's
    /// first frame captures a new one.
    pub(crate) fn milkdrop_on_window_closed(&mut self) {
        self.milkdrop_release();
        self.milkdrop.shared.gpu.lock().take();
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
                Some(gpu) if self.milkdrop_is_running() => {
                    self.milkdrop_build_task(gpu, generation, preset)
                }
                _ => {
                    // No device yet, or nobody would see it: the tick builds it
                    // once `prepare` has run and MilkDrop is running.
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
                // The name toast and the failure reset wait for the first frame
                // (`shown_generation`, read by the tick).
                self.milkdrop.build_in_flight = None;
                self.milkdrop_arm_timer();
                Task::none()
            }
        }
    }

    fn handle_milkdrop_next(&mut self) -> Task<Message> {
        if !self.milkdrop_is_running() {
            debug!("milkdrop: Next Preset ignored (MilkDrop not on screen and playing)");
            return Task::none();
        }
        // An explicit request retries even after the failure cap.
        self.milkdrop.consecutive_failures = 0;
        self.milkdrop.next_switch_at = None;
        self.milkdrop_advance(AdvanceReason::Manual)
    }

    fn handle_milkdrop_previous(&mut self) -> Task<Message> {
        if !self.milkdrop_is_running() {
            debug!("milkdrop: Previous Preset ignored (MilkDrop not on screen and playing)");
            return Task::none();
        }
        // Skip presets hidden (or removed) since they were shown.
        while let Some(previous) = self.milkdrop.history.pop() {
            if self.milkdrop.library.source(&previous).is_some()
                && !self.milkdrop.library.is_hidden(&previous)
            {
                self.milkdrop.consecutive_failures = 0;
                self.milkdrop.next_switch_at = None;
                return self.milkdrop_load(previous);
            }
        }
        Task::none()
    }

    fn handle_milkdrop_toggle_lock(&mut self) -> Task<Message> {
        if !self.milkdrop_mode_active() {
            debug!("milkdrop: Lock Preset ignored outside MilkDrop mode");
            return Task::none();
        }
        self.milkdrop.locked = !self.milkdrop.locked;
        if self.milkdrop.locked {
            self.milkdrop.next_switch_at = None;
            self.toast_info("MilkDrop: preset locked");
        } else {
            if self.milkdrop.build_in_flight.is_none() {
                self.milkdrop_arm_timer();
            }
            self.toast_info("MilkDrop: preset unlocked");
        }
        Task::none()
    }

    /// "Never Show This Preset": hide the preset on screen for good (persisted
    /// in `curation.toml`, logged for the repo cleanup) and move on.
    fn handle_milkdrop_hide(&mut self) -> Task<Message> {
        if !self.milkdrop_mode_active() {
            debug!("milkdrop: Hide ignored outside MilkDrop mode");
            return Task::none();
        }
        let Some(name) = self.milkdrop.on_screen.clone() else {
            return Task::none();
        };
        self.milkdrop.library.hide(&name);
        info!(preset = %name, "milkdrop: preset hidden");
        self.milkdrop_save_curation();
        self.toast_info(format!("MilkDrop: {name} won't show again"));
        if self.milkdrop_is_running() {
            self.milkdrop.consecutive_failures = 0;
            self.milkdrop.next_switch_at = None;
            let task = self.milkdrop_advance(AdvanceReason::Manual);
            // A hidden preset never belongs in Previous's history.
            self.milkdrop.history.retain(|n| *n != name);
            task
        } else {
            Task::none()
        }
    }

    fn handle_milkdrop_toggle_favorite(&mut self) -> Task<Message> {
        if !self.milkdrop_mode_active() {
            debug!("milkdrop: Favorite ignored outside MilkDrop mode");
            return Task::none();
        }
        let Some(name) = self.milkdrop.on_screen.clone() else {
            return Task::none();
        };
        let favorite = self.milkdrop.library.toggle_favorite(&name);
        self.milkdrop_save_curation();
        self.toast_info(if favorite {
            format!("MilkDrop: {name} favorited")
        } else {
            format!("MilkDrop: {name} unfavorited")
        });
        Task::none()
    }

    /// Persist hidden + favorite presets; a failed write is reported, never
    /// fatal (the in-memory curation still applies this session).
    fn milkdrop_save_curation(&mut self) {
        if self.milkdrop.curation_path.as_os_str().is_empty() {
            return;
        }
        if let Err(e) = self
            .milkdrop
            .library
            .curation()
            .save(&self.milkdrop.curation_path)
        {
            warn!("milkdrop: could not save curation: {e:#}");
            self.toast_warn("MilkDrop: could not save the preset list; see nokkvi.log");
        }
    }

    /// Re-read the user preset folder (the refresh key, in MilkDrop mode only).
    pub(crate) fn milkdrop_rescan_if_active(&mut self) {
        if self.milkdrop_mode_active() {
            self.milkdrop.library.rescan_user_dir();
            debug!(
                presets = self.milkdrop.library.len(),
                "milkdrop: rescanned the user preset folder"
            );
        }
    }

    /// Hook for a new track: switch presets when that setting is on.
    pub(crate) fn milkdrop_on_track_change(&mut self) -> Task<Message> {
        if self.milkdrop_is_running()
            && self.milkdrop_switch_on_track_change()
            && !self.milkdrop.locked
            && self.milkdrop.current.is_some()
        {
            self.milkdrop.next_switch_at = None;
            self.milkdrop_advance(AdvanceReason::TrackChange)
        } else {
            Task::none()
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
        if self.milkdrop_is_running() {
            self.milkdrop_advance(AdvanceReason::Failure)
        } else {
            // Nobody would see a retry; the tick loads one once running.
            if self.milkdrop.consecutive_failures >= MILKDROP_MAX_CONSECUTIVE_FAILURES {
                self.milkdrop_give_up();
            }
            self.milkdrop.current = None;
            Task::none()
        }
    }

    /// Stop auto-advancing after too many failures; warns exactly once.
    fn milkdrop_give_up(&mut self) {
        if self.milkdrop.consecutive_failures == MILKDROP_MAX_CONSECUTIVE_FAILURES {
            self.toast_warn("MilkDrop: no preset could be built; check nokkvi.log");
        }
        self.milkdrop.current = None;
        self.milkdrop.next_switch_at = None;
    }

    /// Pick the next preset and load it, unless too many builds failed in a
    /// row or nothing is eligible (each warns once).
    pub(crate) fn milkdrop_advance(&mut self, reason: AdvanceReason) -> Task<Message> {
        if self.milkdrop.consecutive_failures >= MILKDROP_MAX_CONSECUTIVE_FAILURES {
            self.milkdrop_give_up();
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
        // History records only presets that reached the screen.
        let current_was_shown = self.milkdrop.announced_generation == self.milkdrop.generation;
        if let Some(previous) = current
            && reason != AdvanceReason::Failure
            && current_was_shown
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
        self.milkdrop_apply_config();

        let running = self.milkdrop_is_running();
        self.milkdrop
            .shared
            .running
            .store(running, Ordering::Release);

        // `prepare` dropped the renderer after a GPU error: a failure like any
        // other (so a device that fails every preset stops at the cap).
        if self.milkdrop.shared.slot_lost.swap(false, Ordering::AcqRel) {
            self.milkdrop.build_in_flight = None;
            self.milkdrop.consecutive_failures =
                self.milkdrop.consecutive_failures.saturating_add(1);
            if self.milkdrop.consecutive_failures >= MILKDROP_MAX_CONSECUTIVE_FAILURES {
                self.milkdrop_give_up();
            }
            self.milkdrop.current = None;
        }

        // The current load reached the screen: announce it once, and only now
        // count the preset as working.
        let shown = self
            .milkdrop
            .shared
            .shown_generation
            .load(Ordering::Acquire);
        if shown == self.milkdrop.generation && shown != self.milkdrop.announced_generation {
            self.milkdrop.announced_generation = shown;
            self.milkdrop.consecutive_failures = 0;
            self.milkdrop.empty_warned = false;
            if let Some(name) = self.milkdrop.current.clone() {
                info!(preset = %name, "milkdrop: preset on screen");
                self.milkdrop.on_screen = Some(name.clone());
                if self.milkdrop_config().show_preset_names {
                    self.toast_info(name);
                }
            }
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
                _ if self.milkdrop.build_in_flight != Some(generation) => {}
                Some(gpu) if running => {
                    tasks.push(self.milkdrop_build_task(gpu, generation, preset));
                }
                _ => self.milkdrop.awaiting_gpu = Some((generation, preset)),
            }
        }

        if !running {
            self.milkdrop.next_switch_at = None;
        } else if self.milkdrop.current.is_none()
            && self.milkdrop.build_in_flight.is_none()
            && self.milkdrop.consecutive_failures < MILKDROP_MAX_CONSECUTIVE_FAILURES
        {
            tasks.push(self.milkdrop_advance(AdvanceReason::Enter));
        } else if self.milkdrop.build_in_flight.is_none() && self.milkdrop.current.is_some() {
            match self.milkdrop.next_switch_at {
                None if !self.milkdrop.locked => self.milkdrop_arm_timer(),
                None => {}
                Some(at) if Instant::now() >= at => {
                    // Disarm first: re-firing every tick until the build lands
                    // would bump the generation each time and starve it.
                    self.milkdrop.next_switch_at = None;
                    tasks.push(self.milkdrop_advance(AdvanceReason::Timer));
                }
                Some(_) => {}
            }
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
