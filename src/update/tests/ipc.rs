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
fn every_verb_carries_a_data_payload_so_success_is_never_silent() {
    // The MPD "no command is silent" discipline: every successful verb must
    // return a non-null `data` payload so the CLI always prints something.
    // This pins that invariant across the whole no-arg set; the exact shapes
    // are asserted by the per-verb tests below.
    for verb in [
        "ping",
        "status",
        "next",
        "previous",
        "play",
        "pause",
        "play-pause",
        "stop",
        "shuffle",
        "repeat",
        "consume",
        "clear-queue",
    ] {
        let resp = drive(verb);
        assert_eq!(resp.request_id, 7, "{verb}: request_id must echo");
        assert!(resp.error.is_none(), "{verb}: should not error");
        let data = resp
            .data
            .unwrap_or_else(|| panic!("{verb}: must carry data"));
        assert!(
            !data.is_null(),
            "{verb}: data must not be null (success is never silent)"
        );
    }
}

#[test]
fn async_result_verbs_acknowledge_with_ok_true() {
    // `next`/`previous` change the current track asynchronously, so there is no
    // resulting state to echo synchronously — they acknowledge with {"ok":true}.
    for verb in ["next", "previous"] {
        let resp = drive(verb);
        assert_eq!(resp.data, Some(json!({ "ok": true })), "{verb}");
        assert!(resp.error.is_none(), "{verb}");
    }
}

#[test]
fn consume_echoes_resulting_state() {
    // test_app() starts with consume off and a non-radio active_playback, so
    // the toggle flips it on and the response echoes the post-flip value.
    let resp = drive("consume");
    assert_eq!(resp.data, Some(json!({ "consume": true })));
    assert!(resp.error.is_none());
}

#[test]
fn shuffle_echoes_resulting_random_state() {
    let resp = drive("shuffle");
    assert_eq!(resp.data, Some(json!({ "random": true })));
    assert!(resp.error.is_none());
}

#[test]
fn repeat_echoes_resulting_mode_token() {
    // Cycle starts at off → one on the first toggle.
    let resp = drive("repeat");
    assert_eq!(resp.data, Some(json!({ "repeat": "one" })));
    assert!(resp.error.is_none());
}

#[test]
fn pause_and_stop_echo_play_state() {
    // handle_pause sets paused synchronously; handle_stop clears both. These
    // are deterministic regardless of whether a track is loaded.
    assert_eq!(drive("pause").data, Some(json!({ "state": "paused" })));
    assert_eq!(drive("stop").data, Some(json!({ "state": "stopped" })));
}

#[test]
fn play_and_toggle_play_echo_a_state_key() {
    // With test_app()'s empty queue, the exact resulting state depends on the
    // cold-start path, so pin the shape (a "state" string, no error) rather
    // than the value.
    for verb in ["play", "play-pause"] {
        let resp = drive(verb);
        assert!(resp.error.is_none(), "{verb}");
        let data = resp
            .data
            .unwrap_or_else(|| panic!("{verb}: must carry data"));
        assert!(
            data.get("state").and_then(|v| v.as_str()).is_some(),
            "{verb}: data must carry a string `state` key, got {data}"
        );
    }
}

#[test]
fn navigation_verbs_acknowledge_with_ok_true() {
    // nav-up/nav-down/enter route an existing SlotListMessage through the normal
    // loop (fire-and-forget); the move/activation is async, so they ack.
    for verb in ["nav-up", "nav-down", "enter"] {
        let resp = drive(verb);
        assert_eq!(resp.data, Some(json!({ "ok": true })), "{verb}");
        assert!(resp.error.is_none(), "{verb}");
    }
}

#[test]
fn selection_returns_a_stable_record_with_the_focused_view() {
    // Pure read of the centered item. test_app()'s library is empty, so nothing
    // is selectable → `kind` is null, but the key set (and `view`) is stable.
    let resp = drive("selection");
    assert!(resp.error.is_none());
    let data = resp.data.expect("selection must carry data");
    assert!(
        data.get("view").and_then(|v| v.as_str()).is_some(),
        "selection must report the focused view, got: {data}"
    );
    for key in ["kind", "name", "artist", "rating", "starred"] {
        assert!(
            data.get(key).is_some(),
            "selection must keep a stable schema (missing `{key}`): {data}"
        );
    }
    // Empty library → nothing centered.
    assert_eq!(data.get("kind"), Some(&serde_json::Value::Null));
}

#[test]
fn status_returns_a_full_state_snapshot() {
    let resp = drive("status");
    assert!(resp.error.is_none());
    let data = resp.data.expect("status must carry data");
    for key in [
        "state", "title", "artist", "album", "position", "duration", "volume", "random", "repeat",
        "consume",
    ] {
        assert!(
            data.get(key).is_some(),
            "status data missing `{key}` key: {data}"
        );
    }
    // status is a pure read — it must NOT mutate modes (consume stays off).
    assert_eq!(data.get("consume"), Some(&json!(false)));
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

/// Catalog tripwire — pins the exact set of verbs the macro publishes.
/// Adding a verb without updating this list (and the doc tables that
/// readers consult) becomes a compile-then-test failure rather than a
/// silent drift between the macro and its surrounding documentation.
#[test]
fn known_commands_lists_the_documented_phase0_through_phase2_set() {
    use std::collections::BTreeSet;

    let expected: BTreeSet<&str> = [
        // Phase 0
        "ping",
        // Query
        "status",
        // Phase 1
        "next",
        "previous",
        "play",
        "pause",
        "play-pause",
        "stop",
        "seek",
        "volume",
        "shuffle",
        "repeat",
        // Phase 2
        "consume",
        "clear-queue",
        "switch-view",
        "love",
        "rate",
        // Navigation
        "nav-up",
        "nav-down",
        "enter",
        "selection",
    ]
    .into_iter()
    .collect();

    let actual: BTreeSet<&str> = crate::update::IPC_KNOWN_COMMANDS.iter().copied().collect();

    assert_eq!(
        actual, expected,
        "macro-generated KNOWN_COMMANDS drifted from the documented catalog \
         (extra/missing verbs are the symmetric difference of the two sets)"
    );
}

/// CLI arg routing is macro-driven via `IPC_CLI_ARGS`. Adding a verb that
/// takes an arg without using `with_f32` / `act_str` would silently
/// land it in `IPC_CLI_ARGS` as `None`, so the CLI would forward
/// `Value::Null` and the server would always return `invalid_args`. This
/// pins the exact set of verbs the CLI knows how to wrap and which arg
/// name it forwards, so a future macro-row drift trips a test.
#[test]
fn cli_args_const_lists_every_arg_taking_verb() {
    use std::collections::BTreeMap;

    let actual: BTreeMap<&str, &str> = crate::update::IPC_CLI_ARGS
        .iter()
        .filter_map(|(verb, spec)| spec.map(|(arg, _)| (*verb, arg)))
        .collect();

    let expected: BTreeMap<&str, &str> = [
        ("seek", "position"),
        ("volume", "value"),
        ("switch-view", "view"),
        ("rate", "delta"),
    ]
    .into_iter()
    .collect();

    assert_eq!(
        actual, expected,
        "macro-generated CLI_ARGS drifted from the documented arg-taking set"
    );
}

#[test]
fn seek_accepts_f32_position_arg_and_echoes_it() {
    let resp = drive_with_args("seek", json!({"position": 42.5}));
    assert_eq!(resp.request_id, 7);
    assert_eq!(resp.data, Some(json!({ "position": 42.5 })));
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
fn seek_during_radio_returns_unavailable_error_not_a_false_echo() {
    // handle_seek no-ops on radio playback, so the verb must surface an error
    // rather than echo a `{"position": …}` success that never happened.
    let mut app = test_app();
    app.active_playback = crate::state::ActivePlayback::Radio(crate::state::RadioPlaybackState {
        station: nokkvi_data::types::radio_station::RadioStation {
            id: "r".into(),
            name: "n".into(),
            stream_url: "u".into(),
            home_page_url: None,
        },
        icy_artist: None,
        icy_title: None,
        icy_url: None,
    });
    let (incoming, rx) = make_incoming_with_args("seek", json!({"position": 42.0}));

    let dispatched = app.update(Message::Ipc(Box::new(incoming)));
    drop(dispatched);

    let resp = rx.blocking_recv().expect("responder must fire for seek");
    assert!(resp.data.is_none());
    let err = resp.error.expect("radio seek must error");
    assert_eq!(err.code, "unavailable");
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
fn volume_accepts_absolute_string_arg_and_echoes_committed_value() {
    let resp = drive_with_args("volume", json!({"value": "0.6"}));
    assert!(resp.error.is_none());
    assert_eq!(resp.data, Some(json!({ "volume": 0.6 })));
}

#[test]
fn volume_accepts_boundary_absolutes() {
    for v in ["0", "0.0", "1", "1.0"] {
        let resp = drive_with_args("volume", json!({"value": v}));
        assert!(resp.error.is_none(), "{v}: boundary absolute must accept");
    }
}

#[test]
fn volume_accepts_delta_strings() {
    // Default test_app volume is 1.0, so positive deltas clamp; negatives
    // produce in-range values. Either way the responder must fire ok.
    for delta in ["+0.05", "-0.05", "+0.5", "-0.5", "+0", "-0"] {
        let resp = drive_with_args("volume", json!({"value": delta}));
        assert!(resp.error.is_none(), "{delta}: delta must accept");
    }
}

#[test]
fn volume_missing_arg_returns_invalid_args_error() {
    let resp = drive_with_args("volume", json!({}));
    let err = resp.error.expect("missing arg must error");
    assert_eq!(err.code, "invalid_args");
    assert!(err.message.contains("value"));
}

#[test]
fn volume_numeric_arg_returns_invalid_args_error() {
    // act_str arm requires the arg as a JSON string; legacy CLIs sending
    // `{"value": 0.6}` (number) get the macro's "missing required string arg"
    // error rather than silently mis-parsing.
    let resp = drive_with_args("volume", json!({"value": 0.6}));
    let err = resp.error.expect("numeric volume must error post act_str");
    assert_eq!(err.code, "invalid_args");
    assert!(err.message.contains("value"));
}

#[test]
fn volume_unparseable_string_returns_invalid_args_error() {
    let resp = drive_with_args("volume", json!({"value": "loud"}));
    let err = resp.error.expect("non-numeric volume must error");
    assert_eq!(err.code, "invalid_args");
    assert!(err.message.contains("must be a number"));
}

#[test]
fn volume_rejects_absolute_out_of_range() {
    for v in ["1.5", "2"] {
        let resp = drive_with_args("volume", json!({"value": v}));
        let err = resp.error.expect("out-of-range absolute must error");
        assert_eq!(err.code, "invalid_args");
        assert!(err.message.contains("out of range"));
    }
}

#[test]
fn volume_rejects_empty_value() {
    let resp = drive_with_args("volume", json!({"value": ""}));
    let err = resp.error.expect("empty value must error");
    assert_eq!(err.code, "invalid_args");
}

#[test]
fn clear_queue_responds_ok_from_any_view() {
    // `clear-queue` is an `act` verb that bypasses the in-app handler's
    // "Not in queue view" gate. The IPC contract is: responder fires ok with
    // an {"ok":true} acknowledgement regardless of which view the running
    // instance is on. The actual backend wipe is exercised by the
    // playback-handler tests.
    let resp = drive("clear-queue");
    assert_eq!(resp.request_id, 7);
    assert_eq!(resp.data, Some(json!({ "ok": true })));
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
        assert_eq!(
            resp.data,
            Some(json!({ "view": view_name })),
            "{view_name}: should echo the view name"
        );
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
    // Empty queue → current_starred falls back to false, so the optimistic
    // toggle targets `loved: true` and the response echoes it.
    assert_eq!(resp.data, Some(json!({ "loved": true })));
    assert!(resp.error.is_none());
}

#[test]
fn love_pushes_an_in_window_toast() {
    // IPC curation should surface the same in-window toast the Shift+L hotkey
    // shows, so a `nokkvi love` from a WM keybind gives visible feedback.
    let mut app = test_app();
    app.scrobble.current_song_id = Some("song-123".into());
    app.playback.title = "Below The Waterfall Room".into();
    let (incoming, rx) = make_incoming("love");

    let dispatched = app.update(Message::Ipc(Box::new(incoming)));
    drop(dispatched);
    rx.blocking_recv().expect("responder must fire for love");

    let toast = app
        .toast
        .toasts
        .iter()
        .find(|t| t.message.contains("Starred"))
        .expect("love should push a Starred/Unstarred toast");
    assert!(
        toast.message.contains("Below The Waterfall Room"),
        "toast should name the track, got: {}",
        toast.message
    );
    assert_eq!(toast.level, nokkvi_data::types::toast::ToastLevel::Success);
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
fn rate_echoes_resulting_rating() {
    // Empty queue → current_rating falls back to 0, so an absolute "4" lands
    // at 4 and the response echoes it.
    let resp = drive_rate_with_song("4");
    assert_eq!(resp.data, Some(json!({ "rating": 4 })));
    assert!(resp.error.is_none());
}

#[test]
fn rate_pushes_an_in_window_toast() {
    // IPC rating should surface the same in-window toast the rating hotkey
    // shows, so a `nokkvi rate` from a WM keybind gives visible feedback.
    let mut app = test_app();
    app.scrobble.current_song_id = Some("song-123".into());
    app.playback.title = "Below The Waterfall Room".into();
    app.playback.artist = "Hello Meteor".into();
    let (incoming, rx) = make_incoming_with_args("rate", json!({"delta": "4"}));

    let dispatched = app.update(Message::Ipc(Box::new(incoming)));
    drop(dispatched);
    rx.blocking_recv().expect("responder must fire for rate");

    let toast = app
        .toast
        .toasts
        .iter()
        .find(|t| t.message.contains("Rated"))
        .expect("rate should push a Rated toast");
    assert!(
        toast.message.contains("4/5"),
        "toast should show the new rating, got: {}",
        toast.message
    );
    assert!(
        toast.message.contains("Below The Waterfall Room"),
        "toast should name the track, got: {}",
        toast.message
    );
    assert_eq!(toast.level, nokkvi_data::types::toast::ToastLevel::Success);
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
