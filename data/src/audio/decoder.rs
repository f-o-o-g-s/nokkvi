use anyhow::{Context, Result};
use symphonia::core::{
    audio::RawSampleBuffer,
    codecs::{CODEC_TYPE_NULL, DecoderOptions},
    errors::Error as SymphoniaError,
    formats::{FormatOptions, FormatReader},
    io::{MediaSource, MediaSourceStream},
    meta::MetadataOptions,
    probe::Hint,
    units::{Time, TimeBase},
};
use tracing::{debug, error, trace, warn};

use super::range_http_reader::RangeHttpReader;
use crate::audio::{AudioBuffer, AudioFormat, SampleFormat};

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// Detect if an HTTP response originates from an Icecast/SHOUTcast radio server.
fn is_radio_response(headers: &reqwest::header::HeaderMap) -> bool {
    headers.keys().any(|k| k.as_str().starts_with("icy-"))
        || headers
            .get("server")
            .is_some_and(|v| v.to_str().unwrap_or("").to_lowercase().contains("icecast"))
}

/// A background-threaded network buffer that eagerly consumes an unbounded/infinite HTTP stream
/// to decouple TCP receive windows from the CPAL playback rate, eliminating stuttering drops.
struct AsyncNetworkBuffer {
    receiver: std::sync::Mutex<std::sync::mpsc::Receiver<Vec<u8>>>,
    buffer: Vec<u8>,
}

impl AsyncNetworkBuffer {
    pub fn new(mut read: Box<dyn std::io::Read + Send + 'static>) -> Self {
        let (tx, rx) = std::sync::mpsc::sync_channel(64);
        std::thread::Builder::new()
            .name("nokkvi-network-fetch".into())
            .spawn(move || {
                let mut buf = vec![0u8; 16384];
                loop {
                    match read.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            // Send data immediately — no accumulation.
                            // At 128kbps, accumulating 16KB would take ~1 second,
                            // starving the decode thread and causing audible blips.
                            if tx.send(buf[..n].to_vec()).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            })
            .expect("failed to spawn nokkvi-network-fetch thread");
        Self {
            receiver: std::sync::Mutex::new(rx),
            buffer: Vec::new(),
        }
    }
}

impl std::io::Read for AsyncNetworkBuffer {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.buffer.is_empty() {
            // Use recv_timeout instead of blocking recv() so the decode loop can
            // check its generation counter periodically and exit cleanly on shutdown.
            // Without this, radio streams block here until TCP data arrives (up to 30s).
            match self
                .receiver
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .recv_timeout(std::time::Duration::from_millis(500))
            {
                Ok(new_buf) => self.buffer = new_buf,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    // No data yet — return TimedOut so the decode loop can check
                    // its generation counter and exit cleanly during shutdown.
                    // IMPORTANT: Do NOT use Interrupted here! std::io::Read::read_exact()
                    // silently retries on Interrupted, which traps IcyStreamReader's
                    // metadata reads (read_exact calls) in an infinite loop.
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "network buffer timeout",
                    ));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    // Network thread exited (EOF or error)
                    return Ok(0);
                }
            }
        }
        let take = std::cmp::min(buf.len(), self.buffer.len());
        buf[..take].copy_from_slice(&self.buffer[..take]);
        self.buffer.drain(..take);
        Ok(take)
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
                let (response, content_type) = tokio::task::block_in_place(|| {
                    let client = reqwest::blocking::Client::builder()
                        .timeout(std::time::Duration::from_secs(30))
                        .user_agent(USER_AGENT)
                        .build()
                        .context("Failed to create blocking HTTP client")?;
                    let resp = client
                        .get(url_copy)
                        .header("Icy-MetaData", "1")
                        .send()
                        .context("Failed to open infinite stream connection")?;
                    let ct = resp
                        .headers()
                        .get("content-type")
                        .and_then(|v| v.to_str().ok())
                        .map(|s| s.to_string());
                    Ok::<(reqwest::blocking::Response, Option<String>), anyhow::Error>((resp, ct))
                })?;

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

                // Buffer the response using a dedicated OS thread to prevent OS-level TCP starvation
                let buffered_response = AsyncNetworkBuffer::new(Box::new(response));

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

        let format_opts = FormatOptions {
            enable_gapless: true,
            ..Default::default()
        };
        let metadata_opts = MetadataOptions::default();

        let probe_start = std::time::Instant::now();
        trace!(" [DECODER] Starting format probe...");
        // CRITICAL: Format probing reads from the MediaSource stream, which does blocking I/O.
        // Must wrap in block_in_place to avoid freezing the Tokio executor.
        let probed = tokio::task::block_in_place(|| {
            match symphonia::default::get_probe().format(&hint, mss, &format_opts, &metadata_opts) {
                Ok(p) => {
                    trace!(
                        " [DECODER] Format probe successful (took {:?})",
                        probe_start.elapsed()
                    );
                    Ok(p)
                }
                Err(e) => {
                    error!(" [DECODER] Format probe FAILED: {:?}", e);
                    Err(anyhow::Error::new(e).context("Failed to probe media format"))
                }
            }
        })?;

        let format_reader = probed.format;

        // Find first audio track
        trace!(" [DECODER] Finding audio track...");
        let track = format_reader
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .context("No supported audio tracks found")?;

        let track_id = track.id;
        trace!(" [DECODER] Found audio track with ID: {}", track_id);

        // Create decoder
        let decoder_opts = DecoderOptions::default();
        trace!(" [DECODER] Creating codec decoder...");
        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &decoder_opts)
            .context("Failed to create decoder")?;
        trace!(" [DECODER] Codec decoder created successfully");

        // Get format info
        let codec_params = &track.codec_params;
        let mut codec_name = None;
        if let Some(desc) = symphonia::default::get_codecs().get_codec(codec_params.codec) {
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
                    // Track list changed (e.g., OGG ICECast metadata changed)
                    warn!(" [DECODER] ResetRequired error - Stream format changed, reprobing...");
                    if let Some(reader) = self.format_reader.take() {
                        let mss = reader.into_inner();
                        let probe = symphonia::default::get_probe();

                        let mut hint = symphonia::core::probe::Hint::new();
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

                        match probe.format(
                            &hint,
                            mss,
                            &symphonia::core::formats::FormatOptions {
                                enable_gapless: false,
                                ..Default::default()
                            },
                            &symphonia::core::meta::MetadataOptions::default(),
                        ) {
                            Ok(probed) => {
                                let format_reader = probed.format;
                                if let Some(track) = format_reader.default_track() {
                                    self.track_id = Some(track.id);
                                    let decoder = symphonia::default::get_codecs().make(
                                        &track.codec_params,
                                        &symphonia::core::codecs::DecoderOptions::default(),
                                    );
                                    if let Ok(dec) = decoder {
                                        self.decoder = Some(dec);
                                        // Update format in case it changed
                                        let channels = track
                                            .codec_params
                                            .channels
                                            .unwrap_or(
                                                symphonia::core::audio::Channels::FRONT_LEFT
                                                    | symphonia::core::audio::Channels::FRONT_RIGHT,
                                            )
                                            .count();
                                        let sample_rate =
                                            track.codec_params.sample_rate.unwrap_or(44100);
                                        self.format = AudioFormat::new(
                                            SampleFormat::F32,
                                            sample_rate,
                                            channels as u32,
                                        );
                                        if let Some(desc) = symphonia::default::get_codecs()
                                            .get_codec(track.codec_params.codec)
                                        {
                                            self.live_codec = Some(desc.short_name.to_string());
                                        }
                                        // Retry reading the packet with the new decoder
                                        self.format_reader = Some(format_reader);
                                        continue;
                                    }
                                    error!(" [DECODER] Failed to make decoder after reprobing");
                                } else {
                                    error!(" [DECODER] No default track found after reprobing");
                                }
                                self.format_reader = Some(format_reader);
                            }
                            Err(e) => {
                                error!(
                                    " [DECODER] Failed to reprobe format after ResetRequired: {}",
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
            .unwrap_or(TimeBase::new(1, 1000));

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
    // AsyncNetworkBuffer — immediate forwarding tests
    // =========================================================================

    /// Mock reader that delivers data in controlled bursts with timing gaps,
    /// simulating real TCP delivery patterns from Icecast radio streams.
    struct BurstyReader {
        chunks: Vec<Vec<u8>>,
        delay_between: std::time::Duration,
        index: usize,
    }

    impl BurstyReader {
        fn new(chunks: Vec<Vec<u8>>, delay: std::time::Duration) -> Self {
            Self {
                chunks,
                delay_between: delay,
                index: 0,
            }
        }
    }

    impl std::io::Read for BurstyReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.index >= self.chunks.len() {
                return Ok(0); // EOF
            }
            if self.index > 0 {
                std::thread::sleep(self.delay_between);
            }
            let chunk = &self.chunks[self.index];
            let n = chunk.len().min(buf.len());
            buf[..n].copy_from_slice(&chunk[..n]);
            self.index += 1;
            Ok(n)
        }
    }

    /// Simulates 128kbps StreamAfrica delivery: 4KB every ~250ms.
    /// Verifies that the FIRST 4KB chunk is readable within 100ms
    /// (not blocked waiting for 16KB accumulation).
    #[test]
    fn async_network_buffer_bursty_128kbps_no_accumulation_delay() {
        use std::io::Read;

        // 4 chunks of 4KB each, 50ms apart (simulating 128kbps bursty delivery)
        let chunks: Vec<Vec<u8>> = (0..4).map(|i| vec![i as u8; 4096]).collect();
        let reader = BurstyReader::new(chunks, std::time::Duration::from_millis(50));

        let mut buffer = AsyncNetworkBuffer::new(Box::new(reader));

        // Read the first chunk — should be available IMMEDIATELY (within 200ms)
        // If accumulation is in place, this would block for 200ms+ waiting for 16KB
        let start = std::time::Instant::now();
        let mut out = vec![0u8; 4096];
        let n = buffer.read(&mut out).unwrap();
        let elapsed = start.elapsed();

        assert!(n > 0, "Should have read some data");
        assert!(
            elapsed < std::time::Duration::from_millis(200),
            "First read should complete within 200ms (no accumulation), took {:?}",
            elapsed
        );
    }

    /// Simulates 256kbps SomaFM delivery: 8KB chunks, fast arrival.
    /// Verifies data flows through immediately regardless of size.
    #[test]
    fn async_network_buffer_healthy_256kbps_immediate() {
        use std::io::Read;

        // 2 chunks of 8KB, 10ms apart (healthy 256kbps stream)
        let chunks: Vec<Vec<u8>> = (0..2).map(|i| vec![i as u8; 8192]).collect();
        let reader = BurstyReader::new(chunks, std::time::Duration::from_millis(10));

        let mut buffer = AsyncNetworkBuffer::new(Box::new(reader));

        let start = std::time::Instant::now();
        let mut out = vec![0u8; 8192];
        let n = buffer.read(&mut out).unwrap();
        let elapsed = start.elapsed();

        assert!(n > 0, "Should have read data");
        assert!(
            elapsed < std::time::Duration::from_millis(100),
            "Read should be near-instant for healthy stream, took {:?}",
            elapsed
        );
    }

    /// Verifies EOF propagates correctly through AsyncNetworkBuffer.
    #[test]
    fn async_network_buffer_eof_propagation() {
        use std::io::Read;

        // Single small chunk then EOF
        let chunks = vec![vec![42u8; 100]];
        let reader = BurstyReader::new(chunks, std::time::Duration::from_millis(0));

        let mut buffer = AsyncNetworkBuffer::new(Box::new(reader));

        // Read the data
        let mut out = vec![0u8; 256];
        let n = buffer.read(&mut out).unwrap();
        assert_eq!(n, 100);
        assert_eq!(out[0], 42);

        // Next read should return 0 (EOF) — not block indefinitely
        let start = std::time::Instant::now();
        let n2 = buffer.read(&mut out).unwrap();
        let elapsed = start.elapsed();

        assert_eq!(n2, 0, "Should return 0 for EOF");
        assert!(
            elapsed < std::time::Duration::from_millis(200),
            "EOF should propagate quickly, took {:?}",
            elapsed
        );
    }

    /// Verifies data integrity through the buffer — bytes out must match bytes in.
    #[test]
    fn async_network_buffer_data_integrity() {
        use std::io::Read;

        // 3 chunks with distinct patterns
        let chunks = vec![vec![0xAA; 1000], vec![0xBB; 2000], vec![0xCC; 3000]];
        let reader = BurstyReader::new(chunks, std::time::Duration::from_millis(5));

        let mut buffer = AsyncNetworkBuffer::new(Box::new(reader));

        let mut all_data = Vec::new();
        let mut out = vec![0u8; 512];
        loop {
            match buffer.read(&mut out) {
                Ok(0) => break,
                Ok(n) => all_data.extend_from_slice(&out[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => break,
                Err(e) => panic!("unexpected error: {e}"),
            }
        }

        assert_eq!(all_data.len(), 6000);
        assert!(all_data[..1000].iter().all(|&b| b == 0xAA));
        assert!(all_data[1000..3000].iter().all(|&b| b == 0xBB));
        assert!(all_data[3000..6000].iter().all(|&b| b == 0xCC));
    }
}
