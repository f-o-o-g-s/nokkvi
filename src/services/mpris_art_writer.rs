//! MPRIS art writer: decides what `mpris:artUrl` shows, and turns server
//! cover ids into local `file://` URIs.
//!
//! ## What gets published
//!
//! While a song or a radio station is current and the cache dir is writable,
//! `mpris:artUrl` is always present and always describes that item. Desktop shells keep the previous
//! image when the key is absent, which used to pin the last queue song's
//! cover over a radio station. [`art_candidates`] holds the order:
//!
//! - song: its cover (fetched once per song change), else the placeholder;
//! - radio: the station's uploaded logo, else the stream's ICY `StreamUrl`
//!   when it is http(s), published verbatim and never fetched here, else the
//!   placeholder.
//!
//! The placeholder is the app icon, written once per process. "Not Playing"
//! publishes no art. With no cache dir, or while writes to it fail, only a
//! stream URL can be published; a failed cover or placeholder is retried
//! after a minute, never on every tick.
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
//! `$XDG_CACHE_HOME/nokkvi/mpris-art-<pid>-<cover_id>_<hash>.jpg` (falling back
//! to `$HOME/.cache/nokkvi/...`). The `<pid>` suffix matches nokkvi's existing
//! MPRIS bus-name pattern (see `.claude/rules/gotchas.md` — "MPRIS multi-instance
//! bus name") so two simultaneously-running instances don't fight over the same
//! file. The `<cover_id>` suffix makes the URI unique per track — desktop
//! shells (Plasma, GNOME, dunst, waybar) key their `mpris:artUrl` image cache
//! off the URL string, so reusing one filename across tracks pins them on the
//! first track's art for the whole session. After each successor write the
//! previous file is removed best-effort to keep the per-PID footprint at ~1
//! file in steady state. The placeholder lives beside it as
//! `mpris-art-<pid>-placeholder.jpg`, which no cover id can name.
//!
//! ## State management
//!
//! Production calls go through the module-level static `STATE` (a
//! `tokio::sync::Mutex` so the hot path is `await`-friendly). The actual
//! logic lives in [`resolve_art_inner`] (which art to publish) and
//! [`write_art_inner`] (the cache), which take the state by
//! `&mut ArtCacheState` so unit tests can construct their own without
//! racing the global — keeping `cargo test` parallel-safe without a
//! `#[serial]` gate.

use std::{
    future::Future,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use nokkvi_data::{
    types::{radio_station::RadioStation, song::Song},
    utils::server_url::has_http_scheme,
};
use tokio::sync::Mutex;
use tracing::warn;

/// Filename-safe truncation cap for the READABLE STEM of the sanitized
/// cover-id suffix. Subsonic cover ids are typically <40 chars; the cap guards
/// against pathological inputs blowing past `NAME_MAX` (255 on ext4/btrfs)
/// once the `mpris-art-<pid>-` prefix, the hash suffix, and the `.jpg`
/// extension are factored in.
const SANITIZED_COVER_ID_STEM_MAX_LEN: usize = 80;

/// Byte length of the `_<16 hex digits>` disambiguating suffix appended by
/// [`sanitize_cover_id`]. Only the NAME_MAX bound test reads it.
#[cfg(test)]
const HASH_SUFFIX_LEN: usize = 17;

/// How long a failed cover or placeholder is left alone before the next try.
/// It keeps the ~100ms tick from refetching a failing cover (re-logging its
/// credentialed getCoverArt URL) or rewriting into an unwritable cache dir on
/// every tick, and still heals a transient failure within a minute. A radio
/// logo needs the timer: its cover id never changes while the station plays,
/// so no "next track" ever retries it.
const RETRY_FAILED_AFTER: Duration = Duration::from_secs(60);

/// Tracks the most recently-written cache entry so repeat ticks for the
/// same `(server_url, cover_id)` skip the fetch + write.
///
/// The path is included so the next write can remove the file it superseded
/// without re-deriving the previous filename.
#[derive(Debug, Default)]
pub(crate) struct ArtCacheState {
    last_written: Option<(String, String, PathBuf)>,
    /// Last `(server_url, cover_id)` that could not be published (no art,
    /// non-image body, fetch error, or a failed cache write), and when.
    /// `handle_tick` re-resolves the art every ~100ms with the current item's
    /// cover id; without this a cover the server can't resolve would be
    /// re-fetched — and its credentialed getCoverArt URL re-logged — on every
    /// tick. The same key is skipped for [`RETRY_FAILED_AFTER`]; a different
    /// key falls through at once. Cleared on any successful write and on
    /// `clear()`.
    ///
    /// Unlike the UI mini-cover negative cache (which records ONLY deterministic
    /// "not found" misses and lets a transient drop retry on the next scroll),
    /// this deliberately records ANY failure — including a transient throttle.
    /// On the 100ms tick, NOT caching a transient would re-issue the 3-retry
    /// fetch every tick and re-storm an already-throttled server; the accepted
    /// cost is up to a minute of fallback art after a transient drop.
    last_failed: Option<(String, String, Instant)>,
    /// The placeholder file, written once and then reused until `clear()`.
    /// It sits apart from `last_written` / `last_failed` on purpose: falling
    /// back to it must neither clear a failed cover's negative entry (which
    /// would refetch that cover on the next tick) nor be deleted by the next
    /// cover write's supersede.
    placeholder: PlaceholderSlot,
}

impl ArtCacheState {
    pub(crate) const fn new() -> Self {
        Self {
            last_written: None,
            last_failed: None,
            placeholder: PlaceholderSlot::Unwritten,
        }
    }
}

impl ArtCacheState {
    fn mark_failed(&mut self, server_url: &str, cover_id: &str) {
        self.last_failed = Some((server_url.to_string(), cover_id.to_string(), Instant::now()));
    }
}

/// Whether this process has written the placeholder file yet.
#[derive(Debug, Default)]
enum PlaceholderSlot {
    #[default]
    Unwritten,
    Written(PathBuf),
    /// The write failed at this instant. Retried after [`RETRY_FAILED_AFTER`],
    /// so an unwritable cache dir logs one warning a minute, not ten a second.
    Failed(Instant),
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

/// Per-track cache path: `<dir>/mpris-art-<pid>-<cover_id>_<hash>.jpg`, where
/// the `<cover_id>_<hash>` half is produced wholesale by [`sanitize_cover_id`].
fn cache_file_path_for(cache_dir: &Path, cover_id: &str) -> PathBuf {
    per_pid_file_path(cache_dir, &sanitize_cover_id(cover_id))
}

/// Stem of the placeholder art file, `mpris-art-<pid>-placeholder.jpg`.
/// Every cover file's stem ends in the `_<16 hex>` suffix from
/// [`sanitize_cover_id`], so no cover id can name this file, and a cover
/// write's supersede-delete can never remove it. It keeps the
/// `mpris-art-<pid>-*.jpg` shape, so `clear()` and the dead-pid boot sweep
/// collect it with no pattern of their own.
const PLACEHOLDER_STEM: &str = "placeholder";

/// Placeholder art path: `<dir>/mpris-art-<pid>-placeholder.jpg`.
fn placeholder_file_path(cache_dir: &Path) -> PathBuf {
    per_pid_file_path(cache_dir, PLACEHOLDER_STEM)
}

/// `<dir>/mpris-art-<pid>-<stem>.jpg`: the one shape both sweeps match.
fn per_pid_file_path(cache_dir: &Path, stem: &str) -> PathBuf {
    let pid = std::process::id();
    cache_dir.join(format!("mpris-art-{pid}-{stem}.jpg"))
}

/// Replace anything that isn't `[A-Za-z0-9._-]` with `_`, cap the readable
/// stem to [`SANITIZED_COVER_ID_STEM_MAX_LEN`], then append a stable hash of
/// the FULL id. Subsonic cover ids are already filename-safe in practice; this
/// is a defensive belt against future server quirks or non-Navidrome backends.
///
/// The hash suffix is what makes the mapping injective. Sanitization is lossy
/// twice over — every unsafe byte collapses to `_`, and anything past the cap
/// is dropped — so without it two distinct cover ids could name the same cache
/// file and one track would render the other's art.
///
/// Be clear about the scope: with Navidrome this is unreachable. The longest
/// shape it emits is `dc-<id>:<disc>_<hex>` at ~48 bytes against an 80-byte
/// cap, and the only unsafe character in any real id is the single structural
/// `:` in `dc-`, which cannot alias onto anything (base62 and hex ids contain
/// no `_` of their own). This is a belt for non-Navidrome Subsonic backends
/// and future id shapes, not a fix for an observed collision.
///
/// Determinism is load-bearing, but NOT because any path is re-derived to
/// delete it — `write_art_inner` deletes via the `PathBuf` stashed in
/// `ArtCacheState::last_written`. It matters because that same function derives
/// `new_path` and compares `prev != new_path` to decide whether the superseded
/// file is a distinct file at all; a non-deterministic suffix would make every
/// repeat write look like a new track and churn the cache dir.
///
/// The FNV-1a suffix is identical to `lyrics_source::sanitize_filename`. The
/// stem capping deliberately is NOT: that one uses Unicode `is_alphanumeric()`
/// and so needs a byte-aware push loop, whereas this one keeps only
/// `is_ascii_alphanumeric()` and can therefore `truncate()` outright. Relaxing
/// the closure below to Unicode alphanumerics without also porting that loop
/// would turn the `truncate` into a panic on a multi-byte cover id.
fn sanitize_cover_id(cover_id: &str) -> String {
    // Sanitization maps every non-ASCII-alphanumeric char to `_`, so the stem
    // is pure ASCII and a byte truncate can never split a char boundary.
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
    if out.len() > SANITIZED_COVER_ID_STEM_MAX_LEN {
        out.truncate(SANITIZED_COVER_ID_STEM_MAX_LEN);
    }
    // FNV-1a over the full id. Stable and well-distributed, but a 64-bit
    // non-cryptographic hash — it makes accidental aliasing vanishingly
    // unlikely, not impossible, and resists nothing adversarial.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in cover_id.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{out}_{hash:016x}")
}

/// The item `handle_tick` publishes `mpris:artUrl` for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MprisArtItem {
    /// Nothing is loaded ("Not Playing").
    Nothing,
    /// A queue song and the cover id it resolves to (`cover_art`, else
    /// `album_id`).
    Song { cover_id: Option<String> },
    /// A radio station: its uploaded-logo token
    /// (`RadioStation::logo_cover_art`) and the stream's current ICY
    /// `StreamUrl`.
    Radio {
        logo: Option<String>,
        icy_url: Option<String>,
    },
}

impl MprisArtItem {
    /// A queue song: its `cover_art`, else its `album_id`.
    pub(crate) fn for_song(song: &Song) -> Self {
        Self::Song {
            cover_id: song.cover_art.clone().or_else(|| song.album_id.clone()),
        }
    }

    /// A radio station: its uploaded logo, if any, and the stream's current
    /// ICY `StreamUrl`.
    pub(crate) fn for_radio(station: &RadioStation, icy_url: Option<String>) -> Self {
        Self::Radio {
            logo: station.logo_cover_art().map(str::to_owned),
            icy_url,
        }
    }
}

/// One source for `mpris:artUrl`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArtCandidate<'a> {
    /// A server cover id, fetched through the authenticated client and
    /// published as a `file://` cache path.
    Cover(&'a str),
    /// A radio stream's ICY `StreamUrl`, published verbatim and only when it
    /// is http(s). It is never fetched here: `handle_tick` awaits the art
    /// before it sends any state update, and a slow station host would
    /// freeze position and title for the length of the fetch.
    StreamUrl(&'a str),
    /// The app icon, written once into the cache dir.
    Placeholder,
}

/// The MPRIS art precedence, in one place. The resolver publishes the first
/// candidate that yields a URI. A current song or station always ends in the
/// placeholder, so a desktop shell never keeps the previous item's art;
/// "Not Playing" publishes nothing.
fn art_candidates(item: &MprisArtItem) -> [Option<ArtCandidate<'_>>; 3] {
    fn cover(id: &Option<String>) -> Option<ArtCandidate<'_>> {
        id.as_deref()
            .filter(|id| !id.is_empty())
            .map(ArtCandidate::Cover)
    }
    match item {
        MprisArtItem::Nothing => [None, None, None],
        MprisArtItem::Song { cover_id } => [cover(cover_id), Some(ArtCandidate::Placeholder), None],
        // Logo first: it is the station's identity in-app too, and the only
        // radio art validated by the user's own server. Swap the first two
        // entries to prefer the stream's per-track art.
        MprisArtItem::Radio { logo, icy_url } => [
            cover(logo),
            icy_url
                .as_deref()
                .filter(|url| has_http_scheme(url))
                .map(ArtCandidate::StreamUrl),
            Some(ArtCandidate::Placeholder),
        ],
    }
}

/// Resolve `mpris:artUrl` for the current item, in the order
/// [`art_candidates`] sets: a song's cover, else the placeholder; a station's
/// logo, else its http(s) stream art, else the placeholder. Cover ids go
/// through the authenticated fetch and come back as `file://` cache paths
/// (see the module doc), so no credentialed URL reaches D-Bus.
///
/// `fetch_cover` builds the fetch for a cover id each time that cover is
/// tried; the future it returns is awaited only when the writer has no cached
/// result for the id, so the closure must defer its work into that future.
/// Returns `None` for "Not Playing", or when nothing could be written (no
/// cache dir, or the writes are failing) and no stream URL qualifies.
pub(crate) async fn resolve_art_for_mpris<F, Fut>(
    server_url: &str,
    item: MprisArtItem,
    fetch_cover: F,
) -> Option<String>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = anyhow::Result<Vec<u8>>>,
{
    let cache_dir = cache_dir_path();
    let mut state = STATE.lock().await;
    resolve_art_inner(
        &mut state,
        cache_dir.as_deref(),
        server_url,
        item,
        fetch_cover,
    )
    .await
}

/// Pure-ish core of [`resolve_art_for_mpris`]: tests pass their own state,
/// cache dir and fetcher.
async fn resolve_art_inner<F, Fut>(
    state: &mut ArtCacheState,
    cache_dir: Option<&Path>,
    server_url: &str,
    item: MprisArtItem,
    fetch_cover: F,
) -> Option<String>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = anyhow::Result<Vec<u8>>>,
{
    // Each item names at most one cover id, so the fetcher runs at most once.
    let mut fetch_cover = Some(fetch_cover);
    for candidate in art_candidates(&item).into_iter().flatten() {
        let uri = match candidate {
            ArtCandidate::Cover(cover_id) => match (cache_dir, fetch_cover.take()) {
                (Some(dir), Some(fetch)) => {
                    let fetcher = fetch(cover_id.to_owned());
                    write_art_inner(state, dir, server_url, cover_id, fetcher).await
                }
                _ => None,
            },
            ArtCandidate::StreamUrl(url) => Some(url.to_owned()),
            ArtCandidate::Placeholder => match cache_dir {
                Some(dir) => placeholder_uri(state, dir).await,
                None => None,
            },
        };
        if uri.is_some() {
            return uri;
        }
    }
    None
}

/// The placeholder's `file://` URI, writing the app icon on first use. The
/// PNG bytes sit under a `.jpg` name, as some covers already do; shells sniff
/// the content.
///
/// The slot trusts the file to stay on disk until `clear()`, as the cover
/// fast path does. Only an outside delete could remove it before then.
async fn placeholder_uri(state: &mut ArtCacheState, cache_dir: &Path) -> Option<String> {
    match &state.placeholder {
        PlaceholderSlot::Written(path) => return Some(path_to_file_uri(path)),
        PlaceholderSlot::Failed(at) if at.elapsed() < RETRY_FAILED_AFTER => return None,
        PlaceholderSlot::Failed(_) | PlaceholderSlot::Unwritten => {}
    }
    let path = placeholder_file_path(cache_dir);
    if !write_cache_file(&path, crate::services::APP_ICON_PNG).await {
        state.placeholder = PlaceholderSlot::Failed(Instant::now());
        return None;
    }
    let uri = path_to_file_uri(&path);
    state.placeholder = PlaceholderSlot::Written(path);
    Some(uri)
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

/// Extract the `<pid>` portion from any of the three filename shapes:
///   - `mpris-art-<pid>.jpg`                    (pre-NF2 legacy)
///   - `mpris-art-<pid>-<cover_id>.jpg`         (per-cover, pre-hash)
///   - `mpris-art-<pid>-<cover_id>_<hash>.jpg`  (current)
///
/// All three are handled by the same rule — the pid is everything up to the
/// first `-` after the prefix — so the cover segment's shape is irrelevant.
/// Keep it that way: parsing the cover segment would break the upgrade path,
/// where a user's cache dir holds all three at once.
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

    // Negative fast-path: this exact key failed within the last
    // RETRY_FAILED_AFTER (no art, non-image body, fetch error, or a failed
    // write). Skip the re-fetch — handle_tick calls us every ~100ms, so without
    // this a server-unresolvable cover would be re-fetched (and its credentialed
    // getCoverArt URL re-logged) on every tick. A different cover_id falls
    // through at once.
    if let Some((failed_server, failed_cover, failed_at)) = &state.last_failed
        && failed_server == server_url
        && failed_cover == cover_id
        && failed_at.elapsed() < RETRY_FAILED_AFTER
    {
        return None;
    }

    let bytes = match fetcher.await {
        Ok(b) if b.is_empty() => {
            warn!(
                target: "nokkvi::mpris::art",
                server_url, cover_id, "art fetch returned empty body; skipping write"
            );
            state.mark_failed(server_url, cover_id);
            return None;
        }
        Ok(b) => b,
        Err(err) => {
            warn!(
                target: "nokkvi::mpris::art",
                server_url, cover_id, %err, "art fetch failed; mpris falls back"
            );
            state.mark_failed(server_url, cover_id);
            return None;
        }
    };

    if !write_cache_file(&new_path, &bytes).await {
        // Recorded like a failed fetch, so an unwritable cache dir can't turn
        // into a full-size fetch on every tick.
        state.mark_failed(server_url, cover_id);
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

/// Create the cache dir if needed and write `bytes` to `path`. Warns and
/// returns `false` on failure.
async fn write_cache_file(path: &Path, bytes: &[u8]) -> bool {
    if let Some(parent) = path.parent()
        && let Err(err) = tokio::fs::create_dir_all(parent).await
    {
        warn!(
            target: "nokkvi::mpris::art",
            path = %parent.display(), %err, "failed to create mpris art cache dir"
        );
        return false;
    }
    if let Err(err) = tokio::fs::write(path, bytes).await {
        warn!(
            target: "nokkvi::mpris::art",
            path = %path.display(), %err, "failed to write mpris art cache file"
        );
        return false;
    }
    true
}

/// Reset state and best-effort sweep every `mpris-art-<pid>-*.jpg` file in
/// `cache_dir` for the current process. Tests can drive this with a scratch
/// dir without touching the module-level static.
async fn clear_inner(state: &mut ArtCacheState, cache_dir: Option<&Path>) {
    *state = ArtCacheState::new();
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
        // Exact tail, so the hash is pinned as the LAST component — a
        // `contains` check would accept a stray segment after it.
        assert!(
            s.ends_with(&format!("mpris-art-{pid}-al-abc123_29acc9e7096cd157.jpg")),
            "path should be 'mpris-art-<pid>-<cover>_<hash>.jpg', got: {s}"
        );
        assert!(
            s.starts_with("/tmp/scratch/nokkvi/"),
            "path should sit in the provided dir, got: {s}"
        );
    }

    /// The readable stem must survive verbatim so cache files stay greppable;
    /// only the disambiguating hash suffix is appended.
    #[test]
    fn sanitize_cover_id_keeps_safe_chars() {
        assert!(sanitize_cover_id("al-abc_123.foo").starts_with("al-abc_123.foo_"));
        assert!(sanitize_cover_id("ABCxyz09").starts_with("ABCxyz09_"));
    }

    #[test]
    fn sanitize_cover_id_replaces_path_separators_and_whitespace() {
        // Defensive: Subsonic ids are typically `[A-Za-z0-9-]`, but if a
        // backend ever returns slashes / spaces / colons the sanitizer must
        // collapse them so we don't escape the cache dir or break the format.
        assert!(sanitize_cover_id("al/abc:def").starts_with("al_abc_def_"));
        assert!(sanitize_cover_id("a b\tc\nd").starts_with("a_b_c_d_"));
        assert!(sanitize_cover_id("../etc/passwd").starts_with(".._etc_passwd_"));
        // No sanitized output may contain a path separator, whatever the input.
        for probe in ["../../x", "a/b/c", "a\\b"] {
            let out = sanitize_cover_id(probe);
            assert!(
                !out.contains('/') && !out.contains('\\'),
                "sanitized output must not contain a separator, got: {out}"
            );
        }
    }

    #[test]
    fn sanitize_cover_id_truncates_overlong_input() {
        let long = "a".repeat(500);
        let out = sanitize_cover_id(&long);
        assert!(
            out.len() <= SANITIZED_COVER_ID_STEM_MAX_LEN + HASH_SUFFIX_LEN,
            "sanitize must cap output to {} bytes, got {}",
            SANITIZED_COVER_ID_STEM_MAX_LEN + HASH_SUFFIX_LEN,
            out.len()
        );
        // The full filename must stay inside Linux's 255-byte NAME_MAX at the
        // WORST case, not at whatever pid the test happens to run under: the
        // kernel's `pid_max` ceiling is 2^22, so pin the widest pid rather than
        // `std::process::id()` (5-7 digits in CI, which would let a future stem
        // cap bump pass here and still overflow in the field).
        let name_len = format!("mpris-art-{}-{out}.jpg", 4_194_304_u32).len();
        assert!(
            name_len <= 255,
            "filename must fit NAME_MAX, got {name_len}"
        );
    }

    /// Golden values, not shape assertions: these pin the FNV-1a basis and
    /// prime. An accidental edit to either constant silently repartitions every
    /// cache filename, which a `starts_with` check would sail straight past.
    #[test]
    fn sanitize_cover_id_pins_hash_output() {
        assert_eq!(sanitize_cover_id(""), "_cbf29ce484222325");
        assert_eq!(sanitize_cover_id("al-abc123"), "al-abc123_29acc9e7096cd157");
    }

    /// Two distinct cover ids that agree on their first
    /// [`SANITIZED_COVER_ID_STEM_MAX_LEN`] sanitized bytes previously collided
    /// onto one cache file, so one track rendered the other's art. The hash is
    /// taken over the FULL id, so the suffix separates them.
    #[test]
    fn sanitize_cover_id_disambiguates_ids_sharing_a_truncated_prefix() {
        let prefix = "x".repeat(SANITIZED_COVER_ID_STEM_MAX_LEN);
        let a = format!("{prefix}AAA");
        let b = format!("{prefix}BBB");
        assert_ne!(sanitize_cover_id(&a), sanitize_cover_id(&b));
    }

    /// Collisions also arise without truncation: distinct unsafe characters
    /// all sanitize to `_`.
    #[test]
    fn sanitize_cover_id_disambiguates_ids_that_sanitize_alike() {
        assert_ne!(sanitize_cover_id("al/abc"), sanitize_cover_id("al:abc"));
        assert_ne!(sanitize_cover_id(""), sanitize_cover_id("/"));
    }

    /// The hash must depend on the FULL id, not just the retained stem —
    /// otherwise capping still collapses long ids onto one file. Distinct
    /// beyond-the-cap tails must produce distinct hashes for the SAME stem.
    #[test]
    fn sanitize_cover_id_hashes_the_full_id_not_the_stem() {
        let stem = "y".repeat(SANITIZED_COVER_ID_STEM_MAX_LEN);
        let a = sanitize_cover_id(&format!("{stem}tail-one"));
        let b = sanitize_cover_id(&format!("{stem}tail-two"));
        assert!(
            a.starts_with(&stem) && b.starts_with(&stem),
            "both must retain the same capped stem"
        );
        assert_ne!(a, b, "hash must separate ids differing only past the cap");
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

        // One legacy-shape and one current hashed-shape file, so the sweep is
        // pinned against BOTH: a user upgrading carries the old shape forward.
        let mine = [
            format!("mpris-art-{pid}-al-aaa.jpg"),
            format!("mpris-art-{pid}-{}.jpg", sanitize_cover_id("al-bbb")),
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
        // handle_tick resolves the art every ~100ms with the current
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

    /// The parser must handle the CURRENT (hashed) shape, fed through the real
    /// `sanitize_cover_id` rather than a hand-written literal — the other
    /// fixtures in this module predate the hash suffix and would all still pass
    /// if the cover segment were ever parsed instead of skipped. Without this,
    /// tightening `parse_pid_from_filename` could sweep a LIVE instance's cache
    /// file out from under it and no test would notice.
    #[test]
    fn parse_pid_from_filename_extracts_pid_for_hashed_format() {
        for cover in ["al-abc123", "", "/", "..", "-", "x".repeat(200).as_str()] {
            let name = format!("mpris-art-3785659-{}.jpg", sanitize_cover_id(cover));
            assert_eq!(
                parse_pid_from_filename(&name),
                Some(3_785_659),
                "hashed shape must still yield its pid: {name}"
            );
        }
        // The placeholder shares the shape, so the boot sweep collects a dead
        // instance's placeholder too.
        let name = format!("mpris-art-3785659-{PLACEHOLDER_STEM}.jpg");
        assert_eq!(parse_pid_from_filename(&name), Some(3_785_659), "{name}");
        let ours = placeholder_file_path(Path::new("/x"));
        let ours = ours.file_name().and_then(|n| n.to_str()).unwrap();
        assert_eq!(parse_pid_from_filename(ours), Some(std::process::id()));
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
        // Current shape, built through the real sanitizer: a LIVE instance's
        // hashed file must survive another instance's boot sweep.
        let current_self = format!("mpris-art-{my_pid}-{}.jpg", sanitize_cover_id("al-mine"));
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

    // ── resolve_art_inner: what handle_tick publishes ──────────────────

    const SERVER: &str = "https://server.example";

    /// Drive `resolve_art_inner` against `dir` with a fetcher that bumps
    /// `calls` when awaited and answers every cover id with `reply`. Returns
    /// the URI and the cover id the fetcher was built for, if any.
    async fn resolve_with_request(
        state: &mut ArtCacheState,
        dir: Option<&Path>,
        item: MprisArtItem,
        calls: &Arc<AtomicU32>,
        reply: Result<Vec<u8>, &'static str>,
    ) -> (Option<String>, Option<String>) {
        let c = Arc::clone(calls);
        let mut requested = None;
        let uri = resolve_art_inner(state, dir, SERVER, item, |cover_id| {
            requested = Some(cover_id);
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                reply.map_err(anyhow::Error::msg)
            }
        })
        .await;
        (uri, requested)
    }

    async fn resolve_with(
        state: &mut ArtCacheState,
        dir: Option<&Path>,
        item: MprisArtItem,
        calls: &Arc<AtomicU32>,
        reply: Result<Vec<u8>, &'static str>,
    ) -> Option<String> {
        resolve_with_request(state, dir, item, calls, reply).await.0
    }

    /// An instant older than the retry cooldown, for aging a failure record.
    fn past_the_cooldown() -> Instant {
        Instant::now()
            .checked_sub(RETRY_FAILED_AFTER + Duration::from_secs(1))
            .expect("monotonic clock is past the cooldown")
    }

    /// A "cache dir" that is a regular file, so every write under it fails.
    fn unwritable_dir(scratch: &ScratchDir) -> PathBuf {
        let path = scratch.path().join("not-a-dir");
        std::fs::write(&path, b"x").expect("create blocker file");
        path
    }

    fn song(cover_id: &str) -> MprisArtItem {
        MprisArtItem::Song {
            cover_id: Some(cover_id.to_string()),
        }
    }

    fn radio(logo: Option<&str>, icy_url: Option<&str>) -> MprisArtItem {
        MprisArtItem::Radio {
            logo: logo.map(str::to_string),
            icy_url: icy_url.map(str::to_string),
        }
    }

    fn placeholder_uri_in(dir: &Path) -> String {
        path_to_file_uri(&placeholder_file_path(dir))
    }

    fn assert_placeholder_on_disk(dir: &Path) {
        let bytes = std::fs::read(placeholder_file_path(dir)).expect("placeholder file exists");
        assert_eq!(
            bytes,
            crate::services::APP_ICON_PNG,
            "placeholder must hold the app icon"
        );
    }

    #[tokio::test]
    async fn resolve_song_publishes_its_cover_as_a_file_uri() {
        let dir = ScratchDir::new();
        let mut state = ArtCacheState::new();
        let calls = Arc::new(AtomicU32::new(0));

        let (uri, requested) = resolve_with_request(
            &mut state,
            Some(dir.path()),
            song("al-abc"),
            &calls,
            Ok(vec![1, 2, 3]),
        )
        .await;

        assert_eq!(
            uri,
            Some(path_to_file_uri(&cache_file_path_for(dir.path(), "al-abc")))
        );
        assert_eq!(requested.as_deref(), Some("al-abc"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn resolve_nothing_publishes_no_art() {
        let dir = ScratchDir::new();
        let mut state = ArtCacheState::new();
        let calls = Arc::new(AtomicU32::new(0));

        let uri = resolve_with(
            &mut state,
            Some(dir.path()),
            MprisArtItem::Nothing,
            &calls,
            Ok(vec![1]),
        )
        .await;

        assert_eq!(uri, None, "'Not Playing' publishes no artUrl");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn resolve_radio_publishes_its_http_icy_url_verbatim() {
        let dir = ScratchDir::new();
        let mut state = ArtCacheState::new();
        let calls = Arc::new(AtomicU32::new(0));

        for url in ["https://cdn.example/now.jpg", "HTTP://cdn.example/now.jpg"] {
            let uri = resolve_with(
                &mut state,
                Some(dir.path()),
                radio(None, Some(url)),
                &calls,
                Ok(vec![1]),
            )
            .await;
            assert_eq!(uri.as_deref(), Some(url));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    /// The reported bug: a station with no logo and no stream art published
    /// no `artUrl`, so the shell kept the previous song's cover.
    #[tokio::test]
    async fn resolve_radio_without_any_art_publishes_the_placeholder() {
        let dir = ScratchDir::new();
        let mut state = ArtCacheState::new();
        let calls = Arc::new(AtomicU32::new(0));

        let uri = resolve_with(
            &mut state,
            Some(dir.path()),
            radio(None, None),
            &calls,
            Ok(vec![1]),
        )
        .await;

        assert_eq!(uri, Some(placeholder_uri_in(dir.path())));
        assert_placeholder_on_disk(dir.path());
        assert_eq!(calls.load(Ordering::SeqCst), 0, "no cover id to fetch");
    }

    #[tokio::test]
    async fn resolve_radio_with_a_logo_publishes_the_logo_file_not_the_icy_url() {
        let dir = ScratchDir::new();
        let mut state = ArtCacheState::new();
        let calls = Arc::new(AtomicU32::new(0));

        let (uri, requested) = resolve_with_request(
            &mut state,
            Some(dir.path()),
            radio(Some("ra-42_abc"), Some("http://cdn.example/now.jpg")),
            &calls,
            Ok(vec![1, 2, 3]),
        )
        .await;

        assert_eq!(
            uri,
            Some(path_to_file_uri(&cache_file_path_for(
                dir.path(),
                "ra-42_abc"
            )))
        );
        assert_eq!(requested.as_deref(), Some("ra-42_abc"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let on_disk = std::fs::read(cache_file_path_for(dir.path(), "ra-42_abc")).unwrap();
        assert_eq!(on_disk, vec![1, 2, 3], "the file holds the fetched logo");
    }

    #[tokio::test]
    async fn resolve_radio_never_publishes_a_non_http_icy_url() {
        let dir = ScratchDir::new();
        let mut state = ArtCacheState::new();
        let calls = Arc::new(AtomicU32::new(0));

        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "ftp://example.com/a.png",
            "example.com/a.png",
            "",
        ] {
            let uri = resolve_with(
                &mut state,
                Some(dir.path()),
                radio(None, Some(url)),
                &calls,
                Ok(vec![1]),
            )
            .await;
            assert_eq!(
                uri,
                Some(placeholder_uri_in(dir.path())),
                "{url:?} must fall to the placeholder"
            );
        }
    }

    /// A logo whose fetch fails falls through the chain, and its negative
    /// entry survives the fallback, so the 100 ms tick never refetches it.
    #[tokio::test]
    async fn resolve_radio_whose_logo_fetch_fails_falls_back_without_refetching() {
        for icy_url in [Some("https://cdn.example/now.jpg"), None] {
            let dir = ScratchDir::new();
            let mut state = ArtCacheState::new();
            let calls = Arc::new(AtomicU32::new(0));
            let expected = icy_url.map_or_else(|| placeholder_uri_in(dir.path()), str::to_string);

            for _ in 0..5 {
                let uri = resolve_with(
                    &mut state,
                    Some(dir.path()),
                    radio(Some("ra-42_abc"), icy_url),
                    &calls,
                    Err("artwork response was not an image"),
                )
                .await;
                assert_eq!(uri.as_deref(), Some(expected.as_str()));
            }
            assert_eq!(
                calls.load(Ordering::SeqCst),
                1,
                "a failed logo is fetched once per station, not every tick"
            );
        }
    }

    #[tokio::test]
    async fn resolve_song_whose_cover_fetch_fails_publishes_the_placeholder() {
        let dir = ScratchDir::new();
        let mut state = ArtCacheState::new();
        let calls = Arc::new(AtomicU32::new(0));

        let uri = resolve_with(
            &mut state,
            Some(dir.path()),
            song("al-missing"),
            &calls,
            Err("artwork response was not an image"),
        )
        .await;

        assert_eq!(uri, Some(placeholder_uri_in(dir.path())));
        assert_placeholder_on_disk(dir.path());
    }

    /// With no cache dir (`HOME` and `XDG_CACHE_HOME` unset) nothing can be
    /// written, so only a publishable ICY URL survives.
    #[tokio::test]
    async fn resolve_without_a_cache_dir_publishes_only_an_http_icy_url() {
        let mut state = ArtCacheState::new();
        let calls = Arc::new(AtomicU32::new(0));

        for item in [
            song("al-abc"),
            radio(Some("ra-42_abc"), None),
            radio(None, None),
            radio(None, Some("ftp://example.com/a.png")),
        ] {
            let uri = resolve_with(&mut state, None, item.clone(), &calls, Ok(vec![1])).await;
            assert_eq!(uri, None, "{item:?} has nothing to publish");
        }
        let uri = resolve_with(
            &mut state,
            None,
            radio(Some("ra-42_abc"), Some("https://cdn.example/now.jpg")),
            &calls,
            Ok(vec![1]),
        )
        .await;
        assert_eq!(uri.as_deref(), Some("https://cdn.example/now.jpg"));
        assert_eq!(calls.load(Ordering::SeqCst), 0, "no cache dir, no fetch");
    }

    /// Covers and the placeholder alternate without either deleting the
    /// other's file: every placeholder URI handed out names a file on disk.
    #[tokio::test]
    async fn placeholder_survives_cover_writes_on_either_side() {
        let dir = ScratchDir::new();
        let mut state = ArtCacheState::new();
        let calls = Arc::new(AtomicU32::new(0));
        let logo_less = radio(None, None);

        let steps = [
            (song("al-aaa"), Ok(vec![1])),
            (logo_less.clone(), Ok(vec![1])),
            (song("al-bbb"), Ok(vec![2])),
            (logo_less.clone(), Ok(vec![2])),
            (radio(Some("ra-42_abc"), None), Ok(vec![3])),
            (logo_less, Ok(vec![3])),
        ];
        for (item, reply) in steps {
            let is_placeholder = item == radio(None, None);
            let uri = resolve_with(&mut state, Some(dir.path()), item, &calls, reply)
                .await
                .expect("a current item always publishes art");
            if is_placeholder {
                assert_eq!(uri, placeholder_uri_in(dir.path()));
                assert_placeholder_on_disk(dir.path());
            }
        }
        assert!(
            cache_file_path_for(dir.path(), "ra-42_abc").exists(),
            "the placeholder must not supersede-delete the last cover"
        );
    }

    #[tokio::test]
    async fn clear_inner_removes_the_placeholder_and_the_next_resolve_rewrites_it() {
        let dir = ScratchDir::new();
        let mut state = ArtCacheState::new();
        let calls = Arc::new(AtomicU32::new(0));

        let first = resolve_with(
            &mut state,
            Some(dir.path()),
            radio(None, None),
            &calls,
            Ok(vec![1]),
        )
        .await;
        assert_eq!(first, Some(placeholder_uri_in(dir.path())));

        clear_inner(&mut state, Some(dir.path())).await;
        assert!(
            !placeholder_file_path(dir.path()).exists(),
            "clear must remove the placeholder file"
        );

        let second = resolve_with(
            &mut state,
            Some(dir.path()),
            radio(None, None),
            &calls,
            Ok(vec![1]),
        )
        .await;
        assert_eq!(second, first);
        assert_placeholder_on_disk(dir.path());
    }

    #[test]
    fn for_song_prefers_cover_art_then_album_id() {
        let with_cover = Song {
            cover_art: Some("mf-1".to_string()),
            album_id: Some("al-1".to_string()),
            ..Song::default()
        };
        assert_eq!(MprisArtItem::for_song(&with_cover), song("mf-1"));

        let album_only = Song {
            album_id: Some("al-1".to_string()),
            ..Song::default()
        };
        assert_eq!(MprisArtItem::for_song(&album_only), song("al-1"));

        assert_eq!(
            MprisArtItem::for_song(&Song::default()),
            MprisArtItem::Song { cover_id: None }
        );
    }

    #[test]
    fn for_radio_carries_the_logo_token_and_the_icy_url() {
        let station = |cover_art: Option<&str>| RadioStation {
            id: "st-1".to_string(),
            name: "Station".to_string(),
            stream_url: "https://stream.example/live".to_string(),
            home_page_url: None,
            cover_art: cover_art.map(str::to_string),
        };
        let icy = || Some("https://cdn.example/now.jpg".to_string());

        assert_eq!(
            MprisArtItem::for_radio(&station(Some("ra-st-1_abc")), icy()),
            radio(Some("ra-st-1_abc"), Some("https://cdn.example/now.jpg"))
        );
        // Navidrome sends an empty token for a station with no logo.
        assert_eq!(
            MprisArtItem::for_radio(&station(Some("")), icy()),
            radio(None, Some("https://cdn.example/now.jpg"))
        );
        assert_eq!(
            MprisArtItem::for_radio(&station(None), None),
            radio(None, None)
        );
    }

    /// A cover that fetches but can't be written is recorded like a failed
    /// fetch, so an unwritable cache dir can't cause a full fetch every tick.
    #[tokio::test]
    async fn failed_write_is_not_refetched_every_tick() {
        let scratch = ScratchDir::new();
        let dir = unwritable_dir(&scratch);
        let mut state = ArtCacheState::new();
        let calls = Arc::new(AtomicU32::new(0));

        for _ in 0..5 {
            let uri = resolve_with(
                &mut state,
                Some(&dir),
                radio(Some("ra-42_abc"), Some("https://cdn.example/now.jpg")),
                &calls,
                Ok(vec![1, 2, 3]),
            )
            .await;
            assert_eq!(uri.as_deref(), Some("https://cdn.example/now.jpg"));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    /// A station's logo token never changes while it plays, so a transient
    /// failure must heal on a timer, not on a track change.
    #[tokio::test]
    async fn failed_cover_is_retried_after_the_cooldown() {
        let dir = ScratchDir::new();
        let mut state = ArtCacheState::new();
        let calls = Arc::new(AtomicU32::new(0));
        let logo_station = radio(Some("ra-42_abc"), None);

        let first = resolve_with(
            &mut state,
            Some(dir.path()),
            logo_station.clone(),
            &calls,
            Err("throttled"),
        )
        .await;
        assert_eq!(first, Some(placeholder_uri_in(dir.path())));

        if let Some((_, _, failed_at)) = state.last_failed.as_mut() {
            *failed_at = past_the_cooldown();
        }
        let second = resolve_with(
            &mut state,
            Some(dir.path()),
            logo_station,
            &calls,
            Ok(vec![1, 2, 3]),
        )
        .await;
        assert_eq!(
            second,
            Some(path_to_file_uri(&cache_file_path_for(
                dir.path(),
                "ra-42_abc"
            )))
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    /// A failed placeholder write is retried after the cooldown, not left
    /// failed until logout: that would bring the stuck-cover bug back.
    #[tokio::test]
    async fn failed_placeholder_write_is_retried_after_the_cooldown() {
        let scratch = ScratchDir::new();
        let bad = unwritable_dir(&scratch);
        let good = ScratchDir::new();
        let mut state = ArtCacheState::new();
        let calls = Arc::new(AtomicU32::new(0));
        let logo_less = radio(None, None);

        let failed = resolve_with(
            &mut state,
            Some(&bad),
            logo_less.clone(),
            &calls,
            Ok(vec![1]),
        )
        .await;
        assert_eq!(failed, None);

        // Inside the cooldown, even a now-writable dir is not tried.
        let cooling = resolve_with(
            &mut state,
            Some(good.path()),
            logo_less.clone(),
            &calls,
            Ok(vec![1]),
        )
        .await;
        assert_eq!(cooling, None);

        state.placeholder = PlaceholderSlot::Failed(past_the_cooldown());
        let healed = resolve_with(
            &mut state,
            Some(good.path()),
            logo_less,
            &calls,
            Ok(vec![1]),
        )
        .await;
        assert_eq!(healed, Some(placeholder_uri_in(good.path())));
        assert_placeholder_on_disk(good.path());
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
