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
//! Approach borrowed from rmpc (`reference-rmpc/rmpcd/src/mpris/metadata.rs`):
//! fetch the artwork bytes through the authenticated client, write them to a
//! local cache file, then advertise the `file://` URI on D-Bus. The
//! `(server_url, cover_id)` short-circuit avoids re-fetching every 100ms tick.
//!
//! ## Path shape
//!
//! `$XDG_CACHE_HOME/nokkvi/mpris-art-<pid>-<cover_id>.jpg` (falling back to
//! `$HOME/.cache/nokkvi/...`). The `<pid>` suffix matches nokkvi's existing
//! MPRIS bus-name pattern (see `.claude/rules/gotchas.md` — "MPRIS multi-instance
//! bus name") so two simultaneously-running instances don't fight over the same
//! file. The `<cover_id>` suffix makes the URI unique per track — desktop
//! shells (Plasma, GNOME, dunst, waybar) key their `mpris:artUrl` image cache
//! off the URL string, so reusing one filename across tracks pins them on the
//! first track's art for the whole session. After each successor write the
//! previous file is removed best-effort to keep the per-PID footprint at ~1
//! file in steady state.
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

/// Filename-safe truncation cap for the sanitized cover-id suffix. Subsonic
/// cover ids are typically <40 chars; the cap guards against pathological
/// inputs blowing past `NAME_MAX` (255 on ext4/btrfs) once the
/// `mpris-art-<pid>-` prefix and `.jpg` extension are factored in.
const SANITIZED_COVER_ID_MAX_LEN: usize = 80;

/// Tracks the most recently-written cache entry so repeat ticks for the
/// same `(server_url, cover_id)` skip the fetch + write.
///
/// The path is included so the next write can remove the file it superseded
/// without re-deriving the previous filename.
#[derive(Debug, Default)]
pub(crate) struct ArtCacheState {
    last_written: Option<(String, String, PathBuf)>,
    /// Last `(server_url, cover_id)` whose fetch failed (no art, non-image
    /// body, or fetch error). `handle_tick` re-runs `write_art_for_mpris` every
    /// ~100ms with the current track's cover_id; without this a cover the
    /// server can't resolve would be re-fetched — and its credentialed
    /// getCoverArt URL re-logged — on every tick. Recording the failed key
    /// bounds it to one attempt per song, mirroring the `last_written` success
    /// fast-path. Cleared on any success and on `clear()`.
    ///
    /// Unlike the UI mini-cover negative cache (which records ONLY deterministic
    /// "not found" misses and lets a transient drop retry on the next scroll),
    /// this deliberately records ANY failure — including a transient throttle.
    /// On the 100ms tick, NOT caching a transient would re-issue the 3-retry
    /// fetch every tick and re-storm an already-throttled server; the accepted
    /// cost is a one-song blank on a transient drop, which self-heals at the next
    /// track change (a fresh cover_id falls through). That trade favours the
    /// server over a one-song cosmetic gap.
    last_failed: Option<(String, String)>,
}

impl ArtCacheState {
    pub(crate) const fn new() -> Self {
        Self {
            last_written: None,
            last_failed: None,
        }
    }
}

static STATE: Mutex<ArtCacheState> = Mutex::const_new(ArtCacheState::new());

/// Resolve the MPRIS art cache directory: `$XDG_CACHE_HOME/nokkvi/` (falls
/// back to `$HOME/.cache/nokkvi/`).
///
/// Returns `None` only if neither `$XDG_CACHE_HOME` nor `$HOME` resolves to
/// an absolute path — typically only in stripped container/test envs without
/// `HOME` set.
fn cache_dir_path() -> Option<PathBuf> {
    Some(resolve_cache_root()?.join("nokkvi"))
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

/// Per-track cache path: `<dir>/mpris-art-<pid>-<sanitized_cover_id>.jpg`.
fn cache_file_path_for(cache_dir: &Path, cover_id: &str) -> PathBuf {
    let pid = std::process::id();
    let safe = sanitize_cover_id(cover_id);
    cache_dir.join(format!("mpris-art-{pid}-{safe}.jpg"))
}

/// Replace anything that isn't `[A-Za-z0-9._-]` with `_`, cap to
/// [`SANITIZED_COVER_ID_MAX_LEN`], and emit `_` for an empty input. Subsonic
/// cover ids are already filename-safe in practice; this is a defensive
/// belt against future server quirks or non-Navidrome backends.
fn sanitize_cover_id(cover_id: &str) -> String {
    let mut out: String = cover_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.len() > SANITIZED_COVER_ID_MAX_LEN {
        out.truncate(SANITIZED_COVER_ID_MAX_LEN);
    }
    if out.is_empty() {
        out.push('_');
    }
    out
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
    let cache_dir = cache_dir_path()?;
    let mut state = STATE.lock().await;
    write_art_inner(&mut state, &cache_dir, server_url, cover_id, fetcher).await
}

/// Reset the cache state and best-effort remove every per-PID cache file for
/// this process. Safe to call from teardown paths (logout, server switch) —
/// missing files are not an error.
///
/// Called from `reset_session_state` on logout / session-expired so server-B's
/// MPRIS metadata doesn't reuse the bytes server-A wrote, and so the cache
/// dir doesn't accumulate every album the user played pre-logout.
pub(crate) async fn clear() {
    let mut state = STATE.lock().await;
    clear_inner(&mut state, cache_dir_path().as_deref()).await;
}

/// Boot-time best-effort sweep of `mpris-art-<pid>[-...].jpg` files whose
/// `<pid>` is no longer alive on this system. Covers two leak vectors that
/// per-write cleanup misses:
///   1. The previous nokkvi run was killed / crashed mid-track and never
///      went through `clear()`, leaving its current-track file behind.
///   2. Pre-NF2 sessions wrote `mpris-art-<pid>.jpg` (no cover suffix); this
///      sweep parses that legacy shape too so the dir collapses to "current
///      process + any other live nokkvi instance" on the next launch.
///
/// Live PIDs (current process, other running nokkvi instances) are preserved.
/// Files for unrelated processes that happen to be alive at the same PID are
/// also preserved — PID reuse is rare enough on Linux (32-bit PID space) that
/// the false-negative is acceptable; the alternative would require parsing
/// `/proc/<pid>/comm` and risks tearing down a sibling nokkvi instance's
/// cache if the comm check ever misclassifies.
pub(crate) async fn sweep_dead_pid_files() {
    let Some(dir) = cache_dir_path() else { return };
    sweep_dead_pid_files_in(&dir).await;
}

/// Pure-ish core: tests pass a scratch dir so the suite stays parallel-safe
/// without touching the real `$XDG_CACHE_HOME`.
async fn sweep_dead_pid_files_in(dir: &Path) {
    sweep_dir_where(dir, "dead-pid-sweep", |name| {
        parse_pid_from_filename(name).is_some_and(|pid| !pid_is_alive(pid))
    })
    .await;
}

/// Extract the `<pid>` portion from either filename shape:
///   - `mpris-art-<pid>.jpg`             (pre-NF2 legacy)
///   - `mpris-art-<pid>-<cover_id>.jpg`  (post-fix)
///
/// Returns `None` if the prefix / extension don't match or the pid segment
/// doesn't parse as a `u32`.
fn parse_pid_from_filename(name: &str) -> Option<u32> {
    let stripped = name.strip_prefix("mpris-art-")?.strip_suffix(".jpg")?;
    let pid_str = stripped
        .split_once('-')
        .map_or(stripped, |(pid, _rest)| pid);
    pid_str.parse::<u32>().ok()
}

/// Linux-only liveness probe: `/proc/<pid>` exists iff the kernel knows the
/// pid. nokkvi is Linux-only (PipeWire / ksni / CLAUDE.md), so we lean on
/// procfs directly rather than pulling `nix` or `libc` into the UI crate.
fn pid_is_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

/// Shared directory-sweep skeleton for the two cache cleanups
/// (`sweep_dead_pid_files_in` / `clear_inner`): enumerate `dir`, remove every
/// entry whose filename satisfies `should_remove`, tolerate a missing
/// directory and already-deleted files, and warn (never fail) on anything
/// else. `op` tags the warn lines with the calling sweep.
async fn sweep_dir_where(dir: &Path, op: &'static str, should_remove: impl Fn(&str) -> bool) {
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(e) => e,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
        Err(err) => {
            warn!(
                target: "nokkvi::mpris::art",
                path = %dir.display(), %err, op, "failed to enumerate mpris art cache dir"
            );
            return;
        }
    };

    loop {
        match entries.next_entry().await {
            Ok(Some(entry)) => {
                let name = entry.file_name();
                let Some(name_str) = name.to_str() else {
                    continue;
                };
                if !should_remove(name_str) {
                    continue;
                }
                if let Err(err) = tokio::fs::remove_file(entry.path()).await
                    && err.kind() != std::io::ErrorKind::NotFound
                {
                    warn!(
                        target: "nokkvi::mpris::art",
                        path = %entry.path().display(), %err, op, "failed to remove mpris art cache file"
                    );
                }
            }
            Ok(None) => break,
            Err(err) => {
                warn!(
                    target: "nokkvi::mpris::art",
                    path = %dir.display(), %err, op, "error iterating mpris art cache dir"
                );
                break;
            }
        }
    }
}

/// Pure-ish core: tests pass their own `ArtCacheState` and `cache_dir`. Keeps
/// `cargo test` parallel-safe without a `#[serial]` lock around the global.
async fn write_art_inner<F>(
    state: &mut ArtCacheState,
    cache_dir: &Path,
    server_url: &str,
    cover_id: &str,
    fetcher: F,
) -> Option<String>
where
    F: Future<Output = anyhow::Result<Vec<u8>>>,
{
    let new_path = cache_file_path_for(cache_dir, cover_id);

    // Fast path: same key as the most recent write — return the cached URI
    // without re-fetching or re-writing. We trust `prev_path` still exists
    // on disk; if it was externally deleted MPRIS shows no art for one tick
    // and the next track change rewrites.
    if let Some((prev_server, prev_cover, prev_path)) = &state.last_written
        && prev_server == server_url
        && prev_cover == cover_id
    {
        return Some(path_to_file_uri(prev_path));
    }

    // Negative fast-path: this exact key already failed for the current song
    // (no art, non-image body, or fetch error). Skip the re-fetch — handle_tick
    // calls us every ~100ms, so without this a server-unresolvable cover would
    // be re-fetched (and its credentialed getCoverArt URL re-logged) on every
    // tick for the track's whole duration. The next track change carries a new
    // cover_id and falls through to a fresh attempt.
    if let Some((failed_server, failed_cover)) = &state.last_failed
        && failed_server == server_url
        && failed_cover == cover_id
    {
        return None;
    }

    let bytes = match fetcher.await {
        Ok(b) if b.is_empty() => {
            warn!(
                target: "nokkvi::mpris::art",
                server_url, cover_id, "art fetch returned empty body; skipping write"
            );
            state.last_failed = Some((server_url.to_string(), cover_id.to_string()));
            return None;
        }
        Ok(b) => b,
        Err(err) => {
            warn!(
                target: "nokkvi::mpris::art",
                server_url, cover_id, %err, "art fetch failed; mpris will show no art"
            );
            state.last_failed = Some((server_url.to_string(), cover_id.to_string()));
            return None;
        }
    };

    if let Some(parent) = new_path.parent()
        && let Err(err) = tokio::fs::create_dir_all(parent).await
    {
        warn!(
            target: "nokkvi::mpris::art",
            path = %parent.display(), %err, "failed to create mpris art cache dir"
        );
        return None;
    }

    if let Err(err) = tokio::fs::write(&new_path, &bytes).await {
        warn!(
            target: "nokkvi::mpris::art",
            path = %new_path.display(), %err, "failed to write mpris art cache file"
        );
        return None;
    }

    // Capture the path being superseded BEFORE updating state. Updating state
    // first ensures any concurrent reader (after we drop the lock) sees the
    // new entry — but since callers serialize on `STATE`, this is just defensive.
    let prev_to_delete = state.last_written.take().map(|(_, _, p)| p);
    state.last_written = Some((
        server_url.to_string(),
        cover_id.to_string(),
        new_path.clone(),
    ));
    state.last_failed = None;

    if let Some(prev) = prev_to_delete
        && prev != new_path
    {
        // Best-effort: by the time we get here the new file is already on
        // disk, so MPRIS clients responding to the next PropertiesChanged
        // signal will load `new_path`. The shell has already cached `prev`'s
        // bytes in memory from the previous track change, so removing the
        // disk file doesn't affect what they display. Errors are ignored —
        // the file is at most ~1 MB and orphans are swept on `clear()` /
        // process exit will leave them for the OS tmp cleaner.
        if let Err(err) = tokio::fs::remove_file(&prev).await
            && err.kind() != std::io::ErrorKind::NotFound
        {
            warn!(
                target: "nokkvi::mpris::art",
                path = %prev.display(), %err, "failed to remove superseded mpris art cache file"
            );
        }
    }

    Some(path_to_file_uri(&new_path))
}

/// Reset state and best-effort sweep every `mpris-art-<pid>-*.jpg` file in
/// `cache_dir` for the current process. Tests can drive this with a scratch
/// dir without touching the module-level static.
async fn clear_inner(state: &mut ArtCacheState, cache_dir: Option<&Path>) {
    state.last_written = None;
    state.last_failed = None;
    let Some(dir) = cache_dir else { return };
    let pid = std::process::id();
    let prefix = format!("mpris-art-{pid}-");
    sweep_dir_where(dir, "clear", |name| {
        name.starts_with(&prefix) && name.ends_with(".jpg")
    })
    .await;
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
    fn cache_file_path_for_contains_nokkvi_pid_and_cover_id() {
        let dir = std::path::PathBuf::from("/tmp/scratch/nokkvi");
        let path = cache_file_path_for(&dir, "al-abc123");
        let s = path.to_string_lossy();
        let pid = std::process::id();
        assert!(
            s.ends_with(&format!("mpris-art-{pid}-al-abc123.jpg")),
            "path should end with 'mpris-art-<pid>-<cover>.jpg', got: {s}"
        );
        assert!(
            s.starts_with("/tmp/scratch/nokkvi/"),
            "path should sit in the provided dir, got: {s}"
        );
    }

    #[test]
    fn sanitize_cover_id_keeps_safe_chars() {
        assert_eq!(sanitize_cover_id("al-abc_123.foo"), "al-abc_123.foo");
        assert_eq!(sanitize_cover_id("ABCxyz09"), "ABCxyz09");
    }

    #[test]
    fn sanitize_cover_id_replaces_path_separators_and_whitespace() {
        // Defensive: Subsonic ids are typically `[A-Za-z0-9-]`, but if a
        // backend ever returns slashes / spaces / colons the sanitizer must
        // collapse them so we don't escape the cache dir or break the format.
        assert_eq!(sanitize_cover_id("al/abc:def"), "al_abc_def");
        assert_eq!(sanitize_cover_id("a b\tc\nd"), "a_b_c_d");
        assert_eq!(sanitize_cover_id("../etc/passwd"), ".._etc_passwd");
    }

    #[test]
    fn sanitize_cover_id_truncates_overlong_input() {
        let long = "a".repeat(500);
        let out = sanitize_cover_id(&long);
        assert!(
            out.len() <= SANITIZED_COVER_ID_MAX_LEN,
            "sanitize must cap output to {} chars, got {}",
            SANITIZED_COVER_ID_MAX_LEN,
            out.len()
        );
    }

    #[test]
    fn sanitize_cover_id_handles_empty_input() {
        assert_eq!(sanitize_cover_id(""), "_");
    }

    #[tokio::test]
    async fn write_then_read_cycle_persists_bytes() {
        let dir = ScratchDir::new();
        let payload: Vec<u8> = vec![0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3, 4];

        let mut state = ArtCacheState::new();
        let payload_clone = payload.clone();
        let uri = write_art_inner(
            &mut state,
            dir.path(),
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
        let expected_path = cache_file_path_for(dir.path(), "al-abc");
        assert!(
            uri.contains(&expected_path.display().to_string()),
            "uri {uri} should reference {}",
            expected_path.display()
        );

        let on_disk = tokio::fs::read(&expected_path).await.unwrap();
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
        let payload: Vec<u8> = vec![1, 2, 3];
        let counter = Arc::new(AtomicU32::new(0));

        let mut state = ArtCacheState::new();
        let first = write_art_inner(
            &mut state,
            dir.path(),
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
            dir.path(),
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
    async fn distinct_server_url_triggers_refetch_even_with_same_cover_id() {
        let dir = ScratchDir::new();
        let counter = Arc::new(AtomicU32::new(0));

        let mut state = ArtCacheState::new();
        let _ = write_art_inner(
            &mut state,
            dir.path(),
            "https://server-a.example",
            "al-abc",
            counting_fetcher(&counter, vec![9, 9, 9]),
        )
        .await;
        let _ = write_art_inner(
            &mut state,
            dir.path(),
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
    async fn distinct_cover_ids_produce_distinct_uris() {
        // Regression test for the "MPRIS shows stale album art across track
        // changes" bug. Desktop shells (Plasma, GNOME Shell, dunst, waybar,
        // playerctl consumers) key their `mpris:artUrl` image cache off the
        // URL string. If two consecutive writes for different cover_ids
        // collapse to the same `file://` URI, every subsequent track keeps
        // showing the first track's artwork until the player is restarted.
        let dir = ScratchDir::new();
        let mut state = ArtCacheState::new();

        let uri_a = write_art_inner(
            &mut state,
            dir.path(),
            "https://server.example",
            "al-aaa",
            async { Ok(vec![1, 2, 3]) },
        )
        .await
        .expect("first write returns a uri");

        let uri_b = write_art_inner(
            &mut state,
            dir.path(),
            "https://server.example",
            "al-bbb",
            async { Ok(vec![4, 5, 6]) },
        )
        .await
        .expect("second write returns a uri");

        assert_ne!(
            uri_a, uri_b,
            "distinct cover_ids must produce distinct file:// URIs so MPRIS \
             clients invalidate their per-URL image cache"
        );
    }

    #[tokio::test]
    async fn successor_write_removes_previous_cache_file() {
        // Steady-state per-PID footprint should be a single cache file. After
        // a track change the previous file is best-effort removed; tests
        // assert the eviction so we don't silently regress to "1 file per
        // distinct album the user ever played in this session".
        let dir = ScratchDir::new();
        let mut state = ArtCacheState::new();

        let _ = write_art_inner(
            &mut state,
            dir.path(),
            "https://server.example",
            "al-aaa",
            async { Ok(vec![1, 2, 3]) },
        )
        .await
        .expect("first write");
        let path_a = cache_file_path_for(dir.path(), "al-aaa");
        assert!(path_a.exists(), "first write should create file A");

        let _ = write_art_inner(
            &mut state,
            dir.path(),
            "https://server.example",
            "al-bbb",
            async { Ok(vec![4, 5, 6]) },
        )
        .await
        .expect("second write");
        let path_b = cache_file_path_for(dir.path(), "al-bbb");
        assert!(path_b.exists(), "second write should create file B");
        assert!(
            !path_a.exists(),
            "successor write should remove the previous cache file"
        );
    }

    #[tokio::test]
    async fn same_key_repeated_keeps_one_file_and_no_extra_writes() {
        // Repeated 100ms ticks for the same track must not churn the file.
        let dir = ScratchDir::new();
        let mut state = ArtCacheState::new();
        let counter = Arc::new(AtomicU32::new(0));

        for _ in 0..5 {
            let _ = write_art_inner(
                &mut state,
                dir.path(),
                "https://server.example",
                "al-abc",
                counting_fetcher(&counter, vec![7, 7, 7]),
            )
            .await
            .expect("write");
        }

        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "same-key repeated calls must only fetch once"
        );
        // Only the single cache file for `al-abc` should exist.
        let mut count = 0;
        let mut entries = tokio::fs::read_dir(dir.path()).await.unwrap();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            if let Some(name) = entry.file_name().to_str()
                && name.starts_with("mpris-art-")
                && name.ends_with(".jpg")
            {
                count += 1;
            }
        }
        assert_eq!(
            count, 1,
            "steady state for one track must leave exactly one cache file"
        );
    }

    #[tokio::test]
    async fn clear_inner_resets_state_and_sweeps_only_current_pid_files() {
        let dir = ScratchDir::new();
        let pid = std::process::id();

        let mine = [
            format!("mpris-art-{pid}-al-aaa.jpg"),
            format!("mpris-art-{pid}-al-bbb.jpg"),
        ];
        let other = [
            format!("mpris-art-{}-al-xxx.jpg", pid.wrapping_add(1)),
            "mpris-art-other-pid-al-yyy.jpg".to_string(),
            "unrelated.txt".to_string(),
        ];
        for f in mine.iter().chain(other.iter()) {
            std::fs::write(dir.path().join(f), b"x").unwrap();
        }

        let mut state = ArtCacheState::new();
        state.last_written = Some((
            "https://server.example".to_string(),
            "al-aaa".to_string(),
            dir.path().join(&mine[0]),
        ));

        clear_inner(&mut state, Some(dir.path())).await;

        assert!(state.last_written.is_none(), "clear must reset state");
        for f in &mine {
            assert!(
                !dir.path().join(f).exists(),
                "clear should sweep current-PID file {f}"
            );
        }
        for f in &other {
            assert!(
                dir.path().join(f).exists(),
                "clear must not touch unrelated file {f}"
            );
        }
    }

    #[tokio::test]
    async fn clear_inner_after_write_then_reuse_refetches() {
        // After a session reset the next call with the same key must rewrite.
        let dir = ScratchDir::new();
        let counter = Arc::new(AtomicU32::new(0));

        let mut state = ArtCacheState::new();
        let _ = write_art_inner(
            &mut state,
            dir.path(),
            "https://server.example",
            "al-abc",
            counting_fetcher(&counter, vec![7, 7, 7]),
        )
        .await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        clear_inner(&mut state, Some(dir.path())).await;
        assert!(state.last_written.is_none());
        assert!(
            !cache_file_path_for(dir.path(), "al-abc").exists(),
            "clear should remove the cache file"
        );

        let _ = write_art_inner(
            &mut state,
            dir.path(),
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

    #[tokio::test]
    async fn clear_inner_tolerates_missing_directory() {
        let mut state = ArtCacheState::new();
        let nonexistent = std::env::temp_dir().join(format!(
            "nokkvi-mpris-art-clear-missing-{}-{}",
            std::process::id(),
            42_u64
        ));
        clear_inner(&mut state, Some(&nonexistent)).await;
        assert!(state.last_written.is_none());
    }

    #[tokio::test]
    async fn fetch_error_returns_none_and_leaves_state_clean() {
        let dir = ScratchDir::new();
        let mut state = ArtCacheState::new();

        let result = write_art_inner(
            &mut state,
            dir.path(),
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
            !cache_file_path_for(dir.path(), "al-fail").exists(),
            "failed fetch must not leave a cache file behind"
        );
    }

    #[tokio::test]
    async fn failed_cover_is_negative_cached_to_avoid_per_tick_refetch() {
        // handle_tick calls write_art_for_mpris every ~100ms with the current
        // track's cover_id. A cover the server can't resolve must be attempted
        // at most once per song, not re-fetched (and re-logged) on every tick.
        let dir = ScratchDir::new();
        let mut state = ArtCacheState::new();
        let calls = Arc::new(AtomicU32::new(0));

        for _ in 0..4 {
            let c = Arc::clone(&calls);
            let result = write_art_inner(
                &mut state,
                dir.path(),
                "https://server.example",
                "al-missing",
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Err(anyhow::anyhow!("artwork response was not an image"))
                },
            )
            .await;
            assert!(result.is_none(), "a failing cover must yield no art");
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "a known-failing cover must be fetched once per song, not every tick"
        );

        // A different cover is a different key — it must still be attempted.
        let c = Arc::clone(&calls);
        let _ = write_art_inner(
            &mut state,
            dir.path(),
            "https://server.example",
            "al-other",
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Err(anyhow::anyhow!("artwork response was not an image"))
            },
        )
        .await;
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "a different cover must not be suppressed by another key's negative-cache entry"
        );
    }

    #[test]
    fn parse_pid_from_filename_extracts_pid_for_legacy_format() {
        // Pre-NF2 shape: `mpris-art-<pid>.jpg`. Boot sweep must still
        // recognise these so the 17-orphan pile users carry forward gets
        // collapsed on first launch of the fixed binary.
        assert_eq!(parse_pid_from_filename("mpris-art-12345.jpg"), Some(12345));
    }

    #[test]
    fn parse_pid_from_filename_extracts_pid_for_per_cover_format() {
        assert_eq!(
            parse_pid_from_filename("mpris-art-3785659-3utkWH4Dfq9cvWQ2EIcQ1e.jpg"),
            Some(3_785_659)
        );
        assert_eq!(parse_pid_from_filename("mpris-art-1-al-abc.jpg"), Some(1));
    }

    #[test]
    fn parse_pid_from_filename_rejects_non_matching() {
        assert_eq!(parse_pid_from_filename("unrelated.jpg"), None);
        assert_eq!(parse_pid_from_filename("mpris-art-.jpg"), None);
        assert_eq!(parse_pid_from_filename("mpris-art-notanumber.jpg"), None);
        assert_eq!(parse_pid_from_filename("mpris-art-12345.png"), None);
        assert_eq!(parse_pid_from_filename("mpris-art-12345"), None);
    }

    #[test]
    fn pid_is_alive_true_for_self_and_pid_1() {
        // The test binary is alive by definition; init (pid 1) always exists
        // on Linux. nokkvi is Linux-only so both invariants hold in CI.
        assert!(pid_is_alive(std::process::id()));
        assert!(pid_is_alive(1));
    }

    #[test]
    fn pid_is_alive_false_for_definitely_dead_pid() {
        // Linux kernel.pid_max is at most 2^22 (4_194_304); u32::MAX is far
        // above any reachable PID, so /proc/4294967295 can never exist.
        assert!(!pid_is_alive(u32::MAX));
    }

    #[tokio::test]
    async fn sweep_removes_only_dead_pid_art_files() {
        let dir = ScratchDir::new();
        let my_pid = std::process::id();
        let dead_pid = u32::MAX;

        let dead_legacy = format!("mpris-art-{dead_pid}.jpg");
        let dead_per_cover = format!("mpris-art-{dead_pid}-al-zzz.jpg");
        let alive_other = "mpris-art-1-al-xyz.jpg".to_string();
        let current_self = format!("mpris-art-{my_pid}-al-mine.jpg");
        let wrong_ext = format!("mpris-art-{dead_pid}.png");
        let unrelated = "something-else.jpg".to_string();

        for f in [
            &dead_legacy,
            &dead_per_cover,
            &alive_other,
            &current_self,
            &wrong_ext,
            &unrelated,
        ] {
            std::fs::write(dir.path().join(f), b"x").unwrap();
        }

        sweep_dead_pid_files_in(dir.path()).await;

        assert!(
            !dir.path().join(&dead_legacy).exists(),
            "dead-pid legacy file should be swept"
        );
        assert!(
            !dir.path().join(&dead_per_cover).exists(),
            "dead-pid per-cover file should be swept"
        );
        assert!(
            dir.path().join(&alive_other).exists(),
            "alive other-pid file (pid 1 = init) must be preserved"
        );
        assert!(
            dir.path().join(&current_self).exists(),
            "current-process file must be preserved"
        );
        assert!(
            dir.path().join(&wrong_ext).exists(),
            "wrong-extension file must be preserved"
        );
        assert!(
            dir.path().join(&unrelated).exists(),
            "unrelated file must be preserved"
        );
    }

    #[tokio::test]
    async fn sweep_tolerates_missing_directory() {
        let nonexistent = std::env::temp_dir().join(format!(
            "nokkvi-mpris-art-sweep-missing-{}-{}",
            std::process::id(),
            17_u64
        ));
        // Should return cleanly without panicking or logging an error path.
        sweep_dead_pid_files_in(&nonexistent).await;
    }

    #[tokio::test]
    async fn empty_fetch_body_returns_none() {
        let dir = ScratchDir::new();
        let mut state = ArtCacheState::new();

        let result = write_art_inner(
            &mut state,
            dir.path(),
            "https://server.example",
            "al-empty",
            async { Ok(Vec::new()) },
        )
        .await;

        assert!(result.is_none(), "empty body must not be cached");
        assert!(state.last_written.is_none());
        assert!(!cache_file_path_for(dir.path(), "al-empty").exists());
    }
}
