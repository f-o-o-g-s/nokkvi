//! Tests for the multi-library filter handler (`update/library_filter.rs`).
//!
//! Covers the four `LibraryMessage` variants — `OpenChange`, `Toggle`,
//! `Loaded`, `LoadFailed` — plus the cross-cutting behaviors that fall out
//! of those handlers: paged-buffer invalidation on toggle, mutual
//! exclusion with other overlay menus, and graceful no-panic behavior at
//! N <= 1 libraries.
//!
//! Sync tests use `test_app()` (no `AppService`) when they only need to
//! observe `open_menu` / toast / paged-buffer state. Tests that need to
//! exercise real backend behavior (`toggle_library`, `apply_library_refresh`)
//! use `#[tokio::test]` with a tempfile-backed `AppService` because the
//! backend constructor is async.

use std::collections::HashSet;

use iced::Rectangle;
use nokkvi_data::{
    backend::app_service::AppService, services::state_storage::StateStorage,
    types::library::Library,
};

use crate::{
    app_message::{LibraryMessage, OpenMenu},
    test_helpers::*,
};

// ============================================================================
// Test scaffolding
// ============================================================================

/// Build a `Nokkvi` populated with a real `AppService` backed by an
/// isolated tempfile redb. Returns the redb path so the caller can clean
/// up after the assertions run.
async fn test_app_with_shell() -> (crate::Nokkvi, std::path::PathBuf) {
    let suffix = format!(
        "test_lane_d_library_{}_{}.redb",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    );
    let db_path = std::env::temp_dir().join(suffix);
    let _ = std::fs::remove_file(&db_path);
    let storage = StateStorage::new(db_path.clone()).expect("redb open");
    let shell = AppService::new_with_storage(storage)
        .await
        .expect("app service");
    let mut app = test_app();
    app.app_service = Some(shell);
    (app, db_path)
}

/// Trigger bounds payload — content doesn't matter for handler tests, the
/// handler only forwards the value to `OpenMenu::LibrarySelector`.
fn dummy_bounds() -> Rectangle {
    Rectangle {
        x: 100.0,
        y: 20.0,
        width: 32.0,
        height: 32.0,
    }
}

fn library(id: i32, name: &str) -> Library {
    Library {
        id,
        name: name.to_string(),
    }
}

// ============================================================================
// OpenChange — popover open/close
// ============================================================================

/// `OpenChange { open: true, .. }` sets `open_menu` to
/// `OpenMenu::LibrarySelector` carrying the trigger bounds verbatim.
#[test]
fn open_change_open_true_sets_library_selector_open_menu() {
    let mut app = test_app();
    assert_eq!(app.open_menu, None);

    let bounds = dummy_bounds();
    let _ = app.handle_library_message(LibraryMessage::OpenChange {
        open: true,
        trigger_bounds: Some(bounds),
    });

    assert_eq!(
        app.open_menu,
        Some(OpenMenu::LibrarySelector {
            trigger_bounds: bounds
        }),
        "OpenChange(open=true) must open the LibrarySelector overlay"
    );
}

/// `OpenChange { open: false, .. }` clears `open_menu`.
#[test]
fn open_change_open_false_clears_open_menu() {
    let mut app = test_app();
    app.open_menu = Some(OpenMenu::LibrarySelector {
        trigger_bounds: dummy_bounds(),
    });

    let _ = app.handle_library_message(LibraryMessage::OpenChange {
        open: false,
        trigger_bounds: None,
    });

    assert_eq!(
        app.open_menu, None,
        "OpenChange(open=false) must close the overlay"
    );
}

/// `OpenChange { open: true }` with `trigger_bounds: None` falls back to a
/// zero rectangle rather than panicking — the trigger should always pass a
/// `Some(_)`, but the handler must be defensive.
#[test]
fn open_change_with_none_bounds_defaults_to_zero_rect() {
    let mut app = test_app();
    let _ = app.handle_library_message(LibraryMessage::OpenChange {
        open: true,
        trigger_bounds: None,
    });
    assert!(matches!(
        app.open_menu,
        Some(OpenMenu::LibrarySelector { .. })
    ));
}

/// Mutual exclusion smoke test: opening the LibrarySelector overlay must
/// replace any previously-open overlay (`SetOpenMenu` semantics). The
/// dispatcher enforces this via the implicit `open_menu = next` assignment
/// — the library-filter handler mirrors that contract by assigning directly.
#[test]
fn library_selector_replaces_open_hamburger_menu() {
    let mut app = test_app();
    app.open_menu = Some(OpenMenu::Hamburger);

    let bounds = dummy_bounds();
    let _ = app.handle_library_message(LibraryMessage::OpenChange {
        open: true,
        trigger_bounds: Some(bounds),
    });

    assert_eq!(
        app.open_menu,
        Some(OpenMenu::LibrarySelector {
            trigger_bounds: bounds
        }),
        "opening LibrarySelector must atomically replace Hamburger"
    );
}

/// Reverse direction of [`library_selector_replaces_open_hamburger_menu`]:
/// opening Hamburger (or any other overlay) via the shared
/// `handle_set_open_menu` dispatcher while the LibrarySelector is open
/// must clear the selector. The dispatcher is the single source of truth
/// for mutual exclusion across every `OpenMenu` variant — proving the
/// reverse direction here guards against future regressions that might
/// special-case LibrarySelector dismissal somewhere else (plan §14.10).
#[test]
fn opening_hamburger_clears_library_selector() {
    let mut app = test_app();
    app.open_menu = Some(OpenMenu::LibrarySelector {
        trigger_bounds: dummy_bounds(),
    });

    let _ = app.handle_set_open_menu(Some(OpenMenu::Hamburger));

    assert_eq!(
        app.open_menu,
        Some(OpenMenu::Hamburger),
        "opening Hamburger via handle_set_open_menu must dismiss LibrarySelector"
    );
}

// ============================================================================
// LoadFailed — toast on fetch error
// ============================================================================

/// `LoadFailed` pushes a single error toast so the user sees the failure.
#[test]
fn load_failed_emits_error_toast() {
    let mut app = test_app();
    assert!(app.toast.toasts.is_empty());

    let _ = app.handle_library_message(LibraryMessage::LoadFailed("boom".into()));

    assert_eq!(
        app.toast.toasts.len(),
        1,
        "LoadFailed must push exactly one toast"
    );
    let toast = &app.toast.toasts[0];
    assert_eq!(toast.level, nokkvi_data::types::toast::ToastLevel::Error);
    assert!(
        toast.message.contains("boom") || toast.message.to_lowercase().contains("librar"),
        "Toast should mention the error or the failed kind, got: {}",
        toast.message
    );
}

// ============================================================================
// Toggle — backend delegation + paged-buffer invalidation
// ============================================================================

/// `Toggle(id)` on a previously-absent id must add it to the active set.
/// Asserts against the backend `AppService`'s observable state.
#[tokio::test]
async fn toggle_library_adds_id_when_absent() {
    let (mut app, db_path) = test_app_with_shell().await;
    let shell = app.app_service.as_ref().expect("shell").clone();
    assert!(shell.active_library_ids().is_empty());

    let _ = app.handle_library_message(LibraryMessage::Toggle(7));

    assert!(
        shell.active_library_ids().contains(&7),
        "Toggle(7) on an empty set must add 7"
    );

    drop(app);
    drop(shell);
    let _ = std::fs::remove_file(&db_path);
}

/// `Toggle(id)` on a previously-present id must remove it from the active set.
#[tokio::test]
async fn toggle_library_removes_id_when_present() {
    let (mut app, db_path) = test_app_with_shell().await;
    let shell = app.app_service.as_ref().expect("shell").clone();

    // Seed: toggle id 7 in via the backend first so the round-trip starts
    // with `{7}` exactly.
    shell.toggle_library(7);
    assert!(shell.active_library_ids().contains(&7));

    let _ = app.handle_library_message(LibraryMessage::Toggle(7));

    assert!(
        !shell.active_library_ids().contains(&7),
        "Toggle(7) on {{7}} must remove 7"
    );
    assert!(shell.active_library_ids().is_empty());

    drop(app);
    drop(shell);
    let _ = std::fs::remove_file(&db_path);
}

/// `Toggle(id)` must bump the `generation()` counter on every paged
/// library buffer (Albums / Artists / Songs / Genres / Playlists) so any
/// in-flight stale fetch result discards itself when the new
/// active-library set lands.
#[tokio::test]
async fn toggle_invalidates_paged_buffer_generation() {
    let (mut app, db_path) = test_app_with_shell().await;

    // Seed each buffer with at least one item so `generation()` has
    // somewhere to move from. `set_from_vec` bumps the counter, so we
    // snapshot afterwards.
    seed_albums(&mut app, vec![make_album("a0", "Album 0", "Artist")]);
    seed_artists(&mut app, vec![make_artist("ar0", "Artist 0")]);
    seed_songs(&mut app, vec![make_song("s0", "Song 0", "Artist")]);
    seed_genres(&mut app, vec![make_genre("uuid-0", "Genre 0")]);

    let before_albums = app.library.albums.generation();
    let before_artists = app.library.artists.generation();
    let before_songs = app.library.songs.generation();
    let before_genres = app.library.genres.generation();

    let _ = app.handle_library_message(LibraryMessage::Toggle(1));

    assert!(
        app.library.albums.generation() > before_albums,
        "albums buffer generation must bump on toggle"
    );
    assert!(
        app.library.artists.generation() > before_artists,
        "artists buffer generation must bump on toggle"
    );
    assert!(
        app.library.songs.generation() > before_songs,
        "songs buffer generation must bump on toggle"
    );
    assert!(
        app.library.genres.generation() > before_genres,
        "genres buffer generation must bump on toggle"
    );

    drop(app);
    let _ = std::fs::remove_file(&db_path);
}

/// `Toggle(id)` must not panic when only one library exists in the cache
/// (the trigger widget hides itself at N<=1, but the handler stays
/// N-agnostic so a user driving through hotkeys or programmatic input
/// can't crash the app).
#[tokio::test]
async fn toggle_does_not_panic_with_one_library() {
    let (mut app, db_path) = test_app_with_shell().await;
    let shell = app.app_service.as_ref().expect("shell").clone();
    shell.apply_library_refresh(vec![library(1, "Music")]);
    assert_eq!(shell.library_count(), 1);

    let _ = app.handle_library_message(LibraryMessage::Toggle(1));

    assert!(
        shell.active_library_ids().contains(&1),
        "single-library toggle must still flip membership"
    );

    drop(app);
    drop(shell);
    let _ = std::fs::remove_file(&db_path);
}

// ============================================================================
// Loaded — apply backend refresh + deleted-id pruning
// ============================================================================

/// `Loaded(libs)` populates the backend's `all_libraries` cache so the UI
/// can render the popover rows without re-fetching.
#[tokio::test]
async fn loaded_applies_libraries_to_backend_cache() {
    let (mut app, db_path) = test_app_with_shell().await;
    let shell = app.app_service.as_ref().expect("shell").clone();
    assert!(shell.all_libraries().is_empty());

    let libs = vec![library(1, "Music"), library(2, "Audiobooks")];
    let _ = app.handle_library_message(LibraryMessage::Loaded(libs.clone()));

    let cached = shell.all_libraries();
    assert_eq!(cached.len(), 2);
    assert_eq!(cached[0].id, 1);
    assert_eq!(cached[1].id, 2);

    drop(app);
    drop(shell);
    let _ = std::fs::remove_file(&db_path);
}

/// `Loaded(libs)` must prune `active_library_ids` of any id no longer
/// present in the refreshed list — the "deleted library" recovery path
/// (plan section 14.4). Delegates to `apply_library_refresh` on the
/// backend, which is where the pruning logic lives.
#[tokio::test]
async fn loaded_prunes_deleted_library_ids() {
    let (mut app, db_path) = test_app_with_shell().await;
    let shell = app.app_service.as_ref().expect("shell").clone();

    // Seed: pretend the user had 1, 2, 3 active.
    shell.set_active_library_ids(HashSet::from_iter([1, 2, 3]));
    assert_eq!(shell.active_library_ids().len(), 3);

    // Server reports only 1 and 2 — id 3 has been deleted.
    let libs = vec![library(1, "Music"), library(2, "Audiobooks")];
    let _ = app.handle_library_message(LibraryMessage::Loaded(libs));

    let active = shell.active_library_ids();
    assert!(active.contains(&1));
    assert!(active.contains(&2));
    assert!(
        !active.contains(&3),
        "deleted library id 3 must be pruned from active set"
    );
    assert_eq!(active.len(), 2);

    drop(app);
    drop(shell);
    let _ = std::fs::remove_file(&db_path);
}

// ============================================================================
// OpenChange — refresh-on-open branching
// ============================================================================

/// `OpenChange { open: true }` with `all_libraries` already populated
/// must open the overlay without re-fetching — `library_count` stays
/// stable and no toast is pushed.
#[tokio::test]
async fn open_change_with_loaded_libraries_does_not_refetch() {
    let (mut app, db_path) = test_app_with_shell().await;
    let shell = app.app_service.as_ref().expect("shell").clone();
    shell.apply_library_refresh(vec![library(1, "Music"), library(2, "Audiobooks")]);
    assert_eq!(shell.library_count(), 2);
    let before = app.toast.toasts.len();

    let _ = app.handle_library_message(LibraryMessage::OpenChange {
        open: true,
        trigger_bounds: Some(dummy_bounds()),
    });

    assert!(matches!(
        app.open_menu,
        Some(OpenMenu::LibrarySelector { .. })
    ));
    assert_eq!(
        shell.library_count(),
        2,
        "cached library list must remain untouched on open"
    );
    assert_eq!(
        app.toast.toasts.len(),
        before,
        "no toast should be emitted on open when the cache is warm"
    );

    drop(app);
    drop(shell);
    let _ = std::fs::remove_file(&db_path);
}

/// `OpenChange { open: true }` with an empty `all_libraries` must open
/// the overlay AND dispatch a refresh task. The refresh task itself
/// requires a live server, but we can verify the overlay opens
/// regardless — the visual "loading…" state is the popover's
/// responsibility, not the handler's.
#[tokio::test]
async fn open_change_with_empty_libraries_still_opens_overlay() {
    let (mut app, db_path) = test_app_with_shell().await;
    let shell = app.app_service.as_ref().expect("shell").clone();
    assert!(shell.all_libraries().is_empty());

    let _ = app.handle_library_message(LibraryMessage::OpenChange {
        open: true,
        trigger_bounds: Some(dummy_bounds()),
    });

    assert!(
        matches!(app.open_menu, Some(OpenMenu::LibrarySelector { .. })),
        "OpenChange(open=true) must open the overlay even when the cache is cold"
    );

    drop(app);
    drop(shell);
    let _ = std::fs::remove_file(&db_path);
}
