//! Nokkvi's Symphonia codec registry.
//!
//! Symphonia 0.5 ships no Opus decoder (upstream issue pdeljanov/Symphonia#8,
//! open since 2020 with no ETA). We register the built-in feature-gated codecs
//! and bolt on `symphonia-adapter-libopus` so `.opus` files decode.
//!
//! Every audio decoder in this crate must obtain its `CodecRegistry` through
//! [`codecs()`] rather than `symphonia::default::get_codecs()` — otherwise Opus
//! tracks fall back to the default registry and fail with
//! `unsupported feature: core (codec):unsupported codec`.
//!
//! # Removal plan
//!
//! When upstream Symphonia lands native Opus (track pdeljanov/Symphonia#8):
//! 1. Drop the `symphonia-adapter-libopus` dep in `data/Cargo.toml`.
//! 2. Add `"opus"` to the symphonia features list there.
//! 3. Delete this module and switch every `symphonia_registry::codecs()` call
//!    back to `symphonia::default::get_codecs()`.
//! 4. Drop the `cmake` mention from CI apt-install + Arch pacman lines in
//!    `README.md` / `CLAUDE.md`.

use std::sync::LazyLock;

use anyhow::{Context, Result};
use symphonia::core::{
    codecs::{CodecRegistry, Decoder, DecoderOptions},
    formats::{FormatOptions, FormatReader},
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};
use symphonia_adapter_libopus::OpusDecoder;

static CODECS: LazyLock<CodecRegistry> = LazyLock::new(|| {
    let mut registry = CodecRegistry::new();
    symphonia::default::register_enabled_codecs(&mut registry);
    registry.register_all::<OpusDecoder>();
    registry
});

/// Process-wide codec registry: Symphonia defaults plus the libopus adapter.
pub fn codecs() -> &'static CodecRegistry {
    &CODECS
}

/// Tuple produced by [`probe_and_make_decoder`]: the boxed Symphonia
/// [`FormatReader`], the boxed [`Decoder`], and the selected track's id.
pub type ProbedDecoder = (Box<dyn FormatReader>, Box<dyn Decoder>, u32);

/// Probe a `MediaSourceStream`, select the first decodable track, and construct
/// its decoder via the project-wide codec registry.
///
/// The `enable_gapless` flag is load-bearing for OGG ICEcast chained-metadata
/// handling — `AudioDecoder::open_input` passes `true` for the primary init,
/// the `SymphoniaError::ResetRequired` reprobe inside `read_buffer` passes
/// `false`, and `SfxEngine::decode_wav_stream` uses the default (`false`).
/// The helper preserves each caller's value rather than collapsing them.
///
/// Returns the boxed [`FormatReader`], the boxed [`Decoder`], and the selected
/// track's id (for downstream packet filtering / `format_reader.tracks()`
/// re-lookup of codec parameters).
///
/// # Errors
///
/// - Returns the underlying Symphonia error (wrapped via `anyhow::Context`)
///   when the probe fails to identify a container format.
/// - Returns an error when the probed format contains no tracks with a
///   non-NULL codec (matches the pre-extraction check in
///   `AudioDecoder::open_input`).
/// - Returns the underlying Symphonia error when the codec registry cannot
///   construct a decoder for the selected track's codec parameters.
pub fn probe_and_make_decoder(
    mss: MediaSourceStream,
    hint: &Hint,
    enable_gapless: bool,
) -> Result<ProbedDecoder> {
    use symphonia::core::codecs::CODEC_TYPE_NULL;

    let format_opts = FormatOptions {
        enable_gapless,
        ..Default::default()
    };
    let metadata_opts = MetadataOptions::default();
    let decoder_opts = DecoderOptions::default();

    let probed = symphonia::default::get_probe()
        .format(hint, mss, &format_opts, &metadata_opts)
        .context("Failed to probe media format")?;

    let format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .context("No supported audio tracks found")?;
    let track_id = track.id;

    let decoder = codecs()
        .make(&track.codec_params, &decoder_opts)
        .context("Failed to create decoder")?;

    Ok((format, decoder, track_id))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use symphonia::core::{
        codecs::{CODEC_TYPE_FLAC, CODEC_TYPE_MP3, CODEC_TYPE_OPUS},
        io::MediaSourceStream,
        probe::Hint,
    };

    use super::*;

    /// A real, validly-encoded WAV ships in the repo for the SFX engine. Reusing
    /// it as the probe input avoids hand-rolling a synthetic WAV header.
    const TEST_WAV: &[u8] = include_bytes!("../../../assets/sound_effects/enter.wav");

    fn wav_stream() -> MediaSourceStream {
        MediaSourceStream::new(Box::new(Cursor::new(TEST_WAV.to_vec())), Default::default())
    }

    fn wav_hint() -> Hint {
        let mut hint = Hint::new();
        hint.with_extension("wav");
        hint
    }

    #[test]
    fn opus_decoder_is_registered() {
        assert!(
            codecs().get_codec(CODEC_TYPE_OPUS).is_some(),
            "OpusDecoder must be in the registry — otherwise .opus files fail to decode (GH#3)"
        );
    }

    #[test]
    fn default_codecs_are_still_registered() {
        assert!(codecs().get_codec(CODEC_TYPE_MP3).is_some());
        assert!(codecs().get_codec(CODEC_TYPE_FLAC).is_some());
    }

    /// Smoke test: the extracted helper produces a usable
    /// `(FormatReader, Decoder, track_id)` triple from a real WAV stream.
    /// This is the path every site (primary init, ResetRequired reprobe, SFX
    /// decode) takes after Lane 2.
    #[test]
    fn probe_and_make_decoder_returns_decoder_for_synthetic_wav() {
        let (format, _decoder, track_id) = probe_and_make_decoder(wav_stream(), &wav_hint(), false)
            .expect("probing a known-good WAV should succeed");

        let track = format
            .tracks()
            .iter()
            .find(|t| t.id == track_id)
            .expect("returned track_id must reference a real track in the format reader");
        assert!(
            track.codec_params.sample_rate.is_some_and(|sr| sr > 0),
            "WAV track should expose a positive sample rate"
        );
    }

    /// The `enable_gapless` parameter is load-bearing — Site 1 passes `true`,
    /// Sites 2 and 3 pass `false`. The helper must accept both values without
    /// surfacing a probe error for a vanilla WAV input. (Symphonia hides the
    /// gapless flag inside the format reader's private state, so the cleanest
    /// observable assertion is that both polarities probe successfully.)
    #[test]
    fn probe_and_make_decoder_accepts_both_gapless_polarities() {
        let with_gapless = probe_and_make_decoder(wav_stream(), &wav_hint(), true);
        assert!(
            with_gapless.is_ok(),
            "enable_gapless=true (primary init path) must probe WAV input cleanly"
        );

        let without_gapless = probe_and_make_decoder(wav_stream(), &wav_hint(), false);
        assert!(
            without_gapless.is_ok(),
            "enable_gapless=false (ResetRequired reprobe + SFX decode path) must \
             probe WAV input cleanly"
        );
    }

    /// Empty input must surface a probe error rather than a panic — the
    /// production sites all use `?`-propagation, so a panic here would mean
    /// the helper is silently `unwrap`ing somewhere it shouldn't.
    #[test]
    fn probe_and_make_decoder_returns_err_on_empty_input() {
        let empty =
            MediaSourceStream::new(Box::new(Cursor::new(Vec::<u8>::new())), Default::default());
        assert!(
            probe_and_make_decoder(empty, &Hint::new(), false).is_err(),
            "probing an empty stream with no hint must return Err, not panic"
        );
    }
}
