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
//! Each command row maps to one of five arm shapes. Every success carries a
//! JSON `data` payload so a verb can never succeed silently (the MPD "no
//! command is silent" discipline): mutating verbs echo their resulting state,
//! the rest send `{"ok": true}`.
//!
//! - `respond (<payload>)` — synchronous reply with a const JSON payload (no
//!   app access). The responder is filled in and `Task::none()` is returned.
//!   Used by `ping`'s `"pong"`.
//! - `dispatch (<Message>)` — fire-and-forget. The responder gets an
//!   `{"ok": true}` acknowledgement and the message is queued via
//!   [`Task::done`] for the normal update loop. Use this for verbs whose
//!   result is genuinely async at dispatch time (e.g. `next`/`previous` — the
//!   new track lands later) and whose side-effects belong on the regular
//!   pipeline.
//! - `act (<closure>)` — closure receives `&mut Nokkvi`, returns
//!   `Result<(Task<Message>, serde_json::Value), (&'static str, String)>`. On
//!   `Ok((task, data))` the responder gets `data` and the task is returned; on
//!   `Err((code, message))` the responder gets an error response and
//!   `Task::none()`. Use this for verbs that need direct app-state access and
//!   take no named arg — including the toggle/transport verbs, which call the
//!   real handler and then read the resulting state back into `data` (the
//!   `PlaybackMessage` arms are 1:1 handler wrappers, so calling the handler
//!   directly loses no side-effects).
//! - `act_str (<arg_name>, <closure>)` — `act` with an auto-extracted text
//!   arg. Closure receives `&mut Nokkvi` and `&str` and returns the same
//!   `(Task, data)` result, so it can echo the resulting value and gate on
//!   app state (e.g. `seek` rejects radio playback). Missing-arg returns
//!   `invalid_args` before the closure runs; a JSON number is read by its
//!   decimal text, so raw-socket clients and older CLIs keep working.
//!   **Use this** whenever a verb takes a single named arg — it (a) keeps the
//!   closure focused, (b) lets the macro publish the CLI arg name, which
//!   prevents the macro/CLI-parser drift class that plagued earlier designs,
//!   and (c) leaves the `+`/`-` prefix intact for relative values.
//!
//! ## Decision rule (which arm shape to pick)
//!
//! 1. **No args, result genuinely async at dispatch time** → `dispatch`
//!    (acks `{"ok": true}`).
//! 2. **No args, const compute-and-return payload (no app access)** →
//!    `respond`.
//! 3. **Single arg, needs `&mut Nokkvi`** → `act_str`. The closure gets the
//!    arg's text and owns parsing, which is what lets a verb accept a
//!    `+`/`-` prefix for a relative value.
//! 4. **No args but needs `&mut Nokkvi` (gate-bypass / app-state read /
//!    resulting-state echo)** → `act` with `|app|`.
//! 5. **Multiple args or a complex arg shape** → `act` with manual extraction
//!    from `incoming.request.args`. Document the arg names in the verb's
//!    catalog entry; the CLI side will need a matching `build_ipc_cli_args`
//!    arm (drift risk — minimize this case).
//!
//! Per-row arg groups are parenthesized so the macro can capture them as a
//! single token tree (`:tt`) and re-destructure them in the inner @arm
//! rules — `:expr` metavars can't be re-matched after forwarding.
//!
//! Unknown verbs return a structured `unknown_command` error response.
//!
//! # Single source of truth for CLI arg routing
//!
//! Alongside `KNOWN_COMMANDS`, the macro emits `CLI_ARGS` — a const slice
//! of `(verb, Option<(arg_name, CliArgType)>)` pairs. `main.rs`'s
//! [`crate::build_ipc_cli_args`] looks up the verb in `CLI_ARGS` to decide
//! how to wrap the positional CLI string before forwarding. That eliminates
//! the per-verb match arms a previous design relied on, and with them the
//! drift risk of "added a new verb to the macro, forgot to register its
//! CLI arg name."
//!
//! # Verb catalog
//!
//! Every success carries a JSON `data` payload — the `data column` below is the
//! exact shape the CLI prints (compact JSON; a bare `respond` string prints
//! unquoted). Mutating verbs echo their resulting (optimistic) state; `status`
//! reports ground-truth.
//!
//! | Verb          | Arm shape | `data` on success / notes                      |
//! |---------------|-----------|------------------------------------------------|
//! | `ping`        | respond   | `"pong"` (bare string).                        |
//! | `status`      | act       | `{state,title,artist,album,position,duration,` |
//! |               |           | `volume,random,repeat,consume,theater,`        |
//! |               |           | `visualizer,preset}` — pure                     |
//! |               |           | read.                                          |
//! | `next`        | dispatch  | `{"ok":true}`; `NextTrack` (new track async).  |
//! | `previous`    | dispatch  | `{"ok":true}`; `PrevTrack` (new track async).  |
//! | `play`        | act       | `{"state":…}`; calls `handle_play`.            |
//! | `pause`       | act       | `{"state":"paused"}`; calls `handle_pause`.    |
//! | `play-pause`  | act       | `{"state":…}`; calls `handle_toggle_play`.     |
//! | `stop`        | act       | `{"state":"stopped"}`; calls `handle_stop`.    |
//! | `seek`        | act_str   | `{"position":N}` absolute, or `{"offset":±N}`  |
//! |               |           | relative; arg `position` (seconds). `"+N"`/    |
//! |               |           | `"-N"` seeks relative to the current position, |
//! |               |           | `"N"` is absolute. `unavailable` during radio. |
//! | `volume`      | act_str   | `{"volume":N}`; arg `value`. `"+N"`/`"-N"`     |
//! |               |           | (delta, clamped 0.0..=1.0) or `"N"` absolute  |
//! |               |           | (0.0..=1.0, rejected if out of range). Routes |
//! |               |           | through `VolumeCommitted` (bypasses throttle).|
//! | `shuffle`     | act       | `{"random":bool}`; calls `handle_toggle_random`.|
//! | `repeat`      | act       | `{"repeat":"off"\|"one"\|"queue"}`; cycles.     |
//! | `consume`     | act       | `{"consume":bool}`; calls `handle_toggle_consume`.|
//! | `clear-queue` | act       | `{"ok":true}`; `clear_queue_action()` (gate-free).|
//! | `add-to-queue`| act       | `{"added":name\|null}`; enqueue the focused item|
//! |               |           | (Shift+A); null when nothing is selected.       |
//! | `remove-from-queue` | act | `{"removed":name\|null}`; remove the centered    |
//! |               |           | queue song (Ctrl+D); `not_in_queue_view` error |
//! |               |           | outside the queue view.                        |
//! | `switch-view` | act_str   | `{"view":name}`; arg `view` (one of `albums`/  |
//! |               |           | `queue`/`songs`/`artists`/`genres`/`playlists`/|
//! |               |           | `radios`/`settings`). Invalid → `invalid_args`.|
//! | `nav-up`      | dispatch  | `{"ok":true}`; move focused list up (async).   |
//! | `nav-down`    | dispatch  | `{"ok":true}`; move focused list down (async). |
//! | `enter`       | dispatch  | `{"ok":true}`; activate centered item.         |
//! | `selection`   | act       | `{view,kind,name,artist,rating,starred}` of    |
//! |               |           | the centered item; `kind:null` if none. Read.  |
//! | `love`        | act       | `{"loved":bool}`; toggle star on playing track.|
//! |               |           | `no_playing_track` error if nothing playing.   |
//! | `rate`        | act_str   | `{"rating":0..5}`; arg `delta` `"+N"`/`"-N"`    |
//! |               |           | (delta, clamped 0..=5) or `"0".."5"` absolute. |
//! |               |           | Same playing-track rules as `love`.            |
//! | `queue-push`  | act       | `{"dispatched":"push","tracks":N}`; save the    |
//! |               |           | queue to the server (async — failures toast).  |
//! |               |           | `unsupported` without indexBasedQueue;          |
//! |               |           | `unavailable` during radio; `empty_queue`.      |
//! | `queue-pull`  | act       | `{"dispatched":"pull"}`; restore the server's   |
//! |               |           | saved queue (cue, don't play). Same guards     |
//! |               |           | minus the empty-queue one.                     |
//! | `show`        | act       | `{"window":"opened"\|"already-open"\|"opening"}`;|
//! |               |           | reopen from the tray, flag an open window, or  |
//! |               |           | no-op while one is opening (`show_window`).    |
//! | `theater`     | act       | `{"theater":bool}`; toggle Theater Mode (the   |
//! |               |           | F11 entry point). `unavailable` on Login or    |
//! |               |           | while the window is closed to the tray.        |
//! | `preset`      | act_str   | `{"preset":name\|null,"locked":bool}`; arg     |
//! |               |           | `action`: `next`/`previous`/`lock`/`unlock`/   |
//! |               |           | `favorite`/`unfavorite`/`hide` (MilkDrop).     |
//! |               |           | `unavailable` on Login, outside MilkDrop mode, |
//! |               |           | when not playing on screen (next/previous) or  |
//! |               |           | with no preset on screen; bad word →           |
//! |               |           | `invalid_args`.                                |

use iced::Task;
use nokkvi_data::types::ItemKind;
use nokkvi_ipc::IpcResponse;
use serde_json::json;

use crate::{
    Nokkvi, View,
    app_message::{Message, NavigationMessage, PlaybackMessage, SlotListMessage},
    services::ipc::IpcIncoming,
};

/// Generate the IPC dispatcher plus three companion consts that
/// `main.rs`'s argv parser reads to stay drift-free:
///
/// - `KNOWN_COMMANDS: &[&str]` — every declared verb name.
/// - `CLI_ARGS: &[(&'static str, Option<&'static str>)]` — per verb, the CLI
///   arg-name (or `None` for no-arg verbs). Every arg-taking verb forwards
///   its positional verbatim as a JSON string; the server-side closure owns
///   parsing, which is what lets `seek`, `volume` and `rate` take a leading
///   `+`/`-`.
///
/// See the module-level docs for the five arm shapes and the decision rule.
macro_rules! define_commands {
    (
        $( $verb:literal => $kind:ident $arg:tt ; )+ $(,)?
    ) => {
        pub(crate) const KNOWN_COMMANDS: &[&str] = &[ $( $verb ),+ ];

        pub(crate) const CLI_ARGS: &[(&'static str, Option<&'static str>)] = &[
            $( ($verb, define_commands!(@cli_arg $kind $arg)) ),+
        ];

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

    // ----- Per-arm CLI arg metadata (consumed by the CLI_ARGS const) -----

    (@cli_arg respond ($payload:expr))                              => { None };
    (@cli_arg dispatch ($msg:expr))                                 => { None };
    (@cli_arg act ($closure:expr))                                  => { None };
    (@cli_arg act_str ($arg_name:literal, $closure:expr))           => { Some($arg_name) };

    // ----- Per-arm dispatch bodies -----

    (@arm respond ($payload:expr), $incoming:ident, $request_id:ident, $app:ident) => {{
        let _ = &$app;
        $incoming
            .responder
            .send(IpcResponse::ok($request_id, Some($payload)));
        Task::none()
    }};

    (@arm dispatch ($msg:expr), $incoming:ident, $request_id:ident, $app:ident) => {{
        let _ = &$app;
        // Fire-and-forget verbs whose result is genuinely async at dispatch
        // time (the new track lands later): acknowledge with a content-free
        // `{"ok": true}` so the shell never sees a silent success, then queue
        // the message for the normal update loop.
        $incoming
            .responder
            .send(IpcResponse::ok($request_id, Some(json!({ "ok": true }))));
        Task::done($msg)
    }};

    (@arm act ($closure:expr), $incoming:ident, $request_id:ident, $app:ident) => {{
        let result: Result<(Task<Message>, serde_json::Value), (&'static str, String)> =
            ($closure)($app);
        match result {
            Ok((task, data)) => {
                $incoming
                    .responder
                    .send(IpcResponse::ok($request_id, Some(data)));
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

    (@arm act_str ($arg_name:literal, $closure:expr), $incoming:ident, $request_id:ident, $app:ident) => {{
        let raw = $incoming.request.args.get($arg_name).and_then(arg_as_text);
        let Some(raw) = raw else {
            $incoming.responder.send(IpcResponse::err(
                $request_id,
                "invalid_args",
                format!("missing required arg: {}", $arg_name),
            ));
            return Task::none();
        };
        let result: Result<(Task<Message>, serde_json::Value), (&'static str, String)> =
            ($closure)($app, raw.as_ref());
        match result {
            Ok((task, data)) => {
                $incoming
                    .responder
                    .send(IpcResponse::ok($request_id, Some(data)));
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

/// Read an `act_str` arg as text: a JSON string verbatim, a JSON number by
/// its decimal text.
///
/// The number case is a compatibility shim, not a convenience. The CLI
/// forwards every positional as a string, but a raw-socket client or an older
/// `nokkvi` binary still sends `{"position": 30}` / `{"value": 0.6}`, and
/// those should keep working rather than reading as "missing required arg".
/// Booleans, nulls, arrays and objects are `None` — there is no honest text
/// for them.
fn arg_as_text(value: &serde_json::Value) -> Option<std::borrow::Cow<'_, str>> {
    match value {
        serde_json::Value::String(s) => Some(std::borrow::Cow::Borrowed(s)),
        serde_json::Value::Number(n) => Some(std::borrow::Cow::Owned(n.to_string())),
        _ => None,
    }
}

/// Map a CLI / wire view-name to the corresponding [`View`] variant. Accepts
/// the lowercase canonical names matching the CLI surface — derived from
/// [`View::ALL`] filtered through [`ipc_switchable`], so the parser and its
/// error listing track the enum automatically. Returns the list of supported
/// names in the error message so the caller (or curious user) can
/// self-correct.
pub(crate) fn parse_view_name(name: &str) -> Result<View, String> {
    let switchable = || View::ALL.iter().copied().filter(|v| ipc_switchable(*v));
    switchable().find(|v| view_name(*v) == name).ok_or_else(|| {
        let supported: Vec<&'static str> = switchable().map(view_name).collect();
        format!(
            "unknown view `{name}` (expected one of: {})",
            supported.join(", ")
        )
    })
}

/// Whether a [`View`] is a valid `switch-view` target. Exhaustive on
/// purpose — a new view must opt in (or out) here explicitly; an enumerated
/// `matches!` would silently return `false` for new variants.
const fn ipc_switchable(view: View) -> bool {
    match view {
        View::Albums
        | View::Queue
        | View::Songs
        | View::Artists
        | View::Genres
        | View::Playlists
        | View::Radios
        | View::Harbour
        | View::Settings => true,
        // Contextual destination — never a switch-view target.
        View::PlaylistEditor => false,
    }
}

/// Canonical lowercase name for a [`View`] — the inverse of [`parse_view_name`]
/// (plus `playlist-editor`, which has no nav tab and isn't a `switch-view`
/// target). Used by the `selection` verb to report the focused view and by
/// [`parse_view_name`] to derive the accepted-name list.
fn view_name(view: View) -> &'static str {
    match view {
        View::Albums => "albums",
        View::Queue => "queue",
        View::Songs => "songs",
        View::Artists => "artists",
        View::Genres => "genres",
        View::Playlists => "playlists",
        View::Radios => "radios",
        View::Harbour => "harbour",
        View::Settings => "settings",
        View::PlaylistEditor => "playlist-editor",
    }
}

/// JSON snapshot of the focused view's centered item for the `selection` verb.
/// Reuses [`Nokkvi::get_center_item_info`] — the same resolver the rating/star
/// hotkeys use — so it's accurate across every list view. Always returns the
/// same key set so scripts see a stable schema; a `null` `kind` means nothing
/// selectable is centered (empty list, Settings, the playlist editor).
fn selection_json(app: &Nokkvi) -> serde_json::Value {
    let view = view_name(app.current_view);
    match app.get_center_item_info() {
        Some(info) => json!({
            "view": view,
            "kind": info.kind.to_string(),
            "name": info.name,
            "artist": info.artist,
            "rating": info.rating,
            "starred": info.starred,
        }),
        None => json!({
            "view": view,
            "kind": null,
            "name": null,
            "artist": null,
            "rating": null,
            "starred": null,
        }),
    }
}

// VolumeCommitted bypasses the 500ms VolumeChanged throttle — discrete
// external commands (playerctl, IPC) must persist immediately so rapid
// presses don't silently drop on next launch. Mirrors the MPRIS SetVolume
// handler — the `MprisEvent::SetVolume` arm in src/update/mpris.rs::handle_mpris.
define_commands! {
    "ping"        => respond  (json!("pong"));
    // Read-only snapshot of transport + now-playing + modes. Pure read of
    // resident state (never a prediction), so it always reports ground truth.
    "status"      => act      (|app: &mut Nokkvi| Ok((Task::none(), status_json(app))));
    "next"        => dispatch (Message::Playback(PlaybackMessage::NextTrack));
    "previous"    => dispatch (Message::Playback(PlaybackMessage::PrevTrack));
    // Transport verbs call the real handler directly (the `PlaybackMessage`
    // arms in playback.rs are 1:1 wrappers, so no side-effects are lost) and
    // then read the post-handler play state back — the echoed value is exactly
    // what the player bar shows. pause/stop and the resume branches flip
    // `playback.playing`/`paused` synchronously; the cold-start branches of
    // play/play-pause start playback asynchronously and leave the flag at
    // "stopped" until the follow-up `Tick`, so those echo `{"state":"stopped"}`
    // until the async start reconciles — consistent with the UI either way.
    "play"        => act      (|app: &mut Nokkvi| {
        let task = app.handle_play();
        Ok((task, json!({ "state": play_state_str(&app.playback) })))
    });
    "pause"       => act      (|app: &mut Nokkvi| {
        let task = app.handle_pause();
        Ok((task, json!({ "state": play_state_str(&app.playback) })))
    });
    "play-pause"  => act      (|app: &mut Nokkvi| {
        let task = app.handle_toggle_play();
        Ok((task, json!({ "state": play_state_str(&app.playback) })))
    });
    "stop"        => act      (|app: &mut Nokkvi| {
        let task = app.handle_stop();
        Ok((task, json!({ "state": play_state_str(&app.playback) })))
    });
    // The seek handlers no-op on radio (no seekable position), so guard here
    // and return an error rather than echoing a false success.
    //
    // Absolute answers `{"position": N}`; relative answers `{"offset": ±N}`,
    // the honest "what was dispatched" shape — the landing point is not known
    // synchronously (ask `status` for it).
    "seek"        => act_str ("position", |app: &mut Nokkvi, raw: &str| {
        if app.active_playback.is_radio() {
            return Err(("unavailable", "seek is not available during radio playback".to_string()));
        }
        let request = parse_seek_arg(raw).map_err(|message| ("invalid_args", message))?;
        match request {
            crate::state::SeekRequest::Absolute(position) => {
                let task = app.handle_seek(position);
                Ok((task, json!({ "position": round_f32(position) })))
            }
            crate::state::SeekRequest::Relative(offset) => {
                let task = app.handle_seek_relative(offset);
                Ok((task, json!({ "offset": round_f32(offset) })))
            }
        }
    });
    "volume"      => act_str ("value", |app: &mut Nokkvi, raw: &str| {
        let current = app.playback.volume;
        let new = parse_volume_change(raw, current)
            .map_err(|message| ("invalid_args", message))?;
        let task = Task::done(Message::Playback(PlaybackMessage::VolumeCommitted(new)));
        Ok((task, json!({ "volume": round_f32(new) })))
    });
    // Toggle-only — matches WM-hotkey ergonomics. Each calls the real handler
    // (radio guard + optimistic flip + toast + async reconcile) and then reads
    // the post-flip mode back, so the echoed value is honest even on the radio
    // no-op (handler leaves the mode unchanged → we report the unchanged value).
    // Arg-taking variants (shuffle on/off, repeat off/one/queue) are deferred
    // until direct-setter PlaybackMessage variants exist. See §6 of new-feats.md.
    "shuffle"     => act      (|app: &mut Nokkvi| {
        let task = app.handle_toggle_random();
        Ok((task, json!({ "random": app.modes.random })))
    });
    "repeat"      => act      (|app: &mut Nokkvi| {
        let task = app.handle_toggle_repeat();
        Ok((task, json!({ "repeat": repeat_str(&app.modes) })))
    });
    "consume"     => act      (|app: &mut Nokkvi| {
        let task = app.handle_toggle_consume();
        Ok((task, json!({ "consume": app.modes.consume })))
    });
    // IPC bypasses handle_clear_queue's "not in queue view" gate — external
    // callers expect `nokkvi clear-queue` to clear from any view. The shared
    // clear_queue_action() lives in src/update/hotkeys/queue.rs.
    "clear-queue" => act      (|app: &mut Nokkvi| Ok((app.clear_queue_action(), json!({ "ok": true }))));
    // Add the focused item (centered song/album/artist/genre/playlist) to the
    // queue — the in-app Shift+A hotkey. `add_to_queue_message()` resolving to
    // Some is the exact condition handle_add_to_queue uses to decide there's an
    // item to enqueue, so gate the echo on it: report the focused item's name,
    // or null when nothing is selected (mirrors the "No item selected" toast).
    // The actual enqueue + "Added 'X' to queue" toast run via the real handler.
    "add-to-queue" => act      (|app: &mut Nokkvi| {
        let will_add = app
            .current_view_page()
            .and_then(|page| page.add_to_queue_message())
            .is_some();
        let added = if will_add {
            app.get_center_item_info().map(|info| info.name)
        } else {
            None
        };
        let task = app.handle_add_to_queue();
        Ok((task, json!({ "added": added })))
    });
    // Remove the centered queue song — the in-app Ctrl+D hotkey. Queue-gated:
    // the centered item only has meaning in the queue view, so this errors
    // elsewhere (unlike clear-queue, which is ungated because it needs no
    // selection). The Queue branch of get_center_item_info resolves the same
    // filtered-center song handle_remove_from_queue removes, so the echo is
    // exact; null when the queue is empty (no song centered).
    "remove-from-queue" => act (|app: &mut Nokkvi| {
        if app.current_view != View::Queue {
            return Err((
                "not_in_queue_view",
                "remove-from-queue only works in the queue view".to_string(),
            ));
        }
        let removed = app.get_center_item_info().map(|info| info.name);
        let task = app.handle_remove_from_queue();
        Ok((task, json!({ "removed": removed })))
    });
    // Switch the top-pane view. The `view` arg is required and validated
    // against the View enum before dispatch; the actual switch goes through
    // the normal NavigationMessage::SwitchView path so view-change side
    // effects (data loads, focus shifts) fire as usual. `raw` is already the
    // canonical lowercase name once parse_view_name accepts it, so echo it.
    "switch-view" => act_str ("view", |_app: &mut Nokkvi, raw: &str| {
        let view = parse_view_name(raw)
            .map_err(|message| ("invalid_args", message))?;
        let task = Task::done(Message::Navigation(NavigationMessage::SwitchView(view)));
        Ok((task, json!({ "view": raw })))
    });
    // Slot-list navigation — the in-app Backspace/Tab/Enter hotkeys, exposed so
    // a WM keybind or script can drive the focused list without focusing the
    // window. Routed through Message::SlotList so the existing handler resolves
    // the focused pane/view and applies all guards (roulette, picker, play
    // gate). The move is async (a further per-page message), so the resulting
    // center can't be echoed here — query `selection` to read where it landed.
    "nav-up"      => dispatch (Message::SlotList(SlotListMessage::NavigateUp));
    "nav-down"    => dispatch (Message::SlotList(SlotListMessage::NavigateDown));
    // Activate the centered item (Enter): plays in Queue/Songs, expands or
    // navigates in Albums/Artists/Genres/Playlists, edits a Settings row.
    "enter"       => dispatch (Message::SlotList(SlotListMessage::ActivateCenter));
    // Read-only: the focused view's currently-centered item (or a null-valued
    // record when nothing is selectable — empty list, Settings, editor).
    "selection"   => act      (|app: &mut Nokkvi| Ok((Task::none(), selection_json(app))));
    // Toggle star on whatever's currently playing — the original seed's pain
    // ("rate from a WM hotkey without focusing the window"). Acts on
    // `scrobble.current_song_id` (authoritative for "the playing track")
    // rather than the slot-list centered item the in-app hotkey targets.
    "love"        => act      (|app: &mut Nokkvi| {
        let song_id = current_playing_song_id(app)?;
        let starred = current_starred(app, &song_id);
        let loved = !starred;
        // Mirror the Shift+L hotkey's in-window toast (handle_toggle_star) so a
        // `nokkvi love` from a WM keybind gives the same visible feedback.
        let marker = if loved { "★ Starred" } else { "☆ Unstarred" };
        let label = format!("{marker}: {}", app.playback.title);
        app.toast_success(label);
        // Confirm the change with a desktop notification (opt-in), for when the
        // window is minimized / on another workspace. Acts on the playing
        // track, so its title/artist are the player-bar fields. The toggle
        // always flips the star state, so — unlike `rate` — there is no no-op
        // to guard against.
        app.notify_love_changed(&app.playback.title, &app.playback.artist, loved);
        let task = app.toggle_star_with_revert_task(song_id, ItemKind::Song, loved);
        Ok((task, json!({ "loved": loved })))
    });
    "rate"        => act_str ("delta", |app: &mut Nokkvi, raw: &str| {
        let song_id = current_playing_song_id(app)?;
        let current = current_rating(app, &song_id);
        let new_rating = parse_rating_change(raw, current)
            .map_err(|message| ("invalid_args", message))?;
        // Mirror the rating hotkey's in-window toast (handle_rating_change) so a
        // `nokkvi rate` from a WM keybind gives the same visible feedback. Acts
        // on the playing track, so its title/artist are the player-bar fields.
        let display_name = if app.playback.artist.is_empty() {
            app.playback.title.clone()
        } else {
            format!("{} - {}", app.playback.title, app.playback.artist)
        };
        app.toast_success(format!("⭐ Rated {display_name}: {new_rating}/5"));
        // Confirm the change with a desktop notification (opt-in). Unlike the
        // in-window toast, this only fires on a real transition: `rate` clamps,
        // so `nokkvi rate +1` at 5/5 (or `rate 3` while already 3) is a no-op
        // and must not push a "Rating updated" popup announcing no change.
        if new_rating as u32 != current {
            app.notify_rating_changed(&app.playback.title, &app.playback.artist, new_rating as u32);
        }
        let task = app.set_item_rating_task(song_id, ItemKind::Song, new_rating, current);
        Ok((task, json!({ "rating": new_rating })))
    });
    // Server queue sync (OpenSubsonic indexBasedQueue) — the queue header's
    // push/pull pair, exposed headless. Guarded exactly like the buttons
    // (capability + radio; the CLI has no hidden-button safety), and push
    // refuses an empty queue (an empty save CLEARS the server's stored copy).
    // The network round-trip is async, so success means "dispatched" — a
    // later failure surfaces via the in-window toast, same as the buttons.
    "queue-push"  => act      (|app: &mut Nokkvi| {
        guard_queue_sync(app)?;
        if app.library.queue_songs.is_empty() {
            return Err(("empty_queue", "queue is empty — nothing to push".to_string()));
        }
        let tracks = app.library.queue_songs.len();
        let task = app.push_queue_task();
        Ok((task, json!({ "dispatched": "push", "tracks": tracks })))
    });
    "queue-pull"  => act      (|app: &mut Nokkvi| {
        guard_queue_sync(app)?;
        let task = app.pull_queue_task();
        Ok((task, json!({ "dispatched": "pull" })))
    });
    // Window: also the path MPRIS Raise and a bare second launch take.
    "show"        => act      (|app: &mut Nokkvi| {
        let (task, outcome) = app.show_window();
        Ok((task, json!({ "window": outcome.as_str() })))
    });
    // Layout: the same entry point F11, the corner icon and the panel menu use.
    "theater"     => act      (|app: &mut Nokkvi| {
        if app.screen != crate::Screen::Home {
            return Err(("unavailable", "theater mode needs a logged-in window".to_string()));
        }
        // Closed to the tray: entering now would greet the next window.
        if !app.theater.active && app.main_window_id.is_none() {
            return Err(("unavailable", "theater mode needs an open window".to_string()));
        }
        let task = app.toggle_theater();
        Ok((task, json!({ "theater": app.theater.active })))
    });
    // Visualizer: the MilkDrop preset controls the keys and panel menus use.
    "preset"      => act_str ("action", |app: &mut Nokkvi, raw: &str| {
        if app.screen != crate::Screen::Home {
            return Err(("unavailable", "preset controls need a logged-in window".to_string()));
        }
        let action = crate::update::milkdrop::PresetAction::parse(raw).ok_or_else(|| {
            (
                "invalid_args",
                format!(
                    "unknown preset action `{raw}`; expected one of: {}",
                    crate::update::milkdrop::PresetAction::WORDS.join(", ")
                ),
            )
        })?;
        app.milkdrop_ipc_control(action)
    });
}

/// Shared guard for the queue-sync verbs: the server must advertise the
/// `indexBasedQueue` extension and radio must be inactive — mirrors the
/// header buttons' visibility gate (during radio the engine position is a
/// stream offset, and a pull would fight the radio engine mode).
fn guard_queue_sync(app: &Nokkvi) -> Result<(), (&'static str, String)> {
    if !app.supports_index_based_queue() {
        return Err((
            "unsupported",
            "server does not advertise the indexBasedQueue extension".to_string(),
        ));
    }
    if app.active_playback.is_radio() {
        return Err((
            "unavailable",
            "queue sync is not available during radio playback".to_string(),
        ));
    }
    Ok(())
}

/// Resolve the song id of whatever's currently playing, returning the
/// `no_playing_track` IPC error when nothing's loaded. Used by every
/// playing-track-scoped verb (`love`, `rate`, future `current` queries).
fn current_playing_song_id(app: &Nokkvi) -> Result<String, (&'static str, String)> {
    app.scrobble
        .current_song_id
        .clone()
        .ok_or_else(|| ("no_playing_track", "no track is currently playing".into()))
}

/// Best-effort lookup of the currently-known starred state for a song id,
/// using the queue snapshot. Falls back to `false` when the song isn't in
/// the queue (rare edge case — e.g. server-side race during track change);
/// the API call still goes through, and the optimistic UI update gets
/// reverted on failure either way.
fn current_starred(app: &Nokkvi, song_id: &str) -> bool {
    app.library
        .queue_songs
        .iter()
        .find(|s| s.id == song_id)
        .is_some_and(|s| s.starred)
}

/// Best-effort lookup of the current rating (0–5) for a song id from the
/// queue snapshot. Falls back to `0` when the song isn't in the queue.
fn current_rating(app: &Nokkvi, song_id: &str) -> u32 {
    app.library
        .queue_songs
        .iter()
        .find(|s| s.id == song_id)
        .and_then(|s| s.rating)
        .unwrap_or(0)
}

/// Round an `f32` to 3 decimal places (as `f64`) for clean JSON echoes. An
/// `f32` promoted straight to `f64` serializes its full binary tail
/// (`0.6f32` → `0.6000000238…`); volumes (0.0..=1.0) and seek positions
/// (seconds) only need 0.001 fidelity, so round before echoing.
fn round_f32(v: f32) -> f64 {
    (f64::from(v) * 1000.0).round() / 1000.0
}

/// Canonical lowercase playback-state token for IPC/`status` replies:
/// `"playing"`, `"paused"`, or `"stopped"`. This is the scriptable wire
/// vocabulary — distinct from the capitalized player-bar toast labels.
fn play_state_str(playback: &crate::state::PlaybackState) -> &'static str {
    if playback.playing && !playback.paused {
        "playing"
    } else if playback.paused {
        "paused"
    } else {
        "stopped"
    }
}

/// Canonical lowercase repeat-mode token: `"one"`, `"queue"`, or `"off"`.
/// The single source of truth for rendering `(repeat, repeat_queue)` on the
/// wire, so the `repeat` and `status` verbs can never disagree.
fn repeat_str(modes: &crate::state::PlaybackModes) -> &'static str {
    match (modes.repeat, modes.repeat_queue) {
        (true, false) => "one",
        (false, true) => "queue",
        // (false, false) is off; (true, true) is an invalid state treated as off.
        _ => "off",
    }
}

/// Flat JSON snapshot for the `status` query verb: now-playing track, transport
/// state, volume, and the three playback modes — all read synchronously from
/// resident app state. Unlike the toggle verbs' optimistic echo, this reports
/// ground-truth current state and never predicts.
fn status_json(app: &Nokkvi) -> serde_json::Value {
    json!({
        "state": play_state_str(&app.playback),
        "title": app.playback.title,
        "artist": app.playback.artist,
        "album": app.playback.album,
        "position": app.playback.position,
        "duration": app.playback.duration,
        "volume": round_f32(app.playback.volume),
        "random": app.modes.random,
        "repeat": repeat_str(&app.modes),
        "consume": app.modes.consume,
        "theater": app.theater.active,
        "visualizer": app.engine.visualization_mode.to_string(),
        "preset": app.milkdrop.on_screen,
    })
}

/// Parse a seek-arg string into a [`SeekRequest`](crate::state::SeekRequest).
/// Two accepted shapes, matching `volume` and `rate`:
///
/// - **Relative**: `"+N"` / `"-N"` — move N seconds from wherever the engine
///   is. `"+0"` / `"-0"` are no-ops by construction. Past the end ends the
///   track (the engine clamps), past the start parks at 0:00.
/// - **Absolute**: `"N"` — seek to N seconds from the start.
///
/// Non-finite values are rejected rather than clamped: `nan` and `inf` are
/// typos, not intents, and the engine would silently turn them into 0 or the
/// end of the track.
fn parse_seek_arg(raw: &str) -> Result<crate::state::SeekRequest, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("seek arg `position` must not be empty".into());
    }
    let parsed = raw.parse::<f32>().map_err(|_| {
        format!(
            "seek arg `position` `{raw}` must be a number of seconds, optionally \
             ±-prefixed for a relative offset"
        )
    })?;
    if !parsed.is_finite() {
        return Err(format!(
            "seek arg `position` `{raw}` must be a finite number"
        ));
    }
    let first = raw.as_bytes()[0];
    Ok(if first == b'+' || first == b'-' {
        crate::state::SeekRequest::Relative(parsed)
    } else {
        crate::state::SeekRequest::Absolute(parsed)
    })
}

/// Parse a volume-arg string into a final 0.0..=1.0 value. Two accepted shapes:
///
/// - **Delta**: `"+N"` / `"-N"` (e.g. `"+0.05"`, `"-0.1"`) — added to the
///   current volume, then clamped to the 0.0..=1.0 range. Delta clamps
///   silently rather than erroring so repeated `volume +0.05` keypresses at
///   the ceiling are a no-op instead of a stream of errors.
/// - **Absolute**: `"N"` (e.g. `"0.5"`) — replaces the current volume.
///   Out-of-range absolutes (e.g. `"1.5"`) error rather than clamp, so a
///   typo doesn't silently produce a different volume than asked.
fn parse_volume_change(raw: &str, current: f32) -> Result<f32, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("volume arg `value` must not be empty".into());
    }

    let first = raw.as_bytes()[0];
    if first == b'+' || first == b'-' {
        let delta = raw.parse::<f32>().map_err(|_| {
            format!(
                "volume arg `value` `{raw}` must be a number, optionally \
                 ±-prefixed for delta"
            )
        })?;
        Ok((current + delta).clamp(0.0, 1.0))
    } else {
        let abs = raw.parse::<f32>().map_err(|_| {
            format!(
                "volume arg `value` `{raw}` must be a number, optionally \
                 ±-prefixed for delta"
            )
        })?;
        if !(0.0..=1.0).contains(&abs) {
            return Err(format!("absolute volume `{abs}` out of range (0.0..=1.0)"));
        }
        Ok(abs)
    }
}

/// Parse a rate-arg string into a final 0..=5 rating. Two accepted shapes:
///
/// - **Delta**: `"+N"` / `"-N"` — added to the current rating, clamped to
///   the 0..=5 range. `"+0"` and `"-0"` are no-ops by construction.
/// - **Absolute**: `"0"`–`"5"` — replaces the current rating outright.
///   Out-of-range absolute values (e.g. `"7"`) error rather than clamp,
///   so a typo doesn't silently produce a different rating than asked.
fn parse_rating_change(raw: &str, current: u32) -> Result<usize, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("rate arg `delta` must not be empty".into());
    }

    let first = raw.as_bytes()[0];
    if first == b'+' || first == b'-' {
        let delta = raw
            .parse::<i32>()
            .map_err(|_| format!("rate arg `delta` `{raw}` must be ±integer or 0..=5"))?;
        let new = (current as i32 + delta).clamp(0, 5);
        Ok(new as usize)
    } else {
        let abs = raw
            .parse::<u32>()
            .map_err(|_| format!("rate arg `delta` `{raw}` must be ±integer or 0..=5"))?;
        if abs > 5 {
            return Err(format!("absolute rating `{abs}` out of range (0..=5)"));
        }
        Ok(abs as usize)
    }
}
