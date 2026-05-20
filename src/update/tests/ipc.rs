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

#[test]
fn ping_command_yields_pong_payload() {
    let mut app = test_app();
    let (incoming, rx) = make_incoming("ping");

    let dispatched = app.update(Message::Ipc(Box::new(incoming)));
    drop(dispatched);

    let resp = rx
        .blocking_recv()
        .expect("responder must fire for ping command");
    assert_eq!(resp.request_id, 7);
    assert_eq!(resp.data, Some(json!("pong")));
    assert!(resp.error.is_none());
}

#[test]
fn unknown_command_yields_structured_error() {
    let mut app = test_app();
    let (incoming, rx) = make_incoming("bogus-verb");

    let dispatched = app.update(Message::Ipc(Box::new(incoming)));
    drop(dispatched);

    let resp = rx
        .blocking_recv()
        .expect("responder must fire for unknown command");
    assert_eq!(resp.request_id, 7);
    assert!(resp.data.is_none());
    let err = resp.error.expect("error populated");
    assert_eq!(err.code, "unknown_command");
    assert!(err.message.contains("bogus-verb"));
}
