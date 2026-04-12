//! Navidrome SSE Event Types and Parser
//!
//! Parses the text/event-stream payload from Navidrome's /api/events endpoint.

use serde::Deserialize;

/// Events emitted by Navidrome's /api/events SSE endpoint
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavidromeEvent {
    /// Library scan completed with changes — UI should reload
    RefreshResource,
    /// Scan progress
    ScanStatus { scanning: bool, count: i64 },
    /// Server heartbeat (every ~15s)
    KeepAlive,
    /// Server restarted
    ServerStart,
    /// Unknown or unparseable event
    Unknown(String),
}

#[derive(Deserialize)]
struct ScanStatusData {
    #[serde(default)]
    scanning: bool,
    #[serde(default)]
    count: i64,
}

/// Parse a complete SSE frame (event type + data line)
pub fn parse_sse_event(event_type: &str, data: &str) -> NavidromeEvent {
    match event_type {
        "refreshResource" => NavidromeEvent::RefreshResource,
        "keepAlive" => NavidromeEvent::KeepAlive,
        "serverStart" => NavidromeEvent::ServerStart,
        "scanStatus" => {
            if let Ok(parsed) = serde_json::from_str::<ScanStatusData>(data) {
                NavidromeEvent::ScanStatus {
                    scanning: parsed.scanning,
                    count: parsed.count,
                }
            } else {
                NavidromeEvent::Unknown(event_type.to_string())
            }
        }
        _ => NavidromeEvent::Unknown(event_type.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_refresh_resource() {
        let event = parse_sse_event("refreshResource", "{}");
        assert_eq!(event, NavidromeEvent::RefreshResource);
    }

    #[test]
    fn test_parse_keep_alive() {
        let event = parse_sse_event("keepAlive", "");
        assert_eq!(event, NavidromeEvent::KeepAlive);
    }

    #[test]
    fn test_parse_scan_status() {
        let event = parse_sse_event("scanStatus", r#"{"scanning":true,"count":1337}"#);
        assert_eq!(event, NavidromeEvent::ScanStatus { scanning: true, count: 1337 });

        // Missing fields should use default
        let event = parse_sse_event("scanStatus", "{}");
        assert_eq!(event, NavidromeEvent::ScanStatus { scanning: false, count: 0 });
    }

    #[test]
    fn test_parse_unknown() {
        let event = parse_sse_event("newFeature", "{}");
        assert_eq!(event, NavidromeEvent::Unknown("newFeature".to_string()));
    }
}
