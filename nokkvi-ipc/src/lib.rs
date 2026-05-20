//! IPC layer for nokkvi external control. See `~/nokkvi-new-feats.md` §14.
//!
//! # Structural invariant
//!
//! This crate is **iced-free**. The fork-before-iced client path
//! (`nokkvi <cmd>` in `src/main.rs`) links it directly, before iced's
//! runtime is constructed, so adding an iced dependency here would break
//! the startup-latency contract (and the build).
//!
//! # Wire-protocol invariants (locked from Phase 0; see `protocol` module)
//!
//! 1. `protocol_version` accompanies every request.
//! 2. `request_id` is echoed on every response so callers can multiplex
//!    multiple in-flight requests over a single connection.
//! 3. Tolerant deserialization on both peers — no `deny_unknown_fields` —
//!    so newer fields don't break older peers.
//! 4. Reserved top-level field names: `protocol_version`, `request_id`,
//!    `command`, `args`, `data`, `error`, `event`, `name`. Anything else
//!    is namespaced under `args` / `data`.
//!
//! # Phase 0 surface
//!
//! Only the `protocol` module is wired today. The socket transport
//! (`server`, `client`, `socket_path`) and the iced subscription / dispatch
//! glue land in subsequent commits on this branch.

pub mod protocol;

pub use protocol::{IpcError, IpcEvent, IpcRequest, IpcResponse, PROTOCOL_VERSION};

/// Phase 0 smoke command. Retained until `nokkvi ping` is wired end-to-end
/// through the socket — at which point this trivial helper goes away.
pub fn ping() -> &'static str {
    "pong"
}
