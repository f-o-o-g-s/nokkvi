//! HTTP reader with Range request support for random access.
//!
//! Fetches audio data using HTTP Range requests, allowing Symphonia to read
//! any part of a file without downloading everything. Uses a sparse cache with
//! LRU eviction to bound memory usage.
//!
//! # Read-ahead (issue #9 "Stuttering audio")
//!
//! The synchronous `Read::read` path is what Symphonia drives, on the decode
//! loop under `block_in_place`. On a remote/high-latency link a cache miss used
//! to block that read on a full HTTP round-trip while the small decoded ring
//! buffer drained to silence — the reporter's short hiccups. To fix it, a
//! background async prefetch task keeps a sliding window of `PREFETCH_WINDOW_CHUNKS`
//! chunks resident *ahead* of the read cursor, fetching concurrently with
//! playback so steady-state reads hit the cache (memcpy only). A cache miss
//! still falls back to the original synchronous fetch, so worst-case behavior
//! equals the pre-fix path — the change is strictly additive.
//!
//! The prefetch task mirrors `AsyncNetworkBuffer` (the radio reader): a
//! `tokio::spawn`'d async task using an async `reqwest::Client` (so the GET is a
//! real `.await` point and `abort()` cancels it promptly), and `Drop` aborts +
//! detaches the task — never joins — because the reader's `Drop` fires on a
//! tokio worker during the gapless decoder swap, where a blocking join would
//! park the worker.

use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Seek, SeekFrom},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use parking_lot::Mutex;
use symphonia::core::io::MediaSource;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// The reader's chunk: 256 KB, fetched with one Range request each.
pub(super) const CHUNK_SIZE: u64 = 256 * 1024;
/// Maximum chunks to keep in cache (~4MB total).
/// Larger cache reduces re-fetches during sequential playback and decoder backtracks.
const MAX_CACHED_CHUNKS: usize = 16;
/// Chunks the background task keeps resident ahead of the read cursor.
const PREFETCH_WINDOW_CHUNKS: usize = 5;
/// Low watermark for batch-refilling the window. The task lets the resident-ahead
/// window drain to this before refetching up to the full window in a burst
/// (hysteresis), so refills batch instead of one GET per chunk the cursor passes
/// — matching queue2's low/high, MPD's resume-384KB/pause-512KB, and mpv's
/// curl-ring 50%/100%.
const PREFETCH_LOW_WATERMARK_CHUNKS: usize = PREFETCH_WINDOW_CHUNKS / 2; // = 2
/// Headroom reserved for chunks the demuxer reads *behind* the cursor (FLAC
/// resync / backtrack), so a forward prefetch can never evict a not-yet-read
/// window chunk. `1` is the current chunk.
const EXPECTED_BEHIND_CHUNKS: usize = 4;
// The live working set (current + forward window + backtrack headroom) must fit
// the LRU, or the prefetcher could evict a chunk it just fetched ahead.
const _: () = assert!(1 + PREFETCH_WINDOW_CHUNKS + EXPECTED_BEHIND_CHUNKS <= MAX_CACHED_CHUNKS);
/// Per-request timeout for the background prefetch client. Deliberately shorter
/// than the read-path's 10s so a detached/cancelled task self-terminates fast.
const PREFETCH_REQUEST_TIMEOUT_SECS: u64 = 4;
/// How long the prefetch task sleeps when its window is already full (the
/// gapless-prep decoder sits idle with a static cursor — without this it would
/// busy-spin a core).
const PREFETCH_IDLE_POLL_MS: u64 = 100;
/// Retries for a transient read-path chunk fetch (unchanged from the original).
const MAX_RETRIES: u32 = 5;

/// Sparse chunk cache with LRU eviction, shared by the synchronous read path
/// and the background prefetch task (serialized by the enclosing `Mutex`).
///
/// Chunk bytes are `Arc<Vec<u8>>` so a reader can clone the handle under the
/// lock and copy the bytes *without* holding it — a concurrent insert/evict can
/// never free bytes mid-copy. Note: NOT single-writer — both the read-path
/// miss-fallback and the prefetch task insert here; the `Mutex` serializes them.
struct ChunkStore {
    chunks: HashMap<u64, Arc<Vec<u8>>>,
    /// LRU order: oldest at front, newest at back.
    chunk_order: VecDeque<u64>,
}

impl ChunkStore {
    fn new() -> Self {
        Self {
            chunks: HashMap::new(),
            chunk_order: VecDeque::new(),
        }
    }

    fn contains(&self, idx: u64) -> bool {
        self.chunks.contains_key(&idx)
    }

    /// Fetch a chunk handle, bumping it to most-recently-used.
    fn get(&mut self, idx: u64) -> Option<Arc<Vec<u8>>> {
        let bytes = self.chunks.get(&idx)?.clone();
        self.chunk_order.retain(|&i| i != idx);
        self.chunk_order.push_back(idx);
        Some(bytes)
    }

    /// Insert a chunk with LRU eviction. `chunk_order` is deduped first so a
    /// re-insert of an existing index (read-path + prefetch racing on the same
    /// chunk) can never push a duplicate and desync the eviction count.
    /// Callers re-check `contains` after re-acquiring the lock, so an insert of
    /// an already-present index is not expected in practice.
    fn insert(&mut self, idx: u64, bytes: Arc<Vec<u8>>) {
        self.chunk_order.retain(|&i| i != idx);
        while self.chunks.len() >= MAX_CACHED_CHUNKS {
            match self.chunk_order.pop_front() {
                Some(oldest) => {
                    self.chunks.remove(&oldest);
                }
                None => break,
            }
        }
        self.chunks.insert(idx, bytes);
        self.chunk_order.push_back(idx);
    }
}

/// Compute which chunks in the forward window `[cursor_chunk+1 ..= +WINDOW]` are
/// absent from the store (and within the file). Returns empty when the window is
/// fully resident — the prefetch task uses that to go idle instead of spinning.
fn chunks_to_prefetch(cursor: u64, content_length: u64, store: &ChunkStore) -> Vec<u64> {
    let current = cursor / CHUNK_SIZE;
    let mut missing = Vec::new();
    for idx in (current + 1)..=(current + PREFETCH_WINDOW_CHUNKS as u64) {
        if idx * CHUNK_SIZE >= content_length {
            break; // past EOF — nothing more to fetch
        }
        if !store.contains(idx) {
            missing.push(idx);
        }
    }
    missing
}

/// Count the forward-window chunks `[cursor_chunk+1 ..= +WINDOW]` currently
/// resident in the store (within the file). `resident + chunks_to_prefetch().len()`
/// equals `window_capacity`.
fn resident_ahead_count(cursor: u64, content_length: u64, store: &ChunkStore) -> usize {
    let current = cursor / CHUNK_SIZE;
    let mut count = 0;
    for idx in (current + 1)..=(current + PREFETCH_WINDOW_CHUNKS as u64) {
        if idx * CHUNK_SIZE >= content_length {
            break;
        }
        if store.contains(idx) {
            count += 1;
        }
    }
    count
}

/// Number of forward-window slots that actually exist within the file (the
/// window shrinks near EOF).
fn window_capacity(cursor: u64, content_length: u64) -> usize {
    let current = cursor / CHUNK_SIZE;
    let mut capacity = 0;
    for idx in (current + 1)..=(current + PREFETCH_WINDOW_CHUNKS as u64) {
        if idx * CHUNK_SIZE >= content_length {
            break;
        }
        capacity += 1;
    }
    capacity
}

/// Hysteresis latch for batch-refilling. Begin refilling once the resident window
/// drops below the low watermark; keep refilling until the window is full again;
/// hold the prior state in between. Stops the producer from issuing one fetch per
/// chunk the cursor advances past.
fn update_refilling(resident: usize, capacity: usize, refilling: bool) -> bool {
    let low = PREFETCH_LOW_WATERMARK_CHUNKS.min(capacity);
    if resident < low {
        true
    } else if resident >= capacity {
        false // window full (or at EOF)
    } else {
        refilling // within the hysteresis band — hold
    }
}

/// A `Content-Range` header's parts (`bytes 0-262143/3216574`,
/// `bytes */0`, `bytes 0-9/*`): the first and last byte sent, and the whole
/// length, each when the header names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ContentRange {
    pub(super) start: Option<u64>,
    pub(super) end: Option<u64>,
    pub(super) total: Option<u64>,
}

impl ContentRange {
    pub(super) fn of(headers: &reqwest::header::HeaderMap) -> Option<Self> {
        let value = headers.get(reqwest::header::CONTENT_RANGE)?.to_str().ok()?;
        let (range, total) = value.trim().strip_prefix("bytes")?.trim().split_once('/')?;
        let (start, end) = range.split_once('-').map_or((None, None), |(start, end)| {
            (start.trim().parse().ok(), end.trim().parse().ok())
        });
        Some(Self {
            start,
            end,
            total: total.trim().parse().ok(),
        })
    }
}

/// Whether an answer to the Range request for bytes `fetch_start..fetch_end`
/// of a `content_length`-byte file is that chunk: a 206 whose
/// `Content-Range` is exactly the range asked for, or, for the first chunk
/// of a file that fits in one chunk, a 200 (the whole file, from a server or
/// proxy that ignores Range). Anything else (a 200 for a later chunk, which
/// would store the file's start in the middle; a 206 that starts elsewhere
/// or was capped short, which would end the read early) is not.
fn answer_is_chunk(
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
    fetch_start: u64,
    fetch_end: u64,
    content_length: u64,
) -> bool {
    if status == reqwest::StatusCode::PARTIAL_CONTENT {
        ContentRange::of(headers).is_some_and(|range| {
            range.start == Some(fetch_start) && range.end == Some(fetch_end - 1)
        })
    } else {
        status == reqwest::StatusCode::OK && fetch_start == 0 && fetch_end == content_length
    }
}

/// The URL a [`RangeHttpReader`] fetches its ranges from, shared with its
/// read-ahead task and with the decoder that built it, which can point it
/// elsewhere ([`Self::set`]): a request at an offset answered with the song's
/// own file is read on from the file's own URL, without the offset (the
/// server answers the offset URL by its own rules, a cache entry or a live
/// transcode, the next time it is asked).
#[derive(Clone)]
pub(super) struct RangeUrl(Arc<parking_lot::RwLock<String>>);

impl RangeUrl {
    fn new(url: String) -> Self {
        Self(Arc::new(parking_lot::RwLock::new(url)))
    }

    /// The URL the next range is fetched from.
    fn get(&self) -> String {
        self.0.read().clone()
    }

    /// Fetch every later range from `url`.
    pub(super) fn set(&self, url: String) {
        *self.0.write() = url;
    }
}

/// HTTP reader with Range request support for random access.
pub(super) struct RangeHttpReader {
    /// Blocking client for the synchronous read-path miss-fallback.
    client: reqwest::blocking::Client,
    /// URL to fetch from.
    url: RangeUrl,
    /// Total content length.
    content_length: u64,
    /// Current read position.
    position: u64,
    /// Shared chunk cache (read path + prefetch task).
    store: Arc<Mutex<ChunkStore>>,
    /// Read position published for the prefetch task to track (bytes).
    read_cursor: Arc<AtomicU64>,
    /// Cancels the prefetch task on `Drop`.
    cancel: CancellationToken,
    /// Background prefetch task, spawned lazily on the first forward-sequential read.
    prefetch: Option<JoinHandle<()>>,
    /// Chunk index of the previous read, to detect forward-sequential progress
    /// (so the prefetch task isn't spawned off a bouncing init-probe cursor).
    last_chunk_index: Option<u64>,
}

impl RangeHttpReader {
    /// Create a new Range-based HTTP reader. The prefetch task is NOT spawned
    /// here (no guaranteed runtime context, and the init-probe seeks bounce the
    /// cursor) — it spawns lazily from the first forward-sequential `read`.
    pub(super) fn new(url: String, content_length: u64) -> Self {
        Self {
            client: Self::create_client(),
            url: RangeUrl::new(url),
            content_length,
            position: 0,
            store: Arc::new(Mutex::new(ChunkStore::new())),
            read_cursor: Arc::new(AtomicU64::new(0)),
            cancel: CancellationToken::new(),
            prefetch: None,
            last_chunk_index: None,
        }
    }

    /// A reader whose first chunk (bytes `0..min(CHUNK_SIZE, content_length)`)
    /// is already in hand, so the first read needs no request: the decoder's
    /// one GET for a requested transcode asks for exactly that range.
    /// `first_chunk` is used only when it is that whole chunk; anything else
    /// is dropped and the chunk fetched on demand as usual.
    pub(super) fn with_first_chunk(url: String, content_length: u64, first_chunk: Vec<u8>) -> Self {
        let reader = Self::new(url, content_length);
        if first_chunk.len() as u64 == Self::first_chunk_len(content_length)
            && !first_chunk.is_empty()
        {
            reader.store.lock().insert(0, Arc::new(first_chunk));
        }
        reader
    }

    /// The handle to the URL this reader fetches from ([`RangeUrl`]).
    pub(super) fn url_handle(&self) -> RangeUrl {
        self.url.clone()
    }

    /// Bytes `0..first_chunk_len(content_length)` make up the first chunk.
    pub(super) fn first_chunk_len(content_length: u64) -> u64 {
        CHUNK_SIZE.min(content_length)
    }

    /// Create a new blocking HTTP client with optimal settings for streaming.
    ///
    /// Built on a thread of its own: reqwest's blocking client waits for its
    /// own runtime thread to come up inside a throwaway tokio runtime, which
    /// a debug build refuses on a runtime thread ("Cannot drop a runtime in a
    /// context where blocking is not allowed"), and the first build runs on
    /// one (`AudioDecoder::open_input` is async; the chunk reads that follow
    /// are under `block_in_place`, which tokio allows). A release build has
    /// no such check, so this is what lets `cargo run` play a sized track.
    fn create_client() -> reqwest::blocking::Client {
        std::thread::scope(|scope| {
            scope
                .spawn(Self::build_client)
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
        })
    }

    fn build_client() -> reqwest::blocking::Client {
        reqwest::blocking::Client::builder()
            // Identify as nokkvi (shared crate UA) so the server sees one player.
            .user_agent(crate::USER_AGENT)
            // A queue song's URL carries the /rest credential.
            .redirect(crate::utils::redirects::guard_credential_urls())
            // Fast fail on connect - 5s is plenty for local server
            .connect_timeout(std::time::Duration::from_secs(5))
            // Reduced from 30s - 10s is enough for chunk reads, prevents long stalls
            .timeout(std::time::Duration::from_secs(10))
            // More aggressive keepalive to detect dead connections faster
            .tcp_keepalive(std::time::Duration::from_secs(10))
            // Connection pooling enabled (default) — reuses TCP connections between
            // chunk fetches, avoiding repeated TLS handshake and TCP slow start.
            // Retry logic already recreates the client on failures.
            .build()
            .expect("Failed to create HTTP client")
    }

    /// Get the chunk index for a byte position.
    fn chunk_index(pos: u64) -> u64 {
        pos / CHUNK_SIZE
    }

    /// Read-path synchronous fetch fallback: ensure `chunk_idx` is cached,
    /// fetching it on this thread if absent. Used only on a cache MISS (the
    /// prefetch task didn't get there first); worst-case behavior matches the
    /// pre-read-ahead path. Retries transient failures, recreating the client to
    /// force a fresh connection. The network fetch happens with the store lock
    /// released.
    fn ensure_chunk(&mut self, chunk_idx: u64) -> std::io::Result<()> {
        if self.store.lock().get(chunk_idx).is_some() {
            return Ok(()); // cache hit (LRU bumped)
        }

        let fetch_start = chunk_idx * CHUNK_SIZE;
        let fetch_end = ((chunk_idx + 1) * CHUNK_SIZE).min(self.content_length);

        if fetch_start >= self.content_length {
            return Ok(()); // Beyond EOF
        }

        let range_header = format!("bytes={}-{}", fetch_start, fetch_end - 1);
        let mut last_error = None;

        for attempt in 0..MAX_RETRIES {
            tracing::trace!(
                "📥 [HTTP] Fetching chunk {} (bytes {}-{}), attempt {}/{} (read-path miss)",
                chunk_idx,
                fetch_start,
                fetch_end - 1,
                attempt + 1,
                MAX_RETRIES
            );

            let url = self.url.get();
            let result = self.client.get(&url).header("Range", &range_header).send();

            match result {
                Ok(response) => {
                    let status = response.status();
                    if !status.is_success() {
                        return Err(std::io::Error::other(format!(
                            "HTTP Range request failed with status: {status}"
                        )));
                    }
                    // Not a retry: a server that answers another part once
                    // answers it every time.
                    if !answer_is_chunk(
                        status,
                        response.headers(),
                        fetch_start,
                        fetch_end,
                        self.content_length,
                    ) {
                        return Err(std::io::Error::other(format!(
                            "HTTP Range request for chunk {chunk_idx} was answered with \
                             another part of the stream ({status})"
                        )));
                    }

                    let read_start = std::time::Instant::now();
                    let bytes = response.bytes().map_err(|e| {
                        // Body-read (`Decode`-kind) errors carry no URL in reqwest, so
                        // `without_url()` is a no-op here — kept as defense-in-depth.
                        // The credentialed stream URL on this path is stripped where it
                        // actually appears: the `.send()` error captured below (which
                        // also feeds the io::Error that resurfaces in the decoder logs).
                        std::io::Error::other(format!(
                            "Failed to read response: {}",
                            e.without_url()
                        ))
                    })?;
                    let read_elapsed = read_start.elapsed();

                    // Shorter than its range (empty, or the transfer cut off):
                    // stored, it would end the read early.
                    if bytes.len() as u64 != fetch_end - fetch_start {
                        tracing::warn!(
                            "⚠️ [HTTP] Chunk {} returned {} bytes (expected {}), treating as error",
                            chunk_idx,
                            bytes.len(),
                            fetch_end - fetch_start
                        );
                        if attempt < MAX_RETRIES - 1 {
                            self.client = Self::create_client();
                            let backoff_ms = 500 * (attempt as u64 + 1);
                            std::thread::sleep(std::time::Duration::from_millis(backoff_ms));
                        }
                        continue; // Retry
                    }

                    // A read-path fetch means the prefetcher fell behind: log it
                    // (this is the residual stutter path). Slow / post-retry are
                    // milestones per the severity contract.
                    if attempt > 0 || read_elapsed.as_millis() > 1000 {
                        tracing::info!(
                            "📥 [HTTP] Chunk {} read-path fetch completed (attempt {}/{}, read took {:?}, {} bytes)",
                            chunk_idx,
                            attempt + 1,
                            MAX_RETRIES,
                            read_elapsed,
                            bytes.len()
                        );
                    }

                    let mut store = self.store.lock();
                    if !store.contains(chunk_idx) {
                        store.insert(chunk_idx, Arc::new(bytes.to_vec()));
                    }
                    return Ok(());
                }
                Err(e) => {
                    // Strip the credentialed stream URL on capture: this one error
                    // value feeds the retry debug!, the final error!, and the
                    // io::Error message below (which resurfaces in the decoder logs).
                    last_error = Some(e.without_url());
                    if attempt < MAX_RETRIES - 1 {
                        let backoff_ms = 500 * (attempt as u64 + 1);
                        tracing::debug!(
                            "📥 [HTTP] Chunk {} fetch failed (attempt {}/{}), retrying in {}ms: {:?}",
                            chunk_idx,
                            attempt + 1,
                            MAX_RETRIES,
                            backoff_ms,
                            last_error
                        );
                        self.client = Self::create_client();
                        std::thread::sleep(std::time::Duration::from_millis(backoff_ms));
                    }
                }
            }
        }

        tracing::error!(
            "❌ [HTTP] Chunk {} fetch FAILED after {} retries: {:?}",
            chunk_idx,
            MAX_RETRIES,
            last_error
        );
        Err(std::io::Error::other(format!(
            "HTTP request failed after {MAX_RETRIES} retries: {last_error:?}"
        )))
    }

    /// Spawn the background prefetch task the first time we see forward-sequential
    /// reads (current chunk == previous + 1). Spawning lazily from `read` (which
    /// runs under `block_in_place` on the decode loop) guarantees a tokio runtime
    /// context, and waiting for sequential progress avoids chasing the bouncing
    /// cursor of the FLAC init-probe's binary-search seeks.
    fn maybe_spawn_prefetch(&mut self) {
        if self.prefetch.is_some() {
            return;
        }
        // Spawn only once forward-sequential progress is observed (current chunk
        // == previous + 1), so the FLAC init-probe's binary-search seeks don't
        // spawn a task that chases a bouncing cursor.
        let current = Self::chunk_index(self.position);
        let advance = self
            .last_chunk_index
            .is_some_and(|last| current == last + 1);
        if !advance {
            return;
        }

        let url = self.url.clone();
        let content_length = self.content_length;
        let store = Arc::clone(&self.store);
        let read_cursor = Arc::clone(&self.read_cursor);
        let cancel = self.cancel.clone();

        tracing::debug!("📥 [HTTP] Spawning background read-ahead prefetch task");
        self.prefetch = Some(tokio::spawn(prefetch_loop(
            url,
            content_length,
            store,
            read_cursor,
            cancel,
        )));
    }

    /// Read bytes from cache, fetching chunks synchronously only on a miss.
    fn read_from_cache(&mut self, offset: u64, buf: &mut [u8]) -> std::io::Result<usize> {
        if offset >= self.content_length {
            return Ok(0); // EOF
        }

        let mut bytes_read = 0;
        let mut current_pos = offset;

        while bytes_read < buf.len() && current_pos < self.content_length {
            let chunk_idx = Self::chunk_index(current_pos);

            // Clone the Arc handle out of the lock, then copy without holding it.
            // Bind the lookup to a local first so the MutexGuard is dropped
            // before `ensure_chunk` takes `&mut self` on a miss.
            let cached = self.store.lock().get(chunk_idx);
            let chunk = match cached {
                Some(chunk) => chunk,
                None => {
                    // Miss: synchronous fallback (the prefetcher didn't get here).
                    self.ensure_chunk(chunk_idx)?;
                    let refetched = self.store.lock().get(chunk_idx);
                    match refetched {
                        Some(chunk) => chunk,
                        None => {
                            tracing::warn!(
                                " [HTTP] Chunk {} not in cache after ensure_chunk succeeded",
                                chunk_idx
                            );
                            break;
                        }
                    }
                }
            };

            let chunk_start = chunk_idx * CHUNK_SIZE;
            let offset_in_chunk = (current_pos - chunk_start) as usize;

            if offset_in_chunk >= chunk.len() {
                tracing::warn!(
                    " [HTTP] Chunk {} offset {} exceeds chunk len {}, current_pos={}",
                    chunk_idx,
                    offset_in_chunk,
                    chunk.len(),
                    current_pos
                );
                break;
            }

            let available_in_chunk = chunk.len() - offset_in_chunk;
            let remaining_to_read = buf.len() - bytes_read;
            let remaining_in_file = (self.content_length - current_pos) as usize;
            let to_read = remaining_to_read
                .min(available_in_chunk)
                .min(remaining_in_file);

            if to_read == 0 {
                break;
            }

            buf[bytes_read..bytes_read + to_read]
                .copy_from_slice(&chunk[offset_in_chunk..offset_in_chunk + to_read]);

            bytes_read += to_read;
            current_pos += to_read as u64;
        }

        Ok(bytes_read)
    }
}

/// Background read-ahead loop: keep the forward window resident ahead of the
/// read cursor. Mirrors `AsyncNetworkBuffer`'s producer — an async task whose
/// every network call is a real `.await` raced against the `CancellationToken`,
/// so `abort()` on `Drop` stops it promptly. Never holds the store `Mutex`
/// across an `.await`.
async fn prefetch_loop(
    url: RangeUrl,
    content_length: u64,
    store: Arc<Mutex<ChunkStore>>,
    read_cursor: Arc<AtomicU64>,
    cancel: CancellationToken,
) {
    let client = match reqwest::Client::builder()
        .user_agent(crate::USER_AGENT)
        .redirect(crate::utils::redirects::guard_credential_urls())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(PREFETCH_REQUEST_TIMEOUT_SECS))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("📥 [HTTP] Prefetch client build failed, read-ahead disabled: {e}");
            return;
        }
    };

    let mut refilling = false;
    loop {
        if cancel.is_cancelled() {
            return;
        }

        let cursor = read_cursor.load(Ordering::Relaxed);
        let (resident, capacity, missing) = {
            let store = store.lock();
            (
                resident_ahead_count(cursor, content_length, &store),
                window_capacity(cursor, content_length),
                chunks_to_prefetch(cursor, content_length, &store),
            )
        };

        // Hysteresis: only (re)fill once the resident window has drained below the
        // low watermark, then burst back up to the full window. Between low and
        // full we idle, so refills batch instead of one GET per chunk passed.
        refilling = update_refilling(resident, capacity, refilling);

        if !refilling || missing.is_empty() {
            // Not below the low mark yet (or already full / at EOF): idle until
            // the cursor advances. Without this the idle gapless-prep decoder
            // would pin a CPU core.
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = tokio::time::sleep(Duration::from_millis(PREFETCH_IDLE_POLL_MS)) => {}
            }
            continue;
        }

        for idx in missing {
            if cancel.is_cancelled() {
                return;
            }
            // Re-check under the lock: the cursor may have moved or the read-path
            // may have filled this chunk while we were deciding.
            if store.lock().contains(idx) {
                continue;
            }

            let chunk_url = url.get();
            let fetched = tokio::select! {
                _ = cancel.cancelled() => return,
                bytes = fetch_chunk_async(&client, &chunk_url, idx, content_length) => bytes,
            };

            match fetched {
                Some(bytes) if !bytes.is_empty() => {
                    let mut store = store.lock();
                    if !store.contains(idx) {
                        store.insert(idx, Arc::new(bytes));
                    }
                    tracing::trace!(
                        "📥 [HTTP] Prefetched chunk {idx} ({} bytes)",
                        store.chunks.len()
                    );
                }
                _ => {
                    // Failed / empty: back off briefly and re-evaluate from the
                    // (possibly advanced) cursor rather than hammering.
                    tokio::select! {
                        _ = cancel.cancelled() => return,
                        _ = tokio::time::sleep(Duration::from_millis(PREFETCH_IDLE_POLL_MS)) => {}
                    }
                    break;
                }
            }
        }
    }
}

/// Fetch one chunk via the async client. Returns `None` on any error so the
/// read-path fallback can still recover.
async fn fetch_chunk_async(
    client: &reqwest::Client,
    url: &str,
    idx: u64,
    content_length: u64,
) -> Option<Vec<u8>> {
    let fetch_start = idx * CHUNK_SIZE;
    let fetch_end = ((idx + 1) * CHUNK_SIZE).min(content_length);
    if fetch_start >= content_length {
        return None;
    }
    let range_header = format!("bytes={}-{}", fetch_start, fetch_end - 1);

    let response = client
        .get(url)
        .header("Range", range_header)
        .send()
        .await
        .ok()?;
    if !answer_is_chunk(
        response.status(),
        response.headers(),
        fetch_start,
        fetch_end,
        content_length,
    ) {
        return None;
    }
    let bytes = response.bytes().await.ok()?;
    (bytes.len() as u64 == fetch_end - fetch_start).then(|| bytes.to_vec())
}

impl Read for RangeHttpReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // Publish the read position so the prefetch task can track the playhead.
        self.read_cursor.store(self.position, Ordering::Relaxed);
        self.maybe_spawn_prefetch();
        self.last_chunk_index = Some(Self::chunk_index(self.position));

        let bytes_read = self.read_from_cache(self.position, buf)?;
        self.position += bytes_read as u64;
        Ok(bytes_read)
    }
}

impl Seek for RangeHttpReader {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let new_pos = match pos {
            SeekFrom::Start(p) => p,
            SeekFrom::End(p) => {
                if p >= 0 {
                    self.content_length.saturating_add(p as u64)
                } else {
                    self.content_length.saturating_sub((-p) as u64)
                }
            }
            SeekFrom::Current(p) => {
                if p >= 0 {
                    self.position.saturating_add(p as u64)
                } else {
                    self.position.saturating_sub((-p) as u64)
                }
            }
        };

        self.position = new_pos.min(self.content_length);
        // Re-anchor the prefetch task's window forward from the seek target.
        self.read_cursor.store(self.position, Ordering::Relaxed);

        // PREFETCH: When seeking to a new position, synchronously prefetch the
        // chunk at that position and the next. This is critical for the FLAC
        // demuxer's binary-search resync, which reads forward from the seek
        // position. (We do NOT spawn/respawn the background task here — only the
        // cursor moves.)
        // Best-effort: a chunk that fails here is fetched again by the read
        // that needs it, which reports the error.
        let target_chunk = Self::chunk_index(self.position);
        if let Err(e) = self.ensure_chunk(target_chunk) {
            tracing::debug!("seek prefetch of chunk {target_chunk} failed: {e}");
        }
        if (target_chunk + 1) * CHUNK_SIZE < self.content_length
            && let Err(e) = self.ensure_chunk(target_chunk + 1)
        {
            tracing::debug!("seek prefetch of chunk {} failed: {e}", target_chunk + 1);
        }

        Ok(self.position)
    }
}

impl MediaSource for RangeHttpReader {
    fn is_seekable(&self) -> bool {
        true // Range requests allow seeking
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.content_length)
    }
}

impl Drop for RangeHttpReader {
    /// Cancel and DETACH the prefetch task — never join. `Drop` fires on a tokio
    /// worker during the gapless decoder swap (`*decoder.lock().await = next`),
    /// so a blocking join would park that worker for up to the request timeout.
    /// `cancel()` + `abort()` are non-blocking and need no runtime context, so
    /// this is safe on every thread (mirrors `AsyncNetworkBuffer::drop`).
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(handle) = self.prefetch.take() {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arc(bytes: &[u8]) -> Arc<Vec<u8>> {
        Arc::new(bytes.to_vec())
    }

    /// A reader handed its whole first chunk serves it with no request (the
    /// URL here is unreachable): the decoder's one GET for a requested
    /// transcode already fetched exactly those bytes.
    #[test]
    fn a_reader_handed_its_first_chunk_reads_it_without_a_request() {
        let body: Vec<u8> = (0..5_000u32).map(|i| (i % 251) as u8).collect();
        let mut reader = RangeHttpReader::with_first_chunk(
            "http://127.0.0.1:9/rest/stream?id=s1".to_string(),
            body.len() as u64,
            body.clone(),
        );
        let mut out = Vec::new();
        reader.read_to_end(&mut out).expect("read the seeded chunk");
        assert_eq!(out, body);
    }

    /// How [`spawn_chunk_server`] answers a Range request.
    #[derive(Clone, Copy)]
    enum ChunkReply {
        /// Like `http.ServeContent`: a 206 for exactly the range asked for.
        WellBehaved,
        /// The Range header stripped on the way: 200 and the whole file.
        IgnoresRange,
        /// A 206 for the range one byte later than asked.
        WrongStart,
        /// A 206 for only the first 1000 bytes of the range asked.
        Capped,
    }

    /// A server answering Range requests for `body` per `reply`. Returns the
    /// URL and the number of requests served.
    async fn spawn_chunk_server(
        reply: ChunkReply,
        body: Vec<u8>,
    ) -> (String, Arc<std::sync::atomic::AtomicUsize>) {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };
        let body = Arc::new(body);
        let served = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = served.clone();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                count.fetch_add(1, Ordering::SeqCst);
                let body = body.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let len = socket.read(&mut buf).await.unwrap_or(0);
                    let text = String::from_utf8_lossy(&buf[..len]).into_owned();
                    let (start, end) = text
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("range")
                                .then(|| value.trim().to_owned())
                        })
                        .and_then(|r| {
                            let (start, end) = r.strip_prefix("bytes=")?.split_once('-')?;
                            Some((start.parse::<usize>().ok()?, end.parse::<usize>().ok()?))
                        })
                        .unwrap_or((0, body.len() - 1));
                    let end = end.min(body.len() - 1);
                    let (status, range, payload) = match reply {
                        ChunkReply::WellBehaved => (
                            "206 Partial Content",
                            Some((start, end)),
                            &body[start..=end],
                        ),
                        ChunkReply::IgnoresRange => ("200 OK", None, &body[..]),
                        ChunkReply::WrongStart => {
                            let (s, e) = (start + 1, (end + 1).min(body.len() - 1));
                            ("206 Partial Content", Some((s, e)), &body[s..=e])
                        }
                        ChunkReply::Capped => {
                            let e = (start + 999).min(end);
                            ("206 Partial Content", Some((start, e)), &body[start..=e])
                        }
                    };
                    let mut headers = format!(
                        "HTTP/1.1 {status}\r\nAccept-Ranges: bytes\r\nContent-Length: {}\r\n",
                        payload.len()
                    );
                    if let Some((s, e)) = range {
                        headers
                            .push_str(&format!("Content-Range: bytes {s}-{e}/{}\r\n", body.len()));
                    }
                    headers.push_str("Connection: close\r\n\r\n");
                    let _ = socket.write_all(headers.as_bytes()).await;
                    let _ = socket.write_all(payload).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        (format!("http://{addr}/rest/stream?id=s1"), served)
    }

    fn body_of(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i % 251) as u8).collect()
    }

    /// Read all of `reader` the way the decoder does (on a runtime thread).
    fn read_all(reader: &mut RangeHttpReader) -> std::io::Result<Vec<u8>> {
        let mut out = Vec::new();
        tokio::task::block_in_place(|| reader.read_to_end(&mut out))?;
        Ok(out)
    }

    /// A server that answers ranges properly is read whole, its short last
    /// chunk included.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_well_behaved_server_is_read_whole_short_last_chunk_included() {
        let body = body_of(CHUNK_SIZE as usize + 1_000);
        let (url, _served) = spawn_chunk_server(ChunkReply::WellBehaved, body.clone()).await;
        let mut reader = RangeHttpReader::new(url, body.len() as u64);

        assert_eq!(read_all(&mut reader).expect("read it all"), body);
    }

    /// A one-chunk file whose Range header was stripped comes back whole as
    /// a 200: that is its first chunk, so it is taken.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_one_chunk_file_answered_whole_is_its_first_chunk() {
        let body = body_of(5_000);
        let (url, _served) = spawn_chunk_server(ChunkReply::IgnoresRange, body.clone()).await;
        let mut reader = RangeHttpReader::new(url, body.len() as u64);

        assert_eq!(read_all(&mut reader).expect("read it all"), body);
    }

    /// A later chunk answered with the whole file (a 200) must not be stored
    /// as that chunk: the decoder would get the file's start in the middle.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_later_chunk_answered_with_the_whole_file_is_an_error() {
        let body = body_of(CHUNK_SIZE as usize * 2);
        let (url, _served) = spawn_chunk_server(ChunkReply::IgnoresRange, body.clone()).await;
        let mut reader = RangeHttpReader::with_first_chunk(
            url,
            body.len() as u64,
            body[..CHUNK_SIZE as usize].to_vec(),
        );

        assert!(read_all(&mut reader).is_err());
    }

    /// A 206 that starts at another byte than asked is no chunk.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_206_that_does_not_start_at_the_byte_asked_is_an_error() {
        let body = body_of(CHUNK_SIZE as usize + 1_000);
        let (url, _served) = spawn_chunk_server(ChunkReply::WrongStart, body.clone()).await;
        let mut reader = RangeHttpReader::new(url, body.len() as u64);

        assert!(read_all(&mut reader).is_err());
    }

    /// A 206 the server capped short of the range asked must not be stored
    /// as a short chunk: the read would end there and the track stop partway.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_capped_206_is_an_error_not_a_short_read() {
        let body = body_of(CHUNK_SIZE as usize + 1_000);
        let (url, _served) = spawn_chunk_server(ChunkReply::Capped, body.clone()).await;
        let mut reader = RangeHttpReader::new(url, body.len() as u64);

        assert!(read_all(&mut reader).is_err());
    }

    /// The read-ahead task takes chunks by the same rule: only the range it
    /// asked for.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_read_ahead_takes_only_the_range_it_asked_for() {
        let body = body_of(CHUNK_SIZE as usize * 2 + 1_000);
        let total = body.len() as u64;
        let client = reqwest::Client::new();
        for (reply, takes) in [
            (ChunkReply::WellBehaved, true),
            (ChunkReply::IgnoresRange, false),
            (ChunkReply::WrongStart, false),
            (ChunkReply::Capped, false),
        ] {
            let (url, _served) = spawn_chunk_server(reply, body.clone()).await;
            let fetched = fetch_chunk_async(&client, &url, 1, total).await;
            assert_eq!(
                fetched.is_some(),
                takes,
                "chunk 1 under {:?}",
                std::mem::discriminant(&reply)
            );
            if let Some(bytes) = fetched {
                assert_eq!(bytes, body[CHUNK_SIZE as usize..CHUNK_SIZE as usize * 2]);
            }
        }
    }

    /// Only the whole first chunk is taken: a short one would leave a hole
    /// the reader serves as if it were the file.
    #[test]
    fn a_partial_first_chunk_is_not_taken() {
        let reader = RangeHttpReader::with_first_chunk(
            "http://127.0.0.1:9/rest/stream?id=s1".to_string(),
            CHUNK_SIZE * 2,
            vec![7; 1_000],
        );
        assert!(!reader.store.lock().contains(0));
        let first = RangeHttpReader::with_first_chunk(
            "http://127.0.0.1:9/rest/stream?id=s1".to_string(),
            CHUNK_SIZE * 2,
            vec![7; CHUNK_SIZE as usize],
        );
        assert!(first.store.lock().contains(0));
        assert_eq!(RangeHttpReader::first_chunk_len(1_000), 1_000);
        assert_eq!(RangeHttpReader::first_chunk_len(CHUNK_SIZE * 3), CHUNK_SIZE);
    }

    #[test]
    fn chunk_store_get_bumps_lru_and_inserts_evict_oldest() {
        let mut store = ChunkStore::new();
        for idx in 0..(MAX_CACHED_CHUNKS as u64) {
            store.insert(idx, arc(&[idx as u8]));
        }
        assert_eq!(store.chunks.len(), MAX_CACHED_CHUNKS);
        // Touch chunk 0 so it is most-recently-used; chunk 1 becomes oldest.
        assert!(store.get(0).is_some());
        // Insert a new chunk: must evict the oldest (1), keep 0.
        store.insert(MAX_CACHED_CHUNKS as u64, arc(&[42]));
        assert_eq!(store.chunks.len(), MAX_CACHED_CHUNKS);
        assert!(store.contains(0), "recently-touched chunk must survive");
        assert!(!store.contains(1), "oldest chunk must be evicted");
    }

    #[test]
    fn insert_chunk_dedups_order_so_lru_stays_consistent() {
        let mut store = ChunkStore::new();
        // Re-insert the same index (read-path + prefetch racing on one chunk).
        store.insert(7, arc(&[1]));
        store.insert(7, arc(&[2]));
        assert_eq!(store.chunks.len(), 1, "no duplicate chunk");
        assert_eq!(
            store.chunk_order.len(),
            store.chunks.len(),
            "chunk_order must not accumulate duplicates (LRU accounting stays consistent)"
        );
        assert_eq!(store.chunk_order.iter().filter(|&&i| i == 7).count(), 1);
    }

    #[test]
    fn chunks_to_prefetch_returns_window_then_empty_when_resident() {
        let content_length = CHUNK_SIZE * 100; // plenty of chunks
        let mut store = ChunkStore::new();
        // Cursor in chunk 0 → window is chunks 1..=5.
        let want: Vec<u64> = (1..=PREFETCH_WINDOW_CHUNKS as u64).collect();
        assert_eq!(chunks_to_prefetch(0, content_length, &store), want);

        // Fill the window: now nothing to prefetch (the task goes idle).
        for idx in &want {
            store.insert(*idx, arc(&[0]));
        }
        assert!(
            chunks_to_prefetch(0, content_length, &store).is_empty(),
            "a fully-resident window must yield no work (no busy-spin)"
        );
    }

    #[test]
    fn chunks_to_prefetch_stops_at_eof() {
        // Only ~2 chunks of content; the window must not run past EOF.
        let content_length = CHUNK_SIZE + 10;
        let store = ChunkStore::new();
        // Cursor in chunk 0 → only chunk 1 exists within the file.
        assert_eq!(chunks_to_prefetch(0, content_length, &store), vec![1]);
        // Cursor in the last chunk → nothing ahead.
        assert!(chunks_to_prefetch(CHUNK_SIZE, content_length, &store).is_empty());
    }

    #[test]
    fn resident_count_and_window_capacity_track_the_forward_window() {
        let content_length = CHUNK_SIZE * 100;
        let mut store = ChunkStore::new();
        assert_eq!(window_capacity(0, content_length), PREFETCH_WINDOW_CHUNKS);
        assert_eq!(resident_ahead_count(0, content_length, &store), 0);
        store.insert(1, arc(&[0]));
        store.insert(2, arc(&[0]));
        assert_eq!(resident_ahead_count(0, content_length, &store), 2);
        // Invariant the latch relies on: resident + missing == capacity.
        assert_eq!(
            resident_ahead_count(0, content_length, &store)
                + chunks_to_prefetch(0, content_length, &store).len(),
            window_capacity(0, content_length)
        );
    }

    #[test]
    fn window_capacity_clamps_near_eof() {
        let content_length = CHUNK_SIZE + 10; // ~2 chunks total
        assert_eq!(window_capacity(0, content_length), 1);
        assert_eq!(window_capacity(CHUNK_SIZE, content_length), 0);
    }

    #[test]
    fn update_refilling_batches_via_low_watermark_hysteresis() {
        let cap = PREFETCH_WINDOW_CHUNKS;
        let low = PREFETCH_LOW_WATERMARK_CHUNKS;
        // Drop below low → begin a refill burst.
        assert!(update_refilling(low - 1, cap, false));
        // Window full → stop refilling.
        assert!(!update_refilling(cap, cap, true));
        // Within the band → hold prior state (don't top up one chunk at a time).
        assert!(!update_refilling(cap - 1, cap, false));
        assert!(update_refilling(low + 1, cap, true));
        // Near EOF (capacity < low): "full" as soon as resident == capacity.
        assert!(!update_refilling(1, 1, false));
    }

    #[test]
    fn warm_cache_read_serves_from_store_without_network() {
        // A poisoned URL guarantees any network fetch fails; if read() returns
        // the cached bytes, it proved the hot path is memcpy-only (no fetch).
        let mut reader = RangeHttpReader::new("http://127.0.0.1:1/dead".to_string(), 8);
        reader
            .store
            .lock()
            .insert(0, arc(&[1, 2, 3, 4, 5, 6, 7, 8]));

        let mut buf = [0u8; 4];
        let n = reader
            .read(&mut buf)
            .expect("cached read must not touch the network");
        assert_eq!(n, 4);
        assert_eq!(&buf, &[1, 2, 3, 4]);
        assert_eq!(reader.position, 4);
        // read() publishes the cursor for the prefetch task.
        assert_eq!(reader.read_cursor.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn seek_publishes_cursor_without_spawning() {
        let mut reader =
            RangeHttpReader::new("http://127.0.0.1:1/dead".to_string(), CHUNK_SIZE * 10);
        // Pre-cache the seek-target chunks so the synchronous seek prefetch is a
        // no-op (no network). Seek to the start of chunk 3.
        reader.store.lock().insert(3, arc(&[0; 16]));
        reader.store.lock().insert(4, arc(&[0; 16]));

        let pos = reader.seek(SeekFrom::Start(CHUNK_SIZE * 3)).expect("seek");
        assert_eq!(pos, CHUNK_SIZE * 3);
        assert_eq!(reader.read_cursor.load(Ordering::Relaxed), CHUNK_SIZE * 3);
        assert!(
            reader.prefetch.is_none(),
            "seek must never spawn the prefetch task"
        );
    }

    /// Regression guard for the F4-class hazard: `Drop` stops the prefetch task
    /// via `cancel()` + `abort()` — never a blocking join. This tests that exact
    /// primitive (the two lines `Drop` runs) on a never-resolving task: it must
    /// stop promptly, and awaiting it must not hang or park a worker.
    ///
    /// (We don't drop a full `RangeHttpReader` here: the two lines are what
    /// `Drop` adds over the blocking client's own drop, which only joins the
    /// client's thread and is unchanged from the pre-read-ahead reader.)
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancel_and_abort_stops_prefetch_task_promptly() {
        let cancel = CancellationToken::new();
        let task_cancel = cancel.clone();
        let handle = tokio::spawn(async move {
            // Never finishes on its own; only cancellation/abort ends it.
            loop {
                tokio::select! {
                    _ = task_cancel.cancelled() => return,
                    _ = tokio::time::sleep(Duration::from_secs(3600)) => {}
                }
            }
        });

        // Exactly what Drop does — non-blocking, no join.
        cancel.cancel();
        handle.abort();

        // Must end well under the 4s per-request timeout, proving no join/park.
        let joined = tokio::time::timeout(Duration::from_millis(500), handle).await;
        assert!(
            joined.is_ok(),
            "cancel()+abort() must stop the prefetch task promptly (never join)"
        );
    }
}
