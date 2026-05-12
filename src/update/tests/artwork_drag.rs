//! Tests for the shared `handle_artwork_vertical_drag` handler.
//!
//! Covers Audit finding #5 (2026-05-11 artwork audit): the new
//! Always-Vertical drag handler had no test. Both `Change` and `Commit`
//! variants must land the new percentage in the live theme atomic
//! (`crate::theme::artwork_vertical_height_pct`). The `Commit` path
//! additionally spawns a persistence task via `shell_spawn`; we cannot
//! observe that from a `test_app()` (no `AppService` attached) so we
//! assert only on the observable atomic state per CLAUDE.md's
//! "observable state mutations" rule.
//!
//! Note on parallelism: `artwork_vertical_height_pct` is a process-wide
//! static atomic. The two tests below write distinct values to it and
//! immediately read them back; a `Mutex` serializes them so an
//! interleaved write from a sibling test cannot pollute the assertion.

use std::sync::Mutex;

use crate::{
    app_message::Message, test_helpers::test_app, views::AlbumsMessage,
    widgets::artwork_split_handle::DragEvent,
};

/// Serializes tests that read+write the `artwork_vertical_height_pct`
/// static atomic so they don't observe each other's stores under
/// `cargo test`'s default parallel runner.
static ATOMIC_GUARD: Mutex<()> = Mutex::new(());

#[test]
fn vertical_drag_change_updates_atomic() {
    let _guard = ATOMIC_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = test_app();

    let _task = app.update(Message::Albums(AlbumsMessage::ArtworkColumnVerticalDrag(
        DragEvent::Change(0.55),
    )));

    let after = crate::theme::artwork_vertical_height_pct();
    assert!(
        (after - 0.55).abs() < 1e-5,
        "Change(0.55) should land 0.55 in the atomic, got {after}"
    );
}

#[test]
fn vertical_drag_commit_updates_atomic() {
    let _guard = ATOMIC_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = test_app();

    let _task = app.update(Message::Albums(AlbumsMessage::ArtworkColumnVerticalDrag(
        DragEvent::Commit(0.62),
    )));

    let after = crate::theme::artwork_vertical_height_pct();
    assert!(
        (after - 0.62).abs() < 1e-5,
        "Commit(0.62) should land 0.62 in the atomic, got {after}"
    );
    // The Commit branch additionally calls `shell_spawn(...)` to persist
    // via the settings backend. `test_app()` has no `AppService`, so the
    // spawn is a no-op here; we cannot positively observe it. The atomic
    // assertion above is the observable state mutation guaranteed by the
    // handler regardless of persistence path.
}
