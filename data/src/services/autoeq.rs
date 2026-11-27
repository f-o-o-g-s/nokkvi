//! AutoEq headphone profiles: the list of measured headphones the AutoEq
//! project publishes (`results/INDEX.md` in `jaakkopasanen/AutoEq` on GitHub),
//! one headphone's `ParametricEQ.txt`, the search the equalizer's AutoEq panel
//! runs over the list, and the copies kept under `~/.cache/nokkvi/autoeq/`.
//!
//! Nothing here runs on its own. The list is read only when the user opens the
//! panel, a profile only when they pick a row; nothing is fetched at startup.
//! Every request is a plain GET to [`RESULTS_URL`]'s host carrying nokkvi's
//! name and version as its user agent and nothing that tells who asks: no
//! cookies (the client keeps none), no query string, no account. A redirect
//! off that host is not followed. Each request has a total time limit, and
//! its answer is cut off while it streams once it passes its size cap.
//!
//! The index line format, the profile URL and the cache file names are ported
//! from Pawse's `crates/autoeq/src/lib.rs`. Changed here: a path must build a
//! profile URL on the results host exactly as written (or the line is
//! skipped), a cache name also escapes `%` so two paths never share a file,
//! and the list and the profiles are cached here with a freshness rule for the
//! list.
//!
//! MIT License
//!
//! Copyright (c) 2026 Aleksandr Popov
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy
//! of this software and associated documentation files (the "Software"), to deal
//! in the Software without restriction, including without limitation the rights
//! to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
//! copies of the Software, and to permit persons to whom the Software is
//! furnished to do so, subject to the following conditions:
//!
//! The above copyright notice and this permission notice shall be included in all
//! copies or substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//! IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//! FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
//! AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//! LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
//! OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
//! SOFTWARE.

use std::{
    future::Future,
    io::Read as _,
    path::{Path, PathBuf},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};

use anyhow::{Context as _, Result, anyhow, bail};
use tracing::{debug, warn};

use crate::audio::eq::{EqBand, apo};

/// Where AutoEq keeps its results; the list and every profile live under it.
pub const RESULTS_URL: &str =
    "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/results/";
/// The one host requests go to.
const RESULTS_HOST: &str = "raw.githubusercontent.com";
const INDEX_FILE: &str = "INDEX.md";
/// A profile's file name after its folder's: `<folder> ParametricEQ.txt`.
const PROFILE_SUFFIX: &str = "%20ParametricEQ.txt";

/// Most the list's answer may hold (it is about 850 KiB in 2026).
pub const MAX_INDEX_BYTES: usize = 16 * 1024 * 1024;
/// Most a profile's answer may hold (AutoEq's are under 1 KiB).
pub const MAX_PROFILE_BYTES: usize = 64 * 1024;
/// A cached list younger than this is used without asking GitHub; an older
/// one is downloaded again, and still used when that fails.
pub const INDEX_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// The whole list request, connect to last byte.
const INDEX_TIMEOUT: Duration = Duration::from_secs(60);
/// The whole profile request, connect to last byte.
const PROFILE_TIMEOUT: Duration = Duration::from_secs(20);
/// A stalled answer (no bytes for this long) is given up.
const READ_TIMEOUT: Duration = Duration::from_secs(15);

/// Longest cache file name (bytes), well under the 255 a Linux file system
/// takes, so the temp name written beside it fits too.
const MAX_CACHE_NAME_BYTES: usize = 200;
/// The characters a cache file name may hold besides ASCII letters and
/// digits: what the index's percent-encoded paths use, nothing a file system
/// reads as a separator.
const CACHE_NAME_CHARS: &str = "%-_.()&,+'!$";

const CACHE_INDEX_FILE: &str = "index.md";
const CACHE_PROFILES_DIR: &str = "profiles";

/// One measurement in the AutoEq list: a headphone, who measured it and on
/// what rig, and where its results live under [`RESULTS_URL`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoEqEntry {
    /// The headphone (`Sennheiser HD 600`).
    pub name: String,
    /// Who measured it (`oratory1990`, `crinacle`).
    pub source: String,
    /// The measurement rig, when the index names one (`GRAS 43AG-7`).
    pub rig: Option<String>,
    /// The results folder under [`RESULTS_URL`], percent-encoded as the
    /// index writes it (`oratory1990/over-ear/Sennheiser%20HD%20600`).
    pub path: String,
}

impl AutoEqEntry {
    /// Who measured it and on what: `crinacle on GRAS 43AG-7`, `oratory1990`.
    pub fn by_line(&self) -> String {
        match &self.rig {
            Some(rig) => format!("{} on {rig}", self.source),
            None => self.source.clone(),
        }
    }

    /// The profile's URL: `<results>/<path>/<last path segment>
    /// ParametricEQ.txt`. `None` when the path doesn't make a URL on the
    /// results host exactly as written (a query, a fragment, a dot segment,
    /// an unencoded character): the index comes from the network.
    pub fn profile_url(&self) -> Option<String> {
        let folder = self.path.rsplit('/').next().unwrap_or_default();
        let url = format!("{RESULTS_URL}{}/{folder}{PROFILE_SUFFIX}", self.path);
        on_results_host(&url).then_some(url)
    }

    /// The file name the profile is cached under: the path with `%` and `/`
    /// escaped (`%25`, `%2F`) plus `.txt`, so it is flat and no two paths
    /// share it. `None` (the profile is not cached) when it holds a character
    /// outside a small set every file system takes, or is too long.
    pub fn cache_name(&self) -> Option<String> {
        let name = format!("{}.txt", self.path.replace('%', "%25").replace('/', "%2F"));
        let safe = name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || CACHE_NAME_CHARS.contains(c));
        (safe && name.len() <= MAX_CACHE_NAME_BYTES).then_some(name)
    }
}

/// The list's URL.
pub fn index_url() -> String {
    format!("{RESULTS_URL}{INDEX_FILE}")
}

/// Whether `url` is an https URL on the results host, under the results
/// folder, with no query or fragment, and reads back exactly as written (the
/// URL parser normalized nothing away, such as a `%2e%2e` segment).
fn on_results_host(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    parsed.scheme() == "https"
        && parsed.host_str() == Some(RESULTS_HOST)
        && parsed.query().is_none()
        && parsed.fragment().is_none()
        && parsed.as_str() == url
        && url.starts_with(RESULTS_URL)
}

/// Every measurement the index lists, in its order (by name, and within a
/// name AutoEq's own preference of sources). An index line is
/// `- [Name](./source/rig/Folder) by Source[ on Rig]`; lines that don't fit
/// are skipped, and so is any path with `..` in it or one that doesn't make a
/// profile URL on the results host.
pub fn parse_index(text: &str) -> Vec<AutoEqEntry> {
    text.lines().filter_map(parse_line).collect()
}

fn parse_line(line: &str) -> Option<AutoEqEntry> {
    let rest = line.trim().strip_prefix("- [")?;
    let (name, rest) = rest.split_once("](")?;
    // Up to the LAST `) by `: model names have parentheses, and so do their
    // folders.
    let (path, by) = rest.rsplit_once(") by ")?;
    let path = path.strip_prefix("./")?.trim_end_matches('/');
    let name = name.trim();
    if name.is_empty() || path.is_empty() || path.contains("..") {
        return None;
    }
    let by = by.trim();
    let (source, rig) = match by.split_once(" on ") {
        Some((source, rig)) => (source.trim(), Some(rig.trim().to_owned())),
        None => (by, None),
    };
    if source.is_empty() {
        return None;
    }
    let entry = AutoEqEntry {
        name: name.to_owned(),
        source: source.to_owned(),
        rig: rig.filter(|rig| !rig.is_empty()),
        path: path.to_owned(),
    };
    entry.profile_url().is_some().then_some(entry)
}

// =============================================================================
// Search
// =============================================================================

/// What a search found: the entries to show (indices into the list, best
/// first, at most the limit) and how many matched in all.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutoEqMatches {
    pub shown: Vec<usize>,
    pub matched: usize,
}

/// How a name meets the query; the order is the result order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    Exact,
    Prefix,
    Substring,
}

/// The entries that match `query`, best first. Case-insensitive; every
/// whitespace-separated term must occur in the name or the source. A name
/// that is the query beats one that starts with it, which beats one that
/// only contains the terms; then by name, and equal names keep the list's
/// order. An empty query matches everything, in the list's order.
pub fn search(entries: &[AutoEqEntry], query: &str, limit: usize) -> AutoEqMatches {
    let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let phrase = terms.join(" ");
    let mut hits: Vec<(Rank, String, usize)> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let name = entry
            .name
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        if !terms.is_empty() {
            let source = entry.source.to_lowercase();
            if !terms
                .iter()
                .all(|term| name.contains(term.as_str()) || source.contains(term.as_str()))
            {
                continue;
            }
        }
        let rank = if name == phrase {
            Rank::Exact
        } else if name.starts_with(&phrase) {
            Rank::Prefix
        } else {
            Rank::Substring
        };
        hits.push((rank, name, index));
    }
    // Stable: equal names keep the index's order of sources.
    hits.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    AutoEqMatches {
        matched: hits.len(),
        shown: hits
            .into_iter()
            .take(limit)
            .map(|(_, _, index)| index)
            .collect(),
    }
}

// =============================================================================
// Cache
// =============================================================================

/// The list and the profiles on disk: `index.md` and `profiles/<cache
/// name>`, written through a temp file and a rename.
#[derive(Debug, Clone)]
pub struct AutoEqCache {
    root: PathBuf,
}

/// The cached list's text, and whether it is younger than [`INDEX_MAX_AGE`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedIndex {
    pub text: String,
    pub fresh: bool,
}

/// Whether a list written at `modified` is still fresh at `now`. A time in
/// the future (the clock went back) is not: the list is downloaded again.
pub fn index_is_fresh(modified: SystemTime, now: SystemTime) -> bool {
    now.duration_since(modified)
        .is_ok_and(|age| age < INDEX_MAX_AGE)
}

impl AutoEqCache {
    /// A cache kept under `root` (the app's is `paths::get_autoeq_dir`).
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn index_path(&self) -> PathBuf {
        self.root.join(CACHE_INDEX_FILE)
    }

    fn profile_path(&self, entry: &AutoEqEntry) -> Option<PathBuf> {
        entry
            .cache_name()
            .map(|name| self.root.join(CACHE_PROFILES_DIR).join(name))
    }

    /// The cached list, or `None` when there is none (or it can't be read,
    /// or is over [`MAX_INDEX_BYTES`]).
    pub fn read_index(&self, now: SystemTime) -> Option<CachedIndex> {
        let path = self.index_path();
        let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
        let text = read_capped_file(&path, MAX_INDEX_BYTES)?;
        Some(CachedIndex {
            text,
            fresh: index_is_fresh(modified, now),
        })
    }

    pub fn write_index(&self, text: &str) -> Result<()> {
        write_file(&self.index_path(), text.as_bytes())
    }

    /// The cached profile text of `entry`, if it has one.
    pub fn read_profile(&self, entry: &AutoEqEntry) -> Option<String> {
        read_capped_file(&self.profile_path(entry)?, MAX_PROFILE_BYTES)
    }

    /// Keep `text` as `entry`'s profile. `Ok(false)`, nothing written, for an
    /// entry whose path makes no safe file name.
    pub fn write_profile(&self, entry: &AutoEqEntry, text: &str) -> Result<bool> {
        match self.profile_path(entry) {
            Some(path) => write_file(&path, text.as_bytes()).map(|()| true),
            None => Ok(false),
        }
    }
}

/// A file's text, or `None` when it is missing, unreadable, over `cap`
/// bytes or not UTF-8.
fn read_capped_file(path: &Path, cap: usize) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    // One byte past the cap tells an oversized file from one at the cap.
    let limit = u64::try_from(cap).unwrap_or(u64::MAX).saturating_add(1);
    file.take(limit).read_to_end(&mut bytes).ok()?;
    if bytes.len() > cap {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// Write `bytes` to `path` through a temp file beside it and a rename, so a
/// reader never sees half a file. Each write has a temp name of its own.
fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    static WRITES: AtomicU64 = AtomicU64::new(0);
    let dir = path
        .parent()
        .ok_or_else(|| anyhow!("{} has no folder", path.display()))?;
    std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    let tmp = path.with_extension(format!(
        "{}.{}.tmp",
        std::process::id(),
        WRITES.fetch_add(1, Ordering::Relaxed)
    ));
    if let Err(e) = std::fs::write(&tmp, bytes) {
        discard_temp(&tmp);
        return Err(e).with_context(|| format!("write {}", tmp.display()));
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        discard_temp(&tmp);
        anyhow!(e).context(format!("rename to {}", path.display()))
    })
}

/// Remove a temp file a failed write left (there may be none).
fn discard_temp(tmp: &Path) {
    if let Err(e) = std::fs::remove_file(tmp)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        debug!("AutoEq: couldn't remove {}: {e}", tmp.display());
    }
}

// =============================================================================
// Loading (the cache, then the network)
// =============================================================================

/// The list: the cached copy while it is fresh, else `fetch`'s download
/// (cached when it parses), else a stale cached copy when the download fails.
/// `force` skips the cache and asks `fetch` (the panel's Refresh list), and
/// its failure is the error. A download with no entries in it is an error
/// (an error page that slipped through), never cached.
pub async fn load_index<F, Fut>(
    cache: Option<&AutoEqCache>,
    force: bool,
    now: SystemTime,
    fetch: F,
) -> Result<Vec<AutoEqEntry>>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<String>>,
{
    let cached = if force {
        None
    } else {
        cache.and_then(|cache| cache.read_index(now))
    };
    if let Some(cached) = cached.as_ref().filter(|cached| cached.fresh) {
        let entries = parse_index(&cached.text);
        if !entries.is_empty() {
            return Ok(entries);
        }
    }
    let downloaded = fetch().await.and_then(|text| {
        let entries = parse_index(&text);
        if entries.is_empty() {
            bail!("the list has no headphones in it");
        }
        Ok((text, entries))
    });
    match downloaded {
        Ok((text, entries)) => {
            if let Some(cache) = cache
                && let Err(e) = cache.write_index(&text)
            {
                warn!("AutoEq: couldn't keep the list: {e:#}");
            }
            Ok(entries)
        }
        Err(e) => {
            // An older copy still serves when the download fails.
            if let Some(cached) = cached {
                let entries = parse_index(&cached.text);
                if !entries.is_empty() {
                    warn!("AutoEq: the list couldn't be refreshed, using the cached one: {e:#}");
                    return Ok(entries);
                }
            }
            Err(e)
        }
    }
}

/// `entry`'s bands: the cached profile while it still parses, else `fetch`'s
/// download, which is cached only once it parses. A download that doesn't
/// parse is an error with the reason, never a partial list
/// (`apo::parse` refuses the whole file).
pub async fn load_profile<F, Fut>(
    cache: Option<&AutoEqCache>,
    entry: &AutoEqEntry,
    fetch: F,
) -> Result<Vec<EqBand>>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<String>>,
{
    if let Some(text) = cache.and_then(|cache| cache.read_profile(entry)) {
        match apo::parse(&text) {
            Ok(bands) => return Ok(bands),
            Err(why) => {
                debug!("AutoEq: the cached profile no longer reads ({why}), downloading it");
            }
        }
    }
    let text = fetch().await?;
    let bands = apo::parse(&text).map_err(|why| anyhow!("its profile can't be used ({why})"))?;
    if let Some(cache) = cache {
        match cache.write_profile(entry, &text) {
            Ok(true) => {}
            Ok(false) => debug!("AutoEq: {} has no safe cache name, not cached", entry.path),
            Err(e) => warn!("AutoEq: couldn't keep a profile: {e:#}"),
        }
    }
    Ok(bands)
}

// =============================================================================
// Network
// =============================================================================

/// The user agent of every AutoEq request: nokkvi's name and version.
pub fn user_agent(app_version: &str) -> String {
    format!("nokkvi/{app_version}")
}

/// One client for every AutoEq request: it follows a redirect only while it
/// stays on the same host (a hop elsewhere comes back as the answer, which
/// reads as a failure), drops a stalled answer, and keeps no cookies.
fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        crate::utils::http::client_base()
            .read_timeout(READ_TIMEOUT)
            .redirect(crate::utils::redirects::server_only())
            .build()
            .expect("Failed to build the AutoEq HTTP client")
    })
}

/// A response body collected up to a cap: a push past it is an error, so an
/// answer is refused while it streams in, not after.
#[derive(Debug)]
struct CappedBody {
    bytes: Vec<u8>,
    cap: usize,
}

impl CappedBody {
    fn new(cap: usize) -> Self {
        Self {
            bytes: Vec::new(),
            cap,
        }
    }

    fn push(&mut self, chunk: &[u8]) -> Result<()> {
        if self.bytes.len().saturating_add(chunk.len()) > self.cap {
            bail!("the answer is over {}", size_label(self.cap));
        }
        self.bytes.extend_from_slice(chunk);
        Ok(())
    }

    fn into_text(self) -> Result<String> {
        String::from_utf8(self.bytes).map_err(|_| anyhow!("the answer isn't text"))
    }
}

/// `16 MiB`, `64 KiB`.
fn size_label(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{} MiB", bytes / (1024 * 1024))
    } else {
        format!("{} KiB", bytes / 1024)
    }
}

/// GET `url` (on the results host only) and read its text, at most `cap`
/// bytes, within `total`. The errors are written for the panel and a toast.
async fn fetch_text(url: &str, cap: usize, total: Duration, user_agent: &str) -> Result<String> {
    if !on_results_host(url) {
        bail!("not an AutoEq address");
    }
    let unreachable = |e: reqwest::Error| {
        if e.is_timeout() {
            anyhow!("GitHub didn't answer in time")
        } else {
            anyhow!("couldn't reach GitHub ({})", e.without_url())
        }
    };
    let mut response = client()
        .get(url)
        .header(reqwest::header::USER_AGENT, user_agent)
        .timeout(total)
        .send()
        .await
        .map_err(unreachable)?;
    let status = response.status();
    if !status.is_success() {
        bail!("GitHub answered HTTP {}", status.as_u16());
    }
    if response
        .content_length()
        .is_some_and(|len| len > u64::try_from(cap).unwrap_or(u64::MAX))
    {
        bail!("the answer is over {}", size_label(cap));
    }
    let mut body = CappedBody::new(cap);
    while let Some(chunk) = response.chunk().await.map_err(unreachable)? {
        body.push(&chunk)?;
    }
    body.into_text()
}

// =============================================================================
// The service AppService holds
// =============================================================================

/// The AutoEq side of a session: the cache, and the list once read, kept for
/// the session so the panel's next open shows it at once.
#[derive(Debug)]
pub struct AutoEqService {
    cache: Option<AutoEqCache>,
    session: parking_lot::Mutex<Option<Arc<Vec<AutoEqEntry>>>>,
}

impl Default for AutoEqService {
    fn default() -> Self {
        Self::new()
    }
}

impl AutoEqService {
    /// Resolves the cache folder only: nothing is read or fetched until the
    /// panel asks.
    pub fn new() -> Self {
        let cache = match crate::utils::paths::get_autoeq_dir() {
            Ok(dir) => Some(AutoEqCache::at(dir)),
            Err(e) => {
                warn!("AutoEq: no cache folder, downloads won't be kept: {e:#}");
                None
            }
        };
        Self {
            cache,
            session: parking_lot::Mutex::new(None),
        }
    }

    /// The list this session already read, if it read one.
    pub fn session_index(&self) -> Option<Arc<Vec<AutoEqEntry>>> {
        self.session.lock().clone()
    }

    /// The list: this session's copy, else [`load_index`] over the cache and
    /// GitHub. `force_refresh` downloads it again.
    pub async fn index(
        &self,
        force_refresh: bool,
        app_version: &str,
    ) -> Result<Arc<Vec<AutoEqEntry>>> {
        let agent = user_agent(app_version);
        let url = index_url();
        self.index_with(force_refresh, || {
            fetch_text(&url, MAX_INDEX_BYTES, INDEX_TIMEOUT, &agent)
        })
        .await
    }

    /// [`Self::index`] with `fetch` standing in for the download.
    async fn index_with<F, Fut>(
        &self,
        force_refresh: bool,
        fetch: F,
    ) -> Result<Arc<Vec<AutoEqEntry>>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<String>>,
    {
        if !force_refresh && let Some(list) = self.session_index() {
            return Ok(list);
        }
        let entries =
            load_index(self.cache.as_ref(), force_refresh, SystemTime::now(), fetch).await?;
        let list = Arc::new(entries);
        *self.session.lock() = Some(Arc::clone(&list));
        Ok(list)
    }

    /// `entry`'s bands through [`load_profile`] (the cache, then GitHub).
    pub async fn profile(&self, entry: &AutoEqEntry, app_version: &str) -> Result<Vec<EqBand>> {
        let url = entry
            .profile_url()
            .ok_or_else(|| anyhow!("its address isn't an AutoEq one"))?;
        let agent = user_agent(app_version);
        load_profile(self.cache.as_ref(), entry, || {
            fetch_text(&url, MAX_PROFILE_BYTES, PROFILE_TIMEOUT, &agent)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::*;

    /// Lines from the real index (2026-11-27), plus lines that must be
    /// skipped.
    const INDEX: &str = "# Index
This is a list of all equalization profiles. Target is in parentheses if there are results with multiple targets
from the same source.

- [Sennheiser HD 600](./oratory1990/over-ear/Sennheiser%20HD%20600) by oratory1990
- [Sennheiser HD 600](./crinacle/GRAS%2043AG-7%20over-ear/Sennheiser%20HD%20600) by crinacle on GRAS 43AG-7
- [1MORE Aero (ANC Off)](./HypetheSonics/GRAS%20RA0045%20in-ear/1MORE%20Aero%20(ANC%20Off)) by HypetheSonics on GRAS RA0045
- [Sennheiser HD 600 (2020, worn earpads)](./crinacle/GRAS%2043AG-7%20over-ear/Sennheiser%20HD%20600%20(2020,%20worn%20earpads)) by crinacle on GRAS 43AG-7
- [Daiso $2 earphones](./crinacle/711%20in-ear/Daiso%20$2%20earphones) by crinacle on 711
- [1MORE Quad Driver](./HypetheSonics/Bruel%20&%20Kjaer%205128%20in-ear/1MORE%20Quad%20Driver) by HypetheSonics on Bruel & Kjaer 5128
- [Broken](no-dot-slash) by nobody
- [Escape](./../etc) by nobody
- [Escape too](./a/../../b) by nobody
- [No source](./a/b/c) by
- [Query](./a/b/c?x=1) by someone
- [Fragment](./a/b/c#top) by someone
- [Space](./a/b/c d) by someone
- [Dots](./a/%2e%2e/%2e%2e/x) by someone
- [](./a/b/c) by someone
* [Wrong bullet](./a/b/c) by someone
";

    fn entries() -> Vec<AutoEqEntry> {
        parse_index(INDEX)
    }

    fn entry(name: &str, source: &str, path: &str) -> AutoEqEntry {
        AutoEqEntry {
            name: name.to_owned(),
            source: source.to_owned(),
            rig: None,
            path: path.to_owned(),
        }
    }

    #[test]
    fn index_lines_read_names_sources_and_rigs() {
        let list = entries();
        assert_eq!(list.len(), 6, "{list:#?}");
        assert_eq!(list[0].name, "Sennheiser HD 600");
        assert_eq!(list[0].source, "oratory1990");
        assert_eq!(list[0].rig, None);
        assert_eq!(list[0].path, "oratory1990/over-ear/Sennheiser%20HD%20600");
        assert_eq!(list[1].rig.as_deref(), Some("GRAS 43AG-7"));
        assert_eq!(list[1].by_line(), "crinacle on GRAS 43AG-7");
        assert_eq!(list[0].by_line(), "oratory1990");
        // Parentheses in the name and in the folder.
        assert_eq!(list[2].name, "1MORE Aero (ANC Off)");
        assert_eq!(
            list[2].path,
            "HypetheSonics/GRAS%20RA0045%20in-ear/1MORE%20Aero%20(ANC%20Off)"
        );
        assert_eq!(list[3].name, "Sennheiser HD 600 (2020, worn earpads)");
        assert_eq!(list[4].name, "Daiso $2 earphones");
        assert_eq!(list[5].rig.as_deref(), Some("Bruel & Kjaer 5128"));
    }

    #[test]
    fn junk_lines_and_paths_that_leave_the_results_are_skipped() {
        let names: Vec<String> = entries().into_iter().map(|e| e.name).collect();
        for skipped in [
            "Broken",
            "Escape",
            "Escape too",
            "No source",
            "Query",
            "Fragment",
            "Space",
            "Dots",
            "Wrong bullet",
        ] {
            assert!(!names.iter().any(|n| n == skipped), "{skipped} was read");
        }
        assert!(parse_index("").is_empty());
        assert!(parse_index("# Index\n\nnothing here\n").is_empty());
    }

    #[test]
    fn a_profile_url_repeats_the_folder_name() {
        let list = entries();
        assert_eq!(
            list[0].profile_url().as_deref(),
            Some(
                "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/results/oratory1990/over-ear/Sennheiser%20HD%20600/Sennheiser%20HD%20600%20ParametricEQ.txt"
            )
        );
        assert_eq!(
            list[2].profile_url().as_deref(),
            Some(
                "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/results/HypetheSonics/GRAS%20RA0045%20in-ear/1MORE%20Aero%20(ANC%20Off)/1MORE%20Aero%20(ANC%20Off)%20ParametricEQ.txt"
            )
        );
        assert_eq!(
            index_url(),
            "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/results/INDEX.md"
        );
        // A hand-built entry that would leave the host gets no URL.
        assert_eq!(entry("X", "y", "a/b?c").profile_url(), None);
        assert_eq!(entry("X", "y", "a/b#c").profile_url(), None);
        assert_eq!(entry("X", "y", "a/%2E%2E/%2e%2e/x").profile_url(), None);
    }

    #[test]
    fn requests_go_only_to_the_results_host() {
        assert!(on_results_host(&index_url()));
        for url in [
            "http://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/results/INDEX.md",
            "https://raw.githubusercontent.com.evil.net/jaakkopasanen/AutoEq/master/results/x",
            "https://raw.githubusercontent.com/someone/else/INDEX.md",
            "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/results/INDEX.md?x",
            "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master/results/%2e%2e/x",
            "not a url",
        ] {
            assert!(!on_results_host(url), "{url}");
        }
    }

    #[test]
    fn cache_names_are_flat_safe_and_distinct() {
        let list = entries();
        assert_eq!(
            list[0].cache_name().as_deref(),
            Some("oratory1990%2Fover-ear%2FSennheiser%2520HD%2520600.txt")
        );
        assert!(list.iter().all(|entry| entry.cache_name().is_some()));
        // `%2F` in a path and a real `/` never share a file.
        assert_ne!(
            entry("X", "y", "a%2Fb").cache_name(),
            entry("X", "y", "a/b").cache_name()
        );
        // Anything outside the allow-list, or too long, is not cached.
        for hostile in ["a/b:c", "a/C%3A\\x", "a/b c", "a/\u{e9}", "a/b\0c"] {
            assert_eq!(entry("X", "y", hostile).cache_name(), None, "{hostile:?}");
        }
        let long = format!("a/{}", "b".repeat(MAX_CACHE_NAME_BYTES));
        assert_eq!(entry("X", "y", &long).cache_name(), None);
        let fits = format!("a/{}", "b".repeat(MAX_CACHE_NAME_BYTES - 10));
        assert!(entry("X", "y", &fits).cache_name().is_some());
    }

    #[test]
    fn search_ranks_an_exact_name_over_a_prefix_over_a_substring() {
        let list = vec![
            entry("Sennheiser HD 600 (2020)", "crinacle", "a/b/1"),
            entry("Sennheiser HD 600", "oratory1990", "a/b/2"),
            entry("AKG K371", "oratory1990", "a/b/3"),
            entry("Sennheiser HD 600", "crinacle", "a/b/4"),
            entry("Beyerdynamic DT 770 (for HD 600 fans)", "x", "a/b/5"),
        ];
        let found = search(&list, "SENNHEISER hd 600", 10);
        // Exact (both sources, in the list's order), prefix, then substring.
        assert_eq!(found.shown, [1, 3, 0]);
        assert_eq!(found.matched, 3);

        // Every term must occur, in any order, in the name or the source;
        // names that only contain them sort by name.
        assert_eq!(search(&list, "600 hd", 10).shown, [4, 1, 3, 0]);
        assert_eq!(search(&list, "akg oratory", 10).shown, [2]);
        assert_eq!(search(&list, "crinacle 2020", 10).shown, [0]);
        assert!(search(&list, "hd 650", 10).shown.is_empty());

        // Prefixes sort by name among themselves.
        let found = search(&list, "  sennheiser  ", 10);
        assert_eq!(found.shown, [1, 3, 0]);

        // At most the limit, with the full count.
        let capped = search(&list, "hd", 2);
        assert_eq!(capped.shown.len(), 2);
        assert_eq!(capped.matched, 4);

        // An empty query matches everything, by name.
        let all = search(&list, "", 10);
        assert_eq!(all.matched, 5);
        assert_eq!(all.shown, [2, 4, 1, 3, 0]);
    }

    #[test]
    fn a_list_is_fresh_for_seven_days() {
        let now = SystemTime::now();
        let day = Duration::from_secs(24 * 60 * 60);
        assert!(index_is_fresh(now, now));
        assert!(index_is_fresh(now - day * 6, now));
        assert!(!index_is_fresh(now - day * 7, now));
        assert!(!index_is_fresh(now - day * 8, now));
        assert!(!index_is_fresh(now + day, now), "a time ahead of the clock");
    }

    /// A fetch that counts its calls and answers `answer`.
    fn counted(
        calls: &AtomicUsize,
        answer: Result<String>,
    ) -> impl FnOnce() -> std::future::Ready<Result<String>> + '_ {
        move || {
            calls.fetch_add(1, Ordering::Relaxed);
            std::future::ready(answer)
        }
    }

    fn age(path: &Path, by: Duration) {
        let file = std::fs::File::options()
            .write(true)
            .open(path)
            .expect("the cached file");
        file.set_modified(SystemTime::now() - by)
            .expect("set the time");
    }

    #[tokio::test]
    async fn the_list_is_cached_reused_while_fresh_and_refreshed_after_a_week() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let cache = AutoEqCache::at(dir.path().join("autoeq"));
        let calls = AtomicUsize::new(0);

        // Nothing cached: downloaded, then kept.
        let list = load_index(
            Some(&cache),
            false,
            SystemTime::now(),
            counted(&calls, Ok(INDEX.to_owned())),
        )
        .await
        .expect("the list");
        assert_eq!(list.len(), 6);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        let index = dir.path().join("autoeq").join(CACHE_INDEX_FILE);
        assert!(index.exists());

        // Fresh: no request.
        let list = load_index(
            Some(&cache),
            false,
            SystemTime::now(),
            counted(&calls, Err(anyhow!("offline"))),
        )
        .await
        .expect("the cached list");
        assert_eq!(list.len(), 6);
        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "a fresh list asks nothing"
        );

        // A week old: asked again; the failure still gets the old copy.
        age(&index, INDEX_MAX_AGE + Duration::from_secs(60));
        let list = load_index(
            Some(&cache),
            false,
            SystemTime::now(),
            counted(&calls, Err(anyhow!("offline"))),
        )
        .await
        .expect("the stale list");
        assert_eq!(list.len(), 6);
        assert_eq!(calls.load(Ordering::Relaxed), 2);

        // A week old and online: the new list replaces it, fresh again.
        let one = "- [AKG K371](./oratory1990/over-ear/AKG%20K371) by oratory1990\n";
        let list = load_index(
            Some(&cache),
            false,
            SystemTime::now(),
            counted(&calls, Ok(one.to_owned())),
        )
        .await
        .expect("the new list");
        assert_eq!(list.len(), 1);
        let cached = cache.read_index(SystemTime::now()).expect("kept");
        assert!(cached.fresh);
        assert_eq!(cached.text, one);
        // No temp file is left behind.
        let leftovers = std::fs::read_dir(dir.path().join("autoeq"))
            .expect("the cache dir")
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "tmp"))
            .count();
        assert_eq!(leftovers, 0);
    }

    #[tokio::test]
    async fn a_forced_refresh_asks_and_its_failure_is_the_error() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let cache = AutoEqCache::at(dir.path());
        cache.write_index(INDEX).expect("seeded");
        let calls = AtomicUsize::new(0);
        let failed = load_index(
            Some(&cache),
            true,
            SystemTime::now(),
            counted(&calls, Err(anyhow!("offline"))),
        )
        .await;
        assert!(failed.is_err());
        assert_eq!(calls.load(Ordering::Relaxed), 1, "fresh, but asked anyway");
    }

    #[tokio::test]
    async fn a_download_with_no_entries_is_an_error_and_never_cached() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let cache = AutoEqCache::at(dir.path());
        let calls = AtomicUsize::new(0);
        let error_page = "<html>rate limited</html>".to_owned();
        let failed = load_index(
            Some(&cache),
            false,
            SystemTime::now(),
            counted(&calls, Ok(error_page)),
        )
        .await
        .expect_err("refused");
        assert!(failed.to_string().contains("no headphones"), "{failed}");
        assert!(cache.read_index(SystemTime::now()).is_none());
        // Without a cache at all the download still works.
        let list = load_index(
            None,
            false,
            SystemTime::now(),
            counted(&calls, Ok(INDEX.to_owned())),
        )
        .await
        .expect("the list");
        assert_eq!(list.len(), 6);
    }

    const PROFILE: &str = "Preamp: -6.2 dB\n\
                           Filter 1: ON LSC Fc 105 Hz Gain 6.5 dB Q 0.70\n\
                           Filter 2: ON PK Fc 220 Hz Gain -3.1 dB Q 0.85\n";

    #[tokio::test]
    async fn a_profile_is_downloaded_once_and_then_read_from_the_cache() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let cache = AutoEqCache::at(dir.path());
        let hd600 = entries().remove(0);
        let calls = AtomicUsize::new(0);

        let bands = load_profile(Some(&cache), &hd600, counted(&calls, Ok(PROFILE.into())))
            .await
            .expect("bands");
        assert_eq!(bands.len(), 2);
        assert_eq!(cache.read_profile(&hd600).as_deref(), Some(PROFILE));

        let again = load_profile(
            Some(&cache),
            &hd600,
            counted(&calls, Err(anyhow!("offline"))),
        )
        .await
        .expect("from the cache");
        assert_eq!(again, bands);
        assert_eq!(calls.load(Ordering::Relaxed), 1);

        // A cached file that no longer reads is downloaded again.
        std::fs::write(
            dir.path()
                .join(CACHE_PROFILES_DIR)
                .join(hd600.cache_name().expect("a name")),
            "garbage",
        )
        .expect("spoiled");
        let fixed = load_profile(Some(&cache), &hd600, counted(&calls, Ok(PROFILE.into())))
            .await
            .expect("downloaded again");
        assert_eq!(fixed, bands);
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn a_profile_that_does_not_parse_is_an_error_and_not_cached() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let cache = AutoEqCache::at(dir.path());
        let hd600 = entries().remove(0);
        let calls = AtomicUsize::new(0);
        let bad = "Filter 1: ON BP Fc 1000 Hz Q 1\n".to_owned();
        let why = load_profile(Some(&cache), &hd600, counted(&calls, Ok(bad)))
            .await
            .expect_err("refused");
        assert!(
            why.to_string().contains("unsupported filter type BP"),
            "{why}"
        );
        assert_eq!(cache.read_profile(&hd600), None);

        // An entry with no safe name still loads; it just isn't kept.
        let odd = entry("Odd", "y", "a/b:c");
        let bands = load_profile(Some(&cache), &odd, counted(&calls, Ok(PROFILE.into())))
            .await
            .expect("bands");
        assert_eq!(bands.len(), 2);
        assert_eq!(cache.read_profile(&odd), None);
        assert!(!cache.write_profile(&odd, PROFILE).expect("no error"));
    }

    #[test]
    fn an_answer_is_cut_off_at_its_cap_while_it_streams() {
        let mut body = CappedBody::new(10);
        body.push(b"12345").expect("under the cap");
        body.push(b"67890").expect("at the cap");
        let over = body.push(b"1").expect_err("past the cap");
        assert!(over.to_string().contains("over"), "{over}");
        assert_eq!(body.into_text().expect("text"), "1234567890");

        let mut binary = CappedBody::new(10);
        binary.push(&[0xff, 0xfe]).expect("under the cap");
        assert!(binary.into_text().is_err());

        assert_eq!(size_label(MAX_INDEX_BYTES), "16 MiB");
        assert_eq!(size_label(MAX_PROFILE_BYTES), "64 KiB");
    }

    #[test]
    fn a_cached_file_past_its_cap_is_not_read() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let path = dir.path().join("big.txt");
        std::fs::write(&path, "x".repeat(11)).expect("written");
        assert_eq!(read_capped_file(&path, 10), None);
        assert_eq!(read_capped_file(&path, 11).map(|t| t.len()), Some(11));
        assert_eq!(read_capped_file(&dir.path().join("missing"), 10), None);
    }

    #[tokio::test]
    async fn the_session_keeps_the_list_until_a_refresh() {
        let service = AutoEqService {
            cache: None,
            session: parking_lot::Mutex::new(None),
        };
        assert!(service.session_index().is_none(), "nothing read at start");
        let calls = AtomicUsize::new(0);
        let first = service
            .index_with(false, counted(&calls, Ok(INDEX.to_owned())))
            .await
            .expect("the list");
        assert_eq!(calls.load(Ordering::Relaxed), 1);

        // The next open: the session's copy, nothing asked.
        let again = service
            .index_with(false, counted(&calls, Err(anyhow!("offline"))))
            .await
            .expect("the session's list");
        assert!(Arc::ptr_eq(&first, &again));
        assert_eq!(calls.load(Ordering::Relaxed), 1);

        // Refresh list asks; a failed refresh keeps the session's copy.
        assert!(
            service
                .index_with(true, counted(&calls, Err(anyhow!("offline"))))
                .await
                .is_err()
        );
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        assert!(
            service
                .session_index()
                .is_some_and(|list| Arc::ptr_eq(&list, &first))
        );
        let one = "- [AKG K371](./oratory1990/over-ear/AKG%20K371) by oratory1990\n";
        let refreshed = service
            .index_with(true, counted(&calls, Ok(one.to_owned())))
            .await
            .expect("the new list");
        assert_eq!(refreshed.len(), 1);
        assert!(service.session_index().is_some_and(|list| list.len() == 1));
    }

    #[test]
    fn the_user_agent_names_nokkvi_and_its_version_only() {
        assert_eq!(user_agent("0.32.1"), "nokkvi/0.32.1");
    }
}
