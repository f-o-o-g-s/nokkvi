//! Shared visualizer state
//!
//! Manages FFT processing and shared state between UI and audio callback.
//! Uses a pure-Rust SpectrumEngine (RustFFT) for DSP — no C/FFI dependencies.
//! Configuration is loaded from config.toml and hot-reloadable.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use nokkvi_data::audio::spectrum::{self, SpectrumEngine};
use parking_lot::{Mutex, RwLock};
use tracing::{debug, trace};

use crate::visualizer_config::VisualizerConfig;

/// Maximum number of bars the FFT can meaningfully produce for a given sample rate.
///
/// Delegates to the SpectrumEngine's calculation (treble_buffer_size / 2).
pub(super) fn max_bars_for_sample_rate(sample_rate: u32) -> usize {
    spectrum::max_bars_for_sample_rate(sample_rate)
}

/// Linearly interpolate FFT output (fft_count bins) to a larger visual bar count.
///
/// Each visual bar maps to a fractional position in the FFT output and lerps
/// between the two nearest bins. Adjacent frequency bins are naturally correlated,
/// so the interpolation is visually seamless.
fn interpolate_bars(fft_output: &[f64], visual_count: usize) -> Vec<f64> {
    let fft_count = fft_output.len();
    if fft_count == 0 || visual_count == 0 {
        return vec![0.0; visual_count];
    }
    if visual_count <= fft_count {
        // No upsampling needed — just truncate (shouldn't happen in practice)
        return fft_output[..visual_count].to_vec();
    }

    let mut result = Vec::with_capacity(visual_count);
    let scale = (fft_count - 1) as f64 / (visual_count - 1) as f64;

    for i in 0..visual_count {
        let pos = i as f64 * scale;
        let idx_lo = (pos as usize).min(fft_count - 1);
        let idx_hi = (idx_lo + 1).min(fft_count - 1);
        let frac = pos - idx_lo as f64;
        let value = fft_output[idx_lo] * (1.0 - frac) + fft_output[idx_hi] * frac;
        result.push(value);
    }

    result
}

// ========================================
// Spectrum Engine Initialization Helper
// ========================================

/// Build a new SpectrumEngine instance with the given parameters.
///
/// Centralized function to ensure consistent configuration across all initialization sites:
/// - `new()` - Initial construction
/// - `apply_config()` - Config hot-reload
/// - `reinit_engine_with_current_sample_rate()` - Sample rate change
/// - `apply_pending_resize()` - Window resize
/// - `reset()` - Track change
fn build_spectrum_engine(
    bar_count: usize,
    sample_rate: u32,
    config: &SharedVisualizerConfig,
) -> Option<SpectrumEngine> {
    let cfg = config.read();
    let (auto_sensitivity, noise_reduction, lower_cutoff, higher_cutoff) = (
        cfg.auto_sensitivity,
        cfg.noise_reduction,
        cfg.lower_cutoff_freq,
        cfg.higher_cutoff_freq,
    );
    drop(cfg);

    SpectrumEngine::new(
        bar_count,
        sample_rate,
        auto_sensitivity,
        noise_reduction,
        lower_cutoff,
        higher_cutoff,
    )
    .ok()
}

/// Shared config type alias for convenience
pub(crate) type SharedVisualizerConfig = Arc<RwLock<VisualizerConfig>>;

// ========================================
// Consolidated Inner Structs
// ========================================

/// Output buffers read by the GPU renderer
/// Groups all display-related data behind a single lock
#[derive(Clone)]
struct DisplayBuffers {
    /// Frequency bar values (0.0-1.0 normalized)
    bars: Vec<f64>,
    /// Peak bar values (0.0-1.0 normalized) - tracks recent maximums
    peak_bars: Vec<f64>,
    /// Alpha values for each peak (1.0 = visible, 0.0 = hidden) - for fade mode
    peak_alphas: Vec<f64>,
    /// Flash intensity for each bar (0.0-1.0, decays over time)
    flash_intensities: Vec<f32>,
    /// Dirty flag: true when data has changed and needs GPU upload
    dirty: bool,
}

impl DisplayBuffers {
    fn new(bar_count: usize) -> Self {
        Self {
            bars: vec![0.0; bar_count],
            peak_bars: vec![0.0; bar_count],
            peak_alphas: vec![1.0; bar_count],
            flash_intensities: vec![0.0; bar_count],
            dirty: false,
        }
    }

    fn resize(&mut self, bar_count: usize) {
        self.bars = vec![0.0; bar_count];
        self.peak_bars = vec![0.0; bar_count];
        self.peak_alphas = vec![1.0; bar_count];
        self.flash_intensities = vec![0.0; bar_count];
    }

    fn clear(&mut self) {
        for bar in self.bars.iter_mut() {
            *bar = 0.0;
        }
        for peak in self.peak_bars.iter_mut() {
            *peak = 0.0;
        }
        for alpha in self.peak_alphas.iter_mut() {
            *alpha = 1.0;
        }
        for flash in self.flash_intensities.iter_mut() {
            *flash = 0.0;
        }
        self.dirty = true;
    }
}

/// Peak animation state (internal to tick)
/// Groups peak decay state that's only used during tick processing
#[derive(Clone)]
struct PeakState {
    /// Hold time for each peak (Duration remaining before decay starts)
    hold_times: Vec<Duration>,
    /// Decay velocities for each peak (accelerating fall)
    velocities: Vec<f64>,
}

const PEAK_INITIAL_VELOCITY: f64 = 0.01;

impl PeakState {
    fn new(bar_count: usize) -> Self {
        Self {
            hold_times: vec![Duration::ZERO; bar_count],
            velocities: vec![PEAK_INITIAL_VELOCITY; bar_count],
        }
    }

    fn resize(&mut self, bar_count: usize) {
        self.hold_times = vec![Duration::ZERO; bar_count];
        self.velocities = vec![PEAK_INITIAL_VELOCITY; bar_count];
    }
}

/// Shimmer/flash effect state (internal to tick)
/// Groups effect processing state
#[derive(Clone)]
struct EffectState {
    /// Previous bar values for detecting rapid increases
    prev_bars: Vec<f64>,
    /// Elapsed time tracker for flash effect timing
    elapsed_time: f32,
}

impl EffectState {
    fn new(bar_count: usize) -> Self {
        Self {
            prev_bars: vec![0.0; bar_count],
            elapsed_time: 0.0,
        }
    }

    fn resize(&mut self, bar_count: usize) {
        self.prev_bars = vec![0.0; bar_count];
    }

    fn clear(&mut self, bar_count: usize) {
        self.prev_bars = vec![0.0; bar_count];
        // Note: don't reset elapsed_time - it should continue monotonically
    }
}

/// Audio processing state
/// Groups smoothing and sync tracking
#[derive(Clone)]
struct ProcessingState {
    /// Number of samples processed since last reset (for sync calculation)
    processed_samples: u64,
}

impl ProcessingState {
    fn new(_bar_count: usize) -> Self {
        Self {
            processed_samples: 0,
        }
    }

    fn resize(&mut self, _bar_count: usize) {}

    fn clear(&mut self, _bar_count: usize) {
        self.processed_samples = 0;
    }
}

// ========================================
// Main State Struct
// ========================================

/// Visualizer state shared between UI and audio callback
#[derive(Clone)]
pub(crate) struct VisualizerState {
    /// Unique instance ID for debugging
    _instance_id: u64,

    // === Consolidated Buffers (R5 refactoring) ===
    /// Display output buffers (bars, peaks, flash) — single lock for renderer reads
    display: Arc<Mutex<DisplayBuffers>>,
    /// Peak animation state (hold times, velocities) — internal to tick
    peaks: Arc<Mutex<PeakState>>,
    /// Effect processing state (prev_bars, elapsed) — internal to tick
    effects: Arc<Mutex<EffectState>>,
    /// Audio processing state (smoothed, processed_samples) — internal to tick
    processing: Arc<Mutex<ProcessingState>>,

    // === Separate (different access patterns) ===
    /// Sample buffer for FFT processing (audio callback writes, tick reads)
    sample_buffer: Arc<Mutex<Vec<f64>>>,
    /// Spectrum engine (needs exclusive access, reinitializes on resize).
    /// `None` when initialization failed — tick() produces flat bars.
    engine: Arc<Mutex<Option<SpectrumEngine>>>,
    /// Sample rate for sync calculations (rarely changes)
    sample_rate: Arc<Mutex<u32>>,

    // === Shared Config ===
    /// Shared config for hot-reloadable settings
    config: SharedVisualizerConfig,

    // === Resize debouncing (prevents audio stutter during window resize) ===
    /// Pending bar count from resize request (0 = no pending resize)
    pending_bar_count: Arc<AtomicUsize>,
    /// Timestamp of last resize request for debouncing
    last_resize_request: Arc<Mutex<Option<Instant>>>,

    // === Track change buffer clear (atomic to avoid deadlocks) ===
    /// Flag to signal tick() should clear all display buffers
    pending_clear: Arc<AtomicBool>,
    /// Flag indicating we're rebuilding buffer after clear
    rebuilding_after_clear: Arc<AtomicBool>,

    /// Dynamic sample rate handling ===
    /// Cached chunk size based on current sample rate (updated atomically when rate changes)
    /// Formula: (sample_rate * 2 channels) / 60 FPS = samples per ~16.67ms tick
    cached_chunk_size: Arc<AtomicUsize>,
    /// Flag indicating engine needs reinitialization due to sample rate change
    pending_engine_reinit: Arc<AtomicBool>,
    /// Desired visual bar count (may exceed FFT limit; interpolation bridges the gap)
    visual_bar_count: Arc<AtomicUsize>,

    // === Background FFT Thread ===
    /// Flag to control background FFT thread (true = running, false = stop)
    fft_thread_running: Arc<AtomicBool>,
    /// Handle to background FFT thread (wrapped in Arc for Clone)
    fft_thread_handle: Arc<Mutex<Option<JoinHandle<()>>>>,

    // === Visualization mode (for mode-specific smoothing) ===
    /// True when in lines mode — skips CPU-side smoothing (lines smooth in GPU shader)
    is_lines_mode: Arc<AtomicBool>,
}

impl VisualizerState {
    pub(crate) fn new(bar_count: usize, config: SharedVisualizerConfig) -> Self {
        // Generate unique instance ID for debugging
        static INSTANCE_COUNTER: AtomicU64 = AtomicU64::new(0);
        let instance_id = INSTANCE_COUNTER.fetch_add(1, Ordering::Relaxed);

        // FFT bar count may be lower than visual if FFT-limited
        let fft_limit = max_bars_for_sample_rate(44100);
        let fft_count = bar_count.min(fft_limit);

        // Create spectrum engine (default 44.1kHz, will reinit when audio starts).
        // If this fails (e.g. resource exhaustion), the visualizer shows flat bars.
        let engine = build_spectrum_engine(fft_count, 44100, &config);
        if engine.is_none() {
            tracing::error!("Failed to initialize spectrum engine — visualizer disabled");
        }

        // Create background thread control
        let fft_thread_running = Arc::new(AtomicBool::new(true));
        let fft_thread_handle = Arc::new(Mutex::new(None));

        let state = Self {
            _instance_id: instance_id,
            // Consolidated buffers (sized to visual bar count)
            display: Arc::new(Mutex::new(DisplayBuffers::new(bar_count))),
            peaks: Arc::new(Mutex::new(PeakState::new(bar_count))),
            effects: Arc::new(Mutex::new(EffectState::new(bar_count))),
            processing: Arc::new(Mutex::new(ProcessingState::new(bar_count))),
            // Separate buffers
            sample_buffer: Arc::new(Mutex::new(Vec::new())),
            engine: Arc::new(Mutex::new(engine)),
            sample_rate: Arc::new(Mutex::new(44100)),
            config,
            // Resize debouncing
            pending_bar_count: Arc::new(AtomicUsize::new(0)),
            last_resize_request: Arc::new(Mutex::new(None)),
            pending_clear: Arc::new(AtomicBool::new(false)),
            rebuilding_after_clear: Arc::new(AtomicBool::new(false)),
            // Initialize chunk size for default 44100Hz: (44100 * 2) / 60 = 1470
            cached_chunk_size: Arc::new(AtomicUsize::new(1470)),
            pending_engine_reinit: Arc::new(AtomicBool::new(false)),
            visual_bar_count: Arc::new(AtomicUsize::new(bar_count)),
            // Background FFT thread
            fft_thread_running,
            fft_thread_handle,
            // Visualization mode
            is_lines_mode: Arc::new(AtomicBool::new(false)),
        };

        // Spawn the background FFT thread
        state.start_fft_thread();

        state
    }

    /// Start the background FFT processing thread
    ///
    /// This thread runs tick() at 60fps, completely independent of the UI/render thread.
    /// This is the key fix for visualizer lag when the system is under GPU load.
    fn start_fft_thread(&self) {
        let running = self.fft_thread_running.clone();
        let state_clone = self.clone();

        let handle = thread::Builder::new()
            .name("visualizer-fft".to_string())
            .spawn(move || {
                debug!("📊 [FFT THREAD] Started background FFT processing at 60fps");
                let mut frame_count: u64 = 0;
                let target_interval = Duration::from_micros(16667); // ~60fps

                while running.load(Ordering::Relaxed) {
                    let frame_start = Instant::now();

                    // Run FFT processing
                    state_clone.tick();

                    frame_count += 1;
                    if frame_count.is_multiple_of(600) {
                        trace!("📊 [FFT THREAD] Processed {} frames", frame_count);
                    }

                    // Sleep for remaining time to hit 60fps
                    let elapsed = frame_start.elapsed();
                    if elapsed < target_interval {
                        thread::sleep(target_interval - elapsed);
                    }
                }

                debug!("📊 [FFT THREAD] Stopped after {} frames", frame_count);
            });

        match handle {
            Ok(h) => *self.fft_thread_handle.lock() = Some(h),
            Err(e) => tracing::error!("Failed to spawn FFT thread — visualizer disabled: {e}"),
        }
    }

    /// Get audio callback for connecting to audio engine.
    /// This ONLY buffers samples — actual processing happens in tick() at 60 FPS.
    ///
    /// Accepts `&[f32]` directly from the `StreamingSource` (cpal audio thread).
    /// Converts f32→f64 inline during `extend` to avoid any heap allocation on
    /// the real-time audio thread.
    pub(crate) fn audio_callback(&self) -> impl Fn(&[f32], u32) + Send + Sync + use<> {
        let sample_buffer = self.sample_buffer.clone();
        let sample_rate_arc = self.sample_rate.clone();
        let cached_chunk_size = self.cached_chunk_size.clone();
        let pending_engine_reinit = self.pending_engine_reinit.clone();

        move |samples: &[f32], sample_rate: u32| {
            // Update sample rate and cached chunk size if changed
            {
                let mut sr = sample_rate_arc.lock();
                if *sr != sample_rate && sample_rate > 0 {
                    *sr = sample_rate;
                    // Calculate chunk size: samples for ~16.67ms at this rate
                    // Formula: (sample_rate * 2 channels) / 60 FPS
                    // Minimum 512 for efficiency
                    let new_chunk_size = ((sample_rate as usize * 2) / 60).max(512);
                    cached_chunk_size.store(new_chunk_size, Ordering::Release);
                    // Signal engine needs reinitialization with new sample rate
                    pending_engine_reinit.store(true, Ordering::Release);
                    tracing::debug!(
                        "Sample rate changed to {}Hz, chunk_size now {} samples, engine reinit pending",
                        sample_rate,
                        new_chunk_size
                    );
                }
            }

            // Buffer samples with inline f32→f64 conversion (zero-alloc on audio thread).
            // The parking_lot Mutex lock is ~20ns, well within audio deadline budgets.
            let mut buffer = sample_buffer.lock();
            buffer.extend(samples.iter().map(|&s| s as f64));

            // Simple buffer limit: keep ~10 seconds max to prevent unbounded growth
            // Use a generous limit that works for all sample rates up to 192kHz
            const MAX_BUFFER_SIZE: usize = 192000 * 2 * 10; // 10 seconds stereo at max rate
            if buffer.len() > MAX_BUFFER_SIZE {
                let excess = buffer.len() - MAX_BUFFER_SIZE;
                buffer.drain(..excess);
            }
        }
    }

    /// Process one chunk of buffered samples (called at 60 FPS from shader prepare())
    /// Returns true if the visualizer was updated
    pub(crate) fn tick(&self) -> bool {
        // Check for pending buffer clear (from track change callback)
        if self.pending_clear.swap(false, Ordering::SeqCst) {
            self.apply_pending_clear();
            return true;
        }

        // Check for pending resize (debounced)
        self.apply_pending_resize();

        // Check for pending engine reinitialization (sample rate changed)
        if self.pending_engine_reinit.swap(false, Ordering::SeqCst) {
            self.reinit_engine_with_current_sample_rate();
        }

        // Process samples in chunks matching ~16.67ms at current sample rate
        // Chunk size is dynamically calculated: (sample_rate * 2) / 60
        let chunk_size = self.cached_chunk_size.load(Ordering::Acquire);
        let min_buffer_after_reset = chunk_size;

        if self.rebuilding_after_clear.load(Ordering::SeqCst) {
            // Use try_lock to avoid blocking during shutdown
            let Some(buffer) = self.sample_buffer.try_lock() else {
                return false; // Lock contended, skip this tick
            };
            if buffer.len() < min_buffer_after_reset {
                return false;
            }
            self.rebuilding_after_clear.store(false, Ordering::SeqCst);
        }

        // Use try_lock to avoid blocking during shutdown - if locks are contended, skip this tick
        let Some(mut engine_guard) = self.engine.try_lock() else {
            return false; // Engine lock contended, skip this tick
        };
        let Some(ref mut engine) = *engine_guard else {
            return false; // Engine not initialized (init failed), skip
        };
        let Some(mut buffer) = self.sample_buffer.try_lock() else {
            return false; // Sample buffer lock contended, skip this tick
        };

        // Get FFT bar count (what the engine is initialized with, may be < visual count)
        let fft_bar_count = self.fft_bar_count();
        // Get visual bar count (what the display buffers are sized to)
        let visual_count = self.visual_bar_count.load(Ordering::Relaxed);

        // Buffer cap to prevent falling behind real-time (~50ms of audio)
        let max_buffer_size = chunk_size * 3;
        if buffer.len() > max_buffer_size {
            let samples_to_drop = buffer.len() - max_buffer_size;
            buffer.drain(..samples_to_drop);
        }

        if buffer.len() >= chunk_size {
            let process_samples: Vec<f64> = buffer.drain(..chunk_size).collect();

            // Track processed samples
            {
                let mut proc = self.processing.lock();
                proc.processed_samples += chunk_size as u64;
            }

            // Execute spectrum engine FFT at the FFT-limited bar count
            let mut fft_output = vec![0.0; fft_bar_count];
            engine.execute(&process_samples, &mut fft_output);

            {
                // Read config values for hot-reload support
                let cfg = self.config.read();
                let (waves, waves_smoothing, monstercat) =
                    (cfg.waves, cfg.waves_smoothing, cfg.monstercat);
                drop(cfg);

                // Apply smoothing filters on FFT output (before interpolation).
                // Only in bars mode — lines mode does its own GPU-side Catmull-Rom smoothing.
                if !self.is_lines_mode.load(Ordering::Relaxed) {
                    if waves {
                        waves_filter(&mut fft_output, waves_smoothing as usize);
                    } else if monstercat > 0.0 {
                        monstercat_filter(&mut fft_output, monstercat);
                    }
                }

                // Interpolate from FFT bins to visual bar count
                let output = if visual_count > fft_bar_count {
                    interpolate_bars(&fft_output, visual_count)
                } else {
                    fft_output
                };

                // Update display buffers
                {
                    let Some(mut display) = self.display.try_lock() else {
                        return true; // Skip display update if lock contended
                    };
                    display.bars = output.clone();
                    display.dirty = true;
                }

                // Update peaks
                let cfg = self.config.read();
                let (peak_mode, peak_hold_time_ms, peak_fade_time_ms, peak_fall_speed) = (
                    cfg.bars.get_peak_mode_value(),
                    cfg.bars.peak_hold_time,
                    cfg.bars.peak_fade_time,
                    cfg.bars.peak_fall_speed,
                );
                drop(cfg);

                if peak_mode != 0 {
                    let Some(mut display) = self.display.try_lock() else {
                        return true; // Skip peak update if lock contended
                    };
                    let Some(mut peaks) = self.peaks.try_lock() else {
                        return true; // Skip peak update if lock contended
                    };

                    const PEAK_UPDATE_THRESHOLD: f64 = 0.001;
                    // Scale velocities by peak_fall_speed (5 = baseline)
                    let speed_scale = peak_fall_speed as f64 / 5.0;
                    let peak_falloff_multiplier = 1.0 + (0.05 * speed_scale);
                    let peak_constant_velocity = 0.02 * speed_scale;

                    let peak_hold_duration = Duration::from_millis(u64::from(peak_hold_time_ms));
                    let fade_rate_per_frame = if peak_fade_time_ms > 0 {
                        16.67 / f64::from(peak_fade_time_ms)
                    } else {
                        0.1
                    };

                    let chunk_duration = Duration::from_micros(16670);
                    let safe_len = visual_count.min(output.len()).min(display.peak_bars.len());

                    // Using explicit index loop: we need `i` to cross-reference output[], display.peak_bars[],
                    // peaks.hold_times[], peaks.velocities[], and display.peak_alphas[] simultaneously.
                    #[allow(clippy::needless_range_loop)]
                    for i in 0..safe_len {
                        if output[i] > display.peak_bars[i] + PEAK_UPDATE_THRESHOLD {
                            display.peak_bars[i] = output[i];
                            peaks.hold_times[i] = peak_hold_duration;
                            peaks.velocities[i] = PEAK_INITIAL_VELOCITY;
                            display.peak_alphas[i] = 1.0;
                        } else if peaks.hold_times[i] > Duration::ZERO {
                            peaks.hold_times[i] =
                                peaks.hold_times[i].saturating_sub(chunk_duration);
                        } else if display.peak_bars[i] > 0.0 || display.peak_alphas[i] > 0.0 {
                            match peak_mode {
                                1 => {
                                    // Fade mode
                                    display.peak_alphas[i] =
                                        (display.peak_alphas[i] - fade_rate_per_frame).max(0.0);
                                    if display.peak_alphas[i] <= 0.0 {
                                        display.peak_bars[i] = 0.0;
                                    }
                                }
                                2 => {
                                    // Fall mode
                                    let new_peak = display.peak_bars[i] - peak_constant_velocity;
                                    display.peak_bars[i] =
                                        if new_peak <= 0.0 { 0.0 } else { new_peak };
                                }
                                3 => {
                                    // Fall_accel mode
                                    let new_peak = display.peak_bars[i] - peaks.velocities[i];
                                    let new_velocity =
                                        peaks.velocities[i] * peak_falloff_multiplier;
                                    if new_peak <= 0.0 {
                                        display.peak_bars[i] = 0.0;
                                        peaks.velocities[i] = PEAK_INITIAL_VELOCITY;
                                    } else {
                                        display.peak_bars[i] = new_peak;
                                        peaks.velocities[i] = new_velocity;
                                    }
                                }
                                _ => {
                                    // Fall_fade mode: fall at constant speed + fade opacity
                                    let new_peak = display.peak_bars[i] - peak_constant_velocity;
                                    display.peak_alphas[i] =
                                        (display.peak_alphas[i] - fade_rate_per_frame).max(0.0);
                                    if new_peak <= 0.0 || display.peak_alphas[i] <= 0.0 {
                                        display.peak_bars[i] = 0.0;
                                        display.peak_alphas[i] = 0.0;
                                    } else {
                                        display.peak_bars[i] = new_peak;
                                    }
                                }
                            }
                        }

                        // Invariant: peak must never be below the current bar value.
                        // During decay (fade/fall/fall_accel), the bar can rise back up
                        // past the decaying peak. Clamp the peak up and reset its state.
                        if display.peak_bars[i] < output[i] {
                            display.peak_bars[i] = output[i];
                            peaks.hold_times[i] = peak_hold_duration;
                            peaks.velocities[i] = PEAK_INITIAL_VELOCITY;
                            display.peak_alphas[i] = 1.0;
                        }
                    }
                } else {
                    // peak_mode == 0 ("none"): clear any lingering peak data
                    if let Some(mut display) = self.display.try_lock() {
                        for peak in display.peak_bars.iter_mut() {
                            *peak = 0.0;
                        }
                        for alpha in display.peak_alphas.iter_mut() {
                            *alpha = 0.0;
                        }
                    }
                }

                // Shimmer flash effect
                self.update_flash_effect(&output, visual_count);

                return true;
            }
        }
        false
    }

    /// Update flash effect (extracted for clarity)
    fn update_flash_effect(&self, output: &[f64], bar_count: usize) {
        const FLASH_DECAY_RATE: f32 = 0.08;
        const FLASH_SPREAD_RADIUS: usize = 3;
        const FLASH_FALLOFF: f32 = 0.5;
        const SHIMMER_AMPLITUDE_MIN: f64 = 0.15;
        const SHIMMER_CHANCE_BASE: f64 = 0.08;
        const SHIMMER_CHANCE_SCALE: f64 = 0.25;
        const SPIKE_THRESHOLD: f64 = 0.12;

        let mut display = self.display.lock();
        let mut effects = self.effects.lock();

        if display.flash_intensities.len() != bar_count {
            display.flash_intensities = vec![0.0; bar_count];
            effects.prev_bars = vec![0.0; bar_count];
        }

        // Simple LCG random
        let mut rng_state = ((effects.elapsed_time * 12_345.679) as u32)
            .wrapping_mul(1103515245)
            .wrapping_add(12345);
        let mut next_rand = || -> f64 {
            rng_state = rng_state.wrapping_mul(1103515245).wrapping_add(12345);
            f64::from((rng_state >> 16) & 0x7FFF) / 32768.0
        };

        let mut new_flashes: Vec<(usize, f32)> = Vec::new();

        // Using explicit index loop: we need `i` to cross-reference output[], effects.prev_bars[],
        // and to use as center_idx for flash spreading calculations.
        #[allow(clippy::needless_range_loop)]
        for i in 0..bar_count.min(output.len()) {
            let current = output[i];
            let previous = effects.prev_bars[i];
            let increase = current - previous;

            if increase > SPIKE_THRESHOLD {
                let flash = (increase / 0.2).min(1.0) as f32;
                new_flashes.push((i, flash));
            } else if current > SHIMMER_AMPLITUDE_MIN {
                let shimmer_chance = SHIMMER_CHANCE_BASE + (current * SHIMMER_CHANCE_SCALE);
                if next_rand() < shimmer_chance {
                    let flash = (0.3 + current * 0.4).min(0.7) as f32;
                    new_flashes.push((i, flash));
                }
            }

            effects.prev_bars[i] = current;
        }

        // Apply flashes with neighbor spread
        for (center_idx, flash_intensity) in new_flashes {
            display.flash_intensities[center_idx] =
                display.flash_intensities[center_idx].max(flash_intensity);

            for dist in 1..=FLASH_SPREAD_RADIUS {
                let neighbor_intensity = flash_intensity * FLASH_FALLOFF.powi(dist as i32);

                if center_idx >= dist {
                    let left_idx = center_idx - dist;
                    display.flash_intensities[left_idx] =
                        display.flash_intensities[left_idx].max(neighbor_intensity);
                }

                let right_idx = center_idx + dist;
                if right_idx < bar_count {
                    display.flash_intensities[right_idx] =
                        display.flash_intensities[right_idx].max(neighbor_intensity);
                }
            }
        }

        // Decay all flashes
        for flash in display.flash_intensities.iter_mut() {
            *flash = (*flash - FLASH_DECAY_RATE).max(0.0);
        }

        // Update elapsed time
        effects.elapsed_time += 0.01667;
    }

    pub(crate) fn get_bars(&self) -> Vec<f64> {
        if self.pending_clear.load(Ordering::SeqCst)
            || self.rebuilding_after_clear.load(Ordering::SeqCst)
        {
            let display = self.display.lock();
            return vec![0.0; display.bars.len()];
        }
        self.display.lock().bars.clone()
    }

    pub(crate) fn get_peak_bars(&self) -> Vec<f64> {
        if self.pending_clear.load(Ordering::SeqCst)
            || self.rebuilding_after_clear.load(Ordering::SeqCst)
        {
            let display = self.display.lock();
            return vec![0.0; display.peak_bars.len()];
        }
        self.display.lock().peak_bars.clone()
    }

    pub(crate) fn get_peak_alphas(&self) -> Vec<f64> {
        self.display.lock().peak_alphas.clone()
    }

    pub(crate) fn get_flash_intensities(&self) -> Vec<f32> {
        self.display.lock().flash_intensities.clone()
    }

    /// Set the current visualization mode so tick() can skip CPU-side smoothing in lines mode.
    /// Lines mode performs its own Catmull-Rom smoothing in the GPU shader.
    pub(crate) fn set_lines_mode(&self, is_lines: bool) {
        self.is_lines_mode.store(is_lines, Ordering::Relaxed);
    }

    /// Apply config changes by signaling engine reinitialization on the FFT thread.
    ///
    /// The FFT thread picks up the reinit flag on its next tick() and rebuilds
    /// the SpectrumEngine with updated config values.
    pub(crate) fn apply_config(&self) {
        self.pending_engine_reinit.store(true, Ordering::SeqCst);
        debug!(
            " Config change queued for FFT thread reinit ({} visual bars)",
            self.visual_bar_count.load(Ordering::Relaxed)
        );
    }

    /// Reinitialize the spectrum engine with the current sample rate.
    fn reinit_engine_with_current_sample_rate(&self) {
        let sample_rate = *self.sample_rate.lock();
        let fft_count = self.fft_bar_count();

        if let Some(new_engine) = build_spectrum_engine(fft_count, sample_rate, &self.config) {
            *self.engine.lock() = Some(new_engine);
        } else {
            tracing::warn!(
                " [VIZ] Failed to reinit spectrum engine for sample rate change, keeping existing instance"
            );
        }
        // Clear sample buffer since engine state is reset
        self.sample_buffer.lock().clear();
        // Mark as rebuilding to allow buffer to fill before processing
        self.rebuilding_after_clear.store(true, Ordering::SeqCst);

        debug!(
            " Spectrum engine reinitialized for sample rate change: {}Hz, {} FFT bars ({} visual)",
            sample_rate,
            fft_count,
            self.visual_bar_count.load(Ordering::Relaxed)
        );
    }

    /// Decay peaks based on configured peak_mode
    pub(crate) fn decay_peaks(&self, delta_time: Duration) {
        let cfg = self.config.read();
        let (peak_mode, peak_fade_time_ms, peak_fall_speed) = (
            cfg.bars.get_peak_mode_value(),
            cfg.bars.peak_fade_time,
            cfg.bars.peak_fall_speed,
        );
        drop(cfg);

        if peak_mode == 0 {
            return;
        }

        // Scale velocities by peak_fall_speed (5 = baseline)
        let speed_scale = peak_fall_speed as f64 / 5.0;
        let peak_falloff_multiplier = 1.0 + (0.05 * speed_scale);
        let peak_constant_velocity = 0.02 * speed_scale;

        let fade_rate = if peak_fade_time_ms > 0 {
            delta_time.as_secs_f64() * 1000.0 / f64::from(peak_fade_time_ms)
        } else {
            0.1
        };

        let mut display = self.display.lock();
        let mut peaks = self.peaks.lock();

        for i in 0..display.peak_bars.len() {
            if peaks.hold_times[i] > Duration::ZERO {
                peaks.hold_times[i] = peaks.hold_times[i].saturating_sub(delta_time);
            } else if display.peak_bars[i] > 0.0 || display.peak_alphas[i] > 0.0 {
                match peak_mode {
                    1 => {
                        display.peak_alphas[i] = (display.peak_alphas[i] - fade_rate).max(0.0);
                        if display.peak_alphas[i] <= 0.0 {
                            display.peak_bars[i] = 0.0;
                        }
                    }
                    2 => {
                        let new_peak = display.peak_bars[i] - peak_constant_velocity;
                        display.peak_bars[i] = if new_peak <= 0.0 { 0.0 } else { new_peak };
                    }
                    3 => {
                        let new_peak = display.peak_bars[i] - peaks.velocities[i];
                        let new_velocity = peaks.velocities[i] * peak_falloff_multiplier;
                        if new_peak <= 0.0 {
                            display.peak_bars[i] = 0.0;
                            peaks.velocities[i] = PEAK_INITIAL_VELOCITY;
                        } else {
                            display.peak_bars[i] = new_peak;
                            peaks.velocities[i] = new_velocity;
                        }
                    }
                    _ => {
                        // Fall_fade mode: fall at constant speed + fade opacity
                        let new_peak = display.peak_bars[i] - peak_constant_velocity;
                        display.peak_alphas[i] = (display.peak_alphas[i] - fade_rate).max(0.0);
                        if new_peak <= 0.0 || display.peak_alphas[i] <= 0.0 {
                            display.peak_bars[i] = 0.0;
                            display.peak_alphas[i] = 0.0;
                        } else {
                            display.peak_bars[i] = new_peak;
                        }
                    }
                }
            }
        }
    }

    /// Queue a resize request with debouncing
    pub(crate) fn resize(&self, new_bar_count: usize) {
        self.pending_bar_count
            .store(new_bar_count, Ordering::SeqCst);
        *self.last_resize_request.lock() = Some(Instant::now());
    }

    /// Apply pending resize if debounce period has passed
    fn apply_pending_resize(&self) -> bool {
        const RESIZE_DEBOUNCE_MS: u64 = 100;

        let pending = self.pending_bar_count.load(Ordering::SeqCst);
        if pending == 0 {
            return false;
        }

        let current_count = self.display.lock().bars.len();
        if pending == current_count {
            self.pending_bar_count.store(0, Ordering::SeqCst);
            *self.last_resize_request.lock() = None;
            return false;
        }

        let last_request = *self.last_resize_request.lock();
        if let Some(timestamp) = last_request {
            if timestamp.elapsed().as_millis() < u128::from(RESIZE_DEBOUNCE_MS) {
                return false;
            }
        } else {
            return false;
        }

        let visual_count = pending;
        let sample_rate = *self.sample_rate.lock();

        // Store the desired visual count
        self.visual_bar_count.store(visual_count, Ordering::SeqCst);

        // Engine gets the FFT-limited count
        let fft_limit = max_bars_for_sample_rate(sample_rate);
        let fft_count = visual_count.min(fft_limit);

        if let Some(new_engine) = build_spectrum_engine(fft_count, sample_rate, &self.config) {
            *self.engine.lock() = Some(new_engine);
        } else {
            tracing::warn!(
                " [VIZ] Failed to reinit spectrum engine for resize, keeping existing instance"
            );
        }

        // Display buffers are sized to the visual count (interpolation bridges the gap)
        self.display.lock().resize(visual_count);
        self.peaks.lock().resize(visual_count);
        self.effects.lock().resize(visual_count);
        self.processing.lock().resize(visual_count);

        self.pending_bar_count.store(0, Ordering::SeqCst);
        *self.last_resize_request.lock() = None;

        true
    }

    pub(crate) fn is_dirty(&self) -> bool {
        self.display.lock().dirty
    }

    pub(crate) fn clear_dirty(&self) {
        self.display.lock().dirty = false;
    }

    pub(crate) fn bar_count(&self) -> usize {
        self.display.lock().bars.len()
    }

    /// Get the number of bars the spectrum engine is initialized with (FFT-limited).
    /// This may be less than the visual bar count; interpolation bridges the gap.
    fn fft_bar_count(&self) -> usize {
        let visual = self.visual_bar_count.load(Ordering::Relaxed);
        let sample_rate = *self.sample_rate.lock();
        let fft_limit = max_bars_for_sample_rate(sample_rate);
        visual.min(fft_limit)
    }

    pub(crate) fn target_bar_count(&self) -> usize {
        let pending = self.pending_bar_count.load(Ordering::Relaxed);
        if pending > 0 {
            pending
        } else {
            self.display.lock().bars.len()
        }
    }

    /// Clear the sample buffer for track changes
    pub(crate) fn clear_sample_buffer(&self) {
        self.pending_clear.store(true, Ordering::SeqCst);
    }

    /// Apply pending buffer clear
    fn apply_pending_clear(&self) {
        let bar_count = self.bar_count();

        self.sample_buffer.lock().clear();
        self.display.lock().clear();
        self.effects.lock().clear(bar_count);
        self.processing.lock().clear(bar_count);

        self.rebuilding_after_clear.store(true, Ordering::SeqCst);
    }

    /// Reset the visualizer state completely.
    ///
    /// Sets atomic flags so the FFT thread handles the actual engine reinitialization
    /// on its next tick().
    pub(crate) fn reset(&self) {
        // Signal buffer clear + engine reinit on the FFT thread
        self.pending_clear.store(true, Ordering::SeqCst);
        self.pending_engine_reinit.store(true, Ordering::SeqCst);
    }
}

/// Monstercat filter (ported from cava.c, with Catmull-Rom post-smoothing)
///
/// Exponential decay spreading creates sharp triangular peaks. A light
/// Catmull-Rom pass afterward smooths the kinks where overlapping decays
/// meet without flattening the distinctive monstercat aesthetic.
#[allow(clippy::needless_range_loop)]
fn monstercat_filter(bars: &mut [f64], monstercat: f64) {
    let number_of_bars = bars.len();
    if number_of_bars == 0 {
        return;
    }

    for z in 0..number_of_bars {
        let bar_value = bars[z];

        for m_y in (0..z).rev() {
            let de = (z - m_y) as f64;
            let spread_value = bar_value / (monstercat * 1.5_f64).powf(de);
            if spread_value > bars[m_y] {
                bars[m_y] = spread_value;
            }
        }

        for m_y in (z + 1)..number_of_bars {
            let de = (m_y - z) as f64;
            let spread_value = bar_value / (monstercat * 1.5_f64).powf(de);
            if spread_value > bars[m_y] {
                bars[m_y] = spread_value;
            }
        }
    }

    // Light Catmull-Rom post-smoothing to clean up kinks between overlapping decays
    waves_filter(bars, 2);
}

/// Catmull-Rom spline interpolation for a single dimension.
///
/// Given four control values p0..p3 and parameter t in [0, 1],
/// returns the interpolated value at t between p1 and p2.
fn catmull_rom_1d(p0: f64, p1: f64, p2: f64, p3: f64, t: f64) -> f64 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}

/// Waves filter — Catmull-Rom spline smoothing for bar values.
///
/// Subsamples the raw bar values into sparse control points (every ~4th bar),
/// then interpolates a smooth C¹-continuous curve back to the full bar count.
/// This produces naturally smooth rolling hills between frequency peaks.
///
/// The spline output is clamped to [0, 1] to prevent overshoot artifacts
/// at sharp peaks (the Catmull-Rom basis can overshoot by design).
fn waves_filter(bars: &mut [f64], step: usize) {
    let n = bars.len();
    if n < 4 {
        return;
    }

    // Subsample: take every `step`th bar as a control point.
    // Higher step = smoother result (fewer control points).
    let step = step.clamp(2, 16);

    // Build sparse control points by direct sampling at each step position.
    // Using the actual bar value (not max within a window) preserves the
    // original amplitude envelope — max-based windowing inflated values,
    // acting as an unwanted multiplier on bar heights.
    let mut control_points: Vec<f64> = Vec::new();
    let mut i = 0;
    while i < n {
        control_points.push(bars[i]);
        i += step;
    }
    // Ensure the last bar is always a control point for accurate endpoint
    if n > 0 && !(n - 1).is_multiple_of(step) {
        control_points.push(bars[n - 1]);
    }

    let cp_count = control_points.len();
    if cp_count < 2 {
        return;
    }

    // Interpolate from sparse control points back to full bar count
    let last_cp = (cp_count - 1) as f64;
    for (i, bar) in bars.iter_mut().enumerate() {
        // Map output index to fractional position along control points
        let pos = i as f64 / (n - 1).max(1) as f64 * last_cp;
        let segment = (pos.floor() as usize).min(cp_count - 2);
        let t = pos - segment as f64;

        // Get four control points, clamping at boundaries
        let p0 = control_points[segment.saturating_sub(1)];
        let p1 = control_points[segment];
        let p2 = control_points[(segment + 1).min(cp_count - 1)];
        let p3 = control_points[(segment + 2).min(cp_count - 1)];

        *bar = catmull_rom_1d(p0, p1, p2, p3, t).clamp(0.0, 1.0);
    }
}

impl std::fmt::Debug for VisualizerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VisualizerState")
            .field("bar_count", &self.bar_count())
            .field("dirty", &self.is_dirty())
            .finish()
    }
}
