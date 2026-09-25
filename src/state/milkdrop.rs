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
    /// Stem of the preset being built, or on screen once built.
    pub current: Option<String>,
    /// Stem of the preset whose frames are on screen now; what Hide and
    /// Favorite act on (it lags `current` while a replacement builds).
    pub on_screen: Option<String>,
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
    /// Why the curation file could not be read; while set, the file is never
    /// written (it would overwrite the user's hand edits).
    pub curation_error: Option<String>,
    /// The current preset names theme colours (`NOKKVI_*` tokens), so a
    /// palette change reloads it.
    pub current_themed: bool,
    /// The palette the current load was coloured with.
    pub palette_used: Option<crate::widgets::visualizer::milkdrop::palette::PresetPalette>,
    /// `theme_generation()` when the tick last compared palettes.
    pub theme_generation_seen: u64,
    /// The theme changed while a themed preset could not reload (paused, off
    /// the panel); compare once MilkDrop runs again.
    pub palette_check_pending: bool,
    /// The artwork handle whose cover is being decoded for the engine.
    pub cover_pending: Option<iced::advanced::image::Id>,
    /// The artwork handle whose cover the engine last received (or that
    /// failed to decode), so each cover is decoded once.
    pub cover_sent: Option<iced::advanced::image::Id>,
    /// The switch interval the current timer was armed with; a different
    /// live setting re-arms it.
    pub armed_interval: Option<std::time::Duration>,
}

impl Default for MilkdropState {
    fn default() -> Self {
        Self {
            shared: Arc::new(MilkdropShared::new()),
            library: PresetLibrary::default(),
            current: None,
            on_screen: None,
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
            curation_error: None,
            current_themed: false,
            palette_used: None,
            theme_generation_seen: 0,
            palette_check_pending: false,
            cover_pending: None,
            cover_sent: None,
            armed_interval: None,
        }
    }
}
