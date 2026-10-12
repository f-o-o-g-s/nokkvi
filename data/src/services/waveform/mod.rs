//! Song envelopes for the Waveform seek bar style.
//!
//! Navidrome serves no peak data, so a song's envelope is worked out from its
//! own stream, once, and kept in a small file under
//! `~/.cache/nokkvi/waveforms/` ([`cache`]). The scan ([`scan`]) requests the
//! exact stream URL playback uses: with no transcoding on nokkvi's player the
//! server sends the original file, and with one it serves the same transcode
//! (and transcode cache entry) playback gets, so the scan adds no ffmpeg run
//! and the envelope matches what is heard. A scan that runs too slowly stops
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

use anyhow::{Context, Result};
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
    /// came through. `cancel` stops it (the song stopped being wanted, or the
    /// session ended).
    pub async fn scan(
        &self,
        server_url: &str,
        id: &WaveformIdentity,
        stream_url: &str,
        cancel: CancellationToken,
    ) -> Result<ScanEnd> {
        if !self.scanning() {
            return Ok(ScanEnd::Cancelled);
        }
        let expected_ms = (id.duration_secs > 0).then(|| u64::from(id.duration_secs) * 1000);
        let started = std::time::Instant::now();
        let end = scan::scan_stream(stream_url, expected_ms, &id.song_id, cancel).await?;
        match &end {
            ScanEnd::Ready(waveform) => {
                debug!(song = %id.song_id, took = ?started.elapsed(), "waveform scanned");
                if let Some(root) = &self.root {
                    let cache = WaveformCache::for_server(root, server_url);
                    let (id, waveform) = (id.clone(), Arc::clone(waveform));
                    tokio::task::spawn_blocking(move || cache.store(&id, &waveform))
                        .await
                        .context("waveform cache write thread")?
                        .context("cache the waveform")?;
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
            ScanEnd::Cancelled => {}
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
                "http://127.0.0.1:9/rest/stream?id=s",
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(matches!(end, ScanEnd::Cancelled), "{end:?}");
    }
}
