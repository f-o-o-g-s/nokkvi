//! MPRIS cover-art writer: fetches album bytes once per song change and
//! emits a `file://` URI for `mpris:artUrl`.
//!
//! ## Why this module exists
//!
//! `mpris-server` publishes `mpris:artUrl` on the public session bus, where
//! any same-user process can read it via `dbus-monitor`. Until this module
//! existed, that URL was a `getCoverArt?id=...&u=USER&s=SALT&t=TOKEN` link —
//! the Subsonic credential triple authenticates against Navidrome until the
//! user rotates their password, so leaking it on D-Bus was an account-takeover
//! primitive any sandboxed process could harvest.
//!
//! Borrowed the approach from rmpc (`reference-rmpc/rmpcd/src/mpris/metadata.rs`):
//! fetch the artwork bytes through the authenticated client, write them to a
//! local cache file, then advertise the `file://` URI on D-Bus. Same-key
//! short-circuit avoids re-fetching every 100ms tick.
//!
//! ## Path shape
//!
//! `$XDG_CACHE_HOME/nokkvi/mpris-art-<pid>.jpg` (falling back to
//! `$HOME/.cache/nokkvi/mpris-art-<pid>.jpg`). The `<pid>` suffix matches
//! nokkvi's existing MPRIS bus-name pattern (see `.agent/rules/gotchas.md`
//! — "MPRIS multi-instance bus name") so two simultaneously-running instances
//! don't fight over the same file.
//!
//! ## State management
//!
//! Production calls go through the module-level static `STATE` (a
//! `tokio::sync::Mutex` so the hot path is `await`-friendly). The actual
//! cache logic lives in [`write_art_inner`], which takes the state by
//! `&mut ArtCacheState` so unit tests can construct their own without
//! racing the global — keeping `cargo test` parallel-safe without a
//! `#[serial]` gate.

use std::{
    future::Future,
    path::{Path, PathBuf},
};

use tokio::sync::Mutex;
use tracing::warn;

/// Tracks the most recently-written cache entry so repeat ticks for the
/// same `(server_url, cover_id)` skip the fetch + write.
///
/// The path is included so [`clear`] can remove the file without re-deriving it.
#[derive(Debug, Default)]
pub(crate) struct ArtCacheState {
    last_written: Option<(String, String, PathBuf)>,
}

impl ArtCacheState {
    pub(crate) const fn new() -> Self {
        Self { last_written: None }
    }
}

static STATE: Mutex<ArtCacheState> = Mutex::const_new(ArtCacheState::new());

/// Resolve the per-process MPRIS art cache path:
/// `$XDG_CACHE_HOME/nokkvi/mpris-art-<pid>.jpg` (falls back to
/// `$HOME/.cache/nokkvi/mpris-art-<pid>.jpg`).
///
/// Returns `None` only if neither `$XDG_CACHE_HOME` nor `$HOME` resolves to
/// an absolute path — typically only happens in stripped container/test envs
/// without `HOME` set.
pub(crate) fn cache_file_path() -> Option<PathBuf> {
    let cache_root = resolve_cache_root()?;
    let pid = std::process::id();
    Some(
        cache_root
            .join("nokkvi")
            .join(format!("mpris-art-{pid}.jpg")),
    )
}

/// Resolve the XDG cache root: `$XDG_CACHE_HOME` (if absolute) else
/// `$HOME/.cache`. Matches `directories::BaseDirs::cache_dir()` on Linux
/// (nokkvi is Linux-only) without pulling the dep into the UI crate.
fn resolve_cache_root() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME") {
        let p = PathBuf::from(xdg);
        if p.is_absolute() {
            return Some(p);
        }
    }
    let home = std::env::var_os("HOME")?;
    let p = PathBuf::from(home);
    if !p.is_absolute() {
        return None;
    }
    Some(p.join(".cache"))
}

/// Cache the album art bytes for `(server_url, cover_id)` to a local file and
/// return a `file://` URI suitable for `mpris:artUrl`. The fetcher is only
/// awaited on a cache miss.
///
/// Returns `None` on any failure (path resolution, fetch, write); in that
/// case the caller should pass `None` to MPRIS so the desktop shell simply
/// shows no art instead of crashing or leaking a credentialed URL.
pub(crate) async fn write_art_for_mpris<F>(
    server_url: &str,
    cover_id: &str,
    fetcher: F,
) -> Option<String>
where
    F: Future<Output = anyhow::Result<Vec<u8>>>,
{
    let cache_path = cache_file_path()?;
    let mut state = STATE.lock().await;
    write_art_inner(&mut state, &cache_path, server_url, cover_id, fetcher).await
}

/// Reset the cache state and best-effort remove the cache file. Safe to call
/// from teardown paths (logout, server switch) — missing files are not an error.
///
/// Called from `reset_session_state` on logout / session-expired so server-B's
/// MPRIS metadata doesn't reuse the file written by server-A.
pub(crate) async fn clear() {
    let mut state = STATE.lock().await;
    clear_inner(&mut state, cache_file_path().as_deref());
}

/// Pure-ish core: tests pass their own `ArtCacheState`. Keeps `cargo test`
/// parallel-safe without a `#[serial]` lock around the global.
async fn write_art_inner<F>(
    state: &mut ArtCacheState,
    cache_path: &Path,
    server_url: &str,
    cover_id: &str,
    fetcher: F,
) -> Option<String>
where
    F: Future<Output = anyhow::Result<Vec<u8>>>,
{
    // Fast path: same key as the most recent write — return the cached URI.
    if let Some((prev_server, prev_cover, prev_path)) = &state.last_written
        && prev_server == server_url
        && prev_cover == cover_id
    {
        return Some(path_to_file_uri(prev_path));
    }

    // Cache miss: fetch + write.
    let bytes = match fetcher.await {
        Ok(b) if b.is_empty() => {
            warn!(
                target: "nokkvi::mpris::art",
                server_url, cover_id, "art fetch returned empty body; skipping write"
            );
            return None;
        }
        Ok(b) => b,
        Err(err) => {
            warn!(
                target: "nokkvi::mpris::art",
                server_url, cover_id, %err, "art fetch failed; mpris will show no art"
            );
            return None;
        }
    };

    if let Some(parent) = cache_path.parent()
        && let Err(err) = tokio::fs::create_dir_all(parent).await
    {
        warn!(
            target: "nokkvi::mpris::art",
            path = %parent.display(), %err, "failed to create mpris art cache dir"
        );
        return None;
    }

    if let Err(err) = tokio::fs::write(cache_path, &bytes).await {
        warn!(
            target: "nokkvi::mpris::art",
            path = %cache_path.display(), %err, "failed to write mpris art cache file"
        );
        return None;
    }

    state.last_written = Some((
        server_url.to_string(),
        cover_id.to_string(),
        cache_path.to_path_buf(),
    ));
    Some(path_to_file_uri(cache_path))
}

/// Pure helper for `clear` — tests can drive it without touching the global.
fn clear_inner(state: &mut ArtCacheState, cache_path: Option<&Path>) {
    state.last_written = None;
    if let Some(p) = cache_path
        && let Err(err) = std::fs::remove_file(p)
        && err.kind() != std::io::ErrorKind::NotFound
    {
        warn!(
            target: "nokkvi::mpris::art",
            path = %p.display(), %err, "failed to remove mpris art cache file"
        );
    }
}

fn path_to_file_uri(p: &Path) -> String {
    format!("file://{}", p.display())
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::{
            Arc,
            atomic::{AtomicU32, AtomicU64, Ordering},
        },
    };

    use super::*;

    /// Per-test temp dir under `$TMPDIR` (no `tempfile` dep — not in this
    /// crate's `[dev-dependencies]`). Each call returns a fresh
    /// `nokkvi-mpris-art-test-<pid>-<counter>/` directory and a Drop guard
    /// that removes it recursively on scope exit.
    struct ScratchDir {
        path: PathBuf,
    }

    impl ScratchDir {
        fn new() -> Self {
            static SEQ: AtomicU64 = AtomicU64::new(0);
            let seq = SEQ.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "nokkvi-mpris-art-test-{}-{}",
                std::process::id(),
                seq
            ));
            std::fs::create_dir_all(&path).expect("create scratch dir");
            Self { path }
        }

        fn path(&self) -> &std::path::Path {
            &self.path
        }
    }

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn cache_file_path_contains_nokkvi_and_pid() {
        // Tests run with the user's real env, so HOME (or XDG_CACHE_HOME) is
        // always present in practice — just assert the shape if it resolves.
        let path = cache_file_path().expect("cache path resolves in test env");
        let s = path.to_string_lossy();
        assert!(
            s.contains("nokkvi/mpris-art-"),
            "path should contain 'nokkvi/mpris-art-', got: {s}"
        );
        let pid = std::process::id();
        assert!(
            s.contains(&format!("mpris-art-{pid}.jpg")),
            "path should embed the current pid {pid}, got: {s}"
        );
    }

    #[tokio::test]
    async fn write_then_read_cycle_persists_bytes() {
        let dir = ScratchDir::new();
        let cache_path = dir.path().join("mpris-art-test.jpg");
        let payload: Vec<u8> = vec![0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3, 4];

        let mut state = ArtCacheState::new();
        let payload_clone = payload.clone();
        let uri = write_art_inner(
            &mut state,
            &cache_path,
            "https://server.example",
            "al-abc",
            async move { Ok(payload_clone) },
        )
        .await
        .expect("write should succeed and return a uri");

        assert!(
            uri.starts_with("file://"),
            "expected file:// uri, got: {uri}"
        );
        assert!(
            uri.contains(&cache_path.display().to_string()),
            "uri should reference the cache path"
        );

        let on_disk = tokio::fs::read(&cache_path).await.unwrap();
        assert_eq!(on_disk, payload, "written bytes must match payload");
    }

    /// Builds an async fetcher that bumps `counter` on each `.await` and returns
    /// `payload`. Constructing it must NOT bump the counter — the assertion
    /// is "the fetcher was awaited", not "the future was constructed".
    fn counting_fetcher(
        counter: &Arc<AtomicU32>,
        payload: Vec<u8>,
    ) -> impl Future<Output = anyhow::Result<Vec<u8>>> + use<> {
        let c = Arc::clone(counter);
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Ok(payload)
        }
    }

    #[tokio::test]
    async fn skip_on_same_key_does_not_refetch() {
        let dir = ScratchDir::new();
        let cache_path = dir.path().join("mpris-art-test.jpg");
        let payload: Vec<u8> = vec![1, 2, 3];

        let counter = Arc::new(AtomicU32::new(0));

        let mut state = ArtCacheState::new();
        let first = write_art_inner(
            &mut state,
            &cache_path,
            "https://server.example",
            "al-abc",
            counting_fetcher(&counter, payload.clone()),
        )
        .await;
        assert!(first.is_some(), "first call should write");
        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "first call must invoke the fetcher exactly once"
        );

        let second = write_art_inner(
            &mut state,
            &cache_path,
            "https://server.example",
            "al-abc",
            counting_fetcher(&counter, payload.clone()),
        )
        .await;
        assert_eq!(
            first, second,
            "same-key second call must return the cached uri"
        );
        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "same-key second call must NOT invoke the fetcher again"
        );
    }

    #[tokio::test]
    async fn distinct_key_triggers_refetch() {
        // Guards against a regression where the equality check is dropped.
        let dir = ScratchDir::new();
        let cache_path = dir.path().join("mpris-art-test.jpg");
        let counter = Arc::new(AtomicU32::new(0));

        let mut state = ArtCacheState::new();
        let _ = write_art_inner(
            &mut state,
            &cache_path,
            "https://server-a.example",
            "al-abc",
            counting_fetcher(&counter, vec![9, 9, 9]),
        )
        .await;
        // Different server_url — same cover_id collision shouldn't replay the cache.
        let _ = write_art_inner(
            &mut state,
            &cache_path,
            "https://server-b.example",
            "al-abc",
            counting_fetcher(&counter, vec![9, 9, 9]),
        )
        .await;
        assert_eq!(
            counter.load(Ordering::SeqCst),
            2,
            "server switch must force a refetch even with the same cover_id"
        );
    }

    #[tokio::test]
    async fn clear_resets_state_and_next_call_rewrites() {
        let dir = ScratchDir::new();
        let cache_path = dir.path().join("mpris-art-test.jpg");
        let counter = Arc::new(AtomicU32::new(0));

        let mut state = ArtCacheState::new();
        let _ = write_art_inner(
            &mut state,
            &cache_path,
            "https://server.example",
            "al-abc",
            counting_fetcher(&counter, vec![7, 7, 7]),
        )
        .await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        clear_inner(&mut state, Some(&cache_path));
        assert!(state.last_written.is_none(), "clear must reset state");
        assert!(
            !cache_path.exists(),
            "clear should remove the cache file when present"
        );

        // Same key after clear: must re-invoke the fetcher.
        let _ = write_art_inner(
            &mut state,
            &cache_path,
            "https://server.example",
            "al-abc",
            counting_fetcher(&counter, vec![7, 7, 7]),
        )
        .await;
        assert_eq!(
            counter.load(Ordering::SeqCst),
            2,
            "after clear, same key must trigger a fresh fetch"
        );
    }

    #[test]
    fn clear_inner_tolerates_missing_file() {
        let dir = ScratchDir::new();
        let cache_path = dir.path().join("not-here.jpg");
        let mut state = ArtCacheState::new();
        // Should not panic / log anything user-actionable when the file isn't there.
        clear_inner(&mut state, Some(&cache_path));
        assert!(state.last_written.is_none());
    }

    #[tokio::test]
    async fn fetch_error_returns_none_and_leaves_state_clean() {
        let dir = ScratchDir::new();
        let cache_path = dir.path().join("mpris-art-test.jpg");
        let mut state = ArtCacheState::new();

        let result = write_art_inner(
            &mut state,
            &cache_path,
            "https://server.example",
            "al-fail",
            async { Err(anyhow::anyhow!("simulated fetch failure")) },
        )
        .await;

        assert!(result.is_none(), "fetch error must yield None");
        assert!(
            state.last_written.is_none(),
            "failed fetch must not poison the cache state"
        );
        assert!(
            !cache_path.exists(),
            "failed fetch must not leave a cache file behind"
        );
    }

    #[tokio::test]
    async fn empty_fetch_body_returns_none() {
        let dir = ScratchDir::new();
        let cache_path = dir.path().join("mpris-art-test.jpg");
        let mut state = ArtCacheState::new();

        let result = write_art_inner(
            &mut state,
            &cache_path,
            "https://server.example",
            "al-empty",
            async { Ok(Vec::new()) },
        )
        .await;

        assert!(result.is_none(), "empty body must not be cached");
        assert!(state.last_written.is_none());
        assert!(!cache_path.exists());
    }
}
