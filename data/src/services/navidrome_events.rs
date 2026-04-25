//! Navidrome SSE Event Types and Parser
//!
//! Parses the text/event-stream payload from Navidrome's /api/events endpoint.

use std::collections::HashMap;

use serde::Deserialize;

/// Events emitted by Navidrome's /api/events SSE endpoint
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavidromeEvent {
    /// Library scan completed with changes — UI should reload.
    ///
    /// `resources` maps resource type (e.g. `"album"`, `"artist"`, `"playlist"`)
    /// to the IDs that changed. `is_wildcard` is `true` when the server signals
    /// a full-library change (payload `{"*": "*"}`), in which case `resources`
    /// is empty and consumers should treat every cached resource as suspect.
    RefreshResource {
        resources: HashMap<String, Vec<String>>,
        is_wildcard: bool,
    },
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
        "refreshResource" => parse_refresh_resource(data),
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

fn parse_refresh_resource(data: &str) -> NavidromeEvent {
    if data.trim().is_empty() {
        return NavidromeEvent::RefreshResource {
            resources: HashMap::new(),
            is_wildcard: false,
        };
    }

    let raw: HashMap<String, serde_json::Value> = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(_) => {
            return NavidromeEvent::RefreshResource {
                resources: HashMap::new(),
                is_wildcard: false,
            };
        }
    };

    if raw.contains_key("*") {
        return NavidromeEvent::RefreshResource {
            resources: HashMap::new(),
            is_wildcard: true,
        };
    }

    let mut resources = HashMap::new();
    for (key, value) in raw {
        if let serde_json::Value::Array(arr) = value {
            let ids: Vec<String> = arr
                .into_iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
            if !ids.is_empty() {
                resources.insert(key, ids);
            }
        }
    }

    NavidromeEvent::RefreshResource {
        resources,
        is_wildcard: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_refresh_resource_empty() {
        let event = parse_sse_event("refreshResource", "{}");
        assert_eq!(
            event,
            NavidromeEvent::RefreshResource {
                resources: HashMap::new(),
                is_wildcard: false,
            }
        );
    }

    #[test]
    fn test_parse_refresh_resource_with_albums() {
        let event = parse_sse_event("refreshResource", r#"{"album":["a1","a2"]}"#);
        let NavidromeEvent::RefreshResource {
            resources,
            is_wildcard,
        } = event
        else {
            panic!("expected RefreshResource");
        };
        assert!(!is_wildcard);
        assert_eq!(
            resources.get("album").unwrap(),
            &vec!["a1".to_string(), "a2".to_string()]
        );
    }

    #[test]
    fn test_parse_refresh_resource_wildcard() {
        let event = parse_sse_event("refreshResource", r#"{"*":"*"}"#);
        assert_eq!(
            event,
            NavidromeEvent::RefreshResource {
                resources: HashMap::new(),
                is_wildcard: true,
            }
        );
    }

    #[test]
    fn test_parse_refresh_resource_mixed() {
        let event = parse_sse_event("refreshResource", r#"{"album":["a1"],"artist":["x","y"]}"#);
        let NavidromeEvent::RefreshResource {
            resources,
            is_wildcard,
        } = event
        else {
            panic!("expected RefreshResource");
        };
        assert!(!is_wildcard);
        assert_eq!(resources.get("album").unwrap(), &vec!["a1".to_string()]);
        assert_eq!(
            resources.get("artist").unwrap(),
            &vec!["x".to_string(), "y".to_string()]
        );
    }

    #[test]
    fn test_parse_refresh_resource_malformed() {
        let event = parse_sse_event("refreshResource", "not-json");
        assert_eq!(
            event,
            NavidromeEvent::RefreshResource {
                resources: HashMap::new(),
                is_wildcard: false,
            }
        );
    }

    #[test]
    fn test_parse_refresh_resource_blank_data() {
        let event = parse_sse_event("refreshResource", "");
        assert_eq!(
            event,
            NavidromeEvent::RefreshResource {
                resources: HashMap::new(),
                is_wildcard: false,
            }
        );
    }

    #[test]
    fn test_parse_keep_alive() {
        let event = parse_sse_event("keepAlive", "");
        assert_eq!(event, NavidromeEvent::KeepAlive);
    }

    #[test]
    fn test_parse_scan_status() {
        let event = parse_sse_event("scanStatus", r#"{"scanning":true,"count":1337}"#);
        assert_eq!(
            event,
            NavidromeEvent::ScanStatus {
                scanning: true,
                count: 1337
            }
        );

        // Missing fields should use default
        let event = parse_sse_event("scanStatus", "{}");
        assert_eq!(
            event,
            NavidromeEvent::ScanStatus {
                scanning: false,
                count: 0
            }
        );
    }

    #[test]
    fn test_parse_unknown() {
        let event = parse_sse_event("newFeature", "{}");
        assert_eq!(event, NavidromeEvent::Unknown("newFeature".to_string()));
    }
}
