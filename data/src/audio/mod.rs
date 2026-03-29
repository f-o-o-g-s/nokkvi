//! Audio subsystem — rodio/cpal-based playback engine
//!
//! Cross-platform audio engine with Symphonia HTTP streaming decoder, gapless transitions,
//! lock-free volume control, and visualizer sample emission.
//! Includes a separate polyphonic SFX engine for UI sounds and a pure-Rust
//! spectrum analyzer engine (RustFFT) for the visualizer.

pub mod buffer;
pub mod decoder;
pub mod engine;
pub mod format;

#[cfg(target_os = "linux")]
pub mod pipewire_output;
mod range_http_reader;
pub mod renderer;
pub mod rodio_output;
pub mod sfx_engine;
pub mod spectrum;

pub mod streaming_source;

pub use buffer::AudioBuffer;
pub use decoder::AudioDecoder;
pub use format::{AudioFormat, SampleFormat};
pub use renderer::AudioRenderer;
pub use rodio_output::{ActiveStream, RodioOutput};
pub use sfx_engine::{SfxEngine, SfxType};
pub use spectrum::{SpectrumEngine, SpectrumError};
pub use streaming_source::VisualizerCallback;
