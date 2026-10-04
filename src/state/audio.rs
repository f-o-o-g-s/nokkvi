//! Audio-engine transient state.

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
