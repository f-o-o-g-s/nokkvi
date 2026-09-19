//! Nokkvi's Symphonia codec registry.
//!
//! Symphonia 0.6 ships no Opus decoder (upstream issue pdeljanov/Symphonia#8,
//! open since 2020 with no ETA). We register the built-in feature-gated codecs
//! and bolt on `symphonia-adapter-libopus` so `.opus` files decode.
//!
//! Every audio decoder in this crate must obtain its `CodecRegistry` through
//! [`codecs()`] rather than `symphonia::default::get_codecs()` — otherwise Opus
//! tracks fall back to the default registry and fail with
//! `unsupported feature: core (codec): unsupported audio codec`.
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
    codecs::{
        CodecParameters,
        audio::{AudioDecoder, AudioDecoderOptions},
        registry::CodecRegistry,
    },
    formats::{FormatOptions, FormatReader, TrackType, probe::Hint},
    io::MediaSourceStream,
    meta::MetadataOptions,
};
use symphonia_adapter_libopus::OpusDecoder;

static CODECS: LazyLock<CodecRegistry> = LazyLock::new(|| {
    let mut registry = CodecRegistry::new();
    symphonia::default::register_enabled_codecs(&mut registry);
    registry.register_audio_decoder::<OpusDecoder>();
    registry
});

/// Process-wide codec registry: Symphonia defaults plus the libopus adapter.
pub fn codecs() -> &'static CodecRegistry {
    &CODECS
}

/// Tuple produced by [`probe_and_make_decoder`]: the boxed Symphonia
/// [`FormatReader`] (living as long as its media source, `'s`), the boxed
/// [`AudioDecoder`], and the selected track's id.
pub type ProbedDecoder<'s> = (Box<dyn FormatReader + 's>, Box<dyn AudioDecoder>, u32);

/// Probe a `MediaSourceStream`, select the first decodable audio track, and
/// construct its decoder via the project-wide codec registry.
///
/// `gapless` sets `AudioDecoderOptions::gapless`: when true the decoder drops
/// the encoder delay and padding frames the container declares (LAME/Xing tag,
/// Ogg pre-skip), so consecutive album tracks join without a gap. Since
/// Symphonia 0.6 the decoder owns this trim, not the caller.
/// `AudioDecoder::open_input` passes `true`; `SfxEngine::decode_wav_stream`
/// passes `false` (WAV declares no delay or padding, so the flag is inert
/// there).
///
/// Returns the boxed [`FormatReader`], the boxed [`AudioDecoder`], and the
/// selected track's id (for downstream packet filtering / `format.tracks()`
/// re-lookup of the track's timing and codec parameters).
///
/// # Errors
///
/// - Returns the underlying Symphonia error (wrapped via `anyhow::Context`)
///   when the probe fails to identify a container format.
/// - Returns an error when the probed format has no audio track with a known
///   codec, or when the codec registry cannot construct a decoder for it (see
///   [`make_track_decoder`]).
pub fn probe_and_make_decoder<'s>(
    mss: MediaSourceStream<'s>,
    hint: &Hint,
    gapless: bool,
) -> Result<ProbedDecoder<'s>> {
    let format = symphonia::default::get_probe()
        .probe(
            hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .context("Failed to probe media format")?;

    let (decoder, track_id) = make_track_decoder(format.as_ref(), gapless)?;

    Ok((format, decoder, track_id))
}

/// Select the first audio track with a known codec on an already-open format
/// reader and build its decoder via [`codecs()`].
///
/// Shared by [`probe_and_make_decoder`] and the `ResetRequired` path in
/// `AudioDecoder::read_buffer`: since Symphonia 0.6 a reader that signals
/// `ResetRequired` (chained Ogg radio streams) has already rebuilt its track
/// list in place, so the caller re-selects a track and rebuilds the decoder
/// on the same reader instead of re-probing the stream.
///
/// # Errors
///
/// - Returns an error when the reader has no audio track with a known codec.
/// - Returns the underlying Symphonia error when the codec registry cannot
///   construct a decoder for the selected track's codec parameters.
pub fn make_track_decoder(
    format: &dyn FormatReader,
    gapless: bool,
) -> Result<(Box<dyn AudioDecoder>, u32)> {
    let track = format
        .first_track_known_codec(TrackType::Audio)
        .context("No supported audio tracks found")?;
    let params = track
        .codec_params
        .as_ref()
        .and_then(CodecParameters::audio)
        .context("Selected audio track carries no audio codec parameters")?;

    let decoder = codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default().gapless(gapless))
        .context("Failed to create decoder")?;

    Ok((decoder, track.id))
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Cursor, Read, Seek, SeekFrom},
        sync::{Arc, Mutex},
    };

    use symphonia::core::{
        codecs::audio::well_known::{CODEC_ID_FLAC, CODEC_ID_MP3, CODEC_ID_OPUS},
        formats::Track,
        io::MediaSource,
    };

    use super::*;

    /// A real, validly-encoded WAV ships in the repo for the SFX engine. Reusing
    /// it as the probe input avoids hand-rolling a synthetic WAV header.
    const TEST_WAV: &[u8] = include_bytes!("../../../assets/sound_effects/enter.wav");

    /// CRC-protected LAME VBR MP3 (1 s digital silence + 1 s noise,
    /// `lame -p -V 2`) whose Xing tag declares 78 MPEG frames. Symphonia 0.5
    /// rejected that tag (pdeljanov/Symphonia#516); see
    /// `symphonia_516_xing_tag_on_crc_protected_mp3_is_honored`.
    const XING_CRC_MP3: &[u8] = include_bytes!("../../testdata/xing_crc_protected.mp3");

    fn wav_stream() -> MediaSourceStream<'static> {
        MediaSourceStream::new(Box::new(Cursor::new(TEST_WAV.to_vec())), Default::default())
    }

    fn wav_hint() -> Hint {
        let mut hint = Hint::new();
        hint.with_extension("wav");
        hint
    }

    fn mp3_hint() -> Hint {
        let mut hint = Hint::new();
        hint.with_extension("mp3");
        hint
    }

    fn selected_track(format: &dyn FormatReader, track_id: u32) -> &Track {
        format
            .tracks()
            .iter()
            .find(|t| t.id == track_id)
            .expect("returned track_id must reference a real track in the format reader")
    }

    #[test]
    fn opus_decoder_is_registered() {
        assert!(
            codecs().get_audio_decoder(CODEC_ID_OPUS).is_some(),
            "OpusDecoder must be in the registry — otherwise .opus files fail to decode (GH#3)"
        );
    }

    #[test]
    fn default_codecs_are_still_registered() {
        assert!(codecs().get_audio_decoder(CODEC_ID_MP3).is_some());
        assert!(codecs().get_audio_decoder(CODEC_ID_FLAC).is_some());
    }

    /// Smoke test: the helper produces a usable `(FormatReader, AudioDecoder,
    /// track_id)` triple from a real WAV stream. This is the path the primary
    /// init and the SFX decode both take.
    #[test]
    fn probe_and_make_decoder_returns_decoder_for_synthetic_wav() {
        let (format, _decoder, track_id) = probe_and_make_decoder(wav_stream(), &wav_hint(), false)
            .expect("probing a known-good WAV should succeed");

        let sample_rate = selected_track(format.as_ref(), track_id)
            .codec_params
            .as_ref()
            .and_then(CodecParameters::audio)
            .and_then(|params| params.sample_rate);
        assert!(
            sample_rate.is_some_and(|sr| sr > 0),
            "WAV track should expose a positive sample rate"
        );
    }

    /// The `gapless` parameter differs per caller — the primary init passes
    /// `true`, the SFX decode `false`. The helper must accept both values
    /// without surfacing a probe error for a vanilla WAV input. (The flag
    /// lives inside the decoder's private state, so the cleanest observable
    /// assertion is that both polarities probe successfully.)
    #[test]
    fn probe_and_make_decoder_accepts_both_gapless_polarities() {
        let with_gapless = probe_and_make_decoder(wav_stream(), &wav_hint(), true);
        assert!(
            with_gapless.is_ok(),
            "gapless=true (primary init path) must probe WAV input cleanly"
        );

        let without_gapless = probe_and_make_decoder(wav_stream(), &wav_hint(), false);
        assert!(
            without_gapless.is_ok(),
            "gapless=false (SFX decode path) must probe WAV input cleanly"
        );
    }

    /// Regression guard for upstream pdeljanov/Symphonia#516, fixed in
    /// Symphonia 0.6.1: the Xing tag of a CRC-protected MP3 frame used to be
    /// rejected (the zero-side-info scan read the 16-bit frame CRC as side
    /// info), and the probe fell back to a 17-frame bitrate extrapolation that
    /// put this 2 s fixture at ~4 s.
    ///
    /// The Xing tag declares 78 frames × 1152 = 89,856 samples (2.038 s at
    /// 44.1 kHz); minus the LAME encoder delay and padding the track lasts
    /// 2.000 s. nokkvi's `sanitize_probed_duration` + `seek_scale` in
    /// `decoder.rs` stay as defense in depth — MP3s with no Xing tag at all
    /// still get the extrapolated estimate — but a failure here means the
    /// pinned Symphonia regressed on the root fix.
    #[test]
    fn symphonia_516_xing_tag_on_crc_protected_mp3_is_honored() {
        let mss = MediaSourceStream::new(
            Box::new(Cursor::new(XING_CRC_MP3.to_vec())),
            Default::default(),
        );

        let (format, _decoder, track_id) = probe_and_make_decoder(mss, &mp3_hint(), true)
            .expect("the CRC-protected MP3 fixture must probe successfully");
        let track = selected_track(format.as_ref(), track_id);
        let probed_ms = track
            .time_base
            .zip(track.duration)
            .and_then(|(time_base, duration)| time_base.calc_duration(duration))
            .map(|time| time.as_millis())
            .expect("an MP3 probe with known byte length must produce a duration");

        assert!(
            (1_900..=2_100).contains(&probed_ms),
            "Symphonia#516 regressed: the CRC-protected MP3 fixture probed at \
             {probed_ms} ms instead of ~2000 ms from its Xing tag"
        );
    }

    /// Wraps a seekable in-memory source and records the byte position of
    /// every seek, so a test can see where the probe looked.
    struct SeekRecorder {
        inner: Cursor<Vec<u8>>,
        seeks: Arc<Mutex<Vec<u64>>>,
    }

    impl Read for SeekRecorder {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.inner.read(buf)
        }
    }

    impl Seek for SeekRecorder {
        fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
            let landed = self.inner.seek(pos)?;
            self.seeks.lock().expect("seek log poisoned").push(landed);
            Ok(landed)
        }
    }

    impl MediaSource for SeekRecorder {
        fn is_seekable(&self) -> bool {
            true
        }

        fn byte_len(&self) -> Option<u64> {
            Some(self.inner.get_ref().len() as u64)
        }
    }

    /// Pins the `default-features = false` feature list in `data/Cargo.toml`:
    /// with Symphonia's default `ape` / `id3v1` metadata readers registered,
    /// every probe of a seekable, known-length source first seeks to the END
    /// of the file to look for trailing tags. On `RangeHttpReader` that seek
    /// is a blocking range request for the last 256 KB chunk before playback
    /// can start. If this fails, a feature change re-enabled those readers.
    #[test]
    fn probe_never_seeks_to_trailing_metadata() {
        let seeks = Arc::new(Mutex::new(Vec::new()));
        let source = SeekRecorder {
            inner: Cursor::new(XING_CRC_MP3.to_vec()),
            seeks: Arc::clone(&seeks),
        };
        let mss = MediaSourceStream::new(Box::new(source), Default::default());

        probe_and_make_decoder(mss, &mp3_hint(), true)
            .expect("the MP3 fixture must probe successfully");

        // The trailing readers anchor at most 32 + 128 bytes before EOF.
        let tail_start = XING_CRC_MP3.len() as u64 - 256;
        let seeks = seeks.lock().expect("seek log poisoned");
        assert!(
            seeks.iter().all(|&pos| pos < tail_start),
            "the probe seeked into the last 256 bytes ({seeks:?} of {} bytes) — \
             a trailing-metadata reader is registered again",
            XING_CRC_MP3.len()
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
