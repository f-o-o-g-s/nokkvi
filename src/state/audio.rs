//! Audio-engine transient state.

/// Audio engine transient state. The engine-related settings (visualizer
/// mode, crossfade, bit-perfect, normalization) live on `Nokkvi.settings`
/// only: the player-bar toggles write them there and every reader reads
/// them there, so there is no copy to fall out of step.
#[derive(Debug, Clone, Default)]
pub struct EngineState {
    /// Set once the gapless prep for the current track has been requested;
    /// cleared on the next track or queue-position change, and whenever the
    /// engine's next-track epoch moves (the prepared track was thrown away).
    pub gapless_preparing: bool,
    /// The engine's next-track epoch as of the last tick.
    pub next_track_epoch: u64,
}
