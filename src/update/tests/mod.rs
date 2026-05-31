//! Tests for update handlers.
//!
//! Covers pure-state-mutation handlers that don't require app_service or async.
//! Sub-modules group tests by the handler family they exercise; some files mix
//! a few related concerns where the original single-file section markers had
//! drifted (e.g. `general.rs` covers server version, light mode, task manager,
//! radios, and auth).

/// Serializes every test that touches the process-global `SSE_CONNECTION_INFO`
/// slot — whether directly via `navidrome_sse::{register,clear}` or indirectly
/// via `reset_session_state()` (which calls `navidrome_sse::clear()`). All such
/// tests flip the same shared static, so under parallel execution they race: a
/// concurrent `clear()` empties the slot between another test's `register()` and
/// its `slot_is_set()` assertion. Holding this lock for the full duration of
/// each such test forces them to run one at a time. `unwrap_or_else(|e|
/// e.into_inner())` recovers a poisoned lock so a panic in one test does not
/// cascade into the others. Mirrors the `OVERLAY_TEST_LOCK` pattern in
/// `settings.rs` for the albums-artwork-overlay atomic.
pub(super) static SSE_SLOT_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

mod artwork;
mod artwork_drag;
mod boat;
mod components;
mod cross_pane_drag;
mod default_playlist_picker;
mod editor;
mod general;
mod hotkeys;
mod ipc;
mod library;
mod library_refresh;
mod menus;
mod mpris;
#[macro_use]
mod navigation_macros;
mod navigation;
mod page_loader;
mod playback;
mod queue;
mod redesign_chrome;
mod roulette;
mod scrobble;
mod session;
mod settings;
mod settings_sidebar;
mod settings_slider;
mod slot_list;
mod split_view;
mod star_rating;
mod state;
mod window;
