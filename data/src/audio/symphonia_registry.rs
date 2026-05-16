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

use std::sync::LazyLock;

use symphonia::core::codecs::CodecRegistry;
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

#[cfg(test)]
mod tests {
    use symphonia::core::codecs::{CODEC_TYPE_FLAC, CODEC_TYPE_MP3, CODEC_TYPE_OPUS};

    use super::*;

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
}
