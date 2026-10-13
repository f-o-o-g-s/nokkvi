//! Song envelopes for the Waveform seek bar style.
//!
//! Navidrome serves no peak data, so a song's envelope is worked out from its
//! own stream, once, and kept in a small file under
//! `~/.cache/nokkvi/waveforms/` ([`cache`]). The scan ([`scan`]) requests the
//! exact stream URL playback uses: with no transcoding on nokkvi's player the
//! server sends the original file, and with one it serves the same transcode
//! playback gets, so the envelope matches what is heard. With Navidrome's
//! transcode cache on (its default), the scan also reads the same cache
//! entry, in progress or finished, so it starts no second ffmpeg run. A scan that runs too slowly stops
//! scanning for the rest of the session, so a slow link never trades
//! playback's buffering for a waveform.

mod cache;
mod scan;

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::Result;
pub use cache::WaveformIdentity;
pub use scan::ScanEnd;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use self::cache::WaveformCache;
use crate::types::waveform::Waveform;

/// The cache is pruned back to [`CACHE_TARGET_BYTES`] (least recently used
/// first) once it grows past this. ~2 KB a song: tens of thousands of songs.
const CACHE_MAX_BYTES: u64 = 64 << 20;
const CACHE_TARGET_BYTES: u64 = 48 << 20;

/// What a cache lookup for a queue song found.
#[derive(Debug, Clone)]
pub enum WaveformLookup {
    /// An envelope worked out from the same audio.
    Cached(Arc<Waveform>),
    /// No envelope for this audio yet: scan it.
    NotCached,
    /// The song left the queue, so its audio can't be identified.
    NotInQueue,
}

/// Envelope lookups and scans for one session.
#[derive(Debug)]
pub struct WaveformService {
    /// `~/.cache/nokkvi/waveforms`, or `None` when no cache directory
    /// resolves (scans still work; nothing is kept).
    root: Option<PathBuf>,
    /// Set once a scan ran too slowly: the rest of the session only reads the
    /// cache.
    scans_off: AtomicBool,
}

impl Default for WaveformService {
    fn default() -> Self {
        Self::new()
    }
}

impl WaveformService {
    pub fn new() -> Self {
        let root = match crate::utils::paths::get_cache_dir() {
            Ok(dir) => Some(dir.join("waveforms")),
            Err(e) => {
                warn!(error = %e, "waveforms: no cache directory, envelopes won't be kept");
                None
            }
        };
        Self {
            root,
            scans_off: AtomicBool::new(false),
        }
    }

    /// A service keeping its cache under `root` (tests).
    #[cfg(test)]
    fn with_root(root: PathBuf) -> Self {
        Self {
            root: Some(root),
            scans_off: AtomicBool::new(false),
        }
    }

    /// Trim the cache back under its cap, off the async runtime. Run once per
    /// session.
    pub async fn prune(&self) {
        let Some(root) = self.root.clone() else {
            return;
        };
        match tokio::task::spawn_blocking(move || {
            cache::prune(&root, CACHE_MAX_BYTES, CACHE_TARGET_BYTES)
        })
        .await
        {
            Ok(0) => {}
            Ok(removed) => debug!(removed, "waveforms: pruned the cache"),
            Err(e) => warn!(error = %e, "waveforms: prune thread failed"),
        }
    }

    /// Whether this session still scans (it stops after a too-slow scan).
    pub fn scanning(&self) -> bool {
        !self.scans_off.load(Ordering::Relaxed)
    }

    /// The cached envelope for `id` on `server_url`, if one was worked out
    /// from the same audio.
    pub async fn cached(&self, server_url: &str, id: &WaveformIdentity) -> Option<Arc<Waveform>> {
        let cache = WaveformCache::for_server(self.root.as_ref()?, server_url);
        let id = id.clone();
        tokio::task::spawn_blocking(move || cache.load(&id))
            .await
            .ok()
            .flatten()
            .map(Arc::new)
    }

    /// Scan `stream_url` for `id`, caching the envelope when the whole song
    /// came through. `expected_ms` is the song's length as the server reports
    /// it (`Song::expected_duration_ms`, the value playback's decoder is
    /// given), which a scan that ends short is judged against. `cancel` stops
    /// it (the song stopped being wanted, or the session ended).
    pub async fn scan(
        &self,
        server_url: &str,
        id: &WaveformIdentity,
        expected_ms: Option<u64>,
        stream_url: &str,
        cancel: CancellationToken,
    ) -> Result<ScanEnd> {
        if !self.scanning() {
            return Ok(ScanEnd::Cancelled);
        }
        let started = std::time::Instant::now();
        let end = scan::scan_stream(stream_url, expected_ms, &id.song_id, cancel).await?;
        match &end {
            ScanEnd::Ready(waveform) => {
                debug!(song = %id.song_id, took = ?started.elapsed(), "waveform scanned");
                // A failed write costs a rescan on the next play, never the
                // envelope this scan just worked out.
                if let Some(root) = &self.root {
                    let cache = WaveformCache::for_server(root, server_url);
                    let (song, waveform) = (id.clone(), Arc::clone(waveform));
                    match tokio::task::spawn_blocking(move || cache.store(&song, &waveform)).await {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => {
                            warn!(song = %id.song_id, error = %format!("{e:#}"), "waveform cache write failed");
                        }
                        Err(e) => {
                            warn!(song = %id.song_id, error = %e, "waveform cache write thread failed");
                        }
                    }
                }
            }
            ScanEnd::TooSlow { speed } => {
                info!(
                    song = %id.song_id,
                    speed = format!("{speed:.1}x"),
                    "waveform scan ran too slowly for this link; scanning stops for this session"
                );
                self.scans_off.store(true, Ordering::Relaxed);
            }
            ScanEnd::Truncated {
                decoded_ms,
                expected_ms,
            } => {
                debug!(
                    song = %id.song_id, decoded_ms, expected_ms,
                    "waveform scan ended short of the song; not cached"
                );
            }
            ScanEnd::Cancelled | ScanEnd::NotInQueue => {}
        }
        Ok(end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> WaveformIdentity {
        WaveformIdentity {
            song_id: "s".into(),
            size: 10,
            duration_secs: 60,
            suffix: "flac".into(),
        }
    }

    #[tokio::test]
    async fn a_stored_envelope_is_served_from_the_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let service = WaveformService::with_root(tmp.path().to_path_buf());
        assert!(service.cached("http://server", &identity()).await.is_none());
        let waveform = Waveform::new(vec![1, 2], vec![1, 2]).unwrap();
        WaveformCache::for_server(tmp.path(), "http://server")
            .store(&identity(), &waveform)
            .unwrap();
        assert_eq!(
            service
                .cached("http://server", &identity())
                .await
                .as_deref(),
            Some(&waveform)
        );
    }

    /// A mono 16-bit 8 kHz WAV: one quiet second, then one loud second.
    fn wav(path: &std::path::Path) {
        let rate: u32 = 8000;
        let samples: Vec<i16> = (0..2 * rate)
            .map(|i| {
                let amp = if i < rate { 2000.0 } else { 24000.0 };
                (amp * (i as f32 * 0.3).sin()) as i16
            })
            .collect();
        let data_len = (samples.len() * 2) as u32;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&1u16.to_le_bytes()); // mono
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for s in samples {
            out.extend_from_slice(&s.to_le_bytes());
        }
        std::fs::write(path, out).unwrap();
    }

    fn wav_identity() -> WaveformIdentity {
        WaveformIdentity {
            song_id: "wav".into(),
            size: 0,
            duration_secs: 2,
            suffix: "wav".into(),
        }
    }

    /// The whole path the app takes — playback's decoder opening the source,
    /// the fold, the cache write — on a real (local) file.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_song_scans_end_to_end_and_is_cached() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("song.wav");
        wav(&file);
        let service = WaveformService::with_root(tmp.path().join("cache"));
        let end = service
            .scan(
                "http://server",
                &wav_identity(),
                Some(2_000),
                file.to_str().unwrap(),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let ScanEnd::Ready(waveform) = end else {
            panic!("expected an envelope, got {end:?}");
        };
        let half = waveform.rms().len() / 2;
        assert!(
            waveform.rms()[half + 10] > waveform.rms()[10] * 4,
            "loud half is louder"
        );
        assert_eq!(
            service
                .cached("http://server", &wav_identity())
                .await
                .as_deref(),
            Some(&*waveform)
        );
    }

    /// A cache that can't be written still hands the envelope to the bar.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_failed_cache_write_keeps_the_envelope() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("song.wav");
        wav(&file);
        // A file where the cache directory should be: every write fails.
        let blocked = tmp.path().join("blocked");
        std::fs::write(&blocked, b"not a directory").unwrap();
        let service = WaveformService::with_root(blocked);
        let end = service
            .scan(
                "http://server",
                &wav_identity(),
                Some(2_000),
                file.to_str().unwrap(),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(matches!(end, ScanEnd::Ready(_)), "{end:?}");
    }

    #[tokio::test]
    async fn a_session_that_stopped_scanning_never_opens_a_stream() {
        let tmp = tempfile::tempdir().unwrap();
        let service = WaveformService::with_root(tmp.path().to_path_buf());
        service.scans_off.store(true, Ordering::Relaxed);
        // An unreachable URL: reaching the network would be an error.
        let end = service
            .scan(
                "http://server",
                &identity(),
                Some(60_000),
                "http://127.0.0.1:9/rest/stream?id=s",
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(matches!(end, ScanEnd::Cancelled), "{end:?}");
    }
}
