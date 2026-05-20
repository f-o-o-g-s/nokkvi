//! Iced subscription wrapper for the `nokkvi-ipc` server.
//!
//! The IPC server lives in the iced-free `nokkvi-ipc` crate. This module
//! adapts its async stream of [`nokkvi_ipc::server::IncomingRequest`] values
//! into the iced [`Sipper`] shape used by the rest of `src/services/*`, and
//! wraps each request in a [`IpcIncoming`] handle that satisfies the
//! `Message: Clone` bound iced requires.
//!
//! # Why the wrapper instead of carrying [`oneshot::Sender`] directly
//!
//! `Message` derives `Clone` (most variants are cheap to clone). The raw
//! [`tokio::sync::oneshot::Sender`] from the IPC crate is *not* `Clone`, by
//! design — it represents an exactly-once send. We wrap it in
//! `Arc<Mutex<Option<Sender>>>` here so the message can be cloned freely while
//! still preserving the "first send wins, subsequent calls silently drop"
//! exactly-once semantics.
//!
//! Phase 0 only spawns one consumer per message, so the `Mutex` is uncontended;
//! it's a cheap insurance policy against a future caller cloning the message
//! and racing to respond first.

use std::sync::{Arc, Mutex};

use futures::StreamExt;
use iced::task::{Never, Sipper, sipper};
use nokkvi_ipc::{IpcResponse, server::IncomingRequest};
use tokio::sync::oneshot;

/// Cloneable handle to the per-request response back-channel. Sending more
/// than once silently no-ops; sending after the connection has been dropped
/// silently no-ops. See `IpcResponder::send` for details.
#[derive(Debug, Clone)]
pub struct IpcResponder {
    inner: Arc<Mutex<Option<oneshot::Sender<IpcResponse>>>>,
}

impl IpcResponder {
    pub(crate) fn new(tx: oneshot::Sender<IpcResponse>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Some(tx))),
        }
    }

    /// Send the response, consuming the underlying [`oneshot::Sender`]. If
    /// another caller has already sent — or if the client connection has been
    /// closed and the receiver dropped — this is a no-op.
    pub fn send(&self, response: IpcResponse) {
        // The lock is uncontended in practice (the dispatcher only sends from
        // one place) and held only for the `.take()`. A poisoned lock here
        // would mean a previous handler panicked while holding it — the only
        // sensible recovery is to silently skip this response, since the
        // alternative is widening the panic to the IPC subscription.
        if let Ok(mut guard) = self.inner.lock()
            && let Some(tx) = guard.take()
        {
            let _ = tx.send(response);
        }
    }
}

/// A request received from a client, ready to be dispatched. The cloneable
/// wrapper that flows through the iced `Message` channel.
#[derive(Debug, Clone)]
pub struct IpcIncoming {
    pub request: nokkvi_ipc::IpcRequest,
    pub responder: IpcResponder,
}

impl From<IncomingRequest> for IpcIncoming {
    fn from(raw: IncomingRequest) -> Self {
        Self {
            request: raw.request,
            responder: IpcResponder::new(raw.responder),
        }
    }
}

/// Iced subscription: bind the IPC socket and yield each parsed request.
///
/// Binds at the path returned by [`nokkvi_ipc::default_socket_path`]
/// (`$XDG_RUNTIME_DIR/nokkvi.sock` with a `/tmp` fallback). If the bind
/// fails — the most common cause being a parallel nokkvi instance that
/// holds a live socket — the error is logged and the sipper stalls on
/// `pending::<Never>()` so the iced subscription identity stays alive but
/// emits nothing. The single-instance refuse-guard in `main.rs` keeps this
/// from happening in normal operation.
pub(crate) fn run() -> impl Sipper<Never, IpcIncoming> {
    sipper(async move |mut output| {
        let path = nokkvi_ipc::default_socket_path();
        match nokkvi_ipc::server::listen(&path).await {
            Ok(mut stream) => {
                while let Some(req) = stream.next().await {
                    output.send(IpcIncoming::from(req)).await;
                }
                tracing::warn!("IPC listener stream ended unexpectedly");
            }
            Err(err) => {
                tracing::error!("IPC listener failed to bind: {err}");
            }
        }

        // Keep the iced subscription identity alive even after the listener
        // gives up — matches the pattern used by the other service modules
        // (`subscription_slot::run` parks on `pending::<Never>()` after the
        // channel closes).
        std::future::pending::<Never>().await
    })
}
