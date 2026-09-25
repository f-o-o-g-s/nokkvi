//! State shared between the app, the FFT worker and the MilkDrop render pipeline.

use std::sync::atomic::{AtomicU32, Ordering};

use parking_lot::Mutex;

/// One per session: created with the app, handed to the visualizer at login so
/// the FFT worker (writer) and the render pipeline (reader) meet here.
pub struct MilkdropShared {
    /// `analyzer.analysis_sample_rate()` as `f32` bits (0 = no analyzer yet).
    /// Written by the FFT thread on (re)init, read when a renderer is swapped in.
    analysis_rate: AtomicU32,
    /// Latest analysis frame. The FFT thread writes with `try_lock`; the render
    /// thread may `lock()`.
    pub features: Mutex<particle_audio::Features>,
}

impl MilkdropShared {
    pub(crate) fn new() -> Self {
        Self {
            analysis_rate: AtomicU32::new(0),
            features: Mutex::new(particle_audio::Features::default()),
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
}

impl Default for MilkdropShared {
    fn default() -> Self {
        Self::new()
    }
}

// Manual: `Features` is ~6 KB of arrays; print the rate only.
impl std::fmt::Debug for MilkdropShared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MilkdropShared")
            .field("analysis_rate", &self.analysis_rate())
            .finish_non_exhaustive()
    }
}
