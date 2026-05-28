//! Tests for update handlers.
//!
//! Covers pure-state-mutation handlers that don't require app_service or async.
//! Sub-modules group tests by the handler family they exercise; some files mix
//! a few related concerns where the original single-file section markers had
//! drifted (e.g. `general.rs` covers server version, light mode, task manager,
//! radios, and auth).

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
mod session;
mod settings;
mod settings_sidebar;
mod settings_slider;
mod slot_list;
mod star_rating;
mod state;
mod window;
