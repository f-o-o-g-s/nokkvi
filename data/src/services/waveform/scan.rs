//! Working a song's envelope out from its stream.
//!
//! The scan opens the stream with playback's own [`AudioDecoder`] (same
//! redirect guard, error-envelope check, probe and codecs, so whatever plays
//! also scans), then folds every decoded buffer into a [`WaveformBuilder`] on
//! a blocking thread. It gives up — and caches nothing — when it is cancelled,
//! when the link is too slow to scan without crowding playback, or when the
//! stream ends well short of the song's length (a dropped connection reads as
//! a clean end of file, and caching it would stretch half a song across the
//! whole bar).

use std::{sync::Arc, time::Instant};

use anyhow::{Context, Result};
use tokio_util::sync::CancellationToken;
use tracing::Instrument as _;

use crate::{
    audio::decoder::AudioDecoder,
    types::waveform::{Waveform, WaveformBuilder},
};

/// Bytes of decoded PCM asked for per read: ~0.7 s of 44.1 kHz stereo f32.
const READ_BYTES: usize = 256 * 1024;

/// Wall time the scan runs before its speed is judged, so the connection and
/// probe don't count against it.
const SPEED_GRACE_SECS: f64 = 3.0;

/// Slowest acceptable scan, in seconds of audio per second of wall time —
/// that is, how many times the stream's own bitrate the link delivers. Under
/// 4x, a full-speed second download would leave playback too little of the
/// link (TCP splits it roughly evenly), so the session stops scanning. Hi-res
/// (24/192 FLAC, ~9 Mbps) still clears it on a 100 Mbps link.
const MIN_SPEED: f64 = 4.0;

/// How far short of the expected length a finished scan may end and still
/// count as whole: 2 % of the length, at least 2 s (metadata lengths are
/// whole seconds, and gapless trimming drops encoder padding).
const SHORT_TOLERANCE_MIN_MS: u64 = 2_000;

/// How a scan ended.
#[derive(Debug, Clone)]
pub enum ScanEnd {
    /// The whole song was folded.
    Ready(Arc<Waveform>),
    /// The song stopped being wanted, or the session ended.
    Cancelled,
    /// The stream ended this far into a song of `expected_ms`.
    Truncated { decoded_ms: u64, expected_ms: u64 },
    /// The scan ran slower than [`MIN_SPEED`].
    TooSlow { speed: f64 },
    /// The song left the queue before its scan could start.
    NotInQueue,
}

/// One read from a PCM source.
pub(super) enum Read {
    /// Interleaved samples in `-1.0..=1.0`.
    Samples(Vec<f32>),
    /// Nothing yet (a read timed out); ask again.
    Pending,
    /// The source is finished.
    End,
}

/// What the fold needs from a decoder; [`AudioDecoder`] in the app, a fake in
/// tests.
pub(super) trait PcmSource {
    fn sample_rate(&self) -> u32;
    fn channels(&self) -> usize;
    /// The song's length as the source knows it (ms), 0 when unknown.
    fn expected_ms(&self) -> u64;
    fn read(&mut self) -> Read;
}

impl PcmSource for AudioDecoder {
    fn sample_rate(&self) -> u32 {
        self.format().sample_rate()
    }

    fn channels(&self) -> usize {
        self.format().channel_count() as usize
    }

    fn expected_ms(&self) -> u64 {
        self.duration()
    }

    fn read(&mut self) -> Read {
        let buffer = self.read_buffer(READ_BYTES);
        let bytes = buffer.data();
        if !buffer.is_valid() || bytes.is_empty() {
            // An empty buffer is either the end or a 500 ms read timeout;
            // only `is_eof` tells them apart.
            return if self.is_eof() {
                Read::End
            } else {
                Read::Pending
            };
        }
        Read::Samples(
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes(*b))
                .collect(),
        )
    }
}

/// Open `url` and fold the whole stream. `expected_ms` is the server's length
/// for the song, which the decoder cross-checks its probe against. Runs in a
/// `waveform_scan` span (entered on the blocking thread too), so the
/// decoder's own warnings read as the scan's, not playback's.
pub(super) async fn scan_stream(
    url: &str,
    expected_ms: Option<u64>,
    song_id: &str,
    cancel: CancellationToken,
) -> Result<ScanEnd> {
    let span = tracing::debug_span!("waveform_scan", song = %song_id);
    let mut decoder = AudioDecoder::new(Arc::new(std::sync::RwLock::new(None)));
    decoder.set_expected_duration_ms(expected_ms);
    let opened = async {
        tokio::select! {
            opened = decoder.init(url) => opened.map(|()| true),
            () = cancel.cancelled() => Ok(false),
        }
    }
    .instrument(span.clone())
    .await
    .context("open the stream for the waveform scan")?;
    if !opened {
        return Ok(ScanEnd::Cancelled);
    }
    tokio::task::spawn_blocking(move || {
        let _enter = span.enter();
        let start = Instant::now();
        fold(&mut decoder, &cancel, || start.elapsed().as_secs_f64())
    })
    .await
    .context("waveform scan thread")?
}

/// Fold `source` into an envelope. `wall_secs` reports seconds since the scan
/// started (injected so tests can run on a fake clock).
pub(super) fn fold(
    source: &mut impl PcmSource,
    cancel: &CancellationToken,
    mut wall_secs: impl FnMut() -> f64,
) -> Result<ScanEnd> {
    let mut builder = WaveformBuilder::new(source.sample_rate(), source.channels())
        .context("the stream reported no audio format")?;
    loop {
        if cancel.is_cancelled() {
            return Ok(ScanEnd::Cancelled);
        }
        match source.read() {
            Read::Samples(samples) => builder.push_interleaved(&samples),
            Read::Pending => {}
            Read::End => break,
        }
        let wall = wall_secs();
        if wall >= SPEED_GRACE_SECS {
            let speed = builder.decoded_secs() / wall;
            if speed < MIN_SPEED {
                return Ok(ScanEnd::TooSlow { speed });
            }
        }
    }
    let decoded_ms = (builder.decoded_secs() * 1000.0) as u64;
    let expected_ms = source.expected_ms();
    let tolerance = (expected_ms / 50).max(SHORT_TOLERANCE_MIN_MS);
    if expected_ms > 0 && decoded_ms + tolerance < expected_ms {
        return Ok(ScanEnd::Truncated {
            decoded_ms,
            expected_ms,
        });
    }
    builder
        .finish()
        .map(|waveform| ScanEnd::Ready(Arc::new(waveform)))
        .context("the stream held no audio")
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    /// A mono source handing out `reads`, then the end.
    struct Fake {
        reads: VecDeque<Read>,
        expected_ms: u64,
    }

    impl Fake {
        /// `seconds` of one-second mono reads at 1 kHz.
        fn seconds(seconds: usize, expected_ms: u64) -> Self {
            Self {
                reads: (0..seconds)
                    .map(|_| Read::Samples(vec![0.5; 1000]))
                    .collect(),
                expected_ms,
            }
        }
    }

    impl PcmSource for Fake {
        fn sample_rate(&self) -> u32 {
            1000
        }
        fn channels(&self) -> usize {
            1
        }
        fn expected_ms(&self) -> u64 {
            self.expected_ms
        }
        fn read(&mut self) -> Read {
            self.reads.pop_front().unwrap_or(Read::End)
        }
    }

    /// A clock that advances `step` seconds per call.
    fn clock(step: f64) -> impl FnMut() -> f64 {
        let mut t = 0.0;
        move || {
            t += step;
            t
        }
    }

    #[test]
    fn a_whole_song_folds_into_an_envelope() {
        let mut source = Fake::seconds(60, 60_000);
        let end = fold(&mut source, &CancellationToken::new(), clock(0.01)).unwrap();
        assert!(matches!(end, ScanEnd::Ready(_)), "{end:?}");
    }

    #[test]
    fn timed_out_reads_are_waited_out() {
        let mut source = Fake::seconds(10, 10_000);
        source.reads.push_front(Read::Pending);
        source.reads.insert(5, Read::Pending);
        let end = fold(&mut source, &CancellationToken::new(), clock(0.01)).unwrap();
        assert!(matches!(end, ScanEnd::Ready(_)), "{end:?}");
    }

    /// A dropped connection reads as a clean end; caching it would stretch
    /// half the song over the whole bar.
    #[test]
    fn a_stream_that_ends_early_is_not_kept() {
        let mut source = Fake::seconds(30, 60_000);
        let end = fold(&mut source, &CancellationToken::new(), clock(0.01)).unwrap();
        assert!(
            matches!(
                end,
                ScanEnd::Truncated {
                    decoded_ms: 30_000,
                    expected_ms: 60_000
                }
            ),
            "{end:?}"
        );
        // Within the tolerance (whole-second metadata, trimmed padding) is whole.
        let mut close = Fake::seconds(59, 60_000);
        let end = fold(&mut close, &CancellationToken::new(), clock(0.01)).unwrap();
        assert!(matches!(end, ScanEnd::Ready(_)), "{end:?}");
    }

    #[test]
    fn a_slow_link_stops_the_scan_after_the_grace_period() {
        // One second of audio per wall second: far under MIN_SPEED.
        let mut source = Fake::seconds(600, 600_000);
        let end = fold(&mut source, &CancellationToken::new(), clock(1.0)).unwrap();
        assert!(matches!(end, ScanEnd::TooSlow { .. }), "{end:?}");
        // The same speed inside the grace period is not judged.
        let mut short = Fake::seconds(2, 2_000);
        let end = fold(&mut short, &CancellationToken::new(), clock(1.0)).unwrap();
        assert!(matches!(end, ScanEnd::Ready(_)), "{end:?}");
    }

    #[test]
    fn cancelling_stops_the_scan() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        let mut source = Fake::seconds(60, 60_000);
        let end = fold(&mut source, &cancel, clock(0.01)).unwrap();
        assert!(matches!(end, ScanEnd::Cancelled), "{end:?}");
    }

    #[test]
    fn a_source_without_audio_is_an_error() {
        let mut source = Fake::seconds(0, 0);
        assert!(fold(&mut source, &CancellationToken::new(), clock(0.01)).is_err());
    }
}
