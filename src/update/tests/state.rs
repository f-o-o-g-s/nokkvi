//! Tests for state-type update handlers.

use crate::test_helpers::*;

// ============================================================================
// ScrobbleState (state.rs)
// ============================================================================

#[test]
fn scrobble_state_reset_for_new_song() {
    let mut state = crate::state::ScrobbleState {
        listening_time: 120.0,
        last_position: 120.0,
        submitted: true,
        current_song_id: Some("old".to_string()),
        ..Default::default()
    };

    state.reset_for_new_song(Some("new".to_string()), 0.0, 240);

    assert_eq!(state.current_song_id.as_deref(), Some("new"));
    assert_eq!(state.listening_time, 0.0);
    assert_eq!(state.last_position, 0.0);
    assert!(!state.submitted);
    assert!(!state.submission_in_flight);
    assert_eq!(state.current_song_duration, 240);
}

#[test]
fn scrobble_state_reset_with_nonzero_position() {
    let mut state = crate::state::ScrobbleState::default();

    state.reset_for_new_song(Some("song".to_string()), 5.0, 180);

    assert_eq!(state.last_position, 5.0);
    assert_eq!(state.listening_time, 0.0);
    assert_eq!(state.current_song_duration, 180);
}

// Progressive Queue Generation Counter (state.rs)
// ============================================================================

#[test]
fn progressive_queue_generation_starts_at_zero() {
    let app = test_app();
    assert_eq!(app.library.progressive_queue_generation, 0);
}

#[test]
fn progressive_queue_generation_increments() {
    let mut app = test_app();
    app.library.progressive_queue_generation += 1;
    assert_eq!(app.library.progressive_queue_generation, 1);
    app.library.progressive_queue_generation += 1;
    assert_eq!(app.library.progressive_queue_generation, 2);
}

// ============================================================================
// ScrobbleState Edge Cases (state.rs)
// ============================================================================

#[test]
fn should_scrobble_returns_true_when_threshold_met() {
    let state = crate::state::ScrobbleState {
        listening_time: 120.0,
        submitted: false,
        current_song_id: Some("song".to_string()),
        ..Default::default()
    };
    // 120s listened, track is 200s, threshold 50% → need 100s → should scrobble
    assert!(state.should_scrobble(200, 0.50));
}

#[test]
fn should_scrobble_returns_false_when_already_submitted() {
    let state = crate::state::ScrobbleState {
        listening_time: 120.0,
        submitted: true,
        current_song_id: Some("song".to_string()),
        ..Default::default()
    };
    assert!(
        !state.should_scrobble(200, 0.50),
        "should not scrobble twice"
    );
}

#[test]
fn should_scrobble_returns_false_for_zero_duration() {
    let state = crate::state::ScrobbleState {
        listening_time: 120.0,
        submitted: false,
        current_song_id: Some("song".to_string()),
        ..Default::default()
    };
    assert!(
        !state.should_scrobble(0, 0.50),
        "zero-duration tracks should never scrobble"
    );
}

// --- N2: absolute 4-minute arm (min(50%, 240s) per Navidrome) ---

#[test]
fn should_scrobble_absolute_four_minute_arm() {
    // 60-minute track, only 4 minutes listened. The percent arm would require
    // 1800s; the absolute 240s arm makes it eligible.
    let state = crate::state::ScrobbleState {
        listening_time: 240.0,
        submitted: false,
        current_song_id: Some("song".to_string()),
        ..Default::default()
    };
    assert!(state.should_scrobble(3600, 0.50));
}

#[test]
fn should_scrobble_below_absolute_and_percent_is_false() {
    let state = crate::state::ScrobbleState {
        listening_time: 239.0,
        submitted: false,
        ..Default::default()
    };
    assert!(!state.should_scrobble(3600, 0.50));
}

#[test]
fn should_scrobble_short_track_still_uses_percent() {
    // 200s track, 50% threshold → 100s needed; 100s listened qualifies via the
    // percent arm even though it is far under the 240s absolute arm.
    let state = crate::state::ScrobbleState {
        listening_time: 100.0,
        submitted: false,
        ..Default::default()
    };
    assert!(state.should_scrobble(200, 0.50));
}

#[test]
fn should_scrobble_blocked_while_in_flight() {
    // A pending submission gates re-dispatch even if the threshold is met.
    let state = crate::state::ScrobbleState {
        listening_time: 120.0,
        submitted: false,
        submission_in_flight: true,
        ..Default::default()
    };
    assert!(
        !state.should_scrobble(200, 0.50),
        "an in-flight submission must block re-dispatch"
    );
}

// --- I22: fallback judges the finished song by its own duration ---

#[test]
fn should_scrobble_uses_finished_song_duration_not_successor() {
    let state = crate::state::ScrobbleState {
        listening_time: 27.0,
        submitted: false,
        ..Default::default()
    };
    // A short (30s) song listened to 27s (>= 90%) is eligible...
    assert!(state.should_scrobble(30, 0.9));
    // ...but judged against a long successor's 360s duration it would not be.
    assert!(!state.should_scrobble(360, 0.9));
}

// ============================================================================
// ToastState Edge Cases (state.rs)
// ============================================================================

#[test]
fn toast_keyed_dedup_replaces_existing() {
    use nokkvi_data::types::toast::{Toast, ToastLevel};
    let mut state = crate::state::ToastState::default();

    // Push a keyed toast
    let mut t1 = Toast::new("Loading 1/10", ToastLevel::Info);
    t1.key = Some("progress".to_string());
    state.push(t1);

    // Push another toast with the same key — should replace, not duplicate
    let mut t2 = Toast::new("Loading 5/10", ToastLevel::Info);
    t2.key = Some("progress".to_string());
    state.push(t2);

    assert_eq!(state.toasts.len(), 1, "keyed toast should deduplicate");
    assert_eq!(state.toasts[0].message, "Loading 5/10");
}

#[test]
fn toast_capacity_evicts_oldest() {
    use nokkvi_data::types::toast::{Toast, ToastLevel};
    let mut state = crate::state::ToastState::default();

    // Fill to capacity (MAX_TOASTS = 10)
    for i in 0..10 {
        state.push(Toast::new(format!("Toast {i}"), ToastLevel::Info));
    }
    assert_eq!(state.toasts.len(), 10);

    // Push one more — oldest should be evicted
    state.push(Toast::new("Overflow", ToastLevel::Info));
    assert_eq!(state.toasts.len(), 10, "should not exceed capacity");
    assert_eq!(
        state.toasts.front().map(|t| t.message.as_str()),
        Some("Toast 1"),
        "oldest toast (Toast 0) should have been evicted"
    );
    assert_eq!(
        state.toasts.back().map(|t| t.message.as_str()),
        Some("Overflow")
    );
}

#[test]
fn toast_dismiss_key_removes_matching() {
    use nokkvi_data::types::toast::{Toast, ToastLevel};
    let mut state = crate::state::ToastState::default();

    let mut t1 = Toast::new("Loading...", ToastLevel::Info);
    t1.key = Some("load".to_string());
    state.push(t1);
    state.push(Toast::new("Unrelated", ToastLevel::Success));

    assert_eq!(state.toasts.len(), 2);

    state.dismiss_key("load");
    assert_eq!(state.toasts.len(), 1);
    assert_eq!(state.toasts[0].message, "Unrelated");
}

// ============================================================================
