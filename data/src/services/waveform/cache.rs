//! On-disk waveform cache: one small file per song under
//! `~/.cache/nokkvi/waveforms/<server>/`.
//!
//! A file records the identity of the audio it was worked out from (size,
//! duration, suffix); a song whose file changed on the server no longer
//! matches and is scanned again. Writes go through a temp file + rename, so a
//! reader never sees half a file.

use std::{
    io::Write as _,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::SystemTime,
};

use anyhow::{Context, Result};
use md5::{Digest, Md5};

use crate::types::waveform::Waveform;

/// File signature and layout version. Bump the version when the layout or the
/// envelope's meaning changes; older files then read as misses.
const MAGIC: &[u8; 4] = b"NKWF";
const VERSION: u8 = 1;

/// What a cached envelope was worked out from. A file whose recorded identity
/// differs (the song's file was replaced on the server) is a miss.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaveformIdentity {
    pub song_id: String,
    /// File size in bytes as the server reports it (0 when unknown).
    pub size: u64,
    /// Duration in whole seconds as the server reports it.
    pub duration_secs: u32,
    /// The file's suffix (`flac`, `mp3`, …), empty when unknown.
    pub suffix: String,
}

impl WaveformIdentity {
    /// The identity of a queue song's audio, from its server metadata.
    pub fn of(song: &crate::types::song::Song) -> Self {
        Self {
            song_id: song.id.clone(),
            size: song.size,
            duration_secs: song.duration,
            suffix: song.suffix.clone().unwrap_or_default(),
        }
    }
}

/// The cache directory for one server.
#[derive(Debug, Clone)]
pub(super) struct WaveformCache {
    dir: PathBuf,
}

impl WaveformCache {
    /// `root/<server key>`: song ids are only unique per server. The URL is
    /// keyed the way the queue's session owner is
    /// ([`server_key`](crate::services::queue::slots::server_key)), so a
    /// re-typed URL shares the cache.
    pub(super) fn for_server(root: &Path, server_url: &str) -> Self {
        let key = hex_md5(&crate::services::queue::slots::server_key(server_url));
        Self {
            dir: root.join(&key[..16]),
        }
    }

    fn path(&self, song_id: &str) -> PathBuf {
        self.dir.join(format!("{}.wf", hex_md5(song_id)))
    }

    /// The cached envelope for `id`, or `None` for a missing, unreadable,
    /// outdated or mismatched file. A hit refreshes the file's modified time,
    /// which is what pruning keeps by.
    pub(super) fn load(&self, id: &WaveformIdentity) -> Option<Waveform> {
        let path = self.path(&id.song_id);
        let bytes = std::fs::read(&path).ok()?;
        let waveform = decode(&bytes, id)?;
        if let Ok(file) = std::fs::File::options().write(true).open(&path)
            && let Err(e) = file.set_modified(SystemTime::now())
        {
            tracing::trace!(error = %e, "waveform cache: could not refresh mtime");
        }
        Some(waveform)
    }

    /// Store `waveform` for `id`, replacing any older file.
    pub(super) fn store(&self, id: &WaveformIdentity, waveform: &Waveform) -> Result<()> {
        std::fs::create_dir_all(&self.dir)
            .with_context(|| format!("create {}", self.dir.display()))?;
        let path = self.path(&id.song_id);
        // A name no other writer shares: two scans of one song (a cancelled
        // one still finishing, say) must not rename each other's file away.
        static WRITES: AtomicU64 = AtomicU64::new(0);
        let tmp = path.with_extension(format!(
            "wf.{}.{}.tmp",
            std::process::id(),
            WRITES.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file =
            std::fs::File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
        file.write_all(&encode(id, waveform))
            .with_context(|| format!("write {}", tmp.display()))?;
        if let Err(e) = file.sync_all() {
            // The rename below still lands the bytes; a crash before the
            // flush costs at most one rescan.
            tracing::trace!(error = %e, "waveform cache: fsync failed");
        }
        std::fs::rename(&tmp, &path).with_context(|| format!("rename to {}", path.display()))
    }
}

fn hex_md5(s: &str) -> String {
    Md5::digest(s.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn encode(id: &WaveformIdentity, waveform: &Waveform) -> Vec<u8> {
    let suffix = id.suffix.as_bytes();
    let suffix = &suffix[..suffix.len().min(usize::from(u8::MAX))];
    let bins = waveform.peaks().len();
    let mut out = Vec::with_capacity(24 + suffix.len() + 2 * bins);
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.extend_from_slice(&id.size.to_le_bytes());
    out.extend_from_slice(&id.duration_secs.to_le_bytes());
    out.push(suffix.len() as u8);
    out.extend_from_slice(suffix);
    out.extend_from_slice(&(bins as u32).to_le_bytes());
    out.extend_from_slice(waveform.peaks());
    out.extend_from_slice(waveform.rms());
    out
}

/// Parse a cache file, `None` unless it is well formed AND was worked out from
/// the audio `id` describes.
fn decode(bytes: &[u8], id: &WaveformIdentity) -> Option<Waveform> {
    let mut cursor = bytes;
    let mut take = |n: usize| -> Option<&[u8]> {
        let (head, rest) = cursor.split_at_checked(n)?;
        cursor = rest;
        Some(head)
    };
    if take(4)? != MAGIC || take(1)? != [VERSION] {
        return None;
    }
    let size = u64::from_le_bytes(take(8)?.try_into().ok()?);
    let duration_secs = u32::from_le_bytes(take(4)?.try_into().ok()?);
    let suffix_len = usize::from(*take(1)?.first()?);
    let suffix = take(suffix_len)?;
    if size != id.size || duration_secs != id.duration_secs || suffix != id.suffix.as_bytes() {
        return None;
    }
    let bins = usize::try_from(u32::from_le_bytes(take(4)?.try_into().ok()?)).ok()?;
    let peaks = take(bins)?.to_vec();
    let rms = take(bins)?.to_vec();
    Waveform::new(peaks, rms)
}

/// Keep the cache under `max_bytes`: when it is over, delete the least
/// recently used files (oldest modified time first) until it is under
/// `target_bytes`. Returns how many files went.
pub(super) fn prune(root: &Path, max_bytes: u64, target_bytes: u64) -> usize {
    let mut files: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
    let Ok(servers) = std::fs::read_dir(root) else {
        return 0;
    };
    for server in servers.flatten() {
        let Ok(entries) = std::fs::read_dir(server.path()) else {
            continue;
        };
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata()
                && meta.is_file()
            {
                let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                files.push((modified, meta.len(), entry.path()));
            }
        }
    }
    let mut total: u64 = files.iter().map(|(_, len, _)| len).sum();
    if total <= max_bytes {
        return 0;
    }
    files.sort_by_key(|(modified, _, _)| *modified);
    let mut removed = 0;
    for (_, len, path) in files {
        if total <= target_bytes {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(len);
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> WaveformIdentity {
        WaveformIdentity {
            song_id: "song-1".into(),
            size: 31_337_000,
            duration_secs: 939,
            suffix: "flac".into(),
        }
    }

    fn waveform() -> Waveform {
        Waveform::new(vec![10, 200, 255], vec![5, 100, 180]).unwrap()
    }

    #[test]
    fn stored_waveform_loads_back_for_the_same_audio() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = WaveformCache::for_server(tmp.path(), "http://Music.example:4533/");
        cache.store(&identity(), &waveform()).unwrap();
        assert_eq!(cache.load(&identity()), Some(waveform()));
        // A re-typed URL (case, trailing slash) shares the cache.
        let same = WaveformCache::for_server(tmp.path(), "http://music.example:4533");
        assert_eq!(same.load(&identity()), Some(waveform()));
        // No temp file is left behind.
        let leftovers = std::fs::read_dir(&cache.dir)
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "tmp"))
            .count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn a_replaced_file_on_the_server_is_a_miss() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = WaveformCache::for_server(tmp.path(), "http://server");
        cache.store(&identity(), &waveform()).unwrap();
        for changed in [
            WaveformIdentity {
                size: 1,
                ..identity()
            },
            WaveformIdentity {
                duration_secs: 940,
                ..identity()
            },
            WaveformIdentity {
                suffix: "mp3".into(),
                ..identity()
            },
        ] {
            assert_eq!(cache.load(&changed), None, "{changed:?}");
        }
    }

    #[test]
    fn songs_are_kept_apart_per_server() {
        let tmp = tempfile::tempdir().unwrap();
        WaveformCache::for_server(tmp.path(), "http://a")
            .store(&identity(), &waveform())
            .unwrap();
        assert_eq!(
            WaveformCache::for_server(tmp.path(), "http://b").load(&identity()),
            None
        );
    }

    #[test]
    fn corrupt_or_outdated_files_are_misses() {
        let id = identity();
        let good = encode(&id, &waveform());
        assert!(decode(&good, &id).is_some());
        // Truncated anywhere.
        for cut in [0, 3, 5, 12, good.len() - 1] {
            assert!(decode(&good[..cut], &id).is_none(), "cut at {cut}");
        }
        // Another layout version.
        let mut old = good.clone();
        old[4] = VERSION + 1;
        assert!(decode(&old, &id).is_none());
    }

    #[test]
    fn prune_drops_the_least_recently_used_files_first() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("server");
        std::fs::create_dir_all(&dir).unwrap();
        let base = SystemTime::now() - std::time::Duration::from_secs(1000);
        for (i, name) in ["old", "mid", "new"].iter().enumerate() {
            let path = dir.join(name);
            std::fs::write(&path, vec![0u8; 100]).unwrap();
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(base + std::time::Duration::from_secs(i as u64 * 10))
                .unwrap();
        }
        // Under the cap: nothing goes.
        assert_eq!(prune(tmp.path(), 300, 150), 0);
        // Over the cap: the oldest go until under the target.
        assert_eq!(prune(tmp.path(), 250, 150), 2);
        assert!(dir.join("new").exists());
        assert!(!dir.join("old").exists() && !dir.join("mid").exists());
    }
}
