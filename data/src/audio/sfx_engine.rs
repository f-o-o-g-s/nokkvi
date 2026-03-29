//! Sound Effects Engine (rodio-based)
//!
//! Low-latency polyphonic SFX player for UI navigation sounds.
//! Uses rodio's built-in mixer for voice polyphony — each play() call
//! creates a `SamplesBuffer` and adds it directly to the mixer.
//! No background thread needed; rodio handles mixing in the cpal callback.

use std::{
    cell::Cell,
    io::Cursor,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow};
use rodio::{DeviceSinkBuilder, MixerDeviceSink, buffer::SamplesBuffer};
use symphonia::core::{
    audio::SampleBuffer, codecs::DecoderOptions, formats::FormatOptions, io::MediaSourceStream,
    meta::MetadataOptions, probe::Hint,
};

/// Sound effect types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SfxType {
    Tab,
    Enter,
    Backspace,
    ViewSelect,
    ExpandCollapse,
    Escape,
}

// Bundled WAV files — always available as fallback regardless of install method
const BUNDLED_TAB_NAV: &[u8] = include_bytes!("../../../assets/sound_effects/tab_nav.wav");
const BUNDLED_ENTER: &[u8] = include_bytes!("../../../assets/sound_effects/enter.wav");
const BUNDLED_BACKSPACE_NAV: &[u8] =
    include_bytes!("../../../assets/sound_effects/backspace_nav.wav");
const BUNDLED_VIEW_SELECT: &[u8] = include_bytes!("../../../assets/sound_effects/view_select.wav");
const BUNDLED_EXPAND_COLLAPSE: &[u8] =
    include_bytes!("../../../assets/sound_effects/expand_collapse.wav");
const BUNDLED_ESCAPE: &[u8] = include_bytes!("../../../assets/sound_effects/escape.wav");

/// Mapping of SFX filename to bundled bytes for iteration
const SFX_FILES: &[(&str, &[u8])] = &[
    ("tab_nav.wav", BUNDLED_TAB_NAV),
    ("enter.wav", BUNDLED_ENTER),
    ("backspace_nav.wav", BUNDLED_BACKSPACE_NAV),
    ("view_select.wav", BUNDLED_VIEW_SELECT),
    ("expand_collapse.wav", BUNDLED_EXPAND_COLLAPSE),
    ("escape.wav", BUNDLED_ESCAPE),
];

/// Atomic f32 helpers
fn store_f32(atomic: &AtomicU32, value: f32) {
    atomic.store(value.to_bits(), Ordering::Relaxed);
}

fn load_f32(atomic: &AtomicU32) -> f32 {
    f32::from_bits(atomic.load(Ordering::Relaxed))
}

/// Sound effects engine using rodio's mixer for polyphonic playback.
///
/// Each `play()` call creates a lightweight `SamplesBuffer` and adds it
/// to the device's mixer. The mixer handles polyphonic mixing in the
/// cpal audio callback — no background thread needed.
pub struct SfxEngine {
    /// Shared atomic volume (0.0–1.0) — applied to each new voice at play time.
    volume: Arc<AtomicU32>,
    /// Whether SFX are enabled.
    enabled: Arc<AtomicBool>,
    // Pre-loaded audio samples (shared via Arc for zero-copy)
    tab_samples: Arc<Vec<f32>>,
    enter_samples: Arc<Vec<f32>>,
    backspace_samples: Arc<Vec<f32>>,
    view_select_samples: Arc<Vec<f32>>,
    expand_collapse_samples: Arc<Vec<f32>>,
    escape_samples: Arc<Vec<f32>>,
    /// Rodio device sink — its mixer() is used to add voices.
    /// `None` when no audio device is available (graceful degradation).
    sink: Option<MixerDeviceSink>,
    // Throttle: last play timestamps
    last_tab_play: Cell<Instant>,
    last_view_select_play: Cell<Instant>,
}

impl SfxEngine {
    /// Sample rate for SFX playback (48kHz is native for most devices)
    const SAMPLE_RATE: u32 = 48000;

    /// Create new SFX engine with rodio output
    pub fn new() -> Result<Self> {
        // Seed config dir with bundled defaults (no-op if files already exist)
        Self::seed_sfx_dir();

        // Load each SFX: config dir first, bundled fallback
        let tab_samples = Self::load_sfx("tab_nav.wav", BUNDLED_TAB_NAV)?;
        let enter_samples = Self::load_sfx("enter.wav", BUNDLED_ENTER)?;
        let backspace_samples = Self::load_sfx("backspace_nav.wav", BUNDLED_BACKSPACE_NAV)?;
        let view_select_samples = Self::load_sfx("view_select.wav", BUNDLED_VIEW_SELECT)?;
        let expand_collapse_samples =
            Self::load_sfx("expand_collapse.wav", BUNDLED_EXPAND_COLLAPSE)?;
        let escape_samples = Self::load_sfx("escape.wav", BUNDLED_ESCAPE)?;

        tracing::info!("🔊 SfxEngine: Loaded {} sound effects (rodio mode)", 6);

        // Open a dedicated rodio output for SFX
        // (separate from the music output so SFX volume is independent)
        let mut sink = open_preferred_sink()?;
        sink.log_on_drop(false);
        let sink = Some(sink);

        Ok(Self {
            volume: Arc::new(AtomicU32::new(0.68_f32.to_bits())),
            enabled: Arc::new(AtomicBool::new(true)),
            tab_samples: Arc::new(tab_samples),
            enter_samples: Arc::new(enter_samples),
            backspace_samples: Arc::new(backspace_samples),
            view_select_samples: Arc::new(view_select_samples),
            expand_collapse_samples: Arc::new(expand_collapse_samples),
            escape_samples: Arc::new(escape_samples),
            sink,
            last_tab_play: Cell::new(Instant::now() - Duration::from_millis(100)),
            last_view_select_play: Cell::new(Instant::now() - Duration::from_millis(100)),
        })
    }

    /// Seed the config sfx directory with bundled defaults.
    /// Only writes files that don't already exist (preserves user customizations).
    fn seed_sfx_dir() {
        let sfx_dir = match crate::utils::paths::get_sfx_dir() {
            Ok(dir) => dir,
            Err(e) => {
                tracing::warn!("🔊 SfxEngine: Could not create sfx config dir: {e}");
                return;
            }
        };

        for (filename, bytes) in SFX_FILES {
            let dest = sfx_dir.join(filename);
            if !dest.exists() {
                if let Err(e) = std::fs::write(&dest, bytes) {
                    tracing::warn!("🔊 SfxEngine: Failed to seed {filename}: {e}");
                } else {
                    tracing::trace!("🔊 SfxEngine: Seeded {filename} to config dir");
                }
            }
        }
    }

    /// Load a single SFX: try config dir first, fall back to bundled bytes.
    fn load_sfx(filename: &str, bundled: &'static [u8]) -> Result<Vec<f32>> {
        // Try loading from user's config sfx directory
        if let Ok(sfx_dir) = crate::utils::paths::get_sfx_dir() {
            let user_path = sfx_dir.join(filename);
            if user_path.exists() {
                match Self::load_wav(&user_path) {
                    Ok((samples, _)) => {
                        tracing::trace!("🔊 SfxEngine: Loaded {filename} from config dir");
                        return Ok(samples);
                    }
                    Err(e) => {
                        tracing::warn!(
                            "🔊 SfxEngine: Failed to load {filename} from config dir, \
                             falling back to bundled: {e}"
                        );
                    }
                }
            }
        }

        // Fall back to bundled bytes
        let (samples, _) = Self::load_wav_from_bytes(bundled)?;
        tracing::trace!("🔊 SfxEngine: Loaded {filename} from bundled data");
        Ok(samples)
    }

    /// Load WAV file from disk and return mono f32 samples at 48kHz
    fn load_wav(path: &Path) -> Result<(Vec<f32>, u32)> {
        let file = std::fs::File::open(path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        Self::decode_wav_stream(mss, path.file_name().and_then(|n| n.to_str()))
    }

    /// Load WAV from in-memory bytes and return mono f32 samples at 48kHz
    fn load_wav_from_bytes(bytes: &[u8]) -> Result<(Vec<f32>, u32)> {
        let cursor = Cursor::new(bytes.to_vec());
        let mss = MediaSourceStream::new(Box::new(cursor), Default::default());
        Self::decode_wav_stream(mss, Some("bundled"))
    }

    /// Shared WAV decoding logic for both file and in-memory sources
    fn decode_wav_stream(mss: MediaSourceStream, label: Option<&str>) -> Result<(Vec<f32>, u32)> {
        let mut hint = Hint::new();
        hint.with_extension("wav");

        let format_opts = FormatOptions::default();
        let metadata_opts = MetadataOptions::default();
        let decoder_opts = DecoderOptions::default();

        let probed =
            symphonia::default::get_probe().format(&hint, mss, &format_opts, &metadata_opts)?;

        let mut format = probed.format;
        let track = format
            .default_track()
            .ok_or_else(|| anyhow!("No audio track found"))?;

        let sample_rate = track.codec_params.sample_rate.unwrap_or(48000);
        let channels = track.codec_params.channels.map_or(2, |c| c.count());

        let mut decoder =
            symphonia::default::get_codecs().make(&track.codec_params, &decoder_opts)?;

        let mut samples = Vec::new();

        loop {
            match format.next_packet() {
                Ok(packet) => {
                    match decoder.decode(&packet) {
                        Ok(decoded) => {
                            let spec = *decoded.spec();
                            let duration = decoded.capacity();
                            let mut sample_buf = SampleBuffer::<f32>::new(duration as u64, spec);
                            sample_buf.copy_interleaved_ref(decoded);

                            let buf = sample_buf.samples();
                            // Convert to mono by averaging channels
                            for chunk in buf.chunks(channels) {
                                let mono: f32 = chunk.iter().sum::<f32>() / channels as f32;
                                samples.push(mono);
                            }
                        }
                        Err(e) => {
                            tracing::warn!("🔊 SfxEngine: Decode error: {}", e);
                        }
                    }
                }
                Err(symphonia::core::errors::Error::IoError(ref e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break;
                }
                Err(e) => {
                    tracing::warn!("🔊 SfxEngine: Format error: {}", e);
                    break;
                }
            }
        }

        // Resample to 48kHz if needed (simple linear interpolation)
        let samples = if sample_rate != Self::SAMPLE_RATE {
            Self::resample(&samples, sample_rate, Self::SAMPLE_RATE)
        } else {
            samples
        };

        tracing::trace!(
            "🔊 SfxEngine: Loaded {:?} ({} samples, {}Hz)",
            label.unwrap_or("unknown"),
            samples.len(),
            Self::SAMPLE_RATE
        );

        Ok((samples, Self::SAMPLE_RATE))
    }

    /// Simple linear resampling
    fn resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
        let ratio = from_rate as f64 / to_rate as f64;
        let new_len = (samples.len() as f64 / ratio) as usize;
        let mut result = Vec::with_capacity(new_len);

        for i in 0..new_len {
            let src_pos = i as f64 * ratio;
            let src_idx = src_pos as usize;
            let frac = src_pos - src_idx as f64;

            let s0 = samples.get(src_idx).copied().unwrap_or(0.0);
            let s1 = samples.get(src_idx + 1).copied().unwrap_or(s0);

            result.push(s0 + (s1 - s0) * frac as f32);
        }

        result
    }

    /// Play a sound effect (non-blocking, <5ms latency)
    ///
    /// Creates a mono `SamplesBuffer`, applies volume, duplicates to stereo,
    /// and adds it to the rodio mixer. The mixer handles polyphonic mixing.
    pub fn play(&self, sfx: SfxType) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }

        // Throttle SFX to prevent overwhelming system with rapid key-repeat or view cycling
        const THROTTLE_MS: u64 = 40;
        let now = Instant::now();
        match sfx {
            SfxType::Tab => {
                if now.duration_since(self.last_tab_play.get()) < Duration::from_millis(THROTTLE_MS)
                {
                    return; // Skip if within throttle window
                }
                self.last_tab_play.set(now);
            }
            SfxType::ViewSelect => {
                if now.duration_since(self.last_view_select_play.get())
                    < Duration::from_millis(THROTTLE_MS)
                {
                    return; // Skip if within throttle window
                }
                self.last_view_select_play.set(now);
            }
            _ => {}
        }

        let mono_samples = match sfx {
            SfxType::Tab => Arc::clone(&self.tab_samples),
            SfxType::Enter => Arc::clone(&self.enter_samples),
            SfxType::Backspace => Arc::clone(&self.backspace_samples),
            SfxType::ViewSelect => Arc::clone(&self.view_select_samples),
            SfxType::ExpandCollapse => Arc::clone(&self.expand_collapse_samples),
            SfxType::Escape => Arc::clone(&self.escape_samples),
        };

        let vol = load_f32(&self.volume);

        // Create stereo samples with volume applied
        let mut stereo = Vec::with_capacity(mono_samples.len() * 2);
        for &sample in mono_samples.iter() {
            let s = sample * vol;
            stereo.push(s); // Left
            stereo.push(s); // Right
        }

        // Create a SamplesBuffer (stereo, 48kHz) and add to mixer
        let Some(ref sink) = self.sink else { return };
        use std::num::NonZero;
        let channels_nz = NonZero::new(2u16).expect("2 is nonzero");
        let sample_rate_nz = NonZero::new(Self::SAMPLE_RATE).expect("48000 is nonzero");
        let buffer = SamplesBuffer::new(channels_nz, sample_rate_nz, stereo);

        // mixer().add() adds the source to the mixer — polyphony is automatic
        sink.mixer().add(buffer);
    }

    /// Set volume (0.0-1.0)
    pub fn set_volume(&mut self, volume: f32) {
        store_f32(&self.volume, volume.clamp(0.0, 1.0));
    }

    /// Get current volume
    pub fn volume(&self) -> f32 {
        load_f32(&self.volume)
    }

    /// Enable/disable sound effects
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    /// Check if enabled
    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Get a clone of the shared mixer reference.
    ///
    /// Used by the music engine to share the same cpal output stream,
    /// avoiding conflicts with dual ALSA/PipeWire streams.
    /// Returns `None` if no audio device is available.
    pub fn mixer(&self) -> Option<rodio::mixer::Mixer> {
        self.sink.as_ref().map(|s| s.mixer().clone())
    }
}

impl Default for SfxEngine {
    fn default() -> Self {
        Self::new().unwrap_or_else(|e| {
            tracing::error!("🔊 SfxEngine: Failed to initialize: {}", e);
            // Return a dummy engine that does nothing — no panic if audio device
            // is unavailable (e.g., headless servers, CI).
            let sink = open_preferred_sink().ok();
            if sink.is_none() {
                tracing::warn!("🔊 SfxEngine: No audio device available — SFX disabled");
            }
            Self {
                volume: Arc::new(AtomicU32::new(0.68_f32.to_bits())),
                enabled: Arc::new(AtomicBool::new(false)),
                tab_samples: Arc::new(Vec::new()),
                enter_samples: Arc::new(Vec::new()),
                backspace_samples: Arc::new(Vec::new()),
                view_select_samples: Arc::new(Vec::new()),
                expand_collapse_samples: Arc::new(Vec::new()),
                escape_samples: Arc::new(Vec::new()),
                sink,
                last_tab_play: Cell::new(Instant::now()),
                last_view_select_play: Cell::new(Instant::now()),
            }
        })
    }
}

/// Try to open a `MixerDeviceSink` using native PipeWire, falling back to ALSA.
///
/// On Linux with the `pipewire` cpal feature enabled, this attempts to initialise
/// the PipeWire host and open its default output device. If PipeWire is unavailable
/// (not running, wrong permissions, etc.) it falls back to `open_default_sink()`
/// which uses the ALSA host.
///
/// On non-Linux platforms this is a no-op wrapper around `open_default_sink()`.
fn open_preferred_sink() -> Result<MixerDeviceSink> {
    #[cfg(target_os = "linux")]
    {
        use rodio::cpal::{self, traits::HostTrait};

        // Set PipeWire stream properties so the stream appears as "Nokkvi"
        // in PipeWire-aware UIs (e.g., Noctalia shell volume panel).
        // This is read by libpipewire's pw_context_new() for both native
        // PipeWire and the ALSA compatibility shim.
        // Safety: called once before any audio stream is opened.
        unsafe {
            std::env::set_var(
                "PIPEWIRE_PROPS",
                "{ application.name=Nokkvi media.name=Music application.icon-name=nokkvi media.role=Music }",
            );
        }

        // Try PipeWire first
        match cpal::host_from_id(cpal::HostId::PipeWire) {
            Ok(pw_host) => {
                if let Some(device) = pw_host.default_output_device() {
                    match DeviceSinkBuilder::from_device(device).and_then(|b| b.open_stream()) {
                        Ok(sink) => {
                            tracing::info!("🔊 Audio output: native PipeWire");
                            return Ok(sink);
                        }
                        Err(e) => {
                            tracing::warn!(
                                "🔊 PipeWire device found but stream failed, falling back to ALSA: {e}"
                            );
                        }
                    }
                } else {
                    tracing::warn!(
                        "🔊 PipeWire host available but no default output device, falling back to ALSA"
                    );
                }
            }
            Err(_) => {
                tracing::debug!("🔊 PipeWire host not available, using ALSA");
            }
        }
    }

    // Fallback: default host (ALSA on Linux, CoreAudio on macOS, WASAPI on Windows)
    let sink = DeviceSinkBuilder::open_default_sink()
        .map_err(|e| anyhow!("Failed to open audio output: {e}"))?;
    #[cfg(target_os = "linux")]
    tracing::info!("🔊 Audio output: ALSA (via PipeWire compatibility shim)");
    #[cfg(not(target_os = "linux"))]
    tracing::info!("🔊 Audio output: system default");
    Ok(sink)
}
