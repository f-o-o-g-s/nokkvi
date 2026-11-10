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
    /// What the setting asks of the server: the cap counts only beside a
    /// format, so every Original reads the same (Original at the default
    /// cap) and a Max Bitrate change under Original is no change.
    pub fn effective(self) -> Self {
        match self.format {
            StreamFormat::Original => Self::default(),
            StreamFormat::Opus | StreamFormat::Mp3 => self,
        }
    }

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

/// File suffixes of lossless audio nokkvi decodes itself.
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
/// 3. A suffix nokkvi cannot decode: the chosen format at the cap. A
///    lossless suffix: as it is when its known bitrate is at or under the cap
///    (a mono or quiet FLAC would come out no smaller, and lossy), otherwise
///    (above the cap, or no known bitrate) the chosen format at the cap.
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
    if let Some(suffix) = suffix.as_deref() {
        if !DECODABLE_SUFFIXES.contains(&suffix) {
            return at_cap(chosen);
        }
        if LOSSLESS_SUFFIXES.contains(&suffix) {
            return match song.bitrate {
                Some(kbps) if kbps > 0 && kbps <= cap => None,
                Some(_) | None => at_cap(chosen),
            };
        }
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

    /// Put `quality` in force; whether it asks the server for anything other
    /// than the one it replaced did ([`StreamQuality::effective`]: a cap
    /// change under Original is none).
    pub fn replace(&self, quality: StreamQuality) -> bool {
        let previous = Self::decode(self.0.swap(Self::encode(quality), Ordering::SeqCst));
        previous.effective() != quality.effective()
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

    /// A lossless file above the cap, or one whose bitrate isn't known, is
    /// transcoded to the chosen format.
    #[test]
    fn a_lossless_file_above_the_cap_or_of_unknown_bitrate_is_transcoded() {
        for suffix in [
            "flac", "FLAC", " flac ", "wav", "wave", "aif", "aiff", "aifc",
        ] {
            for bitrate in [None, Some(0), Some(129), Some(1_411)] {
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

    /// A lossless file whose known bitrate is at or under the cap (a mono or
    /// quiet FLAC) streams as it is: a transcode would come out no smaller,
    /// and lossy.
    #[test]
    fn a_lossless_file_at_or_under_the_cap_streams_as_it_is() {
        for (cap, at) in [
            (StreamMaxBitrate::Kbps128, 128),
            (StreamMaxBitrate::Kbps320, 320),
            (StreamMaxBitrate::Kbps64, 64),
        ] {
            for suffix in ["flac", "WAV", " aiff "] {
                for bitrate in [1, at - 1, at] {
                    assert_eq!(
                        stream_params(
                            &song(Some(suffix), Some(bitrate), Some(2)),
                            quality(StreamFormat::Opus, cap)
                        ),
                        None,
                        "{suffix} at {bitrate} kbps, cap {cap:?}"
                    );
                }
            }
        }
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
            (StreamMaxBitrate::Kbps160, 200, 201),
            (StreamMaxBitrate::Kbps192, 240, 241),
            (StreamMaxBitrate::Kbps256, 320, 321),
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

    /// The suffix is read trimmed and in any case.
    #[test]
    fn a_suffix_with_spaces_or_capitals_is_read_as_its_format() {
        assert_eq!(
            stream_params(&song(Some(" FLAC "), Some(900), Some(2)), OPUS_128),
            transcode(TranscodeFormat::Opus, 128)
        );
        assert_eq!(
            stream_params(&song(Some(" Mp3 "), Some(128), Some(2)), OPUS_128),
            None,
            "a lossy file under the cap, not an undecodable format"
        );
        assert_eq!(
            stream_params(&song(Some("APE"), Some(64), Some(2)), OPUS_128),
            transcode(TranscodeFormat::Opus, 128)
        );
    }

    /// A lossy file nokkvi decodes, well above the cap, is transcoded like
    /// any other: an ALAC `.m4a` (judged by its bitrate) or a high Vorbis.
    #[test]
    fn an_m4a_or_ogg_at_900_kbps_is_transcoded() {
        for suffix in ["m4a", "ogg"] {
            assert_eq!(
                stream_params(&song(Some(suffix), Some(900), Some(2)), OPUS_128),
                transcode(TranscodeFormat::Opus, 128),
                "{suffix}"
            );
        }
    }

    /// Under Original the cap is ignored, so a Max Bitrate change there is
    /// no change at all (it would otherwise drop the prepared next track for
    /// nothing).
    #[test]
    fn a_max_bitrate_change_under_original_is_no_change() {
        let shared = SharedStreamQuality::new(StreamQuality::default());
        for cap in StreamMaxBitrate::ALL {
            assert!(
                !shared.replace(quality(StreamFormat::Original, cap)),
                "Original at {cap:?}"
            );
        }
        assert!(shared.replace(OPUS_128), "a format is a change");
        assert!(
            shared.replace(quality(StreamFormat::Opus, StreamMaxBitrate::Kbps320)),
            "a cap under Opus is a change"
        );
        assert!(shared.replace(quality(StreamFormat::Original, StreamMaxBitrate::Kbps64)));
        assert!(!shared.replace(quality(StreamFormat::Original, StreamMaxBitrate::Kbps192)));
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
