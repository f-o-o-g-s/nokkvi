//! The equalizer's shared state: what the UI sets and every stream's
//! [`EqProcessor`](super::EqProcessor) reads.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering},
};

use arc_swap::ArcSwap;
use serde::{Deserialize, Serialize};

use super::{
    EQ_BAND_COUNT, EQ_MAX_BANDS,
    band::{EqBand, graphic_bands},
    clamp_gain,
};

/// Which layout of the band list plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EqMode {
    /// The ten sliders: ten peaks at the ISO centres.
    #[default]
    Graphic,
    /// A free band list: peaks, shelves and pass filters.
    Parametric,
}

impl EqMode {
    pub const ALL: [Self; 2] = [Self::Graphic, Self::Parametric];

    pub fn label(self) -> &'static str {
        match self {
            Self::Graphic => "Graphic",
            Self::Parametric => "Parametric",
        }
    }

    fn from_byte(byte: u8) -> Self {
        if byte == Self::Parametric as u8 {
            Self::Parametric
        } else {
            Self::Graphic
        }
    }
}

impl std::fmt::Display for EqMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug)]
struct Shared {
    /// Master bypass.
    enabled: AtomicBool,
    /// The playing layout, an [`EqMode`] discriminant.
    mode: AtomicU8,
    /// The Graphic layout: per-slider gain in dB as `f32::to_bits`, held to
    /// `EQ_GAIN_DB_MIN..=EQ_GAIN_DB_MAX`.
    gains: [AtomicU32; EQ_BAND_COUNT],
    /// The Parametric layout: clamped bands, at most [`EQ_MAX_BANDS`].
    parametric: ArcSwap<Vec<EqBand>>,
    /// Counts every change above. A processor compares it once per check
    /// interval (one atomic load) and reads the rest only when it moved.
    generation: AtomicU64,
}

/// The equalizer as the user set it, shared lock-free between the UI thread
/// (the only writer) and the audio thread. Cloned into every stream.
///
/// It holds both layouts, so switching the mode keeps the other one's
/// values. Every setter that changes something bumps the generation after
/// its write: a processor that read a half-written state sees the generation
/// move again and rebuilds from the finished one.
#[derive(Clone, Debug)]
pub struct EqState {
    shared: Arc<Shared>,
}

impl Default for EqState {
    fn default() -> Self {
        Self::new()
    }
}

impl EqState {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Shared {
                enabled: AtomicBool::new(false),
                mode: AtomicU8::new(EqMode::Graphic as u8),
                gains: std::array::from_fn(|_| AtomicU32::new(0f32.to_bits())),
                parametric: ArcSwap::from_pointee(Vec::new()),
                generation: AtomicU64::new(0),
            }),
        }
    }

    fn bump(&self) {
        self.shared.generation.fetch_add(1, Ordering::Release);
    }

    /// The change counter: moves whenever a setter changed something.
    pub fn generation(&self) -> u64 {
        self.shared.generation.load(Ordering::Acquire)
    }

    pub fn set_enabled(&self, enabled: bool) {
        if self.shared.enabled.swap(enabled, Ordering::Relaxed) != enabled {
            self.bump();
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.shared.enabled.load(Ordering::Relaxed)
    }

    /// The layout that plays.
    pub fn mode(&self) -> EqMode {
        EqMode::from_byte(self.shared.mode.load(Ordering::Relaxed))
    }

    pub fn set_mode(&self, mode: EqMode) {
        if self.shared.mode.swap(mode as u8, Ordering::Relaxed) != mode as u8 {
            self.bump();
        }
    }

    /// Set one Graphic slider's gain in dB.
    pub fn set_band_gain(&self, band: usize, gain_db: f32) {
        if let Some(slot) = self.shared.gains.get(band) {
            let bits = clamp_gain(gain_db).to_bits();
            if slot.swap(bits, Ordering::Relaxed) != bits {
                self.bump();
            }
        }
    }

    /// Set all ten Graphic sliders at once (a preset).
    pub fn set_all_gains(&self, gains: &[f32; EQ_BAND_COUNT]) {
        let mut changed = false;
        for (slot, &gain) in self.shared.gains.iter().zip(gains) {
            let bits = clamp_gain(gain).to_bits();
            changed |= slot.swap(bits, Ordering::Relaxed) != bits;
        }
        if changed {
            self.bump();
        }
    }

    /// One Graphic slider's gain in dB (0 for a band that doesn't exist).
    pub fn get_band_gain(&self, band: usize) -> f32 {
        self.shared
            .gains
            .get(band)
            .map_or(0.0, |slot| f32::from_bits(slot.load(Ordering::Relaxed)))
    }

    /// The ten Graphic slider gains in dB.
    pub fn graphic_gains(&self) -> [f32; EQ_BAND_COUNT] {
        std::array::from_fn(|band| self.get_band_gain(band))
    }

    /// The Parametric layout's band list.
    pub fn parametric_bands(&self) -> Arc<Vec<EqBand>> {
        self.shared.parametric.load_full()
    }

    /// Replace the Parametric layout's band list. Every band is clamped and
    /// the list cut at [`EQ_MAX_BANDS`].
    pub fn set_parametric_bands(&self, bands: &[EqBand]) {
        let bands: Vec<EqBand> = bands
            .iter()
            .take(EQ_MAX_BANDS)
            .map(|band| band.clamped())
            .collect();
        if **self.shared.parametric.load() == bands {
            return;
        }
        self.shared.parametric.store(Arc::new(bands));
        self.bump();
    }

    /// The band list of the layout that plays, whether or not the equalizer
    /// is enabled: what the editor draws its curve from.
    pub fn active_bands(&self) -> Vec<EqBand> {
        match self.mode() {
            EqMode::Graphic => graphic_bands(&self.graphic_gains()).to_vec(),
            EqMode::Parametric => self.shared.parametric.load().as_ref().clone(),
        }
    }

    /// Copy the band list a stream should run into `out` and return its
    /// length: the playing layout, or nothing while the equalizer is off.
    /// For the audio thread: takes no lock and allocates nothing.
    pub(super) fn snapshot_into(&self, out: &mut [EqBand; EQ_MAX_BANDS]) -> usize {
        if !self.is_enabled() {
            return 0;
        }
        match self.mode() {
            EqMode::Graphic => {
                let bands = graphic_bands(&self.graphic_gains());
                out[..EQ_BAND_COUNT].copy_from_slice(&bands);
                EQ_BAND_COUNT
            }
            EqMode::Parametric => {
                let bands = self.shared.parametric.load();
                let len = bands.len().min(EQ_MAX_BANDS);
                out[..len].copy_from_slice(&bands[..len]);
                len
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        super::{EQ_GAIN_DB_MAX, EQ_GAIN_DB_MIN, EqBandKind},
        *,
    };

    /// Gains are held to the shared bounds, and a NaN never reaches the
    /// filter bank (it would turn every sample of the band into NaN).
    #[test]
    fn band_gains_are_clamped_and_never_nan() {
        let state = EqState::new();
        state.set_band_gain(0, 15.0);
        state.set_band_gain(1, -15.0);
        state.set_band_gain(2, f32::NAN);
        assert_eq!(state.get_band_gain(0), EQ_GAIN_DB_MAX);
        assert_eq!(state.get_band_gain(1), EQ_GAIN_DB_MIN);
        assert!(state.get_band_gain(2).is_finite());

        let mut gains = [0.0; EQ_BAND_COUNT];
        gains[3] = 99.0;
        gains[4] = f32::NAN;
        state.set_all_gains(&gains);
        assert_eq!(state.get_band_gain(3), EQ_GAIN_DB_MAX);
        assert!(state.get_band_gain(4).is_finite());
        assert_eq!(state.get_band_gain(EQ_BAND_COUNT), 0.0);
    }

    #[test]
    fn the_generation_moves_only_when_something_changed() {
        let state = EqState::new();
        let start = state.generation();
        state.set_enabled(false);
        state.set_mode(EqMode::Graphic);
        state.set_band_gain(0, 0.0);
        state.set_all_gains(&[0.0; EQ_BAND_COUNT]);
        state.set_parametric_bands(&[]);
        assert_eq!(state.generation(), start, "nothing changed");

        state.set_enabled(true);
        state.set_mode(EqMode::Parametric);
        state.set_band_gain(0, 3.0);
        state.set_all_gains(&[1.0; EQ_BAND_COUNT]);
        let band = EqBand::new(EqBandKind::Peak, 1000.0, 3.0, 1.0);
        state.set_parametric_bands(&[band]);
        assert_eq!(state.generation(), start + 5);
        state.set_parametric_bands(&[band]);
        assert_eq!(state.generation(), start + 5, "the same list again");
    }

    #[test]
    fn parametric_bands_are_clamped_and_capped() {
        let state = EqState::new();
        let wild = EqBand::new(EqBandKind::Peak, 5.0, 99.0, 0.0);
        state.set_parametric_bands(&vec![wild; EQ_MAX_BANDS + 4]);
        let bands = state.parametric_bands();
        assert_eq!(bands.len(), EQ_MAX_BANDS);
        assert_eq!(bands[0], wild.clamped());
    }

    /// Each layout keeps its values while the other plays, and a stream is
    /// handed the playing one only while the equalizer is on.
    #[test]
    fn the_mode_picks_the_layout_and_off_hands_out_nothing() {
        let state = EqState::new();
        state.set_band_gain(1, 5.0);
        let shelf = EqBand::new(EqBandKind::LowShelf, 105.0, -4.0, 0.7);
        state.set_parametric_bands(&[shelf]);

        let mut out = [EqBand::new(EqBandKind::Peak, 1000.0, 0.0, 1.0); EQ_MAX_BANDS];
        assert_eq!(state.snapshot_into(&mut out), 0, "off");
        assert_eq!(state.active_bands().len(), EQ_BAND_COUNT);

        state.set_enabled(true);
        assert_eq!(state.snapshot_into(&mut out), EQ_BAND_COUNT);
        assert_eq!(out[1].gain, 5.0);

        state.set_mode(EqMode::Parametric);
        assert_eq!(state.snapshot_into(&mut out), 1);
        assert_eq!(out[0], shelf);
        assert_eq!(state.active_bands(), vec![shelf]);
        assert_eq!(state.get_band_gain(1), 5.0, "the sliders keep their values");
    }
}
