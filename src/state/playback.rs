//! Playback state: the active playback source, player-bar fields, and modes.

/// Honest bit-perfect status for the now-playing indicator. Derived from the
/// bit-perfect mode, the track's rate, and the REAL device clock rate (read
/// from `/proc/asound`, not nokkvi's requested rate — which lies when PipeWire
/// resamples on a live down-switch).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BitPerfectStatus {
    /// Bit-perfect mode is off (standard mixed output).
    #[default]
    Off,
    /// Mode on and the device clock matches the track rate — bit-perfect to the DAC.
    Verified,
    /// Mode on but the device is clocked at a different rate, so PipeWire is
    /// resampling (e.g. a live 96k→44.1k down-switch the DAC can't follow).
    Resampled { device_rate: u32 },
    /// Mode on but the device rate hasn't been determined yet — the TRANSIENT
    /// state between a track/play change and the off-thread probe landing. The
    /// badge stays hidden so it never flashes a stale verdict mid-transition.
    Unknown,
    /// Mode on but the real device rate can't be read AT ALL — the probe found
    /// no open ALSA hardware PCM for the sink: a Bluetooth sink (which re-encodes,
    /// so it can't be bit-perfect), an idle/suspended device, or several devices
    /// open at once. Shows an "UNVERIFIED" hint with the reason.
    Unverifiable,
}

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
    /// Honest bit-perfect status for the now-playing indicator (recomputed each
    /// snapshot from the mode + track rate + the real device clock rate).
    pub bit_perfect_status: BitPerfectStatus,
    /// Whether the honest bit-perfect badge is currently live for the playing
    /// track (the stream was built bit-perfect, playback is active, and it is
    /// queue — not radio). Recomputed each snapshot and used as the staleness
    /// guard for the off-thread device-rate probe result.
    pub bit_perfect_engaged: bool,
    /// Countdown (in 100ms ticks) until the next `/proc/asound` device-rate
    /// re-probe. Throttles the blocking walk off the hot path while still
    /// self-healing the asynchronous device re-clock — see the handler.
    pub bit_perfect_probe_ticks: u8,
    /// Monotonic id of the most recently dispatched device-rate probe. Each
    /// off-thread probe carries the id it was dispatched under; the result is
    /// applied only if it still matches, so an older probe that resolves out of
    /// order (the blocking pool gives no ordering) can't clobber a fresher one.
    pub bit_perfect_probe_generation: u64,
    /// When resampled, the app holding the output device at a different rate
    /// (from the PipeWire graph probe), shown inline as `RESAMPLED→96k · Zen`.
    /// `None` when verified, on Bluetooth, or when the graph couldn't be read.
    pub bit_perfect_holder: Option<String>,
    /// Consecutive device-rate probes that came back unreadable since the last
    /// transition. PipeWire opens the hardware PCM asynchronously after a sink
    /// rebuild, so the FIRST probe(s) right after a track/play change can read
    /// nothing even on a wired DAC about to verify. The badge stays `Unknown`
    /// (hidden) until this reaches the grace threshold, so a genuine
    /// settling-in window never flashes a false "UNVERIFIED"; a truly
    /// unreadable sink (Bluetooth / idle) still settles to `Unverifiable`.
    pub bit_perfect_unverifiable_streak: u8,
    /// Countdown (in dispatched device-rate probes) until the resampled
    /// HOLDER name is re-derived from the PipeWire graph. Re-opening a PipeWire
    /// client + enumerating the whole graph every ~1s while steadily resampled
    /// is wasteful, so the cheap `/proc/asound` rate read runs each cycle while
    /// the holder name is reused from cache between re-derives — but bounded, so
    /// a holder that closes / is replaced at the same device rate self-corrects
    /// within a few seconds instead of lingering stale for the whole episode.
    pub bit_perfect_holder_reprobe_ticks: u8,
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
            bit_perfect_status: BitPerfectStatus::Off,
            bit_perfect_engaged: false,
            bit_perfect_probe_ticks: 0,
            bit_perfect_probe_generation: 0,
            bit_perfect_holder: None,
            bit_perfect_unverifiable_streak: 0,
            bit_perfect_holder_reprobe_ticks: 0,
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
