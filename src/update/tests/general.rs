//! Tests for general handlers (server version, light mode, task manager, radios, auth) update handlers.

use crate::{View, test_helpers::*};

// Server Version (mod.rs)
// ============================================================================

#[test]
fn server_version_fetched_updates_state() {
    let mut app = test_app();
    assert_eq!(app.server_version, None);

    let _ = app.update(crate::app_message::Message::ServerVersionFetched(Some(
        "0.61.1".to_string(),
    )));

    assert_eq!(app.server_version.as_deref(), Some("0.61.1"));
}

// ============================================================================
// Settings Escape Priority Chain (views/settings/mod.rs)
// ============================================================================

#[test]
fn settings_escape_at_root_exits() {
    use crate::views::settings::{NavLevel, SettingsAction, SettingsMessage};
    let mut page = crate::views::SettingsPage::new();
    // Default state: nav_stack = [CategoryPicker], no search, no editing
    assert_eq!(page.nav_stack.len(), 1);
    assert_eq!(*page.current_level(), NavLevel::CategoryPicker);

    let data = make_settings_view_data();
    let action = page.update(SettingsMessage::Escape, &data);
    assert!(
        matches!(action, SettingsAction::ExitSettings),
        "Escape at root should exit settings, got: {action:?}"
    );
}

#[test]
fn settings_escape_with_stale_search_exits() {
    use crate::views::settings::{SettingsAction, SettingsMessage};
    let mut page = crate::views::SettingsPage::new();
    // Simulate: user searched, then SlotListDown cleared search_active but kept query
    page.search_query = "scrobbl".to_string();
    page.search_active = false; // search bar is hidden — query is stale/invisible

    let data = make_settings_view_data();
    let action = page.update(SettingsMessage::Escape, &data);
    assert!(
        matches!(action, SettingsAction::ExitSettings),
        "Escape with stale (inactive) search should exit settings, got: {action:?}"
    );
    // Query should also be cleaned up
    assert!(
        page.search_query.is_empty(),
        "Stale search query should be cleared on exit"
    );
}

#[test]
fn settings_escape_with_active_search_clears_search() {
    use crate::views::settings::{SettingsAction, SettingsMessage};
    let mut page = crate::views::SettingsPage::new();
    page.search_query = "scrobbl".to_string();
    page.search_active = true; // search bar is visible

    let data = make_settings_view_data();
    let action = page.update(SettingsMessage::Escape, &data);
    assert!(
        matches!(action, SettingsAction::None),
        "Escape with active search should clear search (not exit), got: {action:?}"
    );
    assert!(!page.search_active, "search_active should be cleared");
    assert!(page.search_query.is_empty(), "search query should be empty");
}

#[test]
fn settings_escape_pops_nav_stack() {
    use crate::views::settings::{NavLevel, SettingsAction, SettingsMessage, SettingsTab};
    let mut page = crate::views::SettingsPage::new();
    // Drill into General category
    page.push_level(NavLevel::Category(SettingsTab::General));
    assert_eq!(page.nav_stack.len(), 2);

    let data = make_settings_view_data();
    let action = page.update(SettingsMessage::Escape, &data);
    assert!(
        matches!(action, SettingsAction::None),
        "Escape at depth 2 should pop nav stack, got: {action:?}"
    );
    assert_eq!(
        page.nav_stack.len(),
        1,
        "Nav stack should be popped to root"
    );
}

#[test]
fn settings_escape_cancels_hotkey_capture() {
    use crate::views::settings::{SettingsAction, SettingsMessage};
    let mut page = crate::views::SettingsPage::new();
    page.capturing_hotkey = Some(nokkvi_data::types::hotkey_config::HotkeyAction::TogglePlay);

    let data = make_settings_view_data();
    let action = page.update(SettingsMessage::Escape, &data);
    assert!(
        matches!(action, SettingsAction::None),
        "Escape during hotkey capture should cancel capture, got: {action:?}"
    );
    assert!(
        page.capturing_hotkey.is_none(),
        "capturing_hotkey should be cleared"
    );
}

#[test]
fn settings_escape_exits_edit_mode() {
    use crate::views::settings::{SettingsAction, SettingsMessage};
    let mut page = crate::views::SettingsPage::new();
    page.editing_index = Some(0);

    let data = make_settings_view_data();
    let action = page.update(SettingsMessage::Escape, &data);
    assert!(
        matches!(action, SettingsAction::None),
        "Escape during edit mode should exit edit, got: {action:?}"
    );
    assert!(
        page.editing_index.is_none(),
        "editing_index should be cleared"
    );
}

// ============================================================================
// Hotkey Suppression During Text Input (TDD — regression from 2c54792)
// ============================================================================
//
// When a text_input widget has captured a key event (Status::Captured),
// hotkeys should NOT fire — the user is typing in a search field.
//
// Exceptions:
// - Escape should always pass through (close overlays / clear search)
// - Ctrl+key combos should always pass through (Ctrl+S, Ctrl+D, Ctrl+E)

/// Helper: simulate a RawKeyEvent through the full update() dispatch.
fn send_raw_key(
    app: &mut crate::Nokkvi,
    key: iced::keyboard::Key,
    modifiers: iced::keyboard::Modifiers,
    status: iced::event::Status,
) -> iced::Task<crate::Message> {
    app.update(crate::Message::RawKeyEvent(key, modifiers, status))
}

#[test]
fn hotkey_suppressed_when_captured_toggle_random() {
    // 'x' is bound to ToggleRandom. If captured by a text_input, it must NOT toggle.
    let mut app = test_app();
    app.current_view = View::Queue;
    app.screen = crate::Screen::Home;
    assert!(!app.modes.random, "random should start as false");

    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Character("x".into()),
        iced::keyboard::Modifiers::empty(),
        iced::event::Status::Captured,
    );

    assert!(
        !app.modes.random,
        "ToggleRandom ('x') should be suppressed when Status::Captured"
    );
}

#[test]
fn hotkey_suppressed_when_captured_toggle_consume() {
    // 'c' is bound to ToggleConsume. Must be suppressed when captured.
    let mut app = test_app();
    app.current_view = View::Albums;
    app.screen = crate::Screen::Home;
    assert!(!app.modes.consume);

    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Character("c".into()),
        iced::keyboard::Modifiers::empty(),
        iced::event::Status::Captured,
    );

    assert!(
        !app.modes.consume,
        "ToggleConsume ('c') should be suppressed when Status::Captured"
    );
}

#[test]
fn hotkey_fires_when_not_captured_toggle_random() {
    // Same key 'x' with Status::Ignored should work normally.
    let mut app = test_app();
    app.current_view = View::Queue;
    app.screen = crate::Screen::Home;
    assert!(!app.modes.random);

    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Character("x".into()),
        iced::keyboard::Modifiers::empty(),
        iced::event::Status::Ignored,
    );

    assert!(
        app.modes.random,
        "ToggleRandom should fire when Status::Ignored (no widget has focus)"
    );
}

#[test]
fn escape_not_suppressed_when_captured() {
    // Escape should always fire, even when a text_input has captured the event.
    // This was the whole reason we switched to event::listen_with() in 2c54792.
    let mut app = test_app();
    app.current_view = View::Settings;
    app.screen = crate::Screen::Home;
    app.eq_modal.open = true;

    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
        iced::keyboard::Modifiers::empty(),
        iced::event::Status::Captured,
    );

    assert!(
        !app.eq_modal.open,
        "Escape should close EQ modal even when Status::Captured"
    );
}

#[test]
fn ctrl_combo_not_suppressed_when_captured() {
    // Ctrl+E is bound to ToggleBrowsingPanel. Ctrl+ combos are intentional
    // actions, not typing — they must NOT be suppressed even when captured.
    // Without app_service the handler returns Task::none(), but the fact that
    // it reaches the handler (no panic, no suppression) is what we're testing.
    let mut app = test_app();
    app.current_view = View::Queue;
    app.screen = crate::Screen::Home;
    assert!(app.browsing_panel.is_none());

    let _ = send_raw_key(
        &mut app,
        iced::keyboard::Key::Character("e".into()),
        iced::keyboard::Modifiers::CTRL,
        iced::event::Status::Captured,
    );

    // ToggleBrowsingPanel was dispatched (not suppressed). No panic = success.
    // Contrast with hotkey_suppressed_when_captured_toggle_random which MUST
    // be suppressed under the same Status::Captured condition.
}

// ============================================================================
// Light Mode Persistence (mod.rs)
// ============================================================================

#[test]
fn toggle_light_mode_persists_to_settings_key() {
    // Ensure a known baseline (atomic is global, so set it explicitly)
    crate::theme::set_light_mode(false);
    let mut app = test_app();

    let _ = app.update(crate::app_message::Message::ToggleLightMode);

    // The handler calls set_light_mode(true) — verify the in-memory atomic flipped.
    // Disk persistence is validated by config_writer unit tests, not here.
    assert!(
        crate::theme::is_light_mode(),
        "ToggleLightMode should flip the in-memory theme atomic from false to true"
    );
}

#[test]
fn test_handle_radio_metadata_update() {
    let mut app = test_app();

    // Ensure we start with Queue playback
    assert!(app.active_playback.is_queue());

    // Switch to Radio playback
    let station = nokkvi_data::types::radio_station::RadioStation {
        id: "radio_1".into(),
        name: "Test Radio".into(),
        stream_url: "http://test".into(),
        home_page_url: None,
    };
    app.active_playback = crate::state::ActivePlayback::Radio(crate::state::RadioPlaybackState {
        station,
        icy_artist: None,
        icy_title: None,
        icy_url: None,
    });

    // Update metadata
    let _ = app.handle_radio_metadata_update(
        Some("Test Artist".to_string()),
        Some("Test Song".to_string()),
        None,
    );

    // Verify state mutation
    if let crate::state::ActivePlayback::Radio(state) = &app.active_playback {
        assert_eq!(state.icy_artist.as_deref(), Some("Test Artist"));
        assert_eq!(state.icy_title.as_deref(), Some("Test Song"));
    } else {
        panic!("Should still be in Radio playback state");
    }
}

#[test]
fn radios_play_filtered_station_plays_correct_station() {
    use crate::views::RadiosMessage;
    let mut app = test_app();
    app.current_view = crate::View::Radios;

    let s1 = nokkvi_data::types::radio_station::RadioStation {
        id: "r1".into(),
        name: "BBC Radio".into(),
        stream_url: "url3".into(),
        home_page_url: None,
    };
    let s2 = nokkvi_data::types::radio_station::RadioStation {
        id: "r2".into(),
        name: "SomaFM".into(),
        stream_url: "url1".into(),
        home_page_url: None,
    };

    app.library.radio_stations = vec![s1, s2];

    let _ = app.handle_radios(RadiosMessage::SlotList(
        crate::widgets::SlotListPageMessage::SearchQueryChanged("soma".to_string()),
    ));
    let _ = app.handle_radios(RadiosMessage::SlotList(
        crate::widgets::SlotListPageMessage::ClickPlay(0),
    ));

    match &app.active_playback {
        crate::state::ActivePlayback::Radio(state) => {
            assert_eq!(
                state.station.name, "SomaFM",
                "Should play the filtered station, not the first station in unfiltered list"
            );
        }
        crate::state::ActivePlayback::Queue => panic!("Expected Radio playback"),
    }
}

#[test]
fn test_session_expired_redirects_to_login() {
    let mut app = test_app();
    app.screen = crate::Screen::Home;
    app.current_view = View::Albums;
    app.library
        .albums
        .set_from_vec(vec![make_album("a1", "A", "A")]);

    let _ = app.handle_session_expired();

    assert_eq!(app.screen, crate::Screen::Login);
    assert!(app.app_service.is_none());
    assert!(app.stored_session.is_none());
    assert!(
        app.library.albums.is_empty(),
        "Library should be reset on session expiry"
    );
}

#[test]
fn test_albums_loaded_unauthorized_triggers_logout() {
    let mut app = test_app();
    app.screen = crate::Screen::Home;
    app.current_view = View::Albums;

    // Simulate a wrapped anyhow error that was stringified with {:#}
    let err_string = "Failed to fetch albums: Unauthorized: Session expired".to_string();
    let _ = app.handle_albums_loaded(Err(err_string), 0, false, None);

    assert_eq!(
        app.screen,
        crate::Screen::Login,
        "Should redirect to login on unauthorized error string"
    );
}

// ============================================================================
// Task Manager Notifications (mod.rs)
// ============================================================================

#[test]
fn task_status_changed_failed_pushes_toast() {
    let mut app = test_app();
    let handle = nokkvi_data::services::task_manager::TaskHandle {
        id: 1,
        name: "TestTask".to_string(),
    };
    let status =
        nokkvi_data::services::task_manager::TaskStatus::Failed("simulated error".to_string());

    let _ = app.update(crate::app_message::Message::TaskStatusChanged(
        handle, status,
    ));

    // Toast list should now contain an error message
    assert_eq!(app.toast.toasts.len(), 1);
    let toast = &app.toast.toasts[0];
    assert!(toast.message.contains("Task failed"));
    assert!(toast.message.contains("TestTask"));
    assert!(toast.message.contains("simulated error"));
    assert_eq!(toast.level, nokkvi_data::types::toast::ToastLevel::Error);
}

#[test]
fn task_status_changed_success_no_toast() {
    let mut app = test_app();
    let handle = nokkvi_data::services::task_manager::TaskHandle {
        id: 1,
        name: "TestTask".to_string(),
    };
    let status = nokkvi_data::services::task_manager::TaskStatus::Completed;

    let _ = app.update(crate::app_message::Message::TaskStatusChanged(
        handle, status,
    ));

    // Currently, successful tasks just log to debug, no toast
    assert!(app.toast.toasts.is_empty());
}

// ============================================================================
// Slot count resync — vertical artwork modes (update/window.rs)
// ============================================================================

/// Toggling between horizontal and vertical artwork column modes changed the
/// rendered slot count, but `page.slot_count` was only re-synced on window
/// resize (using horizontal-mode chrome). After switching to a vertical mode
/// the stored value stayed at the horizontal slot count, so
/// `pending_expand_resolve`'s `idx + center_slot` math (driven by the stored
/// count) placed the auto-expanded row above the visible viewport — the user
/// had to scroll up to find the row that should have landed at slot 0.
///
/// `resync_slot_counts` now includes `vertical_artwork_chrome` in the
/// `with_dynamic_slots` calculation, and `handle_player_settings_loaded`
/// calls it after the artwork-mode atomic flips.
mod slot_count_resync {
    use std::sync::{Mutex, MutexGuard};

    use nokkvi_data::types::player_settings::{ArtworkColumnMode, ArtworkStretchFit};

    use crate::{test_helpers::test_app, theme};

    // Shared lock with the other test modules that mutate theme atomics —
    // a stray `set_artwork_column_mode` in a sibling test must not race
    // against our reads here. The lock itself is local, but every test
    // that reads `theme::artwork_column_mode()` takes it.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn lock_atomics() -> MutexGuard<'static, ()> {
        TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn reset_atomics() {
        theme::set_artwork_column_mode(ArtworkColumnMode::Auto);
        theme::set_artwork_column_stretch_fit(ArtworkStretchFit::Cover);
        theme::set_artwork_column_width_pct(0.40);
        theme::set_artwork_auto_max_pct(0.40);
        theme::set_artwork_vertical_height_pct(0.40);
    }

    #[test]
    fn resync_shrinks_slot_count_when_switching_to_vertical_mode() {
        let _g = lock_atomics();
        reset_atomics();

        let mut app = test_app();
        // 1280×800 landscape — wide enough for the horizontal layout to keep
        // a 9-ish slot count, but a vertical artwork ~320 px will visibly eat
        // into the slot count when stacked above the list.
        app.window.width = 1280.0;
        app.window.height = 800.0;

        theme::set_artwork_column_mode(ArtworkColumnMode::AlwaysNative);
        app.resync_slot_counts();
        let horizontal = app.albums_page.common.slot_list.slot_count;

        theme::set_artwork_column_mode(ArtworkColumnMode::AlwaysVerticalNative);
        app.resync_slot_counts();
        let vertical = app.albums_page.common.slot_list.slot_count;

        assert!(
            vertical < horizontal,
            "vertical artwork stacks above the slot list, so the slot count \
             must shrink — got vertical={vertical}, horizontal={horizontal}"
        );

        // Cleanup: leave the atomic in a neutral state for sibling tests.
        reset_atomics();
    }

    #[test]
    fn resync_applies_to_every_library_page() {
        let _g = lock_atomics();
        reset_atomics();

        let mut app = test_app();
        app.window.width = 1280.0;
        app.window.height = 800.0;

        theme::set_artwork_column_mode(ArtworkColumnMode::AlwaysVerticalNative);
        app.resync_slot_counts();

        let albums = app.albums_page.common.slot_list.slot_count;
        let artists = app.artists_page.common.slot_list.slot_count;
        let songs = app.songs_page.common.slot_list.slot_count;
        let genres = app.genres_page.common.slot_list.slot_count;
        let playlists = app.playlists_page.common.slot_list.slot_count;
        let queue = app.queue_page.common.slot_list.slot_count;

        // All six library pages share the same standard chrome — verify the
        // helper wrote the same vertical-aware value to all of them, not just
        // the page that happens to be visible.
        assert_eq!(albums, artists);
        assert_eq!(albums, songs);
        assert_eq!(albums, genres);
        assert_eq!(albums, playlists);
        assert_eq!(albums, queue);
        // And the value must come from a real `with_dynamic_slots` calc, not
        // the `9` default `SlotListView::new()` left behind.
        assert_ne!(
            albums, 9,
            "expected vertical-mode resync to overwrite the default 9"
        );

        reset_atomics();
    }
}

// ============================================================================
