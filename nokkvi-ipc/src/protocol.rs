//! Wire-protocol types for the nokkvi IPC channel.
//!
//! These are the on-the-wire shapes exchanged over the Unix domain socket
//! between the long-running `nokkvi` instance (server) and the forked-and-
//! exited `nokkvi <cmd>` client invocations (and any third-party scripts).
//!
//! # Example payloads
//!
//! ```json
//! // Request (client → server)
//! {"protocol_version": 1, "request_id": 42, "command": "ping", "args": null}
//!
//! // Response — success with no body (request_id echoes the request)
//! {"request_id": 42, "data": null}
//!
//! // Response — success with body
//! {"request_id": 42, "data": "pong"}
//!
//! // Response — error
//! {"request_id": 42, "error": {"code": "unknown_command", "message": "..."}}
//!
//! // Event (server → client, unsolicited; no request_id, no protocol_version)
//! {"event": "property-change", "name": "playback-status", "data": "Paused"}
//! ```
//!
//! # Forward-compatibility invariants (locked from Phase 0)
//!
//! 1. `protocol_version` on every request. Major-version mismatch is the
//!    caller's signal to reject the connection or downgrade.
//! 2. `request_id` echoed on every response so callers can multiplex multiple
//!    in-flight requests over a single connection (mpv-style).
//! 3. Tolerant deserialization: neither side uses `#[serde(deny_unknown_fields)]`,
//!    so a peer compiled against a newer protocol revision can still talk to
//!    an older peer — unknown fields are silently dropped.
//! 4. Reserved top-level field names — `protocol_version`, `request_id`,
//!    `command`, `args`, `data`, `error`, `event`, `name`. Anything new is
//!    namespaced under `args` or `data`.

use serde::{Deserialize, Serialize};

/// Wire-protocol major version. Bump only on backward-incompatible changes.
pub const PROTOCOL_VERSION: u8 = 1;

/// Client → server command envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcRequest {
    pub protocol_version: u8,
    pub request_id: u64,
    pub command: String,
    #[serde(default)]
    pub args: serde_json::Value,
}

/// Server → client reply to an `IpcRequest`. Exactly one of `data` / `error`
/// should be populated; both are optional so success-with-no-body is the
/// `{request_id, data: null}` shape rather than requiring a sentinel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcResponse {
    pub request_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<IpcError>,
}

/// Structured error body. `code` is the machine-parseable identifier (stable
/// across releases); `message` is human prose that may evolve freely.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcError {
    pub code: String,
    pub message: String,
}

/// Server → client unsolicited event. No `request_id` because events are
/// broadcast, not correlated to a request (§14B invariant: events live on
/// their own envelope shape).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcEvent {
    pub event: String,
    pub name: String,
    #[serde(default)]
    pub data: serde_json::Value,
}

impl IpcRequest {
    /// Convenience constructor that stamps the current `PROTOCOL_VERSION`.
    pub fn new(request_id: u64, command: impl Into<String>, args: serde_json::Value) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id,
            command: command.into(),
            args,
        }
    }
}

impl IpcResponse {
    /// Build a success response with optional payload.
    pub fn ok(request_id: u64, data: Option<serde_json::Value>) -> Self {
        Self {
            request_id,
            data,
            error: None,
        }
    }

    /// Build an error response.
    pub fn err(request_id: u64, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            request_id,
            data: None,
            error: Some(IpcError {
                code: code.into(),
                message: message.into(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn request_roundtrips_through_json() {
        let req = IpcRequest::new(7, "rate", json!({"delta": "up"}));
        let wire = serde_json::to_string(&req).expect("serialize request");
        let back: IpcRequest = serde_json::from_str(&wire).expect("deserialize request");
        assert_eq!(back.protocol_version, PROTOCOL_VERSION);
        assert_eq!(back.request_id, 7);
        assert_eq!(back.command, "rate");
        assert_eq!(back.args, json!({"delta": "up"}));
    }

    #[test]
    fn response_ok_omits_error_field() {
        let resp = IpcResponse::ok(1, Some(json!("pong")));
        let wire = serde_json::to_string(&resp).expect("serialize response");
        assert!(
            !wire.contains("error"),
            "ok response should not serialize an `error` field: {wire}"
        );
        assert!(
            wire.contains("\"data\""),
            "ok response should keep data: {wire}"
        );
    }

    #[test]
    fn response_err_omits_data_field() {
        let resp = IpcResponse::err(2, "unknown_command", "no such command: bogus");
        let wire = serde_json::to_string(&resp).expect("serialize response");
        assert!(
            !wire.contains("\"data\""),
            "err response should not serialize a `data` field: {wire}"
        );
        assert!(wire.contains("unknown_command"));
    }

    #[test]
    fn request_accepts_unknown_fields_for_forward_compat() {
        // A newer client adds a `trace_id` field. Old server must still parse it.
        let wire = r#"{
            "protocol_version": 1,
            "request_id": 99,
            "command": "ping",
            "args": null,
            "trace_id": "abc-123",
            "future_thing": {"nested": true}
        }"#;
        let req: IpcRequest = serde_json::from_str(wire).expect("forward-compat parse");
        assert_eq!(req.request_id, 99);
        assert_eq!(req.command, "ping");
    }

    #[test]
    fn response_accepts_unknown_fields_for_forward_compat() {
        let wire = r#"{
            "request_id": 5,
            "data": {"title": "Song"},
            "warnings": ["deprecated_field"]
        }"#;
        let resp: IpcResponse = serde_json::from_str(wire).expect("forward-compat parse");
        assert_eq!(resp.request_id, 5);
        assert!(resp.error.is_none());
    }

    #[test]
    fn request_defaults_args_to_null_when_omitted() {
        // Server-side tolerance: a hand-written client that omits `args` should
        // still parse — the empty/null arg shape is the common case for verbs
        // like `ping`, `next`, `quit`.
        let wire = r#"{"protocol_version": 1, "request_id": 1, "command": "ping"}"#;
        let req: IpcRequest = serde_json::from_str(wire).expect("defaulted args parse");
        assert_eq!(req.args, serde_json::Value::Null);
    }

    #[test]
    fn event_roundtrips_without_request_id() {
        let ev = IpcEvent {
            event: "property-change".into(),
            name: "playback-status".into(),
            data: json!("Paused"),
        };
        let wire = serde_json::to_string(&ev).expect("serialize event");
        assert!(
            !wire.contains("request_id"),
            "events must not carry request_id: {wire}"
        );
        let back: IpcEvent = serde_json::from_str(&wire).expect("deserialize event");
        assert_eq!(back.event, "property-change");
        assert_eq!(back.name, "playback-status");
        assert_eq!(back.data, json!("Paused"));
    }

    #[test]
    fn protocol_version_constant_is_one() {
        // Tripwire: bumping PROTOCOL_VERSION is a deliberate breaking change.
        // Force the bumper to also update this test (and think about the
        // server-side compatibility shim that should land alongside it).
        assert_eq!(PROTOCOL_VERSION, 1);
    }
}
