//! Lyrics source helpers: the direct LRCLIB fetch, the disk cache writer, the
//! session cache, and the pure precedence helper (the user's own `.lrc` files
//! -> the server -> cached LRCLIB downloads -> the LRCLIB network fetch). The
//! `resolve_lyrics` chain that wires these to the real channels lives on
//! `AppService`.

use std::{sync::OnceLock, time::Duration};

use crate::{
    backend::albums::{artwork_client_base, external_host_is_blocked},
    types::{lyrics::LrcDocument, song::Song},
};

const LRCLIB_URL: &str = "https://lrclib.net/api/get";
/// One request's ceiling.
const LRCLIB_TIMEOUT: Duration = Duration::from_secs(5);
/// The whole fetch's ceiling, busy retries included. An overloaded LRCLIB
/// answers 503 with `Retry-After: 1` in about half a second, and the same
/// request a few seconds later goes through.
const LRCLIB_BUDGET: Duration = Duration::from_secs(12);
/// The least a busy answer waits before the same ask: LRCLIB's own
/// `Retry-After` is 1 s, and an answer without one, or with a shorter one,
/// waits this long. A longer `Retry-After` is honoured as given, and one that
/// would end past the budget ends the ask instead of asking sooner than told.
const LRCLIB_BUSY_WAIT_MIN: Duration = Duration::from_secs(1);

/// What one lyrics lookup found. A `Miss` is an answer about the song; a
/// `Failed` lookup (a timeout, a network or server error, an unreadable file)
/// is not, so the session cache never records one and the next play asks
/// again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LyricsLookup<T> {
    Hit(T),
    Miss,
    Failed,
}

impl<T> LyricsLookup<T> {
    /// The hit, if there was one.
    pub fn hit(self) -> Option<T> {
        match self {
            LyricsLookup::Hit(found) => Some(found),
            LyricsLookup::Miss | LyricsLookup::Failed => None,
        }
    }
}

/// Options that gate the resolve chain's optional channels.
#[derive(Debug, Clone, Copy)]
pub struct ResolveOpts {
    /// The server advertises the `songLyrics` extension (gates getLyricsBySongId).
    pub songlyrics_ext: bool,
    /// Whether the async OpenSubsonic extensions probe has RESPONDED at all.
    /// `songlyrics_ext == false` is only authoritative once this is true —
    /// before the probe lands the server channel is skipped-but-unknown, so a
    /// miss resolved then must not be negative-cached (the miss-cache
    /// completeness gate consumes this; `resolve_from` itself does not).
    pub ext_probe_landed: bool,
    /// The user allows the direct third-party LRCLIB fetch.
    pub fetch_online: bool,
}

/// A single, lazily-built, redirect-guarded client for the LRCLIB fetch —
/// mirrors `AlbumsService`'s external-image client (per-hop SSRF re-check) so a
/// public host can't `302` the fetch to a private address.
fn lrclib_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        artwork_client_base()
            .redirect(crate::utils::redirects::public_hosts_only(
                "lrclib",
                external_host_is_blocked,
            ))
            .build()
            .expect("Failed to build LRCLIB HTTP client")
    })
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
struct LrclibResponse {
    #[serde(default)]
    instrumental: bool,
    #[serde(rename = "syncedLyrics", default)]
    synced_lyrics: Option<String>,
    #[serde(rename = "plainLyrics", default)]
    plain_lyrics: Option<String>,
}

/// One `/api/get` answer.
#[derive(Debug, Clone)]
enum LrclibAnswer {
    /// A record under the asked signature.
    Record(LrclibResponse),
    /// A 404: no record has the signature.
    NotFound,
    /// A 503 or 429: the server is overloaded. Carries its `Retry-After` when
    /// it gave one in seconds.
    Busy(Option<Duration>),
    /// A timeout, a transport error, any other status, or an unreadable body.
    Failed,
}

/// What an ask settled on once its busy answers were waited out.
#[derive(Debug)]
enum Settled {
    /// A record under the asked signature.
    Record(LrclibResponse),
    /// A 404: no record has the signature.
    NotFound,
    /// A timeout, a transport error, any other status, an unreadable body, or
    /// a server still busy at the budget.
    Failed,
}

/// What [`fetch_lrclib`] found: the sheet, and for a synced one the raw
/// `syncedLyrics` text (already valid LRC) so the cache can persist it
/// verbatim — no serializer needed. A plain sheet carries `None`: it is never
/// written to disk, so the store's channels stay synced-only and a song whose
/// timed lyrics reach LRCLIB later picks them up in a later session.
pub type LrclibHit = (LrcDocument, Option<String>);

/// Fetch lyrics from LRCLIB's **exact** `/api/get` (never the fuzzy
/// `/api/search`), so a hit is never-wrong by construction.
///
/// LRCLIB matches the album letter for letter, so a deluxe edition or a single
/// tagged differently from its copy reads as no record. When nothing synced is
/// filed under the song's album, the fetch asks once more without it: title,
/// artist and duration (LRCLIB's ±2 s) still have to match. An instrumental
/// under the album is final.
///
/// Synced lyrics from either ask win; then the album's own `plainLyrics`, then
/// the album-less record's. `Miss` when neither ask found lyrics or the album's
/// record is an instrumental. `Failed` when an ask hits a timeout, a transport
/// error, a server still busy at the budget, any other HTTP status, or an
/// unreadable body, unless a plain sheet from the first ask is in hand.
pub async fn fetch_lrclib(
    artist: &str,
    title: &str,
    album: &str,
    duration_secs: u32,
) -> LyricsLookup<LrclibHit> {
    // A static check of a constant URL: it never changes, so it is a miss.
    if external_host_is_blocked(LRCLIB_URL) {
        return LyricsLookup::Miss;
    }
    fetch_lrclib_with(!album.is_empty(), |with_album| {
        lrclib_get(artist, title, with_album.then_some(album), duration_secs)
    })
    .await
}

/// [`fetch_lrclib`] behind any `get` (called with whether to send the album),
/// so its rules run on tokio's paused clock in tests. Each ask is cut at
/// [`LRCLIB_TIMEOUT`] and the whole fetch, both asks, at [`LRCLIB_BUDGET`].
async fn fetch_lrclib_with<Fut>(
    has_album: bool,
    mut get: impl FnMut(bool) -> Fut,
) -> LyricsLookup<LrclibHit>
where
    Fut: std::future::Future<Output = LrclibAnswer>,
{
    let deadline = tokio::time::Instant::now() + LRCLIB_BUDGET;
    let plain = match ask_lrclib(&mut get, has_album, deadline).await {
        Settled::Record(body) if body.instrumental => return LyricsLookup::Miss,
        Settled::Record(body) => match record_lyrics(body) {
            RecordLyrics::Synced(doc, raw) => return LyricsLookup::Hit((doc, Some(raw))),
            RecordLyrics::Plain(doc) => Some(doc),
            RecordLyrics::Nothing => None,
        },
        // A 404 is the normal "no match" answer — not an error worth logging.
        Settled::NotFound => None,
        // Says nothing about the song.
        Settled::Failed => return LyricsLookup::Failed,
    };
    if !has_album {
        return plain.map_or(LyricsLookup::Miss, |doc| LyricsLookup::Hit((doc, None)));
    }

    tracing::debug!("lrclib: nothing synced under the album, asking without it");
    let elsewhere = match ask_lrclib(&mut get, false, deadline).await {
        Settled::Record(body) => match record_lyrics(body) {
            RecordLyrics::Synced(doc, raw) => return LyricsLookup::Hit((doc, Some(raw))),
            RecordLyrics::Plain(doc) => Some(doc),
            RecordLyrics::Nothing => None,
        },
        Settled::Failed if plain.is_none() => return LyricsLookup::Failed,
        // No record; or a failure with the album's plain sheet in hand, which
        // still shows.
        Settled::NotFound | Settled::Failed => None,
    };
    plain
        .or(elsewhere)
        .map_or(LyricsLookup::Miss, |doc| LyricsLookup::Hit((doc, None)))
}

/// The lyrics in one LRCLIB record, synced first.
enum RecordLyrics {
    /// Parsed, beside the raw `syncedLyrics` the disk cache writes.
    Synced(LrcDocument, String),
    /// Untimed `plainLyrics`, as a plain sheet.
    Plain(LrcDocument),
    /// An instrumental, or a record with neither.
    Nothing,
}

/// Read a record's lyrics: `syncedLyrics` when it carries a timestamp, else
/// non-blank `plainLyrics`.
fn record_lyrics(body: LrclibResponse) -> RecordLyrics {
    if body.instrumental {
        return RecordLyrics::Nothing;
    }
    if let Some(synced) = body.synced_lyrics {
        let doc = crate::types::lyrics::parse(&synced);
        if doc.synced {
            return RecordLyrics::Synced(doc, synced);
        }
    }
    body.plain_lyrics
        .map(|text| LrcDocument::plain(&text))
        .filter(LrcDocument::is_renderable)
        .map_or(RecordLyrics::Nothing, RecordLyrics::Plain)
}

/// Ask until LRCLIB answers something other than busy. A busy answer is asked
/// again after its `Retry-After` (at least [`LRCLIB_BUSY_WAIT_MIN`]) when that
/// wait still ends before `deadline`; a longer one ends the ask as `Failed`
/// rather than asking sooner than told. A timeout is never retried: it already
/// spent most of the budget.
async fn ask_lrclib<Fut>(
    get: &mut impl FnMut(bool) -> Fut,
    with_album: bool,
    deadline: tokio::time::Instant,
) -> Settled
where
    Fut: std::future::Future<Output = LrclibAnswer>,
{
    let mut busy: u32 = 0;
    let settled = loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        let Ok(answer) = tokio::time::timeout(left.min(LRCLIB_TIMEOUT), get(with_album)).await
        else {
            tracing::debug!("lrclib fetch timed out");
            return Settled::Failed;
        };
        let retry_after = match answer {
            LrclibAnswer::Record(body) => break Settled::Record(body),
            LrclibAnswer::NotFound => break Settled::NotFound,
            LrclibAnswer::Failed => break Settled::Failed,
            LrclibAnswer::Busy(retry_after) => retry_after,
        };
        busy += 1;
        let wait = retry_after
            .unwrap_or(LRCLIB_BUSY_WAIT_MIN)
            .max(LRCLIB_BUSY_WAIT_MIN);
        if wait >= deadline.saturating_duration_since(tokio::time::Instant::now()) {
            tracing::debug!(
                busy,
                wait_ms = wait.as_millis(),
                "lrclib busy, its wait ends past the budget"
            );
            return Settled::Failed;
        }
        tracing::debug!(
            busy,
            wait_ms = wait.as_millis(),
            "lrclib busy, asking again"
        );
        tokio::time::sleep(wait).await;
    };
    if busy > 0 {
        tracing::debug!(busy, "lrclib answered after busy retries");
    }
    settled
}

/// One `/api/get` request, untimed (`ask_lrclib` times it). `album: None`
/// leaves `album_name` out, which LRCLIB reads as any album.
async fn lrclib_get(
    artist: &str,
    title: &str,
    album: Option<&str>,
    duration_secs: u32,
) -> LrclibAnswer {
    let duration = duration_secs.to_string();
    let mut params = vec![("artist_name", artist), ("track_name", title)];
    if let Some(album) = album {
        params.push(("album_name", album));
    }
    params.push(("duration", duration.as_str()));
    let mut full_url = String::from(LRCLIB_URL);
    for (i, (key, value)) in params.iter().enumerate() {
        full_url.push(if i == 0 { '?' } else { '&' });
        full_url.push_str(key);
        full_url.push('=');
        full_url
            .push_str(&url::form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>());
    }

    let response = match lrclib_client().get(&full_url).send().await {
        Ok(response) => response,
        Err(e) => {
            tracing::debug!(error = %e.without_url(), "lrclib fetch failed");
            return LrclibAnswer::Failed;
        }
    };

    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return LrclibAnswer::NotFound;
    }
    // LRCLIB's overload answer (503 + `Retry-After: 1`) and a rate limit in
    // front of it both ask for the same request again shortly.
    if status == reqwest::StatusCode::SERVICE_UNAVAILABLE
        || status == reqwest::StatusCode::TOO_MANY_REQUESTS
    {
        return LrclibAnswer::Busy(retry_after_secs(response.headers()));
    }
    if !status.is_success() {
        tracing::debug!(%status, "lrclib fetch refused");
        return LrclibAnswer::Failed;
    }

    match response.json().await {
        Ok(body) => LrclibAnswer::Record(body),
        Err(e) => {
            tracing::debug!(error = %e.without_url(), "lrclib response parse failed");
            LrclibAnswer::Failed
        }
    }
}

/// A `Retry-After` given in whole seconds (LRCLIB sends `1`). The HTTP-date
/// form reads as none.
fn retry_after_secs(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let value = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?;
    value.trim().parse::<u64>().ok().map(Duration::from_secs)
}

/// Persist a fetched synced LRCLIB result into the store so the next play resolves it
/// offline via the CACHED channel (the third of the four, after the user's own
/// files and the server — see `resolve_from`). LRCLIB's `syncedLyrics` is header-less, and
/// the index matches on `[ar:]/[ti:]/[al:]` headers (not paths) — so synthesize
/// authoritative headers from the Song's own tags (which drove the exact match)
/// before the raw text. Best-effort: a write failure is logged, not surfaced.
pub async fn cache_to_store(raw_synced: &str, song: &Song) {
    let Ok(dir) = crate::utils::paths::get_lyrics_dir() else {
        return;
    };
    let cache_dir = dir.join(".cache");
    if tokio::fs::create_dir_all(&cache_dir).await.is_err() {
        return;
    }

    let mut content = String::with_capacity(raw_synced.len() + 128);
    content.push_str(&format!("[ar:{}]\n", header_safe(&song.artist)));
    content.push_str(&format!("[ti:{}]\n", header_safe(&song.title)));
    if !song.album.is_empty() {
        content.push_str(&format!("[al:{}]\n", header_safe(&song.album)));
    }
    content.push_str(&format!(
        "[length:{:02}:{:02}]\n",
        song.duration / 60,
        song.duration % 60
    ));
    content.push_str(raw_synced);

    let name = sanitize_filename(&format!("{}-{}-{}", song.artist, song.title, song.album));
    let path = cache_dir.join(format!("{name}.lrc"));
    if let Err(e) = tokio::fs::write(&path, content).await {
        tracing::debug!(error = %e, "lrclib cache write failed");
    }
}

/// Make a tag value safe to interpolate into a `[key:value]` header. Newlines
/// always collapse to a space. Brackets are kept VERBATIM when balanced —
/// `next_tag`'s depth counter round-trips nested brackets, so a title like
/// `Song [Live]` re-parses exactly and keeps its Tier-1 identity (mangling it
/// desynced the cached header from the song on BOTH match tiers: the query
/// side strips the `[Live]` qualifier, a de-bracketed header cannot). Only an
/// UNBALANCED sequence — the case that actually breaks parsing — degrades,
/// to parentheses, which the qualifier-stripper treats identically.
fn header_safe(value: &str) -> String {
    let no_newlines: String = value
        .chars()
        .map(|c| if c == '\r' || c == '\n' { ' ' } else { c })
        .collect();

    let mut depth: i32 = 0;
    let mut balanced = true;
    for c in no_newlines.chars() {
        match c {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth < 0 {
                    balanced = false;
                    break;
                }
            }
            _ => {}
        }
    }
    if balanced && depth == 0 {
        return no_newlines.trim().to_string();
    }
    no_newlines
        .chars()
        .map(|c| match c {
            '[' => '(',
            ']' => ')',
            other => other,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Reduce a string to a filesystem-safe stem, then append a stable hash of the
/// FULL key. Two songs whose readable stems collide (punctuation-only
/// differences that both map to `_`, or content beyond the cap) still get
/// distinct filenames — without the hash they silently overwrote each other's
/// cached lyrics. The hash is deterministic, so re-caching the same song reuses
/// its file rather than accumulating duplicates.
fn sanitize_filename(s: &str) -> String {
    // Cap the stem by BYTES, not chars: 80 CJK chars are ~240 bytes, and with
    // the hash suffix + ".lrc" that overflows Linux's 255-byte NAME_MAX —
    // tokio::fs::write then fails silently (debug-logged) and the cache never
    // persists. 80 bytes + 21-byte suffix stays comfortably inside.
    let mut stem = String::new();
    for c in s.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }) {
        if stem.len() + c.len_utf8() > 80 {
            break;
        }
        stem.push(c);
    }
    // FNV-1a over the full key — a stable, collision-resistant suffix.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{stem}_{hash:016x}")
}

/// Which channel answered a resolve. Carried out of [`resolve_from`] so the
/// boundary log can name the winner without re-deriving it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LyricsChannel {
    /// A `.lrc` the user placed in the lyrics dir themselves.
    Store,
    /// The Navidrome server's `getLyricsBySongId` (sidecar files AND the
    /// lyrics embedded in the file's tags).
    Server,
    /// An LRCLIB download cached under the store's root `.cache/`.
    Cache,
    /// A fresh LRCLIB network fetch.
    Lrclib,
}

impl LyricsChannel {
    /// Stable log token.
    pub fn as_str(self) -> &'static str {
        match self {
            LyricsChannel::Store => "store",
            LyricsChannel::Server => "server",
            LyricsChannel::Cache => "cache",
            LyricsChannel::Lrclib => "lrclib",
        }
    }
}

/// Pure precedence helper, testable without a live `AppService`: the user's own
/// `.lrc` files, then the server (`songLyrics` ext), then LRCLIB downloads
/// already cached on disk, then the LRCLIB network fetch. The server outranks
/// both LRCLIB channels deliberately — a Navidrome answer (a sidecar file or
/// the lyrics embedded in the track's tags) is the user's own library talking,
/// and it wins even when it is plain untimed text. Probes are lazy, so a store
/// hit never touches the disk cache or the network.
///
/// The first hit wins. With no hit, the chain is `Failed` if any channel it
/// asked failed (that channel might have held the lyrics), else a `Miss`.
pub async fn resolve_from<StoreFut, ApiFut, CacheFut, LrclibFut>(
    store: impl FnOnce() -> StoreFut,
    api: impl FnOnce() -> ApiFut,
    cached: impl FnOnce() -> CacheFut,
    lrclib: impl FnOnce() -> LrclibFut,
    opts: ResolveOpts,
) -> LyricsLookup<(LyricsChannel, LrcDocument)>
where
    StoreFut: std::future::Future<Output = LyricsLookup<LrcDocument>>,
    ApiFut: std::future::Future<Output = LyricsLookup<LrcDocument>>,
    CacheFut: std::future::Future<Output = LyricsLookup<LrcDocument>>,
    LrclibFut: std::future::Future<Output = LyricsLookup<LrcDocument>>,
{
    let mut failed = false;
    let mut settle = |lookup: LyricsLookup<LrcDocument>, channel: LyricsChannel| match lookup {
        LyricsLookup::Hit(doc) => Some((channel, doc)),
        LyricsLookup::Miss => None,
        LyricsLookup::Failed => {
            failed = true;
            None
        }
    };

    if let Some(won) = settle(store().await, LyricsChannel::Store) {
        return LyricsLookup::Hit(won);
    }
    if opts.songlyrics_ext
        && let Some(won) = settle(api().await, LyricsChannel::Server)
    {
        return LyricsLookup::Hit(won);
    }
    // The disk cache is local: it answers even with the direct third-party
    // fetch switched off (the download already happened, under consent).
    if let Some(won) = settle(cached().await, LyricsChannel::Cache) {
        return LyricsLookup::Hit(won);
    }
    if opts.fetch_online
        && let Some(won) = settle(lrclib().await, LyricsChannel::Lrclib)
    {
        return LyricsLookup::Hit(won);
    }
    if failed {
        LyricsLookup::Failed
    } else {
        LyricsLookup::Miss
    }
}

/// What the session cache should record for a resolve, if anything: a hit
/// always, a miss only when `complete` (no channel that could answer was
/// skipped for want of the index, the online-fetch consent, or the server's
/// extension probe), a failure never.
pub fn session_verdict(
    lookup: &LyricsLookup<(LyricsChannel, LrcDocument)>,
    complete: bool,
) -> Option<Option<LrcDocument>> {
    match lookup {
        LyricsLookup::Hit((_, doc)) => Some(Some(doc.clone())),
        LyricsLookup::Miss => complete.then_some(None),
        LyricsLookup::Failed => None,
    }
}

/// Mutex-protected half of [`LyricsSessionCache`]: the entries and the
/// generation they belong to, so a compare-and-put is atomic against a drop.
#[derive(Debug)]
struct SessionCacheInner {
    entries: lru::LruCache<String, Option<LrcDocument>>,
    generation: u64,
}

/// Session cache for resolved lyrics, keyed by song id. Holds negatives (a
/// `None` value) too, so a repeated skip past an unmatched track doesn't re-hit
/// the network. Owns its mutex so `AppService` can hand out shared access
/// through one `Arc` and the UI can drop entries when the library changes.
///
/// Every drop bumps a generation. A resolve snapshots it on entry and writes
/// back only if it still matches, so a resolve already in flight when the
/// library changed cannot overwrite the verdict of the resolve sent to replace
/// it. The slow channel is the one that loses that race in practice: a fresh
/// resolve short-circuits at the server while the older one is still waiting
/// out the 5 s LRCLIB timeout, so without the guard the stale miss lands last
/// and the song reads "no lyrics" until restart.
#[derive(Debug)]
pub struct LyricsSessionCache {
    inner: parking_lot::Mutex<SessionCacheInner>,
}

impl LyricsSessionCache {
    /// Build a cache holding at most `capacity` songs (a zero capacity is
    /// raised to one rather than panicking).
    pub fn new(capacity: usize) -> Self {
        let capacity = std::num::NonZeroUsize::new(capacity).unwrap_or(std::num::NonZeroUsize::MIN);
        Self {
            inner: parking_lot::Mutex::new(SessionCacheInner {
                entries: lru::LruCache::new(capacity),
                generation: 0,
            }),
        }
    }

    /// The cached verdict for `song_id`: `Some(Some(doc))` = a hit,
    /// `Some(None)` = a cached miss, `None` = never resolved.
    pub fn get(&self, song_id: &str) -> Option<Option<LrcDocument>> {
        self.inner.lock().entries.get(song_id).cloned()
    }

    /// The live generation, snapshotted by a resolve before it starts.
    pub fn generation(&self) -> u64 {
        self.inner.lock().generation
    }

    /// Record a verdict (a hit or a complete miss, see [`session_verdict`]) iff
    /// no drop has happened since `generation` was taken. Returns whether it
    /// was stored.
    pub fn put_if_current(
        &self,
        generation: u64,
        song_id: String,
        doc: Option<LrcDocument>,
    ) -> bool {
        let mut cache = self.inner.lock();
        if cache.generation != generation {
            return false;
        }
        cache.entries.put(song_id, doc);
        true
    }

    /// Forget everything AND invalidate every resolve in flight (a full
    /// library rescan announces itself as a wildcard event with no ids, so
    /// every song is suspect).
    pub fn drop_all(&self) {
        let mut cache = self.inner.lock();
        cache.entries.clear();
        cache.generation = cache.generation.wrapping_add(1);
    }
}

impl Default for LyricsSessionCache {
    fn default() -> Self {
        Self::new(128)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::lyrics::{LrcDocument, LrcLine};

    fn doc(tag: &str) -> LrcDocument {
        LrcDocument {
            lines: vec![LrcLine {
                time_ms: 0,
                text: tag.into(),
                words: vec![],
            }],
            synced: true,
        }
    }

    const BOTH_ON: ResolveOpts = ResolveOpts {
        songlyrics_ext: true,
        ext_probe_landed: true,
        fetch_online: true,
    };

    #[test]
    fn header_safe_roundtrips_balanced_and_degrades_unbalanced() {
        // BALANCED brackets round-trip verbatim — next_tag's depth counter
        // handles nesting, and preserving them keeps the cached header on the
        // song's exact Tier-1 identity (a qualifier like "[Live]" must remain
        // strippable on both sides of the match).
        assert_eq!(header_safe("Song [Live]"), "Song [Live]");
        let content = format!("[ti:{}]\n[00:01.00]x", header_safe("Song [Live]"));
        assert_eq!(
            crate::types::lyrics::read_metadata(&content)
                .title
                .as_deref(),
            Some("Song [Live]")
        );

        // UNBALANCED brackets (the case that actually breaks parsing) degrade
        // to parentheses; the header still parses, and Tier-2's qualifier
        // strip + reduce treat parens exactly like brackets.
        assert_eq!(header_safe("Song [Live"), "Song (Live");
        let content = format!("[ti:{}]\n[00:01.00]x", header_safe("Song [Live"));
        assert_eq!(
            crate::types::lyrics::read_metadata(&content)
                .title
                .as_deref(),
            Some("Song (Live")
        );
        assert_eq!(header_safe("Song] X"), "Song) X");

        // Newlines always collapse to spaces.
        assert_eq!(header_safe("A\r\nB"), "A  B");
    }

    #[test]
    fn sanitize_filename_distinguishes_colliding_stems() {
        // Two songs whose readable stems collapse identically must not share a
        // cache filename (which silently overwrote the first file's lyrics).
        let a = sanitize_filename("AC/DC-T.N.T.-High Voltage");
        let b = sanitize_filename("AC DC-T N T-High Voltage");
        assert_ne!(a, b);
        // Deterministic: the same key always maps to the same file.
        assert_eq!(a, sanitize_filename("AC/DC-T.N.T.-High Voltage"));
    }

    #[test]
    fn sanitize_filename_stays_under_name_max_for_multibyte() {
        // 80 CJK chars are ~240 bytes — the stem must cap by BYTES so the
        // final name (stem + "_<16 hex>.lrc") stays inside Linux's 255-byte
        // NAME_MAX; otherwise the cache write fails silently for CJK tags.
        let key = "東".repeat(120);
        let name = format!("{}.lrc", sanitize_filename(&key));
        assert!(name.len() <= 255, "filename {} bytes", name.len());
        assert!(name.len() >= 80, "stem should still carry real content");
    }

    #[tokio::test]
    #[ignore]
    async fn live_lrclib_fetch() {
        // Real network: a widely-available track should return synced lyrics.
        let hit = fetch_lrclib("Radiohead", "Creep", "Pablo Honey", 238).await;
        match hit {
            LyricsLookup::Hit((doc, raw)) => {
                eprintln!(
                    "lrclib hit: {} lines, {:?} raw bytes",
                    doc.lines.len(),
                    raw.as_ref().map(String::len)
                );
                assert!(doc.synced && !doc.lines.is_empty());
                assert!(raw.is_some_and(|raw| raw.contains('[')));
            }
            LyricsLookup::Miss => eprintln!("lrclib returned no match (acceptable if not in db)"),
            LyricsLookup::Failed => eprintln!("lrclib fetch failed (acceptable if offline)"),
        }
    }

    fn synced_record(lrc: &str) -> LrclibAnswer {
        LrclibAnswer::Record(LrclibResponse {
            synced_lyrics: Some(lrc.into()),
            ..Default::default()
        })
    }

    /// Run `fetch_lrclib_with` for a song with an album against scripted
    /// answers, by ask number and whether that ask sent the album (`None` = a
    /// request that never answers). Returns what each ask sent.
    async fn fetch_scripted(
        answer: impl FnMut(usize, bool) -> Option<LrclibAnswer>,
    ) -> (LyricsLookup<LrclibHit>, Vec<bool>) {
        fetch_scripted_for(true, answer).await
    }

    async fn fetch_scripted_for(
        has_album: bool,
        mut answer: impl FnMut(usize, bool) -> Option<LrclibAnswer>,
    ) -> (LyricsLookup<LrclibHit>, Vec<bool>) {
        let mut asked = Vec::new();
        let got = fetch_lrclib_with(has_album, |with_album| {
            let scripted = answer(asked.len(), with_album);
            asked.push(with_album);
            async move {
                match scripted {
                    Some(scripted) => scripted,
                    None => std::future::pending().await,
                }
            }
        })
        .await;
        (got, asked)
    }

    #[tokio::test(start_paused = true)]
    async fn a_busy_answer_is_asked_again_after_its_wait() {
        let start = tokio::time::Instant::now();
        let (got, asked) = fetch_scripted(|ask, _| {
            Some(match ask {
                0 => LrclibAnswer::Busy(Some(Duration::from_secs(1))),
                _ => synced_record("[00:01.00]found"),
            })
        })
        .await;
        let (doc, raw) = got.hit().expect("the second ask answers");
        assert_eq!(doc.lines[0].text, "found");
        assert_eq!(
            raw.as_deref(),
            Some("[00:01.00]found"),
            "synced text is cached"
        );
        assert_eq!(asked, [true, true], "a busy ask repeats as it was");
        assert_eq!(start.elapsed(), Duration::from_secs(1));
    }

    #[tokio::test(start_paused = true)]
    async fn a_busy_answer_waits_what_it_asked_and_at_least_a_second() {
        let start = tokio::time::Instant::now();
        let (got, asked) = fetch_scripted(|ask, _| {
            Some(match ask {
                0 => LrclibAnswer::Busy(Some(Duration::ZERO)),
                1 => LrclibAnswer::Busy(None),
                2 => LrclibAnswer::Busy(Some(Duration::from_secs(8))),
                _ => synced_record("[00:01.00]found"),
            })
        })
        .await;
        assert!(got.hit().is_some());
        assert_eq!(asked.len(), 4);
        assert_eq!(
            start.elapsed(),
            Duration::from_secs(1 + 1 + 8),
            "a wait inside the budget is honoured as asked"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_busy_answer_asking_past_the_budget_is_not_asked_again() {
        let start = tokio::time::Instant::now();
        let (got, asked) =
            fetch_scripted(|_, _| Some(LrclibAnswer::Busy(Some(Duration::from_secs(60))))).await;
        assert_eq!(
            (got, asked),
            (LyricsLookup::Failed, vec![true]),
            "a 60 s wait overruns the 12 s budget: give up now, don't ask sooner than told"
        );
        assert_eq!(start.elapsed(), Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn a_server_still_busy_at_the_budget_fails_the_fetch() {
        let start = tokio::time::Instant::now();
        let (got, asked) =
            fetch_scripted(|_, _| Some(LrclibAnswer::Busy(Some(Duration::from_secs(1))))).await;
        assert_eq!(
            got,
            LyricsLookup::Failed,
            "busy says nothing about the song"
        );
        assert!(asked.len() > 1);
        assert!(
            asked.iter().all(|&with_album| with_album),
            "busy never falls back"
        );
        assert!(
            start.elapsed() < LRCLIB_BUDGET,
            "gave up at {:?}, before a wait that would end past the budget",
            start.elapsed()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_hung_request_fails_at_the_request_timeout_and_is_not_retried() {
        let start = tokio::time::Instant::now();
        let (got, asked) = fetch_scripted(|_, _| None).await;
        assert_eq!(got, LyricsLookup::Failed);
        assert_eq!(asked, [true]);
        assert_eq!(start.elapsed(), LRCLIB_TIMEOUT);
    }

    #[tokio::test(start_paused = true)]
    async fn not_found_either_way_is_a_miss_and_any_other_failure_fails() {
        let (got, asked) = fetch_scripted(|_, _| Some(LrclibAnswer::NotFound)).await;
        assert_eq!((got, asked), (LyricsLookup::Miss, vec![true, false]));
        let (got, asked) = fetch_scripted(|_, _| Some(LrclibAnswer::Failed)).await;
        assert_eq!(
            (got, asked),
            (LyricsLookup::Failed, vec![true]),
            "a failed ask never falls back"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn no_record_under_the_album_asks_again_without_it() {
        let (got, asked) = fetch_scripted(|_, with_album| {
            Some(if with_album {
                LrclibAnswer::NotFound
            } else {
                synced_record("[00:01.00]other album")
            })
        })
        .await;
        let (doc, _) = got.hit().expect("the album-less ask answers");
        assert_eq!(doc.lines[0].text, "other album");
        assert_eq!(asked, [true, false]);
    }

    #[tokio::test(start_paused = true)]
    async fn an_unsynced_record_under_the_album_asks_again_without_it() {
        let (got, asked) = fetch_scripted(|_, with_album| {
            Some(if with_album {
                LrclibAnswer::Record(LrclibResponse::default())
            } else {
                synced_record("[00:01.00]synced elsewhere")
            })
        })
        .await;
        assert_eq!(
            got.hit().expect("synced wins").0.lines[0].text,
            "synced elsewhere"
        );
        assert_eq!(asked, [true, false]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_synced_record_or_an_instrumental_under_the_album_is_final() {
        let (got, asked) = fetch_scripted(|_, _| Some(synced_record("[00:01.00]x"))).await;
        assert!(got.hit().is_some());
        assert_eq!(asked, [true]);

        let (got, asked) = fetch_scripted(|_, _| {
            Some(LrclibAnswer::Record(LrclibResponse {
                instrumental: true,
                plain_lyrics: Some("stray text".into()),
                ..Default::default()
            }))
        })
        .await;
        assert_eq!((got, asked), (LyricsLookup::Miss, vec![true]));
    }

    #[tokio::test(start_paused = true)]
    async fn a_song_without_an_album_asks_once() {
        let (got, asked) = fetch_scripted_for(false, |_, _| Some(LrclibAnswer::NotFound)).await;
        assert_eq!((got, asked), (LyricsLookup::Miss, vec![false]));
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_album_less_ask_fails_the_fetch() {
        let (got, asked) = fetch_scripted(|_, with_album| {
            Some(if with_album {
                LrclibAnswer::NotFound
            } else {
                LrclibAnswer::Failed
            })
        })
        .await;
        assert_eq!((got, asked), (LyricsLookup::Failed, vec![true, false]));
    }

    fn plain_record(text: &str) -> LrclibAnswer {
        LrclibAnswer::Record(LrclibResponse {
            plain_lyrics: Some(text.into()),
            ..Default::default()
        })
    }

    /// The hit's lines, and whether it would be cached to disk.
    fn plain_hit(got: LyricsLookup<LrclibHit>) -> (Vec<String>, bool, bool) {
        let (doc, raw) = got.hit().expect("a hit");
        let lines = doc.lines.into_iter().map(|l| l.text).collect();
        (lines, doc.synced, raw.is_some())
    }

    #[tokio::test(start_paused = true)]
    async fn plain_lyrics_show_when_nothing_synced_is_anywhere() {
        let (got, asked) = fetch_scripted(|_, with_album| {
            Some(if with_album {
                plain_record("first line\n\nsecond line")
            } else {
                LrclibAnswer::NotFound
            })
        })
        .await;
        assert_eq!(
            asked,
            [true, false],
            "the album-less ask may still be synced"
        );
        assert_eq!(
            plain_hit(got),
            (
                vec!["first line".into(), "second line".into()],
                false,
                false
            ),
            "an untimed sheet, never cached to disk"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn synced_without_the_album_beats_plain_under_it() {
        let (got, _) = fetch_scripted(|_, with_album| {
            Some(if with_album {
                plain_record("plain")
            } else {
                synced_record("[00:01.00]synced")
            })
        })
        .await;
        assert_eq!(plain_hit(got), (vec!["synced".into()], true, true));
    }

    #[tokio::test(start_paused = true)]
    async fn the_albums_own_plain_text_beats_another_albums() {
        let (got, _) = fetch_scripted(|_, with_album| {
            Some(plain_record(if with_album { "ours" } else { "theirs" }))
        })
        .await;
        assert_eq!(plain_hit(got).0, ["ours"]);

        let (got, _) = fetch_scripted(|_, with_album| {
            Some(if with_album {
                LrclibAnswer::NotFound
            } else {
                plain_record("theirs")
            })
        })
        .await;
        assert_eq!(plain_hit(got).0, ["theirs"], "plain from either ask shows");
    }

    #[tokio::test(start_paused = true)]
    async fn unstamped_synced_text_reads_as_its_plain_lyrics() {
        let (got, _) = fetch_scripted_for(false, |_, _| {
            Some(LrclibAnswer::Record(LrclibResponse {
                synced_lyrics: Some("no stamps here".into()),
                plain_lyrics: Some("the plain copy".into()),
                ..Default::default()
            }))
        })
        .await;
        assert_eq!(
            plain_hit(got),
            (vec!["the plain copy".into()], false, false)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_album_less_ask_keeps_the_plain_sheet_in_hand() {
        let (got, _) = fetch_scripted(|_, with_album| {
            Some(if with_album {
                plain_record("in hand")
            } else {
                LrclibAnswer::Failed
            })
        })
        .await;
        assert_eq!(plain_hit(got).0, ["in hand"]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_song_without_an_album_shows_plain_lyrics_too() {
        let (got, asked) = fetch_scripted_for(false, |_, _| Some(plain_record("plain"))).await;
        assert_eq!(asked, [false]);
        assert_eq!(plain_hit(got).0, ["plain"]);
    }

    #[test]
    fn retry_after_reads_whole_seconds_only() {
        let mut headers = reqwest::header::HeaderMap::new();
        assert_eq!(retry_after_secs(&headers), None);
        headers.insert(reqwest::header::RETRY_AFTER, "1".parse().expect("header"));
        assert_eq!(retry_after_secs(&headers), Some(Duration::from_secs(1)));
        headers.insert(
            reqwest::header::RETRY_AFTER,
            "Wed, 21 Oct 2026 07:28:00 GMT".parse().expect("header"),
        );
        assert_eq!(retry_after_secs(&headers), None);
    }

    /// `(channel token, first line's text)` — the two things every precedence
    /// test asserts.
    fn won(result: LyricsLookup<(LyricsChannel, LrcDocument)>) -> (&'static str, String) {
        let (channel, doc) = result.hit().expect("a channel must answer");
        (channel.as_str(), doc.lines[0].text.clone())
    }

    #[tokio::test]
    async fn store_hit_wins() {
        // The user's own file ends the chain: nothing later even runs.
        let result = resolve_from(
            || async { LyricsLookup::Hit(doc("store")) },
            || async { panic!("the server must not run after a store hit") },
            || async { panic!("the cache must not run after a store hit") },
            || async { panic!("lrclib must not run after a store hit") },
            BOTH_ON,
        )
        .await;
        assert_eq!(won(result), ("store", "store".to_string()));
    }

    #[tokio::test]
    async fn server_beats_the_cached_download() {
        // The owner's rule: a Navidrome answer outranks LRCLIB, including a
        // copy already sitting in `.cache/`. A server hit runs neither.
        let result = resolve_from(
            || async { LyricsLookup::Miss },
            || async { LyricsLookup::Hit(doc("api")) },
            || async { panic!("the cache must not run after a server hit") },
            || async { panic!("lrclib must not run after a server hit") },
            BOTH_ON,
        )
        .await;
        assert_eq!(won(result), ("server", "api".to_string()));
    }

    #[tokio::test]
    async fn cached_download_beats_the_network() {
        let result = resolve_from(
            || async { LyricsLookup::Miss },
            || async { LyricsLookup::Miss },
            || async { LyricsLookup::Hit(doc("cache")) },
            || async { panic!("lrclib must not run after a cache hit") },
            BOTH_ON,
        )
        .await;
        assert_eq!(won(result), ("cache", "cache".to_string()));
    }

    #[tokio::test]
    async fn lrclib_runs_last() {
        let result = resolve_from(
            || async { LyricsLookup::Miss },
            || async { LyricsLookup::Miss },
            || async { LyricsLookup::Miss },
            || async { LyricsLookup::Hit(doc("lrclib")) },
            BOTH_ON,
        )
        .await;
        assert_eq!(won(result), ("lrclib", "lrclib".to_string()));
    }

    #[tokio::test]
    async fn api_skipped_without_ext() {
        let opts = ResolveOpts {
            songlyrics_ext: false,
            ext_probe_landed: true,
            fetch_online: true,
        };
        let result = resolve_from(
            || async { LyricsLookup::Miss },
            || async { panic!("api must not run without the songLyrics ext") },
            || async { LyricsLookup::Hit(doc("cache")) },
            || async { panic!("lrclib must not run after a cache hit") },
            opts,
        )
        .await;
        assert_eq!(won(result), ("cache", "cache".to_string()));
    }

    #[tokio::test]
    async fn cached_channel_runs_offline_but_the_network_one_does_not() {
        // Online Fetch off gates the third-party REQUEST, not the download the
        // user already consented to and that now sits on their disk.
        let opts = ResolveOpts {
            songlyrics_ext: true,
            ext_probe_landed: true,
            fetch_online: false,
        };
        let result = resolve_from(
            || async { LyricsLookup::Miss },
            || async { LyricsLookup::Miss },
            || async { LyricsLookup::Hit(doc("cache")) },
            || async { panic!("lrclib must not run when fetch_online is off") },
            opts,
        )
        .await;
        assert_eq!(won(result), ("cache", "cache".to_string()));

        let miss = resolve_from(
            || async { LyricsLookup::Miss },
            || async { LyricsLookup::Miss },
            || async { LyricsLookup::Miss },
            || async { panic!("lrclib must not run when fetch_online is off") },
            opts,
        )
        .await;
        assert_eq!(miss, LyricsLookup::Miss);
    }

    #[tokio::test]
    async fn a_failed_channel_makes_the_miss_a_failure() {
        // The server timed out: it might hold the lyrics, so the chain's
        // empty-handed end says nothing about the song.
        let result = resolve_from(
            || async { LyricsLookup::Miss },
            || async { LyricsLookup::Failed },
            || async { LyricsLookup::Miss },
            || async { LyricsLookup::Miss },
            BOTH_ON,
        )
        .await;
        assert_eq!(result, LyricsLookup::Failed);
    }

    #[tokio::test]
    async fn a_later_hit_wins_over_an_earlier_failure() {
        let result = resolve_from(
            || async { LyricsLookup::Failed },
            || async { LyricsLookup::Failed },
            || async { LyricsLookup::Miss },
            || async { LyricsLookup::Hit(doc("lrclib")) },
            BOTH_ON,
        )
        .await;
        assert_eq!(won(result), ("lrclib", "lrclib".to_string()));
    }

    #[tokio::test]
    async fn a_skipped_channel_cannot_fail_the_chain() {
        // Only channels the chain asks count: lrclib is off, so its failure
        // never runs and the miss stands.
        let opts = ResolveOpts {
            fetch_online: false,
            ..BOTH_ON
        };
        let result = resolve_from(
            || async { LyricsLookup::Miss },
            || async { LyricsLookup::Miss },
            || async { LyricsLookup::Miss },
            || async { LyricsLookup::Failed },
            opts,
        )
        .await;
        assert_eq!(result, LyricsLookup::Miss);
    }

    #[test]
    fn the_session_records_hits_and_complete_misses_only() {
        let hit = LyricsLookup::Hit((LyricsChannel::Server, doc("api")));
        for complete in [true, false] {
            assert_eq!(session_verdict(&hit, complete), Some(Some(doc("api"))));
            assert_eq!(
                session_verdict(&LyricsLookup::Failed, complete),
                None,
                "a failed lookup is never remembered as no lyrics"
            );
        }
        assert_eq!(session_verdict(&LyricsLookup::Miss, true), Some(None));
        assert_eq!(session_verdict(&LyricsLookup::Miss, false), None);
    }

    #[test]
    fn session_cache_gets_puts_and_drops() {
        let cache = LyricsSessionCache::new(4);
        assert!(cache.get("s1").is_none(), "never resolved");

        let generation = cache.generation();
        assert!(cache.put_if_current(generation, "s1".to_string(), Some(doc("hit"))));
        assert!(cache.put_if_current(generation, "s2".to_string(), None));
        assert_eq!(
            cache.get("s1").flatten().map(|d| d.lines[0].text.clone()),
            Some("hit".to_string())
        );
        assert!(
            matches!(cache.get("s2"), Some(None)),
            "a cached miss is distinguishable from never-resolved"
        );

        cache.drop_all();
        assert!(cache.get("s1").is_none(), "a drop clears everything");
        assert!(cache.get("s2").is_none());
    }

    #[test]
    fn a_resolve_from_before_a_drop_cannot_write_back() {
        // The rescan race: a slow resolve (waiting out the 5 s LRCLIB timeout)
        // started before the wildcard event; the fresh one sent to replace it
        // short-circuits at the server and stores the new lyrics. The stale one
        // lands last and must be refused, or the song reads "no lyrics" for the
        // rest of the session — the very failure the drop was added to fix.
        let cache = LyricsSessionCache::new(4);
        let stale = cache.generation();

        cache.drop_all();
        let fresh = cache.generation();
        assert!(cache.put_if_current(fresh, "s1".to_string(), Some(doc("fresh"))));

        assert!(
            !cache.put_if_current(stale, "s1".to_string(), None),
            "a pre-drop resolve must not write back"
        );
        assert_eq!(
            cache.get("s1").flatten().map(|d| d.lines[0].text.clone()),
            Some("fresh".to_string()),
            "the post-drop verdict survives"
        );
    }

    #[tokio::test]
    async fn cache_roundtrips_through_index() {
        // A cached LRCLIB result must be re-indexable by the song's tags.
        let tmp = tempfile::tempdir().expect("tempdir");
        // Point the cache writer at a temp store via a direct write mirroring
        // cache_to_store's header synthesis, then build_index over it.
        let cache_dir = tmp.path().join(".cache");
        std::fs::create_dir_all(&cache_dir).expect("mkdir");
        let song = Song {
            id: "s1".into(),
            title: "Myth".into(),
            artist: "Beach House".into(),
            album: "Bloom".into(),
            duration: 275,
            ..Default::default()
        };
        let raw = "[00:45.47]Drifting in and out";
        let mut content = String::new();
        content.push_str(&format!(
            "[ar:{}]\n[ti:{}]\n[al:{}]\n",
            song.artist, song.title, song.album
        ));
        content.push_str(&format!(
            "[length:{:02}:{:02}]\n",
            song.duration / 60,
            song.duration % 60
        ));
        content.push_str(raw);
        std::fs::write(
            cache_dir.join(format!(
                "{}.lrc",
                sanitize_filename("Beach House-Myth-Bloom")
            )),
            content,
        )
        .expect("write");

        let index = crate::types::lyrics::build_index(tmp.path().to_path_buf()).await;
        assert!(
            index
                .find_cached(&song.artist, &song.title, Some(&song.album), None)
                .is_some(),
            "synthetic headers must make the cache tag-matchable"
        );
        assert!(
            index
                .find_user(&song.artist, &song.title, Some(&song.album), None)
                .is_none(),
            "a `.cache/` download is never the user's own file"
        );
    }
}
