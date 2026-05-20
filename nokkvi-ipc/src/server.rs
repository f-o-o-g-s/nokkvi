//! Async server side of the IPC channel.
//!
//! [`listen`] binds a Unix domain socket at the given path and returns a
//! [`futures::Stream`] of [`IncomingRequest`] values. Each item carries the
//! parsed [`IpcRequest`] plus a [`tokio::sync::oneshot::Sender`] that the
//! consumer fills with the matching [`IpcResponse`]. The server task owns
//! the writer end of the connection and posts the response back over the
//! wire once the oneshot fires.
//!
//! # Per-connection protocol (Phase 0)
//!
//! One request per connection. The connection is closed as soon as the
//! response is sent. Phase 4 will lift this restriction once events / long-
//! lived `nokkvi watch` connections land — the wire shape (line-delimited
//! JSON, `request_id`-multiplexed) is already designed for it.
//!
//! # Errors and observability
//!
//! Connection-level errors (bind failure, accept failure, malformed JSON, a
//! client that hangs up before the response is written) are logged via
//! [`tracing`] at the appropriate level. They do not propagate to the stream
//! consumer — there is nothing for the UI handler to do about a single
//! broken connection, and surfacing them as toasts would be noise.
//!
//! Bind failure is the one error that does propagate, because the caller
//! needs to decide whether to retry or give up (returned via [`ServerError`]
//! from [`listen`]).

use std::{
    path::{Path, PathBuf},
    pin::Pin,
    task::{Context, Poll},
};

use futures::Stream;
use interprocess::local_socket::{
    GenericFilePath, ListenerOptions, ToFsName,
    tokio::{Stream as IpcStream, prelude::*},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::{mpsc, oneshot},
};

use crate::protocol::{IpcRequest, IpcResponse};

/// A request that has been parsed off the wire and is awaiting a response.
///
/// The consumer of [`listen`]'s stream is expected to fill `responder` with
/// the matching [`IpcResponse`]. Dropping the responder without sending is
/// treated as a server-side fault: the client connection is closed without a
/// reply and a `warn!` is logged. (Phase 0 dispatch never drops; the warn is
/// there to catch future regressions.)
#[derive(Debug)]
pub struct IncomingRequest {
    pub request: IpcRequest,
    pub responder: oneshot::Sender<IpcResponse>,
}

/// Errors returned synchronously from [`listen`] before the stream starts.
///
/// Per-connection errors after the bind succeeds are logged, not returned.
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("failed to construct socket name for {path:?}: {source}")]
    InvalidPath {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to bind IPC socket at {path:?}: {source}")]
    Bind {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Bind the IPC socket and return a stream of incoming requests.
///
/// Removes any stale socket file at `path` before binding (halloy pattern —
/// crashed previous instances leave a corpse socket that would otherwise
/// fail `EADDRINUSE` even though no process holds it).
pub async fn listen(path: &Path) -> Result<IncomingRequestStream, ServerError> {
    let _ = tokio::fs::remove_file(path).await;

    let name = path
        .to_fs_name::<GenericFilePath>()
        .map_err(|source| ServerError::InvalidPath {
            path: path.to_path_buf(),
            source,
        })?;

    let listener = ListenerOptions::new()
        .name(name)
        .create_tokio()
        .map_err(|source| ServerError::Bind {
            path: path.to_path_buf(),
            source,
        })?;

    let (tx, rx) = mpsc::unbounded_channel::<IncomingRequest>();

    tokio::spawn(accept_loop(listener, tx));

    Ok(IncomingRequestStream { rx })
}

/// Stream adapter over the per-connection mpsc that [`listen`] populates.
#[derive(Debug)]
pub struct IncomingRequestStream {
    rx: mpsc::UnboundedReceiver<IncomingRequest>,
}

impl Stream for IncomingRequestStream {
    type Item = IncomingRequest;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}

async fn accept_loop(
    listener: interprocess::local_socket::tokio::Listener,
    tx: mpsc::UnboundedSender<IncomingRequest>,
) {
    loop {
        match listener.accept().await {
            Ok(conn) => {
                let tx = tx.clone();
                tokio::spawn(async move {
                    if let Err(err) = handle_connection(conn, tx).await {
                        tracing::warn!("IPC connection failed: {err}");
                    }
                });
            }
            Err(err) => {
                tracing::error!("IPC accept failed: {err}");
                // A persistent accept failure (e.g. listener fd closed) would
                // spin this loop. Yield once to avoid pegging a core in that
                // pathological case; the listener will continue to error and
                // the consumer's stream stays alive but unfed.
                tokio::task::yield_now().await;
            }
        }
    }
}

async fn handle_connection(
    conn: IpcStream,
    tx: mpsc::UnboundedSender<IncomingRequest>,
) -> std::io::Result<()> {
    let (read_half, mut write_half) = conn.split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::with_capacity(256);

    let read_bytes = reader.read_line(&mut line).await?;
    if read_bytes == 0 {
        // Client closed without sending anything.
        return Ok(());
    }

    let request: IpcRequest = match serde_json::from_str(line.trim_end()) {
        Ok(req) => req,
        Err(err) => {
            // Send a structured error back so the client gets a clean message
            // instead of an unexplained EOF. request_id is unknown — surface
            // 0 as the sentinel, since the wire has no concept of "no id."
            let resp = IpcResponse::err(
                0,
                "malformed_request",
                format!("could not parse request as JSON: {err}"),
            );
            write_response(&mut write_half, &resp).await?;
            return Ok(());
        }
    };

    let (resp_tx, resp_rx) = oneshot::channel::<IpcResponse>();
    let request_id = request.request_id;

    if tx
        .send(IncomingRequest {
            request,
            responder: resp_tx,
        })
        .is_err()
    {
        // Receiver dropped — server is shutting down. Tell the client.
        let resp = IpcResponse::err(
            request_id,
            "server_shutting_down",
            "server is shutting down",
        );
        write_response(&mut write_half, &resp).await?;
        return Ok(());
    }

    match resp_rx.await {
        Ok(resp) => write_response(&mut write_half, &resp).await?,
        Err(_) => {
            tracing::warn!(
                request_id,
                "IPC dispatcher dropped responder without sending a reply"
            );
            let resp = IpcResponse::err(
                request_id,
                "no_response",
                "dispatcher dropped responder without sending a reply",
            );
            write_response(&mut write_half, &resp).await?;
        }
    }

    Ok(())
}

async fn write_response<W>(writer: &mut W, response: &IpcResponse) -> std::io::Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut wire = serde_json::to_vec(response).map_err(std::io::Error::other)?;
    wire.push(b'\n');
    writer.write_all(&wire).await?;
    writer.flush().await?;
    Ok(())
}
