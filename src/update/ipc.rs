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
//! - `act_f32 (<arg_name>, <closure>)` — `act` with a single named `f32`
//!   argument extracted from `incoming.request.args`. Closure receives
//!   `&mut Nokkvi` and the `f32` and returns the same `(Task, data)` result,
//!   so it can echo the resulting value (and gate on app state — e.g. `seek`
//!   rejects radio playback). Missing or non-numeric args return an
//!   `invalid_args` error response before the closure runs.
//! - `act (<closure>)` — closure receives `&mut Nokkvi`, returns
//!   `Result<(Task<Message>, serde_json::Value), (&'static str, String)>`. On
//!   `Ok((task, data))` the responder gets `data` and the task is returned; on
//!   `Err((code, message))` the responder gets an error response and
//!   `Task::none()`. Use this for verbs that need direct app-state access and
//!   take no named arg — including the toggle/transport verbs, which call the
//!   real handler and then read the resulting state back into `data` (the
//!   `PlaybackMessage` arms are 1:1 handler wrappers, so calling the handler
//!   directly loses no side-effects).
//! - `act_str (<arg_name>, <closure>)` — `act` with an auto-extracted string
//!   arg. Closure receives `&mut Nokkvi` and `&str` (the already-extracted arg
//!   value) and returns the same `(Task, data)` result. Missing-arg returns
//!   `invalid_args` before the closure runs. **Use this** whenever a verb
//!   takes a single named string arg — it (a) keeps the closure focused and
//!   (b) lets the macro publish the CLI arg name, which prevents the
//!   macro/CLI-parser drift class that plagued earlier designs.
//!
//! ## Decision rule (which arm shape to pick)
//!
//! 1. **No args, result genuinely async at dispatch time** → `dispatch`
//!    (acks `{"ok": true}`).
//! 2. **No args, const compute-and-return payload (no app access)** →
//!    `respond`.
//! 3. **Single `f32` arg, needs `&mut Nokkvi`** → `act_f32`.
//! 4. **Single string arg, needs `&mut Nokkvi`** → `act_str`.
//! 5. **No args but needs `&mut Nokkvi` (gate-bypass / app-state read /
//!    resulting-state echo)** → `act` with `|app|`.
//! 6. **Multiple args or a complex arg shape** → `act` with manual extraction
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
//! |               |           | `volume,random,repeat,consume}` — pure read.   |
//! | `next`        | dispatch  | `{"ok":true}`; `NextTrack` (new track async).  |
//! | `previous`    | dispatch  | `{"ok":true}`; `PrevTrack` (new track async).  |
//! | `play`        | act       | `{"state":…}`; calls `handle_play`.            |
//! | `pause`       | act       | `{"state":"paused"}`; calls `handle_pause`.    |
//! | `play-pause`  | act       | `{"state":…}`; calls `handle_toggle_play`.     |
//! | `stop`        | act       | `{"state":"stopped"}`; calls `handle_stop`.    |
//! | `seek`        | act_f32   | `{"position":N}`; arg `position` (seconds).     |
//! |               |           | `unavailable` error during radio playback.      |
//! | `volume`      | act_str   | `{"volume":N}`; arg `value`. `"+N"`/`"-N"`     |
//! |               |           | (delta, clamped 0.0..=1.0) or `"N"` absolute  |
//! |               |           | (0.0..=1.0, rejected if out of range). Routes |
//! |               |           | through `VolumeCommitted` (bypasses throttle).|
//! | `shuffle`     | act       | `{"random":bool}`; calls `handle_toggle_random`.|
//! | `repeat`      | act       | `{"repeat":"off"\|"one"\|"queue"}`; cycles.     |
//! | `consume`     | act       | `{"consume":bool}`; calls `handle_toggle_consume`.|
//! | `clear-queue` | act       | `{"ok":true}`; `clear_queue_action()` (gate-free).|
//! | `switch-view` | act_str   | `{"view":name}`; arg `view` (one of `albums`/  |
//! |               |           | `queue`/`songs`/`artists`/`genres`/`playlists`/|
//! |               |           | `radios`/`settings`). Invalid → `invalid_args`.|
//! | `love`        | act       | `{"loved":bool}`; toggle star on playing track.|
//! |               |           | `no_playing_track` error if nothing playing.   |
//! | `rate`        | act_str   | `{"rating":0..5}`; arg `delta` `"+N"`/`"-N"`    |
//! |               |           | (delta, clamped 0..=5) or `"0".."5"` absolute. |
//! |               |           | Same playing-track rules as `love`.            |

use iced::Task;
use nokkvi_data::types::ItemKind;
use nokkvi_ipc::IpcResponse;
use serde_json::json;

use crate::{
    Nokkvi, View,
    app_message::{Message, NavigationMessage, PlaybackMessage},
    services::ipc::IpcIncoming,
};

/// CLI-side JSON wrapping shape for a verb's positional arg. The macro
/// emits one of these per arg-taking verb in `CLI_ARGS`;
/// [`crate::build_ipc_cli_args`] reads them to construct the `args` object
/// without per-verb match arms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CliArgType {
    /// CLI string is parsed as a JSON number first; unparseable inputs
    /// fall back to forwarding as a raw string (lets the server emit the
    /// precise "must be a number" error rather than "missing required arg").
    Number,
    /// CLI string forwarded verbatim as a JSON string. The server-side
    /// closure owns parsing.
    String,
}

/// Generate the IPC dispatcher plus three companion consts that
/// `main.rs`'s argv parser reads to stay drift-free:
///
/// - `KNOWN_COMMANDS: &[&str]` — every declared verb name.
/// - `CLI_ARGS: &[(&'static str, Option<(&'static str, CliArgType)>)]` —
///   per verb, the CLI arg-name and forwarding type (or `None` for no-arg
///   verbs).
///
/// See the module-level docs for the five arm shapes and the decision rule.
macro_rules! define_commands {
    (
        $( $verb:literal => $kind:ident $arg:tt ; )+ $(,)?
    ) => {
        pub(crate) const KNOWN_COMMANDS: &[&str] = &[ $( $verb ),+ ];

        pub(crate) const CLI_ARGS: &[(
            &'static str,
            Option<(&'static str, CliArgType)>,
        )] = &[
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
    (@cli_arg act_f32 ($arg_name:literal, $closure:expr))           => {
        Some(($arg_name, CliArgType::Number))
    };
    (@cli_arg act_str ($arg_name:literal, $closure:expr))           => {
        Some(($arg_name, CliArgType::String))
    };

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

    (@arm act_f32 ($arg_name:literal, $closure:expr), $incoming:ident, $request_id:ident, $app:ident) => {{
        match extract_f32_arg(&$incoming.request.args, $arg_name) {
            Ok(value) => {
                let result: Result<(Task<Message>, serde_json::Value), (&'static str, String)> =
                    ($closure)($app, value);
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
        let raw = $incoming
            .request
            .args
            .get($arg_name)
            .and_then(|v| v.as_str());
        let Some(raw) = raw else {
            $incoming.responder.send(IpcResponse::err(
                $request_id,
                "invalid_args",
                format!("missing required arg: {}", $arg_name),
            ));
            return Task::none();
        };
        let result: Result<(Task<Message>, serde_json::Value), (&'static str, String)> =
            ($closure)($app, raw);
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
    // handle_seek no-ops on radio (no seekable position), so guard here and
    // return an error rather than echoing a false `{"position": …}` success.
    "seek"        => act_f32 ("position", |app: &mut Nokkvi, value: f32| {
        if app.active_playback.is_radio() {
            return Err(("unavailable", "seek is not available during radio playback".to_string()));
        }
        let task = app.handle_seek(value);
        Ok((task, json!({ "position": round_f32(value) })))
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
        let task = app.set_item_rating_task(song_id, ItemKind::Song, new_rating, current);
        Ok((task, json!({ "rating": new_rating })))
    });
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
