//! MilkDrop mode state (`Nokkvi.milkdrop`): which preset is on screen, the
//! load pipeline's bookkeeping, and the handle shared with the render side.
//! Transient: nothing here is persisted except the curation file.

use std::{path::PathBuf, sync::Arc, time::Instant};

use nokkvi_data::services::milkdrop_presets::PresetLibrary;

use crate::widgets::visualizer::milkdrop::{CompiledPreset, MilkdropShared};

/// How many presets Previous can walk back through.
pub(crate) const MILKDROP_HISTORY_CAP: usize = 50;

/// After this many builds fail in a row, auto-advance stops with one warning.
pub(crate) const MILKDROP_MAX_CONSECUTIVE_FAILURES: u8 = 5;

/// Manual `Default`: the shared handle is an `Arc::new`, and the library
/// starts EMPTY with empty paths so no `test_app()` ever reads the real
/// `~/.config/nokkvi/milkdrop/`. Login builds the real library.
#[derive(Debug)]
pub struct MilkdropState {
    pub shared: Arc<MilkdropShared>,
    pub library: PresetLibrary,
    /// Stem of the preset on screen (or being built).
    pub current: Option<String>,
    /// For Previous; capped at [`MILKDROP_HISTORY_CAP`].
    pub history: Vec<String>,
    pub locked: bool,
    /// Bumps on every load request AND every release; mirrored to
    /// `shared.current_generation`.
    pub generation: u64,
    /// The generation whose `Compiled` / `Built` is still pending.
    pub build_in_flight: Option<u64>,
    /// A compiled preset that arrived before `prepare` captured the GPU.
    pub awaiting_gpu: Option<(u64, Arc<CompiledPreset>)>,
    /// The GPU epoch the tick last saw; a change means the device was replaced.
    pub gpu_epoch_seen: u64,
    /// The load generation whose name was last toasted (its first frame was
    /// drawn); equals `generation` while the current preset is on screen.
    pub announced_generation: u64,
    pub next_switch_at: Option<Instant>,
    pub consecutive_failures: u8,
    /// Set once the "nothing eligible" warning has shown, so the 100 ms tick
    /// does not repeat it; cleared by the next successful build.
    pub empty_warned: bool,
    /// The user's preset directory and the curation file. Injectable so tests
    /// use a temp dir.
    pub user_dir: PathBuf,
    pub curation_path: PathBuf,
}

impl Default for MilkdropState {
    fn default() -> Self {
        Self {
            shared: Arc::new(MilkdropShared::new()),
            library: PresetLibrary::default(),
            current: None,
            history: Vec::new(),
            locked: false,
            generation: 0,
            build_in_flight: None,
            awaiting_gpu: None,
            gpu_epoch_seen: 0,
            announced_generation: 0,
            next_switch_at: None,
            consecutive_failures: 0,
            empty_warned: false,
            user_dir: PathBuf::new(),
            curation_path: PathBuf::new(),
        }
    }
}
