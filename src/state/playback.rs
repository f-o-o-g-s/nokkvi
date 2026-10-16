//! Playback state: the active playback source, player-bar fields, and modes.

use nokkvi_data::types::queue::RepeatMode;

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

/// The station a play attempt switched away from (see `guard_play_action`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StationLeftForPlay {
    /// The `play_attempt` whose guard switched away from the station.
    pub attempt: u64,
    pub radio: RadioPlaybackState,
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

/// The UI's mirror of the queue's current row, stamped from each
/// `PlaybackStateUpdate`. Index and entry id are read under the same queue
/// lock, so they are always written (and cleared) together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct QueueCurrent {
    pub index: Option<usize>,
    /// Drift-immune partner of `index`: the per-row id of the playing track,
    /// so producers of `FocusCurrentPlaying` can dispatch a handle that
    /// survives the optimistic-mutation window.
    pub entry_id: Option<u64>,
}

/// Playback-related state for the player bar
///
/// Logout resets only what [`Self::reset_for_session`] names. The track
/// display fields stay: the engine stops asynchronously, so clearing them
/// here could race its last updates; the Login screen draws no player bar;
/// and the next session's first update overwrites them.
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
    pub bit_perfect_probe_generation: super::StaleDropGen,
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
    /// Whether the engine can seek what it plays, from the last tick: false
    /// for a song the server transcodes on the fly (and for a station).
    /// The seek producers and the progress bar read it through
    /// `Nokkvi::seek_available`.
    pub seekable: bool,
    /// Whether the playing song is the server's live transcode, from the
    /// last tick. Such a stream is never shown as bit-perfect.
    pub live_transcode: bool,
    /// Throttle timestamp for volume persistence to storage
    pub volume_persist_throttle: Option<std::time::Instant>,
    /// Set while volume saves keep failing, so only the first failure of a
    /// run warns (each later one would cover the "Volume: N%" readout); a
    /// successful save clears it.
    pub volume_save_failing: bool,
    /// Bumped by every `guard_play_action`. A failed play hands radio mode
    /// back only while it is still the latest attempt.
    pub play_attempt: super::StaleDropGen,
    /// The station `guard_play_action` last switched away from, and which
    /// attempt did it, so that play can hand radio mode back if it fails
    /// while the station is still the engine's source.
    pub station_left_for_play: Option<StationLeftForPlay>,
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

    /// Logout: a play that fails after this must not hand back the old
    /// session's station, so the attempt counter carries forward and the
    /// station record goes. The reset also stops the engine, and the tick
    /// that mirrors the engine's transport stands down without a shell, so
    /// the transport is stopped here too: the `status` verb and MPRIS read
    /// these flags, and used to report "playing" at a frozen position until
    /// the next login. The track display (title, artist, album, duration)
    /// stays, as after any stop.
    pub fn reset_for_session(&mut self) {
        self.station_left_for_play = None;
        self.play_attempt.bump();
        self.playing = false;
        self.paused = false;
        self.position = 0;
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
            format_suffix: String::new(),
            sample_rate: 0,
            bit_perfect_status: BitPerfectStatus::Off,
            bit_perfect_engaged: false,
            bit_perfect_probe_ticks: 0,
            bit_perfect_probe_generation: super::StaleDropGen::default(),
            bit_perfect_holder: None,
            bit_perfect_unverifiable_streak: 0,
            bit_perfect_holder_reprobe_ticks: 0,
            bitrate: 0,
            seekable: true,
            live_transcode: false,
            volume_persist_throttle: None,
            volume_save_failing: false,
            play_attempt: super::StaleDropGen::default(),
            station_left_for_play: None,
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

/// The repeat mode a `(repeat, repeat_queue)` pair encodes. Both set is not a
/// mode the backend reports and reads as Off. The one reading of the pair:
/// the `status` verb and MPRIS used to project it separately and disagreed
/// on that case.
pub(crate) fn repeat_mode_of(repeat: bool, repeat_queue: bool) -> RepeatMode {
    match (repeat, repeat_queue) {
        (true, false) => RepeatMode::Track,
        (false, true) => RepeatMode::Playlist,
        (false, false) | (true, true) => RepeatMode::None,
    }
}

/// The lowercase repeat token of the `repeat` and `status` verbs: `"one"`,
/// `"queue"` or `"off"`.
pub(crate) fn repeat_token(mode: RepeatMode) -> &'static str {
    match mode {
        RepeatMode::Track => "one",
        RepeatMode::Playlist => "queue",
        RepeatMode::None => "off",
    }
}

/// The MPRIS `LoopStatus` for `mode`.
pub(crate) fn mpris_loop_status(mode: RepeatMode) -> mpris_server::LoopStatus {
    match mode {
        RepeatMode::Track => mpris_server::LoopStatus::Track,
        RepeatMode::Playlist => mpris_server::LoopStatus::Playlist,
        RepeatMode::None => mpris_server::LoopStatus::None,
    }
}

/// The transport as every surface reports it: the `status` and transport
/// verbs, MPRIS and the tray read one derivation of the `(playing, paused)`
/// pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportState {
    Playing,
    Paused,
    Stopped,
}

impl TransportState {
    pub fn from_flags(playing: bool, paused: bool) -> Self {
        if playing && !paused {
            Self::Playing
        } else if paused {
            Self::Paused
        } else {
            Self::Stopped
        }
    }

    /// The lowercase wire token: `"playing"`, `"paused"` or `"stopped"`. The
    /// scriptable vocabulary, distinct from the capitalized player-bar toast
    /// labels.
    pub fn ipc_token(self) -> &'static str {
        match self {
            Self::Playing => "playing",
            Self::Paused => "paused",
            Self::Stopped => "stopped",
        }
    }

    pub fn mpris(self) -> mpris_server::PlaybackStatus {
        match self {
            Self::Playing => mpris_server::PlaybackStatus::Playing,
            Self::Paused => mpris_server::PlaybackStatus::Paused,
            Self::Stopped => mpris_server::PlaybackStatus::Stopped,
        }
    }
}

impl PlaybackState {
    /// The transport the flags encode (see [`TransportState`]).
    pub fn transport(&self) -> TransportState {
        TransportState::from_flags(self.playing, self.paused)
    }
}

impl PlaybackModes {
    /// The repeat mode the `(repeat, repeat_queue)` pair encodes. Both set
    /// is not a mode the backend reports and reads as Off.
    pub fn repeat_mode(&self) -> RepeatMode {
        repeat_mode_of(self.repeat, self.repeat_queue)
    }

    /// Write `mode` as the `(repeat, repeat_queue)` pair.
    pub fn set_repeat_mode(&mut self, mode: RepeatMode) {
        self.repeat = mode == RepeatMode::Track;
        self.repeat_queue = mode == RepeatMode::Playlist;
    }
}

#[cfg(test)]
mod transport_tests {
    use super::*;

    /// The `status` verb's token and MPRIS's LoopStatus come from one
    /// RepeatMode, so they agree on every mode; both flags set is not a mode.
    #[test]
    fn repeat_projections_agree() {
        for (mode, token, loop_status) in [
            (RepeatMode::None, "off", mpris_server::LoopStatus::None),
            (RepeatMode::Track, "one", mpris_server::LoopStatus::Track),
            (
                RepeatMode::Playlist,
                "queue",
                mpris_server::LoopStatus::Playlist,
            ),
        ] {
            assert_eq!(repeat_token(mode), token);
            assert_eq!(mpris_loop_status(mode), loop_status);
        }
        assert_eq!(repeat_mode_of(true, true), RepeatMode::None);
    }

    #[test]
    fn transport_tokens_follow_the_flags() {
        assert_eq!(
            TransportState::from_flags(true, false).ipc_token(),
            "playing"
        );
        assert_eq!(TransportState::from_flags(true, true).ipc_token(), "paused");
        assert_eq!(
            TransportState::from_flags(false, true).ipc_token(),
            "paused"
        );
        assert_eq!(
            TransportState::from_flags(false, false).ipc_token(),
            "stopped"
        );
        assert_eq!(
            TransportState::from_flags(false, false).mpris(),
            mpris_server::PlaybackStatus::Stopped
        );
    }
}
