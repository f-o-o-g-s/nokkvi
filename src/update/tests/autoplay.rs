//! Autoplay: the toggle and its going-off cleanup, the fill trigger the
//! playback tick drives, each fill outcome, the retry gate and the resume
//! after the queue ran out (`update/playback.rs`, `state/autoplay.rs`).

use std::time::{Duration, Instant};

use nokkvi_data::types::autoplay::{AUTOPLAY_MAX_RETRIES, AutoplayOutcome};

use super::playback::make_playback_update;
use crate::{
    app_message::PlaybackStateUpdate,
    state::{
        AUTOPLAY_NOT_NEEDED_WAIT, AUTOPLAY_RETRY_WAIT, AutoplayAsk, AutoplayFill, AutoplayGate,
        AutoplayState,
    },
    test_helpers::*,
};

fn toast_messages(app: &crate::Nokkvi) -> Vec<String> {
    app.toast.toasts.iter().map(|t| t.message.clone()).collect()
}

fn radio() -> crate::state::ActivePlayback {
    crate::state::ActivePlayback::Radio(crate::state::RadioPlaybackState {
        station: nokkvi_data::types::radio_station::RadioStation {
            id: "station".into(),
            name: "Station".into(),
            stream_url: "http://station".into(),
            home_page_url: None,
            cover_art: None,
        },
        icy_artist: None,
        icy_title: None,
        icy_url: None,
    })
}

/// An app with Autoplay on, queue playback, Repeat off.
fn autoplay_app() -> crate::Nokkvi {
    let mut app = test_app();
    app.settings.autoplay_enabled = true;
    app
}

/// A playing tick on the last song (one row after it would still ask; none
/// after it is the plainest case), Repeat off.
fn low_runway_tick() -> PlaybackStateUpdate {
    PlaybackStateUpdate {
        autoplay_runway: Some(0),
        ..make_playback_update()
    }
}

/// A stopped tick with no current row: what the tick reads after the queue
/// ran out.
fn ran_out_tick(epoch: u64) -> PlaybackStateUpdate {
    PlaybackStateUpdate {
        playing: false,
        paused: false,
        current_index: None,
        current_entry_id: None,
        song_id: None,
        autoplay_runway: None,
        queue_ran_out_epoch: epoch,
        ..make_playback_update()
    }
}

// ============================================================================
// The toggle
// ============================================================================

#[test]
fn toggle_flips_the_setting_and_toasts() {
    let mut app = test_app();
    assert!(!app.settings.autoplay_enabled);

    let _ = app.handle_toggle_autoplay();
    assert!(app.settings.autoplay_enabled);
    assert_eq!(
        toast_messages(&app).last().map(String::as_str),
        Some("Autoplay: On")
    );

    let _ = app.handle_toggle_autoplay();
    assert!(!app.settings.autoplay_enabled);
    assert_eq!(
        toast_messages(&app).last().map(String::as_str),
        Some("Autoplay: Off")
    );
}

#[test]
fn toggle_is_a_noop_during_radio() {
    let mut app = test_app();
    app.active_playback = radio();
    let _ = app.handle_toggle_autoplay();
    assert!(!app.settings.autoplay_enabled);
    assert!(app.toast.toasts.is_empty());
}

#[test]
fn turning_off_resets_the_autoplay_state_but_keeps_a_fill_in_flight() {
    let mut app = autoplay_app();
    app.autoplay = AutoplayState {
        in_flight: true,
        failures: 2,
        wait_until: Some(Instant::now() + Duration::from_secs(9)),
        nothing_toasted: true,
        resume_pending: true,
        seen_ran_out_epoch: Some(4),
    };
    let _ = app.handle_toggle_autoplay();
    assert_eq!(
        app.autoplay,
        AutoplayState {
            in_flight: true,
            seen_ran_out_epoch: Some(4),
            ..AutoplayState::default()
        }
    );
}

#[test]
fn the_settings_row_going_off_runs_the_same_cleanup_as_the_toggle() {
    use nokkvi_data::services::settings_tables::SettingsSideEffect;

    use crate::views::settings::items::SettingValue;

    // `finish_general_setting` applies the settings through
    // `handle_player_settings_loaded`, which mirrors the process-global theme
    // atomics (auto-hide included); unlocked, it flipped auto-hide off under
    // `window_unfocus_drops_search_focus_so_refocus_doesnt_reveal`.
    let _guard = crate::theme::THEME_MODE_LOCK.lock();

    assert!(crate::update::settings::settings_row_turns_autoplay_off(
        "general.autoplay_enabled",
        &SettingValue::Bool(false)
    ));
    assert!(!crate::update::settings::settings_row_turns_autoplay_off(
        "general.autoplay_enabled",
        &SettingValue::Bool(true)
    ));
    assert!(!crate::update::settings::settings_row_turns_autoplay_off(
        "general.crossfade_enabled",
        &SettingValue::Bool(false)
    ));

    let mut app = autoplay_app();
    app.autoplay = AutoplayState {
        failures: 1,
        resume_pending: true,
        nothing_toasted: true,
        ..AutoplayState::default()
    };
    let mut loaded = app.settings.clone();
    loaded.autoplay_enabled = false;
    let _ = app.finish_general_setting(true, SettingsSideEffect::None, loaded);
    assert!(!app.settings.autoplay_enabled);
    assert_eq!(app.autoplay, AutoplayState::default());

    // Any other row leaves Autoplay's state alone.
    let mut app = autoplay_app();
    app.autoplay.resume_pending = true;
    let loaded = app.settings.clone();
    let _ = app.finish_general_setting(false, SettingsSideEffect::None, loaded);
    assert!(app.autoplay.resume_pending);
}

#[test]
fn a_clear_that_removed_rows_says_so_in_the_one_toast() {
    let mut app = autoplay_app();
    let _ = app.handle_toggle_autoplay();
    let _ = app.handle_autoplay_cleared(3);
    let autoplay_toasts: Vec<String> = toast_messages(&app)
        .into_iter()
        .filter(|m| m.starts_with("Autoplay"))
        .collect();
    assert_eq!(autoplay_toasts, ["Autoplay: Off, removed 3 queued songs"]);

    let mut app = autoplay_app();
    let _ = app.handle_toggle_autoplay();
    let _ = app.handle_autoplay_cleared(1);
    assert_eq!(
        toast_messages(&app),
        ["Autoplay: Off, removed 1 queued song"]
    );

    // Nothing removed: the toggle's toast stands alone.
    let mut app = autoplay_app();
    let _ = app.handle_toggle_autoplay();
    let _ = app.handle_autoplay_cleared(0);
    assert_eq!(toast_messages(&app), ["Autoplay: Off"]);
}

#[test]
fn a_clear_that_lands_after_autoplay_came_back_on_toasts_nothing() {
    let mut app = autoplay_app();
    let _ = app.handle_autoplay_cleared(2);
    assert!(app.toast.toasts.is_empty());
}

// ============================================================================
// The fill predicate
// ============================================================================

fn wanting_gate() -> AutoplayGate {
    AutoplayGate {
        enabled: true,
        queue_mode: true,
        repeat_off: true,
        runway: Some(1),
        transport_active: true,
        progressive_load: false,
    }
}

#[test]
fn fill_predicate_truth_table() {
    let now = Instant::now();
    let idle = AutoplayState::default();
    let ask = |state: &AutoplayState, gate: AutoplayGate| state.ask(&gate, now);

    assert_eq!(ask(&idle, wanting_gate()), Some(AutoplayAsk::Fill));
    assert_eq!(
        ask(
            &idle,
            AutoplayGate {
                runway: Some(0),
                ..wanting_gate()
            }
        ),
        Some(AutoplayAsk::Fill),
        "on the last song",
    );

    let refused = [
        (
            "off",
            AutoplayGate {
                enabled: false,
                ..wanting_gate()
            },
        ),
        (
            "radio",
            AutoplayGate {
                queue_mode: false,
                ..wanting_gate()
            },
        ),
        (
            "Repeat on",
            AutoplayGate {
                repeat_off: false,
                ..wanting_gate()
            },
        ),
        (
            "long runway",
            AutoplayGate {
                runway: Some(2),
                ..wanting_gate()
            },
        ),
        (
            "no current row",
            AutoplayGate {
                runway: None,
                ..wanting_gate()
            },
        ),
        (
            "stopped",
            AutoplayGate {
                transport_active: false,
                ..wanting_gate()
            },
        ),
        (
            "progressive load",
            AutoplayGate {
                progressive_load: true,
                ..wanting_gate()
            },
        ),
    ];
    for (why, gate) in refused {
        assert_eq!(ask(&idle, gate), None, "{why}");
    }

    let in_flight = AutoplayState {
        in_flight: true,
        ..AutoplayState::default()
    };
    assert_eq!(ask(&in_flight, wanting_gate()), None, "in flight");

    let waiting = AutoplayState {
        wait_until: Some(now + Duration::from_secs(1)),
        ..AutoplayState::default()
    };
    assert_eq!(ask(&waiting, wanting_gate()), None, "retry wait");
    let waited = AutoplayState {
        wait_until: Some(now),
        ..AutoplayState::default()
    };
    assert_eq!(
        ask(&waited, wanting_gate()),
        Some(AutoplayAsk::Fill),
        "wait over"
    );

    let last_retry = AutoplayState {
        failures: AUTOPLAY_MAX_RETRIES,
        ..AutoplayState::default()
    };
    assert_eq!(
        ask(&last_retry, wanting_gate()),
        Some(AutoplayAsk::Fill),
        "the last ask again",
    );
    let exhausted = AutoplayState {
        failures: AUTOPLAY_MAX_RETRIES + 1,
        ..AutoplayState::default()
    };
    assert_eq!(ask(&exhausted, wanting_gate()), None, "retries exhausted");

    // A pending resume asks after the run-out with no current row, stopped
    // or not, but still behind the other gates.
    let resume = AutoplayState {
        resume_pending: true,
        ..AutoplayState::default()
    };
    let ran_out = AutoplayGate {
        runway: None,
        transport_active: false,
        ..wanting_gate()
    };
    assert_eq!(ask(&resume, ran_out), Some(AutoplayAsk::AfterRunOut));
    assert_eq!(
        ask(
            &resume,
            AutoplayGate {
                repeat_off: false,
                ..ran_out
            }
        ),
        None
    );
    assert_eq!(
        ask(
            &AutoplayState {
                wait_until: Some(now + Duration::from_secs(1)),
                ..resume
            },
            ran_out
        ),
        None
    );
}

// ============================================================================
// The tick
// ============================================================================

#[test]
fn a_tick_that_wants_a_fill_sets_the_in_flight_flag() {
    let mut app = autoplay_app();
    let _ = app.handle_playback_state_updated(low_runway_tick());
    assert!(app.autoplay.in_flight);
}

#[test]
fn a_tick_that_wants_no_fill_leaves_the_flag_alone() {
    for (why, mut app, tick) in [
        ("off", test_app(), low_runway_tick()),
        (
            "long runway",
            autoplay_app(),
            PlaybackStateUpdate {
                autoplay_runway: Some(5),
                ..make_playback_update()
            },
        ),
        (
            "Repeat on",
            autoplay_app(),
            PlaybackStateUpdate {
                repeat: true,
                ..low_runway_tick()
            },
        ),
        (
            "stopped",
            autoplay_app(),
            PlaybackStateUpdate {
                playing: false,
                ..low_runway_tick()
            },
        ),
    ] {
        let _ = app.handle_playback_state_updated(tick);
        assert!(!app.autoplay.in_flight, "{why}");
    }

    let mut app = autoplay_app();
    app.active_playback = radio();
    let _ = app.handle_playback_state_updated(low_runway_tick());
    assert!(!app.autoplay.in_flight, "radio");

    let mut app = autoplay_app();
    app.library.start_progressive_queue_load(1000);
    let _ = app.handle_playback_state_updated(low_runway_tick());
    assert!(!app.autoplay.in_flight, "progressive load");
}

#[test]
fn a_song_change_resets_the_retry_state() {
    let mut app = autoplay_app();
    let _ = app.handle_playback_state_updated(make_playback_update());
    app.autoplay.failures = AUTOPLAY_MAX_RETRIES + 1;
    app.autoplay.wait_until = Some(Instant::now() + Duration::from_secs(60));
    app.autoplay.nothing_toasted = true;

    // Same song: kept.
    let _ = app.handle_playback_state_updated(make_playback_update());
    assert_eq!(app.autoplay.failures, AUTOPLAY_MAX_RETRIES + 1);

    let _ = app.handle_playback_state_updated(PlaybackStateUpdate {
        song_id: Some("song_2".into()),
        ..make_playback_update()
    });
    assert_eq!(app.autoplay.failures, 0);
    assert_eq!(app.autoplay.wait_until, None);
    assert!(!app.autoplay.nothing_toasted);
}

// ============================================================================
// Fill outcomes
// ============================================================================

#[test]
fn added_clears_the_flag_and_the_retry_state() {
    let now = Instant::now();
    let mut app = autoplay_app();
    app.autoplay.in_flight = true;
    app.autoplay.failures = 2;
    app.autoplay.wait_until = Some(now);
    let _ = app.handle_autoplay_filled(
        false,
        AutoplayFill::Outcome(AutoplayOutcome::Added {
            count: 10,
            first_entry_id: 7,
        }),
        now,
    );
    assert!(!app.autoplay.in_flight);
    assert_eq!(app.autoplay.failures, 0);
    assert_eq!(app.autoplay.wait_until, None);
    assert!(app.toast.toasts.is_empty(), "the rows carry the mark");
}

#[test]
fn nothing_found_counts_waits_and_toasts_once_per_song() {
    let now = Instant::now();
    let mut app = autoplay_app();
    for round in 1..=2u8 {
        app.autoplay.in_flight = true;
        let _ = app.handle_autoplay_filled(
            false,
            AutoplayFill::Outcome(AutoplayOutcome::NothingFound),
            now,
        );
        assert!(!app.autoplay.in_flight);
        assert_eq!(app.autoplay.failures, round);
        assert_eq!(app.autoplay.wait_until, Some(now + AUTOPLAY_RETRY_WAIT));
    }
    assert_eq!(toast_messages(&app), ["Autoplay found nothing new to add"]);
    assert_eq!(AUTOPLAY_RETRY_WAIT, Duration::from_secs(30));
}

#[test]
fn a_failure_counts_and_waits_without_a_toast() {
    let now = Instant::now();
    let mut app = autoplay_app();
    app.autoplay.in_flight = true;
    let _ = app.handle_autoplay_filled(false, AutoplayFill::Failed("boom".into()), now);
    assert!(!app.autoplay.in_flight);
    assert_eq!(app.autoplay.failures, 1);
    assert_eq!(app.autoplay.wait_until, Some(now + AUTOPLAY_RETRY_WAIT));
    assert!(app.toast.toasts.is_empty());
}

#[test]
fn not_needed_waits_briefly_without_counting() {
    let now = Instant::now();
    let mut app = autoplay_app();
    app.autoplay.in_flight = true;
    let _ = app.handle_autoplay_filled(
        false,
        AutoplayFill::Outcome(AutoplayOutcome::NotNeeded),
        now,
    );
    assert!(!app.autoplay.in_flight);
    assert_eq!(app.autoplay.failures, 0);
    assert_eq!(
        app.autoplay.wait_until,
        Some(now + AUTOPLAY_NOT_NEEDED_WAIT)
    );
    assert!(app.toast.toasts.is_empty());
}

#[test]
fn a_rejected_session_clears_the_flag() {
    let mut app = autoplay_app();
    app.autoplay.in_flight = true;
    let _ = app.handle_autoplay_filled(false, AutoplayFill::SessionRejected, Instant::now());
    assert!(!app.autoplay.in_flight);
    assert_eq!(app.autoplay.failures, 0, "not a retry: the session ends");
}

// ============================================================================
// The queue ran out
// ============================================================================

#[test]
fn the_first_tick_adopts_the_ran_out_count_without_acting() {
    let mut app = autoplay_app();
    let _ = app.handle_playback_state_updated(ran_out_tick(5));
    assert_eq!(app.autoplay.seen_ran_out_epoch, Some(5));
    assert!(!app.autoplay.resume_pending);
    assert!(!app.autoplay.in_flight);
}

#[test]
fn a_moved_ran_out_count_marks_the_resume_and_asks_at_once() {
    let mut app = autoplay_app();
    let _ = app.handle_playback_state_updated(make_playback_update());
    let _ = app.handle_playback_state_updated(ran_out_tick(1));
    assert!(app.autoplay.resume_pending);
    assert!(app.autoplay.in_flight);
}

#[test]
fn a_run_out_with_a_fill_in_flight_keeps_that_fill() {
    let mut app = autoplay_app();
    let _ = app.handle_playback_state_updated(make_playback_update());
    app.autoplay.in_flight = true;
    let _ = app.handle_playback_state_updated(ran_out_tick(1));
    assert!(app.autoplay.resume_pending);
    assert!(app.autoplay.in_flight);
}

#[test]
fn a_run_out_while_off_or_under_repeat_marks_nothing() {
    let mut app = test_app();
    let _ = app.handle_playback_state_updated(make_playback_update());
    let _ = app.handle_playback_state_updated(ran_out_tick(1));
    assert!(!app.autoplay.resume_pending, "off");
    assert_eq!(app.autoplay.seen_ran_out_epoch, Some(1));

    let mut app = autoplay_app();
    let _ = app.handle_playback_state_updated(make_playback_update());
    let _ = app.handle_playback_state_updated(PlaybackStateUpdate {
        repeat_queue: true,
        ..ran_out_tick(1)
    });
    assert!(!app.autoplay.resume_pending, "Repeat on");
}

/// An app whose queue just ran out with Autoplay on: resume pending, the
/// fill in flight.
fn ran_out_app() -> crate::Nokkvi {
    let mut app = autoplay_app();
    let _ = app.handle_playback_state_updated(make_playback_update());
    let _ = app.handle_playback_state_updated(ran_out_tick(1));
    assert!(app.autoplay.resume_pending);
    app
}

#[test]
fn the_pending_resume_clears_on_a_playing_tick() {
    let mut app = ran_out_app();
    let _ = app.handle_playback_state_updated(PlaybackStateUpdate {
        queue_ran_out_epoch: 1,
        ..make_playback_update()
    });
    assert!(!app.autoplay.resume_pending);
}

#[test]
fn the_pending_resume_clears_on_stop_and_play() {
    let mut app = ran_out_app();
    let _ = app.handle_stop();
    assert!(!app.autoplay.resume_pending, "stop");

    let mut app = ran_out_app();
    let _ = app.handle_play();
    assert!(!app.autoplay.resume_pending, "play");

    let mut app = ran_out_app();
    app.guard_play_action();
    assert!(!app.autoplay.resume_pending, "a queue play's prologue");
}

#[test]
fn the_pending_resume_clears_when_the_mode_goes_off_or_the_session_resets() {
    let mut app = ran_out_app();
    let _ = app.handle_toggle_autoplay();
    assert!(!app.autoplay.resume_pending, "mode off");

    let mut app = ran_out_app();
    let _ = app.reset_session_state();
    assert_eq!(app.autoplay, AutoplayState::default(), "session reset");
}

#[test]
fn an_added_batch_consumes_the_pending_resume() {
    let mut app = ran_out_app();
    let _ = app.handle_autoplay_filled(
        true,
        AutoplayFill::Outcome(AutoplayOutcome::Added {
            count: 10,
            first_entry_id: 40,
        }),
        Instant::now(),
    );
    assert!(!app.autoplay.resume_pending);
    assert!(!app.autoplay.in_flight);
}

#[test]
fn nothing_found_after_a_run_out_keeps_the_resume_pending() {
    let now = Instant::now();
    let mut app = ran_out_app();
    let _ = app.handle_autoplay_filled(
        true,
        AutoplayFill::Outcome(AutoplayOutcome::NothingFound),
        now,
    );
    assert!(app.autoplay.resume_pending);
    assert_eq!(app.autoplay.failures, 1);

    // Once the wait is over the tick asks again after the run-out.
    app.autoplay.wait_until = Some(now);
    let _ = app.handle_playback_state_updated(ran_out_tick(1));
    assert!(app.autoplay.in_flight);
}

#[test]
fn not_needed_after_a_run_out_drops_the_resume() {
    let mut app = ran_out_app();
    let _ = app.handle_autoplay_filled(
        true,
        AutoplayFill::Outcome(AutoplayOutcome::NotNeeded),
        Instant::now(),
    );
    assert!(!app.autoplay.resume_pending);
}
