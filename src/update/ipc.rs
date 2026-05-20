//! Dispatcher for IPC requests routed in via [`Message::Ipc`].
//!
//! # Where the macro lives
//!
//! [`define_commands!`] sits here, in the iced UI crate, even though
//! `~/nokkvi-new-feats.md` §14C originally placed it under
//! `nokkvi-ipc/src/commands.rs`. The macro inputs include
//! `Message::Playback(PlaybackMessage::NextTrack)`-style references that name
//! types from this crate's `Message` enum, so a `nokkvi-ipc`-resident macro
//! would either need heavy parameterization or would force `nokkvi-ipc` to
//! depend on iced — breaking the [`nokkvi_ipc`] structural invariant.
//!
//! The wire-protocol envelope types (`IpcRequest` / `IpcResponse` / `IpcEvent`)
//! that *are* the cross-crate contract live in `nokkvi-ipc::protocol` where
//! they belong. The dispatch macro is an internal convenience and can move
//! later if the shape evolves (e.g. once `nokkvi-ipc` learns about a
//! `Dispatch` trait callers can implement).
//!
//! # How it dispatches
//!
//! Each command row maps to one of two arm shapes:
//!
//! - `respond <payload>` — synchronous reply with a JSON payload. The
//!   responder is filled in and `Task::none()` is returned. Use this for
//!   compute-and-return verbs (`ping`'s `"pong"` today, `current`/`queue`/
//!   `state` in Phase 3).
//! - `dispatch <Message>` — fire-and-forget. The responder is filled with an
//!   empty success and the message is queued via [`Task::done`] for the
//!   normal update loop to handle. Use this for transport verbs whose
//!   side-effects (UI updates, MPRIS broadcasts, scrobble timing) belong on
//!   the regular pipeline.
//!
//! Unknown verbs return a structured `unknown_command` error response.

use iced::Task;
use nokkvi_ipc::IpcResponse;
use serde_json::json;

use crate::{
    Nokkvi,
    app_message::{Message, PlaybackMessage},
    services::ipc::IpcIncoming,
};

/// Generate a fire-and-forget / synchronous-respond IPC dispatcher plus a
/// `KNOWN_COMMANDS` const that callers (the argv parser in `main.rs`) can
/// use to decide whether an argument is a known IPC verb.
///
/// See the module-level docs for the two arm shapes (`respond` / `dispatch`)
/// and the rationale for keeping this macro in the UI crate.
macro_rules! define_commands {
    (
        $( $verb:literal => $kind:ident $arg:expr ; )+ $(,)?
    ) => {
        pub(crate) const KNOWN_COMMANDS: &[&str] = &[ $( $verb ),+ ];

        pub(crate) fn handle(_app: &mut Nokkvi, incoming: IpcIncoming) -> Task<Message> {
            let request_id = incoming.request.request_id;
            match incoming.request.command.as_str() {
                $(
                    $verb => define_commands!(@arm $kind, $arg, incoming, request_id),
                )+
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
    };

    (@arm respond, $payload:expr, $incoming:ident, $request_id:ident) => {{
        $incoming
            .responder
            .send(IpcResponse::ok($request_id, Some($payload)));
        Task::none()
    }};

    (@arm dispatch, $msg:expr, $incoming:ident, $request_id:ident) => {{
        $incoming
            .responder
            .send(IpcResponse::ok($request_id, None));
        Task::done($msg)
    }};
}

define_commands! {
    "ping"     => respond  json!("pong");
    "next"     => dispatch Message::Playback(PlaybackMessage::NextTrack);
    "previous" => dispatch Message::Playback(PlaybackMessage::PrevTrack);
}
