//! Dispatcher for IPC requests routed in via [`Message::Ipc`].
//!
//! Phase 0 shipped the hand-rolled `ping` arm. Phase 1 adds two more
//! fire-and-forget transport verbs (`next`, `previous`) so the boilerplate
//! has actually shown up before the `define_commands!` macro from
//! `~/nokkvi-new-feats.md` §14D gets extracted — Rule of Three.
//!
//! Each fire-and-forget verb follows the same shape:
//! 1. Build a success [`IpcResponse`] and post it back over the responder
//!    immediately so the client unblocks. We don't wait for the dispatched
//!    `Message` to actually complete — that's the mpv/MPRIS contract for
//!    transport commands and matches what scripts expect.
//! 2. Return `Task::done(Message::Playback(PlaybackMessage::Variant))` so
//!    the message flows through the normal update loop and any side-effects
//!    (UI updates, MPRIS broadcasts, scrobble timing) fire as usual.

use iced::Task;
use nokkvi_ipc::IpcResponse;
use serde_json::json;

use crate::{
    Nokkvi,
    app_message::{Message, PlaybackMessage},
    services::ipc::IpcIncoming,
};

pub(crate) fn handle(_app: &mut Nokkvi, incoming: IpcIncoming) -> Task<Message> {
    let request_id = incoming.request.request_id;

    match incoming.request.command.as_str() {
        "ping" => {
            incoming
                .responder
                .send(IpcResponse::ok(request_id, Some(json!("pong"))));
            Task::none()
        }
        "next" => {
            incoming.responder.send(IpcResponse::ok(request_id, None));
            Task::done(Message::Playback(PlaybackMessage::NextTrack))
        }
        "previous" => {
            incoming.responder.send(IpcResponse::ok(request_id, None));
            Task::done(Message::Playback(PlaybackMessage::PrevTrack))
        }
        other => {
            incoming.responder.send(IpcResponse::err(
                request_id,
                "unknown_command",
                format!("unknown command: {other}"),
            ));
            Task::none()
        }
    }
}
