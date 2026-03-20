//! HTTP reader with Range request support for random access
//!
//! Fetches audio data using HTTP Range requests, allowing Symphonia to read
//! any part of a file without downloading everything. Uses a sparse cache
//! with LRU eviction to bound memory usage.

use std::io::{Read, Seek, SeekFrom};

use symphonia::core::io::MediaSource;

const CHUNK_SIZE: u64 = 256 * 1024; // 256KB chunks
/// Maximum chunks to keep in cache (~4MB total)
/// Larger cache reduces re-fetches during sequential playback and decoder backtracks
const MAX_CACHED_CHUNKS: usize = 16;

/// HTTP reader with Range request support for random access
///
/// This reader fetches audio data using HTTP Range requests, allowing
/// Symphonia to read any part of the file without downloading everything.
/// Uses a sparse cache with LRU eviction to bound memory usage.
pub(super) struct RangeHttpReader {
    /// HTTP client for making requests
    client: reqwest::blocking::Client,
    /// URL to fetch from
    url: String,
    /// Total content length
    content_length: u64,
    /// Current read position
    position: u64,
    /// Sparse cache: maps chunk_index -> chunk data
    /// Each chunk is 256KB
    chunks: std::collections::HashMap<u64, Vec<u8>>,
    /// LRU tracking: newest at back, oldest at front
    /// Used to evict old chunks when cache exceeds MAX_CACHED_CHUNKS
    chunk_order: std::collections::VecDeque<u64>,
}

impl RangeHttpReader {
    /// Create a new Range-based HTTP reader
    pub(super) fn new(url: String, content_length: u64) -> Self {
        Self {
            client: Self::create_client(),
            url,
            content_length,
            position: 0,
            chunks: std::collections::HashMap::new(),
            chunk_order: std::collections::VecDeque::new(),
        }
    }

    /// Create a new HTTP client with optimal settings for streaming
    fn create_client() -> reqwest::blocking::Client {
        reqwest::blocking::Client::builder()
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

    /// Get the chunk index for a byte position
    fn chunk_index(pos: u64) -> u64 {
        pos / CHUNK_SIZE
    }

    /// Fetch a chunk if not already cached, with retry logic
    /// Implements LRU eviction to bound memory usage
    fn ensure_chunk(&mut self, chunk_idx: u64) -> std::io::Result<()> {
        if self.chunks.contains_key(&chunk_idx) {
            // Cache hit: update LRU order (move to back = most recently used)
            self.chunk_order.retain(|&idx| idx != chunk_idx);
            self.chunk_order.push_back(chunk_idx);
            return Ok(());
        }

        let fetch_start = chunk_idx * CHUNK_SIZE;
        let fetch_end = ((chunk_idx + 1) * CHUNK_SIZE).min(self.content_length);

        if fetch_start >= self.content_length {
            return Ok(()); // Beyond EOF
        }

        let range_header = format!("bytes={}-{}", fetch_start, fetch_end - 1);

        // Retry logic for transient connection failures
        // Use more retries with longer backoff for streaming stability
        const MAX_RETRIES: u32 = 5;
        let mut last_error = None;

        for attempt in 0..MAX_RETRIES {
            tracing::trace!(
                "📥 [HTTP] Fetching chunk {} (bytes {}-{}), attempt {}/{}",
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

                    // Log before blocking read - this helps diagnose hangs
                    let read_start = std::time::Instant::now();
                    tracing::trace!("📥 [HTTP] Chunk {} - reading response body...", chunk_idx);

                    let bytes = response.bytes().map_err(|e| {
                        std::io::Error::other(format!("Failed to read response: {e}"))
                    })?;

                    let read_elapsed = read_start.elapsed();

                    // Validate we actually got data - empty response is an error
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

                    // Log success (always log if slow or after retry)
                    if attempt > 0 || read_elapsed.as_millis() > 1000 {
                        tracing::debug!(
                            "📥 [HTTP] Chunk {} fetch completed (attempt {}/{}, read took {:?}, {} bytes)",
                            chunk_idx,
                            attempt + 1,
                            MAX_RETRIES,
                            read_elapsed,
                            bytes.len()
                        );
                    }

                    // Calculate total chunks in file for logging
                    let total_chunks = self.content_length.div_ceil(CHUNK_SIZE);

                    // LRU eviction: remove oldest chunks if cache is full
                    while self.chunks.len() >= MAX_CACHED_CHUNKS {
                        if let Some(oldest_idx) = self.chunk_order.pop_front() {
                            if self.chunks.remove(&oldest_idx).is_some() {
                                tracing::trace!(
                                    "🗑️ [HTTP] Evicted chunk {} from cache (LRU, cache was full)",
                                    oldest_idx
                                );
                            }
                        } else {
                            break; // Safety: no more chunks to evict
                        }
                    }

                    // Insert new chunk and track in LRU order
                    self.chunks.insert(chunk_idx, bytes.to_vec());
                    self.chunk_order.push_back(chunk_idx);

                    // Log cache status (trace level - only with RUST_LOG=trace)
                    tracing::trace!(
                        "📊 [HTTP] Cache: chunk {}/{} fetched, {} chunks cached (~{}KB), max {}",
                        chunk_idx,
                        total_chunks,
                        self.chunks.len(),
                        (self.chunks.len() as u64 * CHUNK_SIZE) / 1024,
                        MAX_CACHED_CHUNKS
                    );

                    return Ok(());
                }
                Err(e) => {
                    last_error = Some(e);

                    if attempt < MAX_RETRIES - 1 {
                        // Recreate client to force fresh connection
                        // Use longer backoff: 500ms, 1s, 1.5s, 2s
                        let backoff_ms = 500 * (attempt as u64 + 1);
                        tracing::warn!(
                            "⚠️ [HTTP] Chunk {} fetch failed (attempt {}/{}), retrying in {}ms: {:?}",
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

        // All retries exhausted - log prominently
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

    /// Read bytes from cache, fetching chunks as needed
    fn read_from_cache(&mut self, offset: u64, buf: &mut [u8]) -> std::io::Result<usize> {
        if offset >= self.content_length {
            return Ok(0); // EOF
        }

        let mut bytes_read = 0;
        let mut current_pos = offset;
        let mut last_chunk_idx = None;

        while bytes_read < buf.len() && current_pos < self.content_length {
            let chunk_idx = Self::chunk_index(current_pos);
            self.ensure_chunk(chunk_idx)?;
            last_chunk_idx = Some(chunk_idx);

            if let Some(chunk) = self.chunks.get(&chunk_idx) {
                let chunk_start = chunk_idx * CHUNK_SIZE;
                let offset_in_chunk = (current_pos - chunk_start) as usize;

                // Defensive check: ensure offset is within chunk bounds
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
            } else {
                tracing::warn!(
                    " [HTTP] Chunk {} not in cache after ensure_chunk succeeded",
                    chunk_idx
                );
                break;
            }
        }

        // PREFETCH: After reading, speculatively fetch the next chunk.
        // This keeps the cache ahead of the decoder, reducing stalls
        // during sequential playback over the network.
        if let Some(last_idx) = last_chunk_idx {
            let next_chunk = last_idx + 1;
            if next_chunk * CHUNK_SIZE < self.content_length
                && !self.chunks.contains_key(&next_chunk)
            {
                let _ = self.ensure_chunk(next_chunk);
            }
        }

        Ok(bytes_read)
    }
}

impl Read for RangeHttpReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
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

        // PREFETCH: When seeking to a new position, prefetch the chunk at that position
        // and the next chunk. This is critical for FLAC demuxer which does binary search
        // seeking that calls resync() multiple times, each doing byte-by-byte reads.
        // Without prefetching, each seek causes a new HTTP request which stalls the UI.
        let target_chunk = Self::chunk_index(self.position);
        let _ = self.ensure_chunk(target_chunk);
        // Also prefetch next chunk since resync reads forward from the seek position
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
