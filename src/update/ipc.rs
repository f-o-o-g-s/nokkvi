//! Dispatcher for IPC requests routed in via [`Message::Ipc`].
//!
//! Phase 0 implements a single hand-rolled command (`ping`) plus the
//! `unknown_command` default arm. The `define_commands!` macro called out in
//! `~/nokkvi-new-feats.md` §14D is deliberately deferred until Phase 1 has
//! at least three concrete commands to refactor against (Rule of Three).
//!
//! Adding a new command before the macro arrives means: append a match arm
//! here, fill in the response shape, and add a handler-level test in
//! `src/update/tests/ipc.rs`. The macro will collapse the verbose arms once
//! the boilerplate has actually shown up.

use iced::Task;
use nokkvi_ipc::IpcResponse;
use serde_json::json;

use crate::{Nokkvi, app_message::Message, services::ipc::IpcIncoming};

/// Top-level entry called from `src/update/mod.rs` when [`Message::Ipc`]
/// fires. Builds the response synchronously and posts it back over the
/// embedded responder. Phase 0 has no async commands; everything completes
/// in-handler, so the returned task is always `Task::none()`.
pub(crate) fn handle(_app: &mut Nokkvi, incoming: IpcIncoming) -> Task<Message> {
    let request_id = incoming.request.request_id;
    let response = match incoming.request.command.as_str() {
        "ping" => IpcResponse::ok(request_id, Some(json!("pong"))),
        other => IpcResponse::err(
            request_id,
            "unknown_command",
            format!("unknown command: {other}"),
        ),
    };
    incoming.responder.send(response);
    Task::none()
}
