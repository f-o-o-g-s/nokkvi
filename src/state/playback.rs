//! Playback state: the active playback source, player-bar fields, and modes.

/// What is currently driving audio output.
/// `Queue` = normal library playback. `Radio` = direct internet radio stream.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ActivePlayback {
    #[default]
    Queue,
    Radio(RadioPlaybackState),
}

/// Transient state for an active radio stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RadioPlaybackState {
    pub station: nokkvi_data::types::radio_station::RadioStation,
    pub icy_artist: Option<String>,
    pub icy_title: Option<String>,
    pub icy_url: Option<String>,
}

impl ActivePlayback {
    pub fn is_radio(&self) -> bool {
        matches!(self, Self::Radio(_))
    }

    pub fn is_queue(&self) -> bool {
        matches!(self, Self::Queue)
    }

    pub fn radio_station(&self) -> Option<&nokkvi_data::types::radio_station::RadioStation> {
        match self {
            Self::Radio(state) => Some(&state.station),
            Self::Queue => None,
        }
    }

    /// Extract standard metadata for the Top Nav bar, overriding with
    /// radio metadata if a radio stream is active.
    pub fn nav_metadata(&self, fallback: &PlaybackState) -> (String, String, String) {
        match self {
            Self::Radio(state) => (
                state.station.name.clone(),
                "Radio".to_string(),
                state.station.stream_url.clone(),
            ),
            Self::Queue => (
                fallback.title.clone(),
                fallback.artist.clone(),
                fallback.album.clone(),
            ),
        }
    }
}

/// Playback-related state for the player bar
#[derive(Debug, Clone)]
pub struct PlaybackState {
    pub position: u32,
    pub duration: u32,
    pub playing: bool,
    pub paused: bool,
    pub title: String,
    pub artist: String,
    /// Album name of the currently playing track
    pub album: String,
    pub volume: f32,
    /// Audio format suffix (e.g., "flac", "mp3", "opus")
    pub format_suffix: String,
    /// Sample rate in Hz (e.g., 44100, 48000, 96000)
    pub sample_rate: u32,
    /// Bitrate in kbps (e.g., 320, 1411)
    pub bitrate: u32,
    /// Throttle timestamp for volume persistence to storage
    pub volume_persist_throttle: Option<std::time::Instant>,
    /// The last title successfully sent to PipeWire via IPC, to prevent redundant cross-thread FFI calls
    pub pw_last_title: Option<String>,
    /// Shared EQ state — gains and enabled flag. Read by audio thread, written by UI.
    pub eq_state: nokkvi_data::audio::EqState,
    /// Tagged BPM of the currently playing song, when the file/server
    /// reports one. The boat handler reads this every tick to drive
    /// beat-locked sail-thrust pulses; absence falls back to the
    /// spectral-flux onset envelope.
    pub bpm: Option<u32>,
}

impl PlaybackState {
    /// Whether a track is actively loaded (playing or paused).
    pub fn has_track(&self) -> bool {
        self.playing || self.paused
    }
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            position: 0,
            duration: 0,
            playing: false,
            paused: false,
            title: "Not Playing".to_string(),
            artist: String::new(),
            album: String::new(),
            volume: 1.0,
            format_suffix: String::new(),
            sample_rate: 0,
            bitrate: 0,
            volume_persist_throttle: None,
            pw_last_title: None,
            eq_state: nokkvi_data::audio::EqState::default(),
            bpm: None,
        }
    }
}

/// Playback modes (random, repeat, consume) — persisted via AppService
#[derive(Debug, Clone, Default)]
pub struct PlaybackModes {
    pub random: bool,
    pub repeat: bool,
    pub repeat_queue: bool,
    pub consume: bool,
}
