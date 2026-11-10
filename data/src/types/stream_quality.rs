//! What a queue song's stream request asks the server for under the
//! Streaming Quality setting (Settings → Playback).
//!
//! [`stream_params`] decides per song: no params (the file as it is) or a
//! transcode to the chosen format at the chosen cap. Navidrome reads them as
//! the classic `/rest/stream` `format` + `maxBitRate` pair, which
//! `utils::artwork_url::build_stream_url` appends.

use std::sync::atomic::{AtomicU32, Ordering};

use crate::types::{
    player_settings::{LivePlayerSettings, StreamFormat, StreamMaxBitrate},
    song::Song,
};

/// The Streaming Quality setting, as the stream URL builder reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StreamQuality {
    pub format: StreamFormat,
    pub max_bitrate: StreamMaxBitrate,
}

impl StreamQuality {
    /// The setting as `settings` holds it.
    pub fn of(settings: &LivePlayerSettings) -> Self {
        Self {
            format: settings.stream_format,
            max_bitrate: settings.stream_max_bitrate,
        }
    }
}

/// The format a transcode is asked for. There is no "raw": Original sends
/// no params at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscodeFormat {
    Opus,
    Mp3,
}

impl TranscodeFormat {
    /// The value of the stream URL's `format` param.
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Opus => "opus",
            Self::Mp3 => "mp3",
        }
    }
}

/// A transcode request: the format and the cap always travel together
/// (Navidrome turns a cap alone into Opus whatever was chosen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamParams {
    pub format: TranscodeFormat,
    pub max_bitrate_kbps: u32,
}

/// File suffixes of lossless audio nokkvi decodes itself. Each one is far
/// above any cap, so it is always transcoded when a format is chosen.
const LOSSLESS_SUFFIXES: &[&str] = &["flac", "wav", "wave", "aif", "aiff", "aifc"];

/// File suffixes nokkvi decodes, from the Symphonia features enabled in
/// `data/Cargo.toml` (adpcm, flac, mkv, ogg, pcm, vorbis, wav, mp3, aac,
/// isomp4, alac, aiff) plus the libopus adapter. Anything else (APE, WavPack,
/// DSD, TAK, TTA, WMA, Musepack, MP2, CAF, ...) is transcoded whenever a
/// format is chosen: at least then it plays.
const DECODABLE_SUFFIXES: &[&str] = &[
    "mp3", "flac", "ogg", "oga", "opus", "wav", "wave", "aif", "aiff", "aifc", "m4a", "m4b", "mp4",
    "aac", "mka", "mkv", "webm",
];

/// What `song` is asked for under `quality`: `None` streams the file as it
/// is. The rule, in order:
///
/// 1. Original: nothing is transcoded.
/// 2. More than 2 channels: MP3 at the cap, whatever format was chosen
///    (nokkvi's Opus decoder is stereo-only, and Navidrome keeps 5.1 in Opus;
///    its MP3 encoder downmixes). Unknown channels count as stereo.
/// 3. A lossless suffix, or one nokkvi cannot decode: the chosen format at
///    the cap.
/// 4. Any other song with no known bitrate (missing or 0): as it is.
/// 5. Above 1.25 times the cap: the chosen format at the cap; otherwise as
///    it is. The margin keeps a VBR album that straddles the cap from
///    flipping per track (and losing its gapless joins).
///
/// A song with no suffix skips 3 and is judged by its bitrate.
pub fn stream_params(song: &Song, quality: StreamQuality) -> Option<StreamParams> {
    let chosen = match quality.format {
        StreamFormat::Original => return None,
        StreamFormat::Opus => TranscodeFormat::Opus,
        StreamFormat::Mp3 => TranscodeFormat::Mp3,
    };
    let cap = quality.max_bitrate.kbps();
    let at_cap = |format| {
        Some(StreamParams {
            format,
            max_bitrate_kbps: cap,
        })
    };
    if song.channels.is_some_and(|channels| channels > 2) {
        return at_cap(TranscodeFormat::Mp3);
    }
    let suffix = song
        .suffix
        .as_deref()
        .map(str::trim)
        .filter(|suffix| !suffix.is_empty())
        .map(str::to_ascii_lowercase);
    if let Some(suffix) = suffix.as_deref()
        && (LOSSLESS_SUFFIXES.contains(&suffix) || !DECODABLE_SUFFIXES.contains(&suffix))
    {
        return at_cap(chosen);
    }
    match song.bitrate {
        None | Some(0) => None,
        Some(kbps) if u64::from(kbps) * 4 > u64::from(cap) * 5 => at_cap(chosen),
        Some(_) => None,
    }
}

/// A [`StreamQuality`] shared between the writer (the settings push) and
/// every stream URL built meanwhile, without a lock: both halves live in one
/// atomic, so a reader never sees a new format with the old cap.
#[derive(Debug)]
pub struct SharedStreamQuality(AtomicU32);

impl SharedStreamQuality {
    pub fn new(quality: StreamQuality) -> Self {
        Self(AtomicU32::new(Self::encode(quality)))
    }

    /// The quality now in force.
    pub fn get(&self) -> StreamQuality {
        Self::decode(self.0.load(Ordering::SeqCst))
    }

    /// Put `quality` in force; whether it differs from the one it replaced.
    pub fn replace(&self, quality: StreamQuality) -> bool {
        let packed = Self::encode(quality);
        self.0.swap(packed, Ordering::SeqCst) != packed
    }

    /// The format in the high half, the cap's kbps in the low half.
    fn encode(quality: StreamQuality) -> u32 {
        let format: u32 = match quality.format {
            StreamFormat::Original => 0,
            StreamFormat::Opus => 1,
            StreamFormat::Mp3 => 2,
        };
        (format << 16) | quality.max_bitrate.kbps()
    }

    fn decode(packed: u32) -> StreamQuality {
        let format = match packed >> 16 {
            1 => StreamFormat::Opus,
            2 => StreamFormat::Mp3,
            // 0, and anything `encode` never writes.
            _ => StreamFormat::Original,
        };
        StreamQuality {
            format,
            max_bitrate: StreamMaxBitrate::at_most(i64::from(packed & 0xFFFF)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A queue song with the three facts the rule reads.
    fn song(suffix: Option<&str>, bitrate: Option<u32>, channels: Option<u32>) -> Song {
        Song {
            suffix: suffix.map(str::to_owned),
            bitrate,
            channels,
            ..Song::test_default("s1", "Song")
        }
    }

    fn quality(format: StreamFormat, max_bitrate: StreamMaxBitrate) -> StreamQuality {
        StreamQuality {
            format,
            max_bitrate,
        }
    }

    const OPUS_128: StreamQuality = StreamQuality {
        format: StreamFormat::Opus,
        max_bitrate: StreamMaxBitrate::Kbps128,
    };

    /// The `stream_params` answer for a transcode, to compare against.
    #[expect(
        clippy::unnecessary_wraps,
        reason = "shaped like the `stream_params` answer it is compared with"
    )]
    fn transcode(format: TranscodeFormat, max_bitrate_kbps: u32) -> Option<StreamParams> {
        Some(StreamParams {
            format,
            max_bitrate_kbps,
        })
    }

    #[test]
    fn original_asks_for_no_transcode_whatever_the_song() {
        for song in [
            song(Some("flac"), Some(1_000), Some(6)),
            song(Some("ape"), None, None),
            song(Some("mp3"), Some(320), Some(2)),
            song(None, None, None),
        ] {
            for cap in StreamMaxBitrate::ALL {
                assert_eq!(
                    stream_params(&song, quality(StreamFormat::Original, cap)),
                    None,
                    "{:?} at {cap:?}",
                    song.suffix
                );
            }
        }
    }

    #[test]
    fn a_song_with_more_than_two_channels_streams_as_mp3_at_the_cap() {
        // nokkvi's Opus decoder is stereo-only, and Navidrome keeps 5.1 in
        // Opus, so a surround song goes MP3 (downmixed) even when Opus is
        // chosen, and even when it is a lossy file under the cap.
        for song in [
            song(Some("flac"), Some(2_500), Some(6)),
            song(Some("m4a"), Some(96), Some(6)),
            song(None, None, Some(3)),
        ] {
            assert_eq!(
                stream_params(&song, OPUS_128),
                transcode(TranscodeFormat::Mp3, 128),
                "{:?}",
                song.suffix
            );
            assert_eq!(
                stream_params(&song, quality(StreamFormat::Mp3, StreamMaxBitrate::Kbps192)),
                transcode(TranscodeFormat::Mp3, 192)
            );
        }
    }

    #[test]
    fn a_song_whose_channels_are_unknown_is_taken_for_stereo() {
        assert_eq!(
            stream_params(&song(Some("flac"), Some(900), None), OPUS_128),
            transcode(TranscodeFormat::Opus, 128)
        );
        assert_eq!(
            stream_params(&song(Some("flac"), Some(900), Some(2)), OPUS_128),
            transcode(TranscodeFormat::Opus, 128)
        );
        assert_eq!(
            stream_params(&song(Some("mp3"), Some(128), None), OPUS_128),
            None
        );
    }

    #[test]
    fn a_lossless_file_is_transcoded_to_the_chosen_format_whatever_its_bitrate() {
        for suffix in ["flac", "FLAC", "wav", "wave", "aif", "aiff", "aifc"] {
            for bitrate in [None, Some(0), Some(100), Some(1_411)] {
                assert_eq!(
                    stream_params(&song(Some(suffix), bitrate, Some(2)), OPUS_128),
                    transcode(TranscodeFormat::Opus, 128),
                    "{suffix} at {bitrate:?}"
                );
            }
        }
        assert_eq!(
            stream_params(
                &song(Some("flac"), Some(900), Some(2)),
                quality(StreamFormat::Mp3, StreamMaxBitrate::Kbps320)
            ),
            transcode(TranscodeFormat::Mp3, 320)
        );
    }

    #[test]
    fn a_format_nokkvi_cannot_decode_is_transcoded_even_under_the_cap() {
        for suffix in [
            "ape", "wv", "dsf", "dff", "tak", "tta", "wma", "mpc", "mp2", "caf",
        ] {
            for bitrate in [None, Some(0), Some(64)] {
                assert_eq!(
                    stream_params(&song(Some(suffix), bitrate, Some(2)), OPUS_128),
                    transcode(TranscodeFormat::Opus, 128),
                    "{suffix} at {bitrate:?}"
                );
            }
        }
    }

    #[test]
    fn a_lossy_file_with_no_known_bitrate_streams_as_it_is() {
        for suffix in [Some("mp3"), Some("ogg"), Some("opus"), Some("m4a"), None] {
            for bitrate in [None, Some(0)] {
                assert_eq!(
                    stream_params(&song(suffix, bitrate, Some(2)), OPUS_128),
                    None,
                    "{suffix:?} at {bitrate:?}"
                );
            }
        }
    }

    /// Up to 1.25 times the cap a lossy file plays as it is, so a VBR album
    /// straddling the cap doesn't flip per track and lose its gapless joins.
    #[test]
    fn a_lossy_file_is_transcoded_only_above_a_quarter_over_the_cap() {
        for (cap, under, over) in [
            (StreamMaxBitrate::Kbps128, 160, 161),
            (StreamMaxBitrate::Kbps64, 80, 81),
            (StreamMaxBitrate::Kbps96, 120, 121),
            (StreamMaxBitrate::Kbps320, 400, 401),
        ] {
            let q = quality(StreamFormat::Opus, cap);
            for suffix in ["mp3", "ogg", "opus", "m4a", "aac", "webm"] {
                assert_eq!(
                    stream_params(&song(Some(suffix), Some(under), Some(2)), q),
                    None,
                    "{suffix} at {under} kbps, cap {cap:?}"
                );
                assert_eq!(
                    stream_params(&song(Some(suffix), Some(over), Some(2)), q),
                    transcode(TranscodeFormat::Opus, cap.kbps()),
                    "{suffix} at {over} kbps, cap {cap:?}"
                );
            }
        }
        assert_eq!(
            stream_params(&song(Some("mp3"), Some(64), Some(2)), OPUS_128),
            None,
            "well under the cap"
        );
    }

    #[test]
    fn a_song_with_no_suffix_is_judged_by_its_bitrate() {
        assert_eq!(
            stream_params(&song(None, Some(900), Some(2)), OPUS_128),
            transcode(TranscodeFormat::Opus, 128)
        );
        assert_eq!(
            stream_params(&song(Some(""), Some(128), Some(2)), OPUS_128),
            None
        );
    }

    #[test]
    fn a_transcode_is_never_raw_and_never_a_cap_alone() {
        let songs = [
            song(Some("flac"), Some(900), Some(2)),
            song(Some("mp3"), Some(320), Some(2)),
            song(Some("ape"), None, Some(6)),
        ];
        let mut transcodes = 0;
        for format in [StreamFormat::Opus, StreamFormat::Mp3] {
            for cap in StreamMaxBitrate::ALL {
                for song in &songs {
                    if let Some(params) = stream_params(song, quality(format, cap)) {
                        assert!(matches!(params.format.wire(), "opus" | "mp3"), "{params:?}");
                        assert_eq!(params.max_bitrate_kbps, cap.kbps());
                        transcodes += 1;
                    }
                }
            }
        }
        assert!(transcodes > 0, "the FLAC and the APE always transcode");
    }

    #[test]
    fn a_shared_quality_reports_only_a_real_change() {
        let shared = SharedStreamQuality::new(OPUS_128);
        assert_eq!(shared.get(), OPUS_128);
        assert!(!shared.replace(OPUS_128), "the same quality is no change");
        let mp3 = quality(StreamFormat::Mp3, StreamMaxBitrate::Kbps128);
        assert!(shared.replace(mp3), "a new format is a change");
        assert_eq!(shared.get(), mp3);
        let cap = quality(StreamFormat::Mp3, StreamMaxBitrate::Kbps96);
        assert!(shared.replace(cap), "a new cap is a change");
        assert!(!shared.replace(cap));
        assert_eq!(shared.get(), cap);
    }

    #[test]
    fn a_shared_quality_holds_every_setting_exactly() {
        for format in [
            StreamFormat::Original,
            StreamFormat::Opus,
            StreamFormat::Mp3,
        ] {
            for cap in StreamMaxBitrate::ALL {
                let q = quality(format, cap);
                assert_eq!(SharedStreamQuality::new(q).get(), q);
            }
        }
    }
}
