//! Tests for the IPC command dispatcher.
//!
//! These exercise `update::ipc::handle` synchronously against a synthesized
//! `IpcIncoming`. The socket transport, the iced subscription, and the
//! `Sipper` wrapping are all covered separately (`nokkvi-ipc/tests/round_trip.rs`
//! for the transport; iced owns the subscription path).

use nokkvi_ipc::{IpcRequest, IpcResponse, PROTOCOL_VERSION};
use serde_json::json;
use tokio::sync::oneshot;

use crate::{
    app_message::Message,
    services::ipc::{IpcIncoming, IpcResponder},
    test_helpers::test_app,
};

fn make_incoming(command: &str) -> (IpcIncoming, oneshot::Receiver<IpcResponse>) {
    make_incoming_with_args(command, serde_json::Value::Null)
}

fn make_incoming_with_args(
    command: &str,
    args: serde_json::Value,
) -> (IpcIncoming, oneshot::Receiver<IpcResponse>) {
    let (tx, rx) = oneshot::channel::<IpcResponse>();
    let request = IpcRequest {
        protocol_version: PROTOCOL_VERSION,
        request_id: 7,
        command: command.to_string(),
        args,
    };
    let incoming = IpcIncoming {
        request,
        responder: IpcResponder::new(tx),
    };
    (incoming, rx)
}

fn drive_with_args(command: &str, args: serde_json::Value) -> IpcResponse {
    let mut app = test_app();
    let (incoming, rx) = make_incoming_with_args(command, args);

    let dispatched = app.update(Message::Ipc(Box::new(incoming)));
    drop(dispatched);

    rx.blocking_recv()
        .unwrap_or_else(|_| panic!("responder must fire for {command} command"))
}

/// Drive one verb through the dispatcher and return the response that the
/// responder receives. Shared by every fire-and-forget verb test.
fn drive(command: &str) -> IpcResponse {
    let mut app = test_app();
    let (incoming, rx) = make_incoming(command);

    let dispatched = app.update(Message::Ipc(Box::new(incoming)));
    drop(dispatched);

    rx.blocking_recv()
        .unwrap_or_else(|_| panic!("responder must fire for {command} command"))
}

#[test]
fn ping_command_yields_pong_payload() {
    let resp = drive("ping");
    assert_eq!(resp.request_id, 7);
    assert_eq!(resp.data, Some(json!("pong")));
    assert!(resp.error.is_none());
}

#[test]
fn fire_and_forget_verbs_all_respond_ok_with_no_payload() {
    // Single table-driven test for every Phase 0 + Phase 1 verb whose contract
    // is "ack now, side-effects later" — adding the next verb is one row, not
    // a new test function. Pins the IPC-layer contract (responder fires,
    // request_id echoed, data empty, no error). Playback side-effects are
    // covered by the existing playback handler tests.
    for verb in [
        "next",
        "previous",
        "play",
        "pause",
        "play-pause",
        "stop",
        "shuffle",
        "repeat",
        "consume",
    ] {
        let resp = drive(verb);
        assert_eq!(resp.request_id, 7, "{verb}: request_id must echo");
        assert!(
            resp.data.is_none(),
            "{verb}: fire-and-forget verbs should not carry data"
        );
        assert!(resp.error.is_none(), "{verb}: should not error");
    }
}

#[test]
fn unknown_command_yields_structured_error() {
    let resp = drive("bogus-verb");
    assert_eq!(resp.request_id, 7);
    assert!(resp.data.is_none());
    let err = resp.error.expect("error populated");
    assert_eq!(err.code, "unknown_command");
    assert!(err.message.contains("bogus-verb"));
}

#[test]
fn seek_accepts_f32_position_arg() {
    let resp = drive_with_args("seek", json!({"position": 42.5}));
    assert_eq!(resp.request_id, 7);
    assert!(resp.data.is_none());
    assert!(resp.error.is_none());
}

#[test]
fn seek_accepts_integer_arg_via_json_number_coercion() {
    // JSON `30` (integer) should still parse as f32 — covers the common case
    // where a CLI user types `nokkvi seek 30` without a decimal.
    let resp = drive_with_args("seek", json!({"position": 30}));
    assert!(resp.error.is_none());
}

#[test]
fn seek_missing_arg_returns_invalid_args_error() {
    let resp = drive_with_args("seek", json!({}));
    let err = resp.error.expect("missing arg must error");
    assert_eq!(err.code, "invalid_args");
    assert!(err.message.contains("position"));
}

#[test]
fn seek_wrong_arg_type_returns_invalid_args_error() {
    let resp = drive_with_args("seek", json!({"position": "thirty"}));
    let err = resp.error.expect("non-numeric arg must error");
    assert_eq!(err.code, "invalid_args");
    assert!(err.message.contains("must be a number"));
}

#[test]
fn volume_accepts_f32_value_arg() {
    let resp = drive_with_args("volume", json!({"value": 0.6}));
    assert!(resp.error.is_none());
}

#[test]
fn volume_missing_arg_returns_invalid_args_error() {
    let resp = drive_with_args("volume", json!({}));
    let err = resp.error.expect("missing arg must error");
    assert_eq!(err.code, "invalid_args");
    assert!(err.message.contains("value"));
}

#[test]
fn clear_queue_responds_ok_from_any_view() {
    // `clear-queue` is a try_act verb that bypasses the in-app handler's
    // "Not in queue view" gate. The IPC contract is: responder fires ok with
    // no payload regardless of which view the running instance is on. The
    // actual backend wipe is exercised by the playback-handler tests.
    let resp = drive("clear-queue");
    assert_eq!(resp.request_id, 7);
    assert!(resp.data.is_none());
    assert!(resp.error.is_none());
}

#[test]
fn switch_view_with_valid_view_name_responds_ok() {
    for view_name in [
        "albums",
        "queue",
        "songs",
        "artists",
        "genres",
        "playlists",
        "radios",
        "settings",
    ] {
        let resp = drive_with_args("switch-view", json!({"view": view_name}));
        assert!(
            resp.error.is_none(),
            "{view_name}: should be a valid view target"
        );
        assert!(resp.data.is_none());
    }
}

#[test]
fn switch_view_missing_view_arg_returns_invalid_args_error() {
    let resp = drive_with_args("switch-view", json!({}));
    let err = resp.error.expect("missing view arg must error");
    assert_eq!(err.code, "invalid_args");
    assert!(err.message.contains("view"));
}

#[test]
fn switch_view_unknown_view_returns_invalid_args_error_listing_options() {
    let resp = drive_with_args("switch-view", json!({"view": "favorites"}));
    let err = resp.error.expect("unknown view must error");
    assert_eq!(err.code, "invalid_args");
    // The error message lists the supported names so the caller can self-correct.
    assert!(err.message.contains("favorites"));
    assert!(err.message.contains("albums"));
    assert!(err.message.contains("settings"));
}

#[test]
fn love_with_no_playing_track_returns_no_playing_track_error() {
    // Default test_app() has scrobble.current_song_id = None, so `love`
    // should bail with a structured error rather than dispatching anything.
    let resp = drive("love");
    let err = resp.error.expect("no playing track → error");
    assert_eq!(err.code, "no_playing_track");
    assert!(err.message.contains("no track"));
}

#[test]
fn love_with_playing_track_responds_ok() {
    // Plant a song_id in the scrobble state so the playing-track resolver
    // sees something live. The actual API call goes through shell_task but
    // never executes (no AppService in test_app), and the optimistic local
    // toggle has nothing to find in the empty queue — both paths are no-ops
    // at the level we're pinning here (IPC responder fires ok with no data).
    let mut app = test_app();
    app.scrobble.current_song_id = Some("song-123".into());
    let (incoming, rx) = make_incoming("love");

    let dispatched = app.update(Message::Ipc(Box::new(incoming)));
    drop(dispatched);

    let resp = rx.blocking_recv().expect("responder must fire for love");
    assert_eq!(resp.request_id, 7);
    assert!(resp.data.is_none());
    assert!(resp.error.is_none());
}

#[test]
fn rate_with_no_playing_track_returns_no_playing_track_error() {
    let resp = drive_with_args("rate", json!({"delta": "+1"}));
    let err = resp.error.expect("no playing track → error");
    assert_eq!(err.code, "no_playing_track");
}

#[test]
fn rate_missing_arg_returns_invalid_args_error() {
    // Even with a playing track, an absent delta arg should error before
    // reaching the playing-track resolver. (Order: arg check first.)
    let mut app = test_app();
    app.scrobble.current_song_id = Some("song-123".into());
    let (incoming, rx) = make_incoming_with_args("rate", json!({}));

    let dispatched = app.update(Message::Ipc(Box::new(incoming)));
    drop(dispatched);

    let resp = rx.blocking_recv().expect("responder must fire");
    let err = resp.error.expect("missing arg must error");
    assert_eq!(err.code, "invalid_args");
    assert!(err.message.contains("delta"));
}

fn drive_rate_with_song(delta: &str) -> IpcResponse {
    let mut app = test_app();
    app.scrobble.current_song_id = Some("song-123".into());
    let (incoming, rx) = make_incoming_with_args("rate", json!({"delta": delta}));

    let dispatched = app.update(Message::Ipc(Box::new(incoming)));
    drop(dispatched);

    rx.blocking_recv().expect("responder must fire for rate")
}

#[test]
fn rate_accepts_delta_strings() {
    for delta in ["+1", "-1", "+2", "-2", "+0", "-0"] {
        let resp = drive_rate_with_song(delta);
        assert!(resp.error.is_none(), "{delta}: should accept");
    }
}

#[test]
fn rate_accepts_absolute_zero_through_five() {
    for abs in ["0", "1", "2", "3", "4", "5"] {
        let resp = drive_rate_with_song(abs);
        assert!(resp.error.is_none(), "{abs}: should accept");
    }
}

#[test]
fn rate_rejects_absolute_above_five() {
    let resp = drive_rate_with_song("7");
    let err = resp.error.expect("out-of-range absolute must error");
    assert_eq!(err.code, "invalid_args");
    assert!(err.message.contains("out of range"));
}

#[test]
fn rate_rejects_non_numeric_arg() {
    let resp = drive_rate_with_song("loud");
    let err = resp.error.expect("non-numeric must error");
    assert_eq!(err.code, "invalid_args");
}

#[test]
fn rate_rejects_empty_delta() {
    let resp = drive_rate_with_song("");
    let err = resp.error.expect("empty delta must error");
    assert_eq!(err.code, "invalid_args");
}
