//! End-to-end ping over a real Unix domain socket.
//!
//! The async server runs on the test's tokio runtime; the sync client runs on
//! a blocking task so it doesn't deadlock the runtime. Each test uses a
//! `TempDir` for the socket path so parallel runs don't collide.

use std::time::Duration;

use futures::StreamExt;
use nokkvi_ipc::{
    client::{ClientError, send_request},
    protocol::{IpcRequest, IpcResponse, PROTOCOL_VERSION},
    server::listen,
};
use serde_json::json;
use tempfile::TempDir;
use tokio::time::timeout;

fn socket_path(tmp: &TempDir) -> std::path::PathBuf {
    tmp.path().join("nokkvi.sock")
}

#[tokio::test]
async fn ping_round_trips_through_real_socket() {
    let tmp = TempDir::new().expect("tempdir");
    let path = socket_path(&tmp);

    let mut stream = listen(&path).await.expect("bind socket");

    // Dispatcher: receive one request, echo a pong response.
    let dispatcher = tokio::spawn(async move {
        let incoming = stream.next().await.expect("one request");
        assert_eq!(incoming.request.command, "ping");
        assert_eq!(incoming.request.protocol_version, PROTOCOL_VERSION);
        let resp = IpcResponse::ok(incoming.request.request_id, Some(json!("pong")));
        incoming
            .responder
            .send(resp)
            .expect("send response oneshot");
    });

    // Client runs on a blocking task to keep the sync API off the runtime.
    let req = IpcRequest::new(1, "ping", serde_json::Value::Null);
    let client_path = path.clone();
    let resp = tokio::task::spawn_blocking(move || send_request(&client_path, &req))
        .await
        .expect("client join")
        .expect("client send");

    assert_eq!(resp.request_id, 1);
    assert_eq!(resp.data, Some(json!("pong")));
    assert!(resp.error.is_none());

    timeout(Duration::from_secs(1), dispatcher)
        .await
        .expect("dispatcher finished within timeout")
        .expect("dispatcher task did not panic");
}

#[tokio::test]
async fn connect_to_missing_socket_returns_connect_error() {
    let tmp = TempDir::new().expect("tempdir");
    let path = socket_path(&tmp);

    // No listener bound — the client must fail with Connect, not panic.
    let req = IpcRequest::new(1, "ping", serde_json::Value::Null);
    let result = tokio::task::spawn_blocking(move || send_request(&path, &req))
        .await
        .expect("client join");

    match result {
        Err(ClientError::Connect { .. }) => {}
        other => panic!("expected ClientError::Connect, got {other:?}"),
    }
}

#[tokio::test]
async fn malformed_request_returns_structured_error_response() {
    use std::io::{BufRead, BufReader, Write};

    use interprocess::local_socket::{GenericFilePath, Stream, ToFsName, prelude::*};

    let tmp = TempDir::new().expect("tempdir");
    let path = socket_path(&tmp);

    let _stream = listen(&path).await.expect("bind socket");

    let resp_line = tokio::task::spawn_blocking({
        let path = path.clone();
        move || -> std::io::Result<String> {
            let name = path.to_fs_name::<GenericFilePath>()?;
            let mut reader = BufReader::new(Stream::connect(name)?);
            reader.get_mut().write_all(b"not json at all\n")?;
            reader.get_mut().flush()?;
            let mut line = String::new();
            reader.read_line(&mut line)?;
            Ok(line)
        }
    })
    .await
    .expect("client join")
    .expect("client io");

    let resp: IpcResponse = serde_json::from_str(resp_line.trim_end()).expect("parse response");
    assert_eq!(resp.request_id, 0, "no request_id in payload → sentinel 0");
    assert!(resp.data.is_none());
    let err = resp.error.expect("error populated");
    assert_eq!(err.code, "malformed_request");
}
