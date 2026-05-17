//! Audio subsystem — rodio/cpal-based playback engine
//!
//! Cross-platform audio engine with Symphonia HTTP streaming decoder, gapless transitions,
//! lock-free volume control, and visualizer sample emission.
//! Includes a separate polyphonic SFX engine for UI sounds and a pure-Rust
//! spectrum analyzer engine (RustFFT) for the visualizer.

use std::sync::atomic::{AtomicU32, Ordering};

pub mod buffer;
pub mod decoder;
pub mod engine;
pub mod eq;
pub mod format;
mod generation;
pub mod normalization;

#[cfg(target_os = "linux")]
pub mod pipewire_output;
mod range_http_reader;
pub mod renderer;
pub mod rodio_output;
pub mod sfx_engine;
pub mod spectrum;

pub mod streaming_source;
pub mod symphonia_registry;

pub use buffer::AudioBuffer;
pub use decoder::AudioDecoder;
pub use eq::{EqProcessor, EqState};
pub use format::{AudioFormat, SampleFormat};
pub(crate) use generation::{DecodeLoopHandle, SourceGeneration};
pub use normalization::{NormalizationConfig, NormalizationContext, resolve_normalization};
pub use renderer::AudioRenderer;
pub use rodio_output::{ActiveStream, RING_BUFFER_CAPACITY, RodioOutput};
pub use sfx_engine::{SfxEngine, SfxType};
pub use spectrum::{SpectrumEngine, SpectrumError};
pub use streaming_source::VisualizerCallback;

/// Lock-free `f32` ↔ `AtomicU32` shim using `Relaxed` ordering. Used for
/// volume-style shared atomics where the operative discipline is "the next
/// reader is fine with the latest value but doesn't need to synchronize
/// with any other write." Do NOT change the ordering — `Acquire`/`Release`
/// would over-constrain the audio hot path and `SeqCst` is unnecessary
/// given there are no co-ordinating reads.
pub(super) fn store_f32(atomic: &AtomicU32, value: f32) {
    atomic.store(value.to_bits(), Ordering::Relaxed);
}

pub(super) fn load_f32(atomic: &AtomicU32) -> f32 {
    f32::from_bits(atomic.load(Ordering::Relaxed))
}

/// Shared HTTP User-Agent for all outbound Navidrome/Subsonic requests.
/// Centralizing this string means a future User-Agent filter on the Navidrome
/// side affects all three HTTP clients (decoder HEAD probe, decoder infinite-
/// stream, range_http_reader chunk fetch) consistently.
pub(super) const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU32;

    use super::{RING_BUFFER_CAPACITY, USER_AGENT, load_f32, store_f32};

    #[test]
    fn store_load_f32_roundtrip() {
        let a = AtomicU32::new(0);

        store_f32(&a, 1.23_f32);
        let loaded = load_f32(&a);
        assert!(
            (loaded - 1.23_f32).abs() < 1e-6,
            "1.23 round-trip mismatched: got {loaded}"
        );

        store_f32(&a, 0.0_f32);
        assert_eq!(load_f32(&a), 0.0_f32, "0.0 round-trip mismatched");

        store_f32(&a, f32::INFINITY);
        assert_eq!(
            load_f32(&a),
            f32::INFINITY,
            "INFINITY round-trip mismatched"
        );

        store_f32(&a, f32::NEG_INFINITY);
        assert_eq!(
            load_f32(&a),
            f32::NEG_INFINITY,
            "NEG_INFINITY round-trip mismatched"
        );

        store_f32(&a, f32::NAN);
        assert!(
            load_f32(&a).is_nan(),
            "NaN round-trip must remain NaN (it compares unequal to itself)"
        );
    }

    #[test]
    fn ring_buffer_capacity_reexport_matches_rodio_output() {
        assert_eq!(
            RING_BUFFER_CAPACITY,
            crate::audio::rodio_output::RING_BUFFER_CAPACITY,
            "audio::RING_BUFFER_CAPACITY re-export must point at the same const \
             as rodio_output::RING_BUFFER_CAPACITY"
        );
    }

    #[test]
    fn user_agent_is_chrome_desktop() {
        assert!(
            USER_AGENT.starts_with("Mozilla/5.0"),
            "USER_AGENT should advertise a browser-style header so Navidrome's \
             optional User-Agent filter treats us like a desktop client"
        );
        assert!(
            USER_AGENT.contains("Chrome"),
            "USER_AGENT should explicitly mention Chrome — Navidrome operators \
             commonly allowlist Chromium-family browsers"
        );
    }
}
