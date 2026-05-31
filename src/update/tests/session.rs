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

use crate::test_helpers::*;

#[test]
fn reset_session_state_clears_artwork_cache() {
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

#[tokio::test]
async fn navidrome_sse_register_never_silently_drops_under_contention() {
    use std::time::Duration;

    use crate::services::navidrome_sse;

    // Start from a known-empty slot.
    navidrome_sse::clear();

    // Hold the connection-slot lock for ~30ms on a blocking thread.
    let hold = tokio::task::spawn_blocking(|| {
        navidrome_sse::hold_slot_lock_blocking(Duration::from_millis(30));
    });

    // Give the blocking task time to grab the lock first.
    tokio::time::sleep(Duration::from_millis(5)).await;

    // Under the old `try_lock` shape this register would silently no-op while
    // the lock was held. The blocking `parking_lot::Mutex` makes it wait and
    // complete.
    navidrome_sse::register(test_sse_info());

    hold.await.expect("hold task should join");

    assert!(
        navidrome_sse::slot_is_set(),
        "register must populate the slot even under lock contention"
    );

    // Leave the global slot clean for any subsequent test.
    navidrome_sse::clear();
}
