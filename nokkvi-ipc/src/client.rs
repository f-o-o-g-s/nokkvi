//! Synchronous client side of the IPC channel.
//!
//! [`send_request`] is intentionally synchronous: the only caller is the
//! fork-before-iced argv path in `nokkvi`'s `main()` (and any third-party
//! script that wants the same shape). Avoiding tokio here keeps the
//! startup-latency contract — `nokkvi ping` does not spin up an async
//! runtime just to write one JSON line.
//!
//! Phase 0 contract: one request → one response → connection closes. Phase 4
//! will add an async variant for `nokkvi watch`-style streaming, but the
//! sync path stays as-is forever.

use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

use interprocess::local_socket::{GenericFilePath, Stream, ToFsName, prelude::*};

use crate::protocol::{IpcRequest, IpcResponse};

/// Errors returned by [`send_request`].
///
/// `Connect` is the one most callers care about — it's how the single-instance
/// probe in `main()` distinguishes "no server running" from "server running
/// and responding."
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("failed to construct socket name for {path:?}: {source}")]
    InvalidPath {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to connect to IPC socket at {path:?}: {source}")]
    Connect {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("I/O error while talking to IPC server: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to serialize request: {0}")]
    SerializeRequest(#[source] serde_json::Error),
    #[error("server closed connection without sending a response")]
    EmptyResponse,
    #[error("failed to parse response from IPC server: {0}")]
    ParseResponse(#[source] serde_json::Error),
}

/// Connect to the IPC socket at `path`, send `req`, read one response, return.
///
/// Returns [`ClientError::Connect`] when the socket file is absent or the
/// kernel refuses the connection — that's the signal `main()` uses to fall
/// through to bringing up a fresh nokkvi instance.
pub fn send_request(path: &Path, req: &IpcRequest) -> Result<IpcResponse, ClientError> {
    let name = path
        .to_fs_name::<GenericFilePath>()
        .map_err(|source| ClientError::InvalidPath {
            path: path.to_path_buf(),
            source,
        })?;

    let stream = Stream::connect(name).map_err(|source| ClientError::Connect {
        path: path.to_path_buf(),
        source,
    })?;

    let mut payload = serde_json::to_vec(req).map_err(ClientError::SerializeRequest)?;
    payload.push(b'\n');

    let mut reader = BufReader::new(stream);
    reader.get_mut().write_all(&payload)?;
    reader.get_mut().flush()?;

    let mut line = String::with_capacity(256);
    let read = reader.read_line(&mut line)?;
    if read == 0 {
        return Err(ClientError::EmptyResponse);
    }

    serde_json::from_str(line.trim_end()).map_err(ClientError::ParseResponse)
}
