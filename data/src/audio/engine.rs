use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
};

use anyhow::Result;
use parking_lot::Mutex as PlMutex;
use tracing::{debug, error, trace, warn};

use crate::audio::{AudioDecoder, AudioFormat, AudioRenderer, DecodeLoopHandle, SourceGeneration};

/// Convert S16 (i16) PCM bytes to f32 samples normalized to [-1.0, 1.0].
/// The decoder always produces S16 via `RawSampleBuffer::<i16>`.
fn s16_bytes_to_f32(bytes: &[u8]) -> Vec<f32> {
    let samples: &[i16] = bytemuck::cast_slice(bytes);
    samples.iter().map(|&s| s as f32 / 32768.0).collect()
}

/// Playback state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

/// Crossfade transition phase.
///
/// Variants carry the crossfade decoder + incoming source URL — these
/// fields used to live as parallel `Arc<Mutex<Option<AudioDecoder>>>` /
/// `String` on `CustomAudioEngine`, where every transition reset them
/// in lockstep with the phase flag. Now the data lives WITH the phase
/// so transitions are one `mem::replace` and impossible states are
/// unrepresentable (e.g. `Idle` carries no decoder; `Active` has it).
pub enum CrossfadePhase {
    /// Normal single-track playback.
    Idle,
    /// Two decoders active, blending audio in renderer.
    Active {
        decoder: Arc<tokio::sync::Mutex<Option<AudioDecoder>>>,
        incoming_source: String,
    },
    /// Outgoing decoder finished, incoming still draining.
    OutgoingFinished {
        decoder: Arc<tokio::sync::Mutex<Option<AudioDecoder>>>,
        incoming_source: String,
    },
}

impl CrossfadePhase {
    pub fn is_idle(&self) -> bool {
        matches!(self, CrossfadePhase::Idle)
    }

    pub fn is_active_or_finished(&self) -> bool {
        matches!(
            self,
            CrossfadePhase::Active { .. } | CrossfadePhase::OutgoingFinished { .. }
        )
    }

    /// Short label for diagnostic logs (the variants' inner `Mutex`
    /// makes a derived `Debug` impl impractical).
    fn label(&self) -> &'static str {
        match self {
            CrossfadePhase::Idle => "idle",
            CrossfadePhase::Active { .. } => "active",
            CrossfadePhase::OutgoingFinished { .. } => "outgoing_finished",
        }
    }
}

/// Info about a gapless transition that occurred in the decode loop.
/// The decode loop writes this, and the engine reads it to update its metadata.
#[derive(Debug, Clone)]
pub struct GaplessTransitionInfo {
    pub source: String,
    pub duration: u64,
    pub format: AudioFormat,
    pub codec: Option<String>,
}

/// Calculate buffer size for one decode chunk (~100ms of audio).
///
/// Returns bytes for 100ms of the given format, clamped to [4096, 16384],
/// or 8192 if the format is not yet known.
fn decode_buffer_size(format: &AudioFormat) -> usize {
    if format.is_valid() {
        format.bytes_for_duration(100).clamp(4096, 16384)
    } else {
        8192
    }
}

/// Compute backpressure watermarks scaled by crossfade duration.
///
/// Returns `(high_watermark, low_watermark)` — the thresholds at which the
/// decode loop pauses/resumes fetching. Shared by both the primary and
/// crossfade decode loops.
fn compute_watermarks(crossfade_ms: u64) -> (usize, usize) {
    const BASE_HIGH: usize = 30; // ~3 seconds at 100ms per buffer
    const BUFFER_MS: u64 = 100;
    let cf_buffers = if crossfade_ms > 0 {
        (crossfade_ms / BUFFER_MS) as usize + 10 // crossfade duration + margin
    } else {
        0
    };
    let high = BASE_HIGH.max(cf_buffers);
    (high, high / 3)
}

/// Custom audio engine - main orchestrator
pub struct CustomAudioEngine {
    source: String,
    playing: bool,
    paused: bool,
    position: u64, // milliseconds
    duration: u64, // milliseconds
    volume: f64,   // 0.0-1.0

    // Decoder
    decoder: Arc<tokio::sync::Mutex<AudioDecoder>>,
    next_decoder: Arc<tokio::sync::Mutex<Option<AudioDecoder>>>,

    // Format tracking for gapless
    current_format: AudioFormat,
    next_format: AudioFormat,

    // Next track source
    next_source: String,

    // Renderer
    renderer: Arc<PlMutex<AudioRenderer>>,

    // State
    state: PlaybackState,

    // Decoding loop cancellation: each spawned loop captures the current
    // generation at spawn time and exits when the generation no longer matches.
    // This prevents the old loop from continuing when a new loop starts.
    decode_loop: DecodeLoopHandle,

    // Gapless preloading state
    next_track_prepared: Arc<tokio::sync::Mutex<bool>>,

    // Completion callback — called when a track ends.
    // The bool argument is `true` when the same track is looping (repeat-one),
    // `false` when advancing to a new track.
    completion_callback: Option<Arc<dyn Fn(bool) + Send + Sync>>,

    // Seeking flag - prevents EOF detection during seek
    seeking: Arc<AtomicBool>,

    // Live compressed bitrate from decoder (updated per-packet in decode loop)
    live_bitrate: Arc<AtomicU32>,

    // Live sample rate from decoder (updated when format is set, atomic for threading consistency)
    live_sample_rate: Arc<AtomicU32>,

    // Dedicated render thread (decoupled from iced event loop)
    render_thread: Option<std::thread::JoinHandle<()>>,
    render_running: Arc<AtomicBool>,

    /// Incremented on every source change. Shared with the renderer so
    /// completion callbacks can detect staleness (e.g. manual skip raced
    /// with track-end) without needing the engine lock.
    source_generation: SourceGeneration,

    /// Set by the decode loop when the primary decoder reaches EOF.
    /// Shared with the renderer to gate crossfade trigger: prevents false
    /// triggers from transiently empty buffers after a seek.
    decoder_eof: Arc<AtomicBool>,

    /// Lock-free crossfade duration for the decode loop's dynamic backpressure.
    /// Updated by `set_crossfade_duration()`, read by the spawned decode task.
    crossfade_duration_shared: Arc<AtomicU64>,

    // ---- Crossfade state ----
    /// Current crossfade phase + per-phase data (decoder + incoming source
    /// live inside `Active` / `OutgoingFinished` variants).
    crossfade_phase: CrossfadePhase,
    /// Whether crossfade is enabled (from settings)
    crossfade_enabled: bool,
    /// Crossfade duration in milliseconds (from settings)
    crossfade_duration_ms: u64,

    // ---- Gapless transition state ----
    /// Transition info written by the decode loop, consumed by the engine.
    gapless_transition_info: Arc<tokio::sync::Mutex<Option<GaplessTransitionInfo>>>,
    /// Next track source URL — shared with the decode loop for gapless transitions.
    next_source_shared: Arc<tokio::sync::Mutex<String>>,

    /// Raw ICY-metadata parsed by IcyMetadataReader
    live_icy_metadata: Arc<std::sync::RwLock<Option<String>>>,

    /// Extracted stream codec based on Symphonia probing (e.g. mp3, aac)
    live_codec_name: Arc<std::sync::RwLock<Option<String>>>,
}

impl CustomAudioEngine {
    pub fn new() -> Self {
        let live_icy_metadata = Arc::new(std::sync::RwLock::new(None));
        Self {
            source: String::new(),
            playing: false,
            paused: false,
            position: 0,
            duration: 0,
            volume: 1.0,
            decoder: Arc::new(tokio::sync::Mutex::new(AudioDecoder::new(
                live_icy_metadata.clone(),
            ))),
            next_decoder: Arc::new(tokio::sync::Mutex::new(None)),
            current_format: AudioFormat::invalid(),
            next_format: AudioFormat::invalid(),
            next_source: String::new(),
            renderer: Arc::new(PlMutex::new(AudioRenderer::new())),
            state: PlaybackState::Stopped,
            decode_loop: DecodeLoopHandle::new(),
            next_track_prepared: Arc::new(tokio::sync::Mutex::new(false)),
            completion_callback: None,
            seeking: Arc::new(AtomicBool::new(false)),
            render_thread: None,
            render_running: Arc::new(AtomicBool::new(false)),
            live_bitrate: Arc::new(AtomicU32::new(0)),
            live_sample_rate: Arc::new(AtomicU32::new(0)),
            source_generation: SourceGeneration::new(),
            decoder_eof: Arc::new(AtomicBool::new(false)),
            crossfade_duration_shared: Arc::new(AtomicU64::new(5000)),
            crossfade_phase: CrossfadePhase::Idle,
            crossfade_enabled: false,
            crossfade_duration_ms: 5000,
            gapless_transition_info: Arc::new(tokio::sync::Mutex::new(None)),
            next_source_shared: Arc::new(tokio::sync::Mutex::new(String::new())),
            live_icy_metadata,
            live_codec_name: Arc::new(std::sync::RwLock::new(None)),
        }
    }

    /// Get current source URL
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Set source URL
    pub async fn set_source(&mut self, source: String) {
        trace!(" AudioEngine: set_source called with: {}", source);
        if self.source == source {
            trace!(" AudioEngine: source unchanged, returning early");
            return;
        }

        if self.playing || self.paused {
            trace!(" AudioEngine: stopping current playback before changing source");
            self.stop().await;
        }

        // CRITICAL FIX: Reset fields *BEFORE* creating AudioDecoder.
        // During `AudioDecoder::new`, Symphonia's probe reads the first chunk of the stream synchronously.
        // If this stream contains ICY metadata, the callback fires during `new()` and populates `live_icy_metadata`.
        // If we reset this to `None` after `new()`, we will permanently discard the first stream title!
        self.live_bitrate.store(0, Ordering::Relaxed);
        self.live_sample_rate.store(0, Ordering::Relaxed);
        self.decoder_eof.store(false, Ordering::Release);
        if let Ok(mut guard) = self.live_icy_metadata.try_write() {
            *guard = None;
        }
        // Non-blocking like the icy_metadata reset above: stale codec data
        // is acceptable here, and `set_source` must not block on UI readers.
        if let Ok(mut guard) = self.live_codec_name.try_write() {
            *guard = None;
        }

        trace!(" AudioEngine: creating fresh decoder for new source");
        self.decoder = Arc::new(tokio::sync::Mutex::new(AudioDecoder::new(
            self.live_icy_metadata.clone(),
        )));

        self.duration = 0;
        self.position = 0;

        self.source = source;
        self.source_generation.bump_for_user_action();
        trace!(" AudioEngine: source set successfully");
    }

    /// Get current parsed ICY-metadata from the stream buffer
    pub fn live_icy_metadata(&self) -> Option<String> {
        self.live_icy_metadata
            .read()
            .ok()
            .and_then(|guard| guard.clone())
    }

    /// Get current live codec name
    pub fn live_codec(&self) -> Option<String> {
        self.live_codec_name
            .read()
            .ok()
            .and_then(|guard| guard.clone())
    }

    /// Get playing state
    pub fn playing(&self) -> bool {
        self.playing
    }

    /// Get position (milliseconds)
    /// Reads from renderer if playing, otherwise returns stored position
    pub fn position(&self) -> u64 {
        if self.playing && !self.paused {
            let renderer = self.renderer.lock();
            renderer.position()
        } else {
            self.position
        }
    }

    /// Get duration (milliseconds)
    pub fn duration(&self) -> u64 {
        self.duration
    }

    /// Get volume (0.0-1.0)
    pub fn volume(&self) -> f64 {
        self.volume
    }

    /// Set volume (0.0-1.0)
    pub fn set_volume(&mut self, volume: f64) {
        self.volume = volume.clamp(0.0, 1.0);

        // Apply volume to renderer
        let mut renderer = self.renderer.lock();
        renderer.set_volume(self.volume);
    }

    /// Play
    pub async fn play(&mut self) -> Result<()> {
        debug!(
            "🎵 AudioEngine: play() called, source: '{}', playing: {}, paused: {}",
            self.source, self.playing, self.paused
        );
        if self.source.is_empty() {
            trace!(" AudioEngine: ERROR - cannot play, source is empty");
            anyhow::bail!("Cannot play - source is empty");
        }

        if self.playing && !self.paused {
            // Check if a gapless transition happened in the decode loop.
            // If so, consume the transition info to update engine metadata
            // (source, duration, format). The decode loop already swapped the
            // decoder and the stream is still feeding data — no restart needed.
            self.consume_gapless_transition().await;
            trace!(" AudioEngine: already playing, returning (gapless info consumed if pending)");
            return Ok(());
        }

        if self.paused {
            // Resume from pause
            self.paused = false;
            self.playing = true;
            {
                let mut renderer = self.renderer.lock();
                renderer.start();
            } // renderer guard dropped before .await
            self.state = PlaybackState::Playing;
            // Restart the decoding loop so new buffers are produced after resume
            self.start_decoding_loop();
            // Restart render thread
            self.start_render_thread();
            return Ok(());
        }

        // Start new playback
        trace!(" AudioEngine: starting new playback");
        *self.next_track_prepared.lock().await = false; // Reset prepared flag for new track
        let mut decoder = self.decoder.lock().await;
        if !decoder.is_initialized() {
            trace!(" AudioEngine: decoder not initialized, initializing with source");
            match decoder.init(&self.source).await {
                Ok(()) => {
                    debug!(
                        "🎵 AudioEngine: decoder initialized successfully, duration: {}",
                        decoder.duration()
                    );
                    self.duration = decoder.duration();
                    if let Ok(mut guard) = self.live_codec_name.write() {
                        *guard = decoder.live_codec();
                    }
                }
                Err(e) => {
                    error!(" AudioEngine: decoder initialization FAILED: {:?}", e);
                    return Err(e);
                }
            }
        } else {
            trace!(" AudioEngine: decoder already initialized, seeking to start");
            // Seek back to the beginning for replay
            if !decoder.seek(0) {
                trace!(" AudioEngine: seek to start failed");
            } else {
                trace!(" AudioEngine: seek to start completed");
            }
            // CRITICAL: Restore duration from decoder (may have been cleared by stop())
            self.duration = decoder.duration();
            trace!(" AudioEngine: duration restored: {}", self.duration);
        }

        // Initialize renderer with format (only if needed)
        self.current_format = decoder.format().clone();
        self.live_sample_rate
            .store(self.current_format.sample_rate(), Ordering::Relaxed);
        trace!(" AudioEngine: format set: {:?}", self.current_format);
        drop(decoder);

        {
            let mut renderer = self.renderer.lock();

            let needs_init = !renderer.format().is_valid()
                || renderer.format() != &self.current_format
                || !renderer.has_primary_stream();

            if needs_init {
                trace!(" AudioEngine: initializing renderer (format changed or first init)");
                let init_result = renderer.init(&self.current_format, false, None);
                match init_result {
                    Ok(_) => trace!(" AudioEngine: renderer initialized successfully"),
                    Err(e) => {
                        trace!(" AudioEngine: renderer initialization failed: {:?}", e);
                        return Err(e);
                    }
                }
            } else {
                trace!(
                    " AudioEngine: renderer already initialized with correct format, skipping init"
                );
            }

            // Apply current volume to renderer
            renderer.set_volume(self.volume);

            // Set playing state BEFORE starting decoding
            self.playing = true;
            trace!(" AudioEngine: set playing state to true");
            self.paused = false;
            self.state = PlaybackState::Playing;
            trace!(" AudioEngine: set paused=false, state=Playing");
        } // Drop renderer lock before acquiring decoder lock

        // PREBUFFERING: Queue initial buffers before starting renderer
        // This prevents buffer starvation at playback start
        const PLAY_PREBUFFER_COUNT: usize = 15;
        trace!(
            " AudioEngine: prebuffering {} buffers before playback",
            PLAY_PREBUFFER_COUNT
        );

        {
            let mut decoder_guard = self.decoder.lock().await;
            for i in 0..PLAY_PREBUFFER_COUNT {
                let buffer_size = decode_buffer_size(decoder_guard.format());

                // Use block_in_place for blocking HTTP I/O
                let buffer = tokio::task::block_in_place(|| decoder_guard.read_buffer(buffer_size));
                if buffer.is_valid() && buffer.byte_count() > 0 {
                    let samples = s16_bytes_to_f32(buffer.data());
                    let mut renderer = self.renderer.lock();
                    renderer.write_samples(&samples);
                    drop(renderer);
                    trace!(
                        " AudioEngine: queued prebuffer {}/{}",
                        i + 1,
                        PLAY_PREBUFFER_COUNT
                    );
                } else {
                    warn!(
                        "  AudioEngine: prebuffering stopped at {}/{} (no more data)",
                        i + 1,
                        PLAY_PREBUFFER_COUNT
                    );
                    break;
                }
            }
            drop(decoder_guard);
        }

        // Start rendering with buffers already queued
        {
            trace!(" AudioEngine: starting renderer");
            let mut renderer = self.renderer.lock();
            renderer.start();
            trace!(" AudioEngine: renderer started");
            // Renderer started, starting decoding loop
        }

        // Start decoding loop
        trace!(" AudioEngine: starting decoding loop");
        self.start_decoding_loop();
        trace!(" AudioEngine: decoding loop started");

        // Start dedicated render thread (decoupled from iced event loop)
        self.start_render_thread();
        trace!(" AudioEngine: render thread started");

        debug!(" AudioEngine: play() completed successfully");
        Ok(())
    }

    /// Start the decoding loop in a background task
    fn start_decoding_loop(&mut self) {
        let decoder = self.decoder.clone();
        let renderer = self.renderer.clone();
        let live_bitrate = self.live_bitrate.clone();
        let decoder_eof = self.decoder_eof.clone();
        let crossfade_duration_shared = self.crossfade_duration_shared.clone();

        // Gapless: pass next-track state so the decode loop can swap inline
        let next_decoder = self.next_decoder.clone();
        let next_track_prepared = self.next_track_prepared.clone();
        let completion_callback = self.completion_callback.clone();
        let gapless_info = self.gapless_transition_info.clone();
        let source_generation = self.source_generation.clone();
        let next_source_shared = self.next_source_shared.clone();
        let reconnect_url = self.source.clone();

        // Clear EOF flag — this decoder is starting fresh
        self.decoder_eof.store(false, Ordering::Release);

        // Increment decode generation — invalidates any previous decode loop.
        // Each loop captures its generation at spawn time and exits when
        // the generation no longer matches (i.e. a newer loop superseded it).
        let my_gen = self.decode_loop.supersede();
        let decode_gen = self.decode_loop.clone();

        // Spawn decoding task
        tokio::spawn(async move {
            let mut loop_count = 0;
            let mut backpressure_active = false;
            let mut stream_type_checked = false;
            let mut stream_is_infinite_cached = false;
            let mut radio_music_jitter_filled = false;
            let mut last_heartbeat = std::time::Instant::now();
            let mut empty_buffer_count: u64 = 0;

            'decode_loop: loop {
                loop_count += 1;

                // Heartbeat every 10 seconds to confirm loop is still running
                if last_heartbeat.elapsed() > std::time::Duration::from_secs(10) {
                    tracing::trace!(
                        "💓 [DECODE LOOP] Heartbeat: {} iterations, still running",
                        loop_count
                    );
                    last_heartbeat = std::time::Instant::now();
                }

                // Check if this loop has been superseded by a newer one.
                // Uses a lock-free atomic check instead of a mutex.
                if decode_gen.current() != my_gen {
                    tracing::trace!(
                        "🔄 [DECODE LOOP] Exiting - generation superseded (my={}, current={}) after {} iterations",
                        my_gen,
                        decode_gen.current(),
                        loop_count
                    );
                    break;
                }

                // BACKPRESSURE CHECK: If ring buffer is full, wait for it to drain
                let buffer_count = {
                    let renderer_guard = renderer.lock();
                    // Approximate number of "buffer units" in the ring buffer
                    // (divide samples by ~800 to get equivalent buffer count)
                    renderer_guard.buffer_count() / 800
                }; // renderer lock dropped here, before any .await

                // Dynamic watermarks: scale with crossfade duration so the
                // buffer can hold enough audio for a full fade-out.
                let cf_ms = crossfade_duration_shared.load(Ordering::Relaxed);
                let (high_watermark, low_watermark) = compute_watermarks(cf_ms);

                // CRITICAL FIX: NEVER apply backpressure to Infinite Streams (Internet Radio)!
                // Radio streams send data precisely at 1x speed. If we apply backpressure by sleeping
                // the decode async thread, we completely neglect the raw TCP socket! When it wakes up,
                // the TCP Zero Window will cause Icecast dropping and 1-2 second connection delays.
                // We MUST let Symphonia read the Icecast stream continuously to maintain network stability!
                if buffer_count >= high_watermark && !stream_is_infinite_cached {
                    if !backpressure_active {
                        tracing::trace!(
                            "⏸️ [DECODE LOOP] Backpressure ON: buffer count {} >= {} (high watermark, cf={}ms)",
                            buffer_count,
                            high_watermark,
                            cf_ms,
                        );
                        backpressure_active = true;
                    }
                    // Sleep longer while waiting for buffers to drain
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    continue;
                } else if backpressure_active && buffer_count <= low_watermark {
                    tracing::trace!(
                        "▶️ [DECODE LOOP] Backpressure OFF: buffer count {} <= {} (low watermark)",
                        buffer_count,
                        low_watermark,
                    );
                    backpressure_active = false;
                } else if backpressure_active {
                    // Still in backpressure mode, waiting for low_watermark
                    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                    continue;
                }

                // Try to acquire decoder lock with a short timeout
                // This allows the loop to check the flag frequently even if lock is contested
                let mut decoder_guard = decoder.lock().await;

                // CRITICAL: Check generation AGAIN after acquiring lock, before doing I/O!
                // If a new loop started while we were waiting for the lock,
                // we release the lock immediately instead of starting a long HTTP read.
                if decode_gen.current() != my_gen {
                    tracing::trace!("🔄 [DECODE LOOP] Exiting after lock - generation superseded");
                    drop(decoder_guard);
                    break;
                }

                if !decoder_guard.is_initialized() {
                    drop(decoder_guard);
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                    continue;
                }

                let buffer_size = decode_buffer_size(decoder_guard.format());

                // Decode buffer - this is where HTTP I/O happens
                // CRITICAL: Use block_in_place to prevent blocking the async runtime!
                // The decoder uses reqwest::blocking::Client for HTTP which would otherwise
                // starve the tokio runtime, causing timeouts and deadlocks.
                let buffer = tokio::task::block_in_place(|| decoder_guard.read_buffer(buffer_size));

                // Propagate live bitrate from decoder to engine atomic
                let current_bitrate = decoder_guard.live_bitrate();
                if current_bitrate > 0 {
                    live_bitrate.store(current_bitrate, Ordering::Relaxed);
                }

                // Check stream type once and cache it so we can safely disable backpressure.
                if !stream_type_checked {
                    stream_is_infinite_cached = decoder_guard.is_infinite_stream();
                    stream_type_checked = true;
                }

                let is_eof = decoder_guard.is_eof();
                let is_infinite = stream_is_infinite_cached;

                if buffer.is_valid() && buffer.byte_count() > 0 {
                    // Release decoder lock before acquiring renderer lock
                    drop(decoder_guard);

                    // Convert S16 bytes to f32 and write to ring buffer
                    let samples = s16_bytes_to_f32(buffer.data());

                    let mut samples_to_write = samples.as_slice();
                    while !samples_to_write.is_empty() {
                        if decode_gen.current() != my_gen {
                            break;
                        }

                        let written = {
                            let mut renderer_guard = renderer.lock();
                            renderer_guard.write_samples(samples_to_write)
                        };

                        if written < samples_to_write.len() {
                            samples_to_write = &samples_to_write[written..];
                            tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
                        } else {
                            break;
                        }
                    }

                    // Radio jitter buffer: initial prebuffer only, then never pause.
                    // SomaFM sends at exactly 1.0× realtime, so the buffer level
                    // will hover near the consumption rate. Pausing playback to
                    // re-buffer causes audible gaps — instead, let transient
                    // underruns produce natural silence via try_pop().unwrap_or(0.0).
                    if is_infinite && !radio_music_jitter_filled {
                        let buffered_samples = renderer.lock().buffer_count();
                        if buffered_samples < 441_000 {
                            // Enforce pause continuously until full. This prevents front-end
                            // UI events (like `engine.play()`) from unpausing prematurely.
                            renderer.lock().pause();
                        } else {
                            tracing::info!(
                                "📻 [DECODE LOOP] Pre-buffered 5+ seconds of radio, starting playback."
                            );
                            radio_music_jitter_filled = true;
                            renderer.lock().start();
                        }
                    }

                    if last_heartbeat.elapsed().as_secs() >= 5 {
                        let guard = renderer.lock();
                        let buffered = guard.buffer_count();
                        let (ur_count, ur_peak, ur_total) = guard.underrun_stats();
                        drop(guard);
                        let sec_rem = buffered as f32 / 88_200.0;
                        let peak_ms = ur_peak as f32 / 88.2;
                        tracing::info!(
                            "🔌 [STREAM HEALTH] Buffer: {} ({:.1}s) | Underruns: {} (peak {:.0}ms) | Silence: {} | EmptyBufs: {} | HW: {} LW: {}",
                            buffered,
                            sec_rem,
                            ur_count,
                            peak_ms,
                            ur_total,
                            empty_buffer_count,
                            high_watermark,
                            low_watermark,
                        );
                        empty_buffer_count = 0;
                        last_heartbeat = std::time::Instant::now();
                    }
                } else if is_eof {
                    // =========================================================
                    // RADIO STREAM EOF: connection dropped or server closed.
                    // Skip gapless transition — radio has no "next track".
                    // =========================================================
                    if is_infinite {
                        tracing::warn!(
                            "📻 [DECODE LOOP] Radio stream dropped, attempting reconnect..."
                        );
                        // CRITICAL: Drop decoder_guard BEFORE sleeping!
                        // Holding this lock during the backoff would deadlock the UI tick
                        // (which reads position/duration via decoder.lock()).
                        drop(decoder_guard);

                        let mut retry_count = 0u32;
                        const MAX_RETRIES: u32 = 5;

                        loop {
                            // Abort reconnect if source changed (user skipped/stopped)
                            if decode_gen.current() != my_gen {
                                tracing::debug!("📻 [RECONNECT] Aborted — generation superseded");
                                break;
                            }
                            retry_count += 1;
                            if retry_count > MAX_RETRIES {
                                tracing::warn!(
                                    "📻 [RECONNECT] Failed after {} attempts, giving up",
                                    MAX_RETRIES
                                );
                                decoder_eof.store(true, Ordering::Release);
                                break;
                            }
                            let backoff =
                                std::time::Duration::from_secs(1u64 << retry_count.min(4));
                            tracing::info!(
                                "📻 [RECONNECT] Attempt {}/{} in {:?}",
                                retry_count,
                                MAX_RETRIES,
                                backoff
                            );
                            tokio::time::sleep(backoff).await;

                            // Re-acquire decoder lock for re-init
                            let mut dec = decoder.lock().await;
                            match dec.init(&reconnect_url).await {
                                Ok(()) => {
                                    tracing::info!("📻 [RECONNECT] Success!");
                                    // Reset stream-type check so jitter buffer and
                                    // backpressure caching are re-evaluated
                                    stream_type_checked = false;
                                    radio_music_jitter_filled = false;
                                    drop(dec);
                                    continue 'decode_loop;
                                }
                                Err(e) => {
                                    tracing::warn!("📻 [RECONNECT] Failed: {}", e);
                                    drop(dec);
                                    // Continue retry loop
                                }
                            }
                        }
                        break; // Exit decode loop (either exhausted or generation superseded)
                    }

                    // =========================================================
                    // GAPLESS TRANSITION: try to swap the next decoder inline
                    // =========================================================
                    let current_format = decoder_guard.format().clone();
                    drop(decoder_guard); // release primary decoder lock

                    let did_gapless = {
                        let is_prepared = *next_track_prepared.lock().await;
                        if is_prepared {
                            let mut next_dec_guard = next_decoder.lock().await;
                            if let Some(ref next_dec) = *next_dec_guard {
                                let next_fmt = next_dec.format().clone();
                                let formats_match = current_format.is_valid()
                                    && next_fmt.is_valid()
                                    && current_format.sample_rate() == next_fmt.sample_rate()
                                    && current_format.channel_count() == next_fmt.channel_count();
                                // RG-track mode: the live stream's amplify
                                // factor is baked at create time; deny gapless
                                // when the next track needs a different gain.
                                let rg_allows_swap = renderer.lock().gapless_swap_allowed();
                                if !rg_allows_swap {
                                    tracing::debug!(
                                        "🔄 [DECODE LOOP] RG-track gain differs — denying gapless swap"
                                    );
                                }

                                if formats_match && rg_allows_swap {
                                    // Take the next decoder and swap it into the primary slot
                                    if let Some(next_dec) = next_dec_guard.take() {
                                        let next_duration = next_dec.duration();
                                        let next_source_url =
                                            next_source_shared.lock().await.clone();
                                        let next_codec = next_dec.live_codec();

                                        // Swap into primary decoder
                                        *decoder.lock().await = next_dec;
                                        *next_track_prepared.lock().await = false;

                                        // Increment source generation for stale callback detection
                                        source_generation.bump_for_gapless();

                                        // Reset renderer position for the new track and
                                        // promote the staged crossfade RG to "current"
                                        // (since we're keeping the same stream, the
                                        // amplify factor is already correct — we just
                                        // need our bookkeeping to reflect the new track).
                                        {
                                            let mut r = renderer.lock();
                                            r.reset_position();
                                            r.reset_finished_called();
                                            r.adopt_pending_crossfade_replay_gain();
                                        }

                                        // Store transition info for the engine to pick up
                                        {
                                            let mut info = gapless_info.lock().await;
                                            *info = Some(GaplessTransitionInfo {
                                                source: next_source_url,
                                                duration: next_duration,
                                                format: next_fmt,
                                                codec: next_codec,
                                            });
                                        }

                                        // Fire completion callback so the UI updates
                                        // (queue advances, track info refreshes)
                                        if let Some(ref cb) = completion_callback {
                                            cb(false);
                                        }

                                        tracing::info!(
                                            "🎵 [DECODE LOOP] Gapless transition — continuing decode loop"
                                        );
                                        backpressure_active = false;
                                        true
                                    } else {
                                        false
                                    }
                                } else {
                                    tracing::debug!(
                                        "🔄 [DECODE LOOP] Format mismatch for gapless: {:?} → {:?}",
                                        current_format,
                                        next_fmt
                                    );
                                    false
                                }
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    };

                    if did_gapless {
                        // Successfully swapped — continue the decode loop
                        // with the new decoder (no gap!)
                        continue;
                    }

                    // No gapless possible — signal EOF and exit
                    decoder_eof.store(true, Ordering::Release);
                    tracing::debug!("📭 [DECODE LOOP] Decoder EOF — signaling renderer");
                    break;
                } else {
                    // Release decoder lock before sleeping
                    drop(decoder_guard);

                    // Temporary empty buffer (network stall, seek refill, etc.)
                    empty_buffer_count += 1;
                    tracing::trace!(
                        "📭 [DECODE LOOP] Empty/invalid buffer received, waiting for decoder"
                    );
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                    continue;
                }
            }
        });
    }

    /// Pause
    pub fn pause(&mut self) {
        if !self.playing {
            return;
        }

        // Capture current position from renderer before pausing
        // This ensures position() returns the correct paused position
        {
            let renderer = self.renderer.lock();
            self.position = renderer.position();
        }

        self.paused = true;
        self.playing = false;
        {
            let mut renderer = self.renderer.lock();
            renderer.pause();
        }
        self.state = PlaybackState::Paused;
    }

    /// Stop
    pub async fn stop(&mut self) {
        if !self.playing && !self.paused {
            return;
        }

        // Cancel any active crossfade
        self.cancel_crossfade().await;

        // Unconditionally disarm renderer's crossfade trigger.
        // cancel_crossfade() skips when phase is Idle, but the renderer
        // may still be armed from prepare_next_for_gapless().
        {
            self.renderer.lock().disarm_crossfade();
        }

        // Stop decoding loop by advancing the generation counter.
        // Any running loop will see the mismatch and exit.
        self.decode_loop.supersede();

        // Stop render thread
        self.stop_render_thread();

        self.reset_next_track().await;
        {
            let mut renderer = self.renderer.lock();
            renderer.stop();
        }

        self.playing = false;
        self.paused = false;
        self.position = 0;
        self.duration = 0;
        self.live_bitrate.store(0, Ordering::Relaxed);
        self.live_sample_rate.store(0, Ordering::Relaxed);
        self.state = PlaybackState::Stopped;
    }

    /// Seek to position (milliseconds)
    ///
    /// Stops the decoding loop temporarily, performs the seek, then restarts.
    /// This ensures the decoder lock is available for seeking.
    pub async fn seek(&mut self, position_ms: u64) {
        use tracing::{debug, trace, warn};

        let seek_start = std::time::Instant::now();
        debug!(
            "🔍 [SEEK] Starting seek to {}ms (duration={}ms)",
            position_ms, self.duration
        );

        if self.duration == 0 {
            debug!("🔍 [SEEK] Aborting - duration is 0");
            return;
        }

        // CRITICAL FIX: Stop the decoding loop FIRST, before trying to acquire decoder lock!
        // The decoding loop holds the decoder lock while doing HTTP I/O (which can take 20+ seconds).
        // If we try to acquire the lock before stopping the loop, we'll block for the entire I/O duration.
        trace!("🔍 [SEEK] Stopping decoding loop FIRST");

        // Cancel any active crossfade before seeking
        self.cancel_crossfade().await;

        // Clear EOF — decoder will restart from seek position
        self.decoder_eof.store(false, Ordering::Release);

        self.decode_loop.supersede();

        // Give the decoding loop time to notice the flag and release the lock
        trace!("🔍 [SEEK] Waiting for decoding loop to release lock");
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;

        // NOW we can safely acquire the lock for the init check
        let decoder_initialized = {
            trace!("🔍 [SEEK] Acquiring decoder lock for init check...");
            let lock_start = std::time::Instant::now();
            let decoder = self.decoder.lock().await;
            trace!(
                "🔍 [SEEK] Decoder lock acquired in {:?}",
                lock_start.elapsed()
            );
            decoder.is_initialized()
        };

        if !decoder_initialized {
            debug!("🔍 [SEEK] Aborting - decoder not initialized");
            // Restart the decoding loop (start_decoding_loop handles generation)
            self.start_decoding_loop();
            return;
        }

        // Set seeking flag to prevent EOF detection during seek
        trace!("🔍 [SEEK] Setting seeking flag");
        self.seeking.store(true, Ordering::Release);

        let pos = position_ms.min(self.duration);

        // Acquire the async decoder lock natively, then use block_in_place
        // for the blocking HTTP I/O (RangeHttpReader uses reqwest::blocking).
        // This matches the proven pattern in play() and the decode loop.
        trace!("🔍 [SEEK] Acquiring decoder lock...");
        let lock_start = std::time::Instant::now();
        let mut decoder = self.decoder.lock().await;
        trace!(
            "🔍 [SEEK] Decoder lock acquired in {:?}",
            lock_start.elapsed()
        );

        let blocking_start = std::time::Instant::now();
        let seek_result = tokio::task::block_in_place(|| {
            trace!("🔍 [SEEK] Calling decoder.seek({})", pos);
            let seek_op_start = std::time::Instant::now();
            let seek_ok = decoder.seek(pos);
            debug!(
                "🔍 [SEEK] decoder.seek() completed in {:?}, success={}",
                seek_op_start.elapsed(),
                seek_ok
            );

            if seek_ok {
                trace!("🔍 [SEEK] Acquiring renderer lock...");
                let mut renderer = self.renderer.lock();
                renderer.seek(pos);

                // PREBUFFERING: Queue initial buffers after seek
                const SEEK_PREBUFFER_COUNT: usize = 10;
                trace!("🔍 [SEEK] Prebuffering {} buffers", SEEK_PREBUFFER_COUNT);

                for i in 0..SEEK_PREBUFFER_COUNT {
                    let buffer_size = decode_buffer_size(decoder.format());

                    let buffer = decoder.read_buffer(buffer_size);
                    if buffer.is_valid() && buffer.byte_count() > 0 {
                        let samples = s16_bytes_to_f32(buffer.data());
                        renderer.write_samples(&samples);
                        trace!(
                            "🔍 [SEEK] Queued prebuffer {}/{}",
                            i + 1,
                            SEEK_PREBUFFER_COUNT
                        );
                    } else {
                        trace!(
                            "🔍 [SEEK] Prebuffering stopped at {}/{} (no more data)",
                            i + 1,
                            SEEK_PREBUFFER_COUNT
                        );
                        break;
                    }
                }
            }

            seek_ok
        });
        drop(decoder);
        debug!(
            "🔍 [SEEK] Seek + prebuffer completed in {:?}, success={}",
            blocking_start.elapsed(),
            seek_result
        );

        if seek_result {
            self.position = pos;
        } else {
            warn!("🔍 [SEEK] Seek operation failed!");
        }

        // Restart the decoding loop
        trace!("🔍 [SEEK] Restarting decoding loop");
        self.start_decoding_loop();

        // Clear seeking flag
        trace!("🔍 [SEEK] Clearing seeking flag");
        self.seeking.store(false, Ordering::Release);

        debug!(
            "🔍 [SEEK] Seek completed in {:?} total",
            seek_start.elapsed()
        );
    }

    /// Load track
    pub async fn load_track(&mut self, url: &str) {
        debug!(" AudioEngine: loading track: {}", url);
        self.set_source(url.to_string()).await;
    }

    /// Atomic three-step: stash ReplayGain → set source. The caller still
    /// invokes `play()` afterward, but the RG-stash + source-update pair is
    /// uncuttable. Replaces the historical `set_pending_replay_gain` +
    /// `load_track` / `set_source` pairing in `PlaybackController`.
    pub async fn load_track_with_rg(
        &mut self,
        url: &str,
        rg: Option<crate::types::song::ReplayGain>,
    ) {
        self.renderer.lock().set_pending_replay_gain(rg);
        self.set_source(url.to_string()).await;
    }

    /// Prepare next track for gapless playback
    /// NOTE: This method holds the engine lock during the HTTP download.
    /// For better visualizer performance, use store_prepared_decoder() instead.
    pub async fn prepare_next_track(
        &mut self,
        url: &str,
        replay_gain: Option<crate::types::song::ReplayGain>,
    ) {
        self.reset_next_track().await;
        *self.next_track_prepared.lock().await = false;

        if url.is_empty() {
            return;
        }

        // Don't prepare if it's the same as current source
        if url == self.source {
            return;
        }

        // Create and initialize next decoder
        let mut next_decoder = AudioDecoder::new(self.live_icy_metadata.clone());
        if next_decoder.init(url).await.is_ok() {
            let incoming_duration = next_decoder.duration();
            self.next_format = next_decoder.format().clone();
            *self.next_decoder.lock().await = Some(next_decoder);
            self.next_source = url.to_string();
            *self.next_source_shared.lock().await = url.to_string();
            *self.next_track_prepared.lock().await = true;

            // Stash the incoming track's ReplayGain so the next crossfade
            // (or gapless transition) applies the right amplify factor.
            self.renderer
                .lock()
                .set_pending_crossfade_replay_gain(replay_gain);

            // Arm the renderer to trigger crossfade when the queue drains
            if self.crossfade_enabled && self.crossfade_duration_ms > 0 {
                self.renderer.lock().arm_crossfade(
                    self.crossfade_duration_ms,
                    &self.next_format,
                    self.duration,
                    incoming_duration,
                );
            }
        }
    }

    /// Store an already-initialized decoder for gapless playback.
    /// This is the preferred method for gapless prep because it doesn't block
    /// the engine lock during network I/O, allowing the visualizer to continue.
    ///
    /// Caller should:
    /// 1. Create and init the decoder OUTSIDE of engine lock (do the download)
    /// 2. Call this method briefly to store the ready decoder
    pub async fn store_prepared_decoder(
        &mut self,
        decoder: AudioDecoder,
        url: String,
        replay_gain: Option<crate::types::song::ReplayGain>,
    ) {
        // Check if we should store this decoder
        if url.is_empty() || url == self.source {
            return;
        }

        // Only reset if we're actually going to store something new
        if self.next_source != url {
            self.reset_next_track().await;
        }

        self.next_format = decoder.format().clone();
        *self.next_decoder.lock().await = Some(decoder);
        self.next_source = url;
        *self.next_source_shared.lock().await = self.next_source.clone();
        *self.next_track_prepared.lock().await = true;

        // Stash the incoming track's ReplayGain so the next crossfade
        // (or gapless transition) applies the right amplify factor.
        self.renderer
            .lock()
            .set_pending_crossfade_replay_gain(replay_gain);

        // Arm the renderer to trigger crossfade when the queue drains
        if self.crossfade_enabled && self.crossfade_duration_ms > 0 {
            // decoder was moved into next_decoder — read duration from it
            let incoming_duration = self
                .next_decoder
                .lock()
                .await
                .as_ref()
                .map_or(0, |d| d.duration());
            self.renderer.lock().arm_crossfade(
                self.crossfade_duration_ms,
                &self.next_format,
                self.duration,
                incoming_duration,
            );
        }
    }

    /// Consume gapless transition info that was set by the decode loop.
    /// Updates the engine's metadata (source, duration, format) to reflect
    /// the track that the decode loop has already swapped to.
    pub async fn consume_gapless_transition(&mut self) {
        let info = self.gapless_transition_info.lock().await.take();
        if let Some(info) = info {
            debug!(
                "🎵 [GAPLESS] Consuming transition: source={}, duration={}, format={:?}",
                info.source, info.duration, info.format
            );
            self.source = info.source;
            self.duration = info.duration;
            self.position = 0;
            self.current_format = info.format;
            if let Ok(mut guard) = self.live_codec_name.write() {
                *guard = info.codec;
            }
            self.next_source.clear();
            *self.next_source_shared.lock().await = String::new();
            self.live_sample_rate
                .store(self.current_format.sample_rate(), Ordering::Relaxed);
        }
    }

    // =========================================================================
    // Crossfade Engine API
    // =========================================================================

    /// Set crossfade enabled from settings
    pub fn set_crossfade_enabled(&mut self, enabled: bool) {
        self.crossfade_enabled = enabled;
    }

    /// Set crossfade duration from settings (in seconds)
    pub fn set_crossfade_duration(&mut self, duration_secs: u32) {
        let ms = duration_secs as u64 * 1000;
        self.crossfade_duration_ms = ms;
        self.crossfade_duration_shared.store(ms, Ordering::Relaxed);
    }

    /// Whether crossfade is enabled
    pub fn crossfade_enabled(&self) -> bool {
        self.crossfade_enabled
    }

    /// Crossfade duration in milliseconds
    pub fn crossfade_duration_ms(&self) -> u64 {
        self.crossfade_duration_ms
    }

    // =========================================================================
    // Volume Normalization API
    // =========================================================================

    /// Update volume normalization settings on the renderer.
    ///
    /// Takes effect on the next stream creation (play, seek, crossfade).
    pub fn set_volume_normalization(
        &mut self,
        mode: crate::types::player_settings::VolumeNormalizationMode,
        target_level: f32,
        preamp_db: f32,
        fallback_db: f32,
        fallback_to_agc: bool,
        prevent_clipping: bool,
    ) {
        let mut renderer = self.renderer.lock();
        renderer.set_volume_normalization(
            mode,
            target_level,
            preamp_db,
            fallback_db,
            fallback_to_agc,
            prevent_clipping,
        );
    }

    /// Stash ReplayGain tags for the next primary-stream creation
    /// (next `load_track` / `set_source` / `seek_to`).
    pub fn set_pending_replay_gain(&mut self, rg: Option<crate::types::song::ReplayGain>) {
        self.renderer.lock().set_pending_replay_gain(rg);
    }

    /// Stash ReplayGain tags for the next crossfade-stream creation
    /// (next `prepare_next_track` / `store_prepared_decoder`).
    pub fn set_pending_crossfade_replay_gain(
        &mut self,
        rg: Option<crate::types::song::ReplayGain>,
    ) {
        self.renderer.lock().set_pending_crossfade_replay_gain(rg);
    }

    /// Update shared EQ state. Replaces existing eq state, taking effect on new streams.
    pub fn set_eq_state(&mut self, state: super::eq::EqState) {
        let mut renderer = self.renderer.lock();
        renderer.set_eq_state(state);
    }

    /// Whether the engine is in the `Idle` crossfade phase.
    pub fn crossfade_is_idle(&self) -> bool {
        self.crossfade_phase.is_idle()
    }

    /// Whether the engine is mid-crossfade (`Active` or `OutgoingFinished`).
    pub fn crossfade_is_active_or_finished(&self) -> bool {
        self.crossfade_phase.is_active_or_finished()
    }

    /// Start a crossfade transition using the prepared next decoder.
    /// Returns `true` if crossfade was started successfully.
    pub async fn start_crossfade(&mut self) -> bool {
        if !self.crossfade_phase.is_idle() {
            debug!("🔀 [CROSSFADE] Already active, skipping");
            return false;
        }

        // Check if we have a prepared next track
        let has_prepared = *self.next_track_prepared.lock().await;
        if !has_prepared {
            debug!("🔀 [CROSSFADE] No prepared decoder, cannot start");
            return false;
        }

        // Take the prepared decoder for crossfade use
        let next_decoder_opt = self.next_decoder.lock().await.take();
        let next_decoder = match next_decoder_opt {
            Some(d) => d,
            None => {
                debug!("🔀 [CROSSFADE] Prepared flag set but no decoder, skipping");
                return false;
            }
        };
        *self.next_track_prepared.lock().await = false;

        let incoming_format = next_decoder.format().clone();
        let duration_ms = self.crossfade_duration_ms;
        let incoming_source = self.next_source.clone();
        self.next_source.clear();

        debug!(
            "🔀 [CROSSFADE] Starting: outgoing={:?}, incoming={:?}, duration={}ms",
            self.current_format, incoming_format, duration_ms
        );

        // Wrap the decoder in a shared Arc<Mutex<Option<...>>> — this same Arc
        // is stored inside the `Active` variant AND captured by the spawned
        // decode loop. The loop watches its inner `Option` for `None` as the
        // signal to exit (see `cancel_crossfade` / `finalize_crossfade_engine`).
        let decoder_arc = Arc::new(tokio::sync::Mutex::new(Some(next_decoder)));

        // Only tell the renderer to start crossfade if it hasn't already
        // been activated synchronously by the renderer's queue-threshold
        // trigger. The renderer may have already called start_crossfade()
        // on itself before this async path runs.
        {
            let mut renderer = self.renderer.lock();
            if !renderer.is_crossfade_active() {
                renderer.start_crossfade(duration_ms, &incoming_format);
            }
        }

        self.crossfade_phase = CrossfadePhase::Active {
            decoder: decoder_arc.clone(),
            incoming_source,
        };

        // Start a decode loop for the incoming track
        self.start_crossfade_decode_loop(decoder_arc);

        true
    }

    /// Cancel an active crossfade (e.g., on skip, seek, or stop).
    pub async fn cancel_crossfade(&mut self) {
        let phase = std::mem::replace(&mut self.crossfade_phase, CrossfadePhase::Idle);
        let decoder = match phase {
            CrossfadePhase::Idle => return,
            CrossfadePhase::Active { decoder, .. }
            | CrossfadePhase::OutgoingFinished { decoder, .. } => decoder,
        };
        debug!("🔀 [CROSSFADE] Cancelling");
        // Signal the spawned decode loop to exit by clearing its inner Option.
        *decoder.lock().await = None;
        let mut renderer = self.renderer.lock();
        renderer.cancel_crossfade();
        renderer.disarm_crossfade();
    }

    /// Finalize crossfade: promote the incoming track to become the current track.
    /// Called when the renderer finishes mixing (crossfade progress reaches 1.0)
    /// or when the outgoing decoder's buffers are fully consumed.
    pub async fn finalize_crossfade_engine(&mut self) {
        let phase = std::mem::replace(&mut self.crossfade_phase, CrossfadePhase::Idle);
        let (decoder_arc, incoming_source) = match phase {
            CrossfadePhase::Idle => return,
            CrossfadePhase::Active {
                decoder,
                incoming_source,
            }
            | CrossfadePhase::OutgoingFinished {
                decoder,
                incoming_source,
            } => (decoder, incoming_source),
        };

        debug!("🔀 [CROSSFADE] Finalizing — incoming becomes current");

        // Stop outgoing decode loop by advancing generation
        self.decode_loop.supersede();

        // Take the crossfade decoder and make it the primary
        let crossfade_dec = decoder_arc.lock().await.take();
        if let Some(decoder) = crossfade_dec {
            // Swap decoders
            *self.decoder.lock().await = decoder;
            let dec = self.decoder.lock().await;

            // Update engine state to reflect the incoming track
            self.source = incoming_source;
            self.current_format = dec.format().clone();
            self.live_sample_rate
                .store(self.current_format.sample_rate(), Ordering::Relaxed);
            self.duration = dec.duration();
            self.position = 0;
            self.next_format = AudioFormat::invalid();
            drop(dec);

            // Read the stored crossfade elapsed time and apply state resets.
            // The renderer already finalized (from render_buffers), so we just
            // read the stored elapsed time and reset position tracking.
            //
            // Do NOT call renderer.init() here — it clears the primary queue,
            // wiping the crossfade buffers that finalize_crossfade() just
            // transferred. Instead, do targeted state resets.
            let crossfade_elapsed_ms;
            {
                let mut renderer = self.renderer.lock();
                // Finalize the renderer-side crossfade: swap crossfade stream → primary,
                // reset crossfade_active. In the PipeWire architecture this was done by
                // render_buffers(), but in rodio we must do it explicitly here.
                renderer.finalize_crossfade();
                // Read the stored elapsed time for position offset.
                crossfade_elapsed_ms = renderer.take_crossfade_elapsed_ms();
                // Reset position tracking with offset: the incoming track has
                // been playing for crossfade_elapsed_ms already.
                renderer.reset_position_with_offset(crossfade_elapsed_ms);
                // Reset finished_called so on_renderer_finished can fire again
                renderer.reset_finished_called();
                renderer.set_volume(self.volume);
            }
            // Engine position also starts at the crossfade offset
            self.position = crossfade_elapsed_ms;

            // Intentional no-op (was: "Don't increment source_generation here")
            // — the crossfade was an intentional transition, not a user-
            // initiated skip.
            self.source_generation.accept_internal_swap();

            // Restart the primary decode loop with the new decoder
            self.start_decoding_loop();
        }

        // Notify completion callback (gapless-style: a new track started)
        if let Some(callback) = &self.completion_callback {
            callback(false);
        }
    }

    /// Start a decode loop for the incoming crossfade track.
    /// Similar to `start_decoding_loop` but writes to the renderer's crossfade buffer queue.
    ///
    /// `decoder_arc` is the same Arc stored inside `CrossfadePhase::Active.decoder`
    /// — the spawned loop watches its inner `Option` and exits when it
    /// becomes `None` (which `cancel_crossfade` / `finalize_crossfade_engine`
    /// trigger by clearing or taking the decoder respectively).
    fn start_crossfade_decode_loop(
        &mut self,
        decoder_arc: Arc<tokio::sync::Mutex<Option<AudioDecoder>>>,
    ) {
        let decoder = decoder_arc;
        let renderer = self.renderer.clone();
        let crossfade_duration_shared = self.crossfade_duration_shared.clone();

        tokio::spawn(async move {
            trace!("🔀 [CROSSFADE DECODE] Loop started");

            // Backpressure: dual-watermark strategy matching the primary decode loop.
            // Convert raw sample counts to ~100ms "buffer units" (÷800) and scale
            // watermarks with crossfade duration so the ring buffer can hold enough
            // audio for the full fade-in ramp.
            let mut backpressure_active = false;

            loop {
                // Check if crossfade is still active by checking if decoder still exists
                let decoder_guard = decoder.lock().await;
                let decoder_exists = decoder_guard.is_some();
                drop(decoder_guard);

                if !decoder_exists {
                    trace!("🔀 [CROSSFADE DECODE] Decoder removed, exiting loop");
                    break;
                }

                // Backpressure check — normalize to buffer units (same as primary loop)
                let buffer_count = {
                    let renderer_guard = renderer.lock();
                    renderer_guard.crossfade_buffer_count() / 800
                };

                let cf_ms = crossfade_duration_shared.load(Ordering::Relaxed);
                let (high_watermark, low_watermark) = compute_watermarks(cf_ms);

                if buffer_count >= high_watermark {
                    if !backpressure_active {
                        trace!(
                            "🔀 [CROSSFADE DECODE] Backpressure ON: {} >= {} (cf={}ms)",
                            buffer_count, high_watermark, cf_ms,
                        );
                        backpressure_active = true;
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    continue;
                } else if backpressure_active && buffer_count <= low_watermark {
                    trace!(
                        "🔀 [CROSSFADE DECODE] Backpressure OFF: {} <= {}",
                        buffer_count, low_watermark
                    );
                    backpressure_active = false;
                } else if backpressure_active {
                    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                    continue;
                }

                // Decode a buffer from the incoming track
                let mut decoder_guard = decoder.lock().await;
                let dec = match decoder_guard.as_mut() {
                    Some(d) => d,
                    None => break,
                };

                if !dec.is_initialized() || dec.is_eof() {
                    trace!("🔀 [CROSSFADE DECODE] EOF or not initialized, exiting loop");
                    drop(decoder_guard);
                    break;
                }

                let buffer_size = decode_buffer_size(dec.format());

                let buffer = tokio::task::block_in_place(|| dec.read_buffer(buffer_size));
                drop(decoder_guard);

                if buffer.is_valid() && buffer.byte_count() > 0 {
                    let samples = s16_bytes_to_f32(buffer.data());
                    let mut renderer_guard = renderer.lock();
                    renderer_guard.write_crossfade_samples(&samples);
                    drop(renderer_guard);
                } else {
                    // No data, wait a bit
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                }
            }

            trace!("🔀 [CROSSFADE DECODE] Loop finished");
        });
    }

    /// Load prepared track (for gapless transition)
    pub async fn load_prepared_track(&mut self) -> Result<()> {
        let mut next_decoder_guard = self.next_decoder.lock().await;
        let next_decoder = match next_decoder_guard.take() {
            Some(d) => d,
            None => {
                anyhow::bail!("No prepared track to load");
            }
        };
        drop(next_decoder_guard);

        // Stop current decoding loop before swapping decoders
        self.decode_loop.supersede();

        // Store previous format for gapless detection
        let prev_format = self.current_format.clone();

        // Switch decoders
        *self.decoder.lock().await = next_decoder;
        let decoder = self.decoder.lock().await;

        // Update source and format
        self.source = self.next_source.clone();
        self.next_source.clear();
        self.current_format = decoder.format().clone();
        self.live_sample_rate
            .store(self.current_format.sample_rate(), Ordering::Relaxed);
        self.next_format = AudioFormat::invalid();
        *self.next_track_prepared.lock().await = false; // Reset flag after loading prepared track

        // Update duration
        self.duration = decoder.duration();
        self.position = 0;
        drop(decoder);

        // Check if formats match for gapless playback
        let formats_match = prev_format.is_valid()
            && self.current_format.is_valid()
            && prev_format == self.current_format;
        let force_reload = !formats_match;

        debug!(
            "🔄 [GAPLESS] Transition: prev={:?} → cur={:?}, formats_match={}, force_reload={}, source={}",
            prev_format, self.current_format, formats_match, force_reload, self.source
        );

        // Initialize renderer with format-aware gapless logic
        let should_start = {
            let mut renderer = self.renderer.lock();
            renderer.init(&self.current_format, force_reload, Some(&prev_format))?;

            // Apply current volume to renderer
            renderer.set_volume(self.volume);

            // If we were playing, continue playing
            if self.playing && !self.paused {
                renderer.start();
                true
            } else {
                false
            }
        }; // renderer lock dropped here, before any .await

        if should_start {
            // Restart decoding loop for the new track
            self.start_decoding_loop();
            // Restart render thread for new track
            self.start_render_thread();
        }

        Ok(())
    }

    /// Immediate state access methods for UI-critical operations
    /// These avoid async locks for better responsiveness
    /// Get immediate playing state (for UI updates that need instant response)
    pub fn immediate_playing(&self) -> bool {
        self.playing && !self.paused
    }

    /// Get immediate paused state
    pub fn immediate_paused(&self) -> bool {
        self.paused
    }

    /// Get current sample rate in Hz (for UI display)
    /// Uses lock-free atomic for threading consistency with live_bitrate.
    pub fn sample_rate(&self) -> u32 {
        self.live_sample_rate.load(Ordering::Relaxed)
    }

    /// Get live compressed bitrate in kbps (updated per-packet from decoder)
    pub fn live_bitrate(&self) -> u32 {
        self.live_bitrate.load(Ordering::Relaxed)
    }

    /// Current source generation (incremented on every `set_source` call).
    /// Used by the renderer's stale-callback guard.
    pub fn source_generation(&self) -> u64 {
        self.source_generation.current()
    }

    /// Clear the prepared next-track decoder and all associated state.
    ///
    /// Call this whenever the play order changes (shuffle/repeat/consume toggle)
    /// to prevent a stale gapless transition to the wrong song.
    pub async fn reset_next_track(&mut self) {
        *self.next_decoder.lock().await = None;
        *self.next_track_prepared.lock().await = false;
        self.next_source.clear();
        *self.next_source_shared.lock().await = String::new();
        self.next_format = AudioFormat::invalid();
        self.renderer.lock().disarm_crossfade();
    }

    /// Get playback state
    pub fn state(&self) -> PlaybackState {
        self.state
    }

    /// Set completion callback.
    ///
    /// The callback receives `true` when the same track is looping (repeat-one),
    /// `false` when a different track starts.
    pub fn set_completion_callback<F>(&mut self, callback: F)
    where
        F: Fn(bool) + Send + Sync + 'static,
    {
        self.completion_callback = Some(Arc::new(callback));
    }

    /// Set visualizer callback
    pub fn set_visualizer_callback(
        &mut self,
        callback: crate::audio::renderer::VisualizerCallback,
    ) {
        let renderer = self.renderer.lock();
        renderer.set_visualizer_callback(callback);
    }

    /// Set the shared mixer from the app-wide MixerDeviceSink.
    /// Delegates to the renderer so it can use the shared mixer on first play.
    pub fn set_shared_mixer(&mut self, mixer: rodio::mixer::Mixer) {
        let mut renderer = self.renderer.lock();
        renderer.set_shared_mixer(mixer);
    }

    /// Enable/disable PipeWire-native volume control on the renderer.
    ///
    /// When `true`, the renderer keeps software volume at 1.0 and the
    /// caller is responsible for mirroring volume changes to PipeWire.
    pub fn set_pw_volume_active(&mut self, active: bool) {
        let mut renderer = self.renderer.lock();
        renderer.set_pw_volume_active(active);
    }

    /// Set engine reference in renderer
    pub fn set_engine_reference(&mut self, engine: Weak<tokio::sync::Mutex<CustomAudioEngine>>) {
        let mut renderer = self.renderer.lock();
        renderer.set_engine_link(
            engine,
            self.source_generation.clone(),
            self.decoder_eof.clone(),
        );
    }

    /// Check if next track is prepared for gapless playback
    pub async fn is_next_track_prepared(&self) -> bool {
        *self.next_track_prepared.lock().await
    }

    /// Handle renderer finished (called when renderer runs out of buffers)
    /// This matches the C++ onRendererFinished implementation
    /// Returns true if the track was actually finished
    pub async fn on_renderer_finished(&mut self) -> bool {
        // Don't trigger track end if we're in the middle of seeking
        if self.seeking.load(Ordering::Acquire) {
            trace!(" [RENDERER FINISHED] Ignoring - seek in progress");
            return false;
        }

        // Renderer finished all its buffers - check if track is truly finished
        let decoder = self.decoder.lock().await;
        let is_eof = decoder.is_eof();
        let duration = decoder.duration();
        drop(decoder);

        let position = self.position();

        debug!(
            " [RENDERER FINISHED] EOF={}, position={}ms, duration={}ms, playing={}, paused={}",
            is_eof, position, duration, self.playing, self.paused
        );

        // Phase 1: Crossfade finalization — outgoing queue drained
        if self.try_finalize_crossfade(is_eof).await {
            return false;
        }

        // Phase 2: Crossfade initiation — position-based trigger fired
        if self.try_start_crossfade_transition(is_eof).await {
            return false;
        }

        // Phase 3: Normal track completion or buffer starvation
        let position_indicates_finished = duration > 0 && position >= duration;
        if is_eof || position_indicates_finished {
            debug!(
                " [RENDERER FINISHED] Track finished (EOF={}, pos={} >= dur={}, pos_finished={})",
                is_eof, position, duration, position_indicates_finished
            );
            self.on_decoder_finished().await;
            true
        } else if !is_eof && self.playing && !self.paused {
            self.handle_buffer_starvation(position, duration).await
        } else {
            trace!(" [RENDERER FINISHED] Not playing or paused, no action taken");
            false
        }
    }

    /// Check if an active crossfade's outgoing queue has drained and finalize it.
    ///
    /// Handles both phases:
    /// - `Active + is_eof`: queue drained BEFORE decoder signaled EOF (race)
    /// - `OutgoingFinished`: decoder already signaled EOF, queue drained after
    async fn try_finalize_crossfade(&mut self, is_eof: bool) -> bool {
        let should_finalize = matches!(
            (&self.crossfade_phase, is_eof),
            (CrossfadePhase::OutgoingFinished { .. }, _) | (CrossfadePhase::Active { .. }, true)
        );

        if should_finalize {
            debug!(
                "🔀 [RENDERER FINISHED] Outgoing queue drained during crossfade (phase={}, eof={}) — finalizing",
                self.crossfade_phase.label(),
                is_eof
            );
            self.finalize_crossfade_engine().await;
        }
        should_finalize
    }

    /// Try to start a crossfade transition if conditions are met.
    ///
    /// This is the main crossfade entry point: render_tick's position-based trigger
    /// fired (pos >= track_duration - crossfade_duration), disarmed the trigger,
    /// and signaled us. We start the crossfade from the engine so the decode loop
    /// and stream creation happen together.
    ///
    /// NOTE: Does NOT gate on is_eof — the position-based trigger fires
    /// intentionally BEFORE EOF so both tracks can overlap during the fade.
    async fn try_start_crossfade_transition(&mut self, is_eof: bool) -> bool {
        if !self.crossfade_phase.is_idle()
            || !self.crossfade_enabled
            || self.crossfade_duration_ms == 0
        {
            return false;
        }

        let has_prepared = *self.next_track_prepared.lock().await;
        if has_prepared {
            debug!(
                "🔀 [RENDERER FINISHED] Starting crossfade (prepared={}, eof={})",
                has_prepared, is_eof
            );
            self.start_crossfade().await;
        }
        has_prepared
    }

    /// Handle temporary buffer starvation when decoder hasn't reached EOF.
    ///
    /// Waits briefly for the decoder to produce more buffers (e.g., after seek
    /// or transient network stall). Returns `true` if the track actually ended.
    async fn handle_buffer_starvation(&mut self, position: u64, duration: u64) -> bool {
        debug!(
            " [RENDERER FINISHED] Buffer starvation detected (pos={}, dur={}), waiting",
            position, duration
        );

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let has_buffers = !self.renderer.lock().is_buffer_queue_empty();
        if has_buffers {
            trace!(" [RENDERER FINISHED] Buffers recovered after wait");
            return false;
        }

        // Still no buffers — re-check decoder state
        let decoder = self.decoder.lock().await;
        let still_eof = decoder.is_eof();
        drop(decoder);

        if still_eof {
            debug!("🎵 [RENDERER FINISHED] Decoder reached EOF after wait, finishing track");
            self.on_decoder_finished().await;
            true
        } else {
            trace!(" [RENDERER FINISHED] Decoder still producing, continuing to wait");
            false
        }
    }

    /// Handle decoder finished (track completed)
    async fn on_decoder_finished(&mut self) {
        debug!(
            "🎵 [DECODER FINISHED] source={}, crossfade_phase={}, playing={}, paused={}",
            self.source,
            self.crossfade_phase.label(),
            self.playing,
            self.paused
        );

        // If crossfade is active and the outgoing decoder finished, that's expected.
        // The renderer still has buffered outgoing audio to mix — transition to
        // OutgoingFinished to let the renderer continue the crossfade using
        // already-buffered data. Engine finalization happens when the renderer
        // completes the crossfade (crossfade_done) or the outgoing queue drains.
        match std::mem::replace(&mut self.crossfade_phase, CrossfadePhase::Idle) {
            CrossfadePhase::Active {
                decoder,
                incoming_source,
            } => {
                debug!(
                    "🔀 [DECODER FINISHED] Outgoing EOF during crossfade — phase → OutgoingFinished"
                );
                self.crossfade_phase = CrossfadePhase::OutgoingFinished {
                    decoder,
                    incoming_source,
                };
                return;
            }
            phase @ CrossfadePhase::OutgoingFinished { .. } => {
                debug!("🔀 [DECODER FINISHED] Ignoring — OutgoingFinished, waiting for renderer");
                self.crossfade_phase = phase;
                return;
            }
            CrossfadePhase::Idle => {
                // Already restored to Idle by the mem::replace above; fall
                // through to the normal track-completion path.
            }
        }

        // Snapshot the current source so we can detect repeat-one loops after the
        // completion callback selects the next track.
        let source_before = self.source.clone();

        // Check if we have a prepared next track
        let has_prepared = {
            let next_decoder = self.next_decoder.lock().await;
            next_decoder.is_some()
        };

        if has_prepared {
            debug!(" Track finished, loading prepared next track");
            if self.load_prepared_track().await.is_ok() {
                // Gapless transition successful - continue playing
                // NOTE: load_prepared_track() already starts the decoding loop
                // and render thread, so we do NOT call start_decoding_loop() here.
                debug!(" Gapless transition successful (source: {})", self.source);
                // IMPORTANT: Still call completion callback so playback controller updates queue index!
                // Gapless always means a new track (we skip same-URL gapless prep), so is_loop=false.
                if let Some(callback) = &self.completion_callback {
                    callback(false);
                }
                return;
            }
            warn!(" Gapless transition failed, falling back to normal next song");
        } else {
            debug!(
                " [DECODER FINISHED] No prepared decoder available — will fall through to stop+callback"
            );
        }

        // No next track prepared, stop and emit finished
        debug!(" No prepared track, stopping playback");
        self.stop().await;
        if let Some(callback) = &self.completion_callback {
            // If the new source equals the old source, this is a repeat-one loop.
            let is_loop = !self.source.is_empty() && self.source == source_before;
            debug!(
                " [DECODER FINISHED] Calling completion callback (is_loop={})",
                is_loop
            );
            callback(is_loop);
        }
    }

    /// Start the dedicated render thread.
    /// With rodio, the actual audio rendering is done by the cpal callback.
    /// This thread just handles control logic: crossfade ticking, completion
    /// detection, etc. Runs at 20ms intervals (50Hz — sufficient for smooth
    /// crossfade curves and responsive completion detection).
    fn start_render_thread(&mut self) {
        // Stop any existing render thread first
        self.stop_render_thread();

        let renderer = self.renderer.clone();
        let running = self.render_running.clone();
        running.store(true, Ordering::Release);

        let handle = std::thread::Builder::new()
            .name("audio-render".into())
            .spawn(move || {
                trace!("🔊 [RENDER THREAD] Started");
                while running.load(Ordering::Acquire) {
                    {
                        let mut r = renderer.lock();
                        r.render_tick();
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                trace!("🔊 [RENDER THREAD] Stopped");
            })
            .expect("Failed to spawn audio render thread");

        self.render_thread = Some(handle);
    }

    /// Stop the dedicated render thread
    fn stop_render_thread(&mut self) {
        self.render_running.store(false, Ordering::Release);
        if let Some(handle) = self.render_thread.take() {
            let _ = handle.join();
        }
    }
}

impl Default for CustomAudioEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for CustomAudioEngine {
    fn drop(&mut self) {
        // Kill the decode loop immediately (lock-free atomic check).
        self.decode_loop.supersede();

        // Stop the render thread — sets render_running=false and joins the OS thread.
        // Without this, the render thread keeps feeding buffered audio to CPAL/PipeWire
        // for several seconds after the window closes.
        self.stop_render_thread();

        // Stop the audio renderer — sets `stopped=true` on the StreamingSourceHandle,
        // causing the CPAL callback to immediately emit silence instead of draining
        // the ring buffer.
        self.renderer.lock().stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// IG-8: the typestate makes `Idle → OutgoingFinished` direct transition
    /// impossible. Previously the bool-flag representation could be set to
    /// any value at any time; now `OutgoingFinished` only exists by destructuring
    /// `Active` (its `decoder` and `incoming_source` are non-`Default`,
    /// so they can only come from a prior `Active` state).
    ///
    /// This test exercises the runtime side: feeding `on_decoder_finished`
    /// to a fresh engine (which starts in `Idle`) must NOT promote the phase
    /// to `OutgoingFinished` — the match arm in `on_decoder_finished` falls
    /// through to the normal track-completion path instead.
    #[tokio::test]
    async fn crossfade_idle_cannot_transition_directly_to_outgoing_finished() {
        let mut engine = CustomAudioEngine::new();

        assert!(
            engine.crossfade_is_idle(),
            "fresh engine must start in Idle"
        );

        engine.on_decoder_finished().await;

        assert!(
            engine.crossfade_is_idle(),
            "phase must remain Idle when no crossfade is active",
        );
        assert!(
            !engine.crossfade_is_active_or_finished(),
            "Idle predicate must NOT report Active/OutgoingFinished",
        );
    }
}
