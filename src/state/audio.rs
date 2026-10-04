//! SFX and audio-engine transient state.

/// Sound effects engine state
#[derive(Debug, Clone)]
pub struct SfxState {
    pub enabled: bool,
    pub volume: f32,
}

impl Default for SfxState {
    fn default() -> Self {
        Self {
            enabled: true,
            volume: 0.68,
        }
    }
}

/// Audio engine transient state. The engine-related settings (visualizer
/// mode, crossfade, bit-perfect, normalization) live on `Nokkvi.settings`
/// only: the player-bar toggles write them there and every reader reads
/// them there, so there is no copy to fall out of step.
#[derive(Debug, Clone, Default)]
pub struct EngineState {
    /// Set once the gapless prep for the current track has been requested;
    /// cleared on the next track or queue-position change.
    pub gapless_preparing: bool,
}
