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
}

impl AudioDecoder {
    pub fn new() -> Self {
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
                .build()
                .context("Failed to create HTTP client")?;

            // Try HEAD request first
            let mut content_length = match client.head(&self.url).send().await {
                Ok(head_response) if head_response.status().is_success() => {
                    head_response.content_length().filter(|&len| len > 0)
                }
                _ => None,
            };

            // If HEAD didn't give us content-length, try a Range request with bytes=0-0
            // to get the Content-Range header which tells us total size
            if content_length.is_none() {
                trace!(" [DECODER] HEAD didn't return Content-Length, trying Range probe...");
                match client
                    .get(&self.url)
                    .header("Range", "bytes=0-0")
                    .send()
                    .await
                {
                    Ok(range_response) => {
                        // Check Content-Range header: "bytes 0-0/TOTAL_SIZE"
                        if let Some(content_range) = range_response.headers().get("content-range")
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
                            // For bytes=0-0 request, actual content is 1 byte, but we need full size
                            // This won't work, but try anyway
                            trace!(
                                " [DECODER] Range response Content-Length: {} (not useful)",
                                len
                            );
                        }
                    }
                    Err(e) => {
                        trace!(" [DECODER] Range probe failed: {}", e);
                    }
                }
            }

            // If we still don't have content-length, try a regular GET and read content-length header
            if content_length.is_none() {
                trace!(" [DECODER] Trying regular GET to detect Content-Length...");
                match client.get(&self.url).send().await {
                    Ok(response) if response.status().is_success() => {
                        content_length = response.content_length().filter(|&len| len > 0);
                        if let Some(len) = content_length {
                            trace!(" [DECODER] Got Content-Length from GET: {} bytes", len);
                        }
                        // Note: we're throwing away this response, which is wasteful
                        // but it's a fallback path
                    }
                    _ => {}
                }
            }

            let content_length =
                content_length.context("Could not determine Content-Length for HTTP stream")?;
            trace!(
                " [DECODER] Final Content-Length: {} bytes (took {:?})",
                content_length,
                init_start.elapsed()
            );

            // Extract format hint from URL
            let detected_format = self.extract_format_hint(&self.url);
            if let Some(ref format) = detected_format {
                trace!(" [DECODER] Detected format from URL: {}", format);
            }

            // Create Range-based reader - fetches only needed bytes on demand
            let reader = RangeHttpReader::new(self.url.clone(), content_length);

            let media_source: Box<dyn MediaSource> = Box::new(reader);
            let mss = MediaSourceStream::new(media_source, Default::default());

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
        let probed = match symphonia::default::get_probe().format(
            &hint,
            mss,
            &format_opts,
            &metadata_opts,
        ) {
            Ok(p) => {
                trace!(
                    " [DECODER] Format probe successful (took {:?})",
                    probe_start.elapsed()
                );
                p
            }
            Err(e) => {
                error!(" [DECODER] Format probe FAILED: {:?}", e);
                return Err(e).context("Failed to probe media format");
            }
        };

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
                    // Track list changed - not handled for now
                    warn!(" [DECODER] ResetRequired error - treating as EOF");
                    self.eof = true;
                    break;
                }
                Err(SymphoniaError::IoError(ref io_err)) => {
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
                    continue;
                }
                Err(SymphoniaError::DecodeError(ref e)) => {
                    // Log and skip packet on decode error
                    warn!(" [DECODER] Decode error (skipping): {:?}", e);
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

    /// Get the EMA-smoothed live compressed bitrate in kbps.
    /// Returns 0 if no packets have been decoded yet.
    pub fn live_bitrate(&self) -> u32 {
        self.smoothed_bitrate_kbps.round() as u32
    }

    /// Stop decoding
    pub fn stop(&mut self) {
        self.close_input();
    }
}

impl Default for AudioDecoder {
    fn default() -> Self {
        Self::new()
    }
}
