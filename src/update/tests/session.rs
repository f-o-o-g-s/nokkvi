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
    assert!(
        app.artwork.album_dominant_colors.is_empty(),
        "album_dominant_colors LRU must be empty after reset",
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
