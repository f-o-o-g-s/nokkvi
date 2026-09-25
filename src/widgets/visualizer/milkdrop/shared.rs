//! State shared between the app, the FFT worker, the preset builder thread and
//! the MilkDrop render pipeline.

use std::{
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use iced::wgpu;
use parking_lot::Mutex;
use particle_milkdrop::MilkdropRenderer;

/// Shorter-side render cap (physical px) before the Render Quality setting is
/// first applied (the `medium` value).
pub(crate) const DEFAULT_QUALITY_SHORT_SIDE: u32 = 720;

/// A panel counts as on screen while its `prepare` ran this recently. While
/// playing, the 100 ms progress updates redraw the window, so a mounted panel
/// reports well inside this.
pub(crate) const MOUNTED_WINDOW: Duration = Duration::from_millis(500);

/// Milliseconds since the first call (a process-local monotonic clock that
/// fits an atomic). Never 0, so 0 can mean "never".
fn now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    let start = *START.get_or_init(Instant::now);
    u64::try_from(start.elapsed().as_millis())
        .unwrap_or(u64::MAX)
        .saturating_add(1)
}

/// Size the builder uses before the panel has reported one.
pub(crate) const FALLBACK_RENDER_SIZE: (u32, u32) = (720, 720);

/// iced's device + queue, captured in `prepare` so presets can be built off the
/// render thread. Replaced when `prepare` sees a different device (hiding to the
/// tray and showing again opens a new window with a new device); `epoch` bumps
/// on every replacement and every build carries the epoch it used.
#[derive(Clone)]
pub(crate) struct GpuHandles {
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    pub format: wgpu::TextureFormat,
    pub epoch: u64,
}

/// A finished renderer waiting for `prepare` to swap it in.
pub(crate) struct BuiltPreset {
    pub generation: u64,
    pub epoch: u64,
    /// The cover version the builder already set (so `prepare` skips it).
    pub cover_version: u64,
    pub name: String,
    pub renderer: MilkdropRenderer,
}

/// One per session: created with the app, handed to the visualizer at login so
/// the app, the FFT worker (features) and the render pipeline meet here.
pub struct MilkdropShared {
    /// Playing, not paused, MilkDrop mode, a panel on screen. The app sets it
    /// every tick; `prepare` advances the renderer only while it is set.
    pub(crate) running: AtomicBool,
    /// Release watermark: every renderer (on screen or waiting) whose load
    /// generation is below this is dropped. The app raises it on leaving the
    /// mode, stop and logout; the pipeline enforces it in `trim()` (every
    /// frame, mounted or not) and at the top of `prepare`. A watermark rather
    /// than a one-shot flag, so a release that no frame saw can never drop a
    /// renderer built by a LATER load.
    pub(crate) released_below: AtomicU64,
    /// The load generation of a renderer `prepare` dropped after a GPU error
    /// (0 = none). The app's tick takes it and blames that load only: after a
    /// switch the lost renderer may be the OLD preset, not the one building.
    pub(crate) slot_lost: AtomicU64,
    /// The load generation whose renderer `prepare` last drew a first frame
    /// for. The app toasts the name and clears the failure count off this, so
    /// both follow what is really on screen.
    pub(crate) shown_generation: AtomicU64,
    /// `now_ms()` of the last `prepare`: proof a MilkDrop panel is mounted
    /// (Artwork Column Never, a narrow window or a split view draw none).
    last_prepare_ms: AtomicU64,
    /// Bumps on every device capture; never reset, so a device captured after
    /// a release can never share an epoch with a build for the old one.
    pub(crate) epoch_counter: AtomicU64,
    /// The app's newest load generation; the builder and `prepare` refuse
    /// anything older.
    pub(crate) current_generation: AtomicU64,
    /// Shorter-side render cap in physical px; 0 = native.
    pub(crate) quality_short_side: AtomicU32,
    /// `analyzer.analysis_sample_rate()` as `f32` bits (0 = no analyzer yet).
    /// Written by the FFT thread on (re)init, read when a renderer is swapped in.
    analysis_rate: AtomicU32,
    /// Latest analysis frame. The FFT thread writes with `try_lock`; the render
    /// thread may `lock()`.
    pub(crate) features: Mutex<particle_audio::Features>,
    /// See [`GpuHandles`]. The panel's `shader::Program` is generic over the
    /// host view's message type and cannot publish a root message, so the app's
    /// 100 ms tick polls this instead.
    pub(crate) gpu: Mutex<Option<GpuHandles>>,
    /// A finished renderer waiting for `prepare`.
    pub(crate) built: Mutex<Option<BuiltPreset>>,
    /// The size `prepare` last wanted, so the builder constructs at that size.
    pub(crate) render_size: Mutex<(u32, u32)>,
    /// The playing cover for presets that sample `cover`; replaced (never
    /// mutated) on each new cover, with `cover_version` bumped after.
    pub(crate) cover: Mutex<Option<Arc<super::CoverImage>>>,
    cover_version: AtomicU64,
}

impl MilkdropShared {
    pub(crate) fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            released_below: AtomicU64::new(0),
            slot_lost: AtomicU64::new(0),
            shown_generation: AtomicU64::new(0),
            last_prepare_ms: AtomicU64::new(0),
            epoch_counter: AtomicU64::new(0),
            current_generation: AtomicU64::new(0),
            quality_short_side: AtomicU32::new(DEFAULT_QUALITY_SHORT_SIDE),
            analysis_rate: AtomicU32::new(0),
            features: Mutex::new(particle_audio::Features::default()),
            gpu: Mutex::new(None),
            built: Mutex::new(None),
            render_size: Mutex::new(FALLBACK_RENDER_SIZE),
            cover: Mutex::new(None),
            cover_version: AtomicU64::new(0),
        }
    }

    /// The analyzer's decimated rate in Hz, or `None` before the first analysis.
    pub(crate) fn analysis_rate(&self) -> Option<f32> {
        let bits = self.analysis_rate.load(Ordering::Acquire);
        (bits != 0).then(|| f32::from_bits(bits))
    }

    pub(crate) fn set_analysis_rate(&self, hz: f32) {
        self.analysis_rate.store(hz.to_bits(), Ordering::Release);
    }

    /// Publish a new cover; the renderer on screen picks it up next frame.
    pub(crate) fn publish_cover(&self, cover: Arc<super::CoverImage>) {
        *self.cover.lock() = Some(cover);
        self.cover_version.fetch_add(1, Ordering::AcqRel);
    }

    /// Forget the cover (logout).
    pub(crate) fn clear_cover(&self) {
        *self.cover.lock() = None;
        self.cover_version.fetch_add(1, Ordering::AcqRel);
    }

    /// Bumps on every published cover (0 = none yet).
    pub(crate) fn cover_version(&self) -> u64 {
        self.cover_version.load(Ordering::Acquire)
    }

    /// Called by `prepare` every frame the panel draws.
    pub(crate) fn mark_mounted(&self) {
        self.last_prepare_ms.store(now_ms(), Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn forget_mounted(&self) {
        self.last_prepare_ms.store(0, Ordering::Release);
    }

    /// Whether a MilkDrop panel drew within [`MOUNTED_WINDOW`].
    pub(crate) fn mounted_recently(&self) -> bool {
        let last = self.last_prepare_ms.load(Ordering::Acquire);
        last != 0
            && now_ms().saturating_sub(last)
                <= u64::try_from(MOUNTED_WINDOW.as_millis()).unwrap_or(u64::MAX)
    }

    /// Called by `prepare` after a swapped-in renderer's first frame.
    pub(crate) fn mark_shown(&self, generation: u64) {
        self.shown_generation.store(generation, Ordering::Release);
    }

    pub(crate) fn gpu_handles(&self) -> Option<GpuHandles> {
        self.gpu.lock().clone()
    }

    pub(crate) fn gpu_epoch(&self) -> Option<u64> {
        self.gpu.lock().as_ref().map(|g| g.epoch)
    }

    /// Hand a finished renderer to `prepare`, unless a newer load (or a release)
    /// superseded it while it was building. Checked under the `built` lock, so
    /// a generation bump that lands first always wins. Returns whether it was
    /// kept; a refused renderer drops here, on the builder thread.
    pub(crate) fn offer_built(&self, built: BuiltPreset) -> bool {
        let mut slot = self.built.lock();
        if built.generation == self.current_generation.load(Ordering::Acquire) {
            let displaced = slot.replace(built);
            // Drop a displaced renderer (~20 pipelines) after unlocking, so the
            // render thread never waits on its teardown.
            drop(slot);
            drop(displaced);
            true
        } else {
            false
        }
    }
}

impl Default for MilkdropShared {
    fn default() -> Self {
        Self::new()
    }
}

// Manual: `Features` is ~6 KB of arrays and the renderer has no `Debug`.
impl std::fmt::Debug for MilkdropShared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MilkdropShared")
            .field("running", &self.running.load(Ordering::Relaxed))
            .field(
                "generation",
                &self.current_generation.load(Ordering::Relaxed),
            )
            .field("analysis_rate", &self.analysis_rate())
            .finish_non_exhaustive()
    }
}
