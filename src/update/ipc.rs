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
//! Each command row maps to one of four arm shapes:
//!
//! - `respond (<payload>)` — synchronous reply with a JSON payload. The
//!   responder is filled in and `Task::none()` is returned. Use this for
//!   compute-and-return verbs (`ping`'s `"pong"` today, `current`/`queue`/
//!   `state` in Phase 3).
//! - `dispatch (<Message>)` — fire-and-forget. The responder is filled with
//!   an empty success and the message is queued via [`Task::done`] for the
//!   normal update loop to handle. Use this for transport verbs whose
//!   side-effects (UI updates, MPRIS broadcasts, scrobble timing) belong on
//!   the regular pipeline.
//! - `with_f32 (<arg_name>, <closure>)` — fire-and-forget with a single named
//!   `f32` argument extracted from `incoming.request.args`. The closure
//!   builds the `Message`. Missing or non-numeric args return an
//!   `invalid_args` error response instead of dispatching.
//! - `try_act (<closure>)` — closure receives `&mut Nokkvi` and the args
//!   `serde_json::Value`, returns `Result<Task<Message>, (&'static str,
//!   String)>`. On `Ok(task)` the responder gets an empty success and the
//!   task is returned; on `Err((code, message))` the responder gets an
//!   error response with those fields and `Task::none()` is returned. Use
//!   this for verbs that need direct app-state access (resolving the
//!   currently-playing track, view switches, bypassing UI-gate handlers).
//!
//! Per-row arg groups are parenthesized so the macro can capture them as a
//! single token tree (`:tt`) and re-destructure them in the inner @arm
//! rules — `:expr` metavars can't be re-matched after forwarding.
//!
//! Unknown verbs return a structured `unknown_command` error response.
//!
//! # Phase 0 + Phase 1 + Phase 2 catalog
//!
//! | Verb          | Arm shape | Notes                                          |
//! |---------------|-----------|------------------------------------------------|
//! | `ping`        | respond   | Returns the JSON string `"pong"`.              |
//! | `next`        | dispatch  | `PlaybackMessage::NextTrack`                   |
//! | `previous`    | dispatch  | `PlaybackMessage::PrevTrack`                   |
//! | `play`        | dispatch  | `PlaybackMessage::Play`                        |
//! | `pause`       | dispatch  | `PlaybackMessage::Pause`                       |
//! | `play-pause`  | dispatch  | `PlaybackMessage::TogglePlay`                  |
//! | `stop`        | dispatch  | `PlaybackMessage::Stop`                        |
//! | `seek`        | with_f32  | arg `position` (seconds, absolute).            |
//! | `volume`      | with_f32  | arg `value` 0.0–1.0; routes through            |
//! |               |           | `VolumeCommitted` to bypass the 500ms throttle.|
//! | `shuffle`     | dispatch  | Toggle (`ToggleRandom`).                       |
//! | `repeat`      | dispatch  | Cycle off → one → queue (`ToggleRepeat`).      |
//! | `consume`     | dispatch  | Toggle (`ToggleConsume`).                      |
//! | `clear-queue` | try_act   | Calls `clear_queue_action()` (gate-free).      |
//! | `switch-view` | try_act   | arg `view` (one of `albums`/`queue`/`songs`/   |
//! |               |           | `artists`/`genres`/`playlists`/`radios`/       |
//! |               |           | `settings`). Invalid name → `invalid_args`.    |

use iced::Task;
use nokkvi_ipc::IpcResponse;
use serde_json::json;

use crate::{
    Nokkvi, View,
    app_message::{Message, NavigationMessage, PlaybackMessage},
    services::ipc::IpcIncoming,
};

/// Generate a fire-and-forget / synchronous-respond IPC dispatcher plus a
/// `KNOWN_COMMANDS` const that callers (the argv parser in `main.rs`) can
/// use to decide whether an argument is a known IPC verb.
///
/// See the module-level docs for the four arm shapes
/// (`respond` / `dispatch` / `with_f32` / `try_act`) and the rationale for
/// keeping this macro in the UI crate.
macro_rules! define_commands {
    (
        $( $verb:literal => $kind:ident $arg:tt ; )+ $(,)?
    ) => {
        pub(crate) const KNOWN_COMMANDS: &[&str] = &[ $( $verb ),+ ];

        pub(crate) fn handle(app: &mut Nokkvi, incoming: IpcIncoming) -> Task<Message> {
            let request_id = incoming.request.request_id;
            match incoming.request.command.as_str() {
                $(
                    $verb => define_commands!(@arm $kind $arg, incoming, request_id, app),
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

    (@arm respond ($payload:expr), $incoming:ident, $request_id:ident, $app:ident) => {{
        let _ = &$app;
        $incoming
            .responder
            .send(IpcResponse::ok($request_id, Some($payload)));
        Task::none()
    }};

    (@arm dispatch ($msg:expr), $incoming:ident, $request_id:ident, $app:ident) => {{
        let _ = &$app;
        $incoming
            .responder
            .send(IpcResponse::ok($request_id, None));
        Task::done($msg)
    }};

    (@arm with_f32 ($arg_name:literal, $build:expr), $incoming:ident, $request_id:ident, $app:ident) => {{
        let _ = &$app;
        match extract_f32_arg(&$incoming.request.args, $arg_name) {
            Ok(value) => {
                $incoming
                    .responder
                    .send(IpcResponse::ok($request_id, None));
                Task::done(($build)(value))
            }
            Err(message) => {
                $incoming.responder.send(IpcResponse::err(
                    $request_id,
                    "invalid_args",
                    message,
                ));
                Task::none()
            }
        }
    }};

    (@arm try_act ($closure:expr), $incoming:ident, $request_id:ident, $app:ident) => {{
        let result: Result<Task<Message>, (&'static str, String)> =
            ($closure)($app, &$incoming.request.args);
        match result {
            Ok(task) => {
                $incoming
                    .responder
                    .send(IpcResponse::ok($request_id, None));
                task
            }
            Err((code, message)) => {
                $incoming.responder.send(IpcResponse::err(
                    $request_id,
                    code,
                    message,
                ));
                Task::none()
            }
        }
    }};
}

/// Extract a named `f32` arg from an `IpcRequest::args` JSON value. Accepts
/// JSON numbers (integer or float); rejects strings, nulls, and missing
/// keys with a precise error message the client can show to a user.
fn extract_f32_arg(args: &serde_json::Value, name: &str) -> Result<f32, String> {
    match args.get(name) {
        Some(v) => v
            .as_f64()
            .map(|n| n as f32)
            .ok_or_else(|| format!("arg `{name}` must be a number, got {v}")),
        None => Err(format!("missing required arg: {name}")),
    }
}

/// Map a CLI / wire view-name to the corresponding [`View`] variant. Accepts
/// the lowercase canonical names matching the CLI surface. Returns the list
/// of supported names in the error message so the caller (or curious user)
/// can self-correct.
pub(crate) fn parse_view_name(name: &str) -> Result<View, String> {
    match name {
        "albums" => Ok(View::Albums),
        "queue" => Ok(View::Queue),
        "songs" => Ok(View::Songs),
        "artists" => Ok(View::Artists),
        "genres" => Ok(View::Genres),
        "playlists" => Ok(View::Playlists),
        "radios" => Ok(View::Radios),
        "settings" => Ok(View::Settings),
        other => Err(format!(
            "unknown view `{other}` (expected one of: albums, queue, songs, \
             artists, genres, playlists, radios, settings)"
        )),
    }
}

// VolumeCommitted bypasses the 500ms VolumeChanged throttle — discrete
// external commands (playerctl, IPC) must persist immediately so rapid
// presses don't silently drop on next launch. Mirrors the MPRIS SetVolume
// handler at src/update/mpris.rs:66.
define_commands! {
    "ping"        => respond  (json!("pong"));
    "next"        => dispatch (Message::Playback(PlaybackMessage::NextTrack));
    "previous"    => dispatch (Message::Playback(PlaybackMessage::PrevTrack));
    "play"        => dispatch (Message::Playback(PlaybackMessage::Play));
    "pause"       => dispatch (Message::Playback(PlaybackMessage::Pause));
    "play-pause"  => dispatch (Message::Playback(PlaybackMessage::TogglePlay));
    "stop"        => dispatch (Message::Playback(PlaybackMessage::Stop));
    "seek"        => with_f32 ("position", |v: f32| Message::Playback(PlaybackMessage::Seek(v)));
    "volume"      => with_f32 ("value",    |v: f32| Message::Playback(PlaybackMessage::VolumeCommitted(v.clamp(0.0, 1.0))));
    // Toggle-only in Phase 1 — matches WM-hotkey ergonomics. Arg-taking
    // variants (shuffle on/off, repeat none/track/queue) are deferred until
    // direct-setter PlaybackMessage variants exist. See §6 of new-feats.md.
    "shuffle"     => dispatch (Message::Playback(PlaybackMessage::ToggleRandom));
    "repeat"      => dispatch (Message::Playback(PlaybackMessage::ToggleRepeat));
    "consume"     => dispatch (Message::Playback(PlaybackMessage::ToggleConsume));
    // IPC bypasses handle_clear_queue's "not in queue view" gate — external
    // callers expect `nokkvi clear-queue` to clear from any view. The shared
    // clear_queue_action() lives in src/update/hotkeys/queue.rs.
    "clear-queue" => try_act  (|app: &mut Nokkvi, _args: &serde_json::Value| {
        Ok(app.clear_queue_action())
    });
    // Switch the top-pane view. The `view` arg is required and validated
    // against the View enum before dispatch; the actual switch goes through
    // the normal NavigationMessage::SwitchView path so view-change side
    // effects (data loads, focus shifts) fire as usual.
    "switch-view" => try_act  (|_app: &mut Nokkvi, args: &serde_json::Value| {
        let raw = args
            .get("view")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ("invalid_args", "missing required arg: view".into()))?;
        let view = parse_view_name(raw)
            .map_err(|message| ("invalid_args", message))?;
        Ok(Task::done(Message::Navigation(NavigationMessage::SwitchView(view))))
    });
}
