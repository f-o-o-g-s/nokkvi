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

const CHUNK_SIZE: u64 = 256 * 1024; // 256KB chunks
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

/// HTTP reader with Range request support for random access.
pub(super) struct RangeHttpReader {
    /// Blocking client for the synchronous read-path miss-fallback.
    client: reqwest::blocking::Client,
    /// URL to fetch from.
    url: String,
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
            url,
            content_length,
            position: 0,
            store: Arc::new(Mutex::new(ChunkStore::new())),
            read_cursor: Arc::new(AtomicU64::new(0)),
            cancel: CancellationToken::new(),
            prefetch: None,
            last_chunk_index: None,
        }
    }

    /// Create a new blocking HTTP client with optimal settings for streaming.
    fn create_client() -> reqwest::blocking::Client {
        reqwest::blocking::Client::builder()
            // Identify as nokkvi (shared crate UA) so the server sees one player.
            .user_agent(crate::USER_AGENT)
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

            let result = self
                .client
                .get(&self.url)
                .header("Range", &range_header)
                .send();

            match result {
                Ok(response) => {
                    if !response.status().is_success()
                        && response.status() != reqwest::StatusCode::PARTIAL_CONTENT
                    {
                        return Err(std::io::Error::other(format!(
                            "HTTP Range request failed with status: {}",
                            response.status()
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

                    if bytes.is_empty() {
                        tracing::warn!(
                            "⚠️ [HTTP] Chunk {} returned empty response (expected {} bytes), treating as error",
                            chunk_idx,
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
    url: String,
    content_length: u64,
    store: Arc<Mutex<ChunkStore>>,
    read_cursor: Arc<AtomicU64>,
    cancel: CancellationToken,
) {
    let client = match reqwest::Client::builder()
        .user_agent(crate::USER_AGENT)
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

            let fetched = tokio::select! {
                _ = cancel.cancelled() => return,
                bytes = fetch_chunk_async(&client, &url, idx, content_length) => bytes,
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
    if !response.status().is_success() && response.status() != reqwest::StatusCode::PARTIAL_CONTENT
    {
        return None;
    }
    let bytes = response.bytes().await.ok()?;
    Some(bytes.to_vec())
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
        let target_chunk = Self::chunk_index(self.position);
        let _ = self.ensure_chunk(target_chunk);
        if (target_chunk + 1) * CHUNK_SIZE < self.content_length {
            let _ = self.ensure_chunk(target_chunk + 1);
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
    /// (We don't drop a full `RangeHttpReader` here: its `reqwest::blocking`
    /// client owns an internal runtime that cannot be dropped inside a
    /// `#[tokio::test]` async context. That blocking-client drop is unchanged
    /// from the pre-read-ahead reader, which is dropped at the same gapless-swap
    /// site in production; `Drop` only adds the non-blocking cancel+abort below.)
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
