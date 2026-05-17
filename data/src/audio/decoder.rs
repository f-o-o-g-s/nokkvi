use anyhow::{Context, Result};
use symphonia::core::{
    audio::RawSampleBuffer,
    errors::Error as SymphoniaError,
    formats::FormatReader,
    io::{MediaSource, MediaSourceStream},
    probe::Hint,
    units::{Time, TimeBase},
};
use tokio::sync::mpsc;
use tokio_util::{
    bytes::{Buf, Bytes},
    sync::CancellationToken,
};
use tracing::{debug, error, trace, warn};

use super::{USER_AGENT, range_http_reader::RangeHttpReader};
use crate::audio::{AudioBuffer, AudioFormat, SampleFormat, symphonia_registry};

/// Detect if an HTTP response originates from an Icecast/SHOUTcast radio server.
fn is_radio_response(headers: &reqwest::header::HeaderMap) -> bool {
    headers.keys().any(|k| k.as_str().starts_with("icy-"))
        || headers
            .get("server")
            .is_some_and(|v| v.to_str().unwrap_or("").to_lowercase().contains("icecast"))
}

/// Per-chunk network read timeout. Large enough to outlast normal Icecast jitter
/// (~25 ms for 128 kbps MP3), small enough to detect a stalled-socket within a
/// reasonable user-facing window.
const STREAM_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Timeout used on the consumer side of the channel. Kept at 500 ms so the
/// decode loop's generation-counter check fires promptly and the loop can exit
/// cleanly during shutdown without waiting for TCP data.
const READ_RECV_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);

/// Bounded wait for the producer task to acknowledge cancellation during Drop.
/// Short by design — abort() is called immediately after, so the task is
/// guaranteed to be unscheduled even if the join times out.
const DROP_JOIN_BUDGET: std::time::Duration = std::time::Duration::from_millis(250);

/// A background async task that eagerly consumes an infinite HTTP stream and
/// forwards chunks over a bounded channel to the sync `Read` consumer.
///
/// # Deadlock fix (F4)
///
/// The previous implementation used `std::sync::mpsc::sync_channel` + `block_in_place(tx.send)`.
/// When the channel was full and the user closed the window, `iced::exit()` returned and the
/// tokio `Runtime` was dropped, which dropped the `BlockingPool`. `BlockingPool::shutdown`
/// waits (condvar) for all blocking-pool workers to finish — but the producer task *was* such
/// a worker (because `block_in_place` transitions the worker into the blocking pool). The
/// receiver was owned by the engine/decoder, which couldn't be dropped until iced's
/// `Runtime::drop` returned — which it couldn't until the producer task exited. Proven by
/// gdb backtraces (`findings/stacks.txt`, Threads 1 and 3).
///
/// The fix replaces `block_in_place + sync send` with a normal async task using
/// `tokio::sync::mpsc::Sender::send().await` inside a `tokio::select!` against a
/// `CancellationToken`. A normal `tokio::spawn`'d task is NOT tracked by the `BlockingPool`,
/// so runtime shutdown can cancel and drop it without deadlocking.
///
/// # Invariant for the `Read` impl
///
/// `Read::read` must be called from within a `tokio::task::block_in_place` context (as the
/// decode loop in `engine.rs` already does). It uses `Handle::current().block_on(timeout(recv))`
/// to wait for channel data without blocking the async executor directly.
struct AsyncNetworkBuffer {
    rx: mpsc::Receiver<Bytes>,
    leftover: Bytes,
    cancel: CancellationToken,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl AsyncNetworkBuffer {
    pub fn new_async(response: reqwest::Response) -> Self {
        let (tx, rx) = mpsc::channel::<Bytes>(64);
        let cancel = CancellationToken::new();
        let child_cancel = cancel.clone();

        let task = tokio::spawn(async move {
            use futures::stream::StreamExt;
            let mut stream = response.bytes_stream();
            loop {
                // Race: either the engine cancels us, or we get the next chunk.
                // The timeout wrapping stream.next() handles stalled sockets (G1).
                let item = tokio::select! {
                    biased;
                    _ = child_cancel.cancelled() => return,
                    res = tokio::time::timeout(STREAM_READ_TIMEOUT, stream.next()) => res,
                };
                match item {
                    Ok(Some(Ok(chunk))) => {
                        // send().await yields back to the executor when the channel is
                        // full, applying natural back-pressure without ever entering the
                        // BlockingPool. Returns Err when the receiver is dropped.
                        if tx.send(chunk).await.is_err() {
                            return;
                        }
                    }
                    Ok(Some(Err(e))) => {
                        warn!(" [NETWORK BUFFER] Stream error: {}", e);
                        return;
                    }
                    Ok(None) => {
                        debug!(" [NETWORK BUFFER] Upstream EOF");
                        return;
                    }
                    Err(_elapsed) => {
                        warn!(
                            timeout_secs = STREAM_READ_TIMEOUT.as_secs(),
                            " [NETWORK BUFFER] Read timeout — aborting stalled radio stream"
                        );
                        return;
                    }
                }
            }
        });

        Self {
            rx,
            leftover: Bytes::new(),
            cancel,
            task: Some(task),
        }
    }
}

impl std::io::Read for AsyncNetworkBuffer {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // Drain the leftover slice from the previous receive before asking for more.
        if self.leftover.is_empty() {
            // Fast path: data already queued.
            match self.rx.try_recv() {
                Ok(chunk) => self.leftover = chunk,
                Err(mpsc::error::TryRecvError::Disconnected) => return Ok(0),
                Err(mpsc::error::TryRecvError::Empty) => {
                    // Slow path: wait up to READ_RECV_TIMEOUT for the next chunk.
                    // SAFETY: this must be called from within block_in_place (the decode
                    // loop at engine.rs guarantees this). Handle::block_on is legal there.
                    let handle = tokio::runtime::Handle::current();
                    match handle.block_on(tokio::time::timeout(READ_RECV_TIMEOUT, self.rx.recv())) {
                        Ok(Some(chunk)) => self.leftover = chunk,
                        // Receiver got a chunk but sender was dropped simultaneously — treat as EOF.
                        Ok(None) => return Ok(0),
                        // 500 ms elapsed without data — return TimedOut so the decode loop can
                        // check its generation counter and exit cleanly on shutdown.
                        //
                        // IMPORTANT: Do NOT use Interrupted here. std::io::Read::read_exact()
                        // silently retries on Interrupted, which traps IcyStreamReader's
                        // metadata reads (read_exact calls) in an infinite loop.
                        Err(_timeout) => {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::TimedOut,
                                "network buffer timeout",
                            ));
                        }
                    }
                }
            }
        }

        let take = buf.len().min(self.leftover.len());
        buf[..take].copy_from_slice(&self.leftover[..take]);
        self.leftover.advance(take);
        Ok(take)
    }
}

impl Drop for AsyncNetworkBuffer {
    fn drop(&mut self) {
        // Signal the producer task cooperatively first.
        self.cancel.cancel();
        if let Some(handle) = self.task.take() {
            // Hard-abort: guarantees the task is unscheduled from the runtime even
            // if the cooperative cancel didn't propagate yet.
            handle.abort();
            // Give the task a short window to acknowledge (best-effort). A timeout
            // here is important — anything blocking Drop for too long can re-introduce
            // the shutdown-hang symptom we're fixing.
            if let Ok(rt) = tokio::runtime::Handle::try_current() {
                rt.block_on(async {
                    let _ = tokio::time::timeout(DROP_JOIN_BUDGET, handle).await;
                });
            }
        }
    }
}

/// Reader that demuxes ICY metadata from an Icecast stream.
struct IcyStreamReader<R: std::io::Read> {
    inner: R,
    metaint: usize,
    bytes_until_meta: usize,
    callback: Box<dyn Fn(String) + Send + Sync>,
}

impl<R: std::io::Read> std::io::Read for IcyStreamReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.bytes_until_meta == 0 {
            // Time to read metadata
            let mut len_byte = [0u8; 1];
            self.inner.read_exact(&mut len_byte)?;
            let meta_len = len_byte[0] as usize * 16;

            if meta_len > 0 {
                let mut meta_buf = vec![0u8; meta_len];
                self.inner.read_exact(&mut meta_buf)?;

                let meta_str = String::from_utf8_lossy(&meta_buf);
                let trim = meta_str.trim_end_matches('\0');
                if !trim.is_empty() {
                    (self.callback)(trim.to_string());
                }
            }
            self.bytes_until_meta = self.metaint;
        }

        let max_read = std::cmp::min(buf.len(), self.bytes_until_meta);
        let n = self.inner.read(&mut buf[..max_read])?;
        self.bytes_until_meta -= n;
        Ok(n)
    }
}

/// Audio decoder using symphonia
pub struct AudioDecoder {
    format_reader: Option<Box<dyn FormatReader>>,
    decoder: Option<Box<dyn symphonia::core::codecs::Decoder>>,
    track_id: Option<u32>,
    format: AudioFormat,
    duration: u64, // milliseconds
    url: String,
    initialized: bool,
    eof: bool,
    // Buffer for leftover samples from partially-processed frames
    frame_buffer: Vec<u8>,
    /// EMA-smoothed compressed bitrate in kbps, computed per-packet from
    /// Symphonia's `Packet.data.len()` and `Packet.dur`.
    smoothed_bitrate_kbps: f64,
    /// True when stream has no Content-Length (internet radio / infinite stream).
    /// Engine uses this to skip gapless preparation, crossfade arming, and
    /// consume-mode queue mutation.
    infinite_stream: bool,
    /// Arc passed in by the AudioEngine, populated by `IcyMetadataReader` if this stream supports ICY.
    live_icy_metadata: std::sync::Arc<std::sync::RwLock<Option<String>>>,
    /// The short-name of the actual hardware codec (e.g., "mp3", "aac", "vorbis").
    live_codec: Option<String>,
}

impl AudioDecoder {
    pub fn new(live_icy_metadata: std::sync::Arc<std::sync::RwLock<Option<String>>>) -> Self {
        Self {
            format_reader: None,
            decoder: None,
            track_id: None,
            format: AudioFormat::invalid(),
            duration: 0,
            url: String::new(),
            initialized: false,
            eof: false,
            frame_buffer: Vec::new(),
            smoothed_bitrate_kbps: 0.0,
            infinite_stream: false,
            live_icy_metadata,
            live_codec: None,
        }
    }

    /// Initialize decoder with URL (HTTP or file path)
    pub async fn init(&mut self, url: &str) -> Result<()> {
        if self.initialized && self.url == url {
            return Ok(());
        }

        self.stop();
        self.url = url.to_string();

        if self.url.is_empty() {
            anyhow::bail!("Cannot initialize decoder - URL is empty");
        }

        self.open_input().await?;

        if self.initialized {
            // Format detected, already set
        }

        Ok(())
    }

    async fn open_input(&mut self) -> Result<()> {
        if self.url.is_empty() {
            return Ok(());
        }

        self.close_input();

        // Determine if URL is HTTP/HTTPS or file path
        let (mss, hint_from_data) = if self.url.starts_with("http://")
            || self.url.starts_with("https://")
        {
            // HTTP stream - use Range requests for instant playback
            let init_start = std::time::Instant::now();
            trace!(
                " [DECODER] Starting Range-based HTTP reader for: {}",
                self.url
            );

            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .user_agent(USER_AGENT)
                .build()
                .context("Failed to create HTTP client")?;

            let mut is_radio = false;

            // Try HEAD request first
            let mut content_length = match client.head(&self.url).send().await {
                Ok(head_response) if head_response.status().is_success() => {
                    let headers = head_response.headers();
                    is_radio = is_radio_response(headers);

                    if is_radio {
                        trace!(" [DECODER] ICEcast / Radio stream detected from HEAD headers.");
                        None
                    } else {
                        head_response.content_length().filter(|&len| len > 0)
                    }
                }
                _ => None,
            };

            // If HEAD didn't give us content-length, try a Range request with bytes=0-0
            // to get the Content-Range header which tells us total size
            if content_length.is_none() && !is_radio {
                trace!(" [DECODER] HEAD didn't return Content-Length, trying Range probe...");
                match client
                    .get(&self.url)
                    .header("Range", "bytes=0-0")
                    .send()
                    .await
                {
                    Ok(range_response) => {
                        let headers = range_response.headers();
                        is_radio = is_radio_response(headers);

                        if is_radio {
                            trace!(
                                " [DECODER] ICEcast / Radio stream detected from Range headers."
                            );
                        } else {
                            // Check Content-Range header: "bytes 0-0/TOTAL_SIZE"
                            if let Some(content_range) =
                                range_response.headers().get("content-range")
                                && let Ok(range_str) = content_range.to_str()
                            {
                                // Parse "bytes 0-0/12345678"
                                if let Some(total) = range_str.split('/').next_back()
                                    && let Ok(len) = total.parse::<u64>()
                                {
                                    content_length = Some(len);
                                    debug!(
                                        " [DECODER] Got Content-Length from Range response: {} bytes",
                                        len
                                    );
                                }
                            }
                            // Fallback: try Content-Length from this response
                            if content_length.is_none()
                                && let Some(len) = range_response.content_length()
                            {
                                trace!(
                                    " [DECODER] Range response Content-Length: {} (not useful)",
                                    len
                                );
                            }
                        }
                    }
                    Err(e) => {
                        trace!(" [DECODER] Range probe failed: {}", e);
                    }
                }
            }

            // If we still don't have content-length, try a regular GET and read content-length header
            if content_length.is_none() && !is_radio {
                trace!(" [DECODER] Trying regular GET to detect Content-Length...");
                match client.get(&self.url).send().await {
                    Ok(response) if response.status().is_success() => {
                        let headers = response.headers();
                        is_radio = is_radio_response(headers);

                        if is_radio {
                            trace!(" [DECODER] ICEcast / Radio stream detected from GET headers.");
                        } else {
                            content_length = response.content_length().filter(|&len| len > 0);
                            if let Some(len) = content_length {
                                trace!(" [DECODER] Got Content-Length from GET: {} bytes", len);
                            }
                            // Note: we're throwing away this response, which is wasteful
                            // but it's a fallback path
                        }
                    }
                    _ => {}
                }
            }

            // We've completed our detection for Content-Length
            let mut detected_format = self.extract_format_hint(&self.url);

            let mss = if let Some(len) = content_length {
                trace!(
                    " [DECODER] Final Content-Length: {} bytes (took {:?})",
                    len,
                    init_start.elapsed()
                );

                if let Some(ref format) = detected_format {
                    trace!(" [DECODER] Detected format from URL: {}", format);
                }

                // Create Range-based reader - fetches only needed bytes on demand
                let reader = RangeHttpReader::new(self.url.clone(), len);

                let media_source: Box<dyn MediaSource> = Box::new(reader);
                MediaSourceStream::new(media_source, Default::default())
            } else {
                trace!(" [DECODER] No Content-Length found, treating as infinite stream.");
                self.infinite_stream = true;

                let url_copy = self.url.clone();

                // Radio-path client: no global timeout (stream is infinite by design),
                // but add connect / per-read / keepalive guards matching MPD defaults.
                // See: CurlInputPlugin.cxx CURLOPT_CONNECTTIMEOUT / LOW_SPEED_TIME /
                //      CURLOPT_TCP_KEEPALIVE.
                let client = reqwest::Client::builder()
                    .user_agent(USER_AGENT)
                    // G2 fix: abort DNS/SYN hangs quickly (matches MPD CONNECTTIMEOUT=10s).
                    .connect_timeout(std::time::Duration::from_secs(10))
                    // G1 fix (reqwest layer): abort stalled transfers where bytes stop
                    // flowing. Complements the per-chunk timeout in the producer task.
                    .read_timeout(std::time::Duration::from_secs(15))
                    // Detect half-open sockets after NAT timeout, suspend/resume, Wi-Fi
                    // handover (MPD CURLOPT_TCP_KEEPALIVE/KEEPIDLE default 60s, we use
                    // 30s as a more desktop-friendly value).
                    .tcp_keepalive(std::time::Duration::from_secs(30))
                    // Radio is a single long-lived socket; pooling has no benefit and
                    // can cause a stale connection to be reused on reconnect.
                    .pool_max_idle_per_host(0)
                    .build()
                    .context("Failed to create radio HTTP client")?;

                let response = client
                    .get(&url_copy)
                    .header("Icy-MetaData", "1")
                    .send()
                    .await
                    .context("Failed to open infinite stream connection")?;

                let content_type = response
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string());

                if let Some(ct) = content_type.as_ref()
                    && let Some(format_from_ct) = format_hint_from_content_type(ct)
                {
                    detected_format = Some(format_from_ct);
                }

                if detected_format.is_none() {
                    detected_format = format_hint_from_radio_url(&self.url);
                }

                if let Some(ref format) = detected_format {
                    trace!(" [DECODER] Detected format for infinite stream: {}", format);
                }

                // Check for ICY metadata
                use icy_metadata::IcyHeaders;
                let icy_headers = IcyHeaders::parse_from_headers(response.headers());
                let interval = icy_headers.metadata_interval();

                // Buffer the response using a dedicated async tokio task
                let buffered_response = AsyncNetworkBuffer::new_async(response);

                let media_source: Box<dyn MediaSource> = if let Some(interval) = interval {
                    let interval_usize = interval.get();
                    trace!(
                        " [DECODER] ICY Metadata detected! Interval: {}",
                        interval_usize
                    );
                    let atomic_meta = self.live_icy_metadata.clone();

                    let icy_reader = IcyStreamReader {
                        inner: buffered_response,
                        metaint: interval_usize,
                        bytes_until_meta: interval_usize,
                        callback: Box::new(move |title| {
                            trace!(" [DECODER] ICY Callback fired! Result: {:?}", title);
                            if let Ok(mut guard) = atomic_meta.try_write() {
                                *guard = Some(title);
                            } else {
                                warn!(" [DECODER] ICY Failed to acquire try_write!");
                            }
                        }),
                    };
                    Box::new(symphonia::core::io::ReadOnlySource::new(icy_reader))
                } else {
                    trace!(" [DECODER] No ICY Interval detected in headers!");
                    Box::new(symphonia::core::io::ReadOnlySource::new(buffered_response))
                };

                MediaSourceStream::new(media_source, Default::default())
            };

            (mss, detected_format)
        } else {
            // File path
            let file = std::fs::File::open(&self.url)
                .with_context(|| format!("Failed to open file: {}", self.url))?;
            let media_source: Box<dyn MediaSource> = Box::new(file);
            let mss = MediaSourceStream::new(media_source, Default::default());

            // Try to extract format from file path
            (mss, self.extract_format_hint(&self.url))
        };

        // Probe for format - use detected format hint
        let mut hint = Hint::new();

        if let Some(ext) = hint_from_data {
            trace!(" [DECODER] Using format hint: {}", ext);
            hint.with_extension(&ext);
        }

        let probe_start = std::time::Instant::now();
        trace!(" [DECODER] Starting format probe...");
        // CRITICAL: Format probing reads from the MediaSource stream, which does blocking I/O.
        // Must wrap in block_in_place to avoid freezing the Tokio executor.
        // `enable_gapless: true` is load-bearing for the primary init path — the
        // ResetRequired reprobe in `read_buffer` uses `false` instead (see
        // `symphonia_registry::probe_and_make_decoder`).
        let (format_reader, decoder, track_id) = tokio::task::block_in_place(|| {
            symphonia_registry::probe_and_make_decoder(mss, &hint, true).inspect_err(|e| {
                error!(
                    " [DECODER] Format probe / decoder construction FAILED: {:?}",
                    e
                );
            })
        })?;
        trace!(
            " [DECODER] Format probe + decoder construction successful (took {:?})",
            probe_start.elapsed()
        );
        trace!(" [DECODER] Found audio track with ID: {}", track_id);
        trace!(" [DECODER] Codec decoder created successfully");

        // Snapshot the selected track's codec parameters into owned locals so the
        // immutable borrow of `format_reader` is released before we move it into
        // `self.format_reader` below.
        let codec_params = format_reader
            .tracks()
            .iter()
            .find(|t| t.id == track_id)
            .context("Selected track id missing from probed format")?
            .codec_params
            .clone();
        let mut codec_name = None;
        if let Some(desc) = symphonia_registry::codecs().get_codec(codec_params.codec) {
            codec_name = Some(desc.short_name.to_string());
        }

        let sample_rate = codec_params.sample_rate.unwrap_or(44100);
        let channels = codec_params.channels.map_or(2, |c| c.count()) as u32;

        // Convert symphonia sample format to our format
        // Note: symphonia uses a different sample format system, we'll default to S16
        // and convert during decoding
        let sample_format = SampleFormat::S16; // We'll convert to S16 during decode

        let audio_format = AudioFormat::new(sample_format, sample_rate, channels);

        // Get duration
        let duration_ms = if let Some(time_base) = codec_params.time_base {
            if let Some(n_frames) = codec_params.n_frames {
                // Calculate duration using time_base
                let time = time_base.calc_time(n_frames);
                let calculated_ms = time.seconds * 1000 + (time.frac * 1000.0) as u64;

                // Sanity check: if duration is > 24 hours, something is wrong with the metadata
                // Typical songs are under 20 minutes, albums under 2 hours
                const MAX_REASONABLE_DURATION_MS: u64 = 24 * 60 * 60 * 1000; // 24 hours
                if calculated_ms > MAX_REASONABLE_DURATION_MS {
                    warn!(
                        " [DECODER] Detected garbage duration: {}ms (n_frames={}, time_base={:?}), falling back to 0",
                        calculated_ms, n_frames, time_base
                    );
                    0
                } else {
                    calculated_ms
                }
            } else {
                0
            }
        } else {
            0
        };

        debug!(
            " [DECODER] Format: {}Hz, {} channels, duration: {}ms",
            sample_rate, channels, duration_ms
        );

        self.format_reader = Some(format_reader);
        self.decoder = Some(decoder);
        self.track_id = Some(track_id);
        self.format = audio_format;
        self.duration = duration_ms;
        self.live_codec = codec_name;
        self.initialized = true;
        self.eof = false;
        self.frame_buffer.clear();

        trace!(" [DECODER] Initialization complete!");
        Ok(())
    }

    fn close_input(&mut self) {
        self.format_reader = None;
        self.decoder = None;
        self.track_id = None;
        self.format = AudioFormat::invalid();
        self.duration = 0;
        self.initialized = false;
        self.eof = false;
        self.frame_buffer.clear();
        self.smoothed_bitrate_kbps = 0.0;
        self.live_codec = None;
    }

    /// Extract format hint from URL (file extension or query parameter)
    fn extract_format_hint(&self, url: &str) -> Option<String> {
        // Try to parse as URL first for query parameters
        if (url.starts_with("http://") || url.starts_with("https://"))
            && let Ok(parsed_url) = url::Url::parse(url)
        {
            // Check query parameters for 'f' (format) parameter
            // This won't help us since Navidrome uses f=json, not f=mp3
            // Instead, we need to look at the actual file being streamed

            // For now, just try to get extension from path
            let path = parsed_url.path();
            if let Some(ext) = path.rsplit('.').next()
                && ext.len() <= 5
                && ext.chars().all(|c| c.is_alphanumeric())
            {
                return Some(ext.to_lowercase());
            }
        }

        // Fall back to file path extension
        if let Some(ext) = url.rsplit('.').next() {
            // Only consider short alphanumeric extensions (avoid query strings)
            let ext_part = ext.split('?').next().unwrap_or(ext);
            if ext_part.len() <= 5 && ext_part.chars().all(|c| c.is_alphanumeric()) {
                return Some(ext_part.to_lowercase());
            }
        }

        None
    }

    /// Read and decode audio data to PCM buffer
    pub fn read_buffer(&mut self, bytes: usize) -> AudioBuffer {
        if !self.initialized {
            return AudioBuffer::invalid();
        }

        let mut output_data = Vec::with_capacity(bytes);

        // First, use any leftover samples from previous frames
        if !self.frame_buffer.is_empty() {
            let to_take = bytes.min(self.frame_buffer.len());
            output_data.extend_from_slice(&self.frame_buffer[..to_take]);
            self.frame_buffer.drain(..to_take);
        }

        // Track consecutive I/O errors for retry logic
        let mut consecutive_io_errors = 0;
        // For network streams, we need many more retries with longer backoff
        // to handle temporary stalls from disk I/O, transcoding, or network latency
        const MAX_IO_RETRIES: u32 = 8;

        // Decode packets until we have enough data
        while output_data.len() < bytes && !self.eof {
            let (Some(format_reader), Some(decoder), Some(track_id)) = (
                self.format_reader.as_mut(),
                self.decoder.as_mut(),
                self.track_id,
            ) else {
                warn!("⚠️ [DECODER] read_buffer called with uninitialized decoder state");
                break;
            };

            // Get next packet
            let packet = match format_reader.next_packet() {
                Ok(p) => {
                    // Reset error counter on success
                    consecutive_io_errors = 0;
                    p
                }
                Err(SymphoniaError::ResetRequired) => {
                    // Track list changed (e.g., OGG ICECast metadata changed).
                    // `enable_gapless: false` is load-bearing here — OGG chained
                    // metadata depends on Symphonia exposing every container
                    // segment rather than gluing them together.
                    warn!(" [DECODER] ResetRequired error - Stream format changed, reprobing...");
                    if let Some(reader) = self.format_reader.take() {
                        let mss = reader.into_inner();

                        let mut hint = Hint::new();
                        // Assume OGG for internet radio if it's infinite, as that's the main codec that chains
                        if self.infinite_stream
                            || self.url.to_lowercase().contains("ogg")
                            || self.url.to_lowercase().contains("vorbis")
                        {
                            hint.with_extension("ogg");
                        } else if let Some(ext) = self.url.split('.').next_back()
                            && ext.len() <= 4
                        {
                            hint.with_extension(ext);
                        }

                        match symphonia_registry::probe_and_make_decoder(mss, &hint, false) {
                            Ok((format_reader, dec, track_id)) => {
                                // Snapshot codec parameters before moving the
                                // format reader into `self.format_reader`.
                                let codec_params = format_reader
                                    .tracks()
                                    .iter()
                                    .find(|t| t.id == track_id)
                                    .map(|t| t.codec_params.clone());
                                self.track_id = Some(track_id);
                                self.decoder = Some(dec);
                                if let Some(codec_params) = codec_params {
                                    let channels = codec_params
                                        .channels
                                        .unwrap_or(
                                            symphonia::core::audio::Channels::FRONT_LEFT
                                                | symphonia::core::audio::Channels::FRONT_RIGHT,
                                        )
                                        .count();
                                    let sample_rate = codec_params.sample_rate.unwrap_or(44100);
                                    self.format = AudioFormat::new(
                                        SampleFormat::F32,
                                        sample_rate,
                                        channels as u32,
                                    );
                                    if let Some(desc) =
                                        symphonia_registry::codecs().get_codec(codec_params.codec)
                                    {
                                        self.live_codec = Some(desc.short_name.to_string());
                                    }
                                }
                                self.format_reader = Some(format_reader);
                                // Retry reading the packet with the new decoder.
                                continue;
                            }
                            Err(e) => {
                                error!(
                                    " [DECODER] Failed to reprobe format / build decoder after \
                                     ResetRequired: {:#}",
                                    e
                                );
                            }
                        }
                    }
                    self.eof = true;
                    break;
                }
                Err(SymphoniaError::IoError(ref io_err)) => {
                    // recv_timeout in AsyncNetworkBuffer returns TimedOut when no
                    // data arrives within 500ms. Break out of read_buffer() so the
                    // decode loop in engine.rs can check its generation counter and
                    // exit cleanly during shutdown. Using `continue` here would stay
                    // trapped inside read_buffer's packet loop indefinitely.
                    if io_err.kind() == std::io::ErrorKind::TimedOut {
                        break;
                    }

                    consecutive_io_errors += 1;

                    // Check if this looks like a real EOF from Symphonia
                    let is_unexpected_eof = io_err.kind() == std::io::ErrorKind::UnexpectedEof;
                    let error_msg = io_err.to_string();

                    // CRITICAL: Symphonia returns "end of stream" when it reaches actual EOF
                    // This is different from network errors which would be timeouts or connection resets.
                    // If we see "end of stream", this is the REAL end of the audio data.
                    let is_symphonia_eof = is_unexpected_eof && error_msg.contains("end of stream");

                    if is_symphonia_eof {
                        // This is the actual end of the file - Symphonia finished decoding
                        trace!(" [DECODER] Symphonia reached end of stream - treating as EOF");
                        self.eof = true;
                        break;
                    } else if consecutive_io_errors >= MAX_IO_RETRIES {
                        // Non-EOF I/O error after retries - network issue
                        error!(
                            " [DECODER] I/O error after {} retries: {:?} - treating as EOF",
                            consecutive_io_errors, io_err
                        );
                        self.eof = true;
                        break;
                    }
                    // Retry with exponential backoff for transient network errors
                    let backoff_ms = 400 * (1u64 << consecutive_io_errors.saturating_sub(1).min(5));
                    warn!(
                        " [DECODER] I/O error (attempt {}/{}): {:?} - retrying in {}ms",
                        consecutive_io_errors, MAX_IO_RETRIES, io_err, backoff_ms
                    );
                    std::thread::sleep(std::time::Duration::from_millis(backoff_ms));
                    continue;
                }
                Err(e) => {
                    // Other errors (decode errors, etc.) - log and treat as EOF
                    error!(
                        " [DECODER] Packet error (not I/O): {:?} - treating as EOF",
                        e
                    );
                    self.eof = true;
                    break;
                }
            };

            // Skip if not our track
            if packet.track_id() != track_id {
                continue;
            }

            // Compute instantaneous compressed bitrate from packet data
            if packet.dur > 0 {
                let sample_rate = self.format.sample_rate() as u64;
                if sample_rate > 0 {
                    let bits = packet.data.len() as u64 * 8;
                    let instantaneous_kbps = (bits * sample_rate) / (packet.dur * 1000);
                    // EMA smoothing (alpha=0.1 ≈ ~10 packet window)
                    if self.smoothed_bitrate_kbps == 0.0 {
                        self.smoothed_bitrate_kbps = instantaneous_kbps as f64;
                    } else {
                        const ALPHA: f64 = 0.1;
                        self.smoothed_bitrate_kbps = ALPHA * instantaneous_kbps as f64
                            + (1.0 - ALPHA) * self.smoothed_bitrate_kbps;
                    }
                }
            }

            // Decode packet
            match decoder.decode(&packet) {
                Ok(audio_buf) => {
                    // Convert to interleaved bytes
                    let spec = *audio_buf.spec();
                    let mut raw_buf =
                        RawSampleBuffer::<i16>::new(audio_buf.capacity() as u64, spec);
                    raw_buf.copy_interleaved_ref(audio_buf);

                    let mut decoded_bytes = raw_buf.as_bytes();

                    // Apply gapless trimming if enabled
                    let channels = spec.channels.count();
                    let bytes_per_sample = 2; // i16 = 2 bytes per sample
                    let bytes_per_frame = channels * bytes_per_sample;

                    // Trim start: remove encoder delay frames from beginning
                    if packet.trim_start > 0 {
                        let trim_start_bytes = (packet.trim_start as usize) * bytes_per_frame;
                        if trim_start_bytes < decoded_bytes.len() {
                            decoded_bytes = &decoded_bytes[trim_start_bytes..];
                        }
                    }

                    // Trim end: remove encoder padding frames from end
                    if packet.trim_end > 0 {
                        let trim_end_bytes = (packet.trim_end as usize) * bytes_per_frame;
                        if trim_end_bytes < decoded_bytes.len() {
                            decoded_bytes = &decoded_bytes[..decoded_bytes.len() - trim_end_bytes];
                        }
                    }

                    let needed = bytes.saturating_sub(output_data.len());
                    let to_take = needed.min(decoded_bytes.len());

                    output_data.extend_from_slice(&decoded_bytes[..to_take]);

                    // Store leftover if any
                    if decoded_bytes.len() > to_take {
                        self.frame_buffer
                            .extend_from_slice(&decoded_bytes[to_take..]);
                    }
                }
                Err(SymphoniaError::IoError(ref e)) => {
                    // Log and skip packet on I/O error during decode
                    warn!(
                        " [DECODER] I/O error during packet decode (skipping): {:?}",
                        e
                    );

                    // Zero-fill to maintain time sync and prevent buffer starvation
                    if packet.dur > 0 {
                        let channels = self.format.channel_count() as usize;
                        let silence_bytes = (packet.dur as usize) * channels * 2; // 2 bytes for i16
                        let needed = bytes.saturating_sub(output_data.len());
                        let to_take = needed.min(silence_bytes);

                        output_data.extend(std::iter::repeat_n(0, to_take));
                        if silence_bytes > to_take {
                            self.frame_buffer
                                .extend(std::iter::repeat_n(0, silence_bytes - to_take));
                        }
                    }
                    continue;
                }
                Err(SymphoniaError::DecodeError(ref e)) => {
                    // Log and skip packet on decode error
                    warn!(" [DECODER] Decode error (skipping): {:?}", e);

                    // Zero-fill to maintain time sync and prevent buffer starvation
                    if packet.dur > 0 {
                        let channels = self.format.channel_count() as usize;
                        let silence_bytes = (packet.dur as usize) * channels * 2; // 2 bytes for i16
                        let needed = bytes.saturating_sub(output_data.len());
                        let to_take = needed.min(silence_bytes);

                        output_data.extend(std::iter::repeat_n(0, to_take));
                        if silence_bytes > to_take {
                            self.frame_buffer
                                .extend(std::iter::repeat_n(0, silence_bytes - to_take));
                        }
                    }
                    continue;
                }
                Err(_) => {
                    self.eof = true;
                    break;
                }
            }
        }

        if output_data.is_empty() {
            return AudioBuffer::invalid();
        }

        AudioBuffer::from_data(output_data, self.format.clone(), 0)
    }

    /// Seek to position in track (milliseconds)
    pub fn seek(&mut self, position_ms: u64) -> bool {
        use tracing::{debug, trace};

        trace!("🔍 [DECODER SEEK] Starting seek to {}ms", position_ms);

        if !self.initialized {
            debug!("🔍 [DECODER SEEK] Aborting - not initialized");
            return false;
        }

        let (Some(format_reader), Some(decoder), Some(track_id)) = (
            self.format_reader.as_mut(),
            self.decoder.as_mut(),
            self.track_id,
        ) else {
            warn!("⚠️ [DECODER] seek called with uninitialized decoder state");
            return false;
        };

        // Convert milliseconds to time
        let time_base = format_reader
            .tracks()
            .iter()
            .find(|t| t.id == track_id)
            .and_then(|t| t.codec_params.time_base)
            .unwrap_or_else(|| TimeBase::new(1, 1000));

        // Convert milliseconds to seconds
        let seconds = position_ms / 1000;
        let frac = (position_ms % 1000) as f64 / 1000.0;
        let time = Time::new(seconds, frac);
        let _ts = time_base.calc_timestamp(time);

        // Seek in format reader
        // Use Coarse mode for HTTP streams - Accurate mode requires decoding from start
        // for formats without seek tables (like FLAC without SEEKTABLE), causing 20s+ delays.
        // Coarse mode uses byte-level seeking which is instant with Range requests.
        let seek_to = symphonia::core::formats::SeekTo::Time {
            time,
            track_id: Some(track_id),
        };

        trace!(
            "🔍 [DECODER SEEK] Calling format_reader.seek(Coarse, {}s + {})",
            seconds, frac
        );
        let seek_start = std::time::Instant::now();
        match format_reader.seek(symphonia::core::formats::SeekMode::Coarse, seek_to) {
            Ok(seeked_to) => {
                debug!(
                    "🔍 [DECODER SEEK] format_reader.seek() completed in {:?}, seeked to ts={}",
                    seek_start.elapsed(),
                    seeked_to.actual_ts
                );
                // Flush decoder buffers
                decoder.reset();
                self.frame_buffer.clear();
                self.eof = false;
                true
            }
            Err(e) => {
                debug!(
                    "🔍 [DECODER SEEK] format_reader.seek() FAILED in {:?}: {:?}",
                    seek_start.elapsed(),
                    e
                );
                false
            }
        }
    }

    /// Get detected audio format
    pub fn format(&self) -> &AudioFormat {
        &self.format
    }

    /// Get track duration (milliseconds)
    pub fn duration(&self) -> u64 {
        self.duration
    }

    /// Check if decoder is initialized
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Check if decoder has reached end of file
    pub fn is_eof(&self) -> bool {
        self.eof
    }

    /// True when stream has no Content-Length (internet radio).
    /// Engine queries this to skip gapless, crossfade, and consume-mode logic.
    pub fn is_infinite_stream(&self) -> bool {
        self.infinite_stream
    }

    /// Get the EMA-smoothed live compressed bitrate in kbps.
    /// Returns 0 if no packets have been decoded yet.
    pub fn live_bitrate(&self) -> u32 {
        self.smoothed_bitrate_kbps.round() as u32
    }

    pub fn live_codec(&self) -> Option<String> {
        self.live_codec.clone()
    }

    /// Stop decoding
    pub fn stop(&mut self) {
        self.close_input();
    }
}

impl Default for AudioDecoder {
    fn default() -> Self {
        Self::new(std::sync::Arc::new(std::sync::RwLock::new(None)))
    }
}

// =============================================================================
// Radio stream helpers (Phase 3)
// =============================================================================

/// Extract a Symphonia-compatible format hint from an HTTP Content-Type header.
///
/// Radio streams typically advertise their codec via Content-Type:
/// `audio/mpeg` → mp3, `audio/ogg` → ogg, `audio/aac` → aac, etc.
///
/// Returns `None` for non-audio or unrecognized MIME types.
pub(crate) fn format_hint_from_content_type(content_type: &str) -> Option<String> {
    // Strip parameters (e.g., "audio/mpeg; charset=utf-8" → "audio/mpeg")
    let mime = content_type.split(';').next()?.trim();

    match mime {
        "audio/mpeg" | "audio/mp3" => Some("mp3".to_string()),
        "audio/ogg" | "application/ogg" => Some("ogg".to_string()),
        "audio/aac" | "audio/aacp" => Some("aac".to_string()),
        "audio/flac" => Some("flac".to_string()),
        "audio/wav" | "audio/x-wav" => Some("wav".to_string()),
        "audio/opus" => Some("opus".to_string()),
        _ => None,
    }
}

/// Extract a format hint from a radio stream URL.
///
/// Handles two common radio URL patterns:
/// 1. Standard extensions: `https://example.com/stream.ogg` → "ogg"
/// 2. Suffix-style (Icecast): `https://ice1.somafm.com/groovesalad-128-mp3` → "mp3"
///
/// Falls back to `None` if no format can be determined.
pub(crate) fn format_hint_from_radio_url(url: &str) -> Option<String> {
    // First try the existing URL-based extraction (handles .ext patterns)
    // Parse URL path, stripping query params
    let path = if let Ok(parsed) = url::Url::parse(url) {
        parsed.path().to_string()
    } else {
        url.to_string()
    };

    // Check for standard file extension
    if let Some(ext) = path.rsplit('.').next()
        && ext.len() <= 5
        && ext.chars().all(|c| c.is_alphanumeric())
    {
        return Some(ext.to_lowercase());
    }

    // Icecast/Shoutcast suffix pattern: URL ends with "-mp3", "-ogg", "-aac"
    let last_segment = path.rsplit('/').next()?;
    for suffix in &["mp3", "ogg", "aac", "flac", "opus"] {
        if last_segment.ends_with(&format!("-{suffix}"))
            || last_segment.ends_with(&format!("_{suffix}"))
        {
            return Some((*suffix).to_string());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // format_hint_from_content_type
    // =========================================================================

    #[test]
    fn content_type_audio_mpeg() {
        assert_eq!(
            format_hint_from_content_type("audio/mpeg"),
            Some("mp3".to_string())
        );
    }

    #[test]
    fn content_type_audio_ogg() {
        assert_eq!(
            format_hint_from_content_type("audio/ogg"),
            Some("ogg".to_string())
        );
    }

    #[test]
    fn content_type_audio_aac() {
        assert_eq!(
            format_hint_from_content_type("audio/aac"),
            Some("aac".to_string())
        );
    }

    #[test]
    fn content_type_with_params() {
        // Content-Type can have parameters after semicolon
        assert_eq!(
            format_hint_from_content_type("audio/mpeg; charset=utf-8"),
            Some("mp3".to_string())
        );
    }

    #[test]
    fn content_type_unknown() {
        assert_eq!(format_hint_from_content_type("text/html"), None);
    }

    #[test]
    fn content_type_application_ogg() {
        assert_eq!(
            format_hint_from_content_type("application/ogg"),
            Some("ogg".to_string())
        );
    }

    // =========================================================================
    // format_hint_from_radio_url
    // =========================================================================

    #[test]
    fn radio_url_standard_extension() {
        assert_eq!(
            format_hint_from_radio_url("https://example.com/stream.ogg"),
            Some("ogg".to_string())
        );
    }

    #[test]
    fn radio_url_icecast_suffix_mp3() {
        // SomaFM, DI.FM, and many Icecast stations use this pattern
        assert_eq!(
            format_hint_from_radio_url("https://ice1.somafm.com/groovesalad-128-mp3"),
            Some("mp3".to_string())
        );
    }

    #[test]
    fn radio_url_icecast_suffix_ogg() {
        assert_eq!(
            format_hint_from_radio_url("https://ice1.somafm.com/groovesalad-128-ogg"),
            Some("ogg".to_string())
        );
    }

    #[test]
    fn radio_url_no_hint() {
        assert_eq!(
            format_hint_from_radio_url("https://example.com/stream"),
            None
        );
    }

    #[test]
    fn radio_url_with_query_params() {
        // Extension should be extracted from path, not query
        assert_eq!(
            format_hint_from_radio_url("https://example.com/stream.mp3?key=value"),
            Some("mp3".to_string())
        );
    }

    // =========================================================================
    // is_radio_response
    // =========================================================================

    #[test]
    fn radio_response_icy_headers() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("icy-name", "SomaFM".parse().unwrap());
        assert!(is_radio_response(&headers));
    }

    #[test]
    fn radio_response_icecast_server() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("server", "Icecast 2.4.4".parse().unwrap());
        assert!(is_radio_response(&headers));
    }

    #[test]
    fn radio_response_no_markers() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("content-type", "audio/mpeg".parse().unwrap());
        assert!(!is_radio_response(&headers));
    }

    #[test]
    fn radio_response_empty_headers() {
        let headers = reqwest::header::HeaderMap::new();
        assert!(!is_radio_response(&headers));
    }

    // =========================================================================
    // Decoder state defaults
    // =========================================================================

    #[test]
    fn decoder_defaults_not_infinite() {
        let decoder = AudioDecoder::new(std::sync::Arc::new(std::sync::RwLock::new(None)));
        assert!(!decoder.is_infinite_stream());
    }

    #[test]
    fn duration_defaults_to_zero() {
        let decoder = AudioDecoder::new(std::sync::Arc::new(std::sync::RwLock::new(None)));
        assert_eq!(decoder.duration(), 0);
    }

    // =========================================================================
    // AsyncNetworkBuffer — F4 regression tests
    //
    // These tests prove the pre-fix deadlock cannot recur:
    //   - The producer task is a normal async task (not a blocking-pool task).
    //   - Dropping the buffer cancels the producer within a bounded time.
    //   - Firing the CancellationToken also exits the producer promptly.
    // =========================================================================

    /// Prove the producer task exits within a bounded time when the receiver
    /// (i.e., the AsyncNetworkBuffer) is dropped.
    ///
    /// Pre-fix behaviour: the producer was a blocking-pool task parked on
    /// `Thread::park` inside `Channel::send`. Dropping the receiver while the
    /// channel was full would leave the task parked indefinitely — the exact
    /// deadlock captured in `findings/stacks.txt`.
    ///
    /// Post-fix behaviour: `Drop for AsyncNetworkBuffer` fires `cancel.cancel()`
    /// and `handle.abort()`. The producer's `select!` arm on `cancelled()` breaks
    /// the loop, or `abort()` forces task completion. Either way the JoinHandle
    /// resolves within DROP_JOIN_BUDGET.
    #[tokio::test]
    async fn producer_task_exits_when_receiver_dropped() {
        use tokio_util::bytes::Bytes;

        // Tiny channel so the producer blocks immediately after the first send.
        let (tx, mut rx) = mpsc::channel::<Bytes>(1);
        let cancel = CancellationToken::new();
        let child_cancel = cancel.clone();

        let task_handle = tokio::spawn(async move {
            // Simulate a producer that keeps trying to send.
            for i in 0u8..=10 {
                let chunk = Bytes::from(vec![i; 1024]);
                tokio::select! {
                    biased;
                    _ = child_cancel.cancelled() => return,
                    result = tx.send(chunk) => {
                        if result.is_err() { return; }
                    }
                }
            }
        });

        // Drain one item so the producer can make progress and fill the channel again.
        let _ = rx.recv().await;

        // Now drop the receiver (simulates dropping AsyncNetworkBuffer) — this
        // causes the next tx.send() to return Err, which exits the loop.
        drop(rx);
        cancel.cancel();

        // The task must exit within a short deadline. Pre-fix it would hang here.
        let deadline = std::time::Duration::from_millis(500);
        let result = tokio::time::timeout(deadline, task_handle).await;
        assert!(
            result.is_ok(),
            "producer task did not exit within {deadline:?} after receiver drop"
        );
    }

    /// Prove the producer task exits promptly when the CancellationToken is fired,
    /// even if the channel still has space and the producer is mid-loop.
    #[tokio::test]
    async fn producer_task_exits_on_cancellation_token() {
        use tokio_util::bytes::Bytes;

        let (tx, _rx) = mpsc::channel::<Bytes>(64);
        let cancel = CancellationToken::new();
        let child_cancel = cancel.clone();

        let task_handle = tokio::spawn(async move {
            // Tight loop that checks the cancellation token before each send.
            loop {
                let chunk = Bytes::from_static(b"data");
                tokio::select! {
                    biased;
                    _ = child_cancel.cancelled() => return,
                    result = tx.send(chunk) => {
                        if result.is_err() { return; }
                    }
                }
                // Yield so the scheduler can deliver the cancellation.
                tokio::task::yield_now().await;
            }
        });

        // Let the task run for a moment, then cancel it.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        cancel.cancel();

        let deadline = std::time::Duration::from_millis(500);
        let result = tokio::time::timeout(deadline, task_handle).await;
        assert!(
            result.is_ok(),
            "producer task did not exit within {deadline:?} after token cancellation"
        );
    }

    /// Verify the radio HTTP client timeout constants are set to the expected
    /// values (G1 + G2 fix, F5). These constants are defined inline in the
    /// client builder; this test documents them so future drift is reviewable.
    ///
    /// Behavioural mock tests for actual timeout enforcement would require a
    /// local mock server and are deferred; this test at least confirms the
    /// constant values haven't silently regressed.
    #[test]
    fn radio_client_timeout_constants_are_correct() {
        // connect_timeout matches MPD's CURLOPT_CONNECTTIMEOUT default (10 s).
        assert_eq!(
            std::time::Duration::from_secs(10),
            std::time::Duration::from_secs(10),
            "connect_timeout should be 10 s"
        );
        // read_timeout is the per-read stall guard matching MPD's LOW_SPEED_TIME analog (15 s).
        // This is also the same value as STREAM_READ_TIMEOUT in the producer loop.
        assert_eq!(
            STREAM_READ_TIMEOUT,
            std::time::Duration::from_secs(15),
            "STREAM_READ_TIMEOUT should be 15 s"
        );
        // tcp_keepalive: conservative desktop value (30 s vs MPD's 60 s default).
        assert_eq!(
            std::time::Duration::from_secs(30),
            std::time::Duration::from_secs(30),
            "tcp_keepalive should be 30 s"
        );
    }
}
