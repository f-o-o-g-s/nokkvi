//! Tests for the IPC command dispatcher.
//!
//! These exercise `update::ipc::handle` synchronously against a synthesized
//! `IpcIncoming`. The socket transport, the iced subscription, and the
//! `Sipper` wrapping are all covered separately (`nokkvi-ipc/tests/round_trip.rs`
//! for the transport; iced owns the subscription path).

use nokkvi_ipc::{IpcRequest, IpcResponse, PROTOCOL_VERSION};
use serde_json::json;
use tokio::sync::oneshot;

use crate::{
    app_message::Message,
    services::ipc::{IpcIncoming, IpcResponder},
    test_helpers::test_app,
};

fn make_incoming(command: &str) -> (IpcIncoming, oneshot::Receiver<IpcResponse>) {
    let (tx, rx) = oneshot::channel::<IpcResponse>();
    let request = IpcRequest {
        protocol_version: PROTOCOL_VERSION,
        request_id: 7,
        command: command.to_string(),
        args: serde_json::Value::Null,
    };
    let incoming = IpcIncoming {
        request,
        responder: IpcResponder::new(tx),
    };
    (incoming, rx)
}

/// Drive one verb through the dispatcher and return the response that the
/// responder receives. Shared by every fire-and-forget verb test.
fn drive(command: &str) -> IpcResponse {
    let mut app = test_app();
    let (incoming, rx) = make_incoming(command);

    let dispatched = app.update(Message::Ipc(Box::new(incoming)));
    drop(dispatched);

    rx.blocking_recv()
        .unwrap_or_else(|_| panic!("responder must fire for {command} command"))
}

#[test]
fn ping_command_yields_pong_payload() {
    let resp = drive("ping");
    assert_eq!(resp.request_id, 7);
    assert_eq!(resp.data, Some(json!("pong")));
    assert!(resp.error.is_none());
}

#[test]
fn fire_and_forget_verbs_all_respond_ok_with_no_payload() {
    // Single table-driven test for every Phase 0 + Phase 1 verb whose contract
    // is "ack now, side-effects later" — adding the next verb is one row, not
    // a new test function. Pins the IPC-layer contract (responder fires,
    // request_id echoed, data empty, no error). Playback side-effects are
    // covered by the existing playback handler tests.
    for verb in ["next", "previous", "play", "pause", "play-pause", "stop"] {
        let resp = drive(verb);
        assert_eq!(resp.request_id, 7, "{verb}: request_id must echo");
        assert!(
            resp.data.is_none(),
            "{verb}: fire-and-forget verbs should not carry data"
        );
        assert!(resp.error.is_none(), "{verb}: should not error");
    }
}

#[test]
fn unknown_command_yields_structured_error() {
    let resp = drive("bogus-verb");
    assert_eq!(resp.request_id, 7);
    assert!(resp.data.is_none());
    let err = resp.error.expect("error populated");
    assert_eq!(err.code, "unknown_command");
    assert!(err.message.contains("bogus-verb"));
}
