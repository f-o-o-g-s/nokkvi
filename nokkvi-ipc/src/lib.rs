//! IPC layer for nokkvi external control. See `~/nokkvi-new-feats.md` §14.
//!
//! # Structural invariant
//!
//! This crate is **iced-free**. The fork-before-iced client path
//! (`nokkvi <cmd>` in `src/main.rs`) links it directly, before iced's
//! runtime is constructed, so adding an iced dependency here would break
//! the startup-latency contract (and the build).
//!
//! # Wire-protocol invariants (locked from Phase 0; see [`protocol`])
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
//! # Module surface
//!
//! - [`protocol`] — wire envelope types (`IpcRequest` / `IpcResponse` / …).
//! - [`server`] — async `listen(path)` returning a stream of
//!   [`server::IncomingRequest`] values, each with an embedded oneshot
//!   responder. Phase 0 is one-request-per-connection.
//! - [`client`] — sync [`client::send_request`] for the fork-before-iced
//!   path; one request → one response → close.
//! - [`socket_path`] — XDG-aware default socket path resolution.
//!
//! # UI-side wiring
//!
//! The iced subscription glue and the argv fork in `nokkvi`'s `main.rs` live
//! in the UI crate, not here. This crate intentionally knows nothing about
//! iced, `Message`, or the surrounding app — it just speaks the wire.
//!
//! The dispatch macro (`define_commands!`) that maps verb names to UI
//! `Message` variants also lives in the UI crate (`src/update/ipc.rs`),
//! because its inputs reference iced-land types. Verb catalogs published by
//! the macro are not part of this crate's API.
//!
//! # Phase 0 + Phase 1 verbs (informative — the wire accepts any string)
//!
//! - **Phase 0:** `ping`.
//! - **Phase 1:** `next`, `previous`, `play`, `pause`, `play-pause`, `stop`,
//!   `seek` (arg `position: f32` seconds), `volume` (arg `value: f32` 0–1),
//!   `shuffle` (toggle), `repeat` (cycle).
//!
//! The full per-verb dispatch catalog lives in `src/update/ipc.rs`.

pub mod client;
pub mod protocol;
pub mod server;
pub mod socket_path;

pub use protocol::{IpcError, IpcEvent, IpcRequest, IpcResponse, PROTOCOL_VERSION};
pub use socket_path::default_socket_path;
