//! Tests for session-bound state cleanup on logout / session-expired.
//!
//! Complements the bucket-coverage tests in `general.rs` by pinning the
//! NF7 leaks that motivated `reset_session_state` to also clear the
//! per-process artwork cache and the SSE connection registration:
//!
//! - **Artwork leak**: server-A's artwork stayed in `Nokkvi.artwork` after
//!   logout and was served for server-B's IDs after re-login when album IDs
//!   happened to overlap.
//! - **SSE leak**: the static `SSE_CONNECTION_INFO` slot retained the old
//!   `auth_gateway` + `server_url` across logout, so the SSE event loop
//!   kept retrying with stale credentials and got 401 forever until the
//!   new login overwrote the slot.

use super::SSE_SLOT_TEST_LOCK;
use crate::test_helpers::*;

#[test]
fn reset_session_state_clears_stranded_within_list_drag() {
    // A logout (hotkey/IPC) mid-drag must not leave the queue's drag state set,
    // or the floating ghost + edge auto-scroll would resume on the next session.
    use crate::widgets::drag_column::EdgeZone;
    let _guard = SSE_SLOT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = test_app();

    app.queue_page.drag_source = Some(vec![42]);
    app.queue_page.drag_edge = EdgeZone::Bottom;
    app.queue_page.drag_cursor = Some(iced::Point::new(1.0, 2.0));

    let _ = app.reset_session_state();

    assert!(app.queue_page.drag_source.is_none());
    assert_eq!(app.queue_page.drag_edge, EdgeZone::None);
    assert!(app.queue_page.drag_cursor.is_none());
}

#[test]
fn reset_session_state_clears_artwork_cache() {
    // `reset_session_state()` calls `navidrome_sse::clear()`; serialize against
    // the other SSE-slot tests so the shared static cannot race.
    let _guard = SSE_SLOT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = test_app();

    // Seed a sentinel into ArtworkState. `loading_large_artwork` is the
    // cheapest signal — no `image::Handle` construction needed, and the
    // `Default` impl puts it back to `None` on reset.
    app.artwork.loading_large_artwork = Some("seed-album-id".into());

    // Sanity: the seed actually landed.
    assert_eq!(
        app.artwork.loading_large_artwork.as_deref(),
        Some("seed-album-id"),
    );

    let _ = app.reset_session_state();

    assert!(
        app.artwork.loading_large_artwork.is_none(),
        "artwork cache leaks across sessions: server-A art could be served \
         for server-B IDs after re-login",
    );
    assert!(
        app.artwork.album_art.is_empty(),
        "album_art LRU must be empty after reset",
    );
    assert!(
        app.artwork.large_artwork.is_empty(),
        "large_artwork LRU must be empty after reset",
    );
}

#[test]
fn navidrome_sse_clear_is_idempotent() {
    let _guard = SSE_SLOT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // `clear()` must be safe to call without a prior `register()`, and
    // safe to call repeatedly. The reset path runs unconditionally on
    // logout / session-expired; if `clear()` panicked on an empty slot
    // or on a double-clear, every logout would take the app down.
    crate::services::navidrome_sse::clear();
    crate::services::navidrome_sse::clear();
}

/// Build a throwaway `SseConnectionInfo` for the slot-state regression tests.
/// `AuthGateway::new()` is pure (no I/O / keyring) so this is cheap.
fn test_sse_info() -> crate::services::navidrome_sse::SseConnectionInfo {
    crate::services::navidrome_sse::SseConnectionInfo {
        server_url: "http://example.test".into(),
        auth_gateway: nokkvi_data::backend::auth::AuthGateway::new()
            .expect("AuthGateway::new is pure and cannot fail in tests"),
    }
}

#[test]
fn navidrome_sse_clear_actually_empties_slot() {
    use crate::services::navidrome_sse;

    let _guard = SSE_SLOT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    // `SSE_CONNECTION_INFO` is a process-global static; isolate this test by
    // registering then clearing within the same body so cross-test order
    // cannot pollute the assertion.
    navidrome_sse::register(test_sse_info());
    assert!(
        navidrome_sse::slot_is_set(),
        "register must populate the slot"
    );

    navidrome_sse::clear();
    assert!(
        !navidrome_sse::slot_is_set(),
        "clear must EMPTY the slot — otherwise the SSE loop keeps reconnecting \
         against the stale server after logout"
    );
}

#[test]
fn navidrome_sse_register_never_silently_drops_under_contention() {
    use std::{thread, time::Duration};

    use crate::services::navidrome_sse;

    // Serialize against the other `SSE_CONNECTION_INFO` tests so a concurrent
    // `clear()` cannot empty the slot between this test's `register()` and its
    // `slot_is_set()` assertion. Plain `std::thread` (not tokio) keeps this a
    // synchronous test: the test-serialization guard never crosses an `.await`,
    // so it shares one lock with the sibling sync tests cleanly.
    let _guard = SSE_SLOT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    // Start from a known-empty slot.
    navidrome_sse::clear();

    // Hold the connection-slot lock for ~30ms on a background thread.
    let hold = thread::spawn(|| {
        navidrome_sse::hold_slot_lock_blocking(Duration::from_millis(30));
    });

    // Give the background thread time to grab the lock first.
    thread::sleep(Duration::from_millis(5));

    // Under the old `try_lock` shape this register would silently no-op while
    // the lock was held. The blocking `parking_lot::Mutex` makes it wait and
    // complete.
    navidrome_sse::register(test_sse_info());

    hold.join().expect("hold thread should join");

    assert!(
        navidrome_sse::slot_is_set(),
        "register must populate the slot even under lock contention"
    );

    // Leave the global slot clean for any subsequent test.
    navidrome_sse::clear();
}

/// A scrobble submission in flight at logout never gets its result: the
/// task's answer is dropped with the session (`Message::SessionScoped`). The
/// latches that gate one submission per track must be released, or the same
/// track resumed after re-login would never scrobble. The track identity
/// itself stays (it follows the now-playing track).
#[test]
fn reset_session_state_releases_the_scrobble_latches() {
    use crate::state::RadioScrobbleAction;

    let _guard = SSE_SLOT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = test_app();
    app.scrobble.current_song_id = Some("s1".into());
    app.scrobble.submission_in_flight = true;
    // A radio track whose submit was dispatched (the latch is set).
    let _ = app.radio_scrobble.observe("Artist", "Title", 100);
    assert!(matches!(
        app.radio_scrobble.tick(101, true, 1),
        RadioScrobbleAction::Scrobble { .. }
    ));
    assert!(
        matches!(
            app.radio_scrobble.tick(102, true, 1),
            RadioScrobbleAction::None
        ),
        "latched while the submit is in flight"
    );

    let _ = app.reset_session_state();

    assert_eq!(app.scrobble.current_song_id.as_deref(), Some("s1"));
    assert!(
        !app.scrobble.submission_in_flight,
        "the queue scrobble latch is released at logout"
    );
    assert!(
        matches!(
            app.radio_scrobble.tick(103, true, 1),
            RadioScrobbleAction::Scrobble { .. }
        ),
        "the radio latch is released at logout"
    );
}

/// A resume whose session check the server refused (a changed password)
/// leaves the login form ready for the new password: no home screen, no
/// error on the form, the username kept, and the same toast a session that
/// expires mid-use shows (#14).
#[test]
fn a_refused_resume_leaves_a_ready_login_form() {
    let mut app = test_app();
    app.screen = crate::Screen::Login;
    app.login_page.username = "foogs".into();
    app.login_page.login_in_progress = true;

    let _ = app.update(crate::app_message::Message::LoginResult(Box::new(Err(
        crate::app_message::LoginFailure::SessionEnded,
    ))));

    assert_eq!(app.screen, crate::Screen::Login);
    assert!(app.app_service.is_none());
    assert!(!app.login_page.login_in_progress, "the form takes a login");
    assert!(app.login_page.error.is_none(), "{:?}", app.login_page.error);
    assert_eq!(app.login_page.username, "foogs");
    assert!(
        app.toast
            .toasts
            .iter()
            .any(|t| t.message.contains("Session expired")),
        "the expiry is announced"
    );
}

/// A login waiting out the server's failed-login lockout says so on the form
/// until the login ends either way.
#[test]
fn the_lockout_wait_shows_until_the_login_ends() {
    use crate::app_message::{LoginFailure, LoginProgress, Message};

    let mut app = test_app();
    app.screen = crate::Screen::Login;
    app.login_page.login_in_progress = true;

    let _ = app.update(Message::LoginProgress(LoginProgress::WaitingOutLockout));
    assert!(app.login_page.waiting_out_lockout);

    let _ = app.update(Message::LoginResult(Box::new(Err(LoginFailure::Failed(
        "Navidrome is still pausing sign-ins".into(),
    )))));
    assert!(!app.login_page.waiting_out_lockout);
    assert!(!app.login_page.login_in_progress);
    assert!(app.login_page.error.is_some());
}

/// A report that outlives its login changes nothing on an idle form.
#[test]
fn a_late_lockout_report_leaves_an_idle_form_alone() {
    let mut app = test_app();
    app.screen = crate::Screen::Login;

    let _ = app.update(crate::app_message::Message::LoginProgress(
        crate::app_message::LoginProgress::WaitingOutLockout,
    ));

    assert!(!app.login_page.waiting_out_lockout);
}
