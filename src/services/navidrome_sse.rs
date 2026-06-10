//! Iced subscription for Navidrome SSE (/api/events)
//!
//! Connects to Navidrome's server-sent events stream to receive real-time updates.
//! When a `refreshResource` event is received, emits `SseEvent::LibraryChanged`
//! to trigger a transparent background UI refresh.

use std::{collections::HashSet, sync::OnceLock, time::Duration};

use futures::StreamExt;
use iced::task::{Never, Sipper, sipper};
use nokkvi_data::{
    backend::auth::AuthGateway,
    services::navidrome_events::{NavidromeEvent, parse_sse_event},
};
use parking_lot::Mutex as ParkingMutex;
use tracing::{debug, error, info, trace, warn};

/// Connection parameters for the SSE stream
#[derive(Clone)]
pub(crate) struct SseConnectionInfo {
    pub server_url: String,
    pub auth_gateway: AuthGateway,
}

/// Connection slot for the SSE event loop. A synchronous `parking_lot::Mutex`
/// (mirroring `subscription_slot.rs`) so `register` / `clear` always succeed —
/// the previous `OnceLock<tokio::Mutex>` + `try_lock` shape could silently drop
/// a mutation under contention with `run()`'s blocking lock, leaving stale
/// `auth_gateway` + `server_url` in the slot after a logout (audit Finding 13).
static SSE_CONNECTION_INFO: ParkingMutex<Option<SseConnectionInfo>> = ParkingMutex::new(None);

/// Tracks SSE event names already logged at debug for this session. Navidrome emits
/// recurring unknown events (e.g. `nowPlayingCount`); gating prevents log spam while
/// still surfacing the first occurrence of any new event type as a useful diagnostic.
static SEEN_UNKNOWN_SSE_EVENTS: OnceLock<ParkingMutex<HashSet<String>>> = OnceLock::new();

/// Register connection details. Called once from handle_login_result.
pub(crate) fn register(info: SseConnectionInfo) {
    *SSE_CONNECTION_INFO.lock() = Some(info);
    debug!(" [SSE] Connection info registered");
}

/// Drop registered connection info. Called from `reset_session_state` on
/// logout / session-expired. Without this, the SSE event loop keeps reusing
/// the stale `auth_gateway` + `server_url` and retries forever against the
/// old server (401 against a re-pointed server, indefinite reconnect against
/// an unreachable one) until the next successful `register()` overwrites
/// the slot.
pub(crate) fn clear() {
    *SSE_CONNECTION_INFO.lock() = None;
    debug!(" [SSE] Connection info cleared");
}

/// Test-only helper: reports whether the SSE connection slot currently holds
/// connection info. Mirrors `subscription_slot.rs::slot_is_set`; used by the
/// regression tests that pin the logout-clears-the-slot contract.
#[cfg(test)]
pub(crate) fn slot_is_set() -> bool {
    SSE_CONNECTION_INFO.lock().is_some()
}

/// Test-only helper: grab the connection-slot lock and hold it for `dur`,
/// then release. The static is module-private, so the contention regression
/// test in `update::tests::session` drives the lock through this rather than
/// reaching into the static directly. Under the old `try_lock` shape a
/// `register` racing this hold would have silently dropped; the blocking
/// `parking_lot::Mutex` makes the register wait and complete.
#[cfg(test)]
pub(crate) fn hold_slot_lock_blocking(dur: Duration) {
    let _guard = SSE_CONNECTION_INFO.lock();
    std::thread::sleep(dur);
}

/// Structured payload carrying the resource kinds Navidrome reports as changed.
///
/// Built by the SSE consumer from `NavidromeEvent::RefreshResource`'s raw
/// `HashMap<String, Vec<String>>` so the update layer can branch on which
/// entity caches actually need a reload. Navidrome currently emits `album`,
/// `artist`, `song`, `library`, `user`, and `plugin` resource kinds; the
/// `playlist` and `genre` fields are populated forward-compat if/when the
/// server starts emitting those keys (the parser already extracts them).
///
/// `is_wildcard = true` corresponds to the `{"*": "*"}` payload (full scan).
/// In that case every kind is considered changed and all id vectors are empty.
/// Per `gotchas.md`, consumers must still skip per-album artwork eviction on
/// wildcards to avoid a mass re-download of every cached cover.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LibraryChange {
    pub album_ids: Vec<String>,
    pub artist_ids: Vec<String>,
    pub song_ids: Vec<String>,
    pub playlist_ids: Vec<String>,
    pub genre_ids: Vec<String>,
    pub is_wildcard: bool,
}

/// Events yielded to the iced runloop
#[derive(Debug, Clone)]
pub(crate) enum SseEvent {
    /// Library scan emitted a refreshResource event.
    ///
    /// The payload enumerates which resource kinds the server reports as
    /// changed; `handle_library_changed` branches on each `*_ids` / `is_wildcard`
    /// to reload only the caches that received SSE notifications. Wildcard
    /// (full-scan) payloads still reload slot lists but skip per-album
    /// artwork eviction — see `LibraryChange` docs.
    LibraryChanged(LibraryChange),
}

/// Append a raw SSE byte chunk to `byte_buffer`, decoding as much of the
/// accumulated bytes as possible into `text_buffer` and carrying forward any
/// trailing partial UTF-8 codepoint.
///
/// SSE chunks arriving from the HTTP stream can split a multi-byte UTF-8
/// codepoint at the chunk boundary (e.g. `é` = `0xC3 0xA9`, where `0xC3`
/// arrives in one chunk and `0xA9` in the next). The naive
/// `std::str::from_utf8(&bytes)` decode then returns `Err` and the entire
/// chunk is silently dropped, taking the rest of that SSE event with it.
///
/// This helper appends the chunk to `byte_buffer`, then uses
/// `std::str::Utf8Error::valid_up_to()` to find the largest valid UTF-8
/// prefix and pushes that prefix into `text_buffer`. Trailing bytes that
/// form a partial codepoint stay in `byte_buffer` for the next call.
/// Genuinely invalid byte sequences (signaled by `error_len() = Some(n)`)
/// are dropped with a `warn!` so the stream can resynchronize.
///
/// Returns the number of bytes that were drained from `byte_buffer` on this
/// call (valid prefix length + any dropped invalid bytes) — useful in tests.
fn append_chunk_decoded(
    byte_buffer: &mut Vec<u8>,
    text_buffer: &mut String,
    chunk: &[u8],
) -> usize {
    byte_buffer.extend_from_slice(chunk);

    let (valid_len, invalid_len_opt) = match std::str::from_utf8(byte_buffer) {
        Ok(s) => (s.len(), None),
        Err(e) => (e.valid_up_to(), e.error_len()),
    };

    if valid_len > 0 {
        let valid_slice = std::str::from_utf8(&byte_buffer[..valid_len])
            .expect("valid_up_to() guarantees this prefix is valid UTF-8");
        text_buffer.push_str(valid_slice);
    }

    let drain_end = match invalid_len_opt {
        Some(invalid_len) => {
            warn!(
                " [SSE] dropping {invalid_len} invalid UTF-8 byte(s) at offset {valid_len} to resync stream"
            );
            valid_len + invalid_len
        }
        None => valid_len,
    };

    byte_buffer.drain(..drain_end);
    drain_end
}

/// Reconnect backoff floor: a fresh attempt waits at least this long.
const SSE_BACKOFF_FLOOR: Duration = Duration::from_secs(2);
/// Reconnect backoff ceiling: escalation saturates here.
const SSE_BACKOFF_CAP: Duration = Duration::from_secs(30);
/// A connection that stayed up at least this long is considered healthy, so
/// its disconnect resets the backoff floor (and incurs no reconnect sleep).
const SSE_HEALTHY_UPTIME: Duration = Duration::from_secs(60);

/// Decide the reconnect backoff after a stream-died disconnect.
///
/// Pure and clock-free (the caller reads `Instant::now()` / `elapsed()` and
/// passes the uptime in), so the escalate / cap / reset policy is unit-testable
/// without I/O.
///
/// - A connection that stayed up `>= SSE_HEALTHY_UPTIME` is healthy: returns
///   `(None, SSE_BACKOFF_FLOOR)` — no sleep, floor reset. This preserves the
///   original "reset backoff on a good connection" intent while defeating the
///   flap-resets-backoff defect (a 200 → EOF → 200 flap never stayed up long
///   enough to count as healthy, so it now escalates).
/// - A short-lived connection (flap) returns `(Some(prev), min(prev*2, CAP))`:
///   sleep `prev`, then escalate `2 → 4 → 8 → 16 → 30`, saturating at 30s.
fn next_backoff(prev: Duration, connection_uptime: Duration) -> (Option<Duration>, Duration) {
    if connection_uptime >= SSE_HEALTHY_UPTIME {
        (None, SSE_BACKOFF_FLOOR)
    } else {
        (Some(prev), escalate_backoff(prev))
    }
}

/// Double the reconnect backoff, saturating at `SSE_BACKOFF_CAP`.
///
/// The single escalation policy shared by all three reconnect paths: the
/// non-2xx response path, the request-error path, and the `next_backoff`
/// flap arm.
fn escalate_backoff(prev: Duration) -> Duration {
    std::cmp::min(prev * 2, SSE_BACKOFF_CAP)
}

/// Start the SSE subscription loop
pub(crate) fn run() -> impl Sipper<Never, SseEvent> {
    sipper(async |mut output| {
        let client = reqwest::Client::builder()
            .user_agent(nokkvi_data::USER_AGENT)
            .timeout(Duration::from_secs(60 * 60 * 24)) // 24 hour total connection timeout
            .build()
            .unwrap_or_default();

        let mut backoff = SSE_BACKOFF_FLOOR;

        loop {
            // 1. Get connection info. The synchronous parking_lot guard is
            //    dropped at the end of this statement — never held across an
            //    `.await`. `.cloned()` (not `.take()`): the loop re-reads the
            //    slot every iteration to pick up JWT refreshes across
            //    reconnects, so the slot must retain the info.
            let info = SSE_CONNECTION_INFO.lock().as_ref().cloned();
            let Some(info) = info else {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            };

            // 2. Read latest JWT (can update across reconnects if redb session is resumed)
            let token = info.auth_gateway.get_token().await;
            if token.is_empty() {
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }

            // 3. Connect to /api/events
            let url = format!("{}/api/events", info.server_url.trim_end_matches('/'));
            debug!(" [SSE] Connecting to {}", url);

            match client
                .get(&url)
                .header("Accept", "text/event-stream")
                .header("X-ND-Authorization", format!("Bearer {token}"))
                .send()
                .await
            {
                Ok(response) => {
                    let status = response.status();
                    if !status.is_success() {
                        warn!(
                            " [SSE] Connection failed with status {}: {:?}",
                            status,
                            response.text().await.unwrap_or_default()
                        );
                        tokio::time::sleep(backoff).await;
                        backoff = escalate_backoff(backoff);
                        continue;
                    }

                    info!(" [SSE] Connected to Navidrome event stream");
                    // Record connection start so the post-disconnect backoff
                    // (`next_backoff`) can tell a healthy connection (reset the
                    // floor, no sleep) from a flap (escalate). The backoff is
                    // NOT reset here unconditionally — a 200 → EOF → 200 flap
                    // would otherwise reset the floor on every short-lived 200.
                    let connected_at = std::time::Instant::now();

                    let mut stream = response.bytes_stream();
                    let mut buffer = String::new();
                    let mut byte_buffer: Vec<u8> = Vec::new();
                    let mut event_type = String::new();
                    let mut data = String::new();

                    // Navidrome sends keepAlive every 15s. If we don't hear from the server for 45s,
                    // consider the connection dead.
                    let read_timeout = Duration::from_secs(45);

                    loop {
                        let chunk_future = stream.next();
                        let result = tokio::time::timeout(read_timeout, chunk_future).await;

                        match result {
                            Ok(Some(Ok(bytes))) => {
                                // Buffered UTF-8 decode handles codepoints
                                // split across HTTP chunks (e.g. `é` =
                                // 0xC3 0xA9 arriving as two chunks).
                                append_chunk_decoded(&mut byte_buffer, &mut buffer, &bytes);

                                // Process complete lines
                                while let Some(pos) = buffer.find('\n') {
                                    let mut line = buffer[..pos].to_string();
                                    buffer.drain(..=pos); // remove line + \n
                                    if line.ends_with('\r') {
                                        line.pop();
                                    }

                                    if let Some(stripped) = line.strip_prefix("event: ") {
                                        event_type = stripped.to_string();
                                    } else if let Some(stripped) = line.strip_prefix("data: ") {
                                        data = stripped.to_string();
                                    } else if line.is_empty() {
                                        // Empty line marks end of frame
                                        if !event_type.is_empty() {
                                            match parse_sse_event(&event_type, &data) {
                                                NavidromeEvent::RefreshResource {
                                                    mut resources,
                                                    is_wildcard,
                                                } => {
                                                    let take = |resources: &mut std::collections::HashMap<String, Vec<String>>, key: &str| {
                                                        resources.remove(key).unwrap_or_default()
                                                    };
                                                    let change = LibraryChange {
                                                        album_ids: take(&mut resources, "album"),
                                                        artist_ids: take(&mut resources, "artist"),
                                                        song_ids: take(&mut resources, "song"),
                                                        playlist_ids: take(
                                                            &mut resources,
                                                            "playlist",
                                                        ),
                                                        genre_ids: take(&mut resources, "genre"),
                                                        is_wildcard,
                                                    };
                                                    debug!(
                                                        " [SSE] refreshResource — wildcard={is_wildcard}, albums={}, artists={}, songs={}, playlists={}, genres={}",
                                                        change.album_ids.len(),
                                                        change.artist_ids.len(),
                                                        change.song_ids.len(),
                                                        change.playlist_ids.len(),
                                                        change.genre_ids.len(),
                                                    );
                                                    output
                                                        .send(SseEvent::LibraryChanged(change))
                                                        .await;
                                                }
                                                NavidromeEvent::ScanStatus { scanning, count } => {
                                                    debug!(
                                                        " [SSE] ScanStatus: scanning={}, count={}",
                                                        scanning, count
                                                    );
                                                }
                                                NavidromeEvent::KeepAlive => {
                                                    // Heartbeat, just keeps loop active
                                                }
                                                NavidromeEvent::ServerStart => {
                                                    info!(" [SSE] Server restarted");
                                                }
                                                NavidromeEvent::Unknown(t) => {
                                                    let seen =
                                                        SEEN_UNKNOWN_SSE_EVENTS.get_or_init(|| {
                                                            ParkingMutex::new(HashSet::new())
                                                        });
                                                    let first_time = seen.lock().insert(t.clone());
                                                    if first_time {
                                                        debug!(
                                                            " [SSE] Unknown event (first occurrence): {}",
                                                            t
                                                        );
                                                    } else {
                                                        trace!(" [SSE] Unknown event: {}", t);
                                                    }
                                                }
                                            }
                                        }
                                        event_type.clear();
                                        data.clear();
                                    }
                                }
                            }
                            Ok(Some(Err(e))) => {
                                error!(" [SSE] Stream read error: {}", e);
                                break;
                            }
                            Ok(None) => {
                                warn!(" [SSE] Stream ended by server");
                                break;
                            }
                            Err(_) => {
                                warn!(
                                    " [SSE] Read timeout (no keepAlive for {}s), reconnecting...",
                                    read_timeout.as_secs()
                                );
                                break;
                            }
                        }
                    }

                    // The inner read loop only `break`s on a stream-died path
                    // (read error / server-ended / read timeout). Throttle the
                    // reconnect: a healthy connection resets the floor and
                    // reconnects immediately; a flap escalates 2 → 4 → … → 30s
                    // so a proxy idle-timeout / restart loop can't spin.
                    let (sleep, new_backoff) = next_backoff(backoff, connected_at.elapsed());
                    backoff = new_backoff;
                    if let Some(d) = sleep {
                        tokio::time::sleep(d).await;
                    }
                }
                Err(e) => {
                    error!(" [SSE] Request failed: {}", e);
                    tokio::time::sleep(backoff).await;
                    backoff = escalate_backoff(backoff);
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pure-ASCII chunks decode wholesale in one call and leave the byte
    /// buffer empty. This is the trivial path that exercises the
    /// "all valid" branch of `from_utf8`.
    #[test]
    fn append_chunk_decoded_pure_ascii_drains_completely() {
        let mut byte_buffer: Vec<u8> = Vec::new();
        let mut text_buffer = String::new();

        let drained = append_chunk_decoded(&mut byte_buffer, &mut text_buffer, b"hello world\n");

        assert_eq!(drained, 12);
        assert_eq!(text_buffer, "hello world\n");
        assert!(
            byte_buffer.is_empty(),
            "byte_buffer should be empty after fully-valid input, got {byte_buffer:?}"
        );
    }

    /// `é` is `0xC3 0xA9`. Splitting the codepoint between two chunks must
    /// NOT drop the surrounding text: the first chunk's valid prefix is
    /// flushed, the trailing `0xC3` is carried forward, and the next chunk's
    /// `0xA9` completes the codepoint. The current buggy implementation
    /// drops the entire first chunk (because the chunk ends mid-codepoint),
    /// then drops the second chunk (continuation bytes without a leader),
    /// so both `assert_eq!`s below should fail in the Red phase.
    #[test]
    fn append_chunk_decoded_carries_partial_codepoint_across_chunks() {
        let mut byte_buffer: Vec<u8> = Vec::new();
        let mut text_buffer = String::new();

        // First chunk: `b"h"` + leading byte of `é`.
        append_chunk_decoded(&mut byte_buffer, &mut text_buffer, b"h\xC3");
        assert_eq!(
            text_buffer, "h",
            "valid ASCII prefix must be flushed before the partial codepoint"
        );
        assert_eq!(
            byte_buffer,
            vec![0xC3],
            "trailing partial-codepoint byte must be carried forward"
        );

        // Second chunk: continuation byte + rest of word.
        append_chunk_decoded(&mut byte_buffer, &mut text_buffer, b"\xA9llo");
        assert_eq!(text_buffer, "héllo");
        assert!(byte_buffer.is_empty());
    }

    /// 3-byte codepoint `日` = `0xE6 0x97 0xA5`. Feeding it one byte at a
    /// time exercises two consecutive carry-forwards: after byte 1 we hold
    /// 1 byte; after byte 2 we hold 2 bytes; only after byte 3 does the
    /// character materialize in the text buffer.
    #[test]
    fn append_chunk_decoded_three_byte_codepoint_one_byte_at_a_time() {
        let mut byte_buffer: Vec<u8> = Vec::new();
        let mut text_buffer = String::new();

        append_chunk_decoded(&mut byte_buffer, &mut text_buffer, &[0xE6]);
        assert_eq!(text_buffer, "");
        assert_eq!(byte_buffer, vec![0xE6]);

        append_chunk_decoded(&mut byte_buffer, &mut text_buffer, &[0x97]);
        assert_eq!(text_buffer, "");
        assert_eq!(byte_buffer, vec![0xE6, 0x97]);

        append_chunk_decoded(&mut byte_buffer, &mut text_buffer, &[0xA5]);
        assert_eq!(text_buffer, "日");
        assert!(byte_buffer.is_empty());
    }

    /// A genuinely invalid byte (stray `0xFF`) inside otherwise-valid text
    /// must be dropped so the stream can resync. Without this, the buffer
    /// would never advance and subsequent valid bytes would pile up behind
    /// the bad byte forever. The valid surrounding text is still preserved.
    #[test]
    fn append_chunk_decoded_drops_invalid_byte_and_advances() {
        let mut byte_buffer: Vec<u8> = Vec::new();
        let mut text_buffer = String::new();

        // `a` (valid) | `0xFF` (invalid) | `b` (valid, follows the bad byte).
        append_chunk_decoded(&mut byte_buffer, &mut text_buffer, b"a\xFFb");

        // First call flushes "a" and drops 0xFF; the trailing "b" then
        // sits in byte_buffer awaiting more bytes — UNLESS the helper
        // re-runs after the drain. We accept either: at minimum, the
        // leading "a" must have been flushed and the 0xFF dropped.
        assert!(
            text_buffer.starts_with("a"),
            "valid prefix before invalid byte must be flushed, got {text_buffer:?}"
        );
        assert!(
            !byte_buffer.contains(&0xFF),
            "0xFF must be dropped, byte_buffer = {byte_buffer:?}"
        );

        // Feed an empty chunk to let the helper keep decoding the leftover
        // valid bytes (covers the case where the drained range left valid
        // bytes still in the buffer).
        append_chunk_decoded(&mut byte_buffer, &mut text_buffer, b"");
        assert_eq!(text_buffer, "ab");
        assert!(byte_buffer.is_empty());
    }

    /// SSE event lines spanning chunks: the line-processing loop in `run()`
    /// finds `\n` only after `buffer` has accumulated the full line. This
    /// test confirms the helper composes correctly: feed the `event:` line
    /// fragmented across two chunks, then look for the completed line in
    /// `text_buffer`. The actual line-parsing loop is unchanged from the
    /// pre-fix code, so this just verifies the buffering substrate it sits
    /// on top of.
    #[test]
    fn append_chunk_decoded_sse_event_line_spans_chunks() {
        let mut byte_buffer: Vec<u8> = Vec::new();
        let mut text_buffer = String::new();

        append_chunk_decoded(&mut byte_buffer, &mut text_buffer, b"event: refr");
        append_chunk_decoded(
            &mut byte_buffer,
            &mut text_buffer,
            b"eshResource\ndata: {\"album\":[\"1\"]}\n\n",
        );

        assert_eq!(
            text_buffer,
            "event: refreshResource\ndata: {\"album\":[\"1\"]}\n\n"
        );
        assert!(byte_buffer.is_empty());

        // Sanity: the existing line-processing logic in `run()` operates by
        // repeatedly searching `\n` in this buffer, so confirm both newlines
        // are now visible to it.
        assert_eq!(text_buffer.matches('\n').count(), 3);
    }

    /// Combined real-world scenario: a `library_changed` SSE event with a
    /// non-ASCII artist name (`Sigur Rós`, where `ó` = `0xC3 0xB3`) split
    /// across two HTTP chunks at the codepoint boundary. The pre-fix code
    /// drops the entire payload; the fixed helper preserves it.
    #[test]
    fn append_chunk_decoded_preserves_non_ascii_payload_across_split() {
        let mut byte_buffer: Vec<u8> = Vec::new();
        let mut text_buffer = String::new();

        // First chunk ends with the leading byte of `ó`.
        append_chunk_decoded(
            &mut byte_buffer,
            &mut text_buffer,
            b"event: refreshResource\ndata: {\"artist\":\"Sigur R\xC3",
        );
        // Second chunk picks up with the continuation byte.
        append_chunk_decoded(&mut byte_buffer, &mut text_buffer, b"\xB3s\"}\n\n");

        assert_eq!(
            text_buffer,
            "event: refreshResource\ndata: {\"artist\":\"Sigur Rós\"}\n\n"
        );
        assert!(byte_buffer.is_empty());
    }

    // ── next_backoff (I17) ──────────────────────────────────────────────
    //
    // A connection that drops immediately (uptime ~0) is a flap: backoff must
    // escalate so a proxy idle-timeout / server-restart loop does not produce a
    // tight reconnect spin. A connection that stayed up past the healthy
    // threshold resets the floor and incurs no reconnect sleep.

    /// An immediate flap (zero uptime) escalates: sleep the previous backoff,
    /// double it for next time.
    #[test]
    fn next_backoff_escalates_on_immediate_flap() {
        assert_eq!(
            next_backoff(Duration::from_secs(2), Duration::from_secs(0)),
            (Some(Duration::from_secs(2)), Duration::from_secs(4)),
        );
    }

    /// Escalation saturates at the 30s cap and stays there.
    #[test]
    fn next_backoff_caps_at_30s() {
        assert_eq!(
            next_backoff(Duration::from_secs(16), Duration::from_secs(0)).1,
            Duration::from_secs(30),
        );
        assert_eq!(
            next_backoff(Duration::from_secs(30), Duration::from_secs(0)).1,
            Duration::from_secs(30),
        );
    }

    /// A healthy connection (uptime past the threshold) resets the floor and
    /// does NOT sleep before reconnecting.
    #[test]
    fn next_backoff_resets_on_healthy_uptime() {
        assert_eq!(
            next_backoff(Duration::from_secs(16), Duration::from_secs(120)),
            (None, Duration::from_secs(2)),
        );
    }

    /// The shared escalation helper doubles the backoff and saturates at the
    /// cap — guards every reconnect path against a half-applied const edit.
    #[test]
    fn escalate_backoff_doubles_then_saturates_at_cap() {
        assert_eq!(escalate_backoff(SSE_BACKOFF_FLOOR), Duration::from_secs(4));
        assert_eq!(escalate_backoff(Duration::from_secs(16)), SSE_BACKOFF_CAP);
        assert_eq!(escalate_backoff(SSE_BACKOFF_CAP), SSE_BACKOFF_CAP);
    }
}
