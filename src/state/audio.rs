//! SFX and audio-engine transient state (visualization, gapless, crossfade).

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

/// Audio engine transient state (visualization, gapless, crossfade)
#[derive(Debug, Clone, Default)]
pub struct EngineState {
    pub visualization_mode: nokkvi_data::types::player_settings::VisualizationMode,
    pub gapless_preparing: bool,
    /// Whether crossfade between tracks is enabled
    pub crossfade_enabled: bool,
    /// Crossfade duration in seconds (1–12)
    pub crossfade_duration_secs: u32,
    /// Volume normalization mode (Off / AGC / ReplayGain-track / ReplayGain-album)
    pub volume_normalization: nokkvi_data::types::player_settings::VolumeNormalizationMode,
    /// AGC target level — only meaningful when `volume_normalization == Agc`
    pub normalization_level: nokkvi_data::types::player_settings::NormalizationLevel,
    /// Pre-amp dB applied on top of resolved ReplayGain
    pub replay_gain_preamp_db: f32,
    /// Fallback dB for tracks with no ReplayGain tags (default 0.0 = unity)
    pub replay_gain_fallback_db: f32,
    /// When true, untagged tracks fall through to AGC instead of the fallback dB
    pub replay_gain_fallback_to_agc: bool,
    /// When true, clamp gain so `peak * gain <= 1.0`
    pub replay_gain_prevent_clipping: bool,
}
