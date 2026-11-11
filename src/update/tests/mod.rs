//! Tests for update handlers.
//!
//! Covers pure-state-mutation handlers that don't require app_service or async.
//! Sub-modules group tests by the handler family they exercise; some files mix
//! a few related concerns where the original single-file section markers had
//! drifted (e.g. `general.rs` covers server version, light mode, task manager,
//! radios, and auth).

/// Take the process-global SSE connection slot for a test that reads it back
/// or needs it steady ([`crate::services::navidrome_sse::own_slot_for_test`]).
pub(super) fn sse_slot_guard() -> crate::services::navidrome_sse::SlotTestGuard {
    crate::services::navidrome_sse::own_slot_for_test()
}

mod artist_info;
mod artist_recs;
mod artwork;
mod artwork_064;
mod artwork_drag;
mod autohide_toolbar;
mod background_reload;
mod boat;
mod components;
mod cross_pane_drag;
mod default_playlist_picker;
mod discography;
mod dynamic_accent;
mod editor;
mod focused_row;
mod frame;
mod general;
mod harbour;
mod hotkeys;
mod ipc;
mod library;
mod library_refresh;
mod lyrics;
mod menus;
mod milkdrop;
mod milkdrop_picker;
mod modals;
mod mpris;
mod panel_art;
mod placeholders;
#[macro_use]
mod navigation_macros;
mod navigation;
mod notifications;
mod page_loader;
mod playback;
mod playlist_artwork;
mod playlists;
mod queue;
mod radio_artwork;
mod redesign_chrome;
mod remote;
mod roulette;
mod row_menus;
mod rules_editor;
mod scrobble;
mod session;
mod settings;
mod settings_sidebar;
mod settings_slider;
mod similar_artists_modal;
mod slot_list;
mod split_view;
mod star_rating;
mod state;
mod theater;
mod track_artwork;
mod trawl_menu;
mod trawl_modal;
mod visualizer;
mod waveform;
mod window;
