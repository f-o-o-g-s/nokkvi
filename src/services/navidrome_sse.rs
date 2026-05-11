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
use tokio::sync::Mutex;
use tracing::{debug, error, info, trace, warn};

/// Connection parameters for the SSE stream
#[derive(Clone)]
pub(crate) struct SseConnectionInfo {
    pub server_url: String,
    pub auth_gateway: AuthGateway,
}

static SSE_CONNECTION_INFO: OnceLock<Mutex<Option<SseConnectionInfo>>> = OnceLock::new();

/// Tracks SSE event names already logged at debug for this session. Navidrome emits
/// recurring unknown events (e.g. `nowPlayingCount`); gating prevents log spam while
/// still surfacing the first occurrence of any new event type as a useful diagnostic.
static SEEN_UNKNOWN_SSE_EVENTS: OnceLock<ParkingMutex<HashSet<String>>> = OnceLock::new();

/// Register connection details. Called once from handle_login_result.
pub(crate) fn register(info: SseConnectionInfo) {
    let slot = SSE_CONNECTION_INFO.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = slot.try_lock() {
        *guard = Some(info);
        debug!(" [SSE] Connection info registered");
    }
}

/// Events yielded to the iced runloop
#[derive(Debug, Clone)]
pub(crate) enum SseEvent {
    /// Library scan emitted a refreshResource event.
    ///
    /// `album_ids` carries the IDs whose data/artwork the server says changed.
    /// `is_wildcard` is `true` when the payload was `{"*": "*"}` (full scan)
    /// — consumers should reload slot lists but skip per-album artwork eviction
    /// to avoid mass re-downloads.
    LibraryChanged {
        album_ids: Vec<String>,
        is_wildcard: bool,
    },
}

/// Start the SSE subscription loop
pub(crate) fn run() -> impl Sipper<Never, SseEvent> {
    sipper(async |mut output| {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60 * 60 * 24)) // 24 hour total connection timeout
            .build()
            .unwrap_or_default();

        let mut backoff = Duration::from_secs(2);

        loop {
            // 1. Get connection info
            let info = {
                let slot = SSE_CONNECTION_INFO.get_or_init(|| Mutex::new(None));
                let guard = slot.lock().await;
                match guard.as_ref() {
                    Some(info) => info.clone(),
                    None => {
                        drop(guard);
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                }
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
                        backoff = std::cmp::min(backoff * 2, Duration::from_secs(30));
                        continue;
                    }

                    info!(" [SSE] Connected to Navidrome event stream");
                    backoff = Duration::from_secs(2); // Reset backoff on success

                    let mut stream = response.bytes_stream();
                    let mut buffer = String::new();
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
                                // Accumulate string buffer
                                if let Ok(text) = std::str::from_utf8(&bytes) {
                                    buffer.push_str(text);

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
                                                        resources,
                                                        is_wildcard,
                                                    } => {
                                                        let album_ids = resources
                                                            .get("album")
                                                            .cloned()
                                                            .unwrap_or_default();
                                                        debug!(
                                                            " [SSE] refreshResource — wildcard={is_wildcard}, album_ids={}",
                                                            album_ids.len()
                                                        );
                                                        output
                                                            .send(SseEvent::LibraryChanged {
                                                                album_ids,
                                                                is_wildcard,
                                                            })
                                                            .await;
                                                    }
                                                    NavidromeEvent::ScanStatus {
                                                        scanning,
                                                        count,
                                                    } => {
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
                                                        let seen = SEEN_UNKNOWN_SSE_EVENTS
                                                            .get_or_init(|| {
                                                                ParkingMutex::new(HashSet::new())
                                                            });
                                                        let first_time =
                                                            seen.lock().insert(t.clone());
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
                }
                Err(e) => {
                    error!(" [SSE] Request failed: {}", e);
                    tokio::time::sleep(backoff).await;
                    backoff = std::cmp::min(backoff * 2, Duration::from_secs(30));
                }
            }
        }
    })
}
