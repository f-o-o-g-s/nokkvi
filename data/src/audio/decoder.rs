use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use anyhow::{Context, Result};
use symphonia::core::{
    audio::Channels,
    codecs::audio::AudioDecoder as SymphoniaDecoder,
    errors::Error as SymphoniaError,
    formats::{
        FormatId, FormatReader, SeekMode, SeekTo, SeekedTo, Track,
        probe::Hint,
        well_known::{FORMAT_ID_MP1, FORMAT_ID_MP2, FORMAT_ID_MP3},
    },
    io::{MediaSource, MediaSourceStream, ReadOnlySource},
    units::{Time, TimeBase},
};
use tokio::sync::mpsc;
use tokio_util::{
    bytes::{Buf, Bytes},
    sync::CancellationToken,
};
use tracing::{debug, error, info, trace, warn};

use super::range_http_reader::{CHUNK_SIZE, ContentRange, RangeHttpReader, RangeUrl};
use crate::{
    audio::{AudioBuffer, AudioFormat, SampleFormat, symphonia_registry},
    utils::{
        artwork_url::{requests_transcode, with_time_offset},
        url_redaction::redact_subsonic_url,
    },
};

/// The answer to a stream request whose `timeOffset` lies past the song's
/// end: Navidrome sends an Ogg holding only its headers, or an empty body.
/// Nothing went wrong; the song is over. [`AudioDecoder::open_at`] reports it
/// as this error so the engine can end the track as at its natural end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamPastEnd;

impl std::fmt::Display for StreamPastEnd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the song ends before that point")
    }
}

impl std::error::Error for StreamPastEnd {}

/// The server refused a transcode because it is busy: HTTP 429, its limit of
/// concurrent transcodes. Worth trying again in a moment. Carries the
/// server's own message when its answer had one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerBusy {
    pub message: Option<String>,
}

impl std::fmt::Display for ServerBusy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "the server is busy with other transcodes (HTTP 429 Too Many Requests); try again \
             shortly",
        )?;
        match &self.message {
            Some(message) => write!(f, ": {message}"),
            None => Ok(()),
        }
    }
}

impl std::error::Error for ServerBusy {}

/// No answer from the server: the request could not reach it, or its
/// answer did not come within the open's time ([`AudioDecoder::set_answer_timeout`]).
/// Holds the cause, without the URL (it carries the credential).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoAnswer(pub String);

impl std::fmt::Display for NoAnswer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the server didn't answer: {}", self.0)
    }
}

impl std::error::Error for NoAnswer {}

/// Whether `error`, or anything in its chain, is a `T`.
pub fn error_is<T: std::error::Error + 'static>(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| cause.is::<T>())
}

/// Detect if an HTTP response originates from an Icecast/SHOUTcast radio server.
fn is_radio_response(headers: &reqwest::header::HeaderMap) -> bool {
    headers.keys().any(|k| k.as_str().starts_with("icy-"))
        || headers
            .get("server")
            .is_some_and(|v| v.to_str().unwrap_or("").to_lowercase().contains("icecast"))
}

/// Whether an answer says the server won't serve byte ranges
/// (`Accept-Ranges: none`), as Navidrome's live transcode does.
fn refuses_ranges(headers: &reqwest::header::HeaderMap) -> bool {
    headers
        .get(reqwest::header::ACCEPT_RANGES)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("none"))
}

/// The answer's `Content-Length` header, when it names a length. Read from
/// the header itself: reqwest's `content_length()` is the body's size, which
/// a HEAD answer never has.
fn header_content_length(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    declared_content_length(headers).filter(|&len| len > 0)
}

/// The `Content-Length` header as declared, 0 included.
fn declared_content_length(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get(reqwest::header::CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// Whether an answer says it serves byte ranges (`Accept-Ranges: bytes`).
fn serves_ranges(headers: &reqwest::header::HeaderMap) -> bool {
    headers
        .get(reqwest::header::ACCEPT_RANGES)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("bytes"))
}

/// Whether an answer's body is a JSON or XML document (a Subsonic error
/// envelope), which no audio stream, ICY radio included, is served as.
fn is_document_answer(headers: &reqwest::header::HeaderMap) -> bool {
    headers
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| {
            let lowered = ct.to_ascii_lowercase();
            lowered.starts_with("application/json")
                || lowered.starts_with("text/xml")
                || lowered.starts_with("application/xml")
        })
}

/// The stream's length in ms from an `X-Content-Duration` header (seconds,
/// as a float), or `None` when absent or unusable.
fn x_content_duration_ms(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    let secs: f64 = headers
        .get("x-content-duration")?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()?;
    (secs.is_finite() && secs > 0.0).then(|| (secs * 1000.0).round() as u64)
}

/// What the one GET of a requested transcode answered, judged from its
/// status and headers alone (see `AudioDecoder::open_requested_transcode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TranscodeAnswer {
    /// Not a 2xx (a 3xx redirect the client wouldn't follow, a 429, a 4xx or
    /// 5xx), or a 206 that names no length: an error.
    Refused(reqwest::StatusCode),
    /// A 2xx whose body is JSON or XML: a Subsonic error envelope, not audio.
    ErrorEnvelope,
    /// Nothing to play: a 2xx with `Content-Length: 0`, or a 416 (a range
    /// asked of an empty entry).
    Empty,
    /// A 206 for another range than the one asked for (it starts elsewhere,
    /// or the server capped it): the range reader could never fill its
    /// chunks from this server.
    WrongRange,
    /// A 206 whose `Content-Range` names no total length: nothing to size the
    /// stream by, and its body is only the first slice.
    PartialWithoutLength,
    /// The answer serves byte ranges, `total` bytes long: read it with Range
    /// requests like a file.
    Sized { total: u64 },
    /// No usable length (a live transcode): read once, front to back.
    Stream,
}

/// Judge a requested transcode's answer to its `Range: bytes=0-<chunk - 1>`
/// GET. Only a 206 for exactly that range is sized: it names the whole
/// length in its `Content-Range` (its own `Content-Length` is the slice).
/// Navidrome answers every range it can serve with a 206, so a 200 means a
/// live transcode or something on the way that stripped the Range header;
/// later Range requests would be stripped too, so a 200 is read once
/// through, except when it is the whole file in one chunk with
/// `Accept-Ranges: bytes` (then it is all in hand). Go sizes a tiny live
/// answer, such as the headers-only Ogg past the end of a song, next to
/// `Accept-Ranges: none`, which stays a stream.
fn classify_transcode_answer(
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
) -> TranscodeAnswer {
    if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        return TranscodeAnswer::Empty;
    }
    if !status.is_success() {
        return TranscodeAnswer::Refused(status);
    }
    if is_document_answer(headers) {
        return TranscodeAnswer::ErrorEnvelope;
    }
    if declared_content_length(headers) == Some(0) {
        return TranscodeAnswer::Empty;
    }
    if status == reqwest::StatusCode::PARTIAL_CONTENT {
        let range = ContentRange::of(headers);
        return match range.and_then(|range| range.total) {
            None => TranscodeAnswer::PartialWithoutLength,
            Some(0) => TranscodeAnswer::Empty,
            Some(total)
                if range.is_some_and(|range| {
                    range.start == Some(0) && range.end == Some(CHUNK_SIZE.min(total) - 1)
                }) =>
            {
                TranscodeAnswer::Sized { total }
            }
            Some(_) => TranscodeAnswer::WrongRange,
        };
    }
    match header_content_length(headers) {
        Some(total) if serves_ranges(headers) && total <= CHUNK_SIZE => {
            TranscodeAnswer::Sized { total }
        }
        Some(_) | None => TranscodeAnswer::Stream,
    }
}

/// The error an empty answer to a requested transcode reports: ffmpeg on the
/// server wrote nothing (Navidrome still answers 200).
const EMPTY_TRANSCODE: &str = "the server couldn't transcode this song: it sent no audio";

/// The error for a requested transcode the server answered with `status`
/// (not a 2xx), with the server's own message when its body had one. Never
/// names the URL (it carries the credential).
fn refused_transcode_error(status: reqwest::StatusCode, message: Option<String>) -> anyhow::Error {
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return ServerBusy { message }.into();
    }
    let what = if status.is_redirection() {
        format!("the server sent the stream elsewhere (HTTP {status}), which nokkvi won't follow")
    } else {
        format!("the server refused the stream (HTTP {status})")
    };
    match message {
        Some(message) => anyhow::anyhow!("{what}: {message}"),
        None => anyhow::anyhow!(what),
    }
}

/// The error for an answer that did not come within `timeout`.
fn no_answer_within(timeout: std::time::Duration) -> anyhow::Error {
    NoAnswer(format!("no answer within {} ms", timeout.as_millis())).into()
}

/// Up to `limit` bytes from the front of `response`'s body, fewer when the
/// body ends first or a read fails (the caller only uses a whole prefix).
async fn read_prefix(mut response: reqwest::Response, limit: u64) -> Vec<u8> {
    let limit = usize::try_from(limit).unwrap_or(usize::MAX);
    let mut bytes = Vec::new();
    while bytes.len() < limit {
        match response.chunk().await {
            Ok(Some(chunk)) => bytes.extend_from_slice(&chunk),
            Ok(None) => break,
            Err(e) => {
                debug!(
                    " [DECODER] Reading the answer's first bytes stopped: {}",
                    e.without_url()
                );
                break;
            }
        }
    }
    bytes.truncate(limit);
    bytes
}

/// The server's message from a Subsonic error envelope in `response`'s body
/// (read up to 64 KB), when the body is one.
async fn envelope_message(response: reqwest::Response) -> Option<String> {
    if !is_document_answer(response.headers()) {
        return None;
    }
    let body = read_prefix(response, 64 * 1024).await;
    crate::services::api::subsonic::failed_envelope_error(&String::from_utf8_lossy(&body))
        .map(|error| error.message)
}

/// What a stream open hands the probe: the source, a format hint, the
/// receive timeout of a once-through stream's buffer (dropped to
/// `READ_RECV_TIMEOUT` after the probe), and the server's length for the
/// stream (`X-Content-Duration`).
struct OpenedStream {
    mss: MediaSourceStream<'static>,
    hint: Option<String>,
    recv_timeout: Option<RecvTimeout>,
    header_duration_ms: Option<u64>,
    /// The whole answer is in hand and small enough to be a container's
    /// headers alone ([`PAST_END_ANSWER_MAX_BYTES`]), judged by the bytes
    /// read, never by a `Content-Length` (a proxy may send it chunked).
    tiny: bool,
}

/// The largest answer to a request at an offset that is taken for a
/// container's headers alone when the probe finds no audio in it (Navidrome
/// sends a 266-byte Ogg past a song's end; one second of 64 kbps Opus is
/// already 8 KB).
const PAST_END_ANSWER_MAX_BYTES: u64 = 4096;

/// Bounded backoff (ms) between transient-I/O-error retries inside
/// `read_buffer`. The retry sleep is a synchronous `std::thread::sleep` run
/// under `block_in_place`, so it parks a tokio worker thread; capping it keeps
/// that parking brief and lets the engine decode loop's generation check run
/// frequently enough to abandon a decoder whose source was already skipped.
///
/// Grows linearly (100 ms per consecutive error) and clamps at 500 ms — the
/// same per-attempt ceiling as the radio path's `READ_RECV_TIMEOUT`, while
/// `MAX_IO_RETRIES` still bounds the total number of attempts before EOF.
fn io_retry_backoff_ms(consecutive_io_errors: u32) -> u64 {
    (100 * consecutive_io_errors as u64).min(500)
}

/// Per-chunk network read timeout. Large enough to outlast normal Icecast jitter
/// (~25 ms for 128 kbps MP3), small enough to detect a stalled-socket within a
/// reasonable user-facing window.
const STREAM_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Timeout used on the consumer side of the channel. Kept at 500 ms so the
/// decode loop's generation-counter check fires promptly and the loop can exit
/// cleanly during shutdown without waiting for TCP data.
const READ_RECV_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);

/// Consumer-side wait while the format probe reads a radio stream's FIRST
/// bytes. Icecast starts a new listener at the next Ogg page, so the first
/// audio can lag the response headers by a page's worth of time (Radio SEGA
/// measured 260–500 ms), and the probe reads a `TimedOut` as "no data" and
/// fails. The probe has no generation check to keep responsive, so it gets
/// the same 10 s budget as connecting (the radio client's `connect_timeout`);
/// `open_input` drops back to [`READ_RECV_TIMEOUT`] once the probe returns.
const PROBE_RECV_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// How long [`AsyncNetworkBuffer`]'s `Read` waits for the next chunk. Shared
/// between `open_input` and the buffer it builds, because the buffer is
/// boxed inside the format reader by the time the probe returns and the wait
/// must shrink from [`PROBE_RECV_TIMEOUT`] to [`READ_RECV_TIMEOUT`].
#[derive(Clone)]
struct RecvTimeout(Arc<AtomicU64>);

impl RecvTimeout {
    fn new(timeout: std::time::Duration) -> Self {
        Self(Arc::new(AtomicU64::new(Self::to_millis(timeout))))
    }

    fn set(&self, timeout: std::time::Duration) {
        self.0.store(Self::to_millis(timeout), Ordering::Relaxed);
    }

    fn get(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.0.load(Ordering::Relaxed))
    }

    fn to_millis(timeout: std::time::Duration) -> u64 {
        u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX)
    }
}

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
    recv_timeout: RecvTimeout,
}

impl AsyncNetworkBuffer {
    /// Buffer `response`'s body. `first` holds bytes already read from it,
    /// which `Read` serves before anything the task receives.
    pub(crate) fn new_async(
        response: reqwest::Response,
        first: Bytes,
        recv_timeout: RecvTimeout,
    ) -> Self {
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
                        // `bytes_stream()` yields `Decode`-kind errors, which reqwest
                        // builds with no URL attached, so `without_url()` is a no-op
                        // here today — kept as defense-in-depth so this stays correct
                        // if a future reqwest attaches the URL to body/stream errors.
                        warn!(" [NETWORK BUFFER] Stream error: {}", e.without_url());
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
            leftover: first,
            cancel,
            task: Some(task),
            recv_timeout,
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
                    // Slow path: wait up to the current receive timeout for the next
                    // chunk (PROBE_RECV_TIMEOUT while the probe reads the first bytes,
                    // READ_RECV_TIMEOUT once decoding runs).
                    // SAFETY: this must be called from within block_in_place (the decode
                    // loop at engine.rs and the probe in open_input guarantee this).
                    // Handle::block_on is legal there.
                    let handle = tokio::runtime::Handle::current();
                    let wait = self.recv_timeout.get();
                    match handle.block_on(tokio::time::timeout(wait, self.rx.recv())) {
                        Ok(Some(chunk)) => self.leftover = chunk,
                        // Receiver got a chunk but sender was dropped simultaneously — treat as EOF.
                        Ok(None) => return Ok(0),
                        // The wait elapsed without data — return TimedOut so the decode loop can
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
        // Cooperative shutdown via the select! arm in the producer loop.
        self.cancel.cancel();
        if let Some(handle) = self.task.take() {
            // Hard-abort schedules cancellation at the producer's next yield point;
            // dropping the JoinHandle detaches it so the runtime owns the cleanup.
            //
            // Deliberately NOT awaiting the handle here: Drop can fire from a tokio
            // worker thread (e.g., the radio→library decoder swap on track change),
            // and `Handle::block_on` from inside a runtime panics with "Cannot start
            // a runtime from within a runtime". The original F4 shutdown-hang was
            // caused by the producer being a blocking-pool task (`block_in_place`
            // path); since that fix the producer is a normal `tokio::spawn`'d task,
            // so `BlockingPool::shutdown` no longer waits on it and the synchronous
            // join here is not load-bearing for clean shutdown.
            handle.abort();
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

/// What a caller is loading into a decoder. It decides how `open_input`
/// reads a response that carries no length: a queue song is still a finite
/// track, a station is a live stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SourceKind {
    /// A queue song. Finite even when the server sends it without a length
    /// (a live transcode); never reconnected.
    #[default]
    Track,
    /// An internet radio station: a live stream that is reconnected when it
    /// drops. Only `CustomAudioEngine::load_radio_stream` loads one.
    Station,
}

/// How the open source delivers its bytes, decided by `open_input`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StreamDelivery {
    /// A source of known length: HTTP Range reads, or a local file. Finite
    /// and seekable.
    #[default]
    Sized,
    /// A queue song the server sends without a length, read once from front
    /// to back. Navidrome serves a live (not yet cached) transcode this way:
    /// `Accept-Ranges: none`, no Content-Length, `X-Content-Duration`.
    /// Finite (its EOF ends the track). It can't seek within itself; a
    /// transcode nokkvi asked for seeks by being opened again at an offset
    /// (`CustomAudioEngine::seekable`).
    Unsized,
    /// A radio station's live stream: infinite, reconnected when it drops.
    Live,
}

/// What a track's open stream is: how its bytes arrive, the answer's byte
/// length, the codec that decodes it and where in the song it starts. Taken
/// from a decoder in one piece ([`Self::of`]) and stored in one piece wherever
/// the engine keeps a track (the now-playing track, a skip blend's incoming,
/// `GaplessTransitionInfo`), so no site can record one fact and forget
/// another.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamFacts {
    /// How the bytes arrive, and so whether the track can seek.
    pub delivery: StreamDelivery,
    /// The answer's byte length when `Sized` ([`AudioDecoder::byte_len`]).
    pub byte_len: Option<u64>,
    /// The codec Symphonia decodes, by its short name (`flac`, `opus`,
    /// `pcm_s16le`, ...).
    pub codec: Option<String>,
    /// Where in the song the stream starts, in whole seconds: the
    /// `timeOffset` the server honoured ([`AudioDecoder::offset_secs`]), 0
    /// for a stream from the start. The track's URL never carries it.
    pub offset_secs: u64,
}

impl StreamFacts {
    /// The facts of `decoder`'s open stream.
    pub fn of(decoder: &AudioDecoder) -> Self {
        Self {
            delivery: decoder.delivery(),
            byte_len: decoder.byte_len(),
            codec: decoder.live_codec(),
            offset_secs: decoder.offset_secs(),
        }
    }
}

/// What it takes to open a decoder's track again elsewhere in the song,
/// read off the playing decoder ([`AudioDecoder::reopen_hints`]) so the open
/// itself needs no lock on it: a seek's reopen runs while the old stream
/// plays on, a cut stream's resume while its ring drains. The new decoder
/// keeps the track's own URL (so its `format` / `maxBitRate`), its kind and
/// what the caller said of the song.
#[derive(Clone)]
pub struct ReopenHints {
    url: String,
    source_kind: SourceKind,
    expected_duration_ms: Option<u64>,
    expected_file_size: Option<u64>,
    live_icy_metadata: std::sync::Arc<std::sync::RwLock<Option<String>>>,
}

impl ReopenHints {
    /// The track's own URL (no `timeOffset`).
    pub fn url(&self) -> &str {
        &self.url
    }

    /// A new decoder on the track, opened at `position_ms` of the song
    /// ([`AudioDecoder::open_at`] at its whole second) and brought to it
    /// exactly ([`AudioDecoder::land_on`]). `answer_timeout` bounds the wait
    /// for the server's answer ([`AudioDecoder::set_answer_timeout`]); the
    /// decoder it returns waits as long as any other afterwards.
    pub async fn open_at(
        &self,
        position_ms: u64,
        answer_timeout: Option<std::time::Duration>,
    ) -> Result<AudioDecoder> {
        let mut decoder = AudioDecoder::new(self.live_icy_metadata.clone());
        decoder.set_source_kind(self.source_kind);
        decoder.set_expected_duration_ms(self.expected_duration_ms);
        decoder.set_expected_file_size(self.expected_file_size);
        decoder.set_answer_timeout(answer_timeout);
        let opened = decoder.open_at(&self.url, position_ms / 1000).await;
        decoder.set_answer_timeout(None);
        opened?;
        if !tokio::task::block_in_place(|| decoder.land_on(position_ms)) {
            anyhow::bail!("the stream could not move to {position_ms} ms");
        }
        Ok(decoder)
    }
}

/// Audio decoder using symphonia
pub struct AudioDecoder {
    format_reader: Option<Box<dyn FormatReader>>,
    decoder: Option<Box<dyn SymphoniaDecoder>>,
    track_id: Option<u32>,
    format: AudioFormat,
    duration: u64, // milliseconds
    url: String,
    initialized: bool,
    eof: bool,
    // Buffer for leftover samples from partially-processed frames
    frame_buffer: Vec<u8>,
    /// EMA-smoothed compressed bitrate in kbps, computed per-packet from
    /// Symphonia's `Packet.data.len()` and `Packet::block_dur()`.
    smoothed_bitrate_kbps: f64,
    /// What the caller is loading (see [`SourceKind`]); set before `init`.
    source_kind: SourceKind,
    /// How the open source delivers its bytes (see [`StreamDelivery`]).
    /// `Live` is what the engine treats as radio: no backpressure, the jitter
    /// prebuffer, and the reconnect at EOF.
    delivery: StreamDelivery,
    /// The answer's byte length, recorded with `delivery`: the length a
    /// `Sized` source is read in (the HTTP length the range reader serves, or
    /// the file's size); `None` for an `Unsized` or `Live` stream. Navidrome
    /// serves a song's own file at exactly its scanned size, so the UI compares
    /// the two to tell the file from a transcode its cache serves sized.
    byte_len: Option<u64>,
    /// Server-reported track duration in ms, used to sanity-check the probed
    /// duration in `open_input`. `None` for radio and engine-internal loads.
    expected_duration_ms: Option<u64>,
    /// The song's file size on disk (`Song::expected_file_size`), stamped
    /// beside `expected_duration_ms`. A sized answer to a request at an
    /// offset whose length is exactly this is the song's own file (which
    /// ignores the offset); any other length is a transcode the server's
    /// cache holds for that offset. `None` when unknown: the probed length
    /// decides then ([`sized_answer_is_the_file`]).
    expected_file_size: Option<u64>,
    /// How long a requested transcode's open waits for the answer's headers
    /// and its first bytes ([`Self::set_answer_timeout`]); `None` waits as
    /// long as the stream client allows. A seek's reopen and a cut stream's
    /// resume set a shorter one: they run while the old stream plays, or
    /// while the ring drains.
    answer_timeout: Option<std::time::Duration>,
    /// Maps the real timeline onto Symphonia's internal one for `seek()`.
    /// 1.0 except when `sanitize_probed_duration` overrode the probe: the
    /// format reader still believes the probed total, so coarse seeks must be
    /// scaled by probed/real or they land at the wrong byte offset.
    seek_scale: f64,
    /// Arc passed in by the AudioEngine, populated by `IcyMetadataReader` if this stream supports ICY.
    live_icy_metadata: std::sync::Arc<std::sync::RwLock<Option<String>>>,
    /// The short-name of the actual hardware codec (e.g., "mp3", "aac", "vorbis").
    live_codec: Option<String>,
    /// M8 "Skip Silence Between Tracks": producer-side leading-silence scan
    /// state. Enabled (via [`Self::set_trim_leading_silence`]) only on
    /// prepared TRANSITION decoders — user-initiated loads keep the honest
    /// recorded start. See [`LeadingTrim`].
    leading_trim: LeadingTrim,
    /// Decoded frames still to drop before the last seek's target. A seek
    /// lands on the packet that holds the target (an MP3 one up to three
    /// bit-reservoir frames earlier), so its first frames come before the
    /// position the clock names; `read_buffer` drops them. See
    /// [`seek_preroll_frames`].
    seek_preroll_frames: u64,
    /// Where in the song the open stream starts, in whole seconds: the
    /// `timeOffset` the server honoured ([`Self::open_at`]). 0 for a stream
    /// from the start, and for an answer that ignored the offset (the song's
    /// own file). `url` never carries it.
    offset_secs: u64,
    /// The URL handle of the range reader a requested transcode's sized
    /// answer is read through, so an answer that turns out to be the song's
    /// own file is read on without the offset ([`RangeUrl`]).
    range_url: Option<RangeUrl>,
}

/// Leading-silence trim state (M8). The scan lives inside the registry-backed
/// decode path (`read_buffer`'s decode-ok arm) and is DISTINCT from the
/// decoder's own gapless trim: Symphonia drops the container-declared encoder
/// delay and padding inside `decode()`, this drops *musical* leading silence
/// below [`super::SOURCE_SILENCE_THRESHOLD`], frame-aligned, until the first
/// loud frame or the [`LEADING_TRIM_MAX_MS`] budget runs out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeadingTrim {
    /// Feature off for this decoder (the default).
    Off,
    /// Enabled, but the frame budget isn't derived yet (format unknown until
    /// the first decoded packet reports its spec).
    Pending,
    /// Scanning the leading frames; `frames_left` is the remaining budget.
    Scanning { frames_left: u64 },
    /// A loud frame was seen (or the budget ran out) — pass-through from here,
    /// including after any seek.
    Done,
}

/// Budget cap for the leading-silence scan: at most this much leading audio is
/// ever dropped (bounds the skip on pathological pregap/hidden-track silence,
/// and guarantees an all-silent track still plays rather than fast-forwarding
/// to EOF).
const LEADING_TRIM_MAX_MS: u64 = 15_000;

/// How many bytes to drop from the FRONT of `decoded_bytes` for the
/// leading-silence trim (M8). Advances `state`:
/// - `Off` / `Done` / `Pending` → 0 (Pending is resolved by the caller, which
///   knows the frame rate).
/// - `Scanning`: walks whole frames; the first frame with any sample at or
///   above [`super::SOURCE_SILENCE_THRESHOLD`] flips to `Done` and everything
///   from that frame on survives. All-silent buffers are dropped whole until
///   the frame budget runs out, which also flips to `Done` (the remaining
///   silence plays honestly). Only whole frames are ever dropped — a
///   mid-frame cut would rotate the channel interleave.
fn leading_trim_prefix_bytes(
    state: &mut LeadingTrim,
    decoded_bytes: &[u8],
    bytes_per_frame: usize,
) -> usize {
    let LeadingTrim::Scanning { frames_left } = *state else {
        return 0;
    };
    if bytes_per_frame == 0 {
        return 0;
    }

    let mut dropped_frames: u64 = 0;
    for frame in decoded_bytes.chunks_exact(bytes_per_frame) {
        let loud = frame.as_chunks::<4>().0.iter().any(|s| {
            let sample = f32::from_le_bytes(*s);
            sample.abs() >= super::SOURCE_SILENCE_THRESHOLD
        });
        if loud {
            *state = LeadingTrim::Done;
            return (dropped_frames as usize) * bytes_per_frame;
        }
        dropped_frames += 1;
        if dropped_frames >= frames_left {
            // Budget exhausted: stop scanning; the remaining silence plays.
            *state = LeadingTrim::Done;
            return (dropped_frames as usize) * bytes_per_frame;
        }
    }

    *state = LeadingTrim::Scanning {
        frames_left: frames_left - dropped_frames,
    };
    (dropped_frames as usize) * bytes_per_frame
}

/// The seek mode for a container. MPEG audio (MP1/MP2/MP3) carries no
/// timestamps: its coarse seek places the target by spreading the byte length
/// evenly over the duration and reports that estimate as where it landed, so
/// a VBR file plays seconds away from the position the clock shows. Its
/// accurate seek counts frame headers up to the target instead (from the read
/// position, or from the first frame for a seek back): it reads those bytes
/// but decodes none. Every other container lands coarsely on a real
/// timestamp (a FLAC frame number, an Ogg granule position, an MP4 sample
/// table, a WAV byte offset) and stays coarse, because an accurate seek there
/// can decode from the start (FLAC without a SEEKTABLE).
fn seek_mode_for(format: FormatId) -> SeekMode {
    if [FORMAT_ID_MP1, FORMAT_ID_MP2, FORMAT_ID_MP3].contains(&format) {
        SeekMode::Accurate
    } else {
        SeekMode::Coarse
    }
}

/// Decoded frames that come before a seek's target: the reader lands on the
/// packet holding the target (`actual_ts <= required_ts`), so playback would
/// start that far early while the clock names the target. Timestamps below
/// zero are encoder delay, which the gapless decoder drops on its own, so
/// they never count twice. 0 when the reader landed on or past the target,
/// or the track has no time base.
fn seek_preroll_frames(seeked: &SeekedTo, time_base: Option<TimeBase>, sample_rate: u32) -> u64 {
    let start = seeked.actual_ts.get().max(0);
    let target = seeked.required_ts.get().max(0);
    let (Some(time_base), Ok(ticks)) = (time_base, u64::try_from(target - start)) else {
        return 0;
    };
    let frames = u128::from(ticks) * u128::from(time_base.numer.get()) * u128::from(sample_rate)
        / u128::from(time_base.denom.get());
    u64::try_from(frames).unwrap_or(u64::MAX)
}

/// Take up to `frames` off the seek preroll `frames_left`; returns how many
/// it took.
fn take_seek_preroll(frames_left: &mut u64, frames: u64) -> u64 {
    let taken = (*frames_left).min(frames);
    *frames_left -= taken;
    taken
}

/// How many bytes to drop from the front of a decoded packet for the seek
/// preroll (whole frames only, so the channel interleave never rotates).
fn seek_preroll_prefix_bytes(
    frames_left: &mut u64,
    decoded_len: usize,
    bytes_per_frame: usize,
) -> usize {
    if bytes_per_frame == 0 {
        return 0;
    }
    take_seek_preroll(frames_left, (decoded_len / bytes_per_frame) as u64) as usize
        * bytes_per_frame
}

/// Append `packet_dur` frames of silence (`channel_count` channels,
/// `bytes_per_sample` bytes per sample) when a packet is skipped, keeping time
/// sync and preventing buffer starvation. Fills `output_data` up to its
/// `target_bytes` goal, then carries any remainder into the decoder's
/// `frame_buffer` for the next `read_buffer` call. No-op when `packet_dur == 0`.
///
/// A free function taking the buffers as disjoint `&mut` args (not a `&mut self`
/// method): inside `read_buffer`, `self.decoder`/`self.format_reader` are already
/// borrowed by the `decode()` match, so only field-disjoint access compiles here.
/// Shared by `read_buffer`'s two decode-error arms (I/O and decode errors).
fn fill_silence_for_error(
    channel_count: usize,
    packet_dur: u64,
    bytes_per_sample: usize,
    target_bytes: usize,
    output_data: &mut Vec<u8>,
    frame_buffer: &mut Vec<u8>,
) {
    if packet_dur == 0 {
        return;
    }
    let silence_bytes = (packet_dur as usize) * channel_count * bytes_per_sample;
    let needed = target_bytes.saturating_sub(output_data.len());
    let to_take = needed.min(silence_bytes);

    output_data.extend(std::iter::repeat_n(0, to_take));
    if silence_bytes > to_take {
        frame_buffer.extend(std::iter::repeat_n(0, silence_bytes - to_take));
    }
}

/// Cross-check the probe-derived duration against the server-reported one.
///
/// Symphonia's probe is sample-accurate when the container carries real frame
/// counts, but an MP3 without a Xing/Info or VBRI tag falls back to
/// extrapolating from the first 17 frames, producing durations that are
/// several times off for VBR files with quiet intros. (Before Symphonia 0.6.1
/// a Xing tag on a CRC-protected frame was also rejected outright —
/// pdeljanov/Symphonia#516 — which is how this guard came about: a 4:44 track
/// probed as 30:24.) The server value comes from a full metadata scan, so when
/// the two disagree beyond `max(2s, 5%)` the probe is considered garbage and
/// the server value wins. Within tolerance the probe wins: it is
/// sample-accurate (gapless-trimmed) where the server rounds to whole seconds.
///
/// Known cost of the inverse case (probe right, server stale-short by more
/// than the tolerance — e.g. a file extended on disk before a Navidrome
/// rescan): the displayed range, seek clamp, and crossfade trigger follow the
/// short server value until the server rescans. Accepted: that case is rare
/// and self-healing, while a tagless VBR MP3 misreads the same way every time
/// it plays.
fn sanitize_probed_duration(probed_ms: u64, expected_ms: Option<u64>) -> u64 {
    let Some(expected) = expected_ms.filter(|&e| e > 0) else {
        return probed_ms;
    };

    if probed_ms == 0 {
        return expected;
    }

    const TOLERANCE_FLOOR_MS: u64 = 2_000;
    let tolerance = (expected / 20).max(TOLERANCE_FLOOR_MS);

    if probed_ms.abs_diff(expected) > tolerance {
        warn!(
            " [DECODER] Probed duration {probed_ms}ms contradicts server metadata \
             ({expected}ms) — using the server value (likely a bitrate-extrapolated \
             MP3 estimate)"
        );
        expected
    } else {
        probed_ms
    }
}

/// Whether a sized answer to a request at `offset_secs` holds the whole song
/// from its start (the song's own file, which Navidrome sends whatever the
/// offset) rather than the rest of it from the offset (a transcode its cache
/// holds for that offset, the offset being part of the cache key). Told by
/// the answer's own length (`probed_ms`) against the whole song's: the file
/// runs about the whole song, the cache entry about the whole song less the
/// offset, so whichever is nearer wins. With no length on either side it
/// can't be told, and the offset is taken as honoured (the transcode's
/// case, which a requested transcode on a server that transcoded its first
/// request is, unless its player settings changed since).
fn answer_starts_at_song_start(
    probed_ms: u64,
    whole_song_ms: Option<u64>,
    offset_secs: u64,
) -> bool {
    let offset_ms = offset_secs.saturating_mul(1000);
    whole_song_ms.filter(|&ms| ms > 0).is_some_and(|whole_ms| {
        probed_ms > 0
            && probed_ms.saturating_mul(2) >= whole_ms.saturating_mul(2).saturating_sub(offset_ms)
    })
}

/// Whether a sized answer (`answer_len` bytes) to a request at
/// `offset_secs` is the song's own file, which ignores the offset, rather
/// than a transcode the server's cache holds for that offset. By the song's
/// size on disk when it is known (`file_size`, `Song.size`): Navidrome
/// serves the file at exactly that length, and the UI's `StreamOrigin`
/// trusts the same comparison, so what the clock does and what the badge
/// shows agree. Without a size, by the probed length
/// ([`answer_starts_at_song_start`]).
fn sized_answer_is_the_file(
    answer_len: Option<u64>,
    file_size: Option<u64>,
    probed_ms: u64,
    whole_song_ms: Option<u64>,
    offset_secs: u64,
) -> bool {
    match file_size.filter(|&size| size > 0) {
        Some(size) => answer_len == Some(size),
        None => answer_starts_at_song_start(probed_ms, whole_song_ms, offset_secs),
    }
}

/// How near the whole song's end an offset must be for an answer with no
/// audio in it to be the song's end ([`offset_is_past_end`]): the server's
/// length is whole seconds of a DB value, and an encoder's last frames can
/// run short of it.
const PAST_END_SLACK_MS: u64 = 2_000;

/// Whether an answer with no audio in it, to a request at `offset_secs`,
/// is the song's end: the offset is within [`PAST_END_SLACK_MS`] of the
/// whole song's length, or beyond it. Anywhere else in the song it is a
/// transcode that failed (ffmpeg wrote nothing), and an unknown length can't
/// prove an end.
fn offset_is_past_end(offset_secs: u64, whole_song_ms: Option<u64>) -> bool {
    whole_song_ms.filter(|&ms| ms > 0).is_some_and(|whole_ms| {
        offset_secs
            .saturating_mul(1000)
            .saturating_add(PAST_END_SLACK_MS)
            >= whole_ms
    })
}

/// The error an answer with no audio in it reports, at an offset short of
/// the song's end: the server failed to transcode from there.
const NO_AUDIO_FROM_OFFSET: &str = "the server sent no audio from that point in the song";

/// Ratio that maps the real (displayed) timeline onto Symphonia's internal
/// one after `sanitize_probed_duration` overrode the probe.
///
/// The format reader keeps believing its probed total, so a coarse seek
/// interpolates `target / probed_total` into the byte range. Scaling the
/// target by `probed / real` first makes that interpolation come out at
/// `target / real` of the bytes — the mapping a correct coarse seek would
/// use. Without this, seeking on a file whose probe was overridden lands far
/// short of the target (observed on a Symphonia#516 file: seek to 262s of a
/// 4:44 track landed at ~41s, the position-based crossfade then fired 230s
/// before real EOF and the completion handler wedged on "buffer starvation").
fn compute_seek_scale(probed_ms: u64, sanitized_ms: u64) -> f64 {
    if probed_ms == 0 || sanitized_ms == 0 || probed_ms == sanitized_ms {
        return 1.0;
    }
    probed_ms as f64 / sanitized_ms as f64
}

/// Track length in ms from the container's timing fields, or 0 when the
/// container doesn't declare one (radio) or declares garbage.
///
/// Reads `Track::duration`, not `num_frames`: Symphonia 0.6 defines the frame
/// count as unit-less and only equal to a duration when the time base is
/// `1 / sample_rate`.
fn probed_track_duration_ms(track: &Track) -> u64 {
    let Some(time) = track
        .time_base
        .zip(track.duration)
        .and_then(|(time_base, duration)| time_base.calc_duration(duration))
    else {
        return 0;
    };
    let calculated_ms = u64::try_from(time.as_millis()).unwrap_or_default();

    // Sanity check: if duration is > 24 hours, something is wrong with the metadata
    // Typical songs are under 20 minutes, albums under 2 hours
    const MAX_REASONABLE_DURATION_MS: u64 = 24 * 60 * 60 * 1000; // 24 hours
    if calculated_ms > MAX_REASONABLE_DURATION_MS {
        warn!(
            " [DECODER] Detected garbage duration: {}ms (duration={:?}, time_base={:?}), falling back to 0",
            calculated_ms, track.duration, track.time_base
        );
        return 0;
    }
    calculated_ms
}

/// Output format and codec short name of a freshly built Symphonia decoder.
///
/// Reads the decoder's own codec parameters rather than the track's: each
/// decoder starts from a clone of the track's and some (FLAC, AAC, ALAC) fill
/// in fields from the stream header. Every codec decodes to interleaved f32
/// (see `read_buffer`), which losslessly represents 16- and 24-bit integer PCM
/// — the prerequisite for bit-perfect hi-res playback.
fn decoder_output_format(decoder: &dyn SymphoniaDecoder) -> (AudioFormat, String) {
    let params = decoder.codec_params();
    let sample_rate = params.sample_rate.unwrap_or(44100);
    let channels = params.channels.as_ref().map_or(2, Channels::count) as u32;
    (
        AudioFormat::new(SampleFormat::F32, sample_rate, channels),
        decoder.codec_info().short_name.to_string(),
    )
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
            source_kind: SourceKind::Track,
            delivery: StreamDelivery::Sized,
            byte_len: None,
            expected_duration_ms: None,
            expected_file_size: None,
            answer_timeout: None,
            seek_scale: 1.0,
            live_icy_metadata,
            live_codec: None,
            leading_trim: LeadingTrim::Off,
            seek_preroll_frames: 0,
            offset_secs: 0,
            range_url: None,
        }
    }

    /// Stamp the server-reported duration (ms) before `init()`. The probe's
    /// duration is cross-checked against it in `open_input`; pass `None` when
    /// no trustworthy metadata exists (radio, engine-internal fallback loads).
    pub fn set_expected_duration_ms(&mut self, expected_ms: Option<u64>) {
        self.expected_duration_ms = expected_ms;
    }

    /// Stamp the song's file size on disk before `init()` (see the field
    /// doc); `None` when unknown.
    pub fn set_expected_file_size(&mut self, size: Option<u64>) {
        self.expected_file_size = size.filter(|&size| size > 0);
    }

    /// The song's file size the caller stamped
    /// ([`Self::set_expected_file_size`]).
    pub fn expected_file_size(&self) -> Option<u64> {
        self.expected_file_size
    }

    /// Bound how long a requested transcode's open waits for its answer
    /// (see the field doc).
    pub fn set_answer_timeout(&mut self, timeout: Option<std::time::Duration>) {
        self.answer_timeout = timeout;
    }

    /// Say what this decoder is for before `init()`: a queue song
    /// (the default) or a radio station. See [`SourceKind`].
    pub fn set_source_kind(&mut self, kind: SourceKind) {
        self.source_kind = kind;
    }

    /// Enable the M8 leading-silence trim for this decoder (call before any
    /// `read_buffer`, alongside `set_expected_duration_ms`). Only prepared
    /// TRANSITION decoders opt in — a user-initiated play keeps the honest
    /// recorded start — and the caller gates it off under bit-perfect modes
    /// (dropping samples is a content change).
    pub fn set_trim_leading_silence(&mut self, enabled: bool) {
        self.leading_trim = if enabled {
            LeadingTrim::Pending
        } else {
            LeadingTrim::Off
        };
    }

    /// Initialize decoder with URL (HTTP or file path). A no-op while the
    /// decoder is already open on `url` with audio left to read; at EOF it
    /// reopens the URL, which is how the radio reconnect revives a station
    /// that dropped the connection.
    pub async fn init(&mut self, url: &str) -> Result<()> {
        // Open on `url` from its start only: a stream open at an offset is
        // another stream of the same URL.
        if self.initialized && self.url == url && self.offset_secs == 0 && !self.eof {
            return Ok(());
        }
        self.open_at(url, 0).await
    }

    /// Open `url` from `offset_secs` whole seconds into the song: the request
    /// asks the server for `timeOffset=<offset_secs>` (0 asks for none).
    /// `url` is the track's own URL, never one carrying an offset; it is what
    /// [`Self::url`] reports afterwards. Navidrome honours an offset only on
    /// a transcode, so only a queue song whose URL asks for one can start
    /// mid-song. Always opens anew.
    pub async fn open_at(&mut self, url: &str, offset_secs: u64) -> Result<()> {
        self.stop();
        self.url = url.to_string();
        self.offset_secs = 0;

        if self.url.is_empty() {
            anyhow::bail!("Cannot initialize decoder - URL is empty");
        }
        if offset_secs > 0 && !self.opens_as_requested_transcode() {
            anyhow::bail!("only a transcode nokkvi asked for can start mid-song");
        }

        // What was asked; `open_input` settles what the server did (0 when
        // it sent the song's own file, which ignores the offset).
        self.offset_secs = offset_secs;
        let opened = self.open_input().await;
        if opened.is_err() {
            self.offset_secs = 0;
        }
        opened
    }

    /// Whether the open URL is a queue song asking for a transcode, which
    /// opens with one GET ([`Self::open_requested_transcode`]) and can start
    /// at an offset.
    fn opens_as_requested_transcode(&self) -> bool {
        crate::utils::server_url::has_http_scheme(&self.url)
            && self.source_kind == SourceKind::Track
            && requests_transcode(&self.url)
    }

    /// The URL this decoder opened (the track's own, without `timeOffset`).
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The server's length for the track that the caller stamped before
    /// opening it ([`Self::set_expected_duration_ms`]).
    pub fn expected_duration_ms(&self) -> Option<u64> {
        self.expected_duration_ms
    }

    /// Where in the song the open stream starts, in whole seconds (see the
    /// field doc): its position is that much ahead of what it decoded.
    pub fn offset_secs(&self) -> u64 {
        self.offset_secs
    }

    /// What it takes to open this decoder's track again elsewhere in the
    /// song, without holding it ([`ReopenHints`]).
    pub fn reopen_hints(&self) -> ReopenHints {
        ReopenHints {
            url: self.url.clone(),
            source_kind: self.source_kind,
            expected_duration_ms: self.expected_duration_ms,
            expected_file_size: self.expected_file_size,
            live_icy_metadata: self.live_icy_metadata.clone(),
        }
    }

    /// Bring a freshly opened stream to `target_ms` of the song: a stream
    /// sized from the song's start (the song's own file, which ignored the
    /// offset, or a cache entry from the start) seeks within its bytes; one
    /// that starts at its offset drops the milliseconds between the offset
    /// and the target through its preroll. Blocking (a range seek can
    /// fetch): run under `block_in_place`. False when the range seek failed.
    pub fn land_on(&mut self, target_ms: u64) -> bool {
        if self.delivery == StreamDelivery::Sized && self.offset_secs == 0 {
            target_ms == 0 || self.seek(target_ms)
        } else {
            self.skip_ms(target_ms.saturating_sub(self.offset_secs.saturating_mul(1000)));
            true
        }
    }

    /// Drop the next `ms` of decoded audio, in whole frames, through the seek
    /// preroll: a stream opened at a whole-second offset reaches a target
    /// between two seconds this way, so audio resumes exactly at it.
    pub fn skip_ms(&mut self, ms: u64) {
        self.seek_preroll_frames = ms.saturating_mul(u64::from(self.format.sample_rate())) / 1000;
    }

    /// End the open stream where it is: the next read reports EOF, so the
    /// track ends as it would at its natural end.
    pub fn end_here(&mut self) {
        self.eof = true;
        self.frame_buffer.clear();
        self.seek_preroll_frames = 0;
    }

    async fn open_input(&mut self) -> Result<()> {
        if self.url.is_empty() {
            return Ok(());
        }

        self.close_input();

        // Set by the radio branch: its buffer waits PROBE_RECV_TIMEOUT for the
        // stream's first bytes and drops to READ_RECV_TIMEOUT after the probe.
        let mut radio_recv_timeout: Option<RecvTimeout> = None;
        // The server's length for the stream (`X-Content-Duration`), from the
        // first answer that carries one. Used only for an `Unsized` track,
        // whose probe has no byte length to read one from.
        let mut header_duration_ms: Option<u64> = None;

        // Determine if URL is HTTP/HTTPS or file path (the scheme is
        // case-insensitive; a stream URL must never reach the file branch,
        // whose error would carry the URL and its credential). A queue song
        // whose URL asks for a transcode opens with one GET (no HEAD, no
        // probes); every other URL keeps the sizing sequence below.
        // A requested transcode's answer small enough to be headers alone
        // (the Ogg Navidrome sends for an offset past the song's end).
        let mut tiny_answer = false;
        let (mss, hint_from_data) = if self.opens_as_requested_transcode() {
            let opened = self.open_requested_transcode().await?;
            radio_recv_timeout = opened.recv_timeout;
            header_duration_ms = opened.header_duration_ms;
            tiny_answer = opened.tiny;
            (opened.mss, opened.hint)
        } else if crate::utils::server_url::has_http_scheme(&self.url) {
            // HTTP stream - use Range requests for instant playback
            let init_start = std::time::Instant::now();
            trace!(
                " [DECODER] Starting Range-based HTTP reader for: {}",
                redact_subsonic_url(&self.url)
            );

            let client = reqwest::Client::builder()
                .user_agent(crate::USER_AGENT)
                .timeout(std::time::Duration::from_secs(30))
                // A queue song's URL carries the /rest credential; a station's
                // doesn't, and stations redirect freely.
                .redirect(crate::utils::redirects::guard_credential_urls())
                .build()
                .context("Failed to create HTTP client")?;

            // A station is a live stream whatever its server says about
            // length: skip the sizing probes, so a Content-Length (a plain
            // HTTP server or a proxy) can't turn it into a finite file that
            // ends and never reconnects.
            let mut is_radio = self.source_kind == SourceKind::Station;
            // Set once an answer carries no length and says the server won't
            // serve ranges (`Accept-Ranges: none`, Navidrome's live
            // transcode): no later probe can turn up a length, and each one
            // is a full stream request (with the transcode cache off, its
            // own ffmpeg run), so the open goes straight to the stream GET.
            // Navidrome sends the refusal only on the uncached-transcode
            // path, never for a file it serves in ranges.
            let mut lengthless = false;

            // Try HEAD request first
            let mut content_length = if is_radio {
                None
            } else {
                match client.head(&self.url).send().await {
                    Ok(head_response) if head_response.status().is_success() => {
                        let headers = head_response.headers();
                        // Navidrome answers Subsonic errors with HTTP **200** and an
                        // error envelope in the body (`sendResponseWithStatus` is
                        // called with status 0, so `WriteHeader` never runs). A
                        // track id the server can't resolve therefore sails past
                        // `is_success()` and hands Symphonia a JSON document, which
                        // surfaces as an opaque "format probe failed" rather than
                        // "the server doesn't have this track" — exactly the wrong
                        // diagnostic when a server-side id migration has invalidated
                        // a persisted queue. Content-type is a safe discriminator:
                        // no audio stream, including ICY radio, is served as
                        // JSON/XML, so this cannot reject a working stream.
                        if is_document_answer(headers) {
                            let ct = headers
                                .get(reqwest::header::CONTENT_TYPE)
                                .and_then(|v| v.to_str().ok())
                                .unwrap_or_default();
                            anyhow::bail!(
                                "server returned an error response instead of audio \
                                 (content-type {ct}); the track may no longer exist \
                                 on the server"
                            );
                        }
                        is_radio = is_radio_response(headers);
                        header_duration_ms = x_content_duration_ms(headers);

                        if is_radio {
                            trace!(" [DECODER] ICEcast / Radio stream detected from HEAD headers.");
                            None
                        } else {
                            // A length sizes the stream only from a server
                            // that serves ranges, so a file skips the Range
                            // probe. Beside the refusal it is at most
                            // Navidrome's estimate for a live transcode
                            // (`estimateContentLength`): no file to read in
                            // ranges, so the open still goes straight to the
                            // stream GET.
                            lengthless = refuses_ranges(headers);
                            if lengthless {
                                None
                            } else {
                                header_content_length(headers)
                            }
                        }
                    }
                    _ => None,
                }
            };

            // If HEAD didn't give us content-length, try a Range request with bytes=0-0
            // to get the Content-Range header which tells us total size
            if content_length.is_none() && !is_radio && !lengthless {
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
                        header_duration_ms =
                            header_duration_ms.or_else(|| x_content_duration_ms(headers));

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
                            lengthless = content_length.is_none()
                                && range_response.status().is_success()
                                && refuses_ranges(range_response.headers());
                        }
                    }
                    Err(e) => {
                        trace!(" [DECODER] Range probe failed: {}", e.without_url());
                    }
                }
            }

            // If we still don't have content-length, try a regular GET and read content-length header
            if content_length.is_none() && !is_radio && !lengthless {
                trace!(" [DECODER] Trying regular GET to detect Content-Length...");
                match client.get(&self.url).send().await {
                    Ok(response) if response.status().is_success() => {
                        let headers = response.headers();
                        is_radio = is_radio_response(headers);
                        header_duration_ms =
                            header_duration_ms.or_else(|| x_content_duration_ms(headers));

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
            let mut detected_format = Self::extract_format_hint(&self.url);

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
                self.delivery = StreamDelivery::Sized;
                self.byte_len = Some(len);

                let media_source: Box<dyn MediaSource> = Box::new(reader);
                MediaSourceStream::new(media_source, Default::default())
            } else {
                // No length. A station is a live stream. A queue song is still
                // a finite track: the server sends it without a length when it
                // transcodes on the fly (Navidrome: `Accept-Ranges: none`, no
                // Content-Length until the transcode is cached).
                let station = is_radio || self.source_kind == SourceKind::Station;
                trace!(
                    " [DECODER] No Content-Length found, opening a {} stream.",
                    if station { "live" } else { "length-less track" }
                );

                let client = Self::stream_client()?;
                let mut request = client.get(&self.url);
                if station {
                    request = request.header("Icy-MetaData", "1");
                }
                let response = request
                    .send()
                    .await
                    // A `.send()` error carries the credentialed URL; strip it before
                    // wrapping. This anyhow chain surfaces in full at the decoder-init
                    // failure log (engine.rs, `{:?}`); the radio-reconnect log uses
                    // `{}`, which prints only the top context, so it never showed the URL.
                    .map_err(reqwest::Error::without_url)
                    .context("Failed to open the stream connection")?;

                header_duration_ms =
                    header_duration_ms.or_else(|| x_content_duration_ms(response.headers()));

                let (mss, format, recv_timeout) =
                    self.once_through_source(response, station, Bytes::new(), detected_format);
                detected_format = format;
                radio_recv_timeout = Some(recv_timeout);
                mss
            };

            (mss, detected_format)
        } else {
            // File path
            let file = std::fs::File::open(&self.url).with_context(|| {
                format!(
                    "Failed to open file: {}",
                    crate::utils::url_redaction::redact_subsonic_url(&self.url)
                )
            })?;
            let byte_len = file.metadata().map(|metadata| metadata.len()).ok();
            let media_source: Box<dyn MediaSource> = Box::new(file);
            let mss = MediaSourceStream::new(media_source, Default::default());
            self.delivery = StreamDelivery::Sized;
            self.byte_len = byte_len;

            // Try to extract format from file path
            (mss, Self::extract_format_hint(&self.url))
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
        // `gapless: true` makes the decoder drop the container-declared encoder
        // delay and padding, so album tracks join seamlessly (see
        // `symphonia_registry::probe_and_make_decoder`).
        // The whole song's length, as the server says (`X-Content-Duration`
        // names the whole song even on a stream opened at an offset), else
        // the caller's metadata.
        let whole_song_ms = header_duration_ms
            .or(self.expected_duration_ms)
            .filter(|&ms| ms > 0);
        let probed = tokio::task::block_in_place(|| {
            symphonia_registry::probe_and_make_decoder(mss, &hint, true)
        });
        let (format_reader, decoder, track_id) = match probed {
            Ok(probed) => probed,
            // Past the song's end the server sends a container's headers
            // alone; a probe that finds no audio in that is the song's end.
            // Short of the end, the server failed to transcode from there.
            Err(e) if self.offset_secs > 0 && tiny_answer => {
                if offset_is_past_end(self.offset_secs, whole_song_ms) {
                    debug!(
                        " [DECODER] The answer at {}s holds no audio ({e:#}): past the song's end",
                        self.offset_secs
                    );
                    return Err(StreamPastEnd.into());
                }
                warn!(
                    " [DECODER] The answer at {}s holds no audio ({e:#}), short of the song's end",
                    self.offset_secs
                );
                return Err(anyhow::anyhow!(NO_AUDIO_FROM_OFFSET));
            }
            Err(e) => {
                error!(
                    " [DECODER] Format probe / decoder construction FAILED: {:?}",
                    e
                );
                return Err(e);
            }
        };
        if let Some(recv_timeout) = &radio_recv_timeout {
            recv_timeout.set(READ_RECV_TIMEOUT);
        }
        trace!(
            " [DECODER] Format probe + decoder construction successful (took {:?})",
            probe_start.elapsed()
        );
        trace!(" [DECODER] Found audio track with ID: {}", track_id);
        trace!(" [DECODER] Codec decoder created successfully");

        let (audio_format, codec_name) = decoder_output_format(decoder.as_ref());
        let sample_rate = audio_format.sample_rate();
        let channels = audio_format.channel_count();

        let probed_ms = probed_track_duration_ms(
            format_reader
                .tracks()
                .iter()
                .find(|t| t.id == track_id)
                .context("Selected track id missing from probed format")?,
        );

        if self.offset_secs > 0
            && self.delivery == StreamDelivery::Sized
            && sized_answer_is_the_file(
                self.byte_len,
                self.expected_file_size,
                probed_ms,
                whole_song_ms,
                self.offset_secs,
            )
        {
            // A request that resolves to the song's own file is answered
            // with the whole file, whatever its offset. Read on from the
            // file's own URL: asked again with the offset, the server answers
            // by its own rules (a cache entry, a live transcode).
            info!(
                " [DECODER] The server sent the song's own file for an offset of {}s: \
                 reading it from the song's start",
                self.offset_secs
            );
            self.offset_secs = 0;
            if let Some(range_url) = &self.range_url {
                range_url.set(self.url.clone());
            }
        }

        let duration_ms = if self.offset_secs > 0 {
            // The stream holds only the rest of the song, and its timestamps
            // start at 0: the track's length is the whole song's, and the
            // stream is never sought within (a seek opens the track again at
            // its own offset), so no seek scale applies.
            self.seek_scale = 1.0;
            whole_song_ms
                .unwrap_or_else(|| probed_ms.saturating_add(self.offset_secs.saturating_mul(1000)))
        } else {
            // Cross-check against server metadata: Symphonia's probe can hand
            // back a bitrate-extrapolated MP3 estimate that is several times
            // off. A live transcode's own header wins over the caller's
            // metadata: it is the server's word on this stream.
            let expected_ms = match self.delivery {
                StreamDelivery::Unsized => whole_song_ms,
                StreamDelivery::Sized | StreamDelivery::Live => self.expected_duration_ms,
            };
            let duration_ms = sanitize_probed_duration(probed_ms, expected_ms);
            // When the probe was overridden, the format reader still believes
            // the probed timeline — seeks must be mapped into it (see seek()).
            self.seek_scale = compute_seek_scale(probed_ms, duration_ms);
            duration_ms
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
        self.live_codec = Some(codec_name);
        self.initialized = true;
        self.eof = false;
        self.frame_buffer.clear();

        trace!(" [DECODER] Initialization complete!");
        Ok(())
    }

    /// The client for a stream read once from front to back (a station, or a
    /// queue song sent without a length): no overall timeout, since the
    /// stream may run for hours, but connect / per-read / keepalive guards
    /// matching MPD's defaults (CurlInputPlugin.cxx CURLOPT_CONNECTTIMEOUT /
    /// LOW_SPEED_TIME / CURLOPT_TCP_KEEPALIVE).
    fn stream_client() -> Result<reqwest::Client> {
        reqwest::Client::builder()
            .user_agent(crate::USER_AGENT)
            // A length-less queue song's URL carries the credential.
            .redirect(crate::utils::redirects::guard_credential_urls())
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
            .context("Failed to create the streaming HTTP client")
    }

    /// The source for `response` read once from front to back: a station's
    /// live stream (`station`, or an answer with ICY headers), otherwise a
    /// queue song sent without a length. `first` holds bytes already read
    /// from the body. Returns the source, the format hint (the answer's
    /// content type first, then `detected_format`, then the URL's look) and
    /// the buffer's receive timeout, which the caller drops to
    /// `READ_RECV_TIMEOUT` once the probe is done.
    fn once_through_source(
        &mut self,
        response: reqwest::Response,
        station: bool,
        first: Bytes,
        mut detected_format: Option<String>,
    ) -> (MediaSourceStream<'static>, Option<String>, RecvTimeout) {
        // An answer with ICY headers is a station whatever the caller
        // loaded it as.
        self.delivery = if station || is_radio_response(response.headers()) {
            StreamDelivery::Live
        } else {
            info!(
                " [DECODER] The server sent this track without a length (a live \
                 transcode): playing it once through"
            );
            StreamDelivery::Unsized
        };
        self.byte_len = None;

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
            trace!(" [DECODER] Detected format for the stream: {}", format);
        }

        // Check for ICY metadata
        use icy_metadata::IcyHeaders;
        let icy_headers = IcyHeaders::parse_from_headers(response.headers());
        let interval = icy_headers.metadata_interval();

        // Buffer the response using a dedicated async tokio task
        let recv_timeout = RecvTimeout::new(PROBE_RECV_TIMEOUT);
        let buffered_response =
            AsyncNetworkBuffer::new_async(response, first, recv_timeout.clone());

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
            Box::new(ReadOnlySource::new(icy_reader))
        } else {
            trace!(" [DECODER] No ICY Interval detected in headers!");
            Box::new(ReadOnlySource::new(buffered_response))
        };

        (
            MediaSourceStream::new(media_source, Default::default()),
            detected_format,
            recv_timeout,
        )
    }

    /// Open a queue song whose URL asks for a transcode with ONE GET: no
    /// HEAD (it carries no length or ranges for a transcode, and with the
    /// server's transcode cache off it starts an ffmpeg run that is thrown
    /// away) and no sizing probes. The GET asks for the range reader's first
    /// chunk: a live transcode ignores the range and streams whole, while an
    /// answer the server can serve in ranges (its cache, or the file itself)
    /// sends exactly that chunk with the whole length, which becomes the range
    /// reader's first chunk, so nothing is downloaded twice and the stream
    /// seeks like a file.
    ///
    /// With an offset ([`Self::open_at`]) the request carries `timeOffset`
    /// and so does every Range read of a sized answer (a cache entry is kept
    /// per offset). An empty answer to it is the song's end
    /// ([`StreamPastEnd`]), not a failed transcode.
    async fn open_requested_transcode(&mut self) -> Result<OpenedStream> {
        let request_url = with_time_offset(&self.url, self.offset_secs);
        // The answer's headers and first bytes come by this, when the caller
        // bounded the open ([`Self::set_answer_timeout`]).
        let deadline = self
            .answer_timeout
            .map(|timeout| (tokio::time::Instant::now() + timeout, timeout));
        let sent = Self::stream_client()?
            .get(&request_url)
            .header(
                reqwest::header::RANGE,
                format!("bytes=0-{}", CHUNK_SIZE - 1),
            )
            .send();
        let sent = match deadline {
            Some((deadline, timeout)) => tokio::time::timeout_at(deadline, sent)
                .await
                .map_err(|_| no_answer_within(timeout))?,
            None => sent.await,
        };
        let response = sent
            // The error would carry the credentialed URL.
            .map_err(|e| anyhow::Error::new(NoAnswer(e.without_url().to_string())))
            .context("Failed to open the stream connection")?;
        let header_duration_ms = x_content_duration_ms(response.headers());
        // The whole song's length, for telling an answer with no audio past
        // the song's end from one that failed mid-song.
        let whole_song_ms = header_duration_ms.or(self.expected_duration_ms);
        let content_type_hint = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(format_hint_from_content_type);

        match classify_transcode_answer(response.status(), response.headers()) {
            TranscodeAnswer::Refused(status) => Err(refused_transcode_error(
                status,
                envelope_message(response).await,
            )),
            TranscodeAnswer::ErrorEnvelope => {
                let message = envelope_message(response)
                    .await
                    .unwrap_or_else(|| "an error with no message".to_owned());
                anyhow::bail!("the server sent an error instead of the song: {message}")
            }
            TranscodeAnswer::Empty => Err(self.no_audio_error(whole_song_ms)),
            TranscodeAnswer::WrongRange => anyhow::bail!(
                "the server sent another part of the stream than the one asked for (HTTP 206), \
                 so it can't be read in parts"
            ),
            TranscodeAnswer::PartialWithoutLength => anyhow::bail!(
                "the server sent the start of the stream with no total length (HTTP 206), \
                 so it can't be played"
            ),
            TranscodeAnswer::Sized { total } => {
                // Exactly the first chunk (a 206 for the range asked), or the
                // whole one-chunk file (a 200): either way bytes 0.. of it.
                let wanted = RangeHttpReader::first_chunk_len(total);
                let first_chunk = match deadline {
                    Some((deadline, timeout)) => {
                        tokio::time::timeout_at(deadline, read_prefix(response, wanted))
                            .await
                            .map_err(|_| no_answer_within(timeout))?
                    }
                    None => read_prefix(response, wanted).await,
                };
                debug!(
                    " [DECODER] The requested transcode is served in ranges: {} bytes \
                     ({} of the first chunk in hand)",
                    total,
                    first_chunk.len()
                );
                // Small enough to be a container's headers alone, all of it
                // in hand.
                let tiny = total <= PAST_END_ANSWER_MAX_BYTES && first_chunk.len() as u64 == total;
                self.delivery = StreamDelivery::Sized;
                self.byte_len = Some(total);
                // At an offset, an answer exactly the song's size is its own
                // file, which ignored the offset: read it on from the file's
                // own URL from the start (the classification after the probe
                // agrees). Without a size the probe decides, and moves the
                // reader then (`range_url`).
                let reader_url = if self.offset_secs > 0 && self.expected_file_size == Some(total) {
                    self.url.clone()
                } else {
                    request_url
                };
                let reader = RangeHttpReader::with_first_chunk(reader_url, total, first_chunk);
                self.range_url = Some(reader.url_handle());
                let media_source: Box<dyn MediaSource> = Box::new(reader);
                Ok(OpenedStream {
                    mss: MediaSourceStream::new(media_source, Default::default()),
                    hint: content_type_hint.or_else(|| Self::extract_format_hint(&self.url)),
                    recv_timeout: None,
                    header_duration_ms,
                    tiny,
                })
            }
            TranscodeAnswer::Stream => {
                let mut response = response;
                // The first bytes, so an empty body (ffmpeg wrote nothing)
                // reads as that, not as a format the probe can't make out. At
                // an offset, read past the size of a container's headers
                // alone, or to the end: whether the answer is only that is
                // told by the bytes it really holds, never by a
                // `Content-Length`, which a proxy may drop.
                let wanted = if self.offset_secs > 0 {
                    PAST_END_ANSWER_MAX_BYTES + 1
                } else {
                    1
                };
                let mut first = Vec::new();
                let mut ended = false;
                while (first.len() as u64) < wanted {
                    let chunk = response.chunk();
                    let chunk = match deadline {
                        Some((deadline, timeout)) => tokio::time::timeout_at(deadline, chunk)
                            .await
                            .map_err(|_| no_answer_within(timeout))?,
                        None => chunk.await,
                    };
                    match chunk
                        .map_err(reqwest::Error::without_url)
                        .context("Failed to read the stream")?
                    {
                        Some(chunk) => first.extend_from_slice(&chunk),
                        None => {
                            ended = true;
                            break;
                        }
                    }
                }
                if first.is_empty() {
                    return Err(self.no_audio_error(whole_song_ms));
                }
                let tiny = ended && first.len() as u64 <= PAST_END_ANSWER_MAX_BYTES;
                let (mss, hint, recv_timeout) =
                    self.once_through_source(response, false, Bytes::from(first), None);
                Ok(OpenedStream {
                    mss,
                    hint,
                    recv_timeout: Some(recv_timeout),
                    header_duration_ms,
                    tiny,
                })
            }
        }
    }

    /// The error an answer with no audio in it reports: from the song's
    /// start, the server couldn't transcode the song; at an offset at or
    /// near the song's end, the song is over ([`StreamPastEnd`]); at any
    /// other offset, the server failed to transcode from there (a failed
    /// seek, not an end).
    fn no_audio_error(&self, whole_song_ms: Option<u64>) -> anyhow::Error {
        if self.offset_secs == 0 {
            anyhow::anyhow!(EMPTY_TRANSCODE)
        } else if offset_is_past_end(self.offset_secs, whole_song_ms) {
            StreamPastEnd.into()
        } else {
            anyhow::anyhow!(NO_AUDIO_FROM_OFFSET)
        }
    }

    fn close_input(&mut self) {
        self.format_reader = None;
        self.decoder = None;
        self.track_id = None;
        self.format = AudioFormat::invalid();
        self.duration = 0;
        self.seek_scale = 1.0;
        self.initialized = false;
        self.eof = false;
        self.frame_buffer.clear();
        self.seek_preroll_frames = 0;
        self.smoothed_bitrate_kbps = 0.0;
        self.live_codec = None;
        self.range_url = None;
    }

    /// Extract format hint from URL (file extension or query parameter)
    fn extract_format_hint(url: &str) -> Option<String> {
        // Try to parse as URL first for query parameters
        if crate::utils::server_url::has_http_scheme(url)
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
                Ok(Some(p)) => {
                    // Reset error counter on success
                    consecutive_io_errors = 0;
                    p
                }
                Ok(None) => {
                    // Symphonia 0.6 reports the real end of the media as
                    // `Ok(None)`, so no I/O error below is a clean end.
                    trace!(" [DECODER] Symphonia reached end of stream - treating as EOF");
                    self.eof = true;
                    break;
                }
                Err(SymphoniaError::ResetRequired) => {
                    // The track list changed mid-stream (a chained Ogg ICEcast
                    // stream started a new link). Since Symphonia 0.6 the reader
                    // has already rebuilt its tracks in place, so re-select a
                    // track and rebuild the decoder on the same reader.
                    // `gapless: false` carries over the pre-0.6 behavior of this
                    // path, whose reprobe disabled gapless trimming.
                    // Routine on chained Ogg radio (every song change), so debug.
                    debug!(
                        " [DECODER] ResetRequired - stream's track list changed, rebuilding decoder..."
                    );
                    match symphonia_registry::make_track_decoder(format_reader.as_ref(), false) {
                        Ok((dec, new_track_id)) => {
                            let (format, codec_name) = decoder_output_format(dec.as_ref());
                            self.format = format;
                            self.live_codec = Some(codec_name);
                            self.track_id = Some(new_track_id);
                            self.decoder = Some(dec);
                            // Retry reading the packet with the new decoder.
                            continue;
                        }
                        Err(e) => {
                            error!(
                                " [DECODER] Failed to rebuild the decoder after \
                                 ResetRequired: {:#}",
                                e
                            );
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

                    // The source ran dry mid-packet: a truncated file, or a radio
                    // socket that closed (IcyStreamReader's `read_exact` surfaces
                    // that as UnexpectedEof too). The source has nothing more to
                    // give, so retrying only delays the radio reconnect that EOF
                    // triggers. RangeHttpReader reports network failures as
                    // `ErrorKind::Other`, which the retry path below handles.
                    if io_err.kind() == std::io::ErrorKind::UnexpectedEof {
                        warn!(
                            " [DECODER] Stream ended mid-packet ({}) - treating as EOF",
                            io_err
                        );
                        self.eof = true;
                        break;
                    }

                    consecutive_io_errors += 1;

                    if consecutive_io_errors >= MAX_IO_RETRIES {
                        // I/O error after retries - network issue
                        error!(
                            " [DECODER] I/O error after {} retries: {:?} - treating as EOF",
                            consecutive_io_errors, io_err
                        );
                        self.eof = true;
                        break;
                    }
                    // Retry with bounded backoff for transient network errors.
                    // Capped so even the worst case parks the block_in_place
                    // worker thread only briefly, letting the engine decode
                    // loop's generation check run frequently and abandon a
                    // decoder whose source was already skipped.
                    let backoff_ms = io_retry_backoff_ms(consecutive_io_errors);
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
            if packet.track_id != track_id {
                continue;
            }

            // Compute instantaneous compressed bitrate from packet data.
            // `block_dur` counts every frame the packet codes, including any
            // the decoder trims as encoder delay/padding — the bytes cover them.
            let block_dur = packet.block_dur().get();
            if block_dur > 0 {
                let sample_rate = self.format.sample_rate() as u64;
                if sample_rate > 0 {
                    let bits = packet.data.len() as u64 * 8;
                    let instantaneous_kbps = (bits * sample_rate) / (block_dur * 1000);
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
                    // Convert to interleaved f32 bytes. The decoder has already
                    // dropped the encoder delay/padding frames (gapless trim).
                    let channels = audio_buf.spec().channels().count();
                    let rate = audio_buf.spec().rate();
                    let mut interleaved = Vec::new();
                    audio_buf.copy_bytes_to_vec_interleaved_as::<f32>(&mut interleaved);

                    let mut decoded_bytes = interleaved.as_slice();
                    let bytes_per_frame = channels * std::mem::size_of::<f32>();

                    // M8 leading-silence trim (content-aware — the decoder's
                    // gapless trim is format bookkeeping, this drops musical
                    // silence). Resolve the frame budget on first use, now that
                    // the decoded spec reports the rate.
                    if self.leading_trim == LeadingTrim::Pending {
                        let frames_budget = LEADING_TRIM_MAX_MS * u64::from(rate) / 1000;
                        self.leading_trim = if frames_budget == 0 {
                            LeadingTrim::Done
                        } else {
                            LeadingTrim::Scanning {
                                frames_left: frames_budget,
                            }
                        };
                    }
                    if matches!(self.leading_trim, LeadingTrim::Scanning { .. }) {
                        let drop = leading_trim_prefix_bytes(
                            &mut self.leading_trim,
                            decoded_bytes,
                            bytes_per_frame,
                        );
                        if drop > 0 {
                            trace!("🤫 [DECODER] Leading-silence trim: dropped {} bytes", drop);
                            decoded_bytes = &decoded_bytes[drop..];
                        }
                    }
                    if self.seek_preroll_frames > 0 {
                        let drop = seek_preroll_prefix_bytes(
                            &mut self.seek_preroll_frames,
                            decoded_bytes.len(),
                            bytes_per_frame,
                        );
                        decoded_bytes = &decoded_bytes[drop..];
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

                    // Zero-fill to maintain time sync and prevent buffer
                    // starvation; frames before a seek's target are dropped.
                    let dur = packet.dur.get();
                    fill_silence_for_error(
                        self.format.channel_count() as usize,
                        dur - take_seek_preroll(&mut self.seek_preroll_frames, dur),
                        self.format.bytes_per_sample(),
                        bytes,
                        &mut output_data,
                        &mut self.frame_buffer,
                    );
                    continue;
                }
                Err(SymphoniaError::DecodeError(ref e)) => {
                    // Log and skip packet on decode error
                    warn!(" [DECODER] Decode error (skipping): {:?}", e);

                    // Zero-fill to maintain time sync and prevent buffer
                    // starvation; frames before a seek's target are dropped.
                    let dur = packet.dur.get();
                    fill_silence_for_error(
                        self.format.channel_count() as usize,
                        dur - take_seek_preroll(&mut self.seek_preroll_frames, dur),
                        self.format.bytes_per_sample(),
                        bytes,
                        &mut output_data,
                        &mut self.frame_buffer,
                    );
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

        // M8: a seek ends the leading-silence scan unconditionally (even a
        // failed/early-return seek expresses user intent to move within the
        // track) — only the true track start is ever scanned.
        if self.leading_trim != LeadingTrim::Off {
            self.leading_trim = LeadingTrim::Done;
        }

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

        let mode = seek_mode_for(format_reader.format_info().format);
        let to = |ms: u64| SeekTo::Time {
            time: Time::from_millis_u64(ms),
            track_id: Some(track_id),
        };

        // Map the real-timeline target into Symphonia's internal timeline for
        // a coarse seek. Identity unless the probe duration was overridden in
        // open_input — then the format reader still believes its inflated
        // total and would interpolate the byte offset against it
        // (Symphonia#516). An accurate seek counts frames, which is the real
        // timeline already.
        let coarse_ms = if self.seek_scale == 1.0 {
            position_ms
        } else {
            let scaled = (position_ms as f64 * self.seek_scale) as u64;
            debug!(
                "🔍 [DECODER SEEK] Mapping {}ms into the format reader's timeline: {}ms (scale {:.3})",
                position_ms, scaled, self.seek_scale
            );
            scaled
        };

        trace!(
            "🔍 [DECODER SEEK] Calling format_reader.seek({:?}, {}ms)",
            mode, position_ms
        );
        let seek_start = std::time::Instant::now();
        let result = match mode {
            SeekMode::Accurate => format_reader
                .seek(SeekMode::Accurate, to(position_ms))
                .or_else(|e| {
                    // Past the end of a probe that undercounted the frames:
                    // fall back to where the coarse estimate puts it.
                    debug!(
                        "🔍 [DECODER SEEK] Accurate seek failed ({:?}); seeking coarsely",
                        e
                    );
                    format_reader.seek(SeekMode::Coarse, to(coarse_ms))
                }),
            SeekMode::Coarse => format_reader.seek(SeekMode::Coarse, to(coarse_ms)),
        };
        match result {
            Ok(seeked_to) => {
                let time_base = format_reader
                    .tracks()
                    .iter()
                    .find(|t| t.id == track_id)
                    .and_then(|t| t.time_base);
                self.seek_preroll_frames =
                    seek_preroll_frames(&seeked_to, time_base, self.format.sample_rate());
                debug!(
                    "🔍 [DECODER SEEK] format_reader.seek() completed in {:?}, seeked to ts={} \
                     (target ts={}, dropping {} preroll frames)",
                    seek_start.elapsed(),
                    seeked_to.actual_ts,
                    seeked_to.required_ts,
                    self.seek_preroll_frames
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

    /// Test-only: stamp the reported track duration without going through
    /// `init`/`open_input` (which need real network/file I/O). Lets engine arm
    /// tests build a prepared decoder whose `duration()` clears the renderer's
    /// minimum-track-length arm gate (`crossfade_length_ms`).
    #[cfg(test)]
    pub fn set_duration_for_test(&mut self, duration_ms: u64) {
        self.duration = duration_ms;
    }

    /// Test-only: stamp the reported audio format without going through
    /// `init`/`open_input` (which need real network/file I/O). Lets the engine
    /// gapless-swap characterization test build decoders whose `format()` is
    /// `valid()` and matches, so the inline swap's format-equality gate clears.
    #[cfg(test)]
    pub fn set_format_for_test(&mut self, format: AudioFormat) {
        self.format = format;
    }

    /// Test-only: stamp the live codec string so the gapless-swap
    /// characterization test can assert it propagates into `GaplessTransitionInfo`.
    #[cfg(test)]
    pub fn set_live_codec_for_test(&mut self, codec: Option<String>) {
        self.live_codec = codec;
    }

    /// Test-only: stamp how the open source delivers its bytes, so an engine
    /// test can take a sized file for one its first open read without a
    /// length (a live transcode the server has since finished caching).
    #[cfg(test)]
    pub fn set_delivery_for_test(&mut self, delivery: StreamDelivery) {
        self.delivery = delivery;
    }

    /// Test-only: stamp the answer's byte length (see [`Self::byte_len`]) so
    /// engine tests can follow it through the promotions without a server.
    #[cfg(test)]
    pub fn set_byte_len_for_test(&mut self, byte_len: Option<u64>) {
        self.byte_len = byte_len;
    }

    /// Test-only: stamp decoder EOF so engine tests can drive the
    /// track-completion path of `on_renderer_finished` without real I/O.
    #[cfg(test)]
    pub fn set_eof_for_test(&mut self, eof: bool) {
        self.eof = eof;
    }

    /// Check if decoder is initialized
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Check if decoder has reached end of file
    /// Whether this decoder trims leading silence ("Skip Silence Between
    /// Tracks" on a prepared transition decoder): its played length then
    /// falls short of its duration by the trimmed intro, which nothing
    /// counts, so the engine passes no verdict on an early end.
    pub fn trims_leading_silence(&self) -> bool {
        self.leading_trim != LeadingTrim::Off
    }

    pub fn is_eof(&self) -> bool {
        self.eof
    }

    /// True for a radio station's live stream ([`StreamDelivery::Live`]).
    /// The engine reads it to skip backpressure and to reconnect at EOF
    /// instead of ending the track.
    pub fn is_infinite_stream(&self) -> bool {
        self.delivery == StreamDelivery::Live
    }

    /// How the open source delivers its bytes (see [`StreamDelivery`]).
    pub fn delivery(&self) -> StreamDelivery {
        self.delivery
    }

    /// The answer's byte length when the source is `Sized`, `None` otherwise.
    pub fn byte_len(&self) -> Option<u64> {
        self.byte_len
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
pub(crate) mod tests {
    use super::*;

    // =========================================================================
    // sanitize_probed_duration — the probe-vs-server-metadata cross-check
    // applied in open_input. Pure fn; the bug it guards against is Symphonia's
    // bitrate-extrapolated MP3 duration estimate (pdeljanov/Symphonia#516:
    // a 4:44 VBR file probed as 30:24).
    // =========================================================================

    // =========================================================================
    // M8 leading-silence trim — pure scan over interleaved f32 LE frames.
    // =========================================================================

    /// Interleaved stereo f32 LE bytes from per-sample amplitudes.
    fn frames_bytes(samples: &[f32]) -> Vec<u8> {
        samples.iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    const STEREO_FRAME: usize = 8; // 2 channels × 4 bytes

    #[test]
    fn leading_trim_off_and_done_pass_through() {
        let bytes = frames_bytes(&[0.0, 0.0, 0.5, 0.5]);
        let mut off = LeadingTrim::Off;
        assert_eq!(leading_trim_prefix_bytes(&mut off, &bytes, STEREO_FRAME), 0);
        assert_eq!(off, LeadingTrim::Off);
        let mut done = LeadingTrim::Done;
        assert_eq!(
            leading_trim_prefix_bytes(&mut done, &bytes, STEREO_FRAME),
            0
        );
        assert_eq!(done, LeadingTrim::Done);
    }

    /// The first frame with ANY sample at/above the threshold survives; the
    /// silent frames before it are dropped, frame-aligned (a mid-frame cut
    /// would rotate the channel interleave).
    #[test]
    fn leading_trim_drops_silent_frames_until_first_loud_frame() {
        // 3 silent stereo frames, then a frame loud only in its RIGHT channel.
        let bytes = frames_bytes(&[0.0, 0.0, 1e-4, -1e-4, 0.0, 0.0, 0.0, 0.5, 0.3, 0.3]);
        let mut state = LeadingTrim::Scanning { frames_left: 1_000 };
        assert_eq!(
            leading_trim_prefix_bytes(&mut state, &bytes, STEREO_FRAME),
            3 * STEREO_FRAME
        );
        assert_eq!(state, LeadingTrim::Done, "a loud frame ends the scan");
    }

    /// An all-silent buffer under budget is dropped whole and the scan
    /// continues into the next buffer (multi-call state).
    #[test]
    fn leading_trim_continues_across_buffers() {
        let silent = frames_bytes(&[0.0; 8]); // 4 stereo frames
        let mut state = LeadingTrim::Scanning { frames_left: 10 };
        assert_eq!(
            leading_trim_prefix_bytes(&mut state, &silent, STEREO_FRAME),
            silent.len()
        );
        assert_eq!(state, LeadingTrim::Scanning { frames_left: 6 });

        // Next buffer starts loud — nothing more is dropped.
        let loud = frames_bytes(&[0.5, 0.5, 0.1, 0.1]);
        assert_eq!(
            leading_trim_prefix_bytes(&mut state, &loud, STEREO_FRAME),
            0
        );
        assert_eq!(state, LeadingTrim::Done);
    }

    /// Budget exhaustion mid-buffer flips to Done and the remaining silence
    /// plays honestly (an all-silent track must still play, never
    /// fast-forward to EOF).
    #[test]
    fn leading_trim_budget_exhaustion_stops_trimming() {
        let silent = frames_bytes(&[0.0; 10]); // 5 stereo frames
        let mut state = LeadingTrim::Scanning { frames_left: 2 };
        assert_eq!(
            leading_trim_prefix_bytes(&mut state, &silent, STEREO_FRAME),
            2 * STEREO_FRAME,
            "only the budgeted frames are dropped"
        );
        assert_eq!(state, LeadingTrim::Done);
    }

    /// Threshold boundary: exactly −60 dBFS (1e-3) counts as LOUD (the scan
    /// keeps it), one ULP below stays silent.
    #[test]
    fn leading_trim_threshold_is_inclusive() {
        let bytes = frames_bytes(&[9.9e-4, -9.9e-4, 1e-3, 0.0]);
        let mut state = LeadingTrim::Scanning { frames_left: 1_000 };
        assert_eq!(
            leading_trim_prefix_bytes(&mut state, &bytes, STEREO_FRAME),
            STEREO_FRAME,
            "the sub-threshold frame drops; the at-threshold frame survives"
        );
        assert_eq!(state, LeadingTrim::Done);
    }

    /// A seek must end the scan: `set_trim_leading_silence(true)` arms
    /// Pending, and `seek()` flips it to Done so a mid-track seek never trims
    /// content (only the true track start is scanned).
    #[test]
    fn leading_trim_seek_disables_scan() {
        let mut dec = AudioDecoder::default();
        dec.set_trim_leading_silence(true);
        assert_eq!(dec.leading_trim, LeadingTrim::Pending);
        // Uninitialized seek returns false but must still kill the scan arm.
        let _ = dec.seek(1_000);
        assert_eq!(dec.leading_trim, LeadingTrim::Done);
    }

    #[test]
    fn sanitize_keeps_probe_without_expectation() {
        assert_eq!(sanitize_probed_duration(284_280, None), 284_280);
        assert_eq!(
            sanitize_probed_duration(284_280, Some(0)),
            284_280,
            "a zero expectation carries no information"
        );
    }

    #[test]
    fn sanitize_keeps_probe_within_tolerance() {
        // Probe is sample-accurate where the server rounds to whole seconds —
        // small disagreement must not displace it.
        assert_eq!(sanitize_probed_duration(284_280, Some(284_000)), 284_280);
        // Short track: the 2s floor dominates 5% (30s ± 2s). Exactly at the
        // boundary is agreement; one ms past it is not.
        assert_eq!(sanitize_probed_duration(32_000, Some(30_000)), 32_000);
        assert_eq!(sanitize_probed_duration(32_001, Some(30_000)), 30_000);
        // Long track: 5% beats the 2s floor (3600s ± 180s).
        assert_eq!(
            sanitize_probed_duration(3_750_000, Some(3_600_000)),
            3_750_000
        );
    }

    #[test]
    fn sanitize_prefers_server_when_probe_is_wildly_off() {
        // The real Symphonia#516 numbers: 4:44 track probed as 30:24.757.
        assert_eq!(sanitize_probed_duration(1_824_757, Some(284_000)), 284_000);
        // Probe far too short is equally garbage.
        assert_eq!(sanitize_probed_duration(30_000, Some(284_000)), 284_000);
    }

    #[test]
    fn sanitize_falls_back_to_server_when_probe_is_zero() {
        // No n_frames (or the 24h garbage guard zeroed the probe): the server
        // value is strictly better than reporting nothing.
        assert_eq!(sanitize_probed_duration(0, Some(284_000)), 284_000);
        assert_eq!(sanitize_probed_duration(0, None), 0);
    }

    // =========================================================================
    // compute_seek_scale — real-timeline → Symphonia-timeline mapping for
    // coarse seeks after the duration override. Wrong scale = seeks land at
    // the wrong byte offset (the "seek to the end repeats the song" bug).
    // =========================================================================

    #[test]
    fn seek_scale_is_identity_when_probe_was_kept() {
        assert_eq!(compute_seek_scale(284_280, 284_280), 1.0);
    }

    #[test]
    fn seek_scale_maps_real_targets_into_the_inflated_probe_timeline() {
        // The Symphonia#516 numbers: probe 1824.757s, real 284.28s. A seek to
        // the real end must land near the probe timeline's end.
        let scale = compute_seek_scale(1_824_757, 284_280);
        assert!((scale - 6.418).abs() < 0.01, "scale was {scale}");
        let mapped = (284_280.0 * scale) as u64;
        assert!((1_820_000..=1_830_000).contains(&mapped), "mapped {mapped}");
    }

    #[test]
    fn seek_scale_is_identity_without_a_probe_timeline() {
        // probed == 0 means Symphonia has no duration model to map into.
        assert_eq!(compute_seek_scale(0, 284_000), 1.0);
        // sanitized == 0 cannot happen after the zero-fallback, but stay safe.
        assert_eq!(compute_seek_scale(284_000, 0), 1.0);
    }

    // =========================================================================
    // fill_silence_for_error — the shared silence-fill used by read_buffer's two
    // decode-error arms. Tested directly as a pure fn (the two arms can't be
    // driven without a Symphonia error-injection harness, which doesn't exist).
    // =========================================================================

    #[test]
    fn fill_silence_zero_duration_is_noop() {
        let mut output = vec![1u8; 4];
        let mut frame_buffer = Vec::new();
        fill_silence_for_error(2, 0, 4, 100, &mut output, &mut frame_buffer);
        assert_eq!(output, vec![1u8; 4], "zero-dur packet must not append");
        assert!(frame_buffer.is_empty());
    }

    #[test]
    fn fill_silence_all_fits_in_output_leaves_frame_buffer_empty() {
        // 2 ch * 10 frames * 4 bytes (f32) = 80 bytes of silence; output goal has room.
        let mut output = Vec::new();
        let mut frame_buffer = Vec::new();
        fill_silence_for_error(2, 10, 4, 100, &mut output, &mut frame_buffer);
        assert_eq!(output, vec![0u8; 80], "all silence goes to output");
        assert!(
            frame_buffer.is_empty(),
            "no carry-over when silence fits the goal"
        );
    }

    #[test]
    fn fill_silence_splits_remainder_into_frame_buffer() {
        // Output already 90/100 full of real data; 80 bytes of silence → 10 to
        // output (reaching the 100 goal), 70 carried into the frame buffer.
        let mut output = vec![1u8; 90];
        let mut frame_buffer = Vec::new();
        fill_silence_for_error(2, 10, 4, 100, &mut output, &mut frame_buffer);
        assert_eq!(output.len(), 100);
        assert_eq!(&output[..90], &[1u8; 90], "existing data is preserved");
        assert_eq!(&output[90..], &[0u8; 10], "goal is topped up with silence");
        assert_eq!(frame_buffer, vec![0u8; 70], "remainder carries over");
    }

    #[test]
    fn fill_silence_when_output_already_full_carries_all_over() {
        // Output already at the goal: nothing appended there, all silence carried.
        let mut output = vec![1u8; 100];
        let mut frame_buffer = Vec::new();
        fill_silence_for_error(2, 10, 4, 100, &mut output, &mut frame_buffer);
        assert_eq!(output.len(), 100, "no append once the goal is met");
        assert_eq!(frame_buffer, vec![0u8; 80], "full silence carries over");
    }

    // =========================================================================
    // io_retry_backoff_ms (N5)
    // =========================================================================

    /// N5: the transient-I/O retry backoff must stay bounded so a synchronous
    /// std::thread::sleep under block_in_place never parks a tokio worker for
    /// multiple seconds (the old `400 * (1 << min(n-1, 5))` formula reached
    /// 12800 ms at attempt 8, pinning a worker thread for a decoder whose
    /// source may have already been skipped).
    #[test]
    fn io_retry_backoff_is_bounded() {
        for n in 0..=MAX_IO_RETRIES_FOR_TEST {
            assert!(
                io_retry_backoff_ms(n) <= 500,
                "backoff for {n} consecutive errors must be <= 500ms, got {}",
                io_retry_backoff_ms(n),
            );
        }
        // Worst observed retry count (MAX_IO_RETRIES = 8) must be capped.
        assert!(io_retry_backoff_ms(8) <= 500);
    }

    /// The backoff still grows with consecutive errors (gives transient
    /// network blips a chance to recover) before saturating at the cap.
    #[test]
    fn io_retry_backoff_grows_then_saturates() {
        assert_eq!(io_retry_backoff_ms(0), 0);
        assert_eq!(io_retry_backoff_ms(1), 100);
        assert_eq!(io_retry_backoff_ms(3), 300);
        assert_eq!(io_retry_backoff_ms(5), 500);
        assert_eq!(io_retry_backoff_ms(6), 500);
    }

    /// Mirror of the production `MAX_IO_RETRIES` so the bound test covers the
    /// full retry range without exposing the const outside `read_buffer`.
    const MAX_IO_RETRIES_FOR_TEST: u32 = 8;

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
    /// resolves promptly.
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

    /// Regression: `Drop for AsyncNetworkBuffer` must not panic when invoked
    /// from inside a tokio runtime. The earlier implementation called
    /// `Handle::current().block_on(...)` to await the producer's exit, which
    /// panics with "Cannot start a runtime from within a runtime" the moment
    /// Drop fires from a worker thread — production hit this on the
    /// radio→library track transition (the new decoder replaces the old
    /// `AsyncNetworkBuffer` from inside a tokio worker).
    ///
    /// Hand-constructed fields stand in for `AsyncNetworkBuffer::new_async`,
    /// which needs a live HTTP response.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn drop_inside_runtime_worker_does_not_panic() {
        let (tx, rx) = mpsc::channel::<Bytes>(1);
        let cancel = CancellationToken::new();
        let child_cancel = cancel.clone();
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = child_cancel.cancelled() => return,
                    result = tx.send(Bytes::from_static(b"x")) => {
                        if result.is_err() { return; }
                    }
                }
                tokio::task::yield_now().await;
            }
        });

        let buf = AsyncNetworkBuffer {
            rx,
            leftover: Bytes::new(),
            cancel,
            task: Some(task),
            recv_timeout: RecvTimeout::new(READ_RECV_TIMEOUT),
        };

        // Run the Drop from a spawned task so it executes on a worker thread —
        // mirrors the production trigger (async decoder swap on track change).
        let drop_result = tokio::spawn(async move {
            drop(buf);
        })
        .await;

        assert!(
            drop_result.is_ok(),
            "AsyncNetworkBuffer::drop must not panic on a tokio worker thread, got: \
             {drop_result:?}"
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

    /// How long the fake station in [`spawn_slow_start_station`] holds its
    /// first audio bytes back after the response headers.
    const FIRST_BYTE_DELAY: std::time::Duration = std::time::Duration::from_millis(800);

    /// What the fake station in [`spawn_slow_start_station`] does once it has
    /// sent its audio.
    #[derive(Clone, Copy)]
    pub(crate) enum AfterAudio {
        /// Keep the socket open and send nothing more (a stalled stream).
        Stall,
        /// Close the socket (the station dropped the listener).
        HangUp,
    }

    /// Local Icecast-shaped station: `icy-` header, no Content-Length, the
    /// response headers at once, then the MP3 fixture after
    /// [`FIRST_BYTE_DELAY`], then `after_audio`. Every connection gets the
    /// same treatment, so a reconnect finds the station live again. Returns
    /// the stream URL.
    pub(crate) async fn spawn_slow_start_station(after_audio: AfterAudio) -> String {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };

        const AUDIO: &[u8] = include_bytes!("../../testdata/xing_crc_protected.mp3");

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut request = [0u8; 2048];
                    let len = socket.read(&mut request).await.unwrap_or(0);
                    let headers = b"HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\n\
                        icy-name: slow start\r\nConnection: close\r\n\r\n";
                    let _ = socket.write_all(headers).await;
                    if request[..len].starts_with(b"GET") {
                        tokio::time::sleep(FIRST_BYTE_DELAY).await;
                        let _ = socket.write_all(AUDIO).await;
                        match after_audio {
                            AfterAudio::Stall => {
                                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                            }
                            AfterAudio::HangUp => {}
                        }
                    }
                });
            }
        });
        format!("http://{addr}/stream.mp3")
    }

    /// A station that answers the GET promptly but holds its first audio bytes
    /// back for longer than `READ_RECV_TIMEOUT` must still open. Icecast starts
    /// a new listener at the next Ogg page, so the first audio can lag the
    /// response headers by a page's worth of time (Radio SEGA measured
    /// 260–500 ms, so its first play attempt failed about half the time).
    #[tokio::test(flavor = "multi_thread")]
    async fn radio_open_waits_for_a_slow_first_audio_byte() {
        let url = spawn_slow_start_station(AfterAudio::Stall).await;

        let mut decoder = AudioDecoder::default();
        let result = decoder.init(&url).await;

        assert!(
            result.is_ok(),
            "a station whose first audio byte lags the headers by {FIRST_BYTE_DELAY:?} \
             failed to open: {result:?}"
        );
        assert!(decoder.is_infinite_stream());
    }

    /// Once the probe returns, a stalled radio stream must hand control back
    /// to the decode loop within `READ_RECV_TIMEOUT` again, not the probe's
    /// 10 s — the loop's generation check is what makes stop/skip responsive.
    #[tokio::test(flavor = "multi_thread")]
    async fn radio_read_after_probe_returns_promptly_on_a_stall() {
        let url = spawn_slow_start_station(AfterAudio::Stall).await;
        let mut decoder = AudioDecoder::default();
        decoder
            .init(&url)
            .await
            .expect("the slow-start station must open");

        // The fixture decodes to ~0.7 MB of f32; asking for more drains it and
        // then blocks on the stalled socket until the receive timeout.
        let started = std::time::Instant::now();
        tokio::task::block_in_place(|| decoder.read_buffer(4 * 1024 * 1024));
        let elapsed = started.elapsed();

        assert!(
            elapsed < std::time::Duration::from_secs(3),
            "read_buffer on a stalled stream took {elapsed:?}; the probe's receive \
             timeout leaked into the decode path"
        );
        assert!(!decoder.is_eof(), "a stall is not the end of the stream");
    }

    /// The radio reconnect (`radio_reconnect_loop` in engine.rs) re-inits the
    /// decoder with the URL that just dropped. That re-init must open a fresh
    /// connection: returning early because the URL matches left the spent
    /// stream in place, so the loop logged "Success!" and the station stayed
    /// silent forever.
    #[tokio::test(flavor = "multi_thread")]
    async fn radio_reinit_after_a_dropped_stream_reconnects() {
        let url = spawn_slow_start_station(AfterAudio::HangUp).await;
        let mut decoder = AudioDecoder::default();
        decoder.init(&url).await.expect("the station must open");

        // Drain the fixture; the station then hangs up, which reads as EOF.
        let started = std::time::Instant::now();
        while !decoder.is_eof() && started.elapsed() < std::time::Duration::from_secs(10) {
            tokio::task::block_in_place(|| decoder.read_buffer(4 * 1024 * 1024));
        }
        assert!(decoder.is_eof(), "precondition: a hang-up must read as EOF");

        decoder
            .init(&url)
            .await
            .expect("the station must accept the reconnect");

        assert!(
            !decoder.is_eof(),
            "a re-init of the dropped URL must reopen it, not keep the spent stream"
        );
        let buffer = tokio::task::block_in_place(|| decoder.read_buffer(64 * 1024));
        assert!(
            buffer.is_valid() && buffer.byte_count() > 0,
            "the reconnected stream must decode audio"
        );
    }

    // =========================================================================
    // A server-side live transcode (Navidrome, not yet cached)
    // =========================================================================

    /// A quarter second (ten frames) of the fixture, without its first MPEG frame (the Xing
    /// header). ffmpeg writing to a pipe can't go back to fill one in, so a
    /// live transcode carries none and the probe can't read a length from
    /// it. Kept shorter than the decode cushion (`CUSHION_MS`), so an engine
    /// decode loop reaches the end without anything draining its ring.
    pub(crate) fn transcode_body() -> &'static [u8] {
        const AUDIO: &[u8] = include_bytes!("../../testdata/xing_crc_protected.mp3");
        // 128 kbps MPEG-1 Layer III at 44.1 kHz, no padding: 417 bytes.
        const FRAME_LEN: usize = 417;
        &AUDIO[FRAME_LEN..FRAME_LEN * 11]
    }

    /// A local server shaped like Navidrome serving a live transcode on
    /// `/rest/stream`: `Accept-Ranges: none`, no Content-Length, an
    /// `X-Content-Duration` header when `duration_header` is set, an audio
    /// content type and no ICY headers. Every GET (a Range probe too, which a
    /// live transcode ignores) gets the whole body, chunked, then the
    /// connection closes; a HEAD gets the headers alone. Returns the stream
    /// URL and the number of requests served so far.
    pub(crate) async fn spawn_live_transcode(
        duration_header: Option<&'static str>,
    ) -> (String, Arc<std::sync::atomic::AtomicUsize>) {
        let (url, requests, _methods) = spawn_live_transcode_logged(duration_header).await;
        (url, requests)
    }

    /// [`spawn_live_transcode`], also returning each request's method in the
    /// order the server read them.
    pub(crate) async fn spawn_live_transcode_logged(
        duration_header: Option<&'static str>,
    ) -> (
        String,
        Arc<std::sync::atomic::AtomicUsize>,
        Arc<parking_lot::Mutex<Vec<String>>>,
    ) {
        spawn_lengthless_stream(duration_header, true, None).await
    }

    /// The live-transcode server, with `Accept-Ranges: none` only when
    /// `refuse_ranges` is set (Navidrome always sends it), and a
    /// Content-Length on the HEAD when `head_length` is set (Navidrome's
    /// estimate under `estimateContentLength`).
    async fn spawn_lengthless_stream(
        duration_header: Option<&'static str>,
        refuse_ranges: bool,
        head_length: Option<usize>,
    ) -> (
        String,
        Arc<std::sync::atomic::AtomicUsize>,
        Arc<parking_lot::Mutex<Vec<String>>>,
    ) {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };

        let requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let methods = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let served = requests.clone();
        let logged = methods.clone();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                served.fetch_add(1, Ordering::SeqCst);
                let logged = logged.clone();
                tokio::spawn(async move {
                    let mut request = [0u8; 2048];
                    let len = socket.read(&mut request).await.unwrap_or(0);
                    if let Some(method) = String::from_utf8_lossy(&request[..len])
                        .split_whitespace()
                        .next()
                    {
                        logged.lock().push(method.to_owned());
                    }
                    let mut headers =
                        String::from("HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\n");
                    if refuse_ranges {
                        headers.push_str("Accept-Ranges: none\r\n");
                    }
                    if let Some(secs) = duration_header {
                        headers.push_str(&format!("X-Content-Duration: {secs}\r\n"));
                    }
                    if request[..len].starts_with(b"GET") {
                        headers.push_str("Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n");
                        let _ = socket.write_all(headers.as_bytes()).await;
                        for chunk in transcode_body().chunks(4096) {
                            let _ = socket
                                .write_all(format!("{:x}\r\n", chunk.len()).as_bytes())
                                .await;
                            let _ = socket.write_all(chunk).await;
                            let _ = socket.write_all(b"\r\n").await;
                        }
                        let _ = socket.write_all(b"0\r\n\r\n").await;
                    } else {
                        if let Some(len) = head_length {
                            headers.push_str(&format!("Content-Length: {len}\r\n"));
                        }
                        headers.push_str("Connection: close\r\n\r\n");
                        let _ = socket.write_all(headers.as_bytes()).await;
                    }
                    let _ = socket.shutdown().await;
                });
            }
        });
        (
            format!("http://{addr}/rest/stream?id=t1&f=json"),
            requests,
            methods,
        )
    }

    /// Read `decoder` until EOF (or 10 s), as the decode loop would.
    fn drain_to_eof(decoder: &mut AudioDecoder) {
        let started = std::time::Instant::now();
        while !decoder.is_eof() && started.elapsed() < std::time::Duration::from_secs(10) {
            tokio::task::block_in_place(|| decoder.read_buffer(64 * 1024));
        }
    }

    /// A stream URL with the Subsonic credential in it never follows a
    /// redirect to another host; a station's URL (no credential) still does.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_credentialed_stream_never_follows_a_redirect_off_the_server() {
        let (redirecting, elsewhere) =
            crate::services::api::test_server::redirect_to_another_host().await;

        let mut decoder = AudioDecoder::default();
        let song = format!(
            "{}/rest/stream?id=s1&apiKey=nds_SECRET&f=json",
            redirecting.base_url
        );
        assert!(decoder.init(&song).await.is_err());
        assert!(
            elsewhere.requests().is_empty(),
            "{:?}",
            elsewhere.requests()
        );

        let mut station = AudioDecoder::default();
        let live = format!("{}/live.mp3", redirecting.base_url);
        let _ = station.init(&live).await;
        assert!(
            !elsewhere.requests().is_empty(),
            "a station's redirect is followed"
        );
    }

    /// A queue song the server transcodes on the fly arrives with no length
    /// and no ICY headers. It is still a finite track: it must not be taken
    /// for a radio station (which the engine reconnects at EOF, restarting
    /// the song instead of moving on), and it can't seek.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_track_served_without_a_length_is_a_finite_unseekable_track() {
        let (url, _requests) = spawn_live_transcode(Some("0.261")).await;

        let mut decoder = AudioDecoder::default();
        decoder.init(&url).await.expect("the transcode must open");

        assert!(
            !decoder.is_infinite_stream(),
            "a queue song without a length was classified as a radio stream"
        );
        assert_eq!(decoder.delivery(), StreamDelivery::Unsized);

        drain_to_eof(&mut decoder);
        assert!(decoder.is_eof(), "the transcode's end must read as EOF");
    }

    /// A live transcode's length comes from its `X-Content-Duration` header
    /// (Navidrome sets it on every `/rest/stream` answer): the probe can't
    /// read one from a stream with no Xing header and no byte length, and
    /// without it the progress bar and the crossfade timing have nothing.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_live_transcode_takes_its_length_from_x_content_duration() {
        let (url, _requests) = spawn_live_transcode(Some("245.5")).await;

        let mut decoder = AudioDecoder::default();
        decoder.init(&url).await.expect("the transcode must open");

        assert_eq!(decoder.duration(), 245_500);
    }

    /// With no `X-Content-Duration`, a live transcode falls back to the
    /// server's metadata length passed in by the caller.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_live_transcode_without_the_header_uses_the_metadata_length() {
        let (url, _requests) = spawn_live_transcode(None).await;

        let mut decoder = AudioDecoder::default();
        decoder.set_expected_duration_ms(Some(200_000));
        decoder.init(&url).await.expect("the transcode must open");

        assert_eq!(decoder.duration(), 200_000);
    }

    /// A live transcode's first answer (the HEAD) already says it has no
    /// length and serves no ranges, so the open goes straight to the stream
    /// GET. It used to send a Range probe and a plain GET as well, each
    /// thrown away, and with Navidrome's transcode cache off each one started
    /// its own ffmpeg run.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_live_transcode_opens_with_a_head_and_one_get() {
        let (url, _requests, methods) = spawn_live_transcode_logged(Some("245.5")).await;

        let mut decoder = AudioDecoder::default();
        decoder.init(&url).await.expect("the transcode must open");

        assert_eq!(*methods.lock(), ["HEAD", "GET"]);
        assert_eq!(decoder.delivery(), StreamDelivery::Unsized);
        assert_eq!(
            decoder.duration(),
            245_500,
            "the length still comes from the HEAD's X-Content-Duration"
        );
    }

    /// The control: a server that sends no length but never says it refuses
    /// ranges keeps the full sequence (HEAD, Range probe, plain GET, stream
    /// GET), since a later answer may still carry a length.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_lengthless_answer_that_allows_ranges_keeps_probing() {
        let (url, _requests, methods) = spawn_lengthless_stream(Some("245.5"), false, None).await;

        let mut decoder = AudioDecoder::default();
        decoder.init(&url).await.expect("the stream must open");

        assert_eq!(*methods.lock(), ["HEAD", "GET", "GET", "GET"]);
        assert_eq!(decoder.delivery(), StreamDelivery::Unsized);
    }

    /// With `estimateContentLength`, Navidrome puts an estimated
    /// Content-Length on a live transcode's answer beside
    /// `Accept-Ranges: none`. That is no file to read in ranges: the open
    /// stays the HEAD and the stream GET, and the track stays `Unsized`.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_live_transcode_with_an_estimated_length_stays_unsized() {
        let (url, _requests, methods) =
            spawn_lengthless_stream(Some("245.5"), true, Some(3_928_000)).await;

        let mut decoder = AudioDecoder::default();
        decoder.init(&url).await.expect("the transcode must open");

        assert_eq!(*methods.lock(), ["HEAD", "GET"]);
        assert_eq!(decoder.delivery(), StreamDelivery::Unsized);
    }

    /// The bytes [`spawn_sized_file`] serves.
    pub(crate) const SIZED_FILE: &[u8] = include_bytes!("../../testdata/xing_crc_protected.mp3");

    /// A local server shaped like Navidrome serving a file it has whole
    /// (`http.ServeContent`): every answer carries `Accept-Ranges: bytes` and
    /// a Content-Length, a HEAD gets the headers alone, a GET with a Range
    /// header gets that slice (206) and a GET without one the whole file.
    /// Returns the stream URL and each request as `"METHOD"` or
    /// `"METHOD <range>"`, in the order the server read them.
    pub(crate) async fn spawn_sized_file() -> (String, Arc<parking_lot::Mutex<Vec<String>>>) {
        spawn_sized_file_with(true).await
    }

    /// [`spawn_sized_file`], whose HEAD answer carries the Content-Length
    /// only when `head_length` is set (a server or proxy whose HEAD names no
    /// length leaves the size to the `bytes=0-0` Range probe).
    async fn spawn_sized_file_with(
        head_length: bool,
    ) -> (String, Arc<parking_lot::Mutex<Vec<String>>>) {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };

        let requests = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let logged = requests.clone();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let logged = logged.clone();
                tokio::spawn(async move {
                    let mut request = [0u8; 2048];
                    let len = socket.read(&mut request).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&request[..len]).into_owned();
                    let method = request.split_whitespace().next().unwrap_or("").to_owned();
                    let range = request.lines().find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("range")
                            .then(|| value.trim().to_owned())
                    });
                    logged.lock().push(match &range {
                        Some(range) => format!("{method} {range}"),
                        None => method.clone(),
                    });

                    let last = SIZED_FILE.len() - 1;
                    let slice = range
                        .as_deref()
                        .and_then(|r| r.strip_prefix("bytes="))
                        .and_then(|r| r.split_once('-'))
                        .map(|(start, end)| {
                            let start: usize = start.parse().unwrap();
                            let end = end.parse::<usize>().map_or(last, |end| end.min(last));
                            (start, end)
                        });
                    let (status, body) = match slice {
                        Some((start, end)) => ("206 Partial Content", &SIZED_FILE[start..=end]),
                        None => ("200 OK", SIZED_FILE),
                    };
                    let mut headers = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: audio/mpeg\r\nAccept-Ranges: bytes\r\n"
                    );
                    if method != "HEAD" || head_length {
                        headers.push_str(&format!("Content-Length: {}\r\n", body.len()));
                    }
                    if let Some((start, end)) = slice {
                        headers.push_str(&format!(
                            "Content-Range: bytes {start}-{end}/{}\r\n",
                            SIZED_FILE.len()
                        ));
                    }
                    headers.push_str("Connection: close\r\n\r\n");
                    let _ = socket.write_all(headers.as_bytes()).await;
                    if method != "HEAD" {
                        let _ = socket.write_all(body).await;
                    }
                    let _ = socket.shutdown().await;
                });
            }
        });
        (format!("http://{addr}/rest/stream?id=t1&f=json"), requests)
    }

    /// A file the server serves in ranges opens with the HEAD, whose
    /// Content-Length sizes the stream, and the reader's range GET. It used
    /// to send a `bytes=0-0` Range probe between the two on every open:
    /// reqwest's `content_length()` is the body's size, which a HEAD answer
    /// never has, so the HEAD sized nothing.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_sized_file_opens_with_a_head_and_its_range_get() {
        let (url, requests) = spawn_sized_file().await;

        // Opened on a runtime thread, as `play()` opens every track: the
        // reader's blocking reqwest client is built there, and a debug build
        // used to refuse that (`RangeHttpReader::create_client`).
        let mut decoder = AudioDecoder::default();
        decoder.init(&url).await.expect("the file must open");
        let delivery = decoder.delivery();

        assert_eq!(
            *requests.lock(),
            [
                "HEAD".to_owned(),
                format!("GET bytes=0-{}", SIZED_FILE.len() - 1)
            ]
        );
        assert_eq!(delivery, StreamDelivery::Sized);
    }

    /// A sized answer records its byte length beside its delivery: the UI
    /// compares it with the song's file size to tell the file from a
    /// transcode the server's cache serves sized (an MP3 body for a `.flac`
    /// song). The length is what the server said; the codec is what decodes.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_sized_answer_records_its_byte_length_and_its_codec() {
        let (url, _requests) = spawn_sized_file().await;

        let mut decoder = AudioDecoder::default();
        decoder.init(&url).await.expect("the answer must open");

        assert_eq!(decoder.delivery(), StreamDelivery::Sized);
        assert_eq!(decoder.byte_len(), Some(SIZED_FILE.len() as u64));
        assert_eq!(decoder.live_codec().as_deref(), Some("mp3"));
    }

    /// A local file is sized by the file itself.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_local_file_records_its_size_as_its_byte_length() {
        let size = std::fs::metadata(VBR_SILENCE_THEN_NOISE)
            .expect("the fixture exists")
            .len();

        let mut decoder = AudioDecoder::default();
        decoder
            .init(VBR_SILENCE_THEN_NOISE)
            .await
            .expect("the fixture opens");

        assert_eq!(decoder.byte_len(), Some(size));
    }

    /// A live transcode has no length to record, and neither has a station,
    /// even on a decoder whose previous open was sized: the length is the
    /// open answer's, never left over from the last one.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_stream_without_a_length_records_no_byte_length() {
        let (sized, _requests) = spawn_sized_file().await;
        let (lengthless, _requests) = spawn_live_transcode(Some("245.5")).await;

        let mut transcode = AudioDecoder::default();
        transcode.init(&sized).await.expect("the file must open");
        assert_eq!(transcode.byte_len(), Some(SIZED_FILE.len() as u64));
        transcode
            .init(&lengthless)
            .await
            .expect("the transcode must open");
        let mut station = AudioDecoder::default();
        station.init(&sized).await.expect("the file must open");
        station.set_source_kind(SourceKind::Station);
        station
            .init(&lengthless)
            .await
            .expect("the station must open");

        assert_eq!(transcode.delivery(), StreamDelivery::Unsized);
        assert_eq!(transcode.byte_len(), None);
        assert_eq!(station.delivery(), StreamDelivery::Live);
        assert_eq!(station.byte_len(), None);
    }

    /// A HEAD that names no length leaves the size to the `bytes=0-0` Range
    /// probe, whose 206 carries the whole length as the Content-Range total
    /// (its own Content-Length is the one byte it sends): that total is the
    /// answer's byte length.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_head_without_a_length_takes_the_range_probes_total_as_the_byte_length() {
        let (url, requests) = spawn_sized_file_with(false).await;

        let mut decoder = AudioDecoder::default();
        decoder.init(&url).await.expect("the file must open");

        assert_eq!(
            *requests.lock(),
            [
                "HEAD".to_owned(),
                "GET bytes=0-0".to_owned(),
                format!("GET bytes=0-{}", SIZED_FILE.len() - 1)
            ]
        );
        assert_eq!(decoder.delivery(), StreamDelivery::Sized);
        assert_eq!(decoder.byte_len(), Some(SIZED_FILE.len() as u64));
    }

    // =========================================================================
    // A requested transcode (Streaming Quality): one GET, no HEAD
    // =========================================================================

    fn headers(pairs: &[(&'static str, &'static str)]) -> reqwest::header::HeaderMap {
        let mut map = reqwest::header::HeaderMap::new();
        for (name, value) in pairs {
            map.insert(*name, value.parse().unwrap());
        }
        map
    }

    #[test]
    fn a_transcode_answer_is_judged_by_its_status_and_headers() {
        use reqwest::StatusCode;
        let cases = [
            // The server's cache (or the raw file) answering the range asked for.
            (
                StatusCode::PARTIAL_CONTENT,
                headers(&[
                    ("content-range", "bytes 0-262143/3216574"),
                    ("content-length", "262144"),
                    ("accept-ranges", "bytes"),
                ]),
                TranscodeAnswer::Sized { total: 3_216_574 },
            ),
            // A small file answered whole: the range asked for covered it all.
            (
                StatusCode::PARTIAL_CONTENT,
                headers(&[
                    ("content-range", "bytes 0-4169/4170"),
                    ("content-length", "4170"),
                    ("accept-ranges", "bytes"),
                ]),
                TranscodeAnswer::Sized { total: 4170 },
            ),
            // The Range header stripped on the way, but the whole file fits
            // in the first chunk: it is all in hand, so it stays sized.
            (
                StatusCode::OK,
                headers(&[("accept-ranges", "bytes"), ("content-length", "4170")]),
                TranscodeAnswer::Sized { total: 4170 },
            ),
            // The Range header stripped on the way, a file bigger than one
            // chunk: later Range requests would be stripped too, so it is
            // read once through, not sized.
            (
                StatusCode::OK,
                headers(&[("accept-ranges", "bytes"), ("content-length", "300000")]),
                TranscodeAnswer::Stream,
            ),
            // A 206 for another range than asked.
            (
                StatusCode::PARTIAL_CONTENT,
                headers(&[("content-range", "bytes 1-262144/3216574")]),
                TranscodeAnswer::WrongRange,
            ),
            (
                StatusCode::PARTIAL_CONTENT,
                headers(&[("content-range", "bytes 0-999/3216574")]),
                TranscodeAnswer::WrongRange,
            ),
            (
                StatusCode::PARTIAL_CONTENT,
                headers(&[("content-range", "bytes 0-262143/*")]),
                TranscodeAnswer::PartialWithoutLength,
            ),
            // A live transcode: no length, ranges refused.
            (
                StatusCode::OK,
                headers(&[("accept-ranges", "none"), ("content-type", "audio/ogg")]),
                TranscodeAnswer::Stream,
            ),
            // A tiny live answer Go sized itself (an offset past the end) is
            // still no file to read in ranges.
            (
                StatusCode::OK,
                headers(&[("accept-ranges", "none"), ("content-length", "266")]),
                TranscodeAnswer::Stream,
            ),
            // A length alone, ranges never mentioned.
            (
                StatusCode::OK,
                headers(&[("content-length", "4170")]),
                TranscodeAnswer::Stream,
            ),
            (StatusCode::OK, headers(&[]), TranscodeAnswer::Stream),
            (
                StatusCode::OK,
                headers(&[("accept-ranges", "none"), ("content-length", "0")]),
                TranscodeAnswer::Empty,
            ),
            (
                StatusCode::RANGE_NOT_SATISFIABLE,
                headers(&[("content-range", "bytes */0")]),
                TranscodeAnswer::Empty,
            ),
            (
                StatusCode::OK,
                headers(&[("content-type", "application/json; charset=utf-8")]),
                TranscodeAnswer::ErrorEnvelope,
            ),
            (
                StatusCode::OK,
                headers(&[("content-type", "text/xml")]),
                TranscodeAnswer::ErrorEnvelope,
            ),
            (
                StatusCode::FOUND,
                headers(&[("location", "http://elsewhere.invalid/")]),
                TranscodeAnswer::Refused(StatusCode::FOUND),
            ),
            (
                StatusCode::TOO_MANY_REQUESTS,
                headers(&[("content-type", "application/json"), ("retry-after", "5")]),
                TranscodeAnswer::Refused(StatusCode::TOO_MANY_REQUESTS),
            ),
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                headers(&[]),
                TranscodeAnswer::Refused(StatusCode::INTERNAL_SERVER_ERROR),
            ),
        ];
        for (status, headers, want) in cases {
            assert_eq!(
                classify_transcode_answer(status, &headers),
                want,
                "{status} {headers:?}"
            );
        }
    }

    /// One request a [`spawn_stream_server`] read.
    #[derive(Debug, Clone)]
    pub(crate) struct LoggedRequest {
        pub(crate) method: String,
        /// Path and query, as sent.
        pub(crate) target: String,
        pub(crate) range: Option<String>,
    }

    impl LoggedRequest {
        fn asks_for_transcode(&self) -> bool {
            self.target.contains("&format=")
        }
    }

    /// How [`spawn_stream_server`] answers a request whose query asks for a
    /// transcode (`format=`). A request without one always gets the song's
    /// file, the way Navidrome serves it (`http.ServeContent`).
    #[derive(Clone, Copy)]
    pub(crate) enum TranscodeReply {
        /// A live transcode: 200, chunked, `Accept-Ranges: none`,
        /// `X-Content-Duration`; any Range header ignored; a HEAD gets the
        /// headers alone.
        Live,
        /// A cached transcode, served like a file: Range honoured.
        Cached,
        /// A Subsonic error envelope in a 200 (Navidrome's way).
        SubsonicError,
        /// A redirect to another host.
        RedirectOffServer,
        /// 429 + an envelope: the server's transcode limit is reached.
        TooBusy,
        /// A 200 with `Content-Length: 0`: ffmpeg wrote nothing.
        Empty,
        /// A 200 whose chunked body ends at once.
        EmptyChunked,
        /// Something on the way stripped the Range header: 200, the whole
        /// body, `Accept-Ranges: bytes` and its length.
        RangeDropped,
        /// A 206 for the range one byte later than asked.
        WrongStart,
        /// A 206 for only the first 1000 bytes of the range asked.
        Capped,
        /// A tiny live answer Go sized itself: 200, `Accept-Ranges: none`,
        /// a Content-Length, the whole body.
        LiveTiny,
        /// A 206 whose `Content-Range` total is `*`.
        PartialNoTotal,
    }

    /// Bigger than the range reader's 256 KB chunk, so reading it to the end
    /// takes more than the first chunk: the transcode body's frames
    /// repeated (no Xing header, so nothing in it names a length).
    pub(crate) fn two_chunk_body() -> Vec<u8> {
        transcode_body().repeat(70)
    }

    /// A Navidrome-shaped `/rest/stream` server that logs every request
    /// (method, path + query, Range) and answers by query: `reply` for a
    /// transcode request, the file (`two_chunk_body`, served like
    /// `http.ServeContent`) for any other. Returns the server's base URL and
    /// the log.
    pub(crate) async fn spawn_stream_server(
        reply: TranscodeReply,
    ) -> (String, Arc<parking_lot::Mutex<Vec<LoggedRequest>>>) {
        spawn_stream_server_with(reply, two_chunk_body()).await
    }

    /// [`spawn_stream_server`] serving `body` (the file and every transcode).
    pub(crate) async fn spawn_stream_server_with(
        reply: TranscodeReply,
        body: Vec<u8>,
    ) -> (String, Arc<parking_lot::Mutex<Vec<LoggedRequest>>>) {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };

        let body: Arc<Vec<u8>> = Arc::new(body);
        let log = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let logged = log.clone();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let logged = logged.clone();
                let body = body.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let len = socket.read(&mut buf).await.unwrap_or(0);
                    let text = String::from_utf8_lossy(&buf[..len]).into_owned();
                    let mut first = text.lines().next().unwrap_or("").split_whitespace();
                    let method = first.next().unwrap_or("").to_owned();
                    let target = first.next().unwrap_or("").to_owned();
                    let range = text.lines().find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("range")
                            .then(|| value.trim().to_owned())
                    });
                    let request = LoggedRequest {
                        method,
                        target,
                        range,
                    };
                    logged.lock().push(request.clone());
                    let head = request.method == "HEAD";

                    let answer = if request.asks_for_transcode() {
                        Some(reply)
                    } else {
                        None
                    };
                    let (status, mut headers, payload): (&str, String, Vec<u8>) = match answer {
                        Some(TranscodeReply::Live) => {
                            let mut headers = String::from(
                                "Content-Type: audio/mpeg\r\nAccept-Ranges: none\r\n\
                                 X-Content-Duration: 245.5\r\n",
                            );
                            if head {
                                ("200 OK", headers, Vec::new())
                            } else {
                                headers.push_str("Transfer-Encoding: chunked\r\n");
                                let mut chunked = Vec::new();
                                for chunk in body.chunks(4096) {
                                    chunked.extend_from_slice(
                                        format!("{:x}\r\n", chunk.len()).as_bytes(),
                                    );
                                    chunked.extend_from_slice(chunk);
                                    chunked.extend_from_slice(b"\r\n");
                                }
                                chunked.extend_from_slice(b"0\r\n\r\n");
                                ("200 OK", headers, chunked)
                            }
                        }
                        Some(TranscodeReply::SubsonicError) => {
                            let envelope = br#"{"subsonic-response":{"status":"failed","version":"1.16.1","type":"navidrome","error":{"code":70,"message":"data not found"}}}"#;
                            (
                                "200 OK",
                                format!(
                                    "Content-Type: application/json\r\nContent-Length: {}\r\n",
                                    envelope.len()
                                ),
                                envelope.to_vec(),
                            )
                        }
                        Some(TranscodeReply::RedirectOffServer) => (
                            "302 Found",
                            "Location: http://elsewhere.invalid/rest/stream?id=s1\r\n\
                             Content-Length: 0\r\n"
                                .to_owned(),
                            Vec::new(),
                        ),
                        Some(TranscodeReply::TooBusy) => {
                            let envelope = br#"{"subsonic-response":{"status":"failed","version":"1.16.1","type":"navidrome","error":{"code":0,"message":"too many concurrent transcodes"}}}"#;
                            (
                                "429 Too Many Requests",
                                format!(
                                    "Content-Type: application/json\r\nRetry-After: 5\r\n\
                                     Content-Length: {}\r\n",
                                    envelope.len()
                                ),
                                envelope.to_vec(),
                            )
                        }
                        Some(TranscodeReply::Empty) => (
                            "200 OK",
                            "Content-Type: audio/ogg\r\nAccept-Ranges: none\r\n\
                             Content-Length: 0\r\nX-Content-Duration: 245.5\r\n"
                                .to_owned(),
                            Vec::new(),
                        ),
                        Some(TranscodeReply::EmptyChunked) => (
                            "200 OK",
                            "Content-Type: audio/ogg\r\nAccept-Ranges: none\r\n\
                             Transfer-Encoding: chunked\r\nX-Content-Duration: 245.5\r\n"
                                .to_owned(),
                            b"0\r\n\r\n".to_vec(),
                        ),
                        Some(TranscodeReply::RangeDropped) => (
                            "200 OK",
                            format!(
                                "Content-Type: audio/mpeg\r\nAccept-Ranges: bytes\r\n\
                                 Content-Length: {}\r\nX-Content-Duration: 245.5\r\n",
                                body.len()
                            ),
                            if head { Vec::new() } else { body.to_vec() },
                        ),
                        Some(TranscodeReply::LiveTiny) => (
                            "200 OK",
                            format!(
                                "Content-Type: audio/mpeg\r\nAccept-Ranges: none\r\n\
                                 Content-Length: {}\r\nX-Content-Duration: 245.5\r\n",
                                body.len()
                            ),
                            if head { Vec::new() } else { body.to_vec() },
                        ),
                        Some(TranscodeReply::WrongStart) => {
                            let end = 262_144.min(body.len() - 1);
                            (
                                "206 Partial Content",
                                format!(
                                    "Content-Type: audio/mpeg\r\nAccept-Ranges: bytes\r\n\
                                     Content-Range: bytes 1-{end}/{}\r\nContent-Length: {}\r\n",
                                    body.len(),
                                    end
                                ),
                                body[1..=end].to_vec(),
                            )
                        }
                        Some(TranscodeReply::Capped) => (
                            "206 Partial Content",
                            format!(
                                "Content-Type: audio/mpeg\r\nAccept-Ranges: bytes\r\n\
                                 Content-Range: bytes 0-999/{}\r\nContent-Length: 1000\r\n",
                                body.len()
                            ),
                            body[..1000].to_vec(),
                        ),
                        Some(TranscodeReply::PartialNoTotal) => {
                            let end = 262_143.min(body.len() - 1);
                            (
                                "206 Partial Content",
                                format!(
                                    "Content-Type: audio/mpeg\r\nAccept-Ranges: bytes\r\n\
                                     Content-Range: bytes 0-{end}/*\r\nContent-Length: {}\r\n",
                                    end + 1
                                ),
                                body[..=end].to_vec(),
                            )
                        }
                        Some(TranscodeReply::Cached) | None => {
                            let last = body.len() - 1;
                            let slice = request
                                .range
                                .as_deref()
                                .and_then(|r| r.strip_prefix("bytes="))
                                .and_then(|r| r.split_once('-'))
                                .map(|(start, end)| {
                                    let start: usize = start.parse().unwrap();
                                    let end =
                                        end.parse::<usize>().map_or(last, |end| end.min(last));
                                    (start, end)
                                });
                            let mut headers = String::from(
                                "Content-Type: audio/mpeg\r\nAccept-Ranges: bytes\r\n\
                                 X-Content-Duration: 245.5\r\n",
                            );
                            match slice {
                                Some((start, end)) => {
                                    headers.push_str(&format!(
                                        "Content-Range: bytes {start}-{end}/{}\r\n\
                                         Content-Length: {}\r\n",
                                        body.len(),
                                        end - start + 1
                                    ));
                                    let payload = if head {
                                        Vec::new()
                                    } else {
                                        body[start..=end].to_vec()
                                    };
                                    ("206 Partial Content", headers, payload)
                                }
                                None => {
                                    headers
                                        .push_str(&format!("Content-Length: {}\r\n", body.len()));
                                    let payload = if head { Vec::new() } else { body.to_vec() };
                                    ("200 OK", headers, payload)
                                }
                            }
                        }
                    };
                    headers.push_str("Connection: close\r\n\r\n");
                    let _ = socket
                        .write_all(format!("HTTP/1.1 {status}\r\n{headers}").as_bytes())
                        .await;
                    let _ = socket.write_all(&payload).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        (format!("http://{addr}"), log)
    }

    /// A queue song's stream URL on `base`, asking for an Opus 128 transcode
    /// when `transcode` is set, with the token credential a real one carries.
    fn song_url(base: &str, transcode: bool) -> String {
        let params = if transcode {
            "&format=opus&maxBitRate=128"
        } else {
            ""
        };
        format!("{base}/rest/stream?id=s1{params}&u=nk&t=token&s=salt&f=json&v=1.8.0&c=nokkvi")
    }

    /// Opening a requested transcode is one GET: no HEAD (with the server's
    /// transcode cache off, a HEAD starts an ffmpeg run that is thrown away)
    /// and no probes. A live answer streams once through, unsized, its length
    /// from the GET's `X-Content-Duration`.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_requested_transcode_opens_with_one_get_and_no_head() {
        let (base, log) = spawn_stream_server(TranscodeReply::Live).await;

        let mut decoder = AudioDecoder::default();
        decoder
            .init(&song_url(&base, true))
            .await
            .expect("the transcode must open");

        let log = log.lock().clone();
        assert_eq!(log.len(), 1, "{log:?}");
        assert_eq!(log[0].method, "GET", "{log:?}");
        assert!(log[0].asks_for_transcode(), "{log:?}");
        assert_eq!(decoder.delivery(), StreamDelivery::Unsized);
        assert_eq!(decoder.byte_len(), None);
        assert_eq!(decoder.duration(), 245_500);
        drain_to_eof(&mut decoder);
        assert!(decoder.is_eof(), "the transcode plays through to its end");
    }

    /// A transcode the server already holds whole (its cache) answers the
    /// GET's first-chunk range sized. That one answer is the range reader's
    /// first chunk, so nothing is downloaded twice, and every later read is a
    /// Range request like a file's.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_requested_transcode_answered_in_ranges_reads_on_with_range_requests() {
        let (base, log) = spawn_stream_server(TranscodeReply::Cached).await;
        let total = two_chunk_body().len() as u64;

        let mut decoder = AudioDecoder::default();
        decoder
            .init(&song_url(&base, true))
            .await
            .expect("the cached transcode must open");

        assert_eq!(decoder.delivery(), StreamDelivery::Sized);
        assert_eq!(decoder.byte_len(), Some(total));
        drain_to_eof(&mut decoder);

        let log = log.lock().clone();
        assert!(log.iter().all(|r| r.method == "GET"), "no HEAD: {log:?}");
        assert!(
            log.iter().all(|r| r.range.is_some()),
            "only Range reads: {log:?}"
        );
        assert_eq!(log[0].range.as_deref(), Some("bytes=0-262143"), "{log:?}");
        assert_eq!(
            log.iter()
                .filter(|r| r.range.as_deref() == Some("bytes=0-262143"))
                .count(),
            1,
            "the first chunk is fetched once: {log:?}"
        );
        assert!(
            log.iter().any(|r| r
                .range
                .as_deref()
                .is_some_and(|r| r.starts_with("bytes=262144-"))),
            "the rest comes in by Range: {log:?}"
        );
    }

    /// Navidrome answers a Subsonic error with HTTP 200 and an error envelope.
    /// The open fails with the server's own message, after that one GET.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_requested_transcode_answered_with_a_subsonic_error_fails_with_its_message() {
        let (base, log) = spawn_stream_server(TranscodeReply::SubsonicError).await;

        let mut decoder = AudioDecoder::default();
        let error = decoder
            .init(&song_url(&base, true))
            .await
            .expect_err("an error envelope is no audio");

        assert!(format!("{error}").contains("data not found"), "{error:#}");
        assert_eq!(log.lock().len(), 1, "{:?}", log.lock());
    }

    /// A redirect the client won't follow (it leaves the server) is an error,
    /// not audio, and nothing reaches the other host.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_requested_transcode_that_redirects_off_the_server_fails() {
        let (base, log) = spawn_stream_server(TranscodeReply::RedirectOffServer).await;

        let mut decoder = AudioDecoder::default();
        let error = decoder
            .init(&song_url(&base, true))
            .await
            .expect_err("a redirect is no audio");

        assert!(format!("{error}").contains("302"), "{error:#}");
        assert_eq!(log.lock().len(), 1, "{:?}", log.lock());
        assert!(
            !format!("{error:#}").contains("token"),
            "the credential stays out of the error: {error:#}"
        );
    }

    /// The server's transcode limit answers 429. The open fails and says the
    /// server is busy, after that one GET.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_requested_transcode_refused_as_too_busy_fails_and_says_so() {
        let (base, log) = spawn_stream_server(TranscodeReply::TooBusy).await;

        let mut decoder = AudioDecoder::default();
        let error = decoder
            .init(&song_url(&base, true))
            .await
            .expect_err("a 429 is no audio");

        assert!(format!("{error}").contains("busy"), "{error:#}");
        assert_eq!(log.lock().len(), 1, "{:?}", log.lock());
    }

    /// When ffmpeg writes nothing, Navidrome still answers 200 with an empty
    /// body. That reads as the server failing to transcode the song, not as a
    /// format probe failure, whether the empty body is sized or chunked.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_empty_answer_to_a_requested_transcode_says_the_server_could_not_transcode() {
        for reply in [TranscodeReply::Empty, TranscodeReply::EmptyChunked] {
            let (base, log) = spawn_stream_server(reply).await;

            let mut decoder = AudioDecoder::default();
            let error = decoder
                .init(&song_url(&base, true))
                .await
                .expect_err("an empty answer is no audio");

            assert!(
                format!("{error}").contains("the server couldn't transcode this song"),
                "{error:#}"
            );
            assert_eq!(log.lock().len(), 1, "{:?}", log.lock());
        }
    }

    /// A proxy that strips the Range header gets the whole body back as a
    /// 200. Later Range requests would be stripped too, so a file bigger than
    /// one chunk is read once through on that one answer (the bytes already
    /// read go first), never sized: sized, every later chunk would come back
    /// as the file's start.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_requested_transcode_whose_range_was_dropped_on_the_way_streams_once_through() {
        let (base, log) = spawn_stream_server(TranscodeReply::RangeDropped).await;

        let mut decoder = AudioDecoder::default();
        decoder
            .init(&song_url(&base, true))
            .await
            .expect("the answer must open");

        assert_eq!(decoder.delivery(), StreamDelivery::Unsized);
        assert_eq!(decoder.byte_len(), None);
        drain_to_eof(&mut decoder);
        assert!(decoder.is_eof());
        assert_eq!(log.lock().len(), 1, "{:?}", log.lock());
    }

    /// The same answer for a file that fits in one chunk is the whole file
    /// in hand: it stays sized and seekable, and nothing more is asked for.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_small_answer_whose_range_was_dropped_is_sized_from_what_is_in_hand() {
        let body = transcode_body().to_vec();
        let (base, log) =
            spawn_stream_server_with(TranscodeReply::RangeDropped, body.clone()).await;

        let mut decoder = AudioDecoder::default();
        decoder
            .init(&song_url(&base, true))
            .await
            .expect("the answer must open");

        assert_eq!(decoder.delivery(), StreamDelivery::Sized);
        assert_eq!(decoder.byte_len(), Some(body.len() as u64));
        assert!(tokio::task::block_in_place(|| decoder.seek(100)));
        drain_to_eof(&mut decoder);
        assert_eq!(log.lock().len(), 1, "{:?}", log.lock());
    }

    /// A small cached transcode answers the first-chunk range with all of
    /// itself: one request in total, open and play through included.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_small_cached_transcode_opens_with_one_request_in_total() {
        let body = transcode_body().to_vec();
        let (base, log) = spawn_stream_server_with(TranscodeReply::Cached, body.clone()).await;

        let mut decoder = AudioDecoder::default();
        decoder
            .init(&song_url(&base, true))
            .await
            .expect("the cached transcode must open");

        assert_eq!(decoder.delivery(), StreamDelivery::Sized);
        assert_eq!(decoder.byte_len(), Some(body.len() as u64));
        drain_to_eof(&mut decoder);
        let log = log.lock().clone();
        assert_eq!(log.len(), 1, "{log:?}");
        assert_eq!(log[0].range.as_deref(), Some("bytes=0-262143"));
    }

    /// A tiny live answer Go sized itself (`Accept-Ranges: none` beside a
    /// length, like the headers-only Ogg past a song's end) is no file to
    /// read in ranges: it stays a once-through stream.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_tiny_live_answer_with_a_length_stays_unsized() {
        let (base, log) =
            spawn_stream_server_with(TranscodeReply::LiveTiny, transcode_body().to_vec()).await;

        let mut decoder = AudioDecoder::default();
        decoder
            .init(&song_url(&base, true))
            .await
            .expect("the answer must open");

        assert_eq!(decoder.delivery(), StreamDelivery::Unsized);
        assert_eq!(decoder.byte_len(), None);
        drain_to_eof(&mut decoder);
        assert_eq!(log.lock().len(), 1, "{:?}", log.lock());
    }

    /// A 206 for another range than the one asked for (starting elsewhere,
    /// or capped short) fails the open with that said, after the one GET:
    /// the range reader could never fill its chunks from such a server.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_206_for_another_range_than_asked_fails_the_open() {
        for reply in [TranscodeReply::WrongStart, TranscodeReply::Capped] {
            let (base, log) = spawn_stream_server(reply).await;

            let mut decoder = AudioDecoder::default();
            let error = decoder
                .init(&song_url(&base, true))
                .await
                .expect_err("the wrong range is no stream");

            assert!(format!("{error}").contains("another part"), "{error:#}");
            assert_eq!(log.lock().len(), 1, "{:?}", log.lock());
        }
    }

    /// A 206 whose total is `*` names no length: the open fails and says so
    /// (it used to read as the server refusing the stream with a 206).
    #[tokio::test(flavor = "multi_thread")]
    async fn a_206_without_a_total_length_fails_and_says_so() {
        let (base, _log) = spawn_stream_server(TranscodeReply::PartialNoTotal).await;

        let mut decoder = AudioDecoder::default();
        let error = decoder
            .init(&song_url(&base, true))
            .await
            .expect_err("a slice with no length is no stream");

        let text = format!("{error}");
        assert!(text.contains("no total length"), "{error:#}");
        assert!(!text.contains("refused"), "{error:#}");
    }

    /// A song streamed as its file (no `format`) still opens exactly as
    /// before: the HEAD sizes it, then the reader's Range GET.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_song_streamed_as_its_file_still_opens_with_the_head_first() {
        let (base, log) = spawn_stream_server(TranscodeReply::Live).await;
        let total = two_chunk_body().len() as u64;

        let mut decoder = AudioDecoder::default();
        decoder
            .init(&song_url(&base, false))
            .await
            .expect("the file must open");

        let log = log.lock().clone();
        assert_eq!(log[0].method, "HEAD", "{log:?}");
        assert_eq!(log[0].range, None, "{log:?}");
        assert_eq!(log[1].method, "GET", "{log:?}");
        assert_eq!(log[1].range.as_deref(), Some("bytes=0-262143"), "{log:?}");
        assert!(log.iter().all(|r| !r.asks_for_transcode()), "{log:?}");
        assert_eq!(decoder.delivery(), StreamDelivery::Sized);
        assert_eq!(decoder.byte_len(), Some(total));
    }

    /// The header's parse: Navidrome writes seconds as a float (Go's `'G'`
    /// format); anything unusable is ignored.
    #[test]
    fn x_content_duration_parses_seconds() {
        let ms = |value: &str| {
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert("x-content-duration", value.parse().unwrap());
            x_content_duration_ms(&headers)
        };
        assert_eq!(ms("245.5"), Some(245_500));
        assert_eq!(ms("300"), Some(300_000));
        assert_eq!(ms("1.2E+06"), Some(1_200_000_000));
        assert_eq!(ms("0"), None);
        assert_eq!(ms("-3"), None);
        assert_eq!(ms("NaN"), None);
        assert_eq!(ms("soon"), None);
        assert_eq!(
            x_content_duration_ms(&reqwest::header::HeaderMap::new()),
            None
        );
    }

    /// A station is live however its server answers: one with no length and
    /// no ICY headers (the same response shape as a live transcode) stays a
    /// live stream when the caller loaded it as a station.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_station_without_icy_headers_stays_live() {
        let (url, _requests) = spawn_live_transcode(None).await;

        let mut decoder = AudioDecoder::default();
        decoder.set_source_kind(SourceKind::Station);
        decoder.init(&url).await.expect("the station must open");

        assert!(decoder.is_infinite_stream());
        assert_eq!(decoder.delivery(), StreamDelivery::Live);
    }

    /// A station whose server answers with a Content-Length (no ICY
    /// headers, a plain HTTP server or proxy) is still a live stream: read
    /// as a sized file it would end at that length and never reconnect.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_station_whose_server_sends_a_length_stays_live() {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut request = [0u8; 2048];
                    let len = socket.read(&mut request).await.unwrap_or(0);
                    let body = transcode_body();
                    let headers = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\n\
                         Accept-Ranges: bytes\r\nContent-Length: {}\r\n\
                         Connection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = socket.write_all(headers.as_bytes()).await;
                    if request[..len].starts_with(b"GET") {
                        let _ = socket.write_all(body).await;
                    }
                    let _ = socket.shutdown().await;
                });
            }
        });
        let url = format!("http://{addr}/stream.mp3");

        let mut decoder = AudioDecoder::default();
        decoder.set_source_kind(SourceKind::Station);
        decoder.init(&url).await.expect("the station must open");

        assert_eq!(decoder.delivery(), StreamDelivery::Live);
    }

    /// ICY headers make a stream live even when the caller loaded it as a
    /// track: radio detection by header is unchanged.
    #[tokio::test(flavor = "multi_thread")]
    async fn icy_headers_make_a_track_load_live() {
        let url = spawn_slow_start_station(AfterAudio::HangUp).await;

        let mut decoder = AudioDecoder::default();
        decoder.set_source_kind(SourceKind::Track);
        decoder.init(&url).await.expect("the station must open");

        assert_eq!(decoder.delivery(), StreamDelivery::Live);
    }

    // =========================================================================
    // Seek accuracy: the audio after a seek starts where the clock says
    // =========================================================================

    /// Three seconds of digital silence, then three of white noise (onset at
    /// exactly 3.0 s), mono 44.1 kHz LAME VBR with a Xing header. The halves
    /// code at very different bitrates, which is what a constant-bitrate
    /// byte estimate gets wrong.
    const VBR_SILENCE_THEN_NOISE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/testdata/vbr_silence_then_noise.mp3"
    );

    /// Seek `path` to `target_ms`, decode up to four seconds and return the
    /// milliseconds from the first decoded frame to the first loud sample
    /// (the noise onset), or `None` when none was reached.
    async fn ms_to_onset_after_seek(path: &str, target_ms: u64) -> Option<u64> {
        let mut decoder = AudioDecoder::default();
        decoder.init(path).await.expect("the fixture opens");
        assert!(decoder.seek(target_ms), "the seek is accepted");
        let rate = u64::from(decoder.format().sample_rate());
        let channels = u64::from(decoder.format().channel_count());

        let mut samples: Vec<f32> = Vec::new();
        while (samples.len() as u64) < 4 * rate * channels && !decoder.is_eof() {
            let buffer = tokio::task::block_in_place(|| decoder.read_buffer(64 * 1024));
            if !buffer.is_valid() {
                break;
            }
            samples.extend(
                buffer
                    .data()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| f32::from_le_bytes(*b)),
            );
        }
        let onset = samples.iter().position(|s| s.abs() > 0.05)? as u64;
        Some(onset / channels * 1000 / rate)
    }

    /// An MP3 carries no timestamps, and Symphonia's coarse seek places the
    /// target by spreading the byte length evenly over the duration, then
    /// reports that estimate as where it landed. On a VBR file it lands
    /// seconds off (a 70 s seek in a VBR album track played 73.7 s while
    /// the clock and the word-timed lyrics said 70 s). After `seek(2000)`
    /// the noise onset at 3.0 s must be heard one second in.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_vbr_mp3_seek_lands_on_its_target() {
        let onset_ms = ms_to_onset_after_seek(VBR_SILENCE_THEN_NOISE, 2_000)
            .await
            .expect("the noise half is reached");
        assert!(
            onset_ms.abs_diff(1_000) <= 10,
            "the 3.0 s onset played {onset_ms} ms after a seek to 2.0 s (expected 1000 ms)"
        );
    }

    /// The preroll converts the track's time base into decoded frames: an MP4
    /// track can tick in other units than its sample rate.
    #[test]
    fn seek_preroll_converts_the_track_time_base_to_frames() {
        use symphonia::core::units::Timestamp;
        let seeked = |actual: i64, required: i64| SeekedTo {
            track_id: 0,
            required_ts: Timestamp::new(required),
            actual_ts: Timestamp::new(actual),
        };
        let millis = TimeBase::try_new(1, 1_000);
        let per_frame = TimeBase::try_from_recip(44_100);

        // Landed 10 ms early on a 1/1000 s timeline, decoding at 44.1 kHz.
        assert_eq!(
            seek_preroll_frames(&seeked(2_000, 2_010), millis, 44_100),
            441
        );
        // A 1/sample-rate timeline counts frames directly.
        assert_eq!(
            seek_preroll_frames(&seeked(1_000, 2_152), per_frame, 44_100),
            1_152
        );
        // On or past the target, or with no time base: nothing to drop.
        assert_eq!(
            seek_preroll_frames(&seeked(2_010, 2_000), millis, 44_100),
            0
        );
        assert_eq!(seek_preroll_frames(&seeked(2_000, 2_010), None, 44_100), 0);
        // Encoder delay (negative timestamps) is the gapless decoder's to drop.
        assert_eq!(
            seek_preroll_frames(&seeked(-1_105, 0), per_frame, 44_100),
            0
        );
        assert_eq!(
            seek_preroll_frames(&seeked(-1_105, 47), per_frame, 44_100),
            47
        );
    }

    /// A seek back to the start replays the whole opening: the decoder's
    /// gapless trim already drops the encoder delay (negative timestamps),
    /// so the seek must not drop it a second time.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_vbr_mp3_seek_to_zero_keeps_the_whole_opening() {
        let onset_ms = ms_to_onset_after_seek(VBR_SILENCE_THEN_NOISE, 0)
            .await
            .expect("the noise half is reached");
        assert!(
            onset_ms.abs_diff(3_000) <= 10,
            "the 3.0 s onset played {onset_ms} ms after a seek to 0 (expected 3000 ms)"
        );
    }

    // =========================================================================
    // A requested transcode reopened at `timeOffset` (seeking it)
    // =========================================================================

    /// The clock song's sample rate (the test mixer's, so nothing resamples
    /// it on the way out) and its length.
    pub(crate) const CLOCK_RATE: u64 = 48_000;
    pub(crate) const CLOCK_SECS: u64 = 10;

    /// The clock song from `offset_secs` on, as a 16-bit stereo WAV: both
    /// samples of every frame hold the song time it plays at, in whole
    /// milliseconds, so one decoded sample says where in the song it is
    /// ([`clock_ms_of`]) and so which offset's answer was decoded. The WAV
    /// header names the length of the part it holds, as the transcode of
    /// that part would.
    pub(crate) fn clock_wav_from(offset_secs: u64) -> Vec<u8> {
        clock_wav_at(CLOCK_RATE, offset_secs)
    }

    /// [`clock_wav_from`] at sample rate `rate` (the song's own file at
    /// 44.1 kHz where its transcode plays at 48 kHz).
    pub(crate) fn clock_wav_at(rate: u64, offset_secs: u64) -> Vec<u8> {
        let first = offset_secs.min(CLOCK_SECS) * rate;
        let frames = CLOCK_SECS * rate - first;
        let data_len = u32::try_from(frames * 4).unwrap();
        let mut wav = Vec::with_capacity(44 + data_len as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&2_u16.to_le_bytes()); // stereo
        wav.extend_from_slice(&(rate as u32).to_le_bytes());
        wav.extend_from_slice(&(rate as u32 * 4).to_le_bytes());
        wav.extend_from_slice(&4_u16.to_le_bytes()); // block align
        wav.extend_from_slice(&16_u16.to_le_bytes()); // bits per sample
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        for frame in first..CLOCK_SECS * rate {
            let ms = i16::try_from(frame * 1000 / rate).unwrap();
            wav.extend_from_slice(&ms.to_le_bytes());
            wav.extend_from_slice(&ms.to_le_bytes());
        }
        wav
    }

    /// The song time (ms) a decoded clock sample plays at.
    pub(crate) fn clock_ms_of(sample: f32) -> u64 {
        (f64::from(sample) * 32_768.0).round() as u64
    }

    /// The song time (ms) of the next frame `decoder` decodes.
    fn next_clock_ms(decoder: &mut AudioDecoder) -> u64 {
        let buffer = tokio::task::block_in_place(|| decoder.read_buffer(8));
        assert!(buffer.is_valid(), "the stream decodes");
        let first = f32::from_ne_bytes(buffer.data()[..4].try_into().unwrap());
        clock_ms_of(first)
    }

    impl LoggedRequest {
        /// The `timeOffset` the request asked for, if any.
        pub(crate) fn time_offset(&self) -> Option<String> {
            self.target
                .split_once('?')?
                .1
                .split('&')
                .find_map(|param| param.strip_prefix("timeOffset="))
                .map(str::to_owned)
        }

        /// The value of query param `key`.
        pub(crate) fn param(&self, key: &str) -> Option<String> {
            self.target
                .split_once('?')?
                .1
                .split('&')
                .find_map(|param| param.strip_prefix(&format!("{key}=")))
                .map(str::to_owned)
        }
    }

    /// How [`spawn_clock_server`] answers. The song is [`CLOCK_SECS`] long
    /// and every answer carries `X-Content-Duration` for all of it, as
    /// Navidrome's does.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum ClockReply {
        /// A live transcode that honours `timeOffset`: 200, chunked,
        /// `Accept-Ranges: none`, the song from the offset (any Range
        /// ignored).
        Live,
        /// A transcode the server's cache holds whole for the offset asked
        /// (the offset is part of the cache key): served like a file, a 206
        /// for the Range asked, its total that entry's own length.
        CachedAtOffset,
        /// The song's own file, which a request that resolves to it is sent
        /// whatever its offset: a 206 for the Range asked, from byte 0.
        Raw,
        /// [`Self::Raw`], the file being 44.1 kHz where the transcode is
        /// 48 kHz.
        Raw44k,
        /// Past the song's end: the headers-only Ogg Navidrome sends for an
        /// Opus transcode (`Content-Length: 266`, `Accept-Ranges: none`).
        /// Only a request with an offset; any other is answered `Live`.
        PastEndOgg,
        /// The same headers-only Ogg sent chunked, with no Content-Length (a
        /// proxy on the way re-chunked it). Only a request with an offset.
        PastEndOggChunked,
        /// Past the song's end, an MP3 transcode's way: a 200 with
        /// `Content-Length: 0`. Only a request with an offset.
        PastEndEmpty,
        /// A 416 for the range asked (an empty cache entry). Only a request
        /// with an offset.
        Unsatisfiable,
        /// The server's transcode limit: 429 with an envelope. Only a
        /// request with an offset.
        TooBusy,
        /// No answer at all: the request is read and the socket held open.
        /// Only a request with an offset.
        Stall,
    }

    impl ClockReply {
        /// Whether this reply is only ever given to a request carrying an
        /// offset (a first open from the song's start is answered `Live`).
        fn needs_an_offset(self) -> bool {
            match self {
                Self::PastEndOgg
                | Self::PastEndOggChunked
                | Self::PastEndEmpty
                | Self::Unsatisfiable
                | Self::TooBusy
                | Self::Stall => true,
                Self::Live | Self::CachedAtOffset | Self::Raw | Self::Raw44k => false,
            }
        }
    }

    /// What a [`ClockServer`] does, switchable between requests.
    #[derive(Debug, Clone, Copy)]
    struct ClockState {
        reply: ClockReply,
        /// Held before the answer's headers go out.
        delay: std::time::Duration,
        /// `X-Content-Duration`, in whole seconds ([`CLOCK_SECS`] unless a
        /// test says otherwise).
        header_secs: u64,
        /// A `Live` answer's body is cut (the connection closed with no
        /// final chunk) after this many bytes of the WAV.
        cut_after: Option<usize>,
        /// How many more `Live` answers are cut; `None` cuts every one.
        cuts_left: Option<u32>,
    }

    /// A Navidrome-shaped server for the clock song: it reads `timeOffset`,
    /// logs every request (method, path + query, Range) and answers as its
    /// [`ClockReply`] says, which a test can switch between requests.
    pub(crate) struct ClockServer {
        pub(crate) base: String,
        log: Arc<parking_lot::Mutex<Vec<LoggedRequest>>>,
        state: Arc<parking_lot::Mutex<ClockState>>,
    }

    impl ClockServer {
        /// Answer every request from now on as `reply` says.
        pub(crate) fn set_reply(&self, reply: ClockReply) {
            self.state.lock().reply = reply;
        }

        /// Hold every answer's headers back for `delay` from now on.
        pub(crate) fn set_answer_delay(&self, delay: std::time::Duration) {
            self.state.lock().delay = delay;
        }

        /// Send `X-Content-Duration: <secs>` from now on (a server whose idea
        /// of the song's length is off).
        pub(crate) fn set_header_secs(&self, secs: u64) {
            self.state.lock().header_secs = secs;
        }

        /// Every request read so far, in order.
        pub(crate) fn requests(&self) -> Vec<LoggedRequest> {
            self.log.lock().clone()
        }

        /// The stream URL nokkvi builds for queue song `id` when it asks for
        /// an Opus 128 transcode (no offset: the track's own URL).
        pub(crate) fn song_url(&self, id: &str) -> String {
            format!(
                "{}/rest/stream?id={id}&format=opus&maxBitRate=128&u=nk&t=token&s=salt&f=json\
                 &v=1.8.0&c=nokkvi&_=1",
                self.base
            )
        }
    }

    /// The headers-only Ogg (OpusHead + OpusTags, no audio) Navidrome 0.64.2
    /// answered `timeOffset=400` on a 360 s song with, as captured.
    pub(crate) const OPUS_HEADERS_ONLY: &[u8] =
        include_bytes!("../../testdata/opus_headers_only_past_end.ogg");

    /// `body` in HTTP/1.1 chunked encoding, in 16 KB chunks; with
    /// `terminated` false the final zero chunk is left out (the transfer was
    /// cut).
    fn chunked(body: &[u8], terminated: bool) -> Vec<u8> {
        let mut out = Vec::new();
        for chunk in body.chunks(16 * 1024) {
            out.extend_from_slice(format!("{:x}\r\n", chunk.len()).as_bytes());
            out.extend_from_slice(chunk);
            out.extend_from_slice(b"\r\n");
        }
        if terminated {
            out.extend_from_slice(b"0\r\n\r\n");
        }
        out
    }

    /// Start a [`ClockServer`] answering as `reply` says.
    pub(crate) async fn spawn_clock_server(reply: ClockReply) -> ClockServer {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };

        let log = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let state = Arc::new(parking_lot::Mutex::new(ClockState {
            reply,
            delay: std::time::Duration::ZERO,
            header_secs: CLOCK_SECS,
            cut_after: None,
            cuts_left: None,
        }));
        let (logged, shared) = (log.clone(), state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let logged = logged.clone();
                let shared = shared.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let len = socket.read(&mut buf).await.unwrap_or(0);
                    let text = String::from_utf8_lossy(&buf[..len]).into_owned();
                    let mut first = text.lines().next().unwrap_or("").split_whitespace();
                    let request = LoggedRequest {
                        method: first.next().unwrap_or("").to_owned(),
                        target: first.next().unwrap_or("").to_owned(),
                        range: text.lines().find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("range")
                                .then(|| value.trim().to_owned())
                        }),
                    };
                    let offset: u64 = request
                        .time_offset()
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(0);
                    // Read the state and take a cut together with logging
                    // the request, so a test sees both at once.
                    let (reply, delay, header_secs, cut) = {
                        let mut state = shared.lock();
                        let reply = if state.reply.needs_an_offset() && offset == 0 {
                            ClockReply::Live
                        } else {
                            state.reply
                        };
                        let cut = match (reply, state.cut_after, state.cuts_left) {
                            (ClockReply::Live, Some(bytes), None) => Some(bytes),
                            (ClockReply::Live, Some(bytes), Some(left)) if left > 0 => {
                                state.cuts_left = Some(left - 1);
                                Some(bytes)
                            }
                            _ => None,
                        };
                        logged.lock().push(request.clone());
                        (reply, state.delay, state.header_secs, cut)
                    };
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                    let duration = format!("X-Content-Duration: {header_secs}\r\n");
                    let (status, headers, payload): (&str, String, Vec<u8>) = match reply {
                        ClockReply::Live => {
                            let wav = clock_wav_from(offset);
                            let body = match cut {
                                Some(bytes) => chunked(&wav[..bytes.min(wav.len())], false),
                                None => chunked(&wav, true),
                            };
                            (
                                "200 OK",
                                format!(
                                    "Content-Type: audio/wav\r\nAccept-Ranges: none\r\n{duration}\
                                     Transfer-Encoding: chunked\r\n"
                                ),
                                body,
                            )
                        }
                        ClockReply::CachedAtOffset | ClockReply::Raw | ClockReply::Raw44k => {
                            let body = match reply {
                                ClockReply::Raw => clock_wav_from(0),
                                ClockReply::Raw44k => clock_wav_at(44_100, 0),
                                _ => clock_wav_from(offset),
                            };
                            let last = body.len() - 1;
                            let (start, end) = request
                                .range
                                .as_deref()
                                .and_then(|r| r.strip_prefix("bytes="))
                                .and_then(|r| r.split_once('-'))
                                .map_or((0, last), |(start, end)| {
                                    (
                                        start.parse().unwrap(),
                                        end.parse::<usize>().map_or(last, |end| end.min(last)),
                                    )
                                });
                            (
                                "206 Partial Content",
                                format!(
                                    "Content-Type: audio/wav\r\nAccept-Ranges: bytes\r\n{duration}\
                                     Content-Range: bytes {start}-{end}/{}\r\n\
                                     Content-Length: {}\r\n",
                                    body.len(),
                                    end - start + 1
                                ),
                                body[start..=end].to_vec(),
                            )
                        }
                        ClockReply::PastEndOgg => (
                            "200 OK",
                            format!(
                                "Content-Type: audio/ogg\r\nAccept-Ranges: none\r\n{duration}\
                                 Content-Length: {}\r\n",
                                OPUS_HEADERS_ONLY.len()
                            ),
                            OPUS_HEADERS_ONLY.to_vec(),
                        ),
                        ClockReply::PastEndOggChunked => (
                            "200 OK",
                            format!(
                                "Content-Type: audio/ogg\r\nAccept-Ranges: none\r\n{duration}\
                                 Transfer-Encoding: chunked\r\n"
                            ),
                            chunked(OPUS_HEADERS_ONLY, true),
                        ),
                        ClockReply::PastEndEmpty => (
                            "200 OK",
                            format!(
                                "Content-Type: audio/mpeg\r\nAccept-Ranges: none\r\n{duration}\
                                 Content-Length: 0\r\n"
                            ),
                            Vec::new(),
                        ),
                        ClockReply::Unsatisfiable => (
                            "416 Range Not Satisfiable",
                            format!("{duration}Content-Length: 0\r\n"),
                            Vec::new(),
                        ),
                        ClockReply::TooBusy => {
                            let envelope = br#"{"subsonic-response":{"status":"failed","version":"1.16.1","type":"navidrome","error":{"code":0,"message":"too many concurrent transcodes"}}}"#;
                            (
                                "429 Too Many Requests",
                                format!(
                                    "Content-Type: application/json\r\nContent-Length: {}\r\n",
                                    envelope.len()
                                ),
                                envelope.to_vec(),
                            )
                        }
                        ClockReply::Stall => {
                            // Hold the connection open, answering nothing.
                            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                            return;
                        }
                    };
                    let head = format!("HTTP/1.1 {status}\r\n{headers}Connection: close\r\n\r\n");
                    let _ = socket.write_all(head.as_bytes()).await;
                    if request.method != "HEAD" {
                        let _ = socket.write_all(&payload).await;
                    }
                    let _ = socket.shutdown().await;
                });
            }
        });
        ClockServer {
            base: format!("http://{addr}"),
            log,
            state,
        }
    }

    /// A stream opened at an offset asks the server for that whole second
    /// (`timeOffset=7`, with the track's own params) in its one request, and
    /// the milliseconds past it are decoded and dropped through the seek
    /// preroll, so the first frame heard is the target's. Its length is the
    /// whole song, never what is left of it, and the URL it reports is the
    /// track's own.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_stream_opened_at_an_offset_starts_there_to_the_millisecond() {
        let server = spawn_clock_server(ClockReply::Live).await;
        let url = server.song_url("s1");

        let mut decoder = AudioDecoder::default();
        decoder
            .open_at(&url, 7)
            .await
            .expect("the transcode opens at 7 s");
        decoder.skip_ms(300);

        let requests = server.requests();
        assert_eq!(requests.len(), 1, "{requests:?}");
        assert_eq!(requests[0].method, "GET");
        assert_eq!(
            requests[0].time_offset().as_deref(),
            Some("7"),
            "{requests:?}"
        );
        assert_eq!(requests[0].param("format").as_deref(), Some("opus"));
        assert_eq!(requests[0].param("maxBitRate").as_deref(), Some("128"));
        assert_eq!(
            decoder.url(),
            url,
            "the track's own URL, without the offset"
        );
        assert_eq!(decoder.offset_secs(), 7);
        assert_eq!(decoder.delivery(), StreamDelivery::Unsized);
        assert_eq!(decoder.duration(), CLOCK_SECS * 1000, "the whole song");
        assert_eq!(next_clock_ms(&mut decoder), 7_300);
    }

    /// A transcode the server's cache holds whole for that offset comes back
    /// sized, at that entry's own length. It still starts at the offset.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_cached_entry_at_an_offset_starts_at_the_offset() {
        let server = spawn_clock_server(ClockReply::CachedAtOffset).await;

        let mut decoder = AudioDecoder::default();
        decoder
            .open_at(&server.song_url("s1"), 4)
            .await
            .expect("the cached entry opens");

        assert_eq!(decoder.delivery(), StreamDelivery::Sized);
        assert_eq!(decoder.byte_len(), Some(clock_wav_from(4).len() as u64));
        assert_eq!(decoder.offset_secs(), 4);
        assert_eq!(decoder.duration(), CLOCK_SECS * 1000, "the whole song");
        assert_eq!(next_clock_ms(&mut decoder), 4_000);
        assert!(
            server
                .requests()
                .iter()
                .all(|r| r.time_offset().as_deref() == Some("4")),
            "every read of the entry asks for its offset: {:?}",
            server.requests()
        );
    }

    /// A request that resolves to the song's own file is answered with the
    /// whole file whatever its offset. The stream then starts at the song's
    /// start, not at the offset, and seeks within itself.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_answer_that_ignored_the_offset_reads_from_the_songs_start() {
        let server = spawn_clock_server(ClockReply::Raw).await;

        let mut decoder = AudioDecoder::default();
        decoder
            .open_at(&server.song_url("s1"), 6)
            .await
            .expect("the file opens");

        assert_eq!(decoder.delivery(), StreamDelivery::Sized);
        assert_eq!(
            decoder.offset_secs(),
            0,
            "the file starts at the song's start"
        );
        assert_eq!(decoder.byte_len(), Some(clock_wav_from(0).len() as u64));
        assert_eq!(next_clock_ms(&mut decoder), 0);
    }

    /// Past the song's end Navidrome answers with an Ogg holding only its
    /// headers (the probe finds no audio in it), or an empty body: the song
    /// is over. The open says so ([`StreamPastEnd`]) instead of failing as a
    /// transcode the server couldn't make; from the song's start the same
    /// empty answer still is that failure.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_answer_past_the_songs_end_reads_as_its_end() {
        for reply in [ClockReply::PastEndOgg, ClockReply::PastEndEmpty] {
            let server = spawn_clock_server(reply).await;

            let mut decoder = AudioDecoder::default();
            let error = decoder
                .open_at(&server.song_url("s1"), 9)
                .await
                .expect_err("no audio past the end");

            assert!(error.is::<StreamPastEnd>(), "{reply:?}: {error:#}");
            assert!(!decoder.is_initialized(), "{reply:?}");
            assert_eq!(decoder.offset_secs(), 0, "{reply:?}");
        }
    }

    /// `init` on a decoder open at an offset opens the track again from its
    /// start: the URL is the same, but the stream is not.
    #[tokio::test(flavor = "multi_thread")]
    async fn init_after_an_offset_open_starts_the_track_from_the_top() {
        let server = spawn_clock_server(ClockReply::Live).await;
        let url = server.song_url("s1");
        let mut decoder = AudioDecoder::default();
        decoder.open_at(&url, 5).await.expect("opens at 5 s");

        decoder.init(&url).await.expect("opens from the start");

        let requests = server.requests();
        assert_eq!(requests.len(), 2, "{requests:?}");
        assert_eq!(requests[1].time_offset(), None, "{requests:?}");
        assert_eq!(decoder.offset_secs(), 0);
        assert_eq!(next_clock_ms(&mut decoder), 0);
    }

    // =========================================================================
    // Telling the song's file from a cache entry, and an empty answer
    // =========================================================================

    /// A sized answer to a request at an offset is the song's own file when
    /// its length is exactly the song's size on disk (Navidrome serves the
    /// file at that size, which `Song.size` holds), and a cache entry for the
    /// offset at any other length, whatever the probe makes of it. Only
    /// without a size does the probed length decide, and it can't tell
    /// without a whole-song length or a probed one.
    #[test]
    fn a_sized_answer_is_the_file_when_it_is_exactly_the_songs_size() {
        // (answer length, song size, probed ms, whole song ms, offset s,
        //  is the file, why)
        type Row = (
            Option<u64>,
            Option<u64>,
            u64,
            Option<u64>,
            u64,
            bool,
            &'static str,
        );
        let rows: [Row; 11] = [
            (
                Some(5_000_000),
                Some(5_000_000),
                4_000,
                Some(10_000),
                6,
                true,
                "the song's size, though the probe looks like an entry",
            ),
            (
                Some(3_000_000),
                Some(5_000_000),
                10_000,
                Some(10_000),
                1,
                false,
                "another size, though the probe looks like the file",
            ),
            (
                None,
                Some(5_000_000),
                10_000,
                Some(10_000),
                1,
                false,
                "no answer length to compare with the size",
            ),
            (
                Some(5_000_000),
                Some(0),
                10_000,
                Some(10_000),
                4,
                true,
                "a zero size is no size: the probe decides",
            ),
            (
                Some(1),
                None,
                10_000,
                None,
                4,
                false,
                "no whole-song length: can't tell",
            ),
            (
                Some(1),
                None,
                10_000,
                Some(0),
                4,
                false,
                "a zero whole-song length: can't tell",
            ),
            (
                Some(1),
                None,
                0,
                Some(10_000),
                4,
                false,
                "no probed length: can't tell",
            ),
            (
                Some(1),
                None,
                9_501,
                Some(10_000),
                1,
                true,
                "offset 1: the file probed 0.499 s short",
            ),
            (
                Some(1),
                None,
                10_499,
                Some(10_000),
                1,
                true,
                "offset 1: the file probed 0.499 s long",
            ),
            (
                Some(1),
                None,
                9_499,
                Some(10_000),
                1,
                false,
                "offset 1: the entry probed 0.499 s long",
            ),
            (
                Some(1),
                None,
                8_501,
                Some(10_000),
                1,
                false,
                "offset 1: the entry probed 0.499 s short",
            ),
        ];
        for (answer_len, size, probed_ms, whole_ms, offset, expected, why) in rows {
            assert_eq!(
                sized_answer_is_the_file(answer_len, size, probed_ms, whole_ms, offset),
                expected,
                "{why}"
            );
        }
    }

    /// The song's size decides where the probe would get it wrong: a server
    /// whose `X-Content-Duration` is off makes the file look like an entry,
    /// and an entry look like the file.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_songs_size_tells_the_file_from_an_entry_whatever_the_probe_says() {
        let file_size = clock_wav_from(0).len() as u64;

        let server = spawn_clock_server(ClockReply::Raw).await;
        server.set_header_secs(20);
        let mut decoder = AudioDecoder::default();
        decoder.set_expected_file_size(Some(file_size));
        decoder
            .open_at(&server.song_url("s1"), 6)
            .await
            .expect("the file opens");
        assert_eq!(
            decoder.offset_secs(),
            0,
            "the file starts at the song's start"
        );
        assert_eq!(next_clock_ms(&mut decoder), 0);

        let server = spawn_clock_server(ClockReply::CachedAtOffset).await;
        server.set_header_secs(5);
        let mut decoder = AudioDecoder::default();
        decoder.set_expected_file_size(Some(file_size));
        decoder
            .open_at(&server.song_url("s1"), 1)
            .await
            .expect("the entry opens");
        assert_eq!(decoder.offset_secs(), 1, "the entry starts at its offset");
        assert_eq!(next_clock_ms(&mut decoder), 1_000);
    }

    /// An answer with no audio in it in the middle of the song is no end:
    /// the server failed to transcode from there (ffmpeg wrote nothing, an
    /// empty cache entry). It fails the open, so a seek keeps the old stream
    /// playing, instead of ending the song.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_empty_answer_mid_song_fails_the_open_instead_of_ending_the_song() {
        for reply in [
            ClockReply::PastEndEmpty,
            ClockReply::PastEndOgg,
            ClockReply::PastEndOggChunked,
            ClockReply::Unsatisfiable,
        ] {
            let server = spawn_clock_server(reply).await;
            let mut decoder = AudioDecoder::default();
            let error = decoder
                .open_at(&server.song_url("s1"), 3)
                .await
                .expect_err("no audio from 3 s");

            assert!(!error.is::<StreamPastEnd>(), "{reply:?}: {error:#}");
            assert!(!decoder.is_initialized(), "{reply:?}");
        }
    }

    /// Within the song's last seconds the same answers are its end, however
    /// they arrive: with a length, chunked with none (a proxy on the way),
    /// empty, or a 416. A headers-only answer is told by the bytes read, not
    /// by its `Content-Length`.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_empty_answer_near_the_end_is_the_end_however_it_is_sent() {
        for reply in [
            ClockReply::PastEndEmpty,
            ClockReply::PastEndOgg,
            ClockReply::PastEndOggChunked,
            ClockReply::Unsatisfiable,
        ] {
            let server = spawn_clock_server(reply).await;
            let mut decoder = AudioDecoder::default();
            let error = decoder
                .open_at(&server.song_url("s1"), 9)
                .await
                .expect_err("no audio from 9 s");

            assert!(error.is::<StreamPastEnd>(), "{reply:?}: {error:#}");
        }
    }

    /// A request at an offset answered with the song's own file goes on
    /// reading that file without the offset: the server answers the offset
    /// URL by its own rules later (a cache entry, a live transcode), which
    /// would hand the range reader another stream than the one it reads.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_file_answered_for_an_offset_is_read_on_without_the_offset() {
        for file_size in [None, Some(clock_wav_from(0).len() as u64)] {
            let server = spawn_clock_server(ClockReply::Raw).await;
            let mut decoder = AudioDecoder::default();
            decoder.set_expected_file_size(file_size);
            decoder
                .open_at(&server.song_url("s1"), 6)
                .await
                .expect("the file opens");
            let opened = server.requests().len();
            assert!(
                tokio::task::block_in_place(|| decoder.seek(9_000)),
                "{file_size:?}: the file seeks within itself"
            );
            assert_eq!(next_clock_ms(&mut decoder), 9_000, "{file_size:?}");

            // Known by its size, the file is read from its own URL from the
            // first range on; told by the probe, from the probe on.
            let skip = if file_size.is_some() { 1 } else { opened };
            let later: Vec<_> = server.requests().into_iter().skip(skip).collect();
            assert!(!later.is_empty(), "{file_size:?}: a later range was read");
            assert!(
                later.iter().all(|r| r.time_offset().is_none()),
                "{file_size:?}: later reads ask for the file: {later:?}"
            );
        }
    }

    /// The server's transcode limit and a server that doesn't answer are
    /// told apart from other failures, so a seek can say which it was.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_busy_server_and_no_answer_are_told_apart() {
        let server = spawn_clock_server(ClockReply::TooBusy).await;
        let mut decoder = AudioDecoder::default();
        let busy = decoder
            .open_at(&server.song_url("s1"), 4)
            .await
            .expect_err("busy");
        assert!(error_is::<ServerBusy>(&busy), "{busy:#}");
        assert!(!error_is::<NoAnswer>(&busy), "{busy:#}");

        let server = spawn_clock_server(ClockReply::Stall).await;
        let mut decoder = AudioDecoder::default();
        decoder.set_answer_timeout(Some(std::time::Duration::from_millis(300)));
        let started = std::time::Instant::now();
        let silent = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            decoder.open_at(&server.song_url("s1"), 4),
        )
        .await
        .expect("the open gives up within its own time")
        .expect_err("no answer");
        assert!(error_is::<NoAnswer>(&silent), "{silent:#}");
        assert!(!error_is::<ServerBusy>(&silent), "{silent:#}");
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }
}
